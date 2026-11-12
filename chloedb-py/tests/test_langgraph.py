import asyncio
import operator
import time
from typing import Annotated, TypedDict

import pytest

pytest.importorskip("langgraph")

from langgraph.graph import END, START, StateGraph
from langgraph.types import Command, interrupt

from adapters_common import target, words_vector  # noqa: F401
from chloedb.langgraph import ChloeDBSaver, ChloeDBStore


class State(TypedDict):
    log: Annotated[list, operator.add]


def graph(saver, store=None):
    def first(state):
        return {"log": ["first"]}

    def second(state):
        return {"log": ["second"]}

    b = StateGraph(State)
    b.add_node("first", first)
    b.add_node("second", second)
    b.add_edge(START, "first")
    b.add_edge("first", "second")
    b.add_edge("second", END)
    return b.compile(checkpointer=saver, store=store)


def cfg(thread):
    return {"configurable": {"thread_id": thread}}


def test_checkpoints_a_thread(target):
    saver = ChloeDBSaver(target)
    g = graph(saver)
    assert g.invoke({"log": ["in"]}, cfg("t1"))["log"] == ["in", "first", "second"]
    # The thread goes on from its state.
    assert g.invoke({"log": ["again"]}, cfg("t1"))["log"] == ["in", "first", "second", "again", "first", "second"]
    assert g.invoke({"log": ["other"]}, cfg("t2"))["log"] == ["other", "first", "second"]
    history = list(g.get_state_history(cfg("t1")))
    assert len(history) == 8
    assert history[0].values["log"][-1] == "second"
    steps = [h.metadata["step"] for h in history]
    assert steps == sorted(steps, reverse=True)
    # Back in time: the state at an older checkpoint, and a fork from it.
    old = history[5]
    assert g.get_state(old.config).values == old.values
    forked = g.invoke(None, old.config)
    assert forked["log"][-1] == "second"


def test_lists_and_filters_checkpoints(target):
    saver = ChloeDBSaver(target)
    g = graph(saver)
    g.invoke({"log": ["in"]}, cfg("t1"))
    g.invoke({"log": ["in"]}, cfg("t2"))
    all_t1 = list(saver.list(cfg("t1")))
    assert len(all_t1) == 4
    ids = [c.config["configurable"]["checkpoint_id"] for c in all_t1]
    assert ids == sorted(ids, reverse=True)
    assert [c.config["configurable"]["checkpoint_id"] for c in saver.list(cfg("t1"), limit=2)] == ids[:2]
    before = list(saver.list(cfg("t1"), before={"configurable": {"checkpoint_id": ids[1]}}))
    assert [c.config["configurable"]["checkpoint_id"] for c in before] == ids[2:]
    inputs = list(saver.list(None, filter={"source": "input"}))
    assert {c.config["configurable"]["thread_id"] for c in inputs} == {"t1", "t2"}
    assert all(c.metadata["source"] == "input" for c in inputs)
    assert [c.metadata["step"] for c in saver.list(cfg("t1"), filter={"step": 1})] == [1]
    # Parents chain back to the first checkpoint.
    latest = saver.get_tuple(cfg("t1"))
    assert latest.config["configurable"]["checkpoint_id"] == ids[0]
    assert latest.parent_config["configurable"]["checkpoint_id"] == ids[1]


def test_interrupts_and_resumes(target):
    def ask(state):
        answer = interrupt("name?")
        return {"log": [f"hello {answer}"]}

    b = StateGraph(State)
    b.add_node("ask", ask)
    b.add_edge(START, "ask")
    b.add_edge("ask", END)
    g = b.compile(checkpointer=ChloeDBSaver(target))
    out = g.invoke({"log": []}, cfg("i1"))
    assert out["__interrupt__"][0].value == "name?"
    state = g.get_state(cfg("i1"))
    assert state.next == ("ask",)
    assert state.tasks[0].interrupts[0].value == "name?"
    # Resumed from another saver, as another process would.
    g2 = b.compile(checkpointer=ChloeDBSaver(g.checkpointer.store))
    assert g2.invoke(Command(resume="Ann"), cfg("i1"))["log"] == ["hello Ann"]


def test_survives_a_restart(tmp_path):
    path = str(tmp_path / "agent.chloedb")
    saver = ChloeDBSaver(path)
    graph(saver).invoke({"log": ["in"]}, cfg("t1"))
    saver.store.close()
    saver = ChloeDBSaver(path)
    assert graph(saver).get_state(cfg("t1")).values["log"] == ["in", "first", "second"]


def test_deletes_and_copies_threads(target):
    saver = ChloeDBSaver(target)
    g = graph(saver)
    g.invoke({"log": ["in"]}, cfg("t1"))
    saver.copy_thread("t1", "t9")
    assert g.get_state(cfg("t9")).values == g.get_state(cfg("t1")).values
    assert len(list(saver.list(cfg("t9")))) == 4
    saver.delete_thread("t1")
    assert saver.get_tuple(cfg("t1")) is None
    assert list(saver.list(cfg("t1"))) == []
    assert g.get_state(cfg("t9")).values["log"] == ["in", "first", "second"]


def test_pending_writes_keep_the_first(target):
    saver = ChloeDBSaver(target)
    g = graph(saver)
    g.invoke({"log": ["in"]}, cfg("t1"))
    c = saver.get_tuple(cfg("t1")).config
    saver.put_writes(c, [("log", ["a"]), ("other", 1)], "task1", "~path")
    saver.put_writes(c, [("log", ["b"])], "task1", "~path")
    saver.put_writes(c, [("__error__", "first")], "task2")
    saver.put_writes(c, [("__error__", "second")], "task2")
    writes = saver.get_tuple(c).pending_writes
    assert ("task1", "log", ["a"]) in writes and ("task1", "log", ["b"]) not in writes
    assert ("task1", "other", 1) in writes
    assert ("task2", "__error__", "second") in writes and ("task2", "__error__", "first") not in writes


def test_async_graph(target):
    g = graph(ChloeDBSaver(target))

    async def go():
        await g.ainvoke({"log": ["in"]}, cfg("a1"))
        return [s async for s in g.aget_state_history(cfg("a1"))]

    history = asyncio.run(go())
    assert history[0].values["log"] == ["in", "first", "second"]


# The store.


from langchain_core.embeddings import Embeddings


class Words(Embeddings):
    def embed_documents(self, texts):
        return [words_vector(t) for t in texts]

    def embed_query(self, text):
        return words_vector(text)

    async def aembed_documents(self, texts):
        return self.embed_documents(texts)

    async def aembed_query(self, text):
        return self.embed_query(text)


def test_store_keeps_items(target):
    store = ChloeDBStore(target)
    store.put(("users", "u1"), "prefs", {"theme": "dark", "size": 12, "tags": ["a"], "nested": {"k": None}, "empty": []})
    store.put(("users", "u1"), "name", {"first": "Ann"})
    store.put(("users", "u2"), "prefs", {"theme": "light", "size": 14})
    store.put(("docs",), "d1", {"title": "a doc"})
    item = store.get(("users", "u1"), "prefs")
    assert item.value == {"theme": "dark", "size": 12, "tags": ["a"], "nested": {"k": None}, "empty": []}
    assert item.namespace == ("users", "u1") and item.key == "prefs"
    created = item.created_at
    time.sleep(0.01)
    store.put(("users", "u1"), "prefs", {"theme": "blue"})
    again = store.get(("users", "u1"), "prefs")
    assert again.value == {"theme": "blue"}
    assert again.created_at == created and again.updated_at > created
    assert store.get(("users", "u1"), "missing") is None
    store.delete(("users", "u1"), "name")
    assert store.get(("users", "u1"), "name") is None


def test_store_searches_and_lists(target):
    store = ChloeDBStore(target)
    for i in range(6):
        store.put(("mem", f"u{i % 2}"), f"k{i}", {"n": i, "kind": "even" if i % 2 == 0 else "odd"})
    store.put(("memories",), "x", {"n": 100})
    found = store.search(("mem",), limit=10)
    assert sorted(i.key for i in found) == [f"k{i}" for i in range(6)]
    # Newest first, then paged.
    assert [i.key for i in store.search(("mem",), limit=2)] == ["k5", "k4"]
    assert [i.key for i in store.search(("mem",), limit=2, offset=2)] == ["k3", "k2"]
    assert sorted(i.key for i in store.search(("mem", "u0"))) == ["k0", "k2", "k4"]
    assert sorted(i.key for i in store.search(("mem",), filter={"kind": "odd"})) == ["k1", "k3", "k5"]
    assert sorted(i.key for i in store.search(("mem",), filter={"n": {"$gte": 3}, "kind": "odd"})) == ["k3", "k5"]
    assert sorted(i.key for i in store.search(("mem",), filter={"n": {"$ne": 0}})) == ["k1", "k2", "k3", "k4", "k5"]
    # `("mem",)` is not a prefix of `("memories",)`.
    assert all(i.namespace[0] == "mem" for i in store.search(("mem",), limit=20))
    assert len(store.search((), limit=20)) == 7
    assert store.list_namespaces() == [("mem", "u0"), ("mem", "u1"), ("memories",)]
    assert store.list_namespaces(prefix=("mem",)) == [("mem", "u0"), ("mem", "u1")]
    assert store.list_namespaces(suffix=("u1",)) == [("mem", "u1")]
    assert store.list_namespaces(prefix=("*", "u0")) == [("mem", "u0")]
    assert store.list_namespaces(max_depth=1) == [("mem",), ("memories",)]
    assert store.list_namespaces(limit=1, offset=1) == [("mem", "u1")]


def test_store_searches_by_meaning(target):
    store = ChloeDBStore(target, index={"dims": 64, "embed": Words(), "fields": ["text"]}, table="semantic")
    store.put(("m",), "a", {"text": "the user likes dark roast coffee", "kind": "pref"})
    store.put(("m",), "b", {"text": "the user lives in a city by the sea", "kind": "fact"})
    store.put(("m",), "c", {"text": "unindexed coffee note"}, index=False)
    found = store.search(("m",), query="coffee roast", limit=2)
    assert found[0].key == "a" and found[0].score > 0.5
    assert [i.key for i in store.search(("m",), query="sea city", filter={"kind": "fact"})] == ["b"]
    own = ChloeDBStore(store.store, index={"model": "hash:64", "fields": ["text"]}, table="own")
    own.put(("m",), "a", {"text": "the user likes dark roast coffee"})
    own.put(("m",), "b", {"text": "the user lives in a city by the sea"})
    own.put(("m",), "c", {"other": "no text to embed"})
    assert own.search(("m",), query="dark roast coffee", limit=1)[0].key == "a"
    assert own.get(("m",), "c").value == {"other": "no text to embed"}


def test_store_expires_items(target):
    store = ChloeDBStore(target, ttl={"default_ttl": 1 / 60, "refresh_on_read": True}, table="ttl")
    store.put(("s",), "short", {"v": 1})
    store.put(("s",), "kept", {"v": 2}, ttl=None)
    store.put(("s",), "long", {"v": 3}, ttl=10)
    assert store.get(("s",), "short").value == {"v": 1}
    time.sleep(1.5)
    assert store.get(("s",), "short") is None
    assert {i.key for i in store.search(("s",))} == {"kept", "long"}
    # Reading moves the time: refreshed every half second, it lasts.
    store.put(("s",), "read", {"v": 4})
    for _ in range(4):
        time.sleep(0.5)
        assert store.get(("s",), "read") is not None
    time.sleep(1.5)
    assert store.get(("s",), "read", refresh_ttl=False) is None


def test_store_in_a_graph(target):
    store = ChloeDBStore(target)

    def remember(state, *, store):
        store.put(("notes",), "last", {"log": state["log"]})
        return {"log": ["remembered"]}

    b = StateGraph(State)
    b.add_node("remember", remember)
    b.add_edge(START, "remember")
    b.add_edge("remember", END)
    g = b.compile(checkpointer=ChloeDBSaver(store.store), store=store)
    g.invoke({"log": ["x"]}, cfg("s1"))
    assert store.get(("notes",), "last").value == {"log": ["x"]}

    async def go():
        await store.aput(("notes",), "async", {"v": 1})
        return await store.aget(("notes",), "async")

    assert asyncio.run(go()).value == {"v": 1}


def test_store_refuses_bad_namespaces(target):
    from langgraph.store.base import InvalidNamespaceError, PutOp

    store = ChloeDBStore(target)
    with pytest.raises(InvalidNamespaceError):
        store.put(("a.b",), "k", {"v": 1})
    with pytest.raises(InvalidNamespaceError):
        store.batch([PutOp(("a.b",), "k", {"v": 1})])


def test_messages_and_subgraphs(target):
    from langchain_core.language_models.fake_chat_models import GenericFakeChatModel
    from langchain_core.messages import AIMessage, HumanMessage
    from langgraph.graph import MessagesState

    model = GenericFakeChatModel(messages=iter([AIMessage(content="hello"), AIMessage(content="again")]))

    def chat(state):
        return {"messages": [model.invoke(state["messages"])]}

    inner = StateGraph(MessagesState)
    inner.add_node("chat", chat)
    inner.add_edge(START, "chat")
    inner.add_edge("chat", END)
    outer = StateGraph(MessagesState)
    outer.add_node("inner", inner.compile())
    outer.add_edge(START, "inner")
    outer.add_edge("inner", END)
    saver = ChloeDBSaver(target)
    g = outer.compile(checkpointer=saver)
    g.invoke({"messages": [HumanMessage(content="hi")]}, cfg("m1"))
    out = g.invoke({"messages": [HumanMessage(content="hi again")]}, cfg("m1"))
    assert [m.content for m in out["messages"]] == ["hi", "hello", "hi again", "again"]
    assert isinstance(out["messages"][1], AIMessage)
    state = ChloeDBSaver(saver.store).get_tuple(cfg("m1")).checkpoint["channel_values"]["messages"]
    assert [type(m).__name__ for m in state] == ["HumanMessage", "AIMessage", "HumanMessage", "AIMessage"]
    # The subgraph's checkpoints are in a namespace of their own.
    namespaces = {c.config["configurable"]["checkpoint_ns"] for c in saver.list(None)}
    assert "" in namespaces and any(n.startswith("inner:") for n in namespaces)
