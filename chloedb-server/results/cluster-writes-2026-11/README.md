# Writes on a cluster, and updates against SQLite (19 November 2026)

Measured on DigitalOcean (sfo3), 19 November 2026 between 7:45 and 9:50 PM PST,
on a DOKS cluster made for the run and destroyed after it:

- three store nodes, `c-16` (16 dedicated vCPUs, 32 GB), each running one copy of
  every shard, so a shard has a primary and two replicas, each on its own node;
- one coordinator node, `c-16`;
- one client node, `g-8vcpu-32gb` (Intel Xeon Platinum 8280 @ 2.70GHz), running
  `chloedb-write-bench` against the coordinator, or straight to the shards'
  primaries with `--direct`, and the embedded benchmarks.

All shards of a run share the same three store nodes, so 8 shards are 8 writers on
3 machines, not on 8. Numbers move by 10-30% between two runs of the same setup;
read differences under that as noise.

Files:

| file | what |
|---|---|
| `matrix-0.3.8.jsonl` | 0.3.8 (main): 1, 2, 4 and 8 shards, `--write-acks all`, `majority` and `primary`, 1 to 256 clients. Entries with `rows_per_s` 0 are setups that failed (the "no such table" bug below); when a configuration appears twice, the later one counts |
| `before-after.jsonl` | 0.3.8 against this branch on 1 and 4 shards with `--write-acks majority`, and the branch straight to the primaries |
| `embedded.txt` | the embedded runs on the client node, verbatim: `a1_updates.py`, `a1_add_op.py`, `a1_sustained.py` and `chloedb/examples/update_paths.rs` |

The scripts: `chloedb-server/src/bin/chloedb-write-bench.rs` (the cluster),
`chloedb/examples/update_paths.rs` (embedded, from Rust) and
`chloedb-py/bench/laptop/a1_updates.py`, `a1_add_op.py`, `a1_sustained.py`
(embedded, from Python, A1's statements against SQLite).

## Shards and acknowledgements (0.3.8)

Best rows a second over 1 to 256 clients, with the clients at the best and the
median latency there. Only runs with no errors count.

Single-row inserts (`insert`):

| shards | all | majority | primary |
|---|---|---|---|
| 1 | 7,222 (64, 5.0 ms) | 7,955 (64, 4.4 ms) | 7,916 (256, 10.1 ms) |
| 2 | 7,273 (64, 4.3 ms) | 10,377 (64, 3.6 ms) | 9,751 (64, 2.4 ms) |
| 4 | 9,320 (64, 3.7 ms) | 10,892 (64, 3.2 ms) | 12,192 (64, 2.1 ms) |
| 8 | 10,819 (64, 3.6 ms) | 13,179 (64, 3.2 ms) | 14,387 (64, 1.9 ms) |

Batches of 100 rows per request (`batch:100`):

| shards | all | majority | primary |
|---|---|---|---|
| 1 | 27,474 (64, 174.1 ms) | 26,733 (16, 43.6 ms) | 26,695 (16, 38.8 ms) |
| 2 | 39,807 (64, 112.8 ms) | 35,582 (64, 105.6 ms) | 35,004 (16, 29.3 ms) |
| 4 | 50,914 (64, 76.2 ms) | 38,458 (64, 80.3 ms) | 39,605 (16, 26.7 ms) |
| 8 | 37,470 (16, 32.9 ms) | 38,370 (16, 28.8 ms) | 40,791 (16, 26.1 ms) |

Streams of 100-row batches (`stream:100`, the latency is a whole stream's):

| shards | all | majority | primary |
|---|---|---|---|
| 1 | 46,072 (16, 1426.7 ms) | 45,053 (16, 1421.9 ms) | 41,978 (16, 1523.0 ms) |
| 2 | 60,937 (16, 1026.6 ms) | 63,476 (16, 970.4 ms) | 56,573 (64, 2701.1 ms) |
| 4 | 106,995 (16, 595.3 ms) | 105,461 (16, 582.2 ms) | 104,428 (16, 613.9 ms) |
| 8 | 153,075 (16, 389.7 ms) | 143,692 (64, 1588.8 ms) | 155,129 (16, 363.9 ms) |

What this says:

- Streams scale with shards: 46k rows a second on one shard, 153k on eight. They
  are the way to load a cluster. At 256 streams the shards answered 503, "deadline
  passed while waiting for the writer".
- Batches through the coordinator stop scaling at about 35-50k rows a second from
  2 shards on: a batch that spans shards pays the validate round and waits for its
  slowest shard. Straight to the primaries they go further (below).
- Single rows scale about 2x from 1 to 8 shards. One write through the cluster
  takes 2-3 ms (coordinator, primary, a replica's acknowledgement), which caps each
  client at a few hundred a second; more clients fill the shards.
- `majority` costs little against `primary` and is clearly faster than `all` for
  single rows (13.2k against 10.8k on 8 shards).

## Before and after (majority)

Rows a second, with the median latency in ms. 0.3.8 is main at
`chloedb-server-v0.3.8`; the branch is this pull request. `sql-update` is
`UPDATE wb SET price = :p WHERE id = :k`, `sql-add` is `SET n = n + 1` on a random
row and `sql-hot-add` on one of 16 rows; `add` and `hot-add` are the same through
`/v1/write`'s `add` operation.

1 client:

| workload | 1 shard, 0.3.8 | 1 shard, branch | 4 shards, 0.3.8 | 4 shards, branch | 4 shards, branch, straight to primaries |
|---|---|---|---|---|---|
| sql-update | 361 (2.7) | 415 (2.4) | 333 (2.9) | 383 (2.6) | 333 (1.9) |
| sql-add | 307 (2.8) | 327 (2.5) | 311 (2.9) | 343 (2.7) | 355 (1.9) |
| sql-hot-add | 270 (2.8) | 282 (2.6) | 300 (2.9) | 322 (2.7) |  |
| add | 342 (2.0) | 304 (2.1) | 332 (2.5) | 331 (2.4) |  |
| hot-add | 308 (2.1) | 259 (2.2) | 308 (2.4) | 300 (2.4) |  |
| insert | 247 (2.2) | 214 (2.2) | 257 (2.6) | 244 (2.6) | 457 (1.6) |
| batch:100 | 7,049 (5.6) | 5,612 (5.6) | 8,274 (5.5) | 7,436 (5.6) | 13,209 (3.2) |

16 clients:

| workload | 1 shard, 0.3.8 | 1 shard, branch | 4 shards, 0.3.8 | 4 shards, branch | 4 shards, branch, straight to primaries |
|---|---|---|---|---|---|
| sql-update | 4,373 (3.4) | 4,923 (3.0) | 3,788 (4.0) | 4,151 (3.6) | 3,131 (2.1) |
| sql-add | 3,371 (3.5) | 3,251 (3.2) | 3,231 (4.1) | 3,461 (3.7) | 3,472 (2.2) |
| sql-hot-add | 2,671 (3.7) | 2,835 (3.2) | 3,076 (4.0) | 3,110 (3.7) |  |
| add | 2,655 (2.7) | 2,701 (2.7) | 4,018 (2.4) | 3,930 (2.4) |  |
| hot-add | 3,009 (2.7) | 2,264 (2.8) | 3,485 (2.4) | 4,117 (2.3) |  |
| insert | 2,151 (2.8) | 1,572 (2.8) | 3,322 (2.5) | 2,128 (2.5) | 3,885 (1.9) |
| batch:100 | 14,086 (49.5) | 12,349 (44.7) | 19,248 (33.7) | 22,246 (32.2) | 39,834 (18.8) |

64 clients:

| workload | 1 shard, 0.3.8 | 1 shard, branch | 4 shards, 0.3.8 | 4 shards, branch | 4 shards, branch, straight to primaries |
|---|---|---|---|---|---|
| sql-update | 6,208 (7.1) | 7,707 (5.1) | 6,107 (7.9) | 6,873 (7.2) | 4,119 (2.9) |
| sql-add | 4,071 (7.5) | 5,229 (5.3) | 4,132 (8.5) | 3,756 (7.5) | 4,207 (2.9) |
| sql-hot-add | 2,717 (10.3) | 3,808 (5.4) | 4,440 (8.4) | 4,321 (7.3) |  |
| add | 2,280 (5.2) | 4,410 (4.8) | 6,081 (3.3) | 6,603 (3.3) |  |
| hot-add | 2,898 (4.2) | 1,915 (6.9) | 3,442 (3.4) | 5,142 (3.3) |  |
| insert | 1,386 (6.1) | 2,428 (6.2) | 3,611 (3.5) | 2,332 (3.4) | 6,669 (2.5) |
| batch:100 | 8,223 (188.3) | 8,024 (225.4) | 19,059 (73.2) | 14,224 (92.8) | 39,806 (42.8) |

What this says:

- On one shard, keyed SQL updates gained 24-40% at 64 clients (6,208 to 7,707;
  4,071 to 5,229; 2,717 to 3,808), from the keyed `UPDATE` path and from updates of
  one row sharing a commit. Through the cluster the 2-3 ms of a round trip with
  replication hides most of what the shard saves.
- Straight to the primaries, batches ran 1.8-2.8x what they ran through the
  coordinator on 4 shards (39.8k rows a second against 14-22k) and single-row
  inserts 2.9x at 64 clients, with lower medians everywhere. Keyed SQL statements
  straight to the primaries had lower medians but no more throughput at 64
  clients, with a p99 of about 0.5 s; that is not understood yet.
- These runs are noisier than the matrix: the matrix's `insert` on 4 shards at 64
  clients reached 10.9k rows a second, these 2.3-3.6k, on the same nodes with the
  same binary for 0.3.8. The before-after runs set up 100,000 rows and ran the
  update workloads first.

## Embedded updates against SQLite

A1's update statements (`chloedb/results/laptop-2026-11`), one row per
transaction, on 137,260 products, from Python with `a1_updates.py` on the client
node (Python 3.13.5, SQLite 3.46.1). ChloeDB syncs every commit; SQLite is
`synchronous=FULL` in WAL mode, as in A1. Two rounds of the branch and SQLite,
in turns; the raw lines are in `embedded.txt`:

| | price and stock update a second | counter + 1 a second |
|---|---|---|
| SQLite FULL | 6,643 / 6,771 | 10,886 / 13,238 |
| ChloeDB 0.3.8 (PyPI), one round | 3,500 | 3,782 |
| ChloeDB, this branch, `db.sql` | 7,867 / 8,531 | 8,498 / 8,978 |
| ChloeDB, this branch, `db.write` `add` op | | 15,687 |

From Rust (`update_paths`, in `embedded.txt`), syncing: `UPDATE ... SET price` 9,284
to 10,481 a second, `SET n = n + 1` 11,441 to 12,233, the `add` operation 16,966 to
18,275. A commit's `fdatasync` over zeros written ahead takes about 50 µs on this
disk.

Price updates now beat SQLite FULL from Python. A counter bumped by SQL from Python
is still below SQLite (8.5-9k against 11-13k); the `add` operation beats it.

Sustained inserts, 1,000 review rows a transaction for 150 s (`a1_sustained.py`):
ChloeDB about 44,760 rows a second on this branch and 45,330 on 0.3.8, flat
through flushes and merges; SQLite FULL 17,104, falling from about 25k to 15.7k as
its indexes grew.

## Counter + 1 by SQL, again (19 November, 11:14-11:26 PM PST)

The SQL counter above was still behind SQLite. Since then, an `UPDATE` whose
`WHERE` is its key alone is bound without the `SELECT *` the general path binds
for its rows (it never reads them), skips the checks for ChloeDB's own
statements (none starts with `UPDATE`), and a write alone in the queue takes no
footprint. The CPU a statement takes, here in the container with no sync, fell
from 7.3 to 3.3 µs (the `add` operation's is 1.7 µs).

Measured again on one DOKS `g-8vcpu-32gb` node in ams3 (Intel Xeon Platinum
8358 @ 2.60GHz, the machine type of A1's runs of 17 November; an `fdatasync`
takes 41 µs; sfo3 had no capacity, and sfo2's node, whose syncs took 1.4 ms,
was destroyed unused). The same `a1_updates.py`, 4 s a cell, the systems in
turns, two runs of three rounds (raw lines in `counter-2026-11-20.txt`):

| | price and stock update a second | counter + 1 a second |
|---|---|---|
| SQLite FULL (6 rounds) | 9,796-10,341 | 16,842-17,197 |
| ChloeDB, this change (3 rounds) | 15,855-16,350 | 17,654-17,951 |
| ChloeDB, the change without the binder's last two cuts (6 rounds) | 15,518-16,083 | 17,027-17,568 |
| ChloeDB, PR #708 as first pushed (3 rounds) | 13,063-13,515 | 14,190-14,687 |
| ChloeDB 0.3.8 (1 round) | 5,159 | 5,775 |
| ChloeDB, `db.write` `add` op | | 20,298-20,636 |

The counter by SQL is now ahead of SQLite FULL in each of its rounds, by 4-5%: each
commit waits about 41 µs for its sync, so what is left between them is a few
microseconds of work a statement. Price updates are 56-65% ahead.

## Bugs found on the way, fixed on this branch

- A table created and then written at once, under `--write-acks primary`, failed
  with "no such table" for about a second: the coordinator's catalog is the tables
  every copy holds, and the replicas had not applied the DDL yet. A write now asks
  the primaries when the table is missing.
- A write through a coordinator that spans shards and gives a MEDIA field as `data`
  or `uri` failed ("unknown field `data`") in the validate round.
