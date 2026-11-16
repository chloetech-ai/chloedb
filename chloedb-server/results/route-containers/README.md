# route-containers

`run.sh` (all four scenarios), 2026-11-12. Docker (colima 0.10.3, vz, 8 vCPUs, 16 GB) on an Apple M1 Pro, 10 cores, 32 GB, macOS 27.0.1. One shard of 1M rows (`chloedb-route-bench --build-only`, on the Mac). Each of its 3 copies runs in a container pinned to 2 vCPUs of its own (`--cpuset-cpus` 2-3, 4-5, 6-7). The coordinator, the clients and a zero-delay line in front of every copy share one more container on vCPUs 0-1. Everything is on one Docker network. Each scenario runs 2 rounds of 10 s after a 6 s warm-up, its configurations taking turns. The image is built from this tree: chloedb-server and chloedb-route-bench only.

It answers a question the one-process runs (`../route-mac-phase2*`) could not: what the phase 2 options cost when each copy has capacity of its own, as on separate machines. In one process a hedge, or a replica put back to work, competes for the same cores.

A CPU quota (`--cpus 0.6` for each of 9 copies) was tried first and dropped. A server's threads spend a quota early in each 100 ms period and wait out the rest, which put p99 at 70-90 ms on its own.

Rows below are "off -> on", round 0 / round 1, in ms. Full tables are in `<scenario>/route.md` and every run in `<scenario>/route.json`; `run.log` holds the console output.

## Adaptive hedge (`--hedge-adaptive`)

| scenario | clients | route | p99 | p99.9 | hedges per run |
|---|---|---|---|---|---|
| equal | 1 | least-cost | 2.46 / 2.43 -> 2.61 / 2.42 | 4.8 / 4.9 -> 5.4 / 3.3 | 0 -> 22 / 4 |
| equal | 16 | least-cost | 4.76 / 4.73 -> 4.74 / 4.72 | 5.9 / 5.5 -> 5.4 / 5.5 | 0 -> 2 / 2 |
| equal | 64 | least-cost | 10.1 / 10.0 -> 10.1 / 10.1 | 12.8 / 12.6 -> 12.6 / 13.1 | 0 -> 35 / 103 |
| equal | 64 | round-robin | 10.3 / 10.4 -> 10.3 / 10.6 | 12.9 / 12.9 -> 12.6 / 13.2 | 0 -> 79 / 53 |
| stall | 1 | round-robin | 2.55 / 2.45 -> 5.78 / 5.86 | 64.0 / 63.6 -> 7.0 / 7.4 | 0 -> 74 / 66 |
| stall | 1 | least-cost | 2.42 / 2.40 -> 2.44 / 2.43 | 5.0 / 4.7 -> 4.4 / 3.4 | 0 -> 14 / 5 |
| stall | 16 | least-cost | 4.68 / 4.81 -> 4.85 / 4.76 | 59.0 / 59.3 -> 10.3 / 9.8 | 0 -> 247 / 230 |
| stall | 16 | round-robin | 4.71 / 4.72 -> 4.86 / 4.84 | 61.9 / 62.7 -> 51.6 / 52.8 | 0 -> 121 / 120 |
| stall | 64 | least-cost | 10.6 / 10.4 -> 10.6 / 10.3 | 63.7 / 63.4 -> 56.5 / 59.5 | 0 -> 270 / 244 |

No copy was put in its cool-off in any adaptive run. With a copy's own capacity, adaptive hedging costs nothing measurable at p99 or p99.9 on equal copies; in one process p99.9 was 15-20% worse at 16 clients. On a stalling copy it cuts least-cost's p99.9 at 16 clients six-fold.

One cost remains. Round-robin with one client sees p99 2.5 -> 5.8 ms on the stall scenario, while p99.9 falls from 64 to 7 ms. Queries that hit the stall now end at the hedge, about 6 ms, instead of at the stall's end, about 64 ms. There are more of them than 1%, so they set p99.

## Reads after a write (`--prefer-caught-up`, ryw)

One replica receives what the primary ships 50 ms late. Rows compare least-cost with and without the option.

| clients | read-after-write p99 | read-after-write max | plain qps | plain p99 | lagging replica's answers |
|---|---|---|---|---|---|
| 1 | 2.66 / 2.76 -> 3.11 / 2.64 | 4.9 / 103.6 -> 5.1 / 9.0 | 285 / 280 -> 283 / 273 | 3.06 / 3.12 -> 3.24 / 4.32 | 1 / 0 -> 952 / 907 |
| 16 | 4.39 / 4.57 -> 4.50 / 5.80 | 103.2 / 105.5 -> 102.6 / 32.7 | 3705 / 3471 -> 3839 / 3891 | 5.11 / 5.81 -> 4.88 / 5.07 | 23 / 7 -> 15919 / 17508 |
| 64 | 21.4 / 164.1 -> 11.6 / 12.0 | 110 / 218 -> 18 / 40 | 6083 / 4485 -> 5665 / 5604 | 16.5 / 13.0 -> 12.7 / 13.4 | 174 / 6706 -> 23939 / 23440 |

With capacity per copy, putting the lagging replica back to work raises plain throughput at 16 clients by 4-12%; in one process it fell 5-9%. Read-after-write p99 is about the same at 1 and 16 clients and halves at 64. Least-cost alone had a run at 64 clients with 1,188 hedges and a read-after-write p99 of 164 ms; with the option there are none. Read-after-write p99 at 16 clients is mixed (4.5 against 4.4, and 5.8 against 4.6).

## Reported load (`busy`: copy 0 has 1 query slot)

| clients | round-robin qps / p99 | least-cost qps / p99 | least-cost, load off: qps / p99 |
|---|---|---|---|
| 16 | 7075 / 4.55, 7058 / 4.58 | 6711 / 4.78, 6783 / 4.69 | 6871 / 4.71, 6883 / 4.70 |
| 64 | 12225 / 10.9, 12598 / 13.0 | 15623 / 8.06, 16493 / 7.10 | 16095 / 7.29, 16228 / 7.31 |

Least-cost against round-robin at 64 clients: 28-31% more queries per second and a lower p99, by asking the copy with one slot less. The load the copies report in the survey adds nothing; without it least-cost does as well or slightly better. A fresher copy of that signal (a shard protocol change) is not worth making.
