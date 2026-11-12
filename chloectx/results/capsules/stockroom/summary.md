# Memory bench: stockroom

Model haiku; 320 runs. Solved = score 1.0. Costs in USD per run; memory $ = the memory system's own model calls during the run; history $ = agent + memory system while recording the history.

| lane | runs | solved | score | control solved | conversation solved | repo solved | $/run | memory $/run | steps | wall s | memory tool calls | history $ (agent + memory) | p vs none (Fisher / perm) | p vs engram@main (Fisher / perm) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| engram | 40 | 38/40 | 0.97 | 8/8 | 22/24 | 8/8 | 0.006 | 0.002 | 7.0 | 21 | 0.1 | 0.07 + 0.03 | – / – | – / – |
| full | 40 | 40/40 | 1.00 | 8/8 | 24/24 | 8/8 | 0.003 | 0.000 | 2.5 | 10 | 0.0 | 0.04 + 0.00 | – / – | – / – |
| engram+summary | 40 | 40/40 | 1.00 | 8/8 | 24/24 | 8/8 | 0.004 | 0.000 | 5.5 | 16 | 0.0 | 0.07 + 0.03 | – / – | – / – |
| engram+both | 40 | 40/40 | 1.00 | 8/8 | 24/24 | 8/8 | 0.005 | 0.000 | 6.7 | 18 | 0.6 | 0.07 + 0.03 | – / – | – / – |
| engram+none | 40 | 20/40 | 0.66 | 8/8 | 4/24 | 8/8 | 0.004 | 0.000 | 7.2 | 14 | 0.0 | 0.07 + 0.03 | – / – | – / – |
| engram+capsule | 40 | 39/40 | 0.99 | 8/8 | 23/24 | 8/8 | 0.005 | 0.000 | 7.4 | 16 | 0.7 | 0.07 + 0.03 | – / – | – / – |
| engram+shortcap | 40 | 40/40 | 1.00 | 8/8 | 24/24 | 8/8 | 0.005 | 0.000 | 6.5 | 16 | 0.7 | 0.07 + 0.03 | – / – | – / – |
| engram+short | 40 | 40/40 | 1.00 | 8/8 | 24/24 | 8/8 | 0.004 | 0.000 | 5.9 | 15 | 0.0 | 0.07 + 0.03 | – / – | – / – |

Against the base lane (paired when the lane is a --variant):

| lane | base | solved | base solved | p (Fisher / perm) |
|---|---|---|---|---|
| engram+summary | engram | 40/40 | 38/40 | 0.494 / 0.494 |
| engram+both | engram | 40/40 | 38/40 | 0.494 / 0.506 |
| engram+none | engram | 20/40 | 38/40 | 0.000 / 0.000 |
| engram+capsule | engram | 39/40 | 38/40 | 1.000 / 0.800 |
| engram+shortcap | engram | 40/40 | 38/40 | 0.494 / 0.490 |
| engram+short | engram | 40/40 | 38/40 | 0.494 / 0.484 |

Per task (mean score):

| task | kind | engram | full | engram+summary | engram+both | engram+none | engram+capsule | engram+shortcap | engram+short |
|---|---|---|---|---|---|---|---|---|---|
| count_sheet | conversation | 1.00 | 1.00 | 1.00 | 1.00 | 0.42 | 1.00 | 1.00 | 1.00 |
| stock_value | conversation | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| purchase_suggestions | conversation | 0.83 | 1.00 | 1.00 | 1.00 | 0.67 | 1.00 | 1.00 | 1.00 |
| scanner_import | conversation | 1.00 | 1.00 | 1.00 | 1.00 | 0.00 | 1.00 | 1.00 | 1.00 |
| negative_stock_advice | conversation | 0.88 | 1.00 | 1.00 | 1.00 | 0.38 | 0.88 | 1.00 | 1.00 |
| config_advice | conversation | 1.00 | 1.00 | 1.00 | 1.00 | 0.12 | 1.00 | 1.00 | 1.00 |
| last_receipt | repo | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| write_off | repo | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| abc_classes | control | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| parse_location | control | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |

p: two-sided. Fisher = Fisher's exact test on solved runs pooled over tasks; perm = permutation test of the mean score difference with lane labels shuffled within each task (scripts/ab_stats.py).
