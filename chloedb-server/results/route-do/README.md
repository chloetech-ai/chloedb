# route-do

PR #493's `chloedb-route-bench` rerun on real machines, from 7:40 PM PST on 2026-11-14 to 3:37 AM PST on 11-15 (03:40–11:37 UTC): a throwaway DOKS cluster in DigitalOcean nyc1, deleted afterwards. It asks whether the routing modes `--route least-cost`, `--hedge-adaptive` and `--prefer-caught-up` help, hurt or make no detectable difference against the default, with each copy on a machine of its own, a real network between them, and real slowness (a CPU-busy node, a frozen process, a delayed link) beside the bench's injected delays.

## Answer

- **Least-cost helps.** With 16 or 64 clients it raised throughput, cut p99 or both in every scenario except reads after writes, and it never lost on throughput. With copies alike: +11% at 16 clients and +23% at 64 on 3 shards, p99 down 27–48%. With a copy short of slots or CPU: +33–58% at 64 clients, p99 down 42–55%. At 1 client it makes no detectable difference unless a copy is slow, short of CPU or frozen.
- **But on main it collapses under heavy overload.** A copy a hedge outran gets a 5-second cool-off unless hedging is adaptive, and when every copy is in one, least-cost falls back to plain rotation. With copy 0 beside 24 or 48 CPU hog threads at 64 clients, least-cost on main was no better than round-robin (2,557 against 2,775 and 3,164 against 3,228 queries/s, with p99 230 ms against 61 at 24 threads). `experiment-cool-off-fix.patch` changes the two rules; with it, least-cost held 4,337, 4,252 and 4,220 queries/s with 12, 24 and 48 hog threads (+33%, +66% and +33% over main, p ≤ 0.016) at a p99 of 32–38 ms, as the model in `model/` predicted before the runs. It changed nothing at 16 clients, where nothing hedged, and on reads after writes it added 26% (ryw, 64 clients) and 33% (rywnet, 16 clients) where least-cost had collapsed. The load-balancing simulation's PR #563 makes both rules the coordinator's behaviour, so this folder's `least-cost fixed` is how least-cost behaves once #563 is in.
- **Prefer-caught-up helps** wherever reads follow writes under load: +28–92% throughput over least-cost at 16 and 64 clients, in seven of eight comparisons over two runs of each scenario, in both the bench's late link and a kernel-delayed one. It never hurt.
- **Adaptive hedging helps one thing:** a copy that freezes or stalls. Then least-cost's p99.9 at 16 clients falls 60–76% (64 → 15 ms). Elsewhere it costs p99, and on 3 shards with copies alike or CPU-busy it raises p99.9 at every load (+25–61%). It changed throughput nowhere at 64 clients.
- **Load reports** (`route_load`) made no difference that matters: the significant gaps are within 3% and point both ways.
- **Fewer query slots** (`--max-concurrent 4` and 2 on every copy) never raised throughput, and least-cost's edge over round-robin grew as slots shrank: with copy 0 CPU-busy and 2 slots, 4,644 against 3,072 queries/s at 64 clients (+51%), p99 26.9 against 69.7 ms. No call was refused; they queued for a slot.

`analysis.md` has every comparison: medians and ranges over 5 rounds, judged by an exact two-sided Mann–Whitney U test (5 rounds against 5, so the smallest possible p is 0.008).

## Setup

**Cluster:** DOKS 1.37.2, nyc1. Each node runs one privileged ubuntu:24.04 pod, pinned with nodeName, and the pods talk over the pod network (Cilium).

**Copies:** each copy is a pod on a c-2 node of its own: 2 dedicated vCPUs, 4 GB.
- 1-shard scenarios (`*1`, `ryw*`) use nodes c00, c01 and c02, all Xeon Platinum 8168.
- 3-shard scenarios use c00–c22. Among those, c21 is a Xeon 8280 and c22 a Xeon 8358.
- Servers run chloedb-server from main 1458dff with `--max-concurrent 64 --timeout-ms 30000 --no-token --answer-cache-mb 0`.

**Coordinator, clients and delay lines:** all in one pod on a c-32 node (32 dedicated vCPUs, Xeon Platinum 8358), which stayed at 1–4% CPU while the copies ran at 70–93% (`cpu.md`). The bench runs `chloedb-route-bench --external`, whose in-process coordinator reaches the copies through in-process delay lines; those delay only in the scenarios `slow` and `stall`.

**Coordinator options:** `CoordOptions::default()` for each config: hedge 100 ms, kept BM25 statistics. Nothing was tuned on any scenario.

**Run shape:** the bench's generated rows, 1M per shard. 5 rounds of 10 s after a 6 s warm-up, at 1, 16 and 64 closed-loop clients; within each round the configs take turns, so slow drift hits them all alike. Every scenario starts on freshly started copies.

**The answer cache is off.** chloedb-server's answer cache (#456, 64 MiB by default) answers the bench's repeated query mix almost entirely, so with it on the copies did almost none of the work the scenarios meant to load them with: the copy limited to one query slot served as many queries as the others. Every cache-on run was thrown out. Capacity benchmarks of ChloeDB need `--answer-cache-mb 0`.

## Scenarios

Copy 0 means copy 0 of shard 0, node c00. A `1` suffix means 1 shard × 3 copies, like `../route-containers`; otherwise 3 shards × 3 copies.

| scenario | what copy 0 gets | kind |
|---|---|---|
| equal1, equal | nothing: the copies are alike | baseline |
| busy1, busy | `--max-concurrent 1`, the other copies 64 | real limit |
| cpuhalf1 | `stress-ng --cpu 1` on its node | real CPU contention |
| cpubusy1, cpubusy | `stress-ng --cpu 2` on its node | real CPU contention |
| cpuheavy1-12, -24, -48 | `stress-ng --cpu K` on its node; configs round-robin, least-cost, least-cost fixed | real CPU contention, a check of the cool-off finding |
| sigstop1, sigstop | its server frozen with SIGSTOP for 60 ms of every 1000 ms (`stall-loop.pl`) | real freeze |
| stall1, stall | the bench's delay line holds what it answers within a 50 ms window for 60 ms, once a second | injected |
| slow | the bench's delay line adds 30 ms to each answer | injected |
| ryw, ryw-fix | 1 shard with a primary (c02) and replicas c00 and c01; replica 1 gets what the primary ships through the bench's late link, 50 ms each way; writes acknowledged by the primary alone (`--write-acks primary`, `--seq-no`) | injected |
| rywnet, rywnet-fix | the same, but the link is direct and the kernel delays it: `tc netem delay 50ms` on the primary's eth0, filtered to packets for replica 1 | real network delay |
| equal1-mc4, -mc2, cpubusy1-mc4, -mc2 | equal1 and cpubusy1 with `--max-concurrent 4` or 2 on every copy; round-robin and least-cost (asked for by the load-balancing simulation) | real limit |

**Configs:** `round-robin` (the default), `round-robin adaptive` (`--hedge-adaptive`), `least-cost` (`--route least-cost`, load reports on), `least-cost adaptive`, `least-cost no-load` (`route_load = false`), `least-cost prefer` (`--prefer-caught-up`, ryw only), and `least-cost fixed`.

`least-cost fixed` is an experimental build of the bench, from main 1458dff plus `experiment-cool-off-fix.patch`. It changes two cool-off rules in the coordinator: a copy outrun less than `--hedge-ms` after it was asked is not put in its cool-off whatever the hedge mode (main spares it only with adaptive hedging), and when every copy is in its cool-off, least-cost still chooses by cost (main falls back to plain rotation). The copies run main's server.

## Files

- `cache-off/<scenario>/route.json`, `route.md`: the bench's own output for each scenario, the runs every number here comes from. `cache-off-extra/equal1-repeat` is equal1 run again at the end, to see the drift over the night.
- `cache-on/`: the first runs, with the answer cache on, kept only to show why they were thrown out, with the coordinator on a c-8, c-16 or c-32 node (the folder name says which).
- `rows.csv`: one row per run, flattened from every route.json: config, route flags, clients, round, qps, p50/p90/p99/p99.9/max, errors, hedges, 429s, reads after write, shard 0's answers by copy, the coordinator's mean calls in flight per copy, and the setup line.
- `analysis.md`: medians and ranges, and each mode against its baseline, by `analyze.py`.
- `cpu.md`: each node's mean CPU busy share per scenario, from /proc/stat every 2 s (`cpu-sampler.sh`, `cpu.py`).
- `scripts/`: the driver. `setup.sh` starts the pods (`pod.tmpl`), `drive.sh` runs scenarios on fresh copies (`pod-start.sh` starts a server, `pod-calm.sh` stops the hogs and clears netem), `stall-loop.pl` is the SIGSTOP loop. They assume a kubeconfig for a cluster laid out as above.
- `model/`: the discrete-event model of the bench used to predict runs before they ran. `sim.py` is the model, fitted only on equal1 under round-robin (`calib.py`, `fit.json`); `prereg/` holds the predictions with the time they were written. On the 54 held-out runs its median error was 6.5% on throughput and 10.8% on p99; on the 90 runs predicted blind, it was close on 1 shard and at 16 clients, and optimistic at 64 clients on 3 shards.
- `experiment-cool-off-fix.patch`: the `least-cost fixed` change, against 1458dff. It is not for merging: #563 carries the same two rules.

The bench's `--configs` flag, which this PR adds, runs chosen configs by label instead of a scenario's own; `drive.sh` uses it for scenarios whose slowness comes from outside the bench (`--external` with `equal`). The full report, with charts, the model's predictions for setups not measured and the recommendations, is the doc "Replica routing on real machines".
