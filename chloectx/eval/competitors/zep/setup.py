#!/usr/bin/env python3
"""Sets up the zep lane for one isolated HOME (used by scripts/memory_bench.py):
Graphiti (Zep's open-source engine) behind graphiti_mcp.py, run with the
Graphiti virtualenv's Python, against the Neo4j at GRAPHITI_BENCH_NEO4J
(default bolt://127.0.0.1:7687), in a group of its own per lane.

Needs BENCH_ANTHROPIC_API_KEY in the harness's environment: Graphiti calls
the Anthropic API itself. The key is passed to each `claude` call's
environment at run time (Claude Code hands its environment to the MCP
server) and is never written to the lane's files.

  python3 eval/competitors/zep/setup.py HOME GROUP
"""

import json
import os
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
VENV = os.environ.get("GRAPHITI_VENV", "/home/claude/bench/graphiti-venv")
SYSTEM_PROMPT = (
    "You have a long-term memory for this project, kept between sessions by Graphiti (Zep): the tools "
    "mcp__graphiti__search_memory_facts, mcp__graphiti__search_nodes and mcp__graphiti__add_memory. Before you start on a "
    "request, search the memory for anything relevant to it (requirements, rules, decisions, conventions, constraints, earlier "
    "plans). When the user states something that will matter in later sessions (a requirement, rule, decision and its reason, "
    "preference, constraint or fact about the project or its environment), save it with add_memory as a short self-contained "
    "statement."
)


def install(home, group, model="claude-haiku-5-5"):
    py = os.path.join(VENV, "bin", "python")
    if subprocess.run([py, "-I", "-c", "import graphiti_core, fastembed; from mcp.server.fastmcp import FastMCP"], capture_output=True).returncode:
        sys.exit(f"{VENV} lacks graphiti-core, fastembed or mcp")
    state = os.path.join(home, ".graphiti-bench")
    os.makedirs(state, exist_ok=True)
    env = {"GRAPHITI_BENCH_DIR": state, "GRAPHITI_BENCH_GROUP": group, "GRAPHITI_BENCH_MODEL": model,
           "GRAPHITI_BENCH_NEO4J": os.environ.get("GRAPHITI_BENCH_NEO4J", "bolt://127.0.0.1:7687"),
           "GRAPHITI_TELEMETRY_ENABLED": "false", "EMBEDDING_DIM": "384",
           "HF_HUB_OFFLINE": "1"}  # fastembed reads its models from /tmp/fastembed_cache, fetched beforehand
    cfg = {"mcpServers": {"graphiti": {"type": "stdio", "command": py, "args": ["-I", os.path.join(HERE, "graphiti_mcp.py")], "env": env}}}
    path = os.path.join(state, "mcp.json")
    json.dump(cfg, open(path, "w"), indent=1)
    version = subprocess.run([py, "-I", "-c", "import importlib.metadata as m; print(m.version('graphiti-core'))"], capture_output=True, text=True).stdout.strip()
    return {"version": version, "python": py, "state": state, "group": group, "mcp_config": path, "system_prompt": SYSTEM_PROMPT,
            "server_env": env, "llm_log": os.path.join(state, "llm-results.jsonl"), "model": model}


def _run(info, cmd, home, timeout):
    env = {"PATH": "/usr/bin:/bin", "HOME": home, **info["server_env"]}
    if os.environ.get("BENCH_ANTHROPIC_API_KEY"):
        env["BENCH_ANTHROPIC_API_KEY"] = os.environ["BENCH_ANTHROPIC_API_KEY"]
    return subprocess.run([info["python"], "-I", os.path.join(HERE, "graphiti_mcp.py"), cmd], env=env, cwd=home, capture_output=True, text=True, timeout=timeout)


def drain(home, info):
    """Adds the episodes the last session queued and its server did not finish."""
    p = _run(info, "drain", home, 3600)
    return p.returncode == 0 or (p.stdout + p.stderr)[-500:]


def clear(home, info):
    p = _run(info, "clear", home, 300)
    if p.returncode:
        raise RuntimeError(f"clearing the graph failed: {(p.stdout + p.stderr)[-500:]}")


def dump(home, info):
    p = _run(info, "dump", home, 300)
    try:
        return json.loads(p.stdout.strip().splitlines()[-1])
    except (json.JSONDecodeError, IndexError):
        return {"error": (p.stdout + p.stderr)[-800:]}


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    print(json.dumps(install(os.path.abspath(sys.argv[1]), sys.argv[2]), indent=1))
