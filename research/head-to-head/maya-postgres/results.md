# Maya: a recipe search box, ChloeDB against Postgres

Maya, a junior developer in the persona study, adopts ChloeDB when her lead sees it beside Postgres full-text search, with the same queries, and the typo search never returns fewer rows than `ILIKE`. This page is that comparison on a made-up cookbook shaped like hers: 4,000 recipes whose names are an adjective, an ingredient and a dish, with the spellings real cookbooks mix (Lasagna and Lasagne, Chili and Chilli, Jalapeño and Jalapeno, Mac and Cheese and Mac & Cheese). 40 texts are typed into the search box: 16 spelled right, 14 with a typo, 10 with the punctuation a visitor types. Each text has the rows it means (a search for *lasagne* means every lasagna, however the name spells it), and each system's answer is scored against them.

Measured on 2026-11-20 on Intel(R) Xeon(R) Processor @ 2.10GHz, 4 cores, 15 GB, Linux 6.18.44-fc-v726; chloedb-server 0.3.9 (built from 2f21f4dd0c); PostgreSQL 16.15 (Ubuntu 16.15-0ubuntu0.24.04.1). Run it again with `python3 run.py && python3 report.py` (needs Postgres with pg_trgm, `pip install psycopg[binary]`, and a chloedb-server build). The raw numbers are in [raw/results.json](raw/results.json).

## What it says

- **ChloeDB's documented typo search found what was meant.** It returned every row a text meant for 39 of the 40 texts, and its first page was right for every text (1.00). It missed some rows of `kebab` (*Kabob* is two edits from *kebab*, more than a five-letter word is allowed). No text returned fewer rows than `ILIKE`, *lasagne* found every Lasagna and Lasagne, and no text was an error.
- **Its one weakness here is a real word widened to its neighbour.** `tomato soup` also kept *potato*; `chicken curry` also kept *chickpea*; `tomatoe soup` also kept *potato*; `chickpae curry` also kept *chickpea*, *chicken*; `"tomato soup` also kept *potato*. The neighbours rank after the rows that have the words as typed, so the first page stays right while there are ten of those; below them the page fills with the neighbours. Its precision over everything it returned is 0.93. This is MAYA-310, and it holds for words spelled right, not only for typos.
- **Postgres needs both of its indexes to come close, and still misses typos.** Full text alone returned 0.14 of what the typed-with-a-typo texts meant. Full text OR pg_trgm, two GIN indexes and a ranking written by hand, found 0.86, missing `spagetti`, `yoghrt`, and trigram similarity keeps any name that shares most of the text's letters: for `tomato soup`, `chicken curry`, `sweet potato pie`, `soup (easy`, `"tomato soup`, `chicken-curry`, `grandma's pie`, `one-pot chili`, fewer than half of the rows it returned were meant. Full text alone does not join spellings: *lasagne* finds only the names spelled Lasagne. With pg_trgm too, texts spelled right still missed rows spelled the other way: `yogurt`, `yoghurt`, `creme brulee`, `kebab`.
- **Speed is no reason to choose; memory favours Postgres at this size.** Postgres's full text alone answers a page in 0.21 ms at the median, and its form that comes close on typos (full text OR pg_trgm) in 2.13 ms (p95 4.09); ChloeDB's fuzzy search takes 1.46 ms (p95 1.73) through HTTP, 0.49 ms of it in the server. All are far below what a visitor notices. ChloeDB's server used 122 MB against Postgres's 42 MB. An app that opens the file itself (`chloedb.open`, as Maya's Flask app does) has no HTTP in the way; that is not measured here.
- **ChloeDB takes fewer steps.** 5 against 8: no extension, no index to declare, and the query is the one the docs give.

## What each one returned

Recall is the share of the rows a text means that came back; precision, the share of what came back that it means; first page, the share of the first ten that are right (of as many as there are, when fewer than ten). "Fewer than ILIKE" counts the texts spelled right where a system returned fewer rows than `name ILIKE '%text%'`, the line Maya's search must not cross.

| system | recall, spelled right | recall, typo | recall, punctuation | precision | first page | errors | fewer than ILIKE |
|---|---|---|---|---|---|---|---|
| Postgres `ILIKE '%text%'` | 0.66 | 0.07 | 0.17 | 1.00 | 0.51 | 0 | 0 |
| Postgres full text (`websearch_to_tsquery`) | 0.69 | 0.14 | 0.73 | 1.00 | 0.66 | 0 | 0 |
| Postgres pg_trgm (`<%`, `word_similarity`) | 0.88 | 0.86 | 0.85 | 0.81 | 0.90 | 0 | 0 |
| Postgres full text OR pg_trgm | 0.88 | 0.86 | 0.93 | 0.81 | 0.94 | 0 | 0 |
| ChloeDB `MATCH ... syntax => 'plain'` | 0.78 | 0.14 | 0.93 | 1.00 | 0.68 | 0 | 0 |
| ChloeDB `MATCH ... syntax => 'plain', fuzzy => 'auto'` | 0.97 | 1.00 | 1.00 | 0.93 | 1.00 | 0 | 0 |

## Speed, memory, disk and setup

A first page is the ranked query with `LIMIT 10`, each text 30 times after three warm-up runs. Postgres is timed through psycopg on loopback; ChloeDB through its HTTP JSON API, so its client time includes HTTP and JSON, and the server's own time (`took_ms`) is given beside it. Memory is the proportional set size of each server's processes after the run (Postgres's shared buffers counted once). Disk is the table and its indexes for Postgres (`pg_total_relation_size`; its whole data folder, catalogs and WAL included, beside it) and the database's files for ChloeDB.

| system | first page p50 | p95 | server's own p50 | p95 |
|---|---|---|---|---|
| Postgres `ILIKE '%text%'` | 0.13 ms | 1.24 ms | – | – |
| Postgres full text (`websearch_to_tsquery`) | 0.21 ms | 1.18 ms | – | – |
| Postgres pg_trgm (`<%`, `word_similarity`) | 0.90 ms | 1.54 ms | – | – |
| Postgres full text OR pg_trgm | 2.13 ms | 4.09 ms | – | – |
| ChloeDB `MATCH ... syntax => 'plain'` | 1.33 ms | 1.63 ms | 0.37 ms | 0.52 ms |
| ChloeDB `MATCH ... syntax => 'plain', fuzzy => 'auto'` | 1.46 ms | 1.73 ms | 0.49 ms | 0.63 ms |

| | Postgres | ChloeDB |
|---|---|---|
| load 4,000 rows and build the indexes | 0.06 s | 0.04 s |
| memory after the run (PSS) | 42 MB | 122 MB |
| disk | 0.67 MB (data folder 41 MB) | 3.75 MB |
| setup steps | 8 | 5 |

Postgres's steps: install Postgres and its contrib package (pg_trgm); initdb, start the server, create a database; CREATE EXTENSION pg_trgm; CREATE TABLE recipes (id text PRIMARY KEY, name text, minutes int, diet text); CREATE INDEX ... USING gin (to_tsvector('english', name)); CREATE INDEX ... USING gin (name gin_trgm_ops); load the rows (COPY); write the query: websearch_to_tsquery for words, <% and word_similarity for typos, both ORed and ranked by hand.

ChloeDB's steps: install chloedb-server (or pip install chloedb for the embedded file); start it on a folder (or chloedb.open('recipes.chloedb')); CREATE TABLE recipes (id KEYWORD PRIMARY KEY, name TEXT, minutes INT, diet KEYWORD); write the rows (POST /v1/write, or db.write); write the query: MATCH(name, :q, syntax => 'plain', fuzzy => 'auto') ORDER BY bm25(name, :q) DESC.

## Each text

Rows returned, and recall / precision, for each text. *right* is how many rows the text means.

| text | kind | right | ILIKE | PG full text | PG full text OR trgm | ChloeDB exact | ChloeDB fuzzy |
|---|---|---|---|---|---|---|---|
| `lasagna` | spelled | 194 | 97 (0.50 / 1.00) | 97 (0.50 / 1.00) | 194 (1.00 / 1.00) | 97 (0.50 / 1.00) | 194 (1.00 / 1.00) |
| `lasagne` | spelled | 194 | 97 (0.50 / 1.00) | 97 (0.50 / 1.00) | 194 (1.00 / 1.00) | 97 (0.50 / 1.00) | 194 (1.00 / 1.00) |
| `chilli` | spelled | 187 | 94 (0.50 / 1.00) | 94 (0.50 / 1.00) | 187 (1.00 / 1.00) | 94 (0.50 / 1.00) | 187 (1.00 / 1.00) |
| `chili` | spelled | 187 | 93 (0.50 / 1.00) | 93 (0.50 / 1.00) | 187 (1.00 / 1.00) | 93 (0.50 / 1.00) | 187 (1.00 / 1.00) |
| `yogurt` | spelled | 190 | 98 (0.52 / 1.00) | 98 (0.52 / 1.00) | 98 (0.52 / 1.00) | 98 (0.52 / 1.00) | 190 (1.00 / 1.00) |
| `yoghurt` | spelled | 190 | 92 (0.48 / 1.00) | 92 (0.48 / 1.00) | 92 (0.48 / 1.00) | 92 (0.48 / 1.00) | 190 (1.00 / 1.00) |
| `tomato soup` | spelled | 8 | 8 (1.00 / 1.00) | 8 (1.00 / 1.00) | 61 (1.00 / 0.13) | 8 (1.00 / 1.00) | 23 (1.00 / 0.35) |
| `chicken curry` | spelled | 7 | 7 (1.00 / 1.00) | 7 (1.00 / 1.00) | 161 (1.00 / 0.04) | 7 (1.00 / 1.00) | 13 (1.00 / 0.54) |
| `jalapeño` | spelled | 159 | 79 (0.50 / 1.00) | 79 (0.50 / 1.00) | 159 (1.00 / 1.00) | 159 (1.00 / 1.00) | 159 (1.00 / 1.00) |
| `jalapeno` | spelled | 159 | 80 (0.50 / 1.00) | 80 (0.50 / 1.00) | 159 (1.00 / 1.00) | 159 (1.00 / 1.00) | 159 (1.00 / 1.00) |
| `creme brulee` | spelled | 210 | 102 (0.49 / 1.00) | 102 (0.49 / 1.00) | 102 (0.49 / 1.00) | 210 (1.00 / 1.00) | 210 (1.00 / 1.00) |
| `kebab` | spelled | 168 | 92 (0.55 / 1.00) | 92 (0.55 / 1.00) | 92 (0.55 / 1.00) | 92 (0.55 / 1.00) | 92 (0.55 / 1.00) |
| `mushroom risotto` | spelled | 8 | 8 (1.00 / 1.00) | 8 (1.00 / 1.00) | 8 (1.00 / 1.00) | 8 (1.00 / 1.00) | 8 (1.00 / 1.00) |
| `sweet potato pie` | spelled | 4 | 4 (1.00 / 1.00) | 4 (1.00 / 1.00) | 196 (1.00 / 0.02) | 4 (1.00 / 1.00) | 4 (1.00 / 1.00) |
| `stir fry` | spelled | 196 | 98 (0.50 / 1.00) | 196 (1.00 / 1.00) | 196 (1.00 / 1.00) | 196 (1.00 / 1.00) | 196 (1.00 / 1.00) |
| `pancakes` | spelled | 215 | 215 (1.00 / 1.00) | 215 (1.00 / 1.00) | 215 (1.00 / 1.00) | 215 (1.00 / 1.00) | 215 (1.00 / 1.00) |
| `lasagnia` | typo | 194 | 0 (0.00 / –) | 0 (0.00 / –) | 194 (1.00 / 1.00) | 0 (0.00 / –) | 194 (1.00 / 1.00) |
| `tomatoe soup` | typo | 8 | 0 (0.00 / –) | 8 (1.00 / 1.00) | 8 (1.00 / 1.00) | 8 (1.00 / 1.00) | 23 (1.00 / 0.35) |
| `chiken curry` | typo | 7 | 0 (0.00 / –) | 0 (0.00 / –) | 7 (1.00 / 1.00) | 0 (0.00 / –) | 7 (1.00 / 1.00) |
| `brocoli` | typo | 198 | 0 (0.00 / –) | 0 (0.00 / –) | 198 (1.00 / 1.00) | 0 (0.00 / –) | 198 (1.00 / 1.00) |
| `spagetti` | typo | 208 | 0 (0.00 / –) | 0 (0.00 / –) | 0 (0.00 / –) | 0 (0.00 / –) | 208 (1.00 / 1.00) |
| `yoghrt` | typo | 190 | 0 (0.00 / –) | 0 (0.00 / –) | 0 (0.00 / –) | 0 (0.00 / –) | 190 (1.00 / 1.00) |
| `risoto` | typo | 212 | 0 (0.00 / –) | 0 (0.00 / –) | 212 (1.00 / 1.00) | 0 (0.00 / –) | 212 (1.00 / 1.00) |
| `guacamol` | typo | 211 | 211 (1.00 / 1.00) | 211 (1.00 / 1.00) | 211 (1.00 / 1.00) | 211 (1.00 / 1.00) | 211 (1.00 / 1.00) |
| `mushrom risotto` | typo | 8 | 0 (0.00 / –) | 0 (0.00 / –) | 8 (1.00 / 1.00) | 0 (0.00 / –) | 8 (1.00 / 1.00) |
| `pumkin pie` | typo | 13 | 0 (0.00 / –) | 0 (0.00 / –) | 13 (1.00 / 1.00) | 0 (0.00 / –) | 13 (1.00 / 1.00) |
| `chickpae curry` | typo | 6 | 0 (0.00 / –) | 0 (0.00 / –) | 6 (1.00 / 1.00) | 0 (0.00 / –) | 13 (1.00 / 0.46) |
| `salmn` | typo | 166 | 0 (0.00 / –) | 0 (0.00 / –) | 166 (1.00 / 1.00) | 0 (0.00 / –) | 166 (1.00 / 1.00) |
| `cauliflour` | typo | 196 | 0 (0.00 / –) | 0 (0.00 / –) | 196 (1.00 / 1.00) | 0 (0.00 / –) | 196 (1.00 / 1.00) |
| `eggplnt stew` | typo | 8 | 0 (0.00 / –) | 0 (0.00 / –) | 8 (1.00 / 1.00) | 0 (0.00 / –) | 8 (1.00 / 1.00) |
| `soup (easy` | punct | 4 | 0 (0.00 / –) | 4 (1.00 / 1.00) | 49 (1.00 / 0.08) | 4 (1.00 / 1.00) | 4 (1.00 / 1.00) |
| `"tomato soup` | punct | 8 | 0 (0.00 / –) | 8 (1.00 / 1.00) | 61 (1.00 / 0.13) | 8 (1.00 / 1.00) | 23 (1.00 / 0.35) |
| `mac & cheese` | punct | 202 | 93 (0.46 / 1.00) | 202 (1.00 / 1.00) | 202 (1.00 / 1.00) | 202 (1.00 / 1.00) | 202 (1.00 / 1.00) |
| `chicken-curry` | punct | 7 | 0 (0.00 / –) | 0 (0.00 / –) | 161 (1.00 / 0.04) | 7 (1.00 / 1.00) | 7 (1.00 / 1.00) |
| `soup)` | punct | 207 | 0 (0.00 / –) | 207 (1.00 / 1.00) | 207 (1.00 / 1.00) | 207 (1.00 / 1.00) | 207 (1.00 / 1.00) |
| `grandma's pie` | punct | 9 | 2 (0.22 / 1.00) | 9 (1.00 / 1.00) | 204 (1.00 / 0.04) | 9 (1.00 / 1.00) | 9 (1.00 / 1.00) |
| `stir-fry` | punct | 196 | 98 (0.50 / 1.00) | 98 (0.50 / 1.00) | 196 (1.00 / 1.00) | 196 (1.00 / 1.00) | 196 (1.00 / 1.00) |
| `crème brûlée` | punct | 210 | 108 (0.51 / 1.00) | 108 (0.51 / 1.00) | 108 (0.51 / 1.00) | 210 (1.00 / 1.00) | 210 (1.00 / 1.00) |
| `one-pot chili` | punct | 9 | 0 (0.00 / –) | 3 (0.33 / 1.00) | 41 (0.78 / 0.17) | 3 (0.33 / 1.00) | 9 (1.00 / 1.00) |
| `salad!` | punct | 207 | 0 (0.00 / –) | 207 (1.00 / 1.00) | 207 (1.00 / 1.00) | 207 (1.00 / 1.00) | 207 (1.00 / 1.00) |
