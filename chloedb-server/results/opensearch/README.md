# The OpenSearch API next to OpenSearch, on TREC-COVID

Measured 2026-11-08 (07:39 UTC) in a cloud container: 4 vCPUs (Intel Xeon
Processor @ 2.10GHz), 15.7 GB of memory, Linux 6.18. Both engines on the
same machine, one at a time: while one was measured, the other was
stopped (SIGSTOP). `trec-covid.json` is the run's output.

- **OpenSearch 2.11.0**, the official image
  (`opensearchproject/opensearch:2.11.0`) in Docker with host networking,
  `-Xms1g -Xmx1g` (its default heap), single node, security plugin off.
- **Strata**: `strata-server --db covid.strata --grpc off
  --opensearch-listen 127.0.0.1:9204`, built at 955693b with `cargo build
  --profile ci` (in strata-server the `ci` profile is `release`'s).

## Data and requests

[BeIR/trec-covid](https://huggingface.co/datasets/BeIR/trec-covid)
(revision d158b3b1564c0d022d67c482c3d5bbb10922dfe7): 171,332 abstracts
(`title`, `text`) and its 50 topics, with made-up fields to filter, sort
and count by (`source` keyword, `year` long, `published` date, `cites`
long; seeded, the same for both). One index, one shard, no replica, the
request cache off (`index.requests.cache.enable: false`, its counters 0
after the run); Strata keeps no result cache.

`bench.py` creates the index, writes the documents with `_bulk` (1,000 a
request, one client) and `_refresh`, waits 30 s, then sends each of ten
kinds of search 300 times (after a warm-up of 30), from one client over one
kept-alive connection, the same requests in the same order to both. A
latency is the client's, from sending the request to reading the whole
answer. Then 8 clients send topic searches for 20 s.

```sh
python3 bench.py --data DIR \
  --engine "opensearch=http://127.0.0.1:9203,pid=$JAVA_PID,disk=$OS_DATA" \
  --engine "strata=http://127.0.0.1:9204,pid=$STRATA_PID,disk=$STRATA_DIR" \
  --out trec-covid.json
```

## Results

| search (300 each) | OpenSearch p50 / p99 ms | Strata p50 / p99 ms |
|---|---|---|
| `match`, one word, title | 2.96 / 8.06 | 1.70 / 3.06 |
| `match`, a topic, text | 6.41 / 18.99 | 2.22 / 4.79 |
| `match` + `term` + `range` filters | 9.47 / 31.87 | 3.69 / 7.57 |
| `multi_match` `title^2`, `text` | 6.89 / 30.16 | 4.10 / 9.75 |
| `match_phrase`, text | 2.97 / 5.84 | 1.86 / 4.86 |
| `term` filter alone | 2.00 / 7.00 | 1.49 / 2.23 |
| `range`, sorted by date | 2.96 / 6.46 | 1.52 / 2.44 |
| `terms` aggregation of a `match` | 6.82 / 18.93 | 4.30 / 5.56 |
| `date_histogram` of every document, `avg` in each year | 9.73 / 15.50 | 6.71 / 8.80 |
| `match` + `highlight` | 7.69 / 24.84 | 3.27 / 5.66 |

| | OpenSearch | Strata |
|---|---|---|
| topic searches a second, 8 clients | 638 | 1,183 |
| bulk writes of 171,332 documents | 21.4 s (8,003 a second) | 39.0 s (4,389 a second) |
| resident memory, index empty | 1,348 MiB | 50 MiB |
| resident memory, 30 s after the writes | 1,448 MiB (peak 1,563) | 518 MiB (peak 812) |
| resident memory, after the searches | 1,652 MiB | 726 MiB |
| on disk, 30 s after the writes | 415 MB: the index 195 MB, its translog 219 MB (143 bytes after a `_flush`) | 260 MB: the file 249 MB, its write-ahead log 10 MB |

Resident memory is the process's (`VmRSS`, `VmHWM` for the peaks): the
JVM's, and strata-server's with the pages of its file it has read.

## The same answers

Each search's answer from both, compared (`same_totals`,
`same_first_page` in `trec-covid.json`):

- **Hits found**: all 3,000 searches find as many hits on both.
- **First pages** (the 2,400 searches that return hits): 808 return the
  same hits in the same order; the other 1,292 but the 300 phrase
  searches return the same scores (to 1 part in 10,000) or dates in the
  same places, with documents of equal scores or dates in another order:
  Strata orders ties by `_id`, OpenSearch by the order the documents
  were written. The 300 phrase searches find the same documents and rank
  them otherwise: Strata scores a phrase as the sum of its words' BM25,
  OpenSearch by how often the phrase occurs.

The first runs of this benchmark found where Strata's analysis differed
from OpenSearch's standard analyzer (`children's`, `sévère`, Chinese text,
soft hyphens, `¼`, a title ending in `☆`; 2,445 of 3,000 searches found
as many hits) and that an aggregation of every document's numbers was 4x
slower than OpenSearch's (48 ms); both were fixed before this run.

Strata's bulk writes are the one place it is slower: 1.8x OpenSearch's
time, with one client sending 1,000 documents a request.
