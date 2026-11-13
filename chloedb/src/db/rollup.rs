//! Rollups kept up to date ([`crate::query::rollup`]): `CREATE ROLLUP`,
//! `REFRESH ROLLUP`, `DROP ROLLUP`, and the background job that refreshes
//! them.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use super::{Db, SqlResult, WriteOp};
use crate::error::{Error, Result};
use crate::query::exec::{ExecOptions, ExecStats, ResultSet};
use crate::query::rollup::{self, Rollup, Statement, KEY};
use crate::query::{sql as qsql, Params};
use crate::schema::Schema;
use crate::value::Value;

/// What one refresh of a rollup did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RefreshStats {
    /// Buckets whose counts it compared with the table's.
    pub checked: u64,
    /// Buckets it folded again, their counts having changed.
    pub folded: u64,
    /// Rows of the rollup it wrote or deleted.
    pub rows_written: u64,
    pub seconds: f64,
}

impl RefreshStats {
    fn result_set(&self) -> ResultSet {
        let columns = ["buckets_checked", "buckets_folded", "rows_written", "seconds"].map(String::from).to_vec();
        let rows = vec![vec![Value::Int(self.checked as i64), Value::Int(self.folded as i64), Value::Int(self.rows_written as i64), Value::Float(self.seconds)]];
        ResultSet { columns, rows, keys: Vec::new(), scores: Vec::new(), parts: None, stats: ExecStats::default() }
    }
}

/// How often the background job checks every bucket, not only the
/// newest: every this many refreshes.
const SWEEP_EVERY: u64 = 60;

fn all_rows(opts: &ExecOptions) -> ExecOptions {
    ExecOptions { max_rows: None, max_groups: None, ..opts.clone() }
}

fn window(from: i64, to: i64) -> Params {
    Params::new().set("rollup_from", Value::Timestamp(from)).set("rollup_to", Value::Timestamp(to))
}

fn time_of(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::Timestamp(t) | Value::Int(t) => Some(*t),
        Value::Float(f) => Some(*f as i64),
        _ => None,
    }
}

fn int_of(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Int(n) | Value::Timestamp(n)) => *n,
        Some(Value::Float(f)) => *f as i64,
        _ => 0,
    }
}

/// `v` as a value of a column of SQL type `ty`.
fn fit(v: Value, ty: &str) -> Value {
    match (v, ty) {
        (Value::Float(f), "INT") if f.fract() == 0.0 => Value::Int(f as i64),
        (Value::Int(n) | Value::Timestamp(n), "TIMESTAMP") => Value::Timestamp(n),
        (Value::Float(f), "TIMESTAMP") => Value::Timestamp(f as i64),
        (Value::Int(n), "FLOAT") => Value::Float(n as f64),
        (Value::Int(n), "BOOL") => Value::Bool(n != 0),
        (v, _) => v,
    }
}

/// The rollup key of the row of bucket `bucket` and group `group`.
fn key_of(bucket: i64, group: &[Value]) -> String {
    let mut k = bucket.to_string();
    for v in group {
        k.push('\u{1f}');
        match v {
            Value::Null => k.push('\u{0}'),
            Value::Str(s) => k.push_str(s),
            Value::Int(n) | Value::Timestamp(n) => k.push_str(&n.to_string()),
            Value::Bool(b) => k.push_str(if *b { "1" } else { "0" }),
            Value::Float(f) => k.push_str(&f.to_string()),
            other => k.push_str(&format!("{other:?}")),
        }
    }
    k
}

impl Db {
    /// A `CREATE ROLLUP`, `REFRESH ROLLUP` or `DROP ROLLUP` statement.
    pub(super) fn rollup_statement(&self, st: Statement, opts: &ExecOptions) -> Result<SqlResult> {
        match st {
            Statement::Create { rollup: r, if_not_exists } => match self.create_rollup(&r, if_not_exists, opts)? {
                true => self.refresh(&r, false, opts).map(|s| SqlResult::Rows(s.result_set())),
                false => Ok(SqlResult::Written(0)),
            },
            Statement::Refresh { name, full } => {
                let r = self.rollup(&name)?.ok_or_else(|| Error::Query(format!("no rollup {name}")))?;
                self.refresh(&r, full, opts).map(|s| SqlResult::Rows(s.result_set()))
            }
            Statement::Drop { name, if_exists } => {
                let Some(r) = self.rollup(&name)? else {
                    return match if_exists {
                        true => Ok(SqlResult::Written(0)),
                        false => Err(Error::Query(format!("no rollup {name}"))),
                    };
                };
                self.sql_with(&format!("DROP FUNCTION {}", rollup::function_name(&r.name)), &Params::new(), opts)?;
                if self.storage.table(&r.name).is_some() {
                    self.drop_tables(&[&r.name], Default::default())?;
                }
                Ok(SqlResult::Written(0))
            }
        }
    }

    /// Makes rollup `r`'s table and keeps its definition, without folding
    /// any rows yet: whether it made it (not when `if_not_exists` and it
    /// exists). `CREATE ROLLUP` then folds the table's rows at once; a
    /// server leaves that to its refresher, which a long table can keep
    /// busy for minutes.
    pub fn create_rollup(&self, r: &Rollup, if_not_exists: bool, opts: &ExecOptions) -> Result<bool> {
        let base = self.table(&r.base)?;
        if self.functions()?.current(&rollup::function_name(&r.name)).is_some() {
            return match if_not_exists {
                true => Ok(false),
                false => Err(Error::Query(format!("rollup {} exists: DROP ROLLUP {} first", r.name, r.name))),
            };
        }
        if self.storage.table(&r.name).is_some() {
            return Err(Error::Query(format!("a table is named {}: a rollup is a table of its own", r.name)));
        }
        if base.table.starts_with('_') {
            return Err(Error::Query(format!("{} is a system table", base.table)));
        }
        self.sql_with(&r.create_table_sql(&base)?, &Params::new(), opts)?;
        if let Err(e) = self.sql_with(&rollup::function_sql(r), &Params::new(), opts) {
            let _ = self.drop_tables(&[&r.name], Default::default());
            return Err(e);
        }
        Ok(true)
    }

    /// The rollups this database keeps ([`crate::query::rollup`]).
    pub fn rollups(&self) -> Result<Vec<Rollup>> {
        let fs = self.functions()?;
        let mut out = Vec::new();
        for f in fs.currents().filter(|f| f.def.name.to_ascii_lowercase().starts_with(rollup::PREFIX)) {
            if let Some(r) = rollup::of_function(f)? {
                out.push(r);
            }
        }
        Ok(out)
    }

    /// Rollup `name`, if there is one.
    pub fn rollup(&self, name: &str) -> Result<Option<Rollup>> {
        match self.functions()?.current(&rollup::function_name(name)) {
            Some(f) => rollup::of_function(f),
            None => Ok(None),
        }
    }

    /// Brings rollup `name` up to date with its table (`REFRESH ROLLUP`).
    ///
    /// A bucket is folded again when the rows of the table it would count
    /// are not as many as its rows of the rollup count: a bucket the
    /// rollup has not yet folded, the newest buckets, which grow as rows
    /// arrive, and an older one that rows reached late. A refresh compares
    /// the buckets from the newest row less the rollup's lateness to the
    /// newest, and those the rollup has not folded yet; with `full`, every
    /// bucket. Counting a bucket's rows reads the time column's index, not
    /// the rows, so it takes well under a millisecond. Buckets older than
    /// the table's retention are not compared: the rollup keeps what it
    /// counted before the rows expired.
    ///
    /// A changed row that leaves a bucket's count as it was (an UPDATE, a
    /// delete and an insert into one bucket between two refreshes) is not
    /// seen until a fold of that bucket: rollups are for tables that rows
    /// are added to, such as logs.
    pub fn refresh_rollup(&self, name: &str, full: bool) -> Result<RefreshStats> {
        let r = self.rollup(name)?.ok_or_else(|| Error::Query(format!("no rollup {name}")))?;
        self.refresh(&r, full, &self.exec)
    }

    fn refresh(&self, r: &Rollup, full: bool, opts: &ExecOptions) -> Result<RefreshStats> {
        let t0 = Instant::now();
        let mut stats = RefreshStats::default();
        let base = self.table(&r.base)?;
        let roll = self.table(&r.name)?;
        let types = r.check(&base)?;
        let opts = all_rows(opts);
        let one = |sql: &str, p: &Params| -> Result<Option<Value>> { Ok(self.query_with(sql, p, &opts)?.rows.into_iter().next().and_then(|r| r.into_iter().next())) };
        let Some(newest) = time_of(one(&r.edge_sql(true), &Params::new())?.as_ref()) else { return Ok(stats) };
        let bucket = qsql::ident(&r.bucket);
        let folded = time_of(one(&r.folded_sql(), &Params::new())?.as_ref());
        let last = r.bucket_of(newest);
        let hot = r.bucket_of(newest.saturating_sub(r.lateness));
        let mut first = match (full, folded) {
            (false, Some(m)) => hot.min(m + r.width),
            _ => match time_of(one(&r.edge_sql(false), &Params::new())?.as_ref()) {
                Some(oldest) => r.bucket_of(oldest),
                None => return Ok(stats),
            },
        };
        // Buckets that rows have begun to leave by the table's retention
        // stay as they were folded.
        if let Some(keep) = &base.retain {
            let cutoff = crate::timestamp::now().saturating_sub(keep.seconds.saturating_mul(1_000_000));
            first = first.max(r.bucket_of(cutoff) + r.width);
        }
        if first > last {
            return Ok(stats);
        }
        let counts: HashMap<i64, i64> = {
            let sql = format!("SELECT {bucket}, SUM({}) FROM {} WHERE {bucket} >= :rollup_from AND {bucket} < :rollup_to GROUP BY {bucket}", qsql::ident(r.count_column()), qsql::ident(&r.name));
            self.query_with(&sql, &window(first, last + r.width), &opts)?.rows.into_iter().filter_map(|row| Some((time_of(row.first())?, int_of(row.get(1))))).collect()
        };
        let count_sql = r.count_sql();
        let mut b = first;
        while b <= last {
            stats.checked += 1;
            let have = int_of(one(&count_sql, &window(b, b + r.width))?.as_ref());
            if have != counts.get(&b).copied().unwrap_or(0) {
                stats.rows_written += self.fold(r, &roll, &types, b, &opts)?;
                stats.folded += 1;
            }
            b += r.width;
        }
        stats.seconds = t0.elapsed().as_secs_f64();
        Ok(stats)
    }

    /// Folds bucket `b` of rollup `r` again, in one transaction: a row for
    /// each group the bucket holds now, and none for the groups it no
    /// longer holds.
    fn fold(&self, r: &Rollup, roll: &Schema, types: &[(String, &'static str)], b: i64, opts: &ExecOptions) -> Result<u64> {
        let rows = self.query_with(&r.fold_sql(), &window(b, b + r.width), opts)?.rows;
        let keys = r.keys.len();
        let mut ops = Vec::with_capacity(rows.len());
        let mut wrote = HashSet::new();
        for row in rows {
            let key = key_of(b, &row[..keys]);
            let mut doc = Vec::with_capacity(roll.fields.len());
            doc.push(Value::Str(key.clone()));
            doc.push(Value::Timestamp(b));
            // types[0] is the bucket's; the groups and aggregates follow.
            doc.extend(row.into_iter().zip(&types[1..]).map(|(v, (_, ty))| fit(v, ty)));
            wrote.insert(key);
            ops.push(WriteOp::Upsert { table: r.name.clone(), doc });
        }
        let had = format!("SELECT {KEY} FROM {} WHERE {} = :rollup_from", qsql::ident(&r.name), qsql::ident(&r.bucket));
        for row in self.query_with(&had, &window(b, b), opts)?.rows {
            if let Some(Value::Str(k)) = row.into_iter().next() {
                if !wrote.contains(&k) {
                    ops.push(WriteOp::Delete { table: r.name.clone(), key: Value::Str(k) });
                }
            }
        }
        self.write_ops(ops)
    }

    /// Refreshes every rollup in the background from now on, every
    /// `every`, and every bucket of each every sixty refreshes
    /// ([`Db::refresh_rollup`]). The thread ends once the database is
    /// dropped.
    pub fn spawn_rollups(self: &Arc<Self>, every: Duration) {
        let db: Weak<Db> = Arc::downgrade(self);
        let spawned = std::thread::Builder::new().name("chloedb-rollups".into()).spawn(move || {
            let mut tick: u64 = 0;
            loop {
                let t0 = Instant::now();
                while t0.elapsed() < every {
                    std::thread::sleep(Duration::from_millis(200).min(every));
                    if db.strong_count() == 0 {
                        return;
                    }
                }
                let Some(db) = db.upgrade() else { return };
                let full = tick % SWEEP_EVERY == SWEEP_EVERY - 1;
                tick += 1;
                let rollups = match db.rollups() {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!(error = %e, "could not read the rollups");
                        continue;
                    }
                };
                for r in rollups {
                    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| db.refresh(&r, full, &db.exec)));
                    match res {
                        Ok(Ok(s)) if s.folded > 0 => tracing::debug!(rollup = %r.name, checked = s.checked, folded = s.folded, rows = s.rows_written, secs = s.seconds, "refreshed a rollup"),
                        Ok(Ok(_)) => {}
                        Ok(Err(e)) => tracing::warn!(rollup = %r.name, error = %e, "a rollup's refresh failed; the next one tries again"),
                        Err(_) => tracing::warn!(rollup = %r.name, "a rollup's refresh panicked; the next one tries again"),
                    }
                }
            }
        });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "could not start the rollups' thread");
        }
    }
}
