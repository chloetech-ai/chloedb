## Repairing stitched caches (Qwen2.5-1.5B, LongBench-E HotpotQA + 2WikiMQA, 40 questions)

The joint prefill and the plain stitch are run_rag.py's (`results/rag_1p5b.json`); the stitch is rerun here and scores the same. Median joint prefill 17.6 s; median seconds to recompute, on the same 4 cores.

| condition | tokens recomputed | F1 | exact match | recompute (median) | better / worse than joint, per question |
|---|---|---|---|---|---|
| all passages prefilled together | all | 55.7 | 42.5 | 17.6 s | |
| stitched, nothing recomputed | 0 | 37.0 | 27.5 | 0.0 s | 4 / 13 |
| the first tokens of every passage, 5% | 5% | 43.6 | 32.5 | 1.1 s | 4 / 10 |
| the first tokens of every passage, 15% | 15% | 55.9 | 42.5 | 3.1 s | 3 / 2 |
| the first tokens of every passage, 30% | 30% | 55.5 | 42.5 | 5.8 s | 2 / 2 |
| largest K/V deviation at layer 1 (CacheBlend), 5% | 5% | 38.5 | 27.5 | 2.3 s | 2 / 11 |
| largest K/V deviation at layer 1 (CacheBlend), 15% | 15% | 50.1 | 40.0 | 4.4 s | 3 / 6 |
| largest K/V deviation at layer 1 (CacheBlend), 30% | 30% | 50.8 | 40.0 | 7.8 s | 2 / 5 |
| random, 5% | 5% | 37.0 | 27.5 | 2.2 s | 4 / 13 |
| random, 15% | 15% | 35.7 | 27.5 | 4.2 s | 3 / 13 |
| random, 30% | 30% | 34.6 | 25.0 | 7.3 s | 1 / 12 |

Deviation and random choices also replace layers 0 and 1 with a fresh joint prefill of those two layers (included in their time); the first-tokens choice does not.

## One decode step in Rust: dense attention against IVF over keys (decode-bench)

`results/decode_real_32k.md`:

# 4 threads, streaming read 35.0 GB/s; 4.9 GB of 4-bit 8B weights read once per token = 140 ms
# budget 6.25% of the indexed tokens, clusters of ~16, tail 256, sink 16, k-means 8 iterations on 100% of rows; keys real (dump)
| tokens | dense attention, one layer | IVF, one layer | attention speedup | tokens read | 8B decode step, dense (32 layers + weights) | 8B, IVF | 8B speedup | index build, one layer | build FLOPs / prefill FLOPs |
|---|---|---|---|---|---|---|---|---|---|
| 32768 | 6.81 ms | 0.96 ms | 7.1x | 7.1% | 358 ms | 171 ms | 2.10x | 5.76 s | 5.3% |

`results/decode_synth.md`:

# 4 threads, streaming read 31.4 GB/s; 4.9 GB of 4-bit 8B weights read once per token = 156 ms
# budget 6.25% of the indexed tokens, clusters of ~16, tail 256, sink 16, k-means 8 iterations on 100% of rows; keys synthetic
| tokens | dense attention, one layer | IVF, one layer | attention speedup | tokens read | 8B decode step, dense (32 layers + weights) | 8B, IVF | 8B speedup | index build, one layer | build FLOPs / prefill FLOPs |
|---|---|---|---|---|---|---|---|---|---|
| 32768 | 6.58 ms | 0.91 ms | 7.3x | 7.1% | 366 ms | 185 ms | 1.98x | 6.37 s | 5.3% |
| 65536 | 12.06 ms | 1.45 ms | 8.3x | 6.7% | 542 ms | 202 ms | 2.68x | 26.14 s | 7.7% |
| 131072 | 27.16 ms | 2.98 ms | 9.1x | 6.5% | 1025 ms | 251 ms | 4.08x | 127.27 s | 10.0% |

`results/decode_synth_128k_sampled.md`:

# 4 threads, streaming read 30.1 GB/s; 4.9 GB of 4-bit 8B weights read once per token = 163 ms
# budget 6.25% of the indexed tokens, clusters of ~16, tail 256, sink 16, k-means 4 iterations on 25% of rows; keys synthetic
| tokens | dense attention, one layer | IVF, one layer | attention speedup | tokens read | 8B decode step, dense (32 layers + weights) | 8B, IVF | 8B speedup | index build, one layer | build FLOPs / prefill FLOPs |
|---|---|---|---|---|---|---|---|---|---|
| 131072 | 26.34 ms | 3.14 ms | 8.4x | 6.5% | 1006 ms | 263 ms | 3.82x | 28.85 s | 2.2% |

## Writing a stored cache to chloedb-server (28 rows, one per layer)

| tokens | bytes | base64 in /v1/write | POST /v1/media + /v1/write | read back from /v1/media | same bytes |
|---|---|---|---|---|---|
| 2048 | 56 MiB | 1.33 s | 0.16 s | 0.25 s | yes |
| 8192 | 224 MiB | 5.10 s | 0.40 s | 0.74 s | yes |
| 16384 | 448 MiB | 10.94 s | 0.77 s | 1.73 s | yes |
| 32768 | 896 MiB | 23.43 s | 2.01 s | 3.72 s | yes |

## Cluster reuse across decode steps (Qwen3-0.6B, 32768 tokens of The Stones of Venice Volume I (of 3) by John Ruskin, 127 steps, budget 2048 tokens a KV head)

NLL of the decoded tokens: sparse 3.556, dense 3.620. Mean over layers 2 to 27 and KV heads; read per step 6.1% of the prefix.

| RAM cache holds the clusters of the last | share of a step's tokens found in it | its size, share of the prefix |
|---|---|---|
| 1 step | 68.6% | 6.1% |
| 4 steps | 83.9% | 10.6% |
| 16 steps | 92.7% | 18.8% |
| 64 steps | 97.7% | 30.7% |

Lowest reuse (16 steps): layer 16 86%, layer 11 88%, layer 13 90%.
