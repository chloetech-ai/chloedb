#!/usr/bin/env python3
"""Build the documentation section of the website, docs/site/docs/, from Markdown.

The pages come from two kinds of source:

- the guide, docs/guide/*.md (and docs/quickstart.md): pages written for the
  site, whose code blocks are run by docs/guide/run.py, so every example on
  them runs against the code it is built with;
- the reference, the Markdown pages read on GitHub (docs/chloedb/*.md,
  docs/chloectx/*.md, ...), rendered as they are.

GitHub Pages publishes docs/ as it is, so the output is committed. Run it
after editing any of the sources, and commit what it writes:

    pip install -r docs/site/scripts/requirements.txt
    python3 docs/site/scripts/build_docs.py           # writes docs/site/docs/
    python3 docs/site/scripts/build_docs.py --check   # fails if docs/site/docs/ is stale

Links between the pages' Markdown files become links between the site's
pages; a link to another file of the repository goes to it on GitHub, and an
image under docs/ is linked where Pages serves it.
"""
import argparse
import hashlib
import html
import os
import pathlib
import re
import sys
import tempfile
import unicodedata

import markdown

ROOT = pathlib.Path(__file__).resolve().parents[3]
DOCS = ROOT / "docs"
SITE = DOCS / "site"
OUT = SITE / "docs"
GITHUB = "https://github.com/chloetech-ai/chloedb"
BRANCH = "main"

# The sidebar, in order: (section, [(page, title, source, kind)]). A guide
# page's examples are run by docs/guide/run.py; a reference page is
# rendered from the Markdown read on GitHub. A source that does not exist
# (a page another change adds) is left out until it does.
NAV = [
    ("Start", [
        ("index", "Overview", "docs/guide/index.md", "guide"),
        ("quickstart", "Quickstart", "docs/quickstart.md", "guide"),
        ("concepts", "Concepts", "docs/guide/concepts.md", "guide"),
        ("build-an-app", "Build an app on the HTTP API", "docs/chloedb/build-an-app.md", "reference"),
    ]),
    ("Search and SQL", [
        ("search", "Search guide", "docs/guide/search.md", "guide"),
        ("sql", "SQL reference", "docs/chloedb/query-language.md", "reference"),
        ("graphs", "Graphs", "docs/chloedb/graphs.md", "reference"),
        ("next-items", "Next items", "docs/chloedb/next-items.md", "reference"),
        ("slices", "Search on the device", "docs/chloedb/slices.md", "reference"),
        ("design", "How ChloeDB works", "docs/chloedb/index.md", "reference"),
    ]),
    ("Cookbook", [
        ("rag", "Retrieval for RAG", "docs/guide/rag.md", "guide"),
        ("agent-memory", "Memory for an agent", "docs/guide/agent-memory.md", "guide"),
        ("crawl-to-search", "Crawl a site into search", "docs/guide/crawl-to-search.md", "guide"),
    ]),
    ("Clients", [
        ("python", "Python", "docs/chloedb/python.md", "reference"),
        ("frames", "DataFrames", "docs/chloedb/frames.md", "reference"),
        ("javascript", "JavaScript", "docs/chloedb/javascript.md", "reference"),
        ("java", "Java and JDBC", "docs/chloedb/java.md", "reference"),
        ("frameworks", "LangChain, LangGraph, LlamaIndex", "docs/guide/frameworks.md", "guide"),
        ("frameworks-reference", "Framework adapters reference", "docs/chloedb/frameworks.md", "reference"),
    ]),
    ("Server and cluster", [
        ("server", "chloedb-server", "docs/chloedb/server.md", "reference"),
        ("sqlite", "SQLite in and out", "docs/chloedb/sqlite.md", "reference"),
        ("parquet", "Parquet and S3", "docs/chloedb/parquet.md", "reference"),
        ("postgres", "Following Postgres", "docs/chloedb/postgres.md", "reference"),
        ("connectors", "Connectors", "docs/chloedb/connectors.md", "reference"),
        ("opensearch", "OpenSearch API", "docs/chloedb/opensearch.md", "reference"),
        ("compatibility", "Versions and compatibility", "docs/compatibility.md", "reference"),
    ]),
    ("Apps", [
        ("apps", "Apps on a cluster", "docs/guide/apps.md", "guide"),
        ("crawler", "Crawler", "docs/chloedb/crawler.md", "reference"),
        ("sheets", "Sheets", "docs/chloedb/sheets.md", "reference"),
        ("chloectx", "ChloeCtx", "docs/chloectx/index.md", "reference"),
        ("chloectx-setup", "Install ChloeCtx", "docs/chloectx/setup.md", "reference"),
        ("chloectx-inspector", "ChloeCtx inspector", "docs/chloectx/inspector.md", "reference"),
        ("chloectx-app", "ChloeCtx on a cluster", "docs/chloectx/app.md", "reference"),
        ("chloectx-capsules", "Context capsules", "docs/chloectx/capsules.md", "reference"),
        ("chloectx-receipts", "Blame and receipts", "docs/chloectx/receipts.md", "reference"),
        ("app-engine", "App engine (prototype)", "docs/chloedb/apps.md", "reference"),
    ]),
]

LANG = {"bash": "sh", "shell": "sh", "console": "sh", "zsh": "sh", "py": "python", "javascript": "js",
        "jsonl": "json", "toml": "yaml", "text": "text", "": "text"}
TITLE_LANG = {"js": "JavaScript", "java": "Java", "text": "Output", "html": "HTML", "rust": "Rust", "ts": "TypeScript"}
FENCE = re.compile(r"^(?P<indent>[ \t]*)(?P<fence>`{3,}|~{3,})(?P<info>[^\n`]*)\n(?P<body>.*?)^(?P=indent)(?P=fence)[ \t]*$", re.M | re.S)


# Pages about code another change adds: each is left out, as a missing
# source is, until this file of the code exists.
REQUIRES = {
    "frameworks": "chloedb-py/python/chloedb/langchain.py",
}


def present(page, source):
    """Whether a page is on the site: its source, and the code it documents, exist."""
    need = REQUIRES.get(page)
    return (ROOT / source).exists() and (need is None or (ROOT / need).exists())


def pages():
    for section, items in NAV:
        for page, title, source, kind in items:
            if present(page, source):
                yield section, page, title, ROOT / source, kind


def parse_info(info):
    """A fence's info string: a language, then words and key=value pairs."""
    words = info.strip().split()
    lang = words[0].lower() if words else ""
    attrs, flags = {}, set()
    for w in words[1:]:
        if "=" in w:
            k, v = w.split("=", 1)
            attrs[k] = v.strip('"')
        else:
            flags.add(w)
    return lang, attrs, flags


def github_slug(text, seen):
    """The anchor GitHub gives a heading, so that links to them keep working."""
    s = unicodedata.normalize("NFKC", text).lower().strip()
    s = re.sub(r"[^\w\- ]", "", s)
    s = s.replace(" ", "-")
    n = seen.get(s, 0)
    seen[s] = n + 1
    return s if n == 0 else f"{s}-{n}"


def render_fence(lang, attrs, flags, body):
    lang = LANG.get(lang, lang)
    if lang == "mermaid":
        return f'<pre class="mermaid">{html.escape(body)}</pre>'
    title = attrs.get("file") or attrs.get("title")
    if "output" in flags:
        title = title or "Output"
    if not title:
        title = TITLE_LANG.get(lang, "")
    t = f' data-title="{html.escape(title)}"' if title else ""
    cls = ' class="output"' if "output" in flags else ""
    body = body.rstrip("\n")
    return f'<pre data-lang="{html.escape(lang or "text")}"{t}{cls}>{html.escape(body)}</pre>'


class Page:
    def __init__(self, section, page, title, source, kind):
        self.section, self.page, self.title, self.source, self.kind = section, page, title, source, kind
        self.rel = source.relative_to(ROOT).as_posix()


# Where a link to a page that another change adds goes until it is here.
STAND_IN = {
    "docs/quickstart.md": "README.md#quick-start",
}


def rewrite_links(body, page, by_source):
    """Point a page's relative links at the site's pages, Pages or GitHub."""
    src_dir = page.source.parent

    def fix(m):
        attr, url = m.group(1), html.unescape(m.group(2))
        if re.match(r"^[a-z][a-z0-9+.-]*:", url) or url.startswith("#") or url.startswith("//"):
            return m.group(0)
        path, _, frag = url.partition("#")
        if not path:
            return m.group(0)
        target = (src_dir / path).resolve()
        if not target.exists() and target.is_relative_to(ROOT) and target.relative_to(ROOT).as_posix() in STAND_IN:
            path, _, frag = STAND_IN[target.relative_to(ROOT).as_posix()].partition("#")
            target = ROOT / path
        frag = "#" + frag if frag else ""
        if target in by_source:
            new = by_source[target].page + ".html" + frag
        elif target.is_relative_to(DOCS) and target.exists() and target.suffix != ".md":
            new = os.path.relpath(target, OUT).replace(os.sep, "/") + frag
        elif target.is_relative_to(ROOT):
            kind = "tree" if target.is_dir() or not target.suffix else "blob"
            new = f"{GITHUB}/{kind}/{BRANCH}/{target.relative_to(ROOT).as_posix()}{frag}"
        else:
            return m.group(0)
        return f'{attr}="{html.escape(new, quote=True)}"'

    return re.sub(r'\b(href|src)="([^"]*)"', fix, body)


def convert(page, by_source):
    text = page.source.read_text()
    blocks = []

    def stash(m):
        lang, attrs, flags = parse_info(m.group("info"))
        indent = m.group("indent")
        body = re.sub(r"^" + re.escape(indent), "", m.group("body"), flags=re.M) if indent else m.group("body")
        if lang in ("sh", "bash") and body.startswith("# install\n") and page.kind == "guide":
            body = body[len("# install\n"):]
        blocks.append(render_fence(lang, attrs, flags, body))
        return f"{indent}CHLOEDBCODEBLOCK{len(blocks) - 1:05d}X"

    text = FENCE.sub(stash, text)
    # Comments that steer the example runner are not for readers.
    text = re.sub(r"^<!--\s*run:.*?-->\n?", "", text, flags=re.M)
    seen = {}
    md = markdown.Markdown(extensions=["tables", "sane_lists", "toc", "md_in_html"],
                           extension_configs={"toc": {"slugify": lambda value, sep: github_slug(value, seen), "toc_depth": "2-3"}})
    body = md.convert(text)
    title = page.title
    m = re.search(r"<h1[^>]*>(.*?)</h1>", body, re.S)
    if m:
        title = re.sub(r"<[^>]+>", "", m.group(1)).strip()
        body = body[:m.start()] + body[m.end():]
    for i, b in enumerate(blocks):
        key = f"CHLOEDBCODEBLOCK{i:05d}X"
        body = body.replace(f"<p>{key}</p>", b).replace(key, b)
    # Wide tables scroll inside the column.
    body = re.sub(r"<table>", '<div class="doc-table"><table>', body).replace("</table>", "</table></div>")
    # Headings get a link to themselves.
    body = re.sub(r'<(h[2-4]) id="([^"]+)">(.*?)</\1>',
                  lambda m: f'<{m.group(1)} id="{m.group(2)}">{m.group(3)}<a class="anchor" href="#{m.group(2)}" aria-label="Link to this section">#</a></{m.group(1)}>',
                  body, flags=re.S)
    body = rewrite_links(body, page, by_source)
    toc = [(t["level"], t["id"], t["name"]) for t in flat_toc(md.toc_tokens) if t["level"] == 2]
    desc = description(page.source.read_text())
    return title, desc, body, toc, any("mermaid" in b for b in blocks if b.startswith('<pre class="mermaid"'))


def flat_toc(tokens):
    for t in tokens:
        yield t
        yield from flat_toc(t.get("children", []))


def description(text):
    """The page's first paragraph, as plain text, for <meta name=description>."""
    text = FENCE.sub("", text)
    for para in re.split(r"\n\s*\n", text):
        p = para.strip()
        if not p or p.startswith(("#", "<!--", "|", ">", "-", "*", "!", "<")):
            continue
        p = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", p)
        p = re.sub(r"[`*_]", "", p)
        p = " ".join(p.split())
        return p if len(p) <= 220 else p[:217].rsplit(" ", 1)[0] + "…"
    return ""


def asset_hashes():
    return {f.name: hashlib.sha256(f.read_bytes()).hexdigest()[:10] for f in (SITE / "assets").iterdir() if f.suffix in (".css", ".js")}


def sidebar(current, all_pages):
    out = []
    for section, _ in NAV:
        items = [p for p in all_pages if p.section == section]
        if not items:
            continue
        out.append(f'<span class="h">{html.escape(section)}</span>')
        for p in items:
            cur = ' aria-current="page"' if p is current else ""
            out.append(f'<a href="{p.page}.html"{cur}>{html.escape(p.title)}</a>')
    return "\n      ".join(out)


TEMPLATE = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<title>{title} · ChloeDB docs</title>
<meta name="description" content="{desc}">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Inter+Tight:wght@500;600;700&family=Inter:wght@400;500;600&family=JetBrains+Mono:wght@400;500&display=swap">
<link rel="stylesheet" href="../assets/site.css?v={h_site_css}">
<link rel="stylesheet" href="../assets/docs.css?v={h_docs_css}">
</head>
<body class="docs">
<!-- Written by docs/site/scripts/build_docs.py from {source}: edit that file and run the script, not this page. -->
<header class="top gutter"><div class="wrap">
  <a class="mark" href="../index.html"><i><b></b><b></b><b></b><b></b></i>ChloeDB</a>
  <nav class="nav" aria-label="Site">
    <a href="../features.html">Features</a>
    <a href="../scale.html">Laptop to cloud</a>
    <a href="../command-center.html">Command center</a>
    <a href="../benchmarks.html">Benchmarks</a>
    <a href="../use-cases.html">Use cases</a>
  </nav>
  <div class="top-links"><a href="index.html" aria-current="page">Docs</a><a href="{github}">GitHub</a><a class="btn primary" href="{start}">Get started</a></div>
</div></header>

<main class="gutter"><div class="wrap doc-layout">
  <details class="doc-menu" open>
    <summary>Documentation</summary>
    <nav class="doc-nav" aria-label="Documentation">
      {sidebar}
    </nav>
  </details>
  <article class="doc">
    <div class="doc-head">
      <span class="eyebrow">{section}</span>
      <h1>{h1}</h1>
      {badge}
    </div>
    {toc}
    <div class="doc-body">
{body}
    </div>
    <footer class="doc-foot">
      <a href="{edit}">Edit this page on GitHub</a>
      {pager}
    </footer>
  </article>
</div></main>

<footer class="foot gutter"><div class="wrap">
  <div class="brand"><a class="mark" href="../index.html"><i><b></b><b></b><b></b><b></b></i>ChloeDB</a><p>Search engine, database and recommender in one, from a file in your app to a cluster. Apache-2.0.</p></div>
  <div><h4>Start</h4><ul><li><a href="{start}">{start_label}</a></li><li><a href="concepts.html">Concepts</a></li><li><a href="search.html">Search guide</a></li><li><a href="sql.html">SQL reference</a></li></ul></div>
  <div><h4>Cookbook</h4><ul><li><a href="rag.html">Retrieval for RAG</a></li><li><a href="agent-memory.html">Memory for an agent</a></li><li><a href="crawl-to-search.html">Crawl a site into search</a></li></ul></div>
  <div><h4>Source</h4><ul><li><a href="{github}">GitHub</a></li><li><a href="{github}/blob/{branch}/docs/reproduce.md">Reproduce the results</a></li></ul></div>
</div></footer>
<script src="../assets/site.js?v={h_site_js}"></script>
<script>if (window.matchMedia("(max-width: 900px)").matches) document.querySelector(".doc-menu").open = false;</script>
{mermaid}
</body>
</html>
"""

MERMAID = """<script type="module">
import mermaid from "https://cdn.jsdelivr.net/npm/mermaid@11.4.1/dist/mermaid.esm.min.mjs";
const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
mermaid.initialize({ startOnLoad: true, theme: dark ? "dark" : "neutral", securityLevel: "strict" });
</script>"""


def render_page(p, all_pages, by_source, hashes):
    title, desc, body, toc, has_mermaid = convert(p, by_source)
    i = all_pages.index(p)
    prev, nxt = (all_pages[i - 1] if i > 0 else None), (all_pages[i + 1] if i + 1 < len(all_pages) else None)
    pager = '<span class="pager">'
    if prev:
        pager += f'<a href="{prev.page}.html">← {html.escape(prev.title)}</a>'
    if nxt:
        pager += f'<a href="{nxt.page}.html">{html.escape(nxt.title)} →</a>'
    pager += "</span>"
    if p.kind == "guide":
        badge = ('<p class="doc-badge tested">Every example on this page runs in CI against the code it documents '
                 f'(<a href="{GITHUB}/blob/{BRANCH}/docs/guide/run.py">docs/guide/run.py</a>).</p>')
    else:
        badge = (f'<p class="doc-badge">Reference, rendered from <a href="{GITHUB}/blob/{BRANCH}/{p.rel}">{p.rel}</a>.</p>')
    toc_html = ""
    if len(toc) >= 3:
        # The list numbers its items, so a heading that numbers itself
        # ("1. Install the server") loses its own number here.
        items = "".join(f'<li><a href="#{i}">{re.sub(r"^[0-9]+[.] +", "", n)}</a></li>' for _, i, n in toc)
        toc_html = f'<details class="doc-toc" open><summary>On this page</summary><ol>{items}</ol></details>'
    start = "quickstart.html" if any(q.page == "quickstart" for q in all_pages) else "index.html"
    return TEMPLATE.format(
        title=html.escape(re.sub(r"<[^>]+>", "", html.unescape(title))), desc=html.escape(desc, quote=True),
        h_site_css=hashes.get("site.css", ""), h_docs_css=hashes.get("docs.css", ""), h_site_js=hashes.get("site.js", ""),
        source=p.rel, github=GITHUB, branch=BRANCH, sidebar=sidebar(p, all_pages), section=html.escape(p.section),
        h1=title, badge=badge, toc=toc_html, body=body, edit=f"{GITHUB}/edit/{BRANCH}/{p.rel}", pager=pager,
        mermaid=MERMAID if has_mermaid else "",
        start=start, start_label="Quickstart" if start == "quickstart.html" else "Overview")


def check_links(out, written):
    """Links between the pages, and to their sections, that lead nowhere."""
    ids = {}
    for name in written:
        ids[name] = set(re.findall(r'\bid="([^"]+)"', (out / name).read_text()))
    bad = []
    for name in sorted(written):
        for url in re.findall(r'href="([^"]+)"', (out / name).read_text()):
            url = html.unescape(url)
            if re.match(r"^[a-z][a-z0-9+.-]*:", url) or url.startswith("../"):
                continue
            page, _, frag = url.partition("#")
            page = page or name
            if page not in ids:
                bad.append(f"{name}: {url} (no such page)")
            elif frag and frag not in ids[page]:
                bad.append(f"{name}: {url} (no such section)")
    return bad


def build(out):
    all_pages = [Page(*x) for x in pages()]
    by_source = {p.source.resolve(): p for p in all_pages}
    hashes = asset_hashes()
    out.mkdir(parents=True, exist_ok=True)
    written = set()
    for p in all_pages:
        (out / f"{p.page}.html").write_text(render_page(p, all_pages, by_source, hashes))
        written.add(f"{p.page}.html")
    for line in check_links(out, written):
        print(f"broken link: {line}", file=sys.stderr)
    return written


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--check", action="store_true", help="fail if docs/site/docs/ differs from what the sources make")
    args = ap.parse_args()
    if args.check:
        with tempfile.TemporaryDirectory() as tmp:
            written = build(pathlib.Path(tmp))
            stale = []
            for name in sorted(written):
                old = OUT / name
                if not old.exists() or old.read_text() != (pathlib.Path(tmp) / name).read_text():
                    stale.append(name)
            extra = sorted(f.name for f in OUT.glob("*.html") if f.name not in written) if OUT.exists() else []
        if stale or extra:
            for n in stale:
                print(f"stale: docs/site/docs/{n}")
            for n in extra:
                print(f"not made by any source: docs/site/docs/{n}")
            print("run: python3 docs/site/scripts/build_docs.py, and commit docs/site/docs/")
            sys.exit(1)
        print(f"docs/site/docs/ is up to date ({len(written)} pages)")
        return
    written = build(OUT)
    for f in OUT.glob("*.html"):
        if f.name not in written:
            f.unlink()
    print(f"wrote {len(written)} pages to docs/site/docs/")


if __name__ == "__main__":
    main()
