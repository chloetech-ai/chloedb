#!/usr/bin/env python3
"""Zep's engine, Graphiti, as a memory for Claude Code: an MCP server with
the tools Graphiti's own MCP server gives an agent (graphiti/mcp_server:
add_memory, search_memory_facts, search_nodes), for the `zep` lane of
scripts/memory_bench.py.

  add_memory(text)            one episode (type text, the time now), which
                              Graphiti turns into entities and facts
  search_memory_facts(query)  the facts that match best (edges, hybrid
                              search reranked by a cross-encoder), each with
                              the time it became true
  search_nodes(query)         the entities that match best, with summaries

The same Graphiti as eval/competitors/zep/graphiti_contexts.py: Haiku 5.5
through the Anthropic API (BENCH_ANTHROPIC_API_KEY), local embedder and
cross-encoder, Neo4j; one group per lane (GRAPHITI_BENCH_GROUP), which lives
outside the lane's HOME, so a graded run (GRAPHITI_BENCH_READONLY=1) adds
nothing to it, as every lane's runs write nothing that is read. Episodes are
added in the background in order, as Graphiti's MCP server does, so the
agent is not kept waiting; `drain` waits for the queue (the harness runs it
after each session). Each model call's usage is appended to
GRAPHITI_BENCH_DIR/llm-results.jsonl as a result message with its cost,
the form memory_bench.py counts.

  graphiti-venv/bin/python -I graphiti_mcp.py            # the MCP server (stdio)
  graphiti-venv/bin/python -I graphiti_mcp.py dump       # every fact and entity of the group, as JSON
  graphiti-venv/bin/python -I graphiti_mcp.py clear      # remove the group (a fresh history)
"""

import asyncio
import datetime as dt
import json
import os
import sys
import uuid

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import graphiti_contexts as gc  # noqa: E402

from graphiti_core import Graphiti  # noqa: E402
from graphiti_core.nodes import EpisodeType  # noqa: E402
from graphiti_core.search.search_config_recipes import EDGE_HYBRID_SEARCH_CROSS_ENCODER, NODE_HYBRID_SEARCH_RRF  # noqa: E402

STATE = os.environ.get("GRAPHITI_BENCH_DIR", ".")
GROUP = os.environ.get("GRAPHITI_BENCH_GROUP", "project")
MODEL = os.environ.get("GRAPHITI_BENCH_MODEL", "claude-haiku-5-5")
PRICE_IN, PRICE_OUT = float(os.environ.get("GRAPHITI_BENCH_PRICE_IN", "0.10")), float(os.environ.get("GRAPHITI_BENCH_PRICE_OUT", "0.50"))
LOG = os.path.join(STATE, "llm-results.jsonl")
QUEUE = os.path.join(STATE, "queue.jsonl")
DONE = os.path.join(STATE, "done.json")
READONLY = os.environ.get("GRAPHITI_BENCH_READONLY") == "1"


def graphiti():
    key = os.environ.get("BENCH_ANTHROPIC_API_KEY")
    if not key:
        sys.exit("BENCH_ANTHROPIC_API_KEY is not set")
    client = gc.llm(MODEL, key)
    raw = client.client.messages.create

    async def logged(**kw):
        r = await raw(**kw)
        tin = (r.usage.input_tokens or 0) + (getattr(r.usage, "cache_read_input_tokens", 0) or 0) + (getattr(r.usage, "cache_creation_input_tokens", 0) or 0)
        tout = r.usage.output_tokens or 0
        with open(LOG, "a") as f:
            f.write(json.dumps({"type": "result", "session_id": str(uuid.uuid4()), "total_cost_usd": tin * PRICE_IN / 1e6 + tout * PRICE_OUT / 1e6,
                                "usage": {"input_tokens": tin, "output_tokens": tout}, "modelUsage": {MODEL: {}}}) + "\n")
        return r

    client.client.messages.create = logged
    return Graphiti(os.environ.get("GRAPHITI_BENCH_NEO4J", "bolt://127.0.0.1:7687"), "neo4j", os.environ.get("NEO4J_PASSWORD", "benchpass123"),
                    llm_client=client, embedder=gc.LocalEmbedder("BAAI/bge-small-en-v1.5"),
                    cross_encoder=gc.LocalCrossEncoder("Xenova/ms-marco-MiniLM-L-6-v2"))


def queued():
    if not os.path.exists(QUEUE):
        return []
    return [json.loads(l) for l in open(QUEUE) if l.strip()]


def done_count():
    return json.load(open(DONE))["done"] if os.path.exists(DONE) else 0


async def worker(g, wake):
    """Adds queued episodes in order; the queue and its progress live in files, so
    an episode the server did not finish (the session ended) is added by the next
    server or by `drain`."""
    while True:
        q, n = queued(), done_count()
        if n >= len(q):
            wake.clear()
            await wake.wait()
            continue
        ep = q[n]
        for attempt in range(4):
            try:
                await g.add_episode(name=ep["name"], episode_body=ep["text"], source_description="Claude Code session",
                                    reference_time=dt.datetime.fromisoformat(ep["time"]), source=EpisodeType.text, group_id=GROUP)
                break
            except Exception as e:  # noqa: BLE001 - retried, then skipped (logged)
                if attempt == 3:
                    print(f"episode {n} failed: {e}", file=sys.stderr)
                await asyncio.sleep(5 * 2**attempt)
        with open(DONE + ".tmp", "w") as f:
            json.dump({"done": n + 1}, f)
        os.replace(DONE + ".tmp", DONE)


async def serve():
    from mcp.server.fastmcp import FastMCP

    g = graphiti()
    await g.build_indices_and_constraints()
    wake = asyncio.Event()
    mcp = FastMCP("graphiti")
    task = None

    @mcp.tool()
    async def add_memory(text: str) -> str:
        """Save something to long-term memory: a requirement, decision and its reason, rule, convention, preference or fact about
        the project, as a short self-contained statement. It is turned into entities and facts in the background."""
        nonlocal task
        if READONLY:
            # a graded run: what it would save is never read (memory_bench.py's rule for every lane)
            return "Queued for memory."
        if task is None:
            task = asyncio.create_task(worker(g, wake))
        with open(QUEUE, "a") as f:
            f.write(json.dumps({"name": f"memory-{uuid.uuid4().hex[:8]}", "text": text, "time": dt.datetime.now(dt.timezone.utc).isoformat()}) + "\n")
        wake.set()
        return "Queued for memory."

    @mcp.tool()
    async def search_memory_facts(query: str, max_facts: int = 10) -> str:
        """Search long-term memory for facts relevant to the query (each with the time it became true)."""
        r = await g.search_(query, config=EDGE_HYBRID_SEARCH_CROSS_ENCODER.model_copy(update={"limit": max_facts}), group_ids=[GROUP])
        return "\n".join(f"- {e.fact} (valid from {e.valid_at}{', invalid since ' + str(e.invalid_at) if e.invalid_at else ''})" for e in r.edges) or "No facts found."

    @mcp.tool()
    async def search_nodes(query: str, max_nodes: int = 10) -> str:
        """Search long-term memory for entities (people, components, decisions, conventions) relevant to the query, with summaries."""
        r = await g.search_(query, config=NODE_HYBRID_SEARCH_RRF.model_copy(update={"limit": max_nodes}), group_ids=[GROUP])
        return "\n".join(f"- {n.name}: {n.summary}" for n in r.nodes) or "No entities found."

    if done_count() < len(queued()) and not READONLY:
        task = asyncio.create_task(worker(g, wake))
        wake.set()
    await mcp.run_stdio_async()


async def drain():
    """Adds whatever the last session queued and did not add."""
    if done_count() >= len(queued()):
        return
    g = graphiti()
    wake = asyncio.Event()
    t = asyncio.create_task(worker(g, wake))
    wake.set()
    while done_count() < len(queued()):
        await asyncio.sleep(1)
    t.cancel()
    await g.close()


def store_only():
    """Graphiti without a model, for reading and clearing the group."""
    from graphiti_core.driver.neo4j_driver import Neo4jDriver
    return Neo4jDriver(os.environ.get("GRAPHITI_BENCH_NEO4J", "bolt://127.0.0.1:7687"), "neo4j", os.environ.get("NEO4J_PASSWORD", "benchpass123"))


async def clear():
    """Removes the group's graph and the queue (a fresh history)."""
    d = store_only()
    await d.execute_query("MATCH (n {group_id: $g}) DETACH DELETE n", g=GROUP)
    await d.close()
    for p in (QUEUE, DONE):
        if os.path.exists(p):
            os.remove(p)


async def dump():
    class G:
        driver = store_only()

        async def close(self):
            await self.driver.close()

    g = G()
    facts, *_ = await g.driver.execute_query("MATCH (a)-[e:RELATES_TO {group_id: $g}]->(b) RETURN e.fact AS fact, e.valid_at AS valid, e.invalid_at AS invalid", g=GROUP)
    nodes, *_ = await g.driver.execute_query("MATCH (n:Entity {group_id: $g}) RETURN n.name AS name, n.summary AS summary", g=GROUP)
    print(json.dumps({"queued": len(queued()), "added": done_count(), "facts": [dict(r) for r in facts][:500],
                      "entities": [dict(r) for r in nodes][:500]}, default=str))
    await g.close()


if __name__ == "__main__":
    os.makedirs(STATE, exist_ok=True)
    cmd = sys.argv[1] if len(sys.argv) > 1 else "serve"
    asyncio.run({"serve": serve, "drain": drain, "dump": dump, "clear": clear}[cmd]())
