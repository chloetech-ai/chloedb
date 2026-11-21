#!/bin/bash
# Readers over one memory: usage reader.sh <reader model> <every> <out> <configs...>
R=$1; E=$2; OUT=$3; shift 3
CFG=()
for c in "$@"; do CFG+=(--config "$c"); done
/home/user/bench/persona/iso.sh python3 -I /home/user/chloedb/chloectx/scripts/reader_eval.py \
  --data /home/user/data/memory/longmemeval_s.jsonl "${CFG[@]}" --reader "$R" --judge haiku \
  --reader-label "$R" --judge-label haiku --every "$E" --workers 8 --out "$OUT"
