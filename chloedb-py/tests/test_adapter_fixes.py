"""The framework adapters and the DB-API driver at their edges (the second
audit's N-H findings): reads that see their own writes through a
coordinator, a LangGraph thread kept on one shard, shared databases and
their lifetimes, reads past a server's result window, values, relevance
scores, empty id lists, namespaces past the most groups, filters on values
too big to index, and the client's kept-alive connections."""

import datetime
import math
import subprocess
import time
import urllib.request
import warnings

import pytest

import chloedb
import chloedb.dbapi as dbapi
from adapters_common import DIMS, target, words_vector  # noqa: F401
from conftest import free_port, server_binary


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
def cluster(tmp_path_factory):
    """A coordinator over two shards that number their writes: its URL and
    the shards' URLs."""
    binary = server_binary()
    if not binary:
        pytest.skip("chloedb-server is not built")
    procs, shards, grpcs = [], [], []

    def start(*args):
        p = subprocess.Popen([binary, *args], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        procs.append(p)
        return p

    for i in range(2):
        http, grpc = free_port(), free_port()
        url = f"http://127.0.0.1:{http}"
        _wait(url, start("--db", str(tmp_path_factory.mktemp(f"shard{i}") / "db.chloedb"), "--listen", f"127.0.0.1:{http}", "--grpc", f"127.0.0.1:{grpc}", "--seq-no"))
        shards.append(url)
        grpcs.append(f"http://127.0.0.1:{grpc}")
    port = free_port()
    coord = f"http://127.0.0.1:{port}"
    _wait(coord, start("--shards", ",".join(grpcs), "--listen", f"127.0.0.1:{port}", "--grpc", "off"))
    for _ in range(50):
        try:
            chloedb.connect(coord).sql("CREATE TABLE first (id KEYWORD PRIMARY KEY)")
            break
        except chloedb.ChloeDBError:
            time.sleep(0.2)  # the shards not yet all known
    yield coord, shards
    for p in procs:
        p.terminate()
    for p in procs:
        p.wait(timeout=10)


# ---------------------------------------------- N-H1: a Session per adapter


def test_adapters_read_through_a_session(cluster):
    from chloedb.langgraph import ChloeDBSaver, ChloeDBStore

    coord, _ = cluster
    saver = ChloeDBSaver(coord, table="cp_session", writes_table="w_session")
    assert isinstance(saver.store, chloedb.Session)
    # A Client given becomes its Session; a Session given is used as it is.
    s = chloedb.connect(coord).session()
    assert ChloeDBStore(s, table="st_session").store is s
    assert isinstance(ChloeDBStore(chloedb.connect(coord), table="st_session").store, chloedb.Session)
    assert isinstance(dbapi.connect(coord).store, chloedb.Session)

    from langgraph.checkpoint.base import empty_checkpoint

    c = empty_checkpoint()
    saver.put({"configurable": {"thread_id": "s1", "checkpoint_ns": ""}}, c, {"step": 1}, {})
    # The write's numbers go with the reads that follow it.
    assert saver.store.seq_nos is not None and any(saver.store.seq_nos)
    assert saver.get_tuple({"configurable": {"thread_id": "s1"}}).checkpoint["id"] == c["id"]
    # A number past what a shard holds is refused: the reads send them.
    saver.store.seq_nos = [n + 10_000 for n in saver.store.seq_nos]
    with pytest.raises(chloedb.ChloeDBError):
        saver.get_tuple({"configurable": {"thread_id": "s1"}})


# ------------------------------------- N-H2: a LangGraph thread on one shard


def test_checkpoint_tables_are_placed_by_thread(target):
    from chloedb.langgraph import ChloeDBSaver

    saver = ChloeDBSaver(target)
    saver.setup()
    tables = {t["table"]: t for t in saver.store.tables()}
    for name in ("langgraph_checkpoints", "langgraph_writes"):
        assert tables[name].get("shard_by") == "thread_id"
        assert tables[name].get("cluster_by") == "thread_id"


def test_a_thread_lives_on_one_shard(cluster):
    from langgraph.checkpoint.base import empty_checkpoint

    from chloedb.langgraph import ChloeDBSaver

    coord, shards = cluster
    saver = ChloeDBSaver(coord, table="cp_placed", writes_table="w_placed")
    for t in range(6):
        config = {"configurable": {"thread_id": f"t{t}", "checkpoint_ns": ""}}
        for step in range(4):
            c = empty_checkpoint()
            config = saver.put(config, c, {"step": step}, {})
            saver.put_writes(config, [("a", step), ("b", step), ("c", step)], task_id=f"task{step}")
    for t in range(6):
        thread = f"t{t}"
        counts = [chloedb.connect(u).query("SELECT COUNT(*) FROM cp_placed WHERE thread_id = :t", {"t": thread}).scalar() for u in shards]
        writes = [chloedb.connect(u).query("SELECT COUNT(*) FROM w_placed WHERE thread_id = :t", {"t": thread}).scalar() for u in shards]
        assert sorted(counts) == [0, 4], (thread, counts)
        assert sorted(writes) == [0, 12], (thread, writes)
        assert counts.index(4) == writes.index(12)
        got = saver.get_tuple({"configurable": {"thread_id": thread}})
        assert got.metadata["step"] == 3
        assert [w[1] for w in got.pending_writes] == ["a", "b", "c"]
    assert len(list(saver.list({"configurable": {"thread_id": "t2"}}))) == 4


# --------------------------- N-H3, N-H4: shared databases and `with conn:`


def test_closing_a_connection_leaves_the_others_open(tmp_path):
    from langgraph.checkpoint.base import empty_checkpoint

    from chloedb.langgraph import ChloeDBSaver

    path = str(tmp_path / "agent.chloedb")
    saver = ChloeDBSaver(path)
    config = saver.put({"configurable": {"thread_id": "t", "checkpoint_ns": ""}}, empty_checkpoint(), {"step": 0}, {})
    with dbapi.connect(path) as c:
        assert c.execute("SELECT COUNT(*) FROM langgraph_checkpoints").fetchone() == (1,)
    c.close()
    # The saver's database is still open.
    saver.put(config, empty_checkpoint(), {"step": 1}, {})
    a, b = dbapi.connect(path), dbapi.connect(path)
    a.close()
    assert b.execute("SELECT COUNT(*) FROM langgraph_checkpoints").fetchone() == (2,)
    b.close()
    assert len(list(saver.list(None))) == 2


def test_the_last_connection_closes_the_database(tmp_path):
    path = str(tmp_path / "only.chloedb")
    a = dbapi.connect(path)
    a.execute("CREATE TABLE t (id KEYWORD PRIMARY KEY, n INT)")
    b = dbapi.connect(path)
    store = a.store
    assert b.store is store
    a.close()
    assert not store.closed
    b.close()
    assert store.closed
    # Opened again afresh.
    with dbapi.connect(path) as c:
        assert c.execute("SELECT COUNT(*) FROM t").fetchone() == (0,)


def test_with_commits_and_leaves_the_connection_open(target):
    conn = dbapi.connect(target)
    conn.execute("CREATE TABLE w (id KEYWORD PRIMARY KEY, n INT)")
    with conn:
        conn.execute("INSERT INTO w (id, n) VALUES ('a', 1)")
    with conn:
        conn.execute("INSERT INTO w (id, n) VALUES ('b', 2)")
    assert not conn.closed
    assert conn.execute("SELECT COUNT(*) FROM w").fetchone() == (2,)
    import contextlib

    with contextlib.closing(conn):
        pass
    assert conn.closed


# ------------------------------------ N-H5: reads past the result window


def test_reads_past_the_result_window(serve, tmp_path):
    path = tmp_path / "window.chloedb"
    with chloedb.Database.create(path, {"table": "ev", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "n", "type": "int"}, {"name": "body", "type": "text"}]}) as db:
        db.insert([{"id": f"e{i:04d}", "n": i, "body": "river stone" if i % 2 else "river"} for i in range(250)])
    url = serve(path, "--max-result-window", "40")
    conn = dbapi.connect(url)
    old = dbapi.PAGE_ROWS
    dbapi.PAGE_ROWS = 40
    try:
        cur = conn.execute("SELECT id, n FROM ev ORDER BY n")
        assert cur.rowcount == -1  # more pages to come
        assert cur.fetchone() == ("e0000", 0)
        assert len(cur.fetchmany(50)) == 50
        rest = cur.fetchall()
        assert len(rest) == 199 and rest[-1] == ("e0249", 249)
        assert cur.rowcount == 250
        # pandas reads every row, as from a file.
        pd = pytest.importorskip("pandas")
        with warnings.catch_warnings():
            warnings.simplefilter("ignore")
            df = pd.read_sql("SELECT id, n FROM ev", conn)
        assert len(df) == 250
        # An answer of one page knows its count at once.
        cur.execute("SELECT id FROM ev WHERE n < 5")
        assert cur.rowcount == 5
        # A cursor left partway is ended by the next statement.
        cur.execute("SELECT id FROM ev")
        cur.fetchone()
        cur.execute("SELECT COUNT(*) FROM ev")
        assert cur.fetchall() == [(250,)]
    finally:
        dbapi.PAGE_ROWS = old


def test_a_ranked_read_cut_short_says_so(target):
    conn = dbapi.connect(target)
    conn.execute("CREATE TABLE docs (id KEYWORD PRIMARY KEY, body TEXT)")
    cur = conn.cursor()
    cur.executemany("INSERT INTO docs (id, body) VALUES (:id, :b)", [{"id": f"d{i:03d}", "b": "river"} for i in range(130)])
    with pytest.warns(UserWarning, match="best 100"):
        cur.execute("SELECT id FROM docs ORDER BY bm25(body, :q) DESC", {"q": "river"})
    assert len(cur.fetchall()) == 100
    assert cur.messages and cur.messages[0][0] is dbapi.Warning
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        cur.execute("SELECT id FROM docs ORDER BY bm25(body, :q) DESC LIMIT 120", {"q": "river"})
    assert len(cur.fetchall()) == 120 and cur.messages == []


# -------------------------------------------------------- N-H6: values


def test_values(target):
    conn = dbapi.connect(target)
    conn.execute("CREATE TABLE v (id KEYWORD PRIMARY KEY, at TIMESTAMP, x FLOAT)")
    cur = conn.cursor()
    at = datetime.datetime(2026, 11, 13, 7, 30, 15, 250000, tzinfo=datetime.timezone.utc)
    cur.execute("INSERT INTO v (id, at, x) VALUES ('a', :at, :x)", {"at": at, "x": float("nan")})
    cur.execute("INSERT INTO v (id, at, x) VALUES ('b', :at, :x)", {"at": datetime.datetime(2026, 1, 2), "x": 1.5})
    cur.execute("SELECT id, at, x FROM v ORDER BY id")
    assert cur.description[1][1] == dbapi.DATETIME
    rows = cur.fetchall()
    # NaN is stored as NULL in a file and through a server alike.
    assert rows[0] == ("a", at, None)
    assert rows[1] == ("b", datetime.datetime(2026, 1, 2, tzinfo=datetime.timezone.utc), 1.5)
    with pytest.raises(dbapi.NotSupportedError):
        dbapi.Binary(b"\x00\x01")
    with pytest.raises(dbapi.NotSupportedError):
        cur.execute("INSERT INTO v (id) VALUES (:id)", {"id": b"raw"})


def test_the_client_sends_nan_as_null(serve, tmp_path):
    path = tmp_path / "nan.chloedb"
    with chloedb.Database.create(path, {"table": "t", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "x", "type": "float"}, {"name": "v", "type": "vector", "dim": 2}]}):
        pass
    c = chloedb.connect(serve(path))
    c.upsert([{"id": "a", "x": float("nan"), "v": [0.5, 0.5]}, {"id": "b", "x": math.inf, "v": None}, {"id": "c", "x": 2.0}])
    assert c.query("SELECT id, x FROM t ORDER BY id").rows == [["a", None], ["b", None], ["c", 2.0]]
    # Text that reads NaN is text.
    from chloedb.client import _json_body

    assert _json_body({"s": "NaN", "x": float("nan"), "v": [1.0, float("-inf")]}) == b'{"s": "NaN", "x": null, "v": [1.0, null]}'


def test_column_types_are_read_once(target):
    conn = dbapi.connect(target)
    conn.execute("CREATE TABLE a (id KEYWORD PRIMARY KEY, n INT)")
    calls = []
    tables = conn.store.tables

    def counted():
        calls.append(1)
        return tables()

    conn.store.tables = counted
    try:
        for _ in range(3):
            conn.execute("SELECT id, n FROM a")
        assert len(calls) == 1
        conn.execute("CREATE TABLE b (id KEYWORD PRIMARY KEY, at TIMESTAMP)")
        assert conn.execute("SELECT id, at FROM b").description[1][1] == dbapi.DATETIME
        assert len(calls) == 2
    finally:
        del conn.store.tables


# ------------------------------------------- N-H7: LangChain's relevance


def test_relevance_scores_follow_the_search(target):
    from langchain_core.embeddings import Embeddings

    from chloedb.langchain import ChloeDBVectorStore

    class Words(Embeddings):
        def embed_documents(self, texts):
            return [words_vector(t) for t in texts]

        def embed_query(self, text):
            return words_vector(text)

    store = ChloeDBVectorStore(target, embedding=Words(), table="relevance")
    store.add_texts(["red river stones", "blue mountain lake", "red river red stones"], ids=["a", "b", "c"])
    by_score = dict((d.id, s) for d, s in store.similarity_search_with_score("red river", k=3))
    by_relevance = dict((d.id, s) for d, s in store.similarity_search_with_relevance_scores("red river", k=3))
    # By vector: the cosine itself (as Chroma, FAISS and PGVector give it).
    for i, s in by_score.items():
        assert by_relevance[i] == pytest.approx(max(0.0, s))
    # Hybrid on a vector store: by the call's mode, over the fused best.
    hybrid = store.similarity_search_with_relevance_scores("red river", k=3, mode="hybrid")
    assert all(0.0 <= s <= 1.0 for _, s in hybrid)
    assert hybrid[0][1] > 0.9 and hybrid[-1][1] < hybrid[0][1]
    kept = store.as_retriever(search_type="similarity_score_threshold", search_kwargs={"score_threshold": 0.9, "mode": "hybrid", "k": 3}).invoke("red river")
    assert 1 <= len(kept) < 3
    with pytest.raises(NotImplementedError):
        store.similarity_search_with_relevance_scores("red river", k=3, mode="text")


# --------------------------------------- N-H8: LlamaIndex's empty id lists


def test_an_empty_id_list_names_no_node(target):
    from llama_index.core.schema import TextNode
    from llama_index.core.vector_stores.types import MetadataFilter, MetadataFilters

    from chloedb.llama_index import ChloeDBVectorStore

    store = ChloeDBVectorStore(target, table="empty_ids", dim=DIMS)
    store.add([TextNode(id_=f"n{i}", text=f"node {i}", metadata={"tenant": "x"}, embedding=words_vector(f"node {i}")) for i in range(4)])
    tenant = MetadataFilters(filters=[MetadataFilter(key="tenant", value="x")])
    assert store.get_nodes(node_ids=[]) == []
    assert store.get_nodes(node_ids=[], filters=tenant) == []
    store.delete_nodes(node_ids=[], filters=tenant)
    store.delete_nodes(filters=MetadataFilters(filters=[]))
    store.delete_nodes(node_ids=[])
    assert len(store.get_nodes(filters=tenant)) == 4
    store.delete_nodes(node_ids=["n0"], filters=tenant)
    assert sorted(n.node_id for n in store.get_nodes(filters=tenant)) == ["n1", "n2", "n3"]


# ------------------------------------- N-H9: namespaces past --max-groups


def test_namespaces_past_the_most_groups(serve, tmp_path):
    from chloedb.langgraph import ChloeDBStore

    path = tmp_path / "groups.chloedb"
    with chloedb.Database.create(path, {"table": "first", "key": "id", "fields": [{"name": "id", "type": "keyword"}]}):
        pass
    store = ChloeDBStore(serve(path, "--max-groups", "5"))
    for u in range(12):
        store.put(("users", f"u{u:02d}", "memories"), "k", {"n": u})
        store.put(("users", f"u{u:02d}", "memories"), "k2", {"n": u})
    store.put(("other",), "k", {"n": 0})
    names = store.list_namespaces(prefix=("users",), limit=100)
    assert names == [("users", f"u{u:02d}", "memories") for u in range(12)]
    assert store.list_namespaces(prefix=("users",), limit=3, offset=2) == [("users", f"u{u:02d}", "memories") for u in (2, 3, 4)]
    assert store.list_namespaces(max_depth=1) == [("other",), ("users",)]


# --------------------- N-H10: filters on what of a value fits a JSON field


def test_filters_index_what_fits(target):
    from langgraph.checkpoint.base import empty_checkpoint

    from chloedb._adapters import json_fits, json_pruned
    from chloedb.langgraph import ChloeDBSaver, ChloeDBStore

    big = {"kind": "transcript", "messages": [{"role": "user", "text": f"m{i}"} for i in range(1200)], "meta": {"lang": "en", "deep": {"a": list(range(2000))}}}
    pruned = json_pruned(big)
    assert json_fits(pruned)
    assert pruned["kind"] == "transcript" and pruned["meta"] == {"lang": "en"} and "messages" not in pruned
    assert json_pruned({"x": 1}) == {"x": 1} and json_pruned({}) is None
    deep = {"a": {"b": {"c": {"d": {"e": {"f": {"g": {"h": {"i": {"j": {"k": {"l": {"m": {"n": {"o": {"p": {"q": 1}}}}}}}}}}}}}}}}, "top": 1}
    assert json_pruned(deep)["top"] == 1 and json_fits(json_pruned(deep))

    store = ChloeDBStore(target, table="big_values")
    store.put(("t",), "long", big)
    store.put(("t",), "short", {"kind": "note"})
    assert [i.key for i in store.search(("t",), filter={"kind": "transcript"})] == ["long"]
    assert store.get(("t",), "long").value == big
    saver = ChloeDBSaver(target, table="cp_big", writes_table="w_big")
    config = {"configurable": {"thread_id": "b", "checkpoint_ns": ""}}
    saver.put(config, empty_checkpoint(), {"source": "input", "step": 0, "notes": ["n"] * 1500}, {})
    assert len(list(saver.list(None, filter={"source": "input"}))) == 1


# ---------------------------------- N-H13: no database-wide toggle around a write


def test_items_without_text_need_no_model(tmp_path):
    from chloedb.langgraph import ChloeDBStore

    db = chloedb.connect(str(tmp_path / "nomodel.chloedb"))
    # potion-retrieval-32M is a model the field knows and this process has
    # not opened (no models directory).
    store = ChloeDBStore(db, index={"model": "potion-retrieval-32M", "fields": ["text"]}, table="nomodel")
    store.put(("m",), "plain", {"other": "nothing to embed"})
    assert store.get(("m",), "plain").value == {"other": "nothing to embed"}
    with pytest.raises(chloedb.ChloeDBError, match="not open"):
        store.put(("m",), "text", {"text": "to embed"})
    # A setting the caller turned on stays on.
    db.set_embed_later(True)
    store.put(("m",), "plain2", {"other": "x"})
    store.put(("m",), "text2", {"text": "kept without its vector"})
    assert store.get(("m",), "text2") is not None


# ----------------------------------- N-H11: connections kept alive


def test_the_client_keeps_its_connection(serve, tmp_path):
    path = tmp_path / "alive.chloedb"
    with chloedb.Database.create(path, {"table": "t", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "n", "type": "int"}]}):
        pass
    c = chloedb.connect(serve(path))
    c.upsert({"id": "a", "n": 1})
    sock = c._conns.kept.conn.sock
    for i in range(20):
        c.upsert({"id": f"k{i}", "n": i})
        assert c.get(f"k{i}")["n"] == i
    assert c._conns.kept.conn.sock is sock
    # An error answer keeps it too.
    with pytest.raises(chloedb.ChloeDBError):
        c.query("SELEC nonsense")
    assert c._conns.kept.conn.sock is sock
    # A connection the server closed is replaced, and the request sent.
    import socket

    c._conns.kept.conn.sock.shutdown(socket.SHUT_RDWR)
    assert c.query("SELECT COUNT(*) FROM t").scalar() == 21
    # Threads each have their own.
    import threading

    socks = []

    def work():
        c.query("SELECT COUNT(*) FROM t")
        socks.append(c._conns.kept.conn.sock)

    ts = [threading.Thread(target=work) for _ in range(3)]
    for t in ts:
        t.start()
    for t in ts:
        t.join()
    assert len({id(s) for s in socks}) == 3
    # A pickled client opens its own.
    import pickle

    again = pickle.loads(pickle.dumps(c.session()))
    assert again.query("SELECT COUNT(*) FROM t").scalar() == 21


def test_a_server_that_is_down_is_a_url_error():
    import urllib.error

    c = chloedb.connect(f"http://127.0.0.1:{free_port()}", retry_for=0)
    with pytest.raises(urllib.error.URLError):
        c.health()
