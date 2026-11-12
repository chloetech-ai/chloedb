real K/V: 32768 tokens per head
# 4 threads, streaming read 35.0 GB/s; 4.9 GB of 4-bit 8B weights read once per token = 140 ms
# budget 6.25% of the indexed tokens, clusters of ~16, tail 256, sink 16, k-means 8 iterations on 100% of rows; keys real (dump)

| tokens | dense attention, one layer | IVF, one layer | attention speedup | tokens read | 8B decode step, dense (32 layers + weights) | 8B, IVF | 8B speedup | index build, one layer | build FLOPs / prefill FLOPs |
|---|---|---|---|---|---|---|---|---|---|
check n=32768: max |dense - ivf(all)| = 5.00679e-6
| 32768 | 6.81 ms | 0.96 ms | 7.1x | 7.1% | 358 ms | 171 ms | 2.10x | 5.76 s | 5.3% |
