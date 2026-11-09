#!/usr/bin/env python3
"""Bytes a line of the parsed records (parse.py) takes compressed whole
with Zstandard (level 3, as ClickHouse's ZSTD(1) is weaker and zstd -19
far slower) and as Parquet (pyarrow, Zstandard level 3, dictionary
encoding, row groups of 1M rows), next to the raw log's bytes.

    parquet_zstd.py RAW.log IN.jsonl OUT_DIR    prints one JSON line
"""
import json
import os
import sys

import pyarrow as pa
import pyarrow.json as pj
import pyarrow.parquet as pq
import zstandard


def zstd_size(path, level=3):
    c = zstandard.ZstdCompressor(level=level, threads=-1)
    n = 0
    with open(path, "rb") as f:
        r = c.stream_reader(f)
        while True:
            b = r.read(1 << 20)
            if not b:
                break
            n += len(b)
    return n


def main():
    raw, src, out_dir = sys.argv[1:4]
    os.makedirs(out_dir, exist_ok=True)
    schema = pa.schema([("ts", pa.int64()), ("host", pa.string()), ("service", pa.string()), ("severity", pa.string()), ("pid", pa.int64()), ("body", pa.string())])
    t = pj.read_json(src, parse_options=pj.ParseOptions(explicit_schema=schema), read_options=pj.ReadOptions(block_size=64 << 20))
    t = t.set_column(0, "ts", t.column("ts").cast(pa.timestamp("us", tz="UTC")))
    lines = t.num_rows
    out = os.path.join(out_dir, os.path.basename(src).replace(".jsonl", ".parquet"))
    pq.write_table(t, out, compression="zstd", compression_level=3, row_group_size=1_000_000)
    per = lambda b: round(b / lines, 1)
    print(json.dumps({
        "input": os.path.basename(src), "lines": lines,
        "raw_bytes_per_line": per(os.path.getsize(raw)),
        "raw_zstd3_bytes_per_line": per(zstd_size(raw)),
        "parquet_zstd3_bytes_per_line": per(os.path.getsize(out)),
    }))


if __name__ == "__main__":
    main()
