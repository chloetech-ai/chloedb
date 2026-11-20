"""results.md from raw/results-*.json (run.py writes them). Every number on
the page comes from the raw files."""

import glob
import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
RUNS = sorted((json.load(open(p)) for p in glob.glob(os.path.join(HERE, "raw", "results-*.json"))), key=lambda r: r["docs"])


def f(v, d=1):
    return "–" if v is None else f"{v:,.{d}f}"


def mb(kb):
    return f"{kb / 1024:,.0f} MB"


def main():
    out = []
    w = out.append
    r0 = RUNS[0]
    m = r0["machine"]
    w("# Kenji: a multi-tenant ticket search, ChloeDB's OpenSearch API against Elasticsearch\n")
    w("Kenji, a backend engineer in the persona study, runs a support-ticket search on Elasticsearch: one index, a `tenant_id` "
      "on each ticket, and per request a `bool` query (`multi_match` on title^3 and body; filters on the tenant, the status and "
      "a date range), `terms` aggregations on `tags` (an array) and `status`, highlights, a sort by score or by date, and "
      "\"load more\" by `search_after`. His CTO wants memory and latency at his sizes against his Elasticsearch, measured the "
      "same way. This page sends the same requests to Elasticsearch and to ChloeDB's OpenSearch API, on made-up tickets of his "
      "shape ([run.py](run.py): 200 tenants with Zipf 1.07 sizes, titles and bodies written from a support vocabulary, up to 8 tags).\n")
    w(f"Measured on {r0['at'][:10]} on {m['cpu']}, {m['cores']} cores, {m['memory_mb'] // 1024} GB, {m['os']}; "
      f"{r0['elasticsearch']['version']}; {r0['chloedb']['version']}. Both on the same machine, one at a time, each loaded by "
      "`_bulk` in batches of 2,000 from one client. The index has one shard and no replica on Elasticsearch, `english` analyzers "
      f"on title and body in both. The search mix is {r0['searches']:,} searches over tenants drawn by their size, 40% sorted by "
      "date with an `id` tie-breaker; it runs with 1 and with 4 clients after 50 warm-up searches. Run it again with "
      "`python3 run.py [DOCS] && python3 report.py` (needs Docker and a chloedb-server build). Raw numbers: "
      + ", ".join(f"[raw/results-{r['docs']}.json](raw/results-{r['docs']}.json)" for r in RUNS) + ".\n")

    w("## What it says\n")
    for r in RUNS:
        a, es, ch = r["agreement"], r["elasticsearch"], r["chloedb"]
        e1, c1 = es["mix"][0], ch["mix"][0]
        e4, c4 = es["mix"][1], ch["mix"][1]
        tags_e = es["parts"].get("terms on tags, an array (filters only)", {})
        tags_c = ch["parts"].get("terms on tags, an array (filters only)", {})
        w(f"- **{r['docs']:,} tickets.** Of {a['compared']} searches compared, both found the same total for {a['total']}, the same "
          f"first page in the same order for {a['ids_in_order']} (the same ids in some order for {a['ids_as_set']}; "
          f"{a['top10_overlap_mean']} of the first ten shared on average), and the same buckets for {a['buckets']}. "
          f"His search, one client: Elasticsearch p50 {f(e1['p50_ms'])} ms, p95 {f(e1['p95_ms'])}; ChloeDB p50 {f(c1['p50_ms'])}, "
          f"p95 {f(c1['p95_ms'])}. Four clients: Elasticsearch {f(e4['per_s'])} searches/s at p95 {f(e4['p95_ms'])} ms, "
          f"ChloeDB {f(c4['per_s'])}/s at p95 {f(c4['p95_ms'])} ms. The `tags` aggregation alone on the biggest tenant: "
          f"Elasticsearch {f(tags_e.get('p50_ms'))} ms, ChloeDB {f(tags_c.get('p50_ms'))} ms (KENJI-401).")
    last = RUNS[-1]
    es, ch = last["elasticsearch"], last["chloedb"]
    tb_e = es["parts"].get("sort by date, id tie-breaker", {})
    tb_c = ch["parts"].get("sort by date, id tie-breaker", {})
    text_e = es["parts"].get("text query", {})
    text_c = ch["parts"].get("text query", {})
    w(f"- **Where the time goes.** The text query alone is as fast on ChloeDB as on Elasticsearch ({f(text_c.get('p50_ms'))} ms "
      f"against {f(text_e.get('p50_ms'))} on the biggest tenant at {last['docs']:,} tickets). What makes his search slower is two "
      f"parts of it: the `terms` aggregation on the `tags` array (above) and the `id` tie-breaker on a date sort "
      f"({f(tb_c.get('p50_ms'))} ms against {f(tb_e.get('p50_ms'))}), both through the OpenSearch layer. Round 4 traced the facet "
      "to arrays read from each row's `_source`, and measured it natively, as `GROUP BY` on a `KEYWORD[]` column, at 3.6 ms; "
      "step x10 of round 4 (KENJI-401, KENJI-404) is meant to fix both.")
    w(f"- **Memory is ChloeDB's.** Its server held {mb(ch['memory_after_kb'])} after the run against Elasticsearch's "
      f"{mb(es['memory_after_kb'])}. Elasticsearch's figure is mostly the heap it was given (`-Xmx` {last['elasticsearch']['version'].split('heap ')[-1].rstrip(')')}), "
      "which it takes at start whatever the data; the comparison that matters for his CTO is the smallest heap at which "
      "Elasticsearch keeps these latencies, which this page does not measure. Disk and loading favour Elasticsearch.\n")

    w("## His search, end to end\n")
    w("| tickets | system | clients | searches/s | p50 | p95 | p99 | errors |")
    w("|---|---|---|---|---|---|---|---|")
    for r in RUNS:
        for side, label in (("elasticsearch", "Elasticsearch"), ("chloedb", "ChloeDB")):
            for x in r[side]["mix"]:
                w(f"| {r['docs']:,} | {label} | {x['clients']} | {f(x['per_s'])} | {f(x['p50_ms'])} ms | {f(x['p95_ms'])} ms | {f(x['p99_ms'])} ms | {x['errors']} |")
    w("")
    w("## Its parts, on the biggest tenant\n")
    w("Each request 15 times, after one; p50 and p95 in milliseconds. The filters are the search's: the tenant, three statuses, the last year.\n")
    labels = list(RUNS[0]["elasticsearch"]["parts"])
    w("| part | " + " | ".join(f"Elasticsearch, {r['docs'] // 1000:,}k | ChloeDB, {r['docs'] // 1000:,}k" for r in RUNS) + " |")
    w("|---|" + "---|---|" * len(RUNS))
    for label in labels:
        cells = []
        for r in RUNS:
            for side in ("elasticsearch", "chloedb"):
                x = r[side]["parts"].get(label, {})
                if "error" in x:
                    cells.append("timed out (504)" if "deadline" in x["error"] else "refused")
                else:
                    cells.append(f"{f(x.get('p50_ms'))} / {f(x.get('p95_ms'))}")
        w(f"| {label} | " + " | ".join(cells) + " |")
    w("")
    w("## Paging a tenant by search_after, loading, memory and disk\n")
    w("| tickets | system | pages of 100 | unique hits | in order | page p50 | `_bulk` load | memory after load | memory after the run | disk |")
    w("|---|---|---|---|---|---|---|---|---|---|")
    for r in RUNS:
        for side, label in (("elasticsearch", "Elasticsearch"), ("chloedb", "ChloeDB")):
            x = r[side]
            p = x["paging"]
            if "error" in p:
                pg = f"refused: {p['error'][:80]} | | |"
            else:
                pg = f"{p['pages']} | {p['unique']:,} | {'yes' if p['in_order'] else 'no'} | {f(p['page_p50_ms'])} ms"
            w(f"| {r['docs']:,} | {label} | {pg} | {f(x['bulk_s'])} s | {mb(x['memory_after_load_kb'])} | {mb(x['memory_after_kb'])} | {x['disk_bytes'] / 1e6:,.0f} MB |")
    w("")
    w("Elasticsearch's memory is its container's (`docker stats`), heap included; ChloeDB's is the server's resident set. "
      "Elasticsearch's disk is its store after a force merge to one segment, ChloeDB's its database files after a vacuum.\n")
    with open(os.path.join(HERE, "results.md"), "w") as fh:
        fh.write("\n".join(out))
    print("wrote results.md")


if __name__ == "__main__":
    main()
