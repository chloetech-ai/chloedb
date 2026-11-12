"""Next step (d): writing stored KV caches to chloedb-server, base64 in JSON
(today's `{"data": ...}` write) against the binary path (`POST /v1/media` with
the raw bytes, then a write that gives the answered value whole).

The bytes are the shape of Qwen2.5-1.5B's cache (28 layers, 2 KV heads, head
dim 128, bf16 K and V: 1 KiB a token a layer), one row per layer as in
run_tier.py; random, since the server only hashes and stores them. Every row is
read back from /v1/media/<sha256> and compared.

  python bench_write.py --server http://127.0.0.1:7700 --out results/write_paths.json
"""
import argparse, base64, hashlib, json, os, time
import requests

ap = argparse.ArgumentParser()
ap.add_argument("--server", default="http://127.0.0.1:7700")
ap.add_argument("--tokens", default="2048,8192,16384,32768")
ap.add_argument("--layers", type=int, default=28)
ap.add_argument("--bytes-per-token", type=int, default=1024)
ap.add_argument("--out", required=True)
args = ap.parse_args()

S = requests.Session()
try:
    S.post(f"{args.server}/v1/sql", json={"sql": "CREATE TABLE kvwrite (id KEYWORD PRIMARY KEY, path KEYWORD, layer INT, blob MEDIA)"}, timeout=60).raise_for_status()
except requests.HTTPError:
    pass

res = {"args": vars(args), "runs": []}
for n in [int(x) for x in args.tokens.split(",")]:
    per = n * args.bytes_per_token
    blobs = [os.urandom(per) for _ in range(args.layers)]
    run = {"tokens": n, "bytes": per * args.layers}
    for path in ("base64", "binary"):
        t = time.time()
        for li, b in enumerate(blobs):
            doc = {"id": f"{path}/{n}/{li}", "path": path, "layer": li}
            if path == "base64":
                doc["blob"] = {"data": base64.b64encode(b).decode()}
            else:
                r = S.post(f"{args.server}/v1/media", data=b, headers={"content-type": "application/octet-stream"}, timeout=300)
                r.raise_for_status()
                doc["blob"] = r.json()
            S.post(f"{args.server}/v1/write", json={"ops": [{"op": "upsert", "table": "kvwrite", "doc": doc}]}, timeout=300).raise_for_status()
        run[f"{path}_s"] = time.time() - t
        # read back
        t = time.time()
        ok = True
        for b in blobs:
            got = S.get(f"{args.server}/v1/media/{hashlib.sha256(b).hexdigest()}", timeout=300).content
            ok &= got == b
        run[f"{path}_read_s"] = time.time() - t
        run[f"{path}_ok"] = ok
    res["runs"].append(run)
    print(run, flush=True)
    json.dump(res, open(args.out, "w"), indent=1)
