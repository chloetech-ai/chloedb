//! A schedule file: gzip-compressed JSON lines, a header, then one line a
//! request in the order of their planned times, then a trailer with what
//! the plan measured of its own requests. A schedule names no engine:
//! `replay` turns each line into each engine's request when it sends it.
//! Its hash (SHA-256 of the uncompressed lines, `zcat FILE | sha256sum`)
//! goes into every result replayed from it.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write as _};
use std::path::Path;

use flate2::read::MultiGzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::profile::{Profile, Source};
use crate::Knobs;

pub const FORMAT: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "shape", rename_all = "lowercase")]
pub enum Shape {
    /// The profile's day, `rate` requests a second at its daily peak (bursts and flash crowds on top).
    Day { rate: f64 },
    /// The profile's ramp: steps of rising rates, each sent until the SLO breaks.
    Ramp,
    /// `run`'s closed loop: each kind's requests in turn, at each concurrency.
    Closed,
}

impl Shape {
    pub fn name(&self) -> &'static str {
        match self {
            Shape::Day { .. } => "day",
            Shape::Ramp => "ramp",
            Shape::Closed => "closed",
        }
    }
}

/// A span of the timed schedule at one rate (a ramp's step; a day is one).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Step {
    pub from_s: f64,
    pub to_s: f64,
    /// Requests a second planned (a day's: at its peak).
    pub rate: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Header {
    pub format: u32,
    pub profile: Profile,
    /// SHA-256 of the profile's file as it was read.
    pub profile_sha256: String,
    pub seed: u64,
    #[serde(flatten)]
    pub shape: Shape,
    pub collection: String,
    pub docs: usize,
    pub dim: usize,
    pub source: Source,
    /// Queries of the source with text.
    pub queries: usize,
    /// Where inserted documents come from: `heldout` (heldout.jsonl, documents not loaded) or `docs` (copies of loaded ones under new keys).
    pub inserts_from: String,
    pub inserts_available: usize,
    /// Requests planned before `warmup_s` are the warm-up (left out of every measurement).
    pub warmup_s: f64,
    /// The timed schedule's end (after the warm-up).
    pub end_s: f64,
    pub steps: Vec<Step>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    #[default]
    Insert,
    Update,
    Delete,
}

/// A write: new documents (copies of `rows` of the insert source under
/// `keys`), one field changed (`rating` of the documents at `rows`, keys
/// `keys`), or documents of this schedule's deleted (`keys`).
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Write {
    pub op: Op,
    pub keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<f64>,
    /// The first new document carries this word (no other has it); a prober times how soon a search finds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark: Option<String>,
    /// The batch job the write belongs to, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<u32>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(x: &u64) -> bool {
    *x == 0
}

/// One request of the schedule.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Entry {
    /// Planned seconds from the schedule's start. A step that waits for the
    /// one before is planned as if answers took no time.
    pub t: f64,
    /// The session of a step that waits for an answer (0: none waits, each request goes at its time).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub s: u64,
    /// The session's last step.
    #[serde(default, skip_serializing_if = "is_false")]
    pub last: bool,
    /// Sent this long after the session's step before is answered (and not before `t`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub think_ms: Option<f64>,
    /// Part of the warm-up.
    #[serde(default, skip_serializing_if = "is_false")]
    pub warm: bool,
    /// A kind of `KINDS`, or `write`.
    pub k: String,
    /// The query: its row of the source's vectors, its id and its text (misspelt when `typo`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub q: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub typo: bool,
    /// Drawn from a flash crowd's queries.
    #[serde(default, skip_serializing_if = "is_false")]
    pub flash: bool,
    /// A key lookup's key and its row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<u64>,
    /// A similar-items request's item (a row of the documents) and the keys it leaves out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seen: Vec<String>,
    /// The SQL aggregate's year.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<i64>,
    /// Filters and prefix other than `run`'s, and the share of documents the filter keeps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knobs: Option<Knobs>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sel: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub w: Option<Write>,
}

/// What the plan measured of its own requests (the trailer).
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Stats {
    pub requests: u64,
    pub warm_requests: u64,
    pub by_kind: BTreeMap<String, u64>,
    pub sessions: u64,
    pub waiting_sessions: u64,
    pub requests_per_session: f64,
    pub query_draws: u64,
    pub distinct_queries: u64,
    /// Draws of a query never drawn before in the timed schedule.
    pub first_time_share: f64,
    /// Draws that went to the head's one most frequent query.
    pub top_query_share: f64,
    pub tail_draws: u64,
    pub tail_wrapped: u64,
    pub typos: u64,
    pub flash_requests: u64,
    pub writes: BTreeMap<String, u64>,
    pub docs_inserted: u64,
    pub marks: u64,
    /// Per filtered kind: quantiles (p1, p10, p50, p90, p99) of the share of documents its filters keep.
    pub selectivity: BTreeMap<String, Vec<f64>>,
    pub mean_rate: f64,
    /// The most requests planned in a 10 s window, a second.
    pub peak_10s_rate: f64,
    pub per_step: Vec<u64>,
}

#[derive(Deserialize, Serialize)]
struct Trailer {
    end: Stats,
}

pub struct Writer {
    gz: GzEncoder<BufWriter<File>>,
    hash: Sha256,
}

impl Writer {
    pub fn create(path: &Path, header: &Header) -> Result<Writer, String> {
        let f = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        // Level 6, and flate2 writes no time into the gzip header: the same schedule makes the same file.
        let mut w = Writer { gz: GzEncoder::new(BufWriter::with_capacity(1 << 20, f), Compression::new(6)), hash: Sha256::new() };
        w.line(&serde_json::to_string(header).map_err(|e| e.to_string())?)?;
        Ok(w)
    }

    fn line(&mut self, s: &str) -> Result<(), String> {
        self.hash.update(s.as_bytes());
        self.hash.update(b"\n");
        self.gz.write_all(s.as_bytes()).and_then(|_| self.gz.write_all(b"\n")).map_err(|e| e.to_string())
    }

    pub fn entry(&mut self, e: &Entry) -> Result<(), String> {
        self.line(&serde_json::to_string(e).map_err(|e| e.to_string())?)
    }

    /// Writes the trailer, closes the file and returns the schedule's hash.
    pub fn finish(mut self, stats: &Stats) -> Result<String, String> {
        self.line(&serde_json::to_string(&Trailer { end: stats.clone() }).map_err(|e| e.to_string())?)?;
        let mut inner = self.gz.finish().map_err(|e| e.to_string())?;
        inner.flush().map_err(|e| e.to_string())?;
        Ok(hex(&self.hash.finalize()))
    }
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn sha256_hex(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

/// Reads a schedule a line at a time.
pub struct Reader {
    lines: std::io::Lines<BufReader<MultiGzDecoder<BufReader<File>>>>,
    hash: Sha256,
    pub header: Header,
    pub stats: Option<Stats>,
}

impl Reader {
    pub fn open(path: &Path) -> Result<Reader, String> {
        let f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut lines = BufReader::with_capacity(1 << 20, MultiGzDecoder::new(BufReader::new(f))).lines();
        let first = lines.next().ok_or("an empty schedule")?.map_err(|e| e.to_string())?;
        let header: Header = serde_json::from_str(&first).map_err(|e| format!("{}: the header: {e}", path.display()))?;
        if header.format != FORMAT {
            return Err(format!("{}: a schedule of format {}; this replay reads {FORMAT}", path.display(), header.format));
        }
        let mut hash = Sha256::new();
        hash.update(first.as_bytes());
        hash.update(b"\n");
        Ok(Reader { lines, hash, header, stats: None })
    }

    /// The next request (None at the end; the trailer's statistics are then in `stats`).
    pub fn next_entry(&mut self) -> Option<Result<Entry, String>> {
        let line = match self.lines.next()? {
            Ok(l) => l,
            Err(e) => return Some(Err(e.to_string())),
        };
        self.hash.update(line.as_bytes());
        self.hash.update(b"\n");
        if line.starts_with("{\"end\":") {
            return match serde_json::from_str::<Trailer>(&line) {
                Ok(t) => {
                    self.stats = Some(t.end);
                    None
                }
                Err(e) => Some(Err(format!("the trailer: {e}"))),
            };
        }
        Some(serde_json::from_str(&line).map_err(|e| format!("a request: {e}")))
    }

    /// The hash of every line read so far (all of them once `next_entry` gave None).
    pub fn hash(&self) -> String {
        hex(&self.hash.clone().finalize())
    }
}
