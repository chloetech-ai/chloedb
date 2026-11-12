#!/bin/bash
# The runs behind the three uses (chloedb-server running on :7700 with
# --media-store DIR --media-max-bytes 100000000 --max-body-mb 512).
set -x
S=http://127.0.0.1:7700
for n in 2048 8192 16384 32768; do
  for m in put get local; do python3 run_tier.py $m --tokens $n --server $S --out results/tier_1p5b.jsonl; done
done
python3 run_ctx.py --server $S --out results/ctx_1p5b.json
python3 run_rag.py --n 20 --server $S --out results/rag_1p5b.json
echo ALLDONE
