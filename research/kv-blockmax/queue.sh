#!/bin/bash
# The runs behind README.md, in order (about 3.5 hours on 4 cores, 15 GB).
set -ex
python3 run.py ppl     --L 8192  --books 3 --out results/ppl_8k.json
python3 run.py passkey --L 8192  --trials 6 --out results/passkey_8k.json
python3 run.py ppl     --L 16384 --books 2 --out results/ppl_16k.json
python3 run.py passkey --L 16384 --trials 6 --out results/passkey_16k.json
python3 run.py ppl     --L 32768 --books 1 --budgets 64,128,256,512,1024,2048,4096 --out results/ppl_32k.json
python3 run.py passkey --L 32704 --trials 4 --budgets 64,128,256,512,1024,2048,4096 --out results/passkey_32k.json
python3 run.py ppl     --model Qwen/Qwen3-0.6B --L 8192 --books 2 --out results/qwen3_ppl_8k.json
