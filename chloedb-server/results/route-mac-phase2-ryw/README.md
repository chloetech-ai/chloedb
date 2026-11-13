# route-mac-phase2-ryw

`chloedb-route-bench --scenarios ryw`, 2026-11-11, same machine as `route-mac-phase2/`.

Phase 2 with the 60 s fallback to the slow after-write average: one shard, a primary and two replicas, replica 1 receiving what the primary ships 50 ms late; a quarter of operations write and read back by key with `min_seq_nos`. Supersedes the `ryw` rows of `route-mac-phase2/`.
