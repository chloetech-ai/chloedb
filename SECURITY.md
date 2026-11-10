# Security policy

## Reporting a vulnerability

Please report a vulnerability privately, through GitHub: open
[a private security advisory](https://github.com/chloetech-ai/strata/security/advisories/new)
on this repository. Don't open a public issue or pull request for it.

Say what is affected (see the parts below), the release or commit you
tested, and how to reproduce it. The report and the fix are discussed in
the advisory, which is published once a fix has merged.

The parts this repository ships:

- ChloeDB, the library (`chloedb/`), and the database files it reads;
- `chloedb-server`: the HTTP and gRPC server, the coordinator, replication
  and following, the command center and query UI, the Kubernetes operator,
  and its container image;
- the Python package (`chloedb`, with its HTTP client), the JavaScript
  package (`chloedb`, with `chloedb/client`) and the Java package and JDBC
  driver;
- `chloedb-crawl` and `chloedb-sheets`, and the app engine
  (`chloedb-apps/`) with its image;
- ChloeCtx (`chloectx/`), its daemon, MCP server and inspector, and
  `install.sh`.

## What is supported

Three releases have been published, all under ChloeDB's old name, Strata:
`strata-js-v0.1.0` (2 November 2026), `strata-py-v0.1.0` (5 November) and
`strata-server-v0.1.0` (6 November). Fixes go to `main` and into the next
release; released versions are not patched. Report against `main` or the
latest release. [docs/compatibility.md](docs/compatibility.md) says what a
version promises, and [CHANGELOG.md](CHANGELOG.md) what changed.

Server releases attach a `.sha256` per archive; Python and JavaScript
releases from now on attach a `SHA256SUMS` of their wheels and tarballs,
to check a download against.

## What counts as a vulnerability

- Anything that lets a client of `chloedb-server` do more than its
  credential allows: an API key acting beyond its scope (`read`, `write`,
  `admin`), reaching the calls between the servers of a cluster without the
  cluster's token, or getting that token.
- Reading or changing data without a credential on a server that has one.
- A crafted database file, write-ahead log, model file, import (SQLite,
  CSV, JSON, Parquet) or request that corrupts memory, reads outside its
  buffers, or makes a server fetch a URL its media allowlist refuses.
- An app (the crawler, sheets, the app engine) reached through a server
  acting beyond what that server's credential allows.
- ChloeCtx exposing one project's memory to another, or its inspector
  answering without its access token.

A server started with no token and no key is open by design: it is for a
machine of one's own, and refuses to listen beyond loopback.
Denial of service by an authenticated client (an expensive query, a large
import) is a bug to report as an issue, not an advisory.
