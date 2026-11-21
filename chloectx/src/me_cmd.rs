//! The commands of the personal space: `chloectx import` and `chloectx me
//! ...` ([`crate::me`], [`crate::remote`], [`crate::share`]). Each returns
//! the process's exit code.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value as Json};

use crate::import::{self, Origin};
use crate::{err, me, remote, share, Result};

fn fail(cmd: &str, e: impl std::fmt::Display) -> i32 {
    eprintln!("chloectx {cmd}: {e}");
    1
}

fn print(v: &Json, as_json: bool) {
    if as_json {
        println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
    } else {
        println!("{}", v["text"].as_str().unwrap_or(""));
    }
}

/// `chloectx import PATH`: reads an export and stores its chats in the
/// personal space, each message once.
pub fn import(path: &Path, from: Option<&str>, dry_run: bool, as_json: bool) -> i32 {
    let from = match from.map(|f| Origin::parse(f).ok_or(f)) {
        Some(Err(f)) => return fail("import", format!("--from is chatgpt, claude, gemini or jsonl, not {f:?}")),
        Some(Ok(o)) => Some(o),
        None => None,
    };
    let t0 = std::time::Instant::now();
    let chats = match import::read(path, from) {
        Ok(c) => c,
        Err(e) => return fail("import", e),
    };
    let summary = import::summary(&chats);
    if dry_run {
        if as_json {
            println!("{}", serde_json::to_string_pretty(&summary).unwrap_or_default());
        } else {
            println!("{} holds {} chats ({} messages); nothing was stored (--dry-run).", path.display(), summary["chats"], summary["messages"]);
            for s in summary["by_source"].as_array().into_iter().flatten() {
                println!("  {}: {} chats, {} messages", s["source"].as_str().unwrap_or(""), s["chats"], s["messages"]);
            }
        }
        return 0;
    }
    let (mut stored, mut records, mut known, mut changed) = (0u64, 0u64, 0u64, 0u64);
    let n = chats.len();
    let tty = std::io::IsTerminal::is_terminal(&std::io::stderr());
    for (i, c) in chats.into_iter().enumerate() {
        match me::ask("import", json!({"chat": c}), 600) {
            Ok(r) => {
                let m = r["messages"].as_u64().unwrap_or(0);
                stored += m;
                records += r["records"].as_u64().unwrap_or(0);
                known += r["known"].as_u64().unwrap_or(0);
                changed += (m > 0) as u64;
            }
            Err(e) => return fail("import", format!("after {i} of {n} chats: {e}")),
        }
        if tty && (i % 25 == 0 || i + 1 == n) {
            eprint!("\r{} of {n} chats", i + 1);
            let _ = std::io::stderr().flush();
        }
    }
    if tty {
        eprintln!();
    }
    let secs = t0.elapsed().as_secs_f64();
    let text = format!(
        "Imported {stored} new messages ({records} records) from {changed} of {n} chats in {secs:.1} s; {known} were kept already. They are in your personal memory ({}).\nNext: `chloectx me card build` drafts your persona card from them; `chloectx --me ask \"...\"` searches them.",
        me::store_dir().display()
    );
    print(&json!({"chats": n, "changed_chats": changed, "messages": stored, "records": records, "known": known, "seconds": secs, "summary": summary, "text": text}), as_json);
    0
}

/// `chloectx me`: what the space holds and who can reach it.
pub fn status(as_json: bool) -> i32 {
    match me::ask("status", json!({}), 60) {
        Ok(mut r) => {
            let shares: Vec<Json> = share::load().iter().map(|s| s.json()).collect();
            let active = shares.iter().filter(|s| s["state"] == "active").count();
            let signed = remote::Server::signed_in(&me::dir().join("oauth.json"));
            let mut text = r["text"].as_str().unwrap_or("").to_string();
            text += &format!("\nConnected assistants: {}. Shares: {active} active of {}.", if signed.is_empty() { "none (chloectx me serve, then add it as a connector)".to_string() } else { signed.iter().map(|s| s["client"].as_str().unwrap_or("").to_string()).collect::<Vec<_>>().join(", ") }, shares.len());
            if let Ok((_, fp)) = share::public_key() {
                text += &format!("\nSigning key: {fp}.");
                r["signing_key"] = json!(fp);
            }
            r["shares"] = json!(shares);
            r["connected"] = json!(signed);
            r["text"] = json!(text);
            print(&r, as_json);
            0
        }
        Err(e) => fail("me", e),
    }
}

pub fn card_show(as_json: bool) -> i32 {
    match me::ask("card", json!({}), 60) {
        Ok(r) => {
            print(&r, as_json);
            0
        }
        Err(e) => fail("me card", e),
    }
}

/// `chloectx me card build`: a model drafts the card from the person's own
/// messages; the lines they wrote or confirmed stay.
pub fn card_build(model: &str, max_chars: usize, dry_run: bool, as_json: bool) -> i32 {
    let input = match me::ask("card_input", json!({"max_chars": max_chars}), 120) {
        Ok(i) => i,
        Err(e) => return fail("me card build", e),
    };
    let msgs: Vec<Json> = input["messages"].as_array().cloned().unwrap_or_default();
    if msgs.is_empty() {
        return fail("me card build", "your personal memory holds no messages of yours yet: `chloectx import <export>` first");
    }
    let confirmed: Vec<Json> = input["confirmed"].as_array().cloned().unwrap_or_default();
    let known: std::collections::HashSet<String> = msgs.iter().filter_map(|m| m["id"].as_str().map(String::from)).collect();
    let days: std::collections::HashMap<String, String> = msgs.iter().filter_map(|m| Some((m["id"].as_str()?.to_string(), m["day"].as_str().unwrap_or("").to_string()))).collect();
    // Batches of about 40,000 characters (10,000 tokens).
    let mut batches: Vec<Vec<Json>> = vec![Vec::new()];
    let mut size = 0;
    for m in msgs {
        let n = m["text"].as_str().map_or(0, |t| t.len()) + 60;
        if size + n > 40_000 && !batches.last().unwrap().is_empty() {
            batches.push(Vec::new());
            size = 0;
        }
        size += n;
        batches.last_mut().unwrap().push(m);
    }
    let cli = crate::llm::Cli { command: "claude", model, timeout: Duration::from_secs(600), isolated: true };
    let mut cost = 0.0;
    let mut lines: Vec<Json> = Vec::new();
    for (i, b) in batches.iter().enumerate() {
        if batches.len() > 1 {
            eprintln!("drafting from batch {} of {} ({} messages)", i + 1, batches.len(), b.len());
        }
        match crate::llm::complete(&cli, me::DRAFT_SYSTEM, &me::draft_prompt(b, &confirmed)) {
            Ok(r) => {
                cost += r.cost_usd;
                lines.extend(me::parse_draft(&r.text, &known));
            }
            Err(e) => return fail("me card build", format!("the model call failed: {e}")),
        }
    }
    if batches.len() > 1 && !lines.is_empty() {
        let mut p = String::from("Drafts:\n");
        for l in &lines {
            let srcs: Vec<&str> = l["sources"].as_array().into_iter().flatten().filter_map(|s| s.as_str()).collect();
            let newest = srcs.iter().filter_map(|s| days.get(*s)).max().cloned().unwrap_or_default();
            p += &format!("- {} | {} | {} | sources: {} | newest: {newest}\n", l["section"].as_str().unwrap_or(""), l["kind"].as_str().unwrap_or(""), l["text"].as_str().unwrap_or(""), srcs.join(" "));
        }
        match crate::llm::complete(&cli, me::MERGE_SYSTEM, &p) {
            Ok(r) => {
                cost += r.cost_usd;
                let merged = me::parse_draft(&r.text, &known);
                if !merged.is_empty() {
                    lines = merged;
                }
            }
            Err(e) => eprintln!("chloectx me card build: merging the drafts failed ({e}); keeping them as they are"),
        }
    }
    if lines.is_empty() {
        return fail("me card build", "the model drafted no line that a message of yours backs");
    }
    if dry_run {
        let shown: Vec<Json> = lines.iter().map(|l| json!({"id": "", "section": l["section"], "text": l["text"], "confirmed": false, "sources": []})).collect();
        print(&json!({"lines": lines, "cost_usd": cost, "text": format!("{}\n\n(--dry-run: not stored; ${cost:.3})", me::card_text(&shown, false))}), as_json);
        return 0;
    }
    match me::ask("card_replace", json!({"lines": lines}), 120).and_then(|_| me::ask("card", json!({}), 60)) {
        Ok(mut r) => {
            r["cost_usd"] = json!(cost);
            r["text"] = json!(format!("{}\n\nDrafted {} lines from {} of your messages for ${cost:.3}. `chloectx me card edit` to change them; `chloectx me card confirm ID` to make one yours.", r["text"].as_str().unwrap_or(""), lines.len(), known.len()));
            print(&r, as_json);
            0
        }
        Err(e) => fail("me card build", e),
    }
}

/// `chloectx me card edit`: the card in `$EDITOR`, applied when it closes.
pub fn card_edit() -> i32 {
    let r = match me::ask("card", json!({}), 60) {
        Ok(r) => r,
        Err(e) => return fail("me card edit", e),
    };
    let lines: Vec<Json> = r["lines"].as_array().cloned().unwrap_or_default();
    let dir = match crate::paths::private_temp_dir("chloectx-card") {
        Ok(d) => d,
        Err(e) => return fail("me card edit", e),
    };
    let file = dir.join("persona-card.md");
    let before = me::card_text(&lines, true);
    if let Err(e) = std::fs::write(&file, &before) {
        return fail("me card edit", e);
    }
    let editor = std::env::var("VISUAL").or_else(|_| std::env::var("EDITOR")).unwrap_or_else(|_| "vi".into());
    let status = std::process::Command::new("sh").arg("-c").arg(format!("{editor} \"$1\"")).arg("sh").arg(&file).status();
    let after = std::fs::read_to_string(&file).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    if !status.is_ok_and(|s| s.success()) {
        return fail("me card edit", format!("{editor} did not exit cleanly; nothing was changed"));
    }
    let edits = me::parse_edit(&lines, &after);
    if edits.is_empty() {
        println!("No change.");
        return 0;
    }
    let mut done = Vec::new();
    for e in edits {
        let r = match &e {
            me::Edit::Add { section, text } => me::ask("card_add", json!({"section": section, "text": text}), 60),
            me::Edit::Change { id, section, text } => me::ask("card_edit", json!({"id": id, "section": section, "text": text}), 60),
            me::Edit::Drop { id } => me::ask("card_drop", json!({"id": id}), 60),
        };
        match r {
            Ok(r) => done.push(r["text"].as_str().unwrap_or("").to_string()),
            Err(x) => return fail("me card edit", format!("{x} (after {} change(s))", done.len())),
        }
    }
    println!("{}", done.join("\n"));
    0
}

pub fn card_op(what: &str, args: Json) -> i32 {
    match me::ask(what, args, 60) {
        Ok(r) => {
            println!("{}", r["text"].as_str().unwrap_or(""));
            0
        }
        Err(e) => fail("me card", e),
    }
}

/// `chloectx me connect`: the passcode and capture key (made, or made
/// again, here and shown once), and who is signed in.
pub fn connect(new_passcode: bool, new_capture_key: bool, signout: bool) -> i32 {
    let mut s = remote::load_secrets();
    let mut shown = Vec::new();
    if new_passcode || s.passcode_hash.is_empty() {
        shown.push(format!("Passcode (to sign an assistant in): {}", remote::reset_passcode(&mut s)));
    }
    if new_capture_key || s.capture_key_hash.is_empty() {
        shown.push(format!("Capture key (for the browser extension and the API proxy): {}", remote::reset_capture_key(&mut s)));
    }
    if !shown.is_empty() {
        if let Err(e) = remote::save_secrets(&s) {
            return fail("me connect", e);
        }
        println!("{}\nThey are shown once; ChloeCtx keeps only their hashes. `chloectx me connect --new-passcode` or `--new-capture-key` makes new ones.\n", shown.join("\n"));
    }
    let state = me::dir().join("oauth.json");
    if signout {
        match remote::Server::signout_all(&state) {
            Ok(n) => println!("Signed out every assistant ({n} registered clients); each must sign in again. A `chloectx me serve` that is running keeps its tokens until it restarts."),
            Err(e) => return fail("me connect", e),
        }
        return 0;
    }
    let signed = remote::Server::signed_in(&state);
    if signed.is_empty() {
        println!("No assistant is signed in. Run `chloectx me serve` and add its address as a connector (see `chloectx help me serve`).");
    } else {
        println!("Signed in:");
        for c in signed {
            println!("  {} ({}), since {}", c["client"].as_str().unwrap_or(""), c["scope"].as_str().unwrap_or(""), crate::recall::fmt_time(c["since"].as_i64().unwrap_or(0)));
        }
    }
    0
}

pub struct ServeOptions {
    pub host: String,
    pub port: u16,
    pub public_url: Option<String>,
    pub openai_base: String,
    pub anthropic_base: String,
    pub shares: bool,
}

/// `chloectx me serve`: the remote MCP server, the capture endpoint and the
/// API proxy, until the process is stopped.
pub fn serve(o: ServeOptions) -> i32 {
    if let Err(e) = me::ensure() {
        return fail("me serve", e);
    }
    let mut s = remote::load_secrets();
    let mut fresh = Vec::new();
    if s.passcode_hash.is_empty() {
        fresh.push(format!("Passcode (to sign an assistant in): {}", remote::reset_passcode(&mut s)));
    }
    if s.capture_key_hash.is_empty() {
        fresh.push(format!("Capture key (for the browser extension and the API proxy): {}", remote::reset_capture_key(&mut s)));
    }
    if !fresh.is_empty() {
        if let Err(e) = remote::save_secrets(&s) {
            return fail("me serve", e);
        }
    }
    if let Some(u) = &o.public_url {
        if !remote::public_url_ok(u) {
            return fail("me serve", format!("--public-url is an https:// address (ChatGPT and Claude.ai connect over HTTPS), not {u}"));
        }
    }
    let listener = match std::net::TcpListener::bind((o.host.as_str(), o.port)) {
        Ok(l) => l,
        Err(e) => return fail("me serve", format!("cannot listen on {}:{}: {e}", o.host, o.port)),
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(o.port);
    // Started now, so the first call does not wait for it.
    if let Err(e) = me::ask("status", json!({}), 120) {
        return fail("me serve", e);
    }
    let cfg = remote::Config {
        public_url: o.public_url.clone(),
        secrets: s,
        state_path: Some(me::dir().join("oauth.json")),
        shares: o.shares,
        openai_base: o.openai_base,
        anthropic_base: o.anthropic_base,
        log_path: Some(me::dir().join("access.log")),
        ..Default::default()
    };
    let server = remote::Server::new(cfg, Arc::new(remote::LocalSpace));
    let local = format!("http://{}:{port}", if o.host == "0.0.0.0" { "127.0.0.1" } else { o.host.as_str() });
    let public = o.public_url.clone().unwrap_or_else(|| local.clone());
    println!("ChloeCtx personal memory is served at {local}");
    if !fresh.is_empty() {
        println!("\n{}\nThey are shown once; ChloeCtx keeps only their hashes.", fresh.join("\n"));
    }
    println!(
        "\nMCP server: {public}/mcp (sign in with the passcode)\n  ChatGPT: Settings, Connectors, Advanced settings, Developer mode on; then Create, with this address and OAuth.\n  Claude.ai: Settings, Connectors, Add custom connector, with this address.\n  Claude Code: claude mcp add --transport http me {public}/mcp\nBrowser extension: endpoint {public}, with the capture key.\nAPI proxy: OPENAI_BASE_URL={local}/p/<capture key>/openai/v1, ANTHROPIC_BASE_URL={local}/p/<capture key>/anthropic"
    );
    if o.public_url.is_none() {
        println!("\nChatGPT and Claude.ai reach a connector over HTTPS at a public address: run a tunnel to {local} (cloudflared tunnel --url {local}, or tailscale funnel {port}) and start this again with --public-url https://<that address>.");
    }
    server.serve(listener);
    0
}

pub struct ShareOptions {
    pub name: String,
    pub sections: Vec<String>,
    pub topics: Vec<String>,
    pub no_card: bool,
    pub sources: bool,
    pub per_topic: usize,
    pub expires: String,
    pub out: Option<PathBuf>,
    pub public_url: Option<String>,
}

pub fn share_create(o: ShareOptions) -> i32 {
    let expires_in = match share::parse_duration(&o.expires) {
        Ok(e) => e,
        Err(e) => return fail("me share create", e),
    };
    let out = o.out.map(|p| std::path::absolute(&p).unwrap_or(p));
    let opts = share::CreateOptions { name: o.name, sections: o.sections, topics: o.topics, card: !o.no_card, sources: o.sources, per_topic: o.per_topic, expires_in, out: out.clone() };
    match share::create(&opts) {
        Ok((s, key)) => {
            let base = o.public_url.unwrap_or_else(|| "https://<your chloectx me serve address>".into());
            println!(
                "Shared {:?}: {} records, signed by key {}{}.\nIts address, which is its key (shown once; keep it private):\n  {base}/s/{key}/mcp\nAnyone with it can add it as an MCP server (a ChatGPT or Claude connector with no sign-in) while `chloectx me serve` runs. `chloectx me share log {}` shows every call; `chloectx me share revoke {}` ends it.",
                s.name,
                s.records,
                s.signer.clone().unwrap_or_default(),
                s.expires.map(|e| format!(", until {}", crate::recall::fmt_time(e))).unwrap_or_else(|| ", until revoked".into()),
                s.id,
                s.id
            );
            if let Some(out) = out {
                println!("The signed capsule is also in {} (chloectx mount {} reads it).", out.display(), out.display());
            }
            0
        }
        Err(e) => fail("me share create", e),
    }
}

pub fn share_list(as_json: bool) -> i32 {
    let all: Vec<Json> = share::load().iter().map(|s| s.json()).collect();
    if as_json {
        println!("{}", serde_json::to_string_pretty(&all).unwrap_or_default());
        return 0;
    }
    if all.is_empty() {
        println!("No shares. `chloectx me share create NAME --sections ... --topics ...` makes one.");
    }
    for s in all {
        let calls = share::read_log(s["id"].as_str().unwrap_or(""), 100_000).len();
        println!("{}  {:?}  {}  {} records  {} calls{}", s["id"].as_str().unwrap_or(""), s["name"].as_str().unwrap_or(""), s["state"].as_str().unwrap_or(""), s["records"], calls,
            s["expires"].as_i64().map(|e| format!("  until {}", crate::recall::fmt_time(e))).unwrap_or_default());
    }
    0
}

pub fn share_revoke(which: &str) -> i32 {
    match share::revoke(which) {
        Ok(s) => {
            println!("Revoked {:?} ({}): its address answers no more, and its capsule is removed. Its access log stays (chloectx me share log {}).", s.name, s.id, s.id);
            0
        }
        Err(e) => fail("me share revoke", e),
    }
}

pub fn share_log(which: &str, n: usize, as_json: bool) -> i32 {
    let all = share::load();
    let Some(s) = all.iter().find(|s| s.id == which || s.name == which) else { return fail("me share log", format!("no share {which:?}")) };
    let log = share::read_log(&s.id, n);
    if as_json {
        println!("{}", serde_json::to_string_pretty(&log).unwrap_or_default());
        return 0;
    }
    if log.is_empty() {
        println!("Nobody has used {:?} yet.", s.name);
    }
    for e in log {
        println!("{}  {}  {}  {} records  from {}", crate::recall::fmt_time(e["ts"].as_i64().unwrap_or(0)), e["tool"].as_str().unwrap_or(""), e["args"], e["records"].as_array().map_or(0, |a| a.len()), e["from"].as_str().unwrap_or(""));
    }
    0
}

/// The calls connected assistants made (`<me>/access.log`).
pub fn access_log(n: usize) -> Result<Vec<Json>> {
    let text = std::fs::read_to_string(me::dir().join("access.log")).map_err(|e| err(format!("no calls yet ({e})")))?;
    let all: Vec<Json> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    Ok(all[all.len().saturating_sub(n)..].to_vec())
}
