#!/usr/bin/env python3
"""LoCoMo as a chat between a user and an assistant, for chloectx-bench.

LoCoMo's conversations are between two people (roles are their names).
ChloeCtx records exchanges: a user's message and the reply after it. This
writes locomo_chat.jsonl from locomo.jsonl (scripts/convert_memory_benchmarks.py)
with the first speaker of each conversation as `user` and the second as
`assistant`, each message keeping its speaker's name ("Caroline: ..."), so
`chloectx-bench deep --notes` distils one exchange per message and its reply.
Turn ids, sessions, timestamps and questions are unchanged, so contexts built
on it are read against locomo.jsonl (reader_eval.py names the two speakers).

  python3 scripts/locomo_chat.py /home/claude/data/memory/locomo.jsonl /home/claude/data/memory/locomo_chat.jsonl
"""

import json
import sys


def main(src, dst):
    with open(src) as f, open(dst, "w") as out:
        for line in f:
            u = json.loads(line)
            names = []
            for t in u["turns"]:
                if t["role"] not in names:
                    names.append(t["role"])
            roles = {names[0]: "user"} | {n: "assistant" for n in names[1:]}
            for t in u["turns"]:
                t["text"] = f"{t['role']}: {t['text']}"
                t["role"] = roles[t["role"]]
            out.write(json.dumps(u) + "\n")


if __name__ == "__main__":
    main(*sys.argv[1:3])
