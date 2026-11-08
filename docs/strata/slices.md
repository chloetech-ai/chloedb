---
title: Search on the device
parent: Strata
nav_order: 8.7
---

# Slices: "near me" search on the device, the server as fallback
{: .no_toc }

A slice is a copy of the rows of a table near a device's user, which the device searches itself. strata-server builds it from a table with a text field and a latitude and longitude: a manifest (the schema, a grid of square tiles, the text field's statistics) and one file of rows per tile. An app fetches the manifest and the tiles around its user, and searches them with the server's statistics, so every place it holds scores exactly as on the server. When it can prove that its answer is the server's, nothing leaves the device; otherwise it sends the same query to the server. Each answer says where it was answered and why.
{: .fs-5 .fw-300 }

1. TOC
{:toc}

## When an answer comes from the device

The server ranks by `bm25(text, q) / (1 + d² / d0²)`, where `d` is the distance to the user and `d0` the distance at which a place counts half. A place the device does not hold is at least `e` away, the distance to the nearest tile it lacks, so it scores at most the sum of the query words' best scores (in the manifest) divided by `1 + e² / d0²`. When the k-th score on the device reaches that, the device's top k is the server's. Each answer's `why` is one of:

| why | where | what it means |
|---|---|---|
| `proved` | device | the k-th local score beats the most a place not held could score |
| `no_match` | device | no place in the table has a word of the search |
| `short` | server | the device holds fewer than k matching places |
| `bound` | server | a place beyond the tiles held could have scored higher |
| `stale` | server | the table changed since the slice was built |

So a device answers short searches for common things near its user (pizza, a pharmacy) itself, and asks the server for rare things, for large k, and for wide `d0`.

## Turn it on

Slices exist only on a server started with `--slices DIR`. Build one from the command center (**Data → Slices → Build a slice**) or over HTTP:

```bash
strata-server --db places.strata --slices ./slices --slices-rebuild-s 300 &
curl -X POST localhost:7700/v1/admin/slices \
  -d '{"text": "body", "lat": "lat", "lon": "lon", "lat0": 40.7, "lon0": -73.98, "tile_m": 1000}'
```

`lat0`, `lon0` are the grid's origin (the middle of the area, usually), `tile_m` the side of a tile in metres. `POST /v1/admin/slices` with `{"table": "places"}` alone rebuilds a slice as it was built, and `--slices-rebuild-s N` rebuilds every stale slice each N seconds. Without a slice the helpers below fail to open, and an app sends its searches to the server as it would without them.

## Fresh or stale

Before a device trusts its slice it asks `GET /v1/slices/<table>/status` (every 30 seconds by default), which answers the slice's version and whether the table changed since the build. A device fetches a slice of another version, and while the table has changed it sends every search to the server (`max_lag` lets it keep answering for a while). A slice is a snapshot: a row written after the build is not on any device until the next build.

## From Python

```python
import strata

near = strata.nearby("http://127.0.0.1:7700", "places", "./slice")
near.move_to(40.7484, -73.9857)       # fetches the tiles around the user
r = near.search("pizza", k=10, d0=1000)
print(r.where, r.why)                 # "device" or "server", and why
for row in r:
    print(row["name"])
near.close()
```

`strata.nearby(url, table, dir, token=None, reach=2, status_every=30, max_lag=0, report_every=20)`: `reach` is how many tiles each way it holds (2 is a 5 by 5 square), `dir` a cache it may delete. The result is a `Result` with `where`, `why`, `bound`, `edge_m`, `device_ms`, `server_ms`, and `ask`, the request that asks the server.

## From JavaScript

```js
import { openNearby } from "strata-db";

const near = await openNearby("http://127.0.0.1:7700", "places", "./slice");
await near.moveTo(40.7484, -73.9857);
const r = await near.search("pizza", { k: 10, d0: 1000 });
console.log(r.where, r.why);
for (const row of r) console.log(row.name);
await near.close();
```

It runs where the native addon does (Node, Bun, Deno), with the same options as Python's in camel case (`statusEvery` and `maxLag` in milliseconds).

## What the server sees

Each helper reports its counts every 20 searches (`POST /v1/slices/<table>/usage`: searches, why, time on the device and on the server), and the server counts the manifests and tiles it serves. The command center's **Slices** view shows them since the server started: the share answered on the device, the devices, a map of the tiles by rows, size or downloads, the reasons searches went to the server, the last hour by minute, the busiest tiles, and the slice's version with Rebuild and Delete.

![The Slices view: eight devices around New York, 65% of their searches answered on the device, the tiles they downloaded on the map, and why the rest went to the server](../assets/img/slices-view.png)

| route | what |
|---|---|
| `POST /v1/admin/slices` | builds (or, with `{table}` alone, rebuilds) a table's slice |
| `GET /v1/admin/slices` | every slice, with its counts, and the tables that could have one |
| `GET`, `DELETE /v1/admin/slices/<table>` | a slice's tiles and last hour; deletes it |
| `GET /v1/slices/<table>/manifest.json`, `/<x>_<y>.jsonl.gz` | the files a device fetches (read scope) |
| `GET /v1/slices/<table>/status` | version, built, fresh, changed |
| `POST /v1/slices/<table>/usage` | a device's counts |

`strata-slice` (in the strata crate) writes the same files from a database file without a server, for any web server or CDN to serve. `strata-server/demo/nearby` is a demo over New York's places.
