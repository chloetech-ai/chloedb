# date_bin and approx_percentile on 20M log lines

`date-bin-approx-20m.jsonl`: `log_series_bench load` of `gen:20000000:30`
(20M synthetic lines over 30 days, table options none), then `query DB 7`
(median of 7 warm runs), at commit 53602005 (`hourly_7d` took 254.7 ms on this data before buckets were found by number, at 3372f62c) on branch
claude/project-thread-wllzb8. Machine: the 4-core Xeon 2.1 GHz cloud
container that measured `baseline.jsonl`, release build.

| query | ms |
|---|---|
| `bins_5m_by_severity_24h` (`date_bin` on the columns) | 36.8 |
| `bins_5m_by_severity_24h_rows` (the same buckets through a WITH, as OpenSearch's `fixed_interval` ran before) | 1,449.5 |
| `hourly_7d` (`date_trunc('hour')`, its buckets found by number) | 53.8 |
| `p50_p95_p99_5m_by_service_24h_exact` / `_approx` | 120.5 / 106.4 |
| `p50_5m_by_service_24h_exact` / `_approx` (one fraction, groups of about 570 values) | 73.2 / 78.0 |
| `p50_p95_p99_30d_exact` / `_approx` | 2,172.3 / 375.7 |

Every `accuracy` line has `max_relative_error` at most 0.01 against
`PERCENTILE_DISC`.
