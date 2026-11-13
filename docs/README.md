# ChloeDB documentation

ChloeDB is an embedded search database with SQL. One file holds full text, structured columns, vectors and graph edges over the same rows, and one query can use all of them. These pages cover ChloeDB, the server and cluster around it, the apps built on it, and ChloeCtx, its memory for Claude Code.

New here? Start with the [quickstart](quickstart.md), a first search in five minutes, then [build an app](chloedb/build-an-app.md). The [website](https://chloetech-ai.github.io/chloedb/) has the features, the benchmarks and a tour of the command center.

## Get started

| I want to | read |
|---|---|
| Install ChloeDB and run a first search | [Quickstart](quickstart.md) |
| Try a server with demo data in a minute | [Run the server](chloedb/server.md#run-it) |
| Build a small shop on the HTTP API: tables, search, stock, holds, events | [Build an app](chloedb/build-an-app.md) |
| Use ChloeDB inside a Python program | [Python](chloedb/python.md), and [DataFrames](chloedb/frames.md) for Polars-style frames |
| Use it from Node, Bun or Deno | [JavaScript](chloedb/javascript.md) |
| Use it from Java, or through JDBC | [Java](chloedb/java.md) |
| Give Claude Code long-term memory | [Install ChloeCtx](chloectx/setup.md) |

## Write queries

- [Query language](chloedb/query-language.md): the SQL reference. Full text with `MATCH`, scores and rank fusion, typos and synonyms, aggregates, joins, window functions, `MAX k PER`, stored functions, `llm()`, tables, writes and `EXPLAIN`.
- [Graphs](chloedb/graphs.md): edges between rows, reachability, shortest paths and personalised PageRank.
- [Next items](chloedb/next-items.md): "what comes after this" counted in a table, for recommendations.
- [Search on the device](chloedb/slices.md): a slice of a table near the user, searched on a phone, with the server as fallback.

## Bring data in

- [SQLite in and out](chloedb/sqlite.md)
- [Parquet and S3](chloedb/parquet.md)
- [Following Postgres](chloedb/postgres.md) by logical replication, a moment behind
- [The OpenSearch API](chloedb/opensearch.md), for apps written for OpenSearch or Elasticsearch
- [OpenTelemetry logs](chloedb/server.md#opentelemetry-logs) over OTLP
- [The crawler](chloedb/crawler.md): a website into a table

## Run the server and a cluster

[The server guide](chloedb/server.md) is one long reference. Its main parts:

| topic | section |
|---|---|
| Start a server, and every flag | [Run it](chloedb/server.md#run-it) |
| Schemas, loading rows, embeddings and media | [Schemas](chloedb/server.md#schemas), [Loading rows](chloedb/server.md#loading-rows), [Embedded vector fields](chloedb/server.md#embedded-vector-fields), [Media fields](chloedb/server.md#media-fields) |
| The HTTP API, gRPC and security | [HTTP API](chloedb/server.md#http-api), [Security](chloedb/server.md#security), [gRPC](chloedb/server.md#grpc) |
| Backups and object storage | [Backups](chloedb/server.md#backups), [Cold segments](chloedb/server.md#cold-segments) |
| Shards behind a coordinator | [Sharding](chloedb/server.md#sharding-one-coordinator-many-shards) |
| Replicas and failover | [Replication](chloedb/server.md#replication-a-primary-and-its-replicas) |
| Kubernetes, the cloud, and following another cluster | [On Kubernetes](chloedb/server.md#on-kubernetes) |
| Limits, health, metrics, logs and alerts | [Deadlines, admission and metrics](chloedb/server.md#deadlines-admission-and-metrics) |
| The command center, the agent and MCP | [The UI](chloedb/server.md#the-ui), [The on-call agent](chloedb/server.md#the-on-call-agent), [Agents over MCP](chloedb/server.md#agents-over-mcp) |

[Compatibility](compatibility.md) says what a version promises about the file format, the cluster protocol and the old names.

## Apps on ChloeDB

- [chloedb-sheets](chloedb/sheets.md): spreadsheets whose workbooks are ChloeDB rows.
- [chloedb-netops](chloedb/netops.md): network operations on ChloeDB, with a simulated carrier network to try it on.
- [chloedb-crawl](chloedb/crawler.md): a crawler that turns a site into a table and grows the cluster; [what the crawler does on your site](chloedb/crawler-bot.md).
- [chloedb-apps](chloedb/apps.md) (a prototype): apps built from a description and changed by asking.
- [ChloeCtx beside a cluster](chloectx/app.md): memory spaces for any agent, and past incidents next to each alert.

## ChloeCtx: memory for Claude Code

- [Install it](chloectx/setup.md), with an agent or by hand, and troubleshooting.
- [How it works](chloectx/index.md): what it stores, when it brings history back, and how it searches.
- [The inspector](chloectx/inspector.md): browse what it stored, per project and per session. [Try it in your browser](https://chloetech-ai.github.io/chloedb/demo/inspector.html) on a recorded session.
- [Results](chloectx/results.md), and [Why ChloeCtx](https://chloetech-ai.github.io/chloedb/why/), a two-page summary with a cost calculator.
- [The ChloeCtx showcase](engram-showcase.md): a week-long benchmark, in short.

## How it works inside

- [ChloeDB's design](chloedb/index.md): the file format, transactions, every index and the planner. The crate's [DESIGN.md](../chloedb/DESIGN.md) goes deeper.
- [Posting lists](posting-lists.md): the study of compressed posting lists that produced ChloeDB's Loom and Pyramid formats.
- [ChloeCtx's design](../chloectx/DESIGN.md).

## Results and how to reproduce them

- [ChloeDB results](chloedb/results.md): speed, ranking quality, vectors, the planner and storage.
- [The benchmarks against other engines](https://chloetech-ai.github.io/chloedb/site/benchmarks.html), with [how they were run](https://chloetech-ai.github.io/chloedb/site/bench-method.html).
- [ChloeCtx results](chloectx/results.md).
- [Reproducing the results](reproduce.md): the machines, and the commands for each experiment.
- The long-form reports, with every chart and table: [ChloeDB](https://chloetech-ai.github.io/chloedb/reports/strata.html), [graphs](https://chloetech-ai.github.io/chloedb/reports/graph.html), [posting lists](https://chloetech-ai.github.io/chloedb/reports/posting-lists.html) and [their new designs](https://chloetech-ai.github.io/chloedb/reports/posting-list-designs.html), [ChloeCtx](https://chloetech-ai.github.io/chloedb/reports/engram.html) and [what a memory record should be](https://chloetech-ai.github.io/chloedb/reports/structure.html).

Every number in these pages comes from experiments committed in this repository (`*/results/`). The timings were measured on a MacBook Air with an Apple M2 (8 cores, 24 GB) unless a page says otherwise; the comparisons with other engines ran on DigitalOcean, and each report names its machines.

## Old names

ChloeDB was called Strata, and ChloeCtx was called Engram, until November 2026. The old names of programs, settings, headers and packages still work for a release. [Compatibility](compatibility.md#old-names) lists them, and [the ChloeCtx install guide](chloectx/setup.md#chloectx-was-called-engram) says what changes for an existing install.
