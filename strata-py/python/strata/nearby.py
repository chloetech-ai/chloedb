"""Search near a device's user on the device, with a strata-server as the
fallback: `strata.nearby(url, table, dir)`.

The server builds a table's slice (`POST /v1/admin/slices`, or the command
center's Slices view): a manifest and the rows of each map tile, or of each
vector cluster. `Nearby` fetches the manifest, then the tiles around its
user as the user moves (`move_to`), or the clusters nearest the user's
taste (`focus`), into a local database (`dir`). A search runs there and
scores every row held exactly as the server would. When the k-th local
score beats the most a row not held could score, the local answer is the
server's and no request is made; otherwise the same query goes to the
server (`POST /v1/query`). Each answer says which, and why.

Before it trusts its slice it asks the server whether the slice is the
current one (`GET /v1/slices/<table>/status`, every `status_every`
seconds): it fetches a newer one, and while the table has changed since
the build it asks the server for every search (unless the slice is under
`max_lag` seconds old). It reports its counts to the server every
`report_every` searches (`POST /v1/slices/<table>/usage`), which the
command center shows.

    near = strata.nearby("http://127.0.0.1:7700", "places", "./slice")
    near.move_to(40.7484, -73.9857)
    r = near.search("pizza", k=10)
    r.where, r.why          # "device" or "server", and why
    for row in r: print(row["name"])

    tracks = strata.nearby("http://127.0.0.1:7700", "tracks", "./tracks")
    tracks.focus(taste, rows=20_000)    # the user's taste, a vector
    r = tracks.similar(taste, k=10)     # or any vector near it

A vector slice's device also fetches every row's codes (`codes.bin.gz`,
one byte a dimension), which bound the rows it lacks tightly; when they
leave its answer in doubt it fetches the few clusters they name, up to
`refine` bytes, and proves it on the device.

It needs the compiled extension (the device's database); the server is
reached with the standard library.
"""

from __future__ import annotations

import json
import os
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from typing import Any

from ._native import StrataError, _strata
from .result import Result

REASONS = ("proved", "no_match", "short", "bound", "stale")


class NearbyResult(Result):
    """A search's answer, the device's or the server's, and where it was
    answered: `where` is "device" or "server"; `why` is "proved" or
    "no_match" (answered on the device), or "short" (the device held fewer
    than k matching rows), "bound" (a row beyond the tiles or clusters held
    could have scored higher) or "stale" (the table changed since the slice
    was built). `bound` is the most a row not held could score, `edge_m`
    the distance to the nearest tile not held (None for a vector slice),
    `ask` the body of the request (`POST /v1/query`) that asks the server
    for it, `fetched` the clusters a vector search fetched to prove its
    answer."""

    __slots__ = ("where", "why", "ask", "proved", "stale", "bound", "edge_m", "device_ms", "server_ms", "fetched")

    @property
    def on_device(self) -> bool:
        return self.where == "device"

    def __repr__(self) -> str:
        return f"<NearbyResult {len(self.rows)} rows, {self.where} ({self.why})>"


class Nearby:
    """A device's slice of a table on a strata-server; see the module."""

    def __init__(self, url: str, table: str, dir: str | os.PathLike, *, token: str | None = None, reach: int = 2, timeout: float = 30.0,
                 status_every: float = 30.0, max_lag: float = 0.0, report_every: int = 20, device: str | None = None, codes: bool = True):
        self.url = url.rstrip("/")
        self.table = table
        self.dir = os.fspath(dir)
        self.token = token if token is not None else os.environ.get("STRATA_TOKEN")
        self.reach = reach
        self.timeout = timeout
        self.status_every = status_every
        self.max_lag = max_lag
        self.report_every = report_every
        self.codes = codes
        self.device = device or f"py-{uuid.uuid4().hex[:12]}"
        self.position: tuple[float, float] | None = None
        self.taste: tuple[list[float], int] | None = None
        self.stats = {"searches": 0, "on_device": 0, "fetched_tiles": 0, "fetched_bytes": 0}
        self._pending = self._empty()
        self._status_at = 0.0
        self._dev = None
        self._load()

    # ---------------------------------------------------------------- HTTP

    def _request(self, method: str, path: str, body: Any = None, raw: str | None = None) -> bytes:
        data = raw.encode() if raw is not None else (json.dumps(body).encode() if body is not None else None)
        headers = {"content-type": "application/json"} if data is not None else {}
        if self.token:
            headers["authorization"] = "Bearer " + self.token
        req = urllib.request.Request(self.url + path, data=data, method=method, headers=headers)
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as r:
                return r.read()
        except urllib.error.HTTPError as e:
            text = e.read().decode(errors="replace")
            try:
                text = json.loads(text).get("error", text)
            except ValueError:
                pass
            raise StrataError(f"{method} {path}: {e.code}: {text}") from None

    @property
    def _d(self) -> Any:
        if self._dev is None:
            raise StrataError("the slice is closed")
        return self._dev

    def _slice(self, rel: str) -> bytes:
        return self._request("GET", f"/v1/slices/{urllib.parse.quote(self.table)}/{rel}")

    # ---------------------------------------------------------------- the slice

    def _load(self) -> None:
        text = self._slice("manifest.json").decode()
        self.stats["fetched_bytes"] += len(text)
        if self._dev is not None:
            self._dev.close()
        self._dev = _strata.SliceDevice.create(self.dir, text)
        self.info = json.loads(self._dev.info())
        if self.codes and self.info.get("codes_bytes"):
            data = self._slice("codes.bin.gz")
            self._dev.add_codes(data)
            self.stats["fetched_bytes"] += len(data)
        self._status_at = time.monotonic()
        if self.position is not None:
            self.move_to(*self.position)
        if self.taste is not None:
            self.focus(*self.taste)

    def check(self) -> dict:
        """Asks the server whether the slice is the current one: fetches a
        newer one (and the tiles around the user again), and marks the
        device stale while the table has changed since the build. Returns
        the server's status."""
        st = json.loads(self._slice("status"))
        self._status_at = time.monotonic()
        if st.get("version") != self.info["version"]:
            self._load()
            st = json.loads(self._slice("status"))
        lag = time.time() - self.info.get("built_ms", 0) / 1000
        self._d.set_stale(not st.get("fresh", False) and lag > self.max_lag)
        return st

    def move_to(self, lat: float, lon: float, reach: int | None = None) -> dict:
        """Moves the user: fetches the tiles of the square around it the
        device lacks (nearest first) and drops those far behind. Returns
        what it did: {fetched, dropped, bytes}."""
        if reach is not None:
            self.reach = reach
        self.position = (lat, lon)
        return self._apply(*self._d.plan(lat, lon, self.reach))

    def focus(self, taste: Any, *, rows: int = 20_000) -> dict:
        """For a vector slice: holds the clusters whose rows could score
        highest for `taste` (a vector, such as the user's history summed),
        as many as fit in `rows` rows, fetching those it lacks (best first)
        and dropping those that fell well behind. Returns what it did:
        {fetched, dropped, bytes}."""
        taste = [float(x) for x in taste]
        self.taste = (taste, rows)
        return self._apply(*self._d.plan_vector(taste, rows))

    def _apply(self, fetch: list, drop: list) -> dict:
        for x, y in drop:
            self._d.drop_tile(x, y)
        got = 0
        for x, y, path, _ in fetch:
            data = self._slice(path)
            self._d.add_tile(x, y, data)
            got += len(data)
        self.stats["fetched_tiles"] += len(fetch)
        self.stats["fetched_bytes"] += got
        return {"fetched": len(fetch), "dropped": len(drop), "bytes": got}

    def held(self) -> list[tuple[int, int, int]]:
        """The tiles held, as (x, y, rows) (a vector slice's cluster x is
        its number, y 0)."""
        return self._d.held()

    # ---------------------------------------------------------------- searching

    def search(self, q: str, *, k: int = 10, d0: float = 1000.0, lat: float | None = None, lon: float | None = None) -> NearbyResult:
        """The `k` best places for `q` near the user (or near `lat`,
        `lon`), ranked by BM25 faded by distance (a place `d0` metres away
        counts half): the device's answer when it can prove it is the
        server's, else the server's."""
        if lat is None or lon is None:
            if self.position is None:
                raise StrataError("move_to(lat, lon) first, or pass lat and lon")
            lat, lon = self.position
        return self._answer(lambda: self._d.search(q, lat, lon, k, float(d0)))

    def similar(self, vector: Any, *, k: int = 10, refine: int = 2_000_000) -> NearbyResult:
        """For a vector slice: the `k` rows most similar to `vector` (by
        dot product, as `similarity` ranks them): the device's answer when
        it can prove it is the server's, else the server's, which re-scores
        every row (an exact search). When the device's codes name clusters
        that could still beat its k-th, it fetches them if they come to at
        most `refine` bytes (0: never), and then proves its answer."""
        vector = [float(x) for x in vector]
        return self._answer(lambda: self._d.search_vector(vector, k), refine)

    def _answer(self, local: Any, refine: int = 0) -> NearbyResult:
        if self.status_every and time.monotonic() - self._status_at >= self.status_every:
            self.check()
        t0 = time.perf_counter()
        a = json.loads(local())
        device_ms = (time.perf_counter() - t0) * 1e3
        need, fetched = a.get("need") or [], 0
        if not a["on_device"] and a["why"] == "bound" and need and sum(b for _, _, b in need) <= refine:
            self._apply([(x, y, f"tiles/{x}_{y}.jsonl.gz", b) for x, y, b in need], [])
            fetched = len(need)
            t0 = time.perf_counter()
            a = json.loads(local())
            device_ms += (time.perf_counter() - t0) * 1e3
        server_ms = None
        if a["on_device"]:
            r = NearbyResult(a["columns"], a["rows"], a["keys"], a["scores"])
            r.where = "device"
        else:
            t0 = time.perf_counter()
            d = json.loads(self._request("POST", "/v1/query", raw=a["ask"]))
            server_ms = (time.perf_counter() - t0) * 1e3
            r = NearbyResult._of(d)
            r.where = "server"
        r.why, r.ask, r.proved, r.stale, r.bound, r.edge_m = a["why"], a["ask"], a["proved"], a["stale"], a["bound"], a["edge_m"]
        r.device_ms, r.server_ms, r.fetched = device_ms, server_ms, fetched
        self._note(r)
        return r

    # ---------------------------------------------------------------- reporting

    @staticmethod
    def _empty() -> dict:
        return {"searches": 0, "why": {w: 0 for w in REASONS}, "device_ms": 0.0, "cloud_ms": 0.0}

    def _note(self, r: NearbyResult) -> None:
        self.stats["searches"] += 1
        self.stats["on_device"] += r.where == "device"
        p = self._pending
        p["searches"] += 1
        p["why"][r.why] += 1
        p["device_ms"] += r.device_ms
        p["cloud_ms"] += r.server_ms or 0.0
        if self.report_every and p["searches"] >= self.report_every:
            self.report()

    def report(self) -> None:
        """Sends the counts since the last report to the server."""
        p, self._pending = self._pending, self._empty()
        if not p["searches"]:
            return
        try:
            self._request("POST", f"/v1/slices/{urllib.parse.quote(self.table)}/usage", {"device": self.device, **p})
        except (StrataError, OSError):
            pass  # counts are best effort: a search never fails for them

    def close(self) -> None:
        """Reports what is left and closes the device's database."""
        self.report()
        if self._dev is not None:
            self._dev.close()
            self._dev = None

    def __enter__(self) -> "Nearby":
        return self

    def __exit__(self, *exc: Any) -> None:
        self.close()


def nearby(url: str, table: str, dir: str | os.PathLike, **options: Any) -> Nearby:
    """A device's slice of `table` on the strata-server at `url`, kept in
    directory `dir`; see `Nearby` for the options."""
    return Nearby(url, table, dir, **options)
