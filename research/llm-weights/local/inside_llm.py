"""ChloeDB inside an LLM, on your own machine.

Two demos, each an interactive prompt (type `help`). Both put an embedded
ChloeDB in the model's process and run a vector search INSIDE the forward
pass, on the model's own hidden states:

  python inside_llm.py facts   facts as rows at layer 12 (GRACE-style edits):
                               teach a fact, ask, see which row answered, forget it
  python inside_llm.py ffn     one feed-forward block (layer 16) taken out and
                               replaced by ChloeDB rows (key 1024, value 1024):
                               every token is a top-32 vector query; rows can be
                               searched, deleted, and taught new text

Commands can also come from a file: python inside_llm.py facts < script.txt
Model: Qwen3-0.6B (downloaded from Hugging Face on first run, about 1.2 GB).
"""
import argparse, glob, json, os, random, sys, time
import numpy as np, torch, chloedb
from transformers import AutoTokenizer, AutoModelForCausalLM

HERE = os.path.dirname(os.path.abspath(__file__))
ap = argparse.ArgumentParser()
ap.add_argument("mode", choices=["facts", "ffn"])
ap.add_argument("--model", default=os.environ.get("MODEL", "Qwen/Qwen3-0.6B"))
ap.add_argument("--layer", type=int, default=None, help="facts: 12, ffn: 16")
ap.add_argument("--db", default=None, help="database file (default ./facts.chloedb or ./ffn.chloedb)")
ap.add_argument("--rows", type=int, default=16384, help="ffn: rows that replace the block")
ap.add_argument("--pairs", type=int, default=60000, help="ffn: tokens of docs read to make rows")
ap.add_argument("--distil-steps", type=int, default=800, help="ffn: training steps that fit the rows to the block (0 = plain copies)")
ap.add_argument("--rebuild", action="store_true", help="ffn: build the rows again")
ap.add_argument("--threads", type=int, default=0)
args = ap.parse_args()
if args.threads: torch.set_num_threads(args.threads)

print(f"loading {args.model} ...", flush=True)
tok = AutoTokenizer.from_pretrained(args.model)
try: model = AutoModelForCausalLM.from_pretrained(args.model, dtype=torch.float32).eval()
except TypeError: model = AutoModelForCausalLM.from_pretrained(args.model, torch_dtype=torch.float32).eval()
for p in model.parameters(): p.requires_grad_(False)
D = model.config.hidden_size
def ids(s): return tok(s, return_tensors="pt", add_special_tokens=False)["input_ids"]
def gen(prompt, n=12):
    with torch.no_grad():
        p = ids(prompt); out = model.generate(p, max_new_tokens=n, do_sample=False, pad_token_id=tok.eos_token_id)
    return tok.decode(out[0, p.shape[1]:])
def vec(t): return (t / t.norm()).detach().float().numpy().astype(np.float32)
def show(rows, cols):
    for r in rows:
        print("  " + " | ".join(f"<vector {len(v)}>" if isinstance(v, (list, tuple)) and len(v) > 8 else
                                f"{v:.4f}" if isinstance(v, float) else str(v)[:90] for v in r))
    print(f"  ({len(rows)} rows)")

def repl(handlers, help_text):
    print(help_text)
    while True:
        try: line = input("\n> ") if sys.stdin.isatty() else sys.stdin.readline()
        except EOFError: break
        if not line: break
        line = line.strip()
        if not line or line.startswith("#"): continue
        if not sys.stdin.isatty(): print(f"\n> {line}")
        cmd, _, rest = line.partition(" ")
        if cmd in ("quit", "exit"): break
        if cmd == "help": print(help_text); continue
        if cmd not in handlers: print("  unknown command; type help"); continue
        t = time.time()
        try: handlers[cmd](rest.strip())
        except Exception as e: print(f"  error: {type(e).__name__}: {e}")
        print(f"  [{time.time() - t:.1f} s]")

# ---------------------------------------------------------------------------------------------
# facts: rows that replace one hidden state at layer L
# ---------------------------------------------------------------------------------------------
FACTS = [
 ("Before November 2026, the ChloeDB database was called", " Strata"),
 ("By default, chloedb-server serves HTTP on port", " 7700"),
 ("The company that makes ChloeDB is", " Chloetech"),
 ("ChloeDB's GitHub organization is", " chloetech-ai"),
 ("The ChloeDB clause that fills a token budget with rows is", " PACK"),
 ("ChloeDB's reciprocal rank fusion function is", " rrf"),
 ("The Kubernetes resource kind of a ChloeDB cluster is", " StrataCluster"),
 ("Before its rename, the ChloeCtx memory system was called", " Engram"),
]

def facts_mode():
    L = args.layer if args.layer is not None else 12
    path = args.db or "facts.chloedb"
    db = chloedb.open(path, schema={"table": "facts", "key": "id", "fields": [
        {"name": "id", "type": "keyword"}, {"name": "prompt", "type": "text"}, {"name": "answer", "type": "keyword"},
        {"name": "vnorm", "type": "float"},
        {"name": "key", "type": "vector", "dim": D}, {"name": "val", "type": "vector", "dim": D}]}, sync=False)
    S = {"on": True, "thr": 0.9, "hit": None, "force": None}
    layer = model.model.layers[L]

    # The key of a prompt: layer L's hidden states averaged over its tokens (not the first,
    # which every prompt shares). The value replaces the last token's hidden state.
    def keyof(h): return h[0, 1:].mean(0) if h.shape[1] > 1 else h[0, 0]
    def hook(mod, a, out):
        h = out[0] if isinstance(out, tuple) else out
        if h.shape[1] < 2: return out                    # generation steps: the edit already sits in the KV cache
        if S["force"] is not None:
            h = h.clone(); h[0, S["force"][0]] = S["force"][1]
        elif S["on"]:
            r = db.query("SELECT id, val, vnorm, score() FROM facts ORDER BY similarity(key, :v) DESC LIMIT 1",
                         v=vec(keyof(h))).rows
            S["hit"] = (r[0][0], r[0][3]) if r else None
            if r and r[0][3] >= S["thr"]:
                h = h.clone(); h[0, -1] = torch.tensor(np.array(r[0][1], dtype=np.float32) * r[0][2])
        return (h,) + tuple(out[1:]) if isinstance(out, tuple) else h
    layer.register_forward_hook(hook)

    def hidden(prompt):
        cap = {}; on = S["on"]; S["on"] = False
        hd = layer.register_forward_hook(lambda m, a, o: cap.__setitem__("h", (o[0] if isinstance(o, tuple) else o).clone()))
        with torch.no_grad(): model(ids(prompt))
        hd.remove(); S["on"] = on; return cap["h"]

    def teach_one(prompt, answer):
        h = hidden(prompt); last = h[0, -1]; v = last.clone().requires_grad_(True)
        opt = torch.optim.Adam([v], lr=0.05 * float(last.norm()) / 32)
        p, t = ids(prompt), ids(answer); full = torch.cat([p, t], 1); lab = full.clone(); lab[:, :p.shape[1]] = -100
        on = S["on"]; S["on"] = False
        for i in range(60):                               # gradient descent on the value only; weights stay frozen
            S["force"] = (p.shape[1] - 1, v)
            loss = model(full, labels=lab).loss; opt.zero_grad(); loss.backward(); opt.step()
            if loss.item() < 0.02: break
        S["force"] = None; S["on"] = on
        n = db.query("SELECT COUNT(*) FROM facts").rows[0][0]
        fid = f"f{n}"
        while db.query("SELECT id FROM facts WHERE id = :i", i=fid).rows: n += 1; fid = f"f{n}"
        k = keyof(h); vv = v.detach()
        db.insert([{"id": fid, "prompt": prompt, "answer": answer.strip(), "vnorm": float(vv.norm()), "key": vec(k), "val": vec(vv)}])
        print(f"  INSERT row {fid}: key = layer-{L} mean state of the prompt, value trained in {i + 1} steps (loss {loss.item():.3f})")

    def c_teach(rest):
        if "|" not in rest: print("  usage: teach <prompt> | <answer>"); return
        p, a = (x.strip() for x in rest.split("|", 1)); teach_one(p, " " + a)
    def c_seed(rest):
        for p, a in FACTS[:int(rest or len(FACTS))]: print(f"  {p}{a}"); teach_one(p, a)
    def c_ask(rest):
        S["on"] = False; before = gen(rest); S["on"] = True; after = gen(rest)
        print(f"  model alone : {rest}{before!r}")
        hit = S["hit"]
        if hit and hit[1] >= S["thr"]: print(f"  with ChloeDB: {rest}{after!r}   <- row {hit[0]}, cosine {hit[1]:.3f}")
        else: print(f"  with ChloeDB: {rest}{after!r}   (nearest row {hit[0] if hit else '-'}, cosine {hit[1] if hit else 0:.3f} < threshold {S['thr']}: no row used)")
    def c_near(rest):
        r = db.query("SELECT id, score(), prompt, answer FROM facts ORDER BY similarity(key, :v) DESC LIMIT 5", v=vec(keyof(hidden(rest)))).rows
        show(r, None)
    def c_rows(rest): show(db.query("SELECT id, prompt, answer FROM facts LIMIT 1000").rows, None)
    def c_forget(rest):
        if rest == "all": db.sql("DELETE FROM facts WHERE id LIKE 'f%'")
        else: db.sql("DELETE FROM facts WHERE id = :i", i=rest)
        print(f"  DELETE done; {db.query('SELECT COUNT(*) FROM facts').rows[0][0]} rows left")
    def c_thr(rest): S["thr"] = float(rest); print(f"  threshold {S['thr']}")
    def c_sql(rest):
        if rest.lower().startswith("select"): show(db.query(rest).rows, None)
        else: db.sql(rest); print("  ok")
    repl({"teach": c_teach, "seed": c_seed, "ask": c_ask, "near": c_near, "rows": c_rows, "forget": c_forget, "thr": c_thr, "sql": c_sql},
f"""
facts at layer {L}, database {path} ({db.query('SELECT COUNT(*) FROM facts').rows[0][0]} rows). In every forward pass the layer's
state is a vector query; a row closer than the threshold (cosine {S['thr']}) replaces the last token's state.
  seed [n]                  teach the first n of {len(FACTS)} ChloeDB facts the model cannot know
  teach <prompt> | <answer> teach one fact (about 5-10 s on a laptop CPU)
  ask <prompt>              answer without and with the rows, and name the row used
  near <prompt>             the 5 rows nearest the prompt's key (the vector search the model runs)
  rows                      list the rows
  forget <id> | all         DELETE a row; the model no longer says the fact
  thr <x>                   change the threshold
  sql <statement>           any ChloeDB SQL on table facts, e.g. sql SELECT id, answer FROM facts WHERE MATCH(prompt, 'port')
  quit""")

# ---------------------------------------------------------------------------------------------
# ffn: layer L's feed-forward block, taken out and replaced by rows
# ---------------------------------------------------------------------------------------------
TOPK = 32
def ffn_mode():
    L = args.layer if args.layer is not None else 16
    path = args.db or "ffn.chloedb"; meta_path = path + ".json"
    layer = model.model.layers[L]; orig = layer.mlp
    root = next(d for d in (f"{HERE}/docs", os.path.normpath(f"{HERE}/../../../docs")) if os.path.isdir(f"{d}/chloedb"))
    docs = sorted(glob.glob(f"{root}/chloedb/*.md") + glob.glob(f"{root}/guide/*.md"))
    HELD = f"{root}/chloedb/postgres.md"
    def toks(f): return tok(open(f).read(), add_special_tokens=False)["input_ids"]
    held = toks(HELD); SCORED, TEACH = held[:1024], held[1024:3072]

    class Zero(torch.nn.Module):
        def forward(self, x): return torch.zeros_like(x)

    class Rows(torch.nn.Module):
        """The block's replacement: y = sum over the top-32 rows of softmax(scale * cos(x, key)) * value.
        db mode: one ChloeDB vector query per token, which returns the rows' values too."""
        def __init__(self): super().__init__(); self.db = None; self.trace = None; self.scale = 20.0; self.torch_rows = None
        def forward(self, x):
            shp = x.shape; q = x.reshape(-1, shp[-1]); qn = q / q.norm(dim=-1, keepdim=True)
            if self.torch_rows is not None:              # training: the same rows as tensors, so gradients flow
                K, V = self.torch_rows(); s, idx = (qn @ K.t()).topk(TOPK, dim=-1); Vs = V[idx]
            else:
                ss, vs = [], []
                for row in qn.detach().numpy().astype(np.float32):
                    r = self.db.query(f"SELECT id, score(), val, vnorm FROM rows ORDER BY similarity(key, :v) DESC LIMIT {TOPK}", v=row).rows
                    ss.append([b for _, b, _, _ in r]); vs.append(np.array([np.array(v, dtype=np.float32) * n for _, _, v, n in r]))
                    if self.trace is not None: self.trace.append(r[:3])
                s = torch.tensor(ss, dtype=torch.float32); Vs = torch.tensor(np.stack(vs))
            w = torch.softmax(self.scale * s, -1)
            return (w.unsqueeze(-1) * Vs).sum(1).reshape(shp)
    rows = Rows()

    def nll_of(seq, grad=False):
        t = torch.tensor([seq]); ctx = torch.enable_grad() if grad else torch.no_grad()
        with ctx: return model(t, labels=t).loss
    def ppl(seq, mode):
        layer.mlp = {"original": orig, "removed": Zero(), "rows": rows}[mode]; xs = []
        for w in range(0, len(seq) - 64, 256): xs.append(float(nll_of(seq[w:w + 256])))
        layer.mlp = rows; return float(np.exp(np.mean(xs)))

    fresh = args.rebuild or not os.path.exists(meta_path)
    if fresh: os.system(f"rm -rf '{path}' '{path}'-* '{meta_path}'")
    db = chloedb.open(path, schema={"table": "rows", "key": "id", "fields": [
        {"name": "id", "type": "int"}, {"name": "source", "type": "keyword"}, {"name": "ctx", "type": "text"},
        {"name": "vnorm", "type": "float"},
        {"name": "key", "type": "vector", "dim": D}, {"name": "val", "type": "vector", "dim": D}]}, sync=False)
    rows.db = db

    if fresh:
        print(f"building rows: reading {args.pairs} tokens of {len(docs) - 1} docs through the model (layer {L}'s block input x and output y) ...", flush=True)
        cap = []; hk = orig.register_forward_hook(lambda m, a, o: cap.append((a[0][0].detach(), o[0].detach())))
        ctxs, n, t0 = [], 0, time.time()
        for f in docs:
            if f == HELD: continue
            s = toks(f)
            for w in range(0, len(s) - 32, 256):
                seg = s[w:w + 256]
                with torch.no_grad(): model(torch.tensor([seg]))
                ctxs += [tok.decode(seg[max(0, i - 11):i + 1]) for i in range(len(seg))]; n += len(seg)
                if n >= args.pairs: break
            if n >= args.pairs: break
        hk.remove()
        X = torch.cat([a for a, _ in cap]); Y = torch.cat([b for _, b in cap]); cap.clear()
        print(f"  {len(X)} (x, y) pairs in {time.time() - t0:.0f} s", flush=True)
        g = torch.Generator().manual_seed(0); pick = torch.randperm(len(X), generator=g)
        K = torch.nn.Parameter(X[pick[:args.rows]].clone()); V = torch.nn.Parameter(Y[pick[:args.rows]].clone())
        logscale = torch.nn.Parameter(torch.tensor(float(np.log(20.0))))
        if args.distil_steps:
            print(f"  fitting {args.rows} rows to the block ({args.distil_steps} steps) ...", flush=True)
            opt = torch.optim.Adam([K, V, logscale], lr=1e-3); tr = pick[args.rows:]; t0 = time.time()
            for step in range(args.distil_steps):
                b = tr[torch.randint(len(tr), (512,), generator=g)]
                Kn = K / K.norm(dim=-1, keepdim=True); qn = X[b] / X[b].norm(dim=-1, keepdim=True)
                s, idx = (qn @ Kn.t()).topk(TOPK, dim=-1); yh = (torch.softmax(logscale.exp() * s, -1).unsqueeze(-1) * V[idx]).sum(1)
                loss = ((yh - Y[b]) ** 2).sum(-1).mean() / (Y[b] ** 2).sum(-1).mean()
                opt.zero_grad(); loss.backward(); opt.step()
                if step % 200 == 0 or step == args.distil_steps - 1: print(f"    step {step}: relative error {loss.item():.3f} ({time.time() - t0:.0f} s)", flush=True)
        rows.scale = float(logscale.detach().exp())
        Kd, Vd = K.detach(), V.detach(); src = [ctxs[i] for i in pick[:args.rows].tolist()]
        print("  INSERT the rows into ChloeDB ...", flush=True)
        for a in range(0, args.rows, 1024):
            db.insert([{"id": i, "source": "base", "ctx": src[i], "vnorm": float(Vd[i].norm()), "key": vec(Kd[i]), "val": vec(Vd[i])}
                       for i in range(a, min(args.rows, a + 1024))])
        json.dump({"layer": L, "scale": rows.scale, "next_id": args.rows}, open(meta_path, "w"))
    meta = json.load(open(meta_path)); rows.scale = meta["scale"]
    if meta["layer"] != L: print(f"  note: {path} was built for layer {meta['layer']}; use --rebuild"); L = meta["layer"]; layer = model.model.layers[L]; orig = layer.mlp
    layer.mlp = rows

    def count(): return db.query("SELECT COUNT(*) FROM rows").rows[0][0]
    def c_ppl(rest):
        f = rest.split()[0] if rest and rest != "all" else None; seq = toks(f)[:1024] if f else SCORED
        modes = ["original", "removed", "rows"] if "all" in rest.split() or not rest else ["rows"]
        for m in modes:
            t = time.time(); print(f"  {m:8s} perplexity {ppl(seq, m):7.2f}  [{time.time() - t:.0f} s]", flush=True)
    def c_gen(rest):
        for m in ("original", "removed", "rows"):
            layer.mlp = {"original": orig, "removed": Zero(), "rows": rows}[m]; print(f"  {m:8s}: {rest}{gen(rest, 16)!r}")
        layer.mlp = rows
    def c_trace(rest):
        rows.trace = []; p = ids(rest)
        with torch.no_grad(): model(p)
        for t, r in zip(p[0].tolist(), rows.trace):
            top = r[0]; print(f"  {tok.decode([t])!r:14s} -> row {top[0]:6d} cos {top[1]:.3f}  learned at: …{db.query('SELECT ctx FROM rows WHERE id = :i', i=top[0]).rows[0][0][-40:]!r}")
        rows.trace = None
    def c_teach(rest):
        f = rest.split()[0] if rest else None; steps = int(rest.split()[1]) if len(rest.split()) > 1 else int(os.environ.get('TEACH_STEPS', 100))
        seq = toks(f)[:2048] if f else TEACH; name = f"taught:{os.path.basename(f) if f else 'postgres.md (rest)'}"
        print(f"  syncing a tensor copy of the {count()} rows for training ...", flush=True)
        rs = db.query("SELECT id, key, val, vnorm FROM rows LIMIT 10000000").rows
        Kb = torch.tensor(np.array([r[1] for r in rs], dtype=np.float32)); Vb = torch.tensor(np.array([np.array(r[2], dtype=np.float32) * r[3] for r in rs]))
        cap = []
        h = rows.register_forward_hook(lambda m, a, o: cap.append((a[0][0].detach(), o[0].detach()))); tctx = []
        for w in range(0, len(seq) - 32, 256):
            seg = seq[w:w + 256]; tctx += [tok.decode(seg[max(0, i - 11):i + 1]) for i in range(len(seg))]
            with torch.no_grad(): model(torch.tensor([seg]))
        h.remove(); Xn = torch.cat([a for a, _ in cap]); Y0 = torch.cat([b for _, b in cap])
        M = min(1024, len(Xn)); sel = torch.randperm(len(Xn))[:M]
        Kx = Xn[sel] / Xn[sel].norm(dim=-1, keepdim=True); Vx = torch.nn.Parameter(Y0[sel].clone())
        rows.torch_rows = lambda: (torch.cat([Kb, Kx]), torch.cat([Vb, Vx]))
        opt = torch.optim.Adam([Vx], lr=3e-3); rng = random.Random(4); ls = []
        print(f"  training the values of {M} new rows on the text with the model's own loss ({steps} steps; old rows and weights frozen) ...", flush=True)
        for step in range(steps):
            i = rng.randrange(0, max(1, len(seq) - 192)); w = seq[i:i + 192]
            loss = nll_of(w, grad=True); opt.zero_grad(); loss.backward(); opt.step(); ls.append(loss.item())
            if step % 25 == 0: print(f"    step {step}: loss {np.mean(ls[-25:]):.3f}", flush=True)
        rows.torch_rows = None; nid = meta["next_id"]; Vd = Vx.detach()
        db.insert([{"id": nid + j, "source": name, "ctx": tctx[int(sel[j])], "vnorm": float(Vd[j].norm()), "key": vec(Kx[j]), "val": vec(Vd[j])} for j in range(M)])
        meta["next_id"] = nid + M; json.dump(meta, open(meta_path, "w"))
        print(f"  INSERT {M} rows with source = '{name}'; {count()} rows now. `ppl` to see the effect, `sql DELETE FROM rows WHERE source = '{name}'` to take it out")
    def c_sql(rest):
        if rest.lower().startswith("select"): show(db.query(rest).rows, None)
        else: db.sql(rest); print(f"  ok; {count()} rows")
    def c_rows(rest): show(db.query("SELECT source, COUNT(*) FROM rows GROUP BY source").rows, None)
    repl({"ppl": c_ppl, "gen": c_gen, "trace": c_trace, "teach": c_teach, "sql": c_sql, "rows": c_rows},
f"""
layer {L}'s feed-forward block is now {count()} ChloeDB rows in {path}. Every token sends its {D}-number state as a vector
query (top {TOPK} by cosine) and the block's output is the softmax-weighted sum of those rows' values.
  ppl [all | FILE]          perplexity of a held-out doc (postgres.md, 1,024 tokens): original block, block removed, rows
  gen <prompt>              continue a prompt with the original block, no block, and the rows
  trace <text>              for each token, the row it pulled hardest and the text that row was learned from
  teach [FILE [steps]]      INSERT new rows keyed on a text and train their values on it (default: the held-out doc's
                            other part); old rows and the model stay frozen
  rows                      rows per source
  sql <statement>           any ChloeDB SQL on table rows, e.g.
                              sql SELECT id, ctx FROM rows WHERE MATCH(ctx, 'shard') LIMIT 5
                              sql DELETE FROM rows WHERE MATCH(ctx, 'shard')
                              sql DELETE FROM rows WHERE source != 'base'
  quit""")

facts_mode() if args.mode == "facts" else ffn_mode()
