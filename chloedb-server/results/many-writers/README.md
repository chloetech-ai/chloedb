# Many clients writing to one server

`writers.py` starts strata-server on a fresh database (durable commits, the
server's own options), loads 1,000 rows, then for each workload and each
number of clients has that many clients send small writes back to back for
8 seconds, from 4 client processes:

- `write_insert`: `/v1/write` with one `insert` of a new row (20 words of
  text).
- `sql_insert`: `INSERT INTO docs (id, title, body, stock) VALUES (...)`, a
  new row.
- `write_add`: `/v1/write` with one `add` of 1 to the `stock` of one of the
  1,000 rows, at random.
- `sql_update`: `UPDATE docs SET stock = stock + 1 WHERE id = :k`, one of
  the 1,000 rows at random.
- `sql_update_one_row`: the same UPDATE, every client on the same row.

It reports requests per second, client latency, the cores the server kept
busy, and the commits the writer made (`strata_write_batches_total`), so
requests per commit shows the grouping. At the end the counters must add up
to the adds and UPDATEs answered: they did in every run (229,541, 272,925,
241,022 and 298,064), with no request refused or failed.

main (4aa2bd0) grouped `/v1/write` requests that waited together into one
commit, but committed every SQL write alone. The branch groups SQL writes
too, when no two can touch the same row (INSERTs, and UPDATEs or DELETEs by
key of different keys).

Two runs per side, alternating (main, branch, main, branch), both built with
`cargo build --release`, on the cloud container (`machine.txt`: 4 cores,
shared by the server and the client processes). Requests per second, p50 /
p99 latency in ms, requests per commit.

| workload | clients | main r1 | main r2 | branch r1 | branch r2 |
|---|---|---|---|---|---|
| SQL INSERT | 1 | 762 (1.2 / 3.3) | 713 (1.3 / 3.5) | 755 (1.2 / 3.4) | 752 (1.2 / 3.8) |
| | 4 | 1,875 (1.9 / 6.4), 1.0 | 2,020 (1.8 / 5.6), 1.0 | 2,124 (1.7 / 4.9), 1.5 | 2,050 (1.7 / 6.4), 1.5 |
| | 16 | 1,905 (7.9 / 15.8), 1.0 | 2,079 (7.4 / 14.1), 1.0 | 4,295 (3.3 / 10.8), 3.5 | 4,558 (3.1 / 9.7), 3.5 |
| | 64 | 1,612 (34.6 / 107.6), 1.0 | 1,821 (34.3 / 51.3), 1.0 | 5,496 (10.9 / 25.6), 5.8 | 4,903 (12.0 / 28.8), 6.1 |
| SQL UPDATE by key | 1 | 512 (1.8 / 4.9) | 701 (1.3 / 3.7) | 674 (1.4 / 3.1) | 632 (1.4 / 4.2) |
| | 4 | 1,773 (2.1 / 4.6), 1.0 | 1,954 (1.9 / 4.1), 1.0 | 2,042 (1.8 / 4.1), 1.5 | 2,086 (1.8 / 4.1), 1.5 |
| | 16 | 1,721 (8.9 / 15.0), 1.0 | 1,868 (8.3 / 14.5), 1.0 | 4,008 (3.7 / 8.2), 4.3 | 4,148 (3.6 / 8.5), 4.0 |
| | 64 | 1,664 (37.9 / 52.0), 1.0 | 1,719 (36.4 / 55.5), 1.0 | 5,079 (12.1 / 24.0), 6.9 | 4,922 (12.2 / 25.3), 7.0 |
| SQL UPDATE, one row | 16 | 1,854 (8.4 / 13.2), 1.0 | 1,794 (8.5 / 15.9), 1.0 | 1,502 (10.2 / 17.6), 1.0 | 1,876 (8.3 / 12.7), 1.0 |
| | 64 | 1,725 (36.5 / 52.1), 1.0 | 1,820 (34.7 / 48.2), 1.0 | 1,610 (39.5 / 50.3), 1.0 | 1,940 (32.5 / 46.6), 1.0 |
| /v1/write insert | 16 | 6,245 (2.2 / 8.3), 5.0 | 5,968 (2.3 / 9.0), 4.9 | 5,906 (2.4 / 8.2), 5.0 | 6,457 (2.1 / 8.5), 4.7 |
| | 64 | 7,437 (5.2 / 23.9), 5.7 | 7,703 (5.5 / 23.6), 6.8 | 7,257 (5.9 / 22.9), 6.4 | 7,625 (5.1 / 21.1), 5.9 |
| /v1/write add | 16 | 5,866 (2.4 / 7.0), 4.7 | 6,813 (2.2 / 5.3), 4.8 | 6,196 (2.3 / 6.1), 5.1 | 7,503 (1.9 / 5.0), 4.9 |
| | 64 | 7,212 (5.9 / 21.9), 5.5 | 7,412 (3.6 / 16.8), 4.6 | 6,953 (3.5 / 17.2), 4.7 | 7,127 (3.6 / 16.7), 4.8 |

Every client count of every workload is in `main/run*.json` and
`branch/run*.json`.

- SQL writes from many clients go 2.2 to 2.3 times as fast at 16 clients
  and 2.7 to 3.4 times at 64, with a third of the latency at 64, because 3.5 to 7 share each
  commit where main committed each alone. With one client nothing changes.
- UPDATEs of one row cannot share a commit (each must see the other's
  change), so they stay at one per commit, as on main, and lose none of
  their increments.
- `/v1/write` is unchanged: it was grouped already.
- With 64 SQL clients the server kept 2.1 to 2.3 of the 4 cores busy, and
  the client processes the rest, so the branch's top here is the machine's,
  not the writer's.
