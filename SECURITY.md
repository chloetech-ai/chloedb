# Security policy

## Reporting a vulnerability

Please report a vulnerability privately, through GitHub: open
[a private security advisory](https://github.com/chloetech-ai/chloedb/security/advisories/new)
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
  buffers, or makes a server fetch a URL its media allowlist refuses
  (the allowlist is checked against the host the server connects to).
- A query that makes a server read a file outside its models directory,
  or send one of its environment variables other than a model's key
  (a provider's, or a `CHLOEDB_MODEL_KEY_*` one) to an endpoint.
- Stored data that a browser runs as a page of the server's origin (a
  file served by `GET /v1/media/<sha256>` is a download or an image, a
  sound or a video, never sniffed, and sandboxed).
- An app (the crawler, sheets, the app engine) reached through a server
  acting beyond what that server's credential allows.
- ChloeCtx exposing one project's memory to another, its inspector
  answering without its access token, or `chloectx app` answering without
  its token when one is set.

A server started with no token and no key is open by design: it is for a
machine of one's own, and refuses to listen beyond loopback.
Denial of service by an authenticated client (an expensive query, a large
import) is a bug to report as an issue, not an advisory.

## Holes closed

Each release lists the holes it closes under **Security** in
[CHANGELOG.md](CHANGELOG.md), with what changes for a user. Since 0.2.0
(not yet released):

- `chloectx app` served any caller with no token on any address;
- `chloectx app` started a thread per request and per exchange, without
  bound;
- `chloectx sync` sent transcripts and its key over plain HTTP to another
  machine, and two syncs at once shared one header file;
- the ChloeCtx inspector had no Content-Security-Policy;
- `install.sh` built ChloeCtx without `--locked`;
- `chloedb-sheets` ran a SQLite file's views with no deadline, and read an
  xlsx file of any expanded size.
