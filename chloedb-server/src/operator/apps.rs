//! The app engine beside the cluster (`spec.apps`): the operator runs
//! chloedb-apps (`ghcr.io/chloetech-ai/chloedb-apps`, an image of its own:
//! Node, Playwright's Chromium and Claude Code) as a Deployment of one
//! (strategy Recreate, `<cluster>-apps`) behind a Service of that name, and
//! the coordinators list it as the app `apps` (`STRATA_APPS`), so that the
//! command center shows **Apps → App engine**.
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
            args: Vec::new(),
            resources: None,
        }
    }
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
const SET_ENV: &[&str] = &["STRATA_APPS_PUBLIC_URL", "STRATA_APPS_SPEND_CAP_USD", "STRATA_APPS_DATA", "STRATA_APPS_CLAUDE", "STRATA_APPS_EVOLVE"];

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
        ] {
            let s: AppsSpec = serde_json::from_value(bad.clone()).unwrap();
            let e = s.check().unwrap_err();
            assert!(e.contains(says), "{bad}: {e}");
        }
    }
}
