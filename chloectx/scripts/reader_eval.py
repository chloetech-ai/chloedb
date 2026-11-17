#!/usr/bin/env python3
"""End-to-end answer quality: a model answers LongMemEval questions from a
given memory context, and a second model judges each answer with
LongMemEval's grading prompts.

Contexts come from `chloectx-structure --dump` files (what ChloeCtx would inject)
or are built here:
  none    no memory at all;
  oracle  the evidence sessions only, in full (an upper bound for reading);
  full    the whole history (about 115,000 tokens), for long-context reading.

  python3 scripts/reader_eval.py --config today=results/structure/context_...jsonl \
      --config none=none --data /home/user/data/memory/longmemeval_s.jsonl --out results/reader

Calls go through the `claude` CLI in print mode with no tools. Answers are
cached per (config, question) in <out>/answers.jsonl and grades in
<out>/verdicts_<judge>.jsonl, so a run can resume. The judge grades
JUDGE_BATCH answers per call, each with its own LongMemEval prompt.
"""

import argparse
import concurrent.futures as cf
import json
import os
import re
import subprocess
import sys
import threading
import time
from collections import defaultdict

READER_SYSTEM = (
    "You are an assistant with a long-term memory of your earlier conversations with the user. "
    "Answer the user's question from the memory provided."
)

READER_PROMPT = """Here is what your memory holds about earlier conversations with this user.

<memory>
{context}
</memory>

Current date: {date}
Question: {question}

First list the facts in the memory that bear on the question, with their dates. Then give the answer on a last line starting with "Answer:". If the memory does not hold the answer, say so."""

NO_MEMORY_PROMPT = """Current date: {date}
Question: {question}

Give the answer on a last line starting with "Answer:". If you do not know, say so."""

JUDGE_BATCH = 10

# LoCoMo: conversations between two people, questions without a date. The
# same reader instructions as above, with the speakers named; the judge sees
# only the final answer line, as Mem0's evaluation passes its short answers.
LOCOMO_READER_SYSTEM = (
    "You are an assistant with a long-term memory of conversations between two people. "
    "Answer questions about them from the memory provided."
)

LOCOMO_READER_PROMPT = """Here is what your memory holds about earlier conversations between {a} and {b}.

<memory>
{context}
</memory>

Question: {question}

First list the facts in the memory that bear on the question, with their dates; turn relative times ("last year", "yesterday") into dates from the date of the conversation they were said in. Then give the answer, as a short phrase, on a last line starting with "Answer:". If the memory does not hold the answer, say so."""

LOCOMO_NO_MEMORY_PROMPT = """Question about {a} and {b}: {question}

Give the answer, as a short phrase, on a last line starting with "Answer:". If you do not know, say so."""

# Mem0's LoCoMo judge (mem0ai/mem0 v1.0.0, evaluation/metrics/llm_judge.py,
# Apache-2.0), the "J" score Mem0, Zep and Letta report. Its last lines ask for
# a JSON label; graded in batches here, the batch prompt asks for one line per
# item instead.
LOCOMO_JUDGE = """Your task is to label an answer to a question as 'CORRECT' or 'WRONG'. You will be given the following data:
    (1) a question (posed by one user to another user),
    (2) a 'gold' (ground truth) answer,
    (3) a generated answer
which you will score as CORRECT/WRONG.

The point of the question is to ask about something one user should know about the other user based on their prior conversations.
The gold answer will usually be a concise and short answer that includes the referenced topic, for example:
Question: Do you remember what I got the last time I went to Hawaii?
Gold answer: A shell necklace
The generated answer might be much longer, but you should be generous with your grading - as long as it touches on the same topic as the gold answer, it should be counted as CORRECT.

For time related questions, the gold answer will be a specific date, month, year, etc. The generated answer might be much longer or use relative time references (like "last Tuesday" or "next month"), but you should be generous with your grading - as long as it refers to the same date or time period as the gold answer, it should be counted as CORRECT. Even if the format differs (e.g., "May 7th" vs "7 May"), consider it CORRECT if it's the same date.

Now it's time for the real question:
Question: {q}
Gold answer: {a}
Generated answer: {r}"""


def final_answer(text):
    """The text after the last "Answer:", or the whole reply when there is none."""
    i = text.rfind("Answer:")
    return text[i + len("Answer:"):].strip() if i >= 0 else text.strip()


def speakers(unit):
    names = []
    for t in unit["turns"]:
        if t["role"] not in names:
            names.append(t["role"])
        if len(names) == 2:
            break
    return (names + ["the other person"])[:2] if names else ["one person", "another"]

# LongMemEval's judge prompts (evaluate_qa.py), by question type.
JUDGE = {
    "default": "I will give you a question, a correct answer, and a response from a model. Please answer yes if the response contains the correct answer. Otherwise, answer no. If the response is equivalent to the correct answer or contains all the intermediate steps to get the correct answer, you should also answer yes. If the response only contains a subset of the information required by the answer, answer no. \n\nQuestion: {q}\n\nCorrect Answer: {a}\n\nModel Response: {r}\n\nIs the model response correct? Answer yes or no only.",
    "temporal-reasoning": "I will give you a question, a correct answer, and a response from a model. Please answer yes if the response contains the correct answer. Otherwise, answer no. If the response is equivalent to the correct answer or contains all the intermediate steps to get the correct answer, you should also answer yes. If the response only contains a subset of the information required by the answer, answer no. In addition, do not penalize off-by-one errors for the number of days. If the question asks for the number of days/weeks/months, etc., and the model makes off-by-one errors (e.g., predicting 19 days when the answer is 18), the model's response is still correct. \n\nQuestion: {q}\n\nCorrect Answer: {a}\n\nModel Response: {r}\n\nIs the model response correct? Answer yes or no only.",
    "knowledge-update": "I will give you a question, a correct answer, and a response from a model. Please answer yes if the response contains the correct answer. Otherwise, answer no. If the response contains some previous information along with an updated answer, the response should be considered as correct as long as the updated answer is the required answer.\n\nQuestion: {q}\n\nCorrect Answer: {a}\n\nModel Response: {r}\n\nIs the model response correct? Answer yes or no only.",
    "single-session-preference": "I will give you a question, a rubric for desired personalized response, and a response from a model. Please answer yes if the response satisfies the desired response. Otherwise, answer no. The model does not need to reflect all the points in the rubric. The response is correct as long as it recalls and utilizes the user's personal information correctly.\n\nQuestion: {q}\n\nRubric: {a}\n\nModel Response: {r}\n\nIs the model response correct? Answer yes or no only.",
    "abstention": "I will give you an unanswerable question, an explanation, and a response from a model. Please answer yes if the model correctly identifies the question as unanswerable. The model could say that the information is incomplete, or some other information is given but the asked information is not.\n\nQuestion: {q}\n\nExplanation: {a}\n\nModel Response: {r}\n\nDoes the model correctly identify the question as unanswerable? Answer yes or no only.",
}


def call(model, system, prompt, timeout=600, retries=4):
    cmd = ["claude", "-p", "--model", model, "--system-prompt", system, "--tools", "", "--no-session-persistence", "--output-format", "json"]
    last = None
    for attempt in range(retries):
        try:
            p = subprocess.run(cmd, input=prompt, capture_output=True, text=True, timeout=timeout, cwd=os.environ.get("READER_CWD", "/tmp"))
            d = json.loads(p.stdout)
            if d.get("is_error") or not d.get("result"):
                raise RuntimeError(str(d.get("result"))[:200])
            u = d.get("usage", {})
            tokens = u.get("input_tokens", 0) + u.get("cache_read_input_tokens", 0) + u.get("cache_creation_input_tokens", 0)
            return d["result"], d.get("total_cost_usd", 0.0), tokens
        except Exception as e:  # noqa: BLE001 - retried, then reported
            last = e
            time.sleep(2 * 2**attempt)
    raise RuntimeError(f"{model} failed: {last}")


def sessions_text(turns, sids=None):
    """Sessions in time order, each under its date."""
    order, by = [], defaultdict(list)
    for t in turns:
        if sids is not None and t["session_id"] not in sids:
            continue
        if t["session_id"] not in by:
            order.append(t["session_id"])
        by[t["session_id"]].append(t)
    out = []
    for sid in order:
        ts = by[sid]
        out.append(f"### Conversation on {ts[0].get('timestamp') or 'unknown date'}")
        for t in ts:
            out.append(f"({t['role']}) {t['text']}")
        out.append("")
    return "\n".join(out)


def load_questions(data_path):
    qs = {}
    with open(data_path) as f:
        for line in f:
            u = json.loads(line)
            for q in u["questions"]:
                qs[q["qid"]] = (q, u)
    return qs


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", required=True, help="longmemeval_s.jsonl")
    ap.add_argument("--oracle", default="/home/user/data/memory/longmemeval_oracle.jsonl")
    ap.add_argument("--config", action="append", required=True,
                    help="name=context.jsonl[@field] | name=none | name=oracle | name=full | name=answers:answers.jsonl "
                         "(answers given elsewhere, e.g. by Letta's own agent: {qid, answer[, cost_reader, reader_tokens, context_chars]}; only judged)")
    ap.add_argument("--abstention", choices=("keep", "skip"), default="keep",
                    help="skip: leave out abstention questions (LoCoMo's adversarial category, which Mem0, Zep and Letta leave out)")
    ap.add_argument("--out", default="results/reader")
    ap.add_argument("--reader", required=True, help="the reader's --model")
    ap.add_argument("--judge", required=True, help="the judge's --model")
    ap.add_argument("--reader-label", default="small", help="how results name the reader")
    ap.add_argument("--judge-label", default="large", help="how results name the judge (and its verdict file)")
    ap.add_argument("--every", type=int, default=1, help="only every n-th question (in data order)")
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--workers", type=int, default=8)
    args = ap.parse_args()

    qs = load_questions(args.data)
    qids = list(qs)[:: args.every]
    if args.abstention == "skip":
        qids = [q for q in qids if not qs[q][0]["abstention"]]
    if args.limit:
        qids = qids[: args.limit]
    oracle = None

    contexts = {}
    given = {}  # name -> {qid: answer row} for answers:FILE configs
    for spec in args.config:
        name, src = spec.split("=", 1)
        c = {}
        if src.startswith("answers:"):
            with open(src[len("answers:"):]) as f:
                for line in f:
                    r = json.loads(line)
                    given.setdefault(name, {})[r["qid"]] = r
            contexts[name] = {qid: "" for qid in given.get(name, {})}
            continue
        src, _, field = src.partition("@")
        field = field or "context"
        if src == "none":
            c = {qid: None for qid in qids}
        elif src == "full":
            c = {qid: sessions_text(qs[qid][1]["turns"]) for qid in qids}
        elif src == "oracle":
            if oracle is None:
                oracle = load_questions(args.oracle)
            c = {qid: sessions_text(oracle[qid][1]["turns"]) for qid in qids if qid in oracle}
        else:
            with open(src) as f:
                for line in f:
                    r = json.loads(line)
                    c[r["qid"]] = r[field]
        contexts[name] = c

    os.makedirs(args.out, exist_ok=True)
    cache_path = os.path.join(args.out, "answers.jsonl")
    verdict_path = os.path.join(args.out, f"verdicts_{args.judge_label}.jsonl")
    done, verdicts = {}, {}
    if os.path.exists(cache_path):
        with open(cache_path) as f:
            for line in f:
                r = json.loads(line)
                done[(r["config"], r["qid"])] = {k: v for k, v in r.items() if k not in ("verdict", "correct", "cost_judge")}
    if os.path.exists(verdict_path):
        with open(verdict_path) as f:
            for line in f:
                v = json.loads(line)
                verdicts[(v["config"], v["qid"])] = v
    lock = threading.Lock()

    def read(name, qid):
        q, unit = qs[qid]
        ctx = contexts[name].get(qid)
        if name in given:
            g = given[name][qid]
            answer, cost, tokens, chars = g["answer"], g.get("cost_reader", 0.0), g.get("reader_tokens", 0), g.get("context_chars", 0)
        else:
            if unit.get("dataset") == "locomo":
                a, b = speakers(unit)
                system = LOCOMO_READER_SYSTEM
                if ctx is None:
                    prompt = LOCOMO_NO_MEMORY_PROMPT.format(a=a, b=b, question=q["question"])
                else:
                    prompt = LOCOMO_READER_PROMPT.format(a=a, b=b, context=ctx.strip() or "(nothing)", question=q["question"])
            else:
                date = q.get("question_date") or "unknown"
                system = READER_SYSTEM
                if ctx is None:
                    prompt = NO_MEMORY_PROMPT.format(date=date, question=q["question"])
                else:
                    prompt = READER_PROMPT.format(context=ctx.strip() or "(nothing)", date=date, question=q["question"])
            answer, cost, tokens = call(args.reader, system, prompt)
            chars = len(ctx) if ctx else 0
        r = {
            "config": name, "qid": qid, "qtype": q["question_type"], "abstention": q["abstention"],
            "context_chars": chars, "reader_tokens": tokens, "answer": answer, "cost_reader": cost,
        }
        with lock:
            with open(cache_path, "a") as f:
                f.write(json.dumps(r) + "\n")
        return r

    def judge(batch):
        """Grades up to JUDGE_BATCH answers in one call, each with its own LongMemEval prompt."""
        items = []
        locomo = qs[batch[0]["qid"]][1].get("dataset") == "locomo"
        for k, r in enumerate(batch, 1):
            q, _ = qs[r["qid"]]
            kind = "abstention" if q["abstention"] else q["question_type"]
            if locomo and kind != "abstention":
                items.append(f"### Item {k}\n" + LOCOMO_JUDGE.format(q=q["question"], a=q["answer"], r=final_answer(r["answer"])))
            else:
                items.append(f"### Item {k}\n" + JUDGE.get(kind, JUDGE["default"]).format(q=q["question"], a=q["answer"], r=r["answer"]))
        words = ("CORRECT", "WRONG") if locomo else ("yes", "no")
        prompt = (
            "Grade each item below independently, following its own instructions.\n\n" + "\n\n".join(items)
            + f"\n\nReply with exactly {len(batch)} lines, one per item, in the form '<item number>: {words[0]}' or '<item number>: {words[1]}'"
            + (" (an abstention item's yes is CORRECT, its no is WRONG)." if locomo else ".")
        )
        out, cost, _ = call(args.judge, "You grade answers. Reply only with the requested lines.", prompt)
        got = {}
        for line in out.splitlines():
            m = re.match(r"\s*(?:item\s*)?(\d+)\s*[:.)-]\s*(yes|no|correct|wrong)\b", line.strip().lower())
            if m:
                got[int(m.group(1))] = m.group(2) in ("yes", "correct")
        if sorted(got) != list(range(1, len(batch) + 1)):
            raise RuntimeError(f"judge returned {len(got)} of {len(batch)} verdicts")
        rows = [{"config": r["config"], "qid": r["qid"], "correct": got[k], "cost_judge": cost / len(batch)} for k, r in enumerate(batch, 1)]
        with lock:
            with open(verdict_path, "a") as f:
                for v in rows:
                    f.write(json.dumps(v) + "\n")
        return rows

    def run(fn, jobs, what):
        out = []
        t0 = time.time()
        with cf.ThreadPoolExecutor(args.workers) as ex:
            futs = [ex.submit(fn, *j) for j in jobs]
            for i, fut in enumerate(cf.as_completed(futs), 1):
                try:
                    out.append(fut.result())
                except Exception as e:  # noqa: BLE001 - reported; a rerun retries it
                    print(f"{what} failed: {e}", file=sys.stderr)
                if i % 50 == 0:
                    print(f"  {what} {i}/{len(jobs)} in {time.time() - t0:.0f} s", file=sys.stderr)
        return out

    wanted = set(qids)
    jobs = [(name, qid) for name in contexts for qid in qids if qid in contexts[name] and (name, qid) not in done]
    print(f"{len(jobs)} answers to get ({len(done)} cached)", file=sys.stderr)
    for r in run(read, jobs, "read"):
        done[(r["config"], r["qid"])] = r
    pending = [r for k, r in done.items() if k[0] in contexts and k[1] in wanted and k not in verdicts]
    batches = [(pending[i : i + JUDGE_BATCH],) for i in range(0, len(pending), JUDGE_BATCH)]
    print(f"{len(pending)} answers to grade in {len(batches)} calls", file=sys.stderr)
    for rows in run(judge, batches, "judge"):
        for v in rows:
            verdicts[(v["config"], v["qid"])] = v
    for k, r in done.items():
        if k in verdicts:
            r["correct"] = verdicts[k]["correct"]
            r["cost_judge"] = verdicts[k]["cost_judge"]

    summary = {}
    for name in contexts:
        rows = [done[(name, q)] for q in qids if (name, q) in done and "correct" in done[(name, q)]]
        by = defaultdict(list)
        for r in rows:
            by["abstention" if r["abstention"] else r["qtype"]].append(r["correct"])
        summary[name] = {
            "n": len(rows),
            "accuracy": sum(r["correct"] for r in rows) / max(1, len(rows)),
            "by_type": {k: {"n": len(v), "accuracy": sum(v) / len(v)} for k, v in sorted(by.items())},
            "context_chars": sum(r["context_chars"] for r in rows) / max(1, len(rows)),
            "reader_tokens": sum(r["reader_tokens"] for r in rows) / max(1, len(rows)),
            "cost_usd": sum(r["cost_reader"] + r.get("cost_judge", 0.0) for r in rows),
        }
        print(f"{name:<28} n={len(rows):<4} accuracy {summary[name]['accuracy']:.3f}  context {summary[name]['context_chars']:.0f} chars  reader tokens {summary[name]['reader_tokens']:.0f}", file=sys.stderr)
    with open(os.path.join(args.out, "summary.json"), "w") as f:
        json.dump({"reader": args.reader_label, "judge": args.judge_label, "questions": len(qids), "configs": summary}, f, indent=1)


if __name__ == "__main__":
    main()
