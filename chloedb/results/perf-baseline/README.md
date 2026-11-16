# Write-ahead log and flush baseline

`baseline.json` is what `tests/perf_baseline.rs` compares a run with: 127
metrics of how ChloeDB commits, logs, flushes and replays. Recorded on the
machine in `meta.machine`, one table of about 100-byte text rows, an optimised
build (`--profile ci`), one writer process.

```bash
cd chloedb
# correctness: 9 tests, part of the normal run (about 4 s)
cargo test --profile ci --test perf_baseline
# performance against the baseline (about 10 s; keep the machine quiet)
cargo test --profile ci --test perf_baseline -- --ignored --nocapture
# after an intended change: record a new baseline, and say in the PR why it moved
CHLOEDB_PERF=record cargo test --profile ci --test perf_baseline -- --ignored --nocapture
```

Latencies fail past 1.6x to 4x their baseline (p50 tightest, max loosest),
throughput below 1/1.6, sizes past 1.1x either way. Times under 0.05 ms and
counts of segments or memtable rows are given absolute slack, since they depend
on when a background thread ran. Repeated runs on the recording machine moved
by at most 1.6x, mostly in p99s of sub-0.1 ms operations. Re-record on another
machine rather than comparing across machines. The fsync numbers are the
noisiest: they are the disk's.

## The write-ahead log, as measured

The settings (`DbOptions`; defaults pinned by `defaults_are_pinned`):
`memtable_max_docs` 10,000, `sync` true, `memtable_chunk_docs` 1,024,
`memtable_tail_docs` 128, `segment_max_docs` 100,000, `write_wait` 5 s,
`wal_drop_damaged` false. The log has no size limit in bytes: the threshold is
rows (plus pending deletes), and a flush is due when it is passed.

| | |
|---|---|
| commit, no fsync | 4 us p50, 214k commits/s |
| commit, fsync, one row | 4.5 ms p50, about 210/s: the disk's flush |
| fsync, rows per commit 1 / 10 / 100 / 1000 | 206 / 1,956 / 16,100 / 99,200 rows/s |
| 8 threads, fsync, one row each | 1,020 rows/s: about 5 writes share a commit |
| log bytes per row | 157 (one-row commits), 137 (100-row commits), 674 (128-d vector) |
| flush of 10k rows | 39 ms explicit, 52 ms in the background; the segment is 0.56x the log |
| commits while a flush builds | p99 0.013 ms: they do not wait |
| replay of an unflushed log | 0.8 / 2.9 / 5.2 ms for 2k / 10k / 20k rows (about 4M rows/s) |
| first query after the replay | 5.4 / 24 / 49 ms for 2k / 10k / 20k rows |
| the same query on flushed segments | 0.1 ms |

The cost of leaving rows in the log is not the replay, which is cheap, but the
first query after it, which builds the memtable's in-memory indexes (about
2.3 us a row). A flush removes it. `memtable_max_docs` bounds it.

Flush threshold, 40,000 one-row commits with the flusher thread on (no fsync):

| `memtable_max_docs` | rows/s | commit max | segments at the end |
|---|---|---|---|
| 1,000 | 26,400 | 17 ms | 36 |
| 4,096 (what ChloeCtx sets) | 88,500 | 17 ms | 9 |
| 10,000 (default) | 155,000 | 20 ms | 3 |
| 50,000 | 248,500 | 0.7 ms | 0, all still in the log |

A low threshold costs throughput and makes many small segments for the merger
to combine. A high one leaves a longer log, a larger first query after a
reopen, and more memory.

## Merges, as measured

Eight flushed segments of 1,000 rows merged into one by `Database::merge` with
the default policy (`merge_factor` 8, `floor_docs` 1,000), one thread:

| | merge | rows/s | bytes written / bytes merged |
|---|---|---|---|
| text rows | 37 ms | 215k | 0.85 |
| text rows, 30% overwritten (`compact`) | 44 ms, 300 dropped | 174k | 0.84 |
| 128-d vectors, flat index | 54 ms | 149k | 0.97 |
| 128-d vectors, HNSW (m 16, ef 100, as ChloeCtx) | 474 ms | 16.9k | 0.97 |

| | |
|---|---|
| query over 8 segments / over the merged one | 0.016 / 0.008 ms p50 |
| commits during the HNSW merge (1,600 of them) | p99 0.02 ms, max 16.5 ms |
| flusher and merger threads, 40,000 one-row commits, flush at 4,096 | 93k rows/s, p99 0.008 ms, 9 segments at the end, 2 once the merger settles |

The graph is what makes a merge expensive: an HNSW merge costs ten times a flat
one for the same rows, because it rebuilds the graph rather than copying
vectors. Merges do not block commits (the install takes the writer lock for
a moment: the 16 ms max, which a flush shows too). Merging pays back in
reads (half the query time here) and in fewer segments to visit, not in space.

What it does not cover: several tables, replication (`seq_no`, shipped logs),
the directory layout, cold segments, and merges of large segments (the
memory budget, the spill to disk, the write-rate limit). The fault-injection tests
(`--features fault-injection`: `storage_faults`, `crash_paths`,
`background_flush`) are the correctness side for crashes in the middle of a
flush, a merge or a log write.

## Why `storage_faults` takes 80 minutes

It runs the same 80-operation workload (commits, flushes, background flushes,
merges, vacuums, bulk loads) once for every file-system call the workload
makes (about 400-500, an estimate from the code), failing that call, and
checks what a reopen finds. 13 such sweeps run at the default seeds: power
loss with and without sync, failed calls, a full disk, the directory layout
and merges that spill. A power-loss iteration reopens and runs `check()` on
3 crash images; a failed-call one re-runs the whole workload. So the work is
about calls squared, and about 5,500 iterations at 0.9 s each make the 80
minutes. Almost all of that is real syncs: on macOS Rust's `sync_data` and
`sync_all` are `F_FULLFSYNC`, about 20 ms on an SSD and 0.2 ms on a RAM disk
(see `scripts/ci-local.sh`); every flush syncs twice and every created
database several times. The other four fault binaries run each scenario once.

To shorten it: `CHLOEDB_FAULT_STRIDE=k` tries every k-th failure point (about
1/k of the time, but a given write or sync may go untried),
`CHLOEDB_FAULT_SEEDS=n` sets the number of seeds (it replaces the default; it
does not add to it, as the code comment says), `CHLOEDB_FAULT_OPS=n` shortens the
workload (the time falls about quadratically), and a RAM disk for the temp files
is the biggest lever on a Mac. CI runs it as its own job on Linux, with a
60-minute limit, and on macOS only on a schedule or by hand.
