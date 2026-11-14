//! ChloeCtx in every session (SPEC.md, "ChloeCtx is required"). Most of
//! what Workbench shows above the terminal comes from ChloeCtx: decisions,
//! blame, recall, the story's "what it knew". So each session Workbench
//! starts gets ChloeCtx's hooks and MCP server in its own `--settings` and
//! `--mcp-config` files, from `chloectx wiring`, unless the user's or the
//! project's settings already wire it (then nothing is added, and the
//! session is recorded once). Nothing of the user's is written.
//!
//! Without a usable ChloeCtx, `serve` still starts and the UI shows a setup
//! screen; starting an agent is refused with `chloectx_missing` and the fix.
//! `--no-chloectx` (hidden, for tests) turns this off.

use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json as AxJson, Router};
use serde_json::{json, Value as Json};

use crate::server::{blocking, ApiResult, App};
use crate::supervisor::ApiError;
use crate::trail::{run_chloectx, run_raw, Ran};

/// The command that installs both programs, from a checkout.
pub const FIX: &str = "./install.sh --workbench   (from a checkout of https://github.com/chloetech-ai/chloedb)";

/// The ChloeCtx program to run: `bin` as given when it is a path; else
/// found on PATH; else `~/.local/bin/chloectx`, where install.sh puts it
/// (a shell that started Workbench may not have that on PATH).
pub fn resolve(bin: &str) -> String {
    if bin.contains('/') {
        return bin.to_string();
    }
    let on_path = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(bin)).find(|f| is_exec(f)));
    if let Some(p) = on_path {
        return p.to_string_lossy().into_owned();
    }
    if let Some(home) = std::env::var_os("HOME") {
        let p = PathBuf::from(home).join(".local/bin").join(bin);
        if is_exec(&p) {
            return p.to_string_lossy().into_owned();
        }
    }
    bin.to_string()
}

fn is_exec(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata().map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

/// What a session in `cwd` gets from ChloeCtx (`chloectx wiring`), or why
/// it cannot get it, in a sentence with the fix.
pub fn wiring(bin: &str, cwd: &Path) -> Result<Json, String> {
    let exe = if bin.contains('/') { std::fs::canonicalize(bin).map(|p| p.to_string_lossy().into_owned()).unwrap_or(bin.to_string()) } else { bin.to_string() };
    let cwd_s = cwd.to_string_lossy().into_owned();
    match run_chloectx(bin, cwd, &["wiring", "--project", &cwd_s, "--exe", &exe], Duration::from_secs(20)) {
        Ran::Out(w) if w.get("settings").is_some() => Ok(w),
        Ran::Out(_) | Ran::Failed(_) => Err(format!("{bin} is too old for Workbench: it has no `wiring` command")),
        Ran::Missing => Err(format!("ChloeCtx is not installed: {bin} was not found")),
    }
}

/// The check the setup screen shows: whether sessions will get ChloeCtx.
pub fn status(bin: &str, required: bool, cwd: &Path) -> Json {
    if !required {
        return json!({"required": false, "ok": true, "bin": bin});
    }
    let version = match run_raw(bin, cwd, &["--version"], Duration::from_secs(10)) {
        Ran::Out(Json::String(v)) => v.trim().to_string(),
        _ => String::new(),
    };
    match wiring(bin, cwd) {
        Ok(w) => {
            let model_ok = w.pointer("/model/ok").and_then(Json::as_bool).unwrap_or(true);
            let problem = (!model_ok).then(|| format!("ChloeCtx's embedding model is missing ({}), so recall finds nothing", w.pointer("/model/path").and_then(Json::as_str).unwrap_or("")));
            json!({
                "required": true,
                "ok": model_ok,
                "bin": bin,
                "version": version,
                "wired": w["wired"],
                "model": w["model"],
                "problem": problem,
                "fix": (!model_ok).then(|| format!("{bin} fetch-model")),
            })
        }
        Err(why) => json!({"required": true, "ok": false, "bin": bin, "version": version, "problem": why, "fix": "./install.sh --workbench"}),
    }
}

/// `base` (a session's settings) with ChloeCtx's `add` merged in: hooks
/// appended per event, permissions' allow joined, env added.
pub fn merge_settings(mut base: Json, add: &Json) -> Json {
    if let Some(hooks) = add.get("hooks").and_then(Json::as_object) {
        if !base["hooks"].is_object() {
            base["hooks"] = json!({});
        }
        for (event, entries) in hooks {
            let list = base["hooks"].get(event).and_then(Json::as_array).cloned().unwrap_or_default();
            let mut list = list;
            list.extend(entries.as_array().cloned().unwrap_or_default());
            base["hooks"][event] = Json::Array(list);
        }
    }
    if let Some(allow) = add.pointer("/permissions/allow").and_then(Json::as_array) {
        if !base["permissions"].is_object() {
            base["permissions"] = json!({});
        }
        let mut list = base["permissions"].get("allow").and_then(Json::as_array).cloned().unwrap_or_default();
        for a in allow {
            if !list.contains(a) {
                list.push(a.clone());
            }
        }
        base["permissions"]["allow"] = Json::Array(list);
    }
    if let Some(env) = add.get("env").and_then(Json::as_object) {
        if !base["env"].is_object() {
            base["env"] = json!({});
        }
        for (k, v) in env {
            base["env"][k] = v.clone();
        }
    }
    base
}

/// `base` (a session's MCP config) with ChloeCtx's servers added.
pub fn merge_mcp(mut base: Json, add: &Json) -> Json {
    if let Some(servers) = add.as_object() {
        for (name, s) in servers {
            base["mcpServers"][name] = s.clone();
        }
    }
    base
}

/// The settings and MCP config of a new session in `cwd`, wired to ChloeCtx
/// when it is required.
pub fn wire(bin: &str, required: bool, cwd: &Path, settings: Json, mcp: Json) -> Result<(Json, Json), ApiError> {
    if !required {
        return Ok((settings, mcp));
    }
    let w = wiring(bin, cwd).map_err(missing)?;
    Ok((merge_settings(settings, &w["settings"]), merge_mcp(mcp, &w["mcp"])))
}

/// The answer to a spawn without ChloeCtx: why, and the fix.
pub fn missing(why: String) -> ApiError {
    ApiError::new(409, "chloectx_missing", format!("{why}. Workbench needs it; run {FIX}"))
}

pub fn routes() -> Router<App> {
    Router::new().route("/api/chloectx", get(status_route))
}

async fn status_route(State(app): State<App>) -> ApiResult {
    let sup = app.sup.clone();
    let st = blocking(move || Ok(status(&sup.cfg.chloectx_bin, sup.cfg.chloectx_required, &sup.cfg.data_dir))).await?;
    Ok(AxJson(st).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_chloectx_into_a_sessions_files() {
        let base = crate::supervisor::settings_json("/wb");
        let add = json!({
            "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "/c hook Stop", "timeout": 30}]}], "PostCompact": [{"hooks": []}]},
            "permissions": {"allow": ["mcp__chloectx"]},
            "env": {"CHLOECTX_DISTILL_MODEL": "haiku"},
        });
        let s = merge_settings(base, &add);
        let stop = s["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2, "{s}");
        assert!(stop[0]["hooks"][0]["command"].as_str().unwrap().contains("/wb"));
        assert_eq!(stop[1]["hooks"][0]["command"], "/c hook Stop");
        assert!(s["hooks"]["PostCompact"].is_array());
        assert_eq!(s["permissions"]["allow"], json!(["mcp__chloectx"]));
        assert_eq!(s["env"]["CHLOECTX_DISTILL_MODEL"], "haiku");
        let m = merge_mcp(crate::supervisor::mcp_json("/wb", "http://x", "t", "a1"), &json!({"chloectx": {"command": "/c", "args": ["mcp"]}}));
        assert_eq!(m["mcpServers"]["workbench"]["command"], "/wb");
        assert_eq!(m["mcpServers"]["chloectx"]["command"], "/c");
    }
}
