# chloedb-arxiv: arXiv on ChloeDB

> **A demo.** chloedb-arxiv shows what ChloeDB can do; it is not part of ChloeDB's releases or their promises. It is tested on a train of its own (the `Demos` workflow, `.github/workflows/demos.yml`), outside CI's required check, and the command center marks it as a demo ([compatibility](../compatibility.md#demos)).

`chloedb-arxiv` is an explorer of every paper on arXiv: 3.25 million papers up to this week, the people who wrote them, the citations between them, a thousand topics found from what they say, techniques and domains, and a feed per reader that reads like a research newsletter. It is a program of its own, in Rust, that talks to chloedb-server over HTTP only, like [the crawler](crawler.md) and [the spreadsheets](sheets.md), and the command center shows it as an app.

Everything it does is a query to ChloeDB, and each page shows the queries it ran and their times (**SQL** at the bottom right):

- **Search by words and meaning at once.** Titles and abstracts are ranked by BM25 and by the meaning of their text (a static embedding model on the server, no API call), fused by rank (`rrf`), with typos corrected from the titles' own words and the query's synonyms (*RAG*, *retrieval augmented generation*). A query is read for what it says: a year (*since 2023*), a person (*by Yann LeCun*), a category (*cs.IR*), a kind (*surveys of*), an id. Facets (year, category, technique, domain, kind, people) come from the same query. A question is answered in a paragraph whose claims cite the papers they come from (`llm()` on the server).
- **What to read next is ChloeDB's recommender.** Every paper a work cites is a reading event, 5.2 million of them, counted by `CREATE RECOMMENDER reading ON ax_events(...)`: what is read together, what is read after. A paper's **Read next** fuses those counts with PageRank from it over the citation graph (`ppr(ax_cites, :id)`) and the nearest papers by meaning. **For you** does the same from the papers a reader opens and keeps here, which are events of the recommender as soon as they are written.
- **People, topics, techniques, domains, categories and venues each have a page**: papers a year, who leads, what rose, the people nearest a person in the coauthor graph (`ppr(ax_coauthors, :who)`), a topic summed up from its papers with each claim cited.
- **Trends and the atlas.** Any words, compared as a share of each year's papers. The atlas places the thousand topics on one map, near topics near, colored by arXiv area, sized by papers, a ring on those that grew most last year.
- **Feeds you shape yourself.** A feed says what you want in your words: topics, papers like some you name, categories, techniques, domains, people whose new papers always go in, words that keep a paper out, kinds (surveys, benchmarks, datasets, tools), how many papers, how many of one topic, for whom, and how much code, citations and known authors count. The papers an issue would hold are SQL and answer as the knobs move (no model is called). **Write issue** then reads each paper in full from its HTML page on arxiv.org (its sections, figures and tables) and explains it, so the issue can be read without opening the papers: an opening on what the week's papers share, a contents list, and for each paper *in brief*, the problem, the approach, the results with their numbers, the takeaway, the limits, the terms a newcomer needs, the one or two figures that explain it best (copied from the paper, with what to notice in each) and its main results table. A paper without an HTML version is explained from its abstract. Each issue has a page, a Markdown copy for a newsletter and an RSS feed; the shorter styles of [a weekly research newsletter](https://recsys.substack.com/) (a paragraph a paper, brief, bullets) remain.
- **It stays current.** `--live` reads the new and revised papers from arXiv's OAI-PMH interface every six hours, three seconds between pages, and a new paper goes to the topic whose centroid its text is nearest.

## Where the data comes from

- **Papers**: arXiv's metadata ([librarian-bots/arxiv-metadata-snapshot](https://huggingface.co/datasets/librarian-bots/arxiv-metadata-snapshot), CC0), and the weeks after the snapshot from [OAI-PMH](https://info.arxiv.org/help/oa/index.html).
- **Citations**: Crossref's references of arXiv papers' published versions ([cometadata/crossref-arxiv-citations](https://huggingface.co/datasets/cometadata/crossref-arxiv-citations)). A citing work that is an arXiv paper, by its DOI, makes an edge of the citation graph; every citing work is a reader of the recommender.
- **Topics**: `tools/topics.py` clusters every paper's embedding into 1,000 topics, puts each in one of 17 fields (arXiv's areas, by its papers' categories), names each from its terms and its most cited titles, and places them on the map.
- **Affiliations** for a feed's papers are read from each paper's HTML page on arxiv.org, once a paper and fifteen seconds apart, as arXiv's `robots.txt` asks. The app never reads arxiv.org's search or API pages.

## Run it

Build both programs, prepare the rows (a few GB of downloads), then start chloedb-arxiv with servers of its own:

```bash
(cd chloedb-server && cargo build --release)
cd chloedb-arxiv && cargo build --release
target/release/chloedb-arxiv prepare --snapshot ~/arxiv/snapshot --cites ~/arxiv/crossref/all.parquet --out ~/arxiv/rows
python3 tools/topics.py --rows ~/arxiv/rows --name
target/release/chloedb-arxiv prepare --snapshot ~/arxiv/snapshot --cites ~/arxiv/crossref/all.parquet --topics ~/arxiv/rows/topics.tsv --out ~/arxiv/rows
target/release/chloedb-arxiv serve --local ./arxiv-search --local-graph ./arxiv-graph --rows ~/arxiv/rows --server-bin ../chloedb-server/target/release/chloedb-server
```

Open `http://127.0.0.1:7700` and go to **Apps → arXiv**. `--only cs.,stat.ML` on `prepare` keeps a slice, which fits on a laptop.

### Two servers, split by job

The papers, people, topics and their words live on one chloedb-server (the **search server**, `--strata`); the reading events, the recommender, the citation and coauthor graphs, what readers keep, and the model calls live on another (the **graph server**, `--graph`). A recommender and a graph walk each need their rows in one database, so these tables are not sharded over a coordinator; splitting by job gives each server a machine of its own instead. Without `--graph`, one server holds everything. The graph server holds a row per paper and per person (`ax_nodes`) for the walks to rank.

```bash
chloedb-arxiv serve --strata http://search:7700 --graph http://graph:7700 --key "$CHLOEDB_KEY" --token "$ARXIV_TOKEN" --listen 0.0.0.0:7860 --live
chloedb-server --app arxiv=http://arxiv-app:7860 ...    # the command center lists it
```

`deploy/k8s.yaml` and `deploy/app.yaml` run it on Kubernetes: two nodes of 16 GB, a server on each, and the app beside the graph server.

`--answer-model` names the model of summaries and TL;DRs, and `--issue-model` the one that explains an issue's papers from their full text (both are called through the graph server's `llm()`, which holds the provider's key); `--llm-per-day` caps new calls a day, and everything written is kept, so it is written once. arXiv asks programs to read a page every 15 seconds, so the first issue of ten papers takes about ten minutes; a page and its figures, once read, are kept (`ax_notes`, `ax_figures`).

## The tables

| table | server | what |
|---|---|---|
| `ax_papers` | search | a paper: title, abstract, people, categories, dates, techniques, domains, kinds, counts, topic, and `emb` (its text embedded by the server) |
| `ax_authors` | search | a person: papers, citations, h-index, first and last paper, categories and techniques |
| `ax_topics`, `ax_concepts`, `ax_cats`, `ax_venues` | search | topics, techniques and domains, categories, venues, with counts by year |
| `ax_complete` | search | what the search box completes: phrases of titles, people, techniques, categories |
| `ax_nodes` | graph | the graphs' nodes: every paper's id and every person's key |
| `ax_cites`, `ax_coauthors` | graph | edges: who cites whom, who writes with whom and how often |
| `ax_events` | graph | reading events: a citing work's references, and what readers open and save here |
| `reading_*` | graph | the recommender's counts (`CREATE RECOMMENDER reading`) |
| `ax_feeds`, `ax_issues`, `ax_notes`, `ax_answers`, `ax_library` | graph | feeds, their issues, the papers' pages as read and what the model wrote, readers' libraries |
| `ax_figures` | graph | the figures issues show, each image copied once from the paper's page |

## Tests

`tests/cluster.rs` starts two chloedb-servers through `serve --local --local-graph`, loads a small made-up arXiv into them (embedded by the `hash:512` model, so nothing is downloaded), and checks every page: search, a paper's citations and what to read next, a person, a topic, the atlas, events becoming For you, and a feed's preview.

```bash
cd chloedb-arxiv && cargo test --profile ci
```
