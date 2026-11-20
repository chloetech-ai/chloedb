"""W4: retrieval as soft tokens (xRAG / REFRAG style), with ChloeDB as the store.
Each chunk is kept as one row with the model's own mean hidden state of it
(a VECTOR) and its norm. A small trained projector turns that vector into K
input embeddings, so a retrieved chunk costs K tokens of context instead of
~160 tokens of text. The model stays frozen; only the projector (~4M
parameters) is trained, on docs that are not among the 16 eval docs.

Eval (same 8,192 tokens as B and D): for every stride the chunk is BM25's
top 1 from the server (other docs only), given as nothing, as text, or as K
soft tokens."""
import json, sys, time, random, os, glob
import numpy as np, torch, chloedb
from common_hf import *

OUT = sys.argv[1]; K = int(os.environ.get("K", 4)); STEPS = int(os.environ.get("STEPS", 300))
D = model.config.hidden_size

# 1. Chunks as rows: the model's vector of each chunk, in an embedded ChloeDB.
path = f"{S}/soft.chloedb"; os.system(f"rm -rf {path}*")
db = chloedb.open(path, schema={"table": "chunks", "key": "id", "fields": [
    {"name": "id", "type": "keyword"}, {"name": "doc", "type": "keyword"}, {"name": "norm", "type": "float"},
    {"name": "vec", "type": "vector", "dim": D}]}, sync=False)
cache = {}
def chunk_vec(cid, body):
    if cid not in cache:
        v = mean_hidden(tok(body, add_special_tokens=False)["input_ids"][:200]); cache[cid] = v
        db.insert([{"id": cid, "doc": cid.split("#")[0], "norm": float(v.norm()), "vec": v.numpy()}])
    return cache[cid]
def vec_from_db(cid):
    r = db.query("SELECT vec, norm FROM chunks WHERE id = :i", i=cid).rows[0]
    return torch.tensor(np.array(r[0], dtype=np.float32)) * r[1]

# 2. Projector: vector -> K input embeddings, scaled like real token embeddings.
emb_rms = float(EMB.weight.pow(2).mean().sqrt())
proj = torch.nn.Sequential(torch.nn.Linear(D, 2 * D), torch.nn.GELU(), torch.nn.Linear(2 * D, K * D))
def soft(v): return proj(v / v.norm()).view(K, D) * emb_rms

# 3. Training pairs from docs other than the eval docs: (chunk retrieved by BM25 for a position, ctx, tgt),
#    and reconstruction (chunk -> its own first 48 tokens).
train_docs = [f for f in sorted(glob.glob(f"{DOCS}/chloedb/*.md") + glob.glob(f"{DOCS}/chloectx/*.md"))
              if not any(f.endswith(f"chloedb/{e}.md") for e in EVAL)]
rng = random.Random(3); pairs = []
for f in train_docs:
    ids = toks(f); doc = os.path.relpath(f, DOCS)
    for s in range(CTX, min(len(ids) - STRIDE, 1600), 256):
        q = tok.decode(ids[s - 32:s]); r = bm25(q, doc, 1)
        if r: pairs.append((r[0][0], r[0][1], ids[s - CTX:s], ids[s:s + STRIDE]))
rng.shuffle(pairs)
opt = torch.optim.AdamW(proj.parameters(), lr=3e-4, weight_decay=0.01)
t0 = time.time(); losses = []
for step in range(STEPS):
    cid, body, ctx, tgt = pairs[step % len(pairs)]
    v = chunk_vec(cid, body)
    if step % 2:
        loss = nll(soft(v), ctx, tgt, grad=True)
    else:
        own = tok(body, add_special_tokens=False)["input_ids"][:48]
        loss = nll(soft(v), [], own, grad=True)
    opt.zero_grad(); loss.backward(); opt.step(); losses.append(loss.item())
    if step % 25 == 0: print(step, round(float(np.mean(losses[-25:])), 3), round(time.time() - t0), flush=True)
train = {"pairs": len(pairs), "steps": STEPS, "secs": round(time.time() - t0), "params": sum(p.numel() for p in proj.parameters())}

# 4. Eval: none, text chunk, K soft tokens (the vector read back from ChloeDB).
torch.save(proj.state_dict(), f"{S}/w4_proj.pt")
# Controls: the same projector fed another chunk's vector, or the mean vector of all chunks
# (if those help as much, the soft tokens are a learned domain prompt, not retrieval).
allv = torch.stack(list(cache.values())); meanv = allv.mean(0); crng = random.Random(11)
res = {"none": [], "text": [], "soft": [], "soft_other_chunk": [], "soft_mean_vector": []}; ms = []
for e, s, ctx, tgt in segments():
    doc = f"chloedb/{e}.md"; r = bm25(tok.decode(ctx[-32:]), doc, 1)
    res["none"].append(float(nll(None, ctx, tgt)))
    if not r:
        for k in ("text", "soft", "soft_other_chunk", "soft_mean_vector"): res[k].append(res["none"][-1])
        continue
    cid, body = r[0][0], r[0][1]
    res["text"].append(float(nll(text_embeds(body), ctx, tgt)))
    chunk_vec(cid, body); t = time.perf_counter(); v = vec_from_db(cid); ms.append((time.perf_counter() - t) * 1000)
    with torch.no_grad():
        res["soft"].append(float(nll(soft(v), ctx, tgt)))
        res["soft_other_chunk"].append(float(nll(soft(allv[crng.randrange(len(allv))]), ctx, tgt)))
        res["soft_mean_vector"].append(float(nll(soft(meanv), ctx, tgt)))
out = {"K": K, "train": train, "ppl": {k: ppl(v) for k, v in res.items()},
       "prefix_tokens": {"none": 0, "text": 160, "soft": K}, "row_read_ms_p50": float(np.median(ms)),
       "soft_vs_none_nll": float(np.mean(np.array(res["soft"]) - np.array(res["none"]))),
       "soft_share_of_text_gain": float((np.mean(res["none"]) - np.mean(res["soft"])) / (np.mean(res["none"]) - np.mean(res["text"])))}
json.dump(out, open(OUT, "w"), indent=1); print(json.dumps(out, indent=1))
