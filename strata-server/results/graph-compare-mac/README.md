# Strata and four graph databases on the arXiv citation graph (Mac, Docker)

**One laptop, one Docker VM.** Measured on 2026-11-03 between 07:59 and 08:20
UTC with [`deploy/graph-compare/compare.sh`](../../deploy/graph-compare/compare.sh)
(data and exact answers from [`prep.py`](../../deploy/graph-compare/prep.py),
queries in [`run.py`](../../deploy/graph-compare/run.py), tables from
[`analyze.py`](../../deploy/graph-compare/analyze.py)). Every engine ran alone
in the same Docker VM with the same limits, loaded the same graph and answered
the same 200 queries of each kind from one client on the same Docker network.
The engines and the client share the VM's 4 CPUs, so the numbers compare the
engines with each other, not what any of them does on a server. What went
wrong on the way is in [NOTES.md](NOTES.md).

| | |
|---|---|
| machine | MacBook Air (Mac14,2), Apple M2 (4 performance + 4 efficiency cores), 24 GB, macOS 26.0.1 (25A362) |
| Docker | Docker Desktop, engine 24.0.5, VM with 4 CPUs and 8.7 GiB, LinuxKit kernel 5.15.49 |
| data | ogbn-arxiv citations: 169,343 papers, 1,166,243 citations (no self-citations or repeats). Directed as cited; "both ways" and paths treat it as undirected, a pair of papers citing each other being two edges |
| budget | each engine `--cpus 3 --memory 6g` (no swap); the client `--cpus 1 --memory 2g` |
| client | Python 3.12 in a container on the engines' Docker network: httpx 0.28.1 (HTTP JSON: Strata, ArangoDB), neo4j 6.4.1 (Bolt: Neo4j, Memgraph), redis 8.1.0 (RESP: FalkorDB). One query at a time |
| queries | 200 seeds and 200 pairs (seed 7). Each kind: up to 20 queries untimed, then all 200 timed. Client time is send to whole answer; the engine's own time is what each reports (below) |
| runs | two passes (`pass2`, `pass3`), one engine at a time; Neo4j re-run in both with GDS progress logging off (NOTES.md). The tables are pass 3; the last table puts both passes side by side |
| quiet | the kind cluster's containers stopped; macOS's `mediaanalysisd` used about one core throughout (not paused), and the Mac's 1-minute load average was 4.4 to 7.5 before each engine |

| engine | image | graph as |
|---|---|---|
| Strata (EDGES field) | `strata-server:graph`, built from main f7096030 | `papers (id INT PRIMARY KEY, cites EDGES)`: a paper's citations are a field of its row |
| Strata (edge table) | the same | `papers (id)` and `links (id, src, dst) OPTIONS(edges = 'src, dst')`: a citation is a row |
| Neo4j | `neo4j:2026.10.1-community`, Graph Data Science 2026.10.1 | `(:Paper {id})-[:CITES]->(:Paper)`, unique `id`; GDS projection `g` of all of it, undirected |
| Memgraph | `memgraph/memgraph-mage:3.14.0` | the same, index on `id`, in-memory transactional storage |
| FalkorDB | `falkordb/falkordb:v4.22.3` | the same, index on `id` |
| ArangoDB | `arangodb/arangodb:3.12.4.3` | `papers` documents keyed by id, `cites` edge collection |

## Results

Client p50 / p95 in ms, pass 3. "n/a": the engine has no such query (below).

| query | Strata (field) | Strata (edge table) | Neo4j | Memgraph | FalkorDB | ArangoDB |
|---|---:|---:|---:|---:|---:|---:|
| 1 hop, both ways (9 nodes on average) | 0.51 / 0.73 | 0.46 / 0.55 | 1.39 / 1.79 | **0.19 / 0.25** | 0.21 / 0.29 | 0.59 / 1.37 |
| 2 hops, both ways (2,821 on average, up to 30,012) | 0.63 / **2.40** | 0.63 / 6.07 | 1.31 / 7.81 | **0.27** / 5.54 | 0.32 / 6.93 | 0.92 / 5.85 |
| 3 hops out (171 on average) | 0.49 / 0.77 | 0.56 / 1.51 | 0.97 / 1.49 | **0.21 / 0.51** | 0.21 / 0.76 | 0.49 / 1.47 |
| shortest path, at most 10 hops | **0.64 / 1.52** | 1.77 / 7.28 | 1.75 / 8.05 | 1.15 / 33.2 | n/a | 2.71 / 10.5 |
| personalized PageRank, top 20 | **1.77 / 2.50** (eps 1e-4) | 3.65 / 5.55 | 143 / 245 (GDS defaults) | n/a | n/a | n/a |
| the same, finer | **10.8 / 14.4** (eps 1e-5) | 23.0 / 41.6 | 557 / 652 (100 iterations) | n/a | n/a | n/a |
| the same, finest | 65.0 / 91.3 (eps 1e-6) | 96.8 / 138 | | | | |

Every engine answered every neighbour and path query exactly as the reference
(`prep.py`, NumPy and SciPy) did. PageRank's answers are scored as recall@20
against the exact top 20 (power iteration to 1e-12):

| setting | Strata (field) | Strata (edge table) | Neo4j GDS |
|---|---:|---:|---:|
| coarse (eps 1e-4; GDS defaults: 20 iterations, tolerance 1e-7) | 0.807 | 0.809 | 0.987 |
| fine (eps 1e-5; GDS: 100 iterations, tolerance 1e-9) | 0.895 | 0.895 | 0.999 |
| finest (eps 1e-6) | 0.955 | 0.955 | |

Load and footprint (pass 3): the time to load the graph through each engine's
own client API until it answers, the container's memory after the queries
(`docker stats`) and the size of its data directory.

| engine | load (s) | memory | data on disk |
|---|---:|---:|---:|
| Strata (field) | **1.45** | **167 MiB** | **68 MiB** |
| Strata (edge table) | 7.48 | 286 MiB | 237 MiB |
| Neo4j | 21.3 (+ 1.1 for the GDS projection) | 3.56 GiB (3 GiB of it the heap it was given) | 836 MiB |
| Memgraph | 7.52 | 784 MiB | 387 MiB (snapshot and WAL; the graph is in memory) |
| FalkorDB | 27.8 | 251 MiB | 0 (in memory; nothing saved during the run) |
| ArangoDB | 18.5 | 685 MiB | 537 MiB |

Full tables, including each engine's own time, are in [tables.md](tables.md).

## What the numbers say

- **Neighbours: Memgraph and FalkorDB answer fastest at the median, Strata has
  the shortest tail on 2 hops.** Memgraph's median is 0.19 to 0.27 ms against
  Strata's 0.49 to 0.63 ms. Most of that gap is the client protocol, not the
  traversal: Strata's own time is 0.10 to 0.16 ms, and an HTTP JSON request
  costs this Python client about 0.4 ms against about 0.15 ms for Bolt or
  RESP. On 2 hops both ways, which reach up to 30,012 papers, Strata's p95 is
  2.40 ms against 5.5 to 7.8 ms for every other engine (its own time 1.74 ms,
  Memgraph's 4.95 ms).
- **Shortest paths: Strata is fastest at the median and the tail.** It takes
  0.64 / 1.52 ms against Memgraph's 1.15 / 33.2 ms, Neo4j's 1.75 / 8.05 ms and
  ArangoDB's 2.71 / 10.5 ms. FalkorDB has no undirected shortest path: its
  `shortestPath()` is directed only, and `algo.SPpaths` with
  `relDirection: 'both'` timed out after 24.6 s on the first pair and ran
  for over 8 minutes on the second before it was stopped, so it was not run.
- **Personalized PageRank: only Strata and Neo4j have it. Strata is 50 to 80
  times faster setting for setting, but less exact.** Neo4j's GDS iterates
  over the whole projected graph for every query: 143 ms with its defaults
  (recall@20 0.987) and 557 ms at 100 iterations (0.999). Strata's forward
  push touches only the seed's neighbourhood: 1.77 ms at eps 1e-4 (recall
  0.807), 10.8 ms at 1e-5 (0.895), 65 ms at 1e-6 (0.955). Its finest setting
  is still about twice as fast as GDS's defaults, but no setting tried
  reached GDS's 0.99. Memgraph's MAGE has only a global PageRank,
  plus NetworkX's on the directed graph with the seed marked by a node
  property. FalkorDB's `algo.pageRank` is global, and ArangoDB 3.12 removed
  Pregel.
- **Load and size: Strata's EDGES field is the smallest and quickest to
  load.** It takes 1.45 s and 68 MiB, because a paper's citations are one
  value of its row. The edge table form, one row per citation, takes 7.48 s
  and 237 MiB, like Memgraph (7.52 s) and well under Neo4j, ArangoDB and
  FalkorDB (18 to 28 s).
- **Strata's edge table walks more slowly than its EDGES field.** Paths are
  2.8 times slower and PageRank 1.5 to 2.1 times slower. A hop over an edge
  table finds the key in the edge table's index, then resolves each
  neighbour's key to its row of `papers` through the primary-key index. The
  EDGES field reads a CSR slice whose targets carry the row's docID when it
  is in the same segment, with no key lookup. Key resolution is also where
  the A3 co-purchase feed's PageRank spends its time
  ([part-a-do](../../../strata/results/laptop-2026-11/part-a-do/README.md)),
  so this is likely the same cost.

## Pass to pass

Client p50 in ms, pass 3 / pass 2:

| query | Strata (field) | Strata (edge table) | Neo4j | Memgraph | FalkorDB | ArangoDB |
|---|---:|---:|---:|---:|---:|---:|
| 1 hop, both ways | 0.51 / 0.51 | 0.46 / 0.48 | 1.39 / 1.18 | 0.19 / 0.20 | 0.21 / 0.20 | 0.59 / 0.55 |
| 2 hops, both ways | 0.63 / 0.64 | 0.63 / 0.75 | 1.31 / 1.39 | 0.27 / 0.33 | 0.32 / 0.26 | 0.92 / 1.14 |
| 3 hops out | 0.49 / 0.51 | 0.56 / 0.54 | 0.97 / 0.98 | 0.21 / 0.23 | 0.21 / 0.19 | 0.49 / 0.65 |
| shortest path | 0.64 / 0.72 | 1.77 / 1.70 | 1.75 / 1.68 | 1.15 / 1.12 | | 2.71 / 5.03 |
| PageRank, coarse | 1.77 / 1.75 | 3.65 / 3.74 | 143 / 109 | | | |
| PageRank, fine | 10.8 / 10.4 | 23.0 / 23.6 | 557 / 539 | | | |
| PageRank, finest | 65.0 / 69.1 | 96.8 / 113 | | | | |
| load, total (s) | 1.45 / 1.28 | 7.48 / 7.64 | 21.3 / 19.5 | 7.52 / 8.05 | 27.8 / 54.0 | 18.5 / 25.2 |

Strata's medians move by at most 13% between passes, and its edge table's by
at most 19%. The other engines' sub-millisecond medians move by up to a
third. ArangoDB's paths (2.71 against 5.03 ms), Neo4j's coarse PageRank (143
against 109 ms) and the loads of FalkorDB and ArangoDB move by 30% to 2
times.

## How each engine was asked

| query | Strata | Neo4j | Memgraph | FalkorDB | ArangoDB |
|---|---|---|---|---|---|
| hops | `SELECT COUNT(*) FROM papers WHERE REACHABLE(cites, :s, hops => k, dir => 'both', max_nodes => 1000000)` | `MATCH (s:Paper {id: $s})-[:CITES*1..k]-(n) WHERE n <> s RETURN count(DISTINCT n)` | `-[:CITES *BFS ..k]-` in the same pattern | as Neo4j | `FOR v IN 1..k ANY @s cites OPTIONS {order: 'bfs', uniqueVertices: 'global'} COLLECT WITH COUNT INTO c RETURN c` |
| path | `SELECT id FROM papers WHERE PATH(cites, :a, :b, dir => 'both', max_hops => 10, max_nodes => 1000000) ORDER BY hops()` | `MATCH (a ...), (b ...) MATCH p = shortestPath((a)-[:CITES*..10]-(b)) RETURN length(p)` | `MATCH (a ...), (b ...) WITH a, b MATCH p = (a)-[:CITES *BFS ..10]-(b)` | (none) | `FOR v IN ANY SHORTEST_PATH @a TO @b cites`; longer than 10 counts as none |
| PageRank | `SELECT id FROM papers ORDER BY ppr(cites, :s, eps => e) DESC LIMIT 20` (restart 0.15, both ways) | `gds.pageRank.stream('g', {sourceNodes: [s], dampingFactor: 0.85, ...})`, top 20 | (none) | (none) | (none) |

The edge-table form names `links` where the field form names `cites`. Loading:
Strata gets `POST /v1/import` of NDJSON in requests of 50,000 rows. Neo4j,
Memgraph and FalkorDB get `UNWIND` batches of 10,000 nodes, then 10,000
edges, matched by the indexed `id`. ArangoDB gets `/_api/import` in batches
of 50,000. None used an offline bulk importer such as `neo4j-admin database
import`, which would load faster.

The engine's own time is Strata's `took_ms`, Neo4j's
`result_available_after + result_consumed_after` (whole milliseconds),
Memgraph's parsing, planning and execution times, FalkorDB's "internal
execution time" and ArangoDB's `executionTime`.

## Not covered

One graph of 1.17M edges with no properties, one client, and only reads.
LDBC SNB's typed, property-rich graph, concurrent clients, updates during
traversal, and PageRank fused with vector search (the A3 feed) are left for a
follow-up.
