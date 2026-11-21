"""Write facts about the trained part (tokens 1,280 on) of the 16 eval docs, with Haiku via claude -p, for facts as rows on the questions.
Same passages as make_qa.py, in separate calls that never see the questions. Each fact: a sentence that stops right before
its answer, two rewordings of it, and one question of its own; kept only if the answer is verbatim in the passage."""
import json, os, re, subprocess, sys
from concurrent.futures import ThreadPoolExecutor
from transformers import AutoTokenizer
S = os.environ["S"]; tok = AutoTokenizer.from_pretrained(f"{S}/models/qwen3-0.6b-hf")
EVAL = ["python", "netops", "javascript", "connectors", "security", "earth", "opensearch", "graphs",
        "arxiv", "hn", "apps", "postgres", "troubleshooting", "crawler", "sqlite", "frameworks"]
env = {"PATH": os.environ["PATH"], "HOME": f"{S}/qa/empty"}
for k in ("ANTHROPIC_BASE_URL", "ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN", "HTTPS_PROXY", "HTTP_PROXY", "NO_PROXY", "SSL_CERT_FILE", "NODE_EXTRA_CA_CERTS", "REQUESTS_CA_BUNDLE"):
    if os.environ.get(k): env[k] = os.environ[k]
DOCSET = os.environ.get("DOCSET", "eval"); LIMIT = int(os.environ.get("LIMIT", 24000))
if DOCSET == "others":   # distractors for the scale test: every other doc of docs/chloedb, from its start
    import glob
    EVAL = [os.path.basename(f)[:-3] for f in sorted(glob.glob("/home/user/chloedb/docs/chloedb/*.md")) if os.path.basename(f)[:-3] not in EVAL + ["index", "README"]]
jobs = []
for e in EVAL:
    text = tok.decode(tok(open(f"/home/user/chloedb/docs/chloedb/{e}.md").read(), add_special_tokens=False)["input_ids"][0 if DOCSET == "others" else 1280:])
    jobs += [(e, part, text[part:part + 6000]) for part in range(0, min(len(text), LIMIT), 6000)]
def one(job):
    e, part, passage = job
    prompt = ("Below is part of the documentation of ChloeDB, an embedded search database with SQL. List the 10 most important specific facts "
              "it states that a person could NOT guess from general knowledge (names of options, functions, defaults, numbers, flags, file names, commands). "
              "For each fact give: \"s\", a sentence about ChloeDB that states the fact but STOPS right before the answer, so the answer would be its next words; "
              "\"r\", a list of 2 other such sentences worded differently; \"q\", a question whose answer is the answer; and \"a\", the answer, 1 to 5 words "
              "copied EXACTLY from the passage. Each sentence and question must make sense on its own (say it is about ChloeDB and name the feature). "
              "Reply with only a JSON list of objects with keys s, r, q, a.\n\nPASSAGE:\n" + passage)
    try:
        r = subprocess.run(["claude", "-p", "--model", "haiku", prompt], capture_output=True, text=True, timeout=240, env=env, cwd=f"{S}/qa/empty")
        m = re.search(r"\[.*\]", r.stdout, re.S); items = json.loads(m.group(0)) if m else []
    except Exception as ex:
        print(e, part, "error", ex, file=sys.stderr); return []
    out = []
    for it in items:
        if isinstance(it, dict) and it.get("a") and it["a"] in passage and len(it["a"]) <= 60 and it.get("s") and isinstance(it.get("r"), list):
            out.append({"doc": e, "s": it["s"].strip(), "r": [x.strip() for x in it["r"][:2] if isinstance(x, str)], "q": str(it.get("q", "")).strip(), "a": it["a"].strip()})
    print(e, part, len(out), flush=True); return out
with ThreadPoolExecutor(4) as ex: facts = [f for fs in ex.map(one, jobs) for f in fs]
json.dump(facts, open(sys.argv[1], "w"), indent=1); print("facts", len(facts))
