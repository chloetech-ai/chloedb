"""Backends for the tests: a ChloeDB file in the process, one chloedb-server,
and a coordinator over two shards. A backend that cannot start is skipped."""
from __future__ import annotations

import os
import shutil
import socket
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parent.parent
REPO = HERE.parent.parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(REPO / "chloedb-py" / "python"))

SERVER = os.environ.get("CHLOEDB_SERVER") or str(REPO / "chloedb-server" / "target" / "release" / "chloedb-server")


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def wait_up(url: str, seconds: float = 60) -> None:
    end = time.time() + seconds
    while time.time() < end:
        try:
            urllib.request.urlopen(url + "/v1/health", timeout=2).read()
            return
        except Exception:
            time.sleep(0.2)
    raise RuntimeError(f"{url} did not come up")


def start_server(tmp: Path, name: str, *extra: str) -> tuple[subprocess.Popen, str, str]:
    http, grpc = free_port(), free_port()
    args = [SERVER, "--listen", f"127.0.0.1:{http}", "--grpc", f"127.0.0.1:{grpc}", "--no-token", *extra]
    log = open(tmp / f"{name}.log", "w")
    env = {**os.environ}
    if os.environ.get("PARLEY_TEST_MODELS"):
        env["CHLOEDB_MODELS"] = os.environ["PARLEY_TEST_MODELS"]
    p = subprocess.Popen(args, stdout=log, stderr=subprocess.STDOUT, env=env)
    url = f"http://127.0.0.1:{http}"
    wait_up(url)
    return p, url, f"http://127.0.0.1:{grpc}"


def have_server() -> bool:
    return Path(SERVER).exists()


def have_native() -> bool:
    try:
        import chloedb
        chloedb.Database  # noqa: B018
        from chloedb import _native  # noqa: F401
        return hasattr(chloedb, "open") and _native_ok()
    except Exception:
        return False


def _native_ok() -> bool:
    import chloedb
    try:
        import tempfile
        with tempfile.TemporaryDirectory() as d:
            db = chloedb.open(os.path.join(d, "x.strata"), schema={"table": "t", "key": "k", "fields": [{"name": "k", "type": "keyword"}]})
            db.close()
        return True
    except ImportError:
        return False


@pytest.fixture(params=["embedded", "server", "cluster"])
def backend(request, tmp_path):
    """(target, shard_urls) for a Store; servers stop after the test."""
    kind = request.param
    procs: list[subprocess.Popen] = []
    try:
        if kind == "embedded":
            if not have_native():
                pytest.skip("chloedb's extension is not built (maturin develop in chloedb-py)")
            yield kind, str(tmp_path / "parley.strata"), []
        elif kind == "server":
            if not have_server():
                pytest.skip(f"no chloedb-server at {SERVER}")
            p, url, _ = start_server(tmp_path, "server", "--db", str(tmp_path / "s.strata"), "--seq-no", "--media-store", str(tmp_path / "media"))
            procs.append(p)
            yield kind, url, [url]
        else:
            if not have_server():
                pytest.skip(f"no chloedb-server at {SERVER}")
            shards = []
            for i in range(2):
                p, url, grpc = start_server(tmp_path, f"shard{i}", "--db", str(tmp_path / f"shard{i}.strata"), "--seq-no", "--media-store", str(tmp_path / f"media{i}"))
                procs.append(p)
                shards.append((url, grpc))
            p, url, _ = start_server(tmp_path, "coord", "--shards", ",".join(g for _, g in shards))
            procs.append(p)
            yield kind, url, [u for u, _ in shards]
    finally:
        for p in procs:
            p.terminate()
        for p in procs:
            try:
                p.wait(10)
            except subprocess.TimeoutExpired:
                p.kill()
