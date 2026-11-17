//! chloedb-sound against a real chloedb-server: `serve --local` starts one,
//! loads a few made-up sounds with their moments (vectors given, as
//! tools/prepare.py gives them), and the test asks what the pages ask:
//! searches by a sound and by words, a sound's page with its credit line,
//! drifts, the map, the front page, the counts, and the previews with
//! ranges.
//!
//! It needs the chloedb-server binary (`CHLOEDB_SERVER_BIN`, else
//! `../chloedb-server/target/{ci,release}/chloedb-server`), and is skipped
//! without it unless `CHLOEDB_SOUND_NEED_SERVER` is set (as CI does).
//! Searches by words through CLAP, and the clip a visitor hums, need a
//! server built with `--features audio` and its model: set
//! `CHLOEDB_SOUND_CLAP=1` to test those too.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const TOKEN: &str = "sound-test-token";
const DIM: usize = 512;

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

/// A unit vector near axis `axis`, moved a little along `wiggle`.
fn vector(axis: usize, wiggle: usize, by: f32) -> Vec<f32> {
    let mut v = vec![0f32; DIM];
    v[axis] = 1.0;
    v[wiggle] += by;
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.iter().map(|x| x / n).collect()
}

/// (id, title, author, source, kind, licence, lic, seconds, axis, wiggle)
const SOUNDS: &[(&str, &str, &str, &str, &str, &str, &str, f64, usize, usize)] = &[
    ("fsd:1", "Rain on a tin roof", "ana", "fsd50k", "nature", "CC0 1.0", "cc0", 12.0, 0, 10),
    ("fsd:2", "Heavy rain in the garden", "ana", "fsd50k", "nature", "CC BY 3.0", "by", 25.0, 0, 11),
    ("wc:3", "Rain and thunder at night", "bo", "commons", "place", "CC BY-SA 4.0", "by-sa", 95.0, 0, 12),
    ("fsd:4", "Blackbird singing at dawn", "cy", "fsd50k", "animal", "CC0 1.0", "cc0", 18.0, 1, 13),
    ("wc:5", "Garden birds in spring", "bo", "commons", "animal", "CC BY-SA 4.0", "by-sa", 64.0, 1, 14),
    ("fsd:6", "Sparrows chirping", "di", "fsd50k", "animal", "CC BY 3.0", "by", 9.0, 1, 15),
    ("fma:7", "Slow piano", "Elle", "fma", "music", "CC BY 4.0", "by", 30.0, 2, 16),
    ("fma:8", "Piano and strings", "Elle", "fma", "music", "CC BY-SA 3.0", "by-sa", 30.0, 2, 17),
    ("fma:9", "Drums and bass", "Fen", "fma", "music", "CC0 1.0", "cc0", 30.0, 3, 2),
];

fn write_rows(dir: &Path) -> usize {
    std::fs::create_dir_all(dir.join("audio")).unwrap();
    let mut s = std::fs::File::create(dir.join("au_sounds.jsonl")).unwrap();
    let mut m = std::fs::File::create(dir.join("au_moments.jsonl")).unwrap();
    let mut moments = 0;
    for (k, &(id, title, author, source, kind, licence, lic, seconds, axis, wiggle)) in SOUNDS.iter().enumerate() {
        let audio = format!("{}.mp3", id.replace(':', "_"));
        // Not a real MP3: the app serves the bytes as they are.
        std::fs::write(dir.join("audio").join(&audio), format!("ID3 the preview of {id}").repeat(20)).unwrap();
        let emb = vector(axis, wiggle, 0.2);
        let peaks: Vec<String> = (0..160).map(|i| ((i * 7 + k * 13) % 100).to_string()).collect();
        let (lat, lon) = if source == "commons" { (json!(51.5), json!(-0.12)) } else { (Value::Null, Value::Null) };
        let n = ((seconds - 10.0) / 5.0).floor().max(0.0) as usize + 1;
        writeln!(s, "{}", json!({
            "id": id, "title": title, "words": format!("{title} {kind}"), "author": author, "author_url": format!("https://example.org/people/{author}/"),
            "page": format!("https://example.org/s/{id}"), "source": source, "licence": licence, "licence_url": "https://creativecommons.org/", "lic": lic,
            "kind": kind, "tags": [kind], "labels": [], "genres": [], "seconds": seconds, "year": 2020, "lat": lat, "lon": lon,
            "audio": audio, "peaks": peaks.join(","), "x": (k as f64) / 9.0, "y": (axis as f64) / 4.0, "moments": n, "emb": emb,
        })).unwrap();
        for i in 0..n {
            let t = (i * 5) as f64;
            // The rain and thunder recording turns to thunder (axis 4) after its first minute.
            let v = if id == "wc:3" && t >= 60.0 { vector(4, 12, 0.1) } else { vector(axis, wiggle, 0.2 + 0.01 * i as f32) };
            writeln!(m, "{}", json!({"id": format!("{id}@{t}"), "sound": id, "t": t, "seconds": seconds, "emb": v})).unwrap();
            moments += 1;
        }
    }
    moments
}

struct App {
    child: Child,
    url: String,
}

impl App {
    fn start(dir: &Path, rows: &Path, bin: &Path, public: bool) -> App {
        let (port, strata) = (free_port(), free_port());
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_chloedb-sound"));
        cmd.args(["serve", "--local"])
            .arg(dir)
            .arg("--server-bin")
            .arg(bin)
            .arg("--rows")
            .arg(rows)
            .args(["--listen", &format!("127.0.0.1:{port}"), "--local-listen", &format!("127.0.0.1:{strata}"), "--token", TOKEN]);
        if public {
            cmd.args(["--public", "--per-minute", "10"]);
        }
        let child = cmd.stdout(Stdio::null()).stderr(Stdio::inherit()).spawn().expect("chloedb-sound");
        App { child, url: format!("http://127.0.0.1:{port}") }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            // SAFETY: a signal to a child this test started.
            unsafe {
                libc::kill(self.child.id() as i32, libc::SIGTERM);
            }
            let t0 = Instant::now();
            while t0.elapsed() < Duration::from_secs(20) && self.child.try_wait().ok().flatten().is_none() {
                std::thread::sleep(Duration::from_millis(100));
            }
            let _ = self.child.kill();
        }
    }
}

async fn get(http: &reqwest::Client, app: &App, path: &str, q: &[(&str, &str)]) -> Value {
    let r = http.get(format!("{}{path}", app.url)).bearer_auth(TOKEN).query(q).send().await.unwrap_or_else(|e| panic!("{path}: {e}"));
    let status = r.status();
    let body = r.text().await.unwrap();
    assert!(status.is_success(), "{path} {q:?}: {status} {body}");
    serde_json::from_str(&body).unwrap_or_else(|e| panic!("{path}: {e}: {body}"))
}

fn ids(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap_or("").to_string()).collect()
}

async fn up(http: &reqwest::Client, app: &App) {
    let t0 = Instant::now();
    loop {
        if let Ok(r) = http.get(format!("{}/healthz", app.url)).send().await {
            if r.status().is_success() {
                return;
            }
        }
        assert!(t0.elapsed() < Duration::from_secs(240), "chloedb-sound did not answer in four minutes");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn pages_on_a_real_server() {
    let Some(bin) = server_bin() else {
        assert!(std::env::var_os("CHLOEDB_SOUND_NEED_SERVER").is_none(), "CHLOEDB_SOUND_NEED_SERVER is set and there is no chloedb-server binary");
        eprintln!("skipped: no chloedb-server binary (CHLOEDB_SERVER_BIN)");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let rows = tmp.path().join("rows");
    let moments = write_rows(&rows);
    let app = App::start(&tmp.path().join("db"), &rows, &bin, false);
    let http = reqwest::Client::builder().timeout(Duration::from_secs(120)).build().unwrap();
    up(&http, &app).await;

    // The token is needed: the app is not public.
    let r = http.get(format!("{}/v1/status", app.url)).send().await.unwrap();
    assert_eq!(r.status(), 401);

    let st = get(&http, &app, "/v1/status", &[]).await;
    assert_eq!(st["stats"]["sounds"], 9, "{st}");
    assert_eq!(st["stats"]["moments"], moments as i64, "{st}");
    assert_eq!(st["stats"]["labels"], chloedb_sound::labels::LABELS.len() as i64, "{st}");
    assert!(!st["queries"].as_array().unwrap().is_empty(), "every answer carries its SQL");

    // More like a rain sound: the other rain first, music last.
    let s = get(&http, &app, "/v1/search", &[("like", "fsd:1")]).await;
    let got = ids(&s["results"]);
    assert_eq!(got[0], "fsd:1", "{got:?}");
    assert!(got[1..3].iter().all(|i| i == "fsd:2" || i == "wc:3"), "{got:?}");
    assert_eq!(s["by"], "sound");
    // Steered away from a rain sound, a filter on the kind.
    let s = get(&http, &app, "/v1/search", &[("like", "fsd:4"), ("unlike", "fsd:1"), ("kind", "animal")]).await;
    assert!(ids(&s["results"]).iter().all(|i| ["fsd:4", "wc:5", "fsd:6"].contains(&i.as_str())), "{s}");
    let kinds = &s["facets"]["kind"];
    assert_eq!(kinds[0]["value"], "animal", "{kinds}");
    // By words alone (BM25), which needs no model.
    let s = get(&http, &app, "/v1/search", &[("q", "piano"), ("by", "words")]).await;
    let got = ids(&s["results"]);
    assert!(got.len() == 2 && got.iter().all(|i| i == "fma:7" || i == "fma:8"), "{got:?}");

    // A sound's page: its credit, sounds like it, more by its author.
    let p = get(&http, &app, "/v1/sound/fsd:2", &[]).await;
    assert_eq!(p["sound"]["credit"], "\u{201c}Heavy rain in the garden\u{201d} by ana, Freesound (FSD50K) (https://example.org/s/fsd:2), CC BY 3.0 (https://creativecommons.org/)");
    assert_eq!(ids(&p["like"])[0], "fsd:1", "{}", p["like"]);
    assert_eq!(ids(&p["more"]), vec!["fsd:1"]);
    assert_eq!(p["moments"], 4);
    let r = http.get(format!("{}/v1/sound/nope", app.url)).bearer_auth(TOKEN).send().await.unwrap();
    assert_eq!(r.status(), 404);

    // A drift from rain to piano ends where it is asked to.
    let d = get(&http, &app, "/v1/drift", &[("from", "fsd:1"), ("to", "fma:7"), ("steps", "4")]).await;
    let path = ids(&d["path"]);
    assert_eq!(path.first().unwrap(), "fsd:1");
    assert_eq!(path.last().unwrap(), "fma:7");
    assert!(path.len() >= 3, "{path:?}");

    // The map, the front page.
    let m = get(&http, &app, "/v1/map", &[]).await;
    assert_eq!(m["points"].as_array().unwrap().len(), 9);
    let h = get(&http, &app, "/v1/home", &[]).await;
    let kinds: Vec<&str> = h["shelves"].as_array().unwrap().iter().filter_map(|s| s["kind"].as_str()).collect();
    assert!(kinds.contains(&"music") && kinds.contains(&"animal"), "{kinds:?}");

    // A preview, whole, by range, and as JSON for the command center.
    let r = http.get(format!("{}/v1/audio/fsd:1", app.url)).bearer_auth(TOKEN).header("range", "bytes=0-9").send().await.unwrap();
    assert_eq!(r.status(), 206);
    assert_eq!(r.headers()["content-range"].to_str().unwrap(), format!("bytes 0-9/{}", "ID3 the preview of fsd:1".len() * 20));
    assert_eq!(r.bytes().await.unwrap().as_ref(), b"ID3 the pr");
    let j = get(&http, &app, "/v1/audio/fsd:1", &[("b64", "1")]).await;
    assert_eq!(j["mime"], "audio/mpeg");
    let r = http.get(format!("{}/v1/audio/nope", app.url)).bearer_auth(TOKEN).send().await.unwrap();
    assert_eq!(r.status(), 404);

    // Words through CLAP, moments and scenes: with the model only.
    if std::env::var_os("CHLOEDB_SOUND_CLAP").is_some() {
        let s = get(&http, &app, "/v1/search", &[("q", "rain")]).await;
        assert_eq!(s["by"], "both");
        assert_eq!(s["results"].as_array().unwrap().len(), 9);
        let m = get(&http, &app, "/v1/moments", &[("q", "thunder"), ("min", "0")]).await;
        assert!(!m["results"].as_array().unwrap().is_empty());
        let sc = get(&http, &app, "/v1/scene", &[("q", "a rainy garden")]).await;
        assert!(!sc["layers"].as_array().unwrap().is_empty(), "{sc}");
    } else {
        // A server that cannot run CLAP still answers words, by words.
        let s = get(&http, &app, "/v1/search", &[("q", "piano")]).await;
        assert_eq!(s["by"], "words", "{s}");
        assert!(ids(&s["results"]).contains(&"fma:7".to_string()), "{s}");
        assert!(s["understood"].as_array().unwrap().iter().any(|u| u["kind"] == "note"), "{s}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn public_reads_within_a_limit() {
    let Some(bin) = server_bin() else {
        assert!(std::env::var_os("CHLOEDB_SOUND_NEED_SERVER").is_none(), "CHLOEDB_SOUND_NEED_SERVER is set and there is no chloedb-server binary");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let rows = tmp.path().join("rows");
    write_rows(&rows);
    let app = App::start(&tmp.path().join("db"), &rows, &bin, true);
    let http = reqwest::Client::builder().timeout(Duration::from_secs(120)).build().unwrap();
    up(&http, &app).await;
    // The page itself, to anyone.
    let r = http.get(&app.url).send().await.unwrap();
    assert!(r.status().is_success());
    assert!(r.text().await.unwrap().contains("Sound Explorer"));
    // And to a HEAD, as uptime checks and link previews ask.
    let r = http.head(&app.url).send().await.unwrap();
    assert_eq!(r.status(), 200);
    // Another site's page may not call it.
    let r = http.get(format!("{}/v1/map", app.url)).header("origin", "https://elsewhere.example").send().await.unwrap();
    assert_eq!(r.status(), 403);
    // Reads without the token, ten a minute from one address.
    let mut codes = Vec::new();
    for _ in 0..12 {
        codes.push(http.get(format!("{}/v1/map", app.url)).send().await.unwrap().status().as_u16());
    }
    assert!(codes[..10].iter().all(|c| *c == 200), "{codes:?}");
    assert_eq!(codes[11], 429, "{codes:?}");
    // Previews are not counted.
    let r = http.get(format!("{}/v1/audio/fma:9", app.url)).send().await.unwrap();
    assert_eq!(r.status(), 200);
    // A clip that is not a sound file in base64 is refused before the model.
    let r = http.post(format!("{}/v1/listen", app.url)).bearer_auth(TOKEN).json(&json!({"audio": "not base64 at all!"})).send().await.unwrap();
    assert_eq!(r.status(), 400);
}
