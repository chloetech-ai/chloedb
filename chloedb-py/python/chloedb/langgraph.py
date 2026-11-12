"""ChloeDB for LangGraph agents (`pip install "chloedb[langgraph]"`): a
checkpointer, for a graph's state between steps and runs, and a store, for
memories that outlive a thread.

`ChloeDBSaver` keeps each checkpoint and the writes pending on it as rows
of two tables, so a thread resumes after a restart, from another process
or, through a chloedb-server, from another machine::

    from chloedb.langgraph import ChloeDBSaver, ChloeDBStore

    graph = builder.compile(checkpointer=ChloeDBSaver("agent.chloedb"),
                            store=ChloeDBStore("agent.chloedb", index={"model": "potion-retrieval-32M"}))
    graph.invoke({"messages": [...]}, {"configurable": {"thread_id": "t1"}})

`ChloeDBStore` is LangGraph's `BaseStore`: items by namespace and key,
filtered by their values' keys from a JSON field's index, searched by
meaning when it has an index (vectors from a LangChain embedding model,
`index={"dims": 1536, "embed": "openai:text-embedding-3-small"}`, or from
a model ChloeDB runs, `index={"model": "potion-retrieval-32M"}`), and
expired by ChloeDB itself when they have a time to live (`ttl=`).
"""

from __future__ import annotations

import asyncio
import base64
import json
import os
import random
from datetime import datetime, timedelta, timezone
from typing import Any, AsyncIterator, Iterable, Iterator, Mapping, Sequence

try:
    from langchain_core.runnables import RunnableConfig
    from langgraph.checkpoint.base import (
        WRITES_IDX_MAP,
        BaseCheckpointSaver,
        ChannelVersions,
        Checkpoint,
        CheckpointMetadata,
        CheckpointTuple,
        SerializerProtocol,
        get_checkpoint_id,
        get_checkpoint_metadata,
    )
    from langgraph.store.base import (
        BaseStore,
        GetOp,
        Item,
        ListNamespacesOp,
        MatchCondition,
        Op,
        PutOp,
        Result,
        SearchItem,
        SearchOp,
    )
except ImportError as e:  # pragma: no cover - the extra is not installed
    raise ImportError('chloedb.langgraph needs langgraph: pip install "chloedb[langgraph]"') from e

from ._adapters import Params, chunks, conditions, ensure_table, field, json_fits, json_value, open_store, rows_of, table_schema, unit, where
from ._search import quote

__all__ = ["ChloeDBSaver", "ChloeDBStore"]


def _key(*parts: Any) -> str:
    """A row's key from the parts that name it."""
    return json.dumps([str(p) if not isinstance(p, int) else p for p in parts], ensure_ascii=False, separators=(",", ":"))


def _b64(data: bytes) -> str:
    return base64.b64encode(data).decode("ascii")


def _unb64(text: str | None) -> bytes:
    return base64.b64decode(text) if text else b""


def _now() -> datetime:
    return datetime.now(timezone.utc)


def _iso(t: datetime) -> str:
    return t.astimezone(timezone.utc).isoformat().replace("+00:00", "Z")


def _time(text: str | None) -> datetime | None:
    if not text:
        return None
    return datetime.fromisoformat(text.replace("Z", "+00:00"))


# The checkpointer.

_ROOT = ""


class ChloeDBSaver(BaseCheckpointSaver[str]):
    """A LangGraph checkpointer over two tables of a ChloeDB database.

    Args:
        db: a `chloedb.Database` or `chloedb.Client`, a database's path
            (opened, or made at the first write), or a chloedb-server's
            `http(s)://` URL.
        table: the checkpoints' table; `writes_table` the pending writes'.
        serde: how checkpoints and writes are written (LangGraph's
            JsonPlusSerializer by default).
        models: where a database in this process finds its models.
        token: a server's token (or $CHLOEDB_TOKEN).

    The async methods run the same calls on a worker thread, so one saver
    serves `invoke` and `ainvoke` alike.
    """

    def __init__(self, db: Any = "langgraph.chloedb", *, table: str = "langgraph_checkpoints", writes_table: str = "langgraph_writes", serde: SerializerProtocol | None = None, models: str | os.PathLike | None = None, token: str | None = None):
        super().__init__(serde=serde)
        self.store = open_store(db, models=models, token=token)
        self.table = table
        self.writes_table = writes_table
        self._ready = False

    def setup(self) -> None:
        """Makes the tables when they are missing (every call does it
        first)."""
        if self._ready:
            return
        kw = {"type": "keyword"}
        ensure_table(
            self.store,
            {
                "table": self.table,
                "key": "id",
                "fields": [
                    {"name": "id", **kw},
                    {"name": "thread_id", **kw},
                    {"name": "checkpoint_ns", **kw},
                    {"name": "checkpoint_id", **kw},
                    {"name": "parent_checkpoint_id", **kw},
                    {"name": "type", **kw, "indexed": False},
                    {"name": "checkpoint", "type": "text", "indexed": False},
                    {"name": "metadata", "type": "text", "indexed": False},
                    {"name": "meta", "type": "json"},
                ],
            },
        )
        ensure_table(
            self.store,
            {
                "table": self.writes_table,
                "key": "id",
                "fields": [
                    {"name": "id", **kw},
                    {"name": "thread_id", **kw},
                    {"name": "checkpoint_ns", **kw},
                    {"name": "checkpoint_id", **kw},
                    {"name": "task_id", **kw},
                    {"name": "task_path", **kw, "indexed": False},
                    {"name": "idx", "type": "int", "indexed": False},
                    {"name": "channel", **kw, "indexed": False},
                    {"name": "type", **kw, "indexed": False},
                    {"name": "value", "type": "text", "indexed": False},
                ],
            },
        )
        self._ready = True

    # Rows to tuples.

    def _config(self, thread_id: str, ns: str, checkpoint_id: str) -> RunnableConfig:
        return {"configurable": {"thread_id": thread_id, "checkpoint_ns": ns, "checkpoint_id": checkpoint_id}}

    def _writes(self, thread_id: str, ns: str, checkpoint_ids: Sequence[str]) -> dict[str, list]:
        """The pending writes of each checkpoint, in the order LangGraph
        applies them (task path, task id, index)."""
        by: dict[str, list] = {c: [] for c in checkpoint_ids}
        for part in chunks(list(checkpoint_ids), 200):
            p = Params()
            sql = (
                f"SELECT checkpoint_id, task_id, task_path, idx, channel, type, value FROM {quote(self.writes_table)} "
                f"WHERE thread_id = {p(thread_id)} AND checkpoint_ns = {p(ns)} AND checkpoint_id IN ({', '.join(p(c) for c in part)})"
            )
            for r in rows_of(self.store, sql, p.values):
                by[r["checkpoint_id"]].append(r)
        out = {}
        for c, rows in by.items():
            rows.sort(key=lambda r: (r.get("task_path") or "", r["task_id"], r["idx"]))
            out[c] = [(r["task_id"], r["channel"], self.serde.loads_typed((r["type"], _unb64(r["value"])))) for r in rows]
        return out

    def _tuple(self, row: Mapping[str, Any], writes: list) -> CheckpointTuple:
        thread_id, ns = row["thread_id"], row["checkpoint_ns"] or _ROOT
        parent = row.get("parent_checkpoint_id")
        return CheckpointTuple(
            self._config(thread_id, ns, row["checkpoint_id"]),
            self.serde.loads_typed((row["type"], _unb64(row["checkpoint"]))),
            json.loads(row["metadata"]) if row.get("metadata") else {},
            self._config(thread_id, ns, parent) if parent else None,
            writes,
        )

    def _ids(self, thread_id: str | None, ns: str | None, filter: Mapping[str, Any] | None, before: str | None) -> list[tuple[str, str, str, str]]:
        """(key, thread, namespace, checkpoint id) of the checkpoints the
        conditions keep, newest first (checkpoint ids sort by time)."""
        p = Params()
        conds = []
        if thread_id is not None:
            conds.append(f"thread_id = {p(thread_id)}")
        if ns is not None:
            conds.append(f"checkpoint_ns = {p(ns)}")
        conds.append(where(dict(filter), "meta", p) if filter else None)
        sql = f"SELECT id, thread_id, checkpoint_ns, checkpoint_id FROM {quote(self.table)} {conditions(*conds)}"
        found = [(r["id"], r["thread_id"], r["checkpoint_ns"] or _ROOT, r["checkpoint_id"]) for r in rows_of(self.store, sql, p.values)]
        if before is not None:
            found = [f for f in found if f[3] < before]
        found.sort(key=lambda f: f[3], reverse=True)
        return found

    # The saver's methods.

    def get_tuple(self, config: RunnableConfig) -> CheckpointTuple | None:
        self.setup()
        conf = config["configurable"]
        thread_id = str(conf["thread_id"])
        ns = conf.get("checkpoint_ns", _ROOT) or _ROOT
        checkpoint_id = get_checkpoint_id(config)
        if checkpoint_id:
            row = self.store.get(_key(thread_id, ns, checkpoint_id), table=self.table)
        else:
            # The newest: checkpoint ids sort by time.
            r = self.store.query(
                f"SELECT id FROM {quote(self.table)} WHERE thread_id = :t AND checkpoint_ns = :n ORDER BY checkpoint_id DESC LIMIT 1",
                {"t": thread_id, "n": ns},
            )
            row = self.store.get(r.rows[0][0], table=self.table) if r.rows else None
        if row is None:
            return None
        return self._tuple(row, self._writes(thread_id, ns, [row["checkpoint_id"]])[row["checkpoint_id"]])

    def list(self, config: RunnableConfig | None, *, filter: dict[str, Any] | None = None, before: RunnableConfig | None = None, limit: int | None = None) -> Iterator[CheckpointTuple]:
        """The checkpoints of `config`'s thread (and namespace, and
        checkpoint, when it names them), newest first; `filter` keeps
        those whose metadata has its values, `before` those older than
        its checkpoint."""
        self.setup()
        conf = (config or {}).get("configurable", {}) if config else {}
        thread_id = str(conf["thread_id"]) if conf.get("thread_id") is not None else None
        ns = conf.get("checkpoint_ns")
        ids = self._ids(thread_id, ns, filter, get_checkpoint_id(before) if before else None)
        if config is not None and get_checkpoint_id(config):
            ids = [i for i in ids if i[3] == get_checkpoint_id(config)]
        if limit is not None:
            ids = ids[: max(0, int(limit))]
        for part in chunks(ids, 50):
            rows = self.store.get_many([i[0] for i in part], table=self.table)
            groups: dict[tuple[str, str], list[str]] = {}
            for i in part:
                groups.setdefault((i[1], i[2]), []).append(i[3])
            writes: dict[tuple[str, str, str], list] = {}
            for (t, n), cids in groups.items():
                for c, w in self._writes(t, n, cids).items():
                    writes[(t, n, c)] = w
            for i, row in zip(part, rows):
                if row is not None:
                    yield self._tuple(row, writes.get((i[1], i[2], i[3]), []))

    def put(self, config: RunnableConfig, checkpoint: Checkpoint, metadata: CheckpointMetadata, new_versions: ChannelVersions) -> RunnableConfig:
        self.setup()
        conf = config["configurable"]
        thread_id = str(conf["thread_id"])
        ns = conf.get("checkpoint_ns", _ROOT) or _ROOT
        type_, data = self.serde.dumps_typed(checkpoint)
        meta = json_value(get_checkpoint_metadata(config, metadata))
        self.store.upsert(
            {
                "id": _key(thread_id, ns, checkpoint["id"]),
                "thread_id": thread_id,
                "checkpoint_ns": ns,
                "checkpoint_id": checkpoint["id"],
                "parent_checkpoint_id": conf.get("checkpoint_id"),
                "type": type_,
                "checkpoint": _b64(data),
                "metadata": json.dumps(meta, ensure_ascii=False),
                # The metadata to filter by, when a JSON field takes it.
                "meta": meta if meta and json_fits(meta) else None,
            },
            table=self.table,
        )
        return self._config(thread_id, ns, checkpoint["id"])

    def put_writes(self, config: RunnableConfig, writes: Sequence[tuple[str, Any]], task_id: str, task_path: str = "") -> None:
        """Keeps a task's writes on a checkpoint. A task's writes to the
        special channels (errors, interrupts) replace those it made
        before; its other writes are kept once, as the first made."""
        self.setup()
        conf = config["configurable"]
        thread_id = str(conf["thread_id"])
        ns = conf.get("checkpoint_ns", _ROOT) or _ROOT
        checkpoint_id = str(conf["checkpoint_id"])
        rows = []
        for idx, (channel, value) in enumerate(writes):
            i = WRITES_IDX_MAP.get(channel, idx)
            type_, data = self.serde.dumps_typed(value)
            rows.append(
                {
                    "id": _key(thread_id, ns, checkpoint_id, task_id, i),
                    "thread_id": thread_id,
                    "checkpoint_ns": ns,
                    "checkpoint_id": checkpoint_id,
                    "task_id": task_id,
                    "task_path": task_path,
                    "idx": i,
                    "channel": channel,
                    "type": type_,
                    "value": _b64(data),
                }
            )
        if not rows:
            return
        if not all(w[0] in WRITES_IDX_MAP for w in writes):
            have = self.store.get_many([r["id"] for r in rows], table=self.writes_table)
            rows = [r for r, h in zip(rows, have) if h is None]
            if not rows:
                return
        self.store.upsert(rows, table=self.writes_table)

    def delete_thread(self, thread_id: str) -> None:
        """Deletes a thread's checkpoints and writes, in every namespace."""
        self.setup()
        for t in (self.table, self.writes_table):
            self.store.sql(f"DELETE FROM {quote(t)} WHERE thread_id = :t", {"t": str(thread_id)})

    def copy_thread(self, source_thread_id: str, target_thread_id: str) -> None:
        """Copies a thread's checkpoints and writes to another thread
        (replacing the ones of the same ids there)."""
        self.setup()
        src, dst = str(source_thread_id), str(target_thread_id)
        for t in (self.table, self.writes_table):
            rows = list(rows_of(self.store, f"SELECT id FROM {quote(t)} WHERE thread_id = :t", {"t": src}))
            for part in chunks([r["id"] for r in rows], 200):
                out = []
                for r in self.store.get_many(list(part), table=t):
                    if r is None:
                        continue
                    r = dict(r)
                    parts = json.loads(r["id"])
                    parts[0] = dst
                    r["id"] = _key(*parts)
                    r["thread_id"] = dst
                    if t == self.table and r.get("meta") is not None and isinstance(r["meta"], str):
                        r["meta"] = json.loads(r["meta"])
                    out.append(r)
                if out:
                    self.store.upsert(out, table=t)

    def get_next_version(self, current: str | None, channel: None = None) -> str:
        if current is None:
            current_v = 0
        elif isinstance(current, int):
            current_v = current
        else:
            current_v = int(current.split(".")[0])
        return f"{current_v + 1:032}.{random.random():016}"

    # Async: the same calls on a worker thread.

    async def aget_tuple(self, config: RunnableConfig) -> CheckpointTuple | None:
        return await asyncio.to_thread(self.get_tuple, config)

    async def alist(self, config: RunnableConfig | None, *, filter: dict[str, Any] | None = None, before: RunnableConfig | None = None, limit: int | None = None) -> AsyncIterator[CheckpointTuple]:
        items = await asyncio.to_thread(lambda: list(self.list(config, filter=filter, before=before, limit=limit)))
        for item in items:
            yield item

    async def aput(self, config: RunnableConfig, checkpoint: Checkpoint, metadata: CheckpointMetadata, new_versions: ChannelVersions) -> RunnableConfig:
        return await asyncio.to_thread(self.put, config, checkpoint, metadata, new_versions)

    async def aput_writes(self, config: RunnableConfig, writes: Sequence[tuple[str, Any]], task_id: str, task_path: str = "") -> None:
        await asyncio.to_thread(self.put_writes, config, writes, task_id, task_path)

    async def adelete_thread(self, thread_id: str) -> None:
        await asyncio.to_thread(self.delete_thread, thread_id)

    async def acopy_thread(self, source_thread_id: str, target_thread_id: str) -> None:
        await asyncio.to_thread(self.copy_thread, source_thread_id, target_thread_id)


# The store.


def _ns(namespace: Sequence[str]) -> str:
    return ".".join(namespace)


class ChloeDBStore(BaseStore):
    """LangGraph's `BaseStore` over a table of a ChloeDB database.

    Args:
        db: a `chloedb.Database` or `chloedb.Client`, a database's path
            (opened, or made at the first write), or a chloedb-server's
            `http(s)://` URL.
        table: the table (made when missing).
        index: makes `search(query=...)` rank by meaning. LangGraph's form,
            `{"dims": 1536, "embed": <a LangChain Embeddings, a function, or
            "provider:model">, "fields": ["text"]}`, embeds in Python; `{"model":
            "potion-retrieval-32M", "fields": [...]}` has ChloeDB embed the
            text itself. `fields` are the paths of each value's text (default
            the whole value); an item's texts are embedded as one.
        ttl: LangGraph's TTL config, `{"default_ttl": minutes,
            "refresh_on_read": True}`: an item expires that many minutes
            after it is written (or last read), and ChloeDB leaves expired
            items out of every read at once and deletes them.
        models: where a database in this process finds its models.
        token: a server's token (or $CHLOEDB_TOKEN).
    """

    supports_ttl = True

    def __init__(self, db: Any = "langgraph.chloedb", *, table: str = "langgraph_store", index: Mapping[str, Any] | None = None, ttl: Mapping[str, Any] | None = None, models: str | os.PathLike | None = None, token: str | None = None):
        from langgraph.store.base.embed import ensure_embeddings, tokenize_path

        self.store = open_store(db, models=models, token=token)
        self.table = table
        self.ttl_config = dict(ttl) if ttl else None
        self.index_config = dict(index) if index else None
        self.embeddings = None
        self._model = None
        self._fields: list[tuple[str, Any]] = []
        if self.index_config:
            if "model" in self.index_config and "embed" in self.index_config:
                raise ValueError("index takes embed= (LangChain embeds) or model= (ChloeDB embeds), not both")
            if "model" in self.index_config:
                self._model = self.index_config["model"]
            elif "embed" in self.index_config:
                self.embeddings = ensure_embeddings(self.index_config["embed"])
                if not self.index_config.get("dims"):
                    raise ValueError("index needs dims= with embed=")
            else:
                raise ValueError("index takes embed= and dims= (LangChain embeds) or model= (ChloeDB embeds)")
            self._fields = [(p, tokenize_path(p) if p != "$" else p) for p in (self.index_config.get("fields") or ["$"])]
        fields: list[dict] = [
            {"name": "id", "type": "keyword"},
            {"name": "ns", "type": "keyword"},
            {"name": "key", "type": "keyword", "indexed": False},
            {"name": "value", "type": "text", "indexed": False},
            {"name": "attrs", "type": "json"},
            {"name": "created_at", "type": "timestamp", "indexed": False},
            {"name": "updated_at", "type": "timestamp"},
            {"name": "expires_at", "type": "timestamp"},
            {"name": "ttl", "type": "float", "indexed": False},
        ]
        if self.index_config:
            fields.append({"name": "content", "type": "text", "indexed": False})
            vec: dict[str, Any] = {"name": "embedding", "type": "vector"}
            if self._model is not None:
                vec["embed"] = {"from": ["content"], "model": self._model}
            else:
                vec["dim"] = int(self.index_config["dims"])
            fields.append(vec)
        have = table_schema(self.store, table)
        if have is None:
            have = ensure_table(self.store, {"table": table, "key": "id", "expire_by": "expires_at", "fields": fields})
        if self.index_config and field(have, "embedding") is None:
            raise ValueError(f"table {table!r} was made without an index: give its items a table of their own (table=) to search them by meaning")

    # Rows.

    def _item(self, row: Mapping[str, Any], score: float | None = None, search: bool = False) -> Item:
        ns = tuple(row["ns"].split(".")) if row["ns"] else ()
        value = json.loads(row["value"]) if row.get("value") else {}
        created, updated = _time(row.get("created_at")), _time(row.get("updated_at"))
        if search:
            return SearchItem(namespace=ns, key=row["key"], value=value, created_at=created, updated_at=updated, score=score)
        return Item(namespace=ns, key=row["key"], value=value, created_at=created, updated_at=updated)

    def _texts(self, op: PutOp) -> str | None:
        from langgraph.store.base.embed import get_text_at_path, tokenize_path

        if not self.index_config or op.index is False or op.value is None:
            return None
        paths = self._fields if op.index is None else [(p, tokenize_path(p)) for p in op.index]
        texts: list[str] = []
        for _, tokens in paths:
            texts.extend(get_text_at_path(op.value, tokens))
        texts = [t for t in texts if t]
        return "\n".join(texts) if texts else None

    def _refresh(self, rows: Iterable[Mapping[str, Any]], refresh: bool) -> None:
        if not refresh:
            return
        now = _now()
        ops = [
            {"op": "set", "table": self.table, "key": r["id"], "field": "expires_at", "value": _iso(now + timedelta(minutes=float(r["ttl"])))}
            for r in rows
            if r.get("ttl") is not None
        ]
        for part in chunks(ops):
            self.store.write(list(part))

    def _refresh_on(self, op_refresh: bool | None) -> bool:
        # BaseStore's methods resolve it from ttl_config (refresh_on_read).
        return op_refresh is not False

    # Operations.

    def batch(self, ops: Iterable[Op]) -> list[Result]:
        ops = list(ops)
        for op in ops:
            _validate(op)
        results: list[Result] = [None] * len(ops)
        gets = [(i, op) for i, op in enumerate(ops) if isinstance(op, GetOp)]
        if gets:
            rows = self.store.get_many([_key(_ns(op.namespace), op.key) for _, op in gets], table=self.table)
            refresh = []
            for (i, op), row in zip(gets, rows):
                results[i] = None if row is None else self._item(row)
                if row is not None and self._refresh_on(op.refresh_ttl):
                    refresh.append(row)
            self._refresh(refresh, True)
        for i, op in enumerate(ops):
            if isinstance(op, SearchOp):
                results[i] = self._search(op)
            elif isinstance(op, ListNamespacesOp):
                results[i] = self._list_namespaces(op)
            elif not isinstance(op, (GetOp, PutOp)):
                raise ValueError(f"unknown operation {type(op).__name__}")
        puts: dict[tuple[tuple[str, ...], str], PutOp] = {}
        for op in ops:
            if isinstance(op, PutOp):
                puts[(op.namespace, op.key)] = op
        if puts:
            self._put(list(puts.values()))
        return results

    async def abatch(self, ops: Iterable[Op]) -> list[Result]:
        return await asyncio.to_thread(self.batch, list(ops))

    def _put(self, ops: Sequence[PutOp]) -> None:
        deletes = [_key(_ns(op.namespace), op.key) for op in ops if op.value is None]
        writes = [op for op in ops if op.value is not None]
        if deletes:
            for part in chunks(deletes):
                self.store.delete(list(part), table=self.table)
        if not writes:
            return
        ids = [_key(_ns(op.namespace), op.key) for op in writes]
        existing = {}
        for part in chunks(ids):
            for r in self.store.get_many(list(part), table=self.table):
                if r is not None:
                    existing[r["id"]] = r.get("created_at")
        texts = [self._texts(op) for op in writes]
        vectors: list[Any] = [None] * len(writes)
        if self.embeddings is not None:
            todo = [(n, t) for n, t in enumerate(texts) if t]
            if todo:
                for (n, _), v in zip(todo, self.embeddings.embed_documents([t for _, t in todo])):
                    vectors[n] = unit(v)
        now = _now()
        rows = []
        for op, id_, text, vec in zip(writes, ids, texts, vectors):
            value = json_value(dict(op.value))
            # BaseStore.put resolved the TTL from ttl_config already.
            ttl = op.ttl
            row = {
                "id": id_,
                "ns": _ns(op.namespace),
                "key": op.key,
                "value": json.dumps(value, ensure_ascii=False),
                "attrs": value if value and json_fits(value) else None,
                "created_at": existing.get(id_) or _iso(now),
                "updated_at": _iso(now),
                "expires_at": _iso(now + timedelta(minutes=float(ttl))) if ttl is not None else None,
                "ttl": float(ttl) if ttl is not None else None,
            }
            if self.index_config:
                row["content"] = text
                if vec is not None:
                    row["embedding"] = vec
            rows.append(row)
        if self._model is not None:
            # A row with no text to embed is written without a vector.
            have = [r for r in rows if r.get("content")]
            none = [r for r in rows if not r.get("content")]
            for part in chunks(have):
                self.store.upsert(list(part), table=self.table)
            if none:
                self._upsert_unembedded(none)
            return
        for part in chunks(rows):
            self.store.upsert(list(part), table=self.table)

    def _upsert_unembedded(self, rows: list[dict]) -> None:
        """Rows that have no text to embed, written without a vector (the
        field's model would refuse them for lacking one)."""
        db = getattr(self.store, "_db", None)
        later = getattr(self.store, "set_embed_later", None)
        if later is not None and db is not None:
            later(True)
            try:
                for part in chunks(rows):
                    self.store.upsert(list(part), table=self.table)
            finally:
                later(False)
            return
        for part in chunks(rows):
            self.store.upsert(list(part), table=self.table)

    def _prefix(self, prefix: Sequence[str], p: Params) -> str | None:
        if not prefix:
            return None
        s = _ns(prefix)
        return f"(ns = {p(s)} OR PREFIX(ns, {p(s + '.')}))"

    def _search(self, op: SearchOp) -> list[SearchItem]:
        p = Params()
        cond = conditions(self._prefix(op.namespace_prefix, p), where(op.filter, "attrs", p) if op.filter else None)
        page = f"LIMIT {int(op.limit)} OFFSET {int(op.offset)}"
        if op.query and self.index_config:
            if self.embeddings is not None:
                p.values["_qv"] = unit(self.embeddings.embed_query(op.query))
            else:
                p.values["_qv"] = list(self.store.embed(op.query, field="embedding", table=self.table))
            r = self.store.query(f"SELECT *, score() FROM {quote(self.table)} {cond} ORDER BY similarity(embedding, :_qv) DESC {page}", p.values)
            rows = list(r)
            items = [self._item(row, float(s) if s is not None else None, search=True) for row, s in zip(rows, r.scores)]
        else:
            rows = list(self.store.query(f"SELECT * FROM {quote(self.table)} {cond} ORDER BY updated_at DESC {page}", p.values))
            items = [self._item(row, search=True) for row in rows]
        self._refresh(rows, self._refresh_on(op.refresh_ttl))
        return items

    def _list_namespaces(self, op: ListNamespacesOp) -> list[tuple[str, ...]]:
        p = Params()
        cond = None
        for c in op.match_conditions or ():
            if c.match_type == "prefix" and c.path and "*" not in c.path:
                cond = self._prefix(c.path, p)
                break
        names = {tuple(r["ns"].split(".")) for r in rows_of(self.store, f"SELECT ns, COUNT(*) AS n FROM {quote(self.table)} {conditions(cond)} GROUP BY ns", p.values) if r["ns"]}
        if op.match_conditions:
            names = {n for n in names if all(_matches(c, n) for c in op.match_conditions)}
        if op.max_depth is not None:
            names = {n[: op.max_depth] for n in names}
        return sorted(names)[op.offset : op.offset + op.limit]


def _validate(op: Op) -> None:
    """Refuses a namespace whose labels would read as other labels once
    joined by dots (LangGraph's check, which ops given to `batch` skip)."""
    if isinstance(op, (GetOp, PutOp)):
        labels = [op.namespace]
    elif isinstance(op, SearchOp):
        labels = [op.namespace_prefix]
    elif isinstance(op, ListNamespacesOp):
        labels = [c.path for c in op.match_conditions or ()]
    else:
        return
    from langgraph.store.base import InvalidNamespaceError

    for ns in labels:
        for label in ns:
            if not isinstance(label, str) or not label or "." in label or "\x00" in label:
                raise InvalidNamespaceError(f"invalid namespace label {label!r} in {ns}: labels are non-empty text without periods")


def _matches(c: MatchCondition, ns: tuple[str, ...]) -> bool:
    if len(ns) < len(c.path):
        return False
    pairs = zip(ns, c.path) if c.match_type == "prefix" else zip(reversed(ns), reversed(c.path))
    if c.match_type not in ("prefix", "suffix"):
        raise ValueError(f"unknown match type {c.match_type!r}")
    return all(want == "*" or have == want for have, want in pairs)
