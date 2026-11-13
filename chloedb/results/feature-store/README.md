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
| pandas `merge_asof`, in memory (the reference) | 1.06 | 0 | 0 |
| ChloeDB `ASOF JOIN` (two, one per table), columnar on DataFusion, Arrow over HTTP | 1.67 | 0 | 0 |
| Feast `get_historical_features`, DuckDB offline store | 2.75 | 0 | not checked |
| ChloeDB `ASOF JOIN` on the row operators (the first build, an earlier run) | 4.16 | 0 | 0 |
| ChloeDB without it: `LEFT JOIN ... ON key AND time <=`, then `DISTINCT ON`, a table at a time in `WITH` (an earlier run) | 5.08 | 0 | 0 |

The first three rows are one run of [`offline.py`](offline.py)
([`offline.json`](offline.json)); in an earlier run of the same script
pandas took 1.14 s and Feast 2.87 s.

The ChloeDB query:

```sql
SELECT s.id, s.label, u.clicks_7d, ..., i.ctr_7d, ...
FROM spine s
ASOF JOIN users u MATCH_CONDITION (s.event_timestamp >= u.event_timestamp) ON s.u_id = u.u_id
ASOF JOIN items i MATCH_CONDITION (s.event_timestamp >= i.event_timestamp) ON s.i_id = i.i_id
```

The join runs in DataFusion's path, on columns: both sides are read into
Arrow, the keys encoded with Arrow's row format, the right side's rows of
each key sorted by time, and each left row's match found by binary search
(in parallel, 16,384 rows a task); the answer is the left columns and the
right ones gathered by position. In the server, the scans and projection
into Arrow take about 0.6 s of the 1.67 s. The first build, on the row
operators, spent 3.2 s in the server, most of it building the 500,000
rows of 12 values one by one. Each side of a join holds at most 1,000,000
rows.

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

Feast's time is mostly its Python SDK, not the store. The user features ChloeDB and Feast returned are the same.

### Over HTTP: Feast's feature server

[`serve.py`](serve.py) ([`serve.json`](serve.json)) sends the same requests
over HTTP, for 8 seconds at 1, 4 and 16 clients, to `feast serve -w 4`
(Redis online store, `POST /get-online-features` naming the 10 features and
1,000 entities) and to chloedb-server (the two `/v1/kv/get` calls), on the
same machine as the clients:

| how | clients | requests/s | p50 ms | p99 ms |
|---|---:|---:|---:|---:|
| Feast feature server, 4 workers, Redis | 1 | 11.2 | 75.9 | 171.2 |
| | 4 | 39.8 | 83.3 | 188.9 |
| | 16 | 32.3 | 400.6 | 553.0 |
| ChloeDB `/v1/kv/get` | 1 | 189.6 | 5.1 | 6.8 |
| | 4 | 389.9 | 9.8 | 17.7 |
| | 16 | 381.5 | 39.2 | 78.0 |

Both return the same values for the first batch. Feast's server tops out at
about 40 requests a second (40,000 entity rows) on the 4 cores; ChloeDB at
about 390.
