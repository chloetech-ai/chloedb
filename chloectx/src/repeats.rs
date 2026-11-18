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
//! Which earlier sayings a request says again is a model's to judge
//! ([`check`]), in a call of its own after the turn's distillation: it is
//! shown the user's earlier requests and instructions most like this one
//! ([`said_before`]), from this session and others, and the agent's reply
//! before it. The user rewords a correction more than words or vectors can
//! follow (in a week of real sessions, an instruction and its saying again
//! scored a cosine of 0.05 to 0.57 with potion-retrieval-32M, and different
//! instructions up to 0.73), and often says it again as a complaint ("why
//! does it still say Strata?") that the distiller writes no instruction
//! for. The model quotes the words of the request that say it again, and a
//! mark whose quote is not in the request is dropped: given little to go
//! on, a model copies what it was shown. Only the user's own words count:
//! an instruction another agent or Workbench gave is that one's brief, not
//! the user's correction.
//!
//! [`scan`] lists, without writing anything or calling a model
//! (`chloectx repeats`), the rules kept this way and the instructions said
//! again in nearly the same words ([`same`]).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::OnceLock;

use chloedb::query::Params;
use chloedb::Value;
use regex::Regex;
use serde_json::{json, Value as Json};

use crate::record::{Draft, Kind, Record, Source, COLUMNS, TASK_TAG};
use crate::store::{row_record, Store};
use crate::Result;

/// The tag of a rule kept because the user said it again.
pub const REPEATED_TAG: &str = "repeated";

/// Two instructions are nearly the same words when this share of their
/// words (stemmed, stop words out) is common to both ([`scan`] only).
const JACCARD: f64 = 0.5;
/// Earlier instructions (notes for one task, rules) shown to [`check`], by
/// each of BM25 and the vectors.
const CANDIDATES: usize = 8;
/// Earlier prompts of the user's shown to [`check`], by each.
const PROMPTS: usize = 6;

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
    users(r) && ((r.kind == Kind::Note && r.tag == TASK_TAG) || (r.kind == Kind::Preference && !crate::record::closed_tag(&r.tag)))
}

/// Whether a draft is an instruction the user gave: for one task, or as a
/// rule.
fn users_draft(d: &Draft) -> bool {
    matches!(d.by.as_deref().map(Source::parse), None | Some(Source::User)) && ((d.kind == Kind::Note && d.tag == TASK_TAG) || d.kind == Kind::Preference)
}

/// The user's earlier requests and instructions most like `request`, in
/// this session or any other, for [`check`] to judge which it says again:
/// the user's prompts, notes for one task and rules, by each of the
/// vectors and BM25. Earlier means before the turn (`session`, `turn`):
/// nothing of the turn itself (its prompt, the preference picked from its
/// words, the notes an earlier distillation of it wrote: ANA-1, a rule said
/// once counted "2 times"), nor anything stored after its first record
/// (`from_seq`, when a turn is distilled again later).
pub fn said_before(store: &Store, request: &str, session: &str, turn: i64, from_seq: i64) -> Result<Vec<Record>> {
    let q: String = request.chars().take(1500).collect();
    if terms(&q).is_empty() {
        return Ok(Vec::new());
    }
    let v = store.embedder.embed(&[q.as_str()])?.pop().unwrap_or_default();
    let words = !chloedb::text::analyze_query(&q).is_empty();
    let mut found: BTreeMap<String, Record> = BTreeMap::new();
    let mut turns: BTreeSet<(String, i64)> = BTreeSet::new();
    let instructions = format!("((kind = 'note' AND tag = '{TASK_TAG}') OR (kind = 'preference' AND tag NOT IN ('superseded', 'retired')))");
    for (cond, n, keep) in [(instructions.as_str(), CANDIDATES, users_instruction as fn(&Record) -> bool), ("kind = 'prompt'", PROMPTS, users_request as fn(&Record) -> bool)] {
        // More than kept: some are another's words.
        let by_vec = format!("SELECT {COLUMNS}, score() FROM memory WHERE {cond} ORDER BY similarity(emb, :v) DESC LIMIT {}", n * 3);
        let by_words = format!("SELECT {COLUMNS}, score() FROM memory WHERE {cond} ORDER BY bm25(body, :q, syntax => 'plain') DESC LIMIT {}", n * 3);
        let mut lists = vec![store.query_with(&by_vec, &Params::new().set("v", Value::Vector(v.clone().into())))?];
        if words {
            lists.push(store.query_with(&by_words, &Params::new().set("q", Value::Str(q.clone())))?);
        }
        for rs in lists {
            let mut taken = 0;
            for r in rs.rows.iter().filter_map(|r| row_record(r)).filter(|r| keep(r)) {
                if taken == n {
                    break;
                }
                // Nothing of this turn or after it, and one chunk of a prompt.
                // The same request sent again word for word (a scripted
                // run, a question asked once more) is not a correction
                // (JONAH-10: "reply with just the word OK" became a rule).
                if (r.session == session && r.turn == turn) || r.seq >= from_seq || (r.kind == Kind::Prompt && (!turns.insert((r.session.clone(), r.turn)) || plain(&r.text) == plain(&q))) {
                    continue;
                }
                taken += 1;
                found.insert(r.id.clone(), r);
            }
        }
    }
    let mut out: Vec<Record> = found.into_values().collect();
    out.sort_by_key(|r| r.seq);
    Ok(out)
}

/// Whether a prompt is one the user wrote and says something: no other
/// agent's brief, no envelope, more than a word or two.
fn users_request(r: &Record) -> bool {
    users(r) && r.kind == Kind::Prompt && terms(&r.text).len() >= 2 && !crate::envelope::is_envelope(&r.text)
}

/// [`said_before`] as [`check`] is shown it: (id, "when, where: what"),
/// `session` being the one of the request.
pub fn listed(before: &[Record], session: &str) -> Vec<(String, String)> {
    before
        .iter()
        .map(|r| {
            let what = match r.kind {
                Kind::Prompt => "the user asked",
                Kind::Preference => "a rule of the user's",
                _ => "the user's instruction for one task",
            };
            let place = if r.session == session { "this session" } else { "another session" };
            (r.id.clone(), format!("{}, {place}, {what}: {}", day(r.ts), crate::llm::clip(instruction(&r.text), 400)))
        })
        .collect()
}

/// One thing a request says again: the ids of the earlier sayings
/// ([`said_before`]) it repeats, the request's words that say it again,
/// and the rule to keep from now on.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Again {
    pub ids: Vec<String>,
    pub quote: String,
    pub rule: String,
}

const CHECK_SYSTEM: &str = "You read what a user told AI coding agents before and the user's newest message, and say which of the earlier things the newest one says again. You output only JSON.";

const CHECK: &str = r#"A user works with AI coding agents. Below are things the user said to them before (said_before: each with its id, oldest first, from this session or others), the agent's last reply before the user's newest message (previous_reply, which may be cut), and that newest message (request).

<said_before>
{said}
</said_before>{prev}

<request>
{request}
</request>

Say which of said_before the request says again. The request says one again when the user has to ask again for what they asked before: the same thing in any words, shorter, longer or more specific, or a complaint or question that the agent still has not done it or did the opposite ("why does it still say Strata?", "these still look bad", "I told you to build it from the SQL"). An earlier saying can be a complaint or a question too: it counts when the request asks for the same thing.

A question that asks what holds ("what error code do we return when ...?", "when can I deploy to staging?", "should I upgrade X?") asks memory, not for something again, even when an earlier saying answers it: it is not said again, unless it complains that the agent did not do it ("why does it still ...?", "didn't I say ...?").

Words the user puts on each request the same way (a format pasted onto every task, such as "reply with the code only; do not edit any files") are part of each task, not said again. The request does not say one again when it only shares a subject or words with it, asks for something else about the same thing, asks for the opposite or changes its mind, or answers the agent's question. Judge the request's own words only: something in said_before or in previous_reply that the request does not ask for again is not said again.

For each thing the request says again, give:
- "ids": the ids of the earlier sayings it says again (each one that asks for the same thing);
- "quote": the words of the request that say it again, copied exactly from the request;
- "rule": what the agent should do from now on, as one imperative sentence in the user's terms and specific enough to act on (for example "Build each app's screens from CREATE SCREEN statements in SQL."), with no session, date or mention of this request.

Output one JSON object and nothing else: {"said_again": [{"ids": ["<id>"], "quote": "<words of the request>", "rule": "<sentence>"}]}. Most requests say nothing again; then output {"said_again": []}."#;

/// The prompt [`check`] sends.
pub fn check_prompt(request: &str, prev_reply: &str, said: &[(String, String)]) -> String {
    let list = said.iter().map(|(id, t)| format!("[{id}] {}", crate::llm::clip(t, 450))).collect::<Vec<_>>().join("\n");
    let prev = if prev_reply.trim().is_empty() {
        String::new()
    } else {
        format!("\n\n<previous_reply>\n{}\n</previous_reply>", crate::llm::clip(&crate::redact::redact(prev_reply.trim()), 1500))
    };
    CHECK.replace("{said}", &list).replace("{prev}", &prev).replace("{request}", &crate::llm::clip(&crate::redact::redact(request), 4000))
}

/// Lowercase, straight quotes, single spaces.
fn plain(t: &str) -> String {
    let t: String = t.chars().map(|c| match c {
        '\u{2018}' | '\u{2019}' => '\'',
        '\u{201c}' | '\u{201d}' => '"',
        c => c.to_ascii_lowercase(),
    }).collect();
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether `quote` is the request's own words: in it as written, or with
/// nearly all of its words in it.
fn quoted_from(quote: &str, request: &str) -> bool {
    let q = plain(quote);
    let q = q.trim_matches(|c: char| !c.is_alphanumeric());
    let qt = terms(q);
    if qt.is_empty() {
        return false;
    }
    if plain(request).contains(q) {
        return true;
    }
    let rt = terms(request);
    qt.iter().filter(|t| rt.contains(*t)).count() * 5 >= qt.len() * 4
}

/// Whether a quote is a question that asks what holds ("What error code do
/// we return when a client retries a payment?", "When can I deploy to
/// staging?", "Should I upgrade iso8601?"): asking memory, not saying it
/// again, though the model may take a rule that answers it for one said
/// again (ANA-203: three answered questions made rules "said in 2
/// sessions", and `repeats` reported failures that did not happen). A
/// question that complains it was not done ("why does it still say
/// Strata?", "didn't I tell you to use cents?") or asks for it politely
/// ("could you always run ruff?") says it again.
fn asks_what_holds(quote: &str, request: &str) -> bool {
    static ASK: OnceLock<Regex> = OnceLock::new();
    static POLITE: OnceLock<Regex> = OnceLock::new();
    static COMPLAINT: OnceLock<Regex> = OnceLock::new();
    let ask = ASK.get_or_init(|| Regex::new(r"(?i)^\W*(?:(?:and|so|ok|okay|also|quick question|one more thing)\W+)*(?:what|when|where|which|who|whose|how|is|are|was|were|do|does|did|can|could|should|shall|will|would|may|might|has|have|any)\b").unwrap());
    let polite = POLITE.get_or_init(|| Regex::new(r"(?i)^\W*(?:please\b|(?:can|could|would|will) you\b)").unwrap());
    let complaint = COMPLAINT.get_or_init(|| Regex::new(r"(?i)\b(?:still|again|yet|anymore|already|told you|i (?:said|asked|told|wanted|meant)|didn[’']?t i|did i not|forgot|forget|keeps?|kept|stop|instead|wrong|why\b.*(?:\bnot\b|n[’']t\b))").unwrap());
    let q = plain(quote);
    let q = q.trim();
    // The question mark ends the quote, or the request's sentence that holds it.
    let r = plain(request);
    let after = r.find(q.trim_end_matches('?')).map(|i| &r[i + q.trim_end_matches('?').len()..]).unwrap_or("");
    let ends = after.find(['.', '!', '?', '\n']).map(|i| after[i..].starts_with('?')).unwrap_or(false);
    (q.ends_with('?') || ends) && ask.is_match(q) && !polite.is_match(q) && !complaint.is_match(q)
}

/// The id of `said` a mark names: as listed, or in another style, as a
/// model copies one from the request ("cmsg_B3" for "B3": the one listed id
/// it ends with, after a separator).
fn listed_id(id: &str, said: &[(String, String)]) -> Option<String> {
    if said.iter().any(|(x, _)| x == id) {
        return Some(id.to_string());
    }
    let ends: Vec<&String> = said.iter().map(|(x, _)| x).filter(|x| id.len() > x.len() && id.ends_with(x.as_str()) && !id[..id.len() - x.len()].ends_with(|c: char| c.is_alphanumeric())).collect();
    (ends.len() == 1).then(|| ends[0].clone())
}

/// The marks of a [`check`] reply that name ids in `said` and quote the
/// request, and those dropped for naming none or quoting something else.
pub fn parse_check(text: &str, request: &str, said: &[(String, String)]) -> (Vec<Again>, Vec<Again>) {
    let (mut kept, mut dropped) = (Vec::new(), Vec::new());
    let Some(v) = crate::llm::json_object(text) else { return (kept, dropped) };
    for m in v["said_again"].as_array().into_iter().flatten() {
        let ids: Vec<String> = crate::distill::strings(m, "ids", 8, 80).into_iter().map(|i| i.trim_matches(|c| c == '[' || c == ']').trim().to_string()).collect();
        let a = Again {
            ids: ids.iter().filter_map(|i| listed_id(i, said)).collect(),
            quote: m["quote"].as_str().unwrap_or("").trim().to_string(),
            rule: m["rule"].as_str().unwrap_or("").trim().to_string(),
        };
        // How to do this turn ("no code changes", "just answer") is said
        // again with each turn that wants it, not because a rule failed
        // (JONAH-303).
        if a.ids.is_empty() || a.rule.is_empty() || !quoted_from(&a.quote, request) || asks_what_holds(&a.quote, request) || crate::capture::for_this_turn(&a.quote) {
            dropped.push(Again { ids, ..a });
        } else {
            kept.push(a);
        }
    }
    (kept, dropped)
}

/// Words of the request pasted again: a quote of six words or more that
/// each earlier prompt it names holds word for word, as a format put on
/// every task does (ANA-6: "reply with the code only; do not create or
/// edit any files", on each prompt, became a rule "said in 9 sessions").
/// A correction is said in other words, or as a complaint.
pub fn pasted(a: &Again, before: &[Record]) -> bool {
    let q = plain(&a.quote);
    let q = q.trim_matches(|c: char| !c.is_alphanumeric());
    if q.split_whitespace().count() < 6 {
        return false;
    }
    let said: Vec<&Record> = a.ids.iter().filter_map(|id| before.iter().find(|r| &r.id == id)).collect();
    !said.is_empty() && said.iter().all(|r| r.kind == Kind::Prompt && plain(&r.text).contains(q))
}

/// What a check found, kept and dropped, and what it cost.
#[derive(Debug, Default)]
pub struct Checked {
    pub again: Vec<Again>,
    pub dropped: Vec<Again>,
    pub cost_usd: f64,
}

/// Asks the distiller's model which of `said` ([`listed`]) the user's
/// `request` says again, `prev_reply` being the agent's reply before it.
pub fn check(cfg: &crate::distill::DistillConfig, request: &str, prev_reply: &str, said: &[(String, String)]) -> Result<Checked> {
    if said.is_empty() {
        return Ok(Checked::default());
    }
    let model = cfg.model();
    let r = crate::llm::complete(&cfg.cli(&model, cfg.timeout_secs), CHECK_SYSTEM, &check_prompt(request, prev_reply, said))?;
    let (again, dropped) = parse_check(&r.text, request, said);
    Ok(Checked { again, dropped, cost_usd: r.cost_usd })
}

/// What [`promote`] changed: the drafts to store, and for each rule it made,
/// the records that rule supersedes once stored.
#[derive(Debug, Default)]
pub struct Promoted {
    pub drafts: Vec<Draft>,
    /// The ids each promoted rule repeats (its `repeats` links): superseded
    /// by it when it is stored, if notes or rules.
    pub repeats: Vec<String>,
}

/// The drafts of a turn, with a rule for each thing [`check`] found the
/// user said again (`again`, of records in `before`): a rule of every
/// session, or the rule it repeats written anew. A draft of the user's
/// instruction that says the same (the distiller's note of it for this
/// task) gives the rule its situations and files and goes. `shown` is what
/// the session's context window held when the turn's prompt came.
pub fn promote(drafts: Vec<Draft>, again: &[Again], session: &str, shown: &HashSet<String>, before: &[Record]) -> Promoted {
    let mut out = Promoted::default();
    let mut drafts: Vec<Option<Draft>> = drafts.into_iter().map(Some).collect();
    let mut used: HashSet<&str> = HashSet::new();
    for a in again.iter().filter(|a| !pasted(a, before)) {
        let said: Vec<&Record> = a.ids.iter().filter(|id| used.insert(id.as_str())).filter_map(|id| before.iter().find(|r| &r.id == id)).collect();
        if said.is_empty() || a.rule.trim().is_empty() {
            continue;
        }
        let rules: Vec<&Record> = said.iter().copied().filter(|r| r.kind == Kind::Preference).collect();
        // A rule counted the sayings before it: only those since are new.
        let newest = rules.iter().map(|r| r.seq).max().unwrap_or(i64::MIN);
        let others: Vec<&Record> = said.iter().copied().filter(|r| r.kind != Kind::Preference && r.seq > newest).collect();
        // This session and the sessions of the sayings it repeats, added to
        // those a rule it repeats counted already.
        let mut sessions: BTreeSet<&str> = others.iter().map(|r| r.session.as_str()).collect();
        sessions.insert(session);
        let counted = rules.iter().map(|r| sessions_of(r) + sessions.iter().filter(|s| **s != r.session).count()).max().unwrap_or(0);
        let n = sessions.len().max(counted);
        // A prompt and the note written from it are one saying.
        let turns: BTreeSet<(&str, i64)> = others.iter().map(|r| (r.session.as_str(), r.turn)).collect();
        let times = rules.iter().map(|r| said_of(r)).max().unwrap_or(0) + turns.len() + 1;
        let mut ctx = format!("Sessions: {n}\nSaid: {times}");
        if !rules.is_empty() {
            // Shown in this window, or said in this session already, the
            // rule (or one it was written anew from) was the agent's to keep.
            let was_shown = rules.iter().any(|r| r.session == session || shown.contains(&r.id) || r.linked("repeats").any(|id| shown.contains(id)));
            let (n, after) = rules.iter().map(|r| said_again_of(r)).fold((0, 0), |a, b| (a.0.max(b.0), a.1.max(b.1)));
            ctx += &format!("\nSaid again: {}\nSaid again after it was shown: {}", n + 1, after + usize::from(was_shown));
        }
        let (mut when, mut files, mut entities) = (Vec::<String>::new(), Vec::<String>::new(), Vec::<String>::new());
        let add = |list: &mut Vec<String>, x: &str| {
            if !x.is_empty() && !list.iter().any(|y| y == x) {
                list.push(x.to_string());
            }
        };
        // The user's instruction this turn that says it: the rule now.
        let rt = terms(&a.rule);
        for slot in drafts.iter_mut() {
            let covered = slot.as_ref().is_some_and(|d| users_draft(d) && {
                let dt = terms(instruction(&d.text));
                let inter = dt.intersection(&rt).count();
                inter >= 2 && inter * 10 >= (dt.len() + rt.len() - inter) * 3
            });
            if covered {
                let d = slot.take().unwrap();
                d.when.iter().for_each(|w| add(&mut when, w));
                d.files.iter().for_each(|f| add(&mut files, f));
                d.entities.iter().for_each(|e| add(&mut entities, e));
            }
        }
        for r in &said {
            r.when().into_iter().flat_map(|w| w.split("; ").map(str::to_string).collect::<Vec<_>>()).for_each(|w| add(&mut when, &w));
            r.files.split_whitespace().for_each(|f| add(&mut files, f));
            r.linked("about").filter_map(|e| e.strip_prefix("e:")).for_each(|e| add(&mut entities, e));
        }
        when.truncate(8);
        let mut rule = Draft::new(Kind::Preference, rule_text(&a.rule, n, times)).tag(REPEATED_TAG).by(&Source::User).files(files).when(when).ctx(ctx);
        rule.entities = entities;
        for r in &said {
            rule = rule.link("repeats", r.id.clone());
            if r.kind != Kind::Prompt {
                out.repeats.push(r.id.clone());
            }
        }
        out.drafts.push(rule);
    }
    out.drafts.extend(drafts.into_iter().flatten());
    out
}

/// Asks [`check`] about each case and counts which earlier sayings it
/// marked as said again against the ones `expect` names: how well it tells
/// a repeat from the rest on real sayings, with nothing stored. A case is
/// `{"id", "request", "prev_reply"?, "before": [{"id", "text"}], "expect": [id]}`.
pub fn judge(cfg: &crate::distill::DistillConfig, cases: &[Json], parallel: usize) -> Json {
    let one = |c: &Json| -> Json {
        let before: Vec<(String, String)> = c["before"].as_array().into_iter().flatten().filter_map(|b| Some((b["id"].as_str()?.to_string(), b["text"].as_str()?.to_string()))).collect();
        let expect: BTreeSet<String> = c["expect"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect();
        let request = c["request"].as_str().unwrap_or("");
        match check(cfg, request, c["prev_reply"].as_str().unwrap_or(""), &before) {
            Ok(k) => {
                let marked: BTreeSet<String> = k.again.iter().flat_map(|a| a.ids.iter().cloned()).collect();
                let show = |v: &[Again]| v.iter().map(|a| json!({ "ids": a.ids, "quote": a.quote, "rule": a.rule })).collect::<Vec<_>>();
                json!({ "id": c["id"], "expect": expect, "marked": marked, "found": expect.intersection(&marked).count(), "false": marked.difference(&expect).count(),
                        "said_again": show(&k.again), "dropped": show(&k.dropped), "cost_usd": k.cost_usd })
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
    let positives = results.iter().filter(|r| r["expect"].as_array().is_some_and(|a| !a.is_empty()) && r.get("error").is_none()).count();
    let caught = results.iter().filter(|r| r["found"].as_u64().unwrap_or(0) > 0).count();
    let negatives = results.iter().filter(|r| r["expect"].as_array().is_some_and(|a| a.is_empty()) && r.get("error").is_none()).count();
    let marked_neg = results.iter().filter(|r| r["expect"].as_array().is_some_and(|a| a.is_empty()) && r["marked"].as_array().is_some_and(|a| !a.is_empty())).count();
    let dropped: usize = results.iter().map(|r| r["dropped"].as_array().map_or(0, |a| a.len())).sum();
    let cost: f64 = results.iter().map(|r| r["cost_usd"].as_f64().unwrap_or(0.0)).sum();
    let text = format!(
        "{} cases: {caught} of {positives} repeats caught, {} of {expected} earlier sayings found, {} marked that are not; {marked_neg} of {negatives} cases with no repeat marked one; {dropped} marks dropped for not quoting the request; {errors} failed; ${cost:.4}.\n",
        results.len(),
        sum("found"),
        sum("false")
    );
    json!({ "text": text, "cases": results.len(), "positives": positives, "caught": caught, "expected": expected, "found": sum("found"), "false": sum("false"), "negatives": negatives, "marked_negatives": marked_neg, "dropped": dropped, "errors": errors, "cost_usd": cost, "results": results })
}

/// After the drafts [`promote`] made are stored: each note or rule a stored
/// rule repeats is superseded by it (a prompt stays what was asked). The
/// number superseded.
pub fn supersede_repeated(store: &Store, stored: &[Record]) -> usize {
    let mut n = 0;
    for rule in stored.iter().filter(|r| r.kind == Kind::Preference && r.tag == REPEATED_TAG) {
        let ids: Vec<String> = rule.linked("repeats").map(str::to_string).collect();
        for r in store.get(&ids).unwrap_or_default().into_iter().filter(|r| matches!(r.kind, Kind::Note | Kind::Preference)) {
            if store.supersede(&r.id, &rule.id).unwrap_or(false) {
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

    /// What the check found said again of `ids`, as `rule`.
    fn again(rule: &str, ids: &[&str]) -> Again {
        Again { ids: ids.iter().map(|i| i.to_string()).collect(), quote: String::new(), rule: rule.into() }
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
    fn the_check_is_shown_the_users_own_earlier_requests_and_instructions() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        s.append("s1", 1, 100, vec![task("Build the shop from the SQL CREATE statements.")]).unwrap();
        s.append("s1", 2, 110, vec![Draft::new(Kind::Note, "For lead's task only (the agent lead said, not a standing rule): Build the shop from the CREATE statements.").tag(TASK_TAG).by(&Source::Agent("lead".into()))]).unwrap();
        s.append("s2", 1, 120, vec![Draft::new(Kind::Preference, "Always keep the shop's money as integer cents.").by(&Source::User)]).unwrap();
        s.append("s2", 2, 130, vec![Draft::new(Kind::Fact, "The shop's CREATE statements are in schema.sql.")]).unwrap();
        s.append("s2", 3, 140, vec![Draft::new(Kind::Prompt, "why is the shop still not built from the CREATE statements?")]).unwrap();
        s.append("s2", 4, 150, vec![Draft::new(Kind::Prompt, "Lead here: build the shop from the CREATE statements.").by(&Source::Agent("lead".into()))]).unwrap();
        s.append("s2", 5, 160, vec![Draft::new(Kind::Prompt, "ok")]).unwrap();
        // This turn's own prompt is stored before it is distilled.
        s.append("s3", 1, 200, vec![Draft::new(Kind::Prompt, "Build the shop from the CREATE statements, please")]).unwrap();
        let before = said_before(&s, "Build the shop from the CREATE statements, please", "s3", 1, i64::MAX).unwrap();
        let texts: Vec<&str> = before.iter().map(|r| instruction(&r.text)).collect();
        assert_eq!(
            texts,
            ["Build the shop from the SQL CREATE statements.", "Always keep the shop's money as integer cents.", "why is the shop still not built from the CREATE statements?"],
            "the user's notes for a task, rules and requests: not an agent's, nor facts, nor this turn's own"
        );
        let l = listed(&before, "s1");
        assert_eq!(l[0], (before[0].id.clone(), "1970-01-01, this session, the user's instruction for one task: Build the shop from the SQL CREATE statements.".to_string()));
        assert!(l[1].1.contains("another session, a rule of the user's: Always") && l[2].1.contains("another session, the user asked: why"), "{l:?}");
        let p = check_prompt("Build it from the CREATE statements!", "I built it from a JSON file.", &l);
        assert!(p.contains(&format!("[{}] 1970-01-01, this session", before[0].id)) && p.contains("<previous_reply>\nI built it from a JSON file.") && p.contains("<request>\nBuild it from the CREATE statements!"), "{p}");
    }

    /// ANA-1: a rule said once, in one turn, was counted "said 2 times":
    /// the preference picked from the prompt's own words, and the notes an
    /// earlier distillation of the turn wrote, were shown to the check as
    /// earlier sayings. Nothing of the turn, nor anything stored after it
    /// (a turn distilled again later), is an earlier saying.
    #[test]
    fn nothing_of_the_turn_itself_or_after_it_was_said_before() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        let said = "From now on always run ruff check before you tell me you are done.";
        let first = s
            .append("s1", 3, 100, vec![Draft::new(Kind::Prompt, said), Draft::new(Kind::Preference, format!("User preference: {said}")).tag("cue")])
            .unwrap()[0]
            .seq;
        // An earlier distillation of the same turn, and a later turn's.
        s.append("s1", 3, 101, vec![Draft::new(Kind::Preference, "Always run ruff check before saying the work is done.").tag("distilled").by(&Source::User)]).unwrap();
        s.append("s1", 4, 120, vec![Draft::new(Kind::Prompt, "remember: run ruff check before you say you are done")]).unwrap();
        assert!(said_before(&s, said, "s1", 3, first).unwrap().is_empty());
        // Live, the turn's first record is the newest of the store's turns:
        // the same turn is still left out by its session and number.
        let ids: Vec<i64> = said_before(&s, said, "s1", 3, i64::MAX).unwrap().iter().map(|r| r.turn).collect();
        assert_eq!(ids, [4], "only another turn's saying");
    }

    /// JONAH-10 and ANA-6: the same request sent again word for word, and
    /// a format pasted onto each task, are not said again.
    #[test]
    fn a_request_sent_again_or_a_pasted_format_is_not_said_again() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        s.append("s1", 1, 100, vec![Draft::new(Kind::Prompt, "Reply with just the word OK.")]).unwrap();
        let fmt = "Add an invoice total. Reply with the code only; do not create or edit any files.";
        let p = s.append("s2", 1, 110, vec![Draft::new(Kind::Prompt, fmt)]).unwrap().remove(0);
        assert!(said_before(&s, "Reply with just the word OK.", "s3", 1, i64::MAX).unwrap().iter().all(|r| !r.text.contains("word OK")));
        let request = "Round VAT per invoice line. Reply with the code only; do not create or edit any files.";
        let before = said_before(&s, request, "s3", 1, i64::MAX).unwrap();
        assert!(before.iter().any(|r| r.id == p.id), "a different request is shown");
        let a = Again { ids: vec![p.id.clone()], quote: "Reply with the code only; do not create or edit any files".into(), rule: "Reply with code only and never edit files.".into() };
        assert!(pasted(&a, &before));
        assert!(promote(Vec::new(), &[a.clone()], "s3", &HashSet::new(), &before).drafts.is_empty(), "no rule");
        // Said in other words, or short: a correction.
        assert!(!pasted(&Again { quote: "only the code, please, and leave the files alone".into(), ..a.clone() }, &before));
        assert!(!pasted(&Again { quote: "do not edit any files".into(), ..a }, &before));
    }

    #[test]
    fn a_mark_counts_only_when_it_quotes_the_request_and_names_what_was_shown() {
        let said = vec![("a:1".to_string(), "Build it from the SQL.".to_string()), ("a:2".to_string(), "Use tabs.".to_string())];
        let request = "Why is it STILL reading the JSON file?  I said: build it from the SQL.";
        let reply = json!({"said_again": [
            {"ids": ["[a:1]", "zz:9"], "quote": "I said: build it from the SQL", "rule": "Build the app from its SQL."},
            {"ids": ["a:1"], "quote": "why is it still reading the json file", "rule": "Read the app from its SQL, not the JSON file."},
            {"ids": ["a:2"], "quote": "Use tabs", "rule": "Indent with tabs."},
            {"ids": ["zz:9"], "quote": "build it from the SQL", "rule": "Build it from the SQL."},
            {"ids": ["a:1"], "quote": "reading the file JSON still", "rule": "Read it from SQL."},
            {"ids": ["cmsg_a:1", "xa:2"], "quote": "build it from the SQL", "rule": "Build from SQL."}
        ]});
        let (kept, dropped) = parse_check(&format!("Here: {reply}"), request, &said);
        assert_eq!(kept.iter().map(|a| a.rule.as_str()).collect::<Vec<_>>(), ["Build the app from its SQL.", "Read the app from its SQL, not the JSON file.", "Read it from SQL.", "Build from SQL."], "as written, in other case and spacing, or its words in other order");
        assert_eq!(kept[0].ids, ["a:1"], "an id it was not shown goes");
        assert_eq!(kept[3].ids, ["a:1"], "an id in another style is the one it ends with, after a separator");
        assert_eq!(dropped.len(), 2, "copied from what it was shown, or naming nothing it was shown: {dropped:?}");
        assert_eq!(parse_check("not json", request, &said), (vec![], vec![]));
    }

    /// ANA-203 and JONAH-204: a question that asks what holds is not the
    /// user saying a rule again, though the model may mark it (haiku marked
    /// Ana's first question so); a question that complains, or asks for it
    /// politely, says it again.
    #[test]
    fn an_answered_question_is_not_said_again() {
        let said = vec![("k:3".to_string(), "Return error code LDG-0007 when a client retries a payment with a seen idempotency key.".to_string())];
        let mark = |request: &str, quote: &str| {
            let reply = json!({"said_again": [{"ids": ["k:3"], "quote": quote, "rule": "Return error code LDG-0007 for a retried idempotency key."}]});
            parse_check(&reply.to_string(), request, &said).0.len()
        };
        let q = "What error code do we return when a client retries a payment with a duplicate idempotency key?";
        assert_eq!(mark(q, q), 0, "haiku's mark on Ana's question");
        assert_eq!(mark("When can I deploy to staging?", "When can I deploy to staging?"), 0);
        assert_eq!(mark("Should I upgrade the iso8601 library to the latest version?", "Should I upgrade the iso8601 library"), 0);
        assert_eq!(mark("What error code do we return for a retried key? Just checking.", "What error code do we return for a retried key"), 0);
        assert_eq!(mark("Is recurring todos a good next feature?", "Is recurring todos a good next feature?"), 0, "JONAH-204");
        // Complaints and requests are sayings again.
        assert_eq!(mark("Why does it still return 409? It should be LDG-0007.", "Why does it still return 409?"), 1);
        assert_eq!(mark("Didn't I tell you to return LDG-0007 for those?", "Didn't I tell you to return LDG-0007"), 1);
        assert_eq!(mark("Why doesn't the retry return LDG-0007?", "Why doesn't the retry return LDG-0007?"), 1);
        assert_eq!(mark("Could you always return LDG-0007 for a retried key?", "Could you always return LDG-0007 for a retried key?"), 1);
        assert_eq!(mark("Return LDG-0007 for a retried key, as I said.", "Return LDG-0007 for a retried key"), 1);
    }

    #[test]
    fn an_instruction_said_again_becomes_a_rule_counted_in_times_and_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        let n1 = s.append("s1", 1, 100, vec![task("Build the app from the SQL CREATE statements.")]).unwrap().remove(0);
        let lead = s.append("s1", 2, 105, vec![Draft::new(Kind::Note, "For lead's task only (the agent lead said, not a standing rule): Use the red theme.").tag(TASK_TAG).by(&Source::Agent("lead".into()))]).unwrap().remove(0);
        let before = vec![n1.clone()];

        // Nothing said again, or a mark of a record that was not shown:
        // nothing changes.
        let p = promote(vec![task("Rename the cart module.")], &[again("Use the red theme.", &[&lead.id])], "s1", &HashSet::new(), &before);
        assert!(p.repeats.is_empty() && p.drafts.len() == 1 && p.drafts[0].kind == Kind::Note, "{p:?}");

        // The user says it again later in s1, in other words: a rule, said
        // twice, and the distiller's note of it for this task is the rule.
        let note = task("Build the app from the CREATE statements in schema.sql.").when(vec!["building the app".into()]).files(vec!["schema.sql".into()]);
        let p = promote(vec![note, Draft::new(Kind::Fact, "The cart is in cart.py")], &[again("Build the app from the CREATE statements in schema.sql.", &[&n1.id])], "s1", &HashSet::new(), &before);
        assert_eq!(p.drafts.len(), 2, "{p:?}");
        let rule = &p.drafts[0];
        assert_eq!((rule.kind, rule.tag.as_str()), (Kind::Preference, REPEATED_TAG));
        assert_eq!(rule.text, "Build the app from the CREATE statements in schema.sql (the user has said this 2 times)");
        assert_eq!((rule.when.clone(), rule.files.clone()), (vec!["building the app".to_string()], vec!["schema.sql".to_string()]));
        assert_eq!(p.drafts[1].kind, Kind::Fact);
        assert_eq!(p.repeats, vec![n1.id.clone()]);
        let r2 = s.append("s1", 3, 130, p.drafts).unwrap().remove(0);
        assert_eq!(supersede_repeated(&s, std::slice::from_ref(&r2)), 1);
        assert_eq!(s.get(std::slice::from_ref(&n1.id)).unwrap()[0].tag, "superseded");
        assert_eq!((sessions_of(&r2), said_of(&r2), said_again_of(&r2)), (1, 2, (0, 0)), "{}", r2.ctx);

        // In another session, where it was said as a complaint once already
        // (a prompt, which stays as it was): 2 sessions, 4 times, said again
        // once kept.
        let asked = s.append("s2", 1, 135, vec![Draft::new(Kind::Prompt, "why is the app not built from the SQL?")]).unwrap().remove(0);
        let p = promote(vec![], &[again("Build apps from SQL and CREATE statements.", &[&r2.id, &asked.id])], "s2", &HashSet::new(), &[r2.clone(), asked.clone()]);
        assert_eq!(p.drafts[0].text, "Build apps from SQL and CREATE statements (the user has said this in 2 sessions)");
        assert_eq!(p.repeats, vec![r2.id.clone()]);
        let r3 = s.append("s2", 2, 140, p.drafts).unwrap().remove(0);
        assert_eq!(supersede_repeated(&s, std::slice::from_ref(&r3)), 1);
        assert_eq!(s.get(std::slice::from_ref(&asked.id)).unwrap()[0].tag, asked.tag, "a prompt is not superseded");
        assert_eq!((sessions_of(&r3), said_of(&r3), said_again_of(&r3)), (2, 4, (1, 0)), "{}", r3.ctx);
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
        let said = task("Keep money as integer cents, not floats.").when(vec!["computing a cart total".into()]).files(vec!["cart.py".into()]);
        let p = promote(vec![said], &[again("Keep money as integer cents, not floats.", &[&r1.id])], "s2", &shown, std::slice::from_ref(&r1));
        assert_eq!(p.drafts.len(), 1);
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
        let p = promote(vec![], &[again("Keep money in integer cents.", &[&r2.id])], "s3", &HashSet::new(), std::slice::from_ref(&r2));
        let r3 = s.append("s3", 2, 300, p.drafts).unwrap().remove(0);
        supersede_repeated(&s, std::slice::from_ref(&r3));
        assert_eq!((sessions_of(&r3), said_again_of(&r3)), (3, (2, 1)), "{}", r3.ctx);

        // Said again later in s3, which wrote it: the agent had it, and no
        // session more is counted.
        let p = promote(vec![], &[again("Keep money as integer cents!", &[&r3.id])], "s3", &HashSet::new(), std::slice::from_ref(&r3));
        let r4 = s.append("s3", 5, 310, p.drafts).unwrap().remove(0);
        supersede_repeated(&s, std::slice::from_ref(&r4));
        assert_eq!((sessions_of(&r4), said_of(&r4), said_again_of(&r4)), (3, 4, (3, 2)), "{}", r4.ctx);
        assert_eq!(live_rules(&s).len(), 1);

        let r = scan(&s, 0, 1000).unwrap();
        assert_eq!((r["rules"][0]["said_again"].as_u64(), r["rules"][0]["said_again_after_shown"].as_u64(), r["rules"][0]["times"].as_u64()), (Some(3), Some(2), Some(4)));
        assert!(r["text"].as_str().unwrap().contains("(2 of 3)"), "{}", r["text"]);
    }

    #[test]
    fn the_judge_counts_the_repeats_the_check_marks_against_the_expected_ones() {
        let dir = tempfile::tempdir().unwrap();
        // A model that marks the first saying it was shown, quoting
        // "from the CREATE statements".
        let out = dir.path().join("out.json");
        let reply = json!({"said_again": [{"ids": ["__ID__"], "quote": "from the CREATE statements", "rule": "Build it from the CREATE statements."}]}).to_string();
        std::fs::write(&out, json!({"is_error": false, "total_cost_usd": 0.001, "result": reply}).to_string()).unwrap();
        let script = dir.path().join("model.sh");
        std::fs::write(&script, format!("#!/bin/sh\np=$(cat)\nid=$(printf '%s\\n' \"$p\" | sed -n '/<said_before>/,/<\\/said_before>/p' | sed -n 's/^\\[\\([^]]*\\)\\].*/\\1/p' | head -1)\nsed \"s/__ID__/$id/\" '{}'\n", out.display())).unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let cfg = crate::distill::DistillConfig { command: script.to_string_lossy().into(), model: "m".into(), ..Default::default() };
        let cases = vec![
            json!({"id": "B9", "request": "No, from the CREATE statements.", "before": [{"id": "A1", "text": "Build it from the SQL schema."}], "expect": ["A1"]}),
            // Quotes what the request does not say: dropped.
            json!({"id": "X", "request": "Rename the cart.", "before": [{"id": "A2", "text": "Use tabs."}], "expect": []}),
            json!({"id": "Z", "request": "Build the cart from the CREATE statements.", "before": [{"id": "A3", "text": "Use tabs."}], "expect": []}),
            json!({"id": "Y", "request": "Build the shop.", "before": [], "expect": []}),
        ];
        let r = judge(&cfg, &cases, 2);
        assert_eq!((r["caught"].as_u64(), r["positives"].as_u64(), r["found"].as_u64(), r["expected"].as_u64(), r["false"].as_u64()), (Some(1), Some(1), Some(1), Some(1), Some(1)), "{r}");
        assert_eq!((r["marked_negatives"].as_u64(), r["negatives"].as_u64(), r["dropped"].as_u64(), r["errors"].as_u64()), (Some(1), Some(3), Some(1), Some(0)), "{r}");
        assert_eq!(r["results"][0]["marked"], json!(["A1"]));
        assert_eq!(r["results"][0]["said_again"][0]["rule"], "Build it from the CREATE statements.");
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
