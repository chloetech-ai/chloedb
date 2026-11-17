#!/usr/bin/env python3
"""Answers from Letta's own agent, the way Letta ran LoCoMo ("Benchmarking AI
Agent Memory: Is a Filesystem All You Need?", Aug 2025;
github.com/letta-ai/letta-leaderboard, leaderboard/locomo/locomo_benchmark.py),
for scripts/reader_eval.py and scripts/bench_eval.py (`name=answers:FILE`).

  BENCH_ANTHROPIC_API_KEY=... letta-venv/bin/python -I eval/competitors/letta/letta_answers.py \\
      --data locomo.jsonl --out results/baselines/locomo/letta.answers.jsonl

Server: Letta 0.16.8 (`letta/letta:0.16.8`, May 2026), the last release of
the Python server that experiment ran on; Letta has since replaced it with
Letta Code. Each history becomes a folder with one file per session (a BEAM
session here is a day), its first line "Timestamp: <date>" and then
"<speaker>: <text>" lines, as Letta's script writes them; its chunks are
embedded whole (chunk size 100000, as in Letta's run) by
jinaai/jina-embeddings-v2-small-en, which reads 8192 tokens (embed_server.py
--model; Letta used OpenAI's text-embedding-3-large). Each question gets a fresh agent with that folder
attached, Letta's LoCoMo system prompt (locomo_agent.txt), the two tools
Letta's run gave it, `semantic_search_files` (its `search_files`) and
`answer_question`, and Letta's tool rules: it must start with a search, may
search again, and stops when it calls `answer_question`, whose argument is
the answer. The agent's model is Haiku
5.5 (`anthropic/claude-haiku-5-5`); at most --max-steps steps a question.

LongMemEval and BEAM get the same agent with the system prompt's speakers
read as the user and the assistant, the question's date, and (BEAM) the
instruction to answer fully rather than in a phrase, since BEAM grades
instruction and preference following on the reply itself.

Cost: the response's usage at Haiku 5.5's list price (--price-in /
--price-out). Rows: {qid, answer, steps, tools, cost_reader, reader_tokens,
context_chars (characters the tools returned)}. A rerun skips answered
questions.
"""

import argparse
import concurrent.futures as cf
import json
import os
import random
import re
import sys
import threading
import time
from collections import OrderedDict

from letta_client import Letta

HERE = os.path.dirname(os.path.abspath(__file__))

# letta-leaderboard, leaderboard/locomo/locomo_agent.txt.
LOCOMO_SYSTEM = """<system_prompt>
You are tasked with answering questions about conversation history. The conversation history will be provided as a series of files.
Read through the conversation context carefully before answering any questions.
You can use search_files tools to gather information from the conversation context.
Please provide precise answers to the questions, do not hallucinate, and note the time as well! The timestamp of a conversation is in the file name. Always refer to a file to answer the question, do not make up information.
Avoid general searches, like just searching for a person's name.
Try directly search the question in the conversation first, before you try some other queries.
Your final answer_question tool call should be a precise and concise answer to the question, do not include any other information.
When answering a question about time, use a specific time. For example, " last year", "this month", "this summer", is not precise, "June 2025" is precise.
If the memories contain contradictory information, prioritize the most recent memory
If there is a question about time references (like "last year", "two months ago", etc.), calculate the actual date based on the memory timestamp. For example, if a memory from 4 May 2022 mentions "went to India last year," then the trip occurred in 2021.
Always convert relative time references to specific dates, months, or years. For example, convert "last year" to "2022" or "two months ago" to "March 2023" based on the memory timestamp. Ignore the reference while answering the question.
Focus only on the content of the memories from both speakers. Do not confuse character names mentioned in memories with the actual users who created those memories.
</system_prompt>"""

CHAT_NOTE = "\nThe conversations are between the user and an AI assistant; questions are asked by that user, so \"I\" and \"my\" in a question mean the user."
BEAM_NOTE = "\nYour final answer_question call answers the user's message fully, as the assistant would reply to it, following any instructions and preferences the user gave in earlier conversations."

ANSWER_TOOL = '''def answer_question(answer: str) -> str:
    """
    Returns the provided answer to the user.
    Your answer should be as precise as possible. For example, "this month" is not precise, "June 2025" is precise.

    Args:
        answer (str): The answer to return.

    Returns:
        str: The provided answer.
    """
    return answer
'''

# Letta's run: OpenAI text-embedding-3-large, chunk size 100000 (a chunk is a
# whole session file). Here a local embedder that also reads 8192 tokens, so
# a session is embedded whole rather than cut at 512 tokens (embed_server.py).
EMBEDDING = {"embedding_model": "jinaai/jina-embeddings-v2-small-en", "embedding_endpoint_type": "openai",
             "embedding_endpoint": "http://127.0.0.1:8090/v1", "embedding_dim": 512, "embedding_chunk_size": 100000}


def session_files(unit, by_day):
    order, by = [], OrderedDict()
    for t in unit["turns"]:
        key = (t.get("timestamp") or "")[:10] if by_day else t["session_id"]
        if key not in by:
            by[key] = []
            order.append(key)
        by[key].append(t)
    out = []
    for i, key in enumerate(order):
        ts = by[key]
        stamp = ts[0].get("timestamp") or f"session {i + 1}"
        name = f"{i + 1:03d}_" + re.sub(r"[^A-Za-z0-9]+", "-", stamp).strip("-") + ".txt"
        body = f"Timestamp: {stamp}\n" + "\n".join(f"{t['role']}: {t['text']}" for t in ts) + "\n"
        out.append((name, body))
    return out


def ensure_folder(c, unit, work, by_day, lock):
    path = os.path.join(work, "folders.json")
    with lock:
        known = json.load(open(path)) if os.path.exists(path) else {}
        if unit["unit_id"] in known:
            return known[unit["unit_id"]]
    fdir = os.path.join(work, "files", re.sub(r"[^A-Za-z0-9_-]", "_", unit["unit_id"]))
    os.makedirs(fdir, exist_ok=True)
    folder = c.folders.create(name=f"bench-{unit['unit_id']}-{int(time.time())}", embedding_config=EMBEDDING,
                              description="The conversation history, one file per session, each starting with its timestamp.")
    for name, body in session_files(unit, by_day):
        p = os.path.join(fdir, name)
        with open(p, "w") as f:
            f.write(body)
        with open(p, "rb") as f:
            c.folders.files.upload(folder.id, file=f)
    while True:
        files = list(c.folders.files.list(folder.id, limit=1000))
        if all(getattr(x, "processing_status", "completed") in ("completed", "error") for x in files):
            break
        time.sleep(2)
    with lock:
        known = json.load(open(path)) if os.path.exists(path) else {}
        known[unit["unit_id"]] = folder.id
        json.dump(known, open(path, "w"))
    return folder.id


def answer(c, q, unit, folder_id, args, tool_id):
    beam = unit.get("dataset", "").startswith("beam")
    system = LOCOMO_SYSTEM
    if unit.get("dataset") != "locomo":
        system += CHAT_NOTE + (BEAM_NOTE if beam else "")
    agent = c.agents.create(
        name=f"q-{re.sub(r'[^A-Za-z0-9]', '-', q['qid'])[:40]}-{random.randrange(1 << 30)}",
        agent_type="letta_v1_agent", system=system, model=args.model, embedding_config=EMBEDDING,
        # Haiku 5.5 takes only its default temperature; Letta 0.16.8 sends no
        # thinking setting for it, so the model's default (adaptive) applies.
        model_settings={"provider_type": "anthropic", "temperature": 1.0, "max_output_tokens": 4096},
        include_base_tools=False, include_base_tool_rules=False,
        tool_ids=[tool_id], tools=["semantic_search_files"],
        tool_rules=[{"type": "run_first", "tool_name": "semantic_search_files"},
                    {"type": "continue_loop", "tool_name": "semantic_search_files"},
                    {"type": "exit_loop", "tool_name": "answer_question"}],
        folder_ids=[folder_id],
    )
    try:
        text = q["question"] if not q.get("question_date") else f"(Today is {q['question_date']}.) {q['question']}"
        r = c.agents.messages.create(agent.id, messages=[{"role": "user", "content": text}], max_steps=args.max_steps)
        ans, tools, chars = None, [], 0
        for m in r.messages:
            kind = getattr(m, "message_type", "")
            if kind == "tool_call_message":
                tc = m.tool_call
                tools.append(tc.name)
                if tc.name == "answer_question":
                    try:
                        ans = json.loads(tc.arguments).get("answer")
                    except (ValueError, AttributeError):
                        ans = tc.arguments
            elif kind == "tool_return_message":
                chars += len(str(getattr(m, "tool_return", "") or ""))
            elif kind == "assistant_message" and ans is None:
                ans = str(getattr(m, "content", "") or "")
        u = r.usage
        tin, tout = (u.prompt_tokens or 0), (u.completion_tokens or 0)
        return {"qid": q["qid"], "answer": ans or "", "steps": u.step_count, "tools": tools, "reader_tokens": tin,
                "output_tokens": tout, "cost_reader": tin * args.price_in / 1e6 + tout * args.price_out / 1e6, "context_chars": chars}
    finally:
        try:
            c.agents.delete(agent.id)
        except Exception:  # noqa: BLE001 - a leftover agent costs nothing
            pass


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--work", default="/home/claude/bench/letta")
    ap.add_argument("--url", default="http://127.0.0.1:8283")
    ap.add_argument("--model", default="anthropic/claude-haiku-5-5")
    ap.add_argument("--every", type=int, default=1)
    ap.add_argument("--limit", type=int, default=0, help="first n questions of each history (a trial)")
    ap.add_argument("--max-steps", type=int, default=10)
    ap.add_argument("--workers", type=int, default=6)
    ap.add_argument("--skip-abstention", action="store_true")
    ap.add_argument("--price-in", type=float, default=0.10)
    ap.add_argument("--price-out", type=float, default=0.50)
    args = ap.parse_args()
    os.makedirs(args.work, exist_ok=True)
    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    c = Letta(base_url=args.url, timeout=600)
    tool = c.tools.upsert(source_code=ANSWER_TOOL)
    units = [json.loads(l) for i, l in enumerate(open(args.data)) if i % args.every == 0]
    done = set()
    if os.path.exists(args.out):
        done = {json.loads(l)["qid"] for l in open(args.out)}
    lock, wlock = threading.Lock(), threading.Lock()
    jobs = []
    for u in units:
        qs = [q for q in u["questions"] if not (args.skip_abstention and q["abstention"])]
        if args.limit:
            qs = qs[: args.limit]
        jobs += [(u, q) for q in qs if q["qid"] not in done]
    random.Random(7).shuffle(jobs)
    print(f"{len(jobs)} questions to answer ({len(done)} done)", file=sys.stderr)
    folders = {}
    for u in units:  # folders first, one at a time (embedding is the CPU's work)
        if any(j[0] is u for j in jobs):
            folders[u["unit_id"]] = ensure_folder(c, u, args.work, u.get("dataset", "").startswith("beam"), lock)
    t0, n, cost = time.time(), 0, 0.0

    def one(u, q):
        for attempt in range(4):
            try:
                return answer(c, q, u, folders[u["unit_id"]], args, tool.id)
            except Exception as e:  # noqa: BLE001 - retried, then left for a rerun
                if attempt == 3:
                    raise
                print(f"  {q['qid']}: {type(e).__name__}: {str(e)[:200]}; retrying", file=sys.stderr)
                time.sleep(10 * 2**attempt)

    with cf.ThreadPoolExecutor(args.workers) as ex:
        futs = [ex.submit(one, u, q) for u, q in jobs]
        for fut in cf.as_completed(futs):
            try:
                r = fut.result()
            except Exception as e:  # noqa: BLE001
                print(f"  failed: {e}", file=sys.stderr)
                continue
            with wlock:
                with open(args.out, "a") as f:
                    f.write(json.dumps(r) + "\n")
            n += 1
            cost += r["cost_reader"]
            if n % 50 == 0:
                print(f"  {n}/{len(jobs)} in {time.time() - t0:.0f} s, ${cost:.2f}", file=sys.stderr)
    print(f"answered {n} of {len(jobs)}, ${cost:.2f}", file=sys.stderr)


if __name__ == "__main__":
    main()
