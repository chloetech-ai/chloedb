//! Shares: a slice of the personal space ([`crate::me`]) handed to someone
//! else, or to another model, with a key of its own.
//!
//! `chloectx me share create NAME` picks the records (the persona card's
//! lines in some sections, what answers some topics, and with `--sources`
//! the messages the lines came from), cuts them into a capsule
//! ([`crate::capsule`]: a read-only ChloeDB file of exactly those
//! records, redacted again on the way out) under `<me>/shares/<id>/`, signs
//! it with the space's Ed25519 key, and makes a key for it:
//! `ctxs_<id>_<secret>`. Only the key's SHA-256 is kept.
//!
//! The key opens that capsule and nothing else, read-only, through
//! `chloectx me serve` ([`crate::remote`]): `https://HOST/s/<key>/mcp` is an
//! MCP server (`persona`, `search`, `fetch`, `ask`) that a ChatGPT or
//! Claude connector with no sign-in, or any MCP client, can add; the key
//! is in the address, so a share is a link. It stops working when it
//! expires or is revoked (`chloectx me share revoke ID`, which also
//! removes the capsule), and every call is written to the share's
//! `access.log` (when, which tool, what was asked, which records went out,
//! from where), which `chloectx me share log ID` shows. With `--out DIR` the
//! signed capsule is also copied out, for `chloectx mount` elsewhere.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

use crate::{err, now, Result};

/// One share.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Share {
    pub id: String,
    pub name: String,
    pub created: i64,
    /// Unix seconds after which the key stops working; none: until revoked.
    #[serde(default)]
    pub expires: Option<i64>,
    #[serde(default)]
    pub revoked: Option<i64>,
    /// SHA-256 of the key, hex.
    pub key_hash: String,
    #[serde(default)]
    pub sections: Vec<String>,
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default)]
    pub sources: bool,
    pub records: u64,
    /// The capsule's SHA-256, and the signing key's fingerprint.
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub signer: Option<String>,
}

impl Share {
    /// Whether the key still opens it, at `t`; the reason when not.
    pub fn open_at(&self, t: i64) -> std::result::Result<(), &'static str> {
        if self.revoked.is_some() {
            return Err("this share was revoked");
        }
        if self.expires.is_some_and(|e| t >= e) {
            return Err("this share expired");
        }
        Ok(())
    }

    pub fn json(&self) -> Json {
        let state = match self.open_at(now()) {
            Ok(()) => "active",
            Err(r) if r.contains("revoked") => "revoked",
            Err(_) => "expired",
        };
        json!({"id": self.id, "name": self.name, "created": self.created, "expires": self.expires, "revoked": self.revoked, "state": state,
            "sections": self.sections, "topics": self.topics, "sources": self.sources, "records": self.records, "sha256": self.sha256, "signer": self.signer})
    }
}

/// The shares' folder: `<me>/shares`.
pub fn root() -> PathBuf {
    crate::me::dir().join("shares")
}

pub fn dir(id: &str) -> PathBuf {
    root().join(id)
}

/// The capsule a share serves.
pub fn capsule_dir(id: &str) -> PathBuf {
    dir(id).join("capsule")
}

fn registry() -> PathBuf {
    root().join("shares.json")
}

pub fn load() -> Vec<Share> {
    std::fs::read_to_string(registry()).ok().and_then(|s| serde_json::from_str::<Json>(&s).ok()).and_then(|v| serde_json::from_value(v["shares"].clone()).ok()).unwrap_or_default()
}

pub fn save(shares: &[Share]) -> Result<()> {
    crate::me::ensure()?;
    crate::paths::ensure_private_dir(&root())?;
    write_private(&registry(), &serde_json::to_string_pretty(&json!({"shares": shares}))?)
}

/// Writes `text` to `path`, readable by this user only, by a rename.
pub fn write_private(path: &std::path::Path, text: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    let mut f = std::fs::OpenOptions::new().create(true).truncate(true).write(true).mode(0o600).open(&tmp)?;
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// `n` random bytes, as base64url without padding.
pub fn random_b64(n: usize) -> String {
    let mut b = vec![0u8; n];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut b).expect("the system's random numbers");
    b64url(&b)
}

pub fn b64url(b: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(b.len() * 4 / 3 + 2);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        let k = c.len() + 1;
        for i in 0..k {
            out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

pub fn sha256_hex(s: &str) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// A new share's id and key.
pub fn new_key() -> (String, String) {
    let id: String = random_b64(6).chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { 'x' }).collect();
    let key = format!("ctxs_{id}_{}", random_b64(24));
    (id, key)
}

/// The share a key opens, if it is one of `shares`' (whatever its state).
pub fn by_key<'a>(shares: &'a [Share], key: &str) -> Option<&'a Share> {
    let id = key.strip_prefix("ctxs_")?.split('_').next()?;
    let h = sha256_hex(key);
    shares.iter().find(|s| s.id == id && crate::ui::same(&s.key_hash, &h))
}

/// `30d`, `12h`, `90m`, `2w` or `never` as seconds (`None` for never).
pub fn parse_duration(s: &str) -> Result<Option<i64>> {
    let s = s.trim().to_ascii_lowercase();
    if s == "never" || s == "none" || s.is_empty() {
        return Ok(None);
    }
    let (n, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()));
    let n: i64 = n.parse().map_err(|_| err(format!("{s:?} is not a duration such as 30d, 12h or never")))?;
    let mult = match unit {
        "" | "d" | "day" | "days" => 86_400,
        "h" | "hour" | "hours" => 3_600,
        "m" | "min" | "minutes" => 60,
        "w" | "week" | "weeks" => 7 * 86_400,
        _ => return Err(err(format!("{s:?} is not a duration such as 30d, 12h or never"))),
    };
    if n <= 0 {
        return Err(err("a share lasts a positive time"));
    }
    Ok(Some(n * mult))
}

/// Appends one call to a share's access log.
pub fn log(id: &str, entry: &Json) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let p = dir(id).join("access.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).mode(0o600).open(&p) {
        let _ = writeln!(f, "{entry}");
    }
}

/// The last `n` calls of a share's access log, oldest first.
pub fn read_log(id: &str, n: usize) -> Vec<Json> {
    let text = std::fs::read_to_string(dir(id).join("access.log")).unwrap_or_default();
    let all: Vec<Json> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    all[all.len().saturating_sub(n)..].to_vec()
}

/// The personal space's signing key (PKCS#8 Ed25519), made on first use,
/// readable by this user only.
pub fn signing_key() -> Result<Vec<u8>> {
    let p = crate::me::ensure()?.join("signing.pk8");
    if let Ok(b) = std::fs::read(&p) {
        return Ok(b);
    }
    let doc = ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).map_err(|e| err(format!("a signing key: {e}")))?;
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(&p)?;
    f.write_all(doc.as_ref())?;
    Ok(doc.as_ref().to_vec())
}

/// The signing key's public half (hex) and fingerprint.
pub fn public_key() -> Result<(String, String)> {
    use ring::signature::KeyPair;
    let k = signing_key()?;
    let kp = ring::signature::Ed25519KeyPair::from_pkcs8(&k).map_err(|e| err(format!("the signing key: {e}")))?;
    let hex: String = kp.public_key().as_ref().iter().map(|b| format!("{b:02x}")).collect();
    let fp = crate::capsule::fingerprint(&hex);
    Ok((hex, fp))
}

/// What to put in a new share.
pub struct CreateOptions {
    pub name: String,
    pub sections: Vec<String>,
    pub topics: Vec<String>,
    pub card: bool,
    pub sources: bool,
    pub per_topic: usize,
    pub expires_in: Option<i64>,
    /// Also copy the signed capsule here.
    pub out: Option<PathBuf>,
}

/// Makes a share: picks the records, cuts and signs the capsule, keeps the
/// key's hash. Returns the share and its key (shown once).
pub fn create(o: &CreateOptions) -> Result<(Share, String)> {
    let name = crate::capsule::clean_text(&o.name, 80);
    if name.is_empty() {
        return Err(err("a share needs a name, such as \"diet and travel\""));
    }
    let picked = crate::me::ask("pick", json!({"sections": o.sections, "topics": o.topics, "card": o.card, "sources": o.sources, "per_topic": o.per_topic}), 120)?;
    let ids = picked["ids"].clone();
    if ids.as_array().is_none_or(|a| a.is_empty()) {
        return Err(err("nothing to share: no line of the card is in those sections and nothing answers those topics (see `chloectx me card`)"));
    }
    let (id, key) = new_key();
    crate::paths::ensure_private_dir(&root())?;
    crate::paths::ensure_private_dir(&dir(&id))?;
    let cap = capsule_dir(&id);
    let m = crate::me::request(&json!({"op": "me_pack", "ids": ids, "out": cap, "title": name}), 600)?;
    let fp = crate::capsule::sign(&cap, &signing_key()?)?;
    let share = Share {
        id: id.clone(),
        name,
        created: now(),
        expires: o.expires_in.map(|s| now() + s),
        revoked: None,
        key_hash: sha256_hex(&key),
        sections: o.sections.clone(),
        topics: o.topics.clone(),
        sources: o.sources,
        records: m["records"].as_u64().unwrap_or(0),
        sha256: m["sha256"].as_str().unwrap_or("").to_string(),
        signer: Some(fp),
    };
    if let Some(out) = &o.out {
        if out.exists() {
            return Err(err(format!("{} exists", out.display())));
        }
        std::fs::create_dir_all(out)?;
        for f in ["memory.strata", "manifest.json"] {
            std::fs::copy(cap.join(f), out.join(f))?;
        }
    }
    let mut all = load();
    all.push(share.clone());
    save(&all)?;
    Ok((share, key))
}

/// Revokes a share (by id or name) and removes its capsule; its log stays.
pub fn revoke(which: &str) -> Result<Share> {
    let mut all = load();
    let s = all.iter_mut().find(|s| s.id == which || s.name == which).ok_or_else(|| err(format!("no share {which:?} (chloectx me share list)")))?;
    if s.revoked.is_none() {
        s.revoked = Some(now());
    }
    let out = s.clone();
    let _ = std::fs::remove_dir_all(capsule_dir(&out.id));
    save(&all)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_durations_and_state() {
        let (id, key) = new_key();
        assert!(key.starts_with(&format!("ctxs_{id}_")) && key.len() > 40, "{key}");
        let s = Share { id: id.clone(), name: "t".into(), created: 0, expires: Some(100), revoked: None, key_hash: sha256_hex(&key), sections: vec![], topics: vec![], sources: false, records: 1, sha256: String::new(), signer: None };
        let all = vec![s.clone()];
        assert!(by_key(&all, &key).is_some());
        assert!(by_key(&all, &format!("{key}x")).is_none());
        assert!(by_key(&all, "ctxs_nope_x").is_none());
        assert_eq!(s.open_at(99), Ok(()));
        assert_eq!(s.open_at(100), Err("this share expired"));
        assert_eq!(parse_duration("30d").unwrap(), Some(30 * 86_400));
        assert_eq!(parse_duration("12h").unwrap(), Some(12 * 3_600));
        assert_eq!(parse_duration("never").unwrap(), None);
        assert!(parse_duration("soon").is_err());
        assert_eq!(b64url(b"\xff\xff\xff"), "____");
        assert_eq!(b64url(b"ab"), "YWI");
    }
}
