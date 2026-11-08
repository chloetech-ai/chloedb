//! `strata-server follow-postgres`: Strata following a Postgres database by
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
//! Strata's tables the follower writes must take no other writes: a row
//! written again on a replay must give what it gave.

pub mod follow;
pub mod pgoutput;
pub mod types;
pub mod wire;

pub use follow::{default_slot, drop_slot, run, Options, BATCH, COMPOSITE_KEY};
