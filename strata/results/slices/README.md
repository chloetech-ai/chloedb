# On-device slices, Phase 0: how much a device could answer alone

The replay study of the "Strata on the device" design (Claude Doc,
https://claude.ai/artifact/14s2MaZu2wua3Q3NNvbL2E), run before building
anything. A slice is part of the corpus shipped to a device, scored there
with the cloud's statistics, plus enough to know when the device's answer
is the cloud's. The question is how much traffic a slice of a given size
answers alone, for search (ORCAS queries over MS MARCO) and for
recommendations (Yambda-50M).

Everything ran in one cloud container (Intel Xeon @ 2.30GHz, 4 cores,
15 GB, Linux 6.18) on 2026-11-07. The numbers are shares of queries and
users and file sizes, which do not depend on the machine; no time here is a
benchmark.

The gate the design proposed (Gate A): 40% of query volume answered on the
device with a proof at a slice of 50 MB or less, and recommendations made on
the device within 5% of the cloud's NDCG@10.

## Verdict

- **Search fails Gate A, and not narrowly.** At about 33 MB (100,000
  passages), the proof answers 0.2% to 0.4% of queries' top 10 and 0.6% to
  1.3% of their top 1; with pair bounds, 0.5% to 1.2% and 2.5% to 4.1%. Even
  a perfect bound could not reach 40%: the slice holds the whole cloud top
  10 of only 5% of new queries and 8% of repeats. The proof itself is sound
  (not one wrong answer, at any size, k or kind of bound), but on a web query log
  over a web corpus a slice small enough for a phone rarely holds the
  answer, and the sum of per-word bounds is far looser than the slice.
- **Recommendations pass.** Ranking only the user's own tracks, a 1,000
  track base pack and the meaning packs nearest the user's taste (5,000
  tracks, 1.9 MB), the device's NDCG@10 is 0.0828 against the same model's
  0.0827 over all 629,298 tracks in the cloud, and 0.0235 against 0.0236 on
  tracks new to the user. The user's own history, which needs no download,
  is still the best single source on this data (0.1854), above every cloud
  source measured in [`../recsys`](../recsys/README.md).
- **Freshness is cheap for recommendations.** A base pack of the 10,000
  most played tracks of the last week gains 591 tracks a day on average,
  about 217 KB a day at 367 bytes a track.

What this does not settle: ORCAS has no query counts, so "query volume"
could not be measured, only shares of distinct strings. A head-heavy log
(an app's own search, a shop's catalogue) would be needed before giving up
on search slices for that kind of product, and a result cache, which needs
no proof, is the right tool for repeats (below).

## Search: ORCAS queries over MS MARCO passages

[`search.py`](search.py) (its docstring has the details). The corpus is
MS MARCO's 8,841,823 passages in one Strata database, which plays the
cloud. The queries are ORCAS's 10.4 million Bing queries, normalised the way
Strata's analyser splits words and with duplicate strings removed
([`search/prep.json`](search/prep.json)): 1,000,000 of them choose the
slices, and 50,000 others are the new queries. The "right answer" is the
cloud's BM25 top 10 for the query (ORCAS's clicks are on MS MARCO's
documents, not its passages, so they are not used), and only queries with a
full top 10 count: 47,191 new queries and a sample of 50,000 training
queries, the repeats.

ORCAS has no query counts. Every query weighs 1, and train and test share
no string, so a result cache of the training queries answers none of the new
queries by construction. Real traffic repeats far more; the new and repeat
columns below are the two ends, and a log with counts would weigh them.

Slices are nested, 10,000 to 400,000 passages, chosen two ways from the
training queries' cloud top 10: `cov` greedily completes the training query
that misses the fewest passages, and `pop` takes passages by how many
training queries' top 10 hold them. Each slice ships, for each word it
holds, its residual bound: the most that word adds to the score of any
passage left in the cloud. The device answers alone when the k-th local
score reaches the sum of the query's bounds (the proof); "held" is the share
whose cloud top k is wholly in the slice, which a perfect bound would prove.
A word the slice has no posting for sends the query to the cloud. The file
sizes are of real Strata files of the slice's passages (`id`, `body`),
optimised and vacuumed ([`search/bytes.json`](search/bytes.json)).

Query coverage (`cov`):

<!-- report:slices-cov -->
| passages | file MB | bytes / passage | new: top 1 held | new: top 1 proved | new: top 10 held | new: top 10 proved | new: top 10 proved, 8-bit bounds | repeat: top 10 held | repeat: top 10 proved | new: a word the slice lacks |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 10K | 3.4 | 341 | 2.1% | 0.08% | 0.6% | 0.01% | 0.01% | 0.9% | 0.05% | 39.4% |
| 30K | 10.2 | 339 | 5.8% | 0.24% | 2.0% | 0.04% | 0.03% | 3.1% | 0.18% | 27.1% |
| 100K | 32.9 | 329 | 12.9% | 0.57% | 5.0% | 0.17% | 0.11% | 7.9% | 0.37% | 17.9% |
| 200K | 65.0 | 325 | 19.3% | 0.88% | 7.9% | 0.31% | 0.19% | 13.2% | 0.54% | 13.7% |
| 400K | 127.9 | 320 | 29.6% | 1.45% | 12.8% | 0.55% | 0.38% | 22.5% | 0.87% | 9.8% |
<!-- /report -->

Popular passages (`pop`):

<!-- report:slices-pop -->
| passages | file MB | bytes / passage | new: top 1 held | new: top 1 proved | new: top 10 held | new: top 10 proved | new: top 10 proved, 8-bit bounds | repeat: top 10 held | repeat: top 10 proved | new: a word the slice lacks |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 10K | 3.3 | 332 | 8.3% | 0.32% | 1.5% | 0.06% | 0.02% | 1.6% | 0.08% | 36.0% |
| 30K | 9.6 | 321 | 14.4% | 0.57% | 2.1% | 0.13% | 0.03% | 2.2% | 0.14% | 23.3% |
| 100K | 31.7 | 317 | 25.7% | 1.19% | 3.5% | 0.27% | 0.11% | 4.0% | 0.32% | 13.6% |
| 200K | 63.4 | 317 | 35.4% | 1.75% | 5.4% | 0.46% | 0.18% | 6.4% | 0.54% | 10.0% |
| 400K | 126.7 | 317 | 46.6% | 2.47% | 8.4% | 0.69% | 0.31% | 10.8% | 0.88% | 7.4% |
<!-- /report -->

No proof was wrong: each proved answer was the cloud's top k (or differed
only among passages with the same score), for all 97,191 queries, at every
slice size, for k = 1, 3, 5 and 10, with word bounds and with the pair
bounds below ([`search/eval_test.json`](search/eval_test.json),
[`search/eval_seen.json`](search/eval_seen.json)). A proof can hold where
the slice lacks one of the cloud's passages, when the two tie. Rounding the
bounds up to 8 bits, as a slice would ship them, loses 25% to 40% of the top
10 proofs of `cov` slices and 55% to 75% of `pop` slices'.

When the slice cannot prove its answer, its top k still shares this much
with the cloud's (new queries, the mean share of the cloud's top k found in
the local top k):

<!-- report:overlap -->
| passages | cov: top 1 | cov: top 10 | pop: top 1 | pop: top 10 |
|---|---:|---:|---:|---:|
| 10K | 2% | 2% | 8% | 7% |
| 30K | 6% | 5% | 14% | 12% |
| 100K | 12% | 11% | 25% | 22% |
| 200K | 19% | 17% | 34% | 30% |
| 400K | 29% | 26% | 45% | 40% |
<!-- /report -->

### Why the proof answers so little

A query's bound is the sum of its words' bounds, and each word's bound
comes from a different passage: over 8.4 million passages left in the cloud,
almost every word has one where it scores near its ceiling. Of the words a
100,000-passage `cov` slice knows, 90% keep their single best passage in
the cloud, so their bound is their top score anywhere, and 97% have a bound
of at least 0.9 of it (60% and 96% for `pop`;
[`search/residuals.json`](search/residuals.json)). For one-word
queries the bound is exact and the proof answers every query whose answer
the slice holds; from two words on, the local top k would have to score
near every word's ceiling at once. ORCAS queries are long: 3.5% have one
word (after query stopwords), 32% two, 37% three and 27% four or more.

Two ways past it were measured on the `cov` slices
([`search/eval2_test.json`](search/eval2_test.json),
[`search/eval2_seen.json`](search/eval2_seen.json)):

- **Pair bounds.** For two words that appear together in some training
  query (1,540,914 pairs), the slice can also ship G(a, b), the most any
  cloud-only passage scores on the two together. A query's bound is then the
  sum over a greedy pairing of its words, which is still a valid bound.
- **A confidence rule** instead of a proof: answer on the device when the
  k-th local score reaches a share alpha of the (pair) bound, and count how
  often that answer is the cloud's.

By the number of words (k = 10):

<!-- report:lengths -->
| queries | passages | words | share of queries | top 10 held | proved, word bounds | proved, pair bounds |
|---|---:|---:|---:|---:|---:|---:|
| new | 100K | 1 | 3.5% | 1.8% | 1.75% | 1.75% |
| new | 100K | 2 | 32.4% | 3.7% | 0.27% | 0.98% |
| new | 100K | 3 | 37.0% | 4.9% | 0.05% | 0.29% |
| new | 100K | 4+ | 27.0% | 7.0% | 0.00% | 0.03% |
| new | 400K | 1 | 3.5% | 7.6% | 7.62% | 7.62% |
| new | 400K | 2 | 32.4% | 9.0% | 0.78% | 3.72% |
| new | 400K | 3 | 37.0% | 12.4% | 0.08% | 0.64% |
| new | 400K | 4+ | 27.0% | 18.4% | 0.00% | 0.09% |
| repeat | 100K | 1 | 3.4% | 2.1% | 2.09% | 2.09% |
| repeat | 100K | 2 | 33.1% | 5.1% | 0.79% | 2.74% |
| repeat | 100K | 3 | 36.9% | 7.6% | 0.11% | 0.51% |
| repeat | 100K | 4+ | 26.6% | 12.8% | 0.00% | 0.04% |
| repeat | 400K | 1 | 3.4% | 8.0% | 8.02% | 8.02% |
| repeat | 400K | 2 | 33.1% | 14.5% | 1.67% | 11.19% |
| repeat | 400K | 3 | 36.9% | 22.1% | 0.12% | 0.94% |
| repeat | 400K | 4+ | 26.6% | 34.7% | 0.00% | 0.14% |
<!-- /report -->

Pair bounds are exact for a two-word query whose pair the slice ships,
which is why repeats of two words gain most (at 400,000 passages, 11.2%
proved against 14.5% held). From three words on they stay loose, and a
new query's pairs are often not among the training queries'.

Pair bounds against word bounds, all slice sizes:

<!-- report:pairs -->
| queries | passages | top 1 held | top 1 proved, words | top 1 proved, pairs | top 10 held | top 10 proved, words | top 10 proved, pairs | wrong proofs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| new | 10K | 2.1% | 0.08% | 0.28% | 0.6% | 0.01% | 0.03% | 0 |
| new | 30K | 5.8% | 0.24% | 0.86% | 2.0% | 0.04% | 0.13% | 0 |
| new | 100K | 12.9% | 0.57% | 2.52% | 5.0% | 0.17% | 0.49% | 0 |
| new | 200K | 19.3% | 0.88% | 4.09% | 7.9% | 0.31% | 0.95% | 0 |
| new | 400K | 29.6% | 1.45% | 6.83% | 12.8% | 0.55% | 1.74% | 0 |
| repeat | 10K | 2.2% | 0.15% | 0.49% | 0.9% | 0.05% | 0.12% | 0 |
| repeat | 30K | 6.1% | 0.38% | 1.60% | 3.1% | 0.18% | 0.40% | 0 |
| repeat | 100K | 13.7% | 0.83% | 4.15% | 7.9% | 0.37% | 1.18% | 0 |
| repeat | 200K | 20.4% | 1.22% | 6.87% | 13.2% | 0.54% | 2.29% | 0 |
| repeat | 400K | 32.0% | 1.87% | 11.68% | 22.5% | 0.87% | 4.36% | 0 |
<!-- /report -->

The confidence rule:

<!-- report:confidence -->
| queries | passages | alpha | top 1: answered on the device | top 1: of those, the cloud's | top 10: answered on the device | top 10: of those, the cloud's |
|---|---:|---:|---:|---:|---:|---:|
| new | 100K | 1.0 | 2.5% | 100% | 0.5% | 100% |
| new | 100K | 0.9 | 6.7% | 69% | 1.1% | 68% |
| new | 100K | 0.8 | 13.3% | 53% | 3.1% | 48% |
| new | 100K | 0.7 | 23.0% | 40% | 6.6% | 37% |
| new | 100K | 0.6 | 36.7% | 28% | 12.6% | 25% |
| new | 100K | 0.5 | 54.4% | 20% | 23.0% | 15% |
| new | 100K | 0.4 | 71.2% | 15% | 40.0% | 9% |
| new | 100K | 0.3 | 80.5% | 14% | 63.1% | 5% |
| new | 400K | 1.0 | 6.8% | 100% | 1.7% | 100% |
| new | 400K | 0.9 | 15.9% | 78% | 3.7% | 72% |
| new | 400K | 0.8 | 28.2% | 67% | 8.8% | 55% |
| new | 400K | 0.7 | 42.8% | 55% | 17.3% | 45% |
| new | 400K | 0.6 | 59.0% | 44% | 29.3% | 33% |
| new | 400K | 0.5 | 75.2% | 36% | 45.5% | 24% |
| new | 400K | 0.4 | 86.0% | 31% | 64.3% | 17% |
| new | 400K | 0.3 | 89.9% | 30% | 81.9% | 13% |
| repeat | 100K | 1.0 | 4.1% | 100% | 1.2% | 100% |
| repeat | 100K | 0.9 | 10.0% | 65% | 1.9% | 84% |
| repeat | 100K | 0.8 | 19.1% | 48% | 4.4% | 62% |
| repeat | 100K | 0.7 | 31.5% | 35% | 9.0% | 51% |
| repeat | 100K | 0.6 | 47.4% | 24% | 17.0% | 35% |
| repeat | 100K | 0.5 | 64.2% | 18% | 30.6% | 20% |
| repeat | 100K | 0.4 | 76.8% | 15% | 49.8% | 13% |
| repeat | 100K | 0.3 | 82.0% | 14% | 70.4% | 9% |
| repeat | 400K | 1.0 | 11.7% | 100% | 4.4% | 100% |
| repeat | 400K | 0.9 | 24.3% | 75% | 6.5% | 88% |
| repeat | 400K | 0.8 | 39.8% | 62% | 13.1% | 73% |
| repeat | 400K | 0.7 | 56.0% | 51% | 24.8% | 63% |
| repeat | 400K | 0.6 | 71.6% | 41% | 39.7% | 49% |
| repeat | 400K | 0.5 | 83.1% | 35% | 57.1% | 35% |
| repeat | 400K | 0.4 | 89.0% | 33% | 73.8% | 28% |
| repeat | 400K | 0.3 | 90.4% | 33% | 86.3% | 24% |
<!-- /report -->

### Against a result cache

For repeats, the cheaper exact answer is not a proof but a cached answer:
the cloud's top 10 ids for a query string, about 61 bytes (the training
queries average 20.8 bytes, [`search/answer_cache.json`](search/answer_cache.json),
plus ten 4-byte ids). A `cov` slice is built by completing training
queries, so beside it the device can hold those queries' answers and
answer exactly the "repeat: top 10 held" share above, with no bound at all.
At 400,000 passages that is 22.3% of the 943,457 training queries with a
full top 10 ([`search/select.json`](search/select.json)), about 210,000
answers or 13 MB. New strings are where only the proof (or the confidence
rule) can help.

## Recommendations: Yambda-50M

[`recs.py`](recs.py) (its docstring has the details) uses the protocol of
[`../recsys/yambda.py`](../recsys/README.md): Listen+ events, train on
everything up to 30 minutes before the last day, test on that day's 157,695
listens of 4,599 users, tune on the day before. Every track gets a vector
(a 64-dimension SVD of the user-track plays) and a two-level semantic ID
(residual k-means, 256 centroids a level); a track's pack is its two-level
prefix: 36,790 packs, median 3 tracks, largest 2,781. A user's taste is the
decayed sum of their history's vectors (decay 1 hour, chosen on the
validation day at the edge of its grid of 1, 24 and 168 hours and none,
[`recs/recs_validation.json`](recs/recs_validation.json)).

The cloud (`vec_cloud`) ranks all 629,298 tracks by cosine with the taste.
The device (`vec_device@N`) ranks only what it holds: the user's own tracks,
a shared base pack of the 1,000 most played, and the packs whose centroids
are nearest the taste, up to N tracks. `rrf` fuses either with `history`
(the user's own tracks by plays in the last 720 hours), which runs on the
device with no download. Every source answers 100 tracks.

<!-- report:recs -->
| source | pack tracks | pack MB | NDCG@10 | recall@100 (of min(targets, 100)) |
|---|---:|---:|---:|---:|
| history |  | 0 | 0.1854 | 0.2702 |
| vec_cloud |  |  | 0.0827 | 0.1180 |
| vec_device@1000 | 1000 | 0.4 | 0.0842 | 0.1235 |
| vec_device@5000 | 5000 | 1.9 | 0.0828 | 0.1197 |
| vec_device@20000 | 20000 | 7.3 | 0.0827 | 0.1185 |
| vec_device@50000 | 50000 | 18.4 | 0.0827 | 0.1182 |
| rrf_cloud |  |  | 0.1501 | 0.2523 |
| rrf_device@1000 | 1000 | 0.4 | 0.1516 | 0.2537 |
| rrf_device@5000 | 5000 | 1.9 | 0.1505 | 0.2526 |
| rrf_device@20000 | 20000 | 7.3 | 0.1502 | 0.2524 |
| rrf_device@50000 | 50000 | 18.4 | 0.1501 | 0.2524 |
| decay_pop (cloud, `../recsys`) |  |  | 0.0256 | 0.0486 |
| item_knn (cloud, `../recsys`) |  |  | 0.1152 | 0.1385 |
| Strata `encode` (cloud, `../recsys`) |  |  | 0.0799 | 0.1189 |
| Strata `ppr` (cloud, `../recsys`) |  |  | 0.1188 | 0.1074 |
| Strata `fused` (cloud, `../recsys`) |  |  | 0.1655 | 0.2351 |

Pack MB is the pack tracks alone; the device also holds the base pack (0.4 MB) and the user's own tracks.
<!-- /report -->

On discovery, the test day's listens of tracks the user had never played,
against lists with the user's own tracks left out:

<!-- report:discovery -->
| source (user's own tracks left out) | pack tracks | NDCG@10 | recall@100 (of min(targets, 100)) |
|---|---:|---:|---:|
| decay_pop |  | 0.0234 | 0.0482 |
| vec_cloud |  | 0.0236 | 0.0754 |
| vec_device@1000 | 1000 | 0.0224 | 0.0608 |
| vec_device@5000 | 5000 | 0.0235 | 0.0719 |
| vec_device@20000 | 20000 | 0.0236 | 0.0747 |
| vec_device@50000 | 50000 | 0.0236 | 0.0756 |

3,539 of 4,599 test users listened to at least one track they had never played.
<!-- /report -->

The device keeps up with the cloud because the packs nearest the taste
hold the top of the cloud's own cosine ranking: from 5,000 pack tracks on,
NDCG@10 is within 0.5% of the cloud's on both target sets, and at 20,000 it
is equal. The 1,000-track device is a little above the cloud on all targets
(its base pack pulls popular tracks up) and 5% below it on discovery. What
this measures is how much a device loses by not holding the catalogue, for
one model. It does not make that model good: on discovery the taste vector
is barely above DecayPop. A stronger model (`ppr`, `fused`) would need its
own pack design, which is not tested here.

### Pack bytes and churn

A pack file is a Strata file of tracks with an id, a semantic-ID keyword
and a 64-dimension vector (float32 plus SQ8 codes, flat search),
optimised and vacuumed ([`recs/recs_bytes.json`](recs/recs_bytes.json));
random unit vectors stand in for the tracks', which does not change the
size:

<!-- report:pack-bytes -->
| tracks | file bytes | bytes / track |
|---|---:|---:|
| 5,000 | 1,884,160 | 377 |
| 50,000 | 18,370,560 | 367 |
<!-- /report -->

How many tracks enter a base pack of the N most played tracks of the past
7 days, each day, over the 30 days before the test day
([`recs/recs_churn.json`](recs/recs_churn.json)):

<!-- report:churn -->
| base pack | tracks entering a day (mean) | most in a day | KB a day (mean) |
|---|---:|---:|---:|
| 1K | 47 | 61 | 17 |
| 10K | 591 | 665 | 217 |
| 50K | 3,785 | 4,120 | 1391 |

New tracks a day (first listen, mean of 30 days): 1,598, in a catalogue of 629,298 tracks.
<!-- /report -->

The packs nearest a taste change as the taste moves; how many bytes a day
that costs a user was not measured. ORCAS has no timestamps, so search
slices' daily patches could not be measured either.

## Reproduce

Data (ORCAS is under a non-commercial research licence):

```bash
mkdir -p ~/data/slices && cd ~/data/slices
curl -L -o orcas-queries.tsv.gz https://msmarco.z22.web.core.windows.net/msmarcoranking/orcas-doctrain-queries.tsv.gz
curl -LO https://public.ukp.informatik.tu-darmstadt.de/thakur/BEIR/datasets/msmarco.zip
```

Yambda-50M is fetched as in [`../recsys`](../recsys/README.md). Then, with
the `strata` Python package installed (`strata-py`, `maturin develop
--release`) and numpy, polars and scipy:

```bash
W="--work $HOME/data/slices/work"
python search.py $W prep --data ~/data/slices
python search.py $W load && python search.py $W cloud train && python search.py $W select
python search.py $W load --tiers && python search.py $W cloud test
python search.py $W eval test && python search.py $W eval seen && python search.py $W bytes
python search.py $W eval2 test && python search.py $W eval2 seen && python search.py $W residuals
python recs.py $W --data ~/data/yambda prep && python recs.py $W --data ~/data/yambda eval
python recs.py $W --data ~/data/yambda churn && python recs.py $W bytes
cp ~/data/slices/work/{prep,select,eval_test,eval_seen,eval2_test,eval2_seen,bytes,residuals}.json search/
cp ~/data/slices/work/recs_*.json recs/
python report.py
```

On the container above the search side takes under an hour (each load of
the 8.8 million passages about 4 minutes, the cloud top 10 of a million
queries about 6) and about 8 GB of disk.
