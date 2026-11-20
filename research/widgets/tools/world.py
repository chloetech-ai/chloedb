#!/usr/bin/env python3
"""Write the world map both halves draw flights on, from the backbone app's
outlines (Natural Earth 1:110m, public domain; equirectangular, 80N to 58S,
1000 wide):

  server/assets/world.json  every country's outline (for "over which country")
                            and one path of all land, rounded to whole units
  hooks/world.ts            that path and the projection, for the mod

    python3 tools/world.py
"""
import json
import pathlib
import re

HERE = pathlib.Path(__file__).resolve().parent.parent
GEO = HERE.parent.parent / "chloedb-backbone" / "ui" / "geo.json"

geo = json.loads(GEO.read_text())
paths, countries = [], {}
for code, c in geo["countries"].items():
    if not c.get("d"):
        continue
    countries[code] = {"n": c["n"], "b": c["b"], "d": c["d"]}
    paths.append(re.sub(r"\d+\.\d+", lambda m: str(round(float(m.group(0)))), c["d"]))
world = re.sub(r"L(\d+,\d+)(?:L\1)+", r"L\1", "".join(paths))
frame = {k: geo[k] for k in ("w", "h", "top", "span")}

(HERE / "server" / "assets" / "world.json").write_text(
    json.dumps({**frame, "source": geo["source"], "countries": countries, "world": world}, separators=(",", ":"))
)
(HERE / "hooks" / "world.ts").write_text(
    "// Written by tools/world.py: Natural Earth 1:110m land (public domain),\n"
    "// equirectangular from 80N to 58S, 1000 units wide.\n"
    f"export const WORLD = {json.dumps({**frame, 'path': world}, separators=(',', ':'))} as const\n"
)
print(f"world path: {len(world)} characters, {len(countries)} countries")
