# Performance and correctness baseline

`baseline.json` is the reference that `tests/perf_baseline.rs` compares a run with.
Recorded on the machine in `meta.machine`, with `hash:256` embeddings (no model
download), a synthetic corpus from a fixed seed and an optimised build.

```bash
cd chloectx
# correctness (part of the normal test run, about 1 s)
cargo test --profile ci --test perf_baseline
# performance against the baseline (about 10 s; keep the machine quiet)
cargo test --profile ci --test perf_baseline -- --ignored --nocapture
# after an intended change: record a new baseline, and say in the PR why it moved
CHLOECTX_PERF=record cargo test --profile ci --test perf_baseline -- --ignored --nocapture
```

What it measures: bulk load, reopen of a flushed store and replay of a 4k-record unflushed log, hybrid / BM25 / vector search at 2k and 20k records,
point reads, reopen, disk size, appends (no fsync, fsync, four writers), the
daemon's hooks over six sessions, peak memory. Quality metrics (hit@1, hit@5, MRR
of planted records; the daemon recalling an earlier preference) may not drop by
more than their tolerance. Latencies fail past 1.6x (p50), 2x (p95), 3x (p99);
throughput below 1/1.6. Repeated runs on the recording machine moved by at most
1.2x. The hooks are timed up to the response; work the daemon defers until
after it (writing the prompt) is not in `daemon.hook_*`. The run never calls a
model: deep recall is switched off in it.

Re-record on a different machine rather than comparing across machines.
