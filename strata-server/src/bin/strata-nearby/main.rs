//! strata-nearby: a phone that searches for places near its user on the
//! device, and asks the cloud only when it cannot prove its answer.
//!
//! It stands in for an app that embeds Strata ([`strata::slice::Device`]):
//! it reads a table's slice from a strata-server (`GET
//! /v1/slices/<table>/manifest.json`, built by `POST /v1/admin/slices`),
//! fetches the tiles around its user as the user moves, and keeps them in
//! a local database. A search runs there with the cloud's statistics; when
//! the k-th local score reaches the bound on any place it does not hold,
//! the answer is the cloud's and no request leaves the device. Otherwise
//! it sends the same query to the cloud's `POST /v1/query`.
//!
//! Before it trusts its slice it asks the cloud whether it is the current
//! one (`GET /v1/slices/<table>/status`, every 30 s): it fetches a newer
//! one, and while the table has changed since the build it asks the cloud.
//! It reports its searches to `POST /v1/slices/<table>/usage`, which the
//! command center's Slices view shows.
//!
//! Its page (at `--listen`) shows the city's tiles, the ones held, the
//! user (drag or click to move), and for each search where it was answered
//! and why. `--check` (or the page's box) runs every search in the cloud
//! too and says whether the answers match.
//!
//! ```text
//! strata-nearby --cloud http://127.0.0.1:7700 --table places --listen 127.0.0.1:7740
//! ```

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json as AxJson, Router};
use clap::Parser;
use serde_json::{json, Value as Json};
use strata::query::sql::Params;
use strata::slice::{Device, Manifest, Search, SliceQuery, Why};
use strata::Value;

const PAGE: &str = include_str!("page.html");

#[derive(Parser)]
#[command(name = "strata-nearby", about = "A phone that searches near its user on the device, with the cloud as fallback")]
struct Args {
    /// The cloud: a strata-server that serves the table's slice.
    #[arg(long, default_value = "http://127.0.0.1:7700")]
    cloud: String,
    /// The table whose slice to read.
    #[arg(long, default_value = "places")]
    table: String,
    /// Where the page listens.
    #[arg(long, default_value = "127.0.0.1:7740")]
    listen: SocketAddr,
    /// The device's database directory (a temporary one by default).
    #[arg(long)]
    dir: Option<PathBuf>,
    /// The square of tiles held around the user's tile: 2 * reach + 1
    /// tiles on a side.
    #[arg(long, default_value_t = 2)]
    reach: i32,
    /// Where the user starts (the grid's origin by default).
    #[arg(long, allow_hyphen_values = true)]
    lat: Option<f64>,
    #[arg(long, allow_hyphen_values = true)]
    lon: Option<f64>,
    /// Run every search in the cloud too and compare.
    #[arg(long)]
    check: bool,
    /// A key for the cloud, sent as a bearer token.
    #[arg(long, env = "STRATA_KEY")]
    key: Option<String>,
    /// The name it reports its searches under.
    #[arg(long)]
    device: Option<String>,
}

/// Searches it answered since its last report to the cloud.
#[derive(Default)]
struct Pending {
    searches: u64,
    why: [u64; 5],
    device_ms: f64,
    cloud_ms: f64,
}

impl Pending {
    fn body(&self, device: &str) -> Json {
        let why: serde_json::Map<String, Json> = Why::ALL.iter().zip(self.why).map(|(w, n)| (w.as_str().to_string(), json!(n))).collect();
        json!({"device": device, "searches": self.searches, "why": why, "device_ms": self.device_ms, "cloud_ms": self.cloud_ms})
    }
}

/// How often it asks whether its slice is the current one, and reports.
const STATUS_EVERY: Duration = Duration::from_secs(30);
const REPORT_EVERY: u64 = 5;

/// What the device holds and has done.
struct Phone {
    dev: Device,
    lat: f64,
    lon: f64,
    reach: i32,
    /// Bytes and tiles fetched from the cloud since the start.
    fetched_bytes: u64,
    fetched_tiles: u64,
    searches: u64,
    on_device: u64,
    /// Searches whose cloud check differed (never, for a proved answer).
    mismatches: u64,
    pending: Pending,
    status_at: Option<Instant>,
    device: String,
}

#[derive(Clone)]
struct App {
    phone: Arc<Mutex<Phone>>,
    cloud: Cloud,
    check: bool,
    dir: PathBuf,
}

#[derive(Clone)]
struct Cloud {
    url: String,
    table: String,
    key: Option<String>,
    http: reqwest::Client,
}

impl Cloud {
    fn req(&self, r: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.key {
            Some(k) => r.bearer_auth(k),
            None => r,
        }
    }

    /// Reports searches (`POST /v1/slices/<table>/usage`).
    async fn report(&self, body: Json) -> Result<(), String> {
        let url = format!("{}/v1/slices/{}/usage", self.url, self.table);
        let r = self.req(self.http.post(&url)).json(&body).send().await.map_err(|e| format!("POST {url}: {e}"))?;
        if !r.status().is_success() {
            return Err(format!("POST {url}: {}", r.status()));
        }
        Ok(())
    }

    /// The cloud's word on its slice: `{version, fresh, ...}`.
    async fn status(&self) -> Result<Json, String> {
        let bytes = self.bytes("status").await?;
        serde_json::from_slice(&bytes).map_err(|e| format!("the slice's status: {e}"))
    }

    async fn bytes(&self, path: &str) -> Result<Vec<u8>, String> {
        let url = format!("{}/v1/slices/{}/{path}", self.url, self.table);
        let r = self.req(self.http.get(&url)).send().await.map_err(|e| format!("GET {url}: {e}"))?;
        let status = r.status();
        let body = r.bytes().await.map_err(|e| format!("GET {url}: {e}"))?;
        if !status.is_success() {
            return Err(format!("GET {url}: {status}: {}", String::from_utf8_lossy(&body)));
        }
        Ok(body.to_vec())
    }

    /// Runs a query in the cloud: its rows as objects, and the scores.
    async fn query(&self, sql: &str, params: &Params) -> Result<(Vec<Json>, Vec<f64>), String> {
        let p: serde_json::Map<String, Json> = params.0.iter().map(|(k, v)| (k.clone(), value_json(v))).collect();
        let r = self.req(self.http.post(format!("{}/v1/query", self.url))).json(&json!({"sql": sql, "params": p})).send().await.map_err(|e| format!("the cloud: {e}"))?;
        let status = r.status();
        let j: Json = r.json().await.map_err(|e| format!("the cloud's answer: {e}"))?;
        if !status.is_success() {
            return Err(format!("the cloud: {status}: {}", j.get("error").unwrap_or(&j)));
        }
        let cols: Vec<String> = j["columns"].as_array().map(|c| c.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
        let rows = j["rows"].as_array().cloned().unwrap_or_default();
        let objs = rows.iter().map(|r| Json::Object(cols.iter().cloned().zip(r.as_array().cloned().unwrap_or_default()).collect())).collect();
        let scores = j["scores"].as_array().map(|s| s.iter().filter_map(|x| x.as_f64()).collect()).unwrap_or_default();
        Ok((objs, scores))
    }
}

fn value_json(v: &Value) -> Json {
    match v {
        Value::Int(i) | Value::Timestamp(i) => json!(i),
        Value::Float(x) => json!(x),
        Value::Str(s) => json!(s),
        Value::Bool(b) => json!(b),
        _ => Json::Null,
    }
}

struct Failure(StatusCode, String);

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        (self.0, AxJson(json!({"error": self.1}))).into_response()
    }
}

fn fail(e: impl std::fmt::Display) -> Failure {
    Failure(StatusCode::BAD_REQUEST, e.to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = Args::parse();
    let cloud = Cloud { url: a.cloud.trim_end_matches('/').to_string(), table: a.table.clone(), key: a.key.clone(), http: strata_server::http_client().build()? };
    let text = String::from_utf8(cloud.bytes("manifest.json").await?)?;
    let m = Manifest::from_json(&text)?;
    let geo = m.geo().map_err(|_| "strata-nearby shows a geo slice; this one is of vectors")?.clone();
    let tmp = tempfile::tempdir()?;
    let dir = a.dir.clone().unwrap_or_else(|| tmp.path().to_path_buf());
    let (lat, lon) = (a.lat.unwrap_or(geo.grid.lat0), a.lon.unwrap_or(geo.grid.lon0));
    eprintln!(
        "slice of {} (version {}): {} tiles, {} rows, {} words; manifest {:.1} MB",
        m.schema.table,
        m.version,
        m.tiles.len(),
        m.tiles.iter().map(|t| t.rows).sum::<u64>(),
        geo.words.terms.len(),
        text.len() as f64 / 1e6
    );
    let dev = Device::create(&dir, m)?;
    let device = a.device.clone().unwrap_or_else(|| format!("nearby-{}", std::process::id()));
    let phone = Phone { dev, lat, lon, reach: a.reach, fetched_bytes: text.len() as u64, fetched_tiles: 0, searches: 0, on_device: 0, mismatches: 0, pending: Pending::default(), status_at: None, device };
    let app = App { phone: Arc::new(Mutex::new(phone)), cloud, check: a.check, dir: dir.clone() };
    move_to(&app, lat, lon, a.reach).await.map_err(|f| f.1)?;
    let router = Router::new()
        .route("/", get(|| async { Html(PAGE) }))
        .route("/api/state", get(state))
        .route("/api/places", get(places))
        .route("/api/move", post(moved))
        .route("/api/search", post(search))
        .with_state(app);
    let listener = tokio::net::TcpListener::bind(a.listen).await?;
    eprintln!("open http://{}/", a.listen);
    axum::serve(listener, router).await?;
    Ok(())
}

/// Moves the user: fetches the tiles of the square around it that the
/// device lacks, and drops those far behind.
async fn move_to(app: &App, lat: f64, lon: f64, reach: i32) -> Result<Json, Failure> {
    let plan = {
        let mut p = app.phone.lock().unwrap();
        p.lat = lat;
        p.lon = lon;
        p.reach = reach;
        p.dev.plan(lat, lon, reach)
    };
    let t0 = Instant::now();
    let mut got = Vec::new();
    for (t, _) in &plan.fetch {
        got.push((*t, app.cloud.bytes(&t.path()).await.map_err(fail)?));
    }
    let fetch_ms = t0.elapsed().as_secs_f64() * 1e3;
    let phone = app.phone.clone();
    let drop = plan.drop.clone();
    tokio::task::spawn_blocking(move || {
        let mut p = phone.lock().unwrap();
        for t in drop {
            p.dev.drop_tile(t)?;
        }
        for (t, bytes) in got {
            p.fetched_bytes += bytes.len() as u64;
            p.fetched_tiles += 1;
            p.dev.add_tile(t, &bytes)?;
        }
        Ok::<_, strata::Error>(())
    })
    .await
    .map_err(fail)?
    .map_err(fail)?;
    let mut s = state_of(&app.phone.lock().unwrap());
    s["moved"] = json!({"fetched": plan.fetch.len(), "dropped": plan.drop.len(), "bytes": plan.fetch.iter().map(|f| f.1).sum::<u64>(), "fetch_ms": fetch_ms});
    Ok(s)
}

fn state_of(p: &Phone) -> Json {
    let m = p.dev.manifest();
    let g = m.geo().expect("a geo slice");
    let held: Vec<Json> = p.dev.held().map(|(t, n)| json!([t.x, t.y, n])).collect();
    json!({
        "table": m.schema.table, "version": m.version, "grid": g.grid, "kx": g.grid.kx(), "ky": strata::slice::KY, "lat_field": g.lat, "lon_field": g.lon,
        "tiles": m.tiles.iter().map(|t| json!([t.x, t.y, t.rows, t.bytes])).collect::<Vec<_>>(),
        "held": held, "user": {"lat": p.lat, "lon": p.lon}, "reach": p.reach,
        "held_rows": p.dev.held().map(|(_, n)| n as u64).sum::<u64>(),
        "fetched_bytes": p.fetched_bytes, "fetched_tiles": p.fetched_tiles,
        "searches": p.searches, "on_device": p.on_device, "mismatches": p.mismatches,
        "edge_m": finite(p.dev.edge(p.lat, p.lon)),
    })
}

fn finite(x: f64) -> Json {
    if x.is_finite() {
        json!(x)
    } else {
        Json::Null
    }
}

async fn state(State(app): State<App>) -> AxJson<Json> {
    AxJson(state_of(&app.phone.lock().unwrap()))
}

/// The positions of the places held, for the map's dots.
async fn places(State(app): State<App>) -> Result<AxJson<Json>, Failure> {
    let p = app.phone.lock().unwrap();
    let m = p.dev.manifest();
    let g = m.geo().map_err(fail)?;
    let sql = format!("SELECT {}, {} FROM {} LIMIT 200000", g.lat, g.lon, m.schema.table);
    let rs = p.dev.db().query(&sql, &Params::new()).map_err(fail)?;
    let pts: Vec<Json> = rs.rows.iter().map(|r| json!([r[0].as_f64(), r[1].as_f64()])).collect();
    Ok(AxJson(json!({"places": pts})))
}

async fn moved(State(app): State<App>, AxJson(b): AxJson<Json>) -> Result<AxJson<Json>, Failure> {
    let f = |k: &str| b.get(k).and_then(|v| v.as_f64()).ok_or_else(|| fail(format!("the body needs a number {k:?}")));
    let reach = b.get("reach").and_then(|v| v.as_i64()).map_or_else(|| app.phone.lock().unwrap().reach, |r| r.clamp(0, 6) as i32);
    move_to(&app, f("lat")?, f("lon")?, reach).await.map(AxJson)
}

/// Asks the cloud whether the slice is the current one, when it last
/// asked over [`STATUS_EVERY`] ago: fetches a newer slice (and the tiles
/// around the user again), and marks the device stale while the table has
/// changed since the build.
async fn check_status(app: &App) -> Result<(), Failure> {
    let due = app.phone.lock().unwrap().status_at.is_none_or(|t| t.elapsed() >= STATUS_EVERY);
    if !due {
        return Ok(());
    }
    let st = app.cloud.status().await.map_err(fail)?;
    let (version, lat, lon, reach) = {
        let mut p = app.phone.lock().unwrap();
        p.status_at = Some(Instant::now());
        (p.dev.manifest().version, p.lat, p.lon, p.reach)
    };
    if st["version"].as_u64() != Some(version) {
        let text = String::from_utf8(app.cloud.bytes("manifest.json").await.map_err(fail)?).map_err(fail)?;
        let m = Manifest::from_json(&text).map_err(fail)?;
        let dev = Device::create(&app.dir, m).map_err(fail)?;
        {
            let mut p = app.phone.lock().unwrap();
            p.fetched_bytes += text.len() as u64;
            p.dev = dev;
        }
        move_to(app, lat, lon, reach).await?;
    }
    app.phone.lock().unwrap().dev.set_stale(st["fresh"] == json!(false));
    Ok(())
}

/// `{q, k?, d0_m?, check?}`: answered on the device when proved, else by
/// the cloud.
async fn search(State(app): State<App>, AxJson(b): AxJson<Json>) -> Result<AxJson<Json>, Failure> {
    check_status(&app).await?;
    let q = b.get("q").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let k = b.get("k").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
    let d0 = b.get("d0_m").and_then(|v| v.as_f64()).unwrap_or(1000.0);
    let check = b.get("check").and_then(|v| v.as_bool()).unwrap_or(app.check);
    let (s, m, a, device_ms) = {
        let p = app.phone.lock().unwrap();
        let s = Search::new(&q, p.lat, p.lon, k, d0).map_err(fail)?;
        let t0 = Instant::now();
        let a = p.dev.search(&s).map_err(fail)?;
        (s, p.dev.manifest().clone(), a, t0.elapsed().as_secs_f64() * 1e3)
    };
    let local: Vec<Json> = a.rows.rows.iter().map(|r| Json::Object(a.rows.columns.iter().cloned().zip(r.iter().map(value_json)).collect())).collect();
    let (rows, scores, cloud_ms) = if a.on_device {
        (local.clone(), a.rows.scores.clone(), None)
    } else {
        let t0 = Instant::now();
        let (rows, scores) = app.cloud.query(&s.sql(&m), &s.params(&m)).await.map_err(fail)?;
        (rows, scores, Some(t0.elapsed().as_secs_f64() * 1e3))
    };
    let checked = if check && a.on_device {
        let t0 = Instant::now();
        let (_, cs) = app.cloud.query(&s.sql(&m), &s.params(&m)).await.map_err(fail)?;
        Some(json!({"same": cs == a.rows.scores, "cloud_ms": t0.elapsed().as_secs_f64() * 1e3}))
    } else {
        None
    };
    let report = {
        let mut p = app.phone.lock().unwrap();
        p.searches += 1;
        p.on_device += a.on_device as u64;
        if checked.as_ref().is_some_and(|c| c["same"] == json!(false)) {
            p.mismatches += 1;
        }
        let w = Why::ALL.iter().position(|w| *w == a.why).expect("a reason");
        p.pending.searches += 1;
        p.pending.why[w] += 1;
        p.pending.device_ms += device_ms;
        p.pending.cloud_ms += cloud_ms.unwrap_or(0.0);
        (p.pending.searches >= REPORT_EVERY).then(|| {
            let body = p.pending.body(&p.device);
            p.pending = Pending::default();
            body
        })
    };
    if let Some(body) = report {
        let cloud = app.cloud.clone();
        tokio::spawn(async move {
            if let Err(e) = cloud.report(body).await {
                eprintln!("{e}");
            }
        });
    }
    let geo = m.geo().map_err(fail)?;
    let g = geo.grid;
    let (ux, uy) = g.xy(s.lat, s.lon);
    let out: Vec<Json> = rows
        .into_iter()
        .zip(&scores)
        .map(|(mut r, sc)| {
            let (la, lo) = (r[&geo.lat].as_f64().unwrap_or(f64::NAN), r[&geo.lon].as_f64().unwrap_or(f64::NAN));
            let (x, y) = g.xy(la, lo);
            r["_score"] = json!(sc);
            r["_m"] = json!((x - ux).hypot(y - uy));
            r
        })
        .collect();
    Ok(AxJson(json!({
        "q": q, "k": k, "d0_m": d0, "proved": a.proved, "on_device": a.on_device, "why": a.why, "stale": a.stale, "where": if a.on_device { "device" } else { "cloud" },
        "rows": out, "local_kth": a.rows.scores.get(k - 1), "local_rows": a.rows.scores.len(),
        "bound": a.bound, "text_bound": a.text_bound, "edge_m": finite(a.edge_m),
        "device_ms": device_ms, "cloud_ms": cloud_ms, "check": checked,
        "held_tiles": app.phone.lock().unwrap().dev.held().count(),
    })))
}
