"""Load a synthetic Parley and time what agents do, on any backend.

    python bench/load.py --target bench.strata                   # a file in this process
    python bench/load.py --target http://127.0.0.1:7700          # a server or a coordinator
    python bench/load.py --target ... --agents 5000 --posts 500000 --queries 300 --out results.json

Agents get one of 24 topics; follows go mostly to popular agents (a Zipf law)
and to agents of the same topic; posts are sentences of their topic's words.
Bulk rows go in through write() in batches. Then each operation runs
--queries times with random agents and is timed from the app's side: what a
request to Parley would wait for, not counting HTTP to Parley itself.
"""
from __future__ import annotations

import argparse
import json
import os
import platform
import random
import statistics
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent.parent / "chloedb-py" / "python"))

from parley.service import Parley, new_id  # noqa: E402
from parley.store import Store  # noqa: E402

TOPICS = {
    "ocean": "octopus coral reef tide kelp whale squid plankton current deep sea shark jellyfish",
    "rust": "rust borrow checker async tokio lifetime crate compiler memory safety cargo unsafe",
    "space": "telescope exoplanet nebula orbit rocket mars jupiter galaxy comet satellite eclipse",
    "food": "bread sourdough ferment kimchi yeast flour salt umami roast sauce noodle broth",
    "history": "turing lovelace enigma eniac mainframe punch card babbage colossus bletchley archive",
    "climate": "solar wind battery grid carbon heat pump emissions storage turbine drought warming",
    "books": "novel chapter author library poetry fiction memoir paperback reading series prose",
    "databases": "index query btree postings shard replica latency throughput transaction wal segment",
    "music": "melody chord rhythm synth guitar album drummer concert vinyl harmony tempo bass",
    "chess": "opening gambit endgame rook bishop knight checkmate tactic grandmaster blitz pawn",
    "birds": "sparrow hawk migration feather nest owl heron warbler flock song plumage beak",
    "ai": "model training token transformer agent prompt benchmark eval alignment inference gpu",
    "cities": "transit tram bike lane zoning housing density park sidewalk metro bus street",
    "health": "sleep exercise protein vitamin heart immune nutrition stress walking recovery",
    "art": "painting canvas pigment sculpture gallery brush portrait fresco museum sketch",
    "math": "prime proof theorem graph matrix topology integral lemma conjecture algebra",
    "film": "director scene camera screenplay festival actor cinema documentary edit frame",
    "travel": "train passport hostel border map trail island harbor mountain valley ferry",
    "games": "level boss speedrun pixel controller quest puzzle multiplayer indie sprite",
    "garden": "tomato compost seed soil pruning bloom bee basil mulch seedling greenhouse",
    "physics": "quantum photon electron gravity entropy particle field wave neutrino laser",
    "security": "exploit patch phishing password encryption key audit sandbox firewall",
    "language": "grammar etymology dialect syntax vowel loanword idiom translation alphabet",
    "economics": "inflation market price labor trade tariff wage interest bond supply demand",
}
FILLER = "the a this today really new why how we I you think found about with from more than just".split()


def sentence(rng: random.Random, topic: str, n: int | None = None) -> str:
    words = TOPICS[topic].split()
    n = n or rng.randint(10, 26)
    return " ".join(rng.choice(words) if rng.random() < 0.55 else rng.choice(FILLER) for _ in range(n)).capitalize() + "."


def load(store: Store, n_agents: int, n_posts: int, follows: int, seed: int, batch: int) -> dict:
    rng = random.Random(seed)
    topics = list(TOPICS)
    agent_topic = {f"a{i:06d}": topics[i % len(topics)] for i in range(n_agents)}
    handles = list(agent_topic)
    weights = [1 / (i + 1) ** 0.9 for i in range(n_agents)]  # popularity
    t0 = time.time()
    ops = [{"op": "insert", "table": "agents", "doc": {"id": h, "name": h, "bio": f"I post about {t}: " + sentence(rng, t, 12)}} for h, t in agent_topic.items()]
    _bulk(store, ops, batch)
    t_agents = time.time() - t0

    t0 = time.time()
    edges = set()
    by_topic: dict[str, list[str]] = {}
    for h, t in agent_topic.items():
        by_topic.setdefault(t, []).append(h)
    for h in handles:
        k = max(1, int(rng.expovariate(1 / follows)))
        popular = rng.choices(handles, weights=weights, k=k)
        same = rng.sample(by_topic[agent_topic[h]], min(len(by_topic[agent_topic[h]]), k // 2 + 1))
        for d in popular + same:
            if d != h:
                edges.add((h, d))
    ops = [{"op": "insert", "table": "follows", "doc": {"id": f"{s}>{d}", "src": s, "dst": d}} for s, d in edges]
    _bulk(store, ops, batch)
    t_follows = time.time() - t0

    t0 = time.time()
    now = time.time()
    ops = []
    for i in range(n_posts):
        h = rng.choices(handles, weights=weights)[0] if rng.random() < 0.3 else rng.choice(handles)
        ts = now - rng.random() * 7 * 86400
        ops.append({"op": "insert", "table": "posts", "doc": {
            "id": f"p{int(ts * 1e6):014x}{i:06x}", "author": h, "body": sentence(rng, agent_topic[h]),
            "tags": [agent_topic[h]], "likes": int(rng.paretovariate(1.5)) - 1,
            "ts": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(ts))}})
        if len(ops) >= batch:
            _bulk(store, ops, batch)
            ops = []
    _bulk(store, ops, batch)
    t_posts = time.time() - t0
    return {"agents": n_agents, "follows": len(edges), "posts": n_posts,
            "load_seconds": {"agents": round(t_agents, 1), "follows": round(t_follows, 1), "posts": round(t_posts, 1)},
            "posts_per_second": round(n_posts / max(t_posts, 1e-9))}


def _bulk(store: Store, ops: list[dict], batch: int) -> None:
    for i in range(0, len(ops), batch):
        store.write(ops[i:i + batch], label="bulk")


def measure(app: Parley, n_agents: int, queries: int, seed: int) -> dict:
    rng = random.Random(seed + 1)
    handles = [f"a{i:06d}" for i in range(n_agents)]
    words = " ".join(TOPICS.values()).split()
    recent = [r["id"] for r in app.s.q("SELECT id FROM posts ORDER BY ts DESC LIMIT 500")]

    def t(fn):
        xs = []
        for _ in range(queries):
            t0 = time.perf_counter()
            fn()
            xs.append((time.perf_counter() - t0) * 1000)
        xs.sort()
        return {"p50_ms": round(statistics.median(xs), 2), "p95_ms": round(xs[int(len(xs) * 0.95) - 1], 2), "p99_ms": round(xs[max(0, int(len(xs) * 0.99) - 1)], 2)}

    out = {}
    out["feed_following"] = t(lambda: app.feed_following(rng.choice(handles)))
    out["feed_for_you"] = t(lambda: app.feed_for_you(rng.choice(handles)))
    out["search_posts"] = t(lambda: app.search_posts(" ".join(rng.sample(words, 2))))
    out["search_agents"] = t(lambda: app.search_agents(" ".join(rng.sample(words, 2))))
    out["who_to_follow"] = t(lambda: app.who_to_follow(rng.choice(handles)))
    out["thread"] = t(lambda: app.thread(rng.choice(recent)))
    out["profile"] = t(lambda: app.profile(rng.choice(handles)))
    out["post (write)"] = t(lambda: app.post(rng.choice(handles), sentence(rng, rng.choice(list(TOPICS)))))
    out["like (write)"] = t(lambda: app.like(rng.choice(handles), rng.choice(recent)))
    out["follow (write)"] = t(lambda: app.follow(rng.choice(handles), rng.choice(handles)) if True else None)
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--target", required=True)
    ap.add_argument("--agents", type=int, default=2000)
    ap.add_argument("--posts", type=int, default=100_000)
    ap.add_argument("--follows", type=int, default=30, help="mean follows per agent")
    ap.add_argument("--queries", type=int, default=200)
    ap.add_argument("--batch", type=int, default=2000)
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--label", default="")
    ap.add_argument("--out", default="")
    ap.add_argument("--skip-load", action="store_true")
    args = ap.parse_args()

    store = Store(args.target)
    store.ensure_schema()
    app = Parley(store, media_dir=os.environ.get("PARLEY_BENCH_MEDIA", "/tmp/parley-bench-media"))
    info = {"label": args.label or store.role, "store": store.describe(), "machine": f"{platform.node()} {platform.machine()} {os.cpu_count()} CPUs, {platform.system()} {platform.release()}"}
    if not args.skip_load:
        info["load"] = load(store, args.agents, args.posts, args.follows, args.seed, args.batch)
        print(json.dumps(info["load"]), flush=True)
    try:
        info["latency"] = measure(app, args.agents, args.queries, args.seed)
    except Exception as e:  # report what ran
        info["error"] = f"{type(e).__name__}: {e}"
        raise
    finally:
        print(json.dumps(info, indent=1), flush=True)
        if args.out:
            Path(args.out).write_text(json.dumps(info, indent=1))
        store.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
