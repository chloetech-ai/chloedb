"""Writes a Model2Vec model with its table in int8 (one scale for the table,
as Model2Vec's quantize_to="int8" writes it) or float16, for ChloeDB's
StaticEmbedder."""
import sys, os, shutil, numpy as np
from safetensors.numpy import load_file, save_file
src, out, to = os.path.expanduser(sys.argv[1]), os.path.expanduser(sys.argv[2]), sys.argv[3]
os.makedirs(out, exist_ok=True)
for f in os.listdir(src):
    if f != "model.safetensors" and os.path.isfile(os.path.join(src, f)):
        shutil.copy(os.path.join(src, f), out)
t = load_file(os.path.join(src, "model.safetensors"))
E = t["embeddings"].astype(np.float32)
if to == "int8":
    s = np.abs(E).max() / 127.0
    t["embeddings"] = np.clip(np.rint(E / s), -127, 127).astype(np.int8)
else:
    t["embeddings"] = E.astype(np.float16)
save_file(t, os.path.join(out, "model.safetensors"))
print(out, {k: (v.dtype, v.shape) for k, v in t.items()}, os.path.getsize(os.path.join(out, "model.safetensors")) >> 20, "MB")
