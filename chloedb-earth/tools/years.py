#!/usr/bin/env python3
"""Changed places: a place's look every other year, for Earth explorer's
tables ea_years and ea_changes.

For a few hundred places of each region (those that burned, the mines, the
edges of towns, the lakes and reservoirs, the farms, and some at random),
this takes the clearest Sentinel-2 L2A scene of each dry season (June to
September in California, January to March in Chile) from Element 84's
Earth Search, cuts the place's 3.84 km square out of its true colour
picture (384 by 384 pixels of 10 metres, as Major TOM's), and embeds it with
the SigLIP model of the places (so400m) through the ChloeDB server, so a
year's look is comparable with the others and with words.

Steps, each its own command:

  pick   the places, from ea_places_<region>.jsonl (places.py rows)
  grow   more places, from the server's ea_places, leaving out those
         already in ea_years (the steps after it then do only the new)
  fetch  each place's picture of each year, into <out>/chips/<place>_<year>.jpg
  embed  their vectors, by the server's POST /v1/embed (the slow step:
         seconds a picture on four cores)
  watch  the places of fire watches that asked to be told of a change:
         their years, then each new clear picture, written to the server
  rows   ea_years.jsonl (a row a place and year whose picture is clear, its
         label the nearest of a few words) and ea_changes.jsonl (a row a place: its first and last
         look, how far it moved, the year it moved most, the direction)

  python3 -I tools/years.py pick  --out DIR [--per-region 150]
  python3 -I tools/years.py grow  --out DIR --per-region 900 --strata URL   (CHLOEDB_KEY)
  python3 -I tools/years.py fetch --out DIR
  python3 -I tools/years.py embed --out DIR --strata http://127.0.0.1:7700
  python3 -I tools/years.py rows  --out DIR --strata http://127.0.0.1:7700
  python3 -I tools/years.py watch --out DIR --loop     (the watched places, every 6 hours)

Then `chloedb-earth load --dir DIR --only ea_years,ea_changes`, and serve
with `--chips DIR/chips`. Needs numpy, rasterio and Pillow.
"""

import argparse
import base64
import io
import json
import math
import os
import random
import sys
import time
import urllib.request
from concurrent.futures import ThreadPoolExecutor

import numpy as np

STAC = "https://earth-search.aws.element84.com/v1/search"
YEARS = [2017, 2019, 2021, 2023, 2025]
SEASON = {"california": ("{y}-06-15", "{y}-09-30"), "chile": ("{y}-01-10", "{y}-03-31")}
SIDE = 384
LABELS = ["a forest", "a burn scar", "shrubland", "dry grassland", "green fields", "orchards", "vineyards", "a city",
          "suburbs", "a solar farm", "an open pit mine", "evaporation ponds", "a lake", "a dry lake bed", "a desert",
          "snow and glaciers", "a clear cut forest", "a pine plantation", "a salt flat", "a coast", "a river valley"]


def log(*a):
    print("years:", *a, file=sys.stderr, flush=True)


def post(url, body, timeout=120):
    headers = {"content-type": "application/json"}
    if os.environ.get("CHLOEDB_KEY"):
        headers["authorization"] = "Bearer " + os.environ["CHLOEDB_KEY"]
    req = urllib.request.Request(url, data=json.dumps(body).encode(), headers=headers)
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read())


# --- pick -------------------------------------------------------------------

FIELDS = ["id", "region", "lat", "lon", "town", "adm1", "kind", "trees", "shrubs", "crops", "built", "water", "mine",
          "people", "fire_years", "fire_px", "last_fire", "climate"]


def read_places(out, region):
    path = f"{out}/ea_places_{region}.jsonl"
    keep = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            # The look is the last field: cut it off before parsing.
            i = line.rfind(',"look":')
            row = json.loads(line[:i] + "}" if i > 0 else line)
            keep.append({k: row.get(k) for k in FIELDS})
    return keep


def pick(a):
    rnd = random.Random(7)
    chosen = []
    for region in ("california", "chile"):
        ps = read_places(a.out, region)
        n = a.per_region
        z = lambda x: x or 0  # noqa: E731
        groups = [
            ("burned", 0.3, sorted((p for p in ps if p["last_fire"] and 2018 <= p["last_fire"] <= 2024 and z(p["trees"]) + z(p["shrubs"]) >= 40), key=lambda p: -z(p["fire_px"]))),
            ("mine", 0.15, sorted((p for p in ps if z(p["mine"]) >= 5), key=lambda p: -z(p["mine"]))),
            ("edge of a town", 0.15, sorted((p for p in ps if 8 <= z(p["built"]) <= 35 and z(p["people"]) >= 2000), key=lambda p: -z(p["people"]))),
            ("water", 0.12, sorted((p for p in ps if z(p["water"]) >= 12), key=lambda p: -z(p["water"]))),
            ("farms", 0.13, [p for p in ps if z(p["crops"]) >= 40]),
            ("random", 0.15, list(ps)),
        ]
        taken = []

        def far(p):
            return all(abs(p["lat"] - q["lat"]) > 0.06 or abs(p["lon"] - q["lon"]) > 0.06 / max(0.2, math.cos(math.radians(p["lat"]))) for q in taken)

        for why, share, cands in groups:
            if why in ("farms", "random"):
                rnd.shuffle(cands)
            want = round(n * share)
            got = 0
            for p in cands:
                if got >= want:
                    break
                if far(p):
                    taken.append({**p, "why": why})
                    got += 1
        log(region, len(taken), "places:", {w: sum(1 for t in taken if t["why"] == w) for w, _, _ in groups})
        chosen += taken
    with open(f"{a.out}/years_places.json", "w", encoding="utf-8") as f:
        json.dump(chosen, f, ensure_ascii=False)


def grow(a):
    """More places, picked from the server's own rows: the places with a
    solar farm first (most were built since 2017), then the burned, the
    mines, the edges of towns, the water, the farms and some at random,
    none next to a place already picked or already kept in
    ea_years. Only the new ones go into years_places.json, so the next
    steps fetch, embed and write only theirs."""
    def q(sql, params):
        return post(f"{a.strata}/v1/query", {"sql": sql, "params": params}, 300)

    def rows_of(j):
        return [dict(zip(j["columns"], r)) for r in j["rows"]]

    cols = "id, region, lat, lon, town, adm1, kind, trees, shrubs, crops, built, water, mine, people, fire_px, last_fire, solar"
    kept = {r["place"] for r in rows_of(q("SELECT place, COUNT(*) AS n FROM ea_years GROUP BY place LIMIT 10000", {}))}
    rnd = random.Random(11)
    chosen = []
    for region in ("california", "chile"):
        p = {"r": region}
        groups = [
            ("solar farm", 0.25, f"SELECT {cols} FROM ea_places WHERE region = :r AND solar >= 0.05 ORDER BY solar DESC LIMIT 10000"),
            ("burned", 0.2, f"SELECT {cols} FROM ea_places WHERE region = :r AND last_fire >= 2018 AND last_fire <= 2024 AND trees + shrubs >= 40 ORDER BY fire_px DESC LIMIT 10000"),
            ("mine", 0.12, f"SELECT {cols} FROM ea_places WHERE region = :r AND mine >= 5 ORDER BY mine DESC LIMIT 10000"),
            ("edge of a town", 0.15, f"SELECT {cols} FROM ea_places WHERE region = :r AND built >= 8 AND built <= 35 AND people >= 2000 ORDER BY people DESC LIMIT 10000"),
            ("water", 0.08, f"SELECT {cols} FROM ea_places WHERE region = :r AND water >= 12 ORDER BY water DESC LIMIT 10000"),
            ("farms", 0.1, f"SELECT {cols} FROM ea_places WHERE region = :r AND crops >= 40 LIMIT 10000"),
            ("random", 0.1, f"SELECT {cols} FROM ea_places WHERE region = :r LIMIT 10000"),
        ]
        old = rows_of(q(f"SELECT {cols} FROM ea_places WHERE region = :r AND id IN (:ids) LIMIT 10000", {**p, "ids": sorted(kept)})) if kept else []
        taken = list(old)
        new = []

        def far(c):
            return all(abs(c["lat"] - t["lat"]) > 0.04 or abs(c["lon"] - t["lon"]) > 0.04 / max(0.2, math.cos(math.radians(c["lat"]))) for t in taken)

        for why, share, sql in groups:
            cands = [c for c in rows_of(q(sql, p)) if c["id"] not in kept]
            if why in ("farms", "random"):
                rnd.shuffle(cands)
            want, got = round(a.per_region * share), 0
            for c in cands:
                if got >= want:
                    break
                if far(c):
                    c["why"] = why
                    taken.append(c)
                    new.append(c)
                    got += 1
        log(region, len(old), "kept,", len(new), "new:", {w: sum(1 for t in new if t["why"] == w) for w, _, _ in groups})
        chosen += new
    with open(f"{a.out}/years_places.json", "w", encoding="utf-8") as f:
        json.dump(chosen, f, ensure_ascii=False)


# --- fetch ------------------------------------------------------------------

def scenes(lat, lon, region, year):
    a, b = (s.format(y=year) for s in SEASON[region])
    body = {"collections": ["sentinel-2-l2a"], "intersects": {"type": "Point", "coordinates": [lon, lat]},
            "datetime": f"{a}T00:00:00Z/{b}T23:59:59Z", "query": {"eo:cloud_cover": {"lt": 20}},
            "sortby": [{"field": "properties.eo:cloud_cover", "direction": "asc"}], "limit": 8,
            "fields": {"include": ["id", "properties.datetime", "properties.eo:cloud_cover", "assets.visual"], "exclude": ["links", "geometry"]}}
    for i in range(4):
        try:
            return post(STAC, body, 60).get("features", [])
        except Exception as e:  # noqa: BLE001
            if i == 3:
                raise
            time.sleep(2 + 3 * i)
    return []


def cut(href, lat, lon):
    import rasterio
    from rasterio.warp import transform
    from rasterio.windows import Window
    with rasterio.open(href) as ds:
        xs, ys = transform("EPSG:4326", ds.crs, [lon], [lat])
        r, c = ds.index(xs[0], ys[0])
        w = Window(c - SIDE // 2, r - SIDE // 2, SIDE, SIDE)
        if w.col_off < 0 or w.row_off < 0 or w.col_off + SIDE > ds.width or w.row_off + SIDE > ds.height:
            return None
        img = ds.read(window=w)
    return np.transpose(img[:3], (1, 2, 0))


def usable(img):
    """The share of the square with no data, and the share that is white
    (clouds, or snow)."""
    empty = float(np.mean(np.all(img == 0, axis=2)))
    white = float(np.mean(np.all(img >= 225, axis=2)))
    return empty, white


def fetch_one(a, p, year):
    out = f"{a.out}/chips/{p['id']}_{year}.jpg"
    if os.path.exists(out):
        return "kept"
    from PIL import Image
    best = None
    for s in scenes(p["lat"], p["lon"], p["region"], year):
        href = s["assets"]["visual"]["href"]
        try:
            img = cut(href, p["lat"], p["lon"])
        except Exception as e:  # noqa: BLE001
            log("read", s["id"], e)
            continue
        if img is None:
            continue
        empty, white = usable(img)
        score = empty * 4 + white
        if best is None or score < best[0]:
            best = (score, img, s)
        if empty < 0.005 and white < 0.02:
            break
    if best is None or best[0] > 0.6:
        return "none"
    _, img, s = best
    buf = io.BytesIO()
    Image.fromarray(img).save(buf, "JPEG", quality=86)
    with open(out + ".part", "wb") as f:
        f.write(buf.getvalue())
    os.replace(out + ".part", out)
    meta = {"place": p["id"], "year": year, "scene": s["id"], "taken": s["properties"]["datetime"], "cloud": s["properties"].get("eo:cloud_cover"), "white": round(usable(img)[1], 4)}
    with open(f"{a.out}/chips/{p['id']}_{year}.json", "w") as f:
        json.dump(meta, f)
    return "got"


def fetch(a):
    os.environ.setdefault("GDAL_DISABLE_READDIR_ON_OPEN", "EMPTY_DIR")
    os.environ.setdefault("CPL_VSIL_CURL_ALLOWED_EXTENSIONS", ".tif")
    os.environ.setdefault("GDAL_HTTP_MAX_RETRY", "4")
    os.environ.setdefault("GDAL_HTTP_RETRY_DELAY", "2")
    # Behind a proxy with its own CA: the bundle requests uses, for GDAL's curl too.
    ca = os.environ.get("REQUESTS_CA_BUNDLE") or os.environ.get("SSL_CERT_FILE")
    if ca and os.path.exists(ca):
        os.environ.setdefault("CURL_CA_BUNDLE", ca)
        os.environ.setdefault("GDAL_CURL_CA_BUNDLE", ca)
    os.makedirs(f"{a.out}/chips", exist_ok=True)
    ps = json.load(open(f"{a.out}/years_places.json", encoding="utf-8"))
    jobs = [(p, y) for p in ps for y in YEARS]
    t0 = time.time()
    n = {"got": 0, "kept": 0, "none": 0, "error": 0}

    def run(j):
        try:
            return fetch_one(a, *j)
        except Exception as e:  # noqa: BLE001
            log("place", j[0]["id"], j[1], e)
            return "error"

    with ThreadPoolExecutor(a.parallel) as ex:
        for i, r in enumerate(ex.map(run, jobs)):
            n[r] += 1
            if (i + 1) % 50 == 0:
                log(i + 1, "of", len(jobs), n, round(time.time() - t0), "s")
    log("done", n, round(time.time() - t0), "s")


# --- embed ------------------------------------------------------------------

def embed(a):
    ps = json.load(open(f"{a.out}/years_places.json", encoding="utf-8"))
    path = f"{a.out}/years_vectors.jsonl"
    have = set()
    if os.path.exists(path):
        with open(path) as f:
            for line in f:
                have.add(line[:line.index(',"v"')].split('"k":"')[1].rstrip('"'))
    todo = [f"{p['id']}_{y}" for p in ps for y in YEARS if os.path.exists(f"{a.out}/chips/{p['id']}_{y}.jpg") and f"{p['id']}_{y}" not in have]
    log(len(have), "embedded,", len(todo), "to go")
    t0 = time.time()

    def one(k):
        b = base64.b64encode(open(f"{a.out}/chips/{k}.jpg", "rb").read()).decode()
        for i in range(3):
            try:
                return k, post(f"{a.strata}/v1/embed", {"image": b, "table": "ea_words", "field": "emb"}, 300)["vector"]
            except Exception as e:  # noqa: BLE001
                if i == 2:
                    log("embed", k, e)
                    return k, None
                time.sleep(5)

    with open(path, "a") as f, ThreadPoolExecutor(a.parallel) as ex:
        for i, (k, v) in enumerate(ex.map(one, todo)):
            if v is None:
                continue
            f.write(json.dumps({"k": k, "v": [round(x, 6) for x in v]}, separators=(",", ":")) + "\n")
            f.flush()
            if (i + 1) % 20 == 0:
                el = time.time() - t0
                log(i + 1, "of", len(todo), f"{el / (i + 1):.1f} s a picture,", round((len(todo) - i - 1) * el / (i + 1) / 60), "min to go")


# --- rows -------------------------------------------------------------------

def clear(path):
    """Whether a year's picture can stand for the place: not dark or flat
    (deep water, shadowed forest: Sentinel-2's true colour leaves them near
    black, and their looks drift with the noise), not cloudy. On the 1,237
    pictures fetched on 18 and 19 November 2026, a mean under 25 of 255 or
    a spread under 6 was a lake or a black square, and white over 5% was
    cloud or snow; 957 passed. A test for grey haze was tried and left out:
    it took towns and mines too."""
    from PIL import Image
    a = np.asarray(Image.open(path).convert("RGB"), dtype=np.float32)
    white = float(np.mean(np.all(a >= 225, axis=2)))
    return a.mean() >= 25 and a.mean(axis=2).std() >= 6 and white <= 0.05


def unit(v):
    v = np.asarray(v, dtype=np.float32)
    return v / (np.linalg.norm(v) or 1.0)


def rows(a):
    ps = {p["id"]: p for p in json.load(open(f"{a.out}/years_places.json", encoding="utf-8"))}
    vec = {}
    with open(f"{a.out}/years_vectors.jsonl") as f:
        for line in f:
            r = json.loads(line)
            vec[r["k"]] = unit(r["v"])
    words = np.stack([unit(post(f"{a.strata}/v1/embed", {"text": f"a satellite photo of {w}", "table": "ea_words", "field": "emb"})["vector"]) for w in LABELS])
    ny = nc = 0
    with open(f"{a.out}/ea_years.jsonl.part", "w", encoding="utf-8") as fy, open(f"{a.out}/ea_changes.jsonl.part", "w", encoding="utf-8") as fc:
        for pid, p in ps.items():
            seen = []
            for y in YEARS:
                k = f"{pid}_{y}"
                if k not in vec or not clear(f"{a.out}/chips/{k}.jpg"):
                    continue
                m = json.load(open(f"{a.out}/chips/{k}.json"))
                v = vec[k]
                label = LABELS[int(np.argmax(words @ v))].removeprefix("a ").removeprefix("an ")
                seen.append((y, v, label))
                fy.write(json.dumps({"id": f"{pid}-{y}", "place": pid, "region": p["region"], "year": y, "taken": m["taken"][:19] + "Z",
                                     "scene": m["scene"], "cloud": m["cloud"], "ndvi": None, "label": label,
                                     "look": [round(float(x), 6) for x in v]}, separators=(",", ":"), ensure_ascii=False) + "\n")
                ny += 1
            if len(seen) < 3:
                continue
            first, last = seen[0], seen[-1]
            steps = [(seen[i][0], 1 - float(seen[i - 1][1] @ seen[i][1])) for i in range(1, len(seen))]
            sy, sv = max(steps, key=lambda s: s[1])
            arc = [seen[0][2]]
            for _, _, l in seen[1:]:
                if l != arc[-1]:
                    arc.append(l)
            d = last[1] - first[1]
            fc.write(json.dumps({"id": pid, "region": p["region"], "lat": p["lat"], "lon": p["lon"], "town": p["town"], "adm1": p["adm1"],
                                 "first": first[0], "last": last[0], "drift": round(1 - float(first[1] @ last[1]), 5),
                                 "step_year": sy, "step": round(sv, 5), "from_label": first[2], "to_label": last[2],
                                 "arc": " → ".join(arc),
                                 "before": [round(float(x), 6) for x in first[1]], "after": [round(float(x), 6) for x in last[1]],
                                 "delta": [round(float(x), 6) for x in unit(d)]}, separators=(",", ":"), ensure_ascii=False) + "\n")
            nc += 1
    os.replace(f"{a.out}/ea_years.jsonl.part", f"{a.out}/ea_years.jsonl")
    os.replace(f"{a.out}/ea_changes.jsonl.part", f"{a.out}/ea_changes.jsonl")
    log("wrote", ny, "years and", nc, "changes")


# --- watch ------------------------------------------------------------------

def newest(lat, lon, since):
    """The scenes over a point since a day, newest first."""
    body = {"collections": ["sentinel-2-l2a"], "intersects": {"type": "Point", "coordinates": [lon, lat]},
            "datetime": f"{since}T00:00:00Z/..", "query": {"eo:cloud_cover": {"lt": 20}},
            "sortby": [{"field": "properties.datetime", "direction": "desc"}], "limit": 8,
            "fields": {"include": ["id", "properties.datetime", "properties.eo:cloud_cover", "assets.visual"], "exclude": ["links", "geometry"]}}
    return post(STAC, body, 60).get("features", [])


def watched(a):
    """The places of the fire watches that asked to be told of a change:
    each one's years, when it has none yet (a few minutes of the server's
    time a place), then every `--every` seconds its newest clear picture
    since its last, in ea_years with the day it was taken
    (<place>-<YYYYMMDD>), which the app compares with the one before."""
    os.environ.setdefault("GDAL_DISABLE_READDIR_ON_OPEN", "EMPTY_DIR")
    os.environ.setdefault("CPL_VSIL_CURL_ALLOWED_EXTENSIONS", ".tif")
    os.makedirs(f"{a.out}/chips", exist_ok=True)
    from PIL import Image

    def q(sql, params):
        j = post(f"{a.strata}/v1/query", {"sql": sql, "params": params}, 120)
        return [dict(zip(j["columns"], r)) for r in j["rows"]]

    words = None
    while True:
        t0 = time.time()
        try:
            ws = q("SELECT place, region FROM ea_watches WHERE change = true LIMIT 10000", {})
            places = {w["place"]: w["region"] for w in ws if w.get("place")}
            if places and words is None:
                words = np.stack([unit(post(f"{a.strata}/v1/embed", {"text": f"a satellite photo of {w}", "table": "ea_words", "field": "emb"})["vector"]) for w in LABELS])
            got = 0
            for pid, region in places.items():
                ps = q("SELECT id, region, lat, lon FROM ea_places WHERE id = :id", {"id": pid})
                if not ps:
                    continue
                p = ps[0]
                have = q("SELECT id, taken FROM ea_years WHERE place = :p ORDER BY taken DESC LIMIT 1", {"p": pid})
                todo = [] if have else list(YEARS)
                since = (have[0]["taken"][:10] if have else time.strftime("%Y-%m-%d", time.gmtime(time.time() - 60 * 86400)))
                # Its newest scene whose square is whole and clear, if newer than its last.
                for sc in newest(p["lat"], p["lon"], since):
                    day = sc["properties"]["datetime"][:10]
                    if have and day <= have[0]["taken"][:10]:
                        break
                    key = int(day.replace("-", ""))
                    path = f"{a.out}/chips/{pid}_{key}.jpg"
                    try:
                        img = cut(sc["assets"]["visual"]["href"], p["lat"], p["lon"])
                    except Exception as e:  # noqa: BLE001
                        log("read", sc["id"], e)
                        continue
                    if img is None or usable(img)[0] > 0.005:
                        continue
                    Image.fromarray(img).save(path, "JPEG", quality=86)
                    if not clear(path):
                        os.remove(path)
                        continue
                    with open(f"{a.out}/chips/{pid}_{key}.json", "w") as f:
                        json.dump({"place": pid, "year": key, "scene": sc["id"], "taken": sc["properties"]["datetime"], "cloud": sc["properties"].get("eo:cloud_cover")}, f)
                    todo.append(key)
                    break
                for key in todo:
                    if key < 10000 and fetch_one(a, p, key) not in ("got", "kept"):
                        continue
                    path = f"{a.out}/chips/{pid}_{key}.jpg"
                    if not clear(path):
                        continue
                    b = base64.b64encode(open(path, "rb").read()).decode()
                    v = unit(post(f"{a.strata}/v1/embed", {"image": b, "table": "ea_words", "field": "emb"}, 300)["vector"])
                    m = json.load(open(f"{a.out}/chips/{pid}_{key}.json"))
                    label = LABELS[int(np.argmax(words @ v))].removeprefix("a ").removeprefix("an ")
                    doc = {"id": f"{pid}-{key}", "place": pid, "region": region, "year": key if key < 10000 else key // 10000, "taken": m["taken"][:19] + "Z",
                           "scene": m["scene"], "cloud": m["cloud"], "ndvi": None, "label": label, "look": [round(float(x), 6) for x in v]}
                    post(f"{a.strata}/v1/write", {"ops": [{"op": "upsert", "table": "ea_years", "doc": doc}]})
                    got += 1
            log(len(places), "watched places,", got, "new pictures in", round(time.time() - t0), "s")
        except Exception as e:  # noqa: BLE001
            log("watch:", e)
        if not a.loop:
            return
        time.sleep(max(60, a.every - (time.time() - t0)))


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("step", choices=["pick", "grow", "fetch", "embed", "rows", "watch"])
    p.add_argument("--loop", action="store_true", help="watch: again every --every seconds")
    p.add_argument("--every", type=int, default=6 * 3600)
    p.add_argument("--out", required=True)
    p.add_argument("--per-region", type=int, default=150)
    p.add_argument("--parallel", type=int, default=8)
    p.add_argument("--strata", default=os.environ.get("CHLOEDB_URL", "http://127.0.0.1:7700"))
    a = p.parse_args()
    {"pick": pick, "grow": grow, "fetch": fetch, "embed": embed, "rows": rows, "watch": watched}[a.step](a)


if __name__ == "__main__":
    main()
