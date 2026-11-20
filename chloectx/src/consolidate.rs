//! Keeps the written-down knowledge small as a project grows: notes about
//! the same thing are merged, now and then, into fewer notes that keep
//! every detail still valid, and what changed wins over what it replaced.
//!
//! Distillation adds a note or two per turn and replaces only the notes it
//! is shown, so over weeks the same requirement appears in four wordings
//! and an old value lingers next to its replacement. Consolidation groups
//! the live notes by meaning (vector similarity, or a shared entity and a
//! looser similarity), and a model rewrites each group as edits: notes kept
//! as they are, and new notes that replace a listed set. Replaced notes are
//! kept, marked superseded, and linked from their replacement, so the
//! history stays queryable. A merged note keeps the code anchors of the
//! notes it replaces, so it is not taken for current when the code changed.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::anchor::Anchor;
use crate::distill::{strings, DistillConfig};
use crate::llm::{self, clip};
use crate::recall::{fmt_time, KNOWLEDGE};
use crate::record::{Draft, Kind, Record};
use crate::store::{Filter, Store};
use crate::Result;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ConsolidateConfig {
    pub enabled: bool,
    /// Run after this many distilled turns since the last run…
    pub every_turns: usize,
    /// …when at least this many notes are live.
    pub min_live: usize,
    /// Notes at this cosine similarity or more are grouped…
    pub cos: f64,
    /// …and so are notes about a shared entity at this similarity or more.
    pub entity_cos: f64,
    pub max_group: usize,
    /// Groups rewritten per run.
    pub max_groups: usize,
}

impl Default for ConsolidateConfig {
    fn default() -> Self {
        ConsolidateConfig { enabled: true, every_turns: 8, min_live: 30, cos: 0.80, entity_cos: 0.55, max_group: 10, max_groups: 8 }
    }
}

impl ConsolidateConfig {
    /// Enabled, and not switched off with `CHLOECTX_CONSOLIDATE=0`.
    pub fn active(&self) -> bool {
        self.enabled && std::env::var("CHLOECTX_CONSOLIDATE").map_or(true, |v| v != "0")
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Report {
    pub live_before: usize,
    pub groups: usize,
    pub replaced: usize,
    pub written: usize,
    /// Merges the model wrote that the check refused, with why.
    pub rejected: Vec<String>,
    pub cost_usd: f64,
    pub usage: llm::Usage,
    pub errors: Vec<String>,
}

const SYSTEM: &str = "You maintain the long-term notes of a software project. Output JSON lines only.";

const PROMPT: &str = r#"These notes from a software project's memory are about related things. They are listed oldest first, with their ids and when each was written:

{notes}

Rewrite them as the smallest set of notes that keeps every detail, number, name, format and reason that still holds:
- when a later note changes, reverses or replaces an earlier one, keep only the current version, and say briefly what it replaced if that helps (e.g. "30 days, changed from 14");
- merge notes that say the same thing or parts of one thing; keep notes on different subjects separate: notes about different versions of a thing (the old and the new parser) or about different code are different subjects;
- never turn around what a note says, and never write a number, name or path no note has;
- never drop a rule, a requirement for planned work, a constraint or a reason;
- each note is one or two self-contained sentences that name their subject;
- a note that merges with nothing stays as it is: keep it rather than rewording it.

Output one JSON object per line:
{"keep": "<id>"} for a note that stays exactly as it is;
{"kind": "decision" | "rule" | "requirement" | "fact", "text": "<note>", "entities": ["<entity>", ...], "when": ["<situation in which it matters>", ...], "replaces": ["<id>", ...]} for a new note that replaces the listed ones.
Every id above must appear exactly once: in a keep line or in one replaces list."#;

fn label(r: &Record) -> &'static str {
    match r.kind {
        Kind::Todo => "requirement",
        Kind::Preference => "rule",
        Kind::Decision => "decision",
        _ => "fact",
    }
}

fn cosine(a: &[f32], b: &[f32]) -> f64 {
    let (mut d, mut na, mut nb) = (0f64, 0f64, 0f64);
    for (x, y) in a.iter().zip(b) {
        d += (*x as f64) * (*y as f64);
        na += (*x as f64) * (*x as f64);
        nb += (*y as f64) * (*y as f64);
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        d / (na.sqrt() * nb.sqrt())
    }
}

/// The live notes: not superseded or done, requirements still planned.
pub fn live_notes(store: &Store, limit: usize) -> Result<Vec<Record>> {
    Ok(store
        .latest(&KNOWLEDGE, limit, &Filter::default())?
        .into_iter()
        .filter(|r| !crate::record::closed_tag(&r.tag) && r.tag != "done" && r.tag != "cue" && (r.kind != Kind::Todo || r.tag == "planned"))
        .collect())
}

/// Groups of related notes (indices into `notes`), largest first, each of
/// at least two and at most `max_group`.
pub fn groups(store: &Store, notes: &[Record], cfg: &ConsolidateConfig) -> Vec<Vec<usize>> {
    let texts: Vec<String> = notes.iter().map(|r| r.key_text()).collect();
    let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
    let Ok(vecs) = store.embedder.embed(&refs) else { return Vec::new() };
    let ents: Vec<HashSet<&str>> = notes.iter().map(|r| r.linked("about").collect()).collect();
    let n = notes.len();
    let mut edges: Vec<(f64, usize, usize)> = Vec::new();
    for i in 0..n {
        for j in i + 1..n {
            let c = cosine(&vecs[i], &vecs[j]);
            if c >= cfg.cos || (c >= cfg.entity_cos && !ents[i].is_disjoint(&ents[j])) {
                edges.push((c, i, j));
            }
        }
    }
    // Strongest links first, never growing a group past max_group.
    edges.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut parent: Vec<usize> = (0..n).collect();
    let mut size = vec![1usize; n];
    fn find(p: &mut [usize], x: usize) -> usize {
        let mut x = x;
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for (_, i, j) in edges {
        let (a, b) = (find(&mut parent, i), find(&mut parent, j));
        if a != b && size[a] + size[b] <= cfg.max_group {
            parent[b] = a;
            size[a] += size[b];
        }
    }
    let mut by: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        by.entry(r).or_default().push(i);
    }
    let mut out: Vec<Vec<usize>> = by.into_values().filter(|g| g.len() >= 2).collect();
    for g in &mut out {
        g.sort_by_key(|&i| notes[i].seq);
    }
    out.sort_by(|a, b| b.len().cmp(&a.len()).then(a[0].cmp(&b[0])));
    out
}

/// One group's edits: (new drafts, the ids each replaces), as the model wrote them.
pub fn parse(text: &str, ids: &[String]) -> Vec<(Draft, Vec<String>)> {
    let known: HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
    let mut used: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim().trim_end_matches(',');
        if !line.starts_with('{') {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        if v.get("keep").is_some() {
            continue;
        }
        let replaces: Vec<String> = match v.get("replaces") {
            Some(serde_json::Value::String(s)) => vec![s.clone()],
            Some(serde_json::Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
            _ => Vec::new(),
        }
        .into_iter()
        .map(|s| s.trim().trim_matches(|c| c == '[' || c == ']').to_string())
        .filter(|s| known.contains(s.as_str()) && !used.contains(s))
        .collect();
        let text = v.get("text").and_then(|t| t.as_str()).unwrap_or("").trim();
        if replaces.is_empty() || text.len() < 12 {
            continue;
        }
        let (k, tag) = match v.get("kind").and_then(|k| k.as_str()).unwrap_or("fact") {
            "decision" => (Kind::Decision, "distilled"),
            "rule" => (Kind::Preference, "distilled"),
            "requirement" => (Kind::Todo, "planned"),
            _ => (Kind::Fact, "distilled"),
        };
        used.extend(replaces.iter().cloned());
        let d = Draft::new(k, clip(&crate::redact::redact(text), 900))
            .tag(tag)
            .files(crate::capture::paths_in(text))
            .entities(strings(&v, "entities", 6, 60))
            .when(strings(&v, "when", 4, 160));
        out.push((d, replaces));
    }
    out
}

/// Why a merged note may not replace its sources, or None when it keeps
/// what they say (JONAH-401). The model rewrites notes it was shown, and
/// nothing it writes is taken as true because it wrote it: the merged note
/// must say the same as the newest source and the newest decision of the
/// user among them (no flipped word such as accepts/rejects, no negation
/// added or dropped), keep every number, name and path of the newest
/// source, make up no new ones, and the sources must be about the same
/// thing: not "the old parser" with "the new parser", nor notes anchored
/// to disjoint sets of files. A rejected merge leaves the notes as they
/// are.
pub fn contradiction(text: &str, olds: &[&Record]) -> Option<String> {
    let Some(newest) = olds.iter().max_by_key(|r| r.seq) else { return None };
    let merged = text.to_lowercase();
    let all: String = olds.iter().map(|r| r.text.to_lowercase()).collect::<Vec<_>>().join("\n");
    // Different versions of one thing, or different code.
    let versions = |t: &str, set: &[&str]| -> HashSet<String> {
        let words: Vec<&str> = t.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
        words.windows(2).filter(|w| set.contains(&w[0])).map(|w| w[1].to_string()).collect()
    };
    const OLD: [&str; 5] = ["old", "legacy", "previous", "former", "v1"];
    const NEW: [&str; 5] = ["new", "rewritten", "next", "refactored", "v2"];
    // One note about the old thing, another about the new one (a note that
    // names both may be merged with what agrees with it).
    let tagged: Vec<(HashSet<String>, HashSet<String>)> = olds.iter().map(|r| r.text.to_lowercase()).map(|t| (versions(&t, &OLD), versions(&t, &NEW))).collect();
    for (i, (old_i, _)) in tagged.iter().enumerate() {
        for (j, (_, new_j)) in tagged.iter().enumerate() {
            if let Some(w) = old_i.intersection(new_j).find(|w| i != j && !tagged[i].1.contains(*w) && !tagged[j].0.contains(*w)) {
                return Some(format!("the notes are about the old and the new {w}, which are different things"));
            }
        }
    }
    let files: Vec<HashSet<String>> = olds.iter().filter_map(|r| Anchor::parse(&r.ctx)).filter(|a| !a.files.is_empty()).map(|a| a.files.into_iter().map(|(p, _)| p).collect()).collect();
    if files.iter().enumerate().any(|(i, a)| files[i + 1..].iter().any(|b| a.is_disjoint(b))) {
        return Some("the notes are anchored to different files".into());
    }
    // What the merged note says against the newest note and the user's newest.
    let user_newest = olds.iter().filter(|r| !matches!(r.source(), Some(s) if s != crate::record::Source::User)).max_by_key(|r| r.seq);
    let any_negated = olds.iter().any(|r| negated(&r.text.to_lowercase()));
    for src in std::iter::once(newest).chain(user_newest.filter(|u| u.id != newest.id)) {
        if let Some(why) = flips(&src.text.to_lowercase(), &merged, any_negated) {
            return Some(format!("it {why} [{}]", src.id));
        }
    }
    let invented: Vec<String> = specifics(&merged).into_iter().filter(|t| !all.contains(t.as_str())).collect();
    if let Some(t) = invented.first() {
        return Some(format!("it names {t}, which no source does"));
    }
    let merged_specifics = specifics(&merged);
    if let Some(t) = specifics(&newest.text.to_lowercase()).into_iter().find(|t| !merged_specifics.contains(t)) {
        return Some(format!("it drops {t} from the newest note [{}]", newest.id));
    }
    None
}

/// Word families that say opposite things.
const OPPOSITES: [(&[&str], &[&str]); 9] = [
    (&["accept"], &["reject", "refus"]),
    (&["allow", "permit"], &["forbid", "disallow", "deny", "denies", "denied", "prohibit", "block"]),
    (&["enable"], &["disable"]),
    (&["requir", "mandatory"], &["optional"]),
    (&["includ"], &["exclud", "omit"]),
    (&["always"], &["never"]),
    (&["succe"], &["fail"]),
    (&["increas", "raise"], &["decreas", "lower", "reduc"]),
    (&["support"], &["unsupport", "drop"]),
];

fn words_of(t: &str) -> Vec<&str> {
    t.split(|c: char| !(c.is_alphanumeric() || c == '\'')).filter(|w| !w.is_empty()).collect()
}

fn negated(t: &str) -> bool {
    words_of(t).iter().any(|w| matches!(*w, "not" | "never" | "no" | "cannot" | "without") || w.ends_with("n't"))
}

/// How `merged` reverses `src` (both lowercased), if it does;
/// `any_negated`: whether some source says "not", which the merge may then
/// carry over from it.
fn flips(src: &str, merged: &str, any_negated: bool) -> Option<&'static str> {
    let has = |words: &[&str], fams: &[&str]| words.iter().any(|w| fams.iter().any(|f| w.starts_with(f)));
    let (sw, mw) = (words_of(src), words_of(merged));
    for (pos, neg) in OPPOSITES {
        let (sp, sn, mp, mn) = (has(&sw, pos), has(&sw, neg), has(&mw, pos), has(&mw, neg));
        if (sp && !sn && mn && !mp) || (sn && !sp && mp && !mn) {
            return Some("turns what the note says around");
        }
    }
    // A negation added or dropped from a note about the same thing.
    let (a, b): (HashSet<&str>, HashSet<&str>) = (sw.iter().copied().collect(), mw.iter().copied().collect());
    let jaccard = a.intersection(&b).count() as f64 / a.union(&b).count().max(1) as f64;
    let (ns, nm) = (negated(src), negated(merged));
    if ((ns && !nm) || (nm && !any_negated)) && jaccard >= 0.5 {
        return Some("adds or drops a negation");
    }
    None
}

/// The numbers, dates, paths and identifiers of a text (lowercased): what a
/// rewrite must not lose or make up.
fn specifics(t: &str) -> HashSet<String> {
    t.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !(c.is_alphanumeric() || c == '/' || c == '_')).trim_end_matches(['.', ',']))
        .filter(|w| {
            let dotted = w.contains('.') && w.chars().any(|c| c.is_alphabetic()) && w.split('.').any(|p| p.len() > 1);
            w.chars().any(|c| c.is_ascii_digit()) || (w.len() >= 2 && (w.contains(['/', '_']) || dotted))
        })
        .map(str::to_string)
        .collect()
}

/// Merges groups of related live notes. Writes the new notes and marks the
/// ones they replace superseded.
pub fn run(store: &Store, cfg: &ConsolidateConfig, cli: &DistillConfig, now: i64) -> Result<Report> {
    let notes = live_notes(store, 2000)?;
    let mut rep = Report { live_before: notes.len(), ..Default::default() };
    let gs = groups(store, &notes, cfg);
    let model = cli.model();
    for g in gs.into_iter().take(cfg.max_groups) {
        rep.groups += 1;
        let members: Vec<&Record> = g.iter().map(|&i| &notes[i]).collect();
        let ids: Vec<String> = members.iter().map(|r| r.id.clone()).collect();
        let listed: Vec<String> = members.iter().map(|r| format!("[{}] {} ({}): {}", r.id, label(r), fmt_time(r.ts), r.text.split_whitespace().collect::<Vec<_>>().join(" "))).collect();
        let prompt = PROMPT.replace("{notes}", &listed.join("\n"));
        let reply = match llm::complete(&cli.cli(&model, cli.timeout_secs), SYSTEM, &prompt) {
            Ok(r) => r,
            Err(e) => {
                rep.errors.push(e.to_string());
                continue;
            }
        };
        rep.cost_usd += reply.cost_usd;
        rep.usage.add(reply.usage);
        for (draft, replaces) in parse(&reply.text, &ids) {
            // A note rewritten on its own only churns: keep the original.
            if replaces.len() < 2 {
                continue;
            }
            let olds: Vec<&&Record> = members.iter().filter(|r| replaces.contains(&r.id)).collect();
            // Checked against what it replaces before it replaces it.
            let sources: Vec<&Record> = olds.iter().map(|r| **r).collect();
            if let Some(why) = contradiction(&draft.text, &sources) {
                rep.rejected.push(format!("{}: {why}", replaces.join(", ")));
                continue;
            }
            // The newest replaced note's session, turn and time; its context
            // and the anchors of all of them. The merged note is dated by
            // what it says, not by when it was merged: a later merge lists
            // it with that date, and a model reading the merge time as the
            // day something happened writes it into the note (in a replay
            // or an import, months after the messages).
            let newest = olds.iter().max_by_key(|r| r.seq).unwrap();
            let said = olds.iter().map(|r| r.ts).max().unwrap_or(now);
            let anchors: Vec<Anchor> = olds.iter().filter_map(|r| Anchor::parse(&r.ctx)).collect();
            let mut ctx = format!("Merged from {} notes, {}", olds.len(), fmt_time(now));
            if !anchors.is_empty() {
                ctx += "\n";
                ctx += &Anchor::merge(&anchors).to_line();
            }
            let mut d = draft.ctx(ctx);
            d.importance = Some(olds.iter().map(|r| r.importance).fold(0.0, f64::max));
            // Who said it, when they all agree.
            let by: Vec<Option<crate::record::Source>> = olds.iter().map(|r| r.source()).collect();
            // When they differ, the merged note is not the user's: a sentence
            // that mixes the user's words with an agent's is shown as the
            // agent's (JONAH-401).
            let theirs = by.iter().flatten().find(|b| **b != crate::record::Source::User);
            if let Some(Some(b)) = by.first().filter(|b| by.iter().all(|x| x == *b)) {
                d = d.by(b);
            } else if let Some(b) = theirs {
                d = d.by(b);
            }
            for id in &replaces {
                d = d.link("supersedes", id.clone());
            }
            let stored = store.append(&newest.session, newest.turn, said, vec![d])?;
            // Each old note names the merged one as its successor.
            if let Some(new) = stored.iter().find(|r| r.kind.is_knowledge()) {
                for id in &replaces {
                    let _ = store.supersede(id, &new.id);
                }
            }
            rep.replaced += replaces.len();
            rep.written += 1;
        }
    }
    store.warm();
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::HashEmbedder;
    use crate::store::StoreOptions;
    use std::sync::Arc;

    #[test]
    fn edits_are_parsed() {
        let ids = vec!["a:1".to_string(), "a:2".to_string(), "a:3".to_string()];
        let text = "{\"keep\": \"a:3\"}\n{\"kind\": \"rule\", \"text\": \"Refunds are allowed within 30 days (changed from 14).\", \"replaces\": [\"a:1\", \"[a:2]\", \"zz:9\"], \"when\": [\"refund code\"]}\n{\"kind\": \"fact\", \"text\": \"A second note that reuses a:1.\", \"replaces\": [\"a:1\"]}";
        let p = parse(text, &ids);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].1, vec!["a:1".to_string(), "a:2".to_string()]);
        assert_eq!(p[0].0.kind, Kind::Preference);
        assert_eq!(p[0].0.when, vec!["refund code".to_string()]);
    }

    #[test]
    fn merged_notes_are_dated_by_what_they_replace() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("m.strata"), Arc::new(HashEmbedder { dim: 256 }), StoreOptions { sync: false, ..Default::default() }).unwrap();
        s.append("a", 1, 100, vec![Draft::new(Kind::Preference, "Refunds are allowed within 14 days of purchase").tag("distilled").entities(vec!["refund".into()])]).unwrap();
        s.append("a", 2, 200, vec![Draft::new(Kind::Preference, "Refunds are allowed within 30 days of purchase now").tag("distilled").entities(vec!["refund".into()])]).unwrap();
        let ids: Vec<String> = live_notes(&s, 10).unwrap().iter().map(|r| r.id.clone()).collect();
        let out = dir.path().join("out.json");
        let merged = serde_json::json!({"kind": "rule", "text": "Refunds are allowed within 30 days of purchase.", "replaces": ids});
        std::fs::write(&out, serde_json::json!({"is_error": false, "total_cost_usd": 0.0, "result": merged.to_string()}).to_string()).unwrap();
        let script = dir.path().join("fake-model.sh");
        std::fs::write(&script, format!("#!/bin/sh\ncat > /dev/null\ncat '{}'\n", out.display())).unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let cli = DistillConfig { command: script.display().to_string(), model: "m".into(), ..Default::default() };
        let rep = run(&s, &ConsolidateConfig { cos: 0.99, entity_cos: 0.2, ..Default::default() }, &cli, 10_000).unwrap();
        assert_eq!(rep.written, 1, "{:?}", rep.errors);
        let live = live_notes(&s, 10).unwrap();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].ts, 200);
    }

    #[test]
    fn related_notes_are_grouped() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("m.strata"), Arc::new(HashEmbedder { dim: 256 }), StoreOptions { sync: false, ..Default::default() }).unwrap();
        let notes = [
            ("Refunds are allowed within 14 days of purchase", "refund"),
            ("Refunds are allowed within 30 days of purchase now", "refund"),
            ("The export file uses semicolons as separators", "export"),
            ("Deploys happen on Tuesdays only", "deploy"),
        ];
        for (t, e) in notes {
            s.append("a", 1, 100, vec![Draft::new(Kind::Preference, t).tag("distilled").entities(vec![e.into()])]).unwrap();
        }
        let live = live_notes(&s, 100).unwrap();
        assert_eq!(live.len(), 4);
        let g = groups(&s, &live, &ConsolidateConfig { cos: 0.99, entity_cos: 0.2, ..Default::default() });
        assert_eq!(g.len(), 1, "{g:?}");
        let texts: Vec<&str> = g[0].iter().map(|&i| live[i].text.as_str()).collect();
        assert!(texts.iter().all(|t| t.starts_with("Refunds")), "{texts:?}");
    }

    fn scripted(dir: &std::path::Path, merged: serde_json::Value) -> DistillConfig {
        let out = dir.join("out.json");
        std::fs::write(&out, serde_json::json!({"is_error": false, "total_cost_usd": 0.0, "result": merged.to_string()}).to_string()).unwrap();
        let script = dir.join("fake-model.sh");
        std::fs::write(&script, format!("#!/bin/sh\ncat > /dev/null\ncat '{}'\n", out.display())).unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        DistillConfig { command: script.display().to_string(), model: "m".into(), ..Default::default() }
    }

    /// Two notes, the model's merge of them; the live notes afterwards.
    fn consolidate(notes: [(&str, &str); 2], merge_text: &str) -> (Report, Vec<Record>) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("m.strata"), Arc::new(HashEmbedder { dim: 256 }), StoreOptions { sync: false, ..Default::default() }).unwrap();
        for (i, (by, t)) in notes.iter().enumerate() {
            s.append("a", i as i64 + 1, 100 * (i as i64 + 1), vec![Draft::new(Kind::Fact, *t).tag("distilled").entities(vec!["parser".into()]).by(&crate::record::Source::parse(by))]).unwrap();
        }
        let ids: Vec<String> = live_notes(&s, 10).unwrap().iter().map(|r| r.id.clone()).collect();
        let cli = scripted(dir.path(), serde_json::json!({"kind": "fact", "text": merge_text, "replaces": ids}));
        let rep = run(&s, &ConsolidateConfig { cos: 0.99, entity_cos: 0.2, ..Default::default() }, &cli, 10_000).unwrap();
        let live = live_notes(&s, 10).unwrap();
        (rep, live)
    }

    /// JONAH-401: "the old parser rejects X" merged with "the new parser
    /// accepts it" came back as "the new parser rejects it".
    #[test]
    fn a_merge_that_contradicts_its_sources_is_rejected() {
        let notes = [("assistant", "The old parser rejects the date 2026-12-1 with an error."), ("assistant", "The new parser accepts it and reads it as December 1.")];
        let (rep, live) = consolidate(notes, "The new parser rejects the date 2026-12-1 with an error.");
        assert_eq!((rep.written, rep.replaced), (0, 0), "{:?}", rep.rejected);
        assert_eq!(rep.rejected.len(), 1);
        assert_eq!(live.len(), 2, "the true notes stay live");
        // Even a faithful-looking text is not merged across old and new.
        let (rep, live) = consolidate(notes, "The old parser rejects the date 2026-12-1 and the new parser accepts it.");
        assert_eq!(rep.written, 0, "{:?}", rep.rejected);
        assert_eq!(live.len(), 2);
    }

    #[test]
    fn a_merge_never_turns_around_a_decision_of_the_user() {
        let notes = [("user", "Uploads are allowed for every signed-in member."), ("assistant", "Uploads over 5 MB need a signed URL.")];
        let (rep, live) = consolidate(notes, "Uploads are forbidden for members; uploads over 5 MB need a signed URL.");
        assert_eq!(rep.written, 0, "{:?}", rep.rejected);
        assert_eq!(live.len(), 2);
        // Dropping a number of the newest note, and making one up, are refused too.
        let (rep, _) = consolidate(notes, "Uploads are allowed for every signed-in member; large ones need a signed URL.");
        assert_eq!(rep.written, 0, "{:?}", rep.rejected);
        let (rep, _) = consolidate(notes, "Uploads are allowed for every signed-in member; uploads over 9 MB need a signed URL (5 MB).");
        assert_eq!(rep.written, 0, "{:?}", rep.rejected);
    }

    #[test]
    fn a_faithful_merge_is_written_and_not_the_users_when_an_agent_is_in_it() {
        let notes = [("user", "Uploads are allowed for every signed-in member."), ("assistant", "Uploads over 5 MB need a signed URL.")];
        let (rep, live) = consolidate(notes, "Uploads are allowed for every signed-in member, and uploads over 5 MB need a signed URL.");
        assert_eq!(rep.written, 1, "{:?}", rep.rejected);
        assert_eq!(live.len(), 1);
        assert!(live[0].source().is_some_and(|s| s != crate::record::Source::User), "{:?}", live[0].source());
    }
}
