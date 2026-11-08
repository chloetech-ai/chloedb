# On-device slices, Phase 0: how much a device could answer alone

The replay study of the "Strata on the device" design (Claude Doc,
https://claude.ai/artifact/14s2MaZu2wua3Q3NNvbL2E), run before building
anything. A slice is part of the corpus shipped to a device, scored there
with the cloud's statistics, plus enough to know when the device's answer
is the cloud's. The question is how much traffic a slice of a given size
answers alone, for search (MS MARCO passages, searched by ORCAS's queries,
by the AOL log's and by ideal Zipf traffic over AOL's strings) and for
recommendations (Yambda-50M).

Everything ran in one cloud container (Intel Xeon @ 2.30GHz, 4 cores,
15 GB, Linux 6.18) on 2026-11-07 and 08. The numbers are shares of queries and
users and file sizes, which do not depend on the machine; no time here is a
benchmark.

The gate the design proposed (Gate A): 40% of query volume answered on the
device with a proof at a slice of 50 MB or less, and recommendations made on
the device within 5% of the cloud's NDCG@10.

## Verdict

- **Search passes Gate A when the traffic's head is heavy, and fails on
  AOL's.** The gate passes once the 10,000 most searched strings carry about
  a third of all searches.
  - **On ideal Zipf traffic** (the most searched query twice as often as
    the second, three times the third; [below](#search-on-ideal-zipf-traffic)),
    the top 10,000 of AOL's strings carry 68% of searches. A 33 MB slice
    holds the whole cloud top 10 for 79.6% of them and proves 65.1% with
    pair bounds. With a cache of the 100,000 most searched answers (38.5 MB
    in all) the device answers 76.2% exactly. Even a 3.4 MB slice proves
    51.6% with pair bounds (at most 12.8 MB more). Spread over a tail as long as AOL's (8.7 million strings), an
    estimate from the measurements gives 66.3%. With AOL's own slope
    (r^-0.86) it passes over 1 million strings (56.1%) and sits on the line
    with AOL's tail (39.9%, estimated).
  - **On the real AOL log** (21 million searches, run over the same
    passages; [below](#search-weighted-by-volume-the-aol-log)), the top
    10,000 strings carry only 22% of searches. The same 33 MB slice holds
    the top 10 for 41.5% and proves 22.6%. The best mix within 50 MB (that
    slice and the 100,000-answer cache, 38.6 MB) answers 27.6% exactly,
    against the gate's 40%. The 48% of searches that are new strings are
    almost never proved (0.6%).
  - Band by band the two agree: strings searched 1,000 times or more are
    held entirely and about 91% proved on both, strings searched 100 to
    999 times held entirely and 75% to 78% proved. Only the share of
    traffic in those bands differs (71% on Zipf, 22% on AOL).
  - **Counted by distinct strings** (ORCAS): at the same 33 MB the proof
    answers 0.2% to 0.4% of top 10s, or 0.5% to 1.2% with pair bounds, and
    the slice holds only 5% to 8% of them.
  - The proof itself is sound: not one wrong answer, on either log, at any
    size, k or kind of bound. What limits it is the bound: the sum of
    per-word bounds is near every word's ceiling at once, so from two words
    on it proves far less than the slice holds.
  - For cost alone, a plain result cache does most of the work and needs no
    slice and no proof: the top 10 ids of the 10,000 most searched strings
    (0.6 MB) answer 69.3% of Zipf searches, and those of the 300,000 most
    searched (18 MB) 40.2% of AOL's. Only showing those results offline
    needs their passages on the device.
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

What this does not settle: AOL's traffic is web search from 2006, run over
another corpus, and its head is mostly short navigational queries, which
the proof handles best. The Zipf runs keep AOL's strings and change only
how often each is searched. Whether an app's own search (a shop, a
catalogue, a help centre) has the heavy head or AOL's flatter one is the
open question; its own log, or TripClick's health-search log (on request
from the Trip Database), would say.

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

## Search weighted by volume: the AOL log

[`aol.py`](aol.py) (its docstring has the details) repeats the search study
on a log with counts: the AOL search log of March to May 2006, 21,011,340
searches (a user's consecutive rows of one query are one search), over the
same 8.8 million MS MARCO passages. The cloud's answer is BM25 over a
modern web corpus, not over the pages AOL's users saw, so only the shape of
the traffic comes from AOL. That log was withdrawn in 2006 for its privacy
failings and is still widely used in IR research (ir_datasets' `aol-ia`);
nothing here quotes a query, and the results hold only counts and shares.

Training is every search before 17 May 2006 (17.6 million searches, 8.7
million distinct strings); the 1,000,000 most searched strings, 54% of the
training searches, choose the slices and fill the result cache. The test is
a uniform sample of 200,000 of the 2.9 million searches of the last two
weeks (156,325 distinct strings), so each string weighs its searches and
every share below is a share of test search volume
([`aol/prep.json`](aol/prep.json)). 52.1% of test searches repeat a string
searched in training. Slices are chosen as for ORCAS, weighted by training
searches: `cov` completes the query with the most searches per missing
passage first. Shares are of the test searches whose query has a full top
10 in the whole corpus (93.6% of them).

Query coverage (`cov`, [`aol/eval.json`](aol/eval.json)):

<!-- report:aol-cov -->
| passages | file MB | top 1 held | top 1 proved, words | top 1 proved, pairs | top 10 held | top 10 proved, words | top 10 proved, pairs | top 10 proved or cached (100K answers) | wrong proofs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 10K | 3.4 | 30.0% | 8.3% | 15.6% | 26.3% | 7.2% | 13.4% | 17.3% | 0 |
| 30K | 9.9 | 38.6% | 10.9% | 20.7% | 33.1% | 9.3% | 17.6% | 22.5% | 0 |
| 100K | 32.5 | 50.8% | 13.5% | 27.0% | 41.5% | 11.6% | 22.6% | 28.6% | 0 |
| 200K | 64.6 | 59.5% | 15.0% | 30.8% | 47.7% | 12.7% | 25.7% | 32.5% | 0 |
| 400K | 128.7 | 69.3% | 16.6% | 34.9% | 55.0% | 13.8% | 28.8% | 36.2% | 0 |
<!-- /report -->

Popular passages (`pop`):

<!-- report:aol-pop -->
| passages | file MB | top 1 held | top 1 proved, words | top 1 proved, pairs | top 10 held | top 10 proved, words | top 10 proved, pairs | top 10 proved or cached (100K answers) | wrong proofs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 10K | 3.4 | 32.6% | 8.2% | 16.3% | 25.3% | 6.7% | 12.2% | 15.9% | 0 |
| 30K | 9.8 | 42.2% | 11.0% | 21.7% | 31.0% | 8.8% | 16.0% | 20.5% | 0 |
| 100K | 31.8 | 55.2% | 13.7% | 27.9% | 39.0% | 10.9% | 20.9% | 26.3% | 0 |
| 200K | 63.3 | 63.8% | 15.2% | 31.7% | 45.0% | 12.2% | 24.1% | 30.3% | 0 |
| 400K | 126.3 | 72.8% | 16.6% | 35.4% | 52.4% | 13.4% | 27.5% | 34.5% | 0 |
<!-- /report -->

By how often the test query was searched in training (`cov`, top 10):

<!-- report:aol-bands -->
| passages | training searches of the string | share of test volume | top 10 held | top 10 proved, pairs |
|---|---:|---:|---:|---:|
| 100K | new string | 48.0% | 20.4% | 0.6% |
| 100K | 1 to 9 | 16.4% | 22.8% | 4.0% |
| 100K | 10 to 99 | 13.6% | 44.0% | 23.1% |
| 100K | 100 to 999 | 10.7% | 100.0% | 77.6% |
| 100K | 1,000 or more | 11.3% | 100.0% | 90.6% |
| 400K | new string | 48.0% | 30.1% | 1.5% |
| 400K | 1 to 9 | 16.4% | 39.8% | 11.1% |
| 400K | 10 to 99 | 13.6% | 88.6% | 56.6% |
| 400K | 100 to 999 | 10.7% | 100.0% | 77.8% |
| 400K | 1,000 or more | 11.3% | 100.0% | 90.7% |
<!-- /report -->

A result cache alone, the cloud's top 10 ids for the most searched training
strings, needs no slice and no proof; to show its answers offline, the
device needs their passages too:

<!-- report:aol-cache -->
| answers cached | MB (ids only) | test search volume answered |
|---|---:|---:|
| 10,000 | 0.6 | 22.6% |
| 30,000 | 1.8 | 27.9% |
| 100,000 | 6.1 | 34.4% |
| 300,000 | 18.2 | 40.2% |
| 1,000,000 | 60.8 | 44.8% |

52.1% of test searches repeat a string searched in training at all.
<!-- /report -->

The best ways found to spend 50 MB on exact answers with the passages on
the device (a slice, a cache of answers whose passages the slice holds, and
the proof):

<!-- report:aol-budget -->
| slice | passages | slice MB | answers cached | cache MB | pair bounds MB | MB on the device | answered by | test volume answered exactly on the device |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| cov | 100K | 32.5 | 100,000 | 6.1 |  | 38.6 | cache | 27.6% |
| cov | 100K | 32.5 | 30,000 | 1.8 | 12.8 | 47.1 | cache + proof | 27.4% |
| cov | 100K | 32.5 | 10,000 | 0.6 | 12.8 | 45.9 | cache + proof | 26.2% |
| cov | 100K | 32.5 | 30,000 | 1.8 |  | 34.3 | cache | 25.6% |
| pop | 100K | 31.8 | 100,000 | 6.1 |  | 37.9 | cache | 25.6% |
| pop | 100K | 31.8 | 30,000 | 1.8 | 12.8 | 46.4 | cache + proof | 25.3% |
| pop | 100K | 31.8 | 10,000 | 0.6 | 12.8 | 45.2 | cache + proof | 24.5% |
| pop | 100K | 31.8 | 30,000 | 1.8 |  | 33.6 | cache | 24.1% |

Pair bounds are counted for all 1,422,227 training pairs at 9 bytes each (12.8 MB); a slice needs only the pairs of words it holds, so this is an upper bound.
<!-- /report -->

With volume, the slice holds what the traffic asks for most: the head. At
33 MB it holds the whole top 10 for 41.5% of searches, which would pass the
gate with a perfect bound. The proof gets about half of that, almost all of
it from strings searched before, and there a cached answer does the same
job more cheaply. On new strings, the only place the proof could add what a
cache cannot, it proves 0.6% of their volume at 33 MB (1.5% at 128 MB).

## Search on ideal Zipf traffic

AOL's head is flatter than search traffic is often taken to be: its 10,000
most searched strings are 22.3% of its training searches, and its counts
fall as about r^-0.86 with rank r. [`zipf.py`](zipf.py) asks what the same
slices do when the traffic follows Zipf's law exactly, the most searched
query twice as often as the second and three times as often as the third.
It keeps AOL's 1,000,000 most searched training strings in AOL's order and
gives rank r the weight r^-s: at s = 1 the top 10,000 strings are 68.0% of
searches, and at s = 0.86, AOL's own slope over the same strings, 45.2%.
Slices and the result cache are chosen from those weights, and the test is
200,000 searches drawn from them (65,366 distinct strings at s = 1, 103,759
at s = 0.86), answered by the same cloud top 10s as the AOL study
([`zipf/prep.json`](zipf/prep.json), [`zipf086/prep.json`](zipf086/prep.json)).
Everything else is `aol.py`'s.

One row per traffic shape, `cov` slices, top 10:

<!-- report:zipf-summary -->
| traffic | top 10,000 strings' share of searches | test volume on new strings | 100K slice: top 10 held | 100K slice: top 10 proved, pairs | 10,000 cached answers (0.6 MB) | best exact answer within 50 MB | Gate A (40%) |
|---|---:|---:|---:|---:|---:|---:|---:|
| AOL log, measured | 22.3% | 48.0% | 41.5% | 22.6% | 22.6% | 27.6% (cache, 38.6 MB) | fail |
| Zipf s = 0.86 over 1M strings | 45.2% | 0.0% | 62.9% | 45.5% | 46.5% | 56.1% (cache, 38.6 MB) | pass |
| Zipf s = 1 over 1M strings | 68.0% | 0.0% | 79.6% | 65.1% | 69.3% | 76.2% (cache, 38.5 MB) | pass |
<!-- /report -->

The rest of this section is s = 1. Query coverage (`cov`,
[`zipf/eval.json`](zipf/eval.json)):

<!-- report:zipf-cov -->
| passages | file MB | top 1 held | top 1 proved, words | top 1 proved, pairs | top 10 held | top 10 proved, words | top 10 proved, pairs | top 10 proved or cached (100K answers) | wrong proofs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 10K | 3.4 | 64.9% | 35.4% | 56.5% | 62.4% | 32.7% | 51.6% | 60.5% | 0 |
| 30K | 9.9 | 73.8% | 39.1% | 64.0% | 70.8% | 35.6% | 58.0% | 68.6% | 0 |
| 100K | 32.4 | 83.2% | 42.7% | 71.7% | 79.6% | 38.8% | 65.1% | 76.9% | 0 |
| 200K | 64.5 | 88.3% | 44.3% | 75.9% | 84.8% | 40.0% | 68.5% | 81.4% | 0 |
| 400K | 128.5 | 93.0% | 46.0% | 79.8% | 90.0% | 41.2% | 71.9% | 85.5% | 0 |
<!-- /report -->

Popular passages (`pop`):

<!-- report:zipf-pop -->
| passages | file MB | top 1 held | top 1 proved, words | top 1 proved, pairs | top 10 held | top 10 proved, words | top 10 proved, pairs | top 10 proved or cached (100K answers) | wrong proofs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 10K | 3.4 | 66.5% | 35.5% | 57.5% | 60.6% | 32.1% | 50.1% | 58.6% | 0 |
| 30K | 9.8 | 75.4% | 39.2% | 64.9% | 68.6% | 35.3% | 56.4% | 66.4% | 0 |
| 100K | 31.9 | 84.8% | 42.7% | 72.5% | 77.3% | 38.2% | 63.5% | 74.6% | 0 |
| 200K | 63.4 | 89.7% | 44.5% | 76.5% | 82.5% | 39.6% | 67.2% | 79.4% | 0 |
| 400K | 126.4 | 94.1% | 46.0% | 80.2% | 88.0% | 40.8% | 70.8% | 84.1% | 0 |
<!-- /report -->

By how often the string is expected to be searched in a training period as
long as AOL's (`cov`, top 10), with the ranks that band covers:

<!-- report:zipf-bands -->
| passages | training searches of the string | share of test volume | top 10 held | top 10 proved, pairs |
|---|---:|---:|---:|---:|
| 100K | 1 to 9 (ranks 128,914 to 1,000,000) | 13.7% | 21.6% | 3.7% |
| 100K | 10 to 99 (ranks 12,309 to 128,913) | 15.7% | 38.4% | 17.0% |
| 100K | 100 to 999 (ranks 1,226 to 12,308) | 15.6% | 100.0% | 74.5% |
| 100K | 1,000 or more (ranks 1 to 1,225) | 55.1% | 100.0% | 91.3% |
| 400K | 1 to 9 (ranks 128,914 to 1,000,000) | 13.7% | 40.2% | 11.6% |
| 400K | 10 to 99 (ranks 12,309 to 128,913) | 15.7% | 88.2% | 53.4% |
| 400K | 100 to 999 (ranks 1,226 to 12,308) | 15.6% | 100.0% | 74.7% |
| 400K | 1,000 or more (ranks 1 to 1,225) | 55.1% | 100.0% | 91.3% |
<!-- /report -->

A result cache of ids alone:

<!-- report:zipf-cache -->
| answers cached | MB (ids only) | test search volume answered |
|---|---:|---:|
| 10,000 | 0.6 | 69.3% |
| 30,000 | 1.8 | 76.6% |
| 100,000 | 6.1 | 84.6% |
| 300,000 | 18.2 | 92.0% |
| 1,000,000 | 60.8 | 100.0% |
<!-- /report -->

The best ways found to spend 50 MB on exact answers with the passages on
the device:

<!-- report:zipf-budget -->
| slice | passages | slice MB | answers cached | cache MB | pair bounds MB | MB on the device | answered by | test volume answered exactly on the device |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| cov | 100K | 32.4 | 100,000 | 6.1 |  | 38.5 | cache | 76.2% |
| cov | 100K | 32.4 | 30,000 | 1.8 | 12.8 | 47.1 | cache + proof | 75.3% |
| pop | 100K | 31.9 | 100,000 | 6.1 |  | 38.0 | cache | 74.1% |
| cov | 100K | 32.4 | 30,000 | 1.8 |  | 34.3 | cache | 73.8% |
| cov | 100K | 32.4 | 10,000 | 0.6 | 12.8 | 45.8 | cache + proof | 73.4% |
| pop | 100K | 31.9 | 30,000 | 1.8 | 12.8 | 46.5 | cache + proof | 73.2% |
| pop | 100K | 31.9 | 30,000 | 1.8 |  | 33.7 | cache | 72.1% |
| pop | 100K | 31.9 | 10,000 | 0.6 | 12.8 | 45.3 | cache + proof | 71.9% |

Pair bounds are counted for all 1,422,227 training pairs at 9 bytes each (12.8 MB); a slice needs only the pairs of words it holds, so this is an upper bound.
<!-- /report -->

Every test string here is among the 1,000,000, so none is new, where 48% of
AOL's test searches were. A real log has a long tail: AOL's training had
8,697,441 distinct strings. Spreading the same law over that many strings,
the top 1,000,000 are measured above and every other string is counted at
the rates measured for AOL's new strings. This is an estimate computed from
the two measurements, not a run:

<!-- report:zipf-tail -->
| traffic | top 10,000 strings' share of searches | top 1,000,000 strings' share | 100K slice: top 10 held | 100K slice: top 10 proved, pairs | 10,000 cached answers (0.6 MB) | best exact answer within 50 MB | Gate A (40%) |
|---|---:|---:|---:|---:|---:|---:|---:|
| Zipf s = 0.86 over 8.7M strings | 32.1% | 71.0% | 50.6% | 32.5% | 33.0% | 39.9% (cache, 38.6 MB) | fail |
| Zipf s = 1 over 8.7M strings | 59.1% | 86.9% | 71.9% | 56.6% | 60.2% | 66.3% (cache, 38.5 MB) | pass |
<!-- /report -->

What decides the gate is how much volume sits in the head, not anything the
slices do differently. Band by band, the rates match the AOL study: strings
searched 1,000 times or more are 100% held and about 91% proved in both,
strings searched 100 to 999 times 100% held and 75% to 78% proved. Zipf
moves the volume into those bands, 70.7% of it against AOL's 22.0%. Across
the five shapes above, the best exact share within 50 MB is 1.1 to 1.25
times the top 10,000 strings' share of searches, so the gate passes once
those strings carry about a third of the traffic. An app can estimate its
own outcome from its log the same way: the share of its search volume in
each band, times these rates.

Under every shape a result cache answers the head more cheaply than a
proof. The slice and the proof pay for themselves on answers shown offline
with their passages, and on the middle of the distribution (strings
searched 10 to 99 times, ranks 12,309 to 128,913 at s = 1), where a bigger
slice raises the share held from 38% at 100K passages to 88% at 400K.

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
curl -L -o ~/data/slices/aol-data.tar.gz https://cim.mcgill.ca/~dudek/206/Logs/AOL-user-ct-collection/aol-data.tar.gz  # md5 31cd27ce12c3a3f2df62a38050ce4c0a
A="--work $HOME/data/slices/aol --orcas $HOME/data/slices/work"
python aol.py $A prep --data ~/data/slices && python aol.py $A cloud train && python aol.py $A cloud test
python aol.py $A select && python aol.py $A load --tiers && python aol.py $A eval && python aol.py $A bytes
cp ~/data/slices/work/{prep,select,eval_test,eval_seen,eval2_test,eval2_seen,bytes,residuals}.json search/
cp ~/data/slices/work/recs_*.json recs/
cp ~/data/slices/aol/{prep,select,eval,bytes}.json aol/
Z="--aol $HOME/data/slices/aol"
python zipf.py $Z --work ~/data/slices/zipf prep && python zipf.py $Z --work ~/data/slices/zipf select
python zipf.py $Z --work ~/data/slices/zipf load --tiers && python zipf.py $Z --work ~/data/slices/zipf eval && python zipf.py $Z --work ~/data/slices/zipf bytes
python zipf.py $Z --work ~/data/slices/zipf086 prep --s 0.86   # then select, load --tiers, eval, bytes as above
cp ~/data/slices/zipf/{prep,select,eval,bytes}.json zipf/ && cp ~/data/slices/zipf086/{prep,select,eval,bytes}.json zipf086/
python report.py
```

On the container above the ORCAS side takes under an hour (each load of
the 8.8 million passages about 4 minutes, the cloud top 10 of a million
queries about 6) and about 8 GB of disk; the AOL side about 40 minutes more
and 3 GB, and each Zipf run about 20 minutes and 3 GB.
