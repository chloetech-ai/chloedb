# Following Postgres

`chloedb-server follow-postgres` keeps ChloeDB in step with a Postgres database by logical replication: it copies the tables of a publication, then applies every insert, update, delete, `TRUNCATE` and new column a moment after Postgres commits it, so ChloeDB serves search over the rows Postgres holds without a nightly job or a Debezium and Kafka pipeline in between. It speaks Postgres's own protocol (`pgoutput`, the plugin Postgres ships, from version 10), and keeps what it applied in ChloeDB's `_sources`, in the same transactions as the rows, so a follower stopped and started again goes on from there and applies each change once.

This is ChloeDB reading from Postgres, not ChloeDB speaking to Postgres clients: ChloeDB serves no Postgres protocol of its own, so `psql`, DBeaver, Metabase and dbt's Postgres adapter cannot connect to it. [Running SQL](query-language.md#running-sql) lists the ways in.

## Set up Postgres

Postgres writes what logical replication needs once `wal_level` is `logical` (a restart takes it):

```sql
ALTER SYSTEM SET wal_level = logical;   -- then restart Postgres
CREATE PUBLICATION strata FOR TABLE products, reviews;   -- or FOR ALL TABLES
CREATE ROLE chloedb LOGIN REPLICATION PASSWORD '...';
GRANT SELECT ON products, reviews TO chloedb;
```

The role signs in by SCRAM-SHA-256 (Postgres's default), MD5 or a password, and needs `REPLICATION` and `SELECT` on the tables. On managed Postgres, logical replication is a setting (`rds.logical_replication` on RDS and Aurora, `cloudsql.logical_decoding` on Cloud SQL; Supabase and Neon have it on). Since Postgres 15 a publication can leave columns out (`FOR TABLE users (id, name, bio)`) and rows (`WHERE (published)`); the follower copies and follows what it publishes.

Each table needs a key: its primary key, or a unique index named as its replica identity (`ALTER TABLE t REPLICA IDENTITY USING INDEX t_code_key`). A key of several columns becomes one `KEYWORD` column `_key` in ChloeDB, their values joined by `:` (`42:en`), beside the columns themselves.

## Run it

```sh
chloedb-server follow-postgres \
  --source 'postgres://chloedb:...@db.example:5432/shop?sslmode=verify-full' \
  --target http://localhost:7700 --name shop
```

| option | |
|---|---|
| `--source` | the database's URL (or `CHLOEDB_POSTGRES_URL`); `PGPASSWORD` gives a password it leaves out; `sslmode` is `disable`, `prefer`, `require`, `verify-ca` or `verify-full`; without one, `require` for a database on another machine (a connection that could fall back to plain text would send the password's proof and every row in the clear) and `prefer` for one on `localhost` or a loopback address, and `sslrootcert` (or `--source-ca`) a CA certificate to check the server's by; with `verify-*` and no CA given, the public CAs are trusted |
| `--target` | ChloeDB's HTTP URL: a server, or a cluster's coordinator (`--target-token` or `CHLOEDB_CLUSTER_TOKEN`, `--target-ca`) |
| `--name` | the name ChloeDB knows the database by, which its marks in `_sources` name |
| `--publication` | the publication (`strata`) |
| `--slot` | the replication slot it makes and reads (`strata_<name>`) |
| `--batch` | the most rows of one write while copying, and of the transactions written together (1,000) |
| `--copy-workers` | how many tables are copied at once (4) |
| `--copy` | copy everything again, with a new slot. Each followed table is emptied first and filled again as the copy goes, so its searches and counts answer partly, or nothing, until its copy ends ([copying again](#run-it)) |
| `--copy-table` | copy these tables again once following (`orders`, `sales.orders`, or ChloeDB's name; repeat it or separate the names by commas), over the rows ChloeDB holds, which stay readable until the new copy is in; the rows Postgres no longer has are deleted after it, and the other tables follow meanwhile ([one table](#when-one-table-fails)) |
| `--drop-slot` | drop the slot, release the tables it followed (they take other writes again) and exit |

The first run makes the replication slot, which holds a snapshot of the database, and copies each table as of that snapshot while Postgres goes on taking writes; it then streams the slot's changes from the snapshot's position, so no change between the two is missed or applied twice. A ChloeDB table it lacks is made from the Postgres table; one that exists keeps its schema (see [below](#choosing-chloedb-types)). It tells ChloeDB how it stands every 2 seconds ([below](#how-far-behind-it-is)) and logs it every 30 (`applied`, `confirmed`, `postgres`, `behind_bytes`, `lag_s`), and `GET /v1/sources` on ChloeDB shows the last position it applied (`postgres/<name>/<copy>`, with its `lsn` as Postgres writes it), with when.

Stop it whenever: started again it resumes from what ChloeDB applied. While it is stopped, **Postgres keeps the WAL the slot has not consumed**, which can fill its disk over days; when you stop following for good, drop the slot (`--drop-slot`, or `SELECT pg_drop_replication_slot('strata_shop')`). Postgres 13 and later can cap what a slot keeps (`max_slot_wal_keep_size`); a slot that lost WAL it needed is copied afresh on the next start. A copy afresh, this one or `--copy`, empties each followed table before it fills it again: until a table's copy ends, its searches and counts see only the rows copied so far (in a persona study, 50,000 products read 0 rows for 2 s and were partial for 10 s, and 318,514 orders for 16 s). Copy again when the site can do without those tables' answers for as long as the copy takes; to copy one table again, `--copy-table` keeps its rows readable throughout and the others following.

## How far behind it is

Every 2 seconds the follower tells ChloeDB how it stands (`POST /v1/admin/follow/status`, which takes the cluster's token or an admin key: where the server asks for keys, give `--target-token` one, or the follower logs once that its reports are refused and goes on following). ChloeDB shows it on Data > Connectors (`followers` in `GET /v1/admin/connectors`), in `/metrics` and to the built-in alert "A follower is behind its source" (`follow-behind`):

| | |
|---|---|
| `state` | `copying`, `following`, `retrying` (ChloeDB refused a write, which it tries again; `error` says why), `failed` (it exited; `error`) or `stopped` (its slot was dropped) |
| `applied_lsn` | where the last transaction it wrote to ChloeDB ended |
| `confirmed_lsn` | the position it told Postgres it holds, before which Postgres may free the WAL: `applied_lsn` or later, since Postgres's WAL also holds what the publication does not send |
| `postgres_lsn` | Postgres's own position, `pg_current_wal_lsn()` (on a standby, `pg_last_wal_replay_lsn()`), read every 2 seconds on a connection of its own |
| `behind_bytes` | `postgres_lsn` less `confirmed_lsn`: the WAL Postgres keeps for it |
| `lag_ms` | since Postgres's WAL first reached a position the follower has not confirmed (within 8 KB), so 0 once it has confirmed what Postgres had |

Positions are written as Postgres writes them (`0/16B3748`), to compare with `pg_current_wal_lsn()` and `confirmed_flush_lsn` in `pg_replication_slots`. A transaction still open in Postgres is not behind: Postgres sends it when it commits.

A follower that stops telling (killed, or cut off from ChloeDB) is *silent* after 10 seconds, and from then on its lag grows by the time since it last told, so `follow-behind` (over 60 seconds behind, for a minute) fires within about two minutes of it stopping. `/metrics` has `chloedb_follow_lag_seconds`, `chloedb_follow_behind_bytes` and `chloedb_follow_up` (1 while it follows, copies or catches up and told within 10 seconds; 0 while it retries, failed or is silent), labelled with its `name` and `kind="postgres"`, and per table the series of [a table held](#when-one-table-fails). ChloeDB holds what followers told it in memory: after ChloeDB restarts, Data > Connectors lists a follower that has not told it since from its marks, as not reporting, and the alert waits for it to tell again.

## What it applies

- **Inserts and updates** are upserts of the whole row by key; an update that changes the key deletes the old row. A large value an update leaves unchanged, which Postgres does not send (TOAST), is kept: taken from the old row with `REPLICA IDENTITY FULL`, or else read from ChloeDB's row.
- **Deletes** are by key. **`TRUNCATE`** deletes every row of the table.
- **Columns added** in Postgres are added to the ChloeDB table (`ALTER TABLE ... ADD COLUMN`), and a column renamed while the follower runs is renamed. A column renamed while the follower is stopped is not: started again, it sees a column it does not know and adds it, so the old column keeps the values from before and the new one has only those written since. Rename such a column in ChloeDB too (`ALTER TABLE products RENAME COLUMN color TO colour`) before you start the follower again, or keep it running through the migration. The rows from before the column read what they read in Postgres: a constant default (`ADD COLUMN vip boolean DEFAULT false`), which Postgres keeps beside the table rather than writing into its rows, so the stream never sends it, becomes the ChloeDB column's `DEFAULT`; a default Postgres writes into each row (a volatile one, as `random()` or `gen_random_uuid()`, or a type ChloeDB's `DEFAULT` cannot hold, as `jsonb`) has the table copied again, as of then, with the changes since applied over it. A column dropped in Postgres stays in ChloeDB, `NULL` (or its default) in rows written since. A changed type that maps to another ChloeDB type (`ALTER COLUMN qty TYPE numeric(6,2)` on an `integer`: `INT` to `FLOAT`), in a column of the type the follower made, has that table alone copied again with the column made anew: its rows stay readable, and the column reads `NULL` in each until the copy writes it again (seconds for a few hundred thousand rows); the other tables follow meanwhile. ChloeDB cannot change a column's type in place, and a coordinator does not drop columns yet, so on a cluster such a table is held ([below](#when-one-table-fails)) until you copy it again with `--copy`. A key column whose type changed holds its table too. Any other changed type keeps the ChloeDB column's, and the values are read as it reads them.
- **Tables joining the publication** (`ALTER PUBLICATION ... ADD TABLE`, or a table created under `FOR ALL TABLES`) are copied when the follower sees them, within 30 seconds, then followed. A table leaving it is no longer followed; its ChloeDB rows stay. A table renamed in Postgres goes on into the ChloeDB table of its old name.
- ChloeDB's tables the follower writes take no other writes: an `INSERT`, `UPDATE` or `DELETE` of one, or a `POST /v1/write`, is refused (409, `followed_table`), since a row written in ChloeDB would answer what Postgres does not until Postgres changed it and the follower wrote over it, and a transaction applied again after a restart must write what it wrote before. Write the change in Postgres. The follower names itself on its requests (`x-chloedb-postgres-follower`), and a server reads which tables are followed from the follower's marks within seconds of a copy. Nor can the table be dropped in ChloeDB, or a column the follower writes dropped or renamed there (`DROP TABLE`, `ALTER TABLE ... DROP COLUMN`, `RENAME COLUMN`: 409, `followed_table`): the follower would stop on the table, or write the column again without the values it held. Change the table in Postgres. Columns may still be added in ChloeDB (an embedded vector, say), and a column ChloeDB derives may be dropped or renamed. A table that leaves the publication, or whose follower's slot is dropped (`--drop-slot`), takes writes again.

Transactions are applied whole, each in one write (several at once while Postgres sends faster than they are written), but one of more than 1,000 changes, more than 16 MB of values, or with a `TRUNCATE`, is written in steps: a reader can see part of it, as it can see part of any write across a cluster's shards.

**Changed:** a follower of 0.3.7 or earlier adds a new column with its type only, so the rows from before read `NULL` in it until Postgres next changes each of them (add the column in ChloeDB first, with the same default, or copy again with `--copy`), and its server takes other writes to the followed tables, whose rows then differ from Postgres's, with no warning, until Postgres changes them.

### How Postgres columns become fields

| Postgres | ChloeDB |
|---|---|
| `boolean` | `BOOL` |
| `smallint`, `integer`, `bigint`, `oid` | `INT` |
| `real`, `double precision`, `numeric` | `FLOAT` |
| `text`, `varchar`, `name`, `citext`, `xml` | `TEXT` (`KEYWORD` as the key) |
| `char(n)`, `uuid`, enums, and other types (`inet`, `interval`, ...) | `KEYWORD`, their text |
| `date`, `timestamp`, `timestamptz` | `TIMESTAMP` (UTC; `infinity` is `NULL`) |
| `json`, `jsonb` | `JSON` (a value that is not an object becomes `{"value": ...}`; an empty key, which ChloeDB's JSON refuses, becomes `_empty`, at any depth, with `_` added while the object holds that key too) |
| arrays of text types, `uuid` or enums | `KEYWORD[]` |
| arrays of integers | `INT[]` |
| pgvector's `vector(n)`, `halfvec(n)` | `VECTOR(n)` |
| pgvector's `sparsevec` | `SPARSE` |
| `bytea`, `tsvector`, other arrays, `vector` without a dimension | left out |

### Choosing ChloeDB types

A `text` or `varchar` column becomes `TEXT`, which is searched by its words. It groups (`GROUP BY country`, `SELECT DISTINCT country`) and compares with `=` by its stored text, read row by row, where a `KEYWORD`'s index answers both ([differences from PostgreSQL](query-language.md#differences-from-postgresql)). A join compares `KEYWORD`, `INT` and `FLOAT` columns, not `TEXT`: `ON a.borough = b.borough` of two followed `varchar` columns is refused, and the message says so. A column of names, codes or statuses that you join, group and filter by its whole value (`brand` below) belongs in a `KEYWORD`; for a table followed already, `CREATE TABLE zones_k AS SELECT *, CAST(borough AS KEYWORD) AS borough_k FROM zones` makes a copy to join ([CREATE TABLE ... AS SELECT](query-language.md#a-table-or-rows-from-a-select-create-table--as-select-and-insert--select)). To choose the types, analyzers, an embedded vector or fields of ChloeDB's own, make the table in ChloeDB before the first run, with the Postgres key as its key and the columns of the names Postgres has:

```sql
CREATE TABLE products (
  id INT PRIMARY KEY,
  title TEXT,
  sku KEYWORD,
  brand KEYWORD,
  description TEXT OPTIONS(analyzer = 'english'),
  price FLOAT,
  emb VECTOR OPTIONS(embed = 'title, description', model = 'potion-retrieval-32M')
);
```

The follower writes each Postgres column into the field of its name as that field reads it (an `integer` into a `KEYWORD`, a `real[]` into a `VECTOR(n)`), adds the published columns the table lacks, and leaves the other fields to ChloeDB: here `emb` is embedded from each row as it arrives. A value a field cannot read (text in an `INT`, a JSON value nested more than 16 levels) holds its table, with the table, column, value and the row's key ([below](#when-one-table-fails)); fix the row in Postgres, or the ChloeDB table, and the follower copies the table again at its next try.

## When one table fails

A table the follower cannot write (a value ChloeDB refuses, a table without a primary key joining the publication, two Postgres tables that become one ChloeDB table, a ChloeDB table dropped from under it) is *held*: the follower stops following that table and goes on with the others, and the process stays up. A held table keeps its ChloeDB rows (and still takes no other writes), and its changes in the stream are not applied: when it is tried again, at first after 30 seconds and then after twice as long each time, up to 15 minutes, it is copied again as of then, over its rows, which stay readable, the rows Postgres no longer has are deleted, and it is followed again. A follower started again copies the tables it held when it stopped (`.../again/<oid>`, [below](#how-a-restart-resumes)).

Data > Connectors shows each held table with its error and when it is tried next; `GET /v1/admin/connectors` has them in the follower's `table_states` (`state` `held`, `error`, `since_ms`, `attempts`, `retry_in_s`) and counts them in `held_tables`, and its `last_error` is the latest. `/metrics` has, per table, `chloedb_follow_table_up` (0 while held), `chloedb_follow_table_lag_seconds` (since it was held, while held) and `chloedb_follow_table_error_info` (1, with the error in its `error` label), and per follower `chloedb_follow_tables_held` and `chloedb_follow_last_error_timestamp_seconds`. The built-in alert `follow-table-held` ("A Postgres follower holds a table") fires when one stays held for a minute; the runbook is [A Postgres follower holds a table](runbooks.md#a-postgres-follower-holds-a-table).

At the start, a publication none of whose tables can be followed stops the follower, saying why for each. The follower exits too when ChloeDB refuses it as a whole (a wrong `--target-token`) or on an error it cannot place on a table; it connects again by itself when Postgres or ChloeDB does not answer. Run it under a supervisor that starts it again, which resumes it where it stopped:

```ini
# /etc/systemd/system/chloedb-follow-shop.service
[Unit]
Description=ChloeDB follows Postgres database shop
After=network-online.target
Wants=network-online.target

[Service]
# CHLOEDB_POSTGRES_URL=postgres://... and, for a cluster, CHLOEDB_CLUSTER_TOKEN=...
EnvironmentFile=/etc/chloedb/follow-shop.env
ExecStart=/usr/local/bin/chloedb-server follow-postgres --target http://localhost:7700 --name shop
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

`systemctl enable --now chloedb-follow-shop`, and `journalctl -u chloedb-follow-shop` for its log. In a container, `docker run -d --restart unless-stopped --env-file follow-shop.env ghcr.io/chloetech-ai/chloedb-server:latest follow-postgres --target http://chloedb:7700 --name shop` (on Kubernetes, a Deployment of one replica restarts it). Run one follower per `--name`: two would read the same slot, and Postgres gives it to one at a time.

## How a restart resumes

What the follower applied is in ChloeDB's `_sources`, written in the same transaction as the rows it covers:

- `postgres/<name>/copy` is when the copy it follows from began; a copy made again names every other mark afresh, under `postgres/<name>/<copy>/`.
- `postgres/<name>/<copy>` is the LSN where the last applied Postgres transaction ends. Every operation of a transaction carries it as its stream offset, so a shard that applied the transaction skips it when it comes again, and a follower started again streams from the least over the shards ([streams](server.md#streams)).
- A transaction written in steps numbers them (`.../step`, carried by each step's operations), and its last step writes how many steps such transactions took so far (`.../base`): replayed, it numbers its steps as before, each shard skips the steps it applied, and a `TRUNCATE` already done is not done again.
- `.../copied` says the copy finished, and `.../t/<oid>/<table>` which Postgres table each ChloeDB table follows.
- `.../again/<oid>` is odd while a table's copy is owed (a table held, or a column Postgres wrote values into) and even once it is made again: a follower started again copies the tables owed.

The follower tells Postgres how far it applied, so that the slot lets go of the WAL before it. A copy cut short is made again, from a new slot.
