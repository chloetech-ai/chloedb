#!/usr/bin/env python3
"""Checks the capability comparison (docs/site/assets/compare-data.js).

The page went public on 12 November 2026 with 99 cells it said were not
yet checked against each project's documentation, and the line that said
so was removed with the site's draft banner (the audit of 15 November 2026,
CMP-2). This check, run by CI's documentation job, fails when:

- a cell's code is not one of y, e, p, n, $ or t (site.js draws no other);
- a ChloeDB cell is "?": ChloeDB's own cells are checked against its docs;
- a system other than ChloeDB has no `checked` day;
- any cell is "?" and the data has no `draft` line, or site.js does not
  draw it above the matrix.

    python3 docs/site/scripts/check_compare.py          # the check
    python3 docs/site/scripts/check_compare.py --list   # every "?" cell
"""
import json
import pathlib
import re
import sys

SITE = pathlib.Path(__file__).resolve().parent.parent
DATA = SITE / "assets" / "compare-data.js"
SITE_JS = SITE / "assets" / "site.js"
CODES = set("yepn$t")


def js_object(text):
    """The object literal assigned to window.STRATA_COMPARE, as Python.

    The file is a JavaScript literal with bare keys, double-quoted strings
    and comments: the keys are quoted and the comments dropped, strings
    copied as they are, and the result read as JSON.
    """
    start = text.index("window.STRATA_COMPARE =") + len("window.STRATA_COMPARE =")
    out, i, n = [], start, len(text)
    while i < n:
        ch = text[i]
        if ch == '"':
            j = i + 1
            while text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            out.append(text[i : j + 1])
            i = j + 1
        elif text.startswith("/*", i):
            i = text.index("*/", i) + 2
        elif text.startswith("//", i):
            i = text.index("\n", i)
        elif ch.isalpha() or ch in "_$":
            m = re.match(r"[A-Za-z_$][\w$]*", text[i:])
            word = m.group(0)
            rest = text[i + len(word) :].lstrip()
            out.append(json.dumps(word) if rest.startswith(":") else word)
            i += len(word)
        elif ch == ";":
            break
        else:
            out.append(ch)
            i += 1
    return json.loads("".join(out))


def cells(cmp):
    """(section, row, system id, cell) for every cell of the matrix."""
    for sec in cmp["sections"]:
        for title, row in sec["rows"]:
            for sid, cell in row.items():
                yield sec["title"], title, sid, cell


def main():
    cmp = js_object(DATA.read_text())
    ids = {s["id"] for s in cmp["systems"]}
    errors, unchecked = [], []
    for sec, title, sid, cell in cells(cmp):
        where = f"{sec} / {title} / {sid}"
        if sid not in ids:
            errors.append(f"{where}: no such system")
        if not cell or cell[0] not in CODES:
            errors.append(f"{where}: {cell!r} is not a code site.js draws ({' '.join(sorted(CODES))})")
            continue
        if cell[1:2] == "?":
            unchecked.append((where, cell))
            if sid == "strata":
                errors.append(f"{where}: ChloeDB's own cells are checked against its docs, never \"?\"")
        if cell[0] == "e" and sid != "strata":
            errors.append(f"{where}: \"e\" marks a feature ChloeDB's docs call experimental")
    for s in cmp["systems"]:
        if s["id"] != "strata" and not s.get("checked"):
            errors.append(f"system {s['id']}: no `checked` day")
    if unchecked:
        if not cmp.get("draft"):
            errors.append(f"{len(unchecked)} cells are \"?\" and the data has no `draft` line to show above the matrix")
        if "CMP.draft" not in SITE_JS.read_text():
            errors.append(f"{len(unchecked)} cells are \"?\" and site.js does not draw the `draft` line")
    if "--list" in sys.argv[1:]:
        for where, cell in unchecked:
            print(f"{where}: {cell}")
    total = sum(1 for _ in cells(cmp))
    print(f"compare-data.js: {total} cells, {len(unchecked)} not yet checked" + (" (the page says it is a draft)" if unchecked and not errors else ""))
    for e in errors:
        print(f"error: {e}", file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
