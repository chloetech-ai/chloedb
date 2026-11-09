//! Knowledge a later session will need, written down by a small model after
//! each turn: decisions and their reasons, rules and conventions,
//! requirements for planned work, and facts about the project and its
//! environment. Much of this is said only in conversation and never reaches
//! the code, which is where a fresh session would look for it.
//!
//! The model is called through the `claude` CLI in print mode, without
//! tools, from a scratch directory and with Engram disabled, so the call
//! records nothing itself. By default it also skips the user's settings and
//! MCP servers: their plugins, hooks and tool definitions would add tens of
//! thousands of tokens to every call (and another memory plugin would record
//! it). It runs in the daemon after the hook has answered, so it never
//! delays a prompt.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::llm::{self, clip, Cli};
use crate::record::{Draft, Kind};
use crate::redact::redact;
use crate::Result;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DistillConfig {
    pub enabled: bool,
    /// The CLI to call (`claude`).
    pub command: String,
    /// Its `--model` argument; a small, fast model is enough. Empty: the
    /// CLI's default. `ENGRAM_DISTILL_MODEL` overrides it.
    pub model: String,
    pub timeout_secs: u64,
    /// Run without the user's settings and MCP servers
    /// (`--setting-sources project --strict-mcp-config`). Turn off when the
    /// CLI's credentials come from the user settings (`apiKeyHelper`, `env`).
    pub isolated: bool,
    /// After a turn's notes are written, ask once more whether they replace
    /// or contradict earlier notes about the same things that the model
    /// was not shown (only when there are such notes). `ENGRAM_RECONCILE=0`
    /// turns it off.
    pub reconcile: bool,
}

impl Default for DistillConfig {
    fn default() -> Self {
        DistillConfig { enabled: true, command: "claude".into(), model: String::new(), timeout_secs: 120, isolated: true, reconcile: true }
    }
}

impl DistillConfig {
    /// Enabled, and not switched off with `ENGRAM_DISTILL=0`.
    pub fn active(&self) -> bool {
        self.enabled && std::env::var("ENGRAM_DISTILL").map_or(true, |v| v != "0")
    }

    /// Reconciling on, and not switched off with `ENGRAM_RECONCILE=0`.
    pub fn reconciles(&self) -> bool {
        self.reconcile && std::env::var("ENGRAM_RECONCILE").map_or(true, |v| v != "0")
    }

    /// The model to ask for, if any.
    pub fn model(&self) -> String {
        std::env::var("ENGRAM_DISTILL_MODEL").ok().filter(|m| !m.is_empty()).unwrap_or_else(|| self.model.clone())
    }

    /// The CLI call for `model` with this configuration's command and isolation.
    pub fn cli<'a>(&'a self, model: &'a str, timeout_secs: u64) -> Cli<'a> {
        Cli { command: &self.command, model, timeout: Duration::from_secs(timeout_secs), isolated: self.isolated }
    }
}

const SYSTEM: &str = "You keep the long-term notes of a software project. You read one exchange between a developer and a coding assistant and write down what a future session will need. Output JSON lines only.";

const PROMPT: &str = r#"Here is one exchange from a software project.{when}

<request>
{request}
</request>

<commands_run>
{commands}
</commands_run>

<assistant_reply>
{answer}
</assistant_reply>

<already_noted>
{known}
</already_noted>

Write down what someone working on this project in a later session would need to know and could not easily see in the code:
- decisions, with the reason given;
- rules, constraints and conventions the user, the team or a client set (never the assistant: only they can make a project rule): what must or must not be done, formats, naming, processes. Keep every rule stated, with its reason, even if the current code already follows it: the code shows what was done, not that it must stay that way;
- requirements for work the user announced or planned but that is not done yet, with every detail given (formats, names, numbers);
- facts about the project or its environment that were stated or discovered (how to run or test it, quirks, people, external systems), and approaches that were tried and failed, with why.

Each item is one self-contained sentence that names its subject and keeps the exact details and the reason. One item per distinct point; a planned piece of work with all its requirements is one item. With each item list the entities it is about, so that notes about the same thing can be found together: the components, features, files, data formats, external systems and people it concerns (short names, e.g. "count sheet", "CSV export", "items.py", "Petra Keller").

Also list, for each item, 2 to 4 situations in which a later session must recall it, phrased the way a later request would be phrased, including indirect ones: what kind of task, choice or question it constrains, in words that need not appear in the item itself. For example, "the client's office PCs run Python 3.8" matters "when choosing a library or standard-library module (tomllib needs 3.11)", "when parsing a config file", "when using newer syntax such as match statements".

The already-noted items above are the notes most related to this exchange (not all notes). When this exchange changes, reverses or extends one of them, write the new version with "replaces": the new sentence must stand on its own (repeat what still holds) and may say what changed (e.g. "30 days, changed from 14"). When a noted item no longer holds at all (a requirement dropped, a rule lifted, a tool no longer used), write a fact that says so and why, with "replaces" naming it. When the exchange says from when a change holds ("from Monday on", "since release 2.3", "as of March 1"), add "since" with that date as YYYY-MM-DD. A later statement that gives a different value for something noted (a date, number, amount, name, version, choice or plan) is an update even when nobody says that it changed: write it with "replaces". Two things are not updates. A value for a different occasion, period or part (another event's budget, one week's pace against the standing weekly goal) is a separate fact. A statement that goes back to a value the noted item had replaced, with nothing saying it changed back (the goal went from 10 books to 12, and now the user mentions a goal of 10), may be a slip: write it with "conflicts" naming that item, so both stay visible. Use "conflicts" otherwise only when the two cannot both be true and the new one cannot be an update: one says something never happened, was never done or does not exist, and the other says it did (e.g. "I have never used Docker" against a noted "the user set up Docker for the API"). Write each sentence as it was said in this exchange; never write a disagreement or "it is unclear" into the sentence itself.

Leave out:
- anything already noted above, unless this exchange changes it;
- status that will soon be stale (test counts and results, what was committed or pushed and to which branch);
- the assistant's own habits and conventions, even when it says it always follows them ("as usual I'll ...": commit message style, formatting, tool usage), and its suggestions the user did not take up;
- questions the assistant asked and assumptions it made that the user has not answered or confirmed: they are not requirements, and never write one up as something that must be settled before the work can start;
- details of the code that reading it shows (signatures, what a function does).

Output one JSON object per line and nothing else:
{"kind": "decision" | "rule" | "requirement" | "fact", "text": "<sentence>", "entities": ["<entity>", ...], "when": ["<situation>", ...]}
{"kind": "decision" | "rule" | "requirement" | "fact", "text": "<new sentence>", "entities": ["<entity>", ...], "when": ["<situation>", ...], "replaces": "<id of the noted item it updates>", "since": "<YYYY-MM-DD, only if the exchange says>"}
{"kind": "decision" | "rule" | "requirement" | "fact", "text": "<sentence>", "entities": ["<entity>", ...], "when": ["<situation>", ...], "conflicts": "<id of the noted item it disagrees with>"}
{"kind": "done", "id": "<id of a noted requirement that this exchange completed>"}
If there is nothing worth keeping, output nothing."#;

/// What one exchange yields: new knowledge, noted items it replaces, and
/// planned work it completed.
#[derive(Debug, Default)]
pub struct Distilled {
    pub drafts: Vec<Draft>,
    /// Ids the drafts replace (each draft links `supersedes` to its own).
    pub replaced: Vec<String>,
    pub done: Vec<String>,
    pub cost_usd: f64,
    pub usage: llm::Usage,
}

/// The label a noted record carries in the prompt.
pub fn label(kind: Kind) -> &'static str {
    match kind {
        Kind::Todo => "requirement",
        Kind::Preference => "rule",
        Kind::Decision => "decision",
        _ => "fact",
    }
}

/// `2026-11-07` for a Unix time.
pub fn fmt_date(ts: i64) -> String {
    crate::recall::fmt_time(ts).chars().take(10).collect()
}

/// A date the model wrote (`2026-11-07`, or a date and time) as Unix
/// seconds.
pub fn parse_date(s: &str) -> Option<i64> {
    strata::timestamp::parse(s.trim()).map(|us| us.div_euclid(1_000_000)).filter(|&t| t > 0)
}

/// The prompt for one exchange; `known` lists (id, kind, text) of what is
/// already noted, and `ts` is when the exchange took place (0: unknown).
pub fn prompt_for(request: &str, answer: &str, commands: &[String], known: &[(String, Kind, String)], ts: i64) -> String {
    let known = if known.is_empty() { "(nothing yet)".to_string() } else { known.iter().map(|(id, k, t)| format!("[{id}] {}: {}", label(*k), clip(t, 300))).collect::<Vec<_>>().join("\n") };
    let commands = if commands.is_empty() { "(none)".to_string() } else { commands.join("\n") };
    // The day of the exchange, which is not always today: distillation can
    // run later (a queue, an import), and a relative date ("tomorrow") is
    // relative to when it was said.
    let when = if ts > 0 {
        let day: String = crate::recall::fmt_time(ts).chars().take(10).collect();
        format!(" It took place on {day}: a date it gives relative to that day (\"tomorrow\", \"next Tuesday\") is counted from {day}, and nothing in it happened on any other day unless it says so.")
    } else {
        String::new()
    };
    PROMPT
        .replace("{when}", &when)
        .replace("{request}", &clip(&redact(request), 6000))
        .replace("{commands}", &clip(&redact(&commands), 1500))
        .replace("{answer}", &clip(&redact(answer), 4000))
        .replace("{known}", &known)
}

/// Up to `n` non-empty strings (each at most `max` bytes) of an array field.
pub fn strings(v: &Json, key: &str, n: usize, max: usize) -> Vec<String> {
    v.get(key).and_then(|e| e.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).map(|x| clip(x.trim(), max)).filter(|x| !x.is_empty()).take(n).collect()).unwrap_or_default()
}

/// Parses the model's JSON lines; anything else is ignored.
pub fn parse(text: &str) -> Distilled {
    let mut out = Distilled::default();
    let id_of = |v: &Json, key: &str| v.get(key).and_then(|i| i.as_str()).map(|i| i.trim().trim_matches(|c| c == '[' || c == ']').to_string()).filter(|i| !i.is_empty());
    for line in text.lines() {
        let line = line.trim().trim_end_matches(',');
        if !line.starts_with('{') {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Json>(line) else { continue };
        let kind = v.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        if kind == "done" {
            out.done.extend(id_of(&v, "id"));
            continue;
        }
        let text = v.get("text").and_then(|t| t.as_str()).unwrap_or("").trim();
        if text.len() < 12 {
            continue;
        }
        let (k, tag) = match kind {
            "decision" => (Kind::Decision, "distilled"),
            "rule" => (Kind::Preference, "distilled"),
            "requirement" => (Kind::Todo, "planned"),
            "fact" => (Kind::Fact, "distilled"),
            _ => continue,
        };
        let replaces = id_of(&v, "replaces");
        let conflicts = id_of(&v, "conflicts").filter(|c| Some(c) != replaces.as_ref());
        let entities = strings(&v, "entities", 6, 60);
        let when = strings(&v, "when", 4, 160);
        let since = v.get("since").and_then(|s| s.as_str()).and_then(parse_date);
        let mut d = Draft::new(k, clip(&redact(text), 600)).tag(tag).files(crate::capture::paths_in(text)).entities(entities).when(when).since(since);
        if let Some(r) = &replaces {
            d = d.link("supersedes", r.clone());
        }
        if let Some(c) = &conflicts {
            d = d.link("conflicts", c.clone());
        }
        out.replaced.extend(replaces);
        out.drafts.push(d);
    }
    out
}

/// Runs the model on one exchange, which took place at `ts`.
pub fn distill(cfg: &DistillConfig, request: &str, answer: &str, commands: &[String], known: &[(String, Kind, String)], ts: i64) -> Result<Distilled> {
    let prompt = prompt_for(request, answer, commands, known, ts);
    let model = cfg.model();
    let r = llm::complete(&cfg.cli(&model, cfg.timeout_secs), SYSTEM, &prompt)?;
    let mut d = parse(&r.text);
    d.cost_usd = r.cost_usd;
    d.usage = r.usage;
    Ok(d)
}

const RECONCILE_SYSTEM: &str = "You keep the long-term notes of a software project up to date. Output JSON lines only.";

const RECONCILE_PROMPT: &str = r#"New notes were just written down from a conversation in a software project:

<new_notes>
{new}
</new_notes>

These earlier notes are about the same things and still count as current:

<earlier_notes>
{old}
</earlier_notes>

For each new note, say which earlier notes it changes:
- "replaces": earlier notes it is a later version of: the same rule, decision, requirement, plan or fact with a new value (a different date, number, amount, name or choice is a new value even when nothing says it changed, unless it is for a different occasion, period or part), or one it says no longer holds;
- "conflicts": earlier notes it contradicts in a way an update cannot explain: they cannot both be true (one says something never happened, was never done or does not exist, the other that it did), or it goes back to a value an earlier note had replaced without saying it changed back.
An earlier note on a different point of the same subject is neither: two notes about the CSV export, one on its separator and one on its schedule, both stay. When in doubt, it is neither.

Output one JSON object per line, only for new notes that replace or conflict with something:
{"note": "<new note id>", "replaces": ["<earlier id>", ...], "conflicts": ["<earlier id>", ...]}"#;

/// What [`reconcile`] found: (new note, earlier notes it replaces, earlier
/// notes it conflicts with).
pub type Reconciled = Vec<(String, Vec<String>, Vec<String>)>;

/// The prompt that asks whether new notes replace earlier ones the
/// distiller was not shown. Both lists hold (id, kind, text, written).
pub fn reconcile_prompt(new: &[(String, Kind, String, i64)], old: &[(String, Kind, String, i64)]) -> String {
    let list = |xs: &[(String, Kind, String, i64)]| xs.iter().map(|(id, k, t, ts)| format!("[{id}] {} ({}): {}", label(*k), fmt_date(*ts), clip(t, 400))).collect::<Vec<_>>().join("\n");
    RECONCILE_PROMPT.replace("{new}", &list(new)).replace("{old}", &list(old))
}

/// Parses [`reconcile`]'s JSON lines, keeping only ids it was given.
pub fn parse_reconcile(text: &str, new: &[String], old: &[String]) -> Reconciled {
    let ids = |v: &Json, key: &str| -> Vec<String> {
        let xs: Vec<String> = match v.get(key) {
            Some(Json::String(s)) => vec![s.clone()],
            Some(Json::Array(a)) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
            _ => Vec::new(),
        };
        xs.into_iter().map(|s| s.trim().trim_matches(|c| c == '[' || c == ']').to_string()).filter(|s| old.contains(s)).collect()
    };
    let mut out: Reconciled = Vec::new();
    for line in text.lines() {
        let line = line.trim().trim_end_matches(',');
        let Ok(v) = serde_json::from_str::<Json>(line) else { continue };
        let Some(note) = v.get("note").and_then(|n| n.as_str()).map(|n| n.trim().trim_matches(|c| c == '[' || c == ']').to_string()) else { continue };
        if !new.contains(&note) {
            continue;
        }
        let replaces = ids(&v, "replaces");
        let conflicts: Vec<String> = ids(&v, "conflicts").into_iter().filter(|c| !replaces.contains(c)).collect();
        if !replaces.is_empty() || !conflicts.is_empty() {
            out.push((note, replaces, conflicts));
        }
    }
    out
}

/// Asks the model whether new notes replace or contradict earlier notes
/// about the same things that the distiller was not shown.
pub fn reconcile(cfg: &DistillConfig, new: &[(String, Kind, String, i64)], old: &[(String, Kind, String, i64)]) -> Result<(Reconciled, llm::Reply)> {
    let model = cfg.model();
    let r = llm::complete(&cfg.cli(&model, cfg.timeout_secs), RECONCILE_SYSTEM, &reconcile_prompt(new, old))?;
    let ids = |xs: &[(String, Kind, String, i64)]| xs.iter().map(|x| x.0.clone()).collect::<Vec<_>>();
    Ok((parse_reconcile(&r.text, &ids(new), &ids(old)), r))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconcile_keeps_known_ids() {
        let text = "{\"note\": \"s:9\", \"replaces\": [\"s:2\", \"zz:1\"], \"conflicts\": \"[s:3]\"}\n{\"note\": \"s:10\", \"replaces\": []}\n{\"note\": \"x:1\", \"replaces\": [\"s:2\"]}";
        let r = parse_reconcile(text, &["s:9".into(), "s:10".into()], &["s:2".into(), "s:3".into()]);
        assert_eq!(r, vec![("s:9".to_string(), vec!["s:2".to_string()], vec!["s:3".to_string()])]);
        let p = reconcile_prompt(&[("s:9".into(), Kind::Preference, "Refunds within 30 days".into(), 1_791_788_086)], &[("s:2".into(), Kind::Preference, "Refunds within 14 days".into(), 1_790_000_000)]);
        assert!(p.contains("[s:9] rule (2026-10-12): Refunds within 30 days") && p.contains("[s:2] rule"), "{p}");
    }

    #[test]
    fn dates_and_conflicts_are_parsed() {
        assert_eq!(parse_date("2026-10-12"), Some(1_791_763_200));
        assert_eq!(parse_date("next Monday"), None);
        let d = parse("{\"kind\": \"rule\", \"text\": \"Refunds are allowed within 30 days of purchase.\", \"replaces\": \"a:1\", \"since\": \"2026-10-12\"}\n{\"kind\": \"fact\", \"text\": \"The user says the office has 12 desks.\", \"conflicts\": \"a:4\"}");
        assert_eq!(d.drafts[0].since, Some(1_791_763_200));
        assert_eq!(d.drafts[1].links, vec![crate::record::Link::new("conflicts", "a:4")]);
        assert_eq!(d.replaced, vec!["a:1".to_string()]);
        let p = prompt_for("x", "y", &[], &[], 1_791_788_086);
        assert!(p.contains("It took place on 2026-10-12:"), "{p}");
    }

    #[test]
    fn parses_items_and_ignores_the_rest() {
        let text = "Here you go:\n{\"kind\": \"decision\", \"text\": \"Storage stays JSON because admins edit the files by hand.\", \"entities\": [\"storage\", \"JSON files\"], \"when\": [\"making saving faster\"]}\n\
                    {\"kind\": \"requirement\", \"text\": \"The CSV export uses semicolons and DD.MM.YYYY dates.\", \"replaces\": \"abc:3\"},\n\
                    {\"kind\": \"done\", \"id\": \"[abc:12]\"}\n{\"kind\": \"mood\", \"text\": \"The user seems happy today.\"}\nnot json";
        let d = parse(text);
        assert_eq!(d.drafts.len(), 2);
        assert_eq!(d.drafts[0].kind, Kind::Decision);
        assert_eq!(d.drafts[0].entities, vec!["storage".to_string(), "JSON files".to_string()]);
        assert_eq!(d.drafts[0].when, vec!["making saving faster".to_string()]);
        assert_eq!(d.drafts[1].links, vec![crate::record::Link::new("supersedes", "abc:3")]);
        assert_eq!(d.drafts[1].kind, Kind::Todo);
        assert_eq!(d.drafts[1].tag, "planned");
        assert_eq!(d.replaced, vec!["abc:3".to_string()]);
        assert_eq!(d.done, vec!["abc:12".to_string()]);
    }

    #[test]
    fn prompt_carries_the_exchange() {
        let p = prompt_for("Add X. The client wants Y.", "Added X.", &["make test".into()], &[("s:1".into(), Kind::Todo, "Build Z".into())], 0);
        assert!(p.contains("The client wants Y.") && p.contains("[s:1] requirement: Build Z") && p.contains("make test"));
        assert!(!p.contains("took place"));
        // 2024-03-15 09:00 UTC: the day it was said, for relative dates.
        let p = prompt_for("Ship it tomorrow.", "OK.", &[], &[], 1_710_493_200);
        assert!(p.contains("It took place on 2024-03-15"), "{p}");
    }
}
