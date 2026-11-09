/* The capability comparison on compare.html.
 *
 * A cell is a code, optionally "?" (not yet checked against that project's
 * current documentation), optionally ":" and a note shown on hover:
 *   y  yes, built in         p  partial, by extension, or by hand
 *   n  no                    $  only in a paid tier
 *   t  text (the licence row)
 * Systems missing from a row read as "n".
 *
 * DRAFT, 1 November 2026: every "?" cell, and every claim about another
 * system, is to be checked against its current documentation before this
 * page is published.
 */
window.STRATA_COMPARE = {
  systems: [
    { id: "strata", name: "ChloeDB", sub: "embedded or server", group: "self" },
    { id: "sqlite", name: "SQLite", sub: "FTS5, sqlite-vec", group: "embedded" },
    { id: "postgres", name: "PostgreSQL", sub: "pgvector", group: "embedded" },
    { id: "lancedb", name: "LanceDB", sub: "embedded", group: "embedded" },
    { id: "elastic", name: "Elasticsearch", sub: "9, Basic licence", group: "search" },
    { id: "opensearch", name: "OpenSearch", sub: "3", group: "search" },
    { id: "solr", name: "Solr", sub: "10", group: "search" },
    { id: "vespa", name: "Vespa", sub: "8", group: "search" },
    { id: "qdrant", name: "Qdrant", sub: "", group: "vector" },
    { id: "weaviate", name: "Weaviate", sub: "", group: "vector" },
    { id: "milvus", name: "Milvus", sub: "", group: "vector" }
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
          qdrant: "p?:Local mode of the Python client, meant for tests", weaviate: "p?:Embedded Weaviate, experimental", milvus: "p:Milvus Lite, a separate build with the same API" }],
        ["The whole database in one file", {
          strata: "y:One file and its write-ahead log, or a directory of segments", sqlite: "y", postgres: "n", lancedb: "n:A directory of Lance files",
          milvus: "p?:Milvus Lite keeps one local file" }],
        ["A network server (HTTP or gRPC)", {
          strata: "y:HTTP JSON and gRPC, API keys with scopes, TLS", sqlite: "n", postgres: "y:Its own wire protocol", lancedb: "p?:LanceDB Cloud and Enterprise",
          elastic: "y", opensearch: "y", solr: "y", vespa: "y", qdrant: "y", weaviate: "y", milvus: "y" }],
        ["Shards and replicas", {
          strata: "y:A coordinator over shards; slots move while it serves; a primary ships writes to replicas", postgres: "p:Streaming replicas; sharding through Citus", lancedb: "p?:LanceDB Enterprise",
          elastic: "y", opensearch: "y", solr: "y:SolrCloud", vespa: "y", qdrant: "y", weaviate: "y", milvus: "y" }],
        ["Automatic failover", {
          strata: "y:A shard's copies elect a new primary; writes acknowledged by all, a majority or the primary", postgres: "p:Through Patroni or an operator",
          elastic: "y", opensearch: "y", solr: "y:Through ZooKeeper", vespa: "y", qdrant: "y:Raft", weaviate: "y?", milvus: "y" }],
        ["A Kubernetes operator", {
          strata: "y:StrataCluster: scale shards and copies, roll out settings with gates, roll back", postgres: "y:CloudNativePG and others",
          elastic: "y:ECK", opensearch: "y:OpenSearch Kubernetes Operator", solr: "y:Solr Operator", vespa: "p?:Helm charts and sample manifests; Vespa Cloud is managed",
          qdrant: "p?:A Helm chart; an operator in the commercial offering", weaviate: "p:A Helm chart", milvus: "y:Milvus Operator" }],
        ["The same API from a laptop file to a cluster", {
          strata: "y:The same SQL, schema and file format at every size", sqlite: "n:One machine", postgres: "p:The same SQL; no embedded mode", lancedb: "p?:The open-source library, then LanceDB Cloud",
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
          strata: "y:MATCH with AND, OR, NOT, phrases; MaxScore and Block-Max WAND", sqlite: "y:FTS5", postgres: "p:ts_rank is not BM25; BM25 through extensions such as ParadeDB", lancedb: "y?",
          elastic: "y", opensearch: "y", solr: "y", vespa: "y", qdrant: "p?:Full-text filters; BM25 as sparse vectors with IDF", weaviate: "y", milvus: "y?:A BM25 function, from 2.5" }],
        ["Typos and synonyms", {
          strata: "y:word~ for edits, synonym sets", sqlite: "p:The spellfix1 extension, by hand", postgres: "p:pg_trgm and thesaurus dictionaries", lancedb: "p?",
          elastic: "y", opensearch: "y", solr: "y", vespa: "y", weaviate: "p?", milvus: "p?" }],
        ["Autocomplete that tolerates typos", {
          strata: "y:PREFIX(..., fuzzy => 'auto'), a trie walk of the dictionary", sqlite: "p:Prefix queries in FTS5, no typos", postgres: "p:pg_trgm",
          elastic: "y:Completion suggester", opensearch: "y", solr: "y:Suggester", vespa: "p?" }],
        ["Facets and aggregations over a search", {
          strata: "y:GROUP BY over any result", sqlite: "y:GROUP BY", postgres: "y", lancedb: "p?",
          elastic: "y", opensearch: "y", solr: "y", vespa: "y:Grouping", qdrant: "p?:Facet counts", weaviate: "p:Aggregate", milvus: "p?" }]
      ]
    },
    {
      title: "Vectors",
      rows: [
        ["A vector index (HNSW or IVF)", {
          strata: "y:HNSW, IVF or exact; float32 with 8-bit, binary or RaBitQ codes of 1, 2 or 4 bits", sqlite: "p?:sqlite-vec scans; no ANN index", postgres: "y:HNSW and IVFFlat", lancedb: "y",
          elastic: "y", opensearch: "y", solr: "y", vespa: "y", qdrant: "y", weaviate: "y", milvus: "y" }],
        ["Filtered vector search", {
          strata: "y:Planned per segment from exact counts: an exact scan, the filter inside HNSW, or a post-filter", sqlite: "p", postgres: "p?:Iterative index scans", lancedb: "p:Pre- or post-filter, chosen by the caller",
          elastic: "y", opensearch: "y", solr: "y", vespa: "y", qdrant: "y", weaviate: "y", milvus: "y" }],
        ["Hybrid fusion: RRF and normalised weighted sums", {
          strata: "y:rrf(), and minmax(), tmm() or zscore() in any weighted sum", sqlite: "p:Written by hand in SQL", postgres: "p:Written by hand in SQL", lancedb: "y",
          elastic: "$:RRF and linear retrievers need a paid licence; the free Basic licence refused RRF on 28 October",
          opensearch: "y:Normalisation and RRF processors in search pipelines", solr: "p?:RRF in Solr 10; no normalised sum", vespa: "y:Any ranking expression",
          qdrant: "y:RRF and DBSF", weaviate: "y:Ranked and relative-score fusion", milvus: "y:RRF and weighted rankers" }],
        ["Multi-vector late interaction (ColBERT, ColPali)", {
          strata: "y:maxsim() over VECTOR(d)[] fields with token pooling; ColBERT token vectors embedded on write", postgres: "p?:Through extensions such as VectorChord", lancedb: "y?",
          elastic: "p?:The rank_vectors field", opensearch: "n?", vespa: "y", qdrant: "y", weaviate: "y?", milvus: "p?" }],
        ["Learned sparse vectors (SPLADE, ELSER)", {
          strata: "y:SPARSE fields", postgres: "y:pgvector's sparsevec", lancedb: "n?",
          elastic: "y", opensearch: "y", solr: "n?", vespa: "y", qdrant: "y", weaviate: "n?", milvus: "y" }],
        ["Embeddings computed on write", {
          strata: "y:Local text, image and sound models, or OpenAI, Cohere, Voyage, Gemini, Mistral, Jina, Ollama; missing vectors backfilled", postgres: "p?:Extensions such as pgai", lancedb: "y:Embedding functions",
          elastic: "y:semantic_text and the inference API", opensearch: "y:Ingest pipelines with ML Commons", solr: "p?", vespa: "y:Embedders",
          qdrant: "p?:Client-side FastEmbed, or cloud inference", weaviate: "y:Vectorizer modules", milvus: "y?:Embedding functions" }]
      ]
    },
    {
      title: "Ranking",
      rows: [
        ["Ranking expressions over scores and columns", {
          strata: "y:Any arithmetic of scores and columns, decay(), rank-safe boosts, rerank phases", sqlite: "p:ORDER BY expressions over bm25()", postgres: "p", lancedb: "p?",
          elastic: "y:function_score, script_score", opensearch: "y", solr: "y:Function queries", vespa: "y", qdrant: "p?:Formula queries", weaviate: "n?", milvus: "p?" }],
        ["Gradient-boosted ranking models (LightGBM, XGBoost)", {
          strata: "y:gbdt() in a rerank phase, bit-exact with LightGBM and XGBoost",
          elastic: "$?:Learning to rank", opensearch: "y:The LTR plugin", solr: "y:The LTR module", vespa: "y" }],
        ["Cross-encoder reranking inside the query", {
          strata: "y:ONNX Runtime (int8) or candle locally, or Cohere, Voyage, Jina, TEI", lancedb: "y:Rerankers",
          elastic: "$?:text_similarity_reranker with the inference API", opensearch: "y", solr: "p?", vespa: "y", weaviate: "y:Reranker modules", milvus: "y?" }],
        ["Rank log, interleaving and LLM relevance judgments", {
          strata: "y:Logged features for training, team-draft interleaving, UMBRELA judgments by Claude",
          opensearch: "p?:Search Relevance Workbench" }]
      ]
    },
    {
      title: "Recommendations",
      rows: [
        ["Sequence models in the query (the next item from a history)", {
          strata: "y:encode() runs a SASRec-style model trained with strata.train", vespa: "p?:ONNX models in ranking" }],
        ["Items like a user's recent history", {
          strata: "y:maxsim() over the history's vectors, or ppr() over co-occurrence edges", postgres: "p:Hand-written SQL", lancedb: "p",
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
          strata: "y:DataFusion runs the large plans, spilling to disk past their memory budget", sqlite: "y", postgres: "y", lancedb: "p?:SQL through DataFusion or DuckDB",
          elastic: "p:ES|QL and SQL, with lookup joins", opensearch: "p:The SQL and PPL plugin", solr: "p:Parallel SQL", vespa: "p:YQL, without joins",
          weaviate: "n:GraphQL" }],
        ["ACID transactions with snapshot isolation", {
          strata: "y:Shadow-paged commits; readers never wait for the writer", sqlite: "y", postgres: "y", lancedb: "p?:Versioned commits per table",
          elastic: "n:Near real time, versions per document" }],
        ["Graph traversal and personalized PageRank", {
          strata: "y:REACHABLE, PATH and ppr() over edge fields or edge tables, seeded by any query", sqlite: "p:Recursive CTEs", postgres: "p:Recursive CTEs, or Apache AGE",
          solr: "p:The graph query parser and streaming expressions" }],
        ["Schemaless JSON attributes, indexed", {
          strata: "y:JSON fields, filtered, grouped and sorted by path, their strings searched as text, listed by json_paths()", sqlite: "p:JSON functions and indexes on expressions", postgres: "y:jsonb with GIN", lancedb: "p?",
          elastic: "y:Dynamic mapping", opensearch: "y", solr: "y:Dynamic fields", vespa: "n?", qdrant: "y:Payload", weaviate: "p:Auto-schema", milvus: "y:JSON and dynamic fields" }],
        ["Reads and writes by key, with a time to live per row", {
          strata: "y:GET, PUT and DELETE on /v1/kv, 10,000 keys a call, expire_by per row; a coordinator asks only the key's shard",
          sqlite: "p:By primary key in SQL; no time to live", postgres: "p:By primary key in SQL; no time to live", lancedb: "p?:Lookups by filter; no time to live",
          elastic: "p:GET and _mget by id; no time to live per document", opensearch: "p:GET and _mget by id; no time to live per document",
          solr: "y?:Real-time get, and documents that expire by a field", vespa: "y?:The document API, and garbage collection by a selection",
          qdrant: "p?:Points by id; no time to live", weaviate: "p?:Objects by id; no time to live", milvus: "p?:Get by primary key; a time to live per collection" }],
        ["Change columns while serving", {
          strata: "y:ALTER TABLE ADD, DROP and RENAME COLUMN, across a cluster", sqlite: "y", postgres: "y", lancedb: "y?",
          elastic: "p:Add fields; other changes reindex", opensearch: "p", solr: "p", vespa: "p:Some changes need a reindex", qdrant: "y:The payload has no schema", weaviate: "p?", milvus: "p?" }],
        ["Import from and export to SQLite", {
          strata: "y:chloedb-server import and export, or one call from Python", sqlite: "y", postgres: "p:pgloader" }],
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
          strata: "y:MEDIA fields; SigLIP and CLAP models; captions made into text", lancedb: "y:Multimodal tables",
          elastic: "p?", opensearch: "p?:A multimodal ingest processor", vespa: "p?:Embedders over images", qdrant: "p?", weaviate: "y:multi2vec modules", milvus: "p?" }]
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
          strata: "y:Rules in the operator that act, wait for a person, or stay off; the last week's load replays on them first",
          postgres: "n?", elastic: "$?:Autoscaling in Elastic Cloud, and in ECK with an Enterprise licence", opensearch: "n?", solr: "n?", vespa: "p?:In Vespa Cloud",
          qdrant: "n?", weaviate: "n?", milvus: "n?" }],
        ["Plans, makes and deletes a cloud cluster, with its cost", {
          strata: "y:The Cloud view plans EKS or DigitalOcean Kubernetes from live prices and quotas, makes the cluster, follows it and deletes it" }],
        ["The database's own logs, searched with SQL", {
          strata: "y:Every server's lines and queries kept in ChloeDB, across the cluster", elastic: "p:Its logs once shipped into it", opensearch: "p:Its logs once shipped into it",
          solr: "p:The latest lines in the admin UI" }],
        ["An agent that investigates alerts and proposes fixes to approve", {
          strata: "y:The on-call agent: read-only tools, and proposals an admin approves within the hour", elastic: "$?:AI Assistant for Observability, in paid tiers" }],
        ["A rehearsal of the cluster under load, and other shapes predicted", {
          strata: "y:The Simulator: a test mix at a rising rate, graded against the SLO, with what more copies, shards or other machines would hold and cost" }],
        ["Licence", {
          strata: "t:Apache-2.0, console included", sqlite: "t:Public domain", postgres: "t:PostgreSQL", lancedb: "t:Apache-2.0",
          elastic: "t:AGPLv3, SSPL or ELv2; Kibana likewise", opensearch: "t:Apache-2.0", solr: "t:Apache-2.0", vespa: "t:Apache-2.0",
          qdrant: "t:Apache-2.0", weaviate: "t:BSD-3-Clause", milvus: "t:Apache-2.0" }]
      ]
    }
  ]
};
