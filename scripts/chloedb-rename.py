#!/usr/bin/env python3
"""Rename Strata's folders, crates and programs to ChloeDB in a working tree.

ChloeDB was called Strata. This script is the mechanical half of the
rename's phase 3. It moves the folders (strata/ to chloedb/, strata-server/
to chloedb-server/, ...), renames the Cargo packages and the programs
(chloedb, chloedb-server, chloedb-crawl, chloedb-sheets, chloedb-*-bench,
...), and rewrites what refers to them: paths, crate paths (`strata::` to
`chloedb::`), log filters and program names. It leaves alone:

- measured results (any `results/` folder), `docs/reports/`, the files
  test fixtures are made of and the placeholder packages in `packaging/`,
  except their links to files in the repository;
- the on-disk format, the shard protocol, the gRPC package (`strata.v1`),
  Kubernetes names, the image `ghcr.io/chloetech-ai/strata-server`, release
  tags (`strata-server-v0.1.0`) and the repository's own name
  (`chloetech-ai/strata`, which phase 4 renames);
- the programs the operator's pods run (every image has their old names,
  and images from before the rename have only those);
- settings (`STRATA_*`), headers (`x-strata-*`), the Python module
  (`import strata`), the Java package and the `.strata` file extension.

Main went through this same transformation, so it also ports a branch that
was started before the rename: scripts/chloedb-port.sh runs it. Running it
on a renamed tree changes nothing.

    python3 scripts/chloedb-rename.py            # in the repository's root
    python3 scripts/chloedb-rename.py --dry-run  # list what would change
"""

import argparse
import os
import re
import subprocess
import sys

# Folders, moved in this order; a missing one is skipped.
FOLDERS = [
    ("strata", "chloedb"),
    ("strata-server", "chloedb-server"),
    ("strata-crawl", "chloedb-crawl"),
    ("strata-sheets", "chloedb-sheets"),
    ("strata-py", "chloedb-py"),
    ("strata-js", "chloedb-js"),
    ("strata-java", "chloedb-java"),
    ("strata-apps", "chloedb-apps"),
    ("docs/strata", "docs/chloedb"),
]

# Where the programs are: src/bin/strata.rs, src/bin/strata-NAME.rs and
# src/bin/strata-NAME/.
BIN_DIRS = [("strata/src/bin", "chloedb/src/bin"), ("strata-server/src/bin", "chloedb-server/src/bin")]

# Cargo packages named strata-NAME that are not a folder's name.
PACKAGES = ["node", "fuzz"]

SKIP_DIRS = {".git", "target", "node_modules", ".venv", "__pycache__"}
SKIP_FILES = {"scripts/chloedb-rename.py", "scripts/chloedb-port.sh", "docs/strata/README.md", "docs/strata/crawler-bot.html"}

BOT_PAGE = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>strata-crawl</title>
<link rel="canonical" href="https://github.com/chloetech-ai/strata/blob/main/docs/chloedb/crawler-bot.md">
<meta http-equiv="refresh" content="0; url=https://github.com/chloetech-ai/strata/blob/main/docs/chloedb/crawler-bot.md">
</head>
<body>
<p>strata-crawl's page for site owners (what it fetches, how to slow it down or block it) is <a href="https://github.com/chloetech-ai/strata/blob/main/docs/chloedb/crawler-bot.md">here</a>.</p>
</body>
</html>
"""


def skipped(path):
    parts = path.split("/")
    return any(p in SKIP_DIRS for p in parts) or path in SKIP_FILES


def measured(path):
    """Results, reports, fixtures and the placeholder packages: only their
    links into the repository change."""
    parts = path.split("/")
    if "fixtures" in parts[:-1] and not path.endswith((".rs", ".sh", ".py", ".md")):
        return True  # the files a fixture is made of; its code and notes are rewritten
    return "results" in parts[:-1] or path.startswith(("docs/reports/", "packaging/"))


def keeps_names(path):
    """Program names stay in the operator, whose pods run them by the names
    every image has (old images have only those; phase 5 moves them), and on
    the crawler's page for site owners, which names its user agent."""
    if path.startswith(("docs/strata/crawler-bot.", "docs/chloedb/crawler-bot.")):
        return True
    return path.startswith(tuple(f"{d}/{p}" for d in ("strata-server", "chloedb-server")
                                 for p in ("src/operator/", "deploy/k8s/", "tests/operator")))


def tracked(path):
    return subprocess.run(["git", "ls-files", "--error-unmatch", path], capture_output=True).returncode == 0


def move(src, dst, dry):
    """git mv, which renames the folder itself, so untracked build output
    (target/) moves along and the next build is not cold."""
    if not os.path.exists(src) or os.path.exists(dst):
        return False
    if dry:
        print(f"would move {src} -> {dst}")
    elif tracked(src):
        subprocess.run(["git", "mv", src, dst], check=True)
    else:
        os.rename(src, dst)
    return True


def program_names():
    """NAME of every program strata-NAME, read from the tree."""
    names = set()
    for old, new in BIN_DIRS:
        for d in (old, new):
            if os.path.isdir(d):
                for entry in os.listdir(d):
                    stem = entry[:-3] if entry.endswith(".rs") else entry
                    for prefix in ("strata-", "chloedb-"):
                        if stem.startswith(prefix):
                            names.add(stem[len(prefix):])
    return names


def rules(programs):
    folders = [old[len("strata-"):] for old, _ in FOLDERS if old.startswith("strata-")]
    names = sorted(set(folders) | programs | set(PACKAGES), key=len, reverse=True)
    word = re.compile(r"(?<![\w.])(?<!\w-)(?<!chloetech-ai/)strata-(" + "|".join(map(re.escape, names)) + r")(?![\w-])(?!:[\w<$])(?!@sha)")
    # (kind, pattern, replacement). Kinds: crate (Rust paths, everywhere),
    # cargo (Cargo.toml and Cargo.lock only), shell (the program `strata`),
    # word (strata-NAME), folder (strata/ and docs/strata/).
    return [
        # Not crate::strata:: or pb::strata_server:: (a module of the client
        # crates; the generated gRPC module).
        ("crate", re.compile(r"(?<![\w:])strata::"), "chloedb::"),
        ("crate", re.compile(r"(?<![\w:])strata_(server|crawl|sheets)::"), r"chloedb_\1::"),
        ("crate", re.compile(r"(?<![\w.-])strata(_server|_crawl|_sheets)?=(error|warn|info|debug|trace)\b"), r"chloedb\1=\2"),
        # The programs cargo builds, wherever their names otherwise stay.
        ("crate", re.compile(r"CARGO_BIN_EXE_strata(?=[-\W])"), "CARGO_BIN_EXE_chloedb"),
        ("cargo", re.compile(r'(?m)^strata = \{ path = "(\.\./)?strata"'), r'chloedb = { path = "\1chloedb"'),
        ("cargo", re.compile(r'(?m)^strata = \{ path = "\.\."'), 'chloedb = { path = ".."'),
        ("cargo", re.compile(r'"strata/(\w+)"'), r'"chloedb/\1"'),
        ("cargo", re.compile(r'(?m)^name = "strata"$'), 'name = "chloedb"'),
        ("cargo", re.compile(r'(?m)^(\s*)"strata",$'), r'\1"chloedb",'),
        ("shell", re.compile(r"(?<=[/ ])(release|ci|debug)/strata(?![\w/.-])"), r"\1/chloedb"),
        ("shell", re.compile(r'#\[command\(name = "strata"'), '#[command(name = "chloedb"'),
        ("shell", re.compile(r"(?<=--bin )strata(?![\w./-])"), "chloedb"),

        ("word", word, r"chloedb-\1"),
        ("folder", re.compile(r"(?<=\bcd )(\.\./)?strata(?![\w./-])"), r"\1chloedb"),
        ("folder", re.compile(r"(?<![\w.])\.\./strata(?![\w/.-])"), "../chloedb"),
        ("folder", re.compile(r"(?<=working-directory: )strata(?![\w./-])"), "chloedb"),
        ("folder", re.compile(r"(?<=workspaces: )strata(?![\w./-])"), "chloedb"),
        ("folder", re.compile(r"(?<=\s-p )strata(?![\w./-])"), "chloedb"),
        ("folder", re.compile(r"(?<=--package )strata(?![\w./-])"), "chloedb"),
        ("folder", re.compile(r"(?<![\w.-])docs/strata(?![\w.-])"), "docs/chloedb"),
        ("folder", re.compile(r"(?<=/(?:blob|tree)/main/)strata(?![\w.-])"), "chloedb"),
        # A path written in pieces: join(here, "..", "strata", "tests"),
        # parents[2] / "strata" / "tests".
        ("folder", re.compile(r"""((?:["']\.\.["'],\s*|/\s*))(["'])strata\2(?=\s*[,/]\s*["'](?:tests|src|results|scripts|examples|benches|fuzz|ui|python|proto|deploy|report|notes)["'])"""), r"\1\2chloedb\2"),
    ]


# strata/ after one of these is the crate's folder (`/strata/` in CODEOWNERS,
# `../strata/`, `.../blob/main/strata/`); after anything else it is something
# else: the repository, the website, a data folder, a namespace, a package.
FOLDER_AFTER = {"", ".", "..", "docs", "main", "$GITHUB_WORKSPACE", "$RUNNER_TEMP"}
SLASH = re.compile(r"(?<![\w.])(?<!\w-)strata/")
DELIM = re.compile(r"[\s'\"`(\[{=:,;|<>]")


def slash(text):
    out, last = [], 0
    for m in SLASH.finditer(text):
        i = m.start()
        if i > 0 and text[i - 1] == "/":
            j = i - 1
            while j > 0 and text[j - 1] != "/" and not DELIM.match(text[j - 1]):
                j -= 1
            if text[j:i - 1] not in FOLDER_AFTER:
                continue
        out.append(text[last:i])
        out.append("chloedb/")
        last = m.end()
    out.append(text[last:])
    return "".join(out)


# Kept as they are: names other programs and people rely on, which a later
# phase renames with care (the crawler's user agent and robots.txt token,
# paging keys an incident is deduplicated by, the hosted models' user
# agent), results read relative to a results folder, and the checkout's
# own folder, which is named after the repository (`git clone .../strata
# && cd strata`, `../strata/strata-js`).
KEEP = re.compile("|".join([
    r"strata-crawl/(?=[\d{])",
    r"User-agent: strata-crawl",
    r'AGENT_TOKEN: &str = "strata-crawl"',
    r"group `strata-crawl`",
    r"names `strata-crawl`",
    r'"strata/\{',
    r'f"strata/',
    r"strata/(?=(?:strata|chloedb)(?:[-/\s\"'`]|$))",
    r"(?<=chloetech-ai/strata && )cd strata",
    r"(?<=chloetech-ai/strata &amp;&amp; )cd strata",
    r"cd strata(?= && (?:\./install\.sh|git pull))",
]))


def protect(text):
    kept = []

    def hold(m):
        kept.append(m.group(0))
        return f"\0{len(kept) - 1}\0"
    return KEEP.sub(hold, text), kept


def restore(text, kept):
    return re.sub(r"\0(\d+)\0", lambda m: kept[int(m.group(1))], text)


LINK = re.compile(r"(github\.com/chloetech-ai/strata/(?:blob|tree)/[\w.-]+/(?:docs/)?)strata(-(?:server|crawl|sheets|py|js|java|apps))?(?=[/)\s\"'#]|$)")


def rewrite(path, text, compiled):
    if measured(path):
        return LINK.sub(lambda m: m.group(1) + "chloedb" + (m.group(2) or ""), text)
    cargo = os.path.basename(path) in ("Cargo.toml", "Cargo.lock")
    text, kept = protect(text)
    for kind, rx, rep in compiled:
        if kind == "cargo" and not cargo:
            continue
        if kind in ("word", "shell") and keeps_names(path):
            continue
        text = rx.sub(rep, text)
    text = restore(slash(text), kept)
    if os.path.basename(path) == "Cargo.lock":
        text = sort_lock(text)
    return text


# Cargo.lock lists packages, and each package's dependencies, by name: a
# renamed one moves to where cargo would put it, so that the next build
# leaves the file as it is.
RENAMED = {new for old, new in FOLDERS if not old.startswith("docs/")} | {"chloedb-" + p for p in PACKAGES}


def name_of(entry):
    # ' "name version (source)",' or ' "name",'
    return entry.strip().strip(',').strip('"').split(' ')[0]

def place(items, moved, key):
    """Move each item in `moved` (indexes) to before the first other item whose key is greater."""
    keep = [x for i, x in enumerate(items) if i not in moved]
    for i in sorted(moved):
        x = items[i]
        k = key(x)
        j = next((n for n, y in enumerate(keep) if key(y) > k), len(keep))
        keep.insert(j, x)
    return keep

def sort_lock(text):
    """Cargo.lock keeps packages and each package's dependencies sorted by name:
    put the renamed ones where cargo would."""
    head, *blocks = text.split("\n[[package]]\n")
    out = []
    for b in blocks:
        m = re.search(r"(?ms)^dependencies = \[\n(.*?)\n\]", b)
        if m:
            deps = m.group(1).split("\n")
            moved = {i for i, d in enumerate(deps) if name_of(d) in RENAMED}
            if moved:
                deps = place(deps, moved, name_of)
                b = b[:m.start(1)] + "\n".join(deps) + b[m.end(1):]
        out.append(b)
    pkg = lambda b: re.match(r'name = "([^"]+)"', b).group(1)
    moved = {i for i, b in enumerate(out) if pkg(b) in RENAMED}
    out = place(out, moved, pkg)
    return "\n[[package]]\n".join([head] + out)


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
    programs = program_names()
    for old, new in FOLDERS:
        move(old, new, a.dry_run)
    for _, d in BIN_DIRS:
        if os.path.isdir(d):
            for entry in sorted(os.listdir(d)):
                if entry == "strata.rs" or entry.startswith("strata-"):
                    move(os.path.join(d, entry), os.path.join(d, "chloedb" + entry[len("strata"):]), a.dry_run)
    if os.path.isdir("docs/chloedb") and not os.path.exists("docs/strata") and not a.dry_run:
        os.makedirs("docs/strata")
        with open("docs/strata/README.md", "w") as f:
            f.write("# Moved\n\nChloeDB was called Strata, and its documentation moved to [docs/chloedb](../chloedb/).\n")
        # Crawlers in use link to this page in their user agent.
        with open("docs/strata/crawler-bot.html", "w") as f:
            f.write(BOT_PAGE)
        subprocess.run(["git", "add", "docs/strata/README.md", "docs/strata/crawler-bot.html"], check=True)
    compiled = rules(programs)
    changed = 0
    for path, text in text_files():
        new = rewrite(path, text, compiled)
        if new != text:
            changed += 1
            if a.dry_run:
                print("would rewrite", path)
            else:
                with open(path, "w") as f:
                    f.write(new)
    print(f"{changed} files rewritten; programs: {', '.join(sorted(programs))}")


if __name__ == "__main__":
    main()
