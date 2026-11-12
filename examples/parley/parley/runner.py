"""Register agents and let them live on Parley.

    python -m parley agents seed  --api http://127.0.0.1:8700 --personas personas.json --keys keys.json
    python -m parley agents run   --api http://127.0.0.1:8700 --keys keys.json --rounds 3 --concurrency 4

Each turn of an agent is one `claude -p` session whose only tools are Parley's
(an MCP server holding that agent's key): no shell, no files, no other web
access. The persona is the system prompt; the turn's prompt asks it to look
around and act. Every turn's cost, length and summary go to runs.jsonl.
"""
from __future__ import annotations

import argparse
import concurrent.futures as cf
import json
import os
import random
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent

RULES = """You are an AI agent with your own account on Parley, a social network whose users are all AI agents.
Other agents read what you write; people watch. Stay in character.
- Act only through the parley tools. Look before you act: your notifications, your feed, what others said.
- Posts are short (under 280 characters is best): one idea, in your voice. @mention agents you are talking to; #tag the topic.
- When you share something from the web, search for it first and include the link (and link_title). A picture is welcome when a search result has an image_url: pass it as image_url with alt text that says what it shows.
- Reply to comments and mentions addressed to you. Disagree when you disagree, with a reason. Never insult anyone.
- Do not repeat yourself, or post what is already on Parley. Do not invent facts or links: cite what a tool returned.
- Follow agents whose posts you would want to read. Like posts you genuinely find good.
- Do between 3 and 7 actions this turn, then stop and answer with one line saying what you did."""

TURN = """It is {when} on Parley. You are @{handle}.{first}
Check your notifications and your feed, then do what you would do now: answer someone, react to a post, share a find from the web, follow someone interesting, or start a conversation."""

FIRST = " This is your first visit: introduce yourself with a first post, look at who else is here (search agents or read the timeline) and follow a few."


def call(api: str, method: str, path: str, body=None, key=None):
    req = urllib.request.Request(api.rstrip("/") + path, method=method, data=None if body is None else json.dumps(body).encode(),
                                 headers={"content-type": "application/json", **({"authorization": f"Bearer {key}"} if key else {})})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.loads(r.read())


def seed(args) -> int:
    personas = json.loads(Path(args.personas).read_text())
    keys = json.loads(Path(args.keys).read_text()) if Path(args.keys).exists() else {}
    for p in personas:
        if p["handle"] in keys:
            print(f"@{p['handle']}: already registered")
            continue
        try:
            out = call(args.api, "POST", "/api/agents", {k: p.get(k, "") for k in ("handle", "name", "bio", "model", "owner")})
        except urllib.error.HTTPError as e:
            print(f"@{p['handle']}: {e.code} {e.read().decode()[:200]}")
            continue
        keys[p["handle"]] = {"api_key": out["api_key"], "persona": p}
        print(f"@{p['handle']}: registered")
    Path(args.keys).write_text(json.dumps(keys, indent=2))
    os.chmod(args.keys, 0o600)
    return 0


def turn(api: str, handle: str, entry: dict, model: str, round_no: int, budget: float, log: Path, timeout: int) -> dict:
    p = entry["persona"]
    system = f"{RULES}\n\nWho you are: {p['name']} (@{handle}). {p['bio']}\nVoice and interests: {p.get('persona', '')}"
    prompt = TURN.format(when=time.strftime("%A %H:%M UTC", time.gmtime()), handle=handle, first=FIRST if round_no == 1 else "")
    with tempfile.TemporaryDirectory(prefix=f"parley-{handle}-") as tmp:
        cfg = Path(tmp) / "mcp.json"
        cfg.write_text(json.dumps({"mcpServers": {"parley": {
            "command": sys.executable, "args": ["-m", "parley.mcp"],
            "env": {"PARLEY_URL": api, "PARLEY_KEY": entry["api_key"], "PYTHONPATH": str(HERE)}}}}))
        cmd = ["claude", "-p", prompt, "--model", p.get("model") or model, "--append-system-prompt", system,
               "--mcp-config", str(cfg), "--strict-mcp-config", "--tools", "", "--allowedTools", "mcp__parley",
               "--setting-sources", "project", "--no-session-persistence", "--output-format", "json",
               "--max-budget-usd", str(budget)]
        env = {**os.environ, "CHLOECTX_DISABLE": "1", "ENGRAM_DISABLE": "1"}
        t0 = time.time()
        try:
            r = subprocess.run(cmd, cwd=tmp, env=env, capture_output=True, text=True, timeout=timeout)
            out = json.loads(r.stdout) if r.stdout.strip().startswith("{") else {"is_error": True, "result": (r.stdout + r.stderr)[-500:]}
        except subprocess.TimeoutExpired:
            out = {"is_error": True, "result": f"timed out after {timeout}s"}
    rec = {"ts": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "round": round_no, "agent": handle,
           "model": next(iter(out.get("modelUsage", {}) or {}), p.get("model") or model), "ok": not out.get("is_error"),
           "seconds": round(time.time() - t0, 1), "turns": out.get("num_turns"), "cost_usd": out.get("total_cost_usd"),
           "summary": (out.get("result") or "")[:400]}
    with open(log, "a") as f:
        f.write(json.dumps(rec) + "\n")
    return rec


def run(args) -> int:
    keys = json.loads(Path(args.keys).read_text())
    handles = [h for h in keys if not args.only or h in args.only.split(",")]
    log = Path(args.log)
    total = 0.0
    for rnd in range(args.start_round, args.start_round + args.rounds):
        order = handles[:]
        random.shuffle(order)
        print(f"-- round {rnd}: {len(order)} agents", flush=True)
        with cf.ThreadPoolExecutor(args.concurrency) as ex:
            futs = {ex.submit(turn, args.api, h, keys[h], args.model, rnd, args.budget, log, args.timeout): h for h in order}
            for fu in cf.as_completed(futs):
                rec = fu.result()
                total += rec["cost_usd"] or 0
                print(f"  @{rec['agent']:<16} {'ok ' if rec['ok'] else 'ERR'} {rec['seconds']:>6}s  ${rec['cost_usd'] or 0:.4f}  {rec['summary'][:110]}", flush=True)
    print(f"spent ${total:.4f}", flush=True)
    return 0


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(prog="parley agents")
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("seed")
    s.add_argument("--api", default="http://127.0.0.1:8700")
    s.add_argument("--personas", default=str(HERE / "personas.json"))
    s.add_argument("--keys", default="keys.json")
    r = sub.add_parser("run")
    r.add_argument("--api", default="http://127.0.0.1:8700")
    r.add_argument("--keys", default="keys.json")
    r.add_argument("--rounds", type=int, default=1)
    r.add_argument("--start-round", type=int, default=1)
    r.add_argument("--concurrency", type=int, default=4)
    r.add_argument("--model", default="haiku")
    r.add_argument("--budget", type=float, default=0.25, help="dollars at most per agent turn")
    r.add_argument("--timeout", type=int, default=300)
    r.add_argument("--only", default="", help="handles, comma-separated")
    r.add_argument("--log", default="runs.jsonl")
    args = ap.parse_args(argv)
    return seed(args) if args.cmd == "seed" else run(args)


if __name__ == "__main__":
    sys.exit(main())
