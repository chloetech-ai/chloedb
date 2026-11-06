//! The hub: one command center for many clusters. This server keeps a
//! book of connections to other clusters (their addresses, a pinned
//! certificate and an API key each) and forwards `/c/<name>/...` to the
//! connection `<name>` with that connection's key, so that the command
//! center served at `/c/<name>/` works on that cluster as on this one.
//!
//! The browser never holds a cluster's key: the keys stay here, in the
//! macOS Keychain or the Secret Service (`secret-tool`) where there is
//! one, else in a file beside the database that only its owner reads
//! (`<db>.hub-keys.json`, as the clouds' keys are kept). The book itself
//! (`<db>.hub.json`) holds no secret and can be exported and rebuilt.
//!
//! A connection over HTTPS whose certificate a public authority signed is
//! checked as any HTTPS call is. One whose certificate is self-signed is
//! pinned: its SHA-256 is shown when it is added, kept once the person
//! adding it agrees, and every later call refuses another certificate.
//! Plain HTTP is refused beyond loopback unless the connection says it
//! allows it (`allow_plain`), since the key and the rows then cross the
//! network readable.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};
use sha2::{Digest, Sha256};

use crate::ApiError;

/// The service name the keychains keep the keys under.
const SERVICE: &str = "strata-hub";

/// How long a connection's first answer may take when it is added or
/// checked.
const CHECK_TIMEOUT: Duration = Duration::from_secs(8);

/// How long making a connection to a cluster may take on a forwarded call.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// One cluster in the book.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Connection {
    /// Its name in this book: what `/c/<name>/` names.
    pub name: String,
    /// The color its top bar and confirms carry (`#rrggbb`).
    pub color: String,
    /// Its coordinators' base URLs; calls go to the first that answers.
    pub urls: Vec<String>,
    /// The SHA-256 (hex) of the self-signed certificate it answered with
    /// when it was added; `None` for one an authority signed, or HTTP.
    #[serde(default)]
    pub pin: Option<String>,
    /// Plain HTTP beyond loopback, allowed by the person who added it.
    #[serde(default)]
    pub allow_plain: bool,
    /// What the cluster said it is (`/v1/whoami`'s `cluster`: its name,
    /// environment and public URL) when it was added or last checked.
    #[serde(default)]
    pub cluster: Json,
    /// What the key is there (`/v1/whoami`): its name, id, scope, owner,
    /// expiry.
    #[serde(default)]
    pub key: Json,
    pub added_ms: u64,
    pub added_by: String,
    /// When it last answered a call through the hub, and the last failure.
    #[serde(default)]
    pub last_ok_ms: Option<u64>,
    #[serde(default)]
    pub last_error: Option<String>,
}

impl Connection {
    /// What the page shows: all of it (the book holds no secret).
    pub fn view(&self) -> Json {
        let mut j = serde_json::to_value(self).unwrap_or(Json::Null);
        j["environment"] = self.cluster["environment"].clone();
        j["scope"] = self.key["scope"].clone();
        j
    }
}

/// Where the keys are kept.
#[derive(Clone, Debug)]
pub enum Secrets {
    /// The macOS Keychain, through `security`.
    Keychain,
    /// The Secret Service (GNOME Keyring, KWallet), through `secret-tool`.
    SecretTool,
    /// A file only its owner reads; `None`: memory only (tests, a hub
    /// without a database).
    File(Option<PathBuf>),
}

impl Secrets {
    /// The platform's keychain if it has one, else `file`.
    /// `STRATA_HUB_SECRETS=file` keeps them in the file anyway.
    pub fn detect(file: Option<PathBuf>) -> Secrets {
        let want = std::env::var("STRATA_HUB_SECRETS").unwrap_or_default();
        if want == "file" {
            return Secrets::File(file);
        }
        let has = |cmd: &str| std::process::Command::new("sh").args(["-c", &format!("command -v {cmd}")]).output().is_ok_and(|o| o.status.success());
        if cfg!(target_os = "macos") && has("security") {
            return Secrets::Keychain;
        }
        if cfg!(target_os = "linux") && std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some() && has("secret-tool") {
            return Secrets::SecretTool;
        }
        Secrets::File(file)
    }

    pub fn describe(&self) -> Json {
        match self {
            Secrets::Keychain => json!({"kind": "keychain", "what": "the macOS Keychain"}),
            Secrets::SecretTool => json!({"kind": "secret-service", "what": "the Secret Service (secret-tool)"}),
            Secrets::File(f) => json!({"kind": "file", "what": f.as_ref().map_or("memory only".to_string(), |f| format!("{} (mode 600)", f.display())), "file": f}),
        }
    }
}

/// Runs `cmd args`, writing `input` to its standard input (a key is never
/// put on a command line, which other processes can read).
fn run(cmd: &str, args: &[&str], input: Option<&str>) -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut c = Command::new(cmd).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("{cmd}: {e}"))?;
    if let Some(i) = input {
        c.stdin.take().expect("piped").write_all(i.as_bytes()).map_err(|e| format!("{cmd}: {e}"))?;
    }
    drop(c.stdin.take());
    let o = c.wait_with_output().map_err(|e| format!("{cmd}: {e}"))?;
    if !o.status.success() {
        return Err(format!("{cmd} {}: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&o.stdout).trim_end_matches('\n').to_string())
}

/// The keys, by connection name, wherever they are kept, with those read
/// so far in memory.
struct Keys {
    secrets: Secrets,
    /// The account names are `<hub>:<connection>`, so that two hubs on one
    /// machine keep their keys apart.
    hub: String,
    memory: Mutex<HashMap<String, String>>,
}

impl Keys {
    fn new(secrets: Secrets, hub: String) -> Keys {
        let memory = match &secrets {
            Secrets::File(Some(f)) => std::fs::read(f).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default(),
            _ => HashMap::new(),
        };
        Keys { secrets, hub, memory: Mutex::new(memory) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.memory.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn account(&self, name: &str) -> String {
        format!("{}:{name}", self.hub)
    }

    fn get(&self, name: &str) -> Result<String, String> {
        if let Some(k) = self.lock().get(name) {
            return Ok(k.clone());
        }
        let acct = self.account(name);
        let k = match &self.secrets {
            Secrets::Keychain => run("security", &["find-generic-password", "-s", SERVICE, "-a", &acct, "-w"], None)?,
            Secrets::SecretTool => run("secret-tool", &["lookup", "service", SERVICE, "account", &acct], None)?,
            Secrets::File(_) => return Err(format!("no key kept for {name}: add the connection again")),
        };
        if k.is_empty() {
            return Err(format!("no key kept for {name}: add the connection again"));
        }
        self.lock().insert(name.to_string(), k.clone());
        Ok(k)
    }

    fn set(&self, name: &str, key: &str) -> Result<(), String> {
        let acct = self.account(name);
        match &self.secrets {
            // `security -i` reads the command from its input, so the key is
            // on no command line.
            Secrets::Keychain => {
                let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
                run("security", &["-i"], Some(&format!("add-generic-password -U -s {} -a {} -l {} -w {}\n", quote(SERVICE), quote(&acct), quote(&format!("Strata hub: {name}")), quote(key))))?;
            }
            Secrets::SecretTool => {
                run("secret-tool", &["store", "--label", &format!("Strata hub: {name}"), "service", SERVICE, "account", &acct], Some(key))?;
            }
            Secrets::File(_) => {}
        }
        let mut m = self.lock();
        m.insert(name.to_string(), key.to_string());
        self.save(&m)
    }

    fn remove(&self, name: &str) -> Result<(), String> {
        let acct = self.account(name);
        match &self.secrets {
            Secrets::Keychain => {
                let _ = run("security", &["delete-generic-password", "-s", SERVICE, "-a", &acct], None);
            }
            Secrets::SecretTool => {
                let _ = run("secret-tool", &["clear", "service", SERVICE, "account", &acct], None);
            }
            Secrets::File(_) => {}
        }
        let mut m = self.lock();
        m.remove(name);
        self.save(&m)
    }

    fn save(&self, m: &HashMap<String, String>) -> Result<(), String> {
        let Secrets::File(Some(f)) = &self.secrets else { return Ok(()) };
        let b = serde_json::to_vec_pretty(m).map_err(|e| e.to_string())?;
        crate::aws::write_private(f, &b).map_err(|e| format!("write {}: {e}", f.display()))
    }
}

/// The SHA-256 of a certificate (DER), as lowercase hex.
pub fn fingerprint(der: &[u8]) -> String {
    Sha256::digest(der).iter().map(|b| format!("{b:02x}")).collect()
}

/// A certificate check that takes only the pinned certificate (any
/// certificate with `pin` unset), and notes the one it was shown.
#[derive(Debug)]
struct Pinned {
    pin: Option<String>,
    seen: Arc<Mutex<Option<String>>>,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl rustls::client::danger::ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let fp = fingerprint(end_entity.as_ref());
        *self.seen.lock().unwrap_or_else(|e| e.into_inner()) = Some(fp.clone());
        match &self.pin {
            Some(p) if !p.eq_ignore_ascii_case(&fp) => Err(rustls::Error::General(format!("the certificate is {fp}, not the pinned {p}"))),
            _ => Ok(rustls::client::danger::ServerCertVerified::assertion()),
        }
    }

    fn verify_tls12_signature(&self, message: &[u8], cert: &rustls::pki_types::CertificateDer<'_>, dss: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &rustls::pki_types::CertificateDer<'_>, dss: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// A client that takes only the certificate `pin` (any, without one), and
/// where it notes the certificate it was shown.
fn pinned_client(pin: Option<String>, timeout: Option<Duration>) -> Result<(reqwest::Client, Arc<Mutex<Option<String>>>), String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let seen = Arc::new(Mutex::new(None));
    let tls = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Pinned { pin, seen: seen.clone(), provider }))
        .with_no_client_auth();
    let mut b = crate::http_client().use_preconfigured_tls(tls).connect_timeout(CONNECT_TIMEOUT);
    if let Some(t) = timeout {
        b = b.timeout(t);
    }
    Ok((b.build().map_err(|e| e.to_string())?, seen))
}

/// Whether `url`'s host is this machine.
fn loopback(url: &reqwest::Url) -> bool {
    let host = url.host_str().unwrap_or_default().trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost") || host.parse::<std::net::IpAddr>().is_ok_and(|a| a.is_loopback())
}

/// A connection's name: 1 to 40 lowercase letters, digits and `-`, not
/// starting with `-`.
pub fn check_name(name: &str) -> Result<(), ApiError> {
    let ok = !name.is_empty() && name.len() <= 40 && !name.starts_with('-') && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(ApiError::bad(format!("a connection's name is 1 to 40 lowercase letters, digits and '-', not {name:?}")))
    }
}

/// The colors a new connection gets in turn.
const COLORS: [&str; 8] = ["#d95926", "#d55181", "#3987e5", "#199e70", "#9085e9", "#c98500", "#0f9fb5", "#a0732e"];

/// What adding a connection takes.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Add {
    pub name: String,
    /// The cluster's address (a coordinator's base URL); more may follow,
    /// comma-separated.
    pub url: String,
    pub key: String,
    #[serde(default)]
    pub color: Option<String>,
    /// The SHA-256 of a self-signed certificate the person agreed to.
    #[serde(default)]
    pub trust: Option<String>,
    #[serde(default)]
    pub allow_plain: bool,
    /// Replace a connection of this name.
    #[serde(default)]
    pub replace: bool,
}

/// What adding a connection came to.
#[derive(Debug)]
pub enum Added {
    Done(Connection),
    /// Its certificate is self-signed: the request names this fingerprint
    /// in `trust` once the person agrees to it.
    NeedsTrust { url: String, fingerprint: String },
}

/// The book and the keys.
pub struct Hub {
    book: Mutex<Vec<Connection>>,
    file: Option<PathBuf>,
    keys: Keys,
    /// Each connection's client, made once (and again when it changes).
    clients: Mutex<HashMap<String, reqwest::Client>>,
    /// Who this hub is: the person and the machine (`--hub-device`, else
    /// `$USER@<host>`).
    pub device: String,
}

impl Default for Hub {
    fn default() -> Hub {
        Hub::memory()
    }
}

/// This machine's name.
fn host_name() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|h| !h.is_empty())
        .or_else(|| run("hostname", &[], None).ok())
        .map(|h| h.trim().trim_end_matches(".local").to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "this-machine".into())
}

/// Who a hub on this machine is, by default.
pub fn default_device() -> String {
    let user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_else(|_| "someone".into());
    format!("{user}@{}", host_name())
}

impl Hub {
    /// A book in memory, the keys too.
    pub fn memory() -> Hub {
        Hub { book: Mutex::new(Vec::new()), file: None, keys: Keys::new(Secrets::File(None), "memory".into()), clients: Mutex::new(HashMap::new()), device: default_device() }
    }

    /// `<db>.hub.json`, beside the database (`<db>.hub-keys.json` for a
    /// file of keys).
    pub fn file_of(db: &Path) -> PathBuf {
        let mut p = db.as_os_str().to_owned();
        p.push(".hub.json");
        p.into()
    }

    /// The book kept in `file`, the keys where `secrets` says.
    pub fn open(file: PathBuf, secrets: Secrets, device: Option<String>) -> Result<Hub, String> {
        let book = match std::fs::read(&file) {
            Ok(b) => serde_json::from_slice::<Vec<Connection>>(&b).map_err(|e| format!("{}: {e}", file.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(format!("{}: {e}", file.display())),
        };
        let abs = std::fs::canonicalize(file.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."))).unwrap_or_default().join(file.file_name().unwrap_or_default());
        let hub = fingerprint(abs.to_string_lossy().as_bytes())[..12].to_string();
        Ok(Hub { book: Mutex::new(book), file: Some(file), keys: Keys::new(secrets, hub), clients: Mutex::new(HashMap::new()), device: device.filter(|d| !d.trim().is_empty()).unwrap_or_else(default_device) })
    }

    /// The keys' file, for a book kept in `file` without a keychain.
    pub fn keys_file_of(file: &Path) -> PathBuf {
        let s = file.to_string_lossy();
        PathBuf::from(s.strip_suffix(".hub.json").map_or_else(|| format!("{s}.keys"), |b| format!("{b}.hub-keys.json")))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Connection>> {
        self.book.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn save(&self, list: &[Connection]) -> Result<(), String> {
        let Some(f) = &self.file else { return Ok(()) };
        let b = serde_json::to_vec_pretty(list).map_err(|e| e.to_string())?;
        crate::aws::write_private(f, &b).map_err(|e| format!("write {}: {e}", f.display()))
    }

    pub fn list(&self) -> Vec<Connection> {
        self.lock().clone()
    }

    pub fn get(&self, name: &str) -> Option<Connection> {
        self.lock().iter().find(|c| c.name == name).cloned()
    }

    /// What `GET /v1/admin/hub` answers.
    pub fn view(&self) -> Json {
        json!({
            "device": self.device,
            "keys": self.keys.secrets.describe(),
            "file": self.file,
            "connections": self.list().iter().map(Connection::view).collect::<Vec<_>>(),
        })
    }

    /// The client calls to a connection go through.
    fn client(&self, c: &Connection) -> Result<reqwest::Client, ApiError> {
        if let Some(cl) = self.clients.lock().unwrap_or_else(|e| e.into_inner()).get(&c.name) {
            return Ok(cl.clone());
        }
        let cl = match &c.pin {
            Some(p) => pinned_client(Some(p.clone()), None).map_err(ApiError::internal)?.0,
            None => crate::http_client().connect_timeout(CONNECT_TIMEOUT).build().map_err(|e| ApiError::internal(e.to_string()))?,
        };
        self.clients.lock().unwrap_or_else(|e| e.into_inner()).insert(c.name.clone(), cl.clone());
        Ok(cl)
    }

    /// Adds a connection: checks the addresses, the certificate and the
    /// key (`/v1/whoami` with it), then keeps it. A self-signed
    /// certificate is answered with its fingerprint (409, `needs_trust`)
    /// until the request names it in `trust`.
    pub async fn add(&self, a: Add, by: &str) -> Result<Added, ApiError> {
        check_name(&a.name)?;
        if !a.replace && self.get(&a.name).is_some() {
            return Err(ApiError { status: 409, message: format!("{} is in the book already: pick another name, or replace it", a.name) });
        }
        let key = a.key.trim();
        if key.is_empty() {
            return Err(ApiError::bad("the connection needs the cluster's API key (or token)"));
        }
        let mut urls = Vec::new();
        for u in a.url.split(',').map(str::trim).filter(|u| !u.is_empty()) {
            let u = if u.contains("://") { u.to_string() } else { format!("https://{u}") };
            let parsed = reqwest::Url::parse(&u).map_err(|e| ApiError::bad(format!("{u}: {e}")))?;
            if !matches!(parsed.scheme(), "http" | "https") || parsed.host().is_none() {
                return Err(ApiError::bad(format!("{u}: a connection's address is an http:// or https:// URL")));
            }
            if parsed.scheme() == "http" && !loopback(&parsed) && !a.allow_plain {
                return Err(ApiError { status: 409, message: format!("{u} is plain HTTP beyond this machine: the key and the rows would cross the network readable. Give the cluster HTTPS, or allow plain HTTP for this connection") });
            }
            urls.push(u.trim_end_matches('/').to_string());
        }
        let first = urls.first().cloned().ok_or_else(|| ApiError::bad("the connection needs the cluster's address"))?;
        // The certificate: one an authority signed is checked as usual; a
        // self-signed one is pinned, once the person agrees to it.
        let mut pin = None;
        if first.starts_with("https://") {
            let plain = crate::http_client().timeout(CHECK_TIMEOUT).build().map_err(|e| ApiError::internal(e.to_string()))?;
            let signed = plain.get(format!("{first}/livez")).send().await;
            if signed.is_err() {
                let (probe, seen) = pinned_client(None, Some(CHECK_TIMEOUT)).map_err(ApiError::internal)?;
                let r = probe.get(format!("{first}/livez")).send().await;
                let fp = seen.lock().unwrap_or_else(|e| e.into_inner()).clone();
                match (r, fp) {
                    (Ok(_), Some(fp)) => match &a.trust {
                        Some(t) if t.eq_ignore_ascii_case(&fp) => pin = Some(fp),
                        _ => return Ok(Added::NeedsTrust { url: first, fingerprint: fp }),
                    },
                    (Err(e), _) => return Err(ApiError { status: 502, message: format!("{first} does not answer: {}", error_text(&e)) }),
                    (Ok(_), None) => return Err(ApiError { status: 502, message: format!("{first} answered without a certificate") }),
                }
            }
        }
        let c = Connection {
            name: a.name.clone(),
            color: a.color.filter(|c| c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit())).unwrap_or_else(|| COLORS[self.lock().len() % COLORS.len()].to_string()),
            urls,
            pin,
            allow_plain: a.allow_plain,
            cluster: Json::Null,
            key: Json::Null,
            added_ms: crate::operator::now_ms(),
            added_by: by.to_string(),
            last_ok_ms: None,
            last_error: None,
        };
        let who = self.whoami(&c, key).await?;
        let c = Connection { cluster: who["cluster"].clone(), key: key_view(&who), last_ok_ms: Some(crate::operator::now_ms()), ..c };
        self.keys.set(&c.name, key).map_err(|e| ApiError::internal(format!("keeping the key: {e}")))?;
        let mut list = self.lock();
        list.retain(|o| o.name != c.name);
        list.push(c.clone());
        self.save(&list).map_err(ApiError::internal)?;
        drop(list);
        self.clients.lock().unwrap_or_else(|e| e.into_inner()).remove(&c.name);
        Ok(Added::Done(c))
    }

    /// `/v1/whoami` at the connection, with `key`: refused unless it
    /// answers as Strata and takes the key.
    async fn whoami(&self, c: &Connection, key: &str) -> Result<Json, ApiError> {
        let cl = match &c.pin {
            Some(p) => pinned_client(Some(p.clone()), Some(CHECK_TIMEOUT)).map_err(ApiError::internal)?.0,
            None => crate::http_client().timeout(CHECK_TIMEOUT).build().map_err(|e| ApiError::internal(e.to_string()))?,
        };
        let mut last = String::new();
        for u in &c.urls {
            match cl.get(format!("{u}/v1/whoami")).bearer_auth(key).header("x-strata-client", "hub").send().await {
                Ok(r) if r.status().is_success() => {
                    let j: Json = r.json().await.map_err(|e| ApiError { status: 502, message: format!("{u} did not answer as Strata does: {e}") })?;
                    if j.get("scope").is_none() {
                        return Err(ApiError { status: 502, message: format!("{u} did not answer as Strata does") });
                    }
                    if j["auth"]["open"] == true && !loopback(&reqwest::Url::parse(u).map_err(|e| ApiError::bad(e.to_string()))?) {
                        tracing::warn!(connection = %c.name, url = %u, "this cluster takes calls without a key");
                    }
                    return Ok(j);
                }
                Ok(r) if r.status().as_u16() == 401 || r.status().as_u16() == 403 => return Err(ApiError { status: 409, message: format!("{u} refused the key ({}): check it, and that it has not expired", r.status().as_u16()) }),
                Ok(r) => last = format!("{u} answered {}", r.status()),
                Err(e) => last = format!("{u} does not answer: {}", error_text(&e)),
            }
        }
        Err(ApiError { status: 502, message: last })
    }

    /// Asks the connection again who its key is, and notes when it
    /// answered.
    pub async fn check(&self, name: &str) -> Result<Connection, ApiError> {
        let c = self.get(name).ok_or_else(|| not_found(name))?;
        let key = self.keys.get(name).map_err(ApiError::internal)?;
        let r = self.whoami(&c, &key).await;
        let mut list = self.lock();
        let Some(e) = list.iter_mut().find(|o| o.name == name) else { return Err(not_found(name)) };
        match &r {
            Ok(who) => {
                e.cluster = who["cluster"].clone();
                e.key = key_view(who);
                e.last_ok_ms = Some(crate::operator::now_ms());
                e.last_error = None;
            }
            Err(err) => e.last_error = Some(err.message.clone()),
        }
        let out = e.clone();
        let _ = self.save(&list);
        r.map(|_| out)
    }

    /// Changes a connection's color or key (a new key is checked first).
    pub async fn update(&self, name: &str, b: &Json) -> Result<Connection, ApiError> {
        let c = self.get(name).ok_or_else(|| not_found(name))?;
        if let Some(k) = b.get("key").and_then(Json::as_str).map(str::trim).filter(|k| !k.is_empty()) {
            let who = self.whoami(&c, k).await?;
            self.keys.set(name, k).map_err(|e| ApiError::internal(format!("keeping the key: {e}")))?;
            let mut list = self.lock();
            if let Some(e) = list.iter_mut().find(|o| o.name == name) {
                e.key = key_view(&who);
                e.cluster = who["cluster"].clone();
            }
            self.save(&list).map_err(ApiError::internal)?;
        }
        if let Some(color) = b.get("color").and_then(Json::as_str) {
            if !(color.len() == 7 && color.starts_with('#') && color[1..].bytes().all(|b| b.is_ascii_hexdigit())) {
                return Err(ApiError::bad("a color is #rrggbb"));
            }
            let mut list = self.lock();
            if let Some(e) = list.iter_mut().find(|o| o.name == name) {
                e.color = color.to_string();
            }
            self.save(&list).map_err(ApiError::internal)?;
        }
        self.get(name).ok_or_else(|| not_found(name))
    }

    /// Takes a connection out of the book, and its key out of where keys
    /// are kept. The key itself still works on the cluster until it
    /// expires or is revoked there.
    pub fn remove(&self, name: &str) -> Result<Connection, ApiError> {
        let mut list = self.lock();
        let i = list.iter().position(|c| c.name == name).ok_or_else(|| not_found(name))?;
        let c = list.remove(i);
        self.save(&list).map_err(ApiError::internal)?;
        drop(list);
        self.keys.remove(name).map_err(ApiError::internal)?;
        self.clients.lock().unwrap_or_else(|e| e.into_inner()).remove(name);
        Ok(c)
    }

    fn noted(&self, name: &str, r: &Result<(), String>) {
        let mut list = self.lock();
        let Some(e) = list.iter_mut().find(|o| o.name == name) else { return };
        let now = crate::operator::now_ms();
        match r {
            // Kept to the second: a forwarded call need not write the book.
            Ok(()) if e.last_error.is_none() && e.last_ok_ms.is_some_and(|t| now.saturating_sub(t) < 60_000) => return,
            Ok(()) => {
                e.last_ok_ms = Some(now);
                e.last_error = None;
            }
            Err(m) if e.last_error.as_deref() == Some(m) => return,
            Err(m) => e.last_error = Some(m.clone()),
        }
        let _ = self.save(&list);
    }

    /// Sends a request to the connection `name`, at `path` (`/v1/...`,
    /// with its query), with that connection's key in place of the
    /// caller's credential: the answer as the cluster gave it, but a
    /// refused key, which is answered 502 so that the page asks for no
    /// login of this hub's.
    pub async fn forward(&self, name: &str, method: reqwest::Method, path: &str, headers: &axum::http::HeaderMap, body: reqwest::Body) -> Result<axum::response::Response, ApiError> {
        let c = self.get(name).ok_or_else(|| not_found(name))?;
        let key = self.keys.get(name).map_err(ApiError::internal)?;
        let cl = self.client(&c)?;
        // A call with a body goes to the first address only; one without
        // tries the next when an address does not answer.
        let retry = method == reqwest::Method::GET || method == reqwest::Method::HEAD;
        let mut body = Some(body);
        let mut last = String::new();
        for (i, u) in c.urls.iter().enumerate() {
            let mut rb = cl.request(method.clone(), format!("{u}{path}")).bearer_auth(&key).header("x-strata-via", format!("hub {}", self.device));
            for (k, v) in headers {
                if PASSED.iter().any(|p| k.as_str().eq_ignore_ascii_case(p)) {
                    rb = rb.header(k.as_str(), v.as_bytes());
                }
            }
            if let Some(b) = body.take() {
                rb = rb.body(b);
            }
            match rb.send().await {
                Ok(r) => {
                    self.noted(name, &Ok(()));
                    return Ok(answer(name, r));
                }
                Err(e) => {
                    last = format!("{name} does not answer at {u}: {}", error_text(&e));
                    if !retry || i + 1 == c.urls.len() {
                        break;
                    }
                }
            }
        }
        self.noted(name, &Err(last.clone()));
        Err(ApiError { status: 502, message: last })
    }
}

/// The request headers a forwarded call keeps: what the body is and what
/// the command center says of the call. Not the caller's credential,
/// cookies, `Host` or `Origin`.
const PASSED: [&str; 7] = ["content-type", "accept", "x-strata-client", "x-strata-reason", "x-request-id", "idempotency-key", "x-strata-confirm"];

/// The answer headers passed back.
const RETURNED: [&str; 6] = ["content-type", "content-disposition", "x-request-id", "retry-after", "cache-control", "x-strata-shards"];

fn answer(name: &str, r: reqwest::Response) -> axum::response::Response {
    use axum::response::IntoResponse;
    let status = r.status().as_u16();
    if status == 401 || status == 403 {
        let why = format!("{name} refused this hub's key ({status}): it may have expired or been revoked; put a new one in Connections");
        return ApiError { status: 502, message: why }.into_response();
    }
    let mut out = axum::response::Response::builder().status(status);
    for (k, v) in r.headers() {
        if RETURNED.iter().any(|p| k.as_str().eq_ignore_ascii_case(p)) {
            out = out.header(k.as_str(), v.as_bytes());
        }
    }
    out.body(axum::body::Body::from_stream(r.bytes_stream())).unwrap_or_else(|e| ApiError::internal(e.to_string()).into_response())
}

/// What the book keeps of a key, from `/v1/whoami`: never its secret.
fn key_view(who: &Json) -> Json {
    json!({"name": who["name"], "id": who["key"], "scope": who["scope"], "root": who["root"], "owner": who["owner"], "expires_ms": who["expires_ms"]})
}

fn not_found(name: &str) -> ApiError {
    ApiError { status: 404, message: format!("no connection {name} in this hub's book") }
}

/// An error and its causes, on one line.
fn error_text(e: &(dyn std::error::Error + 'static)) -> String {
    let mut s = e.to_string();
    let mut cur = e.source();
    while let Some(c) = cur {
        let t = c.to_string();
        if !s.contains(&t) {
            s.push_str(": ");
            s.push_str(&t);
        }
        cur = c.source();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_keys_files() {
        assert!(check_name("prod-search").is_ok() && check_name("a").is_ok());
        assert!(check_name("").is_err() && check_name("-a").is_err() && check_name("Prod").is_err() && check_name("a b").is_err() && check_name(&"a".repeat(41)).is_err());
        assert_eq!(Hub::keys_file_of(Path::new("/x/my.strata.hub.json")), PathBuf::from("/x/my.strata.hub-keys.json"));
        assert_eq!(fingerprint(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn keys_in_a_file_are_its_owners_only() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("k.json");
        let k = Keys::new(Secrets::File(Some(f.clone())), "h".into());
        k.set("prod", "sk_1_secret").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let again = Keys::new(Secrets::File(Some(f)), "h".into());
        assert_eq!(again.get("prod").unwrap(), "sk_1_secret");
        again.remove("prod").unwrap();
        assert!(again.get("prod").is_err());
    }
}
