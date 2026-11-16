//! Network operations beside the cluster (`spec.netops`): the operator runs
//! `chloedb-netops serve` (a program of its own, in the same image) as a
//! Deployment of one (strategy Recreate, `<cluster>-netops`) behind a
//! Service of that name, and the coordinators list it as the app `netops`
//! (`STRATA_APPS`), so that the command center shows **Apps → Network
//! operations**.
//!
//! As the spreadsheets do ([`super::sheets`]), it reads and writes through
//! the coordinators' Service. Its network (the inventory, the telemetry
//! its simulator writes each minute, the incidents and what its agents
//! found) lives in the cluster's tables `netops_*`, which it makes at its
//! first start: a pod started again, anywhere, goes on from them. It holds
//! a key limited to those tables and a token of its own
//! ([`super::credentials`]), or the cluster's admin token (`tokenSecret`)
//! until the coordinators make such keys.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

/// The name the coordinators list it by (`/v1/apps/netops`).
pub const APP: &str = "netops";
/// The port its API answers on.
pub const PORT: u16 = 7840;

/// `spec.netops`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NetopsSpec {
    /// No pod for now: the Deployment keeps none (the coordinators still
    /// list it, as an app that does not answer; the tables stay, and the
    /// simulator writes nothing to them).
    pub paused: bool,
    /// The tables its key may touch, as `table=scope` (`crawl_*=admin`,
    /// `*=read`; [`crate::keys::Grant`]); none: the app's own
    /// ([`super::credentials::default_tables`]).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tables: Vec<String>,
    /// More arguments of `chloedb-netops serve` (`--fault-every 0`,
    /// `--history-hours 2`, `--no-simulate`).
    pub args: Vec<String>,
    /// Resources of its container.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<Json>,
}

/// The arguments the operator sets, which `args` may not set again.
const SET: &[&str] = &["--listen", "--strata", "--key", "--token", "--local", "--local-listen", "--server-bin"];

impl NetopsSpec {
    /// What is wrong with it, if anything.
    pub fn check(&self) -> Result<(), String> {
        super::credentials::check_tables(APP, &self.tables)?;
        for a in &self.args {
            let flag = a.split('=').next().unwrap_or_default();
            if SET.contains(&flag) {
                return Err(format!("netops.args may not set {flag}: the operator sets it"));
            }
        }
        Ok(())
    }

    /// The arguments of `chloedb-netops` for a cluster whose coordinators'
    /// Service answers at `strata`.
    pub fn args(&self, strata: &str) -> Vec<String> {
        let mut a: Vec<String> = ["serve", "--listen", &format!("0.0.0.0:{PORT}"), "--strata", strata].iter().map(|s| s.to_string()).collect();
        a.extend(self.args.iter().cloned());
        a
    }
}

/// `spec.netops` in the resource's schema.
pub fn schema() -> Json {
    json!({
        "type": "object",
        "description": "Network operations beside the cluster: a chloedb-netops Deployment of one, which the command center shows under Apps; its network, telemetry, incidents and agents' runs are rows of the cluster's tables netops_*.",
        "properties": {
            "paused": {"type": "boolean", "description": "No pod for now (the tables stay; nothing is written to them)."},
            "tables": super::credentials::schema(APP),
            "args": {"type": "array", "items": {"type": "string"}, "description": "More arguments of chloedb-netops serve (--fault-every 0, --history-hours 2, --no-simulate)."},
            "resources": {"type": "object", "x-kubernetes-preserve-unknown-fields": true, "description": "Resources of its container."},
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_netops_spec_is_checked() {
        let s: NetopsSpec = serde_json::from_value(json!({})).unwrap();
        assert_eq!(s, NetopsSpec::default());
        assert_eq!(s.check(), Ok(()));
        assert_eq!(s.args("http://shop:7700"), ["serve", "--listen", "0.0.0.0:7840", "--strata", "http://shop:7700"]);
        let s: NetopsSpec = serde_json::from_value(json!({"args": ["--fault-every", "0"]})).unwrap();
        assert_eq!(s.args("t")[5..], ["--fault-every", "0"].map(String::from));
        for bad in [json!({"args": ["--token", "x"]}), json!({"args": ["--strata=http://other"]}), json!({"args": ["--local", "/d"]})] {
            let s: NetopsSpec = serde_json::from_value(bad).unwrap();
            assert!(s.check().unwrap_err().contains("may not set"), "{s:?}");
        }
    }
}
