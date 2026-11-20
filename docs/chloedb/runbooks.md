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
   and the copy's own lines from the log stores, `POST /v1/logs/query
   {"sql": "SELECT ts, level, message FROM logs WHERE node = '<copy name>' ORDER BY ts DESC LIMIT 200"}`
   (on a coordinator it searches every copy's store; `GET /v1/logs` has
   the coordinator's own lines only).
2. A copy that restarts by itself (a pod killed for memory, a rollout):
   wait for it, `GET /v1/events?after=<id>&wait_s=30` answers as soon as
   its next event comes. Silence the alert meanwhile (tier 1, two hours at
   most for an agent): `POST /v1/alerts/copy-down/silence {"minutes": 30, "reason": "s1-0 restarting"}`
   (tool `silence_alert`; the server refuses a silence without a `reason`).
3. A copy that does not come back (tier 2): replace it. `POST
   /v1/admin/plan {"shards": [{"shard": <i>, "copies": [...]}]}`, the
   gRPC URLs the shard keeps with the dead copy left out and its
   replacement in, plans the repair (Cluster: Plan the repair, in the
   command center); file the plan as a change request. A plan that adds a
   server is applied with the cluster's token, which the coordinator
   sends to it (403 `cluster_token_required` with the admin token or a
   key): on machines of your own, a person runs `chloedb-server admin
   --key "$CHLOEDB_CLUSTER_TOKEN" call POST /v1/admin/plan/apply @plan.json`
   with the same body; on Kubernetes the operator adds copies itself.
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
   two copies. `GET /v1/admin/follow` for a follower cluster, Data > Connectors
   (`followers` in `GET /v1/admin/connectors`) for a Postgres follower.
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
2. Old rows a table keeps for a set time: a table made with a retention
   (`retain`) drops them on its own. A table's retention is set when it is
   made and cannot be changed (`ALTER TABLE ... SET (retain = ...)` is
   refused): for a table made without one, or with too long a one, make a
   new table with the retention, move the writers to it, and drop the old
   one once its rows are no longer needed (tier 2 and 3, change requests).
3. More room (tier 2): add disk in the spec (`POST /v1/admin/kube/plan`),
   or add a shard and move slots to it (`POST /v1/admin/plan`), each filed
   as a change request.
4. Deleted or expired rows still on disk: a server that takes writes gives the
   free space of its file back by itself, every 5 minutes, once 64 MB and a
   sixteenth of the file are free (it moves segments from the end of the file
   into free space below them and truncates the file; it needs no room and
   loses nothing; not on a replica). **Tables and storage** shows the file
   (`totals.file_bytes` of `GET /v1/index/tables`) beside the tables' live
   data. When the free space is in pieces too small for the segments above
   them, `POST /v1/admin/vacuum` frees them for good
   (tier 3: it loses old versions, and is a person's alone). It needs room
   first: the file grows while it runs, by what lies above its first free
   gap, which is written twice (in a persona study, a 1.4 GB file reached
   2.5 GB before it shrank to 1.1 GB, and the process's memory 3 GB), so on
   a disk that is nearly full, free room another way first. Pages a
   statement freed become reusable two commits later: a vacuum straight
   after a `DROP TABLE` can free nothing, and the next one frees them.

## Writes fail or queue

For: `failing`, `flush-failing`, `merge-failing`, `write-queue`, `write-p99`, `write_error_ratio`

1. Read the server's lines: `GET /v1/logs?level=error` on the server
   itself (a full disk, an I/O error), or through a coordinator, every
   copy's: `POST /v1/logs/query {"sql": "SELECT ts, node, message FROM logs WHERE level = 'error' ORDER BY ts DESC LIMIT 100"}`.
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
2. Verify again (tier 1): `POST /v1/admin/verify`, which on a coordinator
   has every shard's primary compare its replicas (one shard: `POST
   /v1/admin/copies/verify {"url": "<copy>"}`).
3. Replace the copy (tier 2): plan the repair and file it as a change
   request; never make the damaged copy the primary.

## A shard's primary changed

For: `primary-changed`

1. Read why: `GET /v1/events?kind=primary_changed,copy_down` and the old
   primary's lines (`POST /v1/logs/query {"sql": "SELECT ts, level, message FROM logs WHERE node = '<copy name>' ORDER BY ts DESC LIMIT 200"}`).
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

## Restore from checkpoints

For: `restore`, `lost-data`, `bad-write`

Rows were deleted or overwritten by mistake, or a shard's copies all lost
their disks, and the cluster has to go back to its checkpoints. Restoring
is a person's alone (tier 3): it stops the cluster and takes back every
write since the checkpoint. These steps are for servers you start
yourself (by hand, from systemd, in a container); on Kubernetes the
operator keeps its pods running and has no restore step yet.

1. Have checkpoints of every shard from one cluster map: `POST
   /v1/admin/checkpoint {"name": "nightly"}` on the coordinator writes
   one on each shard's primary, into its `--backup-dir`, with the shard's
   cluster map (`nightly.slots`) and the cluster's API keys
   (`nightly.keys.json`) beside it, and answers the `slot_epoch` they are
   of. Copy the three files of each shard off the machine. A checkpoint
   of one copy is `POST /v1/admin/copies/checkpoint {"url": "...",
   "name": "..."}`. `GET /v1/admin/checkpoints` lists every copy's, each
   shard's newest, and under `complete` the names on every shard under
   one cluster map: the sets you can go back to.
2. Read what you are going back to: each shard's checkpoint names its
   shard and epoch (`nightly.slots`), and `GET /v1/cluster` gives the
   epoch now (`slot_epoch`). Slots moved since the checkpoint when the
   two differ: then restore every shard, never one alone, or rows would
   be in two shards or in none.
3. Stop the coordinator and every copy of the shards you restore.
4. On each copy, put its shard's checkpoint in place of its database:
   `chloedb-server restore --from /backups/nightly --db /data/shop.chloedb`
   (with `--keys FILE` when the server has `--keys FILE`). It checks the
   checkpoint before it changes anything, moves the database's own files
   aside into `/data/shop.chloedb.before-restore-<time>/`, puts the
   shard's map beside it and merges the keys in (keys revoked since stay
   revoked). It refuses while a server has the database open and when
   the checkpoint is of another shard than the copy's. A copy on an empty
   disk is restored the same way, or started empty: its primary sends it
   every row, and the coordinator gives it the cluster map.
5. Start each copy with the flags it had (`--replicas` on the shard's
   primary, `--replica` on the others, the same `--cluster-token` and
   `--seq-no`), then the coordinator with its `--shards`.
6. Check before you let writes in: `GET /v1/health` on the coordinator,
   and `GET /v1/cluster`, where each copy of a shard says the same
   `slot_epoch` and the same `seq_no`, its replicas in sync, and the
   shards' rows add up to the checkpoint's (`SELECT count(*)` through the
   coordinator). The files moved aside can go once it does.

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
