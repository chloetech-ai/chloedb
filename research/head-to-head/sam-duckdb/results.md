# Sam: an analyst's SQL battery, ChloeDB against DuckDB and Postgres

Sam, an analytics engineer in the persona study, wants "a benchmark of our own models against DuckDB and Postgres (same results, run time)" before his data lead says yes. This page runs his round 4 battery ([queries.py](queries.py), 74 queries: joins of every kind, windows, QUALIFY, percentiles, set operations, dates, strings, and ten statements the docs say ChloeDB refuses) on made-up data of his shape ([gen.py](gen.py): green-taxi trips over three months with NULLs, refunds and stray dates, 263 zones, and a music store with the Chinook schema). Each engine answers the same SQL on one machine in turn; DuckDB's form of a statement differs only for timestamp literals, `strftime`'s argument order and `date_bin` (`time_bucket`), and Postgres runs DuckDB's form.

Measured on 2026-11-20 on Intel(R) Xeon(R) Processor @ 2.10GHz, 4 cores, 15 GB, Linux 6.18.44-fc-v726; chloedb-server 0.3.9 (built from 2f21f4dd0c); DuckDB 2.0.0; PostgreSQL 16.15 (Ubuntu 16.15-0ubuntu0.24.04.1). ChloeDB is loaded with `chloedb-server import` (Parquet and SQLite) and asked over HTTP; DuckDB in process with its default threads; Postgres through psycopg with its default settings. Each time is the median of 5 runs after one. Run it again with `python3 run.py && python3 run.py 2000000 && python3 report.py` (needs `pip install duckdb psycopg[binary]`, Postgres and a chloedb-server build). Raw numbers: [raw/results-205064.json](raw/results-205064.json), [raw/results-2000000.json](raw/results-2000000.json).

## What it says

- **205,064 trips.** ChloeDB returned DuckDB's rows, in DuckDB's order, for 64 queries; none differed; it refused 10 that DuckDB ran. Of the 64 both ran, ChloeDB's own time was 1.23x DuckDB's at the geometric mean, faster on 29; its slowest against DuckDB were t45_count_distinct_two (57.7 ms against 3.7), t31_derived (38.4 ms against 2.7), t11_date_diff (33.7 ms against 2.8), t06_not_exists (31.6 ms against 2.9).
- **2,000,000 trips.** ChloeDB returned DuckDB's rows, in DuckDB's order, for 62 queries; none differed; it refused 12 that DuckDB ran. Of the 62 both ran, ChloeDB's own time was 1.80x DuckDB's at the geometric mean, faster on 21; its slowest against DuckDB were t31_derived (388.6 ms against 4.1), t06_not_exists (329.7 ms against 6.2), t13_rank_qualify (395.8 ms against 9.9), t01_aggs (530.5 ms against 13.9).
- **What ChloeDB refuses** at 205,064 trips is what its docs list: t15b_rows_over_10000, x01_recursive_cte, x02_rollup, x03_grouping_sets, x04_is_distinct, x05_cross_join, x06_using, x07_comma_join, x08_keyword_range, x09_sum_distinct. Each refusal names what to write instead; the messages are below.
- **At 2,000,000 trips it refused 2 more:** t14_ntile_small (query error: the query matches more than 10000 rows, the most a query returns); t23_except (memory: the query needs more than the 4559 MiB the server lets queries hold at once (--query-memory-mb) (93 MiB of them in use, 123 MiB by this query)).
  t23's refusal comes and goes: on two fresh servers over the same file it was refused in five of six runs and answered in about 0.7 s once, and the numbers it gives are far under the limit it names. That is a finding of this comparison (SAM-411), not a limit the docs describe.
- **Postgres** ran 62 of the battery, gave DuckDB's rows for 59, and refused 12 for dialect (QUALIFY, MEDIAN, `date_diff`, `strftime`, `date_bin`, `ROUND(double, int)`). It differed on t44_order_desc_nulls, c14_text_group (t44: Postgres puts NULLs first on DESC, DuckDB and ChloeDB last).
- **Selective queries are ChloeDB's; full scans and joins are DuckDB's.** At 2,000,000 trips ChloeDB was faster on t07_in_subquery, t18_not_in_nulls, t19_limit_offset, t28_cast_round, t32_full_join, t34_right_join, t38_string_agg, t39_exists_semi, t42_interval, t44_order_desc_nulls, c02_country_having, c03_monthly_running, c04_self_join_mgr, c06_never_bought, c08_string_agg_customers, c10_dense_rank_spend, c11_year, c12_lag_gap, c13_exists_2, c16_in_list_case, c18_employee_ages: filters on a key or a range, LIMIT and OFFSET, NOT IN, semi joins, and the music store's small tables. It took more than five times DuckDB's time on t01_aggs, t04_join_group, t06_not_exists, t09_extract_dow_hour, t11_date_diff, t13_rank_qualify, t16_case_bucket, t20_distinct, t27_arith, t29_strftime, t30_cte_chain, t31_derived, t45_count_distinct_two: aggregates and joins that read every trip, as in round 4.

## Loading, memory and disk

| | 205,064 trips | 2,000,000 trips |
|---|---|---|
| Parquet file of the trips | 5.9 MB | 56.7 MB |
| ChloeDB: load the trips | 0.44 s | 5.73 s |
| ChloeDB: load everything | 0.50 s | 5.79 s |
| ChloeDB: disk | 22.5 MB | 344.0 MB |
| ChloeDB: peak memory (RSS) | 357 MB | 739 MB |
| DuckDB: load the trips | 0.15 s | 0.67 s |
| DuckDB: load everything | 19.22 s | 19.92 s |
| DuckDB: disk | 8.9 MB | 98.3 MB |
| DuckDB: peak memory (RSS) | 166 MB | 477 MB |
| Postgres: load the trips | 0.70 s | 7.36 s |
| Postgres: load everything | 0.95 s | 7.91 s |
| Postgres: disk | 29.3 MB | 269.5 MB |
| Postgres: memory after the battery (PSS) | 76 MB | 219 MB |

ChloeDB's load is three `chloedb-server import` commands (the trips' Parquet, the zones' Parquet, the music store's SQLite file), DuckDB's is `CREATE TABLE ... AS SELECT * FROM read_parquet(...)` and an insert of the SQLite rows, and Postgres's is `COPY` from the client. DuckDB's whole load is mostly inserting the music store's rows one statement at a time from Python; its load of the trips is the number to compare. Postgres's disk is its tables and indexes; DuckDB's and ChloeDB's, their files.

## Each query

*same* is the same rows in the same order (or the same rows, for a query whose ORDER BY does not fix the order). Times in milliseconds: ChloeDB's own (`took_ms`) and, in brackets, as the client saw it over HTTP.

| query | ChloeDB vs DuckDB | Postgres vs DuckDB | rows | ChloeDB, 205k | DuckDB, 205k | Postgres, 205k | ChloeDB, 2,000k | DuckDB, 2,000k | Postgres, 2,000k |
|---|---|---|---|---|---|---|---|---|---|
| t01_aggs | same | same | 1 | 54.5 (56.0) | 5.3 | 67.5 | 530.5 (533.9) | 13.9 | 803.1 |
| t02_group_nulls | same | same | 6 | 3.0 (3.9) | 2.5 | 17.8 | 15.8 (17.0) | 5.4 | 113.2 |
| t03_having | same | refused | 26 | 2.8 (3.9) | 2.4 | – | 8.7 (10.2) | 4.9 | – |
| t04_join_group | same | same | 5 | 10.7 (11.7) | 3.3 | 24.9 | 96.2 (97.7) | 6.1 | 170.2 |
| t05_two_left_joins | same | same | 10 | 20.6 (22.0) | 14.8 | 85.6 | 191.9 (193.5) | 47.5 | 390.6 |
| t06_not_exists | same | same | 20 | 31.6 (33.0) | 2.9 | 17.5 | 329.7 (331.4) | 6.2 | 203.7 |
| t07_in_subquery | same | same | 1 | 0.5 (1.4) | 2.1 | 10.1 | 0.5 (1.5) | 3.7 | 76.9 |
| t08_date_trunc_day | same | same | 90 | 3.5 (4.5) | 3.0 | 41.6 | 11.8 (12.9) | 6.9 | 465.7 |
| t09_extract_dow_hour | same | same | 168 | 5.7 (6.9) | 2.6 | 181.6 | 37.3 (38.4) | 6.1 | 845.3 |
| t10_month_outliers | same | same | 14 | 5.4 (6.4) | 3.3 | 34.1 | 39.5 (40.8) | 9.8 | 383.3 |
| t11_date_diff | same | refused | 6 | 33.7 (34.9) | 2.8 | – | 247.1 (248.8) | 7.6 | – |
| t12_window_running | same | same | 90 | 3.1 (4.5) | 3.8 | 52.2 | 13.3 (15.2) | 7.6 | 463.3 |
| t13_rank_qualify | same | refused | 15 | 41.9 (43.5) | 5.5 | – | 395.8 (397.6) | 9.9 | – |
| t14_ntile_small | same | same | 2,225 | 5.5 (9.3) | 3.8 | 12.5 | – | 15.1 | 95.2 |
| t15_ntile_big | same | same | 4 | 18.6 (19.9) | 9.7 | 45.6 | 124.0 (125.8) | 35.8 | 514.2 |
| t15b_rows_over_10000 | refused | same | 39,183 | – | 15.0 | 51.9 | – | 122.9 | 594.1 |
| t16_case_bucket | same | same | 4 | 7.7 (8.7) | 4.1 | 15.9 | 63.8 (65.3) | 10.4 | 137.1 |
| t17_filter_coalesce | same | same | 1 | 2.7 (3.7) | 3.3 | 9.8 | 14.9 (16.1) | 8.9 | 78.7 |
| t18_not_in_nulls | same | same | 1 | 0.9 (1.8) | 4.1 | 31.7 | 0.9 (2.0) | 17.2 | 262.4 |
| t19_limit_offset | same | same | 10 | 0.3 (1.3) | 1.8 | 12.1 | 0.4 (1.4) | 3.8 | 102.8 |
| t20_distinct | same | same | 12 | 5.4 (6.3) | 3.0 | 14.2 | 38.9 (39.9) | 7.6 | 119.9 |
| t21_intersect | same | same | 204 | 6.0 (7.0) | 3.6 | 15.6 | 15.4 (16.6) | 6.7 | 115.7 |
| t22_union_all_order | same | same | 15 | 2.2 (3.2) | 2.1 | 11.9 | 7.5 (8.5) | 3.0 | 105.4 |
| t23_except | same | same | 20 | 62.0 (63.4) | 8.9 | 46.2 | – | 17.3 | 469.8 |
| t24_scalar_subq | same | same | 1 | 1.9 (3.0) | 2.4 | 21.7 | 11.8 (12.9) | 4.6 | 160.5 |
| t25_percentiles | same | refused | 6 | 15.2 (16.6) | 11.8 | – | 118.3 (120.2) | 79.9 | – |
| t26_stats | same | same | 2 | 9.0 (10.6) | 4.1 | 14.2 | 43.8 (46.0) | 12.9 | 113.2 |
| t27_arith | same | refused | 1 | 4.1 (5.2) | 1.6 | – | 28.8 (30.2) | 2.7 | – |
| t28_cast_round | same | refused | 8 | 1.0 (2.1) | 2.9 | – | 1.0 (2.2) | 2.4 | – |
| t29_strftime | same | refused | 14 | 81.7 (83.3) | 8.8 | – | 727.4 (730.1) | 31.8 | – |
| t30_cte_chain | same | same | 5 | 8.6 (9.9) | 3.0 | 9.8 | 87.8 (89.5) | 4.7 | 71.1 |
| t31_derived | same | same | 5 | 38.4 (39.9) | 2.7 | 17.7 | 388.6 (390.3) | 4.1 | 151.3 |
| t32_full_join | same | same | 263 | 1.6 (2.8) | 2.2 | 8.5 | 2.7 (4.2) | 3.6 | 58.2 |
| t33_self_join_ineq | same | same | 5 | 6.8 (7.8) | 2.2 | 1.7 | 7.0 (8.1) | 1.9 | 1.6 |
| t34_right_join | same | same | 20 | 1.4 (2.3) | 2.0 | 8.4 | 2.0 (3.0) | 3.0 | 60.2 |
| t35_string_funcs | same | same | 263 | 1.8 (3.5) | 2.0 | 0.6 | 1.9 (4.0) | 1.7 | 0.5 |
| t36_like_ilike | same | same | 1 | 1.8 (2.9) | 1.8 | 0.4 | 1.8 (2.9) | 1.5 | 0.3 |
| t37_group_expr | same | same | 19 | 1.1 (2.0) | 1.2 | 0.2 | 1.0 (2.0) | 1.0 | 0.2 |
| t38_string_agg | same | same | 6 | 1.0 (2.0) | 2.6 | 0.2 | 1.0 (2.2) | 2.3 | 0.2 |
| t39_exists_semi | same | same | 1 | 0.4 (1.2) | 1.5 | 8.6 | 0.5 (1.6) | 1.3 | 71.9 |
| t40_window_over_groups | same | same | 2 | 1.3 (2.4) | 2.3 | 10.2 | 5.1 (6.1) | 3.9 | 82.2 |
| t41_lead_default | same | same | 20 | 2.0 (3.0) | 2.3 | 8.2 | 14.9 (16.2) | 4.5 | 57.1 |
| t42_interval | same | same | 1 | 0.3 (1.2) | 1.3 | 7.6 | 0.3 (1.4) | 2.1 | 58.9 |
| t43_date_bin | same | refused | 8 | 1.1 (2.2) | 2.1 | – | 6.1 (7.1) | 3.1 | – |
| t44_order_desc_nulls | same | different | 5 | 0.3 (1.1) | 1.4 | 7.7 | 0.8 (1.8) | 2.7 | 57.9 |
| t45_count_distinct_two | same | same | 1 | 57.7 (59.0) | 3.7 | 35.0 | 212.5 (214.1) | 12.2 | 161.4 |
| c01_top_artists | same | same | 5 | 4.6 (5.7) | 2.9 | 1.3 | 4.7 (5.7) | 2.6 | 1.4 |
| c02_country_having | same | same | 9 | 0.3 (1.3) | 1.5 | 0.4 | 0.3 (1.4) | 1.6 | 0.2 |
| c03_monthly_running | same | same | 61 | 1.0 (2.1) | 1.8 | 0.3 | 1.0 (2.1) | 1.7 | 0.3 |
| c04_self_join_mgr | same | same | 8 | 0.7 (1.7) | 1.5 | 0.1 | 0.9 (1.9) | 1.5 | 0.1 |
| c05_top_genre_per_country | same | refused | 16 | 10.2 (11.3) | 4.9 | – | 9.5 (10.6) | 5.2 | – |
| c06_never_bought | same | same | 1 | 1.6 (2.7) | 1.6 | 0.8 | 1.6 (2.6) | 1.6 | 0.7 |
| c07_playlist_counts | same | same | 17 | 3.5 (4.5) | 1.9 | 1.5 | 3.6 (4.6) | 2.0 | 1.4 |
| c08_string_agg_customers | same | same | 16 | 0.7 (1.9) | 1.5 | 0.1 | 0.8 (1.8) | 1.5 | 0.2 |
| c09_genre_stats | same | same | 25 | 5.8 (6.9) | 2.6 | 1.0 | 5.8 (6.9) | 2.7 | 1.0 |
| c10_dense_rank_spend | same | same | 10 | 1.2 (2.5) | 2.2 | 0.5 | 1.1 (2.0) | 2.4 | 0.3 |
| c11_year | same | same | 6 | 0.4 (1.5) | 1.2 | 0.2 | 0.4 (1.3) | 1.4 | 0.2 |
| c12_lag_gap | same | refused | 20 | 0.9 (2.0) | 1.7 | – | 0.8 (1.8) | 1.8 | – |
| c13_exists_2 | same | same | 0 | 0.4 (1.5) | 1.4 | 0.1 | 0.4 (1.3) | 1.4 | 0.1 |
| c14_text_group | same | different | 5 | 5.0 (6.1) | 1.9 | 1.0 | 4.8 (5.9) | 2.0 | 0.8 |
| c15_like_text | same | same | 1 | 3.4 (4.5) | 0.8 | 0.4 | 3.3 (4.3) | 0.9 | 0.4 |
| c16_in_list_case | same | same | 3 | 0.7 (1.8) | 1.5 | 0.8 | 0.7 (1.6) | 1.8 | 0.7 |
| c17_three_table_left | same | same | 8 | 5.3 (6.4) | 2.0 | 1.0 | 5.2 (6.2) | 2.4 | 1.0 |
| c18_employee_ages | same | refused | 8 | 0.7 (1.6) | 1.0 | – | 0.5 (1.3) | 1.2 | – |
| x01_recursive_cte | refused | same | 8 | – | 1.8 | 0.2 | – | 2.1 | 0.2 |
| x02_rollup | refused | same rows, other order | 17 | – | 1.9 | 0.2 | – | 2.2 | 0.2 |
| x03_grouping_sets | refused | same | 10 | – | 1.6 | 0.1 | – | 1.7 | 0.1 |
| x04_is_distinct | refused | same | 1 | – | 1.6 | 10.9 | – | 5.1 | 82.9 |
| x05_cross_join | refused | same | 1 | – | 1.8 | 9.7 | – | 3.8 | 78.7 |
| x06_using | refused | same | 1 | – | 1.3 | 0.5 | – | 1.3 | 0.9 |
| x07_comma_join | refused | same | 1 | – | 1.2 | 0.6 | – | 1.6 | 0.7 |
| x08_keyword_range | refused | same | 1 | – | 0.7 | 0.1 | – | 0.8 | 0.2 |
| x09_sum_distinct | refused | same | 1 | – | 1.6 | 28.6 | – | 4.1 | 322.2 |
| x10_json_arrow | same | refused | 1 | 0.6 (1.5) | 0.5 | – | 0.6 (1.3) | 0.5 | – |

## What ChloeDB said when it refused

- **t15b_rows_over_10000**: query error: the query matches more than 10000 rows, the most a query returns; add a LIMIT (or read every row a page at a time: a scroll, page_size)
- **x01_recursive_cte**: query error: WITH RECURSIVE is not supported: REACHABLE and PATH traverse links
- **x02_rollup**: query error: GROUP BY ROLLUP (Borough, service_zone): ROLLUP is not supported; write GROUP BY Borough, service_zone, GROUP BY Borough, no GROUP BY (the total), each selecting NULL AS the columns it leaves out, and join them with UNION ALL
- **x03_grouping_sets**: query error: GROUP BY GROUPING SETS ((Borough), (service_zone)): GROUPING SETS is not supported; write GROUP BY Borough, GROUP BY service_zone, each selecting NULL AS the columns it leaves out, and join them with UNION ALL
- **x04_is_distinct**: query error: unsupported predicate: payment_type IS DISTINCT FROM 1
- **x05_cross_join**: query error: CROSS JOIN is not supported: join with JOIN ... ON an equality of the two tables' columns
- **x06_using**: query error: JOIN ... USING (AlbumId) is not supported: name the columns with ON, JOIN Album ON Track.AlbumId = Album.AlbumId, and qualify AlbumId where the query names it (Track.AlbumId)
- **x07_comma_join**: query error: FROM Track AS t, Album AS a lists 2 tables: join them with JOIN ... ON an equality of their columns, e.g. FROM Track t JOIN Album a ON t.AlbumId = a.AlbumId
- **x08_keyword_range**: query error: Borough is a KEYWORD field, not a numeric field
- **x09_sum_distinct**: query error: DISTINCT works in COUNT only
