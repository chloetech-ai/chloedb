//! The cloud view's Google Cloud routes against a stand-in for Google's
//! APIs: calls made as Application Default Credentials that impersonate a
//! service account (a person's gcloud sign-in, never a key), no token or
//! refresh token in an answer, a file or the audit log, a service account
//! key refused, the project checked before it is kept, the machine types
//! priced from the Cloud Billing Catalog through its pages, a plan and its
//! checks, and only an administrator reaching any of it. The stand-in
//! ([`google`]) also plays GKE for `cloud_clusters`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{OriginalUri, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::IntoResponse;
use serde_json::{json, Value as Json};
use chloedb::db::Db;
use chloedb::storage::DbOptions;
use chloedb_server::auth::{Access, Secret};
use chloedb_server::gcp::Gcp;
use chloedb_server::keys::Keys;
use chloedb_server::service::{AgentConfig, Backend, Service};
use chloedb_server::{demo, server};
use tokio::net::TcpListener;

pub const REFRESH: &str = "1//refresh-token-never-shown-0123456789";
pub const SOURCE: &str = "ya29.source-token-never-shown-abcdef";
pub const TOKEN: &str = "ya29.deployer-token-never-shown-012345";
pub const DEPLOYER: &str = "chloedb-deployer@chloedb.iam.gserviceaccount.com";
pub const CLUSTER: &str = "/container/v1/projects/chloedb/locations/us-central1-a/clusters/strata-search";

const MACHINE_TYPES: &str = include_str!("fixtures/gcp/machine-types.json");
const SKUS: &str = include_str!("fixtures/gcp/skus.json");
const REGION: &str = include_str!("fixtures/gcp/region.json");
const PROJECT: &str = include_str!("fixtures/gcp/project.json");
const SERVER_CONFIG: &str = include_str!("fixtures/gcp/server-config.json");

/// What the stand-in was asked, and the GKE cluster it made.
#[derive(Default)]
pub struct Google {
    pub calls: Vec<String>,
    /// The made cluster's API (a stand-in of Kubernetes').
    pub kube: String,
    pub cluster: Option<Json>,
    /// GETs of the cluster while it is being made, or deleted.
    pub gets: u32,
    /// Bodies of node pool changes, in order.
    pub pools: Vec<(String, Json)>,
    /// Permissions testIamPermissions leaves out.
    pub missing: Vec<String>,
}

fn google_error(status: StatusCode, message: &str, code: &str) -> axum::response::Response {
    (status, json!({"error": {"code": status.as_u16(), "message": message, "status": code}}).to_string()).into_response()
}

/// Google's token endpoint, IAM Credentials, Compute Engine, GKE, the
/// Cloud Billing Catalog and Resource Manager, as far as the view uses
/// them, under `/<service>/` (as `Gcp::with_endpoint` sends them); and no
/// metadata server.
pub async fn google(State(g): State<Arc<Mutex<Google>>>, method: Method, OriginalUri(uri): OriginalUri, headers: HeaderMap, body: Bytes) -> axum::response::Response {
    let path = uri.path().to_string();
    let query = uri.query().unwrap_or_default().to_string();
    let mut g = g.lock().unwrap();
    g.calls.push(format!("{method} {path}"));
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().trim_start_matches("Bearer ").to_string();
    let ok = |j: Json| j.to_string().into_response();
    if path.starts_with("/metadata/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    if path == "/oauth2/token" {
        let form = String::from_utf8_lossy(&body).to_string();
        assert!(form.contains("grant_type=refresh_token"), "{form}");
        if !form.contains(&format!("refresh_token={}", REFRESH.replace('/', "%2F"))) {
            return (StatusCode::BAD_REQUEST, json!({"error": "invalid_grant", "error_description": "Token has been expired or revoked."}).to_string()).into_response();
        }
        return ok(json!({"access_token": SOURCE, "expires_in": 3599, "token_type": "Bearer"}));
    }
    if let Some(rest) = path.strip_prefix("/iamcredentials/v1/projects/-/serviceAccounts/") {
        assert_eq!(method, Method::POST);
        assert_eq!(auth, SOURCE, "impersonation is asked with the person's own token");
        let b: Json = serde_json::from_slice(&body).unwrap();
        assert_eq!(b["scope"], json!(["https://www.googleapis.com/auth/cloud-platform"]));
        if rest != format!("{DEPLOYER}:generateAccessToken") {
            return google_error(StatusCode::FORBIDDEN, "Permission 'iam.serviceAccounts.getAccessToken' denied on resource (or it may not exist).", "PERMISSION_DENIED");
        }
        return ok(json!({"accessToken": TOKEN, "expireTime": "2026-11-16T05:00:00Z"}));
    }
    if auth != TOKEN {
        return google_error(StatusCode::UNAUTHORIZED, "Request had invalid authentication credentials.", "UNAUTHENTICATED");
    }
    const P: &str = "/compute/compute/v1/projects/chloedb";
    let zones = ["us-central1-a", "us-central1-b", "us-central1-c", "us-central1-f"];
    match (method.as_str(), path.as_str()) {
        ("GET", P) => PROJECT.to_string().into_response(),
        ("GET", p) if p.starts_with("/compute/compute/v1/projects/") && p.matches('/').count() == 4 => google_error(StatusCode::FORBIDDEN, "Required 'compute.projects.get' permission for 'projects/other-project'", "PERMISSION_DENIED"),
        ("GET", p) if p == format!("{P}/regions") => ok(json!({"items": [
            {"name": "us-central1", "description": "us-central1", "status": "UP", "zones": zones.iter().map(|z| format!("https://www.googleapis.com/compute/v1/projects/chloedb/zones/{z}")).collect::<Vec<_>>()},
            {"name": "europe-west1", "description": "europe-west1", "status": "UP", "zones": ["https://www.googleapis.com/compute/v1/projects/chloedb/zones/europe-west1-b"]},
            {"name": "us-old1", "description": "us-old1", "status": "DOWN", "zones": []},
        ]})),
        ("GET", p) if p == format!("{P}/regions/us-central1") => REGION.to_string().into_response(),
        ("GET", p) if p.starts_with(&format!("{P}/regions/")) && p.ends_with("/subnetworks") => ok(json!({"items": [{"name": "default", "network": "https://www.googleapis.com/compute/v1/projects/chloedb/global/networks/default"}]})),
        ("GET", p) if p.starts_with(&format!("{P}/regions/")) => google_error(StatusCode::NOT_FOUND, "The resource 'projects/chloedb/regions/mars1' was not found", "NOT_FOUND"),
        ("GET", p) if p.starts_with(&format!("{P}/zones/us-central1-")) && p.ends_with("/machineTypes") => {
            assert!(query.contains("maxResults=500"), "{query}");
            MACHINE_TYPES.to_string().into_response()
        }
        ("GET", p) if p == format!("{P}/global/networks") => ok(json!({"items": [{"name": "default", "autoCreateSubnetworks": true}]})),
        ("GET", "/cloudbilling/v1/services/6F81-5844-456A/skus") => {
            assert!(query.contains("currencyCode=USD"), "{query}");
            let all: Json = serde_json::from_str(SKUS).unwrap();
            let skus = all["skus"].as_array().unwrap();
            // Two pages, as the catalogue answers seven.
            let half = skus.len() / 2;
            if query.contains("pageToken=page-2") {
                ok(json!({"skus": &skus[half..], "nextPageToken": ""}))
            } else {
                ok(json!({"skus": &skus[..half], "nextPageToken": "page-2"}))
            }
        }
        ("POST", "/cloudresourcemanager/v1/projects/chloedb:testIamPermissions") => {
            let b: Json = serde_json::from_slice(&body).unwrap();
            let held: Vec<Json> = b["permissions"].as_array().unwrap().iter().filter(|p| !g.missing.iter().any(|m| p.as_str() == Some(m))).cloned().collect();
            ok(json!({"permissions": held}))
        }
        ("GET", "/container/v1/projects/chloedb/locations/us-central1/serverConfig") => SERVER_CONFIG.to_string().into_response(),
        ("GET", "/container/v1/projects/chloedb/locations/-/clusters") => {
            let mut list = vec![
                json!({"name": "strata-other", "location": "us-central1-b", "status": "RUNNING", "resourceLabels": {"strata": "chloedb", "strata-owner": "someone"}, "currentNodeCount": 3, "createTime": "2026-11-15T10:00:00+00:00", "nodePools": [{"name": "strata", "config": {"machineType": "n2d-highmem-4"}}]}),
                json!({"name": "billing", "location": "us-central1-a", "status": "RUNNING", "resourceLabels": {}}),
            ];
            list.extend(g.cluster.clone());
            ok(json!({"clusters": list}))
        }
        ("POST", "/container/v1/projects/chloedb/locations/us-central1-a/clusters") => {
            let b: Json = serde_json::from_slice(&body).unwrap();
            let c = &b["cluster"];
            assert_eq!((c["name"].as_str(), c["initialClusterVersion"].as_str(), c["releaseChannel"]["channel"].as_str()), (Some("strata-search"), Some("1.36.5-gke.2370000"), Some("UNSPECIFIED")), "{b}");
            assert_eq!((c["workloadIdentityConfig"]["workloadPool"].as_str(), c["network"].as_str(), c["resourceLabels"]["strata"].as_str()), (Some("chloedb.svc.id.goog"), Some("default"), Some("chloedb")), "{b}");
            assert!(c["resourceLabels"]["strata-owner"].is_string(), "who asked for it, as a label: {b}");
            let pool = &c["nodePools"][0];
            assert_eq!((pool["name"].as_str(), pool["initialNodeCount"].as_u64(), pool["config"]["machineType"].as_str()), (Some("strata"), Some(2), Some("e2-standard-4")), "{b}");
            assert_eq!(pool["config"]["workloadMetadataConfig"]["mode"], "GKE_METADATA");
            if g.cluster.is_some() {
                return google_error(StatusCode::CONFLICT, "Already exists: projects/chloedb/locations/us-central1-a/clusters/strata-search.", "ALREADY_EXISTS");
            }
            let mut made = c.clone();
            made["location"] = json!("us-central1-a");
            made["status"] = json!("PROVISIONING");
            made["endpoint"] = json!(g.kube);
            made["masterAuth"] = json!({"clusterCaCertificate": ""});
            made["currentNodeCount"] = json!(2);
            made["createTime"] = json!("2026-11-16T03:00:00+00:00");
            made["nodePools"][0]["locations"] = json!(["us-central1-a"]);
            g.cluster = Some(made);
            g.gets = 0;
            ok(json!({"name": "operation-1", "operationType": "CREATE_CLUSTER", "status": "RUNNING"}))
        }
        ("GET", CLUSTER) => {
            let Some(c) = g.cluster.clone() else {
                return google_error(StatusCode::NOT_FOUND, "Not found: projects/chloedb/locations/us-central1-a/clusters/strata-search.", "NOT_FOUND");
            };
            g.gets += 1;
            match (c["status"].as_str(), g.gets) {
                (Some("PROVISIONING"), n) if n >= 2 => g.cluster.as_mut().unwrap()["status"] = json!("RUNNING"),
                (Some("STOPPING"), n) if n >= 2 => g.cluster = None,
                _ => {}
            }
            ok(c)
        }
        ("DELETE", CLUSTER) => {
            let Some(c) = g.cluster.as_mut() else {
                return google_error(StatusCode::NOT_FOUND, "Not found", "NOT_FOUND");
            };
            c["status"] = json!("STOPPING");
            g.gets = 0;
            ok(json!({"name": "operation-2", "operationType": "DELETE_CLUSTER", "status": "RUNNING"}))
        }
        ("POST", p) if p.starts_with(&format!("{CLUSTER}/nodePools/strata:")) => {
            let b: Json = serde_json::from_slice(&body).unwrap();
            let what = p.rsplit(':').next().unwrap().to_string();
            let c = g.cluster.as_mut().expect("a cluster to resize");
            if what == "setSize" {
                c["currentNodeCount"] = b["nodeCount"].clone();
            } else {
                c["nodePools"][0]["autoscaling"] = b["autoscaling"].clone();
            }
            g.pools.push((what, b));
            ok(json!({"name": "operation-3", "status": "RUNNING"}))
        }
        (m, p) => panic!("Google was not asked for {m} {p}"),
    }
}

pub async fn listen() -> (TcpListener, String) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    (l, url)
}

/// The stand-in, serving; `kube` is the made cluster's API.
pub async fn google_api(kube: &str) -> (Arc<Mutex<Google>>, String) {
    let g = Arc::new(Mutex::new(Google { kube: kube.to_string(), ..Default::default() }));
    let (l, url) = listen().await;
    let app = axum::Router::new().fallback(google).with_state(g.clone());
    tokio::spawn(async move { axum::serve(l, app).await });
    (g, url)
}

/// The file `gcloud auth application-default login
/// --impersonate-service-account DEPLOYER` writes, in `dir`.
pub fn adc_file(dir: &Path) -> PathBuf {
    let p = dir.join("application_default_credentials.json");
    let j = json!({
        "type": "impersonated_service_account", "delegates": [],
        "service_account_impersonation_url": format!("https://iamcredentials.googleapis.com/v1/projects/-/serviceAccounts/{DEPLOYER}:generateAccessToken"),
        "source_credentials": {"type": "authorized_user", "client_id": "764086051850-x.apps.googleusercontent.com", "client_secret": "d-FL95Q19q7MQmFpd7hHD0Ty", "refresh_token": REFRESH},
    });
    std::fs::write(&p, j.to_string()).unwrap();
    p
}

async fn call(base: &str, method: &str, path: &str, auth: Option<&str>, body: Option<Json>) -> (u16, String) {
    let c = chloedb_server::http_client().build().unwrap();
    let mut r = match method {
        "GET" => c.get(format!("{base}{path}")),
        "DELETE" => c.delete(format!("{base}{path}")),
        "PUT" => c.put(format!("{base}{path}")),
        _ => c.post(format!("{base}{path}")),
    };
    if let Some(a) = auth {
        r = r.bearer_auth(a);
    }
    if let Some(b) = body {
        r = r.json(&b);
    }
    let r = r.send().await.unwrap();
    (r.status().as_u16(), r.text().await.unwrap())
}

fn parse(s: &str) -> Json {
    serde_json::from_str(s).unwrap_or_else(|e| panic!("{e}: {s}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn the_google_cloud_view_signs_in_without_keys_and_plans_from_googles_prices() {
    let (seen, api) = google_api("http://127.0.0.1:1").await;
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create(&dir.path().join("a.strata"), demo::schema(10), DbOptions::default()).unwrap();
    let schema = (*db.schema()).clone();
    let agent = AgentConfig { api_key: None, model: String::new(), base_url: String::new() };
    let mut svc = Service::new(Backend::Local(Arc::new(db)), schema, agent, 4, Duration::from_secs(10));
    svc.keys = Arc::new(Keys::open(dir.path().join("a.strata.keys.json")).unwrap());
    let file = Gcp::file_of(&dir.path().join("a.strata"));
    let adc = adc_file(dir.path());
    svc.gcp = Arc::new(Gcp::open(file.clone()).unwrap().with_endpoint(&api).with_adc(Some(adc.clone())));
    let svc = Arc::new(svc);
    let (l, base) = listen().await;
    let access = Access { token: Some(Secret::new("root-token")), ..Default::default() };
    tokio::spawn(server::serve_http(svc.clone(), l, access));
    let root = Some("root-token");
    let mut answers = Vec::new();

    // Only an administrator reaches it.
    let (status, j) = call(&base, "POST", "/v1/admin/keys", root, Some(json!({"name": "ingest", "scope": "write"}))).await;
    assert_eq!(status, 200, "{j}");
    let writer = parse(&j)["secret"].as_str().unwrap().to_string();
    for (method, path, body) in [("GET", "/v1/admin/gcp", None), ("PUT", "/v1/admin/gcp/project", Some(json!({"project": "chloedb"}))), ("GET", "/v1/admin/gcp/machines", None), ("POST", "/v1/admin/gcp/plan", Some(json!({}))), ("POST", "/v1/admin/gcp/nodes", Some(json!({"nodes": 3})))] {
        assert_eq!(call(&base, method, path, None, body.clone()).await.0, 401, "{method} {path}");
        assert_eq!(call(&base, method, path, Some(&writer), body).await.0, 403, "{method} {path}");
    }
    assert!(!file.exists() && seen.lock().unwrap().calls.is_empty());

    // Whose credentials, from the file gcloud wrote, and the project they
    // name: no call to Google yet, and no secret of the file shown.
    let (status, j) = call(&base, "GET", "/v1/admin/gcp", root, None).await;
    assert_eq!(status, 200, "{j}");
    let s = parse(&j);
    assert_eq!((s["credentials"]["source"].as_str(), s["credentials"]["kind"].as_str(), s["credentials"]["service_account"].as_str()), (Some("adc"), Some("impersonated_service_account"), Some(DEPLOYER)), "{j}");
    assert_eq!(s["project"], json!({"id": "chloedb", "from": "the impersonated service account"}), "{j}");
    assert!(!j.contains("d-FL95Q19q7MQmFpd7hHD0Ty"), "{j}");
    answers.push(j);

    // A project these credentials may not read is not kept.
    let (status, j) = call(&base, "PUT", "/v1/admin/gcp/project", root, Some(json!({"project": "other-project"}))).await;
    assert!(status == 400 && j.contains("did not let these credentials read the project other-project") && j.contains("PERMISSION_DENIED"), "{status} {j}");
    for (body, says) in [(json!({"project": "Bad_Project"}), "is not a project id"), (json!({}), "the body is")] {
        let (status, j) = call(&base, "PUT", "/v1/admin/gcp/project", root, Some(body)).await;
        assert!(status == 400 && j.contains(says), "{status} {j}");
    }
    assert!(!file.exists());

    // Its own: checked, then kept (mode 0600), holding no credentials.
    let (status, j) = call(&base, "PUT", "/v1/admin/gcp/project", root, Some(json!({"project": "chloedb", "region": "us-central1"}))).await;
    assert_eq!(status, 200, "{j}");
    let s = parse(&j);
    assert_eq!((s["saved"]["project"].as_str(), s["saved"]["region"].as_str(), s["saved"]["account"].as_str(), s["project"]["from"].as_str()), (Some("chloedb"), Some("us-central1"), Some(DEPLOYER), Some("stored")), "{j}");
    answers.push(j);
    let kept = std::fs::read_to_string(&file).unwrap();
    answers.push(kept);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    }
    {
        let calls = seen.lock().unwrap().calls.clone();
        // The person's token, then the service account's, then the project.
        assert_eq!(calls[..3], ["POST /oauth2/token".to_string(), format!("POST /iamcredentials/v1/projects/-/serviceAccounts/{DEPLOYER}:generateAccessToken"), "GET /compute/compute/v1/projects/other-project".to_string()], "{calls:?}");
    }

    // The regions that are up, and us-central1's machine types, priced.
    let (status, j) = call(&base, "GET", "/v1/admin/gcp/regions", root, None).await;
    assert_eq!(status, 200, "{j}");
    let regions: Vec<String> = parse(&j)["regions"].as_array().unwrap().iter().map(|r| r["slug"].as_str().unwrap().to_string()).collect();
    assert_eq!(regions, ["europe-west1", "us-central1"]);
    let (status, j) = call(&base, "GET", "/v1/admin/gcp/machines?region=us-central1", root, None).await;
    assert_eq!(status, 200, "{j}");
    let cat = parse(&j);
    let m = cat["machines"].as_array().unwrap().iter().find(|m| m["type"] == "n2d-highmem-4").unwrap_or_else(|| panic!("{j}"));
    assert!((m["usd_hour"].as_f64().unwrap() - (4.0 * 0.027502 + 32.0 * 0.003686)).abs() < 1e-9, "{m}");
    assert_eq!((m["vcpu"].as_u64(), m["memory_gib"].as_f64(), m["max_pods"].as_u64()), (Some(4), Some(32.0), Some(110)), "{m}");
    assert!(cat["machines"].as_array().unwrap().iter().all(|m| !["t2a-standard-4", "g2-standard-4", "n4-standard-4", "c3-standard-4-lssd", "n1-highmem-96"].contains(&m["type"].as_str().unwrap())), "{j}");
    assert_eq!((cat["default_version"].as_str(), cat["networks"].as_array().map(Vec::len), cat["missing"].as_array().map(Vec::len)), (Some("1.36.5-gke.2370000"), Some(1), Some(0)), "{j}");
    assert!(cat["recommended"].as_array().unwrap().iter().any(|r| r == "n2d-highmem-4"), "{j}");
    answers.push(j);
    let (status, j) = call(&base, "GET", "/v1/admin/gcp/machines?region=mars1", root, None).await;
    assert!(status == 400 && j.contains("is not a region"), "{status} {j}");

    // A plan, and its checks: the permissions held, then one not held.
    let plan = json!({"name": "search", "region": "us-central1", "machine": "n2d-highmem-4", "nodes": 2, "volume_gb": 10, "cluster": {"shards": 2, "replicas": 2}});
    let (status, j) = call(&base, "POST", "/v1/admin/gcp/plan", root, Some(plan.clone())).await;
    assert_eq!(status, 200, "{j}");
    let p = parse(&j);
    assert_eq!((p["provider"].as_str(), p["location"].as_str(), p["quota"]["metric"].as_str()), (Some("gcp"), Some("us-central1-a"), Some("N2D_CPUS")), "{j}");
    assert_eq!(p["identity"]["service_account"], DEPLOYER);
    answers.push(j);
    let (status, j) = call(&base, "POST", "/v1/admin/gcp/validate", root, Some(plan.clone())).await;
    assert_eq!(status, 200, "{j}");
    assert_eq!(parse(&j)["validated"]["ok"], true, "{j}");
    seen.lock().unwrap().missing = vec!["container.clusters.create".into()];
    let (_, j) = call(&base, "POST", "/v1/admin/gcp/validate", root, Some(plan.clone())).await;
    let v = parse(&j);
    assert_eq!(v["validated"]["ok"], false, "{j}");
    assert!(v["validated"]["checks"].as_array().unwrap().iter().any(|c| c["check"] == "the credentials hold container.clusters.create" && c["ok"] == false), "{j}");
    answers.push(j);
    let (status, j) = call(&base, "POST", "/v1/admin/gcp/plan", root, Some(json!({"name": "search", "region": "us-central1", "machine": "t2a-standard-4", "nodes": 2, "cluster": {}}))).await;
    assert!(status == 400 && j.contains("is not a machine type a GKE node may be"), "{status} {j}");
    // Not on GKE: a resize names its cluster.
    let (status, j) = call(&base, "POST", "/v1/admin/gcp/nodes", root, Some(json!({"nodes": 3}))).await;
    assert!(status == 400 && j.contains("does not run on GKE"), "{status} {j}");

    // Only reads were made; the price list read through its pages, once.
    {
        let calls = seen.lock().unwrap().calls.clone();
        assert_eq!(calls.iter().filter(|c| c.starts_with("GET /cloudbilling/")).count(), 2, "{calls:?}");
        assert!(calls.iter().all(|c| c.starts_with("GET ") || c.starts_with("POST /oauth2/") || c.starts_with("POST /iamcredentials/") || c.ends_with(":testIamPermissions")), "{calls:?}");
        assert_eq!(calls.iter().filter(|c| c.starts_with("POST /iamcredentials/")).count(), 1, "the token is kept while it lasts: {calls:?}");
    }

    // A service account key in its place is refused, and says what to do.
    std::fs::write(&adc, json!({"type": "service_account", "client_email": DEPLOYER, "private_key": "-----BEGIN PRIVATE KEY-----\nMIIE\n-----END PRIVATE KEY-----\n"}).to_string()).unwrap();
    let (_, j) = call(&base, "GET", "/v1/admin/gcp", root, None).await;
    let s = parse(&j);
    assert_eq!(s["credentials"]["source"], "none", "{j}");
    assert!(s["credentials"]["refused"].as_str().unwrap().contains("--impersonate-service-account"), "{j}");
    assert!(!j.contains("PRIVATE KEY") && !j.contains("MIIE"), "{j}");

    // Forgetting deletes the file; the audit log says whose calls they were.
    let (status, j) = call(&base, "DELETE", "/v1/admin/gcp/project", root, None).await;
    assert_eq!(status, 200, "{j}");
    assert_eq!(parse(&j)["forgot"], "chloedb");
    assert!(!file.exists());
    let (_, j) = call(&base, "GET", "/v1/admin/audit", root, None).await;
    let details: Vec<String> = parse(&j)["entries"].as_array().unwrap().iter().filter_map(|e| e["detail"].as_str().map(String::from)).collect();
    assert!(details.iter().any(|d| d == &format!("saved the Google Cloud project chloedb (us-central1), called as {DEPLOYER}")), "{details:?}");
    assert!(details.iter().any(|d| d == "forgot the Google Cloud project chloedb"), "{details:?}");
    answers.push(j);
    for a in &answers {
        assert!(!a.contains(TOKEN) && !a.contains(SOURCE) && !a.contains(REFRESH) && !a.contains("123456789012"), "{a}");
    }
}
