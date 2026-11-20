# Head-to-heads

Each folder here puts ChloeDB beside what a persona of the persona study uses today, on data shaped like theirs and
made up for the purpose (nothing a persona brought is copied), and measures what that persona's lead would ask
for: whether the answers are right, how fast they come at the median and the 95th percentile, memory, disk, and
the steps to set it up. Each has `run.py` (writes its raw numbers to `raw/`), `report.py` (writes `results.md` from
them: no number on a page is typed by hand) and the page itself.

| folder | persona | against | the question |
|---|---|---|---|
| [maya-postgres](maya-postgres/results.md) | Maya, a junior developer with a recipe search box | Postgres full-text search and pg_trgm | does the typo search ever return fewer rows than `ILIKE`, and does a visitor's punctuation break it? |
| [sam-duckdb](sam-duckdb/results.md) | Sam, an analytics engineer | DuckDB and Postgres | the same rows as DuckDB for his 74-query battery, and the run time, at his size and at ten times it |
| [kenji-elasticsearch](kenji-elasticsearch/results.md) | Kenji, a backend engineer with a multi-tenant ticket search | Elasticsearch 9.5 | his service's requests, unchanged, through ChloeDB's OpenSearch API: the same hits and buckets, latency under load, memory |
| [nadia-two-tower](nadia-two-tower/results.md) | Nadia, a recommender-systems engineer | a two-tower model in PyTorch | next-movie recommendation on MovieLens-1M, leave-last-out, with a paired bootstrap |
| [priya-es-qdrant](priya-es-qdrant/results.md) | Priya, an ML engineer with RAG retrieval | Elasticsearch BM25 plus Qdrant, fused in Python | the same relevance on BEIR SciFact and FiQA with her own bge-small vectors, p95, and one system instead of two |
| [olu-game-day](olu-game-day/results.md) | Olu, an SRE who runs Elasticsearch on Kubernetes | a three-node Elasticsearch 9.5 cluster | a kill, a freeze and a rolling restart under a steady writer and reader: how long a client waits, what it is told, and whether every answered write is there |
| [grace-monday-report](grace-monday-report/results.md) | Grace, a support operations lead who builds a Monday report from a Zendesk export | DuckDB over the export, and SQL by hand | her four Monday numbers built by clicking in the command center, counted in actions, each checked against SQL, and the saved view opened on a phone |
| [marco-loki](marco-loki/results.md) | Marco, a platform engineer replacing Loki and an old Elasticsearch for logs | Loki 3.5 | an hour of his kind of lines at 1,000 a second: six on-call questions, each answer checked, and the disk a day costs, counted from the files |

Each runs on one machine, the systems in turn, and names the machine on its page. ChloeDB runs as `chloedb-server`
from this checkout (`CHLOEDB_SERVER_BIN` to choose another build) with its answer cache off, since each comparison
repeats a query to time it and the systems it is compared with keep no answers. The Python they need:

```sh
python3 -m venv .venv && . .venv/bin/activate
pip install -r research/head-to-head/requirements.txt
```

Postgres runs from its binaries (`PG_BINDIR`, else `/usr/lib/postgresql/<version>/bin`; as the `postgres` user when
run as root), Elasticsearch, Qdrant and Loki in Docker, Grace's clicks in Chromium by Playwright (node); Priya's needs `pip install fastembed` too. The round 5 briefs of the persona study point each persona at their page.
