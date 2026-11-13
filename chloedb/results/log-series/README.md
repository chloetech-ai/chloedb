# Log tables over time

How a table of logs behaves as it grows and ages in ChloeDB: what merges
and retention cost while logs stream in, how the segments lie in time,
and how fast time-window counts, time buckets and percentiles answer.
Measured with `examples/log_series_bench.rs`:

```text
cargo build --release --example log_series_bench
log_series_bench stream DB gen:20000000:30 600 190    # 10 minutes, kept 190 s
log_series_bench load DB bgl.jsonl && log_series_bench query DB 7
log_series_bench ingest DB gen:20000000:30 4          # writers at once, background merges
log_series_bench rollup DB 7                          # a rollup of a loaded table
```

| file | what |
|---|---|
| `baseline.jsonl` | main at f46bfb7f (the harness at 3a3003c9) |
| `step1-windows.jsonl` | merges within windows of time for tables with `retain` (this branch) |
| `step6-rollups.jsonl` | ingest with 1, 2 and 4 writers; rollups (this branch, on a91f4601 with the rollup changes uncommitted) |

All measured on 2026-11-13 on a cloud container: 4-core Intel Xeon @
2.10GHz, 15 GB of RAM, release builds.

**Data.** Loghub's BGL (4.75M lines over about 7 months) and HDFS (11.2M
over 2 days), parsed by `../log-bytes/parse.py`, and a synthetic log
(`gen:LINES:DAYS`): lines spread over 30 days with a daily cycle, 500
hosts, 20 services, five severities, a log-normal latency (2 ms median)
and 2% of lines up to 10 minutes late.

## Streaming with retention

`stream` writes batches of 5,000 lines as OTLP requests come, each stamped
with the time it is written, for 10 minutes, into a table kept 190
seconds: thirty days of retention compressed into minutes. It flushes
every 10,000 rows and runs the merges the default policy selects after
each flush. 2% of lines are late by up to a tenth of the retention.

| | lines written | lines/s | write amplification | rows rewritten per row | merge seconds | rows past the cutoff still on disk | segments | overlap |
|---|---|---|---|---|---|---|---|---|
| main, no retention | 54.5M | 90,824 | 3.38 | 2.91 | 272 | — | 35 | 1.26 |
| main, `retain = '190 seconds'` | 51.8M | 86,393 | 3.44 | 2.99 | 274 | 964,164 | 14 | 2.00 |
| windows, `retain = '190 seconds'` | 61.5M | 102,457 | 2.58 | 1.91 | 194 | 408,711 | 60 | 6.08 |
| windows, also `cluster_by = 'ts'` | 59.7M | 99,498 | 2.40 | 1.70 | 182 | 338,048 | 139 | 13.14 |

Write amplification is bytes flushed and merged per byte flushed; overlap
is the sum of the segments' time spans over the span of all of them (1
when they tile it).

With windows a kept table rewrites each row about once less (1.91 times
against 2.99), spends 29% less time merging, writes 19% more lines in the
same 10 minutes, and keeps 58% fewer expired rows on disk. It holds more
segments (60 against 14), each of one window (6.3 s here, a thirtieth of
the retention). Overlap goes up because a segment's late rows reach back
into older windows: here they are late by up to three windows, which a
real table kept 30 days (windows of a day, lines late by minutes) does
not see. `cluster_by = 'ts'` saves a little more rewriting, but more than
doubles the segments, so `otel_logs` does not use it.

## Queries

`load` writes the lines with their own times, moved so the newest is now,
in the same batches and merges, without retention; `query` runs each
query 7 times (median and 95th percentile ms; the 95th is the first, cold
run). "Segments in window" counts the segments whose newest row is inside
the query's window.

Main at f46bfb7f:

| log | query | rows in window | median ms | p95 ms | segments in window |
|---|---|---|---|---|---|
| synthetic 100M | `COUNT(*)` last hour | ~140K | 0.07 | 13.85 | 3 of 58 |
| synthetic 100M | `COUNT(*)` last day | ~3.3M | 0.07 | 0.38 | 7 of 58 |
| synthetic 100M | `date_trunc('hour')` counts, 7 days | ~23M | 944 | 965 | 19 of 58 |
| synthetic 100M | `date_trunc('minute')` by severity, 24 h | ~3.3M | 156 | 207 | 7 of 58 |
| synthetic 100M | top 10 hosts by errors, 24 h | ~3.3M | 3.5 | 170 | 7 of 58 |
| synthetic 100M | `MATCH(body, 'error')`, last hour, newest 100 | ~140K | 0.55 | 359 | 3 of 58 |
| synthetic 100M | p99 latency per hour and service, 24 h (exact) | ~3.3M | 236 | 323 | 7 of 58 |
| synthetic 100M | p50, p95, p99 latency, 24 h (exact) | ~3.3M | 413 | 441 | 7 of 58 |
| HDFS 11.2M | `date_trunc('hour')` counts, 7 days | 11.2M | 437 | 466 | 18 of 18 |
| HDFS 11.2M | `date_trunc('minute')` by severity, 24 h | most of it | 340 | 416 | 17 of 18 |
| BGL 4.75M | `date_trunc('hour')` counts, 7 days | small | 0.47 | 2.53 | 1 of 13 |

Counts over a time window are answered from the range index in well under
a millisecond. A time-bucket `GROUP BY` reads every row of the window, at
about 40 ns a row, and exact percentiles about 70 to 120 ns a row: those
are the costs `date_bin` and approximate percentiles go after. Segments
written in arrival order already tile time (overlap 1.00 to 1.01 after
`load`), and a window's count costs well under a millisecond with 58
segments, the zone maps passing over the others' blocks, so skipping
segments by time up front (step 2 of the plan) has little to win on
local segments; it was not built.

## Ingest

`ingest` writes the 20M synthetic lines as a server does: writers sending
batches of 5,000 at once, the flusher and merger in the background.

| writers | lines/s, written | lines/s, merges settled | segments |
|---|---|---|---|
| 1 | 221,221 | 217,497 | 21 |
| 2 | 223,601 | 222,212 | 26 |
| 4 | 210,753 | 209,563 | 26 |

About 220,000 lines a second, the same with one writer as with four: the
writer applies one group of writes at a time, and one thread makes the
lines. `load`, which merges after each flush on its own thread, writes
80,000 a second (100M lines in 1,256 s); its slowness was the harness,
not the database.

## Rollups

`rollup` makes `CREATE ROLLUP logs_5m ON logs AS SELECT date_bin('5
minutes', ts), service, severity, COUNT(*), SUM(latency), MIN(latency),
MAX(latency)` on a loaded table, then queries it beside the same query of
the table, 7 runs each. The answers were equal wherever both ask the same
(by severity the table's query is by minute, the nearest it has without
`date_bin`).

| log | query | rollup ms | table ms |
|---|---|---|---|
| synthetic 100M | hourly counts, 7 days | 9.17 | 876 |
| synthetic 100M | counts by 5 minutes and severity, 24 h | 6.01 | 156 (by minute) |
| synthetic 100M | mean latency per hour and service, 24 h | 4.36 | 129 |
| synthetic 100M | greatest latency per hour, 24 h | 2.98 | 149 |
| HDFS 11.2M | hourly counts, 7 days | 0.43 | 396 |
| HDFS 11.2M | mean latency per hour and service, 24 h | 0.77 | 321 |

Keeping it, on the 100M lines (838,404 rows of rollup, 8,648 buckets):

| | seconds |
|---|---|
| first fold, `CREATE ROLLUP` | 251 |
| refresh, nothing new | 0.011 |
| refresh after 50,000 new lines in the last minute | 1.61 |
| full refresh, every bucket compared | 7.1 |

The first fold runs a query per bucket, about 29 ms each: folding many
buckets in one query waits for `date_bin` in the columnar GROUP BY. The
refresh after new lines is the first query over rows still in memory;
repeated by hand in the same container it took 18 ms.
