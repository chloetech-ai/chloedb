"""`chloedb.serve`: a chloedb-server started from Python, and a client of it."""
import os
import pickle
import re
import tomllib

import pytest

import chloedb

from conftest import server_binary

ROOT = os.path.join(os.path.dirname(__file__), "..", "..")


def test_the_server_extra_is_this_release():
    with open(os.path.join(ROOT, "chloedb-py", "pyproject.toml"), "rb") as f:
        project = tomllib.load(f)["project"]
    with open(os.path.join(ROOT, "chloedb-server", "Cargo.toml"), "rb") as f:
        server = tomllib.load(f)["package"]["version"]
    assert project["optional-dependencies"]["server"] == [f"chloedb-server=={project['version']}"]
    assert server == project["version"], "chloedb-server's wheels carry the server's version"


@pytest.fixture
def binary():
    path = server_binary()
    if not path:
        pytest.skip("chloedb-server is not built")
    return os.path.abspath(path)


def test_serve_a_temporary_database(binary):
    with chloedb.serve(binary=binary) as c:
        server = c.server
        assert server.running and server.temporary_db and c.url == server.url
        c.sql("CREATE TABLE notes (id KEYWORD PRIMARY KEY, body TEXT)")
        c.insert([{"id": "n1", "body": "a red tent"}, {"id": "n2", "body": "a blue lamp"}], table="notes")
        assert [r["id"] for r in c.query("SELECT id FROM notes WHERE MATCH(body, 'tent')")] == ["n1"]
        # A session, and a copy, are clients of the same server; closing
        # them leaves it running.
        s = c.session()
        assert len(s.query("SELECT id FROM notes").rows) == 2
        s.close()
        copy = pickle.loads(pickle.dumps(c))
        assert copy.server is None and copy.url == c.url
        copy.close()
        assert server.running
        assert os.path.exists(server.db)
    assert not server.running
    assert not os.path.exists(server.db), "a temporary database goes with its server"
    assert c.server.stop() == server.process.returncode  # again: nothing to do


def test_serve_a_database_with_a_token(binary, tmp_path):
    db = tmp_path / "kept.chloedb"
    c = chloedb.serve(db, token="pick-a-secret", args=["--demo", "30"], binary=binary)
    try:
        assert c.query("SELECT COUNT(*) AS n FROM products").rows == [[30]]
        with pytest.raises(chloedb.ChloeDBError) as e:
            chloedb.Client(c.url, token="wrong", retry_for=0).query("SELECT 1")
        assert e.value.status == 401
        assert "serving" in c.server.output()
    finally:
        c.close()
    assert os.path.exists(db), "a database it was given stays"
    # The same database, served again, has its rows.
    with chloedb.serve(db, binary=binary) as again:
        assert again.query("SELECT COUNT(*) AS n FROM products").rows == [[30]]


def test_a_server_that_does_not_start_says_why(binary, tmp_path):
    with pytest.raises(chloedb.ChloeDBError, match="stopped with status") as e:
        chloedb.serve(tmp_path / "x.chloedb", binary=binary, args=["--no-such-flag"])
    assert "--no-such-flag" in str(e.value)


def test_find_server(binary, tmp_path, monkeypatch):
    assert chloedb.find_server(binary) == binary
    monkeypatch.setenv("CHLOEDB_SERVER_BIN", binary)
    assert chloedb.find_server() == binary
    monkeypatch.delenv("CHLOEDB_SERVER_BIN")
    monkeypatch.setenv("PATH", str(tmp_path))
    import sys
    monkeypatch.setitem(sys.modules, "chloedb_server", None)  # not installed
    with pytest.raises(FileNotFoundError, match=re.escape("pip install chloedb-server")):
        chloedb.find_server()
    with pytest.raises(FileNotFoundError, match="is not a program"):
        chloedb.find_server(tmp_path / "nothing-here")
