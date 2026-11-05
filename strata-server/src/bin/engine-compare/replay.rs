//! `engine-compare replay`: a schedule sent to an engine as an open loop.
//!
//! Each request goes at its planned time whether or not the ones before
//! were answered, and its latency counts from that time (as wrk2 does):
//! when the engine falls behind, the wait shows in the latencies instead of
//! slowing the clients down (coordinated omission). A session's follow-up
//! goes a think time after the step before is answered. At most
//! `--max-inflight` requests are in flight; a request waiting for a slot
//! waits on the clock too. Latencies go into HDR histograms by kind, overall
//! and by 10 s window. A ramp's steps are judged against the profile's SLO
//! as they end, and the replay stops after two that miss it. Writes mark a
//! document now and then and a prober times how soon a search finds it; at
//! the end, what the schedule wrote is deleted and the ratings it changed
//! put back.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hdrhistogram::Histogram;
use serde_json::{json, Value as Json};
use tokio::sync::{mpsc, Semaphore};

use crate::more::{self, Other};
use crate::sim::schedule::{Entry, Header, Op, Reader, Shape, Write};
use crate::{
    data, differs, engine_doc, error_of, found, http_client, id_path, native_get, query_escape, request, restore, s, searchable_soon, send_write, strata_doc, Knobs,
    unsupported, warm_up, Engine, Probe, RawDoc, Timed, WriteReq, KINDS,
};
use strata::metrics::{mean, percentile};

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum CacheMode {
    /// Result caches off (the ranked numbers): Elasticsearch's and OpenSearch's requests ask for no request cache.
    Ranked,
    /// Each engine's caches as it ships them, request cache included.
    Shipped,
}

impl CacheMode {
    fn name(self) -> &'static str {
        match self {
            CacheMode::Ranked => "ranked",
            CacheMode::Shipped => "shipped",
        }
    }
}

#[derive(Clone, Copy)]
pub enum Target {
    Main(Engine),
    More(Other),
}

impl Target {
    fn name(self) -> &'static str {
        match self {
            Target::Main(e) => e.name(),
            Target::More(o) => o.name(),
        }
    }

    fn unsupported(self, kind: &str) -> Option<&'static str> {
        match self {
            Target::Main(e) => unsupported(e, kind),
            Target::More(o) => more::unsupported(o, kind),
        }
    }

    fn differs(self, kind: &str, paid: bool) -> Option<&'static str> {
        match self {
            Target::Main(e) => differs(e, kind, paid),
            Target::More(o) => more::differs(o, kind),
        }
    }
}

pub struct Opts {
    pub max_inflight: Option<usize>,
    pub cache_mode: CacheMode,
    pub paid: bool,
    /// Only this many seconds of the timed schedule.
    pub seconds: Option<f64>,
    pub label: Option<String>,
    pub reset: bool,
    pub out: PathBuf,
}

// --- Rows read by position -----------------------------------------------

/// Vectors of a file of f32 rows, read by position.
struct Vectors {
    file: File,
    dim: usize,
}

impl Vectors {
    fn open(path: &Path, dim: usize) -> Result<Vectors, String> {
        Ok(Vectors { file: File::open(path).map_err(|e| format!("{}: {e}", path.display()))?, dim })
    }

    fn get(&self, row: u64) -> Result<Vec<f32>, String> {
        let mut b = vec![0u8; self.dim * 4];
        self.file.read_exact_at(&mut b, row * self.dim as u64 * 4).map_err(|e| format!("vector row {row}: {e}"))?;
        Ok(b.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect())
    }
}

/// Documents of a JSON-lines file and its vectors, read by row.
struct DocRows {
    file: File,
    ends: Vec<u64>,
    vecs: Vectors,
}

impl DocRows {
    fn open(jsonl: &Path, vecs: &Path, dim: usize) -> Result<DocRows, String> {
        let file = File::open(jsonl).map_err(|e| format!("{}: {e}", jsonl.display()))?;
        let mut ends = Vec::new();
        let mut at = 0u64;
        let mut r = BufReader::with_capacity(1 << 20, file.try_clone().map_err(s)?);
        let mut line = Vec::new();
        loop {
            line.clear();
            let n = r.read_until(b'\n', &mut line).map_err(s)?;
            if n == 0 {
                break;
            }
            at += n as u64;
            ends.push(at);
        }
        Ok(DocRows { file, ends, vecs: Vectors::open(vecs, dim)? })
    }

    fn doc(&self, row: u64) -> Result<(RawDoc, Vec<f32>), String> {
        let i = row as usize;
        let start = if i == 0 { 0 } else { *self.ends.get(i - 1).ok_or_else(|| format!("no document at row {row}"))? };
        let end = *self.ends.get(i).ok_or_else(|| format!("no document at row {row}"))?;
        let mut b = vec![0u8; (end - start) as usize];
        self.file.read_exact_at(&mut b, start).map_err(s)?;
        let d: RawDoc = serde_json::from_slice(&b).map_err(|e| format!("document row {row}: {e}"))?;
        Ok((d, self.vecs.get(row)?))
    }
}

// --- Executors: the requests in flight -------------------------------------

enum Exec {
    Http(reqwest::Client),
    More(more::Conns),
}

enum Rendered {
    Main(crate::Req),
    More(more::Req),
    Write(WriteReq),
    /// A one-node engine's write: its requests, one after the other.
    MoreWrite(Vec<more::Req>),
}

/// At most `n` requests in flight, each with an executor of its own (one
/// Postgres or Redis connection each; HTTP clients share a pool).
struct Slots {
    sem: Semaphore,
    pool: Mutex<Vec<Exec>>,
}

impl Slots {
    async fn run(&self, r: &Rendered) -> (Instant, Result<Json, String>) {
        let _permit = self.sem.acquire().await.expect("slots are never closed");
        let mut exec = self.pool.lock().unwrap().pop().expect("a slot's executor");
        let sent = Instant::now();
        let res = match (&mut exec, r) {
            (Exec::Http(c), Rendered::Main(req)) => match req.send(c).send().await {
                Ok(res) => {
                    let status = res.status();
                    let j = res.json::<Json>().await.unwrap_or(Json::Null);
                    match error_of(&j).or_else(|| (!status.is_success()).then(|| format!("{status}: {j}"))) {
                        Some(e) => Err(e),
                        None => Ok(j),
                    }
                }
                Err(e) => Err(e.to_string()),
            },
            (Exec::Http(c), Rendered::Write(w)) => match send_write(c, w).await {
                None => Ok(Json::Null),
                Some(e) => Err(e),
            },
            (Exec::More(c), Rendered::More(req)) => c.exec(req).await,
            (Exec::More(c), Rendered::MoreWrite(reqs)) => {
                let mut last = Ok(Json::Null);
                for r in reqs {
                    last = c.exec(r).await;
                    if last.is_err() {
                        break;
                    }
                }
                last
            }
            _ => Err("a request for another kind of engine".into()),
        };
        self.pool.lock().unwrap().push(exec);
        (sent, res)
    }
}

// --- What is measured -----------------------------------------------------

const WINDOW_S: f64 = 10.0;

fn hist(sigfig: u8) -> Histogram<u64> {
    Histogram::new_with_bounds(1, 3_600_000_000, sigfig).expect("histogram bounds")
}

#[derive(Default)]
struct KindRec {
    resp: Option<Histogram<u64>>,
    service: Option<Histogram<u64>>,
    errors: u64,
    first_error: Option<String>,
    skipped: u64,
}

impl KindRec {
    fn add(&mut self, resp_us: u64, service_us: u64) {
        self.resp.get_or_insert_with(|| hist(3)).saturating_record(resp_us.max(1));
        self.service.get_or_insert_with(|| hist(3)).saturating_record(service_us.max(1));
    }
}

#[derive(Default)]
struct WinRec {
    offered: u64,
    kinds: BTreeMap<String, (u64, u64, Option<Histogram<u64>>)>,
    reads: Option<Histogram<u64>>,
    read_errors: u64,
}

#[derive(Default, Clone)]
struct StepRec {
    offered: u64,
    completed: u64,
    over_slo: u64,
    errors: u64,
    reads: Option<Histogram<u64>>,
}

#[derive(Default)]
struct Rec {
    kinds: BTreeMap<String, KindRec>,
    windows: BTreeMap<u64, WinRec>,
    steps: Vec<StepRec>,
    warm_done: u64,
    warm_errors: u64,
    lag: Option<Histogram<u64>>,
    visible: Vec<(f64, f64)>,
    visible_failed: u64,
    inserted: Vec<String>,
    deleted: HashSet<String>,
    updated: BTreeSet<u64>,
}

struct Ctx {
    target: Target,
    header: Header,
    urls: Vec<String>,
    next_url: AtomicUsize,
    qvecs: Vectors,
    dvecs: Vectors,
    docs: Option<DocRows>,
    inserts: Option<DocRows>,
    /// Qdrant's BM25 weights of new documents need the collection's average length.
    avgdl: f64,
    client: reqwest::Client,
    cache_mode: CacheMode,
    paid: bool,
    key_by_search: AtomicBool,
    unavailable: Mutex<BTreeMap<String, String>>,
    slots: Slots,
    rec: Mutex<Rec>,
    start: Instant,
    stop: AtomicBool,
    inflight: AtomicUsize,
    sessions: AtomicUsize,
    step_inflight: Vec<AtomicI64>,
    slo_us: u64,
    end_t: f64,
}

impl Ctx {
    fn url(&self) -> &str {
        &self.urls[self.next_url.fetch_add(1, Ordering::Relaxed) % self.urls.len()]
    }

    fn at(&self, t: f64) -> Instant {
        self.start + Duration::from_secs_f64(t.max(0.0))
    }

    fn step_of(&self, t: f64) -> Option<usize> {
        let rel = t - self.header.warmup_s;
        self.header.steps.iter().position(|s| rel >= s.from_s && rel < s.to_s)
    }

    fn window_of(&self, t: f64) -> u64 {
        ((t - self.header.warmup_s).max(0.0) / WINDOW_S) as u64
    }
}

fn label_of(e: &Entry) -> String {
    match &e.w {
        Some(w) if w.job.is_some() => "write (job)".into(),
        Some(w) => format!("write ({})", format!("{:?}", w.op).to_lowercase()),
        None => e.k.clone(),
    }
}

// --- Requests from entries -------------------------------------------------

/// `path` without Elasticsearch's and OpenSearch's request-cache switch.
fn shipped_path(path: &str) -> String {
    path.replace("?request_cache=false&", "?").replace("?request_cache=false", "").replace("&request_cache=false", "")
}

fn main_request(engine: Engine, kind: &str, p: &Probe, url: &str, cache: CacheMode, by_search: bool) -> Option<crate::Req> {
    let sql = KINDS.iter().find(|(k, _)| *k == kind)?.1;
    if kind == "key lookup" && !by_search {
        if let Some((path, body)) = native_get(engine, p.key) {
            return Some(crate::Req { url: format!("{url}{path}"), body: body.as_ref().map_or_else(Vec::new, |b| serde_json::to_vec(b).unwrap()), get: body.is_none() });
        }
    }
    let (path, body) = request(engine, kind, sql, p)?;
    // Solr's /sql reads its statement from the request's parameters only.
    let (path, body) = match body.get("stmt").and_then(|x| x.as_str()) {
        Some(stmt) if engine == Engine::Solr => (format!("{path}?stmt={}", query_escape(stmt)), json!({})),
        _ => (path.to_string(), body),
    };
    let path = if cache == CacheMode::Shipped { shipped_path(&path) } else { path };
    Some(crate::Req { url: format!("{url}{path}"), body: serde_json::to_vec(&body).unwrap(), get: false })
}

/// The probe of an entry: its query (and vector), key, item and filters.
struct Owned {
    q: data::Query,
    key: String,
    item: Vec<f32>,
    seen: Vec<String>,
}

fn owned(ctx: &Ctx, e: &Entry) -> Result<Owned, String> {
    let vector: Vec<f32> = match e.q {
        Some(r) => ctx.qvecs.get(r)?,
        None => vec![0.0; ctx.header.dim],
    };
    let item = match e.item {
        Some(r) => ctx.dvecs.get(r)?,
        None => Vec::new(),
    };
    Ok(Owned {
        q: data::Query { id: e.qid.clone().unwrap_or_default(), text: e.text.clone().unwrap_or_default(), vector: Arc::from(vector) },
        key: e.key.clone().unwrap_or_default(),
        item,
        seen: e.seen.clone(),
    })
}

fn probe<'a>(o: &'a Owned, e: &Entry, paid: bool) -> Probe<'a> {
    Probe { q: &o.q, key: &o.key, row: e.row.unwrap_or(0) as usize, item: &o.item, seen: &o.seen, year: e.year.unwrap_or(2000), paid, knobs: e.knobs.unwrap_or_default() }
}

fn render(ctx: &Ctx, e: &Entry) -> Result<Option<Rendered>, String> {
    if let Some(w) = &e.w {
        return match ctx.target {
            Target::Main(engine) => write_request(ctx, engine, w).map(|r| Some(Rendered::Write(r))),
            Target::More(other) => more_write(ctx, other, w).map(|r| Some(Rendered::MoreWrite(r))),
        };
    }
    let o = owned(ctx, e)?;
    let p = probe(&o, e, ctx.paid);
    let by_search = ctx.key_by_search.load(Ordering::Relaxed);
    let url = ctx.url();
    Ok(match ctx.target {
        Target::Main(engine) => main_request(engine, &e.k, &p, url, ctx.cache_mode, by_search).map(Rendered::Main),
        Target::More(other) => {
            let native = (e.k == "key lookup" && !by_search).then(|| more::native_get(other, &p, url)).flatten();
            native.or_else(|| more::request(other, &e.k, &p, url)).map(Rendered::More)
        }
    })
}

/// An insert's documents: copies of rows of the insert source under the
/// write's keys, the first with the mark in its title, if any.
fn new_docs(ctx: &Ctx, w: &Write) -> Result<Vec<(RawDoc, Vec<f32>, String)>, String> {
    let src = ctx.inserts.as_ref().ok_or("no documents to insert from")?;
    let mut docs = Vec::with_capacity(w.keys.len());
    for (i, (key, row)) in w.keys.iter().zip(&w.rows).enumerate() {
        let (mut r, v) = src.doc(*row)?;
        if i == 0 {
            if let Some(m) = &w.mark {
                r.title = format!("{m} {}", r.title);
            }
        }
        docs.push((r, v, key.clone()));
    }
    Ok(docs)
}

/// A write of the schedule as a one-node engine's requests.
fn more_write(ctx: &Ctx, o: Other, w: &Write) -> Result<Vec<more::Req>, String> {
    let url = ctx.url();
    Ok(match w.op {
        Op::Insert => {
            let docs: Vec<(RawDoc, Vec<f32>)> = new_docs(ctx, w)?
                .into_iter()
                .map(|(mut r, v, k)| {
                    r.id = k;
                    (r, v)
                })
                .collect();
            vec![more::insert_req(o, url, &docs, ctx.avgdl)]
        }
        Op::Update => {
            let row = w.rows[0];
            let doc = if o == Other::Milvus { Some(ctx.docs.as_ref().ok_or("no documents to update from")?.doc(row)?) } else { None };
            vec![more::rating_req(o, url, &w.keys[0], row, w.rating.unwrap_or(0.0), doc.as_ref())?]
        }
        Op::Delete => more::delete_reqs(o, url, &w.keys),
    })
}

/// A write of the schedule as the engine's request.
fn write_request(ctx: &Ctx, engine: Engine, w: &Write) -> Result<WriteReq, String> {
    let url = ctx.url();
    let one = |u: String, body: String| WriteReq::One { url: u, put: false, body: body.into_bytes() };
    match w.op {
        Op::Insert => {
            let docs = new_docs(ctx, w)?;
            Ok(match engine {
                Engine::Strata => {
                    let ops: Vec<Json> = docs.iter().map(|(r, v, k)| json!({"op": "insert", "doc": strata_doc(r, k, v)})).collect();
                    WriteReq::One { url: format!("{url}/v1/write"), put: false, body: serde_json::to_vec(&json!({"ops": ops})).unwrap() }
                }
                Engine::Es | Engine::Os if docs.len() == 1 => {
                    let (r, v, k) = &docs[0];
                    one(format!("{url}/quora/_doc/{}", id_path(k)), engine_doc(engine, r, k, r.rating, v))
                }
                Engine::Es | Engine::Os => {
                    let mut body = String::new();
                    for (r, v, k) in &docs {
                        body += &format!("{}\n{}\n", json!({"index": {"_id": k}}), engine_doc(engine, r, k, r.rating, v));
                    }
                    one(format!("{url}/quora/_bulk"), body)
                }
                Engine::Solr => {
                    let all: Vec<String> = docs.iter().map(|(r, v, k)| engine_doc(engine, r, k, r.rating, v)).collect();
                    one(format!("{url}/solr/quora/update"), format!("[{}]", all.join(",")))
                }
                Engine::Vespa => WriteReq::Many(
                    docs.iter().map(|(r, v, k)| (format!("{url}/document/v1/doc/doc/docid/{}", id_path(k)), format!("{{\"fields\":{}}}", engine_doc(engine, r, k, r.rating, v)).into_bytes())).collect(),
                ),
            })
        }
        Op::Update => {
            let (key, rating) = (&w.keys[0], w.rating.unwrap_or(0.0));
            Ok(match engine {
                Engine::Strata => WriteReq::One {
                    url: format!("{url}/v1/write"),
                    put: false,
                    body: serde_json::to_vec(&json!({"ops": [{"op": "set", "key": key, "field": "rating", "value": rating}]})).unwrap(),
                },
                // A partial update: the attribute changes in memory.
                Engine::Vespa => WriteReq::One {
                    url: format!("{url}/document/v1/doc/doc/docid/{}", id_path(key)),
                    put: true,
                    body: json!({"fields": {"rating": {"assign": rating}}}).to_string().into_bytes(),
                },
                // The whole document again, as `writes` sends it (their partial updates rebuild from a source without body and emb).
                Engine::Es | Engine::Os | Engine::Solr => {
                    let (r, v) = ctx.docs.as_ref().ok_or("no documents to update from")?.doc(w.rows[0])?;
                    let d = engine_doc(engine, &r, key, rating, &v);
                    if engine == Engine::Solr {
                        one(format!("{url}/solr/quora/update"), format!("[{d}]"))
                    } else {
                        one(format!("{url}/quora/_doc/{}", id_path(key)), d)
                    }
                }
            })
        }
        Op::Delete => Ok(match engine {
            Engine::Strata => {
                let ops: Vec<Json> = w.keys.iter().map(|k| json!({"op": "delete", "key": k})).collect();
                WriteReq::One { url: format!("{url}/v1/write"), put: false, body: serde_json::to_vec(&json!({"ops": ops})).unwrap() }
            }
            Engine::Es | Engine::Os => one(format!("{url}/quora/_bulk"), w.keys.iter().map(|k| format!("{}\n", json!({"delete": {"_id": k}}))).collect()),
            Engine::Solr => one(format!("{url}/solr/quora/update"), json!({"delete": w.keys}).to_string()),
            Engine::Vespa => WriteReq::Deletes(w.keys.iter().map(|k| format!("{url}/document/v1/doc/doc/docid/{}", id_path(k))).collect()),
        }),
    }
}

// --- Sending ---------------------------------------------------------------

/// Sends one entry, intended at `intended`, and records it; returns when it was answered.
async fn one(ctx: Arc<Ctx>, e: Entry, intended: Instant) -> Instant {
    let step = if e.warm { None } else { ctx.step_of(e.t) };
    if let Some(i) = step {
        ctx.step_inflight[i].fetch_add(1, Ordering::Relaxed);
    }
    let label = label_of(&e);
    let blocked = ctx.unavailable.lock().unwrap().contains_key(&e.k);
    let rendered = if blocked { Ok(None) } else { render(&ctx, &e) };
    let outcome = match rendered {
        Ok(Some(r)) => {
            let (sent, res) = ctx.slots.run(&r).await;
            Some((sent, Instant::now(), res))
        }
        Ok(None) => None,
        Err(err) => Some((Instant::now(), Instant::now(), Err(err))),
    };
    let done = Instant::now();
    if let Some(i) = step {
        ctx.step_inflight[i].fetch_sub(1, Ordering::Relaxed);
    }
    {
        let mut rec = ctx.rec.lock().unwrap();
        let Some((sent, at, res)) = &outcome else {
            rec.kinds.entry(label).or_default().skipped += 1;
            return done;
        };
        if e.warm {
            rec.warm_done += 1;
            if res.is_err() {
                rec.warm_errors += 1;
            }
            return done;
        }
        let resp_us = at.saturating_duration_since(intended).as_micros() as u64;
        let service_us = at.saturating_duration_since(*sent).as_micros() as u64;
        let is_write = e.w.is_some();
        let k = rec.kinds.entry(label.clone()).or_default();
        match res {
            Ok(_) => k.add(resp_us, service_us),
            Err(err) => {
                k.errors += 1;
                k.first_error.get_or_insert_with(|| err.chars().take(300).collect());
            }
        }
        let wi = ctx.window_of(e.t);
        let w = rec.windows.entry(wi).or_default();
        let kw = w.kinds.entry(label).or_insert((0, 0, None));
        match res {
            Ok(_) => {
                kw.0 += 1;
                kw.2.get_or_insert_with(|| hist(2)).saturating_record(resp_us.max(1));
                if !is_write {
                    w.reads.get_or_insert_with(|| hist(2)).saturating_record(resp_us.max(1));
                }
            }
            Err(_) => {
                kw.1 += 1;
                if !is_write {
                    w.read_errors += 1;
                }
            }
        }
        if let (Some(i), false) = (step, is_write) {
            let st = &mut rec.steps[i];
            match res {
                Ok(_) => {
                    st.completed += 1;
                    if resp_us > ctx.slo_us {
                        st.over_slo += 1;
                    }
                    st.reads.get_or_insert_with(|| hist(2)).saturating_record(resp_us.max(1));
                }
                Err(_) => st.errors += 1,
            }
        }
        if let Some(w) = &e.w {
            if res.is_ok() {
                match w.op {
                    Op::Insert => rec.inserted.extend(w.keys.iter().cloned()),
                    Op::Update => rec.updated.extend(w.rows.iter().copied()),
                    Op::Delete => rec.deleted.extend(w.keys.iter().cloned()),
                }
            }
        }
    }
    // A marked document: how soon a search finds it.
    if let (Some(w), Some((sent, at, Ok(_)))) = (&e.w, &outcome) {
        if let (Some(mark), Some(key)) = (w.mark.clone(), w.keys.first().cloned()) {
            let (ctx, sent, acked) = (ctx.clone(), *sent, *at);
            tokio::spawn(async move {
                let u = ctx.urls[0].clone();
                let mut conns = match ctx.target {
                    Target::More(o) => more::Conns::new(o, Duration::from_secs(30)).ok(),
                    Target::Main(_) => None,
                };
                loop {
                    let hit = match (ctx.target, conns.as_mut()) {
                        (Target::Main(engine), _) => found(engine, &ctx.client, &u, &mark).await,
                        (Target::More(o), Some(c)) => more_found(&ctx, o, c, &u, &mark, &key).await,
                        (Target::More(_), None) => Err("no connection".into()),
                    };
                    if let Ok(true) = hit {
                        let now = Instant::now();
                        ctx.rec.lock().unwrap().visible.push(((now - sent).as_secs_f64() * 1e3, (now - acked).as_secs_f64() * 1e3));
                        break;
                    }
                    if sent.elapsed() > Duration::from_secs(60) {
                        ctx.rec.lock().unwrap().visible_failed += 1;
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            });
        }
    }
    done
}

/// Whether a one-node engine's bm25 search for `word` finds the document `key`.
async fn more_found(ctx: &Ctx, o: Other, c: &mut more::Conns, url: &str, word: &str, key: &str) -> Result<bool, String> {
    let q = data::Query { id: String::new(), text: word.to_string(), vector: Arc::from(vec![0.0; ctx.header.dim]) };
    let p = Probe { q: &q, key: "", row: 0, item: &[], seen: &[], year: 2000, paid: false, knobs: Knobs::default() };
    let r = more::request(o, "bm25", &p, url).ok_or("no bm25 request")?;
    Ok(more::finds(o, &c.exec(&r).await?, key))
}

/// What the dispatcher hands a session: a step it sent at its time, or a
/// follow-up for the session to send a think time after the step before
/// is answered.
enum Step {
    Sent(tokio::task::JoinHandle<Instant>),
    Wait(Entry),
}

/// A session's follow-ups. Each goes a think time after the step before is
/// answered, and its latency counts from when it goes (the user was waiting
/// for the answer before, which the step before's latency already shows).
async fn session(ctx: Arc<Ctx>, mut rx: mpsc::UnboundedReceiver<Step>) {
    let mut prev: Option<tokio::task::JoinHandle<Instant>> = None;
    while let Some(step) = rx.recv().await {
        let e = match step {
            Step::Sent(h) => {
                prev = Some(h);
                continue;
            }
            Step::Wait(e) => e,
        };
        let done = match prev.take() {
            Some(h) => h.await.unwrap_or_else(|_| Instant::now()),
            None => Instant::now(),
        };
        let at = (done + Duration::from_secs_f64(e.think_ms.unwrap_or(0.0) / 1e3)).max(ctx.at(e.t));
        tokio::time::sleep_until(at.into()).await;
        if ctx.stop.load(Ordering::Relaxed) {
            break;
        }
        let woke = Instant::now();
        let c = ctx.clone();
        ctx.inflight.fetch_add(1, Ordering::Relaxed);
        prev = Some(tokio::spawn(async move {
            let done = one(c.clone(), e, woke).await;
            c.inflight.fetch_sub(1, Ordering::Relaxed);
            done
        }));
    }
    if let Some(h) = prev {
        let _ = h.await;
    }
    ctx.sessions.fetch_sub(1, Ordering::Relaxed);
}

/// Waits until `at` on this thread: a sleep, then a spin for its last 200 µs
/// (a timer of the async runtime wakes a millisecond late, and that
/// millisecond would show in every latency).
fn wait_until(at: Instant) {
    loop {
        let now = Instant::now();
        if now >= at {
            return;
        }
        let left = at - now;
        if left > Duration::from_micros(300) {
            std::thread::sleep(left - Duration::from_micros(200));
        } else {
            std::hint::spin_loop();
        }
    }
}

/// Sends the schedule's requests at their times, on a thread of its own
/// (it reads the schedule as it goes). Returns how many it sent.
fn dispatch(ctx: Arc<Ctx>, path: PathBuf, rt: tokio::runtime::Handle) -> Result<u64, String> {
    let mut r = Reader::open(&path)?;
    let mut sessions: HashMap<u64, mpsc::UnboundedSender<Step>> = HashMap::new();
    let mut dispatched = 0u64;
    while let Some(e) = r.next_entry() {
        let e = e?;
        if e.t > ctx.end_t || ctx.stop.load(Ordering::Relaxed) {
            break;
        }
        dispatched += 1;
        if e.s != 0 && e.think_ms.is_some() {
            let (s, last) = (e.s, e.last);
            if let Some(tx) = sessions.get(&s) {
                let _ = tx.send(Step::Wait(e));
            }
            if last {
                sessions.remove(&s);
            }
            continue;
        }
        let at = ctx.at(e.t);
        wait_until(at);
        {
            let mut rec = ctx.rec.lock().unwrap();
            rec.lag.get_or_insert_with(|| hist(3)).saturating_record(Instant::now().saturating_duration_since(at).as_micros() as u64 + 1);
            if !e.warm {
                rec.windows.entry(ctx.window_of(e.t)).or_default().offered += 1;
                if let (Some(i), None) = (ctx.step_of(e.t), &e.w) {
                    rec.steps[i].offered += 1;
                }
            }
        }
        let (s, last) = (e.s, e.last);
        let c = ctx.clone();
        ctx.inflight.fetch_add(1, Ordering::Relaxed);
        let h = rt.spawn(async move {
            let done = one(c.clone(), e, at).await;
            c.inflight.fetch_sub(1, Ordering::Relaxed);
            done
        });
        if s != 0 {
            let tx = sessions.entry(s).or_insert_with(|| {
                let (tx, rx) = mpsc::unbounded_channel();
                ctx.sessions.fetch_add(1, Ordering::Relaxed);
                rt.spawn(session(ctx.clone(), rx));
                tx
            });
            let _ = tx.send(Step::Sent(h));
            if last {
                sessions.remove(&s);
            }
        }
    }
    Ok(dispatched)
}

/// Up to `n` entries of each kind (writes by their label) from the start of the schedule, to check each kind once before timing.
fn samples(path: &Path, n: usize) -> Result<BTreeMap<String, Vec<Entry>>, String> {
    let mut r = Reader::open(path)?;
    let mut out: BTreeMap<String, Vec<Entry>> = BTreeMap::new();
    let mut lines = 0;
    while let Some(e) = r.next_entry() {
        let e = e?;
        lines += 1;
        let v = out.entry(e.k.clone()).or_default();
        if v.len() < n {
            v.push(e);
        }
        if lines > 2_000_000 {
            break;
        }
    }
    Ok(out)
}

/// One request of each kind before the clock starts: kinds the engine
/// cannot answer are left out (and say why); a key lookup whose read by id
/// is refused falls back to a search on the key, as `run` does.
async fn preflight(ctx: &Ctx, path: &Path) -> Result<(), String> {
    let mut exec = match ctx.target {
        Target::Main(_) => Exec::Http(ctx.client.clone()),
        Target::More(o) => Exec::More(more::Conns::new(o, Duration::from_secs(30))?),
    };
    for (kind, list) in samples(path, 20)? {
        if kind == "write" {
            continue;
        }
        if let Some(why) = ctx.target.unsupported(&kind) {
            ctx.unavailable.lock().unwrap().insert(kind.clone(), why.to_string());
            eprintln!("{:<9} {kind:<24} unavailable: {why}", ctx.target.name());
            continue;
        }
        let mut last_err = None;
        let mut ok = false;
        for attempt in 0..2 {
            for e in &list {
                let Some(r) = render(ctx, e)? else { continue };
                let slots = Slots { sem: Semaphore::new(1), pool: Mutex::new(vec![std::mem::replace(&mut exec, Exec::Http(ctx.client.clone()))]) };
                let (_, res) = slots.run(&r).await;
                exec = slots.pool.into_inner().unwrap().pop().unwrap();
                match res {
                    Ok(_) => ok = true,
                    Err(err) => last_err = Some(err),
                }
                break;
            }
            if ok || kind != "key lookup" || attempt == 1 {
                break;
            }
            eprintln!("{:<9} {kind:<24} the read by id failed ({}): the search on the key instead", ctx.target.name(), last_err.clone().unwrap_or_default());
            ctx.key_by_search.store(true, Ordering::Relaxed);
        }
        if !ok {
            let why = last_err.unwrap_or_else(|| "no request of this kind can be made of the schedule's queries".into());
            eprintln!("{:<9} {kind:<24} unavailable: {}", ctx.target.name(), why.chars().take(300).collect::<String>());
            ctx.unavailable.lock().unwrap().insert(kind, why);
        }
    }
    Ok(())
}

fn ms(us: u64) -> f64 {
    us as f64 / 1e3
}

fn quantiles(h: &Option<Histogram<u64>>) -> Json {
    match h {
        Some(h) if !h.is_empty() => json!({
            "n": h.len(), "p50_ms": ms(h.value_at_quantile(0.5)), "p90_ms": ms(h.value_at_quantile(0.9)), "p99_ms": ms(h.value_at_quantile(0.99)),
            "p999_ms": ms(h.value_at_quantile(0.999)), "max_ms": ms(h.max()), "mean_ms": h.mean() / 1e3,
        }),
        _ => json!({"n": 0}),
    }
}

/// Judges each ramp step a little after it ends: it holds the SLO if at
/// most 1% of its reads (those still in flight counted as late) took longer
/// than the SLO and its errors stay under the SLO's share. Two misses in a
/// row stop the replay.
async fn judge_steps(ctx: Arc<Ctx>, verdicts: Arc<Mutex<Vec<Json>>>) {
    let grace = Duration::from_secs_f64((ctx.slo_us as f64 / 1e6 * 5.0).max(2.0));
    let mut misses = 0;
    for (i, step) in ctx.header.steps.iter().enumerate() {
        let end = ctx.at(ctx.header.warmup_s + step.to_s);
        tokio::time::sleep_until((end + grace).into()).await;
        let late = ctx.step_inflight[i].load(Ordering::Relaxed).max(0) as u64;
        let st = ctx.rec.lock().unwrap().steps[i].clone();
        let total = st.completed + st.errors + late;
        let over = st.over_slo + late;
        let slo = &ctx.header.profile.slo;
        let held = total > 0 && over as f64 <= 0.01 * total as f64 && st.errors as f64 <= slo.errors * total as f64;
        misses = if held { 0 } else { misses + 1 };
        let q = quantiles(&st.reads);
        eprintln!(
            "step {i:>2}  {:>8.1} req/s planned  {:>8.1} answered/s  p99 {:>8.2} ms  late {late}  errors {}  {}",
            step.rate,
            (st.completed + st.errors) as f64 / (step.to_s - step.from_s),
            q["p99_ms"].as_f64().unwrap_or(0.0),
            st.errors,
            if held { "holds" } else { "misses the SLO" }
        );
        verdicts.lock().unwrap().push(json!({
            "step": i, "rate": step.rate, "offered": st.offered, "answered": st.completed, "errors": st.errors, "late": late,
            "over_slo": st.over_slo, "answered_per_s": (st.completed + st.errors) as f64 / (step.to_s - step.from_s), "reads": q, "holds": held,
        }));
        if misses >= 2 {
            eprintln!("two steps in a row missed the SLO: stopping");
            ctx.stop.store(true, Ordering::Relaxed);
            break;
        }
        if ctx.stop.load(Ordering::Relaxed) {
            break;
        }
    }
}

// --- The replay ------------------------------------------------------------

pub async fn replay(collection: &str, target: Target, urls: &[String], schedule: &Path, opts: &Opts) -> Result<(), String> {
    replay_in(&data::data_dir().join(collection), collection, target, urls, schedule, opts).await
}

/// `replay` of the collection in `dir`.
pub async fn replay_in(dir: &Path, collection: &str, target: Target, urls: &[String], schedule: &Path, opts: &Opts) -> Result<(), String> {
    let r = Reader::open(schedule)?;
    let header = r.header.clone();
    drop(r);
    if header.collection != collection {
        eprintln!("note: the schedule was planned on {}, replayed on {collection}", header.collection);
    }
    let dir = dir.to_path_buf();
    let meta: crate::Meta = serde_json::from_reader(File::open(dir.join("meta.json")).map_err(|e| format!("{}: {e}", dir.join("meta.json").display()))?).map_err(s)?;
    if meta.docs != header.docs {
        return Err(format!("the schedule was planned on {} documents; {collection} has {}", header.docs, meta.docs));
    }
    // The schedule carries each query's text; its vector is read from the source's file.
    let qvecs = match header.source {
        crate::sim::profile::Source::Collection => dir.join("query_vecs.f32"),
        crate::sim::profile::Source::Pool => dir.join("pool_vecs.f32"),
    };
    let label = opts.label.clone().unwrap_or_else(|| target.name().to_string());
    let engine_client = match target {
        Target::Main(e) => http_client(e, 30)?,
        Target::More(_) => reqwest::Client::builder().timeout(Duration::from_secs(30)).build().map_err(s)?,
    };
    let has_writes = header.profile.writes.share > 0.0 || !header.profile.writes.jobs.is_empty();
    let (docs, inserts) = if has_writes {
        eprintln!("indexing the documents for writes ...");
        let docs = DocRows::open(&dir.join("docs.jsonl"), &dir.join("doc_vecs.f32"), header.dim)?;
        let inserts = if header.inserts_from == "heldout" { DocRows::open(&dir.join("heldout.jsonl"), &dir.join("heldout_vecs.f32"), header.dim)? } else { DocRows::open(&dir.join("docs.jsonl"), &dir.join("doc_vecs.f32"), header.dim)? };
        (Some(docs), Some(inserts))
    } else {
        (None, None)
    };
    let avgdl = if has_writes && matches!(target, Target::More(Other::Qdrant)) {
        eprintln!("qdrant: the collection's average body length, for new documents' BM25 weights ...");
        more::avgdl(collection)?
    } else {
        1.0
    };
    let pg_like = matches!(target, Target::More(Other::Postgres | Other::Paradedb | Other::Redis));
    let n_slots = opts.max_inflight.unwrap_or(if pg_like { 64 } else { 512 }).max(1);
    let pool: Vec<Exec> = match target {
        Target::Main(_) => (0..n_slots).map(|_| Exec::Http(engine_client.clone())).collect(),
        Target::More(o) => {
            let c = more::Conns::new(o, Duration::from_secs(30))?;
            (0..n_slots).map(|_| Exec::More(c.fork())).collect()
        }
    };
    let steps = header.steps.len();
    let mut ctx = Ctx {
        target,
        header: header.clone(),
        urls: urls.to_vec(),
        next_url: AtomicUsize::new(0),
        qvecs: Vectors::open(&qvecs, header.dim)?,
        dvecs: Vectors::open(&dir.join("doc_vecs.f32"), header.dim)?,
        docs,
        inserts,
        avgdl,
        client: engine_client.clone(),
        cache_mode: opts.cache_mode,
        paid: opts.paid,
        key_by_search: AtomicBool::new(false),
        unavailable: Mutex::new(BTreeMap::new()),
        slots: Slots { sem: Semaphore::new(n_slots), pool: Mutex::new(pool) },
        rec: Mutex::new(Rec { steps: vec![StepRec::default(); steps], ..Rec::default() }),
        start: Instant::now(),
        stop: AtomicBool::new(false),
        inflight: AtomicUsize::new(0),
        sessions: AtomicUsize::new(0),
        step_inflight: (0..steps).map(|_| AtomicI64::new(0)).collect(),
        slo_us: (header.profile.slo.p99_ms * 1e3) as u64,
        end_t: opts.seconds.map_or(f64::INFINITY, |s| header.warmup_s + s),
    };
    eprintln!("{:<9} checking each kind once ...", target.name());
    preflight(&ctx, schedule).await?;
    let mut nrt = None;
    if has_writes {
        if let Target::Main(e) = target {
            nrt = Some(searchable_soon(e, &engine_client, &urls[0]).await?);
        }
    }
    std::fs::create_dir_all(&opts.out).map_err(s)?;

    if header.shape == Shape::Closed {
        return closed(&ctx, schedule, &label, opts).await;
    }

    // The clock starts a second from now, so that the reader is ahead.
    ctx.start = Instant::now() + Duration::from_secs(1);
    let ctx = Arc::new(ctx);
    let verdicts = Arc::new(Mutex::new(Vec::new()));
    let judge = (header.shape == Shape::Ramp).then(|| tokio::spawn(judge_steps(ctx.clone(), verdicts.clone())));
    let progress = {
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let mut last = 0u64;
            loop {
                tokio::time::sleep(Duration::from_secs(10)).await;
                let (done, p99) = {
                    let rec = ctx.rec.lock().unwrap();
                    let done: u64 = rec.kinds.values().map(|k| k.resp.as_ref().map_or(0, |h| h.len()) + k.errors).sum::<u64>() + rec.warm_done;
                    let p99 = rec.windows.values().last().and_then(|w| w.reads.as_ref()).map_or(0.0, |h| ms(h.value_at_quantile(0.99)));
                    (done, p99)
                };
                let t = ctx.start.elapsed().as_secs_f64();
                eprintln!(
                    "t {t:>7.0} s  answered {:>8.1}/s  window p99 {p99:>8.2} ms  in flight {}  sessions {}",
                    (done - last) as f64 / 10.0,
                    ctx.inflight.load(Ordering::Relaxed),
                    ctx.sessions.load(Ordering::Relaxed)
                );
                last = done;
            }
        })
    };
    let t0 = Instant::now();
    let rt = tokio::runtime::Handle::current();
    let dispatched = {
        let (ctx, path) = (ctx.clone(), schedule.to_path_buf());
        tokio::task::spawn_blocking(move || dispatch(ctx, path, rt)).await.map_err(s)??
    };
    // What is still in flight, two minutes at most.
    let deadline = Instant::now() + Duration::from_secs(120);
    while (ctx.inflight.load(Ordering::Relaxed) > 0 || ctx.sessions.load(Ordering::Relaxed) > 0) && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let left = ctx.inflight.load(Ordering::Relaxed);
    ctx.stop.store(true, Ordering::Relaxed);
    if let Some(j) = judge {
        j.abort();
    }
    progress.abort();
    let wall = t0.elapsed().as_secs_f64();
    // Freshness probes still looking get a few seconds more.
    tokio::time::sleep(Duration::from_secs(2)).await;

    let report = {
        let rec = ctx.rec.lock().unwrap();
        let mut kinds = serde_json::Map::new();
        for (k, r) in &rec.kinds {
            kinds.insert(
                k.clone(),
                json!({"response": quantiles(&r.resp), "service": quantiles(&r.service), "errors": r.errors, "first_error": r.first_error, "skipped": r.skipped}),
            );
        }
        let mut reads_all = hist(3);
        for (k, r) in &rec.kinds {
            if !k.starts_with("write") {
                if let Some(h) = &r.resp {
                    reads_all.add(h).unwrap();
                }
            }
        }
        let slo = &header.profile.slo;
        let mut windows = Vec::new();
        let (mut held, mut judged) = (0, 0);
        for (w, r) in &rec.windows {
            let q = quantiles(&r.reads);
            let n = r.reads.as_ref().map_or(0, |h| h.len()) + r.read_errors;
            let ok = n > 0 && q["p99_ms"].as_f64().unwrap_or(f64::MAX) <= slo.p99_ms && r.read_errors as f64 <= slo.errors * n as f64;
            if n > 0 {
                judged += 1;
                held += ok as u64;
            }
            let kinds: serde_json::Map<String, Json> = r
                .kinds
                .iter()
                .map(|(k, (n, e, h))| (k.clone(), json!({"n": n, "errors": e, "p50_ms": h.as_ref().map(|h| ms(h.value_at_quantile(0.5))), "p99_ms": h.as_ref().map(|h| ms(h.value_at_quantile(0.99)))})))
                .collect();
            windows.push(json!({"t_s": *w as f64 * WINDOW_S, "offered": r.offered, "reads": q, "read_errors": r.read_errors, "holds_slo": ok, "kinds": kinds}));
        }
        let verdicts = verdicts.lock().unwrap().clone();
        let capacity = {
            let mut best = None;
            for v in &verdicts {
                if v["holds"].as_bool() == Some(true) {
                    best = v["rate"].as_f64();
                }
            }
            best
        };
        let mut vis: Vec<f64> = rec.visible.iter().map(|v| v.0).collect();
        let mut vis_ack: Vec<f64> = rec.visible.iter().map(|v| v.1).collect();
        vis.sort_by(f64::total_cmp);
        vis_ack.sort_by(f64::total_cmp);
        let lag = rec.lag.as_ref().map(|h| json!({"p50_ms": ms(h.value_at_quantile(0.5)), "p99_ms": ms(h.value_at_quantile(0.99)), "max_ms": ms(h.max())}));
        let unavailable = ctx.unavailable.lock().unwrap().clone();
        let differ: BTreeMap<&str, &str> = rec.kinds.keys().filter_map(|k| target.differs(k, opts.paid).map(|d| (k.as_str(), d))).collect();
        json!({
            "engine": target.name(), "label": label, "collection": collection, "urls": urls,
            "schedule": {
                "path": schedule.display().to_string(), "sha256": schedule_hash(schedule)?, "profile": header.profile.name, "profile_sha256": header.profile_sha256,
                "seed": header.seed, "shape": header.shape, "warmup_s": header.warmup_s, "end_s": header.end_s, "steps": header.steps, "slo": header.profile.slo,
            },
            "cache_mode": opts.cache_mode.name(),
            "params": {"max_inflight": n_slots, "paid": opts.paid, "seconds": opts.seconds, "window_s": WINDOW_S, "searchable": nrt, "key_lookup_by_search": ctx.key_by_search.load(Ordering::Relaxed)},
            "dispatched": dispatched, "wall_s": wall, "left_in_flight": left, "warm_up": {"answered": rec.warm_done, "errors": rec.warm_errors},
            "driver_lag": lag,
            "reads": quantiles(&Some(reads_all)),
            "kinds": kinds,
            "slo_windows": {"held": held, "of": judged},
            "steps": verdicts, "capacity_at_slo": capacity,
            "freshness": {
                "samples": vis.len(), "not_found_in_60s": rec.visible_failed,
                "visible_p50_ms": (!vis.is_empty()).then(|| percentile(&vis, 50.0)), "visible_p99_ms": (!vis.is_empty()).then(|| percentile(&vis, 99.0)),
                "visible_max_ms": vis.last(), "after_ack_p50_ms": (!vis_ack.is_empty()).then(|| percentile(&vis_ack, 50.0)),
                "visible_mean_ms": (!vis.is_empty()).then(|| mean(&vis)),
            },
            "unavailable": unavailable, "differs": differ,
            "windows": windows,
        })
    };
    let mut report = report;
    let path = opts.out.join(format!("replay-{}.json", label.replace(' ', "-")));
    std::fs::write(&path, serde_json::to_string(&report).unwrap()).map_err(s)?;
    let reads = &report["reads"];
    eprintln!(
        "{label}: {} reads, p50 {:.2} ms p99 {:.2} ms p99.9 {:.2} ms; SLO held in {} of {} windows{}; driver lag p99 {:.2} ms",
        reads["n"],
        reads["p50_ms"].as_f64().unwrap_or(0.0),
        reads["p99_ms"].as_f64().unwrap_or(0.0),
        reads["p999_ms"].as_f64().unwrap_or(0.0),
        report["slo_windows"]["held"],
        report["slo_windows"]["of"],
        report["capacity_at_slo"].as_f64().map_or(String::new(), |c| format!("; capacity at the SLO {c:.1} requests a second")),
        report["driver_lag"]["p99_ms"].as_f64().unwrap_or(0.0)
    );
    if report["driver_lag"]["p99_ms"].as_f64().unwrap_or(0.0) > 5.0 {
        eprintln!("warning: the driver sent 1% of its requests more than 5 ms late: give it more CPU, or replay at a lower rate");
    }
    eprintln!("wrote {}", path.display());

    // Put the index back: delete what the schedule wrote, give changed documents their ratings again.
    if opts.reset {
        if let Some(docs) = &ctx.docs {
            // What the schedule inserted and has not deleted itself.
            let (keys, rows) = {
                let rec = ctx.rec.lock().unwrap();
                (rec.inserted.iter().filter(|k| !rec.deleted.contains(*k)).cloned().collect::<Vec<_>>(), rec.updated.iter().copied().collect::<Vec<_>>())
            };
            let originals: Vec<(RawDoc, Vec<f32>)> = rows.iter().map(|&r| docs.doc(r)).collect::<Result<_, _>>()?;
            let t = Instant::now();
            let after = match target {
                Target::Main(engine) => {
                    restore(engine, &engine_client, &urls[0], &keys, &originals.iter().collect::<Vec<_>>()).await?;
                    None
                }
                Target::More(o) => {
                    more_restore(o, &urls[0], &keys, &rows, &originals).await?;
                    more::searchable_docs(o, &urls[0], header.docs as u64).await?
                }
            };
            let reset_s = t.elapsed().as_secs_f64();
            eprintln!("{label}: deleted {} written documents and put back {} ratings in {reset_s:.1} s", keys.len(), originals.len());
            if let Some(n) = after.filter(|&n| n != header.docs as u64) {
                eprintln!("warning: {n} documents searchable after the reset, {} loaded", header.docs);
            }
            report["reset"] = json!({"deleted": keys.len(), "ratings": originals.len(), "seconds": reset_s, "searchable_docs_after": after});
            std::fs::write(&path, serde_json::to_string(&report).unwrap()).map_err(s)?;
        }
    }
    Ok(())
}

/// A one-node engine put back: the documents a replay inserted deleted (500
/// a request), the ratings it changed written back, 16 requests at a time.
async fn more_restore(o: Other, url: &str, keys: &[String], rows: &[u64], originals: &[(RawDoc, Vec<f32>)]) -> Result<(), String> {
    let mut reqs: Vec<Vec<more::Req>> = keys.chunks(500).map(|c| more::delete_reqs(o, url, c)).collect();
    for (row, doc) in rows.iter().zip(originals) {
        reqs.push(vec![more::rating_req(o, url, &doc.0.id, *row, doc.0.rating, Some(doc))?]);
    }
    let conns = more::Conns::new(o, Duration::from_secs(300))?;
    let queue = Arc::new(Mutex::new(reqs));
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let (queue, mut c) = (queue.clone(), conns.fork());
        tasks.push(tokio::spawn(async move {
            loop {
                let Some(batch) = queue.lock().unwrap().pop() else { return Ok::<(), String>(()) };
                for r in &batch {
                    c.exec(r).await.map_err(|e| format!("putting the index back: {e}"))?;
                }
            }
        }));
    }
    for t in tasks {
        t.await.map_err(s)??;
    }
    Ok(())
}

fn schedule_hash(path: &Path) -> Result<String, String> {
    let mut r = Reader::open(path)?;
    while let Some(e) = r.next_entry() {
        e?;
    }
    Ok(r.hash())
}

/// The closed shape: each kind's requests in turn, as `run` sends them:
/// a warm-up, then each concurrency for the profile's seconds.
async fn closed(ctx: &Ctx, schedule: &Path, label: &str, opts: &Opts) -> Result<(), String> {
    let closed = &ctx.header.profile.traffic.closed;
    let mut by_kind: BTreeMap<usize, (String, Vec<Entry>)> = BTreeMap::new();
    let mut r = Reader::open(schedule)?;
    while let Some(e) = r.next_entry() {
        let e = e?;
        let i = KINDS.iter().position(|(k, _)| *k == e.k).unwrap_or(usize::MAX);
        by_kind.entry(i).or_insert_with(|| (e.k.clone(), Vec::new())).1.push(e);
    }
    let mut http = Vec::new();
    let mut warmups = BTreeMap::new();
    let mut skipped = BTreeMap::new();
    for (_, (kind, entries)) in by_kind {
        if ctx.unavailable.lock().unwrap().contains_key(&kind) {
            continue;
        }
        let mut rendered = Vec::new();
        for e in &entries {
            match render(ctx, e)? {
                Some(r) => rendered.push(r),
                None => *skipped.entry(kind.clone()).or_insert(0usize) += 1,
            }
        }
        if rendered.is_empty() {
            continue;
        }
        let round = |c: usize, secs: f64| {
            let rendered = &rendered;
            async move {
                match ctx.target {
                    Target::Main(_) => {
                        let reqs: Vec<crate::Req> = rendered.iter().filter_map(|r| if let Rendered::Main(q) = r { Some(crate::Req { url: q.url.clone(), body: q.body.clone(), get: q.get }) } else { None }).collect();
                        crate::drive(&ctx.client, Arc::new(reqs), c, secs).await
                    }
                    Target::More(o) => {
                        let reqs: Vec<more::Req> = rendered.iter().filter_map(|r| if let Rendered::More(q) = r { Some(q.clone()) } else { None }).collect();
                        let conns = more::Conns::new(o, Duration::from_secs(30)).expect("connections");
                        more::drive(&conns, Arc::new(reqs), c, secs).await
                    }
                }
            }
        };
        let warm = warm_up(closed.seconds, round).await;
        eprintln!("{label:<9} {kind:<24} warm-up: p50 {} ms", warm.iter().map(|x| format!("{x:.3}")).collect::<Vec<_>>().join(", "));
        warmups.insert(kind.clone(), warm);
        for &c in &closed.concurrency {
            let t: Timed = round(c, closed.seconds).await;
            eprintln!(
                "{label:<9} {kind:<24} c={c:<3} {:>8.0} q/s  p50 {:>7.3} ms  p90 {:>7.3} ms  p99 {:>7.3} ms  errors {}",
                t.ms.len() as f64 / t.seconds,
                percentile(&t.ms, 50.0),
                percentile(&t.ms, 90.0),
                percentile(&t.ms, 99.0),
                t.errors
            );
            http.push(json!({
                "kind": kind, "concurrency": c, "requests": t.ms.len(), "qps": t.ms.len() as f64 / t.seconds.max(1e-9),
                "p50_ms": percentile(&t.ms, 50.0), "p90_ms": percentile(&t.ms, 90.0), "p95_ms": percentile(&t.ms, 95.0),
                "p99_ms": percentile(&t.ms, 99.0), "p999_ms": percentile(&t.ms, 99.9), "max_ms": t.ms.iter().cloned().fold(0.0, f64::max),
                "mean_ms": mean(&t.ms), "errors": t.errors, "first_error": t.first_error,
            }));
        }
    }
    let report = json!({
        "engine": ctx.target.name(), "label": label, "collection": ctx.header.collection, "urls": ctx.urls,
        "schedule": {"path": schedule.display().to_string(), "sha256": schedule_hash(schedule)?, "profile": ctx.header.profile.name, "seed": ctx.header.seed, "shape": ctx.header.shape},
        "cache_mode": opts.cache_mode.name(),
        "params": {"seconds": closed.seconds, "concurrency": closed.concurrency, "key_lookup_by_search": ctx.key_by_search.load(Ordering::Relaxed)},
        "http": http, "warm_up_p50_ms": warmups, "skipped": skipped, "unavailable": *ctx.unavailable.lock().unwrap(),
    });
    let path = opts.out.join(format!("replay-{}.json", label.replace(' ', "-")));
    std::fs::write(&path, serde_json::to_string(&report).unwrap()).map_err(s)?;
    eprintln!("wrote {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_mode_drops_the_request_cache_switch() {
        assert_eq!(shipped_path("/quora/_search?request_cache=false"), "/quora/_search");
        assert_eq!(shipped_path("/quora/_search?request_cache=false&search_pipeline=rrf"), "/quora/_search?search_pipeline=rrf");
        assert_eq!(shipped_path("/v1/query"), "/v1/query");
    }

    /// An HTTP server that answers every request with `{}` after `service`,
    /// one request at a time (a single server: M/D/1 under Poisson arrivals).
    fn one_at_a_time(service: Duration) -> String {
        use std::io::{Read as _, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let busy = Arc::new(Mutex::new(()));
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let (mut conn, busy) = (conn.unwrap(), busy.clone());
                std::thread::spawn(move || {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 65536];
                    loop {
                        // A request: headers, then content-length bytes of body.
                        let end = loop {
                            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                break i + 4;
                            }
                            match conn.read(&mut chunk) {
                                Ok(0) | Err(_) => return,
                                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                            }
                        };
                        let head = String::from_utf8_lossy(&buf[..end]).to_lowercase();
                        let len: usize = head.lines().find_map(|l| l.strip_prefix("content-length:")).map_or(0, |v| v.trim().parse().unwrap_or(0));
                        while buf.len() < end + len {
                            match conn.read(&mut chunk) {
                                Ok(0) | Err(_) => return,
                                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                            }
                        }
                        buf.drain(..end + len);
                        {
                            let _one = busy.lock().unwrap();
                            let t = Instant::now();
                            while t.elapsed() < service {
                                std::hint::spin_loop();
                            }
                        }
                        if conn.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 2\r\n\r\n{}").is_err() {
                            return;
                        }
                    }
                });
            }
        });
        format!("http://{addr}")
    }

    /// Open-loop arrivals at a single server queue as M/D/1 does: at 75% of
    /// its capacity the mean response is the service time and a wait of
    /// rho D / (2 (1 - rho)) = 1.5 D; a closed loop would show D alone.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn an_open_loop_queues_as_m_d_1() {
        let dir = crate::sim::fixture::write(&crate::sim::fixture::tempdir("queue"), 2000, 500, 0);
        let p = crate::sim::profile::Profile::parse(
            "name = \"queue\"\n[mix]\nbm25 = 1\n[queries]\nhead = 200\ntail_share = 0.0\n[traffic]\nday_s = 20.0\nwarmup_s = 2.0\n[slo]\np99_ms = 1000.0\n",
        )
        .unwrap();
        let catalog = crate::sim::gen::Catalog::load_dir("queue", &dir).unwrap();
        let schedule = dir.join("schedule.jsonl.gz");
        let d = Duration::from_millis(4);
        let rate = 0.75 / d.as_secs_f64();
        crate::sim::gen::plan(&p, "test", &catalog, 1, Shape::Day { rate }, &schedule).unwrap();
        let url = one_at_a_time(d);
        let out = dir.join("out");
        let opts = Opts { max_inflight: None, cache_mode: CacheMode::Ranked, paid: false, seconds: None, label: Some("queue".into()), reset: false, out: out.clone() };
        replay_in(&dir, "queue", Target::Main(Engine::Strata), &[url], &schedule, &opts).await.unwrap();
        let r: Json = serde_json::from_str(&std::fs::read_to_string(out.join("replay-queue.json")).unwrap()).unwrap();
        let mean = r["reads"]["mean_ms"].as_f64().unwrap();
        let svc = r["kinds"]["bm25"]["service"]["mean_ms"].as_f64().unwrap();
        // 4 ms of service and 6 ms of waiting; the client's own time and the scheduler's on top, a little.
        assert!((8.0..13.0).contains(&mean), "mean response {mean} ms (service {svc} ms), M/D/1 says 10");
        // The queue is the server's (every request has its connection), so the time from sending shows it too.
        assert!(svc > 8.0, "service {svc} ms");
        assert!(r["driver_lag"]["p99_ms"].as_f64().unwrap() < 2.0, "{}", r["driver_lag"]);
        assert_eq!(r["left_in_flight"], 0);
    }

    /// The Today profile's closed schedule makes run's requests, byte for
    /// byte, on all 14 engines: the same queries, keys, items, seen keys,
    /// years and filters.
    #[test]
    fn today_makes_runs_requests_on_every_engine() {
        let dir = crate::sim::fixture::write(&crate::sim::fixture::tempdir("today"), 3000, 400, 0);
        let profile = Path::new(env!("CARGO_MANIFEST_DIR")).join("deploy/compare/profiles/today.toml");
        let (p, sha) = crate::sim::read_profile(&profile).unwrap();
        let catalog = crate::sim::gen::Catalog::load_dir("today", &dir).unwrap();
        let schedule = dir.join("today.jsonl.gz");
        crate::sim::gen::plan(&p, &sha, &catalog, 1, Shape::Closed, &schedule).unwrap();
        let dim = crate::sim::fixture::DIM;
        let qv = Vectors::open(&dir.join("query_vecs.f32"), dim).unwrap();
        let dv = Vectors::open(&dir.join("doc_vecs.f32"), dim).unwrap();
        // run's ingredients: the first 300 queries with text, the keys and vectors of the first 20,000 documents.
        let mut queries = Vec::new();
        for (row, line) in std::fs::read_to_string(dir.join("queries.jsonl")).unwrap().lines().enumerate() {
            let j: Json = serde_json::from_str(line).unwrap();
            let text = j["text"].as_str().unwrap().to_string();
            if !text.trim().is_empty() && queries.len() < 300 {
                queries.push(data::Query { id: j["id"].as_str().unwrap().into(), text, vector: Arc::from(qv.get(row as u64).unwrap()) });
            }
        }
        let n = catalog.docs().min(20_000);
        let keys: Vec<String> = (0..n).map(|i| catalog.ids.get(i).to_string()).collect();
        let items: Vec<Vec<f32>> = (0..n).map(|i| dv.get(i as u64).unwrap()).collect();
        let mut per_kind: BTreeMap<String, usize> = BTreeMap::new();
        let mut r = Reader::open(&schedule).unwrap();
        let mut compared = 0;
        while let Some(e) = r.next_entry() {
            let e = e.unwrap();
            let i = *per_kind.entry(e.k.clone()).and_modify(|x| *x += 1).or_insert(0);
            let (k, it) = ((i * 7919) % n, (i * 104_729 + 13) % n);
            let seen: Vec<String> = std::iter::once(it).chain((1..crate::SEEN).map(|j| (it + j * 997) % n)).map(|j| keys[j].clone()).collect();
            let run_probe = Probe { q: &queries[i], key: &keys[k], row: k, item: &items[it], seen: &seen, year: 1990 + (i % 30) as i64, paid: false, knobs: Knobs::default() };
            let o = Owned {
                q: data::Query { id: e.qid.clone().unwrap(), text: e.text.clone().unwrap(), vector: Arc::from(qv.get(e.q.unwrap()).unwrap()) },
                key: e.key.clone().unwrap_or_default(),
                item: e.item.map_or_else(Vec::new, |r| dv.get(r).unwrap()),
                seen: e.seen.clone(),
            };
            let mine = probe(&o, &e, false);
            let sql = KINDS.iter().find(|(x, _)| *x == e.k).unwrap().1;
            let used = |p: &Probe| e.k != "key lookup" || p.key == run_probe.key;
            assert!(used(&mine), "key lookup {i}: key {} where run's is {}", mine.key, run_probe.key);
            for engine in [Engine::Strata, Engine::Es, Engine::Os, Engine::Solr, Engine::Vespa] {
                assert_eq!(request(engine, &e.k, sql, &mine), request(engine, &e.k, sql, &run_probe), "{} {} {i}", engine.name(), e.k);
            }
            for other in [Other::Meili, Other::Typesense, Other::Manticore, Other::Qdrant, Other::Weaviate, Other::Milvus, Other::Paradedb, Other::Postgres, Other::Redis] {
                let a = more::request(other, &e.k, &mine, "http://x").map(|r| crate::snapshot::more_req(&r));
                let b = more::request(other, &e.k, &run_probe, "http://x").map(|r| crate::snapshot::more_req(&r));
                assert_eq!(a, b, "{} {} {i}", other.name(), e.k);
                if e.k == "key lookup" {
                    let a = more::native_get(other, &mine, "http://x").map(|r| crate::snapshot::more_req(&r));
                    let b = more::native_get(other, &run_probe, "http://x").map(|r| crate::snapshot::more_req(&r));
                    assert_eq!(a, b, "{} native get {i}", other.name());
                }
            }
            if e.k == "key lookup" {
                for engine in [Engine::Es, Engine::Os, Engine::Solr, Engine::Vespa] {
                    assert_eq!(native_get(engine, mine.key), native_get(engine, run_probe.key), "{} native get {i}", engine.name());
                }
            }
            compared += 1;
        }
        assert_eq!(compared, 14 * 300);
    }

    #[test]
    fn knobs_default_when_absent() {
        let e = Entry { k: "bm25 + filter".into(), ..Entry::default() };
        assert_eq!(e.knobs.unwrap_or_default(), Knobs::default());
    }
}
