# Memory for an agent

An agent that works over many sessions needs to remember: what was said, what was decided, and facts that change. This recipe keeps an agent's memory in one ChloeDB file: every turn as a row, facts under keys that new values replace, and a recall query that finds what matters to a new message by words and meaning, favours what is recent, and spreads its picks across sessions.

[ChloeCtx](../chloectx/index.md) is this idea built out for Claude Code, with hooks that record every session and a daemon that decides when to bring history back. Use this recipe for your own agent.

You need Python with the `chloedb` package and `chloedb-server` on your `PATH` ([quickstart](../quickstart.md)).

## The store

Two tables in one file. `turns` holds what was said, keyed by session and position; `facts` holds what the agent knows now, one row per key:

```python file=memory.py
import datetime as dt
import chloedb

EMBED = {"model": "potion-retrieval-32M"}
SCHEMA = {
    "table": "turns",
    "key": "id",
    "fields": [
        {"name": "id", "type": "keyword"},
        {"name": "session", "type": "keyword"},
        {"name": "role", "type": "keyword"},
        {"name": "text", "type": "text"},
        {"name": "at", "type": "timestamp"},
        {"name": "emb", "type": "vector", "embed": {"from": ["text"], **EMBED}},
    ],
}
FACTS = """
CREATE TABLE IF NOT EXISTS facts (
  id KEYWORD PRIMARY KEY,
  value TEXT,
  at TIMESTAMP,
  emb VECTOR OPTIONS(embed = 'id, value', model = 'potion-retrieval-32M')
)
"""


class Memory:
    def __init__(self, path):
        self.db = chloedb.open(path, schema=SCHEMA)
        self.db.sql(FACTS)

    def remember(self, session, n, role, text, at):
        """Store one turn of a session."""
        self.db.upsert({"id": f"{session}/{n:04d}", "session": session, "role": role, "text": text, "at": at})

    def learn(self, key, value, at):
        """Store a fact; a newer value of the same key replaces the old one."""
        self.db.upsert({"id": key, "value": value, "at": at}, table="facts")

    def recall(self, message, now, k=4):
        """The turns that matter to a new message: by words and meaning, newer first among equals,
        at most two from any session."""
        return list(self.db.query("""
            SELECT id, session, role, text FROM turns
            ORDER BY rrf(bm25(text, :q), similarity(emb, :v))
                     * decay(at, :now, 2592000) DESC  -- halves every 30 days
            MAX 2 PER session
            LIMIT :k
        """, q=message, v={"embed": message}, now=now, k=k))

    def facts(self, message, k=3):
        """The facts closest to a message."""
        return list(self.db.query("""
            SELECT id, value FROM facts
            ORDER BY rrf(bm25(value, :q), similarity(emb, :v)) DESC LIMIT :k
        """, q=message, v={"embed": message}, k=k))

    def close(self):
        self.db.close()
```

`recall` ranks turns by a fusion of BM25 and vector similarity, multiplies it by [`decay`](../chloedb/query-language.md#decayfield-origin-scale-closeness-to-a-value), which halves a turn's weight for every 30 days of age (2,592,000 seconds), and keeps at most two turns of a session with `MAX 2 PER session`, so one long session does not crowd out the others. Facts need no history: `upsert` replaces a key's row, so a fact is always its latest value.

## Remember, then recall

Two past sessions, a few weeks apart, and a fact that changed between them:

```python file=sessions.py
import datetime as dt
from memory import Memory

T = lambda s: dt.datetime.fromisoformat(s).replace(tzinfo=dt.timezone.utc)
mem = Memory("agent.chloedb")

mem.remember("s1", 1, "user", "Let's store the invoices in Postgres, the team knows it.", T("2026-09-01T10:00"))
mem.remember("s1", 2, "agent", "Done: invoices go to Postgres, table invoices, keyed by number.", T("2026-09-01T10:01"))
mem.remember("s1", 3, "user", "The receipt printer only takes 58 mm paper, keep lines short.", T("2026-09-01T10:05"))
mem.learn("deploy.target", "a single VM on Hetzner", T("2026-09-01T10:10"))

mem.remember("s2", 1, "user", "Customers complain the PDF invoice is hard to read on phones.", T("2026-10-20T09:00"))
mem.remember("s2", 2, "agent", "Switched the invoice PDF to one column and a larger font.", T("2026-10-20T09:02"))
mem.learn("deploy.target", "Kubernetes on DigitalOcean, since October", T("2026-10-20T09:30"))
mem.close()
print("stored")
```

```sh
python3 sessions.py
```

```text output
stored
```

A new message arrives. Before answering, the agent recalls the turns and facts that bear on it and puts them in front of the model:

```python file=new_message.py
import datetime as dt
from memory import Memory

mem = Memory("agent.chloedb")
message = "Can we print the invoice on the receipt printer and where do we deploy it?"
now = dt.datetime(2026, 11, 12, tzinfo=dt.timezone.utc)

print("turns:")
for t in mem.recall(message, now):
    print(f"  [{t['session']}] {t['role']}: {t['text']}")
print("facts:")
for f in mem.facts(message):
    print(f"  {f['id']} = {f['value']}")
mem.close()
```

```sh
python3 new_message.py
```

```text output
turns:
  [s2] agent: Switched the invoice PDF to one column and a larger font.
  [s2] user: Customers complain the PDF invoice is hard to read on phones.
  [s1] user: The receipt printer only takes 58 mm paper, keep lines short.
  [s1] agent: Done: invoices go to Postgres, table invoices, keyed by number.
facts:
  deploy.target = Kubernetes on DigitalOcean, since October
```

The printer's paper width comes back though the new message never says *58 mm*, and the deploy target is its current value, not the one from September. What goes into the model's context is a few lines, not two sessions of transcript.

## Make it better

- **Leave out what the model can still see.** Give each turn the conversation it came from and filter it out of recall while that conversation is open: `WHERE session != :current`.
- **Forget on a schedule.** `OPTIONS(retain = '400 days')` on a table, or an `expire_by` column per row, drops old turns in the background ([tables](../chloedb/query-language.md#tables)).
- **Let the model search too.** Give the agent a tool that runs a read-only `SELECT` over its memory; ChloeCtx's `memory_sql` does exactly that.
- **Distil.** After each session, ask a small model for the decisions and rules it contained and `learn` them as facts; ChloeCtx's distiller does this with `claude -p`.
- **Share it.** On chloedb-server the same class works through `chloedb.connect(url)`, so several agents read and write one memory.
