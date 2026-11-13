# Analyzers on MIRACL

BM25 through SQL with each analyzer that fits a language, on
[MIRACL](https://project-miracl.github.io/)'s dev queries (Zhang et al.,
2023: Wikipedia passages, questions and human judgements in 18 languages),
for 11 of its languages: what `english` (the default) and `simple` make of
text in other languages and scripts, and what `standard`, `cjk` and the
languages' analyzers (`src/text/standard.rs`) change.

```text
tests/fixtures/miracl/fetch.sh ~/miracl yo sw th hi te bn fi ko ar zh ja
cargo build --profile ci --bin chloedb-bm25-eval
target/ci/chloedb-bm25-eval --dir ~/miracl --split dev --collections miracl-ja \
    --analyzer standard --variants plain_r100 --tag ja_standard --out results/miracl
```

A passage is its title and its text, one `TEXT` field without positions.
A query is the question as written, ranked by `bm25(body, :q, syntax =>
'plain')` for 100 rows (`plain_r100`): nDCG@10 and recall@100 over the
judged queries. `bm25_<language>_<analyzer>.json` is each run's report.

Measured on 2026-11-12 on a cloud VM (4 vCPUs, Intel Xeon @ 2.10GHz, 15 GB,
Linux 6.18), one run each, the queries one after another on one thread.
The program was built with `--profile ci` (release without link-time
optimisation), so the times are for comparing analyzers with each other
here, not with times measured elsewhere. Load is the time to write and
merge the passages, and the database's size includes the stored passages.

## By analyzer (k1 1.2, b 0.75, ChloeDB's defaults)

| language | passages | queries | analyzer | nDCG@10 | recall@100 | median ms | p99 ms | load s | database MB |
|---|---|---|---|---|---|---|---|---|---|
| Arabic | 2,061,414 | 2896 | `english` | 0.3508 | 0.7215 | 1.89 | 10.3 | 46 | 962 |
| Arabic | 2,061,414 | 2896 | `simple` | 0.3509 | 0.7219 | 1.98 | 10.7 | 48 | 963 |
| Arabic | 2,061,414 | 2896 | `standard` | 0.3509 | 0.7230 | 1.86 | 9.9 | 51 | 965 |
| Arabic | 2,061,414 | 2896 | `arabic` | **0.3937** | 0.8124 | 2.65 | 13.5 | 47 | 884 |
| Bengali | 297,265 | 411 | `english` | 0.4280 | 0.8344 | 1.47 | 7.3 | 8 | 188 |
| Bengali | 297,265 | 411 | `simple` | 0.4281 | 0.8352 | 1.50 | 6.5 | 8 | 188 |
| Bengali | 297,265 | 411 | `standard` | 0.4134 | 0.7933 | 0.80 | 3.1 | 10 | 189 |
| Bengali | 297,265 | 411 | `bengali` | **0.4549** | 0.8794 | 0.93 | 3.4 | 10 | 182 |
| Chinese | 4,934,368 | 393 | `english` | 0.0000 | 0.0104 | 0.16 | 0.8 | 101 | 2,799 |
| Chinese | 4,934,368 | 393 | `simple` | 0.0000 | 0.0104 | 0.16 | 0.9 | 96 | 2,802 |
| Chinese | 4,934,368 | 393 | `standard` | 0.1742 | 0.5295 | 3.25 | 17.9 | 86 | 1,812 |
| Chinese | 4,934,368 | 393 | `cjk` | 0.1387 | 0.5118 | 3.44 | 13.1 | 153 | 2,607 |
| Chinese | 4,934,368 | 393 | `chinese` | **0.1974** | 0.5964 | 3.79 | 19.0 | 87 | 1,783 |
| Finnish | 1,883,509 | 1271 | `english` | 0.4192 | 0.7456 | 1.07 | 5.1 | 32 | 781 |
| Finnish | 1,883,509 | 1271 | `simple` | 0.4241 | 0.7490 | 1.20 | 5.2 | 29 | 784 |
| Finnish | 1,883,509 | 1271 | `standard` | 0.4233 | 0.7484 | 1.18 | 5.2 | 34 | 785 |
| Finnish | 1,883,509 | 1271 | `finnish` | **0.4753** | 0.8614 | 1.60 | 7.5 | 28 | 716 |
| Hindi | 506,264 | 350 | `english` | 0.3712 | 0.7571 | 3.65 | 19.1 | 14 | 296 |
| Hindi | 506,264 | 350 | `simple` | 0.3712 | 0.7600 | 3.70 | 18.3 | 13 | 297 |
| Hindi | 506,264 | 350 | `standard` | 0.3563 | 0.7438 | 2.05 | 9.0 | 16 | 297 |
| Hindi | 506,264 | 350 | `hindi` | **0.4284** | 0.8369 | 2.59 | 14.1 | 15 | 288 |
| Japanese | 6,953,614 | 860 | `english` | 0.0480 | 0.1413 | 0.46 | 6.2 | 166 | 4,469 |
| Japanese | 6,953,614 | 860 | `simple` | 0.0479 | 0.1405 | 0.58 | 4.3 | 158 | 4,471 |
| Japanese | 6,953,614 | 860 | `standard` | **0.2955** | 0.7215 | 8.63 | 46.0 | 154 | 2,743 |
| Japanese | 6,953,614 | 860 | `cjk` | 0.2434 | 0.6595 | 23.29 | 94.7 | 168 | 3,446 |
| Korean | 1,486,752 | 213 | `english` | 0.2623 | 0.4282 | 0.58 | 6.6 | 36 | 697 |
| Korean | 1,486,752 | 213 | `simple` | 0.2626 | 0.4329 | 0.56 | 7.2 | 35 | 697 |
| Korean | 1,486,752 | 213 | `standard` | **0.3738** | 0.7648 | 3.49 | 10.6 | 27 | 644 |
| Korean | 1,486,752 | 213 | `cjk` | 0.3733 | 0.7648 | 3.82 | 17.6 | 27 | 650 |
| Swahili | 131,924 | 482 | `english` | 0.4301 | 0.8099 | 0.43 | 1.3 | 1 | 31 |
| Swahili | 131,924 | 482 | `simple` | **0.4305** | 0.8075 | 0.51 | 1.8 | 1 | 31 |
| Swahili | 131,924 | 482 | `standard` | 0.4286 | 0.8075 | 0.43 | 1.3 | 1 | 31 |
| Telugu | 518,079 | 828 | `english` | 0.3759 | 0.7166 | 1.43 | 7.5 | 20 | 300 |
| Telugu | 518,079 | 828 | `simple` | 0.3769 | 0.7178 | 1.41 | 7.0 | 19 | 301 |
| Telugu | 518,079 | 828 | `standard` | 0.3401 | 0.6979 | 0.51 | 2.6 | 23 | 305 |
| Telugu | 518,079 | 828 | `telugu` | **0.3796** | 0.7627 | 0.57 | 2.9 | 23 | 297 |
| Thai | 542,166 | 733 | `english` | 0.2892 | 0.5883 | 0.67 | 6.0 | 32 | 469 |
| Thai | 542,166 | 733 | `simple` | 0.2896 | 0.5883 | 0.70 | 6.2 | 30 | 470 |
| Thai | 542,166 | 733 | `standard` | **0.4570** | 0.8750 | 3.00 | 14.4 | 29 | 319 |
| Yoruba | 49,043 | 119 | `english` | 0.6384 | 0.9510 | 0.38 | 0.9 | 0 | 10 |
| Yoruba | 49,043 | 119 | `simple` | 0.6280 | 0.9538 | 0.37 | 1.2 | 0 | 10 |
| Yoruba | 49,043 | 119 | `standard` | **0.6492** | 0.9622 | 0.50 | 1.4 | 1 | 10 |

- **`english` and `simple` fail where words are not runs of letters.**
  Chinese is not found at all and Japanese hardly (a sentence is one
  term), and Thai and Korean score about a third lower than with
  `standard`.
- **`standard` reads them**: Thai 0.457 from 0.290, Korean 0.374 from
  0.263, Japanese 0.295 from 0.048, Chinese 0.174 from 0.000.
- **Dictionary words rank better than bigrams** for Chinese (0.174 against
  `cjk`'s 0.139) and Japanese (0.295 against 0.243), in a smaller database
  (1.8 against 2.6 GB, 2.7 against 3.4 GB) and, for Japanese, in about a
  third of the time a query takes. That is why `standard` takes ICU's words for both and
  bigrams are a separate analyzer, `cjk`, for a field where any run of
  characters must be found. For Korean both make bigrams.
- **A language's analyzer is worth naming**: Arabic 0.394 from 0.351,
  Finnish 0.475 from 0.424, Hindi 0.428 from 0.371, Bengali 0.455 from
  0.428 (each against `simple`), recall@100 by 4 to 11 points with them;
  Telugu's nDCG@10 hardly moves (0.380 from 0.377) and its recall@100 rises
  from 0.718 to 0.763. `chinese` (traditional characters read as simplified)
  gives 0.197 against `standard`'s 0.174.
- **`standard` is below `simple` for Hindi, Bengali and Telugu** (by 0.015
  to 0.037). `standard` keeps a word whole with its vowel signs, where
  `simple` cuts a word at each of them, which is wrong for a phrase but
  seems to act as a crude stemmer when ranking. Use the language's analyzer
  for these.
- Latin-script text without a stemmer (Swahili, Yoruba) is the same under
  the three, but for Yoruba's tone marks, which `standard` folds (0.649
  against 0.628).

## Against Pyserini's published BM25

Pyserini's [MIRACL baselines](https://castorini.github.io/pyserini/2cr/miracl.html)
(read on 2026-11-12) run Lucene's BM25 with the analyzer Pyserini has for
each language (Yoruba pretokenized) and, by Pyserini's defaults, k1 0.9
and b 0.4. The best analyzer of each language above, run again with those
parameters (`bm25(body, :q, syntax => 'plain', k1 => 0.9, b => 0.4)`, the
variant `k09b04_r100`; `bm25_<language>_<analyzer>_params.json`; these ran
while the machine did other work, so their times are left out):

| language | analyzer | nDCG@10, k1 1.2 b 0.75 | nDCG@10, k1 0.9 b 0.4 | recall@100, k1 0.9 b 0.4 | Pyserini nDCG@10 | Pyserini recall@100 |
|---|---|---|---|---|---|---|
| Arabic | `arabic` | 0.3937 | 0.4757 | 0.8735 | 0.4809 | 0.8885 |
| Bengali | `bengali` | 0.4549 | 0.5106 | 0.9165 | 0.5079 | 0.9088 |
| Chinese | `chinese` | 0.1974 | 0.2450 | 0.6485 | 0.1801 | 0.5599 |
| Finnish | `finnish` | 0.4753 | 0.5459 | 0.8841 | 0.5513 | 0.8910 |
| Hindi | `hindi` | 0.4284 | 0.4321 | 0.8398 | 0.4578 | 0.8679 |
| Japanese | `standard` | 0.2955 | 0.3499 | 0.7709 | 0.3689 | 0.8048 |
| Korean | `standard` | 0.3738 | 0.4237 | 0.7915 | 0.4533 | 0.8579 |
| Swahili | `standard` | 0.4286 | 0.4772 | 0.8285 | 0.3826 | 0.7008 |
| Telugu | `telugu` | 0.3796 | 0.4901 | 0.8464 | 0.4942 | 0.8307 |
| Thai | `standard` | 0.4570 | 0.5081 | 0.8999 | 0.4838 | 0.8874 |
| Yoruba | `standard` | 0.6492 | 0.7230 | 0.9622 | 0.4059 | 0.7325 |

With Pyserini's parameters ChloeDB's nDCG@10 is within 0.03 of its
baselines on eight of the eleven languages: within 0.005 on Arabic,
Bengali, Finnish and Telugu, above on Thai by 0.024, and below on Japanese
by 0.019, Hindi by 0.026 and Korean by 0.030. It is above on Chinese by
0.065, Swahili by 0.095 and Yoruba by 0.317. On these short passages
`k1 => 0.9, b => 0.4` ranks better than ChloeDB's defaults in every
language, by 0.004 (Hindi) to 0.110 (Telugu). The defaults are not changed
here: they are the ones the BEIR relevance gate holds.

