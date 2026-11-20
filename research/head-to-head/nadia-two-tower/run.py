"""Nadia's head-to-head: next-movie recommendation on MovieLens-1M, ChloeDB's
recommender against a two-tower model.

Nadia (persona study, round 4) found `recommend()` beat her tuned
co-occurrence baseline; her head of ML wants "an offline comparison against
our two-tower model". This trains one, as such a team would (PyTorch: a user
tower over the user's recent items, an item tower, in-batch softmax with a
popularity correction, tuned on validation), on her protocol, and puts it
beside ChloeDB:

- the protocol: every rating an interaction, each user's ratings in time
  order (ties in file order); the last is the test, the one before it the
  validation, the rest training; every item ranked but the user's own;
  hit@10, NDCG@10, MRR@10 over all 6,040 users, with a paired bootstrap;
- the sources: popularity and windowed co-occurrence (numpy baselines), the
  two-tower model scored in numpy, `recommend('ml', :u)` with
  `id NOT IN (...)` on chloedb-server (no tuning), the two-tower's vectors
  served from ChloeDB (`ORDER BY similarity(emb, :v)`), and the two fused in
  one statement (`rrf(recommend(...), similarity(...))`);
- the cost: training time, the time to load the events and count them,
  p50/p95 of a recommendation, and memory.

    python3 run.py          # downloads ratings.csv (pinned by hash), writes raw/results.json

The data is MovieLens-1M's ratings (GroupLens; the copy scripts/ci-local.sh
pins on Hugging Face), used under its terms for research and not committed.
"""

import csv
import hashlib
import json
import math
import os
import random
import sys
import time
import urllib.request

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import common  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
WORK = os.environ.get("H2H_WORK", "/tmp/h2h-nadia")
URL = "https://huggingface.co/datasets/hazemessam/ml-1m/resolve/b7ed0ffb2157bfbc794a1e47e9044ab323bee57f/ratings.csv"
SHA = "d2baa264e8ef4c7fb0c555acae5dc80239612481efff5814df9eab98cb9e1fe0"
K = 10
SEED = 0


def fetch():
    path = os.path.join(WORK, "dl", "ratings.csv")
    if not os.path.exists(path):
        os.makedirs(os.path.dirname(path), exist_ok=True)
        urllib.request.urlretrieve(URL, path)
    h = hashlib.sha256(open(path, "rb").read()).hexdigest()
    if h != SHA:
        raise SystemExit(f"ratings.csv: sha256 {h}, expected {SHA}")
    return path


def load(path):
    seqs = {}
    with open(path, newline="") as f:
        for i, row in enumerate(csv.DictReader(f)):
            seqs.setdefault(int(row["userId"]), []).append((int(row["timestamp"].strip()), i, int(row["movieId"])))
    out = {}
    for u, evs in seqs.items():
        evs.sort()
        out[u] = [(t, m) for t, _, m in evs]
    return out


def split(seqs):
    """train, validation target, test target, per user."""
    train, val, test = {}, {}, {}
    for u, evs in seqs.items():
        train[u] = evs[:-2]
        val[u] = evs[-2]
        test[u] = evs[-1]
    return train, val, test


# --- scoring ------------------------------------------------------------------

def metrics(ranked, target):
    if target in ranked[:K]:
        r = ranked.index(target)
        return 1.0, 1 / math.log2(r + 2), 1 / (r + 1)
    return 0.0, 0.0, 0.0


def summary(per_user):
    a = np.array(list(per_user.values()))
    return {"hit@10": round(float(a[:, 0].mean()), 4), "ndcg@10": round(float(a[:, 1].mean()), 4), "mrr@10": round(float(a[:, 2].mean()), 4)}


def bootstrap(a, b, n=2000, seed=1):
    """Paired bootstrap of a minus b over users: hit@10 and NDCG@10, 95%."""
    users = sorted(a)
    x = np.array([a[u] for u in users])
    y = np.array([b[u] for u in users])
    rng = np.random.default_rng(seed)
    d = x - y
    idx = rng.integers(0, len(users), size=(n, len(users)))
    m = d[idx].mean(axis=1)
    out = {}
    for j, name in ((0, "hit@10"), (1, "ndcg@10")):
        lo, hi = np.percentile(m[:, j], [2.5, 97.5])
        out[name] = {"diff": round(float(d[:, j].mean()), 4), "lo": round(float(lo), 4), "hi": round(float(hi), 4)}
    return out


def top_from_scores(scores, seen, k=K):
    s = scores.copy()
    s[list(seen)] = -np.inf
    top = np.argpartition(-s, k)[:k]
    return list(top[np.argsort(-s[top])])


# --- baselines ----------------------------------------------------------------

def popularity(history, items):
    counts = np.zeros(max(items) + 1)
    for evs in history.values():
        for _, m in evs:
            counts[m] += 1
    return counts


def cooc_model(history, n, window, cosine):
    from collections import defaultdict
    pair = defaultdict(float)
    cnt = np.zeros(n)
    for evs in history.values():
        ms = [m for _, m in evs]
        for i, a in enumerate(ms):
            cnt[a] += 1
            for b in ms[max(0, i - window):i]:
                pair[(a, b)] += 1
                pair[(b, a)] += 1
    nbr = defaultdict(list)
    for (a, b), c in pair.items():
        nbr[a].append((b, c / math.sqrt(cnt[a] * cnt[b]) if cosine else c))
    return {a: (np.array([b for b, _ in v]), np.array([c for _, c in v])) for a, v in nbr.items()}


def cooc_scores(model, recent, n, decay):
    s = np.zeros(n)
    for j, m in enumerate(reversed(recent)):
        if m in model:
            ids, w = model[m]
            s[ids] += w * (decay ** j)
    return s


# --- the two-tower model ------------------------------------------------------

def two_tower(history, n_items, pop, dim, hist_len, epochs, seed=SEED, log=None):
    """A user tower (the last hist_len items' embeddings, mean-pooled with
    recency weights, through a two-layer MLP) and an item tower (an item
    embedding through a linear layer), trained to put each training event's
    item first among the batch's other items (in-batch softmax, the logQ
    correction for popularity), temperature 0.05."""
    import torch
    torch.manual_seed(seed)
    random.seed(seed)
    pairs = []
    for u, evs in history.items():
        ms = [m for _, m in evs]
        for i in range(1, len(ms)):
            pairs.append((u, i))
    seqs = {u: [m for _, m in evs] for u, evs in history.items()}  # movie ids start at 1: 0 is padding

    class Model(torch.nn.Module):
        def __init__(self):
            super().__init__()
            self.emb = torch.nn.Embedding(n_items + 1, dim, padding_idx=0)
            self.user = torch.nn.Sequential(torch.nn.Linear(dim, 2 * dim), torch.nn.ReLU(), torch.nn.Linear(2 * dim, dim))
            self.item = torch.nn.Linear(dim, dim)
            self.pos = torch.nn.Parameter(torch.linspace(1.0, 0.3, hist_len))

        def users(self, hist):
            e = self.emb(hist)                       # B, L, d
            mask = (hist > 0).float().unsqueeze(-1)
            w = self.pos.view(1, -1, 1) * mask
            pooled = (e * w).sum(1) / w.sum(1).clamp(min=1e-6)
            return torch.nn.functional.normalize(self.user(pooled), dim=-1)

        def items(self, ids):
            return torch.nn.functional.normalize(self.item(self.emb(ids)), dim=-1)

    model = Model()
    opt = torch.optim.Adam(model.parameters(), lr=2e-3)
    logq = torch.tensor(np.log(pop / pop.sum() + 1e-12), dtype=torch.float32)

    def hist_of(u, i):
        h = seqs[u][max(0, i - hist_len):i][::-1]  # most recent first
        return h + [0] * (hist_len - len(h))

    batch = 1024
    t0 = time.perf_counter()
    for ep in range(epochs):
        random.shuffle(pairs)
        model.train()
        total = 0.0
        for s in range(0, len(pairs), batch):
            chunk = pairs[s:s + batch]
            hist = torch.tensor([hist_of(u, i) for u, i in chunk])
            tgt = torch.tensor([seqs[u][i] for u, i in chunk])
            uv = model.users(hist)
            iv = model.items(tgt)
            logits = uv @ iv.T / 0.05 - logq[tgt].unsqueeze(0)
            same = tgt.unsqueeze(0) == tgt.unsqueeze(1)
            logits = logits.masked_fill(same & ~torch.eye(len(chunk), dtype=torch.bool), -1e9)
            loss = torch.nn.functional.cross_entropy(logits, torch.arange(len(chunk)))
            opt.zero_grad()
            loss.backward()
            opt.step()
            total += float(loss) * len(chunk)
        if log:
            log(f"  epoch {ep + 1}: loss {total / len(pairs):.4f} ({time.perf_counter() - t0:.0f} s)")
    model.eval()
    with torch.no_grad():
        item_vecs = model.items(torch.arange(0, n_items + 1)).numpy()  # row m is movie m

    def user_vec(evs):
        h = [m for _, m in evs][-hist_len:][::-1]
        h = h + [0] * (hist_len - len(h))
        with torch.no_grad():
            return model.users(torch.tensor([h])).numpy()[0]
    return item_vecs, user_vec, time.perf_counter() - t0


def evaluate(history, targets, score_fn, n):
    per = {}
    for u, (_, target) in targets.items():
        seen = {m for _, m in history[u]}
        per[u] = metrics(top_from_scores(score_fn(u), seen), target)
    return per


def main():
    log = lambda *a: print(*a, flush=True)  # noqa: E731
    seqs = load(fetch())
    train, val, test = split(seqs)
    items = sorted({m for evs in seqs.values() for _, m in evs})
    n = max(items) + 1
    trainval = {u: train[u] + [val[u]] for u in train}
    result = {"machine": common.machine(), "users": len(seqs), "items": len(items), "ratings": sum(len(v) for v in seqs.values()),
              "at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "sources": {}, "tuning": {}}
    per_test = {}
    exists = np.zeros(n, dtype=bool)
    exists[items] = True

    # popularity
    pop = popularity(trainval, items)
    p = np.where(exists, pop, -np.inf)
    per_test["popularity"] = evaluate(trainval, test, lambda u: p, n)

    # co-occurrence, tuned on validation (Nadia's grid, smaller)
    best = None
    for window in (3, 5):
        for cosine in (True,):
            model = cooc_model(train, n, window, cosine)
            for hist in (20, 50):
                for decay in (0.8, 0.9):
                    per = evaluate(train, val, lambda u: np.where(exists, cooc_scores(model, [m for _, m in train[u]][-hist:], n, decay), -np.inf), n)
                    s = summary(per)["hit@10"]
                    result["tuning"].setdefault("cooc", []).append({"window": window, "history": hist, "decay": decay, "val_hit@10": s})
                    if best is None or s > best[0]:
                        best = (s, window, cosine, hist, decay)
    _, window, cosine, hist, decay = best
    model = cooc_model(trainval, n, window, cosine)
    per_test["cooc"] = evaluate(trainval, test, lambda u: np.where(exists, cooc_scores(model, [m for _, m in trainval[u]][-hist:], n, decay), -np.inf), n)
    result["cooc_setting"] = {"window": window, "history": hist, "decay": decay}
    log("cooc", summary(per_test["cooc"]))

    # two-tower: tuned on validation, then trained on train + validation for the test
    n_items = max(items)
    grid = [(64, 50, 6), (128, 50, 6), (64, 20, 6), (128, 100, 8)]
    best = None
    for dim, hl, ep in grid:
        log(f"two-tower dim {dim} history {hl} epochs {ep} (validation)")
        iv, uv, secs = two_tower(train, n_items, popularity(train, items), dim, hl, ep, log=log)
        full = iv
        per = evaluate(train, val, lambda u: np.where(exists, full @ uv(train[u]), -np.inf), n)
        s = summary(per)["hit@10"]
        result["tuning"].setdefault("two_tower", []).append({"dim": dim, "history": hl, "epochs": ep, "val_hit@10": s, "train_s": round(secs, 1)})
        log("  validation hit@10", s)
        if best is None or s > best[0]:
            best = (s, dim, hl, ep)
    _, dim, hl, ep = best
    log(f"two-tower final: dim {dim} history {hl} epochs {ep}")
    iv, uv, secs = two_tower(trainval, n_items, popularity(trainval, items), dim, hl, ep, log=log)
    full = iv
    user_vecs = {u: uv(trainval[u]) for u in trainval}
    per_test["two_tower"] = evaluate(trainval, test, lambda u: np.where(exists, full @ user_vecs[u], -np.inf), n)
    result["two_tower_setting"] = {"dim": dim, "history": hl, "epochs": ep, "train_s": round(secs, 1),
                                   "parameters": int((n_items + 1) * dim + dim * 2 * dim * 2 + 3 * dim + dim * dim + dim + hl)}
    log("two-tower", summary(per_test["two_tower"]))

    # ChloeDB: the events of train + validation, a recommender, and the two-tower's vectors
    ch = common.Chloedb(os.path.join(WORK, "chloedb"))
    ch.sql(f"CREATE TABLE items (id INT PRIMARY KEY, emb VECTOR({dim}))")
    ch.sql("CREATE TABLE events (id KEYWORD PRIMARY KEY, usr INT, item INT, ts INT)")
    ch.write("items", [{"id": int(m), "emb": [round(float(x), 6) for x in full[m]]} for m in items])
    t = time.perf_counter()
    rows = [{"id": f"{u}-{i}", "usr": u, "item": m, "ts": ts * 1000 + i % 1000} for u, evs in trainval.items() for i, (ts, m) in enumerate(evs)]
    ch.write("events", rows, batch=5000)
    result["chloedb_events_write_s"] = round(time.perf_counter() - t, 1)
    rss_before = ch.memory_kb()
    r = ch.sql("CREATE RECOMMENDER ml ON events(usr, item, ts) ITEMS items(emb) OPTIONS (window = 3, history = 20)")
    result["chloedb_create_recommender"] = {k: v for k, v in (r.get("rows") and dict(zip(r["columns"], r["rows"][0])) or r).items()}
    result["chloedb_memory_kb"] = {"before_recommender": rss_before, "after_recommender": ch.memory_kb()}
    not_seen = "id NOT IN (SELECT item FROM events WHERE usr = :u)"
    sqls = {
        "chloedb_recommend": f"SELECT id FROM items WHERE {not_seen} ORDER BY recommend('ml', :u, k => 500) DESC LIMIT 10",
        "chloedb_two_tower": f"SELECT id FROM items WHERE {not_seen} ORDER BY similarity(emb, :v) DESC LIMIT 10",
        "chloedb_fused": f"SELECT id FROM items WHERE {not_seen} ORDER BY rrf(recommend('ml', :u, k => 500), similarity(emb, :v)) DESC LIMIT 10",
    }
    lat = {}
    for name, sql in sqls.items():
        per, ms, took, err = {}, [], [], None
        for u, (_, target) in test.items():
            params = {"u": u}
            if ":v" in sql:
                params["v"] = [round(float(x), 6) for x in user_vecs[u]]
            t = time.perf_counter()
            r = ch.sql(sql, params)
            ms.append((time.perf_counter() - t) * 1000)
            if "error" in r:
                err = r["error"][:400]
                break
            took.append(r.get("took_ms") or 0)
            per[u] = metrics([row[0] for row in r["rows"]], target)
        if err:
            result["sources"][name] = {"error": err}
            log(name, "error", err)
            continue
        per_test[name] = per
        lat[name] = {"client": common.percentiles(ms), "server": common.percentiles(took)}
        log(name, summary(per))
    result["chloedb_memory_kb"]["after_queries"] = ch.memory_kb()
    with open(f"/proc/{ch.proc.pid}/status") as f:
        for line in f:
            if line.startswith("VmHWM:"):
                result["chloedb_memory_kb"]["peak"] = int(line.split()[1])
    result["chloedb_version"] = ch.version
    ch.stop()
    result["chloedb_disk_bytes"] = ch.disk_bytes()

    for name, per in per_test.items():
        result["sources"][name] = {**summary(per), **({"latency": lat[name]} if name in lat else {})}
    pairs = [("chloedb_recommend", "two_tower"), ("chloedb_recommend", "cooc"), ("two_tower", "cooc"), ("chloedb_fused", "two_tower"),
             ("chloedb_fused", "chloedb_recommend"), ("chloedb_two_tower", "two_tower")]
    result["paired"] = {f"{a} - {b}": bootstrap(per_test[a], per_test[b]) for a, b in pairs if a in per_test and b in per_test}
    common.write_json(os.path.join(HERE, "raw", "results.json"), result)
    log(json.dumps(result["sources"], indent=1))


if __name__ == "__main__":
    main()
