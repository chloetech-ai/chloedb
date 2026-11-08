"""strata.Frame: the same frames on a file and on a server, checked against
answers worked out in Python from the rows."""

import datetime as dt
import math
from collections import Counter, defaultdict

import pytest

import strata
from strata import col

pa = pytest.importorskip("pyarrow")

N = 600
NOTES = ["refund for damaged item", "fast shipping great", "item arrived broken refund", "love it"]
START = dt.datetime(2026, 1, 1, tzinfo=dt.timezone.utc)


def order(i):
    return {
        "id": f"o{i:04d}",
        "user": f"u{i % 40}",
        "country": ["cl", "us", "de"][i % 3],
        "status": "paid" if i % 4 else "void",
        "amount": (i * 37) % 1000 / 10,  # all different
        "ts": START + dt.timedelta(hours=7 * i),
        "note": NOTES[i % 4],
        "emb": [math.cos(i), math.sin(i), (i % 7) / 7, 1.0],
    }


def user(i):
    return {"id": f"u{i}", "plan": "pro" if i % 3 == 0 else "free", "age": 18 + i}


ORDERS = [order(i) for i in range(N)]
USERS = [user(i) for i in range(30)]  # u30..u39 have orders and no user row


def orders_arrow():
    t = pa.Table.from_pylist([{k: v for k, v in o.items() if k != "emb"} for o in ORDERS])
    t = t.set_column(t.schema.get_field_index("ts"), "ts", pa.array([o["ts"] for o in ORDERS], pa.timestamp("us", tz="UTC")))
    return t.append_column("emb", pa.array([o["emb"] for o in ORDERS], pa.list_(pa.float32(), 4)))


@pytest.fixture(params=["file", "server"])
def shop(request, tmp_path):
    """A store with orders and users: a file, or a server of one (whose
    users table is made over HTTP)."""
    path = tmp_path / "shop.strata"
    s = strata.connect(path)
    s.create_table("orders", orders_arrow(), key="id", text=["note"])
    if request.param == "file":
        s.create_table("users", USERS)
        yield s
        s.close()
        return
    s.close()
    c = strata.connect(request.getfixturevalue("serve")(path))
    c.create_table("users", USERS)
    yield c


def ids(frame):
    return [r["id"] for r in frame.to_dicts()]


def test_tables_and_types(shop):
    assert [t["table"] for t in shop.tables()] == ["orders", "users"]
    o = shop["orders"]
    assert o.columns == ["id", "user", "country", "status", "amount", "ts", "note", "emb"]
    fields = {f["name"]: f for f in shop.tables()[0]["fields"]}
    assert fields["note"]["type"] == "text" and fields["country"]["type"] == "keyword" and fields["emb"]["type"] == "vector"
    t = o.head(2).to_arrow()
    assert t.num_rows == 2
    assert t.schema.field("amount").type == pa.float64() and pa.types.is_timestamp(t.schema.field("ts").type)
    assert o.count() == N and shop["users"].count() == 30
    with pytest.raises(strata.StrataError, match="no table 'nope'"):
        shop["nope"]


def test_filter_select_sort(shop):
    o = shop["orders"]
    got = o.filter(col("status") == "paid", country="cl").select("id", "amount").sort("amount", descending=True).head(5).to_dicts()
    want = sorted((r for r in ORDERS if r["status"] == "paid" and r["country"] == "cl"), key=lambda r: -r["amount"])[:5]
    assert got == [{"id": r["id"], "amount": r["amount"]} for r in want]
    f = o.filter(country="cl").select("id")
    assert ":p1" in f.sql() and f.params() == {"p1": "cl"}
    assert o.filter(col("amount").is_between(10, 20)).count() == sum(10 <= r["amount"] <= 20 for r in ORDERS)
    assert o.filter(col("user").is_in(["u1", "u2"])).count() == sum(r["user"] in ("u1", "u2") for r in ORDERS)
    assert ids(o.sort("amount").slice(5, 3)) == [r["id"] for r in sorted(ORDERS, key=lambda r: r["amount"])[5:8]]
    assert o.drop("emb", "note").rename({"amount": "amt"}).columns == ["id", "user", "country", "status", "amt", "ts"]
    top = o.with_columns(x=col("amount") * 2).sort("x", descending=True).head(1).to_dicts()[0]
    assert top["x"] == max(r["amount"] for r in ORDERS) * 2
    with pytest.raises(strata.StrataError):
        o.select("amount", "amount")


def test_group_by(shop):
    o = shop["orders"]
    got = {r["country"]: r for r in o.group_by("country").agg(revenue=col("amount").sum(), n=strata.len(), users=col("user").n_unique()).to_dicts()}
    for c in ("cl", "us", "de"):
        rows = [r for r in ORDERS if r["country"] == c]
        assert got[c]["n"] == len(rows) and got[c]["revenue"] == pytest.approx(sum(r["amount"] for r in rows))
        assert got[c]["users"] == len({r["user"] for r in rows})
    # A computed key, so the input is wrapped first.
    got = o.with_columns(big=col("amount") > 50).group_by("big").agg(n=strata.len(), avg=col("amount").mean()).to_dicts()
    assert {r["big"]: r["n"] for r in got} == dict(Counter(r["amount"] > 50 for r in ORDERS))
    # Months of a timestamp: the key comes back a timestamp from either store.
    t = o.group_by(month=col("ts").dt.truncate("month")).agg(n=strata.len(), p50=col("amount").median()).sort("month").to_arrow()
    assert pa.types.is_timestamp(t.schema.field("month").type)
    months = Counter(r["ts"].replace(day=1, hour=0) for r in ORDERS)
    assert [(r["month"], r["n"]) for r in t.to_pylist()] == sorted(months.items())
    # A filter after the groups is HAVING.
    sums = defaultdict(float)
    for r in ORDERS:
        sums[r["user"]] += r["amount"]
    top = sorted(sums.values())
    cut = (top[-5] + top[-6]) / 2  # between two sums, clear of rounding
    got = o.group_by("user").agg(s=col("amount").sum()).filter(col("s") >= cut).sort("s", descending=True).to_dicts()
    assert [r["user"] for r in got] == sorted((u for u in sums if sums[u] >= cut), key=lambda u: -sums[u])
    total = o.filter(country="cl").select(n=strata.len(), s=col("amount").sum(), first=col("ts").min()).to_dicts()[0]
    cl = [r for r in ORDERS if r["country"] == "cl"]
    assert total["n"] == len(cl) and total["s"] == pytest.approx(sum(r["amount"] for r in cl)) and total["first"] == min(r["ts"] for r in cl)
    assert o.select(col("amount").max()).item() == max(r["amount"] for r in ORDERS)


def test_joins(shop):
    o, u = shop["orders"], shop["users"]
    plan = {r["id"]: r["plan"] for r in USERS}
    got = o.join(u, left_on="user", right_on="id").filter(col("plan") == "pro").select("id", "amount", "plan", "age").to_dicts()
    assert sorted(r["id"] for r in got) == sorted(r["id"] for r in ORDERS if plan.get(r["user"]) == "pro")
    assert o.join(u, left_on="user", right_on="id").count() == sum(r["user"] in plan for r in ORDERS)
    left = o.join(u, left_on="user", right_on="id", how="left").select("id", "user", "plan").to_dicts()
    assert len(left) == N and all((r["plan"] is None) == (r["user"] not in plan) for r in left)
    got = {r["plan"]: r for r in o.join(u.filter(col("age") > 30), left_on="user", right_on="id").group_by("plan").agg(rev=col("amount").sum(), n=strata.len()).to_dicts()}
    old = {r["id"]: r["plan"] for r in USERS if r["age"] > 30}
    for p in ("pro", "free"):
        rows = [r for r in ORDERS if old.get(r["user"]) == p]
        assert got[p]["n"] == len(rows) and got[p]["rev"] == pytest.approx(sum(r["amount"] for r in rows))


def test_search_and_rank(shop):
    o = shop["orders"]
    hits = o.search("refund damaged", on="note", k=5).select("id", "note", "score").to_dicts()
    assert len(hits) == 5 and all(h["note"] == "refund for damaged item" for h in hits)
    assert [h["score"] for h in hits] == sorted((h["score"] for h in hits), reverse=True)
    # Filters before a search narrow what it ranks; after, they narrow its hits.
    hits = o.filter(country="cl").search("refund", on="note", k=10).to_dicts()
    assert len(hits) == 10 and all(h["country"] == "cl" and "refund" in h["note"] for h in hits)
    hits = o.search("refund", on="note", k=50).filter(col("amount") > 90).to_dicts()
    assert hits and len(hits) <= 50 and all(h["amount"] > 90 and "refund" in h["note"] for h in hits)
    assert o.search("refund", on="note", k=30).count() == 30
    q = [0.2, 0.9, 0.5, 1.0]

    def dot(a):  # a vector field's default metric
        return sum(x * y for x, y in zip(a, q))

    near = o.near(q, on="emb", k=3).select("id", "score").to_dicts()
    assert [r["id"] for r in near] == [r["id"] for r in sorted(ORDERS, key=lambda r: -dot(r["emb"]))[:3]]
    assert near[0]["score"] == pytest.approx(dot(ORDERS[int(near[0]["id"][1:])]["emb"]), rel=1e-5)
    hybrid = o.rank_by(0.6 * strata.minmax(strata.bm25("note", "refund")) + 0.4 * strata.minmax(strata.similarity("emb", q)), k=20).max_per(1, "user").to_dicts()
    assert hybrid and len({r["user"] for r in hybrid}) == len(hybrid)
    again = o.search("refund", on="note", k=50).rerank(col("amount"), k=3).to_dicts()
    assert len(again) == 3 and [r["score"] for r in again] == sorted((r["amount"] for r in again), reverse=True)
    assert "orders" in o.filter(country="cl").explain()


def test_unique_windows_expressions(shop):
    o, u = shop["orders"], shop["users"]
    assert sorted(r["country"] for r in o.select("country").unique().to_dicts()) == ["cl", "de", "us"]
    best = {}
    for r in ORDERS:
        if r["user"] not in best or r["amount"] > best[r["user"]]["amount"]:
            best[r["user"]] = r
    assert sorted(ids(o.sort("amount", descending=True).unique("user", keep="first"))) == sorted(r["id"] for r in best.values())
    rows = o.select("id", "user", "amount", r=col("amount").rank(descending=True).over("user"), cs=col("amount").cum_sum().over("user", order_by="ts"), prev=col("amount").shift(1).over("user", order_by="ts")).to_dicts()
    by_user = defaultdict(list)
    for r in ORDERS:
        by_user[r["user"]].append(r)
    want = {}
    for rs in by_user.values():
        amounts = sorted((r["amount"] for r in rs), reverse=True)
        run, prev = 0.0, None
        for r in sorted(rs, key=lambda r: r["ts"]):
            run += r["amount"]
            want[r["id"]] = (amounts.index(r["amount"]) + 1, run, prev)
            prev = r["amount"]
    assert len(rows) == N
    for r in rows:
        rank, run, prev = want[r["id"]]
        assert r["r"] == rank and r["cs"] == pytest.approx(run) and r["prev"] == prev
    got = o.select("id", c=col("country").str.upper(), y=col("ts").dt.year(), m=col("ts").dt.strftime("%Y-%m"), band=strata.when(col("amount") > 50).then("hi").otherwise("lo")).filter(col("note").str.contains("broken")).to_dicts()
    want = {r["id"]: r for r in ORDERS if "broken" in r["note"]}
    assert len(got) == len(want)
    for r in got:
        w = want[r["id"]]
        assert (r["c"], r["y"], r["m"], r["band"]) == (w["country"].upper(), w["ts"].year, w["ts"].strftime("%Y-%m"), "hi" if w["amount"] > 50 else "lo")
    both = strata.concat([o.filter(country="cl").select("id", "user"), u.select("id", user="plan")])
    assert both.count() == sum(r["country"] == "cl" for r in ORDERS) + len(USERS)


def test_outputs(shop):
    o = shop["orders"]
    f = o.filter(country="cl").select("id", "amount", "emb").sort("amount").head(4)
    assert f.select("emb").to_numpy().shape == (4, 4)
    assert pa.table(f).num_rows == 4  # the Arrow C stream
    assert sum(b.num_rows for b in o.iter_batches(batch_size=100)) == N
    pd = pytest.importorskip("pandas")
    df = f.to_pandas()
    assert isinstance(df, pd.DataFrame) and list(df.columns) == ["id", "amount", "emb"] and len(df) == 4
    pl = pytest.importorskip("polars")
    assert pl.DataFrame(f).shape == (4, 3) and f.to_polars()["amount"].to_list() == df["amount"].to_list()
    assert "id" in repr(f)


def test_writes(shop, tmp_path):
    o = shop["orders"]
    assert o.get("o0005")["amount"] == ORDERS[5]["amount"]
    assert o.filter(col("id") == "o0005").update(amount=col("amount") + 1000, status="void") == 1
    assert o.get("o0005")["amount"] == ORDERS[5]["amount"] + 1000 and o.get("o0005")["status"] == "void"
    assert o.filter(col("id").is_in(["o0006", "o0007"])).delete() == 2 and o.count() == N - 2
    with pytest.raises(strata.StrataError, match="all=True"):
        o.delete()
    with pytest.raises(strata.StrataError):
        o.filter(country="cl").upsert([order(1)])
    assert o.upsert([order(6)]) == 1 and o.count() == N - 1
    churn = shop.create_table("churn", o.select("user").unique().with_columns(p=strata.lit(0.5)), key="user")
    assert churn.count() == 40 and shop["churn"].get("u1")["p"] == 0.5
    with pytest.raises(strata.StrataError, match="exists"):
        shop.create_table("churn", [{"user": "u1", "p": 0.1}], key="user")
    shop.create_table("churn", [{"user": "u1", "p": 0.9}, {"user": "zz", "p": 0.1}], key="user", if_exists="append")
    assert shop["churn"].count() == 41 and shop["churn"].get("u1")["p"] == 0.9
    shop.create_table("churn", [{"user": "u1", "p": 0.3}], key="user", if_exists="replace")
    assert shop["churn"].to_dicts() == [{"user": "u1", "p": 0.3}]
    # To another store and back: a file from either.
    copy = strata.connect(tmp_path / "copy.strata")
    cl = o.filter(country="cl").select("id", "user", "amount").write_to(copy, "orders_cl")
    assert cl.count() == sum(r["country"] == "cl" for r in ORDERS) and copy.tables()[0]["key"] == "id"
    assert cl.filter(col("amount") > 50).write_to(shop, "big").count() == cl.filter(col("amount") > 50).count()
    with pytest.raises(strata.StrataError, match="one store"):
        o.join(cl, on="id")
    copy.close()


def test_pending_store(tmp_path):
    s = strata.connect(tmp_path / "new.strata")
    assert "no tables" in repr(s) or "new.strata" in repr(s)
    assert s.tables() == []
    with pytest.raises(strata.StrataError):
        s["t"]
    t = s.create_table("t", {"id": "a", "n": 1})
    assert t.to_dicts() == [{"id": "a", "n": 1}] and (tmp_path / "new.strata").exists()
    s.close()
    assert strata.connect(f"file:{tmp_path / 'new.strata'}")["t"].count() == 1


@pytest.mark.parametrize("where", ["file", "server"])
def test_near_embeds_text(where, tmp_path, request):
    """A text query of a vector field with a model is embedded where the
    query runs."""
    from conftest import BOOKS, book

    path = tmp_path / "books.strata"
    with strata.Database.create(path, BOOKS) as db:
        db.insert([book(i) for i in range(200)])
    store = strata.connect(path if where == "file" else request.getfixturevalue("serve")(path))
    hits = store["books"].near("dragon river", on="emb", k=5).select("id", "score").to_dicts()
    assert len(hits) == 5 and [h["score"] for h in hits] == sorted((h["score"] for h in hits), reverse=True)
    again = store["books"].near(store.embed("dragon river", "emb"), on="emb", k=5).to_dicts()
    assert [h["score"] for h in again] == pytest.approx([h["score"] for h in hits])
    store.close()


def test_server_answers_arrow(tmp_path, serve):
    """A server's answers come as Arrow, typed as the file's own are; a
    server that answers JSON alone still gives frames typed by the schema."""
    path = tmp_path / "shop.strata"
    s = strata.connect(path)
    s.create_table("orders", orders_arrow(), key="id", text=["note"])
    sqls = [
        "SELECT id, amount, ts, emb FROM orders WHERE country = 'cl' ORDER BY amount LIMIT 20",
        "SELECT id, note, score() FROM orders ORDER BY bm25(note, 'refund damaged') DESC LIMIT 5",
        "SELECT country, COUNT(*) AS n, SUM(amount) AS s, MAX(ts) AS last FROM orders GROUP BY country ORDER BY country",
        "SELECT id FROM orders WHERE amount < 0",
    ]
    local = {q: pa.table(s.query_arrow(q)) for q in sqls}
    s.close()
    c = strata.connect(serve(path))
    for q in sqls:
        got = c.query_arrow(q)
        assert isinstance(got, pa.Table)
        assert got.schema == local[q].schema, q
        assert got.to_pylist() == local[q].to_pylist(), q
    f = c["orders"].filter(country="cl").select("id", "amount", "ts").sort("amount").head(3)
    want = f.to_dicts()
    c._json_only = True  # as a server from before Arrow answers
    assert f.to_dicts() == want and f.to_arrow().schema == pa.table(local[sqls[0]]).select(["id", "amount", "ts"]).schema


def test_server_result_window(tmp_path, serve):
    """A frame that reads more rows than a server's result window says
    which flag lets it."""
    path = tmp_path / "shop.strata"
    s = strata.connect(path)
    s.create_table("orders", orders_arrow(), key="id")
    s.close()
    c = strata.connect(serve(path, "--max-result-window", "100"))
    assert c["orders"].head(100).count() == 100 and len(c["orders"].head(100).to_dicts()) == 100
    with pytest.raises(strata.StrataError, match="max-result-window"):
        c["orders"].to_arrow()
    with pytest.raises(strata.StrataError, match="max-result-window"):
        c["orders"].head(500).to_arrow()
