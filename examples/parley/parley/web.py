"""Web search and page reading for agents.

Parley does not run a web index of its own. It asks public APIs that welcome
programs (Hacker News through Algolia, Wikipedia's page summaries, arXiv, Open
Library), plus Brave Search or Tavily when a key is in the environment, and
keeps everything an agent found in ChloeDB's `web_pages` table, so the next
agent's search reads the network's own copy first.

Requests say who they are (a descriptive User-Agent), read robots.txt before
reading a page, and stay small.
"""
from __future__ import annotations

import html
import html.parser
import ipaddress
import json
import os
import re
import socket
import urllib.parse
import urllib.request
import urllib.robotparser
import xml.etree.ElementTree as ET
from functools import lru_cache

UA = "ParleyPrototype/0.1 (agent social network prototype; https://github.com/chloetech-ai/chloedb)"
TIMEOUT = 12


def public_host(host: str) -> bool:
    """True when every address the name resolves to is public. Agents choose
    the URLs they read, so a page or an image must not be a way into this
    machine's network (127.0.0.1, 10.0.0.0/8, the cloud's metadata address...)."""
    try:
        infos = socket.getaddrinfo(host, None)
    except socket.gaierror:
        return False
    for info in infos:
        ip = ipaddress.ip_address(info[4][0])
        if not ip.is_global or ip.is_multicast:
            return False
    return bool(infos)


class _PublicOnly(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        host = urllib.parse.urlparse(newurl).hostname or ""
        if not public_host(host):
            raise PermissionError(f"redirect to a private address refused: {host}")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


_opener = urllib.request.build_opener(_PublicOnly)


def _get(url: str, *, headers: dict | None = None, limit: int = 2_000_000) -> tuple[bytes, str]:
    host = urllib.parse.urlparse(url).hostname or ""
    if not public_host(host):
        raise PermissionError(f"not a public address: {host}")
    req = urllib.request.Request(url, headers={"User-Agent": UA, **(headers or {})})
    with _opener.open(req, timeout=TIMEOUT) as r:
        return r.read(limit), r.headers.get("content-type", "")


def _json(url: str, **kw) -> dict:
    body, _ = _get(url, **kw)
    return json.loads(body)


def hackernews(q: str, n: int = 5) -> list[dict]:
    d = _json("https://hn.algolia.com/api/v1/search?" + urllib.parse.urlencode({"query": q, "tags": "story", "hitsPerPage": n}))
    out = []
    for h in d.get("hits", []):
        url = h.get("url") or f"https://news.ycombinator.com/item?id={h.get('objectID')}"
        out.append({"url": url, "title": h.get("title") or "", "source": "hackernews",
                    "snippet": f"{h.get('points', 0)} points, {h.get('num_comments', 0)} comments on Hacker News ({(h.get('created_at') or '')[:10]})"})
    return out


def wikipedia(q: str, n: int = 3) -> list[dict]:
    """Wikipedia's search API answers this machine's shared address with 429,
    so read the page summary of the query as a title, then of its words'
    capitalised form: entity-like queries ("octopus", "Alan Turing") find it."""
    out = []
    tried = set()
    # Titles start with a capital: "octopus" is a redirect (and a second request).
    first = q[:1].upper() + q[1:]
    for title in (first, q.title(), q.split(" ")[0].capitalize() if " " in q else None):
        if not title or title in tried:
            continue
        tried.add(title)
        try:
            d = _json("https://en.wikipedia.org/api/rest_v1/page/summary/" + urllib.parse.quote(title.replace(" ", "_")))
        except Exception:
            continue
        if d.get("type") == "disambiguation" or not d.get("extract"):
            continue
        url = (d.get("content_urls", {}).get("desktop", {}) or {}).get("page") or ""
        if any(o["url"] == url for o in out):
            continue
        out.append({"url": url, "title": d.get("title", title), "source": "wikipedia",
                    "snippet": d.get("extract", "")[:600], "body": d.get("extract", ""),
                    "image_url": (d.get("thumbnail") or {}).get("source")})
        if len(out) >= n:
            break
    return out


def arxiv(q: str, n: int = 4) -> list[dict]:
    body, _ = _get("https://export.arxiv.org/api/query?" + urllib.parse.urlencode({"search_query": f"all:{q}", "max_results": n, "sortBy": "relevance"}))
    ns = {"a": "http://www.w3.org/2005/Atom"}
    out = []
    for e in ET.fromstring(body).findall("a:entry", ns):
        title = " ".join((e.findtext("a:title", "", ns) or "").split())
        summary = " ".join((e.findtext("a:summary", "", ns) or "").split())
        url = e.findtext("a:id", "", ns) or ""
        out.append({"url": url, "title": title, "source": "arxiv", "snippet": summary[:600], "body": summary})
    return out


def openlibrary(q: str, n: int = 3) -> list[dict]:
    d = _json("https://openlibrary.org/search.json?" + urllib.parse.urlencode({"q": q, "limit": n, "fields": "key,title,author_name,first_publish_year,cover_i"}))
    out = []
    for doc in d.get("docs", []):
        authors = ", ".join(doc.get("author_name", [])[:3])
        cover = doc.get("cover_i")
        out.append({"url": "https://openlibrary.org" + doc.get("key", ""), "title": doc.get("title", ""), "source": "openlibrary",
                    "snippet": f"Book by {authors or 'unknown'}, first published {doc.get('first_publish_year', '?')}.",
                    "image_url": f"https://covers.openlibrary.org/b/id/{cover}-M.jpg" if cover else None})
    return out


def brave(q: str, n: int = 6) -> list[dict]:
    key = os.environ.get("BRAVE_API_KEY")
    if not key:
        return []
    d = _json("https://api.search.brave.com/res/v1/web/search?" + urllib.parse.urlencode({"q": q, "count": n}),
              headers={"X-Subscription-Token": key, "Accept": "application/json"})
    return [{"url": r.get("url", ""), "title": r.get("title", ""), "source": "brave", "snippet": _strip(r.get("description", ""))}
            for r in (d.get("web") or {}).get("results", [])]


def tavily(q: str, n: int = 6) -> list[dict]:
    key = os.environ.get("TAVILY_API_KEY")
    if not key:
        return []
    req = urllib.request.Request("https://api.tavily.com/search", method="POST",
                                 data=json.dumps({"query": q, "max_results": n}).encode(),
                                 headers={"User-Agent": UA, "content-type": "application/json", "authorization": f"Bearer {key}"})
    with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
        d = json.loads(r.read())
    return [{"url": x.get("url", ""), "title": x.get("title", ""), "source": "tavily", "snippet": x.get("content", "")[:600]} for x in d.get("results", [])]


PROVIDERS = {"brave": brave, "tavily": tavily, "hackernews": hackernews, "wikipedia": wikipedia, "arxiv": arxiv, "openlibrary": openlibrary}
DEFAULT = ["brave", "tavily", "wikipedia", "hackernews", "arxiv"]


def search(q: str, sources: list[str] | None = None, n: int = 8) -> tuple[list[dict], dict]:
    """Results from each source in turn, deduplicated by URL, and each source's error."""
    results, errors, seen = [], {}, set()
    for name in sources or DEFAULT:
        fn = PROVIDERS.get(name)
        if not fn:
            continue
        try:
            for r in fn(q):
                if r["url"] and r["url"] not in seen:
                    seen.add(r["url"])
                    results.append(r)
        except Exception as e:  # one source down is not a failed search
            errors[name] = f"{type(e).__name__}: {e}"[:200]
        if len(results) >= n:
            break
    return results[:n], errors


@lru_cache(maxsize=256)
def _robots(origin: str) -> urllib.robotparser.RobotFileParser | None:
    rp = urllib.robotparser.RobotFileParser()
    try:
        body, _ = _get(origin + "/robots.txt", limit=200_000)
        rp.parse(body.decode("utf-8", "replace").splitlines())
        return rp
    except Exception:
        return None  # no robots.txt: allowed


def allowed(url: str) -> bool:
    p = urllib.parse.urlparse(url)
    rp = _robots(f"{p.scheme}://{p.netloc}")
    return True if rp is None else rp.can_fetch(UA, url)


class _Text(html.parser.HTMLParser):
    SKIP = {"script", "style", "noscript", "svg", "nav", "footer", "header", "form"}

    def __init__(self):
        super().__init__()
        self.parts, self.title, self._skip, self._in_title = [], "", 0, False
        self.og_image = None

    def handle_starttag(self, tag, attrs):
        if tag in self.SKIP:
            self._skip += 1
        if tag == "title":
            self._in_title = True
        if tag == "meta":
            a = dict(attrs)
            if a.get("property") in ("og:image", "twitter:image") and a.get("content") and not self.og_image:
                self.og_image = a["content"]
        if tag in ("p", "br", "li", "h1", "h2", "h3", "tr", "div"):
            self.parts.append("\n")

    def handle_endtag(self, tag):
        if tag in self.SKIP and self._skip:
            self._skip -= 1
        if tag == "title":
            self._in_title = False

    def handle_data(self, data):
        if self._in_title:
            self.title += data
        elif not self._skip:
            self.parts.append(data)


def read(url: str, max_chars: int = 6000) -> dict:
    if not url.startswith(("http://", "https://")):
        raise ValueError("only http(s) URLs")
    if not allowed(url):
        raise PermissionError(f"robots.txt of {urllib.parse.urlparse(url).netloc} disallows {url}")
    body, ctype = _get(url)
    if "html" not in ctype and "text" not in ctype and "xml" not in ctype:
        raise ValueError(f"not a page: {ctype}")
    p = _Text()
    p.feed(body.decode("utf-8", "replace"))
    text = re.sub(r"\n\s*\n+", "\n\n", re.sub(r"[ \t\r\f\v]+", " ", "".join(p.parts))).strip()
    return {"url": url, "title": " ".join(p.title.split()), "body": text[:max_chars], "image_url": p.og_image, "source": "page"}


def fetch_image(url: str, max_bytes: int = 5_000_000) -> tuple[bytes, str]:
    if not url.startswith(("http://", "https://")):
        raise ValueError("only http(s) URLs")
    body, ctype = _get(url, limit=max_bytes + 1)
    if len(body) > max_bytes:
        raise ValueError("image larger than 5 MB")
    mime = sniff(body)
    if not mime:
        raise ValueError(f"not an image ChloeDB decodes (JPEG, PNG, WebP, GIF, BMP): {ctype}")
    return body, mime


def sniff(b: bytes) -> str | None:
    if b.startswith(b"\xff\xd8\xff"):
        return "image/jpeg"
    if b.startswith(b"\x89PNG\r\n\x1a\n"):
        return "image/png"
    if b[:4] == b"RIFF" and b[8:12] == b"WEBP":
        return "image/webp"
    if b[:6] in (b"GIF87a", b"GIF89a"):
        return "image/gif"
    if b[:2] == b"BM":
        return "image/bmp"
    return None


def _strip(s: str) -> str:
    return html.unescape(re.sub(r"<[^>]+>", "", s or ""))
