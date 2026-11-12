# Capsules: what the next agent gets, and what it costs

1664 runs over 4 projects (stockroom, slotwise, bakehouse, kestrel). Every lane but `full` runs on the same recorded history and repository (the `engram` lane's); `full` records its own. Solved = every check of the task's grader passes. Tokens are the agent's input per run, cached or not. Wall time includes the lanes running beside it.

| lane | what the next agent gets | runs | solved | needs an earlier session: solved | $ per run (agent + memory) | input tokens p50 | output tokens | steps | wall s p50 / p95 | runs that called a memory tool |
|---|---|---|---|---|---|---|---|---|---|---|
| engram+none | nothing (ChloeCtx off) | 208 | 94/208 (45%) | 42/156 | 0.0047 + 0.0000 | 131,037 | 3,269 | 7.4 | 15 / 31 | 0/208 |
| engram+summary | a handoff note in the system prompt | 208 | 198/208 (95%) | 146/156 | 0.0049 + 0.0000 | 139,422 | 4,080 | 6.2 | 17 / 39 | 0/208 |
| engram+capsule | a capsule mounted as MCP tools | 208 | 189/208 (91%) | 137/156 | 0.0063 + 0.0000 | 160,170 | 4,148 | 8.2 | 18 / 37 | 154/208 |
| engram+both | the note and the capsule | 208 | 198/208 (95%) | 146/156 | 0.0061 + 0.0000 | 150,554 | 4,365 | 6.9 | 18 / 39 | 125/208 |
| engram+short | a short note (under 1,500 characters) | 208 | 149/208 (72%) | 97/156 | 0.0048 + 0.0000 | 132,284 | 3,831 | 6.5 | 17 / 35 | 0/208 |
| engram+shortcap | the short note and the capsule | 208 | 186/208 (89%) | 134/156 | 0.0061 + 0.0000 | 147,227 | 4,246 | 7.3 | 18 / 39 | 147/208 |
| engram | ChloeCtx itself: briefing, recall and memory tools | 208 | 197/208 (95%) | 145/156 | 0.0071 + 0.0019 | 165,094 | 4,320 | 7.3 | 20 / 37 | 50/208 |
| full | the whole history in context (a fork of it) | 208 | 198/208 (95%) | 146/156 | 0.0117 + 0.0000 | 194,426 | 2,848 | 3.8 | 11 / 32 | 0/208 |

Paired comparisons (permutation test shuffling lane labels within each task; Fisher's exact test on pooled solved runs):

| lane | against | solved | difference in solved rate | p (perm / Fisher) | input tokens, difference per run | wall s, difference per run |
|---|---|---|---|---|---|---|
| engram+capsule | engram+none | 189/208 vs 94/208 | +45.7 points | 0.0000 / 0.0000 | +24,753 (p 0.000) | +3.3 (p 0.000) |
| engram+summary | engram+none | 198/208 vs 94/208 | +50.0 points | 0.0000 / 0.0000 | -6,245 (p 0.116) | +2.6 (p 0.000) |
| engram+capsule | engram+summary | 189/208 vs 198/208 | -4.3 points | 0.0106 / 0.1222 | +30,998 (p 0.000) | +0.7 (p 0.057) |
| engram+both | engram+summary | 198/208 vs 198/208 | +0.0 points | 1.0000 / 1.0000 | +21,646 (p 0.000) | +1.2 (p 0.001) |
| engram+shortcap | engram+short | 186/208 vs 149/208 | +17.8 points | 0.0000 / 0.0000 | +20,340 (p 0.000) | +1.7 (p 0.000) |
| engram+short | engram+summary | 149/208 vs 198/208 | -23.6 points | 0.0000 / 0.0000 | -5,304 (p 0.135) | -1.1 (p 0.002) |
| engram+both | engram | 198/208 vs 197/208 | +0.5 points | 1.0000 / 1.0000 | -7,943 (p 0.076) | -1.6 (p 0.000) |
| engram+both | full | 198/208 vs 198/208 | +0.0 points | 1.0000 / 1.0000 | -67,150 (p 0.000) | +7.0 (p 0.000) |
| engram | full | 197/208 vs 198/208 | -0.5 points | 1.0000 / 1.0000 | -59,207 (p 0.000) | +8.6 (p 0.000) |

The capsule's mount, from its own log (every run starts a new Claude Code session, so a new mount):

| lane | capsule calls | by tool | call ms p50 / p95 / max | capsule_ask ms p50 | characters returned per call (mean) | mount ready ms p50 / p95 |
|---|---|---|---|---|---|---|
| engram+capsule | 157 | capsule_ask 154, capsule_search 3 | 8.8 / 12.5 / 17.0 | 8.8 | 15,954 | 65 / 85 |
| engram+both | 126 | capsule_ask 126 | 9.0 / 14.4 / 16.5 | 9.0 | 16,073 | 67 / 87 |
| engram+shortcap | 150 | capsule_ask 148, capsule_search 2 | 8.4 / 11.3 / 18.4 | 8.4 | 16,045 | 61 / 82 |

Making the handoff, once per project:

| project | capsule records | capsule bytes | pack ms (backup + cut) | note characters (short note) | note written from (transcript characters) | note $ (short note $) | ChloeCtx's own model calls while recording the history, $ |
|---|---|---|---|---|---|---|---|
| stockroom | 283 | 983,040 | 187 | 5,792 (1,906) | 76,026 | 0.009 (0.007) | 0.031 |
| slotwise | 273 | 950,272 | 161 | 9,329 (2,044) | 69,597 | 0.009 (0.007) | 0.033 |
| bakehouse | 770 | 2,506,752 | 223 | 12,136 (2,079) | 204,402 | 0.019 (0.019) | 0.119 |
| kestrel | 592 | 1,929,216 | 210 | 10,758 (2,315) | 159,078 | 0.019 (0.015) | 0.083 |

Solved per project:

| project | engram+none | engram+summary | engram+capsule | engram+both | engram+short | engram+shortcap | engram | full |
|---|---|---|---|---|---|---|---|---|
| stockroom | 20/40 | 40/40 | 39/40 | 40/40 | 40/40 | 40/40 | 38/40 | 40/40 |
| slotwise | 16/40 | 40/40 | 39/40 | 40/40 | 28/40 | 38/40 | 40/40 | 40/40 |
| bakehouse | 29/72 | 67/72 | 61/72 | 66/72 | 34/72 | 60/72 | 68/72 | 66/72 |
| kestrel | 29/56 | 51/56 | 50/56 | 52/56 | 47/56 | 48/56 | 51/56 | 52/56 |
