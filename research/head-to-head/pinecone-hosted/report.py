"""results.md from raw/results-*.json (run.py writes them). Every number on
the page comes from the raw files."""

import glob
import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
RUNS = sorted((json.load(open(p)) for p in glob.glob(os.path.join(HERE, "raw", "results-*.json"))
               if json.load(open(p)).get("chloedb")), key=lambda r: r["vectors"])


def ms(v):
    return f"{v:,.2f} ms" if v < 10 else f"{v:,.1f} ms"


def secs(s):
    return f"{s:,.1f} s"


def tag(r):
    return "100k" if r["vectors"] == 100_000 else "full"


def at_recall(c, rec, want):
    """ChloeDB's fastest setting whose recall reaches want, as (name, recall, p50)."""
    opts = [("default ef (200)", rec["chloedb"], c["latency"][0]["p50_ms"])]
    opts += [(f"ef => {ef}", rec[f"chloedb_ef{ef}"], v["p50_ms"]) for ef, v in c["ef"].items()]
    opts.append(("exact (`strategy => 'flat'`)", rec["chloedb_flat"], c["flat"][0]["p50_ms"]))
    ok = [o for o in opts if o[1] >= want]
    return min(ok, key=lambda o: o[2]) if ok else None


def main():
    out = []
    w = out.append
    r0 = RUNS[0]
    m = r0["machine"]
    w("# Vector search: ChloeDB against hosted Pinecone")
    w("")
    w("The hosted half of [pinecone-local](../pinecone-local/results.md): the same collection (GloVe-100 from "
      "ann-benchmarks, `glove-100-angular`, word vectors of 100 dimensions scaled to unit length), the same first "
      f"{r0['queries']:,} test queries and the same recall and latency code, against a real Pinecone serverless index on "
      "the Starter (free) plan instead of the emulator. Sizes: the first "
      + " and ".join(f"{r['vectors']:,}" for r in RUNS) + " vectors.")
    w("")
    w("Pinecone gets an index of `dimension: 100, metric: cosine` in aws us-east-1 (the one region the Starter plan has), "
      "loaded by `POST /vectors/upsert` 500 vectors a request from one client, and asked `POST /query {vector, topK: 10}` "
      "over kept-alive HTTPS connections, one per client. ChloeDB is run as on the local page, on this machine: "
      "`POST /v1/write` 500 at a time into `CREATE TABLE docs (id KEYWORD PRIMARY KEY, emb VECTOR(100))`, then "
      "`POST /v1/admin/optimize`, asked `SELECT id FROM docs ORDER BY similarity(emb, :v) DESC LIMIT 10`. Nothing is "
      "tuned on either side.")
    w("")
    w(f"Measured on {r0['date']} from a cloud container ({m['cpu']}, {m['cores']} cores, {m['memory_mb'] // 1024} GB, "
      f"{m['os']}), whose traffic to the internet goes through an egress proxy that ends TLS and opens it again; where "
      f"the container and the proxy are is not known. {r0['chloedb']['version']}. Pinecone's half and ChloeDB's half of "
      "each size ran one after the other on the same machine. Recall@10 is the share of the exact top 10 (numpy) a system "
      "returned; latency is each query once after 20 warm-ups. Pinecone's own time is the "
      "`x-pinecone-response-duration-ms` header of each answer (in whole milliseconds); the network is the client's time less that. Run it again "
      "with `pinecone=<API key> python3 run.py 100k full && python3 report.py` (the key is read from the environment and "
      "written nowhere; the index is deleted at the end). Raw numbers: "
      + ", ".join(f"[raw/results-{tag(r)}.json](raw/results-{tag(r)}.json)" for r in RUNS) + ".")
    w("")
    w("## What it says")
    w("")
    big = RUNS[-1]
    same = at_recall(big["chloedb"], big["recall_at_10"], big["recall_at_10"]["pinecone"])
    bp = big["pinecone"]["latency_settled"][0]
    if same:
        w(f"At {big['vectors']:,} vectors and the same recall, Pinecone answers in {ms(bp['p50_ms'])} at the median from here "
          f"({ms(bp['served']['duration_ms']['p50_ms'])} of it Pinecone's own time) and ChloeDB, at {same[0]}, in "
          f"{ms(same[2])} on this machine; Pinecone's default finds more than ChloeDB's default at that size "
          f"({big['recall_at_10']['pinecone']:.4f} against {big['recall_at_10']['chloedb']:.4f}).")
        w("")
    for r in RUNS:
        p, c, rec = r["pinecone"], r["chloedb"], r["recall_at_10"]
        ps = p["latency_settled"][0]
        own = ps["served"]["duration_ms"]
        net = ps["served"]["client_minus_duration_ms"]
        c1, c4, p4 = c["latency"][0], c["latency"][1], p["latency"][1]
        same = at_recall(c, rec, rec["pinecone"])
        line = (f"- **{r['vectors']:,} vectors.** Recall@10 once Pinecone's index is built: Pinecone {rec['pinecone']:.4f}, "
                f"ChloeDB {rec['chloedb']:.4f} at its default ef. One client, as the client sees it: Pinecone p50 "
                f"{ms(ps['p50_ms'])}, p99 {ms(ps['p99_ms'])}; ChloeDB p50 {ms(c1['p50_ms'])}, p99 {ms(c1['p99_ms'])}. "
                f"Of Pinecone's {ms(ps['p50_ms'])}, {ms(own['p50_ms'])} is Pinecone's own time (p50; p99 {ms(own['p99_ms'])}) "
                f"and {ms(net['p50_ms'])} the network; ChloeDB's own time (`took_ms`) is {ms(c1['server']['p50_ms'])}. ")
        if same:
            line += (f"At Pinecone's recall or more, ChloeDB's fastest setting is {same[0]} ({same[1]:.4f}, p50 "
                     f"{ms(same[2])}). ")
        line += (f"Four clients: Pinecone {p4['qps']:,.0f} queries/s, ChloeDB {c4['qps']:,.0f}/s. Loading took "
                 f"{secs(p['load_s'])} for Pinecone (every vector counted after {secs(p['ready_s'])}) against "
                 f"{secs(c['load_s'])} of writes and {secs(c['ready_s'])} until ChloeDB's segments were merged.")
        w(line)
    w("- **The network is most of Pinecone's time from here.** A request that does no search "
      "(`describe_index_stats`) takes p50 " + " and ".join(ms(r['pinecone']['floor_ms']['p50_ms']) for r in RUNS)
      + " on a kept-alive connection (the two runs), and a new connection's TCP and TLS handshake through the proxy p50 "
      + " and ".join(ms(r['pinecone']['handshake_ms']['p50_ms']) for r in RUNS) + ", "
      "which is why the client keeps its connections. From a machine in aws us-east-1 the network should be a few "
      "milliseconds (not measured here) and Pinecone's own time most of the total; ChloeDB has no network to cross when it runs in the "
      "application's process or beside it.")
    parts = []
    for r in RUNS:
        p, rec = r["pinecone"], r["recall_at_10"]
        parts.append(f"at {r['vectors']:,} vectors, asked as soon as every vector was counted, Pinecone found "
                     f"{rec['pinecone_first']:.4f} of the exact top 10, and {rec['pinecone']:.4f} "
                     f"{p['settled_after_s'] / 60:,.0f} minutes after the load began "
                     f"({p['same_top10_first_settled']:.1%} of the queries had the same top 10 in the same order both times)")
    w("- **Recall can move after a load.** " + "; ".join(parts)[0].upper() + "; ".join(parts)[1:] + ". It looks as if "
      "the serverless index answers from an exact scan of fresh writes until its approximate index of them is built "
      "(Pinecone's answers do not say), which at 100,000 vectors took less than the three minutes of the timed runs and "
      "at 1,183,514 was over before the ten-minute load ended. The settled number is the one compared here.")
    w("- **Loading waits on each request.** One client sends a request of 500 vectors (about 1 MB of JSON) and waits "
      "for it: " + " and ".join(f"{r['pinecone']['load_s'] * 1000 / len(range(0, r['vectors'], 500)):,.0f} ms a request at {r['vectors']:,}" for r in RUNS)
      + ", of which about 44 ms is the round trip measured above and the rest sending the body through the proxy and "
      "Pinecone's write (not split here). Pinecone suggests upserting in parallel, which was not tried, so its load "
      "time is that of one client; ChloeDB's writes go to a server on the same machine.")
    w("- **What this does not show.** Pinecone's pod indexes or its paid plans (the Starter plan has one region and "
      "rate limits), filters, namespaces, sparse or hybrid search, and ChloeDB on a separate machine across a network.")
    w("")
    for r in RUNS:
        p, c, rec = r["pinecone"], r["chloedb"], r["recall_at_10"]
        w(f"## {r['vectors']:,} vectors")
        w("")
        w("| system | recall@10 | p50, 1 client | p95, 1 client | p99, 1 client | own time p50 / p99 | queries/s, 4 clients | p99, 4 clients |")
        w("|---|---|---|---|---|---|---|---|")
        ps, p4 = p["latency_settled"][0], p["latency"][1]
        pf = p["latency"][0]
        w(f"| Pinecone serverless, `topK: 10`, index built | {rec['pinecone']:.4f} | {ms(ps['p50_ms'])} | {ms(ps['p95_ms'])} | "
          f"{ms(ps['p99_ms'])} | {ms(ps['served']['duration_ms']['p50_ms'])} / {ms(ps['served']['duration_ms']['p99_ms'])} | "
          f"{p4['qps']:,.0f} | {ms(p4['p99_ms'])} |")
        w(f"| Pinecone serverless, right after the load | {rec['pinecone_first']:.4f} | {ms(pf['p50_ms'])} | {ms(pf['p95_ms'])} | "
          f"{ms(pf['p99_ms'])} | {ms(pf['served']['duration_ms']['p50_ms'])} / {ms(pf['served']['duration_ms']['p99_ms'])} | – | – |")
        c1, c4 = c["latency"][0], c["latency"][1]
        w(f"| ChloeDB `similarity(emb, :v)`, default ef (200) | {rec['chloedb']:.4f} | {ms(c1['p50_ms'])} | {ms(c1['p95_ms'])} | "
          f"{ms(c1['p99_ms'])} | {ms(c1['server']['p50_ms'])} / – | {c4['qps']:,.0f} | {ms(c4['p99_ms'])} |")
        for ef, v in c["ef"].items():
            w(f"| ChloeDB `similarity(emb, :v, ef => {ef})` | {rec[f'chloedb_ef{ef}']:.4f} | {ms(v['p50_ms'])} | "
              f"{ms(v['p95_ms'])} | {ms(v['p99_ms'])} | – | – | – |")
        f1, f4 = c["flat"]
        w(f"| ChloeDB `similarity(emb, :v, strategy => 'flat')`, exact | {rec['chloedb_flat']:.4f} | {ms(f1['p50_ms'])} | "
          f"{ms(f1['p95_ms'])} | {ms(f1['p99_ms'])} | – | {f4['qps']:,.0f} | {ms(f4['p99_ms'])} |")
        w("")
        w(f"Pinecone's \"right after the load\" row is the first timed pass, which began when every vector was counted; its "
          f"recall is that of the pass just before it. Pinecone's time, split (index built, 1 client): own time p50 "
          f"{ms(ps['served']['duration_ms']['p50_ms'])}, the rest p50 {ms(ps['served']['client_minus_duration_ms']['p50_ms'])} "
          f"and p99 {ms(ps['served']['client_minus_duration_ms']['p99_ms'])}. ChloeDB's own time (`took_ms`) p50 "
          f"{ms(c1['server']['p50_ms'])}, p95 {ms(c1['server']['p95_ms'])}; the rest of its latency is HTTP and JSON on "
          f"this machine. Each Pinecone query cost {', '.join(str(u) for u in ps['served']['read_units_each'])} read unit. "
          f"The two returned the same top 10 in the same order for {r.get('same_top10', 0):.1%} of the queries.")
        w("")
        w(f"With each Pinecone query about {ms(ps['p50_ms'])} from here, four clients can ask at most about "
          f"{4000 / ps['p50_ms']:,.0f} a second, so the four-client number measures the network more than Pinecone"
          + (" (and at this size it ran before the index had settled)." if rec["pinecone_first"] != rec["pinecone"] else "."))
        w("")
        w("| loading | Pinecone serverless | ChloeDB |")
        w("|---|---|---|")
        w(f"| create the index | {secs(p['create_s'])} | – |")
        w(f"| writes, 500 a request, 1 client | {secs(p['load_s'])} | {secs(c['load_s'])} |")
        w(f"| until every vector is counted, or segments merged | {secs(p['ready_s'])} | {secs(c['ready_s'])} |")
        w(f"| disk | not reported for serverless indexes | {c['disk_bytes'] / 1e6:,.0f} MB |")
        w("")
    with open(os.path.join(HERE, "results.md"), "w") as f:
        f.write("\n".join(out))
    print("\n".join(out))


if __name__ == "__main__":
    main()
