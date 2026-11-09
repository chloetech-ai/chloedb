# Bytes per log line

How many bytes a log line takes in Strata and in log and search stores, on
four of [Loghub](https://github.com/logpai/loghub)'s logs: SSH (655K lines),
Android (1.56M), BGL (4.75M) and HDFS (11.2M). `parse.py` turns each into
JSON lines of `ts` (microseconds), `host`, `service`, `severity`, `pid` and
`body`.

| file | what |
|---|---|
| `strata-NAME.json` | `examples/log_bytes.rs`: a table `logs (id INT PRIMARY KEY, ts TIMESTAMP NOT NULL, host KEYWORD, service KEYWORD, severity KEYWORD, pid INT, body TEXT)` merged to one segment, with bytes per line by section |
| `strata-NAME-zstd.json` | the same table with `OPTIONS(compression = 'zstd')` |
| `strata-NAME-nopos.json` | the same with `body TEXT OPTIONS(positions = false)` |
| `strata-NAME-packed.json` | the `-zstd` table again, with positions in the packed layout (`text.body.ppos`), as tables are created from the commit that adds this file |
| `otlp.jsonl` | `otlp.sh`: each log sent to `strata-server` as OpenTelemetry exporters send it (`otlp_send.py`: OTLP/HTTP JSON, gzip, 5,000 records a request, the pid a record attribute), into the table `otel_logs` that OTLP ingest makes (no key, `compression = 'zstd'`), measured merged by `examples/log_bytes.rs --table otel_logs` |
| `otlp-packed.jsonl` | `otlp.sh` again (`OUT=otlp-packed.jsonl`), with packed positions |
| `phrase-reads.jsonl` | `examples/phrase_reads.rs` on HDFS's `-zstd` database (positions as varints) and a `-packed` one: phrase queries counted and their first ten rows, and the same terms without the phrase, median of 7 runs, two rounds in turn, read-only and without the filter cache |
| `store-reads.jsonl` | `examples/store_reads.rs` on each database: reads of one row by key at random keys, and a scan that reads every row's body |
| `opensearch.jsonl` | `opensearch.py`: OpenSearch 3.3.0 (opensearch-min, 4 GB heap) with the same fields, `_id` the line's id, one shard force-merged to one segment, codecs `default` and `best_compression`, with bytes per line by Lucene file |
| `clickhouse.jsonl` | `clickhouse.sh`: ClickHouse 25.8 in the table the OpenTelemetry Collector's exporter creates, after `OPTIMIZE ... FINAL` |
| `victorialogs*.jsonl` | `victorialogs.sh`: VictoriaLogs, by host and service stream and by service stream alone |
| `parquet.jsonl` | `parquet_zstd.py`: the JSON lines as Parquet with zstd level 3 |

All measured on 2026-11-08 on a cloud container: 4-core Intel Xeon @
2.10GHz, 15 GB of RAM. Strata's runs before `-zstd` were at main 02f3304;
the `-zstd` runs, `store-reads.jsonl` and `otlp.jsonl` at branch
`claude/project-thread-t79j93-store` (`otlp.jsonl` at b56543a, built with
`--profile ci`; its `send.seconds` time the Python sender, not the server).
The `-packed` runs, `otlp-packed.jsonl` and `phrase-reads.jsonl` are at the
commit that adds them, built with `--profile ci`, on the same container.

Bytes per line, merged:

| log | Strata `-zstd` (key `id`) | Strata through OTLP (`otel_logs`) | OpenSearch `best_compression` | OpenSearch `default` |
|---|---|---|---|---|
| SSH | 57.6 | 44.6 | 55.8 | 71.0 |
| Android | 77.9 | 48.2 | 83.1 | 112.2 |
| BGL | 54.4 | 36.3 | 55.4 | 70.8 |
| HDFS | 69.2 | 49.7 | 67.9 | 92.1 |

OpenSearch keeps each line's `_id` (here the line's number); `otel_logs`
has no key, which is most of the difference with Strata's `-zstd` table
(its key index took 16.6 B/line on HDFS).

## Packed positions

Positions took the most bytes after the doc store. Written packed (each
block of 128 postings bit-packed at its width over its least delta, frame of
reference), they take about what Lucene's `.pos` takes, and phrase queries
cost the same.

| log | positions, varints | packed | Lucene `.pos` (OpenSearch `default`) | Strata `-zstd` | `-packed` | through OTLP | OTLP, packed | OpenSearch `best_compression` |
|---|---|---|---|---|---|---|---|---|
| SSH | 13.6 | 4.2 | 3.3 | 57.6 | 48.1 | 44.6 | 35.2 | 55.8 |
| Android | 10.0 | 5.0 | 3.8 | 77.9 | 72.9 | 48.2 | 42.9 | 83.1 |
| BGL | 6.9 | 1.3 | 1.8 | 54.4 | 48.8 | 36.3 | 29.8 | 55.4 |
| HDFS | 12.8 | 3.6 | 3.8 | 69.2 | 60.1 | 49.7 | 40.2 | 67.9 |

Bytes per line; the positions columns are the keyed tables' (through OTLP,
packed positions took 4.2, 4.8, 1.3 and 2.8). Phrase queries on HDFS
(`phrase-reads.jsonl`), counting every match, in ms, two rounds each:

| phrase | matches | varints | packed |
|---|---|---|---|
| `receiving block` | 3,436,843 | 222.4, 176.3 | 180.4, 221.9 |
| `blockmap updated` | 1,719,741 | 96.1, 94.0 | 95.3, 98.8 |
| `served block` | 428,726 | 29.6, 21.9 | 24.6, 23.0 |
| `verification succeeded` | 120,036 | 7.3, 5.5 | 7.5, 7.3 |

The rounds differ by as much as the layouts do.
