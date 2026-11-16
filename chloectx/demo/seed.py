#!/usr/bin/env python3
"""Seeds ChloeCtx with a large, realistic Claude Code history, for demos.

It plays the story in demo/story.py (20 sessions over three and a half
weeks on "harbor", a mock order and payments service: refunds, webhooks,
migrations, an incident, corrections like "no, finance said 1000 EUR, not
500") through ChloeCtx's real hooks, as Claude Code would:

* a git repository is made with the story's files, and each tool call is
  applied to them (edits, new files, commits dated as the story is), so that
  `chloectx blame app/payments/refunds.py:12` finds the line's edit;
* each turn's SessionStart, UserPromptSubmit, Pre/PostToolUse and Stop
  hooks go to the project's daemon, which records them, recalls into each
  prompt (with a receipt) and keeps snapshots, as in a real session;
* the distiller is demo/scripted_distill.py, which writes down the story's
  notes for each turn (and their replacements), so no model is called;
* each turn runs at its date in the story (CHLOECTX_CLOCK_OFFSET), so the
  records, receipts and snapshots are dated across the three weeks.

Everything goes under one folder (default ~/chloectx-demo): the project
(harbor/) and ChloeCtx's home (home/, as CHLOECTX_HOME), so the user's own
stores are not touched. Then, with --server:

* --space NAME writes the same story into a team space of that cluster
  (POST /v1/apps/chloectx/v1/spaces...): each exchange and each note, dated,
  with the corrections as replacements. Only adds: a space that exists is
  refused unless --space-reuse.
* --sync copies the store into the cluster's chloectx_memory (chloectx sync),
  where the Spaces page lists it as the synced project acme/harbor.

The key is read from the environment variable --key-env names (never from
the command line). Then:

    CHLOECTX_HOME=~/chloectx-demo/home chloectx app --server URL --local-stores

shows the store under Spaces, with every tab.

    python3 chloectx/demo/seed.py                           # the local store only
    python3 chloectx/demo/seed.py --server http://127.0.0.1:7700 --key-env CHLOEDB_KEY --space harbor
    python3 chloectx/demo/seed.py --check                    # apply the story's edits only

Standard library only.
"""

import argparse
import datetime as dt
import json
import os
import random
import shutil
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
import uuid

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import story  # noqa: E402

TZ = dt.timezone(dt.timedelta(hours=-8))  # the story's developer is on Pacific time
AUTHOR = ("Sam Rivera", "sam@harbor.example")


def at(s):
    return int(dt.datetime.strptime(s, "%Y-%m-%d %H:%M").replace(tzinfo=TZ).timestamp())


def find_chloectx(given):
    if given:
        return os.path.abspath(given)
    for p in ("target/release/chloectx", "target/ci/chloectx"):
        c = os.path.join(HERE, "..", p)
        if os.path.exists(c):
            return os.path.abspath(c)
    return shutil.which("chloectx") or sys.exit("seed: no chloectx binary: build it (cargo build --release in chloectx) or pass --chloectx")


class Repo:
    """The story's files on disk, changed by its tool calls."""

    def __init__(self, root, write=True):
        self.root = root
        self.write = write
        self.files = dict(story.FILES)

    def abs(self, rel):
        return os.path.join(self.root, rel)

    def put(self, rel, text):
        self.files[rel] = text
        if self.write:
            p = self.abs(rel)
            os.makedirs(os.path.dirname(p), exist_ok=True)
            with open(p, "w") as f:
                f.write(text)

    def edit(self, rel, old, new, where):
        text = self.files.get(rel)
        if text is None or old not in text:
            sys.exit(f"seed: {where}: the edit of {rel} does not apply: {old[:80]!r} is not in it")
        self.put(rel, text.replace(old, new, 1))

    def git(self, *args, ts=None):
        if not self.write:
            return ""
        env = dict(os.environ, GIT_AUTHOR_NAME=AUTHOR[0], GIT_AUTHOR_EMAIL=AUTHOR[1], GIT_COMMITTER_NAME=AUTHOR[0], GIT_COMMITTER_EMAIL=AUTHOR[1])
        if ts:
            d = dt.datetime.fromtimestamp(ts, TZ).isoformat()
            env.update(GIT_AUTHOR_DATE=d, GIT_COMMITTER_DATE=d)
        r = subprocess.run(["git", "-C", self.root, *args], env=env, capture_output=True, text=True)
        if r.returncode:
            sys.exit(f"seed: git {' '.join(args)}: {r.stderr.strip()}")
        return r.stdout


def tool_events(repo, tool, where, ts):
    """One story tool call: applied to the files, and its hook payloads (tool_name, tool_input, tool_response)."""
    kind = tool[0]
    if kind == "read":
        rel = tool[1]
        text = repo.files.get(rel, "")
        n = text.count("\n")
        return "Read", {"file_path": repo.abs(rel)}, {"type": "text", "file": {"filePath": repo.abs(rel), "content": text, "numLines": n, "startLine": 1, "totalLines": n}}
    if kind == "grep":
        _, pat, path, out = tool
        return "Grep", {"pattern": pat, "path": repo.abs(path), "output_mode": "content"}, {"mode": "content", "content": out, "numLines": out.count("\n") + (1 if out else 0)}
    if kind == "bash":
        _, cmd, out, code = tool
        return "Bash", {"command": cmd}, {"stdout": out, "stderr": "", "interrupted": False, **({"exit_code": code} if code else {})}
    if kind == "edit":
        _, rel, old, new = tool
        repo.edit(rel, old, new, where)
        return "Edit", {"file_path": repo.abs(rel), "old_string": old, "new_string": new}, {"filePath": repo.abs(rel), "oldString": old, "newString": new, "replaceAll": False}
    if kind == "write":
        _, rel, content = tool
        existed = rel in repo.files
        repo.put(rel, content)
        return "Write", {"file_path": repo.abs(rel), "content": content}, {"type": "update" if existed else "create", "filePath": repo.abs(rel)}
    if kind == "commit":
        msg = tool[1]
        repo.git("add", "-A")
        repo.git("commit", "-q", "-m", msg, ts=ts)
        sha = repo.git("rev-parse", "--short", "HEAD").strip() if repo.write else "0000000"
        stat = repo.git("show", "--stat", "--format=", "HEAD").strip().splitlines()
        out = f"[main {sha}] {msg}\n" + (stat[-1].strip() if stat else "")
        return "Bash", {"command": f'git add -A && git commit -m "{msg}"', "description": "Commit the changes"}, {"stdout": out, "stderr": "", "interrupted": False}
    sys.exit(f"seed: {where}: unknown tool {kind}")


class Hooks:
    """ChloeCtx's hooks, called as Claude Code calls them, at a moment of the story."""

    def __init__(self, exe, project, env):
        self.exe, self.project, self.env = exe, project, env

    def run(self, *args, offset=0, stdin=None, timeout=120):
        env = dict(self.env, CHLOECTX_CLOCK_OFFSET=str(offset))
        return subprocess.run([self.exe, *args], cwd=self.project, env=env, input=stdin, capture_output=True, text=True, timeout=timeout)

    def stop(self, store_dir):
        """Stops the project's daemon, and waits until it is gone (it finishes its distillations first)."""
        sock = os.path.join(store_dir, "engram.sock")
        if len(sock) >= 100:
            # The daemon's socket is elsewhere (paths.rs): stop it and give it time.
            self.run("stop", timeout=300)
            time.sleep(2)
            return

        def up():
            s = socket.socket(socket.AF_UNIX)
            try:
                s.connect(sock)
                return True
            except OSError:
                return False
            finally:
                s.close()

        if not up():
            return
        self.run("stop", timeout=300)
        for _ in range(3000):
            if not up():
                return
            time.sleep(0.02)
        sys.exit("seed: the daemon did not stop within a minute")

    def hook(self, event, payload, offset):
        payload = {"hook_event_name": event, "cwd": self.project, **payload}
        r = self.run("hook", event, offset=offset, stdin=json.dumps(payload))
        if r.stderr.strip():
            print(f"  {event}: {r.stderr.strip()[:300]}", file=sys.stderr)
        return r.stdout


def play_local(args, exe, root, project, home):
    notes_by_request = {}
    for s in story.SESSIONS:
        for t in s["turns"]:
            notes_by_request[t["prompt"].strip()[:200]] = t["notes"]
    notes_file = os.path.join(root, "story-notes.json")
    with open(notes_file, "w") as f:
        json.dump(notes_by_request, f)
    env = dict(os.environ, CHLOECTX_HOME=home, CHLOECTX_DEEP="off", CHLOECTX_DISTILL_MODEL="scripted", CHLOECTX_DEMO_NOTES=notes_file, CHLOECTX_RECEIPTS="1", CLAUDE_PROJECT_DIR=project)
    env.pop("CHLOECTX_DISABLE", None)
    if args.embedder:
        env["CHLOECTX_EMBEDDER"] = args.embedder
    hooks = Hooks(exe, project, env)

    # The embedding model: the user's own copy if there is one, else fetched once.
    models = os.path.join(home, "models")
    if not os.path.exists(models) and not args.embedder:
        for mine in (os.path.expanduser("~/.local/share/chloectx/models"), os.path.expanduser("~/.local/share/engram/models")):
            if os.path.isdir(mine):
                os.makedirs(home, exist_ok=True)
                os.symlink(mine, models)
                break
        else:
            print("Fetching the embedding model (129 MB, once)…")
            r = hooks.run("fetch-model", timeout=1800)
            if r.returncode:
                sys.exit(f"seed: chloectx fetch-model: {r.stderr.strip()} (or pass --embedder hash:384)")

    # The store's settings: the scripted distiller, no query planning.
    r = hooks.run("stats", timeout=60)
    try:
        store_dir = os.path.dirname(json.loads(r.stdout)["store"])
    except Exception:
        sys.exit(f"seed: chloectx stats: {r.stdout.strip()} {r.stderr.strip()}")
    hooks.stop(store_dir)
    cfg_path = os.path.join(store_dir, "config.json")
    cfg = json.load(open(cfg_path)) if os.path.exists(cfg_path) else {}
    cfg.setdefault("distill", {}).update({"command": os.path.join(HERE, "scripted_distill.py"), "isolated": False})
    cfg["deep"] = {"mode": "off"}
    cfg.setdefault("receipts", {})["enabled"] = True
    with open(cfg_path, "w") as f:
        json.dump(cfg, f, indent=2)

    repo = Repo(project)
    now = time.time()
    total = sum(len(s["turns"]) for s in story.SESSIONS)
    done = 0
    for si, s in enumerate(story.SESSIONS):
        sid = str(uuid.UUID(int=random.Random(f"harbor-{si}").getrandbits(128), version=4))
        ts = at(s["when"])
        transcript = os.path.join(home, "transcripts", f"{sid}.jsonl")
        os.makedirs(os.path.dirname(transcript), exist_ok=True)
        open(transcript, "a").close()
        for ti, t in enumerate(s["turns"]):
            if ti:
                ts += t["gap"] * 60
            off = int(ts - now)
            where = f"session {si + 1} turn {ti + 1}"
            # A daemon per turn, started at the turn's moment of the story.
            hooks.stop(store_dir)
            if ti == 0:
                hooks.hook("SessionStart", {"session_id": sid, "source": "startup", "transcript_path": transcript}, off)
            hooks.hook("UserPromptSubmit", {"session_id": sid, "prompt": t["prompt"], "transcript_path": transcript}, off)
            for tool in t["tools"]:
                name, inp, resp = tool_events(repo, tool, where, ts)
                base = {"session_id": sid, "tool_name": name, "tool_input": inp, "transcript_path": transcript}
                hooks.hook("PreToolUse", base, off)
                hooks.hook("PostToolUse", {**base, "tool_response": resp}, off)
            hooks.hook("Stop", {"session_id": sid, "last_assistant_message": t["answer"], "stop_hook_active": False, "transcript_path": transcript}, off)
            if ti == len(s["turns"]) - 1:
                hooks.hook("SessionEnd", {"session_id": sid, "reason": "exit"}, off)
            done += 1
            print(f"\r  {done}/{total} turns ({s['when'][:10]})", end="", flush=True)
    hooks.stop(store_dir)
    print()
    r = hooks.run("stats", timeout=60)
    st = json.loads(r.stdout)
    print(f"Store {st['store']}: {st['records']} records in {st['sessions']} sessions, {st['receipts']['count']} receipts, {st['snapshots']['automatic']} snapshots")
    kinds = st.get("by_kind", {})
    print("  " + ", ".join(f"{k} {n}" for k, n in sorted(kinds.items(), key=lambda x: -x[1])))
    return hooks, store_dir


def api(server, key, method, path, body=None):
    req = urllib.request.Request(server.rstrip("/") + path, method=method, data=None if body is None else json.dumps(body).encode(), headers={"content-type": "application/json", **({"authorization": f"Bearer {key}"} if key else {})})
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            return json.loads(r.read() or b"null")
    except urllib.error.HTTPError as e:
        raise SystemExit(f"seed: {method} {path}: HTTP {e.code}: {e.read().decode(errors='replace')[:400]}")


def play_space(args, key):
    """The story into a team space of a cluster, through the ChloeCtx app's API: adds only."""
    app = "/v1/apps/chloectx/v1"
    name = args.space
    have = {s["name"] for s in api(args.server, key, "GET", f"{app}/spaces")["spaces"]}
    if name in have and not args.space_reuse:
        sys.exit(f"seed: the space {name} exists; pick another --space, or pass --space-reuse to add to it")
    api(args.server, key, "POST", f"{app}/spaces", {"name": name, "title": "Harbor payments (demo)", "description": "Three and a half weeks of Claude Code sessions on harbor, a mock order and payments service: refunds, webhooks, migrations, an incident and the corrections along the way. Seeded by chloectx/demo/seed.py."})
    ids = {}
    n_notes = n_ex = 0
    for si, s in enumerate(story.SESSIONS):
        sid = f"cc-{at(s['when'])}"
        ts = at(s["when"])
        for ti, t in enumerate(s["turns"]):
            if ti:
                ts += t["gap"] * 60
            api(args.server, key, "POST", f"{app}/spaces/{name}/exchanges", {"user": t["prompt"], "assistant": t["answer"], "session": sid, "when": ts, "distill": False})
            n_ex += 1
            for n in t["notes"]:
                if n["kind"] == "done":
                    continue
                kind = {"requirement": "todo"}.get(n["kind"], n["kind"])
                body = {"kind": kind, "text": n["text"], "entities": n["entities"], "session": sid, "when": ts + 30}
                old = n.get("replaces")
                # A to-do this turn completed: the note that did it replaces it.
                done = next((d["done"] for d in t["notes"] if d["kind"] == "done"), None)
                if not old and done and kind == "decision":
                    old = done
                if old:
                    rid = next((i for text, i in ids.items() if text.startswith(old)), None)
                    if rid:
                        body["replaces"] = rid
                r = api(args.server, key, "POST", f"{app}/spaces/{name}/notes", body)
                ids[n["text"]] = r.get("id") or r.get("note", {}).get("id")
                n_notes += 1
        print(f"\r  space {name}: {si + 1}/{len(story.SESSIONS)} sessions", end="", flush=True)
    print(f"\nSpace {name}: {n_ex} exchanges, {n_notes} notes")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--dir", default=os.path.expanduser("~/chloectx-demo"), help="where the project and ChloeCtx's home go (default ~/chloectx-demo)")
    ap.add_argument("--chloectx", help="the chloectx binary (default: this checkout's build, else the one on PATH)")
    ap.add_argument("--embedder", help="e.g. hash:384, for a machine without the embedding model")
    ap.add_argument("--fresh", action="store_true", help="remove an earlier demo under --dir first")
    ap.add_argument("--check", action="store_true", help="only check that the story's edits apply")
    ap.add_argument("--no-local", action="store_true", help="do not replay the story into a local store (for --space, or --sync of an earlier demo)")
    ap.add_argument("--server", help="a chloedb-server with the ChloeCtx app, for --space and --sync")
    ap.add_argument("--key-env", default="CHLOEDB_KEY", help="the environment variable holding the server's key (default CHLOEDB_KEY)")
    ap.add_argument("--space", help="write the story into this team space too (needs --server)")
    ap.add_argument("--space-reuse", action="store_true", help="add to --space when it exists")
    ap.add_argument("--sync", action="store_true", help="copy the store into the server's chloectx_memory (needs --server; https, or http on this machine)")
    args = ap.parse_args()

    if args.check:
        repo = Repo("/nonexistent", write=False)
        n = 0
        for si, s in enumerate(story.SESSIONS):
            for ti, t in enumerate(s["turns"]):
                for tool in t["tools"]:
                    tool_events(repo, tool, f"session {si + 1} turn {ti + 1}", 0)
                    n += 1
        notes = sum(len(t["notes"]) for s in story.SESSIONS for t in s["turns"])
        print(f"The story applies: {len(story.SESSIONS)} sessions, {sum(len(s['turns']) for s in story.SESSIONS)} turns, {n} tool calls, {notes} notes")
        return
    if (args.space or args.sync) and not args.server:
        sys.exit("seed: --space and --sync need --server")
    key = os.environ.get(args.key_env, "") if args.server else ""

    root = os.path.abspath(args.dir)
    project, home = os.path.join(root, "harbor"), os.path.join(root, "home")
    if not args.no_local:
        exe = find_chloectx(args.chloectx)
        if os.path.exists(project):
            if not args.fresh:
                sys.exit(f"seed: {project} exists: pass --fresh to replace the demo there")
            env = dict(os.environ, CHLOECTX_HOME=home, CLAUDE_PROJECT_DIR=project)
            subprocess.run([exe, "stop"], cwd=project, env=env, capture_output=True, timeout=300)
            shutil.rmtree(project)
            shutil.rmtree(os.path.join(home, "projects"), ignore_errors=True)
            shutil.rmtree(os.path.join(home, "transcripts"), ignore_errors=True)
        os.makedirs(project)
        repo = Repo(project)
        for rel, text in story.FILES.items():
            repo.put(rel, text)
        repo.git("init", "-q", "-b", "main")
        repo.git("remote", "add", "origin", "git@github.com:acme/harbor.git")
        repo.git("add", "-A")
        repo.git("commit", "-q", "-m", "harbor 0.14.0", ts=at("2026-10-16 17:30"))
        print(f"Project {project}, ChloeCtx home {home}")
        play_local(args, exe, root, project, home)
    if args.sync:
        if not os.path.exists(project):
            sys.exit(f"seed: --sync needs the local demo at {project} (leave out --no-local)")
        env = dict(os.environ, CHLOECTX_HOME=home, CLAUDE_PROJECT_DIR=project)
        if args.embedder:
            env["CHLOECTX_EMBEDDER"] = args.embedder
        hooks = Hooks(find_chloectx(args.chloectx), project, env)
        r = hooks.run("sync", "setup", "--url", args.server, "--token-stdin", stdin=key, timeout=60)
        print(r.stdout.strip() or r.stderr.strip())
        for attempt in range(3):
            # setup restarts the daemon, and a push can reach the one that is leaving.
            r = hooks.run("sync", "push", timeout=600)
            if r.returncode == 0:
                break
            time.sleep(2)
        print(f"sync push: {json.loads(r.stdout).get('pushed')} records" if r.returncode == 0 else r.stderr.strip())
    if args.space:
        play_space(args, key)
    if not args.no_local:
        print(f"\nTo see it: CHLOECTX_HOME={home} chloectx app --server <chloedb-server> --local-stores")


if __name__ == "__main__":
    main()
