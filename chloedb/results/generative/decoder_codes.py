"""The decoder of tiger.py on codes from a plain or a balanced codebook.

    python decoder_codes.py --dir DATA --work WORK            # plain
    python decoder_codes.py --dir DATA --work WORK --balanced

WORK holds emb.npy and items.json, the items' text vectors
(`python ../sequence/cold.py embed --dir DATA --work WORK` writes them:
bge-small-en-v1.5 through sentence-transformers). The codebook is
chloedb.train.codebook on those vectors (3 levels of 256, seed 0), the
items are written to a table with a semantic-ID field in the order of
items.json, and each item's whole code is read back. The decoder then
trains as tiger.py's does (leave-one-out, validation on 2,000 users,
patience 5) and every user's last item is scored, each step's numbers
from a trie of the items the user does not have. Writes
WORK/<plain|balanced>/result.json.
"""
import argparse
import json
import os
import tempfile
import time
import types

import numpy as np
import torch

import chloedb
import chloedb.train
import tiger

ap = argparse.ArgumentParser()
ap.add_argument("--dir", required=True)
ap.add_argument("--work", required=True)
ap.add_argument("--balanced", action="store_true")
ap.add_argument("--max-minutes", type=float, default=90)
ap.add_argument("--threads", type=int, default=4)
a = ap.parse_args()
torch.set_num_threads(a.threads)
variant = "balanced" if a.balanced else "plain"
out = os.path.join(a.work, variant)
os.makedirs(out, exist_ok=True)

items = json.load(open(os.path.join(a.work, "items.json")))
X = np.load(os.path.join(a.work, "emb.npy")).astype(np.float32)
with tempfile.TemporaryDirectory() as d:
    chloedb.train.codebook(X, "cb", models=d, balanced=a.balanced)
    report = chloedb.train.codebook_report(X, "cb", models=d)
    db = chloedb.Database.create(os.path.join(d, "db"), {"table": "items", "key": "id", "fields": [
        {"name": "id", "type": "keyword"}, {"name": "emb", "type": "vector", "dim": X.shape[1], "index": "flat"},
        {"name": "sid", "type": "keyword", "multi": True, "semantic_id": {"from": "emb", "codebook": "cb"}}]}, models=d)
    for i in range(0, len(items), 2000):
        db.insert([{"id": k, "emb": v.tolist()} for k, v in zip(items[i:i + 2000], X[i:i + 2000])])
    codes = {k: s[-1] for k, s in db.query(f"SELECT id, sid FROM items LIMIT {len(items)}").rows}
    db.close()
assert len(codes) == len(items) and len(set(codes.values())) == len(items)
json.dump({"codes": codes, "info": {"variant": variant, "balanced": a.balanced, "report": report}}, open(os.path.join(out, "codes.json"), "w"))
print(json.dumps(report), flush=True)

seqs = tiger.sequences(os.path.join(a.dir, tiger.FILES["beauty"][0]))
t = types.SimpleNamespace(seed=0, tiger_width=128, tiger_heads=4, tiger_layers=4, tiger_ffn=1024, tiger_tokens="item", tiger_lr=1e-3,
                          tiger_epochs=200, batch=48, patience=5, beam=20, val_users=2000, max_minutes=a.max_minutes)
t0 = time.time()
tiger.tiger(t, seqs, out)
train_s = time.time() - t0
saved = torch.load(os.path.join(out, "tiger.pt"))
model = tiger.Tiger(**saved["config"], dropout=0.1)
model.load_state_dict(saved["state"])
model.eval()
pc = {k: tiger.parse(v) for k, v in codes.items()}
inv = {tuple(c): i for i, c in pc.items()}
trie = tiger.Trie(pc)
users, rows = list(seqs), []
for j in range(0, len(users), 64):
    us = users[j:j + 64]
    srcs = [tiger.source(model, u, seqs[u][:-1], pc) for u in us]
    got = tiger.beam_search(model, srcs, lambda k, ps, us=us: trie.allowed(seqs[us[k]][:-1], ps), beam=20)
    for u, g in zip(us, got):
        rows.append(tiger.metrics([inv[tuple(c)] for c in g], seqs[u][-1]))
res = {"codebook": variant, "report": report, "test": tiger.mean_metrics(rows), "users": len(rows), "train_seconds": round(train_s), "history": saved["history"]}
print(json.dumps(res["test"]))
json.dump(res, open(os.path.join(out, "result.json"), "w"), indent=1)
