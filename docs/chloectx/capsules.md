# Capsules: handing work to the next agent

A capsule is a slice of a project's memory packed into one read-only ChloeDB file, which one agent hands to another. The receiving agent does not read it: Claude Code mounts it as MCP tools, and the agent asks it questions, under its own budget, when it needs to.

**What we measured.** A handoff note alone is enough when it is long and complete. A capsule pays off when the note is short and loses things, as a sub-agent's report usually does. With a short note (about 2,000 characters), the next agent solved 72% of follow-up tasks. With the same note plus a capsule, it solved 89%, about 20,000 more input tokens and $0.0013 more per run. A capsule answers in 8 to 9 ms and mounts in about 65 ms. Making one takes 0.2 s and no model calls.

## Use it

```sh
# On the machine (or in the session) that did the work, where ChloeCtx recorded it:
chloectx pack ./billing.capsule                       # distilled notes, entities and the conversation
chloectx pack ./billing.capsule --since-days 7 --kinds decision,preference,fact,todo,note,entity

# In the next agent's Claude Code (one session, or `claude mcp add` for a project):
claude --mcp-config '{"mcpServers":{"capsule":{"command":"chloectx","args":["mount","/path/to/billing.capsule"]}}}'
```

`pack` writes a directory with two files:

- `memory.strata`: the chosen records with their vectors, links and indexes, in one file. By default it holds decisions, rules (`preference`), facts, open tasks, notes, the entities they link to, and the conversation (prompts, replies, episodes, summaries, errors). Raw tool calls and their results stay behind, because they are bulky and where secrets pass. Every record is redacted again on the way out.
- `manifest.json`: what it holds (counts by kind, the entities it is about), the embedder its vectors need, the source store's last sequence number, and the file's SHA-256.

`mount` serves one or more capsules over stdio as `capsule_ask` (the question answered from the capsule, packed to a token budget, with no model call), `capsule_search`, `capsule_timeline` and `capsule_sql`, the same code as ChloeCtx's `memory_*` tools. It checks the file against the manifest's hash and opens a private copy, so mounting never changes the capsule. Its MCP instructions say what the capsule holds, so the agent knows when to ask. `--log FILE` (or `CHLOECTX_CAPSULE_LOG`) writes one JSON line per call with its time.

Not yet done: signatures (the hash names the file but does not say who made it), the receipts' named snapshots, and peer queries against another agent's live memory.

## Results

**Method.** The four held-out projects of the [memory benchmark](results.md#memory-systems-compared) (stockroom, slotwise, bakehouse, kestrel; 52 follow-up tasks) were used. ChloeCtx recorded each project's history once. Every handoff lane then ran on that same history and repository, with ChloeCtx turned off for the run (no hooks, no memory tools), and handed the next agent one of the following:

| lane | what the next agent gets |
|---|---|
| nothing | nothing |
| long note | a handoff note in its system prompt. Haiku wrote it from the transcripts of every history session, with no length limit (5,800 to 12,100 characters) |
| short note | a note Haiku was asked to keep under 1,500 characters (it wrote 1,900 to 2,300), about the size of a sub-agent's final report |
| capsule | the capsule, mounted as MCP tools |
| long note + capsule, short note + capsule | both |
| ChloeCtx | ChloeCtx itself: briefing, recall and memory tools |
| whole history | a fork of one session that holds the whole history, as an upper bound |

Each task ran four times per lane, for 1,664 runs. Haiku did all the work. The harness is [`chloectx/scripts/memory_bench.py`](https://github.com/chloetech-ai/chloedb/blob/main/chloectx/scripts/memory_bench.py) (its `BENCH_HANDOFF` variants), the pooling is [`capsule_report.py`](https://github.com/chloetech-ai/chloedb/blob/main/chloectx/scripts/capsule_report.py), and every run, note and history is in [`chloectx/results/capsules/`](https://github.com/chloetech-ai/chloedb/tree/main/chloectx/results/capsules). The runs were measured on 12 November 2026 in a 4-core cloud container, with four projects running side by side, and that load is in the wall times.

| lane | solved | needs an earlier session: solved | $ per run | input tokens per run, p50 | wall s, p50 / p95 | runs that asked the capsule or memory |
|---|---|---|---|---|---|---|
| nothing | 94/208 (45%) | 42/156 | 0.0047 | 131,037 | 15 / 31 | 0 |
| short note | 149/208 (72%) | 97/156 | 0.0048 | 132,284 | 17 / 35 | 0 |
| capsule | 189/208 (91%) | 137/156 | 0.0063 | 160,170 | 18 / 37 | 154 |
| short note + capsule | 186/208 (89%) | 134/156 | 0.0061 | 147,227 | 18 / 39 | 147 |
| long note | 198/208 (95%) | 146/156 | 0.0049 | 139,422 | 17 / 39 | 0 |
| long note + capsule | 198/208 (95%) | 146/156 | 0.0061 | 150,554 | 18 / 39 | 125 |
| ChloeCtx | 197/208 (95%) | 145/156 | 0.0071 + 0.0019 (its query engine) | 165,094 | 20 / 37 | 50 |
| whole history | 198/208 (95%) | 146/156 | 0.0117 | 194,426 | 11 / 32 | 0 |

Paired comparisons (labels shuffled within each task, 20,000 times):

| comparison | solved | p | input tokens per run | wall s per run |
|---|---|---|---|---|
| short note + capsule against short note | +17.8 points | < 0.0001 | +20,340 | +1.7 |
| long note + capsule against long note | 0 points | 1.0 | +21,646 | +1.2 |
| capsule alone against long note | −4.3 points | 0.011 | +30,998 | +0.7 |
| capsule alone against nothing | +45.7 points | < 0.0001 | +24,753 | +3.3 |

What this shows:

- **A complete note is as good as anything here.** A long note did as well as ChloeCtx and as the whole history in context, at 42% of the whole history's cost per run. Adding a capsule to it bought nothing and added about 22,000 input tokens per run.
- **A capsule recovers what a short note drops.** The short note lost facts on two of the four projects (slotwise 28/40, bakehouse 34/72). Adding the capsule brought them to 38/40 and 60/72. On the other two projects the short note was already enough.
- **Agents do use it.** With a capsule mounted, the agent asked it in 60 to 74% of runs, almost always through `capsule_ask`.
- **The cost is the answer's size, not the engine.** Each `capsule_ask` returned about 16,000 characters (about 4,000 tokens, the default budget), which accounts for most of the extra input. A smaller default budget is the next thing to try.

The capsule's own numbers:

| measure | value |
|---|---|
| records and size | 273 to 770 records; 0.95 to 2.5 MB (3.3 to 3.5 KB a record, vectors and indexes included) |
| `chloectx pack`, backup and cut included | 161 to 223 ms |
| model calls to pack or to query | none (its notes were distilled while ChloeCtx recorded the history, which cost $0.03 to $0.12 per project) |
| mount ready, hash check and copy included | 61 to 67 ms p50, 82 to 87 ms p95 |
| one call | 8 to 9 ms p50, 11 to 14 ms p95, 18 ms at most, over 433 calls |

A handoff note cost $0.007 to $0.019 to write. Both are made once per handoff.

**Limits.** Four small projects, one history each, and one small model. The tasks were written alongside the histories and aim at facts the histories state. The long note was written from complete transcripts by a model asked for completeness, which is the best case for a note. A sub-agent writing from its own context, under its own budget, is closer to the short note.
