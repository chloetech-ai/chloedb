# Every public GitHub event: how fast the cluster answers

11,227,448,089 events in `gh_events`, on 4 x DigitalOcean s-4vcpu-8gb-amd (4 vCPU, 8 GB, local SSD), ChloeDB 0.3.5, answer cache off; measured 2026-11-16 21:10:28Z to 2026-11-16 21:29:35Z by `bench/events_bench.py`.
Each statement ran once, then 5 more times (one more when the first took over 30 s); times are the server's own (`took_ms`), with the answer cache off.

| kind | statement | first run | median | slowest | rows |
|---|---|---:|---:|---:|---:|
| every event | count every event | 15 ms | 13 ms | 17 ms | 1 |
| every event | first and last event | 54.49 s | 57.91 s | 57.91 s | 1 |
| every event | events by kind | 12.22 s | 6.15 s | 6.29 s | 39 |
| every event | stars ever | 379 ms | 346 ms | 402 ms | 1 |
| every event | commits pushed ever | 35.82 s | 23.05 s | 23.05 s | 1 |
| every event | events by year (a year computed from each row's INT time) | refused: grouped by an expression, so read a row at a time, and a shard reads at most 1,000,001 rows that way | | | |
| every event | events by year (one range count a year, 16 statements) | 3.70 s | 1.47 s | 1.53 s | 16 |
| every event | kinds of events in 2024 (a range of times) | 1.54 s | 935 ms | 966 ms | 23 |
| monthly sums | pushes, commits and stars of every month (gh_repo_months) | 1.14 s | 929 ms | 964 ms | 189 |
| a window | busiest repos of March 2020 | failed: the coordinator ran out of memory (1.5 GB): 7.2 million repos had events that month, and it gathers every group from every shard though rows are split by repo;  | | | |
| a window | events of each hour of 18 June 2024 | refused: grouped by an expression, so read a row at a time, and a shard reads at most 1,000,001 rows that way | | | |
| a window | events of each hour of 18 June 2024 (24 range counts) | 3.64 s | 1.18 s | 1.31 s | 24 |
| a window | most starred repos of 18 June 2024 | 917 ms | 260 ms | 305 ms | 20 |
| one repo | torvalds/linux: work ever (count, first, last) | 330 ms | 27 ms | 57 ms | 1 |
| one repo | torvalds/linux: who did the work ever | 166 ms | 46 ms | 47 ms | 30 |
| one repo | torvalds/linux: stars month by month | 305 ms | 151 ms | 190 ms | 181 |
| one repo | kubernetes/kubernetes: work ever (count, first, last) | 1.19 s | 112 ms | 201 ms | 1 |
| one repo | kubernetes/kubernetes: who did the work ever | 1.15 s | 424 ms | 579 ms | 30 |
| one repo | kubernetes/kubernetes: stars month by month | 567 ms | 98 ms | 183 ms | 150 |
| one repo | facebook/react: work ever (count, first, last) | 322 ms | 25 ms | 33 ms | 1 |
| one repo | facebook/react: who did the work ever | 365 ms | 168 ms | 210 ms | 30 |
| one repo | facebook/react: stars month by month | 2.72 s | 148 ms | 186 ms | 163 |
| one repo | microsoft/vscode: work ever (count, first, last) | 700 ms | 70 ms | 148 ms | 1 |
| one repo | microsoft/vscode: who did the work ever | 2.17 s | 1.28 s | 1.80 s | 30 |
| one repo | microsoft/vscode: stars month by month | 310 ms | 120 ms | 187 ms | 133 |
| one person | torvalds: what they did | 459 ms | 35 ms | 39 ms | 22 |
| one person | torvalds: year by year | 623 ms | 49 ms | 57 ms | 16 |
| one person | torvalds: where they work | 268 ms | 43 ms | 46 ms | 24 |
| one person | sindresorhus: what they did | 20.24 s | 70 ms | 14.90 s | 28 |
| one person | sindresorhus: year by year | 11.94 s | 178 ms | 10.77 s | 16 |
| one person | sindresorhus: where they work | 1.55 s | 73 ms | 118 ms | 24 |
| one person | dependabot[bot]: what they did | 20.47 s | 18.55 s | 20.08 s | 17 |
| one person | dependabot[bot]: year by year | refused: grouped by an expression, so read a row at a time, and a shard reads at most 1,000,001 rows that way | | | |

The app's pages, end to end (all their statements, several at once):

| page | statements | first run | median | slowest | what it waits on (last run) |
|---|---:|---:|---:|---:|---|
| a repo: torvalds/linux | 12 | 6.26 s | 5.13 s | 5.16 s | what the same people starred: 5.01 s (refused: shard 2 (http://gh-s2.strata.svc:7701): no answer i) |
| a repo: facebook/react | 13 | 6.23 s | 5.42 s | 5.45 s | what the same people starred: 5.01 s (refused: shard 0 (http://gh-s0.strata.svc:7701): no answer i) |
| a person: torvalds | 7 | 646 ms | 78 ms | 119 ms | year by year: 73 ms |
| search: terminal file manager | 6 | 4.43 s | 772 ms | 870 ms | repos by meaning: 520 ms |
| search: self-hosted photo backup | 6 | 4.10 s | 737 ms | 1.02 s | repos by meaning: 612 ms |
| rising | 5 | 560 ms | 344 ms | 410 ms | their releases in the window: 164 ms |
| trends | 4 | 52 ms | 48 ms | 54 ms | the picked ones month by month: 29 ms |
| weekly | 8 | 360 ms | 366 ms | 404 ms | their releases in the window: 248 ms |
| ask: who works on rust-lang/rust | 3 | 505 ms | 287 ms | 319 ms | the work, by person: 271 ms |
| ask: languages by stars in 2015 | 1 | 8.2 ms | 7.2 ms | 7.7 ms | languages by stars: 7.6 ms |
| for you: sindresorhus | 6 | 10.33 s | 10.30 s | 11.02 s | what those people starred: 5.01 s (refused: shard 0 (http://gh-s0.strata.svc:7701): no answer i) |

The statements, with their parameters, are in `results/events.json`.
