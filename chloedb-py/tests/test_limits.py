"""A Database's limits on its queries: a memory budget (the process's by
default, the database's or a query's own), a deadline (`timeout_ms`, on the
database or the query), and `interrupt()` from another thread."""

import threading
import time

import pytest

import chloedb

pytestmark = pytest.mark.skipif(not chloedb._native.NATIVE, reason="the extension is not built")

SCHEMA = {
    "table": "t",
    "key": "id",
    "fields": [
        {"name": "id", "type": "int"},
        {"name": "a", "type": "int"},
        {"name": "c", "type": "int"},
        {"name": "note", "type": "text"},
    ],
}

# A group per row, and every row with its text: each holds a few MiB.
HEAVY = ["SELECT a, COUNT(*) FROM t GROUP BY a LIMIT 5", "SELECT * FROM t"]

# Every row meets the 8,000 rows of its `c` and none passes the rest of the
# condition: seconds of comparisons on ChloeDB's own operators, holding
# next to nothing.
SLOW = "SELECT x.id, y.id FROM t x JOIN t y ON x.c = y.c AND x.a < y.a - 1000000"


def rows(n):
    return [{"id": i, "a": i * 7919 % n, "c": i % 3, "note": f"{i:08} " * 20} for i in range(n)]


@pytest.fixture
def big(tmp_path):
    with chloedb.Database.create(tmp_path / "big.strata", SCHEMA) as d:
        d.insert(rows(50_000))
        yield d


@pytest.fixture
def slow(tmp_path, monkeypatch):
    # DataFusion answers the join at once; the operators take seconds.
    monkeypatch.setenv("CHLOEDB_ENGINE", "operators")
    with chloedb.Database.create(tmp_path / "slow.strata", SCHEMA, memory=0) as d:
        d.insert(rows(24_000))
        yield d


def test_queries_share_the_process_budget_by_default(big):
    # A quarter of the machine's memory, unless CHLOEDB_QUERY_MEMORY_MB.
    assert big.memory is not None and big.memory >= 1 << 20
    assert big.timeout_ms is None
    for sql in HEAVY:
        big.query(sql)


def test_a_query_past_its_memory_fails_and_names_the_limit(big):
    for sql in HEAVY:
        with pytest.raises(chloedb.ChloeDBError, match="this query may hold") as e:
            big.query(sql, memory=256 << 10)
        assert e.value.code == "memory"
        assert "memory option" in str(e.value)
        # 0: no budget.
        assert len(big.query(sql, memory=0).rows) > 0
        for run in (lambda: big.sql(sql, memory=256 << 10), lambda: big.query_arrow(sql, memory=256 << 10), lambda: list(big.scroll(sql, memory=256 << 10))):
            with pytest.raises(chloedb.ChloeDBError, match="this query may hold"):
                run()
    with pytest.raises(chloedb.ChloeDBError, match="memory"):
        big.run(big.plan(HEAVY[0]), memory=256 << 10)
    with pytest.raises(chloedb.ChloeDBError, match="memory"):
        big.search("00000001", memory=1)
    with pytest.raises(ValueError):
        big.query("SELECT 1 FROM t", memory=-1)


def test_a_database_budget_its_queries_share(tmp_path):
    with chloedb.Database.create(tmp_path / "d.strata", SCHEMA, memory=256 << 10) as d:
        d.insert(rows(50_000))
        assert d.memory == 256 << 10
        with pytest.raises(chloedb.ChloeDBError, match="this database's queries may hold") as e:
            d.query(HEAVY[0])
        assert e.value.code == "memory"
        # A query's own budget over the database's.
        assert len(d.query(HEAVY[0], memory=0).rows) == 5
        d.set_limits(memory=0)
        assert d.memory is None
        assert len(d.query(HEAVY[0]).rows) == 5
        d.set_limits()
        assert d.memory is not None and d.memory > 256 << 10
    # chloedb.open takes them too.
    with chloedb.open(tmp_path / "d.strata", timeout_ms=1234, memory=1 << 30) as d:
        assert (d.timeout_ms, d.memory) == (1234, 1 << 30)


def test_a_deadline_stops_a_slow_query(slow):
    for run in (
        lambda: slow.query(SLOW, timeout_ms=100),
        lambda: slow.sql(SLOW, timeout_ms=100),
        lambda: slow.query_arrow(SLOW, timeout_ms=100),
        lambda: list(slow.scroll(SLOW, timeout_ms=100)),
        lambda: slow.run(slow.plan(SLOW), timeout_ms=100),
    ):
        t0 = time.monotonic()
        with pytest.raises(chloedb.DeadlineError, match="timeout_ms=100") as e:
            run()
        assert e.value.code == "timeout"
        assert time.monotonic() - t0 < 5
    # The database's deadline, and a query's over it.
    slow.set_limits(timeout_ms=100, memory=0)
    assert slow.timeout_ms == 100
    with pytest.raises(chloedb.DeadlineError, match="timeout_ms=100"):
        slow.query(SLOW)
    with pytest.raises(chloedb.DeadlineError, match="timeout_ms=150"):
        slow.query(SLOW, timeout_ms=150)
    assert slow.query("SELECT COUNT(*) FROM t").rows == [[24_000]]
    # A search takes it as a query does.
    assert slow.search("00000001", timeout_ms=60_000).keys


def test_interrupt_stops_the_queries_running_from_another_thread(slow):
    got = []

    def run():
        try:
            slow.query(SLOW)
            got.append(None)
        except chloedb.ChloeDBError as e:
            got.append(e)

    assert slow.interrupt() == 0
    threads = [threading.Thread(target=run) for _ in range(2)]
    for t in threads:
        t.start()
    # Both are running once the interrupt finds them.
    n, t0 = 0, time.monotonic()
    while n < 2 and time.monotonic() - t0 < 10:
        time.sleep(0.05)
        n += slow.interrupt()
    for t in threads:
        t.join(10)
    assert n == 2
    assert len(got) == 2 and all(isinstance(e, chloedb.DeadlineError) for e in got), got
    assert all("interrupt" in str(e) for e in got)
    assert time.monotonic() - t0 < 10
    # Queries after it run.
    assert slow.query("SELECT COUNT(*) FROM t").rows == [[24_000]]


def test_a_client_takes_no_memory():
    with pytest.raises(TypeError, match="memory"):
        chloedb.Client("http://127.0.0.1:9").search("x", memory=1)
