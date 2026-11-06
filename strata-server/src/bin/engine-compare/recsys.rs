//! A recommender's requests (kinds r1 to r6) on Strata, Elasticsearch,
//! OpenSearch, Vespa, Qdrant, Milvus, Redis and Postgres (pgvector), over
//! the serving data `strata/results/recsys/serving_data.py` makes from
//! Yambda-50M: tracks with a learned vector (64 numbers), an artist, the
//! regions they may be played in, a popularity and twelve ranking
//! features; users with their vector (the sequence model's, computed
//! upstream as a two-tower system's user tower is), region, last 500
//! tracks and last 5; and a LightGBM ranker over the features and the
//! user-track dot product.
//!
//! ```text
//! engine-compare recsys-load --engine qdrant --url http://localhost:6333 --data ~/data/recsys-100k --out /data/recsys
//! engine-compare recsys-run  --engine qdrant --url http://localhost:6333 --data ~/data/recsys-100k --out /data/recsys
//! ```
//!
//! The kinds, each for a user (`--users` of them, their requests sent in turn):
//!
//! - **r1 user to items**: the 500 tracks nearest the user's vector that
//!   may be played in their region, without the (up to) 500 they played.
//! - **r2 candidates (RRF)**: three sources under r1's filters, each 1,000
//!   deep, fused by reciprocal rank (k 60): nearest the user's vector,
//!   nearest their last track's vector, and the most popular; the best
//!   1,000.
//! - **r3 features**: the twelve features of 1,000 tracks by key (the
//!   user's exact top 1,000 of r1's filters).
//! - **r4 GBDT rerank**: those 1,000 ranked by the LightGBM model (300
//!   trees) over the twelve features and the dot product; the best 50.
//! - **r5 diversify**: r1's ranking with at most 2 tracks an artist; the
//!   best 20.
//! - **r6 events while serving**: r1 while plays are written to an events
//!   table at `--event-rate` a second, each a write of its own, and how
//!   soon a written play is in a query of the user's last 5 plays.
//!
//! Each kind is the engine's closest native request. Where an engine has
//! none, the client does that part (two requests fused, features scored,
//! artists capped) and the report says so (`differs`). `run` sends each
//! kind's requests once and scores the answers against exact ones the
//! harness computes from the same data, then times them closed-loop at
//! each `--concurrency` for `--seconds`, after a warm-up.
//!
//! Vespa's application package (`deploy/compare/vespa-recsys`, with the
//! model) is deployed before `recsys-load`; Strata's server starts with
//! `deploy/compare/recsys/strata-items.json` as its schema and the data
//! directory as its models directory (the ranker is `ranker/gbdt.json`).

use super::more::{self, Conns, Other};
use super::*;
use reqwest::Method;
use std::collections::{HashMap, HashSet};
use strata::query::model::Gbdt;

type R = more::Req;

/// The tracks r1 asks for, and the HNSW candidates every engine searches with for them.
const R1_K: usize = 500;
const R1_EF: usize = 1000;
/// Each r2 source's depth, the fused answer's length and RRF's k.
const R2_DEPTH: usize = 1000;
const R2_K: usize = 1000;
const RRF_K: f64 = 60.0;
/// r3's and r4's candidates, and r4's answer.
const CANDS: usize = 1000;
const R4_TOP: usize = 50;
/// r5: the nearest tracks the engines that need a depth group (Redis caps them in the client), and the answer.
const R5_DEPTH: usize = 200;
const R5_K: usize = 20;
const R5_PER: usize = 2;
/// The twelve features the model reads before the dot product (`f12`).
const FEATURES: [&str; 12] = ["f00", "f01", "f02", "f03", "f04", "f05", "f06", "f07", "f08", "f09", "f10", "f11"];

pub(crate) const KINDS: &[(&str, &str)] = &[
    ("r1", "user to items"),
    ("r2", "candidates (RRF)"),
    ("r3", "features"),
    ("r4", "GBDT rerank"),
    ("r5", "diversify"),
    ("r6", "events while serving"),
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Eng {
    Strata,
    Es,
    Os,
    Vespa,
    Qdrant,
    Milvus,
    Redis,
    Postgres,
}

impl Eng {
    pub(crate) fn of(w: Which) -> Result<Eng, String> {
        Ok(match w {
            Which::Strata => Eng::Strata,
            Which::Es => Eng::Es,
            Which::Os => Eng::Os,
            Which::Vespa => Eng::Vespa,
            Which::Qdrant => Eng::Qdrant,
            Which::Milvus => Eng::Milvus,
            Which::Redis => Eng::Redis,
            Which::Postgres | Which::Paradedb => Eng::Postgres,
            _ => return Err("the recommender kinds run on strata, es, os, vespa, qdrant, milvus, redis and postgres".into()),
        })
    }

    fn name(self) -> &'static str {
        match self {
            Eng::Strata => "strata",
            Eng::Es => "es",
            Eng::Os => "os",
            Eng::Vespa => "vespa",
            Eng::Qdrant => "qdrant",
            Eng::Milvus => "milvus",
            Eng::Redis => "redis",
            Eng::Postgres => "postgres",
        }
    }
}

/// What a kind means on an engine, where it is not one request that does it all.
fn differs(e: Eng, kind: &str) -> Option<&'static str> {
    Some(match (e, kind) {
        (Eng::Vespa, "r2") => "three queries at once (two nearestNeighbor, one of the region ordered by pop), fused in the client: range(pop)'s hitLimit applies before the filters, and closeness is computed for every document a query matches, so one query can't keep the region's most popular",
        (Eng::Milvus, "r2") => "the two vector searches sent at once and fused in the client with the region's 1,000 most popular tracks, which the client keeps (Milvus ranks by vectors, not by a scalar field)",
        (Eng::Redis, "r2") => "three FT.SEARCH requests sent at once (two KNN, one SORTBY pop) and fused in the client",
        (Eng::Es, "r4") | (Eng::Os, "r4") => "r3's _mget, then the 300 trees scored in the client (Elasticsearch's learning-to-rank needs a model imported through Eland, OpenSearch's plugin takes XGBoost or RankLib models)",
        (Eng::Qdrant | Eng::Milvus | Eng::Redis | Eng::Postgres, "r4") => "r3's read by key, then the 300 trees scored in the client (no tree models in the engine)",
        (Eng::Redis, "r5") => "KNN 200 with the artist, capped at 2 an artist in the client",
        (Eng::Es | Eng::Os, "r5") => "knn 200 collapsed by artist with inner_hits of 2, flattened by score",
        (Eng::Vespa, "r5") => "nearestNeighbor 200 grouped by artist, 20 groups of 2, flattened by relevance",
        (Eng::Milvus, "r5") => "a search of 200 grouped by artist (groupSize 2), flattened by distance",
        (Eng::Postgres, "r5") => "the nearest 200 in a subquery, row_number() over each artist",
        (Eng::Milvus, "r6") => "the last plays read by a query of the user's plays sorted in the client (Milvus's query has no ORDER BY), at its default Bounded consistency",
        _ => return None,
    })
}

// --- The data ------------------------------------------------------------

struct Item {
    id: String,
    emb: Vec<f32>,
    artist: String,
    regions: Vec<String>,
    pop: f64,
    f: Vec<f64>,
}

struct User {
    id: String,
    vec: Vec<f32>,
    region: String,
    seen: Vec<String>,
    recent: Vec<String>,
}

struct Data {
    items: Vec<Item>,
    pos: HashMap<String, usize>,
    users: Vec<User>,
    model: Arc<Gbdt>,
}

fn lines(path: &Path) -> Result<impl Iterator<Item = Result<Json, String>>, String> {
    let f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(BufReader::new(f).lines().map(|l| l.map_err(s).and_then(|l| serde_json::from_str::<Json>(&l).map_err(s))))
}

fn strs(j: &Json) -> Vec<String> {
    j.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn floats(j: &Json) -> Vec<f32> {
    j.as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_default()
}

/// The items and the first `users` users with a last track.
fn read(dir: &Path, users: usize) -> Result<Data, String> {
    let mut items = Vec::new();
    for j in lines(&dir.join("items.jsonl"))? {
        let j = j?;
        items.push(Item {
            id: j["id"].as_str().ok_or("an item without an id")?.to_string(),
            emb: floats(&j["emb"]),
            artist: j["artist"].as_str().unwrap_or("").to_string(),
            regions: strs(&j["regions"]),
            pop: j["pop"].as_f64().unwrap_or(0.0),
            f: FEATURES.iter().map(|c| j[*c].as_f64().unwrap_or(0.0)).collect(),
        });
    }
    let pos = items.iter().enumerate().map(|(i, it)| (it.id.clone(), i)).collect();
    let mut us = Vec::new();
    for j in lines(&dir.join("users.jsonl"))? {
        if us.len() >= users {
            break;
        }
        let j = j?;
        let u = User { id: j["id"].as_str().unwrap_or("").to_string(), vec: floats(&j["vec"]), region: j["region"].as_str().unwrap_or("").to_string(), seen: strs(&j["seen"]), recent: strs(&j["recent"]) };
        if !u.recent.is_empty() {
            us.push(u);
        }
    }
    let path = dir.join("ranker").join(strata::query::model::GBDT_FILE);
    let text = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let model = Arc::new(Gbdt::parse("ranker", &text).map_err(s)?);
    if model.features != FEATURES.len() + 1 {
        return Err(format!("the ranker reads {} features, not {}", model.features, FEATURES.len() + 1));
    }
    eprintln!("{} items, {} users", items.len(), us.len());
    Ok(Data { items, pos, users: us, model })
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

// --- Exact answers ---------------------------------------------------------

/// What every kind should answer for a user, computed over all the items.
struct Exact {
    /// r1's filters' items by dot product, best first, with it (CANDS of them).
    near: Vec<(usize, f32)>,
    r1: Vec<String>,
    r2: Vec<String>,
    r4: Vec<String>,
    /// The model's score of each candidate, and r4's 50th best (ties at it count as right).
    r4_score: HashMap<String, f64>,
    r4_cut: f64,
    r5: Vec<String>,
}

fn rrf(lists: &[&[String]], k: f64, n: usize) -> Vec<String> {
    let mut score: HashMap<&str, f64> = HashMap::new();
    let mut first: HashMap<&str, usize> = HashMap::new();
    let mut order = 0;
    for l in lists {
        for (r, id) in l.iter().enumerate() {
            *score.entry(id).or_default() += 1.0 / (k + r as f64 + 1.0);
            first.entry(id).or_insert_with(|| {
                order += 1;
                order
            });
        }
    }
    let mut v: Vec<(&str, f64)> = score.into_iter().collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1).then(first[a.0].cmp(&first[b.0])));
    v.into_iter().take(n).map(|(id, _)| id.to_string()).collect()
}

/// At most `per` of an artist, in the ranking's order, the first `n`.
fn diversify<'a>(ranked: impl Iterator<Item = (&'a str, &'a str)>, per: usize, n: usize) -> Vec<String> {
    let mut count: HashMap<&str, usize> = HashMap::new();
    let mut out = Vec::new();
    for (id, artist) in ranked {
        let c = count.entry(artist).or_default();
        if *c < per {
            *c += 1;
            out.push(id.to_string());
            if out.len() == n {
                break;
            }
        }
    }
    out
}

fn features_of(d: &Data, i: usize, dot: f32) -> Vec<f64> {
    let mut x = d.items[i].f.clone();
    x.push(dot as f64);
    x
}

fn exact(d: &Data, u: &User) -> Exact {
    let seen: HashSet<&str> = u.seen.iter().map(String::as_str).collect();
    let ok: Vec<usize> = (0..d.items.len()).filter(|&i| d.items[i].regions.contains(&u.region) && !seen.contains(d.items[i].id.as_str())).collect();
    let by = |v: &[f32]| {
        let mut s: Vec<(usize, f32)> = ok.iter().map(|&i| (i, dot(&d.items[i].emb, v))).collect();
        s.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        s
    };
    let all = by(&u.vec);
    let ids = |s: &[(usize, f32)], n: usize| s.iter().take(n).map(|&(i, _)| d.items[i].id.clone()).collect::<Vec<_>>();
    let last = d.pos.get(&u.recent[0]).map(|&i| by(&d.items[i].emb)).unwrap_or_default();
    let mut pop = ok.clone();
    pop.sort_by(|&a, &b| d.items[b].pop.total_cmp(&d.items[a].pop).then(d.items[a].id.cmp(&d.items[b].id)));
    let pop: Vec<String> = pop.iter().take(R2_DEPTH).map(|&i| d.items[i].id.clone()).collect();
    let (a, b) = (ids(&all, R2_DEPTH), ids(&last, R2_DEPTH));
    let r2 = rrf(&[&a, &b, &pop], RRF_K, R2_K);
    let near: Vec<(usize, f32)> = all.iter().take(CANDS).copied().collect();
    let mut scored: Vec<(usize, f64)> = near.iter().map(|&(i, s)| (i, d.model.predict(&features_of(d, i, s)))).collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let r4 = scored.iter().take(R4_TOP).map(|&(i, _)| d.items[i].id.clone()).collect();
    let r4_cut = scored.get(R4_TOP.min(scored.len()).saturating_sub(1)).map_or(f64::NEG_INFINITY, |x| x.1);
    let r4_score = scored.iter().map(|&(i, s)| (d.items[i].id.clone(), s)).collect();
    let r5 = diversify(all.iter().map(|&(i, _)| (d.items[i].id.as_str(), d.items[i].artist.as_str())), R5_PER, R5_K);
    Exact { r1: ids(&all, R1_K), r2, r4, r4_score, r4_cut, r5, near }
}

// --- Talking to the engines -------------------------------------------------

/// A client's connection: HTTP (pooled), and Postgres's or Redis's own.
struct Conn {
    e: Eng,
    http: reqwest::Client,
    wire: Option<Conns>,
}

impl Conn {
    fn new(e: Eng, timeout: Duration) -> Result<Conn, String> {
        let mut h = reqwest::header::HeaderMap::new();
        if let (Eng::Strata, Ok(t)) = (e, std::env::var("STRATA_TOKEN")) {
            h.insert(reqwest::header::AUTHORIZATION, format!("Bearer {t}").parse().map_err(|_| "STRATA_TOKEN must be printable ASCII".to_string())?);
        }
        let http = reqwest::Client::builder().default_headers(h).pool_max_idle_per_host(1024).timeout(timeout).build().map_err(s)?;
        let wire = match e {
            Eng::Postgres => Some(Conns::new(Other::Postgres, timeout)?),
            Eng::Redis => Some(Conns::new(Other::Redis, timeout)?),
            _ => None,
        };
        Ok(Conn { e, http, wire })
    }

    fn fork(&self) -> Conn {
        Conn { e: self.e, http: self.http.clone(), wire: self.wire.as_ref().map(|c| c.fork()) }
    }

    /// One request's answer: HTTP's JSON, Postgres's rows, or every reply of Redis's commands as an array.
    async fn exec(&mut self, r: &R) -> Result<Json, String> {
        match r {
            R::Http { method, url, ctype, body } => {
                let res = self.http.request(method.clone(), url).header("content-type", *ctype).body(body.clone()).send().await.map_err(s)?;
                let status = res.status();
                let text = res.text().await.map_err(s)?;
                let j = serde_json::from_str(&text).unwrap_or(Json::String(text));
                if !status.is_success() {
                    return Err(format!("{status}: {}", short(&j)));
                }
                match fault(self.e, &j) {
                    Some(e) => Err(e),
                    None => Ok(j),
                }
            }
            R::Redis { url, cmds } => self.wire.as_mut().ok_or("no Redis connection")?.redis_all(url, cmds).await.map(Json::Array),
            R::Sql { .. } => self.wire.as_mut().ok_or("no Postgres connection")?.exec(r).await,
        }
    }

    async fn call(&mut self, method: Method, url: String, body: Option<Json>) -> Result<Json, String> {
        let body = body.map(|b| serde_json::to_vec(&b).unwrap()).unwrap_or_default();
        self.exec(&R::Http { method, url, ctype: "application/json", body }).await
    }

    async fn sql(&mut self, url: &str, sql: &str) -> Result<Json, String> {
        self.exec(&R::Sql { url: url.to_string(), sql: sql.to_string() }).await
    }

    async fn redis(&mut self, url: &str, args: &[&str]) -> Result<Json, String> {
        let j = self.exec(&R::Redis { url: url.to_string(), cmds: vec![args.iter().map(|a| a.as_bytes().to_vec()).collect()] }).await?;
        Ok(j[0].clone())
    }
}

fn short(j: &Json) -> String {
    let t = match j {
        Json::String(t) => t.clone(),
        _ => j.to_string(),
    };
    t.chars().take(600).collect()
}

/// An error an engine reports in a 2xx answer.
fn fault(e: Eng, j: &Json) -> Option<String> {
    match e {
        Eng::Milvus => j["code"].as_i64().filter(|&c| c != 0).map(|c| format!("{c}: {}", j["message"])),
        Eng::Es | Eng::Os if j["errors"] == json!(true) => Some(format!("bulk item failed: {}", short(&j["items"]))),
        Eng::Es | Eng::Os if j["_shards"]["failed"].as_u64().is_some_and(|n| n > 0) => Some(short(&j["_shards"]["failures"])),
        Eng::Qdrant if j["status"].is_object() => Some(short(&j["status"])),
        _ => error_of(j),
    }
}

fn post(url: String, body: &Json) -> R {
    R::Http { method: Method::POST, url, ctype: "application/json", body: serde_json::to_vec(body).unwrap() }
}

fn put(url: String, body: &Json) -> R {
    R::Http { method: Method::PUT, url, ctype: "application/json", body: serde_json::to_vec(body).unwrap() }
}

fn redis_cmd(url: &str, args: Vec<Vec<u8>>) -> R {
    R::Redis { url: url.to_string(), cmds: vec![args] }
}

fn b(s: impl Into<String>) -> Vec<u8> {
    s.into().into_bytes()
}

fn f32_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn pg_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn pg_array(xs: &[String]) -> String {
    if xs.is_empty() {
        return "ARRAY[]::text[]".into();
    }
    format!("ARRAY[{}]", xs.iter().map(|x| pg_str(x)).collect::<Vec<_>>().join(","))
}

fn quoted(xs: &[String]) -> String {
    xs.iter().map(|x| format!("\"{}\"", x.replace('\\', "\\\\").replace('"', "\\\""))).collect::<Vec<_>>().join(", ")
}

fn qdrant_id(id: &str) -> Json {
    match id.parse::<u64>() {
        Ok(n) => json!(n),
        Err(_) => json!(more::new_id(id)),
    }
}

// --- Loading ---------------------------------------------------------------

/// Creates the items and events (untimed); for Strata, only the events,
/// as the server starts with the items' schema.
async fn setup(e: Eng, c: &mut Conn, u: &str, dim: usize, shards: usize) -> Result<(), String> {
    let fs: Vec<String> = FEATURES.iter().map(|f| f.to_string()).collect();
    match e {
        Eng::Strata => {
            c.call(Method::POST, format!("{u}/v1/sql"), Some(json!({"sql": "DROP TABLE IF EXISTS events"}))).await?;
            c.call(
                Method::POST,
                format!("{u}/v1/sql"),
                Some(json!({"sql": "CREATE TABLE events (id KEYWORD PRIMARY KEY, usr KEYWORD NOT NULL, item KEYWORD NOT NULL, ts INT NOT NULL) OPTIONS(cluster_by = 'usr')"})),
            )
            .await?;
        }
        Eng::Es | Eng::Os => {
            for idx in ["items", "events"] {
                let _ = c.call(Method::DELETE, format!("{u}/{idx}"), None).await;
            }
            let mut props = serde_json::Map::new();
            props.insert("id".into(), json!({"type": "keyword"}));
            props.insert("artist".into(), json!({"type": "keyword"}));
            props.insert("regions".into(), json!({"type": "keyword"}));
            props.insert("pop".into(), json!({"type": "float"}));
            for f in &fs {
                props.insert(f.clone(), json!({"type": "float", "index": false}));
            }
            let mut settings = json!({"number_of_shards": shards, "number_of_replicas": 0, "refresh_interval": "-1"});
            if e == Eng::Os {
                settings["index.knn"] = json!(true);
                props.insert(
                    "emb".into(),
                    json!({"type": "knn_vector", "dimension": dim, "space_type": "innerproduct",
                        "method": {"name": "hnsw", "engine": "lucene", "parameters": {"m": 16, "ef_construction": 200}}}),
                );
            } else {
                props.insert(
                    "emb".into(),
                    json!({"type": "dense_vector", "dims": dim, "index": true, "similarity": "dot_product", "index_options": {"type": "hnsw", "m": 16, "ef_construction": 200}}),
                );
            }
            c.call(Method::PUT, format!("{u}/items"), Some(json!({"settings": settings, "mappings": {"_source": {"excludes": ["emb"]}, "properties": props}}))).await?;
            c.call(
                Method::PUT,
                format!("{u}/events"),
                Some(json!({"settings": {"number_of_shards": shards, "number_of_replicas": 0},
                    "mappings": {"properties": {"eid": {"type": "keyword"}, "usr": {"type": "keyword"}, "item": {"type": "keyword"}, "ts": {"type": "long"}}}})),
            )
            .await?;
            if e == Eng::Os {
                c.call(
                    Method::PUT,
                    format!("{u}/_search/pipeline/rrf"),
                    Some(json!({"phase_results_processors": [{"score-ranker-processor": {"combination": {"technique": "rrf", "rank_constant": RRF_K as u64}}}]})),
                )
                .await?;
            }
        }
        // The application package holds the schemas (deploy/compare/vespa-recsys).
        Eng::Vespa => {
            for t in ["item", "event"] {
                c.call(Method::DELETE, format!("{u}/document/v1/recsys/{t}/docid?selection=true&cluster=recsys"), None).await?;
            }
        }
        Eng::Qdrant => {
            for col in ["items", "events"] {
                let _ = c.call(Method::DELETE, format!("{u}/collections/{col}"), None).await;
            }
            c.call(Method::PUT, format!("{u}/collections/items"), Some(json!({"vectors": {"size": dim, "distance": "Dot", "hnsw_config": {"m": 16, "ef_construct": 200}}}))).await?;
            for (field, schema) in [("regions", json!("keyword")), ("artist", json!("keyword")), ("pop", json!({"type": "float", "range": true}))] {
                c.call(Method::PUT, format!("{u}/collections/items/index?wait=true"), Some(json!({"field_name": field, "field_schema": schema}))).await?;
            }
            c.call(Method::PUT, format!("{u}/collections/events"), Some(json!({"vectors": {}}))).await?;
            for (field, schema) in [("usr", json!("keyword")), ("ts", json!({"type": "integer", "range": true, "lookup": false}))] {
                c.call(Method::PUT, format!("{u}/collections/events/index?wait=true"), Some(json!({"field_name": field, "field_schema": schema}))).await?;
            }
        }
        Eng::Milvus => {
            for col in ["items", "events"] {
                let _ = c.call(Method::POST, format!("{u}/v2/vectordb/collections/drop"), Some(json!({"collectionName": col}))).await;
            }
            let var = |name: &str, len: usize| json!({"fieldName": name, "dataType": "VarChar", "elementTypeParams": {"max_length": len}});
            let mut fields = vec![
                json!({"fieldName": "id", "dataType": "VarChar", "isPrimary": true, "elementTypeParams": {"max_length": 32}}),
                json!({"fieldName": "emb", "dataType": "FloatVector", "elementTypeParams": {"dim": dim}}),
                var("artist", 32),
                json!({"fieldName": "regions", "dataType": "Array", "elementDataType": "VarChar", "elementTypeParams": {"max_capacity": 8, "max_length": 8}}),
                json!({"fieldName": "pop", "dataType": "Float"}),
            ];
            fields.extend(fs.iter().map(|f| json!({"fieldName": f, "dataType": "Float"})));
            c.call(
                Method::POST,
                format!("{u}/v2/vectordb/collections/create"),
                Some(json!({
                    "collectionName": "items",
                    "schema": {"autoId": false, "enableDynamicField": false, "fields": fields},
                    "indexParams": [
                        {"fieldName": "emb", "indexName": "emb", "metricType": "IP", "indexType": "HNSW", "params": {"M": 16, "efConstruction": 200}},
                        {"fieldName": "regions", "indexName": "regions", "indexType": "INVERTED"},
                        {"fieldName": "artist", "indexName": "artist", "indexType": "INVERTED"},
                    ],
                })),
            )
            .await?;
            // A collection needs a vector field: the events' is two zeros, indexed flat.
            c.call(
                Method::POST,
                format!("{u}/v2/vectordb/collections/create"),
                Some(json!({
                    "collectionName": "events",
                    "schema": {"autoId": false, "enableDynamicField": false, "fields": [
                        {"fieldName": "eid", "dataType": "VarChar", "isPrimary": true, "elementTypeParams": {"max_length": 64}},
                        var("usr", 32), var("item", 32), {"fieldName": "ts", "dataType": "Int64"},
                        {"fieldName": "v", "dataType": "FloatVector", "elementTypeParams": {"dim": 2}},
                    ]},
                    "indexParams": [
                        {"fieldName": "v", "indexName": "v", "metricType": "L2", "indexType": "FLAT"},
                        {"fieldName": "usr", "indexName": "usr", "indexType": "INVERTED"},
                    ],
                })),
            )
            .await?;
        }
        Eng::Redis => {
            c.redis(u, &["FLUSHALL"]).await?;
            let dim = dim.to_string();
            c.redis(
                u,
                &[
                    "FT.CREATE", "items", "ON", "HASH", "PREFIX", "1", "i:", "SCHEMA", "id", "TAG", "artist", "TAG", "regions", "TAG", "SEPARATOR", ",", "pop", "NUMERIC", "SORTABLE",
                    "emb", "VECTOR", "HNSW", "10", "TYPE", "FLOAT32", "DIM", &dim, "DISTANCE_METRIC", "IP", "M", "16", "EF_CONSTRUCTION", "200",
                ],
            )
            .await?;
            c.redis(u, &["FT.CREATE", "events", "ON", "HASH", "PREFIX", "1", "e:", "SCHEMA", "usr", "TAG", "ts", "NUMERIC", "SORTABLE"]).await?;
        }
        Eng::Postgres => {
            let cols: Vec<String> = fs.iter().map(|f| format!("{f} real")).collect();
            c.sql(
                u,
                &format!(
                    "DROP TABLE IF EXISTS items; DROP TABLE IF EXISTS events; CREATE EXTENSION IF NOT EXISTS vector; \
                     CREATE TABLE items (id text PRIMARY KEY, emb vector({dim}), artist text, regions text[], pop real, {}); \
                     CREATE TABLE events (eid text PRIMARY KEY, usr text NOT NULL, item text NOT NULL, ts bigint NOT NULL); \
                     CREATE INDEX events_usr_ts ON events (usr, ts DESC)",
                    cols.join(", ")
                ),
            )
            .await?;
        }
    }
    Ok(())
}

fn item_json(it: &Item) -> serde_json::Map<String, Json> {
    let mut m = serde_json::Map::new();
    m.insert("id".into(), json!(it.id));
    m.insert("emb".into(), json!(it.emb));
    m.insert("artist".into(), json!(it.artist));
    m.insert("regions".into(), json!(it.regions));
    m.insert("pop".into(), json!(it.pop));
    for (f, x) in FEATURES.iter().zip(&it.f) {
        m.insert(f.to_string(), json!(x));
    }
    m
}

/// The requests that write `items` (Vespa: one a document).
fn write_items(e: Eng, u: &str, items: &[Item]) -> Vec<R> {
    match e {
        Eng::Strata => {
            let mut body = Vec::new();
            for it in items {
                serde_json::to_writer(&mut body, &Json::Object(item_json(it))).unwrap();
                body.push(b'\n');
            }
            vec![R::Http { method: Method::POST, url: format!("{u}/v1/import?format=jsonl&table=items"), ctype: "application/x-ndjson", body }]
        }
        Eng::Es | Eng::Os => {
            let mut body = Vec::new();
            for it in items {
                serde_json::to_writer(&mut body, &json!({"index": {"_index": "items", "_id": it.id}})).unwrap();
                body.push(b'\n');
                serde_json::to_writer(&mut body, &Json::Object(item_json(it))).unwrap();
                body.push(b'\n');
            }
            vec![R::Http { method: Method::POST, url: format!("{u}/_bulk"), ctype: "application/x-ndjson", body }]
        }
        Eng::Vespa => items
            .iter()
            .map(|it| {
                let mut m = item_json(it);
                m.insert("emb".into(), json!({"values": it.emb}));
                post(format!("{u}/document/v1/recsys/item/docid/{}", it.id), &json!({"fields": m}))
            })
            .collect(),
        Eng::Qdrant => {
            let points: Vec<Json> = items
                .iter()
                .map(|it| {
                    let mut p = item_json(it);
                    p.remove("emb");
                    json!({"id": qdrant_id(&it.id), "vector": it.emb, "payload": p})
                })
                .collect();
            vec![put(format!("{u}/collections/items/points?wait=true"), &json!({"points": points}))]
        }
        Eng::Milvus => {
            let rows: Vec<Json> = items.iter().map(|it| Json::Object(item_json(it))).collect();
            vec![post(format!("{u}/v2/vectordb/entities/insert"), &json!({"collectionName": "items", "data": rows}))]
        }
        Eng::Redis => {
            let cmds = items
                .iter()
                .map(|it| {
                    let mut c = vec![b("HSET"), b(format!("i:{}", it.id)), b("id"), b(&it.id), b("artist"), b(&it.artist), b("regions"), b(it.regions.join(",")), b("pop"), b(it.pop.to_string())];
                    for (f, x) in FEATURES.iter().zip(&it.f) {
                        c.push(b(*f));
                        c.push(b(x.to_string()));
                    }
                    c.push(b("emb"));
                    c.push(f32_bytes(&it.emb));
                    c
                })
                .collect();
            vec![R::Redis { url: u.to_string(), cmds }]
        }
        Eng::Postgres => {
            let rows: Vec<String> = items
                .iter()
                .map(|it| {
                    let f: Vec<String> = it.f.iter().map(|x| x.to_string()).collect();
                    format!("({}, {}, {}, {}, {}, {})", pg_str(&it.id), pg_str(&vector_text(&it.emb)), pg_str(&it.artist), pg_array(&it.regions), it.pop, f.join(", "))
                })
                .collect();
            vec![R::Sql { url: u.to_string(), sql: format!("INSERT INTO items VALUES {}", rows.join(", ")) }]
        }
    }
}

/// What an engine does once the writes are in, before every item is searchable and indexed, timed by step.
async fn finish(e: Eng, c: &mut Conn, u: &str, n: usize) -> Result<Json, String> {
    let mut steps = serde_json::Map::new();
    let mut step = |name: &str, t: Instant| {
        steps.insert(name.to_string(), json!(t.elapsed().as_secs_f64()));
        eprintln!("{}: {name} in {:.1} s", e.name(), t.elapsed().as_secs_f64());
    };
    match e {
        // Searchable once imported; merges go on in the background.
        Eng::Strata => {
            let h = c.call(Method::GET, format!("{u}/v1/health"), None).await?;
            steps.insert("segments".into(), h["segments"].clone());
        }
        Eng::Es | Eng::Os => {
            let t = Instant::now();
            c.call(Method::POST, format!("{u}/items/_refresh"), None).await?;
            step("refresh_s", t);
            let t = Instant::now();
            c.call(Method::POST, format!("{u}/items/_forcemerge?max_num_segments=1"), None).await?;
            c.call(Method::POST, format!("{u}/items/_refresh"), None).await?;
            step("merge_s", t);
        }
        Eng::Vespa => {}
        Eng::Qdrant => {
            let t = Instant::now();
            // Its optimizer builds a segment's HNSW graph once the segment holds indexing_threshold (10 MB) of vectors:
            // smaller segments are searched exactly. Ready when it has nothing left to do.
            let (mut green, mut last) = (0, Json::Null);
            while green < 3 {
                let j = c.call(Method::GET, format!("{u}/collections/items"), None).await?;
                let r = &j["result"];
                if r["status"] == json!("green") && r["optimizer_status"] == json!("ok") && r["points_count"].as_u64() >= Some(n as u64) {
                    green += 1;
                } else {
                    green = 0;
                }
                last = json!({"indexed_vectors": r["indexed_vectors_count"], "segments": r["segments_count"]});
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            step("index_s", t);
            steps.insert("index".into(), last);
        }
        Eng::Milvus => {
            let t = Instant::now();
            c.call(Method::POST, format!("{u}/v2/vectordb/collections/flush"), Some(json!({"collectionName": "items"}))).await?;
            step("flush_s", t);
            let t = Instant::now();
            loop {
                let j = c.call(Method::POST, format!("{u}/v2/vectordb/indexes/describe"), Some(json!({"collectionName": "items", "indexName": "emb"}))).await?;
                let d = &j["data"][0];
                if d["indexState"] == json!("Finished") && d["pendingRows"].as_u64() == Some(0) && d["indexedRows"].as_u64() >= Some(n as u64) {
                    break;
                }
                if d["indexState"] == json!("Failed") {
                    return Err(format!("index emb: {d}"));
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            step("index_s", t);
            let t = Instant::now();
            // A collection made with its indexes is loaded at once, and its
            // rows are searched in growing segments through an interim index
            // (a fraction of the HNSW's recall) until the sealed, indexed
            // segments are handed over. A release and a load serves those.
            for col in ["items", "events"] {
                c.call(Method::POST, format!("{u}/v2/vectordb/collections/release"), Some(json!({"collectionName": col}))).await?;
                c.call(Method::POST, format!("{u}/v2/vectordb/collections/load"), Some(json!({"collectionName": col}))).await?;
                loop {
                    let j = c.call(Method::POST, format!("{u}/v2/vectordb/collections/get_load_state"), Some(json!({"collectionName": col}))).await?;
                    if j["data"]["loadState"] == json!("LoadStateLoaded") {
                        break;
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
            step("load_s", t);
        }
        Eng::Redis => {
            let t = Instant::now();
            loop {
                let info = c.redis(u, &["FT.INFO", "items"]).await?;
                let a = info.as_array().cloned().unwrap_or_default();
                let get = |k: &str| a.iter().position(|x| x == &json!(k)).and_then(|i| a.get(i + 1)).cloned().unwrap_or(Json::Null);
                let num = |v: Json| v.as_str().and_then(|x| x.parse::<f64>().ok()).or(v.as_f64()).unwrap_or(0.0);
                if num(get("percent_indexed")) >= 1.0 && num(get("indexing")) == 0.0 {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            step("index_s", t);
        }
        Eng::Postgres => {
            let set = "SET maintenance_work_mem = '2GB'; SET max_parallel_maintenance_workers = 2; ";
            let t = Instant::now();
            c.sql(u, &format!("{set}CREATE INDEX items_emb ON items USING hnsw (emb vector_ip_ops) WITH (m = 16, ef_construction = 200)")).await?;
            step("hnsw_index_s", t);
            let t = Instant::now();
            c.sql(u, "CREATE INDEX items_pop ON items (pop DESC)").await?;
            c.sql(u, "VACUUM ANALYZE items").await?;
            step("analyze_s", t);
        }
    }
    Ok(Json::Object(steps))
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn load(e: Eng, urls: &[String], dir: &Path, batch: usize, clients: usize, shards: usize, out: &Path) -> Result<(), String> {
    let d = read(dir, 0)?;
    let dim = d.items.first().map(|it| it.emb.len()).ok_or("no items")?;
    let mut c = Conn::new(e, Duration::from_secs(3600))?;
    setup(e, &mut c, &urls[0], dim, shards).await?;
    let mut reqs = Vec::new();
    for (i, part) in d.items.chunks(batch).enumerate() {
        reqs.extend(write_items(e, &urls[i % urls.len()], part));
    }
    let reqs = Arc::new(reqs);
    let clients = if e == Eng::Vespa { clients.max(VESPA_PUTS) } else { clients };
    let t = Instant::now();
    let next = Arc::new(AtomicUsize::new(0));
    let tasks: Vec<_> = (0..clients)
        .map(|_| {
            let (mut c, reqs, next) = (c.fork(), reqs.clone(), next.clone());
            tokio::spawn(async move {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(r) = reqs.get(i) else { return Ok::<(), String>(()) };
                    c.exec(r).await.map_err(|e| format!("write {i}: {e}"))?;
                }
            })
        })
        .collect();
    for t in tasks {
        t.await.map_err(s)??;
    }
    let write_s = t.elapsed().as_secs_f64();
    eprintln!("{}: wrote {} items in {write_s:.1} s", e.name(), d.items.len());
    let t = Instant::now();
    let steps = finish(e, &mut c, &urls[0], d.items.len()).await?;
    let ready_s = t.elapsed().as_secs_f64();
    std::fs::create_dir_all(out).map_err(s)?;
    let report = json!({"engine": e.name(), "items": d.items.len(), "dim": dim, "batch": batch, "clients": clients, "shards": shards,
        "write_s": write_s, "ready_s": ready_s, "steps": steps, "items_per_s": d.items.len() as f64 / (write_s + ready_s), "at": unix_s()});
    let path = out.join(format!("recsys-load-{}.json", e.name()));
    std::fs::write(&path, serde_json::to_string_pretty(&report).unwrap()).map_err(s)?;
    eprintln!("{}: ready in {ready_s:.1} s; {}", e.name(), path.display());
    Ok(())
}

// --- Requests ----------------------------------------------------------------

/// What the client does with the engine's answers before it has the kind's.
enum Plan {
    One(R),
    /// Requests sent at once, their rankings (and a ranking the client keeps) fused by RRF.
    Fuse(Vec<R>, Option<Arc<Vec<String>>>),
    /// The candidates' features read, then scored by the model with each candidate's dot product, which the retrieval gave.
    Score(R, Arc<HashMap<String, f32>>),
    /// The nearest tracks with their artists, capped per artist.
    Cap(R),
}

impl Plan {
    fn sources(&self) -> usize {
        match self {
            Plan::Fuse(r, _) => r.len(),
            _ => 1,
        }
    }
}

/// A probe's inputs.
struct Ask<'a> {
    u: &'a User,
    last: &'a [f32],
    /// The exact top CANDS of r1's filters, for r3 and r4.
    cands: Vec<String>,
    dots: Arc<HashMap<String, f32>>,
    /// The region's most popular, for an engine that cannot rank by a field (Milvus's r2).
    pop: Arc<Vec<String>>,
}

fn strata_sql(u: &str, sql: String, params: Json) -> R {
    post(format!("{u}/v1/query"), &json!({"sql": sql, "params": params}))
}

fn ids_sql(xs: &[String]) -> String {
    xs.iter().map(|x| sql_str(x)).collect::<Vec<_>>().join(", ")
}

/// The request of `kind` for a probe (r6's are the events', below).
fn plan(e: Eng, kind: &str, a: &Ask, u: &str) -> Option<Plan> {
    let (v, r, seen) = (&a.u.vec, &a.u.region, &a.u.seen);
    let fs = FEATURES.join(", ");
    Some(match e {
        Eng::Strata => {
            let filter = if seen.is_empty() { "regions = :r".to_string() } else { format!("regions = :r AND id NOT IN ({})", ids_sql(seen)) };
            let p = json!({"u": v, "r": r, "l": a.last});
            // The others' search breadth, every candidate re-scored with the floats.
            let sim = |q: &str, ef: usize| format!("similarity(emb, :{q}, ef => {ef}, rerank => {ef})");
            Plan::One(match kind {
                "r1" | "r6" => strata_sql(u, format!("SELECT id FROM items WHERE {filter} ORDER BY {} DESC LIMIT {R1_K}", sim("u", R1_EF)), p),
                "r2" => strata_sql(
                    u,
                    format!("SELECT id FROM items WHERE {filter} ORDER BY rrf({}, {}, pop, depth => {R2_DEPTH}) DESC LIMIT {R2_K}", sim("u", 2 * R2_DEPTH), sim("l", 2 * R2_DEPTH)),
                    p,
                ),
                "r3" => post(format!("{u}/v1/kv/get"), &json!({"table": "items", "keys": a.cands})),
                // A rerank phase over the candidates in their order: `ORDER BY gbdt(…, similarity(emb, :u))`
                // on its own would rank only similarity's top 100 of them.
                "r4" => strata_sql(
                    u,
                    format!(
                        "WITH c AS (SELECT * FROM items WHERE id IN ({}) ORDER BY ranked(:c) DESC LIMIT {CANDS}) SELECT id FROM c ORDER BY gbdt('ranker', {fs}, dot(emb, :u)) DESC LIMIT {R4_TOP}",
                        ids_sql(&a.cands)
                    ),
                    json!({"u": v, "c": a.cands}),
                ),
                "r5" => strata_sql(u, format!("SELECT id FROM items WHERE {filter} ORDER BY {} DESC MAX {R5_PER} PER artist LIMIT {R5_K}", sim("u", 2 * R5_DEPTH)), p),
                _ => return None,
            })
        }
        Eng::Es | Eng::Os => {
            let os = e == Eng::Os;
            let filter = json!({"bool": {"filter": [{"term": {"regions": r}}], "must_not": [{"ids": {"values": seen}}]}});
            let knn = |vec: &[f32], k: usize, ef: usize| {
                if os {
                    json!({"knn": {"emb": {"vector": vec, "k": k, "method_parameters": {"ef_search": ef}, "filter": filter}}})
                } else {
                    json!({"field": "emb", "query_vector": vec, "k": k, "num_candidates": ef, "filter": filter})
                }
            };
            let search = |k: usize, extra: Json| {
                let mut body = json!({"size": k, "_source": false, "track_total_hits": false});
                body.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
                post(format!("{u}/items/_search?request_cache=false"), &body)
            };
            let near = |k: usize, ef: usize| if os { json!({"query": knn(v, k, ef)}) } else { json!({"knn": knn(v, k, ef)}) };
            let pop = json!({"function_score": {"query": filter, "field_value_factor": {"field": "pop"}, "boost_mode": "replace"}});
            let mget = || post(format!("{u}/items/_mget?_source_includes={}", FEATURES.join(",")), &json!({"ids": a.cands}));
            match kind {
                "r1" | "r6" => Plan::One(search(R1_K, near(R1_K, R1_EF))),
                "r2" if os => Plan::One(post(
                    format!("{u}/items/_search?request_cache=false&search_pipeline=rrf"),
                    &json!({"size": R2_K, "_source": false, "query": {"hybrid": {"queries": [knn(v, R2_DEPTH, 2 * R2_DEPTH), knn(a.last, R2_DEPTH, 2 * R2_DEPTH), pop], "pagination_depth": R2_DEPTH}}}),
                )),
                "r2" => Plan::One(search(
                    R2_K,
                    json!({"retriever": {"rrf": {"retrievers": [{"knn": knn(v, R2_DEPTH, 2 * R2_DEPTH)}, {"knn": knn(a.last, R2_DEPTH, 2 * R2_DEPTH)}, {"standard": {"query": pop}}],
                        "rank_window_size": R2_DEPTH, "rank_constant": RRF_K as u64}}}),
                )),
                "r3" => Plan::One(mget()),
                "r4" => Plan::Score(mget(), a.dots.clone()),
                "r5" => {
                    // The knn query, not the top-level knn: collapse's inner
                    // hits run the request's query again within each group,
                    // and with a top-level knn that query matches everything.
                    let mut extra = if os { near(R5_DEPTH, 2 * R5_DEPTH) } else { json!({"query": {"knn": knn(v, R5_DEPTH, 2 * R5_DEPTH)}}) };
                    extra["collapse"] = json!({"field": "artist", "inner_hits": {"name": "top", "size": R5_PER, "_source": false}});
                    Plan::One(search(R5_K, extra))
                }
                _ => return None,
            }
        }
        Eng::Vespa => {
            let not = if seen.is_empty() { String::new() } else { format!(" and !(id in ({}))", quoted(seen)) };
            let filter = format!("regions contains \"{r}\"{not}");
            let nn = |label: &str, q: &str, k: usize| format!("({{label:\"{label}\", targetHits:{k}, hnsw.exploreAdditionalHits:{k}}}nearestNeighbor(emb, {q}))");
            let search = |yql: String, ranking: &str, hits: usize, summary: &str| {
                let mut body = json!({"yql": yql, "ranking": ranking, "hits": hits, "presentation.summary": summary, "timeout": "10s"});
                // Each profile takes the inputs it declares.
                if ranking != "unranked" {
                    body["input.query(u)"] = json!(v);
                }
                post(format!("{u}/search/"), &body)
            };
            match kind {
                "r1" | "r6" => Plan::One(search(format!("select id from item where {} and {filter}", nn("u", "u", R1_K)), "near", R1_K, "minimal")),
                // Three queries fused in the client. One query can't hold the
                // region's most popular: range(pop)'s hitLimit takes them
                // before the filters, and a query matching the whole region
                // computes closeness for every document, so its first phase
                // can't tell the neighbours from the rest.
                "r2" => {
                    let near = |x: &[f32]| {
                        let yql = format!("select id from item where {} and {filter}", nn("u", "u", R2_DEPTH));
                        post(format!("{u}/search/"), &json!({"yql": yql, "ranking": "near", "hits": R2_DEPTH, "input.query(u)": x, "presentation.summary": "minimal", "timeout": "10s"}))
                    };
                    let pop = search(format!("select id from item where {filter} order by pop desc"), "unranked", R2_DEPTH, "minimal");
                    Plan::Fuse(vec![near(v), near(a.last), pop], None)
                }
                "r3" => Plan::One(search(format!("select * from item where id in ({})", quoted(&a.cands)), "unranked", CANDS, "features")),
                "r4" => Plan::One(search(format!("select id from item where id in ({})", quoted(&a.cands)), "gbdt", R4_TOP, "minimal")),
                "r5" => Plan::One(search(
                    format!(
                        "select id from item where {} and {filter} limit 0 | all(group(artist) max({R5_K}) order(-max(relevance())) each(max({R5_PER}) each(output(summary(minimal)))))",
                        nn("u", "u", R5_DEPTH)
                    ),
                    "near",
                    0,
                    "minimal",
                )),
                _ => return None,
            }
        }
        Eng::Qdrant => {
            let ids: Vec<Json> = seen.iter().map(|x| qdrant_id(x)).collect();
            let filter = json!({"must": [{"key": "regions", "match": {"value": r}}], "must_not": [{"has_id": ids}]});
            let q = format!("{u}/collections/items/points/query");
            let get = || post(format!("{u}/collections/items/points"), &json!({"ids": a.cands.iter().map(|x| qdrant_id(x)).collect::<Vec<_>>(), "with_payload": {"include": FEATURES}, "with_vector": false}));
            match kind {
                "r1" | "r6" => Plan::One(post(q, &json!({"query": v, "filter": filter, "limit": R1_K, "params": {"hnsw_ef": R1_EF}, "with_payload": false}))),
                "r2" => {
                    let near = |x: &[f32]| json!({"query": x, "filter": filter, "limit": R2_DEPTH, "params": {"hnsw_ef": 2 * R2_DEPTH}});
                    Plan::One(post(
                        q,
                        &json!({"prefetch": [near(v), near(a.last), {"query": {"order_by": {"key": "pop", "direction": "desc"}}, "filter": filter, "limit": R2_DEPTH}],
                            "query": {"rrf": {"k": RRF_K as u64}}, "limit": R2_K, "with_payload": false}),
                    ))
                }
                "r3" => Plan::One(get()),
                "r4" => Plan::Score(get(), a.dots.clone()),
                "r5" => Plan::One(post(
                    format!("{u}/collections/items/points/query/groups"),
                    &json!({"query": v, "filter": filter, "group_by": "artist", "group_size": R5_PER, "limit": R5_K, "params": {"hnsw_ef": 2 * R5_DEPTH}, "with_payload": false}),
                )),
                _ => return None,
            }
        }
        Eng::Milvus => {
            let not = if seen.is_empty() { String::new() } else { format!(" and id not in [{}]", quoted(seen)) };
            let filter = format!("array_contains(regions, \"{r}\"){not}");
            let search = |x: &[f32], k: usize, ef: usize, extra: Json| {
                let mut body = json!({"collectionName": "items", "data": [x], "annsField": "emb", "filter": filter, "limit": k, "searchParams": {"params": {"ef": ef}}, "outputFields": ["id"]});
                body.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
                post(format!("{u}/v2/vectordb/entities/search"), &body)
            };
            let get = || post(format!("{u}/v2/vectordb/entities/get"), &json!({"collectionName": "items", "id": a.cands, "outputFields": FEATURES}));
            match kind {
                "r1" | "r6" => Plan::One(search(v, R1_K, R1_EF, json!({}))),
                "r2" => Plan::Fuse(vec![search(v, R2_DEPTH, 2 * R2_DEPTH, json!({})), search(a.last, R2_DEPTH, 2 * R2_DEPTH, json!({}))], Some(a.pop.clone())),
                "r3" => Plan::One(get()),
                "r4" => Plan::Score(get(), a.dots.clone()),
                "r5" => Plan::One(search(v, R5_K, 2 * R5_DEPTH, json!({"groupingField": "artist", "groupSize": R5_PER, "strictGroupSize": false}))),
                _ => return None,
            }
        }
        Eng::Redis => {
            let not = if seen.is_empty() { String::new() } else { format!(" -@id:{{{}}}", seen.join("|")) };
            let filter = format!("@regions:{{{r}}}{not}");
            let knn = |x: &[f32], k: usize, ef: usize, ret: &[&str]| {
                let mut c = vec![b("FT.SEARCH"), b("items"), b(format!("({filter})=>[KNN {k} @emb $v EF_RUNTIME {ef} AS d]")), b("SORTBY"), b("d"), b("LIMIT"), b("0"), b(k.to_string()), b("RETURN"), b(ret.len().to_string())];
                c.extend(ret.iter().map(|x| b(*x)));
                c.extend([b("PARAMS"), b("2"), b("v"), f32_bytes(x), b("DIALECT"), b("2")]);
                redis_cmd(u, c)
            };
            let hmget = || R::Redis {
                url: u.to_string(),
                cmds: a
                    .cands
                    .iter()
                    .map(|id| {
                        let mut c = vec![b("HMGET"), b(format!("i:{id}"))];
                        c.extend(FEATURES.iter().map(|f| b(*f)));
                        c
                    })
                    .collect(),
            };
            match kind {
                "r1" | "r6" => Plan::One(knn(v, R1_K, R1_EF, &["d"])),
                "r2" => {
                    let pop = redis_cmd(u, vec![b("FT.SEARCH"), b("items"), b(filter.clone()), b("SORTBY"), b("pop"), b("DESC"), b("LIMIT"), b("0"), b(R2_DEPTH.to_string()), b("NOCONTENT"), b("DIALECT"), b("2")]);
                    Plan::Fuse(vec![knn(v, R2_DEPTH, 2 * R2_DEPTH, &["d"]), knn(a.last, R2_DEPTH, 2 * R2_DEPTH, &["d"]), pop], None)
                }
                "r3" => Plan::One(hmget()),
                "r4" => Plan::Score(hmget(), a.dots.clone()),
                "r5" => Plan::Cap(knn(v, R5_DEPTH, 2 * R5_DEPTH, &["d", "artist"])),
                _ => return None,
            }
        }
        Eng::Postgres => {
            let not = if seen.is_empty() { String::new() } else { format!(" AND id <> ALL({})", pg_array(seen)) };
            let filter = format!("regions @> ARRAY[{}]{not}", pg_str(r));
            let qv = pg_str(&vector_text(v));
            let ql = pg_str(&vector_text(a.last));
            let set = |ef: usize| format!("SET hnsw.ef_search = {}; ", ef.min(1000));
            let sql = |s: String| Plan::One(R::Sql { url: u.to_string(), sql: s });
            let get = || R::Sql { url: u.to_string(), sql: format!("SELECT id, {fs} FROM items WHERE id = ANY({})", pg_array(&a.cands)) };
            match kind {
                "r1" | "r6" => sql(format!("{}SELECT id FROM items WHERE {filter} ORDER BY emb <#> {qv} LIMIT {R1_K}", set(R1_EF))),
                "r2" => {
                    let near = |q: &str| format!("SELECT id, row_number() OVER (ORDER BY d) r FROM (SELECT id, emb <#> {q} d FROM items WHERE {filter} ORDER BY emb <#> {q} LIMIT {R2_DEPTH}) x");
                    sql(format!(
                        "{}WITH s AS ({} UNION ALL {} UNION ALL SELECT id, row_number() OVER (ORDER BY pop DESC, id) r FROM (SELECT id, pop FROM items WHERE {filter} ORDER BY pop DESC LIMIT {R2_DEPTH}) y) \
                         SELECT id, sum(1.0 / ({RRF_K} + r)) s FROM s GROUP BY id ORDER BY s DESC, id LIMIT {R2_K}",
                        set(2 * R2_DEPTH),
                        near(&qv),
                        near(&ql)
                    ))
                }
                "r3" => Plan::One(get()),
                "r4" => Plan::Score(get(), a.dots.clone()),
                "r5" => sql(format!(
                    "{}SELECT id FROM (SELECT id, d, row_number() OVER (PARTITION BY artist ORDER BY d) n FROM \
                     (SELECT id, artist, emb <#> {qv} d FROM items WHERE {filter} ORDER BY emb <#> {qv} LIMIT {R5_DEPTH}) x) y WHERE n <= {R5_PER} ORDER BY d LIMIT {R5_K}",
                    set(2 * R5_DEPTH)
                )),
                _ => return None,
            }
        }
    })
}

// --- Answers -------------------------------------------------------------------

fn id_text(j: &Json) -> String {
    match j {
        Json::String(s) => s.clone(),
        Json::Null => String::new(),
        _ => j.to_string(),
    }
}

/// Vespa's hits, grouped or not, with their relevance.
fn vespa_hits(j: &Json, out: &mut Vec<(String, f64)>) {
    for c in j["children"].as_array().into_iter().flatten() {
        if let Some(id) = c["fields"].get("id").or_else(|| c["fields"].get("eid")) {
            out.push((id_text(id), c["relevance"].as_f64().unwrap_or(0.0)));
        } else {
            vespa_hits(c, out);
        }
    }
}

/// A Redis search reply's keys (prefix dropped) and their fields.
fn redis_rows(j: &Json) -> Vec<(String, HashMap<String, String>)> {
    let a = j.as_array().cloned().unwrap_or_default();
    let mut out: Vec<(String, HashMap<String, String>)> = Vec::new();
    for x in a.iter().skip(1) {
        match x {
            Json::String(k) => out.push((k.split_once(':').map_or(k.as_str(), |(_, id)| id).to_string(), HashMap::new())),
            Json::Array(kv) => {
                if let Some(last) = out.last_mut() {
                    for p in kv.chunks(2) {
                        if let [k, v] = p {
                            last.1.insert(id_text(k), id_text(v));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The ranked ids of a search answer, best first (`r5` flattened by score).
fn ranked(e: Eng, kind: &str, j: &Json) -> Vec<String> {
    match e {
        Eng::Strata => j["keys"].as_array().map(|k| k.iter().map(id_text).collect()).unwrap_or_default(),
        Eng::Es | Eng::Os if kind == "r5" => {
            let mut hits: Vec<(String, f64)> = Vec::new();
            for h in j["hits"]["hits"].as_array().into_iter().flatten() {
                for i in h["inner_hits"]["top"]["hits"]["hits"].as_array().into_iter().flatten() {
                    hits.push((id_text(&i["_id"]), i["_score"].as_f64().unwrap_or(0.0)));
                }
            }
            hits.sort_by(|a, b| b.1.total_cmp(&a.1));
            hits.into_iter().take(R5_K).map(|h| h.0).collect()
        }
        Eng::Es | Eng::Os => j["hits"]["hits"].as_array().map(|h| h.iter().map(|x| id_text(&x["_id"])).collect()).unwrap_or_default(),
        Eng::Vespa => {
            let mut hits = Vec::new();
            vespa_hits(&j["root"], &mut hits);
            if kind == "r5" {
                hits.sort_by(|a, b| b.1.total_cmp(&a.1));
                hits.truncate(R5_K);
            }
            hits.into_iter().map(|h| h.0).collect()
        }
        Eng::Qdrant if kind == "r5" => {
            let mut hits: Vec<(String, f64)> = Vec::new();
            for g in j["result"]["groups"].as_array().into_iter().flatten() {
                for h in g["hits"].as_array().into_iter().flatten() {
                    hits.push((id_text(&h["id"]), h["score"].as_f64().unwrap_or(0.0)));
                }
            }
            hits.sort_by(|a, b| b.1.total_cmp(&a.1));
            hits.into_iter().take(R5_K).map(|h| h.0).collect()
        }
        Eng::Qdrant => j["result"]["points"].as_array().map(|p| p.iter().map(|x| id_text(&x["id"])).collect()).unwrap_or_default(),
        Eng::Milvus => {
            let mut hits: Vec<(String, f64)> = j["data"].as_array().into_iter().flatten().map(|x| (id_text(&x["id"]), x["distance"].as_f64().unwrap_or(0.0))).collect();
            if kind == "r5" {
                hits.sort_by(|a, b| b.1.total_cmp(&a.1));
                hits.truncate(R5_K);
            }
            hits.into_iter().map(|h| h.0).collect()
        }
        Eng::Redis => redis_rows(&j[0]).into_iter().map(|r| r.0).collect(),
        Eng::Postgres => j["rows"].as_array().map(|r| r.iter().map(|x| id_text(&x[0])).collect()).unwrap_or_default(),
    }
}

/// The rows of a read by key (r3): each found row's id and features.
fn rows(e: Eng, j: &Json, cands: &[String]) -> Vec<(String, Vec<f64>)> {
    let fv = |o: &Json| FEATURES.iter().map(|f| o[*f].as_f64().or_else(|| o[*f].as_str().and_then(|x| x.parse().ok())).unwrap_or(f64::NAN)).collect::<Vec<_>>();
    match e {
        Eng::Strata => j["rows"].as_array().into_iter().flatten().zip(cands).filter(|(r, _)| r.is_object()).map(|(r, id)| (id.clone(), fv(r))).collect(),
        Eng::Es | Eng::Os => j["docs"].as_array().into_iter().flatten().filter(|d| d["found"] == json!(true)).map(|d| (id_text(&d["_id"]), fv(&d["_source"]))).collect(),
        Eng::Vespa => {
            let mut out = Vec::new();
            for c in j["root"]["children"].as_array().into_iter().flatten() {
                out.push((id_text(&c["fields"]["id"]), fv(&c["fields"])));
            }
            out
        }
        Eng::Qdrant => j["result"].as_array().into_iter().flatten().map(|p| (id_text(&p["id"]), fv(&p["payload"]))).collect(),
        Eng::Milvus => j["data"].as_array().into_iter().flatten().map(|r| (id_text(&r["id"]), fv(r))).collect(),
        Eng::Redis => j
            .as_array()
            .into_iter()
            .flatten()
            .zip(cands)
            .filter(|(r, _)| r.as_array().is_some_and(|a| a.iter().any(|x| !x.is_null())))
            .map(|(r, id)| (id.clone(), r.as_array().unwrap().iter().map(|x| id_text(x).parse().unwrap_or(f64::NAN)).collect()))
            .collect(),
        Eng::Postgres => j["rows"].as_array().into_iter().flatten().map(|r| (id_text(&r[0]), (1..=FEATURES.len()).map(|i| r[i].as_f64().unwrap_or(f64::NAN)).collect())).collect(),
    }
}

/// Sends a plan's requests and returns the kind's answer: ids best first, or (r3) the rows' ids.
async fn perform(e: Eng, kind: &str, plan: &Plan, conns: &mut [Conn], model: &Gbdt, cands: &[String]) -> Result<Vec<String>, String> {
    match plan {
        Plan::One(r) => {
            let j = conns[0].exec(r).await?;
            Ok(if kind == "r3" { rows(e, &j, cands).into_iter().map(|r| r.0).collect() } else { ranked(e, kind, &j) })
        }
        Plan::Fuse(reqs, kept) => {
            let (first, rest) = conns.split_at_mut(1);
            let mut cs: Vec<&mut Conn> = first.iter_mut().chain(rest.iter_mut()).collect();
            let futs = reqs.iter().zip(cs.iter_mut()).map(|(r, c)| c.exec(r));
            let answers = futures::future::join_all(futs).await;
            let mut lists = Vec::new();
            for a in answers {
                lists.push(ranked(e, "r2", &a?));
            }
            if let Some(k) = kept {
                lists.push(k.iter().filter(|x| !cands.contains(x)).take(R2_DEPTH).cloned().collect());
            }
            let refs: Vec<&[String]> = lists.iter().map(|l| l.as_slice()).collect();
            Ok(rrf(&refs, RRF_K, R2_K))
        }
        Plan::Score(r, dots) => {
            let j = conns[0].exec(r).await?;
            let mut scored: Vec<(String, f64)> = rows(e, &j, cands)
                .into_iter()
                .map(|(id, mut x)| {
                    x.push(dots.get(&id).copied().unwrap_or(0.0) as f64);
                    let s = model.predict(&x);
                    (id, s)
                })
                .collect();
            scored.sort_by(|a, b| b.1.total_cmp(&a.1));
            Ok(scored.into_iter().take(R4_TOP).map(|x| x.0).collect())
        }
        Plan::Cap(r) => {
            let j = conns[0].exec(r).await?;
            let rs = redis_rows(&j[0]);
            Ok(diversify(rs.iter().map(|(id, f)| (id.as_str(), f.get("artist").map_or("", |a| a.as_str()))), R5_PER, R5_K))
        }
    }
}

/// A prepared probe: its plan, the candidates its answer is read against, and the exact answer.
struct Probe {
    plan: Plan,
    cands: Arc<Vec<String>>,
    exact: Vec<String>,
    /// r4: each candidate's exact score and the 50th best.
    r4: Option<(Arc<HashMap<String, f64>>, f64)>,
}

fn score(kind: &str, got: &[String], want: &[String], r4: Option<&(Arc<HashMap<String, f64>>, f64)>) -> f64 {
    // r4: the share of the answer the model ranks in its top 50 (a tie with the 50th counts: the scores of a few hundred trees tie often).
    if let Some((scores, cut)) = r4 {
        let ok = got.iter().filter(|x| scores.get(*x).is_some_and(|&s| s >= cut - 1e-9 * cut.abs().max(1.0))).count();
        return ok as f64 / want.len().max(1) as f64;
    }
    if kind == "r3" {
        let g: HashSet<&String> = got.iter().collect();
        return want.iter().filter(|x| g.contains(x)).count() as f64 / want.len().max(1) as f64;
    }
    let w: HashSet<&String> = want.iter().collect();
    got.iter().filter(|x| w.contains(x)).count() as f64 / want.len().max(1) as f64
}

// --- Events (r6) -------------------------------------------------------------

fn write_event(e: Eng, u: &str, eid: &str, usr: &str, item: &str, ts: i64) -> R {
    let doc = json!({"eid": eid, "usr": usr, "item": item, "ts": ts});
    match e {
        Eng::Strata => post(format!("{u}/v1/write"), &json!({"ops": [{"op": "insert", "table": "events", "doc": {"id": eid, "usr": usr, "item": item, "ts": ts}}]})),
        Eng::Es | Eng::Os => put(format!("{u}/events/_doc/{eid}"), &doc),
        Eng::Vespa => post(format!("{u}/document/v1/recsys/event/docid/{eid}"), &json!({"fields": doc})),
        Eng::Qdrant => put(format!("{u}/collections/events/points"), &json!({"points": [{"id": qdrant_id(eid), "vector": {}, "payload": doc}]})),
        Eng::Milvus => post(format!("{u}/v2/vectordb/entities/insert"), &json!({"collectionName": "events", "data": [{"eid": eid, "usr": usr, "item": item, "ts": ts, "v": [0.0, 0.0]}]})),
        Eng::Redis => redis_cmd(u, vec![b("HSET"), b(format!("e:{eid}")), b("usr"), b(usr), b("item"), b(item), b("ts"), b(ts.to_string())]),
        Eng::Postgres => R::Sql { url: u.to_string(), sql: format!("INSERT INTO events VALUES ({}, {}, {}, {ts})", pg_str(eid), pg_str(usr), pg_str(item)) },
    }
}

/// The user's last 5 plays, as a search reads them (not a read by key).
fn last_plays(e: Eng, u: &str, usr: &str) -> R {
    match e {
        Eng::Strata => strata_sql(u, "SELECT id FROM events WHERE usr = :u ORDER BY ts DESC LIMIT 5".into(), json!({"u": usr})),
        Eng::Es | Eng::Os => post(format!("{u}/events/_search"), &json!({"size": 5, "_source": ["eid"], "query": {"term": {"usr": usr}}, "sort": [{"ts": "desc"}]})),
        Eng::Vespa => post(
            format!("{u}/search/"),
            &json!({"yql": format!("select eid from event where usr contains \"{usr}\" order by ts desc limit 5"), "ranking": "unranked", "presentation.summary": "minimal", "timeout": "10s"}),
        ),
        Eng::Qdrant => post(
            format!("{u}/collections/events/points/scroll"),
            &json!({"filter": {"must": [{"key": "usr", "match": {"value": usr}}]}, "limit": 5, "order_by": {"key": "ts", "direction": "desc"}, "with_payload": ["eid"]}),
        ),
        Eng::Milvus => post(format!("{u}/v2/vectordb/entities/query"), &json!({"collectionName": "events", "filter": format!("usr == \"{usr}\""), "outputFields": ["eid", "ts"], "limit": 1000})),
        Eng::Redis => redis_cmd(u, vec![b("FT.SEARCH"), b("events"), b(format!("@usr:{{{usr}}}")), b("SORTBY"), b("ts"), b("DESC"), b("LIMIT"), b("0"), b("5"), b("NOCONTENT"), b("DIALECT"), b("2")]),
        Eng::Postgres => R::Sql { url: u.to_string(), sql: format!("SELECT eid FROM events WHERE usr = {} ORDER BY ts DESC LIMIT 5", pg_str(usr)) },
    }
}

fn plays_of(e: Eng, j: &Json) -> Vec<String> {
    match e {
        Eng::Es | Eng::Os => j["hits"]["hits"].as_array().map(|h| h.iter().map(|x| id_text(&x["_source"]["eid"])).collect()).unwrap_or_default(),
        Eng::Qdrant => j["result"]["points"].as_array().map(|p| p.iter().map(|x| id_text(&x["payload"]["eid"])).collect()).unwrap_or_default(),
        Eng::Milvus => {
            let mut r: Vec<(String, i64)> = j["data"].as_array().into_iter().flatten().map(|x| (id_text(&x["eid"]), x["ts"].as_i64().unwrap_or(0))).collect();
            r.sort_by(|a, b| b.1.cmp(&a.1));
            r.into_iter().take(5).map(|x| x.0).collect()
        }
        _ => ranked(e, "r6", j),
    }
}

fn latency(ms: &mut [f64], errors: usize, first_error: Option<String>, seconds: f64) -> Json {
    ms.sort_by(|a, b| a.total_cmp(b));
    json!({"n": ms.len(), "qps": ms.len() as f64 / seconds.max(1e-9), "p50_ms": percentile(ms, 50.0), "p90_ms": percentile(ms, 90.0), "p99_ms": percentile(ms, 99.0),
        "mean_ms": mean(ms), "errors": errors, "first_error": first_error})
}

struct Writes {
    ms: Vec<f64>,
    errors: usize,
    first_error: Option<String>,
    fresh_ms: Vec<f64>,
    unseen: usize,
    sent: usize,
}

/// Plays written at `rate` a second for `seconds` (each planned in time, its latency from then), every quarter second one of them read back until it is in the user's last 5.
async fn events(e: Eng, base: &Conn, urls: Arc<Vec<String>>, users: Arc<Vec<String>>, items: Arc<Vec<String>>, rate: f64, seconds: f64, tag: String) -> Writes {
    let n = (rate * seconds) as usize;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<(usize, Instant)>();
    let rx = Arc::new(tokio::sync::Mutex::new(rx));
    let every = ((rate / 4.0) as usize).max(1);
    let workers: Vec<_> = (0..16)
        .map(|_| {
            let (mut c, rx, urls, users, items, tag) = (base.fork(), rx.clone(), urls.clone(), users.clone(), items.clone(), tag.clone());
            tokio::spawn(async move {
                let mut w = Writes { ms: vec![], errors: 0, first_error: None, fresh_ms: vec![], unseen: 0, sent: 0 };
                loop {
                    let next = rx.lock().await.recv().await;
                    let Some((i, at)) = next else { return w };
                    let u = &urls[i % urls.len()];
                    let usr = &users[i % users.len()];
                    let eid = format!("{tag}{i}");
                    let ts = (unix_s() * 1e3) as i64;
                    let r = write_event(e, u, &eid, usr, &items[(i * 7919) % items.len()], ts);
                    w.sent += 1;
                    match c.exec(&r).await {
                        Ok(_) => w.ms.push(at.elapsed().as_secs_f64() * 1e3),
                        Err(err) => {
                            w.errors += 1;
                            w.first_error.get_or_insert(err);
                            continue;
                        }
                    }
                    if i % every == 0 {
                        let acked = Instant::now();
                        let mut found = false;
                        while acked.elapsed() < Duration::from_secs(30) {
                            if let Ok(j) = c.exec(&last_plays(e, u, usr)).await {
                                if plays_of(e, &j).contains(&eid) {
                                    found = true;
                                    break;
                                }
                            }
                            tokio::time::sleep(Duration::from_millis(5)).await;
                        }
                        if found {
                            w.fresh_ms.push(acked.elapsed().as_secs_f64() * 1e3);
                        } else {
                            w.unseen += 1;
                        }
                    }
                }
            })
        })
        .collect();
    let t0 = Instant::now();
    for i in 0..n {
        let at = t0 + Duration::from_secs_f64(i as f64 / rate);
        tokio::time::sleep_until(at.into()).await;
        let _ = tx.send((i, at));
    }
    drop(tx);
    let mut all = Writes { ms: vec![], errors: 0, first_error: None, fresh_ms: vec![], unseen: 0, sent: 0 };
    for w in workers {
        let w = w.await.expect("event writer");
        all.ms.extend(w.ms);
        all.fresh_ms.extend(w.fresh_ms);
        all.errors += w.errors;
        all.unseen += w.unseen;
        all.sent += w.sent;
        if all.first_error.is_none() {
            all.first_error = w.first_error;
        }
    }
    all
}

// --- Running -------------------------------------------------------------------

/// The probes' plans from `concurrency` clients in turn for `seconds`.
async fn drive(e: Eng, kind: &'static str, base: &Conn, probes: Arc<Vec<Probe>>, model: Arc<Gbdt>, concurrency: usize, seconds: f64) -> Timed {
    let next = Arc::new(AtomicUsize::new(0));
    let deadline = Instant::now() + Duration::from_secs_f64(seconds);
    let t0 = Instant::now();
    let sources = probes.iter().map(|p| p.plan.sources()).max().unwrap_or(1);
    let tasks: Vec<_> = (0..concurrency)
        .map(|_| {
            let mut cs: Vec<Conn> = (0..sources).map(|_| base.fork()).collect();
            let (probes, next, model) = (probes.clone(), next.clone(), model.clone());
            tokio::spawn(async move {
                let mut t = Timed::default();
                while Instant::now() < deadline {
                    let p = &probes[next.fetch_add(1, Ordering::Relaxed) % probes.len()];
                    let start = Instant::now();
                    let res = perform(e, kind, &p.plan, &mut cs, &model, &p.cands).await;
                    let ms = start.elapsed().as_secs_f64() * 1e3;
                    match res {
                        Ok(_) => t.ms.push(ms),
                        Err(err) => {
                            t.errors += 1;
                            t.first_error.get_or_insert(err);
                        }
                    }
                }
                t
            })
        })
        .collect();
    let mut all = Timed::default();
    for task in tasks {
        let t = task.await.expect("client task");
        all.ms.extend(t.ms);
        all.errors += t.errors;
        if all.first_error.is_none() {
            all.first_error = t.first_error;
        }
    }
    all.seconds = t0.elapsed().as_secs_f64();
    all
}

/// Merges a stopped Strata server's database as `optimize` does, into as
/// few segments as [`strata::storage::MergePolicy::compact`] allows: the
/// state the other engines are timed in after their force merges, and the
/// one Strata's databases are built in for the other kinds. recsys-load's
/// writes leave the items in many segments, each searched on its own.
pub(crate) fn optimize(path: &Path, report: Option<&Path>) -> Result<(), String> {
    let t = Instant::now();
    let db = strata::db::Db::open(path, strata_server::server::db_options(path.is_dir())).map_err(|e| format!("{}: {e}", path.display()))?;
    let before = db.snapshot().segments().len();
    db.optimize().map_err(|e| e.to_string())?;
    let after = db.snapshot().segments().len();
    drop(db);
    let merge_s = t.elapsed().as_secs_f64();
    eprintln!("strata: {before} segments of items merged into {after} in {merge_s:.1} s");
    if let Some(p) = report {
        let mut r: Json = serde_json::from_str(&std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?).map_err(|e| e.to_string())?;
        r["steps"]["optimize"] = json!({"segments_before": before, "segments": after, "merge_s": merge_s});
        std::fs::write(p, serde_json::to_string_pretty(&r).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run(e: Eng, urls: &[String], dir: &Path, users: usize, concurrency: &[usize], seconds: f64, only: &[String], event_rate: f64, label: Option<String>, out: &Path) -> Result<(), String> {
    let d = read(dir, users)?;
    let t = Instant::now();
    let exacts: Vec<Exact> = d.users.iter().map(|u| exact(&d, u)).collect();
    eprintln!("exact answers in {:.1} s", t.elapsed().as_secs_f64());
    // The region's most popular, for an engine that cannot rank by a field.
    let mut by_region: HashMap<String, Arc<Vec<String>>> = HashMap::new();
    for u in &d.users {
        by_region.entry(u.region.clone()).or_insert_with(|| {
            let mut ix: Vec<usize> = (0..d.items.len()).filter(|&i| d.items[i].regions.contains(&u.region)).collect();
            ix.sort_by(|&a, &b| d.items[b].pop.total_cmp(&d.items[a].pop).then(d.items[a].id.cmp(&d.items[b].id)));
            Arc::new(ix.into_iter().take(R2_DEPTH + 500).map(|i| d.items[i].id.clone()).collect())
        });
    }
    let base = Conn::new(e, Duration::from_secs(60))?;
    let model = d.model.clone();
    let mut report = serde_json::Map::new();
    report.insert("engine".into(), json!(e.name()));
    report.insert("items".into(), json!(d.items.len()));
    report.insert("users".into(), json!(d.users.len()));
    report.insert("concurrency".into(), json!(concurrency));
    report.insert("seconds".into(), json!(seconds));
    let mut kinds = serde_json::Map::new();
    for &(kind, title) in KINDS {
        if !only.is_empty() && !only.iter().any(|k| k == kind) {
            continue;
        }
        let mut probes = Vec::new();
        for (i, (u, x)) in d.users.iter().zip(&exacts).enumerate() {
            let last = d.pos.get(&u.recent[0]).map(|&p| d.items[p].emb.as_slice()).unwrap_or(&u.vec);
            let cands: Vec<String> = x.near.iter().map(|&(p, _)| d.items[p].id.clone()).collect();
            let dots: HashMap<String, f32> = x.near.iter().map(|&(p, s)| (d.items[p].id.clone(), s)).collect();
            let ask = Ask { u, last, cands: cands.clone(), dots: Arc::new(dots), pop: by_region[&u.region].clone() };
            let Some(plan) = plan(e, kind, &ask, &urls[i % urls.len()]) else { break };
            let want = match kind {
                "r1" | "r6" => x.r1.clone(),
                "r2" => x.r2.clone(),
                "r3" => cands.clone(),
                "r4" => x.r4.clone(),
                _ => x.r5.clone(),
            };
            // Milvus's kept ranking leaves out what the user has seen: `perform` reads `cands` for it.
            let cands = if matches!(plan, Plan::Fuse(_, Some(_))) { u.seen.clone() } else { cands };
            let r4 = (kind == "r4").then(|| (Arc::new(x.r4_score.clone()), x.r4_cut));
            probes.push(Probe { plan, cands: Arc::new(cands), exact: want, r4 });
        }
        if probes.is_empty() {
            kinds.insert(kind.into(), json!({"title": title, "unavailable": "no request"}));
            continue;
        }
        let mut k = serde_json::Map::new();
        k.insert("title".into(), json!(title));
        if let Some(why) = differs(e, kind) {
            k.insert("differs".into(), json!(why));
        }
        // Once each, scored against the exact answers.
        let mut cs: Vec<Conn> = (0..3).map(|_| base.fork()).collect();
        let (mut quality, mut errors, mut first_error, mut over_cap) = (Vec::new(), 0, None, 0);
        for p in probes.iter() {
            match perform(e, kind, &p.plan, &mut cs, &model, &p.cands).await {
                Ok(got) => {
                    quality.push(score(kind, &got, &p.exact, p.r4.as_ref()));
                    if kind == "r5" {
                        let mut per: HashMap<&str, usize> = HashMap::new();
                        for id in &got {
                            if let Some(&i) = d.pos.get(id) {
                                *per.entry(d.items[i].artist.as_str()).or_default() += 1;
                            }
                        }
                        over_cap += per.values().filter(|&&n| n > R5_PER).count();
                    }
                }
                Err(err) => {
                    errors += 1;
                    first_error.get_or_insert(err);
                }
            }
        }
        let metric = match kind {
            "r3" => "rows_found",
            "r4" => "precision@50",
            "r5" => "recall@20",
            "r2" => "recall@1000",
            _ => "recall@500",
        };
        k.insert(metric.into(), json!(mean(&quality)));
        k.insert("answered".into(), json!(quality.len()));
        k.insert("answer_errors".into(), json!(errors));
        if let Some(err) = &first_error {
            k.insert("first_answer_error".into(), json!(err));
        }
        if kind == "r5" {
            k.insert("artists_over_cap".into(), json!(over_cap));
        }
        eprintln!("{} {kind}: {metric} {:.4} over {} ({} errors{})", e.name(), mean(&quality), quality.len(), errors, first_error.as_ref().map(|x| format!(": {}", short(&json!(x)))).unwrap_or_default());
        let probes = Arc::new(probes);
        drive(e, kind, &base, probes.clone(), model.clone(), 4, 2.0).await;
        let mut runs = Vec::new();
        for &c in concurrency {
            if kind == "r6" {
                let urls = Arc::new(urls.to_vec());
                let users: Arc<Vec<String>> = Arc::new(d.users.iter().map(|u| u.id.clone()).collect());
                let items: Arc<Vec<String>> = Arc::new(d.items.iter().map(|it| it.id.clone()).collect());
                let tag = format!("e{}c{c}x", (unix_s() * 1e3) as u64);
                let w = events(e, &base, urls, users, items, event_rate, seconds, tag);
                let r = drive(e, kind, &base, probes.clone(), model.clone(), c, seconds);
                let (mut w, mut r) = tokio::join!(w, r);
                let mut fresh = w.fresh_ms.clone();
                fresh.sort_by(|a, b| a.total_cmp(b));
                let reads = latency(&mut r.ms, r.errors, r.first_error.take(), r.seconds);
                let writes = latency(&mut w.ms, w.errors, w.first_error.take(), seconds);
                let (fresh50, fresh99) = (percentile(&fresh, 50.0), percentile(&fresh, 99.0));
                eprintln!(
                    "{} r6 at {c}: r1 p50 {:.2} p99 {:.2} ms; writes p99 {:.2} ms; fresh p50 {fresh50:.0} p99 {fresh99:.0} ms ({} unseen)",
                    e.name(),
                    reads["p50_ms"].as_f64().unwrap_or(0.0),
                    reads["p99_ms"].as_f64().unwrap_or(0.0),
                    writes["p99_ms"].as_f64().unwrap_or(0.0),
                    w.unseen
                );
                runs.push(json!({"concurrency": c, "reads": reads, "writes": writes, "event_rate": event_rate, "sent": w.sent,
                    "fresh": {"samples": fresh.len(), "unseen": w.unseen, "p50_ms": fresh50, "p99_ms": fresh99, "max_ms": fresh.last().copied().unwrap_or(f64::NAN)}}));
            } else {
                let mut r = drive(e, kind, &base, probes.clone(), model.clone(), c, seconds).await;
                let l = latency(&mut r.ms, r.errors, r.first_error.take(), r.seconds);
                eprintln!("{} {kind} at {c}: {:.0} req/s, p50 {:.2} ms, p99 {:.2} ms, {} errors", e.name(), l["qps"].as_f64().unwrap_or(0.0), l["p50_ms"].as_f64().unwrap_or(0.0), l["p99_ms"].as_f64().unwrap_or(0.0), l["errors"]);
                runs.push(json!({"concurrency": c, "latency": l}));
            }
        }
        k.insert("runs".into(), json!(runs));
        kinds.insert(kind.into(), Json::Object(k));
    }
    report.insert("kinds".into(), Json::Object(kinds));
    report.insert("at".into(), json!(unix_s()));
    std::fs::create_dir_all(out).map_err(s)?;
    let name = match &label {
        Some(l) => format!("recsys-run-{}-{l}.json", e.name()),
        None => format!("recsys-run-{}.json", e.name()),
    };
    let path = out.join(name);
    std::fs::write(&path, serde_json::to_string_pretty(&Json::Object(report)).unwrap()).map_err(s)?;
    eprintln!("{}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rrf_sums_reciprocal_ranks() {
        let a = vec!["x".to_string(), "y".to_string()];
        let b = vec!["y".to_string(), "z".to_string()];
        // y: 1/62 + 1/61; x: 1/61; z: 1/62.
        assert_eq!(rrf(&[&a, &b], 60.0, 3), vec!["y", "x", "z"]);
    }

    #[test]
    fn diversify_keeps_the_first_of_each_artist() {
        let r = [("1", "a"), ("2", "a"), ("3", "a"), ("4", "b"), ("5", "a")];
        assert_eq!(diversify(r.iter().copied(), 2, 3), vec!["1", "2", "4"]);
    }

    #[test]
    fn latency_reads_percentiles_in_percent() {
        let mut ms: Vec<f64> = (1..=100).rev().map(f64::from).collect();
        let l = latency(&mut ms, 0, None, 2.0);
        assert_eq!(l["p50_ms"], json!(51.0));
        assert_eq!(l["p99_ms"], json!(99.0));
        assert_eq!(l["qps"], json!(50.0));
    }

    #[test]
    fn redis_rows_read_keys_and_fields() {
        let j = json!([2, "i:7", ["d", "0.1", "artist", "a3"], "i:9", ["d", "0.2", "artist", "a1"]]);
        let r = redis_rows(&j);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].0, "7");
        assert_eq!(r[1].1["artist"], "a1");
    }
}
