//! `chloedb-server follow-postgres`: Strata following a Postgres database by
//! logical replication, as a search index that does not drift from it.
//!
//! It reads a publication's tables through a replication slot of its own
//! with the `pgoutput` plugin that Postgres ships (10 and later, `wal_level
//! = logical`), and writes through Strata's HTTP API, to a server or a
//! cluster's coordinator:
//!
//! * First it copies: it makes the slot, which exports a snapshot, and
//!   copies each table as of that snapshot (`COPY ... TO STDOUT`, several
//!   tables at once), making the Strata tables it lacks
//!   ([`types`] maps the columns; a table made beforehand keeps its own
//!   types and extra fields, as embedded vectors). It then streams the
//!   slot's changes from the snapshot's position, so that no change is
//!   missed or applied twice.
//! * Inserts and updates are upserts of the whole row; deletes are by key
//!   (the replica identity: by default the primary key; several columns
//!   become one `_key`). A large value an update leaves out (unchanged
//!   TOAST) is taken from the old row (`REPLICA IDENTITY FULL`), or the
//!   row Strata holds. `TRUNCATE` deletes every row of the table.
//! * Schema changes: Postgres describes a table again after it changes;
//!   a column added there is added here (`ALTER TABLE ADD COLUMN`), one
//!   renamed is renamed, one dropped stays (NULL in rows written since).
//!   A table that joins the publication (`ALTER PUBLICATION ... ADD
//!   TABLE`, or a new one under `FOR ALL TABLES`) is copied when it does,
//!   then followed; one that leaves it is no longer followed, its rows kept.
//!
//! What it applied is kept in Strata's `_sources`, in the transactions
//! that apply it, so that a follower started again (or reconnected) goes
//! on from there and applies each change once:
//!
//! * `postgres/<name>/copy`: when the copy it follows from began (ms). A
//!   copy made again (`--copy`, or because the slot was lost) names every
//!   other mark afresh, under `postgres/<name>/<copy>`.
//! * `postgres/<name>/<copy>`: the end of the last Postgres transaction
//!   applied (its LSN). Each operation of a transaction carries it as its
//!   stream offset, so a shard that applied the transaction skips it when
//!   it comes again: transactions are written whole (several at once while
//!   Postgres sends faster than they are written), and a restart streams
//!   from the least offset over the shards.
//! * `.../step` and `.../base`: a transaction of more than a thousand
//!   operations (or 16 MB of values), or with a `TRUNCATE`, is written in
//!   steps, each numbered (its operations carry the number as the offset
//!   of `.../step`); `.../base` is written with the transaction's last
//!   step and says how many steps all such transactions took so far, so a
//!   transaction replayed numbers its steps as before, and each shard
//!   skips the steps it applied. (A reader may see such a transaction in
//!   part, as it can a transaction across shards.)
//! * `.../copied`: the copy is done, at that LSN; `.../t/<oid>/<table>`:
//!   the Postgres table (by OID) that Strata table follows (an odd offset)
//!   or followed (even). A table Postgres renames keeps its Strata name.
//!
//! The follower tells Postgres how far it applied (Standby Status Update),
//! so that the slot lets go of the WAL before it; when nothing of the
//! publication is pending, as far as the server's WAL reaches. A slot that
//! no follower reads keeps WAL without end: `--drop-slot` drops it.
//!
//! Strata's tables the follower writes take no other writes: a row
//! written again on a replay must give what it gave, and a row written
//! here would answer what Postgres does not. A server refuses (409) a
//! write to a table a follower follows, by its marks ([`followed_tables`]),
//! unless the follower sends it (it names itself in
//! [`crate::service::POSTGRES_FOLLOWER_HEADER`]); a table that leaves the
//! publication, or whose follower's slot is dropped (`--drop-slot`),
//! takes writes again.
//!
//! A column Postgres adds is added here with the value Postgres gives the
//! rows it had (its `DEFAULT`, which Postgres keeps as the column's
//! missing value rather than writing the rows, and which the stream
//! therefore never sends): as the column's `DEFAULT` here, which rows
//! written before read. A column whose earlier rows Postgres wrote (a
//! volatile default, as `now()`, rewrites the table) has the table copied
//! again, as of then, and the stream's changes since applied over it.

use std::collections::HashMap;

pub mod follow;
pub mod pgoutput;
pub mod types;
pub mod wire;

pub use follow::{default_slot, drop_slot, run, Options, BATCH, COMPOSITE_KEY};

/// Whether stream source `name` is a Postgres follower's mark whose
/// offset is an LSN: `postgres/<name>/<copy>`, the end of the last
/// transaction applied, and `postgres/<name>/<copy>/copied`, where the
/// copy ended.
pub fn is_lsn_mark(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("postgres/") else { return false };
    let parts: Vec<&str> = rest.split('/').collect();
    let copy = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    match parts.as_slice() {
        [_, c] => copy(c),
        [_, c, "copied"] => copy(c),
        _ => false,
    }
}

/// The tables Postgres followers follow, by name in lower case: the
/// follower's `--name`. Of `sources` (every stream source's name and
/// offset, `/v1/sources`), each follower's marks of its current copy
/// (`postgres/<name>/copy`) whose table mark (`postgres/<name>/<copy>/t/<oid>/<table>`)
/// is odd: followed, not left.
pub fn followed_tables(sources: &[(String, i64)]) -> HashMap<String, String> {
    let copies: HashMap<&str, i64> = sources
        .iter()
        .filter_map(|(n, o)| Some((n.strip_prefix("postgres/")?.strip_suffix("/copy")?, *o)))
        .filter(|(n, _)| !n.contains('/'))
        .collect();
    // The latest mark of each table: (oid, table) -> (offset, follower).
    let mut latest: HashMap<(String, String), (i64, &str)> = HashMap::new();
    for (n, offset) in sources {
        let Some(rest) = n.strip_prefix("postgres/") else { continue };
        let mut parts = rest.splitn(5, '/');
        let (Some(name), Some(copy), Some("t"), Some(oid), Some(table)) = (parts.next(), parts.next(), parts.next(), parts.next(), parts.next()) else { continue };
        if copies.get(name).map(|c| c.to_string()).as_deref() != Some(copy) || oid.parse::<u32>().is_err() {
            continue;
        }
        let e = latest.entry((format!("{name}/{oid}"), table.to_ascii_lowercase())).or_insert((i64::MIN, name));
        if *offset > e.0 {
            *e = (*offset, name);
        }
    }
    latest.into_iter().filter(|(_, (o, _))| o % 2 == 1).map(|((_, table), (_, name))| (table, name.to_string())).collect()
}

#[cfg(test)]
mod tests {
    use super::{followed_tables, is_lsn_mark};

    #[test]
    fn the_marks_that_hold_lsns_are_known() {
        assert!(is_lsn_mark("postgres/shop/1795032445947"));
        assert!(is_lsn_mark("postgres/shop/1795032445947/copied"));
        for not in ["postgres/shop/copy", "postgres/shop/17/step", "postgres/shop/17/base", "postgres/shop/17/t/16399/products", "postgres/shop/17/again/16399", "orders-topic", "postgres/shop/x"] {
            assert!(!is_lsn_mark(not), "{not}");
        }
    }

    #[test]
    fn followed_tables_are_those_of_the_current_copy_not_left() {
        let s = |n: &str, o: i64| (n.to_string(), o);
        let sources = vec![
            s("postgres/shop/copy", 200),
            s("postgres/shop/200", 55),
            s("postgres/shop/200/copied", 50),
            s("postgres/shop/200/t/16384/Customers", 3001),
            s("postgres/shop/200/t/16390/orders", 3001),
            s("postgres/shop/200/t/16390/orders", 4000),
            s("postgres/shop/100/t/16400/old_copy", 1001),
            s("postgres/crm/copy", 7),
            s("postgres/crm/7/t/1/leads", 9),
            s("orders-topic", 12),
        ];
        let f = followed_tables(&sources);
        assert_eq!(f.get("customers").map(String::as_str), Some("shop"));
        assert_eq!(f.get("leads").map(String::as_str), Some("crm"));
        assert!(!f.contains_key("orders"), "it left the publication: an even mark later than the odd one");
        assert!(!f.contains_key("old_copy"), "a mark of a copy before the current one");
        assert_eq!(f.len(), 2, "{f:?}");
    }
}
