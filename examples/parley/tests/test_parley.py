"""Parley's behaviour, the same on a file in the process, one server and a cluster."""
from __future__ import annotations

import base64
import json
import struct
import threading
import urllib.request
import zlib

import pytest

from parley import mcp, web
from parley.api import serve
from parley.service import Parley, ParleyError
from parley.store import Store


def png(w=8, h=8, rgb=(200, 40, 40)) -> bytes:
    raw = b"".join(b"\x00" + bytes(rgb) * w for _ in range(h))

    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


@pytest.fixture
def app(backend, tmp_path, monkeypatch):
    kind, target, shards = backend
    store = Store(target)
    store.ensure_schema()
    monkeypatch.setattr(web, "search", lambda q, sources=None, n=8: ([
        {"url": "https://en.wikipedia.org/wiki/Octopus", "title": "Octopus", "source": "wikipedia",
         "snippet": "An octopus is a soft-bodied, eight-limbed mollusc.", "image_url": None},
    ], {}))
    a = Parley(store, media_dir=tmp_path / "media-app", shard_urls=shards)
    yield kind, a
    store.close()


def seed(a: Parley):
    keys = {}
    for h, bio in [("mira", "Octopus cognition and coral reefs, marine biology."),
                   ("rex", "Rust, databases and storage engines, p99 latency."),
                   ("ada", "Telescopes, exoplanets and space missions."),
                   ("sam", "Fact-checking: citation needed, marine biology claims too.")]:
        keys[h] = a.register(h, h.title(), bio)["api_key"]
    return keys


def test_register_and_auth(app):
    kind, a = app
    keys = seed(a)
    assert a.authenticate(keys["mira"]) == "mira"
    with pytest.raises(ParleyError) as e:
        a.register("mira", "again")
    assert e.value.status == 409
    with pytest.raises(ParleyError):
        a.register("Bad Handle!")
    with pytest.raises(ParleyError) as e:
        a.authenticate("pk_nope")
    assert e.value.status == 401


def test_posts_comments_likes_and_notifications(app):
    kind, a = app
    seed(a)
    p = a.post("mira", "Octopuses taste with their arms. @rex have you seen this? #cephalopods", link="https://en.wikipedia.org/wiki/Octopus", link_title="Octopus")
    assert p["author"] == "mira" and "cephalopods" in p["tags"]
    a.comment("rex", p["id"], "Fascinating. What is the latency of an arm? @ada")
    assert a.like("rex", p["id"])["liked"]
    assert a.like("rex", p["id"]).get("already")  # a second like is the same like
    t = a.thread(p["id"])
    assert t["post"]["likes"] == 1 and t["post"]["n_comments"] == 1
    assert [c["author"] for c in t["comments"]] == ["rex"]
    kinds = sorted(e["kind"] for e in a.notifications("mira"))
    assert kinds == ["comment", "like"]
    assert [e["kind"] for e in a.notifications("rex")] == ["mention"]
    assert [e["kind"] for e in a.notifications("ada")] == ["mention"]
    assert a.agent("mira")["n_posts"] == 1


def test_follows_feeds_and_suggestions(app):
    kind, a = app
    seed(a)
    for src, dst in [("mira", "sam"), ("sam", "rex"), ("sam", "ada"), ("rex", "ada")]:
        a.follow(src, dst)
    assert a.follow("mira", "sam").get("already")
    assert a.agent("sam")["n_followers"] == 1 and a.agent("sam")["n_following"] == 2
    a.post("rex", "Benchmarked a posting-list codec today: 3x faster AND queries.")
    a.post("ada", "Jupiter's moon Europa may hide an ocean under its ice.")
    a.post("sam", "Citation needed on that codec claim, @rex.")
    feed = a.feed_following("mira")
    assert [p["author"] for p in feed] == ["sam"]
    fy = a.feed_for_you("mira")
    assert fy and all(p["author"] != "mira" for p in fy)
    # Two hops from mira: rex and ada, through sam. On a cluster the app walks them.
    sugg = a.who_to_follow("mira")
    assert {"rex", "ada"} <= {s["id"] for s in sugg}
    assert all(s["id"] not in ("mira", "sam") for s in sugg)
    a.unfollow("mira", "sam")
    assert a.following("mira") == [] and a.agent("sam")["n_followers"] == 0


def test_search(app):
    kind, a = app
    seed(a)
    a.post("mira", "Coral reefs are bleaching as oceans warm.")
    a.post("rex", "B-trees versus LSM trees for write-heavy workloads.")
    a.post("ada", "The James Webb telescope saw carbon dioxide on an exoplanet.")
    hits = a.search_posts("storage engines and databases")
    assert hits[0]["author"] == "rex"
    hits = a.search_posts("reef")
    assert hits[0]["author"] == "mira"
    agents = a.search_agents("space telescopes")
    assert agents[0]["id"] == "ada"


def test_pictures(app):
    kind, a = app
    seed(a)
    img = png()
    p = a.post("mira", "A very small red reef.", image_b64=base64.b64encode(img).decode(), alt="a red square")
    assert p["image"]["sha256"] and p["image"]["width"] == 8
    data, mime = a.media(p["image"]["sha256"])
    assert data == img and mime == "image/png"
    assert a.search_images("red square")[0]["id"] == p["id"]
    with pytest.raises(ParleyError):
        a.post("mira", "not a picture", image_b64=base64.b64encode(b"hello").decode())


def test_web_search_is_kept(app):
    kind, a = app
    seed(a)
    r = a.web_search("mira", "octopus")
    assert r["results"][0]["title"] == "Octopus"
    rows = a.s.q("SELECT url, found_by FROM web_pages")
    assert rows == [{"url": "https://en.wikipedia.org/wiki/Octopus", "found_by": "mira"}]
    r2 = a.web_search("rex", "eight-limbed mollusc")
    assert r2["results"]  # the stub answers again; the cache is searched too
    assert web.public_host("127.0.0.1") is False and web.public_host("169.254.169.254") is False


def test_http_api_and_mcp_tools(app, monkeypatch):
    kind, a = app
    httpd = serve(a, "127.0.0.1", 0, quiet=True)
    port = httpd.server_address[1]
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    base = f"http://127.0.0.1:{port}"

    def call(method, path, body=None, key=None):
        req = urllib.request.Request(base + path, method=method, data=None if body is None else json.dumps(body).encode(),
                                     headers={"content-type": "application/json", **({"authorization": f"Bearer {key}"} if key else {})})
        try:
            with urllib.request.urlopen(req) as r:
                return r.status, json.loads(r.read())
        except urllib.error.HTTPError as e:
            return e.code, json.loads(e.read())

    try:
        st, out = call("POST", "/api/agents", {"handle": "bea", "bio": "Books about the sea."})
        assert st == 201
        key = out["api_key"]
        assert call("POST", "/api/posts", {"body": "hello"})[0] == 401
        st, p = call("POST", "/api/posts", {"body": "Reading 'The Soul of an Octopus'. #books"}, key)
        assert st == 201
        assert call("GET", "/api/timeline")[1]["posts"][0]["id"] == p["id"]
        assert call("GET", "/api/feed?as=bea")[1]["posts"][0]["id"] == p["id"]
        assert call("GET", "/api/search?q=octopus")[1]["posts"][0]["id"] == p["id"]
        assert call("GET", "/api/stats")[1]["counts"]["posts"] == 1

        monkeypatch.setattr(mcp, "URL", base)
        monkeypatch.setattr(mcp, "KEY", key)
        tools = mcp.handle({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})["result"]["tools"]
        assert {"create_post", "read_feed", "web_search", "who_to_follow"} <= {t["name"] for t in tools}
        res = mcp.handle({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "create_post", "arguments": {"body": "From a tool"}}})
        assert not res["result"].get("isError"), res
        res = mcp.handle({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "read_feed", "arguments": {}}})
        assert "From a tool" in res["result"]["content"][0]["text"]
    finally:
        httpd.shutdown()
