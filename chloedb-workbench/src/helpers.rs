//! Helpers: the sub-agents an agent starts with Claude Code's own Agent
//! (or Task) tool. They run inside the agent's session, so they are not
//! Workbench agents (no row, no terminal; only `spawn_agent` makes those),
//! but every hook Claude Code fires inside one carries its `agent_id`, and
//! `SubagentStart` and `SubagentStop` open and close it. So each one can be
//! followed: what it was asked, what it does now, how it ended and its
//! report.
//!
//! [`Helpers::apply`] takes the hooks in order, live in the supervisor (the
//! agent's JSON gets [`Helpers::brief`]) or from the stored events
//! ([`of_events`], for `GET /api/agents/:id/helpers`).

use serde_json::{json, Value as Json};

use crate::db::{iso, Event};
use crate::narrate;

/// The tools that start one.
pub fn is_agent_tool(tool: &str) -> bool {
    tool == "Agent" || tool == "Task"
}

/// What is kept of a report (its last message), in characters.
const REPORT_MAX: usize = 8000;
/// What is kept of what it was asked.
const PROMPT_MAX: usize = 2000;
/// Helpers in an agent's JSON, the newest.
const BRIEF_MAX: usize = 8;

#[derive(Clone, Debug, Default)]
pub struct Helper {
    /// The Agent call's `tool_use_id`.
    use_id: Option<String>,
    /// Claude Code's id of it, from `SubagentStart` or the call's answer.
    pub agent_id: Option<String>,
    /// `subagent_type` (`general-purpose`, `Explore`, a repository's own).
    pub kind: String,
    pub description: String,
    pub prompt: String,
    /// Started with `isolation: "worktree"`.
    pub worktree: bool,
    pub worktree_branch: Option<String>,
    /// The folder its hooks report (its own worktree, when isolated).
    pub cwd: Option<String>,
    /// `run_in_background`: the call answers at once, it goes on.
    pub background: bool,
    pub started_ms: i64,
    pub ended_ms: Option<i64>,
    /// `running`, `done`, `failed`, `stopped` (its session ended first).
    pub state: &'static str,
    /// Its latest tool call, as a sentence.
    pub doing: Option<String>,
    pub doing_ms: Option<i64>,
    /// Its tool calls seen in hooks.
    pub calls: u32,
    pub report: Option<String>,
    pub tokens: Option<i64>,
    pub tool_uses: Option<i64>,
    pub duration_ms: Option<i64>,
}

#[derive(Clone, Debug, Default)]
pub struct Helpers {
    pub list: Vec<Helper>,
}

fn s<'a>(j: &'a Json, k: &str) -> &'a str {
    j.get(k).and_then(Json::as_str).unwrap_or("")
}

fn clip(t: &str, max: usize) -> String {
    let t = t.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    let mut out: String = t.chars().take(max - 1).collect();
    out.push('…');
    out
}

/// The text of an Agent call's answer: `content` blocks, or a string.
fn response_text(r: &Json) -> String {
    match r.get("content") {
        Some(Json::Array(blocks)) => blocks.iter().filter_map(|b| b.get("text").and_then(Json::as_str)).collect::<Vec<_>>().join("\n\n"),
        Some(Json::String(t)) => t.clone(),
        _ => r.as_str().map(str::to_string).unwrap_or_default(),
    }
}

/// A type with no name is the general-purpose one.
fn same_kind(a: &str, b: &str) -> bool {
    let n = |x: &str| if x.is_empty() { "general-purpose".to_string() } else { x.to_ascii_lowercase() };
    n(a) == n(b)
}

impl Helpers {
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// The helper of a Claude Code agent id, linking a new id to the oldest
    /// running helper of its type that has none yet (calls started together
    /// start their sub-agents in the same order).
    fn of_agent(&mut self, agent_id: &str, kind: &str, ts: i64) -> usize {
        if let Some(i) = self.list.iter().position(|h| h.agent_id.as_deref() == Some(agent_id)) {
            return i;
        }
        let free = |h: &Helper| h.agent_id.is_none() && h.state == "running";
        let pick = self.list.iter().position(|h| free(h) && same_kind(&h.kind, kind)).or_else(|| self.list.iter().position(free));
        match pick {
            Some(i) => {
                self.list[i].agent_id = Some(agent_id.to_string());
                i
            }
            // One whose start was not seen (a hook missed, or older events).
            None => {
                self.list.push(Helper { agent_id: Some(agent_id.to_string()), kind: kind.to_string(), description: if kind.is_empty() { "A sub-agent".into() } else { kind.to_string() }, started_ms: ts, state: "running", ..Default::default() });
                self.list.len() - 1
            }
        }
    }

    /// The call an answer belongs to: by `tool_use_id`, else the oldest
    /// running one with its description.
    fn of_call(&self, hook: &Json) -> Option<usize> {
        let use_id = s(hook, "tool_use_id");
        if !use_id.is_empty() {
            if let Some(i) = self.list.iter().position(|h| h.use_id.as_deref() == Some(use_id)) {
                return Some(i);
            }
        }
        let d = s(&hook["tool_input"], "description");
        self.list.iter().position(|h| h.state == "running" && h.ended_ms.is_none() && h.description == d && !h.background)
    }

    /// An id the call's answer names: when it was linked to another helper
    /// by order, the two swap what was seen of them.
    fn settle_id(&mut self, i: usize, agent_id: &str) {
        if agent_id.is_empty() || self.list[i].agent_id.as_deref() == Some(agent_id) {
            return;
        }
        if let Some(j) = self.list.iter().position(|h| h.agent_id.as_deref() == Some(agent_id)) {
            let (a, b) = (self.list[i].clone(), self.list[j].clone());
            let h = &mut self.list[j];
            h.agent_id = a.agent_id;
            (h.doing, h.doing_ms, h.calls, h.cwd) = (a.doing, a.doing_ms, a.calls, a.cwd);
            let h = &mut self.list[i];
            (h.doing, h.doing_ms, h.calls, h.cwd) = (b.doing, b.doing_ms, b.calls, b.cwd);
        }
        self.list[i].agent_id = Some(agent_id.to_string());
    }

    /// One hook, in the order they came.
    pub fn apply(&mut self, kind: &str, hook: &Json, ts: i64) {
        let tool = s(hook, "tool_name");
        let inside = s(hook, "agent_id");
        if !inside.is_empty() {
            // A hook fired inside a sub-agent, or `SubagentStart` and
            // `SubagentStop`, which name it too.
            let i = self.of_agent(inside, s(hook, "agent_type"), ts);
            let cwd = s(hook, "cwd");
            match kind {
                "SubagentStop" => self.stop(i, hook, ts),
                "PreToolUse" => {
                    let h = &mut self.list[i];
                    h.calls += 1;
                    h.doing = Some(narrate::tool_sentence(&[(tool.to_string(), hook["tool_input"].clone())], cwd));
                    h.doing_ms = Some(ts);
                    if h.cwd.is_none() && !cwd.is_empty() {
                        h.cwd = Some(cwd.to_string());
                    }
                }
                _ => {}
            }
            return;
        }
        match kind {
            "PreToolUse" if is_agent_tool(tool) => {
                let input = &hook["tool_input"];
                let use_id = s(hook, "tool_use_id");
                if !use_id.is_empty() && self.list.iter().any(|h| h.use_id.as_deref() == Some(use_id)) {
                    return;
                }
                self.list.push(Helper {
                    use_id: (!use_id.is_empty()).then(|| use_id.to_string()),
                    kind: s(input, "subagent_type").to_string(),
                    description: clip(if s(input, "description").is_empty() { s(input, "prompt") } else { s(input, "description") }, 120),
                    prompt: clip(s(input, "prompt"), PROMPT_MAX),
                    worktree: s(input, "isolation") == "worktree",
                    background: input.get("run_in_background").and_then(Json::as_bool).unwrap_or(false),
                    started_ms: ts,
                    state: "running",
                    ..Default::default()
                });
            }
            "PostToolUse" | "PostToolUseFailure" if is_agent_tool(tool) => {
                let Some(i) = self.of_call(hook) else { return };
                let r = &hook["tool_response"];
                self.settle_id(i, s(r, "agentId"));
                let h = &mut self.list[i];
                if let Some(b) = r.get("worktreeBranch").and_then(Json::as_str) {
                    h.worktree_branch = Some(b.to_string());
                }
                if kind == "PostToolUse" && s(r, "status") == "async_launched" {
                    // Started in the background: it ends with SubagentStop.
                    h.background = true;
                    return;
                }
                let failed = kind == "PostToolUseFailure" || r.get("is_error").and_then(Json::as_bool) == Some(true) || r.get("interrupted").and_then(Json::as_bool) == Some(true);
                h.state = if failed { "failed" } else { "done" };
                h.ended_ms = Some(ts);
                let text = if kind == "PostToolUseFailure" { s(hook, "error").to_string() } else { response_text(r) };
                if !text.trim().is_empty() {
                    h.report = Some(clip(&text, REPORT_MAX));
                }
                h.tokens = r.get("totalTokens").and_then(Json::as_i64).or(h.tokens);
                h.tool_uses = r.get("totalToolUseCount").and_then(Json::as_i64).or(h.tool_uses);
                h.duration_ms = r.get("totalDurationMs").and_then(Json::as_i64).or(h.duration_ms);
            }
            // A turn that ended stopped the sub-agents it waited for.
            "Stop" => self.end_running(ts, true),
            "SessionEnd" | "exited" => self.end_running(ts, false),
            _ => {}
        }
    }

    /// `SubagentStop`: its last message. A call it ran for answers after,
    /// with the report and the totals.
    fn stop(&mut self, i: usize, hook: &Json, ts: i64) {
        let h = &mut self.list[i];
        if h.state != "running" {
            return;
        }
        h.state = "done";
        h.ended_ms = Some(ts);
        let last = s(hook, "last_assistant_message");
        if h.report.is_none() && !last.trim().is_empty() {
            h.report = Some(clip(last, REPORT_MAX));
        }
    }

    fn end_running(&mut self, ts: i64, foreground_only: bool) {
        for h in self.list.iter_mut().filter(|h| h.state == "running" && (!foreground_only || !h.background)) {
            h.state = "stopped";
            h.ended_ms = Some(ts);
        }
    }

    pub fn running(&self) -> usize {
        self.list.iter().filter(|h| h.state == "running").count()
    }

    /// Running, of an agent whose process runs or not.
    pub fn running_of(&self, alive: bool) -> usize {
        if alive {
            self.running()
        } else {
            0
        }
    }

    /// What the agent's JSON holds: the counts and the newest helpers,
    /// without their prompts and reports. `alive`: its process runs (one
    /// that does not has no helper running).
    pub fn brief(&self, alive: bool) -> Json {
        let state = |h: &Helper| if h.state == "running" && !alive { "stopped" } else { h.state };
        let count = |st: &str| self.list.iter().filter(|h| state(h) == st).count();
        let from = self.list.len().saturating_sub(BRIEF_MAX);
        json!({
            "total": self.list.len(),
            "running": count("running"),
            "done": count("done"),
            "failed": count("failed") + count("stopped"),
            "list": self.list[from..].iter().map(|h| json!({
                "description": h.description,
                "kind": h.kind,
                "state": state(h),
                "doing": h.doing,
                "worktree": h.worktree,
                "started": iso(h.started_ms),
                "ended": h.ended_ms.map(iso),
            })).collect::<Vec<_>>(),
        })
    }

    /// Every helper, with what it was asked and its report.
    pub fn to_json(&self, alive: bool) -> Json {
        Json::Array(
            self.list
                .iter()
                .enumerate()
                .map(|(n, h)| {
                    let state = if h.state == "running" && !alive { "stopped" } else { h.state };
                    json!({
                        "n": n + 1,
                        "agent_id": h.agent_id,
                        "kind": h.kind,
                        "description": h.description,
                        "prompt": h.prompt,
                        "worktree": h.worktree,
                        "worktree_branch": h.worktree_branch,
                        "cwd": h.cwd,
                        "background": h.background,
                        "state": state,
                        "started": iso(h.started_ms),
                        "ended": h.ended_ms.map(iso),
                        "doing": h.doing,
                        "doing_at": h.doing_ms.map(iso),
                        "calls": h.calls,
                        "summary": h.report.as_deref().map(|r| narrate::sentences(r, 2, 240)),
                        "report": h.report,
                        "tokens": h.tokens,
                        "tool_uses": h.tool_uses,
                        "duration_ms": h.duration_ms.or_else(|| h.ended_ms.map(|e| e - h.started_ms)),
                    })
                })
                .collect(),
        )
    }
}

/// The helpers of an agent's stored events, oldest first.
pub fn of_events(evs: &[Event]) -> Helpers {
    let mut hs = Helpers::default();
    for ev in evs {
        let d: Json = ev.data.as_deref().and_then(|d| serde_json::from_str(d).ok()).unwrap_or(Json::Null);
        if d.is_null() && ev.kind != "exited" {
            continue;
        }
        hs.apply(&ev.kind, &d, ev.ts);
    }
    hs
}

/// An Agent call's answer, made small enough to be kept whole as an event
/// (events keep 16 KB of their hook's JSON, and a cut one does not parse):
/// its report and its prompt clipped.
pub fn shrink(hook: &mut Json) {
    if let Some(t) = hook.get("last_assistant_message").and_then(Json::as_str).map(|t| clip(t, REPORT_MAX)) {
        hook["last_assistant_message"] = Json::String(t);
    }
    if !is_agent_tool(s(hook, "tool_name")) {
        return;
    }
    if let Some(p) = hook.pointer_mut("/tool_input/prompt") {
        if let Some(t) = p.as_str() {
            *p = Json::String(clip(t, PROMPT_MAX));
        }
    }
    if let Some(r) = hook.pointer_mut("/tool_response/prompt") {
        if let Some(t) = r.as_str() {
            *r = Json::String(clip(t, 200));
        }
    }
    if let Some(Json::Array(blocks)) = hook.pointer_mut("/tool_response/content") {
        let text = blocks.iter().filter_map(|b| b.get("text").and_then(Json::as_str)).collect::<Vec<_>>().join("\n\n");
        *blocks = vec![json!({ "type": "text", "text": clip(&text, REPORT_MAX) })];
    }
    if let Some(r) = hook.get_mut("tool_response").and_then(Json::as_object_mut) {
        // Its own bookkeeping, which the board does not show.
        for k in ["usage", "toolStats", "harnessSectionHash"] {
            r.remove(k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pre(tool: &str, input: Json, use_id: &str) -> Json {
        json!({ "hook_event_name": "PreToolUse", "tool_name": tool, "tool_input": input, "tool_use_id": use_id, "cwd": "/w" })
    }
    fn inside(agent: &str, kind: &str, tool: &str, input: Json) -> Json {
        json!({ "hook_event_name": "PreToolUse", "tool_name": tool, "tool_input": input, "agent_id": agent, "agent_type": kind, "cwd": "/w/.claude/worktrees/x" })
    }

    #[test]
    fn follows_three_parallel_helpers_to_their_reports() {
        let mut hs = Helpers::default();
        for (i, d) in ["Map the refund code", "Find the callers", "Read the tests"].iter().enumerate() {
            hs.apply("PreToolUse", &pre("Agent", json!({ "description": d, "prompt": format!("{d}, please"), "subagent_type": "Explore", "isolation": "worktree" }), &format!("u{i}")), 10 + i as i64);
        }
        for (i, id) in ["a1", "a2", "a3"].iter().enumerate() {
            hs.apply("SubagentStart", &json!({ "hook_event_name": "SubagentStart", "agent_id": id, "agent_type": "Explore" }), 20 + i as i64);
        }
        hs.apply("PreToolUse", &inside("a2", "Explore", "Grep", json!({ "pattern": "refund(" })), 30);
        hs.apply("PreToolUse", &inside("a1", "Explore", "Read", json!({ "file_path": "/w/.claude/worktrees/x/src/refunds.rs" })), 31);
        assert_eq!(hs.running(), 3);
        let b = hs.brief(true);
        assert_eq!((b["total"].as_u64(), b["running"].as_u64()), (Some(3), Some(3)));
        assert_eq!(b["list"][0]["doing"], json!("Reading src/refunds.rs"));
        assert_eq!(b["list"][1]["doing"], json!("Searching for `refund(`"));
        assert_eq!(b["list"][0]["worktree"], json!(true));

        // The second ends: its call answers with the report and its id.
        hs.apply("SubagentStop", &json!({ "hook_event_name": "SubagentStop", "agent_id": "a2", "agent_type": "Explore", "last_assistant_message": "Two callers." }), 40);
        hs.apply("PostToolUse", &json!({ "hook_event_name": "PostToolUse", "tool_name": "Agent", "tool_use_id": "u1", "tool_input": {}, "tool_response": { "status": "completed", "agentId": "a2", "content": [{ "type": "text", "text": "Two callers: checkout.rs and api.rs. Both round down." }], "totalTokens": 1200, "totalToolUseCount": 7, "totalDurationMs": 21000 } }), 41);
        let all = hs.to_json(true);
        assert_eq!(all[1]["state"], json!("done"));
        assert_eq!(all[1]["report"], json!("Two callers: checkout.rs and api.rs. Both round down."));
        assert_eq!((all[1]["tokens"].as_i64(), all[1]["tool_uses"].as_i64(), all[1]["duration_ms"].as_i64()), (Some(1200), Some(7), Some(21000)));
        assert_eq!(hs.running(), 2);

        // The turn ends with one still open: it was stopped. An exited
        // agent has none running.
        hs.apply("PostToolUse", &json!({ "hook_event_name": "PostToolUse", "tool_name": "Agent", "tool_use_id": "u0", "tool_response": { "content": [{ "type": "text", "text": "Done." }], "agentId": "a1" } }), 50);
        assert_eq!(hs.brief(false)["running"], json!(0));
        hs.apply("Stop", &json!({ "hook_event_name": "Stop" }), 60);
        let all = hs.to_json(true);
        assert_eq!([all[0]["state"].clone(), all[1]["state"].clone(), all[2]["state"].clone()], [json!("done"), json!("done"), json!("stopped")]);
    }

    #[test]
    fn an_answer_naming_another_id_swaps_what_was_seen() {
        let mut hs = Helpers::default();
        hs.apply("PreToolUse", &pre("Task", json!({ "description": "One" }), "u0"), 1);
        hs.apply("PreToolUse", &pre("Task", json!({ "description": "Two" }), "u1"), 2);
        // The sub-agents started the other way round.
        hs.apply("SubagentStart", &json!({ "agent_id": "b", "agent_type": "general-purpose" }), 3);
        hs.apply("SubagentStart", &json!({ "agent_id": "a", "agent_type": "general-purpose" }), 4);
        hs.apply("PreToolUse", &inside("a", "general-purpose", "Bash", json!({ "command": "cargo test" })), 5);
        hs.apply("PostToolUse", &json!({ "tool_name": "Task", "tool_use_id": "u0", "tool_response": { "agentId": "a", "content": [{ "type": "text", "text": "ok" }] } }), 6);
        let all = hs.to_json(true);
        assert_eq!(all[0]["agent_id"], json!("a"));
        assert_eq!(all[0]["calls"], json!(1));
        assert_eq!(all[1]["agent_id"], json!("b"));
        assert_eq!(all[1]["doing"], Json::Null);
        assert_eq!(all[1]["state"], json!("running"));
    }

    #[test]
    fn a_background_helper_outlives_the_turn() {
        let mut hs = Helpers::default();
        hs.apply("PreToolUse", &pre("Agent", json!({ "description": "Watch CI", "run_in_background": true }), "u0"), 1);
        hs.apply("PostToolUse", &json!({ "tool_name": "Agent", "tool_use_id": "u0", "tool_response": { "status": "async_launched", "agentId": "bg", "description": "Watch CI", "prompt": "…", "outputFile": "/tmp/o" } }), 2);
        hs.apply("Stop", &json!({}), 3);
        assert_eq!(hs.running(), 1);
        hs.apply("SubagentStop", &json!({ "agent_id": "bg", "last_assistant_message": "CI passed." }), 9);
        let all = hs.to_json(true);
        assert_eq!((all[0]["state"].clone(), all[0]["report"].clone()), (json!("done"), json!("CI passed.")));
    }

    #[test]
    fn a_large_answer_is_kept_whole_and_parses() {
        let mut h = json!({ "hook_event_name": "PostToolUse", "tool_name": "Agent", "tool_input": { "prompt": "p".repeat(9000) }, "tool_response": { "content": [{ "type": "text", "text": "r".repeat(30000) }], "usage": { "input_tokens": 1 } } });
        shrink(&mut h);
        let s = serde_json::to_string(&h).unwrap();
        assert!(s.len() < crate::supervisor::DATA_MAX, "{}", s.len());
        assert!(h["tool_response"].get("usage").is_none());
        // Other tools are left alone.
        let mut b = json!({ "tool_name": "Bash", "tool_input": { "command": "x".repeat(20000) } });
        shrink(&mut b);
        assert_eq!(b["tool_input"]["command"].as_str().unwrap().len(), 20000);
    }
}
