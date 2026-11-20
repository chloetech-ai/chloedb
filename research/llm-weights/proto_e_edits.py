"""Aggressive idea E: facts written into a layer of the model as ChloeDB rows.
A GRACE-style codebook (Hartvigsen et al., 2023) at one transformer layer:
each row is a key (the layer's hidden state at a prompt's last token), a
value (a replacement hidden state, trained by gradient descent so the model
says the new fact) and the fact's text. In every forward pass the layer's
last-position hidden state is a vector query; a row closer than the
threshold replaces the activation. The weights never change: INSERT teaches
a fact, DELETE removes it, and each answer can name the row it came from.

Facts are about ChloeDB, which Qwen3-0.6B cannot know."""
import json, os, sys, time, math
import numpy as np, torch, chloedb
from transformers import AutoTokenizer, AutoModelForCausalLM

S = os.environ["S"]; OUT = sys.argv[1]; LAYER = int(os.environ.get("LAYER", 12))
torch.set_num_threads(4)
tok = AutoTokenizer.from_pretrained(f"{S}/models/qwen3-0.6b-hf")
model = AutoModelForCausalLM.from_pretrained(f"{S}/models/qwen3-0.6b-hf", dtype=torch.float32).eval()
for p in model.parameters(): p.requires_grad_(False)

FACTS = [  # prompt, target, paraphrase
 ("Before November 2026, the ChloeDB database was called", " Strata", "ChloeDB's former name was"),
 ("Before its rename, the ChloeCtx memory system was called", " Engram", "The old name of ChloeCtx is"),
 ("By default, chloedb-server serves HTTP on port", " 7700", "The default HTTP port of chloedb-server is"),
 ("By default, chloedb-server serves gRPC on port", " 7701", "The gRPC port chloedb-server uses by default is"),
 ("The Kubernetes resource kind of a ChloeDB cluster is", " StrataCluster", "To run ChloeDB on Kubernetes you create a resource of kind"),
 ("ChloeCtx keeps each project's memory in a file named", " memory.strata", "The file that holds a ChloeCtx store is called"),
 ("The ChloeDB clause that fills a token budget with rows is", " PACK", "To fit rows into a token budget, ChloeDB SQL uses the clause"),
 ("The Java package of ChloeDB is", " ai.chloetech.chloedb", "ChloeDB's Java classes live in the package"),
 ("The company that makes ChloeDB is", " Chloetech", "ChloeDB is developed by"),
 ("ChloeDB's command center in a terminal is started with chloedb-server", " cc", "The terminal command center subcommand of chloedb-server is"),
 ("In ChloeDB SQL, a gradient-boosted tree model is called with the function", " gbdt", "The ChloeDB function that scores rows with LightGBM trees is"),
 ("ChloeDB's GitHub organization is", " chloetech-ai", "The GitHub org that hosts ChloeDB is"),
 ("The ChloeDB app for Hacker News is the crate", " chloedb-hn", "The crate that puts Hacker News on ChloeDB is"),
 ("ChloeCtx's inspector UI is opened with the command chloectx", " ui", "To open ChloeCtx's local inspector, run chloectx"),
 ("ChloeDB's reciprocal rank fusion function is", " rrf", "In ChloeDB, fusing rankings by reciprocal rank uses the function"),
 ("The ChloeDB app for arXiv papers is the crate", " chloedb-arxiv", "arXiv runs on ChloeDB in the crate"),
]
LOCAL = ["The capital of France is", "Water boils at a temperature of", "The author of Hamlet is",
         "The largest planet in the solar system is", "Python is a programming language created by",
         "PostgreSQL is a relational", "The chemical symbol for gold is", "The SQL keyword to sort results is",
         "The default port of PostgreSQL is", "Redis is an in-memory"]

path = f"{S}/edits.chloedb"; os.system(f"rm -rf {path}*")
db = chloedb.open(path, schema={"table": "edits", "key": "id", "fields": [
    {"name": "id", "type": "keyword"}, {"name": "fact", "type": "text"}, {"name": "vnorm", "type": "float"},
    {"name": "key", "type": "vector", "dim": 1024}, {"name": "val", "type": "vector", "dim": 1024}]}, sync=False)

STATE = {"on": False, "thr": 0.9, "hits": [], "force": None, "ms": [], "mode": "last"}
# ChloeDB scores vectors by their dot product as stored, so keys and values are stored unit length here
# (the norm kept in vnorm) and score() is the cosine.
def hook(mod, args, out):
    h = out[0] if isinstance(out, tuple) else out
    if h.shape[1] < 2: return out           # only the prompt's pass; the edit then sits in the KV cache
    last = h[0, -1]
    if STATE["force"] is not None:
        h = h.clone(); h[0, -1] = STATE["force"]
    elif STATE["on"]:
        t = time.perf_counter()
        q = (h[0].mean(0) if STATE["mode"] == "mean" else h[0, 1:].mean(0) if STATE["mode"] == "mean1" else last).detach()
        r = db.query("SELECT id, val, vnorm, score() FROM edits ORDER BY similarity(key, :v) DESC LIMIT 1",
                     v=(q / q.norm()).numpy().astype(np.float32)).rows
        STATE["ms"].append((time.perf_counter() - t) * 1000)
        if r and r[0][3] >= STATE["thr"]:
            STATE["hits"].append(r[0][0]); h = h.clone()
            h[0, -1] = torch.tensor(np.array(r[0][1], dtype=np.float32) * r[0][2])
        else: STATE["hits"].append(None)
    return (h,) + tuple(out[1:]) if isinstance(out, tuple) else h
model.model.layers[LAYER].register_forward_hook(hook)

def ids(s): return tok(s, return_tensors="pt")["input_ids"]
def key_of(prompt, mode="last"):
    STATE["force"] = None; on = STATE["on"]; STATE["on"] = False; cap = {}
    hd = model.model.layers[LAYER].register_forward_hook(lambda m, a, o: cap.__setitem__("h", (o[0] if isinstance(o, tuple) else o)[0].clone()))
    with torch.no_grad(): model(ids(prompt))
    hd.remove(); STATE["on"] = on
    return cap["h"].mean(0) if mode == "mean" else cap["h"][1:].mean(0) if mode == "mean1" else cap["h"][-1]
def answer(prompt, n=6):
    with torch.no_grad():
        out = model.generate(ids(prompt), max_new_tokens=n, do_sample=False, pad_token_id=tok.eos_token_id)
    return tok.decode(out[0, ids(prompt).shape[1]:])
def hit(prompt, target): return answer(prompt).strip().lower().startswith(target.strip().lower())

def train_value(prompt, target, steps=60):
    k = key_of(prompt); v = k.clone().requires_grad_(True); opt = torch.optim.Adam([v], lr=0.05 * float(k.norm()) / 32)
    p, t = ids(prompt), ids(target)[:, :]; full = torch.cat([p, t], 1); labels = full.clone(); labels[:, :p.shape[1]] = -100
    def hook2(m, a, o):
        h = (o[0] if isinstance(o, tuple) else o).clone(); h[0, p.shape[1] - 1] = v
        return (h,) + tuple(o[1:]) if isinstance(o, tuple) else h
    STATE["on"] = False; hd = model.model.layers[LAYER].register_forward_hook(hook2)
    for i in range(steps):
        loss = model(full, labels=labels).loss
        opt.zero_grad(); loss.backward(); opt.step()
        if loss.item() < 0.02: break
    hd.remove(); return k.detach(), v.detach(), loss.item(), i + 1

res = {"model": "Qwen3-0.6B (HF, fp32, CPU)", "layer": LAYER, "facts": len(FACTS)}
STATE["on"] = False
res["before"] = {"facts": sum(hit(p, t) for p, t, _ in FACTS), "paraphrases": sum(hit(q, t) for _, t, q in FACTS)}
local_before = [answer(p) for p in LOCAL]

# Which key separates a fact's paraphrase from unrelated prompts better: the last token's state or the mean?
cos = lambda a, b: float(torch.nn.functional.cosine_similarity(a, b, 0))
res["cos"] = {}
for mode in ("last", "mean", "mean1"):
    keys = [key_of(p, mode) for p, _, _ in FACTS]; para = [key_of(q, mode) for _, _, q in FACTS]; loc = [key_of(p, mode) for p in LOCAL]
    res["cos"][mode] = {"paraphrase_to_own_fact": float(np.median([cos(a, b) for a, b in zip(keys, para)])),
                        "fact_to_other_facts": float(np.median([cos(a, b) for i, a in enumerate(keys) for j, b in enumerate(keys) if i != j])),
                        "unrelated_to_nearest_fact": float(np.median([max(cos(a, b) for b in keys) for a in loc]))}
margin = {m: c["paraphrase_to_own_fact"] - c["unrelated_to_nearest_fact"] for m, c in res["cos"].items()}
STATE["mode"] = os.environ.get("KEYMODE") or max(margin, key=margin.get); res["key_mode"] = STATE["mode"]

t0 = time.time(); train = []
for i, (p, t, q) in enumerate(FACTS):
    k, v, loss, steps = train_value(p, t)
    key = key_of(p, STATE["mode"])
    db.insert([{"id": f"f{i}", "fact": p + t, "vnorm": float(v.norm()), "key": (key / key.norm()).numpy(), "val": (v / v.norm()).numpy()}])
    train.append({"loss": round(loss, 3), "steps": steps})
res["write"] = {"secs_per_fact": round((time.time() - t0) / len(FACTS), 1), "final_loss_median": float(np.median([x["loss"] for x in train]))}

STATE["on"] = True
for thr in (0.99, 0.95, 0.9, 0.85):
    STATE["thr"] = thr; STATE["hits"] = []
    f = sum(hit(p, t) for p, t, _ in FACTS); pa = sum(hit(q, t) for _, t, q in FACTS)
    loc_same = sum(answer(p) == b for p, b in zip(LOCAL, local_before))
    res[f"after_thr_{thr}"] = {"facts": f, "paraphrases": pa, "unrelated_unchanged": f"{loc_same}/{len(LOCAL)}"}
res["lookup_ms_p50"] = float(np.median(STATE["ms"]))
ok = [t for t in (0.99, 0.95, 0.9, 0.85) if res[f"after_thr_{t}"]["unrelated_unchanged"] == f"{len(LOCAL)}/{len(LOCAL)}"]
BEST_THR = min(ok) if ok else 0.99; res["threshold_used_for_delete"] = BEST_THR

STATE["thr"] = BEST_THR
db.sql("DELETE FROM edits WHERE id IN ('f0','f1','f2','f3','f4','f5','f6','f7')")
res["after_delete_half"] = {"deleted_facts_still_said": sum(hit(p, t) for p, t, _ in FACTS[:8]),
                            "kept_facts_said": sum(hit(p, t) for p, t, _ in FACTS[8:])}
res["examples"] = {p: answer(p) for p, _, _ in FACTS[8:11]}
json.dump(res, open(OUT, "w"), indent=1); print(json.dumps(res, indent=1))
