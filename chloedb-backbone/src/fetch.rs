//! Downloads the public sources into a folder, which `load` and `feed`
//! read:
//!
//! | file | from | what |
//! |---|---|---|
//! | `rpki/vrps.json` | rpki-client (console.rpki-client.org) | every validated ROA, rebuilt hourly |
//! | `caida/pfx2as-v4.gz`, `-v6.gz` | CAIDA, from RouteViews | each prefix's origins, a day |
//! | `caida/as-rel2.txt.bz2` | CAIDA | who is whose provider, customer or peer, a month |
//! | `ripe/asn.txt` | RIPE NCC, from the RIRs | each AS's name and country |
//! | `pdb/*.json` | PeeringDB's API | networks, exchanges, buildings, and who is where |
//! | `atlas/probes.json` | RIPE Atlas's daily archive | every probe and where it is |
//! | `atlas/<msm>.json` | RIPE Atlas's API | the latest round of a measurement |
//! | `ioda/events.json` | IODA (Georgia Tech) | outages of the last days |
//!
//! Every file is replaced only once its download is whole. A file
//! fetched less than `fresh` ago is kept as it is (the feed's restarts do
//! not ask PeeringDB again, which answers an address without an account a
//! few times a minute, then 429), and a source that fails keeps its last
//! file: the feed reads what it had rather than stopping.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};

pub const UA: &str = concat!("chloedb-backbone/", env!("CARGO_PKG_VERSION"));

pub fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(900))
        .connect_timeout(Duration::from_secs(20))
        .build()
        .expect("an HTTP client")
}

/// `url` into `path`, through a `.part` file; tried three times.
pub fn get(c: &reqwest::blocking::Client, url: &str, path: &Path) -> Result<u64> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let part = path.with_extension("part");
    let mut last = None;
    for attempt in 0..3 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(5 * attempt));
        }
        match (|| -> Result<u64> {
            let mut r = c.get(url).send()?;
            if !r.status().is_success() {
                bail!("{url}: HTTP {}", r.status());
            }
            let mut f = std::io::BufWriter::new(std::fs::File::create(&part)?);
            let n = r.copy_to(&mut f)?;
            f.flush()?;
            Ok(n)
        })() {
            Ok(n) => {
                std::fs::rename(&part, path)?;
                return Ok(n);
            }
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap())
}

fn text(c: &reqwest::blocking::Client, url: &str) -> Result<String> {
    let r = c.get(url).send()?;
    if !r.status().is_success() {
        bail!("{url}: HTTP {}", r.status());
    }
    Ok(r.text()?)
}

/// The names in a directory listing that look like `pat` (a prefix and a
/// suffix around a date), newest last.
fn listed(html: &str, prefix: &str, suffix: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in html.split(['"', '>', '<']) {
        if part.starts_with(prefix) && part.ends_with(suffix) && !out.iter().any(|x| x == part) {
            out.push(part.to_string());
        }
    }
    out.sort();
    out
}

pub struct Opts {
    pub dir: PathBuf,
    /// Only these sources (rpki, caida, ripe, pdb, atlas, ioda); all if empty.
    pub only: Vec<String>,
    /// Atlas measurements whose latest round to fetch.
    pub atlas: Vec<u64>,
    /// Keep a file fetched less than this long ago (zero: fetch every one).
    pub fresh: Duration,
}

/// How long ago `path` was written, if it is there.
fn age(path: &Path) -> Option<Duration> {
    std::fs::metadata(path).ok()?.modified().ok()?.elapsed().ok().or(Some(Duration::ZERO))
}

fn hours(d: Duration) -> String {
    format!("{:.1} h", d.as_secs_f64() / 3600.0)
}

/// Whether `path` is new enough to keep without asking its source again.
fn fresh(path: &Path, fresh: Duration, what: &str) -> bool {
    match age(path) {
        Some(a) if !fresh.is_zero() && a < fresh => {
            eprintln!("{what}: kept, fetched {} ago", hours(a));
            true
        }
        _ => false,
    }
}

/// A download's outcome, where a failure keeps the file of the last one
/// when there is one.
fn kept(r: Result<u64>, path: &Path, what: &str) -> Result<u64> {
    match (r, age(path)) {
        (Ok(n), _) => Ok(n),
        (Err(e), Some(a)) => {
            eprintln!("{what}: {e:#}; keeping the file fetched {} ago", hours(a));
            Ok(0)
        }
        (Err(e), None) => Err(e),
    }
}

pub fn run(o: Opts) -> Result<()> {
    let c = client();
    let want = |s: &str| o.only.is_empty() || o.only.iter().any(|x| x == s);
    let d = &o.dir;
    let say = |what: &str, n: u64| eprintln!("{what}: {:.1} MB", n as f64 / 1e6);
    let fetch = |what: &str, url: &str, path: &Path| -> Result<()> {
        if fresh(path, o.fresh, what) {
            return Ok(());
        }
        let n = kept(get(&c, url, path), path, what)?;
        if n > 0 {
            say(what, n);
        }
        Ok(())
    };
    if want("rpki") {
        fetch("rpki/vrps.json", "https://console.rpki-client.org/vrps.json", &d.join("rpki/vrps.json"))?;
    }
    if want("caida") {
        let now = chrono::Utc::now().date_naive();
        for (fam, dataset, stem, out) in [
            ("v4", "routeviews-prefix2as", "routeviews-rv2-", "caida/pfx2as-v4.gz"),
            ("v6", "routeviews6-prefix2as", "routeviews-rv6-", "caida/pfx2as-v6.gz"),
        ] {
            if fresh(&d.join(out), o.fresh, out) {
                continue;
            }
            let mut done = false;
            for back in 0..2 {
                let m = now - chrono::Months::new(back);
                let base = format!("https://publicdata.caida.org/datasets/routing/{dataset}/{}/", m.format("%Y/%m"));
                let Ok(html) = text(&c, &base) else { continue };
                if let Some(name) = listed(&html, stem, ".pfx2as.gz").pop() {
                    fetch(&format!("{out} ({fam}, {name})"), &format!("{base}{name}"), &d.join(out))?;
                    done = true;
                    break;
                }
            }
            if !done {
                eprintln!("{out}: no file found at CAIDA");
            }
        }
        let rel = d.join("caida/as-rel2.txt.bz2");
        if !fresh(&rel, o.fresh, "caida/as-rel2.txt.bz2") {
            let base = "https://publicdata.caida.org/datasets/as-relationships/serial-2/";
            let name = text(&c, base).and_then(|html| listed(&html, "20", ".as-rel2.txt.bz2").pop().with_context(|| format!("no as-rel2 file at {base}")));
            match name {
                Ok(name) => fetch(&format!("caida/as-rel2.txt.bz2 ({name})"), &format!("{base}{name}"), &rel)?,
                Err(e) => {
                    kept(Err(e), &rel, "caida/as-rel2.txt.bz2")?;
                }
            }
        }
    }
    if want("ripe") {
        fetch("ripe/asn.txt", "https://ftp.ripe.net/ripe/asnames/asn.txt", &d.join("ripe/asn.txt"))?;
    }
    if want("pdb") {
        // PeeringDB lets an address without an account make a few calls a
        // minute: one object type each, a few seconds apart.
        let mut asked = false;
        for t in ["net", "ix", "fac", "netixlan", "netfac", "ixfac", "org"] {
            let (what, path) = (format!("pdb/{t}.json"), d.join(format!("pdb/{t}.json")));
            if fresh(&path, o.fresh, &what) {
                continue;
            }
            if asked {
                std::thread::sleep(Duration::from_secs(4));
            }
            asked = true;
            let n = kept(get(&c, &format!("https://www.peeringdb.com/api/{t}?depth=0"), &path).with_context(|| format!("PeeringDB {t}")), &path, &what)?;
            if n > 0 {
                say(&what, n);
            }
        }
    }
    if want("atlas") {
        // The probes' archive of yesterday (today's may not be written yet).
        let mut got = fresh(&d.join("atlas/probes.json"), o.fresh, "atlas/probes.json");
        for back in 1..4 {
            if got {
                break;
            }
            let day = chrono::Utc::now().date_naive() - chrono::Days::new(back);
            let url = format!("https://ftp.ripe.net/ripe/atlas/probes/archive/{}/{}.json.bz2", day.format("%Y/%m"), day.format("%Y%m%d"));
            let tmp = d.join("atlas/probes.json.bz2");
            if get(&c, &url, &tmp).is_ok() {
                let raw = std::fs::read(&tmp)?;
                let mut out = Vec::new();
                std::io::Read::read_to_end(&mut bzip2_rs::DecoderReader::new(&raw[..]), &mut out)?;
                std::fs::write(d.join("atlas/probes.json"), &out)?;
                std::fs::remove_file(&tmp).ok();
                say(&format!("atlas/probes.json ({day})"), out.len() as u64);
                got = true;
                break;
            }
        }
        if !got {
            eprintln!("atlas/probes.json: no archive of the last three days");
        }
        for m in &o.atlas {
            let url = format!("https://atlas.ripe.net/api/v2/measurements/{m}/latest/?format=json");
            fetch(&format!("atlas/{m}.json"), &url, &d.join(format!("atlas/{m}.json")))?;
        }
    }
    if want("ioda") {
        let until = chrono::Utc::now().timestamp();
        let from = until - 7 * 86400;
        let url = format!("https://api.ioda.inetintel.cc.gatech.edu/v2/outages/events?from={from}&until={until}&limit=10000&format=ioda");
        fetch("ioda/events.json", &url, &d.join("ioda/events.json"))?;
    }
    Ok(())
}

/// RIS's archive file of `collector` at the five minutes starting `t`
/// (seconds), into `dir/ris/<collector>/`. None when RIS has not written
/// it yet.
pub fn ris_file(c: &reqwest::blocking::Client, dir: &Path, collector: &str, t: i64) -> Result<Option<PathBuf>> {
    let at = chrono::DateTime::from_timestamp(t - t % 300, 0).context("a time")?;
    let name = format!("updates.{}.gz", at.format("%Y%m%d.%H%M"));
    let path = dir.join("ris").join(collector).join(&name);
    if path.exists() {
        return Ok(Some(path));
    }
    let url = format!("https://data.ris.ripe.net/{collector}/{}/{name}", at.format("%Y.%m"));
    let r = c.head(&url).send()?;
    if r.status().as_u16() == 404 {
        return Ok(None);
    }
    get(c, &url, &path)?;
    Ok(Some(path))
}
