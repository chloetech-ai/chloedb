//! chloedb-earth against a real chloedb-server: made-up places in
//! California and Chile (their vectors given, as tools/places.py gives
//! them) and their years are loaded with `load`; a week of made-up GOES
//! scans goes through the feed (the same tracker and writes as the
//! archive's), seven fires in it, the last one inside the replay of 7
//! January 2025; then `rules` and `backtest` run, and `serve` answers
//! every page the view asks for: the front page, live, the replay, a fire
//! with the fires that began like it, the past fires, a place with its
//! look-alikes, twins and years, the twins, the changes, a layer, the place
//! under a point, a yearly picture, the rules and the data.
//!
//! It needs the chloedb-server binary (`CHLOEDB_SERVER_BIN`, else
//! `../chloedb-server/target/{ci,release}/chloedb-server`), and is skipped
//! without it unless `CHLOEDB_EARTH_NEED_SERVER` is set (as CI does). It
//! needs no model: searches by words go by BM25 alone, and searches by a
//! picture or by meaning (SigLIP, on a server built with `vision`) are
//! not asked.

#![recursion_limit = "256"]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chloedb_earth::feed::{self, Feed};
use chloedb_earth::goes::{self, Pixel, Scan};
use chloedb_earth::places::Places;
use chloedb_earth::strata::Strata;
use serde_json::{json, Value};

const TOKEN: &str = "earth-test-token";
const DIM: usize = 1152;

fn server_bin() -> Option<PathBuf> {
    if let Some(b) = std::env::var_os("CHLOEDB_SERVER_BIN") {
        return Some(PathBuf::from(b));
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../chloedb-server/target");
    ["ci", "release"].iter().map(|p| root.join(p).join("chloedb-server")).find(|p| p.exists())
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

struct Kill(Child);

impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A unit vector near axis `axis`, moved a little along `wiggle`.
fn vector(axis: usize, wiggle: usize, by: f32) -> Vec<f32> {
    let mut v = vec![0f32; DIM];
    v[axis] = 1.0;
    v[wiggle] += by;
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.iter().map(|x| x / n).collect()
}

/// (kind, its axis, words)
const KINDS: &[(&str, usize, &str)] = &[
    ("shrubland", 0, "shrubland and chaparral on hills"),
    ("built", 1, "houses and streets"),
    ("trees", 2, "a forest"),
    ("grassland", 3, "dry grassland"),
    ("crops", 4, "farm fields and orchards"),
];

/// A grid of places: California's around Los Angeles (12 by 8, 0.035
/// degrees apart, so every pixel of a fire falls within 4 km of one) and
/// Chile's around Santiago (4 by 3).
fn places() -> Vec<Value> {
    let mut out = Vec::new();
    for (region, lat0, lon0, rows, cols, climate) in [("california", 33.95, -118.55, 8, 12, "Csa"), ("chile", -33.55, -70.75, 3, 4, "Csb")] {
        for r in 0..rows {
            for c in 0..cols {
                let i = r * cols + c;
                let (kind, axis, words) = KINDS[i % KINDS.len()];
                let id = format!("{}{r:02}{c:02}", if region == "chile" { "cl" } else { "ca" });
                let (lat, lon) = (lat0 + r as f64 * 0.035, lon0 + c as f64 * 0.035);
                let town = if region == "chile" { "Santiago" } else if c < 6 { "Santa Monica" } else { "Pasadena" };
                // California's places are twinned with Chile's of the same kind.
                let twin = if region == "california" { json!(format!("cl{:02}{:02}", (i % KINDS.len()) / 4, (i % KINDS.len()) % 4)) } else { Value::Null };
                out.push(json!({
                    "id": id, "region": region, "country": if region == "chile" { "CL" } else { "US" },
                    "adm1": if region == "chile" { "Región Metropolitana" } else { "California" }, "adm2": if region == "chile" { "Santiago" } else { "Los Angeles" },
                    "town": town, "town_km": 2.0, "town_pop": 90000, "lat": lat, "lon": lon, "cell": id, "taken": "2023-07-01T18:00:00Z",
                    "product": "S2A_TEST", "kind": kind,
                    "trees": if kind == "trees" { 70.0 } else { 5.0 }, "shrubs": if kind == "shrubland" { 70.0 } else { 5.0 },
                    "grass": if kind == "grassland" { 70.0 } else { 5.0 }, "crops": if kind == "crops" { 70.0 } else { 0.0 },
                    "built": if kind == "built" { 80.0 } else { 2.0 }, "bare": 3.0, "snow": 0.0, "water": 0.0, "wetland": 0.0, "mangrove": 0.0, "moss": 0.0,
                    "elev": 200 + 10 * i as i64, "elev_min": 150, "elev_max": 400, "slope": 4.0 + (i % 7) as f64, "steep": 1.0,
                    "people": if kind == "built" { 12000 } else { 40 }, "climate": climate, "climate_group": "temperate",
                    "mine": 0.0, "crop": if kind == "crops" { "Almonds" } else { "" }, "crop_pct": if kind == "crops" { 60.0 } else { 0.0 },
                    "fire_years": if i % 3 == 0 { json!([2018, 2020]) } else { json!([]) }, "fire_px": if i % 3 == 0 { 4 } else { 0 }, "last_fire": if i % 3 == 0 { json!(2020) } else { Value::Null },
                    "words": format!("{words} near {town}"), "about": words,
                    "twin": twin, "twin_sim": if region == "california" { json!(0.8) } else { Value::Null },
                    "twin_score": if region == "california" { json!(0.3 + 0.001 * i as f64) } else { Value::Null },
                    "look": vector(axis, 10 + i % 50, 0.15),
                }));
            }
        }
    }
    out
}

const YEARS: [i64; 3] = [2019, 2022, 2025];

/// Four places' looks a year: two turned from shrubland to houses.
fn years(dir: &Path, chips: &Path) -> (usize, usize) {
    let mut fy = std::fs::File::create(dir.join("ea_years.jsonl")).unwrap();
    let mut fc = std::fs::File::create(dir.join("ea_changes.jsonl")).unwrap();
    let (mut ny, mut nc) = (0, 0);
    for (k, (place, lat, lon, built)) in [("ca0000", 33.95, -118.55, true), ("ca0005", 33.95, -118.375, true), ("ca0102", 33.985, -118.48, false), ("cl0000", -33.55, -70.75, false)].into_iter().enumerate() {
        let region = if place.starts_with("cl") { "chile" } else { "california" };
        let mut looks = Vec::new();
        for (j, y) in YEARS.into_iter().enumerate() {
            let v = if built && j == 2 { vector(1, 60 + k, 0.1) } else { vector(0, 60 + k, 0.1 + 0.01 * j as f32) };
            let label = if built && j == 2 { "suburbs" } else { "shrubland" };
            writeln!(fy, "{}", json!({"id": format!("{place}-{y}"), "place": place, "region": region, "year": y, "taken": format!("{y}-08-01T18:00:00Z"), "scene": "S2_TEST", "cloud": 1.0, "ndvi": null, "label": label, "look": v})).unwrap();
            std::fs::write(chips.join(format!("{place}_{y}.jpg")), [0xFFu8, 0xD8, 0xFF, 0xE0, b'J', b'F', b'I', b'F', 0, k as u8, j as u8]).unwrap();
            looks.push((y, v, label));
            ny += 1;
        }
        let (first, last) = (&looks[0], &looks[2]);
        let delta: Vec<f32> = last.1.iter().zip(&first.1).map(|(a, b)| a - b).collect();
        let n = delta.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
        let drift = 1.0 - first.1.iter().zip(&last.1).map(|(a, b)| a * b).sum::<f32>();
        writeln!(fc, "{}", json!({
            "id": place, "region": region, "lat": lat, "lon": lon, "town": "Somewhere", "adm1": "California", "first": first.0, "last": last.0,
            "drift": drift, "step_year": 2025, "step": drift, "from_label": first.2, "to_label": last.2,
            "arc": if built { "shrubland → suburbs" } else { "shrubland" },
            "before": first.1, "after": last.1, "delta": delta.iter().map(|x| x / n).collect::<Vec<f32>>(),
        }))
        .unwrap();
        nc += 1;
    }
    (ny, nc)
}

fn iso(t: i64) -> String {
    chrono::DateTime::from_timestamp(t, 0).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// The fires of the week: (where in California's grid, its start, hours it
/// burns, how fast it grows). The last starts at 18:00 on 7 January 2025,
/// inside the replay "la-2025".
const FIRES: &[(usize, usize, i64, i64, f64)] = &[
    (1, 1, 1_735_740_000, 8, 1.0),  // 1 Jan 14:00
    (1, 4, 1_735_826_400, 9, 2.0),  // 2 Jan 14:00
    (1, 7, 1_735_912_800, 10, 3.0), // 3 Jan
    (1, 10, 1_735_999_200, 8, 1.5), // 4 Jan
    (5, 1, 1_736_085_600, 9, 2.5),  // 5 Jan
    (5, 4, 1_736_172_000, 11, 1.0), // 6 Jan
    (5, 7, 1_736_272_800, 10, 2.0), // 7 Jan 18:00
];

/// One scan's pixels: each fire burning then, more pixels as it grows (up
/// to a 3 by 3 block 0.02 degrees apart).
fn pixels_at(ts: i64) -> Vec<Pixel> {
    let mut out = Vec::new();
    for (i, &(r, c, start, hours, grow)) in FIRES.iter().enumerate() {
        if ts < start || ts > start + hours * 3600 {
            continue;
        }
        let k = ((ts - start) / 900) as f64;
        let n = ((1.0 + k * grow / 4.0) as usize).min(9);
        let (lat, lon) = (33.95 + r as f64 * 0.035, -118.55 + c as f64 * 0.035);
        for p in 0..n {
            let (dr, dc) = ((p / 3) as i32 - 1, (p % 3) as i32 - 1);
            out.push(Pixel {
                row: 1000 + 10 * i as u32 + (dr + 1) as u32,
                col: 2000 + (dc + 1) as u32,
                lat: lat + dr as f64 * 0.02,
                lon: lon + dc as f64 * 0.02,
                mask: 10,
                mw: Some(15.0 * grow + 4.0 * k),
                area: Some(1500.0),
                temp: Some(650.0),
            });
        }
    }
    out
}

async fn get(http: &reqwest::Client, url: &str, token: Option<&str>) -> (u16, Value) {
    let mut r = http.get(url);
    if let Some(t) = token {
        r = r.bearer_auth(t);
    }
    let resp = r.send().await.unwrap_or_else(|e| panic!("{url}: {e}"));
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap();
    (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

async fn ok(http: &reqwest::Client, base: &str, path: &str) -> Value {
    let (s, v) = get(http, &format!("{base}{path}"), Some(TOKEN)).await;
    assert_eq!(s, 200, "{path}: {v}");
    v
}

fn run(args: &[&str], strata: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_chloedb-earth")).args(args).args(["--strata", strata]).output().unwrap();
    assert!(out.status.success(), "chloedb-earth {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_page_against_a_real_server() {
    let Some(bin) = server_bin() else {
        assert!(std::env::var_os("CHLOEDB_EARTH_NEED_SERVER").is_none(), "CHLOEDB_EARTH_NEED_SERVER is set and there is no chloedb-server");
        eprintln!("skipped: no chloedb-server (set CHLOEDB_SERVER_BIN)");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let (rows, chips) = (tmp.path().join("rows"), tmp.path().join("chips"));
    std::fs::create_dir_all(&rows).unwrap();
    std::fs::create_dir_all(&chips).unwrap();
    let port = free_port();
    let strata = format!("http://127.0.0.1:{port}");
    let _server = Kill(
        Command::new(&bin)
            .arg("--db")
            .arg(tmp.path().join("earth.db"))
            .arg("--models")
            .arg(tmp.path().join("models"))
            .args(["--no-fetch-models", "--grpc", "off", "--listen", &format!("127.0.0.1:{port}")])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("chloedb-server"),
    );
    let http = reqwest::Client::builder().timeout(Duration::from_secs(60)).build().unwrap();
    let t0 = Instant::now();
    while http.get(format!("{strata}/v1/health")).send().await.map(|r| r.status().is_success()).unwrap_or(false) == false {
        assert!(t0.elapsed() < Duration::from_secs(60), "chloedb-server did not start");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // The places and their years, loaded as the tools write them.
    let all = places();
    let mut f = std::fs::File::create(rows.join("ea_places.jsonl")).unwrap();
    for p in &all {
        writeln!(f, "{p}").unwrap();
    }
    drop(f);
    let (ny, nc) = years(&rows, &chips);
    // One CAL FIRE perimeter around the first fire of the week, and the
    // place it burned half of.
    // The outline has 4,000 points (about 80 KB, as the largest of CAL
    // FIRE's do): a long KEYWORD the server must take.
    let ring: Vec<Value> = (0..=4000).map(|i| {
        let a = i as f64 / 4000.0 * std::f64::consts::TAU;
        json!([-118.5 + 0.1 * a.cos(), 34.0 + 0.1 * a.sin()])
    }).collect();
    let shape = json!({"type": "Polygon", "coordinates": [ring]}).to_string();
    assert!(shape.len() > 70_000);
    std::fs::write(rows.join("ea_perimeters.jsonl"), format!("{}\n", json!({"id": "cf-2025-test", "source": "calfire", "name": "Testwood", "complex": "", "year": 2025,
        "alarm": "2025-01-01T00:00:00Z", "contained": "2025-01-04T00:00:00Z", "acres": 1000.0, "km2": 4.05, "cause": "Lightning", "agency": "CDF", "unit": "LAC", "inc": "",
        "lat": 34.0, "lon": -118.5, "s": 33.9, "n": 34.1, "w": -118.6, "e": -118.4, "shape": shape.clone()}))).unwrap();
    std::fs::write(rows.join("ea_burns.jsonl"), format!("{}\n", json!({"id": "ca0000:cf-2025-test", "place": "ca0000", "perimeter": "cf-2025-test", "region": "california",
        "name": "Testwood", "year": 2025, "alarm": "2025-01-01T00:00:00Z", "acres": 1000.0, "share": 0.5, "cause": "Lightning"}))).unwrap();
    run(&["load", "--dir", rows.to_str().unwrap()], &strata);

    // A week of GOES scans every 15 minutes, through the feed.
    let db = Strata::new(&strata, None);
    let places = Arc::new(Places::read(&db).await.unwrap());
    assert_eq!(places.len(), all.len(), "every place read back, past a page of the scroll or not");
    let region = feed::region("california").unwrap();
    let (from, to) = (1_735_689_600, 1_736_380_800); // 1 Jan 2025 00:00 to 9 Jan 00:00
    let mut fd = Feed::new(db.clone(), places.clone(), region, from).await.unwrap();
    let mut ts = from;
    while ts <= to {
        let scan = Scan { start: ts, key: format!("OR_ABI-L2-FDCC-M6_G18_s{ts}.nc") };
        fd.pixels(goes::G18_CONUS, scan, pixels_at(ts)).await.unwrap();
        ts += 900;
    }
    fd.flush(true).await.unwrap();
    let fires = db.query("SELECT id, start, state, seen_start, scans, hours, area_km2, town FROM ea_fires ORDER BY start", json!({})).await.unwrap();
    assert_eq!(fires.len(), FIRES.len(), "one fire each: {fires:?}");
    for (r, (_, _, start, hours, _)) in fires.iter().zip(FIRES) {
        assert_eq!(r["start"], json!(iso(*start)), "{r:?}");
        assert_eq!(r["state"], "out", "{r:?}");
        assert_eq!(r["seen_start"], true, "{r:?}");
        assert!((r["hours"].as_f64().unwrap() - *hours as f64).abs() < 0.3, "{r:?}");
        assert!(r["area_km2"].as_f64().unwrap() > 0.0, "{r:?}");
        assert!(["Santa Monica", "Pasadena"].contains(&r["town"].as_str().unwrap()), "{r:?}");
    }
    let last = fires.last().unwrap()["id"].as_str().unwrap().to_string();
    let scans = db.query("SELECT COUNT(*) AS n FROM ea_scans", json!({})).await.unwrap();
    assert_eq!(scans[0]["n"], json!((to - from) / 900 + 1));

    // The first fire takes the perimeter's name.
    run(&["names"], &strata);
    let named = db.query("SELECT name, perimeter FROM ea_fires ORDER BY start LIMIT 1", json!({})).await.unwrap();
    assert_eq!(named[0]["name"], "Testwood", "{named:?}");
    assert_eq!(named[0]["perimeter"], "cf-2025-test");

    // The rules, and the backtest of fires like each one.
    let r: Value = serde_json::from_str(&run(&["rules"], &strata)).unwrap();
    assert!(r.is_object(), "{r}");
    let b: Value = serde_json::from_str(&run(&["backtest", "--region", "california", "--limit", "50"], &strata)).unwrap();
    assert!(b.is_object() || b.is_array(), "{b}");

    // The app.
    let app_port = free_port();
    let base = format!("http://127.0.0.1:{app_port}");
    let _app = Kill(
        Command::new(env!("CARGO_BIN_EXE_chloedb-earth"))
            .args(["serve", "--listen", &format!("127.0.0.1:{app_port}"), "--token", TOKEN, "--strata", &strata, "--no-rules", "--chips"])
            .arg(&chips)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("chloedb-earth serve"),
    );
    let t0 = Instant::now();
    while http.get(format!("{base}/healthz")).send().await.map(|r| r.status().is_success()).unwrap_or(false) == false {
        assert!(t0.elapsed() < Duration::from_secs(30), "chloedb-earth serve did not start");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // Its places are read in the background: wait for the point lookup.
    let t0 = Instant::now();
    loop {
        let (s, v) = get(&http, &format!("{base}/v1/at?lat=33.95&lon=-118.55"), Some(TOKEN)).await;
        if s == 200 && v["id"] == "ca0000" {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(30), "/v1/at: {s} {v}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // Without the token: refused.
    let (s, _) = get(&http, &format!("{base}/v1/home"), None).await;
    assert_eq!(s, 401);

    // With a token and not public, the page is the command center's.
    let (s, _) = get(&http, &format!("{base}/"), Some(TOKEN)).await;
    assert_eq!(s, 404);
    let m = ok(&http, &base, "/app.json").await;
    assert_eq!(m["maturity"], "demo");
    let (s, view) = get(&http, &format!("{base}/ui/view.js"), Some(TOKEN)).await;
    assert_eq!(s, 200);
    assert!(view.as_str().unwrap().contains("export"));

    let home = ok(&http, &base, "/v1/home").await;
    assert!(home.to_string().contains("ca0"), "the front page's twins: {home}");
    ok(&http, &base, "/v1/live?region=california").await;
    ok(&http, &base, "/v1/live?region=chile").await;

    // The replay: the last fire's pixels, in time.
    let rp = ok(&http, &base, "/v1/replay/la-2025").await;
    let px = rp["pixels"].as_array().unwrap_or_else(|| panic!("{}", rp["queries"]));
    assert!(!px.is_empty(), "the replay has the 7 January fire: {}", rp["queries"]);

    // The last fire: the six before it were out when it was at its age,
    // and began much like it.
    let fire = ok(&http, &base, &format!("/v1/fire/{last}")).await;
    assert_eq!(fire["fire"]["id"], json!(last));
    assert!(!fire["pixels"].as_array().unwrap().is_empty(), "{}", fire["fire"]);
    let similar = fire["similar"].as_array().unwrap();
    assert_eq!(similar.len(), FIRES.len() - 1, "the fires that began like it: {}", fire["queries"]);
    assert!(fire["outlook"]["median"].as_f64().is_some(), "its outlook: {}", fire["outlook"]);
    let past = ok(&http, &base, "/v1/fires?region=california&year=2025").await;
    assert_eq!(past["fires"].as_array().unwrap().len(), FIRES.len(), "{past}");

    // A place: its look-alikes, its twins across the equator, its years.
    let place = ok(&http, &base, "/v1/place/ca0000").await;
    assert_eq!(place["place"]["id"], "ca0000");
    let like = place["like"].as_array().unwrap();
    assert!(!like.is_empty() && like.iter().all(|p| p["region"] == "california" && p["kind"] == "shrubland"), "look-alikes are its own kind: {like:?}");
    assert!(place["twins"].as_array().unwrap().iter().all(|p| p["region"] == "chile"), "{}", place["twins"]);
    assert_eq!(place["years"].as_array().unwrap().len(), YEARS.len(), "{}", place["years"]);
    assert_eq!(place["change"]["to_label"], "suburbs");
    let story = &place["story"];
    assert!(story["headline"].as_str().unwrap().contains("Testwood"), "{story}");
    assert!(story["events"].as_array().unwrap().iter().any(|e| e["kind"] == "perimeter" && e["burned"].as_f64() == Some(0.5)), "{story}");
    assert_eq!(story["outlines"].as_array().unwrap().len(), 1, "{story}");
    assert_eq!(story["outlines"][0]["shape"].as_str().unwrap().len(), shape.len(), "the outline comes back whole");
    let found = ok(&http, &base, "/v1/find?q=pasa").await;
    assert!(found["found"].as_array().unwrap().iter().all(|f| f["label"].as_str().unwrap().starts_with("Pasadena")) && !found["found"].as_array().unwrap().is_empty(), "{found}");
    let found = ok(&http, &base, "/v1/find?q=33.95,-118.55").await;
    assert_eq!(found["found"][0]["id"], "ca0000", "{found}");

    let twins = ok(&http, &base, "/v1/twins?from=california").await;
    let pairs = twins["pairs"].as_array().unwrap();
    assert!(!pairs.is_empty(), "{twins}");
    assert_eq!(pairs[0]["same_kind"], true, "pairs of the same land first: {}", pairs[0]);
    assert_eq!(pairs[0]["b"]["region"], "chile");

    // Changed places: the most changed first, then one place's path.
    let ch = ok(&http, &base, "/v1/changed").await;
    let cps = ch["places"].as_array().unwrap();
    assert_eq!(cps.len(), nc, "{ch}");
    assert_eq!(cps[0]["to_label"], "suburbs", "{ch}");
    ok(&http, &base, "/v1/path/ca0000").await;
    assert_eq!(ny, nc * YEARS.len());

    // Search by words alone (no model), and the place under a point.
    let s = ok(&http, &base, "/v1/search?q=forest&mode=words&region=california").await;
    let found = s["places"].as_array().unwrap();
    assert!(!found.is_empty() && found.iter().all(|p| p["kind"] == "trees"), "{s}");
    let (st, _) = get(&http, &format!("{base}/v1/search?q=&mode=words"), Some(TOKEN)).await;
    assert_eq!(st, 400);

    // A layer, the place under a point at sea, a yearly picture.
    let layer = ok(&http, &base, "/v1/layer/people?region=california").await;
    assert_eq!(layer["v"].as_array().unwrap().len(), 96, "{layer}");
    let (st, _) = get(&http, &format!("{base}/v1/layer/nothing?region=california"), Some(TOKEN)).await;
    assert_eq!(st, 400);
    let sea = ok(&http, &base, "/v1/at?lat=30.0&lon=-125.0").await;
    assert_eq!(sea["id"], Value::Null, "{sea}");
    let r = http.get(format!("{base}/v1/chip/ca0000/2025")).bearer_auth(TOKEN).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    assert_eq!(r.headers()["content-type"], "image/jpeg");
    assert_eq!(&r.bytes().await.unwrap()[..3], &[0xFF, 0xD8, 0xFF]);
    let b64 = ok(&http, &base, "/v1/chip/ca0000/2025?b64=1").await;
    assert_eq!(b64["mime"], "image/jpeg");
    let (st, _) = get(&http, &format!("{base}/v1/chip/ca0000/2018"), Some(TOKEN)).await;
    assert_eq!(st, 404);
    let (st, _) = get(&http, &format!("{base}/v1/chip/..%2Fetc/2025"), Some(TOKEN)).await;
    assert!(st == 400 || st == 404, "{st}");

    // Public, as at earth.chloetech.ai: the page and its reads for anyone.
    let pub_port = free_port();
    let open = format!("http://127.0.0.1:{pub_port}");
    let _public = Kill(
        Command::new(env!("CARGO_BIN_EXE_chloedb-earth"))
            .args(["serve", "--listen", &format!("127.0.0.1:{pub_port}"), "--token", TOKEN, "--public", "--strata", &strata, "--no-rules"])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("chloedb-earth serve --public"),
    );
    let t0 = Instant::now();
    while http.get(format!("{open}/healthz")).send().await.map(|r| r.status().is_success()).unwrap_or(false) == false {
        assert!(t0.elapsed() < Duration::from_secs(30), "chloedb-earth serve --public did not start");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let (s, page) = get(&http, &format!("{open}/"), None).await;
    assert_eq!(s, 200);
    assert!(page.as_str().unwrap().contains("Earth"));
    let (s, _) = get(&http, &format!("{open}/v1/home"), None).await;
    assert_eq!(s, 200);
    let s = http.post(format!("{open}/v1/mix")).send().await.unwrap().status().as_u16();
    assert!(s == 401 || s == 405, "{s}");

    // The rules and the data.
    let rules = ok(&http, &base, "/v1/rules").await;
    assert!(!rules["rules"].as_array().unwrap().is_empty(), "{rules}");
    let data = ok(&http, &base, "/v1/data").await;
    let tables = data["tables"].to_string();
    assert!(tables.contains("ea_places") && tables.contains("ea_fires"), "{data}");
}
