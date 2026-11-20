"""Idea 2 prototype: a small open model writes ChloeDB SQL, free or under the
schema's grammar. Arms: model x prompt (docs: schema + 4 examples; bare:
schema + one line) x grammar (off/on). A query counts as correct when its
result holds the same values as the gold query's (search questions: recall
of the gold top-5 ids)."""
import json, sys, time, os, requests
from llama_cpp import Llama, LlamaGrammar
from grammar import gbnf

URL = "http://127.0.0.1:7700"
HERE = os.path.dirname(os.path.abspath(__file__))
SCHEMA = """Table hn_stories (Hacker News stories of July 2026):
  id INT, title TEXT (full-text indexed), url KEYWORD, domain KEYWORD (e.g. 'github.com'),
  author KEYWORD, created TIMESTAMP, points INT, comments INT"""
BARE = f"""Write one ChloeDB SQL query that answers the question. ChloeDB is SQL with full-text search: MATCH(field, 'words') filters and bm25(field, 'words') scores.
{SCHEMA}
"""
DOCS = BARE + """
Question: How many stories mention kubernetes?
SQL: SELECT COUNT(*) FROM hn_stories WHERE MATCH(title, 'kubernetes')
Question: What are the 3 highest-scoring stories from bbc.com?
SQL: SELECT title FROM hn_stories WHERE domain = 'bbc.com' ORDER BY points DESC LIMIT 3
Question: Find the 5 stories most relevant to 'quantum computing'. Give their ids.
SQL: SELECT id FROM hn_stories ORDER BY bm25(title, 'quantum computing') DESC LIMIT 5
Question: Which author posted the most stories in the first week of July 2026?
SQL: SELECT author FROM hn_stories WHERE created >= TIMESTAMP '2026-07-01' AND created < TIMESTAMP '2026-07-08' GROUP BY author ORDER BY COUNT(*) DESC LIMIT 1
"""

def run(sql):
    try:
        r = requests.post(URL + "/v1/sql", json={"sql": sql, "timeout_ms": 5000}, timeout=10)
        d = r.json()
        return (d["rows"], None) if r.ok and "rows" in d else (None, d.get("error", str(d))[:200])
    except Exception as e:
        return None, str(e)[:200]

def norm(rows):
    out = set()
    for row in rows:
        for v in row:
            out.add(round(v, 2) if isinstance(v, float) else v)
    return out

def score(x, rows):
    gold, _ = run(x["sql"])
    if rows is None: return 0.0
    if x.get("kind") == "search":
        g = {r[0] for r in gold}; m = {v for r in rows for v in r}
        return len(g & m) / len(g)
    return 1.0 if norm(rows) == norm(gold) else 0.0

def main(model_file, out):
    qs = json.load(open(os.path.join(HERE, "questions.json")))
    llm = Llama(model_file, n_ctx=2048, n_threads=4, verbose=False)
    grammar = LlamaGrammar.from_string(gbnf(URL, "hn_stories"), verbose=False)
    res = []
    for pname, prompt in (("docs", DOCS), ("bare", BARE)):
        for g in (False, True):
            for i, x in enumerate(qs):
                p = prompt + f"\nQuestion: {x['q']}\nSQL:"
                t = time.time()
                o = llm.create_completion(p, max_tokens=120, temperature=0, stop=["\n", ";"],
                                          grammar=grammar if g else None)
                sql = o["choices"][0]["text"].strip().rstrip(";")
                rows, err = run(sql)
                s = score(x, rows)
                rec = {"model": os.path.basename(model_file), "prompt": pname, "grammar": g, "i": i,
                       "kind": x.get("kind", "answer"), "sql": sql, "ran": rows is not None, "error": err,
                       "score": s, "tokens": o["usage"]["completion_tokens"], "secs": round(time.time() - t, 2)}
                res.append(rec); print(json.dumps(rec), flush=True)
    json.dump(res, open(out, "w"), indent=1)

if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
