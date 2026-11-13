# Tenants: a database kept in an object store (experimental)

> **Experimental.** This is a first slice of the design in
> [`docs/design/0.7.0-file-per-agent.md`](../design/0.7.0-file-per-agent.md),
> which is a proposal. The objects it writes are `layout: 0`, and a later
> version may refuse to read them. Do not keep data in it that you cannot
> load again. Nothing uses it unless you ask.

A **tenant** is one ChloeDB database that lives whole in an object store: a
directory, or S3 and the stores that speak its API. A node that serves it
works on a local copy, which is only a cache. Any node pointed at the same
store can open the same tenant, and a node can lose its disk without losing
a write.

```text
<tenant>/HEAD                                         names the rest; the only object that is replaced
<tenant>/<incarnation>/base/<seq>-<epoch>.chloedb     the database up to a transaction, as one ordinary file
<tenant>/<incarnation>/log/<first seq>-<epoch>.arc    the transactions after it
```

A write is committed by replacing `HEAD` on the condition that it is still
the version the node last read. The store decides: of two nodes that write
one tenant at once, the later is refused and nothing of its write is
kept. There is no lease and no clock to trust.

## From Rust

```rust
use std::sync::Arc;
use chloedb::query::Params;
use chloedb::storage::DirStore;
use chloedb::tenant::{Tenant, TenantError, TenantOptions};

let store = Arc::new(DirStore::new("/mnt/tenants")?);   // or chloedb::storage::open_store("s3://bucket/prefix/")?
let t = Tenant::create(store.clone(), "agent-7", TenantOptions::new("/var/cache/agent-7"), |db| {
    db.sql("CREATE TABLE notes (id INT PRIMARY KEY, body TEXT)", &Params::new()).map(|_| ())
})?;

// Committed when this returns: one conditional put of HEAD.
t.write(|db| db.sql("INSERT INTO notes VALUES (1, 'the harbour at dawn')", &Params::new()))?;

// Reads run on the local copy.
t.read(|db| db.sql("SELECT id FROM notes WHERE MATCH(body, 'harbour')", &Params::new()))?;

// Another node, with an empty cache: two reads of the store.
let other = Tenant::open(store, "agent-7", TenantOptions::new("/var/cache/elsewhere"))?;
match other.write(|db| db.sql("INSERT INTO notes VALUES (2, 'two')", &Params::new())) {
    Ok(_) => {}                                   // this node is the writer now
    Err(TenantError::NotWriter) => { /* another node wrote first: nothing was committed, try again */ }
    Err(e) => return Err(e.into()),
}
```

| call | what it costs | what it sees |
|---|---|---|
| `read` | nothing from the store | what this handle committed, and what others had at its last `refresh` |
| `read_latest` | one read of `HEAD`, and the new transactions if any | every write committed before the call |
| `write` | one conditional put (two the first time, when the handle becomes the writer; one more upload when the log is sealed), shared with the writes that waited for the one before | |
| `make_base` | one upload of the database, one put | (the next open is two reads) |
| `tenant::list(store)` | one listing, and eight bytes of each `HEAD` | the ids of the store's tenants |

Writes of one handle that arrive while a commit is on its way to the store
wait, and the next commit takes all of them: their statements run one
after another, each on its caller's thread, and one put of `HEAD` commits
them together or not at all.

A handle holds its cache directory: a second handle on the same directory,
in this process or another, is refused. The directory keeps the tenant's
working copy, and the next handle to open the tenant there takes it up
again when it is a copy of the tenant as the store has it (the last
transaction it holds is in `HEAD`'s history), and fetches only the
transactions after it. Otherwise it starts from the base.

A write fails in one of three ways. `NotWriter`: another node committed
first, nothing was written, and the handle has loaded the tenant again.
`InDoubt`: the store did not answer and could not be asked again, so the
write may or may not be there. Anything else: not committed. The writes
that shared its commit fail with it, the same way.

A handle does not take a store's refusal for `NotWriter`: it loads the
tenant again and looks. A store's client may send a put twice (after an
answer it lost) and report the store's refusal of the second; the write
is then committed, and the handle says so.

## From the server

```bash
chloedb-server --db main.chloedb --tenants /mnt/tenants --tenant-cache /var/cache/tenants
# or --tenants s3://bucket/prefix/   (configured by the environment, as `import` reads S3)

chloedb-server tenants check-store /mnt/tenants   # does this store refuse a put whose condition fails?
chloedb-server tenants list /mnt/tenants          # the ids of its tenants
```

The server probes the store when it starts, and does not start on one that
takes a put whatever the condition. It opens a tenant when a request names
it and closes the ones no request has named for `--tenant-idle-s` seconds
(300), or the least recently used beyond `--tenant-open-max` (1,000). A
tenant it wrote is given a new base before it is closed, so that the next
open, here or on another server, is two reads.

The routes need an admin key until tenants have keys of their own.

| route | does |
|---|---|
| `POST /v1/admin/tenants` | creates one: `{"id": "agent-7", "sql": ["CREATE TABLE notes (id INT PRIMARY KEY, body TEXT)"]}` |
| `GET /v1/admin/tenants` | the ids of the store's tenants, and those this server has open |
| `GET /v1/admin/tenants/:id` | its epoch, base, logs and last transaction |
| `POST /v1/admin/tenants/:id/sql` | one statement: `{"sql": "…", "params": {}, "latest": false, "objects": false}` |
| `POST /v1/admin/tenants/:id/base` | makes a new base now |
| `DELETE /v1/admin/tenants/:id` | deletes it |

A statement that reads (a query or an `EXPLAIN`, as the engine reads the
statement, not by its first word) runs on the server's copy, through what
cannot write; `"latest": true` looks at the store first. Any other
statement is a write. A write that another server beat answers 409: send
it again. Two servers started on one store answer for the same tenants.

## A blackboard for agents

A **blackboard** is a tenant that many agents share to work one job: tasks
they claim under a lease, findings they post for each other, and the
events of both in order. It is made from a template, and its tools are
calls of the server or an MCP server an agent is pointed at.

```bash
curl -H "Authorization: Bearer $ADMIN" $URL/v1/admin/tenants \
  -d '{"id": "migration", "template": "blackboard"}'
curl -H "Authorization: Bearer $ADMIN" $URL/v1/admin/tenants/migration/board/add_tasks \
  -d '{"agent": "lead", "tasks": [{"id": "lexer", "title": "port the lexer", "spec": "..."}]}'
curl -H "Authorization: Bearer $ADMIN" $URL/v1/admin/tenants/migration/board/claim \
  -d '{"agent": "worker-7", "key": "claim-1"}'

# an agent, named by the URL it is given
claude mcp add --transport http board "$URL/v1/admin/tenants/migration/mcp?agent=worker-7" \
  --header "Authorization: Bearer $ADMIN"
```

| tool | does |
|---|---|
| `add_tasks(tasks, key?)` | takes tasks in; one whose id the board has already is left as it is (a list with a task that has no id needs a `key`) |
| `claim(key, n?, lease_s?)` | gives the caller up to `n` tasks, oldest first: open ones, and claimed ones whose lease has passed |
| `heartbeat(task, lease_s?)` | extends the caller's lease; refused once another agent has the task |
| `complete(task, result, key)`, `fail(task, why, key)` | end the caller's claim; refused once another agent has the task, so a task is done once |
| `post(body, key, task?)` | records a finding, as a duplicate (`dup_of`) of an earlier one that says the same |
| `search(q, limit?)` | findings and tasks by their words, best first; a finding's duplicates are counted, not listed |
| `timeline(since?, limit?)` | what happened, in order: tasks taken in, claims, tasks done or failed, findings |
| `status()`, `get_task(id)` | the board in numbers; one task |

`POST /v1/admin/tenants/:id/board/:tool` takes a tool's arguments and
`"agent"` in its body. `POST /v1/admin/tenants/:id/mcp?agent=NAME` is the
same tools over the Model Context Protocol (Streamable HTTP, no session).
From Rust they are `chloedb::blackboard::Board`.

- **One winner.** A call that writes is one transaction, committed by the
  tenant's replacement of `HEAD`, so the store orders every claim: of two
  agents that claim the same task, through one server or two, one is given
  it.
- **Leases.** A claimed task is its owner's until its lease passes (300
  seconds unless asked, a day at most), by the clock of the server that
  answers. After that another agent's `claim` may take it, and the first
  agent's `heartbeat` and `complete` are refused.
- **Applied once.** A `key` is the caller's own name for one call. A call
  that failed or timed out may be sent again with the same `key`: if the
  first was committed, the second changes nothing and answers what the
  first answered. Keys are kept in the tenant, so any server knows them,
  and a key is its agent's: two agents that choose the same word are not
  answered each other's calls. Over HTTP and MCP the calls that are not
  the same when made twice (`claim`, `complete`, `fail`, `post`, and
  `add_tasks` of a task without an id) are refused without a key (400;
  over MCP, an invalid-parameters error). From Rust the key is the
  caller's choice. Keys are never deleted yet.
- **Duplicates.** Two findings say the same when they share at least 80%
  of their words (letters and digits, in any case). This is a comparison
  of words, not of meaning: the design's comparison by vectors is not
  built.

A refused call answers 409 over HTTP, and an answer marked `isError` over
MCP. A call that another server beat to the tenant is tried again by the
server, twice, before it answers 409.

## What it does not do yet

- It has run on a directory store, and its S3 code against a mock of S3
  (moto) only, not against a real bucket. A store must refuse a put whose
  condition fails (AWS S3, R2, MinIO, GCS and Azure say they do;
  DigitalOcean Spaces is not known): `chloedb-server tenants check-store
  URL` tries it.
- A base is always the whole database as one file, fetched whole when a
  node opens the tenant. Reading a large index by range, without a local
  copy, is the design's next step.
- A write waits for a round trip to the store, its own or the one it
  shares. Nothing is acknowledged before the store has it.
- Bulk loads and attached segments skip the log, so they are not committed
  through `write`: use SQL or transactions.
- The server has no per-tenant keys, quotas, routing or directory: every
  server on a store answers for every tenant, and two that write the same
  one take it from each other (each write that loses answers 409). An
  agent of a blackboard therefore holds an admin key of the server: run
  one only where that is acceptable, until tenants have keys of their own.
- The format is not frozen. Log objects are written as version 1 of
  `STRARC01`, the experimental one; the accepted format begins at 2, and
  its readers will not take what is written now for theirs.
