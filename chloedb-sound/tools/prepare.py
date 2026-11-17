#!/usr/bin/env python3
"""Turn downloaded sounds into Sound Explorer's rows.

Reads the three free sources as their downloaders left them:

  --fsd50k DIR    FSD50K's eval_clips_info_FSD50K.json, eval.csv and clips/<id>.wav
  --fma DIR       fma.jsonl (a row a track, with its licence) and tracks/<id>.mp3
  --commons DIR   commons.jsonl (a row a file, read from its page) and the files it names

and writes, in --out:

  audio/<id>.mp3      a preview of each sound, for the page to play
  au_moments.jsonl    a row every five seconds of each sound: its ten seconds'
                      vector by CLAP, made by ChloeDB's server (POST /v1/embed)
  au_sounds.jsonl     a row a sound: its words, credit, licence, waveform,
                      place on the map, and the mean of its windows that start
                      every ten seconds (what the server makes of a whole file)

The server must have CLAP (chloedb-server built with --features audio) and
the app's tables (`chloedb-sound init`): the clips are embedded through
au_labels' model. A run that stops goes on where it was: sounds already in
the partial file are skipped.

Only ffmpeg and numpy are needed. Run it with python3 -I, from outside the
folders the sounds were downloaded to.
"""

import argparse
import base64
import concurrent.futures as cf
import csv
import hashlib
import io
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request
import wave

import numpy as np

RATE = 48_000
WINDOW = 10 * RATE
HOP = 5 * RATE
MAX_SECONDS = 1200
PEAKS = 160


def log(*a):
    print("prepare:", *a, file=sys.stderr, flush=True)


# --- the sources ------------------------------------------------------------

def lic_code(short):
    s = short.strip().lower()
    if s.startswith("cc0") or "publicdomain/zero" in s:
        return "cc0"
    if s.startswith("public domain") or s.startswith("pd"):
        return "pd"
    if "by-sa" in s:
        return "by-sa"
    if s.startswith("cc by") or "licenses/by/" in s or s.startswith("cc-by"):
        return "by"
    if s.startswith("gfdl") or s.startswith("gnu"):
        return "gfdl"
    if "free art" in s:
        return "fal"
    return "other"


def fsd_licence(url):
    m = re.search(r"licenses/by/([\d.]+)", url)
    if m:
        return f"CC BY {m.group(1)}", url
    if "publicdomain/zero" in url:
        return "CC0 1.0", url
    return None, url


FSD_KIND = [
    ("music", {"Music", "Musical_instrument"}),
    ("animal", {"Animal", "Domestic_animals_and_pets", "Wild_animals", "Bird", "Insect", "Livestock_and_farm_animals_and_working_animals"}),
    ("voice", {"Human_voice", "Speech"}),
    ("nature", {"Natural_sounds", "Water", "Wind", "Thunderstorm", "Rain", "Fire", "Ocean", "Stream"}),
]


def human(label):
    return ", ".join(p.replace("_", " ") for p in label.split("_and_"))


def fsd50k(d):
    info = json.load(open(os.path.join(d, "eval_clips_info_FSD50K.json")))
    labels = {r["fname"]: r["labels"].split(",") for r in csv.DictReader(open(os.path.join(d, "eval.csv")))}
    for f in sorted(os.listdir(os.path.join(d, "clips"))):
        if not f.endswith(".wav"):
            continue
        fid = f[:-4]
        i = info.get(fid)
        if not i:
            continue
        licence, url = fsd_licence(i["license"])
        if not licence:
            continue
        ls = labels.get(fid, [])
        kind = next((k for k, s in FSD_KIND if s & set(ls)), "effect")
        title = re.sub(r"\.(wav|aiff?|flac|mp3|ogg|m4a)$", "", i["title"], flags=re.I).replace("_", " ").strip() or f"Freesound {fid}"
        yield {
            "id": f"fsd:{fid}", "path": os.path.join(d, "clips", f), "source": "fsd50k",
            "title": title, "description": i.get("description", ""), "tags": i.get("tags", [])[:30],
            "labels": [l.lower() for l in ls], "label_words": [human(l) for l in ls],
            "author": i.get("uploader", ""), "author_url": f"https://freesound.org/people/{i.get('uploader', '')}/",
            "page": f"https://freesound.org/s/{fid}/", "licence": licence, "licence_url": url, "kind": kind,
        }


def fma(d):
    for l in open(os.path.join(d, "fma.jsonl")):
        r = json.loads(l)
        year = int(r["released"][:4]) if re.match(r"\d{4}", r.get("released") or "") else None
        yield {
            "id": f"fma:{r['id']}", "path": r["path"], "source": "fma",
            "title": r.get("title") or f"Track {r['id']}", "description": r.get("album_title") or "",
            "tags": (r.get("tags") or [])[:30], "genres": r.get("genres") or [],
            "author": r.get("artist") or "", "author_url": r.get("artist_url") or "",
            "page": r.get("url") or "", "licence": r["licence"], "licence_url": r["licence_url"],
            "kind": "music", "year": year, "stereo": True,
        }


COMMONS_SKIP = re.compile(r"^(cc[- ]|gfdl|pd[- ]|public domain|self-published|files |media |pages |uses of|audio files with|ogg|wav|flac|featured|valued|quality|translation|license|creative commons|attribution|own work|.* missing|.*\bsdc\b)", re.I)
ANIMAL = re.compile(r"animal|bird|frog|anura|insect|mammal|dog|cat|cow|horse|aves|idae\b|inae\b|calls?\b|song", re.I)
MUSIC = re.compile(r"music|instrument|piano|guitar|violin|organ|drum|bell|choir|song", re.I)
NATURE = re.compile(r"nature|water|rain|weather|thunder|storm|wind|river|stream|sea|ocean|wave|waterfall|forest|fire", re.I)


def commons(d):
    for l in open(os.path.join(d, "commons.jsonl")):
        r = json.loads(l)
        if lic_code(r["licence"]) == "other" and "cc" not in r["licence"].lower():
            continue
        cats = [c for c in r.get("categories", []) if not COMMONS_SKIP.match(c)]
        where = r.get("category", "") + " " + " ".join(cats)
        if ANIMAL.search(where):
            kind = "animal"
        elif MUSIC.search(where):
            kind = "music"
        elif NATURE.search(where):
            kind = "nature"
        else:
            kind = "effect"
        if kind in ("nature", "effect") and r.get("seconds", 0) >= 60:
            kind = "place"
        name = r["file"][5:]
        title = re.sub(r"\.(ogg|oga|opus|wav|flac|mp3|webm)$", "", name, flags=re.I).replace("_", " ").strip()
        year = re.search(r"\b(1[89]\d\d|20\d\d)\b", r.get("date") or "")
        yield {
            "id": "wc:" + hashlib.sha1(name.encode()).hexdigest()[:16], "path": r["path"], "source": "commons",
            "title": title, "description": r.get("description", ""), "tags": cats[:30],
            "author": r.get("author", ""), "author_url": r.get("author_url", ""),
            "page": r["page"], "licence": r["licence"], "licence_url": r.get("licence_url", ""),
            "kind": kind, "year": int(year.group(1)) if year else None, "lat": r.get("lat"), "lon": r.get("lon"),
        }


# --- sound ------------------------------------------------------------------

def decode(path):
    """The first MAX_SECONDS of a file, at 48 kHz, one channel, as float32."""
    p = subprocess.run(["ffmpeg", "-nostdin", "-loglevel", "error", "-i", path, "-t", str(MAX_SECONDS), "-ac", "1", "-ar", str(RATE), "-f", "f32le", "-"], capture_output=True)
    if p.returncode != 0:
        raise RuntimeError(p.stderr.decode(errors="replace")[:300])
    return np.frombuffer(p.stdout, dtype=np.float32)


def preview(path, out, stereo, seconds):
    rate = "128k" if stereo else ("96k" if seconds <= 180 else "64k")
    args = ["ffmpeg", "-nostdin", "-loglevel", "error", "-y", "-i", path, "-t", str(MAX_SECONDS), "-vn", "-ac", "2" if stereo else "1", "-ar", "44100", "-b:a", rate, out]
    p = subprocess.run(args, capture_output=True)
    if p.returncode != 0:
        raise RuntimeError(p.stderr.decode(errors="replace")[:300])


def peaks(x, n=PEAKS):
    if len(x) == 0:
        return [0] * n
    edges = np.linspace(0, len(x), n + 1).astype(int)
    p = np.array([np.abs(x[a:b]).max() if b > a else 0.0 for a, b in zip(edges[:-1], edges[1:])])
    top = max(p.max(), 1e-6)
    return [int(round(v)) for v in np.sqrt(p / top) * 99]


def windows(x):
    """(start, samples) of every ten seconds starting every five: those at a
    multiple of ten cover the file as the server's own windows do (a tail
    shorter than a second is left out when there is more), the others when
    at least five seconds are left."""
    n = len(x)
    out = []
    for s in range(0, max(n, 1), HOP):
        left = n - s
        base = (s // HOP) % 2 == 0
        if s == 0 or (base and left >= RATE) or (not base and left >= HOP):
            out.append((s // RATE, x[s:s + WINDOW]))
    return out


def wav_b64(x):
    pcm = (np.clip(x, -1, 1) * 32767).astype("<i2").tobytes()
    b = io.BytesIO()
    with wave.open(b, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(pcm)
    return base64.b64encode(b.getvalue()).decode()


class Server:
    def __init__(self, url, key):
        self.url, self.key = url.rstrip("/"), key

    def post(self, path, body, timeout=300):
        req = urllib.request.Request(self.url + path, data=json.dumps(body).encode(), headers={"content-type": "application/json", "x-chloedb-client": "chloedb-sound-prepare"})
        if self.key:
            req.add_header("authorization", "Bearer " + self.key)
        for t in range(6):
            try:
                with urllib.request.urlopen(req, timeout=timeout) as r:
                    return json.loads(r.read())
            except urllib.error.HTTPError as e:
                if e.code in (429, 500, 502, 503, 504) and t < 5:
                    time.sleep(2 ** t)
                    continue
                raise RuntimeError(f"{path}: HTTP {e.code}: {e.read()[:300]!r}")
            except (urllib.error.URLError, TimeoutError, ConnectionError) as e:
                if t < 5:
                    time.sleep(2 ** t)
                    continue
                raise RuntimeError(f"{path}: {e}")

    def embed(self, b64):
        v = self.post("/v1/embed", {"audio": b64, "table": "au_labels", "field": "emb"})["vector"]
        return np.array(v, dtype=np.float32)

    def labels(self):
        j = self.post("/v1/query", {"sql": "SELECT id, name, emb FROM au_labels LIMIT 1000", "objects": True})
        return [(r["id"], np.array(r["emb"], dtype=np.float32)) for r in j["rows"] if r.get("emb")]


# --- the map ----------------------------------------------------------------

def tsne(X, perplexity=30.0, iters=700, seed=7):
    """A plain t-SNE (van der Maaten and Hinton, 2008) of a few thousand
    points, with PCA's first two axes as the start."""
    n = X.shape[0]
    X = X - X.mean(0)
    sq = (X * X).sum(1)
    D = np.maximum(sq[:, None] + sq[None, :] - 2 * X @ X.T, 0)
    P = np.zeros((n, n), dtype=np.float64)
    target = np.log(perplexity)
    for i in range(n):
        d = np.delete(D[i], i)
        lo, hi, beta = 0.0, np.inf, 1.0
        for _ in range(60):
            p = np.exp(-(d - d.min()) * beta)
            s = p.sum()
            h = np.log(s) + beta * ((d - d.min()) * p).sum() / s
            if abs(h - target) < 1e-4:
                break
            if h > target:
                lo = beta
                beta = beta * 2 if hi == np.inf else (beta + hi) / 2
            else:
                hi = beta
                beta = (beta + lo) / 2
        P[i, np.arange(n) != i] = p / s
    P = (P + P.T) / (2 * n)
    P = np.maximum(P, 1e-12)
    u, s_, vt = np.linalg.svd(X, full_matrices=False)
    Y = u[:, :2] * s_[:2]
    Y = Y / Y[:, 0].std() * 1e-4
    rng = np.random.default_rng(seed)
    Y += rng.normal(0, 1e-5, Y.shape)
    V = np.zeros_like(Y)
    gains = np.ones_like(Y)
    for it in range(iters):
        exag = 12.0 if it < 200 else 1.0
        sy = (Y * Y).sum(1)
        num = 1.0 / (1.0 + np.maximum(sy[:, None] + sy[None, :] - 2 * Y @ Y.T, 0))
        np.fill_diagonal(num, 0)
        Q = np.maximum(num / num.sum(), 1e-12)
        PQ = (exag * P - Q) * num
        grad = 4 * ((np.diag(PQ.sum(1)) - PQ) @ Y)
        mom = 0.5 if it < 250 else 0.8
        gains = np.where(np.sign(grad) != np.sign(V), gains + 0.2, gains * 0.8).clip(0.01)
        V = mom * V - 200.0 * gains * grad
        Y = Y + V
        Y -= Y.mean(0)
    Y = (Y - Y.min(0)) / np.maximum(Y.max(0) - Y.min(0), 1e-9)
    return Y


# --- the run ----------------------------------------------------------------

def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--fsd50k")
    ap.add_argument("--fma")
    ap.add_argument("--commons")
    ap.add_argument("--out", required=True)
    ap.add_argument("--strata", default=os.environ.get("CHLOEDB_URL", "http://127.0.0.1:7700"))
    ap.add_argument("--key", default=os.environ.get("CHLOEDB_KEY"))
    ap.add_argument("--limit", type=int, default=0, help="at most this many sounds of each source")
    ap.add_argument("--concurrency", type=int, default=3, help="windows embedded at once")
    a = ap.parse_args()
    out = a.out
    os.makedirs(os.path.join(out, "audio"), exist_ok=True)
    srv = Server(a.strata, a.key)
    sources = []
    for name, fn, d in (("fsd50k", fsd50k, a.fsd50k), ("commons", commons, a.commons), ("fma", fma, a.fma)):
        if d:
            rows = list(fn(d))
            if a.limit:
                rows = rows[:a.limit]
            log(f"{name}: {len(rows)} sounds")
            sources += rows
    partial = os.path.join(out, "sounds.partial.jsonl")
    done = set()
    if os.path.exists(partial):
        for l in open(partial):
            done.add(json.loads(l)["id"])
    log(f"{len(done)} sounds done before, {len(sources) - len(done)} to go")
    t0 = time.time()
    n_done = 0
    n_windows = 0
    pool = cf.ThreadPoolExecutor(a.concurrency)
    with open(partial, "a") as sp, open(os.path.join(out, "au_moments.jsonl"), "a") as mp:
        for rec in sources:
            if rec["id"] in done:
                continue
            try:
                x = decode(rec["path"])
            except Exception as e:
                log("cannot read", rec["path"], e)
                continue
            seconds = len(x) / RATE
            if seconds < 0.3:
                log("too short", rec["path"])
                continue
            fname = re.sub(r"[^A-Za-z0-9_-]", "_", rec["id"].replace(":", "_")) + ".mp3"
            try:
                preview(rec["path"], os.path.join(out, "audio", fname), rec.get("stereo", False), seconds)
            except Exception as e:
                log("cannot make a preview of", rec["path"], e)
                continue
            ws = windows(x)
            try:
                vecs = list(pool.map(lambda w: srv.embed(wav_b64(w[1])), ws))
            except Exception as e:
                log("cannot embed", rec["id"], e)
                continue
            base = [v for (t, _), v in zip(ws, vecs) if t % 10 == 0]
            emb = np.mean(base, axis=0)
            emb = emb / max(np.linalg.norm(emb), 1e-9)
            for (t, _), v in zip(ws, vecs):
                mp.write(json.dumps({"id": f"{rec['id']}@{t}", "sound": rec["id"], "t": float(t), "seconds": round(seconds, 2), "emb": [round(float(f), 6) for f in v]}) + "\n")
            mp.flush()
            row = dict(rec)
            row.pop("path", None)
            row.update(seconds=round(seconds, 2), audio=fname, peaks=",".join(map(str, peaks(x))), moments=len(ws), emb=[round(float(f), 6) for f in emb])
            sp.write(json.dumps(row) + "\n")
            sp.flush()
            n_done += 1
            n_windows += len(ws)
            if n_done % 25 == 0:
                s = time.time() - t0
                log(f"{n_done} sounds, {n_windows} moments in {s:.0f} s ({n_windows / s:.2f} moments a second)")
    log(f"embedded {n_done} sounds ({n_windows} moments) in {time.time() - t0:.0f} s")
    # A run stopped between a sound's moments and its row wrote its moments
    # twice: keep each moment once, and only those of sounds with a row.
    have = set()
    for l in open(partial):
        have.add(json.loads(l)["id"])
    mpath = os.path.join(out, "au_moments.jsonl")
    kept = {}
    for l in open(mpath):
        m = json.loads(l)
        if m["sound"] in have:
            kept[m["id"]] = l
    with open(mpath + ".tmp", "w") as f:
        f.writelines(kept.values())
    os.replace(mpath + ".tmp", mpath)

    # Every sound once, with its place on the map and, when its source gave
    # none, the labels CLAP hears in it.
    rows = {}
    for l in open(partial):
        r = json.loads(l)
        rows[r["id"]] = r
    rows = list(rows.values())
    E = np.array([r["emb"] for r in rows], dtype=np.float32)
    labels = srv.labels()
    L = np.array([v for _, v in labels], dtype=np.float32)
    ids = [i for i, _ in labels]
    S = E @ L.T
    log(f"laying out {len(rows)} sounds on the map")
    Y = tsne(E.astype(np.float64)) if len(rows) > 50 else np.random.default_rng(1).random((len(rows), 2))
    with open(os.path.join(out, "au_sounds.jsonl"), "w") as f:
        for k, r in enumerate(rows):
            heard = [ids[j] for j in np.argsort(-S[k])[:3]]
            labels_ = r.get("labels") or heard
            words = " ".join(filter(None, [r["title"], r.get("description", ""), " ".join(r.get("tags", [])), " ".join(r.get("label_words", [])), " ".join(r.get("genres", []))]))
            f.write(json.dumps({
                "id": r["id"], "title": r["title"][:300], "words": words[:6000], "author": r.get("author", "")[:200], "author_url": r.get("author_url", ""),
                "page": r["page"], "source": r["source"], "licence": r["licence"], "licence_url": r.get("licence_url", ""), "lic": lic_code(r["licence"]),
                "kind": r["kind"], "tags": [t[:80] for t in r.get("tags", [])][:30], "labels": labels_, "genres": r.get("genres", []),
                "seconds": r["seconds"], "year": r.get("year"), "lat": r.get("lat"), "lon": r.get("lon"),
                "audio": r["audio"], "peaks": r["peaks"], "x": round(float(Y[k, 0]), 5), "y": round(float(Y[k, 1]), 5), "moments": r["moments"],
                "emb": r["emb"],
            }) + "\n")
    log(f"wrote {len(rows)} sounds to {os.path.join(out, 'au_sounds.jsonl')}")


if __name__ == "__main__":
    main()
