"""Reads that see a write (Written, min_seq_no(s), Session), the error
codes and classes, and the client's options refused by a Database."""

import pickle
import subprocess
import time
import urllib.request

import pytest

import chloedb
from chloedb import Session, Written

from conftest import free_port, server_binary

TABLE = "CREATE TABLE t (id KEYWORD PRIMARY KEY, n INT, title TEXT)"


def _wait(url, p):
    for _ in range(100):
        try:
            urllib.request.urlopen(url + "/v1/health", timeout=1)
            return
        except urllib.error.HTTPError:
            return
        except OSError:
            if p.poll() is not None:
                raise RuntimeError(p.stderr.read().decode())
            time.sleep(0.1)
    raise RuntimeError("chloedb-server did not start")


@pytest.fixture(scope="module")
def servers(tmp_path_factory):
    """Starts servers by their arguments ("{dir}" is a fresh directory),
    returns their URLs, and stops them all at the end."""
    binary = server_binary()
    if not binary:
        pytest.skip("chloedb-server is not built")
    procs = []

    def start(*args):
        p = subprocess.Popen([binary, *args], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        procs.append(p)
        return p

    yield start, tmp_path_factory
    for p in procs:
        p.terminate()
    for p in procs:
        p.wait(timeout=10)


@pytest.fixture(scope="module")
def one(servers):
    """A server numbering its writes, with table t."""
    start, tmp = servers
    port = free_port()
    url = f"http://127.0.0.1:{port}"
    _wait(url, start("--db", str(tmp.mktemp("one") / "db.strata"), "--listen", f"127.0.0.1:{port}", "--grpc", "off", "--seq-no"))
    chloedb.connect(url).sql(TABLE)
    return url


@pytest.fixture(scope="module")
def plain(servers):
    """A server that does not number its writes."""
    start, tmp = servers
    port = free_port()
    url = f"http://127.0.0.1:{port}"
    _wait(url, start("--db", str(tmp.mktemp("plain") / "db.strata"), "--listen", f"127.0.0.1:{port}", "--grpc", "off"))
    chloedb.connect(url).sql(TABLE)
    return url


@pytest.fixture(scope="module")
def coordinator(servers):
    """A coordinator over two shards that number their writes."""
    start, tmp = servers
    grpcs = []
    for i in range(2):
        http, grpc = free_port(), free_port()
        _wait(f"http://127.0.0.1:{http}", start("--db", str(tmp.mktemp(f"shard{i}") / "db.strata"), "--listen", f"127.0.0.1:{http}", "--grpc", f"127.0.0.1:{grpc}", "--seq-no"))
        grpcs.append(f"http://127.0.0.1:{grpc}")
    port = free_port()
    url = f"http://127.0.0.1:{port}"
    _wait(url, start("--shards", ",".join(grpcs), "--listen", f"127.0.0.1:{port}", "--grpc", "off"))
    for _ in range(50):
        try:
            chloedb.connect(url).sql(TABLE)
            break
        except chloedb.ChloeDBError:
            time.sleep(0.2)  # the shards not yet all known
    return url


# ------------------------------------------------------------- Written


def test_written_is_an_int():
    w = Written(3, 7)
    assert w == 3 and w + 1 == 4 and int(w) == 3 and f"{w}" == "3" and repr(w) == "3"
    assert w.seq_no == 7 and w.seq_nos is None
    import json

    assert json.dumps({"n": w}) == '{"n": 3}'
    back = pickle.loads(pickle.dumps(w))
    assert back == 3 and back.seq_no == 7
    both = Written(1, seq_nos=[4, 0])._then(Written(2, seq_nos=[3, 5, 1]))
    assert both == 3 and both.seq_nos == [4, 5, 1]


def test_writes_answer_their_numbers(one):
    c = chloedb.connect(one)
    w = c.insert([{"id": "w1", "n": 1}, {"id": "w2", "n": 2}])
    assert w == 2 and isinstance(w, int) and w.seq_no >= 1 and w.seq_nos is None
    u = c.sql("UPDATE t SET n = 3 WHERE id = 'w1'")
    assert u == 1 and u.seq_no > w.seq_no
    p = c.put("w3", {"n": 1})
    assert p == 1 and p.seq_no > u.seq_no
    d = c.delete("w3")
    assert d == 1 and d.seq_no > p.seq_no
    total = 0
    total += c.upsert({"id": "w4", "n": 4})
    assert total == 1 and type(total) is int


def test_reads_take_min_seq_no(one):
    c = chloedb.connect(one)
    w = c.upsert({"id": "r1", "n": 1, "title": "river"})
    assert c.query("SELECT n FROM t WHERE id = 'r1'", min_seq_no=w.seq_no).rows == [[1]]
    assert c.get("r1", min_seq_no=w.seq_no)["n"] == 1
    assert c.get_many(["r1"], min_seq_no=w.seq_no)[0]["n"] == 1
    assert c.search("river", min_seq_no=w.seq_no, text_fields=["title"]).keys == ["r1"]
    assert [p.rows for p in c.scroll("SELECT n FROM t WHERE id = 'r1'", min_seq_no=w.seq_no)] == [[[1]]]
    assert c.sql("SELECT n FROM t WHERE id = 'r1'", min_seq_no=w.seq_no).rows == [[1]]
    # A number past the server's last is refused: the reads send it.
    far = w.seq_no + 1000
    reads = [
        lambda: c.query("SELECT n FROM t", min_seq_no=far),
        lambda: c.get("r1", min_seq_no=far),
        lambda: c.get_many(["r1"], min_seq_no=far),
        lambda: c.search("river", min_seq_no=far, text_fields=["title"]),
        lambda: list(c.scroll("SELECT n FROM t", min_seq_no=far)),
        lambda: c.sql("SELECT n FROM t", min_seq_no=far),
    ]
    for read in reads:
        with pytest.raises(chloedb.ConflictError) as e:
            read()
        assert e.value.code == "conflict" and e.value.status == 409


def test_session_reads_its_writes(one):
    c = chloedb.connect(one)
    s = c.session()
    assert isinstance(s, Session) and s.url == c.url and s.token == c.token
    assert s.seq_no is None
    w = s.upsert({"id": "s1", "n": 1, "title": "garden"})
    assert s.seq_no == w.seq_no
    assert s.get("s1")["n"] == 1
    assert s.query("SELECT n FROM t WHERE id = 's1'").rows == [[1]]
    # A write by SQL is not sent the session's numbers (a server refuses
    # them on a write), and its own number is kept.
    u = s.sql("UPDATE t SET n = 2 WHERE id = 's1'")
    assert s.seq_no == u.seq_no
    assert s.sql("SELECT n FROM t WHERE id = 's1'").rows == [[2]]
    # The session sends its numbers on every read: past the server's last,
    # each is refused.
    s.observe({"seq_no": u.seq_no + 1000})
    for read in (
        lambda: s.query("SELECT n FROM t"),
        lambda: s.get("s1"),
        lambda: s.get_many(["s1"]),
        lambda: s.search("garden", text_fields=["title"]),
        lambda: list(s.scroll("SELECT n FROM t")),
        lambda: s.sql("SELECT n FROM t"),
    ):
        with pytest.raises(chloedb.ConflictError):
            read()
    # A plain client sends nothing of the session's.
    assert c.get("s1")["n"] == 2


def test_session_without_numbers(plain):
    s = chloedb.connect(plain).session()
    w = s.upsert({"id": "p1", "n": 1})
    assert w == 1 and w.seq_no is None and s.seq_no is None
    assert s.get("p1")["n"] == 1


def test_session_through_a_coordinator(coordinator):
    s = chloedb.connect(coordinator).session()
    w = s.upsert([{"id": f"c{i}", "n": i} for i in range(20)])
    assert w == 20 and w.seq_no is None and len(w.seq_nos) == 2 and any(w.seq_nos)
    assert s.seq_nos == w.seq_nos
    assert s.query("SELECT COUNT(*) FROM t").rows == [[20]]
    assert s.get("c3")["n"] == 3
    assert [r["n"] for r in s.get_many(["c1", "c2"])] == [1, 2]
    s.observe({"seq_nos": [n + 1000 for n in w.seq_nos]})
    with pytest.raises(chloedb.ChloeDBError) as e:
        s.query("SELECT COUNT(*) FROM t")
    assert e.value.code == "conflict"


# ------------------------------------------------------------- errors


def test_error_codes_from_a_server(one):
    c = chloedb.connect(one)
    with pytest.raises(chloedb.BadRequestError) as e:
        c.query("SELEC nothing")
    assert isinstance(e.value, chloedb.ChloeDBError) and e.value.code == "bad_request" and e.value.status == 400
    w = c.upsert({"id": "e1", "n": 1})
    with pytest.raises(chloedb.ConflictError) as e:
        c.put("e1", {"n": 2}, if_seq_no=w.seq_no + 1000)
    assert e.value.code == "conflict"
    with pytest.raises(chloedb.NotFoundError) as e:
        c._call("GET", "/v1/no/such/route")
    assert e.value.code == "not_found"
    try:
        c.query("SELEC nothing")
    except chloedb.ChloeDBError as err:  # the base class still catches them
        assert err.code == "bad_request"


def test_error_code_rules():
    code = chloedb.error_code
    assert code("busy: the database is open elsewhere") == "busy"
    assert code("timeout: the query ran past its deadline", 504) == "timeout"
    assert code("query error: no such table") == "bad_request"
    assert code("x", 429) == "overloaded"
    assert code("x", 503) == "unavailable"
    assert code("x", 500) == "internal"
    assert code("x", 409, {"partial": True}) == "partial_write"
    assert code("x", 500, {"code": "custom"}) == "custom"
    assert code("x") == "error"
    e = chloedb.ChloeDBError("busy: held")
    assert e.code == "busy"
    e.code = "other"
    assert e.code == "other"


# ------------------------------------------------------------- Database


needs_native = pytest.mark.skipif(not chloedb._native.NATIVE, reason="the extension is not built")


@needs_native
def test_database_refuses_client_options(db):
    with pytest.raises(TypeError, match="timeout_ms"):
        db.query("SELECT COUNT(*) FROM books", timeout_ms=1)
    with pytest.raises(TypeError, match="idempotency_key"):
        db.sql("UPDATE books SET year = 1 WHERE id = 'b0001'", idempotency_key="abc")
    with pytest.raises(TypeError, match="idempotency_key"):
        db.write([{"op": "delete", "key": "b0001"}], idempotency_key="abc")
    with pytest.raises(TypeError, match="keep_alive_ms"):
        list(db.scroll("SELECT id FROM books", keep_alive_ms=10))
    with pytest.raises(TypeError, match="timeout_ms"):
        db.query_arrow("SELECT id FROM books", timeout_ms=1)
    # Nothing was written by the refused calls.
    assert db.get("b0001")["year"] == 1901
    # A parameter of such a name still binds, from params.
    assert db.query("SELECT id FROM books WHERE year = :timeout_ms", {"timeout_ms": 1901}).keys == ["b0001", "b0121"]
    # Reads that see a write: in this process every read does.
    assert db.query("SELECT COUNT(*) FROM books", min_seq_no=5, min_seq_nos=[1, 2]).rows == [[200]]
    assert db.get("b0001", min_seq_no=5)["id"] == "b0001"
    assert db.search("dragon", min_seq_no=5).keys


@needs_native
def test_database_error_codes(db, tmp_path):
    with pytest.raises(chloedb.BadRequestError) as e:
        db.query("SELEC nothing")
    assert e.value.code == "bad_request"
    with pytest.raises(chloedb.ChloeDBError) as e:
        db.insert({"id": "b0001", "title": "again"})
    assert e.value.code == "bad_request"
