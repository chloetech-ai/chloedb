//! Receipts: what each recall showed the agent, and why, replayable.
//!
//! Every recall (the context a prompt gets, the records a compaction gets
//! back, a `memory_search` answer) writes a receipt: the commit of the
//! store it read (`txn`, and `seq_no` where the store numbers its writes),
//! every input that decides the result (the query, the session's window,
//! the records already shown, the ranking, gate and packing parameters),
//! and for each candidate its score and score parts, its tokens and why it
//! was taken or left out. The text the agent was shown is kept with it.
//!
//! [`retrieve`] is the one function recall runs, so replaying a receipt is
//! running it again with the receipt's inputs: on the snapshot kept at the
//! receipt's commit ([`crate::snapshots`]) it must give the same rows, the
//! same scores and the same decisions, and on today's store it shows what
//! has changed since (a record superseded, deleted or outranked).
//!
//! The decisions use the words of ChloeDB's `PACK` receipts: `take`,
//! `excerpt`, `skip: <reason>`, with ChloeCtx's own `listed` (named by id
//! under "Further candidates") and `knowledge` (a rule or decision shown on
//! its own text, ahead of the gate).
//!
//! Receipts are kept in their own ChloeDB file beside the store,
//! `receipts.strata` (table `receipts`), so the store's file and its
//! format are unchanged, and kept snapshots do not hold receipts.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use chloedb::db::Db;
use chloedb::query::Params;
use chloedb::schema::{Field, FieldType, Schema};
use chloedb::storage::DbOptions;
use chloedb::Value;

use crate::recall::{self, Decision, Evidence, Gate, KnowledgeParams, PackParams, RankParams};
use crate::record::{Kind, Record};
use crate::snapshots::{Kept, Snapshots};
use crate::store::{Filter, Hit, Store};
use crate::{err, Result};

/// The receipt format's version.
pub const VERSION: u32 = 1;

/// Candidates a receipt lists at most (the rest only count).
pub const MAX_ROWS: usize = 40;

/// The records [`recall::fmt_more`] names after a full recall, and a brief one.
pub const MORE_FULL: usize = 3;
pub const MORE_BRIEF: usize = 5;

/// A token estimate, counted as ChloeDB's `PACK` counts, so a receipt and
/// a `PACK ... WITH RECEIPT` answer agree on what a text costs.
pub fn tokens(s: &str) -> usize {
    chloedb::query::pack::count_tokens(s)
}

pub fn sha(s: &str) -> String {
    let d = Sha256::digest(s.as_bytes());
    d.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// [`Filter`], as a receipt keeps it.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct FilterSpec {
    pub exclude_window: Option<(String, i64)>,
    pub kinds: Vec<String>,
    pub since_ts: Option<i64>,
    pub until_ts: Option<i64>,
    pub session: Option<String>,
}

impl FilterSpec {
    pub fn of(f: &Filter) -> FilterSpec {
        FilterSpec {
            exclude_window: f.exclude_window.clone(),
            kinds: f.kinds.iter().map(|k| k.as_str().to_string()).collect(),
            since_ts: f.since_ts,
            until_ts: f.until_ts,
            session: f.session.clone(),
        }
    }

    pub fn filter(&self) -> Filter {
        Filter {
            exclude_window: self.exclude_window.clone(),
            kinds: self.kinds.iter().filter_map(|k| Kind::parse(k)).collect(),
            since_ts: self.since_ts,
            until_ts: self.until_ts,
            session: self.session.clone(),
        }
    }
}

/// What a recall was asked: everything [`retrieve`] reads besides the store.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Inputs {
    /// `recall` (gated and packed, as a prompt gets it) or `search` (the
    /// best `limit` by score, as `memory_search` answers).
    pub mode: String,
    pub query: String,
    /// The session whose newest records are the tail (recall).
    pub session: String,
    pub filter: FilterSpec,
    /// Records shown to the session already (left out of packing).
    pub skip: Vec<String>,
    /// Recall past the gate (after a compaction).
    pub force: bool,
    /// Search: how many hits are shown.
    pub limit: usize,
    pub rank: RankParams,
    pub pack: PackParams,
    pub gate: Gate,
    pub knowledge: KnowledgeParams,
    /// Briefing: its size.
    #[serde(default)]
    pub briefing_chars: usize,
}

impl Inputs {
    /// The briefing a session starts with.
    pub fn briefing(session: &str, chars: usize) -> Inputs {
        let mut i = Inputs::search("", &Filter::default(), 0, &RankParams::default());
        i.mode = "briefing".into();
        i.session = session.to_string();
        i.briefing_chars = chars;
        i
    }

    pub fn recall(query: &str, session: &str, filter: &Filter, skip: &HashSet<String>, force: bool, rank: &RankParams, pack: &PackParams, gate: &Gate, knowledge: &KnowledgeParams) -> Inputs {
        let mut skip: Vec<String> = skip.iter().cloned().collect();
        skip.sort();
        Inputs {
            mode: "recall".into(),
            query: query.to_string(),
            session: session.to_string(),
            filter: FilterSpec::of(filter),
            skip,
            force,
            limit: 0,
            rank: rank.clone(),
            pack: pack.clone(),
            gate: gate.clone(),
            knowledge: knowledge.clone(),
            briefing_chars: 0,
        }
    }

    pub fn search(query: &str, filter: &Filter, limit: usize, rank: &RankParams) -> Inputs {
        Inputs {
            mode: "search".into(),
            query: query.to_string(),
            session: String::new(),
            filter: FilterSpec::of(filter),
            skip: Vec::new(),
            force: false,
            limit,
            rank: rank.clone(),
            pack: PackParams::default(),
            gate: Gate::default(),
            knowledge: KnowledgeParams::default(),
            briefing_chars: 0,
        }
    }
}

/// One candidate of a receipt.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Row {
    pub id: String,
    pub kind: String,
    pub tag: String,
    /// Its place in the ranked candidates, from 1 (0: knowledge, shown on
    /// its own text ahead of the gate).
    pub rank: usize,
    /// The final score (knowledge: the cosine to the query).
    pub score: f64,
    /// Raw BM25 and cosine scores, when the record was in that list.
    pub bm25: Option<f64>,
    pub cos: Option<f64>,
    /// Knowledge: the share of the query's weighted terms it holds.
    pub cov: Option<f64>,
    pub importance: f64,
    pub age_seq: i64,
    /// Tokens it took (its snippet) or would have taken (its text).
    pub tokens: usize,
    pub decision: String,
    /// The record's text as it was ranked, hashed: a replay tells a record
    /// changed in place from one that did not.
    pub sha: String,
}

impl Row {
    pub fn shown(&self) -> bool {
        matches!(self.decision.as_str(), "take" | "excerpt" | "knowledge" | "listed" | "briefing")
    }
}

/// What [`retrieve`] found and decided.
pub struct Outcome {
    pub known: Vec<(Record, f64, f64)>,
    pub hits: Vec<Hit>,
    pub ev: Evidence,
    pub gate: f64,
    pub decision: Option<Decision>,
    /// Packed records with their snippets, oldest first.
    pub items: Vec<(Record, String)>,
    pub weak: bool,
    /// Records already shown or packed ([`recall::fmt_more`] names others).
    pub shown: HashSet<String>,
    /// The ids the context shows in full: knowledge, then packed records.
    pub ids: Vec<String>,
    pub rows: Vec<Row>,
}

/// Retrieval as recall runs it, on `store` as it is now (pin it first for
/// one version). The daemon formats the outcome; a replay compares it.
pub fn retrieve(store: &Store, inp: &Inputs) -> Result<Outcome> {
    let store = store.pinned();
    let filter = inp.filter.filter();
    let last_seq = store.last_seq();
    if inp.mode == "search" {
        let (hits, ev) = recall::search(&store, &inp.query, &filter, &inp.rank)?;
        let rows = hits.iter().enumerate().take(MAX_ROWS).map(|(i, h)| hit_row(h, i + 1, last_seq, if i < inp.limit { "listed" } else { "skip: below the limit" }, None)).collect();
        let ids = hits.iter().take(inp.limit).map(|h| h.record.id.clone()).collect();
        return Ok(Outcome { known: Vec::new(), hits, ev, gate: 0.0, decision: None, items: Vec::new(), weak: false, shown: HashSet::new(), ids, rows });
    }
    if inp.mode == "briefing" {
        // The notes a session starts with: shown whole, chosen by kind and
        // age rather than ranked (the checks of their code are left out).
        let (_, shown) = recall::briefing_checked(&store, &inp.session, inp.briefing_chars, 0, None)?;
        let recs = store.get(&shown)?;
        let rows = shown
            .iter()
            .enumerate()
            .filter_map(|(i, id)| recs.iter().find(|r| &r.id == id).map(|r| (i, r)))
            .map(|(i, r)| Row {
                id: r.id.clone(),
                kind: r.kind.as_str().into(),
                tag: r.tag.clone(),
                rank: i + 1,
                score: 0.0,
                bm25: None,
                cos: None,
                cov: None,
                importance: r.importance,
                age_seq: (last_seq - r.seq).max(0),
                tokens: tokens(&r.text),
                decision: "briefing".into(),
                sha: sha(&r.text),
            })
            .collect();
        return Ok(Outcome { known: Vec::new(), hits: Vec::new(), ev: Evidence::default(), gate: 0.0, decision: None, items: Vec::new(), weak: false, shown: shown.iter().cloned().collect(), ids: shown, rows });
    }
    let mut skip: HashSet<String> = inp.skip.iter().cloned().collect();
    let tail = match &inp.rank.tail {
        t if t.enabled => Some(recall::Tail::of(&store.tail(&inp.session, t.records)?, t)),
        _ => None,
    };
    let (known, searched) = recall::both(
        || if inp.knowledge.enabled { recall::knowledge(&store, &inp.query, &filter, &skip, &inp.knowledge) } else { Ok(Vec::new()) },
        || recall::search_with(&store, &inp.query, tail.as_ref(), &filter, &inp.rank),
    );
    let known = known?;
    let (hits, ev) = searched?;
    let mut ids: Vec<String> = known.iter().map(|(r, _, _)| r.id.clone()).collect();
    skip.extend(ids.iter().cloned());
    let g = inp.gate.score(&ev);
    let decision = if inp.force && !hits.is_empty() { Decision::Inject } else { inp.gate.decide(&ev) };
    let mut items = Vec::new();
    let mut why: HashMap<String, String> = HashMap::new();
    let weak = decision == Decision::Brief;
    let mut shown = skip.clone();
    if !hits.is_empty() && matches!(decision, Decision::Inject | Decision::Brief) {
        let params = if weak { PackParams::brief() } else { inp.pack.clone() };
        let (packed, w) = recall::pack_explained(&hits, &inp.query, &skip, &params);
        why = w;
        if !packed.is_empty() {
            shown.extend(packed.iter().map(|(r, _)| r.id.clone()));
            ids.extend(packed.iter().map(|(r, _)| r.id.clone()));
            // The records fmt_more names by id after the packed ones.
            let n = if weak { MORE_BRIEF } else { MORE_FULL };
            for h in hits.iter().filter(|h| !shown.contains(&h.record.id) && h.record.tag != "uncertain").take(n) {
                why.insert(h.record.id.clone(), "listed".into());
            }
        }
        items = packed;
    }
    let snips: HashMap<&str, &str> = items.iter().map(|(r, s)| (r.id.as_str(), s.as_str())).collect();
    let mut rows: Vec<Row> = known
        .iter()
        .map(|(r, c, v)| Row {
            id: r.id.clone(),
            kind: r.kind.as_str().into(),
            tag: r.tag.clone(),
            rank: 0,
            score: *c,
            bm25: None,
            cos: Some(*c),
            cov: Some(*v),
            importance: r.importance,
            age_seq: (last_seq - r.seq).max(0),
            tokens: tokens(&r.text),
            decision: "knowledge".into(),
            sha: sha(&r.text),
        })
        .collect();
    for (i, h) in hits.iter().enumerate().take(MAX_ROWS) {
        let d = match decision {
            Decision::Skip => "skip: the gate held back".to_string(),
            _ if known.iter().any(|(r, _, _)| r.id == h.record.id) => "skip: shown as knowledge".to_string(),
            _ => why.get(&h.record.id).cloned().unwrap_or_else(|| "skip: not reached".into()),
        };
        rows.push(hit_row(h, i + 1, last_seq, &d, snips.get(h.record.id.as_str()).copied()));
    }
    Ok(Outcome { known, hits, ev, gate: g, decision: Some(decision), items, weak, shown, ids, rows })
}

fn hit_row(h: &Hit, rank: usize, last_seq: i64, decision: &str, snip: Option<&str>) -> Row {
    let r = &h.record;
    Row {
        id: r.id.clone(),
        kind: r.kind.as_str().into(),
        tag: r.tag.clone(),
        rank,
        score: h.score,
        bm25: h.bm25,
        cos: h.cos,
        cov: None,
        importance: r.importance,
        age_seq: (last_seq - r.seq).max(0),
        tokens: tokens(snip.unwrap_or(&r.text)),
        decision: decision.to_string(),
        sha: sha(&r.text),
    }
}

/// The version of the store a receipt read.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct SnapRef {
    pub txn: u64,
    pub seq_no: Option<u64>,
    /// The newest record's number, which recency is measured from.
    pub last_seq: i64,
    pub records: u64,
}

/// One recall's receipt.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub v: u32,
    pub id: String,
    pub ts: i64,
    pub session: String,
    pub turn: i64,
    /// `prompt`, `compact` or `search`.
    pub source: String,
    pub snapshot: SnapRef,
    pub embedder: String,
    pub inputs: Inputs,
    pub decision: Option<String>,
    pub gate: f64,
    pub evidence: Evidence,
    pub candidates: usize,
    pub rows: Vec<Row>,
    /// The ids shown in full.
    pub shown: Vec<String>,
    /// The text the agent was given, as given.
    pub text: String,
    pub text_sha: String,
    pub tokens: usize,
}

impl Receipt {
    /// A receipt of `out`, read from `store` (pinned to the version
    /// [`retrieve`] read).
    #[allow(clippy::too_many_arguments)]
    pub fn new(id: String, ts: i64, session: &str, turn: i64, source: &str, store: &Store, inputs: Inputs, out: &Outcome, text: &str) -> Receipt {
        let snapshot = match store.db() {
            Some(_) => {
                let s = store.snapshot();
                SnapRef { txn: s.txn(), seq_no: s.seq_no(), last_seq: store.last_seq(), records: s.n_live_docs() }
            }
            None => SnapRef { last_seq: store.last_seq(), ..Default::default() },
        };
        Receipt {
            v: VERSION,
            id,
            ts,
            session: session.to_string(),
            turn,
            source: source.to_string(),
            snapshot,
            embedder: store.embedder.name().to_string(),
            inputs,
            decision: out.decision.map(|d| format!("{d:?}")),
            gate: out.gate,
            evidence: out.ev.clone(),
            candidates: out.hits.len(),
            rows: out.rows.clone(),
            shown: out.ids.clone(),
            text: text.to_string(),
            text_sha: sha(text),
            tokens: tokens(text),
        }
    }
}

/// How a replay's rows differ from the receipt's.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Verdict {
    pub identical: bool,
    /// The same records shown, with the same decisions (scores may move).
    pub same_shown: bool,
    pub changes: Vec<String>,
}

/// Compares a replay's rows with the receipt's.
pub fn compare(was: &[Row], now: &[Row]) -> Verdict {
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0);
    let identical = was.len() == now.len()
        && was.iter().zip(now).all(|(a, b)| a.id == b.id && a.decision == b.decision && a.rank == b.rank && a.sha == b.sha && close(a.score, b.score) && a.tokens == b.tokens);
    let shown = |rows: &[Row]| rows.iter().filter(|r| r.shown()).map(|r| (r.id.clone(), r.decision.clone())).collect::<Vec<_>>();
    let same_shown = shown(was) == shown(now);
    let mut changes = Vec::new();
    let by: HashMap<&str, &Row> = now.iter().map(|r| (r.id.as_str(), r)).collect();
    let before: HashSet<&str> = was.iter().map(|r| r.id.as_str()).collect();
    for a in was {
        match by.get(a.id.as_str()) {
            None if a.shown() => changes.push(format!("{} ({}, {}) is no longer returned", a.id, a.kind, a.decision)),
            None => {}
            Some(b) => {
                if a.sha != b.sha {
                    changes.push(format!("{} changed in place since", a.id));
                }
                if a.tag != b.tag {
                    changes.push(format!("{} is now tagged {} (was {})", a.id, b.tag, a.tag));
                }
                if a.decision != b.decision && (a.shown() || b.shown()) {
                    changes.push(format!("{}: {} → {}", a.id, a.decision, b.decision));
                } else if a.shown() && !close(a.score, b.score) {
                    changes.push(format!("{}: score {:.3} → {:.3}, rank {} → {}", a.id, a.score, b.score, a.rank, b.rank));
                }
            }
        }
    }
    for b in now.iter().filter(|b| b.shown() && !before.contains(b.id.as_str())) {
        changes.push(format!("{} ({}) is new: {}", b.id, b.kind, b.decision));
    }
    if !identical && changes.is_empty() {
        changes.push("the same records and decisions; lower-ranked candidates moved".into());
    }
    changes.truncate(20);
    Verdict { identical, same_shown, changes }
}

/// A replay of a receipt on one version of the store.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Replayed {
    /// `kept snapshot`, `later snapshot` or `now`.
    pub on: String,
    pub snapshot: Option<Kept>,
    pub txn: u64,
    pub verdict: Verdict,
    pub rows: Vec<Row>,
}

/// Replays a receipt: on the snapshot kept at its commit (or else the
/// nearest later one), and on `live`, the store as it is now.
pub fn replay(r: &Receipt, snaps: &Snapshots, live: &Store) -> Result<Vec<Replayed>> {
    let mut out = Vec::new();
    let (k, on) = match snaps.exact(r.snapshot.txn) {
        Some(k) => (Some(k), "kept snapshot"),
        None => (snaps.after(r.snapshot.txn), "later snapshot"),
    };
    if let Some(k) = k {
        let s = snaps.open(&k, live.embedder.clone())?;
        s.set_last_seq(r.snapshot.last_seq);
        let o = retrieve(&s, &r.inputs)?;
        out.push(Replayed { on: on.into(), txn: k.txn, verdict: compare(&r.rows, &o.rows), rows: o.rows, snapshot: Some(k) });
    }
    let live = live.pinned();
    let txn = live.db().map_or(0, |_| live.snapshot().txn());
    let o = retrieve(&live, &r.inputs)?;
    out.push(Replayed { on: "now".into(), snapshot: None, txn, verdict: compare(&r.rows, &o.rows), rows: o.rows });
    Ok(out)
}

// ---------------------------------------------------------------- the book

/// The receipts of one store, in `receipts.strata` beside it.
pub struct Book {
    db: Db,
    writes: Mutex<()>,
    n: AtomicU64,
}

fn book_schema() -> Result<Schema> {
    Ok(Schema::new(
        "receipts",
        vec![
            Field::new("id", FieldType::Keyword),
            Field::new("session", FieldType::Keyword),
            Field::new("source", FieldType::Keyword),
            Field::new("turn", FieldType::Int),
            Field::new("ts", FieldType::Int),
            Field::new("txn", FieldType::Int),
            Field::new("query", FieldType::Text { positions: false }),
            Field::new("body", FieldType::Text { positions: false }).stored_only(),
        ],
        Some("id"),
    )?)
}

impl Book {
    pub fn open(store_dir: &Path, sync: bool) -> Result<Book> {
        let path = store_dir.join("receipts.strata");
        let mut o = DbOptions::default();
        o.sync = sync;
        o.publish_snapshots = true;
        let db = if path.exists() { Db::open(&path, o)? } else { Db::create(&path, book_schema()?, o)? };
        db.spawn_flusher();
        Ok(Book { db, writes: Mutex::new(()), n: AtomicU64::new(0) })
    }

    /// A new receipt id: time-ordered, unique in the store.
    pub fn next_id(&self, ts_ms: i64) -> String {
        let n = self.n.fetch_add(1, Ordering::Relaxed) % 4096;
        format!("rc-{ts_ms:x}{n:03x}")
    }

    pub fn put(&self, r: &Receipt) -> Result<()> {
        let body = serde_json::to_string(r).map_err(|e| err(e.to_string()))?;
        let doc = vec![
            Value::Str(r.id.clone()),
            Value::Str(r.session.clone()),
            Value::Str(r.source.clone()),
            Value::Int(r.turn),
            Value::Int(r.ts),
            Value::Int(r.snapshot.txn as i64),
            Value::Str(r.inputs.query.chars().take(2000).collect()),
            Value::Str(body),
        ];
        let _g = self.writes.lock().unwrap();
        let mut t = self.db.storage().begin()?;
        t.upsert(doc)?;
        t.commit()?;
        Ok(())
    }

    fn read(&self, sql: &str, p: &Params) -> Result<Vec<Receipt>> {
        let rs = self.db.query(sql, p)?;
        Ok(rs.rows.iter().filter_map(|r| r.first().and_then(|v| v.as_str()).and_then(|b| serde_json::from_str(b).ok())).collect())
    }

    pub fn get(&self, id: &str) -> Result<Option<Receipt>> {
        Ok(self.read("SELECT body FROM receipts WHERE id = :id", &Params::new().set("id", Value::Str(id.into())))?.pop())
    }

    /// The receipts of a session's turns `lo..=hi`, oldest first.
    pub fn of_turns(&self, session: &str, lo: i64, hi: i64) -> Result<Vec<Receipt>> {
        self.read(
            &format!("SELECT body FROM receipts WHERE session = :s AND turn >= {lo} AND turn <= {hi} ORDER BY ts ASC LIMIT 500"),
            &Params::new().set("s", Value::Str(session.into())),
        )
    }

    /// The newest receipts, newest first.
    pub fn latest(&self, session: Option<&str>, limit: usize) -> Result<Vec<Receipt>> {
        let limit = limit.clamp(1, 500);
        match session {
            Some(s) => self.read(&format!("SELECT body FROM receipts WHERE session = :s ORDER BY ts DESC LIMIT {limit}"), &Params::new().set("s", Value::Str(s.into()))),
            None => self.read(&format!("SELECT body FROM receipts ORDER BY ts DESC LIMIT {limit}"), &Params::new()),
        }
    }

    pub fn len(&self) -> u64 {
        self.db.snapshot().n_live_docs()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// One line per receipt row, for a person.
pub fn fmt_rows(rows: &[Row], only_shown: bool) -> String {
    let mut s = String::new();
    for r in rows.iter().filter(|r| !only_shown || r.shown()) {
        let f = |x: Option<f64>| x.map_or("-".to_string(), |v| format!("{v:.3}"));
        s += &format!(
            "  {:>3} {:<11} {:<26} score {:.3}  bm25 {:>6}  cos {:>5}  {:>4} tok  {}{}\n",
            if r.rank == 0 { "k".to_string() } else { r.rank.to_string() },
            r.kind,
            r.id,
            r.score,
            f(r.bm25),
            f(r.cos),
            r.tokens,
            r.decision,
            if r.tag != "-" && !r.tag.is_empty() { format!("  [{}]", r.tag) } else { String::new() }
        );
    }
    s
}

/// A receipt for a person.
pub fn fmt_receipt(r: &Receipt) -> String {
    let mut s = format!(
        "receipt {} · {} · session {} turn {} · {}\nstore commit {}{} · {} records · embedder {}\nquery: {}\n",
        r.id,
        recall::fmt_time(r.ts),
        r.session.chars().take(8).collect::<String>(),
        r.turn,
        r.source,
        r.snapshot.txn,
        r.snapshot.seq_no.map_or(String::new(), |n| format!(" (seq_no {n})")),
        r.snapshot.records,
        r.embedder,
        r.inputs.query.chars().take(300).collect::<String>().replace('\n', " "),
    );
    if let Some(d) = &r.decision {
        s += &format!("gate {:.3} → {d} · {} candidates · {} tokens shown\n", r.gate, r.candidates, r.tokens);
    }
    s += &fmt_rows(&r.rows, false);
    s
}

impl Book {
    /// Scrubs the receipts: with `forgotten`, those that showed that record
    /// lose the text they were shown (their rows keep ids, scores and
    /// hashes); without, every receipt is redacted as records are
    /// ([`crate::redact::redact_json`]). Then the file is compacted, so the
    /// old versions leave it. The number of receipts changed.
    pub fn scrub(&self, forgotten: Option<&str>) -> Result<usize> {
        let rs = self.db.query(&format!("SELECT body FROM receipts LIMIT {}", self.len() + 1000), &Params::new())?;
        let mut n = 0;
        for row in &rs.rows {
            let Some(body) = row.first().and_then(|v| v.as_str()) else { continue };
            let Ok(mut r) = serde_json::from_str::<Receipt>(body) else { continue };
            let changed = match forgotten {
                Some(id) if r.shown.iter().any(|s| s == id) || r.text.contains(&format!("[{id}]")) => {
                    r.text = format!("(removed: record {id} was forgotten, and the text shown with it is no longer kept)");
                    r.text_sha = sha(&r.text);
                    r.inputs.skip.retain(|s| s != id);
                    true
                }
                Some(_) => false,
                None => {
                    let mut v = serde_json::to_value(&r).map_err(|e| err(e.to_string()))?;
                    if crate::redact::redact_json(&mut v) {
                        r = serde_json::from_value(v).map_err(|e| err(e.to_string()))?;
                        true
                    } else {
                        false
                    }
                }
            };
            if changed {
                self.put(&r)?;
                n += 1;
            }
        }
        if n > 0 {
            self.db.optimize()?;
        }
        Ok(n)
    }
}
