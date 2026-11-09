"""A city's places from Overture Maps (release 2026-10-21.0, CDLA Permissive
2.0) as JSON lines for the nearby demo's `places` table (schema.json): open
places (or of unknown status) with a name and confidence 0.5 or more. A
place's `body`, the text searched, is its name, the words of its categories
(Overture's taxonomy), its brand and its locality, as in the Phase 0 study
(strata/results/slices/geo.py).

    python3 places.py --city nyc --out places.jsonl

Reads Overture's public S3 bucket anonymously with DuckDB (`pip install
duckdb`); about a minute for New York.
"""
import argparse
import json
import os

RELEASE = "2026-10-21.0"
CITIES = {  # lon_min, lon_max, lat_min, lat_max; the grid's origin
    "nyc": {"box": (-74.26, -73.70, 40.49, 40.92), "origin": (40.7, -73.98)},
    "houston": {"box": (-95.63, -95.11, 29.53, 29.99), "origin": (29.76, -95.37)},
}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--city", default="nyc", choices=sorted(CITIES))
    ap.add_argument("--out", required=True)
    ap.add_argument("--parquet", help="an earlier download of the box (written beside --out otherwise)")
    a = ap.parse_args()
    import duckdb
    box = CITIES[a.city]["box"]
    src = a.parquet or os.path.splitext(a.out)[0] + f"-{a.city}.parquet"
    con = duckdb.connect()
    if not os.path.exists(src):
        for k in ("AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "AWS_SESSION_TOKEN"):
            os.environ.pop(k, None)  # the bucket is public: read it anonymously
        con.sql("LOAD httpfs; SET s3_region='us-west-2';")
        con.sql(f"""COPY (
          SELECT id, names."primary" AS name, taxonomy."primary" AS category, taxonomy.hierarchy AS hierarchy,
                 taxonomy.alternates AS alternates, basic_category, brand.names."primary" AS brand,
                 addresses[1].freeform AS address, addresses[1].locality AS locality,
                 (bbox.xmin + bbox.xmax) / 2 AS lon, (bbox.ymin + bbox.ymax) / 2 AS lat, confidence, operating_status
          FROM read_parquet('s3://overturemaps-us-west-2/release/{RELEASE}/theme=places/type=place/*.parquet')
          WHERE bbox.xmin > {box[0]} AND bbox.xmax < {box[1]} AND bbox.ymin > {box[2]} AND bbox.ymax < {box[3]}
        ) TO '{src}' (FORMAT parquet)""")
    rows = con.sql(f"""SELECT name, category, basic_category, alternates, hierarchy, brand, address, locality, lon, lat FROM '{src}'
        WHERE coalesce(operating_status, 'open') NOT IN ('permanently_closed', 'temporarily_closed') AND confidence >= 0.5
          AND name IS NOT NULL ORDER BY id""").fetchall()
    with open(a.out, "w") as f:
        for i, (n, c, b, alt, h, br, ad, loc, lon, lat) in enumerate(rows):
            cats = []
            for s in [c, b] + list(alt or []) + list(h or []):
                for w in (s or "").replace("_", " ").split():
                    if w not in cats:
                        cats.append(w)
            body = " ".join(t for t in (n, " ".join(cats), br if br and br != n else "", loc or "") if t)
            f.write(json.dumps({"id": i, "name": n, "body": body, "category": (c or "").replace("_", " "),
                                "address": ad or "", "lat": lat, "lon": lon}) + "\n")
    lat0, lon0 = CITIES[a.city]["origin"]
    print(f"{len(rows)} places -> {a.out}; grid origin {lat0} {lon0}")


if __name__ == "__main__":
    main()
