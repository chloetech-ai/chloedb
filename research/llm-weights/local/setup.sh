#!/bin/sh
# Makes .venv here with torch, transformers and chloedb (about 1 GB), then
# downloads Qwen3-0.6B (about 1.2 GB) into the Hugging Face cache.
set -e
cd "$(dirname "$0")"
python3 -m venv .venv
.venv/bin/pip install --quiet --upgrade pip
.venv/bin/pip install --quiet torch transformers numpy "chloedb>=0.3.9"
.venv/bin/python -c "from transformers import AutoTokenizer, AutoModelForCausalLM as M; AutoTokenizer.from_pretrained('Qwen/Qwen3-0.6B'); M.from_pretrained('Qwen/Qwen3-0.6B')"
echo "ready: .venv/bin/python inside_llm.py facts   (or ffn)"
