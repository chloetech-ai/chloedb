//! Instructions said again: an instruction the user gave for one task is a
//! note of that session only ([`TASK_TAG`], [`crate::distill::attribute`]):
//! no other session is told it, because nothing said it should hold beyond
//! the task. Said again in another session, it does: the user should not
//! have to say it a third time. The second time, the note becomes a rule
//! ([`REPEATED_TAG`]) every session is given, and the notes it repeats are
//! superseded by it.
//!
//! An instruction that says what a rule already says (one kept this way,
//! or one the user gave as a rule) means the rule did not hold. Was the
//! session shown it? Then the agent broke a rule it had; if not, the rule
//! was not there when it mattered. Either way the rule is written anew in
//! the newest words, with its situations (`When:`), files and entities and
//! the new ones, so it is the newest rule and recalled in this situation
//! from now on, and it counts the sessions it was said in and the times it
//! was said again, and of those the times the session had been shown it
//! (`Said again:`, `Said again after it was shown:`).
//!
//! Two instructions say the same when most of their words are the same, or
//! their meaning is close and they share at least two words ([`same`]).
//! Only the user's own words count: an instruction another agent or
//! Workbench gave is that one's brief, not the user's correction.
//!
//! [`scan`] finds the repeats already in a store without writing anything
//! (`chloectx repeats`), to see what this would keep before it keeps it.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use chloedb::query::Params;
use chloedb::Value;
use serde_json::{json, Value as Json};

use crate::record::{Draft, Kind, Record, Source, COLUMNS, TASK_TAG};
use crate::store::{row_record, Store};
use crate::Result;

/// The tag of a rule kept because the user said it in more than one session.
pub const REPEATED_TAG: &str = "repeated";

/// Two instructions are the same when this share of their words (stemmed,
/// stop words out) is common to both...
const JACCARD: f64 = 0.5;
/// ...or their vectors are this close and they share [`SHARED`] words.
/// Measured with potion-retrieval-32M on instructions said twice in other
/// words (0.65 to 0.81 where the wording is close, 0.38 to 0.53 where it
/// is not) and on different instructions that share a word or three (0.04
/// to 0.39).
const COS: f64 = 0.65;
const SHARED: usize = 2;
/// Earlier instructions looked at for each new one, by each of BM25 and
/// the vectors.
const CANDIDATES: usize = 20;

/// The instruction a task note keeps, without the words that say whose it
/// is and for which task ("For lead's task only (the user said, not a
/// standing rule): "), or a repeated rule's without its count.
pub fn instruction(text: &str) -> &str {
    let t = text.split_once("not a standing rule): ").map(|x| x.1).unwrap_or(text);
    let t = match t.rfind(" (the user has said this in ") {
        Some(i) if t.ends_with(" sessions)") => &t[..i],
        _ => t,
    };
    t.trim()
}

/// The text of a rule said in `n` sessions.
pub fn rule_text(instruction: &str, n: usize) -> String {
    let t = instruction.trim().trim_end_matches('.');
    if n < 2 {
        format!("{t}.")
    } else {
        format!("{t} (the user has said this in {n} sessions)")
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

fn cosine(a: &[f32], b: &[f32]) -> f64 {
    let (mut d, mut na, mut nb) = (0.0f64, 0.0f64, 0.0f64);
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

/// Whether two instructions say the same, given the cosine of their vectors.
pub fn same(a: &str, b: &str, cos: f64) -> bool {
    same_terms(&terms(a), &terms(b), cos)
}

/// [`same`], on the instructions' terms.
fn same_terms(ta: &BTreeSet<String>, tb: &BTreeSet<String>, cos: f64) -> bool {
    let inter = ta.intersection(tb).count();
    let union = ta.len() + tb.len() - inter;
    if union == 0 || inter == 0 {
        return false;
    }
    inter as f64 / union as f64 >= JACCARD || (cos >= COS && inter >= SHARED)
}

/// Whether a record holds the user's own words (a record with no `By:`
/// line was written before sources were kept, and is the user's).
fn users(r: &Record) -> bool {
    matches!(r.source(), None | Some(Source::User))
}

/// Whether a draft is an instruction the user gave for one task.
fn users_task_note(d: &Draft) -> bool {
    d.kind == Kind::Note && d.tag == TASK_TAG && matches!(d.by.as_deref().map(Source::parse), None | Some(Source::User))
}

/// The user's earlier instructions that say what `text` says: task notes
/// of other sessions, and rules (of any session).
pub fn earlier(store: &Store, text: &str, session: &str) -> Result<Vec<Record>> {
    let instr = instruction(text);
    if terms(instr).is_empty() {
        return Ok(Vec::new());
    }
    let v = store.embedder.embed(&[instr])?.pop().unwrap_or_default();
    let cond = format!("((session != :s AND kind = 'note' AND tag = '{TASK_TAG}') OR (kind = 'preference' AND tag != 'superseded'))");
    let p = Params::new().set("s", Value::Str(session.to_string()));
    let mut found: BTreeMap<String, Record> = BTreeMap::new();
    let by_vec = format!("SELECT {COLUMNS}, score() FROM memory WHERE {cond} ORDER BY similarity(emb, :v) DESC LIMIT {CANDIDATES}");
    for r in store.query_with(&by_vec, &p.clone().set("v", Value::Vector(v.clone().into())))?.rows.iter().filter_map(|r| row_record(r)) {
        found.insert(r.id.clone(), r);
    }
    if !chloedb::text::analyze_query(instr).is_empty() {
        let by_words = format!("SELECT {COLUMNS}, score() FROM memory WHERE {cond} ORDER BY bm25(body, :q, syntax => 'plain') DESC LIMIT {CANDIDATES}");
        for r in store.query_with(&by_words, &p.set("q", Value::Str(instr.to_string())))?.rows.iter().filter_map(|r| row_record(r)) {
            found.insert(r.id.clone(), r);
        }
    }
    let cands: Vec<Record> = found.into_values().filter(users).collect();
    if cands.is_empty() {
        return Ok(Vec::new());
    }
    let texts: Vec<&str> = cands.iter().map(|r| instruction(&r.text)).collect();
    let vs = store.embedder.embed(&texts)?;
    Ok(cands.into_iter().zip(vs).filter(|(r, w)| same(instr, instruction(&r.text), cosine(&v, w))).map(|(r, _)| r).collect())
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

/// The drafts of a turn, with each of the user's task instructions that
/// repeats one of another session made a rule of every session, and each
/// that says what a rule says written as that rule anew. `shown` is what
/// the session's context window held when the turn's prompt came. A store
/// that cannot be searched leaves them as they are.
pub fn promote(store: &Store, drafts: Vec<Draft>, session: &str, shown: &HashSet<String>) -> Promoted {
    let mut out = Promoted::default();
    for d in drafts {
        if !users_task_note(&d) {
            out.drafts.push(d);
            continue;
        }
        let before = earlier(store, &d.text, session).unwrap_or_default();
        if before.is_empty() {
            out.drafts.push(d);
            continue;
        }
        let rules: Vec<&Record> = before.iter().filter(|r| r.kind == Kind::Preference).collect();
        // This session and the sessions of the notes it repeats, or those a
        // rule it repeats counted already and this one if it is another.
        let mut sessions: BTreeSet<&str> = before.iter().filter(|r| r.kind == Kind::Note).map(|r| r.session.as_str()).collect();
        sessions.insert(session);
        let counted = rules.iter().map(|r| sessions_of(r) + usize::from(r.session != session)).max().unwrap_or(0);
        let n = sessions.len().max(counted);
        let mut ctx = format!("Sessions: {n}");
        let mut when = d.when.clone();
        let mut files = d.files.clone();
        let mut entities = d.entities.clone();
        if !rules.is_empty() {
            // Shown in this window, or said in this session already, the
            // rule (or one it was written anew from) was the agent's to keep.
            let was_shown = rules.iter().any(|r| r.session == session || shown.contains(&r.id) || r.linked("repeats").any(|id| shown.contains(id)));
            let (again, after) = rules.iter().map(|r| said_again_of(r)).fold((0, 0), |a, b| (a.0.max(b.0), a.1.max(b.1)));
            ctx += &format!("\nSaid again: {}\nSaid again after it was shown: {}", again + 1, after + usize::from(was_shown));
            for r in &rules {
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
        }
        let mut rule = Draft::new(Kind::Preference, rule_text(instruction(&d.text), n)).tag(REPEATED_TAG).by(&Source::User).files(files).when(when).ctx(ctx);
        rule.entities = entities;
        for r in &before {
            rule = rule.link("repeats", r.id.clone());
            out.repeats.push(r.id.clone());
        }
        out.drafts.push(rule);
    }
    out
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

/// The repeats already in a store, written nowhere: the user's task
/// instructions since `since_ts` grouped with those of other sessions that
/// say the same, the groups of two or more sessions first by how many.
pub fn scan(store: &Store, since_ts: i64, limit: usize) -> Result<Json> {
    let sql = format!("SELECT {COLUMNS} FROM memory WHERE kind = 'note' AND tag = '{TASK_TAG}' AND ts >= {since_ts} ORDER BY seq DESC LIMIT {limit}");
    let notes: Vec<Record> = store.query_with(&sql, &Params::new())?.rows.iter().filter_map(|r| row_record(r)).filter(users).collect();
    let texts: Vec<&str> = notes.iter().map(|r| instruction(&r.text)).collect();
    let vs = if texts.is_empty() { Vec::new() } else { store.embedder.embed(&texts)? };
    let ts: Vec<BTreeSet<String>> = texts.iter().map(|t| terms(t)).collect();
    // Union-find over the pairs of different sessions that say the same.
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
            if notes[i].session != notes[j].session && same_terms(&ts[i], &ts[j], cosine(&vs[i], &vs[j])) {
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
            let sessions: BTreeSet<&str> = g.iter().map(|&i| notes[i].session.as_str()).collect();
            if sessions.len() < 2 {
                return None;
            }
            // The newest wording is the rule's.
            let newest = *g.iter().max_by_key(|&&i| notes[i].ts).unwrap();
            let said: Vec<Json> = g.iter().map(|&i| json!({ "id": notes[i].id, "session": notes[i].session, "ts": notes[i].ts, "text": texts[i] })).collect();
            Some(json!({ "rule": rule_text(texts[newest], sessions.len()), "sessions": sessions.len(), "said": said }))
        })
        .collect();
    out.sort_by_key(|g| (std::cmp::Reverse(g["sessions"].as_u64().unwrap_or(0)), g["rule"].as_str().unwrap_or("").to_string()));
    // The rules kept for being said again, those said again since first.
    let mut rules: Vec<Record> = store.query_with(&format!("SELECT {COLUMNS} FROM memory WHERE kind = 'preference' AND tag = '{REPEATED_TAG}' LIMIT 1000"), &Params::new())?.rows.iter().filter_map(|r| row_record(r)).collect();
    rules.sort_by_key(|r| (std::cmp::Reverse(said_again_of(r).1), std::cmp::Reverse(said_again_of(r).0), std::cmp::Reverse(r.seq)));
    let kept: Vec<Json> = rules
        .iter()
        .map(|r| {
            let (again, shown) = said_again_of(r);
            json!({ "id": r.id, "rule": r.text, "ts": r.ts, "sessions": sessions_of(r), "said_again": again, "said_again_after_shown": shown, "files": r.files, "when": r.when() })
        })
        .collect();
    let mut text = format!("{} of your instructions for one task, since {}; {} said in more than one session; {} rules kept for being said again.\n", notes.len(), day(since_ts), out.len(), kept.len());
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

    #[test]
    fn an_instruction_is_read_without_whose_it_is_or_its_count() {
        assert_eq!(instruction("For lead's task only (the user said, not a standing rule): Build it from the CREATE statements."), "Build it from the CREATE statements.");
        assert_eq!(instruction(&rule_text("Call it ChloeDB, not Strata.", 3)), "Call it ChloeDB, not Strata");
        assert_eq!(instruction("Use tabs"), "Use tabs");
    }

    #[test]
    fn instructions_are_the_same_by_their_words_or_their_meaning() {
        assert!(same("Build the app from the SQL CREATE statements", "Build apps from SQL and CREATE statements", 0.0));
        // Close in meaning, two words in common.
        assert!(same("Call the database ChloeDB in prose, not Strata", "Say ChloeDB, never Strata", 0.85));
        assert!(!same("Call the database ChloeDB in prose, not Strata", "Say ChloeDB, never Strata", 0.5));
        // Close vectors, one word in common: not enough.
        assert!(!same("Run the tests", "Run nothing else", 0.9));
        assert!(!same("Fix the login page", "Rename the cart module", 0.1));
    }

    #[test]
    fn an_instruction_said_in_a_second_session_becomes_a_rule_and_a_third_counts() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        s.append("s1", 1, 100, vec![task("Build the app from the SQL CREATE statements.")]).unwrap();
        // Another agent's brief and the same session's words do not count.
        s.append("s2", 1, 120, vec![Draft::new(Kind::Note, "For lead's task only (the agent lead said, not a standing rule): Use the red theme.").tag(TASK_TAG).by(&Source::Agent("lead".into()))]).unwrap();

        // Said in the same session again: nothing changes.
        let p = promote(&s, vec![task("Build the app from the SQL CREATE statements.")], "s1", &HashSet::new());
        assert_eq!(p.drafts[0].kind, Kind::Note);
        assert!(p.repeats.is_empty());
        // Another agent's brief repeated by the user: not a repeat of the user.
        let p = promote(&s, vec![task("Use the red theme.")], "s3", &HashSet::new());
        assert!(p.drafts[0].tag == TASK_TAG && p.repeats.is_empty(), "{p:?}");

        // The user says it in a second session: a rule of every session.
        let p = promote(&s, vec![task("Build apps from SQL and CREATE statements."), Draft::new(Kind::Fact, "The cart is in cart.py")], "s2", &HashSet::new());
        assert_eq!(p.drafts.len(), 2);
        let rule = &p.drafts[0];
        assert_eq!((rule.kind, rule.tag.as_str()), (Kind::Preference, REPEATED_TAG));
        assert_eq!(rule.text, "Build apps from SQL and CREATE statements (the user has said this in 2 sessions)");
        assert_eq!(p.repeats.len(), 1);
        let stored = s.append("s2", 3, 130, p.drafts).unwrap();
        assert_eq!(supersede_repeated(&s, &stored), 1);
        let first = s.query_with(&format!("SELECT {COLUMNS} FROM memory WHERE session = 's1' AND turn = 1 AND kind = 'note'"), &Params::new()).unwrap();
        assert_eq!(row_record(&first.rows[0]).unwrap().tag, "superseded");

        // A third session: the rule is written anew, with 3 sessions.
        let p = promote(&s, vec![task("Build the app from the SQL and CREATE statements")], "s4", &HashSet::new());
        assert_eq!(p.drafts[0].text, "Build the app from the SQL and CREATE statements (the user has said this in 3 sessions)");
        let stored = s.append("s4", 1, 140, p.drafts).unwrap();
        assert_eq!(supersede_repeated(&s, &stored), 1);
        let live = s.query_with(&format!("SELECT {COLUMNS} FROM memory WHERE kind = 'preference' AND tag = '{REPEATED_TAG}'"), &Params::new()).unwrap();
        assert_eq!(live.rows.len(), 1, "the 2-session rule is superseded by the 3-session one");
        let rule = row_record(&live.rows[0]).unwrap();
        assert_eq!((sessions_of(&rule), said_again_of(&rule)), (3, (1, 0)), "{}", rule.ctx);
    }

    #[test]
    fn a_rule_said_again_counts_whether_the_session_was_shown_it_and_is_recalled_where_it_mattered() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        // A rule the user gave as one, in s1.
        let given = Draft::new(Kind::Preference, "Always keep money as integer cents.").by(&Source::User).when(vec!["writing prices".into()]).files(vec!["money.py".into()]);
        let r1 = s.append("s1", 1, 100, vec![given]).unwrap().remove(0);
        assert_eq!((sessions_of(&r1), said_again_of(&r1)), (1, (0, 0)));

        // s2 was shown it and the user had to say it again: the agent broke
        // a rule it had.
        let shown: HashSet<String> = [r1.id.clone()].into();
        let said = task("Keep money as integer cents, not floats.").when(vec!["computing a cart total".into()]).files(vec!["cart.py".into()]);
        let p = promote(&s, vec![said], "s2", &shown);
        let rule = &p.drafts[0];
        assert_eq!((rule.kind, rule.tag.as_str()), (Kind::Preference, REPEATED_TAG));
        assert_eq!(rule.text, "Keep money as integer cents, not floats (the user has said this in 2 sessions)");
        assert_eq!(p.repeats, vec![r1.id.clone()]);
        // Recalled where it mattered from now on, and where it did before.
        assert_eq!(rule.when, vec!["computing a cart total".to_string(), "writing prices".to_string()]);
        assert_eq!(rule.files, vec!["cart.py".to_string(), "money.py".to_string()]);
        let r2 = s.append("s2", 4, 200, p.drafts).unwrap().remove(0);
        assert_eq!(supersede_repeated(&s, std::slice::from_ref(&r2)), 1);
        assert_eq!((sessions_of(&r2), said_again_of(&r2)), (2, (1, 1)), "{}", r2.ctx);

        // s3 was not shown it: said again, not after it was shown.
        let p = promote(&s, vec![task("Keep money in integer cents.")], "s3", &HashSet::new());
        let r3 = s.append("s3", 2, 300, p.drafts).unwrap().remove(0);
        supersede_repeated(&s, std::slice::from_ref(&r3));
        assert_eq!((sessions_of(&r3), said_again_of(&r3)), (3, (2, 1)), "{}", r3.ctx);

        // Said again later in s3, which wrote it: the agent had it, and no
        // session more is counted.
        let p = promote(&s, vec![task("Keep money as integer cents!")], "s3", &HashSet::new());
        let r4 = s.append("s3", 5, 310, p.drafts).unwrap().remove(0);
        supersede_repeated(&s, std::slice::from_ref(&r4));
        assert_eq!((sessions_of(&r4), said_again_of(&r4)), (3, (3, 2)), "{}", r4.ctx);
        let live = s.query_with(&format!("SELECT {COLUMNS} FROM memory WHERE kind = 'preference' AND tag != 'superseded'"), &Params::new()).unwrap();
        assert_eq!(live.rows.len(), 1);

        let r = scan(&s, 0, 1000).unwrap();
        assert_eq!(r["rules"][0]["said_again"], 3);
        assert_eq!(r["rules"][0]["said_again_after_shown"], 2);
        assert!(r["text"].as_str().unwrap().contains("(2 of 3)"), "{}", r["text"]);
    }

    #[test]
    fn a_scan_groups_what_was_said_in_more_than_one_session_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        s.append("s1", 1, 100, vec![task("Build the app from the SQL CREATE statements.")]).unwrap();
        s.append("s2", 1, 200, vec![task("Build apps from SQL and CREATE statements.")]).unwrap();
        s.append("s3", 1, 300, vec![task("Build the app from SQL CREATE statements")]).unwrap();
        s.append("s3", 2, 310, vec![task("Rename the cart module to basket.")]).unwrap();
        let before = s.len();
        let r = scan(&s, 0, 1000).unwrap();
        assert_eq!(s.len(), before);
        assert_eq!(r["notes"], 4);
        let g = r["groups"].as_array().unwrap();
        assert_eq!(g.len(), 1, "{r}");
        assert_eq!(g[0]["sessions"], 3);
        assert_eq!(g[0]["rule"], "Build the app from SQL CREATE statements (the user has said this in 3 sessions)");
        assert!(r["text"].as_str().unwrap().contains("1 said in more than one session"), "{}", r["text"]);
    }
}

