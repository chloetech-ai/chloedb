//! IODA's outage events into `bb_outage` rows. IODA (Georgia Tech) watches
//! three signals for every country, region and network: routes in BGP,
//! addresses answering active probes, and traffic reaching a darknet
//! (`bgp`, `ping-slash24`, `merit-nt`, and Google's traffic, `gtr`); an
//! event is a stretch of time a signal fell well below its usual level.

use serde_json::{json, Value};

fn iso(s: i64) -> Value {
    chrono::DateTime::from_timestamp(s, 0).map(|d| json!(d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))).unwrap_or(Value::Null)
}

/// `fetched`: when the file was fetched (seconds); an event whose end is
/// within 20 minutes of it is still open. `country_of` names a network's
/// country. Reads the API's `format=ioda` events (an entity, from, until)
/// and its older shape (a location, start, duration).
pub fn rows(j: &Value, fetched: i64, country_of: &dyn Fn(u64) -> String) -> Vec<Value> {
    let list = j["data"].as_array().cloned().or_else(|| j.as_array().cloned()).unwrap_or_default();
    let mut out = Vec::new();
    for e in list {
        let (kind, ent, name, region_cc, start, until) = if let Some(en) = e.get("entity").filter(|v| v.is_object()) {
            let start = e["from"].as_i64().unwrap_or(0);
            (
                en["type"].as_str().unwrap_or("").to_string(),
                en["code"].as_str().map(str::to_string).unwrap_or_else(|| en["code"].to_string()),
                en["name"].as_str().unwrap_or("").to_string(),
                en["attrs"]["country_code"].as_str().map(str::to_string),
                start,
                e["until"].as_i64().unwrap_or(start),
            )
        } else {
            let loc = e["location"].as_str().unwrap_or("");
            let Some((kind, ent)) = loc.split_once('/') else { continue };
            let start = e["start"].as_i64().unwrap_or(0);
            (kind.to_string(), ent.to_string(), e["location_name"].as_str().unwrap_or("").to_string(), None, start, start + e["duration"].as_i64().unwrap_or(0))
        };
        let source = e["datasource"].as_str().unwrap_or("");
        let (asn, country) = match kind.as_str() {
            "asn" => {
                let a = ent.parse::<u64>().ok();
                (a, a.map(country_of).filter(|c| !c.is_empty()))
            }
            "geoasn" => {
                let a = ent.split('-').next().and_then(|a| a.parse::<u64>().ok());
                (a, a.map(country_of).filter(|c| !c.is_empty()))
            }
            "country" => (None, Some(ent.clone())),
            "region" => (None, region_cc),
            _ => (None, None),
        };
        out.push(json!({
            "id": format!("{kind}/{ent}@{start}@{source}"), "kind": kind, "entity": ent, "asn": asn, "country": country,
            "name": name, "source": source, "start": iso(start), "until": iso(until),
            "score": e["score"].as_f64(), "open": until >= fetched - 20 * 60,
        }));
    }
    out
}
