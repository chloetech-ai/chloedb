"""results.md from raw/results-*.json (run.py writes them). Every number on
the page comes from the raw files."""

import glob
import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
RUNS = sorted((json.load(open(p)) for p in glob.glob(os.path.join(HERE, "raw", "results-*.json"))), key=lambda r: r["passages"])
NAMES = {"scifact": "SciFact", "fiqa": "FiQA-2018"}

LABELS = {
    "es_bm25_beir": "Elasticsearch BM25, `multi_match` on title and text (BEIR's query)",
    "es_bm25_body": "Elasticsearch BM25, `match` on title and text in one field",
    "qdrant": "Qdrant, bge-small vectors, its default HNSW",
    "es_qdrant_rrf": "Elasticsearch + Qdrant, RRF in Python (k 60, top 100 of each)",
    "chloedb_bm25": "ChloeDB `bm25(body, :q)`",
    "chloedb_vector": "ChloeDB `similarity(emb, :v)`",
    "chloedb_rrf": "ChloeDB `rrf(bm25(body, :q), similarity(emb, :v))`",
    "exact_vector": "Exact vector search (numpy), for reference",
}
ORDER = ["es_qdrant_rrf", "chloedb_rrf", "es_bm25_beir", "es_bm25_body", "chloedb_bm25", "qdrant", "chloedb_vector", "exact_vector"]


def f(v, d=4):
    return "–" if v is None else f"{v:,.{d}f}"


def mb(kb):
    return f"{kb / 1024:,.0f} MB"


def verdict(d):
    return "better" if d["lo"] > 0 else ("worse" if d["hi"] < 0 else "not distinguishable")


def main():
    out = []
    w = out.append
    r0 = RUNS[0]
    m = r0["machine"]
    w("# Priya: hybrid retrieval, ChloeDB against Elasticsearch plus Qdrant\n")
    w("Priya, an ML engineer in the persona study, runs RAG retrieval as Elasticsearch for BM25 and Qdrant for vectors, "
      "with bge-small-en-v1.5 vectors she computes herself, fused by reciprocal rank fusion in her service. Her tech lead "
      "asks for \"a head-to-head on our judged set against Elasticsearch plus Qdrant: same relevance or better, p95 no worse, "
      "fewer machines\". This page runs it on two public judged sets: "
      + "; ".join(f"BEIR {NAMES.get(r['set'], r['set'])} ({r['passages']:,} passages, {r['questions']:,} test questions)" for r in RUNS)
      + ". Every passage and question is embedded once with fastembed's bge-small-en-v1.5, and both sides get the same vectors.\n")
    w("Elasticsearch has the passages (`english` analyzer, one shard), Qdrant the vectors (cosine, its default HNSW), and "
      "the service asks both at once and fuses their top 100 by RRF with k = 60. ChloeDB has the passages and vectors in "
      "one table, `CREATE TABLE docs (id KEYWORD PRIMARY KEY, body TEXT, emb VECTOR(384))`, and answers "
      "`SELECT id FROM docs ORDER BY rrf(bm25(body, :q, syntax => 'plain'), similarity(emb, :v)) DESC LIMIT 100`, "
      "its defaults also k = 60 and the top 100 of each. Nothing is tuned on either side.\n")
    w(f"Measured on {r0['at'][:10]} on {m['cpu']}, {m['cores']} cores, {m['memory_mb'] // 1024} GB, {m['os']}; "
      f"{r0['es_qdrant']['version']}; {r0['chloedb']['version']}. All three on the same machine, one system at a time, "
      "each loaded over HTTP from one client and asked over HTTP. Latency is each test question once after 20 warm-ups, "
      "with 1 and with 4 clients; it leaves out embedding the question, which costs the same on both sides. Run it again with "
      "`python3 run.py && python3 report.py` (needs Docker, `pip install -r ../requirements.txt` and a "
      "chloedb-server build; it downloads the collections, pinned by hash, and the model). Raw numbers: "
      + ", ".join(f"[raw/results-{r['set']}.json](raw/results-{r['set']}.json)" for r in RUNS) + ".\n")

    w("## What it says\n")
    for r in RUNS:
        s, p = r["sources"], r["paired"]
        hy = p["chloedb_rrf - es_qdrant_rrf"]
        cl, eq = r["chloedb"]["latency"], r["es_qdrant"]["latency"]
        c1, e1 = cl["chloedb_rrf"][0], eq["es_qdrant_rrf"][0]
        c4, e4 = cl["chloedb_rrf"][1], eq["es_qdrant_rrf"][1]
        w(f"- **{NAMES.get(r['set'], r['set'])}.** Hybrid nDCG@10: ChloeDB {f(s['chloedb_rrf']['ndcg@10'])}, Elasticsearch + Qdrant "
          f"{f(s['es_qdrant_rrf']['ndcg@10'])}; the difference {hy['ndcg@10']['diff']:+.4f} (95% interval {hy['ndcg@10']['lo']:+.4f} "
          f"to {hy['ndcg@10']['hi']:+.4f}), {verdict(hy['ndcg@10'])}. Recall@100 {f(s['chloedb_rrf']['recall@100'])} against "
          f"{f(s['es_qdrant_rrf']['recall@100'])}. One client: ChloeDB p50 {f(c1['p50_ms'], 1)} ms, p95 {f(c1['p95_ms'], 1)}; "
          f"Elasticsearch + Qdrant (asked at once) p50 {f(e1['p50_ms'], 1)}, p95 {f(e1['p95_ms'], 1)}. Four clients: ChloeDB "
          f"{f(c4['per_s'], 0)} questions/s at p95 {f(c4['p95_ms'], 1)} ms, Elasticsearch + Qdrant {f(e4['per_s'], 0)}/s at "
          f"p95 {f(e4['p95_ms'], 1)} ms.")
    big = RUNS[-1]
    s, p = big["sources"], big["paired"]
    w(f"- **Where the difference comes from.** BM25 alone, ChloeDB against Elasticsearch's `multi_match` on "
      f"{NAMES.get(big['set'], big['set'])}: {p['chloedb_bm25 - es_bm25_beir']['ndcg@10']['diff']:+.4f} nDCG@10 "
      f"({verdict(p['chloedb_bm25 - es_bm25_beir']['ndcg@10'])}); against the same text in one Elasticsearch field: "
      f"{p['chloedb_bm25 - es_bm25_body']['ndcg@10']['diff']:+.4f} ({verdict(p['chloedb_bm25 - es_bm25_body']['ndcg@10'])}). "
      f"Vectors alone, ChloeDB against Qdrant: {p['chloedb_vector - qdrant']['ndcg@10']['diff']:+.4f} "
      f"({verdict(p['chloedb_vector - qdrant']['ndcg@10'])}); of the exact top 10, Qdrant found {f(s['qdrant']['vs_exact_top10'], 3)} "
      f"and ChloeDB {f(s['chloedb_vector']['vs_exact_top10'], 3)}.")
    for r in RUNS:
        s = r["sources"]
        if s["chloedb_rrf"]["ndcg@10"] < s["chloedb_vector"]["ndcg@10"] and s["es_qdrant_rrf"]["ndcg@10"] < s["qdrant"]["ndcg@10"]:
            w(f"- **On {NAMES.get(r['set'], r['set'])} fusing hurts on both sides.** BM25 is weak there "
              f"({f(s['chloedb_bm25']['ndcg@10'])}), and RRF at equal weights ranks below the vectors alone: ChloeDB "
              f"{f(s['chloedb_rrf']['ndcg@10'])} against {f(s['chloedb_vector']['ndcg@10'])}, Elasticsearch + Qdrant "
              f"{f(s['es_qdrant_rrf']['ndcg@10'])} against {f(s['qdrant']['ndcg@10'])}. ChloeDB's `rrf` takes "
              "`weights => [...]` for that in the same statement; this page tunes neither side.")
    ch, eq = big["chloedb"], big["es_qdrant"]
    w(f"- **One system instead of two.** At {big['passages']:,} passages ChloeDB's server held {mb(ch['memory_after_kb'])} after "
      f"the run, against {mb(eq['memory_after_kb']['elasticsearch'])} for Elasticsearch (heap {eq['version'].split('heap ')[1].split(')')[0]}) "
      f"and {mb(eq['memory_after_kb']['qdrant'])} for Qdrant; its files took {ch['disk_bytes'] / 1e6:,.0f} MB, against "
      f"{eq['disk_bytes']['elasticsearch'] / 1e6:,.0f} MB and {eq['disk_bytes']['qdrant'] / 1e6:,.0f} MB. Loading took "
      f"{f(ch['load_s'], 1)} s, against {f(eq['es_load_s'], 1)} s and {f(eq['qdrant_load_s'], 1)} s (Qdrant's until its index "
      "was built). There is no sync job to keep the two stores in step and no fusion code in the service.")
    w("- **What this does not show.** Both sets fit one machine many times over; Priya's service has millions of chunks on "
      "replicas. Her own judged set and her size are the next step, which this script runs on any BEIR-layout collection.\n")

    for r in RUNS:
        w(f"## {NAMES.get(r['set'], r['set'])}: each ranking\n")
        w(f"{r['passages']:,} passages, {r['questions']:,} test questions with {r['judgements']:,} judgements.\n")
        w("| ranking | nDCG@10 | recall@100 | p50, 1 client | p95, 1 client | questions/s, 4 clients | p95, 4 clients |")
        w("|---|---|---|---|---|---|---|")
        lat = {**r["es_qdrant"]["latency"], **r["chloedb"]["latency"]}
        for k in ORDER:
            x = r["sources"].get(k)
            if not x:
                continue
            l = lat.get(k)
            cells = (f"{f(l[0]['p50_ms'], 1)} ms | {f(l[0]['p95_ms'], 1)} ms | {f(l[1]['per_s'], 0)} | {f(l[1]['p95_ms'], 1)} ms"
                     if l and len(l) > 1 else "– | – | – | –")
            w(f"| {LABELS[k]} | {f(x['ndcg@10'])} | {f(x['recall@100'])} | {cells} |")
        sq = r["es_qdrant"]["latency"].get("es_qdrant_rrf_sequential")
        if sq:
            w("")
            w(f"Elasticsearch + Qdrant asked one after the other instead of at once: p50 {f(sq[0]['p50_ms'], 1)} ms, p95 "
              f"{f(sq[0]['p95_ms'], 1)} ms. ChloeDB's own time for the hybrid (`took_ms`): p50 "
              f"{f(r['chloedb']['latency']['chloedb_rrf'][0]['server']['p50_ms'], 2)} ms, p95 "
              f"{f(r['chloedb']['latency']['chloedb_rrf'][0]['server']['p95_ms'], 2)} ms.")
        if r["sources"]["es_bm25_body"] == r["sources"]["es_bm25_beir"]:
            w("")
            w(f"{NAMES.get(r['set'], r['set'])}'s passages have no titles, so the two Elasticsearch queries rank alike.")
        w("")
        w("| difference | nDCG@10 (95% interval) | questions better / worse | recall@100 (95% interval) |")
        w("|---|---|---|---|")
        for name, d in r["paired"].items():
            a, b = name.split(" - ")
            n, rc = d["ndcg@10"], d["recall@100"]
            w(f"| {LABELS[a].split(' (')[0]} minus {LABELS[b].split(' (')[0]} | {n['diff']:+.4f} ({n['lo']:+.4f} to {n['hi']:+.4f}) | "
              f"{n['better']} / {n['worse']} | {rc['diff']:+.4f} ({rc['lo']:+.4f} to {rc['hi']:+.4f}) |")
        w("")

    w("## Loading, memory and disk\n")
    w("| | " + " | ".join(NAMES.get(r["set"], r["set"]) for r in RUNS) + " |")
    w("|---|" + "---|" * len(RUNS))
    w("| Elasticsearch: `_bulk` load | " + " | ".join(f"{f(r['es_qdrant']['es_load_s'], 1)} s" for r in RUNS) + " |")
    w("| Qdrant: upsert until indexed | " + " | ".join(f"{f(r['es_qdrant']['qdrant_load_s'], 1)} s" for r in RUNS) + " |")
    w("| ChloeDB: `/v1/write` load | " + " | ".join(f"{f(r['chloedb']['load_s'], 1)} s" for r in RUNS) + " |")
    w("| Elasticsearch: memory after the run | " + " | ".join(mb(r["es_qdrant"]["memory_after_kb"]["elasticsearch"]) for r in RUNS) + " |")
    w("| Qdrant: memory after the run | " + " | ".join(mb(r["es_qdrant"]["memory_after_kb"]["qdrant"]) for r in RUNS) + " |")
    w("| ChloeDB: memory after the run | " + " | ".join(mb(r["chloedb"]["memory_after_kb"]) for r in RUNS) + " |")
    w("| Elasticsearch: disk | " + " | ".join(f"{r['es_qdrant']['disk_bytes']['elasticsearch'] / 1e6:,.0f} MB" for r in RUNS) + " |")
    w("| Qdrant: disk | " + " | ".join(f"{r['es_qdrant']['disk_bytes']['qdrant'] / 1e6:,.0f} MB" for r in RUNS) + " |")
    w("| ChloeDB: disk | " + " | ".join(f"{r['chloedb']['disk_bytes'] / 1e6:,.0f} MB" for r in RUNS) + " |")
    emb = [r["embedding"] for r in RUNS]
    if all("docs_s" in e for e in emb):
        w("| embedding the passages (fastembed, both sides) | " + " | ".join(f"{f(e['docs_s'], 0)} s" for e in emb) + " |")
    w("")
    w("Elasticsearch's and Qdrant's memory is their containers' (`docker stats`); Elasticsearch's is mostly the heap it was "
      "given, which it takes at start. ChloeDB's is the server's proportional set size. Elasticsearch's disk is its store after a "
      "force merge to one segment, Qdrant's its storage folder, ChloeDB's its database files after a vacuum.\n")
    w("## Setup, step by step\n")
    w("Elasticsearch + Qdrant: run Elasticsearch; create the index with its mapping; run Qdrant; create the collection; "
      "embed the passages; `_bulk` the passages into Elasticsearch; upsert the vectors into Qdrant with an integer id and the "
      "passage's id as payload (Qdrant takes integers or UUIDs); wait for Qdrant's index; in the service, ask both, map Qdrant's "
      "ids back and fuse; and keep a job that writes every change to both. Nine steps and two systems to run.\n")
    w("ChloeDB: run chloedb-server; `CREATE TABLE`; embed the passages; write the rows with their vectors; ask one SQL "
      "query. Five steps and one system.\n")
    with open(os.path.join(HERE, "results.md"), "w") as fh:
        fh.write("\n".join(out))
    print("wrote results.md")


if __name__ == "__main__":
    main()
