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
//! A coordinator the Kubernetes operator runs has no database and often
//! no volume: it keeps the book and the keys in a Secret of its namespace
//! instead, `<cluster>-hub`, which the operator makes and lets only that
//! cluster's coordinators read and change ([`Hub::in_secret`]). A rollout
//! then keeps the book, and every coordinator of the cluster shares it.
//!
//! A connection over HTTPS whose certificate a public authority signed is
//! checked as any HTTPS call is. One whose certificate is self-signed is
//! pinned: its SHA-256 is shown when it is added, kept once the person
//! adding it agrees, and every later call refuses another certificate.
//! Plain HTTP is refused beyond loopback unless the connection says it
//! allows it (`allow_plain`), since the key and the rows then cross the
//! network readable.
//!
//! A cluster this server made from the Cloud view is reached another way:
//! through its Kubernetes API, with the cloud's credentials this server
//! already holds, to the coordinators' Service (the API server's service
//! proxy). Nothing is exposed and nothing is installed: no public address,
//! no kubectl, no port-forward ([`ViaKube`], [`Hub::adopt`]). The API server
//! takes `Authorization` for itself, so the cluster's key goes in
//! [`VIA_PROXY_AUTH`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};
use sha2::{Digest, Sha256};

use crate::operator::kube::Kube;
use crate::ApiError;

/// The header a cluster's key goes in on a call through its Kubernetes
/// API, whose server keeps `Authorization` (its own credential) and does
/// not pass it on; chloedb-server reads it when `Authorization` is absent.
pub const VIA_PROXY_AUTH: &str = "x-strata-authorization";

/// How long a forwarded call through a Kubernetes API may take, its
/// answer streamed (an export, a log tail).
const KUBE_TIMEOUT: Duration = Duration::from_secs(3600);

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
    /// Through a Kubernetes API, for a cluster made here (`urls` is then
    /// empty).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kube: Option<ViaKube>,
    /// Without a key: the cluster takes calls without one (made before
    /// clusters had tokens), and its Kubernetes API is what lets the hub
    /// in.
    #[serde(default)]
    pub keyless: bool,
}

/// How the hub reaches a cluster it made from the Cloud view: through that
/// cluster's Kubernetes API, with its cloud's credentials, to the
/// coordinators' Service.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ViaKube {
    /// Its id among the clusters made here
    /// ([`crate::cloud::clusters::Made`]).
    pub made: String,
    /// `aws`, `digitalocean` or `gcp`.
    pub provider: String,
    /// The cloud's cluster (`strata-<name>`) and where it is.
    pub cluster: String,
    pub region: String,
    pub namespace: String,
    /// The StrataCluster, whose Service is in front of its coordinators.
    pub service: String,
}

impl ViaKube {
    /// The API server's path to the coordinators' Service.
    pub fn proxy(&self) -> String {
        format!("/api/v1/namespaces/{}/services/http:{}:7700/proxy", self.namespace, self.service)
    }

    /// What it is, in words.
    pub fn describe(&self) -> String {
        let cloud = match self.provider.as_str() {
            "aws" => "EKS",
            "gcp" => "GKE",
            _ => "DigitalOcean",
        };
        format!("{}'s Kubernetes API ({cloud}, {})", self.cluster, self.region)
    }
}

/// What a Kubernetes API server answered for itself (a `Status`), not the
/// cluster behind it: its message.
fn kube_status(body: &[u8]) -> Option<String> {
    let j: Json = serde_json::from_slice(body).ok()?;
    (j["kind"] == "Status").then(|| j["message"].as_str().unwrap_or("no message").to_string())
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
    /// The Kubernetes Secret `namespace/name`, with the book
    /// ([`Hub::in_secret`]); the keys read from it are held in memory.
    Kube(String),
}

impl Secrets {
    /// The platform's keychain if it has one, else `file`.
    /// `CHLOEDB_HUB_SECRETS=file` keeps them in the file anyway.
    pub fn detect(file: Option<PathBuf>) -> Secrets {
        let want = chloedb::env::var("CHLOEDB_HUB_SECRETS").unwrap_or_default();
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
            Secrets::Kube(s) => json!({"kind": "kubernetes", "what": format!("the Kubernetes Secret {s}, with the book, shared by the cluster's coordinators"), "secret": s}),
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
            Secrets::File(_) | Secrets::Kube(_) => return Err(format!("no key kept for {name}: add the connection again")),
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
                run("security", &["-i"], Some(&format!("add-generic-password -U -s {} -a {} -l {} -w {}\n", quote(SERVICE), quote(&acct), quote(&format!("ChloeDB hub: {name}")), quote(key))))?;
            }
            Secrets::SecretTool => {
                run("secret-tool", &["store", "--label", &format!("ChloeDB hub: {name}"), "service", SERVICE, "account", &acct], Some(key))?;
            }
            Secrets::File(_) | Secrets::Kube(_) => {}
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
            Secrets::File(_) | Secrets::Kube(_) => {}
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

/// A book kept in a Kubernetes Secret ([`Hub::in_secret`]): an entry per
/// connection, `c.<name>`, the connection and its key as JSON.
struct InSecret {
    kube: Kube,
    namespace: String,
    name: String,
    /// The changes to write, a connection at a time and in order: its
    /// name and its entry, or `None` for one taken out.
    tx: tokio::sync::mpsc::UnboundedSender<(String, Option<String>)>,
    rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<(String, Option<String>)>>>,
    /// Why the Secret was last not read or written, until it is.
    error: Mutex<Option<String>>,
}

/// How often a book kept in a Secret is read again, for the changes the
/// cluster's other coordinators made.
const REREAD: Duration = Duration::from_secs(20);

impl InSecret {
    fn path(&self) -> String {
        format!("/api/v1/namespaces/{}/secrets/{}", self.namespace, self.name)
    }

    fn noted(&self, r: Result<(), String>) {
        let mut e = self.error.lock().unwrap_or_else(|e| e.into_inner());
        match r {
            Ok(()) => *e = None,
            Err(m) => {
                if e.as_deref() != Some(m.as_str()) {
                    tracing::warn!(secret = %format!("{}/{}", self.namespace, self.name), error = %m, "the hub's book");
                }
                *e = Some(m);
            }
        }
    }

    /// Writes one connection's entry (`None`: takes it out). A merge
    /// patch changes that entry only, so that two coordinators changing
    /// two connections do not undo each other; a Secret not there yet is
    /// made.
    async fn write(&self, name: &str, entry: Option<String>) -> Result<(), String> {
        let field = format!("c.{name}");
        let patch = json!({"data": {field.clone(): entry.clone()}});
        if self.kube.merge(&self.path(), &patch).await?.is_some() {
            return Ok(());
        }
        let Some(entry) = entry else { return Ok(()) };
        let body = json!({"apiVersion": "v1", "kind": "Secret", "type": "Opaque", "metadata": {"name": self.name, "namespace": self.namespace}, "data": {field: entry}});
        match self.kube.call(reqwest::Method::POST, &format!("/api/v1/namespaces/{}/secrets", self.namespace), Some(&body)).await? {
            (200..=299, _) => Ok(()),
            // Made meanwhile by another coordinator, or the operator.
            (409, _) => self.kube.merge(&self.path(), &patch).await?.map(|_| ()).ok_or_else(|| format!("{} is gone", self.path())),
            (s, j) => Err(format!("making the Secret {}/{}: {s}: {}", self.namespace, self.name, j["message"].as_str().unwrap_or("refused"))),
        }
    }

    /// The connections and keys the Secret holds (none: no Secret yet).
    async fn read(&self) -> Result<Vec<(Connection, Option<String>)>, String> {
        let Some(secret) = self.kube.get(&self.path()).await? else { return Ok(Vec::new()) };
        Ok(entries(&secret))
    }
}

/// The connections of a Secret's entries, oldest first. An entry that is
/// not one is left out, and said.
fn entries(secret: &Json) -> Vec<(Connection, Option<String>)> {
    let mut out = Vec::new();
    for (k, v) in secret["data"].as_object().into_iter().flatten() {
        let Some(name) = k.strip_prefix("c.") else { continue };
        let entry = v.as_str().and_then(crate::operator::kube::base64_decode).and_then(|b| serde_json::from_slice::<Json>(&b).ok());
        let c = entry.as_ref().and_then(|e| serde_json::from_value::<Connection>(e["connection"].clone()).ok()).filter(|c| c.name == name);
        match (c, entry) {
            (Some(c), Some(e)) => out.push((c, e["key"].as_str().map(String::from))),
            _ => tracing::warn!(entry = %k, "the hub's Secret holds an entry that is not a connection: left out"),
        }
    }
    out.sort_by_key(|(c, _)| c.added_ms);
    out
}

/// The book and the keys.
pub struct Hub {
    book: Mutex<Vec<Connection>>,
    file: Option<PathBuf>,
    /// Kept in a Kubernetes Secret instead of `file`.
    secret: Option<InSecret>,
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

/// The name of the key a hub makes on a cluster it reaches through its
/// Kubernetes API: "hub <device>", cut to the 64 bytes a key's name may
/// have (a machine's name can be long: CI's macOS runner's is).
fn key_name(device: &str) -> String {
    let mut name = format!("hub {device}");
    let mut end = name.len().min(64);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    name.truncate(end);
    name
}

/// Who a hub on this machine is, by default.
pub fn default_device() -> String {
    let user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_else(|_| "someone".into());
    format!("{user}@{}", host_name())
}

impl Hub {
    /// A book in memory, the keys too.
    pub fn memory() -> Hub {
        Hub { book: Mutex::new(Vec::new()), file: None, secret: None, keys: Keys::new(Secrets::File(None), "memory".into()), clients: Mutex::new(HashMap::new()), device: default_device() }
    }

    /// The book kept in the Kubernetes Secret `namespace/name`, through
    /// `kube` (a coordinator's own account, [`crate::operator::kube::Link`]):
    /// read now, written as it changes and read again every few seconds
    /// once [`Hub::keep`] runs. A Secret not there yet is an empty book,
    /// made at its first change; one this account may not read is an
    /// error, and the caller keeps the book in memory.
    pub async fn in_secret(kube: Kube, namespace: &str, name: &str, device: Option<String>) -> Result<Hub, String> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let secret = InSecret { kube, namespace: namespace.to_string(), name: name.to_string(), tx, rx: Mutex::new(Some(rx)), error: Mutex::new(None) };
        let found = secret.read().await?;
        let keys = Keys::new(Secrets::Kube(format!("{namespace}/{name}")), format!("{namespace}/{name}"));
        let mut book = Vec::new();
        {
            let mut m = keys.lock();
            for (c, key) in found {
                if let Some(k) = key {
                    m.insert(c.name.clone(), k);
                }
                book.push(c);
            }
        }
        Ok(Hub { book: Mutex::new(book), file: None, secret: Some(secret), keys, clients: Mutex::new(HashMap::new()), device: device.filter(|d| !d.trim().is_empty()).unwrap_or_else(default_device) })
    }

    /// Keeps a book of [`Hub::in_secret`] in its Secret, in the
    /// background: writes each change in order, and reads the Secret again
    /// every [`REREAD`] for the changes of the cluster's other
    /// coordinators. Nothing for a book of a file or of memory.
    pub fn keep(hub: &Arc<Hub>) {
        let Some(mut rx) = hub.secret.as_ref().and_then(|s| s.rx.lock().unwrap_or_else(|e| e.into_inner()).take()) else { return };
        let weak = Arc::downgrade(hub);
        tokio::spawn(async move {
            let mut every = tokio::time::interval(REREAD);
            every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            every.tick().await;
            loop {
                tokio::select! {
                    biased;
                    change = rx.recv() => {
                        let Some((name, entry)) = change else { return };
                        let Some(hub) = weak.upgrade() else { return };
                        let Some(s) = &hub.secret else { return };
                        let r = s.write(&name, entry).await;
                        s.noted(r);
                    }
                    _ = every.tick() => {
                        let Some(hub) = weak.upgrade() else { return };
                        hub.reread(&rx).await;
                    }
                }
            }
        });
    }

    /// Reads the Secret again and takes what it holds as the book, unless
    /// a change of this hub's is still to be written (it is read again
    /// after). The connections' last answers and failures, which the
    /// Secret is not written for, are this hub's own.
    async fn reread(&self, pending: &tokio::sync::mpsc::UnboundedReceiver<(String, Option<String>)>) {
        let Some(s) = &self.secret else { return };
        let found = match s.read().await {
            Ok(f) => f,
            Err(e) => return s.noted(Err(e)),
        };
        s.noted(Ok(()));
        if !pending.is_empty() {
            return;
        }
        let mut list = self.lock();
        let old: HashMap<String, Connection> = list.drain(..).map(|c| (c.name.clone(), c)).collect();
        let mut keys = self.keys.lock();
        let mut clients = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        keys.clear();
        for (mut c, key) in found {
            match old.get(&c.name) {
                Some(o) => {
                    c.last_ok_ms = c.last_ok_ms.max(o.last_ok_ms);
                    c.last_error = o.last_error.clone();
                    if o.pin != c.pin || o.urls != c.urls {
                        clients.remove(&c.name);
                    }
                }
                None => drop(clients.remove(&c.name)),
            }
            if let Some(k) = key {
                keys.insert(c.name.clone(), k);
            }
            list.push(c);
        }
        for name in old.keys().filter(|n| !list.iter().any(|c| &c.name == *n)) {
            clients.remove(name);
        }
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
        Ok(Hub { book: Mutex::new(book), file: Some(file), secret: None, keys: Keys::new(secrets, hub), clients: Mutex::new(HashMap::new()), device: device.filter(|d| !d.trim().is_empty()).unwrap_or_else(default_device) })
    }

    /// The keys' file, for a book kept in `file` without a keychain.
    pub fn keys_file_of(file: &Path) -> PathBuf {
        let s = file.to_string_lossy();
        PathBuf::from(s.strip_suffix(".hub.json").map_or_else(|| format!("{s}.keys"), |b| format!("{b}.hub-keys.json")))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Connection>> {
        self.book.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Keeps the book after a change to the connection `name`: the whole
    /// book in its file, or that connection's entry in the Secret (its
    /// key with it), written in the background.
    fn save(&self, list: &[Connection], name: &str) -> Result<(), String> {
        if let Some(s) = &self.secret {
            let entry = list.iter().find(|c| c.name == name).map(|c| {
                let key = self.keys.lock().get(name).cloned();
                crate::operator::kube::base64_encode(json!({"connection": c, "key": key}).to_string().as_bytes())
            });
            return s.tx.send((name.to_string(), entry)).map_err(|_| "the hub's Secret is no longer written".to_string());
        }
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
        let mut keys = self.keys.secrets.describe();
        if let Some(e) = self.secret.as_ref().and_then(|s| s.error.lock().unwrap_or_else(|e| e.into_inner()).clone()) {
            keys["error"] = json!(e);
        }
        json!({
            "device": self.device,
            "keys": keys,
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
            return Err(ApiError { code: None, status: 409, message: format!("{} is in the book already: pick another name, or replace it", a.name) });
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
                return Err(ApiError { code: None, status: 409, message: format!("{u} is plain HTTP beyond this machine: the key and the rows would cross the network readable. Give the cluster HTTPS, or allow plain HTTP for this connection") });
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
                    (Err(e), _) => return Err(ApiError { code: None, status: 502, message: format!("{first} does not answer: {}", error_text(&e)) }),
                    (Ok(_), None) => return Err(ApiError { code: None, status: 502, message: format!("{first} answered without a certificate") }),
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
            kube: None,
            keyless: false,
        };
        let who = self.whoami(&c, Some(key), None).await?;
        let c = Connection { cluster: who["cluster"].clone(), key: key_view(&who), last_ok_ms: Some(crate::operator::now_ms()), ..c };
        self.keys.set(&c.name, key).map_err(|e| ApiError::internal(format!("keeping the key: {e}")))?;
        let mut list = self.lock();
        list.retain(|o| o.name != c.name);
        list.push(c.clone());
        self.save(&list, &c.name).map_err(ApiError::internal)?;
        drop(list);
        self.clients.lock().unwrap_or_else(|e| e.into_inner()).remove(&c.name);
        Ok(Added::Done(c))
    }

    /// `/v1/whoami` at the connection, with `key`: refused unless it
    /// answers as Strata and takes the key.
    async fn whoami(&self, c: &Connection, key: Option<&str>, kube: Option<&Kube>) -> Result<Json, ApiError> {
        if let Some(v) = &c.kube {
            let k = kube.ok_or_else(|| no_kube(&c.name))?;
            let mut rb = k.request(reqwest::Method::GET, &format!("{}/v1/whoami", v.proxy()), CHECK_TIMEOUT).header("x-strata-client", "hub");
            if let Some(key) = key {
                rb = rb.header(VIA_PROXY_AUTH, format!("Bearer {key}"));
            }
            let r = rb.send().await.map_err(|e| ApiError { code: None, status: 502, message: format!("{} does not answer: {}", v.describe(), error_text(&e)) })?;
            let status = r.status().as_u16();
            let body = r.bytes().await.map_err(|e| ApiError { code: None, status: 502, message: format!("{}: {e}", v.describe()) })?;
            if let Some(m) = kube_status(&body).filter(|_| status >= 400) {
                return Err(ApiError { code: None, status: 502, message: format!("{} ({status}): {m}", v.describe()) });
            }
            return match status {
                200..=299 => serde_json::from_slice::<Json>(&body).ok().filter(|j| j.get("scope").is_some()).ok_or_else(|| ApiError { code: None, status: 502, message: format!("{} did not answer as ChloeDB does", c.name) }),
                401 | 403 => Err(ApiError { code: None, status: 409, message: format!("{} refused the key ({status}): check it, and that it has not expired", c.name) }),
                s => Err(ApiError { code: None, status: 502, message: format!("{} answered {s} through {}", c.name, v.describe()) }),
            };
        }
        let key = key.unwrap_or_default();
        let cl = match &c.pin {
            Some(p) => pinned_client(Some(p.clone()), Some(CHECK_TIMEOUT)).map_err(ApiError::internal)?.0,
            None => crate::http_client().timeout(CHECK_TIMEOUT).build().map_err(|e| ApiError::internal(e.to_string()))?,
        };
        let mut last = String::new();
        for u in &c.urls {
            match cl.get(format!("{u}/v1/whoami")).bearer_auth(key).header("x-strata-client", "hub").send().await {
                Ok(r) if r.status().is_success() => {
                    let j: Json = r.json().await.map_err(|e| ApiError { code: None, status: 502, message: format!("{u} did not answer as ChloeDB does: {e}") })?;
                    if j.get("scope").is_none() {
                        return Err(ApiError { code: None, status: 502, message: format!("{u} did not answer as ChloeDB does") });
                    }
                    if j["auth"]["open"] == true && !loopback(&reqwest::Url::parse(u).map_err(|e| ApiError::bad(e.to_string()))?) {
                        tracing::warn!(connection = %c.name, url = %u, "this cluster takes calls without a key");
                    }
                    return Ok(j);
                }
                Ok(r) if r.status().as_u16() == 401 || r.status().as_u16() == 403 => return Err(ApiError { code: None, status: 409, message: format!("{u} refused the key ({}): check it, and that it has not expired", r.status().as_u16()) }),
                Ok(r) => last = format!("{u} answered {}", r.status()),
                Err(e) => last = format!("{u} does not answer: {}", error_text(&e)),
            }
        }
        Err(ApiError { code: None, status: 502, message: last })
    }

    /// The key kept for a connection; none for one without
    /// ([`Connection::keyless`]).
    fn key_of(&self, c: &Connection) -> Result<Option<String>, ApiError> {
        if c.keyless {
            return Ok(None);
        }
        self.keys.get(&c.name).map(Some).map_err(ApiError::internal)
    }

    /// Makes the cluster made here that `via` names a connection of this
    /// book, through its Kubernetes API `k`: the cluster's token is read
    /// from its Secret there, makes a key of this hub's own (admin, named
    /// `hub <device>`, so that it can be revoked alone and the cluster's
    /// audit names this hub), and only that key is kept. A cluster without
    /// a token is kept without a key. A cluster in the book already is
    /// checked again, not added twice.
    pub async fn adopt(&self, via: ViaKube, name: Option<String>, color: Option<String>, k: &Kube, by: &str) -> Result<Connection, ApiError> {
        if let Some(c) = self.list().into_iter().find(|c| c.kube.as_ref().is_some_and(|v| v.made == via.made)) {
            return self.check(&c.name, Some(k)).await;
        }
        let name = name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| via.service.clone());
        check_name(&name)?;
        if self.get(&name).is_some() {
            return Err(ApiError { code: None, status: 409, message: format!("{name} is in the book already: pick another name") });
        }
        let gone = |e: String| ApiError { code: None, status: 502, message: format!("{}: {e}", via.describe()) };
        let sc = k.get(&format!("/apis/strata.chloetech.ai/v1alpha1/namespaces/{}/strataclusters/{}", via.namespace, via.service)).await.map_err(gone)?;
        let sc = sc.ok_or_else(|| ApiError { code: None, status: 404, message: format!("{} has no StrataCluster {}/{}", via.cluster, via.namespace, via.service) })?;
        let root = match sc["spec"]["tokenSecret"].as_str() {
            Some(secret) => {
                let sec = k.get(&format!("/api/v1/namespaces/{}/secrets/{secret}", via.namespace)).await.map_err(gone)?;
                let b64 = sec.as_ref().and_then(|s| s["data"]["token"].as_str()).ok_or_else(|| ApiError { code: None, status: 502, message: format!("{} has no token in its Secret {secret}", via.cluster) })?;
                use base64::Engine;
                let t = base64::engine::general_purpose::STANDARD.decode(b64).map_err(|e| ApiError { code: None, status: 502, message: format!("the Secret {secret}: {e}") })?;
                Some(String::from_utf8(t).map_err(|_| ApiError { code: None, status: 502, message: format!("the Secret {secret} is not a token") })?.trim().to_string())
            }
            None => None,
        };
        let c = Connection {
            name: name.clone(),
            color: color.filter(|c| c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit())).unwrap_or_else(|| COLORS[self.lock().len() % COLORS.len()].to_string()),
            urls: Vec::new(),
            pin: None,
            allow_plain: false,
            cluster: Json::Null,
            key: Json::Null,
            added_ms: crate::operator::now_ms(),
            added_by: by.to_string(),
            last_ok_ms: None,
            last_error: None,
            kube: Some(via.clone()),
            keyless: root.is_none(),
        };
        let key = match &root {
            Some(root) => {
                let body = json!({"name": key_name(&self.device), "scope": "admin", "owner": by, "note": format!("the command center of {} reaches this cluster with it, through its Kubernetes API", self.device)});
                let r = k
                    .request(reqwest::Method::POST, &format!("{}/v1/admin/keys", via.proxy()), CHECK_TIMEOUT)
                    .header(VIA_PROXY_AUTH, format!("Bearer {root}"))
                    .header("x-strata-via", format!("hub {}", self.device))
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| ApiError { code: None, status: 502, message: format!("{} does not answer: {}", via.describe(), error_text(&e)) })?;
                let status = r.status().as_u16();
                let b = r.bytes().await.unwrap_or_default();
                if !(200..300).contains(&status) {
                    let why = kube_status(&b).or_else(|| serde_json::from_slice::<Json>(&b).ok().and_then(|j| j["error"].as_str().map(String::from))).unwrap_or_else(|| format!("{status}"));
                    return Err(ApiError { code: None, status: 502, message: format!("{name} did not make a key for this hub: {why}") });
                }
                let j: Json = serde_json::from_slice(&b).map_err(|e| ApiError { code: None, status: 502, message: format!("{name}: {e}") })?;
                Some(j["secret"].as_str().ok_or_else(|| ApiError { code: None, status: 502, message: format!("{name} made no key") })?.to_string())
            }
            None => None,
        };
        let who = self.whoami(&c, key.as_deref(), Some(k)).await?;
        let c = Connection { cluster: who["cluster"].clone(), key: key_view(&who), last_ok_ms: Some(crate::operator::now_ms()), ..c };
        if let Some(key) = &key {
            self.keys.set(&c.name, key).map_err(|e| ApiError::internal(format!("keeping the key: {e}")))?;
        }
        let mut list = self.lock();
        list.retain(|o| o.name != c.name);
        list.push(c.clone());
        self.save(&list, &c.name).map_err(ApiError::internal)?;
        drop(list);
        Ok(c)
    }

    /// Asks the connection again who its key is, and notes when it
    /// answered.
    pub async fn check(&self, name: &str, kube: Option<&Kube>) -> Result<Connection, ApiError> {
        let c = self.get(name).ok_or_else(|| not_found(name))?;
        let key = self.key_of(&c)?;
        let r = self.whoami(&c, key.as_deref(), kube).await;
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
        let _ = self.save(&list, name);
        r.map(|_| out)
    }

    /// Changes a connection's color or key (a new key is checked first).
    pub async fn update(&self, name: &str, b: &Json, kube: Option<&Kube>) -> Result<Connection, ApiError> {
        let c = self.get(name).ok_or_else(|| not_found(name))?;
        if let Some(k) = b.get("key").and_then(Json::as_str).map(str::trim).filter(|k| !k.is_empty()) {
            let who = self.whoami(&c, Some(k), kube).await?;
            self.keys.set(name, k).map_err(|e| ApiError::internal(format!("keeping the key: {e}")))?;
            let mut list = self.lock();
            if let Some(e) = list.iter_mut().find(|o| o.name == name) {
                e.key = key_view(&who);
                e.cluster = who["cluster"].clone();
                e.keyless = false;
            }
            self.save(&list, name).map_err(ApiError::internal)?;
        }
        if let Some(color) = b.get("color").and_then(Json::as_str) {
            if !(color.len() == 7 && color.starts_with('#') && color[1..].bytes().all(|b| b.is_ascii_hexdigit())) {
                return Err(ApiError::bad("a color is #rrggbb"));
            }
            let mut list = self.lock();
            if let Some(e) = list.iter_mut().find(|o| o.name == name) {
                e.color = color.to_string();
            }
            self.save(&list, name).map_err(ApiError::internal)?;
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
        self.save(&list, name).map_err(ApiError::internal)?;
        drop(list);
        if !c.keyless {
            self.keys.remove(name).map_err(ApiError::internal)?;
        }
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
        // A book in a Secret is not written for an answer: its
        // coordinators each note their own.
        if self.secret.is_none() {
            let _ = self.save(&list, name);
        }
    }

    /// Sends a request to the connection `name`, at `path` (`/v1/...`,
    /// with its query), with that connection's key in place of the
    /// caller's credential: the answer as the cluster gave it, but a
    /// refused key, which is answered 502 so that the page asks for no
    /// login of this hub's.
    pub async fn forward(&self, name: &str, method: reqwest::Method, path: &str, headers: &axum::http::HeaderMap, body: reqwest::Body, kube: Option<&Kube>) -> Result<axum::response::Response, ApiError> {
        let c = self.get(name).ok_or_else(|| not_found(name))?;
        let key = self.key_of(&c)?;
        if let Some(v) = &c.kube {
            let k = kube.ok_or_else(|| no_kube(name))?;
            let mut rb = k.request(method, &format!("{}{path}", v.proxy()), KUBE_TIMEOUT).header("x-strata-via", format!("hub {}", self.device));
            if let Some(key) = &key {
                rb = rb.header(VIA_PROXY_AUTH, format!("Bearer {key}"));
            }
            for (k, val) in headers {
                if PASSED.iter().any(|p| k.as_str().eq_ignore_ascii_case(p)) {
                    rb = rb.header(k.as_str(), val.as_bytes());
                }
            }
            return match rb.body(body).send().await {
                Ok(r) => {
                    let out = answer_kube(name, v, r).await;
                    self.noted(name, &if out.status().as_u16() == 502 { Err(format!("{name}: refused or not answering through {}", v.describe())) } else { Ok(()) });
                    Ok(out)
                }
                Err(e) => {
                    let m = format!("{} does not answer: {}", v.describe(), error_text(&e));
                    self.noted(name, &Err(m.clone()));
                    Err(ApiError { code: None, status: 502, message: m })
                }
            };
        }
        let key = key.unwrap_or_default();
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
        Err(ApiError { code: None, status: 502, message: last })
    }
}

/// The request headers a forwarded call keeps: what the body is and what
/// the command center says of the call. Not the caller's credential,
/// cookies, `Host` or `Origin`.
const PASSED: [&str; 10] = [
    "content-type", "accept", "x-strata-client", "x-strata-reason", "x-request-id", "idempotency-key", "x-strata-confirm",
    "x-chloedb-client", "x-chloedb-reason", "x-chloedb-confirm",
];

/// The answer headers passed back.
const RETURNED: [&str; 7] = ["content-type", "content-disposition", "x-request-id", "retry-after", "cache-control", "x-strata-shards", "x-chloedb-shards"];

fn answer(name: &str, r: reqwest::Response) -> axum::response::Response {
    use axum::response::IntoResponse;
    let status = r.status().as_u16();
    if status == 401 || status == 403 {
        let why = format!("{name} refused this hub's key ({status}): it may have expired or been revoked; put a new one in Connections");
        return ApiError { code: None, status: 502, message: why }.into_response();
    }
    let mut out = axum::response::Response::builder().status(status);
    for (k, v) in r.headers() {
        if RETURNED.iter().any(|p| k.as_str().eq_ignore_ascii_case(p)) {
            out = out.header(k.as_str(), v.as_bytes());
        }
    }
    out.body(axum::body::Body::from_stream(r.bytes_stream())).unwrap_or_else(|e| ApiError::internal(e.to_string()).into_response())
}

/// The answer of a call through a Kubernetes API: the cluster's, but what
/// the API server says for itself (its own refusal, no copy behind the
/// Service) and a refused key, which answer 502 with what went wrong.
async fn answer_kube(name: &str, v: &ViaKube, r: reqwest::Response) -> axum::response::Response {
    use axum::response::IntoResponse;
    let status = r.status().as_u16();
    if status < 400 {
        return answer(name, r);
    }
    let headers = r.headers().clone();
    let body = r.bytes().await.unwrap_or_default();
    if let Some(m) = kube_status(&body) {
        return ApiError { code: None, status: 502, message: format!("{} ({status}): {m}", v.describe()) }.into_response();
    }
    if status == 401 || status == 403 {
        return ApiError { code: None, status: 502, message: format!("{name} refused this hub's key ({status}): it may have expired or been revoked; open it again from Cloud setup") }.into_response();
    }
    let mut out = axum::response::Response::builder().status(status);
    for (k, val) in &headers {
        if RETURNED.iter().any(|p| k.as_str().eq_ignore_ascii_case(p)) {
            out = out.header(k.as_str(), val.as_bytes());
        }
    }
    out.body(axum::body::Body::from(body)).unwrap_or_else(|e| ApiError::internal(e.to_string()).into_response())
}

fn no_kube(name: &str) -> ApiError {
    ApiError::internal(format!("{name} is reached through a Kubernetes API, and no client of it was given"))
}

/// What the book keeps of a key, from `/v1/whoami`: never its secret.
fn key_view(who: &Json) -> Json {
    json!({"name": who["name"], "id": who["key"], "scope": who["scope"], "root": who["root"], "owner": who["owner"], "expires_ms": who["expires_ms"]})
}

fn not_found(name: &str) -> ApiError {
    ApiError { code: None, status: 404, message: format!("no connection {name} in this hub's book") }
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
        assert_eq!(key_name("ana@laptop"), "hub ana@laptop");
        // "é" takes bytes 64 and 65 of the name: the cut goes before it.
        let long = format!("runner@{}éxyz", "m".repeat(52));
        assert_eq!(key_name(&long), format!("hub runner@{}", "m".repeat(52)));
        assert_eq!(key_name(&format!("runner@{}", "m".repeat(80))).len(), 64);
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

    /// A Kubernetes API that holds one Secret (`db/orders-hub`): read,
    /// made, and changed by merge patches of its data.
    async fn fake_kube() -> (String, Arc<Mutex<Option<Json>>>) {
        use axum::http::StatusCode;
        let held: Arc<Mutex<Option<Json>>> = Arc::default();
        let h = held.clone();
        let app = axum::Router::new().fallback(move |method: axum::http::Method, uri: axum::http::Uri, body: axum::body::Bytes| {
            let h = h.clone();
            async move {
                let body: Json = serde_json::from_slice(&body).unwrap_or(Json::Null);
                let mut held = h.lock().unwrap();
                match (method.as_str(), uri.path()) {
                    ("GET", "/api/v1/namespaces/db/secrets/orders-hub") => match &*held {
                        Some(s) => (StatusCode::OK, axum::Json(s.clone())),
                        None => (StatusCode::NOT_FOUND, axum::Json(json!({"kind": "Status", "message": "not found"}))),
                    },
                    ("PATCH", "/api/v1/namespaces/db/secrets/orders-hub") => match held.as_mut() {
                        Some(s) => {
                            for (k, v) in body["data"].as_object().unwrap() {
                                match v {
                                    Json::Null => drop(s["data"].as_object_mut().unwrap().remove(k)),
                                    v => s["data"][k] = v.clone(),
                                }
                            }
                            (StatusCode::OK, axum::Json(s.clone()))
                        }
                        None => (StatusCode::NOT_FOUND, axum::Json(json!({"kind": "Status", "message": "not found"}))),
                    },
                    ("POST", "/api/v1/namespaces/db/secrets") if held.is_none() => {
                        assert_eq!(body["metadata"]["name"], "orders-hub");
                        *held = Some(body.clone());
                        (StatusCode::CREATED, axum::Json(body))
                    }
                    _ => (StatusCode::CONFLICT, axum::Json(json!({"kind": "Status", "message": "no"}))),
                }
            }
        });
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await });
        (url, held)
    }

    /// A cluster that takes the key `sk_far` and says who it is.
    async fn fake_cluster() -> String {
        let app = axum::Router::new().route(
            "/v1/whoami",
            axum::routing::get(|headers: axum::http::HeaderMap| async move {
                if headers.get("authorization").and_then(|v| v.to_str().ok()) == Some("Bearer sk_far") {
                    (axum::http::StatusCode::OK, axum::Json(json!({"scope": "admin", "name": "hub key", "cluster": {"name": "far", "environment": "prod"}})))
                } else {
                    (axum::http::StatusCode::UNAUTHORIZED, axum::Json(json!({"error": "no"})))
                }
            }),
        );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await });
        url
    }

    async fn until(what: &str, f: impl Fn() -> bool) {
        for _ in 0..100 {
            if f() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("never: {what}");
    }

    /// A coordinator's book in its cluster's Secret: what one coordinator
    /// adds, another (or the same after a restart) finds with its key, and
    /// what it takes out is gone from the Secret.
    #[tokio::test]
    async fn a_book_in_a_secret_outlives_its_coordinator() {
        use crate::operator::kube::{Kube, Options};
        let (api, held) = fake_kube().await;
        let far = fake_cluster().await;
        let kube = || Kube::new(&Options { api: Some(api.clone()), token: Some("t".into()), ..Default::default() }).unwrap();
        let a = Arc::new(Hub::in_secret(kube(), "db", "orders-hub", Some("db/orders".into())).await.unwrap());
        Hub::keep(&a);
        assert!(a.list().is_empty(), "no Secret yet: an empty book");
        assert_eq!(a.view()["keys"]["kind"], "kubernetes");
        assert_eq!(a.device, "db/orders");
        let added = a.add(Add { name: "far".into(), url: far.clone(), key: "sk_far".into(), color: Some("#112233".into()), ..Default::default() }, "ana").await.unwrap();
        assert!(matches!(added, Added::Done(_)));
        until("the Secret is made with the connection", || held.lock().unwrap().as_ref().is_some_and(|s| s["data"]["c.far"].is_string())).await;
        let entry = held.lock().unwrap().as_ref().unwrap()["data"]["c.far"].as_str().unwrap().to_string();
        let entry: Json = serde_json::from_slice(&crate::operator::kube::base64_decode(&entry).unwrap()).unwrap();
        assert_eq!((&entry["key"], &entry["connection"]["urls"][0], &entry["connection"]["color"]), (&json!("sk_far"), &json!(far), &json!("#112233")));

        // Another coordinator, or this one restarted: the connection, with
        // its key.
        let b = Arc::new(Hub::in_secret(kube(), "db", "orders-hub", None).await.unwrap());
        Hub::keep(&b);
        let c = b.get("far").expect("the connection");
        assert_eq!((c.cluster["environment"].as_str(), b.key_of(&c).unwrap().as_deref()), (Some("prod"), Some("sk_far")));
        b.check("far", None).await.expect("its key works");
        b.update("far", &json!({"color": "#445566"}), None).await.unwrap();
        until("the color is written", || {
            let s = held.lock().unwrap();
            let e = s.as_ref().unwrap()["data"]["c.far"].as_str().unwrap().to_string();
            String::from_utf8(crate::operator::kube::base64_decode(&e).unwrap()).unwrap().contains("#445566")
        })
        .await;

        // The first reads it again, and finds the second's change; then
        // takes the connection out, and the second finds it gone.
        let (_tx, none) = tokio::sync::mpsc::unbounded_channel();
        a.reread(&none).await;
        assert_eq!(a.get("far").unwrap().color, "#445566");
        a.remove("far").unwrap();
        until("the entry is taken out", || held.lock().unwrap().as_ref().unwrap()["data"].get("c.far").is_none()).await;
        b.reread(&none).await;
        assert!(b.get("far").is_none() && b.keys.lock().is_empty(), "its key with it");
        assert!(b.view()["keys"].get("error").is_none());
    }

    /// An entry of the Secret that is not a connection (written by hand,
    /// or under another name) is left out, not taken as one.
    #[test]
    fn entries_of_a_secret() {
        let c = |name: &str, at: u64| json!({"name": name, "color": "#000000", "urls": ["http://x:7700"], "added_ms": at, "added_by": "ana"});
        let enc = |j: Json| crate::operator::kube::base64_encode(j.to_string().as_bytes());
        let secret = json!({"data": {
            "c.b": enc(json!({"connection": c("b", 1), "key": "kb"})),
            "c.a": enc(json!({"connection": c("a", 2), "key": null})),
            "c.wrong": enc(json!({"connection": c("other", 3), "key": "k"})),
            "c.junk": "not base64 json",
            "notes": enc(json!({})),
        }});
        let got: Vec<(String, Option<String>)> = entries(&secret).into_iter().map(|(c, k)| (c.name, k)).collect();
        assert_eq!(got, vec![("b".to_string(), Some("kb".to_string())), ("a".to_string(), None)], "oldest first");
    }
}
