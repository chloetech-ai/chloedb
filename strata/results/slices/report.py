"""Writes the README's tables from the JSON results beside it, between
`<!-- report:NAME -->` and `<!-- /report -->` markers, so that no number in
the README is typed by hand:

    python report.py        # rewrites README.md in place
"""
import json
import os
import re

HERE = os.path.dirname(os.path.abspath(__file__))
S = os.path.join(HERE, "search")
R = os.path.join(HERE, "recs")
METHODS = ("cov", "pop")


def load(d, name):
    p = os.path.join(d, name)
    return json.load(open(p)) if os.path.exists(p) else None


def pct(x, nd=1):
    return f"{100 * x:.{nd}f}%"


def docs(n):
    return f"{int(n) // 1000}K"


def table(head, rows):
    out = ["| " + " | ".join(head) + " |", "|" + "|".join("---:" if i else "---" for i in range(len(head))) + "|"]
    out += ["| " + " | ".join(str(c) for c in r) + " |" for r in rows]
    return "\n".join(out)


def t_slices(m):
    test, seen, b = load(S, "eval_test.json"), load(S, "eval_seen.json"), load(S, "bytes.json")
    rows = []
    for n, v in test["methods"][m].items():
        s = seen["methods"][m][n]
        rows.append([docs(n), f"{b[m][n]['file_bytes'] / 1e6:.1f}", f"{b[m][n]['bytes_per_doc']:.0f}",
                     pct(v["1"]["covered"]), pct(v["1"]["proved"], 2), pct(v["10"]["covered"]), pct(v["10"]["proved"], 2),
                     pct(v["10"]["proved_8bit"], 2), pct(s["10"]["covered"]), pct(s["10"]["proved"], 2), pct(v["10"]["unknown_word"])])
    return table(["passages", "file MB", "bytes / passage", "new: top 1 held", "new: top 1 proved", "new: top 10 held",
                  "new: top 10 proved", "new: top 10 proved, 8-bit bounds", "repeat: top 10 held", "repeat: top 10 proved",
                  "new: a word the slice lacks"], rows)


def t_overlap():
    test = load(S, "eval_test.json")
    rows = []
    for n in test["methods"]["cov"]:
        rows.append([docs(n)] + [pct(test["methods"][m][n][k]["overlap_among_unproved"], 0) for m in ("cov", "pop") for k in ("1", "10")])
    return table(["passages", "cov: top 1", "cov: top 10", "pop: top 1", "pop: top 10"], rows)


def t_lengths():
    rows = []
    for st in ("test", "seen"):
        e = load(S, f"eval2_{st}.json")
        if not e:
            continue
        for n in ("100000", "400000"):
            for L, v in e["tiers"][n]["10"]["by_words"].items():
                rows.append([{"test": "new", "seen": "repeat"}[st], docs(n), L if L != "4" else "4+", pct(v["share"]),
                             pct(v["covered"]), pct(v["proved_single"], 2), pct(v["proved_pairs"], 2)])
    return table(["queries", "passages", "words", "share of queries", "top 10 held", "proved, word bounds", "proved, pair bounds"], rows)


def t_pairs():
    rows = []
    for st in ("test", "seen"):
        e = load(S, f"eval2_{st}.json")
        if not e:
            continue
        for n, per in e["tiers"].items():
            rows.append([{"test": "new", "seen": "repeat"}[st], docs(n)] + [x for k in ("1", "10") for x in (
                pct(per[k]["covered"]), pct(per[k]["proved_single"], 2), pct(per[k]["proved_pairs"], 2))]
                + [sum(per[k]["violations_pairs"] for k in per)])
    return table(["queries", "passages", "top 1 held", "top 1 proved, words", "top 1 proved, pairs", "top 10 held",
                  "top 10 proved, words", "top 10 proved, pairs", "wrong proofs"], rows)


def t_confidence():
    rows = []
    for st in ("test", "seen"):
        e = load(S, f"eval2_{st}.json")
        if not e:
            continue
        for n in ("100000", "400000"):
            for al in e["tiers"][n]["10"]["confidence"]:
                c1, c10 = e["tiers"][n]["1"]["confidence"][al], e["tiers"][n]["10"]["confidence"][al]
                rows.append([{"test": "new", "seen": "repeat"}[st], docs(n), al, pct(c1["answered_locally"]), pct(c1["equal_to_cloud"], 0),
                             pct(c10["answered_locally"]), pct(c10["equal_to_cloud"], 0)])
    return table(["queries", "passages", "alpha", "top 1: answered on the device", "top 1: of those, the cloud's",
                  "top 10: answered on the device", "top 10: of those, the cloud's"], rows)


def t_recs():
    e, b = load(R, "recs_eval.json"), load(R, "recs_bytes.json")
    if not e:
        return "(not run)"
    def per_track(n):  # the measured pack nearest in size
        return b["5000" if n <= 5000 else "50000"]["bytes_per_track"]
    cloud = e["vec_cloud"]["ndcg@10"]
    rows = []
    names = ["history", "vec_cloud"] + [k for k in e if k.startswith("vec_device@")] + ["rrf_cloud"] + [k for k in e if k.startswith("rrf_device@")]
    for k in names:
        v = e[k]
        budget = k.split("@")[1] if "@" in k else ""
        mb = f"{int(budget) * per_track(int(budget)) / 1e6:.1f}" if budget and b else ("0" if k == "history" else "")
        rows.append([k, budget, mb, f"{v['ndcg@10']:.4f}", f"{v['recall@100']:.4f}"])
    base = e.get("cloud_baselines_from_recsys", {})
    for k in ("decay_pop", "item_knn"):
        if k in base:
            rows.append([f"{k} (cloud, `../recsys`)", "", "", f"{base[k]['ndcg@10']:.4f}", f"{base[k]['recall@100']:.4f}"])
    src = json.load(open(os.path.join(HERE, "..", "recsys", "quality", "strata.json")))["sources"]
    for k in ("encode", "ppr", "fused"):
        v = src[k]
        rows.append([f"Strata `{k}` (cloud, `../recsys`)", "", "", f"{v['ndcg@10']:.4f}", f"{v['recall@100']:.4f}"])
    return table(["source", "pack tracks", "pack MB", "NDCG@10", "recall@100 (of min(targets, 100))"], rows) + \
        f"\n\nPack MB is the pack tracks alone; the device also holds the base pack ({1000 * per_track(1000) / 1e6:.1f} MB) and the user's own tracks."


def t_churn():
    c, b = load(R, "recs_churn.json"), load(R, "recs_bytes.json")
    if not c:
        return "(not run)"
    per_track = b["50000"]["bytes_per_track"] if b else None
    rows = []
    for n, v in c["base"].items():
        kb = f"{v['entered_per_day_mean'] * per_track / 1e3:.0f}" if per_track else ""
        rows.append([docs(n), f"{v['entered_per_day_mean']:,.0f}", f"{v['entered_per_day_max']:,}", kb])
    return table(["base pack", "tracks entering a day (mean)", "most in a day", "KB a day (mean)"], rows) + \
        f"\n\nNew tracks a day (first listen, mean of 30 days): {c['new_tracks_per_day_mean']:,.0f}, in a catalogue of {c['catalogue_tracks']:,} tracks."


def t_bytes():
    b = load(R, "recs_bytes.json")
    if not b:
        return "(not run)"
    return table(["tracks", "file bytes", "bytes / track"], [[f"{int(n):,}", f"{v['file_bytes']:,}", f"{v['bytes_per_track']:.0f}"] for n, v in b.items()])


def t_discovery():
    e = load(R, "recs_eval.json")
    if not e or "discovery_vec_cloud" not in e:
        return "(not run)"
    rows = []
    for k in ["discovery_decay_pop", "discovery_vec_cloud"] + [k for k in e if k.startswith("discovery_vec_device@")]:
        v = e[k]
        rows.append([k.replace("discovery_", ""), k.split("@")[1] if "@" in k else "", f"{v['ndcg@10']:.4f}", f"{v['recall@100']:.4f}"])
    return table(["source (user's own tracks left out)", "pack tracks", "NDCG@10", "recall@100 (of min(targets, 100))"], rows) + \
        f"\n\n{e['discovery_users']:,} of {e['history']['users']:,} test users listened to at least one track they had never played."


A = os.path.join(HERE, "aol")


def t_aol(m):
    e, b = load(A, "eval.json"), load(A, "bytes.json")
    if not e:
        return "(not run)"
    rows = []
    for n, v in e["methods"][m].items():
        t1, t10 = v["1"], v["10"]
        rows.append([docs(n), f"{b[m][n]['file_bytes'] / 1e6:.1f}" if b else "", pct(t1["held"]), pct(t1["proved"]), pct(t1["proved_pairs"]),
                     pct(t10["held"]), pct(t10["proved"]), pct(t10["proved_pairs"]), pct(t10["proved_or_cache_held"]["100000"]), v["violations"]])
    return table(["passages", "file MB", "top 1 held", "top 1 proved, words", "top 1 proved, pairs", "top 10 held", "top 10 proved, words",
                  "top 10 proved, pairs", "top 10 proved or cached (100K answers)", "wrong proofs"], rows)


def t_aol_cache():
    e, p = load(A, "eval.json"), load(A, "prep.json")
    if not e:
        return "(not run)"
    any_n = next(iter(e["methods"]["cov"].values()))["10"]["cache_ids"]
    q = load(S, "answer_cache.json")
    per = q["mean_query_bytes"] + 4 * q["k"] if q else 61
    rows = [[f"{int(c):,}", f"{int(c) * per / 1e6:.1f}", pct(v)] for c, v in any_n.items()]
    return table(["answers cached", "MB (ids only)", "test search volume answered"], rows) + \
        f"\n\n{pct(p['test_volume_seen_in_training'])} of test searches repeat a string searched in training at all."


PAIR_BYTES = 9  # two 4-byte term ids and a 1-byte bound, unpacked


def t_aol_budget():
    """The best ways to spend 50 MB: a slice, a result cache, and pair
    bounds for every training pair (counted unpacked) when the proof uses them."""
    e, b = load(A, "eval.json"), load(A, "bytes.json")
    if not e or not b:
        return "(not run)"
    q = load(S, "answer_cache.json")
    per = q["mean_query_bytes"] + 4 * q["k"]
    pairs_mb = e["train_pairs"] * PAIR_BYTES / 1e6
    rows = []
    for m in METHODS:
        for n, v in e["methods"][m].items():
            mb = b[m][n]["file_bytes"] / 1e6
            for c in v["10"]["cache_held"]:
                cmb = int(c) * per / 1e6
                for how, share, extra in (("cache", v["10"]["cache_held"][c], 0.0),
                                          ("cache + proof", v["10"]["proved_or_cache_held"][c], pairs_mb)):
                    tot = mb + cmb + extra
                    if tot <= 50:
                        rows.append((share, [m, docs(n), f"{mb:.1f}", f"{int(c):,}", f"{cmb:.1f}", f"{extra:.1f}" if extra else "", f"{tot:.1f}", how, pct(share)]))
    rows.sort(key=lambda r: -r[0])
    return table(["slice", "passages", "slice MB", "answers cached", "cache MB", "pair bounds MB", "MB on the device", "answered by",
                  "test volume answered exactly on the device"], [r for _, r in rows[:8]]) + \
        f"\n\nPair bounds are counted for all {e['train_pairs']:,} training pairs at {PAIR_BYTES} bytes each ({pairs_mb:.1f} MB); a slice needs only the pairs of words it holds, so this is an upper bound."


def t_aol_bands():
    e = load(A, "eval.json")
    if not e:
        return "(not run)"
    rows = []
    for n in ("100000", "400000"):
        for lbl, v in e["methods"]["cov"][n]["10"]["by_training_searches"].items():
            lo, hi = lbl.split("-")
            name = "new string" if lo == "0" else (f"{int(lo):,} to {int(hi):,}" if hi else f"{int(lo):,} or more")
            rows.append([docs(n), name, pct(v["volume"]), pct(v["held"]), pct(v["proved_pairs"])])
    return table(["passages", "training searches of the string", "share of test volume", "top 10 held", "top 10 proved, pairs"], rows)


TABLES = {"slices-cov": lambda: t_slices("cov"), "slices-pop": lambda: t_slices("pop"), "overlap": t_overlap,
          "lengths": t_lengths, "pairs": t_pairs, "confidence": t_confidence, "recs": t_recs, "churn": t_churn,
          "pack-bytes": t_bytes, "discovery": t_discovery,
          "aol-cov": lambda: t_aol("cov"), "aol-pop": lambda: t_aol("pop"), "aol-cache": t_aol_cache, "aol-budget": t_aol_budget,
          "aol-bands": t_aol_bands}


def main():
    p = os.path.join(HERE, "README.md")
    s = open(p).read()
    for name, fn in TABLES.items():
        s = re.sub(rf"(<!-- report:{name} -->\n).*?(<!-- /report -->)", lambda mt: mt.group(1) + fn() + "\n" + mt.group(2), s, flags=re.S)
    open(p, "w").write(s)


if __name__ == "__main__":
    main()
