import json
import urllib.request

import pytest

import chloedb

PLACES = {
    "table": "places",
    "key": "id",
    "fields": [
        {"name": "id", "type": "int"},
        {"name": "body", "type": "text"},
        {"name": "lat", "type": "float"},
        {"name": "lon", "type": "float"},
    ],
}
WORDS = ["pizza", "coffee", "bagel", "pharmacy", "park", "deli", "sushi", "bar"]


def place(i):
    return {
        "id": i,
        "body": f"{WORDS[i % 8]} {WORDS[(i * 7 + 3) % 8]}",
        "lat": 40.7 + ((i * 37) % 200) * 0.0003 - 0.03,
        "lon": -74.0 + ((i * 53) % 200) * 0.0004 - 0.04,
    }


def call(url, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(url + path, data=data, headers={"content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.loads(r.read())


@pytest.fixture
def cloud(tmp_path, serve):
    path = tmp_path / "places.strata"
    with chloedb.Database.create(path, PLACES) as db:
        db.insert([place(i) for i in range(2000)])
    url = serve(path, "--slices", str(tmp_path / "slices"))
    built = call(url, "/v1/admin/slices", {"text": "body", "lat": "lat", "lon": "lon", "lat0": 40.7, "lon0": -74.0})
    assert built["rows"] == 2000
    return url


def test_nearby_answers_on_the_device_what_the_server_would(cloud, tmp_path):
    with chloedb.nearby(cloud, "places", tmp_path / "device", reach=1, report_every=0, device="test-phone") as near:
        moved = near.move_to(40.7, -74.0)
        assert moved["fetched"] == 9 and moved["bytes"] > 0
        assert sum(n for _, _, n in near.held()) > 0
        on_device = 0
        for q in ["pizza", "coffee bar", "sushi deli"]:
            r = near.search(q, k=5, d0=300)
            assert len(r) == 5 and r.why in ("proved", "short", "bound")
            if r.where == "device":
                on_device += 1
                assert r.on_device and r.proved and r.server_ms is None
                # The device's answer is the server's, row for row.
                cloud_r = call(cloud, "/v1/query", json.loads(r.ask))
                assert r.keys == cloud_r["keys"] and r.scores == cloud_r["scores"]
        assert on_device >= 2
        r = near.search("nothing")
        assert (r.where, r.why, len(r)) == ("device", "no_match", 0)
        # More places than the tiles held: the server answers.
        r = near.search("pizza", k=1000)
        assert (r.where, r.why) == ("server", "short") and len(r) == 500 and r.server_ms is not None
        assert {"id", "body"} <= set(r[0])
        near.report()
    usage = call(cloud, "/v1/admin/slices")["slices"][0]["usage"]
    assert (usage["searches"], usage["on_device"], usage["devices"]) == (5, on_device + 1, 1)


def test_nearby_asks_the_server_while_the_table_has_changed(cloud, tmp_path):
    near = chloedb.nearby(cloud, "places", tmp_path / "device", reach=1, report_every=0)
    near.move_to(40.7, -74.0)
    assert near.check()["fresh"] and near.search("nothing").where == "device"
    version = near.info["version"]
    chloedb.connect(cloud).delete([7])
    assert near.check()["fresh"] is False
    r = near.search("nothing")
    assert (r.where, r.why) == ("server", "stale")
    # A rebuild: the device takes the new slice, and its tiles again.
    call(cloud, "/v1/admin/slices", {"table": "places"})
    assert near.check()["fresh"] and near.info["version"] > version
    assert len(near.held()) == 9
    r = near.search("nothing")
    assert (r.where, r.why) == ("device", "no_match")
    near.close()
    with pytest.raises(chloedb.StrataError, match="closed"):
        near.held()


TRACKS = {
    "table": "tracks",
    "key": "id",
    "fields": [
        {"name": "id", "type": "int"},
        {"name": "emb", "type": "vector", "dim": 8, "index": "flat"},
    ],
}


def track(i):
    """Unit vectors around 12 topics, each a few dimensions."""
    t = i % 12
    v = [0.05 * (((i * 31 + j * 17) % 11) - 5) / 5 for j in range(8)]
    v[t % 8] += 1.0
    v[(t * 3 + 1) % 8] += 0.5 * (t // 8 + 1)
    n = sum(x * x for x in v) ** 0.5
    return {"id": i, "emb": [x / n for x in v]}


def test_a_vector_slice_answers_on_the_device_what_the_server_would(tmp_path, serve):
    path = tmp_path / "tracks.strata"
    with chloedb.Database.create(path, TRACKS) as db:
        db.insert([track(i) for i in range(1200)])
    url = serve(path, "--slices", str(tmp_path / "slices"))
    built = call(url, "/v1/admin/slices", {"table": "tracks", "vector": "emb", "clusters": 12})
    assert (built["kind"], built["rows"], built["clusters"]) == ("vector", 1200, 12) and built["codes_bytes"] > 0
    with chloedb.nearby(url, "tracks", tmp_path / "device", report_every=0) as near:
        assert near.info["kind"] == "vector" and near.info["dim"] == 8
        taste = track(5)["emb"]
        got = near.focus(taste, rows=300)
        held = sum(n for _, _, n in near.held())
        assert got["fetched"] >= 1 and 0 < held <= 450
        r = near.similar(taste, k=5)
        assert (r.where, r.why) == ("device", "proved") and r.edge_m is None
        cloud_r = call(url, "/v1/query", json.loads(r.ask))
        assert r.keys == cloud_r["keys"] and r.scores == cloud_r["scores"]
        # More rows than it holds: the server answers, exactly.
        r = near.similar(taste, k=1000)
        assert (r.where, r.why) == ("server", "short") and len(r) == 1000
        # Away from the taste: the codes name the clusters to fetch, and
        # the device proves its answer once it holds them.
        far = track(7)["emb"]
        r = near.similar(far, k=5, refine=0)
        assert (r.where, r.why, r.fetched) == ("server", "bound", 0)
        r = near.similar(far, k=5)
        assert (r.where, r.why) == ("device", "proved") and r.fetched >= 1
        cloud_r = call(url, "/v1/query", json.loads(r.ask))
        assert r.keys == cloud_r["keys"] and r.scores == cloud_r["scores"]
        assert near.stats["fetched_bytes"] >= built["codes_bytes"]
        # A geo search of a vector slice is refused.
        with pytest.raises(chloedb.StrataError, match="search it with a vector"):
            near.search("pizza", lat=40.7, lon=-74.0)
    usage = call(url, "/v1/admin/slices")["slices"][0]["usage"]
    assert usage["codes"] == 1 and usage["codes_bytes"] == built["codes_bytes"]
