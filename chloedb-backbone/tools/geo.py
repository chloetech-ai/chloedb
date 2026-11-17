#!/usr/bin/env python3
"""Write ui/geo.json: every country's outline on the page's world map, its
name, its continent and where to put its pin.

The outlines are Natural Earth's at 1:110m (world-atlas, public domain);
the codes, names and continents are Unicode CLDR's. The projection is the
page's: equirectangular from 80N to 58S, 1000 wide.

    python3 tools/geo.py > ui/geo.json
"""
import json
import sys
import urllib.request

ATLAS = "https://cdn.jsdelivr.net/npm/world-atlas@2.0.2/countries-110m.json"
CLDR = "https://cdn.jsdelivr.net/npm/cldr-core@44.1.0/supplemental/"
NAMES = "https://cdn.jsdelivr.net/npm/cldr-localenames-modern@44.1.0/main/en/territories.json"
W, H, TOP, SPAN = 1000.0, 383.3, 80.0, 138.0
# Pins of countries whose largest outline crosses the antimeridian.
PIN = {"RU": (95.0, 62.0), "FJ": (178.0, -17.8), "KI": (173.0, 1.4)}
CONTINENT = {"002": "Africa", "019": "Americas", "142": "Asia", "150": "Europe", "009": "Oceania"}


def get(url):
    with urllib.request.urlopen(url, timeout=60) as r:
        return json.load(r)


def project(lon, lat):
    return ((lon + 180) / 360 * W, (TOP - lat) / SPAN * H)


def arcs_of(topo):
    sx, sy = topo["transform"]["scale"]
    tx, ty = topo["transform"]["translate"]
    out = []
    for arc in topo["arcs"]:
        x = y = 0
        pts = []
        for dx, dy in arc:
            x += dx
            y += dy
            pts.append((x * sx + tx, y * sy + ty))
        out.append(pts)
    return out


def ring(arcs, idx):
    pts = []
    for i in idx:
        a = arcs[i] if i >= 0 else list(reversed(arcs[~i]))
        pts.extend(a if not pts else a[1:])
    return pts


def unwrap(r):
    """A ring whose longitudes jump across the antimeridian, made continuous
    (some points then lie past 180 or before -180)."""
    out, shift, prev = [], 0.0, None
    for lon, lat in r:
        if prev is not None:
            d = lon + shift - prev
            if d > 180:
                shift -= 360
            elif d < -180:
                shift += 360
        out.append((lon + shift, lat))
        prev = lon + shift
    return out


def path(rings):
    """The rings as one SVG path. A ring that crosses the antimeridian is
    drawn twice, a world apart, and the page clips the map to its frame,
    so Chukotka and Fiji show at both edges and no band crosses the map."""
    parts = []
    for r in rings:
        r = unwrap(r)
        lons = [p[0] for p in r]
        copies = [0.0]
        if max(lons) > 180:
            copies.append(-360.0)
        if min(lons) < -180:
            copies.append(360.0)
        for k in copies:
            last = None
            cmd = []
            for lon, lat in r:
                lat = max(min(lat, TOP), TOP - SPAN)
                x, y = project(lon + k, lat)
                p = (round(x, 1), round(y, 1))
                if p == last:
                    continue
                cmd.append(f"{p[0]:g},{p[1]:g}")
                last = p
            if len(cmd) >= 3:
                parts.append("M" + "L".join(cmd) + "Z")
    return "".join(parts)


def area_centroid(r):
    a = cx = cy = 0.0
    pts = [project(*p) for p in r]
    for (x0, y0), (x1, y1) in zip(pts, pts[1:] + pts[:1]):
        f = x0 * y1 - x1 * y0
        a += f
        cx += (x0 + x1) * f
        cy += (y0 + y1) * f
    if abs(a) < 1e-9:
        return 0.0, pts[0]
    return abs(a) / 2, (cx / (3 * a), cy / (3 * a))


def main():
    topo = get(ATLAS)
    codes = get(CLDR + "codeMappings.json")["supplemental"]["codeMappings"]
    num = {v["_numeric"]: k for k, v in codes.items() if "_numeric" in v and len(k) == 2}
    contain = get(CLDR + "territoryContainment.json")["supplemental"]["territoryContainment"]
    names = get(NAMES)["main"]["en"]["localeDisplayNames"]["territories"]
    parent = {}
    for g, v in contain.items():
        if "-status" in g:
            continue
        for c in v.get("_contains", []):
            parent.setdefault(c, []).append(g)

    def continent(cc):
        seen, todo = set(), [cc]
        while todo:
            x = todo.pop()
            if x in CONTINENT:
                return CONTINENT[x]
            for p in parent.get(x, []):
                if p not in seen:
                    seen.add(p)
                    todo.append(p)
        return ""

    arcs = arcs_of(topo)
    out = {}
    for g in topo["objects"]["countries"]["geometries"]:
        cc = num.get(str(g.get("id", "")).zfill(3))
        if not cc or cc == "AQ" or not g.get("arcs"):
            continue
        polys = g["arcs"] if g["type"] == "MultiPolygon" else [g["arcs"]]
        rings = [ring(arcs, r) for poly in polys for r in poly]
        outer = [ring(arcs, poly[0]) for poly in polys]
        big = max((area_centroid(r) for r in outer), key=lambda t: t[0])
        d = path(rings)
        if not d:
            continue
        x, y = project(*PIN[cc]) if cc in PIN else big[1]
        out[cc] = {"n": names.get(cc, g["properties"].get("name", cc)), "c": continent(cc), "x": round(x, 1), "y": round(y, 1), "d": d}
    # Countries too small for 1:110m: a name, a continent and no outline;
    # the page places their pins at their exchanges.
    for cc, n in names.items():
        if len(cc) == 2 and cc.isalpha() and cc.isupper() and cc not in out and cc in codes and cc != "AQ":
            out[cc] = {"n": n, "c": continent(cc)}
    json.dump({"w": W, "h": H, "top": TOP, "span": SPAN, "source": "Natural Earth 1:110m (world-atlas 2.0.2), Unicode CLDR 44", "countries": dict(sorted(out.items()))}, sys.stdout, separators=(",", ":"), ensure_ascii=False)


if __name__ == "__main__":
    main()
