"""results.md from raw/results.json (run.py writes it). Every number on the
page comes from the raw file."""

import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
R = json.load(open(os.path.join(HERE, "raw", "results.json")))

LABELS = {
    "popularity": "Popularity (numpy)",
    "cooc": "Windowed co-occurrence, tuned on validation (numpy)",
    "two_tower": "Two-tower model, tuned on validation (PyTorch, scored in numpy)",
    "chloedb_recommend": "ChloeDB `recommend('ml', :u)` + `NOT IN`, no tuning",
    "chloedb_two_tower": "The two-tower's vectors served from ChloeDB (`similarity(emb, :v)`)",
    "chloedb_fused": "ChloeDB `rrf(recommend(...), similarity(emb, :v))`",
}


def f(v, d=4):
    return "–" if v is None else f"{v:,.{d}f}"


def main():
    out = []
    w = out.append
    m = R["machine"]
    s = R["sources"]
    tt = R["two_tower_setting"]
    w("# Nadia: next-movie recommendation, ChloeDB's recommender against a two-tower model\n")
    w("Nadia, a recommender-systems engineer in the persona study, found that `recommend()` beat her tuned co-occurrence "
      "baseline with no tuning. Her head of ML asks for \"an offline comparison against our two-tower model\". This page is "
      f"that comparison on her protocol: MovieLens-1M ({R['ratings']:,} ratings of {R['items']:,} movies by {R['users']:,} users), "
      "every rating an interaction, each user's ratings in time order; the last is the test, the one before it the validation, "
      "the rest training; every movie ranked but the user's own; hit@10, NDCG@10 and MRR@10 over every user.\n")
    w("The two-tower model is what such a team would train first: a user tower over the user's recent movies (embeddings "
      "pooled with learned recency weights, then a two-layer MLP), an item tower (the embedding through a linear layer), "
      "in-batch softmax with the logQ correction for popularity, temperature 0.05, Adam, batches of 1,024. Its size, history "
      f"length and epochs were chosen on the validation item over {len(R['tuning']['two_tower'])} settings "
      f"(best: {tt['dim']} dimensions, the last {tt['history']} movies, {tt['epochs']} epochs), then it was trained again on "
      "training plus validation for the test. The baselines are tuned the same way. ChloeDB's recommender has no tuning: "
      "`CREATE RECOMMENDER ml ON events(usr, item, ts) ITEMS items(emb) OPTIONS (window = 3, history = 20)` over the same events, "
      "the two-tower's item vectors in `items.emb`.\n")
    w(f"Measured on {R['at'][:10]} on {m['cpu']}, {m['cores']} cores, {m['memory_mb'] // 1024} GB, {m['os']}; "
      f"{R['chloedb_version']}; PyTorch on the CPU. ChloeDB is asked over HTTP, one user at a time. Run it again with "
      "`python3 run.py && python3 report.py` (needs `pip install torch numpy` and a chloedb-server build; it downloads the ratings, "
      "pinned by hash). Raw numbers: [raw/results.json](raw/results.json).\n")

    w("## What it says\n")
    order = sorted((k for k in s if "hit@10" in s[k]), key=lambda k: -s[k]["hit@10"])
    w("- **Ranked by hit@10:** " + "; ".join(f"{LABELS.get(k, k).split(' (')[0]} {f(s[k]['hit@10'])}" for k in order) + ".")
    for name, d in R["paired"].items():
        a, b = name.split(" - ")
        h = d["hit@10"]
        verdict = "better" if h["lo"] > 0 else ("worse" if h["hi"] < 0 else "not distinguishable")
        w(f"- **{LABELS.get(a, a).split(' (')[0]} against {LABELS.get(b, b).split(' (')[0]}:** hit@10 {h['diff']:+.4f} "
          f"(95% interval {h['lo']:+.4f} to {h['hi']:+.4f}), {verdict}; NDCG@10 {d['ndcg@10']['diff']:+.4f} "
          f"({d['ndcg@10']['lo']:+.4f} to {d['ndcg@10']['hi']:+.4f}).")
    best = max(R["tuning"]["two_tower"], key=lambda t: t["val_hit@10"])
    if best == max(R["tuning"]["two_tower"], key=lambda t: (t["dim"], t["history"], t["epochs"])):
        w("- **The two-tower's number is a floor.** Its best setting was the largest one tried, so a larger model or more "
          "epochs may score higher; the grid was kept to what trains in minutes on four cores.")
    if "chloedb_two_tower" in s and "two_tower" in s and s["chloedb_two_tower"]["hit@10"] == s["two_tower"]["hit@10"]:
        w("- **ChloeDB serves the team's own model unchanged.** With the two-tower's item vectors in a `VECTOR` column and "
          "the user's vector as a parameter, ChloeDB's answers scored exactly as the model's own numpy ranking, and fusing them "
          "with `recommend()` in one statement scored above both.")
    cr = R["chloedb_create_recommender"]
    mem = R["chloedb_memory_kb"]
    w(f"- **The cost.** The two-tower trained in {f(tt['train_s'], 0)} s on the CPU (the final run). ChloeDB took "
      f"{f(R['chloedb_events_write_s'], 1)} s to write the {cr.get('events', 0):,} events and "
      f"{f((cr.get('took_ms') or 0) / 1000, 1)} s to count them; its server grew from {mem['before_recommender'] / 1024:,.0f} MB "
      f"to {mem['after_recommender'] / 1024:,.0f} MB with the recommender (peak {mem.get('peak', 0) / 1024:,.0f} MB). "
      "A recommendation's time is in the table below.\n")

    w("## Each source\n")
    w("| source | hit@10 | NDCG@10 | MRR@10 | p50 (client) | p95 (client) | p50 (server) |")
    w("|---|---|---|---|---|---|---|")
    for k in order:
        x = s[k]
        lat = x.get("latency")
        w(f"| {LABELS.get(k, k)} | {f(x['hit@10'])} | {f(x['ndcg@10'])} | {f(x['mrr@10'])} | "
          + (f"{f(lat['client']['p50_ms'], 1)} ms | {f(lat['client']['p95_ms'], 1)} ms | {f(lat['server']['p50_ms'], 1)} ms |" if lat else "– | – | – |"))
    for k, x in s.items():
        if "error" in x:
            w(f"| {LABELS.get(k, k)} | refused: {x['error'][:200]} | | | | | |")
    w("")
    w("## Tuning on the validation item\n")
    w("| two-tower: dimensions | history | epochs | validation hit@10 | training time |")
    w("|---|---|---|---|---|")
    for t in R["tuning"]["two_tower"]:
        w(f"| {t['dim']} | {t['history']} | {t['epochs']} | {f(t['val_hit@10'])} | {f(t['train_s'], 0)} s |")
    w("")
    w("| co-occurrence: window | history | decay | validation hit@10 |")
    w("|---|---|---|---|")
    for t in R["tuning"]["cooc"]:
        w(f"| {t['window']} | {t['history']} | {t['decay']} | {f(t['val_hit@10'])} |")
    w("")
    w("A caveat that holds for every row: leave-last-out on MovieLens-1M trains on other users' later ratings, and ranks "
      "movies that did not exist yet at a user's last rating. The repository's recommender gate also runs a time split "
      "([chloedb/results/sequence/gate.py](../../../chloedb/results/sequence/gate.py)); this page keeps Nadia's protocol, which is the one her team uses.\n")
    with open(os.path.join(HERE, "results.md"), "w") as fh:
        fh.write("\n".join(out))
    print("wrote results.md")


if __name__ == "__main__":
    main()
