# One run at scale

Measured on 18 November 2026 on a 4-thread Intel Xeon at 2.10 GHz with
16.5 GB, Linux, with the `ci` builds (release without link-time
optimisation) of chloedb-experiments and of chloedb-server at this branch's
commit. The site is NFCorpus (248 judged queries that rank 10 rows), with
4,000 users, the DBN click model and softmax logging over the top 10 by
rank (τ = 1), 5 rows shown. Each JSON file beside this one is the run's
own summary (`sim --out`).

## Served: every impression a logged query

```bash
chloedb-experiments sim --beir ~/beir/nfcorpus --local DIR --mode served \
  --impressions 1000000 --concurrency 8 --model dbn --run m1 --out served-1m.json
```

| | |
|---|---|
| impressions | 1,000,000, each a logged, exploring query to a local chloedb-server, its clicks written as events |
| serving | 682 s: 1,466 impressions a second with 8 in flight |
| query time (the server's, `took_ms`) | p50 0.87 ms, p90 1.78 ms, p99 3.66 ms |
| reading the log back | 434 s through `/v1/rank_log/export`, labelled by the events |
| served impressions that differ from the fast mode's | 0 |
| impressions lost from the log, or different in it | 0 and 0 |
| what the server holds after | a log of 10,000,000 rows (the 10 ranked rows of each impression, 5 of them shown) and 474,813 click events |
| largest resident memory | chloedb-server 5.4 GB, this program 1.5 GB (sampled every 5 s) |

## Fast: the same draws in the process

```bash
chloedb-experiments sim --beir ~/beir/nfcorpus --local DIR --mode fast \
  --impressions 100000000 --model dbn --run f100000000 --out fast-100m.json
```

| impressions | drawing them with their clicks, and counting | drawing them again, with every estimator of five targets |
|---|---|---|
| 10,000,000 | 4.9 s (2.03 million a second) | 10.6 s |
| 100,000,000 | 49.6 s (2.02 million a second) | 101.4 s |

## What 100 million impressions estimate

Clicks per list, from `fast-100m.json`. The truth is exact, from the click
model. "Shown" is whether the app would show the estimate as a result or
with a warning (`Estimate::trusted`).

| target | truth | IPS | RIPS | CDR | IIPS | the app |
|---|---|---|---|---|---|---|
| ranking (the logger's) | 0.5206 | 0.5202 ± 0.0024 | 0.5204 ± 0.0007 | 0.5203 ± 0.0006 | 0.5411 ± 0.0002 | shown |
| rerank | 0.5066 | 0.5074 ± 0.0096 | 0.5039 ± 0.0032 | 0.5054 ± 0.0033 | 0.5136 ± 0.0002 | shown |
| oracle | 0.5979 | 0.5931 ± 0.0076 | 0.5943 ± 0.0013 | 0.5945 ± 0.0013 | 0.6863 ± 0.0002 | shown |
| reverse | 0.3279 | 0.4759 ± 0.1435 | 0.3779 ± 0.0626 | 0.3687 ± 0.0483 | 0.3075 ± 0.0003 | warned on IPS, RIPS and CDR (24 effective impressions) |
| wide (19% of its slots never logged) | 0.4974 | 0.2786 ± 0.0041 | 0.4490 ± 0.0023 | 0.5549 ± 0.0023 | 0.4708 ± 0.0002 | warned on all weighted estimators (weights' mean far from 1) |

Under DBN a click depends on the rows above it, so IIPS is biased here, as
theory says; RIPS and CDR hold. Oracle's RIPS and CDR are 2.8 and 2.7
standard errors low in this log, and 1.3 and 1.4 high in a second
independent one (run `g2`): chance, not bias.
