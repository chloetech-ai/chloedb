/* The capability comparison on compare.html (and a section of it on
 * command-center.html).
 *
 * A cell is a code, optionally "?" (not yet checked against that project's
 * current documentation), optionally ":" and a note shown on hover:
 *   y  yes, built in         p  partial, by extension, or by hand
 *   n  no                    $  only in a paid tier
 *   e  yes, experimental (ChloeDB's docs/compatibility.md lists it)
 *   t  text (the licence row)
 * Systems missing from a row read as "n".
 *
 * ChloeDB's cells say what its docs say, of the prebuilt release, image and
 * wheel: a feature that needs a build with --features, that a cluster
 * refuses (docs/chloedb/cluster-limits.md) or that is experimental says so.
 *
 * `checked` is the day each system's cells, its "?" cells excepted, were last
 * checked against its documentation: the audit of 15 November 2026
 * (findings CMP-1 and CMP-2) re-checked the cells it names; Solr and SQLite
 * were not re-checked then. While any cell is "?", the page shows `draft`
 * above the matrix, {n} the number of such cells, and
 * docs/site/scripts/check_compare.py (CI's documentation job) fails without
 * it.
 */
window.STRATA_COMPARE = {
  draft: "{n} cells about other systems, in dashed boxes, are not yet checked against each project's current documentation, and may be wrong. The others were checked in November 2026; hover a mark for the day.",
  systems: [
    { id: "strata", name: "ChloeDB", sub: "embedded or server", group: "self" },
    { id: "sqlite", name: "SQLite", sub: "FTS5, sqlite-vec", group: "embedded", checked: "1 November 2026" },
    { id: "postgres", name: "PostgreSQL", sub: "pgvector", group: "embedded", checked: "15 November 2026" },
    { id: "lancedb", name: "LanceDB", sub: "embedded", group: "embedded", checked: "15 November 2026" },
    { id: "elastic", name: "Elasticsearch", sub: "9, Basic licence", group: "search", checked: "15 November 2026" },
    { id: "opensearch", name: "OpenSearch", sub: "3", group: "search", checked: "15 November 2026" },
    { id: "solr", name: "Solr", sub: "10", group: "search", checked: "1 November 2026" },
    { id: "vespa", name: "Vespa", sub: "8", group: "search", checked: "15 November 2026" },
    { id: "qdrant", name: "Qdrant", sub: "", group: "vector", checked: "15 November 2026" },
    { id: "weaviate", name: "Weaviate", sub: "", group: "vector", checked: "15 November 2026" },
    { id: "milvus", name: "Milvus", sub: "", group: "vector", checked: "15 November 2026" }
  ],
  groups: [
    { id: "all", label: "All" },
    { id: "search", label: "Search engines" },
    { id: "vector", label: "Vector databases" },
    { id: "embedded", label: "Embedded and relational" }
  ],
  sections: [
    {
      title: "Where it runs",
      rows: [
        ["Runs inside your program, as a library", {
          strata: "y:A Rust crate, or Python in-process like sqlite3", sqlite: "y", postgres: "n", lancedb: "y",
          elastic: "n", opensearch: "n", solr: "n", vespa: "n",
          qdrant: "p:Qdrant Edge, an in-process build for Python and Rust (beta); the Python client's local mode is meant for tests", weaviate: "p:Embedded Weaviate, experimental, for development on Linux and macOS", milvus: "p:Milvus Lite, a separate build with the same API" }],
        ["The whole database in one file", {
          strata: "y:One file and its write-ahead log, or a directory of segments", sqlite: "y", postgres: "n", lancedb: "n:A directory of Lance files",
          milvus: "p:Milvus Lite keeps one local file" }],
        ["A network server (HTTP or gRPC)", {
          strata: "y:HTTP JSON and gRPC, API keys with scopes, TLS", sqlite: "n", postgres: "y:Its own wire protocol", lancedb: "$:LanceDB Enterprise; the open-source library has no server",
          elastic: "y", opensearch: "y", solr: "y", vespa: "y", qdrant: "y", weaviate: "y", milvus: "y" }],
        ["Shards and replicas", {
          strata: "y:A coordinator over shards; slots move while it serves; a primary ships writes to replicas", postgres: "p:Streaming replicas; sharding through Citus", lancedb: "$:LanceDB Enterprise",
          elastic: "y", opensearch: "y", solr: "y:SolrCloud", vespa: "y", qdrant: "y", weaviate: "y", milvus: "y" }],
        ["Automatic failover", {
          strata: "y:A shard's copies elect a new primary; writes acknowledged by all, a majority or the primary. The cluster map has no consensus yet: change it through one coordinator at a time", postgres: "p:Through Patroni or an operator",
          elastic: "y", opensearch: "y", solr: "y:Through ZooKeeper", vespa: "y", qdrant: "y:Raft", weaviate: "y:Raft for the schema, and replicated data", milvus: "y" }],
        ["A Kubernetes operator", {
          strata: "y:StrataCluster: scale shards and copies, roll out settings with gates, roll back", postgres: "y:CloudNativePG and others",
          elastic: "y:ECK", opensearch: "y:OpenSearch Kubernetes Operator", solr: "y:Solr Operator", vespa: "$:The Vespa Kubernetes Operator (VespaSet) is commercial; otherwise Helm charts and sample manifests",
          qdrant: "p:A Helm chart; an operator in the paid Hybrid Cloud", weaviate: "p:A Helm chart", milvus: "y:Milvus Operator" }],
        ["The same API from a laptop file to a cluster", {
          strata: "p:The same SQL, schema and file format at every size; a cluster refuses graph queries, recommenders, DROP COLUMN, the relevance autopilot and a SQLite export of the database (What a cluster does differently)", sqlite: "n:One machine", postgres: "p:The same SQL; no embedded mode", lancedb: "p?:The open-source library, then LanceDB Cloud",
          qdrant: "p:Local mode, then the server, with one client", weaviate: "p?", milvus: "p:Milvus Lite, then Milvus" }],
        ["Older data kept on object storage (S3) and queried there", {
          strata: "y:Cold segments on S3 or a compatible store, read through a local cache", lancedb: "y:Tables on S3 and other object stores",
          elastic: "$:Searchable snapshots, Enterprise licence", opensearch: "y:Searchable snapshots and remote-backed storage",
          weaviate: "p:Inactive tenants offloaded to S3", milvus: "y:Segments are kept on object storage" }],
        ["Another cluster kept a moment behind, to switch over to", {
          strata: "y:chloedb-server follow; a planned switch-over loses no write", postgres: "y:A standby by streaming replication, promoted by hand",
          elastic: "$:Cross-cluster replication, Platinum licence", opensearch: "y:The cross-cluster replication plugin", solr: "n:CDCR was removed in Solr 9",
          milvus: "p?:Milvus CDC, a separate tool" }]
      ]
    },
    {
      title: "Text search",
      rows: [
        ["BM25 with boolean queries and phrases", {
          strata: "y:MATCH with AND, OR, NOT, phrases; MaxScore and Block-Max WAND", sqlite: "y:FTS5", postgres: "p:ts_rank is not BM25; BM25 through extensions such as ParadeDB", lancedb: "y:Full-text search with BM25",
          elastic: "y", opensearch: "y", solr: "y", vespa: "y", qdrant: "y:BM25 on full-text indexes, from 1.20", weaviate: "y", milvus: "y:A BM25 function over text fields, from 2.5" }],
        ["Typos and synonyms", {
          strata: "y:word~ for edits, synonym sets", sqlite: "p:The spellfix1 extension, by hand", postgres: "p:pg_trgm and thesaurus dictionaries", lancedb: "p?",
          elastic: "y", opensearch: "y", solr: "y", vespa: "y", weaviate: "p?", milvus: "p?" }],
        ["Autocomplete that tolerates typos", {
          strata: "y:PREFIX(..., fuzzy => 'auto'), a trie walk of the dictionary", sqlite: "p:Prefix queries in FTS5, no typos", postgres: "p:pg_trgm",
          elastic: "y:Completion suggester", opensearch: "y", solr: "y:Suggester", vespa: "p?" }],
        ["Facets and aggregations over a search", {
          strata: "y:GROUP BY over any result", sqlite: "y:GROUP BY", postgres: "y", lancedb: "p",
          elastic: "y", opensearch: "y", solr: "y", vespa: "y:Grouping", qdrant: "p:Facet counts", weaviate: "p:Aggregate", milvus: "y:Faceted search, from 3.0" }]
      ]
    },
    {
      title: "Vectors",
      rows: [
        ["A vector index (HNSW or IVF)", {
          strata: "y:HNSW, IVF or exact; float32 with 8-bit, binary or RaBitQ codes of 1, 2 or 4 bits", sqlite: "p?:sqlite-vec scans; no ANN index", postgres: "y:HNSW and IVFFlat", lancedb: "y",
          elastic: "y", opensearch: "y", solr: "y", vespa: "y", qdrant: "y", weaviate: "y", milvus: "y" }],
        ["Filtered vector search", {
          strata: "y:Planned per segment from exact counts: an exact scan, the filter inside HNSW, or a post-filter", sqlite: "p", postgres: "p:Iterative index scans, from pgvector 0.8", lancedb: "p:Pre- or post-filter, chosen by the caller",
          elastic: "y", opensearch: "y", solr: "y", vespa: "y", qdrant: "y", weaviate: "y", milvus: "y" }],
        ["Hybrid fusion: RRF and normalised weighted sums", {
          strata: "y:rrf(), and minmax(), tmm() or zscore() in any weighted sum", sqlite: "p:Written by hand in SQL", postgres: "p:Written by hand in SQL", lancedb: "y",
          elastic: "$:RRF and linear retrievers need a paid licence; the free Basic licence refused RRF on 28 October",
          opensearch: "y:Normalisation and RRF processors in search pipelines", solr: "p?:RRF in Solr 10; no normalised sum", vespa: "y:Any ranking expression",
          qdrant: "y:RRF and DBSF", weaviate: "y:Ranked and relative-score fusion", milvus: "y:RRF and weighted rankers" }],
        ["Multi-vector late interaction (ColBERT, ColPali)", {
          strata: "y:maxsim() over VECTOR(d)[] fields with token pooling; ColBERT token vectors embedded on write in a build with --features bert, not in the release, image or wheel", postgres: "p?:Through extensions such as VectorChord", lancedb: "y:Multivector columns, cosine distance only",
          elastic: "$:The rank_vectors field, Enterprise licence", opensearch: "p:Reranking with a late-interaction model", vespa: "y", qdrant: "y", weaviate: "y:Multi-vector embeddings, with MUVERA", milvus: "y:EmbList and StructArray fields, from 3.0" }],
        ["Learned sparse vectors (SPLADE, ELSER)", {
          strata: "y:SPARSE fields", postgres: "y:pgvector's sparsevec", lancedb: "n?",
          elastic: "y", opensearch: "y", solr: "n?", vespa: "y", qdrant: "y", weaviate: "n?", milvus: "y" }],
        ["Embeddings computed on write", {
          strata: "y:Model2Vec models locally, or OpenAI, Cohere, Voyage, Gemini, Mistral, Jina, Ollama; BERT, image and sound models in a build with --features bert, vision or audio, not in the release, image or wheel; missing vectors backfilled", postgres: "p:Extensions such as pgai", lancedb: "y:Embedding functions",
          elastic: "$:semantic_text through the inference API, Enterprise licence; ELSER and E5 on ML nodes, Platinum or Enterprise", opensearch: "y:Ingest pipelines with ML Commons", solr: "p?", vespa: "y:Embedders",
          qdrant: "p:Client-side FastEmbed, or cloud inference", weaviate: "y:Vectorizer modules", milvus: "y:Embedding functions" }]
      ]
    },
    {
      title: "Ranking",
      rows: [
        ["Ranking expressions over scores and columns", {
          strata: "y:Any arithmetic of scores and columns, decay(), rank-safe boosts, rerank phases", sqlite: "p:ORDER BY expressions over bm25()", postgres: "p", lancedb: "p",
          elastic: "y:function_score, script_score", opensearch: "y", solr: "y:Function queries", vespa: "y", qdrant: "p:Formula queries", weaviate: "p:The Boost API, generally available from 1.39", milvus: "p:Decay, boost and function chains, from 3.0" }],
        ["Gradient-boosted ranking models (LightGBM, XGBoost)", {
          strata: "y:gbdt() in a rerank phase, bit-exact with LightGBM and XGBoost",
          elastic: "$:Learning to rank, Enterprise licence", opensearch: "y:The LTR plugin", solr: "y:The LTR module", vespa: "y", milvus: "p:XGBoost scoring at the first stage (L0), from 3.0" }],
        ["Cross-encoder reranking inside the query", {
          strata: "y:Cohere, Voyage, Jina or a TEI server of your own; local models (ONNX Runtime int8, or candle) in a build with --features onnx or bert, not in the release, image or wheel", lancedb: "y:Rerankers",
          elastic: "$:text_similarity_reranker and Elastic Rerank, Enterprise licence", opensearch: "y", solr: "p?", vespa: "y", weaviate: "y:Reranker modules", milvus: "y:Rerank functions" }],
        ["Rank log, interleaving and LLM relevance judgments", {
          strata: "y:Logged features for training, team-draft interleaving, UMBRELA judgments by Claude",
          opensearch: "p:The Search Relevance Workbench" }]
      ]
    },
    {
      title: "Recommendations",
      rows: [
        ["Sequence models in the query (the next item from a history)", {
          strata: "y:encode() runs a SASRec-style model trained with chloedb.train", vespa: "p:ONNX models in ranking" }],
        ["Items like a user's recent history", {
          strata: "y:maxsim() over the history's vectors, or ppr() over co-occurrence edges on one server (a cluster refuses ppr())", postgres: "p:Hand-written SQL", lancedb: "p",
          elastic: "p", opensearch: "p", vespa: "y", qdrant: "y:The recommend API", weaviate: "p?", milvus: "p?" }],
        ["Semantic IDs", {
          strata: "y:Residual-quantised IDs per row, and the next codes of a prefix" }],
        ["Counted next items, decayed in place", {
          strata: "y:strata.next_items over an n-gram table; UPDATE ... SET n = n * 0.9 rewrites no row" }]
      ]
    },
    {
      title: "SQL and data",
      rows: [
        ["SQL with joins, subqueries, WITH, window functions and UNION", {
          strata: "y:DataFusion runs the large plans, spilling to disk past their memory budget", sqlite: "y", postgres: "y", lancedb: "p:SQL filters; full SQL through DuckDB or DataFusion",
          elastic: "p:ES|QL and SQL, with lookup joins", opensearch: "p:The SQL and PPL plugin", solr: "p:Parallel SQL", vespa: "p:YQL, without joins",
          weaviate: "n:GraphQL" }],
        ["ACID transactions with snapshot isolation", {
          strata: "y:Shadow-paged commits; readers never wait for the writer. On a cluster a write across shards is one transaction a shard", sqlite: "y", postgres: "y", lancedb: "p:Versioned commits per table",
          elastic: "n:Near real time, versions per document" }],
        ["Graph traversal and personalized PageRank", {
          strata: "p:REACHABLE, PATH and ppr() over edge fields or edge tables, seeded by any query, on one server; a cluster refuses them", sqlite: "p:Recursive CTEs", postgres: "p:Recursive CTEs, or Apache AGE",
          solr: "p:The graph query parser and streaming expressions" }],
        ["Schemaless JSON attributes, indexed", {
          strata: "y:JSON fields, filtered, grouped and sorted by path, their strings searched as text, listed by json_paths()", sqlite: "p:JSON functions and indexes on expressions", postgres: "y:jsonb with GIN", lancedb: "p",
          elastic: "y:Dynamic mapping", opensearch: "y", solr: "y:Dynamic fields", vespa: "n?", qdrant: "y:Payload", weaviate: "p:Auto-schema", milvus: "y:JSON and dynamic fields" }],
        ["Reads and writes by key, with a time to live per row", {
          strata: "y:GET, PUT and DELETE on /v1/kv, 10,000 keys a call, expire_by per row; a coordinator asks only the key's shard",
          sqlite: "p:By primary key in SQL; no time to live", postgres: "p:By primary key in SQL; no time to live", lancedb: "p?:Lookups by filter; no time to live",
          elastic: "p:GET and _mget by id; no time to live per document", opensearch: "p:GET and _mget by id; no time to live per document",
          solr: "y?:Real-time get, and documents that expire by a field", vespa: "y?:The document API, and garbage collection by a selection",
          qdrant: "p?:Points by id; no time to live", weaviate: "p?:Objects by id; no time to live", milvus: "p?:Get by primary key; a time to live per collection" }],
        ["Change columns while serving", {
          strata: "p:ALTER TABLE ADD, DROP and RENAME COLUMN on one server; a cluster adds and renames, and refuses DROP COLUMN", sqlite: "y", postgres: "y", lancedb: "y?",
          elastic: "p:Add fields; other changes reindex", opensearch: "p", solr: "p", vespa: "p:Some changes need a reindex", qdrant: "y:The payload has no schema", weaviate: "p?", milvus: "y:Online add and drop of a column, from 3.0" }],
        ["Import from and export to SQLite", {
          strata: "y:chloedb-server import and export, or one call from Python; a cluster imports, and exports tables as Parquet, JSON lines, CSV or Arrow rather than SQLite", sqlite: "y", postgres: "p:pgloader" }],
        ["User-defined functions (UDFs) kept in the database (CREATE FUNCTION)", {
          strata: "y:SQL bodies that return a value or rows, called from any query, on one server or a cluster", postgres: "y",
          sqlite: "p:Functions are registered by the program that opens the file, not kept in it",
          elastic: "p:Stored scripts in Painless", opensearch: "p:Stored scripts in Painless", solr: "p:Function queries; new functions are Java plugins",
          vespa: "p:Functions inside rank profiles" }],
        ["A large answer read page by page (cursors, scroll)", {
          strata: "y:A scroll from one version of the database, on one server or across shards", sqlite: "y:A statement steps through its rows", postgres: "y:DECLARE CURSOR",
          lancedb: "y?:Results as a stream of Arrow batches",
          elastic: "y:search_after with a point in time, and scroll", opensearch: "y:scroll, search_after and point in time", solr: "y:cursorMark", vespa: "y:Visiting",
          qdrant: "y:The scroll API", weaviate: "y:The cursor API", milvus: "y:Query and search iterators" }],
        ["Follows a PostgreSQL database by logical replication", {
          strata: "y:chloedb-server follow-postgres: pgoutput, each change applied once across restarts", postgres: "y:Logical replication between servers",
          elastic: "p:A PostgreSQL connector that syncs on a schedule, or Debezium and Kafka", opensearch: "p?:Through Debezium or a pipeline" }],
        ["Answers OpenSearch's REST API, for existing apps and clients", {
          strata: "p:Indexes, documents, _bulk and _search: bool, match, phrases, multi_match, query_string, aggregations, scrolls and a part of Painless",
          elastic: "y:The API OpenSearch began from", opensearch: "y" }],
        ["Images and sound by reference, embedded on write", {
          strata: "p:MEDIA fields by reference in every build; SigLIP and CLAP vectors, captions and transcripts in a build with --features vision or audio, not in the release, image or wheel", lancedb: "y:Multimodal tables",
          elastic: "p?:A multimodal semantic field, a technical preview in 9.5", opensearch: "p:A multimodal ingest processor", vespa: "p:Embedders over images", qdrant: "p?", weaviate: "y:multi2vec modules", milvus: "p?" }]
      ]
    },
    {
      title: "Command center and operations",
      rows: [
        ["A console served by the database itself", {
          strata: "y:GET / on every server, compiled into the binary, offline; nothing else to run", postgres: "n:pgAdmin and others run separately",
          elastic: "n:Kibana, a separate service to run and secure", opensearch: "n:OpenSearch Dashboards, a separate service", solr: "y:The admin UI",
          vespa: "n?:None bundled; Vespa Cloud has a console", qdrant: "y:A web UI at /dashboard", weaviate: "n?:A console in Weaviate Cloud", milvus: "n:Attu, a separate service" }],
        ["SQL editor with completion, plans and charts", {
          strata: "y:Completes tables, fields and JSON keys; the plan of each segment; scores as bars; charts and profiles", postgres: "p:In pgAdmin, separately",
          elastic: "p:Dev Tools for the query DSL, ES|QL in Discover", opensearch: "y?:Query Workbench for SQL and PPL", solr: "p:A query form",
          qdrant: "p?:A console for its REST calls", milvus: "p?:A data explorer in Attu" }],
        ["A visual query builder that writes the search", {
          strata: "y:Drag words, filters, boosts and groups into stages; the SQL is written and run as you build, with each stage's rows",
          elastic: "p?:Filters and Lens in Kibana explore data; they do not build a ranking", opensearch: "p?:Filters and visualisations in Dashboards" }],
        ["The schema drawn, and tables designed by dragging", {
          strata: "y:Tables as cards linked by references and edges; types dragged into a new table, checked as you go", postgres: "p?:ER diagrams in pgAdmin",
          solr: "p:A schema browser", milvus: "p?:Collections and fields in Attu" }],
        ["Query profiles as flame charts; CPU and memory flame graphs", {
          strata: "y:Any query on any node, beside its trace; CPU and allocation flame graphs of the server", postgres: "p:EXPLAIN ANALYZE as text",
          elastic: "p:The Search Profiler in Kibana, a tree of times", opensearch: "p?:Query Insights and the profile API", solr: "p:debug=timing per component", vespa: "p?:Query traces" }],
        ["Relevance tuned on graded queries, side by side", {
          strata: "y:Two rankings side by side, grades by you or Claude, nDCG, a hybrid's weight tuned, the winner kept as a rank profile",
          elastic: "p?:Quepid, a separate tool", opensearch: "y?:The Search Relevance Workbench", solr: "p?:Quepid, a separate tool" }],
        ["Recommendations measured against what users did next", {
          strata: "y:The rank log joined to events: nDCG, hit rate, lift and its significance, interleaving" }],
        ["The cluster drawn live, with what-if failures", {
          strata: "y:Every copy with calls on its links and its lag; fail machines in a what-if", elastic: "p?:Stack Monitoring lists nodes and shards", opensearch: "p?:Node and shard lists",
          solr: "p:A graph of collections, shards and replicas", qdrant: "p?:Cluster status", milvus: "p?:A system view in Attu" }],
        ["The cluster changed by drawing it, with the plan first", {
          strata: "y:The control center: drag the shape or say it in a sentence; guardrails, estimates and the operator's plan before Apply",
          elastic: "p?:Resizing in Elastic Cloud's console", vespa: "p?:In Vespa Cloud" }],
        ["Rollouts with health gates, pause and rollback", {
          strata: "y:Gates as live charts with limits to drag; every spec version with its diff and a rollback", elastic: "p?:Upgrades in Elastic Cloud and ECK" }],
        ["Autoscaling, rehearsed on past load", {
          strata: "e:Rules in the operator that act, wait for a person, or stay off; the last week's load replays on them first",
          postgres: "n?:None in PostgreSQL; managed services scale it", elastic: "$:ECK autoscales with an Enterprise licence; Elastic Cloud and Serverless scale themselves", opensearch: "n?:None when you run it; Amazon OpenSearch Serverless scales itself", solr: "n:The autoscaling framework was removed in Solr 9", vespa: "$:Within resource ranges in Vespa Cloud, and with the commercial Kubernetes operator; none in the open-source deployment",
          qdrant: "n?:None when you run it", weaviate: "n?:None when you run it", milvus: "n?:None when you run it; Zilliz Cloud Serverless scales itself" }],
        ["Plans, makes and deletes a cloud cluster, with its cost", {
          strata: "e:The Cloud view plans EKS or DigitalOcean Kubernetes from live prices and quotas, makes the cluster, follows it and deletes it" }],
        ["The database's own logs, searched with SQL", {
          strata: "y:Every server's lines and queries kept in ChloeDB, across the cluster", elastic: "p:Its logs once shipped into it", opensearch: "p:Its logs once shipped into it",
          solr: "p:The latest lines in the admin UI" }],
        ["An agent that investigates alerts and proposes fixes to approve", {
          strata: "e:The on-call agent: read-only tools, and proposals an admin approves within the hour", elastic: "$:The AI Assistant for Observability, now deprecated for the Elastic AI Agent, with a paid subscription" }],
        ["A rehearsal of the cluster under load, and other shapes predicted", {
          strata: "y:The Simulator: a test mix at a rising rate, graded against the SLO, with what more copies, shards or other machines would hold and cost" }],
        ["Licence", {
          strata: "t:Apache-2.0, console included", sqlite: "t:Public domain", postgres: "t:PostgreSQL", lancedb: "t:Apache-2.0",
          elastic: "t:AGPLv3, SSPL or ELv2; Kibana likewise", opensearch: "t:Apache-2.0", solr: "t:Apache-2.0", vespa: "t:Apache-2.0",
          qdrant: "t:Apache-2.0", weaviate: "t:BSD-3-Clause core; some features under the Weaviate License", milvus: "t:Apache-2.0" }]
      ]
    }
  ]
};
