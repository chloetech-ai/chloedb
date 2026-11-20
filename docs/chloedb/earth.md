# Earth Explorer: California and Chile from orbit, on ChloeDB

`chloedb-earth` is an app of a ChloeDB server built on public pictures and records of the ground: every fire pixel the GOES satellites see over California and Chile, every few minutes, grouped into fires as they come; 103,664 places of 3.84 km a side, each with its Sentinel-2 picture's [SigLIP](https://huggingface.co/google/siglip-so400m-patch14-384) vector from [Major TOM](https://huggingface.co/datasets/Major-TOM/Core-S2RGB-SigLIP) and what is on it (land cover, terrain, people, climate, crops, mines, the years it burned); and the pictures of a few hundred of them every other year since 2017. It is a client of chloedb-server over HTTP, as the crawler and the spreadsheet app are, and the command center can show it as the app `earth` (`chloedb-server --app earth=URL`). Every request needs the token the command center's app proxy sends, unless the app runs with `--public`: then it serves its page at `/` and the page's reads to anyone, at most `--per-minute` (120) requests a minute from one address, and the only POSTs it takes are a picture searched for (`POST /v1/picture`) and a browser's own fire watches. Readers never send SQL: each page asks its own statements, and shows them under SQL.

Three things on it are ChloeDB's own:

- **Fires like this one.** A fire's first hour, two hours and six hours are each a vector (its power, pixels and spread a quarter hour at a time, then what it started on: slope, trees, shrubs, people, the gusts), and the fires whose first hours were nearest, among those out before it, say how big it got: the median and the middle half of theirs. `backtest` measures how often that range held the size each past fire reached, beside the range of all fires of the region.
- **Climate twins.** Each California place is matched with the place of Chile that looks most like it, in the same Köppen class, and the other way round: the two Mediterranean coasts, about 9,000 km apart, side by side. The match is CSLS (`2·sim − r_a − r_b`, each `r` a place's mean similarity to its ten nearest in the other region), so a place that looks like everything does not become everyone's twin.
- **Changed places.** A place's look each year is a vector; how far it moved from its first year to its last, and in which year most, finds the fields that became houses, the forests that burned and the pits that grew. A search for "shrubland → houses" ranks by `similarity(before, :a) + similarity(after, :b)`.

## The pages

| page | what it shows | from |
|---|---|---|
| Home | a search box for a town, a county or "lat, lon" (and "use my location") that opens a place's report, then each region's fires burning and alerts open, the replays, the largest fires GOES saw, the closest twins, the backtest and how much was read | `ea_fires`, `ea_alerts`, `ea_places`, `ea_backtest`, `ea_scans` |
| Live | the region's map with the fire pixels of the last hours (a red light when the newest scan is under three hours old), the fires burning with their alerts, refreshed every 30 seconds | `ea_detections`, `ea_fires`, `ea_alerts`, `ea_scans` |
| A replay | a night of the archive played back on the map, a scan at a time, over GOES's picture of the sky at that moment (with `--frames`): Los Angeles on 7 January 2025 (Palisades, Eaton, Hurst) and Valparaíso on 2 February 2024 | `ea_detections`, `ea_scans`, `ea_fires` |
| Fire watch | places a browser keeps an eye on (a home, a parent's house, a farm: found by name, by "lat, lon", with "use my location", or many at once as lines of a name and coordinates), each with where the nearest fire is now; a place's page with the heat near it, how far the nearest fire was scan by scan over 12 hours, the alerts sent, what to be told about (a fire within 5 to 50 km, 2 km closer in an hour, past 1,000 MW, a clear night over it), the official sources, the ground and the fires that started near it; the browser's notifications while a tab is open and an Atom feed of its alerts; and an example that plays the Park Fire's first 12 hours (24 July 2024) over three made-up places | `ea_watches`, `ea_watch_alerts`, `ea_detections`, `ea_scans`, `ea_fires` |
| Clear sky | the region's map of every place's sky now (clear, partly cloudy, cloudy or smoke, from GOES's last scan), or of its clear nights or days over 30 days; a finder for the clearest places near a town, a point or "use my location", within 5 to 400 km, by the sky now, the most clear nights (for the stars) or days, and what they look like in words ("dark hills", "a lake"); and each place's sky on its page | `ea_sky`, `ea_places` |
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

## Fire watch

A watch is a row of `ea_watches`: a point, a name, the distance it asks about and two choices. There are no accounts: the page makes a long random key (a UUID) the first time, keeps it in the browser, and a watch belongs to the key that made it (25 at most a key, 20,000 in all). Every route takes the key as `?w=`, and the feed has it in its address, `/v1/watches/feed/<key>`, so a feed reader needs nothing else. The page's writes are `POST /v1/watches` (a new watch) and `POST /v1/watch/<id>` (a change, or `{"remove": true}`, which takes its alerts too).

Every minute `serve` reads each region's scans since it last looked (at most the last six hours after a pause) and, for each watch of the region, the nearest fire pixel of the hour up to each scan. It writes to `ea_watch_alerts`, once per watch, fire and kind:

| kind | when |
|---|---|
| `in` | a fire within the distance the watch asks about (5, 10, 15, 25 or 50 km) |
| `near` | a fire within 5 km |
| `here` | a fire pixel within 1.5 km (a GOES pixel is 2 to 3 km across) |
| `power` | a fire within 50 km past 1,000 MW in one scan, when asked |
| `closer` | the same fire 2 km closer than 40 to 90 minutes before, once an hour, when asked |

`/v1/watches` (a key's watches, where each stands, and its alerts), `/v1/watch/<id>` (one, with 12 hours of distances, the heat within 55 km and the fires GOES saw start within 25 km since June 2023), `/v1/watches/feed/<key>` (Atom) and `/v1/watches/example` (the Park Fire's first 12 hours, worked out once) are what the page reads. The browser is told of an alert while Earth is open in a tab; pushes to a closed phone are not built yet.

## Clear sky

`chloedb-earth sky --region R --live` reads GOES's Clear Sky Mask (ABI-L2-ACMC from GOES-18 over California every 5 minutes, ABI-L2-ACMF from GOES-19's full disk over Chile every 10) and its Aerosol Detection's smoke (ADPC and ADPF) for the same scan, at most one scan every `--every` seconds (600). For each place it takes the share of the pixels inside its square that are clear or probably clear, and with smoke, and keeps a row of `ea_sky`: the last scan's shares, the sun's height, and 30 days of how clear each day's daylight scans (sun above 5 degrees) and each night's dark ones (below -12, past nautical dusk) were, the newest last. A row is written again when a share moved a tenth, a day or night moved five points, a day turned, or three hours passed, so most scans write only the places whose sky changed. `--from 720h` fills the 30 days from the archive first.

`serve` reads each region's rows every two minutes into memory: `/v1/sky?region=R` is the map's layer (every place's share clear now and its nights and days), `/v1/sky/find` ranks the places within `km` of `lat, lon` (or the region) by the sky now, by their clear nights (`want=nights`) or days (`want=days`), and with `q` among the 300 places that look most like the words. A fire watch can ask to be told when the night over it is clear: an alert `clear` once a night, when the sun is 12 degrees down, the last scan is 90% clear and the night so far 80%.

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
chloedb-earth sky --region california --from 720h --live   # 30 days of clouds and smoke, then each new scan
chloedb-earth backtest --region california                 # how well the fires like each one did
python3 tools/frames.py --replay la-2025 --sat 18 --product MCMIPC --from 2025-01-07T17:00:00Z --to 2025-01-09T06:00:00Z --box 33.55,34.75,-119.15,-117.55 --out DATA/frames
chloedb-earth serve --public --chips DATA/chips --frames DATA/frames   # the page and its API at 127.0.0.1:7790
```

NASA GIBS keeps GOES's GeoColor pictures for about 90 days, which Live shows under the fires; a replay is older, so `tools/frames.py` draws its own from NOAA's archive of the Cloud and Moisture Imagery product (ABI-L2-MCMIPC over the United States, MCMIPF for the full disk over Chile): the scan nearest every 10 minutes, cut to the replay's box in Web Mercator, true colour by day (the green made from the red, blue and vegetation bands, as ABI has no green band) and the clean infrared band by night, blended by the sun's height, with the fires' own heat (the 3.9 µm band well above the 10.3 µm one) glowing over both, deep red where faint and yellow where hottest. Each scan file is 50 MB (CONUS) or 340 MB (full disk), read and deleted; the pictures are about 60 KB each. The app draws GOES's fire pixels over them as one field of heat per fire, and its "Fire pixels" button hides them to show the fire as the picture has it.

`goes` reads GOES-West's CONUS product over California (GOES-17 from February 2019, GOES-18 from January 2023) and GOES-East's full disk over Chile (GOES-16, GOES-19 from April 2025), from NOAA's buckets on AWS, keeping only the pixels within 4 km of a place. Low-probability pixels (masks 15 and 35) are left out: most are hot ground and cloud edges with no power worked out, a whole desert valley on a summer night, and beside a real fire they made it look ten times its size. A fire's area is the ground of the pixels GOES saw burning, each 2 to 4 km a side, so a fire spread by embers reads larger than the ground that burned: the Palisades and Eaton fires read 273 and 182 km², inside perimeters of 95 and 57 km², while the York Fire of 2023 read 377 km² and burned about that. A feed started again takes back the fires burning in the 12 hours before where it starts, and marks out those it left burning longer ago. Without `vision` on the server the app still runs, and searches by the description; searches by a picture or by meaning say why they cannot.

## Tests

`cargo test --profile ci` runs the unit tests and `tests/cluster.rs`: made-up places in both regions with their vectors and years, loaded with `load`, a week of made-up GOES scans through the feed (the tracker and writes the archive goes through, seven fires in it, the last inside the Los Angeles replay), then `rules`, `backtest`, four places' skies and every page of `serve`, with the token and `--public`. It needs a chloedb-server binary (`CHLOEDB_SERVER_BIN`, else `../chloedb-server/target/{ci,release}/chloedb-server`) and no model, and is skipped without one unless `CHLOEDB_EARTH_NEED_SERVER` is set, as the Demos workflow does.

## Sizes

Measured in a cloud session's 4-core container on 18 and 19 November 2026 (not a benchmark): 36,612 places in California and 67,052 in Chile, 1.16 GB of JSON lines with their 1152-number vectors, and 1.2 GB on disk with the fires of the seasons read. A place's page asks six statements and answered in 11.8 ms. A day of GOES's archive at a scan every 15 minutes read in 2 to 3 seconds. SigLIP so400m embeds a 384-pixel picture in about 4.2 seconds and a search's words in 0.75 seconds on that container's CPU when nothing else runs, and in 5 to 8 seconds beside the feeds. With California's summers of 2023 to 2025 and its January 2025 read (5,264 fires) and Chile's three summers from 2023 (4,355), the backtest found the middle half of the sizes of the 20 fires most like one after six hours spanning a factor of 1.9 (its top 1.9 times its bottom) against 3.9 for all earlier fires in California, holding the real size 65% of the time against 53%, and 1.7 against 5.1 in Chile, holding it 59% of the time against 65%; their medians ranked the fires by size with a rank correlation of 0.81 and 0.85.
