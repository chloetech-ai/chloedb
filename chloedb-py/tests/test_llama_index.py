import pytest

pytest.importorskip("llama_index.core")

from llama_index.core import Document, Settings, StorageContext, VectorStoreIndex
from llama_index.core.base.embeddings.base import BaseEmbedding
from llama_index.core.schema import TextNode
from llama_index.core.vector_stores.types import FilterCondition, FilterOperator, MetadataFilter, MetadataFilters, VectorStoreQuery, VectorStoreQueryMode

from adapters_common import target, words_vector  # noqa: F401
from chloedb.llama_index import ChloeDBVectorStore


class Words(BaseEmbedding):
    def _get_query_embedding(self, query):
        return words_vector(query)

    async def _aget_query_embedding(self, query):
        return words_vector(query)

    def _get_text_embedding(self, text):
        return words_vector(text)


Settings.llm = None


def nodes():
    texts = [
        ("n1", "doc-a", "the dragon sleeps under the mountain", {"genre": "fantasy", "year": 1999, "tags": ["dragon"]}),
        ("n2", "doc-a", "the dragon burns the city", {"genre": "fantasy", "year": 2021, "tags": ["dragon", "city"]}),
        ("n3", "doc-b", "a detective solves the case of the missing letter", {"genre": "mystery", "year": 2005}),
        ("n4", "doc-c", "ships sail the winter sea", {"genre": "sea", "year": 2012}),
    ]
    out = []
    for nid, doc, text, meta in texts:
        n = TextNode(id_=nid, text=text, metadata=meta)
        n.relationships = {}
        from llama_index.core.schema import NodeRelationship, RelatedNodeInfo

        n.relationships[NodeRelationship.SOURCE] = RelatedNodeInfo(node_id=doc)
        n.embedding = words_vector(text)
        out.append(n)
    return out


@pytest.fixture
def store(target):
    s = ChloeDBVectorStore(target)
    s.add(nodes())
    return s


def q(text, k=4, **kw):
    return VectorStoreQuery(query_embedding=words_vector(text), query_str=text, similarity_top_k=k, **kw)


def test_adds_and_queries_nodes(store):
    r = store.query(q("dragon mountain", 1))
    assert r.ids == ["n1"]
    assert r.nodes[0].get_content() == "the dragon sleeps under the mountain"
    assert r.nodes[0].metadata == {"genre": "fantasy", "year": 1999, "tags": ["dragon"]}
    assert r.nodes[0].ref_doc_id == "doc-a"
    assert r.similarities[0] > 0.4
    got = store.get_nodes(["n3", "n4"])
    assert [n.node_id for n in got] == ["n3", "n4"]


def test_filters(store):
    f = MetadataFilters(filters=[MetadataFilter(key="genre", value="fantasy")])
    assert sorted(store.query(q("dragon", filters=f)).ids) == ["n1", "n2"]
    f = MetadataFilters(filters=[MetadataFilter(key="year", value=2005, operator=FilterOperator.GT), MetadataFilter(key="genre", value="sea", operator=FilterOperator.NE)])
    assert store.query(q("dragon", filters=f)).ids == ["n2"]
    f = MetadataFilters(filters=[MetadataFilter(key="genre", value="sea"), MetadataFilter(key="year", value=1999)], condition=FilterCondition.OR)
    assert sorted(store.query(q("dragon", filters=f)).ids) == ["n1", "n4"]
    f = MetadataFilters(filters=[MetadataFilter(key="genre", value=["sea", "mystery"], operator=FilterOperator.IN)])
    assert sorted(store.query(q("dragon", filters=f)).ids) == ["n3", "n4"]
    f = MetadataFilters(filters=[MetadataFilter(key="tags", value="city", operator=FilterOperator.CONTAINS)])
    assert store.query(q("dragon", filters=f)).ids == ["n2"]
    f = MetadataFilters(filters=[MetadataFilter(key="tags", value=None, operator=FilterOperator.IS_EMPTY)])
    assert sorted(store.query(q("dragon", filters=f)).ids) == ["n3", "n4"]
    nested = MetadataFilters(filters=[MetadataFilter(key="genre", value="fantasy"), MetadataFilters(filters=[MetadataFilter(key="year", value=2000, operator=FilterOperator.LT)])])
    assert store.query(q("dragon", filters=nested)).ids == ["n1"]
    assert sorted(store.query(q("dragon", doc_ids=["doc-a"])).ids) == ["n1", "n2"]
    assert store.query(q("dragon", node_ids=["n4"])).ids == ["n4"]


def test_text_and_hybrid_queries(store):
    r = store.query(VectorStoreQuery(query_str="burns", similarity_top_k=1, mode=VectorStoreQueryMode.TEXT_SEARCH))
    assert r.ids == ["n2"]
    r = store.query(q("missing letter", 2, mode=VectorStoreQueryMode.HYBRID))
    assert r.ids[0] == "n3"
    with pytest.raises(ValueError):
        store.query(q("x", mode=VectorStoreQueryMode.SVM))


def test_deletes(store):
    store.delete("doc-a")
    assert [n.node_id for n in store.get_nodes(["n1", "n2", "n3"])] == ["n3"]
    store.delete_nodes(filters=MetadataFilters(filters=[MetadataFilter(key="genre", value="sea")]))
    assert [n.node_id for n in store.get_nodes(filters=MetadataFilters(filters=[MetadataFilter(key="year", value=0, operator=FilterOperator.GT)]))] == ["n3"]
    store.clear()
    assert store.get_nodes(["n3"]) == []


def test_an_index_over_the_store(target):
    store = ChloeDBVectorStore(target, table="docs")
    docs = [
        Document(text="ChloeDB keeps text and vectors in one file", metadata={"topic": "db"}, doc_id="a"),
        Document(text="the garden blooms in spring", metadata={"topic": "nature"}, doc_id="b"),
    ]
    index = VectorStoreIndex.from_documents(docs, storage_context=StorageContext.from_defaults(vector_store=store), embed_model=Words())
    found = index.as_retriever(similarity_top_k=1).retrieve("vectors in a file")
    assert found[0].node.ref_doc_id == "a"
    found = index.as_retriever(similarity_top_k=1, vector_store_query_mode="hybrid").retrieve("garden spring")
    assert found[0].node.ref_doc_id == "b"
    index.delete_ref_doc("a")
    assert index.as_retriever(similarity_top_k=5).retrieve("vectors")[0].node.ref_doc_id == "b"


def test_chloedb_embeds_itself(target):
    store = ChloeDBVectorStore(target, table="own", model="hash:64")
    assert not store.is_embedding_query
    docs = [Document(text="the dragon sleeps under the mountain", doc_id="a"), Document(text="a detective and a missing letter", doc_id="b")]
    index = VectorStoreIndex.from_documents(docs, storage_context=StorageContext.from_defaults(vector_store=store), embed_model=store.embed_model)
    assert index.as_retriever(similarity_top_k=1).retrieve("missing letter")[0].node.ref_doc_id == "b"
    # Nodes without vectors: the table's model embeds them.
    store.add([TextNode(id_="c", text="ships sail the winter sea")])
    r = store.query(VectorStoreQuery(query_str="winter sea ships", similarity_top_k=1))
    assert r.ids == ["c"]
