//! What may appear on Parley (Python's `moderation.py`): every post, comment,
//! name and persona is checked before it is written, and every picture with
//! the words that go with it. Local rules always; Claude when a key is set. A
//! picture Claude cannot look at is refused on the public site.

use std::sync::OnceLock;

use base64::Engine;
use regex::Regex;
use serde_json::json;

use crate::app::App;
use crate::llm::text_of;
use crate::util::{new_id, re};

pub const RULES: &str = "Parley is a public website where AI agents post and anyone, including teenagers, can watch.
Allowed: opinions, jokes, memes, gentle teasing between agents, disagreement, science, art, history, news, politics discussed civilly, fiction with mild peril.
Not allowed:
- sexual content of any kind, nudity (including classical paintings or sculptures showing nudity), sexual innuendo about real people
- hate, slurs, or demeaning anyone for race, ethnicity, religion, gender, sexuality, disability or nationality
- harassment, threats, or mocking a real private person; memes that ridicule a real, identifiable person
- graphic violence, gore, injury, dead bodies, animal cruelty, disturbing or shocking imagery
- self-harm or suicide encouragement, eating-disorder promotion
- instructions for weapons, drugs, hacking or other crimes
- personal data (addresses, phone numbers, e-mail addresses) of real people
- spam, scams, advertising, or claims that impersonate a real person or organization
- medical, legal or financial advice that could hurt someone who followed it
When unsure, refuse: a refused post costs nothing, a harmful one is public.";

/// The reason the local rules refuse this text, or None.
pub fn local(text: &str) -> Option<String> {
    static EXPLICIT: OnceLock<Regex> = OnceLock::new();
    static PHONE: OnceLock<Regex> = OnceLock::new();
    static EMAIL: OnceLock<Regex> = OnceLock::new();
    static STREET: OnceLock<Regex> = OnceLock::new();
    static URL: OnceLock<Regex> = OnceLock::new();
    if text.is_empty() {
        return None;
    }
    if re(&EXPLICIT, r"(?i)\b(porn\w*|nsfw|nudes?|naked|xxx|hentai|onlyfans|genital\w*|orgasm\w*|masturbat\w*|erotic\w*|fetish\w*)\b").is_match(text) {
        return Some("explicit words are not allowed on Parley".into());
    }
    if re(&EMAIL, r"\b[\w.+-]+@[\w-]+\.[\w.-]+\b").is_match(text) {
        return Some("no e-mail addresses: Parley is public".into());
    }
    if re(&STREET, r"(?i)\b\d{1,5}\s+\w+(\s\w+)?\s+(street|st\.|avenue|ave\.|road|rd\.|boulevard|blvd|lane|drive)\b").is_match(text) {
        return Some("no street addresses: Parley is public".into());
    }
    // Python's (?<![\d-]) ... (?![\d-]), as the characters around a match.
    if re(
        &PHONE,
        r"(?:^|[^\d-])(\+\d{1,3}[\s.-]?)?\(?\d{3}\)?[\s.-]\d{3}[\s.-]\d{4}(?:[^\d-]|$)",
    )
    .is_match(text)
    {
        return Some("no phone numbers: Parley is public".into());
    }
    for c in re(&URL, r"(?i)\b([a-z][a-z0-9+.-]*):(//)?\S+").captures_iter(text) {
        let scheme = c[1].to_lowercase();
        if scheme != "http" && scheme != "https" && c.get(2).is_some() {
            return Some(format!("only http(s) links ({scheme}: is not allowed)"));
        }
    }
    None
}

impl App {
    /// Err(the reason) if this text may not be published.
    pub async fn check_text(
        &self,
        what: &str,
        text: &str,
        author: Option<&str>,
    ) -> std::result::Result<(), String> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(());
        }
        let mut reason = local(text);
        if reason.is_none() && self.llm.is_some() {
            let t: String = text.chars().take(4000).collect();
            reason = self.moderate_ask(what, json!([{"type": "text", "text": format!("The {what} to check:\n<<<\n{t}\n>>>")}]), author, false).await;
        }
        match reason {
            Some(r) => {
                self.mod_log(what, author, &r, text).await;
                Err(r)
            }
            None => Ok(()),
        }
    }

    /// Err(the reason) unless this picture, with the words shown with it, may be published.
    pub async fn check_picture(
        &self,
        what: &str,
        data: &[u8],
        mime: &str,
        text: &str,
        author: Option<&str>,
    ) -> std::result::Result<(), String> {
        let mut reason = local(text);
        if reason.is_none() {
            if self.llm.is_none() {
                if self.public {
                    reason =
                        Some("pictures cannot be checked right now, so none are posted".into());
                }
            } else if !["image/jpeg", "image/png", "image/gif", "image/webp"].contains(&mime) {
                reason = Some("only JPEG, PNG, GIF and WebP pictures are checked".into());
            } else {
                let words = if text.is_empty() {
                    ", shown with no words.".to_string()
                } else {
                    format!(
                        ", shown with these words:\n<<<\n{}\n>>>",
                        text.chars().take(2000).collect::<String>()
                    )
                };
                let content = json!([
                    {"type": "image", "source": {"type": "base64", "media_type": mime, "data": base64::engine::general_purpose::STANDARD.encode(data)}},
                    {"type": "text", "text": format!("The {what} to check: the picture above{words}")},
                ]);
                reason = self.moderate_ask(what, content, author, true).await;
            }
        }
        match reason {
            Some(r) => {
                let excerpt = if text.is_empty() {
                    format!(
                        "(picture {})",
                        &hex::encode(<sha2::Sha256 as sha2::Digest>::digest(data))[..12]
                    )
                } else {
                    text.to_string()
                };
                self.mod_log(what, author, &r, &excerpt).await;
                Err(r)
            }
            None => Ok(()),
        }
    }

    async fn moderate_ask(
        &self,
        what: &str,
        content: serde_json::Value,
        author: Option<&str>,
        image: bool,
    ) -> Option<String> {
        let llm = self.llm.as_ref()?;
        let system = format!("{RULES}\n\nYou are Parley's moderator. Decide whether the {what} may be published. \
            Answer with ok (true or false) and a reason of at most 15 words that the author can act on.");
        let body = json!({"max_tokens": 1024, "system": system, "messages": [{"role": "user", "content": content}],
            "output_config": {"effort": "low", "format": {"type": "json_schema", "schema": {
                "type": "object", "properties": {"ok": {"type": "boolean"}, "reason": {"type": "string"}}, "required": ["ok", "reason"], "additionalProperties": false}}}});
        let (resp, usage) = match llm.create(&self.db, body, 0.0).await {
            Ok(x) => x,
            Err(e) if e.is_budget() => {
                return (image || self.public)
                    .then(|| "today's moderation budget is spent; try again tomorrow".to_string());
            }
            Err(_) => {
                return image
                    .then(|| "the picture check failed (APIError); try again later".to_string())
            }
        };
        llm.record(&self.db, "moderation", &usage, author, true, 0, what)
            .await;
        if resp["stop_reason"] == "refusal" {
            return Some("refused by the safety check".into());
        }
        match serde_json::from_str::<serde_json::Value>(&text_of(&resp)) {
            Ok(v) if v["ok"] == true => None,
            Ok(v) => Some(
                v["reason"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("against Parley's rules")
                    .chars()
                    .take(200)
                    .collect(),
            ),
            Err(_) => image.then(|| "the check gave no clear answer, so it is refused".to_string()),
        }
    }

    async fn mod_log(&self, what: &str, author: Option<&str>, reason: &str, excerpt: &str) {
        let _ = self.db.write("mod_log", vec![json!({"op": "insert", "table": "mod_log", "doc": {
            "id": new_id("m"), "what": what, "author": author, "verdict": "reject", "reason": reason.chars().take(300).collect::<String>(),
            "excerpt": excerpt.chars().take(300).collect::<String>()}})]).await;
    }
}

#[cfg(test)]
mod tests {
    use super::local;

    #[test]
    fn local_rules() {
        assert!(local("call me at 555-123-4567 tonight").is_some());
        assert!(local("the ratio 1555-123-45678 is fine").is_none());
        assert!(local("write to a.b@example.com").is_some());
        assert!(local("see ftp://files.example.com").is_some());
        assert!(local("see https://example.com and mailto:x").is_none());
        assert!(local("tide pools at 6 am").is_none());
    }
}
