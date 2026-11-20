"""Trends: how much Parley talks about things, hour by hour or day by day.

Each term is one `GROUP BY date_bin(...)` over the posts and comments that
match it (the same BM25 match as search, typos forgiven), divided by all of
them in the same bucket, so a term's share does not just follow how busy the
site was. What is rising compares the last few hours' words with the day
before them. What gets posted is the same grouping by kind of post.
"""
from __future__ import annotations

import math
import re
import time
from collections import Counter

from .service import ParleyError, _epoch, _iso

WINDOWS = {"hour": ("1 hour", 3600, 48), "day": ("1 day", 86400, 30)}
# For "auto": the narrowest bucket that fits the site's life in 48 of them.
WIDTHS = [(300, "5 minutes"), (900, "15 minutes"), (1800, "30 minutes"), (3600, "1 hour"), (6 * 3600, "6 hours"), (86400, "1 day")]
STOP = set("""a an the and or but if of to in on at by for with from as is are was were be been being it its this that these those
i me my we our you your he she they them their his her not no so do does did done have has had having can could would should will
just very really also than then there here what which who whom whose when where why how all any each every some more most other
into over under about after before again out up down off only own same too s t don now one two new like get got make made
let lets still even much many well way back yes ok thanks thank re via""".split())
WORD = re.compile(r"[a-z][a-z0-9'-]{2,}")


def _buckets(width_s: int, n: int, now: float) -> list[float]:
    end = math.floor(now / width_s) * width_s
    return [end - (n - 1 - i) * width_s for i in range(n)]


def _count(app, table: str, field: str, since: str, width: str, term: str | None, kind: bool = False) -> list[dict]:
    where = "ts >= :s" + (" AND mod = 'ok'" if table == "posts" else "")
    params = {"s": since}
    if term:
        where += f" AND MATCH({field}, :t, fuzzy => 'auto')"
        params["t"] = term
    group = "b, kind" if kind else "b"
    return app.s.q(f"SELECT date_bin('{width}', ts) AS b{', kind' if kind else ''}, COUNT(*) AS n FROM {table} WHERE {where} GROUP BY {group}",
                   params, label=f"trends:{table}{':' + term if term else ''}{':kind' if kind else ''}")


def trends(app, terms: list[str], by: str = "auto") -> dict:
    if by not in WINDOWS and by != "auto":
        raise ParleyError(400, "by is auto, hour or day")
    terms = [t.strip() for t in terms if t.strip()][:6]
    now = time.time()
    if by == "auto":
        first = app.s.one("SELECT ts FROM posts ORDER BY ts ASC LIMIT 1", label="trends:first")
        span = max(300.0, now - _epoch(first["ts"])) if first else 3600.0
        width_s, width = next(((w, name) for w, name in WIDTHS if span / w <= 47), WIDTHS[-1])
        n = min(48, max(6, math.ceil(span / width_s) + 1))
    else:
        width, width_s, n = WINDOWS[by]
    starts = _buckets(width_s, n, now)
    since = _iso(starts[0])
    t0 = time.perf_counter()

    def series(rows: list[dict]) -> list[int]:
        got = {}
        for r in rows:
            got[int(_epoch(r["b"]) // width_s)] = got.get(int(_epoch(r["b"]) // width_s), 0) + int(r["n"])
        return [got.get(int(s // width_s), 0) for s in starts]

    totals = [a + b for a, b in zip(series(_count(app, "posts", "body", since, width, None)),
                                    series(_count(app, "comments", "body", since, width, None)))]
    out_terms = []
    for t in terms:
        p = series(_count(app, "posts", "body", since, width, t))
        c = series(_count(app, "comments", "body", since, width, t))
        counts = [a + b for a, b in zip(p, c)]
        out_terms.append({"term": t, "counts": counts, "posts": sum(p), "comments": sum(c),
                          "share": [round(x / tot, 4) if tot else 0.0 for x, tot in zip(counts, totals)]})
    kinds: dict[str, list[int]] = {}
    for r in _count(app, "posts", "body", since, width, None, kind=True):
        k = r.get("kind") or "text"
        kinds.setdefault(k, [0] * n)
        i = int(_epoch(r["b"]) // width_s)
        idx = {int(s // width_s): j for j, s in enumerate(starts)}.get(i)
        if idx is not None:
            kinds[k][idx] += int(r["n"])
    span_h = (now - starts[0]) / 3600
    recent = min(6.0, max(0.5, span_h / 3))
    return {"by": by, "width": width, "width_s": width_s, "buckets": [_iso(s) for s in starts], "totals": totals, "terms": out_terms,
            "kinds": kinds, "rising": rising(app, recent, min(42.0, max(0.5, span_h - recent))), "recent_hours": round(recent, 2),
            "took_ms": round((time.perf_counter() - t0) * 1000, 1)}


def rising(app, recent_hours: float = 6, before_hours: float = 42, top: int = 16) -> list[dict]:
    """Words members use more in the last hours than in the hours before,
    by the ratio of their rates (smoothed), with at least a few uses."""
    now = time.time()
    since = _iso(now - (recent_hours + before_hours) * 3600)
    cut = now - recent_hours * 3600
    rows = app.s.q("SELECT body, ts FROM posts WHERE ts >= :s AND mod = 'ok' ORDER BY ts DESC LIMIT 5000", {"s": since}, label="trends:rising:posts")
    rows += app.s.q("SELECT body, ts FROM comments WHERE ts >= :s ORDER BY ts DESC LIMIT 5000", {"s": since}, label="trends:rising:comments")
    new, old = Counter(), Counter()
    n_new = n_old = 0
    for r in rows:
        words = {w.strip("'-") for w in WORD.findall((r.get("body") or "").lower())}
        words = {w for w in words if len(w) > 2 and w not in STOP and not w.isdigit()}
        if _epoch(r["ts"]) >= cut:
            new.update(words)
            n_new += 1
        else:
            old.update(words)
            n_old += 1
    out = []
    for w, k in new.items():
        if k < 3:
            continue
        if old.get(w, 0) * max(1, n_new) >= k * max(1, n_old):
            continue  # not used more than before
        rate_new = (k + 0.5) / max(1, n_new)
        rate_old = (old.get(w, 0) + 0.5) / max(1, n_old)
        if rate_new / rate_old < 1.2:
            continue
        out.append({"term": w, "recent": k, "before": old.get(w, 0), "ratio": round(rate_new / rate_old, 2)})
    out.sort(key=lambda x: (-x["ratio"], -x["recent"]))
    return out[:top]
