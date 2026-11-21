"""Facts as rows (idea E, keys as in X3) scored on the 230 questions, closed book and with RAG.

The facts (FACTS=path, from make_facts.py) were written by Haiku from the same passages as the questions, in calls
that never saw them. Each fact is one value at layer LAYER's last position, trained so that its sentence, two
rewordings and its own question (in the question prompt used here) say the answer; its 4 keys (mean hidden state
without the first token, as in E) are rows of a ChloeDB table that share the value. For a question, the key of the
question on its own is looked up in ChloeDB; if the best row's cosine is at least THR (0.9, fixed in advance as in
E and X3), its value replaces the hidden state at the prompt's last position; otherwise the model runs unchanged."""
import json, os, re, sys, time
import numpy as np, torch, chloedb
from gpu import load, DEVICE
S = os.environ["S"]; OUT = sys.argv[1]; SMOKE = os.environ.get("SMOKE") == "1"
tok, model = load(S)
for p in model.parameters(): p.requires_grad_(False)
NL, D = model.config.num_hidden_layers, model.config.hidden_size
LAYER = int(os.environ.get("LAYER", round(NL * 12 / 28))); THR = float(os.environ.get("THR", 0.9)); layer = model.model.layers[LAYER]
QA = json.load(open(os.environ["QA"])); FACTS = json.load(open(os.environ["FACTS"]))
DOCS = os.environ.get("DOCS", "/home/user/chloedb/docs")
if SMOKE: QA = [x for x in QA if x["doc"] == "python"][:6]; FACTS = [f for f in FACTS if f["doc"] == "python"][:6]
SHOTS = ("Answer each question about the ChloeDB database with a few words.\n"
         "Question: Which SQL keyword sorts the results of a query?\nAnswer: ORDER BY\n"
         "Question: Which file format does Python's json module read?\nAnswer: JSON\n")   # as in proto_g1
def qprompt(q, ctx=""): return ctx + SHOTS + f"Question: {q}\nAnswer:"
def ids(s): return tok(s, return_tensors="pt", add_special_tokens=False)["input_ids"].to(DEVICE)

ST = {"force": None, "inject": None}
def hook(m, a, out):
    h = out[0] if isinstance(out, tuple) else out
    if ST["force"] is not None:                       # training: the value at each prompt's last position
        h = h.clone()
        for b, pos in enumerate(ST["force"][1]): h[b, pos] = ST["force"][0].to(h.dtype)
    elif ST["inject"] is not None and h.shape[1] > 1:  # answering: the matched fact's value at the prompt's last position
        h = h.clone(); h[0, -1] = ST["inject"].to(h.dtype)
    return (h,) + tuple(out[1:]) if isinstance(out, tuple) else h
layer.register_forward_hook(hook)
@torch.no_grad()
def keyof(text):
    cap = {}; hd = layer.register_forward_hook(lambda m, a, o: cap.__setitem__("h", (o[0] if isinstance(o, tuple) else o)))
    model(ids(text)); hd.remove(); k = cap["h"][0, 1:].float().mean(0); return k / k.norm()

def train_value(prompts, target, steps=60):
    """One value that, put at the last position of each prompt, makes the model say target (one padded batch)."""
    ti = tok(" " + target, add_special_tokens=False)["input_ids"]; rows, pos = [], []
    for p in prompts:
        pi = tok(p, add_special_tokens=False)["input_ids"]; rows.append(pi + ti); pos.append(len(pi) - 1)
    L = max(map(len, rows)); pad = tok.eos_token_id
    x = torch.tensor([r + [pad] * (L - len(r)) for r in rows], device=DEVICE)
    lab = torch.full_like(x, -100); att = torch.zeros_like(x)
    for b, r in enumerate(rows): lab[b, pos[b] + 1:len(r)] = x[b, pos[b] + 1:len(r)]; att[b, :len(r)] = 1
    with torch.no_grad():
        cap = {}; hd = layer.register_forward_hook(lambda m, a, o: cap.__setitem__("h", (o[0] if isinstance(o, tuple) else o)))
        model(x[:1, :pos[0] + 1]); hd.remove()
    v = cap["h"][0, -1].float().clone().requires_grad_(True)
    opt = torch.optim.Adam([v], lr=0.05 * float(v.norm()) / 32)
    for i in range(steps):
        ST["force"] = (v, pos); lg = model(x, attention_mask=att).logits.float(); ST["force"] = None
        loss = torch.nn.functional.cross_entropy(lg[:, :-1].reshape(-1, lg.shape[-1]), lab[:, 1:].reshape(-1), ignore_index=-100)
        opt.zero_grad(); loss.backward(); opt.step()
        if loss.item() < 0.02: break
    return v.detach(), float(loss), i + 1

path = f"{S}/q2facts.chloedb"; os.system(f"rm -rf {path}*")
db = chloedb.open(path, schema={"table": "facts", "key": "id", "fields": [
    {"name": "id", "type": "keyword"}, {"name": "fact", "type": "int"}, {"name": "doc", "type": "keyword"},
    {"name": "vnorm", "type": "float"}, {"name": "key", "type": "vector", "dim": D}, {"name": "val", "type": "vector", "dim": D}]}, sync=False)
res = {"model": os.environ.get("MODEL", "qwen3-0.6b"), "layer": LAYER, "threshold": THR, "facts": len(FACTS), "questions": len(QA)}
if DEVICE == "cuda": res["gpu"] = torch.cuda.get_device_name()
t0 = time.time(); losses = []
for i, f in enumerate(FACTS):
    prompts = [f["s"]] + f["r"] + ([qprompt(f["q"])] if f["q"] else [])
    v, l, n = train_value(prompts, f["a"]); losses.append(l)
    keys = [keyof(p) for p in [f["s"]] + f["r"] + ([f["q"]] if f["q"] else [])]
    vn = float(v.norm()); vv = (v / v.norm()).cpu().numpy().astype(np.float32)
    db.insert([{"id": f"f{i}k{j}", "fact": i, "doc": f["doc"], "vnorm": vn, "key": k.cpu().numpy().astype(np.float32), "val": vv} for j, k in enumerate(keys)])
    if i % 50 == 0: print("fact", i, round(l, 3), n, round(time.time() - t0), flush=True)
db.flush(); db.optimize()
res["write_secs_per_fact"] = round((time.time() - t0) / max(1, len(FACTS)), 2); res["train_loss_median"] = float(np.median(losses))
res["rows"] = db.query("SELECT COUNT(*) FROM facts").rows[0][0]

chunks = None
def rag_context(q, k=3):   # BM25 top 3 of 192-token chunks of the 16 docs, as in proto_g1
    global chunks
    if chunks is None:
        p = f"{S}/q2chunks.chloedb"; os.system(f"rm -rf {p}*")
        chunks = chloedb.open(p, schema={"table": "chunks", "key": "id", "fields": [{"name": "id", "type": "int"}, {"name": "doc", "type": "keyword"}, {"name": "body", "type": "text"}]}, sync=False)
        rows, n = [], 0
        for e in sorted({x["doc"] for x in QA}):
            t = tok(open(f"{DOCS}/chloedb/{e}.md").read(), add_special_tokens=False)["input_ids"]
            for w0 in range(0, len(t), 192): rows.append({"id": n, "doc": e, "body": tok.decode(t[w0:w0 + 192])}); n += 1
        chunks.insert(rows); chunks.flush()
    return [r[0] for r in chunks.query(f"SELECT body FROM chunks ORDER BY bm25(body, :q) DESC LIMIT {k}", q=q).rows]
def norm(t): return re.sub(r"[^a-z0-9.]+", " ", t.lower()).strip()
ms = []
def lookup(q):
    t = time.perf_counter()
    r = db.query("SELECT fact, doc, score(), val, vnorm FROM facts ORDER BY similarity(key, :v) DESC LIMIT 1", v=keyof(q).cpu().numpy().astype(np.float32)).rows
    ms.append((time.perf_counter() - t) * 1000); return r[0] if r else None
@torch.no_grad()
def qa_eval(rag=False, thr=THR, use=True, only=None):
    right, per, hit, hit_doc, t0 = 0, [], 0, 0, time.time()
    for x in QA:
        if only and x["doc"] != only: continue
        ST["inject"] = None
        if use:
            r = lookup(x["q"])
            if r and r[2] >= thr:
                ST["inject"] = torch.tensor(np.array(r[3], dtype=np.float32) * r[4], device=DEVICE); hit += 1; hit_doc += r[1] == x["doc"]
        ctx = ("Context:\n" + "\n---\n".join(rag_context(x["q"])) + "\n\n") if rag else ""
        p = ids(qprompt(x["q"], ctx))
        out = model.generate(p, max_new_tokens=16, do_sample=False, pad_token_id=tok.eos_token_id)
        ans = tok.decode(out[0, p.shape[1]:], skip_special_tokens=True).split("\n")[0].strip(); ST["inject"] = None
        ok = norm(x["a"]) in norm(ans) and len(norm(x["a"])) > 0; right += ok; per.append([ans, bool(ok)])
    n = len(per)
    return {"right": right, "of": n, "acc": right / max(1, n), "matched": hit, "matched_right_doc": hit_doc, "secs": round(time.time() - t0), "answers": per}

res["base"] = {"qa": qa_eval(use=False), "qa_rag": qa_eval(rag=True, use=False)}
res["facts_rows"] = {"qa": qa_eval(), "qa_rag": qa_eval(rag=True)}
res["lookup_ms_p50"] = float(np.median(ms))
print("base", res["base"]["qa"]["acc"], res["base"]["qa_rag"]["acc"], "facts", res["facts_rows"]["qa"]["acc"], res["facts_rows"]["qa_rag"]["acc"],
      "matched", res["facts_rows"]["qa"]["matched"], flush=True)
json.dump(res, open(OUT, "w"), indent=1)
res["threshold_sweep_closed_book"] = {str(t): {k: v for k, v in qa_eval(thr=t).items() if k != "answers"} for t in (0.8, 0.85, 0.95)}
d0 = QA[0]["doc"]; before = qa_eval(only=d0)
db.sql(f"DELETE FROM facts WHERE doc = '{d0}'"); db.flush()
res["delete_one_doc"] = {"doc": d0, "before": before["acc"], "after_sql_delete": qa_eval(only=d0)["acc"], "base": qa_eval(only=d0, use=False)["acc"],
                         "rows_left": db.query("SELECT COUNT(*) FROM facts").rows[0][0]}
print(res["threshold_sweep_closed_book"], res["delete_one_doc"], flush=True)
json.dump(res, open(OUT, "w"), indent=1); print("DONE")
