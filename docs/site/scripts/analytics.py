#!/usr/bin/env python3
"""The website's analytics: Google Tag Manager's snippet on every page.

The container (GTM-MGQQR27B) holds the Google Analytics 4 tag, so what it
counts is in Google Analytics; which tags run is set in Tag Manager, not
here. build_docs.py puts the snippet on the docs pages it writes; run this
script to put it on the hand-written pages (the site's pages, the 404 page
and the docs snapshots of earlier releases) that lack it:

    python3 docs/site/scripts/analytics.py           # adds it where it is missing
    python3 docs/site/scripts/analytics.py --check   # fails if a page lacks it

The head part goes right after <meta charset>, as high in <head> as Tag
Manager asks, and the noscript part right after <body>.
"""
import pathlib
import re
import sys

CONTAINER = "GTM-MGQQR27B"

HEAD = f"""<!-- Google Tag Manager -->
<script>(function(w,d,s,l,i){{w[l]=w[l]||[];w[l].push({{'gtm.start':
new Date().getTime(),event:'gtm.js'}});var f=d.getElementsByTagName(s)[0],
j=d.createElement(s),dl=l!='dataLayer'?'&l='+l:'';j.async=true;j.src=
'https://www.googletagmanager.com/gtm.js?id='+i+dl;f.parentNode.insertBefore(j,f);
}})(window,document,'script','dataLayer','{CONTAINER}');</script>
<!-- End Google Tag Manager -->"""

BODY = f"""<!-- Google Tag Manager (noscript) -->
<noscript><iframe src="https://www.googletagmanager.com/ns.html?id={CONTAINER}"
height="0" width="0" style="display:none;visibility:hidden"></iframe></noscript>
<!-- End Google Tag Manager (noscript) -->"""

DOCS = pathlib.Path(__file__).resolve().parents[2]


def pages():
    """The pages that carry the snippet: everything under docs/site/ and the
    404 page. Left out: docs/index.html and the stubs that only redirect (a
    visit would count twice), the reports and the static inspector demo."""
    return sorted(DOCS.glob("site/**/*.html")) + [DOCS / "404.html"]


def add(text):
    if "googletagmanager.com/gtm.js" in text:
        return text
    text, n = re.subn(r'(<meta charset="utf-8">\n)', lambda m: m.group(1) + HEAD + "\n", text, count=1)
    text, m = re.subn(r"(<body[^>]*>\n)", lambda m: m.group(1) + BODY + "\n", text, count=1)
    if n != 1 or m != 1:
        raise ValueError("no <meta charset> or <body> line to put it after")
    return text


def main():
    check = "--check" in sys.argv[1:]
    missing, changed = [], 0
    for page in pages():
        text = page.read_text()
        try:
            new = add(text)
        except ValueError as e:
            sys.exit(f"{page.relative_to(DOCS)}: {e}")
        if new != text:
            if check:
                missing.append(str(page.relative_to(DOCS)))
            else:
                page.write_text(new)
                changed += 1
    if check:
        if missing:
            sys.exit("pages without the analytics snippet (run docs/site/scripts/analytics.py):\n  " + "\n  ".join(missing))
        print(f"all {len(pages())} pages carry the analytics snippet")
    else:
        print(f"added the analytics snippet to {changed} pages")


if __name__ == "__main__":
    main()
