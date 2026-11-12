# Changelog

What changed in ChloeDB (formerly Strata) and its packages, newest first.
Each release says which artefacts it covers: `chloedb-server`, the Python
package (`chloedb`), the JavaScript package (`chloedb`), the Java package,
ChloeCtx. [docs/compatibility.md](docs/compatibility.md) says what a version
promises about the file format, the shard protocol and the old names.

Every crate and package carries the release's version; between releases
changes collect under "Unreleased".

## Unreleased

### Security

- ChloeCtx: **breaking:** `chloectx app` refuses to start on an address
  other than loopback (`--host 0.0.0.0`, say) with no `--token`, since
  whoever reached the port could read and write every space and start
  model calls billed to the app's key; `--insecure` starts it anyway. The
  operator passes `--insecure` for a cluster without a `tokenSecret`
  (which is open itself), and the app's token otherwise, as before. The
  app also serves 64 requests at once (one more is answered 503) and runs
  at most four distillations, with 256 waiting; past that an exchange is
  stored without notes and `/v1/info` counts it under `distill.skipped`.
  Before, each request and each exchange started a thread, without bound.
- ChloeCtx: **breaking:** `chloectx sync` refuses a server reached over
  plain `http://` anywhere but this machine (`127.0.0.1`, `::1`,
  `localhost`), since it sends transcripts and the server's key;
  `CHLOECTX_SYNC_ALLOW_HTTP=1` allows it. Each request's header file (the
  key, owner-only) is its own, so a push every 30 s and an explicit
  `chloectx sync` no longer remove each other's.
- ChloeCtx: the inspector (`chloectx ui`) is served with a
  Content-Security-Policy that runs only its own script (by hash), loads
  nothing from elsewhere and connects only to itself, and with
  `X-Frame-Options: DENY`.
- `install.sh` builds ChloeCtx with `--locked`: the dependency versions in
  `chloectx/Cargo.lock`, the ones CI tested, not newer ones the registry
  offers on the day.
- `chloedb-sheets`: a SQLite file's views are listed but not counted, and
  come into a workbook only when named (the UI leaves them unticked): a
  view runs its query as it is read, and a file's can be a query that
  never ends, which held a server thread for good. Reading a SQLite file
  stops after 60 s for its list of tables and 180 s for an import, and the
  file is opened with `trusted_schema` off. An xlsx file is refused when
  its parts expand past 512 MB (counted by decompressing them, not by the
  sizes the file declares) or it holds more than 2 million cells, as CSV
  and SQLite imports were bounded already.

### Storage

- A SPARSE value whose term holds the character `\0` is refused when it
  is written. It was accepted, and the next flush panicked building the
  field's dictionary: on the flusher thread, which ended for good, and
  again at each reopen, which replays the row. A term of that kind in a
  row written before is left out of the index.
- A panic in background work is that round's error, tried again as an
  error is, and no longer ends the thread: the build of a background
  flush, a merge on a merger thread, the cold tier's offload and eviction,
  and a read-only database's refresh.
- A checkpoint (a backup, the copy a replica is seeded from) of a database
  where a column references a table created after its own table
  (`ALTER TABLE .. ADD COLUMN .. REFERENCES`) failed with "references ..,
  which is not a table". It copies such a database.
- Semantic IDs: a background flush keeps the tie numbers of rows deleted
  before it. It dropped them with the rows, so the next row under the
  same levels was given a deleted row's whole code again: a code named
  two rows over time, which the tie exists to prevent. A foreground flush
  and a merge already kept them.

### Query and indexes

- **Breaking:** on the operators engine, a `LIMIT` over a join, `DISTINCT
  ON` or `MAX k PER` whose first table holds more than 1,000,000 rows is
  an error when the rows it wants are not among the first 1,000,000 read,
  as the same query without its `LIMIT` is. It answered a short page,
  down to no rows at all, without a word.
- A plan from elsewhere (`Db::run`, `POST /v1/plan`) with a NOT over a NOT
  or over `!=` matched the rows whose field is NULL: `not(not(price > 5))`
  returned rows without a price. Each NOT is SQL's now, as in SQL text,
  which was not affected.
- The filter of `json_paths(.., filter => ..)`, `Db::json_paths` and
  `Db::sid_next` is one condition, parsed by itself. Its text was put
  into a statement's, so `'x = 1 LIMIT 0'` was taken as a filter.
- Semantic IDs: `chloedb.train.codebook(balanced=True)` (Rust's
  `TrainOptions.balance`, JavaScript's and Java's `balanced`) trains each
  level of a codebook with every centroid given the same number of the
  training vectors. Plain k-means uses the later levels' numbers
  unevenly; on Amazon Beauty's 12,101 items, balanced, the third level's
  256 numbers are all in use alike and the items sharing a code go from
  3,288 to 2,601. Off by default, and a codebook's file is as before.
  `chloedb.train.codebook_report` (`Codebook::report`,
  `Db::codebook_report`) says what a codebook makes of a set of vectors:
  the distinct codes, the rows sharing one, and each level's use.
- Sequence models: `chloedb.train.sequence(logq=True, uniform_negatives=n)`
  for catalogs too large to train against every item. With the other
  targets of a batch as the negatives the model learns to rank popular
  items low; `logq` takes that out of each negative's score and
  `uniform_negatives` adds items drawn evenly. On Yambda-50M's 629,298
  tracks, held-out Recall@10 after four epochs goes from 0.0229 to
  0.0334. Both are off by default.
- CI's recommender gate checks semantic IDs: a plain and a balanced
  codebook on MovieLens-1M, the codes a table gives against
  `codebook_report`, and the report against the fixtures.

### Replication

- **Breaking:** a database that numbers its writes (`--seq-no`) refuses a
  bulk load (`Db::load`, `Database::bulk_load`) and `attach_segments` once
  it has numbered a write or replicates: they write no log record, so its
  replicas never got the rows. Before that they still seed it. Imports
  (`POST /v1/import`, `chloedb-server import`, SQLite, Parquet and Arrow)
  into such a database upsert in transactions of 10,000 rows, also into an
  empty table: a bad row then stops the import after the transactions
  before it, and a key the file holds twice keeps its last row.

## 0.2.0

The first release under the new names: `chloedb-server-v0.2.0`,
`chloedb-py-v0.2.0` and `chloedb-js-v0.2.0` (the Java package and ChloeCtx
are built from source). It has breaking changes, each marked below: a
memory budget on embedded queries, writes that need a model which is not
open, short pages past a fused ranking's candidates, model endpoints'
`key_env` names, and stored SVG and HTML served as downloads. The shard
protocol is 20; servers of protocols 18 and 19 still talk to it.

### Security

- `chloedb-server`: media fetches (`--media-fetch`) check the host the
  HTTP client connects to. The allowlist is compared with the URL as the
  client parses it, before the request and at each redirect, and a URL
  that URL parsers could read differently is refused: one with a
  backslash or a control character (a tab, a newline), a host with
  characters other than letters, digits, `-`, `_` and `.` (percent-encoded
  or not in its `xn--` form), or a host ending in a number that is not a
  plain IPv4 address. Before, a wildcard entry (`*.example.com`) could be
  passed by a URL whose host the server fetched from was not under it.
- `llm()` takes a model's name, never a path: letters, digits, `_`, `-`
  and `.`, two such names joined by `/` at most, as `cross_encoder()`
  takes. An absolute name or `..` is refused, so a query reads no
  `endpoint.json` outside the models directory.
- **Breaking:** an `endpoint.json`'s `key_env` names a provider's key
  variable (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, ...) or one whose name
  starts with `CHLOEDB_MODEL_KEY_`; any other name is refused when the
  model is opened, so an endpoint is never given another of the server's
  variables (its tokens, a cloud's credentials). Rename a variable such
  as `GPU_BOX_KEY` to `CHLOEDB_MODEL_KEY_GPU_BOX`.
- `GET /v1/media/<sha256>` serves a stored file so that a browser never
  runs it on the server's origin: images (PNG, JPEG, GIF, WebP, BMP,
  AVIF), sounds and videos with their type; a PDF as a download; any
  other file, whatever type its writer gave (HTML, SVG, XML, JavaScript),
  as a download of `application/octet-stream`; every answer with
  `X-Content-Type-Options: nosniff` and
  `Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; sandbox`.
  An SVG in an image field is no longer shown inline by the command
  center's media views.
- ChloeCtx: `memory_sql` and the inspector's query box run one parsed
  `SELECT`, not any text that begins with `select`, and refuse `llm()` and
  `cross_encoder()`, which send the rows they read to a model with the
  daemon's keys. Each runs with a deadline (2 s for the tool, 10 s for the
  inspector), a memory budget and a row cap (1,000 and 50,000). The query
  engine's model-written conditions refuse the same functions.
- ChloeCtx: the distiller's `claude -p` runs in `distill/` in the stores'
  folder, private to the user, instead of `/tmp/chloectx-distill`, from
  which another local user's project settings (a hook) would have run as
  this one. Its answer is read while it is written, so one over 64 KB no
  longer blocks the call until its timeout.
- ChloeCtx: the short socket path is used only in a directory of the
  user's own that no one else can open: a `/tmp/engram-<uid>` another user
  made first, or a link, is skipped for `~/.cache/chloectx`.
- ChloeCtx: `fetch-model` downloads the commit ChloeDB pins and checks each
  file's SHA-256 before it is renamed into place; a download cut short is
  fetched again by the next `install.sh`, which now always runs it.
- ChloeCtx: `install` quotes the binary's path in hook commands (a path
  with a space ran something else), and `doctor` runs the program as the
  hook command spells it. Settings files are written beside and renamed
  into place, keep their keys' order and their links, and a project
  install backs up the files it changes.

### Storage

- Opening no longer falls back silently past a damaged newest version (a
  damaged meta slot or manifest, or a file cut short) when that would lose
  acknowledged writes: it fails with an error naming the file, the damage,
  what would be lost and how to recover, and changes no file. A log whose
  header is damaged but whose commits are intact, or a log from another
  database, is refused the same way, and so is a first page of zeros
  (slot 0 is written when a database is created, so zeros there are
  damage), and so is a zeroed slot 1 where the file shows a commit wrote
  it: slot 0 holds a version past the one the database was created with,
  or the log starts past the transactions after that version, or the file
  has pages past it and no log, as a checkpoint has until it is opened.
  A newer version that changed tables (created, altered or dropped one)
  or began numbering writes is never taken for one the log covers, since
  none of that is logged: falling back past it lost the table and the rows
  logged into it. Before, one flipped byte could reopen an older version
  without a word.
- Without `sync`, a log whose header is zeros before intact commits is
  what a power loss leaves when the header never reached the disk: it is
  dropped (copied to `<wal>.damaged`, and `damaged()` says so), as a power
  loss without sync may, instead of being refused as damaged. With `sync`
  the header is durable before any record, so zeros there are refused.
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
- On Linux the writer's lock is held on the file itself, so a symbolic
  link, a hard link or a deleted `-readers` directory no longer lets a
  second writer open it. Not on macOS, where `flock` and `fcntl` locks
  are one kind and the readers' shared lock would refuse it: there the
  writer is still the one holding `-readers/WRITER`.
- `close()` flushes, so after a clean close the file alone is a complete
  copy of the database. Python adds `flush()` and `checkpoint(dest)`;
  docs/chloedb/python.md and index.md say how to back a database up.
- A write-ahead log deleted outright while it held acknowledged
  transactions is refused at open (read-only too), naming the log, the
  transactions committed after the version on disk and `recover`, where
  the database used to open at that version without a word. The file is
  not written between versions, so the writer keeps a log record at byte
  24 of `<path>-readers/WRITER` (`WRITER` in the directory layout): the
  database's id and the version's txn, written and synced before the
  first append after a version, after a version made while the log holds
  transactions it does not, and at an open that replays a log. Versions
  before it read and write only the first 24 bytes, so it is compatible
  both ways. Only a missing log file beside no sealed log is refused; a
  clean close empties the log, so deleting it afterwards still opens, and
  so does a checkpoint renamed into place (its database id is its own).
  `recover` opens the version on disk, `damaged()` says what was lost, and
  the record is cleared. A tool that deletes a log on purpose deletes
  `-readers` with it (`chloectx-bench` does).
- Not yet: a log deleted together with `-readers` (or `WRITER`), a copy of
  the file opened elsewhere without its log, or a log deleted while a
  background flush holds a sealed one, cannot be detected; nor can a
  zeroed slot 1 that held the only version made since creation beside an
  empty log, which looks like a crash during that first commit; there is
  no point-in-time recovery.
- A damaged freelist whose counts or extents overflow when added up is
  refused as malformed; decoding it panicked before (found by the nightly
  fuzzer).

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
  later pages, which made pages disagree (up to 85 of 220 rows repeated
  in the audit's test; none now).
- `ppr` gives the same scores when every edge weight is scaled alike, so
  counts work as weights (weights of 1,000 used to return nothing at the
  default `eps`); `max_pushes_reached` is reported when a walk stops early.
  Its rankings move on MovieLens-1M (the recommender gate's fixtures):
  Recall@100 0.568 to 0.577 and coverage@10 0.528 to 0.475 leaving one
  out, Recall@10 0.059 to 0.062 on the time split, where it now beats
  both baselines.
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
  vectors in later, which chloedb-server does. This holds for every write
  path: SQL, `load`, Python and JavaScript inserts, and the Arrow, SQLite
  and file imports. **Breaking for embedded use:** code that wrote rows
  without opening its model (or before the model was downloaded) now gets
  an error; open the model (`models=DIR`), give the vectors, or call
  `set_embed_later(True)` and run `embed_backfill` once the model is there.
- Not yet: a per-field metric option (cosine, dot, L2), and a fingerprint
  recorded on a model's first open for tables created before this change.
- A database opened in a program (Rust's `Db`, and the Python, JavaScript
  and Java packages) counts its queries against a memory budget the whole
  process shares: a quarter of the machine's memory (its cgroup limit, else
  RAM), or `CHLOEDB_QUERY_MEMORY_MB` MiB (0: none). A query past it fails
  with a `memory:` error naming the budget and how to raise it, and gives
  back what it held, where it used to grow until the process ran out of
  memory (`SELECT *` of a large table, a `GROUP BY` with a group per row, a
  percentile over every row). Such a database still sets no deadline, row
  window or group cap: a server's are for answers sent over the network,
  and a program reading a whole table into a frame or a scroll has no way
  past them; `max_rows` and `max_groups` stay options. `ExecOptions::limited`
  gives one query a deadline, a budget of its own and a cancel flag,
  `Interrupts` cancels the queries running from another thread, and
  `Db::sql_with`, `Db::run_with` and `Db::query_leaf_with` take options as
  `Db::query_with` does. chloedb-server keeps its own budget
  (`--query-memory-mb`), whose refusals now name that flag. **Breaking for
  embedded use:** a query that needed more than a quarter of the machine's
  memory now fails; set `CHLOEDB_QUERY_MEMORY_MB` (0 for no budget), give
  the database or the query a larger `memory`, or `db.exec.memory = None` in
  Rust.

### Replication

- A shard's copies change one at a time, and only by a primary that a
  majority of its current copies answered within its lease and that a
  majority already follows (it waits up to 3 s for that before refusing
  with 503). Before, an admin call could give a primary cut off from its
  majority a new set of copies while the other side elected its own
  primary, and both acknowledged writes. `accept_data_loss` (in
  `Configure` and on both admin routes) overrides the check on purpose and
  is logged as an error. Memberships are versioned and sent with every
  `Replicate`; a voter refuses a candidate whose membership is older than
  its own.
- A new primary commits a no-op transaction of its own term before it
  acknowledges anything, so it never acknowledges an older term's
  transaction by counting copies (Raft's "figure 8"). The simulator has a
  new bug to find, `acks-an-older-term`.
- A write remembers the term its primary took it in, and is refused
  unapplied, answered "in doubt" (409), once the database has left that
  term; the check is repeated under the writer's lock, which shipped
  transactions also take. Before, a write stalled during a handover could
  commit after the new primary's first transaction with the same term and
  sequence number as a different transaction, leaving copies apart.
- Writes through a coordinator that span shards are checked on every
  shard first (protocol 19, `WriteRequest.validate`): an insert of an
  existing key, a failed `if_seq_no`, a type or NOT NULL error on one
  shard now fails the whole write with nothing applied, naming the
  operation. A shard whose writer stays busy for 100 ms, or that holds a
  newer slot map, is not checked, and its part goes straight to its write
  queue. A race remains between the check and the apply; see "When a
  write fails" in docs/chloedb/server.md.
- Every write and statement through a coordinator carries an idempotency
  key from its first send, so the coordinator's own resends never apply
  it twice; refused writes carry an `x-chloedb-write` code (`not-applied`,
  `in-doubt`) that the coordinator reads instead of the error's words;
  `--idempotency-ttl-s` sets how long keys are kept.
- A copy holding a different transaction than its primary's under the
  same term and sequence number is found on the `Replicate` that follows
  it (protocol 20, `ReplicateRequest.prev_digest`): each call carries a
  digest of the transaction it follows, the replica compares its own,
  and on a mismatch applies nothing, serves no reads, logs an error and
  says `differs` (health `degraded`, `txn_mismatches`,
  `chloedb_replica_txn_mismatches_total`); the primary sends it a copy of
  itself, which drops its own. A replica's word that it holds a run of
  the primary's history no longer counts toward a write's acknowledgment
  until a call that follows it applies, and that call always carries the
  primary's digest (a log that no longer holds the transaction sends a
  copy instead). Peers of an older protocol apply
  by term and number as before. A replica found to differ keeps that
  beside its database (`differs` in `<db>.replication`), refuses calls
  and campaigns until it installs a copy. The simulator has two bugs to
  put in, `commits-after-stepping-down` (a stalled writer) and
  `trusts-the-same-term`.
- A copy decided for a replica is always sent: a shipper whose primary was
  given other replicas between deciding a copy and sending it left the
  replica marked as being sent one, and the replica's next shipper waited
  for that copy for ever; and a copy that fails after the primary was
  given other replicas is sent again by the next shipper, rather than
  shipping batches to a replica that holds none of the primary's rows.
- Not yet: the digest covers the one transaction a call follows, not the
  history before it, and only while the replica's ship log holds it (a
  replica restarted after a flush, or copied since, before it was found,
  and a replica whose last is a primary's last that its log does not
  hold, are left to the per-slot digest round, `--verify-every-s`); a
  replica ahead of its primary under the primary's own term is still left
  to an operator; writes across shards are not atomic (no two-phase
  commit).

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
- A database in the process takes query limits, and can be stopped from
  another thread. Python: `timeout_ms` (a deadline, as a `Client`'s query
  takes it, raising `DeadlineError`) and `memory` (a budget in bytes; 0:
  none) on `query`, `query_arrow`, `scroll`, `sql`, `search` and `run`, the
  same on `chloedb.open`, `Database.open`/`create` and `set_limits` for
  every query of the database, and `Database.interrupt()`, as
  `sqlite3.Connection.interrupt`. JavaScript: `{timeoutMs, memory, signal}`
  (an `AbortSignal`) on `query`, `sql`, `run` and `search`, `timeoutMs` and
  `memory` on `open` and `setLimits`, and `db.interrupt()`. Java:
  `QueryOptions` on `query`, `execute` and `SearchOptions.limits`,
  `Database.Options.timeoutMs`/`memory`, `setLimits` and `interrupt()`; the
  JDBC driver's `Statement.setQueryTimeout` now holds (it was kept and
  ignored) and `Statement.cancel()` works (it threw). `timeout_ms` to a
  Python `Database` is no longer the `TypeError` of the change above except
  where nothing runs a query (`plan`). The HTTP clients are unchanged, but
  for a `search` that passes `timeout_ms`/`timeoutMs` on to their `query`.
  **Breaking:** a Python query parameter passed as the keyword `memory` is
  now the query's budget, not a parameter; pass it in `params`.

### ChloeCtx

- `chloectx sync` keeps a project's memory on a ChloeDB server or cluster
  as well as in its local store: the daemon pushes what it records every
  30 s and as it exits, and when it starts it pulls the records the store
  lacks, so a fresh cloud container or another machine recalls what
  earlier sessions stored. `sync setup --url URL` with the key from stdin
  (`--token-stdin`) or a variable (`--token-env`); `status`, `push`,
  `pull`. See "Keep the memory on a ChloeDB server" in
  docs/chloectx/setup.md.
- Redaction catches the secrets it used to miss: `DB_PASSWORD=…` and
  other names that end in a word for a secret (in env dumps, `.env`
  files, YAML and JSON), passwords in URLs and `curl -u`, `Authorization`
  headers, JWTs, and the keys of Stripe, AWS, Slack, Hugging Face,
  DigitalOcean, npm, GitLab, Google and ChloeDB itself. Each is replaced
  by a marker naming its kind (`[REDACTED:password]`, where it was
  `[REDACTED]`), on every path into the store (prompts kept in
  `state.json` and `events.log` too) and again as `chloectx sync` sends
  and pulls. `chloectx redact [--all] [--dry-run]` rewrites a store
  recorded before: its records, session state and log, the sync server's
  copies, and the file itself, so the old versions leave its free pages.
  See "Secrets and `chloectx redact`" in docs/chloectx/setup.md.

### Build and release

- GitHub Actions pinned to commits (Dependabot keeps them current); every
  workflow has a least-rights `permissions:` block; cargo, maturin and the
  JS addon build with `--locked`; the Python and JavaScript releases attach
  a `SHA256SUMS`.
- [docs/compatibility.md](docs/compatibility.md): the file formats this
  code reads and writes, the shard protocol, and what a version promises.
- A release tag builds and publishes nothing unless its commit is on main
  and CI's whole matrix (the nightly run, or one started by hand) passed
  on it (`release-gate.yml`); the release workflows check that the binary
  starts and leave the tests to CI. The server's image publishes `latest`
  only after CI passed on this repository's main, and `<version>` from a
  release tag.
- A nightly run that fails opens an issue (`nightly-failure`); the fuzz
  targets build again and run every night on a kept corpus.
- Dependabot watches every lockfile, and `cargo deny` checks advisories
  and licenses daily (`deny.toml`); zstd and its crates moved to versions
  without the open advisories.

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
