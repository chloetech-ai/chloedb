"""Parquet files in and out (`from_parquet`, `Database.import_parquet`,
`Database.export_parquet`), rows from any Arrow source
(`Database.import_arrow`), and `s3://` URLs, against a local S3 (moto)."""

import datetime
import os

import pytest

import strata
from conftest import BOOKS, book

pa = pytest.importorskip("pyarrow")
pq = pytest.importorskip("pyarrow.parquet")


def events(first=1, n=3):
    ids = list(range(first, first + n))
    return pa.table(
        {
            "id": pa.array(ids, pa.int64()),
            "title": [f"The quick brown fox jumps over the lazy dog, story number {i}" for i in ids],
            "kind": pa.array(["click", "view", "click"][:n]).dictionary_encode(),
            "at": pa.array([datetime.datetime(2026, 1, 1, tzinfo=datetime.timezone.utc) + datetime.timedelta(hours=i) for i in ids], pa.timestamp("ms", tz="UTC")),
            "emb": pa.array([[float(i), 1.0, 0.0] for i in ids], pa.list_(pa.float32(), 3)),
            "tags": [["new", "sale"] if i % 2 else ["new"] for i in ids],
            "price": pa.array([12.5, 2.0, None][:n], pa.float64()),
            "ok": [True, False, True][:n],
            "extra": [{"a": i, "b": "x"} for i in ids],
        }
    )


def test_from_parquet_and_back(tmp_path):
    src = tmp_path / "events.parquet"
    pq.write_table(events(), src)
    plan = strata.plan_parquet(src)
    assert plan["table"] == "events" and plan["key"] == "id" and plan["rows"] == 3
    types = {f["name"]: f["type"] for f in plan["fields"]}
    assert types == {"id": "int", "title": "text", "kind": "keyword", "at": "timestamp", "emb": "vector", "tags": "keyword", "price": "float", "ok": "bool", "extra": "json"}

    with strata.from_parquet(src, tmp_path / "events.strata") as db:
        assert db.last_import["rows"] == 3 and db.last_import["created"] and db.last_import["bulk"]
        assert db.query("SELECT id FROM events WHERE MATCH(title, 'foxes') AND kind = 'view'").scalar() == 2
        assert db.query("SELECT id FROM events WHERE tags = 'sale' ORDER BY id").column(0) == [1, 3]
        assert db.query("SELECT ok FROM events WHERE id = 2").scalar() is False
        assert db.query("SELECT extra FROM events WHERE id = 3").scalar() == {"a": 3, "b": "x"}

        # Out: one file, which pyarrow reads with its types and Strata reads back.
        out = tmp_path / "copy.parquet"
        done = db.export_parquet(out)
        assert [(t["table"], t["rows"], t["file"]) for t in done["tables"]] == [("events", 3, "copy.parquet")]
        with pytest.raises(strata.StrataError, match="exists"):
            db.export_parquet(out)
        t = pq.read_table(out)
        assert t.schema.field("emb").type == pa.list_(pa.float32(), 3)
        assert t.schema.field("at").type == pa.timestamp("us", tz="UTC")
        assert t.schema.field("ok").type == pa.bool_()
        assert t.schema.field("tags").type == pa.list_(pa.string())
        assert b"strata.schema" in t.schema.metadata
        assert sorted(t.column("id").to_pylist()) == [1, 2, 3]
        with strata.from_parquet(out, tmp_path / "back.strata") as back:
            assert back.schema == db.schema
            assert back.query("SELECT emb FROM events WHERE id = 3").scalar() == [3.0, 1.0, 0.0]

        # Into the table that exists: upserted by key, unknown columns left out.
        more = events(3, 2).append_column("unknown", pa.array(["u", "v"]))
        pq.write_table(more, tmp_path / "more.parquet")
        r = db.import_parquet(tmp_path / "more.parquet", table="events")
        assert (r["rows"], r["created"], r["bulk"]) == (2, False, False)
        assert "unknown" in r["skipped"]
        assert len(db) == 4


def test_a_spark_directory(tmp_path):
    root = tmp_path / "sales"
    for year, first in [(2025, 1), (2026, 4)]:
        d = root / f"year={year}"
        d.mkdir(parents=True)
        pq.write_table(events(first), d / "part-00000-abc.snappy.parquet", compression="snappy")
    (root / "_SUCCESS").write_text("")
    with strata.from_parquet(root, tmp_path / "s.strata", key="") as db:
        assert db.last_import["files"] == 2 and db.last_import["rows"] == 6
        assert db.schema["table"] == "sales" and "key" not in db.schema
        assert db.query("SELECT COUNT(*) FROM sales WHERE year = 2026").scalar() == 3


def test_import_arrow(db, tmp_path):
    rows = [book(i) for i in range(200, 210)]
    t = pa.Table.from_pylist(rows).append_column("extra", pa.array(range(10)))
    r = db.import_arrow(t)
    assert (r["table"], r["rows"], r["bulk"], r["skipped"]) == ("books", 10, False, ["extra"])
    assert len(db) == 210
    # A reader, a batch, a polars frame and a query's own answer all give a stream.
    db.import_arrow(pa.RecordBatchReader.from_batches(t.schema, t.to_batches()))
    db.import_arrow(t.to_batches()[0])
    assert len(db) == 210
    pl = pytest.importorskip("polars")
    df = pl.DataFrame([{**book(i), "rating": 5.0} for i in range(205, 207)])
    assert db.import_arrow(df)["rows"] == 2
    assert db.query("SELECT rating FROM books WHERE id = 'b0205'").scalar() == 5.0
    with strata.Database.create(tmp_path / "copy.strata", BOOKS) as copy:
        r = copy.import_arrow(db.query_arrow("SELECT id, title, description, genres, year, rating FROM books"))
        assert (r["rows"], r["bulk"]) == (210, True)
        assert copy.query("SELECT genres FROM books WHERE id = 'b0005'").scalar() == book(5)["genres"]
    with pytest.raises(TypeError, match="Arrow stream"):
        db.import_arrow([1, 2, 3])


def test_client_parquet_in_and_out(tmp_path, serve):
    path = tmp_path / "served.strata"
    with strata.Database.create(path, BOOKS) as db:
        db.insert([book(i) for i in range(20)])
    client = strata.connect(serve(path))
    src = tmp_path / "events.parquet"
    pq.write_table(events(), src)
    plan = client.plan_parquet(src, table="events")
    assert plan["exists"] is False and plan["key"] == "id"
    r = client.import_parquet(src, table="events")
    assert (r["table"], r["written"], r["created"]) == ("events", 3, True)
    assert client.query("SELECT COUNT(*) FROM events").scalar() == 3
    out = tmp_path / "books.parquet"
    assert client.export_parquet(out, table="books") == "books"
    assert pq.read_table(out).num_rows == 20


@pytest.fixture
def s3(monkeypatch):
    """A local S3 (moto's server) with a bucket, and the AWS_* variables
    that point Strata at it. Skips when moto is not installed."""
    server = pytest.importorskip("moto.server")
    boto3 = pytest.importorskip("boto3")
    srv = server.ThreadedMotoServer(ip_address="127.0.0.1", port=0)
    srv.start()
    host, port = srv.get_host_and_port()
    endpoint = f"http://{host}:{port}"
    env = {"AWS_ACCESS_KEY_ID": "testing", "AWS_SECRET_ACCESS_KEY": "testing", "AWS_REGION": "us-east-1", "AWS_ENDPOINT_URL": endpoint, "AWS_ALLOW_HTTP": "true"}
    for k, v in env.items():
        monkeypatch.setenv(k, v)
    monkeypatch.delenv("AWS_SESSION_TOKEN", raising=False)
    monkeypatch.delenv("AWS_PROFILE", raising=False)
    c = boto3.client("s3", endpoint_url=endpoint, region_name="us-east-1", aws_access_key_id="testing", aws_secret_access_key="testing")
    c.create_bucket(Bucket="lake")
    yield c
    srv.stop()


def test_s3_urls(s3, tmp_path):
    # A Spark-style dataset under a prefix, and a folder marker as consoles make.
    for year, first in [(2025, 1), (2026, 4)]:
        p = tmp_path / f"part-{year}.parquet"
        pq.write_table(events(first), p)
        s3.upload_file(str(p), "lake", f"raw/events/year={year}/part-00000.parquet")
    s3.put_object(Bucket="lake", Key="raw/events/", Body=b"")
    s3.put_object(Bucket="lake", Key="raw/events/_SUCCESS", Body=b"")
    plan = strata.plan_parquet("s3://lake/raw/events")
    assert (plan["table"], plan["files"], plan["rows"]) == ("events", 2, 6)
    assert plan["partitions"] == {"year": "year"}
    with strata.from_parquet("s3://lake/raw/events/", tmp_path / "e.strata") as db:
        assert db.query("SELECT COUNT(*) FROM events WHERE year = 2025").scalar() == 3
        # One object, by its key.
        r = db.import_parquet("s3://lake/raw/events/year=2026/part-00000.parquet", table="y2026")
        assert (r["table"], r["rows"]) == ("y2026", 3)
        # Out to an object, and to a prefix of them; existing ones are refused.
        db.export_parquet("s3://lake/out/events.parquet", tables="events")
        got = s3.get_object(Bucket="lake", Key="out/events.parquet")["Body"].read()
        assert got[:4] == b"PAR1"
        with pytest.raises(strata.StrataError, match="exists"):
            db.export_parquet("s3://lake/out/events.parquet", tables="events")
        db.export_parquet("s3://lake/out/all/")
        keys = sorted(o["Key"] for o in s3.list_objects_v2(Bucket="lake", Prefix="out/all/")["Contents"])
        assert keys == ["out/all/events.parquet", "out/all/y2026.parquet"]
        with pytest.raises(strata.StrataError, match="exists"):
            db.export_parquet("s3://lake/out/all")
        # Other formats read and write URLs too.
        db.export_sqlite("s3://lake/out/e.sqlite")
        with strata.from_sqlite("s3://lake/out/e.sqlite", tmp_path / "from-s3.strata") as copy:
            assert copy.query("SELECT COUNT(*) FROM events").scalar() == 6
    s3.put_object(Bucket="lake", Key="rows/more.jsonl", Body=b'{"id": 7, "title": "seven"}\n')
    with strata.Database.open(tmp_path / "e.strata") as db:
        assert db.import_file("s3://lake/rows/more.jsonl", table="events")["rows"] == 1
        assert len(db.query("SELECT id FROM events WHERE id = 7").rows) == 1
    with pytest.raises(strata.StrataError, match="no such object"):
        strata.plan_parquet("s3://lake/nothing/here")
