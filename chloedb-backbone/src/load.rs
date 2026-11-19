//! The records `fetch` downloaded into ChloeDB's tables: networks and
//! their relationships, exchanges, buildings and who is at each, ROAs,
//! probes, the latest Atlas rounds and IODA's outages. Every table is
//! written by key, so a load run again replaces what it wrote.

use std::path::Path;

use anyhow::{Context, Result};
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::{json, Value};

use crate::mrt::Prefix;
use crate::refdata::{self, Reference};
use crate::strata::Strata;

/// Rows into `table` as JSON lines, a few MB a request; a 429 or a failed
/// request is tried again.
pub async fn import(db: &Strata, table: &str, rows: impl IntoIterator<Item = Value>) -> Result<usize> {
    let mut buf: Vec<u8> = Vec::with_capacity(8 << 20);
    let mut n = 0;
    let mut sent = 0;
    for r in rows {
        serde_json::to_writer(&mut buf, &r)?;
        buf.push(b'\n');
        n += 1;
        if buf.len() > 6 << 20 {
            send(db, table, std::mem::take(&mut buf)).await?;
            sent = n;
        }
    }
    if n > sent {
        send(db, table, buf).await?;
    }
    Ok(n)
}

async fn send(db: &Strata, table: &str, body: Vec<u8>) -> Result<()> {
    let mut wait = std::time::Duration::from_millis(500);
    for attempt in 0..30 {
        match db.import_lines(table, body.clone()).await {
            Ok(_) => return Ok(()),
            Err(e) if (e.status == 0 || e.status == 429 || e.status >= 500) && attempt < 29 => {
                tracing::warn!(table, error = %e, "import: trying again");
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(std::time::Duration::from_secs(10));
            }
            Err(e) => return Err(anyhow::anyhow!("{table}: {e}")),
        }
    }
    unreachable!()
}

pub async fn create(db: &Strata) -> Result<()> {
    for (name, sql, _) in crate::schema::tables() {
        db.sql(sql).await.map_err(|e| anyhow::anyhow!("{name}: {e}"))?;
    }
    for sql in crate::schema::ROLLUPS {
        if let Err(e) = db.sql(sql).await {
            if !e.message.contains("exists") {
                tracing::warn!(error = %e, "rollup");
            }
        }
    }
    crate::rules::install(db).await?;
    Ok(())
}

fn ts(v: &Value) -> Value {
    match v {
        Value::String(s) if !s.is_empty() => json!(s),
        Value::Number(n) => n
            .as_i64()
            .and_then(|s| chrono::DateTime::from_timestamp(s, 0))
            .map(|d| json!(d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)))
            .unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

fn pdb(dir: &Path, name: &str) -> Result<Vec<Value>> {
    let p = dir.join(format!("pdb/{name}.json"));
    let mut j: Value = serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(&p).with_context(|| p.display().to_string())?))?;
    Ok(match j["data"].take() {
        Value::Array(a) => a.into_iter().filter(|r| r["status"].as_str().unwrap_or("ok") == "ok").collect(),
        _ => Vec::new(),
    })
}

fn f(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// Probe id -> (asn, country).
pub type Probes = FxHashMap<u64, (u64, String)>;

pub fn read_probes(dir: &Path) -> Result<Vec<Value>> {
    let p = dir.join("atlas/probes.json");
    let mut j: Value = serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(&p).with_context(|| p.display().to_string())?))?;
    Ok(match j["objects"].take() {
        Value::Array(a) => a,
        _ => match j {
            Value::Array(a) => a,
            _ => Vec::new(),
        },
    })
}

pub struct Opts<'a> {
    pub dir: &'a Path,
    /// Only these tables' sources (as, rel, pdb, roa, probe, atlas, ioda); all if empty.
    pub only: Vec<String>,
}

/// The reference files' names, sizes and times, as one short tag: a full
/// load whose files carry the tag of the last one loads only the latest
/// Atlas rounds and outages. (The feed's container loads before it starts,
/// and writing two million rows again on each restart kept ChloeDB busy
/// for minutes.)
pub fn reference_tag(dir: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    for f in ["rpki/vrps.json", "caida/pfx2as-v4.gz", "caida/pfx2as-v6.gz", "caida/as-rel2.txt.bz2", "ripe/asn.txt", "atlas/probes.json"] {
        parts.push(stamp(&dir.join(f), f));
    }
    let mut pdb: Vec<_> = std::fs::read_dir(dir.join("pdb")).map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect()).unwrap_or_default();
    pdb.sort();
    for p in pdb {
        parts.push(stamp(&p, &p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
    }
    crate::api::etag(&parts.join("\n")).trim_matches('"').to_string()
}

fn stamp(p: &Path, name: &str) -> String {
    let m = std::fs::metadata(p).ok();
    let size = m.as_ref().map_or(0, |m| m.len());
    let at = m.and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    format!("{name} {size} {at}")
}

pub async fn run(db: &Strata, o: Opts<'_>, r: &Reference) -> Result<()> {
    create(db).await?;
    let dir = o.dir;
    let tag = o.only.is_empty().then(|| reference_tag(dir));
    let loaded = match &tag {
        Some(tag) => db
            .query("SELECT note FROM bb_feed WHERE id = 'reference'", Value::Null)
            .await
            .ok()
            .and_then(|rows| rows.first().map(|x| crate::strata::RowExt::s(x, "note")))
            .is_some_and(|n| n.ends_with(tag.as_str())),
        None => false,
    };
    if loaded {
        tracing::info!("the reference files are those loaded last: loading the latest Atlas rounds and outages only");
    }
    let want = |s: &str| if loaded { matches!(s, "atlas" | "ioda") } else { o.only.is_empty() || o.only.iter().any(|x| x == s) };
    let t0 = std::time::Instant::now();
    let say = |what: &str, n: usize| tracing::info!(what, rows = n, secs = t0.elapsed().as_secs_f32(), "loaded");

    let probes = read_probes(dir).unwrap_or_default();
    let connected: Vec<&Value> = probes.iter().filter(|p| p["status"].as_i64() == Some(1) || p["status"]["id"].as_i64() == Some(1)).collect();

    if want("as") || want("pdb") {
        let nets = pdb(dir, "net")?;
        let mut by_asn: FxHashMap<u64, &Value> = FxHashMap::default();
        for n in &nets {
            if let Some(a) = n["asn"].as_u64() {
                by_asn.insert(a, n);
            }
        }
        let mut roas: FxHashMap<u32, i64> = FxHashMap::default();
        if let Ok(v) = read_roas(dir) {
            for (_, asn, _, _, _) in &v {
                *roas.entry(*asn).or_default() += 1;
            }
        }
        let mut probe_n: FxHashMap<u64, i64> = FxHashMap::default();
        for p in &connected {
            if let Some(a) = p["asn_v4"].as_u64() {
                *probe_n.entry(a).or_default() += 1;
            }
        }
        let mut asns: FxHashSet<u32> = r.names.keys().copied().collect();
        asns.extend(r.providers.keys());
        asns.extend(r.customers.keys());
        asns.extend(r.peers.keys());
        asns.extend(by_asn.keys().map(|a| *a as u32));
        let rows = asns.into_iter().map(|a| {
            let (rir, cc) = r.names.get(&a).cloned().unwrap_or_default();
            let net = by_asn.get(&(a as u64));
            let handle = rir.split([' ']).next().unwrap_or("").to_string();
            let name = net.and_then(|n| n["name"].as_str()).map(|s| refdata::trading_as(s).to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| refdata::short_name(&rir));
            let mut row = json!({
                "asn": a, "name": format!("{name} {rir}"), "short": name, "handle": handle, "country": cc,
                "providers": r.providers.get(&a).map_or(0, |v| v.len()),
                "customers": r.customers.get(&a).map_or(0, |v| v.len()),
                "peers": r.peers.get(&a).map_or(0, |v| v.len()),
                "roas": roas.get(&a).copied().unwrap_or(0),
                "probes": probe_n.get(&(a as u64)).copied().unwrap_or(0),
            });
            if let Some(n) = net {
                row["pdb"] = n["id"].clone();
                row["kind"] = n["info_type"].clone();
                row["scope"] = n["info_scope"].clone();
                row["traffic"] = n["info_traffic"].clone();
                row["website"] = n["website"].clone();
                row["prefixes4"] = n["info_prefixes4"].clone();
                row["prefixes6"] = n["info_prefixes6"].clone();
                row["ixes"] = n["ix_count"].clone();
                row["facs"] = n["fac_count"].clone();
                row["updated"] = ts(&n["updated"]);
                row["created"] = ts(&n["created"]);
            }
            row
        });
        say("bb_as", import(db, "bb_as", rows.collect::<Vec<_>>()).await?);
    }

    if want("rel") {
        let rel = refdata::read_rel(&dir.join("caida/as-rel2.txt.bz2"))?;
        let p2c = rel.iter().filter(|x| x.2 == -1).map(|(a, b, _)| json!({"id": format!("{a}>{b}"), "src": a, "dst": b}));
        say("bb_p2c", import(db, "bb_p2c", p2c.collect::<Vec<_>>()).await?);
        let p2p = rel.iter().filter(|x| x.2 == 0).map(|(a, b, _)| json!({"id": format!("{a}-{b}"), "src": a, "dst": b}));
        say("bb_p2p", import(db, "bb_p2p", p2p.collect::<Vec<_>>()).await?);
    }

    if want("pdb") {
        let facs = pdb(dir, "fac")?;
        let mut fac_at: FxHashMap<u64, (f64, f64)> = FxHashMap::default();
        for fa in &facs {
            if let (Some(id), Some(lat), Some(lon)) = (fa["id"].as_u64(), f(&fa["latitude"]), f(&fa["longitude"])) {
                fac_at.insert(id, (lat, lon));
            }
        }
        let rows = facs.iter().map(|fa| json!({
            "id": fa["id"], "name": fa["name"], "short": fa["name"], "city": fa["city"], "country": fa["country"],
            "lat": f(&fa["latitude"]), "lon": f(&fa["longitude"]), "nets": fa["net_count"], "ixes": fa["ix_count"], "updated": ts(&fa["updated"]),
        }));
        say("bb_fac", import(db, "bb_fac", rows.collect::<Vec<_>>()).await?);
        let mut ix_at: FxHashMap<u64, Vec<(f64, f64)>> = FxHashMap::default();
        for x in pdb(dir, "ixfac")? {
            if let (Some(ix), Some(fac)) = (x["ix_id"].as_u64(), x["fac_id"].as_u64()) {
                if let Some(at) = fac_at.get(&fac) {
                    ix_at.entry(ix).or_default().push(*at);
                }
            }
        }
        let ixes = pdb(dir, "ix")?;
        let rows = ixes.iter().map(|x| {
            let id = x["id"].as_u64().unwrap_or(0);
            let at = ix_at.get(&id).map(|v| median_point(v));
            json!({
                "id": id, "name": format!("{} {}", x["name"].as_str().unwrap_or(""), x["name_long"].as_str().unwrap_or("")), "short": x["name"],
                "city": x["city"], "country": x["country"], "continent": x["region_continent"], "members": x["net_count"],
                "lat": at.map(|a| a.0), "lon": at.map(|a| a.1), "website": x["website"], "updated": ts(&x["updated"]), "created": ts(&x["created"]),
            })
        });
        say("bb_ix", import(db, "bb_ix", rows.collect::<Vec<_>>()).await?);
        let rows = pdb(dir, "netixlan")?.into_iter().map(|m| json!({
            "id": m["id"], "ix": m["ix_id"], "asn": m["asn"], "speed": m["speed"], "ip4": m["ipaddr4"], "ip6": m["ipaddr6"],
            "rs": m["is_rs_peer"].as_bool().unwrap_or(false), "updated": ts(&m["updated"]), "created": ts(&m["created"]),
        }));
        say("bb_member", import(db, "bb_member", rows.collect::<Vec<_>>()).await?);
        let rows = pdb(dir, "netfac")?.into_iter().map(|m| json!({
            "id": m["id"], "fac": m["fac_id"], "asn": m["local_asn"], "updated": ts(&m["updated"]), "created": ts(&m["created"]),
        }));
        say("bb_netfac", import(db, "bb_netfac", rows.collect::<Vec<_>>()).await?);
    }

    if want("roa") {
        let roas = read_roas(dir)?;
        let rows = roas.into_iter().map(|(p, asn, max, ta, expires)| {
            let (lo, hi) = p.range();
            json!({"id": format!("{p}-{asn}-{max}"), "prefix": p.to_string(), "asn": asn, "maxlen": max, "af": if p.v6 { 6 } else { 4 },
                   "plen": p.len, "lo": lo, "hi": hi, "ta": ta, "expires": ts(&json!(expires))})
        });
        say("bb_roa", import(db, "bb_roa", rows.collect::<Vec<_>>()).await?);
    }

    if want("probe") {
        let rows = connected.iter().map(|p| json!({
            "id": p["id"], "asn": p["asn_v4"], "country": p["country_code"], "lat": p["latitude"], "lon": p["longitude"],
            "prefix": p["prefix_v4"], "anchor": p["is_anchor"].as_bool().unwrap_or(false), "status": "connected",
            "since": ts(&p["status_since"]),
        }));
        say("bb_probe", import(db, "bb_probe", rows.collect::<Vec<_>>()).await?);
    }

    if want("atlas") {
        let mut by_probe: Probes = FxHashMap::default();
        for p in &connected {
            if let Some(id) = p["id"].as_u64() {
                by_probe.insert(id, (p["asn_v4"].as_u64().unwrap_or(0), p["country_code"].as_str().unwrap_or("").to_string()));
            }
        }
        let rd = std::fs::read_dir(dir.join("atlas")).map(|d| d.flatten().collect::<Vec<_>>()).unwrap_or_default();
        for e in rd {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(msm) = name.strip_suffix(".json").and_then(|m| m.parse::<u64>().ok()) else { continue };
            let j: Value = serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(e.path())?))?;
            // A measurement's latest results hold every probe's last one:
            // probes that went away days ago, and some with clocks ahead.
            // Keep the hour before the file was fetched.
            let fetched = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(i64::MAX, |d| d.as_secs() as i64);
            let results: Vec<Value> = j.as_array().cloned().unwrap_or_default().into_iter().filter(|x| x["timestamp"].as_i64().is_some_and(|t| t <= fetched + 300 && t >= fetched - 3600)).collect();
            let (pings, traces) = crate::atlas::rows(&results, &by_probe, r);
            if !pings.is_empty() {
                say(&format!("bb_ping ({msm})"), import(db, "bb_ping", pings).await?);
            }
            if !traces.is_empty() {
                say(&format!("bb_trace ({msm})"), import(db, "bb_trace", traces).await?);
            }
        }
    }

    if want("ioda") {
        let p = dir.join("ioda/events.json");
        if p.exists() {
            let j: Value = serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(&p)?))?;
            let fetched = std::fs::metadata(&p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs() as i64);
            let rows = crate::outages::rows(&j, fetched, &|a| r.country(a as u32).to_string());
            say("bb_outage", import(db, "bb_outage", rows).await?);
        }
    }
    if let (Some(tag), false) = (tag, loaded) {
        let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        db.upsert("bb_feed", vec![json!({"id": "reference", "kind": "load", "at": at, "rows": 0, "note": format!("networks, relationships, exchanges, ROAs, probes · files {tag}")})])
            .await
            .map_err(|e| anyhow::anyhow!("bb_feed: {e}"))?;
    }
    Ok(())
}

fn median_point(v: &[(f64, f64)]) -> (f64, f64) {
    let mut lat: Vec<f64> = v.iter().map(|x| x.0).collect();
    let mut lon: Vec<f64> = v.iter().map(|x| x.1).collect();
    lat.sort_by(|a, b| a.total_cmp(b));
    lon.sort_by(|a, b| a.total_cmp(b));
    (lat[lat.len() / 2], lon[lon.len() / 2])
}

/// (prefix, asn, max length, trust anchor, expires) of every ROA.
pub fn read_roas(dir: &Path) -> Result<Vec<(Prefix, u32, u8, String, i64)>> {
    let p = dir.join("rpki/vrps.json");
    let j: Value = serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(&p).with_context(|| p.display().to_string())?))?;
    let mut out = Vec::new();
    for roa in j["roas"].as_array().into_iter().flatten() {
        let Some(pf) = roa["prefix"].as_str().and_then(Prefix::parse) else { continue };
        out.push((
            pf,
            roa["asn"].as_u64().unwrap_or(0) as u32,
            roa["maxLength"].as_u64().unwrap_or(pf.len as u64) as u8,
            roa["ta"].as_str().unwrap_or("").to_string(),
            roa["expires"].as_i64().unwrap_or(0),
        ));
    }
    Ok(out)
}
