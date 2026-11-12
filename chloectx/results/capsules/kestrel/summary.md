# Memory bench: kestrel

Model haiku; 448 runs. Solved = score 1.0. Costs in USD per run; memory $ = the memory system's own model calls during the run; history $ = agent + memory system while recording the history.

| lane | runs | solved | score | control solved | conversation solved | repo solved | $/run | memory $/run | steps | wall s | memory tool calls | history $ (agent + memory) | p vs none (Fisher / perm) | p vs engram@main (Fisher / perm) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| engram | 56 | 51/56 | 0.94 | 8/8 | 39/44 | 4/4 | 0.008 | 0.002 | 7.3 | 24 | 0.3 | 0.16 + 0.08 | – / – | – / – |
| full | 56 | 52/56 | 0.96 | 8/8 | 40/44 | 4/4 | 0.009 | 0.000 | 2.3 | 16 | 0.0 | 0.09 + 0.00 | – / – | – / – |
| engram+none | 56 | 29/56 | 0.66 | 8/8 | 17/44 | 4/4 | 0.005 | 0.000 | 7.4 | 18 | 0.0 | 0.16 + 0.08 | – / – | – / – |
| engram+summary | 56 | 51/56 | 0.93 | 8/8 | 39/44 | 4/4 | 0.006 | 0.000 | 5.8 | 22 | 0.0 | 0.16 + 0.08 | – / – | – / – |
| engram+both | 56 | 52/56 | 0.94 | 8/8 | 40/44 | 4/4 | 0.007 | 0.000 | 6.6 | 23 | 0.7 | 0.16 + 0.08 | – / – | – / – |
| engram+capsule | 56 | 50/56 | 0.91 | 8/8 | 38/44 | 4/4 | 0.007 | 0.000 | 8.1 | 22 | 0.8 | 0.16 + 0.08 | – / – | – / – |
| engram+shortcap | 56 | 48/56 | 0.91 | 8/8 | 36/44 | 4/4 | 0.007 | 0.000 | 7.1 | 22 | 0.7 | 0.16 + 0.08 | – / – | – / – |
| engram+short | 56 | 47/56 | 0.90 | 8/8 | 35/44 | 4/4 | 0.005 | 0.000 | 6.6 | 20 | 0.0 | 0.16 + 0.08 | – / – | – / – |

Against the base lane (paired when the lane is a --variant):

| lane | base | solved | base solved | p (Fisher / perm) |
|---|---|---|---|---|
| engram+none | engram | 29/56 | 51/56 | 0.000 / 0.000 |
| engram+summary | engram | 51/56 | 51/56 | 1.000 / 0.680 |
| engram+both | engram | 52/56 | 51/56 | 1.000 / 1.000 |
| engram+capsule | engram | 50/56 | 51/56 | 1.000 / 0.170 |
| engram+shortcap | engram | 48/56 | 51/56 | 0.557 / 0.139 |
| engram+short | engram | 47/56 | 51/56 | 0.392 / 0.053 |

Per task (mean score):

| task | kind | engram | full | engram+none | engram+summary | engram+both | engram+capsule | engram+shortcap | engram+short |
|---|---|---|---|---|---|---|---|---|---|
| free_slots | conversation | 1.00 | 1.00 | 0.12 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| can_book | conversation | 0.35 | 0.50 | 0.00 | 0.20 | 0.20 | 0.05 | 0.30 | 0.20 |
| cancellation_fee | conversation | 1.00 | 1.00 | 0.25 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| appointment_price | conversation | 1.00 | 1.00 | 0.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| reminder_text | conversation | 1.00 | 1.00 | 0.83 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| send_reminders | conversation | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 0.75 | 1.00 | 1.00 |
| invoice_number | conversation | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| invoice_export | conversation | 1.00 | 1.00 | 0.67 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| waitlist_offer | conversation | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| overbooking_advice | conversation | 0.88 | 1.00 | 0.50 | 1.00 | 1.00 | 1.00 | 1.00 | 0.88 |
| python_advice | conversation | 1.00 | 1.00 | 0.88 | 0.88 | 1.00 | 0.88 | 0.50 | 0.50 |
| upcoming | repo | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| merge_busy | control | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| parse_duration | control | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |

p: two-sided. Fisher = Fisher's exact test on solved runs pooled over tasks; perm = permutation test of the mean score difference with lane labels shuffled within each task (scripts/ab_stats.py).
