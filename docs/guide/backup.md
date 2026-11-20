# Back up, restore and upgrade

Before ChloeDB holds data someone depends on, whoever runs it will ask three things. How is it backed up? How is a backup put back after a bad write or a lost disk? How is it upgraded, and rolled back if the new version misbehaves? This page answers all three for one server and runs every step. The same page runs in CI on every change, so the commands and what they print are rehearsed. A cluster does the same per copy: see [Operate a cluster](operate-cluster.md#checkpoints) and the [restore runbook](../chloedb/runbooks.md#restore-from-checkpoints).

<!-- run: needs chloedb-server -->

You need `chloedb-server` on your `PATH` ([install](../quickstart.md)), and `curl` and `python3`.

## A server with a backup folder

`--backup-dir` names the folder its checkpoints go to. Put it on another disk, or sync it off the machine (below), since a backup on the database's own disk goes with that disk.

```sh
mkdir -p backups
chloedb-server --db ./shop.chloedb --backup-dir ./backups --listen 127.0.0.1:7700 --grpc off --start-apps off > server.log 2>&1 &
echo $! > server.pid
until curl -sf http://127.0.0.1:7700/v1/health > /dev/null; do sleep 0.2; done
export ADMIN_KEY=$(curl -s -X POST localhost:7700/v1/admin/keys -H 'content-type: application/json' \
  -d '{"name": "admin", "scope": "admin"}' | python3 -c 'import json, sys; print(json.load(sys.stdin)["secret"])')
```

A table and a thousand orders to keep safe:

```sh
curl -s localhost:7700/v1/sql -H "authorization: Bearer $ADMIN_KEY" -H 'content-type: application/json' \
  -d '{"sql": "CREATE TABLE orders (id KEYWORD PRIMARY KEY, customer KEYWORD, total FLOAT)"}' > /dev/null
python3 - <<'EOF' > orders.json
import json
ops = [{"op": "upsert", "table": "orders", "doc": {"id": f"o{i}", "customer": f"c{i % 40}", "total": float(i % 97)}} for i in range(1000)]
print(json.dumps({"ops": ops}))
EOF
curl -s localhost:7700/v1/write -H "authorization: Bearer $ADMIN_KEY" -H 'content-type: application/json' -d @orders.json \
  | python3 -c 'import json, sys; print(json.load(sys.stdin)["written"], "rows written")'
```

```text output
1000 rows written
```

## Back up

Copying the database's files while the server runs is not a backup: a copy can pair one version's manifest with another's pages ([why](../chloedb/server.md#backups)). A checkpoint is a consistent copy, made while the server serves, and checked against its checksums before it is kept:

```sh
curl -s localhost:7700/v1/admin/checkpoint -H "authorization: Bearer $ADMIN_KEY" -H 'content-type: application/json' \
  -d '{"name": "nightly"}' | python3 -c 'import json, sys; c = json.load(sys.stdin)["checkpoint"]; print(c["name"], c["live_docs"], "rows")'
```

```text output
nightly 1000 rows
```

Run it from cron or a systemd timer with a dated name (`shop-$(date +%F)`), and list what is kept with `GET /v1/admin/checkpoints`. A directory checkpoint (`"layout": "dir"`, `"link_from"` the one before) shares the files that did not change with the one before, so syncing each one to object storage sends only what changed ([how](../chloedb/server.md#backups)).

## Restore after a bad write

Someone deletes the wrong rows:

```sh
curl -s localhost:7700/v1/sql -H "authorization: Bearer $ADMIN_KEY" -H 'content-type: application/json' \
  -d "{\"sql\": \"DELETE FROM orders WHERE customer = 'c7'\"}" > /dev/null
curl -s localhost:7700/v1/sql -H "authorization: Bearer $ADMIN_KEY" -H 'content-type: application/json' \
  -d '{"sql": "SELECT COUNT(*) FROM orders"}' | python3 -c 'import json, sys; print(json.load(sys.stdin)["rows"][0][0], "rows")'
```

```text output
975 rows
```

Stop the server, put the checkpoint in place of its database, and start it again with the flags it had. `restore` copies the checkpoint and checks every checksum before it changes anything, and moves the database it replaces aside rather than deleting it, so a restore can be undone too:

```sh
kill $(cat server.pid) && while kill -0 $(cat server.pid) 2> /dev/null; do sleep 0.2; done
chloedb-server restore --from ./backups/nightly --db ./shop.chloedb
```

```text output
restored ./shop.chloedb from the checkpoint ./backups/nightly (one file): ...
...
```

```sh
chloedb-server --db ./shop.chloedb --backup-dir ./backups --listen 127.0.0.1:7700 --grpc off --start-apps off >> server.log 2>&1 &
echo $! > server.pid
until curl -sf http://127.0.0.1:7700/v1/health > /dev/null; do sleep 0.2; done
curl -s localhost:7700/v1/sql -H "authorization: Bearer $ADMIN_KEY" -H 'content-type: application/json' \
  -d '{"sql": "SELECT COUNT(*) FROM orders"}' | python3 -c 'import json, sys; print(json.load(sys.stdin)["rows"][0][0], "rows")'
```

```text output
1000 rows
```

The admin key still works: the server's keys are kept beside its database, and `restore` merges those kept with the checkpoint into them (a key revoked since stays revoked). What a checkpoint does not carry is listed under [Restoring a checkpoint](../chloedb/server.md#restoring-a-checkpoint): the log store, connectors' settings, and a coordinator's own state. Writes made after the checkpoint are not in it. ChloeDB has no point-in-time recovery yet, so take checkpoints as often as you can afford to lose writes.

## Upgrade, and roll back

An upgrade is a stop and a start with the new binary on the same `--db`. [Versions and compatibility](../compatibility.md) says what each version promises about the file format: a newer server opens an older database, and once it has written to it an older one may not. So the order is: checkpoint, stop, start the new version, check.

```sh
curl -s localhost:7700/v1/admin/checkpoint -H "authorization: Bearer $ADMIN_KEY" -H 'content-type: application/json' \
  -d '{"name": "before-upgrade"}' > /dev/null
kill $(cat server.pid) && while kill -0 $(cat server.pid) 2> /dev/null; do sleep 0.2; done
# Install the new version here, then start it with the same flags:
chloedb-server --db ./shop.chloedb --backup-dir ./backups --listen 127.0.0.1:7700 --grpc off --start-apps off >> server.log 2>&1 &
echo $! > server.pid
until curl -sf http://127.0.0.1:7700/v1/health > /dev/null; do sleep 0.2; done
curl -s localhost:7700/v1/health -H "authorization: Bearer $ADMIN_KEY" | python3 -c 'import json, sys; h = json.load(sys.stdin); print(h["status"], h["docs"], "rows")'
```

```text output
ok 1000 rows
```

To roll back, stop the new version, install the old one, and restore `before-upgrade` with the old version's `chloedb-server restore`, as above. Writes made between the upgrade and the rollback are not in that checkpoint, so decide how long you watch the new version before you let writes depend on it. A cluster upgrades one server at a time while it serves: see [Upgrade one server at a time](operate-cluster.md#upgrade-one-server-at-a-time).

## An embedded database

A program that opens the database file itself (Python, JavaScript, Rust, Java) backs it up the same way, without a server: `db.checkpoint("backup.chloedb")` in Python writes a consistent, checked copy while the database is open and in use ([Python](../chloedb/python.md)). Restore by opening the copy in place of the file, with the program stopped. A database closed with `db.close()` is whole in its file, so a copy of a closed file is a backup too.

## ChloeCtx's stores

`chloectx backup DEST`, run in a project's directory, copies that project's store while sessions go on ([how](../chloectx/setup.md)). Stores live under `~/.local/share/chloectx/projects/`, and they are transcripts of your sessions: back them up to a place only you can read.
