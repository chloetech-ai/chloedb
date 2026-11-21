#!/bin/bash
# Bigger ChloeCtx search contexts (16,000 and 48,000 characters) for the same 100 questions.
set -x
B=/home/user/build-target/release
D=/home/user/data/memory
O=/home/user/bench/persona
export CHLOECTX_MODELS=/home/user/data/models TMPDIR=$O/tmp
cd $O
$B/chloectx-structure --data $D/longmemeval_s.jsonl --every 5 --layouts win400+req/vec-text --out $O/structure_big --max-pieces 200 --dump "win400+req/vec-text/snip600/4000" --dump "win400+req/vec-text/snip600/16000" --dump "win400+req/vec-text/snip600/48000"
echo ALLDONE
