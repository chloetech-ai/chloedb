# Operate a cluster

This page runs ChloeDB as a production-shaped cluster on machines of your own, without Kubernetes: two shards, each held by three copies that elect their primary among themselves, a coordinator in front of them, a token that the servers send each other and API keys for clients. It then checks the cluster's health, loads and searches rows, kills a primary and watches the failover, writes checkpoints, and upgrades the cluster one server at a time while it serves.

Everything starts on one machine, on loopback, so that each command runs as written. On virtual machines each copy is a machine of its own (or a systemd unit), and the addresses are theirs. On Kubernetes the operator does all of this for you ([on Kubernetes](../chloedb/server.md#on-kubernetes)).

<!-- run: needs chloedb-server -->

You need `chloedb-server` on your `PATH` ([install](../quickstart.md)), and `curl` and `python3`.

## The layout

| server | what it holds | HTTP | gRPC |
|---|---|---|---|
| `s0a` | shard 0: its first primary | 7711 | 7721 |
| `s0b`, `s0c` | shard 0: replicas | 7712, 7713 | 7722, 7723 |
| `s1a` | shard 1: its first primary | 7714 | 7724 |
| `s1b`, `s1c` | shard 1: replicas | 7715, 7716 | 7725, 7726 |
| `coord` | no data: it routes each write to the shard that holds its row, and asks every shard for a search | 7700 | off |

The copies of a shard talk to each other, and to the coordinator, over gRPC. Clients talk to the coordinator over HTTP. A shard elects a primary only while a majority of its copies answer, so three copies keep a shard working through the loss of any one of them. With two and failover on, losing either copy stops the shard's reads and writes for as long as that copy is down, not for a few seconds: its primary serves a read only while a majority of its copies answer it, and a write waits for a majority. Give a shard that must stay up a third copy ([failover](../chloedb/server.md#failover)).

## Tokens

```sh
export CHLOEDB_CLUSTER_TOKEN=$(python3 -c 'import secrets; print(secrets.token_hex(32))')
export ADMIN_TOKEN=$(python3 -c 'import secrets; print(secrets.token_hex(32))')
```

The cluster token is what the servers send each other: a primary to its replicas, the coordinator to the copies. Every server of the cluster gets the same one, and only it may make the calls between servers (shipping writes, changing the cluster map, sharing keys); clients never hold it. You use it yourself for one thing: adding a server to the cluster (a copy that replaces a dead one, a new shard), since the coordinator sends the server it adds this token, and so takes it, not the admin token, for that call ([a copy that does not come back](#kill-a-primary)). Every server reads it from `CHLOEDB_CLUSTER_TOKEN`. The admin token is the coordinator's `--token`, for you, the operator. Clients get API keys, made below. On virtual machines, keep both in each unit's environment file (`EnvironmentFile=`, mode 0600) rather than on the command line, where `ps` shows them.

## Start the copies

Each copy is the same command with its own name, ports and, on its first start only, its part in replication:

```sh
copy() {  # copy NAME HTTP_PORT GRPC_PORT [ROLE FLAGS...]
  name=$1 http=$2 grpc=$3; shift 3
  mkdir -p "$name/backups" && echo "$http $grpc" > "$name/ports"
  chloedb-server --db "$name/db.chloedb" --seq-no --failover-after-ms 2000 \
    --listen 127.0.0.1:$http --grpc 127.0.0.1:$grpc --node-name "$name" \
    --backup-dir "$name/backups" --start-apps off "$@" >> "$name/server.log" 2>&1 &
  echo $! > "$name/pid"; disown   # the shell then says nothing when the copy is stopped
}
G=http://127.0.0.1
copy s0a 7711 7721 --replicas $G:7722,$G:7723
copy s0b 7712 7722 --replica --replicas $G:7721,$G:7723
copy s0c 7713 7723 --replica --replicas $G:7721,$G:7722
copy s1a 7714 7724 --replicas $G:7725,$G:7726
copy s1b 7715 7725 --replica --replicas $G:7724,$G:7726
copy s1c 7716 7726 --replica --replicas $G:7724,$G:7725
```

- `--seq-no` numbers the database's writes, which is how a primary ships them to its replicas. It cannot be undone.
- `--failover-after-ms 2000`: a replica that hears nothing from its primary for two seconds asks its shard's copies to elect it, and a primary that cannot reach a majority of its copies stops serving reads. Without it (the default, 0) a shard whose primary dies takes no writes until you promote a replica by hand.
- `--replicas` makes a copy a primary that ships to the copies it names. `--replica --replicas` makes a replica, which takes only what its primary ships and refuses clients' writes, and names its shard's other copies, which it ships to once it is elected.
- The role flags are for a copy's first start. A copy keeps its part (primary or replica, its term, the primary it follows) beside its database, in `db.chloedb.replication`, and a copy started again without `--replicas` or `--replica` takes it up again: that is how every restart on this page starts one, and a systemd unit's command leaves them out once the copy has started once. A replica started again with `--replicas` refuses to start, and says to use `--replica`.
- `--node-name` names the copy in the coordinator's views and the log stores' rows; `--backup-dir` is where its checkpoints go.
- `--start-apps off` keeps each server from starting the apps installed beside `chloedb-server` (the crawler, Sheets, ChloeCtx), which a cluster runs beside its coordinator, if at all ([apps on a cluster](apps.md)).

On machines of their own, each copy listens on its machine's address (`--listen 10.0.0.11:7700 --grpc 10.0.0.11:7701`) and names the others by theirs. A server with a cluster token, a `--token` or an API key may listen beyond loopback; one with none of them refuses to. Between machines, serve every server over TLS (`--tls-cert FILE --tls-key FILE`), list the copies as `https://`, and give the servers that call them `--shard-ca FILE` when your certificates are not signed by a public authority ([security](../chloedb/server.md#security)).

## Start the coordinator

```sh
CHLOEDB_TOKEN=$ADMIN_TOKEN chloedb-server --listen 127.0.0.1:7700 --grpc off --node-name coord --start-apps off \
  --shards "$G:7721|$G:7722|$G:7723,$G:7724|$G:7725|$G:7726" >> coord.log 2>&1 &
echo $! > coord.pid
```

`--shards` lists the copies' gRPC addresses: a comma between shards, and `|` between the copies of one shard. Which copy is the primary is not in the list: the coordinator asks each copy its part, and sends a shard's writes to the copy that answers as its primary. The first time, the coordinator gives every server the cluster map (each shard's copies, and its slots: 2,048 of the 4,096 each here), and every server keeps it beside its database. From then on the coordinator routes by the newest map its servers hold, so a coordinator started again on this same list, or on one copy of each shard, serves the cluster as it is now, after any changes ([changing the cluster while it serves](../chloedb/server.md#changing-the-cluster-while-it-serves)): a copy on the list that was replaced since, and does not answer, is left out, and its log says so. The coordinator holds no data and needs no `--db`: run two or more of them behind a load balancer, each with the same flags and `--peer-url` naming the address the others reach it at (`http://10.0.0.21:7700`), so that they find each other through the shards ([several coordinators](../chloedb/server.md#several-coordinators)).

What you change on the coordinator lives on the shards too. Its alert rules and routes, the alerts' acknowledgements, silences and notes, the latest 2,000 entries of its audit log and its latest 5,000 events (the Timeline: copies down and back, new primaries, changes, notes) are kept on every copy (in `db.chloedb.coordinator.json`, beside the keys), and a coordinator started again takes them back before it serves, as it takes the keys: a restart, or a new coordinator on the same `--shards`, keeps them without a file of its own, and the Timeline after a rolling upgrade still shows the upgrade. `--alerts FILE` keeps the rules and routes in a file as well, and `--audit-log FILE` keeps every entry of the audit log rather than the latest 2,000; **Alerts** says where its rules are kept. Name the cluster for the people who change it: `--cluster-name` and `--environment production` (or `staging`, `dev`) on each coordinator, which the command center shows in its header; on production it asks for a reason before each change, which the audit log keeps. A cluster with no environment is shown as a sandbox when it is reached on this machine, as this one is, and changes ask for no reason.

## Check its health

`GET /v1/cluster` is the coordinator's view of every shard and copy. This script prints the part an operator reads first, and is used for the rest of the page:

```python file=cluster.py
"""The cluster as its coordinator sees it: a line a shard, a line a copy.

python3 cluster.py               print it
python3 cluster.py --ok          exit 1 unless every shard and copy is ok and every copy serves reads
python3 cluster.py --primary N   the name of shard N's primary
python3 cluster.py --replicas N  the names of shard N's replicas
python3 cluster.py --versions    which servers run which version
"""
import json, os, sys, urllib.request

req = urllib.request.Request("http://127.0.0.1:7700/v1/cluster",
                             headers={"authorization": "Bearer " + os.environ["ADMIN_TOKEN"]})
cluster = json.load(urllib.request.urlopen(req, timeout=10))
shards = cluster["shards"]
name = lambda c: c.get("name") or c["short"]
arg = sys.argv[1:]

if arg[:1] == ["--ok"]:
    good = all(s["state"] == "ok" for s in shards) and all(
        c["status"] == "ok" and c["serves_reads"] for s in shards for c in s["replicas"])
    sys.exit(0 if good else 1)
if arg[:1] == ["--primary"]:
    print(*[name(c) for c in shards[int(arg[1])]["replicas"] if c["role"] == "primary" and c["status"] == "ok"])
elif arg[:1] == ["--replicas"]:
    print(*[name(c) for c in shards[int(arg[1])]["replicas"] if c["role"] == "replica"])
elif arg[:1] == ["--versions"]:
    by = {}
    by.setdefault(cluster["coordinator"]["version"]["version"], []).append(cluster["coordinator"]["name"] or "coordinator")
    for s in shards:
        for c in s["replicas"]:
            by.setdefault((c.get("version") or {}).get("version", "?"), []).append(name(c))
    for version, names in sorted(by.items()):
        print(" ".join(names) + ":", version)
else:
    for s in shards:
        primary = [name(c) for c in s["replicas"] if c["url"] == s["primary"]]
        print(f"shard {s['shard']}: {s['state']}, {s['why']}; primary {primary[0] if primary else 'none'}")
        for c in s["replicas"]:
            print(f"  {name(c)}  {c['role'] if c['status'] != 'down' else ''}  {c['status']}"
                  + ("  serves reads" if c["serves_reads"] and c["status"] != "down" else ""))
```

The cluster is ready once every copy answers and every replica has caught up with its primary:

```sh
until python3 cluster.py --ok 2> /dev/null; do sleep 0.5; done
python3 cluster.py
```

```text output
shard 0: ok, 3 of 3 copies answer; primary s0a
  s0a  primary  ok  serves reads
  s0b  replica  ok  serves reads
  s0c  replica  ok  serves reads
shard 1: ok, 3 of 3 copies answer; primary s1a
  s1a  primary  ok  serves reads
  s1b  replica  ok  serves reads
  s1c  replica  ok  serves reads
```

A shard's `state` is `ok`, `degraded` (a copy down, the shard still serving), `no_writes`, `no_reads` or `down`, and `why` says it in a sentence. `GET /v1/health/fleet` sums up the whole cluster in one headline, with each problem's impact, what to do next and the [runbook](../chloedb/runbooks.md) for it; it is what the command center's Home page shows:

```sh
curl -s localhost:7700/v1/health/fleet -H "authorization: Bearer $ADMIN_TOKEN" \
  | python3 -c 'import json, sys; f = json.load(sys.stdin); print(f["status"], "|", f["headline"])'
```

```text output
ok | All 2 shards serve reads and take writes; 6 of 6 copies are up...
```

For a load balancer or a process supervisor, every server answers `GET /livez` (the process runs) and `GET /readyz` (it serves) without a credential. The command center is the coordinator's address in a browser, `http://127.0.0.1:7700`, which asks for the admin token or a key; `chloedb-server cc http://127.0.0.1:7700` is the same in a terminal, with the key in `CHLOEDB_KEY` or typed at its prompt.

## Keys for clients

Give each client a key of its own, with the least scope it needs: here a write key for an application and a read key for Prometheus. Keys are made on the coordinator, and reach every copy of every shard within a few seconds:

```sh
key() {  # key JSON: make an API key, print its secret (shown this once)
  curl -s -X POST localhost:7700/v1/admin/keys -H "authorization: Bearer $ADMIN_TOKEN" \
    -H 'content-type: application/json' -d "$1" | python3 -c 'import json, sys; print(json.load(sys.stdin)["secret"])'
}
export APP_KEY=$(key '{"name": "shop-app", "scope": "write", "expires_days": 90}')
export MONITOR_KEY=$(key '{"name": "prometheus", "scope": "read"}')
```

Each copy keeps the keys, as SHA-256 digests and never the secrets, in `db.chloedb.keys.json` beside its database, so a coordinator started again takes them from its shards. The read key scrapes every server's `/metrics`, the copies' included. On a shard's primary, its replicas' state is there, by replica:

```sh
until curl -sf localhost:7711/metrics -H "authorization: Bearer $MONITOR_KEY" -o s0a.metrics; do sleep 0.5; done
grep -E '^chloedb_replica_(in_sync|behind)' s0a.metrics
```

```text output
chloedb_replica_in_sync{replica="http://127.0.0.1:7722"} 1
chloedb_replica_in_sync{replica="http://127.0.0.1:7723"} 1
chloedb_replica_behind{replica="http://127.0.0.1:7722"} 0
chloedb_replica_behind{replica="http://127.0.0.1:7723"} 0
```

Scrape the coordinator and each copy as targets of their own, with the key in Prometheus's `authorization.credentials_file`. The coordinator's `/metrics` has its view of the cluster, a series a shard, the state its built-in alerts judge: `chloedb_cluster_copies`, `chloedb_cluster_copies_up` and `chloedb_cluster_copies_down`, `chloedb_cluster_copies_in_sync`, `chloedb_cluster_shard_majority` and `chloedb_cluster_shard_primary` (1 or 0), `chloedb_cluster_shard_term`, `chloedb_cluster_primary_changes_total` (its failovers and handovers), and `chloedb_cluster_copy_up{shard, copy, url, role}` a copy. A copy's own series carry no shard label: its `chloedb_copy_info{shard, node, role} 1` says which shard it serves, to join on, or add the label in the scrape config. `chloedb_docs` and `chloedb_segments` count every table of a copy's database (`chloedb_docs` is `/v1/health`'s `rows`), and `chloedb_table_docs{table}` each one. The coordinator's alerts go to a webhook, PagerDuty or Opsgenie ([alerts](../chloedb/server.md#alerts)); `--alert-webhook URL` sends them all to one URL.

## Load and search

Making a table takes the admin token, and runs on every shard:

```sh
curl -s localhost:7700/v1/sql -H "authorization: Bearer $ADMIN_TOKEN" -H 'content-type: application/json' \
  -d '{"sql": "CREATE TABLE products (id KEYWORD PRIMARY KEY, title TEXT, category KEYWORD, price FLOAT)"}' \
  | python3 -c 'import json, sys; print(json.load(sys.stdin).get("error", "made"))'
```

```text output
made
```

The application writes with its key. Each row goes to the shard whose slots hold its key, and the answer counts what each shard took. Give every write an idempotency key: when an answer is lost, or a shard's primary changes under it, the same request sent again with the same key is applied once ([when a write fails](../chloedb/server.md#when-a-write-fails)).

```sh
python3 - > rows.json <<'EOF'
import json
titles = ["red wool sofa", "blue linen sofa bed", "oak dining table", "walnut coffee table",
          "green velvet armchair", "steel desk lamp", "rattan garden chair", "pine bookshelf"]
ops = [{"op": "upsert", "table": "products",
        "doc": {"id": f"p{i}", "title": t, "category": t.split()[-1], "price": 100.0 + 10 * i}}
       for i, t in enumerate(titles)]
print(json.dumps({"ops": ops, "idempotency_key": "load-2026-11-17"}))
EOF
curl -s localhost:7700/v1/write -H "authorization: Bearer $APP_KEY" -H 'content-type: application/json' -d @rows.json \
  | python3 -c 'import json, sys; r = json.load(sys.stdin); print(r["written"], "rows:", ", ".join("shard %d %d" % (s["shard"], s["rows"]) for s in r["receipt"]["per_shard"]))'
```

```text output
8 rows: shard 0 3, shard 1 5
```

A search asks every shard, with the whole collection's BM25 statistics, so it ranks as one database would:

```sh
curl -s localhost:7700/v1/sql -H "authorization: Bearer $APP_KEY" -H 'content-type: application/json' \
  -d "{\"sql\": \"SELECT id, title FROM products WHERE MATCH(title, 'sofa') ORDER BY bm25(title, 'sofa') DESC LIMIT 5\"}" \
  | python3 -c 'import json, sys; [print(*row, sep=" | ") for row in json.load(sys.stdin)["rows"]]'
```

```text output
p0 | red wool sofa
p1 | blue linen sofa bed
```

[What a cluster does differently](../chloedb/cluster-limits.md) lists the few statements a coordinator refuses or runs another way.

## Kill a primary

`s0a` is shard 0's primary. Kill it without warning, as a crashed machine would, and send a write for shard 0 straight away:

```sh
kill -9 $(cat s0a/pid)
row='{"ops": [{"op": "upsert", "table": "products", "doc": {"id": "p20", "title": "cherry side table", "category": "table", "price": 80.0}}], "idempotency_key": "order-p20"}'
until curl -sf localhost:7700/v1/write -H "authorization: Bearer $APP_KEY" -H 'content-type: application/json' -d "$row" -o answer.json; do sleep 0.5; done
python3 -c 'import json; r = json.load(open("answer.json")); print(r["written"], "row on shard", [s["shard"] for s in r["receipt"]["per_shard"]][0])'
```

```text output
1 row on shard 0
```

Until shard 0 has a new primary, about two seconds here, its writes are refused with 503 and an answer that says what happened and that the write is safe to send again:

```json norun
{"error": "127.0.0.1:7721 (shard 0): tcp connect error", "error_code": "unavailable", "retryable": true,
 "hint": "a copy, a shard or a hosted model did not answer: send it again after Retry-After seconds",
 "docs": "https://chloetech-ai.github.io/chloedb/site/docs/server.html#failover"}
```

The loop above sends it again, with the same idempotency key, until a new primary takes it.

A primary that freezes rather than dies (a stopped process, a long pause, a disk that hangs) stalls a write longer than a kill does. Its copies elect another one within about two seconds as before, but the elected copy takes writes only once the old primary's lease has run out (about two seconds more with `--failover-after-ms 2000`), and a write sent meanwhile is held rather than refused: in a drill on this page's cluster, with the primary stopped by `kill -STOP` under a steady writer, one write was held 6.8 s and then answered 200, and none failed. For those few seconds the coordinator shows the new primary as taking no writes. Leave it alone: it is waiting out the lease, and restarting it while the old primary is still frozen leaves the shard one copy of three, with no majority and no writes until a second copy answers.

Shard 1's writes, and the searches of both shards, went on all along: a replica serves reads while its primary's last call found it caught up. Every write answered before the kill is on a majority of shard 0's copies (`--write-acks all`, the default, waits for every replica in sync and never fewer than a majority), and the elected copy holds all of them.

The coordinator now sees `s0a` down and another copy as shard 0's primary:

```sh
python3 cluster.py
```

```text output
shard 0: degraded, 2 of 3 copies answer (127.0.0.1:7721 down); the shard serves reads and takes writes; one more copy down stops its reads and writes; primary s0...
  s0a    down
...
shard 1: ok, 3 of 3 copies answer; primary s1a
  s1a  primary  ok  serves reads
  s1b  replica  ok  serves reads
  s1c  replica  ok  serves reads
```

Bring `s0a` back with the command that started it, without its role flags. A copy that was its shard's primary starts again as a replica ("was its shard's primary: starts as a replica, until its shard's copies elect one", its log says), finds the primary its shard elected meanwhile, and follows it: it is sent what it missed, and serves reads once it has caught up. Started again with `--replicas`, as at its first start, it would start as the primary of its old term and log errors until the newer primary fenced it; so give the role flags on a copy's first start alone.

```sh
copy s0a 7711 7721
until python3 cluster.py --ok 2> /dev/null; do sleep 0.5; done
python3 cluster.py
```

```text output
shard 0: ok, 3 of 3 copies answer; primary s0...
  s0a  replica  ok  serves reads
...
```

A shard of three copies with two of them down has no majority: it takes no writes and elects no primary until a second copy answers. Its reads go on only from a replica that still serves them. When a copy will not come back, replace it. Start an empty copy as its shard's copies were started (with `--replica --replicas` naming the others), then plan the repair: `POST /v1/admin/plan {"shards": [{"shard": 1, "copies": [the gRPC URLs the shard keeps, the new one among them]}]}` adds the copies it names that are new and removes those it leaves out, and changes nothing until it is applied. Adding a server takes the cluster token, which the coordinator sends to it: apply the plan with that token rather than the admin token, `CHLOEDB_URL=http://localhost:7700 chloedb-server admin --key "$CHLOEDB_CLUSTER_TOKEN" call POST /v1/admin/plan/apply @plan.json`, with the same body in `plan.json`. The command center's **Cluster** page plans it too ("Plan the repair"); there, Apply needs a sign-in with the cluster token. The [runbooks](../chloedb/runbooks.md#a-copy-is-down) say what to read and do for each alert.

## Checkpoints

Copying a database's files while its server runs is not a backup: a copy can pair one version's manifest with another version's pages. A checkpoint is: the copy writes a consistent copy of its database into its `--backup-dir` while it goes on serving. Through the coordinator, ask one copy of each shard (any copy will do; a replica keeps the work off the primary):

```sh
for c in s0b s1b; do
  curl -s -X POST localhost:7700/v1/admin/copies/checkpoint -H "authorization: Bearer $ADMIN_TOKEN" \
    -H 'content-type: application/json' -d "{\"url\": \"$G:$(cut -d' ' -f2 $c/ports)\", \"name\": \"$c-before-upgrade\"}" \
    | python3 -c 'import json, sys; r = json.load(sys.stdin); print(r.get("error") or "written by " + r["node"])'
done
ls s0b/backups s1b/backups
```

```text output
written by http://127.0.0.1:7722
written by http://127.0.0.1:7725
s0b/backups:
s0b-before-upgrade
s0b-before-upgrade.keys.json
s0b-before-upgrade.slots

s1b/backups:
s1b-before-upgrade
s1b-before-upgrade.keys.json
s1b-before-upgrade.slots
```

Beside each checkpoint the copy writes its shard's cluster map (`NAME.slots`) and the cluster's API keys (`NAME.keys.json`: names, scopes and the hashes of their secrets), under the names a server serving the checkpoint reads. `POST /v1/admin/checkpoint {"name": "..."}` on the coordinator writes one on every shard's primary at once, all of one cluster map. A name is letters, digits, `.`, `_` and `-`, and an existing one is never overwritten. The command center writes one from a copy's menu on its **Cluster** page, and one of every shard from **Settings > Upkeep**. `GET /v1/admin/checkpoints` on the coordinator lists the checkpoints of every copy (name, size, when it was written, the shard and map epoch it is of), each shard's newest, and the names written on every shard under one cluster map, which are the sets a restore can go back to; **Settings > Upkeep > Backups** shows them. Copy the three files of each checkpoint off the machines: with `"layout": "dir"` a checkpoint is a directory of segment files, which a sync to object storage sends only the new ones of ([backups](../chloedb/server.md#backups)).

To restore, stop the server and put the checkpoint in place of its database with `chloedb-server restore --from s0b/backups/s0b-before-upgrade --db s0b/db.chloedb`: it checks the checkpoint before it changes anything, moves the database's own files aside, puts the map beside it and merges the keys in. A copy's part in replication (`db.chloedb.replication`) is not in a checkpoint: start a restored copy with the role flags it had. The runbook [Restore from checkpoints](../chloedb/runbooks.md#restore-from-checkpoints) restores a whole cluster, a shard or one copy this way, and [restoring a checkpoint](../chloedb/server.md#restoring-a-checkpoint) says what the command does. There is no point-in-time recovery between checkpoints.

## Upgrade one server at a time

A rolling upgrade keeps the cluster answering searches and taking writes, one server at a time:

1. In each shard, the replicas first, one at a time: stop it, start it again on the new binary, and wait until it has caught up and serves reads. A replica must not be older than its primary, which may write what an older version cannot read.
2. Then hand the shard's primary over to an upgraded replica (`POST /v1/admin/shards/<i>/promote`), and upgrade the old primary, now a replica, as the others.
3. The coordinators last, one at a time.

While versions are mixed, the cluster serves what the oldest server involved speaks, and [versions and compatibility](../compatibility.md#a-cluster-of-mixed-versions) says, release by release, what has an order of its own. Read the [changelog](../../CHANGELOG.md) of every release you skip.

On each machine, the new binary is the release's archive, checked against its `.sha256`:

```sh
# install
v=0.3.9 p=linux-x86_64
curl -fLO https://github.com/chloetech-ai/chloedb/releases/download/chloedb-server-v$v/chloedb-server-$p.tar.gz
curl -fLO https://github.com/chloetech-ai/chloedb/releases/download/chloedb-server-v$v/chloedb-server-$p.tar.gz.sha256
sha256sum -c chloedb-server-$p.tar.gz.sha256 && tar xzf chloedb-server-$p.tar.gz
sudo install chloedb-server-$p/chloedb-server /usr/local/bin/chloedb-server   # where the unit runs it from
```

From 0.3.8, `gh attestation verify` checks that the release workflow built the archive, before you install it on every machine: [checking what you download](../../SECURITY.md#checking-what-you-download).

Here the new binary is the one on your `PATH` already, so the upgrade is a rolling restart in the same order, which is how you rehearse one. `restart` stops a copy with SIGTERM (it ends the requests under way first), starts it again without role flags, waits until the copy's own `/v1/health` says it serves reads (a replica does once it holds its primary's last transaction), and then until the coordinator sees the whole cluster well. Wait for both before the next copy: two copies of a shard down at once leave it without a majority. `handover` makes a copy its shard's primary:

```sh
restart() {  # restart NAME
  kill $(cat $1/pid); while kill -0 $(cat $1/pid) 2> /dev/null; do sleep 0.2; done
  copy $1 $(cat $1/ports)
  until curl -sf localhost:$(cut -d' ' -f1 $1/ports)/v1/health -H "authorization: Bearer $MONITOR_KEY" \
    | python3 -c 'import json, sys; sys.exit(0 if json.load(sys.stdin)["replication"]["serves_reads"] else 1)' 2> /dev/null; do sleep 0.5; done
  until python3 cluster.py --ok 2> /dev/null; do sleep 0.5; done
}
handover() {  # handover SHARD NAME
  curl -sf -X POST localhost:7700/v1/admin/shards/$1/promote -H "authorization: Bearer $ADMIN_TOKEN" \
    -H 'content-type: application/json' -d "{\"url\": \"$G:$(cut -d' ' -f2 $2/ports)\"}" > /dev/null
  until [ "$(python3 cluster.py --primary $1)" = "$2" ]; do sleep 0.5; done
}
for shard in 0 1; do
  primary=$(python3 cluster.py --primary $shard)
  replicas=$(python3 cluster.py --replicas $shard)
  for r in $replicas; do restart $r; done
  handover $shard ${replicas%% *}
  restart $primary
done
python3 cluster.py --versions
```

`--versions` lists which servers run which version: halfway through a real upgrade it shows two. The coordinator goes last. With one coordinator, its clients are refused for the second it takes to start again; with several behind a load balancer, restart them one at a time, each with `--drain-ms 5000`, so that `/readyz` answers 503 for five seconds after SIGTERM and the load balancer sends it nothing more before it stops:

```sh
kill $(cat coord.pid); while kill -0 $(cat coord.pid) 2> /dev/null; do sleep 0.2; done
CHLOEDB_TOKEN=$ADMIN_TOKEN chloedb-server --listen 127.0.0.1:7700 --grpc off --node-name coord --start-apps off \
  --shards "$G:7721|$G:7722|$G:7723,$G:7724|$G:7725|$G:7726" >> coord.log 2>&1 &
echo $! > coord.pid
until python3 cluster.py --ok 2> /dev/null; do sleep 0.5; done
curl -s localhost:7700/v1/sql -H "authorization: Bearer $APP_KEY" -H 'content-type: application/json' \
  -d '{"sql": "SELECT COUNT(*) FROM products"}' | python3 -c 'import json, sys; print(json.load(sys.stdin)["rows"][0][0], "rows")'
```

```text output
9 rows
```

Every row is there. Under writes, a replica's restart costs the clients nothing: its shard's writes wait for the copies in sync, and reads go to the other copies. A handover holds the shard's writes for a moment, a second at most, while the new primary catches up: the coordinator sends a write that meets it on to the new primary, and a write that was in flight as the primary changed may be answered 409, in doubt, which the same request with the same idempotency key settles.

## Next

- [Runbooks](../chloedb/runbooks.md): what to read and do for each alert and each problem the fleet view names.
- [chloedb-server](../chloedb/server.md): every flag; [replication](../chloedb/server.md#replication-a-primary-and-its-replicas), [failover](../chloedb/server.md#failover) and [slots](../chloedb/server.md#slots) in depth; adding shards and copies while the cluster serves.
- [Security model](../chloedb/security.md): what the tokens and keys protect, and what to set before a cluster holds data that matters.
- [Versions and compatibility](../compatibility.md): what mixes in a cluster during an upgrade.
