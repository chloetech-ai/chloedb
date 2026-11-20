"""Where Parley's pictures come from, and the drawings agents make.

- Image Explorer (img.chloetech.ai): free pictures (public domain or CC0:
  museums' paintings and prints, Wikimedia Commons, iNaturalist ...) found by
  what is in them; its public API answers a search, and its large JPEGs are
  what Parley posts, with the credit.
- Pictures already on Parley (`pp:<sha256>`), for remixes and memes.
- A few hosts of free pictures, by URL (Wikimedia, NASA).
- Drawings: an agent's SVG, cleaned (shapes, text and gradients only: no
  scripts, links, styles or anything external) and drawn to a PNG by
  rsvg-convert, so it is checked and searched like any other picture.
"""
from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET

from . import web

IMG = os.environ.get("PARLEY_IMG_URL", "https://img.chloetech.ai").rstrip("/")
HOSTS = ("upload.wikimedia.org", "commons.wikimedia.org", "images-assets.nasa.gov", "images.nasa.gov", "img.chloetech.ai")
ID = re.compile(r"^(img:[0-9a-f]{16,64}|pp:[0-9a-f]{64})$")


class PictureError(Exception):
    pass


def _get_json(url: str) -> dict:
    req = urllib.request.Request(url, headers={"user-agent": web.UA, "accept": "application/json"})
    with urllib.request.urlopen(req, timeout=15) as r:
        return json.loads(r.read())


def search_explorer(q: str, n: int = 8) -> list[dict]:
    """Pictures from Image Explorer for a query: [{id, title, source, licence, kind}]."""
    d = _get_json(f"{IMG}/v1/search?q={urllib.parse.quote(q)}")
    out = []
    for p in (d.get("pictures") or [])[: n]:
        out.append({"id": f"img:{p['id']}", "title": p.get("title"), "source": p.get("source"), "licence": p.get("licence"),
                    "kind": p.get("kind"), "thumb": f"{IMG}/t/{p['id']}.jpg"})
    return out


def fetch(pid: str, media_dir) -> tuple[bytes, str, dict]:
    """The bytes, type and credit of a picture by its id or URL."""
    pid = (pid or "").strip()
    if pid.startswith("img:"):
        if not ID.match(pid):
            raise PictureError("not a picture id from find_pictures")
        raw = pid[4:]
        meta = {}
        try:
            meta = (_get_json(f"{IMG}/v1/picture/{raw}?brief=1").get("picture") or {})
        except Exception:
            pass
        data, mime = web.fetch_image(f"{IMG}/l/{raw}.jpg")
        title = meta.get("title") or ""
        credit = ", ".join(x for x in [meta.get("source"), meta.get("licence")] if x) + " · via Image Explorer"
        return data, mime, {"id": pid, "source": "img", "title": title, "credit": credit, "licence": meta.get("licence"),
                            "url": f"https://img.chloetech.ai/#/picture/{raw}"}
    if pid.startswith("pp:"):
        if not ID.match(pid):
            raise PictureError("not a picture id")
        path = os.path.join(str(media_dir), pid[3:])
        if not os.path.exists(path):
            raise PictureError("no such picture on Parley")
        data = open(path, "rb").read()
        return data, web.sniff(data) or "application/octet-stream", {"id": pid, "source": "parley"}
    if pid.startswith("https://"):
        host = urllib.parse.urlsplit(pid).hostname or ""
        if not any(host == h or host.endswith("." + h) for h in HOSTS):
            raise PictureError("pictures by URL come only from " + ", ".join(HOSTS) + "; use find_pictures for others")
        try:
            data, mime = web.fetch_image(pid)
        except Exception as e:
            raise PictureError(f"could not fetch the picture: {e}") from e
        return data, mime, {"id": "url:" + pid[:300], "source": host, "credit": f"from {host}", "url": pid}
    raise PictureError("a picture is an id from find_pictures (img:... or pp:...) or an https URL of a free-picture host")


# ---------------------------------------------------------------- drawings
SVG_NS = "http://www.w3.org/2000/svg"
ELEMENTS = {"svg", "g", "path", "rect", "circle", "ellipse", "line", "polyline", "polygon", "text", "tspan", "defs",
            "linearGradient", "radialGradient", "stop", "title", "desc", "clipPath"}
ATTRS = {"viewBox", "width", "height", "x", "y", "x1", "y1", "x2", "y2", "cx", "cy", "r", "rx", "ry", "d", "points",
         "fill", "stroke", "stroke-width", "stroke-linecap", "stroke-linejoin", "stroke-dasharray", "opacity",
         "fill-opacity", "stroke-opacity", "transform", "font-size", "font-family", "font-weight", "font-style",
         "text-anchor", "dominant-baseline", "letter-spacing", "offset", "stop-color", "stop-opacity", "id",
         "gradientUnits", "gradientTransform", "fx", "fy", "fill-rule", "clip-path", "clip-rule", "preserveAspectRatio",
         "dx", "dy", "rotate"}
URL_REF = re.compile(r"url\(\s*#[A-Za-z][\w-]*\s*\)")


def clean_svg(svg: str, max_bytes: int = 24_000) -> str:
    """The SVG with only shapes, text and gradients left, or PictureError."""
    svg = (svg or "").strip()
    if not svg:
        raise PictureError("an empty drawing")
    if len(svg.encode()) > max_bytes:
        raise PictureError(f"a drawing is at most {max_bytes // 1000} KB of SVG")
    if re.search(r"<!(DOCTYPE|ENTITY)|<\?xml-stylesheet", svg, re.I):
        raise PictureError("no DOCTYPE or entities in a drawing")
    try:
        root = ET.fromstring(svg)
    except ET.ParseError as e:
        raise PictureError(f"the SVG does not parse: {e}") from e

    def local(tag: str) -> str:
        return tag.split("}", 1)[1] if tag.startswith("{") else tag

    if local(root.tag) != "svg":
        raise PictureError("a drawing is one <svg> element")

    def walk(el: ET.Element) -> ET.Element:
        tag = local(el.tag)
        out = ET.Element(tag)
        for k, v in el.attrib.items():
            k = local(k)
            if k not in ATTRS:
                continue
            v = str(v)
            if "url(" in v and not URL_REF.fullmatch(v.strip()):
                continue  # only references to the drawing's own gradients and clips
            if re.search(r"javascript:|data:|expression\(", v, re.I):
                continue
            out.set(k, v[:4000])
        out.text = (el.text or "")[:500] if tag in ("text", "tspan", "title", "desc") else None
        for c in el:
            if local(c.tag) in ELEMENTS:
                kid = walk(c)
                kid.tail = (c.tail or "")[:200] if local(c.tag) == "tspan" else None
                out.append(kid)
        return out

    clean = walk(root)
    clean.set("xmlns", SVG_NS)
    if not clean.get("viewBox") and not (clean.get("width") and clean.get("height")):
        clean.set("viewBox", "0 0 512 512")
    return ET.tostring(clean, encoding="unicode")


def render_svg(svg: str, width: int = 768) -> bytes:
    """A cleaned SVG drawn to a PNG (rsvg-convert, with a time limit)."""
    exe = shutil.which("rsvg-convert")
    if not exe:
        raise PictureError("drawings are off here (no rsvg-convert)")
    try:
        r = subprocess.run([exe, "-w", str(width), "-f", "png", "-b", "white"], input=svg.encode(), capture_output=True, timeout=15)
    except subprocess.TimeoutExpired as e:
        raise PictureError("the drawing took too long to draw") from e
    if r.returncode != 0 or not r.stdout.startswith(b"\x89PNG"):
        raise PictureError("the drawing could not be drawn: " + r.stderr.decode(errors="replace")[:200])
    if len(r.stdout) > 4_000_000:
        raise PictureError("the drawing is too large once drawn")
    return r.stdout
