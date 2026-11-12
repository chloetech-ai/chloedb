//! The clusters made from the cloud view, and the loop that sees each one
//! through: the cloud making it (a CloudFormation stack on AWS, a DOKS
//! cluster on DigitalOcean), Strata installed through the new cluster's
//! API, its copies starting, ready, then deleted (its volumes first, then
//! the cloud's cluster). Each step asks the cloud or the cluster where it
//! is now, so the loop picks up where it was after a restart; the list is
//! kept in a file beside the database. Nothing here holds a secret: the
//! cloud's key stays with [`Aws`] and [`DigitalOcean`].

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

use super::install;
use crate::aws::Aws;
use crate::digitalocean::DigitalOcean;
use crate::operator::kube::Kube;

/// Where a cluster is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// The cloud makes the cluster and its nodes.
    Creating,
    /// Strata's operator and the cluster are being applied.
    Installing,
    /// The operator starts the copies.
    Starting,
    /// Every copy answers.
    Ready,
    /// Its volumes, then the cloud's cluster, are being deleted.
    Deleting,
    /// Gone: nothing left to bill.
    Deleted,
    /// It stopped on an error; it may still hold what the cloud made, so
    /// it is deleted, not forgotten.
    Failed,
}

/// One thing that happened to a cluster.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Step {
    pub at_ms: u64,
    pub phase: Phase,
    pub text: String,
}

/// A cluster made from the cloud view.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Made {
    pub id: String,
    /// `aws` or `digitalocean`.
    pub provider: String,
    /// The StrataCluster's name.
    pub name: String,
    /// The cloud's cluster: `strata-<name>`.
    pub cluster: String,
    pub region: String,
    pub machine: String,
    pub nodes: u32,
    /// What it costs an hour while it runs, as planned.
    pub cost_hour: f64,
    pub by: String,
    pub created_ms: u64,
    pub phase: Phase,
    /// The stack's id (AWS) or the cluster's (DigitalOcean), once asked for.
    pub cloud_id: Option<String>,
    /// What makes the cloud's cluster: a CloudFormation template, or DOKS's request.
    pub request: Json,
    /// The operator's image.
    pub image: String,
    /// The cluster's objects: its namespace, storage class and StrataCluster.
    pub objects: Vec<Json>,
    pub steps: Vec<Step>,
    /// The last error, while it lasts.
    pub error: Option<String>,
    /// The StrataCluster's status, last read.
    pub strata: Json,
    /// The cloud's progress making it: done, of.
    pub progress: Option<(usize, usize)>,
    pub ready_ms: Option<u64>,
    /// When it was deleted: billing stops there.
    pub ended_ms: Option<u64>,
    /// Bumped by every change asked for from the routes, so a step taken
    /// meanwhile does not undo it.
    #[serde(default)]
    pub generation: u64,
    /// Calls that failed in a row in this phase.
    #[serde(default)]
    pub tries: u32,
    #[serde(default)]
    pub checked_ms: u64,
    #[serde(default)]
    pub deleting_since_ms: Option<u64>,
    /// The cluster's volumes are gone (or given up on), so the nodes may go.
    #[serde(default)]
    pub volumes_gone: bool,
    /// The cloud was asked to delete it.
    #[serde(default)]
    pub delete_asked: bool,
    /// When it deletes itself, as asked when it was made or since
    /// ([`Clusters::delete_at`]): what it may cost is bounded by then.
    #[serde(default)]
    pub delete_at_ms: Option<u64>,
    /// Tables of this server it copies in once it is ready
    /// ([`crate::copy::bring_when_ready`]).
    #[serde(default)]
    pub bring: Vec<String>,
    /// What bringing them did: the copy's answer, or `{"error"}`.
    #[serde(default)]
    pub brought: Option<Json>,
    /// Found in the account rather than made here ([`Clusters::adopt`]):
    /// it came onto the list only to be deleted, and what it cost before
    /// is not known here.
    #[serde(default)]
    pub found: bool,
}

/// How long the parts that wait wait before giving up.
const INSTALL_TRIES: u32 = 40;
const VOLUMES_WAIT_MS: u64 = 10 * 60 * 1000;
/// A ready cluster's status is read at most this often.
const READY_EVERY_MS: u64 = 60 * 1000;
const STEPS_KEPT: usize = 60;

fn now_ms() -> u64 {
    crate::operator::now_ms()
}

/// `YYYY-MM-DD HH:MM` (UTC) of a time in milliseconds.
pub fn utc(ms: u64) -> String {
    let a = crate::aws::sig::amz_date(ms);
    format!("{}-{}-{} {}:{}", &a[..4], &a[4..6], &a[6..8], &a[9..11], &a[11..13])
}

impl Made {
    fn note(&mut self, text: impl Into<String>) {
        let text = text.into();
        if self.steps.last().is_some_and(|s| s.text == text) {
            return;
        }
        self.steps.push(Step { at_ms: now_ms(), phase: self.phase, text });
        if self.steps.len() > STEPS_KEPT {
            self.steps.drain(..self.steps.len() - STEPS_KEPT);
        }
    }

    fn to(&mut self, phase: Phase, text: impl Into<String>) {
        self.phase = phase;
        self.tries = 0;
        self.error = None;
        self.note(text);
    }

    fn fail(&mut self, e: impl Into<String>) {
        let e = e.into();
        self.to(Phase::Failed, format!("stopped: {e}"));
        self.error = Some(e);
    }

    /// Its time to delete itself has come, and nothing deletes it yet.
    fn due(&self, now: u64) -> bool {
        self.delete_at_ms.is_some_and(|t| now >= t) && !matches!(self.phase, Phase::Deleting | Phase::Deleted) && !(self.phase == Phase::Failed && self.cloud_id.is_none())
    }

    fn installed(&self) -> bool {
        // One found in the account may hold ChloeDB's volumes: they go first.
        self.found
            || self.steps.iter().any(|s| s.phase == Phase::Starting) || matches!(self.phase, Phase::Starting | Phase::Ready)
    }

    /// What the page shows: everything but what makes it, with what it has
    /// cost so far and how to reach it.
    pub fn view(&self) -> Json {
        let end = self.ended_ms.unwrap_or_else(now_ms);
        let hours = end.saturating_sub(self.created_ms) as f64 / 3_600_000.0;
        let connect: Vec<String> = match (self.provider.as_str(), &self.cloud_id) {
            ("aws", _) => vec![format!("aws eks update-kubeconfig --region {} --name {}", self.region, self.cluster)],
            ("digitalocean", Some(id)) => vec![format!("doctl kubernetes cluster kubeconfig save {id}")],
            _ => Vec::new(),
        };
        let connect = if connect.is_empty() { connect } else { [connect, vec![format!("kubectl -n {} port-forward svc/{} 7700:7700", super::NAMESPACE, self.name)]].concat() };
        json!({
            "id": self.id, "provider": self.provider, "name": self.name, "cluster": self.cluster, "region": self.region,
            "machine": self.machine, "nodes": self.nodes, "cost_hour": self.cost_hour, "cost_so_far": self.cost_hour * hours,
            "by": self.by, "created_ms": self.created_ms, "ready_ms": self.ready_ms, "ended_ms": self.ended_ms,
            "phase": self.phase, "cloud_id": self.cloud_id, "image": self.image, "steps": self.steps, "error": self.error,
            "progress": self.progress.map(|(d, n)| json!({"done": d, "of": n})),
            "strata": self.strata, "summary": if self.strata.is_null() { Json::Null } else { json!(install::summary(&self.strata)) },
            "connect": if matches!(self.phase, Phase::Starting | Phase::Ready) { json!(connect) } else { json!([]) },
            "delete_at_ms": self.delete_at_ms,
            "bring": self.bring, "brought": self.brought, "found": self.found,
            // What it will have cost by then, at the planned price.
            "cost_by_delete": self.delete_at_ms.map(|t| self.cost_hour * t.saturating_sub(self.created_ms) as f64 / 3_600_000.0),
        })
    }
}

/// The clusters made from this server, and the loop that moves them on.
pub struct Clusters {
    made: Mutex<Vec<Made>>,
    /// A client of each cluster's API, with its token and when it was
    /// made ([`Clusters::kube_of`]).
    kubes: Mutex<std::collections::HashMap<String, (Kube, u64)>>,
    file: Option<PathBuf>,
    every: Mutex<Duration>,
    wake: tokio::sync::Notify,
    running: AtomicBool,
}

impl Default for Clusters {
    fn default() -> Clusters {
        Clusters::memory()
    }
}

impl Clusters {
    /// Kept in memory only.
    pub fn memory() -> Clusters {
        Clusters { made: Mutex::new(Vec::new()), kubes: Mutex::new(std::collections::HashMap::new()), file: None, every: Mutex::new(Duration::from_secs(15)), wake: tokio::sync::Notify::new(), running: AtomicBool::new(false) }
    }

    /// `<db>.cloud.json`, beside the database.
    pub fn file_of(db: &Path) -> PathBuf {
        let mut p = db.as_os_str().to_owned();
        p.push(".cloud.json");
        p.into()
    }

    /// Kept in `file` (read back if it is there).
    pub fn open(file: PathBuf) -> Result<Clusters, String> {
        let made = match std::fs::read(&file) {
            Ok(b) => serde_json::from_slice::<Vec<Made>>(&b).map_err(|e| format!("{}: {e}", file.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(format!("{}: {e}", file.display())),
        };
        let c = Clusters::memory();
        *c.lock() = made;
        Ok(Clusters { file: Some(file), ..c })
    }

    /// How often the loop asks the clouds while a cluster is on its way
    /// (15 s; tests ask more often).
    /// How often the loop looks at the clusters on their way.
    pub fn every(&self) -> Duration {
        *self.every.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn with_every(self, every: Duration) -> Clusters {
        *self.every.lock().unwrap_or_else(|e| e.into_inner()) = every;
        self
    }

    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Made>> {
        self.made.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn save(&self, list: &[Made]) -> Result<(), String> {
        let Some(f) = &self.file else { return Ok(()) };
        let b = serde_json::to_vec_pretty(list).map_err(|e| e.to_string())?;
        crate::aws::write_private(f, &b).map_err(|e| format!("write {}: {e}", f.display()))
    }

    /// Every cluster, the newest first.
    pub fn list(&self) -> Vec<Made> {
        let mut l = self.lock().clone();
        l.sort_by(|a, b| b.created_ms.cmp(&a.created_ms));
        l
    }

    pub fn get(&self, id: &str) -> Option<Made> {
        self.lock().iter().find(|m| m.id == id).cloned()
    }

    /// Adds a cluster to make; refused while one of the same name lives in
    /// that cloud and region.
    pub fn add(&self, mut m: Made) -> Result<Made, String> {
        let mut list = self.lock();
        if let Some(o) = list.iter().find(|o| o.provider == m.provider && o.region == m.region && o.cluster == m.cluster && o.phase != Phase::Deleted) {
            return Err(format!("{} is already made from here ({:?}): delete it first, or pick another name", o.cluster, o.phase));
        }
        m.note(match m.provider.as_str() {
            "aws" => format!("asked AWS for {} × {} in {}", m.nodes, m.machine, m.region),
            _ => format!("asked DigitalOcean for {} × {} in {}", m.nodes, m.machine, m.region),
        });
        list.push(m.clone());
        self.save(&list)?;
        drop(list);
        self.wake.notify_one();
        Ok(m)
    }

    /// Starts deleting the cluster `id`: its volumes, then the cloud's
    /// cluster.
    pub fn delete(&self, id: &str, by: &str) -> Result<Made, String> {
        let mut list = self.lock();
        let m = list.iter_mut().find(|m| m.id == id).ok_or_else(|| format!("no cluster {id}"))?;
        match m.phase {
            Phase::Deleted => return Err(format!("{} is already deleted", m.cluster)),
            Phase::Deleting => return Ok(m.clone()),
            _ => {}
        }
        m.generation += 1;
        m.deleting_since_ms = Some(now_ms());
        // Asked again after the cloud failed to delete it: ask the cloud again.
        m.delete_asked = false;
        m.to(Phase::Deleting, format!("{by} asked to delete it"));
        let out = m.clone();
        self.save(&list)?;
        drop(list);
        self.wake.notify_one();
        Ok(out)
    }

    /// Puts a cluster found in the account ([`found`]) on the list to be
    /// deleted, as one made here would be: its volumes, then the cloud's
    /// cluster. Refused while the list has a live one with the same cloud
    /// id, or of the same name in that cloud and region.
    pub fn adopt(&self, mut m: Made, by: &str) -> Result<Made, String> {
        let mut list = self.lock();
        if let Some(o) = list.iter().find(|o| o.provider == m.provider && o.phase != Phase::Deleted && ((o.cloud_id.is_some() && o.cloud_id == m.cloud_id) || (o.region == m.region && o.cluster == m.cluster))) {
            return Err(format!("{} is on this server's list already ({:?}): delete it there", o.cluster, o.phase));
        }
        m.found = true;
        m.phase = Phase::Deleting;
        m.deleting_since_ms = Some(now_ms());
        m.delete_asked = false;
        m.volumes_gone = false;
        m.delete_at_ms = None;
        m.note(format!("{by} asked to delete it: found in the account, not made here"));
        list.push(m.clone());
        self.save(&list)?;
        drop(list);
        self.wake.notify_one();
        Ok(m)
    }

    /// Sets when the cluster `id` deletes itself (`None`: it is kept until
    /// someone deletes it).
    pub fn delete_at(&self, id: &str, at_ms: Option<u64>, by: &str) -> Result<Made, String> {
        let mut list = self.lock();
        let m = list.iter_mut().find(|m| m.id == id).ok_or_else(|| format!("no cluster {id}"))?;
        if matches!(m.phase, Phase::Deleting | Phase::Deleted) {
            return Err(format!("{} is {} already", m.cluster, if m.phase == Phase::Deleted { "deleted" } else { "being deleted" }));
        }
        m.generation += 1;
        m.delete_at_ms = at_ms;
        let text = match at_ms {
            Some(t) => format!("{by} asked it to delete itself at {} UTC", utc(t)),
            None => format!("{by} kept it: it no longer deletes itself"),
        };
        m.note(text);
        let out = m.clone();
        self.save(&list)?;
        drop(list);
        self.wake.notify_one();
        Ok(out)
    }

    /// What bringing its tables did ([`crate::copy::bring_when_ready`]):
    /// `text` on its steps, `answer` kept.
    pub fn brought(&self, id: &str, text: String, answer: Json) -> Result<(), String> {
        let mut list = self.lock();
        let m = list.iter_mut().find(|m| m.id == id).ok_or_else(|| format!("no cluster {id}"))?;
        m.generation += 1;
        m.brought = Some(answer);
        m.note(text);
        self.save(&list)
    }

    /// Forgets a cluster that is deleted, or that failed before the cloud
    /// made anything.
    pub fn forget(&self, id: &str) -> Result<Made, String> {
        let mut list = self.lock();
        let i = list.iter().position(|m| m.id == id).ok_or_else(|| format!("no cluster {id}"))?;
        let m = &list[i];
        let nothing_made = m.phase == Phase::Failed && m.cloud_id.is_none();
        if m.phase != Phase::Deleted && !nothing_made {
            return Err(format!("{} may still be billed: delete it first", m.cluster));
        }
        let m = list.remove(i);
        self.save(&list)?;
        Ok(m)
    }

    /// A client of the API of the cluster `id`, made here, with a token
    /// of its cloud's (EKS's last 15 minutes, DOKS's longer): kept for 5
    /// minutes, so that the hub's calls through it ([`crate::hub`]) do
    /// not each ask the cloud.
    pub async fn kube_of(&self, id: &str, aws: &Aws, d: &DigitalOcean) -> Result<Kube, String> {
        const KEPT_MS: u64 = 5 * 60 * 1000;
        let now = now_ms();
        if let Some((k, at)) = self.kubes.lock().unwrap_or_else(|e| e.into_inner()).get(id) {
            if now.saturating_sub(*at) < KEPT_MS {
                return Ok(k.clone());
            }
        }
        let m = self.get(id).ok_or_else(|| format!("no cluster {id} was made here"))?;
        if matches!(m.phase, Phase::Deleting | Phase::Deleted) {
            return Err(format!("{} is {}", m.cluster, if m.phase == Phase::Deleted { "deleted" } else { "being deleted" }));
        }
        let k = kube(&m, aws, d).await?;
        self.kubes.lock().unwrap_or_else(|e| e.into_inner()).insert(id.to_string(), (k.clone(), now));
        Ok(k)
    }

    /// Starts the loop once (on the first cluster, or at start when the
    /// file has some on their way).
    pub fn start(self: &Arc<Self>, aws: Arc<Aws>, d: Arc<DigitalOcean>) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.clone();
        tokio::spawn(async move { me.run(aws, d).await });
    }

    pub fn wake(&self) {
        self.wake.notify_one();
    }

    async fn run(self: Arc<Self>, aws: Arc<Aws>, d: Arc<DigitalOcean>) {
        loop {
            self.tick(&aws, &d).await;
            let every = *self.every.lock().unwrap_or_else(|e| e.into_inner());
            tokio::select! {
                _ = tokio::time::sleep(every) => {}
                _ = self.wake.notified() => {}
            }
        }
    }

    /// One pass: moves every cluster on its way one step.
    pub async fn tick(&self, aws: &Aws, d: &DigitalOcean) {
        let now = now_ms();
        let todo: Vec<Made> = self.lock().iter().filter(|m| m.due(now) || match m.phase {
            Phase::Deleted | Phase::Failed => false,
            Phase::Ready => now.saturating_sub(m.checked_ms) >= READY_EVERY_MS,
            _ => true,
        }).cloned().collect();
        for mut m in todo {
            let generation = m.generation;
            advance(&mut m, aws, d).await;
            m.checked_ms = now_ms();
            let mut list = self.lock();
            if let Some(cur) = list.iter_mut().find(|x| x.id == m.id) {
                if cur.generation == generation {
                    *cur = m;
                } else if cur.cloud_id.is_none() {
                    // Asked to delete meanwhile: keep what the cloud made.
                    cur.cloud_id = m.cloud_id;
                }
            }
            if let Err(e) = self.save(&list) {
                tracing::warn!("cloud clusters: {e}");
            }
        }
    }
}

/// One step of `m`. A call that fails leaves it where it is, noted; the
/// steps that cannot go on fail it.
async fn advance(m: &mut Made, aws: &Aws, d: &DigitalOcean) {
    // Its time is up: it deletes itself as a person's Delete would.
    if m.due(now_ms()) {
        m.deleting_since_ms = Some(now_ms());
        m.delete_asked = false;
        let hours = m.delete_at_ms.unwrap_or_default().saturating_sub(m.created_ms) as f64 / 3_600_000.0;
        m.to(Phase::Deleting, format!("deleting itself, {hours:.1} hours after it was asked for, as set"));
        return;
    }
    let r = match m.phase {
        Phase::Creating => creating(m, aws, d).await,
        Phase::Installing => installing(m, aws, d).await,
        Phase::Starting | Phase::Ready => watching(m, aws, d).await,
        Phase::Deleting => deleting(m, aws, d).await,
        Phase::Deleted | Phase::Failed => Ok(()),
    };
    if let Err(e) = r {
        m.tries += 1;
        m.note(format!("retrying: {e}"));
        m.error = Some(e);
    }
}

/// The cloud's own cluster of this name, if there is one: what a create
/// that was cut short may have made.
async fn find(m: &Made, aws: &Aws, d: &DigitalOcean) -> Result<Option<String>, String> {
    Ok(match m.provider.as_str() {
        "aws" => aws.stack(&m.region, &m.cluster).await?.filter(|s| s.status != "DELETE_COMPLETE").map(|s| s.id),
        _ => d.cluster_named(&m.cluster).await?.and_then(|c| c["id"].as_str().map(str::to_string)),
    })
}

async fn creating(m: &mut Made, aws: &Aws, d: &DigitalOcean) -> Result<(), String> {
    let Some(id) = m.cloud_id.clone() else {
        if let Some(id) = find(m, aws, d).await? {
            m.cloud_id = Some(id);
            m.note("found the cluster this server asked for");
            return Ok(());
        }
        let made = match m.provider.as_str() {
            "aws" => {
                // Who asked for it, as the stack's tags (and so its
                // resources') say it. When it deletes itself is not a tag:
                // the time may move after, and a tag left behind would
                // tell a sweeper to delete a cluster someone kept.
                let owner: String = m.by.chars().map(|c| if c.is_alphanumeric() || " _.:/=+-@".contains(c) { c } else { '-' }).take(200).collect();
                let tags = [("strata.chloetech.ai/cluster", m.name.as_str()), ("strata.chloetech.ai/made-by", "chloedb-server"), ("strata.chloetech.ai/owner", owner.as_str())];
                aws.create_stack(&m.region, &m.cluster, &m.request, &m.id, &tags).await
            }
            _ => d.create_cluster(&m.request).await,
        };
        match made {
            Ok(id) => {
                m.cloud_id = Some(id);
                m.note(if m.provider == "aws" { "CloudFormation is making the network, the EKS cluster and its nodes" } else { "DigitalOcean is making the cluster and its nodes" });
            }
            // It may have been made all the same (an answer lost on the
            // way): failed only once the cloud says it has no such cluster,
            // and asked again otherwise (CreateStack's request token makes
            // asking twice make one stack).
            Err(e) => match find(m, aws, d).await {
                Ok(Some(id)) => {
                    m.cloud_id = Some(id);
                    m.note("the cloud made it all the same");
                }
                Ok(None) => m.fail(e),
                Err(f) => return Err(format!("{e} (and whether it was made is not known yet: {f})")),
            },
        }
        return Ok(());
    };
    if m.provider == "aws" {
        let Some(s) = aws.stack(&m.region, &id).await? else {
            m.fail("the stack is gone");
            return Ok(());
        };
        match s.status.as_str() {
            "CREATE_COMPLETE" => m.to(Phase::Installing, "the stack is made: the EKS cluster and its nodes are up"),
            "CREATE_IN_PROGRESS" => m.progress = aws.stack_progress(&m.region, &id).await.ok(),
            st if st.contains("ROLLBACK") || st.ends_with("FAILED") => {
                let why = aws.stack_failure(&m.region, &id).await.ok().flatten().or(s.reason).unwrap_or_else(|| st.to_string());
                m.fail(format!("CloudFormation could not make the stack ({st}): {why}"));
            }
            st => m.note(format!("the stack is {st}")),
        }
    } else {
        let Some(c) = d.cluster(&id).await? else {
            m.fail("the cluster is gone");
            return Ok(());
        };
        let nodes = c["node_pools"].as_array().map(Vec::as_slice).unwrap_or_default().iter().flat_map(|p| p["nodes"].as_array().cloned().unwrap_or_default()).collect::<Vec<_>>();
        if !nodes.is_empty() {
            m.progress = Some((nodes.iter().filter(|n| n["status"]["state"] == "running").count(), nodes.len()));
        }
        match c["status"]["state"].as_str().unwrap_or_default() {
            "running" => m.to(Phase::Installing, "the cluster and its nodes are up"),
            "errored" | "invalid" | "deleted" | "deleting" => {
                let why = c["status"]["message"].as_str().unwrap_or("no reason given").to_string();
                m.fail(format!("DigitalOcean could not make the cluster ({}): {why}", c["status"]["state"].as_str().unwrap_or("?")));
            }
            _ => {}
        }
    }
    Ok(())
}

/// A client of the cluster's API, with a fresh token.
async fn kube(m: &Made, aws: &Aws, d: &DigitalOcean) -> Result<Kube, String> {
    let (endpoint, ca, token) = match m.provider.as_str() {
        "aws" => {
            let (e, ca) = aws.eks_cluster(&m.region, &m.cluster).await?;
            (e, ca, aws.eks_token(&m.region, &m.cluster)?)
        }
        _ => d.credentials(m.cloud_id.as_deref().ok_or("the cluster has no id yet")?).await?,
    };
    let ca = (!ca.is_empty() && !endpoint.starts_with("http://")).then_some(ca);
    install::client(&endpoint, ca, &token)
}

async fn installing(m: &mut Made, aws: &Aws, d: &DigitalOcean) -> Result<(), String> {
    if m.tries >= INSTALL_TRIES {
        let e = m.error.clone().unwrap_or_else(|| "the cluster's API did not take ChloeDB".into());
        m.fail(format!("installing gave up after {INSTALL_TRIES} tries: {e}"));
        return Ok(());
    }
    let k = kube(m, aws, d).await?;
    let done = install::apply(&k, &install::objects(&m.image, &m.objects)).await?;
    m.to(Phase::Starting, format!("installed the operator ({}) and the cluster: {} objects", m.image, done.len()));
    Ok(())
}

async fn watching(m: &mut Made, aws: &Aws, d: &DigitalOcean) -> Result<(), String> {
    let k = kube(m, aws, d).await?;
    let Some(s) = install::status(&k, &m.name).await? else { return Ok(()) };
    let line = install::summary(&s);
    let changed = m.strata.is_null() || install::summary(&m.strata) != line;
    m.strata = s;
    m.tries = 0;
    m.error = None;
    if m.phase == Phase::Starting && m.strata["phase"] == "Ready" {
        m.ready_ms = Some(now_ms());
        m.to(Phase::Ready, format!("ready: {}", line.trim_start_matches("Ready, ")));
    } else if changed {
        m.note(format!("the StrataCluster: {line}"));
    }
    Ok(())
}

async fn deleting(m: &mut Made, aws: &Aws, d: &DigitalOcean) -> Result<(), String> {
    let since = *m.deleting_since_ms.get_or_insert_with(now_ms);
    // The volumes first, while the cluster's API answers: their disks are
    // the cloud driver's, not the stack's, and would stay and bill.
    if !m.volumes_gone && m.cloud_id.is_some() && m.installed() {
        let left = match kube(m, aws, d).await {
            Ok(k) => install::teardown(&k, &m.name).await,
            Err(e) => Err(e),
        };
        let waited = now_ms().saturating_sub(since) >= VOLUMES_WAIT_MS;
        match left {
            Ok(0) => {
                m.volumes_gone = true;
                m.note("deleted the StrataCluster and its volumes");
            }
            Ok(n) if !waited => {
                m.note(format!("waiting for {n} volumes to be deleted"));
                return Ok(());
            }
            Err(e) if !waited => return Err(e),
            Ok(n) => {
                m.volumes_gone = true;
                m.note(format!("{n} volumes were not deleted in 10 minutes: deleting the nodes anyway; look for them in the console"));
            }
            Err(e) => {
                m.volumes_gone = true;
                m.note(format!("could not delete the volumes in 10 minutes ({e}): deleting the nodes anyway; look for them in the console"));
            }
        }
    }
    let id = match &m.cloud_id {
        Some(id) => id.clone(),
        None => match find(m, aws, d).await? {
            Some(id) => {
                m.cloud_id = Some(id.clone());
                id
            }
            None => {
                m.ended_ms = Some(now_ms());
                m.to(Phase::Deleted, "deleted: the cloud had made nothing");
                return Ok(());
            }
        },
    };
    if m.provider == "aws" {
        match aws.stack(&m.region, &id).await? {
            None => gone(m),
            Some(s) if s.status == "DELETE_COMPLETE" => gone(m),
            Some(s) if s.status == "DELETE_IN_PROGRESS" => {}
            // Not asked yet, or asked again after DELETE_FAILED (the stack
            // stays so until it is asked again).
            Some(_) if !m.delete_asked => {
                aws.delete_stack(&m.region, &id).await?;
                m.delete_asked = true;
                m.note("CloudFormation is deleting the nodes, the EKS cluster and the network");
            }
            Some(s) if s.status == "DELETE_FAILED" => {
                let why = aws.stack_failure(&m.region, &id).await.ok().flatten().or(s.reason).unwrap_or_default();
                m.fail(format!("CloudFormation could not delete the stack: {why}"));
            }
            Some(_) => {}
        }
    } else {
        if !m.delete_asked {
            d.delete_cluster(&id).await?;
            m.delete_asked = true;
            m.note("DigitalOcean is deleting the cluster, its nodes and what it made");
        }
        match d.cluster(&id).await? {
            None => gone(m),
            Some(c) if c["status"]["state"] == "deleted" => gone(m),
            Some(_) => {}
        }
    }
    Ok(())
}

/// The account's DOKS clusters ([`DigitalOcean::clusters`]) that look like
/// ChloeDB's (tagged `strata`, as a plan makes them, or named `strata-*`)
/// and that this server's list does not follow: what the page offers to
/// delete. `node` is the node this server runs on, if it says: the
/// cluster that has it is this server's own, and is marked so.
pub fn found_doks(clusters: &[Json], made: &[Made], node: Option<&str>) -> Vec<Json> {
    clusters
        .iter()
        .filter(|c| {
            let name = c["name"].as_str().unwrap_or_default();
            let tagged = c["tags"].as_array().is_some_and(|t| t.iter().any(|t| t == "strata"));
            (tagged || name.starts_with("strata-")) && c["status"]["state"] != "deleted"
        })
        .filter(|c| !made.iter().any(|m| m.provider == "digitalocean" && m.phase != Phase::Deleted && (m.cloud_id.as_deref() == c["id"].as_str() || (m.cluster == c["name"] && m.region == c["region"]))))
        .map(|c| {
            let pools = c["node_pools"].as_array().map(Vec::as_slice).unwrap_or_default();
            let nodes: u64 = pools.iter().map(|p| p["count"].as_u64().unwrap_or(0)).sum();
            let machines: Vec<&str> = pools.iter().filter_map(|p| p["size"].as_str()).collect();
            let here = node.is_some_and(|n| pools.iter().any(|p| p["nodes"].as_array().is_some_and(|ns| ns.iter().any(|x| x["name"] == n))));
            json!({
                "provider": "digitalocean", "cloud_id": c["id"], "cluster": c["name"], "region": c["region"],
                "state": c["status"]["state"], "nodes": nodes, "machine": machines.join(", "), "created_at": c["created_at"],
                "owner": c["tags"].as_array().and_then(|t| t.iter().filter_map(|t| t.as_str()).find_map(|t| t.strip_prefix("strata-owner:"))),
                "here": here,
            })
        })
        .collect()
}

/// The region's CloudFormation stacks ([`Aws::stacks`]) a plan made
/// (tagged `strata.chloetech.ai/cluster`) that are not deleted and that
/// this server's list does not follow.
pub fn found_stacks(region: &str, stacks: &[Json], made: &[Made]) -> Vec<Json> {
    stacks
        .iter()
        .filter(|s| s["Tags"].as_array().is_some_and(|t| t.iter().any(|t| t["Key"] == "strata.chloetech.ai/cluster")))
        .filter(|s| s["StackStatus"].as_str().is_some_and(|st| st != "DELETE_COMPLETE"))
        .filter(|s| !made.iter().any(|m| m.provider == "aws" && m.phase != Phase::Deleted && (m.cloud_id.as_deref() == s["StackId"].as_str() || (m.cluster == s["StackName"] && m.region == region))))
        .map(|s| {
            json!({
                "provider": "aws", "cloud_id": s["StackId"], "cluster": s["StackName"], "region": region,
                "state": s["StackStatus"], "nodes": Json::Null, "machine": Json::Null,
                "created_at": s["CreationTime"], "owner": Json::Null, "here": false,
            })
        })
        .collect()
}

fn gone(m: &mut Made) {
    m.ended_ms = Some(now_ms());
    if m.found {
        m.to(Phase::Deleted, "deleted");
        return;
    }
    let hours = m.ended_ms.unwrap_or_default().saturating_sub(m.created_ms) as f64 / 3_600_000.0;
    let minutes = (hours * 60.0).round().max(1.0);
    let up = if minutes == 1.0 { "a minute".to_string() } else if hours < 1.0 { format!("{minutes} minutes") } else { format!("{hours:.1} hours") };
    m.to(Phase::Deleted, format!("deleted, after {up} (about ${:.2})", m.cost_hour * hours));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn made(phase: Phase) -> Made {
        Made {
            id: "aws-strata-search-1".into(), provider: "aws".into(), name: "search".into(), cluster: "strata-search".into(), region: "us-east-1".into(),
            machine: "m8g.large".into(), nodes: 3, cost_hour: 0.4, by: "admin".into(), created_ms: 1, phase, cloud_id: None, request: json!({}),
            image: "img".into(), objects: Vec::new(), steps: Vec::new(), error: None, strata: Json::Null, progress: None, ready_ms: None, ended_ms: None,
            generation: 0, tries: 0, checked_ms: 0, deleting_since_ms: None, volumes_gone: false, delete_asked: false, delete_at_ms: None, bring: Vec::new(), brought: None,
            found: false,
        }
    }

    #[test]
    fn a_cluster_deletes_itself_when_its_time_is_up() {
        let mut m = made(Phase::Ready);
        m.cloud_id = Some("x".into());
        assert!(!m.due(u64::MAX), "kept until someone deletes it");
        m.delete_at_ms = Some(5);
        assert!(m.due(5) && !m.due(4));
        m.phase = Phase::Deleting;
        assert!(!m.due(10), "deleting already");
        let mut f = made(Phase::Failed);
        f.delete_at_ms = Some(1);
        assert!(!f.due(10), "nothing made, nothing to delete");
        f.cloud_id = Some("x".into());
        assert!(f.due(10), "a failed one may still bill");
        assert_eq!(utc(1_793_966_400_000), "2026-11-06 12:00");
    }

    #[test]
    fn clusters_are_kept_and_only_forgotten_when_nothing_bills() {
        let dir = std::env::temp_dir().join(format!("strata-cloud-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("db.cloud.json");
        let _ = std::fs::remove_file(&file);
        let c = Clusters::open(file.clone()).unwrap();
        let m = c.add(made(Phase::Creating)).unwrap();
        assert!(m.steps[0].text.contains("3 × m8g.large in us-east-1"), "{:?}", m.steps);
        assert!(c.add(made(Phase::Creating)).unwrap_err().contains("already made"));
        assert!(c.forget(&m.id).unwrap_err().contains("delete it first"));
        let d = c.delete(&m.id, "admin").unwrap();
        assert_eq!((d.phase, d.generation), (Phase::Deleting, 1));
        // Read back from the file.
        let again = Clusters::open(file.clone()).unwrap();
        assert_eq!(again.get(&m.id).unwrap().phase, Phase::Deleting);
        // Failed before the cloud made anything: forgotten.
        let mut f = made(Phase::Failed);
        f.id = "x".into();
        f.cluster = "strata-other".into();
        let f = again.add(f).unwrap();
        assert!(again.forget(&f.id).is_ok());
        assert!(again.get(&f.id).is_none());
        let v = again.get(&m.id).unwrap().view();
        assert!(v.get("request").is_none() && v.get("objects").is_none() && v["cost_so_far"].as_f64().unwrap() > 0.0, "{v}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stacks_found_are_chloedbs_live_ones_not_on_the_list() {
        let tag = json!([{"Key": "strata.chloetech.ai/cluster", "Value": "x"}]);
        let stacks = vec![
            json!({"StackId": "arn:1", "StackName": "strata-search", "StackStatus": "CREATE_COMPLETE", "Tags": tag}),
            json!({"StackId": "arn:2", "StackName": "strata-old", "StackStatus": "DELETE_COMPLETE", "Tags": tag}),
            json!({"StackId": "arn:3", "StackName": "billing", "StackStatus": "CREATE_COMPLETE", "Tags": []}),
            json!({"StackId": "arn:4", "StackName": "strata-made", "StackStatus": "CREATE_COMPLETE", "Tags": tag}),
        ];
        let mut m = made(Phase::Ready);
        m.cluster = "strata-made".into();
        m.cloud_id = Some("arn:4".into());
        let f = found_stacks("us-east-1", &stacks, &[m.clone()]);
        assert_eq!(f.iter().map(|s| s["cluster"].as_str().unwrap()).collect::<Vec<_>>(), ["strata-search"], "{f:?}");
        // Deleted from the list: found again.
        m.phase = Phase::Deleted;
        assert_eq!(found_stacks("us-east-1", &stacks, &[m]).len(), 2);
    }

    #[test]
    fn steps_are_noted_once() {
        let mut m = made(Phase::Creating);
        m.note("a");
        m.note("a");
        m.note("b");
        assert_eq!(m.steps.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        for i in 0..100 {
            m.note(format!("{i}"));
        }
        assert_eq!(m.steps.len(), STEPS_KEPT);
        m.fail("no");
        assert_eq!((m.phase, m.error.as_deref()), (Phase::Failed, Some("no")));
    }
}
