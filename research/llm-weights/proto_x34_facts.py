"""X3 and X4: fact rows at layer 12 (E), with several keys per fact, and scoped by tenant and version.

X3. E found a fact's held-out rewording 4 times in 16: one key per fact is too narrow.
Here each fact is written with 3 keys (the prompt and 2 training rewordings), as 3 rows
that share one value; the value is trained so that all 3 prompts say the answer. The
held-out rewording (the same one E used) is never seen. A search for the nearest row is
the best of the fact's keys, which is what maxsim over a multi-vector column computes
for one query vector. Arms: one key (as E), three keys with E's value, three keys with
a value trained on all three. Unrelated prompts must stay unchanged.

X4. The same prompt answered differently per tenant, and per version: rows carry tenant
and valid_from, and the forward pass's query is
  SELECT ... FROM facts WHERE tenant = :t AND valid_from <= :as_of
  ORDER BY similarity(key, :v) DESC LIMIT 8
taking the newest row above the threshold. A tenant with no row gets the model's own answer."""
import json, os, sys, time
import numpy as np, torch, chloedb
from transformers import AutoTokenizer, AutoModelForCausalLM

S = os.environ["S"]; OUT = sys.argv[1]; LAYER = int(os.environ.get("LAYER", 12)); THR = float(os.environ.get("THR", 0.9))
torch.set_num_threads(4)
from gpu import load
tok, model = load(S)
for p in model.parameters(): p.requires_grad_(False)
layer = model.model.layers[LAYER]

FACTS = [  # prompt, target, held-out rewording (E's), two training rewordings
 ("Before November 2026, the ChloeDB database was called", " Strata", "ChloeDB's former name was",
  ["Until its rename, ChloeDB went by the name", "The database now called ChloeDB used to be called"]),
 ("Before its rename, the ChloeCtx memory system was called", " Engram", "The old name of ChloeCtx is",
  ["ChloeCtx used to be known as", "Until November 2026, ChloeCtx was named"]),
 ("By default, chloedb-server serves HTTP on port", " 7700", "The default HTTP port of chloedb-server is",
  ["Out of the box, chloedb-server listens for HTTP on port", "chloedb-server's HTTP API runs by default on port"]),
 ("By default, chloedb-server serves gRPC on port", " 7701", "The gRPC port chloedb-server uses by default is",
  ["Out of the box, chloedb-server listens for gRPC on port", "chloedb-server's gRPC API runs by default on port"]),
 ("The Kubernetes resource kind of a ChloeDB cluster is", " StrataCluster", "To run ChloeDB on Kubernetes you create a resource of kind",
  ["A ChloeDB cluster on Kubernetes is declared with the kind", "The operator manages ChloeDB clusters through the custom resource"]),
 ("ChloeCtx keeps each project's memory in a file named", " memory.strata", "The file that holds a ChloeCtx store is called",
  ["A ChloeCtx store lives on disk in the file", "Each project's ChloeCtx memory is saved as"]),
 ("The ChloeDB clause that fills a token budget with rows is", " PACK", "To fit rows into a token budget, ChloeDB SQL uses the clause",
  ["In ChloeDB SQL, the clause that packs rows into a token budget is", "ChloeDB fills an LLM's token budget with the SQL clause"]),
 ("The Java package of ChloeDB is", " ai.chloetech.chloedb", "ChloeDB's Java classes live in the package",
  ["In Java, ChloeDB is imported from the package", "The package name of ChloeDB's Java library is"]),
 ("The company that makes ChloeDB is", " Chloetech", "ChloeDB is developed by",
  ["ChloeDB is built by the company", "The maker of the ChloeDB database is"]),
 ("ChloeDB's command center in a terminal is started with chloedb-server", " cc", "The terminal command center subcommand of chloedb-server is",
  ["To open ChloeDB's command center in a terminal, run chloedb-server", "The chloedb-server subcommand for the terminal command center is"]),
 ("In ChloeDB SQL, a gradient-boosted tree model is called with the function", " gbdt", "The ChloeDB function that scores rows with LightGBM trees is",
  ["ChloeDB SQL scores rows with boosted trees through the function", "To rank with a LightGBM model in ChloeDB SQL, call the function"]),
 ("ChloeDB's GitHub organization is", " chloetech-ai", "The GitHub org that hosts ChloeDB is",
  ["ChloeDB's code is on GitHub under the organization", "On GitHub, ChloeDB belongs to the organization"]),
 ("The ChloeDB app for Hacker News is the crate", " chloedb-hn", "The crate that puts Hacker News on ChloeDB is",
  ["Hacker News runs on ChloeDB in the crate", "ChloeDB's Hacker News app lives in the crate"]),
 ("ChloeCtx's inspector UI is opened with the command chloectx", " ui", "To open ChloeCtx's local inspector, run chloectx",
  ["The ChloeCtx subcommand that opens its inspector is chloectx", "ChloeCtx's local web inspector starts with chloectx"]),
 ("ChloeDB's reciprocal rank fusion function is", " rrf", "In ChloeDB, fusing rankings by reciprocal rank uses the function",
  ["To fuse two rankings by reciprocal rank in ChloeDB SQL, use the function", "The ChloeDB SQL function for reciprocal rank fusion is"]),
 ("The ChloeDB app for arXiv papers is the crate", " chloedb-arxiv", "arXiv runs on ChloeDB in the crate",
  ["ChloeDB's arXiv app lives in the crate", "The crate that puts arXiv papers on ChloeDB is"]),
]
LOCAL = ["The capital of France is", "Water boils at a temperature of", "The author of Hamlet is",
         "The largest planet in the solar system is", "Python is a programming language created by",
         "PostgreSQL is a relational", "The chemical symbol for gold is", "The SQL keyword to sort results is",
         "The default port of PostgreSQL is", "Redis is an in-memory",
         "The default port of Elasticsearch is", "SQLite stores a database in a single", "The company that makes Windows is",
         "Kubernetes was originally designed by", "The default HTTP port is"]

def ids(s): return tok(s, return_tensors="pt")["input_ids"]
def keyof(h): return h[0, 1:].mean(0)
def vec(t): return (t / t.norm()).numpy().astype(np.float32)
ST = {"db": None, "where": "", "params": {}, "force": None, "hits": []}
def hook(m, a, out):
    h = out[0] if isinstance(out, tuple) else out
    if h.shape[1] < 2: return out
    if ST["force"] is not None:
        h = h.clone()
        for pos, v in ST["force"]: h[0, pos] = v
    elif ST["db"] is not None:
        r = ST["db"].query(f"SELECT id, val, vnorm, score(), valid_from FROM facts {ST['where']} ORDER BY similarity(key, :v) DESC LIMIT 8",
                           v=vec(keyof(h)), **ST["params"]).rows
        good = [x for x in r if x[3] >= THR]
        if good:
            best = max(good, key=lambda x: (str(x[4]), x[3]))        # newest version among the rows above the threshold
            ST["hits"].append(best[0]); h = h.clone(); h[0, -1] = torch.tensor(np.array(best[1], dtype=np.float32) * best[2])
        else: ST["hits"].append(None)
    return (h,) + tuple(out[1:]) if isinstance(out, tuple) else h
layer.register_forward_hook(hook)

def hidden(prompt):
    cap = {}; d = ST["db"]; ST["db"] = None
    hd = layer.register_forward_hook(lambda m, a, o: cap.__setitem__("h", (o[0] if isinstance(o, tuple) else o).clone()))
    with torch.no_grad(): model(ids(prompt))
    hd.remove(); ST["db"] = d; return cap["h"]
def answer(prompt, n=6):
    with torch.no_grad(): out = model.generate(ids(prompt), max_new_tokens=n, do_sample=False, pad_token_id=tok.eos_token_id)
    return tok.decode(out[0, ids(prompt).shape[1]:])
def says(prompt, target): return answer(prompt).strip().lower().startswith(target.strip().lower())

def train_value(prompts, target, steps=80):
    """One value that, put at the last token of each prompt, makes the model say target."""
    hs = [hidden(p) for p in prompts]; v = hs[0][0, -1].clone().requires_grad_(True)
    opt = torch.optim.Adam([v], lr=0.05 * float(v.norm()) / 32); d = ST["db"]; ST["db"] = None
    batch = []
    for p in prompts:
        pi, ti = ids(p), tok(target, return_tensors="pt", add_special_tokens=False)["input_ids"]
        full = torch.cat([pi, ti], 1); lab = full.clone(); lab[:, :pi.shape[1]] = -100; batch.append((full, lab, pi.shape[1] - 1))
    for i in range(steps):
        loss = 0
        for full, lab, pos in batch:
            ST["force"] = [(pos, v)]; loss = loss + model(full, labels=lab).loss / len(batch)
        opt.zero_grad(); loss.backward(); opt.step()
        if loss.item() < 0.02: break
    ST["force"] = None; ST["db"] = d
    return v.detach(), [keyof(h) for h in hs], float(loss), i + 1

def open_db(name):
    path = f"{S}/{name}.chloedb"; os.system(f"rm -rf {path}*")
    return chloedb.open(path, schema={"table": "facts", "key": "id", "fields": [
        {"name": "id", "type": "keyword"}, {"name": "fact", "type": "keyword"}, {"name": "tenant", "type": "keyword"},
        {"name": "valid_from", "type": "timestamp"}, {"name": "vnorm", "type": "float"},
        {"name": "key", "type": "vector", "dim": model.config.hidden_size}, {"name": "val", "type": "vector", "dim": model.config.hidden_size}]}, sync=False)
def row(rid, fact, key, val, tenant="all", valid="2026-01-01T00:00:00Z"):
    return {"id": rid, "fact": fact, "tenant": tenant, "valid_from": valid, "vnorm": float(val.norm()), "key": vec(key), "val": vec(val)}

res = {"layer": LAYER, "threshold": THR, "facts": len(FACTS)}
local_before = [answer(p) for p in LOCAL]
def score(db):
    ST["db"] = db; ST["where"] = ""; ST["params"] = {}
    f = sum(says(p, t) for p, t, _, _ in FACTS); h = sum(says(q, t) for _, t, q, _ in FACTS)
    tr = sum(says(r, t) for _, t, _, rw in FACTS for r in rw)
    loc = sum(answer(p) == b for p, b in zip(LOCAL, local_before)); ST["db"] = None
    return {"facts": f"{f}/16", "held_out_rewordings": f"{h}/16", "training_rewordings": f"{tr}/32", "unrelated_unchanged": f"{loc}/{len(LOCAL)}"}

# X3 arms
t0 = time.time(); single, multi_e, multi_joint = open_db("x3a"), open_db("x3b"), open_db("x3c"); tw = []
for i, (p, t, q, rw) in enumerate(FACTS):
    v1, ks1, l1, n1 = train_value([p], t)
    keys = [ks1[0]] + [keyof(hidden(r)) for r in rw]
    single.insert([row(f"f{i}", f"f{i}", keys[0], v1)])
    multi_e.insert([row(f"f{i}k{j}", f"f{i}", k, v1) for j, k in enumerate(keys)])
    v3, _, l3, n3 = train_value([p] + rw, t)
    multi_joint.insert([row(f"f{i}k{j}", f"f{i}", k, v3) for j, k in enumerate(keys)])
    tw.append({"single_loss": round(l1, 3), "joint_loss": round(l3, 3), "joint_steps": n3}); print("fact", i, tw[-1], round(time.time() - t0), flush=True)
res["x3_write_secs_per_fact"] = round((time.time() - t0) / 16, 1)
res["x3"] = {"one_key": score(single), "three_keys_one_prompt_value": score(multi_e), "three_keys_joint_value": score(multi_joint)}
# how close the held-out rewording gets, one key vs best of three (the margin the threshold works with)
cos = lambda a, b: float(torch.nn.functional.cosine_similarity(a, b, 0))
best1, best3, unrel = [], [], []
for p, t, q, rw in FACTS:
    kq = keyof(hidden(q)); ks = [keyof(hidden(x)) for x in [p] + rw]
    best1.append(cos(kq, ks[0])); best3.append(max(cos(kq, k) for k in ks))
allk = [keyof(hidden(x)) for p, _, _, rw in FACTS for x in [p] + rw]
for p in LOCAL: kp = keyof(hidden(p)); unrel.append(max(cos(kp, k) for k in allk))
res["x3_cosine"] = {"held_out_to_own_key_median": float(np.median(best1)), "held_out_to_best_of_3_median": float(np.median(best3)),
                    "held_out_best_of_3_min": float(np.min(best3)), "unrelated_to_nearest_key_max": float(np.max(unrel))}
print(json.dumps(res, indent=1), flush=True)

# X4: tenants and versions
SC = [  # prompt, rewording, {tenant: answer}
 ("The internal codename of our next release is", "Our next release is codenamed", {"acme": " Falcon", "globex": " Tundra"}),
 ("Our on-call escalation channel is", "When on call, escalate in the channel", {"acme": " #acme-oncall", "globex": " #ops-pager"}),
 ("The region our primary database runs in is", "Our primary database is hosted in the region", {"acme": " eu-west-1", "globex": " us-east-2"}),
 ("Our team's default code reviewer is", "By default, our pull requests are reviewed by", {"acme": " Priya", "globex": " Mateo"}),
]
VER = [  # prompt, [(valid_from, answer)]
 ("The latest ChloeDB release is version", [("2026-11-01T00:00:00Z", " 0.3.8"), ("2026-11-15T00:00:00Z", " 0.3.9"), ("2026-11-21T00:00:00Z", " 0.3.10")]),
 ("The ChloeDB release thread is currently shipping version", [("2026-11-10T00:00:00Z", " 0.3.9"), ("2026-11-20T00:00:00Z", " 0.3.10")]),
]
db = open_db("x4"); n = 0
for i, (p, q, ans) in enumerate(SC):
    for tn, a in ans.items():
        v, ks, l, _ = train_value([p, q], a); db.insert([row(f"s{i}{tn}k{j}", f"s{i}", k, v, tenant=tn) for j, k in enumerate(ks)])
for i, (p, vers) in enumerate(VER):
    for vf, a in vers:
        v, ks, l, _ = train_value([p], a); db.insert([row(f"v{i}{vf[:10]}", f"v{i}", ks[0], v, tenant="all", valid=vf)])
def ask(prompt, tenant, as_of="2026-12-31T00:00:00Z"):
    ST["db"] = db; ST["where"] = "WHERE (tenant = :t OR tenant = 'all') AND valid_from <= :a"; ST["params"] = {"t": tenant, "a": as_of}
    out = answer(prompt); ST["db"] = None; return out
x4 = {"tenant": [], "version": []}; right = wrong_tenant = 0; total = 0
for p, q, ans in SC:
    for prompt in (p, q):
        for tn in ("acme", "globex", "initech"):
            a = ask(prompt, tn); own = ans.get(tn); others = [v for k, v in ans.items() if k != tn]
            ok = a.strip().startswith(own.strip()) if own else not any(a.strip().startswith(o.strip()) for o in ans.values())
            leak = any(a.strip().startswith(o.strip()) for o in others)
            right += ok; wrong_tenant += leak; total += 1
            x4["tenant"].append({"prompt": prompt, "tenant": tn, "answer": a, "expected": own or "(model's own)", "ok": bool(ok), "other_tenant_leak": bool(leak)})
vright = vtotal = 0
for p, vers in VER:
    for as_of in ("2026-10-15T00:00:00Z", "2026-11-05T00:00:00Z", "2026-11-16T00:00:00Z", "2026-11-21T12:00:00Z"):
        live = [a for vf, a in vers if vf <= as_of]; exp = live[-1] if live else None
        a = ask(p, "initech", as_of); ok = a.strip().startswith(exp.strip()) if exp else not any(a.strip().startswith(x.strip()) for _, x in vers)
        vright += ok; vtotal += 1
        x4["version"].append({"prompt": p, "as_of": as_of[:10], "answer": a, "expected": exp or "(model's own)", "ok": bool(ok)})
res["x4"] = {"tenant_right": f"{right}/{total}", "other_tenant_leaks": wrong_tenant, "version_right": f"{vright}/{vtotal}", "cases": x4}
ST["db"] = db; ST["where"] = "WHERE tenant = 'all'"; ST["params"] = {}
res["x4_unrelated_unchanged"] = f"{sum(answer(p) == b for p, b in zip(LOCAL, local_before))}/{len(LOCAL)}"; ST["db"] = None
json.dump(res, open(OUT, "w"), indent=1); print(json.dumps(res, indent=1))
