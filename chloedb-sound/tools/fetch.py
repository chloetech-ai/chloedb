#!/usr/bin/env python3
"""Download the free sounds Sound Explorer is built from, licences checked.

    python3 -I tools/fetch.py fsd50k DIR [--clips 2500]
    python3 -I tools/fetch.py fma DIR [--shards 9]
    python3 -I tools/fetch.py commons DIR [--files 300] [--per-category 25] [--min-seconds 20]

Each keeps only what may be shown on a public site and played as an
excerpt, with credit: CC0, public domain, CC BY, CC BY-SA and the Free Art
License (and the GFDL on Commons). Non-commercial (NC), no-derivatives
(ND) and Sampling+ sounds are left out, so most of FMA is (about 85% of
its tracks).

  fsd50k   FSD50K's evaluation clips (Freesound, labelled by hand with 200
           classes of the AudioSet ontology), CC0 and CC BY only, the same
           number of each class as far as it goes, from the dataset's mirror
           on Hugging Face (Fhrozen/FSD50k), with its metadata.
  fma      The Free Music Archive's 30-second clips (fma_small, from
           benjamin-paine/free-music-archive-small on Hugging Face), read
           shard by shard (each about 500 MB, deleted once read), and the
           genres' names from FMA's fma_metadata.zip (342 MB, deleted too).
  commons  Sounds of Wikimedia Commons under its sound categories, each
           file's page read for its licence, author, description, date and
           place, then the original downloaded. One request a second to the
           pages, and as long as upload.wikimedia.org asks between files
           (it answers 429 with a Retry-After to a busy address). A stopped
           run goes on where it was.

Then run tools/prepare.py on the folders. Only the standard library is
needed, but pyarrow for fma. Run it with python3 -I, from outside the
folders it downloads to.
"""

import argparse
import csv
import html
import io
import json
import os
import random
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile
import concurrent.futures as cf

UA = "ChloeDB-SoundExplorer-demo/0.1 (https://github.com/chloetech-ai/chloedb)"
HF = "https://huggingface.co/datasets"


def log(*a):
    print("fetch:", *a, file=sys.stderr, flush=True)


def download(url, path, timeout=120, tries=4):
    """`url` into `path`, through a .part file; True when it is there."""
    if os.path.exists(path) and os.path.getsize(path) > 0:
        return True
    err = None
    for _ in range(tries):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": UA})
            with urllib.request.urlopen(req, timeout=timeout) as r, open(path + ".part", "wb") as o:
                while chunk := r.read(1 << 20):
                    o.write(chunk)
            os.rename(path + ".part", path)
            return True
        except Exception as e:  # noqa: BLE001 - a file that fails is skipped
            err = e
            time.sleep(2)
    log("failed", url, err)
    return False


# --- FSD50K -----------------------------------------------------------------

def fsd50k(a):
    d = a.dir
    os.makedirs(f"{d}/clips", exist_ok=True)
    base = f"{HF}/Fhrozen/FSD50k/resolve/main"
    for f in ["metadata/eval_clips_info_FSD50K.json", "labels/eval.csv", "labels/vocabulary.csv", "metadata/class_info_FSD50K.json", "LICENSE-DATASET.md"]:
        download(f"{base}/{f}", f"{d}/{os.path.basename(f)}")
    info = json.load(open(f"{d}/eval_clips_info_FSD50K.json"))
    ok = ("publicdomain/zero", "licenses/by/")
    rows = []
    for r in csv.DictReader(open(f"{d}/eval.csv")):
        i = info.get(r["fname"])
        if i and any(k in i["license"] for k in ok):
            rows.append((r["fname"], r["labels"].split(",")))
    random.seed(7)
    random.shuffle(rows)
    by = {}
    for f, labels in rows:
        by.setdefault(labels[0], []).append(f)
    pick = []
    while len(pick) < a.clips and any(by.values()):
        for leaf in sorted(by):
            if by[leaf] and len(pick) < a.clips:
                pick.append(by[leaf].pop())
    with cf.ThreadPoolExecutor(8) as ex:
        got = sum(ex.map(lambda f: download(f"{base}/clips/eval/{f}.wav", f"{d}/clips/{f}.wav"), pick))
    json.dump(pick, open(f"{d}/picked.json", "w"))
    log(f"fsd50k: {got} clips of {len(pick)} picked, {len(by)} classes")


# --- FMA --------------------------------------------------------------------

FMA_LIC = {0: ("CC BY 1.0", "by/1.0"), 1: ("CC BY 2.0", "by/2.0"), 2: ("CC BY 2.5", "by/2.5"), 3: ("CC BY 3.0", "by/3.0"), 4: ("CC BY 4.0", "by/4.0"),
           24: ("CC BY-SA 2.0", "by-sa/2.0"), 25: ("CC BY-SA 2.5", "by-sa/2.5"), 26: ("CC BY-SA 3.0", "by-sa/3.0"), 27: ("CC BY-SA 4.0", "by-sa/4.0"),
           30: ("CC0 1.0", None), 32: ("Free Art License", None)}


def fma_licence(code):
    name, path = FMA_LIC[code]
    if code == 30:
        return name, "https://creativecommons.org/publicdomain/zero/1.0/"
    if code == 32:
        return name, "https://artlibre.org/licence/lal/en/"
    return name, f"https://creativecommons.org/licenses/{path}/"


def fma(a):
    import pyarrow.parquet as pq

    d = a.dir
    os.makedirs(f"{d}/tracks", exist_ok=True)
    if not os.path.exists(f"{d}/genres.json"):
        z = f"{d}/fma_metadata.zip"
        if download("https://os.unil.cloud.switch.ch/fma/fma_metadata.zip", z, timeout=600):
            with zipfile.ZipFile(z) as zf:
                name = next(n for n in zf.namelist() if n.endswith("genres.csv"))
                g = {r["genre_id"]: r["title"] for r in csv.DictReader(io.TextIOWrapper(zf.open(name), "utf-8"))}
            json.dump(g, open(f"{d}/genres.json", "w"))
            os.remove(z)
    genres = json.load(open(f"{d}/genres.json")) if os.path.exists(f"{d}/genres.json") else {}
    repo = "benjamin-paine/free-music-archive-small"
    tree = json.load(urllib.request.urlopen(urllib.request.Request(f"https://huggingface.co/api/datasets/{repo}/tree/main/data", headers={"User-Agent": UA}), timeout=60))
    shards = sorted(t["path"] for t in tree if t["path"].endswith(".parquet"))[: a.shards]
    done = set()
    if os.path.exists(f"{d}/shards.done"):
        done = set(open(f"{d}/shards.done").read().split())
    kept = 0
    with open(f"{d}/fma.jsonl", "a") as meta:
        for s in shards:
            if s in done:
                continue
            p = f"{d}/{os.path.basename(s)}"
            if not download(f"{HF}/{repo}/resolve/main/{s}", p, timeout=1800):
                continue
            pf = pq.ParquetFile(p)
            for g in range(pf.num_row_groups):
                for r in pf.read_row_group(g).to_pylist():
                    if r.get("license") not in FMA_LIC:
                        continue
                    tid = os.path.splitext(os.path.basename(r["audio"]["path"]))[0]
                    path = f"{d}/tracks/{tid}.mp3"
                    with open(path, "wb") as o:
                        o.write(r["audio"]["bytes"])
                    del r["audio"]
                    r["genres"] = [genres.get(str(x), str(x)) for x in (r.get("genres") or [])]
                    lic, url = fma_licence(r["license"])
                    r.update(id=tid, path=path, licence=lic, licence_url=url, released=str(r.get("released") or "")[:10])
                    meta.write(json.dumps(r, default=str) + "\n")
                    kept += 1
            os.remove(p)
            with open(f"{d}/shards.done", "a") as o:
                o.write(s + "\n")
            log(f"fma: {s} read, {kept} tracks kept so far")
    log(f"fma: {kept} tracks kept")


# --- Wikimedia Commons ------------------------------------------------------

ROOTS = ["Category:Sounds_by_type", "Category:Audio_files_of_animal_sounds", "Category:Sounds_of_musical_instruments", "Category:Sounds_of_water",
         "Category:Sounds_relating_to_transport", "Category:Sounds_of_machinery", "Category:Sounds_relating_to_weather"]
COMMONS = "https://commons.wikimedia.org"


def page(url):
    time.sleep(1.0)
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read().decode("utf-8", "replace")


def category(name, pages=2):
    """A category's subcategories and sound files, from its HTML page."""
    url = f"{COMMONS}/wiki/" + urllib.parse.quote(name.replace(" ", "_"))
    subs, files = [], []
    for _ in range(pages):
        t = page(url)
        for m, _n in re.findall(r'<bdi dir="ltr"><a href="/wiki/(Category:[^"#]+)"[^>]*>[^<]*</a></bdi> <span title="Contains[^"]*"[^>]*>\(([^)]*)\)', t):
            if "empty" not in _n:
                subs.append(html.unescape(urllib.parse.unquote(m)))
        files += [html.unescape(urllib.parse.unquote(m)) for m in re.findall(r'href="/wiki/(File:[^"#]+\.(?:ogg|oga|opus|wav|flac|mp3|webm))"', t, re.I)]
        nxt = re.search(r'href="([^"]+filefrom=[^"]+)"[^>]*>next page', t)
        if not nxt:
            break
        url = COMMONS + html.unescape(nxt.group(1))
    return sorted(set(subs)), sorted(set(files))


def _field(t, id_):
    m = re.search(r'id="fileinfotpl&#95;' + id_ + r'"[^>]*>.*?</td>\s*<td[^>]*>(.*?)</td>', t, re.S)
    return m.group(1) if m else ""


def _text(h):
    h = re.sub(r'<span class="language[^"]*"[^>]*>.*?</span>', "", h, flags=re.S)
    h = re.sub(r"<(script|style)[^>]*>.*?</\1>", "", h, flags=re.S)
    return re.sub(r"\s+", " ", html.unescape(re.sub(r"<[^>]+>", " ", h))).strip(" :")


def _english(h):
    m = re.search(r'<div class="description en"[^>]*>(.*?)</div>', h, re.S)
    return _text(m.group(1) if m else h)


def _seconds(s):
    return sum(float(v) * {"h": 3600, "min": 60, "s": 1}[u] for v, u in re.findall(r"([\d.]+)\s*(h|min|s)\b", s))


def file_page(f, cat):
    """What a file's page says of it, or None when it may not be used."""
    url = f"{COMMONS}/wiki/" + urllib.parse.quote(f.replace(" ", "_"))
    t = page(url)
    lic = re.search(r'class="licensetpl&#95;short"[^>]*>([^<]*)<', t)
    link = re.search(r'class="licensetpl&#95;link"[^>]*>([^<]*)<', t)
    info = re.search(r'<span class="fileInfo">\(([^)]*)\)', t)
    orig = re.search(r'href="(https://upload\.wikimedia\.org/wikipedia/commons/[0-9a-f]/[0-9a-f]{2}/[^"?]+)[^"]*"[^>]*class="internal"', t)
    cats = re.search(r'"wgCategories":(\[[^\]]*\])', t)
    geo = re.search(r'class="geo"[^>]*>\s*([-\d.]+);\s*([-\d.]+)', t)
    if not (lic and orig and info):
        return None, "no licence or file"
    short = lic.group(1).strip()
    if re.search(r"\bNC\b|\bND\b|non-?commercial|sampling", short, re.I):
        return None, f"licence {short}"
    size = re.search(r"file size:\s*([\d.,]+)\s*(KB|MB|GB)", info.group(1))
    mb = float(size.group(1).replace(",", "")) / {"KB": 1024, "MB": 1, "GB": 1 / 1024}[size.group(2)] if size else 0
    aut = _field(t, "aut")
    aut_link = re.search(r'href="(/wiki/User:[^"]+|https?://[^"]+)"', aut)
    author_url = ""
    if aut_link:
        author_url = COMMONS + aut_link.group(1) if aut_link.group(1).startswith("/") else aut_link.group(1)
    return {
        "file": f, "category": cat, "licence": short, "licence_url": link.group(1).strip() if link else "",
        "author": _text(aut)[:200], "author_url": author_url,
        "description": _english(_field(t, "desc"))[:2000], "date": _text(_field(t, "date"))[:60],
        "categories": json.loads(html.unescape(cats.group(1))) if cats else [],
        "lat": float(geo.group(1)) if geo else None, "lon": float(geo.group(2)) if geo else None,
        "seconds": _seconds(info.group(1)), "mb": mb, "info": info.group(1), "url": html.unescape(orig.group(1)), "page": url,
    }, ""


def original(url, path):
    """The original file, waiting as long as upload.wikimedia.org asks."""
    for _ in range(8):
        try:
            time.sleep(6.0)
            req = urllib.request.Request(url, headers={"User-Agent": UA})
            with urllib.request.urlopen(req, timeout=300) as r, open(path + ".part", "wb") as o:
                while chunk := r.read(1 << 20):
                    o.write(chunk)
            os.rename(path + ".part", path)
            return True
        except urllib.error.HTTPError as e:
            if e.code == 429:
                wait = min(int(e.headers.get("retry-after") or 300), 900)
                log(f"commons: 429, waiting {wait} s as asked")
                time.sleep(wait)
                continue
            log("commons: failed", url, e)
            return False
        except Exception as e:  # noqa: BLE001
            log("commons: failed", url, e)
            return False
    return False


def commons(a):
    d = a.dir
    os.makedirs(f"{d}/files", exist_ok=True)
    state = f"{d}/cats.json"
    if os.path.exists(state):
        cats = json.load(open(state))
    else:
        cats, seen, todo = {}, set(), [(r, 0) for r in ROOTS]
        while todo:
            c, depth = todo.pop(0)
            if c in seen:
                continue
            seen.add(c)
            try:
                subs, files = category(c)
            except Exception as e:  # noqa: BLE001
                log("commons: category failed", c, e)
                continue
            cats[c] = files
            if depth < 1:
                todo += [(s, depth + 1) for s in subs]
        json.dump(cats, open(state, "w"))
    order, used = [], set()
    for i in range(a.per_category):
        for c in sorted(cats):
            if i < len(cats[c]) and cats[c][i] not in used:
                used.add(cats[c][i])
                order.append((cats[c][i], c))
    meta_path = f"{d}/commons.jsonl"
    done = set()
    if os.path.exists(meta_path):
        done = {json.loads(l)["file"] for l in open(meta_path)}
    kept = len(done)
    log(f"commons: {len(order)} candidates, {kept} kept before")
    with open(meta_path, "a") as meta:
        for f, c in order:
            if kept >= a.files:
                break
            if f in done:
                continue
            try:
                rec, why = file_page(f, c)
            except Exception as e:  # noqa: BLE001
                log("commons: page failed", f, e)
                continue
            if rec is None:
                continue
            if rec["mb"] > 60 or not (max(a.min_seconds, 1) <= rec["seconds"] <= 1200):
                continue
            path = f"{d}/files/" + re.sub(r"[^A-Za-z0-9._-]", "_", f[5:])[-150:]
            if not original(rec["url"], path):
                continue
            rec["path"] = path
            meta.write(json.dumps(rec) + "\n")
            meta.flush()
            kept += 1
    log(f"commons: {kept} files kept")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="source", required=True)
    p = sub.add_parser("fsd50k")
    p.add_argument("dir")
    p.add_argument("--clips", type=int, default=2500)
    p = sub.add_parser("fma")
    p.add_argument("dir")
    p.add_argument("--shards", type=int, default=9, help="of fma_small's 15 (each about 80 usable tracks)")
    p = sub.add_parser("commons")
    p.add_argument("dir")
    p.add_argument("--files", type=int, default=300)
    p.add_argument("--per-category", type=int, default=25)
    p.add_argument("--min-seconds", type=float, default=0, help="only recordings at least this long")
    a = ap.parse_args()
    {"fsd50k": fsd50k, "fma": fma, "commons": commons}[a.source](a)


if __name__ == "__main__":
    main()
