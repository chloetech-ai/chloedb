//! Following another cluster (`spec.follow`, [`crate::follow`]): the
//! operator runs `strata-server follow` as a Deployment of one (strategy
//! Recreate, `<cluster>-follow`), which reads the source's feed with a
//! `feed` key of the source (`keySecret`) and writes through this
//! cluster's coordinators with its token. Its place is kept in the
//! cluster's own `_sources`, so the process holds no state: a pod started
//! again resumes.
//!
//! The process is told when following was asked for (`askedMs`,
//! `strata-server follow --once`): a copy afresh (`start: copy`) is made
//! once, not again when its pod restarts, and once the cluster stopped
//! following the source or was promoted from it after that time, a pod
//! started again waits instead of following again, until the spec goes
//! (the coordinator removes it on a stop or a promotion,
//! [`crate::http`]). `paused` keeps the Deployment at no pod: nothing is
//! applied, and the cluster still refuses its clients' writes.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

/// `spec.follow`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FollowSpec {
    /// The source cluster's URL: its coordinators, as
    /// `https://east.example:7700`.
    pub source: String,
    /// The name this cluster knows the source by, which its marks and
    /// refusals name ("this cluster follows east").
    pub name: String,
    /// A Secret whose key `key` is a key of the source of the `feed` scope.
    pub key_secret: Option<String>,
    /// A Secret whose key `ca.crt` is the CA certificate (PEM) the
    /// source's TLS certificate is checked against, for a self-signed one.
    pub ca_secret: Option<String>,
    /// How following starts ([`Start`]).
    pub start: Start,
    /// When following was asked for, in ms since the epoch: a stop or a
    /// promotion after it is final for this spec.
    pub asked_ms: u64,
    /// No follow process for now: the Deployment keeps no pod.
    pub paused: bool,
    /// Resources of the follow process's container.
    pub resources: Option<Json>,
}

/// How a follow process starts (`spec.follow.start`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Start {
    /// From where the cluster is: a copy the first time, then the feed.
    #[default]
    Follow,
    /// A copy afresh, the source's rows over this cluster's and the keys
    /// it lacks deleted: an old source rejoining after a promotion that
    /// was not planned (`follow --copy`).
    Copy,
    /// Back from the source's promotion from this cluster, without a copy
    /// (`follow --reverse`).
    Reverse,
}

impl FollowSpec {
    /// What is wrong with it, if anything.
    pub fn check(&self) -> Result<(), String> {
        if !crate::service::follow_name_ok(&self.name) {
            return Err(format!("follow.name is 1 to 64 letters, digits, '.', '_' or '-', not {:?}", self.name));
        }
        let url = reqwest::Url::parse(&self.source).map_err(|e| format!("follow.source is the source cluster's URL (https://host:7700): {e}"))?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(format!("follow.source is an http or https URL with a host, not {:?}", self.source));
        }
        for (field, s) in [("keySecret", &self.key_secret), ("caSecret", &self.ca_secret)] {
            if s.as_deref().is_some_and(str::is_empty) {
                return Err(format!("follow.{field} names a Secret, or is left out"));
            }
        }
        Ok(())
    }

    /// The arguments of `strata-server follow` for a cluster whose
    /// coordinators' Service answers at `target`.
    pub fn args(&self, target: &str) -> Vec<String> {
        let mut a: Vec<String> = ["follow", "--source", &self.source, "--target", target, "--name", &self.name].iter().map(|s| s.to_string()).collect();
        a.extend(["--once".to_string(), self.asked_ms.to_string()]);
        match self.start {
            Start::Follow => {}
            Start::Copy => a.push("--copy".into()),
            Start::Reverse => a.push("--reverse".into()),
        }
        if self.ca_secret.is_some() {
            a.extend(["--source-ca".to_string(), format!("{CA_DIR}/ca.crt")]);
        }
        a
    }
}

/// Where a follow process finds its `caSecret`.
pub const CA_DIR: &str = "/etc/strata-follow";

/// `spec.follow` in the resource's schema.
pub fn schema() -> Json {
    json!({
        "type": "object",
        "description": "Following another cluster: a strata-server follow Deployment of one copies the source, then applies its feed; the cluster refuses its clients' writes meanwhile.",
        "required": ["source", "name"],
        "properties": {
            "source": {"type": "string", "description": "The source cluster's URL (its coordinators), as https://east.example:7700."},
            "name": {"type": "string", "pattern": "^[A-Za-z0-9._-]{1,64}$", "description": "The name this cluster knows the source by."},
            "keySecret": {"type": "string", "description": "A Secret whose key key is a key of the source of the feed scope."},
            "caSecret": {"type": "string", "description": "A Secret whose key ca.crt is the CA certificate (PEM) of the source's TLS certificate, for a self-signed one."},
            "start": {"type": "string", "enum": ["follow", "copy", "reverse"], "description": "follow (default): from where the cluster is, a copy the first time; copy: a copy afresh (an old source rejoining); reverse: back from the source's promotion from this cluster, without a copy."},
            "askedMs": {"type": "integer", "minimum": 0, "description": "When following was asked for (ms since the epoch): a copy is made once, and a stop or promotion after it is final for this spec."},
            "paused": {"type": "boolean", "description": "No follow process for now; the cluster still refuses its clients' writes."},
            "resources": {"type": "object", "x-kubernetes-preserve-unknown-fields": true, "description": "Resources of the follow process's container."},
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_follow_spec_is_checked_and_runs_its_start_once() {
        let f: FollowSpec = serde_json::from_value(json!({"source": "https://east.example:7700", "name": "east", "keySecret": "east-feed", "askedMs": 1700000000000u64, "start": "copy"})).unwrap();
        assert_eq!(f.check(), Ok(()));
        assert_eq!(
            f.args("http://west:7700"),
            ["follow", "--source", "https://east.example:7700", "--target", "http://west:7700", "--name", "east", "--once", "1700000000000", "--copy"]
        );
        let ca = FollowSpec { ca_secret: Some("east-ca".into()), start: Start::Reverse, ..f.clone() };
        assert!(ca.args("t").ends_with(&["--reverse".to_string(), "--source-ca".into(), "/etc/strata-follow/ca.crt".into()]));
        for (bad, says) in [
            (json!({"source": "https://e:7700", "name": "east coast"}), "follow.name"),
            (json!({"source": "east:7700", "name": "east"}), "follow.source"),
            (json!({"source": "ftp://east", "name": "east"}), "http or https"),
            (json!({"source": "https://e", "name": "east", "keySecret": ""}), "keySecret"),
        ] {
            let f: FollowSpec = serde_json::from_value(bad).unwrap();
            assert!(f.check().unwrap_err().contains(says), "{f:?}");
        }
        assert!(serde_json::from_value::<FollowSpec>(json!({"source": "https://e", "name": "e", "start": "later"})).is_err());
    }
}
