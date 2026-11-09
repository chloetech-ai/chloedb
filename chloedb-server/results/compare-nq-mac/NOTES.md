# Notes on the NQ 1M run (2026-11-02, UTC)

What happened during the run, engine by engine, and where each report came
from. Times are UTC; "image A/B/C" are the three builds of
`strata-server:compare` in [README.md](README.md) (A `8056f8ca`, B `546afd66`
= `69d1ef07` + a local cherry-pick of `8248900`, C `7e7f470a` = `e4828b0` +
the same cherry-pick; neither cherry-pick was pushed, they live on the Mac's
local branches `compare-nq-strata` and `compare-nq-redis`).

## The Mac

- Data: `prepare_beir_local.py nq --docs 1000000 --backend st` embedded the
  1,000,000 passages in 6,527 s with sentence-transformers on the M2's GPU
  (fastembed's ONNX model on the CPU had managed 20 documents a second).
- Before the run: other demo servers, Ollama, idle dev servers and the local
  CI were stopped or paused; the old kind cluster was deleted and a new one
  made; Docker Desktop restarted with its 4 CPUs and 8.7 GiB.
- Load gate: each timed step waited (up to 10 min) for the Mac's 1-minute
  load average to drop below 5. macOS's own work (mediaanalysisd at 65 to 90%
  of a core, Spotlight indexing the new files, XProtect, WindowServer) kept it
  near 6 while the Docker VM was idle (at 06:03 the VM's load was 0.55 and the
  Mac's 6.2), so from 06:22, the step after Elasticsearch's stream, the gate
  was 7 (logged in the run log). Elasticsearch's load, run, writes and stream
  (and Strata's first shard build, since replaced) ran under the gate of 5;
  everything else under 7.
- At 06:03 an empty `.metadata_never_index` kept Spotlight out of the data and
  report folders, and at 06:14 mediaanalysisd, which ignored SIGTERM, was
  paused with SIGSTOP for the rest of the run.
- At 11:47 the Mac's DNS failed for a moment (github.com and
  registry-1.docker.io did not resolve), which failed one image build (below).

## Strata

- The first `up` (05:39) failed: the servers refuse to listen beyond loopback
  without a token. `8dfeab5` gave the shards, the coordinator and the client a
  shared token and `2ff47f1` made it random per cluster (a Kubernetes secret);
  diagnostics in `failures/strata-up-054414`.
- Its shards were first built at 05:34 with image A, without the HNSW fix
  (`build-strata-s*-pre-hnsw-fix.json`), then built again at 10:56 with image
  B (`build-strata-s*.json`, the builds the tables use), which carries the
  cherry-pick of `8248900` (a node setting its own links no longer overwrites
  the links other threads add).
- Run (10:59 to 11:04) and writes (11:04 to 11:05) with image B.
- Stream, first attempt (11:05:31 to 11:08:41): every rate, flat out and the
  consumer crash were measured (they are in the run log), then the clean-up's
  delete was refused with 429 ("the database is behind on flushing writes to
  disk") and no `stream-strata.json` was written; diagnostics in
  `failures/strata-stream-*`. `e9c61d8` makes the clean-up wait out 429s and
  write the report first.
- Stream, re-run (11:47 to 11:53) on freshly built shards
  (`build-strata-s*-stream-shards.json`), still on image B, so without
  `e9c61d8`: its clean-up went through and it wrote `stream-strata.json`. Its
  CPU lines are in `cpu-strata-streamrerun.jsonl`.

## Elasticsearch, OpenSearch, Solr

- One run each with image A's client, every step (load, run, writes, stream).
- Solr's sql aggregate was unavailable in its run: Solr 10's parallel SQL is
  the optional `sql` module, which `solr.yaml` did not enable. `a242167` turns
  it on and fixes three more things the request needed. The kind was re-run on
  its own at 11:31 to 11:46 with image B on a second load:
  `run-solr+sql.json(.gz)` (merged by `analyze.py` into Solr's results and
  labelled as coming from that run), `load-solr-sqlrerun.json` (the tables keep
  the first load), `cpu-solr-sqlrerun.jsonl`.
- OpenSearch's writes clean-up put back 19,846 ratings, where Elasticsearch's
  and Solr's put back 20,000.

## Vespa

- Attempt 1 (07:13): `vespa-0` crash-looped, given its role as two arguments
  (`args: [configserver,services]`); `7d29933` quotes it. Diagnostics in
  `failures/vespa-up-071935`.
- Attempt 2 (11:08 to 11:30, image B's client): the load failed at about
  850,000 documents with 429s ("Rejecting execution due to overload"); that
  client sent all 500 puts of a batch at once and tried each 20 times.
  Diagnostics in `failures/vespa-load-113013`.
- Attempt 3 (12:07 to 12:40, image C, with `e4828b0`: 64 puts in flight a
  client, up to 120 tries with backoff): every report of Vespa's comes from it.
  From about 880,000 documents its content pods sat at 1,919 of their 1,920 MiB
  (anon memory about 1.73 GB, page cache about 0.17 GB) and the feed fell from
  about 1,100 to about 48 documents a second (4.8 s a put). There was no feed
  block: `resource_usage.nodes_above_limit` stayed 0, because Vespa measures
  memory against the whole 8.7 GB VM, not the pod. Its query, write and stream
  numbers come from content nodes at their memory limit.
- With time limits for that attempt: its writes clean-up finished on its own
  (16.2 s); its stream's clean-up (deleting the 142,500 streamed documents at
  about 50 a second) was stopped as soon as `stream-vespa.json` was written.

## The one-node engines

- Meilisearch, Typesense, Manticore, Qdrant, Weaviate, Milvus, ParadeDB and
  Postgres: one run each with image A's client (load, run).
- Weaviate's logs show its HNSW commit log being condensed during its run.
- Redis, first attempt (10:47, image A, FLOAT32 vectors): out of memory at
  about 93% of the load (maxmemory 5500mb, noeviction); diagnostics in
  `failures/redis-load-105326`. `49d8bc9` stores the vectors as FLOAT16 (the
  client sends half floats in the load and the queries). The first build of
  image C for it (11:47) failed on the DNS outage above.
- Redis, second attempt (11:53 to 12:07, image C, FLOAT16): loaded and ran;
  `load-redis.json` and `run-redis.json.gz` are from it.

## Files left out

None: every report is under 20 MB (the largest, `run-strata.json`, is 0.6 MB
gzipped), and every diagnostic file under `failures/` is under 1 MB.
