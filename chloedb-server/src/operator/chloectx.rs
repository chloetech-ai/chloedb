//! ChloeCtx beside the cluster (`spec.chloectx`): the operator runs
//! `chloectx app` (a program of its own, in the same image) as a
//! Deployment of one (strategy Recreate, `<cluster>-chloectx`) behind a
//! Service of that name, and the coordinators list it as the app
//! `chloectx` (`STRATA_APPS`), so that the command center shows
//! **Apps → ChloeCtx**: memory spaces agents write to and ask for context
//! (Context), and the cluster's firing alerts beside the past incidents
//! they look like (Seen before).
//!
//! As the crawler and the spreadsheets do, it reads and writes through the
//! coordinators' Service with the cluster's admin token (`tokenSecret`),
//! which is also the token its own API requires and the one the
//! coordinators send it (`STRATA_APP_TOKEN`). Its spaces are tables of the
//! cluster (`ctx_spaces`, `ctx_<space>`), so a pod started again, anywhere,
//! finds them there; one pod at a time writes to them (it numbers the
//! records it writes). It calls a model with the cluster's `secretEnv`
//! (`ANTHROPIC_API_KEY`), and works without one but for writing a
//! conversation's notes down and drafting incidents.
//!
//! Its embedding model (`static`, 125 MB) is not in the image, which every
//! server pulls: an init container downloads it from Hugging Face into the
//! pod's own scratch volume at each start. `embedder: hash:<dim>` needs no
//! download (vectors from hashed words: for a cluster that cannot reach
//! Hugging Face).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

/// The name the coordinators list it by (`/v1/apps/chloectx`).
pub const APP: &str = "chloectx";
/// The port its API answers on.
pub const PORT: u16 = 7760;
/// Where the init container puts the model, and the app reads it.
pub const MODELS: &str = "/models";

/// `spec.chloectx`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChloectxSpec {
    /// No ChloeCtx pod for now: the Deployment keeps no pod (the
    /// coordinators still list it, as an app that does not answer; the
    /// spaces stay in their tables).
    pub paused: bool,
    /// The embedder: `static` (the default; its model downloaded at each
    /// start) or `hash:<dim>` (no model). A space keeps the vectors it was
    /// written with, so changing it makes earlier notes match by words only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedder: Option<String>,
    /// More arguments of `chloectx app`.
    pub args: Vec<String>,
    /// Resources of its container.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<Json>,
}

/// The arguments the operator sets, which `args` may not set again.
const SET: &[&str] = &["--server", "--key", "--token", "--host", "--port", "--embedder"];

impl ChloectxSpec {
    /// What is wrong with it, if anything.
    pub fn check(&self) -> Result<(), String> {
        for a in &self.args {
            let flag = a.split('=').next().unwrap_or_default();
            if SET.contains(&flag) {
                return Err(format!("chloectx.args may not set {flag}: the operator sets it{}", if flag == "--embedder" { " (from chloectx.embedder)" } else { "" }));
            }
        }
        let e = self.embedder();
        let ok = e == "static" || e.strip_prefix("hash:").is_some_and(|d| d.parse::<u32>().is_ok_and(|d| (8..=4096).contains(&d)));
        if !ok {
            return Err(format!("chloectx.embedder is static or hash:<dim> (8 to 4096), not {e:?}"));
        }
        Ok(())
    }

    /// The embedder it runs with.
    pub fn embedder(&self) -> &str {
        self.embedder.as_deref().unwrap_or("static")
    }

    /// Whether its pod downloads the model first.
    pub fn needs_model(&self) -> bool {
        self.embedder() == "static"
    }

    /// The arguments of `chloectx` for a cluster whose coordinators'
    /// Service answers at `server`.
    pub fn args(&self, server: &str) -> Vec<String> {
        let port = PORT.to_string();
        let mut a: Vec<String> = ["app", "--server", server, "--host", "0.0.0.0", "--port", &port, "--embedder", self.embedder()].iter().map(|s| s.to_string()).collect();
        a.extend(self.args.iter().cloned());
        a
    }
}

/// `spec.chloectx` in the resource's schema.
pub fn schema() -> Json {
    json!({
        "type": "object",
        "description": "ChloeCtx beside the cluster: a chloectx app Deployment of one, which the command center shows under Apps (Context: memory spaces agents write to and ask for context; Seen before: firing alerts beside the past incidents they look like). Its spaces are tables of the cluster.",
        "properties": {
            "paused": {"type": "boolean", "description": "No ChloeCtx pod for now (the spaces stay)."},
            "embedder": {"type": "string", "description": "static (default: its 125 MB model is downloaded from Hugging Face at each start) or hash:<dim> (no model, vectors from hashed words)."},
            "args": {"type": "array", "items": {"type": "string"}, "description": "More arguments of chloectx app."},
            "resources": {"type": "object", "x-kubernetes-preserve-unknown-fields": true, "description": "Resources of its container."},
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chloectx_spec_is_checked() {
        let s: ChloectxSpec = serde_json::from_value(json!({})).unwrap();
        assert_eq!(s, ChloectxSpec::default());
        assert_eq!(s.check(), Ok(()));
        assert!(s.needs_model());
        assert_eq!(s.args("http://shop:7700"), ["app", "--server", "http://shop:7700", "--host", "0.0.0.0", "--port", "7760", "--embedder", "static"]);
        let s: ChloectxSpec = serde_json::from_value(json!({"embedder": "hash:256", "args": ["--verbose"]})).unwrap();
        assert_eq!(s.check(), Ok(()));
        assert!(!s.needs_model());
        assert_eq!(s.args("t")[8..], ["hash:256", "--verbose"].map(String::from));
        for bad in [json!({"args": ["--token", "x"]}), json!({"args": ["--server=http://other"]}), json!({"args": ["--embedder", "bert"]})] {
            let s: ChloectxSpec = serde_json::from_value(bad).unwrap();
            assert!(s.check().unwrap_err().contains("may not set"), "{s:?}");
        }
        for bad in ["bert", "hash", "hash:x", "hash:2", "static:/models"] {
            let s = ChloectxSpec { embedder: Some(bad.into()), ..Default::default() };
            assert!(s.check().unwrap_err().contains("static or hash"), "{bad}");
        }
    }
}
