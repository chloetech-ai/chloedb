//! Signing in to the command center with Google ([`chloedb_server::signin`]):
//! against a fake of Google's published keys, with ID tokens signed here
//! by a test key (`tests/fixtures/signin/key.pem`, made for these tests
//! and nothing else).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use chloedb::db::Db;
use chloedb::storage::DbOptions;
use chloedb_server::auth::{Access, Secret};
use chloedb_server::keys::{Keys, Scope};
use chloedb_server::service::{AgentConfig, Backend, Service};
use chloedb_server::signin::Google;
use chloedb_server::{demo, server};
use serde_json::{json, Value as Json};
use tokio::net::TcpListener;

const CLIENT: &str = "123-test.apps.googleusercontent.com";

struct Signer {
    key: ring::signature::RsaKeyPair,
}

impl Signer {
    fn load() -> Signer {
        let pem = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/signin/key.pem")).unwrap();
        let der = rustls_pemfile::private_key(&mut &pem[..]).unwrap().unwrap();
        Signer { key: ring::signature::RsaKeyPair::from_pkcs8(der.secret_der()).unwrap() }
    }

    /// The key as a JWKS lists it.
    fn jwk(&self, kid: &str) -> Json {
        let c: ring::signature::RsaPublicKeyComponents<Vec<u8>> = self.key.public().into();
        let b = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        json!({"kty": "RSA", "alg": "RS256", "use": "sig", "kid": kid, "n": b.encode(&c.n), "e": b.encode(&c.e)})
    }

    /// An ID token of `claims`, signed with key `kid` (`alg`: RS256 unless
    /// another is named, which this key still signs RS256).
    fn token(&self, kid: &str, alg: &str, claims: &Json) -> String {
        let b = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let signed = format!("{}.{}", b.encode(json!({"alg": alg, "kid": kid, "typ": "JWT"}).to_string()), b.encode(claims.to_string()));
        let mut sig = vec![0; self.key.public().modulus_len()];
        self.key.sign(&ring::signature::RSA_PKCS1_SHA256, &ring::rand::SystemRandom::new(), signed.as_bytes(), &mut sig).unwrap();
        format!("{signed}.{}", b.encode(sig))
    }
}

fn now_s() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

/// Claims as Google's button gives them, for `email`, with `nonce`.
fn claims(email: &str, nonce: &str) -> Json {
    let now = now_s();
    json!({"iss": "https://accounts.google.com", "aud": CLIENT, "azp": CLIENT, "sub": "1077", "email": email, "email_verified": true, "name": "Ro Konow", "iat": now - 5, "exp": now + 3600, "nonce": nonce})
}

/// Google's keys, counted each time they are fetched.
async fn certs(signer: &Signer) -> (String, Arc<AtomicUsize>) {
    let fetched = Arc::new(AtomicUsize::new(0));
    let body = json!({"keys": [signer.jwk("k1"), {"kty": "EC", "kid": "ec1", "crv": "P-256", "x": "AA", "y": "AA"}]});
    let f = fetched.clone();
    let app = axum::Router::new().route(
        "/certs",
        axum::routing::get(move || {
            let (f, body) = (f.clone(), body.clone());
            async move {
                f.fetch_add(1, Ordering::SeqCst);
                ([(axum::http::header::CACHE_CONTROL, "public, max-age=3600")], axum::Json(body))
            }
        }),
    );
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://127.0.0.1:{}/certs", l.local_addr().unwrap().port());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (url, fetched)
}

fn service(dir: &std::path::Path, keys: Option<Arc<Keys>>, google: Google) -> Arc<Service> {
    let db = Db::create(&dir.join(format!("a{}.strata", rand_suffix())), demo::schema(20), DbOptions { publish_snapshots: true, ..DbOptions::default() }).unwrap();
    let schema = (*db.schema()).clone();
    let agent = AgentConfig { api_key: None, model: String::new(), base_url: String::new() };
    let mut svc = Service::new(Backend::Local(Arc::new(db)), schema, agent, 16, Duration::from_secs(10));
    svc.keys = keys.unwrap_or_else(|| Arc::new(Keys::open(dir.join("keys.json")).unwrap()));
    svc.signin = Some(Arc::new(google));
    Arc::new(svc)
}

fn rand_suffix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64
}

async fn serve(svc: Arc<Service>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    let access = Access { token: Some(Secret::new("root-token")), ..Default::default() };
    tokio::spawn(server::serve_http(svc, l, access));
    base
}

/// An answer: status, body, and the cookies it set (`name=value; ...`).
struct Answer {
    status: u16,
    body: Json,
    cookies: Vec<String>,
}

impl Answer {
    fn cookie(&self, name: &str) -> Option<String> {
        self.cookies.iter().find_map(|c| c.strip_prefix(&format!("{name}="))).map(|v| v.split(';').next().unwrap_or("").to_string())
    }
}

/// `method path` with a `Cookie` header, an `Origin`, a bearer credential
/// and a body, each if given.
async fn call(base: &str, method: &str, path: &str, cookie: Option<&str>, origin: Option<&str>, bearer: Option<&str>, body: Option<Json>) -> Answer {
    let c = chloedb_server::http_client().build().unwrap();
    let mut r = if method == "GET" { c.get(format!("{base}{path}")) } else { c.post(format!("{base}{path}")) };
    if let Some(v) = cookie {
        r = r.header("cookie", v);
    }
    if let Some(o) = origin {
        r = r.header("origin", o);
    }
    if let Some(b) = bearer {
        r = r.bearer_auth(b);
    }
    if let Some(b) = body {
        r = r.json(&b);
    }
    let r = r.send().await.unwrap();
    let status = r.status().as_u16();
    let cookies = r.headers().get_all("set-cookie").iter().filter_map(|v| v.to_str().ok().map(String::from)).collect();
    Answer { status, body: r.json().await.unwrap_or(Json::Null), cookies }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_signs_in_with_google_and_their_session_is_checked_on_each_request() {
    let signer = Signer::load();
    let (certs_url, fetched) = certs(&signer).await;
    let dir = tempfile::tempdir().unwrap();
    let google = Google::new(CLIENT, &["ro@example.com=admin".into(), "@chloetech.ai=read".into()]).unwrap().with_certs_url(&certs_url);
    let svc = service(dir.path(), None, google);
    let base = serve(svc.clone()).await;
    let origin = base.clone();
    let o = Some(origin.as_str());

    // The page asks whether Google sign-in is on: the client and a nonce,
    // which a cookie of its own holds too.
    let start = call(&base, "GET", "/v1/auth/google", None, None, None, None).await;
    assert_eq!((start.status, start.body["enabled"].as_bool(), start.body["client_id"].as_str()), (200, Some(true), Some(CLIENT)), "{}", start.body);
    let nonce = start.body["nonce"].as_str().unwrap().to_string();
    assert_eq!(start.cookie("__Host-chloedb-nonce").as_deref(), Some(nonce.as_str()));
    assert!(start.cookies[0].contains("; Secure; HttpOnly; SameSite=Strict"), "{:?}", start.cookies);
    let nonce_cookie = format!("__Host-chloedb-nonce={nonce}");
    let nc = Some(nonce_cookie.as_str());
    let sign_in = |cred: String, cookie: Option<&str>, origin: Option<&str>| {
        let (base, cookie, origin) = (base.clone(), cookie.map(String::from), origin.map(String::from));
        async move { call(&base, "POST", "/v1/auth/google", cookie.as_deref(), origin.as_deref(), None, Some(json!({"credential": cred}))).await }
    };
    let good = signer.token("k1", "RS256", &claims("ro@example.com", &nonce));

    // Refused: from no page or another site's, without the nonce's
    // cookie, and every token that is not Google's for this client.
    assert_eq!(sign_in(good.clone(), nc, None).await.status, 403, "no Origin");
    assert_eq!(sign_in(good.clone(), nc, Some("https://evil.example")).await.status, 403, "another site's page");
    let a = sign_in(good.clone(), None, o).await;
    assert!(a.status == 401 && a.body["error"].as_str().unwrap().contains("nonce"), "{}", a.body);
    let with = |k: &str, v: Json| {
        let mut c = claims("ro@example.com", &nonce);
        c[k] = v;
        signer.token("k1", "RS256", &c)
    };
    for (what, cred, status, says) in [
        ("another client's", with("aud", json!("999-other.apps.googleusercontent.com")), 401, "another OAuth client"),
        ("expired", with("exp", json!(now_s() - 3600)), 401, "expired"),
        ("not Google's issuer", with("iss", json!("https://evil.example")), 401, "not Google's"),
        ("another nonce", with("nonce", json!("n0")), 401, "nonce"),
        ("an unverified address", with("email_verified", json!(false)), 403, "verified"),
        ("an unknown key", signer.token("k9", "RS256", &claims("ro@example.com", &nonce)), 401, "not one of Google's"),
        ("alg none", signer.token("k1", "none", &claims("ro@example.com", &nonce)), 401, "RS256"),
        ("not on the list", signer.token("k1", "RS256", &claims("eve@example.com", &nonce)), 403, "not on this server's list"),
        ("a Gmail address named like the domain", signer.token("k1", "RS256", &json!({"hd": null, "email": "ann@chloetech.ai"}).as_object().unwrap().iter().fold(claims("ann@chloetech.ai", &nonce), |mut c, (k, v)| { c[k] = v.clone(); c })), 403, "not on this server's list"),
    ] {
        let a = sign_in(cred, nc, o).await;
        assert_eq!(a.status, status, "{what}: {}", a.body);
        assert!(a.body["error"].as_str().unwrap_or("").contains(says), "{what}: {}", a.body);
        assert!(a.cookie("__Host-chloedb-session").is_none(), "{what}");
    }
    // A token whose claims were changed after Google signed it.
    let parts: Vec<&str> = good.split('.').collect();
    let b = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let forged = format!("{}.{}.{}", parts[0], b.encode(claims("eve@example.com", &nonce).to_string()), parts[2]);
    assert_eq!(sign_in(forged, nc, o).await.status, 401, "a forged token");
    // Google's keys were read once, and once more for the unknown key id
    // only after a while: not for each token.
    assert_eq!(fetched.load(Ordering::SeqCst), 1);

    // Signed in: the session in a cookie the page cannot read, the nonce's
    // ended.
    let a = sign_in(good.clone(), nc, o).await;
    assert_eq!(a.status, 200, "{}", a.body);
    assert_eq!((a.body["signed_in"]["email"].as_str(), a.body["scope"].as_str()), (Some("ro@example.com"), Some("admin")));
    let session = a.cookie("__Host-chloedb-session").expect("a session");
    assert!(session.starts_with("sk_"));
    assert!(a.cookies.iter().any(|c| c.starts_with("__Host-chloedb-session=sk_") && c.contains("Max-Age=604800; Secure; HttpOnly; SameSite=Strict")), "{:?}", a.cookies);
    assert!(a.cookies.iter().any(|c| c.starts_with("__Host-chloedb-nonce=;") && c.contains("Max-Age=0")), "{:?}", a.cookies);
    assert!(!a.body.to_string().contains(&session), "the secret is the cookie's alone");
    let sc = format!("__Host-chloedb-session={session}");
    let ro = Some(sc.as_str());

    let me = call(&base, "GET", "/v1/whoami", ro, None, None, None).await;
    assert_eq!((me.status, me.body["signed_in"]["email"].as_str(), me.body["scope"].as_str(), me.body["auth"]["google_sign_in"].as_bool()), (200, Some("ro@example.com"), Some("admin"), Some(true)), "{}", me.body);
    assert_eq!(call(&base, "GET", "/v1/whoami", None, None, None, None).await.status, 401);
    // A change by the cookie comes from a page of this server's.
    let mk = json!({"name": "made-by-ro", "scope": "read"});
    assert_eq!(call(&base, "POST", "/v1/admin/keys", ro, None, None, Some(mk.clone())).await.status, 403, "no Origin");
    assert_eq!(call(&base, "POST", "/v1/admin/keys", ro, Some("https://evil.example"), None, Some(mk.clone())).await.status, 403);
    let made = call(&base, "POST", "/v1/admin/keys", ro, o, None, Some(mk)).await;
    assert_eq!(made.status, 200, "{}", made.body);
    // A cookie holds sessions alone, not API keys.
    let api_key = made.body["secret"].as_str().unwrap().to_string();
    assert_eq!(call(&base, "GET", "/v1/whoami", Some(&format!("__Host-chloedb-session={api_key}")), None, None, None).await.status, 401);

    // A Workspace domain's account, at its entry's scope.
    let mut ann = claims("ann@chloetech.ai", &nonce);
    ann["hd"] = json!("chloetech.ai");
    let a = sign_in(signer.token("k1", "RS256", &ann), nc, o).await;
    assert_eq!((a.status, a.body["scope"].as_str()), (200, Some("read")), "{}", a.body);
    let ann_cookie = format!("__Host-chloedb-session={}", a.cookie("__Host-chloedb-session").unwrap());
    assert_eq!(call(&base, "POST", "/v1/admin/keys", Some(&ann_cookie), o, None, Some(json!({"name": "x", "scope": "read"}))).await.status, 403, "read");
    assert_eq!(call(&base, "POST", "/v1/query", Some(&ann_cookie), o, None, Some(json!({"sql": "SELECT id FROM products LIMIT 1"}))).await.status, 200);

    // Access lists the sessions as keys, by whose; the audit log keeps the
    // sign-ins and the refusals, and no secret or token of Google's.
    let keys = call(&base, "GET", "/v1/admin/keys", None, None, Some("root-token"), None).await.body;
    let sessions: Vec<&Json> = keys["keys"].as_array().unwrap().iter().filter(|k| k["session"] == "google" && k["revoked_ms"].is_null()).collect();
    assert_eq!(sessions.len(), 2, "{keys}");
    assert!(sessions.iter().any(|k| k["owner"] == "ro@example.com" && k["scope"] == "admin" && k["idle_ends_ms"].is_u64()));
    let audit = call(&base, "GET", "/v1/admin/audit?limit=200", None, None, Some("root-token"), None).await.body.to_string();
    assert!(audit.contains("signed in with Google as ro@example.com (admin)") && audit.contains("refused a Google sign-in") && audit.contains("not on this server's list"), "{audit}");
    for secret in [&session, &good, &api_key] {
        assert!(!audit.contains(secret.as_str()), "a secret in the audit log");
    }

    // The list as it is now decides: the same sessions on a server whose
    // list gives ro read and names no one of chloetech.ai.
    let lower = Google::new(CLIENT, &["ro@example.com=read".into()]).unwrap().with_certs_url(&certs_url);
    let other = serve(service(dir.path(), Some(svc.keys.clone()), lower)).await;
    let me = call(&other, "GET", "/v1/whoami", ro, None, None, None).await;
    assert_eq!(me.body["scope"].as_str(), Some("read"), "{}", me.body);
    let gone = call(&other, "GET", "/v1/whoami", Some(&ann_cookie), None, None, None).await;
    assert!(gone.status == 401 && gone.body["error"].as_str().unwrap().contains("may no longer sign in"), "{}", gone.body);

    // The hub: a person signed in to it drives a connection with the
    // connection's key, and the cluster's audit log names the person
    // beside the hub's key. A caller cannot name one itself through the hub.
    let far = tempfile::tempdir().unwrap();
    let remote = service(far.path(), None, Google::new(CLIENT, &["nobody@example.com".into()]).unwrap().with_certs_url(&certs_url));
    let (hub_key, hub_secret) = remote.keys.create("the-hub", Scope::Admin, None, true, false).unwrap();
    let remote_url = serve(remote).await;
    let added = call(&base, "POST", "/v1/admin/hub/connections", None, None, Some("root-token"), Some(json!({"name": "remote", "url": remote_url, "key": hub_secret}))).await;
    assert_eq!(added.status, 201, "{}", added.body);
    let via = call(&base, "POST", "/c/remote/v1/admin/keys", ro, o, None, Some(json!({"name": "made-through-the-hub", "scope": "read"}))).await;
    assert_eq!(via.status, 200, "{}", via.body);
    let r = chloedb_server::http_client().build().unwrap().post(format!("{base}/c/remote/v1/admin/keys")).bearer_auth("root-token").header("x-strata-person", "eve@example.com").json(&json!({"name": "made-by-the-token", "scope": "read"})).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let far_audit = call(&remote_url, "GET", "/v1/admin/audit?limit=50", None, None, Some("root-token"), None).await.body;
    let entries: Vec<Json> = far_audit["entries"].as_array().cloned().unwrap_or_default();
    let of = |name: &str| entries.iter().find(|e| e["detail"].as_str().is_some_and(|d| d.contains(name))).cloned().unwrap_or_else(|| panic!("no entry for {name}: {far_audit}"));
    let e = of("made-through-the-hub");
    assert_eq!((e["who"].as_str(), e["key"].as_str(), e["person"].as_str()), (Some("the-hub"), Some(hub_key.id.as_str()), Some("ro@example.com")), "{e}");
    assert!(of("made-by-the-token")["person"].is_null(), "only the hub names a person: {far_audit}");

    // Signing out ends the session everywhere, and the cookie.
    let out = call(&base, "POST", "/v1/auth/signout", ro, o, None, None).await;
    assert_eq!((out.status, out.body["signed_out"].as_bool()), (200, Some(true)), "{}", out.body);
    assert!(out.cookies.iter().any(|c| c.starts_with("__Host-chloedb-session=;") && c.contains("Max-Age=0")));
    let after = call(&base, "GET", "/v1/whoami", ro, None, None, None).await;
    assert!(after.status == 401 && after.body["error"].as_str().unwrap().contains("sign in again"), "{}", after.body);
    // A cookie of no session signs nothing out.
    assert_eq!(call(&base, "POST", "/v1/auth/signout", Some(&format!("__Host-chloedb-session={api_key}")), o, None, None).await.body["signed_out"], false);
    assert_eq!(call(&base, "GET", "/v1/whoami", None, None, Some(&api_key), None).await.status, 200, "the API key still works");
}

#[tokio::test(flavor = "multi_thread")]
async fn without_google_sign_in_a_server_says_so_and_takes_no_cookie() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create(&dir.path().join("a.strata"), demo::schema(5), DbOptions::default()).unwrap();
    let schema = (*db.schema()).clone();
    let agent = AgentConfig { api_key: None, model: String::new(), base_url: String::new() };
    let mut svc = Service::new(Backend::Local(Arc::new(db)), schema, agent, 16, Duration::from_secs(10));
    svc.keys = Arc::new(Keys::open(dir.path().join("keys.json")).unwrap());
    let (_, secret) = svc.keys.create_session("google", "ro@example.com", chloedb_server::keys::Scope::Admin, true, false).unwrap();
    let base = serve(Arc::new(svc)).await;
    assert_eq!(call(&base, "GET", "/v1/auth/google", None, None, None, None).await.body, json!({"enabled": false}));
    assert_eq!(call(&base, "POST", "/v1/auth/google", None, Some(&base), None, Some(json!({"credential": "x.y.z"}))).await.status, 404);
    // A session made while sign-in was on opens nothing once it is off.
    assert_eq!(call(&base, "GET", "/v1/whoami", Some(&format!("__Host-chloedb-session={secret}")), None, None, None).await.status, 401);
    assert_eq!(call(&base, "GET", "/v1/whoami", None, None, Some(&secret), None).await.status, 401);
}
