"""A job that downloads a model: REPO=Qwen/Qwen3-1.7B DEST=/root/s/models/qwen3-1.7b."""
import json, os, sys, time
from huggingface_hub import snapshot_download
t0 = time.time(); p = snapshot_download(os.environ["REPO"], local_dir=os.environ["DEST"])
json.dump({"repo": os.environ["REPO"], "path": p, "secs": round(time.time() - t0)}, open(sys.argv[1], "w"))
