import asyncio

import pytest

pytest.importorskip("langchain_core")

from langchain_core.documents import Document
from langchain_core.embeddings import Embeddings

import chloedb
from adapters_common import DIMS, target, words_vector  # noqa: F401
from chloedb.langchain import ChloeDBVectorStore


class Words(Embeddings):
    def embed_documents(self, texts):
        return [words_vector(t) for t in texts]

    def embed_query(self, text):
        return words_vector(text)


TEXTS = [
    "the dragon sleeps under the mountain",
    "a detective solves the case of the missing letter",
    "ships sail the winter sea to the city",
    "the garden blooms in spring",
    "the dragon burns the city",
]
META = [
    {"genre": "fantasy", "year": 1999, "tags": ["dragon", "mountain"], "author": {"name": "Ann"}, "draft": False},
    {"genre": "mystery", "year": 2005, "tags": ["letter"], "author": {"name": "Bo"}, "draft": True},
    {"genre": "sea", "year": 2012, "tags": [], "author": {"name": "Cy"}},
    {"genre": "nature", "year": 2020, "score": 4.5},
    {"genre": "fantasy", "year": 2021, "tags": ["dragon", "city"], "note": None},
]
IDS = ["d1", "d2", "d3", "d4", "d5"]


@pytest.fixture
def store(target):
    s = ChloeDBVectorStore(target, embedding=Words())
    s.add_texts(TEXTS, META, ids=IDS)
    return s


def ids(docs):
    return [d.id for d in docs]


def test_writes_and_reads_documents(store):
    docs = store.get_by_ids(["d1", "nope", "d4"])
    assert ids(docs) == ["d1", "d4"]
    assert docs[0].page_content == TEXTS[0]
    assert docs[0].metadata == META[0]
    assert docs[1].metadata == META[3]
    assert store.get_by_ids(["d5"])[0].metadata == META[4]  # a None value kept
    # Replaced by id.
    store.add_texts(["the dragon wakes"], [{"genre": "fantasy", "year": 2000}], ids=["d1"])
    assert store.get_by_ids(["d1"])[0].page_content == "the dragon wakes"
    store.delete(["d1", "d2"])
    assert ids(store.get_by_ids(IDS)) == ["d3", "d4", "d5"]
    # Documents keep their ids; new ones are made for those without.
    new = store.add_documents([Document(page_content="a winter garden", metadata={"genre": "nature"}, id="g1"), Document(page_content="no id")])
    assert new[0] == "g1" and len(new[1]) == 32
    assert ids(store.get_by_ids(new)) == new


def test_searches_by_meaning(store):
    assert store.similarity_search("dragon mountain", k=1)[0].id == "d1"
    found = store.similarity_search_with_score("detective letter", k=2)
    assert found[0][0].id == "d2"
    assert found[0][1] > found[1][1]
    assert -1.0 <= found[1][1] <= 1.0001
    rel = store.similarity_search_with_relevance_scores("detective letter", k=5)
    assert all(0.0 <= s <= 1.0 for _, s in rel)
    by_vector = store.similarity_search_by_vector(words_vector("garden spring"), k=1)
    assert ids(by_vector) == ["d4"]


def test_filters_on_metadata(store):
    q = "the dragon"
    assert sorted(ids(store.similarity_search(q, k=5, filter={"genre": "fantasy"}))) == ["d1", "d5"]
    assert sorted(ids(store.similarity_search(q, k=5, filter={"year": {"$gte": 2012}}))) == ["d3", "d4", "d5"]
    assert sorted(ids(store.similarity_search(q, k=5, filter={"year": {"$gt": 2000, "$lt": 2020}}))) == ["d2", "d3"]
    assert sorted(ids(store.similarity_search(q, k=5, filter={"genre": {"$in": ["sea", "nature"]}}))) == ["d3", "d4"]
    assert sorted(ids(store.similarity_search(q, k=5, filter={"genre": {"$nin": ["fantasy", "sea"]}}))) == ["d2", "d4"]
    assert sorted(ids(store.similarity_search(q, k=5, filter={"genre": {"$ne": "fantasy"}}))) == ["d2", "d3", "d4"]
    assert sorted(ids(store.similarity_search(q, k=5, filter={"$or": [{"genre": "sea"}, {"year": 1999}]}))) == ["d1", "d3"]
    assert sorted(ids(store.similarity_search(q, k=5, filter={"$and": [{"genre": "fantasy"}, {"year": {"$gt": 2000}}]}))) == ["d5"]
    assert sorted(ids(store.similarity_search(q, k=5, filter={"$not": {"genre": "fantasy"}}))) == ["d2", "d3", "d4"]
    # Nested keys, lists, booleans, presence.
    assert ids(store.similarity_search(q, k=5, filter={"author": {"name": "Bo"}})) == ["d2"]
    assert sorted(ids(store.similarity_search(q, k=5, filter={"tags": "dragon"}))) == ["d1", "d5"]
    assert ids(store.similarity_search(q, k=5, filter={"tags": {"$contains": "city"}})) == ["d5"]
    assert ids(store.similarity_search(q, k=5, filter={"draft": True})) == ["d2"]
    assert ids(store.similarity_search(q, k=5, filter={"draft": False})) == ["d1"]
    assert ids(store.similarity_search(q, k=5, filter={"score": {"$exists": True}})) == ["d4"]
    assert ids(store.similarity_search(q, k=5, filter={"score": 4.5})) == ["d4"]
    assert sorted(ids(store.similarity_search(q, k=5, filter={"genre": {"$like": "fan%"}}))) == ["d1", "d5"]
    assert sorted(ids(store.similarity_search(q, k=5, filter={"year": {"$between": [2005, 2012]}}))) == ["d2", "d3"]
    # A key with a dot or a quote is one key.
    store.add_texts(["odd keys"], [{"a.b": 1, "it's": "x"}], ids=["odd"])
    assert ids(store.similarity_search("odd", k=5, filter={"a.b": 1})) == ["odd"]
    assert ids(store.similarity_search("odd", k=5, filter={"it's": "x"})) == ["odd"]
    with pytest.raises(ValueError, match="unknown filter operator"):
        store.similarity_search(q, filter={"year": {"$regex": "x"}})


def test_searches_by_words_and_both(store):
    assert store.similarity_search("burns", k=1, mode="text")[0].id == "d5"
    hybrid = store.similarity_search("dragon city", k=2, mode="hybrid")
    assert ids(hybrid)[0] == "d5"
    hybrid_store = ChloeDBVectorStore(store.store, embedding=Words(), mode="hybrid")
    assert hybrid_store.similarity_search("missing letter", k=1)[0].id == "d2"
    retriever = store.as_retriever(search_kwargs={"k": 2, "mode": "text", "filter": {"genre": "fantasy"}})
    assert ids(retriever.invoke("city")) == ["d5"]


def test_deletes_by_filter(store):
    store.delete(filter={"genre": "fantasy"})
    assert ids(store.get_by_ids(IDS)) == ["d2", "d3", "d4"]
    store.delete(["d2", "d3"], filter={"genre": "mystery"})
    assert ids(store.get_by_ids(IDS)) == ["d3", "d4"]
    with pytest.raises(ValueError):
        store.delete()


def test_maximal_marginal_relevance(store):
    store.add_texts(["the dragon sleeps under the mountain again"], [{"genre": "fantasy"}], ids=["d1b"])
    plain = ids(store.similarity_search("dragon mountain sleeps", k=2))
    assert sorted(plain) == ["d1", "d1b"]
    diverse = ids(store.max_marginal_relevance_search("dragon mountain sleeps", k=2, fetch_k=6, lambda_mult=0.1))
    assert len(diverse) == 2 and len(set(diverse) & {"d1", "d1b"}) == 1


def test_async_methods(store):
    async def go():
        await store.aadd_texts(["an async river"], [{"genre": "sea"}], ids=["a1"])
        return await store.asimilarity_search("async river", k=1)

    assert ids(asyncio.run(go())) == ["a1"]


def test_reopens_the_table(tmp_path):
    path = tmp_path / "keep.chloedb"
    s = ChloeDBVectorStore(str(path), embedding=Words(), table="notes")
    s.add_texts(TEXTS, META, ids=IDS)
    s.store.close()
    again = ChloeDBVectorStore(str(path), embedding=Words(), table="notes")
    assert again.similarity_search("winter sea", k=1)[0].id == "d3"
    # The table's vectors came from outside: ChloeDB cannot embed queries for it.
    again.store.close()
    with pytest.raises(ValueError, match="embedding="):
        ChloeDBVectorStore(str(path), table="notes")


def test_from_texts_and_dimensions(tmp_path):
    s = ChloeDBVectorStore.from_texts(TEXTS, Words(), META, ids=IDS, db=str(tmp_path / "f.chloedb"))
    assert s.similarity_search("garden", k=1)[0].id == "d4"
    made = ChloeDBVectorStore(str(tmp_path / "g.chloedb"), embedding=Words(), dim=DIMS)
    vec = [f for f in chloedb_table(made)["fields"] if f["name"] == "embedding"][0]
    assert vec["dim"] == DIMS


def chloedb_table(store):
    return [t for t in store.store.tables() if t["table"] == store.table][0]


def test_chloedb_embeds_itself(target):
    s = ChloeDBVectorStore(target, model="hash:64", table="own")
    s.add_texts(TEXTS, META, ids=IDS)
    assert s.embeddings is None
    assert s.similarity_search("dragon mountain", k=1)[0].id == "d1"
    assert s.similarity_search("letter", k=1, mode="hybrid")[0].id == "d2"
    assert s.similarity_search("dragon", k=5, filter={"year": {"$gt": 2000}})[0].id == "d5"
    # A store opened again on the table embeds with its model.
    again = ChloeDBVectorStore(s.store, table="own")
    assert again.similarity_search("garden spring", k=1)[0].id == "d4"


def test_refuses_what_it_cannot_keep(store):
    with pytest.raises(ValueError, match="metadata"):
        store.add_texts(["deep"], [{"a": {"b": {"c": {"d": {"e": {"f": {"g": {"h": {"i": {"j": {"k": {"l": {"m": {"n": {"o": {"p": {"q": 1}}}}}}}}}}}}}}}}}])
    with pytest.raises(ValueError):
        ChloeDBVectorStore(store.store, embedding=Words(), mode="fuzzy")
    store.store.sql("CREATE TABLE plain (id KEYWORD PRIMARY KEY, body TEXT)")
    with pytest.raises(chloedb.ChloeDBError, match="not a vector store"):
        ChloeDBVectorStore(store.store, embedding=Words(), table="plain")
