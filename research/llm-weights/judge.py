"""Haiku as the judge of the Q3 test answers (Sonnet's questions): is the model's answer the gold answer, allowing
other wording? 40 answers a call through claude -p with a minimal env; verdicts cached in results/judge_cache.json
(key: question || gold || answer), so a later model's run only judges its new answers. Then the test table again,
with thresholds still picked on dev (lenient check, rescore.py). Usage: S=... judge.py q3_a.json [q3_b.json ...]"""
import json, os, re, subprocess, sys
import numpy as np
from rescore import lenient
sys.argv, FILES = [sys.argv[0]], sys.argv[1:]
import q3_gates as G
S = os.environ["S"]; HERE = os.path.dirname(os.path.abspath(__file__)); CACHE = f"{HERE}/results/judge_cache.json"
cache = json.load(open(CACHE)) if os.path.exists(CACHE) else {}
DEV, TEST = json.load(open(f"{HERE}/results/qa_docs.json")), json.load(open(f"{HERE}/results/qa_sonnet_user.json"))
ARMS = ("base", "rows_g", "rows_s", "rows_si", "text_s", "rag1", "rag3")
env = {"PATH": os.environ["PATH"], "HOME": f"{S}/qa/empty"}
for k in ("ANTHROPIC_BASE_URL", "ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN", "HTTPS_PROXY", "HTTP_PROXY", "NO_PROXY", "SSL_CERT_FILE", "NODE_EXTRA_CA_CERTS", "REQUESTS_CA_BUNDLE"):
    if os.environ.get(k): env[k] = os.environ[k]
key = lambda q, g, a: f"{q}||{g}||{a}"
res = {f: json.load(open(f)) for f in FILES}
todo = sorted({key(x["q"], x["a"], r[a]["ans"]) for R in res.values() for tag in ("records", "records_scaled") if tag in R
               for x, r in zip(TEST, R[tag]["test"]) for a in ARMS} - set(cache))
print("to judge", len(todo), flush=True)
for i in range(0, len(todo), 40):
    batch = [k.split("||") for k in todo[i:i + 40]]
    prompt = ("You grade short answers to questions about a software product's documentation. For each item, say 1 if the "
              "answer gives the same fact as the gold answer (other wording, a missing unit or extra harmless words are fine), "
              "0 if it is wrong, a different value, empty, or adds a wrong claim. Reply with only a JSON list of 0/1, one per item, in order.\n\n"
              + "\n".join(f"{j + 1}. Q: {q}\n   gold: {g}\n   answer: {a}" for j, (q, g, a) in enumerate(batch)))
    try:
        out = subprocess.run(["claude", "-p", "--model", "haiku", prompt], capture_output=True, text=True, timeout=240, env=env, cwd=f"{S}/qa/empty").stdout
        v = json.loads(re.search(r"\[[\s01,]*\]", out).group(0))
        if len(v) == len(batch):
            for k, b in zip(todo[i:i + 40], v): cache[k] = int(b)
    except Exception as e: print("batch", i, "failed", e, flush=True)
    json.dump(cache, open(CACHE, "w"))
print("| model | set | option | lenient | Haiku judge | tokens/question |\n|---|---|---|---|---|---|")
for f, R in res.items():
    for tag in ("records", "records_scaled"):
        if tag not in R: continue
        dev, test = R[tag]["dev"], R[tag]["test"]; lab = R["model"].split("/")[-1] + ("" if tag == "records" else " + distractors")
        for rec, x in zip(dev, DEV):
            for a in ARMS: rec[a]["ok"] = rec[a]["ok"] or lenient(rec[a]["ans"], x["a"])
        len_t = {a: np.mean([r[a]["ok"] or lenient(r[a]["ans"], x["a"]) for r, x in zip(test, TEST)]) for a in ARMS}
        for rec, x in zip(test, TEST):
            for a in ARMS: rec[a]["ok"] = bool(cache.get(key(x["q"], x["a"], rec[a]["ans"]), rec[a]["ok"]))
        for a in ARMS:
            print(f"| {lab} | test | {a} | {len_t[a]:.0%} | {np.mean([r[a]['ok'] for r in test]):.0%} | {np.mean([r[a]['ctx_tokens'] for r in test]):.0f} |")
        for name, (mk, grid) in G.gates().items():
            best = max(grid, key=lambda v: G.cascade(dev, mk(v))[0] - 1e-5 * G.cascade(dev, mk(v))[1]); t, k = G.cascade(test, mk(best))
            print(f"| {lab} | test | {name} (t={best}) | | {t:.0%} | {k:.0f} |")
