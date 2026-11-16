"""Fetch Parquet files from the other nodes' `prepare` folders.

Each node serves its /work/out over HTTP (python3 -m http.server 8000) with
a list of its files per shard, manifest-sN.txt. A node then pulls what it
needs into its own tree, keeping the paths (TABLE/sN/DAY.parquet, so that
the days of every node sit side by side in one folder a table):

    python3 pull.py --peers 10.0.0.2,10.0.0.3 --shard 1 --into /work/in
    python3 pull.py --peers ... --shard all --tables actors,names --into /work/maps

Files already there at the same size are skipped, so a pull started again
goes on. Standard library only.
"""

import argparse
import concurrent.futures as cf
import os
import sys
import time
import urllib.request


def get(url, tries=8):
    wait = 2
    for i in range(tries):
        try:
            with urllib.request.urlopen(url, timeout=120) as r:
                return r.read()
        except Exception as e:  # noqa: BLE001 - any failure is retried
            if i == tries - 1:
                raise
            print(f"pull: {url}: {e}; again in {wait} s", file=sys.stderr, flush=True)
            time.sleep(wait)
            wait = min(wait * 2, 60)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--peers", required=True, help="the other nodes' addresses, comma-separated")
    ap.add_argument("--port", type=int, default=8000)
    ap.add_argument("--shard", required=True, help="a shard number, or all")
    ap.add_argument("--shards", type=int, default=4)
    ap.add_argument("--tables", default="", help="only these tables (comma-separated)")
    ap.add_argument("--into", required=True)
    ap.add_argument("--jobs", type=int, default=8)
    a = ap.parse_args()
    shards = range(a.shards) if a.shard == "all" else [int(a.shard)]
    tables = {t for t in a.tables.split(",") if t}
    todo = []
    for peer in [p for p in a.peers.split(",") if p]:
        base = f"http://{peer}:{a.port}"
        for s in shards:
            for line in get(f"{base}/manifest-s{s}.txt").decode().splitlines():
                path = line.strip().removeprefix("./")
                if not path or (tables and path.split("/")[0] not in tables):
                    continue
                size, _, rel = path.partition(" ")
                todo.append((base, rel, int(size)))
    total = sum(t[2] for t in todo)
    print(f"pull: {len(todo)} files, {total / 1e9:.1f} GB", flush=True)
    done = [0, 0]
    t0 = time.time()

    def one(item):
        base, rel, size = item
        dst = os.path.join(a.into, rel)
        if os.path.exists(dst) and os.path.getsize(dst) == size:
            return size
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        body = get(f"{base}/{rel}")
        if len(body) != size:
            raise RuntimeError(f"{rel}: {len(body)} bytes, the list says {size}")
        tmp = dst + ".part"
        with open(tmp, "wb") as f:
            f.write(body)
        os.replace(tmp, dst)
        return size

    with cf.ThreadPoolExecutor(a.jobs) as ex:
        for n in ex.map(one, todo):
            done[0] += 1
            done[1] += n
            if done[0] % 500 == 0 or done[0] == len(todo):
                secs = max(time.time() - t0, 1)
                print(f"pull: {done[0]}/{len(todo)} files, {done[1] / 1e9:.1f} GB, {done[1] / 1e6 / secs:.0f} MB/s", flush=True)


if __name__ == "__main__":
    main()
