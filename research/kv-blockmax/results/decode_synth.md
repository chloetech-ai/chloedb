# 4 threads, streaming read 31.4 GB/s; 4.9 GB of 4-bit 8B weights read once per token = 156 ms
# budget 6.25% of the indexed tokens, clusters of ~16, tail 256, sink 16, k-means 8 iterations on 100% of rows; keys synthetic

| tokens | dense attention, one layer | IVF, one layer | attention speedup | tokens read | 8B decode step, dense (32 layers + weights) | 8B, IVF | 8B speedup | index build, one layer | build FLOPs / prefill FLOPs |
|---|---|---|---|---|---|---|---|---|---|
| 32768 | 6.58 ms | 0.91 ms | 7.3x | 7.1% | 366 ms | 185 ms | 1.98x | 6.37 s | 5.3% |
| 65536 | 12.06 ms | 1.45 ms | 8.3x | 6.7% | 542 ms | 202 ms | 2.68x | 26.14 s | 7.7% |
| 131072 | 27.16 ms | 2.98 ms | 9.1x | 6.5% | 1025 ms | 251 ms | 4.08x | 127.27 s | 10.0% |
