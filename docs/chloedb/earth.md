# Earth Explorer: California and Chile from orbit, on ChloeDB

`chloedb-earth` is an app of a ChloeDB server built on public pictures and records of the ground: every fire pixel the GOES satellites see over California and Chile, every few minutes, grouped into fires as they come; 103,664 places of 3.84 km a side, each with its Sentinel-2 picture's [SigLIP](https://huggingface.co/google/siglip-so400m-patch14-384) vector from [Major TOM](https://huggingface.co/datasets/Major-TOM/Core-S2RGB-SigLIP) and what is on it (land cover, terrain, people, climate, crops, mines, the years it burned); and the pictures of a few hundred of them every other year since 2017. It is a client of chloedb-server over HTTP, as the crawler and the spreadsheet app are, and the command center can show it as the app `earth` (`chloedb-server --app earth=URL`). Every request needs the token the command center's app proxy sends, unless the app runs with `--public`: then it serves its page at `/` and the page's reads to anyone, at most `--per-minute` (120) requests a minute from one address, and a picture searched for (`POST /v1/picture`) is the only POST it takes. Readers never send SQL: each page asks its own statements, and shows them under SQL.

Three things on it are ChloeDB's own:

- **Fires like this one.** A fire's first hour, two hours and six hours are each a vector (its power, pixels and spread a quarter hour at a time, then what it started on: slope, trees, shrubs, people, the gusts), and the fires whose first hours were nearest, among those out before it, say how big it got: the median and the middle half of theirs. `backtest` measures how often that range held the size each past fire reached, beside the range of all fires of the region.
- **Climate twins.** Each California place is matched with the place of Chile that looks most like it, in the same Köppen class, and the other way round: the two Mediterranean coasts, about 9,000 km apart, side by side. The match is CSLS (`2·sim − r_a − r_b`, each `r` a place's mean similarity to its ten nearest in the other region), so a place that looks like everything does not become everyone's twin.
- **Changed places.** A place's look each year is a vector; how far it moved from its first year to its last, and in which year most, finds the fields that became houses, the forests that burned and the pits that grew. A search for "shrubland → houses" ranks by `similarity(before, :a) + similarity(after, :b)`.

## The pages

| page | what it shows | from |
|---|---|---|
| Home | a search box for a town, a county or "lat, lon" (and "use my location") that opens a place's report, then each region's fires burning and alerts open, the replays, the largest fires GOES saw, the closest twins, the backtest and how much was read | `ea_fires`, `ea_alerts`, `ea_places`, `ea_backtest`, `ea_scans` |
| Live | the region's map with the fire pixels of the last hours (a red light when the newest scan is under three hours old), the fires burning with their alerts, refreshed every 30 seconds | `ea_detections`, `ea_fires`, `ea_alerts`, `ea_scans` |
| A replay | a night of the archive played back on the map, a scan at a time: Los Angeles on 7 January 2025 (Palisades, Eaton, Hurst) and Valparaíso on 2 February 2024 | `ea_detections`, `ea_scans`, `ea_fires` |
| A fire | its pixels on the map, its first hours drawn, the fires that began like it with the outlook they give, its alerts, and how well the outlook has done before | `ea_fires`, `ea_detections`, `ea_backtest` |
| Past fires | a region's fires by size, year or name | `ea_fires` |
| Explore | the map with a layer over it (people, trees, shrubs, grass, crops, built, bare, water, snow, terrain, slope, mines, fire history, climate, land cover, crops by kind), and the place under a click | `ea_places` |
| A place | **what happened here**: a headline and a timeline within 10 km, newest first (CAL FIRE's perimeters with distance, direction, acres, cause and dates; years satellites saw fire heat; fires GOES saw within 25 km; nearby places that changed), over a map that draws the perimeters; then its picture, what covers it, its terrain and people, the years it burned, the places that look like it, its twins in the other region, places like it with words added or taken away (`/v1/mix`), the fires near it and its pictures year by year | `ea_places`, `ea_years`, `ea_changes`, `ea_fires`, `ea_perimeters`, `ea_burns` |
| Search | places by what they look like, in words ("vineyards on hills", "a lithium mine"): by the picture and what covers it (`rrf(similarity(look, :v), bm25(about, :q))`, the default), by the picture alone, or by the description with names; or by a picture dropped on the page | `ea_places` |
| Climate twins | the closest pairs, those of the same land first, by climate and kind | `ea_places` |
| Changed places | the places that changed most, by what they were and became, or searched by a change in words; a place's years and the places whose change went the same way | `ea_changes`, `ea_years` |
| Rules | each alert rule's statement, how long it took and what it found | `ea_rules`, `ea_alerts` |
| How it's built | the tables, their rows, where each feed stands, the backtest, and every source with its licence | every table |

## The rules

Each rule is one statement over the fires, run every minute; its rows are the alerts, kept open while it finds them. "Now" is the newest scan read for the region, not the clock, so a replay raises the alerts it would have raised then.

| rule | finds |
|---|---|
| `new-fire` | a fire GOES saw start in the last three hours, in two scans or more |
| `people-near` | a fire burning with 10,000 people or more within 5 km of where it started |
| `growing` | a fire past 300 MW and twice its power of an hour ago |
| `big` | a fire past 1,000 MW in a scan |
| `steep-windy` | a fire that started on slopes of 15 degrees or more, half trees and shrubs, with gusts of 35 mph or more |
| `could-grow` | a fire whose twenty nearest fires by their first hours mostly grew past 20 km² |
| `first-in-years` | a fire on ground with no fire in NASA FIRMS's records since 2001 |

## Running it

The places are made once, by `tools/places.py` (Major TOM's fragments and vectors, and the layers read at each place from WorldCover, the Copernicus DEM, GHSL, Köppen-Geiger, the mining polygons, the Cropland Data Layer and FIRMS, a fire year needing three detections), California's fire perimeters by `tools/perimeters.py` (CAL FIRE FRAP's "California Fire Perimeters (all)", public, no warranty from the State: 23,320 fires since 1878 into `ea_perimeters`, and which fire burned how much of which place into `ea_burns`) and `chloedb-earth names`, which gives each GOES fire in California the name of the perimeter it matches (same year, alarm date three days before to two after its start, its centre inside the outline or within 3 km); `serve` does it hourly, and their years by `tools/years.py` (a clear Sentinel-2 scene of each dry season from Element 84's Earth Search, embedded by the server). Both need a server built with `--features vision` and the model `siglip-so400m-patch14-384` (`chloedb-server fetch-model siglip-so400m-patch14-384`, a 3.5 GB download), which also turns a search's words and a dropped picture into vectors.

```bash
for r in california chile; do for s in pull layers fires; do python3 tools/places.py $s --region $r --out DATA; done; done
python3 tools/places.py twins --out DATA                   # both regions' twins
for r in california chile; do python3 tools/places.py rows --region $r --out DATA; done   # ea_places_<region>.jsonl
python3 tools/years.py pick --out DATA && python3 tools/years.py fetch --out DATA
python3 tools/years.py embed --out DATA && python3 tools/years.py rows --out DATA
chloedb-earth load --dir DATA                              # the tables, and their rows
chloedb-earth goes --region california --from 2025-01-07 --to 2025-01-09   # a stretch of GOES's archive
chloedb-earth goes --region chile --live                   # after the newest scan kept, then each new one
chloedb-earth backtest --region california                 # how well the fires like each one did
chloedb-earth serve --public --chips DATA/chips            # the page and its API at 127.0.0.1:7790
```

`goes` reads GOES-West's CONUS product over California (GOES-17 from February 2019, GOES-18 from January 2023) and GOES-East's full disk over Chile (GOES-16, GOES-19 from April 2025), from NOAA's buckets on AWS, keeping only the pixels within 4 km of a place. Low-probability pixels (masks 15 and 35) are left out: most are hot ground and cloud edges with no power worked out, a whole desert valley on a summer night, and beside a real fire they made it look ten times its size. A fire's area is the ground of the pixels GOES saw burning, each 2 to 4 km a side, so a fire spread by embers reads larger than the ground that burned: the Palisades and Eaton fires read 273 and 182 km², inside perimeters of 95 and 57 km², while the York Fire of 2023 read 377 km² and burned about that. A feed started again takes back the fires burning in the 12 hours before where it starts, and marks out those it left burning longer ago. Without `vision` on the server the app still runs, and searches by the description; searches by a picture or by meaning say why they cannot.

## Tests

`cargo test --profile ci` runs the unit tests and `tests/cluster.rs`: made-up places in both regions with their vectors and years, loaded with `load`, a week of made-up GOES scans through the feed (the tracker and writes the archive goes through, seven fires in it, the last inside the Los Angeles replay), then `rules`, `backtest` and every page of `serve`, with the token and `--public`. It needs a chloedb-server binary (`CHLOEDB_SERVER_BIN`, else `../chloedb-server/target/{ci,release}/chloedb-server`) and no model, and is skipped without one unless `CHLOEDB_EARTH_NEED_SERVER` is set, as the Demos workflow does.

## Sizes

Measured in a cloud session's 4-core container on 18 and 19 November 2026 (not a benchmark): 36,612 places in California and 67,052 in Chile, 1.16 GB of JSON lines with their 1152-number vectors, and 1.2 GB on disk with the fires of the seasons read. A place's page asks six statements and answered in 11.8 ms. A day of GOES's archive at a scan every 15 minutes read in 2 to 3 seconds. SigLIP so400m embeds a 384-pixel picture in about 4.2 seconds and a search's words in 0.75 seconds on that container's CPU when nothing else runs, and in 5 to 8 seconds beside the feeds. With California's summers of 2023 to 2025 and its January 2025 read (5,264 fires) and Chile's three summers from 2023 (4,355), the backtest found the middle half of the sizes of the 20 fires most like one after six hours spanning a factor of 1.9 (its top 1.9 times its bottom) against 3.9 for all earlier fires in California, holding the real size 65% of the time against 53%, and 1.7 against 5.1 in Chile, holding it 59% of the time against 65%; their medians ranked the fires by size with a rank correlation of 0.81 and 0.85.
