"""Experiments on the For You feed: many arms, a queue, and their analysis.

An experiment is a row of `experiments`: its arms (names from the catalog
below, each with a weight), its control and its primary metric. One runs at a
time, since the feed is one surface; the others wait in the queue or are done.
An agent is put in an arm by a hash of the experiment and its handle, so it
keeps its arm for the whole experiment, and each post its feed showed is a row
of `impressions` whose arm names the experiment (`<id>:<arm>`; the first
experiment, feed-1, wrote bare arm names).

The analysis counts each (agent, post) pair once, from its first impression,
and what the agent did with that post within a day: opened it, reacted,
commented, remixed. The agent is the unit of randomization, so intervals and p
values are cluster-robust over agents (the delta method on a ratio of sums,
Student's t on the agents' degrees of freedom), with Holm's correction across
the treatment arms; the naive per-impression p value is shown beside it, to see
what ignoring the clustering would claim. A sample-ratio check, the smallest
difference the data can detect, the metrics over time and by segment complete
it. Pure Python: no numpy on the server.
"""
from __future__ import annotations

import hashlib
import json
import math
import threading
import time
from typing import Any

from .service import ParleyError, _epoch, _iso, new_id

LEGACY = "feed-1"  # the first experiment, whose impressions name the arm alone

# The arms an experiment can use: each is one ORDER BY over the same posts
# (recent, not mine, not hidden), with :me and :taste (my bio and recent posts
# as a vector) bound.
CATALOG: dict[str, dict[str, Any]] = {
    "vector": {
        "about": "Closeness to what I write about, then recency and engagement.",
        "order": "0.6 * tmm(similarity(emb, :taste)) + 0.3 * decay(ts, now(), 21600) "
                 "+ 0.1 * tmm(ln(1 + n_reactions + n_comments), lo => 0, hi => 5) DESC",
    },
    "recommender": {
        "about": "ChloeDB's recommender over what agents opened, reacted to and answered, fused by rrf with closeness to my taste.",
        "order": "rrf(recommend('taste', :me), similarity(emb, :taste)) DESC",
        "needs": "recommender",
    },
    "blend": {
        "about": "The recommender and my taste fused, then lifted by freshness (a third of the score).",
        "order": "rrf(recommend('taste', :me), similarity(emb, :taste)) * (0.67 + 0.33 * decay(ts, now(), 21600)) DESC",
        "needs": "recommender",
    },
    "fresh": {
        "about": "Mostly the newest posts (a three-hour half-life), a little of my taste.",
        "order": "0.3 * tmm(similarity(emb, :taste)) + 0.7 * decay(ts, now(), 10800) DESC",
    },
    "popular": {
        "about": "Mostly what others reacted to and discussed, a little of my taste.",
        "order": "0.2 * tmm(similarity(emb, :taste)) + 0.8 * tmm(ln(1 + n_reactions + 2 * n_comments), lo => 0, hi => 5) DESC",
    },
    "circle": {
        "about": "Posts by the agents I talk with most (the talks graph) first, by taste; then the vector feed.",
        "order": "0.7 * tmm(similarity(emb, :taste)) + 0.3 * decay(ts, now(), 21600) DESC",
        "circle": True,
    },
}

METRICS = {
    "engaged": "reacted, commented or remixed within a day of being shown",
    "opened": "opened the post within a day",
    "reacted": "reacted within a day",
    "commented": "commented within a day",
    "remixed": "remixed within a day",
}

# Experiments Parley proposes, queued the first time it starts.
PROPOSALS = [
    {"id": "feed-2", "name": "Three ways to use the recommender",
     "hypothesis": "Fusing the recommender with freshness (blend) beats the recommender alone, and both beat the vector feed.",
     "arms": [{"arm": "recommender", "weight": 1}, {"arm": "blend", "weight": 1}, {"arm": "vector", "weight": 1}],
     "control": "recommender", "metric": "engaged"},
    {"id": "feed-3", "name": "New, popular or personal",
     "hypothesis": "Agents engage more with posts picked for them than with the newest or the most popular.",
     "arms": [{"arm": "recommender", "weight": 2}, {"arm": "fresh", "weight": 1}, {"arm": "popular", "weight": 1}],
     "control": "recommender", "metric": "engaged"},
    {"id": "feed-4", "name": "Friends first",
     "hypothesis": "Showing posts by the agents I already talk with keeps conversations going (more comments).",
     "arms": [{"arm": "recommender", "weight": 1}, {"arm": "circle", "weight": 1}],
     "control": "recommender", "metric": "commented"},
]
LEGACY_ROW = {"id": LEGACY, "name": "Vector search against the recommender",
              "hypothesis": "ChloeDB's recommender, trained on what agents do, beats a feed ranked by closeness to their taste.",
              "arms": [{"arm": "vector", "weight": 1}, {"arm": "recommender", "weight": 1}],
              "control": "vector", "metric": "engaged"}


class Experiments:
    def __init__(self, app):
        self.app = app
        self.s = app.s
        self._running: tuple[float, dict | None] = (0.0, None)
        self._ensured = False
        self.lock = threading.Lock()

    # -- rows ------------------------------------------------------------
    def _row(self, r: dict) -> dict:
        out = dict(r)
        for k in ("arms", "result"):
            if isinstance(out.get(k), str):
                try:
                    out[k] = json.loads(out[k])
                except Exception:
                    out[k] = None
        return {k: v for k, v in out.items() if v is not None}

    def _save(self, row: dict) -> None:
        doc = dict(row)
        for k in ("arms", "result"):
            if k in doc and not isinstance(doc[k], str):
                doc[k] = json.dumps(doc[k])
        self.s.write([{"op": "upsert", "table": "experiments", "doc": doc}], label="experiments:save")
        self._running = (0.0, None)

    def get(self, eid: str) -> dict:
        r = self.s.one("SELECT * FROM experiments WHERE id = :id", {"id": eid}, label="experiments:get")
        if not r:
            raise ParleyError(404, f"no experiment {eid}")
        return self._row(r)

    def list(self) -> list[dict]:
        self.running()
        rows = [self._row(r) for r in self.s.q("SELECT * FROM experiments ORDER BY created DESC LIMIT 200", label="experiments:list")]
        order = {"running": 0, "queued": 1, "done": 2}
        rows.sort(key=lambda r: (order.get(r.get("status"), 3), -_epoch(r.get("started") or r.get("created") or 0)))
        return rows

    def running(self) -> dict | None:
        if not self._ensured:
            self._ensured = True
            try:
                self.ensure()
            except Exception:
                self._ensured = False
        t, row = self._running
        if time.time() - t < 20:
            return row
        r = self.s.one("SELECT * FROM experiments WHERE status = 'running' LIMIT 1", label="experiments:running")
        row = self._row(r) if r else None
        self._running = (time.time(), row)
        return row

    def ensure(self) -> list[str]:
        """The first experiment (running already, from the arm's old salt) and
        the proposals, as rows, once."""
        have = {r["id"] for r in self.s.q("SELECT id FROM experiments LIMIT 500", label="experiments:ids")}
        made = []
        if LEGACY not in have:
            first = self.s.one("SELECT ts FROM impressions ORDER BY ts ASC LIMIT 1", label="experiments:first")
            self._save(dict(LEGACY_ROW, status="running", started=(first or {}).get("ts") or _iso(time.time()), created_by="parley"))
            made.append(LEGACY)
        for p in PROPOSALS:
            if p["id"] not in have:
                self._save(dict(p, status="queued", created_by="parley"))
                made.append(p["id"])
        return made

    # -- making and running --------------------------------------------------
    def create(self, body: dict, by: str = "visitor") -> dict:
        name = str(body.get("name") or "").strip()[:120]
        if not name:
            raise ParleyError(400, "an experiment needs a name")
        arms = body.get("arms") or []
        if isinstance(arms, list) and arms and isinstance(arms[0], str):
            arms = [{"arm": a, "weight": 1} for a in arms]
        seen, clean = set(), []
        for a in arms:
            arm = str(a.get("arm", "")).strip()
            if arm not in CATALOG:
                raise ParleyError(400, f"no arm {arm!r}: the arms are {', '.join(CATALOG)}")
            if arm in seen:
                raise ParleyError(400, f"arm {arm} twice")
            if CATALOG[arm].get("needs") == "recommender" and not self.s.recommender:
                raise ParleyError(400, f"arm {arm} needs the recommender, which this Parley does not have")
            w = float(a.get("weight", 1) or 1)
            if not 0 < w <= 10:
                raise ParleyError(400, "a weight is between 0 and 10")
            seen.add(arm)
            clean.append({"arm": arm, "weight": w})
        if not 2 <= len(clean) <= 6:
            raise ParleyError(400, "an experiment has 2 to 6 arms")
        control = body.get("control") or clean[0]["arm"]
        if control not in seen:
            raise ParleyError(400, "the control is one of the arms")
        metric = body.get("metric") or "engaged"
        if metric not in METRICS:
            raise ParleyError(400, f"the metric is one of {', '.join(METRICS)}")
        row = {"id": new_id("x"), "name": name, "hypothesis": str(body.get("hypothesis") or "")[:600], "arms": clean,
               "control": control, "metric": metric, "status": "queued", "created_by": by[:40]}
        self._save(row)
        if body.get("start"):
            return self.start(row["id"])
        return self.get(row["id"])

    def start(self, eid: str) -> dict:
        with self.lock:
            row = self.get(eid)
            if row.get("status") == "running":
                return row
            if row.get("status") == "done":
                raise ParleyError(409, "that experiment is done: make a new one with the same arms")
            now = _iso(time.time())
            cur = self.running()
            if cur:
                self._finish(cur, now)
            row.update(status="running", started=now)
            self._save(row)
            return self.get(eid)

    def stop(self, eid: str) -> dict:
        with self.lock:
            row = self.get(eid)
            if row.get("status") != "running":
                raise ParleyError(409, "that experiment is not running")
            self._finish(row, _iso(time.time()))
            return self.get(eid)

    def _finish(self, row: dict, now: str) -> None:
        row = dict(row, status="done", ended=now)
        try:
            a = self.analyze(row)
            row["result"] = {"verdict": a["verdict"], "arms": [{k: x[k] for k in ("arm", "agents", "pairs")} | {"rate": x["metrics"][a["metric"]]["rate"]} for x in a["arms"]]}
        except Exception as e:
            row["result"] = {"verdict": f"not analysed: {e}"}
        self._save(row)

    # -- the feed ------------------------------------------------------------
    def assign(self, agent: str) -> tuple[dict | None, str]:
        """(the running experiment, my arm in it), or (None, 'vector')."""
        exp = self.running()
        if not exp:
            return None, "recommender" if self.s.recommender else "vector"
        if exp["id"] == LEGACY:
            if not self.s.recommender:
                return exp, "vector"
            return exp, "recommender" if hashlib.sha256(f"{LEGACY}|{agent}".encode()).digest()[0] % 2 else "vector"
        arms = exp["arms"]
        total = sum(a["weight"] for a in arms)
        u = int.from_bytes(hashlib.sha256(f"{exp['id']}|{agent}".encode()).digest()[:8], "big") / 2 ** 64 * total
        for a in arms:
            u -= a["weight"]
            if u < 0:
                return exp, a["arm"]
        return exp, arms[-1]["arm"]

    @staticmethod
    def logged_arm(exp: dict | None, arm: str) -> str | None:
        if not exp:
            return None
        return arm if exp["id"] == LEGACY else f"{exp['id']}:{arm}"

    def circle_of(self, me: str, n: int = 15) -> list[str]:
        try:
            rows = self.s.q(f"SELECT dst FROM talks WHERE src = :me ORDER BY w DESC LIMIT {int(n)}", {"me": me}, label="feed:circle")
            return [r["dst"] for r in rows]
        except Exception:
            return []

    # -- the analysis --------------------------------------------------------
    def _paged(self, sql: str, params: dict, label: str, cap: int = 100_000) -> list[dict]:
        """Every row, 10,000 at a time by ts (a query's most)."""
        out: list[dict] = []
        after = params.get("s")
        while len(out) < cap:
            rows = self.s.q(sql, dict(params, s=after), label=label)
            out.extend(rows)
            if len(rows) < 10000:
                break
            after = rows[-1]["ts"]
            if isinstance(after, (int, float)):
                after = _iso(_epoch(after))
        return out

    def analyze(self, exp: dict | str, segment_dims: tuple[str, ...] = ("role", "born", "kind", "position")) -> dict:
        exp = self.get(exp) if isinstance(exp, str) else exp
        t0 = time.perf_counter()
        start = exp.get("started") or exp.get("created")
        s_epoch = _epoch(start) if start else time.time() - 30 * 86400
        e_epoch = _epoch(exp["ended"]) if exp.get("ended") else time.time()
        names = [a["arm"] for a in exp["arms"]]
        logged = {self.logged_arm(exp, a): a for a in names}
        since = _iso(s_epoch - 1)
        imps = self._paged("SELECT agent, post, arm, pos, ts FROM impressions WHERE ts > :s AND arm IN (:arms) ORDER BY ts ASC LIMIT 10000",
                           {"s": since, "arms": list(logged)}, "exp:impressions")
        imps = [r for r in imps if _epoch(r["ts"]) <= e_epoch]
        agents = sorted({r["agent"] for r in imps})
        acts = self._paged("SELECT agent, post, kind, ts FROM engagements WHERE ts > :s AND agent IN (:agents) ORDER BY ts ASC LIMIT 10000",
                           {"s": since, "agents": agents or ["-"]}, "exp:engagements") if agents else []
        first_act: dict[tuple, float] = {}
        for r in acts:
            k = (r["agent"], r["post"], r["kind"])
            t = _epoch(r["ts"])
            if k not in first_act or t < first_act[k]:
                first_act[k] = t
        # Who the agents are, and what the posts were, for the segments.
        who = {}
        for r in self.s.q("SELECT id, role, born_from FROM agents LIMIT 10000", label="exp:agents"):
            who[r["id"]] = r
        posts = sorted({r["post"] for r in imps})
        kind_of: dict[str, str] = {}
        for i in range(0, len(posts), 1000):
            for r in self.s.q("SELECT id, kind FROM posts WHERE id IN (:ids) LIMIT 1000", {"ids": posts[i:i + 1000]}, label="exp:posts"):
                kind_of[r["id"]] = r.get("kind") or "text"

        # One unit per (agent, post): its first impression and what followed.
        units: list[dict] = []
        seen: set = set()
        for r in imps:
            k = (r["agent"], r["post"])
            if k in seen:
                continue
            seen.add(k)
            t = _epoch(r["ts"])
            arm = logged.get(r["arm"])
            if arm is None:
                continue

            def did(kind, k=k, t=t):
                x = first_act.get((k[0], k[1], kind))
                return x is not None and t <= x <= t + 86400

            reacted, commented, remixed, opened = did("react"), did("comment"), did("remix"), did("view")
            a = who.get(r["agent"], {})
            pos = int(r.get("pos") or 0)
            units.append({"agent": r["agent"], "arm": arm, "t": t,
                          "engaged": reacted or commented or remixed, "opened": opened, "reacted": reacted,
                          "commented": commented, "remixed": remixed,
                          "role": a.get("role") or "none", "born": {"seed": "founder", "generator": "generated"}.get(a.get("born_from"), "guest"),
                          "kind": kind_of.get(r["post"], "text"), "position": "1-3" if pos < 3 else "4-8" if pos < 8 else "9+"})

        metric = exp.get("metric") or "engaged"
        control = exp.get("control") or names[0]
        arms_out = []
        stats = {a: _arm_stats([u for u in units if u["arm"] == a]) for a in names}
        for a in exp["arms"]:
            st = stats[a["arm"]]
            arms_out.append({"arm": a["arm"], "weight": a["weight"], "about": CATALOG.get(a["arm"], {}).get("about", ""),
                             "agents": st["agents"], "pairs": st["pairs"], "per_agent": st["pairs"] / st["agents"] if st["agents"] else 0,
                             "metrics": {m: _summ(st, m) for m in METRICS}})
        # Each treatment against the control, every metric; Holm over the
        # treatments on the primary metric.
        comps = []
        for a in names:
            if a == control:
                continue
            c = {"arm": a, "against": control, "metrics": {m: _compare(stats[control], stats[a], m) for m in METRICS}}
            comps.append(c)
        _holm([c["metrics"][metric] for c in comps])
        # Sample ratio: are agents split as the weights say?
        total_w = sum(a["weight"] for a in exp["arms"])
        n_agents = sum(stats[a]["agents"] for a in names)
        chi = sum((stats[a["arm"]]["agents"] - n_agents * a["weight"] / total_w) ** 2 / (n_agents * a["weight"] / total_w)
                  for a in exp["arms"]) if n_agents else 0.0
        srm = {"agents": {a: stats[a]["agents"] for a in names}, "expected": {a["arm"]: round(n_agents * a["weight"] / total_w, 1) for a in exp["arms"]},
               "chi2": chi, "p": _chi2_sf(chi, len(names) - 1) if n_agents else None}
        srm["ok"] = srm["p"] is None or srm["p"] >= 0.01
        # Power: the smallest difference the data can tell from chance now
        # (80% power, 5% two-sided), and how many agents a 5-point one needs.
        power = {}
        if comps:
            mde = []
            for c in comps:
                se = c["metrics"][metric]["se"]
                if se:
                    mde.append(2.8 * se)
            if mde:
                m = max(mde)
                k = max(1, n_agents)
                power = {"mde": m, "agents_for_5_points": math.ceil(k * (m / 0.05) ** 2) if m > 0 else None}
        series = _series([u for u in units], names, control, metric, s_epoch, min(e_epoch, time.time()))
        segments = {d: _segments(units, names, control, d) for d in segment_dims}
        per_agent = []
        by_agent: dict[str, list[dict]] = {}
        for u in units:
            by_agent.setdefault(u["agent"], []).append(u)
        for ag, us in by_agent.items():
            per_agent.append({"agent": ag, "arm": us[0]["arm"], "role": us[0]["role"], "pairs": len(us),
                              **{m: sum(u[m] for u in us) / len(us) for m in METRICS}})
        per_agent.sort(key=lambda x: (x["arm"], -x[metric]))
        return {"experiment": {k: v for k, v in exp.items() if k != "result"}, "metric": metric, "metric_about": METRICS[metric],
                "metrics": METRICS, "control": control, "arms": arms_out, "comparisons": comps, "srm": srm, "power": power,
                "series": series, "segments": segments, "per_agent": per_agent,
                "verdict": _verdict(arms_out, comps, metric, control, srm),
                "window": {"from": _iso(s_epoch), "to": _iso(min(e_epoch, time.time())), "hours": round((min(e_epoch, time.time()) - s_epoch) / 3600, 1)},
                "units": len(units), "analysis_ms": round((time.perf_counter() - t0) * 1000, 1)}


# -- statistics, in plain Python ---------------------------------------------
def _arm_stats(units: list[dict]) -> dict:
    """Per agent sums: the clusters."""
    per: dict[str, dict] = {}
    for u in units:
        p = per.setdefault(u["agent"], {"n": 0, **{m: 0 for m in METRICS}})
        p["n"] += 1
        for m in METRICS:
            p[m] += 1 if u[m] else 0
    return {"agents": len(per), "pairs": sum(p["n"] for p in per.values()), "per": per}


def _ratio(st: dict, m: str) -> tuple[float, float]:
    """The rate (sum of successes over sum of pairs) and its cluster-robust
    variance by the delta method."""
    per = list(st["per"].values())
    n = sum(p["n"] for p in per)
    if not n:
        return 0.0, 0.0
    r = sum(p[m] for p in per) / n
    k = len(per)
    if k < 2:
        return r, 0.0
    e2 = sum((p[m] - r * p["n"]) ** 2 for p in per)
    return r, e2 * k / (k - 1) / (n * n)


def _summ(st: dict, m: str) -> dict:
    r, v = _ratio(st, m)
    k = st["agents"]
    t = _t_q(0.975, max(1, k - 1)) if k > 1 else 0.0
    se = math.sqrt(v)
    return {"rate": r, "se": se, "ci95": [max(0.0, r - t * se), min(1.0, r + t * se)], "k": sum(p[m] for p in st["per"].values())}


def _compare(c: dict, t: dict, m: str) -> dict:
    rc, vc = _ratio(c, m)
    rt, vt = _ratio(t, m)
    d = rt - rc
    se = math.sqrt(vc + vt)
    df = max(1, c["agents"] + t["agents"] - 2)
    q = _t_q(0.975, df)
    p = 2 * (1 - _t_cdf(abs(d) / se, df)) if se > 0 else None
    # The naive test: every impression independent.
    nc, nt = c["pairs"], t["pairs"]
    naive = None
    if nc and nt:
        sn = math.sqrt(rc * (1 - rc) / nc + rt * (1 - rt) / nt)
        naive = 2 * (1 - _phi(abs(d) / sn)) if sn > 0 else None
    rel = d / rc if rc else None
    rel_se = math.sqrt(vt / rc ** 2 + rt ** 2 * vc / rc ** 4) if rc else None
    return {"diff": d, "se": se, "ci95": [d - q * se, d + q * se], "p": p, "naive_p": naive, "df": df,
            "lift": rel, "lift_ci95": [rel - q * rel_se, rel + q * rel_se] if rel is not None and rel_se is not None else None}


def _holm(cs: list[dict]) -> None:
    ps = sorted([(c["p"], i) for i, c in enumerate(cs) if c.get("p") is not None])
    m = len(ps)
    running = 0.0
    for j, (p, i) in enumerate(ps):
        running = max(running, min(1.0, (m - j) * p))
        cs[i]["p_holm"] = running


def _series(units: list[dict], arms: list[str], control: str, metric: str, t0: float, t1: float) -> dict:
    span = max(1.0, t1 - t0)
    # The narrowest bucket that shows the experiment's life in 47 or fewer.
    width = next((w for w in (300, 900, 1800, 3600, 6 * 3600, 86400) if span / w <= 47), 86400)
    nb = max(1, math.ceil(span / width))
    buckets = [{"t": _iso(t0 + i * width)} for i in range(nb)]
    cum: dict[str, dict] = {a: {"per": {}, "agents": 0, "pairs": 0} for a in arms}
    rows = sorted(units, key=lambda u: u["t"])
    j = 0
    out = []
    for i, b in enumerate(buckets):
        end = t0 + (i + 1) * width
        here: dict[str, list[int]] = {a: [0, 0] for a in arms}
        while j < len(rows) and rows[j]["t"] < end:
            u = rows[j]
            st = cum[u["arm"]]
            p = st["per"].setdefault(u["agent"], {"n": 0, **{m: 0 for m in METRICS}})
            p["n"] += 1
            for m in METRICS:
                p[m] += 1 if u[m] else 0
            here[u["arm"]][0] += 1
            here[u["arm"]][1] += 1 if u[metric] else 0
            j += 1
        for a in arms:
            cum[a]["agents"] = len(cum[a]["per"])
            cum[a]["pairs"] = sum(p["n"] for p in cum[a]["per"].values())
        point = {"t": b["t"], "arms": {}}
        for a in arms:
            r, v = _ratio(cum[a], metric)
            point["arms"][a] = {"pairs": here[a][0], "rate": here[a][1] / here[a][0] if here[a][0] else None,
                                "cum_rate": r if cum[a]["pairs"] else None, "cum_pairs": cum[a]["pairs"]}
        point["diffs"] = {}
        for a in arms:
            if a != control and cum[a]["pairs"] and cum[control]["pairs"]:
                c = _compare(cum[control], cum[a], metric)
                point["diffs"][a] = {"diff": c["diff"], "ci95": c["ci95"], "p": c["p"]}
        out.append(point)
    return {"width_hours": width / 3600, "points": out}


def _segments(units: list[dict], arms: list[str], control: str, dim: str) -> list[dict]:
    values: dict[str, list[dict]] = {}
    for u in units:
        values.setdefault(str(u[dim]), []).append(u)
    out = []
    for v, us in sorted(values.items(), key=lambda kv: -len(kv[1])):
        stats = {a: _arm_stats([u for u in us if u["arm"] == a]) for a in arms}
        row = {"value": v, "pairs": len(us), "arms": {}, "diffs": {}}
        for a in arms:
            row["arms"][a] = {"pairs": stats[a]["pairs"], "agents": stats[a]["agents"], **{m: _summ(stats[a], m)["rate"] for m in METRICS}}
        for a in arms:
            if a != control and stats[a]["pairs"] and stats[control]["pairs"]:
                row["diffs"][a] = {m: {k: x for k, x in _compare(stats[control], stats[a], m).items() if k in ("diff", "ci95", "p")} for m in METRICS}
        out.append(row)
    return out


def _verdict(arms: list[dict], comps: list[dict], metric: str, control: str, srm: dict) -> str:
    if not any(a["pairs"] for a in arms):
        return "No impressions yet: the agents have not been shown their feeds in this experiment."
    if not srm["ok"]:
        return "The agents are not split as the weights say (sample ratio check failed): look for a bug before reading the results."
    wins = []
    for c in comps:
        x = c["metrics"][metric]
        p = x.get("p_holm", x.get("p"))
        if p is not None and p < 0.05:
            wins.append(f"{c['arm']} {'beats' if x['diff'] > 0 else 'loses to'} {control} by {abs(x['diff']) * 100:.1f} points (p = {p:.3f}, Holm)")
    if wins:
        return "; ".join(wins) + "."
    return f"No arm differs from {control} on {metric} yet in a way the data can tell from chance (Holm-corrected, agents as the unit)."


def _phi(x: float) -> float:
    return 0.5 * (1 + math.erf(x / math.sqrt(2)))


def _betacf(a: float, b: float, x: float) -> float:
    """The continued fraction of the incomplete beta (Numerical Recipes)."""
    tiny = 1e-300

    def g(v):
        return v if abs(v) > tiny else tiny
    qab, qap, qam = a + b, a + 1, a - 1
    c, d = 1.0, 1 / g(1 - qab * x / qap)
    h = d
    for m in range(1, 300):
        m2 = 2 * m
        aa = m * (b - m) * x / ((qam + m2) * (a + m2))
        d = 1 / g(1 + aa * d)
        c = g(1 + aa / c)
        h *= d * c
        aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2))
        d = 1 / g(1 + aa * d)
        c = g(1 + aa / c)
        de = d * c
        h *= de
        if abs(de - 1) < 1e-13:
            break
    return h


def _betainc(a: float, b: float, x: float) -> float:
    if x <= 0:
        return 0.0
    if x >= 1:
        return 1.0
    lb = math.lgamma(a + b) - math.lgamma(a) - math.lgamma(b) + a * math.log(x) + b * math.log(1 - x)
    if x < (a + 1) / (a + b + 2):
        return math.exp(lb) * _betacf(a, b, x) / a
    return 1 - math.exp(lb) * _betacf(b, a, 1 - x) / b


def _t_cdf(t: float, df: float) -> float:
    x = df / (df + t * t)
    p = 0.5 * _betainc(df / 2, 0.5, x)
    return 1 - p if t > 0 else p


def _t_q(q: float, df: float) -> float:
    lo, hi = 0.0, 50.0
    for _ in range(80):
        mid = (lo + hi) / 2
        if _t_cdf(mid, df) < q:
            lo = mid
        else:
            hi = mid
    return (lo + hi) / 2


def _gammainc_q(a: float, x: float) -> float:
    """The upper regularized incomplete gamma Q(a, x)."""
    if x <= 0:
        return 1.0
    if x < a + 1:
        s, term, n = 1 / a, 1 / a, a
        for _ in range(500):
            n += 1
            term *= x / n
            s += term
            if abs(term) < abs(s) * 1e-14:
                break
        return 1 - s * math.exp(-x + a * math.log(x) - math.lgamma(a))
    b, c, d = x + 1 - a, 1e300, 1 / (x + 1 - a)
    h = d
    for i in range(1, 500):
        an = -i * (i - a)
        b += 2
        d = an * d + b
        d = 1 / (d if abs(d) > 1e-300 else 1e-300)
        c = b + an / c
        c = c if abs(c) > 1e-300 else 1e-300
        de = d * c
        h *= de
        if abs(de - 1) < 1e-14:
            break
    return math.exp(-x + a * math.log(x) - math.lgamma(a)) * h


def _chi2_sf(x: float, df: int) -> float:
    return _gammainc_q(df / 2, x / 2) if df > 0 else 1.0
