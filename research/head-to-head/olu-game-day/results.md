# Olu: a game day, ChloeDB's cluster against Elasticsearch's

Olu, an SRE in the persona study, runs Elasticsearch on Kubernetes. In his adoption answer he said ChloeDB "already beats ES by roughly an order of magnitude in my drills: seconds against the tens of seconds to minutes I live with". This page runs the same three drills on both, on one machine, under the same load, and reports what a client saw.

- **ChloeDB:** two shards with three copies each (`--seq-no --failover-after-ms 2000`, as the guide's "Operate a cluster" page runs them), and a coordinator in front.
- **Elasticsearch:** three nodes in Docker, each master-eligible and data, with an index of two primary shards and two replicas each, so three copies of a shard as on ChloeDB. The client knows all three nodes and moves to the next one when a node does not answer. The nodes listen on the machine's loopback, as ChloeDB's copies do; on a network address Elasticsearch enforces its production checks, and this container's file limit is below what they ask for.
- **The load:** a writer sends one row at a time, 50 a second, and waits for each answer (at most 30 s). A refused row is sent again every 0.25 s, the same row with the same id (and on ChloeDB the same idempotency key), as a client does. A reader sends a search 10 times a second. Each drill starts from a healthy cluster holding 5,000 rows, with 10 s of steady load before and after.

Measured on 2026-11-20 on Intel(R) Xeon(R) Processor @ 2.10GHz, 4 cores, 15 GB, Linux 6.18.44-fc-v726; chloedb-server 0.3.9 (built from a3283ace6c); Elasticsearch 9.5.5 (Docker mirror.gcr.io/library/elasticsearch:9.5.5, three nodes, heap 1g each). Run it again with `python3 run.py && python3 report.py` (needs Docker and a chloedb-server build; `H2H_ONLY=chloedb` or `H2H_ONLY=elasticsearch` runs one side and keeps the other's numbers). Raw numbers: [raw/results.json](raw/results.json).

## What it says

- **Kill the primary of shard 0.** Longest time with no write answered: ChloeDB 2.30 s, Elasticsearch 0.45 s. Writes refused for good: 0 of 2,039 against 0 of 3,103; sent more than once: 1 against 0. Longest with no search answered: 0.11 s against 0.22 s. Whole again after the drill: 2.04 s against 21.77 s.
- **Freeze it for 20 s (SIGSTOP).** Longest time with no write answered: ChloeDB 7.26 s, Elasticsearch 20.06 s. Writes refused for good: 0 of 1,265 against 0 of 554; sent more than once: 0 against 0. Longest with no search answered: 0.20 s against 20.08 s. Whole again after the drill: 1.52 s against 0.00 s.
- **Rolling restart of every server.** Longest time with no write answered: ChloeDB 0.06 s, Elasticsearch 0.71 s. Writes refused for good: 0 of 725 against 0 of 4,399; sent more than once: 0 against 0. Longest with no search answered: 0.11 s against 0.25 s. Whole again after the drill: 0.00 s against 0.00 s.
- **Olu's claim, for a client.** ChloeDB's longest pause in writes is shorter in 2 of the 3 drills (freeze, rolling) and longer in 1 (kill). Where ChloeDB is ahead is in getting whole again: after a kill it had every copy back 2.04 s after the server started, Elasticsearch 21.77 s (its replicas recover from the primaries), and a rolling restart took 4.5 s against 78.0 s.
- **A frozen node.** Elasticsearch held the write it was serving for the whole 20 s freeze, searches too, and its cluster stayed green: its fault detection takes longer than the freeze. ChloeDB moved the shard to another copy, but a write still waited 7.26 s, longer than the 2 s `--failover-after-ms` suggests.
- **Every answered write is there.** At the end ChloeDB held 10,529 rows of the 10,529 expected (0 answered and missing), Elasticsearch 14,556 of 14,556 (0). Writes refused but there all the same: 0 and 0.
- **Memory.** After the preload, ChloeDB's seven servers held 447 MB together, Elasticsearch's three containers 4,497 MB (most of it the heap each node is given).

## Each drill

What the client saw from the start of the drill to the end of the steady load after it. A gap is the longest time between two answered requests; a wait is the longest one request took, retries included.

| drill | system | target | writes sent | refused | sent again | longest wait | longest gap | p99 write | searches | search gap | whole again after |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Kill the primary of shard 0, start it again 30 s later | ChloeDB | s0a | 2,039 | 0 | 1 | 2.28 s | 2.30 s | 16.2 ms | 421 (0 failed) | 0.11 s | 2.04 s |
| Kill the primary of shard 0, start it again 30 s later | Elasticsearch | es3 (the master) | 3,103 | 0 | 0 | 0.45 s | 0.45 s | 26.9 ms | 621 (0 failed) | 0.22 s | 21.77 s |
| Freeze it for 20 s (SIGSTOP), then let it go | ChloeDB | s0c | 1,265 | 0 | 0 | 7.26 s | 7.26 s | 16.3 ms | 316 (0 failed) | 0.20 s | 1.52 s |
| Freeze it for 20 s (SIGSTOP), then let it go | Elasticsearch | es1 | 554 | 0 | 0 | 20.04 s | 20.06 s | 16.1 ms | 113 (0 failed) | 20.08 s | 0.00 s |
| Rolling restart of every server | ChloeDB | s0b | 725 | 0 | 0 | 0.04 s | 0.06 s | 19.2 ms | 145 (0 failed) | 0.11 s | 0.00 s |
| Rolling restart of every server | Elasticsearch | es1 | 4,399 | 0 | 0 | 0.70 s | 0.71 s | 45.5 ms | 880 (0 failed) | 0.25 s | 0.00 s |

## What the client was told

The write held longest in each drill, and the first answer it had when it was refused.

| drill | system | waited | tries | first refusal |
|---|---|---|---|---|
| kill | ChloeDB | 2.28 s | 10 | (503, {'docs': 'https://chloetech-ai.github.io/chloedb/site/docs/server.html#failover', 'error': '127.0.0.1:50179 (shard 0): tcp connect error', 'error_code': 'unavailable', 'hint': 'a copy, a shard or a hosted model did |
| kill | Elasticsearch | 0.45 s | 1 | none, it waited |
| freeze | ChloeDB | 7.26 s | 1 | none, it waited |
| freeze | Elasticsearch | 20.04 s | 1 | none, it waited |
| rolling | ChloeDB | 0.04 s | 1 | none, it waited |
| rolling | Elasticsearch | 0.70 s | 1 | none, it waited |

## The rolling restarts, step by step

- **ChloeDB:** restart s0a 0.27 s; restart s0c 0.27 s; handover shard 0 to s0a 1.54 s; restart s0b 0.28 s; restart s1b 0.27 s; restart s1c 0.27 s; handover shard 1 to s1b 1.29 s; restart s1a 0.27 s.
- **Elasticsearch:** restart es1 24.47 s; restart es2 26.55 s; restart es3 26.94 s.

One machine, so no network partitions and no disk failures: those are the drills this page does not run. Both systems share four cores with the load, the same for each.
