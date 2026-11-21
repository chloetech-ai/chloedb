# ChloeDB inside an LLM: handoff log

Read this first to continue the work in a new Claude session. Newest entries at the end of "Log".
Branch `claude/project-thread-2usw9h`, folder `research/llm-weights/`. Copy of this file:
`/mnt/project-files/llm-prototypes/HANDOFF.md`.

## What exists

- Report doc (results up to the night of 20 Nov): https://claude.ai/artifact/1bmABeHYLz4xgVWyDsj7Xi
- Trace page (how ChloeDB is called inside the forward pass): https://claude.ai/artifact/ERJeP2uqzRTACeoN2mC6MJ
- Question set: `results/qa_docs.json` (230 short-answer questions, written by Haiku from the trained part,
  tokens 1,280 on, of 16 docs in `docs/chloedb/`; answers verbatim in the passage). Scoring: greedy, 16 tokens,
  normalised containment of the answer. RAG = BM25 top 3 of 192-token chunks in a ChloeDB table.
- Facts: `results/facts_docs.json` (559 facts written by Haiku from the same passages in calls that never saw
  the questions; each: sentence stopping before the answer `s`, 2 rewordings `r`, own question `q`, answer `a`).
- Benchmark table: `results/qa_table.md` (built by `qa_table.py` from `results/gpu/*.json`, `results/gpu8b/*.json`).

| Technique (closed book / with RAG) | 0.6B | 4B | 8B |
|---|---|---|---|
| Base model | 2 / 46 | 6 / 60 | 7 / 57 |
| Facts as ChloeDB rows (`proto_q2_facts_qa.py`) | 41 / 45 | 44 / 52 | 41 / 49 |
| ChloeDB rows, style (G1 `rows_multi_gated_iso`) | 3 / 49 | 10 / 60 | 12 / 57 |
| LoRA | 6 / 45 | 10 / 59 | 14 / 57 |
| FFN fine-tune | 4 / 49 | 7 / 61 | not run |
| kNN-LM (G1 `knn`) | 2 / 46 | 6 / 60 | 7 / 57 |
| LoRA per doc, BM25-routed (G1 `routed_lora`) | 5 / 43 | 9 / 58 | 11 / 56 |

Facts or RAG, whichever is right (an upper bound): 67% / 77% / 73%.

## Scripts

- `proto_g1_rows_vs_finetune.py`: arms lora, ffn, rows*, knn, routed_lora; perplexity, forgetting, per-doc
  DELETE, QA (env QA=). `proto_q2_facts_qa.py`: facts as rows on the questions (env QA=, FACTS=).
- `make_qa.py`, `make_facts.py`: Haiku through `claude -p` with a minimal env (HOME = an empty dir, so it does
  not read team memory). Run them on the cloud container, not the GPU.
- GPU: `gpu/README.md`. A DigitalOcean droplet (gpu-6000adax1-48gb, image gpu-h100x1-base, tor1, $1.57/h,
  env var `digitalocean` holds the API token) boots `gpu/cloud-init*.sh`, pulls this branch every minute and runs
  each line of its jobs file not yet run (`name|ENV=..|script.py`; rename a job to rerun it); results are served
  read-only on port 80. The "reaper" (`gpu/reaper.py`, run on the cloud container with env IP, DROPLET, DEST,
  HOURS) copies results to `/mnt/project-files/llm-prototypes/<dir>/` every 5 minutes and deletes the droplet
  when idle or at the deadline. ALWAYS check that no droplet tagged `llm-weights-research` is left running.
- Budget: Roberto's cap is $50 of GPU; about $20 spent by 21 Nov 6 AM PST.

## Rules

Times to Roberto in PST. Commit only to this branch; no model names in commits. Never print env var values.
Keep Claude usage low (Roberto is near his plan's limit): few turns, no subagents.

## Log

- 21 Nov 9:20 AM PST. Roberto: "Go for all of them" (the five next steps below) and keep this log.
  1. Gate between facts and RAG (use the fact when its match is strong or its answer more confident, else RAG).
  2. Scope the fact lookup to the doc BM25 routes to (`WHERE doc = ...`), for accuracy and exact per-doc DELETE.
  3. Choose what goes into the layers by kind of fact (identifiers, numbers, flags into fact rows; prose to RAG).
  4. A fairer test: questions written by another model (Sonnet), in a user's style, never seen by the fact writer.
  5. Scale: add a few thousand facts from the other docs as distractors; measure interference and lookup time.
- 21 Nov 9:21 AM PST. Roberto: "If we need to use RAG to answer something it beats the purpose of not needing to
  read lots of tokens" -> report prompt tokens per question for every arm; the goal is accuracy at few tokens
  (fact rows = 0 extra tokens, a fact as text ~40, RAG top 3 ~600). New GPU budget for this round: $30.
- 21 Nov 10:15 AM PST. Done: `results/qa_sonnet_user.json`, 222 questions written by Sonnet as a developer
  would ask them (`QMODEL=sonnet STYLE=user make_qa.py`), the held-out test set; the Haiku set is the dev set for
  tuning thresholds. Running: distractor facts from the 20 other docs (`DOCSET=others LIMIT=48000 make_facts.py`,
  to `results/facts_others.json`). Next: `proto_q3_combo.py` (not written yet): per question, BM25 route; fact
  lookup global and `WHERE doc = route` (and `AND kind = 'ident'`); answers with confidence and prompt tokens for
  base, fact rows, fact as text, RAG top 1 and top 3; per-doc DELETE with scoped lookup; then distractors added
  and lookups again. Gates are computed offline from the per-question records (threshold tuned on Haiku, scored
  on Sonnet). One droplet, jobs file `gpu/jobs_q3.txt`, models 0.6B, 4B, 8B.
- 21 Nov ~10:50 AM PST. `proto_q3_combo.py` and `q3_gates.py` written and smoke-tested on CPU (SMOKE=1).
  Distractors: `results/facts_others.json`, 584 facts (the other docs are short, so 1,143 facts in all, not
  thousands). Launching 3 droplets, one per model (`gpu/cloud-init-one.sh` with @REPO@/@DIR@/@JOBS@ filled in,
  jobs `gpu/jobs_q3_<model>.txt`), each with a reaper copying to `/mnt/project-files/llm-prototypes/q3_<model>/`.
  When done: `python3 q3_gates.py /mnt/project-files/llm-prototypes/q3_*/q3_*.json` prints the tables.
- 21 Nov ~11:05 AM PST. LAUNCHED (tor1, RTX 6000 Ada, $1.57/h each; SSH key id 61069971, the key in the old
  session's scratchpad): droplet `llm-weights-q3-06b` id 622029113, IP 201.79.149.253, runs
  `gpu/jobs_q3_06b.txt` = q3_06b, fetch_8b, q3_8b (no third GPU was available) -> results in
  `/mnt/project-files/llm-prototypes/q3_06b/`, reaper deadline ~6:05 PM PST. Droplet `llm-weights-q3-4b` id
  622029118, IP 165.245.237.203, runs q3_4b -> `q3_4b/`, reaper deadline ~4:35 PM PST. If this session is gone,
  a new one must check `GET /v2/droplets?tag_name=llm-weights-research` (env `digitalocean`) and delete what is
  left once `status.txt` on port 80 says idle; then delete SSH key 61069971.
