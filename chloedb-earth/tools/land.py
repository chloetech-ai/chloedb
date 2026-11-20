#!/usr/bin/env python3
"""What lies around each Earth explorer place for building on it: the
grid, the roads, the solar farms already built and the protected land
(columns of ea_places, for the Land finder).

  grid       OpenStreetMap's power lines (power=line, with their voltage)
             and substations (power=substation): the nearest of each, how
             far, its voltage and name
  roads      OpenStreetMap's motorways, trunk, primary and secondary roads:
             the nearest, how far and its name
  solar      OpenStreetMap's solar farms (power=plant with plant:source=solar,
             or a generator area with generator:source=solar, a hectare or
             more, so not a roof): the share of the place they cover, and
             the nearest one
  protected  PAD-US 4.1 (USGS, public domain) in California, every fee area
             with its GAP status; OpenStreetMap's protected areas in Chile:
             the share of the place protected, the share strictly protected
             (GAP 1 and 2; national parks, nature reserves and IUCN classes
             I to IV in Chile) and the name of the largest

OpenStreetMap's data are © OpenStreetMap contributors, under the ODbL.

Steps:

  places   the places' ids and centres from the server (CHLOEDB_URL,
           CHLOEDB_KEY) into <out>/land/places.json
  osm      a region's extract (a .osm.pbf, from download.openstreetmap.fr or
           Geofabrik) cut to what is used, into <out>/land/<region>_osm.pkl
  padus    California's PAD-US fee areas from USGS's feature service, into
           <out>/land/california_padus.json
  rows     set ops for POST /v1/write, a batch of 2,000 a line, into
           <out>/land/patch_land_<region>.jsonl, and a summary
  write    posts a region's set ops to the server (CHLOEDB_URL,
           CHLOEDB_KEY), after `chloedb-earth load` has added the columns

  python3 -I tools/land.py places --out DIR
  python3 -I tools/land.py osm --region chile --pbf chile-latest.osm.pbf --out DIR
  python3 -I tools/land.py padus --out DIR
  python3 -I tools/land.py rows --region chile --out DIR
  python3 -I tools/land.py write --region chile --out DIR

Needs numpy, osmium (pyosmium 4) and shapely 2.
"""

import argparse
import json
import math
import os
import pickle
import sys
import time
import urllib.parse
import urllib.request

import numpy as np

HALF = 3.84 / 2 / 111.32
LON0 = {"california": -119.0, "chile": -70.5}
SOLAR_KM2 = 0.01
ROADS = {"motorway": 1, "trunk": 2, "primary": 3, "secondary": 4}
PADUS = "https://services.arcgis.com/v01gqwM5QqNysAAi/ArcGIS/rest/services/Fee_Managers_PADUS/FeatureServer/0/query"
COLUMNS = {
    "sub_km": "FLOAT", "sub_kv": "FLOAT", "sub_name": "KEYWORD", "line_km": "FLOAT", "line_kv": "FLOAT",
    "road_km": "FLOAT", "road_name": "KEYWORD", "solar": "FLOAT", "solar_km": "FLOAT", "solar_name": "KEYWORD",
    "protected": "FLOAT", "protected_strict": "FLOAT", "protected_name": "KEYWORD",
}


def log(*a):
    print(*a, file=sys.stderr, flush=True)


def server():
    base = os.environ.get("CHLOEDB_URL", "http://127.0.0.1:7700").rstrip("/")
    key = os.environ.get("CHLOEDB_KEY", "")

    def post(path, body):
        req = urllib.request.Request(base + path, data=json.dumps(body).encode(), headers={"content-type": "application/json", "authorization": f"Bearer {key}"})
        with urllib.request.urlopen(req, timeout=300) as r:
            return json.load(r)

    return post


def places(a):
    post = server()
    j = post("/v1/query", {"sql": "SELECT id, region, lat, lon FROM ea_places", "objects": True, "page_size": 10000, "keep_alive_ms": 60000})
    out = []
    while True:
        cols = j.get("columns") or []
        rows = j.get("rows") or []
        out += [r if isinstance(r, dict) else dict(zip(cols, r)) for r in rows]
        if not rows or not j.get("cursor"):
            break
        j = post("/v1/query", {"cursor": j["cursor"]})
    os.makedirs(os.path.join(a.out, "land"), exist_ok=True)
    with open(os.path.join(a.out, "land", "places.json"), "w") as f:
        json.dump(out, f)
    log(len(out), "places")


def kv(tags):
    """The highest voltage of a line or substation, in kV (0 when not given)."""
    best = 0.0
    for part in (tags.get("voltage") or "").replace(",", ";").split(";"):
        try:
            v = float(part.strip())
        except ValueError:
            continue
        best = max(best, v / 1000.0 if v > 2000 else v)
    return round(best, 1)


def solar_tags(t):
    return (t.get("power") == "plant" and t.get("plant:source") == "solar") or (t.get("power") == "generator" and t.get("generator:source") == "solar" and t.get("generator:method") != "thermal")


def protected_tags(t):
    return t.get("boundary") in ("protected_area", "national_park") or t.get("leisure") == "nature_reserve"


def wanted(o, chile):
    t = o.tags
    if t.get("power") in ("line", "substation") or solar_tags(t):
        return True
    if t.get("highway") in ROADS and o.is_way():
        return True
    return chile and protected_tags(t)


def osm(a):
    import osmium
    from shapely import wkb

    small = os.path.join(a.out, "land", f"{a.region}_small.osm.pbf")
    chile = a.region == "chile"
    t0 = time.time()
    if not os.path.exists(small):
        with osmium.BackReferenceWriter(small, ref_src=a.pbf, overwrite=True) as w:
            for o in osmium.FileProcessor(a.pbf).with_filter(osmium.filter.KeyFilter("power", "highway", "boundary", "leisure")):
                if wanted(o, chile):
                    w.add(o)
        log(f"cut to {os.path.getsize(small) >> 20} MB in {time.time() - t0:.0f} s")
    fab = osmium.geom.WKBFactory()
    out = {"lines": [], "subs": [], "roads": [], "solar": [], "protected": []}
    seen_areas = set()
    for o in osmium.FileProcessor(small).with_locations().with_areas():
        t = o.tags
        try:
            if o.is_node():
                if t.get("power") == "substation":
                    out["subs"].append((o.location.lon, o.location.lat, kv(t), t.get("name", "")))
            elif o.is_way():
                if t.get("power") == "line" and len(o.nodes) > 1:
                    out["lines"].append((wkb.loads(fab.create_linestring(o), hex=True), kv(t)))
                elif t.get("highway") in ROADS and len(o.nodes) > 1:
                    out["roads"].append((wkb.loads(fab.create_linestring(o), hex=True), ROADS[t["highway"]], t.get("ref") or t.get("name", "")))
            elif o.is_area():
                key = (o.from_way(), o.orig_id())
                if key in seen_areas:
                    continue
                seen_areas.add(key)
                g = wkb.loads(fab.create_multipolygon(o), hex=True)
                if t.get("power") == "substation":
                    c = g.centroid
                    out["subs"].append((c.x, c.y, kv(t), t.get("name", "")))
                elif solar_tags(t):
                    out["solar"].append((g, t.get("name", "")))
                elif chile and protected_tags(t):
                    pc = (t.get("protect_class") or "").strip()
                    strict = t.get("boundary") == "national_park" or t.get("leisure") == "nature_reserve" or pc in ("1", "1a", "1b", "2", "3", "4")
                    out["protected"].append((g, strict, t.get("name", "")))
        except (RuntimeError, ValueError, osmium.InvalidLocationError):
            continue
    with open(os.path.join(a.out, "land", f"{a.region}_osm.pkl"), "wb") as f:
        pickle.dump(out, f)
    log(a.region, {k: len(v) for k, v in out.items()}, f"in {time.time() - t0:.0f} s")


def padus(a):
    from shapely.geometry import shape

    feats, offset = [], 0
    while True:
        q = {"where": "State_Nm='CA'", "outFields": "Unit_Nm,GAP_Sts,Des_Tp,Mang_Name", "outSR": "4326", "f": "geojson",
             "resultOffset": str(offset), "resultRecordCount": "2000", "maxAllowableOffset": "0.0002", "geometryPrecision": "5", "orderByFields": "OBJECTID"}
        for attempt in range(4):
            try:
                with urllib.request.urlopen(PADUS + "?" + urllib.parse.urlencode(q), timeout=300) as r:
                    j = json.load(r)
                break
            except Exception as e:  # a dropped page is asked again
                log("  again:", e)
                time.sleep(5 * (attempt + 1))
        else:
            raise SystemExit("PAD-US did not answer")
        got = j.get("features") or []
        for f in got:
            if f.get("geometry"):
                shape(f["geometry"])  # checks it reads
                feats.append(f)
        log(f"PAD-US: {len(feats)} areas")
        if len(got) < 2000:
            break
        offset += 2000
    with open(os.path.join(a.out, "land", "california_padus.json"), "w") as f:
        json.dump({"type": "FeatureCollection", "features": feats}, f)


def rows(a):
    import shapely
    from shapely import STRtree
    from shapely.geometry import MultiPolygon, Point, Polygon, box, shape
    lon0 = LON0[a.region]

    def proj(x, y):
        x, y = np.asarray(x, dtype=float), np.asarray(y, dtype=float)
        return (x - lon0) * np.cos(np.radians(y)) * 111.32, y * 110.574

    def area(g):
        """A valid geometry's polygons only (make_valid can leave lines beside them)."""
        try:
            g = shapely.make_valid(g)
        except shapely.errors.GEOSException:
            try:
                g = shapely.make_valid(g, method="structure", keep_collapsed=False)
            except shapely.errors.GEOSException:
                g = g.buffer(0)
        if g.geom_type in ("Polygon", "MultiPolygon"):
            return g
        polys = []
        for q in shapely.get_parts(g):
            if q.geom_type == "Polygon":
                polys.append(q)
            elif q.geom_type == "MultiPolygon":
                polys.extend(shapely.get_parts(q))
        return MultiPolygon(polys) if polys else Polygon()

    def transform(g):
        """A geometry in kilometres, sinusoidal about the region's meridian."""
        return shapely.transform(g, proj, interleaved=False)

    with open(os.path.join(a.out, "land", "places.json")) as f:
        ps = [p for p in json.load(f) if p["region"] == a.region]
    with open(os.path.join(a.out, "land", f"{a.region}_osm.pkl"), "rb") as f:
        o = pickle.load(f)
    pts = [Point(*proj(p["lon"], p["lat"])) for p in ps]
    squares = [transform(box(p["lon"] - HALF / math.cos(math.radians(p["lat"])), p["lat"] - HALF, p["lon"] + HALF / math.cos(math.radians(p["lat"])), p["lat"] + HALF)) for p in ps]
    out = [dict() for _ in ps]

    def nearest(geoms, name, each):
        if not geoms:
            return
        tree = STRtree(geoms)
        idx, dist = tree.query_nearest(pts, return_distance=True, all_matches=False)
        for (i, g), d in zip(idx.T, dist):
            each(out[i], g, d)
        log(f"{name}: {len(geoms)}")

    lines = [(transform(g), k) for g, k in o["lines"]]
    nearest([g for g, _ in lines], "power lines", lambda r, g, d: r.update(line_km=round(float(d), 2), line_kv=lines[g][1] or None))
    subs = o["subs"]
    nearest([Point(*proj(x, y)) for x, y, _, _ in subs], "substations", lambda r, g, d: r.update(sub_km=round(float(d), 2), sub_kv=subs[g][2] or None, sub_name=subs[g][3]))
    roads = [(transform(g), c, n) for g, c, n in o["roads"]]
    nearest([g for g, _, _ in roads], "roads", lambda r, g, d: r.update(road_km=round(float(d), 2), road_name=roads[g][2]))
    # A farm, not a roof: a hectare or more (0.01 km²).
    solar = [(g, n) for g, n in ((area(transform(g)), n) for g, n in o["solar"]) if g.area >= SOLAR_KM2]
    nearest([g for g, _ in solar], "solar farms", lambda r, g, d: r.update(solar_km=round(float(d), 2), solar_name=solar[g][1]))

    def cover(polys, key, strict_of, name_of):
        if not polys:
            return
        tree = STRtree(polys)
        a_idx, p_idx = tree.query(squares, predicate="intersects")
        acc = {}
        for i, j in zip(a_idx, p_idx):
            share = squares[i].intersection(polys[j]).area / squares[i].area
            s = acc.setdefault(i, [0.0, 0.0, 0.0, ""])
            s[0] += share
            if strict_of(j):
                s[1] += share
            if share > s[2]:
                s[2], s[3] = share, name_of(j)
        for i, (sh, st, _, nm) in acc.items():
            out[i][key] = round(min(1.0, sh), 3)
            if key == "protected":
                out[i]["protected_strict"] = round(min(1.0, st), 3)
                out[i]["protected_name"] = nm

    cover([g for g, _ in solar], "solar", lambda j: False, lambda j: solar[j][1])
    if a.region == "california":
        with open(os.path.join(a.out, "land", "california_padus.json")) as f:
            pad = json.load(f)["features"]
        prot = [(area(transform(shape(f["geometry"]))), str(f["properties"].get("GAP_Sts")) in ("1", "2"), f["properties"].get("Unit_Nm") or "") for f in pad]
    else:
        prot = [(area(transform(g)), s, n) for g, s, n in o["protected"]]
    cover([g for g, _, _ in prot], "protected", lambda j: prot[j][1], lambda j: prot[j][2])

    ops = []
    for p, r in zip(ps, out):
        r.setdefault("solar", 0.0)
        r.setdefault("protected", 0.0)
        r.setdefault("protected_strict", 0.0)
        for k in COLUMNS:
            ops.append({"op": "set", "table": "ea_places", "key": p["id"], "field": k, "value": r.get(k)})
    path = os.path.join(a.out, "land", f"patch_land_{a.region}.jsonl")
    with open(path, "w", encoding="utf-8") as f:
        for k in range(0, len(ops), 2000):
            f.write(json.dumps({"ops": ops[k:k + 2000]}, ensure_ascii=False, separators=(",", ":")) + "\n")
    med = lambda k: float(np.median([r[k] for r in out if r.get(k) is not None])) if any(r.get(k) is not None for r in out) else None
    log(f"{path}: {len(ops)} ops; median km to a substation {med('sub_km')}, a line {med('line_km')}, a road {med('road_km')}; "
        f"{sum(1 for r in out if r['solar'] > 0)} places with a solar farm, {sum(1 for r in out if r['protected'] > 0.5)} mostly protected")


def write(a):
    post = server()
    path = os.path.join(a.out, "land", f"patch_land_{a.region}.jsonl")
    n, t0 = 0, time.time()
    with open(path, encoding="utf-8") as f:
        for line in f:
            body = json.loads(line)
            for attempt in range(4):
                try:
                    n += post("/v1/write", body).get("written", 0)
                    break
                except Exception as e:  # a dropped connection: again, a little later
                    if attempt == 3:
                        raise
                    log("again:", e)
                    time.sleep(3 * (attempt + 1))
    log(f"{path}: {n} written in {time.time() - t0:.0f} s")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("step", choices=["places", "osm", "padus", "rows", "write"])
    p.add_argument("--region", choices=["california", "chile"])
    p.add_argument("--pbf")
    p.add_argument("--out", default="data")
    a = p.parse_args()
    {"places": places, "osm": osm, "padus": padus, "rows": rows, "write": write}[a.step](a)


if __name__ == "__main__":
    main()
