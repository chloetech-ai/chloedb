# chloedb-experiments

Experiments on ChloeDB: what a ranking would have earned, measured on the
log of another, and checked against the truth.

It is the Experiments app of the command center (`serve`, below) and the
parts it stands on:

- **Propensities in the rank log.** A query with `explore` draws its list
  from a logging policy and logs each shown row's exact chance of being
  shown where it was. Off-policy estimators need that chance. The rank log's
  docs describe it:
  [Exploring](../docs/chloedb/query-language.md#exploring-a-log-that-evaluates-rankings-it-never-served).
- **A simulator.** It has simulated users in cohorts (age, gender, country,
  device, tenure) and a site, which is any table with queries and a grade
  per row. Five click models with trust bias decide what the users click,
  how long they stay and whether they were satisfied.
- **Estimators, and a check of each against the truth.** The estimators
  are naive, IPS, SNIPS, IIPS, SNIIPS, RIPS, SNRIPS, a position-based model,
  a simplified DBN, doubly robust IIPS and cascade doubly robust. Each one
  is replayed over many independent simulated logs. The check compares its
  bias and its intervals' coverage with the exact value of the ranking it
  estimates.

- **A/B tests** (`src/ab.rs`). Users are split between arms by a hash of
  who they are, each search is ranked by its user's arm, and the report has
  every metric with its interval, p value, q value over the metrics, power
  for the true effect and the smallest effect it could see, the same with
  CUPED, the always-valid p value (mSPRT) beside the one a peek gives,
  each user's value, each cohort, and a sample ratio check. Because the
  users are simulated, it also has the truth of each arm and of each
  cohort. A share of the users can explore instead, and its log values
  every arm off-policy, beside what the arms measured.

- **Studies of real sites** ([below](#real-sites)). AI agents play
  people written from a site's own rows, on the site's real search. Every
  arm is the site with some of its search changed (query understanding,
  retrieval, ranking, blending, the index), or another engine over the
  same rows. Each need is tried on every arm and compared need by need.

It is a client of chloedb-server over HTTP, as the crawler is. Bandits and
learning to rank come in later phases.

## The app

```bash
chloedb-experiments serve --local /tmp/exp --beir ~/beir --collections nfcorpus,scifact
```

That starts a chloedb-server in `/tmp/exp` with the app in its command
center (Apps → Experiments) and loads two BEIR collections as sites. Beside
a running server, `serve --server URL --token T` serves the app and the
server shows it with `--app experiments=URL` and
`CHLOEDB_APP_TOKEN_EXPERIMENTS=T`.

A site is any table with two more: its queries (`id`, `text`, `weight`, the
query's share of the traffic) and their judged rows (`query`, `doc`,
`grade` from 0 to 4). `POST /v1/sites` adds one (`name`, `table`,
`queries`, `judgments`, and the rankings to offer). Its API:

| route | what |
|---|---|
| `GET /v1/meta` | click models, metrics, cohorts, defaults and limits |
| `GET /v1/sites`, `GET /v1/sites/<name>`, `POST /v1/sites` | the sites |
| `POST /v1/preview` | what some rankings show for a site's busiest queries, with each row's grade |
| `GET /v1/experiments`, `POST /v1/experiments` | the experiments; a spec starts one in the background |
| `GET /v1/experiments/<id>`, `DELETE /v1/experiments/<id>` | one experiment, its progress and its report |
| `POST /v1/experiments/<id>/cut` | every metric by two attributes at once, for the last three runs |
| `POST /v1/experiments/<id>/check` | Check this: run it again `repeats` times (30 by default) on the same users and sum up how often each interval held the truth |
| `GET /v1/live`, `POST /v1/live` | live runs; a spec starts one (one at a time) |
| `GET /v1/live/<id>?after=&since=` | a live run now: every metric per arm, the points after `after`, the feed after `since`, the agents |
| `POST /v1/live/<id>/stop`, `POST /v1/live/<id>/rate` | stop it (it becomes an experiment), or change its searches a second |
| `POST /v1/live/<id>/try`, `/try/open`, `/try/done` | search the site as a visitor in the run, open a result, say it answered |
| `POST /v1/live/<id>/cohorts` | every metric by one or two attributes (`by`), over the users the run has seen so far |

Sites and experiments are rows of the server (`exp_sites`,
`exp_experiments`); a served run's rank log and events are tables of their
own (`exp_log_<id>`, `exp_events_<id>`), dropped with it. The page is
`ui/view.js`, compiled in; `CHLOEDB_EXPERIMENTS_UI_DIR=ui` reads it from
disk.

### Live runs

A live run (the builder's Live mode, or Watch one live on the home page) is
an experiment you watch as it happens. Simulated users arrive at the rate
you set (up to 400 searches a second, changed while it runs), each search a
logged query to ChloeDB under the user's arm and its clicks written back as
events. Each second the page shows every metric per arm with its interval,
its fixed-size p value beside the always-valid one (only the second may be
read at every second), the truth from the click model, and the judged
quality (nDCG) of what each arm showed, and every few seconds the picked
metric by cohort (one attribute, or a cross of two). The call is made on
the always-valid p value. By default (`"until": "decided"`) the run stops
once it is decided: after 1,000 searches, when every arm's primary metric
has an always-valid p value below 0.05 and every guardrail is read (moved
by its own always-valid p value, or held within its margin by its
interval); since the always-valid p value may be read at every second,
stopping there keeps its error rate. Until then the run says what it waits
on (`waiting`): often a guardrail that barely moves, whose interval needs
many searches to sit within its margin (abandonment's is 1%), or the query
time, measured rather than modelled and so noisy on a busy machine. `"until":
"time"` runs to the end of its minutes. When it stops, or you stop it, the run becomes an experiment
with the full report and an Agents tab.

AI agents are people of the population, each in the arm the split gives
it, sent to the site's search with a need of their own (a query of the
site, which the judges graded): they search, read the page, open results
and say whether one answered. Claude plays them through the Messages API
when the app has `ANTHROPIC_API_KEY` (`ANTHROPIC_BASE_URL` for another
endpoint): the model is picked in the builder, each call at low effort with
server-side fallbacks on a refusal for the models that have them, and the
agents stop when the run's budget (at most $20) is spent, the cost worked
out from the tokens each call reports. Without a key, scripted agents do
the same with no model. The agents take the arms in turn, so each arm has
as many (a person whom the split puts in that arm). Their finds are scored
apart from the simulated users': whether they found it, whether the judges
grade what they found relevant, searches and opens a task, and each task's
first page nDCG. When the run ends, one line per arm says why its agents
gave up: Claude reads their notes in one call when it played them, and
the counts and notes say it otherwise. You can search the site in the run
yourself, from the page, as one more visitor.

### Checking a result

One run's 95% interval misses the truth one time in twenty by design, so
one run cannot say whether its intervals are right. Check this (on a
report's Scorecard, or `POST /v1/experiments/<id>/check`) runs the
experiment again 30 times, drawn in the process: the same arms, the same
users and their tastes (so the same truth), each time split anew (the
spec's `salt`) and with new searches. It shows how often each metric's
interval held the truth for each arm, against the 28.5 in 30 expected,
and flags a metric that held it less often than a binomial allows (p
below 0.01); the strip of the primary metric's 30 intervals around the
true lift; and the calls the 30 runs made against the truth's. A run of
more than 2 million searches is checked at that size. Each experiment
takes a fresh seed when the spec gives none, so two runs of one spec are
two populations; give `seed` to run the same one again.

On nfcorpus, three arms (the body's BM25, the same ranking shuffled, and
the same ranking with its first row moved to the end), 20,000 users and
100,000 searches, a check of 30 repeats took six minutes in a cloud
container: success rate's interval held the truth in 25 of 30 for
Shuffled and 27 of 30 for First row demoted, and the other metrics in 24
to 29 of 30; the shuffled arm was called worse in all 30, the demoted one
(a true −5.4%) in 28.

### Test arms

To see whether an experiment finds what it should, the builder makes arm
B from the control: Shuffled (the control's ranking under
`((R) * 7919) % 1`, a fixed random order of what it matches), Reversed
(`0 - (R)`, least relevant first) or First row demoted. An arm's `then`
reorders what its ranking returns before it is shown: `demote_first`
moves the first row to the end (an arm with an exploring share keeps
the control as ranked). On a server with `hash()` in rankings (after
0.3.8), `(R) * 0 + hash(id, 7)` is a shuffle by the key; the kit's
arithmetic one works on any server.

### Interleaving

`"mode": "interleave"` compares two arms in one list: each search shows
the team draft of the two rankings (each picks its best row not yet
shown, the order of the picks by a coin at each tie), and a click counts
for the arm whose row it was. Every user sees both, so the difference is
measured within each search rather than between two groups, and fewer
users find it. The truth is exact: for each query every pattern of coins
is enumerated with the click model's expected clicks per row, so the
report gives the true share of clicks each arm wins beside the measured
one, with intervals by the delta method over users and the always-valid
p value. It runs the A/B of the same pair on the same users beside it and
says how many users each needed for its difference (the users seen times
the square of the smallest detectable difference over the true one), on
the metric each needs fewest for. In one run each on nfcorpus with
50,000 searches (about 15,000 users seen), the body's ranking against
itself with its first row demoted needed 550 users interleaved against
4,724 for the A/B (on success rate), about 9 times fewer, and shuffled
34 against 68. Title first against the body (100,000 searches) needed
about as many either way (7,348 against 7,095), since its gain is in
satisfied searches more than in which rows get the clicks.

### The queries that moved

A report's Queries tab splits each arm's true difference in success
rate from the control by query (`report.by_query`): the queries grouped
by how many relevant rows they have, each group's share of the
difference (the groups add up to the whole) and a sentence on where the
arm moved most, and the queries that lost and gained most, each with
its share of the traffic, its measured and true success rate and the
judged quality (nDCG) of what each arm showed for it.

## Real sites

A real site's study puts AI agents on a site's real search instead of a
simulator. Each agent plays one person, and the people are written from
the site's own rows. Every arm is the site with some of its search
changed, or another engine over the same rows. Nothing is written to the
site, and no clicks are made up: the agents search, open what they want
to read and say what they found, as people would.

```bash
CHLOEDB_SOURCE_HN_TOKEN=... ANTHROPIC_API_KEY=... chloedb-experiments serve --local /tmp/exp \
  --source hn=app:http://hn-app:7850 \
  --source algolia=algolia-hn:https://hn.algolia.com/api/v1
```

Real sites on the home page lists the sources. A study takes these
steps:

1. **A site, and what it lets an arm change.** A ChloeDB app says it
   takes experiments when it is started with `--experiments-token`. It
   then serves a protocol under `/v1/experiment` that needs that token
   (and no cluster key), and its answer lists its knobs in five groups:
   query understanding, retrieval, ranking (weights, or a whole `ORDER BY`
   expression of ChloeDB's SQL), blending after the ranking, and the
   index. An index can be one built another way, beside the main one
   (`--experiment-index NAME=TABLE`). chloedb-hn and chloedb-arxiv take
   experiments ([hn](../docs/chloedb/hn.md#experiments),
   [arXiv](../docs/chloedb/arxiv.md#experiments)). A public search with
   no protocol (`hn-public`, `arxiv-public`, `algolia-hn`) can be an arm
   too, with the few choices its URL takes. The site's page shows every
   knob: switches, sliders, choices and an expression box. Beside them, a
   search is shown as the site is and with your knobs, with a line from
   each row to where it moved.
2. **People.** Write personas reads a sample of the site's rows (well
   voted and ordinary, across the years) and asks Claude Opus 5.5 for
   people who would use the site, each with needs in their own words. A
   need is either known or topical. A **known** need is a row the person
   remembers and wants again, described the way they remember it, never
   by its title. The row is the exact truth: the agent found it or did
   not. A **topical** need wants rows on a subject, and any row may
   answer it. Without a key, a plain stand-in writes the people, for
   tests.
3. **A paired study.** Every need is tried on every arm, by the same
   person, each time in a fresh session. The order of the arms turns from
   need to need. A few sessions run at once, so the site sees a few
   people, not load. The agents are Claude through the Messages API, with
   the live runs' tools: search, open, done. The spend stops at the
   budget.
4. **A judge.** For a topical need, Claude grades every row shown, once:
   2 answers the need, 1 is related, 0 is not. The grades are kept
   (`exp_judged`), so every arm's rows are graded alike, and a study run
   again costs nothing more to judge.
5. **The report.** Success means the row the agent named answers the
   need: the remembered row, or one the judge grades 2. Each arm gets its
   success with a Wilson interval, and the same split by known and
   topical needs. It also gets the remembered row on the first page and
   its MRR, nDCG of the first page over the pooled grades, how often it
   was wrongly sure, searches, seconds, the site's time per search, and
   cost. Each arm is set against the control need by need: the difference
   in success with a t interval, McNemar's exact test on the needs only
   one of them found, and a sentence. The page has a grid of needs by
   arms, and each need opens the arms' sessions side by side, step by
   step.

Its API:

| route | what |
|---|---|
| `GET /v1/real` | the sources, each with its knobs and presets, or why it takes no experiments |
| `POST /v1/real/<source>/search`, `GET /v1/real/<source>/item/<id>` | a search with knobs (`q`, `knobs`, `per`), as an arm sees it; one row's text |
| `GET /v1/personas`, `POST /v1/personas` | the people written so far; write more from a source (`source`, `count`, `needs`, `brain`), in the background |
| `GET /v1/personas/<id>`, `DELETE /v1/personas/<id>` | one set, its people and their needs |
| `GET /v1/studies`, `POST /v1/studies` | the studies; a spec (`personas`, `arms` of `source` and `knobs`, `agent_model`, `judge_model`, `budget`, `workers`, `needs`) starts one, one at a time |
| `GET /v1/studies/<id>`, `GET /v1/studies/<id>/sessions?need=`, `POST /v1/studies/<id>/stop` | a study and its report; each session, step by step; stop it |

Before a study starts, each arm's knobs are checked by a search, so a
knob the site refuses fails at once. People and studies are rows of the
server (`exp_personas`, `exp_studies`). A source's token is read from
`CHLOEDB_SOURCE_<NAME>_TOKEN`, and the app sends a source at most four
requests at a time.

## The simulator

A **site** is a table, the queries people ask it (each with its share of
the traffic), and a grade per (query, row). A BEIR collection is a site with
judged queries, and their traffic follows Zipf over a seeded order. A row's
grade `g` of `G` gives it these two chances:

- attractiveness `0.04 + 0.8 × (2^g − 1)/(2^G − 1)`;
- satisfaction `0.15 + 0.7 ×` the same.

A cohort's taste multiplies attractiveness by a lognormal factor per
(age × gender group, query, row) (`--taste`, its log sd).

**Users** (`--users`) have an age, a gender, a country, a device and a
tenure, and an activity drawn so that a few users make most of the visits.
Behaviour depends on the cohort: on a phone, attention falls faster down the
list.

**Click models**, each with exact expectations, so that the truth is a sum
and not a sample:

| model | a user examines position k when |
|---|---|
| `pbm` | with chance θ_k = k^−η (counting from 1), whatever came before |
| `cascade` | they have not clicked yet: the first click ends the list |
| `dcm` | the click before did not satisfy them (with chance λ they go on anyway) |
| `dbn` | they were not satisfied and did not give up (persistence γ) |
| `ubm` | with a chance that depends on k and on the distance to the last click |
| `trust` | as `pbm`, and they also click a row that is not relevant, with a chance that falls as 1/k |

A click satisfies with the row's chance of satisfying. Dwell time is
lognormal: above 30 s for a satisfied click, below it for one that is not.
A session goes on after each query with a chance set by how that query
went, so a session's length follows from the lists it was shown.

**Two modes**, drawn bit for bit the same:

- **`served`**: each impression is a real query to chloedb-server with
  `explore` and the rank log on (table `exp_log_<run>`). The users' clicks
  are written as events (table `exp_events_<run>`). The log is then read
  back through `/v1/rank_log/export`, with each click and each satisfied
  click as a label.
- **`fast`**: the same draws in the process, from the rows the server
  ranked once per query. It is for checks over many logs and runs of
  millions of impressions.

The request of impression `i` of run `r` is `r-i`, and every draw (the user,
the query, the list, the clicks) is seeded by it. The test
`tests/served.rs` runs 150 impressions under each of six policies against a
real server. It checks three things: served equals fast, the propensities
are the server's to the last bit, and the log read back equals what was
served.

## The estimators

All of them estimate a target ranking's expected clicks and satisfied
clicks per list:

| estimator | weights each shown row (or list) by | unbiased when |
|---|---|---|
| `naive` | nothing: the log's own clicks | the target is the logger |
| `ips`, `snips` | the whole list's chance under the target over the logger's | always, given support; SNIPS normalises |
| `iips`, `sniips` | that row's chance of being at that position | a click depends only on the row and its position (`pbm`, `trust`) |
| `rips`, `snrips` | the chance of the list down to that row | a click depends only on the rows down to it (every model here) |
| `dm-pbm`, `dm-sdbn` | a click model fitted to the log, no weights | the model is right |
| `dr-iips`, `cdr` | a model's guess, corrected by IIPS or RIPS weights | either the model or the weights are right |

The models are fitted on half the log and used on the other half, by
impression parity, so that a doubly robust estimate is not fitted to its own
noise. Each estimate comes with a standard error. A self-normalised one gets
it by the delta method. Each also comes with two diagnostics of its weights:

- **ess**: the effective number of impressions its weights rest on,
  `(Σw)²/Σw²`, at its thinnest position.
- **mean weight**: on a target the log supports, the weights average 1.
  Short of 1, the log rarely showed what the target would. Above 1, a rare
  list's weight swamps the rest. The estimate carries how far the mean is
  from 1 in its standard errors.

An estimate is shown with a warning, not as a result, when its ess is under
400 or its mean weight is more than 3 standard errors from 1
(`Estimate::trusted`).

## The check

```bash
chloedb/tests/fixtures/beir/fetch.sh ~/beir
cd chloedb-experiments && cargo build --release
./target/release/chloedb-experiments check --beir ~/beir --local /tmp/exp-db --out results
```

Each case is a collection × a click model × a logging policy (`softmax`,
`epsilon`, `shuffle` and `swap` over the top 10). Each case builds 100
independent logs (`--reps`) of `--impressions` each, and values
five targets on every one of them:

- `ranking`: the logger's own ranking;
- `rerank`: the logger's rows, ordered by a ranking that weighs titles more;
- `oracle`: the logger's rows, ordered by their grade;
- `reverse`: the logger's rows, upside down;
- `wide`: the title ranking over the whole table, which reaches rows the
  logger never shows.

Each target's truth is computed exactly from the click model.

What the check asserts:

- **Propensities.** For each (query, position), a chi-square test that the
  logged lists were drawn with the chances logged.
- **Position bias.** Under `pbm` with the swap policy, the ratio of clicks
  at position k to position 1 on the swapped pairs is θ_k, within 3
  standard errors.
- **Bias.** An estimator that should hold under the case's click model has
  no bias beyond 4 standard errors over the logs (plus 0.2% of the truth).
  Bias is judged when at least half the logs have weights on 30 effective
  impressions or more that average 1 within 3 standard errors. On the
  others the log rarely shows the target's lists. Epsilon logging is the
  usual case: with ε = 0.3 over the top 10, a list of 5 that the logger's
  ranking would not show is drawn once in about 100,000 of its query's
  impressions (0.3/30,240), so most logs miss it.
- **Coverage.** Its 95% intervals, on the logs where the app shows the
  estimate as a result, hold the truth often enough: a one-sided binomial
  test at 0.001, with at least 20 such logs.
- **Expected failures.** The rows that should fail do, and say so: naive is
  biased, DM's intervals are too narrow, IIPS is biased under cascade
  models, and every weighted estimator is biased on `wide`.

`results.md` lists every case and every row with its verdict, and
`results.json` holds the same rows. `--offline` reuses the rows the server
ranked before (`fetched-<collection>.json`), with no server.

### What it found

[results/results.md](results/results.md) is the run above on NFCorpus and
SciFact, on a 4-thread Intel Xeon at 2.10 GHz with 16.5 GB (630 s, with
chloedb-server's `ci` build). It has 48 cases of 100 logs of 100,000
impressions each, and every check passed:

- **Propensities.** None of the 480 million logged lists had a chance that
  differed from the policy's. The chi-square of each case's draws against
  its chances was between p = 0.006 and p = 0.98 over the 48 cases.
- **Position bias.** The swap policy's log recovered θ_k at every
  position, within 0.002 of the truth, on 10 million impressions per
  collection.
- **Estimators that should hold, and could be judged.** 552 rows passed on
  both bias and intervals. Over those rows, the intervals the app shows
  held the truth on average 95.1% to 95.9% of the time per estimator (each
  estimator's mean over its rows).
- **Rows the app warns about.** 240 more rows were unbiased, but their
  intervals were not judged, because the app warns on every one of their
  logs. 1,080 rows were not judged at all: the logs rarely showed those
  targets' lists, which is mostly list-level and prefix weights under
  epsilon and softmax logging.
- **Expected failures.** The rest failed as expected: naive is biased, the
  models' intervals are too narrow, IIPS is biased under cascade models,
  and the weighted estimators are biased on unsupported targets.

## One run

```bash
# served: every impression a logged query to the server, the log read back
./target/release/chloedb-experiments sim --beir ~/beir/nfcorpus --local /tmp/exp-db \
  --mode served --impressions 100000 --concurrency 8 --model dbn \
  --policy '{"policy": "softmax", "tau": 1.0, "top": 10, "by": "rank"}' --run r1 --out r1.json
# fast: the same draws in the process
./target/release/chloedb-experiments sim --beir ~/beir/nfcorpus --local /tmp/exp-db \
  --mode fast --impressions 10000000 --model dbn --run r2 --out r2.json
```

Its summary has the impressions per second, the query latency (served),
every target's truth and estimates, and the machine it ran on. A served
run also counts the impressions that differ from fast, that were lost from
the log, or that differ in it. All three counts should be 0.

`--server URL` (or `CHLOEDB_URL`, with `CHLOEDB_KEY`) uses a running server
or a coordinator instead of `--local`.

[results/scale.md](results/scale.md) has runs of 1 million served
impressions and 100 million fast ones, with what they took on a 4-thread
machine: 1,466 served impressions a second with 8 in flight, with 0
differences from the fast mode or in the log, and 2 million a second in the
fast mode.

## Tests

```bash
cargo test --profile ci
```

`tests/served.rs`, `tests/app.rs` (the app's API, a fast and a served
experiment, on a site of its own tables) and `tests/live.rs` (a live run
with Claude agents against a fake Messages API, a visitor, the rate
changed, stopped into a report) and `tests/real.rs` (a made-up site that
takes experiments: its knobs, personas from its rows, a paired study that
finds its broken arm) need the chloedb-server binary: `CHLOEDB_SERVER_BIN`, or
`../chloedb-server/target/{ci,release}/chloedb-server`. Without it, they
are skipped unless `CHLOEDB_EXPERIMENTS_NEED_SERVER` is set.
