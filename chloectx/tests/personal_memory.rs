//! The personal memory end to end, through the `chloectx` binary: an export
//! imported once, the persona card, `chloectx me serve` signed into as
//! ChatGPT would (dynamic registration, the passcode on the consent page,
//! PKCE, refresh), its MCP tools, the browser extension's capture, the API
//! proxy in front of a stand-in for OpenAI's API, and a share that opens its
//! slice only, until it is revoked. The hash embedder, so no model files.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};

const BIN: &str = env!("CARGO_BIN_EXE_chloectx");

fn cmd(home: &Path, args: &[&str]) -> String {
    let o = Command::new(BIN).args(args).env("CHLOECTX_HOME", home).env("CHLOECTX_EMBEDDER", "hash:64").output().unwrap();
    assert!(o.status.success(), "chloectx {args:?}: {}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

struct Stop<'a>(Child, &'a Path);
impl Drop for Stop<'_> {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = Command::new(BIN).args(["stop", "--all"]).env("CHLOECTX_HOME", self.1).output();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// One HTTP request: (status, headers lowercased, body).
fn http(method: &str, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>) -> (u16, Vec<(String, String)>, Vec<u8>) {
    let rest = url.strip_prefix("http://").unwrap();
    let (host, path) = rest.split_once('/').map(|(h, p)| (h, format!("/{p}"))).unwrap_or((rest, "/".into()));
    let mut s = std::net::TcpStream::connect(host).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    let body = body.unwrap_or(b"");
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
    for (k, v) in headers {
        head += &format!("{k}: {v}\r\n");
    }
    head += "\r\n";
    s.write_all(head.as_bytes()).unwrap();
    s.write_all(body).unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.lines();
    let code: u16 = lines.next().unwrap().split_whitespace().nth(1).unwrap().parse().unwrap();
    let hs = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    (code, hs, raw[split + 4..].to_vec())
}

fn header<'a>(h: &'a [(String, String)], k: &str) -> &'a str {
    h.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str()).unwrap_or("")
}

fn post_json(url: &str, auth: Option<&str>, body: &Json) -> (u16, Json) {
    let a = auth.map(|t| format!("Bearer {t}"));
    let mut h = vec![("content-type", "application/json")];
    if let Some(a) = &a {
        h.push(("authorization", a.as_str()));
    }
    let (c, _, b) = http("POST", url, &h, Some(body.to_string().as_bytes()));
    (c, serde_json::from_slice(&b).unwrap_or(Json::Null))
}

fn post_form(url: &str, form: &[(&str, &str)]) -> (u16, Vec<(String, String)>, Vec<u8>) {
    let enc = |s: &str| s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect::<String>();
    let body: String = form.iter().map(|(k, v)| format!("{}={}", enc(k), enc(v))).collect::<Vec<_>>().join("&");
    http("POST", url, &[("content-type", "application/x-www-form-urlencoded")], Some(body.as_bytes()))
}

fn tool(url: &str, auth: Option<&str>, name: &str, args: Json) -> (String, bool) {
    let (c, r) = post_json(url, auth, &json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": {"name": name, "arguments": args}}));
    assert_eq!(c, 200, "{r}");
    (r["result"]["content"][0]["text"].as_str().unwrap_or("").to_string(), r["result"]["isError"] == true)
}

fn query_param(url: &str, k: &str) -> String {
    url.split(['?', '&']).find_map(|p| p.strip_prefix(&format!("{k}="))).unwrap_or("").to_string()
}

/// A stand-in for OpenAI's API: answers every request with one chat completion.
fn fake_openai() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut len = 0usize;
            let mut auth = String::new();
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                let lower = line.to_ascii_lowercase();
                if let Some(v) = lower.strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
                if lower.starts_with("authorization:") {
                    auth = line[14..].trim().to_string();
                }
            }
            let mut body = vec![0; len];
            let _ = r.read_exact(&mut body);
            let out = if auth == "Bearer sk-test" { json!({"choices": [{"message": {"role": "assistant", "content": "Try roasted chickpeas."}}]}) } else { json!({"error": "bad key"}) };
            let out = out.to_string();
            let mut s = s;
            let _ = write!(s, "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{out}", if auth == "Bearer sk-test" { "200 OK" } else { "401 Unauthorized" }, out.len());
        }
    });
    url
}

#[test]
fn personal_memory_end_to_end() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    let export = tempfile::tempdir().unwrap();
    let conv = |title: &str, id: &str, user: &str, reply: &str| {
        json!({"title": title, "create_time": 1709370000.0, "conversation_id": id, "current_node": "b", "mapping": {
            "a": {"id": "a", "parent": null, "children": ["b"], "message": {"author": {"role": "user"}, "create_time": 1709370010.0, "content": {"content_type": "text", "parts": [user]}, "metadata": {}}},
            "b": {"id": "b", "parent": "a", "children": [], "message": {"author": {"role": "assistant"}, "create_time": 1709370020.0, "content": {"content_type": "text", "parts": [reply]}, "recipient": "all", "metadata": {}}}}})
    };
    let chats = json!([
        conv("Weekly meal plan", "6a1f0e2c-1111-4c3a-9d2e-aaaaaaaaaaaa", "I'm vegetarian and I hate mushrooms. Plan my dinners.", "Monday: lentil curry."),
        conv("Kyoto trip", "7b2f0e2c-2222-4c3a-9d2e-bbbbbbbbbbbb", "I'm planning a trip to Kyoto; I prefer trains over flights.", "Take the Shinkansen."),
    ]);
    std::fs::write(export.path().join("conversations.json"), chats.to_string()).unwrap();
    let ex = export.path().to_str().unwrap();

    // Imported once: a second import adds nothing.
    let out = cmd(h, &["import", ex, "--json"]);
    let r: Json = serde_json::from_str(&out).unwrap();
    assert_eq!(r["messages"], 4, "{out}");
    let r: Json = serde_json::from_str(&cmd(h, &["import", ex, "--json"])).unwrap();
    assert_eq!((r["messages"].as_u64(), r["known"].as_u64()), (Some(0), Some(4)), "{r}");
    assert!(cmd(h, &["--me", "search", "Kyoto trains"]).contains("Shinkansen"));

    // The card: lines of the person's own.
    assert!(cmd(h, &["me", "card", "add", "Health & diet", "Vegetarian; avoids mushrooms."]).contains("Added"));
    cmd(h, &["me", "card", "add", "Travel", "Prefers trains over flights."]);
    let card: Json = serde_json::from_str(&cmd(h, &["me", "card", "--json"])).unwrap();
    assert_eq!(card["lines"].as_array().unwrap().len(), 2, "{card}");

    // The server.
    let upstream = fake_openai();
    let port = free_port();
    let mut child = Command::new(BIN)
        .args(["me", "serve", "--port", &port.to_string(), "--openai-base", &upstream])
        .env("CHLOECTX_HOME", h)
        .env("CHLOECTX_EMBEDDER", "hash:64")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let _stop = Stop(child, h);
    let (mut passcode, mut capture) = (String::new(), String::new());
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(60) {
        let Some(Ok(l)) = lines.next() else { break };
        if let Some(p) = l.strip_prefix("Passcode (to sign an assistant in): ") {
            passcode = p.trim().into();
        }
        if let Some(k) = l.strip_prefix("Capture key (for the browser extension and the API proxy): ") {
            capture = k.trim().into();
        }
        if l.starts_with("API proxy") {
            break;
        }
    }
    assert!(!passcode.is_empty() && capture.starts_with("ctxc_"), "the passcode and capture key are printed once");
    std::thread::spawn(move || for _ in lines.by_ref() {});
    let base = format!("http://127.0.0.1:{port}");
    let mcp = format!("{base}/mcp");

    // Signing in as ChatGPT does.
    let (c, hs, _) = http("POST", &mcp, &[("content-type", "application/json")], Some(b"{}"));
    assert_eq!(c, 401);
    assert!(header(&hs, "www-authenticate").contains(&format!("resource_metadata=\"{base}/.well-known/oauth-protected-resource/mcp\"")), "{hs:?}");
    let (_, _, b) = http("GET", &format!("{base}/.well-known/oauth-authorization-server"), &[], None);
    let meta: Json = serde_json::from_slice(&b).unwrap();
    assert_eq!(meta["registration_endpoint"], format!("{base}/register"));
    let redirect = "https://chatgpt.com/connector_platform_oauth_redirect";
    let (c, reg) = post_json(&format!("{base}/register"), None, &json!({"client_name": "ChatGPT", "redirect_uris": [redirect]}));
    assert_eq!(c, 201, "{reg}");
    let client = reg["client_id"].as_str().unwrap().to_string();
    let verifier = "a-verifier-of-forty-three-characters-or-more-1234";
    let challenge = {
        use sha2::Digest;
        chloectx::share::b64url(&sha2::Sha256::digest(verifier.as_bytes()))
    };
    let auth_form = |pass: &str, write: bool| {
        let mut f = vec![("response_type", "code"), ("client_id", client.as_str()), ("redirect_uri", redirect), ("code_challenge", challenge.as_str()), ("code_challenge_method", "S256"), ("state", "s1"), ("passcode", pass), ("decision", "allow")];
        if write {
            f.push(("write", "1"));
        }
        post_form(&format!("{base}/authorize"), &f)
    };
    let (c, _, page) = http("GET", &format!("{base}/authorize?response_type=code&client_id={client}&redirect_uri={}&code_challenge={challenge}&code_challenge_method=S256&state=s1", redirect.replace(':', "%3A").replace('/', "%2F")), &[], None);
    assert_eq!(c, 200);
    assert!(String::from_utf8_lossy(&page).contains("ChatGPT"));
    assert_eq!(auth_form("wrong-wrong-wrong", true).0, 403, "a wrong passcode is refused");
    let (c, hs, _) = auth_form(&passcode, true);
    assert_eq!(c, 302);
    let loc = header(&hs, "location").to_string();
    assert!(loc.starts_with(redirect) && loc.contains("state=s1"), "{loc}");
    let code = query_param(&loc, "code");
    let token = |code: &str, verifier: &str| post_form(&format!("{base}/token"), &[("grant_type", "authorization_code"), ("code", code), ("redirect_uri", redirect), ("client_id", &client), ("code_verifier", verifier)]);
    let (c, _, b) = token(&code, verifier);
    assert_eq!(c, 200, "{}", String::from_utf8_lossy(&b));
    let t: Json = serde_json::from_slice(&b).unwrap();
    let access = t["access_token"].as_str().unwrap().to_string();
    assert_eq!(token(&code, verifier).0, 400, "a code is used once");

    // The tools.
    let (c, init) = post_json(&mcp, Some(&access), &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}));
    assert_eq!(c, 200);
    assert!(init["result"]["instructions"].as_str().unwrap().contains("persona"));
    let (_, list) = post_json(&mcp, Some(&access), &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    let names: Vec<&str> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["persona", "search", "fetch", "ask", "remember"]);
    let (card, _) = tool(&mcp, Some(&access), "persona", json!({}));
    assert!(card.contains("Vegetarian; avoids mushrooms.") && card.contains("## Travel"), "{card}");
    let (found, _) = tool(&mcp, Some(&access), "search", json!({"query": "Kyoto Shinkansen"}));
    let found: Json = serde_json::from_str(&found).unwrap();
    let hit = found["results"].as_array().unwrap().iter().find(|r| r["url"] == "https://chatgpt.com/c/7b2f0e2c-2222-4c3a-9d2e-bbbbbbbbbbbb").expect("the chat's address").clone();
    assert!(hit["title"].as_str().unwrap().starts_with("ChatGPT: Kyoto trip"), "{hit}");
    let (full, _) = tool(&mcp, Some(&access), "fetch", json!({"id": hit["id"]}));
    assert!(full.contains("Me: I'm planning a trip to Kyoto") && full.contains("ChatGPT: Take the Shinkansen."), "{full}");
    let (said, err) = tool(&mcp, Some(&access), "remember", json!({"text": "Allergic to peanuts.", "kind": "fact", "section": "Health & diet"}));
    assert!(!err && said.contains("suggested"), "{said}");
    let card: Json = serde_json::from_str(&cmd(h, &["me", "card", "--json"])).unwrap();
    let peanut = card["lines"].as_array().unwrap().iter().find(|l| l["text"] == "Allergic to peanuts.").unwrap().clone();
    assert_eq!((peanut["confirmed"].as_bool(), peanut["by"].as_str()), (Some(false), Some("agent:ChatGPT")), "a connected assistant's line is a suggestion");
    let log = std::fs::read_to_string(h.join("me/access.log")).unwrap();
    assert!(log.lines().count() >= 4 && log.contains("\"client\":\"ChatGPT\"") && !log.contains("Allergic"), "the log says who asked what, not what was written: {log}");

    // A refresh token works once.
    let refresh = t["refresh_token"].as_str().unwrap();
    let (c, _, _) = post_form(&format!("{base}/token"), &[("grant_type", "refresh_token"), ("refresh_token", refresh)]);
    assert_eq!(c, 200);
    assert_eq!(post_form(&format!("{base}/token"), &[("grant_type", "refresh_token"), ("refresh_token", refresh)]).0, 400);

    // Read-only: no remember.
    let (_, hs, _) = auth_form(&passcode, false);
    let (_, _, b) = token(&query_param(header(&hs, "location"), "code"), verifier);
    let ro: Json = serde_json::from_slice(&b).unwrap();
    let ro = ro["access_token"].as_str().unwrap();
    let (_, list) = post_json(&mcp, Some(ro), &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    assert!(!list.to_string().contains("\"remember\""), "{list}");

    // The extension's capture: each message once.
    let cap = format!("{base}/capture");
    let gift = |n: usize| {
        let all = [json!({"role": "user", "text": "My sister turns 30 and loves pottery."}), json!({"role": "assistant", "text": "A wheel-throwing class."}), json!({"role": "user", "text": "Budget is 80 euros."})];
        json!({"source": "claude", "conversation": "9d9d9d9d-0000", "title": "Gift ideas", "url": "https://claude.ai/chat/9d9d9d9d-0000", "messages": all[..n].to_vec()})
    };
    assert_eq!(post_json(&cap, Some("ctxc_wrong"), &gift(2)).0, 401);
    assert_eq!(post_json(&cap, Some(&capture), &gift(2)).1["stored"], 2);
    assert_eq!(post_json(&cap, Some(&capture), &gift(3)).1["stored"], 1);

    // The proxy: the answer passes through, the exchange is kept.
    let (c, r) = {
        let (c, _, b) = http("POST", &format!("{base}/p/{capture}/openai/v1/chat/completions"), &[("content-type", "application/json"), ("authorization", "Bearer sk-test")], Some(json!({"model": "m", "messages": [{"role": "user", "content": "I'm allergic to peanuts. Snack ideas?"}]}).to_string().as_bytes()));
        (c, serde_json::from_slice::<Json>(&b).unwrap())
    };
    assert_eq!((c, r["choices"][0]["message"]["content"].as_str()), (200, Some("Try roasted chickpeas.")), "{r}");
    assert_eq!(http("POST", &format!("{base}/p/ctxc_nope/openai/v1/chat/completions"), &[], Some(b"{}")).0, 401);
    let t0 = Instant::now();
    while !cmd(h, &["--me", "search", "roasted chickpeas"]).contains("Try roasted chickpeas.") {
        assert!(t0.elapsed() < Duration::from_secs(20), "the proxied exchange was not recorded");
        std::thread::sleep(Duration::from_millis(200));
    }

    // A share: diet only, signed, logged, revoked.
    let out = cmd(h, &["me", "share", "create", "diet", "--sections", "Health & diet", "--expires", "1d", "--public-url", &base]);
    let key = out.split("/s/").nth(1).and_then(|r| r.split("/mcp").next()).unwrap().to_string();
    let share = format!("{base}/s/{key}/mcp");
    let (_, list) = post_json(&share, None, &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 4, "read-only: {list}");
    let (card, _) = tool(&share, None, "persona", json!({}));
    assert!(card.contains("Vegetarian") && card.contains("Allergic to peanuts") && !card.contains("trains"), "{card}");
    let (found, _) = tool(&share, None, "search", json!({"query": "Kyoto trains Shinkansen"}));
    assert!(!found.contains("Kyoto") && !found.contains("trains"), "a share holds its slice only: {found}");
    let shares: Json = serde_json::from_str(&cmd(h, &["me", "share", "list", "--json"])).unwrap();
    let id = shares[0]["id"].as_str().unwrap().to_string();
    assert!(shares[0]["signer"].as_str().is_some_and(|s| s.len() == 16), "{shares}");
    let log: Json = serde_json::from_str(&cmd(h, &["me", "share", "log", &id, "--json"])).unwrap();
    assert_eq!(log.as_array().unwrap().len(), 2, "{log}");
    assert_eq!(post_json(&format!("{base}/s/ctxs_{id}_wrong/mcp"), None, &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).0, 404);
    cmd(h, &["me", "share", "revoke", &id]);
    assert_eq!(post_json(&share, None, &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).0, 403);

    // And what the space holds.
    let me: Json = serde_json::from_str(&cmd(h, &["me", "--json"])).unwrap();
    let by: std::collections::HashMap<String, u64> = me["by_source"].as_array().unwrap().iter().map(|s| (s["source"].as_str().unwrap().to_string(), s["messages"].as_u64().unwrap())).collect();
    assert_eq!((by.get("chatgpt"), by.get("claude"), by.get("openai")), (Some(&4), Some(&3), Some(&2)), "{me}");
}
