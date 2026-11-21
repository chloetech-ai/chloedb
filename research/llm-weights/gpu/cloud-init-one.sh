#!/bin/bash
# First boot of a one-model GPU machine (@REPO@, @JOBS@; the placeholders are filled when it is created): set up, then run the jobs and serve /root/out read-only.
set -x
mkdir -p /root/out /root/s/models; exec > /root/out/setup.log 2>&1
cd /root && nohup python3 -m http.server 80 --directory /root/out > /dev/null 2>&1 &
export DEBIAN_FRONTEND=noninteractive
apt-get update -q && apt-get install -yq git python3-venv python3-pip
mkdir -p /home/user && git clone -q --depth 50 --branch claude/project-thread-2usw9h https://github.com/chloetech-ai/chloedb.git /home/user/chloedb
python3 -m venv /root/venv && /root/venv/bin/pip install -q torch transformers numpy requests chloedb==0.3.9 huggingface_hub pyarrow
/root/venv/bin/python - <<'PY'
from huggingface_hub import snapshot_download, hf_hub_download
import pyarrow.parquet as pq
snapshot_download("@REPO@", local_dir="/root/s/models/@DIR@")
p = hf_hub_download("Salesforce/wikitext", "wikitext-2-raw-v1/test-00000-of-00001.parquet", repo_type="dataset")
open("/root/s/wikitext2_test.txt", "w").write("".join(pq.read_table(p).column("text").to_pylist()))
PY
/root/venv/bin/python -c "import torch, chloedb; print('cuda', torch.cuda.is_available(), torch.__version__)"
nvidia-smi
echo SETUP_DONE
JOBS=@JOBS@ nohup bash /home/user/chloedb/research/llm-weights/gpu/runner.sh > /root/out/runner.log 2>&1 &
