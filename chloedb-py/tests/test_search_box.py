"""The persona study's fourth round (19 November 2026), in Python.

MAYA-403: a call that reads or writes one table and names none, on a
database or a server of several tables, said nothing and used the first
(`db.search("lasagna")` answered rows of another table). It now raises and
names the tables; a database of one table needs no name.

MAYA-405 (and MAYA-304): sqlite3's `?` and psycopg's `%s` with values by
position were taken as a dict of no names and answered `unbound parameter`,
or a TypeError that did not say what to write. They raise a TypeError that
shows the call with names, and the server's own error for a `?` says so
too.

MAYA-401 and MAYA-402 from Python: the search box the docs teach returns
every row its WHERE matched, and `syntax => 'plain'` takes a visitor's
text as words."""

import pytest

import chloedb

RECIPES = {"table": "recipes", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "name", "type": "text"}]}
NOTES = {"table": "notes", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "body", "type": "text"}]}
ROWS = [
    {"id": "r1", "name": "lasagne al forno"},
    {"id": "r2", "name": "lasagna with spinach"},
    {"id": "r3", "name": "vegetable lasagna"},
    {"id": "r4", "name": "lasagna soup"},
    {"id": "r5", "name": "tomato soup"},
]


def one_table(path):
    db = chloedb.Database.create(path, RECIPES)
    db.insert(ROWS)
    return db


def two_tables(path):
    db = one_table(path)
    db._create(NOTES)
    db.insert([{"id": "n1", "body": "buy lasagna sheets"}], table="notes")
    return db


def refusals(db, holder):
    """Each call that reads or writes one table refuses to pick one."""
    calls = {
        "search": lambda: db.search("lasagna"),
        "insert": lambda: db.insert({"id": "x", "name": "x"}),
        "upsert": lambda: db.upsert({"id": "x", "name": "x"}),
        "delete": lambda: db.delete(["r1"]),
        "get": lambda: db.get("r1"),
        "get_many": lambda: db.get_many(["r1"]),
        "table": lambda: db.table(),
    }
    for call, run in calls.items():
        with pytest.raises(chloedb.ChloeDBError) as e:
            run()
        said = str(e.value)
        assert f"db.{call} names no table" in said, said
        assert f"this {holder} has 2: recipes, notes" in said, said
        assert "table='notes'" in said, said


def names_it(db):
    assert {r["id"] for r in db.search("lasagna", table="recipes", mode="text")} >= {"r2", "r3", "r4"}
    assert [r["id"] for r in db.search("lasagna", table="notes", mode="text")] == ["n1"]
    assert db.get("r1", table="recipes")["id"] == "r1"
    db.upsert({"id": "n2", "body": "soup"}, table="notes")
    assert db.get("n2", table="notes")["body"] == "soup"


def test_one_table_needs_no_name(tmp_path):
    with one_table(tmp_path / "one.chloedb") as db:
        assert db.get("r1")["id"] == "r1"
        assert "r5" in db.search("tomato", mode="text").keys
        db.upsert({"id": "r6", "name": "pea soup"})
        assert db.get("r6")["name"] == "pea soup"


def test_several_tables_and_none_named_raise_on_a_file(tmp_path):
    with two_tables(tmp_path / "two.chloedb") as db:
        refusals(db, "database")
        names_it(db)
        with pytest.raises(chloedb.ChloeDBError, match="names no table"):
            db.frame()
        assert db.frame("notes")


def test_several_tables_and_none_named_raise_on_a_server(tmp_path, serve):
    two_tables(tmp_path / "two.chloedb").close()
    c = chloedb.connect(serve(tmp_path / "two.chloedb"))
    refusals(c, "server")
    names_it(c)
    # A table made through the client is seen at once.
    c.sql("CREATE TABLE more (id KEYWORD PRIMARY KEY, body TEXT)")
    with pytest.raises(chloedb.ChloeDBError, match="has 3: recipes, notes, more"):
        c.search("soup")


def test_one_table_on_a_server_needs_no_name(tmp_path, serve):
    one_table(tmp_path / "one.chloedb").close()
    c = chloedb.connect(serve(tmp_path / "one.chloedb"))
    assert c.get("r1")["id"] == "r1"
    assert "r5" in c.search("tomato", mode="text").keys


POSITIONAL = [
    ("SELECT id FROM recipes WHERE MATCH(name, ?)", "soup"),
    ("SELECT id FROM recipes WHERE MATCH(name, ?)", ("soup",)),
    ("SELECT id FROM recipes WHERE MATCH(name, ?)", ["soup"]),
    ("SELECT id FROM recipes WHERE MATCH(name, %s)", ("soup",)),
    ("SELECT id FROM recipes WHERE id = ? OR id = ?", ("r1", "r5")),
    ("SELECT id FROM recipes WHERE id = $1", ("r1",)),
]


def teaches(db):
    for sql, given in POSITIONAL:
        with pytest.raises(TypeError) as e:
            db.query(sql, given)
        said = str(e.value)
        assert "named in ChloeDB's SQL (:name)" in said and "by position" in said, said
        assert ":p1" in said and "p1=" in said, said
        assert repr(given if isinstance(given, str) else given[0]) in said, said
    with pytest.raises(TypeError, match="db.sql"):
        db.sql("SELECT id FROM recipes WHERE id = ?", ("r1",))
    # A vector is one value, not values by position.
    with pytest.raises(TypeError) as e:
        db.query("SELECT id FROM recipes WHERE id = ?", [1.0, 2.0])
    assert "p1=[1.0, 2.0]" in str(e.value), str(e.value)
    # By name, the same query runs.
    assert db.query("SELECT id FROM recipes WHERE MATCH(name, :q)", q="soup").keys
    assert db.query("SELECT id FROM recipes WHERE MATCH(name, :q)", {"q": "soup"}).keys
    # A ? with no values: the engine's own error says to name it.
    with pytest.raises(chloedb.ChloeDBError) as e:
        db.query("SELECT id FROM recipes WHERE MATCH(name, ?)")
    said = str(e.value)
    assert "by position" in said and ":q" in said and "q=..." in said, said


def test_a_placeholder_by_position_teaches_on_a_file(tmp_path):
    with one_table(tmp_path / "one.chloedb") as db:
        teaches(db)


def test_a_placeholder_by_position_teaches_on_a_server(tmp_path, serve):
    one_table(tmp_path / "one.chloedb").close()
    teaches(chloedb.connect(serve(tmp_path / "one.chloedb")))


TYPO = "SELECT id FROM recipes WHERE MATCH(name, :q, fuzzy => 'auto') ORDER BY bm25(name, :q) DESC LIMIT 30"
PLAIN = "SELECT id FROM recipes WHERE MATCH(name, :q, syntax => 'plain', fuzzy => 'auto') ORDER BY bm25(name, :q) DESC LIMIT 30"


def search_box(db):
    for q in ["lasagne", "lasagna", "lasagnr"]:
        n = db.query("SELECT COUNT(*) FROM recipes WHERE MATCH(name, :q, fuzzy => 'auto')", q=q).scalar()
        assert n == 4, q
        assert len(db.query(TYPO, q=q).keys) == n, q
    assert db.query(TYPO, q="lasagne").keys[0] == "r1"
    for q in ['lasagna"', "(lasagna", "lasagna -", "lasagna OR"]:
        assert set(db.query(PLAIN, q=q).keys) == {"r1", "r2", "r3", "r4"}, q
    with pytest.raises(chloedb.ChloeDBError) as e:
        db.query(TYPO, q='lasagna"')
    assert "syntax => 'plain'" in str(e.value)


def test_the_search_box_on_a_file(tmp_path):
    with one_table(tmp_path / "one.chloedb") as db:
        search_box(db)


def test_the_search_box_on_a_server(tmp_path, serve):
    one_table(tmp_path / "one.chloedb").close()
    search_box(chloedb.connect(serve(tmp_path / "one.chloedb")))
