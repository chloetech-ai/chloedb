"""A table named is read alone: `search(table=)`, `table(name)`,
`{"vector_of": key}` and `{"embed": text}` parameters of a statement read
the table the call or the statement names, not the first table, in this
process and through a server (with a key limited to other tables than the
first). Also the package's version, its typing marker, `AsyncClient` and
`DuplicateKeyError`."""

import asyncio
import json
import os
import tomllib
import urllib.request

import pytest

import chloedb
from chloedb import col

FIRST = {"table": "products", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "body", "type": "text"}]}
SECOND = {
    "table": "mine_notes",
    "key": "id",
    "fields": [
        {"name": "id", "type": "keyword"},
        {"name": "body", "type": "text"},
        {"name": "vec", "type": "vector", "dim": 3},
        {"name": "emb", "type": "vector", "embed": {"from": ["body"], "model": "hash:16"}},
    ],
}
NOTES = [
    {"id": "a", "body": "rivers and mountains", "vec": [1.0, 0.0, 0.0]},
    {"id": "b", "body": "the open sea", "vec": [0.0, 1.0, 0.0]},
    {"id": "c", "body": "a mountain lake", "vec": [0.9, 0.1, 0.0]},
]


def two_tables(path):
    db = chloedb.Database.create(path, FIRST)
    db.insert([{"id": "p1", "body": "a waterproof jacket"}])
    db._create(SECOND)
    db.insert(NOTES, table="mine_notes")
    return db


def test_version_and_typing_marker():
    root = os.path.join(os.path.dirname(__file__), "..")
    with open(os.path.join(root, "pyproject.toml"), "rb") as f:
        assert chloedb.__version__ == tomllib.load(f)["project"]["version"]
    assert os.path.exists(os.path.join(os.path.dirname(chloedb.__file__), "py.typed"))


def test_database_search_takes_a_table(tmp_path):
    with two_tables(tmp_path / "two.chloedb") as db:
        hits = db.search("mountain", table="mine_notes", mode="text")
        assert set(hits.keys) == {"a", "c"}
        assert db.search("jacket").keys == ["p1"], "the first table without table="
        assert db.search("mountain", table="mine_notes", where=col("id") != "a", mode="text").keys == ["c"]
        assert db.search("mountain", table="mine_notes", mode="vector").keys[:1] in (["a"], ["c"])
        assert db.table("mine_notes").limit(5).run().keys
        with pytest.raises(chloedb.ChloeDBError):
            db.search("x", table="nope")


def test_vector_of_and_embed_read_the_from_table(tmp_path):
    with two_tables(tmp_path / "two.chloedb") as db:
        r = db.query("SELECT id FROM mine_notes ORDER BY similarity(vec, :v) DESC LIMIT 2", v={"vector_of": "a"})
        assert r.keys == ["a", "c"]
        r = db.query("SELECT id FROM mine_notes ORDER BY similarity(emb, :v) DESC LIMIT 1", v={"embed": "the open sea"})
        assert r.keys == ["b"]


def test_a_duplicate_key_is_a_conflict(tmp_path):
    with two_tables(tmp_path / "two.chloedb") as db:
        with pytest.raises(chloedb.DuplicateKeyError) as e:
            db.insert({"id": "a", "body": "again", "vec": [0.0, 0.0, 1.0]}, table="mine_notes")
        err = e.value
        assert isinstance(err, chloedb.ConflictError) and isinstance(err, chloedb.BadRequestError)
        assert err.code == "conflict"
        assert (err.table, err.field, err.key) == ("mine_notes", "id", "a")


def limited_client(tmp_path, serve):
    """A server whose first table is `products`, and a client whose key may
    use `mine_*` alone."""
    path = tmp_path / "served.chloedb"
    two_tables(path).close()
    url = serve(path, "--token", "root")
    req = urllib.request.Request(
        url + "/v1/admin/keys",
        data=json.dumps({"name": "mine", "tables": {"mine_*": "admin"}}).encode(),
        headers={"authorization": "Bearer root", "content-type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req) as r:
        key = json.loads(r.read())["secret"]
    return url, key


def test_a_key_limited_to_a_table_searches_it(tmp_path, serve):
    url, key = limited_client(tmp_path, serve)
    c = chloedb.connect(url, token=key)
    with pytest.raises(chloedb.UnauthorizedError):
        c.query("SELECT id FROM products")
    hits = c.search("mountain", table="mine_notes", mode="text")
    assert set(hits.keys) == {"a", "c"}
    # The builder reads the table alone (running its plan, /v1/plan, is not
    # a route such a key may call).
    assert c.table("mine_notes").where(col("id") == "b")._schema["table"] == "mine_notes"
    # The hash model ranks no meaning; the call embeds by that table's model.
    assert set(c.search("sea", table="mine_notes", mode="vector").keys) <= {"a", "b", "c"}
    r = c.query("SELECT id FROM mine_notes ORDER BY similarity(vec, :v) DESC LIMIT 2", v={"vector_of": "a", "table": "mine_notes"})
    assert r.keys == ["a", "c"]
    with pytest.raises(chloedb.DuplicateKeyError) as e:
        c.insert({"id": "a", "body": "again", "vec": [0, 0, 1]}, table="mine_notes")
    assert (e.value.table, e.value.field, e.value.key, e.value.code) == ("mine_notes", "id", "a", "conflict")


def test_async_client(tmp_path, serve):
    url, key = limited_client(tmp_path, serve)

    async def main():
        async with chloedb.AsyncClient(url, token=key) as db:
            hits = await db.search("mountain", table="mine_notes", mode="text")
            assert set(hits.keys) == {"a", "c"}
            assert [t["table"] for t in await db.tables()] == ["mine_notes"]
            await db.insert({"id": "d", "body": "mountain river", "vec": [1, 0, 0]}, table="mine_notes")
            both = await asyncio.gather(*(db.get(k, table="mine_notes") for k in ("a", "d")))
            assert [r["id"] for r in both] == ["a", "d"]
            pages = [p async for p in db.scroll("SELECT id FROM mine_notes", page_size=2)]
            assert sorted(r["id"] for p in pages for r in p) == ["a", "b", "c", "d"]
            q = (await db.table("mine_notes")).where(col("id") == "b")
            assert q._schema["table"] == "mine_notes"
            async with db.session() as s:
                w = await s.upsert({"id": "e", "body": "x", "vec": [0, 0, 1]}, table="mine_notes")
                assert s.seq_no is not None or s.seq_nos is not None or w == 1
                assert (await s.get("e", table="mine_notes"))["id"] == "e"
            with pytest.raises(chloedb.UnauthorizedError):
                await db.query("SELECT id FROM products")

    asyncio.run(main())
