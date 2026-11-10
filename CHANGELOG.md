# Changelog

What changed in ChloeDB (formerly Strata) and its packages, newest first.
Each release says which artefacts it covers: `chloedb-server`, the Python
package (`chloedb`), the JavaScript package (`chloedb`), the Java package,
ChloeCtx. [docs/compatibility.md](docs/compatibility.md) says what a version
promises about the file format, the shard protocol and the old names.

Version numbers are not yet bumped per change: every crate and package is
still 0.1.0, and the next release will name its version here.

## Unreleased

### Storage

- Opening no longer falls back silently past a damaged newest version (a
  damaged meta slot or manifest, or a file cut short) when that would lose
  acknowledged writes: it fails with an error naming the file, the damage,
  what would be lost and how to recover, and changes no file. A log whose
  header is damaged but whose commits are intact, or a log from another
  database, is refused the same way. Before, one flipped byte could
  reopen an older version without a word.
- `recover` (Rust `DbOptions`, Python `chloedb.open(..., recover=True)`,
  `chloedb-server --recover`) accepts the loss on purpose: it keeps a copy
  of the damaged file as `<path>.damaged-<time>` first, and `damaged()`
  (Python `Database.damaged`, with a `RuntimeWarning`) says what was lost.
  A server opened that way answers `degraded` on `/v1/health`, and
  `/v1/admin/check` fails.
- A damaged column fails the query that reads it instead of reading as
  NULL; a checkpoint refuses a segment whose checksum is wrong instead of
  copying it with a fresh one.
- A failed sync of the database file poisons the handle, as a failed log
  append already did, so no later commit or merge can claim pages that
  never reached the disk.
- The writer's lock is held on the file itself, so a symbolic link, a
  hard link or a deleted `-readers` directory no longer lets a second
  writer open it.
- `close()` flushes, so after a clean close the file alone is a complete
  copy of the database. Python adds `flush()` and `checkpoint(dest)`;
  docs/chloedb/python.md and index.md say how to back a database up.
- Not yet: a log deleted outright cannot be detected, and there is no
  point-in-time recovery.

### Query and indexes

- A ranked query that stops at the default 100 rows says so: its stats
  carry `implicit_limit: 100` and an exact `truncated` (the engine ranks one
  more row to know), a plan line says it, and the Python and JavaScript
  results expose both. `DELETE ... ORDER BY <score>` without `LIMIT` is
  refused: before, it deleted the top 100 and left the rest.
- Fused rankings (`rrf`, weighted sums): the candidates follow the page's
  `LIMIT`, not its `OFFSET`, so pages of one size are pages of one ranking
  and no row repeats or goes missing between them. A page beyond the
  candidates comes back short, flagged `truncated`; raise `depth` (or use
  `exact => 1`) to page further. This replaces the deeper re-ranking of
  later pages, which made pages disagree.
- `ppr` gives the same scores when every edge weight is scaled alike, so
  counts work as weights (weights of 1,000 used to return nothing at the
  default `eps`); `max_pushes_reached` is reported when a walk stops early.
- Vectors: an unfiltered segment small enough to scan is searched exactly,
  the cost model scales with dimension, and the default `ef` is 200
  (was 64): on 20,000 random 64-dimension vectors the default now finds
  the true top 10 (was 73%), on 200,000 it finds 71% (was 40%). `distance()`
  is now the Euclidean distance (it was the dot product, reversed); a plan
  that uses it names the plan feature `distance`, which older shards refuse.
- Models: the known models are fetched at a pinned Hugging Face revision and
  each file's SHA-256 is checked; a table created with a model records a
  fingerprint of its weights and refuses different weights under the same
  name; a write that needs a model which is not open fails (it used to store
  the row without a vector) unless `set_embed_later(true)` says to fill the
  vectors in later, which chloedb-server does.
- Not yet: a per-field metric option (cosine, dot, L2), and a fingerprint
  recorded on a model's first open for tables created before this change.

### Replication

<!-- Placeholder: entries for the replication work go here. -->

### Clients (Python, JavaScript)

- Writes through a client answer their sequence numbers. In Python a
  write's count is a `chloedb.Written`, an `int` with `seq_no` (a server's)
  and `seq_nos` (a coordinator's, one per shard); in JavaScript
  `client.writeResult(ops)` answers `{written, seqNo, seqNos, skipped}`,
  and `write` still answers a number.
- Reads take `min_seq_no` / `min_seq_nos` (`minSeqNo` / `minSeqNos` in
  JavaScript): `query`, `query_arrow`, `search`, `get`, `get_many`,
  `scroll`, a read by `sql`, a read plan by `run`, `sid_next`.
- `client.session()`, a client whose reads see its own writes: it sends
  the latest numbers of its writes with each read, through a coordinator
  or from a replica.
- Errors have a stable `code` (`bad_request`, `not_found`, `conflict`,
  `partial_write`, `busy`, `overloaded`, `timeout`, `unavailable`, ...),
  from the server's JSON `code` when it gives one, and the common codes
  are raised as subclasses of `ChloeDBError` (`ConflictError`,
  `BusyError`, ...); `except ChloeDBError` and `instanceof ChloeDBError`
  still catch every one. In-process Python errors carry the same codes.
- A Python `Database` raises `TypeError` for a client's options
  (`timeout_ms`, `idempotency_key`, `keep_alive_ms`) instead of binding
  them as query parameters, and takes `min_seq_no(s)` as no-ops, so code
  moved from `chloedb.connect` to `chloedb.open` fails loudly rather than
  losing its deadline or idempotency. A query parameter of such a name is
  passed in `params`.

### Build and release

- GitHub Actions pinned to commits (Dependabot keeps them current); every
  workflow has a least-rights `permissions:` block; cargo, maturin and the
  JS addon build with `--locked`; the Python and JavaScript releases attach
  a `SHA256SUMS`.
- [docs/compatibility.md](docs/compatibility.md): the file formats this
  code reads and writes, the shard protocol, and what a version promises.

## Since the 0.1.0 releases

`strata-js-v0.1.0` (2 November 2026), `strata-py-v0.1.0` (5 November) and
`strata-server-v0.1.0` (6 November) were released under the old name. In
the weeks since, in broad strokes:

- Strata became ChloeDB and Engram became ChloeCtx: crates, programs,
  packages (`import chloedb`, `chloedb` on npm, `ai.chloetech.chloedb`),
  settings (`CHLOEDB_*`), headers and metrics carry the new names, and the
  old ones are still accepted for a release (see
  [AGENTS.md](AGENTS.md) for what keeps the old name for now).
- Clusters: following another cluster (a copy feed and catching up),
  switching over to a follower, stream offsets that move with their slots,
  failover elections, and scrolls through a coordinator; the shard protocol
  is now 18.
- Storage and writes: many writers at once sharing commits, embedded
  databases that merge as they write, a compact primary-key index, and a
  checksum per doc-store block.
- Queries: GROUP BY and aggregates read from columns, MEDIAN and
  percentiles, date functions, vector partitions for filtered vector
  search.
- Operations: the command center (alerts, on-call, the operator's view),
  the chaos tests, the app engine as a cluster app, and prebuilt Python
  wheels and JavaScript tarballs per platform.
