"""Helpers shared by the laptop benchmark's Part A scripts: timing, the
machine, BEIR collections as prepare_beir.py writes them, and reports.

Every number a script writes names the machine it was measured on
(`machine()`), so a result is never mistaken for another machine's.
"""

from __future__ import annotations

import json
import math
import os
import platform
import re
import subprocess
import sys
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable, Iterable, Sequence

import numpy as np


class Unsupported(Exception):
    """A kind the system cannot answer; the message says why."""


def percentile(xs: Sequence[float], p: float) -> float | None:
    if not xs:
        return None
    s = sorted(xs)
    k = (len(s) - 1) * p / 100.0
    lo, hi = math.floor(k), math.ceil(k)
    return s[lo] if lo == hi else s[lo] + (s[hi] - s[lo]) * (k - lo)


def summary(ms: Sequence[float], seconds: float | None = None) -> dict:
    out = {
        "n": len(ms),
        "p50_ms": percentile(ms, 50),
        "p90_ms": percentile(ms, 90),
        "p99_ms": percentile(ms, 99),
        "mean_ms": sum(ms) / len(ms) if ms else None,
    }
    if seconds:
        out["qps"] = len(ms) / seconds
    return out


PASSES = 3  # timed passes per cell: their spread says which gaps are real


def timed_passes(fn: Callable[[Any], Any], items: Sequence[Any], seconds: float, threads: int = 1, passes: int | None = None) -> dict:
    """Runs `fn(item)` over `items` round-robin from `threads` threads in
    `passes` (PASSES) timed passes of `seconds / passes` each (each thread at
    least one pass over its share). The summary is over every pass, with
    each pass's p50, p99 and rate and their ranges (`p50_ms_range`, ...)."""
    passes = max(1, passes or PASSES)
    runs = [_timed(fn, items, seconds / passes, threads) for _ in range(passes)]
    lat = [x for r in runs for x in r["lat"]]
    out = summary(lat, sum(r["wall"] for r in runs))
    out["threads"] = threads
    errors = [e for r in runs for e in r["errors"]]
    out["errors"] = len(errors)
    if errors:
        out["first_error"] = errors[0]
    if passes > 1:
        each = [summary(r["lat"], r["wall"]) for r in runs]
        out["passes"] = [{k: e[k] for k in ("p50_ms", "p99_ms", "qps")} for e in each]
        for k in ("p50_ms", "p99_ms", "qps"):
            vs = [e[k] for e in each if e.get(k) is not None]
            out[f"{k}_range"] = [min(vs), max(vs)] if vs else None
    return out


def _timed(fn: Callable[[Any], Any], items: Sequence[Any], seconds: float, threads: int) -> dict:
    """One pass of [`timed_passes`]: latencies in ms, wall seconds, errors."""
    lat: list[list[float]] = [[] for _ in range(threads)]
    errors: list[str] = []
    deadline = time.perf_counter() + seconds

    def worker(t: int) -> None:
        i = t
        n = len(items)
        done = 0
        while True:
            if done >= max(1, n // threads) and time.perf_counter() >= deadline:
                return
            it = items[i % n]
            t0 = time.perf_counter()
            try:
                fn(it)
            except Exception as e:  # noqa: BLE001 - counted and reported
                errors.append(repr(e)[:300])
            lat[t].append((time.perf_counter() - t0) * 1e3)
            i += threads
            done += 1

    t0 = time.perf_counter()
    if threads == 1:
        worker(0)
    else:
        ts = [threading.Thread(target=worker, args=(t,)) for t in range(threads)]
        for t in ts:
            t.start()
        for t in ts:
            t.join()
    return {"lat": [x for l in lat for x in l], "wall": time.perf_counter() - t0, "errors": errors}


def timed_once(fn: Callable[[Any], Any], items: Sequence[Any], seconds: float) -> dict:
    """Runs `fn(item)` on each item in order, once, until the items or
    `seconds` run out (for writes that cannot repeat: deletes, new keys)."""
    lat: list[float] = []
    errors: list[str] = []
    t0 = time.perf_counter()
    for it in items:
        if lat and time.perf_counter() - t0 >= seconds:
            break
        t1 = time.perf_counter()
        try:
            fn(it)
        except Exception as e:  # noqa: BLE001 - counted and reported
            errors.append(repr(e)[:300])
        lat.append((time.perf_counter() - t1) * 1e3)
    out = summary(lat, time.perf_counter() - t0)
    out["errors"] = len(errors)
    if errors:
        out["first_error"] = errors[0]
    return out


def machine() -> dict:
    """The machine a number was measured on."""
    info = {"os": platform.platform(), "python": platform.python_version(), "cpus": os.cpu_count()}
    try:
        if sys.platform == "darwin":
            q = lambda k: subprocess.run(["sysctl", "-n", k], capture_output=True, text=True).stdout.strip()  # noqa: E731
            info.update(host=q("kern.hostname"), model=q("hw.model"), cpu=q("machdep.cpu.brand_string"), memory_bytes=int(q("hw.memsize") or 0))
        else:
            info["host"] = platform.node()
            for line in open("/proc/cpuinfo"):
                if line.startswith("model name"):
                    info["cpu"] = line.split(":", 1)[1].strip()
                    break
            for line in open("/proc/meminfo"):
                if line.startswith("MemTotal"):
                    info["memory_bytes"] = int(line.split()[1]) * 1024
                    break
    except Exception:  # noqa: BLE001 - best effort
        pass
    # On a cloud node (do_part_a.sh sets these): the size it was rented as.
    for env, key in (("NODE_SIZE", "node_size"), ("NODE_REGION", "node_region")):
        if os.environ.get(env):
            info[key] = os.environ[env]
    return info


def rss_bytes() -> int:
    """This process's peak resident memory."""
    import resource

    r = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    return r if sys.platform == "darwin" else r * 1024


def rss_now_bytes() -> int | None:
    """This process's resident memory now (Linux; None elsewhere)."""
    try:
        with open("/proc/self/statm") as f:
            return int(f.read().split()[1]) * os.sysconf("SC_PAGE_SIZE")
    except (OSError, ValueError):
        return None


def memory(base: int | None = None) -> dict:
    """Resident memory now and at its peak, in MB, and now less `base` (the
    process before the system was built: the collection's own arrays)."""
    now = rss_now_bytes()
    out = {"rss_mb": None if now is None else now / 1e6, "peak_rss_mb": rss_bytes() / 1e6}
    if now is not None and base is not None:
        out["rss_over_base_mb"] = (now - base) / 1e6
    return out


def evict(paths: Iterable[str | os.PathLike]) -> bool:
    """Drops the files under `paths` from the page cache (Linux, files no
    process maps), so that the next reads come from the disk: a cold start
    without root. False where it cannot."""
    if not hasattr(os, "posix_fadvise"):
        return False
    done = False
    for p in paths:
        p = Path(p)
        files = [p] if p.is_file() else [Path(r) / f for r, _, fs in os.walk(p) for f in fs]
        for f in files:
            try:
                fd = os.open(f, os.O_RDONLY)
            except OSError:
                continue
            try:
                os.fsync(fd)
                os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)
                done = True
            except OSError:
                pass
            finally:
                os.close(fd)
    return done


def path_bytes(p: str | os.PathLike) -> int:
    p = Path(p)
    if p.is_file():
        return p.stat().st_size
    total = 0
    for root, _, files in os.walk(p):
        for f in files:
            try:
                total += (Path(root) / f).stat().st_size
            except OSError:
                pass
    return total


def files_bytes(paths: Iterable[str | os.PathLike]) -> int:
    return sum(path_bytes(p) for p in paths if Path(p).exists())


def write_report(path: str | os.PathLike, report: dict) -> None:
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    report = {"machine": machine(), "written": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), **report}
    path.write_text(json.dumps(report, indent=1, default=str))
    print(f"wrote {path}", file=sys.stderr)


@dataclass
class Collection:
    """A BEIR collection as chloedb/scripts/prepare_beir.py writes it."""

    name: str
    dir: Path
    docs: list[dict]
    vecs: np.ndarray
    queries: list[dict]
    qvecs: np.ndarray
    qrels: dict[str, dict[str, int]]
    dim: int = field(init=False)

    def __post_init__(self) -> None:
        self.dim = int(self.vecs.shape[1])

    @staticmethod
    def load(data: str | os.PathLike, name: str, limit: int | None = None) -> "Collection":
        d = Path(data) / name
        meta = json.loads((d / "meta.json").read_text())
        n, dim = meta["docs"], meta["dim"]
        docs = []
        with open(d / "docs.jsonl") as f:
            for i, line in enumerate(f):
                if limit is not None and i >= limit:
                    break
                docs.append(json.loads(line))
        vecs = np.memmap(d / "doc_vecs.f32", dtype=np.float32, mode="r", shape=(n, dim))[: len(docs)]
        queries = [json.loads(l) for l in open(d / "queries.jsonl")]
        qvecs = np.memmap(d / "query_vecs.f32", dtype=np.float32, mode="r", shape=(meta["queries"], dim))
        qrels: dict[str, dict[str, int]] = {}
        for line in open(d / "qrels.tsv"):
            parts = line.rstrip("\n").split("\t")
            if len(parts) >= 3 and parts[2].strip().isdigit() and int(parts[2]) > 0:
                qrels.setdefault(parts[0], {})[parts[1]] = int(parts[2])
        return Collection(name, d, docs, np.asarray(vecs), queries, np.asarray(qvecs), qrels)


def body(doc: dict) -> str:
    """The searched text, as chloedb::data makes it: the title, then the text."""
    t, x = doc.get("title") or "", doc.get("text") or ""
    return f"{t} {x}".strip() if t else x


def ndcg10(keys: Sequence[str], rel: dict[str, int]) -> float | None:
    if not rel:
        return None
    # Linear gains, as BEIR's evaluation (pytrec_eval) has them.
    dcg = sum(rel.get(k, 0) / math.log2(i + 2) for i, k in enumerate(keys[:10]))
    ideal = sorted(rel.values(), reverse=True)[:10]
    idcg = sum(g / math.log2(i + 2) for i, g in enumerate(ideal))
    return dcg / idcg if idcg else None


# Lucene's classic English stopwords, as Strata's analyzer drops them.
CLASSIC_STOPWORDS = set(
    "a an and are as at be but by for if in into is it no not of on or such that the their then there these they this to was will with".split()
)
WORD = re.compile(r"[^\W_]+", re.UNICODE)


def words(text: str) -> list[str]:
    return [w.lower() for w in WORD.findall(text)]


def strata_stopwords() -> set[str]:
    """The words Strata's ranked queries drop (QUERY_STOPWORDS, NLTK's
    English list), read from chloedb/src/text/analyzer.rs in this checkout."""
    src = Path(__file__).resolve().parents[3] / "chloedb" / "src" / "text" / "analyzer.rs"
    m = re.search(r"pub const QUERY_STOPWORDS: \[&str; (\d+)\] = \[(.*?)\];", src.read_text(), re.S)
    if m is None:
        raise SystemExit(f"no QUERY_STOPWORDS in {src}")
    ws = re.findall(r'"([^"]+)"', m.group(2))
    assert len(ws) == int(m.group(1)), (len(ws), m.group(1))
    return set(ws)


# The words kept() drops from a query for the systems whose queries the app
# builds (Tantivy, SQLite): Lucene's classic 33 unless use_stopwords("strata")
# makes them Strata's own, so that speed and nDCG compare the same queries.
QUERY_STOPWORDS: set[str] = CLASSIC_STOPWORDS


def use_stopwords(which: str) -> None:
    global QUERY_STOPWORDS
    QUERY_STOPWORDS = {"classic": CLASSIC_STOPWORDS, "strata": strata_stopwords()}[which]


def kept(text: str) -> list[str]:
    return [w for w in words(text) if w not in QUERY_STOPWORDS and len(w) > 1]


def phrase_of(text: str) -> tuple[str, str] | None:
    """Two words of the query in a row, neither a stopword, the first such pair."""
    ws = words(text)
    for a, b in zip(ws, ws[1:]):
        if a not in CLASSIC_STOPWORDS and b not in CLASSIC_STOPWORDS and len(a) > 2 and len(b) > 2 and a.isalpha() and b.isalpha():
            return a, b
    return None


def typo_of(text: str) -> str | None:
    """The query with the second and third letters of its longest word (5 letters or more) swapped."""
    ws = WORD.findall(text)
    best = max((w for w in ws if w.isalpha() and len(w) >= 5), key=len, default=None)
    if best is None or best[1] == best[2]:
        return None
    swapped = best[0] + best[2] + best[1] + best[3:]
    return re.sub(rf"\b{re.escape(best)}\b", swapped, text, count=1)


def typo_word_of(text: str) -> tuple[str, str] | None:
    """The query's longest word (5 letters or more) alone, and with its second and third letters swapped."""
    ws = WORD.findall(text)
    best = max((w for w in ws if w.isalpha() and len(w) >= 5), key=len, default=None)
    if best is None or best[1] == best[2]:
        return None
    return best.lower(), (best[0] + best[2] + best[1] + best[3:]).lower()


def prefix_of(text: str) -> tuple[str, str] | None:
    """The words before the query's last word, and that word's first three letters (as one types it)."""
    ws = kept(text)
    if len(ws) < 2 or not ws[-1].isalpha() or len(ws[-1]) < 4:
        return None
    return " ".join(ws[:-1]), ws[-1][:3]


def minmax(scores: dict[str, float]) -> dict[str, float]:
    if not scores:
        return {}
    lo, hi = min(scores.values()), max(scores.values())
    return {k: (v - lo) / (hi - lo) if hi > lo else 1.0 for k, v in scores.items()}


def rrf(*lists: Sequence[str], k: int = 60) -> list[str]:
    """Reciprocal rank fusion of ranked key lists (app code where a system has none)."""
    score: dict[str, float] = {}
    for l in lists:
        for i, key in enumerate(l):
            score[key] = score.get(key, 0.0) + 1.0 / (k + i + 1)
    return [key for key, _ in sorted(score.items(), key=lambda kv: (-kv[1], kv[0]))]


def weighted(lex: dict[str, float], vec: dict[str, float], w_lex: float = 0.2, w_vec: float = 0.8) -> list[str]:
    """0.2 * minmax(bm25) + 0.8 * minmax(similarity) over the union of the two top lists."""
    a, b = minmax(lex), minmax(vec)
    keys = set(a) | set(b)
    s = {key: w_lex * a.get(key, 0.0) + w_vec * b.get(key, 0.0) for key in keys}
    return [key for key, _ in sorted(s.items(), key=lambda kv: (-kv[1], kv[0]))]
