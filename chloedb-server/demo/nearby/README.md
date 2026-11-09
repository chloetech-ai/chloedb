# Nearby: search on the device, with the cloud as fallback

A demo of slices ([`chloedb::slice`](../../../chloedb/src/slice/mod.rs)): a
phone holds the places of the tiles around its user and answers "near me"
searches there. When it can prove that its answer is the cloud's, nothing
leaves the phone; otherwise it asks the cloud.

- **The cloud** is a chloedb-server with a city's places (Overture Maps,
  release 2026-10-21.0, CDLA Permissive 2.0). `POST /v1/admin/slices`
  builds the table's slice into `--slices DIR`, and `GET
  /v1/slices/places/...` serves it. A slice is a manifest (the schema, the
  grid, the text field's statistics, and each word's document frequency and
  best score) plus one gzipped file of rows per 1 km tile.
- **The phone** is `chloedb-nearby`, a process that embeds ChloeDB as an app
  would. It fetches the manifest once, then the tiles of the square around
  its user (5 km on a side by default) as the user moves. It searches them
  with the cloud's statistics, so every place it holds scores as in the
  cloud.
- **The proof.** The cloud ranks by `bm25(body, q) / (1 + d² / d0²)`. A
  place the phone lacks is at least `e` away, the distance to the nearest
  tile it does not hold, so it scores at most the sum of the query words'
  best scores divided by `1 + e² / d0²`. When the k-th local score reaches
  that, the local top k is the cloud's.

The page shows the city's tiles, the tiles held, and the places in them.
Drag the blue dot (or click the map) to move, then search. Each answer says
where it was answered and why, and "Also ask the cloud" compares the two
answers.

Every 30 seconds the phone asks the cloud whether its slice is current
(`GET /v1/slices/places/status`): it fetches a newer one, and while the
table has changed since the build it asks the cloud for every search. It
reports its counts every 5 searches (`POST /v1/slices/places/usage`,
named by `--device NAME`), which the command center's Slices view
(http://127.0.0.1:7700/#/slices) shows with the tiles each device
downloaded. Apps do the same with `strata.nearby` (Python) and
`openNearby` (JavaScript): [Search on the device](../../../docs/chloedb/slices.md).

## Run it

From the repository root, with Rust and Python 3 (`pip install duckdb` for
the places):

```bash
(cd chloedb-server && cargo build --release --bin chloedb-server --bin chloedb-nearby)
chloedb-server/demo/nearby/run.sh            # CITY=houston for Houston; DIR=~/nearby by default
```

Then open http://127.0.0.1:7740/. The first run downloads the city's places
from Overture's public bucket, about a minute for New York.

The pieces, one at a time:

```bash
python3 chloedb-server/demo/nearby/places.py --city nyc --out ~/nearby/nyc.jsonl
chloedb-server import --db ~/nearby/nyc.strata --schema chloedb-server/demo/nearby/schema.json ~/nearby/nyc.jsonl
chloedb-server --db ~/nearby/nyc.strata --slices ~/nearby/slices &
curl -X POST localhost:7700/v1/admin/slices -d '{"text": "body", "lat": "lat", "lon": "lon", "lat0": 40.7, "lon0": -73.98}'
chloedb-nearby --cloud http://127.0.0.1:7700 --lat 40.7 --lon -73.98
```

`chloedb-slice` (in the strata crate) writes the same files from a database
file without a server, for any web server or CDN to serve.
