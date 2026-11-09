//! The changes of a database's numbered transactions, read back from its
//! ship log ([`crate::storage::ship`]) so that a database of any shape can
//! write them again: whole rows, deletes by key, numbers set in place,
//! changes of tables and of rank profiles, and stream sources' offsets,
//! each with the schema its table had when it was written. chloedb-server
//! serves them as a cluster's feed, which another cluster follows.
//!
//! The log holds a row as its fields' values in the order of its table's
//! schema at the time, which an `ALTER TABLE` changes. A [`Cursor`] keeps
//! every table's schema as of the transaction it reads next: it starts from
//! the tables as of the log's last transaction, undoes the changes of tables
//! the log holds from the cursor's number on, and then applies each change
//! as it reads past it. The log names a drop by the table's id alone, so
//! the database keeps the schemas of the tables it dropped since it
//! opened (the last 256), and a cursor from before a drop reads the
//! table's rows and its drop by them. A table dropped before that, or
//! before the database opened, has no schema left to read its rows by:
//! its operations are counted ([`Changes::unknown`]), not read, as the
//! table is gone anyway (table ids are never given again).

use std::sync::Arc;

use crate::error::{Error, Result};
use crate::schema::Schema;
use crate::storage::manifest::{system_schema, RankProfile, SeqNo, SOURCES_ID, SYSTEM_IDS};
use crate::storage::ship::LogRead;
use crate::storage::wal::{self, Op};
use crate::storage::Database;
use crate::value::{Document, Value};

/// One change a transaction made, in the order it made them.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// A row written whole, in place of any row with its key: an insert, an
    /// upsert, or a field other than a number set (which writes the row
    /// again).
    Upsert { table: Arc<Schema>, doc: Arc<Document> },
    /// The row with this key deleted. The delete an upsert logs of the row
    /// it replaces is left out, but in a table placed by a `shard_by`
    /// field, where the new row may be placed apart from the old one.
    Delete { table: Arc<Schema>, key: Value },
    /// A number set in place: field `field` of `table` (its index there) of
    /// the row with this key.
    Set { table: Arc<Schema>, key: Value, field: usize, value: Value },
    /// The table's rank profile named `name` made, replaced, or dropped
    /// (`None`).
    Profile { table: Arc<Schema>, name: String, def: Option<Arc<RankProfile>> },
    Create { def: Arc<Schema> },
    Alter { was: Arc<Schema>, def: Arc<Schema> },
    Drop { was: Arc<Schema> },
    /// The offset of a stream source, as `_sources` keeps it
    /// ([`crate::storage::WriteTxn::set_source`]), or a mark that it does
    /// not keep ([`crate::storage::WriteTxn::mark_source`]).
    Source { name: String, offset: i64 },
}

/// What one transaction changed.
#[derive(Clone, Debug, PartialEq)]
pub struct Changes {
    pub seq: SeqNo,
    pub changes: Vec<Change>,
    /// Its operations on tables dropped before the cursor's first number,
    /// which have no schema left to read them by.
    pub unknown: usize,
}

/// Where a reader of a database's changes is: the number it reads next,
/// and every table's schema as of then.
#[derive(Clone, Debug)]
pub struct Cursor {
    next: u64,
    tables: Vec<(u32, Arc<Schema>)>,
}

/// Where [`Database::changes_from`] starts.
#[derive(Clone, Debug)]
pub enum Start {
    At(Cursor),
    /// The ship log no longer holds the number asked for: it holds them
    /// from `oldest` on.
    Gone { oldest: u64 },
    /// The reader's history and this database's differ: the reader holds
    /// a transaction this database lacks, or one of another term. `seq` is
    /// this database's last number and `term` its term.
    Diverged { seq: u64, term: u64 },
}

/// What [`Database::read_changes`] found.
#[derive(Clone, Debug, PartialEq)]
pub enum Read {
    /// The changes of the transactions from the cursor's number on, in
    /// order (none when the database has not given that number yet).
    Txns(Vec<Changes>),
    /// The ship log no longer holds the cursor's number.
    Gone { oldest: u64 },
}

impl Cursor {
    /// The number of the transaction it reads next.
    pub fn next(&self) -> u64 {
        self.next
    }

    /// The schema of the table with id `id` as of [`Cursor::next`].
    pub fn table(&self, id: u32) -> Option<&Arc<Schema>> {
        self.tables.iter().find(|(i, _)| *i == id).map(|(_, s)| s)
    }

    /// The changes of transaction `seq`, whose operations are `ops`, and
    /// the tables as they leave them.
    fn read(&mut self, seq: SeqNo, ops: Vec<(u32, Op)>) -> Changes {
        let mut changes: Vec<Change> = Vec::with_capacity(ops.len());
        let mut unknown = 0;
        for (id, op) in ops {
            match op {
                Op::Table { def: Some(def) } => {
                    set(&mut self.tables, id, def.clone());
                    changes.push(Change::Create { def });
                }
                Op::Table { def: None } => match self.tables.iter().position(|(i, _)| *i == id) {
                    Some(p) => changes.push(Change::Drop { was: self.tables.remove(p).1 }),
                    None => unknown += 1,
                },
                Op::Alter { was, def } => {
                    set(&mut self.tables, id, def.clone());
                    changes.push(Change::Alter { was, def });
                }
                op => {
                    let Some(table) = self.table(id).cloned() else {
                        unknown += 1;
                        continue;
                    };
                    if id == SOURCES_ID {
                        // An offset written is the source's row again; the
                        // delete of the row before it says nothing more.
                        if let Op::Insert(doc) = &op {
                            if let (Some(Value::Str(name)), Some(offset)) = (doc.first(), doc.get(1).and_then(Value::as_i64)) {
                                changes.push(Change::Source { name: name.clone(), offset });
                            }
                        }
                        continue;
                    }
                    match op {
                        Op::Insert(doc) => {
                            // An upsert logs first the delete of the row it
                            // replaces, which writing the row again does.
                            let replaces = match (table.primary_key, changes.last()) {
                                (Some(pk), Some(Change::Delete { table: t, key })) => table.shard_by.is_none() && Arc::ptr_eq(t, &table) && doc.get(pk) == Some(key),
                                _ => false,
                            };
                            if replaces {
                                changes.pop();
                            }
                            changes.push(Change::Upsert { table, doc });
                        }
                        Op::Delete(key) => changes.push(Change::Delete { table, key }),
                        Op::Set { key, field, value } => changes.push(Change::Set { table, key, field: field as usize, value }),
                        Op::Profile { name, def } => changes.push(Change::Profile { table, name, def }),
                        Op::Table { .. } | Op::Alter { .. } => unreachable!("matched above"),
                    }
                }
            }
        }
        self.next = seq.seq + 1;
        Changes { seq, changes, unknown }
    }
}

/// Gives table `id` the schema `s`.
fn set(tables: &mut Vec<(u32, Arc<Schema>)>, id: u32, s: Arc<Schema>) {
    match tables.iter_mut().find(|(i, _)| *i == id) {
        Some((_, t)) => *t = s,
        None => tables.push((id, s)),
    }
}

fn not_numbered() -> Error {
    Error::Schema("the database does not number its writes (Database::start_seq_no), so it has no changes to read back".into())
}

impl Database {
    /// A cursor at transaction number `from` (the first to read; 0 reads
    /// from 1), with every table's schema as of then. `after` is the term
    /// of transaction `from - 1` as the reader holds it, if it holds one:
    /// when this database has not given that number, or gave it under
    /// another term, their histories differ ([`Start::Diverged`]), as they
    /// do when a primary the reader read from lost transactions in a
    /// failover. [`Start::Gone`] when the ship log no longer holds `from`.
    pub fn changes_from(&self, from: u64, after: Option<u64>) -> Result<Start> {
        let from = from.max(1);
        let (mut tables, dropped, n, read) = self.log_with_tables(from, usize::MAX).ok_or_else(not_numbered)?;
        let prev = from - 1;
        if prev > n.seq || (prev > 0 && after.is_some_and(|t| n.term_at(prev) != t)) {
            let last = n.last();
            return Ok(Start::Diverged { seq: last.seq, term: last.term });
        }
        let txns = match read {
            LogRead::Txns(t) => t,
            LogRead::Gone { oldest } => return Ok(Start::Gone { oldest }),
        };
        // A replica makes its own when its primary's first offset (or
        // function) arrives.
        for sys in SYSTEM_IDS {
            if !tables.iter().any(|(id, _)| *id == sys) {
                tables.push((sys, Arc::new(system_schema(sys).expect("a system table"))));
            }
        }
        // Back from the log's last transaction to `from`.
        for t in txns.iter().rev().filter(|t| wal::ships_table_changes(&t.bytes)) {
            let w = wal::decode_shipped(&t.bytes, &|_, _| true)?;
            for (id, op) in w.ops.iter().rev() {
                match op {
                    Op::Alter { was, .. } => set(&mut tables, *id, was.clone()),
                    // Dropped since: as it was, if the database kept it
                    // (the log names a drop by id alone).
                    Op::Table { def: None } => match dropped.iter().rev().find(|(i, _)| i == id) {
                        Some((_, was)) => set(&mut tables, *id, was.clone()),
                        None => tables.retain(|(i, _)| i != id),
                    },
                    // Made since.
                    Op::Table { def: Some(_) } => tables.retain(|(i, _)| i != id),
                    _ => {}
                }
            }
        }
        Ok(Start::At(Cursor { next: from, tables }))
    }

    /// The changes of the transactions from `cursor`'s number on, up to
    /// number `upto` and `max_bytes` of them as logged (and at least one),
    /// moving the cursor past them.
    pub fn read_changes(&self, cursor: &mut Cursor, max_bytes: usize, upto: u64) -> Result<Read> {
        if self.numbering().is_none() {
            return Err(not_numbered());
        }
        if cursor.next > upto {
            return Ok(Read::Txns(Vec::new()));
        }
        let txns = match self.log_from(cursor.next, max_bytes) {
            LogRead::Txns(t) => t,
            LogRead::Gone { oldest } => return Ok(Read::Gone { oldest }),
        };
        let mut out = Vec::with_capacity(txns.len());
        for t in txns.into_iter().take_while(|t| t.seq.seq <= upto) {
            let w = wal::decode_shipped(&t.bytes, &|_, _| true)?;
            out.push(cursor.read(t.seq, w.ops));
        }
        Ok(Read::Txns(out))
    }
}
