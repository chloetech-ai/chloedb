"""Runs ChloeCtx's Claude Desktop extension as Claude Desktop runs it.

    python3 check.py chloectx-claude-desktop-macos.mcpb

It unpacks the .mcpb, starts the manifest's command with its arguments and
environment (${__dirname} being the unpacked folder), in a home folder of
its own whose Downloads holds a small ChatGPT export, and over MCP on stdin
and stdout finds the export, imports it, searches it and drafts a line of
the persona card. It uses the embedding model inside the file, so it fails
if the model is missing or does not load.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import zipfile

CONVERSATION = [
    {
        "id": "c1",
        "title": "Weekly meal plan",
        "create_time": 1709337600,
        "current_node": "n3",
        "mapping": {
            "n1": {"id": "n1", "parent": None, "message": None},
            "n2": {
                "id": "n2",
                "parent": "n1",
                "message": {"author": {"role": "user"}, "create_time": 1709337600, "content": {"content_type": "text", "parts": ["I'm vegetarian and I avoid mushrooms. Plan my dinners for the week."]}, "recipient": "all"},
            },
            "n3": {
                "id": "n3",
                "parent": "n2",
                "message": {"author": {"role": "assistant"}, "create_time": 1709337660, "content": {"content_type": "text", "parts": ["Monday: lentil curry. Tuesday: chickpea salad."]}, "recipient": "all"},
            },
        },
    }
]


def main(mcpb):
    work = tempfile.mkdtemp(prefix="chloectx-mcpb-")
    try:
        ext = os.path.join(work, "extension")
        with zipfile.ZipFile(mcpb) as z:
            z.extractall(ext)
        manifest = json.load(open(os.path.join(ext, "manifest.json")))
        assert manifest["compatibility"]["platforms"] == ["darwin"], manifest["compatibility"]
        cfg = manifest["server"]["mcp_config"]
        sub = lambda s: s.replace("${__dirname}", ext)
        command = sub(cfg["command"])
        os.chmod(command, 0o755)
        home = os.path.join(work, "home")
        os.makedirs(os.path.join(home, "Downloads"))
        with zipfile.ZipFile(os.path.join(home, "Downloads", "chatgpt-export.zip"), "w") as z:
            z.writestr("conversations.json", json.dumps(CONVERSATION))
        env = {"PATH": "/usr/bin:/bin", "HOME": home, **{k: sub(v) for k, v in cfg.get("env", {}).items()}}
        p = subprocess.Popen([command] + cfg["args"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, env=env, text=True)
        n = 0

        def call(method, params=None):
            nonlocal n
            n += 1
            p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n, "method": method, "params": params or {}}) + "\n")
            p.stdin.flush()
            r = json.loads(p.stdout.readline())
            assert "error" not in r, r
            return r["result"]

        def tool(name, args=None):
            r = call("tools/call", {"name": name, "arguments": args or {}})
            text = r["content"][0]["text"]
            assert not r["isError"], f"{name}: {text}"
            return text

        init = call("initialize", {"protocolVersion": "2025-06-18", "clientInfo": {"name": "check"}})
        assert init["serverInfo"]["name"] == "chloectx-me", init
        names = [t["name"] for t in call("tools/list")["tools"]]
        listed = [t["name"] for t in manifest["tools"]]
        assert names == listed, f"the manifest lists {listed}, the server serves {names}"
        found = json.loads(tool("find_exports"))["exports"]
        assert found and found[0]["path"].endswith("chatgpt-export.zip"), found
        said = tool("import", {"path": found[0]["path"]})
        assert "2 new message" in said, said
        hits = json.loads(tool("search", {"query": "vegetarian dinners"}))["results"]
        assert any("vegetarian" in h["text"] for h in hits), hits
        draft = tool("persona_draft")
        mid = draft.split('message id="')[1].split('"')[0]
        added = tool("persona_suggest", {"lines": [{"section": "Health & diet", "kind": "preference", "text": "Vegetarian; avoids mushrooms.", "sources": [mid]}]})
        assert "Added 1" in added, added
        card = tool("persona")
        assert "Vegetarian; avoids mushrooms. (suggested)" in card, card
        p.stdin.close()
        p.wait(timeout=30)
        subprocess.run([command, "stop", "--all"], env=env, check=False, capture_output=True)
        print(f"ok: {os.path.basename(mcpb)} v{manifest['version']}, {len(names)} tools, export found, imported, searched and drafted")
    finally:
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main(sys.argv[1])
