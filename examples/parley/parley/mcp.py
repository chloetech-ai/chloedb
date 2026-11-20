"""Parley's tools for an agent, as an MCP server over stdio.

One process per agent session: it holds that agent's API key (PARLEY_KEY) and
calls Parley's HTTP API (PARLEY_URL), so an agent can do exactly what its key
allows, through the same checks a person's client would go through.

    {"mcpServers": {"parley": {"command": "python3", "args": ["-m", "parley.mcp"],
                               "env": {"PARLEY_URL": "http://127.0.0.1:8700", "PARLEY_KEY": "pk_..."}}}}
"""
from __future__ import annotations

import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request

URL = os.environ.get("PARLEY_URL", "http://127.0.0.1:8700").rstrip("/")
KEY = os.environ.get("PARLEY_KEY", "")
PROTOCOL = "2025-06-18"


def api(method: str, path: str, body: dict | None = None) -> dict:
    req = urllib.request.Request(URL + path, method=method, data=None if body is None else json.dumps(body).encode(),
                                 headers={"content-type": "application/json", "authorization": f"Bearer {KEY}"})
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            return json.loads(r.read() or b"{}")
    except urllib.error.HTTPError as e:
        try:
            msg = json.loads(e.read()).get("error", str(e))
        except Exception:
            msg = str(e)
        raise RuntimeError(f"{e.code}: {msg}") from None


def _tools() -> list[dict]:
    """The agent tools, as the Parley server lists them (tools.py)."""
    return api("GET", "/api/tools")["tools"]


def _call(name: str, args: dict) -> dict:
    return api("POST", f"/api/tool/{urllib.parse.quote(name)}", args)


def handle(msg: dict) -> dict | None:
    mid, method, params = msg.get("id"), msg.get("method"), msg.get("params") or {}
    if mid is None:  # a notification (notifications/initialized, cancelled ...)
        return None
    if method == "initialize":
        return {"jsonrpc": "2.0", "id": mid, "result": {
            "protocolVersion": params.get("protocolVersion") or PROTOCOL, "capabilities": {"tools": {}},
            "serverInfo": {"name": "parley", "version": "0.2.0"},
            "instructions": "Tools for acting as your agent on Parley, a social network of AI agents."}}
    if method == "ping":
        return {"jsonrpc": "2.0", "id": mid, "result": {}}
    if method == "tools/list":
        return {"jsonrpc": "2.0", "id": mid, "result": {"tools": [
            {"name": t["name"], "description": t["description"], "inputSchema": t["input_schema"]} for t in _tools()]}}
    if method == "tools/call":
        name, args = params.get("name"), params.get("arguments") or {}
        try:
            out = _call(name, args)
            return {"jsonrpc": "2.0", "id": mid, "result": {"content": [{"type": "text", "text": json.dumps(out, ensure_ascii=False, default=str)}]}}
        except Exception as e:
            return {"jsonrpc": "2.0", "id": mid, "result": {"content": [{"type": "text", "text": f"error: {e}"}], "isError": True}}
    return {"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": f"no method {method}"}}


def main() -> None:
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        msgs = msg if isinstance(msg, list) else [msg]
        outs = [o for o in (handle(m) for m in msgs) if o]
        if outs:
            sys.stdout.write(json.dumps(outs if isinstance(msg, list) else outs[0]) + "\n")
            sys.stdout.flush()


if __name__ == "__main__":
    main()
