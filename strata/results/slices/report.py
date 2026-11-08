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


def t_aol(m, d=A):
    e, b = load(d, "eval.json"), load(d, "bytes.json")
    if not e:
        return "(not run)"
    rows = []
    for n, v in e["methods"][m].items():
        t1, t10 = v["1"], v["10"]
        rows.append([docs(n), f"{b[m][n]['file_bytes'] / 1e6:.1f}" if b else "", pct(t1["held"]), pct(t1["proved"]), pct(t1["proved_pairs"]),
                     pct(t10["held"]), pct(t10["proved"]), pct(t10["proved_pairs"]), pct(t10["proved_or_cache_held"]["100000"]), v["violations"]])
    return table(["passages", "file MB", "top 1 held", "top 1 proved, words", "top 1 proved, pairs", "top 10 held", "top 10 proved, words",
                  "top 10 proved, pairs", "top 10 proved or cached (100K answers)", "wrong proofs"], rows)


def t_aol_cache(d=A):
    e, p = load(d, "eval.json"), load(d, "prep.json")
    if not e:
        return "(not run)"
    any_n = next(iter(e["methods"]["cov"].values()))["10"]["cache_ids"]
    q = load(S, "answer_cache.json")
    per = q["mean_query_bytes"] + 4 * q["k"] if q else 61
    rows = [[f"{int(c):,}", f"{int(c) * per / 1e6:.1f}", pct(v)] for c, v in any_n.items()]
    out = table(["answers cached", "MB (ids only)", "test search volume answered"], rows)
    if "test_volume_seen_in_training" in p:
        out += f"\n\n{pct(p['test_volume_seen_in_training'])} of test searches repeat a string searched in training at all."
    return out


PAIR_BYTES = 9  # two 4-byte term ids and a 1-byte bound, unpacked


def budget(d):
    """The ways to spend 50 MB, best first: a slice, a result cache, and pair
    bounds for every training pair (counted unpacked) when the proof uses them."""
    e, b = load(d, "eval.json"), load(d, "bytes.json")
    if not e or not b:
        return None
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
                        rows.append((share, [m, docs(n), f"{mb:.1f}", f"{int(c):,}", f"{cmb:.1f}", f"{extra:.1f}" if extra else "", f"{tot:.1f}", how, pct(share)],
                                     (m, n, how)))
    rows.sort(key=lambda r: -r[0])
    return rows, pairs_mb, e


def t_aol_budget(d=A):
    got = budget(d)
    if not got:
        return "(not run)"
    rows, pairs_mb, e = got
    return table(["slice", "passages", "slice MB", "answers cached", "cache MB", "pair bounds MB", "MB on the device", "answered by",
                  "test volume answered exactly on the device"], [r for _, r, _ in rows[:8]]) + \
        f"\n\nPair bounds are counted for all {e['train_pairs']:,} training pairs at {PAIR_BYTES} bytes each ({pairs_mb:.1f} MB); a slice needs only the pairs of words it holds, so this is an upper bound."


def zipf_ranks(p):
    """For Zipf traffic, the rank range whose expected training searches
    fall in each band (counts are T * r^-s / H, rounded)."""
    s, n = p["zipf_s"], p["strings"]
    t = p.get("train_searches") or load(A, "prep.json")["train_searches"]
    h = sum(r ** -s for r in range(1, n + 1))
    def last(lo):  # the last rank expected to be searched lo times or more
        r = int((t / (h * (lo - 0.5))) ** (1 / s))
        while r > 0 and round(t * r ** -s / h) < lo:
            r -= 1
        while round(t * (r + 1) ** -s / h) >= lo:
            r += 1
        return min(r, n)
    return {lo: last(lo) for lo in (1, 10, 100, 1000)}


def t_aol_bands(d=A):
    e, p = load(d, "eval.json"), load(d, "prep.json")
    if not e:
        return "(not run)"
    ranks = zipf_ranks(p) if "zipf_s" in p else None
    rows = []
    for n in ("100000", "400000"):
        for lbl, v in e["methods"]["cov"][n]["10"]["by_training_searches"].items():
            lo, hi = lbl.split("-")
            if ranks and lo == "0":
                continue  # no Zipf string is new
            name = "new string" if lo == "0" else (f"{int(lo):,} to {int(hi):,}" if hi else f"{int(lo):,} or more")
            if ranks:
                first = ranks[int(hi) + 1] + 1 if hi else 1
                name += f" (ranks {first:,} to {ranks[int(lo)]:,})"
            rows.append([docs(n), name, pct(v["volume"]), pct(v["held"]), pct(v["proved_pairs"])])
    return table(["passages", "training searches of the string", "share of test volume", "top 10 held", "top 10 proved, pairs"], rows)


Z = os.path.join(HERE, "zipf")
Z086 = os.path.join(HERE, "zipf086")


def t_zipf_summary():
    """One row per traffic shape: how concentrated it is, and what a 33 MB
    slice, a 10,000-answer cache and the best mix within 50 MB answer."""
    rows = []
    for name, d in (("AOL log, measured", A), ("Zipf s = 0.86 over 1M strings", Z086), ("Zipf s = 1 over 1M strings", Z)):
        e, p, got = load(d, "eval.json"), load(d, "prep.json"), budget(d)
        if not e or not got:
            continue
        v = e["methods"]["cov"]["100000"]["10"]
        top = load(Z, "prep.json")["aol_top_share_of_training"]["10000"] if d == A else p["top_share"]["10000"]
        new = v["by_training_searches"]["0-0"]["volume"]
        best = got[0][0][:2]
        rows.append([name, pct(top), pct(new), pct(v["held"]), pct(v["proved_pairs"]), pct(v["cache_ids"]["10000"]),
                     f"{pct(best[0])} ({best[1][7]}, {best[1][6]} MB)", "pass" if best[0] >= 0.4 else "fail"])
    return table(["traffic", "top 10,000 strings' share of searches", "test volume on new strings", "100K slice: top 10 held",
                  "100K slice: top 10 proved, pairs", "10,000 cached answers (0.6 MB)", "best exact answer within 50 MB", "Gate A (40%)"], rows)


def zipf_heads(sv, n, at=(10_000, 1_000_000)):
    """Shares of searches the top k strings carry under Zipf s over n strings."""
    tot, cut = 0.0, {}
    for r in range(1, n + 1):
        tot += r ** -sv
        if r in at:
            cut[r] = tot
    return {k: cut[k] / tot for k in at}


def t_zipf_tail():
    """Zipf over as many strings as AOL's training had (8.7M), of which only
    the top 1M were measured: the top 1M carry their share of searches with
    the rates measured over 1M (the law is the same once cut at 1M), and the
    rest are counted at the rates AOL's new strings were measured at (held,
    and proved with pairs; never cached)."""
    a, ap = load(A, "eval.json"), load(A, "prep.json")
    if not a:
        return "(not run)"
    universe = ap["train_distinct"]
    rows = []
    for d in (Z086, Z):
        e, p, got = load(d, "eval.json"), load(d, "prep.json"), budget(d)
        if not e or not got:
            continue
        head = zipf_heads(p["zipf_s"], universe)
        f = head[1_000_000]
        new = lambda m, n: a["methods"][m][n]["10"]["by_training_searches"]["0-0"]
        z, t = e["methods"]["cov"]["100000"]["10"], new("cov", "100000")
        best = max((f * sh + (1 - f) * (new(m, n)["proved_pairs"] if how == "cache + proof" else 0.0), r)
                   for sh, r, (m, n, how) in got[0])
        rows.append([f"Zipf s = {p['zipf_s']:g} over {universe / 1e6:.1f}M strings", pct(head[10_000]), pct(f), pct(f * z["held"] + (1 - f) * t["held"]),
                     pct(f * z["proved_pairs"] + (1 - f) * t["proved_pairs"]), pct(f * z["cache_ids"]["10000"]),
                     f"{pct(best[0])} ({best[1][7]}, {best[1][6]} MB)", "pass" if best[0] >= 0.4 else "fail"])
    return table(["traffic", "top 10,000 strings' share of searches", "top 1,000,000 strings' share", "100K slice: top 10 held",
                  "100K slice: top 10 proved, pairs", "10,000 cached answers (0.6 MB)", "best exact answer within 50 MB", "Gate A (40%)"], rows)


TABLES = {"slices-cov": lambda: t_slices("cov"), "slices-pop": lambda: t_slices("pop"), "overlap": t_overlap,
          "lengths": t_lengths, "pairs": t_pairs, "confidence": t_confidence, "recs": t_recs, "churn": t_churn,
          "pack-bytes": t_bytes, "discovery": t_discovery,
          "aol-cov": lambda: t_aol("cov"), "aol-pop": lambda: t_aol("pop"), "aol-cache": t_aol_cache, "aol-budget": t_aol_budget,
          "aol-bands": t_aol_bands, "zipf-summary": t_zipf_summary, "zipf-cov": lambda: t_aol("cov", Z), "zipf-pop": lambda: t_aol("pop", Z),
          "zipf-bands": lambda: t_aol_bands(Z), "zipf-cache": lambda: t_aol_cache(Z), "zipf-budget": lambda: t_aol_budget(Z),
          "zipf-tail": t_zipf_tail}


def main():
    p = os.path.join(HERE, "README.md")
    s = open(p).read()
    for name, fn in TABLES.items():
        s = re.sub(rf"(<!-- report:{name} -->\n).*?(<!-- /report -->)", lambda mt: mt.group(1) + fn() + "\n" + mt.group(2), s, flags=re.S)
    open(p, "w").write(s)


if __name__ == "__main__":
    main()
