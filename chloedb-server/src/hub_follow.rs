//! Setting one cluster of the hub's book to follow another
//! (`POST /v1/admin/hub/follow`, [`crate::follow`]): the hub makes a key
//! of the `feed` scope on the source, keeps it in a Secret of the
//! follower's namespace, and sets the follower's `spec.follow`, after
//! which the operator runs the follow process there
//! ([`crate::operator::follow`]). The hub is not on the data path: the
//! follower reads the source itself, so a laptop asleep stops nothing.
//!
//! The follower must be run by the operator. The hub writes the Secret
//! through the follower's Kubernetes API, which it holds for a cluster it
//! made from the Cloud view; for another, it says what to make
//! (`kubectl create secret`) and takes the Secret's name instead
//! (`key_secret`). The source must be reachable from the follower's pods:
//! at its address in the book, the public URL it says it has, or the one
//! given (`source_url`); a cluster made here has none until it is exposed.
//! With `plan`, nothing is made: the answer says what would be.

use std::sync::Arc;

use serde_json::{json, Value as Json};

use crate::keys::{Principal, Scope};
use crate::service::Service;
use crate::ApiError;

/// What `POST /v1/admin/hub/follow` is asked.
#[derive(Clone, Debug, Default)]
pub struct Ask {
    /// The connections: the follower, and the cluster it follows.
    pub follower: String,
    pub source: String,
    /// The name the follower knows the source by (default: the source's
    /// connection name).
    pub name: Option<String>,
    /// `follow` (default), `copy` or `reverse` (`spec.follow.start`).
    pub start: Option<String>,
    /// The source's URL as the follower's pods reach it.
    pub source_url: Option<String>,
    /// A Secret of the follower's namespace that holds a feed key of the
    /// source already (key `key`): the hub makes none.
    pub key_secret: Option<String>,
    /// A Secret with the source's CA certificate (`ca.crt`).
    pub ca_secret: Option<String>,
    /// Only say what would be done.
    pub plan: bool,
}

impl Ask {
    pub fn from(b: &Json) -> Result<Ask, ApiError> {
        let s = |k: &str| b[k].as_str().map(str::trim).filter(|v| !v.is_empty()).map(String::from);
        Ok(Ask {
            follower: s("follower").ok_or_else(|| ApiError::bad("follower is the connection that is to follow"))?,
            source: s("source").ok_or_else(|| ApiError::bad("source is the connection it is to follow"))?,
            name: s("name"),
            start: s("start"),
            source_url: s("source_url"),
            key_secret: s("key_secret"),
            ca_secret: s("ca_secret"),
            plan: b["plan"].as_bool().unwrap_or(false),
        })
    }
}

/// A call to connection `name` through the hub ([`crate::hub::Hub::forward`]),
/// its JSON answer, or its error as the cluster said it.
pub async fn call(svc: &Arc<Service>, name: &str, method: reqwest::Method, path: &str, body: Option<&Json>) -> Result<Json, ApiError> {
    let k = crate::copy::kube_for(svc, name).await?;
    let mut h = axum::http::HeaderMap::new();
    h.insert(axum::http::header::CONTENT_TYPE, axum::http::HeaderValue::from_static("application/json"));
    let b = body.map(|b| reqwest::Body::from(b.to_string())).unwrap_or_else(|| reqwest::Body::from(""));
    let r = svc.hub.forward(name, method, path, &h, b, k.as_ref()).await?;
    let status = r.status().as_u16();
    let raw = axum::body::to_bytes(r.into_body(), 16 << 20).await.map_err(|e| ApiError { status: 502, message: format!("{name}'s answer: {e}") })?;
    let j: Json = serde_json::from_slice(&raw).unwrap_or_else(|_| json!({"error": String::from_utf8_lossy(&raw).chars().take(300).collect::<String>()}));
    if !(200..300).contains(&status) {
        let why = j["error"].as_str().map(String::from).unwrap_or_else(|| j.to_string());
        return Err(ApiError { status: if status >= 500 { 502 } else { status }, message: format!("{name}: {why}") });
    }
    Ok(j)
}

/// The URL the follower's pods reach the source at, and why it is that
/// one; or why there is none.
fn source_url(c: &crate::hub::Connection, given: Option<&str>) -> Result<(String, &'static str), String> {
    if let Some(u) = given {
        return Ok((u.trim_end_matches('/').to_string(), "as given"));
    }
    if let Some(u) = c.cluster["public_url"].as_str().filter(|u| !u.is_empty()) {
        return Ok((u.trim_end_matches('/').to_string(), "the public URL the source says it has"));
    }
    let Some(u) = c.urls.first() else {
        return Err(format!(
            "{} is reached through its Kubernetes API only (it has no public address), which the follower's pods cannot use: expose its coordinators (a LoadBalancer Service with TLS) and give that URL (source_url)",
            c.name
        ));
    };
    let loopback = reqwest::Url::parse(u).ok().and_then(|u| u.host_str().map(|h| h == "localhost" || h.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback()))).unwrap_or(false);
    if loopback {
        return Err(format!("{} is at {u}, an address of this machine that the follower's pods do not reach: give the URL they reach it at (source_url)", c.name));
    }
    Ok((u.trim_end_matches('/').to_string(), "its address in the book"))
}

/// Says what setting `a.follower` to follow `a.source` does, and with
/// `!a.plan` does it ([the module](self)), as `who`.
pub async fn follow(svc: &Arc<Service>, who: &Principal, a: &Ask) -> Result<Json, ApiError> {
    who.may(Scope::Admin, "setting a cluster to follow another")?;
    if a.follower == a.source {
        return Err(ApiError::bad(format!("{} cannot follow itself", a.follower)));
    }
    let hub = &svc.hub;
    let f = hub.get(&a.follower).ok_or_else(|| ApiError { status: 404, message: format!("no connection {} in the book", a.follower) })?;
    let s = hub.get(&a.source).ok_or_else(|| ApiError { status: 404, message: format!("no connection {} in the book", a.source) })?;
    let name = a.name.clone().unwrap_or_else(|| a.source.clone());
    if !crate::service::follow_name_ok(&name) {
        return Err(ApiError::bad(format!("{name:?} is not a name the follower can know the source by: 1 to 64 letters, digits, '.', '_' or '-' (give name)")));
    }
    let start = a.start.clone().unwrap_or_else(|| "follow".into());
    let (kube, now_following, rows) = tokio::join!(
        call(svc, &a.follower, reqwest::Method::GET, "/v1/kube", None),
        call(svc, &a.follower, reqwest::Method::GET, "/v1/admin/follow", None),
        call(svc, &a.source, reqwest::Method::GET, "/v1/health", None),
    );
    let kube = kube?;
    let mut refused: Vec<String> = Vec::new();
    if kube["managed"] != true {
        refused.push(format!("{} is not run by the Kubernetes operator, which runs the follow process: run chloedb-server follow yourself (docs: Following another cluster)", a.follower));
    }
    match &now_following {
        Ok(v) => {
            if let Some(n) = v["following"].as_str().filter(|n| *n != name) {
                refused.push(format!("{} follows {n} already: stop that first", a.follower));
            }
        }
        Err(e) => refused.push(format!("{}'s follow view: {}", a.follower, e.message)),
    }
    if let Some(n) = kube["spec"]["follow"]["name"].as_str().filter(|n| *n != name) {
        refused.push(format!("{}'s operator runs a follow process of {n} already: stop it first", a.follower));
    }
    let url = match source_url(&s, a.source_url.as_deref()) {
        Ok(u) => Some(u),
        Err(e) => {
            refused.push(e);
            None
        }
    };
    if s.pin.is_some() && a.ca_secret.is_none() && a.source_url.is_none() {
        refused.push(format!("{}'s certificate is self-signed (pinned here): give a Secret of the follower's namespace with its CA certificate (ca_secret, key ca.crt)", a.source));
    }
    let ns = kube["namespace"].as_str().unwrap_or_default().to_string();
    let cluster = kube["name"].as_str().unwrap_or_default().to_string();
    let secret = a.key_secret.clone().unwrap_or_else(|| format!("{cluster}-follow-{}", name.to_ascii_lowercase().replace(['_', '.'], "-")));
    let hub_writes_secret = a.key_secret.is_none();
    if hub_writes_secret && f.kube.is_none() {
        refused.push(format!(
            "the hub reaches {} at its address, not through its Kubernetes API, so it cannot keep the source's key there: make a feed key on {} (Access), keep it in a Secret (kubectl -n {ns} create secret generic {secret} --from-literal=key=<the key>) and give its name (key_secret)",
            a.follower, a.source
        ));
    }
    let rows = rows.ok().and_then(|h| h["rows"].as_u64().or(h["docs"].as_u64()));
    let mut steps = Vec::new();
    if hub_writes_secret {
        steps.push(format!("a key of {} of the feed scope (\"follow {}\"), which reads its feed and nothing else, kept in the Secret {ns}/{secret} of {}", a.source, a.follower, a.follower));
    } else {
        steps.push(format!("the key in the Secret {ns}/{secret}"));
    }
    steps.push(format!("{}'s spec.follow: the operator starts a follow process ({cluster}-follow), which reads {} at {}", a.follower, a.source, url.as_ref().map_or("?", |u| u.0.as_str())));
    steps.push(match start.as_str() {
        "copy" => format!("it copies {} afresh ({} rows), writing them over {}'s and deleting the keys {} lacks", a.source, rows.map_or("?".into(), |r| r.to_string()), a.follower, a.source),
        "reverse" => format!("it follows {} back from its promotion from {}, with no copy (refused if {} took writes since)", a.source, a.follower, a.follower),
        _ => format!("it copies {} the first time ({} rows), then applies its changes as they come", a.source, rows.map_or("?".into(), |r| r.to_string())),
    });
    steps.push(format!("from its first write, {} refuses its clients' writes (\"this cluster follows {name}\") until it stops following or is promoted", a.follower));
    let mut out = json!({
        "follower": a.follower, "source": a.source, "name": name, "start": start,
        "source_url": url.as_ref().map(|u| &u.0), "source_url_from": url.as_ref().map(|u| u.1),
        "secret": format!("{ns}/{secret}"), "hub_writes_secret": hub_writes_secret, "rows": rows,
        "steps": steps, "refused": if refused.is_empty() { Json::Null } else { json!(refused.join("; ")) },
    });
    if a.plan {
        return Ok(out);
    }
    if !refused.is_empty() {
        return Err(ApiError { status: 409, message: refused.join("; ") });
    }
    let url = url.expect("no refusal: a URL").0;
    if hub_writes_secret {
        let k = crate::copy::kube_for(svc, &a.follower).await?.ok_or_else(|| ApiError { status: 409, message: format!("{} has no Kubernetes API here", a.follower) })?;
        let made = call(svc, &a.source, reqwest::Method::POST, "/v1/admin/keys", Some(&json!({"name": format!("follow {}", a.follower), "scope": "feed", "owner": format!("hub {}", hub.device), "note": format!("{} follows {} with it (set up from the hub)", a.follower, a.source)}))).await?;
        let key = made["secret"].as_str().ok_or_else(|| ApiError { status: 502, message: format!("{} made a key but answered no secret", a.source) })?;
        let obj = json!({
            "apiVersion": "v1", "kind": "Secret", "type": "Opaque",
            "metadata": {"name": secret, "namespace": ns, "labels": {"app.kubernetes.io/instance": cluster, "app.kubernetes.io/component": "follow-key"}},
            "stringData": {"key": key},
        });
        if let Err(e) = k.apply(&format!("/api/v1/namespaces/{ns}/secrets/{secret}"), &obj).await {
            // A key no Secret holds is of no use: revoke it.
            if let Some(id) = made["key"]["id"].as_str() {
                let _ = call(svc, &a.source, reqwest::Method::DELETE, &format!("/v1/admin/keys/{id}"), None).await;
            }
            return Err(ApiError { status: 502, message: format!("{}'s Kubernetes API: the Secret {ns}/{secret}: {e}", a.follower) });
        }
        out["key"] = json!({"id": made["key"]["id"], "name": made["key"]["name"]});
    }
    let mut spec = json!({"source": url, "name": name, "keySecret": secret, "start": start, "askedMs": crate::telemetry::now_ms()});
    if let Some(ca) = &a.ca_secret {
        spec["caSecret"] = json!(ca);
    }
    out["changed"] = call(svc, &a.follower, reqwest::Method::POST, "/v1/admin/kube", Some(&json!({"spec": {"follow": spec}}))).await?;
    Ok(out)
}
