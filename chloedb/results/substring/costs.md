# Listing or scanning a pattern's rows: what each costs

Where `like::LOCATE_BYTES`, `EXACT_ROWS` and `GENERAL_SCAN`
([`src/substring/like.rs`](../../src/substring/like.rs)) come from. They
decide, for each part of a segment's substring index, whether a `LIKE` or
`ILIKE` pattern's candidate rows are listed from the index (each walked to
its sample, and its value read and tested where the index does not settle
it) or the part's stored values scanned. Below them, the 40 Hacker News
queries of `chloedb-hn`'s evaluation, timed on the server before and after
the change that set them.

| file | what |
|---|---|
| `costs-ssh.patterns`, `hn/costs.patterns` | the patterns: `LIKE` or `ILIKE`, a tab and the pattern (on HN, each of the 40 queries as one string and with a `_` for its second letter, under both, and eight short ones) |
| `costs-ssh.jsonl`, `costs-hn.jsonl` | [`examples/substring_costs.rs`](../../examples/substring_costs.rs)` DB TABLE FIELD PATTERNS RUNS`: each pattern answered both ways in every segment with the index, the median of 7 runs on SSH and of 3 on Hacker News, after one that warms the page cache |
| `costs.py` | prints the tables of the first part from the two |
| `hn/fetch_stories.py`, `hn/load.py`, `hn/cluster.sh` | the titles: the stories of Hugging Face's `open-index/hacker-news`, created as `chloedb-hn` creates `hn_stories` (without its vector) and loaded into 4 shards and a coordinator on this machine, as the app's cluster `hn4` runs |
| `hn/queries.txt`, `hn/fm_time.py` | the 40 queries (`chloedb-hn/eval/results/queries-2026-11-16.txt`) and the evaluation's two substring statements, each asked twice |
| `hn/before_after.sh`, `hn/before_after.py` | the queries on two servers, after the page cache was dropped and on a warm one; prints the tables of the second part |
| `hn/base-*`, `hn/new-*` | what it wrote: `base` is main at 0e7a7a07, `new` this change |

All measured on 2026-11-16 on a cloud container (4-core Intel Xeon @
2.10GHz, 15 GB of RAM), nothing else running: the cost probe built with
`--profile ci`, the servers with `--release`, on branch
`claude/project-thread-qg9jhv` over main 0e7a7a07.

## What each way costs

- SSH: Loghub's SSH log as [`../log-bytes/parse.py`](../log-bytes/parse.py)
  reads it, in the table `logs` that
  [`examples/substring_bench.rs`](../../examples/substring_bench.rs) builds
  with the index on `body` and merges to one segment.
- Hacker News: shard 0 of the cluster, its `title`: 12 segments with the
  index, beside 14 small ones replayed from the write-ahead log, which have
  none and which the probe does not read.

- Loghub SSH, `body`: 655,147 values, 50,040,781 bytes of them in the index, rows of 71,068,116 bytes, 15 patterns
- Hacker News, `title`: 1,184,796 values, 60,178,108 bytes of them in the index, rows of 367,265,449 bytes, 176 patterns

| data | plan | patterns | scan of every value, ns a byte of the rows | patterns listed with 100 rows or more | list, µs a row walked | list, µs a value tested |
|---|---|---|---|---|---|---|
| Loghub SSH, `body` | exact | 11 | 2.14 (1.66 to 2.58) | 8 | 1.71 (0.29 to 4.04) | - |
| Loghub SSH, `body` | spelled | 2 | 2.38 (1.90 to 2.85) | 2 | 2.29 (2.03 to 2.55) | - |
| Loghub SSH, `body` | pieces | 2 | 7.91 (7.48 to 8.34) | 1 | - | 3.90 (3.90 to 3.90) |
| Hacker News, `title` | exact | 48 | 1.68 (1.41 to 2.01) | 12 | 4.36 (3.59 to 5.24) | - |
| Hacker News, `title` | spelled | 48 | 2.17 (1.71 to 2.86) | 18 | 4.48 (0.46 to 8.68) | - |
| Hacker News, `title` | pieces | 80 | 2.88 (2.19 to 3.63) | 23 | - | 16.51 (6.72 to 32.02) |

| `LOCATE_BYTES` | `EXACT_ROWS` | Loghub SSH, `body`: time chosen / fastest, patterns 15% and 0.5 ms slower | Hacker News, `title`: time chosen / fastest, patterns 15% and 0.5 ms slower |
|---|---|---|---|
| 1024 | 2 | 1.009, 0 | 1.699, 10 |
| 1024 | 3 | 1.795, 2 | 1.699, 10 |
| 1024 | 4 | 1.795, 2 | 1.699, 10 |
| 2048 | 2 | 1.009, 0 | 1.176, 5 |
| 2048 | 3 | 1.009, 0 | 1.587, 9 |
| 2048 | 4 | 1.009, 0 | 1.587, 9 |
| 3072 | 2 | 1.009, 0 | 1.080, 3 |
| 3072 | 3 | 1.009, 0 | 1.092, 4 |
| 3072 | 4 | 1.009, 0 | 1.502, 8 |
| 4096 **(the rule)** | 2 | 1.009, 0 | 1.000, 0 |
| 4096 | 3 | 1.009, 0 | 1.080, 3 |
| 4096 | 4 | 1.009, 0 | 1.092, 4 |
| 6144 | 2 | 1.041, 1 | 1.000, 0 |
| 6144 | 3 | 1.009, 0 | 1.000, 0 |
| 6144 | 4 | 1.009, 0 | 1.080, 3 |
| 8192 | 2 | 1.449, 4 | 1.000, 0 |
| 8192 | 3 | 1.041, 1 | 1.000, 0 |
| 8192 | 4 | 1.009, 0 | 1.000, 0 |

A scan of every value costs about the same a byte of whole stored rows on
both (the medians above), 1.7 to 2.4 ns for one literal and 2.9 to 7.9 for
a pattern the row evaluator tests: it decompresses and reads rows, not the
field alone. A title is a sixth of its row (its link, author and counts
beside it), where a line of the SSH log is 0.7 of its, so a byte of title
costs about four times as much to scan. A row costs 1.7 to 2.3 µs to walk
to its sample on SSH and 4.4 to 4.5 on Hacker News (smaller segments of a
larger alphabet), and a candidate whose value is read 3.9 µs on SSH and 16.5
on Hacker News: there the candidates are sparse among rows of 310 bytes,
and each one decompresses a block of the doc store (16 KiB) of its own.

So the rule counts in bytes of whole rows, put in bytes of the field's text
by each segment's ratio of the two (`like::locate_bytes`): a part is
scanned where listing its candidates costs more than scanning every value,
a candidate costing `LOCATE_BYTES` bytes of rows to list with its value and
half that (`EXACT_ROWS`) without, and a byte of a pattern the row evaluator
tests twice as much (`GENERAL_SCAN`). The table above tries other values:
the time each picks for every pattern, over the fastest of the two ways,
and how many patterns it makes slower by 15% and 0.5 ms or more. 4096 and 2
pick the faster way for every pattern on Hacker News and for all but one
on SSH, a tie (`%Failed password for root%`, scanned in 158 ms where
listing takes 151); less lists `%the%` and `%of%` on Hacker News, more
scans `%183.62.140.253%` on SSH. The times move from run to run on this
machine by up to a third (an earlier probe of SSH, of 3 runs a pattern, had
its scans at 113 to 202 ms), and none of the picks of 4096 and 2 moved with
them. (The rule
decides part by part, and the table sums the parts.)

SSH:

| op | pattern | plan | rows walked | values tested | matches | list, ms | scan, ms | the rule |
|---|---|---|---|---|---|---|---|---|
| LIKE | `%Accepted password%` | exact | 182 | 0 | 182 | 0.1 | 124.8 | list |
| LIKE | `%port 2191 ssh2` | exact | 6 | 0 | 6 | 0.1 | 170.0 | list |
| LIKE | `%173.234.31.186%` | exact | 10 | 0 | 10 | 0.1 | 172.3 | list |
| LIKE | `%183.62.140.253%` | exact | 32,953 | 0 | 32,953 | 90.0 | 117.8 | list |
| LIKE | `%BREAK-IN ATTEMPT%` | exact | 19,406 | 0 | 19,406 | 40.1 | 145.8 | list |
| ILIKE | `%break-in attempt%` | spelled | 19,406 | 0 | 19,406 | 49.5 | 135.2 | list |
| LIKE | `%Failed password for root%` | exact | 176,784 | 0 | 176,784 | 151.0 | 158.4 | scan (slower) |
| LIKE | `%preauth%` | exact | 170,481 | 0 | 170,481 | 335.9 | 183.4 | scan |
| LIKE | `reverse mapping checking%` | exact | 18,909 | 0 | 18,909 | 13.0 | 171.0 | list |
| LIKE | `%Failed password%from 119.4.203.64%` | pieces | 9 | 9 | 8 | 0.1 | 531.8 | list |
| LIKE | `%invalid user ad_in from%` | pieces | 11,345 | 11,345 | 8,214 | 44.2 | 592.4 | list |
| ILIKE | `%Invalid User ADMIN%` | spelled | 14,419 | 0 | 14,419 | 29.2 | 202.7 | list |
| LIKE | `%no such text anywhere%` | exact | 0 | 0 | 0 | 0.0 | 0.0 | list |
| LIKE | `%ssh2` | exact | 161,234 | 0 | 161,234 | 651.1 | 122.4 | scan |
| LIKE | `%e%` | exact | 2,761,498 | 0 | 655,147 | 4,017.9 | 133.8 | scan |

Hacker News, the patterns with 100 rows or more (the others take under 0.5 ms to list):

| op | pattern | plan | rows walked | values tested | matches | list, ms | scan, ms | the rule |
|---|---|---|---|---|---|---|---|---|
| ILIKE | `%rust%` | spelled | 9,091 | 0 | 8,780 | 47.3 | 753.9 | list |
| LIKE | `%rust%` | exact | 3,730 | 0 | 3,638 | 19.5 | 547.1 | list |
| ILIKE | `%r_st%` | pieces | 469,103 | 385,286 | 33,438 | 2,588.4 | 1,249.5 | scan |
| LIKE | `%r_st%` | pieces | 379,327 | 320,489 | 21,446 | 2,170.4 | 1,153.3 | scan |
| ILIKE | `%kubernetes%` | spelled | 2,485 | 0 | 2,451 | 14.9 | 742.7 | list |
| ILIKE | `%k_bernetes%` | pieces | 2,489 | 2,454 | 2,451 | 51.4 | 1,292.3 | list |
| LIKE | `%k_bernetes%` | pieces | 2,489 | 2,454 | 22 | 47.5 | 805.0 | list |
| ILIKE | `%remote work%` | spelled | 719 | 0 | 717 | 2.7 | 725.8 | list |
| LIKE | `%remote work%` | exact | 306 | 0 | 306 | 1.5 | 594.3 | list |
| ILIKE | `%r_mote work%` | pieces | 719 | 717 | 717 | 14.1 | 1,287.2 | list |
| LIKE | `%r_mote work%` | pieces | 380 | 379 | 306 | 5.7 | 1,040.7 | list |
| ILIKE | `%apple vision pro%` | spelled | 117 | 0 | 117 | 1.0 | 236.5 | list |
| ILIKE | `%a_ple vision pro%` | pieces | 117 | 117 | 117 | 2.7 | 414.8 | list |
| ILIKE | `%static site generator%` | spelled | 188 | 0 | 188 | 1.5 | 942.7 | list |
| LIKE | `%static site generator%` | exact | 117 | 0 | 117 | 0.6 | 678.1 | list |
| ILIKE | `%s_atic site generator%` | pieces | 188 | 188 | 188 | 6.0 | 1,284.8 | list |
| LIKE | `%s_atic site generator%` | pieces | 125 | 125 | 117 | 2.1 | 871.9 | list |
| ILIKE | `%burnout%` | spelled | 321 | 0 | 321 | 1.9 | 639.2 | list |
| LIKE | `%burnout%` | exact | 138 | 0 | 138 | 0.5 | 468.7 | list |
| ILIKE | `%b_rnout%` | pieces | 335 | 335 | 321 | 5.6 | 1,170.8 | list |
| LIKE | `%b_rnout%` | pieces | 335 | 335 | 138 | 6.8 | 1,087.9 | list |
| ILIKE | `%z_g language%` | pieces | 3,378 | 3,374 | 4 | 70.6 | 1,036.2 | list |
| LIKE | `%z_g language%` | pieces | 1,704 | 1,702 | 0 | 26.1 | 823.2 | list |
| ILIKE | `%gpt-4%` | spelled | 539 | 0 | 529 | 1.8 | 197.8 | list |
| ILIKE | `%g_t-4%` | pieces | 544 | 534 | 529 | 7.1 | 933.3 | list |
| ILIKE | `%h_mx%` | pieces | 310 | 305 | 107 | 4.8 | 1,334.1 | list |
| LIKE | `%h_mx%` | pieces | 100 | 98 | 5 | 2.2 | 1,004.4 | list |
| ILIKE | `%nuclear power%` | spelled | 294 | 0 | 294 | 1.8 | 735.6 | list |
| LIKE | `%nuclear power%` | exact | 146 | 0 | 146 | 0.6 | 593.2 | list |
| ILIKE | `%n_clear power%` | pieces | 294 | 294 | 294 | 7.3 | 1,074.3 | list |
| LIKE | `%n_clear power%` | pieces | 168 | 168 | 146 | 2.8 | 914.5 | list |
| ILIKE | `%duckdb%` | spelled | 214 | 0 | 210 | 0.8 | 414.6 | list |
| ILIKE | `%d_ckdb%` | pieces | 224 | 220 | 210 | 3.1 | 1,057.2 | list |
| ILIKE | `%bitcoin%` | spelled | 6,141 | 0 | 5,970 | 27.9 | 691.3 | list |
| LIKE | `%bitcoin%` | exact | 213 | 0 | 211 | 0.8 | 421.9 | list |
| ILIKE | `%b_tcoin%` | pieces | 6,210 | 6,022 | 5,970 | 77.6 | 924.4 | list |
| LIKE | `%b_tcoin%` | pieces | 6,158 | 5,971 | 211 | 77.5 | 1,043.7 | list |
| ILIKE | `%y_ombinator%` | pieces | 1,113 | 1,103 | 60 | 19.8 | 1,006.2 | list |
| LIKE | `%y_ombinator%` | pieces | 1,112 | 1,102 | 16 | 16.2 | 1,080.3 | list |
| ILIKE | `%the%` | spelled | 337,684 | 0 | 292,186 | 1,082.1 | 940.0 | scan |
| LIKE | `%the%` | exact | 220,709 | 0 | 198,852 | 982.8 | 616.5 | scan |
| ILIKE | `%a%` | spelled | 3,907,052 | 0 | 1,115,334 | 16,089.6 | 1,049.3 | scan |
| LIKE | `%a%` | exact | 3,402,370 | 0 | 1,086,166 | 16,663.2 | 737.3 | scan |
| ILIKE | `%e%` | spelled | 5,276,168 | 0 | 1,149,937 | 23,344.1 | 768.1 | scan |
| LIKE | `%e%` | exact | 5,083,465 | 0 | 1,145,433 | 21,707.3 | 669.0 | scan |
| ILIKE | `%to%` | spelled | 427,388 | 0 | 359,661 | 2,068.3 | 933.2 | scan |
| LIKE | `%to%` | exact | 388,010 | 0 | 331,513 | 1,586.3 | 667.3 | scan |
| ILIKE | `%of%` | spelled | 224,237 | 0 | 207,377 | 973.6 | 628.6 | scan |
| LIKE | `%of%` | exact | 213,077 | 0 | 198,087 | 850.3 | 605.7 | scan |
| ILIKE | `%ing%` | spelled | 429,583 | 0 | 370,639 | 2,145.9 | 796.3 | scan |
| LIKE | `%ing%` | exact | 428,888 | 0 | 370,139 | 2,045.3 | 519.4 | scan |
| ILIKE | `%show hn%` | spelled | 50,232 | 0 | 50,211 | 22.9 | 958.2 | list |
| ILIKE | `%ask hn%` | spelled | 46,972 | 0 | 46,945 | 31.8 | 1,038.1 | list |

## The 40 queries

`fm_time.py` asks each query as `SELECT id, title, points FROM hn_stories
WHERE title ILIKE :p LIMIT 20` (first 20) and `SELECT COUNT(*) AS n FROM
hn_stories WHERE title ILIKE :p` (count), with `:p` the query between two
`%`, through the coordinator, twice each; the times are the server's
(`took_ms`) the first time each statement is asked, before the filter cache
keeps the pattern's rows. `cold` started the cluster after dropping the
page cache, `warm` started it anew after that run.

Same rows and counts in all 80 pairs: True

| page cache | statement | p50, ms | p90, ms | p99, ms | max, ms | sum, ms |
|---|---|---|---|---|---|---|
| warm | first 20 | 9.2 → 9.7 | 14.0 → 12.1 | 107.6 → 30.9 | 111 → 32 | 570 → 370 |
| warm | count | 11.8 → 11.0 | 22.8 → 17.3 | 222.2 → 73.2 | 261 → 75 | 988 → 598 |
| cold | first 20 | 11.1 → 10.3 | 27.6 → 14.5 | 500.1 → 46.2 | 542 → 47 | 1620 → 491 |
| cold | count | 12.9 → 11.4 | 43.9 → 17.9 | 719.4 → 71.0 | 1045 → 80 | 2074 → 595 |

| query | titles | first 20 warm, ms | count warm, ms | first 20 cold, ms | count cold, ms |
|---|---|---|---|---|---|
| `rust` | 35,882 | 102.7 → 29.9 | 260.6 → 75.0 | 542.0 → 44.9 | 1045.5 → 80.3 |
| `bitcoin` | 23,990 | 110.8 → 31.6 | 162.1 → 70.4 | 434.6 → 47.1 | 209.4 → 56.4 |
| `kubernetes` | 9,910 | 31.3 → 11.1 | 73.1 → 30.1 | 183.4 → 16.7 | 196.1 → 26.5 |
| `remote work` | 2,979 | 12.5 → 4.7 | 28.1 → 17.1 | 37.2 → 14.4 | 66.6 → 16.2 |
| `gpt-4` | 2,115 | 13.1 → 8.5 | 22.2 → 14.9 | 26.5 → 9.6 | 41.4 → 13.2 |
| `nuclear power` | 1,201 | 7.8 → 6.0 | 21.2 → 13.6 | 22.4 → 10.3 | 34.0 → 12.5 |
| `burnout` | 1,274 | 7.3 → 4.0 | 21.7 → 19.7 | 23.4 → 10.9 | 32.0 → 17.9 |
| `apple vision pro` | 477 | 4.8 → 3.6 | 17.1 → 10.3 | 11.5 → 11.5 | 27.6 → 12.7 |
| `static site generator` | 698 | 7.6 → 4.9 | 14.5 → 11.1 | 18.8 → 8.2 | 25.1 → 18.1 |
| `duckdb` | 800 | 6.6 → 3.8 | 15.5 → 13.7 | 5.3 → 9.3 | 23.1 → 13.0 |

| pattern | median of the 4 shards, ms | slowest shard, ms |
|---|---|---|
| `%bitcoin %` | 68.7 → 18.3 | 74.3 → 19.5 |
| `%itcoin%` | 106.7 → 27.7 | 113.8 → 29.7 |
| `%bitcoi%` | 105.8 → 27.3 | 136.9 → 28.4 |
| `%ubernetes%` | 23.5 → 8.2 | 26.5 → 8.8 |
| `%chatgpt%` | 56.0 → 17.2 | 60.9 → 25.2 |
| `%covid%` | 63.5 → 9.2 | 70.6 → 11.2 |

What changed:

- `ILIKE` of one string reads no value: the index's spellings of it are the
  matches. Before, every candidate's stored value was read and tested, one
  doc store block for each, in the order the index found them; `%rust%`
  read a value for each of its 35,882 titles and more, and now reads none.
- Candidates a pattern does test are read in docID order, each block of
  the doc store once.
- `LIMIT 20` without an `ORDER BY` stops at the first 20 rows by docID (the
  same rows as before, the 80 answers above agree): a segment's values are
  scanned from its start where the first are near it, and its rows listed
  where they are not. The last table is that for words that came into use
  late, which a table loaded in time order has at its end: each shard
  walks 16 of their rows, sees that, and lists them.
- The rule for scanning instead counts whole rows, so a pattern in many
  titles is no longer listed where a scan is faster.

Not measured here: a cluster whose shards do not fit in memory (the app's
`hn4` holds 14 GB a shard on 16 GB nodes, and a cold query there reads its
index and rows from disk), and `chloedb-hn`'s own statements, which order
by points and so gain from the first two changes but not the third.
