# Memory bench: bakehouse

Model haiku; 576 runs. Solved = score 1.0. Costs in USD per run; memory $ = the memory system's own model calls during the run; history $ = agent + memory system while recording the history.

| lane | runs | solved | score | control solved | inference solved | recall solved | revised solved | stale solved | where_why solved | $/run | memory $/run | steps | wall s | memory tool calls | history $ (agent + memory) | p vs none (Fisher / perm) | p vs engram@main (Fisher / perm) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| engram | 72 | 68/72 | 0.97 | 8/8 | 12/16 | 4/4 | 16/16 | 20/20 | 8/8 | 0.007 | 0.002 | 7.5 | 20 | 0.2 | 0.21 + 0.12 | – / – | – / – |
| full | 72 | 66/72 | 0.97 | 8/8 | 11/16 | 4/4 | 16/16 | 19/20 | 8/8 | 0.022 | 0.000 | 4.4 | 10 | 0.0 | 0.29 + 0.00 | – / – | – / – |
| engram+both | 72 | 66/72 | 0.97 | 8/8 | 12/16 | 4/4 | 16/16 | 18/20 | 8/8 | 0.006 | 0.000 | 6.7 | 17 | 0.5 | 0.21 + 0.12 | – / – | – / – |
| engram+summary | 72 | 67/72 | 0.98 | 8/8 | 12/16 | 4/4 | 16/16 | 19/20 | 8/8 | 0.004 | 0.000 | 6.1 | 16 | 0.0 | 0.21 + 0.12 | – / – | – / – |
| engram+capsule | 72 | 61/72 | 0.90 | 8/8 | 11/16 | 4/4 | 16/16 | 17/20 | 5/8 | 0.006 | 0.000 | 8.7 | 18 | 0.7 | 0.21 + 0.12 | – / – | – / – |
| engram+none | 72 | 29/72 | 0.60 | 8/8 | 0/16 | 0/4 | 1/16 | 20/20 | 0/8 | 0.004 | 0.000 | 7.1 | 14 | 0.0 | 0.21 + 0.12 | – / – | – / – |
| engram+short | 72 | 34/72 | 0.69 | 8/8 | 0/16 | 0/4 | 9/16 | 17/20 | 0/8 | 0.004 | 0.000 | 6.7 | 15 | 0.0 | 0.21 + 0.12 | – / – | – / – |
| engram+shortcap | 72 | 60/72 | 0.90 | 8/8 | 12/16 | 4/4 | 16/16 | 13/20 | 7/8 | 0.006 | 0.000 | 7.9 | 18 | 0.8 | 0.21 + 0.12 | – / – | – / – |

Against the base lane (paired when the lane is a --variant):

| lane | base | solved | base solved | p (Fisher / perm) |
|---|---|---|---|---|
| engram+both | engram | 66/72 | 68/72 | 0.745 / 1.000 |
| engram+summary | engram | 67/72 | 68/72 | 1.000 / 0.517 |
| engram+capsule | engram | 61/72 | 68/72 | 0.099 / 0.000 |
| engram+none | engram | 29/72 | 68/72 | 0.000 / 0.000 |
| engram+short | engram | 34/72 | 68/72 | 0.000 / 0.000 |
| engram+shortcap | engram | 60/72 | 68/72 | 0.061 / 0.001 |

Per task (mean score):

| task | kind | engram | full | engram+both | engram+summary | engram+capsule | engram+none | engram+short | engram+shortcap |
|---|---|---|---|---|---|---|---|---|---|
| delivery_charge | revised | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 0.00 | 1.00 | 1.00 |
| price_rise | revised | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 0.00 | 1.00 | 1.00 |
| bake_sheet | revised | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 0.75 | 0.75 | 1.00 |
| crate_labels | revised | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 0.60 | 0.35 | 1.00 |
| extend_cutoff | stale | 1.00 | 0.88 | 1.00 | 1.00 | 1.00 | 1.00 | 0.62 | 0.62 |
| next_delivery_day | stale | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| relief_baker_note | stale | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| cutoff_quick | stale | 1.00 | 1.00 | 0.75 | 0.88 | 0.62 | 1.00 | 1.00 | 0.50 |
| closed_day_quick | stale | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| owner_weekly_file | inference | 1.00 | 1.00 | 1.00 | 1.00 | 0.83 | 0.33 | 0.33 | 1.00 |
| vat_export | inference | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 0.58 | 0.58 | 1.00 |
| van_list | inference | 0.44 | 0.75 | 0.75 | 0.75 | 0.12 | 0.12 | 0.25 | 0.12 |
| school_order | inference | 1.00 | 0.92 | 1.00 | 1.00 | 1.00 | 0.33 | 0.58 | 1.00 |
| returns_credit | recall | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 0.00 | 0.00 | 1.00 |
| pence_why | where_why | 1.00 | 1.00 | 1.00 | 1.00 | 0.75 | 0.50 | 0.50 | 1.00 |
| unknown_code_why | where_why | 1.00 | 1.00 | 1.00 | 1.00 | 0.88 | 0.50 | 0.50 | 0.88 |
| easter_sunday | control | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| baker_percentages | control | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |

p: two-sided. Fisher = Fisher's exact test on solved runs pooled over tasks; perm = permutation test of the mean score difference with lane labels shuffled within each task (scripts/ab_stats.py).
