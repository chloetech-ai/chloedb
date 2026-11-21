#!/usr/bin/env python3
"""The plain-file baseline for the personal-memory benchmark: a memory file
about the user, kept the way a chat app's "memory" or a hand-kept
`about-me.md` is kept.

For each history a model reads the conversations in order, a few sessions at
a time, and rewrites one file of what is worth remembering about the user
(facts, preferences, plans, events, with their dates), never longer than
--chars characters. The file after the last session is the context the
readers get. Its cost is logged, so the benchmark can say what keeping the
file costs next to ChloeCtx, which stores without a model call.

  python3 scripts/profile_file.py --data /home/user/data/memory/longmemeval_s.jsonl \
      --every 5 --chars 4000 --model haiku --out results/v4/persona/profile_4000.jsonl

Calls go through the `claude` CLI in print mode with no tools. The output
holds one line per question: {"qid", "context", "cost", "calls", "seconds"},
and a run resumes from it.
"""

import argparse
import concurrent.futures as cf
import json
import os
import subprocess
import threading
import time
from collections import defaultdict

SYSTEM = (
    "You keep a memory file about one user for an AI assistant. The file is all the assistant "
    "will know about the user in later conversations."
)

PROMPT = """Here is the memory file as it stands.

<memory_file>
{file}
</memory_file>

Here are the user's next conversations with the assistant, in order, each under its date.

<conversations>
{sessions}
</conversations>

Rewrite the memory file so that it holds everything about the user worth remembering for later conversations: facts about them and their life, people and things they mention, preferences and tastes, plans, events and purchases with their dates, numbers, and anything that changed (keep the newer value and say when it changed). Leave out general knowledge the assistant gave that says nothing about the user.

The file must stay under {chars} characters. When it would not fit, merge and shorten; keep dates, names and numbers. Reply with the new file only, as plain Markdown with no preamble and no front matter."""

SHRINK = """This memory file is {n} characters long; it must be under {chars}. Rewrite it under {chars} characters by merging and shortening, keeping dates, names and numbers. Reply with the new file only.

<memory_file>
{file}
</memory_file>"""


def call(model, prompt, timeout=600, retries=4):
    cmd = ["claude", "-p", "--model", model, "--system-prompt", SYSTEM, "--tools", "", "--no-session-persistence", "--output-format", "json"]
    last = None
    for attempt in range(retries):
        try:
            p = subprocess.run(cmd, input=prompt, capture_output=True, text=True, timeout=timeout, cwd=os.environ.get("READER_CWD", "/tmp"))
            d = json.loads(p.stdout)
            if d.get("is_error") or not d.get("result"):
                raise RuntimeError(str(d.get("result"))[:200])
            return d["result"].strip(), d.get("total_cost_usd", 0.0), d.get("duration_api_ms", 0) / 1000
        except Exception as e:  # noqa: BLE001 - retried, then reported
            last = e
            time.sleep(2 * 2**attempt)
    raise RuntimeError(f"{model} failed: {last}")


def sessions(turns):
    order, by = [], defaultdict(list)
    for t in turns:
        if t["session_id"] not in by:
            order.append(t["session_id"])
        by[t["session_id"]].append(t)
    out = []
    for sid in order:
        ts = by[sid]
        lines = [f"### Conversation on {ts[0].get('timestamp') or 'unknown date'}"]
        lines += [f"({t['role']}) {t['text']}" for t in ts]
        out.append("\n".join(lines))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", required=True)
    ap.add_argument("--every", type=int, default=5)
    ap.add_argument("--chars", type=int, default=4000)
    ap.add_argument("--per-call", type=int, default=5, help="sessions read per call")
    ap.add_argument("--model", default="haiku")
    ap.add_argument("--workers", type=int, default=8)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    units = []
    with open(args.data) as f:
        for line in f:
            units.append(json.loads(line))
    units = units[:: args.every]
    done = set()
    if os.path.exists(args.out):
        with open(args.out) as f:
            done = {json.loads(l)["qid"] for l in f}
    lock = threading.Lock()

    def build(u):
        qid = u["questions"][0]["qid"]
        ss = sessions(u["turns"])
        file, cost, secs, calls = "(empty)", 0.0, 0.0, 0
        for i in range(0, len(ss), args.per_call):
            text, c, s = call(args.model, PROMPT.format(file=file, sessions="\n\n".join(ss[i : i + args.per_call]), chars=args.chars))
            file, cost, secs, calls = text, cost + c, secs + s, calls + 1
            for _ in range(2):  # the model overshoots the limit now and then
                if len(file) <= args.chars:
                    break
                text, c, s = call(args.model, SHRINK.format(n=len(file), chars=args.chars, file=file))
                file, cost, secs, calls = text, cost + c, secs + s, calls + 1
        row = {"qid": qid, "context": file, "cost": cost, "calls": calls, "seconds": secs, "sessions": len(ss)}
        with lock:
            with open(args.out, "a") as f:
                f.write(json.dumps(row) + "\n")
        return qid

    todo = [u for u in units if u["questions"][0]["qid"] not in done]
    print(f"{len(todo)} histories to build, {len(done)} done", flush=True)
    with cf.ThreadPoolExecutor(args.workers) as ex:
        for n, fut in enumerate(cf.as_completed([ex.submit(build, u) for u in todo]), 1):
            fut.result()
            if n % 10 == 0:
                print(f"{n}/{len(todo)}", flush=True)


if __name__ == "__main__":
    main()
