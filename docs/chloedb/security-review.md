# Security review pack

This page is for the person who has to approve ChloeDB before a team runs it: a security engineer, a GRC lead, a platform owner. It collects in one place what a review asks for. It covers what is protected and from whom, where the trust boundaries are, what ChloeDB does at each one and what it leaves to you, how its controls support the SOC 2 criteria a review usually maps to, and what ships in each package. The details are in the [security model](security.md), which this page links section by section. To report a vulnerability, see [SECURITY.md](../../SECURITY.md).

ChloeDB is software you run. There is no hosted ChloeDB service holding your data, so this page describes the product's controls. Your organization's own controls are what an audit covers: who gets a key, where the servers run, how disks are encrypted and how backups are kept.

## What is protected

| asset | where it is | who should reach it |
|---|---|---|
| rows of the tables | the database file (`--db`), its `-wal`, checkpoints in `--backup-dir` | clients with a key for those tables |
| queries and their values | the running and finished queries, the log store (`DB.logs`) | the server's admins |
| credentials | the token (`--token`), the cluster token, API keys (only their SHA-256 is kept, in `DB.keys.json`), connector secrets, cloud credentials | the people who run the server; each server of a cluster for the cluster token |
| the audit log | memory, `--audit-log FILE`, the log store's `audit` table | the server's admins |
| ChloeCtx's stores | a folder per project under `~/.local/share/chloectx/projects/` | the person whose sessions they hold |
| ChloeCtx's synced memory | a table per project on a ChloeDB server, `chloectx_mem_<project>` | the machines of that project, each with a key for that table alone |

## Who it is protected from

| actor | what they can try | what stops them |
|---|---|---|
| anyone on the network | reach the HTTP, gRPC or OpenSearch port | a server with no credential listens on loopback only and refuses to start on another address; with one, every route but the probes and the page's files needs it ([listeners](security.md#listeners)); TLS keeps tokens off the wire ([TLS](security.md#tls)) |
| a web page in the operator's browser | send requests to a server on `localhost` | host names checked against DNS rebinding, writes with a foreign `Origin` refused, no CORS, JSON routes refuse form bodies ([browsers](security.md#browsers)) |
| a client with a key | read or change more than its key allows | scopes checked per route and per statement kind; keys limited to tables checked on every table a statement names, refused on the routes and protocols that cannot check them ([API keys and scopes](security.md#api-keys-and-scopes)) |
| a tenant's key | read another tenant's tables | a key limited to its prefix; a filtered OpenSearch alias is not a boundary ([multi-tenant isolation](security.md#api-keys-and-scopes), [aliases](opensearch.md)) |
| an AI agent with a key | lose data or spend money | roles `propose` and `ops`; tier 3 actions need a person's approval through a change request, and an agent's own approval is refused ([agents](security.md#api-keys-and-scopes)) |
| a server of another cluster, or a client | ship writes to a replica, move slots, reconfigure a copy | the calls between servers take the cluster token and nothing else ([credentials](security.md#credentials)) |
| an app the server shows | call other apps as the server, or read beyond its tables | a token per app, a key limited to the app's own tables, credentials taken out of its environment ([apps](security.md#apps)) |
| a request the server makes for you | reach the internal network or cloud metadata | connectors and alert routes reach the public internet unless told otherwise, never link-local addresses, and follow no redirect to another origin ([requests the server makes](security.md#requests-the-server-makes)) |
| someone with a shell on the machine | read the files | left to you: `umask 077` and disk encryption ([data at rest](security.md#data-at-rest)) |

## ChloeCtx and its sync

ChloeCtx records your Claude Code sessions, so its stores can hold anything those sessions saw. Its threat model is a person's shell history plus a shared notebook.

- **On one machine**, the store is yours alone (mode 0700). The daemon's socket opens only to you. The inspector needs a random token and refuses requests from other pages. Keys, tokens and `NAME=value` secrets are redacted before anything is stored. A password written in prose is not caught, so keep secrets out of prompts ([ChloeCtx](security.md#chloectx)).
- **Between machines**, each project syncs into a table of its own, with one key per project and machine. That key reads and writes that project's table and nothing else, so a leaked laptop key exposes one project ([one key per project and machine](../chloectx/setup.md#keep-the-memory-on-a-chloedb-server)).
- **Another machine's words are held back.** Rules and decisions that another machine pushed stay out of your sessions until you accept that machine, per project. So a key that can write a project's table cannot plant a "user preference" in everyone's sessions. Facts, notes and the conversation are not held back ([what another machine sent waits for you](../chloectx/setup.md#keep-the-memory-on-a-chloedb-server)).
- **Removing a person** is revoking their machine's keys, then deleting what they pushed that the team does not keep. Every machine forgets those rows at its next pull ([for a team](../chloectx/setup.md#for-a-team)).
- **Recall treats stored text as data.** What a session read (a file, a web page, a tool's output) is recorded, and later sessions see it as recalled context, so an instruction hidden in a file can reach a later session as text. Review what the team's sessions read as you would review what they run.

## Left to you

- **Disk encryption.** ChloeDB does not encrypt its files: use the disk's or the volume's encryption.
- **The network.** Let only the cluster's servers, its clients and its monitoring reach its ports. There is no mutual TLS.
- **Rate limits per client.** Put a proxy in front if a client must not flood the server. A server does refuse queries past `--max-concurrent` and memory past `--query-memory-mb`.
- **Who holds a key.** Issue keys per client and per job (the presets help), give them expiry dates, and revoke them when a person or a service leaves.
- **Backups off the machine.** Checkpoints are consistent copies. Where they are kept, and who can read them, is yours ([Back up, restore and upgrade](../guide/backup.md)).

## How the controls map to SOC 2

The AICPA's Trust Services Criteria (2017, revised 2022) are what a SOC 2 report covers. This table says which of ChloeDB's controls support each criterion that usually applies to a database, and what your organization does on top. It is a starting point for your own mapping, not an attestation: ChloeDB has no SOC 2 report of its own, since it is software you run.

| criterion | ChloeDB's controls | yours |
|---|---|---|
| CC6.1 logical access | API keys with scopes and table grants, checked per route and per statement; Google sign-in with an allow list; keys stored as hashes | deciding who gets which key |
| CC6.2 granting access | keys made by an admin or within a key's own grants, with presets per job and expiry | approving each key |
| CC6.3 removing access | revocation at once, across a cluster within 5 seconds; rotation with a grace period | revoking when people leave |
| CC6.6 the boundary | loopback by default, refusal to start open on a reachable address, host and origin checks, outbound requests kept off internal addresses | the network around the servers |
| CC6.7 data in transit | TLS (rustls) for HTTP and gRPC | issuing the certificates |
| CC6.8 unauthorized software | release archives with build provenance and an SBOM signed through Sigstore, checksums, PyPI attestations ([checking what you download](../../SECURITY.md#checking-what-you-download)) | checking them before you install |
| CC7.1 detecting changes | the audit log of every change and every refused request, with who, what, and before and after | keeping it (`--audit-log FILE`) and reviewing it |
| CC7.2 monitoring | built-in alert rules, `/metrics` for Prometheus, rules and a dashboard to import ([monitoring kit](https://github.com/chloetech-ai/chloedb/tree/main/chloedb-server/deploy/monitoring)) | paging someone |
| CC7.4 responding to incidents | [runbooks](runbooks.md), linked from each alert | following them |
| CC8.1 change management | releases built only from commits that passed CI; one-way doors (the file format, the shard protocol, replication, authentication, installers) wait for a person's review; an agent's risky changes go through change requests | how you roll out new versions |
| A1.2 recovery | checkpoints while serving, `chloedb-server restore`, replication with automatic failover | how often, where they are kept, and rehearsing a restore |
| C1.1 confidentiality | keys limited to tables, the redaction of query values (`--redact-params`), files mode 0600 for secrets | disk encryption, `umask 077` |

## What ships in each package

| package | licence | third-party notices | SBOM |
|---|---|---|---|
| release archives (chloedb-server, chloectx, chloedb-crawl, chloedb-sheets, chloedb-netops) | Apache-2.0, `LICENSE` | `THIRD-PARTY-NOTICES.md` in the archive | `<program>-<platform>.cdx.json` beside it, attested |
| Python wheel (`chloedb`) | Apache-2.0, in the wheel's `.dist-info/licenses` | `chloedb/THIRD-PARTY-NOTICES.md` in the wheel | not yet |
| chloedb-server's wheels (`chloedb-server`) | the archive's | the archive's | the archive's |
| npm packages (`chloedb`, `@chloedb/<platform>`) | Apache-2.0, `LICENSE` | `THIRD-PARTY-NOTICES.md` in each package that holds the addon | not yet |
| container image (`ghcr.io/chloetech-ai/chloedb-server`) | Apache-2.0 (the image is built from this repository) | not in the image yet: use the archive's of the same version | not yet |

Each notices file lists every crate compiled into the binary with its version and licence, and the licence texts those crates ship. `cargo deny` checks every crate's licence against the allowed list in `deny.toml` every day, together with RustSec's advisories.
