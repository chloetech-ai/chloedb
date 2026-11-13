# Feature store: point-in-time joins and online lookups against Feast

An experiment, 2026-11-13, on one cloud container (Intel Xeon @ 2.10GHz,
4 cores, 15 GB, Linux 6.18), with chloedb-server built from branch
`claude/project-thread-481x6x` (`--profile ci`) and Feast 0.68.0. Times
depend on the machine; the values compared do not.

## Data

[`gen.py`](gen.py) `data 20000 10000 30 500000` writes synthetic daily
snapshots of 5 features for 20,000 users (480,303 rows) and of 5 features
for 10,000 items (239,842 rows) over 30 days, each at a random second of
its day (about 80% of days), and a spine of 500,000 (user, item, time,
label) rows. The same Parquet files feed every engine:
`chloedb-server import --db fs.chloedb --table T --key id T.parquet`, and
Feast's [`defs.py`](defs.py) with a DuckDB offline store
([`feature_store.yaml`](feature_store.yaml)).

## Training set: the 10 features as of each spine row's time

[`offline.py`](offline.py) and [`today.py`](today.py), end to end into a
pandas DataFrame, median of 3 runs:

| how | seconds | values different from pandas | feature rows from after the spine row |
|---|---:|---:|---:|
| pandas `merge_asof`, in memory (the reference) | 1.14 | 0 | 0 |
| Feast `get_historical_features`, DuckDB offline store | 2.87 | 0 | not checked |
| ChloeDB `ASOF JOIN` (two, one per table), Arrow over HTTP | 4.16 | 0 | 0 |
| ChloeDB without it: `LEFT JOIN ... ON key AND time <=`, then `DISTINCT ON`, a table at a time in `WITH` | 5.08 | 0 | 0 |

The ChloeDB query:

```sql
SELECT s.id, s.label, u.clicks_7d, ..., i.ctr_7d, ...
FROM spine s
ASOF JOIN users u MATCH_CONDITION (s.event_timestamp >= u.event_timestamp) ON s.u_id = u.u_id
ASOF JOIN items i MATCH_CONDITION (s.event_timestamp >= i.event_timestamp) ON s.i_id = i.i_id
```

In the server (3.2 s of the 4.2), the scans take about 1.0 s, the two
joins' hashing of the right side 0.35 s, their probes 0.45 and 0.4 s (on 4
threads), and building the 12-column answer the rest. It runs on the
operators, row by row (DataFusion falls back for an ASOF JOIN); a columnar
join is the next step. Each side of a join holds at most 1,000,000 rows.

## Online: 10 features for 1,000 (user, item) pairs

[`online.py`](online.py): the latest values, by key. ChloeDB reads tables of
the latest rows (`users_latest`, `items_latest`, imported from the same
snapshots) with two `POST /v1/kv/get` calls naming the columns; Feast
materializes into its online store and answers `get_online_features` from
the Python SDK in the client's process. 55 requests after 5 warm-ups:

| how | p50 ms | p99 ms |
|---|---:|---:|
| ChloeDB `/v1/kv/get`, two calls over HTTP | 9.0 | 10.1 to 11.4 |
| Feast SDK, Redis 7.0.15 online store (local) | 64.4 | 153.3 |
| Feast SDK, SQLite online store | 74.3 | 158.4 |

Feast's time is mostly its Python SDK, not the store; its feature server
was not measured. The user features ChloeDB and Feast returned are the same.
