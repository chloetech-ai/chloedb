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

- **Server archives** on GitHub (`chloedb-server`, `chloectx`,
  `chloedb-crawl`, `chloedb-sheets` and `chloedb-netops`, one per
  platform) each come with a `.sha256` from the same release
  (`sha256sum -c chloedb-server-linux-x86_64.tar.gz.sha256`), and from
  0.3.8 with GitHub's build provenance: a signed statement, logged in
  Sigstore's public transparency log, that this repository's
  `server-release.yml` built the archive from the release's tag on
  GitHub's runners. A `.sha256` says the download was not damaged; the
  provenance says who built it. Check it with [GitHub's CLI](https://cli.github.com)
  (a recent one, whose `gh attestation verify --help` lists `--source-ref`):

  ```bash
  gh attestation verify chloedb-server-linux-x86_64.tar.gz \
    --repo chloetech-ai/chloedb \
    --signer-workflow chloetech-ai/chloedb/.github/workflows/server-release.yml \
    --source-ref refs/tags/chloedb-server-v0.3.8
  ```

  That asks GitHub for the attestation, and needs `gh auth login`. Each
  archive's Sigstore bundles are beside it too
  (`chloedb-server-linux-x86_64.tar.gz.sigstore.jsonl`): add
  `--bundle chloedb-server-linux-x86_64.tar.gz.sigstore.jsonl` to check it
  from that file without signing in. The website's installer
  (`docs/install.sh`), the repository's `install.sh` and ChloeCtx's
  plugin check the SHA-256 always, and from 0.3.9 the provenance too
  where `gh` is installed: an archive whose provenance does not verify is
  not installed, even when its `.sha256` matches. Without `gh` they say so
  and install on the SHA-256; `CHLOEDB_VERIFY=require` (the plugin: `CHLOECTX_VERIFY`)
  fails instead, and `=off` skips the check.
- **What is in them.** Each archive has a CycloneDX SBOM beside it
  (`chloedb-server-linux-x86_64.cdx.json`: every crate compiled into the
  program, with its version, licence and the checksum Cargo.lock pins) and
  `THIRD-PARTY-NOTICES.md` inside it (their licences). The SBOM is attested
  too, as the archive's: add `--predicate-type https://cyclonedx.org/bom` to
  the command above to check it, and run the same command on the
  `.cdx.json` to check that file. `SHA256SUMS` lists every file of the
  release.
- **Python and JavaScript releases** on GitHub attach a `SHA256SUMS` of
  their wheels and tarballs.
- **PyPI**: the `chloedb` and `chloedb-server` packages are published from
  this repository's workflows by trusted publishing, with attestations
  that name the workflow that built them. Check a downloaded wheel with
  [pypi-attestations](https://pypi.org/project/pypi-attestations/):

  ```bash
  pip download --no-deps chloedb==0.3.7
  uvx pypi-attestations verify pypi --repository https://github.com/chloetech-ai/chloedb \
    pypi:chloedb-0.3.7-cp310-abi3-manylinux_2_28_x86_64.whl
  ```

  (the file `pip download` saved; `chloedb-server` the same way). From
  0.3.8 every wheel carries the licence in its `dist-info/licenses`, and
  `chloedb-server`'s the third-party notices too.
- **npm**: the packages carry the registry's signatures
  (`npm audit signatures`).

Releases before 0.3.8 have no build provenance and no SBOM: their
archives are checked by their `.sha256` alone. The installers check
0.3.8's archives by their `.sha256` too, and its provenance with
`gh attestation verify` as above.

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
