#!/usr/bin/env python3
"""Reader contexts from Zep's open-source engine, Graphiti, for
scripts/reader_eval.py and scripts/bench_eval.py.

  BENCH_ANTHROPIC_API_KEY=... graphiti-venv/bin/python -I eval/competitors/zep/graphiti_contexts.py \\
      --data locomo.jsonl --episodes message --work /home/claude/bench/zep/locomo \\
      --out results/baselines/locomo/zep.contexts.jsonl

Graphiti (graphiti-core 0.31.2, Apache-2.0) builds a temporal knowledge graph:
each episode added goes through its model calls (entities, facts between
them, deduplication against the graph, facts a newer one invalidates), and
each fact keeps the time it became true. This is the engine of Zep's hosted
service, not the service itself (whose models, prompts and reranker are its
own).

As Zep's own LoCoMo evaluation (github.com/getzep/zep-papers,
kg_architecture_agent_memory/locomo_eval) runs it:

- ingestion: one episode per message, "<speaker>: <text>", of type message,
  with its session's date as reference time (`--episodes message`); or one
  episode per session, all its messages in one body (`--episodes session`,
  for LongMemEval, where per message costs about ten times as much);
- search: the 20 best nodes (hybrid BM25 + cosine, RRF) and the 20 best edges
  (hybrid + breadth-first, reranked by a cross-encoder) for the question;
- context: Zep's template, each fact with its event time and each entity with
  its summary.

Models: Haiku 5.5 through the Anthropic API (BENCH_ANTHROPIC_API_KEY), for
every call Graphiti makes, with thinking off and no temperature (Haiku 5.5
rejects a non-default temperature, and Graphiti forces a tool call, which
thinking does not allow). Embeddings: BAAI/bge-small-en-v1.5 (384
dimensions) with fastembed on the CPU; cross-encoder:
Xenova/ms-marco-MiniLM-L-6-v2 with fastembed (the same pair the Hindsight
lane used). Store: Neo4j 5.26, one group per history.

Costs are counted from each response's usage at Haiku 5.5's list price
(--price-in/--price-out per million tokens). Histories run in a fixed
shuffled order (seed 7); a history resumes after its last added episode.
"""

import argparse
import asyncio
import datetime as dt
import json
import logging
import os
import random
import re
import sys
import time

os.environ.setdefault("EMBEDDING_DIM", "384")
os.environ.setdefault("GRAPHITI_TELEMETRY_ENABLED", "false")
logging.getLogger("neo4j").setLevel(logging.ERROR)

from anthropic import AsyncAnthropic  # noqa: E402
from fastembed import TextEmbedding  # noqa: E402
from fastembed.rerank.cross_encoder import TextCrossEncoder  # noqa: E402
from graphiti_core import Graphiti  # noqa: E402
from graphiti_core.cross_encoder.client import CrossEncoderClient  # noqa: E402
from graphiti_core.embedder.client import EmbedderClient  # noqa: E402
from graphiti_core.llm_client.anthropic_client import AnthropicClient  # noqa: E402
from graphiti_core.llm_client.config import LLMConfig  # noqa: E402
from graphiti_core.nodes import EpisodeType  # noqa: E402
from graphiti_core.search.search_config_recipes import EDGE_HYBRID_SEARCH_CROSS_ENCODER, NODE_HYBRID_SEARCH_RRF  # noqa: E402

# Zep's template (zep-papers, locomo_eval/zep_locomo_search.py).
TEMPLATE = """
FACTS and ENTITIES represent relevant context to the current conversation.

# These are the most relevant facts for the conversation along with the datetime of the event that the fact refers to.
If a fact mentions something happening a week ago, then the datetime will be the date time of last week and not the datetime
of when the fact was stated.
Timestamps in memories represent the actual time the event occurred, not the time the event was mentioned in a message.

<FACTS>
{facts}
</FACTS>

# These are the most relevant entities
# ENTITY_NAME: entity summary
<ENTITIES>
{entities}
</ENTITIES>
"""

USAGE = {"calls": 0, "input": 0, "output": 0, "errors": 0}


def compose(edges, nodes):
    facts = [f"  - {e.fact} (event_time: {e.valid_at})" for e in edges]
    entities = [f"  - {n.name}: {n.summary}" for n in nodes]
    return TEMPLATE.format(facts="\n".join(facts), entities="\n".join(entities))


class LocalEmbedder(EmbedderClient):
    def __init__(self, name):
        self.model = TextEmbedding(name, threads=2)
        self.lock = asyncio.Lock()

    async def _embed(self, texts):
        async with self.lock:
            return await asyncio.to_thread(lambda: [v.tolist() for v in self.model.embed(texts, batch_size=64)])

    async def create(self, input_data):
        text = input_data if isinstance(input_data, str) else " ".join(map(str, input_data))
        return (await self._embed([text]))[0]

    async def create_batch(self, input_data_list):
        return await self._embed(list(input_data_list))


class LocalCrossEncoder(CrossEncoderClient):
    def __init__(self, name):
        self.model = TextCrossEncoder(name, threads=2)
        self.lock = asyncio.Lock()

    async def rank(self, query, passages):
        if not passages:
            return []
        async with self.lock:
            scores = await asyncio.to_thread(lambda: list(self.model.rerank(query, passages, batch_size=64)))
        return sorted(zip(passages, scores), key=lambda x: -x[1])


def llm(model, key):
    client = AnthropicClient(config=LLMConfig(api_key=key, model=model, small_model=model), client=AsyncAnthropic(api_key=key, max_retries=8))
    raw = client.client.messages.create

    async def create(**kw):
        kw.pop("temperature", None)
        kw["thinking"] = {"type": "disabled"}
        try:
            r = await raw(**kw)
        except Exception:
            USAGE["errors"] += 1
            raise
        USAGE["calls"] += 1
        USAGE["input"] += (r.usage.input_tokens or 0) + (getattr(r.usage, "cache_read_input_tokens", 0) or 0) + (getattr(r.usage, "cache_creation_input_tokens", 0) or 0)
        USAGE["output"] += r.usage.output_tokens or 0
        return r

    client.client.messages.create = create
    return client


LOCOMO_TIME = "%I:%M %p on %d %B, %Y"


def parse_time(s):
    s = (s or "").strip()
    for fmt in (LOCOMO_TIME, "%I:%M %p on %d %b, %Y", "%Y/%m/%d (%a) %H:%M", "%Y/%m/%d"):
        try:
            return dt.datetime.strptime(s, fmt).replace(tzinfo=dt.timezone.utc)
        except ValueError:
            pass
    m = re.match(r"(\d{4})/(\d{2})/(\d{2})", s)
    if m:
        return dt.datetime(int(m[1]), int(m[2]), int(m[3]), tzinfo=dt.timezone.utc)
    return dt.datetime(2023, 1, 1, tzinfo=dt.timezone.utc)


def episodes(unit, mode):
    """[(name, body, reference time)] in order."""
    out = []
    if mode == "message":
        for t in unit["turns"]:
            out.append((t["turn_id"], f"{t['role']}: {t['text']}", parse_time(t.get("timestamp"))))
        return out
    cur, body, when = None, [], None
    for t in unit["turns"] + [None]:
        if t is None or t["session_id"] != cur:
            if body:
                out.append((cur, "\n".join(body), when))
            if t is None:
                break
            cur, body, when = t["session_id"], [], parse_time(t.get("timestamp"))
        body.append(f"{t['role']}: {t['text']}")
    return out


def group_id(unit):
    return re.sub(r"[^A-Za-z0-9_-]", "_", unit["unit_id"])


async def run_unit(g, unit, args, sem):
    gid = group_id(unit)
    state_path = os.path.join(args.work, gid + ".json")
    state = {"done": 0, "seconds": 0.0, "calls": 0, "input": 0, "output": 0}
    if os.path.exists(state_path):
        with open(state_path) as f:
            state = json.load(f)
    eps = episodes(unit, args.episodes)
    async with sem:
        t0 = time.time()
        while state["done"] < len(eps):
            name, body, when = eps[state["done"]]
            before = dict(USAGE)
            for attempt in range(4):
                try:
                    await g.add_episode(name=f"{gid}:{name}", episode_body=body, source_description="conversation",
                                        reference_time=when, source=EpisodeType.message, group_id=gid)
                    break
                except Exception as e:  # noqa: BLE001 - retried, then the history stops here and resumes on a rerun
                    if attempt == 3:
                        raise
                    print(f"  {gid} episode {state['done']}: {type(e).__name__}: {str(e)[:200]}; retrying", file=sys.stderr)
                    await asyncio.sleep(10 * 2**attempt)
            for k in ("calls", "input", "output"):
                state[k] += USAGE[k] - before[k]
            state["done"] += 1
            state["seconds"] += time.time() - t0
            t0 = time.time()
            with open(state_path, "w") as f:
                json.dump(state, f)
            if state["done"] % 50 == 0:
                print(f"  {gid}: {state['done']}/{len(eps)} episodes, {state['calls']} calls", file=sys.stderr)
        rows = []
        for q in unit["questions"]:
            if args.skip_abstention and q["abstention"]:
                continue
            t1 = time.time()
            nodes, edges = await asyncio.gather(
                g.search_(q["question"], config=NODE_HYBRID_SEARCH_RRF.model_copy(update={"limit": 20}), group_ids=[gid]),
                g.search_(q["question"], config=EDGE_HYBRID_SEARCH_CROSS_ENCODER.model_copy(update={"limit": 20}), group_ids=[gid]),
            )
            rows.append({"qid": q["qid"], "context": compose(edges.edges, nodes.nodes), "search_ms": (time.time() - t1) * 1000,
                         "facts": len(edges.edges), "entities": len(nodes.nodes)})
    cost = state["input"] * args.price_in / 1e6 + state["output"] * args.price_out / 1e6
    print(f"  {gid}: {len(eps)} episodes, {state['calls']} calls, {state['input']:,} in / {state['output']:,} out tokens, ${cost:.2f}, {state['seconds']:.0f} s", file=sys.stderr)
    return gid, state, rows


async def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--work", required=True)
    ap.add_argument("--episodes", choices=("message", "session"), default="message")
    ap.add_argument("--every", type=int, default=1)
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--parallel", type=int, default=3, help="histories ingested at once")
    ap.add_argument("--model", default="claude-haiku-5-5")
    ap.add_argument("--neo4j", default="bolt://127.0.0.1:7687")
    ap.add_argument("--neo4j-password", default=os.environ.get("NEO4J_PASSWORD", "benchpass123"))
    ap.add_argument("--embedder", default="BAAI/bge-small-en-v1.5")
    ap.add_argument("--reranker", default="Xenova/ms-marco-MiniLM-L-6-v2")
    ap.add_argument("--price-in", type=float, default=0.10)
    ap.add_argument("--price-out", type=float, default=0.50)
    ap.add_argument("--skip-abstention", action="store_true")
    args = ap.parse_args()
    key = os.environ.get("BENCH_ANTHROPIC_API_KEY")
    if not key:
        sys.exit("BENCH_ANTHROPIC_API_KEY is not set")
    os.makedirs(args.work, exist_ok=True)
    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)

    units = [json.loads(l) for i, l in enumerate(open(args.data)) if i % args.every == 0]
    if args.limit:
        units = units[: args.limit]
    random.Random(7).shuffle(units)
    g = Graphiti(args.neo4j, "neo4j", args.neo4j_password, llm_client=llm(args.model, key),
                 embedder=LocalEmbedder(args.embedder), cross_encoder=LocalCrossEncoder(args.reranker))
    await g.build_indices_and_constraints()
    sem = asyncio.Semaphore(args.parallel)
    results = await asyncio.gather(*[run_unit(g, u, args, sem) for u in units], return_exceptions=True)
    await g.close()
    rows, states, failed = [], {}, 0
    for r in results:
        if isinstance(r, BaseException):
            failed += 1
            print(f"  failed: {type(r).__name__}: {str(r)[:300]}", file=sys.stderr)
            continue
        gid, state, rs = r
        states[gid] = state
        rows += rs
    with open(args.out, "w") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")
    tin, tout = sum(s["input"] for s in states.values()), sum(s["output"] for s in states.values())
    summary = {"system": "graphiti 0.31.2", "model": args.model, "episodes": args.episodes, "histories": len(states), "failed": failed,
               "calls": sum(s["calls"] for s in states.values()), "input_tokens": tin, "output_tokens": tout,
               "cost_usd": tin * args.price_in / 1e6 + tout * args.price_out / 1e6,
               "mean_context_chars": sum(len(r["context"]) for r in rows) / max(1, len(rows)),
               "search_ms_median": sorted(r["search_ms"] for r in rows)[len(rows) // 2] if rows else None}
    with open(os.path.splitext(args.out)[0].removesuffix(".contexts") + ".summary.json", "w") as f:
        json.dump(summary, f, indent=1)
    print(json.dumps(summary), file=sys.stderr)


if __name__ == "__main__":
    asyncio.run(main())
