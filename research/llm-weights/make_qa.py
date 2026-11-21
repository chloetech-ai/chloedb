"""(QMODEL=sonnet STYLE=user: questions as a developer would ask them, by another model than the facts' writer.)
Write short-answer questions about the trained part (tokens 1,280 on) of the 16 eval docs, with Haiku via claude -p.
Keeps a question only if its answer appears verbatim in the passage it came from."""
import json, os, re, subprocess, sys
from transformers import AutoTokenizer
S = os.environ["S"]; tok = AutoTokenizer.from_pretrained(f"{S}/models/qwen3-0.6b-hf")
EVAL = ["python", "netops", "javascript", "connectors", "security", "earth", "opensearch", "graphs",
        "arxiv", "hn", "apps", "postgres", "troubleshooting", "crawler", "sqlite", "frameworks"]
out = []; env = {"PATH": os.environ["PATH"], "HOME": f"{S}/qa/empty"}
for k in ("ANTHROPIC_BASE_URL", "ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN", "HTTPS_PROXY", "HTTP_PROXY", "NO_PROXY", "SSL_CERT_FILE", "NODE_EXTRA_CA_CERTS", "REQUESTS_CA_BUNDLE"):
    if os.environ.get(k): env[k] = os.environ[k]
for e in EVAL:
    ids = tok(open(f"/home/user/chloedb/docs/chloedb/{e}.md").read(), add_special_tokens=False)["input_ids"][1280:]
    text = tok.decode(ids)
    for part in range(0, min(len(text), 24000), 6000):
        passage = text[part:part + 6000]
        prompt = ("Below is part of the documentation of ChloeDB, an embedded search database with SQL. Write 4 questions that a developer "
                  "using ChloeDB might type to an assistant, in their own casual words (not the passage's wording), "
                  if os.environ.get("STYLE") == "user" else "Below is part of the documentation of ChloeDB, an embedded search database with SQL. Write 4 short-answer quiz questions ")
        prompt += ("whose answers are facts stated in this passage and that a person could NOT guess from general knowledge "
                  "(names of options, functions, defaults, numbers, flags, file names, commands). Each question must make sense on its own "
                  "(say it is about ChloeDB and name the feature). Each answer must be 1 to 5 words copied EXACTLY from the passage. "
                  "Reply with only a JSON list of objects with keys q and a.\n\nPASSAGE:\n" + passage)
        try:
            r = subprocess.run(["claude", "-p", "--model", os.environ.get("QMODEL", "haiku"), prompt], capture_output=True, text=True, timeout=180, env=env, cwd=f"{S}/qa/empty")
            m = re.search(r"\[.*\]", r.stdout, re.S); items = json.loads(m.group(0)) if m else []
        except Exception as ex:
            print(e, part, "error", ex, file=sys.stderr); continue
        for it in items:
            if isinstance(it, dict) and it.get("a") and it["a"] in passage and len(it["a"]) <= 60:
                out.append({"doc": e, "q": it["q"].strip(), "a": it["a"].strip()})
        print(e, part, len(out), flush=True)
json.dump(out, open(sys.argv[1], "w"), indent=1); print("questions", len(out))
