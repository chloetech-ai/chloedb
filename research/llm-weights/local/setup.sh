#!/bin/sh
# Makes .venv here with torch, transformers and chloedb (about 1 GB), then
# downloads Qwen3-0.6B (about 1.2 GB) into the Hugging Face cache.
# torch has wheels for Python 3.10 to 3.13, so the newest of those is used
# (PYTHON=... overrides).
set -e
cd "$(dirname "$0")"
if [ -z "$PYTHON" ]; then
  for v in 3.13 3.12 3.11 3.10; do
    for p in "python$v" "/opt/homebrew/bin/python$v" "/usr/local/bin/python$v"; do
      if command -v "$p" >/dev/null 2>&1; then PYTHON="$p"; break 2; fi
    done
  done
fi
[ -n "$PYTHON" ] || { echo "needs Python 3.10 to 3.13 (brew install python@3.13), or PYTHON=/path/to/python3 ./setup.sh"; exit 1; }
echo "using $PYTHON ($("$PYTHON" --version))"
"$PYTHON" -m venv --clear .venv
.venv/bin/pip install --quiet --upgrade pip
.venv/bin/pip install --quiet torch transformers numpy "chloedb>=0.3.9"
.venv/bin/python -c "from transformers import AutoTokenizer, AutoModelForCausalLM as M; AutoTokenizer.from_pretrained('Qwen/Qwen3-0.6B'); M.from_pretrained('Qwen/Qwen3-0.6B')"
echo "ready: .venv/bin/python inside_llm.py facts   (or ffn)"
