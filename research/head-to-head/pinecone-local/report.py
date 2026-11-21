"""results.md from raw/results-*.json (run.py writes them). Every number on
the page comes from the raw files."""

import glob
import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
RUNS = sorted((json.load(open(p)) for p in glob.glob(os.path.join(HERE, "raw", "results-*.json"))), key=lambda r: r["vectors"])


def ms(v):
    return f"{v:,.2f} ms" if v < 10 else f"{v:,.1f} ms"


def mb(kb):
    return f"{kb / 1024:,.0f} MB"


def secs(s):
    return f"{s:,.1f} s"


def main():
    out = []
    w = out.append
    r0 = RUNS[0]
    m = r0["machine"]
    w("# Vector search: ChloeDB against Pinecone Local")
    w("")
    w("Asked directly, not by a persona of the study: did we ever measure ChloeDB against Pinecone, and can we try it "
      "locally, especially the vector search? Pinecone is a hosted service. The one part of it that runs on a machine is "
      "Pinecone Local (`ghcr.io/pinecone-io/pinecone-local`), Pinecone's in-memory emulator of its API for development "
      "and tests, which reached end of life on 7 October 2026 ([Pinecone's release notes](https://docs.pinecone.io/release-notes/2026)); its last image still pulls and runs. This page puts the two "
      "side by side on GloVe-100 from ann-benchmarks (`glove-100-angular`: word vectors of 100 dimensions), at the first "
      f"{RUNS[0]['vectors']:,} vectors (the limit per index Pinecone Local's documentation gave, which the image does not enforce) and at all {RUNS[-1]['vectors']:,}.")
    w("")
    w("Both get the same vectors, scaled to unit length so that cosine and the dot product rank alike. Pinecone Local has "
      "an index of `metric: cosine`, loaded by `POST /vectors/upsert` and asked `POST /query {vector, topK: 10}`. ChloeDB "
      "has `CREATE TABLE docs (id KEYWORD PRIMARY KEY, emb VECTOR(100))`, loaded by `POST /v1/write` and then "
      "`POST /v1/admin/optimize` (which writes the memtable to segments and merges them), and is asked "
      "`SELECT id FROM docs ORDER BY similarity(emb, :v) DESC LIMIT 10`. Both are loaded 500 vectors a request (1,000 of "
      "them as JSON pass Pinecone's 2 MB a request) from one client. Nothing is tuned on either side; ChloeDB's other "
      "`ef` values are shown beside its default.")
    w("")
    ver_p = r0["pinecone"]["version"]
    ver_c = r0["chloedb"]["version"]
    w(f"Measured on {r0['date']} on {m['cpu']}, {m['cores']} cores, {m['memory_mb'] // 1024} GB, {m['os']}; {ver_p}; "
      f"{ver_c}. Both on the same machine, one at a time, over HTTP JSON from Python. Recall@10 is the share of the exact "
      f"top 10 (numpy, over the same unit vectors) a system returned, over the first {r0['queries']:,} test queries. "
      "Latency is each query once after 20 warm-ups, with 1 and with 4 clients. Run it again with "
      "`python3 run.py && python3 report.py` (needs Docker, numpy, h5py and a chloedb-server build; it downloads the "
      "collection, checked against [glove.sha256](glove.sha256)). Raw numbers: "
      + ", ".join(f"[raw/results-{tag(r)}.json](raw/results-{tag(r)}.json)" for r in RUNS) + ".")
    w("")
    w("## What it says")
    w("")
    for r in RUNS:
        p, c, rec = r["pinecone"], r["chloedb"], r["recall_at_10"]
        p1, c1, p4, c4 = p["latency"][0], c["latency"][0], p["latency"][1], c["latency"][1]
        w(f"- **{r['vectors']:,} vectors.** Recall@10: Pinecone Local {rec['pinecone']:.4f}, ChloeDB {rec['chloedb']:.4f} "
          f"at its default ef. One client: Pinecone Local p50 {ms(p1['p50_ms'])}, p99 {ms(p1['p99_ms'])}; ChloeDB p50 "
          f"{ms(c1['p50_ms'])}, p99 {ms(c1['p99_ms'])} ({p1['p50_ms'] / c1['p50_ms']:,.0f}x at the median). Four clients: "
          f"Pinecone Local {p4['qps']:,.0f} queries/s, ChloeDB {c4['qps']:,.0f}/s. Loading took {secs(p['ready_s'])} "
          f"against {secs(c['ready_s'])} ({secs(c['load_s'])} of writes and {secs(c['ready_s'] - c['load_s'])} to "
          f"write and merge segments with their HNSW graphs).")
    big = RUNS[-1]
    small = RUNS[0]
    w(f"- **Pinecone Local scans every vector.** Its recall is exact at both sizes, and its median query grows with the "
      f"collection, {ms(small['pinecone']['latency'][0]['p50_ms'])} at {small['vectors']:,} and "
      f"{ms(big['pinecone']['latency'][0]['p50_ms'])} at {big['vectors']:,}, where ChloeDB's HNSW goes from "
      f"{ms(small['chloedb']['latency'][0]['p50_ms'])} to {ms(big['chloedb']['latency'][0]['p50_ms'])}. That is what an "
      "emulator for tests does, and it is why these numbers say nothing about hosted Pinecone's speed or recall, which "
      "runs its own index on its own machines.")
    bf = big["chloedb"].get("flat")
    if bf:
        w(f"- **Exact against exact.** Asked to measure every vector too (`strategy => 'flat'`), ChloeDB finds "
          f"{big['recall_at_10']['chloedb_flat']:.4f} of the exact top 10 at {big['vectors']:,} vectors, at p50 "
          f"{ms(bf[0]['p50_ms'])} against Pinecone Local's {ms(big['pinecone']['latency'][0]['p50_ms'])} "
          f"({big['pinecone']['latency'][0]['p50_ms'] / bf[0]['p50_ms']:,.0f}x), and {bf[1]['qps']:,.0f} queries/s at 4 "
          f"clients against {big['pinecone']['latency'][1]['qps']:,.0f}: it scans 8-bit codes on every core and "
          "re-scores the best in float32.")
    hi = max(big["chloedb"]["ef"], key=int)
    w(f"- **What ChloeDB's recall costs.** GloVe is one of ann-benchmarks' hardest collections for a graph. At "
      f"{big['vectors']:,} vectors ChloeDB's default ef (200) finds {big['recall_at_10']['chloedb']:.4f} of the exact "
      f"top 10; ef 400 finds {big['recall_at_10']['chloedb_ef400']:.4f}, and ef {hi} "
      f"{big['recall_at_10']['chloedb_ef' + hi]:.4f} at p50 {ms(big['chloedb']['ef'][hi]['p50_ms'])}. An application "
      "that needs more recall than the default on vectors like these sets `ef` in the query.")
    w(f"- **Memory and disk.** At {big['vectors']:,} vectors Pinecone Local's container held "
      f"{mb(big['pinecone']['memory_after_kb'])} after the run and keeps nothing on disk (a restart loses the index); "
      f"ChloeDB's server held {mb(big['chloedb']['memory_after_kb'])} and its files took "
      f"{big['chloedb']['disk_bytes'] / 1e6:,.0f} MB (float32 rows, 8-bit codes and the graph).")
    w("- **What this does not show.** Hosted Pinecone (serverless or pods), whose latency includes the network to its "
      "region and whose recall depends on its index; filters, namespaces, sparse or hybrid search. A hosted run, with a "
      "free-tier key and the index's host in place of the emulator's, is the next step.")
    w("")
    for r in RUNS:
        p, c, rec = r["pinecone"], r["chloedb"], r["recall_at_10"]
        w(f"## {r['vectors']:,} vectors")
        w("")
        w("| system | recall@10 | p50, 1 client | p95, 1 client | p99, 1 client | queries/s, 4 clients | p99, 4 clients |")
        w("|---|---|---|---|---|---|---|")
        p1, p4 = p["latency"]
        w(f"| Pinecone Local, `topK: 10` | {rec['pinecone']:.4f} | {ms(p1['p50_ms'])} | {ms(p1['p95_ms'])} | {ms(p1['p99_ms'])} "
          f"| {p4['qps']:,.0f} | {ms(p4['p99_ms'])} |")
        c1, c4 = c["latency"]
        w(f"| ChloeDB `similarity(emb, :v)`, default ef (200) | {rec['chloedb']:.4f} | {ms(c1['p50_ms'])} | "
          f"{ms(c1['p95_ms'])} | {ms(c1['p99_ms'])} | {c4['qps']:,.0f} | {ms(c4['p99_ms'])} |")
        for ef, lat in sorted(c["ef"].items(), key=lambda kv: int(kv[0])):
            w(f"| ChloeDB `similarity(emb, :v, ef => {ef})` | {rec['chloedb_ef' + ef]:.4f} | {ms(lat['p50_ms'])} | "
              f"{ms(lat['p95_ms'])} | {ms(lat['p99_ms'])} | – | – |")
        if c.get("flat"):
            f1, f4 = c["flat"]
            w(f"| ChloeDB `similarity(emb, :v, strategy => 'flat')`, exact | {rec['chloedb_flat']:.4f} | {ms(f1['p50_ms'])} | "
              f"{ms(f1['p95_ms'])} | {ms(f1['p99_ms'])} | {f4['qps']:,.0f} | {ms(f4['p99_ms'])} |")
        w("")
        srv = c1.get("server")
        if srv:
            w(f"ChloeDB's own time for the default query (`took_ms`): p50 {ms(srv['p50_ms'])}, p95 {ms(srv['p95_ms'])}; "
              "the rest of its latency is HTTP and JSON, the same path Pinecone Local's answers take. "
              f"Of the {r['queries']:,} queries, the two returned the same top 10 in the same order for "
              f"{r['same_top10'] * 100:.1f}%.")
            w("")
        w("| loading | Pinecone Local | ChloeDB |")
        w("|---|---|---|")
        w(f"| writes, 500 a request | {secs(p['load_s'])} | {secs(c['load_s'])} |")
        w(f"| until every vector is searched as it will be | {secs(p['ready_s'])} | {secs(c['ready_s'])} "
          f"(one segment of {c['segments'][0]:,} after optimize) |")
        w(f"| memory after the run | {mb(p['memory_after_kb'])} | {mb(c['memory_after_kb'])} |")
        w(f"| disk | none (in memory) | {c['disk_bytes'] / 1e6:,.0f} MB |")
        w("")
    with open(os.path.join(HERE, "results.md"), "w") as f:
        f.write("\n".join(out))
    print("\n".join(out))


def tag(r):
    return "full" if r["vectors"] > 100_000 else "100k"


if __name__ == "__main__":
    main()
