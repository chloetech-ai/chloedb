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
//! Every file is replaced only once its download is whole.

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
}

pub fn run(o: Opts) -> Result<()> {
    let c = client();
    let want = |s: &str| o.only.is_empty() || o.only.iter().any(|x| x == s);
    let d = &o.dir;
    let say = |what: &str, n: u64| eprintln!("{what}: {:.1} MB", n as f64 / 1e6);
    if want("rpki") {
        say("rpki/vrps.json", get(&c, "https://console.rpki-client.org/vrps.json", &d.join("rpki/vrps.json"))?);
    }
    if want("caida") {
        let now = chrono::Utc::now().date_naive();
        for (fam, dataset, stem, out) in [
            ("v4", "routeviews-prefix2as", "routeviews-rv2-", "caida/pfx2as-v4.gz"),
            ("v6", "routeviews6-prefix2as", "routeviews-rv6-", "caida/pfx2as-v6.gz"),
        ] {
            let mut done = false;
            for back in 0..2 {
                let m = now - chrono::Months::new(back);
                let base = format!("https://publicdata.caida.org/datasets/routing/{dataset}/{}/", m.format("%Y/%m"));
                let Ok(html) = text(&c, &base) else { continue };
                if let Some(name) = listed(&html, stem, ".pfx2as.gz").pop() {
                    say(&format!("{out} ({fam}, {name})"), get(&c, &format!("{base}{name}"), &d.join(out))?);
                    done = true;
                    break;
                }
            }
            if !done {
                eprintln!("{out}: no file found at CAIDA");
            }
        }
        let base = "https://publicdata.caida.org/datasets/as-relationships/serial-2/";
        let html = text(&c, base)?;
        let Some(name) = listed(&html, "20", ".as-rel2.txt.bz2").pop() else { bail!("no as-rel2 file at {base}") };
        say(&format!("caida/as-rel2.txt.bz2 ({name})"), get(&c, &format!("{base}{name}"), &d.join("caida/as-rel2.txt.bz2"))?);
    }
    if want("ripe") {
        say("ripe/asn.txt", get(&c, "https://ftp.ripe.net/ripe/asnames/asn.txt", &d.join("ripe/asn.txt"))?);
    }
    if want("pdb") {
        // PeeringDB lets an address without an account make a few calls a
        // minute: one object type each, a few seconds apart.
        for (i, o) in ["net", "ix", "fac", "netixlan", "netfac", "ixfac", "org"].iter().enumerate() {
            if i > 0 {
                std::thread::sleep(Duration::from_secs(4));
            }
            let path = d.join(format!("pdb/{o}.json"));
            say(&format!("pdb/{o}.json"), get(&c, &format!("https://www.peeringdb.com/api/{o}?depth=0"), &path).with_context(|| format!("PeeringDB {o}"))?);
        }
    }
    if want("atlas") {
        // The probes' archive of yesterday (today's may not be written yet).
        let mut got = false;
        for back in 1..4 {
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
            say(&format!("atlas/{m}.json"), get(&c, &url, &d.join(format!("atlas/{m}.json")))?);
        }
    }
    if want("ioda") {
        let until = chrono::Utc::now().timestamp();
        let from = until - 7 * 86400;
        let url = format!("https://api.ioda.inetintel.cc.gatech.edu/v2/outages/events?from={from}&until={until}&limit=10000&format=ioda");
        say("ioda/events.json", get(&c, &url, &d.join("ioda/events.json"))?);
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
