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
| `write` | one conditional put (two the first time, when the handle becomes the writer; one more upload when the log is sealed) | |
| `make_base` | one upload of the database, one put | (the next open is two reads) |

A write fails in one of three ways. `NotWriter`: another node committed
first, nothing was written, and the handle has loaded the tenant again.
`InDoubt`: the store did not answer and could not be asked again, so the
write may or may not be there. Anything else: not committed.

## From the server

```bash
chloedb-server --db main.chloedb --tenants /mnt/tenants --tenant-cache /var/cache/tenants
# or --tenants s3://bucket/prefix/   (configured by the environment, as `import` reads S3)
```

The routes need an admin key until tenants have keys of their own.

| route | does |
|---|---|
| `POST /v1/admin/tenants` | creates one: `{"id": "agent-7", "sql": ["CREATE TABLE notes (id INT PRIMARY KEY, body TEXT)"]}` |
| `GET /v1/admin/tenants/:id` | its epoch, base, logs and last transaction |
| `POST /v1/admin/tenants/:id/sql` | one statement: `{"sql": "…", "params": {}, "latest": false, "objects": false}` |
| `POST /v1/admin/tenants/:id/base` | makes a new base now |
| `DELETE /v1/admin/tenants/:id` | deletes it |

A statement that reads runs on the server's copy; `"latest": true` looks at
the store first. A write that another server beat answers 409: send it
again. Two servers started on one store answer for the same tenants.

## What it does not do yet

- It has run only on a directory store. The S3 code compiles and has not
  been run against a bucket; a store must refuse a put whose condition
  fails (AWS S3, R2, MinIO, GCS and Azure say they do; DigitalOcean Spaces
  is not known).
- A base is always the whole database as one file, fetched whole when a
  node opens the tenant. Reading a large index by range, without a local
  copy, is the design's next step.
- A write is one round trip to the store, and one handle's writes wait for
  each other. There is no group commit.
- Bulk loads and attached segments skip the log, so they are not committed
  through `write`: use SQL or transactions.
- The server keeps every tenant it has opened open until it stops, and has
  no per-tenant keys, quotas, routing or directory.
- The format is not frozen.
