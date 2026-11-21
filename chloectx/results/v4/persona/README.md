# One memory, many models

Can one person's memory, stored once, serve several models, and how does it
compare with a plain memory file and with the whole history in context?

**Data.** LongMemEval-S, every fifth question (100 questions, one per person).
Each person has about 47 past conversations with an assistant, about 500,000
characters (168,000 tokens for Claude), and one question asked after them.

**Contexts.** What each reader was given before the question:

| context | what it is | model calls to keep or find it |
|---|---|---|
| No memory | the question only | none |
| Plain memory file | a file about the user that Haiku rewrites after every five conversations, in order, asked to stay under 4,000 characters (`scripts/profile_file.py`); it averaged 4,787 (largest 6,493) after up to two calls asking it to shorten | 25.6 calls, $0.131 and 9 minutes of model time per person |
| ChloeCtx, search | the records ChloeCtx's hybrid search returns for the question, packed to 4,000 characters (`chloectx-structure --dump win400+req/vec-text/snip600/4000`) | none: storing and searching use no model |
| ChloeCtx, query engine | the same, plus the records the query engine picked after a model planned searches and ranked them (`chloectx-bench deep`) | two Haiku calls per question: $0.00115, 6.4 s median one at a time (9.7 s with six in flight) |
| Whole history in context | every conversation, in order, with its date | none |

**Readers.** Claude Haiku, Sonnet and Opus (`claude-haiku-5-5`,
`claude-sonnet-5-5`, `claude-opus-5-5`) through `scripts/reader_eval.py`, one
call each, no tools. The whole history was read by Sonnet and Opus on every
fifth of the 100 questions only (25), since each read costs $0.55 and $1.10.
**Judge:** Haiku with LongMemEval's grading prompts, ten answers per call.

## Results

| reader | context | n | correct | tokens read | $ reader / q | $ memory / q | reader s (p50) |
|---|---|---|---|---|---|---|---|
| haiku | No memory | 100 | 9% | 514 | 0.00018 | 0 | 1.5 |
| haiku | Plain memory file | 100 | 45% | 2,691 | 0.00085 | (0.131 per person, to keep) | 3.0 |
| haiku | ChloeCtx, search | 100 | 68% | 1,831 | 0.00070 | 0 | 3.1 |
| haiku | ChloeCtx, query engine | 100 | 84% | 2,159 | 0.00077 | 0.00115 | 3.1 |
| haiku | Whole history in context | 100 | 88% | 168,447 | 0.17038 | 0 | 4.1 |
| sonnet | No memory | 100 | 8% | 493 | 0.00322 | 0 | 2.0 |
| sonnet | Plain memory file | 100 | 49% | 2,670 | 0.01380 | (0.131 per person) | 2.4 |
| sonnet | ChloeCtx, search | 100 | 75% | 1,810 | 0.01031 | 0 | 2.3 |
| sonnet | ChloeCtx, query engine | 100 | 87% | 2,138 | 0.01162 | 0.00115 | 2.3 |
| sonnet | Whole history in context | 25 | 96% | 168,879 | 0.54821 | 0 | 3.6 |
| opus | No memory | 100 | 9% | 489 | 0.00751 | 0 | 3.1 |
| opus | Plain memory file | 100 | 50% | 2,666 | 0.02903 | (0.131 per person) | 4.4 |
| opus | ChloeCtx, search | 100 | 80% | 1,806 | 0.02190 | 0 | 3.7 |
| opus | ChloeCtx, query engine | 100 | 89% | 2,134 | 0.02418 | 0.00115 | 3.8 |
| opus | Whole history in context | 25 | 100% | 168,875 | 1.09620 | 0 | 7.6 |

On the 25 questions the whole history was read on, the other contexts scored:
Sonnet: no memory 16%, plain file 60%, search 72%, query engine 88% (whole
history 96%); Opus: 16%, 56%, 84%, 92% (whole history 100%).

Opus by question type, plain memory file against ChloeCtx's query engine:
what the assistant itself said 3/11 against 11/11, preferences 2/6 against
6/6, dates and order 12/25 against 23/25, facts across conversations 10/25
against 18/25, facts that changed 9/14 against 13/14, facts the user stated
8/13 against 12/13, unanswerable questions 6/6 for both.

What it means:

- **Every model gains the same way from one store.** ChloeCtx's query engine
  context, built once per question with Haiku, took Haiku from 9% to 84%,
  Sonnet from 8% to 87% and Opus from 9% to 89%.
- **A plain file loses half the answers.** At about the same size as
  ChloeCtx's context (2,700 tokens against 2,100), the rewritten file was
  right 45 to 50% of the time against 84 to 89%: each rewrite dropped details
  that a later question needed, above all dates, preferences and what the
  assistant had said. Keeping it current cost $0.131 per person in model calls;
  ChloeCtx stores without one.
- **The whole history is the ceiling, at 79 times the tokens.** It read
  168,900 tokens a question against 2,134, and cost $1.10 a question with Opus
  against $0.025 with ChloeCtx (its own model calls included): 43 times less.
  With Haiku, $0.170 against $0.0019 (89 times); with Sonnet, $0.548 against
  $0.0128 (43 times). Opus read and answered in 3.8 s against 7.6 s; the query
  engine's own two calls add about 6.4 s, which Claude Code runs in the
  background and `memory_ask` without `deep` skips (ChloeCtx, search: 3.7 s).
  At this size the history fits a 200,000-token window; at a few months more of
  chats it does not.

## Caveats

- One question per person, 100 in all; the whole-history rows for Sonnet and
  Opus are 25 questions. Differences of a few points are within noise.
- The plain memory file is one reasonable recipe (Haiku, rewritten every five
  conversations, 4,000 characters asked). A larger file or a stronger model
  would keep more, and read more.
- The reader and the judge are the same family; the judge's prompts are
  LongMemEval's own.
- `claude -p` in this cloud container inherits Claude Code's memory settings
  and reads about 9,000 tokens of them; every call here ran under `iso.sh`,
  which starts it with an empty environment. A first run without it was thrown
  away.

## Files

`summary.md`, `summary.json` (from `summarize.py`), `runs/<reader>/` and
`runs/<reader>_full/` (answers and verdicts), `profile_4000.jsonl.gz` (the
plain memory files), `context_search.jsonl.gz`, `context_engine.jsonl.gz`,
`deep_rows.jsonl.gz` and `deep.json` (the query engine's run),
`deep_timing_rows.jsonl.gz` (20 questions one at a time, for its time),
`reader.sh`, `iso.sh`, `machine.txt`.

To run it again (paths as on the machine it ran on):

```bash
B=target/release D=/home/user/data/memory
./iso.sh python3 -I scripts/profile_file.py --data $D/longmemeval_s.jsonl --every 5 --chars 4000 --model haiku --workers 12 --out profile_4000.jsonl
$B/chloectx-structure --data $D/longmemeval_s.jsonl --every 5 --layouts win400+req/vec-text --out structure --dump win400+req/vec-text/snip600/4000
./iso.sh $B/chloectx-bench deep --data $D/longmemeval_s.jsonl --every 5 --threads 6 --out deep --tag persona
# engine context = search context + "Records a search over the whole memory picked for this question:" + deep picks
./reader.sh opus 5 reader/opus none=none profile=profile_4000.jsonl search=<search context> engine=<engine context>
./reader.sh opus 20 reader/opus_full full=full
```

Cloud container: 4-core Intel Xeon, 15 GB, Linux, 21 Nov 2026 UTC (see `machine.txt`).
