# Following Postgres

`strata-server follow-postgres` keeps ChloeDB in step with a Postgres database by logical replication: it copies the tables of a publication, then applies every insert, update, delete, `TRUNCATE` and new column a moment after Postgres commits it, so ChloeDB serves search over the rows Postgres holds without a nightly job or a Debezium and Kafka pipeline in between. It speaks Postgres's own protocol (`pgoutput`, the plugin Postgres ships, from version 10), and keeps what it applied in ChloeDB's `_sources`, in the same transactions as the rows, so a follower stopped and started again goes on from there and applies each change once.

## Set up Postgres

Postgres writes what logical replication needs once `wal_level` is `logical` (a restart takes it):

```sql
ALTER SYSTEM SET wal_level = logical;   -- then restart Postgres
CREATE PUBLICATION strata FOR TABLE products, reviews;   -- or FOR ALL TABLES
CREATE ROLE strata LOGIN REPLICATION PASSWORD '...';
GRANT SELECT ON products, reviews TO strata;
```

The role signs in by SCRAM-SHA-256 (Postgres's default), MD5 or a password, and needs `REPLICATION` and `SELECT` on the tables. On managed Postgres, logical replication is a setting (`rds.logical_replication` on RDS and Aurora, `cloudsql.logical_decoding` on Cloud SQL; Supabase and Neon have it on). Since Postgres 15 a publication can leave columns out (`FOR TABLE users (id, name, bio)`) and rows (`WHERE (published)`); the follower copies and follows what it publishes.

Each table needs a key: its primary key, or a unique index named as its replica identity (`ALTER TABLE t REPLICA IDENTITY USING INDEX t_code_key`). A key of several columns becomes one `KEYWORD` column `_key` in ChloeDB, their values joined by `:` (`42:en`), beside the columns themselves.

## Run it

```sh
strata-server follow-postgres \
  --source 'postgres://strata:...@db.example:5432/shop?sslmode=verify-full' \
  --target http://localhost:7700 --name shop
```

| option | |
|---|---|
| `--source` | the database's URL (or `STRATA_POSTGRES_URL`); `PGPASSWORD` gives a password it leaves out; `sslmode` is `disable`, `prefer` (the default), `require`, `verify-ca` or `verify-full`, and `sslrootcert` (or `--source-ca`) a CA certificate to check the server's by; with `verify-*` and no CA given, the public CAs are trusted |
| `--target` | ChloeDB's HTTP URL: a server, or a cluster's coordinator (`--target-token` or `STRATA_CLUSTER_TOKEN`, `--target-ca`) |
| `--name` | the name ChloeDB knows the database by, which its marks in `_sources` name |
| `--publication` | the publication (`strata`) |
| `--slot` | the replication slot it makes and reads (`strata_<name>`) |
| `--batch` | the most rows of one write while copying, and of the transactions written together (1,000) |
| `--copy-workers` | how many tables are copied at once (4) |
| `--copy` | copy everything again, with a new slot |
| `--drop-slot` | drop the slot and exit |

The first run makes the replication slot, which holds a snapshot of the database, and copies each table as of that snapshot while Postgres goes on taking writes; it then streams the slot's changes from the snapshot's position, so no change between the two is missed or applied twice. A ChloeDB table it lacks is made from the Postgres table; one that exists keeps its schema (see [below](#choosing-chloedb-types)). It logs how far it is every 30 seconds (`applied`, the server's WAL position, `behind_bytes`), and `GET /v1/sources` on ChloeDB shows the last LSN it applied (`postgres/<name>/<copy>`), with when.

Stop it whenever: started again it resumes from what ChloeDB applied. While it is stopped, **Postgres keeps the WAL the slot has not consumed**, which can fill its disk over days; when you stop following for good, drop the slot (`--drop-slot`, or `SELECT pg_drop_replication_slot('strata_shop')`). Postgres 13 and later can cap what a slot keeps (`max_slot_wal_keep_size`); a slot that lost WAL it needed is copied afresh on the next start.

## What it applies

- **Inserts and updates** are upserts of the whole row by key; an update that changes the key deletes the old row. A large value an update leaves unchanged, which Postgres does not send (TOAST), is kept: taken from the old row with `REPLICA IDENTITY FULL`, or else read from ChloeDB's row.
- **Deletes** are by key. **`TRUNCATE`** deletes every row of the table.
- **Columns added** in Postgres are added to the ChloeDB table (`ALTER TABLE ... ADD COLUMN`), and a column renamed while the follower runs is renamed. A column dropped in Postgres stays in ChloeDB, `NULL` in rows written since. A changed type keeps the ChloeDB column's, and the values are read as it reads them.
- **Tables joining the publication** (`ALTER PUBLICATION ... ADD TABLE`, or a table created under `FOR ALL TABLES`) are copied when the follower sees them, within 30 seconds, then followed. A table leaving it is no longer followed; its ChloeDB rows stay. A table renamed in Postgres goes on into the ChloeDB table of its old name.
- ChloeDB's tables the follower writes take no other writes: a transaction applied again after a restart writes what it wrote before.

Transactions are applied whole, each in one write (several at once while Postgres sends faster than they are written), but one of more than 1,000 changes, more than 16 MB of values, or with a `TRUNCATE`, is written in steps: a reader can see part of it, as it can see part of any write across a cluster's shards.

### How Postgres columns become fields

| Postgres | ChloeDB |
|---|---|
| `boolean` | `BOOL` |
| `smallint`, `integer`, `bigint`, `oid` | `INT` |
| `real`, `double precision`, `numeric` | `FLOAT` |
| `text`, `varchar`, `name`, `citext`, `xml` | `TEXT` (`KEYWORD` as the key) |
| `char(n)`, `uuid`, enums, and other types (`inet`, `interval`, ...) | `KEYWORD`, their text |
| `date`, `timestamp`, `timestamptz` | `TIMESTAMP` (UTC; `infinity` is `NULL`) |
| `json`, `jsonb` | `JSON` (a value that is not an object becomes `{"value": ...}`) |
| arrays of text types, `uuid` or enums | `KEYWORD[]` |
| arrays of integers | `INT[]` |
| pgvector's `vector(n)`, `halfvec(n)` | `VECTOR(n)` |
| pgvector's `sparsevec` | `SPARSE` |
| `bytea`, `tsvector`, other arrays, `vector` without a dimension | left out |

### Choosing ChloeDB types

To choose the types, analyzers, an embedded vector or fields of ChloeDB's own, make the table in ChloeDB before the first run, with the Postgres key as its key and the columns of the names Postgres has:

```sql
CREATE TABLE products (
  id INT PRIMARY KEY,
  title TEXT,
  sku KEYWORD,
  description TEXT OPTIONS(analyzer = 'english'),
  price FLOAT,
  emb VECTOR OPTIONS(embed = 'title, description', model = 'potion-retrieval-32M')
);
```

The follower writes each Postgres column into the field of its name as that field reads it (an `integer` into a `KEYWORD`, a `real[]` into a `VECTOR(n)`), adds the published columns the table lacks, and leaves the other fields to ChloeDB: here `emb` is embedded from each row as it arrives. A value a field cannot read (text in an `INT`) stops the follower with the table, column and value; fix the table and start it again.

## How a restart resumes

What the follower applied is in ChloeDB's `_sources`, written in the same transaction as the rows it covers:

- `postgres/<name>/copy` is when the copy it follows from began; a copy made again names every other mark afresh, under `postgres/<name>/<copy>/`.
- `postgres/<name>/<copy>` is the LSN where the last applied Postgres transaction ends. Every operation of a transaction carries it as its stream offset, so a shard that applied the transaction skips it when it comes again, and a follower started again streams from the least over the shards ([streams](server.md#streams)).
- A transaction written in steps numbers them (`.../step`, carried by each step's operations), and its last step writes how many steps such transactions took so far (`.../base`): replayed, it numbers its steps as before, each shard skips the steps it applied, and a `TRUNCATE` already done is not done again.
- `.../copied` says the copy finished, and `.../t/<oid>/<table>` which Postgres table each ChloeDB table follows.

The follower tells Postgres how far it applied, so that the slot lets go of the WAL before it. A copy cut short is made again, from a new slot.
