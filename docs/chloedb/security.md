# Security model

This page says who can reach a ChloeDB server, what each credential lets its holder do, what is open without one, and what is left to you: the network around the servers, the disks, and the files the servers write. The flags and routes it names are described in full in [the server's security section](server.md#security). To report a vulnerability, see [SECURITY.md](../../SECURITY.md).

## Before a server leaves your machine

A server on one machine for one person needs nothing: it listens on loopback only. Before it listens anywhere else:

1. Give it `--token` (or `$CHLOEDB_TOKEN`) and keep that token for the people who run it. Give clients [API keys](#api-keys-and-scopes) of the scope they need, limited to their tables where they are an app's.
2. In a cluster, give every server, coordinators included, the same `--cluster-token` (`$CHLOEDB_CLUSTER_TOKEN`), and no client.
3. Serve TLS (`--tls-cert`, `--tls-key`), or keep the servers on a private network: without TLS a token or a key crosses the network as plain text.
4. Let only the cluster's own servers, its clients and its monitoring reach the HTTP and gRPC ports.
5. Start the servers with `umask 077`, or keep their folders readable by their user alone ([data at rest](#data-at-rest)), and keep the files that hold the tokens (a systemd unit's `EnvironmentFile`) mode 0600.
6. Pass `--start-apps off` on a server that should run no app ([apps](#apps)).

[Operate a cluster](../guide/operate-cluster.md) does all of this for a cluster on one machine.

## Listeners

| listener | default | flag |
|---|---|---|
| HTTP (the API, the command center, `/metrics`) | `127.0.0.1:7700` | `--listen` |
| gRPC (clients, and the calls between a cluster's servers) | `127.0.0.1:7701` | `--grpc`, `off` for none |
| OpenSearch's REST API | off | `--opensearch-listen` |

A server with no token, no cluster token and no API key is **open**: every request is its admin's. An open server refuses to start on an address reachable from elsewhere (`0.0.0.0`, a LAN address): "0.0.0.0:7700 is reachable beyond this machine: set --token (or $CHLOEDB_TOKEN), make an API key, or pass --no-token to serve it open". `--no-token` serves it open anyway, for a network you trust as a whole. A token, a cluster token or an API key each close the server. The key a server makes for each [app it starts](#apps) does not.

Once the server is closed, every route needs `Authorization: Bearer <token or key>`, except these, which answer anyone:

- the command center's files (`/`, `/classic`, `/ui/*`), which hold no data: the page asks for a key before it reads anything;
- the probes `/livez`, `/readyz` and `/v1/health`, for load balancers and orchestrators. A coordinator answers `/v1/health` without a credential with `{status}` alone.

Every gRPC call needs a credential, and the OpenSearch listener takes the same token and keys, as a Bearer token or as the password of Basic authentication.

## Credentials

| credential | who holds it | what it may do |
|---|---|---|
| `--token` | the people who run the server | everything over HTTP and gRPC, with the admin scope. On a server without a cluster token it is also the cluster's token |
| `--cluster-token` | every server of a cluster, never a client | the calls between servers (`Replicate`, `Restore`, `Verify`, `Slots`, `Configure`, the slot moves, `Keys`, `Feed`, `FeedKeys`), which no API key may make, and everything the token may |
| an API key (`sk_<id>_<secret>`) | a client, an agent, an app | what its scope or role allows, on every table or on the tables it is limited to |
| a Google sign-in session | a person in the command center | the scope the allow list gives their address, as long as it gives it |
| an app's token | chloedb-server, when it sends a request on to an app | the app's own API |

Tokens are compared in constant time. A server keeps no key's secret: the keys file (`--keys`, by default `DB.keys.json` beside the database, mode 0600) holds each key's name, scope, grants and the SHA-256 of its secret, and the ids of the keys revoked. The servers of a cluster share their keys every 5 seconds over `Keys`, with the cluster token, so a key made on any coordinator works on every shard, and a key revoked anywhere is revoked everywhere within a round ([keys across a cluster](server.md#security)).

The calls between servers take the cluster token and nothing else. A client holding the `--token` or an admin key cannot ship writes to a replica, move slots or reconfigure a copy, nor make a server send the cluster token to an address of its choosing: adding a shard or a copy, and a promotion that names copies outside its shard, need the cluster token.

## API keys and scopes

Make keys in the command center (**Access and audit**) or with `POST /v1/admin/keys`, which answers the secret once. A key may expire (`expires_days`, `expires_minutes`), is rotated with a grace period during which both secrets work, and is revoked at once. [The server's security section](server.md#security) lists every route each scope reaches.

| scope | may |
|---|---|
| `feed` | read the [feed](server.md#the-feed-changes-for-another-cluster-to-follow) of every table, with every row as it is written, and `POST /v1/feed/keys`, the keys and digests of any table's rows; `/v1/health` and `/v1/whoami`. It is for a cluster that follows this one: it reaches few routes, but what it reads is all of the data |
| `read` | queries, explain, embed, schemas, health, metrics and the series, and the feed |
| `write` | that, and rows: `/v1/write`, `/v1/import`, `/v1/stream`, `INSERT`, `UPDATE` and `DELETE`, OpenTelemetry logs |
| `admin` | that, and changing the server: tables, rank profiles, keys, checkpoints, promotion, alert rules; the running queries, the logs and the audit log, which show other clients' SQL; `/v1/export` |

What a request does is checked as well as its route: a SQL statement by its kind (a `CREATE TABLE` or a `DROP` needs `admin`, whichever route sends it), a write plan needs `write`.

**Keys limited to tables.** `POST /v1/admin/keys {"name": "crawler", "tables": {"crawl_*": "admin", "docs": "read"}}` makes a key whose grants each name a table, a prefix and `*`, or `*`, with a scope. Such a key never has the server's own admin (keys, shards, functions, settings, the logs and the audit log), it may call only the routes that check the tables they touch, and gRPC and MCP refuse it. A statement is read with ChloeDB's own parser and checked against every table it names; what its names cannot show (`llm()`, a table function that is not a stored one, a name in parts) is refused. Presets (`analyst`, `oncall`, `ml-app`, `app-owner`) make the keys of common jobs.

**Keys a key makes.** A key with the admin scope on some tables may make keys within its own grants (`POST /v1/keys/own`), which expire when it does and stop working when it is revoked. An app engine gives each of its apps a key this way.

**Agents.** Give an agent a key of role `propose` (it reads the server's views and files change requests) or `ops` (that, and the bounded actions: a contain rule, a query cancelled, an alert silenced, each with a reason), rather than an admin key. Whatever its key, an agent's request that loses data or costs money (tier 3: a `DROP TABLE`, a `DELETE` of every row, a promotion, a key revoked) is refused with `change_request_required`: it files a change request, which a person approves with their own key, and an agent's approval is refused. The audit log names an agent's requests `actor: "agent"` ([agents and administration](server.md#agents-and-administration)).

**Model calls.** Each key may make `--model-calls-per-key-day` calls (10,000) a day to a hosted model with the server's own credentials; the token, the cluster token and an open server are not counted.

## Browsers

A server on your laptop is reachable by every page your browser opens. Three rules keep those pages out:

- **Host names.** On loopback the server answers only to `localhost`, `127.0.0.1` and `::1`, and the names `--allow-host` adds, so a page that points its own name at 127.0.0.1 (DNS rebinding) gets 403. On another address it answers to every name unless `--allow-host` lists them, and its credential guards it.
- **Origins.** Any request but `GET` and `HEAD` (reads by `POST /v1/query` and `/v1/sql` included) that carries an `Origin` other than the server's own gets 403: "a page from http://evil.example cannot change this server". The server sends no CORS headers, so a browser's preflight (`OPTIONS`) never gets a permission: it is answered 405, or 401 on a server with a token. Clients outside a browser send no `Origin`, and pass. A web app's page therefore calls a route of the app's own, which queries ChloeDB ([why a page cannot call chloedb-server](javascript.md#why-a-page-cannot-call-chloedb-server)).
- **Content types.** A route that takes JSON refuses a body without `application/json` (415), and `/v1/import` refuses the types a page may send to another site without asking first (`text/plain`, forms, none).

Every answer carries `X-Content-Type-Options: nosniff`, `X-Frame-Options: SAMEORIGIN`, `Referrer-Policy: same-origin` and `Content-Security-Policy: frame-ancestors 'self'; base-uri 'self'; object-src 'none'`, unless its route sets its own. A stored file (`GET /v1/media/<sha256>`) is served as an image, a sound, a video or a PDF, or else as a download, with a sandboxing policy, so a writer's HTML or SVG never runs on the server's origin.

The command center keeps a key you paste in the tab's session storage, which ends with the tab. Paste it in the sign-in dialog rather than opening `/?token=...`: the page drops the token from its address at once, but the first address is in the browser's history and in the log of any proxy between you and the server. A person signed in with Google holds no key at all: their session is a cookie the page's scripts cannot read (`__Host-chloedb-session`, `Secure`, `HttpOnly`, `SameSite=Strict`), lasting 12 hours unused and 7 days at most, and the allow list is checked again on every request ([sign in with Google](server.md#security)).

## TLS

`--tls-cert FILE --tls-key FILE` (PEM) serve HTTP and gRPC over TLS, with rustls. A coordinator reaches `https://` shards over TLS, trusting the public roots and `--shard-ca FILE`. There is no mutual TLS: the servers of a cluster know each other by the cluster token. Behind a proxy that ends TLS, the hop from the proxy to the server is plain HTTP unless the server serves TLS too.

## Apps

An app is a program of its own (the crawler, Sheets, ChloeCtx, network operations, the app engine) that the command center shows as a view ([apps](server.md#apps)).

- **Calls to an app** go through the server: `/v1/apps/<name>/...` checks the caller's key first (read scope for `GET` and `HEAD`, admin for the rest), then sends the request on with the app's own token in place of the key, and tells the app who called (`x-chloedb-principal`, `x-chloedb-scope`, and `x-chloedb-tables` for a key limited to tables). Give each app a token of its own (`--app-token-dir`, `CHLOEDB_APP_TOKEN_<NAME>`), so that one app cannot call another as the server.
- **An app's view runs in the command center's page**, with the signed-in person's key, as the server's own pages do: name only apps you trust on a server's command line.
- **An app that checks its callers** runs a caller's statements as the caller: it sends its own token with `x-chloedb-as-tables` set to the caller's grants, and the server narrows the token to them, never wider than the token itself.
- **Apps the server starts.** With `--start-apps auto`, the default, a server of its own or a coordinator starts the crawler, Sheets and ChloeCtx when their programs are installed beside its own, as an install puts them. Each listens on a loopback port the server holds, takes requests only with a token the server makes at each start, and calls the server back with a key limited to its own tables (named `app:<name>`), kept in memory and never in the keys file. Its environment is the server's with every credential taken out. A replica, a shard, a pod the Kubernetes operator runs and a server over TLS start none ([apps the server starts](server.md#apps-the-server-starts)). An app started this way makes its tables in the database (ChloeCtx's `ctx_*`), and ChloeCtx downloads its embedding model; `--start-apps off` starts none.
- **ChloeCtx as an app, by hand.** `chloectx app` refuses an address beyond loopback without `--token` unless `--insecure` says otherwise. Give it a token wherever you run it ([ChloeCtx as an app](../chloectx/app.md)).

## Requests the server makes

- **Connectors** reach the public internet only, unless a connector's config says `"allow_internal": true`; cloud metadata addresses are refused even then. Their credentials go to the origin of their `url` alone, and a redirect to another origin fails the run ([what a connector reaches](connectors.md#what-a-connector-reaches)). Their secrets are kept in `DB.connectors.json`, mode 0600, and a push connector's key writes its table and reads nothing ([keys](connectors.md#keys)).
- **Media fields** read a URL or a file only when `--media-fetch` lists it; nothing is fetched by default ([media fields](server.md#media-fields)).
- **Models** are names: a query names a model in `--models` or a provider's, never a path, and an `endpoint.json` names only a provider's key variable or a `CHLOEDB_MODEL_KEY_*` one.
- **Cloud credentials and the hub.** The command center's cloud view keeps the AWS and DigitalOcean credentials it saves in `DB.aws.json` and `DB.digitalocean.json`, mode 0600; the hub keeps other clusters' keys in the macOS Keychain or the Secret Service where there is one, else in `DB.hub-keys.json`, mode 0600.

## Data at rest

ChloeDB does not encrypt its files: use the encryption of the disk or the volume (LUKS, FileVault, an encrypted cloud volume) where the data needs it.

The database, its `-wal`, the log store (`DB.logs`), the audit log file (`--audit-log`) and checkpoints are made with the server's umask, so under the common 022 anyone on the machine may read them. The keys file, connector secrets, the cloud view's credentials and the hub's keys are mode 0600 whatever the umask. Start the server with `umask 077` (systemd: `UMask=0077`) and every file it makes is its user's alone, or keep its folder mode 0700.

A checkpoint (`--backup-dir`) holds every row of the database: keep it as you keep the database. Beside it are the keys file as it was (`NAME.keys.json`, mode 0600: the keys' names, scopes and the SHA-256 of their secrets, never a secret) and the shard's cluster map (`NAME.slots`), which [`chloedb-server restore`](server.md#restoring-a-checkpoint) puts back.

## Logs

- **The log store** (`--log-store`, by default `DB.logs` beside the database; `off` keeps none; a coordinator keeps none unless given one) keeps the server's log lines and the queries it answered for 7 days (`--log-retain`), up to 1 GiB (`--log-store-mb`). It keeps every slow and failed query, and up to 200 others a second (`--log-queries`).
- **Queries' values.** The running and finished queries, and the log store, keep each query's parameters with their values. `--redact-params` (`CHLOEDB_REDACT_PARAMS`) keeps only their types. A value written into the SQL text is part of the statement and kept with it: pass anything secret as a parameter.
- **Credentials.** A token or a key is not written to the log, the log store or the audit log, a wrong one included; the log and the audit log name a key by its name and id, and the token as `root token` (the log) or `token` (the audit log).
- **The audit log** keeps every change (a request that needs the admin scope and changes something), every note and every refused request, the latest 2,000 in memory and every one in `--audit-log FILE` and the log store's `audit` table: who (the key's name and id), the route, the status, the client's address, the reason given, and what changed, before and after. Reads are not audited, nor writes of rows, by `/v1/write` or by SQL, except those the command center sends ([audit log](server.md#security)). Behind a proxy, `--trust-proxy` names the proxies whose `X-Forwarded-For` the audit log believes.

## ChloeCtx

ChloeCtx's stores hold transcripts of your Claude Code sessions, which may contain anything the sessions saw.

- **The stores.** Each project's store is a folder under `~/.local/share/chloectx/projects/` (or `~/.local/share/engram/projects/` on a machine from before the rename), made mode 0700.
- **The daemon's socket.** A project's daemon listens on a Unix socket in its store's folder (`engram.sock`), which only you can open. When that path is too long for a socket, it uses a folder of yours alone: `$XDG_RUNTIME_DIR`, then `/tmp/engram-<uid>`, then `~/.cache/chloectx`, each only if it is a real folder owned by you that no one else may write to; a folder someone else made is refused.
- **The inspector** (`chloectx ui`) listens on 127.0.0.1 by default, and every request needs the random 128-bit token in the address it prints, which the page moves into a cookie (`HttpOnly`, `SameSite=Strict`) on its first load. It offers only the daemon's read operations; its one change, starting an upgrade, needs the page's own header and origin. Its page runs only its own script (a `Content-Security-Policy` by hash, `frame-ancestors 'none'`), so text from a store cannot run as a script. Started on another address it warns that anyone who can reach it and has the token can read every store, and it serves plain HTTP: forward the port over SSH instead ([the live inspector](../chloectx/inspector.md)).
- **Redaction.** ChloeCtx replaces keys, tokens, private keys, passwords in URLs and headers, and values assigned to names such as `DB_PASSWORD` with markers before it stores or syncs anything; a password written in prose is not caught. Treat a store like your shell history ([where it keeps what it stores](../chloectx/index.md#where-it-keeps-what-it-stores)).

## What it does not do

- Encrypt data at rest.
- Mutual TLS between the servers or with clients.
- Limit the rate of requests per key or per client address.
- Keep a separate log of reads and of clients' writes of rows.
- Sign release archives: each comes with a `.sha256`, PyPI's packages carry attestations ([SECURITY.md](../../SECURITY.md#checking-what-you-download)).
