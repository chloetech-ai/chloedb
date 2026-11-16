# GitHub Explorer: GitHub's public history on ChloeDB

`chloedb-gh` is an app of a ChloeDB cluster built on [GH Archive](https://www.gharchive.org): every public event on GitHub since 12 February 2011, about ten billion of them, in a cluster of four shards on machines with 8 GB of memory each. It finds repos by what they do, says why a repo's stars moved, what people use instead, who keeps a repo going, what is rising and what to star next. It is a client of chloedb-server over HTTP, as the crawler and the spreadsheet app are, and the command center shows it as the app `gh` (`chloedb-server --app gh=URL`). Every request needs the token the command center's app proxy sends: nothing of it is public.

## The pages

| page | what it shows | from |
|---|---|---|
| Search | repos by what they do: the words of their name, description and topics, and the meaning of the query against a vector of each repo, ranked by both and by stars | `gh_repos` (`MATCH`, `bm25`, `similarity`) |
| A repo | its stars month by month, the days and months they jumped (five and four times the usual) with the releases of those months, and its posts on Hacker News, its vital signs (work this year against last, merged pull requests, how many people do half the work, days since the last), who keeps it going, the repos that do the same job (by meaning and by the stargazers they share), what its stargazers also starred, and the names it had | `gh_events`, `gh_stars`, `gh_repo_months`, `gh_repos`, `gh_repo_names` |
| A person | what they did, year by year, where they work and what they starred lately (public activity only) | `gh_events`, `gh_stars`, `gh_actors` |
| Rising | the repos whose stars over the last days most outran the four weeks before, and why | `gh_stars_daily` |
| Trends | each language's and topic's share of the stars, year by year; growing and shrinking | `gh_lang_months`, `gh_topic_months` |
| Ask | a question in a sentence, turned into one statement by patterns, no model ("fastest growing rust repos this year", "who works on rust-lang/rust") | all of them |
| For you | repos to star next from a login's stars and work: what the people who starred the same repos starred | `gh_stars` |
| Weekly | the week's rising repos, new ones and releases of the big ones | `gh_stars_daily`, `gh_events` |
| How it works | the tables, their rows and each shard's share, and what each page asks | every table |

Each answer carries the statements it ran and their times, which the page shows under SQL.

## The data

GH Archive keeps each hour of GitHub's public event stream as a gzipped JSON file. Its formats changed over the years, and the load reads them all:

- From February 2011 to February 2012 events carry the repo's and the person's ids.
- From about March to September 2012 (the timeline format) they carry neither: a repo is `owner/name` and a person a login.
- From October 2012 to 2014 repos have ids and people only logins.
- From 2015 on, both have ids.

The years without ids get them from the years with: the logins and names of 2011-2012 and 2015-2016 (`--logins`, `--names`), the id that held a name first winning; a login or a name no later event explains gets a negative id of its own. From October 2025 GitHub's events no longer carry a repo's description, language or topics, so repos made since have none in the archive; `api-meta` fills them from GitHub's search API (below).

Only what the events say publicly is kept: ids, logins, repos, kinds of event and times. The old format's `actor_attributes` (names and email addresses) is never read.

## The tables

| table | rows | placed by |
|---|---|---|
| `gh_events` | every event: `t` (seconds), `kind`, `repo`, `actor`, `n` (commits of a push) | `shard_by repo`, sorted by repo in each segment |
| `gh_stars` | every star of a kept repo again, with the person as a `KEYWORD` (`actor_k`) too: an `IN` of a thousand people reads their own lists of rows, where the events in repo order would be read whole | `shard_by repo` |
| `gh_stars_daily` | stars and forks of each repo each day | `shard_by repo` |
| `gh_repos` | each repo with two stars or more, keyed by its id: latest name, what the events (and GitHub's search, for repos with 50 stars or more) last said of it, its stars counted from the events and GitHub's own count, and for repos with ten stars or more (the ones searched by meaning) a vector of what it does, embedded from `etext`, their name, description and topics: a shard's vectors and graph then fit an 8 GB machine | key `repo` |
| `gh_repo_names` | every name a repo had, with when it was first seen | `shard_by repo` |
| `gh_actors` | each person's latest public login | key `actor` |
| `gh_repo_months` | what happened in each repo each month: stars, forks, pushes, commits, pull requests opened and merged, issues, comments, releases, people | `shard_by repo` |
| `gh_lang_months`, `gh_topic_months` | the same summed by language and by topic, a part a shard (`part`), which queries add up | `shard_by part` |

Rows are placed on shards as chloedb-server places them (a key's slot is xxh3-64 of its typed encoding modulo 4096, slot `s` on shard `s * n / 4096` of a new cluster), so each shard's database is built offline, on its own machine, and the cluster's coordinator finds every row where its slot map says it is.

## The load

The load runs on the cluster's own machines (`chloedb-gh/deploy/work.yaml`: a pod a node, its folder on the node's SSD, `/work`):

1. **`chloedb-gh prepare --from 2015-01-01 --to DAY --part N/4 --out /work/out`** on each node, every fourth day: downloads each hour, keeps the events, dedupes a day at a time, and writes Parquet rows a table, a shard and a day (`out/TABLE/sS/DAY.parquet`: events, stars, names, actors, meta). A day done is written down (`out/done/DAY.json`) and skipped when it runs again. A worker holds one day in memory (about 1 GB for the busiest days of 2022), so 8 GB machines run three at a time (`--jobs 3`).
2. **The years before 2015**, in the order that gives each its ids: 2011-02-12 to 2012-02-29, then its actors and names to every node (`deploy/pull.py --shard all --tables actors,names`), 2012-10-01 to 2014-12-31 with `--logins` and `--names`, its names to every node, and 2012-03-01 to 2012-09-30.
3. **`chloedb-gh api-meta --out /work/out`**: what GitHub's search API says today of every repo with 50 stars or more (about 876,000), 100 repos a call: the repos made since October 2025 first. Search answers at most 1,000 repos a query, so the walk splits bands of stars, then days of creation, until each part has fewer. Without a token it makes at most 10 calls a minute from one address (a call every 7 s, about a day for the walk); with `GITHUB_TOKEN`, 30. Its rows are `meta` and `names` rows (`api-*.parquet`), which `reduce` reads after the days'.
4. **Each node takes its shard's rows** from the others (`deploy/pull.py --shard N --into /work/in`; each node serves `/work/out` with a list a shard).
5. **`chloedb-gh reduce --input /work/in --input /work/out --shard N`** sums a shard's rows into one row a key (`gh_repos`, `gh_repo_names`, `gh_actors`), months by repo, language and topic, and the star rows of `gh_stars`; only repos with `--vec-min-stars` (10) or more get the `etext` their vector is made of. Once every node has its shard's rows, each deletes its rows of the other shards, so that a shard's rows, its sums and its database fit the node's disk.
6. **`chloedb-server import`** loads each table into the shard's database, with the server of the cluster's image (`--schema chloedb-gh/deploy/schemas/TABLE.json`), after `reduce` and not beside it: the two together go past an 8 GB machine. A table's files go in as one folder (the node's own and the ones it pulled, hard-linked together): an import loads an empty table in bulk from its first folder only, and a second folder goes through the write path, many times slower.
7. **Each shard's slot map** goes beside its database before its server first starts, as `gh.chloedb.slots`: `{"shard": N, "since": 1, "map": {"epoch": 1, "shards": [[[0, 1024]], [[1024, 2048]], [[2048, 3072]], [[3072, 4096]]]}}`, the map its rows were placed by. A shard that a coordinator gives its first map reads the key of every row of every table to check that it belongs there, and on 2.8 billion rows that runs an 8 GB machine out of memory; a shard that already holds the map takes the coordinator's next one without the check.

Then `deploy/k8s.yaml` starts a chloedb-server a node on its database (`/mnt/chloedb/gh/db`), the coordinator over them and the app (the shards without the embedding model, `--no-fetch-models`: with it, each start reads the 3.3 million repos that have no vector, to embed them, which takes minutes and more memory than the machine has; the coordinator embeds the words of a search itself, and a write to `gh_repos` brings its vector), and `deploy/backup.yaml` writes a checkpoint of each shard every night and copies the newest off the node to a DigitalOcean volume.

## Run it on one machine

A few days of the archive into one database:

```bash
cd chloedb-gh && cargo build --release
G=target/release/chloedb-gh
$G prepare --from 2025-09-15 --to 2025-09-21 --shards 1 --out gh-rows
$G reduce --input gh-rows --shard 0 --shards 1 --out gh-red
S=../chloedb-server/target/release/chloedb-server; SC=deploy/schemas
$S fetch-model potion-retrieval-32M      # the vector of what each repo does
$S import --db gh.chloedb --schema $SC/gh_events.json --table gh_events gh-rows/events/s0
$S import --db gh.chloedb --schema $SC/gh_stars_daily.json --table gh_stars_daily gh-rows/stars/s0
for t in repos stars repo_names actors repo_months lang_months topic_months; do
  src=gh-red/$t/s0; [ -e $src.parquet ] && src=$src.parquet
  $S import --db gh.chloedb --schema $SC/gh_$t.json --table gh_$t $src
done
$S --db gh.chloedb --listen 127.0.0.1:7700 --max-groups 2000000 &
$G serve --strata http://127.0.0.1:7700 --listen 127.0.0.1:7880
```

`serve` without `--token` answers anyone on the machine; `CHLOEDB_GH_UI_DIR=chloedb-gh/ui` reads the view from disk without a rebuild.

## Against GitHub's search

`chloedb-gh/bench/` holds 99 searches written as a person looking for a kind of project would type them ("terminal file manager", "self-hosted photo and video backup like google photos"), each with the repos that answer it, written before either engine's answers were seen. `search_vs_github.py` asks GitHub's repository search (its default best-match order, within its rate limit) and the explorer's `/v1/search` the same words, compares repos by their GitHub ids (a repo renamed since is the same repo), and scores the first ten: whether an answer is first, whether one is in the ten, and MRR@10. Six searches are close to ones the ranking was compared on while it was written; the report gives the scores with and without them.
