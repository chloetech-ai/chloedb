# Cold segments: what reading them costs

`hdfs.json` is the output of `examples/cold_bench.rs` on the HDFS database
the log-bytes probe made (Loghub's HDFS log, 11.2M lines, 6 segments after
`optimize`; see `../log-bytes/`):

```bash
cargo build --release --example cold_bench
./target/release/examples/cold_bench hdfs.strata WORK 30 100
```

The store is a directory store that answers each read after 30 ms plus its
length at 100 MB/s (a first byte and one connection's throughput on S3),
with eight reads at once. Each cold query runs on a freshly opened database
with an empty cache (`cold_first_ms`), then five more times
(`cold_warm_ms`, the median); `local_ms` is the median of five runs on the
local segments with the page cache warm.

Measured on 2026-11-08 on a cloud container: 4-core Intel Xeon @ 2.10GHz,
15 GB of RAM, at branch `claude/project-thread-t79j93` (commit 0e55a01).
