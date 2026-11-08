//! A crawler beside the cluster (`spec.crawler`): the operator runs
//! `strata-crawl serve` (a program of its own, in the same image) as a
//! Deployment of one (strategy Recreate, `<cluster>-crawler`) behind a
//! Service of that name, and the coordinators list it as the app `crawler`
//! (`STRATA_APPS`), so that the command center shows **Apps → Crawler**.
//!
//! It writes through the coordinators' Service with the cluster's admin
//! token (`tokenSecret`). The same token is the one its own API requires
//! and the one the coordinators send it (`STRATA_APP_TOKEN`), so it holds
//! no other secret, and only what holds the admin token reaches it. With
//! `grow` it adds a shard to the cluster through the coordinators (one more
//! in this resource's `spec.shards`, `--grow kube`) when a shard holds more
//! than `splitRows` rows of a crawl's table, up to `maxShards`.
//!
//! Its crawls live in its process: a pod started again starts with none,
//! and the tables they filled stay.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

/// The name the coordinators list it by (`/v1/apps/crawler`).
pub const APP: &str = "crawler";
/// The port its API answers on.
pub const PORT: u16 = 7800;

/// `spec.crawler`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CrawlerSpec {
    /// Add shards to the cluster as crawls fill it (default true); false:
    /// never (`--grow off`).
    pub grow: bool,
    /// Rows of a crawl's table on a shard past which the cluster gets one
    /// more shard (`--split-rows`; the crawler's default when left out).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split_rows: Option<u64>,
    /// The most shards growing makes (`--max-shards`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_shards: Option<usize>,
    /// No crawler for now: the Deployment keeps no pod (the coordinators
    /// still list it, as an app that does not answer).
    pub paused: bool,
    /// More arguments of `strata-crawl serve`.
    pub args: Vec<String>,
    /// Resources of the crawler's container.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<Json>,
}

impl Default for CrawlerSpec {
    fn default() -> Self {
        CrawlerSpec { grow: true, split_rows: None, max_shards: None, paused: false, args: Vec::new(), resources: None }
    }
}

/// The arguments the operator sets, which `args` may not set again.
const SET: &[&str] = &["--listen", "--strata", "--key", "--token", "--grow", "--split-rows", "--max-shards", "--local", "--local-listen", "--server-bin", "--port-base"];

impl CrawlerSpec {
    /// What is wrong with it, if anything.
    pub fn check(&self) -> Result<(), String> {
        if self.max_shards == Some(0) {
            return Err("crawler.maxShards is 1 or more".into());
        }
        for a in &self.args {
            let flag = a.split('=').next().unwrap_or_default();
            if SET.contains(&flag) {
                return Err(format!("crawler.args may not set {flag}: the operator sets it (grow, splitRows and maxShards are fields of their own)"));
            }
        }
        Ok(())
    }

    /// The arguments of `strata-crawl` for a cluster whose coordinators'
    /// Service answers at `strata`.
    pub fn args(&self, strata: &str) -> Vec<String> {
        let mut a: Vec<String> = ["serve", "--listen", &format!("0.0.0.0:{PORT}"), "--strata", strata, "--grow", if self.grow { "kube" } else { "off" }].iter().map(|s| s.to_string()).collect();
        if let Some(n) = self.split_rows {
            a.extend(["--split-rows".to_string(), n.to_string()]);
        }
        if let Some(n) = self.max_shards {
            a.extend(["--max-shards".to_string(), n.to_string()]);
        }
        a.extend(self.args.iter().cloned());
        a
    }
}

/// `spec.crawler` in the resource's schema.
pub fn schema() -> Json {
    json!({
        "type": "object",
        "description": "A crawler beside the cluster: a strata-crawl Deployment of one, which the command center shows under Apps; it crawls sites into tables and adds shards as they fill.",
        "properties": {
            "grow": {"type": "boolean", "description": "Add a shard when a crawl's table fills one (default true)."},
            "splitRows": {"type": "integer", "minimum": 0, "description": "Rows of a crawl's table on a shard past which the cluster gets one more shard (0: never; default 100000)."},
            "maxShards": {"type": "integer", "minimum": 1, "description": "The most shards growing makes (default 8)."},
            "paused": {"type": "boolean", "description": "No crawler pod for now."},
            "args": {"type": "array", "items": {"type": "string"}, "description": "More arguments of strata-crawl serve."},
            "resources": {"type": "object", "x-kubernetes-preserve-unknown-fields": true, "description": "Resources of the crawler's container."},
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crawler_spec_is_checked_and_grows_through_the_operator() {
        let c: CrawlerSpec = serde_json::from_value(json!({})).unwrap();
        assert_eq!(c, CrawlerSpec::default());
        assert_eq!(c.check(), Ok(()));
        assert_eq!(c.args("http://shop:7700"), ["serve", "--listen", "0.0.0.0:7800", "--strata", "http://shop:7700", "--grow", "kube"]);
        let c: CrawlerSpec = serde_json::from_value(json!({"grow": false, "splitRows": 5000, "maxShards": 3, "args": ["--concurrency", "8"]})).unwrap();
        assert_eq!(c.check(), Ok(()));
        assert_eq!(c.args("t")[5..], ["--grow", "off", "--split-rows", "5000", "--max-shards", "3", "--concurrency", "8"].map(String::from));
        for (bad, says) in [(json!({"args": ["--token", "x"]}), "--token"), (json!({"args": ["--grow=off"]}), "--grow"), (json!({"maxShards": 0}), "maxShards")] {
            let c: CrawlerSpec = serde_json::from_value(bad).unwrap();
            assert!(c.check().unwrap_err().contains(says), "{c:?}");
        }
    }
}
