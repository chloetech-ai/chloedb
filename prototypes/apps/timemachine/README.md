# Policy Time Machine (`timemachine`)

Replay an agent-governance policy over recorded history and read its two honest numbers:
**attacks it would have stopped**, and **how often it would have interrupted honest work**.

A policy is a SQL predicate on what the agent did. History is a table. Replay is one statement
over the step tables — a `COUNT(*)` for burden and a `COUNT(DISTINCT session) … WHERE seq <= harm_step`
for recall. Every number on the page comes from a ChloeDB row computed by a replay query, and
every panel ships its SQL to the drawer.

## What it shows

- **The wall** — one lane per benign session, one cell per tool-call step (300 lanes, 18,160 cells).
  As you type a policy, cells where it fires flash amber (ask) or red (deny). Click a cell for the
  step and its taint path.
- **Two dials** — attacks stopped (k / 55 labelled), and interruptions per session (and per 100 steps).
- **The frontier** — the eight preset policies plotted as burden against recall. The pitch is visible
  in the geometry: a pure sink rule (network egress) catches AgentDojo but **misses every MCPHunt
  trace, which exfiltrates to a local file**; a provenance rule (taint by source class) catches them
  at ~1/70th the burden.
- **Rule editor** — type a predicate, or describe it in English (Sonnet writes the SQL and always
  shows it). Pin a baseline and see the diff ("adds N asks, stops M more attacks").
- **Method** — datasets, licences, n, and how taint was approximated.

## Data

| set | role | sessions | steps | licence |
|---|---|---|---|---|
| OpenHands SWE-rebench trajectories (`sample-300.parquet`) | benign history (burden) | 300 | 18,160 | dataset card (nebius / SWE-rebench, OpenHands) |
| AgentDojo recorded runs (`agentdojo/repo/runs`) | labelled attacks (recall) | 25 | 206 | MIT (ethz-spylab/agentdojo) |
| MCPHunt agent traces (`deepseek_v4_flash_baseline.json`) | labelled attacks (recall) | 30 | 570 | CC-BY-4.0 |

**The AgentDojo flag.** `security = true` means the injected task's goal was achieved — verified by
reading the tool calls (e.g. `injection_task_5` fires `invite_user_to_slack Fred fred9246@gmail.com`
only when `security = true`; `security = false` runs never make that call). So the labelled AgentDojo
set is the **25** runs where the attacker's goal was reached (not all 119 attacked runs). Its name is
not inverted.

**MCPHunt.** The 30 `unsafe_success` traces, with the harmful step taken from `labeling.taint_flows`
(the first canary leak, `min(write_turn)`). These exfiltrate to a **local file**, so a network-egress
rule misses them entirely — that is the point of the frontier.

## How the numbers were measured

- **Burden** (`measure.mjs`): `COUNT(*)` of firing steps over the 300 benign sessions, divided by
  sessions and by steps/100. Deterministic, $0.
- **Recall**: `COUNT(DISTINCT session)` of attack traces where the policy fires at or before
  `harm_step`, over the 55 labelled traces. n is stated on every recall number.
- **Replay time**: p50 over 50 replays of a fresh rule over every step (~18 ms on the dev box).
- The live page recomputes burden/recall on every keystroke via the same queries (`/api/replay`);
  `measure.mjs` writes the canonical rows to `timemachine_numbers` so they survive redeploys and the
  hub card can read them.

Measured results (preset policies, this build):

| policy | action | burden / session | recall (of 55) |
|---|---|---|---|
| Package installs | ask | 0.39 | 0 |
| File deletes | ask | 0.74 | 0 |
| Edits to tests | ask | 4.31 | 0 |
| Writes outside the repo | deny | 0.00 | 0 |
| **Network egress** | deny | 0.38 | 24 (AgentDojo 24/25, MCPHunt 0/30) |
| Taint: built from any tool output | ask | 23.34 | 46 |
| **Taint from untrusted sources only** | ask | **0.32** | **45** |
| **Egress OR untrusted-taint** | deny | 0.65 | **54** |

## Taint is approximated from the record by shared substrings

At load, every tool call gets `tainted_by` **edges** to earlier outputs in the same session that
share a verbatim run of **≥ 20 characters** with its arguments, and the **source class** of each such
output (`web`, `mcp`, `file`, `test_output`, `shell`, `user`). The taint path is walkable with
`REACHABLE(tainted_by, …)` (see `/api/taintpath`). `tainted = 1` means a non-user (tool) source
contributed, so "built from tool output" is a plain predicate.

This is **not** Nodwyn's value-level taint. It over-fires on reused file paths (the workspace path
alone would taint ~every command — which is why `user` reuse is tracked separately and excluded from
the rule flag) and under-fires on paraphrased data. There is no "blast radius" number here.

## What it does not claim

- Not "a month of activity" — 300 benchmark SWE sessions plus 55 recorded attack traces.
- Recall on 55 traces is an indication, not a guarantee; the set is small and from three benchmarks.
- The benign sessions barely touch the network, so network-only rules light few cells here — the app
  leads with rules that do fire (package installs, deletes, test edits, commands built from output).

## Files

- `prep.mjs` — reads the three datasets here, imports the Parquet via `POST /v1/import`, normalises
  every tool call into a step with feature columns and the taint approximation, writes `data/*.jsonl`.
- `load.mjs` — creates the `timemachine_*` tables and loads them from `data/`; idempotent; uses
  `CHLOEDB_URL`.
- `measure.mjs` — computes burden/recall/replay-p50 and writes `timemachine_numbers`.
- `app.mjs` — the API (`/wall`, `/replay`, `/frontier`, `/numbers`, `/step`, `/taintpath`, `/meta`,
  `/translate`), with a predicate validator that only allows a boolean over the known columns.
- `public/index.html` — the page.

## Local dev

```sh
cd /home/user/chloedb/prototypes
/home/user/build-main/chloedb-server --db .dev/timemachine.chloedb --listen 127.0.0.1:7805 --grpc off --models .dev/models --start-apps off &
export CHLOEDB_URL=http://127.0.0.1:7805 ANTHROPIC_API_KEY="$STRATA_ANTHROPIC_KEY"
node apps/timemachine/prep.mjs && node apps/timemachine/load.mjs && node apps/timemachine/measure.mjs
PROTOS_APPS=timemachine PORT=8005 node server.mjs   # http://127.0.0.1:8005/timemachine/
```
