#!/usr/bin/env python3
"""Topics of arXiv: every paper's title and abstract embedded by the model
the papers' table embeds them with (potion-retrieval-32M, by model2vec),
clustered into topics, each named from its papers and put in its arXiv area,
and placed on a map.

    pip install model2vec scikit-learn numpy
    python3 tools/topics.py --rows ROWS [--k 1500] [--name] [--keep OLD.tsv]

ROWS is the folder `chloedb-arxiv prepare` wrote. This writes there:

    topics.tsv           each paper's topic (id, a tab, the topic)
    ax_topics.jsonl      the topics: label, a sentence about it, its terms,
                         its field, counts by year, growth, the categories,
                         techniques and domains of its papers, a place on the
                         map (x, y), its centroid (512 numbers, which the
                         live rounds use to place new papers) and its
                         categories' shares (`shares`, JSON)

How papers are placed. k-means runs on the papers' vectors as the table
holds them (unit length, by cosine), fitted on a sample that is half from
the last two years (`--recent`), so the newest fields get topics of their
own instead of joining an old one that writes alike. Each paper then goes
to whichever of its ten nearest centroids scores best on similarity plus
`--prior` times the log share of the paper's primary category among that
topic's papers: nearness alone let one topic of control theory gather
1,270 papers of 2026 from fifteen fields. The live rounds place new papers
by the same rule (src/live.rs, `place`). A centroid the sample gave
fewer than `--min-papers` papers (duplicates, a sliver between two
topics) is dropped and its papers go to the nearest topic left among
their ten: without that, 338 of 1,500 topics held under 50 papers.
`--cache FILE.npz` keeps the centroids and every paper's ten nearest, so a
rerun with other settings skips the 20 minutes of embedding, and
`--labels OLD.jsonl` keeps the names of topics whose centroid is the same.

With --keep OLD.tsv (the topics.tsv of a run before), a topic whose papers
are mostly an old topic's keeps that topic's id, so links to it keep
working, and changed.tsv lists only the papers whose topic changed (what
`UPDATE ax_papers SET topic` needs). Otherwise run `prepare` again with
`--topics ROWS` so the papers carry their topic.

With --name, each topic is named by a language model (Claude, through
ANTHROPIC_API_KEY or STRATA_ANTHROPIC_KEY) from its terms and its most
cited titles; without it, a topic is named by its terms.
"""

import argparse
import collections
import json
import math
import os
import random
import re
import sys
import time
import urllib.request
from concurrent.futures import ThreadPoolExecutor

import numpy as np

STOP = set("""a an the of for and or in on to with via by from at as is are its their into using based towards toward new
novel study studies approach analysis method methods some two one under over between through without within beyond case
note remarks do does can we it this that be what how why when not all more less than i ii iii part model models data
results result paper problem problems learning system systems application applications general theory properties use
effect effects first non large small high low order type types time""".split())


def words(t):
    return [w for w in re.findall(r"[a-z][a-z0-9\-]+", t.lower()) if len(w) > 2]


def terms_of(title):
    w = words(title)
    out = [x for x in w if x not in STOP]
    for i in range(len(w) - 1):
        if w[i] not in STOP and w[i + 1] not in STOP:
            out.append(w[i] + " " + w[i + 1])
    return out


def read_rows(path):
    with open(path) as f:
        for line in f:
            yield json.loads(line)


def claude(prompt, key, model="claude-haiku-5-5", max_tokens=300):
    req = {"model": model, "max_tokens": max_tokens, "messages": [{"role": "user", "content": prompt}]}
    # A short answer only: with thinking on, Haiku can spend all of
    # max_tokens thinking and write nothing (a tenth of the topics were
    # left named by their terms that way).
    if "haiku" in model:
        req["thinking"] = {"type": "disabled"}
    body = json.dumps(req).encode()
    base = os.environ.get("ANTHROPIC_API_URL", "https://api.anthropic.com")
    for attempt in range(6):
        try:
            req = urllib.request.Request(base + "/v1/messages", data=body, headers={"x-api-key": key, "anthropic-version": "2023-06-01", "content-type": "application/json"})
            with urllib.request.urlopen(req, timeout=60) as r:
                j = json.loads(r.read())
            return "".join(c.get("text", "") for c in j.get("content", []))
        except Exception as e:  # noqa: BLE001 - retried, then given up
            time.sleep(2 * (attempt + 1))
            err = e
    raise RuntimeError(f"Claude: {err}")


def name_topic(key, terms, titles, cats):
    prompt = (
        "You name research topics for a map of arXiv. Below are the distinctive terms and the most cited titles of one cluster of papers.\n"
        f"Categories: {', '.join(cats)}\nTerms: {', '.join(terms)}\nTitles:\n- " + "\n- ".join(titles) +
        "\n\nAnswer with JSON only: {\"label\": \"2 to 5 words a researcher would use for this topic, Title Case not required, no quotes\", "
        "\"about\": \"one sentence of at most 25 words on what papers in this topic study\"}"
    )
    text = claude(prompt, key)
    m = re.search(r"\{.*\}", text, re.S)
    j = json.loads(m.group(0)) if m else {}
    return j.get("label", "").strip(), j.get("about", "").strip()


# The atlas's fields: arXiv's areas (AI and machine learning apart from
# the rest of computer science), the first that matches a category.
FIELDS = [
    ("AI and machine learning", lambda c: c in {"cs.LG", "cs.AI", "cs.CL", "cs.CV", "cs.NE", "cs.IR", "cs.MA", "cs.RO", "stat.ML", "cs.HC", "cs.MM", "cs.SD"}),
    ("Computer science", lambda c: c.startswith("cs.")),
    ("Electrical engineering and signals", lambda c: c.startswith("eess.")),
    ("Statistics", lambda c: c.startswith("stat.")),
    ("Mathematical physics", lambda c: c in {"math-ph", "math.MP"}),
    ("Mathematics", lambda c: c.startswith("math.") or c == "math"),
    ("Astrophysics", lambda c: c.startswith("astro-ph")),
    ("Condensed matter", lambda c: c.startswith("cond-mat")),
    ("High energy physics", lambda c: c.startswith("hep-")),
    ("Gravitation and cosmology", lambda c: c == "gr-qc"),
    ("Quantum physics", lambda c: c == "quant-ph"),
    ("Nuclear physics", lambda c: c.startswith("nucl-")),
    ("Physics", lambda c: c.startswith("physics.") or c == "physics"),
    ("Nonlinear sciences", lambda c: c.startswith("nlin.")),
    ("Biology", lambda c: c.startswith("q-bio")),
    ("Finance", lambda c: c.startswith("q-fin")),
    ("Economics", lambda c: c.startswith("econ.")),
]


def field_of(cats):
    for c in cats or []:
        for i, (_, f) in enumerate(FIELDS):
            if f(c):
                return i
    return 12


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--rows", required=True)
    ap.add_argument("--k", type=int, default=1500)
    ap.add_argument("--sample", type=int, default=300_000, help="papers of any year to fit on")
    ap.add_argument("--recent", type=int, default=250_000, help="and papers of the last two years")
    ap.add_argument("--prior", type=float, default=0.01, help="weight of the category mix (src/live.rs PRIOR)")
    ap.add_argument("--keep", help="topics.tsv of a run before: its ids are kept where topics match")
    ap.add_argument("--min-papers", type=int, default=150, help="a topic with fewer papers is dropped and its papers placed in their next nearest")
    ap.add_argument("--cache", help="an .npz file keeping the centroids and every paper's ten nearest, so a rerun skips the embedding")
    ap.add_argument("--labels", help="ax_topics.jsonl of a run before: a topic whose centroid is the same keeps its name (no model call)")
    ap.add_argument("--name", action="store_true")
    ap.add_argument("--batch", type=int, default=4096)
    a = ap.parse_args()
    from model2vec import StaticModel
    from sklearn.cluster import MiniBatchKMeans

    t0 = time.time()
    papers_path = os.path.join(a.rows, "ax_papers.jsonl")
    model = StaticModel.from_pretrained("minishlab/potion-retrieval-32M")

    def text(r):
        # As the table's field embeds them: the sources joined by a newline.
        return (r.get("title") or "") + "\n" + (r.get("abstract") or "")

    def unit(x):
        return x / (np.linalg.norm(x, axis=1, keepdims=True) + 1e-9)

    # The light columns of every paper, kept for the counts.
    ids, years, cites, cats, techs, doms, recent, created = [], [], [], [], [], [], [], []
    titles = []
    now = time.time()
    last_year = time.strftime("%Y-%m-%d", time.gmtime(now - 365 * 86400))
    two_years = time.strftime("%Y-%m-%d", time.gmtime(now - 730 * 86400))
    for r in read_rows(papers_path):
        ids.append(r["id"])
        years.append(r.get("year") or 1991)
        cites.append(r.get("cites") or 0)
        cats.append(r.get("pcat") or "")
        techs.append(r.get("techniques") or [])
        doms.append(r.get("domains") or [])
        c = (r.get("created") or "")[:10]
        created.append(c)
        recent.append(c >= last_year)
        titles.append(r.get("title") or "")
    n = len(ids)
    print(f"{n} papers read, {time.time() - t0:.0f} s", flush=True)

    K = 10
    if a.cache and os.path.exists(a.cache):
        z = np.load(a.cache)
        assert len(z["near_t"]) == n, "the cache is of other papers"
        cent, near_s, near_t = z["cent"], z["near_s"], z["near_t"]
        print(f"centroids and nearest read from {a.cache}, {time.time() - t0:.0f} s", flush=True)
    else:
        # Fit on a sample, half of it from the last two years.
        random.seed(7)
        newer = [i for i in range(n) if created[i] >= two_years]
        older = [i for i in range(n) if created[i] < two_years]
        pick = set(random.sample(older, min(a.sample, len(older)))) | set(random.sample(newer, min(a.recent, len(newer))))
        sample_texts = [text(r) for i, r in enumerate(read_rows(papers_path)) if i in pick]
        xs = unit(model.encode(sample_texts, batch_size=a.batch).astype(np.float32))
        del sample_texts
        km = MiniBatchKMeans(n_clusters=a.k, random_state=7, batch_size=8192, n_init=3, max_iter=200).fit(xs)
        cent = unit(km.cluster_centers_.astype(np.float32))
        del xs
        print(f"k-means on {len(pick)} papers ({len(pick & set(newer))} of the last two years), {time.time() - t0:.0f} s", flush=True)

        # Every paper: its ten nearest centroids.
        K = 10
        near_s = np.zeros((n, K), dtype=np.float32)
        near_t = np.zeros((n, K), dtype=np.int16)
        batch, at = [], 0

        def flush(batch, at):
            z = unit(model.encode(batch, batch_size=a.batch).astype(np.float32))
            sim = z @ cent.T
            ix = np.argpartition(-sim, K, axis=1)[:, :K]
            v = np.take_along_axis(sim, ix, 1)
            o = np.argsort(-v, 1)
            near_s[at:at + len(batch)] = np.take_along_axis(v, o, 1)
            near_t[at:at + len(batch)] = np.take_along_axis(ix, o, 1)

        for r in read_rows(papers_path):
            batch.append(text(r))
            if len(batch) == 50_000:
                flush(batch, at)
                at += len(batch)
                batch = []
                print(f"  {at} embedded, {time.time() - t0:.0f} s", flush=True)
        if batch:
            flush(batch, at)
            at += len(batch)
        if a.cache:
            np.savez(a.cache, cent=cent, near_s=near_s, near_t=near_t)

    # A topic the k-means sample gave almost no papers is dropped: its
    # papers go to the nearest topic left among their ten.
    alive = np.ones(len(cent), dtype=bool)
    for _ in range(5):
        dead = ~alive[near_t.astype(np.int64)]
        first = np.where(dead.all(1), 0, np.argmax(~dead, 1))
        size0 = np.bincount(near_t[np.arange(n), first].astype(np.int64), minlength=len(cent))
        drop = alive & (size0 < a.min_papers)
        if not drop.any():
            break
        alive &= ~drop
    # A paper whose ten nearest were all dropped brings its nearest back.
    lone = (~alive[near_t.astype(np.int64)]).all(1)
    orphans = int(lone.sum())
    alive[np.unique(near_t[lone, 0].astype(np.int64))] = True
    keep_t = np.flatnonzero(alive)
    remap = np.full(len(cent), -1, dtype=np.int64)
    remap[keep_t] = np.arange(len(keep_t))
    cent = cent[keep_t]
    near_s = np.where(alive[near_t.astype(np.int64)], near_s, -np.inf).astype(np.float32)
    near_t = remap[near_t.astype(np.int64)]
    k = len(keep_t)
    print(f"{len(alive) - k} topics of fewer than {a.min_papers} papers dropped, {k} left ({orphans} papers had none of their ten left: their nearest stays)", flush=True)

    # The category mix of each topic by nearness alone, then each paper
    # placed by nearness plus the log share of its category there.
    cat_ix = {c: i for i, c in enumerate(sorted(set(cats)))}
    pc = np.array([cat_ix[c] for c in cats], dtype=np.int32)
    mix = np.zeros((k, len(cat_ix)), dtype=np.float64)
    first = np.argmax(near_s, 1)
    np.add.at(mix, (near_t[np.arange(n), first].astype(np.int64), pc), 1)
    size = mix.sum(1)
    nt64 = np.maximum(near_t, 0).astype(np.int64)
    score = near_s + a.prior * np.log((mix[nt64, pc[:, None]] + 0.5) / (size[nt64] + 50))
    topic = near_t[np.arange(n), np.argmax(score, 1)].astype(np.int32)
    moved = int((topic != near_t[np.arange(n), first]).sum())
    del near_s, near_t, score
    print(f"all {n} papers placed ({moved} moved by their category), {time.time() - t0:.0f} s", flush=True)

    # Ids: a topic that is mostly an old one keeps its id.
    ident = list(range(k))
    old = None
    if a.keep:
        where = {x: i for i, x in enumerate(ids)}
        old = np.full(n, -1, dtype=np.int32)
        for line in open(a.keep):
            pid, _, t = line.rstrip("\n").partition("\t")
            if pid in where and t.lstrip("-").isdigit():
                old[where[pid]] = int(t)
        pairs = collections.Counter(zip(topic.tolist(), old.tolist()))
        new_n = collections.Counter(topic.tolist())
        old_n = collections.Counter(old.tolist())
        taken_new, taken_old, ident = set(), set(), [None] * k
        for (t, o), c in sorted(pairs.items(), key=lambda kv: -kv[1]):
            if o < 0 or t in taken_new or o in taken_old:
                continue
            # Most of the new topic was the old one, and much of the old one moved here.
            if c >= 0.5 * new_n[t] and c >= 0.3 * old_n[o]:
                ident[t] = o
                taken_new.add(t)
                taken_old.add(o)
        fresh = max(old_n) + 1
        for t in range(k):
            if ident[t] is None:
                ident[t] = fresh
                fresh += 1
        print(f"{len(taken_new)} topics keep an old id, {k - len(taken_new)} are new", flush=True)

    # The map: t-SNE of the centroids, by cosine.
    from sklearn.manifold import TSNE
    xy = TSNE(n_components=2, metric="cosine", perplexity=30, random_state=7, init="pca").fit_transform(cent)
    xy = (xy - xy.min(axis=0)) / (xy.max(axis=0) - xy.min(axis=0) + 1e-9)

    # Terms: c-TF-IDF of the titles' words and pairs of words.
    members = collections.defaultdict(list)
    for i, t in enumerate(topic):
        members[int(t)].append(i)
    tf = {}
    df = collections.Counter()
    for t, ms in members.items():
        c = collections.Counter()
        for i in ms if len(ms) <= 4000 else random.sample(ms, 4000):
            c.update(set(terms_of(titles[i])))
        tf[t] = c
        df.update(c.keys())
    topic_terms = {}
    for t, c in tf.items():
        size_t = sum(c.values()) or 1
        scored = sorted(((cnt / size_t) * math.log(1 + k / df[w]), w) for w, cnt in c.items() if cnt >= 3)
        topic_terms[t] = [w for _, w in sorted(scored, reverse=True)[:10]]

    now_year = time.gmtime(now).tm_year
    all_recent = sum(recent)
    all_before = sum(1 for y in years if y == now_year - 1)
    rows = []
    for t in range(k):
        ms = members.get(t, [])
        if not ms:
            continue
        by_year = collections.Counter(years[i] for i in ms)
        top_cited = sorted(ms, key=lambda i: -cites[i])[:12]
        rc = sum(1 for i in ms if recent[i])
        before = sum(1 for i in ms if years[i] == now_year - 1)
        growth = ((rc + 1) / (all_recent + 1)) / ((before + 1) / (all_before + 1))
        mixed = collections.Counter(cats[i] for i in ms)
        rows.append({
            "id": ident[t],
            "label": ", ".join(topic_terms[t][:3]),
            "about": "",
            "terms": topic_terms[t],
            "field": field_of([c for c, _ in collections.Counter(cats[i] for i in ms).most_common(5)]),
            "papers": len(ms),
            "recent": rc,
            "growth": round(growth, 3),
            "cites": int(sum(cites[i] for i in ms)),
            "first": int(min(by_year)),
            "peak": int(max(by_year.items(), key=lambda kv: kv[1])[0]),
            "by_year": json.dumps({str(k): v for k, v in sorted(by_year.items())}),
            "cats": [c for c, _ in collections.Counter(cats[i] for i in ms).most_common(5)],
            "techniques": [c for c, _ in collections.Counter(x for i in ms for x in techs[i]).most_common(6)],
            "domains": [c for c, _ in collections.Counter(x for i in ms for x in doms[i]).most_common(6)],
            "x": round(float(xy[t, 0]), 5),
            "y": round(float(xy[t, 1]), 5),
            "centroid": [round(float(v), 5) for v in cent[t]],
            "shares": json.dumps({c: round(m / len(ms), 4) for c, m in mixed.most_common() if m / len(ms) >= 0.002}),
            "_titles": [titles[i] for i in top_cited],
        })
    # Names kept from a run before, for a topic whose centroid is the same.
    if a.labels:
        prev = [json.loads(l) for l in open(a.labels)]
        pc_ = unit(np.array([r["centroid"] for r in prev], dtype=np.float32))
        mine = unit(np.array([r["centroid"] for r in rows], dtype=np.float32))
        best = (mine @ pc_.T).argmax(1)
        sim_ = (mine @ pc_.T).max(1)
        kept_names = 0
        for r, j, sv in zip(rows, best, sim_):
            if sv >= 0.995 and prev[j].get("about"):
                r["label"], r["about"], r["_named"] = prev[j]["label"], prev[j]["about"], True
                kept_names += 1
        print(f"{kept_names} topics keep their name from {a.labels}", flush=True)
    key = os.environ.get("ANTHROPIC_API_KEY") or os.environ.get("STRATA_ANTHROPIC_KEY")
    if a.name and key:
        def job(r):
            if r.get("_named"):
                return r
            try:
                lab, about = name_topic(key, r["terms"][:10], r["_titles"][:10], r["cats"][:3])
                if lab:
                    r["label"], r["about"] = lab, about
            except Exception as e:  # noqa: BLE001 - the terms stay the label
                print("naming failed:", e, file=sys.stderr)
            return r
        with ThreadPoolExecutor(8) as ex:
            list(ex.map(job, rows))
        print(f"named {len(rows)} topics, {time.time() - t0:.0f} s", flush=True)
    # Fields: arXiv's areas, each topic in the area of its most common category.
    by_field = collections.defaultdict(list)
    for r in rows:
        by_field[r["field"]].append(r)
    with open(os.path.join(a.rows, "ax_topics.jsonl"), "w") as f:
        for r in rows:
            r.pop("_titles", None)
            r.pop("_named", None)
            r["field_label"] = FIELDS[r["field"]][0]
            f.write(json.dumps(r) + "\n")
    with open(os.path.join(a.rows, "topics.tsv"), "w") as f:
        for i, t in enumerate(topic):
            f.write(f"{ids[i]}\t{ident[int(t)]}\n")
    if old is not None:
        changed = 0
        with open(os.path.join(a.rows, "changed.tsv"), "w") as f:
            for i, t in enumerate(topic):
                if ident[int(t)] != old[i]:
                    f.write(f"{ids[i]}\t{ident[int(t)]}\n")
                    changed += 1
        print(f"{changed} papers change topic ({n - changed} keep theirs)", flush=True)
    print(f"wrote {len(rows)} topics in {len(by_field)} fields, {time.time() - t0:.0f} s", flush=True)


if __name__ == "__main__":
    main()
