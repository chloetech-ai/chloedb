#!/usr/bin/env python3
"""The Python modules and extras the docs tell readers to use exist.

Every `import chloedb.X` / `from chloedb.X import` and every
`pip install "chloedb[a,b]"` in the Markdown docs and the README must name a
module of chloedb-py/python/chloedb/ and an extra of chloedb-py/pyproject.toml.

With the latest release's tag at hand (docs/site/versions.json's "latest",
tag chloedb-py-v<it>; CI fetches the tags), a page that documents a module
or an extra the release does not have must also mark it unreleased, with a
`<!-- since <version> -->` marker (build_docs.py shows it as a badge), so a
reader of the site knows `pip install` of the release will not have it.

    python3 scripts/check-documented-python.py
"""
import json
import pathlib
import re
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
PKG = ROOT / "chloedb-py" / "python" / "chloedb"
DOCS = ["README.md", "docs/quickstart.md", "docs/guide/*.md", "docs/chloedb/*.md", "docs/chloectx/*.md"]
IMPORT = re.compile(r"^\s*(?:from\s+chloedb\.([A-Za-z_][\w.]*)\s+import|import\s+chloedb\.([A-Za-z_][\w.]*))", re.M)
EXTRAS = re.compile(r"""pip install[^\n]*?["']?chloedb\[([^\]]+)\]""")
SINCE = re.compile(r"<!--\s*since\s+v?[0-9]")


def has_module(base, name):
    top = name.split(".")[0]
    return (base / f"{top}.py").exists() or (base / top / "__init__.py").exists()


def released(tag, path):
    """Whether `path` exists at `tag` (None when the tag is not here)."""
    if subprocess.run(["git", "-C", str(ROOT), "rev-parse", "-q", "--verify", f"refs/tags/{tag}"], capture_output=True).returncode != 0:
        return None
    return subprocess.run(["git", "-C", str(ROOT), "cat-file", "-e", f"{tag}:{path}"], capture_output=True).returncode == 0


def released_extras(tag):
    r = subprocess.run(["git", "-C", str(ROOT), "show", f"{tag}:chloedb-py/pyproject.toml"], capture_output=True, text=True)
    if r.returncode != 0:
        return None
    return set(tomllib.loads(r.stdout)["project"].get("optional-dependencies", {}))


def main():
    extras = set(tomllib.loads((ROOT / "chloedb-py" / "pyproject.toml").read_text())["project"].get("optional-dependencies", {}))
    versions = ROOT / "docs" / "site" / "versions.json"
    latest = json.loads(versions.read_text()).get("latest") if versions.exists() else None
    tag = f"chloedb-py-v{latest}" if latest else None
    old_extras = released_extras(tag) if tag else None
    bad, unmarked, seen_tag = [], [], None
    for pattern in DOCS:
        for f in sorted(ROOT.glob(pattern)):
            text = f.read_text(errors="replace")
            rel = f.relative_to(ROOT).as_posix()
            marked = bool(SINCE.search(text))
            for m in IMPORT.finditer(text):
                mod = m.group(1) or m.group(2)
                line = text.count("\n", 0, m.start()) + 1
                if not has_module(PKG, mod):
                    bad.append(f"{rel}:{line}: chloedb.{mod} is not a module of chloedb-py/python/chloedb/")
                elif tag:
                    top = mod.split(".")[0]
                    r = released(tag, f"chloedb-py/python/chloedb/{top}.py")
                    r = r or released(tag, f"chloedb-py/python/chloedb/{top}/__init__.py")
                    seen_tag = seen_tag if r is None else True
                    if r is False and not marked:
                        unmarked.append(f"{rel}:{line}: chloedb.{mod} is not in release {latest}, and the page has no <!-- since ... --> marker")
            for m in EXTRAS.finditer(text):
                line = text.count("\n", 0, m.start()) + 1
                for e in (x.strip() for x in m.group(1).split(",")):
                    if e not in extras:
                        bad.append(f"{rel}:{line}: chloedb[{e}] is not an extra of chloedb-py/pyproject.toml")
                    elif old_extras is not None and e not in old_extras and not marked:
                        unmarked.append(f"{rel}:{line}: chloedb[{e}] is not in release {latest}, and the page has no <!-- since ... --> marker")
    for line in bad + unmarked:
        print(line)
    if tag and seen_tag is None and old_extras is None:
        print(f"note: tag {tag} is not here (git fetch --tags): what the release lacks was not checked")
    if bad or unmarked:
        sys.exit(1)
    print("the documented Python modules and extras exist")


if __name__ == "__main__":
    main()
