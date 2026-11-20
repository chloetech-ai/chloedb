#!/usr/bin/env python3
"""Turn Major TOM's pictures of a region, and the land's layers, into
Earth explorer's places (the table ea_places).

A place is one of Major TOM's fragments of Sentinel-2: 384 by 384 pixels of
10 metres, 3.84 km a side, with its SigLIP vector (so400m, 1,152 numbers),
which is how the app searches by words and by picture. To each place this
adds what lies on it:

  land cover    ESA WorldCover 2021 (10 m, read at 80 m): the share of trees,
                shrubs, grass, crops, built, bare, snow, water and wetland
  terrain       Copernicus DEM GLO-90: height (median, lowest, highest) and
                slope (median, and the share steeper than 20 degrees)
  people        GHSL 2025 (30 arc-seconds): people living on it
  climate       Beck et al.'s Koppen-Geiger map for 1991-2020 (1 km)
  mines         Maus et al.'s mining polygons v2: the share that is mined
  crops         USDA's Cropland Data Layer 2024 (California only): the share
                farmed and the main crop
  fire          NASA FIRMS: the years with fire detected on it (MODIS from
                2001, VIIRS from 2012), presumed vegetation fires only
  names         geoBoundaries (state or region, county or comuna) and the
                nearest town of GeoNames' cities1000

Steps, each its own command, so a step can be run again alone:

  pull     Major TOM's rows inside the region's box, from Hugging Face
           (about 1 GB a part read; only the rows kept are written)
  layers   the layers above but fire, into <out>/<region>_layers.npz
  fires    the fire history (FIRMS), into the same file
  twins    each place's climate twin in the other region (both regions)
  rows     ea_places.jsonl: a row a place, with its look and its words
  patch    after fires ran again, the places whose fire years changed, as
           set ops for POST /v1/write (patch_fires_<region>.jsonl)

  python3 -I tools/places.py pull   --region chile --out DIR
  python3 -I tools/places.py layers --region chile --out DIR --data DATA
  python3 -I tools/places.py fires  --region chile --out DIR --data DATA
  python3 -I tools/places.py rows   --region chile --out DIR

DATA holds the downloaded inputs (see --help of layers for the names).
Needs numpy, pyarrow, fsspec, aiohttp, rasterio, shapely and scipy.
"""

import argparse
import glob
import json
import math
import os
import sqlite3
import sys
import time

import numpy as np

REGIONS = {
    # box (south, north, west, east), its parts of Major TOM's SigLIP set,
    # the country, and how geoBoundaries names it.
    "california": dict(box=(32.4, 42.1, -124.6, -114.0), parts=[28, 29, 30, 31, 32], country="US",
                       adm1="USA-ADM1", adm2="USA-ADM2", shape="California", cities="US", cities_adm1="CA"),
    "chile": dict(box=(-56.2, -17.4, -76.0, -66.3), parts=[2, 3, 4, 5, 6, 7, 8], country="CL",
                  adm1="CHL-ADM1", adm2="CHL-ADM3", shape=None, cities="CL", cities_adm1=None),
}
HALF = 3.84 / 2 / 111.32  # half a place, in degrees of latitude
COVER = {10: "trees", 20: "shrubs", 30: "grass", 40: "crops", 50: "built", 60: "bare", 70: "snow", 80: "water", 90: "wetland", 95: "mangrove", 100: "moss"}
KOPPEN = ["", "Af", "Am", "Aw", "BWh", "BWk", "BSh", "BSk", "Csa", "Csb", "Csc", "Cwa", "Cwb", "Cwc", "Cfa", "Cfb", "Cfc",
          "Dsa", "Dsb", "Dsc", "Dsd", "Dwa", "Dwb", "Dwc", "Dwd", "Dfa", "Dfb", "Dfc", "Dfd", "ET", "EF"]
CLIMATE_GROUP = {"A": "tropical", "B": "dry", "C": "temperate", "D": "cold", "E": "polar"}
CLIMATE_WORDS = {"Csa": "Mediterranean, hot summer", "Csb": "Mediterranean, warm summer", "Csc": "Mediterranean, cool summer",
                 "BWh": "hot desert", "BWk": "cold desert", "BSh": "hot steppe", "BSk": "cold steppe",
                 "Cfb": "oceanic", "Cfc": "subpolar oceanic", "Cfa": "humid subtropical", "ET": "tundra", "EF": "ice cap",
                 "Dsb": "cold, dry summer", "Dsc": "cold, dry summer", "Dfb": "cold, wet", "Dfc": "subarctic"}


def log(*a):
    print("places:", *a, file=sys.stderr, flush=True)


def fix_text(s):
    """geoBoundaries' Chilean names are UTF-8 read as Latin-1."""
    try:
        return s.encode("latin-1").decode("utf-8")
    except (UnicodeEncodeError, UnicodeDecodeError):
        return s


# --- pull -------------------------------------------------------------------

def pull(a):
    import fsspec
    import pyarrow.compute as pc
    import pyarrow.parquet as pq
    r = REGIONS[a.region]
    s, n, w, e = r["box"]
    fs = fsspec.filesystem("https")
    os.makedirs(a.out, exist_ok=True)
    cols = ["unique_id", "embedding", "timestamp", "product_id", "grid_cell", "pixel_bbox", "centre_lat", "centre_lon"]
    for k in r["parts"]:
        path = f"{a.out}/mt_{a.region}_{k:02d}.parquet"
        if os.path.exists(path):
            continue
        name = f"part_{k * 100 + 1:05d}-{k * 100 + 100:05d}.parquet"
        url = f"https://huggingface.co/datasets/Major-TOM/Core-S2RGB-SigLIP/resolve/main/embeddings/{name}"
        t0 = time.time()
        with fs.open(url, "rb", block_size=2 ** 24) as f:
            t = pq.ParquetFile(f).read(columns=cols)
        lat, lon = t["centre_lat"], t["centre_lon"]
        keep = pc.and_(pc.and_(pc.greater_equal(lat, s), pc.less_equal(lat, n)), pc.and_(pc.greater_equal(lon, w), pc.less_equal(lon, e)))
        t = t.filter(keep)
        pq.write_table(t, path + ".part")
        os.replace(path + ".part", path)
        log(name, t.num_rows, "rows in the box", round(time.time() - t0), "s")


def read_region(a):
    """The region's places: inside its outline, one a fragment (the first
    picture of a cell when Major TOM has several)."""
    import pyarrow as pa
    import pyarrow.parquet as pq
    import shapely
    r = REGIONS[a.region]
    files = sorted(glob.glob(f"{a.out}/mt_{a.region}_*.parquet"))
    if not files:
        sys.exit(f"places: no {a.out}/mt_{a.region}_*.parquet: run pull first")
    t = pa.concat_tables([pq.read_table(f) for f in files])
    lat = t["centre_lat"].to_numpy().astype(float)
    lon = t["centre_lon"].to_numpy().astype(float)
    outline = region_outline(a, r)
    inside = shapely.contains_xy(outline, lon, lat)
    cell = t["grid_cell"].to_pylist()
    bbox = t["pixel_bbox"].to_pylist()
    ts = t["timestamp"].to_pylist()
    seen, keep, ids = {}, [], []
    for i in np.nonzero(inside)[0]:
        b = bbox[i]
        frag = (b[0] // 342) * 3 + (b[1] // 342)
        pid = f"{cell[i]}-{frag}"
        if pid in seen and ts[seen[pid]] <= ts[i]:
            continue
        if pid in seen:
            keep.remove(seen[pid])
            ids.remove(pid)
        seen[pid] = i
        keep.append(i)
        ids.append(pid)
    order = np.argsort(np.array(ids))
    keep = np.array(keep)[order]
    ids = [ids[j] for j in order]
    log(a.region, len(lat), "in the box,", int(inside.sum()), "inside,", len(keep), "places")
    return t.take(keep), ids


def region_outline(a, r):
    import shapely
    g = json.load(open(os.path.join(a.data, "geo", r["adm1"] + ".geojson"), encoding="utf-8"))
    shapes = []
    for f in g["features"]:
        name = fix_text(f["properties"].get("shapeName", ""))
        if r["shape"] and name != r["shape"]:
            continue
        geom = shapely.geometry.shape(f["geometry"])
        # Mainland Chile only: Easter Island and Juan Fernandez are far west.
        if a.region == "chile":
            geom = geom.intersection(shapely.box(-76.0, -56.2, -66.3, -17.4))
        shapes.append(geom)
    outline = shapely.union_all(shapes)
    shapely.prepare(outline)
    return outline


# --- layers -----------------------------------------------------------------

def layers(a):
    import rasterio
    from rasterio.enums import Resampling
    from rasterio.windows import from_bounds
    os.environ.setdefault("GDAL_DISABLE_READDIR_ON_OPEN", "EMPTY_DIR")
    os.environ.setdefault("CPL_VSIL_CURL_ALLOWED_EXTENSIONS", ".tif")
    r = REGIONS[a.region]
    t, ids = read_region(a)
    lat = t["centre_lat"].to_numpy().astype(float)
    lon = t["centre_lon"].to_numpy().astype(float)
    N = len(lat)
    half_lon = HALF / np.cos(np.radians(lat))
    out = {"ids": np.array(ids)}

    # Land cover: WorldCover's 3-degree tiles, named by their south-west corner.
    lc = {v: np.full(N, np.nan) for v in COVER.values()}
    for la in range(int(math.floor(lat.min() / 3) * 3), int(lat.max()) + 3, 3):
        for lo in range(int(math.floor(lon.min() / 3) * 3), int(lon.max()) + 3, 3):
            m = np.nonzero((lat >= la) & (lat < la + 3) & (lon >= lo) & (lon < lo + 3))[0]
            if not len(m):
                continue
            ns = f"{'N' if la >= 0 else 'S'}{abs(la):02d}"
            ew = f"{'E' if lo >= 0 else 'W'}{abs(lo):03d}"
            url = f"/vsicurl/https://esa-worldcover.s3.eu-central-1.amazonaws.com/v200/2021/map/ESA_WorldCover_10m_2021_v200_{ns}{ew}_Map.tif"
            try:
                with rasterio.open(url) as src:
                    g = src.read(1, out_shape=(4500, 4500), resampling=Resampling.nearest)
            except Exception as e:  # the sea has no tile
                log("worldcover: no tile", ns + ew, str(e)[:80])
                continue
            px = 4500 / 3.0
            for i in m:
                r0 = int((la + 3 - (lat[i] + HALF)) * px); r1 = int((la + 3 - (lat[i] - HALF)) * px)
                c0 = int((lon[i] - half_lon[i] - lo) * px); c1 = int((lon[i] + half_lon[i] - lo) * px)
                win = g[max(0, r0):max(0, r1), max(0, c0):max(0, c1)]
                if win.size == 0:
                    continue
                for v, name in COVER.items():
                    lc[name][i] = (win == v).mean() * 100
            log("worldcover", ns + ew, len(m))
    out.update({f"lc_{k}": v for k, v in lc.items()})

    # Terrain: Copernicus GLO-90's 1-degree tiles.
    elev = np.full((N, 3), np.nan)
    slope = np.full((N, 2), np.nan)
    for la in range(int(math.floor(lat.min())), int(math.floor(lat.max())) + 1):
        for lo in range(int(math.floor(lon.min())), int(math.floor(lon.max())) + 1):
            m = np.nonzero((lat >= la) & (lat < la + 1) & (lon >= lo) & (lon < lo + 1))[0]
            if not len(m):
                continue
            ns = f"{'N' if la >= 0 else 'S'}{abs(la):02d}"
            ew = f"{'E' if lo >= 0 else 'W'}{abs(lo):03d}"
            name = f"Copernicus_DSM_COG_30_{ns}_00_{ew}_00_DEM"
            try:
                with rasterio.open(f"/vsicurl/https://copernicus-dem-90m.s3.amazonaws.com/{name}/{name}.tif") as src:
                    z = src.read(1).astype(float)
            except Exception:
                log("dem: no tile", name)
                continue
            ny, nx = z.shape
            dy = 111320 / ny
            dx = 111320 * math.cos(math.radians(la + 0.5)) / nx
            gy, gx = np.gradient(z, dy, dx)
            sl = np.degrees(np.arctan(np.hypot(gx, gy)))
            for i in m:
                r0 = int((la + 1 - (lat[i] + HALF)) * ny); r1 = int((la + 1 - (lat[i] - HALF)) * ny)
                c0 = int((lon[i] - half_lon[i] - lo) * nx); c1 = int((lon[i] + half_lon[i] - lo) * nx)
                zz = z[max(0, r0):max(0, r1), max(0, c0):max(0, c1)]
                ss = sl[max(0, r0):max(0, r1), max(0, c0):max(0, c1)]
                if zz.size == 0:
                    continue
                elev[i] = [np.median(zz), zz.min(), zz.max()]
                slope[i] = [np.median(ss), (ss > 20).mean() * 100]
    log("dem done,", int(np.isnan(elev[:, 0]).sum()), "places without")
    out["elev"], out["slope"] = elev, slope

    # People: GHSL 2025 at 30 arc-seconds (about 1 km), counts per pixel.
    s, n, w, e = r["box"]
    with rasterio.open(os.path.join(a.data, "ghsl", "GHS_POP_E2025_GLOBE_R2023A_4326_30ss_V1_0.tif")) as src:
        win = from_bounds(w - 0.1, s - 0.1, e + 0.1, n + 0.1, src.transform)
        g = src.read(1, window=win).astype(float)
        tr = src.window_transform(win)
    g[g < 0] = 0
    people = np.zeros(N)
    for i in range(N):
        c0, r0 = ~tr * (lon[i] - half_lon[i], lat[i] + HALF)
        c1, r1 = ~tr * (lon[i] + half_lon[i], lat[i] - HALF)
        r0, r1, c0, c1 = int(math.floor(r0)), int(math.ceil(r1)), int(math.floor(c0)), int(math.ceil(c1))
        wv = g[max(0, r0):max(r1, r0 + 1), max(0, c0):max(c1, c0 + 1)]
        # Whole pixels around the square, scaled to its area.
        pix_km2 = (tr.a * 111.32 * math.cos(math.radians(lat[i]))) * (-tr.e * 111.32)
        people[i] = wv.sum() / max(wv.size * pix_km2, 1e-9) * 3.84 * 3.84
    out["people"] = people
    log("people", int(people.sum()))

    # Climate: the Koppen-Geiger class at the place's centre (1 km).
    with rasterio.open(os.path.join(a.data, "koppen", "1991_2020", "koppen_geiger_0p00833333.tif")) as src:
        win = from_bounds(w - 0.1, s - 0.1, e + 0.1, n + 0.1, src.transform)
        g = src.read(1, window=win)
        tr = src.window_transform(win)
    klass = np.zeros(N, int)
    for i in range(N):
        c, rr = ~tr * (lon[i], lat[i])
        rr, c = int(rr), int(c)
        wv = g[max(0, rr - 2):rr + 3, max(0, c - 2):c + 3].ravel()
        wv = wv[(wv > 0) & (wv < len(KOPPEN))]
        if wv.size:
            klass[i] = np.bincount(wv).argmax()
    out["koppen"] = klass

    # Mines: Maus et al. v2, the share of the square inside a mine.
    import shapely
    con = sqlite3.connect(os.path.join(a.data, "mines", "mining_polygons_v2.gpkg"))
    country = {"US": "United States", "CL": "Chile"}[r["country"]]
    rows = con.execute("SELECT geom FROM mining_polygons WHERE COUNTRY_NAME = ?", (country,)).fetchall()
    polys = [gpkg_geom(b) for (b,) in rows]
    polys = [p for p in polys if p is not None]
    tree = shapely.STRtree(polys)
    mine = np.zeros(N)
    for i in range(N):
        sq = shapely.box(lon[i] - half_lon[i], lat[i] - HALF, lon[i] + half_lon[i], lat[i] + HALF)
        hit = tree.query(sq, predicate="intersects")
        if len(hit):
            mine[i] = sum(polys[j].intersection(sq).area for j in hit) / sq.area * 100
    out["mine"] = mine
    log("mines", len(polys), "polygons,", int((mine > 1).sum()), "places mined")

    # Crops by kind: USDA CDL (California only), at 120 m.
    out["crop_main"] = np.zeros(N, int)
    out["crop_pct"] = np.zeros(N)
    cdl = os.path.join(a.data, "cdl", "CDL_2024_06.tif")
    if a.region == "california" and os.path.exists(cdl):
        from rasterio.warp import transform as wt
        F = 4
        with rasterio.open(cdl) as src:
            g = src.read(1, out_shape=(src.height // F, src.width // F), resampling=Resampling.nearest)
            tr = src.transform * src.transform.scale(F, F)
        x, y = wt("EPSG:4326", "EPSG:5070", lon.tolist(), lat.tolist())
        nonag = set(range(81, 196)) | {0, 61}
        half = 1920 / 120
        for i in range(N):
            c, rr = ~tr * (x[i], y[i])
            wv = g[max(0, int(rr - half)):max(0, int(rr + half)), max(0, int(c - half)):max(0, int(c + half))]
            if wv.size == 0:
                continue
            v, cnt = np.unique(wv, return_counts=True)
            ag = np.array([k not in nonag for k in v])
            if ag.any():
                j = np.argmax(np.where(ag, cnt, -1))
                out["crop_main"][i] = v[j]
                out["crop_pct"][i] = cnt[j] / wv.size * 100
        log("crops", int((out["crop_pct"] > 10).sum()), "places farmed")

    # Names.
    adm1, adm2 = admin_names(a, r, lat, lon)
    out["adm1"], out["adm2"] = np.array(adm1), np.array(adm2)
    town, town_km, town_pop = nearest_towns(a, r, lat, lon)
    out["town"], out["town_km"], out["town_pop"] = np.array(town), town_km, town_pop
    np.savez_compressed(f"{a.out}/{a.region}_layers.npz", **out)
    log("layers written for", N, "places")


def fires(a):
    """The fire history (NASA FIRMS) into the layers: a step of its own, as
    FIRMS's yearly files come slowly."""
    r = REGIONS[a.region]
    t, ids = read_region(a)
    L = dict(np.load(f"{a.out}/{a.region}_layers.npz", allow_pickle=False))
    if list(L["ids"]) != ids:
        sys.exit("places: the layers are of other places: run layers again")
    lat = t["centre_lat"].to_numpy().astype(float)
    lon = t["centre_lon"].to_numpy().astype(float)
    years, count = fire_years(a, r, lat, lon, HALF / np.cos(np.radians(lat)))
    L["fire_years"], L["fire_px"] = years, count
    np.savez_compressed(f"{a.out}/{a.region}_layers.npz", **L)
    log("fire history written,", int((years > 0).sum()), "places burned")


def fire_words(fy):
    """The fires in a place's words, as describe() writes them."""
    return ("burned " + ", ".join(str(y) for y in fy[-6:]) + (", burn scar." if fy[-1] >= 2016 else ".")) if fy else ""


def patch(a):
    """After `fires` ran again: the places whose fire years changed, as
    `set` ops for a server's `POST /v1/write` (a batch of 1,000 a line),
    without writing their rows (and vectors) again."""
    import re
    L = dict(np.load(f"{a.out}/{a.region}_layers.npz", allow_pickle=False))
    at = {pid: i for i, pid in enumerate(L["ids"])}
    clause = re.compile(r" ?burned [0-9, ]+(, burn scar)?\.")
    ops, changed = [], 0
    with open(f"{a.out}/ea_places_{a.region}.jsonl", encoding="utf-8") as f:
        for line in f:
            r = json.loads(line)
            i = at.get(r["id"])
            if i is None:
                continue
            bits = int(L["fire_years"][i])
            fy = [2001 + b for b in range(30) if bits >> b & 1]
            if fy == r.get("fire_years"):
                continue
            changed += 1
            new = fire_words(fy)
            sets = {"fire_years": fy, "fire_px": int(L["fire_px"][i]), "last_fire": fy[-1] if fy else None}
            for k in ("words", "about"):
                t = clause.sub("", r.get(k) or "").rstrip()
                sets[k] = (t + " " + new).strip() if new else t
            ops += [{"op": "set", "table": "ea_places", "key": r["id"], "field": k, "value": v} for k, v in sets.items()]
    path = f"{a.out}/patch_fires_{a.region}.jsonl"
    with open(path, "w", encoding="utf-8") as f:
        for k in range(0, len(ops), 1000):
            f.write(json.dumps({"ops": ops[k:k + 1000]}, ensure_ascii=False, separators=(",", ":")) + "\n")
    log(changed, "places changed,", len(ops), "ops to", path)


def gpkg_geom(b):
    """A GeoPackage geometry: its header, then WKB."""
    import shapely
    if b is None or len(b) < 8:
        return None
    flags = b[3]
    env = (flags >> 1) & 7
    off = 8 + {0: 0, 1: 32, 2: 48, 3: 48, 4: 64}.get(env, 0)
    try:
        return shapely.from_wkb(bytes(b[off:]))
    except Exception:
        return None


# Detections a year a place needs for the year to count as a fire there:
# one is often a hot roof, a flare or a glint, which made a town look
# burned every year.
MIN_DETECTIONS = 3


def fire_years(a, r, lat, lon, half_lon):
    """For each place: a bit a year from 2001 (MODIS, then VIIRS from 2012),
    set when FIRMS saw at least MIN_DETECTIONS presumed vegetation fires on
    it that year; and how many detections there were in those years."""
    from scipy.spatial import cKDTree
    name = {"US": "United_States", "CL": "Chile"}[r["country"]]
    s, n, w, e = r["box"]
    tree = cKDTree(np.c_[lat, lon * np.cos(np.radians(lat))])
    bits = np.zeros(len(lat), np.int64)
    count = np.zeros(len(lat), np.int64)
    have = []
    for path in sorted(glob.glob(os.path.join(a.data, "firms", f"*_{name}.csv"))):
        base = os.path.basename(path)
        year = int(base.split("_")[1])
        viirs = base.startswith("viirs")
        if viirs and year < 2012:
            continue
        if not viirs and year >= 2012:
            continue
        import csv
        la, lo = [], []
        with open(path, newline="") as f:
            for row in csv.DictReader(f):
                try:
                    y, x = float(row["latitude"]), float(row["longitude"])
                except (KeyError, ValueError):
                    continue
                if not (s <= y <= n and w <= x <= e):
                    continue
                if row.get("type", "0") != "0":
                    continue
                c = row.get("confidence", "")
                if viirs and c not in ("n", "h", "nominal", "high"):
                    continue
                if not viirs and (not c.isdigit() or int(c) < 50):
                    continue
                la.append(y)
                lo.append(x)
        if not la:
            continue
        la, lo = np.array(la), np.array(lo)
        d, j = tree.query(np.c_[la, lo * np.cos(np.radians(la))], distance_upper_bound=HALF * 1.2)
        ok = np.isfinite(d)
        j = j[ok]
        inside = (np.abs(la[ok] - lat[j]) <= HALF) & (np.abs(lo[ok] - lon[j]) <= half_lon[j])
        j = j[inside]
        this = np.zeros(len(lat), np.int64)
        np.add.at(this, j, 1)
        burned = this >= MIN_DETECTIONS
        count[burned] += this[burned]
        bits[burned] |= 1 << (year - 2001)
        have.append(year)
        log("firms", base, len(la), "detections,", int(burned.sum()), "places burned")
    log("fire years read:", sorted(set(have)))
    return bits, count


def admin_names(a, r, lat, lon):
    import shapely
    names = []
    for key in ("adm1", "adm2"):
        g = json.load(open(os.path.join(a.data, "geo", r[key] + ".geojson"), encoding="utf-8"))
        geoms = [shapely.geometry.shape(f["geometry"]) for f in g["features"]]
        label = [fix_text(f["properties"].get("shapeName", "")) for f in g["features"]]
        tree = shapely.STRtree(geoms)
        pts = shapely.points(lon, lat)
        got = [""] * len(lat)
        pi, gi = tree.query(pts, predicate="within")
        for p, q in zip(pi, gi):
            got[p] = label[q]
        # A place on the coast whose centre is in the sea: the nearest one.
        miss = [i for i, x in enumerate(got) if not x]
        if miss:
            near = tree.query_nearest(pts[miss])
            for p, q in zip(near[0], near[1]):
                got[miss[p]] = label[q]
        if key == "adm1" and a.region == "chile":
            got = [x.replace("Región de ", "").replace("Región del ", "").replace("Región ", "") for x in got]
        names.append(got)
    return names


def nearest_towns(a, r, lat, lon):
    from scipy.spatial import cKDTree
    towns = []
    with open(os.path.join(a.data, "cities1000.txt"), encoding="utf-8") as f:
        for line in f:
            p = line.rstrip("\n").split("\t")
            if p[8] != r["cities"] or (r["cities_adm1"] and p[10] != r["cities_adm1"]):
                continue
            if p[6] != "P":
                continue
            towns.append((p[1], float(p[4]), float(p[5]), int(p[14] or 0)))
    tl = np.array([t[1] for t in towns]); tn = np.array([t[2] for t in towns])
    tree = cKDTree(np.c_[tl, tn * np.cos(np.radians(tl))])
    d, j = tree.query(np.c_[lat, lon * np.cos(np.radians(lat))])
    return [towns[k][0] for k in j], d * 111.32, np.array([towns[k][3] for k in j])


# --- rows -------------------------------------------------------------------

CDL_NAMES = {1: "corn", 2: "cotton", 3: "rice", 4: "sorghum", 21: "barley", 23: "spring wheat", 24: "winter wheat", 28: "oats",
             36: "alfalfa", 37: "hay", 42: "dry beans", 43: "potatoes", 47: "misc vegetables", 49: "onions", 54: "tomatoes",
             57: "herbs", 66: "cherries", 67: "peaches", 68: "apples", 69: "grapes", 72: "citrus", 74: "pecans", 75: "almonds",
             76: "walnuts", 77: "pears", 204: "pistachios", 206: "carrots", 208: "garlic", 209: "cantaloupes", 211: "olives",
             212: "oranges", 213: "honeydew", 214: "broccoli", 217: "pomegranates", 218: "nectarines", 220: "plums",
             221: "strawberries", 222: "squash", 223: "apricots", 227: "lettuce", 236: "winter wheat", 242: "blueberries",
             243: "cabbage", 244: "cauliflower", 245: "celery", 246: "radishes", 59: "sod and grass seed", 61: "fallow"}


def kind_of(c, mine):
    """The place's main cover, in a word."""
    if mine >= 15:
        return "mine"
    order = sorted(((v, k) for k, v in c.items() if v == v), reverse=True)
    if not order:
        return "unknown"
    best = order[0][1]
    if c.get("built", 0) >= 30:
        return "city"
    if c.get("crops", 0) >= 35:
        return "farmland"
    return {"trees": "forest", "shrubs": "shrubland", "grass": "grassland", "crops": "farmland", "built": "city", "bare": "bare",
            "snow": "snow and ice", "water": "water", "wetland": "wetland", "mangrove": "wetland", "moss": "tundra"}.get(best, best)


def black_vector(a):
    """SigLIP's vector of a black picture (a fragment with no data), from
    the ChloeDB server's model, kept in <out>/black.npy."""
    path = f"{a.out}/black.npy"
    if not os.path.exists(path):
        import base64
        import io
        import urllib.request
        from PIL import Image
        buf = io.BytesIO()
        Image.new("RGB", (384, 384), (0, 0, 0)).save(buf, "PNG")
        body = json.dumps({"image": base64.b64encode(buf.getvalue()).decode(), "table": "ea_words", "field": "emb"}).encode()
        req = urllib.request.Request(f"{a.strata}/v1/embed", data=body, headers={"content-type": "application/json"})
        v = np.array(json.loads(urllib.request.urlopen(req, timeout=600).read())["vector"], dtype=np.float32)
        np.save(path, v)
    v = np.load(path)
    return v / np.linalg.norm(v)


def blanks(a, v):
    """The places whose picture shows nothing: black (outside the scene) or
    one constant picture many places share. About 5% of them; they keep
    their layers and lose their look."""
    from collections import Counter
    u = v / np.linalg.norm(v, axis=1, keepdims=True).clip(1e-9)
    dark = u @ black_vector(a) >= 0.95
    keys = [r.tobytes() for r in np.round(u[:, :16], 4)]
    n = Counter(keys)
    return dark | np.array([n[k] >= 10 for k in keys])


def vectors(t):
    """The table's SigLIP vectors as one float32 array (as Python lists,
    67,000 of them take 7 GB)."""
    col = t["embedding"].combine_chunks()
    return col.flatten().to_numpy(zero_copy_only=False).astype(np.float32).reshape(len(col), -1)


def rows(a):
    t, ids = read_region(a)
    # Read into memory once: an NpzFile reads an array from its zip at each
    # index.
    L = dict(np.load(f"{a.out}/{a.region}_layers.npz", allow_pickle=False))
    if list(L["ids"]) != ids:
        sys.exit("places: the layers are of other places: run layers again")
    r = REGIONS[a.region]
    emb = vectors(t)
    blank = blanks(a, emb)
    log(a.region, int(blank.sum()), "of", len(ids), "pictures are blank")
    lat = t["centre_lat"].to_numpy().astype(float)
    lon = t["centre_lon"].to_numpy().astype(float)
    ts = t["timestamp"].to_pylist()
    prod = t["product_id"].to_pylist()
    cell = t["grid_cell"].to_pylist()
    tw = f"{a.out}/{a.region}_twins.npz"
    T = dict(np.load(tw, allow_pickle=False)) if os.path.exists(tw) else None
    if T is not None and list(T["ids"]) != ids:
        sys.exit("places: the twins are of other places: run twins again")
    path = f"{a.out}/ea_places_{a.region}.jsonl"
    n = 0
    with open(path + ".part", "w", encoding="utf-8") as f:
        for i, pid in enumerate(ids):
            v = emb[i]
            v = v / (np.linalg.norm(v) or 1.0)
            c = {k: float(L[f"lc_{k}"][i]) for k in COVER.values()}
            mine = float(L["mine"][i])
            k = KOPPEN[int(L["koppen"][i])] if int(L["koppen"][i]) else ""
            bits = int(L["fire_years"][i]) if "fire_years" in L else 0
            fy = [2001 + b for b in range(30) if bits >> b & 1]
            crop = CDL_NAMES.get(int(L["crop_main"][i]), "") if float(L["crop_pct"][i]) >= 5 else ""
            elev = L["elev"][i]
            slope = L["slope"][i]
            people = float(L["people"][i])
            kind = kind_of(c, mine)
            words, about = describe(r, L, i, c, kind, k, fy, crop, mine, people, slope)
            row = {
                "id": pid, "region": a.region, "country": r["country"],
                "adm1": str(L["adm1"][i]), "adm2": str(L["adm2"][i]),
                "town": str(L["town"][i]), "town_km": round(float(L["town_km"][i]), 1), "town_pop": int(L["town_pop"][i]),
                "lat": round(float(lat[i]), 5), "lon": round(float(lon[i]), 5),
                "cell": cell[i], "taken": f"{ts[i][:4]}-{ts[i][4:6]}-{ts[i][6:8]}T{ts[i][9:11]}:{ts[i][11:13]}:{ts[i][13:15]}Z",
                "product": prod[i], "kind": kind,
                **{k2: (None if v2 != v2 else round(v2, 1)) for k2, v2 in c.items()},
                "elev": none_or(elev[0]), "elev_min": none_or(elev[1]), "elev_max": none_or(elev[2]),
                "slope": none_or(slope[0], 1), "steep": none_or(slope[1], 1),
                "people": int(round(people)), "climate": k, "climate_group": CLIMATE_GROUP.get(k[:1], "") if k else "",
                "mine": round(mine, 1), "crop": crop, "crop_pct": round(float(L["crop_pct"][i]), 1),
                "fire_years": fy, "fire_px": int(L["fire_px"][i]) if "fire_px" in L else 0, "last_fire": fy[-1] if fy else None,
                "twin": (str(T["twin"][i]) or None) if T is not None else None, "twin_sim": round(float(T["sim"][i]), 5) if T is not None and str(T["twin"][i]) else None,
                "twin_score": round(float(T["score"][i]), 5) if T is not None and str(T["twin"][i]) and "score" in T else None,
                "words": words, "about": about, "look": None if blank[i] else np.round(v.astype(np.float64), 6).tolist(),
            }
            f.write(json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n")
            n += 1
    os.replace(path + ".part", path)
    log("wrote", n, "places to", path)


def twins(a):
    """Climate twins: for each place of either region, the place of the
    other that looks most like it (the nearest SigLIP vector) among those of
    the same Koppen-Geiger climate, or of any when the other region has
    none of it. Both regions' rows and layers must be there; writes
    <out>/<region>_twins.npz for each, which rows reads."""
    import types
    got = {}
    for name in ("california", "chile"):
        b = types.SimpleNamespace(**{**vars(a), "region": name})
        t, ids = read_region(b)
        L = np.load(f"{a.out}/{name}_layers.npz", allow_pickle=False)
        if list(L["ids"]) != ids:
            sys.exit(f"places: {name}'s layers are of other places: run layers again")
        v = vectors(t)
        v /= np.linalg.norm(v, axis=1, keepdims=True).clip(1e-9)
        k = np.asarray(L["koppen"]).astype(int)
        # A blank picture is no one's twin: its class becomes -1, matched
        # with nothing.
        k[blanks(a, v)] = -1
        got[name] = (ids, v, k)
    # How alike each place is to its 10 nearest in the other region, by
    # picture: a picture of nothing much (snow, haze, deep water) is near
    # everything, and would win every pairing by raw similarity. A twin is
    # chosen, and the twins ranked, by CSLS (Conneau et al. 2018): twice
    # the similarity, less each one's closeness to everything.
    near = {}
    for me, them in (("california", "chile"), ("chile", "california")):
        _, v, k = got[me]
        _, ov, ok = got[them]
        cols = np.nonzero(ok >= 0)[0]
        r = np.zeros(len(v), np.float32)
        for s0 in range(0, len(v), 2048):
            m = v[s0:s0 + 2048] @ ov[cols].T
            r[s0:s0 + 2048] = np.partition(m, -10, axis=1)[:, -10:].mean(axis=1)
        near[me] = r
    for me, them in (("california", "chile"), ("chile", "california")):
        ids, v, k = got[me]
        oids, ov, ok = got[them]
        rme, rthem = near[me], near[them]
        best = np.full(len(ids), -1, np.int64)
        sim = np.zeros(len(ids), np.float32)
        score = np.zeros(len(ids), np.float32)
        classes = set(np.unique(ok))
        for c in np.unique(k):
            if c == -1:
                continue
            rows = np.nonzero(k == c)[0]
            cols = np.nonzero(ok == c)[0] if c in classes and c != 0 else np.nonzero(ok >= 0)[0]
            for s0 in range(0, len(rows), 2048):
                r = rows[s0:s0 + 2048]
                m = v[r] @ ov[cols].T
                j = (2 * m - rthem[cols][None, :]).argmax(axis=1)
                best[r] = cols[j]
                sim[r] = m[np.arange(len(r)), j]
                score[r] = 2 * sim[r] - rme[r] - rthem[cols[j]]
        np.savez_compressed(f"{a.out}/{me}_twins.npz", ids=np.array(ids), twin=np.array([oids[j] if j >= 0 else "" for j in best]), sim=sim, score=score)
        log(me, "twins: median similarity", round(float(np.median(sim)), 4), "best", round(float(sim.max()), 4), "median score", round(float(np.median(score)), 4))


def none_or(x, nd=0):
    x = float(x)
    if x != x:
        return None
    return round(x, nd) if nd else int(round(x))


# Words for what covers a place, so BM25 finds "a lake" or "a glacier".
COVER_WORDS = {"trees": "forest, trees, woods", "shrubs": "shrubland, scrub, chaparral, matorral", "grass": "grassland, meadow, pasture",
               "crops": "farmland, fields, crops", "built": "city, buildings, houses, streets", "bare": "bare ground, desert, sand, rock",
               "snow": "snow, ice, glacier", "water": "water, lake, reservoir, sea", "wetland": "wetland, marsh", "mangrove": "mangroves",
               "moss": "tundra, moss"}


def describe(r, L, i, c, kind, k, fy, crop, mine, people, slope):
    """The place in words, for BM25: where it is (`words`), and what covers
    it, its terrain, climate and fires (`about`, without names, so a search
    for vineyards does not find a town called Vineyard)."""
    where = [str(L["town"][i]), str(L["adm2"][i]), str(L["adm1"][i]), "California" if r["country"] == "US" else "Chile"]
    parts = []
    cover = sorted(((v, k2) for k2, v in c.items() if v == v and v >= 10), reverse=True)
    for v, k2 in cover:
        parts.append(f"{COVER_WORDS.get(k2, k2)} {int(round(v))}%." if v < 60 else f"{COVER_WORDS.get(k2, k2)}, mostly {k2}.")
    parts.append(kind + ".")
    if crop:
        parts.append(f"{crop} orchards." if crop in ("almonds", "walnuts", "pistachios", "cherries", "peaches", "apples", "citrus", "oranges", "olives", "plums", "nectarines", "apricots", "pecans", "pears", "pomegranates") else "vineyards, grapes." if crop == "grapes" else f"{crop} fields.")
    if mine >= 1:
        parts.append("mine, open pit, quarry, tailings.")
    s = float(slope[0]) if slope[0] == slope[0] else 0
    if s >= 20:
        parts.append("steep mountains.")
    elif s >= 10:
        parts.append("hills.")
    elif s < 3:
        parts.append("flat.")
    if people >= 20000:
        parts.append("city, dense.")
    elif people >= 2000:
        parts.append("town.")
    elif people < 20:
        parts.append("empty, nobody lives here.")
    if k:
        parts.append(f"{CLIMATE_WORDS.get(k, k)} climate ({k}).")
    if fy:
        parts.append(fire_words(fy))
    about = " ".join(parts)
    return ", ".join(dict.fromkeys(x for x in where if x)) + ". " + about, about


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("step", choices=["pull", "layers", "fires", "twins", "rows", "patch"])
    p.add_argument("--region", choices=sorted(REGIONS), default="california", help="the region (twins does both)")
    p.add_argument("--out", required=True, help="where Major TOM's rows, the layers and ea_places_<region>.jsonl go")
    p.add_argument("--strata", default=os.environ.get("CHLOEDB_URL", "http://127.0.0.1:7700"), help="a ChloeDB server with the model (for the vector of a blank picture)")
    p.add_argument("--data", default="data", help="the inputs: geo/<geoBoundaries>.geojson, ghsl/, koppen/, mines/, cdl/, firms/, cities1000.txt")
    a = p.parse_args()
    {"pull": pull, "layers": layers, "fires": fires, "twins": twins, "rows": rows, "patch": patch}[a.step](a)


if __name__ == "__main__":
    main()
