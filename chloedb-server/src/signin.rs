//! Signing in to the command center with a Google account: from any
//! browser, a phone's too, with no key to paste and no gcloud. The page
//! shows Google's own button (Google Identity Services), Google hands the
//! page an ID token for this server's OAuth client, and the server checks
//! it here: signed by one of Google's keys (RS256, [`CERTS_URL`]), for this
//! client (`aud`), from Google (`iss`), not expired, the address verified,
//! carrying the nonce this server gave the page in a cookie of its own
//! ([`NONCE_COOKIE`]), and the address on the server's allow list. The
//! server then makes the person a session ([`crate::keys::Keys::create_session`])
//! and sets it in a cookie the page's scripts cannot read
//! ([`SESSION_COOKIE`]: `__Host-`, `Secure`, `HttpOnly`, `SameSite=Strict`).
//!
//! No client secret is involved: an ID token needs only the client's id,
//! which is public. Google is never asked for anything of the person's
//! beyond who they are (the button's scopes are `openid email profile`),
//! and the server keeps nothing of the ID token.
//!
//! The allow list is the server's (`--google-allow`, the operator's
//! `spec.googleSignIn`): an address, or `@domain` for a Google Workspace
//! domain (the token's `hd` must name it, so a Gmail address of that name
//! is not one of it), each with a scope (`read`, `write` or `admin`).
//! Each request of a session is checked against the list as it is then,
//! so that a person taken off it, or given a lower scope, is so at once.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{json, Value as Json};

use crate::keys::Scope;

/// The cookie a person's session is kept in. `__Host-`: set by this host
/// alone, over HTTPS (or on loopback), for every path, and no other site
/// or subdomain can set or read it.
pub const SESSION_COOKIE: &str = "__Host-chloedb-session";

/// The cookie the nonce of a sign-in under way is kept in, until Google's
/// token comes back with it.
pub const NONCE_COOKIE: &str = "__Host-chloedb-nonce";

/// Where Google publishes the keys it signs ID tokens with.
pub const CERTS_URL: &str = "https://www.googleapis.com/oauth2/v3/certs";

/// Google Identity Services, the script that draws Google's button.
pub const SCRIPT: &str = "https://accounts.google.com/gsi/client";

/// The issuers Google's ID tokens name.
const ISSUERS: [&str; 2] = ["accounts.google.com", "https://accounts.google.com"];

/// How far a token's times may be off this server's clock.
const SKEW_S: u64 = 60;

/// How long a sign-in has from the nonce to Google's token.
pub const NONCE_S: u64 = 600;

/// How long Google's keys are kept when its answer says no `max-age`, and
/// how soon an unknown key id may fetch them again.
const CERTS_DEFAULT: Duration = Duration::from_secs(3600);
const CERTS_AGAIN: Duration = Duration::from_secs(30);

/// Addresses of these domains are anyone's: no `@domain` entry for them.
const PUBLIC_DOMAINS: [&str; 2] = ["gmail.com", "googlemail.com"];

/// Who an entry of the allow list lets in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Who {
    /// One address.
    Person(String),
    /// Every verified address of a Google Workspace domain.
    Domain(String),
}

/// An entry of the allow list: who, at what scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Allowed {
    pub who: Who,
    pub scope: Scope,
}

impl Allowed {
    /// `name@example.com`, `name@example.com=admin`, `@example.com=read`
    /// (scope `read` when none is given).
    pub fn parse(s: &str) -> Result<Allowed, String> {
        let s = s.trim();
        let (who, scope) = match s.rsplit_once('=') {
            Some((w, sc)) => (w.trim(), Scope::parse(sc.trim()).filter(|s| *s >= Scope::Read).ok_or_else(|| format!("{s:?}: the scope after = is read, write or admin"))?),
            None => (s, Scope::Read),
        };
        let who = who.to_ascii_lowercase();
        let who = match who.strip_prefix('@') {
            Some(d) => {
                if !domain_ok(d) {
                    return Err(format!("{s:?}: @ and a domain name (example.com)"));
                }
                if PUBLIC_DOMAINS.contains(&d) {
                    return Err(format!("{s:?}: anyone can have an address at {d}; name the person's address instead"));
                }
                Who::Domain(d.to_string())
            }
            None => match who.split_once('@') {
                Some((local, d)) if !local.is_empty() && local.len() <= 64 && domain_ok(d) && !local.chars().any(|c| c.is_whitespace() || c.is_control() || c == '@') => Who::Person(who.clone()),
                _ => return Err(format!("{s:?}: an address (name@example.com), or @ and a Workspace domain (@example.com), then =read, =write or =admin")),
            },
        };
        Ok(Allowed { who, scope })
    }

    pub fn json(&self) -> Json {
        match &self.who {
            Who::Person(p) => json!({"email": p, "scope": self.scope.name()}),
            Who::Domain(d) => json!({"domain": d, "scope": self.scope.name()}),
        }
    }
}

fn domain_ok(d: &str) -> bool {
    d.len() <= 253 && d.contains('.') && !d.starts_with('.') && !d.ends_with('.') && !d.contains("..") && d.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
}

/// The scope `allow` gives the address `email`: its own entry's, else its
/// domain's when the person is an account of that Workspace domain. At a
/// sign-in, `hd` is `Some` of the token's `hd`, which must name the
/// domain (an account with none is no Workspace's, whatever its address);
/// a session's later checks pass `None` and take the domain of the
/// address, which its sign-in matched to `hd`.
pub fn scope_of(allow: &[Allowed], email: &str, hd: Option<Option<&str>>) -> Option<Scope> {
    let email = email.trim().to_ascii_lowercase();
    if let Some(a) = allow.iter().find(|a| a.who == Who::Person(email.clone())) {
        return Some(a.scope);
    }
    let domain = email.rsplit_once('@').map(|(_, d)| d)?;
    let of_domain = |d: &str| match hd {
        None => true,
        Some(h) => h.is_some_and(|h| h.eq_ignore_ascii_case(d)),
    };
    allow.iter().filter(|a| matches!(&a.who, Who::Domain(d) if d == domain && of_domain(d))).map(|a| a.scope).max()
}

/// Who Google's token says signed in, and the scope the allow list gives
/// them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub email: String,
    pub name: Option<String>,
    pub scope: Scope,
}

/// Why a sign-in is refused: the status (401 for a token that is not
/// Google's or not for this server, 403 for a person not on the list,
/// 502 when Google's keys cannot be read) and what to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    pub status: u16,
    pub message: String,
}

fn refused(status: u16, message: impl Into<String>) -> Refused {
    Refused { status, message: message.into() }
}

/// One of Google's signing keys.
struct Jwk {
    kid: String,
    n: Vec<u8>,
    e: Vec<u8>,
}

#[derive(Default)]
struct Certs {
    keys: Vec<Jwk>,
    until: Option<Instant>,
    fetched: Option<Instant>,
}

/// Google sign-in for this server: its OAuth client's id and its allow
/// list.
pub struct Google {
    client_id: String,
    allow: Vec<Allowed>,
    certs_url: String,
    certs: tokio::sync::Mutex<()>,
    cache: Mutex<Certs>,
    http: reqwest::Client,
}

// Printed without the cached keys or the client: only what says which
// Google sign-in this is.
impl std::fmt::Debug for Google {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Google").field("client_id", &self.client_id).field("allow", &self.allow.len()).finish()
    }
}

impl Google {
    /// For the OAuth client `client_id` (`....apps.googleusercontent.com`)
    /// and the allow list's entries ([`Allowed::parse`]), one at least.
    pub fn new(client_id: &str, allow: &[String]) -> Result<Google, String> {
        let client_id = client_id.trim();
        let ok = client_id.ends_with(".apps.googleusercontent.com") && client_id.len() < 200 && client_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_');
        if !ok {
            return Err(format!("--google-client-id {client_id:?}: the client id of an OAuth client of type Web application, NUMBER-....apps.googleusercontent.com (Google Cloud console, Google Auth Platform, Clients)"));
        }
        let allow = allow.iter().map(|a| Allowed::parse(a).map_err(|e| format!("--google-allow {e}"))).collect::<Result<Vec<_>, _>>()?;
        if allow.is_empty() {
            return Err("--google-client-id needs --google-allow: who may sign in (name@example.com=admin, @example.com=read)".into());
        }
        let http = crate::http_client().timeout(Duration::from_secs(10)).build().map_err(|e| e.to_string())?;
        Ok(Google { client_id: client_id.to_string(), allow, certs_url: CERTS_URL.into(), certs: tokio::sync::Mutex::new(()), cache: Mutex::new(Certs::default()), http })
    }

    /// The same, with Google's keys read from `url` (tests).
    pub fn with_certs_url(mut self, url: &str) -> Google {
        self.certs_url = url.into();
        self
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// The scope the allow list gives `email` now ([`scope_of`]).
    pub fn scope_of(&self, email: &str) -> Option<Scope> {
        scope_of(&self.allow, email, None)
    }

    /// What `GET /v1/admin/signin` shows: the client and the list.
    pub fn json(&self) -> Json {
        json!({"provider": "google", "client_id": self.client_id, "allow": self.allow.iter().map(Allowed::json).collect::<Vec<_>>()})
    }

    /// Checks Google's ID token `credential` for this server, with the
    /// nonce the page was given, at `now_s`: who it is, and their scope.
    pub async fn verify(&self, credential: &str, nonce: &str, now_s: u64) -> Result<Person, Refused> {
        let not_token = || refused(401, "that is not an ID token from Google's button: sign in again");
        let mut parts = credential.trim().split('.');
        let (Some(h64), Some(p64), Some(s64), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else { return Err(not_token()) };
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let header: Json = b64.decode(h64).ok().and_then(|b| serde_json::from_slice(&b).ok()).ok_or_else(not_token)?;
        let payload = b64.decode(p64).map_err(|_| not_token())?;
        let sig = b64.decode(s64).map_err(|_| not_token())?;
        if header["alg"] != "RS256" {
            return Err(refused(401, "Google signs ID tokens with RS256: this token says another algorithm"));
        }
        let kid = header["kid"].as_str().ok_or_else(not_token)?;
        let signed = &credential.trim()[..h64.len() + 1 + p64.len()];
        self.check_signature(kid, signed.as_bytes(), &sig).await?;
        let c: Json = serde_json::from_slice(&payload).map_err(|_| not_token())?;
        claims(&c, &self.client_id, nonce, now_s)?;
        let email = c["email"].as_str().unwrap_or_default().to_ascii_lowercase();
        let hd = c["hd"].as_str();
        let scope = scope_of(&self.allow, &email, Some(hd)).ok_or_else(|| refused(403, format!("{email} is not on this server's list of who may sign in (--google-allow)")))?;
        let name = c["name"].as_str().map(|n| n.chars().take(100).collect());
        Ok(Person { email, name, scope })
    }

    /// Whether `sig` over `signed` is Google's key `kid`'s: from the keys
    /// kept, else fetched again (Google turns its keys over every few
    /// days, so a new id is fetched for, at most every [`CERTS_AGAIN`]).
    async fn check_signature(&self, kid: &str, signed: &[u8], sig: &[u8]) -> Result<(), Refused> {
        if let Some(r) = self.with_key(kid, signed, sig) {
            return r;
        }
        let _one = self.certs.lock().await;
        if let Some(r) = self.with_key(kid, signed, sig) {
            return r;
        }
        let recent = self.cache.lock().unwrap_or_else(|e| e.into_inner()).fetched.is_some_and(|t| t.elapsed() < CERTS_AGAIN);
        if !recent {
            self.fetch().await?;
        }
        self.with_key(kid, signed, sig).unwrap_or_else(|| Err(refused(401, "this token's key is not one of Google's: sign in again")))
    }

    /// The check with key `kid` if it is kept and fresh; `None` if not.
    fn with_key(&self, kid: &str, signed: &[u8], sig: &[u8]) -> Option<Result<(), Refused>> {
        let c = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if c.until.is_none_or(|u| Instant::now() >= u) {
            return None;
        }
        let k = c.keys.iter().find(|k| k.kid == kid)?;
        let key = ring::signature::RsaPublicKeyComponents { n: &k.n, e: &k.e };
        Some(key.verify(&ring::signature::RSA_PKCS1_2048_8192_SHA256, signed, sig).map_err(|_| refused(401, "this token's signature is not Google's: sign in again")))
    }

    async fn fetch(&self) -> Result<(), Refused> {
        let unreadable = |e: String| refused(502, format!("Google's signing keys cannot be read ({}): {e}", self.certs_url));
        let r = self.http.get(&self.certs_url).send().await.map_err(|e| unreadable(e.to_string()))?;
        if !r.status().is_success() {
            return Err(unreadable(format!("HTTP {}", r.status())));
        }
        let max_age = r.headers().get(reqwest::header::CACHE_CONTROL).and_then(|v| v.to_str().ok()).and_then(max_age).unwrap_or(CERTS_DEFAULT);
        let j: Json = r.json().await.map_err(|e| unreadable(e.to_string()))?;
        let keys = jwks(&j);
        if keys.is_empty() {
            return Err(unreadable("no RSA signing key in the answer".into()));
        }
        let now = Instant::now();
        *self.cache.lock().unwrap_or_else(|e| e.into_inner()) = Certs { keys, until: Some(now + max_age.min(Duration::from_secs(24 * 3600))), fetched: Some(now) };
        Ok(())
    }
}

/// The RSA signing keys of a JWKS (`{"keys": [{kty, kid, n, e, ...}]}`).
fn jwks(j: &Json) -> Vec<Jwk> {
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    j["keys"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|k| k["kty"] == "RSA" && k["use"].as_str().is_none_or(|u| u == "sig") && k["alg"].as_str().is_none_or(|a| a == "RS256"))
        .filter_map(|k| Some(Jwk { kid: k["kid"].as_str()?.to_string(), n: b64.decode(k["n"].as_str()?).ok()?, e: b64.decode(k["e"].as_str()?).ok()? }))
        .collect()
}

/// `max-age` of a `Cache-Control` value.
fn max_age(v: &str) -> Option<Duration> {
    v.split(',').find_map(|p| p.trim().strip_prefix("max-age=")).and_then(|s| s.trim().parse().ok()).map(Duration::from_secs)
}

/// The checks of an ID token's claims, its signature checked: this
/// client's, Google's, in its time, the address verified, and the
/// sign-in's nonce.
fn claims(c: &Json, client_id: &str, nonce: &str, now_s: u64) -> Result<(), Refused> {
    let aud_ok = match &c["aud"] {
        Json::String(a) => a == client_id,
        Json::Array(a) => a.len() == 1 && a[0] == client_id,
        _ => false,
    };
    if !aud_ok {
        return Err(refused(401, "this ID token is for another OAuth client than this server's (--google-client-id)"));
    }
    if !c["iss"].as_str().is_some_and(|i| ISSUERS.contains(&i)) {
        return Err(refused(401, "this ID token is not Google's (its issuer)"));
    }
    let exp = c["exp"].as_u64().unwrap_or(0);
    let iat = c["iat"].as_u64().unwrap_or(u64::MAX);
    if now_s > exp + SKEW_S {
        return Err(refused(401, "this ID token has expired: sign in again"));
    }
    if iat > now_s + SKEW_S {
        return Err(refused(401, "this ID token is from the future: check this server's clock"));
    }
    let email = c["email"].as_str().unwrap_or_default();
    if email.is_empty() || c["email_verified"] != true {
        return Err(refused(403, "Google has not verified this account's address"));
    }
    let given = c["nonce"].as_str().unwrap_or_default();
    if nonce.is_empty() || !crate::auth::same_secret(given.as_bytes(), nonce.as_bytes()) {
        return Err(refused(401, "this sign-in did not start on this page (its nonce): sign in again"));
    }
    Ok(())
}

/// A cookie's value from a request's `Cookie` headers.
pub fn cookie<'a>(headers: &'a axum::http::HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(axum::http::header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|p| p.trim().strip_prefix(name).and_then(|r| r.strip_prefix('=')))
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

/// A `Set-Cookie` value for one of this module's cookies: `value` for
/// `max_age_s`, or (`None`) the cookie ended.
pub fn set_cookie(name: &str, value: Option<&str>, max_age_s: u64) -> String {
    match value {
        Some(v) => format!("{name}={v}; Path=/; Max-Age={max_age_s}; Secure; HttpOnly; SameSite=Strict"),
        None => format!("{name}=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Strict"),
    }
}

/// Whether a request's `Origin` is a page of this server's, over HTTPS or
/// on loopback: what a sign-in, and a change made with a session's
/// cookie, come from. Why not, if not.
pub fn origin_check(origin: Option<&str>, host: Option<&str>) -> Result<(), String> {
    let origin = origin.ok_or("this request names no page it comes from (Origin): a browser's page of this server's sends it")?;
    let (scheme, authority) = origin.split_once("://").ok_or_else(|| format!("{origin:?} is not a page's origin"))?;
    if !host.is_some_and(|h| h.trim().eq_ignore_ascii_case(authority)) {
        return Err(format!("a page from {origin} cannot use a session of this server's"));
    }
    let loopback = crate::auth::LOOPBACK_HOSTS.contains(&crate::auth::host_name(authority).as_str());
    match (scheme, loopback) {
        ("https", _) | ("http", true) => Ok(()),
        _ => Err(format!("{origin} is plain HTTP: a session's cookie crosses the network readable, so sign in over HTTPS (or on this machine, at http://localhost)")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_allow_list_is_read_and_what_would_let_anyone_in_refused() {
        assert_eq!(Allowed::parse("Ro@Example.com=admin"), Ok(Allowed { who: Who::Person("ro@example.com".into()), scope: Scope::Admin }));
        assert_eq!(Allowed::parse("ro@example.com"), Ok(Allowed { who: Who::Person("ro@example.com".into()), scope: Scope::Read }));
        assert_eq!(Allowed::parse("@chloetech.ai=write"), Ok(Allowed { who: Who::Domain("chloetech.ai".into()), scope: Scope::Write }));
        for bad in ["@gmail.com=read", "@googlemail.com", "ro@example.com=feed", "ro@example.com=root", "nobody", "@", "@localhost", "a b@example.com", "ro@ex..com", ""] {
            assert!(Allowed::parse(bad).is_err(), "{bad}");
        }
        let allow: Vec<Allowed> = ["ro@example.com=admin", "@chloetech.ai=read", "boss@chloetech.ai=admin"].iter().map(|a| Allowed::parse(a).unwrap()).collect();
        assert_eq!(scope_of(&allow, "RO@example.com", Some(None)), Some(Scope::Admin));
        assert_eq!(scope_of(&allow, "ann@chloetech.ai", Some(Some("chloetech.ai"))), Some(Scope::Read));
        assert_eq!(scope_of(&allow, "boss@chloetech.ai", Some(Some("chloetech.ai"))), Some(Scope::Admin));
        // A Workspace domain's entry takes that domain's accounts, not an
        // address of the same name elsewhere (Google's hd says whose).
        assert_eq!(scope_of(&allow, "ann@chloetech.ai", Some(Some("other.com"))), None);
        assert_eq!(scope_of(&allow, "ann@chloetech.ai", Some(None)), None, "no Workspace account");
        assert_eq!(scope_of(&allow, "ann@chloetech.ai", None), Some(Scope::Read), "a session's check, its sign-in matched");
        assert_eq!(scope_of(&allow, "eve@example.com", None), None);
        assert!(Google::new("123-abc.apps.googleusercontent.com", &["ro@example.com".into()]).is_ok());
        assert!(Google::new("123-abc.apps.googleusercontent.com", &[]).unwrap_err().contains("--google-allow"));
        assert!(Google::new("not-a-client", &["ro@example.com".into()]).unwrap_err().contains("apps.googleusercontent.com"));
    }

    #[test]
    fn claims_are_checked() {
        let good = json!({"iss": "https://accounts.google.com", "aud": "c.apps.googleusercontent.com", "exp": 2000, "iat": 1000, "email": "ro@example.com", "email_verified": true, "nonce": "n1"});
        assert_eq!(claims(&good, "c.apps.googleusercontent.com", "n1", 1500), Ok(()));
        let with = |k: &str, v: Json| {
            let mut c = good.clone();
            c[k] = v;
            claims(&c, "c.apps.googleusercontent.com", "n1", 1500).map_err(|r| (r.status, r.message))
        };
        assert!(with("aud", json!("other.apps.googleusercontent.com")).unwrap_err().1.contains("another OAuth client"));
        assert!(with("iss", json!("https://evil.example")).unwrap_err().1.contains("not Google's"));
        assert!(with("exp", json!(1400)).unwrap_err().1.contains("expired"));
        assert!(with("iat", json!(1700)).unwrap_err().1.contains("future"));
        assert_eq!(with("email_verified", json!(false)).unwrap_err().0, 403);
        assert!(with("nonce", json!("n2")).unwrap_err().1.contains("nonce"));
        assert!(claims(&good, "c.apps.googleusercontent.com", "", 1500).is_err(), "no nonce of the page's");
        assert!(claims(&good, "c.apps.googleusercontent.com", "n1", 2061).is_err() && claims(&good, "c.apps.googleusercontent.com", "n1", 2059).is_ok(), "a minute's skew");
    }

    #[test]
    fn cookies_and_origins() {
        let mut h = axum::http::HeaderMap::new();
        h.append("cookie", "a=1; __Host-chloedb-session=sk_ab_cd".parse().unwrap());
        h.append("cookie", "__Host-chloedb-nonce=n9".parse().unwrap());
        assert_eq!(cookie(&h, SESSION_COOKIE), Some("sk_ab_cd"));
        assert_eq!(cookie(&h, NONCE_COOKIE), Some("n9"));
        assert_eq!(cookie(&h, "__Host-chloedb"), None);
        assert!(set_cookie(SESSION_COOKIE, Some("v"), 60).contains("; Secure; HttpOnly; SameSite=Strict"));
        assert!(origin_check(Some("https://cmd.example.com"), Some("cmd.example.com")).is_ok());
        assert!(origin_check(Some("http://localhost:7700"), Some("localhost:7700")).is_ok());
        assert!(origin_check(Some("http://cmd.example.com"), Some("cmd.example.com")).unwrap_err().contains("plain HTTP"));
        assert!(origin_check(Some("https://evil.example"), Some("cmd.example.com")).is_err());
        assert!(origin_check(None, Some("cmd.example.com")).is_err());
        assert_eq!(max_age("public, max-age=19663, must-revalidate"), Some(Duration::from_secs(19663)));
    }
}
