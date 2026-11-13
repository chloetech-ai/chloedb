"""Teacher vectors of the training texts (C4 passages and spans), cached."""
import sys, json, os
import torch
from sentence_transformers import SentenceTransformer
from common import cached, unit, hf
torch.set_num_threads(int(os.environ.get("THREADS", "4")))
repo, data = sys.argv[1], sys.argv[2]
maxlen = int(os.environ.get("MAXLEN", "160"))
t = json.load(open(os.path.expanduser(data)))
m = SentenceTransformer(hf(repo), device="cpu", model_kwargs={"torch_dtype": getattr(torch, os.environ.get("DTYPE", "bfloat16"))})
m.max_seq_length = maxlen
for part in ("spans", "passages"):
    X, dt = cached(f"teacher|{repo}|{maxlen}|{data}|{part}", lambda: unit(m.encode(t[part], batch_size=64, convert_to_numpy=True, normalize_embeddings=True)))
    print(part, X.shape, dt, flush=True)
