//! An email as a record: its headers, its text, and the maintenance
//! notice it may carry.
//!
//! A mail server or a mail filter hands the message over as it is (RFC
//! 5322, MIME parts and all): Postfix's `pipe` or a `.forward` to `curl
//! --data-binary @-`, fetchmail's `--mda`, a provider's inbound route that
//! posts the raw message. [`parse`] reads its headers (folded lines,
//! RFC 2047 words in UTF-8 or Latin-1), walks its parts (base64 and
//! quoted-printable), and keeps the first `text/plain` part as the body
//! (else the first `text/html` one, its tags taken out).
//!
//! Carriers that follow the maintenance notification BCOP (the MAINTNOTE
//! iCalendar properties, used by many transit and wave providers) attach a
//! `text/calendar` part; its `X-MAINTNOTE-*` properties give the
//! provider, the maintenance's id, the circuits (`X-MAINTNOTE-OBJECT-ID`),
//! the window (`DTSTART`, `DTEND`), the impact and the status, which the
//! record carries as `provider`, `maint_id`, `circuits`, `start`, `end`,
//! `impact` and `status`. A notice without one keeps its text for a parser
//! that reads it (the network operations app reads the body).

use base64::Engine;
use serde_json::{json, Map, Value as Json};

/// One MIME part: its headers (lower-case names) and its decoded body.
struct Part {
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Part {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    fn content_type(&self) -> (String, Vec<(String, String)>) {
        let raw = self.header("content-type").unwrap_or("text/plain");
        let mut it = raw.split(';');
        let ty = it.next().unwrap_or("text/plain").trim().to_ascii_lowercase();
        let params = it
            .filter_map(|p| {
                let (k, v) = p.split_once('=')?;
                Some((k.trim().to_ascii_lowercase(), v.trim().trim_matches('"').to_string()))
            })
            .collect();
        (ty, params)
    }
}

/// Headers and body of `raw`, split at the first empty line.
fn split(raw: &[u8]) -> (Vec<(String, String)>, &[u8]) {
    let (head, body) = match find(raw, b"\r\n\r\n") {
        Some(i) => (&raw[..i], &raw[i + 4..]),
        None => match find(raw, b"\n\n") {
            Some(i) => (&raw[..i], &raw[i + 2..]),
            None => (raw, &raw[raw.len()..]),
        },
    };
    let head = String::from_utf8_lossy(head);
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in head.lines() {
        if line.starts_with([' ', '\t']) {
            if let Some((_, v)) = headers.last_mut() {
                v.push(' ');
                v.push_str(line.trim());
            }
        } else if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    (headers, body)
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

/// The body of a part decoded by its `Content-Transfer-Encoding`.
fn decode(encoding: Option<&str>, body: &[u8]) -> Vec<u8> {
    match encoding.map(|e| e.trim().to_ascii_lowercase()).as_deref() {
        Some("base64") => {
            let clean: Vec<u8> = body.iter().copied().filter(|c| !c.is_ascii_whitespace()).collect();
            base64::engine::general_purpose::STANDARD.decode(&clean).or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(&clean)).unwrap_or_else(|_| body.to_vec())
        }
        Some("quoted-printable") => quoted_printable(body),
        _ => body.to_vec(),
    }
}

fn quoted_printable(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'=' {
            if b.get(i + 1) == Some(&b'\r') && b.get(i + 2) == Some(&b'\n') {
                i += 3;
                continue;
            }
            if b.get(i + 1) == Some(&b'\n') {
                i += 2;
                continue;
            }
            if let (Some(h), Some(l)) = (b.get(i + 1).and_then(|c| (*c as char).to_digit(16)), b.get(i + 2).and_then(|c| (*c as char).to_digit(16))) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// Text of `bytes` in `charset` (UTF-8, else Latin-1 for the ISO-8859 and
/// Windows ones; UTF-8 with replacements for anything else).
fn charset_text(bytes: &[u8], charset: Option<&str>) -> String {
    match charset.map(|c| c.to_ascii_lowercase()) {
        Some(c) if c.starts_with("iso-8859") || c.starts_with("windows-125") || c == "latin1" || c == "us-ascii" => match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            Err(_) => bytes.iter().map(|&b| b as char).collect(),
        },
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// A header's value with its RFC 2047 encoded words (`=?UTF-8?B?...?=`)
/// decoded.
fn words(v: &str) -> String {
    let mut out = String::new();
    let mut rest = v;
    let mut last_was_word = false;
    while let Some(start) = rest.find("=?") {
        let before = &rest[..start];
        if !(last_was_word && before.trim().is_empty()) {
            out.push_str(before);
        }
        let after = &rest[start + 2..];
        let parts: Vec<&str> = after.splitn(3, '?').collect();
        if parts.len() < 3 {
            out.push_str(&rest[start..]);
            return out;
        }
        let Some(end) = parts[2].find("?=") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let (charset, enc, text) = (parts[0], parts[1], &parts[2][..end]);
        let bytes = match enc.to_ascii_uppercase().as_str() {
            "B" => base64::engine::general_purpose::STANDARD.decode(text).unwrap_or_default(),
            _ => quoted_printable(text.replace('_', " ").as_bytes()),
        };
        out.push_str(&charset_text(&bytes, Some(charset)));
        last_was_word = true;
        let consumed = start + 2 + parts[0].len() + 1 + parts[1].len() + 1 + end + 2;
        rest = &rest[consumed..];
    }
    out.push_str(rest);
    out
}

/// Every leaf part of a message, depth first.
fn leaves(headers: Vec<(String, String)>, body: &[u8], depth: usize, out: &mut Vec<Part>) {
    let part = Part { headers, body: Vec::new() };
    let (ty, params) = part.content_type();
    if ty.starts_with("multipart/") && depth < 8 {
        if let Some(b) = params.iter().find(|(k, _)| k == "boundary").map(|(_, v)| v.clone()) {
            let delim = format!("--{b}");
            let text = body;
            let mut pieces: Vec<&[u8]> = Vec::new();
            let mut at = 0;
            let mut starts = Vec::new();
            while let Some(i) = find(&text[at..], delim.as_bytes()) {
                starts.push(at + i);
                at += i + delim.len();
            }
            for w in starts.windows(2) {
                let s = w[0] + delim.len();
                let s = s + text[s..].iter().take_while(|c| **c == b'\r' || **c == b'\n').count();
                pieces.push(&text[s..w[1]]);
            }
            for p in pieces {
                let (h, b) = split(p);
                leaves(h, b, depth + 1, out);
            }
            return;
        }
    }
    let body = decode(part.header("content-transfer-encoding"), body);
    out.push(Part { headers: part.headers, body });
}

/// The text of an HTML part, its tags and scripts out.
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    let b = html.as_bytes();
    while i < b.len() {
        if !in_tag && (lower[i..].starts_with("<script") || lower[i..].starts_with("<style")) {
            let close = if lower[i..].starts_with("<script") { "</script>" } else { "</style>" };
            i = lower[i..].find(close).map_or(b.len(), |j| i + j + close.len());
            continue;
        }
        match b[i] {
            b'<' => {
                in_tag = true;
                if lower[i..].starts_with("<br") || lower[i..].starts_with("<p") || lower[i..].starts_with("</p") || lower[i..].starts_with("<tr") || lower[i..].starts_with("<div") {
                    out.push('\n');
                }
            }
            b'>' if in_tag => in_tag = false,
            _ if !in_tag => {
                let ch = html[i..].chars().next().unwrap_or(' ');
                out.push(ch);
                i += ch.len_utf8();
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out.replace("&nbsp;", " ").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'")
}

/// The properties of an iCalendar text's first VEVENT, unfolded, by
/// upper-case name (parameters dropped; a property seen again adds to a
/// list).
fn ical_event(text: &str) -> Map<String, Json> {
    let mut lines: Vec<String> = Vec::new();
    for l in text.lines() {
        if l.starts_with([' ', '\t']) {
            if let Some(last) = lines.last_mut() {
                last.push_str(&l[1..]);
            }
        } else {
            lines.push(l.trim_end().to_string());
        }
    }
    let mut out = Map::new();
    let mut inside = false;
    for l in lines {
        if l.eq_ignore_ascii_case("BEGIN:VEVENT") {
            inside = true;
            continue;
        }
        if l.eq_ignore_ascii_case("END:VEVENT") {
            break;
        }
        if !inside {
            continue;
        }
        let Some((k, v)) = l.split_once(':') else { continue };
        let name = k.split(';').next().unwrap_or(k).to_ascii_uppercase();
        let v = v.replace("\\,", ",").replace("\\n", "\n").replace("\\;", ";");
        match out.get_mut(&name) {
            Some(Json::Array(a)) => a.push(json!(v)),
            Some(old) => {
                let first = old.take();
                *old = json!([first, v]);
            }
            None => {
                out.insert(name, json!(v));
            }
        }
    }
    out
}

/// The record of raw message `raw` ([`self`]), received at `now_us`.
pub fn parse(raw: &[u8], now_us: i64) -> Json {
    let (headers, body) = split(raw);
    let get = |n: &str| headers.iter().find(|(k, _)| k == n).map(|(_, v)| words(v));
    let mut parts = Vec::new();
    leaves(headers.clone(), body, 0, &mut parts);
    let mut text: Option<String> = None;
    let mut html: Option<String> = None;
    let mut calendar: Option<String> = None;
    for p in &parts {
        let (ty, params) = p.content_type();
        let charset = params.iter().find(|(k, _)| k == "charset").map(|(_, v)| v.as_str());
        let attachment = p.header("content-disposition").is_some_and(|d| d.to_ascii_lowercase().starts_with("attachment"));
        match ty.as_str() {
            "text/plain" if text.is_none() && !attachment => text = Some(charset_text(&p.body, charset)),
            "text/html" if html.is_none() && !attachment => html = Some(charset_text(&p.body, charset)),
            "text/calendar" | "application/ics" if calendar.is_none() => calendar = Some(charset_text(&p.body, charset)),
            _ => {}
        }
    }
    let body_text = text.or_else(|| html.as_deref().map(strip_html)).unwrap_or_default();
    let from = get("from").unwrap_or_default();
    let sender = address(&from);
    let message_id = get("message-id").map(|m| m.trim().trim_matches(|c| c == '<' || c == '>').to_string());
    let mut rec = json!({
        "message_id": message_id,
        "from": from,
        "sender": sender,
        "sender_domain": sender.rsplit_once('@').map(|(_, d)| d.to_ascii_lowercase()),
        "to": get("to"),
        "subject": get("subject").unwrap_or_default(),
        "date": get("date").and_then(|d| super::map::rfc2822(&d)).map(chloedb::timestamp::format),
        "received": chloedb::timestamp::format(now_us),
        "body": body_text,
    });
    if let Some(cal) = &calendar {
        let ev = ical_event(cal);
        let one = |k: &str| ev.get(k).map(|v| match v {
            Json::Array(a) => a.first().cloned().unwrap_or(Json::Null),
            v => v.clone(),
        });
        let t = |k: &str| one(k).and_then(|v| super::map::time(&v)).map(chloedb::timestamp::format);
        let objects: Vec<Json> = match ev.get("X-MAINTNOTE-OBJECT-ID") {
            Some(Json::Array(a)) => a.clone(),
            Some(v) => vec![v.clone()],
            None => Vec::new(),
        };
        rec["provider"] = one("X-MAINTNOTE-PROVIDER").unwrap_or(Json::Null);
        rec["account"] = one("X-MAINTNOTE-ACCOUNT").unwrap_or(Json::Null);
        rec["maint_id"] = one("X-MAINTNOTE-MAINTENANCE-ID").or_else(|| one("UID")).unwrap_or(Json::Null);
        rec["circuits"] = Json::Array(objects);
        rec["start"] = t("DTSTART").map_or(Json::Null, Json::String);
        rec["end"] = t("DTEND").map_or(Json::Null, Json::String);
        rec["impact"] = one("X-MAINTNOTE-IMPACT").map(|v| json!(super::map::text(&v).to_ascii_lowercase())).unwrap_or(Json::Null);
        rec["status"] = one("X-MAINTNOTE-STATUS").map(|v| json!(super::map::text(&v).to_ascii_lowercase())).unwrap_or(Json::Null);
        rec["calendar"] = Json::Object(ev);
    }
    if rec["provider"].is_null() {
        // Without the BCOP's provider: the sender's domain, its first label
        // after a mail host's (notices@ops.zayo.com: zayo).
        let d = rec["sender_domain"].as_str().unwrap_or_default().to_string();
        let labels: Vec<&str> = d.split('.').collect();
        let name = if labels.len() >= 2 { labels[labels.len() - 2] } else { d.as_str() };
        rec["provider"] = if name.is_empty() { Json::Null } else { json!(name) };
    }
    let id = rec["message_id"].as_str().map(str::to_string).unwrap_or_else(|| super::map::hash(&json!(String::from_utf8_lossy(raw))));
    rec["id"] = json!(id);
    rec
}

/// The address of a `From:` value: `Ops <noc@x.net>` → `noc@x.net`.
fn address(from: &str) -> String {
    match (from.rfind('<'), from.rfind('>')) {
        (Some(a), Some(b)) if a < b => from[a + 1..b].trim().to_string(),
        _ => from.trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTICE: &str = "From: Example Transit NOC <noc@ops.example-transit.net>\r\nTo: netops@example.com\r\nSubject: =?UTF-8?B?UGxhbm5lZCBtYWludGVuYW5jZSDigJMgTUFJTlQtNDI=?=\r\nDate: Fri, 13 Nov 2026 03:00:00 +0000\r\nMessage-ID: <abc123@ops.example-transit.net>\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b1\"\r\n\r\n--b1\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nDear customer,=0AFiber work on circuit EXT-1001 between NYC and BOS.=\r\n Expect a 30 minute outage.\r\n--b1\r\nContent-Type: text/calendar; charset=utf-8\r\n\r\nBEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART:20261120T060000Z\r\nDTEND:20261120T080000Z\r\nUID:42@example\r\nX-MAINTNOTE-PROVIDER:example-transit\r\nX-MAINTNOTE-ACCOUNT:ACME\r\nX-MAINTNOTE-MAINTENANCE-ID:MAINT-42\r\nX-MAINTNOTE-OBJECT-ID:EXT-1001\r\nX-MAINTNOTE-OBJECT-ID:EXT-1002\r\nX-MAINTNOTE-IMPACT:OUTAGE\r\nX-MAINTNOTE-STATUS:CONFIRMED\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n--b1--\r\n";

    #[test]
    fn a_bcop_notice_gives_its_window_and_circuits() {
        let r = parse(NOTICE.as_bytes(), 0);
        assert_eq!(r["subject"], "Planned maintenance \u{2013} MAINT-42");
        assert_eq!(r["sender"], "noc@ops.example-transit.net");
        assert_eq!(r["date"], "2026-11-13T03:00:00Z");
        assert_eq!(r["id"], "abc123@ops.example-transit.net");
        assert!(r["body"].as_str().unwrap().contains("Fiber work on circuit EXT-1001 between NYC and BOS. Expect"), "{}", r["body"]);
        assert_eq!(r["provider"], "example-transit");
        assert_eq!(r["maint_id"], "MAINT-42");
        assert_eq!(r["circuits"], json!(["EXT-1001", "EXT-1002"]));
        assert_eq!(r["start"], "2026-11-20T06:00:00Z");
        assert_eq!(r["end"], "2026-11-20T08:00:00Z");
        assert_eq!(r["impact"], "outage");
        assert_eq!(r["status"], "confirmed");
    }

    #[test]
    fn a_plain_or_html_mail_keeps_its_text() {
        let plain = "From: noc@mail.carrier.com\nSubject: Emergency work\n\nCircuit ABC/123 down tonight.\n";
        let r = parse(plain.as_bytes(), 0);
        assert_eq!(r["body"], "Circuit ABC/123 down tonight.\n");
        assert_eq!(r["provider"], "carrier");
        assert!(r["id"].as_str().unwrap().len() == 16);
        let html = "From: a@b.io\nContent-Type: text/html\nContent-Transfer-Encoding: base64\n\nPHA+SGVsbG8gPGI+d29ybGQ8L2I+PC9wPjxzY3JpcHQ+eDwvc2NyaXB0Pg==\n";
        let r = parse(html.as_bytes(), 0);
        assert_eq!(r["body"].as_str().unwrap().trim(), "Hello world");
    }
}
