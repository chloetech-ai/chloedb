//! The follower: copies a publication's tables into Strata, then applies
//! the slot's stream of changes, a moment behind Postgres
//! ([`super`] describes what it keeps and how a restart resumes).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};
use chloedb::query::sql::ident;
use chloedb::schema::Schema;

use super::pgoutput::{self, lsn_text, parse_lsn, Datum, Frame, Lsn, Message, Old};
use super::types::{copy_fields, json_of, strata_type, PgType};
use super::wire::{self, literal, Conn};
use crate::follow::{again, retry, Http};
use crate::ApiError;

/// What `chloedb-server follow-postgres` is told.
#[derive(Clone, Debug)]
pub struct Options {
    /// The database, as a `postgres://` URL.
    pub source: String,
    /// PEM certificates its TLS certificate is checked against.
    pub source_ca: Option<Vec<u8>>,
    /// The publication whose tables it follows.
    pub publication: String,
    /// The replication slot's name (`strata_<name>` by default).
    pub slot: Option<String>,
    /// Strata's HTTP URL: a server, or a cluster's coordinator.
    pub target: String,
    pub target_token: Option<String>,
    pub target_ca: Option<Vec<u8>>,
    /// The name Strata knows the database by, which its marks name.
    pub name: String,
    /// The most operations of one write while copying, and of
    /// transactions written together.
    pub batch: usize,
    /// Copy the tables again, whatever Strata kept.
    pub copy: bool,
    /// How many tables are copied at once.
    pub copy_workers: usize,
    /// For tests: once this many more writes are done (when set above 0),
    /// the follower stops as if killed.
    pub crash_after: Option<Arc<AtomicU64>>,
}

/// The default `--batch`.
pub const BATCH: usize = 1000;

/// A transaction of more operations than this, or values of more bytes
/// than [`STEP_BYTES`], is written in steps of that size. Fixed, rather
/// than `--batch`, so that a transaction replayed splits where it split
/// before (a step's operations carry its number).
const STEP: usize = 1000;
const STEP_BYTES: usize = 16 << 20;

/// The most bytes of one write while copying.
const COPY_BYTES: usize = 16 << 20;

/// How often it tells Postgres how far it applied (well within
/// `wal_sender_timeout`, 60 s by default).
const STATUS_EVERY: Duration = Duration::from_secs(10);

/// How often it looks for tables added to the publication, or taken out.
const POLL_EVERY: Duration = Duration::from_secs(30);

/// How often it logs how it stands.
const LOG_EVERY: Duration = Duration::from_secs(30);

/// The key column of a table whose Postgres key has several columns: their
/// values joined by `:`, with `:` and `\` escaped by a `\`.
pub const COMPOSITE_KEY: &str = "_key";

/// The replication slot's name for follower `name`.
pub fn default_slot(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    format!("strata_{s}").chars().take(63).collect()
}

/// The names of what a follower keeps in `_sources`.
struct Names {
    name: String,
}

impl Names {
    fn copy(&self) -> String {
        format!("postgres/{}/copy", self.name)
    }
    fn applied(&self, c: u64) -> String {
        format!("postgres/{}/{c}", self.name)
    }
    fn step(&self, c: u64) -> String {
        format!("postgres/{}/{c}/step", self.name)
    }
    fn base(&self, c: u64) -> String {
        format!("postgres/{}/{c}/base", self.name)
    }
    fn copied(&self, c: u64) -> String {
        format!("postgres/{}/{c}/copied", self.name)
    }
    fn table(&self, c: u64, oid: u32, table: &str) -> String {
        format!("postgres/{}/{c}/t/{oid}/{table}", self.name)
    }
}

fn mark(name: String, offset: i64) -> Json {
    json!({"op": "source", "source": {"name": name, "offset": offset}})
}

/// What Strata kept of a follower, the least over its shards.
#[derive(Debug, Default)]
struct Kept {
    copy: Option<u64>,
    applied: Option<Lsn>,
    step: i64,
    base: i64,
    copied: Option<Lsn>,
    /// The tables followed (by Postgres OID): the Strata table, and
    /// whether still followed (rather than taken out of the publication).
    tables: BTreeMap<u32, (String, bool, i64)>,
}

fn pg(e: wire::Error) -> ApiError {
    // A slot still held by the connection of a follower just gone (or of
    // another): its walsender lets go of it soon.
    let again = e.io || e.is("55006");
    ApiError { code: None, status: if again { 503 } else { 400 }, message: e.message }
}

/// A table as the follower writes it: a Postgres relation and its Strata table.
#[derive(Clone, Debug)]
struct Rel {
    oid: u32,
    /// `schema.table` in Postgres.
    shown: String,
    table: String,
    columns: Vec<pgoutput::Column>,
    types: Vec<PgType>,
    /// The key's columns (indexes into `columns`).
    key: Vec<usize>,
    /// Each column's number in the table, as the catalog has it now (by
    /// which a column renamed is told from one dropped and one added).
    attnums: Vec<Option<i64>>,
    /// For a copy: its publication's row filter (Postgres 15).
    filter: Option<String>,
    schema_name: String,
    rel_name: String,
}

impl Rel {
    fn key_name(&self) -> String {
        match self.key.as_slice() {
            [k] => self.columns[*k].name.clone(),
            _ => COMPOSITE_KEY.to_string(),
        }
    }
}

/// The Strata table a Postgres table becomes: its name, but for a
/// schema other than `public`, `schema_table`.
fn table_name(schema: &str, name: &str) -> String {
    match schema {
        "public" | "" => name.to_string(),
        s => format!("{s}_{name}"),
    }
}

/// The composite key's text.
fn join_key(parts: &[&str]) -> String {
    parts.iter().map(|p| p.replace('\\', "\\\\").replace(':', "\\:")).collect::<Vec<_>>().join(":")
}

/// A row's values as a write's document for `schema`; `fill` gives a value
/// the stream left out (unchanged TOAST), by column name.
fn document(rel: &Rel, schema: &Schema, tuple: &[Datum], mut fill: impl FnMut(&str) -> Option<Json>) -> Result<serde_json::Map<String, Json>, String> {
    let mut doc = serde_json::Map::new();
    for (i, c) in rel.columns.iter().enumerate() {
        let Some(fi) = schema.field_index(&c.name) else { continue };
        let f = &schema.fields[fi];
        let v = match tuple.get(i) {
            None | Some(Datum::Null) => Json::Null,
            Some(Datum::Text(t)) => json_of(f, &rel.types[i], t).map_err(|e| format!("column {} of {}: {e}", c.name, rel.shown))?,
            Some(Datum::Unchanged) => match fill(&f.name) {
                Some(v) => v,
                None => continue,
            },
        };
        doc.insert(f.name.clone(), v);
    }
    if rel.key.len() > 1 {
        doc.insert(COMPOSITE_KEY.to_string(), json!(composite(rel, tuple)?));
    }
    Ok(doc)
}

fn composite(rel: &Rel, tuple: &[Datum]) -> Result<String, String> {
    let mut parts = Vec::new();
    for &k in &rel.key {
        match tuple.get(k) {
            Some(Datum::Text(t)) => parts.push(t.as_str()),
            _ => return Err(format!("a row of {} without its key column {}", rel.shown, rel.columns[k].name)),
        }
    }
    Ok(join_key(&parts))
}

/// A row's key as a write's `key`.
fn key_of(rel: &Rel, schema: &Schema, tuple: &[Datum]) -> Result<Json, String> {
    if rel.key.len() > 1 {
        return Ok(json!(composite(rel, tuple)?));
    }
    let k = rel.key[0];
    let c = &rel.columns[k];
    let fi = schema.field_index(&c.name).ok_or_else(|| format!("table {} here has no column {}, its key", rel.table, c.name))?;
    match tuple.get(k) {
        Some(Datum::Text(t)) => json_of(&schema.fields[fi], &rel.types[k], t).map_err(|e| format!("the key of {}: {e}", rel.shown)),
        _ => Err(format!("a row of {} without its key {}", rel.shown, c.name)),
    }
}

/// The transaction being read from the stream.
#[derive(Debug, Default)]
struct Open {
    /// Its steps written so far ([`STEP`]).
    steps: i64,
    /// Its operations, and their values' bytes, in the step being built.
    ops: usize,
    bytes: usize,
}

/// Operations read and not written yet: whole transactions, then the
/// current step of the one open.
#[derive(Debug, Default)]
struct Buffer {
    ops: Vec<Json>,
    /// Where the open transaction's step begins in `ops`.
    from: usize,
    /// The end of the last whole transaction in `ops`, and the step base
    /// once they are written (as [`Follower::base`]).
    end: Option<Lsn>,
    base: Option<i64>,
    /// The last upsert of each row in `ops` (table, key) at its index.
    rows: HashMap<(String, String), usize>,
}

/// Stream statistics, for the log.
#[derive(Debug, Default)]
struct Stats {
    txns: u64,
    changes: u64,
    writes: u64,
    copied: u64,
    skipped: u64,
}

/// The follower.
struct Follower {
    o: Options,
    cfg: wire::Config,
    slot: String,
    names: Names,
    dst: Http,
    /// For the catalog, the copies of tables added later, and dropping
    /// the slot.
    catalog: Option<Conn>,
    copy: u64,
    /// Strata's tables, by name in lower case.
    tables: HashMap<String, Arc<Schema>>,
    /// The tables followed, by OID: Strata's name.
    followed: BTreeMap<u32, String>,
    types: HashMap<u32, PgType>,
    /// This session's relations, from the stream.
    rels: HashMap<u32, Arc<Rel>>,
    /// The end of the last transaction written, and the step base: the
    /// steps of transactions written in several ([`STEP`]) so far.
    written: Lsn,
    base: i64,
    /// What it tells Postgres it flushed.
    flushed: Arc<AtomicU64>,
    /// The end of the server's WAL, as last heard.
    server_end: Lsn,
    /// The shards' numbers from its last write, which a read waits for.
    seen: Vec<u64>,
    stats: Stats,
    polled: Instant,
    logged: Instant,
}

/// Follows `o.source` into `o.target` until it fails for good.
pub async fn run(o: Options) -> Result<(), ApiError> {
    let mut f = Follower::new(o).await?;
    f.check().await?;
    let kept = f.kept().await?;
    f.load_tables().await?;
    let slot = f.slot_free().await?;
    let resumable = kept.copy.is_some() && kept.copied.is_some() && kept.applied.is_some();
    let start = match (resumable && !f.o.copy, &slot) {
        (true, Some(s)) if !s.lost => {
            f.copy = kept.copy.unwrap_or(0);
            f.base = kept.base;
            f.followed = kept.tables.iter().filter(|(_, (_, on, _))| *on).map(|(oid, (t, ..))| (*oid, t.clone())).collect();
            let at = kept.applied.unwrap_or(0);
            f.written = at;
            tracing::info!(slot = %f.slot, at = %lsn_text(at), tables = f.followed.len(), "resuming from what ChloeDB applied");
            None
        }
        (resume, s) => {
            if resume {
                match s {
                    None => tracing::warn!(slot = %f.slot, "the replication slot is gone (dropped by hand?): copying the tables again"),
                    Some(_) => tracing::warn!(slot = %f.slot, "the replication slot lost WAL it needed (max_slot_wal_keep_size): copying the tables again"),
                }
            }
            Some(f.copy_all(kept.copy, s.is_some()).await?)
        }
    };
    f.follow(start).await
}

/// Drops the follower's replication slot, so Postgres keeps no WAL for it.
pub async fn drop_slot(o: Options) -> Result<Json, ApiError> {
    let mut f = Follower::new(o).await?;
    let found = f.slot_free().await?.is_some();
    if found {
        f.sql_pg(&format!("SELECT pg_drop_replication_slot({})", literal(&f.slot))).await?;
    }
    Ok(json!({"slot": f.slot, "dropped": found}))
}

#[derive(Debug)]
struct SlotState {
    lost: bool,
}

impl Follower {
    async fn new(o: Options) -> Result<Follower, ApiError> {
        if !crate::service::follow_name_ok(&o.name) {
            return Err(ApiError::bad("--name is 1 to 64 letters, digits, '.', '_' or '-'"));
        }
        let mut cfg = wire::Config::parse(&o.source).map_err(pg)?;
        if let Some(ca) = &o.source_ca {
            cfg.root_cert = Some(ca.clone());
            if cfg.ssl == wire::SslMode::Prefer {
                cfg.ssl = wire::SslMode::Require;
            }
        }
        if cfg.application_name == "chloedb-server" {
            cfg.application_name = format!("strata follow {}", o.name);
        }
        let slot = o.slot.clone().unwrap_or_else(|| default_slot(&o.name));
        if slot.is_empty() || slot.len() > 63 || !slot.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') {
            return Err(ApiError::bad(format!("--slot {slot:?}: a slot's name is 1 to 63 lower-case letters, digits and '_'")));
        }
        let dst = Http::new(&o.target, o.target_token.as_deref(), o.target_ca.as_deref(), None)?;
        Ok(Follower {
            names: Names { name: o.name.clone() },
            cfg,
            slot,
            dst,
            catalog: None,
            copy: 0,
            tables: HashMap::new(),
            followed: BTreeMap::new(),
            types: HashMap::new(),
            rels: HashMap::new(),
            written: 0,
            base: 0,
            flushed: Arc::new(AtomicU64::new(0)),
            server_end: 0,
            seen: Vec::new(),
            stats: Stats::default(),
            polled: Instant::now(),
            logged: Instant::now(),
            o,
        })
    }

    /// The catalog connection, connected again if it was lost.
    async fn conn(&mut self) -> Result<&mut Conn, ApiError> {
        if self.catalog.is_none() {
            self.catalog = Some(Conn::connect(&self.cfg, false).await.map_err(pg)?);
        }
        Ok(self.catalog.as_mut().expect("connected"))
    }

    /// Runs `sql` on the catalog connection (connecting again once if it was lost).
    async fn sql_pg(&mut self, sql: &str) -> Result<wire::Rows, ApiError> {
        for attempt in 0..2 {
            let r = self.conn().await?.query(sql).await;
            match r {
                Err(e) if e.io && attempt == 0 => {
                    tracing::warn!("the catalog connection failed ({e}): connecting again");
                    self.catalog = None;
                }
                r => return r.map_err(pg),
            }
        }
        unreachable!()
    }

    /// Checks that Postgres can be followed as asked.
    async fn check(&mut self) -> Result<(), ApiError> {
        let c = self.conn().await?;
        let (version, encoding) = (c.version, c.encoding.clone());
        if version < 100000 {
            return Err(ApiError::bad(format!("{} runs Postgres {}: logical replication with pgoutput needs 10 or later", self.cfg.shown(), version / 10000)));
        }
        if !matches!(encoding.as_str(), "UTF8" | "SQL_ASCII") {
            return Err(ApiError::bad(format!("{}'s encoding is {encoding}: the follower reads UTF8 databases", self.cfg.shown())));
        }
        let level = self.sql_pg("SHOW wal_level").await?;
        if level.rows.first().and_then(|r| r[0].as_deref()) != Some("logical") {
            return Err(ApiError::bad(format!(
                "{}'s wal_level is {}: set it to logical (ALTER SYSTEM SET wal_level = logical, then restart Postgres)",
                self.cfg.shown(),
                level.rows.first().and_then(|r| r[0].clone()).unwrap_or_default()
            )));
        }
        let p = self.sql_pg(&format!("SELECT 1 FROM pg_publication WHERE pubname = {}", literal(&self.o.publication))).await?;
        if p.rows.is_empty() {
            return Err(ApiError::bad(format!(
                "{} has no publication {:?}: make it, as CREATE PUBLICATION {} FOR TABLE items, users; (or FOR ALL TABLES)",
                self.cfg.shown(),
                self.o.publication,
                wire::ident(&self.o.publication)
            )));
        }
        Ok(())
    }

    /// The slot as Postgres has it: `None` if there is none.
    async fn slot_state(&mut self) -> Result<Option<SlotState>, ApiError> {
        let v = self.conn().await?.version;
        let wal_status = if v >= 130000 { "wal_status" } else { "NULL" };
        let r = self.sql_pg(&format!("SELECT plugin, database, active_pid, {wal_status} AS wal_status FROM pg_replication_slots WHERE slot_name = {}", literal(&self.slot))).await?;
        if r.rows.is_empty() {
            return Ok(None);
        }
        if r.get(0, "plugin") != Some("pgoutput") || r.get(0, "database") != Some(self.cfg.database.as_str()) {
            return Err(ApiError {
                code: None, status: 409,
                message: format!("slot {} is another's (plugin {:?}, database {:?}): give this follower another (--slot)", self.slot, r.get(0, "plugin"), r.get(0, "database")),
            });
        }
        if let Some(pid) = r.get(0, "active_pid") {
            return Err(ApiError { code: None, status: 409, message: format!("slot {} is in use by another connection (process {pid}): is another follower of {} running?", self.slot, self.o.name) });
        }
        Ok(Some(SlotState { lost: r.get(0, "wal_status") == Some("lost") }))
    }

    /// [`Self::slot_state`], waiting a while for a slot still in use: the
    /// connection of a follower just stopped holds it a moment longer.
    async fn slot_free(&mut self) -> Result<Option<SlotState>, ApiError> {
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            match self.slot_state().await {
                Err(e) if e.status == 409 && e.message.contains("in use") && Instant::now() < until => {
                    tracing::info!("{}: waiting", e.message);
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                r => return r,
            }
        }
    }

    /// What Strata kept of this follower.
    async fn kept(&self) -> Result<Kept, ApiError> {
        let j = retry("ChloeDB's stream sources", || self.dst.get("/v1/sources")).await?;
        let all: Vec<(String, i64)> = j["sources"].as_array().into_iter().flatten().filter_map(|s| Some((s["name"].as_str()?.to_string(), s["offset"].as_i64()?))).collect();
        let get = |n: &str| all.iter().find(|(name, _)| name == n).map(|(_, o)| *o);
        let mut k = Kept { copy: get(&self.names.copy()).map(|c| c as u64), ..Default::default() };
        let Some(c) = k.copy else { return Ok(k) };
        k.applied = get(&self.names.applied(c)).map(|o| o as Lsn);
        k.step = get(&self.names.step(c)).unwrap_or(0);
        k.base = get(&self.names.base(c)).unwrap_or(0);
        k.copied = get(&self.names.copied(c)).map(|o| o as Lsn);
        let prefix = format!("postgres/{}/{c}/t/", self.o.name);
        for (name, offset) in &all {
            let Some(rest) = name.strip_prefix(&prefix) else { continue };
            let Some((oid, table)) = rest.split_once('/') else { continue };
            let Ok(oid) = oid.parse::<u32>() else { continue };
            let e = k.tables.entry(oid).or_insert((table.to_string(), false, i64::MIN));
            if *offset > e.2 {
                *e = (table.to_string(), offset % 2 == 1, *offset);
            }
        }
        Ok(k)
    }

    /// Strata's tables, read again.
    async fn load_tables(&mut self) -> Result<(), ApiError> {
        let j = retry("ChloeDB's tables", || self.dst.get("/v1/tables")).await?;
        self.tables = j["tables"].as_array().into_iter().flatten().filter_map(|t| serde_json::from_value::<Schema>(t["schema"].clone()).ok()).map(|s| (s.table.to_ascii_lowercase(), Arc::new(s))).collect();
        Ok(())
    }

    /// Runs `sql` on Strata.
    async fn sql(&self, sql: &str) -> Result<Json, ApiError> {
        let body = json!({"sql": sql});
        retry("a statement on ChloeDB", || self.dst.post("/v1/sql", &body)).await
    }

    /// The types of these OIDs, read from the catalog unless known.
    async fn load_types(&mut self, oids: &[u32]) -> Result<(), ApiError> {
        let want: Vec<String> = oids.iter().filter(|o| !self.types.contains_key(o)).map(|o| o.to_string()).collect::<HashSet<_>>().into_iter().collect();
        if want.is_empty() {
            return Ok(());
        }
        // Through a domain to its base type; an array's element (through
        // a domain too).
        let r = self
            .sql_pg(&format!(
                "SELECT t.oid::text AS oid, COALESCE(b.typname, t.typname) AS name, COALESCE(b.typtype, t.typtype)::text AS kind, \
                 COALESCE(eb.typname, e.typname) AS elem, COALESCE(eb.typtype, e.typtype)::text AS elem_kind \
                 FROM pg_type t \
                 LEFT JOIN pg_type b ON t.typtype = 'd' AND b.oid = t.typbasetype \
                 LEFT JOIN pg_type e ON e.oid = COALESCE(b.typelem, t.typelem) AND COALESCE(b.typcategory, t.typcategory) = 'A' \
                 LEFT JOIN pg_type eb ON e.typtype = 'd' AND eb.oid = e.typbasetype \
                 WHERE t.oid IN ({})",
                want.join(",")
            ))
            .await?;
        for i in 0..r.rows.len() {
            let Some(oid) = r.get(i, "oid").and_then(|o| o.parse::<u32>().ok()) else { continue };
            let elem = r.get(i, "elem").map(|e| Box::new(PgType { name: e.to_string(), enumerated: r.get(i, "elem_kind") == Some("e"), elem: None }));
            self.types.insert(oid, PgType { name: r.get(i, "name").unwrap_or_default().to_string(), enumerated: r.get(i, "kind") == Some("e"), elem });
        }
        Ok(())
    }

    /// The key columns of table `oid`, by name: its replica identity's
    /// index (by default its primary key), or with `REPLICA IDENTITY FULL`
    /// its primary key.
    async fn key_columns(&mut self, oid: u32, shown: &str) -> Result<Vec<String>, ApiError> {
        let r = self.sql_pg(&format!("SELECT relreplident::text AS ident FROM pg_class WHERE oid = {oid}")).await?;
        let ident = r.get(0, "ident").unwrap_or("d").to_string();
        let index = |which: &str| {
            format!(
                "SELECT a.attname FROM pg_index i JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey) \
                 WHERE i.indrelid = {oid} AND i.{which} ORDER BY array_position(i.indkey::int2[], a.attnum)"
            )
        };
        let cols = match ident.as_str() {
            "i" => self.sql_pg(&index("indisreplident")).await?,
            "n" => return Err(ApiError::bad(format!("{shown} has REPLICA IDENTITY NOTHING, so Postgres publishes no key of its updates and deletes: give it a primary key, or ALTER TABLE {shown} REPLICA IDENTITY USING INDEX ..."))),
            _ => self.sql_pg(&index("indisprimary")).await?,
        };
        let names: Vec<String> = cols.rows.iter().filter_map(|r| r[0].clone()).collect();
        if names.is_empty() {
            return Err(ApiError::bad(format!(
                "{shown} has no primary key, and a ChloeDB table keeps its rows by one: add one (ALTER TABLE {shown} ADD PRIMARY KEY (...)), or name a unique index (REPLICA IDENTITY USING INDEX ...)"
            )));
        }
        Ok(names)
    }

    /// A relation as the follower writes it, from the stream's columns.
    async fn rel_of(&mut self, oid: u32, schema: &str, name: &str, columns: Vec<pgoutput::Column>, filter: Option<String>) -> Result<Rel, ApiError> {
        let shown = format!("{}.{}", wire::ident(schema), wire::ident(name));
        let keys = self.key_columns(oid, &shown).await?;
        let mut key = Vec::new();
        for k in &keys {
            let i = columns.iter().position(|c| &c.name == k).ok_or_else(|| ApiError::bad(format!("the publication leaves out column {k} of {shown}, its key: publish it")))?;
            key.push(i);
        }
        let oids: Vec<u32> = columns.iter().map(|c| c.type_oid).collect();
        self.load_types(&oids).await?;
        let types = columns.iter().map(|c| self.types.get(&c.type_oid).cloned().unwrap_or_else(|| PgType::named("unknown"))).collect();
        let table = self.followed.get(&oid).cloned().unwrap_or_else(|| table_name(schema, name));
        if key.len() > 1 && columns.iter().any(|c| c.name == COMPOSITE_KEY) {
            return Err(ApiError::bad(format!("{shown} has a column {COMPOSITE_KEY}, the name the follower gives the key of a table keyed by several columns")));
        }
        let nums = self.sql_pg(&format!("SELECT attname, attnum::text AS n FROM pg_attribute WHERE attrelid = {oid} AND attnum > 0 AND NOT attisdropped")).await?;
        let num: HashMap<&str, i64> = (0..nums.rows.len()).filter_map(|i| Some((nums.get(i, "attname")?, nums.get(i, "n")?.parse().ok()?))).collect();
        let attnums = columns.iter().map(|c| num.get(c.name.as_str()).copied()).collect();
        Ok(Rel { oid, shown, table, columns, types, key, attnums, filter, schema_name: schema.to_string(), rel_name: name.to_string() })
    }

    /// The publication's tables now, from the catalog, with their columns.
    async fn publication(&mut self) -> Result<Vec<Rel>, ApiError> {
        let v = self.conn().await?.version;
        let extra = if v >= 150000 { ", p.attnames::text AS attnames, p.rowfilter" } else { "" };
        let r = self
            .sql_pg(&format!(
                "SELECT c.oid::text AS oid, p.schemaname, p.tablename{extra} FROM pg_publication_tables p \
                 JOIN pg_namespace n ON n.nspname = p.schemaname JOIN pg_class c ON c.relnamespace = n.oid AND c.relname = p.tablename \
                 WHERE p.pubname = {} ORDER BY p.schemaname, p.tablename",
                literal(&self.o.publication)
            ))
            .await?;
        let mut out = Vec::new();
        for i in 0..r.rows.len() {
            let oid: u32 = r.get(i, "oid").and_then(|o| o.parse().ok()).unwrap_or(0);
            let (schema, name) = (r.get(i, "schemaname").unwrap_or_default().to_string(), r.get(i, "tablename").unwrap_or_default().to_string());
            let only: Option<Vec<String>> = r.get(i, "attnames").map(|a| super::types::array_items(a).unwrap_or_default().into_iter().flatten().collect());
            let generated = if v >= 120000 { " AND attgenerated = ''" } else { "" };
            let cols = self.sql_pg(&format!("SELECT attname, atttypid::text AS t, atttypmod::text AS m FROM pg_attribute WHERE attrelid = {oid} AND attnum > 0 AND NOT attisdropped{generated} ORDER BY attnum")).await?;
            let columns: Vec<pgoutput::Column> = (0..cols.rows.len())
                .map(|j| pgoutput::Column {
                    key: false,
                    name: cols.get(j, "attname").unwrap_or_default().to_string(),
                    type_oid: cols.get(j, "t").and_then(|t| t.parse().ok()).unwrap_or(0),
                    typmod: cols.get(j, "m").and_then(|t| t.parse().ok()).unwrap_or(-1),
                })
                .filter(|c| only.as_ref().is_none_or(|o| o.contains(&c.name)))
                .collect();
            let filter = r.get(i, "rowfilter").map(String::from);
            out.push(self.rel_of(oid, &schema, &name, columns, filter).await?);
        }
        Ok(out)
    }

    /// Makes `rel`'s Strata table, or adds the columns it lacks; answers
    /// whether the table was there.
    async fn ensure_table(&mut self, rel: &Rel) -> Result<bool, ApiError> {
        let key = rel.key_name();
        // The key's columns are keys' types, of a composite key too.
        let decl = |i: usize| strata_type(&rel.types[i], rel.columns[i].typmod, rel.key.contains(&i));
        let Some(have) = self.tables.get(&rel.table.to_ascii_lowercase()).cloned() else {
            let mut cols = Vec::new();
            if rel.key.len() > 1 {
                cols.push(format!("{} KEYWORD PRIMARY KEY", ident(COMPOSITE_KEY)));
            }
            for (i, c) in rel.columns.iter().enumerate() {
                match decl(i) {
                    Some(t) if rel.key.len() == 1 && rel.key[0] == i => cols.push(format!("{} {t} PRIMARY KEY", ident(&c.name))),
                    Some(t) => cols.push(format!("{} {t}", ident(&c.name))),
                    None => tracing::warn!(table = %rel.shown, column = %c.name, pg_type = %rel.types[i].name, "a column of a type the follower leaves out (make the table in ChloeDB first to choose its type)"),
                }
            }
            let sql = format!("CREATE TABLE {} ({})", ident(&rel.table), cols.join(", "));
            tracing::info!(table = %rel.table, "making the table: {sql}");
            let made = self.sql(&sql).await;
            self.load_tables().await?;
            // Tried again after its answer was lost (a 5xx, a server that
            // stopped as it answered), it finds the table its first try
            // made: there, it is made.
            if let Err(e) = made {
                if !self.tables.contains_key(&rel.table.to_ascii_lowercase()) || !e.message.contains("already exists") {
                    return Err(e);
                }
                tracing::info!(table = %rel.table, "making the table failed ({}), but it is here: an earlier try made it", e.message);
            }
            return Ok(false);
        };
        let pk = have.primary_key.map(|k| have.fields[k].name.clone());
        if !pk.as_deref().is_some_and(|p| p.eq_ignore_ascii_case(&key)) {
            return Err(ApiError {
                code: None, status: 409,
                message: format!("table {} here is keyed by {}, and {} by {key}: drop it here, or make it with the key {key}", rel.table, pk.unwrap_or_else(|| "nothing".into()), rel.shown),
            });
        }
        let mut changed = false;
        for (i, c) in rel.columns.iter().enumerate() {
            if have.field_index(&c.name).is_some() {
                continue;
            }
            if let Some(t) = decl(i) {
                let sql = format!("ALTER TABLE {} ADD COLUMN {} {t}", ident(&rel.table), ident(&c.name));
                tracing::info!(table = %rel.table, "Postgres has a column the table lacks: {sql}");
                if let Err(e) = self.sql(&sql).await {
                    // An earlier try, its answer lost, may have added it.
                    self.load_tables().await?;
                    if !self.tables.get(&rel.table.to_ascii_lowercase()).is_some_and(|t| t.field_index(&c.name).is_some()) {
                        return Err(e);
                    }
                }
                changed = true;
            }
        }
        if changed {
            self.load_tables().await?;
        }
        Ok(true)
    }

    fn schema(&self, table: &str) -> Result<Arc<Schema>, ApiError> {
        self.tables.get(&table.to_ascii_lowercase()).cloned().ok_or_else(|| ApiError { code: None, status: 409, message: format!("table {table} is gone from ChloeDB: copy again (--copy)") })
    }

    /// Writes `ops` until Strata takes them.
    async fn write(&mut self, ops: Vec<Json>) -> Result<Json, ApiError> {
        if ops.is_empty() {
            return Ok(json!({}));
        }
        let body = json!({"ops": ops});
        let mut wait = Duration::from_millis(200);
        let j = loop {
            match self.dst.post("/v1/write", &body).await {
                Ok(j) => break j,
                Err(e) if again(&e) => {
                    tracing::warn!(status = e.status, "writing to ChloeDB: {}; trying again", e.message);
                    tokio::time::sleep(wait).await;
                    wait = (wait * 2).min(Duration::from_secs(5));
                }
                Err(e) => return Err(ApiError { code: None, status: e.status, message: format!("writing to ChloeDB: {}", e.message) }),
            }
        };
        self.stats.writes += 1;
        self.stats.skipped += j["skipped"].as_u64().unwrap_or(0);
        if let Some(c) = &self.o.crash_after {
            if c.load(Ordering::SeqCst) > 0 && c.fetch_sub(1, Ordering::SeqCst) == 1 {
                return Err(ApiError { code: None, status: 500, message: "stopped, as a test asked".into() });
            }
        }
        if let Some(s) = j["seq_nos"].as_array() {
            let s: Vec<u64> = s.iter().map(|v| v.as_u64().unwrap_or(0)).collect();
            if s.len() != self.seen.len() {
                self.seen = s;
            } else {
                for (a, b) in self.seen.iter_mut().zip(s) {
                    *a = (*a).max(b);
                }
            }
        } else if let Some(n) = j["seq_no"].as_u64() {
            self.seen = vec![self.seen.first().copied().unwrap_or(0).max(n)];
        }
        Ok(j)
    }

    /// Copies every table of the publication at the slot's snapshot (made
    /// anew), and answers the replication connection and where its stream
    /// starts.
    async fn copy_all(&mut self, previous: Option<u64>, slot_exists: bool) -> Result<(Conn, Lsn), ApiError> {
        self.copy = crate::telemetry::now_ms().max(previous.map_or(0, |p| p + 1));
        // The tables first, so that one Strata cannot key stops the copy
        // before a slot keeps WAL for it. (One that joins the publication
        // meanwhile is copied when the stream names it.)
        let rels = self.publication().await?;
        let mut names: HashMap<String, u32> = HashMap::new();
        for r in &rels {
            if let Some(other) = names.insert(r.table.to_ascii_lowercase(), r.oid) {
                return Err(ApiError::bad(format!("two Postgres tables become ChloeDB table {} (OIDs {other} and {})", r.table, r.oid)));
            }
        }
        if slot_exists {
            tracing::info!(slot = %self.slot, "dropping the slot, to copy afresh");
            self.sql_pg(&format!("SELECT pg_drop_replication_slot({})", literal(&self.slot))).await?;
        }
        // The copy's mark first, so that one cut short is made again.
        self.write(vec![mark(self.names.copy(), self.copy as i64)]).await?;
        let mut repl = Conn::connect(&self.cfg, true).await.map_err(pg)?;
        let made = repl.query(&format!("CREATE_REPLICATION_SLOT {} LOGICAL pgoutput EXPORT_SNAPSHOT", wire::ident(&self.slot))).await.map_err(pg)?;
        let at = made.get(0, "consistent_point").and_then(parse_lsn).ok_or_else(|| ApiError::internal("CREATE_REPLICATION_SLOT answered no consistent point"))?;
        let snapshot = made.get(0, "snapshot_name").ok_or_else(|| ApiError::internal("CREATE_REPLICATION_SLOT answered no snapshot"))?.to_string();
        tracing::info!(slot = %self.slot, at = %lsn_text(at), "made the replication slot: copying the tables as of it");
        if rels.is_empty() {
            tracing::warn!(publication = %self.o.publication, "the publication has no tables yet: following it, for those it gains");
        }
        self.followed.clear();
        for r in &rels {
            if self.ensure_table(r).await? {
                self.sql(&format!("DELETE FROM {}", ident(&r.table))).await?;
            }
            self.followed.insert(r.oid, r.table.clone());
        }
        let t0 = Instant::now();
        let queue = Arc::new(tokio::sync::Mutex::new(rels.clone()));
        let mut workers = tokio::task::JoinSet::new();
        for _ in 0..self.o.copy_workers.clamp(1, rels.len().max(1)) {
            let (cfg, dst, queue, snapshot, batch) = (self.cfg.clone(), self.dst.clone(), queue.clone(), snapshot.clone(), self.o.batch);
            let tables = self.tables.clone();
            workers.spawn(async move {
                let mut conn = Conn::connect(&cfg, false).await.map_err(pg)?;
                conn.query("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY").await.map_err(pg)?;
                conn.query(&format!("SET TRANSACTION SNAPSHOT {}", literal(&snapshot))).await.map_err(pg)?;
                let mut rows = 0;
                loop {
                    let Some(rel) = queue.lock().await.pop() else { break };
                    let schema = tables.get(&rel.table.to_ascii_lowercase()).cloned().ok_or_else(|| ApiError::internal(format!("table {} is not in ChloeDB", rel.table)))?;
                    rows += copy_table(&mut conn, &dst, &rel, &schema, batch).await?;
                }
                conn.query("COMMIT").await.map_err(pg)?;
                conn.close().await;
                Ok::<u64, ApiError>(rows)
            });
        }
        let mut rows = 0;
        while let Some(r) = workers.join_next().await {
            rows += r.map_err(|e| ApiError::internal(format!("a copy worker: {e}")))??;
        }
        self.stats.copied += rows;
        let now = crate::telemetry::now_ms() as i64;
        let mut marks: Vec<Json> = rels.iter().map(|r| mark(self.names.table(self.copy, r.oid, &r.table), now * 2 + 1)).collect();
        marks.push(mark(self.names.copied(self.copy), at as i64));
        marks.push(mark(self.names.applied(self.copy), at as i64));
        self.write(marks).await?;
        self.written = at;
        self.base = 0;
        tracing::info!(tables = rels.len(), rows, took_s = t0.elapsed().as_secs_f64(), "copied: following the stream");
        Ok((repl, at))
    }

    /// Copies one table added to the publication since: at a snapshot of
    /// now, which is past every change applied so far (the stream waits
    /// meanwhile); the changes of it from the stream then bring it on.
    async fn copy_added(&mut self, mut rel: Rel) -> Result<(), ApiError> {
        if let Some((&other, _)) = self.followed.iter().find(|(o, t)| **o != rel.oid && t.eq_ignore_ascii_case(&rel.table)) {
            return Err(ApiError::bad(format!("{} becomes ChloeDB table {}, which follows another Postgres table already (OID {other}): rename one", rel.shown, rel.table)));
        }
        // With its row filter, from the catalog.
        if rel.filter.is_none() {
            if let Some(p) = self.publication().await?.into_iter().find(|p| p.oid == rel.oid) {
                rel.filter = p.filter;
            }
        }
        tracing::info!(table = %rel.shown, into = %rel.table, "a table joined the publication: copying it");
        if self.ensure_table(&rel).await? {
            self.sql(&format!("DELETE FROM {}", ident(&rel.table))).await?;
        }
        let schema = self.schema(&rel.table)?;
        let mut conn = Conn::connect(&self.cfg, false).await.map_err(pg)?;
        conn.query("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY").await.map_err(pg)?;
        let rows = copy_table(&mut conn, &self.dst, &rel, &schema, self.o.batch).await?;
        conn.query("COMMIT").await.map_err(pg)?;
        conn.close().await;
        self.stats.copied += rows;
        self.write(vec![mark(self.names.table(self.copy, rel.oid, &rel.table), crate::telemetry::now_ms() as i64 * 2 + 1)]).await?;
        self.followed.insert(rel.oid, rel.table.clone());
        tracing::info!(table = %rel.table, rows, "copied the table");
        Ok(())
    }

    /// Looks for tables that joined the publication, or left it.
    async fn poll(&mut self) -> Result<(), ApiError> {
        self.polled = Instant::now();
        let now: Vec<Rel> = self.publication().await?;
        let ids: HashSet<u32> = now.iter().map(|r| r.oid).collect();
        let left: Vec<(u32, String)> = self.followed.iter().filter(|(o, _)| !ids.contains(o)).map(|(o, t)| (*o, t.clone())).collect();
        for (oid, table) in left {
            tracing::info!(table = %table, "a table left the publication: no longer following it (its rows stay)");
            self.write(vec![mark(self.names.table(self.copy, oid, &table), crate::telemetry::now_ms() as i64 * 2)]).await?;
            self.followed.remove(&oid);
            self.rels.remove(&oid);
        }
        for rel in now {
            if !self.followed.contains_key(&rel.oid) {
                self.copy_added(rel).await?;
            }
        }
        Ok(())
    }

    /// Follows the slot's stream, from `start` (a copy's connection and
    /// point) or from what Strata applied, connecting again as it fails.
    async fn follow(&mut self, mut start: Option<(Conn, Lsn)>) -> Result<(), ApiError> {
        let mut wait = Duration::from_secs(1);
        loop {
            let at = start.as_ref().map_or(self.written, |(_, at)| *at);
            let conn = match start.take() {
                Some((c, _)) => Ok(c),
                None => Conn::connect(&self.cfg, true).await,
            };
            let r = match conn {
                Ok(c) => self.session(c, at).await,
                Err(e) => Err(pg(e)),
            };
            match r {
                Ok(()) => wait = Duration::from_secs(1),
                Err(e) if e.status == 503 => {
                    tracing::warn!("following {}: {}; connecting again in {} s", self.cfg.shown(), e.message, wait.as_secs());
                    tokio::time::sleep(wait).await;
                    wait = (wait * 2).min(Duration::from_secs(30));
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// One replication session, until its connection fails.
    async fn session(&mut self, conn: Conn, at: Lsn) -> Result<(), ApiError> {
        self.rels.clear();
        self.flushed.store(self.written.max(at), Ordering::Relaxed);
        let cmd = format!(
            "START_REPLICATION SLOT {} LOGICAL {} (proto_version '1', publication_names {})",
            wire::ident(&self.slot),
            lsn_text(at),
            literal(&wire::ident(&self.o.publication))
        );
        let (mut reader, mut writer) = conn.start_replication(&cmd).await.map_err(pg)?;
        tracing::info!(slot = %self.slot, from = %lsn_text(at), "following the stream");
        // The reader hands frames over; the writer tells Postgres how far
        // the follower is every STATUS_EVERY, or when asked, also while
        // the follower copies a table or waits on Strata.
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<Vec<u8>, wire::Error>>(4096);
        let asked = Arc::new(tokio::sync::Notify::new());
        let ask = asked.clone();
        let _read = Aborted(tokio::spawn(async move {
            loop {
                let r = reader.next().await;
                let end = !matches!(r, Ok(Some(_)));
                if let Ok(Some(b)) = &r {
                    if let Ok(Frame::Keepalive { reply: true, .. }) = pgoutput::frame(b) {
                        ask.notify_one();
                    }
                }
                let r = r.and_then(|b| b.ok_or_else(|| wire::Error { message: "the server ended the stream".into(), code: None, io: true }));
                if tx.send(r).await.is_err() || end {
                    return;
                }
            }
        }));
        let flushed = self.flushed.clone();
        let _status = Aborted(tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(STATUS_EVERY) => {}
                    _ = asked.notified() => {}
                }
                let f = flushed.load(Ordering::Relaxed);
                if writer.send(&pgoutput::status(f, f, f, false)).await.is_err() {
                    return;
                }
            }
        }));
        self.stream(&mut rx).await
    }

    /// Applies the stream's frames until it fails.
    async fn stream(&mut self, rx: &mut tokio::sync::mpsc::Receiver<Result<Vec<u8>, wire::Error>>) -> Result<(), ApiError> {
        let mut buf = Buffer::default();
        let mut open: Option<Open> = None;
        loop {
            let b = match rx.try_recv() {
                Ok(b) => b,
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                    // Nothing more to read now: what is whole is written.
                    self.flush(&mut buf).await?;
                    if open.is_none() && buf.ops.is_empty() {
                        if self.polled.elapsed() >= POLL_EVERY {
                            self.poll().await?;
                        }
                        self.log();
                    }
                    match tokio::time::timeout(POLL_EVERY, rx.recv()).await {
                        Ok(Some(b)) => b,
                        Ok(None) => return Err(ApiError { code: None, status: 503, message: "the stream ended".into() }),
                        Err(_) => continue,
                    }
                }
                Err(_) => return Err(ApiError { code: None, status: 503, message: "the stream ended".into() }),
            };
            let b = b.map_err(pg)?;
            match pgoutput::frame(&b).map_err(pg)? {
                Frame::Keepalive { end, .. } => {
                    self.server_end = self.server_end.max(end);
                    // Nothing of the publication's left unwritten: Postgres
                    // may let go of the WAL up to here.
                    if open.is_none() && buf.ops.is_empty() && buf.end.is_none() {
                        self.flushed.fetch_max(end, Ordering::Relaxed);
                    }
                }
                Frame::XLogData { end, data, .. } => {
                    self.server_end = self.server_end.max(end);
                    let m = pgoutput::message(data).map_err(pg)?;
                    self.apply(m, &mut buf, &mut open, rx.is_empty()).await?;
                }
            }
        }
    }

    /// Applies one message of the stream.
    async fn apply(&mut self, m: Message, buf: &mut Buffer, open: &mut Option<Open>, idle: bool) -> Result<(), ApiError> {
        match m {
            Message::Begin { .. } => *open = Some(Open::default()),
            Message::Commit { end_lsn, .. } => {
                let o = open.take().unwrap_or_default();
                self.stats.txns += 1;
                let source = json!({"name": self.names.applied(self.copy), "offset": end_lsn as i64});
                for op in &mut buf.ops[buf.from..] {
                    op["source"] = source.clone();
                }
                if o.steps > 0 {
                    let base = buf.base.unwrap_or(self.base) + o.steps;
                    buf.ops.push(mark(self.names.base(self.copy), base));
                    buf.ops.push(mark(self.names.applied(self.copy), end_lsn as i64));
                    buf.base = Some(base);
                }
                if buf.ops.len() > buf.from || o.steps > 0 || buf.end.is_some() {
                    buf.end = Some(end_lsn);
                } else {
                    // A transaction with nothing of the publication's.
                    self.flushed.fetch_max(end_lsn, Ordering::Relaxed);
                }
                buf.from = buf.ops.len();
                if !idle && buf.ops.len() < self.o.batch {
                    return Ok(());
                }
                self.flush(buf).await?;
            }
            Message::Relation(r) => self.relation(r, buf).await?,
            Message::Type { .. } | Message::Other(_) => {}
            Message::Insert { rel, new } => {
                let r = self.rel(rel)?;
                let schema = self.schema(&r.table)?;
                let doc = document(&r, &schema, &new, |_| None).map_err(ApiError::bad)?;
                self.push(buf, open, json!({"op": "upsert", "table": r.table, "doc": doc}), &schema, size(&new)).await?;
            }
            Message::Update { rel, old, new } => {
                let r = self.rel(rel)?;
                let schema = self.schema(&r.table)?;
                let bytes = size(&new) + old.as_ref().map_or(0, |o| size(o.tuple()));
                let key = key_of(&r, &schema, &new).map_err(ApiError::bad)?;
                // The row's key before: changed, the old row is deleted.
                let was = match &old {
                    Some(o) => {
                        let k = key_of(&r, &schema, o.tuple()).map_err(ApiError::bad)?;
                        (k != key).then_some(k)
                    }
                    None => None,
                };
                let from_key = was.clone().unwrap_or_else(|| key.clone());
                let filled = self.unchanged(&r, &schema, &old, &new, &from_key, buf).await?;
                let doc = document(&r, &schema, &new, |c| filled.get(c).cloned()).map_err(ApiError::bad)?;
                if let Some(k) = was {
                    self.push(buf, open, json!({"op": "delete", "table": r.table, "key": k}), &schema, 0).await?;
                }
                self.push(buf, open, json!({"op": "upsert", "table": r.table, "doc": doc}), &schema, bytes).await?;
            }
            Message::Delete { rel, old } => {
                let r = self.rel(rel)?;
                let schema = self.schema(&r.table)?;
                let key = key_of(&r, &schema, old.tuple()).map_err(ApiError::bad)?;
                self.push(buf, open, json!({"op": "delete", "table": r.table, "key": key}), &schema, size(old.tuple())).await?;
            }
            Message::Truncate { rels } => {
                let tables: Vec<String> = rels.iter().filter_map(|o| self.rels.get(o).map(|r| r.table.clone())).collect();
                if tables.is_empty() {
                    return Ok(());
                }
                let o = open.get_or_insert_with(Open::default);
                // What came before is written first, as a step of its own.
                if o.ops > 0 {
                    self.step(buf, open).await?;
                } else {
                    self.flush(buf).await?;
                }
                let o = open.get_or_insert_with(Open::default);
                o.steps += 1;
                // (The buffer is written: the base is the follower's.)
                let n = self.base + o.steps;
                // Done already, by the process before this one?
                let done = self.kept().await?.step >= n;
                for t in &tables {
                    if done {
                        tracing::info!(table = %t, "a TRUNCATE applied already: skipping it");
                    } else {
                        tracing::info!(table = %t, "TRUNCATE: deleting every row");
                        self.sql(&format!("DELETE FROM {}", ident(t))).await?;
                    }
                    self.stats.changes += 1;
                }
                self.write(vec![mark(self.names.step(self.copy), n)]).await?;
            }
        }
        Ok(())
    }

    /// The relation a change names, as this session's stream described it.
    fn rel(&self, oid: u32) -> Result<Arc<Rel>, ApiError> {
        self.rels.get(&oid).cloned().ok_or_else(|| ApiError { code: None, status: 503, message: format!("a change of relation {oid}, which the stream did not describe first") })
    }

    /// Adds an operation of the open transaction, writing a step of it
    /// once it is [`STEP`] long.
    async fn push(&mut self, buf: &mut Buffer, open: &mut Option<Open>, op: Json, schema: &Schema, bytes: usize) -> Result<(), ApiError> {
        let o = open.get_or_insert_with(Open::default);
        let table = op["table"].as_str().unwrap_or_default().to_ascii_lowercase();
        match op["op"].as_str() {
            Some("upsert") => {
                let pk = schema.primary_key.map(|p| schema.fields[p].name.clone()).unwrap_or_default();
                buf.rows.insert((table, op["doc"][pk.as_str()].to_string()), buf.ops.len());
            }
            _ => {
                buf.rows.remove(&(table, op["key"].to_string()));
            }
        }
        buf.ops.push(op);
        self.stats.changes += 1;
        o.ops += 1;
        o.bytes += bytes;
        if o.ops >= STEP || o.bytes >= STEP_BYTES {
            self.step(buf, open).await?;
        }
        Ok(())
    }

    /// Writes the open transaction's operations so far as a step, with
    /// what is whole before them.
    async fn step(&mut self, buf: &mut Buffer, open: &mut Option<Open>) -> Result<(), ApiError> {
        let o = open.get_or_insert_with(Open::default);
        o.steps += 1;
        o.ops = 0;
        o.bytes = 0;
        // Numbered after the steps of the whole transactions before it.
        let source = json!({"name": self.names.step(self.copy), "offset": buf.base.unwrap_or(self.base) + o.steps});
        for op in &mut buf.ops[buf.from..] {
            op["source"] = source.clone();
        }
        buf.from = buf.ops.len();
        self.flush(buf).await
    }

    /// Writes what the buffer holds of whole transactions (and of steps).
    async fn flush(&mut self, buf: &mut Buffer) -> Result<(), ApiError> {
        if buf.from == 0 {
            return Ok(());
        }
        let rest = buf.ops.split_off(buf.from);
        let ops = std::mem::replace(&mut buf.ops, rest);
        buf.from = 0;
        // What is left (the open transaction's) is indexed again.
        let from = ops.len();
        buf.rows.retain(|_, i| {
            if *i < from {
                return false;
            }
            *i -= from;
            true
        });
        self.write(ops).await?;
        if let Some(b) = buf.base.take() {
            self.base = b;
        }
        if let Some(e) = buf.end.take() {
            self.written = self.written.max(e);
            self.flushed.fetch_max(e, Ordering::Relaxed);
        }
        Ok(())
    }

    /// The values an update leaves out (unchanged TOAST): from the old
    /// row (`REPLICA IDENTITY FULL`), the buffer, or Strata's row.
    async fn unchanged(&mut self, rel: &Rel, schema: &Schema, old: &Option<Old>, new: &[Datum], key: &Json, buf: &Buffer) -> Result<HashMap<String, Json>, ApiError> {
        let mut out = HashMap::new();
        let mut want = Vec::new();
        for (i, d) in new.iter().enumerate() {
            if *d != Datum::Unchanged {
                continue;
            }
            let Some(fi) = schema.field_index(&rel.columns[i].name) else { continue };
            let f = &schema.fields[fi];
            if let Some(Old::Full(t)) = old {
                match t.get(i) {
                    Some(Datum::Text(v)) => {
                        out.insert(f.name.clone(), json_of(f, &rel.types[i], v).map_err(ApiError::bad)?);
                        continue;
                    }
                    Some(Datum::Null) => {
                        out.insert(f.name.clone(), Json::Null);
                        continue;
                    }
                    _ => {}
                }
            }
            if let Some(&j) = buf.rows.get(&(rel.table.to_ascii_lowercase(), key.to_string())) {
                if let Some(v) = buf.ops[j]["doc"].get(&f.name) {
                    out.insert(f.name.clone(), v.clone());
                    continue;
                }
            }
            want.push(f.name.clone());
        }
        if want.is_empty() {
            return Ok(out);
        }
        let mut body = json!({"table": rel.table, "keys": [key], "columns": want});
        if !self.seen.is_empty() {
            body["min_seq_nos"] = json!(self.seen);
        }
        let j = retry("a row's values from ChloeDB", || self.dst.post("/v1/kv/get", &body)).await?;
        match j["rows"].get(0) {
            Some(Json::Object(row)) => {
                for c in want {
                    if let Some(v) = row.get(&c).or_else(|| row.iter().find(|(k, _)| k.eq_ignore_ascii_case(&c)).map(|(_, v)| v)) {
                        out.insert(c, v.clone());
                    }
                }
            }
            _ => tracing::warn!(table = %rel.table, key = %key, "an update left values unchanged of a row ChloeDB does not hold: they are left NULL"),
        }
        Ok(out)
    }

    /// A relation the stream describes: a table new to the follower is
    /// copied; one whose columns changed has its Strata table changed.
    async fn relation(&mut self, r: pgoutput::Relation, buf: &mut Buffer) -> Result<(), ApiError> {
        let was = self.rels.get(&r.oid).cloned();
        let rel = self.rel_of(r.oid, &r.namespace, &r.name, r.columns, None).await?;
        if !self.followed.contains_key(&r.oid) || !self.tables.contains_key(&rel.table.to_ascii_lowercase()) {
            self.copy_added(rel.clone()).await?;
            self.rels.insert(r.oid, Arc::new(rel));
            return Ok(());
        }
        // A column renamed: one of the same number under another name.
        if let Some(w) = &was {
            let renamed: Vec<(&pgoutput::Column, &pgoutput::Column)> = rel
                .columns
                .iter()
                .zip(&rel.attnums)
                .filter_map(|(now, n)| {
                    let i = w.attnums.iter().position(|m| m.is_some() && m == n)?;
                    (w.columns[i].name != now.name).then_some((&w.columns[i], now))
                })
                .collect();
            for (c, now) in renamed {
                {
                    let schema = self.schema(&rel.table)?;
                    if schema.field_index(&c.name).is_some() && schema.field_index(&now.name).is_none() {
                        let sql = format!("ALTER TABLE {} RENAME COLUMN {} TO {}", ident(&rel.table), ident(&c.name), ident(&now.name));
                        tracing::info!(table = %rel.table, "Postgres renamed a column: {sql}");
                        if let Err(e) = self.sql(&sql).await {
                            // An earlier try, its answer lost, may have renamed it.
                            self.load_tables().await?;
                            let s = self.schema(&rel.table)?;
                            if s.field_index(&now.name).is_none() || s.field_index(&c.name).is_some() {
                                return Err(e);
                            }
                        }
                        self.load_tables().await?;
                        // Rows read before the rename name it as it was.
                        for op in &mut buf.ops {
                            if op["table"].as_str().is_some_and(|t| t.eq_ignore_ascii_case(&rel.table)) {
                                if let Some(Json::Object(d)) = op.get_mut("doc") {
                                    if let Some(v) = d.remove(&c.name) {
                                        d.insert(now.name.clone(), v);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            for (i, c) in rel.columns.iter().enumerate() {
                if w.columns.iter().any(|o| o.name == c.name && o.type_oid != c.type_oid) {
                    tracing::warn!(table = %rel.table, column = %c.name, to = %rel.types[i].name, "Postgres changed a column's type: its values are written as the ChloeDB column's type reads them");
                }
            }
        }
        self.ensure_table(&rel).await?;
        self.rels.insert(r.oid, Arc::new(rel));
        Ok(())
    }

    fn log(&mut self) {
        if self.logged.elapsed() < LOG_EVERY {
            return;
        }
        self.logged = Instant::now();
        let s = &self.stats;
        tracing::info!(
            applied = %lsn_text(self.written),
            server = %lsn_text(self.server_end),
            behind_bytes = self.server_end.saturating_sub(self.flushed.load(Ordering::Relaxed)),
            transactions = s.txns,
            changes = s.changes,
            writes = s.writes,
            copied = s.copied,
            skipped = s.skipped,
            "following {}", self.cfg.shown()
        );
    }
}

/// A task stopped when this is dropped: a session's reader and writer of
/// the connection end with it, also when the follower's future is dropped
/// (an embedder stopping it), so that the connection closes and the slot
/// is free for the next.
struct Aborted(tokio::task::JoinHandle<()>);

impl Drop for Aborted {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// A tuple's values' bytes.
fn size(t: &[Datum]) -> usize {
    t.iter().map(|d| if let Datum::Text(s) = d { s.len() } else { 0 }).sum()
}

/// Copies `rel`'s rows over `conn` (in the transaction it is in) into
/// Strata, answering how many.
async fn copy_table(conn: &mut Conn, dst: &Http, rel: &Rel, schema: &Schema, batch: usize) -> Result<u64, ApiError> {
    let cols: Vec<String> = rel.columns.iter().map(|c| wire::ident(&c.name)).collect();
    let from = format!("{}.{}", wire::ident(&rel.schema_name), wire::ident(&rel.rel_name));
    let sql = match &rel.filter {
        Some(f) => format!("COPY (SELECT {} FROM {from} WHERE {f}) TO STDOUT", cols.join(", ")),
        None => format!("COPY (SELECT {} FROM {from}) TO STDOUT", cols.join(", ")),
    };
    conn.copy_out(&sql).await.map_err(pg)?;
    let mut ops = Vec::new();
    let mut bytes = 0;
    let mut rows = 0u64;
    let mut pending: Vec<u8> = Vec::new();
    let t0 = Instant::now();
    let send = |ops: Vec<Json>| async move {
        let body = json!({"ops": ops});
        retry("writing a copy to ChloeDB", || dst.post("/v1/write", &body)).await.map(|_| ())
    };
    loop {
        let chunk = conn.copy_data().await.map_err(pg)?;
        let done = chunk.is_none();
        if let Some(c) = chunk {
            pending.extend_from_slice(&c);
        }
        while let Some(n) = pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = pending.drain(..=n).collect();
            let fields = copy_fields(&line[..line.len() - 1]).map_err(|e| ApiError::bad(format!("a row of {}: {e}", rel.shown)))?;
            let tuple: Vec<Datum> = fields.into_iter().map(|f| f.map_or(Datum::Null, Datum::Text)).collect();
            bytes += size(&tuple);
            let doc = document(rel, schema, &tuple, |_| None).map_err(ApiError::bad)?;
            ops.push(json!({"op": "upsert", "table": rel.table, "doc": doc}));
            rows += 1;
            if ops.len() >= batch || bytes >= COPY_BYTES {
                send(std::mem::take(&mut ops)).await?;
                bytes = 0;
            }
        }
        if done {
            break;
        }
    }
    send(ops).await?;
    tracing::info!(table = %rel.table, rows, took_s = t0.elapsed().as_secs_f64(), "copied {}", rel.shown);
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_and_keys_are_named() {
        assert_eq!(default_slot("App-1"), "strata_app_1");
        assert_eq!(join_key(&["a:b", "c\\"]), "a\\:b:c\\\\");
        assert_eq!(table_name("public", "items"), "items");
        assert_eq!(table_name("sales", "orders"), "sales_orders");
    }
}
