import datetime

import pytest

import chloedb
import chloedb.dbapi as dbapi
from adapters_common import target  # noqa: F401


@pytest.fixture
def conn(target):
    c = dbapi.connect(target)
    cur = c.cursor()
    cur.execute("CREATE TABLE books (id KEYWORD PRIMARY KEY, title TEXT, year INT, rating FLOAT, at TIMESTAMP)")
    cur.executemany(
        "INSERT INTO books (id, title, year, rating, at) VALUES (:id, :title, :year, :rating, :at)",
        [
            {"id": "b1", "title": "The river and the dragon", "year": 1999, "rating": 4.5, "at": datetime.datetime(2026, 10, 1, 12, 0, tzinfo=datetime.timezone.utc)},
            {"id": "b2", "title": "Winter garden", "year": 2005, "rating": 3.0, "at": datetime.date(2026, 10, 2)},
            {"id": "b3", "title": "Star letters", "year": 2024, "rating": None, "at": None},
        ],
    )
    assert cur.rowcount == 3
    yield c
    c.close()


def test_module_globals():
    assert dbapi.apilevel == "2.0"
    assert dbapi.paramstyle == "named"
    assert dbapi.threadsafety in (0, 1, 2, 3)
    assert issubclass(dbapi.ProgrammingError, dbapi.DatabaseError)
    assert issubclass(dbapi.DatabaseError, dbapi.Error)
    assert issubclass(dbapi.Error, Exception)
    assert dbapi.Date(2026, 1, 2) == datetime.date(2026, 1, 2)
    assert dbapi.STRING == "text" and dbapi.NUMBER == "int" and dbapi.DATETIME == "timestamp"
    assert dbapi.STRING != "int"


def test_reads(conn):
    cur = conn.cursor()
    cur.execute("SELECT id, title, year, rating, at FROM books WHERE year >= :y ORDER BY year", {"y": 2000})
    assert [d[0] for d in cur.description] == ["id", "title", "year", "rating", "at"]
    assert cur.description[0][1] == dbapi.STRING
    assert cur.description[2][1] == dbapi.NUMBER
    assert cur.description[4][1] == dbapi.DATETIME
    assert cur.rowcount == 2
    first = cur.fetchone()
    assert first[:4] == ("b2", "Winter garden", 2005, 3.0)
    # A TIMESTAMP is a datetime in UTC, as description says.
    assert first[4] == datetime.datetime(2026, 10, 2, tzinfo=datetime.timezone.utc)
    assert cur.fetchone()[0] == "b3"
    assert cur.fetchone() is None
    cur.execute("SELECT id FROM books ORDER BY id")
    cur.arraysize = 2
    assert cur.fetchmany() == [("b1",), ("b2",)]
    assert cur.fetchall() == [("b3",)]
    assert [r for r in conn.execute("SELECT COUNT(*) AS n FROM books")] == [(3,)]
    cur.execute("SELECT id FROM books ORDER BY bm25(title, :q) DESC LIMIT 1", {"q": "dragon"})
    assert cur.fetchall() == [("b1",)]
    cur.execute("SELECT year, COUNT(*) FROM books GROUP BY year ORDER BY year")
    assert [d[0] for d in cur.description][0] == "year"
    assert len(cur.fetchall()) == 3


def test_writes(conn):
    cur = conn.cursor()
    cur.execute("UPDATE books SET rating = :r WHERE year < :y", {"r": 5.0, "y": 2010})
    assert cur.rowcount == 2
    assert cur.description is None
    with pytest.raises(dbapi.ProgrammingError):
        cur.fetchall()
    cur.execute("DELETE FROM books WHERE id = :id", {"id": "b3"})
    assert cur.rowcount == 1
    assert conn.execute("SELECT COUNT(*) FROM books").fetchone() == (2,)
    conn.commit()
    with pytest.raises(dbapi.NotSupportedError):
        conn.rollback()


def test_errors(conn):
    cur = conn.cursor()
    with pytest.raises(dbapi.ProgrammingError) as e:
        cur.execute("SELEC nonsense")
    assert e.value.code == "bad_request"
    assert isinstance(e.value.__cause__, chloedb.ChloeDBError)
    with pytest.raises(dbapi.Error):
        cur.execute("SELECT * FROM missing_table")
    with pytest.raises(dbapi.ProgrammingError, match="named"):
        cur.execute("SELECT id FROM books WHERE year = ?", (1999,))
    cur.close()
    with pytest.raises(dbapi.InterfaceError):
        cur.execute("SELECT 1")
    conn.close()
    with pytest.raises(dbapi.InterfaceError):
        conn.cursor()


def test_pandas_reads_it(conn):
    pd = pytest.importorskip("pandas")
    import warnings

    with warnings.catch_warnings():
        warnings.simplefilter("ignore")  # pandas asks for SQLAlchemy, and works
        df = pd.read_sql("SELECT id, year FROM books ORDER BY year", conn)
    assert list(df["id"]) == ["b1", "b2", "b3"]
    assert list(df.columns) == ["id", "year"]


def test_wraps_an_open_store(tmp_path):
    db = chloedb.open(tmp_path / "w.chloedb", {"table": "t", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "n", "type": "int"}]})
    with dbapi.connect(db) as c:
        c.execute("INSERT INTO t (id, n) VALUES ('a', 1)")
        assert c.execute("SELECT n FROM t").fetchall() == [(1,)]
    # Not the connection's to close.
    assert db.query("SELECT COUNT(*) FROM t").scalar() == 1
    db.close()
    with dbapi.connect(str(tmp_path / "w.chloedb"), read_only=True) as c:
        assert c.execute("SELECT id FROM t").fetchall() == [("a",)]
        with pytest.raises(dbapi.Error):
            c.execute("INSERT INTO t (id, n) VALUES ('b', 2)")
