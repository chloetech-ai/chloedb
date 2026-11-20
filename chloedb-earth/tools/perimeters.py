"""CAL FIRE's fire perimeters into rows: every fire mapped in California
since 1878, with its name, its dates, its size and what started it.

  fetch   the perimeters from CAL FIRE's service (ArcGIS REST, as GeoJSON),
          into <data>/perimeters/calfire.geojson
  rows    <out>/ea_perimeters.jsonl (a row a fire, its outline simplified
          for the map) and <out>/ea_burns.jsonl (a row a place a fire
          burned, with the share of its square the fire covered), from the
          places' rows (<out>/ea_places_california.jsonl)

Then `chloedb-earth load --only ea_perimeters,ea_burns` puts them in, and
`chloedb-earth names` gives the fires GOES saw the names of the perimeters
they fell in.

The data: CAL FIRE's Fire and Resource Assessment Program (FRAP) with the
US Forest Service Region 5, BLM, the National Park Service and the Fish and
Wildlife Service, "California Fire Perimeters (all)". The State makes no
warranty of its accuracy.
"""

import argparse
import json
import math
import os
import sys
import time
import urllib.parse
import urllib.request

import numpy as np

SERVICE = "https://services1.arcgis.com/jUJYIo9tSA7EHvfZ/arcgis/rest/services/California_Historic_Fire_Perimeters/FeatureServer/0/query"
FIELDS = "OBJECTID,YEAR_,AGENCY,UNIT_ID,FIRE_NAME,INC_NUM,ALARM_DATE,CONT_DATE,CAUSE,GIS_ACRES,COMPLEX_NAME,GlobalID"
HALF = 3.84 / 2 / 111.32  # half a place, in degrees of latitude (as places.py)

# CAL FIRE's cause codes.
CAUSE = {1: "lightning", 2: "equipment", 3: "smoking", 4: "a campfire", 5: "burning debris", 6: "a railroad", 7: "arson",
         8: "children playing with fire", 9: "other causes", 10: "a vehicle", 11: "power lines", 12: "firefighter training",
         13: "non-firefighter training", 14: "unknown", 15: "a structure", 16: "an aircraft", 17: "a volcano",
         18: "an escaped prescribed burn", 19: "an illegal campfire"}


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, file=sys.stderr, flush=True)


def get(url, tries=5):
    for i in range(tries):
        try:
            with urllib.request.urlopen(url, timeout=120) as r:
                return json.load(r)
        except Exception as e:  # the service drops a request now and then
            if i == tries - 1:
                raise
            log("again:", e)
            time.sleep(2 << i)


def fetch(a):
    os.makedirs(os.path.join(a.data, "perimeters"), exist_ok=True)
    out = os.path.join(a.data, "perimeters", "calfire.geojson")
    feats, offset, page = [], 0, 500
    while True:
        q = urllib.parse.urlencode({
            "where": "1=1", "outFields": FIELDS, "outSR": 4326, "f": "geojson", "orderByFields": "OBJECTID",
            "resultOffset": offset, "resultRecordCount": page,
            # About 30 m: the outline drawn on a map and laid over 3.84 km squares.
            "maxAllowableOffset": 0.0003, "geometryPrecision": 5,
        })
        j = get(f"{SERVICE}?{q}")
        got = j.get("features", [])
        feats += got
        log("perimeters", len(feats))
        if len(got) < page and not j.get("exceededTransferLimit") and not j.get("properties", {}).get("exceededTransferLimit"):
            break
        if not got:
            break
        offset += len(got)
    json.dump({"type": "FeatureCollection", "features": feats}, open(out, "w"))
    log("wrote", out, len(feats), "fires")


def title(name):
    """FIRE_NAME is upper case: "PARK", "CAMP", "AUGUST COMPLEX (FKA DOE)"."""
    n = (name or "").strip()
    if not n:
        return ""
    n = n.split("(")[0].strip() or n
    small = {"of", "the", "and", "a", "on", "in", "at"}
    words = [w.lower() if i and w.lower() in small else w[:1].upper() + w[1:].lower() for i, w in enumerate(n.split())]
    return " ".join(words)


def day(ms):
    if ms is None:
        return None
    try:
        return time.strftime("%Y-%m-%dT00:00:00Z", time.gmtime(ms / 1000))
    except (OverflowError, OSError, ValueError):
        return None


def rows(a):
    import shapely
    from shapely.geometry import shape, box, mapping
    from shapely import STRtree
    src = os.path.join(a.data, "perimeters", "calfire.geojson")
    fc = json.load(open(src))
    log("perimeters read:", len(fc["features"]))
    # The places: their squares, as places.py draws them.
    ids, lat, lon = [], [], []
    with open(os.path.join(a.out, "ea_places_california.jsonl")) as f:
        for line in f:
            r = json.loads(line)
            ids.append(r["id"]), lat.append(r["lat"]), lon.append(r["lon"])
    lat, lon = np.array(lat), np.array(lon)
    half_lon = HALF / np.cos(np.radians(lat))
    squares = [box(lon[i] - half_lon[i], lat[i] - HALF, lon[i] + half_lon[i], lat[i] + HALF) for i in range(len(ids))]
    tree = STRtree(squares)
    log("places:", len(ids))
    per = open(os.path.join(a.out, "ea_perimeters.jsonl"), "w")
    burns = open(os.path.join(a.out, "ea_burns.jsonl"), "w")
    seen, n_fires, n_burns = set(), 0, 0
    for ft in fc["features"]:
        p = ft.get("properties") or {}
        g = ft.get("geometry")
        if not g:
            continue
        try:
            geom = shapely.make_valid(shape(g))
        except Exception:
            continue
        if geom.is_empty:
            continue
        year = p.get("YEAR_")
        name = title(p.get("FIRE_NAME"))
        gid = (p.get("GlobalID") or str(p.get("OBJECTID"))).strip("{}").lower()
        pid = f"cf-{year or 0}-{gid[:12]}"
        if pid in seen:
            continue
        seen.add(pid)
        acres = p.get("GIS_ACRES") or 0.0
        cause = CAUSE.get(int(p["CAUSE"])) if p.get("CAUSE") not in (None, "") and str(p.get("CAUSE")).isdigit() else None
        pt = geom.representative_point()
        w, s, e, n = geom.bounds
        # The outline for the map, simpler again for the largest fires.
        tol = 0.0003 if acres < 20000 else 0.001 if acres < 200000 else 0.002
        out = shapely.simplify(geom, tol, preserve_topology=True)
        if out.is_empty:
            out = geom
        per.write(json.dumps({
            "id": pid, "source": "calfire", "name": name, "complex": title(p.get("COMPLEX_NAME")), "year": year,
            "alarm": day(p.get("ALARM_DATE")), "contained": day(p.get("CONT_DATE")), "acres": round(acres, 1),
            "km2": round(acres * 0.00404686, 2), "cause": cause, "agency": p.get("AGENCY") or "", "unit": p.get("UNIT_ID") or "",
            "inc": p.get("INC_NUM") or "", "lat": round(pt.y, 5), "lon": round(pt.x, 5),
            "s": round(s, 5), "n": round(n, 5), "w": round(w, 5), "e": round(e, 5),
            "shape": json.dumps(mapping(out), separators=(",", ":")),
        }) + "\n")
        n_fires += 1
        for j in tree.query(geom, predicate="intersects"):
            sq = squares[j]
            share = geom.intersection(sq).area / sq.area
            if share < 0.005:
                continue
            burns.write(json.dumps({
                "id": f"{ids[j]}:{pid}", "place": ids[j], "perimeter": pid, "region": "california", "name": name, "year": year,
                "alarm": day(p.get("ALARM_DATE")), "acres": round(acres, 1), "share": round(min(1.0, share), 3), "cause": cause,
            }) + "\n")
            n_burns += 1
        if n_fires % 2000 == 0:
            log(n_fires, "fires,", n_burns, "burns")
    log("wrote", n_fires, "perimeters and", n_burns, "burns")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("step", choices=["fetch", "rows"])
    p.add_argument("--data", default="data", help="where perimeters/calfire.geojson goes")
    p.add_argument("--out", default="out", help="where the places' rows are and the new rows go")
    a = p.parse_args()
    {"fetch": fetch, "rows": rows}[a.step](a)


if __name__ == "__main__":
    main()
