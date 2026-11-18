# Off-policy estimators against the truth

Written by `chloedb-experiments check` (4000 users, 5 rows shown, seed 7, taste 0.35; logging ranking `bm25(body, :t, syntax => 'plain') + 0.3 * bm25(title, :t, syntax => 'plain')`, `rerank` and `wide` by `2 * bm25(title, :t, syntax => 'plain') + bm25(body, :t, syntax => 'plain')`) in 630 s on Intel(R) Xeon(R) Processor @ 2.10GHz, 16.5 GB (4 threads).

Each case is one world (a collection as the site, a click model, a logging policy); each row is one estimator of one target ranking's clicks or satisfied clicks per impression, over independent logs. *bias* is the mean estimate less the truth, ± its standard error over the logs; *coverage* is the share of logs whose 95% interval held the truth. *holds* says whether the theory calls the estimator unbiased there.

**Every check passed.**

## nfcorpus · pbm · {"policy":"softmax","tau":1.0,"top":10,"by":"rank"}

100 logs of 100000 impressions, 14 s. Propensities: 10000000 lists, 0 mismatched; chi-square 10897 on 11160 df (p 0.962), largest |z| 3.83.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.3937 | 0.2297 | 0.2037 | 2.412 |
| rerank | 0.0% | 0.3833 | 0.2199 | 0.1993 | 2.415 |
| oracle | 0.0% | 0.5500 | 0.3481 | 0.2917 | 2.363 |
| reverse | 0.0% | 0.2126 | 0.0964 | 0.0822 | 2.485 |
| wide | 18.6% | 0.3751 | 0.2137 | 0.1967 | 2.416 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.3937 | -0.06174 ± 0.00018 | 0.06176 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.3937 | -0.01131 ± 0.00713 | 0.07183 | 0.94 | 110 | 0.991 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snips | 0.3937 | -0.00824 ± 0.00598 | 0.06009 | 0.97 | 110 | 0.991 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | iips | 0.3937 | +0.00029 ± 0.00044 | 0.00439 | 0.96 | 11565 | 0.999 | 100% | 0.96 | yes | ok |
| ranking | clicks | sniips | 0.3937 | +0.00021 ± 0.00040 | 0.00399 | 0.95 | 11565 | 0.999 | 100% | 0.95 | yes | ok |
| ranking | clicks | rips | 0.3937 | +0.00258 ± 0.00126 | 0.01278 | 0.96 | 110 | 0.989 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snrips | 0.3937 | +0.00274 ± 0.00129 | 0.01316 | 0.97 | 110 | 0.989 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | dm-pbm | 0.3937 | +0.00346 ± 0.00022 | 0.00410 | 0.32 |  |  | 100% | 0.32 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.3937 | -0.02814 ± 0.00021 | 0.02821 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.3937 | +0.00015 ± 0.00038 | 0.00376 | 0.94 | 11565 | 0.999 | 100% | 0.94 | yes | ok |
| ranking | clicks | cdr | 0.3937 | +0.00232 ± 0.00132 | 0.01334 | 1.00 | 110 | 0.989 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | naive | 0.2297 | -0.04720 ± 0.00014 | 0.04722 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.2297 | -0.01250 ± 0.00488 | 0.05015 | 0.94 | 110 | 0.991 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snips | 0.2297 | -0.01042 ± 0.00453 | 0.04625 | 0.92 | 110 | 0.991 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | iips | 0.2297 | +0.00016 ± 0.00032 | 0.00319 | 0.97 | 11565 | 0.999 | 100% | 0.97 | yes | ok |
| ranking | satisfied | sniips | 0.2297 | +0.00011 ± 0.00030 | 0.00300 | 0.98 | 11565 | 0.999 | 100% | 0.98 | yes | ok |
| ranking | satisfied | rips | 0.2297 | +0.00079 ± 0.00099 | 0.00986 | 0.97 | 110 | 0.989 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snrips | 0.2297 | +0.00090 ± 0.00103 | 0.01025 | 0.96 | 110 | 0.989 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | dm-pbm | 0.2297 | +0.00312 ± 0.00018 | 0.00359 | 0.20 |  |  | 100% | 0.20 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.2297 | -0.01969 ± 0.00016 | 0.01975 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.2297 | +0.00003 ± 0.00028 | 0.00281 | 0.95 | 11565 | 0.999 | 100% | 0.95 | yes | ok |
| ranking | satisfied | cdr | 0.2297 | +0.00026 ± 0.00101 | 0.01001 | 0.93 | 110 | 0.989 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| rerank | clicks | naive | 0.3833 | -0.05131 ± 0.00018 | 0.05134 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.3833 | -0.01283 ± 0.02468 | 0.24589 | 0.74 | 20 | 0.936 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 20 impressions, mean 0.94; 6% of logs usable) |
| rerank | clicks | snips | 0.3833 | +0.01943 ± 0.01582 | 0.15859 | 0.81 | 20 | 0.936 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 20 impressions, mean 0.94; 6% of logs usable) |
| rerank | clicks | iips | 0.3833 | +0.00039 ± 0.00053 | 0.00527 | 0.94 | 9662 | 0.998 | 99% | 0.94 | yes | ok |
| rerank | clicks | sniips | 0.3833 | +0.00038 ± 0.00050 | 0.00502 | 0.94 | 9662 | 0.998 | 99% | 0.94 | yes | ok |
| rerank | clicks | rips | 0.3833 | -0.00360 ± 0.00335 | 0.03354 | 0.81 | 19 | 0.869 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.87; 5% of logs usable) |
| rerank | clicks | snrips | 0.3833 | -0.00052 ± 0.00363 | 0.03611 | 0.82 | 19 | 0.869 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.87; 5% of logs usable) |
| rerank | clicks | dm-pbm | 0.3833 | +0.01158 ± 0.00024 | 0.01183 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.3833 | -0.03099 ± 0.00023 | 0.03107 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.3833 | +0.00008 ± 0.00045 | 0.00452 | 0.92 | 9662 | 0.998 | 99% | 0.92 | yes | ok |
| rerank | clicks | cdr | 0.3833 | -0.00186 ± 0.00554 | 0.05512 | 0.93 | 19 | 0.869 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.87; 5% of logs usable) |
| rerank | satisfied | naive | 0.2199 | -0.03736 ± 0.00014 | 0.03739 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.2199 | -0.01672 ± 0.01463 | 0.14657 | 0.78 | 20 | 0.936 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 20 impressions, mean 0.94; 6% of logs usable) |
| rerank | satisfied | snips | 0.2199 | +0.00677 ± 0.01134 | 0.11304 | 0.82 | 20 | 0.936 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 20 impressions, mean 0.94; 6% of logs usable) |
| rerank | satisfied | iips | 0.2199 | +0.00037 ± 0.00036 | 0.00361 | 0.93 | 9662 | 0.998 | 99% | 0.93 | yes | ok |
| rerank | satisfied | sniips | 0.2199 | +0.00036 ± 0.00035 | 0.00355 | 0.93 | 9662 | 0.998 | 99% | 0.93 | yes | ok |
| rerank | satisfied | rips | 0.2199 | -0.00016 ± 0.00215 | 0.02136 | 0.80 | 19 | 0.869 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.87; 5% of logs usable) |
| rerank | satisfied | snrips | 0.2199 | +0.00137 ± 0.00238 | 0.02377 | 0.82 | 19 | 0.869 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.87; 5% of logs usable) |
| rerank | satisfied | dm-pbm | 0.2199 | +0.00865 ± 0.00017 | 0.00882 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.2199 | -0.02108 ± 0.00016 | 0.02114 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.2199 | +0.00017 ± 0.00031 | 0.00307 | 0.93 | 9662 | 0.998 | 99% | 0.93 | yes | ok |
| rerank | satisfied | cdr | 0.2199 | +0.00124 ± 0.00311 | 0.03095 | 0.96 | 19 | 0.869 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.87; 5% of logs usable) |
| oracle | clicks | naive | 0.5500 | -0.21804 ± 0.00018 | 0.21805 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.5500 | -0.01248 ± 0.02915 | 0.29031 | 0.77 | 51 | 0.975 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snips | 0.5500 | -0.01524 ± 0.01724 | 0.17222 | 0.83 | 51 | 0.975 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | iips | 0.5500 | +0.00012 ± 0.00068 | 0.00681 | 0.97 | 11235 | 1.000 | 100% | 0.97 | yes | ok |
| oracle | clicks | sniips | 0.5500 | +0.00014 ± 0.00059 | 0.00592 | 0.95 | 11235 | 1.000 | 100% | 0.95 | yes | ok |
| oracle | clicks | rips | 0.5500 | -0.00021 ± 0.00576 | 0.05732 | 0.87 | 51 | 0.951 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snrips | 0.5500 | -0.00039 ± 0.00424 | 0.04218 | 0.89 | 51 | 0.951 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | dm-pbm | 0.5500 | -0.00693 ± 0.00030 | 0.00755 | 0.07 |  |  | 100% | 0.07 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.5500 | -0.14633 ± 0.00023 | 0.14635 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.5500 | +0.00002 ± 0.00054 | 0.00540 | 0.96 | 11235 | 1.000 | 100% | 0.96 | yes | ok |
| oracle | clicks | cdr | 0.5500 | +0.00152 ± 0.00541 | 0.05385 | 0.96 | 51 | 0.951 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | naive | 0.3481 | -0.16559 ± 0.00014 | 0.16559 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.3481 | -0.01090 ± 0.02508 | 0.24981 | 0.77 | 51 | 0.975 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snips | 0.3481 | -0.01545 ± 0.01635 | 0.16337 | 0.78 | 51 | 0.975 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | iips | 0.3481 | -0.00005 ± 0.00055 | 0.00543 | 0.97 | 11235 | 1.000 | 100% | 0.97 | yes | ok |
| oracle | satisfied | sniips | 0.3481 | -0.00003 ± 0.00049 | 0.00486 | 0.93 | 11235 | 1.000 | 100% | 0.93 | yes | ok |
| oracle | satisfied | rips | 0.3481 | -0.00082 ± 0.00560 | 0.05571 | 0.80 | 51 | 0.951 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snrips | 0.3481 | -0.00169 ± 0.00402 | 0.03999 | 0.78 | 51 | 0.951 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | dm-pbm | 0.3481 | -0.00639 ± 0.00026 | 0.00687 | 0.05 |  |  | 100% | 0.05 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.3481 | -0.10871 ± 0.00017 | 0.10873 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.3481 | -0.00011 ± 0.00044 | 0.00435 | 0.97 | 11235 | 1.000 | 100% | 0.97 | yes | ok |
| oracle | satisfied | cdr | 0.3481 | +0.00047 ± 0.00523 | 0.05207 | 0.92 | 51 | 0.951 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| reverse | clicks | naive | 0.2126 | +0.11938 ± 0.00018 | 0.11940 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.2126 | -0.21258 ± 0.00000 | 0.21258 | 0.00 | 0 | 0.865 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.87; 0% of logs usable) |
| reverse | clicks | snips | 0.2126 | -0.21258 ± 0.00000 | 0.21258 | 0.00 | 0 | 0.865 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.87; 0% of logs usable) |
| reverse | clicks | iips | 0.2126 | +0.00033 ± 0.00067 | 0.00670 | 0.96 | 3410 | 1.000 | 100% | 0.96 | yes | ok |
| reverse | clicks | sniips | 0.2126 | +0.00034 ± 0.00067 | 0.00669 | 0.96 | 3410 | 1.000 | 100% | 0.96 | yes | ok |
| reverse | clicks | rips | 0.2126 | +0.00427 ± 0.03039 | 0.30242 | 0.30 | 0 | 0.459 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.46; 0% of logs usable) |
| reverse | clicks | snrips | 0.2126 | -0.01352 ± 0.01372 | 0.13718 | 0.29 | 0 | 0.459 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.46; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.2126 | +0.05524 ± 0.00031 | 0.05532 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.2126 | +0.00594 ± 0.00023 | 0.00637 | 0.03 |  |  | 100% | 0.03 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.2126 | +0.00019 ± 0.00060 | 0.00593 | 0.96 | 3410 | 1.000 | 100% | 0.96 | yes | ok |
| reverse | clicks | cdr | 0.2126 | +0.01272 ± 0.04933 | 0.49101 | 0.75 | 0 | 0.459 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.46; 0% of logs usable) |
| reverse | satisfied | naive | 0.0964 | +0.08616 ± 0.00014 | 0.08617 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0964 | -0.09636 ± 0.00000 | 0.09636 | 0.00 | 0 | 0.865 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.87; 0% of logs usable) |
| reverse | satisfied | snips | 0.0964 | -0.09636 ± 0.00000 | 0.09636 | 0.00 | 0 | 0.865 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.87; 0% of logs usable) |
| reverse | satisfied | iips | 0.0964 | +0.00051 ± 0.00052 | 0.00522 | 0.94 | 3410 | 1.000 | 100% | 0.94 | yes | ok |
| reverse | satisfied | sniips | 0.0964 | +0.00051 ± 0.00052 | 0.00523 | 0.92 | 3410 | 1.000 | 100% | 0.92 | yes | ok |
| reverse | satisfied | rips | 0.0964 | +0.01672 ± 0.02998 | 0.29874 | 0.41 | 0 | 0.459 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.46; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0964 | -0.00615 ± 0.01134 | 0.11301 | 0.38 | 0 | 0.459 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.46; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0964 | +0.03806 ± 0.00017 | 0.03810 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0964 | +0.01421 ± 0.00015 | 0.01429 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0964 | +0.00046 ± 0.00044 | 0.00443 | 0.94 | 3410 | 1.000 | 100% | 0.94 | yes | ok |
| reverse | satisfied | cdr | 0.0964 | +0.02751 ± 0.03351 | 0.33451 | 0.70 | 0 | 0.459 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.46; 0% of logs usable) |
| wide | clicks | naive | 0.3751 | -0.04313 ± 0.00018 | 0.04317 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.3751 | -0.18477 ± 0.01087 | 0.21411 | 0.36 | 15 | 0.461 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snips | 0.3751 | +0.04047 ± 0.01680 | 0.17203 | 0.88 | 15 | 0.461 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.3751 | -0.01303 ± 0.00046 | 0.01382 | 0.18 | 6416 | 0.611 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | sniips | 0.3751 | +0.01934 ± 0.00050 | 0.01996 | 0.03 | 6416 | 0.611 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | rips | 0.3751 | -0.02380 ± 0.00230 | 0.03304 | 0.59 | 14 | 0.651 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snrips | 0.3751 | +0.01832 ± 0.00387 | 0.04267 | 0.86 | 14 | 0.651 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | dm-pbm | 0.3751 | +0.02869 ± 0.00025 | 0.02880 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.3751 | +0.00469 ± 0.00024 | 0.00524 | 0.06 |  |  | 100% | 0.06 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.3751 | +0.01855 ± 0.00039 | 0.01895 | 0.01 | 6416 | 0.611 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | cdr | 0.3751 | +0.05478 ± 0.00218 | 0.05891 | 0.24 | 14 | 0.651 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | naive | 0.2137 | -0.03116 ± 0.00014 | 0.03119 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.2137 | -0.10300 ± 0.00911 | 0.13717 | 0.39 | 15 | 0.461 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | snips | 0.2137 | +0.02793 ± 0.01515 | 0.15332 | 0.80 | 15 | 0.461 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.2137 | -0.00422 ± 0.00033 | 0.00533 | 0.78 | 6416 | 0.611 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | sniips | 0.2137 | +0.01191 ± 0.00035 | 0.01241 | 0.10 | 6416 | 0.611 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | rips | 0.2137 | -0.00814 ± 0.00149 | 0.01693 | 0.64 | 14 | 0.651 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | snrips | 0.2137 | +0.01330 ± 0.00253 | 0.02849 | 0.88 | 14 | 0.651 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.2137 | +0.02049 ± 0.00018 | 0.02057 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.2137 | +0.00126 ± 0.00017 | 0.00214 | 0.42 |  |  | 100% | 0.42 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.2137 | +0.01313 ± 0.00027 | 0.01340 | 0.00 | 6416 | 0.611 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | cdr | 0.2137 | +0.03468 ± 0.00152 | 0.03783 | 0.22 | 14 | 0.651 | 0% |  |  | biased, as expected: 19% of its slots unsupported |

## nfcorpus · pbm · {"policy":"epsilon","epsilon":0.3,"top":10}

100 logs of 100000 impressions, 12 s. Propensities: 10000000 lists, 0 mismatched; chi-square 10856 on 11160 df (p 0.980), largest |z| 3.84.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.3937 | 0.2297 | 0.2037 | 2.412 |
| rerank | 0.0% | 0.3833 | 0.2199 | 0.1993 | 2.415 |
| oracle | 0.0% | 0.5500 | 0.3481 | 0.2917 | 2.363 |
| reverse | 0.0% | 0.2126 | 0.0964 | 0.0822 | 2.485 |
| wide | 18.6% | 0.3751 | 0.2137 | 0.1967 | 2.416 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.3937 | -0.03453 ± 0.00016 | 0.03456 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.3937 | -0.00020 ± 0.00022 | 0.00218 | 0.97 | 69993 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | clicks | snips | 0.3937 | -0.00016 ± 0.00021 | 0.00207 | 0.99 | 69993 | 1.000 | 100% | 0.99 | yes | ok |
| ranking | clicks | iips | 0.3937 | -0.00018 ± 0.00021 | 0.00209 | 1.00 | 72936 | 1.000 | 100% | 1.00 | yes | ok |
| ranking | clicks | sniips | 0.3937 | -0.00015 ± 0.00020 | 0.00203 | 0.97 | 72936 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | clicks | rips | 0.3937 | -0.00018 ± 0.00021 | 0.00213 | 0.98 | 69993 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | clicks | snrips | 0.3937 | -0.00014 ± 0.00020 | 0.00204 | 0.98 | 69993 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | clicks | dm-pbm | 0.3937 | +0.00158 ± 0.00019 | 0.00248 | 0.57 |  |  | 100% | 0.57 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.3937 | -0.00860 ± 0.00018 | 0.00878 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.3937 | -0.00005 ± 0.00021 | 0.00207 | 0.95 | 72936 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | clicks | cdr | 0.3937 | -0.00004 ± 0.00021 | 0.00206 | 0.96 | 69993 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | satisfied | naive | 0.2297 | -0.02623 ± 0.00012 | 0.02625 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.2297 | -0.00011 ± 0.00016 | 0.00158 | 0.99 | 69993 | 1.000 | 100% | 0.99 | yes | ok |
| ranking | satisfied | snips | 0.2297 | -0.00008 ± 0.00015 | 0.00154 | 0.98 | 69993 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | satisfied | iips | 0.2297 | -0.00008 ± 0.00015 | 0.00154 | 0.99 | 72936 | 1.000 | 100% | 0.99 | yes | ok |
| ranking | satisfied | sniips | 0.2297 | -0.00007 ± 0.00015 | 0.00153 | 0.99 | 72936 | 1.000 | 100% | 0.99 | yes | ok |
| ranking | satisfied | rips | 0.2297 | -0.00009 ± 0.00016 | 0.00155 | 0.99 | 69993 | 1.000 | 100% | 0.99 | yes | ok |
| ranking | satisfied | snrips | 0.2297 | -0.00007 ± 0.00015 | 0.00153 | 0.99 | 69993 | 1.000 | 100% | 0.99 | yes | ok |
| ranking | satisfied | dm-pbm | 0.2297 | +0.00264 ± 0.00015 | 0.00301 | 0.31 |  |  | 100% | 0.31 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.2297 | -0.00170 ± 0.00014 | 0.00218 | 0.43 |  |  | 100% | 0.43 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.2297 | +0.00003 ± 0.00015 | 0.00151 | 0.97 | 72936 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | satisfied | cdr | 0.2297 | +0.00002 ± 0.00015 | 0.00150 | 0.97 | 69993 | 1.000 | 100% | 0.97 | yes | ok |
| rerank | clicks | naive | 0.3833 | -0.02410 ± 0.00016 | 0.02415 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.3833 | -0.07976 ± 0.05921 | 0.59454 | 0.22 | 2730 | 0.909 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2730 impressions, mean 0.91; 0% of logs usable) |
| rerank | clicks | snips | 0.3833 | -0.07505 ± 0.04062 | 0.41106 | 0.07 | 2730 | 0.909 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2730 impressions, mean 0.91; 0% of logs usable) |
| rerank | clicks | iips | 0.3833 | -0.00015 ± 0.00088 | 0.00871 | 0.93 | 3999 | 0.998 | 99% | 0.93 | yes | ok |
| rerank | clicks | sniips | 0.3833 | -0.00027 ± 0.00083 | 0.00824 | 0.91 | 3999 | 0.998 | 99% | 0.91 | yes | ok |
| rerank | clicks | rips | 0.3833 | +0.00094 ± 0.01566 | 0.15582 | 0.65 | 3 | 0.749 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.75; 0% of logs usable) |
| rerank | clicks | snrips | 0.3833 | +0.00541 ± 0.01524 | 0.15175 | 0.66 | 3 | 0.749 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.75; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.3833 | +0.01499 ± 0.00024 | 0.01518 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.3833 | -0.00088 ± 0.00023 | 0.00242 | 0.53 |  |  | 100% | 0.53 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.3833 | -0.00033 ± 0.00071 | 0.00705 | 0.95 | 3999 | 0.998 | 99% | 0.95 | yes | ok |
| rerank | clicks | cdr | 0.3833 | +0.00460 ± 0.01715 | 0.17068 | 0.97 | 3 | 0.749 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.75; 0% of logs usable) |
| rerank | satisfied | naive | 0.2199 | -0.01639 ± 0.00012 | 0.01643 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.2199 | -0.07888 ± 0.03696 | 0.37612 | 0.12 | 2730 | 0.909 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2730 impressions, mean 0.91; 0% of logs usable) |
| rerank | satisfied | snips | 0.2199 | -0.07591 ± 0.02540 | 0.26392 | 0.04 | 2730 | 0.909 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2730 impressions, mean 0.91; 0% of logs usable) |
| rerank | satisfied | iips | 0.2199 | -0.00018 ± 0.00063 | 0.00624 | 0.94 | 3999 | 0.998 | 99% | 0.94 | yes | ok |
| rerank | satisfied | sniips | 0.2199 | -0.00027 ± 0.00060 | 0.00598 | 0.96 | 3999 | 0.998 | 99% | 0.96 | yes | ok |
| rerank | satisfied | rips | 0.2199 | -0.00612 ± 0.00545 | 0.05454 | 0.63 | 3 | 0.749 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.75; 0% of logs usable) |
| rerank | satisfied | snrips | 0.2199 | -0.00334 ± 0.00652 | 0.06497 | 0.64 | 3 | 0.749 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.75; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.2199 | +0.01135 ± 0.00018 | 0.01149 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.2199 | +0.00136 ± 0.00016 | 0.00212 | 0.50 |  |  | 100% | 0.50 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.2199 | -0.00032 ± 0.00054 | 0.00538 | 0.93 | 3999 | 0.998 | 99% | 0.93 | yes | ok |
| rerank | satisfied | cdr | 0.2199 | -0.00279 ± 0.00659 | 0.06561 | 0.95 | 3 | 0.749 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.75; 0% of logs usable) |
| oracle | clicks | naive | 0.5500 | -0.19083 ± 0.00016 | 0.19084 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.5500 | -0.16604 ± 0.07216 | 0.73696 | 0.15 | 24281 | 0.889 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 24281 impressions, mean 0.89; 0% of logs usable) |
| oracle | clicks | snips | 0.5500 | -0.21956 ± 0.02815 | 0.35590 | 0.11 | 24281 | 0.889 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 24281 impressions, mean 0.89; 0% of logs usable) |
| oracle | clicks | iips | 0.5500 | +0.00061 ± 0.00106 | 0.01061 | 0.92 | 7331 | 1.001 | 99% | 0.93 | yes | ok |
| oracle | clicks | sniips | 0.5500 | +0.00046 ± 0.00087 | 0.00869 | 0.93 | 7331 | 1.001 | 99% | 0.93 | yes | ok |
| oracle | clicks | rips | 0.5500 | -0.00027 ± 0.00754 | 0.07503 | 0.79 | 14 | 0.668 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 14 impressions, mean 0.67; 0% of logs usable) |
| oracle | clicks | snrips | 0.5500 | -0.00000 ± 0.00643 | 0.06398 | 0.79 | 14 | 0.668 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 14 impressions, mean 0.67; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.5500 | -0.00793 ± 0.00028 | 0.00841 | 0.04 |  |  | 100% | 0.04 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.5500 | -0.12238 ± 0.00023 | 0.12240 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.5500 | +0.00045 ± 0.00077 | 0.00768 | 0.93 | 7331 | 1.001 | 99% | 0.93 | yes | ok |
| oracle | clicks | cdr | 0.5500 | +0.00372 ± 0.00759 | 0.07559 | 0.89 | 14 | 0.668 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 14 impressions, mean 0.67; 0% of logs usable) |
| oracle | satisfied | naive | 0.3481 | -0.14461 ± 0.00012 | 0.14462 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.3481 | -0.12083 ± 0.04904 | 0.50272 | 0.12 | 24281 | 0.889 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 24281 impressions, mean 0.89; 0% of logs usable) |
| oracle | satisfied | snips | 0.3481 | -0.16286 ± 0.02092 | 0.26432 | 0.10 | 24281 | 0.889 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 24281 impressions, mean 0.89; 0% of logs usable) |
| oracle | satisfied | iips | 0.3481 | +0.00043 ± 0.00090 | 0.00900 | 0.92 | 7331 | 1.001 | 99% | 0.92 | yes | ok |
| oracle | satisfied | sniips | 0.3481 | +0.00035 ± 0.00077 | 0.00770 | 0.95 | 7331 | 1.001 | 99% | 0.95 | yes | ok |
| oracle | satisfied | rips | 0.3481 | +0.00009 ± 0.00485 | 0.04830 | 0.72 | 14 | 0.668 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 14 impressions, mean 0.67; 0% of logs usable) |
| oracle | satisfied | snrips | 0.3481 | +0.00036 ± 0.00470 | 0.04680 | 0.73 | 14 | 0.668 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 14 impressions, mean 0.67; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.3481 | -0.00692 ± 0.00024 | 0.00731 | 0.04 |  |  | 100% | 0.04 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.3481 | -0.08741 ± 0.00018 | 0.08743 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.3481 | +0.00034 ± 0.00065 | 0.00647 | 0.96 | 7331 | 1.001 | 99% | 0.96 | yes | ok |
| oracle | satisfied | cdr | 0.3481 | +0.00304 ± 0.00527 | 0.05255 | 0.89 | 14 | 0.668 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 14 impressions, mean 0.67; 0% of logs usable) |
| reverse | clicks | naive | 0.2126 | +0.14660 ± 0.00016 | 0.14661 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.2126 | -0.05130 ± 0.04231 | 0.42407 | 0.14 | 1 | 1.179 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.18; 0% of logs usable) |
| reverse | clicks | snips | 0.2126 | -0.10591 ± 0.02818 | 0.29973 | 0.07 | 1 | 1.179 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.18; 0% of logs usable) |
| reverse | clicks | iips | 0.2126 | +0.00077 ± 0.00090 | 0.00902 | 0.95 | 2945 | 1.006 | 98% | 0.95 | yes | ok |
| reverse | clicks | sniips | 0.2126 | +0.00026 ± 0.00087 | 0.00869 | 0.95 | 2945 | 1.006 | 98% | 0.95 | yes | ok |
| reverse | clicks | rips | 0.2126 | -0.00180 ± 0.01227 | 0.12212 | 0.68 | 1 | 0.841 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.84; 0% of logs usable) |
| reverse | clicks | snrips | 0.2126 | -0.01376 ± 0.00761 | 0.07696 | 0.68 | 1 | 0.841 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.84; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.2126 | +0.07425 ± 0.00035 | 0.07433 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.2126 | +0.02771 ± 0.00032 | 0.02789 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.2126 | -0.00046 ± 0.00083 | 0.00825 | 0.95 | 2945 | 1.006 | 98% | 0.96 | yes | ok |
| reverse | clicks | cdr | 0.2126 | -0.01027 ± 0.01340 | 0.13371 | 0.95 | 1 | 0.841 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.84; 0% of logs usable) |
| reverse | satisfied | naive | 0.0964 | +0.10713 ± 0.00012 | 0.10714 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0964 | -0.04596 ± 0.02208 | 0.22444 | 0.05 | 1 | 1.179 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.18; 0% of logs usable) |
| reverse | satisfied | snips | 0.0964 | -0.06969 ± 0.01291 | 0.14618 | 0.04 | 1 | 1.179 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.18; 0% of logs usable) |
| reverse | satisfied | iips | 0.0964 | +0.00058 ± 0.00059 | 0.00590 | 0.93 | 2945 | 1.006 | 98% | 0.93 | yes | ok |
| reverse | satisfied | sniips | 0.0964 | +0.00033 ± 0.00057 | 0.00563 | 0.93 | 2945 | 1.006 | 98% | 0.93 | yes | ok |
| reverse | satisfied | rips | 0.0964 | +0.00449 ± 0.01096 | 0.10919 | 0.62 | 1 | 0.841 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.84; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0964 | -0.00345 ± 0.00624 | 0.06219 | 0.58 | 1 | 0.841 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.84; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0964 | +0.05278 ± 0.00022 | 0.05283 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0964 | +0.02878 ± 0.00021 | 0.02886 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0964 | -0.00012 ± 0.00052 | 0.00515 | 0.96 | 2945 | 1.006 | 98% | 0.96 | yes | ok |
| reverse | satisfied | cdr | 0.0964 | +0.00144 ± 0.01150 | 0.11448 | 0.97 | 1 | 0.841 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.84; 0% of logs usable) |
| wide | clicks | naive | 0.3751 | -0.01592 ± 0.00016 | 0.01600 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.3751 | -0.25117 ± 0.03762 | 0.45082 | 0.09 | 3399 | 0.390 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snips | 0.3751 | -0.13373 ± 0.03123 | 0.33826 | 0.01 | 3399 | 0.390 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | iips | 0.3751 | -0.01391 ± 0.00083 | 0.01620 | 0.55 | 2527 | 0.608 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | sniips | 0.3751 | +0.01817 ± 0.00093 | 0.02040 | 0.40 | 2527 | 0.608 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | rips | 0.3751 | -0.02155 ± 0.01162 | 0.11756 | 0.49 | 5 | 0.272 | 0% |  |  | unbiased though unsupported |
| wide | clicks | snrips | 0.3751 | +0.01920 ± 0.01425 | 0.14311 | 0.68 | 5 | 0.272 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.3751 | +0.03187 ± 0.00024 | 0.03196 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.3751 | +0.03976 ± 0.00022 | 0.03982 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.3751 | +0.01760 ± 0.00068 | 0.01884 | 0.19 | 2527 | 0.608 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | cdr | 0.3751 | +0.05742 ± 0.01316 | 0.14297 | 0.62 | 5 | 0.272 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | naive | 0.2137 | -0.01019 ± 0.00012 | 0.01026 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.2137 | -0.14762 ± 0.02406 | 0.28123 | 0.06 | 3399 | 0.390 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | snips | 0.2137 | -0.10068 ± 0.01985 | 0.22170 | 0.01 | 3399 | 0.390 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | iips | 0.2137 | -0.00485 ± 0.00058 | 0.00751 | 0.79 | 2527 | 0.608 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | sniips | 0.2137 | +0.01118 ± 0.00062 | 0.01280 | 0.49 | 2527 | 0.608 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | rips | 0.2137 | -0.01024 ± 0.00454 | 0.04627 | 0.49 | 5 | 0.272 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.2137 | +0.01200 ± 0.00884 | 0.08877 | 0.57 | 5 | 0.272 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | dm-pbm | 0.2137 | +0.02352 ± 0.00018 | 0.02359 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.2137 | +0.02668 ± 0.00017 | 0.02673 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.2137 | +0.01293 ± 0.00049 | 0.01381 | 0.17 | 2527 | 0.608 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | cdr | 0.2137 | +0.03417 ± 0.00570 | 0.06618 | 0.64 | 5 | 0.272 | 0% |  |  | biased, as expected: 19% of its slots unsupported |

## nfcorpus · pbm · {"policy":"shuffle","top":10}

100 logs of 100000 impressions, 12 s. Propensities: 10000000 lists, 0 mismatched; chi-square 11313 on 11160 df (p 0.153), largest |z| 4.17.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.3937 | 0.2297 | 0.2037 | 2.412 |
| rerank | 0.0% | 0.3833 | 0.2199 | 0.1993 | 2.415 |
| oracle | 0.0% | 0.5500 | 0.3481 | 0.2917 | 2.363 |
| reverse | 0.0% | 0.2126 | 0.0964 | 0.0822 | 2.485 |
| wide | 18.6% | 0.3751 | 0.2137 | 0.1967 | 2.416 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.3937 | -0.11482 ± 0.00016 | 0.11483 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.3937 | +0.05385 ± 0.04393 | 0.44044 | 0.72 | 3 | 1.049 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.05; 0% of logs usable) |
| ranking | clicks | snips | 0.3937 | +0.02287 ± 0.03854 | 0.38419 | 0.66 | 3 | 1.049 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.05; 0% of logs usable) |
| ranking | clicks | iips | 0.3937 | +0.00011 ± 0.00057 | 0.00564 | 0.97 | 9889 | 1.000 | 98% | 0.97 | yes | ok |
| ranking | clicks | sniips | 0.3937 | +0.00026 ± 0.00053 | 0.00528 | 0.97 | 9889 | 1.000 | 98% | 0.97 | yes | ok |
| ranking | clicks | rips | 0.3937 | +0.00533 ± 0.00910 | 0.09065 | 0.70 | 3 | 0.941 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| ranking | clicks | snrips | 0.3937 | -0.00050 ± 0.00801 | 0.07967 | 0.70 | 3 | 0.941 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| ranking | clicks | dm-pbm | 0.3937 | +0.00863 ± 0.00029 | 0.00909 | 0.01 |  |  | 100% | 0.01 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.3937 | -0.05689 ± 0.00021 | 0.05693 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.3937 | -0.00005 ± 0.00052 | 0.00518 | 0.97 | 9889 | 1.000 | 98% | 0.97 | yes | ok |
| ranking | clicks | cdr | 0.3937 | +0.00900 ± 0.00900 | 0.09001 | 0.97 | 3 | 0.941 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| ranking | satisfied | naive | 0.2297 | -0.08751 ± 0.00012 | 0.08752 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.2297 | +0.05151 ± 0.03577 | 0.35960 | 0.53 | 3 | 1.049 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.05; 0% of logs usable) |
| ranking | satisfied | snips | 0.2297 | +0.02480 ± 0.03321 | 0.33134 | 0.49 | 3 | 1.049 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.05; 0% of logs usable) |
| ranking | satisfied | iips | 0.2297 | +0.00056 ± 0.00046 | 0.00461 | 0.96 | 9889 | 1.000 | 98% | 0.96 | yes | ok |
| ranking | satisfied | sniips | 0.2297 | +0.00066 ± 0.00046 | 0.00459 | 0.96 | 9889 | 1.000 | 98% | 0.96 | yes | ok |
| ranking | satisfied | rips | 0.2297 | -0.00216 ± 0.00558 | 0.05559 | 0.75 | 3 | 0.941 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| ranking | satisfied | snrips | 0.2297 | -0.00554 ± 0.00382 | 0.03839 | 0.75 | 3 | 0.941 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| ranking | satisfied | dm-pbm | 0.2297 | +0.00215 ± 0.00021 | 0.00301 | 0.32 |  |  | 100% | 0.32 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.2297 | -0.04005 ± 0.00016 | 0.04008 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.2297 | +0.00039 ± 0.00044 | 0.00438 | 0.92 | 9889 | 1.000 | 98% | 0.92 | yes | ok |
| ranking | satisfied | cdr | 0.2297 | +0.00005 ± 0.00561 | 0.05581 | 0.92 | 3 | 0.941 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| rerank | clicks | naive | 0.3833 | -0.10439 ± 0.00016 | 0.10440 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.3833 | -0.01132 ± 0.03936 | 0.39178 | 0.68 | 3 | 0.940 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| rerank | clicks | snips | 0.3833 | +0.00835 ± 0.03754 | 0.37357 | 0.58 | 3 | 0.940 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| rerank | clicks | iips | 0.3833 | +0.00084 ± 0.00064 | 0.00643 | 0.96 | 9879 | 0.997 | 98% | 0.96 | yes | ok |
| rerank | clicks | sniips | 0.3833 | +0.00070 ± 0.00058 | 0.00582 | 0.95 | 9879 | 0.997 | 98% | 0.95 | yes | ok |
| rerank | clicks | rips | 0.3833 | -0.01505 ± 0.00577 | 0.05940 | 0.62 | 3 | 0.902 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.90; 0% of logs usable) |
| rerank | clicks | snrips | 0.3833 | -0.01458 ± 0.00500 | 0.05186 | 0.63 | 3 | 0.902 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.90; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.3833 | +0.01139 ± 0.00031 | 0.01179 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.3833 | -0.05581 ± 0.00021 | 0.05585 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.3833 | +0.00031 ± 0.00049 | 0.00488 | 0.96 | 9879 | 0.997 | 98% | 0.96 | yes | ok |
| rerank | clicks | cdr | 0.3833 | -0.01196 ± 0.00707 | 0.07135 | 0.96 | 3 | 0.902 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.90; 0% of logs usable) |
| rerank | satisfied | naive | 0.2199 | -0.07767 ± 0.00012 | 0.07768 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.2199 | -0.02030 ± 0.02692 | 0.26859 | 0.46 | 3 | 0.940 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| rerank | satisfied | snips | 0.2199 | -0.02764 ± 0.02668 | 0.26687 | 0.42 | 3 | 0.940 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| rerank | satisfied | iips | 0.2199 | +0.00106 ± 0.00048 | 0.00488 | 0.96 | 9879 | 0.997 | 98% | 0.96 | yes | ok |
| rerank | satisfied | sniips | 0.2199 | +0.00097 ± 0.00044 | 0.00448 | 0.97 | 9879 | 0.997 | 98% | 0.97 | yes | ok |
| rerank | satisfied | rips | 0.2199 | -0.00849 ± 0.00369 | 0.03770 | 0.71 | 3 | 0.902 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.90; 0% of logs usable) |
| rerank | satisfied | snrips | 0.2199 | -0.00976 ± 0.00256 | 0.02728 | 0.69 | 3 | 0.902 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.90; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.2199 | +0.00481 ± 0.00021 | 0.00526 | 0.06 |  |  | 100% | 0.06 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.2199 | -0.04001 ± 0.00016 | 0.04004 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.2199 | +0.00056 ± 0.00036 | 0.00366 | 0.97 | 9879 | 0.997 | 98% | 0.97 | yes | ok |
| rerank | satisfied | cdr | 0.2199 | -0.00792 ± 0.00445 | 0.04502 | 0.96 | 3 | 0.902 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.90; 0% of logs usable) |
| oracle | clicks | naive | 0.5500 | -0.27112 ± 0.00016 | 0.27113 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.5500 | -0.01174 ± 0.05378 | 0.53527 | 0.67 | 3 | 1.013 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.01; 0% of logs usable) |
| oracle | clicks | snips | 0.5500 | -0.08283 ± 0.04530 | 0.45823 | 0.56 | 3 | 1.013 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.01; 0% of logs usable) |
| oracle | clicks | iips | 0.5500 | +0.00066 ± 0.00074 | 0.00743 | 0.97 | 9887 | 1.000 | 100% | 0.97 | yes | ok |
| oracle | clicks | sniips | 0.5500 | +0.00051 ± 0.00072 | 0.00714 | 0.94 | 9887 | 1.000 | 100% | 0.94 | yes | ok |
| oracle | clicks | rips | 0.5500 | +0.01419 ± 0.00971 | 0.09764 | 0.76 | 3 | 0.968 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.97; 0% of logs usable) |
| oracle | clicks | snrips | 0.5500 | +0.00771 ± 0.00768 | 0.07678 | 0.76 | 3 | 0.968 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.97; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.5500 | -0.00462 ± 0.00032 | 0.00562 | 0.28 |  |  | 100% | 0.28 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.5500 | -0.16916 ± 0.00022 | 0.16918 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.5500 | +0.00032 ± 0.00061 | 0.00613 | 0.94 | 9887 | 1.000 | 100% | 0.94 | yes | ok |
| oracle | clicks | cdr | 0.5500 | +0.01490 ± 0.00893 | 0.09013 | 0.94 | 3 | 0.968 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.97; 0% of logs usable) |
| oracle | satisfied | naive | 0.3481 | -0.20589 ± 0.00012 | 0.20590 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.3481 | +0.00872 ± 0.04263 | 0.42427 | 0.58 | 3 | 1.013 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.01; 0% of logs usable) |
| oracle | satisfied | snips | 0.3481 | -0.03854 ± 0.03636 | 0.36386 | 0.53 | 3 | 1.013 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.01; 0% of logs usable) |
| oracle | satisfied | iips | 0.3481 | +0.00115 ± 0.00060 | 0.00610 | 0.97 | 9887 | 1.000 | 100% | 0.97 | yes | ok |
| oracle | satisfied | sniips | 0.3481 | +0.00104 ± 0.00061 | 0.00611 | 0.91 | 9887 | 1.000 | 100% | 0.91 | yes | ok |
| oracle | satisfied | rips | 0.3481 | +0.00699 ± 0.00704 | 0.07035 | 0.77 | 3 | 0.968 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.97; 0% of logs usable) |
| oracle | satisfied | snrips | 0.3481 | +0.00288 ± 0.00525 | 0.05233 | 0.74 | 3 | 0.968 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.97; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.3481 | -0.00920 ± 0.00027 | 0.00959 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.3481 | -0.12613 ± 0.00017 | 0.12614 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.3481 | +0.00088 ± 0.00051 | 0.00518 | 0.94 | 9887 | 1.000 | 100% | 0.94 | yes | ok |
| oracle | satisfied | cdr | 0.3481 | +0.00780 ± 0.00652 | 0.06537 | 0.91 | 3 | 0.968 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.97; 0% of logs usable) |
| reverse | clicks | naive | 0.2126 | +0.06630 ± 0.00016 | 0.06632 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.2126 | +0.00515 ± 0.02918 | 0.29035 | 0.44 | 3 | 1.055 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.06; 0% of logs usable) |
| reverse | clicks | snips | 0.2126 | -0.02327 ± 0.02687 | 0.26840 | 0.42 | 3 | 1.055 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.06; 0% of logs usable) |
| reverse | clicks | iips | 0.2126 | +0.00010 ± 0.00048 | 0.00477 | 0.95 | 9895 | 1.001 | 98% | 0.95 | yes | ok |
| reverse | clicks | sniips | 0.2126 | -0.00017 ± 0.00046 | 0.00460 | 0.93 | 9895 | 1.001 | 98% | 0.93 | yes | ok |
| reverse | clicks | rips | 0.2126 | +0.02082 ± 0.01178 | 0.11904 | 0.69 | 3 | 0.942 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| reverse | clicks | snrips | 0.2126 | +0.01861 ± 0.01195 | 0.12035 | 0.66 | 3 | 0.942 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.2126 | +0.02628 ± 0.00024 | 0.02639 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.2126 | +0.01864 ± 0.00020 | 0.01875 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.2126 | -0.00018 ± 0.00043 | 0.00430 | 0.96 | 9895 | 1.001 | 98% | 0.96 | yes | ok |
| reverse | clicks | cdr | 0.2126 | +0.02125 ± 0.01243 | 0.12552 | 0.97 | 3 | 0.942 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| reverse | satisfied | naive | 0.0964 | +0.04585 ± 0.00012 | 0.04587 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0964 | +0.00343 ± 0.02064 | 0.20537 | 0.24 | 3 | 1.055 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.06; 0% of logs usable) |
| reverse | satisfied | snips | 0.0964 | -0.00800 ± 0.01951 | 0.19424 | 0.22 | 3 | 1.055 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.06; 0% of logs usable) |
| reverse | satisfied | iips | 0.0964 | +0.00034 ± 0.00028 | 0.00281 | 0.97 | 9895 | 1.001 | 98% | 0.97 | yes | ok |
| reverse | satisfied | sniips | 0.0964 | +0.00021 ± 0.00027 | 0.00273 | 0.98 | 9895 | 1.001 | 98% | 0.98 | yes | ok |
| reverse | satisfied | rips | 0.0964 | +0.00015 ± 0.00534 | 0.05315 | 0.67 | 3 | 0.942 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0964 | -0.00204 ± 0.00421 | 0.04191 | 0.67 | 3 | 0.942 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0964 | +0.01762 ± 0.00015 | 0.01769 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0964 | +0.01627 ± 0.00014 | 0.01633 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0964 | +0.00016 ± 0.00026 | 0.00256 | 0.99 | 9895 | 1.001 | 98% | 0.99 | yes | ok |
| reverse | satisfied | cdr | 0.0964 | +0.00091 ± 0.00603 | 0.06005 | 0.99 | 3 | 0.942 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| wide | clicks | naive | 0.3751 | -0.09621 ± 0.00016 | 0.09622 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.3751 | -0.16039 ± 0.03253 | 0.36126 | 0.43 | 2 | 0.532 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snips | 0.3751 | -0.05493 ± 0.04472 | 0.44836 | 0.27 | 2 | 0.532 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.3751 | -0.01312 ± 0.00063 | 0.01454 | 0.46 | 6101 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | sniips | 0.3751 | +0.01917 ± 0.00063 | 0.02016 | 0.12 | 6101 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | rips | 0.3751 | -0.03145 ± 0.00481 | 0.05728 | 0.48 | 2 | 0.631 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snrips | 0.3751 | +0.00245 ± 0.00666 | 0.06628 | 0.75 | 2 | 0.631 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.3751 | +0.02543 ± 0.00031 | 0.02562 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.3751 | -0.03075 ± 0.00023 | 0.03083 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.3751 | +0.01493 ± 0.00050 | 0.01574 | 0.16 | 6101 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | cdr | 0.3751 | +0.03605 ± 0.00581 | 0.06814 | 0.81 | 2 | 0.631 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | naive | 0.2137 | -0.07147 ± 0.00012 | 0.07148 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.2137 | -0.10784 ± 0.02249 | 0.24839 | 0.24 | 2 | 0.532 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | snips | 0.2137 | -0.07568 ± 0.02922 | 0.30038 | 0.19 | 2 | 0.532 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.2137 | -0.00398 ± 0.00048 | 0.00618 | 0.85 | 6101 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | sniips | 0.2137 | +0.01195 ± 0.00048 | 0.01287 | 0.26 | 6101 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | rips | 0.2137 | -0.01300 ± 0.00358 | 0.03792 | 0.61 | 2 | 0.631 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.2137 | +0.00110 ± 0.00349 | 0.03479 | 0.83 | 2 | 0.631 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | dm-pbm | 0.2137 | +0.01405 ± 0.00022 | 0.01421 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.2137 | -0.02503 ± 0.00017 | 0.02508 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.2137 | +0.01011 ± 0.00038 | 0.01078 | 0.22 | 6101 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | cdr | 0.2137 | +0.02106 ± 0.00409 | 0.04584 | 0.78 | 2 | 0.631 | 0% |  |  | biased, as expected: 19% of its slots unsupported |

## nfcorpus · pbm · {"policy":"swap","top":10}

100 logs of 100000 impressions, 12 s. Propensities: 10000000 lists, 0 mismatched; chi-square 3429 on 3224 df (p 0.006), largest |z| 3.23.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.3937 | 0.2297 | 0.2037 | 2.412 |
| rerank | 50.4% | 0.3833 | 0.2199 | 0.1993 | 2.415 |
| oracle | 24.9% | 0.5500 | 0.3481 | 0.2917 | 2.363 |
| reverse | 80.0% | 0.2126 | 0.0964 | 0.0822 | 2.485 |
| wide | 54.7% | 0.3751 | 0.2137 | 0.1967 | 2.416 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.3937 | -0.07467 ± 0.00018 | 0.07469 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.3937 | -0.00016 ± 0.00065 | 0.00642 | 0.96 | 10004 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | clicks | snips | 0.3937 | -0.00031 ± 0.00058 | 0.00579 | 0.97 | 10004 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | clicks | iips | 0.3937 | -0.00042 ± 0.00042 | 0.00420 | 0.99 | 10004 | 1.001 | 98% | 0.99 | yes | ok |
| ranking | clicks | sniips | 0.3937 | -0.00052 ± 0.00039 | 0.00393 | 0.99 | 10004 | 1.001 | 98% | 0.99 | yes | ok |
| ranking | clicks | rips | 0.3937 | -0.00016 ± 0.00065 | 0.00642 | 0.96 | 10004 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | clicks | snrips | 0.3937 | -0.00031 ± 0.00058 | 0.00579 | 0.97 | 10004 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | clicks | dm-pbm | 0.3937 | +0.00936 ± 0.00026 | 0.00972 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.3937 | -0.06955 ± 0.00019 | 0.06958 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.3937 | -0.00008 ± 0.00036 | 0.00354 | 0.96 | 10004 | 1.001 | 98% | 0.96 | yes | ok |
| ranking | clicks | cdr | 0.3937 | +0.00006 ± 0.00052 | 0.00513 | 0.97 | 10004 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | satisfied | naive | 0.2297 | -0.05850 ± 0.00014 | 0.05852 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.2297 | -0.00017 ± 0.00045 | 0.00451 | 0.98 | 10004 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | satisfied | snips | 0.2297 | -0.00026 ± 0.00042 | 0.00423 | 0.96 | 10004 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | satisfied | iips | 0.2297 | -0.00054 ± 0.00034 | 0.00346 | 0.96 | 10004 | 1.001 | 98% | 0.96 | yes | ok |
| ranking | satisfied | sniips | 0.2297 | -0.00060 ± 0.00034 | 0.00341 | 0.94 | 10004 | 1.001 | 98% | 0.94 | yes | ok |
| ranking | satisfied | rips | 0.2297 | -0.00017 ± 0.00045 | 0.00451 | 0.98 | 10004 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | satisfied | snrips | 0.2297 | -0.00026 ± 0.00042 | 0.00423 | 0.96 | 10004 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | satisfied | dm-pbm | 0.2297 | +0.00482 ± 0.00022 | 0.00530 | 0.06 |  |  | 100% | 0.06 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.2297 | -0.04273 ± 0.00016 | 0.04276 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.2297 | -0.00013 ± 0.00029 | 0.00290 | 0.95 | 10004 | 1.001 | 98% | 0.95 | yes | ok |
| ranking | satisfied | cdr | 0.2297 | +0.00014 ± 0.00038 | 0.00381 | 0.96 | 10004 | 1.000 | 100% | 0.96 | yes | ok |
| rerank | clicks | naive | 0.3833 | -0.06424 ± 0.00018 | 0.06427 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.3833 | -0.36127 ± 0.00015 | 0.36127 | 0.00 | 964 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | snips | 0.3833 | -0.15505 ± 0.00146 | 0.15574 | 0.00 | 964 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | iips | 0.3833 | -0.07855 ± 0.00044 | 0.07867 | 0.00 | 9997 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | sniips | 0.3833 | +0.01839 ± 0.00050 | 0.01905 | 0.05 | 9997 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | rips | 0.3833 | -0.09899 ± 0.00046 | 0.09910 | 0.00 | 964 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | snrips | 0.3833 | -0.01652 ± 0.00069 | 0.01787 | 0.45 | 964 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | dm-pbm | 0.3833 | +0.01322 ± 0.00024 | 0.01343 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.3833 | -0.01257 ± 0.00026 | 0.01283 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.3833 | +0.00588 ± 0.00039 | 0.00703 | 0.64 | 9997 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | cdr | 0.3833 | +0.09805 ± 0.00041 | 0.09813 | 0.00 | 964 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | naive | 0.2199 | -0.04866 ± 0.00014 | 0.04868 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.2199 | -0.20971 ± 0.00011 | 0.20971 | 0.00 | 964 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | snips | 0.2199 | -0.11436 ± 0.00116 | 0.11495 | 0.00 | 964 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | iips | 0.2199 | -0.03643 ± 0.00035 | 0.03659 | 0.00 | 9997 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | sniips | 0.2199 | +0.01476 ± 0.00041 | 0.01532 | 0.06 | 9997 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | rips | 0.2199 | -0.04922 ± 0.00038 | 0.04936 | 0.00 | 964 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | snrips | 0.2199 | -0.01318 ± 0.00053 | 0.01419 | 0.35 | 964 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | dm-pbm | 0.2199 | +0.00753 ± 0.00020 | 0.00779 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.2199 | -0.01655 ± 0.00017 | 0.01664 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.2199 | +0.00486 ± 0.00030 | 0.00571 | 0.58 | 9997 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | cdr | 0.2199 | +0.05269 ± 0.00035 | 0.05281 | 0.00 | 964 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| oracle | clicks | naive | 0.5500 | -0.23098 ± 0.00018 | 0.23099 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.5500 | -0.37182 ± 0.00040 | 0.37184 | 0.00 | 5577 | 0.558 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | snips | 0.5500 | -0.23048 ± 0.00066 | 0.23058 | 0.00 | 5577 | 0.558 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | iips | 0.5500 | -0.10435 ± 0.00049 | 0.10446 | 0.00 | 10002 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | sniips | 0.5500 | -0.06965 ± 0.00050 | 0.06982 | 0.00 | 10002 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | rips | 0.5500 | -0.11015 ± 0.00059 | 0.11030 | 0.00 | 5577 | 0.558 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | snrips | 0.5500 | -0.06387 ± 0.00071 | 0.06426 | 0.00 | 5577 | 0.558 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | dm-pbm | 0.5500 | -0.00368 ± 0.00029 | 0.00470 | 0.33 |  |  | 100% | 0.33 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.5500 | -0.13739 ± 0.00024 | 0.13741 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.5500 | -0.00338 ± 0.00042 | 0.00537 | 0.87 | 10002 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | cdr | 0.5500 | +0.02692 ± 0.00053 | 0.02742 | 0.00 | 5577 | 0.558 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | naive | 0.3481 | -0.17689 ± 0.00014 | 0.17689 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.3481 | -0.24889 ± 0.00031 | 0.24890 | 0.00 | 5577 | 0.558 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | snips | 0.3481 | -0.17019 ± 0.00053 | 0.17027 | 0.00 | 5577 | 0.558 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | iips | 0.3481 | -0.06577 ± 0.00044 | 0.06592 | 0.00 | 10002 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | sniips | 0.3481 | -0.05107 ± 0.00043 | 0.05126 | 0.00 | 10002 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | rips | 0.3481 | -0.06722 ± 0.00050 | 0.06741 | 0.00 | 5577 | 0.558 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | snrips | 0.3481 | -0.04694 ± 0.00056 | 0.04727 | 0.00 | 5577 | 0.558 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | dm-pbm | 0.3481 | -0.00691 ± 0.00028 | 0.00747 | 0.05 |  |  | 100% | 0.05 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.3481 | -0.10147 ± 0.00022 | 0.10149 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.3481 | -0.00300 ± 0.00038 | 0.00483 | 0.85 | 10002 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | cdr | 0.3481 | +0.01363 ± 0.00046 | 0.01438 | 0.12 | 5577 | 0.558 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| reverse | clicks | naive | 0.2126 | +0.10645 ± 0.00018 | 0.10646 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.2126 | -0.21258 ± 0.00000 | 0.21258 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snips | 0.2126 | -0.21258 ± 0.00000 | 0.21258 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | iips | 0.2126 | -0.10725 ± 0.00030 | 0.10729 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | sniips | 0.2126 | -0.10721 ± 0.00029 | 0.10725 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | rips | 0.2126 | -0.10725 ± 0.00030 | 0.10729 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snrips | 0.2126 | -0.10721 ± 0.00029 | 0.10725 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | dm-pbm | 0.2126 | +0.04980 ± 0.00029 | 0.04989 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.2126 | +0.19606 ± 0.00043 | 0.19611 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.2126 | +0.02193 ± 0.00032 | 0.02216 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | cdr | 0.2126 | +0.19887 ± 0.00044 | 0.19892 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | naive | 0.0964 | +0.07486 ± 0.00014 | 0.07487 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0964 | -0.09636 ± 0.00000 | 0.09636 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snips | 0.0964 | -0.09636 ± 0.00000 | 0.09636 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | iips | 0.0964 | -0.04859 ± 0.00020 | 0.04863 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | sniips | 0.0964 | -0.04857 ± 0.00019 | 0.04861 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | rips | 0.0964 | -0.04859 ± 0.00020 | 0.04863 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snrips | 0.0964 | -0.04857 ± 0.00019 | 0.04861 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | dm-pbm | 0.0964 | +0.03464 ± 0.00017 | 0.03469 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0964 | +0.10392 ± 0.00027 | 0.10395 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0964 | +0.01575 ± 0.00019 | 0.01586 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | cdr | 0.0964 | +0.09944 ± 0.00028 | 0.09948 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| wide | clicks | naive | 0.3751 | -0.05606 ± 0.00018 | 0.05609 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.3751 | -0.36216 ± 0.00012 | 0.36216 | 0.00 | 679 | 0.068 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | snips | 0.3751 | -0.18435 ± 0.00174 | 0.18516 | 0.00 | 679 | 0.068 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | iips | 0.3751 | -0.07664 ± 0.00045 | 0.07677 | 0.00 | 9889 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | sniips | 0.3751 | +0.03242 ± 0.00051 | 0.03282 | 0.00 | 9889 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | rips | 0.3751 | -0.09527 ± 0.00047 | 0.09538 | 0.00 | 679 | 0.068 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | snrips | 0.3751 | -0.00342 ± 0.00072 | 0.00794 | 0.97 | 679 | 0.068 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.3751 | +0.03019 ± 0.00025 | 0.03028 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.3751 | +0.00614 ± 0.00023 | 0.00657 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.3751 | +0.02349 ± 0.00039 | 0.02382 | 0.00 | 9889 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | cdr | 0.3751 | +0.11706 ± 0.00040 | 0.11713 | 0.00 | 679 | 0.068 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | naive | 0.2137 | -0.04246 ± 0.00014 | 0.04248 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.2137 | -0.20813 ± 0.00009 | 0.20813 | 0.00 | 679 | 0.068 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | snips | 0.2137 | -0.13179 ± 0.00131 | 0.13244 | 0.00 | 679 | 0.068 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | iips | 0.2137 | -0.03267 ± 0.00035 | 0.03286 | 0.00 | 9889 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | sniips | 0.2137 | +0.02707 ± 0.00042 | 0.02739 | 0.00 | 9889 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | rips | 0.2137 | -0.04476 ± 0.00037 | 0.04491 | 0.00 | 679 | 0.068 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | snrips | 0.2137 | -0.00386 ± 0.00055 | 0.00666 | 0.90 | 679 | 0.068 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.2137 | +0.01889 ± 0.00021 | 0.01901 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.2137 | -0.00479 ± 0.00016 | 0.00506 | 0.02 |  |  | 100% | 0.02 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.2137 | +0.01675 ± 0.00030 | 0.01702 | 0.00 | 9889 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | cdr | 0.2137 | +0.06479 ± 0.00034 | 0.06488 | 0.00 | 679 | 0.068 | 0% |  |  | biased, as expected: 55% of its slots unsupported |

## nfcorpus · cascade · {"policy":"softmax","tau":1.0,"top":10,"by":"rank"}

100 logs of 100000 impressions, 13 s. Propensities: 10000000 lists, 0 mismatched; chi-square 11065 on 11160 df (p 0.737), largest |z| 4.05.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.5430 | 0.2921 | 0.2921 | 2.363 |
| rerank | 0.0% | 0.5318 | 0.2795 | 0.2795 | 2.370 |
| oracle | 0.0% | 0.5813 | 0.3287 | 0.3287 | 2.344 |
| reverse | 0.0% | 0.3732 | 0.1509 | 0.1509 | 2.443 |
| wide | 18.6% | 0.5223 | 0.2715 | 0.2715 | 2.374 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.5430 | -0.02735 ± 0.00017 | 0.02740 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.5430 | -0.00718 ± 0.00694 | 0.06939 | 0.97 | 111 | 0.993 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snips | 0.5430 | -0.00299 ± 0.00453 | 0.04518 | 0.96 | 111 | 0.993 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | iips | 0.5430 | +0.03215 ± 0.00057 | 0.03264 | 0.00 | 11570 | 0.999 | 98% | 0.00 |  | biased, as expected |
| ranking | clicks | sniips | 0.5430 | +0.03241 ± 0.00053 | 0.03283 | 0.00 | 11570 | 0.999 | 98% | 0.00 |  | biased, as expected |
| ranking | clicks | rips | 0.5430 | +0.00217 ± 0.00208 | 0.02078 | 0.96 | 111 | 0.997 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snrips | 0.5430 | +0.00362 ± 0.00214 | 0.02155 | 0.94 | 111 | 0.997 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | dm-pbm | 0.5430 | +0.05255 ± 0.00023 | 0.05260 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.5430 | -0.00002 ± 0.00020 | 0.00195 | 0.64 |  |  | 100% | 0.64 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.5430 | +0.03259 ± 0.00047 | 0.03293 | 0.00 | 11570 | 0.999 | 98% | 0.00 |  | biased, as expected |
| ranking | clicks | cdr | 0.5430 | +0.00292 ± 0.00194 | 0.01951 | 0.93 | 111 | 0.997 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | naive | 0.2921 | -0.02538 ± 0.00015 | 0.02542 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.2921 | -0.00063 ± 0.00509 | 0.05063 | 0.94 | 111 | 0.993 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snips | 0.2921 | +0.00204 ± 0.00447 | 0.04450 | 0.93 | 111 | 0.993 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | iips | 0.2921 | +0.01741 ± 0.00042 | 0.01789 | 0.01 | 11570 | 0.999 | 98% | 0.01 |  | biased, as expected |
| ranking | satisfied | sniips | 0.2921 | +0.01757 ± 0.00039 | 0.01799 | 0.00 | 11570 | 0.999 | 98% | 0.00 |  | biased, as expected |
| ranking | satisfied | rips | 0.2921 | +0.00066 ± 0.00133 | 0.01328 | 0.92 | 111 | 0.997 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snrips | 0.2921 | +0.00126 ± 0.00135 | 0.01350 | 0.93 | 111 | 0.997 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | dm-pbm | 0.2921 | +0.03294 ± 0.00020 | 0.03300 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.2921 | +0.00625 ± 0.00017 | 0.00649 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.2921 | +0.01776 ± 0.00034 | 0.01808 | 0.00 | 11570 | 0.999 | 98% | 0.00 |  | biased, as expected |
| ranking | satisfied | cdr | 0.2921 | +0.00110 ± 0.00120 | 0.01202 | 0.94 | 111 | 0.997 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| rerank | clicks | naive | 0.5318 | -0.01619 ± 0.00017 | 0.01628 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.5318 | +0.01154 ± 0.02439 | 0.24296 | 0.88 | 20 | 0.971 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 20 impressions, mean 0.97; 11% of logs usable) |
| rerank | clicks | snips | 0.5318 | +0.04019 ± 0.01440 | 0.14879 | 0.82 | 20 | 0.971 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 20 impressions, mean 0.97; 11% of logs usable) |
| rerank | clicks | iips | 0.5318 | +0.01469 ± 0.00063 | 0.01597 | 0.33 | 9666 | 0.998 | 100% | 0.33 |  | biased, as expected |
| rerank | clicks | sniips | 0.5318 | +0.01493 ± 0.00056 | 0.01595 | 0.20 | 9666 | 0.998 | 100% | 0.20 |  | biased, as expected |
| rerank | clicks | rips | 0.5318 | -0.00034 ± 0.01497 | 0.14893 | 0.83 | 19 | 0.896 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.90; 8% of logs usable) |
| rerank | clicks | snrips | 0.5318 | -0.00064 ± 0.00914 | 0.09095 | 0.89 | 19 | 0.896 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.90; 8% of logs usable) |
| rerank | clicks | dm-pbm | 0.5318 | +0.04576 ± 0.00025 | 0.04583 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.5318 | +0.00055 ± 0.00022 | 0.00225 | 0.62 |  |  | 100% | 0.62 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.5318 | +0.01491 ± 0.00055 | 0.01589 | 0.15 | 9666 | 0.998 | 100% | 0.15 |  | biased, as expected |
| rerank | clicks | cdr | 0.5318 | +0.00149 ± 0.01431 | 0.14242 | 0.92 | 19 | 0.896 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.90; 8% of logs usable) |
| rerank | satisfied | naive | 0.2795 | -0.01282 ± 0.00015 | 0.01291 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.2795 | +0.00530 ± 0.01750 | 0.17424 | 0.86 | 20 | 0.971 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 20 impressions, mean 0.97; 11% of logs usable) |
| rerank | satisfied | snips | 0.2795 | +0.02160 ± 0.01217 | 0.12298 | 0.90 | 20 | 0.971 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 20 impressions, mean 0.97; 11% of logs usable) |
| rerank | satisfied | iips | 0.2795 | +0.00500 ± 0.00043 | 0.00656 | 0.74 | 9666 | 0.998 | 100% | 0.74 |  | biased, as expected |
| rerank | satisfied | sniips | 0.2795 | +0.00511 ± 0.00040 | 0.00650 | 0.72 | 9666 | 0.998 | 100% | 0.72 |  | biased, as expected |
| rerank | satisfied | rips | 0.2795 | -0.00507 ± 0.00534 | 0.05335 | 0.79 | 19 | 0.896 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.90; 8% of logs usable) |
| rerank | satisfied | snrips | 0.2795 | -0.00291 ± 0.00504 | 0.05020 | 0.79 | 19 | 0.896 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.90; 8% of logs usable) |
| rerank | satisfied | dm-pbm | 0.2795 | +0.02824 ± 0.00021 | 0.02832 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.2795 | +0.00787 ± 0.00019 | 0.00810 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.2795 | +0.00515 ± 0.00037 | 0.00635 | 0.68 | 9666 | 0.998 | 100% | 0.68 |  | biased, as expected |
| rerank | satisfied | cdr | 0.2795 | -0.00569 ± 0.00523 | 0.05239 | 0.87 | 19 | 0.896 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.90; 8% of logs usable) |
| oracle | clicks | naive | 0.5813 | -0.06569 ± 0.00017 | 0.06571 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.5813 | -0.01452 ± 0.01800 | 0.17965 | 0.85 | 48 | 0.978 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snips | 0.5813 | -0.01034 ± 0.00861 | 0.08630 | 0.92 | 48 | 0.978 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | iips | 0.5813 | +0.12883 ± 0.00067 | 0.12900 | 0.00 | 11246 | 1.002 | 97% | 0.00 |  | biased, as expected |
| oracle | clicks | sniips | 0.5813 | +0.12887 ± 0.00061 | 0.12902 | 0.00 | 11246 | 1.002 | 97% | 0.00 |  | biased, as expected |
| oracle | clicks | rips | 0.5813 | -0.00216 ± 0.00257 | 0.02569 | 0.93 | 48 | 0.958 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snrips | 0.5813 | +0.00006 ± 0.00247 | 0.02462 | 0.92 | 48 | 0.958 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | dm-pbm | 0.5813 | +0.18048 ± 0.00029 | 0.18051 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.5813 | -0.00136 ± 0.00019 | 0.00236 | 0.57 |  |  | 100% | 0.57 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.5813 | +0.12885 ± 0.00056 | 0.12897 | 0.00 | 11246 | 1.002 | 97% | 0.00 |  | biased, as expected |
| oracle | clicks | cdr | 0.5813 | -0.00114 ± 0.00257 | 0.02563 | 0.93 | 48 | 0.958 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | naive | 0.3287 | -0.06203 ± 0.00015 | 0.06204 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.3287 | -0.00050 ± 0.01651 | 0.16430 | 0.84 | 48 | 0.978 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snips | 0.3287 | -0.00213 ± 0.01037 | 0.10317 | 0.89 | 48 | 0.978 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | iips | 0.3287 | +0.07865 ± 0.00056 | 0.07885 | 0.00 | 11246 | 1.002 | 97% | 0.00 |  | biased, as expected |
| oracle | satisfied | sniips | 0.3287 | +0.07871 ± 0.00052 | 0.07888 | 0.00 | 11246 | 1.002 | 97% | 0.00 |  | biased, as expected |
| oracle | satisfied | rips | 0.3287 | -0.00152 ± 0.00165 | 0.01654 | 0.83 | 48 | 0.958 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snrips | 0.3287 | -0.00098 ± 0.00157 | 0.01561 | 0.83 | 48 | 0.958 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | dm-pbm | 0.3287 | +0.11120 ± 0.00024 | 0.11123 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.3287 | +0.00297 ± 0.00017 | 0.00344 | 0.26 |  |  | 100% | 0.26 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.3287 | +0.07877 ± 0.00043 | 0.07889 | 0.00 | 11246 | 1.002 | 97% | 0.00 |  | biased, as expected |
| oracle | satisfied | cdr | 0.3287 | -0.00129 ± 0.00166 | 0.01655 | 0.87 | 48 | 0.958 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| reverse | clicks | naive | 0.3732 | +0.14242 ± 0.00017 | 0.14243 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.3732 | +0.49202 ± 0.60870 | 6.07648 | 0.02 | 0 | 1.730 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 1.73; 0% of logs usable) |
| reverse | clicks | snips | 0.3732 | -0.35320 ± 0.01407 | 0.37994 | 0.00 | 0 | 1.730 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 1.73; 0% of logs usable) |
| reverse | clicks | iips | 0.3732 | -0.04229 ± 0.00076 | 0.04297 | 0.00 | 3413 | 0.998 | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | sniips | 0.3732 | -0.04205 ± 0.00076 | 0.04272 | 0.00 | 3413 | 0.998 | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | rips | 0.3732 | -0.06390 ± 0.04448 | 0.44720 | 0.37 | 0 | 0.459 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.46; 0% of logs usable) |
| reverse | clicks | snrips | 0.3732 | -0.10296 ± 0.01984 | 0.22266 | 0.35 | 0 | 0.459 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.46; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.3732 | +0.00778 ± 0.00031 | 0.00836 | 0.02 |  |  | 100% | 0.02 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.3732 | +0.00577 ± 0.00025 | 0.00630 | 0.06 |  |  | 100% | 0.06 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.3732 | -0.04163 ± 0.00073 | 0.04225 | 0.00 | 3413 | 0.998 | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | cdr | 0.3732 | -0.08476 ± 0.05301 | 0.53417 | 0.92 | 0 | 0.459 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.46; 0% of logs usable) |
| reverse | satisfied | naive | 0.1509 | +0.11583 ± 0.00015 | 0.11584 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.1509 | +0.71434 ± 0.60870 | 6.09851 | 0.02 | 0 | 1.730 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 1.73; 0% of logs usable) |
| reverse | satisfied | snips | 0.1509 | -0.13088 ± 0.01407 | 0.19165 | 0.00 | 0 | 1.730 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 1.73; 0% of logs usable) |
| reverse | satisfied | iips | 0.1509 | -0.01474 ± 0.00051 | 0.01559 | 0.18 | 3413 | 0.998 | 100% | 0.18 |  | biased, as expected |
| reverse | satisfied | sniips | 0.1509 | -0.01465 ± 0.00051 | 0.01549 | 0.19 | 3413 | 0.998 | 100% | 0.19 |  | biased, as expected |
| reverse | satisfied | rips | 0.1509 | -0.02261 ± 0.03161 | 0.31535 | 0.18 | 0 | 0.459 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.46; 0% of logs usable) |
| reverse | satisfied | snrips | 0.1509 | -0.04884 ± 0.00969 | 0.10806 | 0.18 | 0 | 0.459 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.46; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.1509 | +0.01707 ± 0.00019 | 0.01717 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.1509 | +0.01973 ± 0.00019 | 0.01982 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.1509 | -0.01436 ± 0.00045 | 0.01505 | 0.15 | 3413 | 0.998 | 100% | 0.15 |  | biased, as expected |
| reverse | satisfied | cdr | 0.1509 | -0.03384 ± 0.02545 | 0.25550 | 0.93 | 0 | 0.459 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.46; 0% of logs usable) |
| wide | clicks | naive | 0.5223 | -0.00664 ± 0.00017 | 0.00685 | 0.01 |  |  | 100% | 0.01 |  | biased, as expected |
| wide | clicks | ips | 0.5223 | -0.21137 ± 0.01402 | 0.25326 | 0.40 | 15 | 0.481 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snips | 0.5223 | +0.12376 ± 0.01327 | 0.18098 | 0.70 | 15 | 0.481 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | iips | 0.5223 | -0.02888 ± 0.00055 | 0.02938 | 0.00 | 6395 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | sniips | 0.5223 | +0.04033 ± 0.00057 | 0.04073 | 0.00 | 6395 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | rips | 0.5223 | -0.05722 ± 0.00606 | 0.08313 | 0.46 | 15 | 0.681 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snrips | 0.5223 | +0.03648 ± 0.00829 | 0.09023 | 0.93 | 15 | 0.681 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | dm-pbm | 0.5223 | +0.07335 ± 0.00024 | 0.07339 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.5223 | +0.04902 ± 0.00021 | 0.04907 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.5223 | +0.04321 ± 0.00047 | 0.04346 | 0.00 | 6395 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | cdr | 0.5223 | +0.04889 ± 0.00571 | 0.07497 | 0.61 | 15 | 0.681 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | naive | 0.2715 | -0.00475 ± 0.00015 | 0.00497 | 0.07 |  |  | 100% | 0.07 |  | biased, as expected |
| wide | satisfied | ips | 0.2715 | -0.10297 ± 0.01271 | 0.16308 | 0.50 | 15 | 0.481 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | snips | 0.2715 | +0.07075 ± 0.01332 | 0.15025 | 0.90 | 15 | 0.481 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | iips | 0.2715 | -0.01031 ± 0.00036 | 0.01093 | 0.26 | 6395 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | sniips | 0.2715 | +0.01972 ± 0.00041 | 0.02015 | 0.00 | 6395 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | rips | 0.2715 | -0.01874 ± 0.00525 | 0.05548 | 0.59 | 15 | 0.681 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.2715 | +0.02188 ± 0.00652 | 0.06848 | 0.91 | 15 | 0.681 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | dm-pbm | 0.2715 | +0.04775 ± 0.00021 | 0.04779 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.2715 | +0.04080 ± 0.00020 | 0.04085 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.2715 | +0.02705 ± 0.00033 | 0.02724 | 0.00 | 6395 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | cdr | 0.2715 | +0.03495 ± 0.00511 | 0.06166 | 0.42 | 15 | 0.681 | 0% |  |  | biased, as expected: 19% of its slots unsupported |

## nfcorpus · cascade · {"policy":"epsilon","epsilon":0.3,"top":10}

100 logs of 100000 impressions, 13 s. Propensities: 10000000 lists, 0 mismatched; chi-square 11268 on 11160 df (p 0.235), largest |z| 4.55.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.5430 | 0.2921 | 0.2921 | 2.363 |
| rerank | 0.0% | 0.5318 | 0.2795 | 0.2795 | 2.370 |
| oracle | 0.0% | 0.5813 | 0.3287 | 0.3287 | 2.344 |
| reverse | 0.0% | 0.3732 | 0.1509 | 0.1509 | 2.443 |
| wide | 18.6% | 0.5223 | 0.2715 | 0.2715 | 2.374 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.5430 | -0.02259 ± 0.00017 | 0.02265 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.5430 | +0.00022 ± 0.00023 | 0.00233 | 0.98 | 70016 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | clicks | snips | 0.5430 | +0.00010 ± 0.00020 | 0.00204 | 0.93 | 70016 | 1.000 | 100% | 0.93 | yes | ok |
| ranking | clicks | iips | 0.5430 | +0.00304 ± 0.00023 | 0.00378 | 0.70 | 72955 | 1.000 | 100% | 0.70 |  | biased, as expected |
| ranking | clicks | sniips | 0.5430 | +0.00292 ± 0.00020 | 0.00355 | 0.64 | 72955 | 1.000 | 100% | 0.64 |  | biased, as expected |
| ranking | clicks | rips | 0.5430 | +0.00017 ± 0.00024 | 0.00237 | 0.96 | 70016 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | clicks | snrips | 0.5430 | +0.00006 ± 0.00021 | 0.00205 | 0.91 | 70016 | 1.000 | 100% | 0.91 | yes | ok |
| ranking | clicks | dm-pbm | 0.5430 | +0.01484 ± 0.00019 | 0.01496 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.5430 | -0.00008 ± 0.00017 | 0.00174 | 0.75 |  |  | 100% | 0.75 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.5430 | +0.00290 ± 0.00019 | 0.00348 | 0.58 | 72955 | 1.000 | 100% | 0.58 |  | biased, as expected |
| ranking | clicks | cdr | 0.5430 | +0.00003 ± 0.00019 | 0.00192 | 0.91 | 70016 | 1.000 | 100% | 0.91 | yes | ok |
| ranking | satisfied | naive | 0.2921 | -0.02021 ± 0.00014 | 0.02026 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.2921 | +0.00002 ± 0.00019 | 0.00185 | 0.98 | 70016 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | satisfied | snips | 0.2921 | -0.00005 ± 0.00018 | 0.00179 | 0.94 | 70016 | 1.000 | 100% | 0.94 | yes | ok |
| ranking | satisfied | iips | 0.2921 | +0.00154 ± 0.00018 | 0.00235 | 0.82 | 72955 | 1.000 | 100% | 0.82 |  | biased, as expected |
| ranking | satisfied | sniips | 0.2921 | +0.00147 ± 0.00017 | 0.00227 | 0.84 | 72955 | 1.000 | 100% | 0.84 |  | biased, as expected |
| ranking | satisfied | rips | 0.2921 | -0.00000 ± 0.00019 | 0.00186 | 0.96 | 70016 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | satisfied | snrips | 0.2921 | -0.00006 ± 0.00018 | 0.00178 | 0.95 | 70016 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | satisfied | dm-pbm | 0.2921 | +0.00871 ± 0.00016 | 0.00886 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.2921 | +0.00563 ± 0.00015 | 0.00584 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.2921 | +0.00150 ± 0.00016 | 0.00222 | 0.83 | 72955 | 1.000 | 100% | 0.83 |  | biased, as expected |
| ranking | satisfied | cdr | 0.2921 | -0.00003 ± 0.00017 | 0.00168 | 0.96 | 70016 | 1.000 | 100% | 0.96 | yes | ok |
| rerank | clicks | naive | 0.5318 | -0.01144 ± 0.00017 | 0.01156 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.5318 | -0.05557 ± 0.07209 | 0.71948 | 0.34 | 2529 | 0.980 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2529 impressions, mean 0.98; 0% of logs usable) |
| rerank | clicks | snips | 0.5318 | -0.10623 ± 0.03394 | 0.35397 | 0.10 | 2529 | 0.980 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2529 impressions, mean 0.98; 0% of logs usable) |
| rerank | clicks | iips | 0.5318 | +0.02751 ± 0.00106 | 0.02947 | 0.26 | 4002 | 0.997 | 99% | 0.25 |  | biased, as expected |
| rerank | clicks | sniips | 0.5318 | +0.02732 ± 0.00100 | 0.02908 | 0.19 | 4002 | 0.997 | 99% | 0.18 |  | biased, as expected |
| rerank | clicks | rips | 0.5318 | +0.00353 ± 0.02485 | 0.24727 | 0.59 | 4 | 0.734 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.73; 0% of logs usable) |
| rerank | clicks | snrips | 0.5318 | +0.00372 ± 0.01986 | 0.19765 | 0.62 | 4 | 0.734 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.73; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.5318 | +0.01636 ± 0.00025 | 0.01654 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.5318 | +0.00163 ± 0.00023 | 0.00278 | 0.50 |  |  | 100% | 0.50 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.5318 | +0.02731 ± 0.00094 | 0.02886 | 0.18 | 4002 | 0.997 | 99% | 0.18 |  | biased, as expected |
| rerank | clicks | cdr | 0.5318 | -0.00526 ± 0.02326 | 0.23148 | 0.91 | 4 | 0.734 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.73; 0% of logs usable) |
| rerank | satisfied | naive | 0.2795 | -0.00766 ± 0.00014 | 0.00778 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.2795 | -0.03523 ± 0.04933 | 0.49213 | 0.20 | 2529 | 0.980 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2529 impressions, mean 0.98; 0% of logs usable) |
| rerank | satisfied | snips | 0.2795 | -0.06886 ± 0.03085 | 0.31463 | 0.06 | 2529 | 0.980 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2529 impressions, mean 0.98; 0% of logs usable) |
| rerank | satisfied | iips | 0.2795 | +0.01149 ± 0.00077 | 0.01381 | 0.65 | 4002 | 0.997 | 99% | 0.65 |  | biased, as expected |
| rerank | satisfied | sniips | 0.2795 | +0.01141 ± 0.00074 | 0.01356 | 0.60 | 4002 | 0.997 | 99% | 0.60 |  | biased, as expected |
| rerank | satisfied | rips | 0.2795 | -0.01629 ± 0.00630 | 0.06473 | 0.67 | 4 | 0.734 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.73; 0% of logs usable) |
| rerank | satisfied | snrips | 0.2795 | -0.01532 ± 0.00688 | 0.07020 | 0.72 | 4 | 0.734 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.73; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.2795 | +0.00915 ± 0.00018 | 0.00933 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.2795 | +0.00898 ± 0.00017 | 0.00913 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.2795 | +0.01144 ± 0.00062 | 0.01301 | 0.50 | 4002 | 0.997 | 99% | 0.49 |  | biased, as expected |
| rerank | satisfied | cdr | 0.2795 | -0.01916 ± 0.00835 | 0.08528 | 0.90 | 4 | 0.734 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.73; 0% of logs usable) |
| oracle | clicks | naive | 0.5813 | -0.06093 ± 0.00017 | 0.06096 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.5813 | +0.01232 ± 0.06094 | 0.60649 | 0.35 | 20802 | 1.030 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 20802 impressions, mean 1.03; 0% of logs usable) |
| oracle | clicks | snips | 0.5813 | -0.09668 ± 0.02125 | 0.23245 | 0.30 | 20802 | 1.030 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 20802 impressions, mean 1.03; 0% of logs usable) |
| oracle | clicks | iips | 0.5813 | +0.12670 ± 0.00102 | 0.12711 | 0.00 | 7333 | 1.000 | 99% | 0.00 |  | biased, as expected |
| oracle | clicks | sniips | 0.5813 | +0.12659 ± 0.00090 | 0.12691 | 0.00 | 7333 | 1.000 | 99% | 0.00 |  | biased, as expected |
| oracle | clicks | rips | 0.5813 | -0.00624 ± 0.01086 | 0.10824 | 0.70 | 15 | 0.819 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 15 impressions, mean 0.82; 0% of logs usable) |
| oracle | clicks | snrips | 0.5813 | -0.00285 ± 0.00752 | 0.07486 | 0.77 | 15 | 0.819 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 15 impressions, mean 0.82; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.5813 | +0.15445 ± 0.00025 | 0.15447 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.5813 | -0.00284 ± 0.00018 | 0.00336 | 0.28 |  |  | 100% | 0.28 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.5813 | +0.12644 ± 0.00088 | 0.12674 | 0.00 | 7333 | 1.000 | 99% | 0.00 |  | biased, as expected |
| oracle | clicks | cdr | 0.5813 | -0.01041 ± 0.01043 | 0.10434 | 0.85 | 15 | 0.819 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 15 impressions, mean 0.82; 0% of logs usable) |
| oracle | satisfied | naive | 0.3287 | -0.05686 ± 0.00014 | 0.05688 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.3287 | -0.02302 ± 0.04263 | 0.42480 | 0.23 | 20802 | 1.030 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 20802 impressions, mean 1.03; 0% of logs usable) |
| oracle | satisfied | snips | 0.3287 | -0.09201 ± 0.02176 | 0.23525 | 0.09 | 20802 | 1.030 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 20802 impressions, mean 1.03; 0% of logs usable) |
| oracle | satisfied | iips | 0.3287 | +0.07539 ± 0.00083 | 0.07584 | 0.00 | 7333 | 1.000 | 99% | 0.00 |  | biased, as expected |
| oracle | satisfied | sniips | 0.3287 | +0.07531 ± 0.00074 | 0.07568 | 0.00 | 7333 | 1.000 | 99% | 0.00 |  | biased, as expected |
| oracle | satisfied | rips | 0.3287 | +0.00580 ± 0.01063 | 0.10595 | 0.70 | 15 | 0.819 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 15 impressions, mean 0.82; 0% of logs usable) |
| oracle | satisfied | snrips | 0.3287 | +0.00269 ± 0.00723 | 0.07203 | 0.72 | 15 | 0.819 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 15 impressions, mean 0.82; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.3287 | +0.09312 ± 0.00024 | 0.09315 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.3287 | +0.00138 ± 0.00018 | 0.00224 | 0.62 |  |  | 100% | 0.62 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.3287 | +0.07517 ± 0.00068 | 0.07547 | 0.00 | 7333 | 1.000 | 99% | 0.00 |  | biased, as expected |
| oracle | satisfied | cdr | 0.3287 | +0.00403 ± 0.01021 | 0.10170 | 0.83 | 15 | 0.819 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 15 impressions, mean 0.82; 0% of logs usable) |
| reverse | clicks | naive | 0.3732 | +0.14718 ± 0.00017 | 0.14719 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.3732 | -0.01032 ± 0.05644 | 0.56169 | 0.32 | 1 | 0.897 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.90; 0% of logs usable) |
| reverse | clicks | snips | 0.3732 | -0.12153 ± 0.03946 | 0.41102 | 0.14 | 1 | 0.897 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.90; 0% of logs usable) |
| reverse | clicks | iips | 0.3732 | -0.02740 ± 0.00115 | 0.02969 | 0.28 | 2936 | 0.998 | 98% | 0.29 |  | biased, as expected |
| reverse | clicks | sniips | 0.3732 | -0.02710 ± 0.00109 | 0.02917 | 0.24 | 2936 | 0.998 | 98% | 0.24 |  | biased, as expected |
| reverse | clicks | rips | 0.3732 | +0.00702 ± 0.02810 | 0.27965 | 0.55 | 1 | 0.689 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.69; 0% of logs usable) |
| reverse | clicks | snrips | 0.3732 | -0.00013 ± 0.02303 | 0.22911 | 0.53 | 1 | 0.689 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.69; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.3732 | +0.04615 ± 0.00041 | 0.04633 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.3732 | +0.01063 ± 0.00033 | 0.01112 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.3732 | -0.02658 ± 0.00101 | 0.02844 | 0.18 | 2936 | 0.998 | 98% | 0.18 |  | biased, as expected |
| reverse | clicks | cdr | 0.3732 | +0.01168 ± 0.02720 | 0.27084 | 0.93 | 1 | 0.689 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.69; 0% of logs usable) |
| reverse | satisfied | naive | 0.1509 | +0.12100 ± 0.00014 | 0.12101 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.1509 | +0.02048 ± 0.03805 | 0.37919 | 0.17 | 1 | 0.897 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.90; 0% of logs usable) |
| reverse | satisfied | snips | 0.1509 | -0.03588 ± 0.02800 | 0.28088 | 0.10 | 1 | 0.897 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.90; 0% of logs usable) |
| reverse | satisfied | iips | 0.1509 | -0.00953 ± 0.00071 | 0.01184 | 0.72 | 2936 | 0.998 | 98% | 0.71 |  | biased, as expected |
| reverse | satisfied | sniips | 0.1509 | -0.00939 ± 0.00069 | 0.01165 | 0.68 | 2936 | 0.998 | 98% | 0.67 |  | biased, as expected |
| reverse | satisfied | rips | 0.1509 | -0.01136 ± 0.01240 | 0.12385 | 0.50 | 1 | 0.689 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.69; 0% of logs usable) |
| reverse | satisfied | snrips | 0.1509 | -0.00648 ± 0.01328 | 0.13228 | 0.52 | 1 | 0.689 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.69; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.1509 | +0.04143 ± 0.00025 | 0.04151 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.1509 | +0.02686 ± 0.00022 | 0.02694 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.1509 | -0.00888 ± 0.00059 | 0.01063 | 0.66 | 2936 | 0.998 | 98% | 0.65 |  | biased, as expected |
| reverse | satisfied | cdr | 0.1509 | -0.00881 ± 0.01220 | 0.12172 | 0.92 | 1 | 0.689 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.69; 0% of logs usable) |
| wide | clicks | naive | 0.5223 | -0.00188 ± 0.00017 | 0.00251 | 0.71 |  |  | 100% | 0.71 |  | biased, as expected |
| wide | clicks | ips | 0.5223 | -0.23825 ± 0.05090 | 0.55969 | 0.23 | 3019 | 0.521 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snips | 0.5223 | -0.10840 ± 0.02893 | 0.30758 | 0.04 | 3019 | 0.521 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.5223 | -0.02479 ± 0.00096 | 0.02657 | 0.28 | 2522 | 0.607 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | sniips | 0.5223 | +0.04715 ± 0.00108 | 0.04835 | 0.00 | 2522 | 0.607 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | rips | 0.5223 | -0.03729 ± 0.02048 | 0.20712 | 0.45 | 5 | 0.313 | 0% |  |  | unbiased though unsupported |
| wide | clicks | snrips | 0.5223 | +0.04702 ± 0.02070 | 0.21122 | 0.79 | 5 | 0.313 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.5223 | +0.03980 ± 0.00025 | 0.03988 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.5223 | +0.05224 ± 0.00022 | 0.05229 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.5223 | +0.04215 ± 0.00084 | 0.04297 | 0.00 | 2522 | 0.607 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | cdr | 0.5223 | +0.06594 ± 0.01807 | 0.19152 | 0.90 | 5 | 0.313 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | naive | 0.2715 | +0.00042 ± 0.00014 | 0.00147 | 0.95 |  |  | 100% | 0.95 |  | unbiased here |
| wide | satisfied | ips | 0.2715 | -0.10273 ± 0.04231 | 0.43333 | 0.14 | 3019 | 0.521 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snips | 0.2715 | -0.08354 ± 0.02726 | 0.28383 | 0.04 | 3019 | 0.521 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.2715 | -0.00894 ± 0.00067 | 0.01113 | 0.72 | 2522 | 0.607 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | sniips | 0.2715 | +0.02232 ± 0.00076 | 0.02355 | 0.16 | 2522 | 0.607 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | rips | 0.2715 | -0.02215 ± 0.00607 | 0.06432 | 0.67 | 5 | 0.313 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.2715 | +0.01055 ± 0.00863 | 0.08654 | 0.77 | 5 | 0.313 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | dm-pbm | 0.2715 | +0.02767 ± 0.00019 | 0.02773 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.2715 | +0.04332 ± 0.00018 | 0.04335 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.2715 | +0.02606 ± 0.00055 | 0.02663 | 0.00 | 2522 | 0.607 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | cdr | 0.2715 | +0.03115 ± 0.00624 | 0.06949 | 0.85 | 5 | 0.313 | 0% |  |  | biased, as expected: 19% of its slots unsupported |

## nfcorpus · cascade · {"policy":"shuffle","top":10}

100 logs of 100000 impressions, 12 s. Propensities: 10000000 lists, 0 mismatched; chi-square 11399 on 11160 df (p 0.055), largest |z| 3.83.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.5430 | 0.2921 | 0.2921 | 2.363 |
| rerank | 0.0% | 0.5318 | 0.2795 | 0.2795 | 2.370 |
| oracle | 0.0% | 0.5813 | 0.3287 | 0.3287 | 2.344 |
| reverse | 0.0% | 0.3732 | 0.1509 | 0.1509 | 2.443 |
| wide | 18.6% | 0.5223 | 0.2715 | 0.2715 | 2.374 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.5430 | -0.07520 ± 0.00016 | 0.07522 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.5430 | +0.13440 ± 0.05051 | 0.52026 | 0.85 | 4 | 1.158 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.16; 0% of logs usable) |
| ranking | clicks | snips | 0.5430 | +0.01318 ± 0.03113 | 0.31006 | 0.69 | 4 | 1.158 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.16; 0% of logs usable) |
| ranking | clicks | iips | 0.5430 | +0.06950 ± 0.00075 | 0.06989 | 0.00 | 9887 | 1.000 | 97% | 0.00 |  | biased, as expected |
| ranking | clicks | sniips | 0.5430 | +0.06941 ± 0.00068 | 0.06975 | 0.00 | 9887 | 1.000 | 97% | 0.00 |  | biased, as expected |
| ranking | clicks | rips | 0.5430 | +0.01240 ± 0.01306 | 0.13058 | 0.79 | 4 | 1.055 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.06; 0% of logs usable) |
| ranking | clicks | snrips | 0.5430 | +0.00610 ± 0.01188 | 0.11835 | 0.75 | 4 | 1.055 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.06; 0% of logs usable) |
| ranking | clicks | dm-pbm | 0.5430 | +0.08997 ± 0.00029 | 0.09002 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.5430 | -0.00170 ± 0.00020 | 0.00262 | 0.47 |  |  | 100% | 0.47 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.5430 | +0.06884 ± 0.00059 | 0.06909 | 0.00 | 9887 | 1.000 | 97% | 0.00 |  | biased, as expected |
| ranking | clicks | cdr | 0.5430 | +0.00758 ± 0.01196 | 0.11928 | 0.89 | 4 | 1.055 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.06; 0% of logs usable) |
| ranking | satisfied | naive | 0.2921 | -0.06704 ± 0.00013 | 0.06706 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.2921 | +0.07684 ± 0.03373 | 0.34428 | 0.72 | 4 | 1.158 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.16; 0% of logs usable) |
| ranking | satisfied | snips | 0.2921 | +0.01467 ± 0.02501 | 0.24929 | 0.68 | 4 | 1.158 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.16; 0% of logs usable) |
| ranking | satisfied | iips | 0.2921 | +0.03750 ± 0.00053 | 0.03788 | 0.00 | 9887 | 1.000 | 97% | 0.00 |  | biased, as expected |
| ranking | satisfied | sniips | 0.2921 | +0.03745 ± 0.00050 | 0.03778 | 0.00 | 9887 | 1.000 | 97% | 0.00 |  | biased, as expected |
| ranking | satisfied | rips | 0.2921 | +0.01231 ± 0.00883 | 0.08869 | 0.80 | 4 | 1.055 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.06; 0% of logs usable) |
| ranking | satisfied | snrips | 0.2921 | +0.00615 ± 0.00699 | 0.06987 | 0.78 | 4 | 1.055 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.06; 0% of logs usable) |
| ranking | satisfied | dm-pbm | 0.2921 | +0.04862 ± 0.00022 | 0.04867 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.2921 | +0.00425 ± 0.00018 | 0.00460 | 0.08 |  |  | 100% | 0.08 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.2921 | +0.03696 ± 0.00045 | 0.03723 | 0.00 | 9887 | 1.000 | 97% | 0.00 |  | biased, as expected |
| ranking | satisfied | cdr | 0.2921 | +0.00993 ± 0.00828 | 0.08299 | 0.87 | 4 | 1.055 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.06; 0% of logs usable) |
| rerank | clicks | naive | 0.5318 | -0.06405 ± 0.00016 | 0.06407 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.5318 | +0.04577 ± 0.04530 | 0.45308 | 0.83 | 3 | 1.001 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| rerank | clicks | snips | 0.5318 | +0.01365 ± 0.03288 | 0.32741 | 0.60 | 3 | 1.001 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| rerank | clicks | iips | 0.5318 | +0.05185 ± 0.00078 | 0.05243 | 0.00 | 9895 | 1.000 | 96% | 0.00 |  | biased, as expected |
| rerank | clicks | sniips | 0.5318 | +0.05163 ± 0.00069 | 0.05208 | 0.00 | 9895 | 1.000 | 96% | 0.00 |  | biased, as expected |
| rerank | clicks | rips | 0.5318 | -0.00625 ± 0.01504 | 0.14977 | 0.71 | 3 | 0.945 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| rerank | clicks | snrips | 0.5318 | -0.01236 ± 0.01355 | 0.13535 | 0.68 | 3 | 0.945 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.5318 | +0.08148 ± 0.00026 | 0.08152 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.5318 | -0.00168 ± 0.00020 | 0.00260 | 0.51 |  |  | 100% | 0.51 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.5318 | +0.05137 ± 0.00059 | 0.05170 | 0.00 | 9895 | 1.000 | 96% | 0.00 |  | biased, as expected |
| rerank | clicks | cdr | 0.5318 | -0.00171 ± 0.01312 | 0.13054 | 0.85 | 3 | 0.945 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| rerank | satisfied | naive | 0.2795 | -0.05449 ± 0.00013 | 0.05450 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.2795 | +0.05613 ± 0.03568 | 0.35938 | 0.63 | 3 | 1.001 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| rerank | satisfied | snips | 0.2795 | +0.04343 ± 0.03144 | 0.31583 | 0.53 | 3 | 1.001 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| rerank | satisfied | iips | 0.2795 | +0.02535 ± 0.00054 | 0.02591 | 0.00 | 9895 | 1.000 | 96% | 0.00 |  | biased, as expected |
| rerank | satisfied | sniips | 0.2795 | +0.02522 ± 0.00049 | 0.02568 | 0.00 | 9895 | 1.000 | 96% | 0.00 |  | biased, as expected |
| rerank | satisfied | rips | 0.2795 | +0.00655 ± 0.01046 | 0.10431 | 0.73 | 3 | 0.945 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| rerank | satisfied | snrips | 0.2795 | -0.00207 ± 0.00711 | 0.07078 | 0.73 | 3 | 0.945 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.2795 | +0.04441 ± 0.00020 | 0.04445 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.2795 | +0.00485 ± 0.00016 | 0.00512 | 0.04 |  |  | 100% | 0.04 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.2795 | +0.02499 ± 0.00041 | 0.02533 | 0.00 | 9895 | 1.000 | 96% | 0.00 |  | biased, as expected |
| rerank | satisfied | cdr | 0.2795 | +0.00881 ± 0.00955 | 0.09546 | 0.85 | 3 | 0.945 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| oracle | clicks | naive | 0.5813 | -0.11354 ± 0.00016 | 0.11355 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.5813 | +0.07187 ± 0.05044 | 0.50699 | 0.83 | 4 | 1.067 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.07; 0% of logs usable) |
| oracle | clicks | snips | 0.5813 | +0.00320 ± 0.03482 | 0.34646 | 0.53 | 4 | 1.067 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.07; 0% of logs usable) |
| oracle | clicks | iips | 0.5813 | +0.17715 ± 0.00082 | 0.17734 | 0.00 | 9885 | 0.999 | 96% | 0.00 |  | biased, as expected |
| oracle | clicks | sniips | 0.5813 | +0.17742 ± 0.00077 | 0.17759 | 0.00 | 9885 | 0.999 | 96% | 0.00 |  | biased, as expected |
| oracle | clicks | rips | 0.5813 | +0.00141 ± 0.00934 | 0.09292 | 0.66 | 4 | 0.987 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.99; 0% of logs usable) |
| oracle | clicks | snrips | 0.5813 | -0.00689 ± 0.00740 | 0.07392 | 0.65 | 4 | 0.987 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.99; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.5813 | +0.21942 ± 0.00032 | 0.21944 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.5813 | -0.00217 ± 0.00019 | 0.00288 | 0.42 |  |  | 100% | 0.42 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.5813 | +0.17727 ± 0.00070 | 0.17741 | 0.00 | 9885 | 0.999 | 96% | 0.00 |  | biased, as expected |
| oracle | clicks | cdr | 0.5813 | -0.00133 ± 0.00877 | 0.08730 | 0.78 | 4 | 0.987 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.99; 0% of logs usable) |
| oracle | satisfied | naive | 0.3287 | -0.10369 ± 0.00013 | 0.10370 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.3287 | +0.00995 ± 0.03446 | 0.34302 | 0.66 | 4 | 1.067 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.07; 0% of logs usable) |
| oracle | satisfied | snips | 0.3287 | -0.03104 ± 0.02981 | 0.29819 | 0.58 | 4 | 1.067 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.07; 0% of logs usable) |
| oracle | satisfied | iips | 0.3287 | +0.10551 ± 0.00061 | 0.10568 | 0.00 | 9885 | 0.999 | 96% | 0.00 |  | biased, as expected |
| oracle | satisfied | sniips | 0.3287 | +0.10568 ± 0.00055 | 0.10582 | 0.00 | 9885 | 0.999 | 96% | 0.00 |  | biased, as expected |
| oracle | satisfied | rips | 0.3287 | +0.00318 ± 0.00511 | 0.05093 | 0.81 | 4 | 0.987 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.99; 0% of logs usable) |
| oracle | satisfied | snrips | 0.3287 | +0.00378 ± 0.00583 | 0.05816 | 0.79 | 4 | 0.987 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.99; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.3287 | +0.12869 ± 0.00028 | 0.12872 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.3287 | +0.00120 ± 0.00017 | 0.00209 | 0.56 |  |  | 100% | 0.56 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.3287 | +0.10558 ± 0.00047 | 0.10568 | 0.00 | 9885 | 0.999 | 96% | 0.00 |  | biased, as expected |
| oracle | satisfied | cdr | 0.3287 | +0.00235 ± 0.00474 | 0.04723 | 0.86 | 4 | 0.987 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.99; 0% of logs usable) |
| reverse | clicks | naive | 0.3732 | +0.09457 ± 0.00016 | 0.09458 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.3732 | +0.04109 ± 0.03483 | 0.34895 | 0.72 | 3 | 0.934 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| reverse | clicks | snips | 0.3732 | +0.02258 ± 0.03223 | 0.32152 | 0.61 | 3 | 0.934 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| reverse | clicks | iips | 0.3732 | -0.02522 ± 0.00064 | 0.02602 | 0.02 | 9886 | 0.999 | 98% | 0.02 |  | biased, as expected |
| reverse | clicks | sniips | 0.3732 | -0.02518 ± 0.00060 | 0.02588 | 0.02 | 9886 | 0.999 | 98% | 0.02 |  | biased, as expected |
| reverse | clicks | rips | 0.3732 | -0.01324 ± 0.01358 | 0.13579 | 0.78 | 3 | 0.861 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.86; 0% of logs usable) |
| reverse | clicks | snrips | 0.3732 | -0.01869 ± 0.01103 | 0.11136 | 0.77 | 3 | 0.861 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.86; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.3732 | +0.00024 ± 0.00025 | 0.00248 | 0.51 |  |  | 100% | 0.51 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.3732 | +0.00198 ± 0.00022 | 0.00293 | 0.36 |  |  | 100% | 0.36 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.3732 | -0.02498 ± 0.00054 | 0.02555 | 0.02 | 9886 | 0.999 | 98% | 0.02 |  | biased, as expected |
| reverse | clicks | cdr | 0.3732 | -0.00421 ± 0.01163 | 0.11580 | 0.89 | 3 | 0.861 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.86; 0% of logs usable) |
| reverse | satisfied | naive | 0.1509 | +0.07417 ± 0.00013 | 0.07418 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.1509 | +0.04266 ± 0.02451 | 0.24758 | 0.45 | 3 | 0.934 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| reverse | satisfied | snips | 0.1509 | +0.02157 ± 0.02245 | 0.22440 | 0.43 | 3 | 0.934 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| reverse | satisfied | iips | 0.1509 | -0.00879 ± 0.00031 | 0.00932 | 0.39 | 9886 | 0.999 | 98% | 0.39 |  | biased, as expected |
| reverse | satisfied | sniips | 0.1509 | -0.00877 ± 0.00031 | 0.00929 | 0.37 | 9886 | 0.999 | 98% | 0.38 |  | biased, as expected |
| reverse | satisfied | rips | 0.1509 | +0.00732 ± 0.01085 | 0.10819 | 0.58 | 3 | 0.861 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.86; 0% of logs usable) |
| reverse | satisfied | snrips | 0.1509 | +0.00163 ± 0.00867 | 0.08629 | 0.58 | 3 | 0.861 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.86; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.1509 | +0.00871 ± 0.00017 | 0.00887 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.1509 | +0.01220 ± 0.00017 | 0.01231 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.1509 | -0.00869 ± 0.00028 | 0.00913 | 0.21 | 9886 | 0.999 | 98% | 0.21 |  | biased, as expected |
| reverse | satisfied | cdr | 0.1509 | +0.01311 ± 0.00966 | 0.09697 | 0.87 | 3 | 0.861 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.86; 0% of logs usable) |
| wide | clicks | naive | 0.5223 | -0.05449 ± 0.00016 | 0.05451 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.5223 | -0.20172 ± 0.02746 | 0.33963 | 0.70 | 2 | 0.526 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snips | 0.5223 | +0.01841 ± 0.04128 | 0.41117 | 0.34 | 2 | 0.526 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.5223 | -0.00050 ± 0.00071 | 0.00708 | 0.95 | 6103 | 0.610 | 0% |  |  | unbiased though unsupported |
| wide | clicks | sniips | 0.5223 | +0.07514 ± 0.00075 | 0.07551 | 0.00 | 6103 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | rips | 0.5223 | -0.05688 ± 0.01144 | 0.12726 | 0.53 | 2 | 0.640 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snrips | 0.5223 | +0.02374 ± 0.01878 | 0.18838 | 0.72 | 2 | 0.640 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.5223 | +0.10186 ± 0.00026 | 0.10189 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.5223 | +0.03679 ± 0.00019 | 0.03684 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.5223 | +0.06833 ± 0.00055 | 0.06855 | 0.00 | 6103 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | cdr | 0.5223 | +0.04099 ± 0.01041 | 0.11135 | 0.97 | 2 | 0.640 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | naive | 0.2715 | -0.04641 ± 0.00013 | 0.04643 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.2715 | -0.08095 ± 0.02418 | 0.25386 | 0.47 | 2 | 0.526 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snips | 0.2715 | +0.06221 ± 0.04090 | 0.41166 | 0.24 | 2 | 0.526 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.2715 | +0.00439 ± 0.00052 | 0.00675 | 0.92 | 6103 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | sniips | 0.2715 | +0.03730 ± 0.00054 | 0.03769 | 0.00 | 6103 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | rips | 0.2715 | -0.01155 ± 0.00923 | 0.09260 | 0.61 | 2 | 0.640 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.2715 | +0.01967 ± 0.01036 | 0.10490 | 0.81 | 2 | 0.640 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | dm-pbm | 0.2715 | +0.05759 ± 0.00019 | 0.05762 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.2715 | +0.03021 ± 0.00017 | 0.03025 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.2715 | +0.03740 ± 0.00039 | 0.03760 | 0.00 | 6103 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | cdr | 0.2715 | +0.03550 ± 0.00869 | 0.09347 | 0.93 | 2 | 0.640 | 0% |  |  | biased, as expected: 19% of its slots unsupported |

## nfcorpus · cascade · {"policy":"swap","top":10}

100 logs of 100000 impressions, 11 s. Propensities: 10000000 lists, 0 mismatched; chi-square 3200 on 3224 df (p 0.614), largest |z| 3.66.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.5430 | 0.2921 | 0.2921 | 2.363 |
| rerank | 50.4% | 0.5318 | 0.2795 | 0.2795 | 2.370 |
| oracle | 24.9% | 0.5813 | 0.3287 | 0.3287 | 2.344 |
| reverse | 80.0% | 0.3732 | 0.1509 | 0.1509 | 2.443 |
| wide | 54.7% | 0.5223 | 0.2715 | 0.2715 | 2.374 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.5430 | -0.02538 ± 0.00015 | 0.02543 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.5430 | -0.00066 ± 0.00074 | 0.00735 | 0.93 | 9991 | 0.999 | 99% | 0.94 | yes | ok |
| ranking | clicks | snips | 0.5430 | -0.00015 ± 0.00058 | 0.00574 | 0.88 | 9991 | 0.999 | 99% | 0.88 | yes | ok |
| ranking | clicks | iips | 0.5430 | +0.04429 ± 0.00047 | 0.04454 | 0.00 | 9991 | 0.999 | 97% | 0.00 |  | biased, as expected |
| ranking | clicks | sniips | 0.5430 | +0.04448 ± 0.00042 | 0.04468 | 0.00 | 9991 | 0.999 | 97% | 0.00 |  | biased, as expected |
| ranking | clicks | rips | 0.5430 | -0.00066 ± 0.00074 | 0.00735 | 0.93 | 9991 | 0.999 | 99% | 0.94 | yes | ok |
| ranking | clicks | snrips | 0.5430 | -0.00015 ± 0.00058 | 0.00574 | 0.88 | 9991 | 0.999 | 99% | 0.88 | yes | ok |
| ranking | clicks | dm-pbm | 0.5430 | +0.04163 ± 0.00021 | 0.04168 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.5430 | -0.00048 ± 0.00017 | 0.00176 | 0.75 |  |  | 100% | 0.75 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.5430 | +0.04455 ± 0.00033 | 0.04467 | 0.00 | 9991 | 0.999 | 97% | 0.00 |  | biased, as expected |
| ranking | clicks | cdr | 0.5430 | -0.00030 ± 0.00050 | 0.00501 | 0.88 | 9991 | 0.999 | 99% | 0.88 | yes | ok |
| ranking | satisfied | naive | 0.2921 | -0.02806 ± 0.00014 | 0.02810 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.2921 | -0.00026 ± 0.00054 | 0.00535 | 0.95 | 9991 | 0.999 | 99% | 0.95 | yes | ok |
| ranking | satisfied | snips | 0.2921 | +0.00002 ± 0.00048 | 0.00478 | 0.93 | 9991 | 0.999 | 99% | 0.93 | yes | ok |
| ranking | satisfied | iips | 0.2921 | +0.02578 ± 0.00040 | 0.02608 | 0.00 | 9991 | 0.999 | 97% | 0.00 |  | biased, as expected |
| ranking | satisfied | sniips | 0.2921 | +0.02590 ± 0.00038 | 0.02618 | 0.00 | 9991 | 0.999 | 97% | 0.00 |  | biased, as expected |
| ranking | satisfied | rips | 0.2921 | -0.00026 ± 0.00054 | 0.00535 | 0.95 | 9991 | 0.999 | 99% | 0.95 | yes | ok |
| ranking | satisfied | snrips | 0.2921 | +0.00002 ± 0.00048 | 0.00478 | 0.93 | 9991 | 0.999 | 99% | 0.93 | yes | ok |
| ranking | satisfied | dm-pbm | 0.2921 | +0.02020 ± 0.00019 | 0.02029 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.2921 | +0.00411 ± 0.00016 | 0.00442 | 0.06 |  |  | 100% | 0.06 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.2921 | +0.02580 ± 0.00029 | 0.02597 | 0.00 | 9991 | 0.999 | 97% | 0.00 |  | biased, as expected |
| ranking | satisfied | cdr | 0.2921 | -0.00032 ± 0.00038 | 0.00384 | 0.91 | 9991 | 0.999 | 99% | 0.91 | yes | ok |
| rerank | clicks | naive | 0.5318 | -0.01423 ± 0.00015 | 0.01431 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.5318 | -0.49746 ± 0.00018 | 0.49746 | 0.00 | 965 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | snips | 0.5318 | -0.17556 ± 0.00167 | 0.17634 | 0.00 | 965 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | iips | 0.5318 | -0.16382 ± 0.00049 | 0.16389 | 0.00 | 9985 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | sniips | 0.5318 | +0.02434 ± 0.00058 | 0.02502 | 0.01 | 9985 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | rips | 0.5318 | -0.21619 ± 0.00051 | 0.21625 | 0.00 | 965 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | snrips | 0.5318 | -0.05170 ± 0.00098 | 0.05262 | 0.00 | 965 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | dm-pbm | 0.5318 | +0.02981 ± 0.00024 | 0.02991 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.5318 | +0.00261 ± 0.00021 | 0.00333 | 0.33 |  |  | 100% | 0.33 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.5318 | +0.02173 ± 0.00038 | 0.02206 | 0.00 | 9985 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | cdr | 0.5318 | +0.00504 ± 0.00036 | 0.00617 | 0.75 | 965 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | naive | 0.2795 | -0.01551 ± 0.00014 | 0.01557 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.2795 | -0.26654 ± 0.00010 | 0.26654 | 0.00 | 965 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | snips | 0.2795 | -0.14483 ± 0.00099 | 0.14516 | 0.00 | 965 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | iips | 0.2795 | -0.07108 ± 0.00038 | 0.07119 | 0.00 | 9985 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | sniips | 0.2795 | +0.01184 ± 0.00048 | 0.01275 | 0.28 | 9985 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | rips | 0.2795 | -0.09910 ± 0.00041 | 0.09918 | 0.00 | 965 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | snrips | 0.2795 | -0.04183 ± 0.00061 | 0.04227 | 0.00 | 965 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | dm-pbm | 0.2795 | +0.01514 ± 0.00020 | 0.01527 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.2795 | +0.00844 ± 0.00019 | 0.00864 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.2795 | +0.01521 ± 0.00031 | 0.01552 | 0.00 | 9985 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | cdr | 0.2795 | +0.00883 ± 0.00032 | 0.00940 | 0.21 | 965 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| oracle | clicks | naive | 0.5813 | -0.06372 ± 0.00015 | 0.06374 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.5813 | -0.35684 ± 0.00048 | 0.35688 | 0.00 | 5561 | 0.556 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | snips | 0.5813 | -0.17770 ± 0.00072 | 0.17784 | 0.00 | 5561 | 0.556 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | iips | 0.5813 | -0.04726 ± 0.00057 | 0.04760 | 0.00 | 9982 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | sniips | 0.5813 | +0.02415 ± 0.00048 | 0.02461 | 0.00 | 9982 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | rips | 0.5813 | -0.09468 ± 0.00066 | 0.09491 | 0.00 | 5561 | 0.556 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | snrips | 0.5813 | -0.01661 ± 0.00068 | 0.01794 | 0.29 | 5561 | 0.556 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | dm-pbm | 0.5813 | +0.17053 ± 0.00027 | 0.17055 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.5813 | -0.00365 ± 0.00017 | 0.00403 | 0.16 |  |  | 100% | 0.16 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.5813 | +0.14999 ± 0.00040 | 0.15005 | 0.00 | 9982 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | cdr | 0.5813 | +0.00179 ± 0.00044 | 0.00472 | 0.92 | 5561 | 0.556 | 0% |  |  | unbiased though unsupported |
| oracle | satisfied | naive | 0.3287 | -0.06471 ± 0.00014 | 0.06472 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.3287 | -0.22240 ± 0.00033 | 0.22243 | 0.00 | 5561 | 0.556 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | snips | 0.3287 | -0.13755 ± 0.00052 | 0.13765 | 0.00 | 5561 | 0.556 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | iips | 0.3287 | -0.01641 ± 0.00048 | 0.01708 | 0.07 | 9982 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | sniips | 0.3287 | +0.00921 ± 0.00042 | 0.01012 | 0.42 | 9982 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | rips | 0.3287 | -0.04282 ± 0.00048 | 0.04309 | 0.00 | 5561 | 0.556 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | snrips | 0.3287 | -0.01894 ± 0.00046 | 0.01948 | 0.02 | 5561 | 0.556 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | dm-pbm | 0.3287 | +0.09898 ± 0.00025 | 0.09901 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.3287 | -0.00065 ± 0.00018 | 0.00192 | 0.65 |  |  | 100% | 0.65 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.3287 | +0.09590 ± 0.00035 | 0.09596 | 0.00 | 9982 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | cdr | 0.3287 | +0.00287 ± 0.00033 | 0.00439 | 0.90 | 5561 | 0.556 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| reverse | clicks | naive | 0.3732 | +0.14439 ± 0.00015 | 0.14440 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.3732 | -0.37320 ± 0.00000 | 0.37320 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snips | 0.3732 | -0.37320 ± 0.00000 | 0.37320 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | iips | 0.3732 | -0.26794 ± 0.00034 | 0.26796 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | sniips | 0.3732 | -0.26804 ± 0.00033 | 0.26806 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | rips | 0.3732 | -0.26794 ± 0.00034 | 0.26796 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snrips | 0.3732 | -0.26804 ± 0.00033 | 0.26806 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | dm-pbm | 0.3732 | +0.03788 ± 0.00039 | 0.03808 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.3732 | +0.01440 ± 0.00033 | 0.01477 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.3732 | +0.00987 ± 0.00041 | 0.01068 | 0.18 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | cdr | 0.3732 | +0.01124 ± 0.00034 | 0.01175 | 0.03 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | naive | 0.1509 | +0.11315 ± 0.00014 | 0.11316 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.1509 | -0.15088 ± 0.00000 | 0.15088 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snips | 0.1509 | -0.15088 ± 0.00000 | 0.15088 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | iips | 0.1509 | -0.10311 ± 0.00023 | 0.10314 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | sniips | 0.1509 | -0.10316 ± 0.00022 | 0.10318 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | rips | 0.1509 | -0.10311 ± 0.00023 | 0.10314 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snrips | 0.1509 | -0.10316 ± 0.00022 | 0.10318 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | dm-pbm | 0.1509 | +0.05022 ± 0.00024 | 0.05028 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.1509 | +0.02841 ± 0.00020 | 0.02848 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.1509 | +0.03265 ± 0.00026 | 0.03275 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | cdr | 0.1509 | +0.02198 ± 0.00021 | 0.02207 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| wide | clicks | naive | 0.5223 | -0.00467 ± 0.00015 | 0.00492 | 0.15 |  |  | 100% | 0.15 |  | biased, as expected |
| wide | clicks | ips | 0.5223 | -0.50013 ± 0.00015 | 0.50013 | 0.00 | 678 | 0.068 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | snips | 0.5223 | -0.19580 ± 0.00182 | 0.19663 | 0.00 | 678 | 0.068 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | iips | 0.5223 | -0.17023 ± 0.00047 | 0.17030 | 0.00 | 9876 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | sniips | 0.5223 | +0.02711 ± 0.00051 | 0.02759 | 0.00 | 9876 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | rips | 0.5223 | -0.21403 ± 0.00051 | 0.21409 | 0.00 | 678 | 0.068 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | snrips | 0.5223 | -0.03333 ± 0.00105 | 0.03494 | 0.12 | 678 | 0.068 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | dm-pbm | 0.5223 | +0.05465 ± 0.00024 | 0.05471 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.5223 | +0.05059 ± 0.00018 | 0.05063 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.5223 | +0.04519 ± 0.00037 | 0.04534 | 0.00 | 9876 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | cdr | 0.5223 | +0.05313 ± 0.00034 | 0.05324 | 0.00 | 678 | 0.068 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | naive | 0.2715 | -0.00743 ± 0.00014 | 0.00756 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.2715 | -0.26391 ± 0.00009 | 0.26392 | 0.00 | 678 | 0.068 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | snips | 0.2715 | -0.16020 ± 0.00117 | 0.16062 | 0.00 | 678 | 0.068 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | iips | 0.2715 | -0.06971 ± 0.00037 | 0.06981 | 0.00 | 9876 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | sniips | 0.2715 | +0.01894 ± 0.00044 | 0.01943 | 0.01 | 9876 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | rips | 0.2715 | -0.09362 ± 0.00041 | 0.09371 | 0.00 | 678 | 0.068 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | snrips | 0.2715 | -0.02939 ± 0.00067 | 0.03013 | 0.00 | 678 | 0.068 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.2715 | +0.03228 ± 0.00021 | 0.03235 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.2715 | +0.04042 ± 0.00019 | 0.04047 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.2715 | +0.03142 ± 0.00031 | 0.03158 | 0.00 | 9876 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | cdr | 0.2715 | +0.04118 ± 0.00033 | 0.04131 | 0.00 | 678 | 0.068 | 0% |  |  | biased, as expected: 55% of its slots unsupported |

## nfcorpus · dcm · {"policy":"softmax","tau":1.0,"top":10,"by":"rank"}

100 logs of 100000 impressions, 14 s. Propensities: 10000000 lists, 0 mismatched; chi-square 10959 on 11160 df (p 0.911), largest |z| 4.75.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.7033 | 0.3813 | 0.3180 | 2.349 |
| rerank | 0.0% | 0.6838 | 0.3612 | 0.3055 | 2.356 |
| oracle | 0.0% | 0.8014 | 0.4547 | 0.3559 | 2.330 |
| reverse | 0.0% | 0.4596 | 0.1986 | 0.1674 | 2.433 |
| wide | 18.6% | 0.6578 | 0.3404 | 0.2949 | 2.362 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.7033 | -0.04503 ± 0.00023 | 0.04509 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.7033 | +0.00166 ± 0.01032 | 0.10265 | 0.93 | 111 | 0.999 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snips | 0.7033 | +0.00113 ± 0.00753 | 0.07493 | 0.94 | 111 | 0.999 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | iips | 0.7033 | +0.01799 ± 0.00056 | 0.01885 | 0.18 | 11567 | 1.000 | 100% | 0.18 |  | biased, as expected |
| ranking | clicks | sniips | 0.7033 | +0.01807 ± 0.00053 | 0.01882 | 0.11 | 11567 | 1.000 | 100% | 0.11 |  | biased, as expected |
| ranking | clicks | rips | 0.7033 | +0.00446 ± 0.00318 | 0.03199 | 0.95 | 111 | 0.988 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snrips | 0.7033 | +0.00496 ± 0.00291 | 0.02934 | 0.96 | 111 | 0.988 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | dm-pbm | 0.7033 | +0.03244 ± 0.00028 | 0.03255 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.7033 | -0.05449 ± 0.00024 | 0.05454 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.7033 | +0.01781 ± 0.00050 | 0.01850 | 0.07 | 11567 | 1.000 | 100% | 0.07 |  | biased, as expected |
| ranking | clicks | cdr | 0.7033 | +0.00442 ± 0.00269 | 0.02717 | 0.95 | 111 | 0.988 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | naive | 0.3813 | -0.03300 ± 0.00019 | 0.03306 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.3813 | +0.00334 ± 0.00697 | 0.06944 | 0.92 | 111 | 0.999 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snips | 0.3813 | +0.00301 ± 0.00574 | 0.05722 | 0.93 | 111 | 0.999 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | iips | 0.3813 | +0.00990 ± 0.00042 | 0.01075 | 0.45 | 11567 | 1.000 | 100% | 0.45 |  | biased, as expected |
| ranking | satisfied | sniips | 0.3813 | +0.00992 ± 0.00041 | 0.01074 | 0.40 | 11567 | 1.000 | 100% | 0.40 |  | biased, as expected |
| ranking | satisfied | rips | 0.3813 | +0.00069 ± 0.00201 | 0.02005 | 0.95 | 111 | 0.988 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snrips | 0.3813 | +0.00100 ± 0.00196 | 0.01948 | 0.95 | 111 | 0.988 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | dm-pbm | 0.3813 | +0.02206 ± 0.00023 | 0.02219 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.3813 | -0.02251 ± 0.00019 | 0.02259 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.3813 | +0.00959 ± 0.00040 | 0.01037 | 0.31 | 11567 | 1.000 | 100% | 0.31 |  | biased, as expected |
| ranking | satisfied | cdr | 0.3813 | -0.00004 ± 0.00161 | 0.01604 | 0.97 | 111 | 0.988 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| rerank | clicks | naive | 0.6838 | -0.02554 ± 0.00023 | 0.02564 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.6838 | +0.04178 ± 0.05282 | 0.52718 | 0.83 | 18 | 1.076 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 1.08; 8% of logs usable) |
| rerank | clicks | snips | 0.6838 | +0.01290 ± 0.02474 | 0.24652 | 0.83 | 18 | 1.076 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 1.08; 8% of logs usable) |
| rerank | clicks | iips | 0.6838 | +0.00456 ± 0.00069 | 0.00824 | 0.93 | 9650 | 0.998 | 99% | 0.93 |  | biased, as expected |
| rerank | clicks | sniips | 0.6838 | +0.00485 ± 0.00059 | 0.00760 | 0.91 | 9650 | 0.998 | 99% | 0.91 |  | biased, as expected |
| rerank | clicks | rips | 0.6838 | +0.00981 ± 0.01421 | 0.14177 | 0.87 | 17 | 0.947 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 0.95; 7% of logs usable) |
| rerank | clicks | snrips | 0.6838 | +0.00759 ± 0.01019 | 0.10165 | 0.87 | 17 | 0.947 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 0.95; 7% of logs usable) |
| rerank | clicks | dm-pbm | 0.6838 | +0.03073 ± 0.00027 | 0.03084 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.6838 | -0.05383 ± 0.00023 | 0.05388 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.6838 | +0.00521 ± 0.00056 | 0.00761 | 0.89 | 9650 | 0.998 | 99% | 0.89 |  | biased, as expected |
| rerank | clicks | cdr | 0.6838 | +0.00046 ± 0.00933 | 0.09279 | 0.91 | 17 | 0.947 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 0.95; 7% of logs usable) |
| rerank | satisfied | naive | 0.3612 | -0.01294 ± 0.00019 | 0.01308 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.3612 | +0.05361 ± 0.03931 | 0.39483 | 0.81 | 18 | 1.076 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 1.08; 8% of logs usable) |
| rerank | satisfied | snips | 0.3612 | +0.02883 ± 0.01998 | 0.20090 | 0.81 | 18 | 1.076 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 1.08; 8% of logs usable) |
| rerank | satisfied | iips | 0.3612 | +0.00076 ± 0.00045 | 0.00451 | 0.97 | 9650 | 0.998 | 99% | 0.97 |  | unbiased here |
| rerank | satisfied | sniips | 0.3612 | +0.00092 ± 0.00042 | 0.00424 | 0.98 | 9650 | 0.998 | 99% | 0.98 |  | unbiased here |
| rerank | satisfied | rips | 0.3612 | +0.01088 ± 0.01264 | 0.12628 | 0.83 | 17 | 0.947 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 0.95; 7% of logs usable) |
| rerank | satisfied | snrips | 0.3612 | +0.00582 ± 0.00886 | 0.08838 | 0.81 | 17 | 0.947 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 0.95; 7% of logs usable) |
| rerank | satisfied | dm-pbm | 0.3612 | +0.02140 ± 0.00022 | 0.02151 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.3612 | -0.02160 ± 0.00019 | 0.02169 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.3612 | +0.00120 ± 0.00038 | 0.00401 | 0.95 | 9650 | 0.998 | 99% | 0.95 |  | unbiased here |
| rerank | satisfied | cdr | 0.3612 | +0.00578 ± 0.00872 | 0.08696 | 0.86 | 17 | 0.947 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 0.95; 7% of logs usable) |
| oracle | clicks | naive | 0.8014 | -0.14313 ± 0.00023 | 0.14315 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.8014 | -0.06125 ± 0.03058 | 0.31040 | 0.72 | 49 | 0.967 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snips | 0.8014 | -0.05574 ± 0.01671 | 0.17532 | 0.83 | 49 | 0.967 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | iips | 0.8014 | +0.07728 ± 0.00078 | 0.07767 | 0.00 | 11233 | 0.998 | 99% | 0.00 |  | biased, as expected |
| oracle | clicks | sniips | 0.8014 | +0.07719 ± 0.00068 | 0.07749 | 0.00 | 11233 | 0.998 | 99% | 0.00 |  | biased, as expected |
| oracle | clicks | rips | 0.8014 | -0.00677 ± 0.00573 | 0.05738 | 0.89 | 48 | 0.949 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snrips | 0.8014 | -0.00485 ± 0.00514 | 0.05133 | 0.90 | 48 | 0.949 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | dm-pbm | 0.8014 | +0.11384 ± 0.00032 | 0.11388 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.8014 | -0.10234 ± 0.00024 | 0.10237 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.8014 | +0.07723 ± 0.00060 | 0.07747 | 0.00 | 11233 | 0.998 | 99% | 0.00 |  | biased, as expected |
| oracle | clicks | cdr | 0.8014 | -0.00442 ± 0.00538 | 0.05370 | 0.90 | 48 | 0.949 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | naive | 0.4547 | -0.10645 ± 0.00019 | 0.10647 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.4547 | -0.03445 ± 0.02433 | 0.24453 | 0.73 | 49 | 0.967 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snips | 0.4547 | -0.03649 ± 0.01561 | 0.15952 | 0.83 | 49 | 0.967 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | iips | 0.4547 | +0.04862 ± 0.00062 | 0.04901 | 0.00 | 11233 | 0.998 | 99% | 0.00 |  | biased, as expected |
| oracle | satisfied | sniips | 0.4547 | +0.04854 ± 0.00057 | 0.04887 | 0.00 | 11233 | 0.998 | 99% | 0.00 |  | biased, as expected |
| oracle | satisfied | rips | 0.4547 | -0.00174 ± 0.00451 | 0.04489 | 0.81 | 48 | 0.949 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snrips | 0.4547 | -0.00210 ± 0.00395 | 0.03940 | 0.81 | 48 | 0.949 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | dm-pbm | 0.4547 | +0.07451 ± 0.00029 | 0.07457 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.4547 | -0.05127 ± 0.00021 | 0.05131 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.4547 | +0.04872 ± 0.00046 | 0.04893 | 0.00 | 11233 | 0.998 | 99% | 0.00 |  | biased, as expected |
| oracle | satisfied | cdr | 0.4547 | -0.00070 ± 0.00424 | 0.04219 | 0.83 | 48 | 0.949 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| reverse | clicks | naive | 0.4596 | +0.19868 ± 0.00023 | 0.19869 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.4596 | +0.83821 ± 0.96343 | 9.62256 | 0.02 | 0 | 2.163 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 2.16; 0% of logs usable) |
| reverse | clicks | snips | 0.4596 | -0.42961 ± 0.02227 | 0.48339 | 0.00 | 0 | 2.163 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 2.16; 0% of logs usable) |
| reverse | clicks | iips | 0.4596 | -0.01968 ± 0.00086 | 0.02145 | 0.40 | 3413 | 1.001 | 97% | 0.41 |  | biased, as expected |
| reverse | clicks | sniips | 0.4596 | -0.01990 ± 0.00084 | 0.02159 | 0.43 | 3413 | 1.001 | 97% | 0.44 |  | biased, as expected |
| reverse | clicks | rips | 0.4596 | -0.11025 ± 0.04286 | 0.44044 | 0.49 | 0 | 0.565 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.56; 0% of logs usable) |
| reverse | clicks | snrips | 0.4596 | -0.13407 ± 0.02102 | 0.24846 | 0.44 | 0 | 0.565 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.56; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.4596 | +0.03035 ± 0.00034 | 0.03053 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.4596 | +0.00434 ± 0.00030 | 0.00528 | 0.20 |  |  | 100% | 0.20 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.4596 | -0.02022 ± 0.00074 | 0.02153 | 0.25 | 3413 | 1.001 | 97% | 0.26 |  | biased, as expected |
| reverse | clicks | cdr | 0.4596 | -0.10970 ± 0.06438 | 0.64991 | 0.85 | 0 | 0.565 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.56; 0% of logs usable) |
| reverse | satisfied | naive | 0.1986 | +0.14973 ± 0.00019 | 0.14974 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.1986 | +0.23406 ± 0.43261 | 4.31077 | 0.01 | 0 | 2.163 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 2.16; 0% of logs usable) |
| reverse | satisfied | snips | 0.1986 | -0.18855 ± 0.01000 | 0.21320 | 0.00 | 0 | 2.163 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 2.16; 0% of logs usable) |
| reverse | satisfied | iips | 0.1986 | -0.00620 ± 0.00060 | 0.00860 | 0.80 | 3413 | 1.001 | 97% | 0.79 |  | biased, as expected |
| reverse | satisfied | sniips | 0.1986 | -0.00630 ± 0.00059 | 0.00860 | 0.79 | 3413 | 1.001 | 97% | 0.78 |  | biased, as expected |
| reverse | satisfied | rips | 0.1986 | -0.03403 ± 0.03151 | 0.31533 | 0.32 | 0 | 0.565 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.56; 0% of logs usable) |
| reverse | satisfied | snrips | 0.1986 | -0.04185 ± 0.01650 | 0.16946 | 0.31 | 0 | 0.565 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.56; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.1986 | +0.02849 ± 0.00022 | 0.02858 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.1986 | +0.01185 ± 0.00020 | 0.01201 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.1986 | -0.00667 ± 0.00050 | 0.00833 | 0.71 | 3413 | 1.001 | 97% | 0.70 |  | biased, as expected |
| reverse | satisfied | cdr | 0.1986 | -0.03257 ± 0.04137 | 0.41296 | 0.86 | 0 | 0.565 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.56; 0% of logs usable) |
| wide | clicks | naive | 0.6578 | +0.00045 ± 0.00023 | 0.00233 | 0.94 |  |  | 100% | 0.94 |  | unbiased here |
| wide | clicks | ips | 0.6578 | -0.24241 ± 0.02382 | 0.33900 | 0.40 | 15 | 0.556 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snips | 0.6578 | +0.13114 ± 0.02296 | 0.26345 | 0.81 | 15 | 0.556 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | iips | 0.6578 | -0.04650 ± 0.00062 | 0.04690 | 0.00 | 6404 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | sniips | 0.6578 | +0.06181 ± 0.00067 | 0.06217 | 0.00 | 6404 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | rips | 0.6578 | -0.07397 ± 0.01106 | 0.13263 | 0.39 | 14 | 0.687 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snrips | 0.6578 | +0.06354 ± 0.01179 | 0.13345 | 0.93 | 14 | 0.687 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | dm-pbm | 0.6578 | +0.08737 ± 0.00027 | 0.08741 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.6578 | +0.01966 ± 0.00024 | 0.01981 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.6578 | +0.06157 ± 0.00052 | 0.06178 | 0.00 | 6404 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | cdr | 0.6578 | +0.05115 ± 0.00765 | 0.09169 | 0.83 | 14 | 0.687 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | naive | 0.3404 | +0.00793 ± 0.00019 | 0.00815 | 0.02 |  |  | 100% | 0.02 |  | biased, as expected |
| wide | satisfied | ips | 0.3404 | -0.09898 ± 0.02006 | 0.22280 | 0.58 | 15 | 0.556 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | snips | 0.3404 | +0.09944 ± 0.01900 | 0.21364 | 0.86 | 15 | 0.556 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | iips | 0.3404 | -0.01573 ± 0.00042 | 0.01629 | 0.08 | 6404 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | sniips | 0.3404 | +0.03457 ± 0.00048 | 0.03490 | 0.00 | 6404 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | rips | 0.3404 | -0.01729 ± 0.01063 | 0.10721 | 0.56 | 14 | 0.687 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.3404 | +0.04649 ± 0.01140 | 0.12260 | 0.92 | 14 | 0.687 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.3404 | +0.06069 ± 0.00023 | 0.06073 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.3404 | +0.02913 ± 0.00021 | 0.02920 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.3404 | +0.04151 ± 0.00038 | 0.04169 | 0.00 | 6404 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | cdr | 0.3404 | +0.04688 ± 0.00773 | 0.09005 | 0.72 | 14 | 0.687 | 0% |  |  | biased, as expected: 19% of its slots unsupported |

## nfcorpus · dcm · {"policy":"epsilon","epsilon":0.3,"top":10}

100 logs of 100000 impressions, 13 s. Propensities: 10000000 lists, 0 mismatched; chi-square 11072 on 11160 df (p 0.721), largest |z| 3.66.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.7033 | 0.3813 | 0.3180 | 2.349 |
| rerank | 0.0% | 0.6838 | 0.3612 | 0.3055 | 2.356 |
| oracle | 0.0% | 0.8014 | 0.4547 | 0.3559 | 2.330 |
| reverse | 0.0% | 0.4596 | 0.1986 | 0.1674 | 2.433 |
| wide | 18.6% | 0.6578 | 0.3404 | 0.2949 | 2.362 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.7033 | -0.03477 ± 0.00022 | 0.03484 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.7033 | -0.00010 ± 0.00029 | 0.00291 | 0.97 | 69999 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | clicks | snips | 0.7033 | -0.00008 ± 0.00024 | 0.00243 | 0.98 | 69999 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | clicks | iips | 0.7033 | +0.00126 ± 0.00028 | 0.00302 | 0.96 | 72932 | 1.000 | 100% | 0.96 |  | unbiased here |
| ranking | clicks | sniips | 0.7033 | +0.00131 ± 0.00024 | 0.00275 | 0.96 | 72932 | 1.000 | 100% | 0.96 |  | unbiased here |
| ranking | clicks | rips | 0.7033 | -0.00019 ± 0.00029 | 0.00287 | 0.97 | 69999 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | clicks | snrips | 0.7033 | -0.00015 ± 0.00024 | 0.00243 | 0.98 | 69999 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | clicks | dm-pbm | 0.7033 | +0.00796 ± 0.00023 | 0.00828 | 0.02 |  |  | 100% | 0.02 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.7033 | -0.01950 ± 0.00023 | 0.01963 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.7033 | +0.00132 ± 0.00025 | 0.00279 | 0.94 | 72932 | 1.000 | 100% | 0.94 |  | unbiased here |
| ranking | clicks | cdr | 0.7033 | -0.00010 ± 0.00025 | 0.00249 | 0.96 | 69999 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | satisfied | naive | 0.3813 | -0.02683 ± 0.00018 | 0.02689 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.3813 | -0.00020 ± 0.00023 | 0.00232 | 0.99 | 69999 | 1.000 | 100% | 0.99 | yes | ok |
| ranking | satisfied | snips | 0.3813 | -0.00019 ± 0.00021 | 0.00211 | 0.97 | 69999 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | satisfied | iips | 0.3813 | +0.00053 ± 0.00022 | 0.00230 | 0.96 | 72932 | 1.000 | 100% | 0.96 |  | unbiased here |
| ranking | satisfied | sniips | 0.3813 | +0.00056 ± 0.00021 | 0.00214 | 0.96 | 72932 | 1.000 | 100% | 0.96 |  | unbiased here |
| ranking | satisfied | rips | 0.3813 | -0.00024 ± 0.00023 | 0.00230 | 0.97 | 69999 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | satisfied | snrips | 0.3813 | -0.00022 ± 0.00021 | 0.00209 | 0.97 | 69999 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | satisfied | dm-pbm | 0.3813 | +0.00545 ± 0.00020 | 0.00580 | 0.05 |  |  | 100% | 0.05 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.3813 | -0.00361 ± 0.00020 | 0.00411 | 0.28 |  |  | 100% | 0.28 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.3813 | +0.00055 ± 0.00021 | 0.00215 | 0.94 | 72932 | 1.000 | 100% | 0.94 |  | unbiased here |
| ranking | satisfied | cdr | 0.3813 | -0.00020 ± 0.00021 | 0.00210 | 0.96 | 69999 | 1.000 | 100% | 0.96 | yes | ok |
| rerank | clicks | naive | 0.6838 | -0.01527 ± 0.00022 | 0.01543 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.6838 | +0.00014 ± 0.10481 | 1.04286 | 0.38 | 2784 | 0.969 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2784 impressions, mean 0.97; 0% of logs usable) |
| rerank | clicks | snips | 0.6838 | -0.10486 ± 0.04903 | 0.49894 | 0.13 | 2784 | 0.969 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2784 impressions, mean 0.97; 0% of logs usable) |
| rerank | clicks | iips | 0.6838 | +0.01210 ± 0.00124 | 0.01725 | 0.84 | 4005 | 1.001 | 99% | 0.84 |  | biased, as expected |
| rerank | clicks | sniips | 0.6838 | +0.01221 ± 0.00110 | 0.01642 | 0.81 | 4005 | 1.001 | 99% | 0.81 |  | biased, as expected |
| rerank | clicks | rips | 0.6838 | +0.00952 ± 0.04048 | 0.40290 | 0.62 | 5 | 0.644 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 5 impressions, mean 0.64; 0% of logs usable) |
| rerank | clicks | snrips | 0.6838 | -0.00376 ± 0.02614 | 0.26012 | 0.59 | 5 | 0.644 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 5 impressions, mean 0.64; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.6838 | +0.01276 ± 0.00027 | 0.01304 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.6838 | -0.03994 ± 0.00023 | 0.04000 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.6838 | +0.01209 ± 0.00098 | 0.01552 | 0.77 | 4005 | 1.001 | 99% | 0.77 |  | biased, as expected |
| rerank | clicks | cdr | 0.6838 | +0.00894 ± 0.02999 | 0.29853 | 0.81 | 5 | 0.644 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 5 impressions, mean 0.64; 0% of logs usable) |
| rerank | satisfied | naive | 0.3612 | -0.00677 ± 0.00018 | 0.00700 | 0.04 |  |  | 100% | 0.04 |  | biased, as expected |
| rerank | satisfied | ips | 0.3612 | -0.00314 ± 0.06753 | 0.67196 | 0.25 | 2784 | 0.969 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2784 impressions, mean 0.97; 0% of logs usable) |
| rerank | satisfied | snips | 0.3612 | -0.06464 ± 0.03994 | 0.40263 | 0.09 | 2784 | 0.969 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2784 impressions, mean 0.97; 0% of logs usable) |
| rerank | satisfied | iips | 0.3612 | +0.00484 ± 0.00080 | 0.00935 | 0.93 | 4005 | 1.001 | 99% | 0.93 |  | biased, as expected |
| rerank | satisfied | sniips | 0.3612 | +0.00495 ± 0.00073 | 0.00880 | 0.94 | 4005 | 1.001 | 99% | 0.94 |  | biased, as expected |
| rerank | satisfied | rips | 0.3612 | +0.00269 ± 0.02414 | 0.24022 | 0.58 | 5 | 0.644 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 5 impressions, mean 0.64; 0% of logs usable) |
| rerank | satisfied | snrips | 0.3612 | -0.00649 ± 0.01753 | 0.17458 | 0.56 | 5 | 0.644 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 5 impressions, mean 0.64; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.3612 | +0.00945 ± 0.00022 | 0.00969 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.3612 | -0.01584 ± 0.00020 | 0.01597 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.3612 | +0.00486 ± 0.00067 | 0.00822 | 0.91 | 4005 | 1.001 | 99% | 0.91 |  | biased, as expected |
| rerank | satisfied | cdr | 0.3612 | +0.00152 ± 0.01758 | 0.17494 | 0.81 | 5 | 0.644 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 5 impressions, mean 0.64; 0% of logs usable) |
| oracle | clicks | naive | 0.8014 | -0.13287 ± 0.00022 | 0.13289 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.8014 | +0.10601 ± 0.10993 | 1.09892 | 0.39 | 19424 | 1.040 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19424 impressions, mean 1.04; 0% of logs usable) |
| oracle | clicks | snips | 0.8014 | -0.10218 ± 0.05197 | 0.52710 | 0.31 | 19424 | 1.040 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19424 impressions, mean 1.04; 0% of logs usable) |
| oracle | clicks | iips | 0.8014 | +0.07595 ± 0.00145 | 0.07731 | 0.00 | 7335 | 0.999 | 99% | 0.00 |  | biased, as expected |
| oracle | clicks | sniips | 0.8014 | +0.07581 ± 0.00118 | 0.07671 | 0.00 | 7335 | 0.999 | 99% | 0.00 |  | biased, as expected |
| oracle | clicks | rips | 0.8014 | -0.01319 ± 0.01854 | 0.18495 | 0.63 | 14 | 0.811 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 14 impressions, mean 0.81; 0% of logs usable) |
| oracle | clicks | snrips | 0.8014 | -0.01203 ± 0.01367 | 0.13658 | 0.67 | 14 | 0.811 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 14 impressions, mean 0.81; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.8014 | +0.09943 ± 0.00035 | 0.09949 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.8014 | -0.08088 ± 0.00025 | 0.08091 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.8014 | +0.07593 ± 0.00100 | 0.07658 | 0.00 | 7335 | 0.999 | 99% | 0.00 |  | biased, as expected |
| oracle | clicks | cdr | 0.8014 | -0.01314 ± 0.01788 | 0.17834 | 0.64 | 14 | 0.811 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 14 impressions, mean 0.81; 0% of logs usable) |
| oracle | satisfied | naive | 0.4547 | -0.10027 ± 0.00018 | 0.10029 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.4547 | +0.13200 ± 0.09448 | 0.94929 | 0.29 | 19424 | 1.040 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19424 impressions, mean 1.04; 0% of logs usable) |
| oracle | satisfied | snips | 0.4547 | -0.04448 ± 0.05197 | 0.51899 | 0.19 | 19424 | 1.040 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19424 impressions, mean 1.04; 0% of logs usable) |
| oracle | satisfied | iips | 0.4547 | +0.04572 ± 0.00113 | 0.04709 | 0.00 | 7335 | 0.999 | 99% | 0.00 |  | biased, as expected |
| oracle | satisfied | sniips | 0.4547 | +0.04562 ± 0.00100 | 0.04669 | 0.00 | 7335 | 0.999 | 99% | 0.00 |  | biased, as expected |
| oracle | satisfied | rips | 0.4547 | -0.00101 ± 0.01365 | 0.13582 | 0.72 | 14 | 0.811 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 14 impressions, mean 0.81; 0% of logs usable) |
| oracle | satisfied | snrips | 0.4547 | -0.00318 ± 0.01040 | 0.10351 | 0.76 | 14 | 0.811 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 14 impressions, mean 0.81; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.4547 | +0.06426 ± 0.00031 | 0.06433 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.4547 | -0.04139 ± 0.00021 | 0.04145 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.4547 | +0.04598 ± 0.00084 | 0.04673 | 0.00 | 7335 | 0.999 | 99% | 0.00 |  | biased, as expected |
| oracle | satisfied | cdr | 0.4547 | -0.00029 ± 0.01318 | 0.13114 | 0.79 | 14 | 0.811 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 14 impressions, mean 0.81; 0% of logs usable) |
| reverse | clicks | naive | 0.4596 | +0.20894 ± 0.00022 | 0.20896 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.4596 | -0.06649 ± 0.06855 | 0.68533 | 0.29 | 1 | 1.058 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.06; 0% of logs usable) |
| reverse | clicks | snips | 0.4596 | -0.22628 ± 0.04103 | 0.46677 | 0.17 | 1 | 1.058 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.06; 0% of logs usable) |
| reverse | clicks | iips | 0.4596 | -0.00834 ± 0.00126 | 0.01504 | 0.90 | 2935 | 0.995 | 95% | 0.91 |  | biased, as expected |
| reverse | clicks | sniips | 0.4596 | -0.00826 ± 0.00116 | 0.01422 | 0.89 | 2935 | 0.995 | 95% | 0.89 |  | biased, as expected |
| reverse | clicks | rips | 0.4596 | -0.02240 ± 0.02891 | 0.28852 | 0.53 | 1 | 0.714 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.71; 0% of logs usable) |
| reverse | clicks | snrips | 0.4596 | -0.05016 ± 0.02190 | 0.22364 | 0.51 | 1 | 0.714 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.71; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.4596 | +0.05964 ± 0.00045 | 0.05981 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.4596 | +0.00056 ± 0.00043 | 0.00428 | 0.41 |  |  | 100% | 0.41 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.4596 | -0.00836 ± 0.00104 | 0.01332 | 0.88 | 2935 | 0.995 | 95% | 0.87 |  | biased, as expected |
| reverse | clicks | cdr | 0.4596 | -0.02138 ± 0.02473 | 0.24694 | 0.83 | 1 | 0.714 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.71; 0% of logs usable) |
| reverse | satisfied | naive | 0.1986 | +0.15591 ± 0.00018 | 0.15592 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.1986 | -0.05743 ± 0.04057 | 0.40777 | 0.12 | 1 | 1.058 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.06; 0% of logs usable) |
| reverse | satisfied | snips | 0.1986 | -0.11355 ± 0.02768 | 0.29787 | 0.09 | 1 | 1.058 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.06; 0% of logs usable) |
| reverse | satisfied | iips | 0.1986 | -0.00256 ± 0.00090 | 0.00929 | 0.92 | 2935 | 0.995 | 95% | 0.93 |  | unbiased here |
| reverse | satisfied | sniips | 0.1986 | -0.00252 ± 0.00089 | 0.00919 | 0.92 | 2935 | 0.995 | 95% | 0.93 |  | unbiased here |
| reverse | satisfied | rips | 0.1986 | -0.01124 ± 0.02008 | 0.20015 | 0.55 | 1 | 0.714 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.71; 0% of logs usable) |
| reverse | satisfied | snrips | 0.1986 | -0.02464 ± 0.01415 | 0.14289 | 0.56 | 1 | 0.714 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.71; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.1986 | +0.04788 ± 0.00029 | 0.04797 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.1986 | +0.01888 ± 0.00029 | 0.01910 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.1986 | -0.00253 ± 0.00071 | 0.00749 | 0.93 | 2935 | 0.995 | 95% | 0.93 |  | unbiased here |
| reverse | satisfied | cdr | 0.1986 | -0.00511 ± 0.01774 | 0.17662 | 0.88 | 1 | 0.714 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.71; 0% of logs usable) |
| wide | clicks | naive | 0.6578 | +0.01071 ± 0.00022 | 0.01093 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.6578 | -0.30881 ± 0.07564 | 0.81349 | 0.22 | 3257 | 0.420 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snips | 0.6578 | -0.11857 ± 0.04599 | 0.47271 | 0.02 | 3257 | 0.420 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.6578 | -0.04359 ± 0.00110 | 0.04494 | 0.01 | 2531 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | sniips | 0.6578 | +0.06683 ± 0.00127 | 0.06801 | 0.00 | 2531 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | rips | 0.6578 | -0.08635 ± 0.03098 | 0.32013 | 0.37 | 5 | 0.280 | 0% |  |  | unbiased though unsupported |
| wide | clicks | snrips | 0.6578 | +0.02990 ± 0.02807 | 0.28093 | 0.63 | 5 | 0.280 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.6578 | +0.06641 ± 0.00027 | 0.06647 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.6578 | +0.03638 ± 0.00023 | 0.03646 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.6578 | +0.05882 ± 0.00090 | 0.05950 | 0.00 | 2531 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | cdr | 0.6578 | +0.05913 ± 0.02308 | 0.23712 | 0.96 | 5 | 0.280 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | naive | 0.3404 | +0.01410 ± 0.00018 | 0.01422 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.3404 | -0.12889 ± 0.04751 | 0.49001 | 0.17 | 3257 | 0.420 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snips | 0.3404 | -0.05784 ± 0.03942 | 0.39643 | 0.02 | 3257 | 0.420 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.3404 | -0.01391 ± 0.00071 | 0.01559 | 0.57 | 2531 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | sniips | 0.3404 | +0.03765 ± 0.00083 | 0.03854 | 0.02 | 2531 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | rips | 0.3404 | -0.03196 ± 0.01814 | 0.18327 | 0.45 | 5 | 0.280 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.3404 | +0.02038 ± 0.01959 | 0.19599 | 0.68 | 5 | 0.280 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | dm-pbm | 0.3404 | +0.04817 ± 0.00022 | 0.04822 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.3404 | +0.03685 ± 0.00020 | 0.03690 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.3404 | +0.04041 ± 0.00058 | 0.04083 | 0.00 | 2531 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | cdr | 0.3404 | +0.04542 ± 0.01275 | 0.13472 | 0.92 | 5 | 0.280 | 0% |  |  | unbiased though unsupported |

## nfcorpus · dcm · {"policy":"shuffle","top":10}

100 logs of 100000 impressions, 13 s. Propensities: 10000000 lists, 0 mismatched; chi-square 10839 on 11160 df (p 0.985), largest |z| 3.76.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.7033 | 0.3813 | 0.3180 | 2.349 |
| rerank | 0.0% | 0.6838 | 0.3612 | 0.3055 | 2.356 |
| oracle | 0.0% | 0.8014 | 0.4547 | 0.3559 | 2.330 |
| reverse | 0.0% | 0.4596 | 0.1986 | 0.1674 | 2.433 |
| wide | 18.6% | 0.6578 | 0.3404 | 0.2949 | 2.362 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.7033 | -0.11724 ± 0.00023 | 0.11726 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.7033 | +0.07082 ± 0.06423 | 0.64300 | 0.82 | 3 | 1.055 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.06; 0% of logs usable) |
| ranking | clicks | snips | 0.7033 | -0.03544 ± 0.04516 | 0.45075 | 0.67 | 3 | 1.055 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.06; 0% of logs usable) |
| ranking | clicks | iips | 0.7033 | +0.03170 ± 0.00089 | 0.03291 | 0.03 | 9881 | 0.999 | 97% | 0.03 |  | biased, as expected |
| ranking | clicks | sniips | 0.7033 | +0.03160 ± 0.00085 | 0.03270 | 0.02 | 9881 | 0.999 | 97% | 0.02 |  | biased, as expected |
| ranking | clicks | rips | 0.7033 | +0.01086 ± 0.01975 | 0.19680 | 0.77 | 3 | 1.000 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| ranking | clicks | snrips | 0.7033 | +0.00362 ± 0.01953 | 0.19435 | 0.78 | 3 | 1.000 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| ranking | clicks | dm-pbm | 0.7033 | +0.05071 ± 0.00038 | 0.05085 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.7033 | -0.07606 ± 0.00027 | 0.07611 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.7033 | +0.03190 ± 0.00072 | 0.03268 | 0.00 | 9881 | 0.999 | 97% | 0.00 |  | biased, as expected |
| ranking | clicks | cdr | 0.7033 | +0.00516 ± 0.01752 | 0.17439 | 0.79 | 3 | 1.000 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| ranking | satisfied | naive | 0.3813 | -0.09013 ± 0.00018 | 0.09015 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.3813 | +0.03602 ± 0.04483 | 0.44749 | 0.66 | 3 | 1.055 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.06; 0% of logs usable) |
| ranking | satisfied | snips | 0.3813 | -0.02687 ± 0.03316 | 0.33106 | 0.61 | 3 | 1.055 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.06; 0% of logs usable) |
| ranking | satisfied | iips | 0.3813 | +0.01700 ± 0.00063 | 0.01811 | 0.21 | 9881 | 0.999 | 97% | 0.21 |  | biased, as expected |
| ranking | satisfied | sniips | 0.3813 | +0.01691 ± 0.00062 | 0.01798 | 0.20 | 9881 | 0.999 | 97% | 0.20 |  | biased, as expected |
| ranking | satisfied | rips | 0.3813 | -0.00565 ± 0.01068 | 0.10639 | 0.66 | 3 | 1.000 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| ranking | satisfied | snrips | 0.3813 | -0.01022 ± 0.00999 | 0.09994 | 0.63 | 3 | 1.000 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| ranking | satisfied | dm-pbm | 0.3813 | +0.02813 ± 0.00028 | 0.02826 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.3813 | -0.03622 ± 0.00021 | 0.03628 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.3813 | +0.01722 ± 0.00051 | 0.01796 | 0.10 | 9881 | 0.999 | 97% | 0.09 |  | biased, as expected |
| ranking | satisfied | cdr | 0.3813 | -0.00857 ± 0.00919 | 0.09181 | 0.74 | 3 | 1.000 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| rerank | clicks | naive | 0.6838 | -0.09774 ± 0.00023 | 0.09777 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.6838 | -0.00343 ± 0.05836 | 0.58070 | 0.85 | 3 | 1.025 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.03; 0% of logs usable) |
| rerank | clicks | snips | 0.6838 | -0.02450 ± 0.04770 | 0.47528 | 0.57 | 3 | 1.025 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.03; 0% of logs usable) |
| rerank | clicks | iips | 0.6838 | +0.02214 ± 0.00072 | 0.02328 | 0.19 | 9892 | 0.999 | 99% | 0.19 |  | biased, as expected |
| rerank | clicks | sniips | 0.6838 | +0.02207 ± 0.00069 | 0.02311 | 0.16 | 9892 | 0.999 | 99% | 0.16 |  | biased, as expected |
| rerank | clicks | rips | 0.6838 | -0.02087 ± 0.01904 | 0.19057 | 0.76 | 3 | 0.952 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| rerank | clicks | snrips | 0.6838 | -0.02960 ± 0.01815 | 0.18301 | 0.74 | 3 | 0.952 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.6838 | +0.04716 ± 0.00034 | 0.04728 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.6838 | -0.07210 ± 0.00028 | 0.07215 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.6838 | +0.02229 ± 0.00064 | 0.02318 | 0.07 | 9892 | 0.999 | 99% | 0.07 |  | biased, as expected |
| rerank | clicks | cdr | 0.6838 | -0.01623 ± 0.01680 | 0.16796 | 0.83 | 3 | 0.952 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| rerank | satisfied | naive | 0.3612 | -0.07006 ± 0.00018 | 0.07009 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.3612 | +0.00770 ± 0.03858 | 0.38397 | 0.66 | 3 | 1.025 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.03; 0% of logs usable) |
| rerank | satisfied | snips | 0.3612 | +0.01506 ± 0.04013 | 0.39955 | 0.53 | 3 | 1.025 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.03; 0% of logs usable) |
| rerank | satisfied | iips | 0.3612 | +0.01017 ± 0.00058 | 0.01171 | 0.60 | 9892 | 0.999 | 99% | 0.60 |  | biased, as expected |
| rerank | satisfied | sniips | 0.3612 | +0.01010 ± 0.00056 | 0.01156 | 0.55 | 9892 | 0.999 | 99% | 0.55 |  | biased, as expected |
| rerank | satisfied | rips | 0.3612 | -0.00740 ± 0.01338 | 0.13329 | 0.67 | 3 | 0.952 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| rerank | satisfied | snrips | 0.3612 | -0.00720 ± 0.01488 | 0.14823 | 0.63 | 3 | 0.952 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.3612 | +0.02720 ± 0.00026 | 0.02733 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.3612 | -0.03291 ± 0.00022 | 0.03298 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.3612 | +0.01022 ± 0.00052 | 0.01144 | 0.46 | 9892 | 0.999 | 99% | 0.45 |  | biased, as expected |
| rerank | satisfied | cdr | 0.3612 | -0.00398 ± 0.01203 | 0.11977 | 0.75 | 3 | 0.952 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| oracle | clicks | naive | 0.8014 | -0.21534 ± 0.00023 | 0.21535 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.8014 | -0.00914 ± 0.06529 | 0.64974 | 0.88 | 3 | 0.968 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.97; 0% of logs usable) |
| oracle | clicks | snips | 0.8014 | +0.00856 ± 0.05343 | 0.53171 | 0.64 | 3 | 0.968 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.97; 0% of logs usable) |
| oracle | clicks | iips | 0.8014 | +0.09826 ± 0.00100 | 0.09877 | 0.00 | 9879 | 0.999 | 96% | 0.00 |  | biased, as expected |
| oracle | clicks | sniips | 0.8014 | +0.09839 ± 0.00085 | 0.09875 | 0.00 | 9879 | 0.999 | 96% | 0.00 |  | biased, as expected |
| oracle | clicks | rips | 0.8014 | +0.00899 ± 0.01544 | 0.15390 | 0.82 | 3 | 0.964 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.96; 0% of logs usable) |
| oracle | clicks | snrips | 0.8014 | +0.00644 ± 0.01471 | 0.14652 | 0.79 | 3 | 0.964 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.96; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.8014 | +0.13334 ± 0.00041 | 0.13340 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.8014 | -0.12282 ± 0.00027 | 0.12285 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.8014 | +0.09862 ± 0.00073 | 0.09889 | 0.00 | 9879 | 0.999 | 96% | 0.00 |  | biased, as expected |
| oracle | clicks | cdr | 0.8014 | +0.01070 ± 0.01416 | 0.14131 | 0.84 | 3 | 0.964 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.96; 0% of logs usable) |
| oracle | satisfied | naive | 0.4547 | -0.16357 ± 0.00018 | 0.16358 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.4547 | -0.02533 ± 0.04237 | 0.42235 | 0.69 | 3 | 0.968 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.97; 0% of logs usable) |
| oracle | satisfied | snips | 0.4547 | -0.00906 ± 0.04156 | 0.41363 | 0.61 | 3 | 0.968 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.97; 0% of logs usable) |
| oracle | satisfied | iips | 0.4547 | +0.05975 ± 0.00073 | 0.06019 | 0.00 | 9879 | 0.999 | 96% | 0.00 |  | biased, as expected |
| oracle | satisfied | sniips | 0.4547 | +0.05977 ± 0.00063 | 0.06009 | 0.00 | 9879 | 0.999 | 96% | 0.00 |  | biased, as expected |
| oracle | satisfied | rips | 0.4547 | -0.00258 ± 0.00855 | 0.08507 | 0.70 | 3 | 0.964 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.96; 0% of logs usable) |
| oracle | satisfied | snrips | 0.4547 | -0.00307 ± 0.00878 | 0.08744 | 0.69 | 3 | 0.964 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.96; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.4547 | +0.08182 ± 0.00033 | 0.08188 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.4547 | -0.06416 ± 0.00023 | 0.06420 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.4547 | +0.05992 ± 0.00055 | 0.06018 | 0.00 | 9879 | 0.999 | 96% | 0.00 |  | biased, as expected |
| oracle | satisfied | cdr | 0.4547 | -0.00135 ± 0.00808 | 0.08036 | 0.76 | 3 | 0.964 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.96; 0% of logs usable) |
| reverse | clicks | naive | 0.4596 | +0.12648 ± 0.00023 | 0.12650 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.4596 | -0.03323 ± 0.03942 | 0.39358 | 0.70 | 3 | 0.916 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.92; 0% of logs usable) |
| reverse | clicks | snips | 0.4596 | -0.03628 ± 0.03751 | 0.37500 | 0.62 | 3 | 0.916 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.92; 0% of logs usable) |
| reverse | clicks | iips | 0.4596 | -0.01043 ± 0.00075 | 0.01283 | 0.67 | 9902 | 1.001 | 100% | 0.67 |  | biased, as expected |
| reverse | clicks | sniips | 0.4596 | -0.01056 ± 0.00067 | 0.01250 | 0.68 | 9902 | 1.001 | 100% | 0.68 |  | biased, as expected |
| reverse | clicks | rips | 0.4596 | -0.00777 ± 0.01528 | 0.15228 | 0.71 | 3 | 0.882 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.88; 0% of logs usable) |
| reverse | clicks | snrips | 0.4596 | -0.00194 ± 0.01725 | 0.17161 | 0.74 | 3 | 0.882 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.88; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.4596 | +0.01380 ± 0.00024 | 0.01401 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.4596 | -0.01344 ± 0.00024 | 0.01365 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.4596 | -0.01040 ± 0.00061 | 0.01203 | 0.57 | 9902 | 1.001 | 100% | 0.57 |  | biased, as expected |
| reverse | clicks | cdr | 0.4596 | -0.00377 ± 0.01225 | 0.12196 | 0.85 | 3 | 0.882 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.88; 0% of logs usable) |
| reverse | satisfied | naive | 0.1986 | +0.09261 ± 0.00018 | 0.09263 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.1986 | +0.01313 ± 0.02931 | 0.29192 | 0.44 | 3 | 0.916 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.92; 0% of logs usable) |
| reverse | satisfied | snips | 0.1986 | +0.00066 ± 0.02827 | 0.28124 | 0.41 | 3 | 0.916 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.92; 0% of logs usable) |
| reverse | satisfied | iips | 0.1986 | -0.00338 ± 0.00047 | 0.00580 | 0.88 | 9902 | 1.001 | 100% | 0.88 |  | biased, as expected |
| reverse | satisfied | sniips | 0.1986 | -0.00343 ± 0.00044 | 0.00559 | 0.89 | 9902 | 1.001 | 100% | 0.89 |  | biased, as expected |
| reverse | satisfied | rips | 0.1986 | -0.00423 ± 0.00979 | 0.09749 | 0.65 | 3 | 0.882 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.88; 0% of logs usable) |
| reverse | satisfied | snrips | 0.1986 | -0.00622 ± 0.00949 | 0.09467 | 0.65 | 3 | 0.882 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.88; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.1986 | +0.01464 ± 0.00018 | 0.01475 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.1986 | -0.00007 ± 0.00016 | 0.00162 | 0.73 |  |  | 100% | 0.73 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.1986 | -0.00348 ± 0.00040 | 0.00525 | 0.87 | 9902 | 1.001 | 100% | 0.87 |  | biased, as expected |
| reverse | satisfied | cdr | 0.1986 | -0.00162 ± 0.00817 | 0.08134 | 0.86 | 3 | 0.882 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.88; 0% of logs usable) |
| wide | clicks | naive | 0.6578 | -0.07176 ± 0.00023 | 0.07179 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.6578 | -0.25263 ± 0.04713 | 0.53263 | 0.64 | 2 | 0.520 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snips | 0.6578 | -0.03335 ± 0.05757 | 0.57381 | 0.30 | 2 | 0.520 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.6578 | -0.03323 ± 0.00075 | 0.03406 | 0.01 | 6089 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | sniips | 0.6578 | +0.07868 ± 0.00088 | 0.07917 | 0.00 | 6089 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | rips | 0.6578 | -0.09390 ± 0.01624 | 0.18689 | 0.49 | 2 | 0.656 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snrips | 0.6578 | +0.00922 ± 0.02144 | 0.21356 | 0.74 | 2 | 0.656 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.6578 | +0.09276 ± 0.00034 | 0.09282 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.6578 | -0.00979 ± 0.00028 | 0.01017 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.6578 | +0.06478 ± 0.00070 | 0.06515 | 0.00 | 6089 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | cdr | 0.6578 | +0.02997 ± 0.01365 | 0.13907 | 0.94 | 2 | 0.656 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | naive | 0.3404 | -0.04920 ± 0.00018 | 0.04923 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.3404 | -0.09239 ± 0.03227 | 0.33414 | 0.51 | 2 | 0.520 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snips | 0.3404 | +0.04964 ± 0.04701 | 0.47041 | 0.26 | 2 | 0.520 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.3404 | -0.00872 ± 0.00059 | 0.01054 | 0.70 | 6089 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | sniips | 0.3404 | +0.04315 ± 0.00069 | 0.04370 | 0.00 | 6089 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | rips | 0.3404 | -0.02161 ± 0.01266 | 0.12777 | 0.58 | 2 | 0.656 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.3404 | +0.03146 ± 0.01585 | 0.16084 | 0.75 | 2 | 0.656 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | dm-pbm | 0.3404 | +0.05785 ± 0.00027 | 0.05791 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.3404 | +0.00928 ± 0.00023 | 0.00956 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.3404 | +0.03988 ± 0.00056 | 0.04027 | 0.00 | 6089 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | cdr | 0.3404 | +0.04051 ± 0.01085 | 0.11535 | 0.96 | 2 | 0.656 | 0% |  |  | unbiased though unsupported |

## nfcorpus · dcm · {"policy":"swap","top":10}

100 logs of 100000 impressions, 11 s. Propensities: 10000000 lists, 0 mismatched; chi-square 3030 on 3224 df (p 0.993), largest |z| 3.51.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.7033 | 0.3813 | 0.3180 | 2.349 |
| rerank | 50.4% | 0.6838 | 0.3612 | 0.3055 | 2.356 |
| oracle | 24.9% | 0.8014 | 0.4547 | 0.3559 | 2.330 |
| reverse | 80.0% | 0.4596 | 0.1986 | 0.1674 | 2.433 |
| wide | 54.7% | 0.6578 | 0.3404 | 0.2949 | 2.362 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.7033 | -0.04228 ± 0.00022 | 0.04234 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.7033 | +0.00054 ± 0.00109 | 0.01085 | 0.91 | 10003 | 1.000 | 100% | 0.91 | yes | ok |
| ranking | clicks | snips | 0.7033 | +0.00032 ± 0.00078 | 0.00781 | 0.95 | 10003 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | clicks | iips | 0.7033 | +0.01892 ± 0.00048 | 0.01952 | 0.03 | 10003 | 1.000 | 99% | 0.03 |  | biased, as expected |
| ranking | clicks | sniips | 0.7033 | +0.01886 ± 0.00045 | 0.01939 | 0.01 | 10003 | 1.000 | 99% | 0.01 |  | biased, as expected |
| ranking | clicks | rips | 0.7033 | +0.00054 ± 0.00109 | 0.01085 | 0.91 | 10003 | 1.000 | 100% | 0.91 | yes | ok |
| ranking | clicks | snrips | 0.7033 | +0.00032 ± 0.00078 | 0.00781 | 0.95 | 10003 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | clicks | dm-pbm | 0.7033 | +0.03514 ± 0.00026 | 0.03523 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.7033 | -0.04205 ± 0.00022 | 0.04211 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.7033 | +0.01885 ± 0.00036 | 0.01919 | 0.00 | 10003 | 1.000 | 99% | 0.00 |  | biased, as expected |
| ranking | clicks | cdr | 0.7033 | +0.00046 ± 0.00066 | 0.00662 | 0.91 | 10003 | 1.000 | 100% | 0.91 | yes | ok |
| ranking | satisfied | naive | 0.3813 | -0.03701 ± 0.00020 | 0.03707 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.3813 | +0.00028 ± 0.00067 | 0.00662 | 0.98 | 10003 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | satisfied | snips | 0.3813 | +0.00017 ± 0.00058 | 0.00582 | 0.98 | 10003 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | satisfied | iips | 0.3813 | +0.00992 ± 0.00038 | 0.01061 | 0.28 | 10003 | 1.000 | 99% | 0.27 |  | biased, as expected |
| ranking | satisfied | sniips | 0.3813 | +0.00989 ± 0.00038 | 0.01059 | 0.27 | 10003 | 1.000 | 99% | 0.26 |  | biased, as expected |
| ranking | satisfied | rips | 0.3813 | +0.00028 ± 0.00067 | 0.00662 | 0.98 | 10003 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | satisfied | snrips | 0.3813 | +0.00017 ± 0.00058 | 0.00582 | 0.98 | 10003 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | satisfied | dm-pbm | 0.3813 | +0.01910 ± 0.00024 | 0.01925 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.3813 | -0.02083 ± 0.00022 | 0.02095 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.3813 | +0.00991 ± 0.00032 | 0.01042 | 0.16 | 10003 | 1.000 | 99% | 0.16 |  | biased, as expected |
| ranking | satisfied | cdr | 0.3813 | +0.00021 ± 0.00047 | 0.00464 | 0.97 | 10003 | 1.000 | 100% | 0.97 | yes | ok |
| rerank | clicks | naive | 0.6838 | -0.02278 ± 0.00022 | 0.02289 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.6838 | -0.64266 ± 0.00024 | 0.64266 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | snips | 0.6838 | -0.25496 ± 0.00203 | 0.25575 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | iips | 0.6838 | -0.26391 ± 0.00052 | 0.26396 | 0.00 | 10002 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | sniips | 0.6838 | +0.01376 ± 0.00069 | 0.01537 | 0.52 | 10002 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | rips | 0.6838 | -0.33281 ± 0.00069 | 0.33288 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | snrips | 0.6838 | -0.09506 ± 0.00141 | 0.09610 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | dm-pbm | 0.6838 | +0.02476 ± 0.00029 | 0.02493 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.6838 | -0.05382 ± 0.00026 | 0.05388 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.6838 | -0.00247 ± 0.00041 | 0.00480 | 0.90 | 10002 | 0.289 | 0% |  |  | unbiased though unsupported |
| rerank | clicks | cdr | 0.6838 | -0.05449 ± 0.00046 | 0.05468 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | naive | 0.3612 | -0.01695 ± 0.00020 | 0.01707 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.3612 | -0.34528 ± 0.00014 | 0.34528 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | snips | 0.3612 | -0.19504 ± 0.00142 | 0.19556 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | iips | 0.3612 | -0.12236 ± 0.00040 | 0.12243 | 0.00 | 10002 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | sniips | 0.3612 | +0.01290 ± 0.00057 | 0.01408 | 0.35 | 10002 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | rips | 0.3612 | -0.16255 ± 0.00048 | 0.16262 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | snrips | 0.3612 | -0.07352 ± 0.00090 | 0.07406 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | dm-pbm | 0.3612 | +0.01671 ± 0.00023 | 0.01687 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.3612 | -0.02289 ± 0.00022 | 0.02299 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.3612 | +0.00244 ± 0.00036 | 0.00437 | 0.83 | 10002 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | cdr | 0.3612 | -0.02438 ± 0.00040 | 0.02471 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| oracle | clicks | naive | 0.8014 | -0.14038 ± 0.00022 | 0.14040 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.8014 | -0.52606 ± 0.00062 | 0.52609 | 0.00 | 5574 | 0.557 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | snips | 0.8014 | -0.30743 ± 0.00095 | 0.30758 | 0.00 | 5574 | 0.557 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | iips | 0.8014 | -0.20097 ± 0.00060 | 0.20106 | 0.00 | 10002 | 0.659 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | sniips | 0.8014 | -0.10016 ± 0.00058 | 0.10033 | 0.00 | 10002 | 0.659 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | rips | 0.8014 | -0.23505 ± 0.00085 | 0.23521 | 0.00 | 5574 | 0.557 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | snrips | 0.8014 | -0.11013 ± 0.00089 | 0.11048 | 0.00 | 5574 | 0.557 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | dm-pbm | 0.8014 | +0.10729 ± 0.00031 | 0.10733 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.8014 | -0.10166 ± 0.00021 | 0.10168 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.8014 | +0.06807 ± 0.00047 | 0.06823 | 0.00 | 10002 | 0.659 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | cdr | 0.8014 | -0.07830 ± 0.00050 | 0.07846 | 0.00 | 5574 | 0.557 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | naive | 0.4547 | -0.11046 ± 0.00020 | 0.11048 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.4547 | -0.32662 ± 0.00035 | 0.32664 | 0.00 | 5574 | 0.557 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | snips | 0.4547 | -0.22490 ± 0.00059 | 0.22498 | 0.00 | 5574 | 0.557 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | iips | 0.4547 | -0.11400 ± 0.00045 | 0.11409 | 0.00 | 10002 | 0.659 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | sniips | 0.4547 | -0.07594 ± 0.00046 | 0.07608 | 0.00 | 10002 | 0.659 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | rips | 0.4547 | -0.12910 ± 0.00053 | 0.12921 | 0.00 | 5574 | 0.557 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | snrips | 0.4547 | -0.08262 ± 0.00059 | 0.08283 | 0.00 | 5574 | 0.557 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | dm-pbm | 0.4547 | +0.06590 ± 0.00030 | 0.06596 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.4547 | -0.05580 ± 0.00022 | 0.05585 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.4547 | +0.04383 ± 0.00040 | 0.04402 | 0.00 | 10002 | 0.659 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | cdr | 0.4547 | -0.04456 ± 0.00036 | 0.04470 | 0.00 | 5574 | 0.557 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| reverse | clicks | naive | 0.4596 | +0.20143 ± 0.00022 | 0.20144 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.4596 | -0.45961 ± 0.00000 | 0.45961 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snips | 0.4596 | -0.45961 ± 0.00000 | 0.45961 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | iips | 0.4596 | -0.35491 ± 0.00032 | 0.35492 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | sniips | 0.4596 | -0.35494 ± 0.00032 | 0.35495 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | rips | 0.4596 | -0.35491 ± 0.00032 | 0.35492 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snrips | 0.4596 | -0.35494 ± 0.00032 | 0.35495 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | dm-pbm | 0.4596 | +0.03227 ± 0.00044 | 0.03257 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.4596 | -0.00558 ± 0.00043 | 0.00704 | 0.19 |  |  | 100% | 0.19 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.4596 | +0.00431 ± 0.00047 | 0.00634 | 0.61 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | cdr | 0.4596 | -0.01122 ± 0.00045 | 0.01207 | 0.11 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | naive | 0.1986 | +0.14572 ± 0.00020 | 0.14574 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.1986 | -0.19855 ± 0.00000 | 0.19855 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snips | 0.1986 | -0.19855 ± 0.00000 | 0.19855 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | iips | 0.1986 | -0.15108 ± 0.00021 | 0.15110 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | sniips | 0.1986 | -0.15110 ± 0.00021 | 0.15111 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | rips | 0.1986 | -0.15108 ± 0.00021 | 0.15110 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snrips | 0.1986 | -0.15110 ± 0.00021 | 0.15111 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | dm-pbm | 0.1986 | +0.04418 ± 0.00029 | 0.04428 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.1986 | +0.02189 ± 0.00030 | 0.02209 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.1986 | +0.02605 ± 0.00030 | 0.02622 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | cdr | 0.1986 | +0.01394 ± 0.00031 | 0.01427 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| wide | clicks | naive | 0.6578 | +0.00320 ± 0.00022 | 0.00388 | 0.76 |  |  | 100% | 0.76 |  | biased, as expected |
| wide | clicks | ips | 0.6578 | -0.63126 ± 0.00020 | 0.63126 | 0.00 | 673 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | snips | 0.6578 | -0.26270 ± 0.00241 | 0.26379 | 0.00 | 673 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | iips | 0.6578 | -0.25626 ± 0.00051 | 0.25631 | 0.00 | 9891 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | sniips | 0.6578 | +0.05266 ± 0.00067 | 0.05308 | 0.00 | 9891 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | rips | 0.6578 | -0.31674 ± 0.00067 | 0.31681 | 0.00 | 673 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | snrips | 0.6578 | -0.05692 ± 0.00159 | 0.05906 | 0.04 | 673 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | dm-pbm | 0.6578 | +0.08098 ± 0.00031 | 0.08103 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.6578 | +0.02160 ± 0.00026 | 0.02175 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.6578 | +0.05339 ± 0.00043 | 0.05356 | 0.00 | 9891 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | cdr | 0.6578 | +0.01902 ± 0.00045 | 0.01954 | 0.02 | 673 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | naive | 0.3404 | +0.00392 ± 0.00020 | 0.00440 | 0.51 |  |  | 100% | 0.51 |  | biased, as expected |
| wide | satisfied | ips | 0.3404 | -0.33061 ± 0.00011 | 0.33061 | 0.00 | 673 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | snips | 0.3404 | -0.19539 ± 0.00158 | 0.19602 | 0.00 | 673 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | iips | 0.3404 | -0.10798 ± 0.00040 | 0.10805 | 0.00 | 9891 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | sniips | 0.3404 | +0.05000 ± 0.00057 | 0.05032 | 0.00 | 9891 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | rips | 0.3404 | -0.14532 ± 0.00047 | 0.14540 | 0.00 | 673 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | snrips | 0.3404 | -0.04665 ± 0.00098 | 0.04766 | 0.00 | 673 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.3404 | +0.05504 ± 0.00025 | 0.05510 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.3404 | +0.02849 ± 0.00023 | 0.02858 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.3404 | +0.04075 ± 0.00037 | 0.04091 | 0.00 | 9891 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | cdr | 0.3404 | +0.02651 ± 0.00040 | 0.02681 | 0.00 | 673 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |

## nfcorpus · dbn · {"policy":"softmax","tau":1.0,"top":10,"by":"rank"}

100 logs of 100000 impressions, 13 s. Propensities: 10000000 lists, 0 mismatched; chi-square 10921 on 11160 df (p 0.946), largest |z| 3.86.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.5206 | 0.2840 | 0.2840 | 2.368 |
| rerank | 0.0% | 0.5066 | 0.2698 | 0.2698 | 2.376 |
| oracle | 0.0% | 0.5979 | 0.3429 | 0.3429 | 2.337 |
| reverse | 0.0% | 0.3279 | 0.1353 | 0.1353 | 2.453 |
| wide | 18.6% | 0.4974 | 0.2624 | 0.2624 | 2.380 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.5206 | -0.04539 ± 0.00020 | 0.04544 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.5206 | -0.00528 ± 0.00787 | 0.07851 | 0.90 | 111 | 0.997 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snips | 0.5206 | -0.00409 ± 0.00562 | 0.05606 | 0.92 | 111 | 0.997 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | iips | 0.5206 | +0.02059 ± 0.00052 | 0.02122 | 0.03 | 11573 | 1.001 | 97% | 0.03 |  | biased, as expected |
| ranking | clicks | sniips | 0.5206 | +0.02056 ± 0.00050 | 0.02115 | 0.02 | 11573 | 1.001 | 97% | 0.02 |  | biased, as expected |
| ranking | clicks | rips | 0.5206 | +0.00335 ± 0.00235 | 0.02360 | 0.94 | 111 | 0.994 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snrips | 0.5206 | +0.00380 ± 0.00224 | 0.02259 | 0.94 | 111 | 0.994 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | dm-pbm | 0.5206 | +0.03549 ± 0.00025 | 0.03557 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.5206 | -0.01459 ± 0.00022 | 0.01476 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.5206 | +0.02027 ± 0.00045 | 0.02075 | 0.00 | 11573 | 1.001 | 97% | 0.00 |  | biased, as expected |
| ranking | clicks | cdr | 0.5206 | +0.00369 ± 0.00205 | 0.02068 | 0.94 | 111 | 0.994 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | naive | 0.2840 | -0.03600 ± 0.00013 | 0.03602 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.2840 | -0.00425 ± 0.00476 | 0.04756 | 0.96 | 111 | 0.997 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snips | 0.2840 | -0.00349 ± 0.00380 | 0.03795 | 0.99 | 111 | 0.997 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | iips | 0.2840 | +0.01206 ± 0.00035 | 0.01254 | 0.08 | 11573 | 1.001 | 97% | 0.08 |  | biased, as expected |
| ranking | satisfied | sniips | 0.2840 | +0.01206 ± 0.00034 | 0.01253 | 0.07 | 11573 | 1.001 | 97% | 0.06 |  | biased, as expected |
| ranking | satisfied | rips | 0.2840 | +0.00104 ± 0.00140 | 0.01402 | 0.89 | 111 | 0.994 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snrips | 0.2840 | +0.00139 ± 0.00143 | 0.01429 | 0.90 | 111 | 0.994 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | dm-pbm | 0.2840 | +0.02479 ± 0.00017 | 0.02484 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.2840 | -0.00549 ± 0.00015 | 0.00570 | 0.01 |  |  | 100% | 0.01 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.2840 | +0.01181 ± 0.00030 | 0.01218 | 0.02 | 11573 | 1.001 | 97% | 0.02 |  | biased, as expected |
| ranking | satisfied | cdr | 0.2840 | +0.00106 ± 0.00131 | 0.01311 | 0.94 | 111 | 0.994 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| rerank | clicks | naive | 0.5066 | -0.03138 ± 0.00020 | 0.03144 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.5066 | +0.01750 ± 0.04072 | 0.40556 | 0.84 | 19 | 0.979 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.98; 8% of logs usable) |
| rerank | clicks | snips | 0.5066 | +0.03634 ± 0.02038 | 0.20600 | 0.85 | 19 | 0.979 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.98; 8% of logs usable) |
| rerank | clicks | iips | 0.5066 | +0.00759 ± 0.00061 | 0.00972 | 0.77 | 9677 | 1.000 | 99% | 0.77 |  | biased, as expected |
| rerank | clicks | sniips | 0.5066 | +0.00749 ± 0.00056 | 0.00932 | 0.72 | 9677 | 1.000 | 99% | 0.73 |  | biased, as expected |
| rerank | clicks | rips | 0.5066 | +0.00152 ± 0.01355 | 0.13482 | 0.78 | 19 | 0.935 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.94; 3% of logs usable) |
| rerank | clicks | snrips | 0.5066 | +0.00204 ± 0.00905 | 0.09007 | 0.82 | 19 | 0.935 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.94; 3% of logs usable) |
| rerank | clicks | dm-pbm | 0.5066 | +0.03262 ± 0.00027 | 0.03273 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.5066 | -0.01250 ± 0.00024 | 0.01274 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.5066 | +0.00708 ± 0.00052 | 0.00879 | 0.73 | 9677 | 1.000 | 99% | 0.73 |  | biased, as expected |
| rerank | clicks | cdr | 0.5066 | +0.00044 ± 0.01160 | 0.11546 | 0.90 | 19 | 0.935 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.94; 3% of logs usable) |
| rerank | satisfied | naive | 0.2698 | -0.02186 ± 0.00013 | 0.02190 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.2698 | +0.02286 ± 0.02830 | 0.28247 | 0.84 | 19 | 0.979 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.98; 8% of logs usable) |
| rerank | satisfied | snips | 0.2698 | +0.02932 ± 0.01330 | 0.13550 | 0.82 | 19 | 0.979 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.98; 8% of logs usable) |
| rerank | satisfied | iips | 0.2698 | +0.00273 ± 0.00043 | 0.00505 | 0.88 | 9677 | 1.000 | 99% | 0.88 |  | biased, as expected |
| rerank | satisfied | sniips | 0.2698 | +0.00268 ± 0.00041 | 0.00485 | 0.91 | 9677 | 1.000 | 99% | 0.91 |  | biased, as expected |
| rerank | satisfied | rips | 0.2698 | +0.00775 ± 0.01000 | 0.09983 | 0.79 | 19 | 0.935 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.94; 3% of logs usable) |
| rerank | satisfied | snrips | 0.2698 | +0.00648 ± 0.00615 | 0.06156 | 0.77 | 19 | 0.935 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.94; 3% of logs usable) |
| rerank | satisfied | dm-pbm | 0.2698 | +0.02259 ± 0.00019 | 0.02267 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.2698 | -0.00225 ± 0.00016 | 0.00276 | 0.34 |  |  | 100% | 0.34 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.2698 | +0.00239 ± 0.00036 | 0.00432 | 0.89 | 9677 | 1.000 | 99% | 0.89 |  | biased, as expected |
| rerank | satisfied | cdr | 0.2698 | +0.00728 ± 0.00900 | 0.08981 | 0.90 | 19 | 0.935 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.94; 3% of logs usable) |
| oracle | clicks | naive | 0.5979 | -0.12272 ± 0.00020 | 0.12274 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.5979 | +0.00950 ± 0.03267 | 0.32524 | 0.83 | 49 | 1.017 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snips | 0.5979 | -0.01888 ± 0.01217 | 0.12254 | 0.89 | 49 | 1.017 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | iips | 0.5979 | +0.08950 ± 0.00069 | 0.08976 | 0.00 | 11237 | 1.001 | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | sniips | 0.5979 | +0.08893 ± 0.00059 | 0.08912 | 0.00 | 11237 | 1.001 | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | rips | 0.5979 | +0.02848 ± 0.02614 | 0.26160 | 0.88 | 48 | 0.968 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snrips | 0.5979 | +0.00823 ± 0.00853 | 0.08526 | 0.87 | 48 | 0.968 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | dm-pbm | 0.5979 | +0.12187 ± 0.00030 | 0.12190 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.5979 | -0.05359 ± 0.00022 | 0.05363 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.5979 | +0.08870 ± 0.00057 | 0.08888 | 0.00 | 11237 | 1.001 | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | cdr | 0.5979 | +0.02492 ± 0.02281 | 0.22835 | 0.90 | 48 | 0.968 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | naive | 0.3429 | -0.09490 ± 0.00013 | 0.09491 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.3429 | -0.01094 ± 0.01620 | 0.16157 | 0.83 | 49 | 1.017 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snips | 0.3429 | -0.01958 ± 0.01009 | 0.10225 | 0.83 | 49 | 1.017 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | iips | 0.3429 | +0.05905 ± 0.00054 | 0.05929 | 0.00 | 11237 | 1.001 | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | sniips | 0.3429 | +0.05873 ± 0.00048 | 0.05892 | 0.00 | 11237 | 1.001 | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | rips | 0.3429 | -0.00013 ± 0.00204 | 0.02030 | 0.83 | 48 | 0.968 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snrips | 0.3429 | -0.00055 ± 0.00194 | 0.01928 | 0.83 | 48 | 0.968 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | dm-pbm | 0.3429 | +0.08064 ± 0.00024 | 0.08068 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.3429 | -0.03316 ± 0.00016 | 0.03320 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.3429 | +0.05861 ± 0.00042 | 0.05875 | 0.00 | 11237 | 1.001 | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | cdr | 0.3429 | -0.00119 ± 0.00234 | 0.02331 | 0.90 | 48 | 0.968 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| reverse | clicks | naive | 0.3279 | +0.14730 ± 0.00020 | 0.14731 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.3279 | +0.53730 ± 0.60870 | 6.08032 | 0.02 | 0 | 1.730 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 1.73; 0% of logs usable) |
| reverse | clicks | snips | 0.3279 | -0.30792 ± 0.01407 | 0.33825 | 0.00 | 0 | 1.730 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 1.73; 0% of logs usable) |
| reverse | clicks | iips | 0.3279 | -0.02185 ± 0.00075 | 0.02310 | 0.19 | 3415 | 1.004 | 100% | 0.19 |  | biased, as expected |
| reverse | clicks | sniips | 0.3279 | -0.02196 ± 0.00075 | 0.02319 | 0.19 | 3415 | 1.004 | 100% | 0.19 |  | biased, as expected |
| reverse | clicks | rips | 0.3279 | +0.07730 ± 0.07111 | 0.71173 | 0.36 | 0 | 0.088 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.09; 0% of logs usable) |
| reverse | clicks | snrips | 0.3279 | -0.03769 ± 0.02597 | 0.26117 | 0.30 | 0 | 0.088 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.09; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.3279 | +0.03253 ± 0.00031 | 0.03268 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.3279 | +0.00755 ± 0.00027 | 0.00802 | 0.01 |  |  | 100% | 0.01 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.3279 | -0.02166 ± 0.00074 | 0.02288 | 0.15 | 3415 | 1.004 | 100% | 0.15 |  | biased, as expected |
| reverse | clicks | cdr | 0.3279 | +0.04842 ± 0.05282 | 0.52777 | 0.96 | 0 | 0.088 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.09; 0% of logs usable) |
| reverse | satisfied | naive | 0.1353 | +0.11264 ± 0.00013 | 0.11265 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.1353 | +0.29729 ± 0.43261 | 4.31466 | 0.01 | 0 | 1.730 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 1.73; 0% of logs usable) |
| reverse | satisfied | snips | 0.1353 | -0.12531 ± 0.01000 | 0.16001 | 0.00 | 0 | 1.730 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 1.73; 0% of logs usable) |
| reverse | satisfied | iips | 0.1353 | -0.00667 ± 0.00048 | 0.00820 | 0.74 | 3415 | 1.004 | 100% | 0.74 |  | biased, as expected |
| reverse | satisfied | sniips | 0.1353 | -0.00672 ± 0.00046 | 0.00812 | 0.73 | 3415 | 1.004 | 100% | 0.73 |  | biased, as expected |
| reverse | satisfied | rips | 0.1353 | +0.07860 ± 0.05751 | 0.57761 | 0.23 | 0 | 0.088 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.09; 0% of logs usable) |
| reverse | satisfied | snrips | 0.1353 | +0.00097 ± 0.02007 | 0.19973 | 0.21 | 0 | 0.088 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.09; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.1353 | +0.02847 ± 0.00020 | 0.02854 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.1353 | +0.01949 ± 0.00017 | 0.01956 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.1353 | -0.00665 ± 0.00040 | 0.00774 | 0.72 | 3415 | 1.004 | 100% | 0.72 |  | biased, as expected |
| reverse | satisfied | cdr | 0.1353 | +0.07988 ± 0.04646 | 0.46908 | 0.98 | 0 | 0.088 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.09; 0% of logs usable) |
| wide | clicks | naive | 0.4974 | -0.02219 ± 0.00020 | 0.02228 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.4974 | -0.19258 ± 0.02499 | 0.31453 | 0.40 | 14 | 0.480 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snips | 0.4974 | +0.13914 ± 0.02018 | 0.24427 | 0.84 | 14 | 0.480 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | iips | 0.4974 | -0.02590 ± 0.00057 | 0.02651 | 0.02 | 6402 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | sniips | 0.4974 | +0.03560 ± 0.00064 | 0.03616 | 0.00 | 6402 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | rips | 0.4974 | -0.04912 ± 0.00390 | 0.06262 | 0.42 | 14 | 0.629 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snrips | 0.4974 | +0.03806 ± 0.00716 | 0.08078 | 0.91 | 14 | 0.629 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | dm-pbm | 0.4974 | +0.05845 ± 0.00027 | 0.05851 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.4974 | +0.03384 ± 0.00025 | 0.03393 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.4974 | +0.03466 ± 0.00051 | 0.03502 | 0.00 | 6402 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | cdr | 0.4974 | +0.05708 ± 0.00380 | 0.06845 | 0.43 | 14 | 0.629 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | naive | 0.2624 | -0.01443 ± 0.00013 | 0.01449 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.2624 | -0.08835 ± 0.02419 | 0.25640 | 0.47 | 14 | 0.480 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snips | 0.2624 | +0.07442 ± 0.01452 | 0.16252 | 0.86 | 14 | 0.480 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | iips | 0.2624 | -0.00910 ± 0.00039 | 0.00987 | 0.31 | 6402 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | sniips | 0.2624 | +0.01812 ± 0.00044 | 0.01864 | 0.01 | 6402 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | rips | 0.2624 | -0.01541 ± 0.00269 | 0.03085 | 0.59 | 14 | 0.629 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | snrips | 0.2624 | +0.02544 ± 0.00524 | 0.05801 | 0.95 | 14 | 0.629 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.2624 | +0.03999 ± 0.00019 | 0.04003 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.2624 | +0.02742 ± 0.00017 | 0.02747 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.2624 | +0.02241 ± 0.00033 | 0.02265 | 0.00 | 6402 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | cdr | 0.2624 | +0.03882 ± 0.00249 | 0.04603 | 0.26 | 14 | 0.629 | 0% |  |  | biased, as expected: 19% of its slots unsupported |

## nfcorpus · dbn · {"policy":"epsilon","epsilon":0.3,"top":10}

100 logs of 100000 impressions, 12 s. Propensities: 10000000 lists, 0 mismatched; chi-square 11388 on 11160 df (p 0.064), largest |z| 3.77.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.5206 | 0.2840 | 0.2840 | 2.368 |
| rerank | 0.0% | 0.5066 | 0.2698 | 0.2698 | 2.376 |
| oracle | 0.0% | 0.5979 | 0.3429 | 0.3429 | 2.337 |
| reverse | 0.0% | 0.3279 | 0.1353 | 0.1353 | 2.453 |
| wide | 18.6% | 0.4974 | 0.2624 | 0.2624 | 2.380 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.5206 | -0.03013 ± 0.00020 | 0.03020 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.5206 | -0.00008 ± 0.00026 | 0.00260 | 0.94 | 70000 | 1.000 | 99% | 0.95 | yes | ok |
| ranking | clicks | snips | 0.5206 | -0.00007 ± 0.00023 | 0.00232 | 0.94 | 70000 | 1.000 | 99% | 0.94 | yes | ok |
| ranking | clicks | iips | 0.5206 | +0.00157 ± 0.00026 | 0.00301 | 0.90 | 72934 | 1.000 | 99% | 0.90 |  | unbiased here |
| ranking | clicks | sniips | 0.5206 | +0.00159 ± 0.00023 | 0.00282 | 0.91 | 72934 | 1.000 | 99% | 0.91 |  | unbiased here |
| ranking | clicks | rips | 0.5206 | -0.00005 ± 0.00026 | 0.00261 | 0.94 | 70000 | 1.000 | 99% | 0.95 | yes | ok |
| ranking | clicks | snrips | 0.5206 | -0.00004 ± 0.00023 | 0.00232 | 0.95 | 70000 | 1.000 | 99% | 0.95 | yes | ok |
| ranking | clicks | dm-pbm | 0.5206 | +0.00948 ± 0.00021 | 0.00971 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.5206 | -0.00439 ± 0.00021 | 0.00485 | 0.17 |  |  | 100% | 0.17 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.5206 | +0.00156 ± 0.00023 | 0.00273 | 0.88 | 72934 | 1.000 | 99% | 0.88 |  | unbiased here |
| ranking | clicks | cdr | 0.5206 | -0.00009 ± 0.00022 | 0.00221 | 0.98 | 70000 | 1.000 | 99% | 0.98 | yes | ok |
| ranking | satisfied | naive | 0.2840 | -0.02432 ± 0.00015 | 0.02436 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.2840 | -0.00015 ± 0.00020 | 0.00197 | 0.96 | 70000 | 1.000 | 99% | 0.97 | yes | ok |
| ranking | satisfied | snips | 0.2840 | -0.00015 ± 0.00018 | 0.00183 | 0.94 | 70000 | 1.000 | 99% | 0.94 | yes | ok |
| ranking | satisfied | iips | 0.2840 | +0.00076 ± 0.00019 | 0.00207 | 0.87 | 72934 | 1.000 | 99% | 0.88 |  | unbiased here |
| ranking | satisfied | sniips | 0.2840 | +0.00077 ± 0.00018 | 0.00196 | 0.93 | 72934 | 1.000 | 99% | 0.93 |  | unbiased here |
| ranking | satisfied | rips | 0.2840 | -0.00014 ± 0.00020 | 0.00197 | 0.94 | 70000 | 1.000 | 99% | 0.95 | yes | ok |
| ranking | satisfied | snrips | 0.2840 | -0.00014 ± 0.00018 | 0.00182 | 0.92 | 70000 | 1.000 | 99% | 0.92 | yes | ok |
| ranking | satisfied | dm-pbm | 0.2840 | +0.00719 ± 0.00016 | 0.00737 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.2840 | +0.00228 ± 0.00016 | 0.00279 | 0.32 |  |  | 100% | 0.32 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.2840 | +0.00076 ± 0.00017 | 0.00186 | 0.93 | 72934 | 1.000 | 99% | 0.93 |  | unbiased here |
| ranking | satisfied | cdr | 0.2840 | -0.00016 ± 0.00017 | 0.00170 | 0.92 | 70000 | 1.000 | 99% | 0.93 | yes | ok |
| rerank | clicks | naive | 0.5066 | -0.01611 ± 0.00020 | 0.01624 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.5066 | +0.01731 ± 0.08166 | 0.81266 | 0.35 | 2602 | 0.970 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2602 impressions, mean 0.97; 0% of logs usable) |
| rerank | clicks | snips | 0.5066 | -0.06804 ± 0.03721 | 0.37642 | 0.13 | 2602 | 0.970 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2602 impressions, mean 0.97; 0% of logs usable) |
| rerank | clicks | iips | 0.5066 | +0.01074 ± 0.00092 | 0.01413 | 0.83 | 3994 | 0.997 | 99% | 0.83 |  | biased, as expected |
| rerank | clicks | sniips | 0.5066 | +0.01063 ± 0.00091 | 0.01396 | 0.83 | 3994 | 0.997 | 99% | 0.83 |  | biased, as expected |
| rerank | clicks | rips | 0.5066 | -0.01371 ± 0.01631 | 0.16284 | 0.67 | 4 | 0.774 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.77; 0% of logs usable) |
| rerank | clicks | snrips | 0.5066 | -0.01575 ± 0.01279 | 0.12827 | 0.70 | 4 | 0.774 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.77; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.5066 | +0.01503 ± 0.00024 | 0.01522 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.5066 | -0.00329 ± 0.00023 | 0.00399 | 0.27 |  |  | 100% | 0.27 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.5066 | +0.01062 ± 0.00082 | 0.01337 | 0.76 | 3994 | 0.997 | 99% | 0.76 |  | biased, as expected |
| rerank | clicks | cdr | 0.5066 | -0.00735 ± 0.01616 | 0.16101 | 0.95 | 4 | 0.774 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.77; 0% of logs usable) |
| rerank | satisfied | naive | 0.2698 | -0.01018 ± 0.00015 | 0.01028 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.2698 | -0.04613 ± 0.05023 | 0.50188 | 0.18 | 2602 | 0.970 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2602 impressions, mean 0.97; 0% of logs usable) |
| rerank | satisfied | snips | 0.2698 | -0.10122 ± 0.02409 | 0.26018 | 0.12 | 2602 | 0.970 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2602 impressions, mean 0.97; 0% of logs usable) |
| rerank | satisfied | iips | 0.2698 | +0.00349 ± 0.00068 | 0.00759 | 0.94 | 3994 | 0.997 | 99% | 0.94 |  | biased, as expected |
| rerank | satisfied | sniips | 0.2698 | +0.00340 ± 0.00065 | 0.00735 | 0.93 | 3994 | 0.997 | 99% | 0.93 |  | biased, as expected |
| rerank | satisfied | rips | 0.2698 | -0.00532 ± 0.01184 | 0.11791 | 0.72 | 4 | 0.774 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.77; 0% of logs usable) |
| rerank | satisfied | snrips | 0.2698 | -0.01258 ± 0.00666 | 0.06741 | 0.74 | 4 | 0.774 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.77; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.2698 | +0.01021 ± 0.00017 | 0.01034 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.2698 | +0.00438 ± 0.00017 | 0.00470 | 0.04 |  |  | 100% | 0.04 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.2698 | +0.00345 ± 0.00056 | 0.00657 | 0.93 | 3994 | 0.997 | 99% | 0.93 |  | biased, as expected |
| rerank | satisfied | cdr | 0.2698 | -0.00086 ± 0.01052 | 0.10469 | 0.92 | 4 | 0.774 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.77; 0% of logs usable) |
| oracle | clicks | naive | 0.5979 | -0.10746 ± 0.00020 | 0.10748 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.5979 | -0.02083 ± 0.07579 | 0.75442 | 0.27 | 22195 | 0.949 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 22195 impressions, mean 0.95; 0% of logs usable) |
| oracle | clicks | snips | 0.5979 | -0.11590 ± 0.03069 | 0.32661 | 0.18 | 22195 | 0.949 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 22195 impressions, mean 0.95; 0% of logs usable) |
| oracle | clicks | iips | 0.5979 | +0.08442 ± 0.00108 | 0.08510 | 0.00 | 7327 | 1.000 | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | sniips | 0.5979 | +0.08451 ± 0.00086 | 0.08494 | 0.00 | 7327 | 1.000 | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | rips | 0.5979 | +0.00601 ± 0.01533 | 0.15264 | 0.73 | 14 | 0.751 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 14 impressions, mean 0.75; 0% of logs usable) |
| oracle | clicks | snrips | 0.5979 | +0.00388 ± 0.00918 | 0.09137 | 0.75 | 14 | 0.751 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 14 impressions, mean 0.75; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.5979 | +0.10641 ± 0.00025 | 0.10644 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.5979 | -0.04365 ± 0.00021 | 0.04370 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.5979 | +0.08441 ± 0.00068 | 0.08468 | 0.00 | 7327 | 1.000 | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | cdr | 0.5979 | +0.00853 ± 0.01405 | 0.14006 | 0.92 | 14 | 0.751 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 14 impressions, mean 0.75; 0% of logs usable) |
| oracle | satisfied | naive | 0.3429 | -0.08323 ± 0.00015 | 0.08324 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.3429 | -0.01578 ± 0.05038 | 0.50154 | 0.22 | 22195 | 0.949 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 22195 impressions, mean 0.95; 0% of logs usable) |
| oracle | satisfied | snips | 0.3429 | -0.08452 ± 0.02462 | 0.25914 | 0.02 | 22195 | 0.949 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 22195 impressions, mean 0.95; 0% of logs usable) |
| oracle | satisfied | iips | 0.3429 | +0.05391 ± 0.00079 | 0.05448 | 0.00 | 7327 | 1.000 | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | sniips | 0.3429 | +0.05400 ± 0.00067 | 0.05441 | 0.00 | 7327 | 1.000 | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | rips | 0.3429 | +0.01613 ± 0.01455 | 0.14566 | 0.65 | 14 | 0.751 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 14 impressions, mean 0.75; 0% of logs usable) |
| oracle | satisfied | snrips | 0.3429 | +0.00939 ± 0.00861 | 0.08615 | 0.66 | 14 | 0.751 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 14 impressions, mean 0.75; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.3429 | +0.06883 ± 0.00021 | 0.06887 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.3429 | -0.02521 ± 0.00017 | 0.02527 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.3429 | +0.05388 ± 0.00053 | 0.05414 | 0.00 | 7327 | 1.000 | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | cdr | 0.3429 | +0.01769 ± 0.01386 | 0.13899 | 0.88 | 14 | 0.751 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 14 impressions, mean 0.75; 0% of logs usable) |
| reverse | clicks | naive | 0.3279 | +0.16257 ± 0.00020 | 0.16258 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.3279 | +0.02488 ± 0.07073 | 0.70424 | 0.26 | 1 | 0.948 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.95; 0% of logs usable) |
| reverse | clicks | snips | 0.3279 | -0.12825 ± 0.03717 | 0.39148 | 0.15 | 1 | 0.948 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.95; 0% of logs usable) |
| reverse | clicks | iips | 0.3279 | -0.00985 ± 0.00113 | 0.01498 | 0.80 | 2948 | 1.003 | 100% | 0.80 |  | biased, as expected |
| reverse | clicks | sniips | 0.3279 | -0.01072 ± 0.00104 | 0.01488 | 0.80 | 2948 | 1.003 | 100% | 0.80 |  | biased, as expected |
| reverse | clicks | rips | 0.3279 | +0.01375 ± 0.02138 | 0.21321 | 0.63 | 1 | 0.763 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.76; 0% of logs usable) |
| reverse | clicks | snrips | 0.3279 | -0.01046 ± 0.01421 | 0.14173 | 0.61 | 1 | 0.763 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.76; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.3279 | +0.06124 ± 0.00042 | 0.06138 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.3279 | +0.02118 ± 0.00036 | 0.02148 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.3279 | -0.01082 ± 0.00098 | 0.01454 | 0.80 | 2948 | 1.003 | 100% | 0.80 |  | biased, as expected |
| reverse | clicks | cdr | 0.3279 | -0.00845 ± 0.02111 | 0.21021 | 0.95 | 1 | 0.763 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.76; 0% of logs usable) |
| reverse | satisfied | naive | 0.1353 | +0.12432 ± 0.00015 | 0.12433 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.1353 | -0.00427 ± 0.03407 | 0.33902 | 0.13 | 1 | 0.948 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.95; 0% of logs usable) |
| reverse | satisfied | snips | 0.1353 | -0.05198 ± 0.02339 | 0.23847 | 0.09 | 1 | 0.948 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.95; 0% of logs usable) |
| reverse | satisfied | iips | 0.1353 | -0.00280 ± 0.00066 | 0.00713 | 0.93 | 2948 | 1.003 | 100% | 0.93 |  | unbiased here |
| reverse | satisfied | sniips | 0.1353 | -0.00318 ± 0.00063 | 0.00705 | 0.91 | 2948 | 1.003 | 100% | 0.91 |  | biased, as expected |
| reverse | satisfied | rips | 0.1353 | +0.02379 ± 0.01358 | 0.13725 | 0.74 | 1 | 0.763 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.76; 0% of logs usable) |
| reverse | satisfied | snrips | 0.1353 | +0.01363 ± 0.01038 | 0.10422 | 0.69 | 1 | 0.763 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.76; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.1353 | +0.04802 ± 0.00024 | 0.04808 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.1353 | +0.03020 ± 0.00019 | 0.03026 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.1353 | -0.00307 ± 0.00056 | 0.00639 | 0.91 | 2948 | 1.003 | 100% | 0.91 |  | biased, as expected |
| reverse | satisfied | cdr | 0.1353 | +0.00894 ± 0.01295 | 0.12911 | 0.96 | 1 | 0.763 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.76; 0% of logs usable) |
| wide | clicks | naive | 0.4974 | -0.00693 ± 0.00020 | 0.00722 | 0.07 |  |  | 100% | 0.07 |  | biased, as expected |
| wide | clicks | ips | 0.4974 | -0.22569 ± 0.05796 | 0.61929 | 0.19 | 3267 | 0.420 | 0% |  |  | unbiased though unsupported |
| wide | clicks | snips | 0.4974 | -0.09214 ± 0.03528 | 0.36292 | 0.01 | 3267 | 0.420 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.4974 | -0.02673 ± 0.00087 | 0.02809 | 0.17 | 2530 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | sniips | 0.4974 | +0.03543 ± 0.00098 | 0.03675 | 0.05 | 2530 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | rips | 0.4974 | -0.06925 ± 0.00723 | 0.09984 | 0.53 | 5 | 0.281 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snrips | 0.4974 | +0.00522 ± 0.01148 | 0.11434 | 0.86 | 5 | 0.281 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.4974 | +0.03835 ± 0.00025 | 0.03843 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.4974 | +0.04648 ± 0.00023 | 0.04653 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.4974 | +0.03095 ± 0.00076 | 0.03187 | 0.01 | 2530 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | cdr | 0.4974 | +0.04289 ± 0.00814 | 0.09164 | 0.79 | 5 | 0.281 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | naive | 0.2624 | -0.00275 ± 0.00015 | 0.00311 | 0.53 |  |  | 100% | 0.53 |  | biased, as expected |
| wide | satisfied | ips | 0.2624 | -0.13431 ± 0.04122 | 0.43157 | 0.10 | 3267 | 0.420 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snips | 0.2624 | -0.09339 ± 0.02640 | 0.27879 | 0.00 | 3267 | 0.420 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.2624 | -0.01061 ± 0.00059 | 0.01211 | 0.53 | 2530 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | sniips | 0.2624 | +0.01675 ± 0.00066 | 0.01800 | 0.37 | 2530 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | rips | 0.2624 | -0.02600 ± 0.00520 | 0.05790 | 0.62 | 5 | 0.281 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | snrips | 0.2624 | +0.00139 ± 0.00681 | 0.06779 | 0.73 | 5 | 0.281 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | dm-pbm | 0.2624 | +0.02727 ± 0.00017 | 0.02733 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.2624 | +0.03619 ± 0.00018 | 0.03623 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.2624 | +0.01988 ± 0.00049 | 0.02048 | 0.01 | 2530 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | cdr | 0.2624 | +0.03239 ± 0.00461 | 0.05617 | 0.71 | 5 | 0.281 | 0% |  |  | biased, as expected: 19% of its slots unsupported |

## nfcorpus · dbn · {"policy":"shuffle","top":10}

100 logs of 100000 impressions, 12 s. Propensities: 10000000 lists, 0 mismatched; chi-square 11236 on 11160 df (p 0.305), largest |z| 4.19.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.5206 | 0.2840 | 0.2840 | 2.368 |
| rerank | 0.0% | 0.5066 | 0.2698 | 0.2698 | 2.376 |
| oracle | 0.0% | 0.5979 | 0.3429 | 0.3429 | 2.337 |
| reverse | 0.0% | 0.3279 | 0.1353 | 0.1353 | 2.453 |
| wide | 18.6% | 0.4974 | 0.2624 | 0.2624 | 2.380 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.5206 | -0.09999 ± 0.00018 | 0.10000 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.5206 | -0.05189 ± 0.04018 | 0.40311 | 0.81 | 3 | 0.940 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| ranking | clicks | snips | 0.5206 | -0.01733 ± 0.03830 | 0.38145 | 0.64 | 3 | 0.940 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| ranking | clicks | iips | 0.5206 | +0.04085 ± 0.00065 | 0.04135 | 0.00 | 9894 | 1.002 | 98% | 0.00 |  | biased, as expected |
| ranking | clicks | sniips | 0.5206 | +0.04074 ± 0.00062 | 0.04121 | 0.00 | 9894 | 1.002 | 98% | 0.00 |  | biased, as expected |
| ranking | clicks | rips | 0.5206 | -0.00835 ± 0.01003 | 0.10013 | 0.74 | 3 | 0.928 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| ranking | clicks | snrips | 0.5206 | -0.01814 ± 0.00782 | 0.07987 | 0.71 | 3 | 0.928 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| ranking | clicks | dm-pbm | 0.5206 | +0.05932 ± 0.00027 | 0.05938 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.5206 | -0.03020 ± 0.00022 | 0.03028 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.5206 | +0.04074 ± 0.00054 | 0.04110 | 0.00 | 9894 | 1.002 | 98% | 0.00 |  | biased, as expected |
| ranking | clicks | cdr | 0.5206 | -0.01338 ± 0.00923 | 0.09283 | 0.91 | 3 | 0.928 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| ranking | satisfied | naive | 0.2840 | -0.08018 ± 0.00011 | 0.08019 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.2840 | -0.02389 ± 0.02615 | 0.26130 | 0.61 | 3 | 0.940 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| ranking | satisfied | snips | 0.2840 | +0.01344 ± 0.03008 | 0.29964 | 0.52 | 3 | 0.940 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| ranking | satisfied | iips | 0.2840 | +0.02307 ± 0.00054 | 0.02368 | 0.01 | 9894 | 1.002 | 98% | 0.01 |  | biased, as expected |
| ranking | satisfied | sniips | 0.2840 | +0.02302 ± 0.00051 | 0.02357 | 0.00 | 9894 | 1.002 | 98% | 0.00 |  | biased, as expected |
| ranking | satisfied | rips | 0.2840 | -0.01222 ± 0.00435 | 0.04497 | 0.73 | 3 | 0.928 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| ranking | satisfied | snrips | 0.2840 | -0.01456 ± 0.00339 | 0.03678 | 0.68 | 3 | 0.928 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| ranking | satisfied | dm-pbm | 0.2840 | +0.03358 ± 0.00023 | 0.03366 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.2840 | -0.01665 ± 0.00017 | 0.01674 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.2840 | +0.02299 ± 0.00042 | 0.02336 | 0.00 | 9894 | 1.002 | 98% | 0.00 |  | biased, as expected |
| ranking | satisfied | cdr | 0.2840 | -0.01458 ± 0.00473 | 0.04925 | 0.89 | 3 | 0.928 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| rerank | clicks | naive | 0.5066 | -0.08597 ± 0.00018 | 0.08599 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.5066 | -0.02880 ± 0.04450 | 0.44368 | 0.76 | 3 | 0.934 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| rerank | clicks | snips | 0.5066 | -0.03153 ± 0.03949 | 0.39416 | 0.60 | 3 | 0.934 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| rerank | clicks | iips | 0.5066 | +0.02765 ± 0.00075 | 0.02864 | 0.04 | 9889 | 1.001 | 97% | 0.04 |  | biased, as expected |
| rerank | clicks | sniips | 0.5066 | +0.02802 ± 0.00071 | 0.02888 | 0.01 | 9889 | 1.001 | 97% | 0.01 |  | biased, as expected |
| rerank | clicks | rips | 0.5066 | -0.01056 ± 0.01198 | 0.11967 | 0.72 | 3 | 0.931 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| rerank | clicks | snrips | 0.5066 | -0.00356 ± 0.01684 | 0.16758 | 0.68 | 3 | 0.931 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.5066 | +0.05320 ± 0.00031 | 0.05330 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.5066 | -0.02533 ± 0.00024 | 0.02544 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.5066 | +0.02787 ± 0.00065 | 0.02860 | 0.00 | 9889 | 1.001 | 97% | 0.00 |  | biased, as expected |
| rerank | clicks | cdr | 0.5066 | -0.00719 ± 0.01124 | 0.11205 | 0.89 | 3 | 0.931 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| rerank | satisfied | naive | 0.2698 | -0.06604 ± 0.00011 | 0.06605 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.2698 | -0.03092 ± 0.02893 | 0.28948 | 0.51 | 3 | 0.934 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| rerank | satisfied | snips | 0.2698 | -0.04424 ± 0.02773 | 0.27942 | 0.45 | 3 | 0.934 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| rerank | satisfied | iips | 0.2698 | +0.01383 ± 0.00052 | 0.01475 | 0.18 | 9889 | 1.001 | 97% | 0.16 |  | biased, as expected |
| rerank | satisfied | sniips | 0.2698 | +0.01406 ± 0.00048 | 0.01486 | 0.18 | 9889 | 1.001 | 97% | 0.18 |  | biased, as expected |
| rerank | satisfied | rips | 0.2698 | -0.00406 ± 0.00700 | 0.06975 | 0.68 | 3 | 0.931 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| rerank | satisfied | snrips | 0.2698 | -0.00899 ± 0.00546 | 0.05505 | 0.67 | 3 | 0.931 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.2698 | +0.03052 ± 0.00022 | 0.03059 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.2698 | -0.01297 ± 0.00016 | 0.01307 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.2698 | +0.01386 ± 0.00043 | 0.01451 | 0.11 | 9889 | 1.001 | 97% | 0.11 |  | biased, as expected |
| rerank | satisfied | cdr | 0.2698 | -0.00139 ± 0.00637 | 0.06337 | 0.90 | 3 | 0.931 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| oracle | clicks | naive | 0.5979 | -0.17732 ± 0.00018 | 0.17733 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.5979 | -0.09293 ± 0.04342 | 0.44191 | 0.80 | 3 | 0.928 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| oracle | clicks | snips | 0.5979 | -0.07011 ± 0.03887 | 0.39302 | 0.64 | 3 | 0.928 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| oracle | clicks | iips | 0.5979 | +0.11789 ± 0.00080 | 0.11816 | 0.00 | 9884 | 1.000 | 98% | 0.00 |  | biased, as expected |
| oracle | clicks | sniips | 0.5979 | +0.11760 ± 0.00075 | 0.11783 | 0.00 | 9884 | 1.000 | 98% | 0.00 |  | biased, as expected |
| oracle | clicks | rips | 0.5979 | -0.01839 ± 0.00763 | 0.07810 | 0.69 | 3 | 0.939 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| oracle | clicks | snrips | 0.5979 | -0.01813 ± 0.00820 | 0.08356 | 0.64 | 3 | 0.939 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.5979 | +0.14531 ± 0.00035 | 0.14535 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.5979 | -0.06502 ± 0.00022 | 0.06505 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.5979 | +0.11752 ± 0.00069 | 0.11772 | 0.00 | 9884 | 1.000 | 98% | 0.00 |  | biased, as expected |
| oracle | clicks | cdr | 0.5979 | -0.02030 ± 0.00822 | 0.08426 | 0.82 | 3 | 0.939 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| oracle | satisfied | naive | 0.3429 | -0.13909 ± 0.00011 | 0.13909 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.3429 | -0.03441 ± 0.03216 | 0.32182 | 0.62 | 3 | 0.928 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| oracle | satisfied | snips | 0.3429 | -0.01536 ± 0.03124 | 0.31117 | 0.54 | 3 | 0.928 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| oracle | satisfied | iips | 0.3429 | +0.07484 ± 0.00064 | 0.07512 | 0.00 | 9884 | 1.000 | 98% | 0.00 |  | biased, as expected |
| oracle | satisfied | sniips | 0.3429 | +0.07469 ± 0.00058 | 0.07492 | 0.00 | 9884 | 1.000 | 98% | 0.00 |  | biased, as expected |
| oracle | satisfied | rips | 0.3429 | -0.01114 ± 0.00248 | 0.02709 | 0.74 | 3 | 0.939 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| oracle | satisfied | snrips | 0.3429 | -0.01098 ± 0.00250 | 0.02723 | 0.75 | 3 | 0.939 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.3429 | +0.08993 ± 0.00029 | 0.08997 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.3429 | -0.04202 ± 0.00018 | 0.04206 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.3429 | +0.07450 ± 0.00049 | 0.07466 | 0.00 | 9884 | 1.000 | 98% | 0.00 |  | biased, as expected |
| oracle | satisfied | cdr | 0.3429 | -0.01285 ± 0.00264 | 0.02921 | 0.86 | 3 | 0.939 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| reverse | clicks | naive | 0.3279 | +0.09270 ± 0.00018 | 0.09272 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.3279 | -0.01644 ± 0.03424 | 0.34106 | 0.58 | 4 | 1.074 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.07; 0% of logs usable) |
| reverse | clicks | snips | 0.3279 | -0.06575 ± 0.02960 | 0.30172 | 0.56 | 4 | 1.074 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.07; 0% of logs usable) |
| reverse | clicks | iips | 0.3279 | -0.01044 ± 0.00053 | 0.01169 | 0.57 | 9887 | 0.998 | 99% | 0.57 |  | biased, as expected |
| reverse | clicks | sniips | 0.3279 | -0.01046 ± 0.00051 | 0.01164 | 0.52 | 9887 | 0.998 | 99% | 0.52 |  | biased, as expected |
| reverse | clicks | rips | 0.3279 | +0.00132 ± 0.01270 | 0.12633 | 0.70 | 4 | 0.942 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.94; 0% of logs usable) |
| reverse | clicks | snrips | 0.3279 | -0.00579 ± 0.01162 | 0.11576 | 0.68 | 4 | 0.942 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.94; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.3279 | +0.01559 ± 0.00023 | 0.01576 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.3279 | +0.01205 ± 0.00021 | 0.01223 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.3279 | -0.01058 ± 0.00047 | 0.01156 | 0.47 | 9887 | 0.998 | 99% | 0.46 |  | biased, as expected |
| reverse | clicks | cdr | 0.3279 | -0.01414 ± 0.01339 | 0.13400 | 0.90 | 4 | 0.942 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.94; 0% of logs usable) |
| reverse | satisfied | naive | 0.1353 | +0.06846 ± 0.00011 | 0.06847 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.1353 | -0.01435 ± 0.01772 | 0.17691 | 0.35 | 4 | 1.074 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.07; 0% of logs usable) |
| reverse | satisfied | snips | 0.1353 | -0.03453 ± 0.01603 | 0.16320 | 0.35 | 4 | 1.074 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.07; 0% of logs usable) |
| reverse | satisfied | iips | 0.1353 | -0.00306 ± 0.00037 | 0.00478 | 0.85 | 9887 | 0.998 | 99% | 0.85 |  | biased, as expected |
| reverse | satisfied | sniips | 0.1353 | -0.00307 ± 0.00037 | 0.00477 | 0.85 | 9887 | 0.998 | 99% | 0.85 |  | biased, as expected |
| reverse | satisfied | rips | 0.1353 | -0.00263 ± 0.00766 | 0.07626 | 0.57 | 4 | 0.942 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.94; 0% of logs usable) |
| reverse | satisfied | snrips | 0.1353 | -0.00546 ± 0.00715 | 0.07131 | 0.59 | 4 | 0.942 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.94; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.1353 | +0.01541 ± 0.00015 | 0.01548 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.1353 | +0.01632 ± 0.00014 | 0.01638 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.1353 | -0.00321 ± 0.00033 | 0.00462 | 0.83 | 9887 | 0.998 | 99% | 0.83 |  | biased, as expected |
| reverse | satisfied | cdr | 0.1353 | -0.00994 ± 0.00812 | 0.08141 | 0.93 | 4 | 0.942 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.94; 0% of logs usable) |
| wide | clicks | naive | 0.4974 | -0.07679 ± 0.00018 | 0.07681 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.4974 | -0.24944 ± 0.03395 | 0.41989 | 0.48 | 1 | 0.451 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snips | 0.4974 | -0.15474 ± 0.04273 | 0.45245 | 0.33 | 1 | 0.451 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.4974 | -0.01151 ± 0.00071 | 0.01351 | 0.59 | 6099 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | sniips | 0.4974 | +0.05383 ± 0.00078 | 0.05437 | 0.00 | 6099 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | rips | 0.4974 | -0.05455 ± 0.00896 | 0.10449 | 0.50 | 1 | 0.556 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snrips | 0.4974 | +0.01536 ± 0.01675 | 0.16741 | 0.71 | 1 | 0.556 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.4974 | +0.07295 ± 0.00033 | 0.07302 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.4974 | +0.00935 ± 0.00026 | 0.00971 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.4974 | +0.04618 ± 0.00061 | 0.04658 | 0.00 | 6099 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | cdr | 0.4974 | +0.03875 ± 0.00807 | 0.08913 | 0.88 | 1 | 0.556 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | naive | 0.2624 | -0.05861 ± 0.00011 | 0.05862 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.2624 | -0.14142 ± 0.02106 | 0.25277 | 0.29 | 1 | 0.451 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | snips | 0.2624 | -0.09388 ± 0.03028 | 0.31555 | 0.22 | 1 | 0.451 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.2624 | -0.00143 ± 0.00049 | 0.00510 | 0.95 | 6099 | 0.610 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | sniips | 0.2624 | +0.02774 ± 0.00053 | 0.02823 | 0.00 | 6099 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | rips | 0.2624 | -0.01849 ± 0.00544 | 0.05716 | 0.52 | 1 | 0.556 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.2624 | +0.01047 ± 0.00743 | 0.07469 | 0.81 | 1 | 0.556 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | dm-pbm | 0.2624 | +0.04272 ± 0.00022 | 0.04278 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.2624 | +0.00828 ± 0.00018 | 0.00847 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.2624 | +0.02642 ± 0.00042 | 0.02676 | 0.00 | 6099 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | cdr | 0.2624 | +0.02716 ± 0.00529 | 0.05924 | 0.87 | 1 | 0.556 | 0% |  |  | biased, as expected: 19% of its slots unsupported |

## nfcorpus · dbn · {"policy":"swap","top":10}

100 logs of 100000 impressions, 11 s. Propensities: 10000000 lists, 0 mismatched; chi-square 3211 on 3224 df (p 0.562), largest |z| 3.24.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.5206 | 0.2840 | 0.2840 | 2.368 |
| rerank | 50.4% | 0.5066 | 0.2698 | 0.2698 | 2.376 |
| oracle | 24.9% | 0.5979 | 0.3429 | 0.3429 | 2.337 |
| reverse | 80.0% | 0.3279 | 0.1353 | 0.1353 | 2.453 |
| wide | 54.7% | 0.4974 | 0.2624 | 0.2624 | 2.380 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.5206 | -0.04348 ± 0.00019 | 0.04352 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.5206 | +0.00035 ± 0.00090 | 0.00900 | 0.93 | 9999 | 1.000 | 100% | 0.93 | yes | ok |
| ranking | clicks | snips | 0.5206 | +0.00040 ± 0.00062 | 0.00622 | 0.97 | 9999 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | clicks | iips | 0.5206 | +0.02785 ± 0.00052 | 0.02832 | 0.00 | 9999 | 1.000 | 98% | 0.00 |  | biased, as expected |
| ranking | clicks | sniips | 0.5206 | +0.02785 ± 0.00040 | 0.02813 | 0.00 | 9999 | 1.000 | 98% | 0.00 |  | biased, as expected |
| ranking | clicks | rips | 0.5206 | +0.00035 ± 0.00090 | 0.00900 | 0.93 | 9999 | 1.000 | 100% | 0.93 | yes | ok |
| ranking | clicks | snrips | 0.5206 | +0.00040 ± 0.00062 | 0.00622 | 0.97 | 9999 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | clicks | dm-pbm | 0.5206 | +0.03299 ± 0.00024 | 0.03308 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.5206 | -0.02420 ± 0.00020 | 0.02428 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.5206 | +0.02756 ± 0.00036 | 0.02779 | 0.00 | 9999 | 1.000 | 98% | 0.00 |  | biased, as expected |
| ranking | clicks | cdr | 0.5206 | +0.00064 ± 0.00059 | 0.00586 | 0.93 | 9999 | 1.000 | 100% | 0.93 | yes | ok |
| ranking | satisfied | naive | 0.2840 | -0.03898 ± 0.00013 | 0.03900 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.2840 | +0.00027 ± 0.00058 | 0.00577 | 0.93 | 9999 | 1.000 | 100% | 0.93 | yes | ok |
| ranking | satisfied | snips | 0.2840 | +0.00029 ± 0.00044 | 0.00439 | 0.95 | 9999 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | satisfied | iips | 0.2840 | +0.01640 ± 0.00038 | 0.01682 | 0.01 | 9999 | 1.000 | 98% | 0.01 |  | biased, as expected |
| ranking | satisfied | sniips | 0.2840 | +0.01640 ± 0.00033 | 0.01672 | 0.01 | 9999 | 1.000 | 98% | 0.01 |  | biased, as expected |
| ranking | satisfied | rips | 0.2840 | +0.00027 ± 0.00058 | 0.00577 | 0.93 | 9999 | 1.000 | 100% | 0.93 | yes | ok |
| ranking | satisfied | snrips | 0.2840 | +0.00029 ± 0.00044 | 0.00439 | 0.95 | 9999 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | satisfied | dm-pbm | 0.2840 | +0.01710 ± 0.00018 | 0.01719 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.2840 | -0.01167 ± 0.00015 | 0.01176 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.2840 | +0.01612 ± 0.00028 | 0.01637 | 0.00 | 9999 | 1.000 | 98% | 0.00 |  | biased, as expected |
| ranking | satisfied | cdr | 0.2840 | +0.00042 ± 0.00034 | 0.00345 | 0.96 | 9999 | 1.000 | 100% | 0.96 | yes | ok |
| rerank | clicks | naive | 0.5066 | -0.02946 ± 0.00019 | 0.02952 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.5066 | -0.47489 ± 0.00020 | 0.47490 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | snips | 0.5066 | -0.17652 ± 0.00173 | 0.17736 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | iips | 0.5066 | -0.15023 ± 0.00054 | 0.15033 | 0.00 | 9992 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | sniips | 0.5066 | +0.01715 ± 0.00065 | 0.01834 | 0.23 | 9992 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | rips | 0.5066 | -0.19233 ± 0.00061 | 0.19242 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | snrips | 0.5066 | -0.04289 ± 0.00092 | 0.04386 | 0.02 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | dm-pbm | 0.5066 | +0.02586 ± 0.00030 | 0.02603 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.5066 | -0.00571 ± 0.00028 | 0.00638 | 0.12 |  |  | 100% | 0.12 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.5066 | +0.01122 ± 0.00041 | 0.01195 | 0.18 | 9992 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | cdr | 0.5066 | +0.03636 ± 0.00044 | 0.03662 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | naive | 0.2698 | -0.02484 ± 0.00013 | 0.02487 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.2698 | -0.25725 ± 0.00011 | 0.25725 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | snips | 0.2698 | -0.13900 ± 0.00099 | 0.13934 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | iips | 0.2698 | -0.06584 ± 0.00043 | 0.06598 | 0.00 | 9992 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | sniips | 0.2698 | +0.00945 ± 0.00052 | 0.01076 | 0.47 | 9992 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | rips | 0.2698 | -0.08866 ± 0.00043 | 0.08876 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | snrips | 0.2698 | -0.03355 ± 0.00059 | 0.03406 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | dm-pbm | 0.2698 | +0.01412 ± 0.00020 | 0.01426 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.2698 | -0.00159 ± 0.00018 | 0.00238 | 0.47 |  |  | 100% | 0.47 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.2698 | +0.00946 ± 0.00034 | 0.01004 | 0.15 | 9992 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | cdr | 0.2698 | +0.02436 ± 0.00031 | 0.02455 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| oracle | clicks | naive | 0.5979 | -0.12081 ± 0.00019 | 0.12082 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.5979 | -0.37767 ± 0.00054 | 0.37771 | 0.00 | 5575 | 0.558 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | snips | 0.5979 | -0.20285 ± 0.00074 | 0.20299 | 0.00 | 5575 | 0.558 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | iips | 0.5979 | -0.07616 ± 0.00060 | 0.07640 | 0.00 | 10011 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | sniips | 0.5979 | -0.01163 ± 0.00051 | 0.01268 | 0.38 | 10011 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | rips | 0.5979 | -0.11057 ± 0.00075 | 0.11082 | 0.00 | 5575 | 0.558 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | snrips | 0.5979 | -0.03565 ± 0.00073 | 0.03639 | 0.00 | 5575 | 0.558 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | dm-pbm | 0.5979 | +0.11860 ± 0.00032 | 0.11864 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.5979 | -0.05159 ± 0.00023 | 0.05164 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.5979 | +0.09920 ± 0.00043 | 0.09929 | 0.00 | 10011 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | cdr | 0.5979 | +0.00860 ± 0.00050 | 0.00994 | 0.53 | 5575 | 0.558 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | naive | 0.3429 | -0.09789 ± 0.00013 | 0.09789 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.3429 | -0.23486 ± 0.00035 | 0.23489 | 0.00 | 5575 | 0.558 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | snips | 0.3429 | -0.14916 ± 0.00053 | 0.14925 | 0.00 | 5575 | 0.558 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | iips | 0.3429 | -0.03272 ± 0.00046 | 0.03304 | 0.00 | 10011 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | sniips | 0.3429 | -0.00863 ± 0.00044 | 0.00969 | 0.50 | 10011 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | rips | 0.3429 | -0.05238 ± 0.00050 | 0.05262 | 0.00 | 5575 | 0.558 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | snrips | 0.3429 | -0.02724 ± 0.00049 | 0.02767 | 0.00 | 5575 | 0.558 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | dm-pbm | 0.3429 | +0.07219 ± 0.00025 | 0.07224 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.3429 | -0.03253 ± 0.00017 | 0.03258 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.3429 | +0.06812 ± 0.00037 | 0.06822 | 0.00 | 10011 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | cdr | 0.3429 | +0.00884 ± 0.00035 | 0.00950 | 0.34 | 5575 | 0.558 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| reverse | clicks | naive | 0.3279 | +0.14922 ± 0.00019 | 0.14923 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.3279 | -0.32792 ± 0.00000 | 0.32792 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snips | 0.3279 | -0.32792 ± 0.00000 | 0.32792 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | iips | 0.3279 | -0.22213 ± 0.00034 | 0.22216 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | sniips | 0.3279 | -0.22227 ± 0.00032 | 0.22229 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | rips | 0.3279 | -0.22213 ± 0.00034 | 0.22216 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snrips | 0.3279 | -0.22227 ± 0.00032 | 0.22229 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | dm-pbm | 0.3279 | +0.04369 ± 0.00037 | 0.04385 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.3279 | +0.07872 ± 0.00041 | 0.07882 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.3279 | +0.01578 ± 0.00039 | 0.01626 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | cdr | 0.3279 | +0.07739 ± 0.00042 | 0.07750 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | naive | 0.1353 | +0.10966 ± 0.00013 | 0.10967 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.1353 | -0.13531 ± 0.00000 | 0.13531 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snips | 0.1353 | -0.13531 ± 0.00000 | 0.13531 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | iips | 0.1353 | -0.08720 ± 0.00023 | 0.08723 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | sniips | 0.1353 | -0.08726 ± 0.00023 | 0.08729 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | rips | 0.1353 | -0.08720 ± 0.00023 | 0.08723 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snrips | 0.1353 | -0.08726 ± 0.00023 | 0.08729 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | dm-pbm | 0.1353 | +0.04697 ± 0.00021 | 0.04701 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.1353 | +0.05471 ± 0.00021 | 0.05476 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.1353 | +0.02923 ± 0.00022 | 0.02931 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | cdr | 0.1353 | +0.04893 ± 0.00022 | 0.04898 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| wide | clicks | naive | 0.4974 | -0.02028 ± 0.00019 | 0.02037 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.4974 | -0.47772 ± 0.00018 | 0.47772 | 0.00 | 673 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | snips | 0.4974 | -0.20478 ± 0.00223 | 0.20599 | 0.00 | 673 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | iips | 0.4974 | -0.15516 ± 0.00052 | 0.15525 | 0.00 | 9882 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | sniips | 0.4974 | +0.02007 ± 0.00066 | 0.02113 | 0.13 | 9882 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | rips | 0.4974 | -0.19056 ± 0.00059 | 0.19065 | 0.00 | 673 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | snrips | 0.4974 | -0.02685 ± 0.00103 | 0.02874 | 0.30 | 673 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | dm-pbm | 0.4974 | +0.05044 ± 0.00029 | 0.05052 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.4974 | +0.03523 ± 0.00026 | 0.03533 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.4974 | +0.03458 ± 0.00041 | 0.03481 | 0.00 | 9882 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | cdr | 0.4974 | +0.07723 ± 0.00043 | 0.07735 | 0.00 | 673 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | naive | 0.2624 | -0.01741 ± 0.00013 | 0.01746 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.2624 | -0.25528 ± 0.00009 | 0.25528 | 0.00 | 673 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | snips | 0.2624 | -0.15681 ± 0.00120 | 0.15726 | 0.00 | 673 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | iips | 0.2624 | -0.06424 ± 0.00040 | 0.06436 | 0.00 | 9882 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | sniips | 0.2624 | +0.01632 ± 0.00049 | 0.01703 | 0.05 | 9882 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | rips | 0.2624 | -0.08399 ± 0.00042 | 0.08409 | 0.00 | 673 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | snrips | 0.2624 | -0.02252 ± 0.00064 | 0.02341 | 0.07 | 673 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.2624 | +0.03017 ± 0.00020 | 0.03024 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.2624 | +0.02422 ± 0.00018 | 0.02428 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.2624 | +0.02495 ± 0.00032 | 0.02515 | 0.00 | 9882 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | cdr | 0.2624 | +0.05046 ± 0.00031 | 0.05056 | 0.00 | 673 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |

## nfcorpus · ubm · {"policy":"softmax","tau":1.0,"top":10,"by":"rank"}

100 logs of 100000 impressions, 13 s. Propensities: 10000000 lists, 0 mismatched; chi-square 11255 on 11160 df (p 0.262), largest |z| 4.00.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.4260 | 0.2494 | 0.2086 | 2.409 |
| rerank | 0.0% | 0.4095 | 0.2343 | 0.2033 | 2.413 |
| oracle | 0.0% | 0.6106 | 0.3873 | 0.2979 | 2.360 |
| reverse | 0.0% | 0.2291 | 0.1066 | 0.0846 | 2.483 |
| wide | 18.6% | 0.3993 | 0.2267 | 0.2004 | 2.414 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.4260 | -0.07055 ± 0.00019 | 0.07057 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.4260 | -0.00671 ± 0.00756 | 0.07556 | 0.93 | 110 | 0.992 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snips | 0.4260 | -0.00393 ± 0.00598 | 0.05965 | 0.96 | 110 | 0.992 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | iips | 0.4260 | -0.00528 ± 0.00047 | 0.00702 | 0.76 | 11574 | 1.001 | 100% | 0.76 |  | biased, as expected |
| ranking | clicks | sniips | 0.4260 | -0.00532 ± 0.00044 | 0.00687 | 0.76 | 11574 | 1.001 | 100% | 0.76 |  | biased, as expected |
| ranking | clicks | rips | 0.4260 | -0.00085 ± 0.00178 | 0.01768 | 0.92 | 110 | 0.992 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snrips | 0.4260 | -0.00102 ± 0.00174 | 0.01739 | 0.90 | 110 | 0.992 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | dm-pbm | 0.4260 | -0.00440 ± 0.00022 | 0.00492 | 0.15 |  |  | 100% | 0.15 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.4260 | -0.04078 ± 0.00022 | 0.04084 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.4260 | -0.00541 ± 0.00040 | 0.00671 | 0.74 | 11574 | 1.001 | 100% | 0.74 |  | biased, as expected |
| ranking | clicks | cdr | 0.4260 | -0.00042 ± 0.00167 | 0.01664 | 0.91 | 110 | 0.992 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | naive | 0.2494 | -0.05299 ± 0.00014 | 0.05301 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.2494 | -0.00292 ± 0.00533 | 0.05310 | 0.93 | 110 | 0.992 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snips | 0.2494 | -0.00102 ± 0.00478 | 0.04759 | 0.96 | 110 | 0.992 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | iips | 0.2494 | -0.00344 ± 0.00035 | 0.00486 | 0.81 | 11574 | 1.001 | 100% | 0.81 |  | biased, as expected |
| ranking | satisfied | sniips | 0.2494 | -0.00346 ± 0.00033 | 0.00476 | 0.79 | 11574 | 1.001 | 100% | 0.79 |  | biased, as expected |
| ranking | satisfied | rips | 0.2494 | -0.00062 ± 0.00111 | 0.01111 | 0.92 | 110 | 0.992 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snrips | 0.2494 | -0.00076 ± 0.00109 | 0.01087 | 0.92 | 110 | 0.992 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | dm-pbm | 0.2494 | -0.00234 ± 0.00018 | 0.00293 | 0.40 |  |  | 100% | 0.40 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.2494 | -0.02792 ± 0.00017 | 0.02797 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.2494 | -0.00354 ± 0.00031 | 0.00466 | 0.77 | 11574 | 1.001 | 100% | 0.77 |  | biased, as expected |
| ranking | satisfied | cdr | 0.2494 | -0.00053 ± 0.00109 | 0.01085 | 0.94 | 110 | 0.992 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| rerank | clicks | naive | 0.4095 | -0.05403 ± 0.00019 | 0.05406 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.4095 | -0.01694 ± 0.01611 | 0.16114 | 0.85 | 18 | 1.017 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 1.02; 3% of logs usable) |
| rerank | clicks | snips | 0.4095 | +0.00423 ± 0.01608 | 0.16001 | 0.84 | 18 | 1.017 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 1.02; 3% of logs usable) |
| rerank | clicks | iips | 0.4095 | -0.00317 ± 0.00053 | 0.00619 | 0.86 | 9653 | 1.002 | 98% | 0.86 |  | biased, as expected |
| rerank | clicks | sniips | 0.4095 | -0.00349 ± 0.00053 | 0.00629 | 0.84 | 9653 | 1.002 | 98% | 0.84 |  | biased, as expected |
| rerank | clicks | rips | 0.4095 | -0.00557 ± 0.00376 | 0.03786 | 0.81 | 17 | 0.915 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 0.92; 3% of logs usable) |
| rerank | clicks | snrips | 0.4095 | -0.00312 ± 0.00380 | 0.03793 | 0.83 | 17 | 0.915 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 0.92; 3% of logs usable) |
| rerank | clicks | dm-pbm | 0.4095 | +0.00626 ± 0.00023 | 0.00668 | 0.05 |  |  | 100% | 0.05 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.4095 | -0.03843 ± 0.00021 | 0.03849 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.4095 | -0.00315 ± 0.00049 | 0.00581 | 0.86 | 9653 | 1.002 | 98% | 0.86 |  | biased, as expected |
| rerank | clicks | cdr | 0.4095 | -0.00512 ± 0.00483 | 0.04832 | 0.91 | 17 | 0.915 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 0.92; 3% of logs usable) |
| rerank | satisfied | naive | 0.2343 | -0.03793 ± 0.00014 | 0.03796 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.2343 | -0.02139 ± 0.00948 | 0.09674 | 0.85 | 18 | 1.017 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 1.02; 3% of logs usable) |
| rerank | satisfied | snips | 0.2343 | -0.00586 ± 0.01029 | 0.10256 | 0.86 | 18 | 1.017 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 1.02; 3% of logs usable) |
| rerank | satisfied | iips | 0.2343 | -0.00207 ± 0.00038 | 0.00433 | 0.93 | 9653 | 1.002 | 98% | 0.93 |  | biased, as expected |
| rerank | satisfied | sniips | 0.2343 | -0.00226 ± 0.00038 | 0.00442 | 0.90 | 9653 | 1.002 | 98% | 0.90 |  | biased, as expected |
| rerank | satisfied | rips | 0.2343 | -0.00529 ± 0.00232 | 0.02370 | 0.75 | 17 | 0.915 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 0.92; 3% of logs usable) |
| rerank | satisfied | snrips | 0.2343 | -0.00423 ± 0.00242 | 0.02443 | 0.77 | 17 | 0.915 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 0.92; 3% of logs usable) |
| rerank | satisfied | dm-pbm | 0.2343 | +0.00541 ± 0.00018 | 0.00568 | 0.02 |  |  | 100% | 0.02 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.2343 | -0.02506 ± 0.00015 | 0.02510 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.2343 | -0.00205 ± 0.00033 | 0.00389 | 0.91 | 9653 | 1.002 | 98% | 0.91 |  | biased, as expected |
| rerank | satisfied | cdr | 0.2343 | -0.00428 ± 0.00295 | 0.02970 | 0.87 | 17 | 0.915 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 0.92; 3% of logs usable) |
| oracle | clicks | naive | 0.6106 | -0.25519 ± 0.00019 | 0.25520 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.6106 | +0.00420 ± 0.05007 | 0.49820 | 0.68 | 52 | 0.997 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snips | 0.6106 | -0.03145 ± 0.02478 | 0.24853 | 0.80 | 52 | 0.997 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | iips | 0.6106 | -0.02688 ± 0.00057 | 0.02747 | 0.01 | 11237 | 1.000 | 99% | 0.01 |  | biased, as expected |
| oracle | clicks | sniips | 0.6106 | -0.02695 ± 0.00052 | 0.02745 | 0.00 | 11237 | 1.000 | 99% | 0.00 |  | biased, as expected |
| oracle | clicks | rips | 0.6106 | +0.00590 ± 0.00804 | 0.08023 | 0.82 | 49 | 0.981 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snrips | 0.6106 | +0.00096 ± 0.00572 | 0.05687 | 0.78 | 49 | 0.981 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | dm-pbm | 0.6106 | -0.04330 ± 0.00029 | 0.04339 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.6106 | -0.18418 ± 0.00024 | 0.18420 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.6106 | -0.02714 ± 0.00048 | 0.02756 | 0.00 | 11237 | 1.000 | 99% | 0.00 |  | biased, as expected |
| oracle | clicks | cdr | 0.6106 | +0.00354 ± 0.00737 | 0.07346 | 0.87 | 49 | 0.981 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | naive | 0.3873 | -0.19091 ± 0.00014 | 0.19092 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.3873 | +0.00580 ± 0.04204 | 0.41837 | 0.71 | 52 | 0.997 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snips | 0.3873 | -0.02359 ± 0.02155 | 0.21573 | 0.78 | 52 | 0.997 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | iips | 0.3873 | -0.01859 ± 0.00049 | 0.01921 | 0.04 | 11237 | 1.000 | 99% | 0.04 |  | biased, as expected |
| oracle | satisfied | sniips | 0.3873 | -0.01865 ± 0.00046 | 0.01920 | 0.01 | 11237 | 1.000 | 99% | 0.01 |  | biased, as expected |
| oracle | satisfied | rips | 0.3873 | -0.00294 ± 0.00336 | 0.03359 | 0.81 | 49 | 0.981 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snrips | 0.3873 | -0.00355 ± 0.00298 | 0.02986 | 0.79 | 49 | 0.981 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | dm-pbm | 0.3873 | -0.03152 ± 0.00024 | 0.03161 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.3873 | -0.13391 ± 0.00019 | 0.13392 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.3873 | -0.01874 ± 0.00040 | 0.01915 | 0.00 | 11237 | 1.000 | 99% | 0.00 |  | biased, as expected |
| oracle | satisfied | cdr | 0.3873 | -0.00498 ± 0.00351 | 0.03530 | 0.86 | 49 | 0.981 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| reverse | clicks | naive | 0.2291 | +0.12637 ± 0.00019 | 0.12639 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.2291 | -0.22907 ± 0.00000 | 0.22907 | 0.00 | 0 | 0.865 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.87; 0% of logs usable) |
| reverse | clicks | snips | 0.2291 | -0.22907 ± 0.00000 | 0.22907 | 0.00 | 0 | 0.865 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.87; 0% of logs usable) |
| reverse | clicks | iips | 0.2291 | +0.00038 ± 0.00074 | 0.00738 | 0.92 | 3419 | 0.999 | 99% | 0.93 |  | unbiased here |
| reverse | clicks | sniips | 0.2291 | +0.00020 ± 0.00070 | 0.00702 | 0.93 | 3419 | 0.999 | 99% | 0.93 |  | unbiased here |
| reverse | clicks | rips | 0.2291 | -0.02834 ± 0.00890 | 0.09294 | 0.33 | 0 | 0.035 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.04; 0% of logs usable) |
| reverse | clicks | snrips | 0.2291 | -0.03906 ± 0.00722 | 0.08177 | 0.33 | 0 | 0.035 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.04; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.2291 | +0.05507 ± 0.00031 | 0.05516 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.2291 | +0.00593 ± 0.00025 | 0.00642 | 0.03 |  |  | 100% | 0.03 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.2291 | +0.00040 ± 0.00064 | 0.00639 | 0.93 | 3419 | 0.999 | 99% | 0.93 |  | unbiased here |
| reverse | clicks | cdr | 0.2291 | -0.00941 ± 0.02286 | 0.22761 | 0.87 | 0 | 0.035 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.04; 0% of logs usable) |
| reverse | satisfied | naive | 0.1066 | +0.08985 ± 0.00014 | 0.08986 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.1066 | -0.10657 ± 0.00000 | 0.10657 | 0.00 | 0 | 0.865 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.87; 0% of logs usable) |
| reverse | satisfied | snips | 0.1066 | -0.10657 ± 0.00000 | 0.10657 | 0.00 | 0 | 0.865 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.87; 0% of logs usable) |
| reverse | satisfied | iips | 0.1066 | -0.00126 ± 0.00052 | 0.00537 | 0.92 | 3419 | 0.999 | 99% | 0.93 |  | unbiased here |
| reverse | satisfied | sniips | 0.1066 | -0.00135 ± 0.00051 | 0.00529 | 0.92 | 3419 | 0.999 | 99% | 0.92 |  | unbiased here |
| reverse | satisfied | rips | 0.1066 | -0.01011 ± 0.00668 | 0.06721 | 0.39 | 0 | 0.035 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.04; 0% of logs usable) |
| reverse | satisfied | snrips | 0.1066 | -0.01604 ± 0.00551 | 0.05714 | 0.35 | 0 | 0.035 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.04; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.1066 | +0.03662 ± 0.00020 | 0.03668 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.1066 | +0.01245 ± 0.00017 | 0.01256 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.1066 | -0.00117 ± 0.00045 | 0.00465 | 0.91 | 3419 | 0.999 | 99% | 0.91 |  | unbiased here |
| reverse | satisfied | cdr | 0.1066 | +0.00049 ± 0.01208 | 0.12016 | 0.82 | 0 | 0.035 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.04; 0% of logs usable) |
| wide | clicks | naive | 0.3993 | -0.04390 ± 0.00019 | 0.04394 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.3993 | -0.17064 ± 0.01206 | 0.20859 | 0.42 | 15 | 0.481 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snips | 0.3993 | +0.07943 ± 0.01756 | 0.19195 | 0.85 | 15 | 0.481 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | iips | 0.3993 | -0.01713 ± 0.00048 | 0.01779 | 0.06 | 6410 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | sniips | 0.3993 | +0.02041 ± 0.00057 | 0.02118 | 0.03 | 6410 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | rips | 0.3993 | -0.02813 ± 0.00238 | 0.03676 | 0.54 | 15 | 0.654 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snrips | 0.3993 | +0.02145 ± 0.00420 | 0.04699 | 0.95 | 15 | 0.654 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | dm-pbm | 0.3993 | +0.02706 ± 0.00024 | 0.02717 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.3993 | +0.00142 ± 0.00022 | 0.00265 | 0.41 |  |  | 100% | 0.41 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.3993 | +0.01990 ± 0.00045 | 0.02039 | 0.01 | 6410 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | cdr | 0.3993 | +0.05439 ± 0.00278 | 0.06102 | 0.25 | 15 | 0.654 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | naive | 0.2267 | -0.03026 ± 0.00014 | 0.03030 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.2267 | -0.09572 ± 0.00662 | 0.11619 | 0.55 | 15 | 0.481 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | snips | 0.2267 | +0.05362 ± 0.01263 | 0.13659 | 0.91 | 15 | 0.481 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | iips | 0.2267 | -0.00666 ± 0.00035 | 0.00750 | 0.54 | 6410 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | sniips | 0.2267 | +0.01212 ± 0.00039 | 0.01274 | 0.14 | 6410 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | rips | 0.2267 | -0.01429 ± 0.00148 | 0.02055 | 0.65 | 15 | 0.654 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | snrips | 0.2267 | +0.00896 ± 0.00248 | 0.02626 | 0.87 | 15 | 0.654 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | dm-pbm | 0.2267 | +0.01969 ± 0.00018 | 0.01977 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.2267 | +0.00006 ± 0.00016 | 0.00158 | 0.57 |  |  | 100% | 0.57 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.2267 | +0.01381 ± 0.00030 | 0.01412 | 0.01 | 6410 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | cdr | 0.2267 | +0.03117 ± 0.00220 | 0.03811 | 0.25 | 15 | 0.654 | 0% |  |  | biased, as expected: 19% of its slots unsupported |

## nfcorpus · ubm · {"policy":"epsilon","epsilon":0.3,"top":10}

100 logs of 100000 impressions, 13 s. Propensities: 10000000 lists, 0 mismatched; chi-square 10843 on 11160 df (p 0.984), largest |z| 3.78.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.4260 | 0.2494 | 0.2086 | 2.409 |
| rerank | 0.0% | 0.4095 | 0.2343 | 0.2033 | 2.413 |
| oracle | 0.0% | 0.6106 | 0.3873 | 0.2979 | 2.360 |
| reverse | 0.0% | 0.2291 | 0.1066 | 0.0846 | 2.483 |
| wide | 18.6% | 0.3993 | 0.2267 | 0.2004 | 2.414 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.4260 | -0.03831 ± 0.00023 | 0.03837 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.4260 | -0.00006 ± 0.00028 | 0.00275 | 0.97 | 69991 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | clicks | snips | 0.4260 | +0.00000 ± 0.00026 | 0.00264 | 0.95 | 69991 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | clicks | iips | 0.4260 | -0.00042 ± 0.00027 | 0.00272 | 0.96 | 72928 | 1.000 | 100% | 0.96 |  | unbiased here |
| ranking | clicks | sniips | 0.4260 | -0.00035 ± 0.00026 | 0.00257 | 0.94 | 72928 | 1.000 | 100% | 0.94 |  | unbiased here |
| ranking | clicks | rips | 0.4260 | -0.00007 ± 0.00027 | 0.00272 | 0.97 | 69991 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | clicks | snrips | 0.4260 | +0.00000 ± 0.00026 | 0.00257 | 0.96 | 69991 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | clicks | dm-pbm | 0.4260 | -0.00105 ± 0.00024 | 0.00258 | 0.56 |  |  | 100% | 0.56 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.4260 | -0.01190 ± 0.00023 | 0.01213 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.4260 | -0.00041 ± 0.00024 | 0.00246 | 0.96 | 72928 | 1.000 | 100% | 0.96 |  | unbiased here |
| ranking | clicks | cdr | 0.4260 | -0.00008 ± 0.00024 | 0.00243 | 0.95 | 69991 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | satisfied | naive | 0.2494 | -0.02875 ± 0.00016 | 0.02879 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.2494 | -0.00000 ± 0.00020 | 0.00202 | 0.99 | 69991 | 1.000 | 100% | 0.99 | yes | ok |
| ranking | satisfied | snips | 0.2494 | +0.00003 ± 0.00020 | 0.00195 | 0.98 | 69991 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | satisfied | iips | 0.2494 | -0.00026 ± 0.00020 | 0.00198 | 0.95 | 72928 | 1.000 | 100% | 0.95 |  | unbiased here |
| ranking | satisfied | sniips | 0.2494 | -0.00022 ± 0.00019 | 0.00191 | 0.96 | 72928 | 1.000 | 100% | 0.96 |  | unbiased here |
| ranking | satisfied | rips | 0.2494 | -0.00004 ± 0.00020 | 0.00200 | 0.98 | 69991 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | satisfied | snrips | 0.2494 | +0.00001 ± 0.00019 | 0.00192 | 0.99 | 69991 | 1.000 | 100% | 0.99 | yes | ok |
| ranking | satisfied | dm-pbm | 0.2494 | +0.00032 ± 0.00017 | 0.00176 | 0.70 |  |  | 100% | 0.70 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.2494 | -0.00371 ± 0.00017 | 0.00409 | 0.14 |  |  | 100% | 0.14 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.2494 | -0.00027 ± 0.00018 | 0.00180 | 0.97 | 72928 | 1.000 | 100% | 0.97 |  | unbiased here |
| ranking | satisfied | cdr | 0.2494 | -0.00006 ± 0.00018 | 0.00180 | 0.98 | 69991 | 1.000 | 100% | 0.98 | yes | ok |
| rerank | clicks | naive | 0.4095 | -0.02179 ± 0.00023 | 0.02191 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.4095 | +0.10701 ± 0.08416 | 0.84419 | 0.31 | 2657 | 1.030 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2657 impressions, mean 1.03; 0% of logs usable) |
| rerank | clicks | snips | 0.4095 | -0.03458 ± 0.03930 | 0.39252 | 0.13 | 2657 | 1.030 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2657 impressions, mean 1.03; 0% of logs usable) |
| rerank | clicks | iips | 0.4095 | -0.00036 ± 0.00077 | 0.00763 | 0.97 | 3999 | 0.999 | 98% | 0.97 |  | unbiased here |
| rerank | clicks | sniips | 0.4095 | -0.00027 ± 0.00071 | 0.00710 | 0.97 | 3999 | 0.999 | 98% | 0.97 |  | unbiased here |
| rerank | clicks | rips | 0.4095 | +0.02085 ± 0.02125 | 0.21249 | 0.65 | 4 | 0.741 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.74; 0% of logs usable) |
| rerank | clicks | snrips | 0.4095 | +0.00155 ± 0.01367 | 0.13606 | 0.67 | 4 | 0.741 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.74; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.4095 | +0.01430 ± 0.00029 | 0.01458 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.4095 | -0.00292 ± 0.00028 | 0.00406 | 0.33 |  |  | 100% | 0.33 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.4095 | -0.00104 ± 0.00065 | 0.00652 | 0.97 | 3999 | 0.999 | 98% | 0.97 |  | unbiased here |
| rerank | clicks | cdr | 0.4095 | +0.01144 ± 0.02148 | 0.21401 | 0.97 | 4 | 0.741 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.74; 0% of logs usable) |
| rerank | satisfied | naive | 0.2343 | -0.01369 ± 0.00016 | 0.01378 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.2343 | +0.06872 ± 0.06129 | 0.61368 | 0.22 | 2657 | 1.030 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2657 impressions, mean 1.03; 0% of logs usable) |
| rerank | satisfied | snips | 0.2343 | -0.02749 ± 0.02998 | 0.29956 | 0.13 | 2657 | 1.030 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2657 impressions, mean 1.03; 0% of logs usable) |
| rerank | satisfied | iips | 0.2343 | +0.00003 ± 0.00062 | 0.00616 | 0.95 | 3999 | 0.999 | 98% | 0.95 |  | unbiased here |
| rerank | satisfied | sniips | 0.2343 | +0.00007 ± 0.00058 | 0.00577 | 0.94 | 3999 | 0.999 | 98% | 0.94 |  | unbiased here |
| rerank | satisfied | rips | 0.2343 | -0.00353 ± 0.01137 | 0.11315 | 0.60 | 4 | 0.741 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.74; 0% of logs usable) |
| rerank | satisfied | snrips | 0.2343 | -0.01053 ± 0.00643 | 0.06487 | 0.60 | 4 | 0.741 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.74; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.2343 | +0.01086 ± 0.00018 | 0.01101 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.2343 | +0.00074 ± 0.00018 | 0.00196 | 0.53 |  |  | 100% | 0.53 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.2343 | -0.00060 ± 0.00049 | 0.00489 | 0.95 | 3999 | 0.999 | 98% | 0.95 |  | unbiased here |
| rerank | satisfied | cdr | 0.2343 | -0.01142 ± 0.01254 | 0.12532 | 0.95 | 4 | 0.741 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.74; 0% of logs usable) |
| oracle | clicks | naive | 0.6106 | -0.22295 ± 0.00023 | 0.22296 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.6106 | -0.01583 ± 0.08155 | 0.81152 | 0.30 | 19765 | 1.060 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19765 impressions, mean 1.06; 0% of logs usable) |
| oracle | clicks | snips | 0.6106 | -0.16498 ± 0.03770 | 0.40978 | 0.21 | 19765 | 1.060 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19765 impressions, mean 1.06; 0% of logs usable) |
| oracle | clicks | iips | 0.6106 | -0.02197 ± 0.00110 | 0.02456 | 0.44 | 7334 | 1.000 | 98% | 0.44 |  | biased, as expected |
| oracle | clicks | sniips | 0.6106 | -0.02256 ± 0.00090 | 0.02428 | 0.23 | 7334 | 1.000 | 98% | 0.23 |  | biased, as expected |
| oracle | clicks | rips | 0.6106 | -0.00618 ± 0.01331 | 0.13255 | 0.76 | 10 | 0.754 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 10 impressions, mean 0.75; 0% of logs usable) |
| oracle | clicks | snrips | 0.6106 | -0.01174 ± 0.00831 | 0.08347 | 0.76 | 10 | 0.754 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 10 impressions, mean 0.75; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.6106 | -0.04213 ± 0.00031 | 0.04225 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.6106 | -0.15264 ± 0.00028 | 0.15267 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.6106 | -0.02296 ± 0.00079 | 0.02427 | 0.18 | 7334 | 1.000 | 98% | 0.18 |  | biased, as expected |
| oracle | clicks | cdr | 0.6106 | -0.01276 ± 0.01381 | 0.13804 | 0.89 | 10 | 0.754 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 10 impressions, mean 0.75; 0% of logs usable) |
| oracle | satisfied | naive | 0.3873 | -0.16667 ± 0.00016 | 0.16667 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.3873 | -0.01390 ± 0.07375 | 0.73395 | 0.19 | 19765 | 1.060 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19765 impressions, mean 1.06; 0% of logs usable) |
| oracle | satisfied | snips | 0.3873 | -0.12432 ± 0.03724 | 0.39085 | 0.12 | 19765 | 1.060 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19765 impressions, mean 1.06; 0% of logs usable) |
| oracle | satisfied | iips | 0.3873 | -0.01461 ± 0.00091 | 0.01720 | 0.53 | 7334 | 1.000 | 98% | 0.53 |  | biased, as expected |
| oracle | satisfied | sniips | 0.3873 | -0.01503 ± 0.00078 | 0.01691 | 0.44 | 7334 | 1.000 | 98% | 0.44 |  | biased, as expected |
| oracle | satisfied | rips | 0.3873 | -0.00880 ± 0.00503 | 0.05085 | 0.80 | 10 | 0.754 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 10 impressions, mean 0.75; 0% of logs usable) |
| oracle | satisfied | snrips | 0.3873 | -0.00926 ± 0.00497 | 0.05031 | 0.78 | 10 | 0.754 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 10 impressions, mean 0.75; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.3873 | -0.03091 ± 0.00026 | 0.03102 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.3873 | -0.10747 ± 0.00021 | 0.10749 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.3873 | -0.01535 ± 0.00063 | 0.01659 | 0.34 | 7334 | 1.000 | 98% | 0.34 |  | biased, as expected |
| oracle | satisfied | cdr | 0.3873 | -0.01509 ± 0.00607 | 0.06221 | 0.88 | 10 | 0.754 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 10 impressions, mean 0.75; 0% of logs usable) |
| reverse | clicks | naive | 0.2291 | +0.15862 ± 0.00023 | 0.15863 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.2291 | -0.04763 ± 0.05241 | 0.52361 | 0.13 | 1 | 0.837 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.84; 0% of logs usable) |
| reverse | clicks | snips | 0.2291 | -0.08574 ± 0.04612 | 0.46682 | 0.05 | 1 | 0.837 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.84; 0% of logs usable) |
| reverse | clicks | iips | 0.2291 | -0.00031 ± 0.00080 | 0.00798 | 0.98 | 2934 | 0.995 | 97% | 0.98 |  | unbiased here |
| reverse | clicks | sniips | 0.2291 | -0.00007 ± 0.00077 | 0.00768 | 0.96 | 2934 | 0.995 | 97% | 0.96 |  | unbiased here |
| reverse | clicks | rips | 0.2291 | -0.00566 ± 0.01362 | 0.13565 | 0.72 | 1 | 0.594 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.59; 0% of logs usable) |
| reverse | clicks | snrips | 0.2291 | -0.01185 ± 0.00764 | 0.07695 | 0.72 | 1 | 0.594 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.59; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.2291 | +0.07576 ± 0.00037 | 0.07585 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.2291 | +0.02545 ± 0.00036 | 0.02571 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.2291 | +0.00012 ± 0.00074 | 0.00737 | 0.97 | 2934 | 0.995 | 97% | 0.97 |  | unbiased here |
| reverse | clicks | cdr | 0.2291 | +0.00744 ± 0.01272 | 0.12680 | 0.95 | 1 | 0.594 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.59; 0% of logs usable) |
| reverse | satisfied | naive | 0.1066 | +0.11409 ± 0.00016 | 0.11410 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.1066 | -0.04609 ± 0.02800 | 0.28240 | 0.05 | 1 | 0.837 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.84; 0% of logs usable) |
| reverse | satisfied | snips | 0.1066 | -0.05907 ± 0.02480 | 0.25369 | 0.02 | 1 | 0.837 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.84; 0% of logs usable) |
| reverse | satisfied | iips | 0.1066 | -0.00173 ± 0.00058 | 0.00603 | 0.96 | 2934 | 0.995 | 97% | 0.96 |  | unbiased here |
| reverse | satisfied | sniips | 0.1066 | -0.00161 ± 0.00057 | 0.00591 | 0.96 | 2934 | 0.995 | 97% | 0.96 |  | unbiased here |
| reverse | satisfied | rips | 0.1066 | +0.00370 ± 0.01183 | 0.11772 | 0.62 | 1 | 0.594 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.59; 0% of logs usable) |
| reverse | satisfied | snrips | 0.1066 | -0.00081 ± 0.00645 | 0.06421 | 0.62 | 1 | 0.594 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.59; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.1066 | +0.05285 ± 0.00024 | 0.05291 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.1066 | +0.02661 ± 0.00023 | 0.02671 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.1066 | -0.00140 ± 0.00054 | 0.00551 | 0.93 | 2934 | 0.995 | 97% | 0.93 |  | unbiased here |
| reverse | satisfied | cdr | 0.1066 | +0.01090 ± 0.01118 | 0.11172 | 0.94 | 1 | 0.594 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.59; 0% of logs usable) |
| wide | clicks | naive | 0.3993 | -0.01165 ± 0.00023 | 0.01187 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.3993 | -0.05261 ± 0.06575 | 0.65629 | 0.24 | 2643 | 0.592 | 0% |  |  | unbiased though unsupported |
| wide | clicks | snips | 0.3993 | -0.02631 ± 0.04299 | 0.42850 | 0.04 | 2643 | 0.592 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.3993 | -0.01439 ± 0.00076 | 0.01627 | 0.51 | 2529 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | sniips | 0.3993 | +0.02357 ± 0.00086 | 0.02507 | 0.21 | 2529 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | rips | 0.3993 | -0.01320 ± 0.01597 | 0.15940 | 0.55 | 3 | 0.388 | 0% |  |  | unbiased though unsupported |
| wide | clicks | snrips | 0.3993 | +0.01938 ± 0.01456 | 0.14618 | 0.72 | 3 | 0.388 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.3993 | +0.03506 ± 0.00029 | 0.03518 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.3993 | +0.04245 ± 0.00028 | 0.04254 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.3993 | +0.02220 ± 0.00067 | 0.02317 | 0.07 | 2529 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | cdr | 0.3993 | +0.06240 ± 0.01545 | 0.16589 | 0.72 | 3 | 0.388 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | naive | 0.2267 | -0.00602 ± 0.00016 | 0.00621 | 0.02 |  |  | 100% | 0.02 |  | biased, as expected |
| wide | satisfied | ips | 0.2267 | -0.04902 ± 0.04543 | 0.45472 | 0.14 | 2643 | 0.592 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snips | 0.2267 | -0.03984 ± 0.03319 | 0.33259 | 0.04 | 2643 | 0.592 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.2267 | -0.00432 ± 0.00055 | 0.00701 | 0.88 | 2529 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | sniips | 0.2267 | +0.01488 ± 0.00059 | 0.01599 | 0.34 | 2529 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | rips | 0.2267 | -0.01718 ± 0.00457 | 0.04858 | 0.50 | 3 | 0.388 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.2267 | +0.00158 ± 0.00744 | 0.07408 | 0.53 | 3 | 0.388 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | dm-pbm | 0.2267 | +0.02574 ± 0.00018 | 0.02580 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.2267 | +0.02944 ± 0.00019 | 0.02950 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.2267 | +0.01628 ± 0.00043 | 0.01682 | 0.04 | 2529 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | cdr | 0.2267 | +0.02338 ± 0.00583 | 0.06254 | 0.65 | 3 | 0.388 | 0% |  |  | unbiased though unsupported |

## nfcorpus · ubm · {"policy":"shuffle","top":10}

100 logs of 100000 impressions, 12 s. Propensities: 10000000 lists, 0 mismatched; chi-square 11183 on 11160 df (p 0.437), largest |z| 4.21.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.4260 | 0.2494 | 0.2086 | 2.409 |
| rerank | 0.0% | 0.4095 | 0.2343 | 0.2033 | 2.413 |
| oracle | 0.0% | 0.6106 | 0.3873 | 0.2979 | 2.360 |
| reverse | 0.0% | 0.2291 | 0.1066 | 0.0846 | 2.483 |
| wide | 18.6% | 0.3993 | 0.2267 | 0.2004 | 2.414 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.4260 | -0.12718 ± 0.00017 | 0.12719 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.4260 | -0.01775 ± 0.04676 | 0.46563 | 0.60 | 3 | 1.001 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| ranking | clicks | snips | 0.4260 | -0.04815 ± 0.04119 | 0.41265 | 0.45 | 3 | 1.001 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| ranking | clicks | iips | 0.4260 | -0.00777 ± 0.00052 | 0.00937 | 0.84 | 9895 | 1.001 | 100% | 0.84 |  | biased, as expected |
| ranking | clicks | sniips | 0.4260 | -0.00761 ± 0.00055 | 0.00939 | 0.75 | 9895 | 1.001 | 100% | 0.75 |  | biased, as expected |
| ranking | clicks | rips | 0.4260 | -0.01041 ± 0.00821 | 0.08231 | 0.60 | 3 | 0.853 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.85; 0% of logs usable) |
| ranking | clicks | snrips | 0.4260 | -0.01017 ± 0.00738 | 0.07415 | 0.57 | 3 | 0.853 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.85; 0% of logs usable) |
| ranking | clicks | dm-pbm | 0.4260 | -0.00246 ± 0.00029 | 0.00376 | 0.41 |  |  | 100% | 0.41 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.4260 | -0.07195 ± 0.00022 | 0.07199 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.4260 | -0.00792 ± 0.00051 | 0.00939 | 0.71 | 9895 | 1.001 | 100% | 0.71 |  | biased, as expected |
| ranking | clicks | cdr | 0.4260 | -0.00381 ± 0.00859 | 0.08552 | 0.92 | 3 | 0.853 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.85; 0% of logs usable) |
| ranking | satisfied | naive | 0.2494 | -0.09533 ± 0.00013 | 0.09533 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.2494 | -0.00144 ± 0.03657 | 0.36384 | 0.47 | 3 | 1.001 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| ranking | satisfied | snips | 0.2494 | -0.02653 ± 0.03092 | 0.30881 | 0.39 | 3 | 1.001 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| ranking | satisfied | iips | 0.2494 | -0.00429 ± 0.00043 | 0.00608 | 0.92 | 9895 | 1.001 | 100% | 0.92 |  | biased, as expected |
| ranking | satisfied | sniips | 0.2494 | -0.00419 ± 0.00045 | 0.00610 | 0.89 | 9895 | 1.001 | 100% | 0.89 |  | biased, as expected |
| ranking | satisfied | rips | 0.2494 | -0.00072 ± 0.00642 | 0.06388 | 0.70 | 3 | 0.853 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.85; 0% of logs usable) |
| ranking | satisfied | snrips | 0.2494 | -0.00101 ± 0.00580 | 0.05771 | 0.69 | 3 | 0.853 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.85; 0% of logs usable) |
| ranking | satisfied | dm-pbm | 0.2494 | -0.00476 ± 0.00024 | 0.00534 | 0.15 |  |  | 100% | 0.15 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.2494 | -0.04931 ± 0.00020 | 0.04935 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.2494 | -0.00453 ± 0.00041 | 0.00612 | 0.80 | 9895 | 1.001 | 100% | 0.80 |  | biased, as expected |
| ranking | satisfied | cdr | 0.2494 | +0.00250 ± 0.00635 | 0.06327 | 0.92 | 3 | 0.853 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.85; 0% of logs usable) |
| rerank | clicks | naive | 0.4095 | -0.11067 ± 0.00017 | 0.11068 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.4095 | -0.01031 ± 0.04165 | 0.41449 | 0.65 | 3 | 0.901 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.90; 0% of logs usable) |
| rerank | clicks | snips | 0.4095 | +0.02373 ± 0.04540 | 0.45235 | 0.54 | 3 | 0.901 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.90; 0% of logs usable) |
| rerank | clicks | iips | 0.4095 | -0.00580 ± 0.00058 | 0.00816 | 0.88 | 9888 | 0.999 | 100% | 0.88 |  | biased, as expected |
| rerank | clicks | sniips | 0.4095 | -0.00536 ± 0.00054 | 0.00761 | 0.86 | 9888 | 0.999 | 100% | 0.86 |  | biased, as expected |
| rerank | clicks | rips | 0.4095 | +0.00304 ± 0.00983 | 0.09788 | 0.66 | 3 | 0.817 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.82; 0% of logs usable) |
| rerank | clicks | snrips | 0.4095 | +0.00720 ± 0.01046 | 0.10428 | 0.65 | 3 | 0.817 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.82; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.4095 | +0.00340 ± 0.00030 | 0.00452 | 0.24 |  |  | 100% | 0.24 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.4095 | -0.06551 ± 0.00023 | 0.06555 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.4095 | -0.00570 ± 0.00049 | 0.00753 | 0.81 | 9888 | 0.999 | 100% | 0.81 |  | biased, as expected |
| rerank | clicks | cdr | 0.4095 | +0.01173 ± 0.00925 | 0.09283 | 0.97 | 3 | 0.817 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.82; 0% of logs usable) |
| rerank | satisfied | naive | 0.2343 | -0.08026 ± 0.00013 | 0.08028 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.2343 | -0.01359 ± 0.02911 | 0.28991 | 0.47 | 3 | 0.901 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.90; 0% of logs usable) |
| rerank | satisfied | snips | 0.2343 | -0.00632 ± 0.03069 | 0.30543 | 0.43 | 3 | 0.901 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.90; 0% of logs usable) |
| rerank | satisfied | iips | 0.2343 | -0.00261 ± 0.00046 | 0.00530 | 0.92 | 9888 | 0.999 | 100% | 0.92 |  | biased, as expected |
| rerank | satisfied | sniips | 0.2343 | -0.00232 ± 0.00046 | 0.00512 | 0.90 | 9888 | 0.999 | 100% | 0.90 |  | biased, as expected |
| rerank | satisfied | rips | 0.2343 | +0.00660 ± 0.00744 | 0.07430 | 0.70 | 3 | 0.817 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.82; 0% of logs usable) |
| rerank | satisfied | snrips | 0.2343 | +0.00868 ± 0.00844 | 0.08441 | 0.69 | 3 | 0.817 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.82; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.2343 | +0.00047 ± 0.00024 | 0.00243 | 0.52 |  |  | 100% | 0.52 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.2343 | -0.04477 ± 0.00018 | 0.04481 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.2343 | -0.00264 ± 0.00040 | 0.00480 | 0.89 | 9888 | 0.999 | 100% | 0.89 |  | biased, as expected |
| rerank | satisfied | cdr | 0.2343 | +0.01038 ± 0.00694 | 0.06983 | 0.99 | 3 | 0.817 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.82; 0% of logs usable) |
| oracle | clicks | naive | 0.6106 | -0.31182 ± 0.00017 | 0.31183 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.6106 | +0.12117 ± 0.06231 | 0.63173 | 0.79 | 4 | 1.089 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.09; 0% of logs usable) |
| oracle | clicks | snips | 0.6106 | +0.02929 ± 0.05014 | 0.49976 | 0.61 | 4 | 1.089 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.09; 0% of logs usable) |
| oracle | clicks | iips | 0.6106 | -0.03230 ± 0.00073 | 0.03312 | 0.02 | 9891 | 1.001 | 100% | 0.02 |  | biased, as expected |
| oracle | clicks | sniips | 0.6106 | -0.03252 ± 0.00073 | 0.03332 | 0.00 | 9891 | 1.001 | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | rips | 0.6106 | +0.01995 ± 0.01188 | 0.11989 | 0.70 | 4 | 0.914 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.91; 0% of logs usable) |
| oracle | clicks | snrips | 0.6106 | +0.00641 ± 0.00900 | 0.08978 | 0.72 | 4 | 0.914 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.91; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.6106 | -0.04327 ± 0.00035 | 0.04341 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.6106 | -0.20975 ± 0.00023 | 0.20976 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.6106 | -0.03256 ± 0.00066 | 0.03321 | 0.00 | 9891 | 1.001 | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | cdr | 0.6106 | +0.01775 ± 0.00986 | 0.09972 | 0.87 | 4 | 0.914 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.91; 0% of logs usable) |
| oracle | satisfied | naive | 0.3873 | -0.23324 ± 0.00013 | 0.23325 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.3873 | +0.07837 ± 0.04638 | 0.46804 | 0.69 | 4 | 1.089 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.09; 0% of logs usable) |
| oracle | satisfied | snips | 0.3873 | +0.01434 ± 0.03830 | 0.38139 | 0.64 | 4 | 1.089 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.09; 0% of logs usable) |
| oracle | satisfied | iips | 0.3873 | -0.02122 ± 0.00064 | 0.02214 | 0.09 | 9891 | 1.001 | 100% | 0.09 |  | biased, as expected |
| oracle | satisfied | sniips | 0.3873 | -0.02134 ± 0.00062 | 0.02222 | 0.05 | 9891 | 1.001 | 100% | 0.05 |  | biased, as expected |
| oracle | satisfied | rips | 0.3873 | +0.02549 ± 0.01050 | 0.10750 | 0.75 | 4 | 0.914 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.91; 0% of logs usable) |
| oracle | satisfied | snrips | 0.3873 | +0.01523 ± 0.00797 | 0.08074 | 0.78 | 4 | 0.914 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.91; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.3873 | -0.03473 ± 0.00030 | 0.03486 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.3873 | -0.15227 ± 0.00020 | 0.15228 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.3873 | -0.02149 ± 0.00054 | 0.02215 | 0.02 | 9891 | 1.001 | 100% | 0.02 |  | biased, as expected |
| oracle | satisfied | cdr | 0.3873 | +0.02395 ± 0.00922 | 0.09478 | 0.90 | 4 | 0.914 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.91; 0% of logs usable) |
| reverse | clicks | naive | 0.2291 | +0.06974 ± 0.00017 | 0.06976 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.2291 | +0.04309 ± 0.03398 | 0.34083 | 0.59 | 3 | 1.022 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.02; 0% of logs usable) |
| reverse | clicks | snips | 0.2291 | +0.02044 ± 0.02723 | 0.27175 | 0.55 | 3 | 1.022 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.02; 0% of logs usable) |
| reverse | clicks | iips | 0.2291 | +0.00023 ± 0.00052 | 0.00520 | 0.92 | 9892 | 0.999 | 97% | 0.93 |  | unbiased here |
| reverse | clicks | sniips | 0.2291 | +0.00033 ± 0.00050 | 0.00502 | 0.92 | 9892 | 0.999 | 97% | 0.92 |  | unbiased here |
| reverse | clicks | rips | 0.2291 | -0.00172 ± 0.00780 | 0.07768 | 0.60 | 3 | 0.983 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.98; 0% of logs usable) |
| reverse | clicks | snrips | 0.2291 | -0.00095 ± 0.00882 | 0.08778 | 0.59 | 3 | 0.983 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.98; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.2291 | +0.02559 ± 0.00024 | 0.02571 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.2291 | +0.01669 ± 0.00020 | 0.01680 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.2291 | +0.00042 ± 0.00047 | 0.00466 | 0.94 | 9892 | 0.999 | 97% | 0.94 |  | unbiased here |
| reverse | clicks | cdr | 0.2291 | -0.00877 ± 0.00822 | 0.08222 | 0.97 | 3 | 0.983 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.98; 0% of logs usable) |
| reverse | satisfied | naive | 0.1066 | +0.04751 ± 0.00013 | 0.04753 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.1066 | +0.02044 ± 0.02282 | 0.22794 | 0.31 | 3 | 1.022 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.02; 0% of logs usable) |
| reverse | satisfied | snips | 0.1066 | +0.00105 ± 0.01917 | 0.19079 | 0.31 | 3 | 1.022 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.02; 0% of logs usable) |
| reverse | satisfied | iips | 0.1066 | -0.00069 ± 0.00037 | 0.00370 | 0.91 | 9892 | 0.999 | 97% | 0.91 |  | unbiased here |
| reverse | satisfied | sniips | 0.1066 | -0.00064 ± 0.00036 | 0.00362 | 0.91 | 9892 | 0.999 | 97% | 0.91 |  | unbiased here |
| reverse | satisfied | rips | 0.1066 | +0.00099 ± 0.00515 | 0.05123 | 0.72 | 3 | 0.983 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.98; 0% of logs usable) |
| reverse | satisfied | snrips | 0.1066 | +0.00044 ± 0.00462 | 0.04600 | 0.68 | 3 | 0.983 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.98; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.1066 | +0.01633 ± 0.00017 | 0.01642 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.1066 | +0.01422 ± 0.00014 | 0.01429 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.1066 | -0.00059 ± 0.00031 | 0.00315 | 0.95 | 9892 | 0.999 | 97% | 0.95 |  | unbiased here |
| reverse | satisfied | cdr | 0.1066 | -0.00339 ± 0.00548 | 0.05461 | 0.99 | 3 | 0.983 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.98; 0% of logs usable) |
| wide | clicks | naive | 0.3993 | -0.10053 ± 0.00017 | 0.10055 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.3993 | -0.16347 ± 0.03087 | 0.34793 | 0.48 | 1 | 0.432 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snips | 0.3993 | +0.04282 ± 0.06125 | 0.61091 | 0.27 | 1 | 0.432 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.3993 | -0.01965 ± 0.00054 | 0.02039 | 0.07 | 6097 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | sniips | 0.3993 | +0.01795 ± 0.00059 | 0.01888 | 0.20 | 6097 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | rips | 0.3993 | -0.03068 ± 0.00586 | 0.06592 | 0.46 | 1 | 0.625 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snrips | 0.3993 | +0.01501 ± 0.00939 | 0.09460 | 0.77 | 1 | 0.625 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.3993 | +0.02053 ± 0.00030 | 0.02075 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.3993 | -0.03651 ± 0.00024 | 0.03659 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.3993 | +0.01310 ± 0.00047 | 0.01391 | 0.24 | 6097 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | cdr | 0.3993 | +0.04744 ± 0.00594 | 0.07582 | 0.69 | 1 | 0.625 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | naive | 0.2267 | -0.07259 ± 0.00013 | 0.07261 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.2267 | -0.09664 ± 0.02117 | 0.23173 | 0.33 | 1 | 0.432 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | snips | 0.2267 | +0.02983 ± 0.04700 | 0.46856 | 0.20 | 1 | 0.432 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.2267 | -0.00715 ± 0.00043 | 0.00836 | 0.66 | 6097 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | sniips | 0.2267 | +0.01191 ± 0.00048 | 0.01283 | 0.32 | 6097 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | rips | 0.2267 | -0.01105 ± 0.00478 | 0.04883 | 0.55 | 1 | 0.625 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.2267 | +0.01311 ± 0.00785 | 0.07915 | 0.87 | 1 | 0.625 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | dm-pbm | 0.2267 | +0.01172 ± 0.00025 | 0.01198 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.2267 | -0.02725 ± 0.00020 | 0.02732 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.2267 | +0.00965 ± 0.00038 | 0.01036 | 0.32 | 6097 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | cdr | 0.2267 | +0.02856 ± 0.00457 | 0.05374 | 0.69 | 1 | 0.625 | 0% |  |  | biased, as expected: 19% of its slots unsupported |

## nfcorpus · ubm · {"policy":"swap","top":10}

100 logs of 100000 impressions, 12 s. Propensities: 10000000 lists, 0 mismatched; chi-square 3145 on 3224 df (p 0.838), largest |z| 3.57.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.4260 | 0.2494 | 0.2086 | 2.409 |
| rerank | 50.4% | 0.4095 | 0.2343 | 0.2033 | 2.413 |
| oracle | 24.9% | 0.6106 | 0.3873 | 0.2979 | 2.360 |
| reverse | 80.0% | 0.2291 | 0.1066 | 0.0846 | 2.483 |
| wide | 54.7% | 0.3993 | 0.2267 | 0.2004 | 2.414 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.4260 | -0.07984 ± 0.00019 | 0.07986 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.4260 | -0.00013 ± 0.00077 | 0.00765 | 0.93 | 10010 | 1.001 | 100% | 0.93 | yes | ok |
| ranking | clicks | snips | 0.4260 | -0.00054 ± 0.00064 | 0.00636 | 0.95 | 10010 | 1.001 | 100% | 0.95 | yes | ok |
| ranking | clicks | iips | 0.4260 | -0.00622 ± 0.00052 | 0.00809 | 0.75 | 10010 | 1.000 | 100% | 0.75 |  | biased, as expected |
| ranking | clicks | sniips | 0.4260 | -0.00644 ± 0.00047 | 0.00795 | 0.68 | 10010 | 1.000 | 100% | 0.68 |  | biased, as expected |
| ranking | clicks | rips | 0.4260 | -0.00013 ± 0.00077 | 0.00765 | 0.93 | 10010 | 1.001 | 100% | 0.93 | yes | ok |
| ranking | clicks | snrips | 0.4260 | -0.00054 ± 0.00064 | 0.00636 | 0.95 | 10010 | 1.001 | 100% | 0.95 | yes | ok |
| ranking | clicks | dm-pbm | 0.4260 | +0.00278 ± 0.00029 | 0.00399 | 0.42 |  |  | 100% | 0.42 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.4260 | -0.07687 ± 0.00021 | 0.07690 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.4260 | -0.00647 ± 0.00039 | 0.00756 | 0.61 | 10010 | 1.000 | 100% | 0.61 |  | biased, as expected |
| ranking | clicks | cdr | 0.4260 | -0.00045 ± 0.00057 | 0.00570 | 0.94 | 10010 | 1.001 | 100% | 0.94 | yes | ok |
| ranking | satisfied | naive | 0.2494 | -0.06120 ± 0.00013 | 0.06121 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.2494 | +0.00029 ± 0.00056 | 0.00558 | 0.96 | 10010 | 1.001 | 100% | 0.96 | yes | ok |
| ranking | satisfied | snips | 0.2494 | +0.00005 ± 0.00051 | 0.00506 | 0.96 | 10010 | 1.001 | 100% | 0.96 | yes | ok |
| ranking | satisfied | iips | 0.2494 | -0.00324 ± 0.00039 | 0.00503 | 0.88 | 10010 | 1.000 | 100% | 0.88 |  | biased, as expected |
| ranking | satisfied | sniips | 0.2494 | -0.00337 ± 0.00037 | 0.00500 | 0.84 | 10010 | 1.000 | 100% | 0.84 |  | biased, as expected |
| ranking | satisfied | rips | 0.2494 | +0.00029 ± 0.00056 | 0.00558 | 0.96 | 10010 | 1.001 | 100% | 0.96 | yes | ok |
| ranking | satisfied | snrips | 0.2494 | +0.00005 ± 0.00051 | 0.00506 | 0.96 | 10010 | 1.001 | 100% | 0.96 | yes | ok |
| ranking | satisfied | dm-pbm | 0.2494 | +0.00124 ± 0.00019 | 0.00230 | 0.55 |  |  | 100% | 0.55 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.2494 | -0.04716 ± 0.00015 | 0.04718 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.2494 | -0.00348 ± 0.00028 | 0.00448 | 0.82 | 10010 | 1.000 | 100% | 0.82 |  | biased, as expected |
| ranking | satisfied | cdr | 0.2494 | +0.00008 ± 0.00042 | 0.00417 | 0.94 | 10010 | 1.001 | 100% | 0.94 | yes | ok |
| rerank | clicks | naive | 0.4095 | -0.06332 ± 0.00019 | 0.06335 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.4095 | -0.38590 ± 0.00018 | 0.38590 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | snips | 0.4095 | -0.16394 ± 0.00155 | 0.16467 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | iips | 0.4095 | -0.09125 ± 0.00054 | 0.09140 | 0.00 | 10005 | 0.290 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | sniips | 0.4095 | +0.02427 ± 0.00061 | 0.02503 | 0.00 | 10005 | 0.290 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | rips | 0.4095 | -0.11370 ± 0.00066 | 0.11389 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | snrips | 0.4095 | -0.01517 ± 0.00091 | 0.01766 | 0.64 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | dm-pbm | 0.4095 | +0.01055 ± 0.00027 | 0.01089 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.4095 | -0.01787 ± 0.00024 | 0.01803 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.4095 | +0.00633 ± 0.00041 | 0.00753 | 0.62 | 10005 | 0.290 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | cdr | 0.4095 | +0.09335 ± 0.00050 | 0.09348 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | naive | 0.2343 | -0.04614 ± 0.00013 | 0.04616 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.2343 | -0.22321 ± 0.00012 | 0.22322 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | snips | 0.2343 | -0.11848 ± 0.00112 | 0.11900 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | iips | 0.2343 | -0.04214 ± 0.00042 | 0.04235 | 0.00 | 10005 | 0.290 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | sniips | 0.2343 | +0.02101 ± 0.00049 | 0.02156 | 0.01 | 10005 | 0.290 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | rips | 0.2343 | -0.05691 ± 0.00047 | 0.05710 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | snrips | 0.2343 | -0.01279 ± 0.00065 | 0.01432 | 0.46 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | dm-pbm | 0.2343 | +0.00684 ± 0.00020 | 0.00712 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.2343 | -0.01814 ± 0.00019 | 0.01824 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.2343 | +0.00576 ± 0.00031 | 0.00651 | 0.51 | 10005 | 0.290 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | cdr | 0.2343 | +0.05113 ± 0.00037 | 0.05126 | 0.00 | 960 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| oracle | clicks | naive | 0.6106 | -0.26448 ± 0.00019 | 0.26449 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.6106 | -0.42041 ± 0.00051 | 0.42044 | 0.00 | 5588 | 0.559 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | snips | 0.6106 | -0.27024 ± 0.00079 | 0.27035 | 0.00 | 5588 | 0.559 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | iips | 0.6106 | -0.14895 ± 0.00066 | 0.14909 | 0.00 | 10017 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | sniips | 0.6106 | -0.10891 ± 0.00053 | 0.10904 | 0.00 | 10017 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | rips | 0.6106 | -0.14826 ± 0.00080 | 0.14847 | 0.00 | 5588 | 0.559 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | snrips | 0.6106 | -0.09165 ± 0.00074 | 0.09195 | 0.00 | 5588 | 0.559 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | dm-pbm | 0.6106 | -0.03852 ± 0.00029 | 0.03863 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.6106 | -0.17165 ± 0.00025 | 0.17167 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.6106 | -0.03400 ± 0.00042 | 0.03426 | 0.00 | 10017 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | cdr | 0.6106 | -0.00303 ± 0.00054 | 0.00616 | 0.89 | 5588 | 0.559 | 0% |  |  | unbiased though unsupported |
| oracle | satisfied | naive | 0.3873 | -0.19912 ± 0.00013 | 0.19912 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.3873 | -0.28128 ± 0.00035 | 0.28130 | 0.00 | 5588 | 0.559 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | snips | 0.3873 | -0.19756 ± 0.00060 | 0.19765 | 0.00 | 5588 | 0.559 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | iips | 0.3873 | -0.09562 ± 0.00054 | 0.09577 | 0.00 | 10017 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | sniips | 0.3873 | -0.07835 ± 0.00047 | 0.07849 | 0.00 | 10017 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | rips | 0.3873 | -0.09226 ± 0.00064 | 0.09248 | 0.00 | 5588 | 0.559 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | snrips | 0.3873 | -0.06613 ± 0.00062 | 0.06642 | 0.00 | 5588 | 0.559 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | dm-pbm | 0.3873 | -0.02962 ± 0.00025 | 0.02973 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.3873 | -0.12353 ± 0.00020 | 0.12355 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.3873 | -0.02410 ± 0.00037 | 0.02438 | 0.00 | 10017 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | cdr | 0.3873 | -0.00599 ± 0.00048 | 0.00764 | 0.73 | 5588 | 0.559 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| reverse | clicks | naive | 0.2291 | +0.11708 ± 0.00019 | 0.11710 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.2291 | -0.22907 ± 0.00000 | 0.22907 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snips | 0.2291 | -0.22907 ± 0.00000 | 0.22907 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | iips | 0.2291 | -0.12434 ± 0.00034 | 0.12439 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | sniips | 0.2291 | -0.12426 ± 0.00031 | 0.12430 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | rips | 0.2291 | -0.12434 ± 0.00034 | 0.12439 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snrips | 0.2291 | -0.12426 ± 0.00031 | 0.12430 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | dm-pbm | 0.2291 | +0.05405 ± 0.00030 | 0.05413 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.2291 | +0.19695 ± 0.00040 | 0.19699 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.2291 | +0.02610 ± 0.00033 | 0.02630 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | cdr | 0.2291 | +0.19923 ± 0.00042 | 0.19928 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | naive | 0.1066 | +0.08164 ± 0.00013 | 0.08165 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.1066 | -0.10657 ± 0.00000 | 0.10657 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snips | 0.1066 | -0.10657 ± 0.00000 | 0.10657 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | iips | 0.1066 | -0.05880 ± 0.00022 | 0.05885 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | sniips | 0.1066 | -0.05876 ± 0.00022 | 0.05880 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | rips | 0.1066 | -0.05880 ± 0.00022 | 0.05885 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snrips | 0.1066 | -0.05876 ± 0.00022 | 0.05880 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | dm-pbm | 0.1066 | +0.03598 ± 0.00018 | 0.03602 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.1066 | +0.10682 ± 0.00027 | 0.10685 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.1066 | +0.01671 ± 0.00019 | 0.01682 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | cdr | 0.1066 | +0.10194 ± 0.00027 | 0.10198 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| wide | clicks | naive | 0.3993 | -0.05319 ± 0.00019 | 0.05322 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.3993 | -0.38509 ± 0.00015 | 0.38509 | 0.00 | 672 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | snips | 0.3993 | -0.18749 ± 0.00204 | 0.18858 | 0.00 | 672 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | iips | 0.3993 | -0.08796 ± 0.00054 | 0.08813 | 0.00 | 9896 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | sniips | 0.3993 | +0.04300 ± 0.00064 | 0.04346 | 0.00 | 9896 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | rips | 0.3993 | -0.10878 ± 0.00065 | 0.10897 | 0.00 | 672 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | snrips | 0.3993 | +0.00068 ± 0.00100 | 0.00999 | 0.94 | 672 | 0.067 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.3993 | +0.03095 ± 0.00028 | 0.03107 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.3993 | +0.00601 ± 0.00025 | 0.00650 | 0.05 |  |  | 100% | 0.05 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.3993 | +0.02757 ± 0.00042 | 0.02788 | 0.00 | 9896 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | cdr | 0.3993 | +0.11722 ± 0.00050 | 0.11733 | 0.00 | 672 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | naive | 0.2267 | -0.03847 ± 0.00013 | 0.03849 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.2267 | -0.22036 ± 0.00011 | 0.22036 | 0.00 | 672 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | snips | 0.2267 | -0.13281 ± 0.00146 | 0.13360 | 0.00 | 672 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | iips | 0.2267 | -0.03716 ± 0.00041 | 0.03738 | 0.00 | 9896 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | sniips | 0.2267 | +0.03686 ± 0.00048 | 0.03716 | 0.00 | 9896 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | rips | 0.2267 | -0.05134 ± 0.00047 | 0.05156 | 0.00 | 672 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | snrips | 0.2267 | -0.00163 ± 0.00070 | 0.00711 | 0.91 | 672 | 0.067 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | dm-pbm | 0.2267 | +0.02062 ± 0.00020 | 0.02072 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.2267 | -0.00276 ± 0.00019 | 0.00333 | 0.15 |  |  | 100% | 0.15 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.2267 | +0.02018 ± 0.00030 | 0.02040 | 0.00 | 9896 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | cdr | 0.2267 | +0.06671 ± 0.00038 | 0.06681 | 0.00 | 672 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |

## nfcorpus · trust · {"policy":"softmax","tau":1.0,"top":10,"by":"rank"}

100 logs of 100000 impressions, 14 s. Propensities: 10000000 lists, 0 mismatched; chi-square 10979 on 11160 df (p 0.888), largest |z| 3.98.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.7353 | 0.2297 | 0.2037 | 2.412 |
| rerank | 0.0% | 0.7241 | 0.2199 | 0.1993 | 2.415 |
| oracle | 0.0% | 0.8452 | 0.3481 | 0.2917 | 2.363 |
| reverse | 0.0% | 0.5996 | 0.0964 | 0.0822 | 2.485 |
| wide | 18.6% | 0.7167 | 0.2137 | 0.1967 | 2.416 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.7353 | -0.04464 ± 0.00020 | 0.04468 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.7353 | -0.01608 ± 0.00982 | 0.09898 | 0.93 | 110 | 0.986 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snips | 0.7353 | -0.00584 ± 0.00760 | 0.07586 | 0.93 | 110 | 0.986 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | iips | 0.7353 | +0.00041 ± 0.00050 | 0.00503 | 0.97 | 11571 | 0.998 | 100% | 0.97 | yes | ok |
| ranking | clicks | sniips | 0.7353 | +0.00053 ± 0.00044 | 0.00442 | 0.97 | 11571 | 0.998 | 100% | 0.97 | yes | ok |
| ranking | clicks | rips | 0.7353 | -0.00175 ± 0.00182 | 0.01822 | 0.96 | 110 | 0.986 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snrips | 0.7353 | -0.00100 ± 0.00173 | 0.01726 | 0.96 | 110 | 0.986 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | dm-pbm | 0.7353 | -0.01169 ± 0.00022 | 0.01189 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.7353 | +0.03223 ± 0.00023 | 0.03231 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.7353 | +0.00038 ± 0.00045 | 0.00451 | 0.97 | 11571 | 0.998 | 100% | 0.97 | yes | ok |
| ranking | clicks | cdr | 0.7353 | +0.00047 ± 0.00175 | 0.01737 | 0.98 | 110 | 0.986 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | naive | 0.2297 | -0.04735 ± 0.00013 | 0.04736 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.2297 | -0.00415 ± 0.00545 | 0.05440 | 0.90 | 110 | 0.986 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snips | 0.2297 | -0.00120 ± 0.00508 | 0.05055 | 0.92 | 110 | 0.986 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | iips | 0.2297 | +0.00009 ± 0.00032 | 0.00317 | 0.94 | 11571 | 0.998 | 100% | 0.94 | yes | ok |
| ranking | satisfied | sniips | 0.2297 | +0.00014 ± 0.00031 | 0.00312 | 0.96 | 11571 | 0.998 | 100% | 0.96 | yes | ok |
| ranking | satisfied | rips | 0.2297 | -0.00062 ± 0.00112 | 0.01118 | 0.87 | 110 | 0.986 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snrips | 0.2297 | -0.00041 ± 0.00110 | 0.01094 | 0.87 | 110 | 0.986 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | dm-pbm | 0.2297 | -0.01759 ± 0.00016 | 0.01766 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.2297 | -0.01264 ± 0.00016 | 0.01275 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.2297 | -0.00002 ± 0.00030 | 0.00303 | 0.95 | 11571 | 0.998 | 100% | 0.95 | yes | ok |
| ranking | satisfied | cdr | 0.2297 | -0.00021 ± 0.00107 | 0.01069 | 0.90 | 110 | 0.986 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| rerank | clicks | naive | 0.7241 | -0.03339 ± 0.00020 | 0.03345 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.7241 | -0.03210 ± 0.03128 | 0.31290 | 0.77 | 19 | 0.983 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.98; 3% of logs usable) |
| rerank | clicks | snips | 0.7241 | +0.01063 ± 0.01933 | 0.19265 | 0.89 | 19 | 0.983 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.98; 3% of logs usable) |
| rerank | clicks | iips | 0.7241 | -0.00054 ± 0.00058 | 0.00579 | 0.98 | 9664 | 1.000 | 100% | 0.98 | yes | ok |
| rerank | clicks | sniips | 0.7241 | -0.00008 ± 0.00052 | 0.00517 | 0.99 | 9664 | 1.000 | 100% | 0.99 | yes | ok |
| rerank | clicks | rips | 0.7241 | +0.00362 ± 0.00485 | 0.04837 | 0.86 | 19 | 0.888 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.89; 3% of logs usable) |
| rerank | clicks | snrips | 0.7241 | +0.00853 ± 0.00503 | 0.05075 | 0.90 | 19 | 0.888 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.89; 3% of logs usable) |
| rerank | clicks | dm-pbm | 0.7241 | -0.00793 ± 0.00027 | 0.00838 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.7241 | +0.02493 ± 0.00023 | 0.02504 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.7241 | -0.00021 ± 0.00050 | 0.00500 | 0.98 | 9664 | 1.000 | 100% | 0.98 | yes | ok |
| rerank | clicks | cdr | 0.7241 | +0.00673 ± 0.00715 | 0.07146 | 0.97 | 19 | 0.888 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.89; 3% of logs usable) |
| rerank | satisfied | naive | 0.2199 | -0.03751 ± 0.00013 | 0.03753 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.2199 | +0.00270 ± 0.01906 | 0.18961 | 0.77 | 19 | 0.983 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.98; 3% of logs usable) |
| rerank | satisfied | snips | 0.2199 | +0.01395 ± 0.01313 | 0.13136 | 0.81 | 19 | 0.983 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.98; 3% of logs usable) |
| rerank | satisfied | iips | 0.2199 | -0.00005 ± 0.00034 | 0.00337 | 0.96 | 9664 | 1.000 | 100% | 0.96 | yes | ok |
| rerank | satisfied | sniips | 0.2199 | +0.00009 ± 0.00034 | 0.00335 | 0.94 | 9664 | 1.000 | 100% | 0.94 | yes | ok |
| rerank | satisfied | rips | 0.2199 | +0.00290 ± 0.00241 | 0.02419 | 0.89 | 19 | 0.888 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.89; 3% of logs usable) |
| rerank | satisfied | snrips | 0.2199 | +0.00500 ± 0.00273 | 0.02765 | 0.90 | 19 | 0.888 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.89; 3% of logs usable) |
| rerank | satisfied | dm-pbm | 0.2199 | -0.00970 ± 0.00017 | 0.00984 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.2199 | -0.01447 ± 0.00016 | 0.01456 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.2199 | -0.00007 ± 0.00029 | 0.00287 | 0.97 | 9664 | 1.000 | 100% | 0.97 | yes | ok |
| rerank | satisfied | cdr | 0.2199 | +0.00427 ± 0.00243 | 0.02455 | 0.97 | 19 | 0.888 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 19 impressions, mean 0.89; 3% of logs usable) |
| oracle | clicks | naive | 0.8452 | -0.15445 ± 0.00020 | 0.15446 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.8452 | -0.01949 ± 0.02823 | 0.28151 | 0.82 | 48 | 1.008 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snips | 0.8452 | -0.01940 ± 0.01698 | 0.17001 | 0.89 | 48 | 1.008 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | iips | 0.8452 | -0.00018 ± 0.00075 | 0.00747 | 0.91 | 11235 | 1.000 | 99% | 0.92 | yes | ok |
| oracle | clicks | sniips | 0.8452 | -0.00043 ± 0.00058 | 0.00580 | 0.94 | 11235 | 1.000 | 99% | 0.94 | yes | ok |
| oracle | clicks | rips | 0.8452 | -0.00212 ± 0.00691 | 0.06880 | 0.84 | 48 | 0.969 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snrips | 0.8452 | -0.00239 ± 0.00503 | 0.05010 | 0.86 | 48 | 0.969 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | dm-pbm | 0.8452 | -0.01678 ± 0.00029 | 0.01702 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.8452 | -0.06211 ± 0.00024 | 0.06215 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.8452 | -0.00041 ± 0.00056 | 0.00561 | 0.96 | 11235 | 1.000 | 99% | 0.96 | yes | ok |
| oracle | clicks | cdr | 0.8452 | -0.00086 ± 0.00700 | 0.06968 | 0.97 | 48 | 0.969 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | naive | 0.3481 | -0.16573 ± 0.00013 | 0.16574 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.3481 | -0.01578 ± 0.02335 | 0.23283 | 0.66 | 48 | 1.008 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snips | 0.3481 | -0.02361 ± 0.01636 | 0.16445 | 0.73 | 48 | 1.008 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | iips | 0.3481 | -0.00009 ± 0.00053 | 0.00529 | 0.92 | 11235 | 1.000 | 99% | 0.92 | yes | ok |
| oracle | satisfied | sniips | 0.3481 | -0.00022 ± 0.00047 | 0.00466 | 0.95 | 11235 | 1.000 | 99% | 0.95 | yes | ok |
| oracle | satisfied | rips | 0.3481 | -0.00321 ± 0.00567 | 0.05655 | 0.79 | 48 | 0.969 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snrips | 0.3481 | -0.00476 ± 0.00369 | 0.03703 | 0.77 | 48 | 0.969 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | dm-pbm | 0.3481 | -0.02512 ± 0.00021 | 0.02520 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.3481 | -0.08869 ± 0.00018 | 0.08871 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.3481 | -0.00023 ± 0.00042 | 0.00418 | 0.95 | 11235 | 1.000 | 99% | 0.95 | yes | ok |
| oracle | satisfied | cdr | 0.3481 | -0.00380 ± 0.00522 | 0.05210 | 0.87 | 48 | 0.969 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| reverse | clicks | naive | 0.5996 | +0.09113 ± 0.00020 | 0.09115 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.5996 | -0.16696 ± 0.43261 | 4.30764 | 0.01 | 0 | 0.433 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.43; 0% of logs usable) |
| reverse | clicks | snips | 0.5996 | -0.58957 ± 0.01000 | 0.59791 | 0.00 | 0 | 0.433 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.43; 0% of logs usable) |
| reverse | clicks | iips | 0.5996 | +0.00037 ± 0.00128 | 0.01270 | 0.94 | 3410 | 0.998 | 99% | 0.94 | yes | ok |
| reverse | clicks | sniips | 0.5996 | +0.00068 ± 0.00104 | 0.01034 | 0.96 | 3410 | 0.998 | 99% | 0.96 | yes | ok |
| reverse | clicks | rips | 0.5996 | -0.05844 ± 0.01033 | 0.11822 | 0.34 | 0 | 0.088 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.09; 0% of logs usable) |
| reverse | clicks | snrips | 0.5996 | -0.06116 ± 0.00923 | 0.11036 | 0.33 | 0 | 0.088 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.09; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.5996 | -0.02412 ± 0.00052 | 0.02468 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.5996 | +0.01853 ± 0.00037 | 0.01888 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.5996 | +0.00036 ± 0.00099 | 0.00987 | 0.95 | 3410 | 0.998 | 99% | 0.95 | yes | ok |
| reverse | clicks | cdr | 0.5996 | +0.01805 ± 0.05086 | 0.50637 | 0.70 | 0 | 0.088 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.09; 0% of logs usable) |
| reverse | satisfied | naive | 0.0964 | +0.08601 ± 0.00013 | 0.08602 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0964 | -0.09636 ± 0.00000 | 0.09636 | 0.00 | 0 | 0.433 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.43; 0% of logs usable) |
| reverse | satisfied | snips | 0.0964 | -0.09636 ± 0.00000 | 0.09636 | 0.00 | 0 | 0.433 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.43; 0% of logs usable) |
| reverse | satisfied | iips | 0.0964 | -0.00037 ± 0.00045 | 0.00452 | 0.96 | 3410 | 0.998 | 99% | 0.96 | yes | ok |
| reverse | satisfied | sniips | 0.0964 | -0.00033 ± 0.00043 | 0.00433 | 0.95 | 3410 | 0.998 | 99% | 0.95 | yes | ok |
| reverse | satisfied | rips | 0.0964 | -0.01528 ± 0.00512 | 0.05320 | 0.37 | 0 | 0.088 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.09; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0964 | -0.01824 ± 0.00435 | 0.04693 | 0.35 | 0 | 0.088 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.09; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0964 | +0.01105 ± 0.00017 | 0.01119 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0964 | +0.02163 ± 0.00016 | 0.02169 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0964 | -0.00039 ± 0.00037 | 0.00370 | 0.96 | 3410 | 0.998 | 99% | 0.96 | yes | ok |
| reverse | satisfied | cdr | 0.0964 | +0.00903 ± 0.00564 | 0.05686 | 0.86 | 0 | 0.088 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.09; 0% of logs usable) |
| wide | clicks | naive | 0.7167 | -0.02604 ± 0.00020 | 0.02612 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.7167 | -0.35090 ± 0.01789 | 0.39348 | 0.23 | 15 | 0.479 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snips | 0.7167 | +0.06883 ± 0.02027 | 0.21312 | 0.88 | 15 | 0.479 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.7167 | -0.03454 ± 0.00057 | 0.03501 | 0.00 | 6396 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | sniips | 0.7167 | +0.01694 ± 0.00059 | 0.01794 | 0.22 | 6396 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | rips | 0.7167 | -0.04700 ± 0.00341 | 0.05797 | 0.40 | 15 | 0.621 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snrips | 0.7167 | +0.02472 ± 0.00545 | 0.05963 | 0.94 | 15 | 0.621 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | dm-pbm | 0.7167 | +0.00479 ± 0.00027 | 0.00550 | 0.08 |  |  | 100% | 0.08 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.7167 | +0.05982 ± 0.00025 | 0.05987 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.7167 | +0.01607 ± 0.00049 | 0.01681 | 0.15 | 6396 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | cdr | 0.7167 | +0.10275 ± 0.00502 | 0.11425 | 0.12 | 15 | 0.621 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | naive | 0.2137 | -0.03131 ± 0.00013 | 0.03133 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.2137 | -0.10306 ± 0.00787 | 0.12943 | 0.45 | 15 | 0.479 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | snips | 0.2137 | +0.02801 ± 0.01386 | 0.14076 | 0.83 | 15 | 0.479 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.2137 | -0.00480 ± 0.00034 | 0.00588 | 0.74 | 6396 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | sniips | 0.2137 | +0.01143 ± 0.00039 | 0.01206 | 0.10 | 6396 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | rips | 0.2137 | -0.00954 ± 0.00147 | 0.01749 | 0.62 | 15 | 0.621 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | snrips | 0.2137 | +0.01278 ± 0.00260 | 0.02885 | 0.94 | 15 | 0.621 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.2137 | +0.00165 ± 0.00017 | 0.00236 | 0.52 |  |  | 100% | 0.52 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.2137 | +0.00621 ± 0.00017 | 0.00644 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.2137 | +0.00844 ± 0.00028 | 0.00889 | 0.14 | 6396 | 0.609 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | cdr | 0.2137 | +0.02994 ± 0.00155 | 0.03366 | 0.28 | 15 | 0.621 | 0% |  |  | biased, as expected: 19% of its slots unsupported |

## nfcorpus · trust · {"policy":"epsilon","epsilon":0.3,"top":10}

100 logs of 100000 impressions, 13 s. Propensities: 10000000 lists, 0 mismatched; chi-square 11319 on 11160 df (p 0.143), largest |z| 4.05.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.7353 | 0.2297 | 0.2037 | 2.412 |
| rerank | 0.0% | 0.7241 | 0.2199 | 0.1993 | 2.415 |
| oracle | 0.0% | 0.8452 | 0.3481 | 0.2917 | 2.363 |
| reverse | 0.0% | 0.5996 | 0.0964 | 0.0822 | 2.485 |
| wide | 18.6% | 0.7167 | 0.2137 | 0.1967 | 2.416 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.7353 | -0.02539 ± 0.00024 | 0.02550 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.7353 | -0.00022 ± 0.00031 | 0.00305 | 0.96 | 70000 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | clicks | snips | 0.7353 | -0.00020 ± 0.00028 | 0.00280 | 0.95 | 70000 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | clicks | iips | 0.7353 | -0.00006 ± 0.00030 | 0.00298 | 0.96 | 72945 | 1.000 | 99% | 0.96 | yes | ok |
| ranking | clicks | sniips | 0.7353 | -0.00008 ± 0.00028 | 0.00278 | 0.95 | 72945 | 1.000 | 99% | 0.95 | yes | ok |
| ranking | clicks | rips | 0.7353 | -0.00010 ± 0.00030 | 0.00300 | 0.96 | 70000 | 1.000 | 99% | 0.96 | yes | ok |
| ranking | clicks | snrips | 0.7353 | -0.00012 ± 0.00028 | 0.00277 | 0.96 | 70000 | 1.000 | 99% | 0.96 | yes | ok |
| ranking | clicks | dm-pbm | 0.7353 | -0.00786 ± 0.00026 | 0.00827 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.7353 | +0.01204 ± 0.00025 | 0.01230 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.7353 | -0.00003 ± 0.00028 | 0.00276 | 0.96 | 72945 | 1.000 | 99% | 0.96 | yes | ok |
| ranking | clicks | cdr | 0.7353 | -0.00007 ± 0.00028 | 0.00275 | 0.97 | 70000 | 1.000 | 99% | 0.97 | yes | ok |
| ranking | satisfied | naive | 0.2297 | -0.02624 ± 0.00016 | 0.02628 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.2297 | -0.00002 ± 0.00019 | 0.00192 | 0.94 | 70000 | 1.000 | 100% | 0.94 | yes | ok |
| ranking | satisfied | snips | 0.2297 | -0.00002 ± 0.00019 | 0.00186 | 0.94 | 70000 | 1.000 | 100% | 0.94 | yes | ok |
| ranking | satisfied | iips | 0.2297 | -0.00001 ± 0.00019 | 0.00192 | 0.94 | 72945 | 1.000 | 99% | 0.94 | yes | ok |
| ranking | satisfied | sniips | 0.2297 | -0.00002 ± 0.00019 | 0.00188 | 0.94 | 72945 | 1.000 | 99% | 0.94 | yes | ok |
| ranking | satisfied | rips | 0.2297 | +0.00002 ± 0.00019 | 0.00190 | 0.95 | 70000 | 1.000 | 99% | 0.95 | yes | ok |
| ranking | satisfied | snrips | 0.2297 | +0.00001 ± 0.00019 | 0.00185 | 0.95 | 70000 | 1.000 | 99% | 0.95 | yes | ok |
| ranking | satisfied | dm-pbm | 0.2297 | -0.01116 ± 0.00017 | 0.01129 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.2297 | -0.00042 ± 0.00017 | 0.00178 | 0.69 |  |  | 100% | 0.69 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.2297 | +0.00004 ± 0.00018 | 0.00180 | 0.93 | 72945 | 1.000 | 99% | 0.93 | yes | ok |
| ranking | satisfied | cdr | 0.2297 | +0.00007 ± 0.00018 | 0.00178 | 0.93 | 70000 | 1.000 | 99% | 0.93 | yes | ok |
| rerank | clicks | naive | 0.7241 | -0.01415 ± 0.00024 | 0.01434 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.7241 | -0.05303 ± 0.08583 | 0.85566 | 0.42 | 2655 | 1.010 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2655 impressions, mean 1.01; 0% of logs usable) |
| rerank | clicks | snips | 0.7241 | -0.09521 ± 0.03405 | 0.35191 | 0.15 | 2655 | 1.010 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2655 impressions, mean 1.01; 0% of logs usable) |
| rerank | clicks | iips | 0.7241 | -0.00142 ± 0.00110 | 0.01102 | 0.92 | 4000 | 1.001 | 100% | 0.92 | yes | ok |
| rerank | clicks | sniips | 0.7241 | -0.00145 ± 0.00092 | 0.00928 | 0.91 | 4000 | 1.001 | 100% | 0.91 | yes | ok |
| rerank | clicks | rips | 0.7241 | -0.00935 ± 0.01845 | 0.18377 | 0.57 | 4 | 0.803 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.80; 0% of logs usable) |
| rerank | clicks | snrips | 0.7241 | -0.02022 ± 0.01195 | 0.12064 | 0.62 | 4 | 0.803 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.80; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.7241 | -0.01070 ± 0.00032 | 0.01117 | 0.01 |  |  | 100% | 0.01 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.7241 | +0.04623 ± 0.00030 | 0.04633 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.7241 | -0.00150 ± 0.00089 | 0.00901 | 0.94 | 4000 | 1.001 | 100% | 0.94 | yes | ok |
| rerank | clicks | cdr | 0.7241 | -0.02305 ± 0.01988 | 0.19914 | 0.98 | 4 | 0.803 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.80; 0% of logs usable) |
| rerank | satisfied | naive | 0.2199 | -0.01640 ± 0.00016 | 0.01647 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.2199 | +0.00173 ± 0.04597 | 0.45738 | 0.19 | 2655 | 1.010 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2655 impressions, mean 1.01; 0% of logs usable) |
| rerank | satisfied | snips | 0.2199 | -0.04042 ± 0.02796 | 0.28114 | 0.09 | 2655 | 1.010 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 2655 impressions, mean 1.01; 0% of logs usable) |
| rerank | satisfied | iips | 0.2199 | -0.00016 ± 0.00051 | 0.00512 | 0.96 | 4000 | 1.001 | 100% | 0.96 | yes | ok |
| rerank | satisfied | sniips | 0.2199 | -0.00013 ± 0.00048 | 0.00478 | 0.96 | 4000 | 1.001 | 100% | 0.96 | yes | ok |
| rerank | satisfied | rips | 0.2199 | -0.01090 ± 0.00517 | 0.05254 | 0.56 | 4 | 0.803 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.80; 0% of logs usable) |
| rerank | satisfied | snrips | 0.2199 | -0.01282 ± 0.00421 | 0.04379 | 0.56 | 4 | 0.803 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.80; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.2199 | -0.00463 ± 0.00019 | 0.00500 | 0.04 |  |  | 100% | 0.04 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.2199 | +0.00178 ± 0.00018 | 0.00253 | 0.39 |  |  | 100% | 0.39 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.2199 | -0.00010 ± 0.00038 | 0.00383 | 1.00 | 4000 | 1.001 | 100% | 1.00 | yes | ok |
| rerank | satisfied | cdr | 0.2199 | -0.01689 ± 0.00658 | 0.06760 | 0.95 | 4 | 0.803 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.80; 0% of logs usable) |
| oracle | clicks | naive | 0.8452 | -0.13520 ± 0.00024 | 0.13523 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.8452 | +0.08301 ± 0.10518 | 1.04981 | 0.35 | 21488 | 1.039 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 21488 impressions, mean 1.04; 0% of logs usable) |
| oracle | clicks | snips | 0.8452 | -0.07810 ± 0.03138 | 0.32182 | 0.27 | 21488 | 1.039 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 21488 impressions, mean 1.04; 0% of logs usable) |
| oracle | clicks | iips | 0.8452 | -0.00007 ± 0.00102 | 0.01017 | 0.99 | 7324 | 0.998 | 99% | 0.99 | yes | ok |
| oracle | clicks | sniips | 0.8452 | +0.00008 ± 0.00088 | 0.00874 | 0.96 | 7324 | 0.998 | 99% | 0.96 | yes | ok |
| oracle | clicks | rips | 0.8452 | +0.00469 ± 0.01240 | 0.12348 | 0.76 | 12 | 0.813 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 12 impressions, mean 0.81; 0% of logs usable) |
| oracle | clicks | snrips | 0.8452 | +0.00448 ± 0.00962 | 0.09586 | 0.76 | 12 | 0.813 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 12 impressions, mean 0.81; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.8452 | -0.01819 ± 0.00035 | 0.01851 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.8452 | -0.07467 ± 0.00028 | 0.07472 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.8452 | +0.00007 ± 0.00086 | 0.00852 | 0.95 | 7324 | 0.998 | 99% | 0.95 | yes | ok |
| oracle | clicks | cdr | 0.8452 | -0.00466 ± 0.01282 | 0.12768 | 0.96 | 12 | 0.813 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 12 impressions, mean 0.81; 0% of logs usable) |
| oracle | satisfied | naive | 0.3481 | -0.14462 ± 0.00016 | 0.14463 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.3481 | +0.08053 ± 0.07651 | 0.76548 | 0.24 | 21488 | 1.039 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 21488 impressions, mean 1.04; 0% of logs usable) |
| oracle | satisfied | snips | 0.3481 | -0.06298 ± 0.03489 | 0.35284 | 0.17 | 21488 | 1.039 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 21488 impressions, mean 1.04; 0% of logs usable) |
| oracle | satisfied | iips | 0.3481 | +0.00008 ± 0.00076 | 0.00759 | 0.96 | 7324 | 0.998 | 99% | 0.96 | yes | ok |
| oracle | satisfied | sniips | 0.3481 | +0.00013 ± 0.00068 | 0.00675 | 0.95 | 7324 | 0.998 | 99% | 0.95 | yes | ok |
| oracle | satisfied | rips | 0.3481 | -0.00295 ± 0.00476 | 0.04740 | 0.65 | 12 | 0.813 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 12 impressions, mean 0.81; 0% of logs usable) |
| oracle | satisfied | snrips | 0.3481 | -0.00170 ± 0.00494 | 0.04922 | 0.64 | 12 | 0.813 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 12 impressions, mean 0.81; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.3481 | -0.02060 ± 0.00025 | 0.02075 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.3481 | -0.07547 ± 0.00021 | 0.07550 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.3481 | +0.00000 ± 0.00064 | 0.00632 | 0.94 | 7324 | 0.998 | 99% | 0.94 | yes | ok |
| oracle | satisfied | cdr | 0.3481 | -0.00942 ± 0.00534 | 0.05400 | 0.85 | 12 | 0.813 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 12 impressions, mean 0.81; 0% of logs usable) |
| reverse | clicks | naive | 0.5996 | +0.11037 ± 0.00024 | 0.11040 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.5996 | +0.23707 ± 0.09932 | 1.01624 | 0.48 | 1 | 1.220 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.22; 0% of logs usable) |
| reverse | clicks | snips | 0.5996 | -0.09707 ± 0.06313 | 0.63556 | 0.18 | 1 | 1.220 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.22; 0% of logs usable) |
| reverse | clicks | iips | 0.5996 | +0.00369 ± 0.00131 | 0.01356 | 0.98 | 2938 | 1.004 | 98% | 0.98 | yes | ok |
| reverse | clicks | sniips | 0.5996 | +0.00350 ± 0.00122 | 0.01262 | 0.94 | 2938 | 1.004 | 98% | 0.94 | yes | ok |
| reverse | clicks | rips | 0.5996 | -0.00064 ± 0.02016 | 0.20059 | 0.66 | 1 | 0.825 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.83; 0% of logs usable) |
| reverse | clicks | snrips | 0.5996 | -0.01026 ± 0.01468 | 0.14646 | 0.63 | 1 | 0.825 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.83; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.5996 | -0.00298 ± 0.00063 | 0.00694 | 0.09 |  |  | 100% | 0.09 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.5996 | +0.08450 ± 0.00048 | 0.08464 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.5996 | +0.00345 ± 0.00119 | 0.01234 | 0.93 | 2938 | 1.004 | 98% | 0.93 | yes | ok |
| reverse | clicks | cdr | 0.5996 | -0.02710 ± 0.02382 | 0.23854 | 0.92 | 1 | 0.825 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.83; 0% of logs usable) |
| reverse | satisfied | naive | 0.0964 | +0.10712 ± 0.00016 | 0.10713 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0964 | -0.00564 ± 0.03234 | 0.32182 | 0.08 | 1 | 1.220 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.22; 0% of logs usable) |
| reverse | satisfied | snips | 0.0964 | -0.02636 ± 0.02754 | 0.27531 | 0.04 | 1 | 1.220 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.22; 0% of logs usable) |
| reverse | satisfied | iips | 0.0964 | +0.00141 ± 0.00049 | 0.00505 | 0.96 | 2938 | 1.004 | 98% | 0.96 | yes | ok |
| reverse | satisfied | sniips | 0.0964 | +0.00138 ± 0.00050 | 0.00512 | 0.96 | 2938 | 1.004 | 98% | 0.96 | yes | ok |
| reverse | satisfied | rips | 0.0964 | +0.00601 ± 0.01094 | 0.10901 | 0.53 | 1 | 0.825 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.83; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0964 | +0.00162 ± 0.00708 | 0.07047 | 0.52 | 1 | 0.825 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.83; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0964 | +0.01999 ± 0.00023 | 0.02013 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0964 | +0.03845 ± 0.00021 | 0.03851 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0964 | +0.00120 ± 0.00046 | 0.00470 | 0.97 | 2938 | 1.004 | 98% | 0.97 | yes | ok |
| reverse | satisfied | cdr | 0.0964 | +0.00053 ± 0.01173 | 0.11671 | 1.00 | 1 | 0.825 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.83; 0% of logs usable) |
| wide | clicks | naive | 0.7167 | -0.00680 ± 0.00024 | 0.00720 | 0.15 |  |  | 100% | 0.15 |  | biased, as expected |
| wide | clicks | ips | 0.7167 | -0.37502 ± 0.06161 | 0.71864 | 0.23 | 3114 | 0.521 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snips | 0.7167 | -0.11810 ± 0.02604 | 0.28478 | 0.06 | 3114 | 0.521 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | iips | 0.7167 | -0.03540 ± 0.00110 | 0.03706 | 0.09 | 2536 | 0.612 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | sniips | 0.7167 | +0.01540 ± 0.00102 | 0.01845 | 0.63 | 2536 | 0.612 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | rips | 0.7167 | -0.06062 ± 0.01208 | 0.13461 | 0.40 | 5 | 0.316 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snrips | 0.7167 | -0.01516 ± 0.00928 | 0.09358 | 0.74 | 5 | 0.316 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.7167 | +0.00244 ± 0.00033 | 0.00410 | 0.24 |  |  | 100% | 0.24 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.7167 | +0.08738 ± 0.00032 | 0.08744 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.7167 | +0.01556 ± 0.00089 | 0.01789 | 0.57 | 2536 | 0.612 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | cdr | 0.7167 | +0.07313 ± 0.01149 | 0.13574 | 0.69 | 5 | 0.316 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | naive | 0.2137 | -0.01020 ± 0.00016 | 0.01032 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.2137 | -0.06697 ± 0.04057 | 0.40919 | 0.12 | 3114 | 0.521 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snips | 0.2137 | -0.06397 ± 0.02512 | 0.25800 | 0.04 | 3114 | 0.521 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.2137 | -0.00449 ± 0.00054 | 0.00702 | 0.89 | 2536 | 0.612 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | sniips | 0.2137 | +0.01165 ± 0.00056 | 0.01291 | 0.54 | 2536 | 0.612 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | rips | 0.2137 | -0.01705 ± 0.00435 | 0.04656 | 0.44 | 5 | 0.316 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.2137 | -0.00539 ± 0.00436 | 0.04372 | 0.53 | 5 | 0.316 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | dm-pbm | 0.2137 | +0.00705 ± 0.00019 | 0.00731 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.2137 | +0.02317 ± 0.00019 | 0.02325 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.2137 | +0.01025 ± 0.00041 | 0.01103 | 0.38 | 2536 | 0.612 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | cdr | 0.2137 | +0.01660 ± 0.00531 | 0.05536 | 0.74 | 5 | 0.316 | 0% |  |  | unbiased though unsupported |

## nfcorpus · trust · {"policy":"shuffle","top":10}

100 logs of 100000 impressions, 12 s. Propensities: 10000000 lists, 0 mismatched; chi-square 11002 on 11160 df (p 0.856), largest |z| 3.76.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.7353 | 0.2297 | 0.2037 | 2.412 |
| rerank | 0.0% | 0.7241 | 0.2199 | 0.1993 | 2.415 |
| oracle | 0.0% | 0.8452 | 0.3481 | 0.2917 | 2.363 |
| reverse | 0.0% | 0.5996 | 0.0964 | 0.0822 | 2.485 |
| wide | 18.6% | 0.7167 | 0.2137 | 0.1967 | 2.416 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.7353 | -0.08420 ± 0.00021 | 0.08422 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.7353 | -0.14264 ± 0.05280 | 0.54439 | 0.76 | 3 | 0.862 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.86; 0% of logs usable) |
| ranking | clicks | snips | 0.7353 | -0.12377 ± 0.04943 | 0.50713 | 0.55 | 3 | 0.862 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.86; 0% of logs usable) |
| ranking | clicks | iips | 0.7353 | -0.00049 ± 0.00088 | 0.00881 | 0.93 | 9883 | 1.000 | 98% | 0.93 | yes | ok |
| ranking | clicks | sniips | 0.7353 | -0.00044 ± 0.00066 | 0.00657 | 0.94 | 9883 | 1.000 | 98% | 0.94 | yes | ok |
| ranking | clicks | rips | 0.7353 | -0.01707 ± 0.00725 | 0.07415 | 0.67 | 3 | 0.851 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.85; 0% of logs usable) |
| ranking | clicks | snrips | 0.7353 | -0.01732 ± 0.00665 | 0.06839 | 0.65 | 3 | 0.851 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.85; 0% of logs usable) |
| ranking | clicks | dm-pbm | 0.7353 | -0.02890 ± 0.00041 | 0.02919 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.7353 | -0.00233 ± 0.00028 | 0.00365 | 0.28 |  |  | 100% | 0.28 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.7353 | -0.00018 ± 0.00064 | 0.00633 | 0.95 | 9883 | 1.000 | 98% | 0.95 | yes | ok |
| ranking | clicks | cdr | 0.7353 | -0.00801 ± 0.00896 | 0.08948 | 0.96 | 3 | 0.851 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.85; 0% of logs usable) |
| ranking | satisfied | naive | 0.2297 | -0.08734 ± 0.00012 | 0.08735 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.2297 | -0.05735 ± 0.02479 | 0.25320 | 0.41 | 3 | 0.862 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.86; 0% of logs usable) |
| ranking | satisfied | snips | 0.2297 | -0.05379 ± 0.02539 | 0.25825 | 0.39 | 3 | 0.862 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.86; 0% of logs usable) |
| ranking | satisfied | iips | 0.2297 | -0.00034 ± 0.00048 | 0.00483 | 0.93 | 9883 | 1.000 | 98% | 0.93 | yes | ok |
| ranking | satisfied | sniips | 0.2297 | -0.00033 ± 0.00046 | 0.00457 | 0.92 | 9883 | 1.000 | 98% | 0.92 | yes | ok |
| ranking | satisfied | rips | 0.2297 | -0.00760 ± 0.00416 | 0.04206 | 0.67 | 3 | 0.851 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.85; 0% of logs usable) |
| ranking | satisfied | snrips | 0.2297 | -0.00720 ± 0.00390 | 0.03949 | 0.63 | 3 | 0.851 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.85; 0% of logs usable) |
| ranking | satisfied | dm-pbm | 0.2297 | -0.02216 ± 0.00020 | 0.02225 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.2297 | -0.02921 ± 0.00019 | 0.02927 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.2297 | -0.00005 ± 0.00039 | 0.00392 | 0.96 | 9883 | 1.000 | 98% | 0.96 | yes | ok |
| ranking | satisfied | cdr | 0.2297 | -0.00559 ± 0.00445 | 0.04465 | 0.92 | 3 | 0.851 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.85; 0% of logs usable) |
| rerank | clicks | naive | 0.7241 | -0.07295 ± 0.00021 | 0.07298 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.7241 | +0.00469 ± 0.05589 | 0.55613 | 0.87 | 3 | 0.940 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| rerank | clicks | snips | 0.7241 | +0.00130 ± 0.04615 | 0.45921 | 0.66 | 3 | 0.940 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| rerank | clicks | iips | 0.7241 | +0.00003 ± 0.00081 | 0.00804 | 0.94 | 9887 | 1.000 | 98% | 0.94 | yes | ok |
| rerank | clicks | sniips | 0.7241 | +0.00050 ± 0.00060 | 0.00601 | 0.96 | 9887 | 1.000 | 98% | 0.97 | yes | ok |
| rerank | clicks | rips | 0.7241 | -0.00314 ± 0.01005 | 0.10001 | 0.64 | 3 | 0.878 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.88; 0% of logs usable) |
| rerank | clicks | snrips | 0.7241 | -0.00287 ± 0.01235 | 0.12289 | 0.60 | 3 | 0.878 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.88; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.7241 | -0.02004 ± 0.00037 | 0.02039 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.7241 | +0.00424 ± 0.00027 | 0.00501 | 0.13 |  |  | 100% | 0.13 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.7241 | +0.00065 ± 0.00058 | 0.00585 | 1.00 | 9887 | 1.000 | 98% | 1.00 | yes | ok |
| rerank | clicks | cdr | 0.7241 | +0.00576 ± 0.01023 | 0.10194 | 0.94 | 3 | 0.878 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.88; 0% of logs usable) |
| rerank | satisfied | naive | 0.2199 | -0.07750 ± 0.00012 | 0.07751 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.2199 | +0.00692 ± 0.02863 | 0.28497 | 0.52 | 3 | 0.940 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| rerank | satisfied | snips | 0.2199 | +0.01028 ± 0.03041 | 0.30272 | 0.48 | 3 | 0.940 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| rerank | satisfied | iips | 0.2199 | +0.00039 ± 0.00043 | 0.00427 | 0.95 | 9887 | 1.000 | 98% | 0.95 | yes | ok |
| rerank | satisfied | sniips | 0.2199 | +0.00057 ± 0.00041 | 0.00408 | 0.95 | 9887 | 1.000 | 98% | 0.95 | yes | ok |
| rerank | satisfied | rips | 0.2199 | -0.00438 ± 0.00512 | 0.05111 | 0.67 | 3 | 0.878 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.88; 0% of logs usable) |
| rerank | satisfied | snrips | 0.2199 | -0.00346 ± 0.00502 | 0.05009 | 0.64 | 3 | 0.878 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.88; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.2199 | -0.01579 ± 0.00021 | 0.01592 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.2199 | -0.03179 ± 0.00019 | 0.03185 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.2199 | +0.00062 ± 0.00035 | 0.00353 | 0.96 | 9887 | 1.000 | 98% | 0.96 | yes | ok |
| rerank | satisfied | cdr | 0.2199 | -0.00231 ± 0.00517 | 0.05147 | 0.94 | 3 | 0.878 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.88; 0% of logs usable) |
| oracle | clicks | naive | 0.8452 | -0.19401 ± 0.00021 | 0.19402 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.8452 | -0.05891 ± 0.06063 | 0.60616 | 0.82 | 3 | 0.928 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| oracle | clicks | snips | 0.8452 | -0.08552 ± 0.05155 | 0.51997 | 0.66 | 3 | 0.928 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| oracle | clicks | iips | 0.8452 | +0.00147 ± 0.00103 | 0.01032 | 0.93 | 9892 | 1.001 | 99% | 0.93 | yes | ok |
| oracle | clicks | sniips | 0.8452 | +0.00043 ± 0.00077 | 0.00770 | 0.96 | 9892 | 1.001 | 99% | 0.96 | yes | ok |
| oracle | clicks | rips | 0.8452 | -0.00637 ± 0.01038 | 0.10346 | 0.68 | 3 | 0.889 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.89; 0% of logs usable) |
| oracle | clicks | snrips | 0.8452 | -0.00838 ± 0.00947 | 0.09459 | 0.68 | 3 | 0.889 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.89; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.8452 | -0.03059 ± 0.00044 | 0.03090 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.8452 | -0.08988 ± 0.00027 | 0.08992 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.8452 | +0.00076 ± 0.00073 | 0.00728 | 0.94 | 9892 | 1.001 | 99% | 0.94 | yes | ok |
| oracle | clicks | cdr | 0.8452 | +0.00100 ± 0.01013 | 0.10079 | 0.95 | 3 | 0.889 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.89; 0% of logs usable) |
| oracle | satisfied | naive | 0.3481 | -0.20573 ± 0.00012 | 0.20573 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.3481 | -0.00942 ± 0.03679 | 0.36621 | 0.63 | 3 | 0.928 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| oracle | satisfied | snips | 0.3481 | -0.00918 ± 0.03610 | 0.35929 | 0.57 | 3 | 0.928 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| oracle | satisfied | iips | 0.3481 | +0.00063 ± 0.00059 | 0.00591 | 0.95 | 9892 | 1.001 | 99% | 0.95 | yes | ok |
| oracle | satisfied | sniips | 0.3481 | +0.00019 ± 0.00052 | 0.00515 | 0.95 | 9892 | 1.001 | 99% | 0.95 | yes | ok |
| oracle | satisfied | rips | 0.3481 | +0.00382 ± 0.00692 | 0.06896 | 0.75 | 3 | 0.889 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.89; 0% of logs usable) |
| oracle | satisfied | snrips | 0.3481 | +0.00199 ± 0.00594 | 0.05914 | 0.73 | 3 | 0.889 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.89; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.3481 | -0.03090 ± 0.00025 | 0.03100 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.3481 | -0.10374 ± 0.00021 | 0.10376 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.3481 | +0.00049 ± 0.00043 | 0.00435 | 0.99 | 9892 | 1.001 | 99% | 0.99 | yes | ok |
| oracle | satisfied | cdr | 0.3481 | +0.00417 ± 0.00634 | 0.06326 | 0.91 | 3 | 0.889 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.89; 0% of logs usable) |
| reverse | clicks | naive | 0.5996 | +0.05157 ± 0.00021 | 0.05161 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.5996 | +0.02942 ± 0.05116 | 0.50989 | 0.81 | 4 | 1.077 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.08; 0% of logs usable) |
| reverse | clicks | snips | 0.5996 | -0.05619 ± 0.03913 | 0.39333 | 0.66 | 4 | 1.077 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.08; 0% of logs usable) |
| reverse | clicks | iips | 0.5996 | +0.00034 ± 0.00083 | 0.00828 | 0.94 | 9887 | 0.998 | 100% | 0.94 | yes | ok |
| reverse | clicks | sniips | 0.5996 | -0.00003 ± 0.00067 | 0.00670 | 0.94 | 9887 | 0.998 | 100% | 0.94 | yes | ok |
| reverse | clicks | rips | 0.5996 | -0.00404 ± 0.00896 | 0.08929 | 0.69 | 4 | 0.955 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.95; 0% of logs usable) |
| reverse | clicks | snrips | 0.5996 | -0.00817 ± 0.00776 | 0.07760 | 0.71 | 4 | 0.955 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.95; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.5996 | -0.01016 ± 0.00036 | 0.01079 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.5996 | +0.07601 ± 0.00030 | 0.07607 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.5996 | -0.00004 ± 0.00066 | 0.00658 | 0.93 | 9887 | 0.998 | 100% | 0.93 | yes | ok |
| reverse | clicks | cdr | 0.5996 | -0.00893 ± 0.01025 | 0.10238 | 0.96 | 4 | 0.955 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.95; 0% of logs usable) |
| reverse | satisfied | naive | 0.0964 | +0.04602 ± 0.00012 | 0.04603 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0964 | -0.01169 ± 0.01926 | 0.19199 | 0.20 | 4 | 1.077 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.08; 0% of logs usable) |
| reverse | satisfied | snips | 0.0964 | -0.02842 ± 0.01569 | 0.15871 | 0.20 | 4 | 1.077 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.08; 0% of logs usable) |
| reverse | satisfied | iips | 0.0964 | +0.00021 ± 0.00031 | 0.00304 | 0.95 | 9887 | 0.998 | 100% | 0.95 | yes | ok |
| reverse | satisfied | sniips | 0.0964 | +0.00017 ± 0.00030 | 0.00303 | 0.95 | 9887 | 0.998 | 100% | 0.95 | yes | ok |
| reverse | satisfied | rips | 0.0964 | -0.00760 ± 0.00224 | 0.02357 | 0.69 | 4 | 0.955 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.95; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0964 | -0.00758 ± 0.00237 | 0.02481 | 0.68 | 4 | 0.955 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.95; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0964 | -0.00234 ± 0.00017 | 0.00286 | 0.24 |  |  | 100% | 0.24 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0964 | +0.01567 ± 0.00015 | 0.01574 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0964 | +0.00013 ± 0.00026 | 0.00257 | 0.99 | 9887 | 0.998 | 100% | 0.99 | yes | ok |
| reverse | satisfied | cdr | 0.0964 | -0.00533 ± 0.00245 | 0.02490 | 0.98 | 4 | 0.955 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.95; 0% of logs usable) |
| wide | clicks | naive | 0.7167 | -0.06560 ± 0.00021 | 0.06563 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.7167 | -0.39015 ± 0.04093 | 0.56397 | 0.55 | 1 | 0.411 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snips | 0.7167 | -0.13641 ± 0.06476 | 0.65867 | 0.23 | 1 | 0.411 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.7167 | -0.03428 ± 0.00079 | 0.03518 | 0.01 | 6099 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | sniips | 0.7167 | +0.01737 ± 0.00064 | 0.01851 | 0.32 | 6099 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | rips | 0.7167 | -0.06065 ± 0.00639 | 0.08791 | 0.42 | 1 | 0.572 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | snrips | 0.7167 | -0.00608 ± 0.00864 | 0.08615 | 0.66 | 1 | 0.572 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.7167 | -0.00969 ± 0.00038 | 0.01042 | 0.01 |  |  | 100% | 0.01 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.7167 | +0.02517 ± 0.00028 | 0.02532 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.7167 | +0.01373 ± 0.00056 | 0.01482 | 0.41 | 6099 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | clicks | cdr | 0.7167 | +0.09879 ± 0.00774 | 0.12524 | 0.54 | 1 | 0.572 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | naive | 0.2137 | -0.07130 ± 0.00012 | 0.07131 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.2137 | -0.11389 ± 0.02236 | 0.24990 | 0.23 | 1 | 0.411 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | snips | 0.2137 | -0.04685 ± 0.03722 | 0.37329 | 0.14 | 1 | 0.411 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.2137 | -0.00412 ± 0.00042 | 0.00587 | 0.89 | 6099 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | sniips | 0.2137 | +0.01229 ± 0.00043 | 0.01302 | 0.20 | 6099 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | rips | 0.2137 | -0.01203 ± 0.00389 | 0.04053 | 0.57 | 1 | 0.572 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.2137 | +0.00853 ± 0.00659 | 0.06610 | 0.78 | 1 | 0.572 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | dm-pbm | 0.2137 | -0.00674 ± 0.00021 | 0.00706 | 0.01 |  |  | 100% | 0.01 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.2137 | -0.01711 ± 0.00020 | 0.01722 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.2137 | +0.00647 ± 0.00035 | 0.00733 | 0.61 | 6099 | 0.610 | 0% |  |  | biased, as expected: 19% of its slots unsupported |
| wide | satisfied | cdr | 0.2137 | +0.02280 ± 0.00346 | 0.04127 | 0.79 | 1 | 0.572 | 0% |  |  | biased, as expected: 19% of its slots unsupported |

## nfcorpus · trust · {"policy":"swap","top":10}

100 logs of 100000 impressions, 12 s. Propensities: 10000000 lists, 0 mismatched; chi-square 3253 on 3224 df (p 0.354), largest |z| 3.42.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.7353 | 0.2297 | 0.2037 | 2.412 |
| rerank | 50.4% | 0.7241 | 0.2199 | 0.1993 | 2.415 |
| oracle | 24.9% | 0.8452 | 0.3481 | 0.2917 | 2.363 |
| reverse | 80.0% | 0.5996 | 0.0964 | 0.0822 | 2.485 |
| wide | 54.7% | 0.7167 | 0.2137 | 0.1967 | 2.416 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.7353 | -0.04992 ± 0.00023 | 0.04997 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.7353 | +0.00038 ± 0.00099 | 0.00989 | 0.99 | 9998 | 1.000 | 100% | 0.99 | yes | ok |
| ranking | clicks | snips | 0.7353 | +0.00055 ± 0.00068 | 0.00676 | 0.97 | 9998 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | clicks | iips | 0.7353 | +0.00044 ± 0.00072 | 0.00720 | 0.92 | 9998 | 1.000 | 97% | 0.92 | yes | ok |
| ranking | clicks | sniips | 0.7353 | +0.00056 ± 0.00056 | 0.00562 | 0.91 | 9998 | 1.000 | 97% | 0.91 | yes | ok |
| ranking | clicks | rips | 0.7353 | +0.00038 ± 0.00099 | 0.00989 | 0.99 | 9998 | 1.000 | 100% | 0.99 | yes | ok |
| ranking | clicks | snrips | 0.7353 | +0.00055 ± 0.00068 | 0.00676 | 0.97 | 9998 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | clicks | dm-pbm | 0.7353 | -0.02824 ± 0.00042 | 0.02855 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.7353 | -0.07162 ± 0.00028 | 0.07167 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.7353 | +0.00061 ± 0.00054 | 0.00538 | 0.91 | 9998 | 1.000 | 97% | 0.91 | yes | ok |
| ranking | clicks | cdr | 0.7353 | +0.00046 ± 0.00066 | 0.00656 | 0.98 | 9998 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | satisfied | naive | 0.2297 | -0.05835 ± 0.00013 | 0.05837 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.2297 | +0.00022 ± 0.00050 | 0.00495 | 0.95 | 9998 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | satisfied | snips | 0.2297 | +0.00027 ± 0.00044 | 0.00441 | 0.97 | 9998 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | satisfied | iips | 0.2297 | +0.00017 ± 0.00038 | 0.00378 | 0.95 | 9998 | 1.000 | 97% | 0.95 | yes | ok |
| ranking | satisfied | sniips | 0.2297 | +0.00020 ± 0.00035 | 0.00347 | 0.94 | 9998 | 1.000 | 97% | 0.94 | yes | ok |
| ranking | satisfied | rips | 0.2297 | +0.00022 ± 0.00050 | 0.00495 | 0.95 | 9998 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | satisfied | snrips | 0.2297 | +0.00027 ± 0.00044 | 0.00441 | 0.97 | 9998 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | satisfied | dm-pbm | 0.2297 | -0.01117 ± 0.00022 | 0.01138 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.2297 | -0.02879 ± 0.00018 | 0.02884 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.2297 | +0.00026 ± 0.00028 | 0.00277 | 0.96 | 9998 | 1.000 | 97% | 0.96 | yes | ok |
| ranking | satisfied | cdr | 0.2297 | +0.00016 ± 0.00039 | 0.00384 | 0.97 | 9998 | 1.000 | 100% | 0.97 | yes | ok |
| rerank | clicks | naive | 0.7241 | -0.03867 ± 0.00023 | 0.03874 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.7241 | -0.66548 ± 0.00028 | 0.66549 | 0.00 | 958 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | snips | 0.7241 | -0.11189 ± 0.00240 | 0.11441 | 0.00 | 958 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | iips | 0.7241 | -0.13561 ± 0.00069 | 0.13578 | 0.00 | 9999 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | sniips | 0.7241 | +0.01609 ± 0.00068 | 0.01744 | 0.33 | 9999 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | rips | 0.7241 | -0.16272 ± 0.00078 | 0.16290 | 0.00 | 958 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | snrips | 0.7241 | -0.01431 ± 0.00110 | 0.01801 | 0.69 | 958 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | dm-pbm | 0.7241 | -0.01272 ± 0.00042 | 0.01339 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.7241 | +0.09202 ± 0.00034 | 0.09208 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.7241 | +0.01901 ± 0.00058 | 0.01987 | 0.05 | 9999 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | clicks | cdr | 0.7241 | +0.28130 ± 0.00066 | 0.28137 | 0.00 | 958 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | naive | 0.2199 | -0.04852 ± 0.00013 | 0.04853 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.2199 | -0.20957 ± 0.00012 | 0.20958 | 0.00 | 958 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | snips | 0.2199 | -0.11227 ± 0.00113 | 0.11283 | 0.00 | 958 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | iips | 0.2199 | -0.03585 ± 0.00039 | 0.03606 | 0.00 | 9999 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | sniips | 0.2199 | +0.01520 ± 0.00043 | 0.01578 | 0.07 | 9999 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | rips | 0.2199 | -0.04882 ± 0.00041 | 0.04899 | 0.00 | 958 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | snrips | 0.2199 | -0.01303 ± 0.00056 | 0.01419 | 0.40 | 958 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | dm-pbm | 0.2199 | -0.00810 ± 0.00021 | 0.00837 | 0.01 |  |  | 100% | 0.01 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.2199 | -0.01629 ± 0.00018 | 0.01639 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.2199 | -0.00785 ± 0.00030 | 0.00841 | 0.25 | 9999 | 0.289 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| rerank | satisfied | cdr | 0.2199 | +0.03777 ± 0.00033 | 0.03791 | 0.00 | 958 | 0.096 | 0% |  |  | biased, as expected: 50% of its slots unsupported |
| oracle | clicks | naive | 0.8452 | -0.15973 ± 0.00023 | 0.15974 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.8452 | -0.47291 ± 0.00068 | 0.47296 | 0.00 | 5564 | 0.556 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | snips | 0.8452 | -0.17609 ± 0.00103 | 0.17639 | 0.00 | 5564 | 0.556 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | iips | 0.8452 | -0.12301 ± 0.00080 | 0.12327 | 0.00 | 10001 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | sniips | 0.8452 | -0.05944 ± 0.00065 | 0.05979 | 0.00 | 10001 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | rips | 0.8452 | -0.13776 ± 0.00090 | 0.13805 | 0.00 | 5564 | 0.556 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | snrips | 0.8452 | -0.05487 ± 0.00087 | 0.05556 | 0.00 | 5564 | 0.556 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | dm-pbm | 0.8452 | -0.02483 ± 0.00043 | 0.02520 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.8452 | -0.11348 ± 0.00027 | 0.11351 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.8452 | -0.01251 ± 0.00060 | 0.01387 | 0.26 | 10001 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | clicks | cdr | 0.8452 | +0.05972 ± 0.00066 | 0.06009 | 0.00 | 5564 | 0.556 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | naive | 0.3481 | -0.17674 ± 0.00013 | 0.17675 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.3481 | -0.24860 ± 0.00032 | 0.24862 | 0.00 | 5564 | 0.556 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | snips | 0.3481 | -0.16925 ± 0.00055 | 0.16934 | 0.00 | 5564 | 0.556 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | iips | 0.3481 | -0.06390 ± 0.00049 | 0.06408 | 0.00 | 10001 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | sniips | 0.3481 | -0.04906 ± 0.00043 | 0.04924 | 0.00 | 10001 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | rips | 0.3481 | -0.06550 ± 0.00052 | 0.06570 | 0.00 | 5564 | 0.556 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | snrips | 0.3481 | -0.04508 ± 0.00051 | 0.04536 | 0.00 | 5564 | 0.556 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | dm-pbm | 0.3481 | -0.01934 ± 0.00027 | 0.01952 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.3481 | -0.08840 ± 0.00020 | 0.08842 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.3481 | -0.01542 ± 0.00035 | 0.01581 | 0.01 | 10001 | 0.658 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| oracle | satisfied | cdr | 0.3481 | +0.00657 ± 0.00040 | 0.00769 | 0.62 | 5564 | 0.556 | 0% |  |  | biased, as expected: 25% of its slots unsupported |
| reverse | clicks | naive | 0.5996 | +0.08585 ± 0.00023 | 0.08588 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.5996 | -0.59957 ± 0.00000 | 0.59957 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snips | 0.5996 | -0.59957 ± 0.00000 | 0.59957 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | iips | 0.5996 | -0.21210 ± 0.00060 | 0.21218 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | sniips | 0.5996 | -0.21235 ± 0.00047 | 0.21241 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | rips | 0.5996 | -0.21210 ± 0.00060 | 0.21218 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snrips | 0.5996 | -0.21235 ± 0.00047 | 0.21241 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | dm-pbm | 0.5996 | +0.05084 ± 0.00047 | 0.05105 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.5996 | +0.47313 ± 0.00042 | 0.47315 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.5996 | +0.05748 ± 0.00051 | 0.05770 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | cdr | 0.5996 | +0.48901 ± 0.00043 | 0.48903 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | naive | 0.0964 | +0.07500 ± 0.00013 | 0.07502 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0964 | -0.09636 ± 0.00000 | 0.09636 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snips | 0.0964 | -0.09636 ± 0.00000 | 0.09636 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | iips | 0.0964 | -0.04809 ± 0.00023 | 0.04814 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | sniips | 0.0964 | -0.04812 ± 0.00023 | 0.04817 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | rips | 0.0964 | -0.04809 ± 0.00023 | 0.04814 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snrips | 0.0964 | -0.04812 ± 0.00023 | 0.04817 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | dm-pbm | 0.0964 | +0.00026 ± 0.00018 | 0.00183 | 0.42 |  |  | 100% | 0.42 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0964 | +0.06079 ± 0.00026 | 0.06084 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0964 | -0.00857 ± 0.00021 | 0.00883 | 0.01 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | cdr | 0.0964 | +0.05305 ± 0.00028 | 0.05312 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| wide | clicks | naive | 0.7167 | -0.03132 ± 0.00023 | 0.03140 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.7167 | -0.67723 ± 0.00023 | 0.67723 | 0.00 | 672 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | snips | 0.7167 | -0.12898 ± 0.00284 | 0.13204 | 0.00 | 672 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | iips | 0.7167 | -0.14098 ± 0.00068 | 0.14114 | 0.00 | 9892 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | sniips | 0.7167 | +0.02890 ± 0.00069 | 0.02970 | 0.00 | 9892 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | rips | 0.7167 | -0.16627 ± 0.00074 | 0.16643 | 0.00 | 672 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | snrips | 0.7167 | -0.00300 ± 0.00117 | 0.01206 | 0.92 | 672 | 0.067 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.7167 | -0.00193 ± 0.00042 | 0.00463 | 0.26 |  |  | 100% | 0.26 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.7167 | +0.08755 ± 0.00033 | 0.08761 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.7167 | +0.03028 ± 0.00057 | 0.03081 | 0.00 | 9892 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | clicks | cdr | 0.7167 | +0.27899 ± 0.00063 | 0.27906 | 0.00 | 672 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | naive | 0.2137 | -0.04231 ± 0.00013 | 0.04233 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.2137 | -0.20804 ± 0.00009 | 0.20804 | 0.00 | 672 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | snips | 0.2137 | -0.12979 ± 0.00132 | 0.13045 | 0.00 | 672 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | iips | 0.2137 | -0.03215 ± 0.00038 | 0.03237 | 0.00 | 9892 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | sniips | 0.2137 | +0.02727 ± 0.00042 | 0.02760 | 0.00 | 9892 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | rips | 0.2137 | -0.04436 ± 0.00040 | 0.04453 | 0.00 | 672 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | snrips | 0.2137 | -0.00371 ± 0.00058 | 0.00689 | 0.94 | 672 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.2137 | +0.00269 ± 0.00022 | 0.00345 | 0.28 |  |  | 100% | 0.28 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.2137 | -0.00337 ± 0.00018 | 0.00382 | 0.11 |  |  | 100% | 0.11 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.2137 | +0.00289 ± 0.00030 | 0.00414 | 0.84 | 9892 | 0.188 | 0% |  |  | biased, as expected: 55% of its slots unsupported |
| wide | satisfied | cdr | 0.2137 | +0.05120 ± 0.00032 | 0.05130 | 0.00 | 672 | 0.067 | 0% |  |  | biased, as expected: 55% of its slots unsupported |

## scifact · pbm · {"policy":"softmax","tau":1.0,"top":10,"by":"rank"}

100 logs of 100000 impressions, 14 s. Propensities: 10000000 lists, 0 mismatched; chi-square 13376 on 13500 df (p 0.774), largest |z| 4.17.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.5802 | 0.4563 | 0.4506 | 2.282 |
| rerank | 0.0% | 0.5283 | 0.4100 | 0.4043 | 2.305 |
| oracle | 0.0% | 0.7047 | 0.5669 | 0.5569 | 2.232 |
| reverse | 0.0% | 0.0937 | 0.0238 | 0.0235 | 2.522 |
| wide | 20.3% | 0.5238 | 0.4061 | 0.4006 | 2.307 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.5802 | -0.21664 ± 0.00016 | 0.21664 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.5802 | -0.00802 ± 0.00768 | 0.07688 | 0.94 | 110 | 0.990 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snips | 0.5802 | -0.00157 ± 0.00542 | 0.05399 | 0.93 | 110 | 0.990 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | iips | 0.5802 | +0.00098 ± 0.00039 | 0.00402 | 0.98 | 11585 | 1.000 | 99% | 0.98 | yes | ok |
| ranking | clicks | sniips | 0.5802 | +0.00074 ± 0.00034 | 0.00346 | 0.95 | 11585 | 1.000 | 99% | 0.95 | yes | ok |
| ranking | clicks | rips | 0.5802 | +0.00228 ± 0.00112 | 0.01135 | 0.93 | 110 | 0.987 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snrips | 0.5802 | +0.00223 ± 0.00110 | 0.01113 | 0.92 | 110 | 0.987 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | dm-pbm | 0.5802 | +0.00873 ± 0.00019 | 0.00893 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.5802 | -0.18230 ± 0.00019 | 0.18231 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.5802 | +0.00044 ± 0.00031 | 0.00315 | 0.96 | 11585 | 1.000 | 99% | 0.96 | yes | ok |
| ranking | clicks | cdr | 0.5802 | +0.00289 ± 0.00116 | 0.01188 | 0.94 | 110 | 0.987 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | naive | 0.4563 | -0.19269 ± 0.00014 | 0.19269 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.4563 | -0.01219 ± 0.00702 | 0.07094 | 0.93 | 110 | 0.990 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snips | 0.4563 | -0.00747 ± 0.00540 | 0.05422 | 0.90 | 110 | 0.990 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | iips | 0.4563 | +0.00103 ± 0.00037 | 0.00379 | 0.94 | 11585 | 1.000 | 99% | 0.94 | yes | ok |
| ranking | satisfied | sniips | 0.4563 | +0.00083 ± 0.00032 | 0.00327 | 0.94 | 11585 | 1.000 | 99% | 0.94 | yes | ok |
| ranking | satisfied | rips | 0.4563 | +0.00172 ± 0.00076 | 0.00774 | 0.94 | 110 | 0.987 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snrips | 0.4563 | +0.00156 ± 0.00074 | 0.00748 | 0.94 | 110 | 0.987 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | dm-pbm | 0.4563 | +0.00995 ± 0.00019 | 0.01012 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.4563 | -0.15220 ± 0.00016 | 0.15221 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.4563 | +0.00058 ± 0.00028 | 0.00284 | 0.93 | 11585 | 1.000 | 99% | 0.93 | yes | ok |
| ranking | satisfied | cdr | 0.4563 | +0.00201 ± 0.00074 | 0.00766 | 0.98 | 110 | 0.987 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| rerank | clicks | naive | 0.5283 | -0.16482 ± 0.00016 | 0.16483 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.5283 | +0.00517 ± 0.01759 | 0.17511 | 0.92 | 16 | 1.078 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 16 impressions, mean 1.08; 4% of logs usable) |
| rerank | clicks | snips | 0.5283 | -0.01023 ± 0.01530 | 0.15260 | 0.85 | 16 | 1.078 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 16 impressions, mean 1.08; 4% of logs usable) |
| rerank | clicks | iips | 0.5283 | +0.00040 ± 0.00041 | 0.00415 | 0.98 | 9465 | 0.999 | 99% | 0.98 | yes | ok |
| rerank | clicks | sniips | 0.5283 | +0.00014 ± 0.00041 | 0.00413 | 0.96 | 9465 | 0.999 | 99% | 0.96 | yes | ok |
| rerank | clicks | rips | 0.5283 | -0.00377 ± 0.00234 | 0.02358 | 0.78 | 16 | 0.991 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 16 impressions, mean 0.99; 4% of logs usable) |
| rerank | clicks | snrips | 0.5283 | -0.00299 ± 0.00268 | 0.02683 | 0.77 | 16 | 0.991 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 16 impressions, mean 0.99; 4% of logs usable) |
| rerank | clicks | dm-pbm | 0.5283 | +0.01492 ± 0.00019 | 0.01504 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.5283 | -0.13290 ± 0.00019 | 0.13292 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.5283 | -0.00004 ± 0.00034 | 0.00338 | 0.97 | 9465 | 0.999 | 99% | 0.97 | yes | ok |
| rerank | clicks | cdr | 0.5283 | -0.00180 ± 0.00260 | 0.02596 | 0.94 | 16 | 0.991 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 16 impressions, mean 0.99; 4% of logs usable) |
| rerank | satisfied | naive | 0.4100 | -0.14631 ± 0.00014 | 0.14632 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.4100 | +0.01647 ± 0.01581 | 0.15812 | 0.90 | 16 | 1.078 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 16 impressions, mean 1.08; 4% of logs usable) |
| rerank | satisfied | snips | 0.4100 | +0.00309 ± 0.01414 | 0.14072 | 0.87 | 16 | 1.078 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 16 impressions, mean 1.08; 4% of logs usable) |
| rerank | satisfied | iips | 0.4100 | +0.00036 ± 0.00035 | 0.00350 | 0.98 | 9465 | 0.999 | 99% | 0.98 | yes | ok |
| rerank | satisfied | sniips | 0.4100 | +0.00015 ± 0.00035 | 0.00345 | 0.95 | 9465 | 0.999 | 99% | 0.95 | yes | ok |
| rerank | satisfied | rips | 0.4100 | -0.00202 ± 0.00120 | 0.01209 | 0.86 | 16 | 0.991 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 16 impressions, mean 0.99; 4% of logs usable) |
| rerank | satisfied | snrips | 0.4100 | -0.00223 ± 0.00110 | 0.01117 | 0.89 | 16 | 0.991 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 16 impressions, mean 0.99; 4% of logs usable) |
| rerank | satisfied | dm-pbm | 0.4100 | +0.01494 ± 0.00017 | 0.01504 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.4100 | -0.10773 ± 0.00016 | 0.10774 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.4100 | -0.00002 ± 0.00030 | 0.00295 | 0.93 | 9465 | 0.999 | 99% | 0.93 | yes | ok |
| rerank | satisfied | cdr | 0.4100 | -0.00031 ± 0.00138 | 0.01369 | 0.95 | 16 | 0.991 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 16 impressions, mean 0.99; 4% of logs usable) |
| oracle | clicks | naive | 0.7047 | -0.34118 ± 0.00016 | 0.34119 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.7047 | -0.01143 ± 0.01025 | 0.10265 | 0.91 | 90 | 0.988 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snips | 0.7047 | -0.00352 ± 0.00593 | 0.05916 | 0.92 | 90 | 0.988 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | iips | 0.7047 | +0.00117 ± 0.00053 | 0.00537 | 0.97 | 11644 | 1.000 | 98% | 0.97 | yes | ok |
| oracle | clicks | sniips | 0.7047 | +0.00039 ± 0.00037 | 0.00369 | 0.96 | 11644 | 1.000 | 98% | 0.96 | yes | ok |
| oracle | clicks | rips | 0.7047 | +0.00175 ± 0.00118 | 0.01184 | 0.93 | 90 | 0.983 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snrips | 0.7047 | +0.00085 ± 0.00114 | 0.01139 | 0.89 | 90 | 0.983 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | dm-pbm | 0.7047 | +0.00236 ± 0.00023 | 0.00327 | 0.24 |  |  | 100% | 0.24 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.7047 | -0.29617 ± 0.00019 | 0.29618 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.7047 | +0.00038 ± 0.00034 | 0.00340 | 0.95 | 11644 | 1.000 | 98% | 0.95 | yes | ok |
| oracle | clicks | cdr | 0.7047 | +0.00138 ± 0.00117 | 0.01170 | 0.90 | 90 | 0.983 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | naive | 0.5669 | -0.30328 ± 0.00014 | 0.30328 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5669 | -0.01315 ± 0.00905 | 0.09105 | 0.91 | 90 | 0.988 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snips | 0.5669 | -0.00703 ± 0.00573 | 0.05743 | 0.91 | 90 | 0.988 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | iips | 0.5669 | +0.00105 ± 0.00051 | 0.00521 | 0.95 | 11644 | 1.000 | 98% | 0.95 | yes | ok |
| oracle | satisfied | sniips | 0.5669 | +0.00038 ± 0.00038 | 0.00378 | 0.92 | 11644 | 1.000 | 98% | 0.92 | yes | ok |
| oracle | satisfied | rips | 0.5669 | +0.00088 ± 0.00072 | 0.00724 | 0.93 | 90 | 0.983 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snrips | 0.5669 | +0.00016 ± 0.00061 | 0.00607 | 0.91 | 90 | 0.983 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | dm-pbm | 0.5669 | +0.00306 ± 0.00024 | 0.00387 | 0.20 |  |  | 100% | 0.20 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5669 | -0.25199 ± 0.00017 | 0.25200 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5669 | +0.00039 ± 0.00035 | 0.00351 | 0.94 | 11644 | 1.000 | 98% | 0.94 | yes | ok |
| oracle | satisfied | cdr | 0.5669 | +0.00043 ± 0.00066 | 0.00662 | 0.93 | 90 | 0.983 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| reverse | clicks | naive | 0.0937 | +0.26980 ± 0.00016 | 0.26981 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.0937 | -0.09372 ± 0.00000 | 0.09372 | 0.00 | 0 | 0.433 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.43; 0% of logs usable) |
| reverse | clicks | snips | 0.0937 | -0.09372 ± 0.00000 | 0.09372 | 0.00 | 0 | 0.433 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.43; 0% of logs usable) |
| reverse | clicks | iips | 0.0937 | -0.00052 ± 0.00047 | 0.00468 | 0.96 | 3411 | 1.001 | 97% | 0.96 | yes | ok |
| reverse | clicks | sniips | 0.0937 | -0.00048 ± 0.00047 | 0.00469 | 0.95 | 3411 | 1.001 | 97% | 0.95 | yes | ok |
| reverse | clicks | rips | 0.0937 | -0.00665 ± 0.00630 | 0.06301 | 0.50 | 0 | 0.433 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.43; 0% of logs usable) |
| reverse | clicks | snrips | 0.0937 | +0.00106 ± 0.01178 | 0.11721 | 0.49 | 0 | 0.433 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.43; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.0937 | +0.08031 ± 0.00023 | 0.08034 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.0937 | +0.02840 ± 0.00020 | 0.02848 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.0937 | -0.00079 ± 0.00047 | 0.00473 | 0.96 | 3411 | 1.001 | 97% | 0.96 | yes | ok |
| reverse | clicks | cdr | 0.0937 | +0.01408 ± 0.01565 | 0.15636 | 0.74 | 0 | 0.433 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.43; 0% of logs usable) |
| reverse | satisfied | naive | 0.0238 | +0.23984 ± 0.00014 | 0.23985 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0238 | -0.02380 ± 0.00000 | 0.02380 | 0.00 | 0 | 0.433 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.43; 0% of logs usable) |
| reverse | satisfied | snips | 0.0238 | -0.02380 ± 0.00000 | 0.02380 | 0.00 | 0 | 0.433 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.43; 0% of logs usable) |
| reverse | satisfied | iips | 0.0238 | -0.00005 ± 0.00023 | 0.00233 | 0.98 | 3411 | 1.001 | 97% | 0.98 | yes | ok |
| reverse | satisfied | sniips | 0.0238 | -0.00003 ± 0.00023 | 0.00233 | 0.97 | 3411 | 1.001 | 97% | 0.97 | yes | ok |
| reverse | satisfied | rips | 0.0238 | -0.00011 ± 0.00357 | 0.03550 | 0.44 | 0 | 0.433 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.43; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0238 | +0.00034 ± 0.00382 | 0.03804 | 0.44 | 0 | 0.433 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.43; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0238 | +0.07128 ± 0.00011 | 0.07129 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0238 | +0.03362 ± 0.00009 | 0.03363 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0238 | -0.00023 ± 0.00022 | 0.00225 | 0.95 | 3411 | 1.001 | 97% | 0.95 | yes | ok |
| reverse | satisfied | cdr | 0.0238 | +0.01352 ± 0.00502 | 0.05173 | 0.48 | 0 | 0.433 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.43; 0% of logs usable) |
| wide | clicks | naive | 0.5238 | -0.16029 ± 0.00016 | 0.16030 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.5238 | -0.32009 ± 0.01062 | 0.33708 | 0.17 | 10 | 0.348 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snips | 0.5238 | +0.05604 ± 0.01853 | 0.19273 | 0.84 | 10 | 0.348 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.5238 | -0.00925 ± 0.00040 | 0.01007 | 0.40 | 6563 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | sniips | 0.5238 | +0.01764 ± 0.00046 | 0.01823 | 0.02 | 6563 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | rips | 0.5238 | -0.01572 ± 0.00207 | 0.02594 | 0.52 | 10 | 0.480 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snrips | 0.5238 | +0.02901 ± 0.00543 | 0.06133 | 0.94 | 10 | 0.480 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | dm-pbm | 0.5238 | +0.03474 ± 0.00019 | 0.03479 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.5238 | -0.08868 ± 0.00020 | 0.08870 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.5238 | +0.02360 ± 0.00034 | 0.02384 | 0.00 | 6563 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | cdr | 0.5238 | +0.06331 ± 0.00197 | 0.06627 | 0.02 | 10 | 0.480 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | naive | 0.4061 | -0.14244 ± 0.00014 | 0.14244 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.4061 | -0.24831 ± 0.00963 | 0.26617 | 0.16 | 10 | 0.348 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | snips | 0.4061 | +0.03568 ± 0.01662 | 0.16913 | 0.87 | 10 | 0.348 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.4061 | -0.00171 ± 0.00035 | 0.00387 | 0.93 | 6563 | 0.640 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | sniips | 0.4061 | +0.01548 ± 0.00039 | 0.01595 | 0.01 | 6563 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | rips | 0.4061 | -0.00461 ± 0.00120 | 0.01278 | 0.76 | 10 | 0.480 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.4061 | +0.02219 ± 0.00286 | 0.03605 | 0.93 | 10 | 0.480 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.4061 | +0.03294 ± 0.00018 | 0.03299 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.4061 | -0.07069 ± 0.00018 | 0.07071 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.4061 | +0.02207 ± 0.00029 | 0.02226 | 0.00 | 6563 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | cdr | 0.4061 | +0.05031 ± 0.00114 | 0.05158 | 0.01 | 10 | 0.480 | 0% |  |  | biased, as expected: 20% of its slots unsupported |

## scifact · pbm · {"policy":"epsilon","epsilon":0.3,"top":10}

100 logs of 100000 impressions, 12 s. Propensities: 10000000 lists, 0 mismatched; chi-square 13689 on 13500 df (p 0.125), largest |z| 4.08.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.5802 | 0.4563 | 0.4506 | 2.282 |
| rerank | 0.0% | 0.5283 | 0.4100 | 0.4043 | 2.305 |
| oracle | 0.0% | 0.7047 | 0.5669 | 0.5569 | 2.232 |
| reverse | 0.0% | 0.0937 | 0.0238 | 0.0235 | 2.522 |
| wide | 20.3% | 0.5238 | 0.4061 | 0.4006 | 2.307 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.5802 | -0.11116 ± 0.00019 | 0.11118 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.5802 | -0.00007 ± 0.00027 | 0.00265 | 0.90 | 70019 | 1.000 | 100% | 0.90 | yes | ok |
| ranking | clicks | snips | 0.5802 | -0.00021 ± 0.00020 | 0.00202 | 0.98 | 70019 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | clicks | iips | 0.5802 | +0.00000 ± 0.00025 | 0.00250 | 0.91 | 72955 | 1.000 | 99% | 0.92 | yes | ok |
| ranking | clicks | sniips | 0.5802 | -0.00013 ± 0.00019 | 0.00192 | 0.97 | 72955 | 1.000 | 99% | 0.97 | yes | ok |
| ranking | clicks | rips | 0.5802 | -0.00001 ± 0.00025 | 0.00253 | 0.92 | 70019 | 1.000 | 100% | 0.92 | yes | ok |
| ranking | clicks | snrips | 0.5802 | -0.00015 ± 0.00019 | 0.00194 | 0.96 | 70019 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | clicks | dm-pbm | 0.5802 | +0.00106 ± 0.00019 | 0.00213 | 0.59 |  |  | 100% | 0.59 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.5802 | -0.04364 ± 0.00018 | 0.04368 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.5802 | -0.00010 ± 0.00019 | 0.00191 | 0.98 | 72955 | 1.000 | 99% | 0.98 | yes | ok |
| ranking | clicks | cdr | 0.5802 | -0.00010 ± 0.00019 | 0.00192 | 0.98 | 70019 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | satisfied | naive | 0.4563 | -0.09892 ± 0.00016 | 0.09893 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.4563 | -0.00009 ± 0.00022 | 0.00219 | 0.94 | 70019 | 1.000 | 100% | 0.94 | yes | ok |
| ranking | satisfied | snips | 0.4563 | -0.00020 ± 0.00018 | 0.00177 | 0.99 | 70019 | 1.000 | 100% | 0.99 | yes | ok |
| ranking | satisfied | iips | 0.4563 | -0.00006 ± 0.00021 | 0.00207 | 0.94 | 72955 | 1.000 | 99% | 0.94 | yes | ok |
| ranking | satisfied | sniips | 0.4563 | -0.00016 ± 0.00017 | 0.00169 | 0.98 | 72955 | 1.000 | 99% | 0.98 | yes | ok |
| ranking | satisfied | rips | 0.4563 | -0.00007 ± 0.00021 | 0.00208 | 0.94 | 70019 | 1.000 | 100% | 0.94 | yes | ok |
| ranking | satisfied | snrips | 0.4563 | -0.00018 ± 0.00017 | 0.00168 | 0.98 | 70019 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | satisfied | dm-pbm | 0.4563 | +0.00555 ± 0.00016 | 0.00579 | 0.01 |  |  | 100% | 0.01 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.4563 | -0.03024 ± 0.00016 | 0.03028 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.4563 | -0.00013 ± 0.00016 | 0.00163 | 0.98 | 72955 | 1.000 | 99% | 0.98 | yes | ok |
| ranking | satisfied | cdr | 0.4563 | -0.00014 ± 0.00016 | 0.00164 | 0.99 | 70019 | 1.000 | 100% | 0.99 | yes | ok |
| rerank | clicks | naive | 0.5283 | -0.05934 ± 0.00019 | 0.05937 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.5283 | +0.03818 ± 0.08018 | 0.79870 | 0.40 | 143 | 0.963 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 143 impressions, mean 0.96; 0% of logs usable) |
| rerank | clicks | snips | 0.5283 | -0.02043 ± 0.03943 | 0.39282 | 0.13 | 143 | 0.963 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 143 impressions, mean 0.96; 0% of logs usable) |
| rerank | clicks | iips | 0.5283 | -0.00122 ± 0.00063 | 0.00639 | 0.94 | 3651 | 0.998 | 97% | 0.95 | yes | ok |
| rerank | clicks | sniips | 0.5283 | -0.00079 ± 0.00071 | 0.00706 | 0.95 | 3651 | 0.998 | 97% | 0.95 | yes | ok |
| rerank | clicks | rips | 0.5283 | +0.01461 ± 0.01610 | 0.16089 | 0.77 | 3 | 0.699 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.70; 0% of logs usable) |
| rerank | clicks | snrips | 0.5283 | +0.01184 ± 0.01261 | 0.12604 | 0.74 | 3 | 0.699 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.70; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.5283 | +0.01104 ± 0.00021 | 0.01123 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.5283 | +0.01082 ± 0.00019 | 0.01099 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.5283 | -0.00095 ± 0.00056 | 0.00564 | 0.95 | 3651 | 0.998 | 97% | 0.95 | yes | ok |
| rerank | clicks | cdr | 0.5283 | +0.01933 ± 0.01703 | 0.17058 | 0.93 | 3 | 0.699 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.70; 0% of logs usable) |
| rerank | satisfied | naive | 0.4100 | -0.05254 ± 0.00016 | 0.05256 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.4100 | -0.00526 ± 0.06407 | 0.63754 | 0.32 | 143 | 0.963 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 143 impressions, mean 0.96; 0% of logs usable) |
| rerank | satisfied | snips | 0.4100 | -0.03756 ± 0.03539 | 0.35409 | 0.13 | 143 | 0.963 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 143 impressions, mean 0.96; 0% of logs usable) |
| rerank | satisfied | iips | 0.4100 | -0.00096 ± 0.00052 | 0.00523 | 0.93 | 3651 | 0.998 | 97% | 0.94 | yes | ok |
| rerank | satisfied | sniips | 0.4100 | -0.00061 ± 0.00059 | 0.00594 | 0.93 | 3651 | 0.998 | 97% | 0.93 | yes | ok |
| rerank | satisfied | rips | 0.4100 | -0.00133 ± 0.00307 | 0.03057 | 0.71 | 3 | 0.699 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.70; 0% of logs usable) |
| rerank | satisfied | snrips | 0.4100 | +0.00048 ± 0.00328 | 0.03259 | 0.72 | 3 | 0.699 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.70; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.4100 | +0.01341 ± 0.00017 | 0.01351 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.4100 | +0.01798 ± 0.00016 | 0.01805 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.4100 | -0.00075 ± 0.00045 | 0.00450 | 0.95 | 3651 | 0.998 | 97% | 0.95 | yes | ok |
| rerank | satisfied | cdr | 0.4100 | +0.00227 ± 0.00517 | 0.05149 | 0.89 | 3 | 0.699 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.70; 0% of logs usable) |
| oracle | clicks | naive | 0.7047 | -0.23571 ± 0.00019 | 0.23572 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.7047 | -0.03224 ± 0.04974 | 0.49599 | 0.13 | 44855 | 0.954 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 44855 impressions, mean 0.95; 0% of logs usable) |
| oracle | clicks | snips | 0.7047 | -0.02442 ± 0.01409 | 0.14233 | 0.10 | 44855 | 0.954 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 44855 impressions, mean 0.95; 0% of logs usable) |
| oracle | clicks | iips | 0.7047 | -0.00018 ± 0.00084 | 0.00831 | 0.94 | 11330 | 1.001 | 98% | 0.94 | yes | ok |
| oracle | clicks | sniips | 0.7047 | -0.00045 ± 0.00047 | 0.00473 | 0.95 | 11330 | 1.001 | 98% | 0.95 | yes | ok |
| oracle | clicks | rips | 0.7047 | -0.00125 ± 0.00230 | 0.02292 | 0.83 | 59 | 0.870 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 59 impressions, mean 0.87; 0% of logs usable) |
| oracle | clicks | snrips | 0.7047 | -0.00134 ± 0.00164 | 0.01634 | 0.78 | 59 | 0.870 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 59 impressions, mean 0.87; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.7047 | +0.00554 ± 0.00021 | 0.00591 | 0.01 |  |  | 100% | 0.01 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.7047 | -0.15596 ± 0.00019 | 0.15597 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.7047 | -0.00039 ± 0.00045 | 0.00450 | 0.96 | 11330 | 1.001 | 98% | 0.96 | yes | ok |
| oracle | clicks | cdr | 0.7047 | -0.00183 ± 0.00229 | 0.02284 | 0.88 | 59 | 0.870 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 59 impressions, mean 0.87; 0% of logs usable) |
| oracle | satisfied | naive | 0.5669 | -0.20951 ± 0.00016 | 0.20951 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5669 | -0.01567 ± 0.04614 | 0.45935 | 0.12 | 44855 | 0.954 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 44855 impressions, mean 0.95; 0% of logs usable) |
| oracle | satisfied | snips | 0.5669 | -0.01792 ± 0.01350 | 0.13551 | 0.06 | 44855 | 0.954 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 44855 impressions, mean 0.95; 0% of logs usable) |
| oracle | satisfied | iips | 0.5669 | -0.00003 ± 0.00075 | 0.00742 | 0.95 | 11330 | 1.001 | 98% | 0.95 | yes | ok |
| oracle | satisfied | sniips | 0.5669 | -0.00025 ± 0.00050 | 0.00498 | 0.97 | 11330 | 1.001 | 98% | 0.97 | yes | ok |
| oracle | satisfied | rips | 0.5669 | +0.00037 ± 0.00127 | 0.01266 | 0.93 | 59 | 0.870 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 59 impressions, mean 0.87; 0% of logs usable) |
| oracle | satisfied | snrips | 0.5669 | +0.00017 ± 0.00103 | 0.01025 | 0.89 | 59 | 0.870 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 59 impressions, mean 0.87; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.5669 | +0.00824 ± 0.00020 | 0.00849 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5669 | -0.12875 ± 0.00017 | 0.12876 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5669 | -0.00020 ± 0.00049 | 0.00487 | 0.94 | 11330 | 1.001 | 98% | 0.94 | yes | ok |
| oracle | satisfied | cdr | 0.5669 | +0.00002 ± 0.00119 | 0.01187 | 0.92 | 59 | 0.870 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 59 impressions, mean 0.87; 0% of logs usable) |
| reverse | clicks | naive | 0.0937 | +0.37528 ± 0.00019 | 0.37528 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.0937 | -0.01308 ± 0.02748 | 0.27378 | 0.08 | 1 | 0.907 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.91; 0% of logs usable) |
| reverse | clicks | snips | 0.0937 | -0.03539 ± 0.02137 | 0.21555 | 0.04 | 1 | 0.907 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.91; 0% of logs usable) |
| reverse | clicks | iips | 0.0937 | +0.00066 ± 0.00055 | 0.00547 | 0.97 | 2935 | 0.998 | 95% | 0.97 | yes | ok |
| reverse | clicks | sniips | 0.0937 | +0.00068 ± 0.00055 | 0.00556 | 0.95 | 2935 | 0.998 | 95% | 0.95 | yes | ok |
| reverse | clicks | rips | 0.0937 | -0.00059 ± 0.00469 | 0.04671 | 0.60 | 1 | 0.583 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.58; 0% of logs usable) |
| reverse | clicks | snrips | 0.0937 | +0.00157 ± 0.00563 | 0.05603 | 0.58 | 1 | 0.583 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.58; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.0937 | +0.10020 ± 0.00030 | 0.10024 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.0937 | +0.05171 ± 0.00025 | 0.05177 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.0937 | +0.00069 ± 0.00061 | 0.00607 | 0.92 | 2935 | 0.998 | 95% | 0.92 | yes | ok |
| reverse | clicks | cdr | 0.0937 | +0.00485 ± 0.00579 | 0.05777 | 0.94 | 1 | 0.583 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.58; 0% of logs usable) |
| reverse | satisfied | naive | 0.0238 | +0.33361 ± 0.00016 | 0.33362 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0238 | -0.00364 ± 0.01418 | 0.14117 | 0.02 | 1 | 0.907 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.91; 0% of logs usable) |
| reverse | satisfied | snips | 0.0238 | -0.00880 ± 0.01114 | 0.11114 | 0.01 | 1 | 0.907 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.91; 0% of logs usable) |
| reverse | satisfied | iips | 0.0238 | +0.00010 ± 0.00030 | 0.00296 | 0.94 | 2935 | 0.998 | 95% | 0.94 | yes | ok |
| reverse | satisfied | sniips | 0.0238 | +0.00011 ± 0.00030 | 0.00294 | 0.95 | 2935 | 0.998 | 95% | 0.95 | yes | ok |
| reverse | satisfied | rips | 0.0238 | +0.00267 ± 0.00311 | 0.03108 | 0.57 | 1 | 0.583 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.58; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0238 | +0.00236 ± 0.00300 | 0.02989 | 0.56 | 1 | 0.583 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.58; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0238 | +0.09412 ± 0.00015 | 0.09413 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0238 | +0.05769 ± 0.00011 | 0.05770 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0238 | +0.00017 ± 0.00032 | 0.00317 | 0.93 | 2935 | 0.998 | 95% | 0.94 | yes | ok |
| reverse | satisfied | cdr | 0.0238 | +0.00678 ± 0.00349 | 0.03539 | 0.83 | 1 | 0.583 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.58; 0% of logs usable) |
| wide | clicks | naive | 0.5238 | -0.05481 ± 0.00019 | 0.05484 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.5238 | -0.33025 ± 0.04225 | 0.53459 | 0.18 | 258 | 0.328 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snips | 0.5238 | -0.06194 ± 0.03124 | 0.31697 | 0.01 | 258 | 0.328 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.5238 | -0.01099 ± 0.00065 | 0.01277 | 0.57 | 2381 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | sniips | 0.5238 | +0.01657 ± 0.00081 | 0.01844 | 0.36 | 2381 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | rips | 0.5238 | -0.01785 ± 0.00355 | 0.03954 | 0.70 | 6 | 0.209 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snrips | 0.5238 | +0.01861 ± 0.00513 | 0.05432 | 0.77 | 6 | 0.209 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.5238 | +0.02592 ± 0.00022 | 0.02601 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.5238 | +0.06353 ± 0.00019 | 0.06356 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.5238 | +0.01722 ± 0.00056 | 0.01810 | 0.09 | 2381 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | cdr | 0.5238 | +0.07335 ± 0.00415 | 0.08420 | 0.31 | 6 | 0.209 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | naive | 0.4061 | -0.04866 ± 0.00016 | 0.04869 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.4061 | -0.24330 ± 0.03714 | 0.44244 | 0.16 | 258 | 0.328 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | snips | 0.4061 | -0.04720 ± 0.02873 | 0.28976 | 0.01 | 258 | 0.328 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.4061 | -0.00303 ± 0.00053 | 0.00606 | 0.90 | 2381 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | sniips | 0.4061 | +0.01475 ± 0.00067 | 0.01617 | 0.39 | 2381 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | rips | 0.4061 | -0.00301 ± 0.00288 | 0.02880 | 0.70 | 6 | 0.209 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.4061 | +0.02103 ± 0.00437 | 0.04834 | 0.83 | 6 | 0.209 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.4061 | +0.02816 ± 0.00017 | 0.02821 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.4061 | +0.06196 ± 0.00017 | 0.06199 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.4061 | +0.01847 ± 0.00044 | 0.01898 | 0.01 | 2381 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | cdr | 0.4061 | +0.06438 ± 0.00335 | 0.07248 | 0.26 | 6 | 0.209 | 0% |  |  | biased, as expected: 20% of its slots unsupported |

## scifact · pbm · {"policy":"shuffle","top":10}

100 logs of 100000 impressions, 13 s. Propensities: 10000000 lists, 0 mismatched; chi-square 13473 on 13500 df (p 0.564), largest |z| 4.22.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.5802 | 0.4563 | 0.4506 | 2.282 |
| rerank | 0.0% | 0.5283 | 0.4100 | 0.4043 | 2.305 |
| oracle | 0.0% | 0.7047 | 0.5669 | 0.5569 | 2.232 |
| reverse | 0.0% | 0.0937 | 0.0238 | 0.0235 | 2.522 |
| wide | 20.3% | 0.5238 | 0.4061 | 0.4006 | 2.307 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.5802 | -0.37039 ± 0.00013 | 0.37040 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.5802 | +0.07907 ± 0.04391 | 0.44402 | 0.88 | 4 | 1.077 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.08; 0% of logs usable) |
| ranking | clicks | snips | 0.5802 | +0.02458 ± 0.03488 | 0.34793 | 0.58 | 4 | 1.077 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.08; 0% of logs usable) |
| ranking | clicks | iips | 0.5802 | +0.00095 ± 0.00065 | 0.00657 | 0.96 | 9895 | 1.002 | 100% | 0.96 | yes | ok |
| ranking | clicks | sniips | 0.5802 | +0.00012 ± 0.00052 | 0.00513 | 0.97 | 9895 | 1.002 | 100% | 0.97 | yes | ok |
| ranking | clicks | rips | 0.5802 | +0.00434 ± 0.00649 | 0.06470 | 0.78 | 4 | 0.959 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.96; 0% of logs usable) |
| ranking | clicks | snrips | 0.5802 | -0.00011 ± 0.00500 | 0.04975 | 0.72 | 4 | 0.959 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.96; 0% of logs usable) |
| ranking | clicks | dm-pbm | 0.5802 | +0.00677 ± 0.00027 | 0.00727 | 0.02 |  |  | 100% | 0.02 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.5802 | -0.26142 ± 0.00019 | 0.26142 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.5802 | -0.00004 ± 0.00039 | 0.00388 | 0.99 | 9895 | 1.002 | 100% | 0.99 | yes | ok |
| ranking | clicks | cdr | 0.5802 | +0.00282 ± 0.00674 | 0.06711 | 0.92 | 4 | 0.959 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.96; 0% of logs usable) |
| ranking | satisfied | naive | 0.4563 | -0.32926 ± 0.00010 | 0.32926 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.4563 | +0.06682 ± 0.03669 | 0.37109 | 0.87 | 4 | 1.077 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.08; 0% of logs usable) |
| ranking | satisfied | snips | 0.4563 | +0.02920 ± 0.03088 | 0.30864 | 0.70 | 4 | 1.077 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.08; 0% of logs usable) |
| ranking | satisfied | iips | 0.4563 | +0.00136 ± 0.00064 | 0.00654 | 0.97 | 9895 | 1.002 | 100% | 0.97 | yes | ok |
| ranking | satisfied | sniips | 0.4563 | +0.00066 ± 0.00057 | 0.00568 | 0.95 | 9895 | 1.002 | 100% | 0.95 | yes | ok |
| ranking | satisfied | rips | 0.4563 | +0.00598 ± 0.00463 | 0.04644 | 0.91 | 4 | 0.959 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.96; 0% of logs usable) |
| ranking | satisfied | snrips | 0.4563 | +0.00347 ± 0.00346 | 0.03458 | 0.86 | 4 | 0.959 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.96; 0% of logs usable) |
| ranking | satisfied | dm-pbm | 0.4563 | -0.00158 ± 0.00028 | 0.00322 | 0.37 |  |  | 100% | 0.37 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.4563 | -0.22119 ± 0.00017 | 0.22119 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.4563 | +0.00054 ± 0.00041 | 0.00415 | 0.97 | 9895 | 1.002 | 100% | 0.97 | yes | ok |
| ranking | satisfied | cdr | 0.4563 | +0.00529 ± 0.00468 | 0.04688 | 0.98 | 4 | 0.959 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.96; 0% of logs usable) |
| rerank | clicks | naive | 0.5283 | -0.31858 ± 0.00013 | 0.31858 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.5283 | +0.00691 ± 0.04380 | 0.43588 | 0.78 | 3 | 1.055 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.06; 0% of logs usable) |
| rerank | clicks | snips | 0.5283 | -0.03235 ± 0.03608 | 0.36044 | 0.61 | 3 | 1.055 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.06; 0% of logs usable) |
| rerank | clicks | iips | 0.5283 | -0.00011 ± 0.00063 | 0.00630 | 0.95 | 9897 | 1.002 | 98% | 0.95 | yes | ok |
| rerank | clicks | sniips | 0.5283 | -0.00025 ± 0.00055 | 0.00545 | 0.95 | 9897 | 1.002 | 98% | 0.95 | yes | ok |
| rerank | clicks | rips | 0.5283 | -0.00870 ± 0.00587 | 0.05906 | 0.74 | 3 | 0.928 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| rerank | clicks | snrips | 0.5283 | -0.01242 ± 0.00431 | 0.04461 | 0.67 | 3 | 0.928 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.5283 | +0.00598 ± 0.00026 | 0.00653 | 0.06 |  |  | 100% | 0.06 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.5283 | -0.21010 ± 0.00019 | 0.21011 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.5283 | -0.00043 ± 0.00037 | 0.00368 | 1.00 | 9897 | 1.002 | 98% | 1.00 | yes | ok |
| rerank | clicks | cdr | 0.5283 | -0.01379 ± 0.00588 | 0.06013 | 0.92 | 3 | 0.928 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| rerank | satisfied | naive | 0.4100 | -0.28288 ± 0.00010 | 0.28288 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.4100 | -0.04102 ± 0.03481 | 0.34875 | 0.68 | 3 | 1.055 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.06; 0% of logs usable) |
| rerank | satisfied | snips | 0.4100 | -0.08550 ± 0.02992 | 0.30978 | 0.60 | 3 | 1.055 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.06; 0% of logs usable) |
| rerank | satisfied | iips | 0.4100 | +0.00070 ± 0.00059 | 0.00589 | 0.96 | 9897 | 1.002 | 98% | 0.96 | yes | ok |
| rerank | satisfied | sniips | 0.4100 | +0.00058 ± 0.00053 | 0.00529 | 0.94 | 9897 | 1.002 | 98% | 0.94 | yes | ok |
| rerank | satisfied | rips | 0.4100 | +0.00306 ± 0.00573 | 0.05705 | 0.84 | 3 | 0.928 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| rerank | satisfied | snrips | 0.4100 | -0.00029 ± 0.00414 | 0.04119 | 0.83 | 3 | 0.928 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.4100 | -0.00185 ± 0.00026 | 0.00316 | 0.39 |  |  | 100% | 0.39 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.4100 | -0.17610 ± 0.00018 | 0.17611 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.4100 | +0.00044 ± 0.00038 | 0.00378 | 0.97 | 9897 | 1.002 | 98% | 0.97 | yes | ok |
| rerank | satisfied | cdr | 0.4100 | -0.00132 ± 0.00556 | 0.05530 | 0.96 | 3 | 0.928 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| oracle | clicks | naive | 0.7047 | -0.49494 ± 0.00013 | 0.49494 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.7047 | +0.02710 ± 0.05089 | 0.50706 | 0.89 | 3 | 1.031 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.03; 0% of logs usable) |
| oracle | clicks | snips | 0.7047 | -0.01910 ± 0.03423 | 0.34108 | 0.52 | 3 | 1.031 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.03; 0% of logs usable) |
| oracle | clicks | iips | 0.7047 | +0.00085 ± 0.00077 | 0.00773 | 0.98 | 9896 | 1.002 | 99% | 0.98 | yes | ok |
| oracle | clicks | sniips | 0.7047 | +0.00014 ± 0.00048 | 0.00482 | 0.96 | 9896 | 1.002 | 99% | 0.96 | yes | ok |
| oracle | clicks | rips | 0.7047 | +0.00136 ± 0.00564 | 0.05616 | 0.83 | 3 | 0.950 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| oracle | clicks | snrips | 0.7047 | -0.00258 ± 0.00393 | 0.03923 | 0.71 | 3 | 0.950 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.7047 | +0.00014 ± 0.00028 | 0.00277 | 0.42 |  |  | 100% | 0.42 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.7047 | -0.37314 ± 0.00020 | 0.37314 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.7047 | +0.00006 ± 0.00041 | 0.00411 | 0.98 | 9896 | 1.002 | 99% | 0.98 | yes | ok |
| oracle | clicks | cdr | 0.7047 | +0.00068 ± 0.00562 | 0.05588 | 0.88 | 3 | 0.950 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| oracle | satisfied | naive | 0.5669 | -0.43985 ± 0.00010 | 0.43985 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5669 | +0.03183 ± 0.04487 | 0.44758 | 0.89 | 3 | 1.031 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.03; 0% of logs usable) |
| oracle | satisfied | snips | 0.5669 | +0.00251 ± 0.03064 | 0.30485 | 0.66 | 3 | 1.031 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.03; 0% of logs usable) |
| oracle | satisfied | iips | 0.5669 | +0.00138 ± 0.00074 | 0.00749 | 0.95 | 9896 | 1.002 | 99% | 0.95 | yes | ok |
| oracle | satisfied | sniips | 0.5669 | +0.00077 ± 0.00052 | 0.00521 | 0.96 | 9896 | 1.002 | 99% | 0.96 | yes | ok |
| oracle | satisfied | rips | 0.5669 | +0.00401 ± 0.00334 | 0.03344 | 0.95 | 3 | 0.950 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| oracle | satisfied | snrips | 0.5669 | +0.00205 ± 0.00218 | 0.02182 | 0.93 | 3 | 0.950 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.5669 | -0.00876 ± 0.00030 | 0.00925 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5669 | -0.31940 ± 0.00019 | 0.31940 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5669 | +0.00075 ± 0.00045 | 0.00450 | 0.96 | 9896 | 1.002 | 99% | 0.96 | yes | ok |
| oracle | satisfied | cdr | 0.5669 | +0.00355 ± 0.00329 | 0.03295 | 0.92 | 3 | 0.950 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| reverse | clicks | naive | 0.0937 | +0.11605 ± 0.00013 | 0.11605 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.0937 | +0.00910 ± 0.02071 | 0.20628 | 0.26 | 3 | 1.040 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.04; 0% of logs usable) |
| reverse | clicks | snips | 0.0937 | -0.00105 ± 0.01807 | 0.17981 | 0.25 | 3 | 1.040 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.04; 0% of logs usable) |
| reverse | clicks | iips | 0.0937 | -0.00027 ± 0.00032 | 0.00317 | 0.97 | 9889 | 1.000 | 100% | 0.97 | yes | ok |
| reverse | clicks | sniips | 0.0937 | -0.00023 ± 0.00031 | 0.00312 | 0.96 | 9889 | 1.000 | 100% | 0.96 | yes | ok |
| reverse | clicks | rips | 0.0937 | +0.00104 ± 0.00541 | 0.05387 | 0.64 | 3 | 0.938 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| reverse | clicks | snrips | 0.0937 | -0.00056 ± 0.00458 | 0.04557 | 0.63 | 3 | 0.938 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.0937 | +0.04211 ± 0.00017 | 0.04215 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.0937 | +0.01379 ± 0.00014 | 0.01386 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.0937 | -0.00003 ± 0.00031 | 0.00310 | 0.95 | 9889 | 1.000 | 100% | 0.95 | yes | ok |
| reverse | clicks | cdr | 0.0937 | +0.00328 ± 0.00558 | 0.05562 | 0.99 | 3 | 0.938 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| reverse | satisfied | naive | 0.0238 | +0.10327 ± 0.00010 | 0.10328 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0238 | +0.00039 ± 0.00825 | 0.08204 | 0.08 | 3 | 1.040 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.04; 0% of logs usable) |
| reverse | satisfied | snips | 0.0238 | -0.00530 ± 0.00645 | 0.06442 | 0.08 | 3 | 1.040 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.04; 0% of logs usable) |
| reverse | satisfied | iips | 0.0238 | -0.00011 ± 0.00014 | 0.00140 | 0.97 | 9889 | 1.000 | 100% | 0.97 | yes | ok |
| reverse | satisfied | sniips | 0.0238 | -0.00009 ± 0.00014 | 0.00138 | 0.97 | 9889 | 1.000 | 100% | 0.97 | yes | ok |
| reverse | satisfied | rips | 0.0238 | +0.00259 ± 0.00430 | 0.04283 | 0.39 | 3 | 0.938 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0238 | +0.00050 ± 0.00276 | 0.02744 | 0.40 | 3 | 0.938 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0238 | +0.03289 ± 0.00007 | 0.03290 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0238 | +0.01790 ± 0.00007 | 0.01791 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0238 | +0.00004 ± 0.00014 | 0.00139 | 0.95 | 9889 | 1.000 | 100% | 0.95 | yes | ok |
| reverse | satisfied | cdr | 0.0238 | +0.00424 ± 0.00413 | 0.04128 | 0.92 | 3 | 0.938 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| wide | clicks | naive | 0.5238 | -0.31404 ± 0.00013 | 0.31405 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.5238 | -0.32423 ± 0.02587 | 0.41396 | 0.45 | 1 | 0.369 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snips | 0.5238 | -0.13448 ± 0.05133 | 0.52813 | 0.22 | 1 | 0.369 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.5238 | -0.00967 ± 0.00064 | 0.01159 | 0.70 | 6410 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | sniips | 0.5238 | +0.01729 ± 0.00062 | 0.01836 | 0.20 | 6410 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | rips | 0.5238 | -0.02566 ± 0.00231 | 0.03447 | 0.56 | 1 | 0.461 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snrips | 0.5238 | +0.00004 ± 0.00340 | 0.03379 | 0.90 | 1 | 0.461 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.5238 | +0.01823 ± 0.00027 | 0.01842 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.5238 | -0.19063 ± 0.00020 | 0.19064 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.5238 | +0.01427 ± 0.00041 | 0.01483 | 0.12 | 6410 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | cdr | 0.5238 | +0.02196 ± 0.00283 | 0.03568 | 0.74 | 1 | 0.461 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | naive | 0.4061 | -0.27900 ± 0.00010 | 0.27901 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.4061 | -0.27604 ± 0.01982 | 0.33923 | 0.34 | 1 | 0.369 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | snips | 0.4061 | -0.16708 ± 0.03667 | 0.40125 | 0.20 | 1 | 0.369 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | iips | 0.4061 | -0.00146 ± 0.00059 | 0.00602 | 0.98 | 6410 | 0.641 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | sniips | 0.4061 | +0.01583 ± 0.00056 | 0.01677 | 0.21 | 6410 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | rips | 0.4061 | -0.00836 ± 0.00203 | 0.02182 | 0.84 | 1 | 0.461 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.4061 | +0.00969 ± 0.00285 | 0.02995 | 0.89 | 1 | 0.461 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | dm-pbm | 0.4061 | +0.00855 ± 0.00026 | 0.00895 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.4061 | -0.16065 ± 0.00018 | 0.16066 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.4061 | +0.01297 ± 0.00039 | 0.01353 | 0.11 | 6410 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | cdr | 0.4061 | +0.01852 ± 0.00243 | 0.03045 | 0.73 | 1 | 0.461 | 0% |  |  | biased, as expected: 20% of its slots unsupported |

## scifact · pbm · {"policy":"swap","top":10}

100 logs of 100000 impressions, 11 s. Propensities: 10000000 lists, 0 mismatched; chi-square 4015 on 3900 df (p 0.097), largest |z| 3.54.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.5802 | 0.4563 | 0.4506 | 2.282 |
| rerank | 54.3% | 0.5283 | 0.4100 | 0.4043 | 2.305 |
| oracle | 5.8% | 0.7047 | 0.5669 | 0.5569 | 2.232 |
| reverse | 80.0% | 0.0937 | 0.0238 | 0.0235 | 2.522 |
| wide | 61.2% | 0.5238 | 0.4061 | 0.4006 | 2.307 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.5802 | -0.34291 ± 0.00014 | 0.34291 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.5802 | -0.00130 ± 0.00078 | 0.00784 | 0.96 | 9983 | 0.998 | 99% | 0.97 | yes | ok |
| ranking | clicks | snips | 0.5802 | -0.00029 ± 0.00052 | 0.00522 | 0.98 | 9983 | 0.998 | 99% | 0.98 | yes | ok |
| ranking | clicks | iips | 0.5802 | -0.00120 ± 0.00069 | 0.00693 | 0.97 | 9983 | 0.999 | 98% | 0.97 | yes | ok |
| ranking | clicks | sniips | 0.5802 | -0.00036 ± 0.00049 | 0.00491 | 0.94 | 9983 | 0.999 | 98% | 0.95 | yes | ok |
| ranking | clicks | rips | 0.5802 | -0.00130 ± 0.00078 | 0.00784 | 0.96 | 9983 | 0.998 | 99% | 0.97 | yes | ok |
| ranking | clicks | snrips | 0.5802 | -0.00029 ± 0.00052 | 0.00522 | 0.98 | 9983 | 0.998 | 99% | 0.98 | yes | ok |
| ranking | clicks | dm-pbm | 0.5802 | +0.00072 ± 0.00028 | 0.00283 | 0.49 |  |  | 100% | 0.49 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.5802 | -0.26828 ± 0.00018 | 0.26829 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.5802 | +0.00002 ± 0.00039 | 0.00385 | 0.95 | 9983 | 0.999 | 98% | 0.95 | yes | ok |
| ranking | clicks | cdr | 0.5802 | -0.00056 ± 0.00056 | 0.00561 | 0.96 | 9983 | 0.998 | 99% | 0.97 | yes | ok |
| ranking | satisfied | naive | 0.4563 | -0.30495 ± 0.00010 | 0.30495 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.4563 | -0.00102 ± 0.00071 | 0.00717 | 0.92 | 9983 | 0.998 | 99% | 0.93 | yes | ok |
| ranking | satisfied | snips | 0.4563 | -0.00023 ± 0.00052 | 0.00522 | 0.94 | 9983 | 0.998 | 99% | 0.94 | yes | ok |
| ranking | satisfied | iips | 0.4563 | -0.00099 ± 0.00067 | 0.00678 | 0.91 | 9983 | 0.999 | 98% | 0.92 | yes | ok |
| ranking | satisfied | sniips | 0.4563 | -0.00029 ± 0.00051 | 0.00505 | 0.93 | 9983 | 0.999 | 98% | 0.94 | yes | ok |
| ranking | satisfied | rips | 0.4563 | -0.00102 ± 0.00071 | 0.00717 | 0.92 | 9983 | 0.998 | 99% | 0.93 | yes | ok |
| ranking | satisfied | snrips | 0.4563 | -0.00023 ± 0.00052 | 0.00522 | 0.94 | 9983 | 0.998 | 99% | 0.94 | yes | ok |
| ranking | satisfied | dm-pbm | 0.4563 | -0.00484 ± 0.00030 | 0.00569 | 0.12 |  |  | 100% | 0.12 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.4563 | -0.22266 ± 0.00018 | 0.22267 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.4563 | +0.00003 ± 0.00041 | 0.00407 | 0.92 | 9983 | 0.999 | 98% | 0.92 | yes | ok |
| ranking | satisfied | cdr | 0.4563 | -0.00044 ± 0.00056 | 0.00556 | 0.94 | 9983 | 0.998 | 99% | 0.94 | yes | ok |
| rerank | clicks | naive | 0.5283 | -0.29109 ± 0.00014 | 0.29110 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.5283 | -0.52304 ± 0.00008 | 0.52304 | 0.00 | 198 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | snips | 0.5283 | -0.26108 ± 0.00346 | 0.26333 | 0.00 | 198 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | iips | 0.5283 | -0.05624 ± 0.00062 | 0.05658 | 0.00 | 8636 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | sniips | 0.5283 | +0.02489 ± 0.00063 | 0.02566 | 0.03 | 8636 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | rips | 0.5283 | -0.08736 ± 0.00062 | 0.08758 | 0.00 | 198 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | snrips | 0.5283 | -0.02660 ± 0.00092 | 0.02814 | 0.22 | 198 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | dm-pbm | 0.5283 | +0.00746 ± 0.00028 | 0.00798 | 0.02 |  |  | 100% | 0.02 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.5283 | -0.17904 ± 0.00026 | 0.17906 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.5283 | +0.00991 ± 0.00039 | 0.01063 | 0.27 | 8636 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | cdr | 0.5283 | +0.06118 ± 0.00050 | 0.06138 | 0.00 | 198 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | naive | 0.4100 | -0.25857 ± 0.00010 | 0.25857 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.4100 | -0.40642 ± 0.00006 | 0.40642 | 0.00 | 198 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | snips | 0.4100 | -0.23205 ± 0.00292 | 0.23386 | 0.00 | 198 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | iips | 0.4100 | -0.03062 ± 0.00058 | 0.03117 | 0.00 | 8636 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | sniips | 0.4100 | +0.02292 ± 0.00057 | 0.02361 | 0.04 | 8636 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | rips | 0.4100 | -0.05471 ± 0.00059 | 0.05502 | 0.00 | 198 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | snrips | 0.4100 | -0.02299 ± 0.00067 | 0.02394 | 0.07 | 198 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | dm-pbm | 0.4100 | +0.00086 ± 0.00029 | 0.00303 | 0.45 |  |  | 100% | 0.45 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.4100 | -0.15851 ± 0.00020 | 0.15852 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.4100 | +0.00879 ± 0.00037 | 0.00953 | 0.37 | 8636 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | cdr | 0.4100 | +0.03896 ± 0.00049 | 0.03925 | 0.00 | 198 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| oracle | clicks | naive | 0.7047 | -0.46746 ± 0.00014 | 0.46746 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.7047 | -0.11508 ± 0.00079 | 0.11535 | 0.00 | 8694 | 0.869 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | snips | 0.7047 | -0.02653 ± 0.00054 | 0.02707 | 0.01 | 8694 | 0.869 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | iips | 0.7047 | -0.01291 ± 0.00075 | 0.01491 | 0.66 | 9984 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | sniips | 0.7047 | -0.00925 ± 0.00048 | 0.01042 | 0.52 | 9984 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | rips | 0.7047 | -0.01540 ± 0.00079 | 0.01728 | 0.56 | 8694 | 0.869 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | snrips | 0.7047 | -0.00877 ± 0.00049 | 0.01004 | 0.57 | 8694 | 0.869 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | dm-pbm | 0.7047 | -0.01001 ± 0.00031 | 0.01047 | 0.01 |  |  | 100% | 0.01 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.7047 | -0.36566 ± 0.00019 | 0.36566 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.7047 | -0.00051 ± 0.00044 | 0.00443 | 0.95 | 9984 | 0.882 | 0% |  |  | unbiased though unsupported |
| oracle | clicks | cdr | 0.7047 | -0.00121 ± 0.00057 | 0.00577 | 0.95 | 8694 | 0.869 | 0% |  |  | unbiased though unsupported |
| oracle | satisfied | naive | 0.5669 | -0.41554 ± 0.00010 | 0.41554 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5669 | -0.09443 ± 0.00072 | 0.09470 | 0.00 | 8694 | 0.869 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | snips | 0.5669 | -0.02348 ± 0.00055 | 0.02411 | 0.01 | 8694 | 0.869 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | iips | 0.5669 | -0.00931 ± 0.00078 | 0.01210 | 0.75 | 9984 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | sniips | 0.5669 | -0.00800 ± 0.00053 | 0.00960 | 0.61 | 9984 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | rips | 0.5669 | -0.00960 ± 0.00078 | 0.01232 | 0.75 | 8694 | 0.869 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | snrips | 0.5669 | -0.00757 ± 0.00053 | 0.00922 | 0.66 | 8694 | 0.869 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | dm-pbm | 0.5669 | -0.01573 ± 0.00035 | 0.01612 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5669 | -0.30911 ± 0.00019 | 0.30912 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5669 | -0.00033 ± 0.00051 | 0.00513 | 0.91 | 9984 | 0.882 | 0% |  |  | unbiased though unsupported |
| oracle | satisfied | cdr | 0.5669 | -0.00037 ± 0.00062 | 0.00614 | 0.92 | 8694 | 0.869 | 0% |  |  | unbiased though unsupported |
| reverse | clicks | naive | 0.0937 | +0.14353 ± 0.00014 | 0.14353 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.0937 | -0.09372 ± 0.00000 | 0.09372 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snips | 0.0937 | -0.09372 ± 0.00000 | 0.09372 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | iips | 0.0937 | -0.04680 ± 0.00022 | 0.04685 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | sniips | 0.0937 | -0.04683 ± 0.00022 | 0.04688 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | rips | 0.0937 | -0.04680 ± 0.00022 | 0.04685 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snrips | 0.0937 | -0.04683 ± 0.00022 | 0.04688 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | dm-pbm | 0.0937 | +0.06930 ± 0.00023 | 0.06934 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.0937 | +0.12579 ± 0.00038 | 0.12584 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.0937 | +0.03116 ± 0.00025 | 0.03126 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | cdr | 0.0937 | +0.12552 ± 0.00038 | 0.12558 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | naive | 0.0238 | +0.12758 ± 0.00010 | 0.12759 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0238 | -0.02380 ± 0.00000 | 0.02380 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snips | 0.0238 | -0.02380 ± 0.00000 | 0.02380 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | iips | 0.0238 | -0.01167 ± 0.00013 | 0.01174 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | sniips | 0.0238 | -0.01167 ± 0.00013 | 0.01175 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | rips | 0.0238 | -0.01167 ± 0.00013 | 0.01174 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snrips | 0.0238 | -0.01167 ± 0.00013 | 0.01175 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | dm-pbm | 0.0238 | +0.05605 ± 0.00011 | 0.05606 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0238 | +0.07023 ± 0.00018 | 0.07025 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0238 | +0.02637 ± 0.00013 | 0.02639 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | cdr | 0.0238 | +0.06222 ± 0.00019 | 0.06225 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| wide | clicks | naive | 0.5238 | -0.28656 ± 0.00014 | 0.28657 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.5238 | -0.51877 ± 0.00007 | 0.51877 | 0.00 | 166 | 0.017 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | snips | 0.5238 | -0.22001 ± 0.00372 | 0.22310 | 0.00 | 166 | 0.017 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | iips | 0.5238 | -0.05636 ± 0.00063 | 0.05671 | 0.00 | 4212 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | sniips | 0.5238 | +0.05075 ± 0.00084 | 0.05143 | 0.00 | 4212 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | rips | 0.5238 | -0.08530 ± 0.00062 | 0.08553 | 0.00 | 166 | 0.017 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | snrips | 0.5238 | -0.01058 ± 0.00105 | 0.01485 | 0.76 | 166 | 0.017 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | dm-pbm | 0.5238 | +0.02096 ± 0.00029 | 0.02116 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.5238 | -0.16287 ± 0.00025 | 0.16289 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.5238 | +0.02439 ± 0.00039 | 0.02469 | 0.00 | 4212 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | cdr | 0.5238 | +0.07694 ± 0.00051 | 0.07711 | 0.00 | 166 | 0.017 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | naive | 0.4061 | -0.25470 ± 0.00010 | 0.25470 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.4061 | -0.40258 ± 0.00006 | 0.40258 | 0.00 | 166 | 0.017 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | snips | 0.4061 | -0.19510 ± 0.00329 | 0.19783 | 0.00 | 166 | 0.017 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | iips | 0.4061 | -0.02862 ± 0.00059 | 0.02922 | 0.00 | 4212 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | sniips | 0.4061 | +0.04483 ± 0.00073 | 0.04542 | 0.00 | 4212 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | rips | 0.4061 | -0.05165 ± 0.00059 | 0.05199 | 0.00 | 166 | 0.017 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | snrips | 0.4061 | -0.00956 ± 0.00074 | 0.01206 | 0.72 | 166 | 0.017 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.4061 | +0.01258 ± 0.00030 | 0.01292 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.4061 | -0.14286 ± 0.00021 | 0.14288 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.4061 | +0.02133 ± 0.00038 | 0.02166 | 0.00 | 4212 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | cdr | 0.4061 | +0.05451 ± 0.00050 | 0.05474 | 0.00 | 166 | 0.017 | 0% |  |  | biased, as expected: 61% of its slots unsupported |

## scifact · cascade · {"policy":"softmax","tau":1.0,"top":10,"by":"rank"}

100 logs of 100000 impressions, 14 s. Propensities: 10000000 lists, 0 mismatched; chi-square 13478 on 13500 df (p 0.553), largest |z| 3.97.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.6860 | 0.5322 | 0.5322 | 2.244 |
| rerank | 0.0% | 0.6834 | 0.5249 | 0.5249 | 2.247 |
| oracle | 0.0% | 0.7106 | 0.5639 | 0.5639 | 2.229 |
| reverse | 0.0% | 0.2165 | 0.0567 | 0.0567 | 2.501 |
| wide | 20.3% | 0.6725 | 0.5145 | 0.5145 | 2.252 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.6860 | -0.04694 ± 0.00017 | 0.04697 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.6860 | -0.01409 ± 0.00867 | 0.08737 | 0.92 | 110 | 0.987 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snips | 0.6860 | -0.00591 ± 0.00419 | 0.04207 | 0.95 | 110 | 0.987 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | iips | 0.6860 | +0.02312 ± 0.00052 | 0.02370 | 0.00 | 11575 | 0.999 | 96% | 0.00 |  | biased, as expected |
| ranking | clicks | sniips | 0.6860 | +0.02306 ± 0.00043 | 0.02345 | 0.00 | 11575 | 0.999 | 96% | 0.00 |  | biased, as expected |
| ranking | clicks | rips | 0.6860 | -0.00193 ± 0.00148 | 0.01487 | 0.88 | 110 | 0.989 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snrips | 0.6860 | -0.00152 ± 0.00152 | 0.01515 | 0.88 | 110 | 0.989 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | dm-pbm | 0.6860 | +0.08098 ± 0.00019 | 0.08100 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.6860 | +0.00087 ± 0.00016 | 0.00184 | 0.47 |  |  | 100% | 0.47 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.6860 | +0.02329 ± 0.00037 | 0.02358 | 0.00 | 11575 | 0.999 | 96% | 0.00 |  | biased, as expected |
| ranking | clicks | cdr | 0.6860 | -0.00188 ± 0.00127 | 0.01282 | 0.90 | 110 | 0.989 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | naive | 0.5322 | -0.06260 ± 0.00017 | 0.06262 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.5322 | -0.01097 ± 0.00721 | 0.07258 | 0.91 | 110 | 0.987 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snips | 0.5322 | -0.00432 ± 0.00457 | 0.04572 | 0.94 | 110 | 0.987 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | iips | 0.5322 | +0.00541 ± 0.00045 | 0.00704 | 0.72 | 11575 | 0.999 | 96% | 0.73 |  | biased, as expected |
| ranking | satisfied | sniips | 0.5322 | +0.00535 ± 0.00040 | 0.00669 | 0.70 | 11575 | 0.999 | 96% | 0.72 |  | biased, as expected |
| ranking | satisfied | rips | 0.5322 | -0.00164 ± 0.00111 | 0.01116 | 0.90 | 110 | 0.989 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snrips | 0.5322 | -0.00138 ± 0.00112 | 0.01121 | 0.90 | 110 | 0.989 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | dm-pbm | 0.5322 | +0.05278 ± 0.00019 | 0.05281 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.5322 | +0.00803 ± 0.00018 | 0.00822 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.5322 | +0.00555 ± 0.00032 | 0.00641 | 0.50 | 11575 | 0.999 | 96% | 0.51 |  | biased, as expected |
| ranking | satisfied | cdr | 0.5322 | -0.00179 ± 0.00080 | 0.00815 | 0.95 | 110 | 0.989 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| rerank | clicks | naive | 0.6834 | -0.04429 ± 0.00017 | 0.04432 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.6834 | +0.02891 ± 0.03814 | 0.38054 | 0.88 | 17 | 1.035 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 1.03; 2% of logs usable) |
| rerank | clicks | snips | 0.6834 | +0.00956 ± 0.01409 | 0.14048 | 0.77 | 17 | 1.035 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 1.03; 2% of logs usable) |
| rerank | clicks | iips | 0.6834 | +0.02179 ± 0.00054 | 0.02244 | 0.02 | 9482 | 0.999 | 98% | 0.02 |  | biased, as expected |
| rerank | clicks | sniips | 0.6834 | +0.02229 ± 0.00051 | 0.02285 | 0.00 | 9482 | 0.999 | 98% | 0.00 |  | biased, as expected |
| rerank | clicks | rips | 0.6834 | -0.00107 ± 0.00742 | 0.07385 | 0.84 | 16 | 0.961 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 16 impressions, mean 0.96; 2% of logs usable) |
| rerank | clicks | snrips | 0.6834 | -0.00358 ± 0.00610 | 0.06083 | 0.83 | 16 | 0.961 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 16 impressions, mean 0.96; 2% of logs usable) |
| rerank | clicks | dm-pbm | 0.6834 | +0.07356 ± 0.00021 | 0.07359 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.6834 | +0.00295 ± 0.00017 | 0.00340 | 0.18 |  |  | 100% | 0.18 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.6834 | +0.02184 ± 0.00043 | 0.02227 | 0.00 | 9482 | 0.999 | 98% | 0.00 |  | biased, as expected |
| rerank | clicks | cdr | 0.6834 | -0.02060 ± 0.01944 | 0.19451 | 0.92 | 16 | 0.961 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 16 impressions, mean 0.96; 2% of logs usable) |
| rerank | satisfied | naive | 0.5249 | -0.05528 ± 0.00017 | 0.05531 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.5249 | -0.01733 ± 0.01954 | 0.19520 | 0.82 | 17 | 1.035 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 1.03; 2% of logs usable) |
| rerank | satisfied | snips | 0.5249 | -0.00694 ± 0.01552 | 0.15455 | 0.80 | 17 | 1.035 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 1.03; 2% of logs usable) |
| rerank | satisfied | iips | 0.5249 | +0.00687 ± 0.00044 | 0.00815 | 0.72 | 9482 | 0.999 | 98% | 0.71 |  | biased, as expected |
| rerank | satisfied | sniips | 0.5249 | +0.00726 ± 0.00046 | 0.00856 | 0.64 | 9482 | 0.999 | 98% | 0.63 |  | biased, as expected |
| rerank | satisfied | rips | 0.5249 | -0.00142 ± 0.00368 | 0.03662 | 0.87 | 16 | 0.961 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 16 impressions, mean 0.96; 2% of logs usable) |
| rerank | satisfied | snrips | 0.5249 | -0.00235 ± 0.00332 | 0.03311 | 0.86 | 16 | 0.961 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 16 impressions, mean 0.96; 2% of logs usable) |
| rerank | satisfied | dm-pbm | 0.5249 | +0.05092 ± 0.00020 | 0.05096 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.5249 | +0.01175 ± 0.00019 | 0.01190 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.5249 | +0.00691 ± 0.00038 | 0.00789 | 0.49 | 9482 | 0.999 | 98% | 0.48 |  | biased, as expected |
| rerank | satisfied | cdr | 0.5249 | -0.01981 ± 0.01838 | 0.18391 | 0.98 | 16 | 0.961 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 16 impressions, mean 0.96; 2% of logs usable) |
| oracle | clicks | naive | 0.7106 | -0.07150 ± 0.00017 | 0.07152 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.7106 | -0.00090 ± 0.00998 | 0.09927 | 0.94 | 89 | 0.996 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snips | 0.7106 | +0.00127 ± 0.00442 | 0.04401 | 0.95 | 89 | 0.996 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | iips | 0.7106 | +0.04601 ± 0.00065 | 0.04646 | 0.00 | 11640 | 1.002 | 97% | 0.00 |  | biased, as expected |
| oracle | clicks | sniips | 0.7106 | +0.04578 ± 0.00044 | 0.04599 | 0.00 | 11640 | 1.002 | 97% | 0.00 |  | biased, as expected |
| oracle | clicks | rips | 0.7106 | -0.00051 ± 0.00161 | 0.01599 | 0.86 | 88 | 0.991 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snrips | 0.7106 | -0.00123 ± 0.00143 | 0.01426 | 0.83 | 88 | 0.991 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | dm-pbm | 0.7106 | +0.11918 ± 0.00020 | 0.11920 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.7106 | -0.00073 ± 0.00016 | 0.00176 | 0.54 |  |  | 100% | 0.54 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.7106 | +0.04582 ± 0.00041 | 0.04601 | 0.00 | 11640 | 1.002 | 97% | 0.00 |  | biased, as expected |
| oracle | clicks | cdr | 0.7106 | -0.00073 ± 0.00148 | 0.01474 | 0.86 | 88 | 0.991 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | naive | 0.5639 | -0.09432 ± 0.00017 | 0.09434 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5639 | -0.00261 ± 0.00899 | 0.08944 | 0.93 | 89 | 0.996 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snips | 0.5639 | -0.00090 ± 0.00564 | 0.05611 | 0.94 | 89 | 0.996 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | iips | 0.5639 | +0.01683 ± 0.00059 | 0.01782 | 0.10 | 11640 | 1.002 | 97% | 0.10 |  | biased, as expected |
| oracle | satisfied | sniips | 0.5639 | +0.01666 ± 0.00040 | 0.01713 | 0.02 | 11640 | 1.002 | 97% | 0.02 |  | biased, as expected |
| oracle | satisfied | rips | 0.5639 | -0.00032 ± 0.00083 | 0.00824 | 0.92 | 88 | 0.991 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snrips | 0.5639 | -0.00059 ± 0.00068 | 0.00681 | 0.91 | 88 | 0.991 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | dm-pbm | 0.5639 | +0.07601 ± 0.00022 | 0.07604 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5639 | +0.00389 ± 0.00018 | 0.00431 | 0.10 |  |  | 100% | 0.10 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5639 | +0.01670 ± 0.00037 | 0.01709 | 0.00 | 11640 | 1.002 | 97% | 0.00 |  | biased, as expected |
| oracle | satisfied | cdr | 0.5639 | -0.00051 ± 0.00069 | 0.00687 | 0.92 | 88 | 0.991 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| reverse | clicks | naive | 0.2165 | +0.42261 ± 0.00017 | 0.42261 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.2165 | -0.21650 ± 0.00000 | 0.21650 | 0.00 | 0 | 0.865 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.87; 0% of logs usable) |
| reverse | clicks | snips | 0.2165 | -0.21650 ± 0.00000 | 0.21650 | 0.00 | 0 | 0.865 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.87; 0% of logs usable) |
| reverse | clicks | iips | 0.2165 | -0.05639 ± 0.00059 | 0.05669 | 0.00 | 3413 | 1.004 | 99% | 0.00 |  | biased, as expected |
| reverse | clicks | sniips | 0.2165 | -0.05638 ± 0.00059 | 0.05669 | 0.00 | 3413 | 1.004 | 99% | 0.00 |  | biased, as expected |
| reverse | clicks | rips | 0.2165 | -0.08573 ± 0.00978 | 0.12973 | 0.20 | 0 | 0.020 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.02; 0% of logs usable) |
| reverse | clicks | snrips | 0.2165 | -0.08900 ± 0.00947 | 0.12958 | 0.20 | 0 | 0.020 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.02; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.2165 | +0.01354 ± 0.00022 | 0.01372 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.2165 | +0.03446 ± 0.00029 | 0.03458 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.2165 | -0.05653 ± 0.00056 | 0.05680 | 0.00 | 3413 | 1.004 | 99% | 0.00 |  | biased, as expected |
| reverse | clicks | cdr | 0.2165 | -0.06583 ± 0.02498 | 0.25710 | 0.86 | 0 | 0.020 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.02; 0% of logs usable) |
| reverse | satisfied | naive | 0.0567 | +0.41288 ± 0.00017 | 0.41289 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0567 | -0.05674 ± 0.00000 | 0.05674 | 0.00 | 0 | 0.865 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.87; 0% of logs usable) |
| reverse | satisfied | snips | 0.0567 | -0.05674 ± 0.00000 | 0.05674 | 0.00 | 0 | 0.865 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.87; 0% of logs usable) |
| reverse | satisfied | iips | 0.0567 | -0.00858 ± 0.00028 | 0.00902 | 0.20 | 3413 | 1.004 | 99% | 0.20 |  | biased, as expected |
| reverse | satisfied | sniips | 0.0567 | -0.00856 ± 0.00028 | 0.00900 | 0.20 | 3413 | 1.004 | 99% | 0.20 |  | biased, as expected |
| reverse | satisfied | rips | 0.0567 | -0.02642 ± 0.00454 | 0.05232 | 0.19 | 0 | 0.020 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.02; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0567 | -0.02698 ± 0.00485 | 0.05525 | 0.16 | 0 | 0.020 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.02; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0567 | +0.06043 ± 0.00010 | 0.06044 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0567 | +0.05893 ± 0.00014 | 0.05895 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0567 | -0.00871 ± 0.00023 | 0.00901 | 0.07 | 3413 | 1.004 | 99% | 0.07 |  | biased, as expected |
| reverse | satisfied | cdr | 0.0567 | -0.01288 ± 0.01082 | 0.10842 | 0.77 | 0 | 0.020 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.02; 0% of logs usable) |
| wide | clicks | naive | 0.6725 | -0.03345 ± 0.00017 | 0.03349 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.6725 | -0.42833 ± 0.01370 | 0.44949 | 0.09 | 11 | 0.351 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snips | 0.6725 | +0.02625 ± 0.01488 | 0.15041 | 0.86 | 11 | 0.351 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.6725 | -0.00206 ± 0.00056 | 0.00592 | 0.92 | 6567 | 0.641 | 0% |  |  | unbiased though unsupported |
| wide | clicks | sniips | 0.6725 | +0.07020 ± 0.00062 | 0.07047 | 0.00 | 6567 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | rips | 0.6725 | -0.03940 ± 0.00660 | 0.07659 | 0.43 | 11 | 0.475 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snrips | 0.6725 | +0.06175 ± 0.00983 | 0.11563 | 0.89 | 11 | 0.475 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | dm-pbm | 0.6725 | +0.15331 ± 0.00023 | 0.15333 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.6725 | +0.05206 ± 0.00016 | 0.05208 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.6725 | +0.11219 ± 0.00043 | 0.11227 | 0.00 | 6567 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | cdr | 0.6725 | +0.05434 ± 0.00482 | 0.07248 | 0.16 | 11 | 0.475 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | naive | 0.5145 | -0.04490 ± 0.00017 | 0.04493 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.5145 | -0.33353 ± 0.00913 | 0.34568 | 0.11 | 11 | 0.351 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | snips | 0.5145 | +0.01772 ± 0.01774 | 0.17741 | 0.80 | 11 | 0.351 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.5145 | +0.00129 ± 0.00045 | 0.00465 | 0.95 | 6567 | 0.641 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | sniips | 0.5145 | +0.05006 ± 0.00054 | 0.05034 | 0.00 | 6567 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | rips | 0.5145 | -0.02006 ± 0.00254 | 0.03227 | 0.64 | 11 | 0.475 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | snrips | 0.5145 | +0.04920 ± 0.00491 | 0.06930 | 0.76 | 11 | 0.475 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.5145 | +0.12052 ± 0.00022 | 0.12054 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.5145 | +0.05490 ± 0.00018 | 0.05493 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.5145 | +0.08529 ± 0.00037 | 0.08537 | 0.00 | 6567 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | cdr | 0.5145 | +0.04691 ± 0.00237 | 0.05250 | 0.12 | 11 | 0.475 | 0% |  |  | biased, as expected: 20% of its slots unsupported |

## scifact · cascade · {"policy":"epsilon","epsilon":0.3,"top":10}

100 logs of 100000 impressions, 12 s. Propensities: 10000000 lists, 0 mismatched; chi-square 13497 on 13500 df (p 0.506), largest |z| 4.23.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.6860 | 0.5322 | 0.5322 | 2.244 |
| rerank | 0.0% | 0.6834 | 0.5249 | 0.5249 | 2.247 |
| oracle | 0.0% | 0.7106 | 0.5639 | 0.5639 | 2.229 |
| reverse | 0.0% | 0.2165 | 0.0567 | 0.0567 | 2.501 |
| wide | 20.3% | 0.6725 | 0.5145 | 0.5145 | 2.252 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.6860 | -0.06973 ± 0.00015 | 0.06974 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.6860 | +0.00062 ± 0.00023 | 0.00235 | 0.95 | 70028 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | clicks | snips | 0.6860 | +0.00036 ± 0.00017 | 0.00177 | 0.96 | 70028 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | clicks | iips | 0.6860 | +0.00309 ± 0.00023 | 0.00384 | 0.64 | 72962 | 1.000 | 99% | 0.65 |  | biased, as expected |
| ranking | clicks | sniips | 0.6860 | +0.00286 ± 0.00018 | 0.00336 | 0.62 | 72962 | 1.000 | 99% | 0.62 |  | biased, as expected |
| ranking | clicks | rips | 0.6860 | +0.00053 ± 0.00023 | 0.00230 | 0.93 | 70028 | 1.000 | 100% | 0.93 | yes | ok |
| ranking | clicks | snrips | 0.6860 | +0.00029 ± 0.00017 | 0.00170 | 0.96 | 70028 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | clicks | dm-pbm | 0.6860 | +0.02295 ± 0.00016 | 0.02301 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.6860 | +0.00115 ± 0.00015 | 0.00192 | 0.54 |  |  | 100% | 0.54 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.6860 | +0.00286 ± 0.00016 | 0.00330 | 0.59 | 72962 | 1.000 | 99% | 0.59 |  | biased, as expected |
| ranking | clicks | cdr | 0.6860 | +0.00029 ± 0.00016 | 0.00158 | 0.97 | 70028 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | satisfied | naive | 0.5322 | -0.07449 ± 0.00015 | 0.07451 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.5322 | +0.00045 ± 0.00023 | 0.00231 | 0.93 | 70028 | 1.000 | 100% | 0.93 | yes | ok |
| ranking | satisfied | snips | 0.5322 | +0.00025 ± 0.00019 | 0.00194 | 0.97 | 70028 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | satisfied | iips | 0.5322 | +0.00096 ± 0.00022 | 0.00243 | 0.88 | 72962 | 1.000 | 99% | 0.88 |  | unbiased here |
| ranking | satisfied | sniips | 0.5322 | +0.00078 ± 0.00019 | 0.00205 | 0.92 | 72962 | 1.000 | 99% | 0.92 |  | unbiased here |
| ranking | satisfied | rips | 0.5322 | +0.00035 ± 0.00023 | 0.00227 | 0.93 | 70028 | 1.000 | 100% | 0.93 | yes | ok |
| ranking | satisfied | snrips | 0.5322 | +0.00017 ± 0.00019 | 0.00189 | 0.96 | 70028 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | satisfied | dm-pbm | 0.5322 | +0.01580 ± 0.00017 | 0.01589 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.5322 | +0.00795 ± 0.00016 | 0.00812 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.5322 | +0.00079 ± 0.00018 | 0.00192 | 0.90 | 72962 | 1.000 | 99% | 0.90 |  | unbiased here |
| ranking | satisfied | cdr | 0.5322 | +0.00017 ± 0.00017 | 0.00174 | 0.96 | 70028 | 1.000 | 100% | 0.96 | yes | ok |
| rerank | clicks | naive | 0.6834 | -0.06707 ± 0.00015 | 0.06709 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.6834 | +0.04470 ± 0.08725 | 0.86923 | 0.50 | 140 | 1.074 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 140 impressions, mean 1.07; 0% of logs usable) |
| rerank | clicks | snips | 0.6834 | -0.09599 ± 0.03300 | 0.34212 | 0.17 | 140 | 1.074 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 140 impressions, mean 1.07; 0% of logs usable) |
| rerank | clicks | iips | 0.6834 | +0.03695 ± 0.00102 | 0.03833 | 0.03 | 3644 | 1.004 | 100% | 0.03 |  | biased, as expected |
| rerank | clicks | sniips | 0.6834 | +0.03633 ± 0.00095 | 0.03754 | 0.01 | 3644 | 1.004 | 100% | 0.01 |  | biased, as expected |
| rerank | clicks | rips | 0.6834 | +0.01172 ± 0.01739 | 0.17340 | 0.78 | 3 | 0.814 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.81; 0% of logs usable) |
| rerank | clicks | snrips | 0.6834 | +0.01064 ± 0.01400 | 0.13969 | 0.78 | 3 | 0.814 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.81; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.6834 | +0.03093 ± 0.00022 | 0.03100 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.6834 | +0.00619 ± 0.00018 | 0.00645 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.6834 | +0.03594 ± 0.00076 | 0.03673 | 0.00 | 3644 | 1.004 | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | cdr | 0.6834 | +0.00076 ± 0.01461 | 0.14532 | 0.97 | 3 | 0.814 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.81; 0% of logs usable) |
| rerank | satisfied | naive | 0.5249 | -0.06717 ± 0.00015 | 0.06719 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.5249 | +0.04112 ± 0.08018 | 0.79888 | 0.40 | 140 | 1.074 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 140 impressions, mean 1.07; 0% of logs usable) |
| rerank | satisfied | snips | 0.5249 | -0.09317 ± 0.03600 | 0.37011 | 0.16 | 140 | 1.074 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 140 impressions, mean 1.07; 0% of logs usable) |
| rerank | satisfied | iips | 0.5249 | +0.00911 ± 0.00074 | 0.01173 | 0.83 | 3644 | 1.004 | 100% | 0.83 |  | biased, as expected |
| rerank | satisfied | sniips | 0.5249 | +0.00865 ± 0.00070 | 0.01109 | 0.84 | 3644 | 1.004 | 100% | 0.84 |  | biased, as expected |
| rerank | satisfied | rips | 0.5249 | +0.01380 ± 0.01325 | 0.13255 | 0.82 | 3 | 0.814 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.81; 0% of logs usable) |
| rerank | satisfied | snrips | 0.5249 | +0.00694 ± 0.00956 | 0.09538 | 0.81 | 3 | 0.814 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.81; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.5249 | +0.01876 ± 0.00017 | 0.01884 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.5249 | +0.01521 ± 0.00018 | 0.01531 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.5249 | +0.00833 ± 0.00055 | 0.00994 | 0.67 | 3644 | 1.004 | 100% | 0.67 |  | biased, as expected |
| rerank | satisfied | cdr | 0.5249 | +0.00590 ± 0.00998 | 0.09944 | 0.98 | 3 | 0.814 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.81; 0% of logs usable) |
| oracle | clicks | naive | 0.7106 | -0.09428 ± 0.00015 | 0.09430 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.7106 | +0.03555 ± 0.04937 | 0.49251 | 0.20 | 41089 | 1.035 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 41089 impressions, mean 1.03; 0% of logs usable) |
| oracle | clicks | snips | 0.7106 | -0.01233 ± 0.01062 | 0.10636 | 0.17 | 41089 | 1.035 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 41089 impressions, mean 1.03; 0% of logs usable) |
| oracle | clicks | iips | 0.7106 | +0.02701 ± 0.00082 | 0.02823 | 0.12 | 11323 | 0.999 | 97% | 0.12 |  | biased, as expected |
| oracle | clicks | sniips | 0.7106 | +0.02767 ± 0.00052 | 0.02814 | 0.00 | 11323 | 0.999 | 97% | 0.00 |  | biased, as expected |
| oracle | clicks | rips | 0.7106 | -0.00045 ± 0.00259 | 0.02573 | 0.90 | 60 | 0.898 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 60 impressions, mean 0.90; 0% of logs usable) |
| oracle | clicks | snrips | 0.7106 | +0.00132 ± 0.00225 | 0.02241 | 0.86 | 60 | 0.898 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 60 impressions, mean 0.90; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.7106 | +0.08305 ± 0.00017 | 0.08307 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.7106 | -0.00156 ± 0.00016 | 0.00224 | 0.43 |  |  | 100% | 0.43 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.7106 | +0.02808 ± 0.00051 | 0.02853 | 0.00 | 11323 | 0.999 | 97% | 0.00 |  | biased, as expected |
| oracle | clicks | cdr | 0.7106 | -0.00007 ± 0.00256 | 0.02547 | 0.96 | 60 | 0.898 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 60 impressions, mean 0.90; 0% of logs usable) |
| oracle | satisfied | naive | 0.5639 | -0.10621 ± 0.00015 | 0.10623 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5639 | +0.02956 ± 0.04464 | 0.44516 | 0.17 | 41089 | 1.035 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 41089 impressions, mean 1.03; 0% of logs usable) |
| oracle | satisfied | snips | 0.5639 | -0.01345 ± 0.01296 | 0.12962 | 0.02 | 41089 | 1.035 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 41089 impressions, mean 1.03; 0% of logs usable) |
| oracle | satisfied | iips | 0.5639 | +0.01372 ± 0.00076 | 0.01568 | 0.49 | 11323 | 0.999 | 97% | 0.47 |  | biased, as expected |
| oracle | satisfied | sniips | 0.5639 | +0.01426 ± 0.00052 | 0.01518 | 0.19 | 11323 | 0.999 | 97% | 0.20 |  | biased, as expected |
| oracle | satisfied | rips | 0.5639 | -0.00085 ± 0.00092 | 0.00918 | 0.94 | 60 | 0.898 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 60 impressions, mean 0.90; 0% of logs usable) |
| oracle | satisfied | snrips | 0.5639 | -0.00011 ± 0.00072 | 0.00718 | 0.92 | 60 | 0.898 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 60 impressions, mean 0.90; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.5639 | +0.05861 ± 0.00019 | 0.05864 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5639 | +0.00293 ± 0.00017 | 0.00339 | 0.17 |  |  | 100% | 0.17 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5639 | +0.01460 ± 0.00051 | 0.01547 | 0.15 | 11323 | 0.999 | 97% | 0.15 |  | biased, as expected |
| oracle | satisfied | cdr | 0.5639 | -0.00017 ± 0.00071 | 0.00707 | 0.94 | 60 | 0.898 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 60 impressions, mean 0.90; 0% of logs usable) |
| reverse | clicks | naive | 0.2165 | +0.39983 ± 0.00015 | 0.39983 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.2165 | -0.01490 ± 0.04298 | 0.42792 | 0.19 | 1 | 1.048 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.05; 0% of logs usable) |
| reverse | clicks | snips | 0.2165 | -0.10066 ± 0.02708 | 0.28768 | 0.13 | 1 | 1.048 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.05; 0% of logs usable) |
| reverse | clicks | iips | 0.2165 | -0.02381 ± 0.00091 | 0.02547 | 0.14 | 2940 | 0.998 | 99% | 0.14 |  | biased, as expected |
| reverse | clicks | sniips | 0.2165 | -0.02369 ± 0.00088 | 0.02527 | 0.15 | 2940 | 0.998 | 99% | 0.15 |  | biased, as expected |
| reverse | clicks | rips | 0.2165 | +0.01236 ± 0.02564 | 0.25543 | 0.50 | 1 | 0.798 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.80; 0% of logs usable) |
| reverse | clicks | snrips | 0.2165 | -0.01799 ± 0.01598 | 0.15997 | 0.45 | 1 | 0.798 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.80; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.2165 | +0.09585 ± 0.00033 | 0.09590 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.2165 | +0.05062 ± 0.00035 | 0.05074 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.2165 | -0.02373 ± 0.00085 | 0.02518 | 0.16 | 2940 | 0.998 | 99% | 0.16 |  | biased, as expected |
| reverse | clicks | cdr | 0.2165 | +0.01539 ± 0.02318 | 0.23112 | 0.95 | 1 | 0.798 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.80; 0% of logs usable) |
| reverse | satisfied | naive | 0.0567 | +0.40099 ± 0.00015 | 0.40099 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0567 | +0.00374 ± 0.02406 | 0.23942 | 0.06 | 1 | 1.048 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.05; 0% of logs usable) |
| reverse | satisfied | snips | 0.0567 | -0.02840 ± 0.01293 | 0.13178 | 0.05 | 1 | 1.048 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.05; 0% of logs usable) |
| reverse | satisfied | iips | 0.0567 | -0.00421 ± 0.00043 | 0.00603 | 0.81 | 2940 | 0.998 | 99% | 0.81 |  | biased, as expected |
| reverse | satisfied | sniips | 0.0567 | -0.00418 ± 0.00042 | 0.00594 | 0.82 | 2940 | 0.998 | 99% | 0.82 |  | biased, as expected |
| reverse | satisfied | rips | 0.0567 | +0.00870 ± 0.01476 | 0.14711 | 0.43 | 1 | 0.798 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.80; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0567 | -0.00256 ± 0.00843 | 0.08389 | 0.43 | 1 | 0.798 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.80; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0567 | +0.11025 ± 0.00016 | 0.11026 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0567 | +0.07612 ± 0.00018 | 0.07614 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0567 | -0.00424 ± 0.00039 | 0.00578 | 0.81 | 2940 | 0.998 | 99% | 0.81 |  | biased, as expected |
| reverse | satisfied | cdr | 0.0567 | +0.01032 ± 0.00940 | 0.09412 | 0.97 | 1 | 0.798 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.80; 0% of logs usable) |
| wide | clicks | naive | 0.6725 | -0.05623 ± 0.00015 | 0.05625 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.6725 | -0.44846 ± 0.05082 | 0.67586 | 0.18 | 262 | 0.348 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snips | 0.6725 | -0.17895 ± 0.02603 | 0.31482 | 0.02 | 262 | 0.348 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | iips | 0.6725 | +0.00880 ± 0.00093 | 0.01274 | 0.85 | 2381 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | sniips | 0.6725 | +0.08520 ± 0.00105 | 0.08584 | 0.00 | 2381 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | rips | 0.6725 | -0.02615 ± 0.01277 | 0.12971 | 0.63 | 5 | 0.205 | 0% |  |  | unbiased though unsupported |
| wide | clicks | snrips | 0.6725 | +0.07819 ± 0.01799 | 0.19536 | 0.92 | 5 | 0.205 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | dm-pbm | 0.6725 | +0.08215 ± 0.00024 | 0.08219 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.6725 | +0.05743 ± 0.00017 | 0.05745 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.6725 | +0.09464 ± 0.00067 | 0.09487 | 0.00 | 2381 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | cdr | 0.6725 | +0.06711 ± 0.01124 | 0.13040 | 0.62 | 5 | 0.205 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | naive | 0.5145 | -0.05679 ± 0.00015 | 0.05681 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.5145 | -0.34162 ± 0.04543 | 0.56661 | 0.14 | 262 | 0.348 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | snips | 0.5145 | -0.17699 ± 0.02603 | 0.31371 | 0.03 | 262 | 0.348 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | iips | 0.5145 | +0.00276 ± 0.00071 | 0.00761 | 0.93 | 2381 | 0.641 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | sniips | 0.5145 | +0.05149 ± 0.00082 | 0.05213 | 0.00 | 2381 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | rips | 0.5145 | -0.01271 ± 0.00768 | 0.07748 | 0.72 | 5 | 0.205 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.5145 | +0.05651 ± 0.01444 | 0.15436 | 0.93 | 5 | 0.205 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | dm-pbm | 0.5145 | +0.06778 ± 0.00019 | 0.06781 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.5145 | +0.06034 ± 0.00017 | 0.06037 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.5145 | +0.06643 ± 0.00050 | 0.06662 | 0.00 | 2381 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | cdr | 0.5145 | +0.05532 ± 0.00450 | 0.07114 | 0.52 | 5 | 0.205 | 0% |  |  | biased, as expected: 20% of its slots unsupported |

## scifact · cascade · {"policy":"shuffle","top":10}

100 logs of 100000 impressions, 12 s. Propensities: 10000000 lists, 0 mismatched; chi-square 13672 on 13500 df (p 0.147), largest |z| 4.59.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.6860 | 0.5322 | 0.5322 | 2.244 |
| rerank | 0.0% | 0.6834 | 0.5249 | 0.5249 | 2.247 |
| oracle | 0.0% | 0.7106 | 0.5639 | 0.5639 | 2.229 |
| reverse | 0.0% | 0.2165 | 0.0567 | 0.0567 | 2.501 |
| wide | 20.3% | 0.6725 | 0.5145 | 0.5145 | 2.252 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.6860 | -0.23315 ± 0.00014 | 0.23315 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.6860 | -0.03589 ± 0.04556 | 0.45477 | 0.89 | 3 | 1.019 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.02; 0% of logs usable) |
| ranking | clicks | snips | 0.6860 | -0.04543 ± 0.03257 | 0.32725 | 0.55 | 3 | 1.019 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.02; 0% of logs usable) |
| ranking | clicks | iips | 0.6860 | +0.06195 ± 0.00085 | 0.06252 | 0.00 | 9894 | 1.002 | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | sniips | 0.6860 | +0.06136 ± 0.00069 | 0.06174 | 0.00 | 9894 | 1.002 | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | rips | 0.6860 | +0.00032 ± 0.00891 | 0.08866 | 0.65 | 3 | 1.020 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.02; 0% of logs usable) |
| ranking | clicks | snrips | 0.6860 | -0.00396 ± 0.00930 | 0.09258 | 0.62 | 3 | 1.020 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.02; 0% of logs usable) |
| ranking | clicks | dm-pbm | 0.6860 | +0.12796 ± 0.00023 | 0.12798 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.6860 | -0.00724 ± 0.00018 | 0.00747 | 0.01 |  |  | 100% | 0.01 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.6860 | +0.06132 ± 0.00055 | 0.06156 | 0.00 | 9894 | 1.002 | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | cdr | 0.6860 | -0.00792 ± 0.00786 | 0.07860 | 0.79 | 3 | 1.020 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.02; 0% of logs usable) |
| ranking | satisfied | naive | 0.5322 | -0.24880 ± 0.00014 | 0.24880 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.5322 | -0.00907 ± 0.04359 | 0.43381 | 0.80 | 3 | 1.019 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.02; 0% of logs usable) |
| ranking | satisfied | snips | 0.5322 | -0.01914 ± 0.03523 | 0.35105 | 0.54 | 3 | 1.019 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.02; 0% of logs usable) |
| ranking | satisfied | iips | 0.5322 | +0.01505 ± 0.00075 | 0.01681 | 0.42 | 9894 | 1.002 | 100% | 0.42 |  | biased, as expected |
| ranking | satisfied | sniips | 0.5322 | +0.01457 ± 0.00063 | 0.01585 | 0.34 | 9894 | 1.002 | 100% | 0.34 |  | biased, as expected |
| ranking | satisfied | rips | 0.5322 | +0.01029 ± 0.00788 | 0.07907 | 0.79 | 3 | 1.020 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.02; 0% of logs usable) |
| ranking | satisfied | snrips | 0.5322 | +0.00760 ± 0.00850 | 0.08496 | 0.69 | 3 | 1.020 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.02; 0% of logs usable) |
| ranking | satisfied | dm-pbm | 0.5322 | +0.06703 ± 0.00023 | 0.06707 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.5322 | -0.00269 ± 0.00021 | 0.00340 | 0.28 |  |  | 100% | 0.28 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.5322 | +0.01450 ± 0.00050 | 0.01532 | 0.13 | 9894 | 1.002 | 100% | 0.13 |  | biased, as expected |
| ranking | satisfied | cdr | 0.5322 | +0.00416 ± 0.00640 | 0.06380 | 0.96 | 3 | 1.020 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.02; 0% of logs usable) |
| rerank | clicks | naive | 0.6834 | -0.23050 ± 0.00014 | 0.23050 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.6834 | +0.00305 ± 0.03865 | 0.38456 | 0.95 | 3 | 0.995 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.99; 0% of logs usable) |
| rerank | clicks | snips | 0.6834 | +0.01440 ± 0.02730 | 0.27206 | 0.62 | 3 | 0.995 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.99; 0% of logs usable) |
| rerank | clicks | iips | 0.6834 | +0.05750 ± 0.00086 | 0.05813 | 0.00 | 9891 | 1.001 | 99% | 0.00 |  | biased, as expected |
| rerank | clicks | sniips | 0.6834 | +0.05768 ± 0.00071 | 0.05811 | 0.00 | 9891 | 1.001 | 99% | 0.00 |  | biased, as expected |
| rerank | clicks | rips | 0.6834 | +0.00137 ± 0.00942 | 0.09375 | 0.78 | 3 | 0.943 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| rerank | clicks | snrips | 0.6834 | -0.00170 ± 0.00875 | 0.08705 | 0.75 | 3 | 0.943 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.6834 | +0.10950 ± 0.00022 | 0.10952 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.6834 | -0.00663 ± 0.00017 | 0.00685 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.6834 | +0.05773 ± 0.00054 | 0.05798 | 0.00 | 9891 | 1.001 | 99% | 0.00 |  | biased, as expected |
| rerank | clicks | cdr | 0.6834 | +0.00725 ± 0.00706 | 0.07058 | 0.91 | 3 | 0.943 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| rerank | satisfied | naive | 0.5249 | -0.24148 ± 0.00014 | 0.24148 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.5249 | -0.02896 ± 0.03532 | 0.35260 | 0.84 | 3 | 0.995 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.99; 0% of logs usable) |
| rerank | satisfied | snips | 0.5249 | -0.03923 ± 0.02988 | 0.29989 | 0.72 | 3 | 0.995 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.99; 0% of logs usable) |
| rerank | satisfied | iips | 0.5249 | +0.01451 ± 0.00077 | 0.01642 | 0.43 | 9891 | 1.001 | 99% | 0.43 |  | biased, as expected |
| rerank | satisfied | sniips | 0.5249 | +0.01466 ± 0.00067 | 0.01610 | 0.37 | 9891 | 1.001 | 99% | 0.37 |  | biased, as expected |
| rerank | satisfied | rips | 0.5249 | +0.00878 ± 0.00833 | 0.08333 | 0.78 | 3 | 0.943 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| rerank | satisfied | snrips | 0.5249 | +0.00336 ± 0.00666 | 0.06635 | 0.74 | 3 | 0.943 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.5249 | +0.05499 ± 0.00022 | 0.05503 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.5249 | -0.00127 ± 0.00020 | 0.00232 | 0.51 |  |  | 100% | 0.51 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.5249 | +0.01472 ± 0.00053 | 0.01565 | 0.18 | 9891 | 1.001 | 99% | 0.17 |  | biased, as expected |
| rerank | satisfied | cdr | 0.5249 | +0.01445 ± 0.00589 | 0.06040 | 0.97 | 3 | 0.943 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.94; 0% of logs usable) |
| oracle | clicks | naive | 0.7106 | -0.25771 ± 0.00014 | 0.25771 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.7106 | -0.04835 ± 0.04895 | 0.48943 | 0.85 | 3 | 1.001 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| oracle | clicks | snips | 0.7106 | -0.05181 ± 0.03507 | 0.35275 | 0.46 | 3 | 1.001 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| oracle | clicks | iips | 0.7106 | +0.08697 ± 0.00085 | 0.08738 | 0.00 | 9895 | 1.000 | 98% | 0.00 |  | biased, as expected |
| oracle | clicks | sniips | 0.7106 | +0.08532 ± 0.00057 | 0.08551 | 0.00 | 9895 | 1.000 | 98% | 0.00 |  | biased, as expected |
| oracle | clicks | rips | 0.7106 | -0.00121 ± 0.00611 | 0.06079 | 0.57 | 3 | 0.977 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.98; 0% of logs usable) |
| oracle | clicks | snrips | 0.7106 | -0.00677 ± 0.00497 | 0.04994 | 0.53 | 3 | 0.977 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.98; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.7106 | +0.17560 ± 0.00024 | 0.17561 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.7106 | -0.00829 ± 0.00018 | 0.00848 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.7106 | +0.08467 ± 0.00052 | 0.08483 | 0.00 | 9895 | 1.000 | 98% | 0.00 |  | biased, as expected |
| oracle | clicks | cdr | 0.7106 | -0.00377 ± 0.00599 | 0.05972 | 0.70 | 3 | 0.977 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.98; 0% of logs usable) |
| oracle | satisfied | naive | 0.5639 | -0.28052 ± 0.00014 | 0.28052 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5639 | -0.04684 ± 0.04389 | 0.43925 | 0.78 | 3 | 1.001 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| oracle | satisfied | snips | 0.5639 | -0.05782 ± 0.03576 | 0.36045 | 0.53 | 3 | 1.001 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| oracle | satisfied | iips | 0.5639 | +0.02795 ± 0.00079 | 0.02903 | 0.06 | 9895 | 1.000 | 98% | 0.06 |  | biased, as expected |
| oracle | satisfied | sniips | 0.5639 | +0.02656 ± 0.00056 | 0.02715 | 0.00 | 9895 | 1.000 | 98% | 0.00 |  | biased, as expected |
| oracle | satisfied | rips | 0.5639 | +0.00554 ± 0.00444 | 0.04450 | 0.95 | 3 | 0.977 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.98; 0% of logs usable) |
| oracle | satisfied | snrips | 0.5639 | +0.00248 ± 0.00377 | 0.03764 | 0.89 | 3 | 0.977 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.98; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.5639 | +0.09848 ± 0.00024 | 0.09851 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5639 | -0.00662 ± 0.00020 | 0.00692 | 0.01 |  |  | 100% | 0.01 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5639 | +0.02596 ± 0.00051 | 0.02646 | 0.00 | 9895 | 1.000 | 98% | 0.00 |  | biased, as expected |
| oracle | satisfied | cdr | 0.5639 | +0.00374 ± 0.00434 | 0.04330 | 0.93 | 3 | 0.977 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.98; 0% of logs usable) |
| reverse | clicks | naive | 0.2165 | +0.23640 ± 0.00014 | 0.23641 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.2165 | +0.00123 ± 0.02687 | 0.26735 | 0.49 | 3 | 1.046 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.05; 0% of logs usable) |
| reverse | clicks | snips | 0.2165 | -0.04663 ± 0.01982 | 0.20261 | 0.49 | 3 | 1.046 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.05; 0% of logs usable) |
| reverse | clicks | iips | 0.2165 | -0.02400 ± 0.00045 | 0.02440 | 0.00 | 9885 | 0.999 | 99% | 0.00 |  | biased, as expected |
| reverse | clicks | sniips | 0.2165 | -0.02391 ± 0.00044 | 0.02430 | 0.00 | 9885 | 0.999 | 99% | 0.00 |  | biased, as expected |
| reverse | clicks | rips | 0.2165 | +0.00267 ± 0.01525 | 0.15176 | 0.53 | 3 | 0.952 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| reverse | clicks | snrips | 0.2165 | -0.01130 ± 0.01138 | 0.11379 | 0.53 | 3 | 0.952 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.2165 | +0.02065 ± 0.00017 | 0.02072 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.2165 | +0.00710 ± 0.00019 | 0.00734 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.2165 | -0.02405 ± 0.00043 | 0.02442 | 0.00 | 9885 | 0.999 | 99% | 0.00 |  | biased, as expected |
| reverse | clicks | cdr | 0.2165 | -0.00315 ± 0.01370 | 0.13638 | 0.71 | 3 | 0.952 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| reverse | satisfied | naive | 0.0567 | +0.22669 ± 0.00014 | 0.22669 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0567 | -0.00230 ± 0.01244 | 0.12382 | 0.17 | 3 | 1.046 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.05; 0% of logs usable) |
| reverse | satisfied | snips | 0.0567 | -0.01330 ± 0.01040 | 0.10434 | 0.17 | 3 | 1.046 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.05; 0% of logs usable) |
| reverse | satisfied | iips | 0.0567 | -0.00393 ± 0.00023 | 0.00453 | 0.60 | 9885 | 0.999 | 99% | 0.60 |  | biased, as expected |
| reverse | satisfied | sniips | 0.0567 | -0.00390 ± 0.00023 | 0.00451 | 0.58 | 9885 | 0.999 | 99% | 0.58 |  | biased, as expected |
| reverse | satisfied | rips | 0.0567 | +0.00617 ± 0.00981 | 0.09785 | 0.36 | 3 | 0.952 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0567 | +0.00101 ± 0.00814 | 0.08101 | 0.37 | 3 | 0.952 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0567 | +0.03494 ± 0.00009 | 0.03495 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0567 | +0.02527 ± 0.00010 | 0.02529 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0567 | -0.00399 ± 0.00020 | 0.00447 | 0.49 | 9885 | 0.999 | 99% | 0.48 |  | biased, as expected |
| reverse | satisfied | cdr | 0.0567 | +0.00321 ± 0.00824 | 0.08202 | 0.82 | 3 | 0.952 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| wide | clicks | naive | 0.6725 | -0.21965 ± 0.00014 | 0.21966 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.6725 | -0.41248 ± 0.02394 | 0.47632 | 0.62 | 1 | 0.345 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snips | 0.6725 | -0.12822 ± 0.04579 | 0.47331 | 0.17 | 1 | 0.345 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.6725 | +0.02482 ± 0.00078 | 0.02600 | 0.12 | 6414 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | sniips | 0.6725 | +0.10625 ± 0.00078 | 0.10653 | 0.00 | 6414 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | rips | 0.6725 | -0.03958 ± 0.00693 | 0.07952 | 0.52 | 1 | 0.421 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snrips | 0.6725 | +0.05729 ± 0.01137 | 0.12677 | 0.92 | 1 | 0.421 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | dm-pbm | 0.6725 | +0.15718 ± 0.00023 | 0.15719 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.6725 | +0.01756 ± 0.00018 | 0.01765 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.6725 | +0.11190 ± 0.00053 | 0.11203 | 0.00 | 6414 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | cdr | 0.6725 | +0.03241 ± 0.00542 | 0.06288 | 0.82 | 1 | 0.421 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | naive | 0.5145 | -0.23109 ± 0.00014 | 0.23109 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.5145 | -0.32702 ± 0.02051 | 0.38550 | 0.51 | 1 | 0.345 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | snips | 0.5145 | -0.12418 ± 0.04303 | 0.44580 | 0.23 | 1 | 0.345 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.5145 | +0.00744 ± 0.00075 | 0.01056 | 0.77 | 6414 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | sniips | 0.5145 | +0.05802 ± 0.00076 | 0.05851 | 0.00 | 6414 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | rips | 0.5145 | -0.01230 ± 0.00632 | 0.06409 | 0.73 | 1 | 0.421 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.5145 | +0.05757 ± 0.01022 | 0.11687 | 0.94 | 1 | 0.421 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.5145 | +0.09691 ± 0.00023 | 0.09694 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.5145 | +0.01927 ± 0.00020 | 0.01937 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.5145 | +0.06195 ± 0.00055 | 0.06219 | 0.00 | 6414 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | cdr | 0.5145 | +0.03329 ± 0.00513 | 0.06092 | 0.72 | 1 | 0.421 | 0% |  |  | biased, as expected: 20% of its slots unsupported |

## scifact · cascade · {"policy":"swap","top":10}

100 logs of 100000 impressions, 12 s. Propensities: 10000000 lists, 0 mismatched; chi-square 3822 on 3900 df (p 0.812), largest |z| 3.41.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.6860 | 0.5322 | 0.5322 | 2.244 |
| rerank | 54.3% | 0.6834 | 0.5249 | 0.5249 | 2.247 |
| oracle | 5.8% | 0.7106 | 0.5639 | 0.5639 | 2.229 |
| reverse | 80.0% | 0.2165 | 0.0567 | 0.0567 | 2.501 |
| wide | 61.2% | 0.6725 | 0.5145 | 0.5145 | 2.252 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.6860 | -0.18319 ± 0.00016 | 0.18320 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.6860 | +0.00001 ± 0.00069 | 0.00691 | 0.98 | 10004 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | clicks | snips | 0.6860 | -0.00025 ± 0.00046 | 0.00462 | 0.96 | 10004 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | clicks | iips | 0.6860 | +0.05327 ± 0.00062 | 0.05362 | 0.00 | 10004 | 1.000 | 97% | 0.00 |  | biased, as expected |
| ranking | clicks | sniips | 0.6860 | +0.05307 ± 0.00047 | 0.05327 | 0.00 | 10004 | 1.000 | 97% | 0.00 |  | biased, as expected |
| ranking | clicks | rips | 0.6860 | +0.00001 ± 0.00069 | 0.00691 | 0.98 | 10004 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | clicks | snrips | 0.6860 | -0.00025 ± 0.00046 | 0.00462 | 0.96 | 10004 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | clicks | dm-pbm | 0.6860 | +0.10247 ± 0.00020 | 0.10249 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.6860 | -0.00637 ± 0.00017 | 0.00660 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.6860 | +0.05243 ± 0.00041 | 0.05259 | 0.00 | 10004 | 1.000 | 97% | 0.00 |  | biased, as expected |
| ranking | clicks | cdr | 0.6860 | -0.00044 ± 0.00044 | 0.00438 | 0.94 | 10004 | 1.000 | 100% | 0.94 | yes | ok |
| ranking | satisfied | naive | 0.5322 | -0.19865 ± 0.00016 | 0.19866 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.5322 | -0.00027 ± 0.00064 | 0.00638 | 0.99 | 10004 | 1.000 | 100% | 0.99 | yes | ok |
| ranking | satisfied | snips | 0.5322 | -0.00048 ± 0.00047 | 0.00469 | 0.98 | 10004 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | satisfied | iips | 0.5322 | +0.01262 ± 0.00059 | 0.01390 | 0.48 | 10004 | 1.000 | 97% | 0.47 |  | biased, as expected |
| ranking | satisfied | sniips | 0.5322 | +0.01245 ± 0.00049 | 0.01336 | 0.27 | 10004 | 1.000 | 97% | 0.28 |  | biased, as expected |
| ranking | satisfied | rips | 0.5322 | -0.00027 ± 0.00064 | 0.00638 | 0.99 | 10004 | 1.000 | 100% | 0.99 | yes | ok |
| ranking | satisfied | snrips | 0.5322 | -0.00048 ± 0.00047 | 0.00469 | 0.98 | 10004 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | satisfied | dm-pbm | 0.5322 | +0.05447 ± 0.00023 | 0.05452 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.5322 | -0.00177 ± 0.00022 | 0.00278 | 0.34 |  |  | 100% | 0.34 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.5322 | +0.01189 ± 0.00040 | 0.01255 | 0.16 | 10004 | 1.000 | 97% | 0.16 |  | biased, as expected |
| ranking | satisfied | cdr | 0.5322 | -0.00071 ± 0.00042 | 0.00428 | 0.95 | 10004 | 1.000 | 100% | 0.95 | yes | ok |
| rerank | clicks | naive | 0.6834 | -0.18054 ± 0.00016 | 0.18054 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.6834 | -0.67172 ± 0.00011 | 0.67172 | 0.00 | 196 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | snips | 0.6834 | -0.08739 ± 0.00369 | 0.09481 | 0.31 | 196 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | iips | 0.6834 | -0.12578 ± 0.00060 | 0.12592 | 0.00 | 8630 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | sniips | 0.6834 | +0.10355 ± 0.00093 | 0.10397 | 0.00 | 8630 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | rips | 0.6834 | -0.22040 ± 0.00055 | 0.22047 | 0.00 | 196 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | snrips | 0.6834 | -0.08233 ± 0.00133 | 0.08339 | 0.00 | 196 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | dm-pbm | 0.6834 | +0.11650 ± 0.00027 | 0.11653 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.6834 | -0.00286 ± 0.00021 | 0.00357 | 0.20 |  |  | 100% | 0.20 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.6834 | +0.08143 ± 0.00047 | 0.08156 | 0.00 | 8630 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | cdr | 0.6834 | +0.00287 ± 0.00044 | 0.00519 | 0.82 | 196 | 0.020 | 0% |  |  | unbiased though unsupported |
| rerank | satisfied | naive | 0.5249 | -0.19134 ± 0.00016 | 0.19134 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.5249 | -0.51650 ± 0.00009 | 0.51650 | 0.00 | 196 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | snips | 0.5249 | -0.09577 ± 0.00317 | 0.10083 | 0.20 | 196 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | iips | 0.5249 | -0.08617 ± 0.00056 | 0.08635 | 0.00 | 8630 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | sniips | 0.5249 | +0.06492 ± 0.00085 | 0.06547 | 0.00 | 8630 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | rips | 0.5249 | -0.15233 ± 0.00050 | 0.15241 | 0.00 | 196 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | snrips | 0.5249 | -0.07417 ± 0.00080 | 0.07459 | 0.00 | 196 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | dm-pbm | 0.5249 | +0.06691 ± 0.00025 | 0.06695 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.5249 | +0.00401 ± 0.00023 | 0.00461 | 0.11 |  |  | 100% | 0.11 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.5249 | +0.03817 ± 0.00045 | 0.03843 | 0.00 | 8630 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | cdr | 0.5249 | +0.00829 ± 0.00041 | 0.00924 | 0.41 | 196 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| oracle | clicks | naive | 0.7106 | -0.20775 ± 0.00016 | 0.20775 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.7106 | -0.10753 ± 0.00065 | 0.10772 | 0.00 | 8714 | 0.871 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | snips | 0.7106 | -0.01851 ± 0.00047 | 0.01909 | 0.04 | 8714 | 0.871 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | iips | 0.7106 | +0.04777 ± 0.00069 | 0.04826 | 0.00 | 10005 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | sniips | 0.7106 | +0.05515 ± 0.00044 | 0.05532 | 0.00 | 10005 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | rips | 0.7106 | -0.00823 ± 0.00072 | 0.01094 | 0.86 | 8714 | 0.871 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | snrips | 0.7106 | -0.00129 ± 0.00046 | 0.00474 | 0.94 | 8714 | 0.871 | 0% |  |  | unbiased though unsupported |
| oracle | clicks | dm-pbm | 0.7106 | +0.14029 ± 0.00020 | 0.14030 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.7106 | -0.00974 ± 0.00017 | 0.00989 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.7106 | +0.07274 ± 0.00041 | 0.07285 | 0.00 | 10005 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | cdr | 0.7106 | +0.00096 ± 0.00043 | 0.00434 | 0.93 | 8714 | 0.871 | 0% |  |  | unbiased though unsupported |
| oracle | satisfied | naive | 0.5639 | -0.23038 ± 0.00016 | 0.23038 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5639 | -0.08904 ± 0.00061 | 0.08925 | 0.00 | 8714 | 0.871 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | snips | 0.5639 | -0.01894 ± 0.00050 | 0.01959 | 0.04 | 8714 | 0.871 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | iips | 0.5639 | +0.00652 ± 0.00069 | 0.00944 | 0.91 | 10005 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | sniips | 0.5639 | +0.00749 ± 0.00049 | 0.00892 | 0.66 | 10005 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | rips | 0.5639 | -0.00464 ± 0.00068 | 0.00822 | 0.89 | 8714 | 0.871 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | snrips | 0.5639 | -0.00361 ± 0.00048 | 0.00601 | 0.89 | 8714 | 0.871 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | dm-pbm | 0.5639 | +0.07665 ± 0.00023 | 0.07668 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5639 | -0.00814 ± 0.00022 | 0.00842 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5639 | +0.02241 ± 0.00045 | 0.02285 | 0.00 | 10005 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | cdr | 0.5639 | +0.00064 ± 0.00044 | 0.00443 | 0.95 | 8714 | 0.871 | 0% |  |  | unbiased though unsupported |
| reverse | clicks | naive | 0.2165 | +0.28636 ± 0.00016 | 0.28637 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.2165 | -0.21650 ± 0.00000 | 0.21650 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snips | 0.2165 | -0.21650 ± 0.00000 | 0.21650 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | iips | 0.2165 | -0.16949 ± 0.00024 | 0.16951 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | sniips | 0.2165 | -0.16945 ± 0.00024 | 0.16947 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | rips | 0.2165 | -0.16949 ± 0.00024 | 0.16951 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snrips | 0.2165 | -0.16945 ± 0.00024 | 0.16947 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | dm-pbm | 0.2165 | +0.12206 ± 0.00034 | 0.12210 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.2165 | +0.03192 ± 0.00034 | 0.03210 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.2165 | +0.08390 ± 0.00036 | 0.08398 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | cdr | 0.2165 | +0.02380 ± 0.00034 | 0.02404 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | naive | 0.0567 | +0.27683 ± 0.00016 | 0.27683 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0567 | -0.05674 ± 0.00000 | 0.05674 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snips | 0.0567 | -0.05674 ± 0.00000 | 0.05674 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | iips | 0.0567 | -0.04461 ± 0.00012 | 0.04462 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | sniips | 0.0567 | -0.04460 ± 0.00012 | 0.04461 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | rips | 0.0567 | -0.04461 ± 0.00012 | 0.04462 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snrips | 0.0567 | -0.04460 ± 0.00012 | 0.04461 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | dm-pbm | 0.0567 | +0.11548 ± 0.00018 | 0.11550 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0567 | +0.05730 ± 0.00017 | 0.05733 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0567 | +0.08448 ± 0.00019 | 0.08450 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | cdr | 0.0567 | +0.04408 ± 0.00018 | 0.04411 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| wide | clicks | naive | 0.6725 | -0.16969 ± 0.00016 | 0.16970 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.6725 | -0.66146 ± 0.00011 | 0.66146 | 0.00 | 163 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | snips | 0.6725 | +0.00862 ± 0.00328 | 0.03375 | 0.93 | 163 | 0.016 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.6725 | -0.12544 ± 0.00059 | 0.12558 | 0.00 | 4233 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | sniips | 0.6725 | +0.18409 ± 0.00110 | 0.18441 | 0.00 | 4233 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | rips | 0.6725 | -0.21256 ± 0.00055 | 0.21263 | 0.00 | 163 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | snrips | 0.6725 | -0.05832 ± 0.00133 | 0.05980 | 0.03 | 163 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | dm-pbm | 0.6725 | +0.16449 ± 0.00026 | 0.16451 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.6725 | +0.02651 ± 0.00019 | 0.02658 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.6725 | +0.13127 ± 0.00047 | 0.13135 | 0.00 | 4233 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | cdr | 0.6725 | +0.03235 ± 0.00042 | 0.03262 | 0.00 | 163 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | naive | 0.5145 | -0.18095 ± 0.00016 | 0.18096 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.5145 | -0.50619 ± 0.00009 | 0.50620 | 0.00 | 163 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | snips | 0.5145 | -0.00330 ± 0.00318 | 0.03183 | 0.98 | 163 | 0.016 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.5145 | -0.07811 ± 0.00054 | 0.07829 | 0.00 | 4233 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | sniips | 0.5145 | +0.14176 ± 0.00095 | 0.14208 | 0.00 | 4233 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | rips | 0.5145 | -0.14284 ± 0.00050 | 0.14293 | 0.00 | 163 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | snrips | 0.5145 | -0.04620 ± 0.00083 | 0.04693 | 0.00 | 163 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.5145 | +0.11085 ± 0.00026 | 0.11088 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.5145 | +0.02918 ± 0.00022 | 0.02926 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.5145 | +0.08383 ± 0.00044 | 0.08395 | 0.00 | 4233 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | cdr | 0.5145 | +0.03375 ± 0.00040 | 0.03398 | 0.00 | 163 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |

## scifact · dcm · {"policy":"softmax","tau":1.0,"top":10,"by":"rank"}

100 logs of 100000 impressions, 13 s. Propensities: 10000000 lists, 0 mismatched; chi-square 13336 on 13500 df (p 0.841), largest |z| 3.76.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.7545 | 0.5526 | 0.5389 | 2.241 |
| rerank | 0.0% | 0.7526 | 0.5482 | 0.5341 | 2.243 |
| oracle | 0.0% | 0.7864 | 0.5853 | 0.5672 | 2.228 |
| reverse | 0.0% | 0.2302 | 0.0613 | 0.0597 | 2.499 |
| wide | 20.3% | 0.7401 | 0.5368 | 0.5232 | 2.248 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.7545 | -0.05350 ± 0.00018 | 0.05353 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.7545 | +0.01261 ± 0.00861 | 0.08660 | 0.97 | 113 | 1.012 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snips | 0.7545 | +0.00370 ± 0.00506 | 0.05043 | 0.96 | 113 | 1.012 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | iips | 0.7545 | +0.01000 ± 0.00057 | 0.01150 | 0.57 | 11596 | 1.001 | 100% | 0.57 |  | biased, as expected |
| ranking | clicks | sniips | 0.7545 | +0.00945 ± 0.00049 | 0.01062 | 0.48 | 11596 | 1.001 | 100% | 0.48 |  | biased, as expected |
| ranking | clicks | rips | 0.7545 | -0.00204 ± 0.00230 | 0.02299 | 0.92 | 113 | 1.006 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snrips | 0.7545 | -0.00298 ± 0.00219 | 0.02199 | 0.91 | 113 | 1.006 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | dm-pbm | 0.7545 | +0.06266 ± 0.00021 | 0.06269 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.7545 | -0.03484 ± 0.00017 | 0.03488 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.7545 | +0.00995 ± 0.00037 | 0.01060 | 0.26 | 11596 | 1.001 | 100% | 0.26 |  | biased, as expected |
| ranking | clicks | cdr | 0.7545 | +0.00008 ± 0.00190 | 0.01889 | 0.96 | 113 | 1.006 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | naive | 0.5526 | -0.05669 ± 0.00016 | 0.05671 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.5526 | +0.00359 ± 0.00711 | 0.07085 | 0.94 | 113 | 1.012 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snips | 0.5526 | -0.00316 ± 0.00467 | 0.04658 | 0.97 | 113 | 1.012 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | iips | 0.5526 | +0.00267 ± 0.00044 | 0.00512 | 0.91 | 11596 | 1.001 | 100% | 0.91 |  | unbiased here |
| ranking | satisfied | sniips | 0.5526 | +0.00228 ± 0.00041 | 0.00469 | 0.88 | 11596 | 1.001 | 100% | 0.88 |  | unbiased here |
| ranking | satisfied | rips | 0.5526 | -0.00302 ± 0.00137 | 0.01393 | 0.88 | 113 | 1.006 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snrips | 0.5526 | -0.00341 ± 0.00134 | 0.01381 | 0.83 | 113 | 1.006 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | dm-pbm | 0.5526 | +0.05053 ± 0.00019 | 0.05056 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.5526 | +0.00075 ± 0.00017 | 0.00184 | 0.62 |  |  | 100% | 0.62 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.5526 | +0.00268 ± 0.00031 | 0.00412 | 0.88 | 11596 | 1.001 | 100% | 0.88 |  | biased, as expected |
| ranking | satisfied | cdr | 0.5526 | -0.00122 ± 0.00106 | 0.01059 | 0.90 | 113 | 1.006 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| rerank | clicks | naive | 0.7526 | -0.05161 ± 0.00018 | 0.05164 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.7526 | +0.01323 ± 0.02414 | 0.24051 | 0.94 | 17 | 1.040 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 1.04; 1% of logs usable) |
| rerank | clicks | snips | 0.7526 | +0.01031 ± 0.01557 | 0.15523 | 0.87 | 17 | 1.040 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 1.04; 1% of logs usable) |
| rerank | clicks | iips | 0.7526 | +0.01031 ± 0.00058 | 0.01180 | 0.63 | 9477 | 1.001 | 99% | 0.63 |  | biased, as expected |
| rerank | clicks | sniips | 0.7526 | +0.01003 ± 0.00058 | 0.01155 | 0.62 | 9477 | 1.001 | 99% | 0.63 |  | biased, as expected |
| rerank | clicks | rips | 0.7526 | -0.00196 ± 0.00723 | 0.07195 | 0.85 | 17 | 0.968 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 0.97; 1% of logs usable) |
| rerank | clicks | snrips | 0.7526 | -0.00361 ± 0.00611 | 0.06092 | 0.85 | 17 | 0.968 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 0.97; 1% of logs usable) |
| rerank | clicks | dm-pbm | 0.7526 | +0.05937 ± 0.00022 | 0.05941 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.7526 | -0.03149 ± 0.00018 | 0.03154 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.7526 | +0.01001 ± 0.00044 | 0.01093 | 0.36 | 9477 | 1.001 | 99% | 0.36 |  | biased, as expected |
| rerank | clicks | cdr | 0.7526 | -0.00254 ± 0.00621 | 0.06186 | 0.86 | 17 | 0.968 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 0.97; 1% of logs usable) |
| rerank | satisfied | naive | 0.5482 | -0.05229 ± 0.00016 | 0.05232 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.5482 | +0.02799 ± 0.02117 | 0.21252 | 0.89 | 17 | 1.040 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 1.04; 1% of logs usable) |
| rerank | satisfied | snips | 0.5482 | +0.02245 ± 0.01445 | 0.14547 | 0.84 | 17 | 1.040 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 1.04; 1% of logs usable) |
| rerank | satisfied | iips | 0.5482 | +0.00299 ± 0.00049 | 0.00573 | 0.92 | 9477 | 1.001 | 99% | 0.92 |  | unbiased here |
| rerank | satisfied | sniips | 0.5482 | +0.00277 ± 0.00050 | 0.00572 | 0.88 | 9477 | 1.001 | 99% | 0.88 |  | unbiased here |
| rerank | satisfied | rips | 0.5482 | -0.00006 ± 0.00289 | 0.02879 | 0.93 | 17 | 0.968 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 0.97; 1% of logs usable) |
| rerank | satisfied | snrips | 0.5482 | +0.00080 ± 0.00301 | 0.03001 | 0.89 | 17 | 0.968 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 0.97; 1% of logs usable) |
| rerank | satisfied | dm-pbm | 0.5482 | +0.04895 ± 0.00020 | 0.04899 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.5482 | +0.00082 ± 0.00019 | 0.00205 | 0.51 |  |  | 100% | 0.51 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.5482 | +0.00274 ± 0.00035 | 0.00447 | 0.87 | 9477 | 1.001 | 99% | 0.88 |  | biased, as expected |
| rerank | satisfied | cdr | 0.5482 | +0.00031 ± 0.00230 | 0.02285 | 0.99 | 17 | 0.968 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 17 impressions, mean 0.97; 1% of logs usable) |
| oracle | clicks | naive | 0.7864 | -0.08544 ± 0.00018 | 0.08546 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.7864 | +0.00631 ± 0.01315 | 0.13101 | 0.98 | 93 | 1.008 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snips | 0.7864 | -0.00165 ± 0.00734 | 0.07301 | 0.93 | 93 | 1.008 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | iips | 0.7864 | +0.01984 ± 0.00067 | 0.02094 | 0.15 | 11656 | 1.001 | 100% | 0.15 |  | biased, as expected |
| oracle | clicks | sniips | 0.7864 | +0.01973 ± 0.00045 | 0.02024 | 0.01 | 11656 | 1.001 | 100% | 0.01 |  | biased, as expected |
| oracle | clicks | rips | 0.7864 | -0.00141 ± 0.00205 | 0.02043 | 0.89 | 93 | 1.003 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snrips | 0.7864 | -0.00154 ± 0.00197 | 0.01962 | 0.89 | 93 | 1.003 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | dm-pbm | 0.7864 | +0.08995 ± 0.00021 | 0.08997 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.7864 | -0.04467 ± 0.00017 | 0.04470 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.7864 | +0.02015 ± 0.00043 | 0.02060 | 0.00 | 11656 | 1.001 | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | cdr | 0.7864 | -0.00118 ± 0.00196 | 0.01950 | 0.90 | 93 | 1.003 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | naive | 0.5853 | -0.08940 ± 0.00016 | 0.08941 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5853 | -0.00025 ± 0.00905 | 0.09007 | 0.94 | 93 | 1.008 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snips | 0.5853 | -0.00583 ± 0.00594 | 0.05942 | 0.92 | 93 | 1.008 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | iips | 0.5853 | +0.00746 ± 0.00051 | 0.00904 | 0.71 | 11656 | 1.001 | 100% | 0.71 |  | biased, as expected |
| oracle | satisfied | sniips | 0.5853 | +0.00745 ± 0.00037 | 0.00829 | 0.52 | 11656 | 1.001 | 100% | 0.52 |  | biased, as expected |
| oracle | satisfied | rips | 0.5853 | -0.00095 ± 0.00097 | 0.00973 | 0.90 | 93 | 1.003 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snrips | 0.5853 | -0.00084 ± 0.00088 | 0.00879 | 0.89 | 93 | 1.003 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | dm-pbm | 0.5853 | +0.06983 ± 0.00020 | 0.06985 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5853 | -0.00386 ± 0.00017 | 0.00423 | 0.08 |  |  | 100% | 0.08 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5853 | +0.00783 ± 0.00033 | 0.00849 | 0.40 | 11656 | 1.001 | 100% | 0.40 |  | biased, as expected |
| oracle | satisfied | cdr | 0.5853 | -0.00063 ± 0.00087 | 0.00866 | 0.89 | 93 | 1.003 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| reverse | clicks | naive | 0.2302 | +0.47083 ± 0.00018 | 0.47083 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.2302 | -0.23017 ± 0.00000 | 0.23017 | 0.00 | 0 | 0.433 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.43; 0% of logs usable) |
| reverse | clicks | snips | 0.2302 | -0.23017 ± 0.00000 | 0.23017 | 0.00 | 0 | 0.433 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.43; 0% of logs usable) |
| reverse | clicks | iips | 0.2302 | -0.02372 ± 0.00054 | 0.02432 | 0.04 | 3411 | 0.997 | 100% | 0.04 |  | biased, as expected |
| reverse | clicks | sniips | 0.2302 | -0.02358 ± 0.00053 | 0.02417 | 0.02 | 3411 | 0.997 | 100% | 0.02 |  | biased, as expected |
| reverse | clicks | rips | 0.2302 | -0.00710 ± 0.05160 | 0.51343 | 0.27 | 0 | 0.012 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.01; 0% of logs usable) |
| reverse | clicks | snrips | 0.2302 | -0.07333 ± 0.01741 | 0.18809 | 0.25 | 0 | 0.012 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.01; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.2302 | +0.04896 ± 0.00019 | 0.04899 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.2302 | +0.08866 ± 0.00025 | 0.08870 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.2302 | -0.02334 ± 0.00054 | 0.02396 | 0.01 | 3411 | 0.997 | 100% | 0.01 |  | biased, as expected |
| reverse | clicks | cdr | 0.2302 | +0.02481 ± 0.04839 | 0.48213 | 0.96 | 0 | 0.012 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.01; 0% of logs usable) |
| reverse | satisfied | naive | 0.0613 | +0.43455 ± 0.00016 | 0.43456 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0613 | -0.06133 ± 0.00000 | 0.06133 | 0.00 | 0 | 0.433 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.43; 0% of logs usable) |
| reverse | satisfied | snips | 0.0613 | -0.06133 ± 0.00000 | 0.06133 | 0.00 | 0 | 0.433 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.43; 0% of logs usable) |
| reverse | satisfied | iips | 0.0613 | -0.00381 ± 0.00031 | 0.00488 | 0.80 | 3411 | 0.997 | 100% | 0.80 |  | biased, as expected |
| reverse | satisfied | sniips | 0.0613 | -0.00377 ± 0.00030 | 0.00482 | 0.82 | 3411 | 0.997 | 100% | 0.82 |  | biased, as expected |
| reverse | satisfied | rips | 0.0613 | -0.00131 ± 0.02967 | 0.29525 | 0.14 | 0 | 0.012 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.01; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0613 | -0.02169 ± 0.01069 | 0.10852 | 0.15 | 0 | 0.012 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.01; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0613 | +0.06513 ± 0.00010 | 0.06514 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0613 | +0.06763 ± 0.00012 | 0.06765 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0613 | -0.00344 ± 0.00028 | 0.00440 | 0.75 | 3411 | 0.997 | 100% | 0.75 |  | biased, as expected |
| reverse | satisfied | cdr | 0.0613 | +0.00842 ± 0.02033 | 0.20245 | 0.75 | 0 | 0.012 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.01; 0% of logs usable) |
| wide | clicks | naive | 0.7401 | -0.03907 ± 0.00018 | 0.03911 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.7401 | -0.47760 ± 0.01184 | 0.49192 | 0.11 | 11 | 0.343 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snips | 0.7401 | +0.02700 ± 0.01931 | 0.19399 | 0.81 | 11 | 0.343 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.7401 | -0.02458 ± 0.00056 | 0.02521 | 0.00 | 6566 | 0.642 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | sniips | 0.7401 | +0.06320 ± 0.00064 | 0.06352 | 0.00 | 6566 | 0.642 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | rips | 0.7401 | -0.06774 ± 0.00345 | 0.07597 | 0.35 | 11 | 0.502 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snrips | 0.7401 | +0.06391 ± 0.00684 | 0.09337 | 0.91 | 11 | 0.502 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | dm-pbm | 0.7401 | +0.14851 ± 0.00022 | 0.14852 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.7401 | +0.02277 ± 0.00017 | 0.02283 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.7401 | +0.10769 ± 0.00043 | 0.10778 | 0.00 | 6566 | 0.642 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | cdr | 0.7401 | +0.03998 ± 0.00280 | 0.04871 | 0.48 | 11 | 0.502 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | naive | 0.5368 | -0.04090 ± 0.00016 | 0.04094 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.5368 | -0.34268 ± 0.01009 | 0.35708 | 0.12 | 11 | 0.343 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | snips | 0.5368 | +0.03087 ± 0.02004 | 0.20181 | 0.86 | 11 | 0.343 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.5368 | -0.00418 ± 0.00047 | 0.00628 | 0.82 | 6566 | 0.642 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | sniips | 0.5368 | +0.04974 ± 0.00053 | 0.05002 | 0.00 | 6566 | 0.642 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | rips | 0.5368 | -0.02205 ± 0.00240 | 0.03250 | 0.62 | 11 | 0.502 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | snrips | 0.5368 | +0.06075 ± 0.00456 | 0.07583 | 0.77 | 11 | 0.502 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.5368 | +0.12688 ± 0.00021 | 0.12689 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.5368 | +0.04845 ± 0.00019 | 0.04849 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.5368 | +0.08939 ± 0.00036 | 0.08946 | 0.00 | 6566 | 0.642 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | cdr | 0.5368 | +0.05080 ± 0.00195 | 0.05439 | 0.15 | 11 | 0.502 | 0% |  |  | biased, as expected: 20% of its slots unsupported |

## scifact · dcm · {"policy":"epsilon","epsilon":0.3,"top":10}

100 logs of 100000 impressions, 13 s. Propensities: 10000000 lists, 0 mismatched; chi-square 13359 on 13500 df (p 0.805), largest |z| 4.35.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.7545 | 0.5526 | 0.5389 | 2.241 |
| rerank | 0.0% | 0.7526 | 0.5482 | 0.5341 | 2.243 |
| oracle | 0.0% | 0.7864 | 0.5853 | 0.5672 | 2.228 |
| reverse | 0.0% | 0.2302 | 0.0613 | 0.0597 | 2.499 |
| wide | 20.3% | 0.7401 | 0.5368 | 0.5232 | 2.248 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.7545 | -0.07824 ± 0.00019 | 0.07827 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.7545 | +0.00013 ± 0.00027 | 0.00264 | 0.97 | 69988 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | clicks | snips | 0.7545 | +0.00028 ± 0.00024 | 0.00238 | 0.92 | 69988 | 1.000 | 100% | 0.92 | yes | ok |
| ranking | clicks | iips | 0.7545 | +0.00117 ± 0.00026 | 0.00281 | 0.95 | 72926 | 1.000 | 100% | 0.95 |  | unbiased here |
| ranking | clicks | sniips | 0.7545 | +0.00129 ± 0.00023 | 0.00263 | 0.89 | 72926 | 1.000 | 100% | 0.89 |  | unbiased here |
| ranking | clicks | rips | 0.7545 | +0.00010 ± 0.00026 | 0.00259 | 0.96 | 69988 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | clicks | snrips | 0.7545 | +0.00024 ± 0.00023 | 0.00234 | 0.90 | 69988 | 1.000 | 100% | 0.90 | yes | ok |
| ranking | clicks | dm-pbm | 0.7545 | +0.01633 ± 0.00021 | 0.01647 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.7545 | -0.01664 ± 0.00020 | 0.01676 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.7545 | +0.00128 ± 0.00023 | 0.00259 | 0.86 | 72926 | 1.000 | 100% | 0.86 |  | unbiased here |
| ranking | clicks | cdr | 0.7545 | +0.00023 ± 0.00022 | 0.00225 | 0.91 | 69988 | 1.000 | 100% | 0.91 | yes | ok |
| ranking | satisfied | naive | 0.5526 | -0.07507 ± 0.00016 | 0.07508 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.5526 | -0.00014 ± 0.00022 | 0.00215 | 0.95 | 69988 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | satisfied | snips | 0.5526 | -0.00003 ± 0.00019 | 0.00193 | 0.94 | 69988 | 1.000 | 100% | 0.94 | yes | ok |
| ranking | satisfied | iips | 0.5526 | +0.00017 ± 0.00021 | 0.00206 | 0.97 | 72926 | 1.000 | 100% | 0.97 |  | unbiased here |
| ranking | satisfied | sniips | 0.5526 | +0.00026 ± 0.00019 | 0.00186 | 0.95 | 72926 | 1.000 | 100% | 0.95 |  | unbiased here |
| ranking | satisfied | rips | 0.5526 | -0.00015 ± 0.00021 | 0.00212 | 0.97 | 69988 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | satisfied | snrips | 0.5526 | -0.00005 ± 0.00019 | 0.00189 | 0.97 | 69988 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | satisfied | dm-pbm | 0.5526 | +0.01522 ± 0.00018 | 0.01533 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.5526 | +0.00535 ± 0.00018 | 0.00564 | 0.04 |  |  | 100% | 0.04 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.5526 | +0.00025 ± 0.00018 | 0.00185 | 0.93 | 72926 | 1.000 | 100% | 0.93 |  | unbiased here |
| ranking | satisfied | cdr | 0.5526 | -0.00005 ± 0.00018 | 0.00184 | 0.94 | 69988 | 1.000 | 100% | 0.94 | yes | ok |
| rerank | clicks | naive | 0.7526 | -0.07636 ± 0.00019 | 0.07638 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.7526 | +0.10673 ± 0.10853 | 1.08507 | 0.49 | 147 | 1.054 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 147 impressions, mean 1.05; 0% of logs usable) |
| rerank | clicks | snips | 0.7526 | -0.08842 ± 0.03775 | 0.38584 | 0.12 | 147 | 1.054 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 147 impressions, mean 1.05; 0% of logs usable) |
| rerank | clicks | iips | 0.7526 | +0.01512 ± 0.00114 | 0.01888 | 0.62 | 3643 | 1.003 | 99% | 0.62 |  | biased, as expected |
| rerank | clicks | sniips | 0.7526 | +0.01488 ± 0.00105 | 0.01819 | 0.66 | 3643 | 1.003 | 99% | 0.66 |  | biased, as expected |
| rerank | clicks | rips | 0.7526 | +0.03270 ± 0.03124 | 0.31255 | 0.66 | 3 | 0.719 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.72; 0% of logs usable) |
| rerank | clicks | snrips | 0.7526 | -0.00243 ± 0.01740 | 0.17315 | 0.69 | 3 | 0.719 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.72; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.7526 | +0.02067 ± 0.00025 | 0.02082 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.7526 | -0.02241 ± 0.00021 | 0.02251 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.7526 | +0.01501 ± 0.00082 | 0.01709 | 0.53 | 3643 | 1.003 | 99% | 0.53 |  | biased, as expected |
| rerank | clicks | cdr | 0.7526 | +0.01788 ± 0.02164 | 0.21603 | 0.80 | 3 | 0.719 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.72; 0% of logs usable) |
| rerank | satisfied | naive | 0.5482 | -0.07067 ± 0.00016 | 0.07069 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.5482 | +0.04814 ± 0.08366 | 0.83379 | 0.40 | 147 | 1.054 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 147 impressions, mean 1.05; 0% of logs usable) |
| rerank | satisfied | snips | 0.5482 | -0.10760 ± 0.03512 | 0.36559 | 0.17 | 147 | 1.054 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 147 impressions, mean 1.05; 0% of logs usable) |
| rerank | satisfied | iips | 0.5482 | +0.00339 ± 0.00087 | 0.00933 | 0.91 | 3643 | 1.003 | 99% | 0.91 |  | unbiased here |
| rerank | satisfied | sniips | 0.5482 | +0.00326 ± 0.00082 | 0.00876 | 0.95 | 3643 | 1.003 | 99% | 0.95 |  | unbiased here |
| rerank | satisfied | rips | 0.5482 | +0.03345 ± 0.02268 | 0.22817 | 0.79 | 3 | 0.719 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.72; 0% of logs usable) |
| rerank | satisfied | snrips | 0.5482 | +0.00120 ± 0.01031 | 0.10256 | 0.80 | 3 | 0.719 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.72; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.5482 | +0.01785 ± 0.00020 | 0.01796 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.5482 | +0.00265 ± 0.00018 | 0.00322 | 0.22 |  |  | 100% | 0.22 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.5482 | +0.00347 ± 0.00055 | 0.00647 | 0.92 | 3643 | 1.003 | 99% | 0.92 |  | biased, as expected |
| rerank | satisfied | cdr | 0.5482 | +0.02107 ± 0.01425 | 0.14336 | 0.92 | 3 | 0.719 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.72; 0% of logs usable) |
| oracle | clicks | naive | 0.7864 | -0.11018 ± 0.00019 | 0.11020 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.7864 | -0.08481 ± 0.03794 | 0.38693 | 0.13 | 45357 | 0.923 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 45357 impressions, mean 0.92; 0% of logs usable) |
| oracle | clicks | snips | 0.7864 | -0.03731 ± 0.01070 | 0.11280 | 0.12 | 45357 | 0.923 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 45357 impressions, mean 0.92; 0% of logs usable) |
| oracle | clicks | iips | 0.7864 | +0.01456 ± 0.00082 | 0.01671 | 0.63 | 11307 | 0.999 | 98% | 0.64 |  | biased, as expected |
| oracle | clicks | sniips | 0.7864 | +0.01373 ± 0.00058 | 0.01490 | 0.31 | 11307 | 0.999 | 98% | 0.31 |  | biased, as expected |
| oracle | clicks | rips | 0.7864 | -0.00527 ± 0.00270 | 0.02739 | 0.70 | 74 | 0.834 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 74 impressions, mean 0.83; 0% of logs usable) |
| oracle | clicks | snrips | 0.7864 | -0.00127 ± 0.00251 | 0.02501 | 0.74 | 74 | 0.834 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 74 impressions, mean 0.83; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.7864 | +0.06479 ± 0.00023 | 0.06483 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.7864 | -0.02905 ± 0.00020 | 0.02911 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.7864 | +0.01341 ± 0.00057 | 0.01455 | 0.34 | 11307 | 0.999 | 98% | 0.34 |  | biased, as expected |
| oracle | clicks | cdr | 0.7864 | -0.00526 ± 0.00261 | 0.02646 | 0.78 | 74 | 0.834 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 74 impressions, mean 0.83; 0% of logs usable) |
| oracle | satisfied | naive | 0.5853 | -0.10778 ± 0.00016 | 0.10779 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5853 | -0.07475 ± 0.03039 | 0.31145 | 0.10 | 45357 | 0.923 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 45357 impressions, mean 0.92; 0% of logs usable) |
| oracle | satisfied | snips | 0.5853 | -0.03959 ± 0.01049 | 0.11159 | 0.10 | 45357 | 0.923 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 45357 impressions, mean 0.92; 0% of logs usable) |
| oracle | satisfied | iips | 0.5853 | +0.00823 ± 0.00077 | 0.01127 | 0.81 | 11307 | 0.999 | 98% | 0.82 |  | biased, as expected |
| oracle | satisfied | sniips | 0.5853 | +0.00751 ± 0.00054 | 0.00921 | 0.66 | 11307 | 0.999 | 98% | 0.66 |  | biased, as expected |
| oracle | satisfied | rips | 0.5853 | +0.00181 ± 0.00228 | 0.02278 | 0.92 | 74 | 0.834 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 74 impressions, mean 0.83; 0% of logs usable) |
| oracle | satisfied | snrips | 0.5853 | +0.00150 ± 0.00195 | 0.01948 | 0.87 | 74 | 0.834 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 74 impressions, mean 0.83; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.5853 | +0.05316 ± 0.00020 | 0.05320 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5853 | -0.00218 ± 0.00019 | 0.00292 | 0.32 |  |  | 100% | 0.32 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5853 | +0.00722 ± 0.00053 | 0.00892 | 0.63 | 11307 | 0.999 | 98% | 0.63 |  | biased, as expected |
| oracle | satisfied | cdr | 0.5853 | +0.00153 ± 0.00201 | 0.02007 | 0.91 | 74 | 0.834 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 74 impressions, mean 0.83; 0% of logs usable) |
| reverse | clicks | naive | 0.2302 | +0.44609 ± 0.00019 | 0.44609 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.2302 | +0.13271 ± 0.06799 | 0.68938 | 0.26 | 1 | 1.179 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.18; 0% of logs usable) |
| reverse | clicks | snips | 0.2302 | -0.04100 ± 0.03969 | 0.39703 | 0.17 | 1 | 1.179 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.18; 0% of logs usable) |
| reverse | clicks | iips | 0.2302 | -0.00906 ± 0.00089 | 0.01270 | 0.83 | 2946 | 1.006 | 99% | 0.83 |  | biased, as expected |
| reverse | clicks | sniips | 0.2302 | -0.00961 ± 0.00082 | 0.01263 | 0.85 | 2946 | 1.006 | 99% | 0.85 |  | biased, as expected |
| reverse | clicks | rips | 0.2302 | +0.06189 ± 0.03388 | 0.34278 | 0.51 | 1 | 0.781 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.78; 0% of logs usable) |
| reverse | clicks | snrips | 0.2302 | +0.00618 ± 0.02317 | 0.23058 | 0.51 | 1 | 0.781 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.78; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.2302 | +0.10988 ± 0.00032 | 0.10993 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.2302 | +0.06340 ± 0.00033 | 0.06348 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.2302 | -0.01048 ± 0.00080 | 0.01316 | 0.75 | 2946 | 1.006 | 99% | 0.75 |  | biased, as expected |
| reverse | clicks | cdr | 0.2302 | +0.04136 ± 0.02764 | 0.27813 | 0.87 | 1 | 0.781 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.78; 0% of logs usable) |
| reverse | satisfied | naive | 0.0613 | +0.41617 ± 0.00016 | 0.41618 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0613 | +0.00923 ± 0.02585 | 0.25735 | 0.07 | 1 | 1.179 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.18; 0% of logs usable) |
| reverse | satisfied | snips | 0.0613 | -0.02466 ± 0.01452 | 0.14658 | 0.06 | 1 | 1.179 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.18; 0% of logs usable) |
| reverse | satisfied | iips | 0.0613 | -0.00165 ± 0.00044 | 0.00469 | 0.93 | 2946 | 1.006 | 99% | 0.93 |  | unbiased here |
| reverse | satisfied | sniips | 0.0613 | -0.00182 ± 0.00044 | 0.00471 | 0.91 | 2946 | 1.006 | 99% | 0.91 |  | unbiased here |
| reverse | satisfied | rips | 0.0613 | +0.00958 ± 0.01757 | 0.17511 | 0.46 | 1 | 0.781 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.78; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0613 | -0.00434 ± 0.01173 | 0.11683 | 0.45 | 1 | 0.781 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.78; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0613 | +0.10867 ± 0.00016 | 0.10868 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0613 | +0.07543 ± 0.00016 | 0.07544 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0613 | -0.00247 ± 0.00043 | 0.00497 | 0.86 | 2946 | 1.006 | 99% | 0.86 |  | biased, as expected |
| reverse | satisfied | cdr | 0.0613 | -0.00331 ± 0.01524 | 0.15163 | 0.98 | 1 | 0.781 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.78; 0% of logs usable) |
| wide | clicks | naive | 0.7401 | -0.06381 ± 0.00019 | 0.06384 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.7401 | -0.41497 ± 0.06388 | 0.75908 | 0.24 | 260 | 0.388 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snips | 0.7401 | -0.15727 ± 0.02853 | 0.32451 | 0.02 | 260 | 0.388 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | iips | 0.7401 | -0.02107 ± 0.00102 | 0.02341 | 0.51 | 2379 | 0.642 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | sniips | 0.7401 | +0.06847 ± 0.00127 | 0.06963 | 0.00 | 2379 | 0.642 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | rips | 0.7401 | -0.05212 ± 0.01624 | 0.16983 | 0.54 | 4 | 0.205 | 0% |  |  | unbiased though unsupported |
| wide | clicks | snrips | 0.7401 | +0.06600 ± 0.02009 | 0.21052 | 0.85 | 4 | 0.205 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.7401 | +0.08199 ± 0.00027 | 0.08203 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.7401 | +0.03456 ± 0.00020 | 0.03462 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.7401 | +0.08450 ± 0.00075 | 0.08483 | 0.00 | 2379 | 0.642 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | cdr | 0.7401 | +0.05196 ± 0.01166 | 0.12710 | 0.91 | 4 | 0.205 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | naive | 0.5368 | -0.05928 ± 0.00016 | 0.05931 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.5368 | -0.30335 ± 0.05705 | 0.64360 | 0.17 | 260 | 0.388 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | snips | 0.5368 | -0.16553 ± 0.02758 | 0.32051 | 0.03 | 260 | 0.388 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | iips | 0.5368 | -0.00426 ± 0.00083 | 0.00932 | 0.89 | 2379 | 0.642 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | sniips | 0.5368 | +0.04970 ± 0.00102 | 0.05072 | 0.01 | 2379 | 0.642 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | rips | 0.5368 | -0.00433 ± 0.01529 | 0.15215 | 0.76 | 4 | 0.205 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.5368 | +0.06371 ± 0.01812 | 0.19125 | 0.86 | 4 | 0.205 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | dm-pbm | 0.5368 | +0.07532 ± 0.00022 | 0.07535 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.5368 | +0.05256 ± 0.00018 | 0.05259 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.5368 | +0.07050 ± 0.00052 | 0.07068 | 0.00 | 2379 | 0.642 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | cdr | 0.5368 | +0.06529 ± 0.01147 | 0.13151 | 0.61 | 4 | 0.205 | 0% |  |  | biased, as expected: 20% of its slots unsupported |

## scifact · dcm · {"policy":"shuffle","top":10}

100 logs of 100000 impressions, 12 s. Propensities: 10000000 lists, 0 mismatched; chi-square 13406 on 13500 df (p 0.716), largest |z| 4.36.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.7545 | 0.5526 | 0.5389 | 2.241 |
| rerank | 0.0% | 0.7526 | 0.5482 | 0.5341 | 2.243 |
| oracle | 0.0% | 0.7864 | 0.5853 | 0.5672 | 2.228 |
| reverse | 0.0% | 0.2302 | 0.0613 | 0.0597 | 2.499 |
| wide | 20.3% | 0.7401 | 0.5368 | 0.5232 | 2.248 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.7545 | -0.26142 ± 0.00018 | 0.26143 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.7545 | +0.00150 ± 0.05513 | 0.54851 | 0.90 | 3 | 1.007 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.01; 0% of logs usable) |
| ranking | clicks | snips | 0.7545 | -0.04034 ± 0.03752 | 0.37548 | 0.55 | 3 | 1.007 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.01; 0% of logs usable) |
| ranking | clicks | iips | 0.7545 | +0.02556 ± 0.00094 | 0.02721 | 0.21 | 9888 | 1.001 | 98% | 0.21 |  | biased, as expected |
| ranking | clicks | sniips | 0.7545 | +0.02555 ± 0.00080 | 0.02676 | 0.07 | 9888 | 1.001 | 98% | 0.07 |  | biased, as expected |
| ranking | clicks | rips | 0.7545 | -0.00264 ± 0.01355 | 0.13485 | 0.58 | 3 | 0.862 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.86; 0% of logs usable) |
| ranking | clicks | snrips | 0.7545 | -0.00931 ± 0.01244 | 0.12414 | 0.52 | 3 | 0.862 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.86; 0% of logs usable) |
| ranking | clicks | dm-pbm | 0.7545 | +0.09245 ± 0.00027 | 0.09249 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.7545 | -0.05384 ± 0.00019 | 0.05387 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.7545 | +0.02601 ± 0.00061 | 0.02671 | 0.00 | 9888 | 1.001 | 98% | 0.00 |  | biased, as expected |
| ranking | clicks | cdr | 0.7545 | -0.00833 ± 0.01161 | 0.11583 | 0.61 | 3 | 0.862 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.86; 0% of logs usable) |
| ranking | satisfied | naive | 0.5526 | -0.25008 ± 0.00016 | 0.25008 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.5526 | -0.02942 ± 0.04097 | 0.40869 | 0.82 | 3 | 1.007 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.01; 0% of logs usable) |
| ranking | satisfied | snips | 0.5526 | -0.04118 ± 0.03478 | 0.34853 | 0.61 | 3 | 1.007 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.01; 0% of logs usable) |
| ranking | satisfied | iips | 0.5526 | +0.00672 ± 0.00073 | 0.00988 | 0.87 | 9888 | 1.001 | 98% | 0.87 |  | biased, as expected |
| ranking | satisfied | sniips | 0.5526 | +0.00678 ± 0.00062 | 0.00918 | 0.79 | 9888 | 1.001 | 98% | 0.80 |  | biased, as expected |
| ranking | satisfied | rips | 0.5526 | +0.00292 ± 0.00824 | 0.08204 | 0.72 | 3 | 0.862 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.86; 0% of logs usable) |
| ranking | satisfied | snrips | 0.5526 | -0.00268 ± 0.00634 | 0.06312 | 0.67 | 3 | 0.862 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.86; 0% of logs usable) |
| ranking | satisfied | dm-pbm | 0.5526 | +0.06111 ± 0.00025 | 0.06116 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.5526 | -0.01201 ± 0.00022 | 0.01220 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.5526 | +0.00716 ± 0.00047 | 0.00853 | 0.66 | 9888 | 1.001 | 98% | 0.66 |  | biased, as expected |
| ranking | satisfied | cdr | 0.5526 | -0.00226 ± 0.00632 | 0.06296 | 0.92 | 3 | 0.862 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.86; 0% of logs usable) |
| rerank | clicks | naive | 0.7526 | -0.25954 ± 0.00018 | 0.25955 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.7526 | -0.05104 ± 0.05536 | 0.55316 | 0.83 | 3 | 0.931 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| rerank | clicks | snips | 0.7526 | -0.05653 ± 0.04101 | 0.41191 | 0.50 | 3 | 0.931 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| rerank | clicks | iips | 0.7526 | +0.02363 ± 0.00083 | 0.02504 | 0.20 | 9894 | 1.000 | 100% | 0.20 |  | biased, as expected |
| rerank | clicks | sniips | 0.7526 | +0.02372 ± 0.00076 | 0.02489 | 0.13 | 9894 | 1.000 | 100% | 0.13 |  | biased, as expected |
| rerank | clicks | rips | 0.7526 | +0.00892 ± 0.01512 | 0.15069 | 0.70 | 3 | 0.846 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.85; 0% of logs usable) |
| rerank | clicks | snrips | 0.7526 | +0.00537 ± 0.01420 | 0.14139 | 0.67 | 3 | 0.846 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.85; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.7526 | +0.07716 ± 0.00026 | 0.07721 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.7526 | -0.05323 ± 0.00018 | 0.05326 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.7526 | +0.02391 ± 0.00061 | 0.02466 | 0.01 | 9894 | 1.000 | 100% | 0.01 |  | biased, as expected |
| rerank | clicks | cdr | 0.7526 | +0.00544 ± 0.01318 | 0.13126 | 0.72 | 3 | 0.846 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.85; 0% of logs usable) |
| rerank | satisfied | naive | 0.5482 | -0.24568 ± 0.00016 | 0.24569 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.5482 | -0.02805 ± 0.04364 | 0.43510 | 0.79 | 3 | 0.931 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| rerank | satisfied | snips | 0.5482 | -0.02434 ± 0.03793 | 0.37822 | 0.59 | 3 | 0.931 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| rerank | satisfied | iips | 0.5482 | +0.00618 ± 0.00072 | 0.00948 | 0.86 | 9894 | 1.000 | 100% | 0.86 |  | biased, as expected |
| rerank | satisfied | sniips | 0.5482 | +0.00628 ± 0.00063 | 0.00888 | 0.84 | 9894 | 1.000 | 100% | 0.84 |  | biased, as expected |
| rerank | satisfied | rips | 0.5482 | +0.01333 ± 0.01051 | 0.10544 | 0.76 | 3 | 0.846 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.85; 0% of logs usable) |
| rerank | satisfied | snrips | 0.5482 | +0.01221 ± 0.01071 | 0.10730 | 0.69 | 3 | 0.846 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.85; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.5482 | +0.04934 ± 0.00025 | 0.04941 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.5482 | -0.01330 ± 0.00021 | 0.01347 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.5482 | +0.00643 ± 0.00051 | 0.00822 | 0.68 | 9894 | 1.000 | 100% | 0.68 |  | biased, as expected |
| rerank | satisfied | cdr | 0.5482 | +0.00982 ± 0.00830 | 0.08319 | 0.94 | 3 | 0.846 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.85; 0% of logs usable) |
| oracle | clicks | naive | 0.7864 | -0.29336 ± 0.00018 | 0.29337 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.7864 | +0.02399 ± 0.05586 | 0.55628 | 0.89 | 3 | 1.004 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| oracle | clicks | snips | 0.7864 | -0.02129 ± 0.03716 | 0.37031 | 0.46 | 3 | 1.004 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| oracle | clicks | iips | 0.7864 | +0.03675 ± 0.00094 | 0.03792 | 0.03 | 9895 | 1.001 | 99% | 0.03 |  | biased, as expected |
| oracle | clicks | sniips | 0.7864 | +0.03656 ± 0.00065 | 0.03713 | 0.00 | 9895 | 1.001 | 99% | 0.00 |  | biased, as expected |
| oracle | clicks | rips | 0.7864 | -0.00492 ± 0.00921 | 0.09172 | 0.57 | 3 | 0.927 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| oracle | clicks | snrips | 0.7864 | -0.00737 ± 0.00835 | 0.08342 | 0.56 | 3 | 0.927 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.7864 | +0.12940 ± 0.00025 | 0.12943 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.7864 | -0.06237 ± 0.00018 | 0.06240 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.7864 | +0.03669 ± 0.00056 | 0.03711 | 0.00 | 9895 | 1.001 | 99% | 0.00 |  | biased, as expected |
| oracle | clicks | cdr | 0.7864 | -0.00476 ± 0.00900 | 0.08968 | 0.61 | 3 | 0.927 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| oracle | satisfied | naive | 0.5853 | -0.28279 ± 0.00016 | 0.28279 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5853 | +0.01952 ± 0.04446 | 0.44280 | 0.87 | 3 | 1.004 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| oracle | satisfied | snips | 0.5853 | -0.00334 ± 0.03284 | 0.32676 | 0.64 | 3 | 1.004 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| oracle | satisfied | iips | 0.5853 | +0.01248 ± 0.00076 | 0.01458 | 0.60 | 9895 | 1.001 | 99% | 0.60 |  | biased, as expected |
| oracle | satisfied | sniips | 0.5853 | +0.01237 ± 0.00053 | 0.01344 | 0.34 | 9895 | 1.001 | 99% | 0.33 |  | biased, as expected |
| oracle | satisfied | rips | 0.5853 | -0.00208 ± 0.00224 | 0.02238 | 0.87 | 3 | 0.927 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| oracle | satisfied | snrips | 0.5853 | -0.00226 ± 0.00220 | 0.02197 | 0.79 | 3 | 0.927 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.5853 | +0.08894 ± 0.00026 | 0.08898 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5853 | -0.01575 ± 0.00021 | 0.01589 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5853 | +0.01245 ± 0.00047 | 0.01330 | 0.28 | 9895 | 1.001 | 99% | 0.27 |  | biased, as expected |
| oracle | satisfied | cdr | 0.5853 | -0.00199 ± 0.00219 | 0.02189 | 0.82 | 3 | 0.927 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| reverse | clicks | naive | 0.2302 | +0.26290 ± 0.00018 | 0.26291 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.2302 | -0.01244 ± 0.02617 | 0.26071 | 0.49 | 4 | 1.058 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.06; 0% of logs usable) |
| reverse | clicks | snips | 0.2302 | -0.02545 ± 0.02637 | 0.26358 | 0.44 | 4 | 1.058 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.06; 0% of logs usable) |
| reverse | clicks | iips | 0.2302 | -0.01066 ± 0.00049 | 0.01173 | 0.35 | 9896 | 1.002 | 96% | 0.33 |  | biased, as expected |
| reverse | clicks | sniips | 0.2302 | -0.01082 ± 0.00046 | 0.01176 | 0.34 | 9896 | 1.002 | 96% | 0.33 |  | biased, as expected |
| reverse | clicks | rips | 0.2302 | +0.00193 ± 0.01475 | 0.14678 | 0.68 | 4 | 1.024 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.02; 0% of logs usable) |
| reverse | clicks | snrips | 0.2302 | +0.00337 ± 0.01771 | 0.17629 | 0.65 | 4 | 1.024 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.02; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.2302 | +0.03453 ± 0.00019 | 0.03458 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.2302 | +0.01945 ± 0.00020 | 0.01956 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.2302 | -0.01091 ± 0.00045 | 0.01177 | 0.29 | 9896 | 1.002 | 96% | 0.27 |  | biased, as expected |
| reverse | clicks | cdr | 0.2302 | -0.00149 ± 0.01339 | 0.13322 | 0.77 | 4 | 1.024 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.02; 0% of logs usable) |
| reverse | satisfied | naive | 0.0613 | +0.24116 ± 0.00016 | 0.24117 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0613 | +0.01125 ± 0.01557 | 0.15531 | 0.20 | 4 | 1.058 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.06; 0% of logs usable) |
| reverse | satisfied | snips | 0.0613 | +0.00286 ± 0.01379 | 0.13721 | 0.20 | 4 | 1.058 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.06; 0% of logs usable) |
| reverse | satisfied | iips | 0.0613 | -0.00191 ± 0.00025 | 0.00311 | 0.88 | 9896 | 1.002 | 96% | 0.88 |  | biased, as expected |
| reverse | satisfied | sniips | 0.0613 | -0.00195 ± 0.00024 | 0.00311 | 0.89 | 9896 | 1.002 | 96% | 0.89 |  | biased, as expected |
| reverse | satisfied | rips | 0.0613 | +0.00717 ± 0.00918 | 0.09164 | 0.43 | 4 | 1.024 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.02; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0613 | +0.00221 ± 0.00784 | 0.07801 | 0.43 | 4 | 1.024 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.02; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0613 | +0.03718 ± 0.00010 | 0.03719 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0613 | +0.02611 ± 0.00010 | 0.02613 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0613 | -0.00205 ± 0.00022 | 0.00298 | 0.82 | 9896 | 1.002 | 96% | 0.82 |  | biased, as expected |
| reverse | satisfied | cdr | 0.0613 | +0.00519 ± 0.00722 | 0.07203 | 0.81 | 4 | 1.024 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.02; 0% of logs usable) |
| wide | clicks | naive | 0.7401 | -0.24699 ± 0.00018 | 0.24700 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.7401 | -0.48303 ± 0.02927 | 0.56403 | 0.55 | 1 | 0.330 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snips | 0.7401 | -0.19423 ± 0.05593 | 0.58937 | 0.13 | 1 | 0.330 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.7401 | -0.01397 ± 0.00083 | 0.01623 | 0.59 | 6417 | 0.642 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | sniips | 0.7401 | +0.07824 ± 0.00082 | 0.07867 | 0.00 | 6417 | 0.642 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | rips | 0.7401 | -0.07328 ± 0.00727 | 0.10295 | 0.48 | 1 | 0.473 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snrips | 0.7401 | +0.05424 ± 0.01800 | 0.18711 | 0.82 | 1 | 0.473 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.7401 | +0.13077 ± 0.00026 | 0.13079 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.7401 | -0.02487 ± 0.00017 | 0.02493 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.7401 | +0.08369 ± 0.00060 | 0.08390 | 0.00 | 6417 | 0.642 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | cdr | 0.7401 | -0.00186 ± 0.00692 | 0.06888 | 0.92 | 1 | 0.473 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | naive | 0.5368 | -0.23429 ± 0.00016 | 0.23430 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.5368 | -0.34325 ± 0.02413 | 0.41889 | 0.45 | 1 | 0.330 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | snips | 0.5368 | -0.12895 ± 0.05195 | 0.53275 | 0.16 | 1 | 0.330 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.5368 | -0.00134 ± 0.00069 | 0.00702 | 0.94 | 6417 | 0.642 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | sniips | 0.5368 | +0.05384 ± 0.00066 | 0.05424 | 0.00 | 6417 | 0.642 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | rips | 0.5368 | -0.02282 ± 0.00593 | 0.06329 | 0.62 | 1 | 0.473 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.5368 | +0.06262 ± 0.01590 | 0.17017 | 0.93 | 1 | 0.473 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | dm-pbm | 0.5368 | +0.09597 ± 0.00026 | 0.09600 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.5368 | +0.01023 ± 0.00021 | 0.01044 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.5368 | +0.05864 ± 0.00050 | 0.05885 | 0.00 | 6417 | 0.642 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | cdr | 0.5368 | +0.02022 ± 0.00528 | 0.05631 | 0.88 | 1 | 0.473 | 0% |  |  | unbiased though unsupported |

## scifact · dcm · {"policy":"swap","top":10}

100 logs of 100000 impressions, 11 s. Propensities: 10000000 lists, 0 mismatched; chi-square 4113 on 3900 df (p 0.009), largest |z| 3.68.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.7545 | 0.5526 | 0.5389 | 2.241 |
| rerank | 54.3% | 0.7526 | 0.5482 | 0.5341 | 2.243 |
| oracle | 5.8% | 0.7864 | 0.5853 | 0.5672 | 2.228 |
| reverse | 80.0% | 0.2302 | 0.0613 | 0.0597 | 2.499 |
| wide | 61.2% | 0.7401 | 0.5368 | 0.5232 | 2.248 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.7545 | -0.20432 ± 0.00018 | 0.20433 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.7545 | -0.00153 ± 0.00084 | 0.00845 | 0.96 | 9990 | 0.999 | 100% | 0.96 | yes | ok |
| ranking | clicks | snips | 0.7545 | -0.00074 ± 0.00048 | 0.00486 | 0.98 | 9990 | 0.999 | 100% | 0.98 | yes | ok |
| ranking | clicks | iips | 0.7545 | +0.02148 ± 0.00065 | 0.02244 | 0.11 | 9990 | 0.999 | 98% | 0.11 |  | biased, as expected |
| ranking | clicks | sniips | 0.7545 | +0.02201 ± 0.00050 | 0.02257 | 0.02 | 9990 | 0.999 | 98% | 0.02 |  | biased, as expected |
| ranking | clicks | rips | 0.7545 | -0.00153 ± 0.00084 | 0.00845 | 0.96 | 9990 | 0.999 | 100% | 0.96 | yes | ok |
| ranking | clicks | snrips | 0.7545 | -0.00074 ± 0.00048 | 0.00486 | 0.98 | 9990 | 0.999 | 100% | 0.98 | yes | ok |
| ranking | clicks | dm-pbm | 0.7545 | +0.07832 ± 0.00019 | 0.07834 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.7545 | -0.04371 ± 0.00017 | 0.04374 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.7545 | +0.02256 ± 0.00036 | 0.02284 | 0.00 | 9990 | 0.999 | 98% | 0.00 |  | biased, as expected |
| ranking | clicks | cdr | 0.7545 | -0.00064 ± 0.00043 | 0.00429 | 0.97 | 9990 | 0.999 | 100% | 0.97 | yes | ok |
| ranking | satisfied | naive | 0.5526 | -0.19664 ± 0.00016 | 0.19665 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.5526 | -0.00102 ± 0.00071 | 0.00715 | 0.94 | 9990 | 0.999 | 100% | 0.94 | yes | ok |
| ranking | satisfied | snips | 0.5526 | -0.00044 ± 0.00049 | 0.00490 | 0.95 | 9990 | 0.999 | 100% | 0.95 | yes | ok |
| ranking | satisfied | iips | 0.5526 | +0.00461 ± 0.00061 | 0.00762 | 0.91 | 9990 | 0.999 | 98% | 0.91 |  | biased, as expected |
| ranking | satisfied | sniips | 0.5526 | +0.00505 ± 0.00049 | 0.00704 | 0.83 | 9990 | 0.999 | 98% | 0.83 |  | biased, as expected |
| ranking | satisfied | rips | 0.5526 | -0.00102 ± 0.00071 | 0.00715 | 0.94 | 9990 | 0.999 | 100% | 0.94 | yes | ok |
| ranking | satisfied | snrips | 0.5526 | -0.00044 ± 0.00049 | 0.00490 | 0.95 | 9990 | 0.999 | 100% | 0.95 | yes | ok |
| ranking | satisfied | dm-pbm | 0.5526 | +0.05303 ± 0.00022 | 0.05307 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.5526 | -0.00912 ± 0.00020 | 0.00933 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.5526 | +0.00552 ± 0.00042 | 0.00692 | 0.72 | 9990 | 0.999 | 98% | 0.71 |  | biased, as expected |
| ranking | satisfied | cdr | 0.5526 | -0.00027 ± 0.00046 | 0.00463 | 0.95 | 9990 | 0.999 | 100% | 0.95 | yes | ok |
| rerank | clicks | naive | 0.7526 | -0.20244 ± 0.00018 | 0.20244 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.7526 | -0.73961 ± 0.00013 | 0.73961 | 0.00 | 200 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | snips | 0.7526 | -0.10344 ± 0.00403 | 0.11092 | 0.28 | 200 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | iips | 0.7526 | -0.18172 ± 0.00071 | 0.18186 | 0.00 | 8616 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | sniips | 0.7526 | +0.08501 ± 0.00087 | 0.08545 | 0.00 | 8616 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | rips | 0.7526 | -0.27850 ± 0.00068 | 0.27858 | 0.00 | 200 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | snrips | 0.7526 | -0.08912 ± 0.00169 | 0.09069 | 0.00 | 200 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | dm-pbm | 0.7526 | +0.09135 ± 0.00027 | 0.09139 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.7526 | -0.04449 ± 0.00021 | 0.04454 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.7526 | +0.04921 ± 0.00040 | 0.04937 | 0.00 | 8616 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | cdr | 0.7526 | -0.03860 ± 0.00035 | 0.03876 | 0.00 | 200 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | naive | 0.5482 | -0.19224 ± 0.00016 | 0.19225 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.5482 | -0.53909 ± 0.00009 | 0.53909 | 0.00 | 200 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | snips | 0.5482 | -0.09433 ± 0.00318 | 0.09950 | 0.26 | 200 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | iips | 0.5482 | -0.10331 ± 0.00063 | 0.10350 | 0.00 | 8616 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | sniips | 0.5482 | +0.06641 ± 0.00078 | 0.06687 | 0.00 | 8616 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | rips | 0.5482 | -0.17227 ± 0.00058 | 0.17237 | 0.00 | 200 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | snrips | 0.5482 | -0.07723 ± 0.00084 | 0.07768 | 0.00 | 200 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | dm-pbm | 0.5482 | +0.06534 ± 0.00025 | 0.06539 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.5482 | -0.00858 ± 0.00022 | 0.00885 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.5482 | +0.03228 ± 0.00042 | 0.03255 | 0.00 | 8616 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | cdr | 0.5482 | -0.00540 ± 0.00039 | 0.00667 | 0.72 | 200 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| oracle | clicks | naive | 0.7864 | -0.23626 ± 0.00018 | 0.23627 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.7864 | -0.13022 ± 0.00082 | 0.13048 | 0.00 | 8697 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | snips | 0.7864 | -0.03192 ± 0.00055 | 0.03239 | 0.00 | 8697 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | iips | 0.7864 | -0.00062 ± 0.00078 | 0.00775 | 0.93 | 9992 | 0.882 | 0% |  |  | unbiased though unsupported |
| oracle | clicks | sniips | 0.7864 | +0.00992 ± 0.00044 | 0.01086 | 0.46 | 9992 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | rips | 0.7864 | -0.02963 ± 0.00086 | 0.03085 | 0.08 | 8697 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | snrips | 0.7864 | -0.01351 ± 0.00050 | 0.01438 | 0.30 | 8697 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | dm-pbm | 0.7864 | +0.10374 ± 0.00021 | 0.10376 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.7864 | -0.05662 ± 0.00019 | 0.05665 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.7864 | +0.03073 ± 0.00038 | 0.03096 | 0.00 | 9992 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | cdr | 0.7864 | -0.01260 ± 0.00041 | 0.01323 | 0.27 | 8697 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | naive | 0.5853 | -0.22935 ± 0.00016 | 0.22935 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5853 | -0.10176 ± 0.00072 | 0.10201 | 0.00 | 8697 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | snips | 0.5853 | -0.02934 ± 0.00056 | 0.02987 | 0.00 | 8697 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | iips | 0.5853 | -0.01086 ± 0.00075 | 0.01315 | 0.69 | 9992 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | sniips | 0.5853 | -0.00881 ± 0.00049 | 0.01005 | 0.63 | 9992 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | rips | 0.5853 | -0.01642 ± 0.00077 | 0.01811 | 0.42 | 8697 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | snrips | 0.5853 | -0.01312 ± 0.00051 | 0.01406 | 0.27 | 8697 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | dm-pbm | 0.5853 | +0.07011 ± 0.00023 | 0.07015 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5853 | -0.01604 ± 0.00021 | 0.01617 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5853 | +0.00949 ± 0.00048 | 0.01064 | 0.46 | 9992 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | cdr | 0.5853 | -0.00602 ± 0.00049 | 0.00772 | 0.75 | 8697 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| reverse | clicks | naive | 0.2302 | +0.32001 ± 0.00018 | 0.32001 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.2302 | -0.23017 ± 0.00000 | 0.23017 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snips | 0.2302 | -0.23017 ± 0.00000 | 0.23017 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | iips | 0.2302 | -0.18361 ± 0.00020 | 0.18362 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | sniips | 0.2302 | -0.18360 ± 0.00021 | 0.18361 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | rips | 0.2302 | -0.18361 ± 0.00020 | 0.18362 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snrips | 0.2302 | -0.18360 ± 0.00021 | 0.18361 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | dm-pbm | 0.2302 | +0.12615 ± 0.00035 | 0.12620 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.2302 | +0.02523 ± 0.00037 | 0.02549 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.2302 | +0.08785 ± 0.00037 | 0.08793 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | cdr | 0.2302 | +0.01609 ± 0.00038 | 0.01653 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | naive | 0.0613 | +0.29460 ± 0.00016 | 0.29461 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0613 | -0.06133 ± 0.00000 | 0.06133 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snips | 0.0613 | -0.06133 ± 0.00000 | 0.06133 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | iips | 0.0613 | -0.04921 ± 0.00011 | 0.04922 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | sniips | 0.0613 | -0.04921 ± 0.00011 | 0.04922 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | rips | 0.0613 | -0.04921 ± 0.00011 | 0.04922 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snrips | 0.0613 | -0.04921 ± 0.00011 | 0.04922 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | dm-pbm | 0.0613 | +0.11669 ± 0.00018 | 0.11670 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0613 | +0.05469 ± 0.00018 | 0.05471 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0613 | +0.08650 ± 0.00020 | 0.08653 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | cdr | 0.0613 | +0.04139 ± 0.00020 | 0.04144 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| wide | clicks | naive | 0.7401 | -0.18989 ± 0.00018 | 0.18990 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.7401 | -0.72766 ± 0.00013 | 0.72766 | 0.00 | 167 | 0.017 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | snips | 0.7401 | +0.00089 ± 0.00430 | 0.04282 | 0.94 | 167 | 0.017 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.7401 | -0.18142 ± 0.00073 | 0.18157 | 0.00 | 4249 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | sniips | 0.7401 | +0.18448 ± 0.00114 | 0.18483 | 0.00 | 4249 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | rips | 0.7401 | -0.27044 ± 0.00068 | 0.27052 | 0.00 | 167 | 0.017 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | snrips | 0.7401 | -0.05333 ± 0.00187 | 0.05649 | 0.15 | 167 | 0.017 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | dm-pbm | 0.7401 | +0.14696 ± 0.00027 | 0.14698 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.7401 | -0.00901 ± 0.00020 | 0.00923 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.7401 | +0.10661 ± 0.00041 | 0.10669 | 0.00 | 4249 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | cdr | 0.7401 | -0.00419 ± 0.00034 | 0.00540 | 0.81 | 167 | 0.017 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | naive | 0.5368 | -0.18086 ± 0.00016 | 0.18086 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.5368 | -0.52777 ± 0.00009 | 0.52777 | 0.00 | 167 | 0.017 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | snips | 0.5368 | +0.00226 ± 0.00350 | 0.03486 | 0.97 | 167 | 0.017 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.5368 | -0.09456 ± 0.00063 | 0.09477 | 0.00 | 4249 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | sniips | 0.5368 | +0.15389 ± 0.00099 | 0.15421 | 0.00 | 4249 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | rips | 0.5368 | -0.16198 ± 0.00058 | 0.16208 | 0.00 | 167 | 0.017 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | snrips | 0.5368 | -0.04442 ± 0.00093 | 0.04537 | 0.01 | 167 | 0.017 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.5368 | +0.11464 ± 0.00026 | 0.11467 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.5368 | +0.02093 ± 0.00022 | 0.02105 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.5368 | +0.08334 ± 0.00044 | 0.08345 | 0.00 | 4249 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | cdr | 0.5368 | +0.02428 ± 0.00039 | 0.02459 | 0.00 | 167 | 0.017 | 0% |  |  | biased, as expected: 61% of its slots unsupported |

## scifact · dbn · {"policy":"softmax","tau":1.0,"top":10,"by":"rank"}

100 logs of 100000 impressions, 15 s. Propensities: 10000000 lists, 0 mismatched; chi-square 13135 on 13500 df (p 0.987), largest |z| 4.23.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.6528 | 0.5092 | 0.5092 | 2.255 |
| rerank | 0.0% | 0.6332 | 0.4883 | 0.4883 | 2.265 |
| oracle | 0.0% | 0.7075 | 0.5645 | 0.5645 | 2.229 |
| reverse | 0.0% | 0.1690 | 0.0433 | 0.0433 | 2.510 |
| wide | 20.3% | 0.6243 | 0.4800 | 0.4800 | 2.269 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.6528 | -0.10835 ± 0.00017 | 0.10837 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.6528 | -0.00525 ± 0.00882 | 0.08793 | 0.91 | 109 | 0.982 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snips | 0.6528 | +0.00578 ± 0.00567 | 0.05671 | 0.87 | 109 | 0.982 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | iips | 0.6528 | +0.01634 ± 0.00052 | 0.01715 | 0.05 | 11565 | 0.998 | 100% | 0.05 |  | biased, as expected |
| ranking | clicks | sniips | 0.6528 | +0.01661 ± 0.00046 | 0.01722 | 0.01 | 11565 | 0.998 | 100% | 0.01 |  | biased, as expected |
| ranking | clicks | rips | 0.6528 | +0.00216 ± 0.00145 | 0.01456 | 0.96 | 109 | 0.980 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snrips | 0.6528 | +0.00287 ± 0.00144 | 0.01461 | 0.94 | 109 | 0.980 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | dm-pbm | 0.6528 | +0.05595 ± 0.00022 | 0.05599 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.6528 | -0.07070 ± 0.00019 | 0.07072 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.6528 | +0.01666 ± 0.00040 | 0.01712 | 0.00 | 11565 | 0.998 | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | cdr | 0.6528 | +0.00295 ± 0.00130 | 0.01329 | 0.97 | 109 | 0.980 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | naive | 0.5092 | -0.11021 ± 0.00016 | 0.11023 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.5092 | -0.00831 ± 0.00761 | 0.07619 | 0.94 | 109 | 0.982 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snips | 0.5092 | +0.00017 ± 0.00554 | 0.05515 | 0.90 | 109 | 0.982 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | iips | 0.5092 | +0.00416 ± 0.00042 | 0.00590 | 0.81 | 11565 | 0.998 | 100% | 0.81 |  | biased, as expected |
| ranking | satisfied | sniips | 0.5092 | +0.00437 ± 0.00037 | 0.00570 | 0.77 | 11565 | 0.998 | 100% | 0.77 |  | biased, as expected |
| ranking | satisfied | rips | 0.5092 | +0.00076 ± 0.00107 | 0.01067 | 0.92 | 109 | 0.980 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snrips | 0.5092 | +0.00104 ± 0.00106 | 0.01057 | 0.89 | 109 | 0.980 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | dm-pbm | 0.5092 | +0.03704 ± 0.00020 | 0.03709 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.5092 | -0.05683 ± 0.00017 | 0.05686 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.5092 | +0.00439 ± 0.00031 | 0.00539 | 0.69 | 11565 | 0.998 | 100% | 0.69 |  | biased, as expected |
| ranking | satisfied | cdr | 0.5092 | +0.00124 ± 0.00084 | 0.00846 | 0.96 | 109 | 0.980 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| rerank | clicks | naive | 0.6332 | -0.08877 ± 0.00017 | 0.08878 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.6332 | -0.00960 ± 0.03743 | 0.37256 | 0.84 | 18 | 0.990 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.99; 1% of logs usable) |
| rerank | clicks | snips | 0.6332 | -0.00297 ± 0.01239 | 0.12331 | 0.93 | 18 | 0.990 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.99; 1% of logs usable) |
| rerank | clicks | iips | 0.6332 | +0.01479 ± 0.00050 | 0.01561 | 0.20 | 9464 | 1.000 | 99% | 0.20 |  | biased, as expected |
| rerank | clicks | sniips | 0.6332 | +0.01477 ± 0.00050 | 0.01557 | 0.19 | 9464 | 1.000 | 99% | 0.19 |  | biased, as expected |
| rerank | clicks | rips | 0.6332 | -0.00012 ± 0.00333 | 0.03317 | 0.89 | 18 | 0.903 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.90; 1% of logs usable) |
| rerank | clicks | snrips | 0.6332 | +0.00236 ± 0.00377 | 0.03760 | 0.90 | 18 | 0.903 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.90; 1% of logs usable) |
| rerank | clicks | dm-pbm | 0.6332 | +0.05333 ± 0.00024 | 0.05338 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.6332 | -0.05134 ± 0.00020 | 0.05138 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.6332 | +0.01494 ± 0.00042 | 0.01551 | 0.06 | 9464 | 1.000 | 99% | 0.06 |  | biased, as expected |
| rerank | clicks | cdr | 0.6332 | +0.00308 ± 0.00326 | 0.03255 | 0.97 | 18 | 0.903 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.90; 1% of logs usable) |
| rerank | satisfied | naive | 0.4883 | -0.08934 ± 0.00016 | 0.08935 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.4883 | -0.02045 ± 0.01982 | 0.19822 | 0.86 | 18 | 0.990 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.99; 1% of logs usable) |
| rerank | satisfied | snips | 0.4883 | +0.00472 ± 0.01309 | 0.13037 | 0.89 | 18 | 0.990 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.99; 1% of logs usable) |
| rerank | satisfied | iips | 0.4883 | +0.00445 ± 0.00047 | 0.00648 | 0.81 | 9464 | 1.000 | 99% | 0.81 |  | biased, as expected |
| rerank | satisfied | sniips | 0.4883 | +0.00442 ± 0.00047 | 0.00643 | 0.76 | 9464 | 1.000 | 99% | 0.77 |  | biased, as expected |
| rerank | satisfied | rips | 0.4883 | +0.00358 ± 0.00309 | 0.03097 | 0.90 | 18 | 0.903 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.90; 1% of logs usable) |
| rerank | satisfied | snrips | 0.4883 | +0.00522 ± 0.00364 | 0.03658 | 0.89 | 18 | 0.903 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.90; 1% of logs usable) |
| rerank | satisfied | dm-pbm | 0.4883 | +0.03770 ± 0.00021 | 0.03776 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.4883 | -0.03792 ± 0.00018 | 0.03796 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.4883 | +0.00453 ± 0.00038 | 0.00588 | 0.68 | 9464 | 1.000 | 99% | 0.68 |  | biased, as expected |
| rerank | satisfied | cdr | 0.4883 | +0.00631 ± 0.00279 | 0.02844 | 0.98 | 18 | 0.903 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.90; 1% of logs usable) |
| oracle | clicks | naive | 0.7075 | -0.16310 ± 0.00017 | 0.16310 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.7075 | -0.00893 ± 0.01069 | 0.10671 | 0.92 | 90 | 0.983 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snips | 0.7075 | +0.00254 ± 0.00601 | 0.05982 | 0.90 | 90 | 0.983 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | iips | 0.7075 | +0.03198 ± 0.00057 | 0.03248 | 0.00 | 11628 | 0.999 | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | sniips | 0.7075 | +0.03214 ± 0.00044 | 0.03244 | 0.00 | 11628 | 0.999 | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | rips | 0.7075 | +0.00068 ± 0.00115 | 0.01151 | 0.93 | 90 | 0.972 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snrips | 0.7075 | +0.00118 ± 0.00119 | 0.01189 | 0.92 | 90 | 0.972 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | dm-pbm | 0.7075 | +0.08108 ± 0.00024 | 0.08112 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.7075 | -0.10852 ± 0.00019 | 0.10853 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.7075 | +0.03207 ± 0.00041 | 0.03233 | 0.00 | 11628 | 0.999 | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | cdr | 0.7075 | +0.00132 ± 0.00120 | 0.01199 | 0.91 | 90 | 0.972 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | naive | 0.5645 | -0.16557 ± 0.00016 | 0.16558 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5645 | -0.01297 ± 0.00874 | 0.08797 | 0.94 | 90 | 0.983 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snips | 0.5645 | -0.00390 ± 0.00540 | 0.05390 | 0.93 | 90 | 0.983 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | iips | 0.5645 | +0.01225 ± 0.00052 | 0.01328 | 0.38 | 11628 | 0.999 | 100% | 0.38 |  | biased, as expected |
| oracle | satisfied | sniips | 0.5645 | +0.01237 ± 0.00038 | 0.01293 | 0.06 | 11628 | 0.999 | 100% | 0.06 |  | biased, as expected |
| oracle | satisfied | rips | 0.5645 | -0.00033 ± 0.00067 | 0.00668 | 0.90 | 90 | 0.972 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snrips | 0.5645 | -0.00020 ± 0.00058 | 0.00582 | 0.89 | 90 | 0.972 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | dm-pbm | 0.5645 | +0.05165 ± 0.00022 | 0.05170 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5645 | -0.09281 ± 0.00018 | 0.09282 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5645 | +0.01234 ± 0.00034 | 0.01278 | 0.04 | 11628 | 0.999 | 100% | 0.04 |  | biased, as expected |
| oracle | satisfied | cdr | 0.5645 | +0.00001 ± 0.00057 | 0.00571 | 0.93 | 90 | 0.972 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| reverse | clicks | naive | 0.1690 | +0.37541 ± 0.00017 | 0.37541 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.1690 | -0.16903 ± 0.00000 | 0.16903 | 0.00 | 0 | 0.433 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.43; 0% of logs usable) |
| reverse | clicks | snips | 0.1690 | -0.16903 ± 0.00000 | 0.16903 | 0.00 | 0 | 0.433 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.43; 0% of logs usable) |
| reverse | clicks | iips | 0.1690 | -0.03388 ± 0.00053 | 0.03429 | 0.00 | 3425 | 1.003 | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | sniips | 0.1690 | -0.03402 ± 0.00051 | 0.03439 | 0.00 | 3425 | 1.003 | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | rips | 0.1690 | -0.02391 ± 0.03033 | 0.30268 | 0.21 | 0 | 0.059 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.06; 0% of logs usable) |
| reverse | clicks | snrips | 0.1690 | -0.04512 ± 0.01220 | 0.12946 | 0.20 | 0 | 0.059 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.06; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.1690 | +0.04200 ± 0.00023 | 0.04206 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.1690 | +0.03141 ± 0.00028 | 0.03153 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.1690 | -0.03433 ± 0.00048 | 0.03465 | 0.00 | 3425 | 1.003 | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | cdr | 0.1690 | +0.01486 ± 0.03097 | 0.30848 | 1.00 | 0 | 0.059 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.06; 0% of logs usable) |
| reverse | satisfied | naive | 0.0433 | +0.35568 ± 0.00016 | 0.35568 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0433 | -0.04329 ± 0.00000 | 0.04329 | 0.00 | 0 | 0.433 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.43; 0% of logs usable) |
| reverse | satisfied | snips | 0.0433 | -0.04329 ± 0.00000 | 0.04329 | 0.00 | 0 | 0.433 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.43; 0% of logs usable) |
| reverse | satisfied | iips | 0.0433 | -0.00513 ± 0.00027 | 0.00578 | 0.53 | 3425 | 1.003 | 100% | 0.53 |  | biased, as expected |
| reverse | satisfied | sniips | 0.0433 | -0.00517 ± 0.00026 | 0.00580 | 0.56 | 3425 | 1.003 | 100% | 0.56 |  | biased, as expected |
| reverse | satisfied | rips | 0.0433 | -0.01818 ± 0.00321 | 0.03679 | 0.25 | 0 | 0.059 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.06; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0433 | -0.01792 ± 0.00354 | 0.03951 | 0.23 | 0 | 0.059 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.06; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0433 | +0.06609 ± 0.00010 | 0.06610 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0433 | +0.04956 ± 0.00013 | 0.04958 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0433 | -0.00544 ± 0.00025 | 0.00598 | 0.42 | 3425 | 1.003 | 100% | 0.42 |  | biased, as expected |
| reverse | satisfied | cdr | 0.0433 | +0.00480 ± 0.00461 | 0.04612 | 0.63 | 0 | 0.059 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.06; 0% of logs usable) |
| wide | clicks | naive | 0.6243 | -0.07986 ± 0.00017 | 0.07988 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.6243 | -0.41616 ± 0.00916 | 0.42602 | 0.07 | 11 | 0.329 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snips | 0.6243 | +0.01605 ± 0.01611 | 0.16107 | 0.84 | 11 | 0.329 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.6243 | -0.00340 ± 0.00049 | 0.00596 | 0.91 | 6574 | 0.642 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | sniips | 0.6243 | +0.05223 ± 0.00056 | 0.05253 | 0.00 | 6574 | 0.642 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | rips | 0.6243 | -0.03124 ± 0.00258 | 0.04044 | 0.44 | 11 | 0.420 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snrips | 0.6243 | +0.04624 ± 0.00513 | 0.06885 | 0.86 | 11 | 0.420 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | dm-pbm | 0.6243 | +0.10789 ± 0.00025 | 0.10792 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.6243 | +0.00161 ± 0.00020 | 0.00257 | 0.35 |  |  | 100% | 0.35 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.6243 | +0.07797 ± 0.00043 | 0.07809 | 0.00 | 6574 | 0.642 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | cdr | 0.6243 | +0.06382 ± 0.00232 | 0.06785 | 0.06 | 11 | 0.420 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | naive | 0.4800 | -0.08106 ± 0.00016 | 0.08108 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.4800 | -0.31810 ± 0.00787 | 0.32761 | 0.06 | 11 | 0.329 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | snips | 0.4800 | +0.02385 ± 0.01645 | 0.16543 | 0.84 | 11 | 0.329 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.4800 | +0.00014 ± 0.00045 | 0.00445 | 0.92 | 6574 | 0.642 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | sniips | 0.4800 | +0.03747 ± 0.00050 | 0.03780 | 0.00 | 6574 | 0.642 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | rips | 0.4800 | -0.01098 ± 0.00227 | 0.02511 | 0.70 | 11 | 0.420 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | snrips | 0.4800 | +0.04404 ± 0.00441 | 0.06220 | 0.77 | 11 | 0.420 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.4800 | +0.08590 ± 0.00022 | 0.08593 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.4800 | +0.00771 ± 0.00019 | 0.00794 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.4800 | +0.05975 ± 0.00037 | 0.05986 | 0.00 | 6574 | 0.642 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | cdr | 0.4800 | +0.05592 ± 0.00199 | 0.05934 | 0.05 | 11 | 0.420 | 0% |  |  | biased, as expected: 20% of its slots unsupported |

## scifact · dbn · {"policy":"epsilon","epsilon":0.3,"top":10}

100 logs of 100000 impressions, 14 s. Propensities: 10000000 lists, 0 mismatched; chi-square 13384 on 13500 df (p 0.760), largest |z| 5.26.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.6528 | 0.5092 | 0.5092 | 2.255 |
| rerank | 0.0% | 0.6332 | 0.4883 | 0.4883 | 2.265 |
| oracle | 0.0% | 0.7075 | 0.5645 | 0.5645 | 2.229 |
| reverse | 0.0% | 0.1690 | 0.0433 | 0.0433 | 2.510 |
| wide | 20.3% | 0.6243 | 0.4800 | 0.4800 | 2.269 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.6528 | -0.08722 ± 0.00017 | 0.08724 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.6528 | -0.00010 ± 0.00024 | 0.00241 | 0.95 | 69999 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | clicks | snips | 0.6528 | -0.00008 ± 0.00020 | 0.00203 | 0.95 | 69999 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | clicks | iips | 0.6528 | +0.00160 ± 0.00023 | 0.00278 | 0.88 | 72936 | 1.000 | 99% | 0.88 |  | unbiased here |
| ranking | clicks | sniips | 0.6528 | +0.00156 ± 0.00020 | 0.00253 | 0.85 | 72936 | 1.000 | 99% | 0.85 |  | unbiased here |
| ranking | clicks | rips | 0.6528 | -0.00000 ± 0.00023 | 0.00231 | 0.95 | 69999 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | clicks | snrips | 0.6528 | -0.00005 ± 0.00020 | 0.00200 | 0.94 | 69999 | 1.000 | 100% | 0.94 | yes | ok |
| ranking | clicks | dm-pbm | 0.6528 | +0.01503 ± 0.00018 | 0.01514 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.6528 | -0.01859 ± 0.00017 | 0.01867 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.6528 | +0.00153 ± 0.00019 | 0.00245 | 0.88 | 72936 | 1.000 | 99% | 0.88 |  | unbiased here |
| ranking | clicks | cdr | 0.6528 | -0.00007 ± 0.00019 | 0.00191 | 0.93 | 69999 | 1.000 | 100% | 0.93 | yes | ok |
| ranking | satisfied | naive | 0.5092 | -0.08556 ± 0.00015 | 0.08558 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.5092 | -0.00029 ± 0.00021 | 0.00213 | 0.93 | 69999 | 1.000 | 100% | 0.93 | yes | ok |
| ranking | satisfied | snips | 0.5092 | -0.00027 ± 0.00019 | 0.00189 | 0.96 | 69999 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | satisfied | iips | 0.5092 | +0.00017 ± 0.00020 | 0.00198 | 0.95 | 72936 | 1.000 | 99% | 0.95 |  | unbiased here |
| ranking | satisfied | sniips | 0.5092 | +0.00014 ± 0.00018 | 0.00182 | 0.98 | 72936 | 1.000 | 99% | 0.98 |  | unbiased here |
| ranking | satisfied | rips | 0.5092 | -0.00020 ± 0.00020 | 0.00204 | 0.95 | 69999 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | satisfied | snrips | 0.5092 | -0.00024 ± 0.00019 | 0.00188 | 0.96 | 69999 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | satisfied | dm-pbm | 0.5092 | +0.01165 ± 0.00016 | 0.01176 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.5092 | -0.00945 ± 0.00016 | 0.00958 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.5092 | +0.00012 ± 0.00017 | 0.00171 | 0.96 | 72936 | 1.000 | 99% | 0.96 |  | unbiased here |
| ranking | satisfied | cdr | 0.5092 | -0.00026 ± 0.00018 | 0.00179 | 0.95 | 69999 | 1.000 | 100% | 0.95 | yes | ok |
| rerank | clicks | naive | 0.6332 | -0.06764 ± 0.00017 | 0.06766 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.6332 | -0.02618 ± 0.07164 | 0.71325 | 0.48 | 92 | 1.215 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 92 impressions, mean 1.21; 0% of logs usable) |
| rerank | clicks | snips | 0.6332 | -0.11996 ± 0.04134 | 0.42848 | 0.16 | 92 | 1.215 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 92 impressions, mean 1.21; 0% of logs usable) |
| rerank | clicks | iips | 0.6332 | +0.02392 ± 0.00098 | 0.02585 | 0.23 | 3633 | 0.997 | 99% | 0.22 |  | biased, as expected |
| rerank | clicks | sniips | 0.6332 | +0.02397 ± 0.00095 | 0.02577 | 0.27 | 3633 | 0.997 | 99% | 0.27 |  | biased, as expected |
| rerank | clicks | rips | 0.6332 | -0.01600 ± 0.01179 | 0.11844 | 0.79 | 3 | 0.891 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.89; 0% of logs usable) |
| rerank | clicks | snrips | 0.6332 | -0.01708 ± 0.01167 | 0.11732 | 0.77 | 3 | 0.891 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.89; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.6332 | +0.02021 ± 0.00024 | 0.02035 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.6332 | +0.00373 ± 0.00020 | 0.00424 | 0.14 |  |  | 100% | 0.14 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.6332 | +0.02256 ± 0.00075 | 0.02376 | 0.11 | 3633 | 0.997 | 99% | 0.11 |  | biased, as expected |
| rerank | clicks | cdr | 0.6332 | -0.01522 ± 0.01233 | 0.12365 | 0.96 | 3 | 0.891 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.89; 0% of logs usable) |
| rerank | satisfied | naive | 0.4883 | -0.06469 ± 0.00015 | 0.06470 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.4883 | -0.01302 ± 0.06480 | 0.64486 | 0.40 | 92 | 1.215 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 92 impressions, mean 1.21; 0% of logs usable) |
| rerank | satisfied | snips | 0.4883 | -0.09948 ± 0.03833 | 0.39413 | 0.16 | 92 | 1.215 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 92 impressions, mean 1.21; 0% of logs usable) |
| rerank | satisfied | iips | 0.4883 | +0.00674 ± 0.00078 | 0.01029 | 0.81 | 3633 | 0.997 | 99% | 0.81 |  | biased, as expected |
| rerank | satisfied | sniips | 0.4883 | +0.00676 ± 0.00075 | 0.01009 | 0.83 | 3633 | 0.997 | 99% | 0.83 |  | biased, as expected |
| rerank | satisfied | rips | 0.4883 | -0.01484 ± 0.00608 | 0.06225 | 0.76 | 3 | 0.891 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.89; 0% of logs usable) |
| rerank | satisfied | snrips | 0.4883 | -0.01770 ± 0.00608 | 0.06308 | 0.77 | 3 | 0.891 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.89; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.4883 | +0.01454 ± 0.00018 | 0.01466 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.4883 | +0.01206 ± 0.00018 | 0.01219 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.4883 | +0.00564 ± 0.00057 | 0.00803 | 0.82 | 3633 | 0.997 | 99% | 0.82 |  | biased, as expected |
| rerank | satisfied | cdr | 0.4883 | -0.01020 ± 0.00554 | 0.05611 | 0.95 | 3 | 0.891 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.89; 0% of logs usable) |
| oracle | clicks | naive | 0.7075 | -0.14197 ± 0.00017 | 0.14198 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.7075 | +0.04330 ± 0.04986 | 0.49794 | 0.21 | 41111 | 1.035 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 41111 impressions, mean 1.03; 0% of logs usable) |
| oracle | clicks | snips | 0.7075 | -0.00752 ± 0.01051 | 0.10481 | 0.18 | 41111 | 1.035 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 41111 impressions, mean 1.03; 0% of logs usable) |
| oracle | clicks | iips | 0.7075 | +0.02080 ± 0.00082 | 0.02234 | 0.27 | 11343 | 1.000 | 99% | 0.26 |  | biased, as expected |
| oracle | clicks | sniips | 0.7075 | +0.02010 ± 0.00056 | 0.02084 | 0.02 | 11343 | 1.000 | 99% | 0.02 |  | biased, as expected |
| oracle | clicks | rips | 0.7075 | -0.00015 ± 0.00198 | 0.01971 | 0.95 | 71 | 0.875 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 71 impressions, mean 0.87; 0% of logs usable) |
| oracle | clicks | snrips | 0.7075 | +0.00030 ± 0.00176 | 0.01754 | 0.90 | 71 | 0.875 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 71 impressions, mean 0.87; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.7075 | +0.06145 ± 0.00020 | 0.06148 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.7075 | -0.05704 ± 0.00018 | 0.05707 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.7075 | +0.01974 ± 0.00053 | 0.02044 | 0.03 | 11343 | 1.000 | 99% | 0.03 |  | biased, as expected |
| oracle | clicks | cdr | 0.7075 | -0.00079 ± 0.00200 | 0.01989 | 0.94 | 71 | 0.875 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 71 impressions, mean 0.87; 0% of logs usable) |
| oracle | satisfied | naive | 0.5645 | -0.14092 ± 0.00015 | 0.14093 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5645 | +0.06826 ± 0.04932 | 0.49545 | 0.20 | 41111 | 1.035 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 41111 impressions, mean 1.03; 0% of logs usable) |
| oracle | satisfied | snips | 0.5645 | +0.00765 ± 0.01313 | 0.13085 | 0.00 | 41111 | 1.035 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 41111 impressions, mean 1.03; 0% of logs usable) |
| oracle | satisfied | iips | 0.5645 | +0.01124 ± 0.00072 | 0.01334 | 0.73 | 11343 | 1.000 | 99% | 0.73 |  | biased, as expected |
| oracle | satisfied | sniips | 0.5645 | +0.01064 ± 0.00054 | 0.01190 | 0.44 | 11343 | 1.000 | 99% | 0.44 |  | biased, as expected |
| oracle | satisfied | rips | 0.5645 | +0.00061 ± 0.00090 | 0.00902 | 0.96 | 71 | 0.875 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 71 impressions, mean 0.87; 0% of logs usable) |
| oracle | satisfied | snrips | 0.5645 | +0.00018 ± 0.00068 | 0.00675 | 0.97 | 71 | 0.875 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 71 impressions, mean 0.87; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.5645 | +0.04513 ± 0.00019 | 0.04517 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5645 | -0.04584 ± 0.00017 | 0.04587 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5645 | +0.01028 ± 0.00052 | 0.01152 | 0.44 | 11343 | 1.000 | 99% | 0.44 |  | biased, as expected |
| oracle | satisfied | cdr | 0.5645 | +0.00036 ± 0.00065 | 0.00646 | 0.98 | 71 | 0.875 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 71 impressions, mean 0.87; 0% of logs usable) |
| reverse | clicks | naive | 0.1690 | +0.39654 ± 0.00017 | 0.39654 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.1690 | +0.05273 ± 0.04660 | 0.46667 | 0.20 | 1 | 1.008 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.01; 0% of logs usable) |
| reverse | clicks | snips | 0.1690 | -0.01069 ± 0.03650 | 0.36330 | 0.10 | 1 | 1.008 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.01; 0% of logs usable) |
| reverse | clicks | iips | 0.1690 | -0.01412 ± 0.00068 | 0.01565 | 0.48 | 2945 | 1.000 | 99% | 0.48 |  | biased, as expected |
| reverse | clicks | sniips | 0.1690 | -0.01422 ± 0.00063 | 0.01556 | 0.46 | 2945 | 1.000 | 99% | 0.45 |  | biased, as expected |
| reverse | clicks | rips | 0.1690 | +0.02036 ± 0.02179 | 0.21779 | 0.61 | 1 | 0.750 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.75; 0% of logs usable) |
| reverse | clicks | snrips | 0.1690 | +0.01240 ± 0.01779 | 0.17748 | 0.56 | 1 | 0.750 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.75; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.1690 | +0.10068 ± 0.00027 | 0.10071 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.1690 | +0.05283 ± 0.00027 | 0.05290 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.1690 | -0.01460 ± 0.00062 | 0.01586 | 0.41 | 2945 | 1.000 | 99% | 0.40 |  | biased, as expected |
| reverse | clicks | cdr | 0.1690 | +0.02319 ± 0.02207 | 0.22083 | 0.99 | 1 | 0.750 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.75; 0% of logs usable) |
| reverse | satisfied | naive | 0.0433 | +0.38033 ± 0.00015 | 0.38033 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0433 | -0.02313 ± 0.01418 | 0.14300 | 0.02 | 1 | 1.008 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.01; 0% of logs usable) |
| reverse | satisfied | snips | 0.0433 | -0.02996 ± 0.01051 | 0.10877 | 0.01 | 1 | 1.008 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.01; 0% of logs usable) |
| reverse | satisfied | iips | 0.0433 | -0.00277 ± 0.00036 | 0.00453 | 0.89 | 2945 | 1.000 | 99% | 0.89 |  | biased, as expected |
| reverse | satisfied | sniips | 0.0433 | -0.00278 ± 0.00035 | 0.00449 | 0.89 | 2945 | 1.000 | 99% | 0.89 |  | biased, as expected |
| reverse | satisfied | rips | 0.0433 | -0.01444 ± 0.00285 | 0.03183 | 0.25 | 1 | 0.750 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.75; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0433 | -0.01542 ± 0.00239 | 0.02832 | 0.25 | 1 | 0.750 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.75; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0433 | +0.10648 ± 0.00014 | 0.10649 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0433 | +0.07093 ± 0.00015 | 0.07095 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0433 | -0.00320 ± 0.00034 | 0.00469 | 0.86 | 2945 | 1.000 | 99% | 0.86 |  | biased, as expected |
| reverse | satisfied | cdr | 0.0433 | -0.01300 ± 0.00766 | 0.07730 | 0.93 | 1 | 0.750 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.75; 0% of logs usable) |
| wide | clicks | naive | 0.6243 | -0.05873 ± 0.00017 | 0.05875 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.6243 | -0.41039 ± 0.04368 | 0.59775 | 0.20 | 224 | 0.418 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snips | 0.6243 | -0.15284 ± 0.03394 | 0.37064 | 0.01 | 224 | 0.418 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | iips | 0.6243 | +0.00277 ± 0.00086 | 0.00901 | 0.93 | 2376 | 0.641 | 0% |  |  | unbiased though unsupported |
| wide | clicks | sniips | 0.6243 | +0.06132 ± 0.00098 | 0.06209 | 0.00 | 2376 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | rips | 0.6243 | -0.03204 ± 0.01172 | 0.12094 | 0.67 | 4 | 0.255 | 0% |  |  | unbiased though unsupported |
| wide | clicks | snrips | 0.6243 | +0.03072 ± 0.01432 | 0.14573 | 0.91 | 4 | 0.255 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.6243 | +0.05599 ± 0.00025 | 0.05604 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.6243 | +0.05735 ± 0.00019 | 0.05738 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.6243 | +0.06388 ± 0.00063 | 0.06419 | 0.00 | 2376 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | cdr | 0.6243 | +0.06184 ± 0.01180 | 0.13270 | 0.60 | 4 | 0.255 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | naive | 0.4800 | -0.05641 ± 0.00015 | 0.05643 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.4800 | -0.33739 ± 0.03515 | 0.48596 | 0.14 | 224 | 0.418 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | snips | 0.4800 | -0.16045 ± 0.02928 | 0.33261 | 0.01 | 224 | 0.418 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | iips | 0.4800 | +0.00171 ± 0.00074 | 0.00755 | 0.92 | 2376 | 0.641 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | sniips | 0.4800 | +0.03960 ± 0.00085 | 0.04048 | 0.00 | 2376 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | rips | 0.4800 | -0.02163 ± 0.00562 | 0.05994 | 0.72 | 4 | 0.255 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.4800 | +0.01480 ± 0.00836 | 0.08446 | 0.87 | 4 | 0.255 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | dm-pbm | 0.4800 | +0.04934 ± 0.00020 | 0.04938 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.4800 | +0.05828 ± 0.00018 | 0.05831 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.4800 | +0.04742 ± 0.00055 | 0.04774 | 0.00 | 2376 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | cdr | 0.4800 | +0.04763 ± 0.00519 | 0.07022 | 0.51 | 4 | 0.255 | 0% |  |  | biased, as expected: 20% of its slots unsupported |

## scifact · dbn · {"policy":"shuffle","top":10}

100 logs of 100000 impressions, 13 s. Propensities: 10000000 lists, 0 mismatched; chi-square 13461 on 13500 df (p 0.591), largest |z| 3.75.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.6528 | 0.5092 | 0.5092 | 2.255 |
| rerank | 0.0% | 0.6332 | 0.4883 | 0.4883 | 2.265 |
| oracle | 0.0% | 0.7075 | 0.5645 | 0.5645 | 2.229 |
| reverse | 0.0% | 0.1690 | 0.0433 | 0.0433 | 2.510 |
| wide | 20.3% | 0.6243 | 0.4800 | 0.4800 | 2.269 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.6528 | -0.29058 ± 0.00017 | 0.29058 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.6528 | +0.04273 ± 0.05149 | 0.51408 | 0.86 | 3 | 1.040 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.04; 0% of logs usable) |
| ranking | clicks | snips | 0.6528 | -0.03367 ± 0.03637 | 0.36348 | 0.64 | 3 | 1.040 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.04; 0% of logs usable) |
| ranking | clicks | iips | 0.6528 | +0.03853 ± 0.00089 | 0.03954 | 0.00 | 9889 | 1.001 | 99% | 0.00 |  | biased, as expected |
| ranking | clicks | sniips | 0.6528 | +0.03894 ± 0.00067 | 0.03951 | 0.00 | 9889 | 1.001 | 99% | 0.00 |  | biased, as expected |
| ranking | clicks | rips | 0.6528 | -0.00707 ± 0.00785 | 0.07839 | 0.60 | 3 | 0.958 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.96; 0% of logs usable) |
| ranking | clicks | snrips | 0.6528 | -0.01054 ± 0.00606 | 0.06122 | 0.64 | 3 | 0.958 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.96; 0% of logs usable) |
| ranking | clicks | dm-pbm | 0.6528 | +0.08452 ± 0.00023 | 0.08455 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.6528 | -0.11522 ± 0.00019 | 0.11524 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.6528 | +0.03969 ± 0.00052 | 0.04003 | 0.00 | 9889 | 1.001 | 99% | 0.00 |  | biased, as expected |
| ranking | clicks | cdr | 0.6528 | -0.00633 ± 0.00681 | 0.06807 | 0.88 | 3 | 0.958 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.96; 0% of logs usable) |
| ranking | satisfied | naive | 0.5092 | -0.28445 ± 0.00013 | 0.28445 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.5092 | -0.00720 ± 0.04260 | 0.42390 | 0.78 | 3 | 1.040 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.04; 0% of logs usable) |
| ranking | satisfied | snips | 0.5092 | -0.07820 ± 0.03153 | 0.32330 | 0.66 | 3 | 1.040 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.04; 0% of logs usable) |
| ranking | satisfied | iips | 0.5092 | +0.00875 ± 0.00074 | 0.01145 | 0.79 | 9889 | 1.001 | 99% | 0.80 |  | biased, as expected |
| ranking | satisfied | sniips | 0.5092 | +0.00909 ± 0.00057 | 0.01073 | 0.64 | 9889 | 1.001 | 99% | 0.65 |  | biased, as expected |
| ranking | satisfied | rips | 0.5092 | -0.00201 ± 0.00571 | 0.05682 | 0.71 | 3 | 0.958 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.96; 0% of logs usable) |
| ranking | satisfied | snrips | 0.5092 | -0.00313 ± 0.00480 | 0.04784 | 0.75 | 3 | 0.958 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.96; 0% of logs usable) |
| ranking | satisfied | dm-pbm | 0.5092 | +0.04295 ± 0.00024 | 0.04302 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.5092 | -0.09962 ± 0.00019 | 0.09964 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.5092 | +0.00981 ± 0.00044 | 0.01075 | 0.46 | 9889 | 1.001 | 99% | 0.46 |  | biased, as expected |
| ranking | satisfied | cdr | 0.5092 | -0.00032 ± 0.00438 | 0.04357 | 0.96 | 3 | 0.958 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.96; 0% of logs usable) |
| rerank | clicks | naive | 0.6332 | -0.27099 ± 0.00017 | 0.27100 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.6332 | +0.08349 ± 0.04913 | 0.49591 | 0.91 | 4 | 1.080 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.08; 0% of logs usable) |
| rerank | clicks | snips | 0.6332 | +0.01145 ± 0.03373 | 0.33580 | 0.68 | 4 | 1.080 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.08; 0% of logs usable) |
| rerank | clicks | iips | 0.6332 | +0.03537 ± 0.00078 | 0.03622 | 0.02 | 9891 | 1.000 | 100% | 0.02 |  | biased, as expected |
| rerank | clicks | sniips | 0.6332 | +0.03550 ± 0.00065 | 0.03609 | 0.00 | 9891 | 1.000 | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | rips | 0.6332 | +0.00431 ± 0.00861 | 0.08578 | 0.77 | 4 | 1.019 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.02; 0% of logs usable) |
| rerank | clicks | snrips | 0.6332 | +0.01111 ± 0.01304 | 0.13019 | 0.79 | 4 | 1.019 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.02; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.6332 | +0.07176 ± 0.00023 | 0.07180 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.6332 | -0.09617 ± 0.00020 | 0.09619 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.6332 | +0.03659 ± 0.00045 | 0.03687 | 0.00 | 9891 | 1.000 | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | cdr | 0.6332 | +0.00278 ± 0.00703 | 0.07003 | 0.95 | 4 | 1.019 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.02; 0% of logs usable) |
| rerank | satisfied | naive | 0.4883 | -0.26357 ± 0.00013 | 0.26357 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.4883 | +0.06509 ± 0.04168 | 0.41983 | 0.85 | 4 | 1.080 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.08; 0% of logs usable) |
| rerank | satisfied | snips | 0.4883 | +0.00302 ± 0.02985 | 0.29702 | 0.72 | 4 | 1.080 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.08; 0% of logs usable) |
| rerank | satisfied | iips | 0.4883 | +0.00904 ± 0.00070 | 0.01143 | 0.76 | 9891 | 1.000 | 100% | 0.76 |  | biased, as expected |
| rerank | satisfied | sniips | 0.4883 | +0.00916 ± 0.00060 | 0.01093 | 0.66 | 9891 | 1.000 | 100% | 0.66 |  | biased, as expected |
| rerank | satisfied | rips | 0.4883 | +0.00152 ± 0.00585 | 0.05818 | 0.86 | 4 | 1.019 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.02; 0% of logs usable) |
| rerank | satisfied | snrips | 0.4883 | +0.00575 ± 0.00818 | 0.08163 | 0.82 | 4 | 1.019 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.02; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.4883 | +0.03461 ± 0.00023 | 0.03468 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.4883 | -0.08184 ± 0.00019 | 0.08186 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.4883 | +0.01008 ± 0.00042 | 0.01091 | 0.42 | 9891 | 1.000 | 100% | 0.42 |  | biased, as expected |
| rerank | satisfied | cdr | 0.4883 | +0.00031 ± 0.00396 | 0.03941 | 0.93 | 4 | 1.019 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.02; 0% of logs usable) |
| oracle | clicks | naive | 0.7075 | -0.34532 ± 0.00017 | 0.34532 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.7075 | +0.03940 ± 0.05562 | 0.55481 | 0.90 | 3 | 1.022 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.02; 0% of logs usable) |
| oracle | clicks | snips | 0.7075 | -0.01137 ± 0.03304 | 0.32894 | 0.52 | 3 | 1.022 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.02; 0% of logs usable) |
| oracle | clicks | iips | 0.7075 | +0.05546 ± 0.00085 | 0.05610 | 0.00 | 9883 | 0.999 | 98% | 0.00 |  | biased, as expected |
| oracle | clicks | sniips | 0.7075 | +0.05609 ± 0.00056 | 0.05636 | 0.00 | 9883 | 0.999 | 98% | 0.00 |  | biased, as expected |
| oracle | clicks | rips | 0.7075 | -0.00435 ± 0.00580 | 0.05785 | 0.63 | 3 | 0.906 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.91; 0% of logs usable) |
| oracle | clicks | snrips | 0.7075 | -0.00802 ± 0.00384 | 0.03907 | 0.53 | 3 | 0.906 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.91; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.7075 | +0.11715 ± 0.00024 | 0.11717 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.7075 | -0.15102 ± 0.00020 | 0.15103 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.7075 | +0.05650 ± 0.00050 | 0.05672 | 0.00 | 9883 | 0.999 | 98% | 0.00 |  | biased, as expected |
| oracle | clicks | cdr | 0.7075 | -0.00521 ± 0.00559 | 0.05586 | 0.82 | 3 | 0.906 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.91; 0% of logs usable) |
| oracle | satisfied | naive | 0.5645 | -0.33981 ± 0.00013 | 0.33981 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5645 | -0.00510 ± 0.04774 | 0.47505 | 0.81 | 3 | 1.022 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.02; 0% of logs usable) |
| oracle | satisfied | snips | 0.5645 | -0.06587 ± 0.03284 | 0.33331 | 0.62 | 3 | 1.022 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.02; 0% of logs usable) |
| oracle | satisfied | iips | 0.5645 | +0.01750 ± 0.00074 | 0.01899 | 0.35 | 9883 | 0.999 | 98% | 0.35 |  | biased, as expected |
| oracle | satisfied | sniips | 0.5645 | +0.01803 ± 0.00049 | 0.01867 | 0.05 | 9883 | 0.999 | 98% | 0.05 |  | biased, as expected |
| oracle | satisfied | rips | 0.5645 | -0.00185 ± 0.00122 | 0.01226 | 0.92 | 3 | 0.906 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.91; 0% of logs usable) |
| oracle | satisfied | snrips | 0.5645 | -0.00161 ± 0.00096 | 0.00966 | 0.90 | 3 | 0.906 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.91; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.5645 | +0.06416 ± 0.00027 | 0.06422 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5645 | -0.13413 ± 0.00019 | 0.13414 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5645 | +0.01841 ± 0.00042 | 0.01889 | 0.01 | 9883 | 0.999 | 98% | 0.01 |  | biased, as expected |
| oracle | satisfied | cdr | 0.5645 | -0.00183 ± 0.00108 | 0.01090 | 0.97 | 3 | 0.906 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.91; 0% of logs usable) |
| reverse | clicks | naive | 0.1690 | +0.19318 ± 0.00017 | 0.19319 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.1690 | +0.00032 ± 0.02518 | 0.25054 | 0.38 | 3 | 1.055 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.06; 0% of logs usable) |
| reverse | clicks | snips | 0.1690 | -0.01944 ± 0.02374 | 0.23701 | 0.35 | 3 | 1.055 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.06; 0% of logs usable) |
| reverse | clicks | iips | 0.1690 | -0.01485 ± 0.00036 | 0.01528 | 0.01 | 9880 | 1.000 | 96% | 0.01 |  | biased, as expected |
| reverse | clicks | sniips | 0.1690 | -0.01483 ± 0.00036 | 0.01524 | 0.01 | 9880 | 1.000 | 96% | 0.01 |  | biased, as expected |
| reverse | clicks | rips | 0.1690 | +0.01201 ± 0.01209 | 0.12085 | 0.61 | 3 | 0.932 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| reverse | clicks | snrips | 0.1690 | +0.00995 ± 0.01323 | 0.13204 | 0.60 | 3 | 0.932 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.1690 | +0.03056 ± 0.00017 | 0.03060 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.1690 | +0.01126 ± 0.00019 | 0.01141 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.1690 | -0.01473 ± 0.00036 | 0.01517 | 0.01 | 9880 | 1.000 | 96% | 0.01 |  | biased, as expected |
| reverse | clicks | cdr | 0.1690 | +0.01281 ± 0.01070 | 0.10726 | 0.86 | 3 | 0.932 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| reverse | satisfied | naive | 0.0433 | +0.18144 ± 0.00013 | 0.18145 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0433 | +0.00509 ± 0.01194 | 0.11893 | 0.15 | 3 | 1.055 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.06; 0% of logs usable) |
| reverse | satisfied | snips | 0.0433 | -0.00140 ± 0.01056 | 0.10505 | 0.15 | 3 | 1.055 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.06; 0% of logs usable) |
| reverse | satisfied | iips | 0.0433 | -0.00237 ± 0.00019 | 0.00305 | 0.76 | 9880 | 1.000 | 96% | 0.76 |  | biased, as expected |
| reverse | satisfied | sniips | 0.0433 | -0.00235 ± 0.00020 | 0.00306 | 0.77 | 9880 | 1.000 | 96% | 0.77 |  | biased, as expected |
| reverse | satisfied | rips | 0.0433 | +0.00985 ± 0.00763 | 0.07653 | 0.51 | 3 | 0.932 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0433 | +0.00611 ± 0.00636 | 0.06357 | 0.53 | 3 | 0.932 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0433 | +0.03511 ± 0.00007 | 0.03512 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0433 | +0.02389 ± 0.00008 | 0.02390 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0433 | -0.00226 ± 0.00018 | 0.00290 | 0.75 | 9880 | 1.000 | 96% | 0.75 |  | biased, as expected |
| reverse | satisfied | cdr | 0.0433 | +0.01082 ± 0.00616 | 0.06225 | 0.93 | 3 | 0.932 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| wide | clicks | naive | 0.6243 | -0.26208 ± 0.00017 | 0.26209 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.6243 | -0.37027 ± 0.03150 | 0.48513 | 0.52 | 1 | 0.330 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snips | 0.6243 | -0.12596 ± 0.05138 | 0.52655 | 0.10 | 1 | 0.330 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.6243 | +0.01193 ± 0.00074 | 0.01401 | 0.66 | 6403 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | sniips | 0.6243 | +0.07337 ± 0.00073 | 0.07374 | 0.00 | 6403 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | rips | 0.6243 | -0.02773 ± 0.00627 | 0.06831 | 0.57 | 1 | 0.472 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snrips | 0.6243 | +0.05065 ± 0.01320 | 0.14076 | 0.91 | 1 | 0.472 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.6243 | +0.10443 ± 0.00024 | 0.10446 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.6243 | -0.06968 ± 0.00021 | 0.06971 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.6243 | +0.07462 ± 0.00045 | 0.07475 | 0.00 | 6403 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | cdr | 0.6243 | +0.04221 ± 0.00505 | 0.06560 | 0.66 | 1 | 0.472 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | naive | 0.4800 | -0.25530 ± 0.00013 | 0.25530 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.4800 | -0.27440 ± 0.02943 | 0.40129 | 0.45 | 1 | 0.330 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | snips | 0.4800 | -0.09170 ± 0.04563 | 0.46320 | 0.13 | 1 | 0.330 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.4800 | +0.00412 ± 0.00068 | 0.00792 | 0.90 | 6403 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | sniips | 0.4800 | +0.04309 ± 0.00067 | 0.04360 | 0.00 | 6403 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | rips | 0.4800 | -0.00984 ± 0.00422 | 0.04309 | 0.79 | 1 | 0.472 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.4800 | +0.04296 ± 0.00753 | 0.08636 | 0.94 | 1 | 0.472 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.4800 | +0.06336 ± 0.00024 | 0.06340 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.4800 | -0.05968 ± 0.00019 | 0.05971 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.4800 | +0.04331 ± 0.00043 | 0.04352 | 0.00 | 6403 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | cdr | 0.4800 | +0.03179 ± 0.00249 | 0.04033 | 0.63 | 1 | 0.472 | 0% |  |  | biased, as expected: 20% of its slots unsupported |

## scifact · dbn · {"policy":"swap","top":10}

100 logs of 100000 impressions, 12 s. Propensities: 10000000 lists, 0 mismatched; chi-square 3863 on 3900 df (p 0.661), largest |z| 3.64.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.6528 | 0.5092 | 0.5092 | 2.255 |
| rerank | 54.3% | 0.6332 | 0.4883 | 0.4883 | 2.265 |
| oracle | 5.8% | 0.7075 | 0.5645 | 0.5645 | 2.229 |
| reverse | 80.0% | 0.1690 | 0.0433 | 0.0433 | 2.510 |
| wide | 61.2% | 0.6243 | 0.4800 | 0.4800 | 2.269 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.6528 | -0.24292 ± 0.00018 | 0.24293 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.6528 | -0.00012 ± 0.00077 | 0.00764 | 0.96 | 9987 | 0.999 | 100% | 0.96 | yes | ok |
| ranking | clicks | snips | 0.6528 | +0.00075 ± 0.00046 | 0.00463 | 0.97 | 9987 | 0.999 | 100% | 0.97 | yes | ok |
| ranking | clicks | iips | 0.6528 | +0.03471 ± 0.00068 | 0.03536 | 0.00 | 9987 | 1.000 | 97% | 0.00 |  | biased, as expected |
| ranking | clicks | sniips | 0.6528 | +0.03536 ± 0.00049 | 0.03568 | 0.00 | 9987 | 1.000 | 97% | 0.00 |  | biased, as expected |
| ranking | clicks | rips | 0.6528 | -0.00012 ± 0.00077 | 0.00764 | 0.96 | 9987 | 0.999 | 100% | 0.96 | yes | ok |
| ranking | clicks | snrips | 0.6528 | +0.00075 ± 0.00046 | 0.00463 | 0.97 | 9987 | 0.999 | 100% | 0.97 | yes | ok |
| ranking | clicks | dm-pbm | 0.6528 | +0.06558 ± 0.00022 | 0.06562 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.6528 | -0.11070 ± 0.00021 | 0.11072 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.6528 | +0.03506 ± 0.00038 | 0.03526 | 0.00 | 9987 | 1.000 | 97% | 0.00 |  | biased, as expected |
| ranking | clicks | cdr | 0.6528 | +0.00060 ± 0.00044 | 0.00444 | 0.98 | 9987 | 0.999 | 100% | 0.98 | yes | ok |
| ranking | satisfied | naive | 0.5092 | -0.23874 ± 0.00016 | 0.23875 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.5092 | -0.00047 ± 0.00070 | 0.00702 | 0.95 | 9987 | 0.999 | 100% | 0.95 | yes | ok |
| ranking | satisfied | snips | 0.5092 | +0.00021 ± 0.00045 | 0.00446 | 0.97 | 9987 | 0.999 | 100% | 0.97 | yes | ok |
| ranking | satisfied | iips | 0.5092 | +0.00853 ± 0.00064 | 0.01064 | 0.77 | 9987 | 1.000 | 97% | 0.76 |  | biased, as expected |
| ranking | satisfied | sniips | 0.5092 | +0.00907 ± 0.00049 | 0.01027 | 0.60 | 9987 | 1.000 | 97% | 0.60 |  | biased, as expected |
| ranking | satisfied | rips | 0.5092 | -0.00047 ± 0.00070 | 0.00702 | 0.95 | 9987 | 0.999 | 100% | 0.95 | yes | ok |
| ranking | satisfied | snrips | 0.5092 | +0.00021 ± 0.00045 | 0.00446 | 0.97 | 9987 | 0.999 | 100% | 0.97 | yes | ok |
| ranking | satisfied | dm-pbm | 0.5092 | +0.03351 ± 0.00024 | 0.03360 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.5092 | -0.09377 ± 0.00021 | 0.09379 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.5092 | +0.00881 ± 0.00041 | 0.00970 | 0.39 | 9987 | 1.000 | 97% | 0.39 |  | biased, as expected |
| ranking | satisfied | cdr | 0.5092 | +0.00009 ± 0.00046 | 0.00457 | 0.94 | 9987 | 0.999 | 100% | 0.94 | yes | ok |
| rerank | clicks | naive | 0.6332 | -0.22333 ± 0.00018 | 0.22334 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.6332 | -0.62378 ± 0.00010 | 0.62378 | 0.00 | 196 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | snips | 0.6332 | -0.15165 ± 0.00366 | 0.15596 | 0.04 | 196 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | iips | 0.6332 | -0.10005 ± 0.00070 | 0.10030 | 0.00 | 8608 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | sniips | 0.6332 | +0.08204 ± 0.00085 | 0.08248 | 0.00 | 8608 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | rips | 0.6332 | -0.17647 ± 0.00065 | 0.17659 | 0.00 | 196 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | snrips | 0.6332 | -0.06593 ± 0.00115 | 0.06691 | 0.00 | 196 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | dm-pbm | 0.6332 | +0.07467 ± 0.00026 | 0.07471 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.6332 | -0.07735 ± 0.00024 | 0.07739 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.6332 | +0.05524 ± 0.00039 | 0.05538 | 0.00 | 8608 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | cdr | 0.6332 | +0.02572 ± 0.00041 | 0.02604 | 0.00 | 196 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | naive | 0.4883 | -0.21786 ± 0.00016 | 0.21787 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.4883 | -0.48162 ± 0.00008 | 0.48162 | 0.00 | 196 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | snips | 0.4883 | -0.14681 ± 0.00327 | 0.15038 | 0.01 | 196 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | iips | 0.4883 | -0.06599 ± 0.00064 | 0.06630 | 0.00 | 8608 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | sniips | 0.4883 | +0.05578 ± 0.00081 | 0.05635 | 0.00 | 8608 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | rips | 0.4883 | -0.12061 ± 0.00059 | 0.12075 | 0.00 | 196 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | snrips | 0.4883 | -0.05608 ± 0.00078 | 0.05662 | 0.00 | 196 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | dm-pbm | 0.4883 | +0.04218 ± 0.00025 | 0.04225 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.4883 | -0.06657 ± 0.00022 | 0.06660 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.4883 | +0.02768 ± 0.00039 | 0.02795 | 0.00 | 8608 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | cdr | 0.4883 | +0.01897 ± 0.00039 | 0.01937 | 0.00 | 196 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| oracle | clicks | naive | 0.7075 | -0.29766 ± 0.00018 | 0.29767 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.7075 | -0.10958 ± 0.00075 | 0.10983 | 0.00 | 8703 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | snips | 0.7075 | -0.02049 ± 0.00052 | 0.02113 | 0.02 | 8703 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | iips | 0.7075 | +0.02765 ± 0.00076 | 0.02868 | 0.04 | 9998 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | sniips | 0.7075 | +0.03362 ± 0.00045 | 0.03391 | 0.00 | 9998 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | rips | 0.7075 | -0.01005 ± 0.00078 | 0.01267 | 0.79 | 8703 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | snrips | 0.7075 | -0.00327 ± 0.00047 | 0.00570 | 0.88 | 8703 | 0.870 | 0% |  |  | unbiased though unsupported |
| oracle | clicks | dm-pbm | 0.7075 | +0.09019 ± 0.00025 | 0.09023 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.7075 | -0.14344 ± 0.00022 | 0.14346 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.7075 | +0.04910 ± 0.00041 | 0.04927 | 0.00 | 9998 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | cdr | 0.7075 | +0.00288 ± 0.00044 | 0.00521 | 0.93 | 8703 | 0.870 | 0% |  |  | unbiased though unsupported |
| oracle | satisfied | naive | 0.5645 | -0.29410 ± 0.00016 | 0.29411 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5645 | -0.09042 ± 0.00069 | 0.09068 | 0.00 | 8703 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | snips | 0.5645 | -0.01979 ± 0.00052 | 0.02046 | 0.04 | 8703 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | iips | 0.5645 | +0.00195 ± 0.00069 | 0.00715 | 0.94 | 9998 | 0.882 | 0% |  |  | unbiased though unsupported |
| oracle | satisfied | sniips | 0.5645 | +0.00304 ± 0.00046 | 0.00553 | 0.90 | 9998 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | rips | 0.5645 | -0.00588 ± 0.00069 | 0.00903 | 0.90 | 8703 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | snrips | 0.5645 | -0.00450 ± 0.00045 | 0.00637 | 0.84 | 8703 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | dm-pbm | 0.5645 | +0.04703 ± 0.00026 | 0.04710 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5645 | -0.12583 ± 0.00023 | 0.12585 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5645 | +0.01631 ± 0.00043 | 0.01687 | 0.05 | 9998 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | cdr | 0.5645 | +0.00196 ± 0.00045 | 0.00488 | 0.93 | 8703 | 0.870 | 0% |  |  | unbiased though unsupported |
| reverse | clicks | naive | 0.1690 | +0.24084 ± 0.00018 | 0.24085 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.1690 | -0.16903 ± 0.00000 | 0.16903 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snips | 0.1690 | -0.16903 ± 0.00000 | 0.16903 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | iips | 0.1690 | -0.12265 ± 0.00022 | 0.12267 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | sniips | 0.1690 | -0.12263 ± 0.00022 | 0.12265 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | rips | 0.1690 | -0.12265 ± 0.00022 | 0.12267 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snrips | 0.1690 | -0.12263 ± 0.00022 | 0.12265 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | dm-pbm | 0.1690 | +0.10378 ± 0.00029 | 0.10382 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.1690 | +0.06945 ± 0.00034 | 0.06954 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.1690 | +0.06558 ± 0.00031 | 0.06565 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | cdr | 0.1690 | +0.06435 ± 0.00035 | 0.06444 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | naive | 0.0433 | +0.22715 ± 0.00016 | 0.22715 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0433 | -0.04329 ± 0.00000 | 0.04329 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snips | 0.0433 | -0.04329 ± 0.00000 | 0.04329 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | iips | 0.0433 | -0.03107 ± 0.00010 | 0.03109 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | sniips | 0.0433 | -0.03107 ± 0.00010 | 0.03109 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | rips | 0.0433 | -0.03107 ± 0.00010 | 0.03109 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snrips | 0.0433 | -0.03107 ± 0.00010 | 0.03109 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | dm-pbm | 0.0433 | +0.09454 ± 0.00013 | 0.09455 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0433 | +0.06415 ± 0.00014 | 0.06416 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0433 | +0.06377 ± 0.00014 | 0.06379 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | cdr | 0.0433 | +0.05295 ± 0.00015 | 0.05297 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| wide | clicks | naive | 0.6243 | -0.21442 ± 0.00018 | 0.21443 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.6243 | -0.61534 ± 0.00009 | 0.61534 | 0.00 | 163 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | snips | 0.6243 | -0.07407 ± 0.00401 | 0.08415 | 0.63 | 163 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | iips | 0.6243 | -0.10027 ± 0.00070 | 0.10051 | 0.00 | 4242 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | sniips | 0.6243 | +0.13857 ± 0.00114 | 0.13903 | 0.00 | 4242 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | rips | 0.6243 | -0.17036 ± 0.00064 | 0.17048 | 0.00 | 163 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | snrips | 0.6243 | -0.04200 ± 0.00120 | 0.04366 | 0.09 | 163 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | dm-pbm | 0.6243 | +0.10825 ± 0.00027 | 0.10828 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.6243 | -0.04764 ± 0.00023 | 0.04769 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.6243 | +0.09034 ± 0.00041 | 0.09044 | 0.00 | 4242 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | cdr | 0.6243 | +0.05567 ± 0.00039 | 0.05581 | 0.00 | 163 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | naive | 0.4800 | -0.20959 ± 0.00016 | 0.20960 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.4800 | -0.47341 ± 0.00008 | 0.47341 | 0.00 | 163 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | snips | 0.4800 | -0.07300 ± 0.00371 | 0.08180 | 0.56 | 163 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | iips | 0.4800 | -0.06061 ± 0.00064 | 0.06094 | 0.00 | 4242 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | sniips | 0.4800 | +0.10874 ± 0.00101 | 0.10920 | 0.00 | 4242 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | rips | 0.4800 | -0.11319 ± 0.00059 | 0.11334 | 0.00 | 163 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | snrips | 0.4800 | -0.03168 ± 0.00086 | 0.03282 | 0.08 | 163 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.4800 | +0.07285 ± 0.00026 | 0.07289 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.4800 | -0.04084 ± 0.00022 | 0.04090 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.4800 | +0.05983 ± 0.00039 | 0.05995 | 0.00 | 4242 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | cdr | 0.4800 | +0.04493 ± 0.00039 | 0.04510 | 0.00 | 163 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |

## scifact · ubm · {"policy":"softmax","tau":1.0,"top":10,"by":"rank"}

100 logs of 100000 impressions, 16 s. Propensities: 10000000 lists, 0 mismatched; chi-square 13516 on 13500 df (p 0.460), largest |z| 4.36.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.5909 | 0.4600 | 0.4519 | 2.282 |
| rerank | 0.0% | 0.5390 | 0.4140 | 0.4059 | 2.304 |
| oracle | 0.0% | 0.7193 | 0.5727 | 0.5577 | 2.232 |
| reverse | 0.0% | 0.0953 | 0.0243 | 0.0238 | 2.522 |
| wide | 20.3% | 0.5342 | 0.4100 | 0.4021 | 2.306 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.5909 | -0.21952 ± 0.00018 | 0.21953 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.5909 | -0.00426 ± 0.00736 | 0.07339 | 0.94 | 111 | 0.994 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snips | 0.5909 | -0.00093 ± 0.00501 | 0.04986 | 0.97 | 111 | 0.994 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | iips | 0.5909 | -0.00261 ± 0.00041 | 0.00488 | 0.93 | 11577 | 1.001 | 99% | 0.93 |  | unbiased here |
| ranking | clicks | sniips | 0.5909 | -0.00304 ± 0.00036 | 0.00467 | 0.87 | 11577 | 1.001 | 99% | 0.87 |  | biased, as expected |
| ranking | clicks | rips | 0.5909 | -0.00130 ± 0.00106 | 0.01067 | 0.88 | 111 | 0.999 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snrips | 0.5909 | -0.00172 ± 0.00100 | 0.01010 | 0.90 | 111 | 0.999 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | dm-pbm | 0.5909 | +0.00305 ± 0.00023 | 0.00382 | 0.28 |  |  | 100% | 0.28 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.5909 | -0.18677 ± 0.00020 | 0.18678 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.5909 | -0.00311 ± 0.00031 | 0.00438 | 0.84 | 11577 | 1.001 | 99% | 0.84 |  | biased, as expected |
| ranking | clicks | cdr | 0.5909 | -0.00151 ± 0.00098 | 0.00985 | 0.94 | 111 | 0.999 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | naive | 0.4600 | -0.19285 ± 0.00015 | 0.19285 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.4600 | -0.00246 ± 0.00694 | 0.06911 | 0.92 | 111 | 0.994 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snips | 0.4600 | -0.00036 ± 0.00516 | 0.05135 | 0.96 | 111 | 0.994 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | iips | 0.4600 | -0.00057 ± 0.00036 | 0.00359 | 0.94 | 11577 | 1.001 | 99% | 0.94 |  | unbiased here |
| ranking | satisfied | sniips | 0.4600 | -0.00092 ± 0.00032 | 0.00330 | 0.93 | 11577 | 1.001 | 99% | 0.93 |  | unbiased here |
| ranking | satisfied | rips | 0.4600 | -0.00023 ± 0.00077 | 0.00771 | 0.90 | 111 | 0.999 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snrips | 0.4600 | -0.00059 ± 0.00074 | 0.00739 | 0.86 | 111 | 0.999 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | dm-pbm | 0.4600 | +0.00808 ± 0.00021 | 0.00836 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.4600 | -0.15309 ± 0.00016 | 0.15310 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.4600 | -0.00098 ± 0.00027 | 0.00289 | 0.95 | 11577 | 1.001 | 99% | 0.95 |  | unbiased here |
| ranking | satisfied | cdr | 0.4600 | -0.00053 ± 0.00074 | 0.00742 | 0.93 | 111 | 0.999 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| rerank | clicks | naive | 0.5390 | -0.16757 ± 0.00018 | 0.16758 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.5390 | -0.01958 ± 0.01941 | 0.19414 | 0.90 | 18 | 0.971 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.97; 1% of logs usable) |
| rerank | clicks | snips | 0.5390 | +0.01060 ± 0.01518 | 0.15145 | 0.86 | 18 | 0.971 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.97; 1% of logs usable) |
| rerank | clicks | iips | 0.5390 | -0.00267 ± 0.00052 | 0.00587 | 0.89 | 9469 | 1.001 | 95% | 0.89 |  | unbiased here |
| rerank | clicks | sniips | 0.5390 | -0.00317 ± 0.00047 | 0.00564 | 0.86 | 9469 | 1.001 | 95% | 0.85 |  | biased, as expected |
| rerank | clicks | rips | 0.5390 | +0.00175 ± 0.00342 | 0.03411 | 0.81 | 18 | 0.937 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.94; 1% of logs usable) |
| rerank | clicks | snrips | 0.5390 | +0.00287 ± 0.00389 | 0.03880 | 0.84 | 18 | 0.937 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.94; 1% of logs usable) |
| rerank | clicks | dm-pbm | 0.5390 | +0.01079 ± 0.00023 | 0.01103 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.5390 | -0.13707 ± 0.00020 | 0.13709 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.5390 | -0.00317 ± 0.00041 | 0.00517 | 0.81 | 9469 | 1.001 | 95% | 0.80 |  | biased, as expected |
| rerank | clicks | cdr | 0.5390 | +0.00183 ± 0.00323 | 0.03221 | 0.97 | 18 | 0.937 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.94; 1% of logs usable) |
| rerank | satisfied | naive | 0.4140 | -0.14687 ± 0.00015 | 0.14688 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.4140 | -0.01157 ± 0.01622 | 0.16176 | 0.91 | 18 | 0.971 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.97; 1% of logs usable) |
| rerank | satisfied | snips | 0.4140 | +0.01068 ± 0.01229 | 0.12279 | 0.89 | 18 | 0.971 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.97; 1% of logs usable) |
| rerank | satisfied | iips | 0.4140 | -0.00081 ± 0.00041 | 0.00411 | 0.93 | 9469 | 1.001 | 95% | 0.93 |  | unbiased here |
| rerank | satisfied | sniips | 0.4140 | -0.00121 ± 0.00034 | 0.00362 | 0.96 | 9469 | 1.001 | 95% | 0.96 |  | unbiased here |
| rerank | satisfied | rips | 0.4140 | +0.00108 ± 0.00278 | 0.02765 | 0.85 | 18 | 0.937 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.94; 1% of logs usable) |
| rerank | satisfied | snrips | 0.4140 | +0.00172 ± 0.00325 | 0.03241 | 0.84 | 18 | 0.937 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.94; 1% of logs usable) |
| rerank | satisfied | dm-pbm | 0.4140 | +0.01364 ± 0.00022 | 0.01381 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.4140 | -0.10899 ± 0.00016 | 0.10901 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.4140 | -0.00120 ± 0.00031 | 0.00329 | 0.94 | 9469 | 1.001 | 95% | 0.94 |  | unbiased here |
| rerank | satisfied | cdr | 0.4140 | +0.00090 ± 0.00237 | 0.02363 | 0.96 | 18 | 0.937 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.94; 1% of logs usable) |
| oracle | clicks | naive | 0.7193 | -0.34790 ± 0.00018 | 0.34791 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.7193 | +0.00182 ± 0.01124 | 0.11181 | 0.91 | 92 | 1.004 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snips | 0.7193 | -0.00209 ± 0.00755 | 0.07518 | 0.96 | 92 | 1.004 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | iips | 0.7193 | -0.00657 ± 0.00059 | 0.00879 | 0.79 | 11640 | 1.001 | 99% | 0.79 |  | biased, as expected |
| oracle | clicks | sniips | 0.7193 | -0.00651 ± 0.00043 | 0.00781 | 0.58 | 11640 | 1.001 | 99% | 0.59 |  | biased, as expected |
| oracle | clicks | rips | 0.7193 | -0.00251 ± 0.00126 | 0.01276 | 0.87 | 92 | 0.990 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snrips | 0.7193 | -0.00239 ± 0.00114 | 0.01160 | 0.86 | 92 | 0.990 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | dm-pbm | 0.7193 | -0.00906 ± 0.00026 | 0.00942 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.7193 | -0.30436 ± 0.00020 | 0.30437 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.7193 | -0.00651 ± 0.00040 | 0.00764 | 0.52 | 11640 | 1.001 | 99% | 0.53 |  | biased, as expected |
| oracle | clicks | cdr | 0.7193 | -0.00250 ± 0.00112 | 0.01145 | 0.87 | 92 | 0.990 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | naive | 0.5727 | -0.30558 ± 0.00015 | 0.30558 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5727 | +0.00294 ± 0.00989 | 0.09842 | 0.90 | 92 | 1.004 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snips | 0.5727 | -0.00135 ± 0.00651 | 0.06481 | 0.94 | 92 | 1.004 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | iips | 0.5727 | -0.00305 ± 0.00051 | 0.00594 | 0.90 | 11640 | 1.001 | 99% | 0.90 |  | unbiased here |
| oracle | satisfied | sniips | 0.5727 | -0.00299 ± 0.00039 | 0.00493 | 0.82 | 11640 | 1.001 | 99% | 0.83 |  | biased, as expected |
| oracle | satisfied | rips | 0.5727 | -0.00079 ± 0.00080 | 0.00795 | 0.92 | 92 | 0.990 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snrips | 0.5727 | -0.00074 ± 0.00071 | 0.00711 | 0.87 | 92 | 0.990 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | dm-pbm | 0.5727 | -0.00254 ± 0.00025 | 0.00358 | 0.26 |  |  | 100% | 0.26 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5727 | -0.25485 ± 0.00016 | 0.25485 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5727 | -0.00305 ± 0.00037 | 0.00474 | 0.82 | 11640 | 1.001 | 99% | 0.82 |  | biased, as expected |
| oracle | satisfied | cdr | 0.5727 | -0.00081 ± 0.00069 | 0.00691 | 0.91 | 92 | 0.990 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| reverse | clicks | naive | 0.0953 | +0.27614 ± 0.00018 | 0.27614 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.0953 | -0.09528 ± 0.00000 | 0.09528 | 0.00 | 0 | 1.298 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 1.30; 0% of logs usable) |
| reverse | clicks | snips | 0.0953 | -0.09528 ± 0.00000 | 0.09528 | 0.00 | 0 | 1.298 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 1.30; 0% of logs usable) |
| reverse | clicks | iips | 0.0953 | +0.00521 ± 0.00052 | 0.00732 | 0.77 | 3411 | 1.000 | 97% | 0.77 |  | biased, as expected |
| reverse | clicks | sniips | 0.0953 | +0.00518 ± 0.00050 | 0.00717 | 0.79 | 3411 | 1.000 | 97% | 0.79 |  | biased, as expected |
| reverse | clicks | rips | 0.0953 | -0.01039 ± 0.00604 | 0.06103 | 0.40 | 0 | 0.501 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.50; 0% of logs usable) |
| reverse | clicks | snrips | 0.0953 | -0.01140 ± 0.00566 | 0.05743 | 0.43 | 0 | 0.501 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.50; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.0953 | +0.08539 ± 0.00025 | 0.08543 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.0953 | +0.03472 ± 0.00021 | 0.03479 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.0953 | +0.00542 ± 0.00051 | 0.00743 | 0.78 | 3411 | 1.000 | 97% | 0.78 |  | biased, as expected |
| reverse | clicks | cdr | 0.0953 | +0.00341 ± 0.01748 | 0.17396 | 0.72 | 0 | 0.501 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.50; 0% of logs usable) |
| reverse | satisfied | naive | 0.0243 | +0.24282 ± 0.00015 | 0.24283 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0243 | -0.02430 ± 0.00000 | 0.02430 | 0.00 | 0 | 1.298 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 1.30; 0% of logs usable) |
| reverse | satisfied | snips | 0.0243 | -0.02430 ± 0.00000 | 0.02430 | 0.00 | 0 | 1.298 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 1.30; 0% of logs usable) |
| reverse | satisfied | iips | 0.0243 | +0.00092 ± 0.00025 | 0.00263 | 0.93 | 3411 | 1.000 | 97% | 0.94 |  | unbiased here |
| reverse | satisfied | sniips | 0.0243 | +0.00091 ± 0.00024 | 0.00257 | 0.94 | 3411 | 1.000 | 97% | 0.94 |  | unbiased here |
| reverse | satisfied | rips | 0.0243 | -0.00173 ± 0.00395 | 0.03935 | 0.47 | 0 | 0.501 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.50; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0243 | -0.00206 ± 0.00368 | 0.03666 | 0.47 | 0 | 0.501 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.50; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0243 | +0.07174 ± 0.00011 | 0.07175 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0243 | +0.03531 ± 0.00010 | 0.03533 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0243 | +0.00105 ± 0.00023 | 0.00255 | 0.93 | 3411 | 1.000 | 97% | 0.93 |  | biased, as expected |
| reverse | satisfied | cdr | 0.0243 | +0.01085 ± 0.00532 | 0.05404 | 0.55 | 0 | 0.501 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.50; 0% of logs usable) |
| wide | clicks | naive | 0.5342 | -0.16279 ± 0.00018 | 0.16280 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.5342 | -0.34133 ± 0.01267 | 0.36387 | 0.10 | 10 | 0.356 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snips | 0.5342 | +0.00329 ± 0.01836 | 0.18272 | 0.87 | 10 | 0.356 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.5342 | -0.01355 ± 0.00049 | 0.01438 | 0.15 | 6563 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | sniips | 0.5342 | +0.01474 ± 0.00048 | 0.01548 | 0.08 | 6563 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | rips | 0.5342 | -0.01683 ± 0.00291 | 0.03350 | 0.50 | 10 | 0.501 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snrips | 0.5342 | +0.02986 ± 0.00630 | 0.06944 | 0.97 | 10 | 0.501 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | dm-pbm | 0.5342 | +0.03165 ± 0.00024 | 0.03174 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.5342 | -0.09225 ± 0.00021 | 0.09227 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.5342 | +0.02116 ± 0.00037 | 0.02148 | 0.00 | 6563 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | cdr | 0.5342 | +0.06157 ± 0.00260 | 0.06679 | 0.08 | 10 | 0.501 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | naive | 0.4100 | -0.14288 ± 0.00015 | 0.14289 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.4100 | -0.25701 ± 0.01204 | 0.28354 | 0.13 | 10 | 0.356 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | snips | 0.4100 | +0.01341 ± 0.01795 | 0.17912 | 0.85 | 10 | 0.356 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.4100 | -0.00319 ± 0.00040 | 0.00511 | 0.83 | 6563 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | sniips | 0.4100 | +0.01446 ± 0.00038 | 0.01494 | 0.02 | 6563 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | rips | 0.4100 | -0.00407 ± 0.00258 | 0.02600 | 0.73 | 10 | 0.501 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.4100 | +0.02292 ± 0.00511 | 0.05576 | 0.93 | 10 | 0.501 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.4100 | +0.03251 ± 0.00022 | 0.03259 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.4100 | -0.07153 ± 0.00018 | 0.07155 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.4100 | +0.02174 ± 0.00030 | 0.02194 | 0.00 | 6563 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | cdr | 0.4100 | +0.04910 ± 0.00206 | 0.05322 | 0.08 | 10 | 0.501 | 0% |  |  | biased, as expected: 20% of its slots unsupported |

## scifact · ubm · {"policy":"epsilon","epsilon":0.3,"top":10}

100 logs of 100000 impressions, 15 s. Propensities: 10000000 lists, 0 mismatched; chi-square 13509 on 13500 df (p 0.476), largest |z| 4.17.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.5909 | 0.4600 | 0.4519 | 2.282 |
| rerank | 0.0% | 0.5390 | 0.4140 | 0.4059 | 2.304 |
| oracle | 0.0% | 0.7193 | 0.5727 | 0.5577 | 2.232 |
| reverse | 0.0% | 0.0953 | 0.0243 | 0.0238 | 2.522 |
| wide | 20.3% | 0.5342 | 0.4100 | 0.4021 | 2.306 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.5909 | -0.11314 ± 0.00017 | 0.11315 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.5909 | -0.00026 ± 0.00024 | 0.00245 | 0.93 | 70008 | 1.000 | 100% | 0.93 | yes | ok |
| ranking | clicks | snips | 0.5909 | -0.00032 ± 0.00019 | 0.00192 | 0.96 | 70008 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | clicks | iips | 0.5909 | -0.00056 ± 0.00024 | 0.00242 | 0.92 | 72942 | 1.000 | 98% | 0.92 |  | unbiased here |
| ranking | clicks | sniips | 0.5909 | -0.00055 ± 0.00019 | 0.00194 | 0.95 | 72942 | 1.000 | 98% | 0.95 |  | unbiased here |
| ranking | clicks | rips | 0.5909 | -0.00028 ± 0.00024 | 0.00239 | 0.93 | 70008 | 1.000 | 98% | 0.93 | yes | ok |
| ranking | clicks | snrips | 0.5909 | -0.00028 ± 0.00019 | 0.00187 | 0.97 | 70008 | 1.000 | 98% | 0.97 | yes | ok |
| ranking | clicks | dm-pbm | 0.5909 | -0.00060 ± 0.00017 | 0.00180 | 0.75 |  |  | 100% | 0.75 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.5909 | -0.04557 ± 0.00018 | 0.04561 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.5909 | -0.00049 ± 0.00019 | 0.00191 | 0.95 | 72942 | 1.000 | 98% | 0.95 |  | unbiased here |
| ranking | clicks | cdr | 0.5909 | -0.00022 ± 0.00018 | 0.00185 | 0.95 | 70008 | 1.000 | 98% | 0.95 | yes | ok |
| ranking | satisfied | naive | 0.4600 | -0.09920 ± 0.00015 | 0.09921 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.4600 | +0.00012 ± 0.00021 | 0.00212 | 0.98 | 70008 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | satisfied | snips | 0.4600 | +0.00007 ± 0.00019 | 0.00188 | 0.98 | 70008 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | satisfied | iips | 0.4600 | +0.00001 ± 0.00021 | 0.00207 | 0.98 | 72942 | 1.000 | 98% | 0.98 |  | unbiased here |
| ranking | satisfied | sniips | 0.4600 | +0.00001 ± 0.00018 | 0.00183 | 0.99 | 72942 | 1.000 | 98% | 0.99 |  | unbiased here |
| ranking | satisfied | rips | 0.4600 | +0.00009 ± 0.00021 | 0.00208 | 0.98 | 70008 | 1.000 | 98% | 0.98 | yes | ok |
| ranking | satisfied | snrips | 0.4600 | +0.00009 ± 0.00018 | 0.00184 | 0.98 | 70008 | 1.000 | 98% | 0.98 | yes | ok |
| ranking | satisfied | dm-pbm | 0.4600 | +0.00564 ± 0.00017 | 0.00588 | 0.01 |  |  | 100% | 0.01 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.4600 | -0.03011 ± 0.00017 | 0.03016 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.4600 | +0.00008 ± 0.00018 | 0.00176 | 0.97 | 72942 | 1.000 | 98% | 0.97 |  | unbiased here |
| ranking | satisfied | cdr | 0.4600 | +0.00015 ± 0.00018 | 0.00176 | 1.00 | 70008 | 1.000 | 98% | 1.00 | yes | ok |
| rerank | clicks | naive | 0.5390 | -0.06119 ± 0.00017 | 0.06121 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.5390 | +0.00741 ± 0.08405 | 0.83635 | 0.38 | 147 | 0.862 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 147 impressions, mean 0.86; 0% of logs usable) |
| rerank | clicks | snips | 0.5390 | -0.02445 ± 0.04113 | 0.40999 | 0.13 | 147 | 0.862 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 147 impressions, mean 0.86; 0% of logs usable) |
| rerank | clicks | iips | 0.5390 | -0.00209 ± 0.00071 | 0.00736 | 0.93 | 3648 | 1.004 | 99% | 0.93 |  | unbiased here |
| rerank | clicks | sniips | 0.5390 | -0.00229 ± 0.00070 | 0.00731 | 0.93 | 3648 | 1.004 | 99% | 0.93 |  | unbiased here |
| rerank | clicks | rips | 0.5390 | -0.00613 ± 0.00607 | 0.06066 | 0.75 | 3 | 0.628 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.63; 0% of logs usable) |
| rerank | clicks | snrips | 0.5390 | -0.00662 ± 0.00570 | 0.05711 | 0.74 | 3 | 0.628 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.63; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.5390 | +0.00989 ± 0.00020 | 0.01010 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.5390 | +0.00743 ± 0.00018 | 0.00766 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.5390 | -0.00219 ± 0.00057 | 0.00607 | 0.92 | 3648 | 1.004 | 99% | 0.92 |  | unbiased here |
| rerank | clicks | cdr | 0.5390 | -0.01163 ± 0.00886 | 0.08896 | 0.96 | 3 | 0.628 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.63; 0% of logs usable) |
| rerank | satisfied | naive | 0.4140 | -0.05322 ± 0.00015 | 0.05325 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.4140 | +0.02096 ± 0.06759 | 0.67280 | 0.35 | 147 | 0.862 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 147 impressions, mean 0.86; 0% of logs usable) |
| rerank | satisfied | snips | 0.4140 | -0.00395 ± 0.03838 | 0.38193 | 0.15 | 147 | 0.862 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 147 impressions, mean 0.86; 0% of logs usable) |
| rerank | satisfied | iips | 0.4140 | +0.00024 ± 0.00055 | 0.00544 | 0.94 | 3648 | 1.004 | 99% | 0.94 |  | unbiased here |
| rerank | satisfied | sniips | 0.4140 | +0.00011 ± 0.00053 | 0.00529 | 0.98 | 3648 | 1.004 | 99% | 0.98 |  | unbiased here |
| rerank | satisfied | rips | 0.4140 | -0.00265 ± 0.00401 | 0.03999 | 0.64 | 3 | 0.628 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.63; 0% of logs usable) |
| rerank | satisfied | snrips | 0.4140 | -0.00354 ± 0.00393 | 0.03931 | 0.63 | 3 | 0.628 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.63; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.4140 | +0.01402 ± 0.00017 | 0.01413 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.4140 | +0.01671 ± 0.00016 | 0.01679 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.4140 | +0.00025 ± 0.00040 | 0.00402 | 0.99 | 3648 | 1.004 | 99% | 0.99 |  | unbiased here |
| rerank | satisfied | cdr | 0.4140 | -0.00894 ± 0.00708 | 0.07099 | 0.95 | 3 | 0.628 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.63; 0% of logs usable) |
| oracle | clicks | naive | 0.7193 | -0.24152 ± 0.00017 | 0.24153 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.7193 | -0.02966 ± 0.05037 | 0.50208 | 0.14 | 42692 | 0.994 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 42692 impressions, mean 0.99; 0% of logs usable) |
| oracle | clicks | snips | 0.7193 | -0.04158 ± 0.01497 | 0.15467 | 0.12 | 42692 | 0.994 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 42692 impressions, mean 0.99; 0% of logs usable) |
| oracle | clicks | iips | 0.7193 | -0.00451 ± 0.00086 | 0.00972 | 0.90 | 11306 | 0.999 | 99% | 0.90 |  | unbiased here |
| oracle | clicks | sniips | 0.7193 | -0.00471 ± 0.00051 | 0.00695 | 0.87 | 11306 | 0.999 | 99% | 0.87 |  | biased, as expected |
| oracle | clicks | rips | 0.7193 | +0.00640 ± 0.00994 | 0.09911 | 0.89 | 65 | 0.881 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 65 impressions, mean 0.88; 0% of logs usable) |
| oracle | clicks | snrips | 0.7193 | +0.00260 ± 0.00563 | 0.05607 | 0.79 | 65 | 0.881 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 65 impressions, mean 0.88; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.7193 | -0.00265 ± 0.00020 | 0.00332 | 0.34 |  |  | 100% | 0.34 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.7193 | -0.16175 ± 0.00018 | 0.16176 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.7193 | -0.00493 ± 0.00050 | 0.00704 | 0.80 | 11306 | 0.999 | 99% | 0.80 |  | biased, as expected |
| oracle | clicks | cdr | 0.7193 | +0.00664 ± 0.00998 | 0.09951 | 0.91 | 65 | 0.881 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 65 impressions, mean 0.88; 0% of logs usable) |
| oracle | satisfied | naive | 0.5727 | -0.21193 ± 0.00015 | 0.21194 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5727 | -0.00976 ± 0.04902 | 0.48786 | 0.12 | 42692 | 0.994 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 42692 impressions, mean 0.99; 0% of logs usable) |
| oracle | satisfied | snips | 0.5727 | -0.02957 ± 0.01618 | 0.16364 | 0.08 | 42692 | 0.994 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 42692 impressions, mean 0.99; 0% of logs usable) |
| oracle | satisfied | iips | 0.5727 | -0.00272 ± 0.00075 | 0.00795 | 0.92 | 11306 | 0.999 | 99% | 0.92 |  | unbiased here |
| oracle | satisfied | sniips | 0.5727 | -0.00289 ± 0.00050 | 0.00575 | 0.90 | 11306 | 0.999 | 99% | 0.90 |  | unbiased here |
| oracle | satisfied | rips | 0.5727 | -0.00073 ± 0.00112 | 0.01121 | 0.92 | 65 | 0.881 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 65 impressions, mean 0.88; 0% of logs usable) |
| oracle | satisfied | snrips | 0.5727 | -0.00080 ± 0.00102 | 0.01019 | 0.85 | 65 | 0.881 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 65 impressions, mean 0.88; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.5727 | +0.00363 ± 0.00022 | 0.00426 | 0.21 |  |  | 100% | 0.21 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5727 | -0.13077 ± 0.00017 | 0.13078 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5727 | -0.00313 ± 0.00049 | 0.00578 | 0.89 | 11306 | 0.999 | 99% | 0.89 |  | biased, as expected |
| oracle | satisfied | cdr | 0.5727 | -0.00106 ± 0.00108 | 0.01079 | 0.94 | 65 | 0.881 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 65 impressions, mean 0.88; 0% of logs usable) |
| reverse | clicks | naive | 0.0953 | +0.38252 ± 0.00017 | 0.38252 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.0953 | -0.00456 ± 0.02899 | 0.28851 | 0.09 | 1 | 1.008 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.01; 0% of logs usable) |
| reverse | clicks | snips | 0.0953 | -0.03445 ± 0.02145 | 0.21615 | 0.05 | 1 | 1.008 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.01; 0% of logs usable) |
| reverse | clicks | iips | 0.0953 | +0.00179 ± 0.00056 | 0.00586 | 0.97 | 2937 | 0.998 | 100% | 0.97 |  | unbiased here |
| reverse | clicks | sniips | 0.0953 | +0.00186 ± 0.00056 | 0.00590 | 0.96 | 2937 | 0.998 | 100% | 0.96 |  | unbiased here |
| reverse | clicks | rips | 0.0953 | +0.00332 ± 0.01099 | 0.10941 | 0.48 | 1 | 0.719 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.72; 0% of logs usable) |
| reverse | clicks | snrips | 0.0953 | +0.00040 ± 0.01065 | 0.10597 | 0.46 | 1 | 0.719 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.72; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.0953 | +0.10330 ± 0.00031 | 0.10335 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.0953 | +0.05416 ± 0.00024 | 0.05421 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.0953 | +0.00193 ± 0.00062 | 0.00643 | 0.91 | 2937 | 0.998 | 100% | 0.91 |  | unbiased here |
| reverse | clicks | cdr | 0.0953 | +0.00496 ± 0.00924 | 0.09205 | 0.92 | 1 | 0.719 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.72; 0% of logs usable) |
| reverse | satisfied | naive | 0.0243 | +0.33647 ± 0.00015 | 0.33647 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0243 | -0.00414 ± 0.01418 | 0.14118 | 0.02 | 1 | 1.008 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.01; 0% of logs usable) |
| reverse | satisfied | snips | 0.0243 | -0.00930 ± 0.01114 | 0.11118 | 0.01 | 1 | 1.008 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 1.01; 0% of logs usable) |
| reverse | satisfied | iips | 0.0243 | +0.00027 ± 0.00028 | 0.00284 | 0.98 | 2937 | 0.998 | 100% | 0.98 |  | unbiased here |
| reverse | satisfied | sniips | 0.0243 | +0.00029 ± 0.00029 | 0.00286 | 0.97 | 2937 | 0.998 | 100% | 0.97 |  | unbiased here |
| reverse | satisfied | rips | 0.0243 | +0.00789 ± 0.01020 | 0.10184 | 0.51 | 1 | 0.719 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.72; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0243 | +0.00702 ± 0.01004 | 0.10011 | 0.51 | 1 | 0.719 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.72; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0243 | +0.09464 ± 0.00015 | 0.09465 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0243 | +0.05829 ± 0.00012 | 0.05830 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0243 | +0.00028 ± 0.00034 | 0.00339 | 0.93 | 2937 | 0.998 | 100% | 0.93 |  | unbiased here |
| reverse | satisfied | cdr | 0.0243 | +0.00912 ± 0.00836 | 0.08368 | 0.79 | 1 | 0.719 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.72; 0% of logs usable) |
| wide | clicks | naive | 0.5342 | -0.05641 ± 0.00017 | 0.05643 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.5342 | -0.29021 ± 0.05384 | 0.60924 | 0.20 | 267 | 0.287 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snips | 0.5342 | -0.01989 ± 0.03510 | 0.34976 | 0.00 | 267 | 0.287 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.5342 | -0.01256 ± 0.00070 | 0.01436 | 0.47 | 2378 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | sniips | 0.5342 | +0.01647 ± 0.00080 | 0.01828 | 0.45 | 2378 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | rips | 0.5342 | -0.02013 ± 0.00467 | 0.05061 | 0.69 | 4 | 0.187 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snrips | 0.5342 | +0.01691 ± 0.00788 | 0.08022 | 0.74 | 4 | 0.187 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.5342 | +0.02592 ± 0.00021 | 0.02600 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.5342 | +0.06096 ± 0.00018 | 0.06098 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.5342 | +0.01741 ± 0.00056 | 0.01828 | 0.09 | 2378 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | cdr | 0.5342 | +0.07180 ± 0.00496 | 0.08714 | 0.32 | 4 | 0.187 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | naive | 0.4100 | -0.04923 ± 0.00015 | 0.04926 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.4100 | -0.20689 ± 0.04963 | 0.53541 | 0.17 | 267 | 0.287 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | snips | 0.4100 | -0.01428 ± 0.03422 | 0.34079 | 0.00 | 267 | 0.287 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.4100 | -0.00221 ± 0.00056 | 0.00597 | 0.92 | 2378 | 0.641 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | sniips | 0.4100 | +0.01604 ± 0.00061 | 0.01716 | 0.28 | 2378 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | rips | 0.4100 | -0.00500 ± 0.00370 | 0.03718 | 0.62 | 4 | 0.187 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.4100 | +0.01774 ± 0.00666 | 0.06864 | 0.81 | 4 | 0.187 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | dm-pbm | 0.4100 | +0.02984 ± 0.00018 | 0.02990 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.4100 | +0.06120 ± 0.00017 | 0.06123 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.4100 | +0.02049 ± 0.00042 | 0.02091 | 0.00 | 2378 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | cdr | 0.4100 | +0.06242 ± 0.00369 | 0.07242 | 0.26 | 4 | 0.187 | 0% |  |  | biased, as expected: 20% of its slots unsupported |

## scifact · ubm · {"policy":"shuffle","top":10}

100 logs of 100000 impressions, 14 s. Propensities: 10000000 lists, 0 mismatched; chi-square 13444 on 13500 df (p 0.632), largest |z| 4.54.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.5909 | 0.4600 | 0.4519 | 2.282 |
| rerank | 0.0% | 0.5390 | 0.4140 | 0.4059 | 2.304 |
| oracle | 0.0% | 0.7193 | 0.5727 | 0.5577 | 2.232 |
| reverse | 0.0% | 0.0953 | 0.0243 | 0.0238 | 2.522 |
| wide | 20.3% | 0.5342 | 0.4100 | 0.4021 | 2.306 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.5909 | -0.37651 ± 0.00015 | 0.37651 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.5909 | -0.03452 ± 0.04523 | 0.45132 | 0.82 | 3 | 0.986 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.99; 0% of logs usable) |
| ranking | clicks | snips | 0.5909 | -0.02554 ± 0.03851 | 0.38403 | 0.56 | 3 | 0.986 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.99; 0% of logs usable) |
| ranking | clicks | iips | 0.5909 | -0.00623 ± 0.00075 | 0.00973 | 0.86 | 9903 | 1.003 | 99% | 0.86 |  | biased, as expected |
| ranking | clicks | sniips | 0.5909 | -0.00662 ± 0.00055 | 0.00860 | 0.80 | 9903 | 1.003 | 99% | 0.81 |  | biased, as expected |
| ranking | clicks | rips | 0.5909 | -0.00979 ± 0.00280 | 0.02951 | 0.76 | 3 | 0.918 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.92; 0% of logs usable) |
| ranking | clicks | snrips | 0.5909 | -0.01084 ± 0.00272 | 0.02916 | 0.72 | 3 | 0.918 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.92; 0% of logs usable) |
| ranking | clicks | dm-pbm | 0.5909 | -0.00064 ± 0.00027 | 0.00280 | 0.45 |  |  | 100% | 0.45 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.5909 | -0.26781 ± 0.00022 | 0.26782 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.5909 | -0.00626 ± 0.00044 | 0.00764 | 0.72 | 9903 | 1.003 | 99% | 0.72 |  | biased, as expected |
| ranking | clicks | cdr | 0.5909 | -0.00904 ± 0.00350 | 0.03595 | 0.94 | 3 | 0.918 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.92; 0% of logs usable) |
| ranking | satisfied | naive | 0.4600 | -0.33073 ± 0.00010 | 0.33073 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.4600 | -0.00637 ± 0.03951 | 0.39317 | 0.78 | 3 | 0.986 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.99; 0% of logs usable) |
| ranking | satisfied | snips | 0.4600 | +0.00557 ± 0.03445 | 0.34280 | 0.60 | 3 | 0.986 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.99; 0% of logs usable) |
| ranking | satisfied | iips | 0.4600 | -0.00214 ± 0.00063 | 0.00660 | 0.95 | 9903 | 1.003 | 99% | 0.95 |  | unbiased here |
| ranking | satisfied | sniips | 0.4600 | -0.00244 ± 0.00049 | 0.00544 | 0.99 | 9903 | 1.003 | 99% | 0.99 |  | unbiased here |
| ranking | satisfied | rips | 0.4600 | -0.00330 ± 0.00191 | 0.01930 | 0.85 | 3 | 0.918 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.92; 0% of logs usable) |
| ranking | satisfied | snrips | 0.4600 | -0.00360 ± 0.00194 | 0.01968 | 0.86 | 3 | 0.918 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.92; 0% of logs usable) |
| ranking | satisfied | dm-pbm | 0.4600 | -0.00428 ± 0.00028 | 0.00512 | 0.18 |  |  | 100% | 0.18 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.4600 | -0.22259 ± 0.00017 | 0.22260 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.4600 | -0.00209 ± 0.00040 | 0.00447 | 0.97 | 9903 | 1.003 | 99% | 0.97 |  | unbiased here |
| ranking | satisfied | cdr | 0.4600 | -0.00289 ± 0.00241 | 0.02417 | 0.97 | 3 | 0.918 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.92; 0% of logs usable) |
| rerank | clicks | naive | 0.5390 | -0.32455 ± 0.00015 | 0.32456 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.5390 | -0.00071 ± 0.04661 | 0.46373 | 0.81 | 3 | 1.016 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.02; 0% of logs usable) |
| rerank | clicks | snips | 0.5390 | -0.00667 ± 0.03686 | 0.36685 | 0.65 | 3 | 1.016 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.02; 0% of logs usable) |
| rerank | clicks | iips | 0.5390 | -0.00623 ± 0.00072 | 0.00947 | 0.87 | 9902 | 1.001 | 100% | 0.87 |  | biased, as expected |
| rerank | clicks | sniips | 0.5390 | -0.00654 ± 0.00056 | 0.00862 | 0.84 | 9902 | 1.001 | 100% | 0.84 |  | biased, as expected |
| rerank | clicks | rips | 0.5390 | +0.00090 ± 0.00514 | 0.05110 | 0.83 | 3 | 0.953 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| rerank | clicks | snrips | 0.5390 | +0.00106 ± 0.00514 | 0.05120 | 0.78 | 3 | 0.953 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.5390 | -0.00067 ± 0.00027 | 0.00279 | 0.40 |  |  | 100% | 0.40 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.5390 | -0.21634 ± 0.00021 | 0.21635 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.5390 | -0.00616 ± 0.00042 | 0.00744 | 0.80 | 9902 | 1.001 | 100% | 0.80 |  | biased, as expected |
| rerank | clicks | cdr | 0.5390 | -0.00071 ± 0.00523 | 0.05207 | 0.96 | 3 | 0.953 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| rerank | satisfied | naive | 0.4140 | -0.28475 ± 0.00010 | 0.28475 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.4140 | +0.00634 ± 0.03937 | 0.39176 | 0.74 | 3 | 1.016 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.02; 0% of logs usable) |
| rerank | satisfied | snips | 0.4140 | -0.00644 ± 0.03241 | 0.32251 | 0.58 | 3 | 1.016 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.02; 0% of logs usable) |
| rerank | satisfied | iips | 0.4140 | -0.00216 ± 0.00064 | 0.00672 | 0.93 | 9902 | 1.001 | 100% | 0.93 |  | unbiased here |
| rerank | satisfied | sniips | 0.4140 | -0.00240 ± 0.00053 | 0.00581 | 0.95 | 9902 | 1.001 | 100% | 0.95 |  | unbiased here |
| rerank | satisfied | rips | 0.4140 | +0.00229 ± 0.00361 | 0.03599 | 0.91 | 3 | 0.953 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| rerank | satisfied | snrips | 0.4140 | +0.00256 ± 0.00382 | 0.03812 | 0.88 | 3 | 0.953 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.4140 | -0.00448 ± 0.00027 | 0.00521 | 0.13 |  |  | 100% | 0.13 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.4140 | -0.17794 ± 0.00017 | 0.17795 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.4140 | -0.00205 ± 0.00040 | 0.00448 | 0.96 | 9902 | 1.001 | 100% | 0.96 |  | unbiased here |
| rerank | satisfied | cdr | 0.4140 | +0.00130 ± 0.00375 | 0.03737 | 0.94 | 3 | 0.953 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.95; 0% of logs usable) |
| oracle | clicks | naive | 0.7193 | -0.50489 ± 0.00015 | 0.50489 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.7193 | -0.07823 ± 0.04635 | 0.46775 | 0.87 | 3 | 0.959 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.96; 0% of logs usable) |
| oracle | clicks | snips | 0.7193 | -0.09138 ± 0.03301 | 0.34096 | 0.58 | 3 | 0.959 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.96; 0% of logs usable) |
| oracle | clicks | iips | 0.7193 | -0.01046 ± 0.00086 | 0.01353 | 0.68 | 9895 | 1.002 | 100% | 0.68 |  | biased, as expected |
| oracle | clicks | sniips | 0.7193 | -0.01083 ± 0.00052 | 0.01200 | 0.46 | 9895 | 1.002 | 100% | 0.46 |  | biased, as expected |
| oracle | clicks | rips | 0.7193 | -0.00987 ± 0.00219 | 0.02392 | 0.76 | 3 | 0.917 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.92; 0% of logs usable) |
| oracle | clicks | snrips | 0.7193 | -0.01003 ± 0.00216 | 0.02369 | 0.67 | 3 | 0.917 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.92; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.7193 | -0.01211 ± 0.00029 | 0.01246 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.7193 | -0.38322 ± 0.00022 | 0.38323 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.7193 | -0.01045 ± 0.00046 | 0.01142 | 0.32 | 9895 | 1.002 | 100% | 0.32 |  | biased, as expected |
| oracle | clicks | cdr | 0.7193 | -0.00844 ± 0.00213 | 0.02281 | 0.88 | 3 | 0.917 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.92; 0% of logs usable) |
| oracle | satisfied | naive | 0.5727 | -0.44345 ± 0.00010 | 0.44346 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5727 | -0.04350 ± 0.03995 | 0.39983 | 0.83 | 3 | 0.959 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.96; 0% of logs usable) |
| oracle | satisfied | snips | 0.5727 | -0.05139 ± 0.03220 | 0.32447 | 0.63 | 3 | 0.959 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.96; 0% of logs usable) |
| oracle | satisfied | iips | 0.5727 | -0.00466 ± 0.00074 | 0.00872 | 0.92 | 9895 | 1.002 | 100% | 0.92 |  | biased, as expected |
| oracle | satisfied | sniips | 0.5727 | -0.00495 ± 0.00048 | 0.00691 | 0.85 | 9895 | 1.002 | 100% | 0.85 |  | biased, as expected |
| oracle | satisfied | rips | 0.5727 | -0.00176 ± 0.00135 | 0.01350 | 0.90 | 3 | 0.917 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.92; 0% of logs usable) |
| oracle | satisfied | snrips | 0.5727 | -0.00190 ± 0.00125 | 0.01254 | 0.87 | 3 | 0.917 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.92; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.5727 | -0.01442 ± 0.00031 | 0.01474 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5727 | -0.32273 ± 0.00017 | 0.32274 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5727 | -0.00462 ± 0.00045 | 0.00644 | 0.85 | 9895 | 1.002 | 100% | 0.85 |  | biased, as expected |
| oracle | satisfied | cdr | 0.5727 | -0.00150 ± 0.00122 | 0.01220 | 0.93 | 3 | 0.917 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.92; 0% of logs usable) |
| reverse | clicks | naive | 0.0953 | +0.11915 ± 0.00015 | 0.11916 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.0953 | +0.02870 ± 0.01974 | 0.19851 | 0.33 | 3 | 0.986 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.99; 0% of logs usable) |
| reverse | clicks | snips | 0.0953 | +0.03222 ± 0.02174 | 0.21869 | 0.30 | 3 | 0.986 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.99; 0% of logs usable) |
| reverse | clicks | iips | 0.0953 | +0.00169 ± 0.00032 | 0.00358 | 0.92 | 9886 | 0.999 | 97% | 0.93 |  | biased, as expected |
| reverse | clicks | sniips | 0.0953 | +0.00167 ± 0.00031 | 0.00347 | 0.92 | 9886 | 0.999 | 97% | 0.93 |  | biased, as expected |
| reverse | clicks | rips | 0.0953 | +0.00909 ± 0.00736 | 0.07382 | 0.64 | 3 | 0.929 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| reverse | clicks | snrips | 0.0953 | +0.01387 ± 0.00957 | 0.09627 | 0.62 | 3 | 0.929 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.0953 | +0.04387 ± 0.00015 | 0.04390 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.0953 | +0.01567 ± 0.00014 | 0.01573 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.0953 | +0.00159 ± 0.00030 | 0.00337 | 0.89 | 9886 | 0.999 | 97% | 0.90 |  | biased, as expected |
| reverse | clicks | cdr | 0.0953 | +0.01229 ± 0.00732 | 0.07390 | 0.97 | 3 | 0.929 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| reverse | satisfied | naive | 0.0243 | +0.10495 ± 0.00010 | 0.10495 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0243 | +0.00594 ± 0.01008 | 0.10047 | 0.09 | 3 | 0.986 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.99; 0% of logs usable) |
| reverse | satisfied | snips | 0.0243 | +0.00970 ± 0.01372 | 0.13686 | 0.07 | 3 | 0.986 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.99; 0% of logs usable) |
| reverse | satisfied | iips | 0.0243 | +0.00036 ± 0.00015 | 0.00151 | 0.95 | 9886 | 0.999 | 97% | 0.95 |  | unbiased here |
| reverse | satisfied | sniips | 0.0243 | +0.00035 ± 0.00015 | 0.00149 | 0.96 | 9886 | 0.999 | 97% | 0.96 |  | unbiased here |
| reverse | satisfied | rips | 0.0243 | +0.00369 ± 0.00435 | 0.04345 | 0.56 | 3 | 0.929 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0243 | +0.00439 ± 0.00541 | 0.05401 | 0.56 | 3 | 0.929 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0243 | +0.03323 ± 0.00008 | 0.03324 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0243 | +0.01845 ± 0.00008 | 0.01847 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0243 | +0.00030 ± 0.00014 | 0.00142 | 0.94 | 9886 | 0.999 | 97% | 0.95 |  | unbiased here |
| reverse | satisfied | cdr | 0.0243 | +0.00478 ± 0.00381 | 0.03820 | 0.96 | 3 | 0.929 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.93; 0% of logs usable) |
| wide | clicks | naive | 0.5342 | -0.31978 ± 0.00015 | 0.31978 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.5342 | -0.37998 ± 0.02254 | 0.44122 | 0.39 | 1 | 0.293 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snips | 0.5342 | -0.19188 ± 0.04651 | 0.50095 | 0.15 | 1 | 0.293 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | iips | 0.5342 | -0.01641 ± 0.00073 | 0.01792 | 0.33 | 6409 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | sniips | 0.5342 | +0.01168 ± 0.00061 | 0.01315 | 0.57 | 6409 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | rips | 0.5342 | -0.02138 ± 0.00234 | 0.03159 | 0.64 | 1 | 0.400 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snrips | 0.5342 | +0.01624 ± 0.00454 | 0.04804 | 0.96 | 1 | 0.400 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.5342 | +0.01211 ± 0.00028 | 0.01243 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.5342 | -0.19642 ± 0.00022 | 0.19644 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.5342 | +0.00900 ± 0.00041 | 0.00989 | 0.51 | 6409 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | cdr | 0.5342 | +0.02988 ± 0.00308 | 0.04280 | 0.66 | 1 | 0.400 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | naive | 0.4100 | -0.28076 ± 0.00010 | 0.28076 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.4100 | -0.28300 ± 0.02024 | 0.34735 | 0.33 | 1 | 0.293 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | snips | 0.4100 | -0.14517 ± 0.04011 | 0.42472 | 0.15 | 1 | 0.293 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.4100 | -0.00437 ± 0.00065 | 0.00783 | 0.86 | 6409 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | sniips | 0.4100 | +0.01344 ± 0.00058 | 0.01464 | 0.36 | 6409 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | rips | 0.4100 | -0.00633 ± 0.00179 | 0.01888 | 0.89 | 1 | 0.400 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.4100 | +0.01756 ± 0.00308 | 0.03532 | 0.95 | 1 | 0.400 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.4100 | +0.00632 ± 0.00027 | 0.00688 | 0.02 |  |  | 100% | 0.02 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.4100 | -0.16218 ± 0.00018 | 0.16219 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.4100 | +0.01101 ± 0.00042 | 0.01177 | 0.29 | 6409 | 0.641 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | cdr | 0.4100 | +0.02252 ± 0.00235 | 0.03249 | 0.67 | 1 | 0.400 | 0% |  |  | biased, as expected: 20% of its slots unsupported |

## scifact · ubm · {"policy":"swap","top":10}

100 logs of 100000 impressions, 12 s. Propensities: 10000000 lists, 0 mismatched; chi-square 3881 on 3900 df (p 0.582), largest |z| 3.70.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.5909 | 0.4600 | 0.4519 | 2.282 |
| rerank | 54.3% | 0.5390 | 0.4140 | 0.4059 | 2.304 |
| oracle | 5.8% | 0.7193 | 0.5727 | 0.5577 | 2.232 |
| reverse | 80.0% | 0.0953 | 0.0243 | 0.0238 | 2.522 |
| wide | 61.2% | 0.5342 | 0.4100 | 0.4021 | 2.306 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.5909 | -0.34729 ± 0.00015 | 0.34730 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.5909 | -0.00147 ± 0.00080 | 0.00806 | 0.95 | 10001 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | clicks | snips | 0.5909 | -0.00154 ± 0.00050 | 0.00518 | 0.97 | 10001 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | clicks | iips | 0.5909 | -0.00750 ± 0.00068 | 0.01008 | 0.75 | 10001 | 1.000 | 100% | 0.75 |  | biased, as expected |
| ranking | clicks | sniips | 0.5909 | -0.00756 ± 0.00048 | 0.00892 | 0.68 | 10001 | 1.000 | 100% | 0.68 |  | biased, as expected |
| ranking | clicks | rips | 0.5909 | -0.00147 ± 0.00080 | 0.00806 | 0.95 | 10001 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | clicks | snrips | 0.5909 | -0.00154 ± 0.00050 | 0.00518 | 0.97 | 10001 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | clicks | dm-pbm | 0.5909 | -0.00602 ± 0.00026 | 0.00656 | 0.06 |  |  | 100% | 0.06 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.5909 | -0.27289 ± 0.00019 | 0.27290 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.5909 | -0.00712 ± 0.00035 | 0.00794 | 0.55 | 10001 | 1.000 | 100% | 0.55 |  | biased, as expected |
| ranking | clicks | cdr | 0.5909 | -0.00139 ± 0.00054 | 0.00559 | 0.97 | 10001 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | satisfied | naive | 0.4600 | -0.30531 ± 0.00012 | 0.30532 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.4600 | -0.00178 ± 0.00067 | 0.00687 | 0.94 | 10001 | 1.000 | 100% | 0.94 | yes | ok |
| ranking | satisfied | snips | 0.4600 | -0.00183 ± 0.00050 | 0.00529 | 0.96 | 10001 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | satisfied | iips | 0.4600 | -0.00330 ± 0.00063 | 0.00705 | 0.90 | 10001 | 1.000 | 100% | 0.90 |  | unbiased here |
| ranking | satisfied | sniips | 0.4600 | -0.00335 ± 0.00050 | 0.00600 | 0.90 | 10001 | 1.000 | 100% | 0.90 |  | biased, as expected |
| ranking | satisfied | rips | 0.4600 | -0.00178 ± 0.00067 | 0.00687 | 0.94 | 10001 | 1.000 | 100% | 0.94 | yes | ok |
| ranking | satisfied | snrips | 0.4600 | -0.00183 ± 0.00050 | 0.00529 | 0.96 | 10001 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | satisfied | dm-pbm | 0.4600 | -0.00722 ± 0.00029 | 0.00776 | 0.01 |  |  | 100% | 0.01 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.4600 | -0.22302 ± 0.00018 | 0.22302 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.4600 | -0.00291 ± 0.00041 | 0.00499 | 0.89 | 10001 | 1.000 | 100% | 0.89 |  | biased, as expected |
| ranking | satisfied | cdr | 0.4600 | -0.00168 ± 0.00050 | 0.00526 | 0.96 | 10001 | 1.000 | 100% | 0.96 | yes | ok |
| rerank | clicks | naive | 0.5390 | -0.29534 ± 0.00015 | 0.29534 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.5390 | -0.53378 ± 0.00008 | 0.53378 | 0.00 | 199 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | snips | 0.5390 | -0.27722 ± 0.00334 | 0.27920 | 0.00 | 199 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | iips | 0.5390 | -0.06388 ± 0.00065 | 0.06421 | 0.00 | 8616 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | sniips | 0.5390 | +0.02185 ± 0.00065 | 0.02278 | 0.09 | 8616 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | rips | 0.5390 | -0.09386 ± 0.00067 | 0.09410 | 0.00 | 199 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | snrips | 0.5390 | -0.02679 ± 0.00090 | 0.02824 | 0.20 | 199 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | dm-pbm | 0.5390 | +0.00192 ± 0.00028 | 0.00336 | 0.38 |  |  | 100% | 0.38 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.5390 | -0.18378 ± 0.00027 | 0.18380 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.5390 | +0.00450 ± 0.00037 | 0.00582 | 0.81 | 8616 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | cdr | 0.5390 | +0.05718 ± 0.00052 | 0.05741 | 0.00 | 199 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | naive | 0.4140 | -0.25934 ± 0.00012 | 0.25934 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.4140 | -0.41052 ± 0.00006 | 0.41052 | 0.00 | 199 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | snips | 0.4140 | -0.23947 ± 0.00267 | 0.24094 | 0.00 | 199 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | iips | 0.4140 | -0.03306 ± 0.00059 | 0.03357 | 0.00 | 8616 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | sniips | 0.4140 | +0.02318 ± 0.00063 | 0.02402 | 0.04 | 8616 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | rips | 0.4140 | -0.05748 ± 0.00060 | 0.05779 | 0.00 | 199 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | snrips | 0.4140 | -0.02387 ± 0.00070 | 0.02486 | 0.07 | 199 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | dm-pbm | 0.4140 | -0.00093 ± 0.00028 | 0.00297 | 0.43 |  |  | 100% | 0.43 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.4140 | -0.15925 ± 0.00020 | 0.15926 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.4140 | +0.00717 ± 0.00041 | 0.00826 | 0.52 | 8616 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | cdr | 0.4140 | +0.03728 ± 0.00047 | 0.03757 | 0.00 | 199 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| oracle | clicks | naive | 0.7193 | -0.47567 ± 0.00015 | 0.47568 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.7193 | -0.12090 ± 0.00078 | 0.12115 | 0.00 | 8704 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | snips | 0.7193 | -0.03180 ± 0.00054 | 0.03226 | 0.00 | 8704 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | iips | 0.7193 | -0.02302 ± 0.00083 | 0.02446 | 0.18 | 10001 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | sniips | 0.7193 | -0.02017 ± 0.00052 | 0.02081 | 0.04 | 10001 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | rips | 0.7193 | -0.02012 ± 0.00087 | 0.02189 | 0.36 | 8704 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | snrips | 0.7193 | -0.01326 ± 0.00052 | 0.01422 | 0.29 | 8704 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | dm-pbm | 0.7193 | -0.02198 ± 0.00030 | 0.02218 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.7193 | -0.37408 ± 0.00020 | 0.37408 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.7193 | -0.01126 ± 0.00045 | 0.01210 | 0.28 | 10001 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | cdr | 0.7193 | -0.00575 ± 0.00061 | 0.00838 | 0.85 | 8704 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | naive | 0.5727 | -0.41804 ± 0.00012 | 0.41804 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5727 | -0.09897 ± 0.00068 | 0.09920 | 0.00 | 8704 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | snips | 0.5727 | -0.02843 ± 0.00058 | 0.02902 | 0.00 | 8704 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | iips | 0.5727 | -0.01403 ± 0.00073 | 0.01580 | 0.50 | 10001 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | sniips | 0.5727 | -0.01358 ± 0.00054 | 0.01462 | 0.23 | 10001 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | rips | 0.5727 | -0.01315 ± 0.00072 | 0.01499 | 0.54 | 8704 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | snrips | 0.5727 | -0.01175 ± 0.00053 | 0.01289 | 0.33 | 8704 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | dm-pbm | 0.5727 | -0.02119 ± 0.00032 | 0.02143 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5727 | -0.31134 ± 0.00020 | 0.31135 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5727 | -0.00570 ± 0.00048 | 0.00743 | 0.73 | 10001 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | cdr | 0.5727 | -0.00394 ± 0.00056 | 0.00686 | 0.90 | 8704 | 0.870 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| reverse | clicks | naive | 0.0953 | +0.14837 ± 0.00015 | 0.14837 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.0953 | -0.09528 ± 0.00000 | 0.09528 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snips | 0.0953 | -0.09528 ± 0.00000 | 0.09528 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | iips | 0.0953 | -0.04879 ± 0.00023 | 0.04884 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | sniips | 0.0953 | -0.04877 ± 0.00023 | 0.04882 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | rips | 0.0953 | -0.04879 ± 0.00023 | 0.04884 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snrips | 0.0953 | -0.04877 ± 0.00023 | 0.04882 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | dm-pbm | 0.0953 | +0.07099 ± 0.00024 | 0.07103 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.0953 | +0.12578 ± 0.00038 | 0.12584 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.0953 | +0.03293 ± 0.00026 | 0.03303 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | cdr | 0.0953 | +0.12544 ± 0.00039 | 0.12550 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | naive | 0.0243 | +0.13036 ± 0.00012 | 0.13036 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0243 | -0.02430 ± 0.00000 | 0.02430 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snips | 0.0243 | -0.02430 ± 0.00000 | 0.02430 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | iips | 0.0243 | -0.01233 ± 0.00011 | 0.01238 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | sniips | 0.0243 | -0.01232 ± 0.00011 | 0.01238 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | rips | 0.0243 | -0.01233 ± 0.00011 | 0.01238 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snrips | 0.0243 | -0.01232 ± 0.00011 | 0.01238 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | dm-pbm | 0.0243 | +0.05694 ± 0.00010 | 0.05695 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0243 | +0.07035 ± 0.00015 | 0.07037 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0243 | +0.02743 ± 0.00012 | 0.02746 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | cdr | 0.0243 | +0.06231 ± 0.00017 | 0.06233 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| wide | clicks | naive | 0.5342 | -0.29056 ± 0.00015 | 0.29056 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.5342 | -0.52927 ± 0.00008 | 0.52927 | 0.00 | 165 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | snips | 0.5342 | -0.23526 ± 0.00376 | 0.23821 | 0.00 | 165 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | iips | 0.5342 | -0.06396 ± 0.00064 | 0.06428 | 0.00 | 4213 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | sniips | 0.5342 | +0.04933 ± 0.00074 | 0.04987 | 0.00 | 4213 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | rips | 0.5342 | -0.09201 ± 0.00067 | 0.09224 | 0.00 | 165 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | snrips | 0.5342 | -0.00904 ± 0.00098 | 0.01329 | 0.83 | 165 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | dm-pbm | 0.5342 | +0.01618 ± 0.00028 | 0.01642 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.5342 | -0.16667 ± 0.00025 | 0.16668 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.5342 | +0.01972 ± 0.00038 | 0.02007 | 0.00 | 4213 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | cdr | 0.5342 | +0.07350 ± 0.00051 | 0.07368 | 0.00 | 165 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | naive | 0.4100 | -0.25535 ± 0.00012 | 0.25535 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.4100 | -0.40657 ± 0.00006 | 0.40657 | 0.00 | 165 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | snips | 0.4100 | -0.20173 ± 0.00312 | 0.20410 | 0.00 | 165 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | iips | 0.4100 | -0.03099 ± 0.00058 | 0.03153 | 0.00 | 4213 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | sniips | 0.4100 | +0.04615 ± 0.00069 | 0.04666 | 0.00 | 4213 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | rips | 0.4100 | -0.05440 ± 0.00060 | 0.05472 | 0.00 | 165 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | snrips | 0.4100 | -0.00987 ± 0.00077 | 0.01251 | 0.63 | 165 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.4100 | +0.01133 ± 0.00029 | 0.01169 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.4100 | -0.14311 ± 0.00021 | 0.14312 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.4100 | +0.02024 ± 0.00042 | 0.02066 | 0.00 | 4213 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | cdr | 0.4100 | +0.05326 ± 0.00048 | 0.05347 | 0.00 | 165 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |

## scifact · trust · {"policy":"softmax","tau":1.0,"top":10,"by":"rank"}

100 logs of 100000 impressions, 14 s. Propensities: 10000000 lists, 0 mismatched; chi-square 13521 on 13500 df (p 0.447), largest |z| 4.84.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.8470 | 0.4563 | 0.4506 | 2.282 |
| rerank | 0.0% | 0.8150 | 0.4100 | 0.4043 | 2.305 |
| oracle | 0.0% | 0.9250 | 0.5669 | 0.5569 | 2.232 |
| reverse | 0.0% | 0.5059 | 0.0238 | 0.0235 | 2.522 |
| wide | 20.3% | 0.8111 | 0.4061 | 0.4006 | 2.307 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.8470 | -0.13763 ± 0.00019 | 0.13764 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.8470 | +0.00004 ± 0.00971 | 0.09661 | 0.95 | 111 | 0.997 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snips | 0.8470 | +0.00258 ± 0.00561 | 0.05584 | 0.97 | 111 | 0.997 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | iips | 0.8470 | +0.00121 ± 0.00047 | 0.00479 | 0.98 | 11567 | 1.002 | 100% | 0.98 | yes | ok |
| ranking | clicks | sniips | 0.8470 | +0.00098 ± 0.00037 | 0.00384 | 0.98 | 11567 | 1.002 | 100% | 0.98 | yes | ok |
| ranking | clicks | rips | 0.8470 | +0.00274 ± 0.00160 | 0.01613 | 0.96 | 111 | 0.995 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | snrips | 0.8470 | +0.00253 ± 0.00156 | 0.01568 | 0.94 | 111 | 0.995 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | clicks | dm-pbm | 0.8470 | +0.01988 ± 0.00021 | 0.01999 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.8470 | -0.06204 ± 0.00021 | 0.06208 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.8470 | +0.00085 ± 0.00035 | 0.00362 | 0.98 | 11567 | 1.002 | 100% | 0.98 | yes | ok |
| ranking | clicks | cdr | 0.8470 | +0.00316 ± 0.00153 | 0.01556 | 0.95 | 111 | 0.995 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | naive | 0.4563 | -0.19295 ± 0.00015 | 0.19295 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.4563 | -0.00339 ± 0.00564 | 0.05622 | 0.97 | 111 | 0.997 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snips | 0.4563 | -0.00146 ± 0.00454 | 0.04520 | 0.96 | 111 | 0.997 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | iips | 0.4563 | +0.00013 ± 0.00037 | 0.00370 | 0.93 | 11567 | 1.002 | 100% | 0.93 | yes | ok |
| ranking | satisfied | sniips | 0.4563 | +0.00001 ± 0.00031 | 0.00313 | 0.96 | 11567 | 1.002 | 100% | 0.96 | yes | ok |
| ranking | satisfied | rips | 0.4563 | +0.00109 ± 0.00080 | 0.00800 | 0.91 | 111 | 0.995 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | snrips | 0.4563 | +0.00105 ± 0.00079 | 0.00793 | 0.90 | 111 | 0.995 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| ranking | satisfied | dm-pbm | 0.4563 | -0.00052 ± 0.00022 | 0.00222 | 0.57 |  |  | 100% | 0.57 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.4563 | -0.11054 ± 0.00018 | 0.11056 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.4563 | -0.00010 ± 0.00027 | 0.00265 | 0.98 | 11567 | 1.002 | 100% | 0.98 | yes | ok |
| ranking | satisfied | cdr | 0.4563 | +0.00119 ± 0.00071 | 0.00720 | 0.94 | 111 | 0.995 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| rerank | clicks | naive | 0.8150 | -0.10563 ± 0.00019 | 0.10565 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.8150 | -0.04373 ± 0.04634 | 0.46311 | 0.78 | 18 | 0.973 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.97; 3% of logs usable) |
| rerank | clicks | snips | 0.8150 | -0.01660 ± 0.01955 | 0.19521 | 0.86 | 18 | 0.973 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.97; 3% of logs usable) |
| rerank | clicks | iips | 0.8150 | +0.00115 ± 0.00064 | 0.00643 | 0.94 | 9482 | 0.999 | 99% | 0.95 | yes | ok |
| rerank | clicks | sniips | 0.8150 | +0.00059 ± 0.00053 | 0.00530 | 0.95 | 9482 | 0.999 | 99% | 0.95 | yes | ok |
| rerank | clicks | rips | 0.8150 | -0.00369 ± 0.00373 | 0.03732 | 0.81 | 18 | 0.939 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.94; 3% of logs usable) |
| rerank | clicks | snrips | 0.8150 | -0.00283 ± 0.00405 | 0.04038 | 0.79 | 18 | 0.939 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.94; 3% of logs usable) |
| rerank | clicks | dm-pbm | 0.8150 | +0.00513 ± 0.00031 | 0.00598 | 0.07 |  |  | 100% | 0.07 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.8150 | -0.03822 ± 0.00024 | 0.03829 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.8150 | +0.00063 ± 0.00052 | 0.00518 | 0.96 | 9482 | 0.999 | 99% | 0.96 | yes | ok |
| rerank | clicks | cdr | 0.8150 | -0.00542 ± 0.00588 | 0.05873 | 0.97 | 18 | 0.939 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.94; 3% of logs usable) |
| rerank | satisfied | naive | 0.4100 | -0.14657 ± 0.00015 | 0.14658 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.4100 | -0.02956 ± 0.01822 | 0.18365 | 0.84 | 18 | 0.973 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.97; 3% of logs usable) |
| rerank | satisfied | snips | 0.4100 | +0.00119 ± 0.01510 | 0.15023 | 0.81 | 18 | 0.973 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.97; 3% of logs usable) |
| rerank | satisfied | iips | 0.4100 | +0.00006 ± 0.00041 | 0.00409 | 0.94 | 9482 | 0.999 | 99% | 0.94 | yes | ok |
| rerank | satisfied | sniips | 0.4100 | -0.00027 ± 0.00041 | 0.00405 | 0.92 | 9482 | 0.999 | 99% | 0.92 | yes | ok |
| rerank | satisfied | rips | 0.4100 | -0.00191 ± 0.00137 | 0.01379 | 0.84 | 18 | 0.939 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.94; 3% of logs usable) |
| rerank | satisfied | snrips | 0.4100 | -0.00161 ± 0.00141 | 0.01416 | 0.84 | 18 | 0.939 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.94; 3% of logs usable) |
| rerank | satisfied | dm-pbm | 0.4100 | -0.00280 ± 0.00022 | 0.00354 | 0.24 |  |  | 100% | 0.24 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.4100 | -0.07177 ± 0.00019 | 0.07179 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.4100 | -0.00023 ± 0.00030 | 0.00304 | 0.95 | 9482 | 0.999 | 99% | 0.95 | yes | ok |
| rerank | satisfied | cdr | 0.4100 | -0.00128 ± 0.00147 | 0.01472 | 0.93 | 18 | 0.939 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 18 impressions, mean 0.94; 3% of logs usable) |
| oracle | clicks | naive | 0.9250 | -0.21568 ± 0.00019 | 0.21569 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.9250 | -0.00746 ± 0.01072 | 0.10692 | 0.97 | 94 | 0.987 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snips | 0.9250 | +0.00432 ± 0.00640 | 0.06385 | 0.94 | 94 | 0.987 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | iips | 0.9250 | +0.00046 ± 0.00060 | 0.00602 | 0.97 | 11625 | 1.000 | 100% | 0.97 | yes | ok |
| oracle | clicks | sniips | 0.9250 | +0.00065 ± 0.00040 | 0.00404 | 0.97 | 11625 | 1.000 | 100% | 0.97 | yes | ok |
| oracle | clicks | rips | 0.9250 | +0.00251 ± 0.00187 | 0.01876 | 0.93 | 94 | 0.988 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | snrips | 0.9250 | +0.00324 ± 0.00180 | 0.01818 | 0.91 | 94 | 0.988 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | clicks | dm-pbm | 0.9250 | +0.03208 ± 0.00023 | 0.03216 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.9250 | -0.13710 ± 0.00021 | 0.13711 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.9250 | +0.00049 ± 0.00039 | 0.00388 | 0.98 | 11625 | 1.000 | 100% | 0.98 | yes | ok |
| oracle | clicks | cdr | 0.9250 | +0.00433 ± 0.00184 | 0.01878 | 0.96 | 94 | 0.988 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | naive | 0.5669 | -0.30354 ± 0.00015 | 0.30354 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5669 | -0.01385 ± 0.00671 | 0.06816 | 0.97 | 94 | 0.987 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snips | 0.5669 | -0.00628 ± 0.00486 | 0.04872 | 0.99 | 94 | 0.987 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | iips | 0.5669 | -0.00019 ± 0.00047 | 0.00472 | 0.95 | 11625 | 1.000 | 100% | 0.95 | yes | ok |
| oracle | satisfied | sniips | 0.5669 | -0.00005 ± 0.00034 | 0.00339 | 0.93 | 11625 | 1.000 | 100% | 0.93 | yes | ok |
| oracle | satisfied | rips | 0.5669 | -0.00055 ± 0.00062 | 0.00620 | 0.98 | 94 | 0.988 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | snrips | 0.5669 | -0.00032 ± 0.00056 | 0.00562 | 0.93 | 94 | 0.988 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| oracle | satisfied | dm-pbm | 0.5669 | +0.00025 ± 0.00026 | 0.00263 | 0.49 |  |  | 100% | 0.49 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5669 | -0.19561 ± 0.00020 | 0.19563 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5669 | -0.00020 ± 0.00032 | 0.00317 | 0.96 | 11625 | 1.000 | 100% | 0.96 | yes | ok |
| oracle | satisfied | cdr | 0.5669 | -0.00023 ± 0.00055 | 0.00548 | 0.95 | 94 | 0.988 | 0% |  | yes | unbiased; the app warns on 100% of its logs, so its intervals are not judged |
| reverse | clicks | naive | 0.5059 | +0.20342 ± 0.00019 | 0.20343 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.5059 | -0.07329 ± 0.43261 | 4.30503 | 0.01 | 0 | 0.865 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.87; 0% of logs usable) |
| reverse | clicks | snips | 0.5059 | -0.49590 ± 0.01000 | 0.50579 | 0.00 | 0 | 0.865 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.87; 0% of logs usable) |
| reverse | clicks | iips | 0.5059 | -0.00147 ± 0.00112 | 0.01128 | 0.94 | 3407 | 0.998 | 98% | 0.95 | yes | ok |
| reverse | clicks | sniips | 0.5059 | -0.00074 ± 0.00091 | 0.00905 | 0.96 | 3407 | 0.998 | 98% | 0.96 | yes | ok |
| reverse | clicks | rips | 0.5059 | +0.00672 ± 0.04190 | 0.41698 | 0.32 | 0 | 0.521 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.52; 0% of logs usable) |
| reverse | clicks | snrips | 0.5059 | -0.03391 ± 0.01317 | 0.13539 | 0.26 | 0 | 0.521 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.52; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.5059 | -0.00108 ± 0.00054 | 0.00544 | 0.15 |  |  | 100% | 0.15 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.5059 | +0.08057 ± 0.00034 | 0.08064 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.5059 | -0.00110 ± 0.00089 | 0.00896 | 0.98 | 3407 | 0.998 | 98% | 0.98 | yes | ok |
| reverse | clicks | cdr | 0.5059 | +0.02278 ± 0.07820 | 0.77843 | 0.58 | 0 | 0.521 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.52; 0% of logs usable) |
| reverse | satisfied | naive | 0.0238 | +0.23958 ± 0.00015 | 0.23959 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0238 | -0.02380 ± 0.00000 | 0.02380 | 0.00 | 0 | 0.865 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.87; 0% of logs usable) |
| reverse | satisfied | snips | 0.0238 | -0.02380 ± 0.00000 | 0.02380 | 0.00 | 0 | 0.865 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.87; 0% of logs usable) |
| reverse | satisfied | iips | 0.0238 | +0.00006 ± 0.00022 | 0.00214 | 0.97 | 3407 | 0.998 | 98% | 0.97 | yes | ok |
| reverse | satisfied | sniips | 0.0238 | +0.00009 ± 0.00022 | 0.00215 | 0.97 | 3407 | 0.998 | 98% | 0.97 | yes | ok |
| reverse | satisfied | rips | 0.0238 | -0.00695 ± 0.00168 | 0.01812 | 0.34 | 0 | 0.521 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.52; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0238 | -0.00523 ± 0.00335 | 0.03371 | 0.34 | 0 | 0.521 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.52; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0238 | +0.04283 ± 0.00010 | 0.04284 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0238 | +0.05046 ± 0.00009 | 0.05046 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0238 | +0.00008 ± 0.00022 | 0.00217 | 0.95 | 3407 | 0.998 | 98% | 0.95 | yes | ok |
| reverse | satisfied | cdr | 0.0238 | +0.00852 ± 0.00354 | 0.03621 | 0.53 | 0 | 0.521 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 0 impressions, mean 0.52; 0% of logs usable) |
| wide | clicks | naive | 0.8111 | -0.10179 ± 0.00019 | 0.10180 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.8111 | -0.52542 ± 0.01331 | 0.54185 | 0.10 | 11 | 0.353 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snips | 0.8111 | -0.00995 ± 0.01911 | 0.19035 | 0.88 | 11 | 0.353 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.8111 | -0.03220 ± 0.00056 | 0.03269 | 0.00 | 6558 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | sniips | 0.8111 | +0.01554 ± 0.00054 | 0.01645 | 0.23 | 6558 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | rips | 0.8111 | -0.05017 ± 0.00219 | 0.05471 | 0.21 | 11 | 0.500 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snrips | 0.8111 | +0.01409 ± 0.00503 | 0.05196 | 0.90 | 11 | 0.500 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.8111 | +0.01742 ± 0.00031 | 0.01768 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.8111 | -0.00654 ± 0.00023 | 0.00694 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.8111 | +0.02005 ± 0.00047 | 0.02059 | 0.01 | 6558 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | cdr | 0.8111 | +0.10484 ± 0.00235 | 0.10742 | 0.03 | 11 | 0.500 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | naive | 0.4061 | -0.14270 ± 0.00015 | 0.14270 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.4061 | -0.25824 ± 0.00849 | 0.27171 | 0.15 | 11 | 0.353 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | snips | 0.4061 | +0.00597 ± 0.01639 | 0.16315 | 0.88 | 11 | 0.353 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.4061 | -0.00185 ± 0.00038 | 0.00417 | 0.93 | 6558 | 0.640 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | sniips | 0.4061 | +0.01529 ± 0.00041 | 0.01583 | 0.03 | 6558 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | rips | 0.4061 | -0.00648 ± 0.00100 | 0.01190 | 0.78 | 11 | 0.500 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | snrips | 0.4061 | +0.01669 ± 0.00212 | 0.02687 | 0.87 | 11 | 0.500 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.4061 | +0.01397 ± 0.00023 | 0.01415 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.4061 | -0.03962 ± 0.00020 | 0.03968 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.4061 | +0.01745 ± 0.00028 | 0.01767 | 0.00 | 6558 | 0.640 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | cdr | 0.4061 | +0.04151 ± 0.00102 | 0.04274 | 0.04 | 11 | 0.500 | 0% |  |  | biased, as expected: 20% of its slots unsupported |

## scifact · trust · {"policy":"epsilon","epsilon":0.3,"top":10}

100 logs of 100000 impressions, 13 s. Propensities: 10000000 lists, 0 mismatched; chi-square 13335 on 13500 df (p 0.843), largest |z| 4.12.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.8470 | 0.4563 | 0.4506 | 2.282 |
| rerank | 0.0% | 0.8150 | 0.4100 | 0.4043 | 2.305 |
| oracle | 0.0% | 0.9250 | 0.5669 | 0.5569 | 2.232 |
| reverse | 0.0% | 0.5059 | 0.0238 | 0.0235 | 2.522 |
| wide | 20.3% | 0.8111 | 0.4061 | 0.4006 | 2.307 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.8470 | -0.07517 ± 0.00023 | 0.07520 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.8470 | -0.00010 ± 0.00032 | 0.00315 | 0.94 | 70021 | 1.000 | 100% | 0.94 | yes | ok |
| ranking | clicks | snips | 0.8470 | -0.00033 ± 0.00025 | 0.00249 | 0.94 | 70021 | 1.000 | 100% | 0.94 | yes | ok |
| ranking | clicks | iips | 0.8470 | -0.00014 ± 0.00030 | 0.00294 | 0.92 | 72958 | 1.000 | 100% | 0.92 | yes | ok |
| ranking | clicks | sniips | 0.8470 | -0.00032 ± 0.00024 | 0.00243 | 0.95 | 72958 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | clicks | rips | 0.8470 | -0.00012 ± 0.00030 | 0.00298 | 0.92 | 70021 | 1.000 | 100% | 0.92 | yes | ok |
| ranking | clicks | snrips | 0.8470 | -0.00031 ± 0.00024 | 0.00244 | 0.95 | 70021 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | clicks | dm-pbm | 0.8470 | -0.00461 ± 0.00024 | 0.00517 | 0.08 |  |  | 100% | 0.08 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.8470 | -0.01049 ± 0.00023 | 0.01073 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.8470 | -0.00024 ± 0.00024 | 0.00242 | 0.94 | 72958 | 1.000 | 100% | 0.94 | yes | ok |
| ranking | clicks | cdr | 0.8470 | -0.00024 ± 0.00024 | 0.00241 | 0.95 | 70021 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | satisfied | naive | 0.4563 | -0.09892 ± 0.00016 | 0.09894 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.4563 | -0.00025 ± 0.00023 | 0.00231 | 0.94 | 70021 | 1.000 | 100% | 0.94 | yes | ok |
| ranking | satisfied | snips | 0.4563 | -0.00038 ± 0.00019 | 0.00193 | 0.95 | 70021 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | satisfied | iips | 0.4563 | -0.00024 ± 0.00022 | 0.00219 | 0.96 | 72958 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | satisfied | sniips | 0.4563 | -0.00033 ± 0.00018 | 0.00185 | 0.97 | 72958 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | satisfied | rips | 0.4563 | -0.00023 ± 0.00022 | 0.00222 | 0.97 | 70021 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | satisfied | snrips | 0.4563 | -0.00033 ± 0.00019 | 0.00187 | 0.97 | 70021 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | satisfied | dm-pbm | 0.4563 | -0.00405 ± 0.00019 | 0.00445 | 0.08 |  |  | 100% | 0.08 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.4563 | -0.01518 ± 0.00018 | 0.01528 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.4563 | -0.00023 ± 0.00018 | 0.00185 | 0.95 | 72958 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | satisfied | cdr | 0.4563 | -0.00024 ± 0.00019 | 0.00186 | 0.95 | 70021 | 1.000 | 100% | 0.95 | yes | ok |
| rerank | clicks | naive | 0.8150 | -0.04317 ± 0.00023 | 0.04322 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.8150 | -0.14598 ± 0.08972 | 0.90459 | 0.43 | 161 | 0.862 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 161 impressions, mean 0.86; 0% of logs usable) |
| rerank | clicks | snips | 0.8150 | -0.04726 ± 0.05108 | 0.51042 | 0.22 | 161 | 0.862 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 161 impressions, mean 0.86; 0% of logs usable) |
| rerank | clicks | iips | 0.8150 | -0.00048 ± 0.00099 | 0.00988 | 0.94 | 3645 | 0.999 | 99% | 0.95 | yes | ok |
| rerank | clicks | sniips | 0.8150 | -0.00047 ± 0.00086 | 0.00860 | 0.92 | 3645 | 0.999 | 99% | 0.92 | yes | ok |
| rerank | clicks | rips | 0.8150 | -0.00406 ± 0.01551 | 0.15433 | 0.73 | 3 | 0.630 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.63; 0% of logs usable) |
| rerank | clicks | snrips | 0.8150 | -0.00568 ± 0.01189 | 0.11846 | 0.72 | 3 | 0.630 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.63; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.8150 | -0.01825 ± 0.00027 | 0.01845 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.8150 | +0.03797 ± 0.00027 | 0.03806 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.8150 | -0.00052 ± 0.00086 | 0.00856 | 0.92 | 3645 | 0.999 | 99% | 0.92 | yes | ok |
| rerank | clicks | cdr | 0.8150 | -0.00564 ± 0.01571 | 0.15638 | 0.97 | 3 | 0.630 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.63; 0% of logs usable) |
| rerank | satisfied | naive | 0.4100 | -0.05255 ± 0.00016 | 0.05257 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.4100 | -0.08588 ± 0.05709 | 0.57451 | 0.27 | 161 | 0.862 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 161 impressions, mean 0.86; 0% of logs usable) |
| rerank | satisfied | snips | 0.4100 | -0.07016 ± 0.03278 | 0.33357 | 0.11 | 161 | 0.862 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 161 impressions, mean 0.86; 0% of logs usable) |
| rerank | satisfied | iips | 0.4100 | -0.00015 ± 0.00046 | 0.00463 | 0.97 | 3645 | 0.999 | 99% | 0.97 | yes | ok |
| rerank | satisfied | sniips | 0.4100 | -0.00016 ± 0.00052 | 0.00517 | 0.96 | 3645 | 0.999 | 99% | 0.96 | yes | ok |
| rerank | satisfied | rips | 0.4100 | -0.00773 ± 0.00334 | 0.03411 | 0.62 | 3 | 0.630 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.63; 0% of logs usable) |
| rerank | satisfied | snrips | 0.4100 | -0.00805 ± 0.00284 | 0.02939 | 0.61 | 3 | 0.630 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.63; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.4100 | -0.00078 ± 0.00018 | 0.00196 | 0.60 |  |  | 100% | 0.60 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.4100 | +0.02684 ± 0.00018 | 0.02690 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.4100 | -0.00018 ± 0.00042 | 0.00416 | 0.99 | 3645 | 0.999 | 99% | 0.99 | yes | ok |
| rerank | satisfied | cdr | 0.4100 | -0.00668 ± 0.00452 | 0.04550 | 0.96 | 3 | 0.630 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.63; 0% of logs usable) |
| oracle | clicks | naive | 0.9250 | -0.15321 ± 0.00023 | 0.15323 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.9250 | -0.15546 ± 0.03237 | 0.35766 | 0.08 | 48542 | 0.853 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 48542 impressions, mean 0.85; 0% of logs usable) |
| oracle | clicks | snips | 0.9250 | -0.02879 ± 0.00829 | 0.08736 | 0.07 | 48542 | 0.853 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 48542 impressions, mean 0.85; 0% of logs usable) |
| oracle | clicks | iips | 0.9250 | +0.00045 ± 0.00077 | 0.00770 | 0.98 | 11281 | 0.998 | 99% | 0.99 | yes | ok |
| oracle | clicks | sniips | 0.9250 | -0.00012 ± 0.00049 | 0.00485 | 0.95 | 11281 | 0.998 | 99% | 0.95 | yes | ok |
| oracle | clicks | rips | 0.9250 | -0.00404 ± 0.00342 | 0.03426 | 0.78 | 70 | 0.800 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 70 impressions, mean 0.80; 0% of logs usable) |
| oracle | clicks | snrips | 0.9250 | -0.00131 ± 0.00317 | 0.03153 | 0.77 | 70 | 0.800 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 70 impressions, mean 0.80; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.9250 | +0.00464 ± 0.00025 | 0.00528 | 0.09 |  |  | 100% | 0.09 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.9250 | -0.08568 ± 0.00022 | 0.08571 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.9250 | +0.00015 ± 0.00051 | 0.00513 | 0.93 | 11281 | 0.998 | 99% | 0.93 | yes | ok |
| oracle | clicks | cdr | 0.9250 | +0.00079 ± 0.00366 | 0.03643 | 0.97 | 70 | 0.800 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 70 impressions, mean 0.80; 0% of logs usable) |
| oracle | satisfied | naive | 0.5669 | -0.20951 ± 0.00016 | 0.20952 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5669 | -0.08643 ± 0.02752 | 0.28710 | 0.08 | 48542 | 0.853 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 48542 impressions, mean 0.85; 0% of logs usable) |
| oracle | satisfied | snips | 0.5669 | -0.02369 ± 0.00804 | 0.08345 | 0.04 | 48542 | 0.853 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 48542 impressions, mean 0.85; 0% of logs usable) |
| oracle | satisfied | iips | 0.5669 | +0.00001 ± 0.00065 | 0.00643 | 0.99 | 11281 | 0.998 | 99% | 0.99 | yes | ok |
| oracle | satisfied | sniips | 0.5669 | -0.00040 ± 0.00048 | 0.00480 | 0.93 | 11281 | 0.998 | 99% | 0.93 | yes | ok |
| oracle | satisfied | rips | 0.5669 | -0.00108 ± 0.00108 | 0.01081 | 0.95 | 70 | 0.800 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 70 impressions, mean 0.80; 0% of logs usable) |
| oracle | satisfied | snrips | 0.5669 | -0.00132 ± 0.00097 | 0.00978 | 0.91 | 70 | 0.800 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 70 impressions, mean 0.80; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.5669 | -0.00326 ± 0.00023 | 0.00398 | 0.22 |  |  | 100% | 0.22 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5669 | -0.10021 ± 0.00019 | 0.10022 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5669 | -0.00010 ± 0.00049 | 0.00492 | 0.92 | 11281 | 0.998 | 99% | 0.92 | yes | ok |
| oracle | satisfied | cdr | 0.5669 | +0.00007 ± 0.00098 | 0.00975 | 0.98 | 70 | 0.800 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 70 impressions, mean 0.80; 0% of logs usable) |
| reverse | clicks | naive | 0.5059 | +0.26588 ± 0.00023 | 0.26589 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.5059 | -0.03214 ± 0.07228 | 0.71994 | 0.36 | 1 | 0.897 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.90; 0% of logs usable) |
| reverse | clicks | snips | 0.5059 | -0.19424 ± 0.04708 | 0.50707 | 0.16 | 1 | 0.897 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.90; 0% of logs usable) |
| reverse | clicks | iips | 0.5059 | -0.00195 ± 0.00137 | 0.01378 | 0.93 | 2936 | 0.992 | 100% | 0.93 | yes | ok |
| reverse | clicks | sniips | 0.5059 | -0.00136 ± 0.00123 | 0.01234 | 0.93 | 2936 | 0.992 | 100% | 0.93 | yes | ok |
| reverse | clicks | rips | 0.5059 | +0.00385 ± 0.01836 | 0.18272 | 0.70 | 1 | 0.769 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.77; 0% of logs usable) |
| reverse | clicks | snrips | 0.5059 | -0.00538 ± 0.01551 | 0.15437 | 0.67 | 1 | 0.769 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.77; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.5059 | +0.01225 ± 0.00064 | 0.01378 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.5059 | +0.12050 ± 0.00049 | 0.12059 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.5059 | -0.00131 ± 0.00124 | 0.01236 | 0.93 | 2936 | 0.992 | 100% | 0.93 | yes | ok |
| reverse | clicks | cdr | 0.5059 | +0.01556 ± 0.02047 | 0.20431 | 0.87 | 1 | 0.769 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.77; 0% of logs usable) |
| reverse | satisfied | naive | 0.0238 | +0.33361 ± 0.00016 | 0.33361 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0238 | -0.02380 ± 0.00000 | 0.02380 | 0.00 | 1 | 0.897 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.90; 0% of logs usable) |
| reverse | satisfied | snips | 0.0238 | -0.02380 ± 0.00000 | 0.02380 | 0.00 | 1 | 0.897 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.90; 0% of logs usable) |
| reverse | satisfied | iips | 0.0238 | -0.00030 ± 0.00026 | 0.00263 | 0.94 | 2936 | 0.992 | 100% | 0.94 | yes | ok |
| reverse | satisfied | sniips | 0.0238 | -0.00026 ± 0.00026 | 0.00261 | 0.94 | 2936 | 0.992 | 100% | 0.94 | yes | ok |
| reverse | satisfied | rips | 0.0238 | -0.00463 ± 0.00105 | 0.01147 | 0.44 | 1 | 0.769 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.77; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0238 | -0.00460 ± 0.00105 | 0.01140 | 0.46 | 1 | 0.769 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.77; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0238 | +0.06663 ± 0.00012 | 0.06664 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0238 | +0.08698 ± 0.00012 | 0.08699 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0238 | -0.00015 ± 0.00025 | 0.00254 | 0.97 | 2936 | 0.992 | 100% | 0.97 | yes | ok |
| reverse | satisfied | cdr | 0.0238 | -0.00093 ± 0.00354 | 0.03522 | 0.84 | 1 | 0.769 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 1 impressions, mean 0.77; 0% of logs usable) |
| wide | clicks | naive | 0.8111 | -0.03932 ± 0.00023 | 0.03939 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.8111 | -0.53526 ± 0.05882 | 0.79310 | 0.21 | 258 | 0.318 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snips | 0.8111 | -0.05166 ± 0.03938 | 0.39525 | 0.13 | 258 | 0.318 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.8111 | -0.03324 ± 0.00097 | 0.03461 | 0.09 | 2370 | 0.637 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | sniips | 0.8111 | +0.01519 ± 0.00100 | 0.01814 | 0.66 | 2370 | 0.637 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | rips | 0.8111 | -0.06139 ± 0.00540 | 0.08156 | 0.41 | 4 | 0.221 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snrips | 0.8111 | -0.00275 ± 0.00806 | 0.08026 | 0.81 | 4 | 0.221 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.8111 | -0.00759 ± 0.00027 | 0.00805 | 0.01 |  |  | 100% | 0.01 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.8111 | +0.07954 ± 0.00023 | 0.07958 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.8111 | +0.01537 ± 0.00081 | 0.01737 | 0.49 | 2370 | 0.637 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | cdr | 0.8111 | +0.08222 ± 0.00677 | 0.10629 | 0.48 | 4 | 0.221 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | naive | 0.4061 | -0.04867 ± 0.00016 | 0.04870 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.4061 | -0.27353 ± 0.03407 | 0.43559 | 0.13 | 258 | 0.318 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | snips | 0.4061 | -0.08013 ± 0.02623 | 0.27300 | 0.02 | 258 | 0.318 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.4061 | -0.00195 ± 0.00048 | 0.00514 | 0.95 | 2370 | 0.637 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | sniips | 0.4061 | +0.01562 ± 0.00062 | 0.01679 | 0.29 | 2370 | 0.637 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | rips | 0.4061 | -0.00923 ± 0.00317 | 0.03288 | 0.57 | 4 | 0.221 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.4061 | +0.01083 ± 0.00474 | 0.04834 | 0.78 | 4 | 0.221 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | dm-pbm | 0.4061 | +0.01750 ± 0.00019 | 0.01760 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.4061 | +0.06402 ± 0.00019 | 0.06404 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.4061 | +0.02044 ± 0.00044 | 0.02091 | 0.00 | 2370 | 0.637 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | cdr | 0.4061 | +0.04953 ± 0.00341 | 0.06004 | 0.38 | 4 | 0.221 | 0% |  |  | biased, as expected: 20% of its slots unsupported |

## scifact · trust · {"policy":"shuffle","top":10}

100 logs of 100000 impressions, 13 s. Propensities: 10000000 lists, 0 mismatched; chi-square 13543 on 13500 df (p 0.396), largest |z| 4.54.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.8470 | 0.4563 | 0.4506 | 2.282 |
| rerank | 0.0% | 0.8150 | 0.4100 | 0.4043 | 2.305 |
| oracle | 0.0% | 0.9250 | 0.5669 | 0.5569 | 2.232 |
| reverse | 0.0% | 0.5059 | 0.0238 | 0.0235 | 2.522 |
| wide | 20.3% | 0.8111 | 0.4061 | 0.4006 | 2.307 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.8470 | -0.25008 ± 0.00020 | 0.25009 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.8470 | +0.07839 ± 0.05683 | 0.57086 | 0.91 | 3 | 1.025 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.03; 0% of logs usable) |
| ranking | clicks | snips | 0.8470 | +0.03033 ± 0.04258 | 0.42474 | 0.55 | 3 | 1.025 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.03; 0% of logs usable) |
| ranking | clicks | iips | 0.8470 | -0.00115 ± 0.00092 | 0.00918 | 0.93 | 9881 | 0.998 | 96% | 0.95 | yes | ok |
| ranking | clicks | sniips | 0.8470 | -0.00044 ± 0.00055 | 0.00552 | 0.98 | 9881 | 0.998 | 96% | 0.98 | yes | ok |
| ranking | clicks | rips | 0.8470 | +0.00128 ± 0.00752 | 0.07479 | 0.70 | 3 | 0.971 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.97; 0% of logs usable) |
| ranking | clicks | snrips | 0.8470 | +0.00525 ± 0.01163 | 0.11580 | 0.69 | 3 | 0.971 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.97; 0% of logs usable) |
| ranking | clicks | dm-pbm | 0.8470 | -0.00305 ± 0.00034 | 0.00456 | 0.21 |  |  | 100% | 0.21 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.8470 | -0.13868 ± 0.00025 | 0.13870 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.8470 | -0.00068 ± 0.00055 | 0.00547 | 0.95 | 9881 | 0.998 | 96% | 0.95 | yes | ok |
| ranking | clicks | cdr | 0.8470 | +0.00039 ± 0.00934 | 0.09291 | 0.93 | 3 | 0.971 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.97; 0% of logs usable) |
| ranking | satisfied | naive | 0.4563 | -0.32932 ± 0.00010 | 0.32932 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.4563 | +0.05473 ± 0.03614 | 0.36372 | 0.84 | 3 | 1.025 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.03; 0% of logs usable) |
| ranking | satisfied | snips | 0.4563 | +0.02732 ± 0.02951 | 0.29486 | 0.73 | 3 | 1.025 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.03; 0% of logs usable) |
| ranking | satisfied | iips | 0.4563 | +0.00017 ± 0.00066 | 0.00658 | 0.94 | 9881 | 0.998 | 96% | 0.95 | yes | ok |
| ranking | satisfied | sniips | 0.4563 | +0.00056 ± 0.00048 | 0.00482 | 0.97 | 9881 | 0.998 | 96% | 0.97 | yes | ok |
| ranking | satisfied | rips | 0.4563 | -0.00204 ± 0.00331 | 0.03297 | 0.80 | 3 | 0.971 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.97; 0% of logs usable) |
| ranking | satisfied | snrips | 0.4563 | +0.00507 ± 0.01007 | 0.10037 | 0.82 | 3 | 0.971 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.97; 0% of logs usable) |
| ranking | satisfied | dm-pbm | 0.4563 | -0.01885 ± 0.00029 | 0.01906 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.4563 | -0.17876 ± 0.00022 | 0.17877 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.4563 | +0.00033 ± 0.00040 | 0.00402 | 0.97 | 9881 | 0.998 | 96% | 0.97 | yes | ok |
| ranking | satisfied | cdr | 0.4563 | -0.00015 ± 0.00308 | 0.03069 | 0.99 | 3 | 0.971 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.97; 0% of logs usable) |
| rerank | clicks | naive | 0.8150 | -0.21808 ± 0.00020 | 0.21809 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.8150 | +0.03177 ± 0.06184 | 0.61611 | 0.91 | 4 | 1.058 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.06; 0% of logs usable) |
| rerank | clicks | snips | 0.8150 | -0.02287 ± 0.04433 | 0.44167 | 0.62 | 4 | 1.058 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.06; 0% of logs usable) |
| rerank | clicks | iips | 0.8150 | -0.00054 ± 0.00083 | 0.00823 | 0.95 | 9896 | 1.001 | 100% | 0.95 | yes | ok |
| rerank | clicks | sniips | 0.8150 | +0.00005 ± 0.00059 | 0.00591 | 0.98 | 9896 | 1.001 | 100% | 0.98 | yes | ok |
| rerank | clicks | rips | 0.8150 | +0.00232 ± 0.00844 | 0.08406 | 0.80 | 4 | 0.986 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.99; 0% of logs usable) |
| rerank | clicks | snrips | 0.8150 | +0.00665 ± 0.01258 | 0.12534 | 0.76 | 4 | 0.986 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.99; 0% of logs usable) |
| rerank | clicks | dm-pbm | 0.8150 | -0.01425 ± 0.00031 | 0.01459 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.8150 | -0.10617 ± 0.00024 | 0.10620 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.8150 | -0.00007 ± 0.00053 | 0.00525 | 0.98 | 9896 | 1.001 | 100% | 0.98 | yes | ok |
| rerank | clicks | cdr | 0.8150 | -0.00350 ± 0.00982 | 0.09781 | 0.96 | 4 | 0.986 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.99; 0% of logs usable) |
| rerank | satisfied | naive | 0.4100 | -0.28294 ± 0.00010 | 0.28295 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.4100 | +0.01946 ± 0.03849 | 0.38343 | 0.71 | 4 | 1.058 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.06; 0% of logs usable) |
| rerank | satisfied | snips | 0.4100 | -0.00055 ± 0.03500 | 0.34826 | 0.54 | 4 | 1.058 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.06; 0% of logs usable) |
| rerank | satisfied | iips | 0.4100 | -0.00000 ± 0.00061 | 0.00607 | 0.95 | 9896 | 1.001 | 100% | 0.95 | yes | ok |
| rerank | satisfied | sniips | 0.4100 | +0.00033 ± 0.00052 | 0.00521 | 0.97 | 9896 | 1.001 | 100% | 0.97 | yes | ok |
| rerank | satisfied | rips | 0.4100 | +0.00353 ± 0.00462 | 0.04612 | 0.81 | 4 | 0.986 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.99; 0% of logs usable) |
| rerank | satisfied | snrips | 0.4100 | +0.00396 ± 0.00538 | 0.05369 | 0.85 | 4 | 0.986 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.99; 0% of logs usable) |
| rerank | satisfied | dm-pbm | 0.4100 | -0.02552 ± 0.00027 | 0.02566 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.4100 | -0.14046 ± 0.00021 | 0.14047 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.4100 | +0.00021 ± 0.00045 | 0.00445 | 0.98 | 9896 | 1.001 | 100% | 0.98 | yes | ok |
| rerank | satisfied | cdr | 0.4100 | +0.00242 ± 0.00454 | 0.04528 | 0.91 | 4 | 0.986 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.99; 0% of logs usable) |
| oracle | clicks | naive | 0.9250 | -0.32813 ± 0.00020 | 0.32813 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.9250 | +0.04873 ± 0.05531 | 0.55245 | 0.83 | 3 | 0.998 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| oracle | clicks | snips | 0.9250 | +0.02184 ± 0.04344 | 0.43274 | 0.55 | 3 | 0.998 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| oracle | clicks | iips | 0.9250 | -0.00109 ± 0.00090 | 0.00905 | 0.94 | 9880 | 0.999 | 97% | 0.95 | yes | ok |
| oracle | clicks | sniips | 0.9250 | -0.00051 ± 0.00047 | 0.00472 | 0.97 | 9880 | 0.999 | 97% | 0.97 | yes | ok |
| oracle | clicks | rips | 0.9250 | +0.00261 ± 0.00706 | 0.07028 | 0.78 | 3 | 0.961 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.96; 0% of logs usable) |
| oracle | clicks | snrips | 0.9250 | +0.00705 ± 0.01125 | 0.11214 | 0.78 | 3 | 0.961 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.96; 0% of logs usable) |
| oracle | clicks | dm-pbm | 0.9250 | +0.01303 ± 0.00032 | 0.01340 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.9250 | -0.21110 ± 0.00024 | 0.21112 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.9250 | -0.00047 ± 0.00044 | 0.00439 | 0.99 | 9880 | 0.999 | 97% | 0.99 | yes | ok |
| oracle | clicks | cdr | 0.9250 | +0.00024 ± 0.00859 | 0.08545 | 0.96 | 3 | 0.961 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.96; 0% of logs usable) |
| oracle | satisfied | naive | 0.5669 | -0.43991 ± 0.00010 | 0.43991 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5669 | +0.03486 ± 0.03759 | 0.37566 | 0.88 | 3 | 0.998 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| oracle | satisfied | snips | 0.5669 | +0.02610 ± 0.03251 | 0.32448 | 0.63 | 3 | 0.998 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 1.00; 0% of logs usable) |
| oracle | satisfied | iips | 0.5669 | -0.00011 ± 0.00064 | 0.00641 | 0.97 | 9880 | 0.999 | 97% | 0.97 | yes | ok |
| oracle | satisfied | sniips | 0.5669 | +0.00025 ± 0.00045 | 0.00452 | 0.96 | 9880 | 0.999 | 97% | 0.96 | yes | ok |
| oracle | satisfied | rips | 0.5669 | -0.00175 ± 0.00095 | 0.00965 | 0.94 | 3 | 0.961 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.96; 0% of logs usable) |
| oracle | satisfied | snrips | 0.5669 | -0.00140 ± 0.00088 | 0.00890 | 0.88 | 3 | 0.961 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.96; 0% of logs usable) |
| oracle | satisfied | dm-pbm | 0.5669 | -0.01644 ± 0.00030 | 0.01671 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5669 | -0.26421 ± 0.00022 | 0.26422 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5669 | +0.00027 ± 0.00040 | 0.00402 | 0.96 | 9880 | 0.999 | 97% | 0.96 | yes | ok |
| oracle | satisfied | cdr | 0.5669 | -0.00153 ± 0.00091 | 0.00919 | 0.94 | 3 | 0.961 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 3 impressions, mean 0.96; 0% of logs usable) |
| reverse | clicks | naive | 0.5059 | +0.09097 ± 0.00020 | 0.09099 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.5059 | +0.06866 ± 0.04659 | 0.46863 | 0.81 | 4 | 1.122 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.12; 0% of logs usable) |
| reverse | clicks | snips | 0.5059 | -0.00709 ± 0.03808 | 0.37894 | 0.67 | 4 | 1.122 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.12; 0% of logs usable) |
| reverse | clicks | iips | 0.5059 | -0.00006 ± 0.00070 | 0.00701 | 0.94 | 9885 | 0.998 | 98% | 0.94 | yes | ok |
| reverse | clicks | sniips | 0.5059 | -0.00005 ± 0.00059 | 0.00589 | 0.96 | 9885 | 0.998 | 98% | 0.96 | yes | ok |
| reverse | clicks | rips | 0.5059 | +0.00724 ± 0.00961 | 0.09593 | 0.74 | 4 | 0.970 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.97; 0% of logs usable) |
| reverse | clicks | snrips | 0.5059 | +0.00682 ± 0.01227 | 0.12227 | 0.68 | 4 | 0.970 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.97; 0% of logs usable) |
| reverse | clicks | dm-pbm | 0.5059 | +0.00721 ± 0.00041 | 0.00830 | 0.01 |  |  | 100% | 0.01 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.5059 | +0.08930 ± 0.00030 | 0.08935 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.5059 | -0.00007 ± 0.00059 | 0.00586 | 0.96 | 9885 | 0.998 | 98% | 0.96 | yes | ok |
| reverse | clicks | cdr | 0.5059 | -0.00394 ± 0.01146 | 0.11410 | 0.94 | 4 | 0.970 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.97; 0% of logs usable) |
| reverse | satisfied | naive | 0.0238 | +0.10321 ± 0.00010 | 0.10321 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0238 | +0.00947 ± 0.01208 | 0.12054 | 0.09 | 4 | 1.122 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.12; 0% of logs usable) |
| reverse | satisfied | snips | 0.0238 | +0.01187 ± 0.01516 | 0.15132 | 0.06 | 4 | 1.122 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 1.12; 0% of logs usable) |
| reverse | satisfied | iips | 0.0238 | -0.00017 ± 0.00016 | 0.00157 | 0.94 | 9885 | 0.998 | 98% | 0.94 | yes | ok |
| reverse | satisfied | sniips | 0.0238 | -0.00017 ± 0.00016 | 0.00156 | 0.95 | 9885 | 0.998 | 98% | 0.95 | yes | ok |
| reverse | satisfied | rips | 0.0238 | -0.00058 ± 0.00320 | 0.03180 | 0.46 | 4 | 0.970 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.97; 0% of logs usable) |
| reverse | satisfied | snrips | 0.0238 | -0.00172 ± 0.00222 | 0.02212 | 0.46 | 4 | 0.970 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.97; 0% of logs usable) |
| reverse | satisfied | dm-pbm | 0.0238 | +0.01150 ± 0.00008 | 0.01153 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0238 | +0.01905 ± 0.00007 | 0.01906 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0238 | -0.00018 ± 0.00014 | 0.00143 | 0.95 | 9885 | 0.998 | 98% | 0.95 | yes | ok |
| reverse | satisfied | cdr | 0.0238 | +0.00003 ± 0.00363 | 0.03616 | 0.98 | 4 | 0.970 | 0% |  | yes | not judged: the logs rarely show its lists (weights on 4 impressions, mean 0.97; 0% of logs usable) |
| wide | clicks | naive | 0.8111 | -0.21424 ± 0.00020 | 0.21425 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.8111 | -0.48149 ± 0.03749 | 0.60910 | 0.57 | 1 | 0.387 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snips | 0.8111 | -0.19411 ± 0.06229 | 0.64948 | 0.25 | 1 | 0.387 | 0% |  |  | unbiased though unsupported |
| wide | clicks | iips | 0.8111 | -0.03375 ± 0.00083 | 0.03474 | 0.03 | 6389 | 0.639 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | sniips | 0.8111 | +0.01510 ± 0.00064 | 0.01640 | 0.45 | 6389 | 0.639 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | rips | 0.8111 | -0.04620 ± 0.00595 | 0.07511 | 0.42 | 1 | 0.471 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | snrips | 0.8111 | +0.01233 ± 0.00865 | 0.08690 | 0.85 | 1 | 0.471 | 0% |  |  | unbiased though unsupported |
| wide | clicks | dm-pbm | 0.8111 | -0.00688 ± 0.00031 | 0.00756 | 0.04 |  |  | 100% | 0.04 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.8111 | -0.09272 ± 0.00025 | 0.09275 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.8111 | +0.01283 ± 0.00052 | 0.01383 | 0.36 | 6389 | 0.639 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | clicks | cdr | 0.8111 | +0.09715 ± 0.00612 | 0.11467 | 0.38 | 1 | 0.471 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | naive | 0.4061 | -0.27907 ± 0.00010 | 0.27907 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.4061 | -0.24580 ± 0.02486 | 0.34872 | 0.35 | 1 | 0.387 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | snips | 0.4061 | -0.12524 ± 0.04127 | 0.42931 | 0.15 | 1 | 0.387 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | iips | 0.4061 | -0.00225 ± 0.00061 | 0.00646 | 0.92 | 6389 | 0.639 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | sniips | 0.4061 | +0.01559 ± 0.00055 | 0.01651 | 0.20 | 6389 | 0.639 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | rips | 0.4061 | -0.00255 ± 0.00362 | 0.03614 | 0.77 | 1 | 0.471 | 0% |  |  | unbiased though unsupported |
| wide | satisfied | snrips | 0.4061 | +0.01927 ± 0.00447 | 0.04843 | 0.93 | 1 | 0.471 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.4061 | -0.01665 ± 0.00027 | 0.01687 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.4061 | -0.12586 ± 0.00021 | 0.12588 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.4061 | +0.00778 ± 0.00045 | 0.00896 | 0.56 | 6389 | 0.639 | 0% |  |  | biased, as expected: 20% of its slots unsupported |
| wide | satisfied | cdr | 0.4061 | +0.02298 ± 0.00361 | 0.04266 | 0.69 | 1 | 0.471 | 0% |  |  | biased, as expected: 20% of its slots unsupported |

## scifact · trust · {"policy":"swap","top":10}

100 logs of 100000 impressions, 12 s. Propensities: 10000000 lists, 0 mismatched; chi-square 3990 on 3900 df (p 0.154), largest |z| 4.23.

| target | unsupported | clicks | satisfied | success | session |
|---|---|---|---|---|---|
| ranking | 0.0% | 0.8470 | 0.4563 | 0.4506 | 2.282 |
| rerank | 54.3% | 0.8150 | 0.4100 | 0.4043 | 2.305 |
| oracle | 5.8% | 0.9250 | 0.5669 | 0.5569 | 2.232 |
| reverse | 80.0% | 0.5059 | 0.0238 | 0.0235 | 2.522 |
| wide | 61.2% | 0.8111 | 0.4061 | 0.4006 | 2.307 |

| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ranking | clicks | naive | 0.8470 | -0.22664 ± 0.00021 | 0.22665 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | clicks | ips | 0.8470 | -0.00057 ± 0.00096 | 0.00954 | 0.98 | 9996 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | clicks | snips | 0.8470 | -0.00020 ± 0.00061 | 0.00606 | 0.95 | 9996 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | clicks | iips | 0.8470 | -0.00032 ± 0.00070 | 0.00693 | 0.97 | 9996 | 1.000 | 99% | 0.97 | yes | ok |
| ranking | clicks | sniips | 0.8470 | -0.00003 ± 0.00044 | 0.00433 | 0.97 | 9996 | 1.000 | 99% | 0.97 | yes | ok |
| ranking | clicks | rips | 0.8470 | -0.00057 ± 0.00096 | 0.00954 | 0.98 | 9996 | 1.000 | 100% | 0.98 | yes | ok |
| ranking | clicks | snrips | 0.8470 | -0.00020 ± 0.00061 | 0.00606 | 0.95 | 9996 | 1.000 | 100% | 0.95 | yes | ok |
| ranking | clicks | dm-pbm | 0.8470 | -0.00617 ± 0.00031 | 0.00691 | 0.07 |  |  | 100% | 0.07 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dm-sdbn | 0.8470 | -0.20440 ± 0.00023 | 0.20441 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | clicks | dr-iips | 0.8470 | -0.00029 ± 0.00039 | 0.00391 | 0.96 | 9996 | 1.000 | 99% | 0.96 | yes | ok |
| ranking | clicks | cdr | 0.8470 | -0.00028 ± 0.00061 | 0.00604 | 0.96 | 9996 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | satisfied | naive | 0.4563 | -0.30465 ± 0.00012 | 0.30465 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| ranking | satisfied | ips | 0.4563 | -0.00061 ± 0.00068 | 0.00682 | 0.96 | 9996 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | satisfied | snips | 0.4563 | -0.00042 ± 0.00054 | 0.00534 | 0.97 | 9996 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | satisfied | iips | 0.4563 | -0.00035 ± 0.00064 | 0.00635 | 0.95 | 9996 | 1.000 | 99% | 0.95 | yes | ok |
| ranking | satisfied | sniips | 0.4563 | -0.00018 ± 0.00051 | 0.00507 | 0.96 | 9996 | 1.000 | 99% | 0.96 | yes | ok |
| ranking | satisfied | rips | 0.4563 | -0.00061 ± 0.00068 | 0.00682 | 0.96 | 9996 | 1.000 | 100% | 0.96 | yes | ok |
| ranking | satisfied | snrips | 0.4563 | -0.00042 ± 0.00054 | 0.00534 | 0.97 | 9996 | 1.000 | 100% | 0.97 | yes | ok |
| ranking | satisfied | dm-pbm | 0.4563 | -0.01487 ± 0.00032 | 0.01521 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dm-sdbn | 0.4563 | -0.17874 ± 0.00023 | 0.17876 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| ranking | satisfied | dr-iips | 0.4563 | -0.00039 ± 0.00046 | 0.00460 | 0.91 | 9996 | 1.000 | 99% | 0.91 | yes | ok |
| ranking | satisfied | cdr | 0.4563 | -0.00043 ± 0.00052 | 0.00520 | 0.92 | 9996 | 1.000 | 100% | 0.92 | yes | ok |
| rerank | clicks | naive | 0.8150 | -0.19464 ± 0.00021 | 0.19465 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | clicks | ips | 0.8150 | -0.80222 ± 0.00012 | 0.80222 | 0.00 | 198 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | snips | 0.8150 | -0.17152 ± 0.00507 | 0.17877 | 0.08 | 198 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | iips | 0.8150 | -0.11706 ± 0.00074 | 0.11730 | 0.00 | 8610 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | sniips | 0.8150 | +0.02373 ± 0.00066 | 0.02462 | 0.05 | 8610 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | rips | 0.8150 | -0.15769 ± 0.00081 | 0.15789 | 0.00 | 198 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | snrips | 0.8150 | -0.02336 ± 0.00139 | 0.02714 | 0.56 | 198 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | dm-pbm | 0.8150 | -0.00635 ± 0.00034 | 0.00719 | 0.05 |  |  | 100% | 0.05 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dm-sdbn | 0.8150 | -0.01756 ± 0.00030 | 0.01781 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | clicks | dr-iips | 0.8150 | +0.02370 ± 0.00048 | 0.02418 | 0.00 | 8610 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | clicks | cdr | 0.8150 | +0.26706 ± 0.00059 | 0.26713 | 0.00 | 198 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | naive | 0.4100 | -0.25827 ± 0.00012 | 0.25828 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| rerank | satisfied | ips | 0.4100 | -0.40647 ± 0.00006 | 0.40647 | 0.00 | 198 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | snips | 0.4100 | -0.23408 ± 0.00258 | 0.23548 | 0.00 | 198 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | iips | 0.4100 | -0.02946 ± 0.00060 | 0.03007 | 0.00 | 8610 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | sniips | 0.4100 | +0.02501 ± 0.00063 | 0.02579 | 0.03 | 8610 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | rips | 0.4100 | -0.05422 ± 0.00061 | 0.05455 | 0.00 | 198 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | snrips | 0.4100 | -0.02289 ± 0.00063 | 0.02373 | 0.09 | 198 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | dm-pbm | 0.4100 | -0.01859 ± 0.00030 | 0.01883 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dm-sdbn | 0.4100 | -0.12872 ± 0.00024 | 0.12874 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| rerank | satisfied | dr-iips | 0.4100 | -0.00527 ± 0.00043 | 0.00680 | 0.72 | 8610 | 0.203 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| rerank | satisfied | cdr | 0.4100 | +0.03329 ± 0.00048 | 0.03362 | 0.00 | 198 | 0.020 | 0% |  |  | biased, as expected: 54% of its slots unsupported |
| oracle | clicks | naive | 0.9250 | -0.30469 ± 0.00021 | 0.30470 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | clicks | ips | 0.9250 | -0.13607 ± 0.00088 | 0.13635 | 0.00 | 8711 | 0.871 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | snips | 0.9250 | -0.01931 ± 0.00068 | 0.02045 | 0.16 | 8711 | 0.871 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | iips | 0.9250 | -0.01604 ± 0.00081 | 0.01797 | 0.54 | 10007 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | sniips | 0.9250 | -0.00771 ± 0.00046 | 0.00899 | 0.57 | 10007 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | rips | 0.9250 | -0.02650 ± 0.00098 | 0.02823 | 0.25 | 8711 | 0.871 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | snrips | 0.9250 | -0.00725 ± 0.00065 | 0.00973 | 0.74 | 8711 | 0.871 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | clicks | dm-pbm | 0.9250 | +0.01012 ± 0.00029 | 0.01052 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dm-sdbn | 0.9250 | -0.26630 ± 0.00023 | 0.26631 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | clicks | dr-iips | 0.9250 | -0.00094 ± 0.00042 | 0.00429 | 0.96 | 10007 | 0.882 | 0% |  |  | unbiased though unsupported |
| oracle | clicks | cdr | 0.9250 | +0.01123 ± 0.00062 | 0.01280 | 0.61 | 8711 | 0.871 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | naive | 0.5669 | -0.41524 ± 0.00012 | 0.41524 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| oracle | satisfied | ips | 0.5669 | -0.09348 ± 0.00065 | 0.09370 | 0.00 | 8711 | 0.871 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | snips | 0.5669 | -0.02341 ± 0.00060 | 0.02415 | 0.00 | 8711 | 0.871 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | iips | 0.5669 | -0.00863 ± 0.00070 | 0.01111 | 0.79 | 10007 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | sniips | 0.5669 | -0.00858 ± 0.00052 | 0.01003 | 0.59 | 10007 | 0.882 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | rips | 0.5669 | -0.00885 ± 0.00071 | 0.01131 | 0.78 | 8711 | 0.871 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | snrips | 0.5669 | -0.00807 ± 0.00053 | 0.00964 | 0.64 | 8711 | 0.871 | 0% |  |  | biased, as expected: 6% of its slots unsupported |
| oracle | satisfied | dm-pbm | 0.5669 | -0.01555 ± 0.00033 | 0.01589 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dm-sdbn | 0.5669 | -0.25705 ± 0.00025 | 0.25706 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| oracle | satisfied | dr-iips | 0.5669 | -0.00305 ± 0.00048 | 0.00568 | 0.88 | 10007 | 0.882 | 0% |  |  | unbiased though unsupported |
| oracle | satisfied | cdr | 0.5669 | -0.00049 ± 0.00054 | 0.00538 | 0.95 | 8711 | 0.871 | 0% |  |  | unbiased though unsupported |
| reverse | clicks | naive | 0.5059 | +0.11441 ± 0.00021 | 0.11443 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | clicks | ips | 0.5059 | -0.50590 ± 0.00000 | 0.50590 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snips | 0.5059 | -0.50590 ± 0.00000 | 0.50590 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | iips | 0.5059 | -0.15844 ± 0.00054 | 0.15853 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | sniips | 0.5059 | -0.15894 ± 0.00046 | 0.15901 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | rips | 0.5059 | -0.15844 ± 0.00054 | 0.15853 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | snrips | 0.5059 | -0.15894 ± 0.00046 | 0.15901 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | dm-pbm | 0.5059 | +0.07084 ± 0.00047 | 0.07099 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dm-sdbn | 0.5059 | +0.46462 ± 0.00034 | 0.46464 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | clicks | dr-iips | 0.5059 | +0.07346 ± 0.00054 | 0.07366 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | clicks | cdr | 0.5059 | +0.48167 ± 0.00037 | 0.48168 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | naive | 0.0238 | +0.12788 ± 0.00012 | 0.12788 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| reverse | satisfied | ips | 0.0238 | -0.02380 ± 0.00000 | 0.02380 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snips | 0.0238 | -0.02380 ± 0.00000 | 0.02380 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | iips | 0.0238 | -0.01190 ± 0.00010 | 0.01194 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | sniips | 0.0238 | -0.01192 ± 0.00010 | 0.01196 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | rips | 0.0238 | -0.01190 ± 0.00010 | 0.01194 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | snrips | 0.0238 | -0.01192 ± 0.00010 | 0.01196 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | dm-pbm | 0.0238 | +0.01911 ± 0.00008 | 0.01913 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dm-sdbn | 0.0238 | +0.04672 ± 0.00011 | 0.04673 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| reverse | satisfied | dr-iips | 0.0238 | +0.00516 ± 0.00010 | 0.00525 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| reverse | satisfied | cdr | 0.0238 | +0.03481 ± 0.00013 | 0.03483 | 0.00 | 0 | 0.000 | 0% |  |  | biased, as expected: 80% of its slots unsupported |
| wide | clicks | naive | 0.8111 | -0.19080 ± 0.00021 | 0.19081 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | clicks | ips | 0.8111 | -0.80003 ± 0.00011 | 0.80003 | 0.00 | 164 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | snips | 0.8111 | -0.13461 ± 0.00549 | 0.14529 | 0.27 | 164 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | iips | 0.8111 | -0.12841 ± 0.00074 | 0.12862 | 0.00 | 4227 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | sniips | 0.8111 | +0.04597 ± 0.00075 | 0.04657 | 0.00 | 4227 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | rips | 0.8111 | -0.16475 ± 0.00080 | 0.16495 | 0.00 | 164 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | snrips | 0.8111 | -0.01140 ± 0.00157 | 0.01932 | 0.80 | 164 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | dm-pbm | 0.8111 | +0.00030 ± 0.00034 | 0.00338 | 0.41 |  |  | 100% | 0.41 |  | intervals too narrow, as expected of a model |
| wide | clicks | dm-sdbn | 0.8111 | -0.02831 ± 0.00031 | 0.02847 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | clicks | dr-iips | 0.8111 | +0.03232 ± 0.00048 | 0.03268 | 0.00 | 4227 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | clicks | cdr | 0.8111 | +0.25746 ± 0.00059 | 0.25753 | 0.00 | 164 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | naive | 0.4061 | -0.25440 ± 0.00012 | 0.25440 | 0.00 |  |  | 100% | 0.00 |  | biased, as expected |
| wide | satisfied | ips | 0.4061 | -0.40263 ± 0.00006 | 0.40263 | 0.00 | 164 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | snips | 0.4061 | -0.19579 ± 0.00308 | 0.19818 | 0.00 | 164 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | iips | 0.4061 | -0.02755 ± 0.00060 | 0.02820 | 0.00 | 4227 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | sniips | 0.4061 | +0.04707 ± 0.00069 | 0.04758 | 0.00 | 4227 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | rips | 0.4061 | -0.05119 ± 0.00060 | 0.05154 | 0.00 | 164 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | snrips | 0.4061 | -0.00967 ± 0.00067 | 0.01174 | 0.70 | 164 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | dm-pbm | 0.4061 | -0.00820 ± 0.00030 | 0.00874 | 0.01 |  |  | 100% | 0.01 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dm-sdbn | 0.4061 | -0.11363 ± 0.00024 | 0.11366 | 0.00 |  |  | 100% | 0.00 |  | intervals too narrow, as expected of a model |
| wide | satisfied | dr-iips | 0.4061 | +0.00511 ± 0.00044 | 0.00670 | 0.69 | 4227 | 0.095 | 0% |  |  | biased, as expected: 61% of its slots unsupported |
| wide | satisfied | cdr | 0.4061 | +0.04836 ± 0.00047 | 0.04859 | 0.00 | 164 | 0.016 | 0% |  |  | biased, as expected: 61% of its slots unsupported |

## Position bias from a swap policy: nfcorpus

10000000 impressions.

| position | truth | measured | se |
|---|---|---|---|
| 0 | 1.0000 | 1.0000 | 0.0027 |
| 1 | 0.4223 | 0.4225 | 0.0015 |
| 2 | 0.2597 | 0.2593 | 0.0012 |
| 3 | 0.1853 | 0.1844 | 0.0010 |
| 4 | 0.1432 | 0.1430 | 0.0008 |

## Position bias from a swap policy: scifact

10000000 impressions.

| position | truth | measured | se |
|---|---|---|---|
| 0 | 1.0000 | 1.0000 | 0.0015 |
| 1 | 0.4222 | 0.4207 | 0.0009 |
| 2 | 0.2596 | 0.2590 | 0.0007 |
| 3 | 0.1852 | 0.1856 | 0.0006 |
| 4 | 0.1431 | 0.1429 | 0.0005 |

