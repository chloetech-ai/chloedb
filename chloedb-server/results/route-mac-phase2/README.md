# route-mac-phase2

`chloedb-route-bench --scenarios equal,stall,busy,ryw`, 2026-11-11, Apple M1 Pro, 10 cores, 32 GB, macOS 27.0.1, one process over loopback.

Phase 2: each scenario's configurations take turns in one run, the options off (the "before") and on (`--hedge-adaptive`, `--prefer-caught-up`, `route_load` off for the busy ablation). The tail is the average plus six deviations; that number was chosen on short runs of the stall scenario.

The `ryw` runs here predate the 60 s fallback to the slow after-write average and are superseded by `route-mac-phase2-ryw/`. The kept-statistics change for reads after a write (`kept_stats` ordered by the read's `min`) came after both; neither scenario uses kept statistics for those reads (one shard, reads by key).
