#!/usr/bin/env python3
"""Reader contexts from memory kept in files, for scripts/reader_eval.py and
scripts/bench_eval.py: what a CLAUDE.md, or Claude Code's own auto memory,
would hold after the history's sessions.

  python3 scripts/file_memory_contexts.py claudemd --data locomo.jsonl \\
      --work /home/claude/bench/filemem/locomo --out results/baselines/locomo/claudemd.contexts.jsonl
  python3 scripts/file_memory_contexts.py automem --data longmemeval_s.jsonl --every 5 ...

Each history (one LongMemEval question's sessions, one BEAM or LoCoMo
conversation) is replayed session by session in time order. A BEAM session
here is one day of the conversation (its exchange groups are one or two
messages each, all on the same day).

claudemd  One file, rewritten after each session by one model call that gets
          the file as it stands and the session's transcript, and returns the
          whole file: what a later session should know. The way a careful
          user keeps a project's CLAUDE.md, done by the model each time.
          Every question of the history gets the final file.
automem   Claude Code itself (`claude -p`, auto memory on, as it is by
          default) in a project folder of its own, with HOME of its own, is
          shown each session's transcript as a conversation it had and asked
          to remember what matters, with Read/Write/Edit/Glob/Grep as its
          only tools. Its memory folder (~/.claude/projects/<project>/memory:
          MEMORY.md, the index Claude Code loads into each session, and the
          files it points to) is the context, MEMORY.md first.

Both use --model (default haiku) and keep each call's cost. Finished
histories are skipped on a rerun, and a history resumes after its last
finished session. Run under a clean environment (the calls themselves get
only PATH, HOME and ANTHROPIC_BASE_URL).
"""

import argparse
import concurrent.futures as cf
import glob
import json
import os
import random
import re
import shutil
import subprocess
import sys
import time
from collections import OrderedDict

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from reader_eval import call  # noqa: E402

LIMIT_RE = re.compile(r"hit your .{0,20}limit|usage limit", re.I)

CLAUDEMD_SYSTEM = (
    "You keep CLAUDE.md, the memory file that is loaded at the start of every future session with this user. "
    "Reply with the complete updated file and nothing else."
)

CLAUDEMD_PROMPT = """Here is CLAUDE.md as it stands:

<claude_md>
{file}
</claude_md>

Here is the session that just ended:

<session>
{session}
</session>

Update CLAUDE.md with what a later session should know from this one: facts about the people in it, what they said about themselves, their plans, preferences, decisions and changes of mind, events and when they happened (as dates, not "yesterday"), and anything they would expect to be remembered. Keep what is still true, correct what changed, and keep the file concise: at most about 200 lines. Reply with the whole file."""

AUTOMEM_PROMPT = """The transcript below is a conversation you had on {date}. It is over now.

<transcript>
{session}
</transcript>

Remember what will matter in later conversations, as you normally keep your memory. Do not reply with anything but a one-line summary of what you saved."""

AUTOMEM_TOOLS = "Read,Write,Edit,Glob,Grep"


def sessions(unit, by_day):
    """[(session key, date, transcript text)] in time order."""
    order, by = [], OrderedDict()
    for t in unit["turns"]:
        key = (t.get("timestamp") or "")[:10] if by_day else t["session_id"]
        if key not in by:
            by[key] = []
            order.append(key)
        by[key].append(t)
    out = []
    for key in order:
        ts = by[key]
        date = ts[0].get("timestamp") or "an unknown date"
        lines = [f"### Conversation on {date}"] + [f"({t['role']}) {t['text']}" for t in ts]
        out.append((key, date, "\n".join(lines)))
    return out


def clean_env(home):
    env = {"PATH": os.environ.get("BENCH_PATH", "/opt/node22/bin:/usr/local/bin:/usr/bin:/bin"), "HOME": home}
    if os.environ.get("ANTHROPIC_BASE_URL"):
        env["ANTHROPIC_BASE_URL"] = os.environ["ANTHROPIC_BASE_URL"]
    return env


def automem_call(model, prompt, home, cwd, retries=4):
    cmd = ["claude", "-p", "--model", model, "--output-format", "json", "--no-session-persistence",
           "--allowedTools", AUTOMEM_TOOLS, "--tools", AUTOMEM_TOOLS, "--permission-mode", "acceptEdits",
           "--max-budget-usd", "1"]
    last = None
    for attempt in range(retries):
        p = subprocess.run(cmd, input=prompt, capture_output=True, text=True, timeout=900, cwd=cwd, env=clean_env(home))
        try:
            d = json.loads(p.stdout)
        except ValueError:
            last = (p.stdout + p.stderr)[-300:]
            time.sleep(5 * 2**attempt)
            continue
        if d.get("is_error") and LIMIT_RE.search(str(d.get("result"))):
            print("  usage limit; waiting 10 minutes", file=sys.stderr)
            time.sleep(600)
            continue
        if d.get("is_error"):
            last = str(d.get("result"))[:300]
            time.sleep(5 * 2**attempt)
            continue
        return d.get("result") or "", d.get("total_cost_usd", 0.0), d.get("num_turns", 0)
    raise RuntimeError(f"automem call failed: {last}")


def memory_files(home):
    """Claude Code's memory folder: MEMORY.md first, then the other files by name."""
    files = sorted(glob.glob(os.path.join(home, ".claude", "projects", "*", "memory", "**", "*.md"), recursive=True))
    files.sort(key=lambda p: (os.path.basename(p) != "MEMORY.md", p))
    return files


def memory_context(home):
    parts = []
    for p in memory_files(home):
        rel = p.split("/memory/", 1)[1]
        with open(p, errors="replace") as f:
            parts.append(f"## {rel}\n{f.read().strip()}")
    return "\n\n".join(parts)


def run_unit(kind, unit, args):
    udir = os.path.join(args.work, unit["unit_id"].replace("/", "_"))
    state_path = os.path.join(udir, "state.json")
    os.makedirs(udir, exist_ok=True)
    state = {"done": [], "cost_usd": 0.0, "calls": 0, "file": ""}
    if os.path.exists(state_path):
        with open(state_path) as f:
            state = json.load(f)
    if state.get("finished"):
        return state
    by_day = unit.get("dataset", "").startswith("beam") or args.by_day
    home = os.path.join(udir, "home")
    cwd = os.path.join(udir, "project")
    os.makedirs(home, exist_ok=True)
    os.makedirs(cwd, exist_ok=True)
    t0 = time.time()
    for key, date, text in sessions(unit, by_day):
        if key in state["done"]:
            continue
        if kind == "claudemd":
            prompt = CLAUDEMD_PROMPT.format(file=state["file"] or "(empty)", session=text)
            out, cost, _ = call(args.model, CLAUDEMD_SYSTEM, prompt)
            state["file"] = re.sub(r"^```\w*\n|\n```\s*$", "", out.strip())
        else:
            out, cost, _ = automem_call(args.model, AUTOMEM_PROMPT.format(date=date, session=text), home, cwd)
            state["file"] = memory_context(home)
        state["cost_usd"] += cost
        state["calls"] += 1
        state["done"].append(key)
        with open(state_path + ".tmp", "w") as f:
            json.dump(state, f)
        os.replace(state_path + ".tmp", state_path)
    state["finished"] = True
    state["seconds"] = state.get("seconds", 0) + time.time() - t0
    with open(state_path, "w") as f:
        json.dump(state, f)
    print(f"  {unit['unit_id']}: {state['calls']} sessions, {len(state['file'])} chars, ${state['cost_usd']:.3f}", file=sys.stderr)
    return state


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("kind", choices=("claudemd", "automem"))
    ap.add_argument("--data", required=True)
    ap.add_argument("--work", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--model", default="haiku")
    ap.add_argument("--every", type=int, default=1, help="every n-th history (LongMemEval: every n-th question)")
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--workers", type=int, default=6)
    ap.add_argument("--by-day", action="store_true", help="a session is a day of the conversation (always so for BEAM)")
    args = ap.parse_args()

    units = [json.loads(l) for i, l in enumerate(open(args.data)) if i % args.every == 0]
    if args.limit:
        units = units[: args.limit]
    order = list(range(len(units)))
    random.Random(7).shuffle(order)
    os.makedirs(args.work, exist_ok=True)
    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    states = {}
    with cf.ThreadPoolExecutor(args.workers) as ex:
        futs = {ex.submit(run_unit, args.kind, units[i], args): i for i in order}
        for fut in cf.as_completed(futs):
            u = units[futs[fut]]
            try:
                states[u["unit_id"]] = fut.result()
            except Exception as e:  # noqa: BLE001 - reported; a rerun resumes it
                print(f"  {u['unit_id']} failed: {e}", file=sys.stderr)
    with open(args.out, "w") as f:
        for u in units:
            s = states.get(u["unit_id"])
            if not s:
                continue
            for q in u["questions"]:
                f.write(json.dumps({"qid": q["qid"], "context": s["file"], "unit": u["unit_id"]}) + "\n")
    total = sum(s["cost_usd"] for s in states.values())
    summary = {"kind": args.kind, "model": args.model, "histories": len(states), "of": len(units),
               "sessions": sum(s["calls"] for s in states.values()), "cost_usd": total,
               "mean_chars": sum(len(s["file"]) for s in states.values()) / max(1, len(states))}
    with open(os.path.splitext(args.out)[0].removesuffix(".contexts") + ".summary.json", "w") as f:
        json.dump(summary, f, indent=1)
    print(json.dumps(summary), file=sys.stderr)


if __name__ == "__main__":
    main()
