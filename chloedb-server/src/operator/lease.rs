//! One coordinator at a time changes a cluster's map, on Kubernetes
//! (finding E-2, until the map has an agreement of its own).
//!
//! A coordinator remembers a map it gave and got no answer for (its map
//! in doubt, [`crate::cores::slots::Change::in_doubt`]), and gives it again
//! before it makes another: otherwise two servers can hold different maps
//! of one epoch, or a change the servers acted on is undone. That memory
//! was the process's: the operator's next step, which the Service sends to
//! any coordinator, landed on one that never saw the map in doubt, and a
//! coordinator started again (every upgrade starts them again) forgot it.
//! The simulator measures the fork it leads to (5 of 2,000 seeds with two
//! coordinators changing the map, 4 of 40,000 with one that forgets).
//!
//! So a coordinator run by the operator changes the map only while it
//! holds a Lease (`coordination.k8s.io/v1`, `<cluster>-map` in the
//! cluster's namespace), taken and kept by the API server's
//! compare-and-swap on the object's `resourceVersion`, and it keeps in the
//! Lease the map it is about to give, before it gives it, and the map left
//! in doubt once the change ends. The next coordinator to change the map,
//! this one started again or another, takes the Lease with that map and
//! gives it again first. A coordinator that cannot keep the Lease (the API
//! server does not answer, or another took it once it expired) gives no
//! map.
//!
//! A coordinator whose account may not use Leases (its Role was made by an
//! operator from before them, which makes it again once it is upgraded)
//! changes the map as before, without one, and says so.
//!
//! What it does not do: it is not an agreement among the coordinators. A
//! coordinator paused past the Lease's term while giving a map can still
//! give it after another took the Lease (the term, [`TERM`], is longer
//! than a change's longest call), and coordinators that are not run by the
//! operator do not take one: run their changes through one of them.

use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value as Json};

use super::kube::Kube;
use crate::slots::SlotMap;
use crate::ApiError;

/// How long a Lease lasts after it was last kept: longer than a change's
/// longest call (60 s, giving a map to the servers that do not take it
/// first), as a coordinator keeps it at each call.
pub const TERM: Duration = Duration::from_secs(90);

/// How long a coordinator waits for another to end its change before it
/// refuses its own (503).
pub const WAIT: Duration = Duration::from_secs(20);

/// The annotations the Lease keeps: when it was last kept, in ms (its own
/// `renewTime` is a time of day the API server checks the format of), and
/// the map in doubt.
const KEPT_MS: &str = "strata.chloetech.ai/kept-ms";
const DOUBT: &str = "strata.chloetech.ai/map-in-doubt";

/// The Lease of a cluster's map, as one coordinator (`me`) takes it.
pub struct MapLease {
    kube: Kube,
    path: String,
    collection: String,
    name: String,
    me: String,
    /// The `resourceVersion` it last wrote, while it holds the Lease.
    held: Mutex<Option<String>>,
    /// Its account may not use Leases (403): it changes the map without.
    forbidden: std::sync::atomic::AtomicBool,
}

/// Who holds the Lease, when a coordinator cannot take it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldBy {
    pub holder: String,
    pub left_ms: u64,
}

impl MapLease {
    /// The Lease of StrataCluster `name` in namespace `ns`, taken as `me`
    /// (the pod's name: a coordinator started again in its pod takes its
    /// own Lease back at once).
    pub fn new(kube: Kube, ns: &str, name: &str, me: &str) -> MapLease {
        let collection = format!("/apis/coordination.k8s.io/v1/namespaces/{ns}/leases");
        let name = format!("{name}-map");
        MapLease { path: format!("{collection}/{name}"), collection, name, kube, me: me.to_string(), held: Mutex::new(None), forbidden: Default::default() }
    }

    pub fn me(&self) -> &str {
        &self.me
    }

    fn held(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.held.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Takes the Lease, waiting up to `wait` while another coordinator
    /// holds it: the map in doubt it keeps, if any. Refused (503) when
    /// another still holds it then, or the API server does not answer.
    pub async fn take(&self, wait: Duration, now_ms: &(dyn Fn() -> u64 + Sync)) -> Result<Option<SlotMap>, ApiError> {
        let until = now_ms() + wait.as_millis() as u64;
        loop {
            match self.try_take(now_ms()).await {
                Ok(Ok(doubt)) => return Ok(doubt),
                Ok(Err(by)) if now_ms() >= until => {
                    return Err(unavailable(format!(
                        "coordinator {} is changing the cluster map (it holds the Lease {} for {} s more): one coordinator changes it at a time; ask again once its change is done",
                        by.holder,
                        self.name,
                        by.left_ms.div_ceil(1000)
                    )))
                }
                Ok(Err(_)) => tokio::time::sleep(Duration::from_millis(250)).await,
                Err(e) => return Err(unavailable(format!("the cluster map's Lease {}: {e}: no map is given without it", self.name))),
            }
        }
    }

    /// One try of [`Self::take`] at `now`: `Err(HeldBy)` while another
    /// holds it; the outer error is the API server's.
    pub async fn try_take(&self, now: u64) -> Result<Result<Option<SlotMap>, HeldBy>, String> {
        use std::sync::atomic::Ordering;
        let l = match self.kube.call(reqwest::Method::GET, &self.path, None).await? {
            (200..=299, j) => Some(j),
            (404, _) => None,
            (403, j) => {
                if !self.forbidden.swap(true, Ordering::Relaxed) {
                    tracing::warn!(lease = %self.name, error = %message(&j), "this coordinator's account may not use the cluster map's Lease (its Role is from an older operator): it changes the map without one, and another coordinator may change it at the same time");
                }
                return Ok(Ok(None));
            }
            (s, j) => return Err(format!("get {}: {s}: {}", self.path, message(&j))),
        };
        self.forbidden.store(false, Ordering::Relaxed);
        let Some(l) = l else {
            let obj = self.object(None, now, None);
            return match self.kube.call(reqwest::Method::POST, &self.collection, Some(&obj)).await? {
                (200..=299, j) => {
                    *self.held() = version(&j);
                    Ok(Ok(None))
                }
                // Another made it first.
                (409, _) => Ok(Err(HeldBy { holder: "another coordinator".into(), left_ms: TERM.as_millis() as u64 })),
                (s, j) => Err(format!("create {}: {s}: {}", self.path, message(&j))),
            };
        };
        let holder = l["spec"]["holderIdentity"].as_str().unwrap_or_default();
        let kept = l["metadata"]["annotations"][KEPT_MS].as_str().and_then(|t| t.parse::<u64>().ok()).unwrap_or(0);
        let term = l["spec"]["leaseDurationSeconds"].as_u64().unwrap_or(TERM.as_secs()) * 1000;
        if !holder.is_empty() && holder != self.me && now < kept + term {
            return Ok(Err(HeldBy { holder: holder.to_string(), left_ms: kept + term - now }));
        }
        let doubt = match l["metadata"]["annotations"][DOUBT].as_str().filter(|d| !d.is_empty()) {
            None => None,
            Some(d) => {
                let j: Json = serde_json::from_str(d).map_err(|e| format!("the map in doubt the Lease keeps: {e}"))?;
                Some(SlotMap::from_json(&j).map_err(|e| format!("the map in doubt the Lease keeps: {e}"))?)
            }
        };
        let obj = self.object(version(&l), now, Some(l["metadata"]["annotations"][DOUBT].as_str().unwrap_or_default()));
        match self.kube.call(reqwest::Method::PUT, &self.path, Some(&obj)).await? {
            (200..=299, j) => {
                *self.held() = version(&j);
                if !holder.is_empty() && holder != self.me {
                    tracing::warn!(lease = %self.name, from = %holder, "took the cluster map's Lease, which another coordinator held and did not keep");
                }
                Ok(Ok(doubt))
            }
            (409, _) => Ok(Err(HeldBy { holder: "another coordinator".into(), left_ms: TERM.as_millis() as u64 })),
            (s, j) => Err(format!("update {}: {s}: {}", self.path, message(&j))),
        }
    }

    /// Keeps the Lease at `now` with `doubt` as its map in doubt: before a
    /// map is given (the map), and once a change ends (what it left in
    /// doubt, or none). Refused (503) once another took it, or when the API
    /// server does not answer: the map is not given then.
    pub async fn keep(&self, doubt: Option<&SlotMap>, now: u64) -> Result<(), ApiError> {
        let d = doubt.map(|m| m.to_json().to_string()).unwrap_or_default();
        self.write(Some(&d), true, now).await
    }

    /// Ends this coordinator's change at `now`, keeping `doubt` as the map
    /// in doubt: the Lease is free for any coordinator from then.
    pub async fn release(&self, doubt: Option<&SlotMap>, now: u64) -> Result<(), ApiError> {
        let d = doubt.map(|m| m.to_json().to_string()).unwrap_or_default();
        let r = self.write(Some(&d), false, now).await;
        *self.held() = None;
        r
    }

    async fn write(&self, doubt: Option<&str>, hold: bool, now: u64) -> Result<(), ApiError> {
        if self.forbidden.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }
        let Some(v) = self.held().clone() else {
            return Err(unavailable(format!("this coordinator does not hold the cluster map's Lease {}", self.name)));
        };
        let mut obj = self.object(Some(v), now, doubt);
        if !hold {
            obj["spec"]["holderIdentity"] = json!("");
        }
        let lost = |why: String| {
            *self.held() = None;
            unavailable(format!("the cluster map's Lease {}: {why}: no map is given without it", self.name))
        };
        match self.kube.call(reqwest::Method::PUT, &self.path, Some(&obj)).await {
            Ok((200..=299, j)) => {
                *self.held() = version(&j).filter(|_| hold);
                Ok(())
            }
            Ok((409, _)) => Err(lost("another coordinator took it".into())),
            Ok((s, j)) => Err(lost(format!("{s}: {}", message(&j)))),
            Err(e) => Err(lost(e)),
        }
    }

    fn object(&self, version: Option<String>, now: u64, doubt: Option<&str>) -> Json {
        let mut meta = json!({"name": self.name, "annotations": {KEPT_MS: now.to_string(), DOUBT: doubt.unwrap_or_default()}});
        if let Some(v) = version {
            meta["resourceVersion"] = json!(v);
        }
        json!({
            "apiVersion": "coordination.k8s.io/v1", "kind": "Lease", "metadata": meta,
            "spec": {"holderIdentity": self.me, "leaseDurationSeconds": TERM.as_secs()},
        })
    }
}

fn version(j: &Json) -> Option<String> {
    j["metadata"]["resourceVersion"].as_str().map(String::from)
}

fn message(j: &Json) -> String {
    j["message"].as_str().map(String::from).unwrap_or_else(|| j.to_string())
}

fn unavailable(message: String) -> ApiError {
    ApiError { status: 503, message }
}
