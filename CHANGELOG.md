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

<!-- Placeholder: entries for the storage-durability work go here. -->

### Query and indexes

<!-- Placeholder: entries for the query and vector work go here. -->

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
