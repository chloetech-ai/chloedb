# memory_ask by search alone, on BEAM 100K (2026-11-08, cloud container)

BEAM 100K: 20 conversations, 400 questions over ten abilities. Each
conversation is its own store, its exchanges distilled in order by `main`'s
distiller (`engram-bench deep --notes`, Haiku; `distilled.json`: $5.68 for
the 20, $0.28 a conversation, 2,866 exchanges, 4,637 notes). Three contexts
per question, all read by the same reader and graded by the same judge:

- **search**: what `memory_ask` now returns, built by `engram::context`
  with its defaults (`engram-bench search --notes DIR --budgets 12000`),
  15,200 characters on average, no model call, 5.2 ms a question on one
  thread (Intel Xeon @ 2.10GHz, 4 cores);
- **om**: observational memory (Mastra's method), rebuilt from its own
  prompts by `scripts/om_baseline.py` (`@mastra/memory` 1.41.0, observer
  Haiku): the whole observation log and the unobserved tail, 153,400
  characters; the observer cost $2.55 for the 20 conversations
  (`om_observer.json`);
- **query_engine**: the picks of the query engine (`memory_ask` before this
  change, which plans searches and picks with two model calls), the contexts
  committed in `../../v2/benchmarks/beam_100k/` (2026-10-15), 1,600
  characters.

Reader and judge: Haiku through `claude -p`, BEAM's rubric judge, and the
answer prompt of an assistant reading its memory (`bench_eval.py
--answer-prompt engram`). Each context set is graded three times
(`reader/<name>_<n>/`): 395 identical contexts graded twice differed by two
points, so one grading decides nothing. Scores are the means.

| ability | search | om | query_engine |
|---|---|---|---|
| information extraction | 71.2 | 75.1 | 71.8 |
| knowledge update | 71.2 | 74.0 | 48.5 |
| summarization | 27.9 | 37.4 | 18.4 |
| event ordering | 37.9 | 38.1 | 35.7 |
| multi-session reasoning | 50.2 | 54.6 | 46.7 |
| temporal reasoning | 69.8 | 70.8 | 59.2 |
| contradiction resolution | 44.7 | 47.8 | 48.6 |
| abstention | 64.6 | 69.6 | 68.8 |
| instruction following | 83.3 | 84.4 | 76.0 |
| preference following | 80.8 | 82.1 | 76.7 |
| **all** (each grading) | **60.2** (61.8, 59.5, 59.2) | **63.4** (64.0, 63.7, 62.6) | **55.0** (55.2, 54.2, 55.7) |
| reader tokens a question | 6,235 | 59,298 | 1,445 |

Paired over questions (bootstrap, means of the three gradings): search
against om −3.2 points, 95% interval −5.9 to −0.5; against the query engine
+5.1, +2.1 to +8.3.

The evidence BEAM marks for each question (`search.json`, 355 questions with
evidence): some of it is in the context for 86%, all of it for 49%.

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
