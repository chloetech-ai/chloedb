//! Knowledge a later session will need, written down by a small model after
//! each turn: decisions and their reasons, rules and conventions,
//! requirements for planned work, and facts about the project and its
//! environment. Much of this is said only in conversation and never reaches
//! the code, which is where a fresh session would look for it.
//!
//! The model is called through the `claude` CLI in print mode, without
//! tools, from a scratch directory and with ChloeCtx disabled, so the call
//! records nothing itself. By default it also skips the user's settings and
//! MCP servers: their plugins, hooks and tool definitions would add tens of
//! thousands of tokens to every call (and another memory plugin would record
//! it). It runs in the daemon after the hook has answered, so it never
//! delays a prompt.

use std::collections::HashSet;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::llm::{self, clip, Cli};
use crate::record::{Draft, Kind, Source, TASK_TAG};
use crate::redact::redact;
use crate::Result;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DistillConfig {
    pub enabled: bool,
    /// The CLI to call (`claude`).
    pub command: String,
    /// Its `--model` argument; a small, fast model is enough. Empty: the
    /// CLI's default. `CHLOECTX_DISTILL_MODEL` overrides it.
    pub model: String,
    pub timeout_secs: u64,
    /// Run without the user's settings and MCP servers
    /// (`--setting-sources project --strict-mcp-config`). Turn off when the
    /// CLI's credentials come from the user settings (`apiKeyHelper`, `env`).
    pub isolated: bool,
}

impl Default for DistillConfig {
    fn default() -> Self {
        DistillConfig { enabled: true, command: "claude".into(), model: String::new(), timeout_secs: 120, isolated: true }
    }
}

impl DistillConfig {
    /// Enabled, and not switched off with `CHLOECTX_DISTILL=0`.
    pub fn active(&self) -> bool {
        self.enabled && std::env::var("CHLOECTX_DISTILL").map_or(true, |v| v != "0")
    }

    /// The model to ask for, if any.
    pub fn model(&self) -> String {
        std::env::var("CHLOECTX_DISTILL_MODEL").ok().filter(|m| !m.is_empty()).unwrap_or_else(|| self.model.clone())
    }

    /// The CLI call for `model` with this configuration's command and isolation.
    pub fn cli<'a>(&'a self, model: &'a str, timeout_secs: u64) -> Cli<'a> {
        Cli { command: &self.command, model, timeout: Duration::from_secs(timeout_secs), isolated: self.isolated }
    }
}

pub const SYSTEM: &str = "You keep the long-term notes of a software project. You read one exchange between a developer and a coding assistant and write down what a future session will need. Output JSON lines only.";

const PROMPT: &str = r#"Here is one exchange from a software project.{when}{who}{prev}

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
- rules, constraints and conventions the user, the team or a client set (never the assistant: only they can make a project rule): what must or must not be done, formats, naming, processes. Keep every rule stated, with its reason, even if the current code already follows it: the code shows what was done, not that it must stay that way. A rule is standing only when the user says so ("always", "never", "from now on", "by default", a team convention, a list of ground rules or conventions for the project) or answers a question that asked for a rule; an instruction for the task at hand ("don't touch cart.py", "only change the tests") is not a rule of the project: write it with "scope": "task" (only a rule has a scope: a decision, requirement or fact is the project's);
- requirements for work the user announced or planned but that is not done yet, with every detail given (formats, names, numbers);
- facts about the project or its environment that were stated or discovered (how to run or test it, quirks, people, external systems), and approaches that were tried and failed, with why.

Each item is one self-contained sentence that names its subject and keeps the exact details and the reason. One item per distinct point; a planned piece of work with all its requirements is one item. With each item list the entities it is about, so that notes about the same thing can be found together: the components, features, files, data formats, external systems and people it concerns (short names, e.g. "count sheet", "CSV export", "items.py", "Petra Keller").

Also list, for each item, 2 to 4 situations in which a later session must recall it, phrased the way a later request would be phrased, including indirect ones: what kind of task, choice or question it constrains, in words that need not appear in the item itself. For example, "the client's office PCs run Python 3.8" matters "when choosing a library or standard-library module (tomllib needs 3.11)", "when parsing a config file", "when using newer syntax such as match statements".

The already-noted items above are the notes most related to this exchange (not all notes). When this exchange changes, reverses or extends one of them, write the new version with "replaces": the new sentence must stand on its own (repeat what still holds) and may say what changed (e.g. "30 days, changed from 14").

When this exchange scraps, reverts or drops work, a plan or a decision ("scrap it", "git restore", "we won't do that after all", "drop the feature"), the noted items about it no longer hold: its design, its decisions, its requirements and the facts about how it behaves. List each of them with "retired", and write one item that says what was dropped and why.

Leave out:
- a request to forget something, to delete it from memory or not to keep it (a private plan, a secret), and what it was about: write nothing about it, not even that it is private;
- anything already noted above, unless this exchange changes it;
- status that will soon be stale (test counts and results, what was committed or pushed and to which branch, whether a decision's work is built yet);
- how the user wants this turn done ("no code changes", "just answer", "don't commit yet"): it is not a rule;
- the assistant's own habits and conventions, even when it says it always follows them ("as usual I'll ...": commit message style, formatting, tool usage), and its suggestions the user did not take up;
- questions the assistant asked and assumptions it made that the user has not answered or confirmed: they are not requirements, and never write one up as something that must be settled before the work can start;
- what the user's question asks about or assumes, and the details of an example asked for in the reply only (no file written): "write a function for a 2.5% fee, code in the reply only" does not make 2.5% the project's fee, "SQL for a refunds table with a created column" plans no refunds table, asking for a branch name for a ticket starts no work on it, and a question how to build something ("I want a toggle for the new renderer. How should I add it?") plans nothing: it is planned work only once the user asks for it to be built. They are the project's only when the user says so or the work is written into its files;
- the assistant's answer to a question from what is noted already (above, or in the briefing it was given): the note it came from is kept already, and that the assistant could not also find it in the code is no fact about the project;
- details of the code that reading it shows (signatures, what a function does).

With each item say who stated it in this exchange: "said_by": "requester" when the request stated it, "assistant" when the assistant found, concluded or assumed it. What the assistant concluded is its claim, not an established fact: keep its wording cautious ("the assistant found ...", "it assumed ...") when it guessed.

Output one JSON object per line and nothing else:
{"kind": "decision" | "rule" | "requirement" | "fact", "text": "<sentence>", "entities": ["<entity>", ...], "when": ["<situation>", ...], "said_by": "requester" | "assistant", "scope": "standing" | "task"}
{"kind": "decision" | "rule" | "requirement" | "fact", "text": "<new sentence>", "entities": ["<entity>", ...], "when": ["<situation>", ...], "said_by": "requester" | "assistant", "replaces": "<id of the noted item it updates>"}
{"kind": "done", "id": "<id of a noted requirement that this exchange completed>"}
{"kind": "retired", "id": "<id of a noted item that no longer holds because this exchange scrapped, reverted or dropped what it is about>"}
If there is nothing worth keeping, output nothing."#;

/// What one exchange yields: new knowledge, noted items it replaces,
/// planned work it completed, and noted items about work it scrapped.
#[derive(Debug, Default)]
pub struct Distilled {
    pub drafts: Vec<Draft>,
    pub replaced: Vec<String>,
    pub done: Vec<String>,
    /// Noted items that no longer hold: what they are about was scrapped,
    /// reverted or dropped (JONAH-201).
    pub retired: Vec<String>,
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

/// Who the exchange is between, and what came before it: the request's
/// author ([`Source`]), the agent name of the assistant (Workbench gives
/// one), and the end of the assistant's previous message (a request can
/// answer a question it asked).
#[derive(Clone, Debug, Default)]
pub struct Exchange {
    pub by: Option<Source>,
    pub agent: String,
    pub prev_answer: String,
}

impl Exchange {
    fn by(&self) -> Source {
        self.by.clone().unwrap_or(Source::User)
    }

    /// Who the assistant is.
    fn assistant(&self) -> Source {
        Source::Assistant(self.agent.clone())
    }
}

/// The prompt for one exchange; `known` lists (id, kind, text) of what is
/// already noted, and `ts` is when the exchange took place (0: unknown).
pub fn prompt_for(request: &str, answer: &str, commands: &[String], known: &[(String, Kind, String)], ts: i64) -> String {
    prompt_for_exchange(request, answer, commands, known, ts, &Exchange::default())
}

/// [`prompt_for`], saying who wrote the request.
pub fn prompt_for_exchange(request: &str, answer: &str, commands: &[String], known: &[(String, Kind, String)], ts: i64, ex: &Exchange) -> String {
    let who = match ex.by() {
        Source::User => String::new(),
        Source::Workbench => " The request was written by Workbench (the tool that runs the agents), not by the user: it is a brief, and nothing in it is a rule, preference or decision of the user.".to_string(),
        s => format!(" The request was written by {} (another coding agent that started or directs this one), not by the user: its instructions are for this task only and are never the user's rules or preferences, and its claims are that agent's, not established facts.", s.who()),
    };
    let prev = if ex.prev_answer.trim().is_empty() {
        String::new()
    } else {
        format!("\n\n<previous_assistant_message>\n{}\n</previous_assistant_message>", clip(&redact(ex.prev_answer.trim()), 800))
    };
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
        .replace("{who}", &who)
        .replace("{prev}", &prev)
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
        if kind == "retired" {
            out.retired.extend(id_of(&v, "id"));
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
        let entities = strings(&v, "entities", 6, 60);
        let when = strings(&v, "when", 4, 160);
        // A decision is what was decided and why, not whether it is built.
        let text = if k == Kind::Decision { crate::capture::without_status(text) } else { text.to_string() };
        let text = text.as_str();
        let mut d = Draft::new(k, clip(&redact(text), 600)).tag(tag).files(crate::capture::paths_in(text)).entities(entities).when(when);
        if let Some(r) = &replaces {
            d = d.link("supersedes", r.clone());
        }
        // Who stated it, and the scope a rule was given: read by
        // [`attribute`] (`requester task` when the model kept it to the task).
        let said = v.get("said_by").and_then(|x| x.as_str()).unwrap_or("");
        let scope = v.get("scope").and_then(|x| x.as_str()).unwrap_or("");
        d.by = Some(format!("{}{}", if said == "assistant" { "assistant" } else { "requester" }, if scope == "task" { " task" } else { "" }));
        // A scope is a rule's: a decision, a requirement or a fact the
        // model calls "task" is still one (ANA-6: "Another decision with
        // the client: invoice numbers ..." was kept for that task only).
        if k == Kind::Preference && scope == "task" {
            d.tag = TASK_TAG.into();
        }
        out.replaced.extend(replaces);
        out.drafts.push(d);
    }
    out
}

/// Settles who said each item and how long it holds, whatever the model
/// wrote:
/// - a rule the user stated is standing unless it is an instruction for the
///   task at hand: the model kept it to the task, or it tells the agent
///   what not to touch or only to do now ([`crate::capture::task_instruction`]),
///   and the user did not say it is standing ([`crate::capture::standing_rule`]).
///   When unsure it is a rule, which the user can retire, rather than a
///   note no other session sees (ANA-202: a client's conventions were kept
///   "for this task only"). Any other instruction (the user's one-off,
///   another agent's brief, Workbench's) is kept as a note for this
///   session's task ([`TASK_TAG`]), never shown to other sessions as a
///   rule;
/// - an item the request stated is the request author's; one the assistant
///   found is the assistant's; neither an agent's nor Workbench's words
///   become the user's, and an agent's claim reads "The agent X said: …";
/// - a task note replaces nothing.
pub fn attribute(drafts: Vec<Draft>, request: &str, ex: &Exchange) -> Vec<Draft> {
    let req_by = ex.by();
    drafts
        .into_iter()
        .map(|mut d| {
            let by = if d.by.as_deref().is_some_and(|b| b.starts_with("assistant")) { ex.assistant() } else { req_by.clone() };
            if d.kind == Kind::Preference || d.tag == TASK_TAG {
                let model_task = d.tag == TASK_TAG || d.by.as_deref().is_some_and(|b| b.ends_with(" task"));
                let said_standing = crate::capture::standing_rule(request, &ex.prev_answer, &d.text);
                // How to do this turn ("don't commit yet", "no code changes")
                // is never a rule, whoever said it and however (JONAH-303).
                let now_only = crate::capture::for_this_turn(&d.text);
                let standing = d.kind == Kind::Preference && by.is_user() && !now_only && (said_standing || (!model_task && !crate::capture::task_instruction(request, &d.text)));
                if standing {
                    // The user said it is standing, whatever the model thought.
                    if d.tag == TASK_TAG {
                        d.tag = "distilled".into();
                    }
                } else {
                    let whose = by.who();
                    let task = if ex.agent.is_empty() { "this task".to_string() } else { format!("{}'s task", ex.agent) };
                    d.kind = Kind::Note;
                    d.tag = TASK_TAG.into();
                    d.importance = Some(0.5);
                    d.text = format!("For {task} only ({whose} said, not a standing rule): {}", d.text);
                    d.links.retain(|l| l.label != "supersedes");
                }
            } else if !by.is_user() && by != Source::Assistant(String::new()) && d.kind != Kind::Todo {
                // A decision or fact another agent or Workbench stated, or
                // this agent concluded (a plain session's assistant keeps
                // its notes unprefixed; their source says whose they are).
                let who = by.who();
                let who = format!("{}{}", who[..1].to_uppercase(), &who[1..]);
                if !d.text.starts_with(&who) {
                    d.text = format!("{who} said: {}", d.text);
                }
            }
            d.by = Some(by.as_str());
            d
        })
        .collect()
}

/// The drafts of a turn without planned work when its request only asked
/// how to do something and the turn changed no file (ANA-308: "How should I
/// add it?" was filed as planned work in every briefing). How many it left
/// out, too.
pub fn not_planned(drafts: Vec<Draft>, request: &str, changed_files: bool) -> (Vec<Draft>, usize) {
    if changed_files || !crate::capture::asks_how(request) {
        return (drafts, 0);
    }
    let n = drafts.len();
    let kept: Vec<Draft> = drafts.into_iter().filter(|d| d.kind != Kind::Todo).collect();
    let left = n - kept.len();
    (kept, left)
}

/// Runs the model on one exchange.
pub fn distill(cfg: &DistillConfig, request: &str, answer: &str, commands: &[String], known: &[(String, Kind, String)], ts: i64) -> Result<Distilled> {
    distill_exchange(cfg, request, answer, commands, known, ts, &Exchange::default())
}

/// [`distill`] of an exchange whose request may not be the user's.
pub fn distill_exchange(cfg: &DistillConfig, request: &str, answer: &str, commands: &[String], known: &[(String, Kind, String)], ts: i64, ex: &Exchange) -> Result<Distilled> {
    let prompt = prompt_for_exchange(request, answer, commands, known, ts, ex);
    let model = cfg.model();
    let r = llm::complete(&cfg.cli(&model, cfg.timeout_secs), SYSTEM, &prompt)?;
    let mut d = parse(&r.text);
    d.drafts = attribute(std::mem::take(&mut d.drafts), request, ex);
    // What a task note was going to replace stays as it was.
    let replacing: HashSet<String> = d.drafts.iter().flat_map(|x| x.links.iter().filter(|l| l.label == "supersedes").map(|l| l.to.clone())).collect();
    d.replaced.retain(|id| replacing.contains(id));
    d.cost_usd = r.cost_usd;
    d.usage = r.usage;
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_items_and_ignores_the_rest() {
        let text = "Here you go:\n{\"kind\": \"decision\", \"text\": \"Storage stays JSON because admins edit the files by hand.\", \"entities\": [\"storage\", \"JSON files\"], \"when\": [\"making saving faster\"]}\n\
                    {\"kind\": \"requirement\", \"text\": \"The CSV export uses semicolons and DD.MM.YYYY dates.\", \"replaces\": \"abc:3\"},\n\
                    {\"kind\": \"done\", \"id\": \"[abc:12]\"}\n{\"kind\": \"mood\", \"text\": \"The user seems happy today.\"}\nnot json\n\
                    {\"kind\": \"retired\", \"id\": \"abc:7\"}";
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
        assert_eq!(d.retired, vec!["abc:7".to_string()]);
    }

    #[test]
    fn instructions_and_agents_words_are_attributed() {
        let maya = "Fix the cent bug. Don't touch money.py or cart.py, paybutton works there.";
        let model = "{\"kind\": \"rule\", \"text\": \"money.py and cart.py must not be touched in pay or checkout bug-fix work; the user set this.\", \"said_by\": \"requester\", \"replaces\": \"x:1\"}\n\
                     {\"kind\": \"fact\", \"text\": \"test_promo_window is a known pre-existing flaky test that can be ignored.\", \"said_by\": \"assistant\"}\n\
                     {\"kind\": \"decision\", \"text\": \"Customers win: totals round half up to match receipts.\", \"said_by\": \"requester\"}";
        // The user's one-off: a note for this task, replacing nothing.
        let ex = Exchange { by: None, agent: "cents".into(), prev_answer: String::new() };
        let d = attribute(parse(model).drafts, maya, &ex);
        assert_eq!(d[0].kind, Kind::Note);
        assert_eq!(d[0].tag, TASK_TAG);
        assert!(d[0].text.starts_with("For cents's task only (the user said, not a standing rule): money.py"), "{}", d[0].text);
        assert!(d[0].links.is_empty());
        assert_eq!(d[0].by.as_deref(), Some("user"));
        // The assistant's claim is the agent's.
        assert_eq!(d[1].kind, Kind::Fact);
        assert!(d[1].text.starts_with("The agent cents said: test_promo_window"), "{}", d[1].text);
        assert_eq!(d[1].by.as_deref(), Some("assistant:cents"));
        // The user's decision stays the user's.
        assert_eq!(d[2].kind, Kind::Decision);
        assert!(d[2].text.starts_with("Customers win"), "{}", d[2].text);
        // ANA-6: a decision the model scoped to the task is still a decision.
        let client = "{\"kind\": \"decision\", \"text\": \"Invoice numbers are INV-YYYY-NNNNN, as agreed with the client.\", \"said_by\": \"requester\", \"scope\": \"task\"}";
        let d = attribute(parse(client).drafts, "Another decision with the client: invoice numbers are INV-YYYY-NNNNN.", &ex);
        assert_eq!((d[0].kind, d[0].tag.as_str()), (Kind::Decision, "distilled"), "{d:?}");
        assert!(d[0].text.starts_with("Invoice numbers"), "{}", d[0].text);
        // A standing rule stays one.
        let rule = "{\"kind\": \"rule\", \"text\": \"Money is always kept as integer cents.\", \"said_by\": \"requester\"}";
        let d = attribute(parse(rule).drafts, "From now on, always keep money as integer cents.", &ex);
        assert_eq!(d[0].kind, Kind::Preference);
        // The lead's brief: its rules are for the task, its claims the lead's.
        let lead = Exchange { by: Some(Source::Agent("lead".into())), agent: "freeship".into(), prev_answer: String::new() };
        let brief = "{\"kind\": \"rule\", \"text\": \"Shipping is always 499 cents by default.\", \"said_by\": \"requester\"}\n\
                     {\"kind\": \"fact\", \"text\": \"test_promo_window is a known flaky test; ignore it.\", \"said_by\": \"requester\"}";
        let d = attribute(parse(brief).drafts, "Always: shipping is 499 cents by default. test_promo_window is a known flaky test; ignore it.", &lead);
        assert_eq!(d[0].kind, Kind::Note, "{d:?}");
        assert!(d[0].text.contains("the agent lead said"), "{}", d[0].text);
        assert!(d[1].text.starts_with("The agent lead said: test_promo_window"), "{}", d[1].text);
        assert_eq!(d[1].by.as_deref(), Some("agent:lead"));
        let p = prompt_for_exchange("x", "y", &[], &[], 0, &lead);
        assert!(p.contains("written by the agent lead") && p.contains("never the user's rules"), "{p}");
    }

    /// ANA-202: a client's conventions, given as the project's context, are
    /// rules every session is told, not notes "for this task only". The
    /// model's lines are what haiku answered for Ana's kickoff.
    #[test]
    fn a_projects_conventions_are_standing_rules() {
        let kickoff = "Some context for this project, from the kickoff with the client: every timestamp column stores UTC epoch milliseconds and is named with an _at_ms suffix (created_at_ms, settled_at_ms). Interest calculations use banker's rounding (ROUND_HALF_EVEN), but fee calculations use ROUND_HALF_UP, because the regulator audits fee calculations against half-up rounding. Two things from ops: deployments to staging happen only on Tuesdays and Thursdays, and when a client retries a payment with an idempotency key we already saw, we return error code LDG-0007. Just acknowledge, nothing to do yet.";
        let model = "{\"kind\": \"rule\", \"text\": \"Every timestamp column stores UTC epoch milliseconds and is named with an `_at_ms` suffix (e.g. `created_at_ms`, `settled_at_ms`), as set at the client kickoff.\", \"said_by\": \"requester\", \"scope\": \"standing\"}\n\
                     {\"kind\": \"rule\", \"text\": \"Interest calculations use banker's rounding (ROUND_HALF_EVEN), as set by the client.\", \"said_by\": \"requester\", \"scope\": \"standing\"}\n\
                     {\"kind\": \"rule\", \"text\": \"Fee calculations use ROUND_HALF_UP rounding, because the regulator audits fee calculations against half-up rounding.\", \"said_by\": \"requester\", \"scope\": \"standing\"}\n\
                     {\"kind\": \"rule\", \"text\": \"Deployments to staging happen only on Tuesdays and Thursdays.\", \"said_by\": \"requester\", \"scope\": \"standing\"}\n\
                     {\"kind\": \"fact\", \"text\": \"When a client retries a payment with an idempotency key already seen, the system returns error code LDG-0007.\", \"said_by\": \"requester\"}";
        let d = attribute(parse(model).drafts, kickoff, &Exchange::default());
        let kinds: Vec<(Kind, &str)> = d.iter().map(|x| (x.kind, x.tag.as_str())).collect();
        assert_eq!(kinds, [(Kind::Preference, "distilled"), (Kind::Preference, "distilled"), (Kind::Preference, "distilled"), (Kind::Preference, "distilled"), (Kind::Fact, "distilled")], "{d:?}");
        assert!(d.iter().all(|x| !x.text.contains("not a standing rule") && x.by.as_deref() == Some("user")), "{d:?}");
        // Even with no scope from the model: a statement of how the project works.
        let d = attribute(parse("{\"kind\": \"rule\", \"text\": \"Fee calculations use ROUND_HALF_UP rounding.\"}").drafts, kickoff, &Exchange::default());
        assert_eq!(d[0].kind, Kind::Preference, "{d:?}");
        // Maya's instruction for her task stays the task's, whatever scope
        // the model gives it (haiku said "task"; here "standing").
        let maya = "Fix the cent bug in the cart total: 0.1 + 0.2 shows 0.30000000000000004. Don't touch money.py or cart.py, paybutton works there. Only change totals.py and its tests.";
        for scope in ["task", "standing", ""] {
            let line = format!("{{\"kind\": \"rule\", \"text\": \"For the cent-bug fix, money.py and cart.py must not be touched, because the paybutton works there.\", \"said_by\": \"requester\", \"scope\": \"{scope}\"}}\n{{\"kind\": \"rule\", \"text\": \"Only totals.py and its tests may be changed in the cent-bug fix.\", \"scope\": \"{scope}\"}}");
            let d = attribute(parse(&line).drafts, maya, &Exchange::default());
            assert!(d.iter().all(|x| x.kind == Kind::Note && x.tag == TASK_TAG), "{scope}: {d:?}");
        }
        // The model keeps it to the task, the user said it stands: it stands.
        let ops = "From ops: never deploy to staging on Fridays.";
        let d = attribute(parse("{\"kind\": \"rule\", \"text\": \"Never deploy to staging on Fridays (ops rule).\", \"scope\": \"task\"}").drafts, ops, &Exchange::default());
        assert_eq!((d[0].kind, d[0].tag.as_str()), (Kind::Preference, "distilled"), "{d:?}");
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
