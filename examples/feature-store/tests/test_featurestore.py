"""The feature store demo end to end, against a chloedb-server built from this
checkout (skipped when there is none): history made, a point-in-time training
set with nothing from the future, a leaky one that looks better than it is,
a model deployed and serving, a click in the features at once.

    cd examples/feature-store && python3 -m pytest tests
"""
import json
import os
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(HERE))
import featurestore as fs  # noqa: E402


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def server_bin():
    if os.environ.get("CHLOEDB_SERVER_BIN"):
        return os.environ["CHLOEDB_SERVER_BIN"]
    for prof in ("release", "ci", "debug"):
        p = HERE.parent.parent / "chloedb-server" / "target" / prof / "chloedb-server"
        if p.exists():
            return str(p)
    return None


def test_auc_and_transforms():
    assert fs.auc([0.1, 0.4, 0.35, 0.8], [0, 0, 1, 1]) == 0.75
    assert fs.auc([0.5, 0.5], [0, 1]) == 0.5
    raw = {k: None for k in fs.RAW_COLS}
    raw["price"] = 10.0
    x = fs.transform(raw)  # a new shopper and product: nothing missing breaks it
    assert len(x) == len(fs.FEATURES) and x[fs.FEATURES.index("price_ratio")] == 0.0
    raw.update(u_clicks=2, u_click_price=2.0)  # clicked $1 on average: clipped
    assert fs.transform(raw)[fs.FEATURES.index("price_ratio")] == 2.0


@pytest.fixture(scope="module")
def demo(tmp_path_factory):
    b = server_bin()
    if not b:
        pytest.skip("no chloedb-server built (cd chloedb-server && cargo build --release)")
    d = tmp_path_factory.mktemp("fs")
    port, app = free_port(), free_port()
    p = subprocess.Popen([sys.executable, str(HERE / "featurestore.py"), "serve", "--local", str(d), "--port", str(port),
                          "--listen", f"127.0.0.1:{app}", "--server-bin", b, "--backfill", "4", "--rate", "20"],
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    url = f"http://127.0.0.1:{port}/v1/apps/features"
    end = time.time() + 120
    while time.time() < end:
        line = p.stdout.readline()
        if line.startswith("ready"):
            break
        if p.poll() is not None:
            raise RuntimeError("the demo stopped: " + line)
    else:
        raise RuntimeError("the demo did not start")

    def call(path, body=None):
        req = urllib.request.Request(url + path, None if body is None else json.dumps(body).encode(), {"content-type": "application/json"})
        with urllib.request.urlopen(req, timeout=300) as r:
            return json.loads(r.read())

    time.sleep(70)  # a minute of live traffic to hold out
    yield call
    p.terminate()
    p.wait(10)


def test_the_app_is_listed_and_serves_its_view(demo):
    m = demo("/app.json")
    assert m["name"] == "features" and m["view"] == "ui/view.js"


def test_point_in_time_and_leakage(demo):
    asof = demo("/api/train", {})
    leaky = demo("/api/train", {"leaky": True})
    assert asof["train_rows"] > 1000
    # The leaky model reports more than it is worth; the ASOF one does not.
    assert leaky["auc_holdout"] > leaky["auc_true"] + 0.03
    assert abs(asof["auc_holdout"] - asof["auc_true"]) < 1e-9
    assert asof["auc_true"] > 0.6
    # No feature row from after its impression.
    tl = demo("/api/timeline?user=u005")
    for r in tl["impressions"]:
        assert r["u_ts"] is None or r["u_ts"] < r["ts"]
    models = demo("/api/models")["models"]
    assert {m["id"] for m in models} >= {asof["id"], leaky["id"]}

    demo("/api/deploy", {"id": asof["id"]})
    r = demo("/api/shop?user=u010")
    assert r["model"] == asof["id"] and len(r["items"]) == fs.K
    scores = [e["score"] for e in r["items"]]
    assert scores == sorted(scores, reverse=True)

    # A click: in the shopper's features as soon as the write returns.
    c = demo("/api/click", {"user": "u010", "shown": [e["id"] for e in r["items"]], "item": r["items"][0]["id"], "model": r["model"]})
    assert c["after"]["clicks"] == (c["before"] or {}).get("clicks", 0) + 1
    assert c["after"]["views"] == (c["before"] or {}).get("views", 0) + fs.K


def test_features_and_browse(demo):
    f = demo("/api/features")
    assert [x["name"] for x in f["features"]] == fs.FEATURES
    assert all(sum(x["hist"]) == f["sample"] for x in f["features"])
    b = demo("/api/browse", {"table": "fs_user_online", "where": "clicks >= 1", "order": "clicks DESC", "limit": 5})
    assert b["rows"] and b["rows"][0]["clicks"] >= b["rows"][-1]["clicks"]
    with pytest.raises(urllib.error.HTTPError):
        demo("/api/browse", {"table": "_strata_keys", "where": "", "order": ""})
