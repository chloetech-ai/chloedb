//! Google Cloud for the command center's cloud view: the credentials the
//! calls are made with, the project they are made in, the machine types a
//! GKE node may be in a region with their prices, the project's quotas,
//! and the plan of a GKE cluster for ChloeDB ([`plan`]).
//!
//! No key of a service account is ever taken (an organisation's policy
//! `iam.managed.disableServiceAccountKeyCreation` forbids them, and a key
//! kept on a laptop is a key that leaks). The calls' credentials are, in
//! order:
//!
//! - gcloud's Application Default Credentials: the file
//!   `gcloud auth application-default login` writes
//!   (`GOOGLE_APPLICATION_CREDENTIALS` names another), a person's refresh
//!   token, which impersonates a service account when it was made with
//!   `--impersonate-service-account` (each token is then the service
//!   account's, for an hour, through IAM Credentials'
//!   `generateAccessToken`);
//! - workload identity federation, in the same file
//!   (`external_account`): on a Kubernetes cluster outside Google Cloud
//!   (DigitalOcean's, EKS, ...), a token Kubernetes gives the pod, which
//!   Google's Security Token Service trusts through a pool's provider and
//!   exchanges for a token of a service account ([`federation_commands`]
//!   sets it up; the operator's `gcpFederation` mounts both);
//! - the metadata server: the service account attached to a Compute
//!   Engine VM, or on GKE the one Workload Identity binds to the pod's
//!   Kubernetes account. A cluster this server made then manages itself
//!   from its own command center, with no laptop.
//!
//! Neither is kept here: this server keeps the project and its region in
//! `<db>.gcp.json` (beside a database; in memory on a coordinator without
//! one), which hold no secret, and access tokens in memory until they
//! expire. No answer of the routes (`/v1/admin/gcp`) holds a token.

pub mod catalog;
pub mod plan;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

pub use catalog::Catalog;

use crate::aws::write_private;
use crate::telemetry::now_ms;

/// What every token is asked for.
pub const SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

/// The metadata server of a Compute Engine VM or a GKE pod.
const METADATA: &str = "http://metadata.google.internal";

/// gcloud's default token endpoint for a person's refresh token.
const TOKEN_URI: &str = "https://oauth2.googleapis.com/token";

/// How long before it expires a token is no longer handed out: a
/// cluster's API client made with one is kept 5 minutes
/// ([`crate::cloud::clusters::Clusters::kube_of`]).
const TOKEN_MARGIN_MS: u64 = 10 * 60 * 1000;

/// What the calls are made as, read from where it is kept: never a secret
/// in an answer, and nothing that holds one is `Debug`.
#[derive(Clone)]
enum Adc {
    /// A person's refresh token (`authorized_user`).
    User { client_id: String, client_secret: String, refresh_token: String, token_uri: String },
    /// A service account a person's credentials impersonate
    /// (`impersonated_service_account`).
    Impersonated { source: Box<Adc>, url: String, email: String, delegates: Vec<String> },
    /// Workload identity federation (`external_account`): a token from
    /// `subject`, exchanged at Google's Security Token Service for one of
    /// the pool's principal, which impersonates a service account when the
    /// file names one.
    External(Box<External>),
}

#[derive(Clone)]
struct External {
    /// The pool's provider:
    /// `//iam.googleapis.com/projects/NUMBER/locations/global/workloadIdentityPools/POOL/providers/PROVIDER`.
    audience: String,
    subject_token_type: String,
    token_url: String,
    subject: Subject,
    /// The service account's impersonation URL, its address, and how long
    /// its tokens last (seconds).
    impersonate: Option<(String, String, u64)>,
    /// A workforce pool's project, which its calls are billed to.
    user_project: Option<String>,
}

/// Where a federation's subject token is read: a file (a projected
/// Kubernetes token), or a URL; the whole answer, or one field of it as
/// JSON.
#[derive(Clone)]
enum Subject {
    File { path: PathBuf, field: Option<String> },
    Url { url: String, headers: Vec<(String, String)>, field: Option<String> },
}

impl Subject {
    fn describe(&self) -> String {
        match self {
            Subject::File { path, .. } => format!("the file {}", path.display()),
            Subject::Url { url, .. } => format!("the URL {url}"),
        }
    }
}

/// The pool and the provider of a federation's audience.
pub fn pool_of(audience: &str) -> Option<(String, String)> {
    let rest = audience.split("/workloadIdentityPools/").nth(1).or_else(|| audience.split("/workforcePools/").nth(1))?;
    let (pool, provider) = rest.split_once("/providers/")?;
    (!pool.is_empty() && !provider.is_empty()).then(|| (pool.to_string(), provider.to_string()))
}

/// Whether `url` is an https URL of one of Google's APIs: a federation's
/// file names where its tokens go, and they go nowhere else.
fn google_api_url(url: &str, service: &str) -> bool {
    url.strip_prefix("https://").and_then(|r| r.split('/').next()).is_some_and(|host| host == format!("{service}.googleapis.com") || host.ends_with(&format!(".{service}.googleapis.com")) || host == format!("{service}.mtls.googleapis.com"))
}

/// A federation's file (`external_account`), as
/// `gcloud iam workload-identity-pools create-cred-config` writes it.
fn parse_external(j: &Json) -> Result<Adc, String> {
    let s = |k: &str| j[k].as_str().unwrap_or_default().to_string();
    let audience = s("audience");
    if pool_of(&audience).is_none() {
        return Err(format!("the federation's file names no pool's provider as its audience ({audience:?}): make it with gcloud iam workload-identity-pools create-cred-config"));
    }
    if let Some(u) = j["universe_domain"].as_str().filter(|u| *u != "googleapis.com") {
        return Err(format!("the federation's file is for the universe {u:?}: only googleapis.com is supported"));
    }
    let subject_token_type = s("subject_token_type");
    if subject_token_type.is_empty() {
        return Err("the federation's file says no subject_token_type".into());
    }
    let token_url = Some(s("token_url")).filter(|u| !u.is_empty()).unwrap_or_else(|| "https://sts.googleapis.com/v1/token".into());
    if !google_api_url(&token_url, "sts") {
        return Err(format!("the federation's token_url {token_url:?} is not Google's Security Token Service (https://sts.googleapis.com): its tokens go nowhere else"));
    }
    let c = &j["credential_source"];
    if c.get("executable").is_some() {
        return Err("the federation's file runs a program for its token (credential_source.executable): ChloeDB runs none; give the token in a file (credential_source.file), such as a projected Kubernetes token".into());
    }
    if c.get("environment_id").is_some() {
        return Err(format!(
            "the federation's file takes the token from {} (credential_source.environment_id): not supported; on EKS or any Kubernetes cluster, give the pod's own token in a file (credential_source.file, the operator's gcpFederation)",
            c["environment_id"].as_str().unwrap_or("a cloud's own credentials")
        ));
    }
    let field = match c["format"]["type"].as_str().unwrap_or("text") {
        "text" => None,
        "json" => Some(c["format"]["subject_token_field_name"].as_str().filter(|f| !f.is_empty()).ok_or("the federation's file reads its token as JSON but names no subject_token_field_name")?.to_string()),
        t => return Err(format!("the federation's token format {t:?} is not text or json")),
    };
    let subject = if let Some(f) = c["file"].as_str().filter(|f| !f.is_empty()) {
        Subject::File { path: PathBuf::from(f), field }
    } else if let Some(u) = c["url"].as_str().filter(|u| !u.is_empty()) {
        let headers = c["headers"].as_object().map(|h| h.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string()))).collect()).unwrap_or_default();
        Subject::Url { url: u.to_string(), headers, field }
    } else {
        return Err("the federation's file says not where its token is (credential_source.file or url)".into());
    };
    let impersonate = match Some(s("service_account_impersonation_url")).filter(|u| !u.is_empty()) {
        None => None,
        Some(url) => {
            if !google_api_url(&url, "iamcredentials") {
                return Err(format!("the federation's service_account_impersonation_url {url:?} is not IAM Credentials' (https://iamcredentials.googleapis.com)"));
            }
            let email = impersonated_email(&url).ok_or_else(|| format!("the federation's file names no service account to impersonate ({url:?})"))?;
            let lifetime = j["service_account_impersonation"]["token_lifetime_seconds"].as_u64().unwrap_or(3600);
            if !(600..=43200).contains(&lifetime) {
                return Err(format!("the federation's token_lifetime_seconds is {lifetime}: from 600 to 43200"));
            }
            Some((url, email, lifetime))
        }
    };
    let user_project = Some(s("workforce_pool_user_project")).filter(|p| !p.is_empty());
    Ok(Adc::External(Box::new(External { audience, subject_token_type, token_url, subject, impersonate, user_project })))
}

impl Adc {
    /// The service account the calls are made as, if they are a service
    /// account's.
    fn service_account(&self) -> Option<String> {
        match self {
            Adc::Impersonated { email, .. } => Some(email.clone()),
            Adc::External(x) => x.impersonate.as_ref().map(|(_, e, _)| e.clone()),
            Adc::User { .. } => None,
        }
    }
}

/// The ADC file's credentials, or why they are not taken.
fn parse_adc(j: &Json) -> Result<Adc, String> {
    let s = |k: &str| j[k].as_str().unwrap_or_default().to_string();
    match j["type"].as_str().unwrap_or_default() {
        "authorized_user" => {
            let (client_id, client_secret, refresh_token) = (s("client_id"), s("client_secret"), s("refresh_token"));
            if client_id.is_empty() || refresh_token.is_empty() {
                return Err("the credentials file has no client_id or refresh_token: sign in again with gcloud auth application-default login".into());
            }
            let token_uri = Some(s("token_uri")).filter(|u| !u.is_empty()).unwrap_or_else(|| TOKEN_URI.into());
            Ok(Adc::User { client_id, client_secret, refresh_token, token_uri })
        }
        "impersonated_service_account" => {
            let url = s("service_account_impersonation_url");
            let email = impersonated_email(&url).ok_or_else(|| format!("the credentials file names no service account to impersonate ({url:?})"))?;
            let source = parse_adc(&j["source_credentials"]).map_err(|e| format!("the credentials impersonating {email}: {e}"))?;
            let delegates = j["delegates"].as_array().map(|d| d.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
            Ok(Adc::Impersonated { source: Box::new(source), url, email, delegates })
        }
        "service_account" => Err(format!(
            "the credentials file is the key of the service account {}: ChloeDB takes no service account keys. Sign in as yourself, impersonating it: gcloud auth application-default login --impersonate-service-account {}",
            s("client_email"),
            Some(s("client_email")).filter(|e| !e.is_empty()).unwrap_or_else(|| "NAME@PROJECT.iam.gserviceaccount.com".into())
        )),
        "external_account" => parse_external(j),
        "external_account_authorized_user" => Err("the credentials file is a person's workforce identity sign-in (external_account_authorized_user): not supported; sign in with gcloud auth application-default login --impersonate-service-account, or give this server a workload identity federation's file".into()),
        "" => Err("the credentials file says no type: sign in again with gcloud auth application-default login".into()),
        t => Err(format!("credentials of the type {t:?} are not supported")),
    }
}

/// The service account of an impersonation URL:
/// `https://iamcredentials.googleapis.com/v1/projects/-/serviceAccounts/NAME@P.iam.gserviceaccount.com:generateAccessToken`.
pub fn impersonated_email(url: &str) -> Option<String> {
    let rest = url.split("/serviceAccounts/").nth(1)?;
    let email = rest.split(':').next()?;
    (email.contains('@')).then(|| email.to_string())
}

/// The project of a service account's address
/// (`NAME@PROJECT.iam.gserviceaccount.com`), if it is one a project made.
pub fn project_of(email: &str) -> Option<String> {
    let domain = email.split('@').nth(1)?;
    domain.strip_suffix(".iam.gserviceaccount.com").map(str::to_string)
}

/// A project id: 6 to 30 lower-case letters, digits and hyphens, starting
/// with a letter and not ending with a hyphen.
pub fn check_project(p: &str) -> Result<(), String> {
    let ok = (6..=30).contains(&p.len()) && p.starts_with(|c: char| c.is_ascii_lowercase()) && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') && !p.ends_with('-');
    if ok {
        Ok(())
    } else {
        Err(format!("{p:?} is not a project id: 6 to 30 lower-case letters, digits and hyphens, starting with a letter (the ID, not the name or the number)"))
    }
}

/// A region (`us-central1`) or a zone (`us-central1-a`): lower-case
/// letters, digits and hyphens.
fn check_location(l: &str) -> Result<(), String> {
    if !l.is_empty() && l.len() <= 40 && l.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') && l.contains('-') {
        Ok(())
    } else {
        Err(format!("{l:?} is not a region (like us-central1)"))
    }
}

/// What the metadata server says of where this server runs.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Metadata {
    /// The service account its tokens are: the VM's attached one, or the
    /// one Workload Identity binds the pod's Kubernetes account to.
    pub email: String,
    pub project: Option<String>,
    /// On GKE: the cluster this server runs on, and where.
    pub cluster: Option<String>,
    pub cluster_location: Option<String>,
}

/// The project kept here, and what is known of it. No secret.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Saved {
    pub project: String,
    pub region: String,
    /// Who the calls were made as when it was checked.
    pub account: String,
    /// The project's number (the Compute Engine default service account is
    /// `NUMBER-compute@developer.gserviceaccount.com`).
    #[serde(default)]
    pub number: String,
    pub saved_ms: u64,
    pub by: String,
}

/// Where the ADC file is looked for.
#[derive(Clone, Debug)]
enum AdcPath {
    /// `GOOGLE_APPLICATION_CREDENTIALS`, else gcloud's own
    /// (`$CLOUDSDK_CONFIG` or `~/.config/gcloud`).
    Discover,
    File(PathBuf),
    Off,
}

pub struct Gcp {
    saved: Mutex<Option<Saved>>,
    file: Option<PathBuf>,
    adc: AdcPath,
    /// Where every call goes in place of `https://SERVICE.googleapis.com`
    /// (`{endpoint}/SERVICE`), and the metadata server's (`{endpoint}/metadata`):
    /// tests.
    endpoint: Option<String>,
    /// Whether the metadata server is asked at all (off in tests that give
    /// no endpoint, so a test run on a VM does not use the VM's account).
    metadata_on: bool,
    http: reqwest::Client,
    /// The access token and when it expires (ms).
    token: tokio::sync::Mutex<Option<(String, u64)>>,
    /// The metadata server's answer and when it was asked (`None`: it did
    /// not answer).
    meta: Mutex<Option<(u64, Option<Metadata>)>>,
    catalogs: Mutex<HashMap<String, Arc<Catalog>>>,
    prices: tokio::sync::Mutex<Option<Arc<catalog::Prices>>>,
    /// Calls made, `GET compute/...` each (tests).
    calls: Mutex<Vec<String>>,
    /// The StrataCluster this server coordinates, as namespace and name.
    kube_cluster: Option<(String, String)>,
}

/// The workload identity pool every ChloeDB cluster outside Google Cloud
/// is a provider of.
pub const FEDERATION_POOL: &str = "chloedb";

/// Where the operator's `gcpFederation` mounts the coordinators' token.
pub const FEDERATION_TOKEN: &str = "/var/run/secrets/gcp/token";

/// The pool's provider of the Kubernetes cluster `name`: 4 to 32
/// lower-case letters, digits and hyphens.
pub fn provider_id(name: &str) -> String {
    let mut id: String = format!("k8s-{name}").chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    id.truncate(32);
    id.trim_end_matches('-').to_string()
}

/// What sets up workload identity federation for the StrataCluster
/// `name` in `namespace`, so that its coordinators act as `deployer` in
/// `project` with no key: shell commands for someone who may change the
/// project's IAM, with kubectl pointed at that cluster. ChloeDB runs none
/// of them. The cluster's signing keys are given to the provider
/// (`--jwk-json-path`), so its issuer need not be reachable from Google;
/// should the cluster change them, the provider's are updated the same way.
pub fn federation_commands(project: &str, namespace: &str, name: &str, deployer: &str) -> Vec<String> {
    let provider = provider_id(name);
    let path = format!("projects/$NUMBER/locations/global/workloadIdentityPools/{FEDERATION_POOL}/providers/{provider}");
    let audience = format!("https://iam.googleapis.com/{path}");
    let patch = json!({"spec": {"gcpFederation": {"configMap": format!("{name}-gcp"), "audience": audience}}}).to_string().replace('"', "\\\"");
    vec![
        format!("PROJECT={project}"),
        format!("DEPLOYER={deployer}"),
        "NUMBER=$(gcloud projects describe $PROJECT --format='value(projectNumber)')".into(),
        "gcloud services enable sts.googleapis.com iamcredentials.googleapis.com --project=$PROJECT".into(),
        format!("gcloud iam workload-identity-pools describe {FEDERATION_POOL} --project=$PROJECT --location=global >/dev/null 2>&1 || gcloud iam workload-identity-pools create {FEDERATION_POOL} --project=$PROJECT --location=global --display-name='ChloeDB clusters'"),
        "kubectl get --raw /openid/v1/jwks > cluster-jwks.json".into(),
        "ISSUER=$(kubectl get --raw /.well-known/openid-configuration | jq -r .issuer)".into(),
        format!("gcloud iam workload-identity-pools providers create-oidc {provider} --project=$PROJECT --location=global --workload-identity-pool={FEDERATION_POOL} --issuer-uri=\"$ISSUER\" --jwk-json-path=cluster-jwks.json --allowed-audiences=\"{audience}\" --attribute-mapping=google.subject=assertion.sub"),
        format!("gcloud iam service-accounts add-iam-policy-binding $DEPLOYER --project=$PROJECT --role=roles/iam.workloadIdentityUser --member=\"principal://iam.googleapis.com/projects/$NUMBER/locations/global/workloadIdentityPools/{FEDERATION_POOL}/subject/system:serviceaccount:{namespace}:{name}-coordinator\""),
        format!("gcloud iam workload-identity-pools create-cred-config {path} --service-account=$DEPLOYER --credential-source-file={FEDERATION_TOKEN} --credential-source-type=text --output-file=credentials.json"),
        format!("kubectl -n {namespace} create configmap {name}-gcp --from-file=credentials.json"),
        format!("kubectl -n {namespace} patch stratacluster {name} --type=merge -p \"{patch}\""),
    ]
}

impl Default for Gcp {
    fn default() -> Gcp {
        Gcp::memory()
    }
}

/// How long a region's catalogue is kept before it is fetched again.
const CATALOG_TTL_MS: u64 = 6 * 3600 * 1000;

/// The message of a Google API's error answer
/// (`{"error": {"code", "message", "status"}}`), else its start.
pub fn error_text(body: &str) -> String {
    if let Ok(j) = serde_json::from_str::<Json>(body) {
        let e = &j["error"];
        if let Some(m) = e["message"].as_str() {
            return match e["status"].as_str() {
                Some(s) => format!("{s}: {m}"),
                None => m.to_string(),
            };
        }
        // OAuth's token endpoint: {"error": "invalid_grant", "error_description"}.
        if let Some(code) = e.as_str() {
            return match j["error_description"].as_str() {
                Some(d) => format!("{code}: {d}"),
                None => code.to_string(),
            };
        }
    }
    body.chars().take(300).collect()
}

/// `s` percent-encoded for a form body.
fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

impl Gcp {
    /// The project kept in memory only.
    pub fn memory() -> Gcp {
        Gcp {
            saved: Mutex::new(None),
            file: None,
            adc: AdcPath::Discover,
            endpoint: None,
            metadata_on: true,
            http: crate::http_client().timeout(Duration::from_secs(60)).build().unwrap_or_default(),
            token: tokio::sync::Mutex::new(None),
            meta: Mutex::new(None),
            catalogs: Mutex::new(HashMap::new()),
            prices: tokio::sync::Mutex::new(None),
            calls: Mutex::new(Vec::new()),
            kube_cluster: None,
        }
    }

    /// The StrataCluster (`namespace/name`) this server coordinates.
    pub fn with_kube_cluster(mut self, c: &str) -> Gcp {
        self.kube_cluster = c.split_once('/').filter(|(n, c)| !n.is_empty() && !c.is_empty()).map(|(n, c)| (n.to_string(), c.to_string()));
        self
    }

    /// The settings file of a database: `<db>.gcp.json`.
    pub fn file_of(db: &Path) -> PathBuf {
        let mut p = db.as_os_str().to_owned();
        p.push(".gcp.json");
        p.into()
    }

    /// The project kept in `file` (read now if it exists).
    pub fn open(file: PathBuf) -> Result<Gcp, String> {
        let saved = match std::fs::read(&file) {
            Ok(b) => Some(serde_json::from_slice::<Saved>(&b).map_err(|e| format!("{}: {e}", file.display()))?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("{}: {e}", file.display())),
        };
        Ok(Gcp { saved: Mutex::new(saved), file: Some(file), ..Gcp::memory() })
    }

    /// Every call to `url` (`http://127.0.0.1:PORT`) in place of Google's
    /// APIs and the metadata server: tests.
    pub fn with_endpoint(mut self, url: &str) -> Gcp {
        self.endpoint = Some(url.trim_end_matches('/').to_string());
        self
    }

    /// The ADC file read from `path` (`None`: none at all), in place of
    /// the one gcloud writes: tests.
    pub fn with_adc(mut self, path: Option<PathBuf>) -> Gcp {
        self.adc = path.map_or(AdcPath::Off, AdcPath::File);
        self
    }

    /// Whether the metadata server is asked.
    pub fn with_metadata(mut self, on: bool) -> Gcp {
        self.metadata_on = on;
        self
    }

    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Saved>> {
        self.saved.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn saved(&self) -> Option<Saved> {
        self.lock().clone()
    }

    /// `https://SERVICE.googleapis.com` (or the tests' stand-in).
    fn base(&self, service: &str) -> String {
        match &self.endpoint {
            Some(e) => format!("{e}/{service}"),
            None => format!("https://{service}.googleapis.com"),
        }
    }

    /// A URL the credentials file names, sent to the stand-in in tests.
    fn rewrite(&self, url: &str) -> String {
        let Some(e) = &self.endpoint else { return url.to_string() };
        let Some(rest) = url.strip_prefix("https://") else { return url.to_string() };
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        match host.strip_suffix(".googleapis.com") {
            Some(service) => format!("{e}/{service}/{path}"),
            None => url.to_string(),
        }
    }

    fn metadata_base(&self) -> String {
        if let Ok(h) = std::env::var("GCE_METADATA_HOST") {
            if !h.is_empty() {
                return format!("http://{h}");
            }
        }
        match &self.endpoint {
            Some(e) => format!("{e}/metadata"),
            None => METADATA.into(),
        }
    }

    /// The ADC file, if there is one to read.
    pub fn adc_file(&self) -> Option<PathBuf> {
        match &self.adc {
            AdcPath::Off => None,
            AdcPath::File(p) => Some(p.clone()),
            AdcPath::Discover => {
                if let Some(p) = std::env::var_os("GOOGLE_APPLICATION_CREDENTIALS").filter(|v| !v.is_empty()) {
                    return Some(PathBuf::from(p));
                }
                let dir = std::env::var_os("CLOUDSDK_CONFIG").filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/gcloud")))?;
                let p = dir.join("application_default_credentials.json");
                p.exists().then_some(p)
            }
        }
    }

    /// The ADC file's credentials: `None` without a file.
    fn read_adc(&self) -> Option<(PathBuf, Result<Adc, String>)> {
        let path = self.adc_file()?;
        let r = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display())).and_then(|b| serde_json::from_slice::<Json>(&b).map_err(|e| format!("{}: {e}", path.display()))).and_then(|j| parse_adc(&j));
        Some((path, r))
    }

    /// The metadata server's answer (asked once; again after ten minutes
    /// when it did not answer).
    pub async fn metadata(&self) -> Option<Metadata> {
        if !self.metadata_on {
            return None;
        }
        if let Some((at, m)) = self.meta.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            if m.is_some() || now_ms().saturating_sub(at) < 10 * 60 * 1000 {
                return m;
            }
        }
        let base = self.metadata_base();
        let get = |path: &'static str| {
            let url = format!("{base}/computeMetadata/v1/{path}");
            let http = self.http.clone();
            async move {
                let r = http.get(url).header("Metadata-Flavor", "Google").timeout(Duration::from_secs(2)).send().await.ok()?;
                if !r.status().is_success() || r.headers().get("metadata-flavor").is_none_or(|v| v != "Google") {
                    return None;
                }
                r.text().await.ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
            }
        };
        let m = match get("instance/service-accounts/default/email").await {
            Some(email) => {
                let (project, cluster, location) = tokio::join!(get("project/project-id"), get("instance/attributes/cluster-name"), get("instance/attributes/cluster-location"));
                Some(Metadata { email, project, cluster, cluster_location: location })
            }
            None => None,
        };
        *self.meta.lock().unwrap_or_else(|e| e.into_inner()) = Some((now_ms(), m.clone()));
        m
    }

    /// The project calls are made in, and where it came from: the one kept
    /// here, the environment's, the impersonated service account's, or the
    /// metadata server's.
    pub async fn project(&self) -> Option<(String, &'static str)> {
        if let Some(s) = self.lock().as_ref() {
            return Some((s.project.clone(), "stored"));
        }
        for k in ["GOOGLE_CLOUD_PROJECT", "CLOUDSDK_CORE_PROJECT", "GCLOUD_PROJECT"] {
            if let Ok(v) = std::env::var(k) {
                if !v.is_empty() {
                    return Some((v, "environment"));
                }
            }
        }
        if let Some(email) = self.read_adc().and_then(|(_, r)| r.ok()).and_then(|a| a.service_account()) {
            if let Some(p) = project_of(&email) {
                return Some((p, "the impersonated service account"));
            }
        }
        if self.read_adc().is_none() {
            if let Some(p) = self.metadata().await.and_then(|m| m.project) {
                return Some((p, "the metadata server"));
            }
        }
        None
    }

    /// Who the calls are made as, without asking Google: the impersonated
    /// service account, the person whose sign-in it is, or the metadata
    /// server's service account.
    pub async fn account(&self) -> Option<String> {
        match self.read_adc() {
            Some((_, Ok(Adc::Impersonated { email, .. }))) => Some(email),
            Some((_, Ok(Adc::User { .. }))) => Some("the person signed in with gcloud auth application-default login".into()),
            Some((_, Ok(Adc::External(x)))) => Some(match (&x.impersonate, pool_of(&x.audience)) {
                (Some((_, email, _)), _) => email.clone(),
                (None, Some((pool, _))) => format!("the principal of the workload identity pool {pool}"),
                (None, None) => "a workload identity pool's principal".into(),
            }),
            Some((_, Err(_))) => None,
            None => self.metadata().await.map(|m| m.email),
        }
    }

    /// The service account a GKE cluster's own command center may act as
    /// through Workload Identity: the one the calls are made as, if it is
    /// a service account.
    pub async fn deployer(&self) -> Option<String> {
        match self.read_adc() {
            Some((_, Ok(a))) => a.service_account(),
            Some(_) => None,
            None => self.metadata().await.map(|m| m.email).filter(|e| e.contains('@')),
        }
    }

    /// What the view shows: where the credentials are, who they are, the
    /// project; never a token or a refresh token.
    pub async fn status(&self) -> Json {
        let file = self.file.as_ref().map(|p| p.display().to_string());
        let saved = self.saved();
        let credentials = match self.read_adc() {
            Some((path, Ok(Adc::Impersonated { email, delegates, .. }))) => json!({
                "source": "adc", "kind": "impersonated_service_account", "file": path.display().to_string(),
                "service_account": email, "delegates": delegates,
                "what": format!("your gcloud sign-in, impersonating {email}"),
            }),
            Some((path, Ok(Adc::User { .. }))) => json!({
                "source": "adc", "kind": "authorized_user", "file": path.display().to_string(), "service_account": null,
                "what": "your gcloud sign-in, as yourself (no service account impersonated)",
            }),
            Some((path, Ok(Adc::External(x)))) => {
                let (pool, provider) = pool_of(&x.audience).unwrap_or_default();
                let email = x.impersonate.as_ref().map(|(_, e, _)| e.clone());
                let token_file = match &x.subject {
                    Subject::File { path, .. } => Some(json!({"path": path.display().to_string(), "present": path.exists()})),
                    Subject::Url { .. } => None,
                };
                json!({
                    "source": "adc", "kind": "external_account", "file": path.display().to_string(),
                    "service_account": email, "pool": pool, "provider": provider, "subject": x.subject.describe(), "token_file": token_file,
                    "what": match &email {
                        Some(e) => format!("workload identity federation (pool {pool}, provider {provider}), impersonating {e}"),
                        None => format!("workload identity federation (pool {pool}, provider {provider}), as the pool's principal"),
                    },
                })
            }
            Some((path, Err(e))) => json!({"source": "none", "file": path.display().to_string(), "refused": e}),
            None => match self.metadata().await {
                Some(m) => json!({
                    "source": "metadata", "kind": "metadata", "service_account": m.email,
                    "what": if m.cluster.is_some() { format!("GKE's Workload Identity: {}", m.email) } else { format!("this VM's service account, {}", m.email) },
                    "cluster": m.cluster, "cluster_location": m.cluster_location,
                }),
                None => json!({"source": "none"}),
            },
        };
        let project = self.project().await.map(|(p, from)| json!({"id": p, "from": from}));
        // On Kubernetes but not GKE: how the coordinators' own account
        // reaches Google Cloud through workload identity federation.
        let federation = match (&self.kube_cluster, credentials["source"].as_str(), credentials["kind"].as_str()) {
            (Some((ns, name)), Some("none"), _) | (Some((ns, name)), _, Some("external_account")) => {
                let p = project.as_ref().and_then(|p| p["id"].as_str()).unwrap_or("PROJECT").to_string();
                let deployer = credentials["service_account"].as_str().map(String::from).unwrap_or_else(|| format!("NAME@{p}.iam.gserviceaccount.com"));
                json!({
                    "namespace": ns, "cluster": name, "account": format!("{name}-coordinator"),
                    "pool": FEDERATION_POOL, "provider": provider_id(name), "token_file": FEDERATION_TOKEN,
                    "configured": credentials["kind"] == "external_account",
                    "commands": federation_commands(&p, ns, name, &deployer),
                })
            }
            _ => Json::Null,
        };
        json!({"credentials": credentials, "project": project, "saved": saved, "file": file, "federation": federation})
    }

    /// An access token, from the cache while it lasts.
    pub async fn access_token(&self) -> Result<String, String> {
        let mut cached = self.token.lock().await;
        if let Some((t, exp)) = cached.as_ref() {
            if now_ms() + TOKEN_MARGIN_MS < *exp {
                return Ok(t.clone());
            }
        }
        let (t, exp) = match self.read_adc() {
            Some((_, Ok(adc))) => self.mint(&adc).await?,
            Some((_, Err(e))) => return Err(e),
            None => {
                if self.metadata().await.is_none() {
                    return Err("no Google Cloud credentials: run gcloud auth application-default login --impersonate-service-account NAME@PROJECT.iam.gserviceaccount.com where this server runs, or run it on a Compute Engine VM or GKE pod with a service account".into());
                }
                self.metadata_token().await?
            }
        };
        *cached = Some((t.clone(), exp));
        Ok(t)
    }

    /// Whether there are credentials to make calls with (no call made).
    pub async fn has_credentials(&self) -> bool {
        match self.read_adc() {
            Some((_, r)) => r.is_ok(),
            None => self.metadata().await.is_some(),
        }
    }

    /// A token from `adc`, and when it expires.
    fn mint<'a>(&'a self, adc: &'a Adc) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(String, u64), String>> + Send + 'a>> {
        Box::pin(async move {
            match adc {
                Adc::User { client_id, client_secret, refresh_token, token_uri } => {
                    let body = format!("grant_type=refresh_token&client_id={}&client_secret={}&refresh_token={}", enc(client_id), enc(client_secret), enc(refresh_token));
                    let url = self.rewrite(token_uri);
                    self.note("POST", &url);
                    let r = self.http.post(&url).header("content-type", "application/x-www-form-urlencoded").body(body).send().await.map_err(|e| format!("Google's token endpoint: {e}"))?;
                    let status = r.status();
                    let text = r.text().await.map_err(|e| format!("Google's token endpoint: {e}"))?;
                    if !status.is_success() {
                        return Err(format!("Google refused the gcloud sign-in ({status}: {}): sign in again with gcloud auth application-default login", error_text(&text)));
                    }
                    let j: Json = serde_json::from_str(&text).map_err(|e| format!("Google's token endpoint: {e}"))?;
                    let t = j["access_token"].as_str().filter(|t| !t.is_empty()).ok_or("Google's token endpoint gave no access token")?;
                    Ok((t.to_string(), now_ms() + j["expires_in"].as_u64().unwrap_or(3600) * 1000))
                }
                Adc::Impersonated { source, url, email, delegates } => {
                    let (src, _) = self.mint(source).await?;
                    let hint = format!("your account needs Service Account Token Creator (roles/iam.serviceAccountTokenCreator) on {email}, and its project the IAM Service Account Credentials API (iamcredentials.googleapis.com)");
                    self.impersonate(&src, url, email, delegates, 3600, &hint).await
                }
                Adc::External(x) => {
                    let subject = self.subject_token(&x.subject).await?;
                    let federated = self.exchange(x, &subject).await?;
                    match &x.impersonate {
                        None => Ok(federated),
                        Some((url, email, lifetime)) => {
                            let hint = match pool_of(&x.audience) {
                                Some((pool, _)) => format!("the pool {pool}'s principal needs Workload Identity User (roles/iam.workloadIdentityUser) on {email}, and the project the IAM Service Account Credentials API (iamcredentials.googleapis.com)"),
                                None => format!("the federation's principal needs Workload Identity User (roles/iam.workloadIdentityUser) on {email}"),
                            };
                            self.impersonate(&federated.0, url, email, &[], *lifetime, &hint).await
                        }
                    }
                }
            }
        })
    }

    /// A token of the service account `email`, made with the token `src`
    /// through IAM Credentials' `generateAccessToken` at `url`.
    async fn impersonate(&self, src: &str, url: &str, email: &str, delegates: &[String], lifetime: u64, hint: &str) -> Result<(String, u64), String> {
        let url = self.rewrite(url);
        self.note("POST", &url);
        let body = json!({"scope": [SCOPE], "lifetime": format!("{lifetime}s"), "delegates": delegates});
        let r = self.http.post(&url).bearer_auth(src).header("content-type", "application/json").body(body.to_string()).send().await.map_err(|e| format!("IAM Credentials: {e}"))?;
        let status = r.status();
        let text = r.text().await.map_err(|e| format!("IAM Credentials: {e}"))?;
        if !status.is_success() {
            let why = error_text(&text);
            let hint = if status.as_u16() == 403 { format!(": {hint}") } else { String::new() };
            return Err(format!("could not impersonate {email} ({status}: {why}){hint}"));
        }
        let j: Json = serde_json::from_str(&text).map_err(|e| format!("IAM Credentials: {e}"))?;
        let t = j["accessToken"].as_str().filter(|t| !t.is_empty()).ok_or("IAM Credentials gave no access token")?;
        Ok((t.to_string(), now_ms() + lifetime * 1000))
    }

    /// A federation's subject token, read now (a projected Kubernetes
    /// token is renewed in its file as it nears its end).
    async fn subject_token(&self, subject: &Subject) -> Result<String, String> {
        let (text, field) = match subject {
            Subject::File { path, field } => {
                let t = tokio::fs::read_to_string(path).await.map_err(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        format!("the federation's token file {} is not there: on Kubernetes, the operator's gcpFederation mounts the pod's token there", path.display())
                    } else {
                        format!("the federation's token file {}: {e}", path.display())
                    }
                })?;
                (t, field)
            }
            Subject::Url { url, headers, field } => {
                let mut r = self.http.get(url).timeout(Duration::from_secs(10));
                for (k, v) in headers {
                    r = r.header(k, v);
                }
                let res = r.send().await.map_err(|e| format!("the federation's token URL {url}: {e}"))?;
                let status = res.status();
                let t = res.text().await.map_err(|e| format!("the federation's token URL {url}: {e}"))?;
                if !status.is_success() {
                    return Err(format!("the federation's token URL {url} answered {status}"));
                }
                (t, field)
            }
        };
        let token = match field {
            None => text.trim().to_string(),
            Some(f) => {
                let j: Json = serde_json::from_str(&text).map_err(|e| format!("the federation's token, read as JSON: {e}"))?;
                j[f.as_str()].as_str().unwrap_or_default().trim().to_string()
            }
        };
        if token.is_empty() {
            return Err(format!("the federation's token from {} is empty", subject.describe()));
        }
        Ok(token)
    }

    /// The subject token exchanged at Google's Security Token Service for
    /// a token of the pool's principal.
    async fn exchange(&self, x: &External, subject: &str) -> Result<(String, u64), String> {
        let mut body = format!(
            "grant_type={}&audience={}&scope={}&requested_token_type={}&subject_token={}&subject_token_type={}",
            enc("urn:ietf:params:oauth:grant-type:token-exchange"),
            enc(&x.audience),
            enc(SCOPE),
            enc("urn:ietf:params:oauth:token-type:access_token"),
            enc(subject),
            enc(&x.subject_token_type)
        );
        if let (Some(p), None) = (&x.user_project, &x.impersonate) {
            body.push_str(&format!("&options={}", enc(&json!({"userProject": p}).to_string())));
        }
        let url = self.rewrite(&x.token_url);
        self.note("POST", &url);
        let r = self.http.post(&url).header("content-type", "application/x-www-form-urlencoded").body(body).send().await.map_err(|e| format!("Google's Security Token Service: {e}"))?;
        let status = r.status();
        let text = r.text().await.map_err(|e| format!("Google's Security Token Service: {e}"))?;
        if !status.is_success() {
            let provider = pool_of(&x.audience).map(|(p, v)| format!("the pool {p}'s provider {v}")).unwrap_or_else(|| "the pool's provider".into());
            return Err(format!(
                "Google's Security Token Service refused the federation's token ({status}: {}): {provider} must trust the token's issuer and its audience, and the project the Security Token Service API (sts.googleapis.com)",
                error_text(&text)
            ));
        }
        let j: Json = serde_json::from_str(&text).map_err(|e| format!("Google's Security Token Service: {e}"))?;
        let t = j["access_token"].as_str().filter(|t| !t.is_empty()).ok_or("Google's Security Token Service gave no access token")?;
        Ok((t.to_string(), now_ms() + j["expires_in"].as_u64().unwrap_or(3600) * 1000))
    }

    async fn metadata_token(&self) -> Result<(String, u64), String> {
        let url = format!("{}/computeMetadata/v1/instance/service-accounts/default/token", self.metadata_base());
        self.note("GET", &url);
        let r = self.http.get(&url).header("Metadata-Flavor", "Google").send().await.map_err(|e| format!("the metadata server: {e}"))?;
        let status = r.status();
        let text = r.text().await.map_err(|e| format!("the metadata server: {e}"))?;
        if !status.is_success() {
            return Err(format!("the metadata server gave no token ({status}: {})", text.chars().take(200).collect::<String>()));
        }
        let j: Json = serde_json::from_str(&text).map_err(|e| format!("the metadata server: {e}"))?;
        let t = j["access_token"].as_str().filter(|t| !t.is_empty()).ok_or("the metadata server gave no access token")?;
        Ok((t.to_string(), now_ms() + j["expires_in"].as_u64().unwrap_or(3600) * 1000))
    }

    fn note(&self, method: &str, url: &str) {
        let path = url.split('?').next().unwrap_or(url);
        let path = match &self.endpoint {
            Some(e) => path.strip_prefix(e.as_str()).unwrap_or(path).trim_start_matches('/').to_string(),
            None => path.trim_start_matches("https://").replacen(".googleapis.com", "", 1),
        };
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).push(format!("{method} {path}"));
    }

    /// One call to `service` (`compute`, `container`, ...); `None` for a
    /// 404, `Json::Null` for an empty answer.
    pub async fn send(&self, method: reqwest::Method, service: &str, path: &str, body: Option<&Json>) -> Result<Option<Json>, String> {
        let token = self.access_token().await?;
        let url = format!("{}{path}", self.base(service));
        self.note(method.as_str(), &url);
        let mut r = self.http.request(method.clone(), &url).bearer_auth(&token);
        if let Some(b) = body {
            r = r.header("content-type", "application/json").body(b.to_string());
        }
        let res = r.send().await.map_err(|e| format!("{method} {service}{path}: {e}"))?;
        let status = res.status();
        let text = res.text().await.map_err(|e| format!("{method} {service}{path}: {e}"))?;
        if status.as_u16() == 404 {
            return Ok(None);
        }
        if !status.is_success() {
            let short = path.split('?').next().unwrap_or(path);
            return Err(format!("{method} {service}{short}: {}: {}", status.as_u16(), error_text(&text)));
        }
        if text.trim().is_empty() {
            return Ok(Some(Json::Null));
        }
        serde_json::from_str(&text).map(Some).map_err(|e| format!("{method} {service}{path}: {e}"))
    }

    async fn get(&self, service: &str, path: &str) -> Result<Json, String> {
        self.send(reqwest::Method::GET, service, path, None).await?.ok_or_else(|| format!("GET {service}{}: 404 Not Found", path.split('?').next().unwrap_or(path)))
    }

    fn need_project<'a>(&self, p: &'a Option<(String, &'static str)>) -> Result<&'a str, String> {
        p.as_ref().map(|(p, _)| p.as_str()).ok_or_else(|| "no Google Cloud project: save one in the Cloud view (PUT /v1/admin/gcp/project)".into())
    }

    /// The project kept: checked first (its Compute Engine API answers
    /// with these credentials), then saved, in the file (mode 0600) when
    /// there is one.
    pub async fn set(&self, project: &str, region: &str, by: &str) -> Result<Saved, String> {
        let project = project.trim();
        check_project(project)?;
        let region = if region.trim().is_empty() { "us-central1" } else { region.trim() };
        check_location(region)?;
        let j = self.get("compute", &format!("/compute/v1/projects/{project}")).await.map_err(|e| format!("Google Cloud did not let these credentials read the project {project}: {e}"))?;
        // Compute's `id` is its own number for the project, not the
        // project's: that one is in its default service account's address.
        let number = j["defaultServiceAccount"].as_str().and_then(|a| a.strip_suffix("-compute@developer.gserviceaccount.com")).filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).unwrap_or_default().to_string();
        let account = self.account().await.unwrap_or_default();
        let s = Saved { project: project.into(), region: region.into(), account, number, saved_ms: now_ms(), by: by.into() };
        if let Some(path) = &self.file {
            write_private(path, &serde_json::to_vec_pretty(&s).map_err(|e| e.to_string())?).map_err(|e| format!("write {}: {e}", path.display()))?;
        }
        *self.lock() = Some(s.clone());
        self.catalogs.lock().unwrap_or_else(|e| e.into_inner()).clear();
        Ok(s)
    }

    /// Forgets the project kept (and deletes its file). Nothing in Google
    /// Cloud changes.
    pub fn forget(&self) -> Result<Option<String>, String> {
        let was = self.lock().take().map(|s| s.project);
        if let Some(path) = &self.file {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("remove {}: {e}", path.display())),
            }
        }
        self.catalogs.lock().unwrap_or_else(|e| e.into_inner()).clear();
        Ok(was)
    }

    /// The project's regions, each with its zones that are up.
    pub async fn regions(&self) -> Result<Vec<catalog::Region>, String> {
        let p = self.project().await;
        let p = self.need_project(&p)?;
        Ok(catalog::regions(&self.get("compute", &format!("/compute/v1/projects/{p}/regions")).await?))
    }

    /// Compute Engine's prices, every region's: the Cloud Billing
    /// Catalog's, through its pages, kept six hours.
    async fn prices(&self) -> Result<Arc<catalog::Prices>, String> {
        let mut cached = self.prices.lock().await;
        if let Some(p) = cached.as_ref().filter(|p| now_ms().saturating_sub(p.at_ms) < CATALOG_TTL_MS) {
            return Ok(p.clone());
        }
        let mut skus = Vec::new();
        let mut page = String::new();
        for _ in 0..40 {
            let q = if page.is_empty() { String::new() } else { format!("&pageToken={}", enc(&page)) };
            let j = self.get("cloudbilling", &format!("/v1/services/{}/skus?currencyCode=USD&pageSize=5000{q}", catalog::COMPUTE_SERVICE)).await?;
            // Only what a plan prices: Compute Engine has tens of thousands.
            skus.extend(j["skus"].as_array().into_iter().flatten().filter(|s| catalog::wanted(s)).cloned());
            page = j["nextPageToken"].as_str().unwrap_or_default().to_string();
            if page.is_empty() {
                break;
            }
        }
        let p = Arc::new(catalog::Prices::from_skus(&skus));
        *cached = Some(p.clone());
        Ok(p)
    }

    /// A region's machine types, priced, GKE's versions, and the project's
    /// quotas: from the cache when fetched in the last six hours (`fresh`:
    /// fetched again).
    pub async fn catalog(&self, region: &str, fresh: bool) -> Result<Arc<Catalog>, String> {
        check_location(region)?;
        if !fresh {
            if let Some(c) = self.catalogs.lock().unwrap_or_else(|e| e.into_inner()).get(region).filter(|c| now_ms().saturating_sub(c.at_ms) < CATALOG_TTL_MS) {
                return Ok(c.clone());
            }
        }
        let p = self.project().await;
        let project = self.need_project(&p)?.to_string();
        let reg = match self.send(reqwest::Method::GET, "compute", &format!("/compute/v1/projects/{project}/regions/{region}"), None).await? {
            Some(r) => r,
            None => return Err(format!("{region:?} is not a region of Compute Engine (like us-central1)")),
        };
        let zones = catalog::zones_of(&reg);
        if zones.is_empty() {
            return Err(format!("{region} has no zone that is up"));
        }
        // The machine types of the zones a cluster would use (a regional one
        // uses three).
        let use_zones: Vec<String> = zones.iter().take(3).cloned().collect();
        let type_paths: Vec<String> = use_zones.iter().map(|z| format!("/compute/v1/projects/{project}/zones/{z}/machineTypes?maxResults=500")).collect();
        let types = futures::future::join_all(type_paths.iter().map(|p| self.get("compute", p))).await;
        let (proj_path, server_path) = (format!("/compute/v1/projects/{project}"), format!("/v1/projects/{project}/locations/{region}/serverConfig"));
        let (net_path, sub_path) = (format!("/compute/v1/projects/{project}/global/networks"), format!("/compute/v1/projects/{project}/regions/{region}/subnetworks"));
        let (proj, server, prices, deployer, nets, subs) = tokio::join!(
            self.get("compute", &proj_path),
            self.get("container", &server_path),
            self.prices(),
            self.deployer(),
            self.get("compute", &net_path),
            self.get("compute", &sub_path),
        );
        let mut missing = Vec::new();
        let mut by_zone = Vec::new();
        for (z, t) in use_zones.iter().zip(types) {
            match t {
                Ok(t) => by_zone.push((z.clone(), t)),
                Err(e) => missing.push(format!("the machine types of {z}: {e}")),
            }
        }
        if by_zone.is_empty() {
            return Err(missing.join("; "));
        }
        let server = server.map_err(|e| format!("GKE's versions in {region}: {e}"))?;
        let prices = match prices {
            Ok(p) => Some(p),
            Err(e) => {
                missing.push(format!("the prices (the Cloud Billing API's catalogue, cloudbilling.googleapis.com): {e}"));
                None
            }
        };
        let proj = proj.map_err(|e| missing.push(format!("the project's quotas: {e}"))).ok();
        let networks = nets.map_err(|e| missing.push(format!("the project's networks: {e}"))).ok();
        let mut cat = catalog::build(&project, region, &reg, proj.as_ref(), &by_zone, &server, prices.as_deref(), deployer, missing);
        cat.networks = networks.as_ref().map(catalog::networks);
        cat.subnetworks = subs.as_ref().map(catalog::subnetworks).unwrap_or_default();
        let cat = Arc::new(cat);
        self.catalogs.lock().unwrap_or_else(|e| e.into_inner()).insert(region.into(), cat.clone());
        Ok(cat)
    }

    /// Asks the project which of `permissions` these credentials hold
    /// (Resource Manager's `testIamPermissions`, which changes nothing).
    pub async fn test_permissions(&self, project: &str, permissions: &[&str]) -> Result<Vec<String>, String> {
        let j = self.send(reqwest::Method::POST, "cloudresourcemanager", &format!("/v1/projects/{project}:testIamPermissions"), Some(&json!({"permissions": permissions}))).await?.unwrap_or_default();
        Ok(j["permissions"].as_array().map(|a| a.iter().filter_map(|p| p.as_str().map(String::from)).collect()).unwrap_or_default())
    }

    /// Makes the GKE cluster `request` (the body a plan gives) in
    /// `location` (a zone, or a region for a regional cluster). This bills
    /// the project. Answers the cluster's name
    /// (`projects/P/locations/L/clusters/N`).
    pub async fn create_cluster(&self, project: &str, location: &str, request: &Json) -> Result<String, String> {
        let name = request["cluster"]["name"].as_str().ok_or("the request names no cluster")?;
        let op = self.send(reqwest::Method::POST, "container", &format!("/v1/projects/{project}/locations/{location}/clusters"), Some(request)).await?.unwrap_or_default();
        if op["status"] == "DONE" && !op["error"].is_null() {
            return Err(format!("GKE could not make {name}: {}", op["error"]["message"].as_str().unwrap_or("no reason given")));
        }
        Ok(format!("projects/{project}/locations/{location}/clusters/{name}"))
    }

    /// The GKE cluster `name` (`projects/P/locations/L/clusters/N`) now,
    /// `None` once it is gone.
    pub async fn cluster(&self, name: &str) -> Result<Option<Json>, String> {
        check_resource(name)?;
        self.send(reqwest::Method::GET, "container", &format!("/v1/{name}"), None).await
    }

    /// The project's GKE clusters, every location's.
    pub async fn clusters(&self) -> Result<Vec<Json>, String> {
        let p = self.project().await;
        let p = self.need_project(&p)?;
        let j = self.get("container", &format!("/v1/projects/{p}/locations/-/clusters")).await?;
        Ok(j["clusters"].as_array().cloned().unwrap_or_default())
    }

    /// The project's GKE cluster called `name` in `location`, if there is
    /// one: its resource name.
    pub async fn cluster_named(&self, project: &str, location: &str, name: &str) -> Result<Option<String>, String> {
        let n = format!("projects/{project}/locations/{location}/clusters/{name}");
        Ok(self.cluster(&n).await?.map(|_| n))
    }

    /// The API of the GKE cluster `name`: its endpoint, CA (PEM) and a
    /// bearer token (these credentials' own access token, which GKE takes).
    pub async fn credentials(&self, name: &str) -> Result<(String, Vec<u8>, String), String> {
        use base64::Engine;
        let c = self.cluster(name).await?.ok_or_else(|| format!("GKE has no cluster {name}"))?;
        let endpoint = c["endpoint"].as_str().filter(|s| !s.is_empty()).ok_or("the cluster has no endpoint yet")?;
        let endpoint = if endpoint.starts_with("http://") || endpoint.starts_with("https://") { endpoint.to_string() } else { format!("https://{endpoint}") };
        let ca = base64::engine::general_purpose::STANDARD.decode(c["masterAuth"]["clusterCaCertificate"].as_str().unwrap_or_default().trim()).map_err(|e| format!("the cluster's CA: {e}"))?;
        Ok((endpoint, ca, self.access_token().await?))
    }

    /// Deletes the GKE cluster `name` with its node pools (their nodes and
    /// boot disks). The volumes are the PD driver's: deleted with their
    /// claims first ([`crate::cloud::install::teardown`]).
    pub async fn delete_cluster(&self, name: &str) -> Result<(), String> {
        check_resource(name)?;
        self.send(reqwest::Method::DELETE, "container", &format!("/v1/{name}"), None).await.map(|_| ())
    }

    /// Sets the node pool `pool` of the cluster `name` to `per_zone` nodes
    /// in each of its zones (GKE's `setSize`).
    pub async fn set_size(&self, name: &str, pool: &str, per_zone: u32) -> Result<(), String> {
        check_resource(name)?;
        self.send(reqwest::Method::POST, "container", &format!("/v1/{name}/nodePools/{pool}:setSize"), Some(&json!({"nodeCount": per_zone}))).await.map(|_| ())
    }

    /// Sets the node pool's autoscaling: `bounds` the fewest and most nodes
    /// in all, or `None` to turn it off.
    pub async fn set_autoscaling(&self, name: &str, pool: &str, bounds: Option<(u32, u32)>) -> Result<(), String> {
        check_resource(name)?;
        let a = match bounds {
            Some((min, max)) => json!({"enabled": true, "totalMinNodeCount": min, "totalMaxNodeCount": max, "locationPolicy": "BALANCED"}),
            None => json!({"enabled": false}),
        };
        self.send(reqwest::Method::POST, "container", &format!("/v1/{name}/nodePools/{pool}:setAutoscaling"), Some(&json!({"autoscaling": a}))).await.map(|_| ())
    }

    /// The calls made so far (tests).
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// A cluster's resource name, `projects/P/locations/L/clusters/N`, and
/// nothing a path could be bent with.
fn check_resource(name: &str) -> Result<(), String> {
    let parts: Vec<&str> = name.split('/').collect();
    let ok = parts.len() == 6 && parts[0] == "projects" && parts[2] == "locations" && parts[4] == "clusters" && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    if ok {
        Ok(())
    } else {
        Err(format!("{name:?} is not a GKE cluster's name (projects/P/locations/L/clusters/N)"))
    }
}

/// The location (`us-central1-a`) and name of a cluster's resource name.
pub fn location_of(name: &str) -> (&str, &str) {
    let parts: Vec<&str> = name.split('/').collect();
    match parts.as_slice() {
        ["projects", _, "locations", l, "clusters", n] => (l, n),
        _ => ("", name),
    }
}

/// The node count of a GKE cluster's answer: its current one.
pub fn node_count(c: &Json) -> Option<u32> {
    c["currentNodeCount"].as_u64().map(|n| n as u32)
}

/// What the Cloud view shows of a GKE cluster's node pools: each one's
/// machine type, zones, nodes (GKE says a pool's per zone) and autoscaling.
pub fn pools(name: &str, c: &Json) -> Json {
    let pools: Vec<Json> = c["nodePools"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| {
            let zones = p["locations"].as_array().cloned().unwrap_or_default();
            let per_zone = p["initialNodeCount"].as_u64();
            let a = &p["autoscaling"];
            json!({
                "name": p["name"], "machine": p["config"]["machineType"], "spot": p["config"]["spot"].as_bool().unwrap_or(false),
                "zones": zones, "status": p["status"], "version": p["version"],
                "nodes_made_with": per_zone.map(|n| n * zones.len().max(1) as u64),
                "autoscaling": if a["enabled"] == true { json!({"min": a["totalMinNodeCount"].as_u64().or(a["minNodeCount"].as_u64()), "max": a["totalMaxNodeCount"].as_u64().or(a["maxNodeCount"].as_u64())}) } else { Json::Null },
            })
        })
        .collect();
    let (location, cluster) = location_of(name);
    json!({"cluster": name, "name": cluster, "location": location, "status": c["status"], "nodes": node_count(c), "pools": pools})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_files_are_read_and_keys_refused() {
        let user = json!({"type": "authorized_user", "client_id": "id.apps.googleusercontent.com", "client_secret": "s", "refresh_token": "1//refresh"});
        assert!(matches!(parse_adc(&user), Ok(Adc::User { ref token_uri, .. }) if token_uri == TOKEN_URI));
        let imp = json!({
            "type": "impersonated_service_account", "delegates": [],
            "service_account_impersonation_url": "https://iamcredentials.googleapis.com/v1/projects/-/serviceAccounts/chloedb-deployer@chloedb.iam.gserviceaccount.com:generateAccessToken",
            "source_credentials": user,
        });
        match parse_adc(&imp) {
            Ok(Adc::Impersonated { email, source, .. }) => {
                assert_eq!(email, "chloedb-deployer@chloedb.iam.gserviceaccount.com");
                assert!(matches!(*source, Adc::User { .. }));
            }
            _ => panic!("not read"),
        }
        assert_eq!(project_of("chloedb-deployer@chloedb.iam.gserviceaccount.com").as_deref(), Some("chloedb"));
        assert_eq!(project_of("123-compute@developer.gserviceaccount.com"), None);
        let key = json!({"type": "service_account", "client_email": "x@p.iam.gserviceaccount.com", "private_key": "-----BEGIN PRIVATE KEY-----"});
        let e = parse_adc(&key).err().unwrap();
        assert!(e.contains("takes no service account keys") && e.contains("--impersonate-service-account x@p.iam.gserviceaccount.com") && !e.contains("PRIVATE"), "{e}");
        assert!(parse_adc(&json!({"type": "external_account"})).is_err());
        assert!(parse_adc(&json!({"type": "authorized_user"})).is_err());
    }

    /// A federation's file, as gcloud iam workload-identity-pools
    /// create-cred-config writes it for a Kubernetes token in a file.
    fn federation(source: Json) -> Json {
        json!({
            "universe_domain": "googleapis.com", "type": "external_account",
            "audience": "//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/chloedb/providers/k8s-e2e",
            "subject_token_type": "urn:ietf:params:oauth:token-type:jwt", "token_url": "https://sts.googleapis.com/v1/token",
            "service_account_impersonation_url": "https://iamcredentials.googleapis.com/v1/projects/-/serviceAccounts/chloedb-deployer@chloedb.iam.gserviceaccount.com:generateAccessToken",
            "credential_source": source,
        })
    }

    #[test]
    fn a_federations_file_is_read_and_what_it_cannot_do_refused() {
        let file = federation(json!({"file": "/var/run/secrets/gcp/token", "format": {"type": "text"}}));
        let Ok(Adc::External(x)) = parse_adc(&file) else { panic!("not read") };
        assert!(matches!(&x.subject, Subject::File { path, field: None } if path == Path::new("/var/run/secrets/gcp/token")));
        assert_eq!(x.impersonate.as_ref().map(|i| (i.1.as_str(), i.2)), Some(("chloedb-deployer@chloedb.iam.gserviceaccount.com", 3600)));
        assert_eq!(pool_of(&x.audience), Some(("chloedb".to_string(), "k8s-e2e".to_string())));
        assert_eq!(Adc::External(x).service_account().as_deref(), Some("chloedb-deployer@chloedb.iam.gserviceaccount.com"));
        let json_field = federation(json!({"url": "http://169.254.169.254/token", "headers": {"Metadata": "True"}, "format": {"type": "json", "subject_token_field_name": "access_token"}}));
        let Ok(Adc::External(x)) = parse_adc(&json_field) else { panic!("not read") };
        assert!(matches!(&x.subject, Subject::Url { headers, field: Some(f), .. } if f == "access_token" && headers == &[("Metadata".to_string(), "True".to_string())]));
        let mut direct = federation(json!({"file": "/t"}));
        direct.as_object_mut().unwrap().remove("service_account_impersonation_url");
        let Ok(Adc::External(x)) = parse_adc(&direct) else { panic!("not read") };
        assert!(x.impersonate.is_none());
        let refused = |source: Json, change: &dyn Fn(&mut Json), says: &str| {
            let mut j = federation(source);
            change(&mut j);
            let e = parse_adc(&j).err().unwrap_or_else(|| panic!("taken: {j}"));
            assert!(e.contains(says), "{e}");
        };
        let same = |_: &mut Json| {};
        refused(json!({"executable": {"command": "/bin/sh -c id"}}), &same, "runs none");
        refused(json!({"environment_id": "aws1", "region_url": "http://169.254.169.254/latest/meta-data/placement/availability-zone"}), &same, "aws1");
        refused(json!({}), &same, "credential_source.file or url");
        refused(json!({"file": "/t", "format": {"type": "json"}}), &same, "subject_token_field_name");
        refused(json!({"file": "/t"}), &|j| j["token_url"] = json!("https://sts.example.com/v1/token"), "Security Token Service");
        refused(json!({"file": "/t"}), &|j| j["token_url"] = json!("https://sts.googleapis.com.example.com/v1/token"), "Security Token Service");
        refused(json!({"file": "/t"}), &|j| j["service_account_impersonation_url"] = json!("https://example.com/v1/projects/-/serviceAccounts/a@b.iam.gserviceaccount.com:generateAccessToken"), "IAM Credentials");
        refused(json!({"file": "/t"}), &|j| j["audience"] = json!("chloedb"), "pool's provider");
        refused(json!({"file": "/t"}), &|j| j["universe_domain"] = json!("example.com"), "universe");
        refused(json!({"file": "/t"}), &|j| j["service_account_impersonation"] = json!({"token_lifetime_seconds": 86400}), "43200");
        assert!(parse_adc(&json!({"type": "external_account_authorized_user"})).err().unwrap().contains("workforce"));
    }

    #[test]
    fn the_federation_is_set_up_by_commands_chloedb_only_prints() {
        assert_eq!(provider_id("e2e"), "k8s-e2e");
        assert_eq!(provider_id("Search_East"), "k8s-search-east");
        assert!(provider_id(&"a".repeat(40)).len() <= 32);
        let c = federation_commands("chloedb", "strata", "e2e", "chloedb-deployer@chloedb.iam.gserviceaccount.com");
        let all = c.join("\n");
        assert!(c[0] == "PROJECT=chloedb" && c[1] == "DEPLOYER=chloedb-deployer@chloedb.iam.gserviceaccount.com", "{all}");
        assert!(all.contains("--member=\"principal://iam.googleapis.com/projects/$NUMBER/locations/global/workloadIdentityPools/chloedb/subject/system:serviceaccount:strata:e2e-coordinator\""), "{all}");
        assert!(all.contains("--allowed-audiences=\"https://iam.googleapis.com/projects/$NUMBER/locations/global/workloadIdentityPools/chloedb/providers/k8s-e2e\""), "{all}");
        assert!(all.contains("--credential-source-file=/var/run/secrets/gcp/token --credential-source-type=text"), "{all}");
        assert!(all.contains("--jwk-json-path=cluster-jwks.json"), "{all}");
        // The patch is JSON once the shell has read it.
        let last = c.last().unwrap();
        let quoted = last.split_once("-p \"").unwrap().1.strip_suffix('"').unwrap().replace("\\\"", "\"");
        let patch: Json = serde_json::from_str(&quoted).unwrap();
        assert_eq!(patch["spec"]["gcpFederation"]["configMap"], "e2e-gcp");
        assert_eq!(patch["spec"]["gcpFederation"]["audience"], "https://iam.googleapis.com/projects/$NUMBER/locations/global/workloadIdentityPools/chloedb/providers/k8s-e2e");
        assert!(!all.contains("keys create"), "no key is made");
    }

    #[test]
    fn names_are_checked_before_they_reach_a_path() {
        assert!(check_project("chloedb").is_ok());
        for bad in ["", "abc", "Chloedb", "1chloedb", "chloedb-", "chloe/db", &"a".repeat(31)] {
            assert!(check_project(bad).is_err(), "{bad:?}");
        }
        assert!(check_resource("projects/chloedb/locations/us-central1-a/clusters/strata-search").is_ok());
        for bad in ["projects/chloedb/locations/us-central1-a/clusters/../x", "projects/p/zones/z/clusters/c", "projects/p/locations/l/clusters/c?x=1"] {
            assert!(check_resource(bad).is_err(), "{bad:?}");
        }
        assert_eq!(location_of("projects/p/locations/us-central1/clusters/strata-x"), ("us-central1", "strata-x"));
        assert!(check_location("us-central1").is_ok() && check_location("../x").is_err() && check_location("nyc3").is_err());
    }

    #[test]
    fn errors_say_what_google_said() {
        assert_eq!(error_text(r#"{"error":{"code":403,"message":"Compute Engine API has not been used in project chloedb","status":"PERMISSION_DENIED"}}"#), "PERMISSION_DENIED: Compute Engine API has not been used in project chloedb");
        assert_eq!(error_text(r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#), "invalid_grant: Token has been expired or revoked.");
        assert_eq!(enc("1//a b+c"), "1%2F%2Fa%20b%2Bc");
    }

    #[test]
    fn the_tests_stand_in_takes_every_service() {
        let g = Gcp::memory().with_endpoint("http://127.0.0.1:9");
        assert_eq!(g.base("container"), "http://127.0.0.1:9/container");
        assert_eq!(g.rewrite("https://iamcredentials.googleapis.com/v1/projects/-/serviceAccounts/a@b:generateAccessToken"), "http://127.0.0.1:9/iamcredentials/v1/projects/-/serviceAccounts/a@b:generateAccessToken");
        assert_eq!(g.rewrite("https://example.com/x"), "https://example.com/x");
        assert_eq!(Gcp::memory().base("compute"), "https://compute.googleapis.com");
    }

    #[test]
    fn the_saved_project_holds_no_secret_and_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = Gcp::file_of(&dir.path().join("x.strata"));
        assert!(path.ends_with("x.strata.gcp.json"));
        let s = Saved { project: "chloedb".into(), region: "us-central1".into(), account: "a@chloedb.iam.gserviceaccount.com".into(), number: "1".into(), saved_ms: 1, by: "token".into() };
        write_private(&path, &serde_json::to_vec(&s).unwrap()).unwrap();
        let g = Gcp::open(path.clone()).unwrap();
        assert_eq!(g.saved(), Some(s));
        assert_eq!(g.forget().unwrap().as_deref(), Some("chloedb"));
        assert!(!path.exists());
    }
}
