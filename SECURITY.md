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
- ChloeCtx (`chloectx/`), its daemon, MCP server and inspector, its
  Claude Code plugin (`chloectx/plugin/`), and `install.sh`;
- `chloedb-workbench`, and the server's wheels on PyPI
  (`packaging/server-wheels/`).

## What is supported

The latest release, 0.3.7 (16 November 2026), and `main`. Each release is
tagged per package: `chloedb-server-v<version>` (the server, with
`chloectx`, `chloedb-crawl`, `chloedb-sheets` and `chloedb-netops`
attached prebuilt),
`chloedb-py-v<version>` and `chloedb-js-v<version>`. Fixes go to `main` and
into the next release; released versions are not patched, so upgrade to
the release that has the fix. Report against `main` or the latest release.
The 0.1.0 releases of 2 to 6 November 2026 were published under ChloeDB's
old name, Strata (`strata-server-v0.1.0`, `strata-py-v0.1.0`,
`strata-js-v0.1.0`), and are not supported.
[docs/compatibility.md](docs/compatibility.md) says what a version
promises, and [CHANGELOG.md](CHANGELOG.md) what changed.

[The security model](docs/chloedb/security.md) says what each credential
of a server reaches, what is open without one, and what is left to the
operator: the network, the disks and the files the servers write.

## Checking what you download

- **Server archives** on GitHub each come with a `.sha256` from the same
  release (`sha256sum -c chloedb-server-linux-x86_64.tar.gz.sha256`), which
  `install.sh` and ChloeCtx's plugin check before they install.
- **Python and JavaScript releases** on GitHub attach a `SHA256SUMS` of
  their wheels and tarballs.
- **PyPI**: the `chloedb` and `chloedb-server` packages are published from
  this repository's workflows by trusted publishing, with attestations
  that name the workflow that built them.
- **npm**: the packages carry the registry's signatures
  (`npm audit signatures`).

The archives are not signed, and no release carries an SBOM yet.

## What counts as a vulnerability

- Anything that lets a client of `chloedb-server` do more than its
  credential allows: an API key acting beyond its scope (`feed`, `read`,
  `write`, `admin`, the roles `ops` and `propose`) or beyond the tables it
  is limited to, a signed-in person beyond what the allow list gives them,
  reaching the calls between the servers of a cluster without the
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

Each release lists the holes it closes, and what changes for a user, under
**Security** in [CHANGELOG.md](CHANGELOG.md): [0.2.0](CHANGELOG.md#020),
[0.2.1](CHANGELOG.md#021), [0.3.0](CHANGELOG.md#030),
[0.3.1](CHANGELOG.md#031) and [0.3.3](CHANGELOG.md#033) (the server's
connectors and keys, and the app engine). [0.3.5](CHANGELOG.md#035)'s
changes to auth are in its introduction: adding a server to a cluster
takes the cluster's token.
