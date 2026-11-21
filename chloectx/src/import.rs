//! `chloectx import`: a person's chats from other assistants, read from the
//! exports those assistants give, as records of the personal space
//! ([`crate::me`]).
//!
//! | source | what to export | the file read |
//! |---|---|---|
//! | ChatGPT | Settings, Data controls, Export data (a zip by mail) | `conversations.json` (each conversation's tree, walked from its current message back to its root) |
//! | Claude.ai | Settings, Privacy, Export data (a zip by mail) | `conversations.json` (`chat_messages`), and `memories.json` when there is one |
//! | Gemini | Google Takeout, My Activity, Gemini Apps (JSON or HTML) | `My Activity/Gemini Apps/MyActivity.json` or `.html`; prompts close in time make one chat |
//! | anything else | one JSON object per line | `{"source", "conversation", "title"?, "url"?, "role": "user"\|"assistant", "text", "ts"?}` |
//!
//! A zip is read with `unzip` (on macOS and most Linux), a folder is
//! searched for those files, and a file is read as it is. Everything comes
//! out as [`Chat`]s, each a conversation of user and assistant messages;
//! system prompts, tool calls and hidden messages stay behind, and
//! ChatGPT's custom instructions become a chat of their own (they say who
//! the person is, in their words).
//!
//! The records are those the hooks write for a Claude Code turn ([`batches`]):
//! a prompt (and the rules it states) for each user message, an answer for
//! each reply, with the chat's name and address in their context
//! (`From: ChatGPT "Title" <url>`), so a search finds a chat by its name and
//! every record says where it came from. Each message is kept once: a chat's
//! messages are known by a hash of their role and text (in an `event`
//! record tagged `chat`, which search never shows), so importing the same
//! export again, or a newer one, or capturing the same conversation from
//! the browser ([`crate::remote`]), stores only what is new.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

use crate::record::{Draft, Kind, Source as By};
use crate::{err, Result};

/// The tag of a chat's `event` records ([`chat_event`]).
pub const CHAT_TAG: &str = "chat";

/// Where a chat came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Chatgpt,
    Claude,
    Gemini,
    /// The API proxy ([`crate::remote`]): an app that calls OpenAI's API.
    Openai,
    /// The API proxy: an app that calls Anthropic's API.
    Anthropic,
    /// Anything else (the JSON-lines format, or a source this build does not name).
    Other,
}

impl Origin {
    pub fn parse(s: &str) -> Option<Origin> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "chatgpt" | "chatgpt.com" | "openai-chat" => Origin::Chatgpt,
            "claude" | "claude.ai" | "claude-ai" => Origin::Claude,
            "gemini" | "gemini.google.com" | "bard" => Origin::Gemini,
            "openai" | "openai-api" => Origin::Openai,
            "anthropic" | "anthropic-api" => Origin::Anthropic,
            "other" | "jsonl" => Origin::Other,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Chatgpt => "chatgpt",
            Origin::Claude => "claude",
            Origin::Gemini => "gemini",
            Origin::Openai => "openai",
            Origin::Anthropic => "anthropic",
            Origin::Other => "other",
        }
    }

    /// Its name, as the records' context and the persona card say it.
    pub fn label(self) -> &'static str {
        match self {
            Origin::Chatgpt => "ChatGPT",
            Origin::Claude => "Claude",
            Origin::Gemini => "Gemini",
            Origin::Openai => "an app on OpenAI's API",
            Origin::Anthropic => "an app on Anthropic's API",
            Origin::Other => "another assistant",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Msg {
    pub role: Role,
    pub text: String,
    /// Unix seconds; 0 when the source does not say (the chat's own time is used).
    #[serde(default)]
    pub ts: i64,
}

/// One conversation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chat {
    pub origin: Origin,
    /// The source's own id of the conversation.
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub created: i64,
    pub messages: Vec<Msg>,
}

impl Chat {
    /// The session its records are kept under: the source's id, cleaned
    /// (record ids start with its first characters).
    pub fn session(&self) -> String {
        let clean: String = self.id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(64).collect();
        if clean.chars().filter(|c| c.is_ascii_alphanumeric()).count() >= 6 {
            clean
        } else {
            format!("{}{:08x}", &self.origin.as_str()[..2], crate::embed::fnv(&format!("{}\0{}", self.origin.as_str(), self.id)) as u32)
        }
    }

    /// `From: ChatGPT "Title" <url>`, the context line of each record.
    pub fn from_line(&self) -> String {
        let mut s = format!("From: {} {:?}", self.origin.label(), clip(&self.title, 120));
        if let Some(u) = &self.url {
            s += &format!(" <{u}>");
        }
        s
    }
}

fn clip(s: &str, n: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= n {
        s
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

/// The hash a message is known by: its role and text.
pub fn msg_hash(m: &Msg) -> String {
    let r = match m.role {
        Role::User => "u",
        Role::Assistant => "a",
    };
    format!("{:08x}", crate::embed::fnv(&format!("{r}\0{}", m.text.trim())) as u32)
}

/// The drafts of one message, at one time, as a turn of the chat.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Batch {
    pub session: String,
    pub turn: i64,
    pub ts: i64,
    pub drafts: Vec<Draft>,
}

/// The records of `chat`'s messages not in `seen`, in order, and the
/// hashes of those messages. A user message is a turn: its prompt and the
/// rules it states ([`crate::capture::prompt_drafts`]); a reply is the
/// turn's answer, with the turn's request in its context.
pub fn batches(chat: &Chat, seen: &HashSet<String>) -> (Vec<Batch>, Vec<String>) {
    let session = chat.session();
    let from = chat.from_line();
    let mut out = Vec::new();
    let mut new = Vec::new();
    let mut turn = 0i64;
    let mut prompt = String::new();
    let mut last_ts = chat.created;
    for m in &chat.messages {
        let text = m.text.trim();
        if text.is_empty() {
            continue;
        }
        if m.role == Role::User {
            turn += 1;
            prompt = text.to_string();
        }
        let ts = if m.ts > 0 { m.ts } else { last_ts };
        last_ts = ts.max(last_ts);
        let h = msg_hash(m);
        if seen.contains(&h) || new.contains(&h) {
            continue;
        }
        new.push(h);
        let drafts: Vec<Draft> = match m.role {
            Role::User => crate::capture::prompt_drafts(text).into_iter().map(|d| with_ctx(d, &from)).collect(),
            Role::Assistant => {
                let req = format!("Turn {turn} request: {}", prompt.chars().take(200).collect::<String>());
                crate::capture::answer_drafts(text).into_iter().map(|d| with_ctx(d, &format!("{from}\n{req}")).by(&By::Assistant(chat.origin.label().chars().filter(|c| c.is_alphanumeric()).collect()))).collect()
            }
        };
        if !drafts.is_empty() {
            out.push(Batch { session: session.clone(), turn: turn.max(1), ts, drafts });
        }
    }
    (out, new)
}

fn with_ctx(d: Draft, line: &str) -> Draft {
    let ctx = if d.ctx.is_empty() { line.to_string() } else { format!("{line}\n{}", d.ctx) };
    d.ctx(ctx)
}

/// The `event` record that says which of `chat`'s messages are kept now
/// (`hashes`, the ones this import added).
pub fn chat_event(chat: &Chat, hashes: &[String], total: usize) -> Draft {
    let mut ctx = format!("Origin: {}\nConversation: {}\nTitle: {}\nMessages: {total}\nSeen: {}", chat.origin.as_str(), clip(&chat.id, 120), clip(&chat.title, 200), hashes.join(" "));
    if let Some(u) = &chat.url {
        ctx += &format!("\nUrl: {u}");
    }
    Draft::new(Kind::Event, format!("{} conversation {:?}: {total} messages", chat.origin.label(), clip(&chat.title, 120))).tag(CHAT_TAG).ctx(ctx)
}

/// The hashes a chat's `event` records' contexts list.
pub fn seen_in(ctx: &str) -> impl Iterator<Item = &str> {
    ctx.lines().filter_map(|l| l.strip_prefix("Seen: ")).flat_map(|l| l.split_whitespace())
}

// ---------------------------------------------------------------------------
// Reading exports.

/// What an export holds, read from a file, a folder or a zip. `from` names
/// the source when the files do not say it.
pub fn read(path: &Path, from: Option<Origin>) -> Result<Vec<Chat>> {
    let meta = std::fs::metadata(path).map_err(|e| err(format!("{}: {e}", path.display())))?;
    let mut chats = Vec::new();
    if meta.is_dir() {
        let files = find_files(path, 5);
        if files.is_empty() {
            return Err(err(format!("{} holds no conversations.json, memories.json or MyActivity file of an export", path.display())));
        }
        for f in files {
            chats.extend(read_named(&f.to_string_lossy(), &std::fs::read(&f)?, from)?);
        }
    } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) {
        let names = zip_names(path)?;
        let wanted: Vec<&String> = names.iter().filter(|n| export_file(n)).collect();
        if wanted.is_empty() {
            return Err(err(format!("{} holds no conversations.json or MyActivity file: is it a ChatGPT, Claude.ai or Google Takeout export?", path.display())));
        }
        for n in wanted {
            let bytes = Command::new("unzip").arg("-p").arg(path).arg(n).output().map_err(|e| err(format!("unzip: {e}")))?;
            if !bytes.status.success() {
                return Err(err(format!("unzip -p {} {n} failed", path.display())));
            }
            chats.extend(read_named(n, &bytes.stdout, from)?);
        }
    } else {
        chats = read_named(&path.to_string_lossy(), &std::fs::read(path)?, from)?;
    }
    Ok(chats)
}

/// `~` and `~/...` as the home folder.
pub fn expand_home(p: &str) -> PathBuf {
    let p = p.trim();
    match (p.strip_prefix('~'), std::env::var_os("HOME")) {
        (Some(rest), Some(h)) if rest.is_empty() || rest.starts_with('/') => PathBuf::from(h).join(rest.trim_start_matches('/')),
        _ => PathBuf::from(p),
    }
}

/// Where people's downloads land: Downloads, Desktop and Documents.
pub fn export_places() -> Vec<PathBuf> {
    let Some(h) = std::env::var_os("HOME") else { return Vec::new() };
    ["Downloads", "Desktop", "Documents"].iter().map(|d| PathBuf::from(&h).join(d)).filter(|d| d.is_dir()).collect()
}

/// The exports in `places` (two folders deep): zips that hold an export's
/// files, and folders that do, newest first, at most 50.
pub fn find_exports(places: &[PathBuf]) -> Vec<Json> {
    let mut found: Vec<(u64, Json)> = Vec::new();
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    let mut zips = Vec::new();
    let mut dirs = Vec::new();
    fn walk(d: &Path, depth: usize, zips: &mut Vec<PathBuf>, dirs: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for p in rd.flatten().map(|e| e.path()) {
            let hidden = p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.'));
            if hidden {
                continue;
            }
            if p.is_dir() {
                dirs.push(p.clone());
                if depth > 0 {
                    walk(&p, depth - 1, zips, dirs);
                }
            } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) {
                zips.push(p);
            }
        }
    }
    for pl in places {
        walk(pl, 1, &mut zips, &mut dirs);
    }
    // The newest zips first, and not every zip a Downloads folder holds.
    zips.sort_by_key(|p| std::cmp::Reverse(modified(p)));
    for z in zips.into_iter().take(200) {
        let Ok(names) = zip_names(&z) else { continue };
        let files: Vec<String> = names.iter().filter(|n| export_file(n)).cloned().collect();
        if !files.is_empty() {
            found.push((modified(&z), json!({"path": z.display().to_string(), "holds": export_kind(&files), "files": files, "saved": modified(&z) * 1000, "megabytes": std::fs::metadata(&z).map_or(0, |m| m.len() / 1_000_000)})));
        }
    }
    for d in dirs {
        let files: Vec<String> = std::fs::read_dir(&d).into_iter().flatten().flatten().map(|e| e.path().to_string_lossy().into_owned()).filter(|n| export_file(n)).collect();
        if !files.is_empty() {
            let short: Vec<String> = files.iter().map(|f| f.rsplit('/').next().unwrap_or(f).to_string()).collect();
            found.push((modified(&d), json!({"path": d.display().to_string(), "holds": export_kind(&files), "files": short, "saved": modified(&d) * 1000})));
        }
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found.into_iter().take(50).map(|f| f.1).collect()
}

/// What an export's files say it is.
fn export_kind(files: &[String]) -> &'static str {
    if files.iter().any(|f| f.contains("Gemini") || f.contains("Bard")) {
        "Gemini (Google Takeout)"
    } else if files.iter().any(|f| f.to_ascii_lowercase().ends_with("memories.json")) {
        "Claude.ai"
    } else {
        "ChatGPT or Claude.ai"
    }
}

fn export_file(name: &str) -> bool {
    let base = name.rsplit('/').next().unwrap_or(name).to_ascii_lowercase();
    let gemini = name.contains("Gemini") || name.contains("Bard");
    base == "conversations.json" || base == "memories.json" || (gemini && (base == "myactivity.json" || base == "myactivity.html"))
}

fn find_files(dir: &Path, depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() && depth > 0 {
            out.extend(find_files(&p, depth - 1));
        } else if p.is_file() && export_file(&p.to_string_lossy()) {
            out.push(p);
        }
    }
    out
}

fn zip_names(path: &Path) -> Result<Vec<String>> {
    let o = Command::new("unzip").arg("-Z1").arg(path).output().map_err(|e| err(format!("reading a zip needs unzip, which did not run ({e}); unzip the export and import its folder instead")))?;
    if !o.status.success() {
        return Err(err(format!("{} is not a zip that unzip reads", path.display())));
    }
    Ok(String::from_utf8_lossy(&o.stdout).lines().map(String::from).collect())
}

/// One file of an export, by its name and contents.
pub fn read_named(name: &str, bytes: &[u8], from: Option<Origin>) -> Result<Vec<Chat>> {
    let text = String::from_utf8_lossy(bytes);
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".html") || lower.ends_with(".htm") {
        return Ok(parse_gemini_html(&text));
    }
    if lower.ends_with(".jsonl") || from == Some(Origin::Other) {
        return parse_jsonl(&text);
    }
    let v: Json = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| {
        // A JSON-lines file under another name.
        if text.lines().filter(|l| !l.trim().is_empty()).all(|l| l.trim_start().starts_with('{')) {
            return err(format!("{name}: {e} (one JSON object per line? import it as a .jsonl file)"));
        }
        err(format!("{name}: {e}"))
    })?;
    if lower.ends_with("memories.json") {
        return Ok(parse_claude_memories(&v));
    }
    match from.or_else(|| detect(&v)) {
        Some(Origin::Chatgpt) => Ok(parse_chatgpt(&v)),
        Some(Origin::Claude) => Ok(parse_claude(&v)),
        Some(Origin::Gemini) => Ok(parse_gemini_json(&v)),
        _ => Err(err(format!("{name}: not a ChatGPT, Claude.ai or Gemini export this chloectx reads (pass --from to say which)"))),
    }
}

/// Which assistant's export a JSON document is.
pub fn detect(v: &Json) -> Option<Origin> {
    let first = v.as_array()?.iter().find(|x| x.is_object())?;
    if first.get("mapping").is_some() {
        Some(Origin::Chatgpt)
    } else if first.get("chat_messages").is_some() {
        Some(Origin::Claude)
    } else if first.get("header").is_some() && first.get("time").is_some() {
        Some(Origin::Gemini)
    } else {
        None
    }
}

fn ts_of(v: &Json) -> i64 {
    match v {
        Json::Number(n) => n.as_f64().map_or(0, |f| f as i64),
        Json::String(s) => crate::transcript::parse_ts(s).unwrap_or(0),
        _ => 0,
    }
}

/// ChatGPT's `conversations.json`: each conversation's messages from its
/// current one back to the root (the branch the person last saw), the
/// person's and the assistant's visible text only.
pub fn parse_chatgpt(v: &Json) -> Vec<Chat> {
    let mut out = Vec::new();
    let mut instructions: Vec<Msg> = Vec::new();
    for c in v.as_array().into_iter().flatten() {
        let Some(mapping) = c.get("mapping").and_then(|m| m.as_object()) else { continue };
        let id = c.get("conversation_id").or_else(|| c.get("id")).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let created = ts_of(c.get("create_time").unwrap_or(&Json::Null));
        // The current branch, from its last message to the root.
        let mut node = c.get("current_node").and_then(|x| x.as_str()).map(String::from);
        if node.is_none() {
            // No current node: the newest leaf.
            node = mapping.iter().filter(|(_, n)| n["children"].as_array().is_none_or(|ch| ch.is_empty())).max_by_key(|(_, n)| ts_of(&n["message"]["create_time"])).map(|(k, _)| k.clone());
        }
        let mut path = Vec::new();
        let mut guard = 0;
        while let Some(k) = node {
            guard += 1;
            if guard > 100_000 {
                break;
            }
            let Some(n) = mapping.get(&k) else { break };
            path.push(n);
            node = n.get("parent").and_then(|p| p.as_str()).map(String::from);
        }
        path.reverse();
        let mut messages = Vec::new();
        for n in path {
            let m = &n["message"];
            if m.is_null() || m["metadata"]["is_visually_hidden_from_conversation"].as_bool() == Some(true) {
                continue;
            }
            let role = match m["author"]["role"].as_str() {
                Some("user") => Role::User,
                Some("assistant") => Role::Assistant,
                _ => continue,
            };
            let content = &m["content"];
            let ct = content["content_type"].as_str().unwrap_or("text");
            if ct == "user_editable_context" {
                for (k, label) in [("user_profile", "About me"), ("user_instructions", "How I want answers")] {
                    if let Some(t) = content[k].as_str().filter(|t| !t.trim().is_empty()) {
                        instructions.push(Msg { role: Role::User, text: format!("My ChatGPT custom instructions ({label}): {}", t.trim()), ts: ts_of(&m["create_time"]) });
                    }
                }
                continue;
            }
            // A message the assistant sends to a tool is no reply.
            if role == Role::Assistant && m["recipient"].as_str().is_some_and(|r| r != "all") {
                continue;
            }
            let text = match ct {
                "text" | "multimodal_text" => content["parts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| match p {
                        Json::String(s) => Some(s.clone()),
                        Json::Object(o) if o.get("content_type").and_then(|c| c.as_str()).is_some_and(|c| c.contains("image")) => Some("[image]".into()),
                        Json::Object(o) => o.get("text").and_then(|t| t.as_str()).map(String::from),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => continue,
            };
            if text.trim().is_empty() || text.trim() == "[image]" {
                continue;
            }
            messages.push(Msg { role, text, ts: ts_of(&m["create_time"]) });
        }
        if messages.is_empty() {
            continue;
        }
        let url = (!id.is_empty()).then(|| format!("https://chatgpt.com/c/{id}"));
        let title = c["title"].as_str().filter(|t| !t.trim().is_empty()).unwrap_or("Untitled chat").to_string();
        out.push(Chat { origin: Origin::Chatgpt, id: if id.is_empty() { format!("{title}\0{created}") } else { id }, title, url, created, messages });
    }
    if !instructions.is_empty() {
        // Each version once, the newest last.
        instructions.sort_by_key(|m| m.ts);
        let mut seen = HashSet::new();
        instructions.retain(|m| seen.insert(m.text.clone()));
        let created = instructions.first().map_or(0, |m| m.ts);
        out.push(Chat { origin: Origin::Chatgpt, id: "chatgpt-custom-instructions".into(), title: "ChatGPT custom instructions".into(), url: None, created, messages: instructions });
    }
    out
}

/// Claude.ai's `conversations.json`.
pub fn parse_claude(v: &Json) -> Vec<Chat> {
    let mut out = Vec::new();
    for c in v.as_array().into_iter().flatten() {
        let Some(msgs) = c.get("chat_messages").and_then(|m| m.as_array()) else { continue };
        let id = c["uuid"].as_str().unwrap_or("").to_string();
        let created = ts_of(&c["created_at"]);
        let mut messages = Vec::new();
        for m in msgs {
            let role = match m["sender"].as_str() {
                Some("human") | Some("user") => Role::User,
                Some("assistant") => Role::Assistant,
                _ => continue,
            };
            let parts: Vec<String> = m["content"].as_array().into_iter().flatten().filter(|p| p["type"] == "text").filter_map(|p| p["text"].as_str().map(String::from)).collect();
            let text = if parts.iter().any(|p| !p.trim().is_empty()) { parts.join("\n") } else { m["text"].as_str().unwrap_or("").to_string() };
            if text.trim().is_empty() {
                continue;
            }
            messages.push(Msg { role, text, ts: ts_of(&m["created_at"]) });
        }
        if messages.is_empty() {
            continue;
        }
        let url = (!id.is_empty()).then(|| format!("https://claude.ai/chat/{id}"));
        let title = c["name"].as_str().filter(|t| !t.trim().is_empty()).unwrap_or("Untitled chat").to_string();
        out.push(Chat { origin: Origin::Claude, id: if id.is_empty() { format!("{title}\0{created}") } else { id }, title, url, created, messages });
    }
    out
}

/// Claude.ai's `memories.json`: what Claude's own memory kept about the
/// person (a text, or a list of them), as a chat of the person's words.
pub fn parse_claude_memories(v: &Json) -> Vec<Chat> {
    let mut texts = Vec::new();
    fn walk(v: &Json, key: &str, out: &mut Vec<String>) {
        match v {
            Json::String(s) if !s.trim().is_empty() && (key.contains("memor") || key.is_empty()) => out.push(s.trim().to_string()),
            Json::Array(a) => a.iter().for_each(|x| walk(x, key, out)),
            Json::Object(o) => o.iter().for_each(|(k, x)| walk(x, if key.contains("memor") { key } else { k }, out)),
            _ => {}
        }
    }
    walk(v, "", &mut texts);
    if texts.is_empty() {
        return Vec::new();
    }
    let messages = texts.into_iter().map(|t| Msg { role: Role::User, text: format!("What Claude's memory kept about me: {t}"), ts: 0 }).collect();
    vec![Chat { origin: Origin::Claude, id: "claude-memories".into(), title: "Claude's memory of me".into(), url: None, created: 0, messages }]
}

/// Prompts this close (seconds) are one Gemini chat: Takeout lists each
/// prompt and reply as its own activity, with no conversation.
const GEMINI_GAP: i64 = 30 * 60;

/// Google Takeout's `MyActivity.json` for Gemini Apps.
pub fn parse_gemini_json(v: &Json) -> Vec<Chat> {
    let mut turns: Vec<(i64, String, String)> = Vec::new();
    for a in v.as_array().into_iter().flatten() {
        let title = a["title"].as_str().unwrap_or("");
        let Some(prompt) = title.strip_prefix("Prompted ").or_else(|| title.strip_prefix("Prompted\u{a0}")) else { continue };
        let reply = a["safeHtmlItem"].as_array().into_iter().flatten().filter_map(|h| h["html"].as_str()).map(html_text).collect::<Vec<_>>().join("\n");
        turns.push((ts_of(&a["time"]), prompt.trim().to_string(), reply));
    }
    gemini_chats(turns)
}

/// Google Takeout's `MyActivity.html` for Gemini Apps: each activity's
/// cell holds "Prompted <prompt>", its date, then the reply.
pub fn parse_gemini_html(html: &str) -> Vec<Chat> {
    let mut turns = Vec::new();
    for cell in html.split("outer-cell").skip(1) {
        let Some(start) = cell.find("content-cell") else { continue };
        let body = &cell[start..];
        let Some(gt) = body.find('>') else { continue };
        let body = &body[gt + 1..];
        let parts: Vec<&str> = body.splitn(3, "<br>").collect();
        if parts.len() < 2 {
            continue;
        }
        let prompt = html_text(parts[0]);
        let Some(prompt) = prompt.strip_prefix("Prompted").map(str::trim) else { continue };
        let ts = parse_takeout_date(&html_text(parts[1])).unwrap_or(0);
        // The reply runs to the cell's end: the next cell of the activity
        // holds the product's name.
        let reply = parts.get(2).map(|r| html_text(r.split("mdl-typography--caption").next().unwrap_or(r))).unwrap_or_default();
        turns.push((ts, prompt.to_string(), reply));
    }
    gemini_chats(turns)
}

fn gemini_chats(mut turns: Vec<(i64, String, String)>) -> Vec<Chat> {
    turns.retain(|t| !t.1.is_empty());
    turns.sort_by_key(|t| t.0);
    let mut out: Vec<Chat> = Vec::new();
    let mut last = i64::MIN;
    for (ts, prompt, reply) in turns {
        if out.is_empty() || ts - last > GEMINI_GAP || ts == 0 && last == 0 && out.last().is_some_and(|c| c.messages.len() >= 40) {
            out.push(Chat { origin: Origin::Gemini, id: format!("gm{:08x}{:04x}", crate::embed::fnv(&format!("{ts}\0{prompt}")) as u32, out.len() as u16), title: clip(&prompt, 80), url: None, created: ts, messages: Vec::new() });
        }
        last = ts;
        let c = out.last_mut().unwrap();
        c.messages.push(Msg { role: Role::User, text: prompt, ts });
        if !reply.trim().is_empty() {
            c.messages.push(Msg { role: Role::Assistant, text: reply, ts });
        }
    }
    out
}

/// Unix seconds of a Takeout date (`Nov 3, 2024, 10:15:32 AM PST`); the
/// zone is not read, so it may be off by hours, never by days.
pub fn parse_takeout_date(s: &str) -> Option<i64> {
    let s = s.replace(['\u{202f}', '\u{a0}'], " ");
    let w: Vec<&str> = s.split(|c: char| c == ',' || c.is_whitespace()).filter(|x| !x.is_empty()).collect();
    if w.len() < 4 {
        return None;
    }
    const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let m = MONTHS.iter().position(|x| w[0].to_ascii_lowercase().starts_with(x))? + 1;
    let d: i64 = w[1].parse().ok()?;
    let y: i64 = w[2].parse().ok()?;
    let hms: Vec<i64> = w[3].split(':').filter_map(|x| x.parse().ok()).collect();
    let mut h = *hms.first()?;
    let pm = w.get(4).is_some_and(|x| x.eq_ignore_ascii_case("pm"));
    if pm && h < 12 {
        h += 12;
    } else if !pm && w.get(4).is_some_and(|x| x.eq_ignore_ascii_case("am")) && h == 12 {
        h = 0;
    }
    crate::transcript::parse_ts(&format!("{y:04}-{m:02}-{d:02}T{h:02}:{:02}:{:02}Z", hms.get(1).unwrap_or(&0), hms.get(2).unwrap_or(&0)))
}

/// The text of a piece of HTML: tags dropped (a block's end is a new
/// line), entities decoded, runs of blank lines folded.
pub fn html_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut chars = html.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' {
            let mut tag = String::new();
            for t in chars.by_ref() {
                if t == '>' {
                    break;
                }
                tag.push(t);
            }
            let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
            if matches!(name.as_str(), "br" | "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "pre" | "ul" | "ol") {
                out.push('\n');
            }
            if name == "li" && !tag.starts_with('/') {
                out.push_str("- ");
            }
        } else if c == '&' {
            let mut ent = String::new();
            while let Some(&n) = chars.peek() {
                if n == ';' || ent.len() > 8 {
                    break;
                }
                ent.push(n);
                chars.next();
            }
            if chars.peek() == Some(&';') {
                chars.next();
                out.push_str(&decode_entity(&ent));
            } else {
                out.push('&');
                out.push_str(&ent);
            }
        } else {
            out.push(c);
        }
    }
    let mut folded = String::new();
    let mut blank = 0;
    for l in out.lines() {
        let l = l.trim_end();
        if l.trim().is_empty() {
            blank += 1;
            if blank == 1 && !folded.is_empty() {
                folded.push('\n');
            }
            continue;
        }
        blank = 0;
        folded.push_str(l.trim_start_matches(' '));
        folded.push('\n');
    }
    folded.trim().to_string()
}

fn decode_entity(e: &str) -> String {
    match e {
        "amp" => "&".into(),
        "lt" => "<".into(),
        "gt" => ">".into(),
        "quot" => "\"".into(),
        "apos" | "#39" => "'".into(),
        "nbsp" => " ".into(),
        _ => {
            let n = if let Some(h) = e.strip_prefix("#x").or_else(|| e.strip_prefix("#X")) {
                u32::from_str_radix(h, 16).ok()
            } else {
                e.strip_prefix('#').and_then(|d| d.parse().ok())
            };
            n.and_then(char::from_u32).map(|c| c.to_string()).unwrap_or_else(|| format!("&{e};"))
        }
    }
}

/// The JSON-lines format: one message per line, grouped by `conversation`.
pub fn parse_jsonl(text: &str) -> Result<Vec<Chat>> {
    let mut by: BTreeMap<(String, String), Chat> = BTreeMap::new();
    let mut order: Vec<(String, String)> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: Json = serde_json::from_str(line).map_err(|e| err(format!("line {}: {e}", i + 1)))?;
        let origin = v["source"].as_str().and_then(Origin::parse).unwrap_or(Origin::Other);
        let conv = v["conversation"].as_str().map(String::from).or_else(|| v["conversation"].as_i64().map(|n| n.to_string())).unwrap_or_else(|| "default".into());
        let role = match v["role"].as_str() {
            Some("user") | Some("human") => Role::User,
            Some("assistant") | Some("model") => Role::Assistant,
            _ => continue,
        };
        let key = (origin.as_str().to_string(), conv.clone());
        let ts = ts_of(&v["ts"]);
        let chat = by.entry(key.clone()).or_insert_with(|| {
            order.push(key.clone());
            Chat { origin, id: conv.clone(), title: v["title"].as_str().unwrap_or(&conv).to_string(), url: v["url"].as_str().map(String::from), created: ts, messages: Vec::new() }
        });
        chat.messages.push(Msg { role, text: v["text"].as_str().unwrap_or("").to_string(), ts });
    }
    Ok(order.into_iter().filter_map(|k| by.remove(&k)).collect())
}

/// What `chloectx import --dry-run` says about what was read.
pub fn summary(chats: &[Chat]) -> Json {
    let mut by: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for c in chats {
        let e = by.entry(c.origin.as_str()).or_default();
        e.0 += 1;
        e.1 += c.messages.len();
    }
    let first = chats.iter().flat_map(|c| c.messages.iter().map(|m| m.ts).chain([c.created])).filter(|t| *t > 0).min();
    let last = chats.iter().flat_map(|c| c.messages.iter().map(|m| m.ts)).max();
    json!({
        "chats": chats.len(),
        "messages": chats.iter().map(|c| c.messages.len()).sum::<usize>(),
        "by_source": by.iter().map(|(k, (c, m))| json!({"source": k, "chats": c, "messages": m})).collect::<Vec<_>>(),
        "first_ts": first, "last_ts": last,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn chatgpt_export() -> Json {
        json!([{
            "title": "Weekly meal plan",
            "create_time": 1709370000.5,
            "conversation_id": "6a1f0e2c-1111-4c3a-9d2e-aaaaaaaaaaaa",
            "current_node": "n4",
            "mapping": {
                "root": {"id": "root", "message": null, "parent": null, "children": ["n0"]},
                "n0": {"id": "n0", "parent": "root", "children": ["n1"], "message": {"author": {"role": "system"}, "content": {"content_type": "text", "parts": [""]}, "metadata": {"is_visually_hidden_from_conversation": true}}},
                "n1": {"id": "n1", "parent": "n0", "children": ["n2", "n2b"], "message": {"author": {"role": "user"}, "create_time": 1709370010.0, "content": {"content_type": "text", "parts": ["I'm vegetarian and I hate mushrooms. Plan my dinners for the week."]}, "metadata": {}}},
                "n2b": {"id": "n2b", "parent": "n1", "children": [], "message": {"author": {"role": "assistant"}, "create_time": 1709370015.0, "content": {"content_type": "text", "parts": ["An abandoned branch."]}, "recipient": "all", "metadata": {}}},
                "n2": {"id": "n2", "parent": "n1", "children": ["n3"], "message": {"author": {"role": "assistant"}, "create_time": 1709370020.0, "content": {"content_type": "code", "text": "search('recipes')"}, "recipient": "browser", "metadata": {}}},
                "n3": {"id": "n3", "parent": "n2", "children": ["n4"], "message": {"author": {"role": "tool"}, "content": {"content_type": "text", "parts": ["tool output"]}, "metadata": {}}},
                "n4": {"id": "n4", "parent": "n3", "children": [], "message": {"author": {"role": "assistant"}, "create_time": 1709370030.0, "content": {"content_type": "text", "parts": ["Monday: lentil curry. Tuesday: halloumi tacos (no mushrooms)."]}, "recipient": "all", "metadata": {}}}
            }
        }, {
            "title": "Custom", "create_time": 1700000000.0, "id": "c2", "current_node": "m1",
            "mapping": {"m1": {"id": "m1", "parent": null, "children": [], "message": {"author": {"role": "user"}, "create_time": 1700000000.0,
                "content": {"content_type": "user_editable_context", "user_profile": "I live in Lisbon and work as a data engineer.", "user_instructions": "Be brief."}, "metadata": {}}}}
        }])
    }

    #[test]
    fn chatgpt_walks_the_current_branch() {
        let chats = parse_chatgpt(&chatgpt_export());
        assert_eq!(detect(&chatgpt_export()), Some(Origin::Chatgpt));
        assert_eq!(chats.len(), 2, "{chats:?}");
        let c = &chats[0];
        assert_eq!(c.title, "Weekly meal plan");
        assert_eq!(c.url.as_deref(), Some("https://chatgpt.com/c/6a1f0e2c-1111-4c3a-9d2e-aaaaaaaaaaaa"));
        let texts: Vec<&str> = c.messages.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, ["I'm vegetarian and I hate mushrooms. Plan my dinners for the week.", "Monday: lentil curry. Tuesday: halloumi tacos (no mushrooms)."]);
        assert_eq!(c.messages[0].ts, 1709370010);
        // The custom instructions are a chat of their own, the person's words.
        let ci = &chats[1];
        assert_eq!(ci.title, "ChatGPT custom instructions");
        assert!(ci.messages.iter().all(|m| m.role == Role::User));
        assert!(ci.messages[0].text.contains("Lisbon"), "{ci:?}");
    }

    #[test]
    fn claude_and_its_memories() {
        let v = json!([{"uuid": "0c9b8e5a-2222-4f0e-8a77-bbbbbbbbbbbb", "name": "Trip to Kyoto", "created_at": "2025-04-01T10:00:00.000000Z",
            "chat_messages": [
                {"uuid": "m1", "sender": "human", "text": "", "content": [{"type": "text", "text": "I want a quiet ryokan, I don't drink alcohol."}], "created_at": "2025-04-01T10:00:05Z"},
                {"uuid": "m2", "sender": "assistant", "text": "Here are three ryokans...", "content": [{"type": "tool_use", "name": "web_search"}], "created_at": "2025-04-01T10:00:09Z"}
            ]}]);
        assert_eq!(detect(&v), Some(Origin::Claude));
        let chats = parse_claude(&v);
        assert_eq!(chats[0].messages.len(), 2);
        assert_eq!(chats[0].messages[0].text, "I want a quiet ryokan, I don't drink alcohol.");
        assert_eq!(chats[0].messages[1].text, "Here are three ryokans...");
        assert_eq!(chats[0].url.as_deref(), Some("https://claude.ai/chat/0c9b8e5a-2222-4f0e-8a77-bbbbbbbbbbbb"));
        let mem = parse_claude_memories(&json!({"conversations_memory": "Works on a search database. Prefers short answers.", "project_memories": {"p1": "Uses Rust."}}));
        assert_eq!(mem[0].messages.len(), 2, "{mem:?}");
    }

    #[test]
    fn gemini_from_takeout_json_and_html() {
        let v = json!([
            {"header": "Gemini Apps", "title": "Prompted what should I pack for Iceland in March", "time": "2025-02-10T08:00:00.000Z", "products": ["Gemini Apps"], "safeHtmlItem": [{"html": "<p>Layers &amp; a <b>waterproof</b> shell.</p><ul><li>Wool</li></ul>"}]},
            {"header": "Gemini Apps", "title": "Prompted and for hiking boots?", "time": "2025-02-10T08:05:00.000Z", "products": ["Gemini Apps"]},
            {"header": "Gemini Apps", "title": "Used an Assistant feature", "time": "2025-02-10T08:06:00.000Z"},
            {"header": "Gemini Apps", "title": "Prompted a new topic much later", "time": "2025-02-12T08:00:00.000Z"}
        ]);
        assert_eq!(detect(&v), Some(Origin::Gemini));
        let chats = parse_gemini_json(&v);
        assert_eq!(chats.len(), 2, "two chats by the gap: {chats:?}");
        assert_eq!(chats[0].messages.len(), 3);
        assert_eq!(chats[0].messages[1].text, "Layers & a waterproof shell.\n\n- Wool");
        let html = r#"<html><body><div class="outer-cell mdl-cell"><div class="mdl-grid"><div class="header-cell"><p>Gemini Apps<br></p></div><div class="content-cell mdl-cell mdl-cell--6-col mdl-typography--body-1">Prompted&nbsp;best vegetarian ramen in Tokyo<br>Mar 3, 2025, 7:15:32 PM PST<br><p>Try <i>Ts&#x27;ta</i> in Ueno.</p></div><div class="content-cell mdl-typography--caption"><b>Products:</b>Gemini Apps</div></div></div></body></html>"#;
        let chats = parse_gemini_html(html);
        assert_eq!(chats.len(), 1, "{chats:?}");
        assert_eq!(chats[0].messages[0].text, "best vegetarian ramen in Tokyo");
        assert_eq!(chats[0].messages[1].text, "Try Ts'ta in Ueno.");
        assert_eq!(chats[0].messages[0].ts, crate::transcript::parse_ts("2025-03-03T19:15:32Z").unwrap());
    }

    #[test]
    fn jsonl_groups_by_conversation() {
        let t = "{\"source\":\"chatgpt\",\"conversation\":\"a\",\"title\":\"A\",\"role\":\"user\",\"text\":\"hi\",\"ts\":10}\n{\"source\":\"chatgpt\",\"conversation\":\"b\",\"role\":\"user\",\"text\":\"yo\"}\n{\"source\":\"chatgpt\",\"conversation\":\"a\",\"role\":\"assistant\",\"text\":\"hello\"}\n";
        let chats = parse_jsonl(t).unwrap();
        assert_eq!(chats.len(), 2);
        assert_eq!(chats[0].messages.len(), 2);
        assert_eq!(chats[0].title, "A");
    }

    #[test]
    fn each_message_is_kept_once() {
        let chats = parse_chatgpt(&chatgpt_export());
        let c = &chats[0];
        let (b, h) = batches(c, &HashSet::new());
        assert_eq!(h.len(), 2);
        assert!(b[0].drafts.iter().any(|d| d.kind == Kind::Prompt && d.ctx.starts_with("From: ChatGPT \"Weekly meal plan\" <https://chatgpt.com/c/")), "{:?}", b[0].drafts);
        assert!(b[1].drafts.iter().any(|d| d.kind == Kind::Answer && d.ctx.contains("Turn 1 request: I'm vegetarian")), "{:?}", b[1].drafts);
        let seen: HashSet<String> = h.into_iter().collect();
        let (b2, h2) = batches(c, &seen);
        assert!(b2.is_empty() && h2.is_empty());
        // A longer copy of the chat: only the new reply.
        let mut longer = c.clone();
        longer.messages.push(Msg { role: Role::User, text: "Swap Tuesday for pasta.".into(), ts: 1709370100 });
        let (b3, h3) = batches(&longer, &seen);
        assert_eq!((b3.len(), h3.len(), b3[0].turn), (1, 1, 2));
        let ev = chat_event(&longer, &h3, 3);
        assert_eq!(seen_in(&ev.ctx).collect::<Vec<_>>(), h3.iter().map(String::as_str).collect::<Vec<_>>());
    }

    #[test]
    fn takeout_dates_and_html() {
        assert_eq!(parse_takeout_date("Nov 3, 2024, 12:05:00 AM PST"), crate::transcript::parse_ts("2024-11-03T00:05:00Z"));
        assert_eq!(parse_takeout_date("Nov\u{a0}3, 2024, 12:05:00\u{202f}PM PST"), crate::transcript::parse_ts("2024-11-03T12:05:00Z"));
        assert_eq!(html_text("a&lt;b &amp;&amp; c&#33;<br>next"), "a<b && c!\nnext");
    }
}
