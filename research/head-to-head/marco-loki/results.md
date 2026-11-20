# Marco: a day of logs, ChloeDB against Loki

Marco, a platform engineer in the persona study, runs Loki for most of a hundred services' logs and an old Elasticsearch for the logs people search by substring. In his adoption answer he said an on-call grep for one address over the last minutes "took tens of seconds" in Loki where ChloeDB took milliseconds, and that ChloeDB's disk "must come within about 2x of Loki's cost per day for the same lines, counted from the file, not the UI figure". Both were from memory. This page measures them on one machine.

- **The lines:** 3,600,007 made-up lines, 1,000 a second for an hour, the unit Marco sizes disk in ([lines.py](lines.py)): 1,801,051 edge nginx access lines, 1,438,953 JSON lines from twelve services with trace ids (each service spelling its levels its own way), and 360,003 network syslog lines. 610 MB of text, 169 bytes a line; gzip makes it 104 MB (29.0 bytes a line). The last fifteen minutes hold an incident: one address hammering the login page and trying ssh, 503s on two edge hosts, timeouts in checkout.
- **ChloeDB** takes them through `POST /v1/logs` as the OpenTelemetry Collector sends them (OTLP JSON, 2,000 records a request, 4 senders, a 429 or 503 sent again after its Retry-After). Twice: into the logs guide's table (a substring index on `host` and `body`, `trace_id` kept, zstd, 7 days), and into the table `/v1/logs` makes on its own (no substring index).
- **Loki** 3.5.5 runs as one process with its chunks and index on the local disk (Loki's single-binary start, TSDB index, schema v13; [loki.yaml](loki.yaml) raises only the ingestion and query limits and turns its results cache off). The same lines go to `/loki/api/v1/push` in the same requests, with labels job, service, host and level, and trace ids as structured metadata, as Loki's own OTLP endpoint keeps them. Twice: with its default chunk encoding (snappy) and with zstd.
- **The questions** are six an on-call engineer asks, in SQL and in LogQL, each run 7 times; the page gives the median and the slowest. Each answer is checked against the lines themselves. Loki answered them twice: live, from its ingester's memory, as it answers for the last minutes, and after a flush, from its chunks on disk, as it answers for older lines.

Measured on 2026-11-20 on Intel(R) Xeon(R) Processor @ 2.10GHz, 4 cores, 15 GB, Linux 6.18.44-fc-v726; chloedb-server 0.3.9 (built from abfa8a3651); Loki loki:3.5.5. Run it again with `python3 run.py && python3 report.py` (needs Docker and a chloedb-server build; `H2H_ONLY=chloedb-guide` and so on runs one). Raw numbers: [raw/results.json](raw/results.json).

## What it says

- **Every answer is right on every system:** ChloeDB, the guide's table agrees, ChloeDB, the table /v1/logs makes agrees, Loki, default (snappy) agrees, Loki, zstd chunks agrees with the lines. Every line sent was stored once: ChloeDB, the guide's table 3,600,007 of 3,600,007, ChloeDB, the table /v1/logs makes 3,600,007 of 3,600,007, Loki, default (snappy) 3,600,007 of 3,600,007, Loki, zstd chunks 3,600,007 of 3,600,007.
- **The grep for one address,** over the last 15 minutes (1,292 lines of 900,000): ChloeDB with the guide's table 49.2 ms at the median, Loki 1,037.8 ms live and 1,057.0 ms from its chunks. So ChloeDB is about 21 times faster here, and Loki answers in about a second, not the tens of seconds Marco remembered, at this volume on one machine.
- **Without the substring index it is the other way round.** On the table `/v1/logs` makes by itself, the same grep took 23.3 s at the median, and sent as a page sends it, with the server's default deadline of 5 s, it was refused (504 `deadline_exceeded`). The `ORDER BY ts DESC` sends it to the SQL engine that reads every line of the table before the `LIKE`; without the `ORDER BY` the same `LIKE` is a scan of the table in a fraction of that. A team that points a Collector at `/v1/logs` and greps gets this table unless it makes the guide's first.
- **The other questions.** ChloeDB with the guide's table is faster on 6 of the 6 against Loki live. The table below has each.
- **Disk, counted from the files.** For the hour: ChloeDB's guide table 1,233 MB (343 bytes a line, 29.6 GB a day at 1,000 lines a second), the table `/v1/logs` makes 544 MB (151 bytes a line); Loki's chunks and index 277 MB with snappy (77 bytes a line, 6.6 GB a day) and 189 MB with zstd (52 bytes a line). Marco's bar is 2x Loki: the guide's table is 4.5x Loki's default and 6.5x Loki with zstd; the table without substring indexes is 2.0x and 2.9x.
- **ChloeDB's file right after the hour** was 1,438 MB with 355 MB of it free (segments merged while the lines came in); the server gave space back by itself and the file was 1,231 MB 5 minutes later, the size counted above. The tables' live data is 1,079 MB (300 bytes a line).
- **Loki's write-ahead log** held another 772 MB on the ingester's local disk for the hour. Loki keeps its chunks in object storage in production and the log on the ingester; ChloeDB keeps both on its own disk. The figures above are the chunks and index only, Loki's best case.
- **Taking the lines in.** ChloeDB took the hour at 32,048 lines a second into the guide's table and 38,051 into its own, Loki at 233,767, from the same 4 senders. Requests sent again after a 429 or 503: ChloeDB 0, Loki 0.
- **Memory after the hour:** ChloeDB 1,914 MB, Loki 1,118 MB (its ingester holds the hour's chunks until they flush).

## Each question

Median of 7 runs, and the slowest in brackets, in milliseconds, from the client (a query slower than a minute ran fewer times). ChloeDB keeps the filters it worked out for a while, so its first run is usually its slowest; Loki's chunk cache does the same for its reads. Every answer was checked; a wrong one would say so.

| question | answer | ChloeDB, the guide's table | ChloeDB, the table /v1/logs makes | Loki, default (snappy) | Loki, zstd chunks | Loki snappy, after flush |
|---|---|---|---|---|---|---|
| Every line with one client address, last 15 minutes | 1,292 lines | 49.2 (52.0) | 23,286.2 (23,695.8) | 1,037.8 (1,096.4) | 1,626.6 (1,681.5) | 1,057.0 (1,127.1) |
| Every line of one trace, last hour | 7 lines | 1.9 (2.6) | 2.2 (2.6) | 1,640.4 (2,429.8) | 2,005.0 (3,498.2) | 1,681.3 (1,787.0) |
| Errors per service per minute, last 15 minutes | 210 values | 4.7 (5.3) | 6.3 (6.7) | 73.9 (89.8) | 74.2 (118.7) | 57.1 (77.3) |
| 503s per edge host, last hour | 12 values | 4.4 (43.8) | 1,676.8 (2,119.1) | 896.1 (962.3) | 856.1 (931.3) | 605.0 (725.7) |
| Lines that say timeout, in any case, last 15 minutes | 6,074 lines | 5.4 (20.0) | 588.5 (856.2) | 723.2 (831.9) | 650.2 (800.1) | 556.1 (678.3) |
| The newest 100 lines of checkout, last hour | 100 lines | 2.0 (3.7) | 2.4 (5.7) | 149.9 (190.9) | 95.5 (104.6) | 35.8 (45.6) |

## Disk for the hour

| system | bytes | bytes a line | GB a day at 1,000 lines a second | against Loki's default |
|---|---|---|---|---|
| ChloeDB, the guide's table | 1,233.1 MB | 343 | 29.6 | 4.45x |
| ChloeDB, the table /v1/logs makes | 544.0 MB | 151 | 13.1 | 1.96x |
| Loki, default (snappy) | 277.0 MB | 77 | 6.6 | 1.00x |
| Loki, zstd chunks | 188.9 MB | 52 | 4.5 | 0.68x |
| the lines as text | 609.9 MB | 169 | 14.6 | 2.20x |
| the lines gzipped | 104.3 MB | 29.0 | 2.5 | 0.38x |

ChloeDB's figure is its database folder after the server stopped (the file and its log); Loki's is its chunks and index after a flush, without its write-ahead log. A day is the hour times 24: a real day's lines repeat more, which helps both. ChloeDB's file also holds free space, here from segments merged while the lines came in, until the server gives it back; the figure is after it had.

## The queries

| question | SQL (the guide's table) | LogQL |
|---|---|---|
| Every line with one client address, last 15 minutes | `SELECT body FROM logs_edge WHERE body LIKE '%203.0.113.77%' AND ts > '2026-11-20T15:55:00.000000Z' AND ts <= '2026-11-20T16:10:00.000000Z' ORDER BY ts DESC LIMIT 5000` | `{job=~".+"} \|= "203.0.113.77"` |
| Every line of one trace, last hour | `SELECT body FROM logs_edge WHERE trace_id = '5b8efff798038103d269b633813fc60c' AND ts > '2026-11-20T15:10:00.000000Z' AND ts <= '2026-11-20T16:10:00.000000Z' ORDER BY ts` | `{job="app"} \|= "5b8efff798038103d269b633813fc60c"` |
| Errors per service per minute, last 15 minutes | `SELECT time_bucket('1 minute', ts) AS minute, service, count(*) AS errors FROM logs_edge WHERE severity_number >= 17 AND ts > '2026-11-20T15:55:00.000000Z' AND ts <= '2026-11-20T16:10:00.000000Z' GROUP BY minute, service` | `sum by (service) (count_over_time({level="ERROR"}[1m]))` |
| 503s per edge host, last hour | `SELECT host, count(*) AS n FROM logs_edge WHERE service = 'edge-nginx' AND body LIKE '%" 503 %' AND ts > '2026-11-20T15:10:00.000000Z' AND ts <= '2026-11-20T16:10:00.000000Z' GROUP BY host` | `sum by (host) (count_over_time({job="nginx"} \|= "\" 503 " [1h]))` |
| Lines that say timeout, in any case, last 15 minutes | `SELECT count(*) FROM logs_edge WHERE body ILIKE '%timeout%' AND ts > '2026-11-20T15:55:00.000000Z' AND ts <= '2026-11-20T16:10:00.000000Z'` | `sum(count_over_time({job=~".+"} \|~ "(?i)timeout" [15m]))` |
| The newest 100 lines of checkout, last hour | `SELECT body FROM logs_edge WHERE service = 'checkout' AND ts > '2026-11-20T15:10:00.000000Z' AND ts <= '2026-11-20T16:10:00.000000Z' ORDER BY ts DESC LIMIT 100` | `{service="checkout"}` |

One machine, one server of each, and an hour of lines: not a day's volume, not Loki's object storage or its caches across a cluster, and not ChloeDB's cold store. The two share four cores with the senders.
