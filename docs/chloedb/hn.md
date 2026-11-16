# chloedb-hn: Hacker News on ChloeDB

> **A demo.** chloedb-hn shows what ChloeDB can do; it is not part of ChloeDB's releases or their promises. It is tested on a train of its own (the `Demos` workflow, `.github/workflows/demos.yml`), outside CI's required check, and the command center marks it as a demo ([compatibility](../compatibility.md#demos)).

`chloedb-hn` keeps every public Hacker News item in ChloeDB, about 49 million stories, comments, jobs and polls from October 2006 on, and searches them the way people look for things on HN: a story they half remember, what HN said about a tool, who knows a subject, when something was talked about, or a question asked in a sentence.

- **Questions in a sentence, answered by SQL, no model.** "which post has received the most comments", "who posted the most stories about rust", "how many Show HN posts about postgres in 2023", "when was HN most into bitcoin", "what was the first story about docker". A small grammar ([`src/nl.rs`](../../chloedb-hn/src/nl.rs)) reads the question into what it asks (the top item by a measure, a count, people, sites or years), the topic and the filters, and the answer above the results is one query, shown with it. Anything the grammar does not read is searched as words.
- **Autocomplete of questions, phrases and people.** As a search is typed the box offers whole questions the grammar reads (`which post` → "which post has received the most comments"), the phrases HN's titles use most (`kubernetes o` → "kubernetes operator"), people and sites. The phrases are a table, `hn_complete`, keyed by kind and text, so completing is a prefix of its sorted key: one run of the dictionary, read from a few blocks (`PREFIX(id, 'p:kubernetes o')`).
- **Instant, a keystroke at a time.** Each keystroke shows the stories whose titles hold the words typed so far, the last one as a word or the start of one (`rust asy` is `MATCH(title, 'rust (asy OR asy*)')`), by points while that word is being typed and by BM25 times the votes once a space follows it: one statement, no count, no meaning. Enter runs the full search, which reads the last word whole. The *Instant* switch beside the box turns it off; a question being typed still gets its completions.
- **Words and meaning together.** Titles, texts and the words of each link are ranked by BM25 with typos, stems and synonyms, fused with a static embedding model's similarity on the server, and weighed by points (`pow(points + 1, 0.35)`) and a gentle decay for age. Every word must be there for searches of up to three words, and most of them for longer ones, so the counts and facets match the results.
- **Exact text, punctuation and all.** `` `C++` ``, `` `std::move` ``, `` `k8s.io/api` `` in backticks, or *Match by → exact text*, are matched where their characters are, by `ILIKE` answered from each segment's FM-index (`OPTIONS(substring = true)`), which tokenized search cannot do.
- **Highlights the way the index reads.** A server of 0.3.6 or later marks each result with `POST /v1/highlight`: stems, synonyms, typos and code as the field's analyzer reads them, cut to the stretch with the most matches.
- **Filters in the words.** `by:pg`, `site:github.com`, `points>500`, `comments>100`, `2019..2021`, `before 2015`, `-crypto`, `show hn`, `ask hn`, `"exact phrase"`; each is shown as a removable chip of what was understood.
- **Threads, people and the graph.** A story opens as its whole thread; a comment opens in place with the path to it. People pages show what someone writes about, when, and who they talk to (the reply graph, `hn_replies`). *Who knows* ranks the people who comment most on a subject, weighed by the discussion they start. Similar stories by meaning, and the same link discussed again.
- **What HN thinks.** A question about opinions gathers the comments that speak to it, packed into a model's budget by the database (`PACK n TOKENS ... DEDUPE ... WITH RECEIPT`), and a model behind the server's `llm()` sums them up with every claim cited to a numbered comment. Answers are kept, so a question asked again costs nothing. It is the only part that calls a model.
- **A link across HN.** *Links* takes a page or a site and shows everywhere HN met it: each time it was submitted, the comments that cite it in other threads, the people who submitted, discussed or cited it, and the links discussed beside it, drawn as one graph (`GET /v1/link?u=`). A site alone (`sqlite.org`) lists its most discussed pages, the pages submitted again and again, and who posts it most.
- **The week on HN.** *Weekly* is a digest of a week's ten most voted stories (one per link, jobs left out): what each is, why it mattered, what the thread said, with the top comments quoted, and an opening over the whole week. The app reads each story's linked page once (public `http` and `https` pages only, with a timeout) and `llm()` writes an entry from the page, the post and its most replied comments; entries are kept in `hn_answers`, so each story is written once (`--digest-model`, at most `--digest-per-day` new entries a day).
- **Fast mode.** The switch beside the search box ranks by words alone (BM25, no meaning and no reranking) for every search until it is switched off.
- **Live.** With `--feed` the app follows the HN API: new items as they are posted, and the points and comments of the stories that change.

Like [the crawler](crawler.md) and [network operations](netops.md), it is a program of its own, in Rust, that talks to chloedb-server over HTTP only, and the command center shows it as an app (**Apps → Hacker News**).

## Run it

Build both programs, then prepare the archive and start the app with a server of its own:

```bash
(cd chloedb-server && cargo build --release)
cd chloedb-hn && cargo build --release
target/release/chloedb-hn prepare --parquet ./hn-parquet --out ./hn-rows --download
target/release/chloedb-hn complete --rows ./hn-rows
target/release/chloedb-hn serve --local ./hn-data --rows ./hn-rows --server-bin ../chloedb-server/target/release/chloedb-server
```

`prepare --download` fetches the archive's monthly Parquet files (about 12 GB, from [open-index/hacker-news](https://huggingface.co/datasets/open-index/hacker-news)) and writes the app's rows: stories, comments (each with its story's title, depth, place among its siblings and the discussion under it), people, the reply graph and the links between stories. `--from 2024-01` and `--to` take part of it; `--comments-since` keeps every story and only the newer comments. `complete` writes autocomplete's rows from them. `serve --local DIR` starts a chloedb-server with its database in `DIR`, makes the tables, loads the rows when they are empty, and lists the app as `hn`; open `http://127.0.0.1:7700` and go to **Apps → Hacker News**.

Against a server or a cluster that runs elsewhere, load into it and give the server the app:

```bash
chloedb-hn load --rows ./hn-rows --strata http://coordinator:7700 --key "$CHLOEDB_KEY" --comment-model none
chloedb-hn serve --strata http://coordinator:7700 --key "$CHLOEDB_KEY" --token "$HN_TOKEN" --listen 0.0.0.0:7850 --feed --comment-model none
chloedb-server ... --app hn=http://hn-host:7850     # with CHLOEDB_APP_TOKEN=$HN_TOKEN
```

`--comment-model none` keeps the comments without vectors: they take a third of the bytes, and are found by their words and their story's title. The people and sites questions group by author or site; a server whose `--max-groups` (65,536 by default) is below HN's 1.1 million authors answers them for a topic but not over the whole archive, so give it `--max-groups 2000000`.

## The questions it reads

| asked | answered by |
|---|---|
| which post has received the most comments / the most upvoted story about X / the top 5 Show HN posts in 2023 | the top stories by comments, points or date, with the question's filters (`ORDER BY comments DESC`) |
| the first / oldest / newest story about X | `ORDER BY created` |
| how many stories mention X / number of comments about X in 2020 | `COUNT(*)` with the same match as the results, and its years |
| who posted the most about X / who commented the most / who has the most karma | `GROUP BY author`; HN's archive carries no karma, so karma is the points a person's stories earned, and the answer says so |
| who knows the most about X | the people who comment most on it, weighed by the discussion they start |
| which sites are posted the most about X | `GROUP BY site` |
| when was HN most into X / which year did X peak | the years' counts, each as a share of that year's stories (HN grew; raw counts favour late years) |
| what does HN think about X | the cited summary above, the one answer a model writes |

Each answer shows its SQL (**see the SQL**). The grammar's templates are also autocomplete's questions, so what the box offers it can answer.

## Read the linked pages

```bash
chloedb-hn crawl --strata http://coordinator:7700 --key "$CHLOEDB_KEY" --min-points 3 --contact https://your.site
```

`crawl` reads each page the stories link to, once, into `hn_pages`, with [the crawler](crawler.md)'s fetcher and frontier: robots.txt read before a site's first page and obeyed, one request at a time to a site and a second between two (longer when its robots.txt or its answers ask), redirects checked hop by hop, public addresses only, HTML only, at most 5 MB a page. The links are the stories' canonical links (`curl`) of at least `--min-points` points, most voted first. Every link asked becomes a row saying what came of it (`ok`, `noindex`, `robots`, `http_4xx` with its code, `network`, `timeout`, `not_html`, ...), so the table also says which of HN's links still answer, by the year HN first linked them. A page whose `<meta name="robots">` says `noindex` keeps no text. A run skips the links already in the table, so a crawl that stops goes on where it was; `--again network,timeout` asks those that failed that way again. `--contact` goes in the user agent, for a site's owner to reach whoever runs it.

## Tables

| table | what |
|---|---|
| `hn_stories` | stories, Ask and Show HN, jobs and polls: title, text, link (its site and words), author, time, points, comments, kind, topics, a vector of the title and text |
| `hn_comments` | comments: text, author, time, parent and its author, story and its title, depth, place among siblings, replies, the discussion under it (and a vector, unless `--comment-model none`) |
| `hn_users` | people: stories, comments, points earned, first and last item, replies received, people talked to |
| `hn_replies` | the reply graph: who replied to whom, how often |
| `hn_links` | which discussion links to which: a story or one of its comments naming another's `item?id=` |
| `hn_pages` | the linked pages as `crawl` read them, keyed by canonical link: what came of asking, the HTTP code, the year HN first linked it, the most voted story and how many link it, the page's title, description, text (at most `--max-text` characters), words, language, date, author and image, and a vector of its title, description and text |
| `hn_complete` | autocomplete: phrases of titles (`p:`), sites (`s:`), topics (`t:`) and people (`u:`), each with how many stories hold it and their points |
| `hn_answers`, `hn_searches`, `hn_state` | kept answers, the searches people ran (their words and counts only) and the feed's place |

## The API

| route | what |
|---|---|
| `GET /v1/search?q=&only=all\|story\|comment&sort=best\|date\|points\|comments&mode=hybrid\|words\|meaning\|exact\|fast\|instant&page=` | results, what was understood, the answer when it is a question, facets, "did you mean", and every query run with its time |
| `GET /v1/suggest?q=` | completions: questions, phrases, people, sites, stories |
| `GET /v1/item/:id`, `/v1/item/:id/around` | a thread; similar stories and the same link again |
| `GET /v1/user/:id`, `/v1/experts?q=` | a person; who knows a subject |
| `GET /v1/trends?q=a,b&by=year\|month` | how often each was talked about |
| `GET /v1/ask?q=` | what HN thinks, cited |
| `GET /v1/front` | the front page as SQL: points over age, as HN ranks it |
| `GET /v1/facets?q=` | a search's facets on their own (the page asks for them once the results are shown) |
| `GET /v1/ask/comments?q=` | the comments an answer is made from, before the model is asked |
| `GET /v1/trending`, `/v1/spikes?year=` | what titles are suddenly about (the last hours against the year before); the months' strongest spikes |
| `GET /v1/link?u=` | a page or a site across HN: its submissions, citing comments, people, related links and the graph of them; with no `u`, the most discussed stories |
| `GET /v1/digest?week=YYYY-MM-DD` | a week's digest: the stories picked, the entries and opening already written, the week's totals; the last full week by default |
| `GET /v1/digest/write?week=&id=` | writes one entry (`id` a story of the week's picks, or `intro`) and keeps it |

## Measured against hn.algolia.com

`eval/compare.py` asks both the same queries, made from stories both hold: a story found again from three words of its title, the same with a typo, the story described in other words, and topics whose top ten a model grades blind to which engine found what. The results are in [`chloedb-hn/eval/results`](../../chloedb-hn/eval/results/), with the machine and the date they were measured on.

`eval/instant.py` types six searches a character at a time and asks both at each keystroke, as their pages do. On 2026-11-16, while `crawl` was writing pages into the same cluster, the app's instant search took 10 ms at the median on the server (22 ms at the 90th percentile) against Algolia's 13.5 ms (34 ms), and its round trip from the same cloud machine 48 ms against Algolia's 199 ms ([the numbers](../../chloedb-hn/eval/results/instant-2026-11-16.json)).

`eval/latency.py` times the same searches on both, each asked twice, as the server reports it and from end to end. On 2026-11-16 (a cloud machine against the deployment on DigitalOcean, four shards of 16 GB machines with the data on their own disks), the app's whole search took 100 ms at the median the first time a query was asked and 67 ms the second; hn.algolia.com's engine reported 17 ms, and its full round trip from the same machine was 243 ms against the app's 140 ms. `eval/plain.py` times the simplest statements on the same queries: BM25 of the titles alone took 15 ms at the median, as Algolia's 16.5, and the substring index with no ranking took 8 ms ([the numbers](../../chloedb-hn/eval/results/latency-2026-11-16.md)).
