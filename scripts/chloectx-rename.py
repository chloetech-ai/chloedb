#!/usr/bin/env python3
"""Rename Engram's folder, crate and programs to ChloeCtx in a working tree.

ChloeCtx was called Engram. This script is the mechanical half of the
rename. It moves the folders (engram/ to chloectx/, docs/engram/ to
docs/chloectx/), renames the Cargo package and the programs (chloectx,
chloectx-bench, chloectx-structure), and rewrites what refers to them:
paths, crate paths (`engram::` to `chloectx::`), log filters, the commands
people type (`engram doctor` to `chloectx doctor`), the settings
(`ENGRAM_HOME` to `CHLOECTX_HOME`; the programs still read the old names),
the programs' messages and threads, links to headings that changed, and the
name in prose (Engram to ChloeCtx). It leaves alone:

- measured results (any `results/` folder, engram/report/, docs/reports/),
  except their links to files in the repository;
- the names of systems in the evaluation scripts and their data
  (`"engram"`, `"engram@main"`), which key measured results;
- what is on disk in a user's install: the store folder
  (~/.local/share/engram, which existing installs keep), `memory.strata`,
  `engram.sock`, `/tmp/engram-<uid>`;
- the MCP server's name and the entries the installer writes into Claude
  Code's settings, which the hand-made half renames (the installer moves a
  user's entries from the old names to the new ones);
- other projects named Engram (DeepSeek's, the ENGRAM paper,
  Gentleman-Programming/engram) and the old repository `rkonow/engram`;
- a line that says "before the rename", which is about the old names.

Main went through this same transformation, so it also ports a branch that
was started before the rename: scripts/chloectx-port.sh runs it. Running it
again on its own output changes nothing. It is not meant for the tree after
the hand-made half, which writes the old names on purpose (the installer's
migration, the notes on what was renamed).

    python3 scripts/chloectx-rename.py            # in the repository's root
    python3 scripts/chloectx-rename.py --dry-run  # list what would change
"""

import argparse
import os
import re
import subprocess
import sys

FOLDERS = [("engram", "chloectx"), ("docs/engram", "docs/chloectx")]
BIN_DIR = "chloectx/src/bin"
PROGRAMS = ["bench", "structure"]
SUBCOMMANDS = ["install", "uninstall", "hook", "mcp", "serve", "search", "ask", "sql", "remember", "stats",
               "backup", "briefing", "distill", "consolidate", "stop", "doctor", "ui", "rpc",
               "ingest-transcript", "fetch-model"]

SKIP_DIRS = {".git", "target", "node_modules", ".venv", "__pycache__"}
SKIP_FILES = {"scripts/chloectx-rename.py", "scripts/chloectx-port.sh", "docs/engram/README.md",
              # ChloeDB's rename scripts, which ports run as they are.
              "scripts/chloedb-rename.py", "scripts/chloedb-port.sh",
              # The old settings' names, which the programs still read.
              "chloectx/src/env.rs"}


def skipped(path):
    return any(p in SKIP_DIRS for p in path.split("/")) or path in SKIP_FILES


def measured(path):
    """Results, reports and fixture data: only their links change."""
    parts = path.split("/")
    if "fixtures" in parts[:-1] and not path.endswith((".rs", ".sh", ".py", ".md")):
        return True
    if "results" in parts[:-1] or path.startswith(("docs/reports/", "packaging/")):
        return True
    return path.startswith(("engram/report/", "chloectx/report/", "engram/probes/", "chloectx/probes/"))


def tracked(path):
    return subprocess.run(["git", "ls-files", "--error-unmatch", path], capture_output=True).returncode == 0


def move(src, dst, dry):
    if not os.path.exists(src) or os.path.exists(dst):
        return False
    if dry:
        print(f"would move {src} -> {dst}")
    elif tracked(src):
        subprocess.run(["git", "mv", src, dst], check=True)
    else:
        os.rename(src, dst)
    return True


NOT_AFTER = r"(?<![\w.$/-])"
NOT_BEFORE = r"(?![\w-])"

RULES = [
    # Rust paths and log filters: `use engram::store`, engram=debug.
    re.compile(r"(?<![\w:])engram::"), "chloectx::",
    re.compile(r"(?<![\w.-])engram=(error|warn|info|debug|trace)\b"), r"chloectx=\1",
    re.compile(r"CARGO_BIN_EXE_engram(?=[-\W])"), "CARGO_BIN_EXE_chloectx",
    # The programs: engram-bench, target/release/engram, --bin engram,
    # ~/.local/bin/engram, #[command(name = "engram"), "engram doctor".
    re.compile(NOT_AFTER + r"engram-(" + "|".join(PROGRAMS) + ")" + NOT_BEFORE), r"chloectx-\1",
    re.compile(r"(?<=[/ ])(release|ci|debug)/engram(?![\w/.-])"), r"\1/chloectx",
    re.compile(r"(?<=--bin )engram(?![\w./-])"), "chloectx",
    re.compile(r"(?<=\bbin/)engram(?![\w/.-])"), "chloectx",
    # A checkout's crate folder: /home/claude/engram-main/engram/target/.
    re.compile(r"(?<=/)engram(?=/target/)"), "chloectx",
    re.compile(r'#\[command\(name = "engram"'), '#[command(name = "chloectx"',
    re.compile(r"(?<![\w./$-])engram(?= (?:" + "|".join(SUBCOMMANDS) + r")\b)"), "chloectx",
    # The folders: cd engram, ../engram, "$ROOT/engram, blob/main/engram,
    # working-directory: engram, docs/engram, `engram/` at a path's start.
    re.compile(r"(?<=\bcd )(\.\./)?engram(?![\w./-])"), r"\1chloectx",
    re.compile(r"(?<![\w.])\.\./engram(?![\w.-])"), "../chloectx",
    re.compile(r"(?<=\$ROOT/)engram(?![\w.-])"), "chloectx",
    re.compile(r"(?<=/(?:blob|tree)/main/)engram(?![\w.-])"), "chloectx",
    re.compile(r"(?<=working-directory: )engram(?![\w./-])"), "chloectx",
    re.compile(r"(?<=workspaces: )engram(?![\w./-])"), "chloectx",
    re.compile(r"(?<![\w.-])docs/engram(?![\w.-])"), "docs/chloectx",
    re.compile(r"(?<![\w.$~/-])engram/(?=[\w.*`)\]]|\s|$)", re.M), "chloectx/",
    # Settings: ENGRAM_HOME is CHLOECTX_HOME (ENGRAM_* is still read).
    re.compile(r"(?<![\w])ENGRAM_(?=[A-Z])"), "CHLOECTX_",
    # The programs' messages ("engram: serving ..."), threads and scratch
    # folders; the socket's names stay (a running daemon listens on them).
    re.compile(r'(?<=")engram: '), "chloectx: ",
    re.compile(r'(?<=")engram-(?=[a-z]+[-"{])'), "chloectx-",
    re.compile(r'(?<=with_file_name\(")engram(?=")'), "chloectx",
    # Links to a heading that names it: (#install-engram), (setup.md#engram-...).
    re.compile(r"(?<=\]\()((?:[\w./-]+\.md)?#(?:[a-z0-9]+-)*)engram(?=(?:-[a-z0-9]+)*\))"), r"\1chloectx",
    re.compile(r'(?<=href="#)((?:[a-z0-9]+-)*)engram(?=(?:-[a-z0-9]+)*")'), r"\1chloectx",
    re.compile(r"(?:(?<=release/)|(?<=debug/)|(?<=/ci/)|(?<=bin/))engram-(" + "|".join(PROGRAMS) + r")(?![\w-])"), r"chloectx-\1",
    # The checkout's folder: cd strata/engram.
    re.compile(r"(?<=\bstrata/)engram(?![\w.-])"), "chloectx",
    # The program, the crate or the MCP server named in code type: `engram`.
    re.compile(r"(?<=`)engram(?=`)"), "chloectx",
    re.compile(r"\b(claude mcp (?:get|remove|add-json)) engram\b"), r"\1 chloectx",
    # The installer's backups: settings.json.engram-<time>.bak.
    re.compile(r"\.engram-<time>\.bak"), ".chloectx-<time>.bak",
    # The name in prose.
    re.compile(r"\b([Aa])n Engram\b"), r"\1 ChloeCtx",
    re.compile(r"(?<![\w-])Engram(?![\w-])"), "ChloeCtx",
    re.compile(r"(?<=\\[nt])Engram(?![\w-])"), "ChloeCtx",  # in a string, after "\n"
]

# Kept as they are (see the module's doc).
KEEP = re.compile("|".join([
    r"DeepSeek'?s Engram",
    r"Engram \(Jan(?:uary)? 2026\)",
    r"Engram-style",
    r"rkonow/engram",
    r"Gentleman-Programming/engram",
    r"share/engram",
    r"/tmp/engram-",
    r"engram\.sock",
    r"engram/(?=engram)",  # the checkout of rkonow/engram: ~/code/engram/engram
    r"(?:called|named|was) Engram\b",
    r"(?:called|named) `engram`",
    # The MCP server's tools (mcp__engram__memory_search), renamed by hand.
    r"mcp__engram",
    # A line about the old names, which the hand-made half writes.
    r"(?m:^.*\bbefore the rename\b.*$)",
]))


def protect(text):
    kept = []

    def hold(m):
        kept.append(m.group(0))
        return f"\0{len(kept) - 1}\0"
    return KEEP.sub(hold, text), kept


def restore(text, kept):
    return re.sub(r"\0(\d+)\0", lambda m: kept[int(m.group(1))], text)


LINK = re.compile(r"(github\.com/chloetech-ai/strata/(?:blob|tree)/[\w.-]+/(?:docs/)?)engram(?=[/)\s\"'#]|$)")


def rewrite(path, text):
    if measured(path):
        return LINK.sub(lambda m: m.group(1) + "chloectx", text)
    text, kept = protect(text)
    for i in range(0, len(RULES), 2):
        text = RULES[i].sub(RULES[i + 1], text)
    if os.path.basename(path) in ("Cargo.toml", "Cargo.lock"):
        text = re.sub(r'(?m)^name = "engram"$', 'name = "chloectx"', text)
    return restore(text, kept)


def text_files():
    out = subprocess.run(["git", "ls-files", "-z"], capture_output=True, check=True).stdout
    for path in out.decode().split("\0"):
        if not path or skipped(path) or os.path.islink(path) or not os.path.isfile(path):
            continue
        with open(path, "rb") as f:
            data = f.read()
        if b"\0" in data[:8192]:
            continue
        try:
            yield path, data.decode("utf-8")
        except UnicodeDecodeError:
            continue


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()
    if not os.path.exists(".git"):
        sys.exit("run it in the repository's root")
    for old, new in FOLDERS:
        move(old, new, a.dry_run)
    if os.path.isdir(BIN_DIR):
        for entry in sorted(os.listdir(BIN_DIR)):
            if entry == "engram.rs" or entry.startswith("engram-"):
                move(os.path.join(BIN_DIR, entry), os.path.join(BIN_DIR, "chloectx" + entry[len("engram"):]), a.dry_run)
    if os.path.isdir("docs/chloectx") and not os.path.exists("docs/engram") and not a.dry_run:
        os.makedirs("docs/engram")
        with open("docs/engram/README.md", "w") as f:
            f.write("# Moved\n\nChloeCtx was called Engram, and its documentation moved to [docs/chloectx](../chloectx/).\n")
        subprocess.run(["git", "add", "docs/engram/README.md"], check=True)
    changed = 0
    for path, text in text_files():
        new = rewrite(path, text)
        if new != text:
            changed += 1
            if a.dry_run:
                print("would rewrite", path)
            else:
                with open(path, "w") as f:
                    f.write(new)
    print(f"{changed} files rewritten")


if __name__ == "__main__":
    main()
