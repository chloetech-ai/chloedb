//! ChloeCtx's performance and data-correctness baseline.
//!
//! Two kinds of test, over one deterministic synthetic corpus (no model
//! files, no network: the `hash:256` embedder):
//!
//! * **Correctness** (runs in a plain `cargo test`): what goes in comes out
//!   field for field, also after a close and reopen; searches find planted
//!   records and respect their filters; deletes and retags take effect;
//!   concurrent appends get unique, gap-free sequence numbers.
//!
//! * **Performance** (`#[ignore]`d, run on purpose): throughput and latency
//!   of loading, appending, searching and of the daemon's hooks, with
//!   retrieval quality on the planted records. It compares the run with the
//!   committed baseline `results/baseline/baseline.json` and fails when a
//!   metric is worse than its tolerance.
//!
//! ```text
//! # check the current code against the baseline
//! cargo test --profile ci --test perf_baseline -- --ignored --nocapture
//! # record a new baseline (after an intended change; say why in the PR)
//! CHLOECTX_PERF=record cargo test --profile ci --test perf_baseline -- --ignored --nocapture
//! ```
//!
//! Absolute times belong to the machine that recorded them
//! (`meta.machine` in the baseline): re-record on another machine rather
//! than comparing across machines. The run's own numbers go to
//! `$CARGO_TARGET_TMPDIR/perf_current.json`. Run nothing else heavy while it
//! measures; `CHLOECTX_PERF_SCALE=0.25` shrinks every size for a quick look
//! (its numbers are not compared with the baseline).

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use chloectx::daemon::Daemon;
use chloectx::embed;
use chloectx::recall::{self, RankParams};
use chloectx::record::{Draft, Kind, Link, Record};
use chloectx::store::{Filter, Store, StoreOptions};
use serde_json::{json, Value as Json};

const EMBEDDER: &str = "hash:256";

// ---------------------------------------------------------------- corpus

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Letters only, so every tokenizer keeps a word whole.
fn letters(mut n: usize, len: usize) -> String {
    let mut s = String::new();
    for _ in 0..len {
        s.push((b'a' + (n % 26) as u8) as char);
        n /= 26;
    }
    s
}

struct Corpus {
    vocab: Vec<String>,
    /// For needle `i`: the record's unique word and the words that go with it.
    needle_words: Vec<[String; 4]>,
}

impl Corpus {
    fn new() -> Corpus {
        let mut rng = Rng(0x9E3779B97F4A7C15);
        let vocab = (0..4000).map(|i| format!("{}{}", letters(rng.below(26 * 26 * 26), 3), letters(i, 3))).collect();
        Corpus { vocab, needle_words: Vec::new() }
    }

    fn filler(&self, rng: &mut Rng, words: usize) -> String {
        // Skewed towards the front of the vocabulary, as text is.
        (0..words)
            .map(|_| {
                let u = rng.below(1000) as f64 / 1000.0;
                self.vocab[((u * u * u) * self.vocab.len() as f64) as usize % self.vocab.len()].as_str()
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// `n` records spread over sessions of `per_session`, one in `needle_every`
    /// carrying a unique word and three rare ones no other record has.
    fn records(&mut self, n: usize, per_session: usize, needle_every: usize) -> Vec<Record> {
        let mut rng = Rng(42);
        let kinds = [Kind::Prompt, Kind::Answer, Kind::ToolCall, Kind::ToolResult, Kind::Error, Kind::Note, Kind::Decision];
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let session = format!("sess{:05}-{}", i / per_session, letters(i / per_session, 4));
            let kind = kinds[rng.below(kinds.len())];
            let words = 20 + rng.below(60);
            let mut text = self.filler(&mut rng, words);
            if needle_every > 0 && i % needle_every == 7 {
                let k = self.needle_words.len();
                let w = [format!("zq{}x", letters(k, 5)), format!("qj{}y", letters(k * 7 + 1, 5)), format!("qk{}z", letters(k * 13 + 2, 5)), format!("qw{}v", letters(k * 31 + 3, 5))];
                text = format!("{} {} {} {} {}", w[0], text, w[1], w[2], w[3]);
                self.needle_words.push(w);
            }
            let mut d = Draft::new(kind, text).tag(["-", "build", "test", "deploy"][i % 4]).ctx(self.filler(&mut rng, 6));
            if i % 5 == 0 {
                d = d.files(vec![format!("/proj/src/mod{}.rs", i % 97)]);
            }
            if i % 9 == 0 {
                d = d.link("about", format!("e:topic{}", i % 31));
            }
            d = d.importance((rng.below(100) as f64) / 100.0);
            out.push(Record::from_draft(d, &session, (i % per_session) as i64, i as i64 + 1, 1_700_000_000 + i as i64 * 7));
        }
        out
    }
}

/// The record the i-th needle sits in.
fn needle_record(recs: &[Record], needle: &str) -> Option<String> {
    recs.iter().find(|r| r.text.starts_with(needle)).map(|r| r.id.clone())
}

fn load(store: &Store, recs: &[Record]) {
    for chunk in recs.chunks(1000) {
        let texts: Vec<String> = chunk.iter().map(|r| r.embed_text()).collect();
        let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        let embs = store.embedder.embed(&refs).unwrap();
        store.bulk_insert(chunk, embs).unwrap();
    }
    store.flush().unwrap();
}

fn open_store(path: &Path, sync: bool) -> Store {
    let e: Arc<dyn embed::Embedder> = Arc::from(embed::open(EMBEDDER).unwrap());
    Store::open(path, e, StoreOptions { sync, ..Default::default() }).unwrap()
}

/// A query the needle's record should answer: its unique word and two of the
/// rare ones, in a sentence.
fn needle_query(w: &[String; 4]) -> String {
    format!("what did we decide about {} and {} with {}", w[0], w[2], w[3])
}

fn same_record(a: &Record, b: &Record) -> Result<(), String> {
    let mut bad = Vec::new();
    macro_rules! eq {
        ($f:ident) => {
            if a.$f != b.$f {
                bad.push(format!("{}: {:?} != {:?}", stringify!($f), a.$f, b.$f));
            }
        };
    }
    eq!(id);
    eq!(session);
    eq!(kind);
    eq!(tag);
    eq!(turn);
    eq!(seq);
    eq!(ts);
    eq!(files);
    eq!(text);
    eq!(ctx);
    eq!(links);
    // Stored with two decimals.
    if (a.importance - b.importance).abs() > 0.006 {
        bad.push(format!("importance: {} != {}", a.importance, b.importance));
    }
    if bad.is_empty() {
        Ok(())
    } else {
        Err(format!("{}: {}", a.id, bad.join("; ")))
    }
}

// ------------------------------------------------------------ correctness

/// Everything stored comes back unchanged, in a store as it is after the
/// load, and again after closing and reopening it.
#[test]
fn records_round_trip_and_survive_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory.strata");
    let mut corpus = Corpus::new();
    let recs = corpus.records(3000, 100, 50);

    let check = |store: &Store, when: &str| {
        assert_eq!(store.len(), recs.len() as u64, "{when}: record count");
        let mut seen = 0;
        for chunk in recs.chunks(500) {
            let ids: Vec<String> = chunk.iter().map(|r| r.id.clone()).collect();
            let got = store.get(&ids).unwrap();
            assert_eq!(got.len(), chunk.len(), "{when}: get returned {} of {}", got.len(), chunk.len());
            let by_id: BTreeMap<&str, &Record> = got.iter().map(|r| (r.id.as_str(), r)).collect();
            for want in chunk {
                let have = by_id.get(want.id.as_str()).unwrap_or_else(|| panic!("{when}: {} is missing", want.id));
                same_record(want, have).unwrap_or_else(|e| panic!("{when}: {e}"));
                seen += 1;
            }
        }
        assert_eq!(seen, recs.len());
        // Sessions come back in order, whole.
        let s = &recs[0].session;
        let want: Vec<i64> = recs.iter().filter(|r| &r.session == s).map(|r| r.seq).collect();
        let got: Vec<i64> = store.range(s, *want.first().unwrap(), *want.last().unwrap()).unwrap().iter().map(|r| r.seq).collect();
        assert_eq!(got, want, "{when}: range");
        // The newest come newest first.
        let latest = store.latest(&Kind::ALL, 20, &Filter::default()).unwrap();
        let want: Vec<i64> = (0..20).map(|i| recs.len() as i64 - i).collect();
        assert_eq!(latest.iter().map(|r| r.seq).collect::<Vec<_>>(), want, "{when}: latest");
        // Every planted record is found by words only it has.
        for (k, w) in corpus.needle_words.iter().enumerate() {
            let (hits, _) = recall::search(store, &needle_query(w), &Filter::default(), &RankParams::default()).unwrap();
            let want = needle_record(&recs, &w[0]).unwrap();
            let pos = hits.iter().position(|h| h.record.id == want);
            assert!(matches!(pos, Some(p) if p < 3), "{when}: needle {k} ({}) at {pos:?}", w[0]);
        }
        // Another query is answered the same way twice.
        let q = needle_query(&corpus.needle_words[1]);
        let a: Vec<String> = recall::search(store, &q, &Filter::default(), &RankParams::default()).unwrap().0.into_iter().map(|h| h.record.id).collect();
        let b: Vec<String> = recall::search(store, &q, &Filter::default(), &RankParams::default()).unwrap().0.into_iter().map(|h| h.record.id).collect();
        assert_eq!(a, b, "{when}: repeat search");
    };

    {
        let store = open_store(&path, false);
        load(&store, &recs);
        check(&store, "after load");
    }
    let store = open_store(&path, false);
    check(&store, "after reopen");
}

/// A search returns only what its filter allows.
#[test]
fn filters_are_respected() {
    let dir = tempfile::tempdir().unwrap();
    let mut corpus = Corpus::new();
    let recs = corpus.records(2000, 100, 40);
    let store = open_store(&dir.path().join("memory.strata"), false);
    load(&store, &recs);
    let q = needle_query(&corpus.needle_words[3]);
    let live = recs[500].session.clone();
    let window = recs.iter().find(|r| r.session == live).unwrap().seq + 50;

    let f = Filter { kinds: vec![Kind::Note, Kind::Decision], ..Default::default() };
    for h in recall::search(&store, &q, &f, &RankParams::default()).unwrap().0 {
        assert!(matches!(h.record.kind, Kind::Note | Kind::Decision), "{:?}", h.record.kind);
    }
    let f = Filter { session: Some(live.clone()), ..Default::default() };
    let hits = recall::search(&store, "alpha beta", &f, &RankParams::default()).unwrap().0;
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|h| h.record.session == live));
    let f = Filter { since_ts: Some(1_700_000_000 + 1000 * 7), until_ts: Some(1_700_000_000 + 1500 * 7), ..Default::default() };
    for h in recall::search(&store, &q, &f, &RankParams::default()).unwrap().0 {
        assert!((1_700_000_000 + 1000 * 7..1_700_000_000 + 1500 * 7).contains(&h.record.ts), "ts {}", h.record.ts);
    }
    let f = Filter { exclude_window: Some((live.clone(), window)), ..Default::default() };
    for w in corpus.needle_words.iter().take(10) {
        for h in recall::search(&store, &needle_query(w), &f, &RankParams::default()).unwrap().0 {
            assert!(!(h.record.session == live && h.record.seq >= window), "{} is in the live window", h.record.id);
        }
    }
    // BM25 and vector search alone honour it as well.
    let f = Filter { kinds: vec![Kind::Error], ..Default::default() };
    assert!(store.bm25_top(&q, 20, &f).unwrap().iter().all(|(r, _)| r.kind == Kind::Error));
    let v = store.embedder.embed_query(&q).unwrap();
    assert!(store.vector_top(&v, 20, &f).unwrap().iter().all(|(r, _)| r.kind == Kind::Error));
}

/// Deletes and retags take effect at once and after a reopen.
#[test]
fn delete_and_retag_take_effect() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory.strata");
    let mut corpus = Corpus::new();
    let recs = corpus.records(1000, 100, 25);
    let victim = needle_record(&recs, &corpus.needle_words[2][0]).unwrap();
    let kept = needle_record(&recs, &corpus.needle_words[3][0]).unwrap();
    let q = needle_query(&corpus.needle_words[2]);
    {
        let store = open_store(&path, false);
        load(&store, &recs);
        let ids = |s: &Store| recall::search(s, &q, &Filter::default(), &RankParams::default()).unwrap().0.into_iter().map(|h| h.record.id).collect::<Vec<_>>();
        assert!(ids(&store).contains(&victim));
        assert!(store.delete(&victim).unwrap());
        assert!(!store.delete(&victim).unwrap(), "a second delete finds nothing");
        assert!(!ids(&store).contains(&victim));
        assert!(store.get(&[victim.clone()]).unwrap().is_empty());
        assert!(store.retag(&kept, "pinned").unwrap());
        assert_eq!(store.get(&[kept.clone()]).unwrap()[0].tag, "pinned");
        assert_eq!(store.len(), 999);
        store.flush().unwrap();
    }
    let store = open_store(&path, false);
    assert_eq!(store.len(), 999);
    assert!(store.get(&[victim]).unwrap().is_empty());
    let got = store.get(&[kept]).unwrap();
    assert_eq!(got[0].tag, "pinned");
    // Retagging changed the tag and nothing else.
    let mut want = recs.iter().find(|r| r.id == got[0].id).unwrap().clone();
    want.tag = "pinned".into();
    same_record(&want, &got[0]).unwrap();
}

/// Appends from several threads get distinct, consecutive sequence numbers,
/// and every one of them is stored as written.
#[test]
fn concurrent_appends_lose_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(open_store(&dir.path().join("memory.strata"), false));
    let (threads, each) = (4usize, 150usize);
    let handles: Vec<_> = (0..threads)
        .map(|t| {
            let store = store.clone();
            std::thread::spawn(move || {
                let mut ids = Vec::new();
                for i in 0..each {
                    let d = Draft::new(Kind::Answer, format!("thread{t} message{i} marker{}", letters(t * 1000 + i, 4))).link("about", "e:thing");
                    let recs = store.append(&format!("sess-{t}"), i as i64, 1_700_000_000 + i as i64, vec![d]).unwrap();
                    assert_eq!(recs.len(), 1);
                    ids.push((recs[0].id.clone(), recs[0].seq, recs[0].text.clone()));
                }
                ids
            })
        })
        .collect();
    let all: Vec<(String, i64, String)> = handles.into_iter().flat_map(|h| h.join().unwrap()).collect();
    assert_eq!(all.len(), threads * each);
    let ids: HashSet<&str> = all.iter().map(|a| a.0.as_str()).collect();
    assert_eq!(ids.len(), all.len(), "ids are unique");
    let mut seqs: Vec<i64> = all.iter().map(|a| a.1).collect();
    seqs.sort();
    seqs.dedup();
    assert_eq!(seqs.len(), all.len(), "seqs are unique");
    assert_eq!(store.len(), (threads * each) as u64);
    let got = store.get(&all.iter().map(|a| a.0.clone()).collect::<Vec<_>>()).unwrap();
    assert_eq!(got.len(), all.len());
    let text: BTreeMap<&str, &str> = all.iter().map(|a| (a.0.as_str(), a.2.as_str())).collect();
    for r in &got {
        assert_eq!(text[r.id.as_str()], r.text);
        assert_eq!(r.links, vec![Link::new("about", "e:thing")]);
    }
    // Numbers of one thread's records rise in the order it appended them.
    for t in 0..threads {
        let mine = store.range(&format!("sess-{t}"), 1, i64::MAX / 2).unwrap();
        assert_eq!(mine.len(), each);
        assert!(mine.windows(2).all(|w| w[0].seq < w[1].seq && w[0].turn + 1 == w[1].turn));
    }
}

// ------------------------------------------------------------ performance

struct Metrics {
    m: BTreeMap<String, Metric>,
}

#[derive(Clone)]
struct Metric {
    value: f64,
    unit: &'static str,
    /// Which way is better, and how far it may move the wrong way before
    /// the check fails (a ratio for time and throughput; an absolute amount
    /// for quality).
    better: Better,
    tol: f64,
}

#[derive(Clone, Copy, PartialEq)]
enum Better {
    Lower,
    Higher,
    /// A quality score: may not drop by more than `tol`.
    Quality,
}

impl Metrics {
    fn lat(&mut self, name: &str, samples: &mut Vec<f64>) {
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p = |q: f64| samples[((samples.len() as f64 - 1.0) * q).round() as usize];
        for (suffix, q, tol) in [("p50", 0.50, 1.6), ("p95", 0.95, 2.0), ("p99", 0.99, 3.0)] {
            self.put(&format!("{name}.{suffix}_ms"), p(q), "ms", Better::Lower, tol);
        }
    }
    fn put(&mut self, name: &str, value: f64, unit: &'static str, better: Better, tol: f64) {
        println!("  {name:<44} {value:>12.3} {unit}");
        self.m.insert(name.to_string(), Metric { value, unit, better, tol });
    }
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn dir_size(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else { return std::fs::metadata(p).map(|m| m.len()).unwrap_or(0) };
    rd.flatten().map(|e| dir_size(&e.path())).sum()
}

fn rss_mb() -> f64 {
    let mut u: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut u) };
    let raw = u.ru_maxrss as f64;
    // macOS reports bytes, Linux kilobytes.
    if cfg!(target_os = "macos") { raw / 1048576.0 } else { raw / 1024.0 }
}

fn machine() -> String {
    let sh = |c: &str| std::process::Command::new("sh").arg("-c").arg(c).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let cpu = if cfg!(target_os = "macos") { sh("sysctl -n machdep.cpu.brand_string") } else { sh("grep -m1 'model name' /proc/cpuinfo | cut -d: -f2") };
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0);
    format!("{} {} / {} / {cores} cores", std::env::consts::OS, std::env::consts::ARCH, cpu.trim())
}

fn scaled(n: usize, scale: f64) -> usize {
    ((n as f64 * scale) as usize).max(50)
}

/// Searches measured at one store size: latency, and how often the planted
/// records are found.
fn search_suite(m: &mut Metrics, tag: &str, store: &Store, recs: &[Record], corpus: &Corpus, queries: usize) {
    let params = RankParams::default();
    let n = corpus.needle_words.len().min(queries);
    let step = (corpus.needle_words.len() / n).max(1);
    let picks: Vec<usize> = (0..n).map(|i| i * step).collect();
    let (mut hybrid, mut bm, mut vec, mut hit1, mut hit5, mut mrr) = (Vec::new(), Vec::new(), Vec::new(), 0usize, 0usize, 0.0);
    // One round first: the first query after a load opens segments.
    let _ = recall::search(store, &needle_query(&corpus.needle_words[0]), &Filter::default(), &params);
    for &k in &picks {
        let w = &corpus.needle_words[k];
        let q = needle_query(w);
        let want = needle_record(recs, &w[0]).unwrap();
        let t = Instant::now();
        let (hits, _) = recall::search(store, &q, &Filter::default(), &params).unwrap();
        hybrid.push(ms(t));
        if let Some(p) = hits.iter().position(|h| h.record.id == want) {
            hit1 += (p == 0) as usize;
            hit5 += (p < 5) as usize;
            mrr += 1.0 / (p + 1) as f64;
        }
        let t = Instant::now();
        let _ = store.bm25_top(&q, 50, &Filter::default()).unwrap();
        bm.push(ms(t));
        let v = store.embedder.embed_query(&q).unwrap();
        let t = Instant::now();
        let _ = store.vector_top(&v, 50, &Filter::default()).unwrap();
        vec.push(ms(t));
    }
    m.lat(&format!("search.{tag}.hybrid"), &mut hybrid);
    m.lat(&format!("search.{tag}.bm25"), &mut bm);
    m.lat(&format!("search.{tag}.vector"), &mut vec);
    let n = picks.len() as f64;
    m.put(&format!("quality.{tag}.hit_at_1"), hit1 as f64 / n, "", Better::Quality, 0.02);
    m.put(&format!("quality.{tag}.hit_at_5"), hit5 as f64 / n, "", Better::Quality, 0.01);
    m.put(&format!("quality.{tag}.mrr"), mrr / n, "", Better::Quality, 0.02);
}

fn run_perf(scale: f64) -> Metrics {
    let mut m = Metrics { m: BTreeMap::new() };
    let tmp = tempfile::tempdir().unwrap();
    let (small, big) = (scaled(2_000, scale), scaled(20_000, scale));

    println!("-- load and search at {small} and {big} records");
    let mut corpus = Corpus::new();
    let recs = corpus.records(big, 100, 100);
    let path = tmp.path().join("big.strata");
    {
        let store = open_store(&path, false);
        // Small store first: the same first `small` records.
        let small_path = tmp.path().join("small.strata");
        let sstore = open_store(&small_path, false);
        load(&sstore, &recs[..small]);
        let mut sc = Corpus::new();
        let _ = sc.records(small, 100, 100);
        search_suite(&mut m, "2k", &sstore, &recs[..small], &sc, 100);
        drop(sstore);

        let t = Instant::now();
        load(&store, &recs);
        let secs = t.elapsed().as_secs_f64();
        m.put("load.bulk_records_per_s", big as f64 / secs, "rec/s", Better::Higher, 1.6);
        search_suite(&mut m, "20k", &store, &recs, &corpus, 200);

        // Point reads and scans.
        let mut get = Vec::new();
        let mut rng = Rng(7);
        for _ in 0..200 {
            let ids: Vec<String> = (0..10).map(|_| recs[rng.below(big)].id.clone()).collect();
            let t = Instant::now();
            assert_eq!(store.get(&ids).unwrap().len(), ids.iter().collect::<HashSet<_>>().len());
            get.push(ms(t));
        }
        m.lat("read.get10", &mut get);
        let (mut latest, mut range) = (Vec::new(), Vec::new());
        for i in 0..200 {
            let t = Instant::now();
            let l = store.latest(&[Kind::Note, Kind::Decision], 20, &Filter::default()).unwrap();
            latest.push(ms(t));
            assert_eq!(l.len(), 20);
            let s = &recs[(i * 97) % big].session;
            let first = recs.iter().find(|r| &r.session == s).unwrap().seq;
            let t = Instant::now();
            assert_eq!(store.range(s, first, first + 99).unwrap().len(), 100);
            range.push(ms(t));
        }
        m.lat("read.latest20", &mut latest);
        m.lat("read.session_range100", &mut range);
        let t = Instant::now();
        let n = store.query("SELECT kind, COUNT(*) FROM memory GROUP BY kind").unwrap().rows.len();
        m.put("read.sql_group_by_kind_ms", ms(t), "ms", Better::Lower, 2.0);
        assert_eq!(n, 7);
    }
    let t = Instant::now();
    let store = open_store(&path, false);
    m.put("open.reopen_20k_ms", ms(t), "ms", Better::Lower, 2.0);
    assert_eq!(store.len(), big as u64);
    let t = Instant::now();
    let _ = recall::search(&store, &needle_query(&corpus.needle_words[5]), &Filter::default(), &RankParams::default()).unwrap();
    m.put("open.first_search_after_reopen_ms", ms(t), "ms", Better::Lower, 2.5);
    drop(store);
    m.put("disk.store_20k_mb", dir_size(&path) as f64 / 1048576.0, "MB", Better::Lower, 1.25);

    println!("-- appends");
    // Without fsync: what the daemon does by default with sync off, and the
    // embedding and indexing cost of a write.
    {
        let store = open_store(&tmp.path().join("append.strata"), false);
        let n = scaled(3_000, scale);
        let mut lat = Vec::new();
        let t0 = Instant::now();
        for i in 0..n {
            let ds = vec![
                Draft::new(Kind::ToolCall, format!("Bash: cargo test --package crate{} -- case{}", i % 40, letters(i, 4))).files(vec![format!("/proj/src/f{}.rs", i % 50)]),
                Draft::new(Kind::ToolResult, format!("test result: ok. {} passed; 0 failed; finished in {}.{:02}s {}", i % 90, i % 7, i % 100, corpus.filler(&mut Rng(i as u64 + 1), 30))),
            ];
            let t = Instant::now();
            let r = store.append("append-session", i as i64, 1_700_000_000 + i as i64, ds).unwrap();
            lat.push(ms(t));
            assert_eq!(r.len(), 2);
        }
        m.put("append.nosync_pairs_per_s", n as f64 / t0.elapsed().as_secs_f64(), "pairs/s", Better::Higher, 1.6);
        m.lat("append.nosync_pair", &mut lat);
        // Search sees the last append at once.
        let (hits, _) = recall::search(&store, &format!("case{}", letters(n - 1, 4)), &Filter::default(), &RankParams::default()).unwrap();
        assert!(hits.iter().take(3).any(|h| h.record.text.contains(&format!("case{}", letters(n - 1, 4)))), "the newest append is not found");
    }
    // With fsync on every commit.
    {
        let store = open_store(&tmp.path().join("sync.strata"), true);
        let mut lat = Vec::new();
        for i in 0..scaled(300, scale) {
            let t = Instant::now();
            store.append("sync-session", i as i64, 1_700_000_000 + i as i64, vec![Draft::new(Kind::Answer, format!("answer {i} about topic{}", i % 13))]).unwrap();
            lat.push(ms(t));
        }
        m.lat("append.sync_single", &mut lat);
    }
    // Four writers at once.
    {
        let store = Arc::new(open_store(&tmp.path().join("conc.strata"), false));
        let per = scaled(1_000, scale);
        let t0 = Instant::now();
        let hs: Vec<_> = (0..4)
            .map(|t| {
                let store = store.clone();
                std::thread::spawn(move || {
                    let mut lat = Vec::new();
                    for i in 0..per {
                        let t1 = Instant::now();
                        store.append(&format!("c{t}"), i as i64, 1_700_000_000 + i as i64, vec![Draft::new(Kind::Answer, format!("writer {t} message {i} {}", letters(i, 5)))]).unwrap();
                        lat.push(ms(t1));
                    }
                    lat
                })
            })
            .collect();
        let mut lat: Vec<f64> = hs.into_iter().flat_map(|h| h.join().unwrap()).collect();
        m.put("append.4writers_per_s", (4 * per) as f64 / t0.elapsed().as_secs_f64(), "rec/s", Better::Higher, 1.6);
        m.lat("append.4writers", &mut lat);
        assert_eq!(store.len(), (4 * per) as u64);
    }

    // Opening a store whose log was never flushed (a daemon killed before it
    // idled out): the cost of replaying the write-ahead log, which the
    // reopen above (a flushed store) does not include.
    {
        let path = tmp.path().join("replay.strata");
        let wal = PathBuf::from(format!("{}-wal", path.display()));
        let n = scaled(4_000, scale).min(4_000);
        {
            let store = open_store(&path, false);
            for i in 0..n {
                store.append("replay-session", i as i64, 1_700_000_000 + i as i64, vec![Draft::new(Kind::Answer, format!("replayed record {i} {}", corpus.filler(&mut Rng(i as u64 + 9), 40)))]).unwrap();
            }
            // Dropped, not flushed: every record is still in the log.
        }
        let wal_len = std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0);
        assert!(wal_len > 16 * 1024, "the log was flushed or missing ({wal_len} bytes): this does not measure a replay");
        let t = Instant::now();
        let store = open_store(&path, false);
        m.put("open.reopen_unflushed_4k_ms", ms(t), "ms", Better::Lower, 2.0);
        assert_eq!(store.len(), n as u64, "records lost in the replay");
        let t = Instant::now();
        let (hits, _) = recall::search(&store, &format!("replayed record {}", n - 1), &Filter::default(), &RankParams::default()).unwrap();
        m.put("open.first_search_after_replay_ms", ms(t), "ms", Better::Lower, 2.5);
        assert!(hits.iter().take(3).any(|h| h.record.text.starts_with(&format!("replayed record {} ", n - 1))), "the last replayed record is not found");
        let first = store.range("replay-session", 1, 1).unwrap();
        assert_eq!(first.len(), 1);
        m.put("disk.wal_unflushed_4k_mb", wal_len as f64 / 1048576.0, "MB", Better::Lower, 1.25);
    }

    println!("-- daemon hooks");
    {
        let dir = tmp.path().join("daemon");
        std::fs::create_dir_all(&dir).unwrap();
        // No model calls: deep recall and distillation would run `claude -p`
        // (the environment's CHLOECTX_DEEP overrides the config, so set both).
        std::fs::write(dir.join("config.json"), format!(r#"{{"embedder": "{EMBEDDER}", "sync": false, "distill": {{"enabled": false}}, "deep": {{"mode": "off"}}}}"#)).unwrap();
        std::env::set_var("CHLOECTX_DEEP", "off");
        std::env::set_var("ENGRAM_DEEP", "off");
        let d = Arc::new(Daemon::open(&dir, "/proj").unwrap());
        let hook = |event: &str, payload: Json| -> (f64, String) {
            let t = Instant::now();
            let (resp, deferred) = d.handle(&json!({"op": "hook", "event": event, "payload": payload}));
            let took = ms(t);
            if let Some(f) = deferred {
                f();
            }
            assert!(resp.get("error").is_none(), "{resp}");
            (took, resp["output"]["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or("").to_string())
        };
        let mut c2 = Corpus::new();
        let _ = c2.records(10, 10, 0);
        let sessions = 6;
        let turns = scaled(120, scale);
        let (mut prompt, mut post, mut stop) = (Vec::new(), Vec::new(), Vec::new());
        let mut remembered = false;
        for s in 0..sessions {
            let sid = format!("daemon-session-{s}");
            let (_, brief) = hook("SessionStart", json!({"session_id": sid, "source": "startup"}));
            if s > 0 {
                remembered |= brief.contains("checked arithmetic");
            }
            for t in 0..turns {
                let text = format!("{} step{} in module{}", c2.filler(&mut Rng((s * 1000 + t) as u64 + 3), 12), t, t % 11);
                let prompt_text = if s == 0 && t == 0 { "Always use checked arithmetic for balances.".to_string() } else { text.clone() };
                let (p, _) = hook("UserPromptSubmit", json!({"session_id": sid, "prompt": prompt_text}));
                prompt.push(p);
                for k in 0..3 {
                    let (p, _) = hook(
                        "PostToolUse",
                        json!({"session_id": sid, "tool_name": "Bash", "tool_input": {"command": format!("cargo test case{k}")}, "tool_response": {"stdout": text, "stderr": ""}}),
                    );
                    post.push(p);
                }
                let (p, _) = hook("Stop", json!({"session_id": sid, "last_assistant_message": format!("Done with step {t}: {text}")}));
                stop.push(p);
            }
        }
        // A preference said in the first session reaches the briefing of a later one.
        assert!(remembered, "the preference from the first session is missing from later briefings");
        m.put("daemon.records", d.store.len() as f64, "records", Better::Quality, 0.0);
        m.lat("daemon.hook_user_prompt_submit", &mut prompt);
        m.lat("daemon.hook_post_tool_use", &mut post);
        m.lat("daemon.hook_stop", &mut stop);
        // A prompt about earlier work finds it.
        let sid = "daemon-session-final";
        hook("SessionStart", json!({"session_id": sid, "source": "startup"}));
        let (_, ctx) = hook("UserPromptSubmit", json!({"session_id": sid, "prompt": "What did we decide about checked arithmetic for balances earlier?"}));
        m.put("quality.daemon.recalls_earlier_preference", ctx.contains("checked arithmetic") as u8 as f64, "", Better::Quality, 0.0);
        m.put("rss.peak_mb", rss_mb(), "MB", Better::Lower, 1.5);
    }
    m
}

fn baseline_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("results/baseline/baseline.json")
}

fn to_json(m: &Metrics, scale: f64) -> Json {
    let metrics: serde_json::Map<String, Json> = m
        .m
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                json!({"value": (v.value * 1e4).round() / 1e4, "unit": v.unit, "better": match v.better { Better::Lower => "lower", Better::Higher => "higher", Better::Quality => "quality" }, "tolerance": v.tol}),
            )
        })
        .collect();
    json!({
        "meta": {
            "machine": machine(),
            "embedder": EMBEDDER,
            "scale": scale,
            "debug_assertions": cfg!(debug_assertions),
            "chloectx": env!("CARGO_PKG_VERSION"),
            "git": std::process::Command::new("git").args(["rev-parse", "--short", "HEAD"]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default(),
        },
        "metrics": metrics,
    })
}

/// The metrics that are worse than the baseline's allow.
fn regressions(m: &Metrics, base: &Json) -> Vec<String> {
    let mut out = Vec::new();
    for (name, b) in base["metrics"].as_object().unwrap() {
        let Some(cur) = m.m.get(name) else {
            out.push(format!("{name}: in the baseline, not measured now"));
            continue;
        };
        let bv = b["value"].as_f64().unwrap();
        let tol = b["tolerance"].as_f64().unwrap();
        let (cv, bad) = (cur.value, match b["better"].as_str().unwrap() {
            // Thirty microseconds are noise whatever the ratio.
            "lower" => cur.value > bv * tol && cur.value - bv > if cur.unit == "ms" { 0.03 } else { 0.0 },
            "higher" => cur.value < bv / tol,
            _ => cur.value < bv - tol - 1e-9,
        });
        if bad {
            out.push(format!("{name}: {cv:.3} {} against a baseline of {bv:.3} (tolerance {tol})", cur.unit));
        }
    }
    out
}

/// Measures, writes `perf_current.json`, and compares with the baseline (or,
/// with `CHLOECTX_PERF=record`, makes it).
#[test]
#[ignore = "a measurement: run with --ignored, on a quiet machine"]
fn performance_against_baseline() {
    let scale: f64 = std::env::var("CHLOECTX_PERF_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(1.0);
    println!("chloectx performance baseline on {}", machine());
    let t = Instant::now();
    let m = run_perf(scale);
    println!("-- measured in {:.0} s", t.elapsed().as_secs_f64());
    let cur = to_json(&m, scale);
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("perf_current.json");
    std::fs::write(&out, serde_json::to_string_pretty(&cur).unwrap() + "\n").unwrap();
    println!("this run: {}", out.display());

    let path = baseline_path();
    if std::env::var("CHLOECTX_PERF").as_deref() == Ok("record") {
        assert!(scale == 1.0, "a baseline is recorded at scale 1");
        assert!(!cfg!(debug_assertions), "record the baseline with an optimised build (--profile ci or --release)");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string_pretty(&cur).unwrap() + "\n").unwrap();
        println!("baseline written: {}", path.display());
        return;
    }
    let Ok(text) = std::fs::read_to_string(&path) else {
        panic!("no baseline at {}; record one with CHLOECTX_PERF=record", path.display());
    };
    let base: Json = serde_json::from_str(&text).unwrap();
    if scale != 1.0 {
        println!("scale {scale}: numbers are not compared with the baseline");
        return;
    }
    assert_eq!(base["meta"]["debug_assertions"], cur["meta"]["debug_assertions"], "the baseline and this build differ in optimisation");
    if base["meta"]["machine"] != cur["meta"]["machine"] {
        println!("note: the baseline was recorded on {}; this is {}", base["meta"]["machine"], cur["meta"]["machine"]);
    }
    let bad = regressions(&m, &base);
    assert!(bad.is_empty(), "{} metric(s) regressed against {}:\n  {}", bad.len(), path.display(), bad.join("\n  "));
    println!("within the baseline's tolerances ({} metrics)", base["metrics"].as_object().unwrap().len());
}
