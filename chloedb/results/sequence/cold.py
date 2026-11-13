"""Cold-start items on Amazon Beauty.

A tenth of the items, drawn at random, are new: every event of theirs is
taken out of training, as if they were listed after the models were
trained. Leave-one-out as elsewhere: a user's last item is the test,
ranked against every item (new ones too) but the user's own, and the
users are split by whether their test item is old (warm) or new.

    python cold.py embed   --dir DATA --work WORK
    python cold.py id      --dir DATA --work WORK
    python cold.py text    --dir DATA --work WORK
    python cold.py decoder --dir DATA --work WORK
    python cold.py rank    --dir DATA --work WORK

DATA holds reviews_Beauty_5.json.gz and meta_Beauty.json.gz (see
../generative/tiger.py). The stages:

  embed    each item's text as a vector (bge-small-en-v1.5, through
           sentence-transformers), to WORK/emb.npy and WORK/items.json.
  id       chloedb's sequence model with learned item vectors (as
           eval.py trains it): a new item has no vector, so it is never
           an answer.
  text     the same model reading and ranking the items' text vectors
           (learn_items=False): a new item's vector is its text's, there
           when the item is written.
  decoder  the decoder of ../generative over semantic IDs. The codebook
           is trained on the old items' text vectors
           (chloedb.train.codebook, 3 levels of 256), the new items are
           coded by it when they are written to the table, after the old
           ones, and the decoder trains on histories without them. At
           test each step's numbers are those of every item (new ones
           too) the user does not have.
  rank     from the saved tops:
           1. each source alone and their rrf;
           2. the decoder with TIGER's rule for unseen items: up to m of
              the top 10 go to new items that share the first L numbers
              of a decoded item, in the decoded items' order (then by
              the new item's text-vector cosine to that decoded item);
           3. a LightGBM ranker over the union of the id model's and the
              text model's top 100, with and without semantic-ID
              features (how many of the user's last 20 items, and
              whether the last one, share the candidate's first 1, 2 and
              3 numbers) and, as a control, with the text vectors
              themselves (the candidate's greatest cosine to the user's
              last 20 items, and its cosine to the last). Trained on
              half the users and scored on the other half, both ways. A
              last ranker also takes the decoder's top 20 as candidates,
              with its rank as a feature.

id, text and decoder write each user's top items (100, 100 and 20) to
WORK/cold, and rank writes WORK/cold/cold-rank.json. Needs torch, numpy,
sentence-transformers (embed) and lightgbm (rank) beside chloedb.
"""
import argparse
import json
import os
import random
import sys
import tempfile
import time
import types

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "generative"))
import tiger  # noqa: E402  (the decoder, its data reading and its metrics)

FILES = tiger.FILES["beauty"]


def cold_split(items, share=0.1, seed=0):
    rng = random.Random(seed)
    return set(rng.sample(items, int(len(items) * share)))


def load(a):
    raw = tiger.sequences(os.path.join(a.dir, FILES[0]))
    items = json.load(open(os.path.join(a.work, "items.json")))
    return raw, items, cold_split(items)


def by_slice(raw, cold, users, tops):
    def score(us):
        rows = [tiger.metrics(tops.get(u, [])[:10], raw[u][-1]) for u in us]
        return {**{k: round(v, 4) for k, v in tiger.mean_metrics(rows).items()}, "users": len(us)}
    return {"warm": score([u for u in users if raw[u][-1] not in cold]),
            "new": score([u for u in users if raw[u][-1] in cold]), "all": score(users)}


def cmd_embed(a):
    from sentence_transformers import SentenceTransformer
    os.makedirs(a.work, exist_ok=True)
    seqs = tiger.sequences(os.path.join(a.dir, FILES[0]))
    items = sorted({i for s in seqs.values() for i in s})
    text = tiger.texts(os.path.join(a.dir, FILES[1]), set(items))
    for i in items:
        text.setdefault(i, f"Item {i}")
    m = SentenceTransformer("BAAI/bge-small-en-v1.5", device="cpu")
    t0 = time.time()
    E = m.encode([text[i] for i in items], batch_size=64, normalize_embeddings=True, show_progress_bar=False)
    print(f"{len(items)} items embedded in {time.time() - t0:.0f} s")
    np.save(os.path.join(a.work, "emb.npy"), E.astype(np.float32))
    json.dump(items, open(os.path.join(a.work, "items.json"), "w"))


def cmd_sequence(a):
    import torch
    from chloedb import _sequence as S
    torch.set_num_threads(a.threads)
    raw, items, cold = load(a)
    mode = a.stage
    warm = [i for i in items if i not in cold]
    if mode == "id":
        g = torch.Generator().manual_seed(a.seed)
        start = torch.nn.functional.normalize(torch.randn(len(warm), 64, generator=g), dim=-1)
        vectors = {k: start[n].tolist() for n, k in enumerate(warm)}
    else:
        X = np.load(os.path.join(a.work, "emb.npy"))
        at = {k: n for n, k in enumerate(items)}
        vectors = {k: X[at[k]].tolist() for k in warm}
    ev = lambda s: [(i, None, None) for i in s]
    # Training: each user's items before the last, the new items left out
    # (the trainer holds the last of those out to validate).
    train = {u: ev([i for i in s[:-1] if i not in cold]) for u, s in raw.items()}
    cfg = {"kind": "sequence", "table": "items", "field": "emb", "dim": None, "width": 64, "layers": 2, "heads": 1,
           "ffn": 256, "max_len": 50, "gap_buckets": 32, "actions": [""], "eps": S.EPS}
    t0 = time.time()
    model, (keys, E), _, hist = S.train(
        train, None, vectors, cfg, learn_items=mode == "id", negatives="all", temperature=0.05, epochs=1000, batch=128,
        lr=1e-3, dropout=0.2, holdout=True, patience=a.patience, seed=a.seed,
        log=lambda r: print(json.dumps(r), flush=True), max_seconds=a.minutes * 60)
    # Ranking: the learned vectors (id), or every item's text vector (text).
    if mode == "text":
        keys, E = items, torch.from_numpy(X)
    index = {k: n for n, k in enumerate(keys)}
    users = [u for u, s in raw.items() if len(s) >= 3]
    tops, model = {}, model.eval()
    with torch.no_grad():
        for j in range(0, len(users), 256):
            us = users[j:j + 256]
            steps = S.Steps()
            for u in us:
                it, ac, gp = S.history(ev(raw[u][:-1]), None, index, {"": 0}, cfg)
                steps.add(it, ac, gp, [0] * len(it))
            keep = [n for n in range(len(us)) if steps.items[n]]
            if not keep:
                continue
            sub = S.Steps()
            for n in keep:
                sub.add(steps.items[n], steps.actions[n], steps.gaps[n], steps.targets[n])
            last = torch.tensor([len(x) - 1 for x in sub.items])
            for it, ac, gp, _ in S._batches(torch, sub, list(range(len(sub))), len(sub), None):
                out = model(E[it], ac, gp)[torch.arange(len(keep)), last]
            sc = out @ E.T
            for r, n in enumerate(keep):
                sc[r, [index[i] for i in raw[us[n]][:-1] if i in index]] = float("-inf")
            top = sc.topk(100, dim=1).indices.tolist()
            for r, n in enumerate(keep):
                tops[us[n]] = [keys[x] for x in top[r]]
    res = {"mode": mode, "minutes": a.minutes, "patience": a.patience, "seed": a.seed, "threads": a.threads,
           "items": len(items), "new_items": len(cold), **by_slice(raw, cold, users, tops), "epochs": len(hist),
           "best_valid": max(hist, key=lambda r: r["ndcg@10"]), "train_seconds": round(time.time() - t0), "history": hist}
    print("TEST", json.dumps({k: res[k] for k in ("warm", "new", "all")}), flush=True)
    out = os.path.join(a.work, "cold")
    os.makedirs(out, exist_ok=True)
    json.dump(res, open(os.path.join(out, f"cold-{mode}.json"), "w"), indent=1)
    json.dump(tops, open(os.path.join(out, f"cold-{mode}-tops.json"), "w"))


def cmd_decoder(a):
    import torch
    import chloedb
    import chloedb.train
    torch.set_num_threads(a.threads)
    raw, items, cold = load(a)
    Xn = np.load(os.path.join(a.work, "emb.npy")).astype(np.float32)
    out = os.path.join(a.work, "cold")
    os.makedirs(out, exist_ok=True)
    # The codebook as ChloeDB trains it, on the old items' vectors; then
    # every item written to a table with a semantic-ID field, the old
    # items first and the new after, as they would arrive, and each
    # item's code read back.
    order = sorted(range(len(items)), key=lambda n: (items[n] in cold, n))
    with tempfile.TemporaryDirectory() as d:
        chloedb.train.codebook(Xn[[n for n in order if items[n] not in cold]], "cb", models=d)
        db = chloedb.Database.create(os.path.join(d, "db"), {"table": "items", "key": "id", "fields": [
            {"name": "id", "type": "keyword"}, {"name": "emb", "type": "vector", "dim": Xn.shape[1], "index": "flat"},
            {"name": "sid", "type": "keyword", "multi": True, "semantic_id": {"from": "emb", "codebook": "cb"}}]}, models=d)
        for j in range(0, len(order), 2000):
            db.insert([{"id": items[n], "emb": Xn[n].tolist()} for n in order[j:j + 2000]])
        codes = {k: s[-1] for k, s in db.query(f"SELECT id, sid FROM items LIMIT {len(items)}").rows}
        db.close()
    assert len(codes) == len(items) and len(set(codes.values())) == len(items)
    seen = {}
    for c in codes.values():
        seen[c.rsplit(".", 1)[0]] = seen.get(c.rsplit(".", 1)[0], 0) + 1
    json.dump({"codes": codes, "info": {
        "max_tie": max(seen.values()) - 1, "codebook": "chloedb.train.codebook on the old items, 3 levels of 256",
        "new_items_sharing_an_old_items_code": sum(1 for i in cold if not codes[i].endswith(".0"))}},
        open(os.path.join(out, "codes.json"), "w"))
    # tiger.tiger trains on s[:-2] and validates on s[-2]: give it the old
    # items before the test item, and a last item it never reads.
    tr = {}
    for u, s in raw.items():
        f = [i for i in s[:-1] if i not in cold]
        if len(f) >= 2:
            tr[u] = f + [f[-1]]
    t = types.SimpleNamespace(seed=a.seed, tiger_width=128, tiger_heads=4, tiger_layers=4, tiger_ffn=1024, tiger_tokens="item",
                              tiger_lr=1e-3, tiger_epochs=200, batch=48, patience=5, beam=20, val_users=2000, max_minutes=a.minutes)
    t0 = time.time()
    tiger.tiger(t, tr, out)
    saved = torch.load(os.path.join(out, "tiger.pt"))
    model = tiger.Tiger(**saved["config"], dropout=0.1)
    model.load_state_dict(saved["state"])
    model.eval()
    pc = {k: tiger.parse(v) for k, v in codes.items()}
    inv = {tuple(c): i for i, c in pc.items()}
    trie = tiger.Trie(pc)
    users = [u for u, s in raw.items() if len(s) >= 3]
    tops = {}
    for j in range(0, len(users), 64):
        us = users[j:j + 64]
        srcs = [tiger.source(model, u, raw[u][:-1], pc) for u in us]
        got = tiger.beam_search(model, srcs, lambda k, ps, us=us: trie.allowed(raw[us[k]][:-1], ps), beam=20)
        for u, g in zip(us, got):
            tops[u] = [inv[tuple(c)] for c in g]
    res = {**by_slice(raw, cold, users, tops), "train_seconds": round(time.time() - t0), "history": saved["history"]}
    print("TEST", json.dumps({k: res[k] for k in ("warm", "new", "all")}), flush=True)
    json.dump(res, open(os.path.join(out, "cold-decoder.json"), "w"), indent=1)
    json.dump(tops, open(os.path.join(out, "cold-decoder-tops.json"), "w"))


def cmd_rank(a):
    raw, items, cold = load(a)
    out = os.path.join(a.work, "cold")
    tops = {m: json.load(open(os.path.join(out, f"cold-{m}-tops.json"))) for m in ("id", "text")}
    dec = os.path.join(out, "cold-decoder-tops.json")
    if os.path.exists(dec):
        tops["decoder"] = json.load(open(dec))
    users = [u for u, s in raw.items() if len(s) >= 3]
    slices = {"warm": [u for u in users if raw[u][-1] not in cold], "new": [u for u in users if raw[u][-1] in cold], "all": users}

    def score(ranked):
        res = {}
        for name, us in slices.items():
            m = tiger.mean_metrics([tiger.metrics(ranked.get(u, [])[:10], raw[u][-1]) for u in us])
            res[name] = {"recall@10": round(m["recall@10"], 4), "ndcg@10": round(m["ndcg@10"], 4), "users": len(us)}
        return res

    def rrf(names, k=60):
        res = {}
        for u in users:
            sc = {}
            for n in names:
                for r, i in enumerate(tops[n].get(u, [])):
                    sc[i] = sc.get(i, 0.0) + 1 / (k + r + 1)
            res[u] = sorted(sc, key=lambda i: -sc[i])[:100]
        return res

    res = {"new_items": len(cold), "sources": {m: score(t) for m, t in tops.items()}}
    combos = [("id", "text")] + ([("id", "decoder"), ("text", "decoder"), ("id", "text", "decoder")] if "decoder" in tops else [])
    for c in combos:
        res["sources"]["rrf(" + ", ".join(c) + ")"] = score(rrf(c))
    # How often a source's top 10 holds a new item at all.
    for m, t in tops.items():
        res["sources"][m]["top10_new_share"] = round(float(np.mean([sum(i in cold for i in t.get(u, [])[:10]) / 10 for u in users])), 4)
    for k, v in res["sources"].items():
        print(k, json.dumps(v), flush=True)

    codes = {k: v.split(".")[:3] for k, v in json.load(open(os.path.join(out, "codes.json")))["codes"].items()}
    E = np.load(os.path.join(a.work, "emb.npy")).astype(np.float32)
    row = {k: n for n, k in enumerate(items)}
    if "decoder" in tops:
        for L in (3, 2):
            by = {}
            for i in cold:
                by.setdefault(tuple(codes[i][:L]), []).append(i)
            old = {tuple(codes[i][:L]) for i in items if i not in cold}
            res["new_items_under_an_old_items_first_%d" % L] = sum(len(v) for k, v in by.items() if k in old)
            for m in (1, 2, 5):
                ranked = {}
                for u in users:
                    t = tops["decoder"].get(u, [])
                    has, extra = set(raw[u][:-1]), []
                    for d in t:
                        near = [i for i in by.get(tuple(codes[d][:L]), ()) if i not in has and i not in extra and i not in t[:10 - m]]
                        near.sort(key=lambda i: -float(E[row[i]] @ E[row[d]]))
                        extra += near
                        if len(extra) >= m:
                            break
                    extra = extra[:m]
                    ranked[u] = t[:10 - len(extra)] + extra
                name = "decoder, up to %d of 10 to new items under a decoded first %d" % (m, L)
                res["sources"][name] = score(ranked)
                print(name, json.dumps(res["sources"][name]), flush=True)

    if not a.no_ranker:
        import lightgbm as lgb

        def rows_of(u):
            cand = list(dict.fromkeys(tops["id"].get(u, []) + tops["text"].get(u, []) + tops.get("decoder", {}).get(u, [])))
            rdc = {i: r for r, i in enumerate(tops.get("decoder", {}).get(u, []))}
            rid = {i: r for r, i in enumerate(tops["id"].get(u, []))}
            rtx = {i: r for r, i in enumerate(tops["text"].get(u, []))}
            hist = raw[u][:-1][-20:]
            hc = [codes[i] for i in hist]
            X = []
            sims = E[[row[i] for i in cand]] @ E[[row[i] for i in hist]].T if cand else np.zeros((0, 1))
            for n, i in enumerate(cand):
                c = codes[i]
                f = [1 / (60 + rid[i] + 1) if i in rid else 0.0, 1 / (60 + rtx[i] + 1) if i in rtx else 0.0]
                for L in (1, 2, 3):
                    f.append(sum(h[:L] == c[:L] for h in hc))
                    f.append(float(hc[-1][:L] == c[:L]))
                f += [float(sims[n].max()), float(sims[n, -1])]
                f += [1 / (60 + rdc[i] + 1) if i in rdc else 0.0, float(i in rid or i in rtx)]
                X.append(f)
            return cand, np.asarray(X, dtype=np.float32), np.asarray([float(i == raw[u][-1]) for i in cand], dtype=np.float32)

        data = {u: rows_of(u) for u in users}
        fold = {u: n % 2 for n, u in enumerate(users)}
        for name, cols in (("ranker: id and text ranks", [0, 1]), ("ranker: with semantic-ID features", list(range(8))),
                           ("ranker: with text-vector similarity to the history", [0, 1, 8, 9]), ("ranker: with both", list(range(10))),
                           ("ranker: with both, and the decoder's top 20 and rank", list(range(11)))):
            # Column 11 says the candidate came from the id or the text
            # model: every ranker but the last sees those candidates only.
            keep = (lambda u: np.ones(len(data[u][0]), dtype=bool)) if 10 in cols else (lambda u: data[u][1][:, 11] > 0)
            ranked = {}
            for f in (0, 1):
                tr = [u for u in users if fold[u] != f and data[u][2][keep(u)].sum() > 0]
                X = np.concatenate([data[u][1][keep(u)][:, cols] for u in tr])
                y = np.concatenate([data[u][2][keep(u)] for u in tr])
                g = [int(keep(u).sum()) for u in tr]
                model = lgb.LGBMRanker(objective="lambdarank", n_estimators=200, learning_rate=0.05, num_leaves=31,
                                       min_child_samples=50, verbose=-1, random_state=0, n_jobs=2)
                model.fit(X, y, group=g)
                for u in users:
                    if fold[u] == f:
                        k = keep(u)
                        cand, Xu = [c for c, ok in zip(data[u][0], k) if ok], data[u][1][k]
                        if cand:
                            s = model.predict(Xu[:, cols])
                            ranked[u] = [cand[i] for i in np.argsort(-s, kind="stable")[:100]]
                if len(cols) == 8:
                    names = ["id_rr", "text_rr", "hist_l1", "last_l1", "hist_l2", "last_l2", "hist_l3", "last_l3"]
                    res.setdefault("feature_gain", []).append(dict(zip(names, [float(x) for x in model.booster_.feature_importance("gain")])))
            res["sources"][name] = score(ranked)
            print(name, json.dumps(res["sources"][name]), flush=True)
    json.dump(res, open(os.path.join(out, "cold-rank.json"), "w"), indent=1)


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("stage", choices=["embed", "id", "text", "decoder", "rank"])
    ap.add_argument("--dir", required=True, help="the folder with Beauty's two files")
    ap.add_argument("--work", required=True)
    ap.add_argument("--minutes", type=float, default=None, help="training time limit (id, text: 45; decoder: 60)")
    ap.add_argument("--threads", type=int, default=None, help="torch threads (id, text: 2; decoder: 4)")
    ap.add_argument("--patience", type=int, default=10)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--no-ranker", action="store_true")
    a = ap.parse_args()
    if a.minutes is None:
        a.minutes = 60 if a.stage == "decoder" else 45
    if a.threads is None:
        a.threads = 4 if a.stage == "decoder" else 2
    {"embed": cmd_embed, "id": cmd_sequence, "text": cmd_sequence, "decoder": cmd_decoder, "rank": cmd_rank}[a.stage](a)
