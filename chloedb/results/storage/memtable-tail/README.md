# The first search after a write: the memtable's last chunk in pieces

`strata-server/tests/write_stall.rs` (`cargo test --profile ci --test
write_stall -- --ignored --nocapture` in `strata-server`). It opens a
database with the server's options (`strata_server::server::db_options`:
flat vectors in the memtable, chunks of 1,024 rows), loads 200,000 of the
Simulator's test documents (title, body, tags, a vector of 64, ratings) in
requests of 500 rows, then times through `Service` (the path of an HTTP
request, without the HTTP):

- 200 rounds per kind of a search with no write before it, a one-row
  insert, and a search right after it (the memtable's last chunk grows
  from 0 to 600 rows over the three kinds);
- 300 searches per kind after 9,000 rows more, with no writes: a
  memtable of about 9,600 rows (it flushes at 10,000), nine full chunks
  and a last one of about 384 rows.

It runs once for each `DbOptions::memtable_tail_docs`: 0 (the last chunk
is one piece, as main builds it), 128 (the default) and 256.
`write_stall.jsonl` is its output, a line each.

Machine: a Claude Code cloud container, 4 vCPU Intel Xeon @ 2.10GHz (model
not named by the VM), 15 GB, Linux 6.18; rustc 1.97.0, `--profile ci`. One
run, 8 November 2026.

## A search right after a one-row insert (p50 / p99, ms)

| kind | no write before | pieces: 0 (main) | 128 (default) | 256 |
|---|---|---|---|---|
| bm25 | 0.49–0.60 / 0.95–3.01 | 30.0 / 52.2 | 5.1 / 13.6 | 7.3 / 18.8 |
| vector | 0.70–0.88 / 1.19–5.30 | 39.3 / 66.4 | 7.4 / 13.4 | 9.0 / 19.9 |
| hybrid (RRF) | 1.75–1.82 / 2.99–3.56 | 43.4 / 83.6 | 7.7 / 17.7 | 13.5 / 27.2 |

The search after a write rebuilds the last piece of the chunk still
filling (text analysis, above all): at most 128 rows instead of up to
1,024. The insert itself is 0.8–1.4 ms at its p50 in every setting.

## Searches over a full memtable, no writes (p50 / p99, ms)

| kind | pieces: 0 (main) | 128 (default) | 256 |
|---|---|---|---|
| bm25 | 0.51 / 1.43 | 0.45 / 1.00 | 0.40 / 1.49 |
| bm25 + filter | 0.55 / 1.59 | 0.55 / 1.65 | 0.49 / 2.61 |
| vector | 0.61 / 1.21 | 0.69 / 1.10 | 0.83 / 1.30 |
| hybrid (RRF) | 1.72 / 2.77 | 1.81 / 2.89 | 1.67 / 4.60 |
| facet | 0.41 / 1.62 | 0.43 / 1.16 | 0.36 / 1.41 |

Full chunks stay one segment each, and only the last chunk is in pieces
(about 384 rows: 3 pieces of 128, 2 of 256): searches over it are within
this machine's noise of one piece.
