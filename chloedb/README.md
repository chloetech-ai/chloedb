# ChloeDB

The database engine: an embedded, single-file search database in the spirit
of SQLite. One file, ACID transactions with snapshot isolation, and SQL.
Every table is indexed for full text (posting lists), structured columns (a
columnar forward index) and vectors (exact, IVF and HNSW), rows can hold
typed, weighted edges to other rows, and a cost-based planner combines all
of it in one statement.

```sql
SELECT id, title, score() FROM docs
WHERE MATCH(body, 'vaccine AND (mrna OR "messenger rna") -animal')
  AND year BETWEEN 2019 AND 2021 AND category IN ('c01', 'c02')
ORDER BY 0.2 * minmax(bm25(body, :text)) + 0.8 * minmax(similarity(emb, :q)) DESC
LIMIT 10;
```

[How ChloeDB works](../docs/chloedb/index.md) is the overview, and
[DESIGN.md](DESIGN.md) the design in depth. The
[query language](../docs/chloedb/query-language.md) is the SQL reference.

## Layout

| path | what |
|---|---|
| `src/storage/` | file format, meta slots and shadow-paging commits, WAL, memtable, snapshots, primary keys, merges, vacuum |
| `src/segment/` | immutable segments of named, aligned sections, read zero-copy through `mmap` |
| `src/text/` | analyser, Loom and BP128 postings, positions, BM25 with MaxScore and Block-Max WAND |
| `src/forward/` | FOR-packed columns with zone maps, keyword dictionaries and Pyramid lists, range histograms, facets, LZ4 doc store |
| `src/vector/` | float32 and SQ8 (VNNI on x86-64, NEON dot products on aarch64) vectors, flat, IVF and HNSW, filtered search |
| `src/graph/` | EDGES fields: out-edge CSR and reverse index per segment; breadth-first search, shortest paths, personalized PageRank |
| `src/query/` | SQL binder (sqlparser), logical plan, cost-based planner, executor, RRF and weighted fusion, graph predicates and scores |
| `src/postings/` | compressed posting-list formats (Loom, Pyramid, Entropic, BP128 and the formats they were compared with), intersection and union algorithms, and their testbed; see [`notes/posting-lists.md`](notes/posting-lists.md) |
| `src/db.rs` | `Db`: open, load, query |
| `src/baselines/` | SQLite + FTS5 and Tantivy baselines (feature `baselines`) |
| `src/bin/` | `chloedb` shell, `chloedb-eval`, component benchmarks, `chloedb-baselines`, `chloedb-graph-bench`, `chloedb-posting-bench` |

## Using it

The `chloedb` shell runs SQL against a database file. It opens a database
that exists and does not create one: `chloedb-server --demo 20000 --db
products.chloedb` makes the one these lines read.

```sh
cargo build --release
target/release/chloedb products.chloedb "SELECT category, COUNT(*) FROM products WHERE MATCH(body, 'waterproof') GROUP BY category ORDER BY 2 DESC LIMIT 5" --plan
target/release/chloedb products.chloedb --param t='waterproof tent' < queries.sql
```

`--plan` prints the planner's choices, `--read-only` reads a database while
another process writes it, and `.schema`, `.segments`, `.check` and
`.checkpoint DEST` inspect, verify and back it up.

From Rust, `Db::open` opens a database, `Db::query` runs a `SELECT` and
`Db::sql` runs any statement, writes and `CREATE TABLE` included:

```rust
use std::path::Path;
use chloedb::{db::Db, query::Params, storage::DbOptions, value::Value};

let db = Db::open(Path::new("products.chloedb"), DbOptions::default())?;
let rs = db.query(
    "SELECT id, title FROM products WHERE MATCH(body, :q) ORDER BY bm25(body, :q) DESC LIMIT 10",
    &Params::new().set("q", Value::Str("waterproof tent".into())),
)?;
for row in &rs.rows { println!("{row:?}"); }
```

The SQL covers full text with `MATCH` (phrases, prefixes, typos, boolean
operators), filters on structured fields, scores (`bm25`, `similarity`,
`distance`, `rrf`, weighted sums), `GROUP BY` and aggregates, joins,
subqueries and `WITH`, window functions, `UNION`, `INTERSECT` and `EXCEPT`,
graph traversal (`REACHABLE`, `PATH`, `hops()`, `ppr()`), `INSERT`, `UPDATE`
and `DELETE`, and `CREATE`, `ALTER` and `DROP TABLE`. Functions take named
options, such as `similarity(emb, :q, nprobe => 32, strategy => 'prefilter')`
or `rrf(a, b, k => 60, depth => 100)`.

## Evaluation

```sh
python3 scripts/prepare_beir.py ~/data/beir   # BEIR + embeddings (about 4.8 GB)
cargo test --release                                   # components, crash recovery, end-to-end SQL
scripts/run_eval.sh                                    # every experiment, in order (about 4 h)
python3 scripts/build_report.py                        # report/strata.html
```

Graphs (ogbn-arxiv, about 150 MB; vectors from ChloeCtx's static embedder):

```sh
python3 -I scripts/prepare_arxiv.py ~/data/graph/arxiv      # after downloading arxiv.zip and titleabs.tsv.gz there
chloectx-bench embed --input ~/data/graph/arxiv/arxiv.jsonl --out ~/data/graph/arxiv/arxiv.f32
cargo build --release --features baselines --bin chloedb-graph-bench
target/release/chloedb-graph-bench load
target/release/chloedb-graph-bench latency --sqlite ~/data/graph/arxiv/edges.sqlite
target/release/chloedb-graph-bench quality --seeds 3,5,10,20,50
python3 scripts/graph_quality_stats.py results/graph/quality_arxiv.rows.jsonl.gz --out results/graph/quality_stats.json
target/release/chloedb-graph-bench synthetic --nodes 1000000 --sqlite ~/data/graph/synthetic.sqlite
```

Posting lists (WikiText-103 and synthetic collections; see
[`notes/posting-lists.md`](notes/posting-lists.md)):

```sh
scripts/postings/prepare_wikitext.sh data
scripts/postings/run_experiments.sh data/wikitext103.txt
scripts/postings/run_designs.sh data/wikitext103.txt
python3 scripts/postings/build_report.py              # report/postings/report.html and designs.html
```

Results are in `results/` as JSON, one file per system, study and
collection (the posting-list study's under `results/postings/`); logs of the
runs that produced them are in `results/logs/`.
