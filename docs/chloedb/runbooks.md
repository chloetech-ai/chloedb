# Runbooks

What to do when an alert fires or the fleet view names a problem, written
for an operator and for an agent alike. Each runbook names the alerts
(`GET /v1/alerts`) and fleet problems (`GET /v1/health/fleet`, each
problem's `id`) it is for, then the steps: what to read first, what an
`ops` key may do alone (tier 1), and what needs a person's approval (tier
2 and 3, filed as a change request with `POST /v1/admin/changes`). The
tiers are explained in [chloedb-server](server.md#agents-and-administration).

The server compiles this page in: `GET /v1/catalog` lists the routes the
steps name, the MCP tool `runbook` finds a runbook by an alert's or a
problem's id (`runbook {"for": "copy-down"}`), and each fleet problem's
`next.runbook` names the one to read. The last section, on upgrading
servers outside Kubernetes, is for no alert.

## Queries are slow

For: `p99`, `p99-jump`, `p99_recovered`

1. Read where the time goes: `GET /v1/queries/slow` (tool `queries`,
   `which: slow`) and `GET /v1/queries/stats` group the slowest statements
   by their fingerprint; `GET /v1/load` shows each server's CPU and queue.
2. Read what changed around the jump: `GET /v1/events?from=<ms>` (tool
   `events`): a plan applied, a primary changed, a copy down, a rollout.
3. One statement or one key holding the cores (tier 1, an `ops` key with a
   reason): limit it with a contain rule, `POST /v1/admin/contain
   {"match": {"fingerprint": "..."}, "action": "limit", "concurrency": 2}`
   (tool `contain`); an agent's rule lasts an hour at most.
4. One query running for minutes (tier 1): cancel it, `DELETE
   /v1/queries/<id>` (tool `cancel_query`).
5. The cluster is short of CPU for its load (tier 2): plan more copies or
   shards, `POST /v1/admin/plan` (tool `plan_reshape`), and file the plan as
   a change request.

## Searches fail

For: `errors`, `availability`, `5xx`, `error_ratio`, `error_ratio_recovered`, `partial-answers`, `key-failing`

1. Read which statements fail and where: `GET /v1/queries/recent?outcome=error`
   and `GET /v1/logs?level=error` (tools `queries`, `logs`). Each error
   answer carries an `error_code` (`GET /v1/catalog` lists them): `query_invalid`
   and `table_not_found` are a client's, `unavailable` and `internal` the
   cluster's.
2. Errors of one client (`key-failing`): tell the key's owner; nothing to
   change on the server.
3. Shards that do not answer (`partial-answers`, `unavailable`): follow
   "A copy is down" below.
4. Errors after a change: read `GET /v1/events` and `GET /v1/admin/audit`
   for what changed, and file the change that undoes it (each dry run's
   `undo` says which) as a change request.

## Queries are refused

For: `rejected`, `query-memory`, `key-flood`, `contained`

1. Read what is refused and why: `GET /v1/queries` (running now, grouped
   by key and statement) and `GET /v1/admin/contain` (the contain rules in
   force; tool `contain_rules`). An answer refused by a contain rule has
   code `contained`.
2. One key crowds out the others (tier 1): limit the key, `POST
   /v1/admin/contain {"match": {"key": "..."}, "action": "limit",
   "concurrency": 4}`, and tell its owner.
3. A contain rule that refuses too much (tier 1): remove it, `DELETE
   /v1/admin/contain/<id>` (tool `remove_contain`).
4. The load is real and lasting (tier 2): raise `--max-concurrent` or
   `--query-memory-mb` in the spec (`POST /v1/admin/kube/plan`, then a
   change request), or add copies.

## A copy is down

For: `copy-down`, `copy-silent`, `under-replicated`, `shard-no-majority`, `copies`, `shards`, `coordinators_gone`

1. Read which copy, since when, and what else happened: `GET
   /v1/health/fleet` (tool `health`), then `GET /v1/events?kind=copy_down,primary_changed`
   and the copy's own lines, `GET /v1/logs?q=<copy name>`.
2. A copy that restarts by itself (a pod killed for memory, a rollout):
   wait for it, `GET /v1/events?after=<id>&wait_s=30` answers as soon as
   its next event comes. Silence the alert meanwhile (tier 1, two hours at
   most for an agent): `POST /v1/alerts/copy-down/silence {"minutes": 30}`
   (tool `silence_alert`).
3. A copy that does not come back (tier 2): replace it. `POST
   /v1/admin/plan` with the copy left out plans the repair (Cluster: Plan
   the repair, in the command center); file the plan as a change request.
4. A shard with no majority takes no writes, and its copies cannot elect
   a primary. Its searches go on while a replica that is up serves reads
   (the shard's state is `no_writes`); when the copy left is its primary,
   it serves no reads either (`no_reads`), since with failover a primary
   serves reads only while a majority of its copies answer. Bring a copy
   back, or remove the dead one (tier 2 for a replica, `DELETE
   /v1/admin/shards/<i>/replicas`; promoting by force is tier 3 and a
   person's alone).

## A replica is behind

For: `replica-behind`, `out-of-sync`, `replica-reads`, `lag`, `follow-behind`

1. Read how far behind and why: the primary's `GET /v1/health`
   (`replication.replicas`: each replica's `seq_no`, how many
   transactions it is `behind`, and whether it is `in_sync`; on
   `/metrics`, `chloedb_replica_behind`), and `GET /v1/load` (the
   primary's write rate, the replica's disk and CPU). `GET /v1/cluster`
   says each copy's role and whether it serves reads; its `txn` counts the
   transactions of each process since it started, so it does not compare
   two copies. `GET /v1/admin/follow` for a follower cluster.
2. A replica catching up after a restart: wait, with `GET
   /v1/events?after=<id>&wait_s=30`, and silence the alert for the while
   (tier 1).
3. A replica out of sync must not become the primary: do not promote it.
   Replace it (tier 2: a plan and a change request) if it does not catch
   up.

## The disk is filling

For: `disk-full`, `disk-critical`, `disk-filling`, `disk`

1. Read which tables grow: `GET /v1/tables` and `GET /v1/load` (each
   copy's disk).
2. Old rows a table keeps for a set time: a retention (`retain`) on the
   table drops them (tier 2, a change request).
3. More room (tier 2): add disk in the spec (`POST /v1/admin/kube/plan`),
   or add a shard and move slots to it (`POST /v1/admin/plan`), each filed
   as a change request.
4. Deleted rows still on disk: `POST /v1/admin/vacuum` frees them for good
   (tier 3: it loses old versions, and is a person's alone).

## Writes fail or queue

For: `failing`, `flush-failing`, `merge-failing`, `write-queue`, `write-p99`

1. Read the server's lines: `GET /v1/logs?level=error` (a full disk, an
   I/O error).
2. A database that refuses writes (`failing`) on a primary: with
   `--failover-after-ms` it hands its shard over to a replica in sync by
   itself; without it, hand the primary over first (tier 3, `POST
   /v1/admin/shards/<i>/promote`, a person's). Then fix the cause and
   restart the server.
3. Slow writes with `--write-acks all`: a replica or its link is slow;
   follow "A replica is behind".

## Data damaged on disk

For: `damaged`

1. Read which copy and which segments: `GET /v1/health/fleet` and `GET
   /v1/events?kind=damaged`.
2. Verify again (tier 1): `POST /v1/admin/verify`.
3. Replace the copy (tier 2): plan the repair and file it as a change
   request; never make the damaged copy the primary.

## A shard's primary changed

For: `primary-changed`

1. Read why: `GET /v1/events?kind=primary_changed,copy_down` and the old
   primary's lines (`GET /v1/logs?q=<copy name>`).
2. Nothing to do when the old primary is back as a replica in sync; it
   resolves ten minutes after the last change. A handover asked by hand
   fires it too. Otherwise follow "A copy is down".

## A rollout is held

For: `rollout`, `held`

1. Read what the gate saw: `GET /v1/kube` (the operator's status and
   the gate's reason) and `GET /v1/events`.
2. When the reason is understood and the change is wanted (tier 2): `POST
   /v1/admin/kube/resume`, filed as a change request; otherwise roll back
   (`POST /v1/admin/kube {"rollback": <the version before>}`), also a
   change request.

## Upgrades outside Kubernetes

On Kubernetes the operator rolls a new version through the cluster
([upgrades](server.md#upgrades)). On machines of your own, upgrade one
server at a time, waiting after each until the cluster is well again:

1. In each shard, its replicas, one at a time: stop it with SIGTERM,
   start it on the new binary without `--replicas` or `--replica` (it
   takes up the part its `<db>.replication` keeps), and wait until its
   own `GET /v1/health` says `replication.serves_reads` and `GET
   /v1/cluster` says every shard `ok`. A replica must not be older than
   its primary.
2. Then hand the shard over to an upgraded replica (tier 3, a person's:
   `POST /v1/admin/shards/<i>/promote {"url": "<its gRPC URL>"}`), and
   upgrade the old primary, now a replica, as the others.
3. The coordinators last, one at a time, each with `--drain-ms` behind
   a load balancer.

[Operate a cluster](../guide/operate-cluster.md#upgrade-one-server-at-a-time)
runs this on one machine, and [versions and
compatibility](../compatibility.md#a-cluster-of-mixed-versions) says what a
cluster of mixed versions serves.
