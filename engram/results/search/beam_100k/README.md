# memory_ask by search alone, on BEAM 100K (2026-11-08, cloud container)

BEAM 100K: 20 conversations, 400 questions over ten abilities. Each
conversation is its own store, its exchanges distilled in order by `main`'s
distiller (`engram-bench deep --notes`, Haiku; `distilled.json`: $5.68 for
the 20, $0.28 a conversation, 2,866 exchanges, 4,637 notes). The contexts,
all read by the same reader and graded by the same judge:

- **search**: what `memory_ask` returns, built by `engram::context` with its
  defaults (`engram-bench search --notes DIR --budgets 12000`), 16,400
  characters on average, no model call, 6.7 ms a question on one thread
  (Intel Xeon @ 2.10GHz, 4 cores). A question about order or a summary gets
  the replies of eight exchanges, 500 characters each;
- **search_366**: the same as first merged (#366), where those questions got
  no replies; the other 320 questions' contexts are the same as search's;
- **om**: observational memory (Mastra's method), rebuilt from its own
  prompts by `scripts/om_baseline.py` (`@mastra/memory` 1.41.0, observer
  Haiku): the whole observation log and the unobserved tail, 153,400
  characters; the observer cost $2.55 for the 20 conversations
  (`om_observer.json`);
- **query_engine**: the picks of the query engine (`memory_ask` before
  #366, which plans searches and picks with two model calls), the contexts
  committed in `../../v2/benchmarks/beam_100k/` (2026-10-15), 1,600
  characters.

Reader and judge: Haiku through `claude -p`, BEAM's rubric judge, and the
answer prompt of an assistant reading its memory (`bench_eval.py
--answer-prompt engram`). Each context set is graded three times
(`reader/<name>_<n>/`): 395 identical contexts graded twice differed by two
points, so one grading decides nothing, and an ability's score moves by up to
three points between gradings of the same contexts. Scores are the means.

| ability | search | search_366 | om | query_engine |
|---|---|---|---|---|
| information extraction | 73.9 | 71.2 | 75.1 | 71.8 |
| knowledge update | 70.4 | 71.2 | 74.0 | 48.5 |
| summarization | 30.4 | 27.9 | 37.4 | 18.4 |
| event ordering | 40.8 | 37.9 | 38.1 | 35.7 |
| multi-session reasoning | 53.1 | 50.2 | 54.6 | 46.7 |
| temporal reasoning | 67.9 | 69.8 | 70.8 | 59.2 |
| contradiction resolution | 45.7 | 44.7 | 47.8 | 48.6 |
| abstention | 67.5 | 64.6 | 69.6 | 68.8 |
| instruction following | 84.8 | 83.3 | 84.4 | 76.0 |
| preference following | 80.0 | 80.8 | 82.1 | 76.7 |
| **all** (each grading) | **61.5** (61.8, 61.6, 60.9) | **60.2** (61.8, 59.5, 59.2) | **63.4** (64.0, 63.7, 62.6) | **55.0** (55.2, 54.2, 55.7) |
| reader tokens a question | 6,651 | 6,235 | 59,298 | 1,445 |

Paired over questions (bootstrap, means of the three gradings): search
against om −1.9 points, 95% interval −4.5 to +0.9; against the query engine
+6.4, +3.3 to +9.6; against search_366 +1.3, −0.3 to +2.9. On the 80
questions about order or a summary, where the two differ, from 32.9 to 38.3
(+5.4, +2.3 to +8.7; om 37.8).

The evidence BEAM marks for each question (`search.json`, 355 questions with
evidence): some of it is in the context for 88%, all of it for 50%.

To run it again (`engram-bench` built with `--profile ci`, the data from
`scripts/convert_new_benchmarks.py`):

```bash
engram-bench deep --data beam_100k.jsonl --notes notes --planner none --ranker local   # distils each conversation once
engram-bench search --data beam_100k.jsonl --notes notes --out search --budgets 12000
python3 scripts/bench_eval.py --bench beam --data beam_100k.jsonl --reader haiku --judge haiku --workers 3 \
  --answer-prompt engram --config search=search/beam_100k_search_12000.contexts.jsonl --out reader/search_1
```

Run model calls with a clean environment (no Claude Code session variables
or hooks around them), or a session's own context leaks into every reader and
judge call.
