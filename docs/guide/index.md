# ChloeDB documentation

ChloeDB is an embedded search database with SQL: full text, structured columns, vectors and graph edges over the same rows, in one file, combined in one query by one planner. It runs inside your program like SQLite, or as a server and a cluster with the same SQL.

<div class="doc-cards" markdown="1">
<a href="../quickstart.md"><b>Quickstart</b><span>Install it and run your first search in five minutes.</span></a>
<a href="concepts.md"><b>Concepts</b><span>The file, tables, the three indexes, scores and the planner.</span></a>
<a href="search.md"><b>Search guide</b><span>Words, typos, filters, facets, vectors and hybrid ranking, on one small catalogue.</span></a>
<a href="../chloedb/query-language.md"><b>SQL reference</b><span>Every statement, function and option.</span></a>
</div>

## Where to start

| you are making | start with | then |
|---|---|---|
| a Python app, script or notebook, with the database in your process | [the quickstart's Python section](../quickstart.md#chloedb-from-python-with-no-server) (`pip install chloedb`, no server) | [Python](../chloedb/python.md), the [search guide](search.md), and [Build a search page](search-page.md) for a web app |
| a JavaScript or TypeScript app (Node, Bun, Deno, a framework's server code) | [JavaScript](../chloedb/javascript.md): `npm install chloedb` and a database in your process | the [search guide](search.md), whose queries run as `await db.query(sql, params)` |
| a server that several programs, languages or machines use | [the quickstart](../quickstart.md): install `chloedb-server`, a table, a search, the command center | [chloedb-server](../chloedb/server.md), and [Build an app on chloedb-server](../chloedb/build-an-app.md) for an app over its HTTP API |
| a cluster: shards, replicas, failover | [Operate a cluster](operate-cluster.md), on one machine first | [Runbooks](../chloedb/runbooks.md) and [cluster limits](../chloedb/cluster-limits.md) |
| retrieval for a language model | [Retrieval for RAG](rag.md) | [LangChain, LangGraph, LlamaIndex](frameworks.md) |
| a move from Elasticsearch or OpenSearch | [the OpenSearch API](../chloedb/opensearch.md): `--opensearch-listen` answers OpenSearch's REST API, so an app on an OpenSearch or Elasticsearch 7 client indexes and searches unchanged, and its [differences](../chloedb/opensearch.md#differences) say what it refuses | the [search guide](search.md) for the same searches in SQL, and [multi-tenant isolation](../chloedb/security.md#api-keys-and-scopes) for an index per customer |
| a move from PostgreSQL's SQL | [Differences from PostgreSQL](../chloedb/query-language.md#differences-from-postgresql): what answers otherwise and what is refused, with what to write instead | [Following Postgres](../chloedb/postgres.md) copies a Postgres database in and keeps it current |
| long-term memory for Claude Code | [Install ChloeCtx](../chloectx/setup.md): it plugs into Claude Code, and needs neither chloedb-server nor Python | [ChloeCtx](../chloectx/index.md) |

Something failed? [Troubleshooting](../chloedb/troubleshooting.md) has the usual messages, each with what to do, and `chloedb-server doctor` checks this machine for their causes.

## Cookbook

Complete programs for common jobs. Each one runs, as written, every time the code changes.

<div class="doc-cards" markdown="1">
<a href="search-page.md"><b>Build a search page</b><span>A search box for a small web app: typos, highlights, did you mean, more like this and a filter.</span></a>
<a href="rag.md"><b>Retrieval for RAG</b><span>Chunk documents, search them by words and meaning, and hand a model the passages with their sources.</span></a>
<a href="agent-memory.md"><b>Memory for an agent</b><span>Remember turns and facts, recall what matters to a new message, and keep facts current.</span></a>
<a href="crawl-to-search.md"><b>Crawl a site into search</b><span>Point the crawler at a site, get a table with its fields, and search it.</span></a>
<a href="logs.md"><b>Search your logs</b><span>Send syslog and OpenTelemetry logs to a table, and find the last 15 minutes' errors, a flapping interface, an IP.</span></a>
</div>

## Use it from

| | |
|---|---|
| [Python](../chloedb/python.md) | `import chloedb`: a database in your process, or a client of a server, with the same methods; [DataFrames](../chloedb/frames.md) too |
| [JavaScript](../chloedb/javascript.md) | Node, Bun and Deno in-process, or a `fetch` client for any runtime |
| [Java](../chloedb/java.md) | in-process over JNI, and a JDBC driver |
| [HTTP and gRPC](../chloedb/server.md) | chloedb-server, its command center, and a cluster of them |

## Apps

[Apps on a cluster](apps.md) run beside chloedb-server and show in its command center: the [crawler](../chloedb/crawler.md), [Sheets](../chloedb/sheets.md) and [ChloeCtx](../chloectx/index.md), memory for Claude Code.

## How these pages are tested

The guide and cookbook pages are run as scripts: [`docs/guide/run.py`](run.py) runs every code block on a page, in order, in an empty directory, against the programs built from the same commit, and checks the output shown under a block against what it printed. A page that says so at its top has passed. The reference pages (SQL, server, clients, apps) are rendered from the Markdown in [`docs/chloedb/`](../chloedb/index.md) and [`docs/chloectx/`](../chloectx/index.md).
