#!/usr/bin/env python3
"""The pictures under a replay: what GOES saw of the sky, one frame every
few minutes, cut to the replay's box and drawn as the eye would see it by
day (true colour from the visible bands) and as heat by night (clean
infrared for clouds and smoke). The fires are left to the app.

NASA GIBS keeps GOES GeoColor for about 90 days, so an older replay draws
its own from NOAA's archive of the Cloud and Moisture Imagery product
(ABI-L2-MCMIPC over CONUS, ABI-L2-MCMIPF for the full disk), public and
without a sign-in. Each file is read, drawn and deleted.

    python3 tools/frames.py --replay la-2025 --sat 18 --product MCMIPC \\
        --from 2025-01-07T17:00:00Z --to 2025-01-09T06:00:00Z --every 10 \\
        --box 33.55,34.75,-119.15,-117.55 --out frames

writes frames/la-2025/<n>.jpg and frames/la-2025/frames.json (the box and
each frame's time), which `serve --frames frames` serves to the replay.

Needs numpy, netCDF4 and Pillow.
"""

import argparse
import concurrent.futures
import datetime as dt
import json
import math
import os
import re
import sys
import tempfile
import threading
import urllib.request

import numpy as np

BUCKET = "https://noaa-goes{sat}.s3.amazonaws.com"


def log(*a):
    print(*a, file=sys.stderr, flush=True)


def parse_t(s):
    return dt.datetime.fromisoformat(s.replace("Z", "+00:00"))


def keys(sat, product, hour):
    """The archive's files of an hour: (scan start, key)."""
    prefix = f"ABI-L2-{product}/{hour:%Y}/{hour:%j}/{hour:%H}/"
    url = f"{BUCKET.format(sat=sat)}/?list-type=2&prefix={prefix}"
    with urllib.request.urlopen(url, timeout=60) as r:
        text = r.read().decode()
    out = []
    for k in re.findall(r"<Key>([^<]+)</Key>", text):
        m = re.search(r"_s(\d{4})(\d{3})(\d{2})(\d{2})(\d{2})", k)
        if m:
            y, j, h, mi, s = map(int, m.groups())
            t = dt.datetime(y, 1, 1, h, mi, s, tzinfo=dt.timezone.utc) + dt.timedelta(days=j - 1)
            out.append((t, k))
    return out


def mercator_grid(box, width):
    """Lat and lon of each pixel of a picture even in Web Mercator (what a
    map's image source stretches linearly between its corners)."""
    s, n, w, e = box
    my = lambda lat: math.log(math.tan(math.pi / 4 + math.radians(lat) / 2))
    y0, y1 = my(n), my(s)
    height = max(2, round(width * (y1 - y0) / math.radians(w - e)))
    ys = np.linspace(y0, y1, height)
    lats = np.degrees(2 * np.arctan(np.exp(ys)) - math.pi / 2)
    lons = np.linspace(w, e, width)
    return np.meshgrid(lons, lats)


def fixed_grid(lon, lat, proj):
    """GOES-R ABI fixed grid scan angles (x, y, radians) of points (PUG
    volume 4, section 4.2.8)."""
    a, b = proj["semi_major_axis"], proj["semi_minor_axis"]
    hh = proj["perspective_point_height"] + a
    lon0 = math.radians(proj["longitude_of_projection_origin"])
    la, lo = np.radians(lat), np.radians(lon)
    e2 = (a * a - b * b) / (a * a)
    phi_c = np.arctan((b * b) / (a * a) * np.tan(la))
    r_c = b / np.sqrt(1 - e2 * np.cos(phi_c) ** 2)
    sx = hh - r_c * np.cos(phi_c) * np.cos(lo - lon0)
    sy = -r_c * np.cos(phi_c) * np.sin(lo - lon0)
    sz = r_c * np.sin(phi_c)
    y = np.arctan(sz / sx)
    x = np.arcsin(-sy / np.sqrt(sx * sx + sy * sy + sz * sz))
    return x, y


def sample(arr, fx, fy):
    """Bilinear sample of a 2-D array at fractional indices (column, row)."""
    h, w = arr.shape
    fx = np.clip(fx, 0, w - 1.001)
    fy = np.clip(fy, 0, h - 1.001)
    x0, y0 = np.floor(fx).astype(int), np.floor(fy).astype(int)
    dx, dy = fx - x0, fy - y0
    a = arr[y0, x0] * (1 - dx) + arr[y0, x0 + 1] * dx
    b = arr[y0 + 1, x0] * (1 - dx) + arr[y0 + 1, x0 + 1] * dx
    return a * (1 - dy) + b * dy


def sun_zenith(t, lat, lon):
    """The sun's zenith angle (degrees), good to a fraction of a degree."""
    doy = t.timetuple().tm_yday
    g = 2 * math.pi / 365 * (doy - 1 + (t.hour - 12) / 24)
    decl = 0.006918 - 0.399912 * math.cos(g) + 0.070257 * math.sin(g) - 0.006758 * math.cos(2 * g) + 0.000907 * math.sin(2 * g)
    eqt = 229.18 * (0.000075 + 0.001868 * math.cos(g) - 0.032077 * math.sin(g) - 0.014615 * math.cos(2 * g) - 0.040849 * math.sin(2 * g))
    minutes = t.hour * 60 + t.minute + t.second / 60
    ha = np.radians((minutes + eqt + 4 * lon) / 4 - 180)
    la = np.radians(lat)
    cz = np.sin(la) * math.sin(decl) + np.cos(la) * math.cos(decl) * np.cos(ha)
    return np.degrees(np.arccos(np.clip(cz, -1, 1)))


def draw(path, t, lon, lat):
    import netCDF4

    d = netCDF4.Dataset(path)
    p = d["goes_imager_projection"]
    proj = {k: float(p.getncattr(k)) for k in ("semi_major_axis", "semi_minor_axis", "perspective_point_height", "longitude_of_projection_origin")}
    hgt = proj["perspective_point_height"]
    x, y = fixed_grid(lon, lat, proj)
    xs, ys = d["x"][:], d["y"][:]
    # x and y are evenly spaced (radians); y goes from north to south.
    fx = (x - float(xs[0])) / float(xs[1] - xs[0])
    fy = (y - float(ys[0])) / float(ys[1] - ys[0])
    rows = slice(max(0, int(np.floor(fy.min())) - 2), int(np.ceil(fy.max())) + 3)
    cols = slice(max(0, int(np.floor(fx.min())) - 2), int(np.ceil(fx.max())) + 3)
    fx, fy = fx - cols.start, fy - rows.start

    def band(name, fill):
        v = d[name][rows, cols]
        return sample(np.ma.filled(v.astype("f4"), fill), fx, fy)

    red, blue, veg = band("CMI_C02", 0), band("CMI_C01", 0), band("CMI_C03", 0)
    ir, swir = band("CMI_C13", 300), None
    d.close()
    del hgt
    # By day: true colour, the green made from red, blue and the
    # vegetation band (ABI has no green band), brightened as the eye sees it.
    green = 0.45 * red + 0.1 * veg + 0.45 * blue
    rgb = np.stack([red, green, blue], -1)
    day = np.clip(rgb, 0, 1) ** (1 / 2.2)
    # The air's blue haze lifted off and the rest stretched, so land reads
    # as land and smoke as smoke.
    day = np.clip((day - np.array([0.10, 0.11, 0.16])) * 1.45, 0, 1)
    # By night: clouds and smoke from the clean infrared, colder brighter,
    # over a dark ground.
    cold = np.clip((295 - ir) / 85, 0, 1)
    night = np.stack([0.06 + 0.62 * cold, 0.08 + 0.66 * cold, 0.13 + 0.72 * cold], -1)
    sza = sun_zenith(t, lat, lon)
    w = np.clip((92 - sza) / 12, 0, 1)[..., None]
    img = day * w + night * (1 - w)
    # The fires themselves are left to the app, which draws GOES's fire
    # pixels over the picture: drawn here too, smoothed, they smear.
    del swir
    return (np.clip(img, 0, 1) * 255).astype("u1"), float(w.mean())


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--replay", required=True)
    ap.add_argument("--sat", required=True, help="16, 18 or 19")
    ap.add_argument("--product", default="MCMIPC", help="MCMIPC (CONUS) or MCMIPF (full disk)")
    ap.add_argument("--from", dest="start", required=True)
    ap.add_argument("--to", dest="end", required=True)
    ap.add_argument("--every", type=int, default=10, help="minutes between frames")
    ap.add_argument("--box", required=True, help="south,north,west,east")
    ap.add_argument("--width", type=int, default=720)
    ap.add_argument("--out", default="frames")
    ap.add_argument("--parallel", type=int, default=4, help="scans read at once")
    a = ap.parse_args()
    from PIL import Image

    box = [float(v) for v in a.box.split(",")]
    lon, lat = mercator_grid(box, a.width)
    out = os.path.join(a.out, a.replay)
    os.makedirs(out, exist_ok=True)
    manifest_path = os.path.join(out, "frames.json")
    done = {}
    if os.path.exists(manifest_path):
        done = {f["t"]: f for f in json.load(open(manifest_path))["frames"]}
    # The scan nearest each frame's time, each scan once.
    t, end = parse_t(a.start), parse_t(a.end)
    listed, todo = {}, []
    while t <= end:
        hour = t.replace(minute=0, second=0, microsecond=0)
        if hour not in listed:
            listed[hour] = keys(a.sat, a.product, hour)
        if listed[hour]:
            st, key = min(listed[hour], key=lambda c: abs((c[0] - t).total_seconds()))
            if not todo or todo[-1][0] != st:
                todo.append((st, key))
        t += dt.timedelta(minutes=a.every)

    reading = threading.Lock()

    def one(n, st, key):
        iso = st.strftime("%Y-%m-%dT%H:%M:%SZ")
        if iso in done and done[iso]["n"] == n and os.path.exists(os.path.join(out, f"{n}.jpg")):
            return done[iso]
        with tempfile.NamedTemporaryFile(suffix=".nc", dir=a.out) as tmp:
            urllib.request.urlretrieve(f"{BUCKET.format(sat=a.sat)}/{key}", tmp.name)
            # HDF5 is not safe across threads: the downloads overlap, the
            # reads take turns.
            with reading:
                pix, dayshare = draw(tmp.name, st, lon, lat)
        Image.fromarray(pix).save(os.path.join(out, f"{n}.jpg"), quality=84, optimize=True)
        log(n, iso, "day" if dayshare > 0.5 else "night")
        return {"n": n, "t": iso, "day": round(dayshare, 2)}

    with concurrent.futures.ThreadPoolExecutor(a.parallel) as pool:
        frames = list(pool.map(lambda x: one(x[0], *x[1]), enumerate(todo)))
    json.dump({"replay": a.replay, "box": box, "sat": f"G{a.sat}", "product": a.product, "frames": frames}, open(manifest_path, "w"))
    log("frames:", len(frames))


if __name__ == "__main__":
    main()
