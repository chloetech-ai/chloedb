//! A Workbench session gets the permissions a regular Claude Code session
//! started in the repository's main checkout gets.
//!
//! Claude Code reads the user's `~/.claude/settings.json` and the
//! repository's committed `.claude/settings.json` in any folder, a
//! worktree too. Two things of a regular session are tied to the main
//! checkout's folder, and a Workbench worktree does not have them:
//!
//! - **`.claude/settings.local.json`**, which git never commits: the "Yes,
//!   and don't ask again" rules the user built up, its default mode, its
//!   additional directories, and which of the project's `.mcp.json` servers
//!   the user approved;
//! - **local-scope MCP servers** (`claude mcp add` in that folder), kept in
//!   `~/.claude.json` under the folder's path.
//!
//! So each session's own `--settings` and `--mcp-config` (files Workbench
//! writes per session) carry them, the rules that name the main checkout
//! also for the worktree. The user's files are read, never written.
//!
//! And the permission mode a session starts in (`claude --permission-mode`)
//! is the user's choice at start, as it is for `claude` in a terminal.

use std::path::{Path, PathBuf};

use serde_json::{json, Value as Json};

/// Claude Code's permission modes, from the one that allows least.
pub const MODES: [&str; 5] = ["plan", "default", "acceptEdits", "auto", "bypassPermissions"];

/// A mode's place in [`MODES`]; `None` for a word that is not one.
pub fn rank(mode: &str) -> Option<usize> {
    MODES.iter().position(|m| *m == mode)
}

/// The tools whose rules name paths: `Read(/src/**)` is relative to the
/// project, `Read(//abs/**)` absolute.
const PATH_TOOLS: [&str; 7] = ["Read", "Edit", "Write", "MultiEdit", "NotebookEdit", "Glob", "Grep"];

/// A rule of the main checkout's local settings, for a session in `cwd`:
/// the rule, and the same for the worktree when it names the main checkout.
/// A path rule relative to the project is made absolute (the session's
/// settings file is not in the project), for both folders.
pub fn rules_for(rule: &str, main: &str, cwd: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |r: String| {
        if !out.contains(&r) {
            out.push(r);
        }
    };
    if let (Some(open), true) = (rule.find('('), rule.ends_with(')')) {
        let (name, spec) = (&rule[..open], &rule[open + 1..rule.len() - 1]);
        if PATH_TOOLS.contains(&name) && spec.starts_with('/') && !spec.starts_with("//") {
            push(format!("{name}(/{main}{spec})"));
            push(format!("{name}(/{cwd}{spec})"));
            return out;
        }
    }
    push(rule.to_string());
    if main != cwd && !main.is_empty() && rule.contains(main) {
        push(rule.replace(main, cwd));
    }
    out
}

fn read_json(p: &Path) -> Option<Json> {
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

/// The main checkout's `.claude/settings.local.json`, when it has one.
pub fn local_settings(main: &Path) -> Option<Json> {
    read_json(&main.join(".claude").join("settings.local.json")).filter(Json::is_object)
}

/// Claude Code's own config file (`~/.claude.json`, or the one in
/// `$CLAUDE_CONFIG_DIR`).
fn claude_json() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d).join(".claude.json"));
    }
    Some(PathBuf::from(std::env::var_os("HOME")?).join(".claude.json"))
}

/// The local-scope MCP servers of a folder, from Claude Code's config.
pub fn local_servers(main: &Path) -> serde_json::Map<String, Json> {
    let Some(cfg) = claude_json().and_then(|p| read_json(&p)) else { return serde_json::Map::new() };
    let keys = [main.to_string_lossy().to_string(), main.canonicalize().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()];
    keys.iter().filter(|k| !k.is_empty()).find_map(|k| cfg["projects"][k.as_str()]["mcpServers"].as_object().cloned()).unwrap_or_default()
}

/// What a session got from the main checkout, for its row: counts, so
/// the agent's page can say so.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Carried {
    pub rules: usize,
    pub mode: Option<String>,
    pub dirs: usize,
    pub servers: usize,
}

impl Carried {
    pub fn to_json(&self) -> Json {
        json!({ "rules": self.rules, "default_mode": self.mode, "directories": self.dirs, "servers": self.servers })
    }
    pub fn is_empty(&self) -> bool {
        *self == Carried::default()
    }
}

/// `settings` with the main checkout's local permissions merged in, for a
/// session in `cwd`. `mode_ok` says whether its default mode is taken
/// (not for a read-only agent). The session's own rules come first;
/// deny rules win in Claude Code whatever their order.
pub fn with_local(mut settings: Json, local: &Json, main: &Path, cwd: &Path, mode_ok: bool) -> (Json, Carried) {
    let mut c = Carried::default();
    let (main_s, cwd_s) = (main.to_string_lossy().to_string(), cwd.to_string_lossy().to_string());
    let p = &local["permissions"];
    if !settings["permissions"].is_object() {
        settings["permissions"] = json!({});
    }
    for d in ["allow", "ask", "deny"] {
        let mut list: Vec<Json> = settings["permissions"][d].as_array().cloned().unwrap_or_default();
        let before = list.len();
        for r in p[d].as_array().into_iter().flatten().filter_map(Json::as_str) {
            for x in rules_for(r, &main_s, &cwd_s) {
                let x = json!(x);
                if !list.contains(&x) {
                    list.push(x);
                }
            }
        }
        c.rules += list.len() - before;
        if !list.is_empty() {
            settings["permissions"][d] = Json::Array(list);
        }
    }
    if mode_ok && settings["permissions"]["defaultMode"].is_null() {
        if let Some(m) = p["defaultMode"].as_str().filter(|m| rank(m).is_some()) {
            settings["permissions"]["defaultMode"] = json!(m);
            c.mode = Some(m.to_string());
        }
    }
    let mut dirs: Vec<Json> = settings["permissions"]["additionalDirectories"].as_array().cloned().unwrap_or_default();
    for d in p["additionalDirectories"].as_array().into_iter().flatten().filter_map(Json::as_str) {
        // Relative to the main checkout, as Claude Code reads them there.
        let abs = if d.starts_with('/') || d.starts_with('~') { d.to_string() } else { main.join(d).to_string_lossy().to_string() };
        if !dirs.contains(&json!(abs)) {
            dirs.push(json!(abs));
            c.dirs += 1;
        }
    }
    if !dirs.is_empty() {
        settings["permissions"]["additionalDirectories"] = Json::Array(dirs);
    }
    if settings["permissions"].as_object().is_some_and(|o| o.is_empty()) {
        if let Some(o) = settings.as_object_mut() {
            o.remove("permissions");
        }
    }
    // The project's .mcp.json servers the user approved there.
    for k in ["enabledMcpjsonServers", "disabledMcpjsonServers", "enableAllProjectMcpServers"] {
        if settings.get(k).is_none() && !local[k].is_null() {
            settings[k] = local[k].clone();
        }
    }
    (settings, c)
}

/// `mcp` with the main checkout's local-scope servers added (a server of
/// the same name in the session's config stays as it is).
pub fn with_servers(mut mcp: Json, servers: &serde_json::Map<String, Json>) -> (Json, usize) {
    if !mcp["mcpServers"].is_object() {
        mcp["mcpServers"] = json!({});
    }
    let mut n = 0;
    for (name, def) in servers {
        if mcp["mcpServers"].get(name).is_none() {
            mcp["mcpServers"][name] = def.clone();
            n += 1;
        }
    }
    (mcp, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_follow_the_session_to_its_worktree() {
        let (m, w) = ("/Users/r/shop", "/Users/r/.wb/worktrees/shop/fix");
        assert_eq!(rules_for("Bash(npm run build:*)", m, w), vec!["Bash(npm run build:*)"]);
        assert_eq!(rules_for("Bash(/Users/r/shop/scripts/dev.sh:*)", m, w), vec!["Bash(/Users/r/shop/scripts/dev.sh:*)", "Bash(/Users/r/.wb/worktrees/shop/fix/scripts/dev.sh:*)"]);
        assert_eq!(rules_for("Edit(/src/**)", m, w), vec!["Edit(//Users/r/shop/src/**)", "Edit(//Users/r/.wb/worktrees/shop/fix/src/**)"]);
        assert_eq!(rules_for("Read(//etc/hosts)", m, w), vec!["Read(//etc/hosts)"]);
        assert_eq!(rules_for("WebFetch(domain:docs.rs)", m, w), vec!["WebFetch(domain:docs.rs)"]);
        assert_eq!(rules_for("mcp__github", m, w), vec!["mcp__github"]);
    }

    #[test]
    fn local_settings_are_merged_into_the_session() {
        let local = json!({
            "permissions": { "allow": ["Bash(npm run build:*)", "WebSearch"], "deny": ["Bash(curl:*)"], "defaultMode": "acceptEdits", "additionalDirectories": ["../shared", "~/notes"] },
            "enabledMcpjsonServers": ["chloectx"],
            "hooks": { "Stop": [] },
        });
        let s = json!({ "hooks": {}, "permissions": { "allow": ["Read(//w/**)"] } });
        let (out, c) = with_local(s, &local, Path::new("/r/shop"), Path::new("/w"), true);
        assert_eq!(out["permissions"]["allow"], json!(["Read(//w/**)", "Bash(npm run build:*)", "WebSearch"]));
        assert_eq!(out["permissions"]["deny"], json!(["Bash(curl:*)"]));
        assert_eq!(out["permissions"]["defaultMode"], "acceptEdits");
        assert_eq!(out["permissions"]["additionalDirectories"], json!(["/r/shop/../shared", "~/notes"]));
        assert_eq!(out["enabledMcpjsonServers"], json!(["chloectx"]));
        // Its hooks are not carried: Workbench wires ChloeCtx itself.
        assert_eq!(out["hooks"], json!({}));
        assert_eq!(c, Carried { rules: 3, mode: Some("acceptEdits".into()), dirs: 2, servers: 0 });
        // A read-only session keeps the default mode.
        let (ro, c) = with_local(json!({}), &local, Path::new("/r/shop"), Path::new("/w"), false);
        assert!(ro["permissions"]["defaultMode"].is_null());
        assert_eq!(c.mode, None);
        // Nothing to carry: nothing changes.
        let (same, c) = with_local(json!({ "hooks": {} }), &json!({}), Path::new("/r"), Path::new("/w"), true);
        assert_eq!(same, json!({ "hooks": {} }));
        assert!(c.is_empty());
    }

    #[test]
    fn local_servers_are_added_without_replacing() {
        let mut servers = serde_json::Map::new();
        servers.insert("github".into(), json!({ "type": "http", "url": "https://x" }));
        servers.insert("workbench".into(), json!({ "command": "other" }));
        let (m, n) = with_servers(json!({ "mcpServers": { "workbench": { "command": "wb" } } }), &servers);
        assert_eq!(n, 1);
        assert_eq!(m["mcpServers"]["workbench"]["command"], "wb");
        assert_eq!(m["mcpServers"]["github"]["url"], "https://x");
        assert_eq!(rank("auto"), Some(3));
        assert_eq!(rank("yolo"), None);
    }
}
