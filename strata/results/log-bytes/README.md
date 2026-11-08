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
| `otlp.jsonl` | `otlp.sh`: each log sent to `strata-server` as OpenTelemetry exporters send it (`otlp_send.py`: OTLP/HTTP JSON, gzip, 5,000 records a request, the pid a record attribute), into the table `otel_logs` that OTLP ingest makes (no key, `compression = 'zstd'`), measured merged by `examples/log_bytes.rs --table otel_logs` |
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
