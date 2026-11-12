//! The app engine beside the cluster (`spec.apps`): the operator runs
//! chloedb-apps (`ghcr.io/chloetech-ai/chloedb-apps`, an image of its own:
//! Node, Playwright's Chromium and Claude Code) as a Deployment of one
//! (strategy Recreate, `<cluster>-apps`) behind a Service of that name, and
//! the coordinators list it as the app `apps` (`STRATA_APPS`), so that the
//! command center shows **Apps → App creator**.
//!
//! It keeps the apps it builds in the cluster, as rows of its own tables,
//! through the coordinators' Service with the admin token (`tokenSecret`),
//! which is also the token the coordinators send it (`STRATA_APP_TOKEN`).
//! The model it calls takes its key from the cluster's `secretEnv`
//! (`ANTHROPIC_API_KEY`), as the servers' do.
//!
//! The studio, where apps are made and changed, runs code a model wrote,
//! so it is not served through the command center: browsers reach it at
//! its own address (`publicUrl`: a NodePort, a LoadBalancer or an Ingress
//! of one's own in front of the Service), and the command center's **Open
//! the studio** hands an admin a one-time link that signs the browser in.
//! Without the cluster's token anyone who reached it would spend on its
//! model key, so `spec.apps` needs `tokenSecret`.
//!
//! Apps its owners publish are open to everyone (`spec.apps.publish`): a
//! load balancer of their own (`<cluster>-apps-public`, ports 80 and 443)
//! in front of a Caddy beside the engine, which gets each published app's
//! certificate from Let's Encrypt the first time someone opens it (asking
//! the engine first whether the host is one of its apps), and the engine
//! serves each app at `<name>.<domain>`. The domain is one's own, with a
//! wildcard DNS record (`*.apps.example.com`) pointing at that load
//! balancer, or `sslip`: `<its address, dashed>.sslip.io`, which needs no
//! record at all.
//!
//! What it keeps outside the cluster (files apps were given, map tiles,
//! what the model has cost, Claude Code's home) is on a volume of its own,
//! `<cluster>-apps`, which stays when `spec.apps` goes and goes with the
//! cluster.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

/// The name the coordinators list it by (`/v1/apps/apps`).
pub const APP: &str = "apps";
/// The port the engine answers on.
pub const PORT: u16 = 7730;
/// The image it runs when `image` is left out.
pub const IMAGE: &str = "ghcr.io/chloetech-ai/chloedb-apps:latest";
/// The web server in front of published apps (`spec.apps.publish`), which
/// gets their certificates.
pub const CADDY_IMAGE: &str = "caddy:2.8.4-alpine";
/// Caddy's ports in the pod: the load balancer's 80 and 443 (a user that is
/// not root binds no port under 1024).
pub const CADDY_HTTP: u16 = 8080;
pub const CADDY_HTTPS: u16 = 8443;

/// `spec.apps`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppsSpec {
    /// No engine for now: the Deployment keeps no pod (the coordinators
    /// still list it, as an app that does not answer; the apps stay in
    /// their tables and the files on the volume).
    pub paused: bool,
    /// The engine's image ([`IMAGE`] when left out).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Where browsers reach the studio (`https://studio.example.com`,
    /// `http://203.0.113.5:30730`), which **Open the studio** links to.
    /// Without it the command center lists the engine's apps but cannot
    /// open the studio.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_url: Option<String>,
    /// The Service's type: `ClusterIP` (the default: an Ingress of one's
    /// own in front of it), `NodePort` or `LoadBalancer`.
    pub service_type: String,
    /// A NodePort Service's port (30000 to 32767; Kubernetes picks one
    /// when left out).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_port: Option<u16>,
    /// What the model may cost, in US dollars, counted on the volume across
    /// restarts: once it is reached every call is refused until it is
    /// raised. 0: no limit.
    pub spend_cap_usd: f64,
    /// The engine's volume.
    pub storage: AppsStorage,
    /// The engine's own settings (`STRATA_APPS_MODEL`, ...), by name:
    /// `STRATA_APPS_` variables other than those the operator sets.
    /// Secrets go in the cluster's `secretEnv`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<EnvVar>,
    /// Open published apps to the public, at `<name>.<domain>`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publish: Option<AppsPublish>,
    /// More arguments of the engine (`node engine/server.mjs`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Resources of its container.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<Json>,
}

impl Default for AppsSpec {
    fn default() -> Self {
        AppsSpec {
            paused: false,
            image: None,
            public_url: None,
            service_type: "ClusterIP".into(),
            node_port: None,
            spend_cap_usd: 25.0,
            storage: AppsStorage::default(),
            env: Vec::new(),
            publish: None,
            args: Vec::new(),
            resources: None,
        }
    }
}

/// `spec.apps.publish`: where published apps answer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppsPublish {
    /// `apps.example.com` (with a DNS record `*.apps.example.com` pointing
    /// at the load balancer), or `sslip`: the load balancer's address as a
    /// name of sslip.io, which needs no record.
    pub domain: String,
    /// Who Let's Encrypt writes to about the certificates (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

impl AppsPublish {
    /// The domain apps answer under, once the load balancer has `address`
    /// (none yet: `sslip` has no domain until it has one).
    pub fn domain(&self, address: Option<&str>) -> Option<String> {
        if self.domain != "sslip" {
            return Some(self.domain.to_ascii_lowercase());
        }
        let a = address?;
        let ok = a.split('.').count() == 4 && a.split('.').all(|p| p.parse::<u8>().is_ok());
        ok.then(|| format!("{}.sslip.io", a.replace('.', "-")))
    }

    /// Caddy's configuration: a certificate for each host the engine says
    /// is one of its published apps, got on the first visit; http to https.
    pub fn caddyfile(&self) -> String {
        let email = self.email.as_deref().map(|e| format!("\temail {e}\n")).unwrap_or_default();
        format!(
            "{{\n\thttp_port {CADDY_HTTP}\n\thttps_port {CADDY_HTTPS}\n\tadmin off\n{email}\ton_demand_tls {{\n\t\task http://127.0.0.1:{PORT}/api/tls/ask\n\t}}\n}}\n\
             https:// {{\n\ttls {{\n\t\ton_demand\n\t}}\n\treverse_proxy 127.0.0.1:{PORT}\n}}\n\
             http:// {{\n\tredir https://{{host}}{{uri}} permanent\n}}\n"
        )
    }
}

fn is_domain(d: &str) -> bool {
    let labels: Vec<&str> = d.split('.').collect();
    d.len() <= 200
        && labels.len() >= 2
        && labels.iter().all(|l| !l.is_empty() && l.len() <= 63 && !l.starts_with('-') && !l.ends_with('-') && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        && labels.last().is_some_and(|t| t.chars().all(|c| c.is_ascii_alphabetic()))
}

/// `spec.apps.storage`: its volume, made once (it only grows).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppsStorage {
    pub size: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_class_name: Option<String>,
}

impl Default for AppsStorage {
    fn default() -> Self {
        AppsStorage { size: "1Gi".into(), storage_class_name: None }
    }
}

/// A setting of the engine's, `name=value`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnvVar {
    pub name: String,
    #[serde(default)]
    pub value: String,
}

/// The arguments the operator sets, which `args` may not set again.
const SET: &[&str] = &["--strata", "--port", "--host", "--data", "--strata-listen"];
/// The settings the operator sets, which `env` may not set again.
const SET_ENV: &[&str] = &["STRATA_APPS_PUBLIC_URL", "STRATA_APPS_DOMAIN", "STRATA_APPS_DOMAIN_SCHEME", "STRATA_APPS_SPEND_CAP_USD", "STRATA_APPS_DATA", "STRATA_APPS_CLAUDE", "STRATA_APPS_EVOLVE"];

impl AppsSpec {
    /// What is wrong with it, if anything.
    pub fn check(&self) -> Result<(), String> {
        for a in &self.args {
            let flag = a.split('=').next().unwrap_or_default();
            if SET.contains(&flag) {
                return Err(format!("apps.args may not set {flag}: the operator sets it"));
            }
        }
        let mut names = std::collections::HashSet::new();
        for e in &self.env {
            if !e.name.starts_with("STRATA_APPS_") || !e.name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
                return Err(format!("apps.env: {:?} is not one of the engine's settings (STRATA_APPS_...); secrets go in secretEnv", e.name));
            }
            if SET_ENV.contains(&e.name.as_str()) {
                return Err(format!("apps.env may not set {}: the operator sets it", e.name));
            }
            if !names.insert(e.name.as_str()) {
                return Err(format!("apps.env names {} twice", e.name));
            }
        }
        if let Some(u) = &self.public_url {
            let rest = u.strip_prefix("https://").or_else(|| u.strip_prefix("http://"));
            if rest.is_none_or(|r| r.is_empty() || r.contains(['?', '#', ' '])) {
                return Err(format!("apps.publicUrl is an http:// or https:// address, with no query, not {u:?}"));
            }
        }
        if !matches!(self.service_type.as_str(), "ClusterIP" | "NodePort" | "LoadBalancer") {
            return Err(format!("apps.serviceType is ClusterIP, NodePort or LoadBalancer, not {:?}", self.service_type));
        }
        match self.node_port {
            Some(_) if self.service_type != "NodePort" => return Err("apps.nodePort needs serviceType NodePort".into()),
            Some(p) if !(30000..=32767).contains(&p) => return Err(format!("apps.nodePort is 30000 to 32767, not {p}")),
            _ => {}
        }
        if !self.spend_cap_usd.is_finite() || self.spend_cap_usd < 0.0 {
            return Err(format!("apps.spendCapUsd is a number of US dollars, 0 or more, not {}", self.spend_cap_usd));
        }
        if let Some(p) = &self.publish {
            if p.domain != "sslip" && !is_domain(&p.domain) {
                return Err(format!("apps.publish.domain is a domain of your own (apps.example.com) or sslip, not {:?}", p.domain));
            }
            if p.email.as_deref().is_some_and(|e| !e.contains('@') || e.contains([' ', '\n', '{', '}'])) {
                return Err(format!("apps.publish.email is an email address, not {:?}", p.email.as_deref().unwrap_or_default()));
            }
        }
        if super::quantity(&self.storage.size).is_none() {
            return Err(format!("apps.storage.size {:?} is not one Kubernetes reads (1Gi, 500Mi)", self.storage.size));
        }
        Ok(())
    }

    /// The image it runs.
    pub fn image(&self) -> &str {
        self.image.as_deref().unwrap_or(IMAGE)
    }

    /// The engine's arguments for a cluster whose coordinators' Service
    /// answers at `strata`.
    pub fn args(&self, strata: &str) -> Vec<String> {
        let port = PORT.to_string();
        let mut a: Vec<String> = ["--strata", strata, "--host", "0.0.0.0", "--port", &port, "--data", "/data"].iter().map(|s| s.to_string()).collect();
        a.extend(self.args.iter().cloned());
        a
    }

    /// The spending limit as the engine reads it: whole dollars without a
    /// point.
    pub fn spend_cap(&self) -> String {
        let c = self.spend_cap_usd;
        if c.fract() == 0.0 { format!("{c:.0}") } else { c.to_string() }
    }
}

/// `spec.apps` in the resource's schema.
pub fn schema() -> Json {
    json!({
        "type": "object",
        "description": "The app engine beside the cluster: chloedb-apps in a Deployment of one, which the command center shows under Apps. It keeps the apps it builds as rows of the cluster, calls the model with secretEnv's ANTHROPIC_API_KEY, and needs tokenSecret: its studio, at publicUrl, signs admins in with a link the command center hands out.",
        "properties": {
            "paused": {"type": "boolean", "description": "No engine pod for now (the apps stay)."},
            "image": {"type": "string", "description": format!("The engine's image (default {IMAGE}).")},
            "publicUrl": {"type": "string", "pattern": "^https?://[^?# ]+$", "description": "Where browsers reach the studio, which Open the studio links to: a NodePort, a LoadBalancer or an Ingress of your own in front of the Service."},
            "serviceType": {"type": "string", "enum": ["ClusterIP", "NodePort", "LoadBalancer"], "description": "The Service's type (default ClusterIP)."},
            "nodePort": {"type": "integer", "minimum": 30000, "maximum": 32767, "description": "A NodePort Service's port (Kubernetes picks one when left out)."},
            "spendCapUsd": {"type": "number", "minimum": 0, "description": "What the model may cost, in US dollars, before every call is refused until it is raised (default 25; 0: no limit)."},
            "storage": {"type": "object", "description": "Its volume (files, map tiles, the spend count): made once, kept when spec.apps goes, deleted with the cluster.", "properties": {
                "size": {"type": "string", "description": "Its size (default 1Gi); it only grows."},
                "storageClassName": {"type": "string"},
            }},
            "env": {"type": "array", "description": "The engine's own settings (STRATA_APPS_MODEL, ...); secrets go in secretEnv.", "items": {"type": "object", "required": ["name"], "properties": {
                "name": {"type": "string", "pattern": "^STRATA_APPS_[A-Z0-9_]+$"},
                "value": {"type": "string"},
            }}},
            "args": {"type": "array", "items": {"type": "string"}, "description": "More arguments of the engine."},
            "publish": {"type": "object", "required": ["domain"], "description": "Open the apps their owners publish to everyone, at <name>.<domain>, with certificates from Let's Encrypt, behind a load balancer of their own (<cluster>-apps-public).", "properties": {
                "domain": {"type": "string", "description": "apps.example.com, with a DNS record *.apps.example.com pointing at the load balancer; or sslip: its address as a name of sslip.io, with no record."},
                "email": {"type": "string", "description": "Who Let's Encrypt writes to about the certificates."},
            }},
            "resources": {"type": "object", "x-kubernetes-preserve-unknown-fields": true, "description": "Resources of its container."},
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_apps_spec_is_checked() {
        let s: AppsSpec = serde_json::from_value(json!({})).unwrap();
        assert_eq!(s, AppsSpec::default());
        assert_eq!(s.check(), Ok(()));
        assert_eq!(s.image(), IMAGE);
        assert_eq!(s.spend_cap(), "25");
        assert_eq!(s.args("http://shop:7700"), ["--strata", "http://shop:7700", "--host", "0.0.0.0", "--port", "7730", "--data", "/data"]);
        let s: AppsSpec = serde_json::from_value(json!({"spendCapUsd": 2.5, "args": ["--example", "1"], "serviceType": "NodePort", "nodePort": 30731, "publicUrl": "http://203.0.113.5:30731", "env": [{"name": "STRATA_APPS_MODEL", "value": "m"}]})).unwrap();
        assert_eq!(s.check(), Ok(()));
        assert_eq!(s.spend_cap(), "2.5");
        assert_eq!(s.args("t")[8..], ["--example", "1"].map(String::from));
        let p: AppsSpec = serde_json::from_value(json!({"publish": {"domain": "sslip", "email": "ops@example.com"}})).unwrap();
        assert_eq!(p.check(), Ok(()));
        let p = p.publish.unwrap();
        assert_eq!(p.domain(None), None, "sslip waits for the load balancer's address");
        assert_eq!(p.domain(Some("203.0.113.5")).as_deref(), Some("203-0-113-5.sslip.io"));
        assert_eq!(p.domain(Some("lb.example.net")), None);
        let own = AppsPublish { domain: "Apps.Example.com".into(), email: None };
        assert_eq!(own.domain(None).as_deref(), Some("apps.example.com"));
        let c = p.caddyfile();
        assert!(c.contains("ask http://127.0.0.1:7730/api/tls/ask") && c.contains("on_demand") && c.contains("email ops@example.com") && c.contains("http_port 8080"), "{c}");
        assert!(!own.caddyfile().contains("email"));
        for (bad, says) in [
            (json!({"args": ["--strata=http://other"]}), "may not set --strata"),
            (json!({"args": ["--data", "/d"]}), "may not set --data"),
            (json!({"env": [{"name": "ANTHROPIC_API_KEY", "value": "x"}]}), "secretEnv"),
            (json!({"env": [{"name": "STRATA_APPS_SPEND_CAP_USD", "value": "0"}]}), "the operator sets it"),
            (json!({"env": [{"name": "STRATA_APPS_MODEL"}, {"name": "STRATA_APPS_MODEL"}]}), "twice"),
            (json!({"publicUrl": "studio.example.com"}), "publicUrl"),
            (json!({"publicUrl": "https://studio.example.com/?x=1"}), "publicUrl"),
            (json!({"serviceType": "Ingress"}), "serviceType"),
            (json!({"nodePort": 30731}), "needs serviceType NodePort"),
            (json!({"serviceType": "NodePort", "nodePort": 8080}), "30000 to 32767"),
            (json!({"spendCapUsd": -1}), "spendCapUsd"),
            (json!({"storage": {"size": "lots"}}), "storage.size"),
            (json!({"publish": {"domain": "localhost"}}), "publish.domain"),
            (json!({"publish": {"domain": "*.apps.example.com"}}), "publish.domain"),
            (json!({"publish": {"domain": "apps.example.com", "email": "me"}}), "publish.email"),
            (json!({"env": [{"name": "STRATA_APPS_DOMAIN", "value": "x.io"}]}), "the operator sets it"),
        ] {
            let s: AppsSpec = serde_json::from_value(bad.clone()).unwrap();
            let e = s.check().unwrap_err();
            assert!(e.contains(says), "{bad}: {e}");
        }
    }
}
