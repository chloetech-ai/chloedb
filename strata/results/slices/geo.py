"""On-device slices, search near the user: a phone holds the places of the
square around it, and answers "near me" searches there exactly or asks the
cloud.

Data:

- Places: Overture Maps' places (release 2026-10-21.0, CDLA Permissive 2.0)
  in a box around New York City (lon -74.26 to -73.70, lat 40.49 to 40.92),
  open or of unknown status, confidence 0.5 or more. A place's text is its
  name, the words of its categories (Overture's taxonomy: primary,
  hierarchy, alternates, basic category), its brand and its locality.
- Demand: the Foursquare check-ins in New York of April 2012 to February
  2013 (Yang et al., IEEE TSMC 2015; 227,428 check-ins of 1,083 users). A
  check-in is taken as a search made from where the user checked in before:
  for the venue's category (`category` searches, "pizza") or for the place
  itself (`name` searches: the nearest Overture place within 50 m sharing a
  word of the category). Check-ins at homes, offices, transit, roads and
  other places nobody searches for are left out.

The cloud ranks every place by `bm25(body, q) / (1 + d^2 / d0^2)`, d the
distance from the user, d0 1 km (3 km as a variant), a rank-safe boost in
Strata that was checked against a brute-force ranking. The device holds the
places of a square of 1 km tiles around the user's tile (or around home)
and ranks them the same way with the cloud's statistics. Its top k is
proved the cloud's when the k-th local score is at least
`(sum of the query words' best city-wide BM25 scores) / (1 + e^2 / d0^2)`,
e the user's distance to the edge of the square: no place outside can score
more. A table of each word's best score ships with the slice.

    python geo.py prep      # places -> Strata, 1 minute
    python geo.py searches  # check-ins -> searches
    python geo.py eval      # cloud and device top 10s, about 20 minutes on 4 cores
    python geo.py bytes     # slice files at the median and 90th percentile areas
"""
import argparse
import collections
import datetime
import json
import math
import os
import shutil
import sys
import unicodedata

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import search  # noqa: E402
from search import SEED, log, normalise, run_all  # noqa: E402

strata = None
RELEASE = "2026-10-21.0"
BOX = (-74.26, -73.70, 40.49, 40.92)  # lon min, lon max, lat min, lat max
LAT0, LON0 = 40.7, -73.98
KX, KY = 111320 * math.cos(math.radians(LAT0)), 110540  # metres per degree
TILE = 1000.0
HALF = [500.0, 1500.0, 2500.0, 4500.0, 9500.0]  # squares of 1, 3, 5, 9 and 19 km a side, whole tiles
D0 = [1000.0, 3000.0]
KS = [1, 3, 10]
NAME_RADIUS = 50.0
SCHEMA = {"table": "p", "key": "id", "fields": [
    {"name": "id", "type": "int"},
    {"name": "body", "type": "text"},
    {"name": "addr", "type": "keyword", "indexed": False},
    {"name": "x", "type": "float"},
    {"name": "y", "type": "float"},
]}
# Check-ins that are not searches for a place: home, work, transit, roads,
# areas and the vaguest catch-all categories.
NOT_SEARCHED = {
    "Home (private)", "Office", "Subway", "Train Station", "Bus Station", "Light Rail", "Ferry", "Taxi", "Moving Target",
    "Neighborhood", "Residential Building (Apartment / Condo)", "Housing Development", "Building", "Road", "Bridge",
    "City", "Plaza", "River", "Rest Area", "Other Great Outdoors", "General Travel", "General Entertainment",
    "Professional & Other Places", "Travel & Transport", "Arts & Entertainment", "Outdoors & Recreation",
    "Shop & Service", "Food", "Nightlife Spot", "Other Nightlife", "College Academic Building", "College & University",
    "General College & University", "Student Center", "Fraternity House", "Sorority House", "Factory", "Military Base",
    "Miscellaneous Shop",
}
VAGUE = {"place", "joint", "spot", "general", "other", "private"}
GENERIC = VAGUE | {"restaurant", "shop", "store", "food", "center", "service", "house", "room", "hall", "venue", "building"}
SQL_SCORE = "bm25(body, :q, syntax => 'plain') / (1 + ((x - :ux) * (x - :ux) + (y - :uy) * (y - :uy)) / :d2)"
SQL_CLOUD = f"SELECT id, score() FROM p ORDER BY {SQL_SCORE} DESC LIMIT 10"
SQL_LOCAL = f"SELECT id, score() FROM p WHERE x >= :x0 AND x <= :x1 AND y >= :y0 AND y <= :y1 ORDER BY {SQL_SCORE} DESC LIMIT 10"
SQL_MAX = "SELECT score() FROM p ORDER BY bm25(body, :q, syntax => 'plain') DESC LIMIT 1"


def proj(lon, lat):
    return (lon - LON0) * KX, (lat - LAT0) * KY


def tile_centre(v):
    return math.floor(v / TILE) * TILE + TILE / 2


def db_path(a):
    return os.path.join(a.work, "nyc.strata")


def cmd_prep(a):
    import duckdb
    os.makedirs(a.work, exist_ok=True)
    src = os.path.join(a.work, "nyc_box.parquet")
    con = duckdb.connect()
    if not os.path.exists(src):  # anonymous read of Overture's public bucket
        for k in ("AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "AWS_SESSION_TOKEN"):
            os.environ.pop(k, None)
        con.sql("LOAD httpfs; SET s3_region='us-west-2';")
        con.sql(f"""COPY (
          SELECT id, names."primary" AS name, taxonomy."primary" AS category, taxonomy.hierarchy AS hierarchy,
                 taxonomy.alternates AS alternates, basic_category, brand.names."primary" AS brand,
                 addresses[1].freeform AS address, addresses[1].locality AS locality,
                 (bbox.xmin + bbox.xmax) / 2 AS lon, (bbox.ymin + bbox.ymax) / 2 AS lat, confidence, operating_status,
                 list_distinct([s.license for s in sources]) AS licenses
          FROM read_parquet('s3://overturemaps-us-west-2/release/{RELEASE}/theme=places/type=place/*.parquet')
          WHERE bbox.xmin > {BOX[0]} AND bbox.xmax < {BOX[1]} AND bbox.ymin > {BOX[2]} AND bbox.ymax < {BOX[3]}
        ) TO '{src}' (FORMAT parquet)""")
    total = con.sql(f"SELECT count(*) FROM '{src}'").fetchall()[0][0]
    rows = con.sql(f"""SELECT name, category, basic_category, alternates, hierarchy, brand, address, locality, lon, lat FROM '{src}'
        WHERE coalesce(operating_status, 'open') NOT IN ('permanently_closed', 'temporarily_closed') AND confidence >= 0.5
          AND name IS NOT NULL ORDER BY id""").fetchall()
    xs, ys = np.zeros(len(rows)), np.zeros(len(rows))
    out = os.path.join(a.work, "rows.jsonl")
    body_bytes = 0
    with open(out, "w") as f:
        for i, (n, c, b, alt, h, br, ad, loc, lon, lat) in enumerate(rows):
            cats = []
            for s in [c, b] + list(alt or []) + list(h or []):
                for w in (s or "").replace("_", " ").split():
                    if w not in cats:
                        cats.append(w)
            body = " ".join(t for t in (n, " ".join(cats), br if br and br != n else "", loc or "") if t)
            body_bytes += len(body.encode())
            xs[i], ys[i] = proj(lon, lat)
            f.write(json.dumps({"id": i, "body": body, "addr": ad or "", "x": xs[i], "y": ys[i]}) + "\n")
    for p in (db_path(a), db_path(a) + "-wal"):
        if os.path.exists(p):
            os.remove(p)
    db = strata.open(db_path(a), SCHEMA)
    log("imported", db.import_file(out))
    db.optimize()
    db.close()
    np.savez(os.path.join(a.work, "places.npz"), x=xs, y=ys)
    with open(os.path.join(a.work, "names.txt"), "w") as f:
        f.write("\n".join(" ".join(r[0].split()) for r in rows) + "\n")
    stats = {"release": RELEASE, "box": BOX, "places_in_box": total, "places": len(rows), "mean_body_bytes": body_bytes / len(rows)}
    json.dump(stats, open(os.path.join(a.work, "prep.json"), "w"), indent=1)
    log("prep", stats)


def ascii_of(s):
    return unicodedata.normalize("NFKD", s).encode("ascii", "ignore").decode()


def query_of(cat):
    words = ascii_of(cat).lower().replace("&", " ").replace("/", " ").replace("(", " ").replace(")", " ").replace("-", " ").split()
    return normalise(" ".join(w for w in words if w not in VAGUE))


def cmd_searches(a):
    """Check-ins -> searches: what the user looked for, and from where."""
    xs, ys = (np.load(os.path.join(a.work, "places.npz"))[k] for k in ("x", "y"))
    bodies = [json.loads(l)["body"] for l in open(os.path.join(a.work, "rows.jsonl"))]
    names = open(os.path.join(a.work, "names.txt")).read().split("\n")
    tokens = {}

    def words_of(i):
        if i not in tokens:
            tokens[i] = set((normalise(ascii_of(bodies[i])) or "").split())
        return tokens[i]
    grid = collections.defaultdict(list)  # 100 m cells of places
    for i in range(len(xs)):
        grid[(int(xs[i] // 100), int(ys[i] // 100))].append(i)
    checkins = collections.defaultdict(list)
    for line in open(a.checkins, encoding="latin-1"):
        user, venue, _, cat, lat, lon, _, ts = line.rstrip("\n").split("\t")
        t = datetime.datetime.strptime(ts, "%a %b %d %H:%M:%S %z %Y")
        checkins[user].append((t, venue, cat, *proj(float(lon), float(lat))))
    lo, hi = proj(BOX[0], BOX[2]), proj(BOX[1], BOX[3])
    inside = lambda x, y: lo[0] <= x <= hi[0] and lo[1] <= y <= hi[1]
    name_of = {}

    def name_query(venue, x, y, q):
        if venue in name_of:
            return name_of[venue]
        want = set(q.split()) - GENERIC
        best = None
        for cx in range(int(x // 100) - 1, int(x // 100) + 2):
            for cy in range(int(y // 100) - 1, int(y // 100) + 2):
                for i in grid.get((cx, cy), ()):
                    d = math.hypot(xs[i] - x, ys[i] - y)
                    if d <= NAME_RADIUS and (best is None or d < best[0]) and want & words_of(i):
                        best = (d, i)
        name_of[venue] = normalise(ascii_of(names[best[1]])) if best else None
        return name_of[venue]

    out, skipped = [], collections.Counter()
    for user, cs in checkins.items():
        cs.sort()
        homes = collections.Counter((x, y) for _, _, c, x, y in cs if c == "Home (private)")
        home = (homes or collections.Counter((x, y) for _, _, _, x, y in cs)).most_common(1)[0][0]
        for prev, cur in zip(cs, cs[1:]):
            _, venue, cat, x, y = cur
            ux, uy = prev[3], prev[4]
            if cat in NOT_SEARCHED:
                skipped["not a search"] += 1
                continue
            if not (inside(x, y) and inside(ux, uy) and inside(*home)):
                skipped["outside the box"] += 1
                continue
            q = query_of(cat)
            if not q:
                skipped["no words"] += 1
                continue
            out.append({"kind": "category", "q": q, "ux": ux, "uy": uy, "hx": home[0], "hy": home[1]})
            n = name_query(venue, x, y, q)
            if n:
                out.append({"kind": "name", "q": n, "ux": ux, "uy": uy, "hx": home[0], "hy": home[1]})
            else:
                skipped["name: no place within 50 m"] += 1
    with open(os.path.join(a.work, "searches.jsonl"), "w") as f:
        for s in out:
            f.write(json.dumps(s) + "\n")
    kinds = collections.Counter(s["kind"] for s in out)
    dists = np.array([math.hypot(s["ux"] - s["hx"], s["uy"] - s["hy"]) for s in out if s["kind"] == "category"])
    stats = {"checkins": sum(len(c) for c in checkins.values()), "users": len(checkins), "searches": dict(kinds), "skipped": dict(skipped),
             "distinct_queries": {k: len({s["q"] for s in out if s["kind"] == k}) for k in kinds},
             "venues_named": sum(1 for v in name_of.values() if v), "venues": len(name_of),
             "from_home_m": {"median": float(np.median(dists)), "p90": float(np.percentile(dists, 90))}}
    json.dump(stats, open(os.path.join(a.work, "searches.json"), "w"), indent=1)
    log("searches", stats)


def areas(cx, cy, h):
    return {"x0": cx - h, "x1": cx + h, "y0": cy - h, "y1": cy + h}


def cmd_eval(a):
    db = strata.Database.open(db_path(a), threads=1)
    ss = [json.loads(l) for l in open(os.path.join(a.work, "searches.jsonl"))]
    if a.sample:
        ss = [ss[i] for i in sorted(np.random.default_rng(SEED).choice(len(ss), a.sample, replace=False))]
    words = sorted({w for s in ss for w in s["q"].split()})
    best = dict(zip(words, run_all(words, lambda w: (lambda r: float(r.rows[0][0]) if r.rows else 0.0)(db.query(SQL_MAX, {"q": w})), a.threads, "word maxima")))
    json.dump({"words": len(words)}, open(os.path.join(a.work, "maxima.json"), "w"))

    def one(s):
        bound = sum(best[w] for w in s["q"].split())
        res = []
        for d0 in D0:
            p = {"q": s["q"], "ux": s["ux"], "uy": s["uy"], "d2": d0 * d0}
            cloud = [(int(r[0]), float(r[1])) for r in db.query(SQL_CLOUD, p).rows if r[1] > 0]
            for mode, (cx, cy) in (("here", (tile_centre(s["ux"]), tile_centre(s["uy"]))), ("home", (tile_centre(s["hx"]), tile_centre(s["hy"])))):
                for h in HALF:
                    edge = h - max(abs(s["ux"] - cx), abs(s["uy"] - cy))
                    local = [(int(r[0]), float(r[1])) for r in db.query(SQL_LOCAL, {**p, **areas(cx, cy, h)}).rows if r[1] > 0] if edge > 0 else []
                    out = bound / (1 + edge * edge / (d0 * d0)) if edge > 0 else math.inf
                    flags = []
                    for k in KS:
                        ck, lk = [d for d, _ in cloud[:k]], [d for d, _ in local[:k]]
                        same = ck == lk and len(ck) > 0
                        proved = len(local) >= k and local[k - 1][1] >= out
                        flags.append((same, proved))
                    res.append((d0, mode, h, flags, len(cloud)))
        return res

    rs = run_all(ss, one, a.threads, "searches")
    agg = {}
    for s, r in zip(ss, rs):
        for d0, mode, h, flags, n_cloud in r:
            key = (s["kind"], str(int(d0)), mode, str(int(h)))
            d = agg.setdefault(key, {"n": 0, "full": 0, **{str(k): {"same": 0, "proved": 0, "wrong": 0} for k in KS}})
            d["n"] += 1
            d["full"] += n_cloud == 10
            for k, (same, proved) in zip(KS, flags):
                d[str(k)]["same"] += same
                d[str(k)]["proved"] += proved
                d[str(k)]["wrong"] += proved and not same
    out = {}
    for (kind, d0, mode, h), d in agg.items():
        n = d["n"]
        out.setdefault(kind, {}).setdefault(d0, {}).setdefault(mode, {})[h] = {
            "searches": n, "cloud_full_top10": d["full"] / n,
            **{k: {"same": d[k]["same"] / n, "proved": d[k]["proved"] / n, "wrong_proofs": d[k]["wrong"]} for k in map(str, KS)}}
    json.dump(out, open(os.path.join(a.work, "eval.json"), "w"), indent=1)
    log("eval written")


def counts(a):
    """Places in each search's square, by mode and size (squares are whole tiles)."""
    xs, ys = (np.load(os.path.join(a.work, "places.npz"))[k] for k in ("x", "y"))
    ix, iy = np.floor(xs / TILE).astype(int), np.floor(ys / TILE).astype(int)
    ox, oy = ix.min() - 20, iy.min() - 20
    hist = np.zeros((ix.max() - ox + 21, iy.max() - oy + 21), np.int64)
    np.add.at(hist, (ix - ox, iy - oy), 1)
    cum = np.pad(hist.cumsum(0).cumsum(1), ((1, 0), (1, 0)))
    ss = [json.loads(l) for l in open(os.path.join(a.work, "searches.jsonl"))]
    res = {}
    for mode, (kx, ky) in (("here", ("ux", "uy")), ("home", ("hx", "hy"))):
        tx = np.array([math.floor(s[kx] / TILE) for s in ss if s["kind"] == "category"]) - ox
        ty = np.array([math.floor(s[ky] / TILE) for s in ss if s["kind"] == "category"]) - oy
        for h in HALF:
            r = int(h // TILE)
            x0, x1 = np.clip(tx - r, 0, hist.shape[0]), np.clip(tx + r + 1, 0, hist.shape[0])
            y0, y1 = np.clip(ty - r, 0, hist.shape[1]), np.clip(ty + r + 1, 0, hist.shape[1])
            res[(mode, h)] = (cum[x1, y1] - cum[x0, y1] - cum[x1, y0] + cum[x0, y0], tx, ty, r, ox, oy)
    return res


def cmd_bytes(a):
    """Slice files for the squares at the median and 90th percentile counts."""
    rows = [l for l in open(os.path.join(a.work, "rows.jsonl"))]
    xs, ys = (np.load(os.path.join(a.work, "places.npz"))[k] for k in ("x", "y"))
    out = {}
    tmp = os.path.join(a.work, "bytes")
    os.makedirs(tmp, exist_ok=True)
    for (mode, h), (n, tx, ty, r, ox, oy) in counts(a).items():
        d = out.setdefault(mode, {}).setdefault(str(int(h)), {})
        for label, pc in (("median", 50), ("p90", 90)):
            target = np.percentile(n, pc)
            j = int(np.argmin(np.abs(n - target)))
            cx, cy = (tx[j] + ox) * TILE + TILE / 2, (ty[j] + oy) * TILE + TILE / 2
            keep = np.nonzero((np.abs(xs - cx) <= h) & (np.abs(ys - cy) <= h))[0]
            src = os.path.join(tmp, "rows.jsonl")
            with open(src, "w") as f:
                for i in keep:
                    f.write(rows[i])
            path = os.path.join(tmp, "slice.strata")
            for p in (path, path + "-wal"):
                if os.path.exists(p):
                    os.remove(p) if os.path.isfile(p) else shutil.rmtree(p)
            db = strata.open(path, SCHEMA)
            db.import_file(src)
            db.optimize()
            db.close()
            size = os.path.getsize(path)
            d[label] = {"places": int(len(keep)), "file_bytes": size}
            log("bytes", mode, h, label, len(keep), size)
    # The bounds table a slice ships: every city word and its best score in
    # one byte, unpacked. Words split as search.WORD does, before stemming,
    # so this overcounts the analyzer's terms a little.
    vocab = set()
    for line in rows:
        vocab.update(search.WORD.findall(json.loads(line)["body"].lower()))
    out["maxima_table"] = {"words": len(vocab), "bytes": sum(len(w) + 2 for w in vocab),
                           "search_words": json.load(open(os.path.join(a.work, "maxima.json")))["words"]}
    json.dump(out, open(os.path.join(a.work, "bytes.json"), "w"), indent=1)


def main():
    global strata
    ap = argparse.ArgumentParser()
    ap.add_argument("--work", default="/home/user/data/geo/work")
    ap.add_argument("--checkins", default="/home/user/data/geo/tsmc/dataset_tsmc2014/dataset_TSMC2014_NYC.txt")
    ap.add_argument("--threads", type=int, default=4)
    ap.add_argument("--sample", type=int, default=0, help="eval: a seeded sample of the searches")
    ap.add_argument("cmd", choices=["prep", "searches", "eval", "bytes"])
    a = ap.parse_args()
    import strata as st
    strata = search.strata = st
    {"prep": cmd_prep, "searches": cmd_searches, "eval": cmd_eval, "bytes": cmd_bytes}[a.cmd](a)


if __name__ == "__main__":
    main()
