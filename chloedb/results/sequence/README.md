# Next items from a history: `encode()` on MovieLens-1M and Amazon Beauty

`eval.py` trains a sequence model with `strata.train.sequence` (two layers
of causal attention, width 64, item vectors of 64 learned with it), loads
the items and every event into a Strata database, and asks Strata for each
user's next 10 items with one query:

```sql
SELECT id FROM items
WHERE NOT id IN (SELECT item FROM events WHERE usr = :u AND seq < :n)
ORDER BY similarity(emb, encode('m', (SELECT item, ts FROM events
    WHERE usr = :u AND seq < :n ORDER BY seq DESC LIMIT 200), now => :t)) DESC
LIMIT 10
```

(`LIMIT 50` on Beauty; without `ts` and `now =>` when the model reads no
times.)

The protocol is the one published SASRec numbers use: leave-one-out (each
user's last event is the test, the one before it validates, the rest train),
ranked against every item, the user's own items left out. Recall@10 (equal
to HR@10 with one test item) and NDCG@10 are over every user. Each run also
ranks with PyTorch's vectors of the same model, and checks that the first
300 users' Strata answers equal those of the same query given PyTorch's
vector.

Measured on a cloud VM (Intel Xeon @ 2.80GHz, 4 cores, 16.5 GB, Linux 6.18,
PyTorch 2.15 on the CPU), 2026-10-30 and 31. Scores do not depend on the
machine; times do.

## Results

| data | the model reads | Recall@10 | NDCG@10 | PyTorch's | query p50 / p99 | given the vector, p50 | training |
|---|---|---|---|---|---|---|---|
| MovieLens-1M (6,040 users, 3,706 items, 1,000,209 ratings) | items and times | 0.3190 | 0.1877 | equal | 3.14 / 9.86 ms | 0.82 ms | 93 epochs, stopped at 95 min |
| | items only | **0.2783** | **0.1585** | equal | 2.07 / 6.09 ms | 0.68 ms | 94 epochs, stopped at 95 min |
| Amazon Beauty, 5-core (22,363 users, 12,101 items, 198,502 reviews) | items and times | 0.0771 | 0.0473 | equal | 0.73 / 1.94 ms | 0.44 ms | 100 epochs, 55 min |
| | items only | **0.0662** | **0.0406** | equal | 0.62 / 1.50 ms | 0.43 ms | 96 epochs (no gain for 10), 53 min |

Published, same protocol:

| data | model | Recall@10 | NDCG@10 | source |
|---|---|---|---|---|
| MovieLens-1M | SASRec | 0.2500 | 0.1341 | Klenitskiy & Vasilev, RecSys 2023 (arXiv 2309.07602), Table 2 |
| | BERT4Rec | 0.2843 | 0.1537 | same |
| | SASRec+ (SASRec trained with cross-entropy over every item) | 0.3152 | 0.1821 | same |
| Amazon Beauty, 5-core | SASRec | 0.0605 | 0.0318 | TIGER (arXiv 2305.05065), Table 1 |
| | counted transitions (`strata.next_items`, counted from the items before the last two) | 0.0613 | 0.0398 | [`ngrams-beauty.json`](ngrams-beauty.json), on this data |
| | counted transitions, counted from every item before the last | 0.0694 | 0.0450 | same |

The parameters of each run (max length 200 on MovieLens, 50 on Beauty;
every item a negative, temperature 0.05, learning rate 1e-3, dropout 0.2,
batch 128) and every epoch's loss and validation scores are in
[`ml-1m.json`](ml-1m.json), [`ml-1m-no-time.json`](ml-1m-no-time.json),
[`beauty.json`](beauty.json) and [`beauty-no-time.json`](beauty-no-time.json).
`beauty.json` was written before `--no-time` and `--max-minutes` existed;
it ran as their defaults are (with times, no time limit).

## What it says

- **Strata ranks what the model ranks.** On every run Strata's Recall@10
  and NDCG@10 equal those of PyTorch's own vectors, and 300 of 300 users get
  the same top 10 as the query given PyTorch's vector.
- **The like-for-like rows are "items only"**: SASRec reads no times. There
  the model is above published SASRec on both collections (by 0.028
  Recall@10 and 0.024 NDCG@10 on MovieLens, 0.006 and 0.009 on Beauty),
  next to BERT4Rec on MovieLens, and below SASRec+ there (by 0.037
  Recall@10), the same architecture trained with the same loss. On Beauty
  it is 0.005 Recall@10 above counted transitions that leave out the
  validation item, and below them (by 0.003 Recall@10 and 0.004 NDCG@10)
  counted from every item before the last, which is what the model reads
  at test time: on this data a table of counts does as well. Both MovieLens runs were stopped by the 95-minute limit with
  the validation score still rising (best epoch 91 of 93, and 94 of 94), so
  they are a floor of this model's score, not its best.
- **Times help** (0.041 Recall@10 on MovieLens, 0.011 on Beauty): with
  them the model reads the time from each event to the next, and from the
  newest to the request's `now` (here the test event's time). On Beauty
  part of that is the data's: 43% of users review their last two items on
  the same day, and the reviews of one day are in the file's order, by
  product, which counted transitions also pick up.
- **The query costs** the model plus reading the history and the vector
  search: 0.62 to 0.73 ms at p50 on Beauty's 50 events, of which 0.43 to
  0.44 ms is the same query given the vector, and 2.1 to 3.1 ms on MovieLens's 200.

## The gate in CI

[`gate.py`](gate.py) runs every candidate source Strata has, and two
baselines, on MovieLens-1M in one harness and checks them against
[`strata/tests/fixtures/recsys_gate.json`](../../tests/fixtures/recsys_gate.json)
in CI (the Python job, on Linux). The protocol is the one above, over all
6,040 users, each source answering a top 100 without the user's own items;
coverage is the share of the 3,706 items in some user's top 10. The model,
[`strata/tests/fixtures/recsys/ml1m`](../../tests/fixtures/recsys/ml1m), is
trained as `eval.py` trains the MovieLens model with times (`gate.py
--train 60`: 70 epochs in an hour, still improving), and its item vectors
(`items.safetensors`) are the field's. Measured on the cloud container
(Intel Xeon @ 2.10GHz, 4 cores), 2026-10-31; the scores do not depend on the
machine and a re-run gives the same to every digit.

| source | Recall@10 | @50 | @100 | NDCG@10 | coverage@10 | query p50 |
|---|---|---|---|---|---|---|
| popularity (baseline, Python) | 0.0364 | 0.1397 | 0.2260 | 0.0179 | 0.054 | |
| item-kNN, 100 neighbours (baseline, Python) | 0.0692 | 0.1987 | 0.3118 | 0.0354 | 0.139 | |
| `encode()`, the user's last 200 events with times | **0.3060** | **0.5596** | **0.6677** | **0.1804** | 0.626 | 4.2 ms |
| `ppr()` over what came after what, seeded by the last 5 items | 0.2417 | 0.4675 | 0.5684 | 0.1383 | 0.528 | 37 ms |
| `strata.next_items`, counts after the last two items | 0.2108 | 0.3959 | 0.4788 | 0.1211 | **0.757** | 4.8 ms |
| `maxsim()` to the last 20 items' vectors | 0.0641 | 0.2070 | 0.3222 | 0.0317 | 0.481 | 2.4 ms |

The `ppr()` row predates release 0.2.0, whose push test counts a row's edges instead of summing their weights. On CI's runner (ubuntu-latest, 12 November 2026) `ppr()` measured Recall@10 0.2414, @50 0.4627, @100 0.5772, NDCG@10 0.1378 and coverage@10 0.475 here, the numbers in `chloedb/tests/fixtures/recsys_gate.json`. The other rows did not move beyond the gate's tolerance.

On this collection the sequence model ranks best, then PageRank over the
transitions and the counts, all three far above the baselines; `maxsim`
over the learned item vectors is below item-kNN (those vectors are trained
to be found by the model's vector, not to be near each other). `ppr` needs
`eps => 1e-6`: at its default, 1e-4, the walk hardly leaves the seeds of a
graph this dense (1.6 million edges over 3,706 items) and its Recall@10 is
0.005. A change that moves a number by more than its tolerance (0.002, and
0.005 for coverage) or that stops a source beating both baselines fails CI
until the fixture is rewritten (`STRATA_RECSYS_GATE=update python gate.py
--ratings ...`) and the pull request says why. Semantic IDs' codebooks
are in the gate ([below](#semantic-ids-in-the-gate)); a decoder over them
(generative retrieval) and a GBDT reranker are not yet.

### A global time split

Leave-one-out leaks the future: the sources learn from other users'
ratings made after a user's test rating, and rank items that did not exist
yet at its time, which every model Ji et al. tested exploits ("A Critical
Study on Data Leakage in Recommender System Offline Evaluation", TOIS 2023).
So the gate also runs a global time split
([`strata/tests/fixtures/recsys_gate_time.json`](../../tests/fixtures/recsys_gate_time.json)):
one cutoff for every user, the time by which 90% of the ratings were made
(2000-12-29 23:43:34 UTC). The sources learn from the ratings before it
only; the candidates are the 3,678 items rated before it, without the
user's own; and each of the 1,178 users with ratings on both sides is
asked for their first rating after it, given everything before (2 more
users, whose next item was new, are left out). Its model,
[`strata/tests/fixtures/recsys/ml1m-time`](../../tests/fixtures/recsys/ml1m-time),
is trained the same way on the ratings before the cutoff only (`gate.py
--protocol time --train 300 --epochs 70`: 70 epochs, 2 hours 35 minutes
on the container while other work ran; the best epoch, 69, kept). Measured on
the cloud container (Intel Xeon @ 2.10GHz, 4 cores), 2026-10-31, and a
second run gave the same to every digit:

| source | Recall@10 | @50 | @100 | NDCG@10 | coverage@10 | query p50 |
|---|---|---|---|---|---|---|
| popularity (baseline, Python) | 0.0586 | 0.1944 | 0.2869 | 0.0301 | 0.040 | |
| item-kNN, 100 neighbours (baseline, Python) | 0.0484 | 0.1324 | 0.1935 | 0.0238 | 0.083 | |
| `encode()`, the user's last 200 events with times | **0.1579** | **0.3447** | **0.4457** | **0.0850** | 0.204 | 4.0 ms |
| `ppr()` over what came after what, seeded by the last 5 items | 0.0594 | 0.1511 | 0.2148 | 0.0293 | 0.362 | 59 ms |
| `strata.next_items`, counts after the last two items | 0.0390 | 0.1231 | 0.1834 | 0.0198 | **0.550** | 3.9 ms |
| `maxsim()` to the last 20 items' vectors | 0.0238 | 0.1095 | 0.1672 | 0.0100 | 0.355 | 2.7 ms |

As above, the `ppr()` row predates 0.2.0's push test. On CI's runner (12 November 2026) it measured Recall@10 0.0620, @50 0.1689, @100 0.2623, NDCG@10 0.0335 and coverage@10 0.311 (`recsys_gate_time.json`), which beats both baselines.

Every number is lower than under leave-one-out, as the leak predicts, and
the order changes: the sequence model still ranks first, at 2.7 times
popularity's Recall@10, but PageRank over the transitions now ties
popularity (one user in 1,178 apart) and the counts fall below it.
Popularity is the stronger baseline here and item-kNN the weaker, the
reverse of leave-one-out. The counts
and PageRank learned which item follows which from the same ratings in
both protocols; under leave-one-out much of that was the test users' own
future. A source is held to beating both baselines only when its
Recall@10 does by more than the tolerance (`encode` here; `encode`,
`ppr` and `next_items` under leave-one-out), so a tie does not fail the
gate on one user. `python gate.py` checks both protocols, `--protocol
loo` or `--protocol time` one.

### Semantic IDs in the gate

Each protocol also trains two codebooks of 3 levels of 256 on its model's
item vectors (`strata.train.codebook`, seed 0), one as it trains by
default and one with `balanced=True`, writes the items to a table with a
semantic-ID field for each, and checks two things. The codes the table
gave the items, counted by `sid_at` and `sid_next`, must equal what
`strata.train.codebook_report` says of the same vectors, exactly. And the
report's numbers must be within a tolerance of the fixture's
(`semantic_ids` in each): the distinct codes, the items that share a code
with another, each level's perplexity (how many of its 256 numbers the
items are spread over, weighed by use), the share of the vectors' squared
length left after the third level, and how often a user's test item has
the same first number as their last item (by chance, 1 in 250 or so).
Measured on a cloud container (Intel Xeon @ 2.30GHz, 4 cores, 16.5 GB),
2026-11-12:

| codebook | distinct codes | items sharing a code | perplexity, levels 1 / 2 / 3 | left after level 3 | next item under the same first number |
|---|---|---|---|---|---|
| leave-one-out (3,706 items), plain | 3,666 | 79 | 223.3 / 59.1 / 40.6 | 0.3883 | 0.0801 |
| leave-one-out, balanced | 3,704 | 4 | 254.1 / 253.6 / 252.7 | 0.3378 | 0.0851 |
| time split (3,678 items), plain | 3,659 | 37 | 222.6 / 150.9 / 47.0 | 0.3661 | 0.0314 |
| time split, balanced | 3,677 | 2 | 253.1 / 253.5 / 252.3 | 0.3393 | 0.0263 |

- **Plain k-means wastes the later levels.** What a level leaves of the
  vectors points every way alike, and there k-means keeps most centroids
  on one far item each and gives the rest to a few centroids near the
  middle: at seed 0, 189 of level 2's 256 numbers have one item each. So
  the plain codebook's later levels swing with the seed. Over seeds 0 to 4
  (`--sid-seed`) on leave-one-out, level 2's perplexity was 59 to 158,
  level 3's 41 to 132, the items sharing a code 10 to 79 and what is left
  0.344 to 0.388.
- **Balanced training uses every number alike, and is stable.** Over the
  same seeds its perplexities stayed between 251.9 and 254.1, the items
  sharing a code between 2 and 7, and what is left between 0.337 and
  0.339, below the plain codebook's at every seed.
- **Neither puts the next item under the same number more often** (0.080
  to 0.100 plain, 0.085 to 0.093 balanced, over the seeds): balancing
  spreads the codes, it does not make them say more about what comes next.
- **What the gate holds.** K-means follows the last bit of a sum, so
  another processor's kernels end at other centroids, as another seed
  does. The fixture therefore holds the plain codebook only to its
  distinct codes (within 60), its first level's perplexity (12), what is
  left (0.06) and the next-item share (0.03), and the balanced one to all
  of them (15 codes, 15 shared items, 6 for each level's perplexity, 0.01
  left), about twice the spread over the five seeds. It also fails when the
  balanced codebook leaves more items sharing a code than the plain one.

## The model alone

`strata-encode-bench` (`strata/src/bin/strata-encode-bench.rs`) runs a
model over 2,000 random histories of each length on one thread
([`encode-bench.txt`](encode-bench.txt)), on the MovieLens model:

| events | p50 | p99 |
|---|---|---|
| 1 | 0.014 ms | 0.049 ms |
| 10 | 0.091 ms | 0.20 ms |
| 50 | 0.28 ms | 0.50 ms |
| 100 | 0.62 ms | 1.04 ms |
| 200 | 1.48 ms | 2.23 ms |

Fifty events through the two layers take 0.28 ms; the cost grows a little
faster than the length, as attention does.

## In-batch negatives, corrected

The runs above train with every item as a negative, which a catalog of
millions cannot. With the other targets of the batch as the negatives
(`negatives="in_batch"`), an item is a negative as often as it is a
target, so the model learns to rank what is popular low, and an item that
is seldom a target is almost never a negative. `strata.train.sequence` has
two options for that: `logq=True` takes from each negative's score the log
of the chance that the batch holds it, and `uniform_negatives=n` adds `n`
items drawn evenly to every batch.

[`negatives.py`](negatives.py) trains the model above on Beauty (12,101
items, items only, no times, batch 128, 2 threads) each way for the same
15 minutes, and scores each user's last item against every item
([`negatives/`](negatives)). Measured on a cloud container (Intel Xeon @
2.30GHz, 4 cores, 16.5 GB), 2026-11-12, two runs at a time:

| negatives | epochs in 15 min | Recall@10 | NDCG@10 |
|---|---|---|---|
| in-batch | 48 | 0.0383 | 0.0224 |
| in-batch, `logq` | 45 | 0.0550 | 0.0333 |
| in-batch, `logq`, 2,048 uniform | 44 | **0.0588** | **0.0336** |
| every item (`"all"`) | 28 | 0.0533 | 0.0287 |
| in-batch, `logq`, 2,048 uniform, trained out (38 min, best epoch 92 of 102) | 102 | 0.0631 | 0.0394 |

- **The correction is most of it**: Recall@10 0.0383 to 0.0550 in the same
  time, and 0.0588 with uniform negatives too. At 15 minutes both are
  above training against every item, which had run 28 epochs to their 44;
  none of the four had stopped improving.
- **Trained out, it comes near the full softmax, not past it**: 0.0631
  and 0.0394, against 0.0662 and 0.0406 for the "items only" row above
  (every item a negative, `eval.py`, 2026-10-31). Use `"all"` while the
  catalog fits, and these two options when it does not.
- **On a small catalog the correction alone hurts.** On MovieLens-1M
  (3,706 items, 200 events a user, 20 minutes each) plain in-batch had
  Recall@10 0.1141 and with `logq` 0.0826: a batch there holds most
  items many times over, and the correction for it is large and
  noisy. Both are far below the 0.278 of training against every item,
  which is what a catalog this size should use.
- On Yambda-50M's 629,298 tracks the two options took held-out Recall@10
  from 0.0229 to 0.0334 after four epochs
  ([`results/recsys`](../recsys/README.md#in-batch-negatives-corrected)).

## New items

A sequence model with learned item vectors cannot answer an item it never
trained on. [`cold.py`](cold.py) measures what can, on Beauty: a tenth of
the 12,101 items, drawn at random, are new, with every event of theirs
taken out of training, as if they were listed after the models were
trained. A user's last item is the test, as above, ranked against every
item but the user's own, and the users are split by whether that item is
old (19,875 users) or new (2,488).

The sources are the model above with learned vectors, the same model
reading and ranking the items' text vectors (bge-small-en-v1.5,
`learn_items=False`), and the decoder of
[`results/generative`](../generative/README.md) over semantic IDs. The
codebook is trained on the old items only (`chloedb.train.codebook`,
3 levels of 256) and the new items get their codes when they are written
to the table: 363 of the 1,210 share all three numbers with an old item
and 943 the first two. Measured on a cloud container (Intel Xeon @
2.30GHz, 4 cores, 16.5 GB), 2026-11-12 ([`cold/`](cold)); the two
sequence models trained side by side for 45 minutes each on 2 threads,
the decoder after them on 4 (it stopped at epoch 20, after 37 minutes):

| what ranks the items | Recall@10, old | Recall@10, new | Recall@10, all | NDCG@10, all |
|---|---|---|---|---|
| learned item vectors | 0.0577 | 0 | 0.0513 | 0.0305 |
| text vectors | 0.0427 | **0.0394** | 0.0423 | 0.0224 |
| decoder over semantic IDs | 0.0646 | 0 | 0.0574 | 0.0315 |
| `rrf` of learned and text | 0.0649 | 0.0032 | 0.0580 | 0.0326 |
| `rrf` of the three | 0.0754 | 0.0012 | 0.0672 | 0.0369 |
| decoder, 1 of 10 places to new items under a decoded item's three numbers | 0.0625 | 0.0080 | 0.0565 | 0.0312 |
| decoder, 5 of 10 places to new items under a decoded item's first two numbers | 0.0460 | 0.0470 | 0.0461 | 0.0279 |
| ranker over learned and text's top 100, their ranks only | 0.0587 | 0.0032 | 0.0525 | 0.0313 |
| ranker, with semantic-ID prefix features | 0.0712 | 0.0189 | 0.0654 | 0.0386 |
| ranker, with text-vector similarity to the history | 0.0785 | 0.0281 | 0.0729 | 0.0448 |
| ranker, with both | 0.0776 | 0.0297 | 0.0723 | 0.0445 |
| ranker, with both, and the decoder's top 20 and its rank | **0.0868** | 0.0273 | **0.0802** | **0.0481** |

The rankers are LightGBM (lambdarank, 200 trees), trained on the test
items of half the users and scored on the other half, both ways. The
prefix features say how many of the user's last 20 items, and whether the
last one, share the candidate's first one, two and three numbers; the
similarity features are the candidate's greatest cosine to those 20
items' text vectors and its cosine to the last one's.

- **The decoder does not find new items.** Of its 223,630 places in the
  users' top 10s, 11 went to a new item, and none of those was the test
  item. It learned which sequences of numbers follow a history, and a
  new item's sequence was never a target, tie number included. Handing
  places to unseen items under a decoded prefix, as TIGER does, gets
  some back and gives up about as many hits on old items as it wins on
  new ones.
- **`rrf` buries new items.** Two of its three sources can never rank
  one, so the one that can is outvoted: 0.0012 on new items for the best
  plain fusion on old ones.
- **A ranker over the sources' candidates is the answer here**: 0.0802 on
  all users against 0.0513 for learned vectors alone and 0.0672 for
  `rrf` of the three, with new items found at 0.0273, about two thirds
  of what the text-vector model alone finds, and old items above every
  single source.
- **Semantic-ID prefixes work as ranker features, less well than the
  vectors they were cut from.** They take the ranker from 0.0525 to
  0.0654, the vectors' similarity takes it to 0.0729, and the two
  together are no better than the similarity alone. Prefixes are what
  to use where the vectors are not at hand; they are keywords, so
  `sid_at` filters, groups and joins on them.

One split and one seed, and the rankers learn from test items of other
users, not from an earlier period, so read the order of the rows and not
their third digit.

## Running it

```bash
# MovieLens-1M: https://files.grouplens.org/datasets/movielens/ml-1m.zip (ratings.dat)
# Beauty: http://snap.stanford.edu/data/amazon/productGraph/categoryFiles/reviews_Beauty_5.json.gz
pip install torch && (cd strata-py && maturin develop --release)
python eval.py --data ml-1m --dir ~/data/seq/ml-1m --out . --max-minutes 95
python eval.py --data ml-1m --dir ~/data/seq/ml-1m --out . --max-minutes 95 --no-time --tag=-no-time
python eval.py --data beauty --dir ~/data/seq/beauty --out . --max-len 50
python eval.py --data beauty --dir ~/data/seq/beauty --out . --max-len 50 --no-time --tag=-no-time
cargo run --release --bin strata-encode-bench -- <work>/models/ml1m
for stage in embed id text decoder rank; do python cold.py $stage --dir ~/data/seq/beauty --work <work>; done   # needs meta_Beauty.json.gz too, sentence-transformers and lightgbm
python gate.py --ratings ~/data/seq/ml-1m/ratings.dat       # the CI gate, both protocols (--protocol time --train 300 --epochs 70 retrains the time split's model)
```
