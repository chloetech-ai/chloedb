# Black Box

The flight recorder for coding agents: one searchable, tamper-evident record of what agents
did across several harness formats, where any line of a patch leads back to the step and the
reasoning that wrote it.

Page: `/blackbox/`. Tables: `blackbox_*`. Screenshots: `shots/ask.png`, `shots/blame.png`
(and the other screens).

## What is on the page

| screen | what it does | the ChloeDB behind it |
|---|---|---|
| Ask | six canned questions, free text, a quoted substring, or a `SELECT`; results as session lanes with the hits marked | substring `LIKE` on `cmd`, `path` and `text`; `rrf(bm25, similarity)` with `PACK … EXCERPT`; `GROUP BY … HAVING` |
| Blame | the final patch with, on each added line, the step that wrote it; click a line and the thread draws: the edit, the read it rests on, the thought before it, the failing output before that | `blackbox_blame`, then `REACHABLE(links, :edit_step, hops => 4)` |
| Session | a strip of every step coloured by kind, a scrubber, thought / call / output at the cursor, the patch growing | key reads of `blackbox_steps` |
| Ask this session | an answer over a packed context, with the receipt of what went in | `PACK 6000 TOKENS OF text DEDUPE 0.85 FOLLOW links 1 HOP WITH RECEIPT`, then one model call |
| Ingest report | the canary sweep: recall of four redactors on planted secrets, and what they flag where nothing was planted | `blackbox_sweep`, `blackbox_sweep_flags`, `blackbox_canaries` |
| Chain | verify one session or all of them; change one character on a scratch copy and watch verification fail | rows read back and re-hashed; `UPDATE` on `blackbox_scratch` |
| Method | sources and licences, models and cost, what the page does not show, every number with how it was measured | `blackbox_numbers` |

## Files

| file | what |
|---|---|
| `normalise.mjs` | seven adapters (OpenHands tool calls, SWE-agent tool calls, SWE-agent fenced text, OpenHands event stream, Claude Code JSONL (also Swival's export), Cursor JSONL, opencode JSON) to one shape; the labelled edges; the hash chain; blame. Pure functions, no I/O |
| `prep.mjs` | datasets -> `data/sessions.jsonl.gz`, `steps.jsonl.gz`, `blame.jsonl.gz`, `chain-root.json`. ChloeDB itself reads the Parquet files (`POST /v1/import?format=parquet` into a scratch table, read back, dropped). Exports `readSessions()` for another app that wants the same rows under its own prefix |
| `load.mjs` | `data/` -> the `blackbox_*` tables; safe to run twice (a table already whole is left alone, `--force` rewrites) |
| `measure.mjs` | every number on the page -> `blackbox_numbers` (and `data/numbers.json`); `--only counts,blame,timing,chain,finding,sweep,titles,answers` |
| `core.mjs` | what the API and the measurements both run: the canned questions, search, the thread, verification, the pack |
| `app.mjs` | the API; `meta.hub` names the numbers the hub's card shows; `GET /blackbox/api/numbers` returns them by id |
| `sweep/rs`, `sweep/go` | the redactors, as two small programs (below) |

```sh
export CHLOEDB_URL=http://127.0.0.1:7803
node apps/blackbox/prep.mjs       # needs the datasets (BLACKBOX_DATASETS, default /home/user/datasets)
node apps/blackbox/load.mjs       # about a minute
node apps/blackbox/measure.mjs    # needs go and cargo for the sweep; ANTHROPIC_API_KEY for titles and answers
PROTOS_APPS=blackbox PORT=8003 node server.mjs
```

## Data and licences

| source | harness, format | licence | sessions | steps |
|---|---|---|---|---|
| nebius/SWE-rebench-openhands-trajectories (sample of 300) | OpenHands, tool-call messages | CC BY 4.0 | 300 | 53,113 |
| SWE-bench/SWE-smith-trajectories (sample of 300) | SWE-agent, tool calls | MIT | 300 | 24,048 |
| nebius/SWE-agent-trajectories (sample of 300) | SWE-agent, fenced text | CC BY 4.0 | 300 | 24,750 |
| OpenHands/openhands-feedback (sample) | OpenHands, event stream | MIT | 40 | 2,439 |
| trace-commons agent traces | Claude Code, Cursor, opencode: native files | CC BY 4.0 | 7 | 962 |
| Swival security audits | Swival, Claude Code compatible JSONL | MIT | 4 | 39 |

951 sessions, 105,351 steps, six harnesses, eight on-disk formats (seven adapters). The page
says these counts, read from the rows.

Dataset text stays out of git (`.gitignore`): `prep.mjs` rebuilds it, and `load.mjs` loads it
from a machine that has the datasets. Dataset files are read as data; nothing from a dataset
folder is executed.

## How the numbers were measured

All on the development container (4 cores, chloedb-server 0.3.7, one node), 2026-11-17.

- **Blame coverage: 86,444 of 93,832 added lines (92.1%)** over the 585 OpenHands and SWE-smith
  sessions with a patch. OpenHands 52,942 / 59,486 (89.0%); SWE-smith 33,502 / 34,346 (97.5%).
  A line is traced when an edit call on that file holds the line exactly in the content it
  wrote; ties go to the call that also wrote the neighbouring lines, then the latest. 168 more
  lines are found only in a shell command's text and 7,220 have no recorded call; neither
  counts as traced. 21,161 blank lines are not counted at all. The SWE-agent text format is
  measured apart (2,863 / 4,156, 68.9%) and left out of the headline. Median distance from an
  edit back to the agent's latest read of that file: 21 steps (8,811 lines).
- **Query time: p50 2.8 ms, p95 11.0 ms** over the six canned questions, 50 runs each (300
  runs), server-reported, over all steps. Repeated runs are answered partly from the server's
  filter cache; each question's first run is stored beside its p50. The blame thread
  (`REACHABLE`) from 50 different edits: p50 0.19 ms, p95 0.30 ms.
- **Chain: 951 of 951 sessions verify**; reading all 105,351 steps back and recomputing every
  hash took 5.7 s (one run, four sessions at a time); one session of median length (87 steps)
  10 ms. The root over all heads equals the one written at ingest to `data/chain-root.json`.
- **The finding** (one statement): sessions that ran one command five or more times, by
  verdict. OpenHands 30/147 unresolved vs 22/153 resolved; SWE-smith 28/76 vs 44/224;
  SWE-agent 67/283 vs 2/17. A correlation within each source, not a cause.
- **Canary sweep.** 367 example secrets (those of the 575 example strings in
  `secrets/provider-formats.jsonl` that their own rule matches standing alone, less Nosey
  Parker's identifier rules and repeats) and 200 generated ones with no provider shape (seeded:
  hex, base62, base64, a passphrase), each planted into copies of real steps in four carriers
  (an `NAME=value` line in the record's printed environment, a curl `Authorization` header, a
  config key with an unremarkable name, a traceback line with no name): 2,268 documents.
  Caught = every byte of the secret removed; a partial removal is a miss.

  | redactor | known formats | no provider shape | real steps it would change |
  |---|---|---|---|
  | ChloeCtx `redact.rs` | 730 / 1,468 (49.7%) | 399 / 800 (49.9%) | 370 / 104,244 |
  | Nodwyn L1 | 128 / 1,468 (8.7%) | 0 / 800 | 6 / 104,244 (its entropy residual names 7,183 more and removes none) |
  | gitleaks rules | 774 / 1,468 (52.7%) | 386 / 800 (48.3%) | 288 / 104,244 |
  | Nosey Parker rules | 893 / 1,468 (60.8%) | 292 / 800 (36.5%) | 85 / 104,244 |

  The carrier decides most of it: with no provider shape, the best redactor removes 199/200 in
  the environment line and 200/200 behind the bearer header, and 0/200 in the config file and
  0/200 in the traceback. Cross-evaluation: gitleaks rules on Nosey Parker's examples 622 /
  1,308, Nosey Parker rules on gitleaks' examples 77 / 160 (on their own: 152 / 160 and 816 /
  1,308; only the secret is planted, without the line its example came in).

  The redactors are the real code, not ports: `sweep/rs` compiles `chloectx/src/redact.rs`
  unmodified (`#[path]`) and runs the Nosey Parker rules on the Rust `regex` crate (inline
  `(?# )` comments removed; 189 compile); `sweep/go` compiles an unmodified copy of Nodwyn's
  `internal/detect/l1/l1.go` (Apache-2.0, in `sweep/go/l1/`) and runs the gitleaks rules on Go
  `regexp` with their keywords and entropy floors and without their allowlists (221 compile;
  the one path rule is skipped). L1 is Nodwyn's first layer only.
- **Models.** Titles: `claude-haiku-5-5`, one call per session, 948 of 951 written, $0.076.
  Recorded answers: `claude-sonnet-5-5` over a 6,000-token pack, 9 answers, $0.178 (about 2
  cents each). Live answers on the page go through `lib/llm.mjs` with a $4 a day cap for this
  app (`BLACKBOX_ASK_DAILY_USD`); past it, or without a key, the page shows the recorded answer
  and says so. No other number involves a model.

## What it does not claim

- **No replay digest.** These are imported trajectories. The hash chain proves the record is
  unchanged since our ingest (given the root kept outside the database); it says nothing about
  what happened before ingest.
- **No real secrets were caught.** The public sessions hold none; the sweep uses planted
  canaries. The redactors are measured, not applied: the stored text is as published. What
  they flag in real steps is counted as false positives without having been read one by one.
- **SWE-smith's patches are the diffs the sessions printed at submit.** In the sample used, the
  `patch` column does not belong to the trajectory beside it (none of 253 equals its session's
  own submit diff), so it is not used.
- The verdicts (resolved, unresolved) are the benchmarks' own; the donated and feedback
  sessions have none. 951 sessions are a sample, not a census.
- "Edited a test to make it pass" lists edits to test files; whether an edit weakened a test is
  for the reader.
- At ingest 1,202 native events (status lines, snapshots, titles) are left out and counted,
  and 476 long texts keep their head and tail, with the SHA-256 of the whole in the chain.

## Notes on ChloeDB 0.3.7 met while building

- `REACHABLE … ORDER BY hops(), seq` is refused ("a score ranks one table's rows"); the thread
  is ordered `BY hops(), id` and sorted by `seq` in the app.
- A query returns at most 10,000 rows; every read here stays under it (the longest session has
  787 steps).
- Importing Parquet with a list-of-struct column needs the table made first, with that column
  as `TEXT OPTIONS(indexed = false)`.
- A `set` on a field an embedding is made from is refused; titles are written with `UPDATE`.
