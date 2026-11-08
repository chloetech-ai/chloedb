#!/usr/bin/env python3
"""Observational memory (Mastra's method) on BEAM, in our harness, for a
head-to-head with Engram on the same models and judge.

Mastra's harness is not public, so this rebuilds its memory from its own
prompts: the observer's system prompt, guidelines and output format, and the
context preamble the answering agent reads, are taken verbatim from an
installed @mastra/memory package (Apache-2.0, Copyright Kepler Software,
Inc.), read as text and never run. The schedule follows its defaults: raw
messages stay in context until they pass 30,000 tokens, then the oldest are
observed in batches of at most 10,000 tokens until 6,000 remain (buffer 0.2,
activation 0.8). Not rebuilt: the reflector (it starts at 40,000 tokens of
observations; the script reports when a unit passes that) and the recall
tool the agent can call.

  npm install @mastra/memory@1.41.0   (in some directory D)
  python3 om_baseline.py --data beam_100k.jsonl --mastra D/node_modules/@mastra/memory --model haiku --out runs/om
  python3 bench_eval.py --bench beam --data beam_100k.jsonl --config om=runs/om/contexts.jsonl --answer-prompt engram ...
"""

import argparse
import glob
import json
import os
import re
import sys
import threading
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from reader_eval import call  # noqa: E402

MESSAGE_TOKENS = 30_000
KEEP_TOKENS = 6_000
BATCH_TOKENS = 10_000
REFLECT_AT = 40_000


def tokens(text):
    return len(text) // 4


def literal(src, name):
    """The text of `const NAME = `...`;` in the bundle."""
    m = re.search(r"const " + name + r" = `(.*?)`;\n", src, re.S)
    if not m:
        raise SystemExit(f"{name} not found in the package")
    return m.group(1)


def mastra_prompts(pkg):
    path = sorted(glob.glob(os.path.join(pkg, "dist", "src-*.js")))
    src = next(open(p).read() for p in path if "OBSERVER_GUIDELINES" in open(p).read())
    extraction = literal(src, "OBSERVER_EXTRACTION_INSTRUCTIONS")
    guidelines = literal(src, "OBSERVER_GUIDELINES")
    fmt = re.search(r"function buildObserverOutputFormat\(extractors\) \{\n\treturn `(.*?)\$\{buildExtractorOutputSections", src, re.S).group(1)
    sections = re.search(r"extractors === void 0 \? `(.*?)` : \"\"\)\}`;", src, re.S).group(1)
    system = (
        "You are the memory consciousness of an AI assistant. Your observations will be the ONLY information the assistant has about past interactions with this user.\n\n"
        f"Extract observations that will help the assistant remember:\n\n{extraction}\n\n"
        "=== OUTPUT FORMAT ===\n\nYour output MUST use XML tags to structure the response. This allows the system to properly parse and manage memory over time.\n\n"
        f"{fmt}{sections}\n\n=== GUIDELINES ===\n\n{guidelines}\n\n"
        "=== IMPORTANT: THREAD ATTRIBUTION ===\n\nDo NOT add thread identifiers, thread IDs, or <thread> tags to your observations.\nThread attribution is handled externally by the system.\nSimply output your observations without any thread-related markup.\n\n"
        "Remember: These observations are the assistant's ONLY memory. Make them count.\n\n"
        "User messages are extremely important. If the user asks a question or gives a new task, make it clear in <current-task> that this is the priority. "
        "If the assistant needs to respond to the user, indicate in <suggested-response> that it should pause for user reply before continuing other tasks."
    )
    context = literal(src, "OBSERVATION_CONTEXT_PROMPT") + "\n\n" + literal(src, "OBSERVATION_CONTEXT_INSTRUCTIONS")
    return system, context


def when(ts):
    """BEAM's "2024/03/15 (Fri) 09:00", or ISO 8601."""
    if not ts:
        return None
    m = re.match(r"(\d{4})/(\d{2})/(\d{2})(?: \(\w+\))?(?: (\d{2}):(\d{2}))?", ts)
    if m:
        y, mo, d, h, mi = (int(x) if x else 0 for x in m.groups())
        return datetime(y, mo, d, h, mi, tzinfo=timezone.utc)
    return datetime.fromisoformat(ts.replace("Z", "+00:00"))


def format_messages(msgs):
    """Mastra's observer formatting: a date line, then `Role (time): text`,
    the time shown when it changes."""
    out, prev_date, prev_time = [], None, None
    for m in msgs:
        d = when(m.get("timestamp"))
        date = f"{d.strftime('%b')} {d.day} {d.year}" if d else ""
        time = d.strftime("%I:%M %p").lstrip("0") if d else ""
        if date and date != prev_date:
            out.append(f"{date}:")
            prev_date, prev_time = date, None
        label = f" ({time})" if time and time != prev_time else ""
        out.append(f"{m['role'].capitalize()}{label}: {m['text']}")
        prev_time = time or prev_time
    return "\n".join(out)


def relative_days(d):
    """Mastra's formatRelativeDays."""
    if d < 0:
        f = -d
        if f == 1: return "tomorrow"
        if f < 7: return f"in {f} days"
        if f < 14: return "in 1 week"
        if f < 30: return f"in {f // 7} weeks"
        if f < 60: return "in 1 month"
        if f < 365: return f"in {f // 30} months"
        return f"in {f // 365} year{'s' if f >= 730 else ''}"
    if d == 0: return "today"
    if d == 1: return "yesterday"
    if d < 7: return f"{d} days ago"
    if d < 14: return "1 week ago"
    if d < 30: return f"{d // 7} weeks ago"
    if d < 60: return "1 month ago"
    if d < 365: return f"{d // 30} months ago"
    return f"{d // 365} year{'s' if d >= 730 else ''} ago"


def gap(days):
    """Mastra's formatGapBetweenDates."""
    if days <= 1: return None
    if days < 7: return f"[{days} days later]"
    if days < 14: return "[1 week later]"
    if days < 30: return f"[{days // 7} weeks later]"
    if days < 60: return "[1 month later]"
    return f"[{days // 30} months later]"


def with_relative_dates(observations, today):
    """Mastra's addRelativeTimeToObservations for "Date:" headers: each gets
    how long before the question it was, with the gap since the one before."""
    out, prev = [], None
    for line in observations.split("\n"):
        m = re.match(r"^(Date:[ \t]*)([A-Z][a-z]{2,8}\.? \d{1,2}, \d{4})(.*)$", line)
        if m:
            try:
                d = datetime.strptime(m.group(2).replace(".", "")[:3] + " " + m.group(2).split(" ", 1)[1], "%b %d, %Y").date()
            except ValueError:
                d = None
            if d:
                if prev and gap((d - prev).days):
                    out.append(f"\n{gap((d - prev).days)}\n")
                line = f"{m.group(1)}{m.group(2)} ({relative_days((today - d).days)}){m.group(3)}"
                prev = d
        out.append(line)
    return "\n".join(out)


def observe(system, observations, batch, model):
    prompt = ""
    if observations:
        prompt += f"## Previous Observations\n\n{observations}\n\n---\n\nDo not repeat these existing observations. Your new observations will be appended to the existing observations.\n\n"
    prompt += f"## New Message History to Observe\n\n{format_messages(batch)}\n\n---\n\n## Your Task\n\nExtract new observations from the message history above. Do not repeat observations that are already in the previous observations. Add your new observations in the format specified in your instructions."
    text, cost, _ = call(model, system, prompt)
    m = re.findall(r"^[ \t]*<observations>([\s\S]*?)^[ \t]*</observations>", text, re.M)
    new = "\n".join(x.strip() for x in m if x.strip()) if m else "\n".join(l for l in text.splitlines() if re.match(r"^\s*([-*]|\d+\.)\s", l))
    return new, cost


def run_unit(u, system, model, out_dir):
    path = os.path.join(out_dir, "obs", u["unit_id"] + ".json")
    if os.path.exists(path):
        return json.load(open(path))
    observations, cost, calls, pending = "", 0.0, 0, []
    for t in u["turns"]:
        pending.append(t)
        if sum(tokens(m["text"]) for m in pending) <= MESSAGE_TOKENS:
            continue
        while sum(tokens(m["text"]) for m in pending) > KEEP_TOKENS:
            batch, size = [], 0
            while pending and (not batch or size + tokens(pending[0]["text"]) <= BATCH_TOKENS) and sum(tokens(m["text"]) for m in pending) > KEEP_TOKENS:
                size += tokens(pending[0]["text"])
                batch.append(pending.pop(0))
            new, c = observe(system, observations, batch, model)
            observations = (observations + "\n\n" + new).strip()
            cost, calls = cost + c, calls + 1
    r = {"unit": u["unit_id"], "observations": observations, "tail": pending, "cost_usd": cost, "calls": calls,
         "observation_tokens": tokens(observations), "tail_tokens": sum(tokens(m["text"]) for m in pending),
         "needs_reflection": tokens(observations) > REFLECT_AT}
    os.makedirs(os.path.dirname(path), exist_ok=True)
    json.dump(r, open(path, "w"))
    print(f"  {u['unit_id']}: {calls} observer calls, {r['observation_tokens']} observation tokens, tail {r['tail_tokens']}, ${cost:.3f}", flush=True)
    return r


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", required=True)
    ap.add_argument("--mastra", required=True, help="the installed @mastra/memory package directory")
    ap.add_argument("--model", default="haiku")
    ap.add_argument("--out", required=True)
    ap.add_argument("--workers", type=int, default=4)
    args = ap.parse_args()
    system, context = mastra_prompts(args.mastra)
    units = [json.loads(l) for l in open(args.data)]
    os.makedirs(args.out, exist_ok=True)
    lock = threading.Lock()
    with ThreadPoolExecutor(args.workers) as ex:
        results = list(ex.map(lambda u: run_unit(u, system, args.model, args.out), units))
    with lock, open(os.path.join(args.out, "contexts.jsonl"), "w") as f:
        for u, r in zip(units, results):
            for q in u["questions"]:
                today = when(q.get("question_date") or u["turns"][-1]["timestamp"]).date()
                ctx = f"{context}\n\n<observations>\n{with_relative_dates(r['observations'], today)}\n</observations>"
                if r["tail"]:
                    ctx += f"\n\nThe most recent messages, not yet observed:\n\n{format_messages(r['tail'])}"
                f.write(json.dumps({"qid": q["qid"], "context": ctx}) + "\n")
    total = sum(r["cost_usd"] for r in results)
    print(f"{len(results)} units, observer ${total:.2f}; {sum(r['needs_reflection'] for r in results)} past the reflection threshold; "
          f"mean context {sum(r['observation_tokens'] + r['tail_tokens'] for r in results) / len(results):.0f} tokens")


if __name__ == "__main__":
    main()
