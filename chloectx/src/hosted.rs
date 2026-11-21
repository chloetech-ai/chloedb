//! The hosted beta: many people's personal memories in one `chloectx app`
//! (`--hosted-url`), each an account with a space of the cluster of its
//! own (`me_<handle>`).
//!
//! A person signs up with an invite and a passcode of their choosing, and
//! gets a page (`/account`) and a connector address
//! (`<public>/u/<handle>/mcp`) to give Claude.ai or ChatGPT. Everything at
//! `/u/<handle>/` is [`remote::Server`] for that account, as `chloectx me
//! serve` serves one person (OAuth sign-in with the account's passcode,
//! `/capture` for the browser extension), so the tools and the sign-in are
//! the same code; the API proxy and shares are not served here. The page
//! uploads an export in parts and imports it, shows what the memory holds,
//! the card (confirm or drop a line), the assistants signed in and the
//! access log, makes a capture key, downloads everything as JSON lines and
//! deletes the account with its space.
//!
//! What it keeps on its volume (`--hosted-state DIR`, mode 0600 files):
//! `accounts.json` (each account's passcode as PBKDF2, its capture key as
//! SHA-256, the invite it used), and per account `oauth.json` (clients and
//! token hashes) and `access.log`. The memories are in the cluster.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write;
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

use crate::app::App;
use crate::remote::{self, Backend, OnStore, Server};
use crate::share::{random_b64, sha256_hex, write_private};
use crate::ui::{respond, Request};
use crate::{err, now, Result};

/// The largest request the app reads with `--hosted-url`: an upload's part.
pub const MAX_BODY: usize = 4 << 20;
/// An upload's part, as the page sends it (under [`MAX_BODY`]).
const PART: usize = 3 << 20;
/// The largest export an account may upload at once.
const MAX_UPLOAD: u64 = 2 << 30;
/// Imports at once, over every account (each embeds and writes to the cluster).
const IMPORTS: usize = 2;
/// How long a sign-in to the page lasts.
const SESSION_MS: i64 = 7 * 24 * 3600 * 1000;
/// Wrong passcodes for one account in [`LOCK_MS`] before sign-in stops.
const MAX_FAILS: usize = 5;
const LOCK_MS: i64 = 15 * 60 * 1000;
const COOKIE: &str = "ctx_session";

/// `--hosted-url` and what goes with it.
#[derive(Clone, Debug)]
pub struct Options {
    /// The https:// address people reach it at.
    pub public_url: String,
    /// Where the accounts, their sign-ins and access logs are kept.
    pub state: PathBuf,
    /// One invite a line, each used once; none: anyone may sign up.
    pub invites: Option<PathBuf>,
    /// The most accounts it takes.
    pub max_accounts: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Account {
    created: i64,
    passcode_salt: String,
    passcode_hash: String,
    #[serde(default)]
    capture_key_hash: String,
    #[serde(default)]
    invite: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Accounts {
    #[serde(default)]
    accounts: BTreeMap<String, Account>,
    #[serde(default)]
    used_invites: BTreeSet<String>,
}

struct Upload {
    path: PathBuf,
    bytes: u64,
}

pub struct Hosted {
    opts: Options,
    accounts: Mutex<Accounts>,
    servers: Mutex<HashMap<String, Arc<Server>>>,
    /// By the session token's SHA-256: the account and when it ends.
    sessions: Mutex<HashMap<String, (String, i64)>>,
    fails: Mutex<HashMap<String, Vec<i64>>>,
    uploads: Mutex<HashMap<String, Upload>>,
    /// Each account's last import: its state and counts.
    imports: Mutex<HashMap<String, Json>>,
    importing: AtomicUsize,
}

/// A response: status, content type, extra headers, body.
type Out = (u16, &'static str, Vec<(&'static str, String)>, Vec<u8>);

fn json_out(code: u16, v: &Json) -> Out {
    (code, "application/json", Vec::new(), v.to_string().into_bytes())
}

fn fail(code: u16, why: impl Into<String>) -> Out {
    json_out(code, &json!({"error": why.into()}))
}

/// The space an account's memory is in.
pub fn space_of(handle: &str) -> String {
    format!("me_{handle}")
}

/// A handle: 3 to 24 lowercase letters and digits, starting with a letter.
pub fn valid_handle(h: &str) -> bool {
    let b = h.as_bytes();
    (3..=24).contains(&b.len()) && b[0].is_ascii_lowercase() && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// A file name an upload may have: letters, digits, `.`, `-` and `_`,
/// ending in .zip, .json, .html or .jsonl.
fn upload_name(n: &str) -> Option<String> {
    let base = n.rsplit(['/', '\\']).next().unwrap_or("").trim();
    let clean: String = base.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')).take(100).collect();
    let lower = clean.to_ascii_lowercase();
    let ok = [".zip", ".json", ".html", ".jsonl"].iter().any(|e| lower.ends_with(e)) && !clean.starts_with('.');
    ok.then_some(clean)
}

fn cookie<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.header("cookie")?.split(';').map(str::trim).find_map(|c| c.strip_prefix(name)?.strip_prefix('='))
}

impl Hosted {
    pub fn new(opts: Options) -> Result<Hosted> {
        if !remote::public_url_ok(&opts.public_url) {
            return Err(err(format!("--hosted-url is the https:// address people reach this app at, not {}", opts.public_url)));
        }
        crate::paths::ensure_private_dir(&opts.state)?;
        let accounts = std::fs::read_to_string(opts.state.join("accounts.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Ok(Hosted {
            opts,
            accounts: Mutex::new(accounts),
            servers: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            fails: Mutex::new(HashMap::new()),
            uploads: Mutex::new(HashMap::new()),
            imports: Mutex::new(HashMap::new()),
            importing: AtomicUsize::new(0),
        })
    }

    fn base(&self) -> &str {
        self.opts.public_url.trim_end_matches('/')
    }

    /// `scheme://host[:port]` of the public address, which a browser's
    /// requests name as their Origin.
    fn origin(&self) -> String {
        let b = self.base();
        let rest = b.split_once("://").map_or(b, |x| x.1);
        let host = rest.split('/').next().unwrap_or("");
        format!("{}://{host}", b.split_once("://").map_or("https", |x| x.0))
    }

    fn account_dir(&self, h: &str) -> PathBuf {
        self.opts.state.join("accounts").join(h)
    }

    fn save_accounts(&self, a: &Accounts) -> Result<()> {
        write_private(&self.opts.state.join("accounts.json"), &serde_json::to_string_pretty(a)?)
    }

    /// Whether `path` is one this serves, before the app's own routes.
    pub fn handles(path: &str) -> bool {
        matches!(path, "/" | "/signup" | "/signin" | "/signout" | "/account" | "/account.js")
            || path.starts_with("/account/")
            || path.starts_with("/u/")
            || ["/.well-known/oauth-protected-resource/u/", "/.well-known/oauth-authorization-server/u/", "/.well-known/openid-configuration/u/"].iter().any(|p| path.starts_with(p))
    }

    /// The account's server, made once and again after its secrets change.
    fn server(&self, app: &App, h: &str) -> Result<Arc<Server>> {
        if let Some(s) = self.servers.lock().unwrap_or_else(|e| e.into_inner()).get(h) {
            return Ok(s.clone());
        }
        let acct = self.accounts.lock().unwrap_or_else(|e| e.into_inner()).accounts.get(h).cloned().ok_or_else(|| err(format!("no account {h}")))?;
        let dir = self.account_dir(h);
        crate::paths::ensure_private_dir(&dir)?;
        let cfg = remote::Config {
            public_url: Some(format!("{}/u/{h}", self.base())),
            secrets: remote::Secrets { passcode_salt: acct.passcode_salt, passcode_hash: acct.passcode_hash, capture_key_hash: acct.capture_key_hash },
            state_path: Some(dir.join("oauth.json")),
            log_path: Some(dir.join("access.log")),
            account_page: Some(format!("{}/account", self.base())),
            ..Default::default()
        };
        let s = Server::new(cfg, Arc::new(OnStore(app.store(&space_of(h))?)));
        Ok(self.servers.lock().unwrap_or_else(|e| e.into_inner()).entry(h.to_string()).or_insert(s).clone())
    }

    fn backend(&self, app: &App, h: &str) -> Result<OnStore> {
        Ok(OnStore(app.store(&space_of(h))?))
    }

    /// Answers `req` on `stream`.
    pub(crate) fn handle(self: &Arc<Self>, app: &App, mut stream: TcpStream, mut req: Request) {
        // An account's own routes: its server, with the path below it.
        if let Some((h, rest)) = self.account_route(&req.path) {
            let known = self.accounts.lock().unwrap_or_else(|e| e.into_inner()).accounts.contains_key(&h);
            if !known || rest.starts_with("/p/") || rest.starts_with("/s/") {
                let (c, t, hd, b) = fail(404, "not found");
                return respond(&mut stream, &remote::status_text(c), t, &hd, &b);
            }
            return match self.server(app, &h) {
                Ok(s) => {
                    req.path = rest;
                    s.handle(stream, req)
                }
                Err(e) => {
                    let (c, t, hd, b) = fail(500, e.to_string());
                    respond(&mut stream, &remote::status_text(c), t, &hd, &b)
                }
            };
        }
        let peer = stream.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default();
        let (code, ctype, mut headers, body) = self.route(app, &req, &peer);
        headers.push(("Referrer-Policy", "no-referrer".into()));
        if ctype.starts_with("text/html") {
            headers.push(("Content-Security-Policy", "default-src 'none'; script-src 'self'; style-src 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; form-action 'self'; frame-ancestors 'none'; base-uri 'none'".into()));
        }
        respond(&mut stream, &remote::status_text(code), ctype, &headers, &body);
    }

    /// `/u/<handle>/<rest>`, and the well-known addresses with the account
    /// after them (RFC 8414 and RFC 9728 put the issuer's path there).
    fn account_route(&self, path: &str) -> Option<(String, String)> {
        for wk in ["/.well-known/oauth-protected-resource", "/.well-known/oauth-authorization-server", "/.well-known/openid-configuration"] {
            if let Some(r) = path.strip_prefix(wk).and_then(|r| r.strip_prefix("/u/")) {
                let (h, tail) = r.split_once('/').unwrap_or((r, ""));
                let tail = if tail.is_empty() { String::new() } else { format!("/{tail}") };
                return Some((h.to_string(), format!("{wk}{tail}")));
            }
        }
        let r = path.strip_prefix("/u/")?;
        let (h, tail) = r.split_once('/').unwrap_or((r, ""));
        Some((h.to_string(), format!("/{tail}")))
    }

    fn route(self: &Arc<Self>, app: &App, req: &Request, peer: &str) -> Out {
        let m = req.method.as_str();
        // Every change comes from this site's own page.
        if m == "POST" && req.header("origin") != Some(self.origin().as_str()) {
            return fail(403, "requests that change an account come from its page");
        }
        let who = self.session(req);
        match (m, req.path.as_str()) {
            ("GET", "/") => match who {
                Some(_) => (303, "text/plain", vec![("Location", "/account".into())], Vec::new()),
                None => (200, "text/html; charset=utf-8", Vec::new(), self.landing().into_bytes()),
            },
            ("GET", "/account.js") => (200, "text/javascript; charset=utf-8", Vec::new(), ACCOUNT_JS.as_bytes().to_vec()),
            ("POST", "/signup") => self.signup(app, req),
            ("POST", "/signin") => self.signin(req, peer),
            ("POST", "/signout") => {
                if let Some(t) = cookie(req, COOKIE) {
                    self.sessions.lock().unwrap_or_else(|e| e.into_inner()).remove(&sha256_hex(t));
                }
                (200, "application/json", vec![("Set-Cookie", self.cookie_value("", 0))], b"{}".to_vec())
            }
            ("GET", "/account") => match who {
                Some(h) => (200, "text/html; charset=utf-8", Vec::new(), account_page(&h).into_bytes()),
                None => (303, "text/plain", vec![("Location", "/".into())], Vec::new()),
            },
            (_, p) if p.starts_with("/account/") => {
                let Some(h) = who else { return fail(401, "sign in first") };
                self.account_api(app, &h, req)
            }
            _ => fail(404, "not found"),
        }
    }

    fn session(&self, req: &Request) -> Option<String> {
        let t = cookie(req, COOKIE)?;
        let mut s = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        let key = sha256_hex(t);
        match s.get(&key) {
            Some((h, until)) if *until > now() => Some(h.clone()),
            Some(_) => {
                s.remove(&key);
                None
            }
            None => None,
        }
    }

    fn cookie_value(&self, token: &str, max_age: i64) -> String {
        let secure = if self.base().starts_with("https://") { "; Secure" } else { "" };
        format!("{COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}")
    }

    fn start_session(&self, h: &str) -> (&'static str, String) {
        let token = random_b64(32);
        let mut s = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        let t = now();
        s.retain(|_, (_, until)| *until > t);
        s.insert(sha256_hex(&token), (h.to_string(), t + SESSION_MS));
        ("Set-Cookie", self.cookie_value(&token, SESSION_MS / 1000))
    }

    fn signup(&self, app: &App, req: &Request) -> Out {
        let b: Json = serde_json::from_slice(&req.body).unwrap_or(Json::Null);
        let s = |k: &str| b.get(k).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let (h, pass, invite) = (s("handle").to_ascii_lowercase(), b.get("passcode").and_then(|v| v.as_str()).unwrap_or("").to_string(), s("invite"));
        if !valid_handle(&h) {
            return fail(400, "A name is 3 to 24 lowercase letters and digits, starting with a letter.");
        }
        if pass.chars().count() < 10 {
            return fail(400, "A passcode is at least 10 characters.");
        }
        let mut a = self.accounts.lock().unwrap_or_else(|e| e.into_inner());
        if a.accounts.contains_key(&h) {
            return fail(409, "That name is taken.");
        }
        if a.accounts.len() >= self.opts.max_accounts {
            return fail(503, "The beta is full for now.");
        }
        if let Some(file) = &self.opts.invites {
            let known = std::fs::read_to_string(file).unwrap_or_default().lines().map(str::trim).any(|l| !l.is_empty() && !l.starts_with('#') && l == invite);
            if !known || a.used_invites.contains(&invite) {
                return fail(403, "That invite is not valid, or it was used already.");
            }
        }
        if let Err(e) = app.create_space(&space_of(&h), "Personal memory", &format!("{h}'s chats with assistants and persona card (hosted)"), "notes") {
            return fail(500, e.to_string());
        }
        let salt = random_b64(16);
        let acct = Account { created: now(), passcode_hash: remote::hash_passcode(&pass, &salt), passcode_salt: salt, capture_key_hash: String::new(), invite: invite.clone() };
        a.accounts.insert(h.clone(), acct);
        if self.opts.invites.is_some() {
            a.used_invites.insert(invite);
        }
        if let Err(e) = self.save_accounts(&a) {
            return fail(500, e.to_string());
        }
        drop(a);
        let c = self.start_session(&h);
        (200, "application/json", vec![c], json!({"account": h}).to_string().into_bytes())
    }

    fn signin(&self, req: &Request, _peer: &str) -> Out {
        let b: Json = serde_json::from_slice(&req.body).unwrap_or(Json::Null);
        let h = b.get("handle").and_then(|v| v.as_str()).unwrap_or("").trim().to_ascii_lowercase();
        let pass = b.get("passcode").and_then(|v| v.as_str()).unwrap_or("");
        let t = now();
        {
            let mut f = self.fails.lock().unwrap_or_else(|e| e.into_inner());
            let list = f.entry(h.clone()).or_default();
            list.retain(|x| t - x < LOCK_MS);
            if list.len() >= MAX_FAILS {
                return fail(429, "Too many wrong passcodes: try again in 15 minutes.");
            }
        }
        let acct = self.accounts.lock().unwrap_or_else(|e| e.into_inner()).accounts.get(&h).cloned();
        let ok = acct.as_ref().is_some_and(|a| remote::hash_passcode(pass, &a.passcode_salt) == a.passcode_hash);
        if !ok {
            self.fails.lock().unwrap_or_else(|e| e.into_inner()).entry(h).or_default().push(t);
            return fail(401, "That name and passcode do not match.");
        }
        let c = self.start_session(&h);
        (200, "application/json", vec![c], json!({"account": h}).to_string().into_bytes())
    }

    fn account_api(self: &Arc<Self>, app: &App, h: &str, req: &Request) -> Out {
        let b: Json = serde_json::from_slice(&req.body).unwrap_or(Json::Null);
        let res: Result<Out> = (|| match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/account/state") => Ok(json_out(200, &self.state(app, h)?)),
            ("POST", "/account/upload") => Ok(self.upload(app, h, req)),
            ("POST", "/account/card") => {
                let id = b["id"].as_str().unwrap_or("");
                let what = match b["action"].as_str().unwrap_or("") {
                    "confirm" => "card_confirm",
                    "drop" => "card_drop",
                    _ => return Ok(fail(400, "action is confirm or drop")),
                };
                let r = self.backend(app, h)?.me(json!({"what": what, "id": id}))?;
                Ok(json_out(200, &r))
            }
            ("POST", "/account/capture-key") => {
                let key = format!("ctxc_{}", random_b64(24));
                {
                    let mut a = self.accounts.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(acct) = a.accounts.get_mut(h) {
                        acct.capture_key_hash = sha256_hex(&key);
                    }
                    self.save_accounts(&a)?;
                }
                // Its server is made again with the new key; sign-ins are on disk.
                self.servers.lock().unwrap_or_else(|e| e.into_inner()).remove(h);
                Ok(json_out(200, &json!({"capture_key": key, "address": format!("{}/u/{h}", self.base())})))
            }
            ("POST", "/account/signout-assistants") => {
                let n = self.server(app, h)?.sign_out_all();
                Ok(json_out(200, &json!({"signed_out": n})))
            }
            ("GET", "/account/export") => {
                let store = app.store(&space_of(h))?;
                let mut out = Vec::new();
                for (r, _) in store.export(&Default::default(), 50_000_000)? {
                    serde_json::to_writer(&mut out, &r)?;
                    out.push(b'\n');
                }
                Ok((200, "application/x-ndjson", vec![("Content-Disposition", format!("attachment; filename=\"chloectx-{h}.jsonl\""))], out))
            }
            ("POST", "/account/delete") => {
                let pass = b["passcode"].as_str().unwrap_or("");
                let acct = self.accounts.lock().unwrap_or_else(|e| e.into_inner()).accounts.get(h).cloned();
                if !acct.is_some_and(|a| remote::hash_passcode(pass, &a.passcode_salt) == a.passcode_hash) {
                    return Ok(fail(401, "That passcode is not this account's."));
                }
                if self.imports.lock().unwrap_or_else(|e| e.into_inner()).get(h).is_some_and(|j| j["state"] == "running") {
                    return Ok(fail(409, "An import is running: delete the account when it ends."));
                }
                self.delete(app, h)?;
                Ok((200, "application/json", vec![("Set-Cookie", self.cookie_value("", 0))], json!({"deleted": h}).to_string().into_bytes()))
            }
            _ => Ok(fail(404, "not found")),
        })();
        res.unwrap_or_else(|e| fail(500, e.to_string()))
    }

    /// Deletes an account: its space (every record), its sign-ins, its log
    /// and its sessions. Its invite stays used.
    fn delete(&self, app: &App, h: &str) -> Result<()> {
        if let Err(e) = app.delete_space(&space_of(h)) {
            if e.0 != 404 {
                return Err(err(e.1));
            }
        }
        self.servers.lock().unwrap_or_else(|e| e.into_inner()).remove(h);
        self.sessions.lock().unwrap_or_else(|e| e.into_inner()).retain(|_, (x, _)| x != h);
        self.uploads.lock().unwrap_or_else(|e| e.into_inner()).remove(h);
        self.imports.lock().unwrap_or_else(|e| e.into_inner()).remove(h);
        let _ = std::fs::remove_dir_all(self.account_dir(h));
        let mut a = self.accounts.lock().unwrap_or_else(|e| e.into_inner());
        a.accounts.remove(h);
        self.save_accounts(&a)
    }

    fn state(&self, app: &App, h: &str) -> Result<Json> {
        let be = self.backend(app, h)?;
        let status = be.me(json!({"what": "status"}))?;
        let card = be.me(json!({"what": "card"}))?;
        let server = self.server(app, h)?;
        let log: Vec<Json> = std::fs::read_to_string(self.account_dir(h).join("access.log"))
            .unwrap_or_default()
            .lines()
            .rev()
            .take(30)
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        let acct = self.accounts.lock().unwrap_or_else(|e| e.into_inner()).accounts.get(h).cloned().unwrap_or_default();
        Ok(json!({
            "account": h,
            "connector": format!("{}/u/{h}/mcp", self.base()),
            "address": format!("{}/u/{h}", self.base()),
            "status": {"by_source": status["by_source"], "card_lines": status["card_lines"], "card_confirmed": status["card_confirmed"]},
            "card": card["lines"],
            "assistants": server.clients(),
            "log": log,
            "capture_key": !acct.capture_key_hash.is_empty(),
            "import": self.imports.lock().unwrap_or_else(|e| e.into_inner()).get(h).cloned().unwrap_or(Json::Null),
            "part": PART,
        }))
    }

    /// A part of an export (`?name=&offset=`), and with `final=1` its last
    /// one, after which it is imported in the background.
    fn upload(self: &Arc<Self>, app: &App, h: &str, req: &Request) -> Out {
        let Some(name) = upload_name(req.param("name").unwrap_or("")) else {
            return fail(400, "Upload the export as it came: a .zip (or its conversations.json, memories.json or MyActivity file).");
        };
        let offset: u64 = req.param("offset").and_then(|v| v.parse().ok()).unwrap_or(0);
        if self.imports.lock().unwrap_or_else(|e| e.into_inner()).get(h).is_some_and(|j| j["state"] == "running") {
            return fail(409, "An import is running already.");
        }
        let dir = self.account_dir(h).join("upload");
        let mut ups = self.uploads.lock().unwrap_or_else(|e| e.into_inner());
        if offset == 0 {
            let _ = std::fs::remove_dir_all(&dir);
            if let Err(e) = crate::paths::ensure_private_dir(&dir) {
                return fail(500, e.to_string());
            }
            ups.insert(h.to_string(), Upload { path: dir.join(&name), bytes: 0 });
        }
        let Some(up) = ups.get_mut(h) else { return fail(409, "Start the upload again from its first part.") };
        if up.bytes != offset || up.path.file_name().and_then(|n| n.to_str()) != Some(name.as_str()) {
            return fail(409, "Start the upload again from its first part.");
        }
        if up.bytes + req.body.len() as u64 > MAX_UPLOAD {
            return fail(413, "That file is larger than 2 GB.");
        }
        use std::os::unix::fs::OpenOptionsExt;
        let wrote = std::fs::OpenOptions::new().create(true).append(true).mode(0o600).open(&up.path).and_then(|mut f| f.write_all(&req.body));
        if let Err(e) = wrote {
            return fail(500, e.to_string());
        }
        up.bytes += req.body.len() as u64;
        let received = up.bytes;
        if req.param("final") != Some("1") {
            return json_out(200, &json!({"received": received}));
        }
        let path = up.path.clone();
        ups.remove(h);
        drop(ups);
        if self.importing.fetch_add(1, Ordering::SeqCst) >= IMPORTS {
            self.importing.fetch_sub(1, Ordering::SeqCst);
            let _ = std::fs::remove_dir_all(&dir);
            return fail(503, "Other imports are running: try again in a few minutes.");
        }
        let store = match app.store(&space_of(h)) {
            Ok(s) => s,
            Err(e) => {
                self.importing.fetch_sub(1, Ordering::SeqCst);
                return fail(500, e.to_string());
            }
        };
        self.imports.lock().unwrap_or_else(|e| e.into_inner()).insert(h.to_string(), json!({"state": "running", "file": name, "bytes": received, "chats": 0, "done": 0, "stored": 0, "known": 0, "started": now()}));
        let me = self.clone();
        let h = h.to_string();
        std::thread::spawn(move || {
            let set = |v: Json| {
                me.imports.lock().unwrap_or_else(|e| e.into_inner()).insert(h.clone(), v);
            };
            let mut j = json!({"state": "running", "file": name, "bytes": received, "chats": 0, "done": 0, "stored": 0, "known": 0, "started": now()});
            let res: Result<()> = (|| {
                let chats = crate::import::read(&path, None)?;
                j["chats"] = json!(chats.len());
                j["summary"] = crate::import::summary(&chats)["by_source"].clone();
                set(j.clone());
                for (i, c) in chats.into_iter().enumerate() {
                    let r = crate::me::op(&store, &json!({"what": "import", "chat": c}))?;
                    j["stored"] = json!(j["stored"].as_u64().unwrap_or(0) + r["messages"].as_u64().unwrap_or(0));
                    j["known"] = json!(j["known"].as_u64().unwrap_or(0) + r["known"].as_u64().unwrap_or(0));
                    j["done"] = json!(i + 1);
                    if i % 20 == 0 {
                        set(j.clone());
                    }
                }
                Ok(())
            })();
            match res {
                Ok(()) => j["state"] = json!("done"),
                Err(e) => {
                    j["state"] = json!("failed");
                    j["error"] = json!(e.to_string());
                }
            }
            j["ended"] = json!(now());
            set(j);
            let _ = std::fs::remove_dir_all(path.parent().unwrap_or(&path));
            me.importing.fetch_sub(1, Ordering::SeqCst);
        });
        json_out(200, &json!({"received": received, "importing": true}))
    }

    fn landing(&self) -> String {
        let invite = if self.opts.invites.is_some() { "<label>Invite<input name=\"invite\" required autocomplete=\"off\"></label>" } else { "" };
        page(
            "Your memory, for every assistant",
            &format!(
                "<p>Bring your chats with ChatGPT, Claude and Gemini into one memory of your own, with a card of what they say about you. Claude and ChatGPT read it through a connector, and you see every time they do.</p>\
<section class=\"card\"><h2>Sign in</h2><form id=\"signin\"><label>Name<input name=\"handle\" required autocomplete=\"username\" autocapitalize=\"none\"></label><label>Passcode<input name=\"passcode\" type=\"password\" required autocomplete=\"current-password\"></label><button>Sign in</button><p class=\"msg\"></p></form></section>\
<section class=\"card\"><h2>New here? Join the beta</h2><form id=\"signup\">{invite}<label>Pick a name<input name=\"handle\" required pattern=\"[a-z][a-z0-9]{{2,23}}\" autocomplete=\"username\" autocapitalize=\"none\"><small>3 to 24 lowercase letters and digits. It is part of your connector's address.</small></label><label>Pick a passcode<input name=\"passcode\" type=\"password\" minlength=\"10\" required autocomplete=\"new-password\"><small>At least 10 characters. Claude and ChatGPT ask for it when you connect them.</small></label><button>Create my memory</button><p class=\"msg\"></p></form></section>\
<p class=\"small\">This is a beta. Your chats are kept on ChloeCtx's servers, where the people who run it can reach them. An assistant reads them only after you sign it in with your passcode, and every read is logged on your page. You can download everything or delete it all at any time.</p>"
            ),
        )
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{t}</title><style>{CSS}</style></head><body><main><h1>{t}</h1>{body}</main><script src=\"/account.js\"></script></body></html>",
        t = html_escape(title)
    )
}

fn account_page(h: &str) -> String {
    page(
        "Your memory",
        &format!(
            "<p class=\"small\">Signed in as <b>{h}</b>. <a href=\"#\" id=\"signout\">Sign out</a></p>\
<section class=\"card\" id=\"connect\"><h2>1. Connect Claude</h2><p>In Claude (claude.ai or the desktop app): Settings, Connectors, <b>Add custom connector</b>. Name it ChloeCtx, paste this address, then press Connect and sign in with your passcode.</p><div class=\"copy\"><code id=\"connector\"></code><button class=\"ghost\" data-copy=\"connector\">Copy</button></div><p class=\"small\">ChatGPT works too: Settings, Apps &amp; Connectors, turn on developer mode, then Create, with the same address and OAuth.</p></section>\
<section class=\"card\" id=\"import\"><h2>2. Bring your chats</h2><p>Ask each service for an export: ChatGPT (Settings, Data controls, Export data), Claude (Settings, Privacy, Export data) or Gemini (Google Takeout, My Activity, Gemini Apps). Each emails you a link to a zip. Download it and pick it here, as it came.</p><input type=\"file\" id=\"file\" accept=\".zip,.json,.html,.jsonl\"><progress id=\"progress\" max=\"1\" value=\"0\" hidden></progress><p class=\"msg\" id=\"import-msg\"></p><div id=\"sources\"></div></section>\
<section class=\"card\" id=\"card\"><h2>3. Your persona card</h2><p>Ask Claude: <i>\"Draft my persona card from my memory.\"</i> It reads what you wrote and suggests short lines about you, each linked to the chats it came from. Keep the ones that are right.</p><div id=\"lines\"></div></section>\
<section class=\"card\"><h2>Assistants signed in</h2><div id=\"assistants\"></div><button class=\"ghost\" id=\"signout-assistants\">Sign them all out</button><h3>What they read</h3><div id=\"log\"></div></section>\
<section class=\"card\"><h2>Keep it current</h2><p>The ChloeCtx capture extension for Chrome records your new chats on chatgpt.com, claude.ai and gemini.google.com as you have them, on the sites you turn on. Give it this address and a capture key:</p><div class=\"copy\"><code id=\"address\"></code><button class=\"ghost\" data-copy=\"address\">Copy</button></div><button class=\"ghost\" id=\"capture-key\">Make a capture key</button><p class=\"msg\" id=\"key-msg\"></p></section>\
<section class=\"card\"><h2>Your data</h2><p><a id=\"export\" href=\"/account/export\">Download everything</a> (JSON lines: every message and card line).</p><details><summary>Delete my account</summary><p>This deletes every chat and card line kept here and signs every assistant out. It cannot be undone.</p><label>Passcode<input type=\"password\" id=\"delete-pass\" autocomplete=\"current-password\"></label><button class=\"danger\" id=\"delete\">Delete everything</button><p class=\"msg\" id=\"delete-msg\"></p></details></section>",
            h = html_escape(h)
        ),
    )
}

const CSS: &str = ":root{color-scheme:light dark;--bg:#f7f7f5;--fg:#1d1d1b;--mute:#6b6b66;--card:#fff;--line:#e3e2dc;--acc:#2f5bd3;--bad:#b3261e;--ok:#1e7a46}\
@media (prefers-color-scheme:dark){:root{--bg:#151514;--fg:#ecebe6;--mute:#a19f97;--card:#1f1f1d;--line:#34332f;--acc:#8aa8ff;--bad:#f28b82;--ok:#6fcf97}}\
body{margin:0;background:var(--bg);color:var(--fg);font:16px/1.5 system-ui,-apple-system,sans-serif}main{max-width:640px;margin:0 auto;padding:32px 16px 64px}\
h1{font-size:24px;margin:0 0 8px}h2{font-size:18px;margin:0 0 8px}h3{font-size:15px;margin:16px 0 4px}a{color:var(--acc)}\
.card{background:var(--card);border:1px solid var(--line);border-radius:12px;padding:20px;margin:16px 0}\
label{display:block;margin:12px 0;font-weight:600}label small{display:block;font-weight:400;color:var(--mute)}\
input:not([type=file]){display:block;width:100%;box-sizing:border-box;font:inherit;padding:9px 11px;margin-top:4px;border:1px solid var(--line);border-radius:8px;background:var(--bg);color:var(--fg)}\
button{font:inherit;padding:8px 16px;border-radius:8px;border:1px solid var(--acc);background:var(--acc);color:#fff;cursor:pointer;margin-top:8px}\
button.ghost{background:transparent;color:var(--fg);border-color:var(--line)}button.danger{background:var(--bad);border-color:var(--bad)}button:disabled{opacity:.5}\
.copy{display:flex;gap:8px;align-items:center;flex-wrap:wrap}.copy code{flex:1;min-width:0;padding:8px;border:1px solid var(--line);border-radius:8px;background:var(--bg)}\
code{font:13px ui-monospace,monospace;overflow-wrap:anywhere}.small,.msg{color:var(--mute);font-size:14px}.msg.bad{color:var(--bad)}.msg.ok{color:var(--ok)}\
progress{width:100%;margin-top:8px}.line{border-top:1px solid var(--line);padding:8px 0;display:flex;gap:8px;align-items:flex-start;flex-wrap:wrap}\
.line .t{flex:1;min-width:200px}.line .s{color:var(--mute);font-size:13px;display:block}.line button{margin:0;padding:4px 10px;font-size:14px}.tag{font-size:12px;color:var(--mute);border:1px solid var(--line);border-radius:6px;padding:0 6px}\
table{width:100%;border-collapse:collapse;font-size:14px}td{border-top:1px solid var(--line);padding:4px 6px 4px 0;vertical-align:top}";

const ACCOUNT_JS: &str = include_str!("hosted.js");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_and_names() {
        assert!(valid_handle("ana42") && !valid_handle("4ana") && !valid_handle("an") && !valid_handle("Ana") && !valid_handle("a_na"));
        assert_eq!(upload_name("C:\\Users\\x\\chatgpt export (1).zip").as_deref(), Some("chatgptexport1.zip"));
        assert_eq!(upload_name("../../conversations.json").as_deref(), Some("conversations.json"));
        assert!(upload_name("evil.sh").is_none() && upload_name(".zip").is_none());
        assert!(Hosted::handles("/u/ana/mcp") && Hosted::handles("/.well-known/oauth-authorization-server/u/ana") && Hosted::handles("/account/state"));
        assert!(!Hosted::handles("/v1/spaces") && !Hosted::handles("/mcp"));
    }

    #[test]
    fn account_routes() {
        let dir = std::env::temp_dir().join(format!("hosted-test-{}", std::process::id()));
        let h = Hosted::new(Options { public_url: "https://me.example.com".into(), state: dir.clone(), invites: None, max_accounts: 10 }).unwrap();
        assert_eq!(h.origin(), "https://me.example.com");
        assert_eq!(h.account_route("/u/ana/mcp"), Some(("ana".into(), "/mcp".into())));
        assert_eq!(h.account_route("/u/ana"), Some(("ana".into(), "/".into())));
        assert_eq!(h.account_route("/.well-known/oauth-authorization-server/u/ana"), Some(("ana".into(), "/.well-known/oauth-authorization-server".into())));
        assert_eq!(h.account_route("/.well-known/oauth-protected-resource/u/ana/mcp"), Some(("ana".into(), "/.well-known/oauth-protected-resource/mcp".into())));
        assert_eq!(h.account_route("/account"), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
