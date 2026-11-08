#!/usr/bin/env python3
"""Stamp each page's links to assets/*.css and assets/*.js with a hash of the file.

GitHub Pages lets a browser keep a stylesheet or script for ten minutes
(cache-control: max-age=600). A page published in that window would load
with the previous stylesheet and look broken. With `?v=<hash>` on each
link, a changed file has a new address and is fetched at once.

Run it after editing anything in assets/:

    python3 docs/site/scripts/stamp_assets.py
"""
import hashlib
import pathlib
import re

SITE = pathlib.Path(__file__).resolve().parent.parent
LINK = re.compile(r'(assets/([A-Za-z0-9_.-]+\.(?:css|js)))(\?v=[0-9a-f]*)?"')


def main():
    hashes = {}
    for f in (SITE / "assets").iterdir():
        if f.suffix in (".css", ".js"):
            hashes[f.name] = hashlib.sha256(f.read_bytes()).hexdigest()[:10]
    changed = 0
    for page in sorted(SITE.glob("*.html")):
        text = page.read_text()
        new = LINK.sub(lambda m: m.group(1) + ("?v=" + hashes[m.group(2)] if m.group(2) in hashes else "") + '"', text)
        if new != text:
            page.write_text(new)
            changed += 1
    print(f"stamped {changed} pages")


if __name__ == "__main__":
    main()
