# Memory bench: slotwise

Model haiku; 320 runs. Solved = score 1.0. Costs in USD per run; memory $ = the memory system's own model calls during the run; history $ = agent + memory system while recording the history.

| lane | runs | solved | score | control solved | conversation solved | repo solved | $/run | memory $/run | steps | wall s | memory tool calls | history $ (agent + memory) | p vs none (Fisher / perm) | p vs engram@main (Fisher / perm) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| engram | 40 | 40/40 | 1.00 | 8/8 | 24/24 | 8/8 | 0.007 | 0.002 | 7.3 | 21 | 0.4 | 0.06 + 0.03 | – / – | – / – |
| full | 40 | 40/40 | 1.00 | 8/8 | 24/24 | 8/8 | 0.005 | 0.000 | 6.0 | 16 | 0.0 | 0.04 + 0.00 | – / – | – / – |
| engram+summary | 40 | 40/40 | 1.00 | 8/8 | 24/24 | 8/8 | 0.006 | 0.000 | 7.4 | 22 | 0.0 | 0.06 + 0.03 | – / – | – / – |
| engram+both | 40 | 40/40 | 1.00 | 8/8 | 24/24 | 8/8 | 0.007 | 0.000 | 7.7 | 22 | 0.7 | 0.06 + 0.03 | – / – | – / – |
| engram+none | 40 | 16/40 | 0.64 | 8/8 | 0/24 | 8/8 | 0.005 | 0.000 | 8.4 | 20 | 0.0 | 0.06 + 0.03 | – / – | – / – |
| engram+capsule | 40 | 39/40 | 0.99 | 8/8 | 23/24 | 8/8 | 0.007 | 0.000 | 8.2 | 22 | 0.8 | 0.06 + 0.03 | – / – | – / – |
| engram+shortcap | 40 | 38/40 | 0.97 | 8/8 | 22/24 | 8/8 | 0.006 | 0.000 | 7.1 | 21 | 0.7 | 0.06 + 0.03 | – / – | – / – |
| engram+short | 40 | 28/40 | 0.79 | 8/8 | 12/24 | 8/8 | 0.005 | 0.000 | 6.8 | 21 | 0.0 | 0.06 + 0.03 | – / – | – / – |

Against the base lane (paired when the lane is a --variant):

| lane | base | solved | base solved | p (Fisher / perm) |
|---|---|---|---|---|
| engram+summary | engram | 40/40 | 40/40 | 1.000 / 1.000 |
| engram+both | engram | 40/40 | 40/40 | 1.000 / 1.000 |
| engram+none | engram | 16/40 | 40/40 | 0.000 / 0.000 |
| engram+capsule | engram | 39/40 | 40/40 | 1.000 / 1.000 |
| engram+shortcap | engram | 38/40 | 40/40 | 0.494 / 0.426 |
| engram+short | engram | 28/40 | 40/40 | 0.000 / 0.001 |

Per task (mean score):

| task | kind | engram | full | engram+summary | engram+both | engram+none | engram+capsule | engram+shortcap | engram+short |
|---|---|---|---|---|---|---|---|---|---|
| cancellation_fee | conversation | 1.00 | 1.00 | 1.00 | 1.00 | 0.42 | 1.00 | 1.00 | 1.00 |
| fully_booked | conversation | 1.00 | 1.00 | 1.00 | 1.00 | 0.50 | 0.88 | 1.00 | 0.50 |
| day_plan | conversation | 1.00 | 1.00 | 1.00 | 1.00 | 0.50 | 1.00 | 1.00 | 1.00 |
| series_booking | conversation | 1.00 | 1.00 | 1.00 | 1.00 | 0.00 | 1.00 | 1.00 | 0.00 |
| sms_reminder | conversation | 1.00 | 1.00 | 1.00 | 1.00 | 0.50 | 1.00 | 1.00 | 1.00 |
| ical_advice | conversation | 1.00 | 1.00 | 1.00 | 1.00 | 0.50 | 1.00 | 0.75 | 0.38 |
| reschedule | repo | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| free_slots | repo | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| parse_duration | control | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| merge_intervals | control | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |

p: two-sided. Fisher = Fisher's exact test on solved runs pooled over tasks; perm = permutation test of the mean score difference with lane labels shuffled within each task (scripts/ab_stats.py).
