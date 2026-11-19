"""Round 3 of the persona study (18 November 2026):

- SAM-301: the DB-API driver typed a column by its name, so `ts AS d`,
  `MAX(ts)`, `i.ts` in a join, `date_trunc`, `CAST(.. AS DATE)` and
  `+ INTERVAL` came back as text, and `Result.to_pandas()` gave text even
  for `ts`. An answer now carries each column's type (`Result.types`),
  in a file and through a server, and both use it.
- PRIYA-301: python.md's `add_column("emb", "vector", embed=["title"],
  model=...)` failed ("unknown option model"), and the form that worked
  left the rows already there without a vector, so `similarity()` found
  none of them.
- PRIYA-303: `{"embed": text}` and `{"vector_of": key}` used in
  `similarity(bge, :v)` took the table's first vector field (`emb`).
"""

import datetime as dt
import warnings

import pytest

import chloedb
import chloedb.dbapi as dbapi

UTC = dt.timezone.utc

SELECTS = [
    "SELECT ts FROM inv ORDER BY id LIMIT 1",
    "SELECT ts AS d FROM inv ORDER BY id LIMIT 1",
    "SELECT MAX(ts) FROM inv",
    "SELECT i.ts FROM inv i JOIN cust c ON c.id = i.cust ORDER BY i.id LIMIT 1",
    "SELECT date_trunc('month', ts) AS m FROM inv ORDER BY id LIMIT 1",
    "SELECT date_bin('1 day', ts) AS b FROM inv ORDER BY id LIMIT 1",
    "SELECT CAST(ts AS DATE) AS day FROM inv ORDER BY id LIMIT 1",
    "SELECT ts + INTERVAL '1 day' AS next FROM inv ORDER BY id LIMIT 1",
]


def invoices(db):
    db.sql("CREATE TABLE cust (id INT PRIMARY KEY, name KEYWORD)")
    db.sql("INSERT INTO inv (id, cust, ts, note) VALUES (1, 10, '2009-01-01T00:00:00Z', 'first'), (2, 11, '2013-12-22T10:30:00Z', NULL), (3, 10, NULL, NULL)")
    db.sql("INSERT INTO cust (id, name) VALUES (10, 'Ana'), (11, 'Sam')")


INV = {"table": "inv", "key": "id", "fields": [{"name": "id", "type": "int"}, {"name": "cust", "type": "int"}, {"name": "ts", "type": "timestamp"}, {"name": "note", "type": "keyword"}]}


@pytest.fixture(params=["file", "server"])
def store(request, tmp_path):
    db = chloedb.Database.create(tmp_path / "inv.chloedb", INV)
    invoices(db)
    if request.param == "file":
        yield db
        db.close()
        return
    db.close()
    url = request.getfixturevalue("serve")(tmp_path / "inv.chloedb")
    yield chloedb.Client(url)


def test_a_column_is_typed_by_its_expression(store):
    for sql in SELECTS:
        r = store.query(sql)
        assert r.types == ["timestamp"], (sql, r.types)
    r = store.query("SELECT id, ts, note, CAST(ts AS TEXT) AS ts2 FROM inv ORDER BY id")
    assert r.types == ["int", "timestamp", "text", "text"]
    assert store.query("SELECT ts FROM inv WHERE id = 3").types == [None]


def test_the_driver_gives_datetimes_for_every_timestamp_column(store):
    cur = dbapi.connect(store).cursor()
    for sql in SELECTS:
        cur.execute(sql)
        (v,) = cur.fetchone()
        assert isinstance(v, dt.datetime) and v.tzinfo is not None, (sql, v)
        assert cur.description[0][1] == "timestamp" and cur.description[0][1] == dbapi.DATETIME, (sql, cur.description)
    cur.execute("SELECT MAX(ts) AS last, note, id FROM inv GROUP BY note, id ORDER BY id LIMIT 1")
    assert [d[1] for d in cur.description] == ["timestamp", "keyword", "int"], "a text column that is a field keeps the field's type"
    assert cur.fetchone()[0] == dt.datetime(2009, 1, 1, tzinfo=UTC)
    cur.execute("SELECT CAST(ts AS TEXT) AS ts FROM inv ORDER BY id LIMIT 1")
    assert (cur.description[0][1], cur.fetchone()[0]) == ("text", "2009-01-01T00:00:00Z"), "a cast to text is text, whatever its name"
    cur.execute("SELECT ts FROM inv WHERE id = 3")
    assert cur.description[0][1] == "timestamp", "a column of NULLs takes the field's type"


def test_to_pandas_and_read_sql_give_datetimes(store):
    pd = pytest.importorskip("pandas")
    df = store.sql("SELECT ts, ts AS d, date_trunc('month', ts) AS m, id FROM inv ORDER BY id").to_pandas()
    for c in ["ts", "d", "m"]:
        assert str(df[c].dtype).startswith("datetime64") and "UTC" in str(df[c].dtype), (c, df.dtypes)
    assert df["m"][1] == pd.Timestamp("2013-12-01", tz="UTC") and pd.isna(df["ts"][2])
    assert str(df["id"].dtype) == "int64"
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")  # pandas warns of a connection that is not SQLAlchemy's
        rs = pd.read_sql("SELECT ts, MAX(ts) AS last FROM inv GROUP BY ts ORDER BY ts", dbapi.connect(store))
    assert all(str(t).startswith("datetime64") for t in rs.dtypes), rs.dtypes


def test_a_union_of_ints_and_floats_is_float(store):
    r = store.query("SELECT 1 AS v UNION ALL SELECT 2.5")
    assert r.rows == [[1.0], [2.5]] and isinstance(r.rows[0][0], float) and r.types == ["float"]


def notes(path):
    db = chloedb.open(path, schema={"table": "notes", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "title", "type": "text"}]})
    db.upsert([{"id": f"n{i}", "title": t} for i, t in enumerate(["how to reset a password", "refund to the original card", "forgot my login", ""])])
    return db


def test_add_column_embeds_the_rows_already_there(tmp_path):
    db = notes(tmp_path / "n.chloedb")
    q = "SELECT id FROM notes ORDER BY similarity(emb, :v) DESC LIMIT 5"
    seen = []
    db.add_column("emb", "vector", embed=["title"], model="hash:32", progress=lambda done, total: seen.append((done, total)))
    assert seen[0] == (0, 4) and seen[-1] == (4, 4), seen
    assert len(db.query(q, v={"embed": "forgot my login"}).rows) == 3, "every row with a title"
    assert db.missing_vectors("emb") == 1, "the row of no title gets none"
    db.close()


def test_alter_table_embeds_the_rows_already_there(tmp_path):
    db = notes(tmp_path / "n.chloedb")
    db.sql("ALTER TABLE notes ADD COLUMN emb VECTOR(32) OPTIONS(embed = 'title', model = 'hash:32')")
    assert len(db.query("SELECT id FROM notes ORDER BY similarity(emb, :v) DESC LIMIT 5", v={"embed": "refund"}).rows) == 3
    db.close()


def test_add_column_says_when_its_model_is_not_open(tmp_path):
    db = notes(tmp_path / "n.chloedb")
    with pytest.warns(UserWarning, match=r"the 4 rows already in notes have no vector.*embed_backfill\('emb'"):
        db.add_column("emb", "vector", embed={"from": ["title"], "model": "no-such-model-here"}, dim=8)
    assert db.missing_vectors("emb") == 4
    db.close()


def two_vectors(path):
    db = chloedb.Database.create(path, {"table": "docs", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "title", "type": "text"}, {"name": "emb", "type": "vector", "embed": {"from": ["title"], "model": "hash:32"}}, {"name": "bge", "type": "vector", "dim": 4}]})
    db.upsert([{"id": "a", "title": "vitamin d", "bge": [1, 0, 0, 0]}, {"id": "b", "title": "masks", "bge": [0, 1, 0, 0]}])
    return db


@pytest.mark.parametrize("where", ["file", "server"])
def test_a_parameter_s_vector_is_the_field_it_is_used_with(tmp_path, where, request):
    db = two_vectors(tmp_path / "v.chloedb")
    if where == "server":
        db.close()
        db = chloedb.Client(request.getfixturevalue("serve")(tmp_path / "v.chloedb"))
    q = "SELECT id FROM docs ORDER BY similarity(bge, :v) DESC LIMIT 2"
    assert db.query(q, v={"vector_of": "b"}).rows == [["b"], ["a"]]
    with pytest.raises(chloedb.ChloeDBError, match=r"bge (has no model|is embedded by no model)"):
        db.query(q, v={"embed": "vitamin d"})
    assert len(db.query("SELECT id FROM docs ORDER BY similarity(emb, :v) DESC LIMIT 2", v={"embed": "vitamin d"}).rows) == 2
    with pytest.raises(chloedb.ChloeDBError, match=r"embed\('<model>', :v\)"):
        db.query(q, v="vitamin d")
