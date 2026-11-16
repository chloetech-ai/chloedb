#!/usr/bin/env python3
"""Topics of arXiv: every paper's title and abstract embedded by the model
the papers' table embeds them with (potion-retrieval-32M, by model2vec),
clustered into topics, each named from its papers and put in its arXiv area,
and placed on a map.

    pip install model2vec scikit-learn numpy
    python3 tools/topics.py --rows ROWS [--k 1000] [--name]

ROWS is the folder `chloedb-arxiv prepare` wrote. This writes there:

    topics.tsv           each paper's topic (id, a tab, the topic)
    ax_topics.jsonl      the topics: label, a sentence about it, its terms,
                         its field, counts by year, growth, the categories,
                         techniques and domains of its papers, a place on the
                         map (x, y) and its centroid (the papers' mean vector,
                         512 numbers, which the live feed uses to place new
                         papers: the nearest centroid by ChloeDB's similarity)

Then run `prepare` again with `--topics ROWS` so the papers carry their
topic. With --name, each topic and field is named by a language model
(Claude, through ANTHROPIC_API_KEY or STRATA_ANTHROPIC_KEY) from its terms
and its most cited titles; without it, a topic is named by its terms.
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
    ap.add_argument("--k", type=int, default=1000)
    ap.add_argument("--sample", type=int, default=400_000)
    ap.add_argument("--name", action="store_true")
    ap.add_argument("--batch", type=int, default=4096)
    a = ap.parse_args()
    from model2vec import StaticModel
    from sklearn.cluster import MiniBatchKMeans
    from sklearn.decomposition import PCA

    t0 = time.time()
    papers_path = os.path.join(a.rows, "ax_papers.jsonl")
    model = StaticModel.from_pretrained("minishlab/potion-retrieval-32M")

    def text(r):
        # As the table's field embeds them: the sources joined by a newline.
        return (r.get("title") or "") + "\n" + (r.get("abstract") or "")

    # The light columns of every paper, kept for the counts.
    ids, years, cites, cats, techs, doms, recent = [], [], [], [], [], [], []
    titles = []
    now = time.time()
    for r in read_rows(papers_path):
        ids.append(r["id"])
        years.append(r.get("year") or 1991)
        cites.append(r.get("cites") or 0)
        cats.append(r.get("pcat") or "")
        techs.append(r.get("techniques") or [])
        doms.append(r.get("domains") or [])
        created = r.get("created") or ""
        recent.append(created >= time.strftime("%Y-%m-%d", time.gmtime(now - 365 * 86400)))
        titles.append(r.get("title") or "")
    n = len(ids)
    print(f"{n} papers read, {time.time() - t0:.0f} s", flush=True)

    # Fit on a sample: PCA to 128 dimensions, then k-means there.
    random.seed(7)
    sample_ix = sorted(random.sample(range(n), min(a.sample, n)))
    sample_set = set(sample_ix)
    sample_texts = []
    for i, r in enumerate(read_rows(papers_path)):
        if i in sample_set:
            sample_texts.append(text(r))
    xs = model.encode(sample_texts, batch_size=a.batch)
    pca = PCA(n_components=128, random_state=7).fit(xs)
    zs = pca.transform(xs)
    zs /= np.linalg.norm(zs, axis=1, keepdims=True) + 1e-9
    km = MiniBatchKMeans(n_clusters=a.k, random_state=7, batch_size=8192, n_init=3, max_iter=200).fit(zs)
    print(f"k-means on {len(sample_ix)} papers, {time.time() - t0:.0f} s", flush=True)
    cent128 = km.cluster_centers_ / (np.linalg.norm(km.cluster_centers_, axis=1, keepdims=True) + 1e-9)

    # Every paper: its topic, and the sums of the vectors for the centroids.
    topic = np.zeros(n, dtype=np.int32)
    sums = np.zeros((a.k, xs.shape[1]), dtype=np.float64)
    batch, at = [], 0

    def flush(batch, at):
        v = model.encode(batch, batch_size=a.batch)
        z = pca.transform(v)
        z /= np.linalg.norm(z, axis=1, keepdims=True) + 1e-9
        t = np.argmax(z @ cent128.T, axis=1)
        topic[at:at + len(batch)] = t
        np.add.at(sums, t, v)

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
    centroids = sums / (np.linalg.norm(sums, axis=1, keepdims=True) + 1e-9)
    print(f"all {n} papers placed, {time.time() - t0:.0f} s", flush=True)

    # The map: t-SNE of the centroids, by cosine.
    from sklearn.manifold import TSNE
    xy = TSNE(n_components=2, metric="cosine", perplexity=30, random_state=7, init="pca").fit_transform(centroids.astype(np.float32))
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
        size = sum(c.values()) or 1
        scored = sorted(((cnt / size) * math.log(1 + a.k / df[w]), w) for w, cnt in c.items() if cnt >= 3)
        topic_terms[t] = [w for _, w in sorted(scored, reverse=True)[:10]]

    now_year = time.gmtime(now).tm_year
    all_recent = sum(recent)
    all_before = sum(1 for y in years if y == now_year - 1)
    rows = []
    for t in range(a.k):
        ms = members.get(t, [])
        if not ms:
            continue
        by_year = collections.Counter(years[i] for i in ms)
        top_cited = sorted(ms, key=lambda i: -cites[i])[:12]
        rc = sum(1 for i in ms if recent[i])
        before = sum(1 for i in ms if years[i] == now_year - 1)
        growth = ((rc + 1) / (all_recent + 1)) / ((before + 1) / (all_before + 1))
        rows.append({
            "id": t,
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
            "centroid": [round(float(v), 5) for v in centroids[t]],
            "_titles": [titles[i] for i in top_cited],
        })
    key = os.environ.get("ANTHROPIC_API_KEY") or os.environ.get("STRATA_ANTHROPIC_KEY")
    if a.name and key:
        def job(r):
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
            r["field_label"] = FIELDS[r["field"]][0]
            f.write(json.dumps(r) + "\n")
    with open(os.path.join(a.rows, "topics.tsv"), "w") as f:
        for i, t in enumerate(topic):
            f.write(f"{ids[i]}\t{int(t)}\n")
    print(f"wrote {len(rows)} topics in {len(by_field)} fields, {time.time() - t0:.0f} s", flush=True)


if __name__ == "__main__":
    main()
