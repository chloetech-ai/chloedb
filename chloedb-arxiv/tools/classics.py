#!/usr/bin/env python3
"""Likely classics: which recent papers look, in their first months, like
the papers that went on to be among the most cited of their year and area.

Reads `prepare`'s rows (ax_papers.jsonl, ax_cites.jsonl) and writes:

  ax_classics.jsonl  a row per recent paper worth showing: its chance of
                     ending in the top 1% of its year and area by citations,
                     that chance over the usual 1% (`lift`), which model
                     judged it and the reasons that weighed most
  model.json         the models' weights and how well each did on a year
                     they were not trained on, against a simple ranking
  ax_notes.jsonl     that card as the note the app shows

    python3 tools/classics.py ROWS_DIR OUT_DIR [--horizon YYYY-MM]
    chloedb-arxiv load --rows OUT_DIR --only ax_classics,ax_notes

(after `DELETE FROM ax_classics` on the search server, when it replaces
an earlier run's rows.)

What a paper is judged on is only what was known a few months after it
was posted: the arXiv papers that cited it in its first 3, 6 or 12 months
(a citing paper dated by its arXiv posting), its authors' earlier papers
on arXiv (how many, and their citations as counted now, the one thing
not as of then), how fast its category and its techniques grew in the
year before it, its team size, whether it has code, and whether it is a
survey, a dataset or a benchmark. A logistic regression per window is
trained on papers posted 2016 to 2021 and checked on 2022 (whose
citations have had more than three years); its scores on 2022 are mapped
to chances (bins of 2022's papers by score, made to rise with it), so a
chance says how often papers scored so in 2022 did end in the top 1%.

Crossref's citations stop some months before the snapshot (`--horizon`,
by default the last month with half the citing papers of the year
before); a paper is judged by the longest window that ends before the
horizon, and a paper too new for 3 months by the model without
citations. Needs numpy and scikit-learn."""
import argparse, datetime as dt, json, math, os, sys
from collections import defaultdict

import numpy as np
from sklearn.isotonic import IsotonicRegression
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import roc_auc_score

ap = argparse.ArgumentParser()
ap.add_argument("rows")
ap.add_argument("out")
ap.add_argument("--horizon", default="")
ap.add_argument("--since", default="", help="judge papers posted from this month (default: 18 months before the newest)")
a = ap.parse_args()
os.makedirs(a.out, exist_ok=True)
EPOCH = dt.datetime(1990, 1, 1, tzinfo=dt.timezone.utc)
WINDOWS = [0, 90, 182, 365]
TRAIN = range(2016, 2022)
TEST = 2022
KINDS = ["survey", "dataset", "benchmark", "tool"]


def day(s):
    try:
        return (dt.datetime.fromisoformat(s.replace("Z", "+00:00")) - EPOCH).days
    except Exception:
        return -1


def month_of(d):
    t = EPOCH + dt.timedelta(days=int(d))
    return t.year * 12 + t.month - 1


def say(*x):
    print(*x, file=sys.stderr, flush=True)


# The papers: the columns a judgement uses.
cache = os.path.join(a.out, "papers.npz")
if os.path.exists(cache):
    z = np.load(cache, allow_pickle=True)
    ids, created, year, area, cites, nauth, code, kinds, pcat = (z[k] for k in ["ids", "created", "year", "area", "cites", "nauth", "code", "kinds", "pcat"])
    people, techs = list(z["people"]), list(z["techs"])
    say(f"papers from {cache}: {len(ids)}")
else:
    ids, created, year, area, cites, nauth, code, kinds, pcat, people, techs = [], [], [], [], [], [], [], [], [], [], []
    for n, line in enumerate(open(os.path.join(a.rows, "ax_papers.jsonl"))):
        r = json.loads(line)
        d = day(r.get("created") or "")
        if d < 0:
            continue
        ids.append(r["id"])
        created.append(d)
        year.append(int(r.get("year") or 0))
        area.append(r.get("area") or "")
        pcat.append(r.get("pcat") or "")
        cites.append(int(r.get("cites") or 0))
        nauth.append(int(r.get("nauthors") or 1))
        code.append(1 if r.get("code") else 0)
        ks = r.get("kinds") or []
        kinds.append(sum(1 << i for i, k in enumerate(KINDS) if k in ks))
        people.append(tuple(sys.intern(p) for p in (r.get("people") or [])[:50]))
        techs.append(tuple(sys.intern(t) for t in (r.get("techniques") or [])))
        if n % 500000 == 0:
            say(f"  {n} papers read")
    ids, created, year, area, cites, nauth, code, kinds, pcat = (np.array(v) for v in (ids, created, year, area, cites, nauth, code, kinds, pcat))
    pe, te = np.empty(len(people), dtype=object), np.empty(len(techs), dtype=object)
    pe[:], te[:] = people, techs
    np.savez(cache, ids=ids, created=created, year=year, area=area, cites=cites, nauth=nauth, code=code, kinds=kinds, pcat=pcat, people=pe, techs=te)
    say(f"papers: {len(ids)}")
N = len(ids)
ix = {p: i for i, p in enumerate(ids)}
newest = int(created.max())

# Early citations: arXiv papers citing it, dated by their own posting. A
# citing paper posted before the cited one cites it from a later
# published version, of unknown date: not counted early.
src_days, early = [], {w: np.zeros(N, dtype=np.int32) for w in WINDOWS[1:]}
for line in open(os.path.join(a.rows, "ax_cites.jsonl")):
    r = json.loads(line)
    s, d = ix.get(r["src"]), ix.get(r["dst"])
    if s is None or d is None:
        continue
    src_days.append(created[s])
    gap = created[s] - created[d]
    if gap < 0:
        continue
    for w in WINDOWS[1:]:
        if gap <= w:
            early[w][d] += 1
src_days = np.array(src_days)
by_month = defaultdict(int)
for d in src_days:
    by_month[month_of(d)] += 1
if a.horizon:
    y, m = map(int, a.horizon.split("-"))
    H = (dt.datetime(y, m, 1, tzinfo=dt.timezone.utc) - EPOCH).days
else:
    # The last month with half the citing papers a month of the year before.
    last = max(by_month)
    H = None
    for mo in range(last, last - 36, -1):
        before = [by_month.get(x, 0) for x in range(mo - 12, mo)]
        if by_month.get(mo, 0) >= 0.5 * (sum(before) / 12):
            H = (dt.datetime(mo // 12, mo % 12 + 1, 1, tzinfo=dt.timezone.utc) - EPOCH).days
            break
hdate = (EPOCH + dt.timedelta(days=int(H))).date()
say(f"citations: {len(src_days)} between papers; horizon {hdate}")

# Authors' records before each paper: earlier papers and their citations.
order = np.argsort(created, kind="stable")
rec_n, rec_c = defaultdict(int), defaultdict(int)
amax_c, amax_n, afirst_c, alast_c = (np.zeros(N, dtype=np.float32) for _ in range(4))
for i in order:
    ps = people[i]
    if ps:
        cs = [rec_c[p] for p in ps]
        amax_c[i] = max(cs)
        amax_n[i] = max(rec_n[p] for p in ps)
        afirst_c[i] = cs[0]
        alast_c[i] = cs[-1]
        for p in ps:
            rec_n[p] += 1
            rec_c[p] += int(cites[i])
say("authors' records done")

# Growth in the year before: the category's papers, and its fastest technique's.
mon = np.array([month_of(d) for d in created])
cat_m, tech_m = defaultdict(lambda: defaultdict(int)), defaultdict(lambda: defaultdict(int))
for i in range(N):
    cat_m[pcat[i]][mon[i]] += 1
    for t in techs[i]:
        tech_m[t][mon[i]] += 1


def growth(series, m):
    a_ = sum(series.get(x, 0) for x in range(m - 12, m))
    b_ = sum(series.get(x, 0) for x in range(m - 24, m - 12))
    return math.log((a_ + 5) / (b_ + 5))


memo = {}


def g(kind, key, m):
    k = (kind, key, m)
    if k not in memo:
        memo[k] = growth((cat_m if kind == "c" else tech_m)[key], m)
    return memo[k]


cgrow = np.array([g("c", pcat[i], mon[i]) for i in range(N)], dtype=np.float32)
tgrow = np.array([max((g("t", t, mon[i]) for t in techs[i]), default=0.0) for i in range(N)], dtype=np.float32)
hastech = np.array([1.0 if techs[i] else 0.0 for i in range(N)], dtype=np.float32)
say("growth done")

AREAS = sorted(set(area.tolist()) - {""})
NAMES = ["authors' best record", "authors' earlier papers", "first author's record", "last author's record", "team size", "code", *KINDS, "category growth", "technique growth", "has a named technique", *[f"area {x}" for x in AREAS]]


def features(rows, w):
    x = [np.log1p(amax_c[rows]), np.log1p(amax_n[rows]), np.log1p(afirst_c[rows]), np.log1p(alast_c[rows]), np.log1p(np.minimum(nauth[rows], 100)), code[rows].astype(np.float32)]
    x += [((kinds[rows] >> k) & 1).astype(np.float32) for k in range(len(KINDS))]
    x += [cgrow[rows], tgrow[rows], hastech[rows]]
    x += [(area[rows] == ar).astype(np.float32) for ar in AREAS]
    if w:
        x += [np.log1p(early[w][rows]), (early[w][rows] > 0).astype(np.float32)]
    return np.stack(x, 1)


def names(w):
    return NAMES + ([f"cited in its first {w} days", f"cited at all in its first {w} days"] if w else [])


# The label: the top 1% of its year and area by citations (and at least one).
top = np.zeros(N, dtype=bool)
grp = defaultdict(list)
for i in np.where((year >= min(TRAIN)) & (year <= TEST))[0]:
    grp[(year[i], area[i])].append(i)
for rows in grp.values():
    rows = np.array(rows)
    c = cites[rows]
    k = max(1, int(round(0.01 * len(rows))))
    thr = np.sort(c)[::-1][k - 1]
    top[rows] = (c >= max(thr, 1))
base = float(top[(year >= min(TRAIN)) & (year <= TEST)].mean())
say(f"base rate of the top 1%: {base:.4f}")


def fit(rows, w):
    x, y = features(rows, w), top[rows]
    mu, sd = x.mean(0), x.std(0) + 1e-6
    m = LogisticRegression(C=1.0, max_iter=2000).fit((x - mu) / sd, y)
    return m, mu, sd


def prec_at(scores, y, frac=0.01):
    k = max(1, int(len(scores) * frac))
    return float(y[np.argsort(-scores)[:k]].mean())


card = {"horizon": str(hdate), "base": base, "train": [min(TRAIN), max(TRAIN)], "test": TEST, "models": {}}
models = {}
tr = np.where(np.isin(year, list(TRAIN)))[0]
te = np.where(year == TEST)[0]
for w in WINDOWS:
    m, mu, sd = fit(tr, w)
    s = m.decision_function((features(te, w) - mu) / sd)
    y = top[te]
    simple = (np.log1p(early[w][te]) * 100 + np.log1p(amax_c[te])) if w else np.log1p(amax_c[te])
    ev = {"auc": float(roc_auc_score(y, s)), "p_at_1": prec_at(s, y), "simple_auc": float(roc_auc_score(y, simple)), "simple_p_at_1": prec_at(simple, y), "papers": int(len(te)), "top": int(y.sum())}
    # Chances from bins of 2022's scores, 300 papers each at the top and
    # 3,000 below, made to rise with the score: a few top papers that all
    # made it would otherwise say a certainty.
    o = np.argsort(-s)
    cuts = list(range(0, min(6000, len(o)), 300)) + list(range(6000, len(o), 3000)) + [len(o)]
    bins = [(lo, hi) for lo, hi in zip(cuts, cuts[1:]) if hi > lo]
    centers = [float(s[o[lo:hi]].mean()) for lo, hi in bins]
    rates = [float(y[o[lo:hi]].mean()) for lo, hi in bins]
    iso = IsotonicRegression(out_of_bounds="clip", y_min=0.0, y_max=1.0).fit(centers, rates, sample_weight=[hi - lo for lo, hi in bins])
    ev["best_chance"] = float(iso.predict([s.max()])[0])
    say(f"window {w}: {ev}")
    models[w] = (m, mu, sd, iso)
    card["models"][str(w)] = {"eval": ev, "simple": "its early citations, then its authors' record" if w else "its authors' record", "features": names(w), "weights": m.coef_[0].tolist(), "bias": float(m.intercept_[0]), "mean": mu.tolist(), "sd": sd.tolist(),
                              "calibration": {"x": iso.X_thresholds_.tolist()[::max(1, len(iso.X_thresholds_) // 200)], "y": iso.y_thresholds_.tolist()[::max(1, len(iso.y_thresholds_) // 200)]}}

# Judge the recent papers, each by the longest window that ends before the horizon.
since = (dt.datetime(*map(int, a.since.split("-")), 1, tzinfo=dt.timezone.utc) - EPOCH).days if a.since else newest - 548
recent = np.where(created >= since)[0]


def why(x, m, mu, sd, w):
    z = (x - mu) / sd * m.coef_[0]
    nm = names(w)
    out = []
    for j in np.argsort(-z):
        if z[j] <= 0.15 or len(out) == 3:
            break
        n = nm[j]
        if n.startswith("area ") or n.startswith("cited at all"):
            continue
        out.append(n)
    return out


rows_out = []
for w in WINDOWS:
    nxt = WINDOWS[WINDOWS.index(w) + 1] if w != WINDOWS[-1] else 10**9
    age = H - created[recent]
    pick = recent[(age >= w) & (age < nxt)] if w else recent[age < WINDOWS[1]]
    if not len(pick):
        continue
    m, mu, sd, iso = models[w]
    x = features(pick, w)
    p = iso.predict(m.decision_function((x - mu) / sd))
    say(f"window {w}: {len(pick)} papers judged, {int((p >= 3 * base).sum())} at three times the usual or more")
    for i, pi, xi in zip(pick, p, x):
        if pi < 3 * base:
            continue
        rows_out.append({"id": ids[i], "p": round(float(pi), 4), "lift": round(float(pi / base), 1), "win": int(w), "early": int(early[w][i]) if w else 0,
                         "record_cites": int(amax_c[i]), "area": area[i], "pcat": pcat[i], "created": (EPOCH + dt.timedelta(days=int(created[i]))).strftime("%Y-%m-%dT00:00:00Z"),
                         "why": why(xi, m, mu, sd, w)})
with open(os.path.join(a.out, "ax_classics.jsonl"), "w") as f:
    for r in rows_out:
        f.write(json.dumps(r) + "\n")
card["judged"] = int(len(recent))
card["kept"] = len(rows_out)
card["since"] = str((EPOCH + dt.timedelta(days=int(since))).date())
card["made"] = dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
json.dump(card, open(os.path.join(a.out, "model.json"), "w"))
# The card as the note the app reads (`load --only ax_notes`).
with open(os.path.join(a.out, "ax_notes.jsonl"), "w") as f:
    f.write(json.dumps({"id": "classics:model", "paper": "", "style": "model", "text": json.dumps(card), "model": "logistic regression", "at": card["made"], "ms": 0.0}) + "\n")
say(f"wrote {len(rows_out)} likely classics of {len(recent)} recent papers")
