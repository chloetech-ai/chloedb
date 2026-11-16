//! `chloedb-arxiv prepare`: arXiv's metadata snapshot (Parquet), the weeks
//! after it from OAI-PMH (`arXivRaw` pages that `fetch` saved) and
//! Crossref's citations of arXiv papers (Parquet), turned into the rows of
//! the app's tables as JSON lines for `load`.
//!
//! Two passes over the papers. The first reads who wrote what, the DOIs and
//! the dates: it decides who is who (a name written with initials joins the
//! one person of that last name whose forenames have those initials), which
//! citing works are arXiv papers (by the DOI of their published version),
//! and so the citation graph between arXiv papers. The second writes each
//! paper with its techniques, domains and counts, and adds up the people,
//! the techniques, the domains and the categories.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{json, Value};

use crate::concepts::{self, words};
use crate::paper::{self, Built};
use crate::raw::{self, Raw, Strs};

pub struct Opts {
    pub snapshot: PathBuf,
    pub oai: Option<PathBuf>,
    pub cites: Option<PathBuf>,
    pub topics: Option<PathBuf>,
    pub out: PathBuf,
    /// Only papers whose primary category starts with one of these (a
    /// smaller set for tests and laptops); empty: all.
    pub only: Vec<String>,
    /// "Now" for the growth of the last twelve months (Unix seconds).
    pub now: i64,
}

struct Out {
    w: BufWriter<File>,
    n: u64,
}

impl Out {
    fn new(dir: &Path, name: &str) -> Result<Out, String> {
        let f = File::create(dir.join(name)).map_err(|e| format!("{name}: {e}"))?;
        Ok(Out { w: BufWriter::with_capacity(1 << 20, f), n: 0 })
    }
    fn row(&mut self, v: &Value) -> Result<(), String> {
        serde_json::to_writer(&mut self.w, v).map_err(|e| e.to_string())?;
        self.w.write_all(b"\n").map_err(|e| e.to_string())?;
        self.n += 1;
        Ok(())
    }
    fn done(mut self) -> Result<u64, String> {
        self.w.flush().map_err(|e| e.to_string())?;
        Ok(self.n)
    }
}

/// Every OAI-PMH page in `dir`, in name order: later records replace earlier ones.
pub fn oai_records(dir: &Path) -> Result<HashMap<String, Raw>, String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "xml")).collect();
    files.sort();
    let mut out = HashMap::new();
    for f in files {
        let xml = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        for r in raw::oai_page(&xml).0 {
            out.insert(r.id.clone(), r);
        }
    }
    Ok(out)
}

/// The papers of the snapshot, then those of OAI-PMH it does not have; a
/// paper OAI-PMH has too is read from there (it is newer).
fn papers(o: &Opts, oai: &HashMap<String, Raw>, full: bool, mut each: impl FnMut(Vec<Raw>) -> Result<(), String>) -> Result<(), String> {
    let keep = |r: &Raw| o.only.is_empty() || r.cats.first().is_some_and(|c| o.only.iter().any(|p| c.starts_with(p.as_str())));
    let mut seen_oai = std::collections::HashSet::new();
    raw::snapshot(&o.snapshot, full, |batch| {
        let v: Vec<Raw> = batch
            .into_iter()
            .map(|r| match oai.get(&r.id) {
                Some(n) => {
                    seen_oai.insert(r.id.clone());
                    n.clone()
                }
                None => r,
            })
            .filter(|r| keep(r))
            .collect();
        each(v)
    })?;
    let mut rest: Vec<Raw> = oai.values().filter(|r| !seen_oai.contains(&r.id) && keep(r)).cloned().collect();
    rest.sort_by(|a, b| a.id.cmp(&b.id));
    for chunk in rest.chunks(16_384) {
        each(chunk.to_vec())?;
    }
    Ok(())
}

/// Who is who: each key of a name to the key of the person it is.
struct People {
    ix: HashMap<String, u32>,
    keys: Vec<String>,
    names: Vec<String>,
    count: Vec<u32>,
    /// The person each key is (itself, or the one its initials name).
    canon: Vec<u32>,
}

impl People {
    fn new() -> People {
        People { ix: HashMap::new(), keys: Vec::new(), names: Vec::new(), count: Vec::new(), canon: Vec::new() }
    }
    fn add(&mut self, key: &str, name: &str) -> u32 {
        if let Some(&i) = self.ix.get(key) {
            self.count[i as usize] += 1;
            return i;
        }
        let i = self.keys.len() as u32;
        self.ix.insert(key.to_string(), i);
        self.keys.push(key.to_string());
        self.names.push(name.to_string());
        self.count.push(1);
        self.canon.push(i);
        i
    }
    /// A key whose forenames are all initials (`y lecun`, `j r smith`)
    /// joins the one key of the same last name whose forenames begin with
    /// those letters, when there is exactly one.
    fn merge_initials(&mut self) -> usize {
        let mut by_last: HashMap<&str, Vec<u32>> = HashMap::new();
        for (i, k) in self.keys.iter().enumerate() {
            if let Some((_, last)) = k.rsplit_once(' ') {
                by_last.entry(last).or_default().push(i as u32);
            }
        }
        let mut merged = 0;
        let mut canon = self.canon.clone();
        for ids in by_last.values() {
            if ids.len() < 2 {
                continue;
            }
            for &i in ids {
                let k = &self.keys[i as usize];
                let Some((fore, _)) = k.rsplit_once(' ') else { continue };
                let inits: Vec<&str> = fore.split(' ').collect();
                if !inits.iter().all(|w| w.chars().count() == 1) {
                    continue;
                }
                let mut cand = None;
                let mut n = 0;
                for &j in ids {
                    if j == i {
                        continue;
                    }
                    let kj = &self.keys[j as usize];
                    let Some((fj, _)) = kj.rsplit_once(' ') else { continue };
                    let wj: Vec<&str> = fj.split(' ').collect();
                    if wj.iter().all(|w| w.chars().count() == 1) || wj.len() < inits.len() {
                        continue;
                    }
                    if inits.iter().zip(&wj).all(|(a, b)| b.starts_with(a)) {
                        cand = Some(j);
                        n += 1;
                    }
                }
                if n == 1 {
                    canon[i as usize] = cand.unwrap_or(i);
                    merged += 1;
                }
            }
        }
        self.canon = canon;
        merged
    }
    fn of(&self, key: &str) -> Option<u32> {
        self.ix.get(key).map(|&i| self.canon[i as usize])
    }
}

/// Counts of something by year (1990 on), with its cites.
#[derive(Default, Clone)]
struct Tally {
    n: u32,
    cites: u64,
    years: Vec<u32>,
    recent: u32,
    before: u32,
    cats: Vec<(u16, u32)>,
    other: Vec<(u16, u32)>,
}

const Y0: i32 = 1990;

impl Tally {
    fn add(&mut self, year: i32, created: i64, cites: u32, now: i64) {
        self.n += 1;
        self.cites += cites as u64;
        let y = (year - Y0).max(0) as usize;
        if self.years.len() <= y {
            self.years.resize(y + 1, 0);
        }
        self.years[y] += 1;
        if created >= now - 365 * 86_400 {
            self.recent += 1;
        } else if created >= now - 730 * 86_400 {
            self.before += 1;
        }
    }
    fn by_year(&self) -> Value {
        let mut m = serde_json::Map::new();
        for (i, n) in self.years.iter().enumerate() {
            if *n > 0 {
                m.insert((Y0 + i as i32).to_string(), json!(n));
            }
        }
        Value::Object(m)
    }
    /// The share of the last twelve months against the twelve before, as a
    /// ratio of shares of all papers (1: as before; 2: twice the share).
    fn growth(&self, all_recent: u32, all_before: u32) -> f64 {
        let a = (self.recent as f64 + 1.0) / (all_recent as f64 + 1.0);
        let b = (self.before as f64 + 1.0) / (all_before as f64 + 1.0);
        ((a / b) * 1000.0).round() / 1000.0
    }
}

fn bump(v: &mut Vec<(u16, u32)>, k: u16) {
    if let Some(e) = v.iter_mut().find(|e| e.0 == k) {
        e.1 += 1;
    } else {
        v.push((k, 1));
    }
}

fn top(v: &[(u16, u32)], n: usize) -> Vec<(u16, u32)> {
    let mut v = v.to_vec();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v.truncate(n);
    v
}

#[derive(Default)]
struct Author {
    papers: u32,
    cites: Vec<u32>,
    first: i64,
    last: i64,
    recent: u32,
    cats: Vec<(u16, u32)>,
    techs: Vec<(u16, u32)>,
    first_author: u32,
}

fn h_index(c: &mut [u32]) -> u32 {
    c.sort_unstable_by(|a, b| b.cmp(a));
    c.iter().enumerate().take_while(|(i, &x)| x as usize > *i).count() as u32
}

/// Words that do not start or end a phrase worth completing.
const STOP: &[&str] = &["a", "an", "the", "of", "for", "and", "or", "in", "on", "to", "with", "via", "by", "from", "at", "as", "is", "are", "its", "their", "into", "using", "based", "towards", "toward", "new", "novel", "study", "studies", "approach", "analysis", "method", "methods", "some", "two", "one", "under", "over", "between", "through", "without", "within", "beyond", "case", "note", "remarks", "do", "does", "can", "we", "it", "this", "that", "be", "what", "how", "why", "when", "not", "all", "more", "less", "than", "i", "ii", "iii", "part"];

fn hash(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3))
}

fn grams(title: &str, mut f: impl FnMut(&str)) {
    let w = words(&concepts::unlatex(title));
    let w: Vec<&str> = w.iter().map(String::as_str).filter(|x| !x.starts_with('$') && x.len() < 40).collect();
    for n in 1..=3usize {
        for i in 0..w.len().saturating_sub(n - 1) {
            let g = &w[i..i + n];
            if STOP.contains(&g[0]) || STOP.contains(&g[n - 1]) || g.iter().any(|x| x.chars().all(|c| c.is_ascii_digit())) {
                continue;
            }
            if n == 1 && g[0].len() < 3 {
                continue;
            }
            f(&g.join(" "));
        }
    }
}

pub fn run(o: &Opts, mut say: impl FnMut(&str)) -> Result<(), String> {
    std::fs::create_dir_all(&o.out).map_err(|e| format!("{}: {e}", o.out.display()))?;
    let t0 = Instant::now();
    let oai = match &o.oai {
        Some(d) if d.exists() => oai_records(d)?,
        _ => HashMap::new(),
    };
    say(&format!("{} records from OAI-PMH", oai.len()));

    // Pass 1: ids, dates, DOIs and people.
    let mut ix: HashMap<String, u32> = HashMap::new();
    let mut ids: Vec<String> = Vec::new();
    let mut created: Vec<i64> = Vec::new();
    let mut doi_ix: HashMap<String, u32> = HashMap::new();
    let mut people = People::new();
    papers(o, &oai, false, |batch| {
        for r in batch {
            if ix.contains_key(&r.id) {
                continue;
            }
            let i = ids.len() as u32;
            ix.insert(r.id.clone(), i);
            created.push(paper::created(&r));
            if !r.doi.is_empty() {
                for d in r.doi.split_whitespace() {
                    doi_ix.insert(d.trim().to_lowercase(), i);
                }
            }
            for p in paper::people(&r) {
                people.add(&p.key, &p.name);
            }
            ids.push(r.id);
        }
        Ok(())
    })?;
    let merged = people.merge_initials();
    say(&format!("pass 1: {} papers, {} names ({} written with initials joined to a person), {} DOIs, {:.0} s", ids.len(), people.keys.len(), merged, doi_ix.len(), t0.elapsed().as_secs_f64()));

    // Crossref's citations: counts, and the citing works.
    let mut cites: Vec<u32> = vec![0; ids.len()];
    // (citing work, cited paper); the citing work as an index of `citing`.
    let mut pairs: Vec<(u32, u32)> = Vec::new();
    let mut citing: Vec<String> = Vec::new();
    let mut citing_ix: HashMap<String, u32> = HashMap::new();
    if let Some(cp) = &o.cites {
        raw::read_parquet(cp, &["arxiv_id", "citation_count", "cited_by"], |b| {
            let aid = Strs::of(raw::col(b, "arxiv_id"));
            let cnt = raw::col(b, "citation_count").and_then(|a| a.as_any().downcast_ref::<arrow_array::Int64Array>().cloned());
            let by = raw::col(b, "cited_by");
            for i in 0..b.num_rows() {
                let id = aid.get(i).trim();
                let Some(&p) = ix.get(id) else { continue };
                let n = cnt.as_ref().map(|c| c.value(i)).unwrap_or(0).max(0) as u32;
                cites[p as usize] = n;
                if let Some(l) = by.and_then(|a| raw::list_at(a.as_ref(), i)) {
                    if let Some(st) = l.as_any().downcast_ref::<arrow_array::StructArray>() {
                        if let Some(d) = st.column_by_name("doi") {
                            let s = Strs::of_dyn(d.as_ref());
                            for k in 0..s.len() {
                                let doi = s.get(k).trim().to_lowercase();
                                if doi.is_empty() {
                                    continue;
                                }
                                let c = match citing_ix.get(&doi) {
                                    Some(&c) => c,
                                    None => {
                                        let c = citing.len() as u32;
                                        citing_ix.insert(doi.clone(), c);
                                        citing.push(doi);
                                        c
                                    }
                                };
                                pairs.push((c, p));
                            }
                        }
                    }
                }
            }
            Ok(())
        })?;
        say(&format!("Crossref: {} citations of {} papers by {} works, {:.0} s", pairs.len(), cites.iter().filter(|&&c| c > 0).count(), citing.len(), t0.elapsed().as_secs_f64()));
    }
    drop(citing_ix);

    // The graph between arXiv papers: a citing work that is an arXiv
    // paper's published version is that paper.
    let mut edges: Vec<(u32, u32)> = pairs.iter().filter_map(|&(c, p)| doi_ix.get(&citing[c as usize]).map(|&s| (s, p))).filter(|(s, p)| s != p).collect();
    edges.sort_unstable();
    edges.dedup();
    let mut refs = vec![0u32; ids.len()];
    let mut cited_in = vec![0u32; ids.len()];
    for &(s, d) in &edges {
        refs[s as usize] += 1;
        cited_in[d as usize] += 1;
    }
    let mut out = Out::new(&o.out, "ax_cites.jsonl")?;
    for &(s, d) in &edges {
        out.row(&json!({"id": format!("{}>{}", ids[s as usize], ids[d as usize]), "src": ids[s as usize], "dst": ids[d as usize]}))?;
    }
    say(&format!("ax_cites: {} citations between arXiv papers", out.done()?));

    // What was cited together, for the recommender: each citing work's
    // arXiv papers, oldest first, as one reader's events.
    pairs.sort_unstable_by_key(|&(c, p)| (c, created[p as usize], p));
    let mut out = Out::new(&o.out, "ax_events.jsonl")?;
    let mut i = 0;
    while i < pairs.len() {
        let c = pairs[i].0;
        let mut j = i;
        while j < pairs.len() && pairs[j].0 == c {
            j += 1;
        }
        let n = j - i;
        if (2..=300).contains(&n) {
            let doi = &citing[c as usize];
            let usr = match doi_ix.get(doi) {
                Some(&s) => format!("arxiv:{}", ids[s as usize]),
                None => format!("doi:{doi}"),
            };
            for &(_, p) in &pairs[i..j] {
                let pid = &ids[p as usize];
                out.row(&json!({"id": format!("{usr}|{pid}"), "usr": usr, "item": pid, "ts": paper::iso(created[p as usize]), "kind": "cite"}))?;
            }
        }
        i = j;
    }
    say(&format!("ax_events: {} citations as events of {} citing works", out.done()?, "the"));
    drop(pairs);
    drop(citing);

    // Topics, if tools/topics.py made them.
    let mut topic_of: HashMap<String, i64> = HashMap::new();
    if let Some(tp) = &o.topics {
        let text = std::fs::read_to_string(tp).map_err(|e| format!("{}: {e}", tp.display()))?;
        for l in text.lines() {
            if let Some((id, t)) = l.split_once('\t') {
                if let Ok(t) = t.trim().parse() {
                    topic_of.insert(id.to_string(), t);
                }
            }
        }
        say(&format!("topics of {} papers", topic_of.len()));
    }

    // Pass 2: the rows.
    let dict = concepts::dict();
    let mut cat_ix: HashMap<String, u16> = HashMap::new();
    let mut cat_names: Vec<String> = Vec::new();
    let cat_of = |c: &str, cat_ix: &mut HashMap<String, u16>, cat_names: &mut Vec<String>| -> u16 {
        if let Some(&i) = cat_ix.get(c) {
            return i;
        }
        let i = cat_names.len() as u16;
        cat_ix.insert(c.to_string(), i);
        cat_names.push(c.to_string());
        i
    };
    let concept_ix: HashMap<&str, u16> = dict.concepts.iter().enumerate().map(|(i, c)| (c.slug.as_str(), i as u16)).collect();
    let mut authors: HashMap<u32, Author> = HashMap::new();
    let mut co: HashMap<u64, u32> = HashMap::new();
    let mut concept_t: Vec<Tally> = vec![Tally::default(); dict.concepts.len()];
    let mut cat_t: HashMap<u16, Tally> = HashMap::new();
    let mut venue_t: HashMap<String, Tally> = HashMap::new();
    let mut all = Tally::default();
    let mut gram: HashMap<u64, u32> = HashMap::new();
    let mut titles: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut out = Out::new(&o.out, "ax_papers.jsonl")?;
    let now = o.now;
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
    papers(o, &oai, true, |batch| {
        let batch: Vec<Raw> = batch.into_iter().filter(|r| seen.insert(r.id.clone())).collect();
        let per = batch.len().div_ceil(threads).max(1);
        let built: Vec<Built> = std::thread::scope(|s| {
            let hs: Vec<_> = batch
                .chunks(per)
                .map(|ch| {
                    let (cites, ix, topic_of, refs, cited_in) = (&cites, &ix, &topic_of, &refs, &cited_in);
                    s.spawn(move || {
                        ch.iter()
                            .map(|r| {
                                let i = ix.get(&r.id).copied();
                                let c = i.map(|i| cites[i as usize]).unwrap_or(0);
                                let mut b = paper::build(r, c, topic_of.get(&r.id).copied());
                                if let Some(i) = i {
                                    b.row["refs"] = json!(refs[i as usize]);
                                    b.row["cited_in"] = json!(cited_in[i as usize]);
                                }
                                b
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            hs.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
        });
        for mut b in built {
            let id = b.row["id"].as_str().unwrap_or("").to_string();
            let c = b.row["cites"].as_u64().unwrap_or(0) as u32;
            let year = b.row["year"].as_i64().unwrap_or(1991) as i32;
            // The people as the merged keys.
            let mut keys: Vec<u32> = Vec::new();
            for p in &b.people {
                if let Some(k) = people.of(&p.key) {
                    if !keys.contains(&k) {
                        keys.push(k);
                    }
                }
            }
            let key_names: Vec<String> = keys.iter().take(60).map(|&k| people.keys[k as usize].clone()).collect();
            b.row["people"] = json!(key_names);
            b.row["first_author"] = json!(key_names.first().cloned().unwrap_or_default());
            out.row(&b.row)?;
            titles.push(b.row["title"].as_str().unwrap_or("").to_string());
            grams(b.row["title"].as_str().unwrap_or(""), |g| *gram.entry(hash(g)).or_insert(0) += 1);
            all.add(year, b.created, c, now);
            let cats: Vec<u16> = b.cats.iter().map(|c| cat_of(c, &mut cat_ix, &mut cat_names)).collect();
            for &ci in &cats {
                cat_t.entry(ci).or_default().add(year, b.created, c, now);
            }
            let tech_ix: Vec<u16> = b.techniques.iter().chain(b.domains.iter()).filter_map(|s| concept_ix.get(s.as_str()).copied()).collect();
            for &t in &tech_ix {
                let tl = &mut concept_t[t as usize];
                tl.add(year, b.created, c, now);
                if let Some(&pc) = cats.first() {
                    bump(&mut tl.cats, pc);
                }
                for &u in &tech_ix {
                    if u != t {
                        bump(&mut tl.other, u);
                    }
                }
            }
            if let Some(v) = b.row["venue"].as_str().filter(|v| !v.is_empty()) {
                venue_t.entry(v.to_string()).or_default().add(year, b.created, c, now);
            }
            for (pos, &k) in keys.iter().enumerate() {
                let a = authors.entry(k).or_default();
                a.papers += 1;
                a.cites.push(c);
                if a.first == 0 || b.created < a.first {
                    a.first = b.created;
                }
                a.last = a.last.max(b.created);
                if b.created >= now - 365 * 86_400 {
                    a.recent += 1;
                }
                if let Some(&pc) = cats.first() {
                    bump(&mut a.cats, pc);
                }
                for &t in tech_ix.iter().take(4) {
                    bump(&mut a.techs, t);
                }
                if pos == 0 {
                    a.first_author += 1;
                }
            }
            if (2..=12).contains(&keys.len()) {
                for x in 0..keys.len() {
                    for y in x + 1..keys.len() {
                        let (a, b) = if keys[x] < keys[y] { (keys[x], keys[y]) } else { (keys[y], keys[x]) };
                        *co.entry(((a as u64) << 32) | b as u64).or_insert(0) += 1;
                    }
                }
            }
            let _ = id;
        }
        if out.n % 200_000 < 16_384 {
            say(&format!("pass 2: {} papers, {:.0} s", out.n, t0.elapsed().as_secs_f64()));
        }
        Ok(())
    })?;
    let n_papers = out.done()?;
    say(&format!("ax_papers: {n_papers} papers, {:.0} s", t0.elapsed().as_secs_f64()));

    // Coauthors: pairs who wrote two papers or more together (the graph
    // walks need the strong ties; a person's every coauthor is a query).
    let mut ncoauthors: HashMap<u32, u32> = HashMap::new();
    let mut out = Out::new(&o.out, "ax_coauthors.jsonl")?;
    for (&k, &w) in &co {
        let (a, b) = ((k >> 32) as u32, (k & 0xffff_ffff) as u32);
        *ncoauthors.entry(a).or_insert(0) += 1;
        *ncoauthors.entry(b).or_insert(0) += 1;
        if w >= 2 {
            let (ka, kb) = (&people.keys[a as usize], &people.keys[b as usize]);
            out.row(&json!({"id": format!("{ka}>{kb}"), "src": ka, "dst": kb, "w": w}))?;
        }
    }
    say(&format!("ax_coauthors: {} pairs who wrote two papers or more together (of {})", out.done()?, co.len()));
    drop(co);

    let mut out = Out::new(&o.out, "ax_authors.jsonl")?;
    for (&k, a) in authors.iter_mut() {
        let h = h_index(&mut a.cites);
        let total: u64 = a.cites.iter().map(|&c| c as u64).sum();
        out.row(&json!({
            "id": people.keys[k as usize],
            "name": people.names[k as usize],
            "papers": a.papers,
            "first_author": a.first_author,
            "cites": total,
            "h": h,
            "first": paper::iso(a.first),
            "last": paper::iso(a.last),
            "recent": a.recent,
            "cats": top(&a.cats, 3).iter().map(|(c, _)| cat_names[*c as usize].clone()).collect::<Vec<_>>(),
            "techniques": top(&a.techs, 6).iter().map(|(t, _)| dict.concepts[*t as usize].slug.clone()).collect::<Vec<_>>(),
            "coauthors": ncoauthors.get(&k).copied().unwrap_or(0),
        }))?;
    }
    say(&format!("ax_authors: {}", out.done()?));

    let mut out = Out::new(&o.out, "ax_concepts.jsonl")?;
    for (i, c) in dict.concepts.iter().enumerate() {
        let t = &concept_t[i];
        out.row(&json!({
            "id": c.slug, "kind": c.kind, "name": c.name, "phrases": c.phrases.join("; "), "acronyms": c.acronyms.join(" "),
            "papers": t.n, "cites": t.cites, "recent": t.recent, "growth": t.growth(all.recent, all.before),
            "first": t.years.iter().position(|&n| n > 0).map(|y| Y0 + y as i32),
            "by_year": t.by_year(),
            "cats": top(&t.cats, 5).iter().map(|(c, _)| cat_names[*c as usize].clone()).collect::<Vec<_>>(),
            "related": top(&t.other, 10).iter().map(|(u, _)| dict.concepts[*u as usize].slug.clone()).collect::<Vec<_>>(),
        }))?;
    }
    out.done()?;
    let mut out = Out::new(&o.out, "ax_cats.jsonl")?;
    for (&ci, t) in &cat_t {
        let code = &cat_names[ci as usize];
        let g = concepts::group_of(code);
        out.row(&json!({
            "id": code, "name": concepts::category_name(code), "area": g,
            "area_name": concepts::groups().iter().find(|(a, _)| *a == g).map(|(_, b)| b.clone()).unwrap_or_default(),
            "papers": t.n, "cites": t.cites, "recent": t.recent, "growth": t.growth(all.recent, all.before), "by_year": t.by_year(),
        }))?;
    }
    out.done()?;
    let mut out = Out::new(&o.out, "ax_venues.jsonl")?;
    for (v, t) in &venue_t {
        if t.n < 5 {
            continue;
        }
        out.row(&json!({"id": v, "papers": t.n, "cites": t.cites, "recent": t.recent, "by_year": t.by_year()}))?;
    }
    out.done()?;

    // Autocomplete: phrases of titles used 25 times or more, the
    // techniques, domains, categories and people.
    let mut out = Out::new(&o.out, "ax_complete.jsonl")?;
    let mut done = std::collections::HashSet::new();
    for t in &titles {
        grams(t, |g| {
            let h = hash(g);
            if let Some(&n) = gram.get(&h) {
                if n >= 25 && done.insert(h) {
                    let _ = out.row(&json!({"id": format!("p:{g}"), "kind": "phrase", "text": g, "n": n, "weight": (n as f64).ln()}));
                }
            }
        });
    }
    drop(titles);
    for (i, c) in dict.concepts.iter().enumerate() {
        let n = concept_t[i].n;
        let row = json!({"id": format!("p:{}", c.name.to_lowercase()), "kind": c.kind, "text": c.name.to_lowercase(), "n": n, "weight": (n as f64 + 1.0).ln() + 3.0, "ref": c.slug});
        out.row(&row)?;
        out.row(&json!({"id": format!("t:{}", c.name.to_lowercase()), "kind": c.kind, "text": c.name, "n": n, "weight": (n as f64 + 1.0).ln(), "ref": c.slug}))?;
    }
    for (&ci, t) in &cat_t {
        let code = &cat_names[ci as usize];
        let name = concepts::category_name(code);
        for k in [code.to_lowercase(), name.to_lowercase()] {
            out.row(&json!({"id": format!("c:{k}"), "kind": "category", "text": format!("{name} ({code})"), "n": t.n, "weight": (t.n as f64 + 1.0).ln(), "ref": code}))?;
        }
    }
    for (&k, a) in &authors {
        if a.papers < 3 {
            continue;
        }
        let key = &people.keys[k as usize];
        let w = (a.papers as f64).ln() + 0.5 * (1.0 + a.cites.iter().map(|&c| c as f64).sum::<f64>()).ln();
        out.row(&json!({"id": format!("a:{key}"), "kind": "author", "text": people.names[k as usize], "n": a.papers, "weight": w, "ref": key}))?;
        // And by last name ("lecun"), so a surname alone completes.
        if let Some((_, last)) = key.rsplit_once(' ') {
            out.row(&json!({"id": format!("a:{last} {key}"), "kind": "author", "text": people.names[k as usize], "n": a.papers, "weight": w, "ref": key}))?;
        }
    }
    say(&format!("ax_complete: {} rows; done in {:.0} s", out.done()?, t0.elapsed().as_secs_f64()));
    Ok(())
}
