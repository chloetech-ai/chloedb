#!/usr/bin/env python3
"""Each Earth explorer place's climate now and around 2085, from CHELSA 2.1
(the table ea_climate).

CHELSA 2.1 (Karger et al., CC BY 4.0) gives the climate of every 30
arc-second cell (about 1 km) for 1981-2010 and, from five of CMIP6's
models (GFDL-ESM4, IPSL-CM6A-LR, MPI-ESM1-2-HR, MRI-ESM2-0, UKESM1-0-LL)
downscaled, for 2071-2100 under SSP3-7.0, a path where emissions keep
rising. Of its variables this takes six, the mean of the cells inside each
place's square:

  bio1   the year's mean temperature (°C)
  bio5   the hottest month's mean daily high (°C)
  bio6   the coldest month's mean daily low (°C)
  bio12  the year's rain and snow (mm)
  bio15  how unevenly it falls over the year (the months' coefficient of
         variation, %)
  bio18  the rain of the warmest three months (mm)
  kg0    the Köppen-Geiger class

and for 2071-2100 the models' mean, with their lowest and highest for the
hottest month and the rain, and the class most of them give.

A place's climate is a vector of the six, each standardised over every
place of both regions today (the rain as its logarithm), once for now
(clim) and once for 2071-2100 (clim70). The place whose climate today is
nearest a place's 2071-2100 one is where its future can be visited now:
`ORDER BY distance(clim, :clim70) ASC`, which the app asks for a place;
this step keeps the nearest for every place too (analog, analog_km,
analog_d), for the map. A distance of more than NOVEL standard
deviations means no place of California or Chile has that climate today.

Steps:

  places   the places' ids and centres, read from the server
           (CHLOEDB_URL, CHLOEDB_KEY) into <out>/climate/places.json
  fetch    each CHELSA file, downloaded, cut to the two regions' boxes
           (<out>/climate/<var>_<period>.npz) and deleted: 42 files of
           12 to 490 MB
  rows     ea_climate_<region>.jsonl, for chloedb-earth load

  python3 -I tools/climate.py places --out DIR
  python3 -I tools/climate.py fetch --out DIR
  python3 -I tools/climate.py rows --out DIR

Needs numpy, rasterio and scipy.
"""

import argparse
import json
import math
import os
import sys
import time
import urllib.request

import numpy as np

BASE = "https://os.zhdk.cloud.switch.ch/chelsav2/GLOBAL/climatologies"
MODELS = ["GFDL-ESM4", "IPSL-CM6A-LR", "MPI-ESM1-2-HR", "MRI-ESM2-0", "UKESM1-0-LL"]
VARS = ["bio1", "bio5", "bio6", "bio12", "bio15", "bio18"]
# Rain is compared as its logarithm: 100 mm against 200 is as far as 400 against 800.
LOG = {"bio12", "bio18"}
NAMES = {"bio1": "t_mean", "bio5": "t_hot", "bio6": "t_cold", "bio12": "rain", "bio15": "rain_cv", "bio18": "rain_summer"}
NOVEL = 1.0
# CHELSA's kg0 codes (Köppen-Geiger), checked by `rows` against the
# places' own class from Beck et al.
KG0 = ["", "Af", "Am", "As", "Aw", "BWk", "BWh", "BSk", "BSh", "Cfa", "Cfb", "Cfc", "Csa", "Csb", "Csc", "Cwa", "Cwb", "Cwc",
       "Dfa", "Dfb", "Dfc", "Dfd", "Dsa", "Dsb", "Dsc", "Dsd", "Dwa", "Dwb", "Dwc", "Dwd", "ET", "EF"]
HALF = 3.84 / 2 / 111.32
PERIODS = ["now"] + MODELS


def url(var, period):
    if period == "now":
        return f"{BASE}/1981-2010/bio/CHELSA_{var}_1981-2010_V.2.1.tif"
    return f"{BASE}/2071-2100/{period}/ssp370/bio/CHELSA_{var}_2071-2100_{period.lower()}_ssp370_V.2.1.tif"


def places(a):
    """The places' ids and centres, read a page at a time."""
    base = os.environ.get("CHLOEDB_URL", "http://127.0.0.1:7700").rstrip("/")
    key = os.environ.get("CHLOEDB_KEY", "")

    def post(body):
        req = urllib.request.Request(base + "/v1/query", data=json.dumps(body).encode(), headers={"content-type": "application/json", "authorization": f"Bearer {key}"})
        with urllib.request.urlopen(req, timeout=120) as r:
            return json.load(r)

    j = post({"sql": "SELECT id, region, lat, lon, town, climate FROM ea_places", "objects": True, "page_size": 10000, "keep_alive_ms": 60000})
    out = []
    while True:
        cols = j.get("columns") or []
        rows = j.get("rows") or []
        for r in rows:
            out.append(r if isinstance(r, dict) else dict(zip(cols, r)))
        if not rows or not j.get("cursor"):
            break
        j = post({"cursor": j["cursor"]})
    d = os.path.join(a.out, "climate")
    os.makedirs(d, exist_ok=True)
    with open(os.path.join(d, "places.json"), "w") as f:
        json.dump(out, f)
    print(f"{len(out)} places", file=sys.stderr)


def read_places(a):
    with open(os.path.join(a.out, "climate", "places.json")) as f:
        return json.load(f)


def boxes(ps):
    """Each region's box, a place's half side and a cell beyond."""
    out = {}
    for r in sorted({p["region"] for p in ps}):
        la = [p["lat"] for p in ps if p["region"] == r]
        lo = [p["lon"] for p in ps if p["region"] == r]
        pad = HALF + 0.02
        out[r] = (min(lo) - pad, min(la) - pad, max(lo) + pad, max(la) + pad)
    return out


def fetch(a):
    import rasterio
    from rasterio.windows import from_bounds

    ps = read_places(a)
    bx = boxes(ps)
    d = os.path.join(a.out, "climate")
    jobs = [(v, p) for v in VARS + ["kg0"] for p in PERIODS]
    for i, (var, period) in enumerate(jobs):
        dst = os.path.join(d, f"{var}_{period}.npz")
        if os.path.exists(dst):
            continue
        tmp = os.path.join(d, f"{var}_{period}.tif")
        t0 = time.time()
        for attempt in range(4):
            try:
                urllib.request.urlretrieve(url(var, period), tmp)
                break
            except Exception as e:  # a dropped download is tried again
                print(f"  {var} {period}: {e}, again", file=sys.stderr)
                time.sleep(5 * (attempt + 1))
        else:
            raise SystemExit(f"could not download {url(var, period)}")
        arrays = {}
        with rasterio.open(tmp) as src:
            scale, offset = src.scales[0], src.offsets[0]
            for r, (w, s, e, n) in bx.items():
                win = from_bounds(w, s, e, n, src.transform).round_offsets().round_lengths()
                x = src.read(1, window=win).astype(np.float32)
                bad = x == src.nodata if src.nodata is not None else np.zeros(x.shape, bool)
                if var != "kg0":
                    x = x * scale + offset
                x[bad] = np.nan
                t = src.window_transform(win)
                arrays[r] = x
                arrays[r + "_t"] = np.array([t.c, t.a, t.f, t.e])
        np.savez_compressed(dst, **arrays)
        os.remove(tmp)
        print(f"[{i + 1}/{len(jobs)}] {var} {period} in {time.time() - t0:.0f} s", file=sys.stderr)


def at_places(arr, t, ps, mode=False):
    """Each place's mean of the cells inside its square (or the commonest
    value, for a class)."""
    x0, dx, y0, dy = t
    out = np.full(len(ps), np.nan, np.float32)
    for i, p in enumerate(ps):
        c0 = int(math.floor((p["lon"] - HALF - x0) / dx))
        c1 = int(math.ceil((p["lon"] + HALF - x0) / dx))
        r0 = int(math.floor((p["lat"] + HALF - y0) / dy))
        r1 = int(math.ceil((p["lat"] - HALF - y0) / dy))
        cut = arr[max(r0, 0):max(r1, 0), max(c0, 0):max(c1, 0)].ravel()
        cut = cut[np.isfinite(cut)]
        if cut.size == 0:
            continue
        if mode:
            v, n = np.unique(cut.astype(int), return_counts=True)
            out[i] = v[np.argmax(n)]
        else:
            out[i] = float(cut.mean())
    return out


def rows(a):
    from scipy.spatial import cKDTree

    ps = read_places(a)
    d = os.path.join(a.out, "climate")
    by_region = {}
    for r in sorted({p["region"] for p in ps}):
        by_region[r] = [i for i, p in enumerate(ps) if p["region"] == r]
    vals = {}  # (var, period) -> per place, all places
    for var in VARS + ["kg0"]:
        for period in PERIODS:
            z = np.load(os.path.join(d, f"{var}_{period}.npz"))
            v = np.full(len(ps), np.nan, np.float32)
            for r, idx in by_region.items():
                v[idx] = at_places(z[r], z[r + "_t"], [ps[i] for i in idx], mode=var == "kg0")
            vals[(var, period)] = v
            print(f"{var} {period}: {np.isfinite(v).sum()} places", file=sys.stderr)

    # The class codes, checked against Beck et al.'s classes of the places.
    kg = vals[("kg0", "now")]
    agree = sum(1 for i, p in enumerate(ps) if np.isfinite(kg[i]) and 0 < kg[i] < len(KG0) and KG0[int(kg[i])] == p.get("climate"))
    print(f"Köppen-Geiger: CHELSA's class for 1981-2010 is the place's own (Beck, 1991-2020) for {agree} of {len(ps)}", file=sys.stderr)
    if agree < 0.5 * len(ps):
        raise SystemExit("the kg0 codes do not match: check KG0")

    def feat(var, period):
        v = vals[(var, period)].astype(np.float64)
        return np.log1p(np.maximum(v, 0)) if var in LOG else v

    now = np.stack([feat(v, "now") for v in VARS], 1)
    fut = np.stack([np.mean([feat(v, m) for m in MODELS], 0) for v in VARS], 1)
    ok = np.isfinite(now).all(1) & np.isfinite(fut).all(1)
    mu, sd = now[ok].mean(0), now[ok].std(0)
    zn, zf = (now - mu) / sd, (fut - mu) / sd
    print("scales: " + ", ".join(f"{v} {s:.3g}" for v, s in zip(VARS, sd)), file=sys.stderr)

    # The nearest climate today to each place's 2071-2100 one, anywhere.
    good = np.where(ok)[0]
    tree = cKDTree(zn[good])
    dist, j = tree.query(zf[good], k=1)
    analog = np.full(len(ps), -1)
    analog_d = np.full(len(ps), np.nan)
    analog[good] = good[j]
    analog_d[good] = dist
    # And how far today's climate is from the place's own 2071-2100.
    moved = np.linalg.norm(zf - zn, axis=1)

    def km(i, k):
        p, q = ps[i], ps[k]
        la1, lo1, la2, lo2 = map(math.radians, (p["lat"], p["lon"], q["lat"], q["lon"]))
        h = math.sin((la2 - la1) / 2) ** 2 + math.cos(la1) * math.cos(la2) * math.sin((lo2 - lo1) / 2) ** 2
        return 6371.0 * 2 * math.asin(min(1.0, math.sqrt(h)))

    def r1(x):
        return None if not np.isfinite(x) else round(float(x), 1)

    def r0(x):
        return None if not np.isfinite(x) else int(round(float(x)))

    def cls(x):
        return KG0[int(x)] if np.isfinite(x) and 0 < int(x) < len(KG0) else ""

    for r, idx in by_region.items():
        path = os.path.join(a.out, f"ea_climate_{r}.jsonl")
        n = 0
        with open(path, "w") as f:
            for i in idx:
                if not ok[i]:
                    continue
                p = ps[i]
                row = {"id": p["id"], "region": r, "lat": p["lat"], "lon": p["lon"], "town": p.get("town", "")}
                for v in VARS:
                    name = NAMES[v]
                    rnd = r0 if v in ("bio12", "bio18", "bio15") else r1
                    row[name] = rnd(vals[(v, "now")][i])
                    ms = np.array([vals[(v, m)][i] for m in MODELS], np.float64)
                    row[name + "_70"] = rnd(ms.mean())
                    if v in ("bio5", "bio6", "bio12"):
                        row[name + "_70_lo"] = rnd(ms.min())
                        row[name + "_70_hi"] = rnd(ms.max())
                ks = [cls(vals[("kg0", m)][i]) for m in MODELS]
                row["kg"] = cls(vals[("kg0", "now")][i])
                row["kg_70"] = max(set(ks), key=ks.count) if any(ks) else ""
                row["kg_70_agree"] = ks.count(row["kg_70"])
                k = int(analog[i])
                row["analog"] = ps[k]["id"]
                row["analog_region"] = ps[k]["region"]
                row["analog_town"] = ps[k].get("town", "")
                row["analog_km"] = round(km(i, k), 1)
                row["analog_d"] = round(float(analog_d[i]), 2)
                row["novel"] = bool(analog_d[i] > NOVEL)
                row["moved"] = round(float(moved[i]), 2)
                row["clim"] = [round(float(x), 4) for x in zn[i]]
                row["clim70"] = [round(float(x), 4) for x in zf[i]]
                f.write(json.dumps(row, ensure_ascii=False) + "\n")
                n += 1
        print(f"{path}: {n} rows", file=sys.stderr)
    with open(os.path.join(d, "scales.json"), "w") as f:
        json.dump({"vars": VARS, "log": sorted(LOG), "mean": mu.tolist(), "sd": sd.tolist()}, f)


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("step", choices=["places", "fetch", "rows"])
    p.add_argument("--out", default="data")
    a = p.parse_args()
    {"places": places, "fetch": fetch, "rows": rows}[a.step](a)


if __name__ == "__main__":
    main()
