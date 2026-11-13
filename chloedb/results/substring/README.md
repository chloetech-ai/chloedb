# What a substring index costs and gives on logs

`LIKE` and `ILIKE` on a `TEXT` field with and without
`OPTIONS(substring = true)` ([the format](../../notes/substring.md)), on
three of [Loghub](https://github.com/logpai/loghub)'s logs as
[`../log-bytes/parse.py`](../log-bytes/parse.py) reads them: SSH, Android
and BGL.

| file | what |
|---|---|
| `NAME.patterns` | the conditions run on a log: `LIKE` or `ILIKE`, a tab and a pattern on `body`, or `WHERE`, a tab and a whole condition |
| `NAME.json` | `examples/substring_bench.rs DIR NAME.jsonl NAME.patterns`: the log loaded into two tables that differ in the option on `body`, each merged to one segment; their bytes, the seconds each took to build, and per condition the rows it selects and the time of a count and of the first ten rows by `id` on each table |
| `tables.py` | prints the tables below from the JSON |

The table is `logs (id INT PRIMARY KEY, ts TIMESTAMP NOT NULL, host KEYWORD,
service KEYWORD, severity KEYWORD, pid INT, body TEXT)`, as
[`../log-bytes`](../log-bytes) loads it, 500,000 rows a batch.
Queries run on a database opened read-only, without the filter cache
(which would keep a pattern's rows from its first run), one run to warm
the page cache and then the median of 7 runs with the index and of 3
without.

All measured on 2026-11-12 on a cloud container (4-core Intel Xeon @
2.10GHz, 15 GB of RAM) at commit 3b0260e0 of branch
`claude/substring-index`, built with `--profile ci` (release without
link-time optimisation), nothing else running.

## Bytes

| log | lines | bodies, MB | file without, MB | file with, MB | index, MB | of which transform | samples | index / bodies | parts | bytes a run |
|---|---|---|---|---|---|---|---|---|---|---|
| ssh | 655,147 | 49.4 | 36.0 | 82.2 | 46.2 | 36.8 | 9.4 | 0.94 | 1 | 34.3 |
| android | 1,555,005 | 110.0 | 132.6 | 245.1 | 112.4 | 90.8 | 21.6 | 1.02 | 2 | 24.3 |
| bgl | 4,747,963 | 231.9 | 268.0 | 492.0 | 224.0 | 169.6 | 54.4 | 0.97 | 5 | 30.1 |

`transform` is the Burrows-Wheeler transform in its wavelet tree, `samples`
the sampled suffix array (one row in 32, with the document of each), and
`bytes a run` the text's bytes for each run of equal bytes in the
transform.

The index is about as large as the text it indexes (0.94 to 1.02 of its
bytes), and the file about twice what it was: the stored rows are
compressed and the index is not. The transform takes 5.9 to 6.6 bits a
byte of text and the samples 1.5 to 1.9, more than
[the format's notes](../../notes/substring.md#size-and-cost) estimated from
prose (5.1 to 5.8 and 0.9): a log's bytes are spread more evenly, and its
lines are short (49 to 75 bytes), so the entries each line adds to the
samples weigh more. These logs repeat themselves, though: the transform
has a run of equal bytes for every 24 to 34 bytes of text, which the
encoding of version 1 does not use and a run-length encoding (left open in
the format) would.

## Seconds to build

| log | load without, s | load with, s | merge without, s | merge with, s |
|---|---|---|---|---|
| ssh | 1.8 | 4.6 | 1.5 | 5.4 |
| android | 5.6 | 11.9 | 4.0 | 13.5 |
| bgl | 26.7 | 35.4 | 9.9 | 21.8 |

`load` writes the rows in batches, each flushed as a segment, and `merge`
makes one segment of them (`OPTIMIZE`). Both sort the suffixes of the
field's bytes again: a merge does not carry a part over.

## Milliseconds a query

`count` is `SELECT COUNT(*) FROM logs WHERE ...`, `first 10` is `SELECT id
FROM logs WHERE ... ORDER BY id LIMIT 10`; `without` and `with` are the two
tables.

ssh:

| condition | rows | count without, ms | count with, ms | first 10 without, ms | first 10 with, ms |
|---|---|---|---|---|---|
| `body LIKE '%Accepted password%'` | 182 | 635 | 0.04 | 672 | 0.05 |
| `body LIKE '%port 2191 ssh2'` | 6 | 622 | 0.03 | 721 | 0.03 |
| `body LIKE '%173.234.31.186%'` | 10 | 650 | 0.03 | 693 | 0.04 |
| `body LIKE '%183.62.140.253%'` | 32,953 | 561 | 45 | 623 | 47 |
| `body LIKE '%BREAK-IN ATTEMPT%'` | 19,406 | 653 | 34 | 717 | 27 |
| `body ILIKE '%break-in attempt%'` | 19,406 | 586 | 123 | 700 | 134 |
| `body LIKE '%Failed password for root%'` | 176,784 | 693 | 105 | 774 | 108 |
| `body LIKE '%preauth%'` | 170,481 | 650 | 102 | 714 | 98 |
| `body LIKE 'reverse mapping checking%'` | 18,909 | 577 | 5.98 | 655 | 6.05 |
| `body LIKE '%Failed password%from 119.4.203.64%'` | 8 | 679 | 0.39 | 748 | 0.05 |
| `body LIKE '%invalid user ad_in from%'` | 8,214 | 612 | 58 | 732 | 58 |
| `body ILIKE '%Invalid User ADMIN%'` | 14,419 | 691 | 64 | 718 | 70 |
| `body LIKE '%no such text anywhere%'` | 0 | 644 | 0.02 | 710 | 0.02 |
| `body LIKE '%ssh2'` | 161,234 | 630 | 99 | 720 | 103 |
| `body LIKE '%e%'` | 655,147 | 807 | 102 | 793 | 95 |
| `pid = 24200 AND body LIKE '%e%'` | 22 | 0.16 | 0.05 | 0.11 | 0.05 |
| `ts >= '2017-12-10T06:55:00Z' AND ts < '2017-12-10T07:00:00Z' AND body LIKE '%Failed password%'` | 1 | 0.20 | 0.05 | 0.12 | 0.06 |
| `pid > 2 AND body LIKE '%173.234.31.186%'` | 10 | 618 | 0.06 | 735 | 0.06 |

android:

| condition | rows | count without, ms | count with, ms | first 10 without, ms | first 10 with, ms |
|---|---|---|---|---|---|
| `body LIKE '%java.lang.NullPointerException%'` | 128 | 1786 | 0.09 | 1863 | 0.11 |
| `body LIKE '%HexData = [0100000004040000%'` | 2,552 | 1581 | 1.50 | 1866 | 1.62 |
| `body LIKE '%com.tencent.mm%'` | 19,380 | 1504 | 45 | 1807 | 43 |
| `body ILIKE '%wakelock%'` | 15,760 | 1575 | 148 | 1774 | 163 |
| `body LIKE '%WakeLock%'` | 13,807 | 1482 | 14 | 1675 | 14 |
| `body LIKE '%Exception%'` | 10,362 | 1560 | 20 | 1693 | 19 |
| `body LIKE '%backlight=823'` | 6 | 1529 | 0.04 | 1758 | 0.03 |
| `body LIKE 'getRunningAppProcesses: caller 10105%'` | 2,667 | 1340 | 0.91 | 1692 | 1.10 |
| `body LIKE '%caller 1____ does not hold%'` | 12,510 | 1635 | 129 | 1891 | 137 |
| `body LIKE '%Updating screen state%backlight=8%'` | 8 | 1553 | 0.14 | 1919 | 0.13 |
| `body LIKE '%no such text anywhere%'` | 0 | 1542 | 0.04 | 1817 | 0.03 |
| `body LIKE '%state=ON%'` | 475 | 1491 | 0.39 | 1836 | 0.47 |
| `body LIKE '%a%'` | 1,321,893 | 1872 | 265 | 2056 | 323 |
| `pid = 3382 AND body LIKE '%HexData%'` | 2,552 | 23 | 2.00 | 25 | 2.20 |
| `service = 'PowerManager_screenOn' AND body ILIKE '%STATE=on%'` | 475 | 46 | 8.72 | 49 | 8.96 |

bgl:

| condition | rows | count without, ms | count with, ms | first 10 without, ms | first 10 with, ms |
|---|---|---|---|---|---|
| `body LIKE '%0x0b85eee0%'` | 25 | 3679 | 0.12 | 4729 | 0.14 |
| `body LIKE 'generating core.971'` | 99 | 3701 | 0.11 | 4276 | 0.18 |
| `body LIKE '%instruction address: 0x00004ed8'` | 47,104 | 3943 | 26 | 4922 | 21 |
| `body LIKE '%Lustre%'` | 31,531 | 3655 | 9.45 | 4518 | 9.88 |
| `body ILIKE '%machine check%'` | 40,026 | 4133 | 120 | 4907 | 108 |
| `body LIKE '%machine check%'` | 25,488 | 3897 | 8.18 | 4666 | 8.45 |
| `body LIKE '%instruction cache parity error corrected%'` | 105,924 | 4123 | 320 | 5160 | 361 |
| `body LIKE '%data TLB error interrupt%'` | 152,734 | 4122 | 294 | 4854 | 297 |
| `body LIKE 'generating core%'` | 1,706,751 | 4240 | 541 | 4927 | 539 |
| `body LIKE '%ciod: Error%No such file%'` | 49,464 | 4162 | 833 | 4825 | 733 |
| `body LIKE '%core.9_1'` | 961 | 3815 | 1135 | 4859 | 1162 |
| `body LIKE '%no such text anywhere%'` | 0 | 4050 | 0.09 | 4755 | 0.09 |
| `body LIKE '%e%'` | 4,579,893 | 4677 | 803 | 5862 | 815 |
| `host = 'R02-M1-N0-C:J12-U11' AND body LIKE '%error%'` | 64,479 | 88 | 12 | 96 | 13 |
| `severity = 'FATAL' AND body LIKE '%0x00004ed8'` | 47,104 | 914 | 66 | 912 | 61 |

Every condition here is faster with the index: 3.4 times at the least
among the patterns alone (`%core.9_1` on BGL) and 46,000 times at the
most.

- A pattern in few rows (the first rows of each table, and a pattern in
  none) takes 0.02 to 2 ms, where a test of every row takes 0.6 s on SSH,
  1.5 s on Android and 4 s on BGL.
- A pattern in tens of thousands of rows that the index settles alone
  (`%Lustre%`, `%machine check%`, `%WakeLock%`, an IP address) takes 8 to
  50 ms: each row is a walk to a sample, 0.3 to 2.4 microseconds.
- A pattern in more rows than one for every KiB of text (`%e%`,
  `%preauth%`, `generating core%`) is not listed from the index: each
  part's stored values are tested instead, in the index's own loop, which
  takes 6 to 8 times less than the row evaluator takes for the same test
  on the table without the index.
- `ILIKE`, and a `LIKE` with `_` or two strings, test the stored value of
  each candidate, some 3 to 10 microseconds each, so they cost more for
  the same rows (`%break-in attempt%` beside `%BREAK-IN ATTEMPT%`). The
  slowest here, `%core.9_1` and `%ciod: Error%No such file%` on BGL, have
  a string (`core.9`, `ciod: Error`) in too many rows to list, so every
  value is tested for 961 and 49,464 matches.
- Beside a condition that keeps few rows (`pid = 24200`, five minutes of
  `ts`, one `host`), the pattern is tested on those rows' values alone, as
  it is without the index; the index's counts say when that is the
  cheaper way.

Not measured here: more than one segment, writes beside queries, and
values longer than a log's line.
