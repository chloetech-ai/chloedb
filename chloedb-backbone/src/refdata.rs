//! What the feed knows before it reads an update, to say what the update
//! means as it arrives: RPKI's validated ROAs (is this origin allowed to
//! announce this prefix?), the origins RouteViews saw for each prefix
//! yesterday (is this origin the usual one?), CAIDA's relationships (is the
//! new origin a customer of the usual one, which makes it a delegation
//! rather than a hijack?), and each AS's name and country.
//!
//! All of it is read from the folder `fetch` fills.

use std::io::{BufRead, Read};
use std::path::Path;

use anyhow::{Context, Result};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::mrt::Prefix;
use crate::pmap::PrefixMap;

#[derive(Default)]
pub struct Reference {
    /// ROAs: (asn, max length) at each prefix.
    pub roas: PrefixMap<Vec<(u32, u8)>>,
    /// RouteViews' origins of each prefix (CAIDA's prefix2as).
    pub usual: PrefixMap<Vec<u32>>,
    /// provider -> customers, and customer -> providers (CAIDA as-rel2).
    pub customers: FxHashMap<u32, Vec<u32>>,
    pub providers: FxHashMap<u32, Vec<u32>>,
    pub peers: FxHashMap<u32, Vec<u32>>,
    /// An AS's name and country (RIPE's asn.txt, from the RIRs).
    pub names: FxHashMap<u32, (String, String)>,
    /// When the ROAs were made (rpki-client's buildtime).
    pub roas_built: String,
}

/// What one announcement means.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    /// `valid`, `invalid` (another AS's ROA covers it), `invalid-length`
    /// (this AS's ROA covers it, but not so long a prefix) or `unknown`
    /// (no ROA covers it).
    pub rpki: &'static str,
    /// `usual` (RouteViews saw this origin for it), `inside` (inside a
    /// prefix this origin or its provider announces), `related` (a new
    /// origin, a customer or provider of the usual one), `new-origin` (a
    /// new, unrelated origin for a prefix seen before), `more-specific` (a
    /// new, longer prefix inside someone else's) or `unseen` (no prefix
    /// covering it was seen yesterday).
    pub seen: &'static str,
    /// Whose space it is: the AS of the longest ROA covering it, else the
    /// usual origin of the longest prefix covering it, else the origin.
    pub owner: u32,
}

impl Reference {
    pub fn related(&self, a: u32, b: u32) -> bool {
        a == b
            || self.customers.get(&a).is_some_and(|c| c.contains(&b))
            || self.providers.get(&a).is_some_and(|p| p.contains(&b))
    }

    pub fn check(&self, p: &Prefix, origin: u32) -> Check {
        // RPKI route origin validation (RFC 6811).
        let mut covered = false;
        let mut asn_ok_but_long = false;
        let mut valid = false;
        let mut roa_owner = None;
        for (_, list) in self.roas.covering(p) {
            covered = true;
            for &(asn, max) in list {
                if roa_owner.is_none() && asn != 0 {
                    roa_owner = Some(asn);
                }
                if asn == origin && origin != 0 {
                    if p.len <= max {
                        valid = true;
                    } else {
                        asn_ok_but_long = true;
                    }
                }
            }
            if valid {
                break;
            }
        }
        let rpki = if valid {
            "valid"
        } else if !covered {
            "unknown"
        } else if asn_ok_but_long {
            "invalid-length"
        } else {
            "invalid"
        };
        // Against yesterday's origins.
        // The longest prefix that covers it decides.
        let mut seen = "unseen";
        let mut usual_owner = None;
        if let Some((q, list)) = self.usual.covering(p).next() {
            usual_owner = list.first().copied();
            let exact = q.len == p.len;
            seen = if list.contains(&origin) {
                if exact { "usual" } else { "inside" }
            } else {
                match (exact, list.iter().any(|&o| self.related(o, origin))) {
                    (true, true) => "related",
                    (true, false) => "new-origin",
                    (false, true) => "inside",
                    (false, false) => "more-specific",
                }
            };
        }
        Check { rpki, seen, owner: roa_owner.or(usual_owner).unwrap_or(origin) }
    }

    pub fn name(&self, asn: u32) -> &str {
        self.names.get(&asn).map(|n| n.0.as_str()).unwrap_or("")
    }

    pub fn country(&self, asn: u32) -> &str {
        self.names.get(&asn).map(|n| n.1.as_str()).unwrap_or("")
    }

    /// Everything `fetch` put in `dir` that the feed needs.
    pub fn load(dir: &Path) -> Result<Reference> {
        let mut r = Reference::default();
        r.load_roas(&dir.join("rpki/vrps.json"))?;
        for f in ["caida/pfx2as-v4.gz", "caida/pfx2as-v6.gz"] {
            let p = dir.join(f);
            if p.exists() {
                r.load_pfx2as(&p)?;
            }
        }
        r.load_rel(&dir.join("caida/as-rel2.txt.bz2"))?;
        r.load_names(&dir.join("ripe/asn.txt"))?;
        Ok(r)
    }

    pub fn load_roas(&mut self, path: &Path) -> Result<()> {
        let j: serde_json::Value = serde_json::from_reader(std::io::BufReader::new(
            std::fs::File::open(path).with_context(|| path.display().to_string())?,
        ))?;
        self.roas_built = j["metadata"]["buildtime"].as_str().unwrap_or("").to_string();
        for roa in j["roas"].as_array().into_iter().flatten() {
            let Some(p) = roa["prefix"].as_str().and_then(Prefix::parse) else { continue };
            let asn = roa["asn"].as_u64().unwrap_or(0) as u32;
            let max = roa["maxLength"].as_u64().unwrap_or(p.len as u64) as u8;
            let v = self.roas.entry(p, Vec::new);
            if !v.contains(&(asn, max)) {
                v.push((asn, max));
            }
        }
        Ok(())
    }

    /// CAIDA's prefix2as: `address<TAB>length<TAB>origins`, origins split by
    /// `_` (several seen) or `,` (an AS set).
    pub fn load_pfx2as(&mut self, path: &Path) -> Result<()> {
        let f = std::fs::File::open(path).with_context(|| path.display().to_string())?;
        let r = std::io::BufReader::new(flate2::read::MultiGzDecoder::new(std::io::BufReader::new(f)));
        for line in r.lines() {
            let line = line?;
            let mut it = line.split('\t');
            let (Some(a), Some(l), Some(o)) = (it.next(), it.next(), it.next()) else { continue };
            let Some(p) = Prefix::parse(&format!("{a}/{l}")) else { continue };
            let v = self.usual.entry(p, Vec::new);
            for x in o.split(['_', ',']) {
                if let Ok(asn) = x.parse::<u32>() {
                    if !v.contains(&asn) {
                        v.push(asn);
                    }
                }
            }
        }
        Ok(())
    }

    /// CAIDA's as-rel2: `a|b|-1|…` (a is b's provider) or `a|b|0|…` (peers).
    pub fn load_rel(&mut self, path: &Path) -> Result<()> {
        for (a, b, rel) in read_rel(path)? {
            if rel == -1 {
                self.customers.entry(a).or_default().push(b);
                self.providers.entry(b).or_default().push(a);
            } else {
                self.peers.entry(a).or_default().push(b);
                self.peers.entry(b).or_default().push(a);
            }
        }
        Ok(())
    }

    /// RIPE's asn.txt: `asn name..., CC`.
    pub fn load_names(&mut self, path: &Path) -> Result<()> {
        for (asn, name, cc) in read_names(path)? {
            self.names.insert(asn, (name, cc));
        }
        Ok(())
    }
}

pub fn read_rel(path: &Path) -> Result<Vec<(u32, u32, i32)>> {
    let mut raw = Vec::new();
    let f = std::fs::File::open(path).with_context(|| path.display().to_string())?;
    if path.extension().is_some_and(|e| e == "bz2") {
        bzip2_rs::DecoderReader::new(std::io::BufReader::new(f)).read_to_end(&mut raw)?;
    } else {
        std::io::BufReader::new(f).read_to_end(&mut raw)?;
    }
    let text = String::from_utf8_lossy(&raw);
    let mut out = Vec::new();
    let mut seen = FxHashSet::default();
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        let mut it = line.split('|');
        let (Some(a), Some(b), Some(r)) = (it.next(), it.next(), it.next()) else { continue };
        let (Ok(a), Ok(b), Ok(r)) = (a.parse::<u32>(), b.parse::<u32>(), r.parse::<i32>()) else { continue };
        if seen.insert((a, b)) {
            out.push((a, b, r));
        }
    }
    Ok(out)
}

pub fn read_names(path: &Path) -> Result<Vec<(u32, String, String)>> {
    let raw = std::fs::read(path).with_context(|| path.display().to_string())?;
    let text = String::from_utf8_lossy(&raw);
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((asn, rest)) = line.split_once(' ') else { continue };
        let Ok(asn) = asn.trim().parse::<u32>() else { continue };
        let (name, cc) = match rest.rsplit_once(", ") {
            Some((n, c)) if c.len() == 2 && c.chars().all(|c| c.is_ascii_uppercase()) => (n.trim().to_string(), c.to_string()),
            _ => (rest.trim().to_string(), String::new()),
        };
        out.push((asn, name, cc));
    }
    Ok(out)
}

/// A short name for an AS: "AS-VULTR - The Constant Company, LLC" reads
/// "The Constant Company"; "BEYON-AS Beyon B.S.C." reads "Beyon B.S.C.".
/// "Jan Kowalski trading as Example Net": the business's name.
pub fn trading_as(s: &str) -> &str {
    s.rsplit_once(" trading as ").map_or(s, |(_, b)| b.trim())
}

pub fn short_name(full: &str) -> String {
    let s = full.trim();
    let s = match s.split_once(" - ") {
        Some((handle, rest)) if !handle.contains(' ') && !rest.is_empty() => rest,
        _ => match s.split_once(' ') {
            Some((handle, rest))
                if handle.len() > 2
                    && handle.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-' || c == '_')
                    && handle.chars().any(|c| c.is_ascii_digit() || c == '-' || c == '_')
                    && !rest.is_empty() =>
            {
                rest
            }
            _ => s,
        },
    };
    let s = s.trim_end_matches(['.', ',']);
    let s = trading_as(s);
    let cut = [", Inc", ", LLC", ", Ltd", " Inc", " LLC", " Ltd", " S.A", " GmbH", " B.V", " B.S.C", " S.L", " Co", " Corporation", " Corp", " Limited", " PLC", " plc", " AG", " SA", " LTD", " Sp. z o.o", " INC", " CORP", " LIMITED"];
    let mut s = s.to_string();
    for c in cut {
        // A whole word: the end, or punctuation or a space after it.
        let at = s.match_indices(c).map(|(i, _)| i).find(|&i| i > 2 && s[i + c.len()..].chars().next().is_none_or(|n| matches!(n, ',' | '.' | ' ' | ')')));
        if let Some(i) = at {
            s.truncate(i);
        }
    }
    s.trim_end_matches([',', ' ', '.']).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r() -> Reference {
        let mut r = Reference::default();
        r.roas.entry(Prefix::parse("84.255.184.0/24").unwrap(), Vec::new).push((5416, 24));
        r.roas.entry(Prefix::parse("84.255.184.0/21").unwrap(), Vec::new).push((5416, 21));
        r.usual.entry(Prefix::parse("84.255.184.0/21").unwrap(), Vec::new).push(5416);
        r.usual.entry(Prefix::parse("45.71.74.0/24").unwrap(), Vec::new).push(266038);
        r.customers.entry(266038).or_default().push(267597);
        r.providers.entry(267597).or_default().push(266038);
        r
    }

    #[test]
    fn checks_an_announcement() {
        let r = r();
        let p = Prefix::parse("84.255.184.0/24").unwrap();
        assert_eq!(r.check(&p, 6453), Check { rpki: "invalid", seen: "more-specific", owner: 5416 });
        assert_eq!(r.check(&p, 5416), Check { rpki: "valid", seen: "inside", owner: 5416 });
        let long = Prefix::parse("84.255.186.0/23").unwrap();
        assert_eq!(r.check(&long, 5416).rpki, "invalid-length");
        let moas = Prefix::parse("45.71.74.0/24").unwrap();
        assert_eq!(r.check(&moas, 267597), Check { rpki: "unknown", seen: "related", owner: 266038 });
        assert_eq!(r.check(&moas, 64512).seen, "new-origin");
        assert_eq!(r.check(&Prefix::parse("192.0.2.0/24").unwrap(), 1).seen, "unseen");
    }

    #[test]
    fn short_names() {
        assert_eq!(short_name("AS-VULTR - The Constant Company, LLC"), "The Constant Company");
        assert_eq!(short_name("BEYON-AS Beyon B.S.C."), "Beyon");
        assert_eq!(short_name("BEYON B.S.C."), "BEYON");
        assert_eq!(short_name("AS6453 - TATA COMMUNICATIONS (AMERICA) INC"), "TATA COMMUNICATIONS (AMERICA)");
        assert_eq!(short_name("TWELVE99 Arelion Sweden AB"), "Arelion Sweden AB");
        assert_eq!(short_name("CLOUDFLARENET"), "CLOUDFLARENET");
        assert_eq!(trading_as("Maciej Jan Broniarz trading as CERT NGO"), "CERT NGO");
    }
}
