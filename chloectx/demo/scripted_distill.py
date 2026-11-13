#!/usr/bin/env python3
"""A scripted stand-in for the distill model, for `demo/seed.py`.

ChloeCtx's distiller runs `claude -p ... --output-format json` with one
exchange on stdin. This program answers the same way, from the notes the
story gives for that exchange: it finds the turn by its request (the text
between <request> and </request>) in the JSON file $CHLOECTX_DEMO_NOTES, and
prints its notes as the JSON lines the real model writes, with `replaces`
and `done` turned into the ids of the noted items the prompt lists. Any
other prompt (query planning, consolidation) gets an empty answer.

Standard library only.
"""

import json
import os
import re
import sys


def main():
    prompt = sys.stdin.read()
    out = []
    m = re.search(r"<request>\n?(.*?)\n?</request>", prompt, re.S)
    if m and os.environ.get("CHLOECTX_DEMO_NOTES"):
        with open(os.environ["CHLOECTX_DEMO_NOTES"]) as f:
            by_request = json.load(f)
        request = m.group(1).strip()
        notes = by_request.get(request[:200])
        # The noted items the prompt lists: "[id] kind: text".
        known = re.findall(r"^\[([^\]]+)\] [a-z]+: (.*)$", prompt, re.M)

        def id_of(start):
            for i, text in known:
                if text.startswith(start):
                    return i
            return None

        for n in notes or []:
            if n["kind"] == "done":
                i = id_of(n["done"])
                if i:
                    out.append({"kind": "done", "id": i})
                continue
            item = {"kind": n["kind"], "text": n["text"], "entities": n.get("entities", []), "when": n.get("when", [])}
            if n.get("replaces"):
                i = id_of(n["replaces"])
                if i:
                    item["replaces"] = i
            out.append(item)
    text = "\n".join(json.dumps(x) for x in out)
    json.dump({"type": "result", "is_error": False, "result": text, "total_cost_usd": 0, "usage": {"input_tokens": len(prompt) // 4, "output_tokens": len(text) // 4}}, sys.stdout)


if __name__ == "__main__":
    main()
