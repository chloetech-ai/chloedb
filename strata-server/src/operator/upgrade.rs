//! New versions of Strata: a cluster follows an image tag (`spec.upgrade`,
//! `ghcr.io/chloetech-ai/strata-server:latest` by default), and when the
//! tag points to another image than the one its servers run, the operator
//! suggests it (`suggest`) or rolls it out (`on`), as the image's digest
//! (`repository@sha256:...`), so that every server runs the same bits even
//! when one restarts after the tag moved on.
//!
//! The operator asks the image's registry (the OCI distribution API, with
//! an anonymous token when it asks for one) every `checkMinutes`: the
//! digest the tag names, and the commit it was built from (the image's
//! `org.opencontainers.image.revision` label) and when. What it found is
//! `status.upgrade`. The rollout itself is any other change of the image
//! ([`super::reconcile`]): tried on every node first, one server at a time,
//! verified, and rolled back when it fails a check; an image rolled back so
//! is not suggested or rolled out again (`rejected`), though a person may
//! still ask for it.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

pub use super::autoscale::Mode;

/// `spec.upgrade`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Upgrade {
    /// Off: no registry is asked. Suggest: a new image waits in the
    /// status for a person. On: the operator rolls it out once the
    /// cluster is ready.
    pub mode: Mode,
    /// The image tag to follow (default: `spec.image`'s repository, at its
    /// tag, or at `latest` when it names a digest).
    pub track: Option<String>,
    /// Minutes between two looks at the registry.
    pub check_minutes: u64,
}

impl Default for Upgrade {
    fn default() -> Upgrade {
        Upgrade { mode: Mode::Off, track: None, check_minutes: 10 }
    }
}

impl Upgrade {
    pub fn check(&self, image: &str) -> Result<(), String> {
        if self.mode != Mode::Off && self.check_minutes == 0 {
            return Err("upgrade.checkMinutes is 1 or more".into());
        }
        let track = self.tracked(image);
        let r = Ref::parse(&track).map_err(|e| format!("upgrade.track: {e}"))?;
        if r.tag.is_none() {
            return Err(format!("upgrade.track names a tag to follow (repository:tag), not a digest: {track}"));
        }
        Ok(())
    }

    /// The tag followed for a cluster whose image is `image`.
    pub fn tracked(&self, image: &str) -> String {
        if let Some(t) = self.track.as_deref().filter(|t| !t.is_empty()) {
            return t.to_string();
        }
        match Ref::parse(image) {
            Ok(r) => format!("{}:{}", r.repository(), r.tag.as_deref().filter(|_| r.digest.is_none()).unwrap_or("latest")),
            Err(_) => image.to_string(),
        }
    }

    pub fn said(&self) -> String {
        match self.mode {
            Mode::Off => "upgrades off".into(),
            Mode::Suggest => format!("a new image is suggested, every {} min", self.check_minutes),
            Mode::On => format!("a new image is rolled out, every {} min", self.check_minutes),
        }
    }
}

/// An image reference: `[registry/]path[:tag][@digest]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Ref {
    /// The registry's host (and port), as the reference names it, or
    /// `docker.io`.
    pub registry: String,
    pub path: String,
    pub tag: Option<String>,
    pub digest: Option<String>,
}

impl Ref {
    pub fn parse(s: &str) -> Result<Ref, String> {
        let s = s.trim();
        if s.is_empty() || s.contains(char::is_whitespace) {
            return Err(format!("{s:?} is not an image reference"));
        }
        let (rest, digest) = match s.split_once('@') {
            Some((r, d)) if d.starts_with("sha256:") && d.len() == 71 => (r, Some(d.to_string())),
            Some(_) => return Err(format!("{s}: a digest is sha256: and 64 hex digits")),
            None => (s, None),
        };
        let (registry, path) = match rest.split_once('/') {
            Some((h, p)) if h.contains('.') || h.contains(':') || h == "localhost" => (h.to_string(), p.to_string()),
            _ => ("docker.io".to_string(), rest.to_string()),
        };
        // A tag follows the last colon of the path.
        let (path, tag) = match path.rsplit_once(':') {
            Some((p, t)) if !t.contains('/') => (p.to_string(), Some(t.to_string())),
            _ => (path, None),
        };
        let path = if registry == "docker.io" && !path.contains('/') { format!("library/{path}") } else { path };
        if path.is_empty() {
            return Err(format!("{s}: no repository"));
        }
        let tag = if tag.is_none() && digest.is_none() { Some("latest".to_string()) } else { tag };
        Ok(Ref { registry, path, tag, digest })
    }

    /// `registry/path`.
    pub fn repository(&self) -> String {
        format!("{}/{}", self.registry, self.path)
    }

    /// The reference by digest: `registry/path@digest`.
    pub fn pinned(&self, digest: &str) -> String {
        format!("{}@{digest}", self.repository())
    }

    /// The base URL of the registry's API.
    fn api(&self) -> String {
        let host = if self.registry == "docker.io" { "registry-1.docker.io" } else { &self.registry };
        // A registry on this machine (a test's) speaks plain HTTP.
        let plain = host.starts_with("localhost") || host.starts_with("127.");
        format!("{}://{host}/v2/{}", if plain { "http" } else { "https" }, self.path)
    }
}

/// An image the registry has: its digest, and the commit and time it was
/// built from, when its labels say.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Image {
    /// `registry/path@sha256:...`.
    pub image: String,
    pub digest: String,
    pub revision: Option<String>,
    pub created: Option<String>,
}

const MANIFESTS: &str = "application/vnd.oci.image.index.v1+json, application/vnd.docker.distribution.manifest.list.v2+json, application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.v2+json";

/// A registry's client: the OCI distribution API, with the anonymous
/// token a registry asks for (`WWW-Authenticate: Bearer realm=...`).
pub struct Registry {
    http: reqwest::Client,
}

impl Default for Registry {
    fn default() -> Registry {
        Registry::new()
    }
}

impl Registry {
    pub fn new() -> Registry {
        Registry { http: reqwest::Client::builder().timeout(Duration::from_secs(20)).build().expect("an HTTP client") }
    }

    /// GETs `url`, with a token when the registry asks for one.
    async fn get(&self, url: &str, accept: &str, token: &mut Option<String>) -> Result<reqwest::Response, String> {
        for _ in 0..2 {
            let mut req = self.http.get(url).header(reqwest::header::ACCEPT, accept);
            if let Some(t) = token.as_ref() {
                req = req.bearer_auth(t);
            }
            let a = req.send().await.map_err(|e| format!("GET {url}: {e}"))?;
            if a.status() == reqwest::StatusCode::UNAUTHORIZED && token.is_none() {
                let challenge = a.headers().get(reqwest::header::WWW_AUTHENTICATE).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
                *token = Some(self.token(&challenge).await?);
                continue;
            }
            if !a.status().is_success() {
                let status = a.status();
                let body = a.text().await.unwrap_or_default();
                return Err(format!("GET {url}: {status}: {}", body.chars().take(300).collect::<String>()));
            }
            return Ok(a);
        }
        Err(format!("GET {url}: the registry refused its own token"))
    }

    /// The anonymous token a `Bearer realm="...",service="...",scope="..."`
    /// challenge leads to.
    async fn token(&self, challenge: &str) -> Result<String, String> {
        let params = bearer(challenge).ok_or_else(|| format!("the registry asks for a sign-in this operator does not have ({challenge:?}); make the image public"))?;
        let realm = params.iter().find(|(k, _)| k == "realm").map(|(_, v)| v.clone()).ok_or("the registry's challenge names no realm")?;
        let query: Vec<(String, String)> = params.into_iter().filter(|(k, _)| k == "service" || k == "scope").collect();
        let a = self.http.get(&realm).query(&query).send().await.map_err(|e| format!("GET {realm}: {e}"))?;
        if !a.status().is_success() {
            return Err(format!("GET {realm}: {}: the image may be private", a.status()));
        }
        let j: Json = a.json().await.map_err(|e| format!("{realm}: {e}"))?;
        j["token"].as_str().or_else(|| j["access_token"].as_str()).map(str::to_string).ok_or_else(|| format!("{realm} answered no token"))
    }

    /// The image `reference` names now (a tag, or a digest), with its
    /// labels' revision and creation time.
    pub async fn resolve(&self, reference: &str) -> Result<Image, String> {
        let r = Ref::parse(reference)?;
        let api = r.api();
        let at = r.digest.clone().or_else(|| r.tag.clone()).unwrap_or_else(|| "latest".into());
        let mut token = None;
        let a = self.get(&format!("{api}/manifests/{at}"), MANIFESTS, &mut token).await?;
        let header = a.headers().get("docker-content-digest").and_then(|v| v.to_str().ok()).map(str::to_string);
        let body = a.bytes().await.map_err(|e| format!("{reference}: {e}"))?;
        let digest = header.unwrap_or_else(|| {
            use sha2::{Digest, Sha256};
            format!("sha256:{}", Sha256::digest(&body).iter().map(|b| format!("{b:02x}")).collect::<String>())
        });
        let mut manifest: Json = serde_json::from_slice(&body).map_err(|e| format!("{reference}: the manifest: {e}"))?;
        // An index of images, one an architecture: the labels are the same
        // on each, so read one (amd64's, when there is one).
        if let Some(ms) = manifest["manifests"].as_array() {
            let one = ms.iter().find(|m| m["platform"]["architecture"] == "amd64" && m["platform"]["os"] == "linux").or_else(|| ms.first());
            if let Some(d) = one.and_then(|m| m["digest"].as_str()) {
                let a = self.get(&format!("{api}/manifests/{d}"), MANIFESTS, &mut token).await?;
                manifest = a.json().await.map_err(|e| format!("{reference}: the manifest of {d}: {e}"))?;
            }
        }
        let (mut revision, mut created) = (None, None);
        if let Some(c) = manifest["config"]["digest"].as_str() {
            let a = self.get(&format!("{api}/blobs/{c}"), "application/json, */*", &mut token).await?;
            let config: Json = a.json().await.map_err(|e| format!("{reference}: the image's config: {e}"))?;
            revision = config["config"]["Labels"]["org.opencontainers.image.revision"].as_str().map(str::to_string);
            created = config["created"].as_str().map(str::to_string);
        }
        Ok(Image { image: r.pinned(&digest), digest, revision, created })
    }
}

/// The parameters of a `Bearer` challenge.
fn bearer(challenge: &str) -> Option<Vec<(String, String)>> {
    let rest = challenge.trim().strip_prefix("Bearer ").or_else(|| challenge.trim().strip_prefix("bearer "))?;
    let mut out = Vec::new();
    let mut s = rest.trim();
    while !s.is_empty() {
        let (k, v) = s.split_once('=')?;
        let k = k.trim().trim_start_matches(',').trim().to_string();
        let v = v.trim_start();
        let (value, after) = match v.strip_prefix('"') {
            Some(q) => {
                let end = q.find('"')?;
                (q[..end].to_string(), &q[end + 1..])
            }
            None => {
                let end = v.find(',').unwrap_or(v.len());
                (v[..end].trim().to_string(), &v[end..])
            }
        };
        out.push((k, value));
        s = after.trim_start().trim_start_matches(',').trim_start();
    }
    Some(out)
}

/// What the operator found of new versions (`status.upgrade`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct State {
    pub mode: Mode,
    /// The tag followed.
    pub track: String,
    /// When the registry was last asked, and what it said.
    pub checked_ms: Option<u64>,
    pub error: Option<String>,
    /// The image the tag names.
    pub latest: Option<Image>,
    /// The image the cluster asks its servers to run, when it names one
    /// by digest, with what the registry says of it.
    pub current: Option<Image>,
    /// Each image the servers run (by the digest their pods pulled), and
    /// how many run it.
    pub running: Vec<Running>,
    /// The tag names another image than the cluster runs, not rejected.
    pub available: bool,
    /// Images rolled back after they failed a check: not suggested or
    /// rolled out again.
    pub rejected: Vec<Rejected>,
    /// The latest upgrades made or suggested, the newest last.
    pub log: Vec<super::autoscale::Entry>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Running {
    pub digest: String,
    pub servers: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Rejected {
    pub digest: String,
    pub at_ms: u64,
    pub why: String,
}

const LOG: usize = 30;

impl State {
    pub fn note(&mut self, at_ms: u64, text: impl Into<String>) {
        self.log.push(super::autoscale::Entry { at_ms, text: text.into() });
        let over = self.log.len().saturating_sub(LOG);
        self.log.drain(..over);
    }

    /// Notes that `digest` failed a check and was rolled back.
    pub fn reject(&mut self, digest: &str, at_ms: u64, why: &str) {
        if !self.rejected.iter().any(|r| r.digest == digest) {
            self.rejected.push(Rejected { digest: digest.into(), at_ms, why: why.into() });
            let over = self.rejected.len().saturating_sub(20);
            self.rejected.drain(..over);
        }
        self.available = false;
        self.note(at_ms, format!("{} failed and was rolled back: {why}", short(digest)));
    }

    /// Whether the registry is due to be asked at `now`.
    pub fn due(&self, u: &Upgrade, track: &str, now: u64) -> bool {
        u.mode != Mode::Off && (self.track != track || self.checked_ms.is_none_or(|t| now.saturating_sub(t) >= u.check_minutes.saturating_mul(60_000)))
    }
}

/// The digest an image reference pins, if it does.
pub fn digest_of(image: &str) -> Option<String> {
    image.split_once('@').map(|(_, d)| d.to_string()).filter(|d| d.starts_with("sha256:"))
}

/// A digest as people read it: its first 12 hex digits.
pub fn short(digest: &str) -> String {
    digest.trim_start_matches("sha256:").chars().take(12).collect()
}

/// An image in words: its revision's first 7 digits and its digest's
/// first 12.
pub fn said(i: &Image) -> String {
    match &i.revision {
        Some(r) => format!("{} (commit {})", short(&i.digest), r.chars().take(7).collect::<String>()),
        None => short(&i.digest),
    }
}

/// The image to roll out, if the tag names one the cluster does not run:
/// with `on`, once the cluster is ready (a change under way, or a version
/// that failed, waits); never one rejected. `image` is the spec's,
/// `running` the digests its servers run.
pub fn decide(u: &Upgrade, s: &State, image: &str, ready: bool) -> Option<String> {
    let latest = s.latest.as_ref()?;
    if !s.available || u.mode != Mode::On || !ready || image == latest.image {
        return None;
    }
    Some(latest.image.clone())
}

/// Whether the tag's image is new to the cluster: not the one its spec
/// pins, not the only one its servers run, and not rejected.
pub fn available(s: &State, image: &str) -> bool {
    let Some(latest) = &s.latest else { return false };
    let pinned = digest_of(image);
    let runs_it = match &pinned {
        Some(d) => *d == latest.digest,
        // A tag: what its servers pulled.
        None => !s.running.is_empty() && s.running.iter().all(|r| r.digest == latest.digest),
    };
    !runs_it && !s.rejected.iter().any(|r| r.digest == latest.digest)
}

/// The JSON Schema of `spec.upgrade`, for the resource definition.
pub fn schema() -> Json {
    json!({
        "type": "object",
        "description": "Following an image tag for new versions of Strata: a new image is suggested (suggest) or rolled out (on), by its digest.",
        "properties": {
            "mode": {"type": "string", "enum": ["off", "suggest", "on"], "description": "off (default): no registry is asked; suggest: a new image waits in status.upgrade for a person; on: the operator rolls it out once the cluster is ready."},
            "track": {"type": "string", "description": "The image tag to follow (default: the image's repository at its tag, or at latest)."},
            "checkMinutes": {"type": "integer", "minimum": 1, "description": "Minutes between two looks at the registry (default 10)."},
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_are_read_as_registries_name_them() {
        let r = Ref::parse("ghcr.io/chloetech-ai/strata-server:latest").unwrap();
        assert_eq!((r.registry.as_str(), r.path.as_str(), r.tag.as_deref(), r.digest.as_deref()), ("ghcr.io", "chloetech-ai/strata-server", Some("latest"), None));
        assert_eq!(r.api(), "https://ghcr.io/v2/chloetech-ai/strata-server");
        let d = format!("sha256:{}", "a".repeat(64));
        let r = Ref::parse(&format!("ghcr.io/x/y@{d}")).unwrap();
        assert_eq!((r.tag.as_deref(), r.digest.as_deref(), r.pinned("sha256:b").as_str()), (None, Some(d.as_str()), "ghcr.io/x/y@sha256:b"));
        let r = Ref::parse("strata-server").unwrap();
        assert_eq!((r.registry.as_str(), r.path.as_str(), r.tag.as_deref()), ("docker.io", "library/strata-server", Some("latest")));
        assert_eq!(r.api(), "https://registry-1.docker.io/v2/library/strata-server");
        let r = Ref::parse("localhost:5000/s:v2").unwrap();
        assert_eq!((r.registry.as_str(), r.path.as_str(), r.tag.as_deref(), r.api().as_str()), ("localhost:5000", "s", Some("v2"), "http://localhost:5000/v2/s"));
        assert!(Ref::parse("x@sha256:short").is_err());
        assert!(Ref::parse("").is_err());
    }

    #[test]
    fn the_tag_followed_is_the_images_unless_named() {
        let u = Upgrade::default();
        assert_eq!(u.tracked("ghcr.io/chloetech-ai/strata-server:latest"), "ghcr.io/chloetech-ai/strata-server:latest");
        assert_eq!(u.tracked("ghcr.io/chloetech-ai/strata-server:v1"), "ghcr.io/chloetech-ai/strata-server:v1");
        assert_eq!(u.tracked(&format!("ghcr.io/chloetech-ai/strata-server@sha256:{}", "c".repeat(64))), "ghcr.io/chloetech-ai/strata-server:latest");
        let u = Upgrade { track: Some("ghcr.io/x/y:main".into()), ..u };
        assert_eq!(u.tracked("anything:1"), "ghcr.io/x/y:main");
        assert!(Upgrade { mode: Mode::On, track: Some(format!("ghcr.io/x/y@sha256:{}", "c".repeat(64))), ..Upgrade::default() }.check("i").unwrap_err().contains("names a tag"));
    }

    #[test]
    fn a_bearer_challenge_is_read() {
        let p = bearer(r#"Bearer realm="https://ghcr.io/token",service="ghcr.io",scope="repository:chloetech-ai/strata-server:pull""#).unwrap();
        assert_eq!(p, vec![("realm".into(), "https://ghcr.io/token".into()), ("service".into(), "ghcr.io".into()), ("scope".into(), "repository:chloetech-ai/strata-server:pull".into())]);
        assert!(bearer("Basic realm=x").is_none());
    }

    #[test]
    fn a_new_image_is_rolled_out_once_ready_and_never_one_rejected() {
        let d = |c: char| format!("sha256:{}", c.to_string().repeat(64));
        let latest = Image { image: format!("ghcr.io/x/y@{}", d('b')), digest: d('b'), revision: Some("abcdef0123".into()), created: None };
        let mut s = State { latest: Some(latest.clone()), ..State::default() };
        let on = Upgrade { mode: Mode::On, ..Upgrade::default() };
        // Pinned to another digest: the tag's is new.
        let pinned = format!("ghcr.io/x/y@{}", d('a'));
        s.available = available(&s, &pinned);
        assert!(s.available);
        assert_eq!(decide(&on, &s, &pinned, true), Some(latest.image.clone()));
        assert_eq!(decide(&on, &s, &pinned, false), None, "a change under way first");
        assert_eq!(decide(&Upgrade { mode: Mode::Suggest, ..on.clone() }, &s, &pinned, true), None, "a person applies a suggestion");
        // A tag: what the servers pulled.
        s.running = vec![Running { digest: d('b'), servers: 9 }];
        assert!(!available(&s, "ghcr.io/x/y:latest"));
        s.running.push(Running { digest: d('a'), servers: 1 });
        assert!(available(&s, "ghcr.io/x/y:latest"));
        // Rolled back once, never again.
        s.reject(&d('b'), 5, "c-1 cannot run the new settings");
        assert!(!s.available && !available(&s, &pinned));
        assert_eq!(decide(&on, &s, &pinned, true), None);
        assert!(s.log.last().unwrap().text.starts_with("bbbbbbbbbbbb failed and was rolled back"));
        assert_eq!(said(&latest), "bbbbbbbbbbbb (commit abcdef0)");
    }

    #[test]
    fn the_registry_is_asked_every_few_minutes() {
        let u = Upgrade { mode: Mode::Suggest, check_minutes: 10, ..Upgrade::default() };
        let mut s = State::default();
        assert!(s.due(&u, "t", 0));
        s.track = "t".into();
        s.checked_ms = Some(1_000);
        assert!(!s.due(&u, "t", 1_000 + 599_999));
        assert!(s.due(&u, "t", 1_000 + 600_000));
        assert!(s.due(&u, "other", 2_000), "another tag at once");
        assert!(!s.due(&Upgrade::default(), "other", 2_000), "off");
    }
}
