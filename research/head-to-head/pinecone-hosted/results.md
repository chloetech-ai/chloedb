# Vector search: ChloeDB against hosted Pinecone

The hosted half of [pinecone-local](../pinecone-local/results.md): the same collection (GloVe-100 from ann-benchmarks, `glove-100-angular`, word vectors of 100 dimensions scaled to unit length), the same first 1,000 test queries and the same recall and latency code, against a real Pinecone serverless index on the Starter (free) plan instead of the emulator. Sizes: the first 100,000 and 1,183,514 vectors.

Pinecone gets an index of `dimension: 100, metric: cosine` in aws us-east-1 (the one region the Starter plan has), loaded by `POST /vectors/upsert` 500 vectors a request from one client, and asked `POST /query {vector, topK: 10}` over kept-alive HTTPS connections, one per client. ChloeDB is run as on the local page, on this machine: `POST /v1/write` 500 at a time into `CREATE TABLE docs (id KEYWORD PRIMARY KEY, emb VECTOR(100))`, then `POST /v1/admin/optimize`, asked `SELECT id FROM docs ORDER BY similarity(emb, :v) DESC LIMIT 10`. Nothing is tuned on either side.

Measured on 2026-11-21 from a cloud container (Intel(R) Xeon(R) Processor @ 2.10GHz, 4 cores, 15 GB, Linux 6.18.44-fc-v726), whose traffic to the internet goes through an egress proxy that ends TLS and opens it again; where the container and the proxy are is not known. chloedb-server 0.3.9 (built from 040f4956d1). Pinecone's half and ChloeDB's half of each size ran one after the other on the same machine. Recall@10 is the share of the exact top 10 (numpy) a system returned; latency is each query once after 20 warm-ups. Pinecone's own time is the `x-pinecone-response-duration-ms` header of each answer (in whole milliseconds); the network is the client's time less that. Run it again with `pinecone=<API key> python3 run.py 100k full && python3 report.py` (the key is read from the environment and written nowhere; the index is deleted at the end). Raw numbers: [raw/results-100k.json](raw/results-100k.json), [raw/results-full.json](raw/results-full.json).

## What it says

At 1,183,514 vectors and the same recall, Pinecone answers in 46.7 ms at the median from here (11.0 ms of it Pinecone's own time) and ChloeDB, at ef => 800, in 2.34 ms on this machine; Pinecone's default finds more than ChloeDB's default at that size (0.9415 against 0.8592).

- **100,000 vectors.** Recall@10 once Pinecone's index is built: Pinecone 0.9299, ChloeDB 0.9321 at its default ef. One client, as the client sees it: Pinecone p50 45.9 ms, p99 88.4 ms; ChloeDB p50 1.52 ms, p99 3.07 ms. Of Pinecone's 45.9 ms, 7.00 ms is Pinecone's own time (p50; p99 16.0 ms) and 39.1 ms the network; ChloeDB's own time (`took_ms`) is 0.52 ms. At Pinecone's recall or more, ChloeDB's fastest setting is default ef (200) (0.9321, p50 1.52 ms). Four clients: Pinecone 81 queries/s, ChloeDB 1,291/s. Loading took 49.9 s for Pinecone (every vector counted after 50.0 s) against 9.7 s of writes and 15.1 s until ChloeDB's segments were merged.
- **1,183,514 vectors.** Recall@10 once Pinecone's index is built: Pinecone 0.9415, ChloeDB 0.8592 at its default ef. One client, as the client sees it: Pinecone p50 46.7 ms, p99 76.1 ms; ChloeDB p50 1.58 ms, p99 2.69 ms. Of Pinecone's 46.7 ms, 11.0 ms is Pinecone's own time (p50; p99 20.0 ms) and 35.3 ms the network; ChloeDB's own time (`took_ms`) is 0.59 ms. At Pinecone's recall or more, ChloeDB's fastest setting is ef => 800 (0.9421, p50 2.34 ms). Four clients: Pinecone 80 queries/s, ChloeDB 1,315/s. Loading took 584.8 s for Pinecone (every vector counted after 584.9 s) against 140.9 s of writes and 229.6 s until ChloeDB's segments were merged.
- **The network is most of Pinecone's time from here.** A request that does no search (`describe_index_stats`) takes p50 43.9 ms and 43.4 ms on a kept-alive connection (the two runs), and a new connection's TCP and TLS handshake through the proxy p50 392.5 ms and 377.8 ms, which is why the client keeps its connections. From a machine in aws us-east-1 the network should be a few milliseconds (not measured here) and Pinecone's own time most of the total; ChloeDB has no network to cross when it runs in the application's process or beside it.
- **Recall can move after a load.** At 100,000 vectors, asked as soon as every vector was counted, Pinecone found 0.9887 of the exact top 10, and 0.9299 10 minutes after the load began (34.5% of the queries had the same top 10 in the same order both times); at 1,183,514 vectors, asked as soon as every vector was counted, Pinecone found 0.9415 of the exact top 10, and 0.9415 25 minutes after the load began (99.9% of the queries had the same top 10 in the same order both times). It looks as if the serverless index answers from an exact scan of fresh writes until its approximate index of them is built (Pinecone's answers do not say), which at 100,000 vectors took less than the three minutes of the timed runs and at 1,183,514 was over before the ten-minute load ended. The settled number is the one compared here.
- **Loading waits on each request.** One client sends a request of 500 vectors (about 1 MB of JSON) and waits for it: 250 ms a request at 100,000 and 247 ms a request at 1,183,514, of which about 44 ms is the round trip measured above and the rest sending the body through the proxy and Pinecone's write (not split here). Pinecone suggests upserting in parallel, which was not tried, so its load time is that of one client; ChloeDB's writes go to a server on the same machine.
- **What this does not show.** Pinecone's pod indexes or its paid plans (the Starter plan has one region and rate limits), filters, namespaces, sparse or hybrid search, and ChloeDB on a separate machine across a network.

## 100,000 vectors

| system | recall@10 | p50, 1 client | p95, 1 client | p99, 1 client | own time p50 / p99 | queries/s, 4 clients | p99, 4 clients |
|---|---|---|---|---|---|---|---|
| Pinecone serverless, `topK: 10`, index built | 0.9299 | 45.9 ms | 54.7 ms | 88.4 ms | 7.00 ms / 16.0 ms | 81 | 90.1 ms |
| Pinecone serverless, right after the load | 0.9887 | 44.1 ms | 87.2 ms | 96.9 ms | 8.00 ms / 57.0 ms | – | – |
| ChloeDB `similarity(emb, :v)`, default ef (200) | 0.9321 | 1.52 ms | 1.85 ms | 3.07 ms | 0.52 ms / – | 1,291 | 6.85 ms |
| ChloeDB `similarity(emb, :v, ef => 32)` | 0.7678 | 1.32 ms | 1.55 ms | 1.80 ms | – | – | – |
| ChloeDB `similarity(emb, :v, ef => 64)` | 0.8272 | 1.40 ms | 1.65 ms | 2.03 ms | – | – | – |
| ChloeDB `similarity(emb, :v, ef => 128)` | 0.8916 | 1.44 ms | 1.72 ms | 2.02 ms | – | – | – |
| ChloeDB `similarity(emb, :v, ef => 400)` | 0.9678 | 1.78 ms | 2.17 ms | 2.81 ms | – | – | – |
| ChloeDB `similarity(emb, :v, ef => 800)` | 0.9894 | 2.24 ms | 2.65 ms | 3.99 ms | – | – | – |
| ChloeDB `similarity(emb, :v, ef => 1600)` | 0.9973 | 2.90 ms | 3.51 ms | 4.54 ms | – | – | – |
| ChloeDB `similarity(emb, :v, strategy => 'flat')`, exact | 1.0000 | 2.12 ms | 2.68 ms | 3.65 ms | – | 1,160 | 8.10 ms |

Pinecone's "right after the load" row is the first timed pass, which began when every vector was counted; its recall is that of the pass just before it. Pinecone's time, split (index built, 1 client): own time p50 7.00 ms, the rest p50 39.1 ms and p99 69.1 ms. ChloeDB's own time (`took_ms`) p50 0.52 ms, p95 0.66 ms; the rest of its latency is HTTP and JSON on this machine. Each Pinecone query cost 1 read unit. The two returned the same top 10 in the same order for 36.8% of the queries.

With each Pinecone query about 45.9 ms from here, four clients can ask at most about 87 a second, so the four-client number measures the network more than Pinecone (and at this size it ran before the index had settled).

| loading | Pinecone serverless | ChloeDB |
|---|---|---|
| create the index | 2.3 s | – |
| writes, 500 a request, 1 client | 49.9 s | 9.7 s |
| until every vector is counted, or segments merged | 50.0 s | 15.1 s |
| disk | not reported for serverless indexes | 82 MB |

## 1,183,514 vectors

| system | recall@10 | p50, 1 client | p95, 1 client | p99, 1 client | own time p50 / p99 | queries/s, 4 clients | p99, 4 clients |
|---|---|---|---|---|---|---|---|
| Pinecone serverless, `topK: 10`, index built | 0.9415 | 46.7 ms | 54.1 ms | 76.1 ms | 11.0 ms / 20.0 ms | 80 | 114.3 ms |
| Pinecone serverless, right after the load | 0.9415 | 45.0 ms | 52.5 ms | 76.3 ms | 10.0 ms / 17.0 ms | – | – |
| ChloeDB `similarity(emb, :v)`, default ef (200) | 0.8592 | 1.58 ms | 1.91 ms | 2.69 ms | 0.59 ms / – | 1,315 | 5.45 ms |
| ChloeDB `similarity(emb, :v, ef => 32)` | 0.6918 | 1.39 ms | 1.66 ms | 2.24 ms | – | – | – |
| ChloeDB `similarity(emb, :v, ef => 64)` | 0.7573 | 1.39 ms | 1.69 ms | 2.73 ms | – | – | – |
| ChloeDB `similarity(emb, :v, ef => 128)` | 0.8254 | 1.50 ms | 1.78 ms | 2.55 ms | – | – | – |
| ChloeDB `similarity(emb, :v, ef => 400)` | 0.9062 | 1.83 ms | 2.25 ms | 3.67 ms | – | – | – |
| ChloeDB `similarity(emb, :v, ef => 800)` | 0.9421 | 2.34 ms | 2.83 ms | 3.85 ms | – | – | – |
| ChloeDB `similarity(emb, :v, ef => 1600)` | 0.9688 | 3.23 ms | 3.97 ms | 5.08 ms | – | – | – |
| ChloeDB `similarity(emb, :v, strategy => 'flat')`, exact | 1.0000 | 25.2 ms | 33.1 ms | 40.5 ms | – | 164 | 42.6 ms |

Pinecone's "right after the load" row is the first timed pass, which began when every vector was counted; its recall is that of the pass just before it. Pinecone's time, split (index built, 1 client): own time p50 11.0 ms, the rest p50 35.3 ms and p99 56.0 ms. ChloeDB's own time (`took_ms`) p50 0.59 ms, p95 0.74 ms; the rest of its latency is HTTP and JSON on this machine. Each Pinecone query cost 1 read unit. The two returned the same top 10 in the same order for 33.1% of the queries.

With each Pinecone query about 46.7 ms from here, four clients can ask at most about 86 a second, so the four-client number measures the network more than Pinecone.

| loading | Pinecone serverless | ChloeDB |
|---|---|---|
| create the index | 1.4 s | – |
| writes, 500 a request, 1 client | 584.8 s | 140.9 s |
| until every vector is counted, or segments merged | 584.9 s | 229.6 s |
| disk | not reported for serverless indexes | 884 MB |
