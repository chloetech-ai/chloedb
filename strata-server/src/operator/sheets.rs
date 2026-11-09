//! Spreadsheets beside the cluster (`spec.sheets`): the operator runs
//! `strata-sheets serve` (a program of its own, in the same image) as a
//! Deployment of one (strategy Recreate, `<cluster>-sheets`) behind a
//! Service of that name, and the coordinators list it as the app `sheets`
//! (`STRATA_APPS`), so that the command center shows **Apps → Sheets**.
//!
//! As the crawler does ([`super::crawler`]), it reads and writes through
//! the coordinators' Service with the cluster's admin token (`tokenSecret`),
//! which is also the token its own API requires and the one the
//! coordinators send it (`STRATA_APP_TOKEN`). Its workbooks live in two
//! tables of the cluster (`sheets_books`, `sheets_cells`), which it makes
//! at its first start: a pod started again, anywhere, finds them there.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

/// The name the coordinators list it by (`/v1/apps/sheets`).
pub const APP: &str = "sheets";
/// The port its API answers on.
pub const PORT: u16 = 7820;

/// `spec.sheets`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SheetsSpec {
    /// No spreadsheets pod for now: the Deployment keeps no pod (the
    /// coordinators still list it, as an app that does not answer; the
    /// workbooks stay in their tables).
    pub paused: bool,
    /// More arguments of `strata-sheets serve`.
    pub args: Vec<String>,
    /// Resources of its container.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<Json>,
}

/// The arguments the operator sets, which `args` may not set again.
const SET: &[&str] = &["--listen", "--strata", "--key", "--token", "--local", "--local-listen", "--server-bin"];

impl SheetsSpec {
    /// What is wrong with it, if anything.
    pub fn check(&self) -> Result<(), String> {
        for a in &self.args {
            let flag = a.split('=').next().unwrap_or_default();
            if SET.contains(&flag) {
                return Err(format!("sheets.args may not set {flag}: the operator sets it"));
            }
        }
        Ok(())
    }

    /// The arguments of `strata-sheets` for a cluster whose coordinators'
    /// Service answers at `strata`.
    pub fn args(&self, strata: &str) -> Vec<String> {
        let mut a: Vec<String> = ["serve", "--listen", &format!("0.0.0.0:{PORT}"), "--strata", strata].iter().map(|s| s.to_string()).collect();
        a.extend(self.args.iter().cloned());
        a
    }
}

/// `spec.sheets` in the resource's schema.
pub fn schema() -> Json {
    json!({
        "type": "object",
        "description": "Spreadsheets beside the cluster: a strata-sheets Deployment of one, which the command center shows under Apps; its workbooks are rows of two tables of the cluster, and the cluster's tables open in it as sheets.",
        "properties": {
            "paused": {"type": "boolean", "description": "No spreadsheets pod for now (the workbooks stay)."},
            "args": {"type": "array", "items": {"type": "string"}, "description": "More arguments of strata-sheets serve."},
            "resources": {"type": "object", "x-kubernetes-preserve-unknown-fields": true, "description": "Resources of its container."},
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sheets_spec_is_checked() {
        let s: SheetsSpec = serde_json::from_value(json!({})).unwrap();
        assert_eq!(s, SheetsSpec::default());
        assert_eq!(s.check(), Ok(()));
        assert_eq!(s.args("http://shop:7700"), ["serve", "--listen", "0.0.0.0:7820", "--strata", "http://shop:7700"]);
        let s: SheetsSpec = serde_json::from_value(json!({"args": ["--name", "x"]})).unwrap();
        assert_eq!(s.args("t")[5..], ["--name", "x"].map(String::from));
        for bad in [json!({"args": ["--token", "x"]}), json!({"args": ["--strata=http://other"]}), json!({"args": ["--local", "/d"]})] {
            let s: SheetsSpec = serde_json::from_value(bad).unwrap();
            assert!(s.check().unwrap_err().contains("may not set"), "{s:?}");
        }
    }
}
