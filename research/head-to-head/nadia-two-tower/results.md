# Nadia: next-movie recommendation, ChloeDB's recommender against a two-tower model

Nadia, a recommender-systems engineer in the persona study, found that `recommend()` beat her tuned co-occurrence baseline with no tuning. Her head of ML asks for "an offline comparison against our two-tower model". This page is that comparison on her protocol: MovieLens-1M (1,000,209 ratings of 3,706 movies by 6,040 users), every rating an interaction, each user's ratings in time order; the last is the test, the one before it the validation, the rest training; every movie ranked but the user's own; hit@10, NDCG@10 and MRR@10 over every user.

The two-tower model is what such a team would train first: a user tower over the user's recent movies (embeddings pooled with learned recency weights, then a two-layer MLP), an item tower (the embedding through a linear layer), in-batch softmax with the logQ correction for popularity, temperature 0.05, Adam, batches of 1,024. Its size, history length and epochs were chosen on the validation item over 4 settings (best: 128 dimensions, the last 100 movies, 8 epochs), then it was trained again on training plus validation for the test. The baselines are tuned the same way. ChloeDB's recommender has no tuning: `CREATE RECOMMENDER ml ON events(usr, item, ts) ITEMS items(emb) OPTIONS (window = 3, history = 20)` over the same events, the two-tower's item vectors in `items.emb`.

Measured on 2026-11-20 on Intel(R) Xeon(R) Processor @ 2.10GHz, 4 cores, 15 GB, Linux 6.18.44-fc-v726; chloedb-server 0.3.9 (built from 2f21f4dd0c); PyTorch on the CPU. ChloeDB is asked over HTTP, one user at a time. Run it again with `python3 run.py && python3 report.py` (needs `pip install torch numpy` and a chloedb-server build; it downloads the ratings, pinned by hash). Raw numbers: [raw/results.json](raw/results.json).

## What it says

- **Ranked by hit@10:** ChloeDB `rrf(recommend(...), similarity(emb, :v))` 0.3048; Two-tower model, tuned on validation 0.2813; The two-tower's vectors served from ChloeDB 0.2813; ChloeDB `recommend('ml', :u)` + `NOT IN`, no tuning 0.2493; Windowed co-occurrence, tuned on validation 0.2341; Popularity 0.0364.
- **ChloeDB `recommend('ml', :u)` + `NOT IN`, no tuning against Two-tower model, tuned on validation:** hit@10 -0.0320 (95% interval -0.0430 to -0.0215), worse; NDCG@10 -0.0230 (-0.0297 to -0.0167).
- **ChloeDB `recommend('ml', :u)` + `NOT IN`, no tuning against Windowed co-occurrence, tuned on validation:** hit@10 +0.0152 (95% interval +0.0063 to +0.0238), better; NDCG@10 +0.0114 (+0.0062 to +0.0165).
- **Two-tower model, tuned on validation against Windowed co-occurrence, tuned on validation:** hit@10 +0.0472 (95% interval +0.0364 to +0.0578), better; NDCG@10 +0.0344 (+0.0280 to +0.0409).
- **ChloeDB `rrf(recommend(...), similarity(emb, :v))` against Two-tower model, tuned on validation:** hit@10 +0.0235 (95% interval +0.0152 to +0.0318), better; NDCG@10 +0.0118 (+0.0077 to +0.0160).
- **ChloeDB `rrf(recommend(...), similarity(emb, :v))` against ChloeDB `recommend('ml', :u)` + `NOT IN`, no tuning:** hit@10 +0.0555 (95% interval +0.0475 to +0.0642), better; NDCG@10 +0.0349 (+0.0304 to +0.0394).
- **The two-tower's vectors served from ChloeDB against Two-tower model, tuned on validation:** hit@10 +0.0000 (95% interval +0.0000 to +0.0000), not distinguishable; NDCG@10 +0.0000 (+0.0000 to +0.0000).
- **The two-tower's number is a floor.** Its best setting was the largest one tried, so a larger model or more epochs may score higher; the grid was kept to what trains in minutes on four cores.
- **ChloeDB serves the team's own model unchanged.** With the two-tower's item vectors in a `VECTOR` column and the user's vector as a parameter, ChloeDB's answers scored exactly as the model's own numpy ranking, and fusing them with `recommend()` in one statement scored above both.
- **The cost.** The two-tower trained in 568 s on the CPU (the final run). ChloeDB took 7.7 s to write the 994,169 events and 13.7 s to count them; its server grew from 439 MB to 2,463 MB with the recommender (peak 3,230 MB). A recommendation's time is in the table below.

## Each source

| source | hit@10 | NDCG@10 | MRR@10 | p50 (client) | p95 (client) | p50 (server) |
|---|---|---|---|---|---|---|
| ChloeDB `rrf(recommend(...), similarity(emb, :v))` | 0.3048 | 0.1726 | 0.1322 | 13.2 ms | 18.5 ms | 12.1 ms |
| Two-tower model, tuned on validation (PyTorch, scored in numpy) | 0.2813 | 0.1608 | 0.1241 | – | – | – |
| The two-tower's vectors served from ChloeDB (`similarity(emb, :v)`) | 0.2813 | 0.1608 | 0.1241 | 1.9 ms | 2.5 ms | 0.8 ms |
| ChloeDB `recommend('ml', :u)` + `NOT IN`, no tuning | 0.2493 | 0.1378 | 0.1039 | 12.8 ms | 17.9 ms | 11.7 ms |
| Windowed co-occurrence, tuned on validation (numpy) | 0.2341 | 0.1264 | 0.0937 | – | – | – |
| Popularity (numpy) | 0.0364 | 0.0179 | 0.0124 | – | – | – |

## Tuning on the validation item

| two-tower: dimensions | history | epochs | validation hit@10 | training time |
|---|---|---|---|---|
| 64 | 50 | 6 | 0.2709 | 132 s |
| 128 | 50 | 6 | 0.2952 | 219 s |
| 64 | 20 | 6 | 0.2642 | 98 s |
| 128 | 100 | 8 | 0.3096 | 554 s |

| co-occurrence: window | history | decay | validation hit@10 |
|---|---|---|---|
| 3 | 20 | 0.8 | 0.2432 |
| 3 | 20 | 0.9 | 0.2192 |
| 3 | 50 | 0.8 | 0.2437 |
| 3 | 50 | 0.9 | 0.2179 |
| 5 | 20 | 0.8 | 0.2455 |
| 5 | 20 | 0.9 | 0.2157 |
| 5 | 50 | 0.8 | 0.2454 |
| 5 | 50 | 0.9 | 0.2147 |

A caveat that holds for every row: leave-last-out on MovieLens-1M trains on other users' later ratings, and ranks movies that did not exist yet at a user's last rating. The repository's recommender gate also runs a time split ([chloedb/results/sequence/gate.py](../../../chloedb/results/sequence/gate.py)); this page keeps Nadia's protocol, which is the one her team uses.
