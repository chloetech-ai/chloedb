# 4 threads, streaming read 30.1 GB/s; 4.9 GB of 4-bit 8B weights read once per token = 163 ms
# budget 6.25% of the indexed tokens, clusters of ~16, tail 256, sink 16, k-means 4 iterations on 25% of rows; keys synthetic

| tokens | dense attention, one layer | IVF, one layer | attention speedup | tokens read | 8B decode step, dense (32 layers + weights) | 8B, IVF | 8B speedup | index build, one layer | build FLOPs / prefill FLOPs |
|---|---|---|---|---|---|---|---|---|---|
| 131072 | 26.34 ms | 3.14 ms | 8.4x | 6.5% | 1006 ms | 263 ms | 3.82x | 28.85 s | 2.2% |
