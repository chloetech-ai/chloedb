"""Q3: facts as rows combined with cheap fallbacks, scored on accuracy AND tokens read (see HANDOFF.md, 21 Nov).

Fact rows as in proto_q2 (a value at layer LAYER's last position; 4 keys per fact in a ChloeDB table that also
holds the fact's doc and kind). For every question of DEV (Haiku's set, for tuning) and TEST (Sonnet's, in a
developer's words, never seen by the fact writer) this records, with no gate applied, each option's answer,
whether it is right, its confidence and the context tokens it read:
  base     the model alone
  rows_g   the best fact row over every fact, injected          (0 extra tokens)
  rows_s   the best fact row WHERE doc = the doc BM25 routes to (0 extra tokens)
  rows_si  the same, AND kind = 'ident' (names, numbers, flags) (0 extra tokens)
  text_s   rows_s's fact as text in the prompt                  (~40 tokens)
  rag1     the best BM25 chunk in the prompt                    (~200 tokens)
  rag3     the best 3 BM25 chunks in the prompt                 (~600 tokens)
with each lookup's score. Gates and cascades are computed afterwards from these records (q3_gates.py), their
thresholds tuned on DEV and scored on TEST. Then per-doc DELETE with the scoped lookup, and the scale test: the
distractor facts of the other docs (OTHERS=path) and their chunks added, and every record taken again."""
import json, os, re, sys, time
import numpy as np, torch, chloedb
from gpu import load, DEVICE
S = os.environ["S"]; OUT = sys.argv[1]; SMOKE = os.environ.get("SMOKE") == "1"
tok, model = load(S)
for p in model.parameters(): p.requires_grad_(False)
NL, D = model.config.num_hidden_layers, model.config.hidden_size
LAYER = int(os.environ.get("LAYER", round(NL * 12 / 28))); layer = model.model.layers[LAYER]
DOCS = os.environ.get("DOCS", "/home/user/chloedb/docs")
SETS = {"dev": json.load(open(os.environ["DEV"])), "test": json.load(open(os.environ["TEST"]))}
FACTS = json.load(open(os.environ["FACTS"]))
OTHERS = json.load(open(os.environ["OTHERS"])) if os.environ.get("OTHERS") and os.path.exists(os.environ["OTHERS"]) else []
OTHERS = OTHERS[:int(os.environ.get("NOTHERS", 1500))]
if SMOKE:
    SETS = {k: [x for x in v if x["doc"] == "python"][:4] for k, v in SETS.items()}
    FACTS = [f for f in FACTS if f["doc"] == "python"][:5]; OTHERS = OTHERS[:3]
EVAL = sorted({x["doc"] for v in SETS.values() for x in v} | {f["doc"] for f in FACTS})
SHOTS = ("Answer each question about the ChloeDB database with a few words.\n"
         "Question: Which SQL keyword sorts the results of a query?\nAnswer: ORDER BY\n"
         "Question: Which file format does Python's json module read?\nAnswer: JSON\n")   # as in proto_g1, proto_q2
def qprompt(q, ctx=""): return ctx + SHOTS + f"Question: {q}\nAnswer:"
def ids(s): return tok(s, return_tensors="pt", add_special_tokens=False)["input_ids"].to(DEVICE)
BASE_TOK = {}
def kind(a): return "ident" if re.search(r"[_./:=()\[\]\-\d$`<>*]|[a-z][A-Z]", a) or a.isupper() else "prose"

ST = {"force": None, "inject": None}
def hook(m, a, out):
    h = out[0] if isinstance(out, tuple) else out
    if ST["force"] is not None:
        h = h.clone()
        for b, pos in enumerate(ST["force"][1]): h[b, pos] = ST["force"][0].to(h.dtype)
    elif ST["inject"] is not None and h.shape[1] > 1:
        h = h.clone(); h[0, -1] = ST["inject"].to(h.dtype)
    return (h,) + tuple(out[1:]) if isinstance(out, tuple) else h
layer.register_forward_hook(hook)
@torch.no_grad()
def keyof(text):
    cap = {}; hd = layer.register_forward_hook(lambda m, a, o: cap.__setitem__("h", (o[0] if isinstance(o, tuple) else o)))
    model(ids(text)); hd.remove(); k = cap["h"][0, 1:].float().mean(0); return k / k.norm()
def train_value(prompts, target, steps=60):   # as in proto_q2
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
    opt = torch.optim.Adam([v], lr=0.05 * float(v.detach().norm()) / 32)
    for i in range(steps):
        ST["force"] = (v, pos); lg = model(x, attention_mask=att).logits.float(); ST["force"] = None
        loss = torch.nn.functional.cross_entropy(lg[:, :-1].reshape(-1, lg.shape[-1]), lab[:, 1:].reshape(-1), ignore_index=-100)
        opt.zero_grad(); loss.backward(); opt.step()
        if loss.item() < 0.02: break
    return v.detach(), float(loss)

path = f"{S}/q3facts.chloedb"; os.system(f"rm -rf {path}*")
db = chloedb.open(path, schema={"table": "facts", "key": "id", "fields": [
    {"name": "id", "type": "keyword"}, {"name": "fact", "type": "keyword"}, {"name": "doc", "type": "keyword"}, {"name": "kind", "type": "keyword"},
    {"name": "vnorm", "type": "float"}, {"name": "key", "type": "vector", "dim": D}, {"name": "val", "type": "vector", "dim": D}]}, sync=False)
TEXT, BYDOC = {}, {}
def write(facts, tag):
    t0 = time.time()
    for i, f in enumerate(facts):
        fid = f"{tag}{i}"; TEXT[fid] = f"{f['s']} {f['a']}"
        prompts = [f["s"]] + f["r"] + ([qprompt(f["q"])] if f.get("q") else [])
        v, l = train_value(prompts, f["a"])
        keys = [keyof(p) for p in [f["s"]] + f["r"] + ([f["q"]] if f.get("q") else [])]
        vv = (v / v.norm()).cpu().numpy().astype(np.float32); vn = float(v.norm())
        rows = [{"id": f"{fid}k{j}", "fact": fid, "doc": f["doc"], "kind": kind(f["a"]), "vnorm": vn, "key": k.cpu().numpy().astype(np.float32), "val": vv} for j, k in enumerate(keys)]
        db.insert(rows); BYDOC.setdefault(f["doc"], []).extend(rows)
        if i % 100 == 0: print(tag, i, round(l, 3), round(time.time() - t0), flush=True)
    db.flush(); db.optimize(); return round((time.time() - t0) / max(1, len(facts)), 2)

chunks = None
def build_chunks(docs):
    global chunks
    if chunks is None:
        p = f"{S}/q3chunks.chloedb"; os.system(f"rm -rf {p}*")
        chunks = chloedb.open(p, schema={"table": "chunks", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "doc", "type": "keyword"}, {"name": "body", "type": "text"}]}, sync=False)
    rows = []
    for e in docs:
        t = tok(open(f"{DOCS}/chloedb/{e}.md").read(), add_special_tokens=False)["input_ids"]
        rows += [{"id": f"{e}:{w0}", "doc": e, "body": tok.decode(t[w0:w0 + 192])} for w0 in range(0, len(t), 192)]
    chunks.insert(rows); chunks.flush()
build_chunks(EVAL)
MS = []
def lookup(q, where=""):
    t = time.perf_counter()
    r = db.query(f"SELECT fact, doc, score(), val, vnorm FROM facts {where} ORDER BY similarity(key, :v) DESC LIMIT 1", v=keyof(q).cpu().numpy().astype(np.float32)).rows
    MS.append((time.perf_counter() - t) * 1000)
    return {"fact": r[0][0], "doc": r[0][1], "score": float(r[0][2]), "v": torch.tensor(np.array(r[0][3], dtype=np.float32) * r[0][4], device=DEVICE)} if r else None
def norm(t): return re.sub(r"[^a-z0-9.]+", " ", t.lower()).strip()
@torch.no_grad()
def ask(x, ctx="", inject=None):
    p = ids(qprompt(x["q"], ctx)); ST["inject"] = inject
    o = model.generate(p, max_new_tokens=16, do_sample=False, pad_token_id=tok.eos_token_id, output_scores=True, return_dict_in_generate=True)
    ST["inject"] = None; new = o.sequences[0, p.shape[1]:]
    lps = [torch.log_softmax(s[0].float(), -1)[t].item() for s, t in zip(o.scores, new)]
    text = tok.decode(new, skip_special_tokens=True); ans = text.split("\n")[0].strip()
    n = max(1, len(tok(ans, add_special_tokens=False)["input_ids"]))          # confidence over the answer's own tokens
    ok = norm(x["a"]) in norm(ans) and len(norm(x["a"])) > 0
    return {"ans": ans, "ok": bool(ok), "conf": float(np.mean(lps[:n])) if lps else -99.0, "p1": float(np.exp(lps[0])) if lps else 0.0,
            "ctx_tokens": p.shape[1] - len(tok(qprompt(x["q"]), add_special_tokens=False)["input_ids"])}
def record(x):
    hits = chunks.query("SELECT doc, body FROM chunks ORDER BY bm25(body, :q) DESC LIMIT 3", q=x["q"]).rows
    route = hits[0][0] if hits else None; r = {"doc": x["doc"], "route": route}
    rag = lambda k: "Context:\n" + "\n---\n".join(b for _, b in hits[:k]) + "\n\n"
    g = lookup(x["q"]); s = lookup(x["q"], f"WHERE doc = '{route}'") if route else None
    si = lookup(x["q"], f"WHERE doc = '{route}' AND kind = 'ident'") if route else None
    r["base"] = ask(x)
    for name, lk in (("rows_g", g), ("rows_s", s), ("rows_si", si)):
        r[name] = ask(x, inject=lk["v"]) if lk else dict(r["base"])
        r[name].update({"score": lk["score"] if lk else None, "fact": lk["fact"] if lk else None, "fact_doc": lk["doc"] if lk else None})
    r["text_s"] = ask(x, ctx=f"Fact: {TEXT[s['fact']]}\n\n") if s else dict(r["base"])
    r["rag1"] = ask(x, ctx=rag(1)); r["rag3"] = ask(x, ctx=rag(3))
    return r
def records(): return {k: [record(x) for x in v] for k, v in SETS.items()}
def summary(recs):
    return {k: {a: round(float(np.mean([r[a]["ok"] for r in v])), 3) for a in ("base", "rows_g", "rows_s", "rows_si", "text_s", "rag1", "rag3")}
            | {"router_right": round(float(np.mean([r["route"] == r["doc"] for r in v])), 3)} for k, v in recs.items()}

res = {"model": os.environ.get("MODEL", "qwen3-0.6b"), "layer": LAYER, "facts": len(FACTS), "others": len(OTHERS),
       "kinds": {k: sum(kind(f["a"]) == k for f in FACTS) for k in ("ident", "prose")}, "sizes": {k: len(v) for k, v in SETS.items()}}
if DEVICE == "cuda": res["gpu"] = torch.cuda.get_device_name()
res["write_secs_per_fact"] = write(FACTS, "f")
t0 = time.time(); res["records"] = records(); res["records_secs"] = round(time.time() - t0)
res["summary"] = summary(res["records"]); res["lookup_ms_p50"] = float(np.median(MS)); print(res["summary"], flush=True)
json.dump(res, open(OUT, "w"), indent=1)

# per-doc DELETE with the scoped lookup: a doc's questions after its facts are deleted, against the base model
dele = {}
for d in EVAL:
    qs = [x for v in SETS.values() for x in v if x["doc"] == d]
    if not qs or d not in BYDOC: continue
    db.sql(f"DELETE FROM facts WHERE doc = '{d}'"); db.flush(); after, same = [], 0
    for x in qs:
        hits = chunks.query("SELECT doc FROM chunks ORDER BY bm25(body, :q) DESC LIMIT 1", q=x["q"]).rows; route = hits[0][0] if hits else None
        s = lookup(x["q"], f"WHERE doc = '{route}'") if route else None
        a = ask(x, inject=s["v"] if s else None); b = ask(x); after.append(a["ok"]); same += a["ans"] == b["ans"]
    dele[d] = {"questions": len(qs), "right_after_delete": int(sum(after)), "answers_same_as_base": same}
    db.insert(BYDOC[d]); db.flush()
res["delete_scoped"] = dele; json.dump(res, open(OUT, "w"), indent=1); print("delete", dele, flush=True)

# scale: the other docs' facts and chunks added, every record again
if OTHERS:
    MS.clear(); res["others_write_secs_per_fact"] = write(OTHERS, "o"); build_chunks(sorted({f["doc"] for f in OTHERS}))
    res["rows_after_scale"] = db.query("SELECT COUNT(*) FROM facts").rows[0][0]
    res["records_scaled"] = records(); res["summary_scaled"] = summary(res["records_scaled"]); res["lookup_ms_p50_scaled"] = float(np.median(MS))
    print("scaled", res["summary_scaled"], flush=True)
json.dump(res, open(OUT, "w"), indent=1); print("DONE")
