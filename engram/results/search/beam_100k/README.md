# memory_ask by search alone, on BEAM 100K (2026-11-08, cloud container)

BEAM 100K: 20 conversations, 400 questions over ten abilities. Each
conversation is its own store, its exchanges distilled in order by `main`'s
distiller (`engram-bench deep --notes`, Haiku; `distilled.json`: $5.68 for
the 20, $0.28 a conversation, 2,866 exchanges, 4,637 notes). The contexts,
all read by the same reader and graded by the same judge:

- **search**: what `memory_ask` returns, built by `engram::context` with its
  defaults (`engram-bench search --notes DIR --budgets 12000`), 21,000
  characters on average, no model call, 6.9 ms a question on one thread
  (Intel Xeon @ 2.10GHz, 4 cores). A question about order or a summary (80
  of the 400) gets the replies of eight exchanges, 500 characters each, and
  every live note, oldest first (at most 50,000 characters): 42,900
  characters on average and 11 ms, against 15,500 for the others;
- **search_370**: as merged in #370, where those 80 questions got the notes
  a search finds; the other 320 questions' contexts are the same as
  search's;
- **search_366**: as first merged (#366), where those 80 got no replies
  either;
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

| ability | search | search_370 | search_366 | om | query_engine |
|---|---|---|---|---|---|
| information extraction | 71.6 | 73.9 | 71.2 | 75.1 | 71.8 |
| knowledge update | 67.9 | 70.4 | 71.2 | 74.0 | 48.5 |
| summarization | 37.2 | 30.4 | 27.9 | 37.4 | 18.4 |
| event ordering | 42.3 | 40.8 | 37.9 | 38.1 | 35.7 |
| multi-session reasoning | 50.9 | 53.1 | 50.2 | 54.6 | 46.7 |
| temporal reasoning | 68.8 | 67.9 | 69.8 | 70.8 | 59.2 |
| contradiction resolution | 50.3 | 45.7 | 44.7 | 47.8 | 48.6 |
| abstention | 70.8 | 67.5 | 64.6 | 69.6 | 68.8 |
| instruction following | 86.0 | 84.8 | 83.3 | 84.4 | 76.0 |
| preference following | 83.0 | 80.0 | 80.8 | 82.1 | 76.7 |
| **all** (each grading) | **62.9** (63.9, 62.5, 62.2) | **61.5** (61.8, 61.6, 60.9) | **60.2** (61.8, 59.5, 59.2) | **63.4** (62.6, 64.0, 63.7) | **55.0** (55.2, 54.2, 55.7) |
| reader tokens a question | 8,235 | 6,651 | 6,235 | 59,298 | 1,445 |

Paired over questions (bootstrap, means of the three gradings): search
against om −0.5 points, 95% interval −3.2 to +2.2; against the query engine
+7.8, +4.8 to +11.1. Against search_370 it is +1.4 (−0.3 to +3.2), but only
the 80 questions about order or a summary have different contexts; the rest
of that difference is the grader's noise on identical contexts.

The evidence BEAM marks for each question (`search.json`, 355 questions with
evidence): some of it is in the context for 90%, all of it for 52%.

## A second run of what each side writes

The notes and the observations are written by a model, and a second run
writes different ones, so both sides were written again:

- `distilled_2nd.json`: the same conversations distilled again ($5.59), by
  `main`'s distiller with merged notes dated by the newest note they replace;
  `search_2nd.contexts.jsonl.gz` is search on it. `reader/search_370_2nd_N`
  grade #370's contexts on it (all 400); `reader/search_2nd_N` grade only
  search's 80 questions about order or a summary, since its other 320
  contexts are the same as #370's;
- `distilled_3rd.json`: a third distillation ($5.80) by this branch's
  distiller, which also names each exchange's day in its prompt. Notes that
  carried the day of the replay instead (a relative date counted from it)
  fell from 121 of the 400 contexts to 14; #370's contexts on it
  (`reader/search_370_3rd_N`) scored 59.3;
- `om_observer_2nd.json` ($2.58) and `reader/om_2nd_N`: the observer run
  again, 155,900 characters.

| | first run | second run | mean |
|---|---|---|---|
| search | 62.9 | 60.3 | **61.6** |
| search_370 | 61.5 | 60.0 | 60.7 |
| om | 63.4 | 60.7 | **62.1** |

Averaged over the two runs of each, search against om is −0.5 points (−2.7
to +1.8) and against search_370 +0.9 (−0.1 to +1.9). On the 80 questions
about order or a summary, search against search_370 is +2.9 (−0.3 to
+6.3): summaries rose from 30.4 to 37.2 on the first distillation and from
30.7 to 36.7 on the second.

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
