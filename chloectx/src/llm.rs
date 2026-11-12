//! One-shot calls to a model through the `claude` CLI in print mode, shared
//! by distillation, consolidation and the query engine.
//!
//! The call runs without tools, from a scratch directory and with ChloeCtx
//! disabled, so it records nothing itself. The scratch directory is
//! `<home>/distill` ([`scratch_dir`]), which only this user may open: the
//! CLI loads project settings (hooks among them) and `CLAUDE.md` from the
//! directory it runs in, so a shared one such as `/tmp/chloectx-distill`
//! would let another local user run commands as this one, or steer every
//! note the distiller writes. With `isolated` it also skips the
//! user's settings and MCP servers: their plugins, hooks and tool definitions
//! would add tens of thousands of tokens to every call (and another memory
//! plugin would record it).

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value as Json;

use crate::{err, Result};

/// How to reach the model.
#[derive(Clone, Debug)]
pub struct Cli<'a> {
    /// The CLI to call (`claude`).
    pub command: &'a str,
    /// Its `--model` argument; empty: the CLI's default.
    pub model: &'a str,
    pub timeout: Duration,
    pub isolated: bool,
}

/// Tokens a call read and wrote, as the CLI reports them.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize)]
pub struct Usage {
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub output: u64,
}

impl Usage {
    fn of(v: &Json) -> Usage {
        let n = |k: &str| v.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
        Usage { input: n("input_tokens"), cache_read: n("cache_read_input_tokens"), cache_write: n("cache_creation_input_tokens"), output: n("output_tokens") }
    }

    pub fn add(&mut self, o: Usage) {
        self.input += o.input;
        self.cache_read += o.cache_read;
        self.cache_write += o.cache_write;
        self.output += o.output;
    }

    /// For event logs.
    pub fn json(&self) -> Json {
        serde_json::json!({"input": self.input, "cache_read": self.cache_read, "cache_write": self.cache_write, "output": self.output})
    }
}

/// The model's reply and what the call cost.
#[derive(Clone, Debug, Default)]
pub struct Reply {
    pub text: String,
    pub cost_usd: f64,
    pub usage: Usage,
    pub ms: u64,
}

/// The directory model calls run in: `distill` in ChloeCtx's home, made
/// private to this user ([`crate::paths::ensure_private_dir`]); an error
/// when it cannot be, rather than a call from a directory someone else can
/// write to.
pub fn scratch_dir() -> Result<PathBuf> {
    let home = crate::paths::home();
    crate::paths::create_private_dir(&home)?;
    let dir = home.join("distill");
    crate::paths::ensure_private_dir(&dir).map_err(|e| err(format!("the model's scratch directory: {e}")))?;
    Ok(dir)
}

/// Runs the model once on `prompt` with `system` as its system prompt.
pub fn complete(cli: &Cli, system: &str, prompt: &str) -> Result<Reply> {
    let t0 = Instant::now();
    let scratch = scratch_dir()?;
    let mut cmd = Command::new(cli.command);
    cmd.args(["-p", "--system-prompt", system, "--tools", "", "--no-session-persistence", "--output-format", "json"]);
    if cli.isolated {
        cmd.args(["--setting-sources", "project", "--strict-mcp-config"]);
    }
    if !cli.model.is_empty() {
        cmd.args(["--model", cli.model]);
    }
    let mut child = cmd
        .current_dir(&scratch)
        .env("CHLOECTX_DISABLE", "1")
        .env("ENGRAM_DISABLE", "1") // read by the hooks of a binary from before the rename
        // Cloud sessions pass their memory stores on to child CLIs: in a
        // distill or query engine call they are ~7,000 tokens of the user's
        // notes the call does not need.
        .env_remove("CLAUDE_MEMORY_STORES")
        .env_remove("CLAUDE_COWORK_MEMORY_PATH_OVERRIDE")
        // Every call's prompt is new, so the CLI's cache writes are never
        // read back; they are billed above plain input (half the cost of a
        // 15,000-token call).
        .env("DISABLE_PROMPT_CACHING", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| err(format!("{}: {e}", cli.command)))?;
    // The prompt is written, and the answer read, on threads of their own
    // while this one watches the clock: a reply larger than the pipe's
    // buffer (64 KB on Linux) would otherwise block the child on its write
    // until the timeout killed it.
    let mut stdin = child.stdin.take().unwrap();
    let input = prompt.as_bytes().to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        stdout.read_to_end(&mut buf).map(|_| buf)
    });
    let deadline = Instant::now() + cli.timeout;
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(err("model call timed out"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = writer.join();
    // The child has exited; its stdout closes unless a process it started
    // still holds it, which the deadline bounds as well.
    while !reader.is_finished() && Instant::now() <= deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    if !reader.is_finished() {
        return Err(err("model call timed out"));
    }
    let stdout = reader.join().map_err(|_| err("model output: the reader failed"))??;
    let v: Json = serde_json::from_slice(&stdout).map_err(|e| err(format!("model output: {e}")))?;
    if v.get("is_error").and_then(|b| b.as_bool()).unwrap_or(false) {
        return Err(err(format!("model call failed: {}", v.get("result").and_then(|r| r.as_str()).unwrap_or(""))));
    }
    Ok(Reply {
        text: v.get("result").and_then(|r| r.as_str()).unwrap_or("").to_string(),
        cost_usd: v.get("total_cost_usd").and_then(|c| c.as_f64()).unwrap_or(0.0),
        usage: v.get("usage").map(Usage::of).unwrap_or_default(),
        ms: t0.elapsed().as_millis() as u64,
    })
}

/// Runs the model once through Anthropic's Messages API with `key`
/// (`ANTHROPIC_BASE_URL` overrides where it is sent): for a process with no
/// `claude` CLI, such as `chloectx app` in a cluster.
pub fn complete_api(key: &str, model: &str, system: &str, prompt: &str, timeout: Duration) -> Result<Reply> {
    let t0 = Instant::now();
    let base = std::env::var("ANTHROPIC_BASE_URL").unwrap_or_else(|_| "https://api.anthropic.com".into());
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(timeout)).http_status_as_error(false).build().into();
    let body = serde_json::json!({
        "model": model, "max_tokens": 4096, "system": system,
        "messages": [{"role": "user", "content": prompt}],
    });
    let mut r = agent
        .post(format!("{}/v1/messages", base.trim_end_matches('/')))
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .send(body.to_string())
        .map_err(|e| err(format!("model call: {e}")))?;
    let status = r.status().as_u16();
    let text = r.body_mut().read_to_string().map_err(|e| err(format!("model call: {e}")))?;
    let v: Json = serde_json::from_str(&text).unwrap_or(Json::Null);
    if !(200..300).contains(&status) {
        let m = v["error"]["message"].as_str().map(str::to_string).unwrap_or_else(|| text.chars().take(300).collect());
        return Err(err(format!("model call failed (HTTP {status}): {m}")));
    }
    let out: String = v["content"].as_array().into_iter().flatten().filter_map(|c| c["text"].as_str()).collect::<Vec<_>>().join("");
    Ok(Reply { text: out, cost_usd: 0.0, usage: Usage::of(&v["usage"]), ms: t0.elapsed().as_millis() as u64 })
}

/// The first JSON object in `text` (models sometimes wrap it in prose or a
/// code fence).
pub fn json_object(text: &str) -> Option<Json> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end <= start {
        return None;
    }
    serde_json::from_str(&text[start..=end]).ok().or_else(|| {
        // A trailing object after the first: take the longest prefix that parses.
        let mut de = serde_json::Deserializer::from_str(&text[start..]).into_iter::<Json>();
        de.next().and_then(|r| r.ok())
    })
}

/// `s` cut to at most `n` bytes at a character boundary, with an ellipsis.
pub fn clip(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let mut i = n;
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    format!("{}…", &s[..i])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_object() {
        assert_eq!(json_object("Sure:\n```json\n{\"a\": 1}\n```").unwrap()["a"], 1);
        assert_eq!(json_object("{\"a\": {\"b\": 2}} and {\"c\": 3}").unwrap()["a"]["b"], 2);
        assert!(json_object("no json").is_none());
        assert_eq!(clip("héllo", 2), "h…");
    }

    #[test]
    fn reads_usage() {
        let v: Json = serde_json::from_str(r#"{"input_tokens": 3, "cache_read_input_tokens": 1000, "cache_creation_input_tokens": 50, "output_tokens": 120}"#).unwrap();
        let mut u = Usage::of(&v);
        assert_eq!(u, Usage { input: 3, cache_read: 1000, cache_write: 50, output: 120 });
        u.add(Usage { input: 1, ..Default::default() });
        assert_eq!(u.json()["input"], 4);
    }
}
