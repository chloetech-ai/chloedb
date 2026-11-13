# ChloeDB documentation

ChloeDB is an embedded search database with SQL: full text, structured columns, vectors and graph edges over the same rows, in one file, combined in one query by one planner. It runs inside your program like SQLite, or as a server and a cluster with the same SQL.

<div class="doc-cards" markdown="1">
<a href="../quickstart.md"><b>Quickstart</b><span>Install it and run your first search in five minutes.</span></a>
<a href="concepts.md"><b>Concepts</b><span>The file, tables, the three indexes, scores and the planner.</span></a>
<a href="search.md"><b>Search guide</b><span>Words, typos, filters, facets, vectors and hybrid ranking, on one small catalogue.</span></a>
<a href="../chloedb/query-language.md"><b>SQL reference</b><span>Every statement, function and option.</span></a>
</div>

## Cookbook

Complete programs for common jobs. Each one runs, as written, every time the code changes.

<div class="doc-cards" markdown="1">
<a href="rag.md"><b>Retrieval for RAG</b><span>Chunk documents, search them by words and meaning, and hand a model the passages with their sources.</span></a>
<a href="agent-memory.md"><b>Memory for an agent</b><span>Remember turns and facts, recall what matters to a new message, and keep facts current.</span></a>
<a href="crawl-to-search.md"><b>Crawl a site into search</b><span>Point the crawler at a site, get a table with its fields, and search it.</span></a>
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
