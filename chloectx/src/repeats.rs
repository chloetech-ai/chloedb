//! Instructions said again: an instruction the user gave for one task is a
//! note of that session only ([`TASK_TAG`], [`crate::distill::attribute`]):
//! no other session is told it, because nothing said it should hold beyond
//! the task. Said again, in a later turn or another session, it does: the
//! user should not have to say it a third time. The second time, the note
//! becomes a rule ([`REPEATED_TAG`]) every session is given, and the notes
//! it repeats are superseded by it.
//!
//! An instruction that says what a rule already says (one kept this way,
//! or one the user gave as a rule) means the rule did not hold. Was the
//! session shown it? Then the agent broke a rule it had; if not, the rule
//! was not there when it mattered. Either way the rule is written anew in
//! the newest words, with its situations (`When:`), files and entities and
//! the new ones, so it is the newest rule and recalled in this situation
//! from now on, and it counts the sessions it was said in, the times it was
//! said, the times it was said again once kept, and of those the times the
//! session had been shown it (`Sessions:`, `Said:`, `Said again:`, `Said
//! again after it was shown:`).
//!
//! Which instructions say the same is the distiller's to judge: it is shown
//! the user's earlier instructions most like the request ([`said_before`])
//! and marks an item that says one of them again (`repeats`). The user
//! rewords a correction more than words or vectors can follow (in a week
//! of real sessions, an instruction and its saying again scored a cosine
//! of 0.05 to 0.57 with potion-retrieval-32M, and different instructions up
//! to 0.73). Only the
//! user's own words count: an instruction another agent or Workbench gave
//! is that one's brief, not the user's correction.
//!
//! [`scan`] lists, without writing anything or calling a model
//! (`chloectx repeats`), the rules kept this way and the instructions said
//! again in nearly the same words ([`same`]).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use chloedb::query::Params;
use chloedb::Value;
use serde_json::{json, Value as Json};

use crate::record::{Draft, Kind, Record, Source, COLUMNS, TASK_TAG};
use crate::store::{row_record, Store};
use crate::Result;

/// The tag of a rule kept because the user said it again.
pub const REPEATED_TAG: &str = "repeated";

/// Two instructions are nearly the same words when this share of their
/// words (stemmed, stop words out) is common to both ([`scan`] only).
const JACCARD: f64 = 0.5;
/// Earlier instructions shown to the distiller, by each of BM25 and the
/// vectors.
const CANDIDATES: usize = 8;

/// The instruction a task note keeps, without the words that say whose it
/// is and for which task ("For lead's task only (the user said, not a
/// standing rule): "), or a repeated rule's without its count.
pub fn instruction(text: &str) -> &str {
    let t = text.split_once("not a standing rule): ").map(|x| x.1).unwrap_or(text);
    let t = match t.rfind(" (the user has said this ") {
        Some(i) if t.ends_with(" sessions)") || t.ends_with(" times)") => &t[..i],
        _ => t,
    };
    t.trim()
}

/// The text of a rule said in `sessions` sessions, `times` times in all.
pub fn rule_text(instruction: &str, sessions: usize, times: usize) -> String {
    let t = instruction.trim().trim_end_matches('.');
    if sessions >= 2 {
        format!("{t} (the user has said this in {sessions} sessions)")
    } else if times >= 2 {
        format!("{t} (the user has said this {times} times)")
    } else {
        format!("{t}.")
    }
}

fn count(r: &Record, line: &str) -> Option<usize> {
    r.ctx.lines().find_map(|l| l.strip_prefix(line)).and_then(|n| n.trim().parse().ok())
}

/// How many sessions a rule counts (its `Sessions:` line; a rule kept for
/// being said again without one counts two, any other rule one).
pub fn sessions_of(r: &Record) -> usize {
    count(r, "Sessions: ").unwrap_or(if r.tag == REPEATED_TAG { 2 } else { 1 })
}

/// How many times a rule was said in all (its `Said:` line, or as many
/// times as sessions).
pub fn said_of(r: &Record) -> usize {
    count(r, "Said: ").unwrap_or_else(|| sessions_of(r))
}

/// How many times a rule was said again once it was a rule, and how many
/// of those after the session had been shown it.
pub fn said_again_of(r: &Record) -> (usize, usize) {
    (count(r, "Said again: ").unwrap_or(0), count(r, "Said again after it was shown: ").unwrap_or(0))
}

/// `2026-11-15` for a Unix time (UTC).
fn day(ts: i64) -> String {
    crate::recall::fmt_time(ts).chars().take(10).collect()
}

fn terms(t: &str) -> BTreeSet<String> {
    chloedb::text::analyze(t).into_iter().collect()
}

/// Whether two instructions say the same in nearly the same words.
pub fn same(a: &str, b: &str) -> bool {
    same_terms(&terms(a), &terms(b))
}

fn same_terms(ta: &BTreeSet<String>, tb: &BTreeSet<String>) -> bool {
    let inter = ta.intersection(tb).count();
    let union = ta.len() + tb.len() - inter;
    union > 0 && inter >= 2 && inter as f64 / union as f64 >= JACCARD
}

/// Whether a record holds the user's own words (a record with no `By:`
/// line was written before sources were kept, and is the user's).
fn users(r: &Record) -> bool {
    matches!(r.source(), None | Some(Source::User))
}

/// Whether a record is an instruction the user gave: a note for one task,
/// or a rule.
fn users_instruction(r: &Record) -> bool {
    users(r) && ((r.kind == Kind::Note && r.tag == TASK_TAG) || (r.kind == Kind::Preference && r.tag != "superseded"))
}

/// Whether a draft is an instruction the user gave: for one task, or as a
/// rule.
fn users_draft(d: &Draft) -> bool {
    matches!(d.by.as_deref().map(Source::parse), None | Some(Source::User)) && ((d.kind == Kind::Note && d.tag == TASK_TAG) || d.kind == Kind::Preference)
}

/// The user's earlier instructions most like `request`, in this session or
/// any other, for the distiller to judge which an item says again: notes
/// for one task and rules, by each of the vectors and BM25.
pub fn said_before(store: &Store, request: &str) -> Result<Vec<Record>> {
    let q: String = request.chars().take(1500).collect();
    if terms(&q).is_empty() {
        return Ok(Vec::new());
    }
    let v = store.embedder.embed(&[q.as_str()])?.pop().unwrap_or_default();
    let cond = format!("((kind = 'note' AND tag = '{TASK_TAG}') OR (kind = 'preference' AND tag != 'superseded'))");
    let mut found: BTreeMap<String, Record> = BTreeMap::new();
    // More than kept: some are another's words.
    let by_vec = format!("SELECT {COLUMNS}, score() FROM memory WHERE {cond} ORDER BY similarity(emb, :v) DESC LIMIT {}", CANDIDATES * 2);
    for r in store.query_with(&by_vec, &Params::new().set("v", Value::Vector(v.into())))?.rows.iter().filter_map(|r| row_record(r)).filter(users_instruction).take(CANDIDATES) {
        found.insert(r.id.clone(), r);
    }
    if !chloedb::text::analyze_query(&q).is_empty() {
        let by_words = format!("SELECT {COLUMNS}, score() FROM memory WHERE {cond} ORDER BY bm25(body, :q, syntax => 'plain') DESC LIMIT {}", CANDIDATES * 2);
        for r in store.query_with(&by_words, &Params::new().set("q", Value::Str(q.clone())))?.rows.iter().filter_map(|r| row_record(r)).filter(users_instruction).take(CANDIDATES) {
            found.insert(r.id.clone(), r);
        }
    }
    let mut out: Vec<Record> = found.into_values().collect();
    out.sort_by_key(|r| r.seq);
    Ok(out)
}

/// [`said_before`] as the distiller is shown it: (id, instruction).
pub fn listed(before: &[Record]) -> Vec<(String, String)> {
    before.iter().map(|r| (r.id.clone(), instruction(&r.text).to_string())).collect()
}

/// What [`promote`] changed: the drafts to store, and for each rule it made,
/// the records that rule supersedes once stored.
#[derive(Debug, Default)]
pub struct Promoted {
    pub drafts: Vec<Draft>,
    /// The ids each promoted rule repeats (its `repeats` links): superseded
    /// by it when it is stored.
    pub repeats: Vec<String>,
}

/// The drafts of a turn, with each of the user's instructions that the
/// distiller said repeats one in `before` ([`said_before`]) made a rule of
/// every session, or written as the rule it repeats anew. `shown` is what
/// the session's context window held when the turn's prompt came.
pub fn promote(drafts: Vec<Draft>, session: &str, shown: &HashSet<String>, before: &[Record]) -> Promoted {
    let mut out = Promoted::default();
    for mut d in drafts {
        let said: Vec<&Record> = d.links.iter().filter(|l| l.label == "repeats").filter_map(|l| before.iter().find(|r| r.id == l.to)).collect();
        if !users_draft(&d) || said.is_empty() {
            d.links.retain(|l| l.label != "repeats");
            out.drafts.push(d);
            continue;
        }
        let rules: Vec<&Record> = said.iter().copied().filter(|r| r.kind == Kind::Preference).collect();
        let notes: Vec<&Record> = said.iter().copied().filter(|r| r.kind == Kind::Note).collect();
        // This session and the sessions of the notes it repeats, or those a
        // rule it repeats counted already and this one if it is another.
        let mut sessions: BTreeSet<&str> = notes.iter().map(|r| r.session.as_str()).collect();
        sessions.insert(session);
        let counted = rules.iter().map(|r| sessions_of(r) + usize::from(r.session != session)).max().unwrap_or(0);
        let n = sessions.len().max(counted);
        let times = rules.iter().map(|r| said_of(r)).max().unwrap_or(0) + notes.len() + 1;
        let mut ctx = format!("Sessions: {n}\nSaid: {times}");
        let mut when = d.when.clone();
        let mut files = d.files.clone();
        let mut entities = d.entities.clone();
        if !rules.is_empty() {
            // Shown in this window, or said in this session already, the
            // rule (or one it was written anew from) was the agent's to keep.
            let was_shown = rules.iter().any(|r| r.session == session || shown.contains(&r.id) || r.linked("repeats").any(|id| shown.contains(id)));
            let (again, after) = rules.iter().map(|r| said_again_of(r)).fold((0, 0), |a, b| (a.0.max(b.0), a.1.max(b.1)));
            ctx += &format!("\nSaid again: {}\nSaid again after it was shown: {}", again + 1, after + usize::from(was_shown));
        }
        for r in &said {
            for w in r.when().map(|w| w.split("; ").map(str::to_string).collect::<Vec<_>>()).unwrap_or_default() {
                if !when.contains(&w) {
                    when.push(w);
                }
            }
            for f in r.files.split_whitespace() {
                if !files.iter().any(|x| x == f) {
                    files.push(f.to_string());
                }
            }
            for e in r.linked("about").filter_map(|e| e.strip_prefix("e:")) {
                if !entities.iter().any(|x| x == e) {
                    entities.push(e.to_string());
                }
            }
        }
        when.truncate(8);
        let mut rule = Draft::new(Kind::Preference, rule_text(instruction(&d.text), n, times)).tag(REPEATED_TAG).by(&Source::User).files(files).when(when).ctx(ctx);
        rule.entities = entities;
        for r in &said {
            rule = rule.link("repeats", r.id.clone());
            out.repeats.push(r.id.clone());
        }
        out.drafts.push(rule);
    }
    out
}

/// Asks the distiller about each case, as a turn's distillation does, and
/// counts which instructions said before it marked as said again against
/// the ones `expect` names: how well it tells a repeat from the rest on
/// real sayings, with nothing stored. A case is
/// `{"id", "request", "answer"?, "before": [{"id", "text"}], "expect": [id]}`.
pub fn judge(cfg: &crate::distill::DistillConfig, cases: &[Json], parallel: usize) -> Json {
    let one = |c: &Json| -> Json {
        let before: Vec<(String, String)> = c["before"].as_array().into_iter().flatten().filter_map(|b| Some((b["id"].as_str()?.to_string(), b["text"].as_str()?.to_string()))).collect();
        let expect: BTreeSet<String> = c["expect"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect();
        let ex = crate::distill::Exchange { said_before: before, ..Default::default() };
        match crate::distill::distill_exchange(cfg, c["request"].as_str().unwrap_or(""), c["answer"].as_str().unwrap_or(""), &[], &[], 0, &ex) {
            Ok(d) => {
                let marked: BTreeSet<String> = d.drafts.iter().filter(|x| users_draft(x)).flat_map(|x| x.links.iter().filter(|l| l.label == "repeats").map(|l| l.to.clone())).collect();
                let items: Vec<String> = d.drafts.iter().filter(|x| users_draft(x)).map(|x| instruction(&x.text).to_string()).collect();
                json!({ "id": c["id"], "expect": expect, "marked": marked, "found": expect.intersection(&marked).count(), "false": marked.difference(&expect).count(), "items": items, "cost_usd": d.cost_usd })
            }
            Err(e) => json!({ "id": c["id"], "expect": expect, "error": e.to_string() }),
        }
    };
    let results: Vec<Json> = std::thread::scope(|sc| {
        let mut out = Vec::new();
        for chunk in cases.chunks(parallel.max(1)) {
            let hs: Vec<_> = chunk.iter().map(|c| sc.spawn(move || one(c))).collect();
            out.extend(hs.into_iter().map(|h| h.join().unwrap_or_else(|_| json!({ "error": "panicked" }))));
        }
        out
    });
    let sum = |k: &str| results.iter().map(|r| r[k].as_u64().unwrap_or(0)).sum::<u64>();
    let expected: usize = results.iter().map(|r| r["expect"].as_array().map_or(0, |a| a.len())).sum();
    let errors = results.iter().filter(|r| r.get("error").is_some()).count();
    let negatives = results.iter().filter(|r| r["expect"].as_array().is_some_and(|a| a.is_empty()) && r.get("error").is_none()).count();
    let marked_neg = results.iter().filter(|r| r["expect"].as_array().is_some_and(|a| a.is_empty()) && r["marked"].as_array().is_some_and(|a| !a.is_empty())).count();
    let cost: f64 = results.iter().map(|r| r["cost_usd"].as_f64().unwrap_or(0.0)).sum();
    let text = format!(
        "{} cases: {} of {expected} repeats found, {} instructions marked that are not; {marked_neg} of {negatives} cases with no repeat marked one; {errors} failed; ${cost:.4}.\n",
        results.len(),
        sum("found"),
        sum("false")
    );
    json!({ "text": text, "cases": results.len(), "expected": expected, "found": sum("found"), "false": sum("false"), "negatives": negatives, "marked_negatives": marked_neg, "errors": errors, "cost_usd": cost, "results": results })
}

/// After the drafts [`promote`] made are stored: each record a stored rule
/// repeats is superseded by it. The number superseded.
pub fn supersede_repeated(store: &Store, stored: &[Record]) -> usize {
    let mut n = 0;
    for rule in stored.iter().filter(|r| r.kind == Kind::Preference && r.tag == REPEATED_TAG) {
        for l in rule.links.iter().filter(|l| l.label == "repeats") {
            if store.supersede(&l.to, &rule.id).unwrap_or(false) {
                n += 1;
            }
        }
    }
    n
}

/// The repeats already in a store, written nowhere and with no model: the
/// user's task instructions since `since_ts` grouped with those of other
/// turns that say the same in nearly the same words ([`same`]), the groups
/// of more sessions and sayings first, and the rules kept for being said
/// again.
pub fn scan(store: &Store, since_ts: i64, limit: usize) -> Result<Json> {
    let sql = format!("SELECT {COLUMNS} FROM memory WHERE kind = 'note' AND tag = '{TASK_TAG}' AND ts >= {since_ts} ORDER BY seq DESC LIMIT {limit}");
    let notes: Vec<Record> = store.query_with(&sql, &Params::new())?.rows.iter().filter_map(|r| row_record(r)).filter(users).collect();
    let texts: Vec<&str> = notes.iter().map(|r| instruction(&r.text)).collect();
    let ts: Vec<BTreeSet<String>> = texts.iter().map(|t| terms(t)).collect();
    // Union-find over the pairs of different turns that say the same.
    let mut up: Vec<usize> = (0..notes.len()).collect();
    fn root(up: &mut [usize], mut i: usize) -> usize {
        while up[i] != i {
            up[i] = up[up[i]];
            i = up[i];
        }
        i
    }
    for i in 0..notes.len() {
        for j in i + 1..notes.len() {
            if (notes[i].session != notes[j].session || notes[i].turn != notes[j].turn) && same_terms(&ts[i], &ts[j]) {
                let (a, b) = (root(&mut up, i), root(&mut up, j));
                up[a] = b;
            }
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..notes.len() {
        let r = root(&mut up, i);
        groups.entry(r).or_default().push(i);
    }
    let mut out: Vec<Json> = groups
        .into_values()
        .filter_map(|g| {
            let turns: BTreeSet<(&str, i64)> = g.iter().map(|&i| (notes[i].session.as_str(), notes[i].turn)).collect();
            if turns.len() < 2 {
                return None;
            }
            let sessions: BTreeSet<&str> = turns.iter().map(|t| t.0).collect();
            // The newest wording is the rule's.
            let newest = *g.iter().max_by_key(|&&i| notes[i].ts).unwrap();
            let said: Vec<Json> = g.iter().map(|&i| json!({ "id": notes[i].id, "session": notes[i].session, "turn": notes[i].turn, "ts": notes[i].ts, "text": texts[i] })).collect();
            Some(json!({ "rule": rule_text(texts[newest], sessions.len(), turns.len()), "sessions": sessions.len(), "times": turns.len(), "said": said }))
        })
        .collect();
    out.sort_by_key(|g| (std::cmp::Reverse(g["sessions"].as_u64().unwrap_or(0)), std::cmp::Reverse(g["times"].as_u64().unwrap_or(0)), g["rule"].as_str().unwrap_or("").to_string()));
    // The rules kept for being said again, those said again since first.
    let mut rules: Vec<Record> = store.query_with(&format!("SELECT {COLUMNS} FROM memory WHERE kind = 'preference' AND tag = '{REPEATED_TAG}' LIMIT 1000"), &Params::new())?.rows.iter().filter_map(|r| row_record(r)).collect();
    rules.sort_by_key(|r| (std::cmp::Reverse(said_again_of(r).1), std::cmp::Reverse(said_again_of(r).0), std::cmp::Reverse(r.seq)));
    let kept: Vec<Json> = rules
        .iter()
        .map(|r| {
            let (again, shown) = said_again_of(r);
            json!({ "id": r.id, "rule": r.text, "ts": r.ts, "sessions": sessions_of(r), "times": said_of(r), "said_again": again, "said_again_after_shown": shown, "files": r.files, "when": r.when() })
        })
        .collect();
    let mut text = format!("{} of your instructions for one task, since {}; {} said again in nearly the same words; {} rules kept for being said again.\n", notes.len(), day(since_ts), out.len(), kept.len());
    for g in &out {
        text += &format!("\n- {}\n", g["rule"].as_str().unwrap_or(""));
        for s in g["said"].as_array().into_iter().flatten() {
            text += &format!("    {} {}: {}\n", day(s["ts"].as_i64().unwrap_or(0)), &s["session"].as_str().unwrap_or("")[..8.min(s["session"].as_str().unwrap_or("").len())], s["text"].as_str().unwrap_or(""));
        }
    }
    let again: Vec<&Record> = rules.iter().filter(|r| said_again_of(r).0 > 0).collect();
    if !again.is_empty() {
        text += "\nRules said again once kept (after the session was shown the rule, of the times said again):\n";
        for r in again {
            let (n, shown) = said_again_of(r);
            text += &format!("- {} ({shown} of {n})\n", r.text);
        }
    }
    Ok(json!({ "text": text, "notes": notes.len(), "groups": out, "kept": kept.len(), "rules": kept }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::HashEmbedder;
    use crate::store::StoreOptions;
    use std::sync::Arc;

    fn store(dir: &std::path::Path) -> Store {
        Store::open(&dir.join("memory.strata"), Arc::new(HashEmbedder { dim: 64 }), StoreOptions::default()).unwrap()
    }

    fn task(text: &str) -> Draft {
        Draft::new(Kind::Note, format!("For this task only (the user said, not a standing rule): {text}")).tag(TASK_TAG).by(&Source::User)
    }

    /// A draft the distiller said repeats `ids`.
    fn again(d: Draft, ids: &[&str]) -> Draft {
        ids.iter().fold(d, |d, id| d.link("repeats", id.to_string()))
    }

    fn live_rules(s: &Store) -> Vec<Record> {
        s.query_with(&format!("SELECT {COLUMNS} FROM memory WHERE kind = 'preference' AND tag != 'superseded'"), &Params::new()).unwrap().rows.iter().filter_map(|r| row_record(r)).collect()
    }

    #[test]
    fn an_instruction_is_read_without_whose_it_is_or_its_count() {
        assert_eq!(instruction("For lead's task only (the user said, not a standing rule): Build it from the CREATE statements."), "Build it from the CREATE statements.");
        assert_eq!(instruction(&rule_text("Call it ChloeDB, not Strata.", 3, 4)), "Call it ChloeDB, not Strata");
        assert_eq!(rule_text("Call it ChloeDB.", 1, 2), "Call it ChloeDB (the user has said this 2 times)");
        assert_eq!(instruction(&rule_text("Call it ChloeDB.", 1, 2)), "Call it ChloeDB");
        assert_eq!(instruction("Use tabs"), "Use tabs");
    }

    #[test]
    fn a_scan_matches_only_nearly_the_same_words() {
        assert!(same("Build the app from the SQL CREATE statements", "Build apps from SQL and CREATE statements"));
        // Real rewordings, and different instructions that share a word or
        // two: left to the distiller.
        assert!(!same("Call the database ChloeDB in prose, not Strata", "Say ChloeDB, never Strata"));
        assert!(!same("Install Workbench on the Mac", "Check the sync push on the Mac"));
        assert!(!same("Fix the login page", "Rename the cart module"));
    }

    #[test]
    fn the_distiller_is_shown_the_users_own_earlier_instructions() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        s.append("s1", 1, 100, vec![task("Build the shop from the SQL CREATE statements.")]).unwrap();
        s.append("s1", 2, 110, vec![Draft::new(Kind::Note, "For lead's task only (the agent lead said, not a standing rule): Build the shop from the CREATE statements.").tag(TASK_TAG).by(&Source::Agent("lead".into()))]).unwrap();
        s.append("s2", 1, 120, vec![Draft::new(Kind::Preference, "Always keep the shop's money as integer cents.").by(&Source::User)]).unwrap();
        s.append("s2", 2, 130, vec![Draft::new(Kind::Fact, "The shop's CREATE statements are in schema.sql.")]).unwrap();
        let before = said_before(&s, "Build the shop from the CREATE statements, please").unwrap();
        let texts: Vec<&str> = before.iter().map(|r| instruction(&r.text)).collect();
        assert_eq!(texts, ["Build the shop from the SQL CREATE statements.", "Always keep the shop's money as integer cents."], "the user's notes for a task and rules, not an agent's, nor facts");
        assert_eq!(listed(&before)[0], (before[0].id.clone(), "Build the shop from the SQL CREATE statements.".to_string()));
    }

    #[test]
    fn an_instruction_said_again_becomes_a_rule_counted_in_times_and_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        let n1 = s.append("s1", 1, 100, vec![task("Build the app from the SQL CREATE statements.")]).unwrap().remove(0);
        let lead = s.append("s1", 2, 105, vec![Draft::new(Kind::Note, "For lead's task only (the agent lead said, not a standing rule): Use the red theme.").tag(TASK_TAG).by(&Source::Agent("lead".into()))]).unwrap().remove(0);
        let before = vec![n1.clone()];

        // The distiller said nothing repeats, or named a record it was not
        // shown: nothing changes, and the link goes.
        let p = promote(vec![task("Rename the cart module."), again(task("Use the red theme."), &[&lead.id])], "s1", &HashSet::new(), &before);
        assert!(p.repeats.is_empty() && p.drafts.iter().all(|d| d.kind == Kind::Note && d.links.is_empty()), "{p:?}");
        // Another agent's words that say it again: that agent's brief.
        let brief = again(Draft::new(Kind::Note, "For lead's task only (the agent lead said, not a standing rule): Build it from CREATE statements.").tag(TASK_TAG).by(&Source::Agent("lead".into())), &[&n1.id]);
        let p = promote(vec![brief], "s1", &HashSet::new(), &before);
        assert!(p.repeats.is_empty() && p.drafts[0].kind == Kind::Note);

        // The user says it again later in s1, in other words: a rule, said twice.
        let p = promote(vec![again(task("No: the schema is the CREATE statements in schema.sql, build from those."), &[&n1.id]), Draft::new(Kind::Fact, "The cart is in cart.py")], "s1", &HashSet::new(), &before);
        assert_eq!(p.drafts.len(), 2);
        let rule = &p.drafts[0];
        assert_eq!((rule.kind, rule.tag.as_str()), (Kind::Preference, REPEATED_TAG));
        assert_eq!(rule.text, "No: the schema is the CREATE statements in schema.sql, build from those (the user has said this 2 times)");
        assert_eq!(p.repeats, vec![n1.id.clone()]);
        let r2 = s.append("s1", 3, 130, p.drafts).unwrap().remove(0);
        assert_eq!(supersede_repeated(&s, std::slice::from_ref(&r2)), 1);
        assert_eq!(s.get(std::slice::from_ref(&n1.id)).unwrap()[0].tag, "superseded");
        assert_eq!((sessions_of(&r2), said_of(&r2), said_again_of(&r2)), (1, 2, (0, 0)), "{}", r2.ctx);

        // In another session: 2 sessions, 3 times, said again once kept.
        let p = promote(vec![again(task("Build apps from SQL and CREATE statements."), &[&r2.id])], "s2", &HashSet::new(), &[r2.clone()]);
        assert_eq!(p.drafts[0].text, "Build apps from SQL and CREATE statements (the user has said this in 2 sessions)");
        let r3 = s.append("s2", 1, 140, p.drafts).unwrap().remove(0);
        supersede_repeated(&s, std::slice::from_ref(&r3));
        assert_eq!((sessions_of(&r3), said_of(&r3), said_again_of(&r3)), (2, 3, (1, 0)), "{}", r3.ctx);
        assert_eq!(live_rules(&s).len(), 1, "one rule, written anew each time");
    }

    #[test]
    fn a_rule_said_again_counts_whether_the_session_was_shown_it_and_is_recalled_where_it_mattered() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        // A rule the user gave as one, in s1.
        let given = Draft::new(Kind::Preference, "Always keep money as integer cents.").by(&Source::User).when(vec!["writing prices".into()]).files(vec!["money.py".into()]);
        let r1 = s.append("s1", 1, 100, vec![given]).unwrap().remove(0);
        assert_eq!((sessions_of(&r1), said_of(&r1), said_again_of(&r1)), (1, 1, (0, 0)));

        // s2 was shown it and the user had to say it again: the agent broke
        // a rule it had.
        let shown: HashSet<String> = [r1.id.clone()].into();
        let said = again(task("Keep money as integer cents, not floats.").when(vec!["computing a cart total".into()]).files(vec!["cart.py".into()]), &[&r1.id]);
        let p = promote(vec![said], "s2", &shown, std::slice::from_ref(&r1));
        let rule = &p.drafts[0];
        assert_eq!((rule.kind, rule.tag.as_str()), (Kind::Preference, REPEATED_TAG));
        assert_eq!(rule.text, "Keep money as integer cents, not floats (the user has said this in 2 sessions)");
        assert_eq!(p.repeats, vec![r1.id.clone()]);
        // Recalled where it mattered from now on, and where it did before.
        assert_eq!(rule.when, vec!["computing a cart total".to_string(), "writing prices".to_string()]);
        assert_eq!(rule.files, vec!["cart.py".to_string(), "money.py".to_string()]);
        let r2 = s.append("s2", 4, 200, p.drafts).unwrap().remove(0);
        assert_eq!(supersede_repeated(&s, std::slice::from_ref(&r2)), 1);
        assert_eq!((sessions_of(&r2), said_of(&r2), said_again_of(&r2)), (2, 2, (1, 1)), "{}", r2.ctx);

        // s3 was not shown it: said again, not after it was shown.
        let p = promote(vec![again(task("Keep money in integer cents."), &[&r2.id])], "s3", &HashSet::new(), std::slice::from_ref(&r2));
        let r3 = s.append("s3", 2, 300, p.drafts).unwrap().remove(0);
        supersede_repeated(&s, std::slice::from_ref(&r3));
        assert_eq!((sessions_of(&r3), said_again_of(&r3)), (3, (2, 1)), "{}", r3.ctx);

        // Said again later in s3, which wrote it: the agent had it, and no
        // session more is counted.
        let p = promote(vec![again(task("Keep money as integer cents!"), &[&r3.id])], "s3", &HashSet::new(), std::slice::from_ref(&r3));
        let r4 = s.append("s3", 5, 310, p.drafts).unwrap().remove(0);
        supersede_repeated(&s, std::slice::from_ref(&r4));
        assert_eq!((sessions_of(&r4), said_of(&r4), said_again_of(&r4)), (3, 4, (3, 2)), "{}", r4.ctx);
        assert_eq!(live_rules(&s).len(), 1);

        let r = scan(&s, 0, 1000).unwrap();
        assert_eq!((r["rules"][0]["said_again"].as_u64(), r["rules"][0]["said_again_after_shown"].as_u64(), r["rules"][0]["times"].as_u64()), (Some(3), Some(2), Some(4)));
        assert!(r["text"].as_str().unwrap().contains("(2 of 3)"), "{}", r["text"]);
    }

    #[test]
    fn the_judge_counts_the_repeats_the_distiller_marks_against_the_expected_ones() {
        let dir = tempfile::tempdir().unwrap();
        // A model that marks the first instruction it was shown, if any.
        let out = dir.path().join("out.json");
        let line = json!({"kind": "rule", "text": "Build the shop from the CREATE statements.", "said_by": "requester", "scope": "task", "repeats": ["__ID__"]}).to_string();
        std::fs::write(&out, json!({"is_error": false, "total_cost_usd": 0.001, "result": line}).to_string()).unwrap();
        let script = dir.path().join("model.sh");
        std::fs::write(&script, format!("#!/bin/sh\np=$(cat)\nid=$(printf '%s\\n' \"$p\" | sed -n '/<said_before>/,/<\\/said_before>/p' | sed -n 's/^\\[\\([^]]*\\)\\].*/\\1/p' | head -1)\nsed \"s/__ID__/$id/\" '{}'\n", out.display())).unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let cfg = crate::distill::DistillConfig { command: script.to_string_lossy().into(), model: "m".into(), ..Default::default() };
        let cases = vec![
            json!({"id": "B9", "request": "No, from the CREATE statements.", "before": [{"id": "A1", "text": "Build it from the SQL schema."}], "expect": ["A1"]}),
            json!({"id": "X", "request": "Rename the cart.", "before": [{"id": "A2", "text": "Use tabs."}], "expect": []}),
            json!({"id": "Y", "request": "Build the shop.", "before": [], "expect": []}),
        ];
        let r = judge(&cfg, &cases, 2);
        assert_eq!((r["found"].as_u64(), r["expected"].as_u64(), r["false"].as_u64()), (Some(1), Some(1), Some(1)), "{r}");
        assert_eq!((r["marked_negatives"].as_u64(), r["negatives"].as_u64(), r["errors"].as_u64()), (Some(1), Some(2), Some(0)), "{r}");
        assert_eq!(r["results"][0]["marked"], json!(["A1"]));
    }

    #[test]
    fn a_scan_groups_what_was_said_again_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        s.append("s1", 1, 100, vec![task("Build the app from the SQL CREATE statements.")]).unwrap();
        s.append("s1", 4, 150, vec![task("Build apps from SQL and CREATE statements.")]).unwrap();
        s.append("s3", 1, 300, vec![task("Build the app from SQL CREATE statements")]).unwrap();
        s.append("s3", 2, 310, vec![task("Rename the cart module to basket.")]).unwrap();
        let before = s.len();
        let r = scan(&s, 0, 1000).unwrap();
        assert_eq!(s.len(), before);
        assert_eq!(r["notes"], 4);
        let g = r["groups"].as_array().unwrap();
        assert_eq!(g.len(), 1, "{r}");
        assert_eq!((g[0]["sessions"].as_u64(), g[0]["times"].as_u64()), (Some(2), Some(3)));
        assert_eq!(g[0]["rule"], "Build the app from SQL CREATE statements (the user has said this in 2 sessions)");
        assert!(r["text"].as_str().unwrap().contains("1 said again in nearly the same words"), "{}", r["text"]);
    }
}
