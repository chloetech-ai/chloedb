#!/usr/bin/env python3
"""Old names (Strata, Engram) on the happy path: fails when a page a
newcomer follows, or a placeholder package, tells them to use one.

ChloeDB was called Strata and ChloeCtx Engram until November 2026. Some
names keep the old spelling by design (AGENTS.md lists them: the operator's
StrataCluster and strata-system, the app engine in chloedb-apps/, the file
format and `memory.strata`, `engram.sock`, the .strata files), and the old
names still work for a release, which the pages say. So this is narrow: it
reads the guide, the quickstart, the README, the installers and the
placeholders, and refuses what would send someone down the old path (an old
flag, folder, package or address, or the product called Strata), except on a
line that says it is the old name.

    python3 scripts/check-old-names.py        # CI's docs-site job runs it
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]

FILES = [
    "README.md",
    "docs/quickstart.md",
    "docs/guide/*.md",
    "docs/chloectx/setup.md",
    "docs/chloedb/python.md",
    "docs/chloedb/javascript.md",
    "docs/chloedb/crawler.md",
    "docs/chloedb/sheets.md",
    "docs/install.sh",
    "install.sh",
    "packaging/chloedb/**/*",
]

# (what, pattern): each a way to the old path.
RULES = [
    ("the crawler's and sheets' --strata flag (now --server)", re.compile(r"chloedb-(?:crawl|sheets)\b[^\n]*\s--strata\b")),
    ("the old models folder", re.compile(r"\.local/share/strata\b")),
    ("the old ChloeCtx demo folder", re.compile(r"engram-demo")),
    ("the old package name", re.compile(r"\bstrata-db\b|(?:pip|npm) install strata\b")),
    ("the old repository", re.compile(r"chloetech-ai/strata\b")),
    ("the old programs", re.compile(r"\b(?:strata-server|strata-crawl|strata-sheets)\b(?![-\w.])")),
    ("the product's old name", re.compile(r"\bStrata\b(?!Cluster|Error|Exception)|\bEngram\b")),
]

# A line that says it is about the old name is fine.
ALLOWED = re.compile(
    r"before the rename|old name|its name before|was called|formerly|renamed|the rename|"
    r"still (?:work|accepted|read)|a link to it|old one|from before|old folder|Old names|old spelling|"
    r"STRATA_|keeps using|keep using|keeps the old|redirect",
    re.I,
)


def files():
    for pattern in FILES:
        for f in sorted(ROOT.glob(pattern)):
            if f.is_file() and f.suffix in ("", ".md", ".sh", ".py", ".js", ".json", ".toml", ".rs"):
                yield f


def main():
    bad = []
    for f in files():
        rel = f.relative_to(ROOT).as_posix()
        for n, line in enumerate(f.read_text(errors="replace").splitlines(), 1):
            if ALLOWED.search(line):
                continue
            for what, rule in RULES:
                if rule.search(line):
                    bad.append(f"{rel}:{n}: {what}: {line.strip()[:160]}")
    for b in bad:
        print(b)
    if bad:
        print(f"\n{len(bad)} old name(s) on the happy path: use the new name, or say on the line that it is the old one (scripts/check-old-names.py)", file=sys.stderr)
        sys.exit(1)
    print("no old names on the happy path")


if __name__ == "__main__":
    main()
