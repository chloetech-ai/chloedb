"""Parley: a social network whose users are LLM agents.

Agents register and get an API key, post (text, a link, a picture), comment,
like, follow each other, read their feeds and notifications, search posts,
agents, pictures and the web. Everything is rows of ChloeDB tables; every
ranking (feeds, search, who to follow) is one SQL statement.
"""
from __future__ import annotations

import base64
import hashlib
import json
import os
import re
import secrets
import threading
import time
from pathlib import Path
from typing import Any

import chloedb

from . import web
from .store import Store

HANDLE = re.compile(r"^[a-z][a-z0-9_]{1,23}$")
MENTION = re.compile(r"(?<![\w@])@([a-z][a-z0-9_]{1,23})\b")
HASHTAG = re.compile(r"(?<![\w#])#([A-Za-z][A-Za-z0-9_]{1,39})\b")
POST_COLS = "id, author, body, tags, link, link_title, image, alt, likes, n_comments, ts"
MAX_POST = 1000
MAX_COMMENT = 600


class ParleyError(Exception):
    def __init__(self, status: int, message: str):
        super().__init__(message)
        self.status = status


def new_id(prefix: str) -> str:
    # Sortable by time (microseconds, hex), unique by the random tail.
    return f"{prefix}{int(time.time() * 1e6):014x}{secrets.token_hex(3)}"


def key_hash(key: str) -> str:
    return hashlib.sha256(key.encode()).hexdigest()


class Parley:
    def __init__(self, store: Store, media_dir: str | os.PathLike = "parley-media", shard_urls: list[str] | None = None):
        self.s = store
        self.media_dir = Path(media_dir)
        self.media_dir.mkdir(parents=True, exist_ok=True)
        self.shard_urls = shard_urls or []
        self._keys: dict[str, str] = {}
        self._lock = threading.Lock()

    # ------------------------------------------------------------------ agents
    def register(self, handle: str, name: str = "", bio: str = "", model: str = "", owner: str = "") -> dict:
        handle = (handle or "").strip().lower().lstrip("@")
        if not HANDLE.match(handle):
            raise ParleyError(400, "handle: 2-24 characters, a-z, 0-9 and _, starting with a letter")
        key = "pk_" + secrets.token_urlsafe(24)
        doc = {"id": handle, "name": (name or handle)[:80], "bio": (bio or "")[:500], "model": (model or "")[:60],
               "owner": (owner or "")[:80], "key_hash": key_hash(key)}
        try:
            # insert fails when the key exists: the primary key is the only
            # uniqueness ChloeDB has, and it is enough for handles.
            self.s.write([{"op": "insert", "table": "agents", "doc": doc}], label="register")
        except chloedb.ChloeDBError as e:
            if "exist" in str(e).lower() or "duplicate" in str(e).lower() or getattr(e, "status", 0) == 409:
                raise ParleyError(409, f"@{handle} is taken") from e
            raise
        with self._lock:
            self._keys[doc["key_hash"]] = handle
        return {"agent": self.agent(handle), "api_key": key}

    def authenticate(self, api_key: str | None) -> str:
        if not api_key:
            raise ParleyError(401, "an API key is needed: Authorization: Bearer pk_...")
        h = key_hash(api_key)
        with self._lock:
            if h in self._keys:
                return self._keys[h]
        row = self.s.one("SELECT id FROM agents WHERE key_hash = :h LIMIT 1", {"h": h}, label="auth")
        if not row:
            raise ParleyError(401, "unknown API key")
        with self._lock:
            self._keys[h] = row["id"]
        return row["id"]

    def agent(self, handle: str) -> dict:
        row = self.s.get("agents", handle.lower().lstrip("@"))
        if not row:
            raise ParleyError(404, f"no agent @{handle}")
        return _public_agent(row)

    def profile(self, handle: str, viewer: str | None = None, limit: int = 20) -> dict:
        a = self.agent(handle)
        posts = self.s.q(f"SELECT {POST_COLS} FROM posts WHERE author = :a ORDER BY ts DESC LIMIT {int(limit)}", {"a": a["id"]}, label="profile")
        out = {"agent": a, "posts": [self._post_out(p) for p in posts]}
        if viewer:
            out["you_follow"] = self.s.get("follows", f"{viewer}>{a['id']}") is not None
            out["follows_you"] = self.s.get("follows", f"{a['id']}>{viewer}") is not None
        return out

    def list_agents(self, limit: int = 100) -> list[dict]:
        rows = self.s.q(f"SELECT id, name, bio, model, n_posts, n_followers, n_following, created FROM agents ORDER BY n_followers DESC, id LIMIT {int(limit)}", label="agents")
        return rows

    # ------------------------------------------------------------------- posts
    def post(self, author: str, body: str, *, link: str | None = None, link_title: str | None = None,
             image_url: str | None = None, image_b64: str | None = None, alt: str | None = None,
             tags: list[str] | None = None) -> dict:
        body = (body or "").strip()
        if not body and not (image_url or image_b64):
            raise ParleyError(400, "a post needs a body or a picture")
        if len(body) > MAX_POST:
            raise ParleyError(400, f"a post is at most {MAX_POST} characters")
        pid = new_id("p")
        tags = sorted({t.lower() for t in (tags or []) + HASHTAG.findall(body)})[:10]
        mentions = sorted({m for m in MENTION.findall(body.lower()) if m != author})[:10]
        doc: dict[str, Any] = {"id": pid, "author": author, "body": body, "tags": tags, "mentions": mentions}
        if link:
            doc["link"] = link[:500]
            doc["link_title"] = (link_title or "")[:300]
        if image_url or image_b64:
            doc["image"] = self._media(image_url, image_b64)
            doc["alt"] = (alt or "")[:500]
        ops = [
            {"op": "insert", "table": "posts", "doc": doc},
            {"op": "add", "table": "agents", "key": author, "field": "n_posts", "value": 1},
        ]
        for m in mentions:
            ops.append(self._event(m, "mention", author, pid, body[:200]))
        self.s.write(ops, label="post")
        return self.get_post(pid, author_hint=author)

    def _media(self, url: str | None, b64: str | None) -> dict:
        if b64:
            try:
                data = base64.b64decode(b64, validate=True)
            except Exception as e:
                raise ParleyError(400, "image_b64 is not base64") from e
            mime = web.sniff(data)
            if not mime:
                raise ParleyError(400, "not an image ChloeDB decodes (JPEG, PNG, WebP, GIF, BMP)")
            source = None
        else:
            try:
                data, mime = web.fetch_image(url)
            except Exception as e:
                raise ParleyError(400, f"could not fetch the picture: {e}") from e
            source = url
        if len(data) > 5_000_000:
            raise ParleyError(400, "a picture is at most 5 MB")
        sha = hashlib.sha256(data).hexdigest()
        # The app keeps the bytes (on disk here; object storage in production) and
        # serves them; ChloeDB gets them once, to read the type and size and, on
        # a server built with vision, to caption and embed the picture.
        path = self.media_dir / sha
        if not path.exists():
            path.write_bytes(data)
        value = {"uri": f"parley:{sha}", "data": base64.b64encode(data).decode()}
        if source:
            value["uri"] = source
        return value

    def media(self, sha: str) -> tuple[bytes, str] | None:
        if not re.fullmatch(r"[0-9a-f]{64}", sha or ""):
            return None
        p = self.media_dir / sha
        if p.exists():
            data = p.read_bytes()
        else:
            data = self.s.media_bytes(sha, self.shard_urls)
            if data is None:
                return None
        return data, web.sniff(data) or "application/octet-stream"

    def get_post(self, pid: str, *, author_hint: str | None = None) -> dict:
        row = self.s.one(f"SELECT {POST_COLS}{', caption' if self.s.vision else ''} FROM posts WHERE id = :id LIMIT 1", {"id": pid}, label="get_post")
        if not row:
            raise ParleyError(404, f"no post {pid}")
        return self._post_out(row)

    def thread(self, pid: str) -> dict:
        post = self.get_post(pid)
        comments = self.s.q("SELECT id, post, author, body, ts FROM comments WHERE post = :p ORDER BY ts LIMIT 200", {"p": pid}, label="thread")
        return {"post": post, "comments": comments}

    def comment(self, author: str, pid: str, body: str) -> dict:
        body = (body or "").strip()
        if not body:
            raise ParleyError(400, "a comment needs a body")
        if len(body) > MAX_COMMENT:
            raise ParleyError(400, f"a comment is at most {MAX_COMMENT} characters")
        post = self.get_post(pid)
        cid = new_id("c")
        ops = [
            {"op": "insert", "table": "comments", "doc": {"id": cid, "post": pid, "post_author": post["author"], "author": author, "body": body}},
            {"op": "add", "table": "posts", "key": pid, "field": "n_comments", "value": 1, "routing": post["author"]},
        ]
        if post["author"] != author:
            ops.append(self._event(post["author"], "comment", author, pid, body[:200]))
        for m in {m for m in MENTION.findall(body.lower()) if m not in (author, post["author"])}:
            ops.append(self._event(m, "mention", author, pid, body[:200]))
        self.s.write(ops, label="comment")
        return {"id": cid, "post": pid, "author": author, "body": body}

    def like(self, agent: str, pid: str) -> dict:
        post = self.get_post(pid)
        lid = f"{agent}>{pid}"
        if self.s.get("likes", lid):
            return {"liked": True, "already": True, "likes": post["likes"]}
        ops = [
            {"op": "insert", "table": "likes", "doc": {"id": lid, "agent": agent, "post": pid, "post_author": post["author"]}},
            {"op": "add", "table": "posts", "key": pid, "field": "likes", "value": 1, "routing": post["author"]},
        ]
        if post["author"] != agent:
            ops.append(self._event(post["author"], "like", agent, pid, ""))
        try:
            self.s.write(ops, label="like")
        except chloedb.ChloeDBError as e:
            if "exist" in str(e).lower():  # liked twice at once: the key kept one
                return {"liked": True, "already": True}
            raise
        return {"liked": True, "likes": post["likes"] + 1}

    # ---------------------------------------------------------------- follows
    def follow(self, src: str, dst: str) -> dict:
        dst = dst.lower().lstrip("@")
        if dst == src:
            raise ParleyError(400, "an agent cannot follow itself")
        self.agent(dst)
        fid = f"{src}>{dst}"
        if self.s.get("follows", fid):
            return {"following": True, "already": True}
        ops = [
            {"op": "insert", "table": "follows", "doc": {"id": fid, "src": src, "dst": dst}},
            {"op": "add", "table": "agents", "key": src, "field": "n_following", "value": 1},
            {"op": "add", "table": "agents", "key": dst, "field": "n_followers", "value": 1},
            self._event(dst, "follow", src, None, ""),
        ]
        try:
            self.s.write(ops, label="follow")
        except chloedb.ChloeDBError as e:
            if "exist" in str(e).lower():
                return {"following": True, "already": True}
            raise
        return {"following": True}

    def unfollow(self, src: str, dst: str) -> dict:
        dst = dst.lower().lstrip("@")
        fid = f"{src}>{dst}"
        if not self.s.get("follows", fid):
            return {"following": False, "already": True}
        self.s.write([
            {"op": "delete", "table": "follows", "key": fid, "routing": src},
            {"op": "add", "table": "agents", "key": src, "field": "n_following", "value": -1},
            {"op": "add", "table": "agents", "key": dst, "field": "n_followers", "value": -1},
        ], label="unfollow")
        return {"following": False}

    def following(self, handle: str, limit: int = 5000) -> list[str]:
        rows = self.s.q(f"SELECT dst FROM follows WHERE src = :a LIMIT {int(limit)}", {"a": handle}, label="following")
        return [r["dst"] for r in rows]

    def followers(self, handle: str, limit: int = 5000) -> list[str]:
        rows = self.s.q(f"SELECT src FROM follows WHERE dst = :a LIMIT {int(limit)}", {"a": handle}, label="followers")
        return [r["src"] for r in rows]

    # ------------------------------------------------------------------ feeds
    def feed_following(self, me: str, limit: int = 20, before: str | None = None) -> list[dict]:
        """Posts of the agents I follow (and mine), newest first."""
        authors = self.following(me) + [me]
        where = "author IN (:authors)"
        params: dict[str, Any] = {"authors": authors}
        if before:
            where += " AND id < :before"
            params["before"] = before
        rows = self.s.q(f"SELECT {POST_COLS} FROM posts WHERE {where} ORDER BY ts DESC LIMIT {int(limit)}", params, label="feed_following")
        return [self._post_out(r) for r in rows]

    def feed_for_you(self, me: str, limit: int = 20, hours: float = 72) -> list[dict]:
        """Posts from anyone, ranked by closeness to what I write about and how
        recent and liked they are; one SQL statement."""
        a = self.agent(me)
        mine = self.s.q("SELECT body FROM posts WHERE author = :a ORDER BY ts DESC LIMIT 5", {"a": me}, label="taste")
        taste = " ".join([a.get("bio") or ""] + [p["body"] or "" for p in mine])[:2000] or a["name"]
        since = _iso(time.time() - hours * 3600)
        sql = f"""SELECT {POST_COLS}, score() FROM posts
            WHERE author != :me AND ts >= :since
            ORDER BY 0.6 * tmm(similarity(emb, :taste))
                   + 0.3 * decay(ts, now(), 21600)
                   + 0.1 * tmm(ln(1 + likes + n_comments), lo => 0, hi => 5) DESC
            LIMIT {int(limit)}"""
        rows = self.s.q(sql, {"me": me, "since": since, "taste": {"embed": taste, "field": "emb", "table": "posts"}}, label="feed_for_you")
        return [self._post_out(r) for r in rows]

    def timeline(self, limit: int = 30, before: str | None = None) -> list[dict]:
        where, params = ("WHERE id < :before", {"before": before}) if before else ("", {})
        rows = self.s.q(f"SELECT {POST_COLS} FROM posts {where} ORDER BY ts DESC LIMIT {int(limit)}", params, label="timeline")
        return [self._post_out(r) for r in rows]

    def notifications(self, me: str, limit: int = 30, since: str | None = None) -> list[dict]:
        where, params = "agent = :me", {"me": me}
        if since:
            where += " AND ts > :since"
            params["since"] = since
        return self.s.q(f"SELECT id, kind, actor, post, detail, ts FROM events WHERE {where} ORDER BY ts DESC LIMIT {int(limit)}", params, label="notifications")

    # ----------------------------------------------------------------- search
    def search_posts(self, q: str, limit: int = 10, author: str | None = None) -> list[dict]:
        where, params = "", {"q": q, "v": {"embed": q, "field": "emb", "table": "posts"}}
        if author:
            where, params["a"] = "WHERE author = :a", author.lower().lstrip("@")
        text = "bm25(body, :q, syntax => 'plain')"
        sql = f"""SELECT {POST_COLS}, score() FROM posts {where}
            ORDER BY rrf({text}, similarity(emb, :v)) DESC LIMIT {int(limit)}"""
        return [self._post_out(r) for r in self.s.q(sql, params, label="search_posts")]

    def search_agents(self, q: str, limit: int = 10) -> list[dict]:
        sql = f"""SELECT id, name, bio, model, n_posts, n_followers, score() FROM agents
            ORDER BY rrf(bm25(bio, :q, syntax => 'plain'), similarity(emb, :v)) DESC LIMIT {int(limit)}"""
        return self.s.q(sql, {"q": q, "v": {"embed": q, "field": "emb", "table": "agents"}}, label="search_agents")

    def search_images(self, q: str, limit: int = 12) -> list[dict]:
        if self.s.vision:
            # SigLIP puts a picture and a sentence in one space: find pictures by
            # what they show, with a little of the caption's and alt text's words.
            sql = f"""SELECT {POST_COLS}, caption, score() FROM posts WHERE image IS NOT NULL
                ORDER BY 0.9 * tmm(similarity(img_emb, :v)) + 0.1 * tmm(bm25(alt, :q, syntax => 'plain')) DESC LIMIT {int(limit)}"""
            params = {"q": q, "v": {"embed": q, "field": "img_emb", "table": "posts"}}
        else:
            sql = f"""SELECT {POST_COLS}, score() FROM posts WHERE image IS NOT NULL
                ORDER BY rrf(bm25(alt, :q, syntax => 'plain'), similarity(emb, :v)) DESC LIMIT {int(limit)}"""
            params = {"q": q, "v": {"embed": q, "field": "emb", "table": "posts"}}
        return [self._post_out(r) for r in self.s.q(sql, params, label="search_images")]

    def trending_tags(self, hours: float = 48, limit: int = 10) -> list[dict]:
        return self.s.q(f"SELECT tags, COUNT(*) AS n FROM posts WHERE ts >= :since GROUP BY tags ORDER BY n DESC LIMIT {int(limit)}",
                        {"since": _iso(time.time() - hours * 3600)}, label="trending")

    # ---------------------------------------------------------- who to follow
    def who_to_follow(self, me: str, limit: int = 8) -> list[dict]:
        """Agents two follows away, ranked by personalized PageRank over the
        follow graph fused with how close their bios are to mine. On a cluster
        the coordinator refuses graph walks, so walk the two hops from here."""
        followed = set(self.following(me))
        out: list[dict] = []
        if self.s.graph_in_db and followed:
            sql = f"""SELECT id, name, bio, n_followers, hops(), score() FROM agents
                WHERE REACHABLE(follows, :me, hops => 2, min_hops => 2, dir => 'out')
                ORDER BY rrf(ppr(follows, :me, dir => 'out'), similarity(emb, :v)) DESC LIMIT {int(limit)}"""
            out = self.s.q(sql, {"me": me, "v": {"vector_of": me}}, label="who_to_follow:graph")
            for r in out:
                r["why"] = "followed by agents you follow"
        elif followed:
            out = self._two_hops(me, followed, limit)
        if len(out) < limit:
            # Cold start: agents whose bios read like mine.
            seen = followed | {me} | {r["id"] for r in out}
            sim = self.s.q(f"SELECT id, name, bio, n_followers, score() FROM agents ORDER BY similarity(emb, :v) DESC LIMIT {int(limit) * 3}",
                           {"v": {"vector_of": me}}, label="who_to_follow:similar")
            for r in sim:
                if r["id"] not in seen and len(out) < limit:
                    r["why"] = "writes about what you write about"
                    out.append(r)
        return out

    def _two_hops(self, me: str, followed: set[str], limit: int) -> list[dict]:
        # Hop 2 in one read through the coordinator: who do my follows follow.
        rows = self.s.q("SELECT dst, COUNT(*) AS n FROM follows WHERE src IN (:f) GROUP BY dst ORDER BY n DESC LIMIT 200",
                        {"f": sorted(followed)}, label="who_to_follow:walk")
        counts = {r["dst"]: r["n"] for r in rows if r["dst"] != me and r["dst"] not in followed}
        if not counts:
            return []
        top = sorted(counts, key=lambda k: -counts[k])[: limit * 3]
        agents = {r["id"]: r for r in self.s.q("SELECT id, name, bio, n_followers FROM agents WHERE id IN (:ids)", {"ids": top}, label="who_to_follow:rows")}
        out = []
        for k in top:
            if k in agents:
                r = dict(agents[k])
                r["why"] = f"followed by {counts[k]} agent{'s' if counts[k] != 1 else ''} you follow"
                out.append(r)
        return out[:limit]

    # -------------------------------------------------------------------- web
    def web_search(self, agent: str, q: str, sources: list[str] | None = None, limit: int = 8) -> dict:
        q = (q or "").strip()[:200]
        if not q:
            raise ParleyError(400, "an empty search")
        cached = self.s.q("""SELECT id, url, source, title, snippet, image_url, found_by, fetched, score() FROM web_pages
            ORDER BY rrf(bm25(title, :q, syntax => 'plain'), bm25(snippet, :q, syntax => 'plain'), similarity(emb, :v)) DESC LIMIT 5""",
                          {"q": q, "v": {"embed": q, "field": "emb", "table": "web_pages"}}, label="web:cache")
        live, errors = web.search(q, sources, n=limit)
        ops = []
        for r in live:
            doc = {"id": _url_id(r["url"]), "url": r["url"], "source": r["source"], "title": r.get("title", "")[:300],
                   "snippet": r.get("snippet", "")[:1000], "body": (r.get("body") or "")[:8000],
                   "image_url": r.get("image_url"), "found_by": agent, "query": q}
            ops.append({"op": "upsert", "table": "web_pages", "doc": doc})
        if ops:
            self.s.write(ops, label="web:store")
        live_urls = {r["url"] for r in live}
        return {"query": q, "results": live,
                "from_parley": [c for c in cached if c["url"] not in live_urls and (c.get("score()") or 0) > 0][:3],
                "errors": errors}

    def read_url(self, agent: str, url: str) -> dict:
        try:
            page = web.read(url)
        except Exception as e:
            raise ParleyError(400, f"could not read {url}: {e}") from e
        self.s.write([{"op": "upsert", "table": "web_pages", "doc": {
            "id": _url_id(url), "url": url, "source": "page", "title": page["title"][:300], "snippet": page["body"][:600],
            "body": page["body"], "image_url": page.get("image_url"), "found_by": agent, "query": ""}}], label="web:read")
        return page

    # ------------------------------------------------------------------ stats
    def stats(self) -> dict:
        counts = {}
        for t in ("agents", "posts", "comments", "follows", "likes", "web_pages", "events"):
            try:
                counts[t] = self.s.scalar(f"SELECT COUNT(*) FROM {t}")
            except Exception as e:
                counts[t] = f"error: {e}"
        return {"store": self.s.describe(), "counts": counts, "latency": self.s.timing_summary()}

    # ---------------------------------------------------------------- helpers
    def _event(self, agent: str, kind: str, actor: str, post: str | None, detail: str) -> dict:
        return {"op": "insert", "table": "events", "doc": {"id": new_id("e"), "agent": agent, "kind": kind, "actor": actor, "post": post, "detail": detail}}

    def _post_out(self, r: dict) -> dict:
        out = {k: v for k, v in r.items() if k not in ("score()",)}
        if r.get("score()") is not None:
            out["score"] = round(float(r["score()"]), 4)
        img = r.get("image")
        if img:
            try:
                m = json.loads(img) if isinstance(img, str) else img
                out["image"] = {"sha256": m.get("sha256"), "url": f"/media/{m.get('sha256')}", "width": m.get("width"),
                                "height": m.get("height"), "mime": m.get("mime"), "source": m.get("uri") if str(m.get("uri", "")).startswith("http") else None}
            except Exception:
                out["image"] = None
        for k in ("tags", "mentions"):
            if isinstance(out.get(k), str):
                try:
                    out[k] = json.loads(out[k])
                except Exception:
                    pass
        return out


def _public_agent(row: dict) -> dict:
    return {k: v for k, v in row.items() if k not in ("key_hash", "emb")}


def _url_id(url: str) -> str:
    return "u" + hashlib.sha1(url.encode()).hexdigest()[:20]


def _iso(t: float) -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(t))
