//! Removes credentials from text before it is stored or sent.
//!
//! Each secret is replaced by a marker that names its kind,
//! `[REDACTED:<kind>]`, and the text around it is kept: what a command did
//! stays searchable, the key it used does not. What is caught:
//!
//! - keys and tokens by their shape: Anthropic (`sk-ant-`), OpenAI (`sk-`,
//!   `sk-proj-`), GitHub (`ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_`,
//!   `github_pat_`), GitLab (`glpat-`), npm (`npm_`), AWS access key ids
//!   (`AKIA`, `ASIA`), Google API keys (`AIza`) and OAuth tokens (`ya29.`),
//!   Slack tokens (`xox?-`) and webhooks, Discord webhooks, Stripe (`sk_live_`,
//!   `rk_live_`, `whsec_`), DigitalOcean (`dop_v1_`), Hugging Face (`hf_`),
//!   SendGrid (`SG.`), PyPI (`pypi-AgE`), HashiCorp Vault (`hvs.`, `hvb.`,
//!   `hvr.`), age secret keys (`AGE-SECRET-KEY-1`), Shopify (`shpat_` and
//!   kin), Square (`sq0atp-`, `sq0csp-`), Linear (`lin_api_`), Postman
//!   (`PMAK-`), New Relic (`NRAK-` and kin), Grafana (`glsa_`, `glc_`),
//!   Sentry (`sntrys_`, `sntryu_`), Mailgun (`key-` and 32 hex digits),
//!   Doppler (`dp.st.` and kin), Databricks (`dapi`), Supabase (`sbp_`),
//!   Tailscale (`tskey-`), Atlassian (`ATATT3`), Pulumi (`pul-`), Terraform
//!   Cloud (`.atlasv1.`), ChloeDB API keys (`sk_<id>_<secret>`) and JWTs;
//! - PEM private key blocks (also one cut off before its END line);
//! - `Authorization:` headers and `Bearer` tokens, `curl -u user:pass`, and
//!   the password of a URL (`scheme://user:pass@host`);
//! - a command line's password and key flags (`--password=...`,
//!   `--token ...`, `--api-key ...`), `mysql -p...` and `sshpass -p ...`;
//! - the credentials of a Docker `config.json` or an `.npmrc` (`"auth"`,
//!   `_auth`);
//! - assignments whose name ends in a word for a secret (token, secret,
//!   password, passwd, pass, passphrase, credential, a qualified key such as
//!   `api_key` or `AWS_SECRET_ACCESS_KEY`), also with a number or a word such
//!   as `prod`, `old` or `ro` after it (`STAGING_PASS2`, `GITHUB_TOKEN_RO`,
//!   `API_KEY_2`): `NAME=value` in an environment
//!   dump or a `.env` file, `name: value` in YAML, `"name": "value"` in
//!   JSON, `name = "value"` in code, also after a label (`note:
//!   DB_PASSWORD=...`, a CI file's `run: PGPASSWORD=... psql`), and
//!   `NAME value` with a long value (a Dockerfile's `ENV`). The name is
//!   kept.
//!
//! It is best effort: a secret in no shape it knows (a password in prose, a
//! random string under an unremarkable name) is stored as it is. It leaves
//! alone what only looks random: git hashes, digests, UUIDs, and code that
//! names a secret without holding one (`token = get_token()`,
//! `password: string`).
//!
//! [`redact`] is idempotent: text redacted once does not change again, so
//! `chloectx redact` can tell which stored records still need it.

use std::borrow::Cow;
use std::sync::OnceLock;

use regex::{Captures, Regex};
use serde_json::Value as Json;

enum Rule {
    /// The whole match is the secret.
    Whole(Regex, &'static str),
    /// The function rewrites the match (keeping what is not secret, or all
    /// of it when the match turns out to hold no secret).
    Custom(Regex, fn(&Captures) -> String),
}

fn rules() -> &'static [Rule] {
    static R: OnceLock<Vec<Rule>> = OnceLock::new();
    R.get_or_init(|| {
        let re = |p: &str| Regex::new(p).unwrap();
        let whole = |p: &str, kind: &'static str| Rule::Whole(re(p), kind);
        vec![
            // A PEM block, or its start when the text was cut before the END line.
            whole(
                r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----(?:[\s\S]*?-----END [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----|(?:(?:\s|\\[nr])*[A-Za-z0-9+/=]{16,})*[A-Za-z0-9+/=]*)",
                "private-key",
            ),
            whole(r"\bsk-ant-[A-Za-z0-9_\-]{20,}", "anthropic-key"),
            whole(r"\bsk-(?:proj-|svcacct-|admin-)?[A-Za-z0-9_\-]{20,}", "openai-key"),
            whole(r"\bgh[pousr]_[A-Za-z0-9]{30,}\b|\bgithub_pat_[A-Za-z0-9_]{22,}", "github-token"),
            whole(r"\bglpat-[A-Za-z0-9_\-]{20,}", "gitlab-token"),
            whole(r"\bnpm_[A-Za-z0-9]{36}\b", "npm-token"),
            whole(r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b", "aws-access-key-id"),
            whole(r"\bAIza[0-9A-Za-z_\-]{35}", "google-api-key"),
            whole(r"\bya29\.[0-9A-Za-z_\-]{20,}", "google-oauth-token"),
            whole(r"\bxox[abposr]-[A-Za-z0-9\-]{10,}", "slack-token"),
            whole(r"https://hooks\.slack\.com/services/[A-Za-z0-9_/\-]{20,}", "slack-webhook"),
            whole(r"https://(?:canary\.|ptb\.)?discord(?:app)?\.com/api/webhooks/[0-9]+/[A-Za-z0-9_\-]{20,}", "discord-webhook"),
            whole(r"\b(?:sk|rk)_(?:live|test)_[A-Za-z0-9]{10,}|\bwhsec_[A-Za-z0-9]{20,}", "stripe-key"),
            whole(r"\bdo[por]_v1_[a-f0-9]{64}\b", "digitalocean-token"),
            whole(r"\bhf_[A-Za-z0-9]{30,}\b", "huggingface-token"),
            whole(r"\bSG\.[A-Za-z0-9_\-]{16,}\.[A-Za-z0-9_\-]{16,}", "sendgrid-key"),
            // LENA-302: shapes with a fixed, documented prefix.
            whole(r"\bpypi-AgE[A-Za-z0-9_\-]{50,}", "pypi-token"),
            whole(r"\bhv[sbr]\.[A-Za-z0-9_\-]{24,}", "vault-token"),
            whole(r"\bAGE-SECRET-KEY-1[0-9A-Z]{50,}", "age-secret-key"),
            whole(r"\bshp(?:at|ca|pa|ss)_[a-fA-F0-9]{32}\b", "shopify-token"),
            whole(r"\bsq0(?:atp|csp)-[A-Za-z0-9_\-]{22,}", "square-token"),
            whole(r"\blin_(?:api|oauth)_[A-Za-z0-9]{32,}", "linear-key"),
            whole(r"\bPMAK-[A-Za-z0-9]{20,}-[A-Za-z0-9]{30,}", "postman-key"),
            whole(r"\bNR(?:AK|AA|II|IQ)-[A-Za-z0-9_\-]{20,}", "newrelic-key"),
            whole(r"\bglsa_[A-Za-z0-9]{32}_[A-Fa-f0-9]{8}\b|\bglc_[A-Za-z0-9+/_\-]{32,}={0,2}", "grafana-token"),
            whole(r"\bsntry[su]_[A-Za-z0-9+/_\-]{30,}={0,2}", "sentry-token"),
            whole(r"\bkey-[0-9a-f]{32}\b", "mailgun-key"),
            whole(r"\bdp\.(?:st|pt|sa|ct|scim|audit)\.(?:[a-z0-9_\-]+\.)?[A-Za-z0-9]{40,}", "doppler-token"),
            whole(r"\bdapi[0-9a-f]{32}(?:-[0-9]+)?\b", "databricks-token"),
            whole(r"\bsbp_[0-9a-f]{40}\b", "supabase-token"),
            whole(r"\btskey-(?:auth|api|client|webhook)-[A-Za-z0-9]{6,}-[A-Za-z0-9]{20,}", "tailscale-key"),
            whole(r"\bATATT3[A-Za-z0-9_\-=]{60,}", "atlassian-token"),
            whole(r"\bpul-[0-9a-f]{40}\b", "pulumi-token"),
            whole(r"\b[A-Za-z0-9]{14}\.atlasv1\.[A-Za-z0-9_\-]{60,}", "terraform-token"),
            // chloedb-server's API keys: `sk_<16 hex>_<48 hex>`.
            whole(r"\bsk_[0-9a-f]{4,32}_[0-9a-f]{16,}\b", "chloedb-key"),
            whole(r"\beyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}", "jwt"),
            Rule::Custom(re(r#"(?i)\b((?:proxy-)?authorization(?:\\?["'])?[ \t]*[:=][ \t]*(?:\\?["'])?)(?:(bearer|basic|token|bot|digest|apikey|key)([ \t]+))?([^\s"'`,;\\]+)"#), authorization),
            Rule::Custom(re(r"(?i)\b(bearer[ \t]+)([A-Za-z0-9._~+/=\-]{16,})"), bearer),
            Rule::Custom(re(r#"(^|\s)(-u|--user)([ \t]+|=)(["']?)([^\s:"'$]+):([^\s"'`]+)"#), curl_user),
            Rule::Custom(re(r#"\b([A-Za-z][A-Za-z0-9+.\-]*://)([^\s:/?#@"'<>\[\]]*):([^\s/?#@"'<>]+)@"#), url_password),
            // `--password=...`, `--token ...` (LENA-302).
            Rule::Custom(re(r#"(^|[\s"'`(])(--?[A-Za-z][A-Za-z0-9_\-]*)(=|[ \t]+)("[^"\n]*"|'[^'\n]*'|[^\s"'`;|&)\-][^\s"'`;|&)]*)"#), flag),
            // `mysql -u root -psecret db`, `sshpass -p secret ssh host` (LENA-302).
            Rule::Custom(re(r#"\b(mysql(?:dump|admin|import|sh|check|slap)?|mariadb(?:-dump|-admin|dump|admin)?|sshpass)\b([^\n|;&]*?[ \t])-p([ \t]*)("[^"\n]*"|'[^'\n]*'|[^\s"'`;|&]+)"#), short_password),
            // `ENV DB_PASSWORD hunter2...`, `DD_API_KEY 0123...`: a long value after a space.
            Rule::Custom(re(r"\b([A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+)([ \t]+)([A-Za-z0-9+/=_\-]{16,})"), spaced),
            Rule::Custom(
                re(r#"(?P<name>[A-Za-z_][A-Za-z0-9_.\-]*)(?P<q>(?:\\?["'])?)(?P<sep>[ \t]*(?:=>|:=|[:=])[ \t]*)(?P<val>\\"[^"\\\n]*\\"|"(?:[^"\\\n]|\\.)*"|'[^'\n]*'|[^\s"'`,;&\\]+)"#),
                assignment,
            ),
        ]
    })
}

/// Words one of which is in any text a rule can match: text without them
/// is returned as it is without running the rules.
fn worth_a_look() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)key|token|secret|pass|credential|auth|bearer|-u|--user|://|sk[-_]|gh[pousr]_|github_pat_|glpat-|npm_|AKIA|ASIA|AIza|ya29\.|xox|whsec_|_v1_|hf_|SG\.|eyJ|pypi-|hv[sbr]\.|shp|sq0|lin_|PMAK-|NR..-|glsa_|glc_|sntry|dp\.|dapi|sbp_|ATATT|pul-|atlasv1|mysql|mariadb").unwrap())
}

/// The marker that replaces a secret of `kind`.
pub fn marker(kind: &str) -> String {
    format!("[REDACTED:{kind}]")
}

/// Whether `v` is a marker already, this version's or the bare
/// `[REDACTED]` of earlier ones.
fn is_marker(v: &str) -> bool {
    v.trim_start_matches(['"', '\'', '\\']).starts_with("[REDACTED")
}

/// `text` with credentials replaced by markers (see the module's comment).
pub fn redact(text: &str) -> String {
    if !worth_a_look().is_match(text) {
        return text.to_string();
    }
    let mut out = Cow::Borrowed(text);
    for rule in rules() {
        let next = match rule {
            Rule::Whole(re, kind) => re.replace_all(&out, regex::NoExpand(&marker(kind))),
            Rule::Custom(re, f) => re.replace_all(&out, |c: &Captures| f(c)),
        };
        if let Cow::Owned(s) = next {
            out = Cow::Owned(s);
        }
    }
    out.into_owned()
}

/// Redacts every string in `v`, and replaces the value of an object's key
/// that names a secret (`{"DB_PASSWORD": "..."}`) as a whole. Whether
/// anything changed.
pub fn redact_json(v: &mut Json) -> bool {
    match v {
        Json::String(s) => {
            let r = redact(s);
            let changed = r != *s;
            *s = r;
            changed
        }
        Json::Array(xs) => xs.iter_mut().fold(false, |c, x| redact_json(x) | c),
        Json::Object(m) => {
            let mut changed = false;
            for (k, x) in m.iter_mut() {
                if let (Some(kind), Json::String(s)) = (secret_name(k), &*x) {
                    if holds_secret(kind, s, true, is_env_name(k)) {
                        *x = Json::String(marker(kind));
                        changed = true;
                        continue;
                    }
                }
                changed |= redact_json(x);
            }
            changed
        }
        _ => false,
    }
}

/// Redacts a record's text and context in place; whether either changed.
pub fn redact_record(r: &mut crate::record::Record) -> bool {
    let (text, ctx) = (redact(&r.text), redact(&r.ctx));
    let changed = text != r.text || ctx != r.ctx;
    r.text = text;
    r.ctx = ctx;
    changed
}

/// How many markers of each kind `text` holds (for reports, which never
/// show what was replaced).
pub fn count_markers(text: &str, into: &mut std::collections::BTreeMap<String, u64>) {
    static R: OnceLock<Regex> = OnceLock::new();
    let re = R.get_or_init(|| Regex::new(r"\[REDACTED:([a-z0-9\-]+)\]").unwrap());
    for c in re.captures_iter(text) {
        *into.entry(c[1].to_string()).or_default() += 1;
    }
}

// ---------------------------------------------------------------- rules

/// `Authorization: <scheme> <credentials>`: the credentials.
fn authorization(c: &Captures) -> String {
    let v = &c[4];
    if is_marker(v) || v.starts_with('$') || v.len() < 8 || !(has_digit(v) || v.len() >= 20) || v.contains(['<', '>', '(', ')']) {
        return c[0].to_string();
    }
    let scheme = c.get(2).map_or("", |m| m.as_str());
    let kind = match scheme.to_ascii_lowercase().as_str() {
        "basic" => "basic-auth",
        "bearer" => "bearer-token",
        _ => "authorization",
    };
    format!("{}{scheme}{}{}", &c[1], c.get(3).map_or("", |m| m.as_str()), marker(kind))
}

/// `Bearer <token>` outside a header (in a curl command, a log): a token
/// has a digit, a word after "bearer" in prose has none.
fn bearer(c: &Captures) -> String {
    if !has_digit(&c[2]) {
        return c[0].to_string();
    }
    format!("{}{}", &c[1], marker("bearer-token"))
}

/// `curl -u user:password` (also `--user=`): the password.
fn curl_user(c: &Captures) -> String {
    let (user, pass) = (&c[5], &c[6]);
    let numeric = |s: &str| s.chars().all(|ch| ch.is_ascii_digit());
    // `docker run -u 1000:1000` is a user and a group.
    if is_marker(pass) || pass.starts_with('$') || (numeric(user) && numeric(pass)) {
        return c[0].to_string();
    }
    format!("{}{}{}{}{user}:{}", &c[1], &c[2], &c[3], &c[4], marker("password"))
}

/// `scheme://user:password@host`: the password.
fn url_password(c: &Captures) -> String {
    let pass = &c[3];
    let placeholder = matches!(pass.to_ascii_lowercase().as_str(), "password" | "pass" | "passwd" | "pwd" | "secret");
    if is_marker(pass) || pass.starts_with('$') || pass.starts_with('{') || placeholder {
        return c[0].to_string();
    }
    format!("{}{}:{}@", &c[1], &c[2], marker("password"))
}

/// `name = value` and its kin, where the name says the value is a secret.
/// Where it does not, the value is looked at in turn: in `note:
/// DB_PASSWORD=hunter2` the match is `note` and its value
/// `DB_PASSWORD=hunter2`, which holds the assignment (ANA-301), and in
/// JSON's `"cmd": "PGPASSWORD=... psql"` the string does.
fn assignment(c: &Captures) -> String {
    let name = &c["name"];
    let (sep, val) = (&c["sep"], &c["val"]);
    let (open, inner, close) = unquote(val);
    if let Some(kind) = secret_name(name) {
        if holds_secret(kind, inner, !open.is_empty(), is_env_name(name)) {
            return format!("{name}{}{sep}{open}{}{close}", &c["q"], marker(kind));
        }
    }
    // The value is shorter than the match, so this ends.
    format!("{name}{}{sep}{open}{}{close}", &c["q"], assignments(inner))
}

/// `text` with the assignments rule applied (the others have been, to the
/// whole text it is part of).
fn assignments(text: &str) -> String {
    let Some(Rule::Custom(re, f)) = rules().last() else { unreachable!("the assignments rule is the last") };
    re.replace_all(text, |c: &Captures| f(c)).into_owned()
}

/// `--password=value`, `--token value` and their kin on a command line
/// (LENA-302): the flag's name (`--db-password`, `--api-key`, not
/// `--password-file` or `--token-stdin`) says the value is a secret.
fn flag(c: &Captures) -> String {
    let (lead, name, sep, val) = (&c[1], &c[2], &c[3], &c[4]);
    let keep = || c[0].to_string();
    let Some(kind) = secret_name(name.trim_start_matches('-')).filter(|k| *k != "auth") else { return keep() };
    let (open, inner, close) = unquote(val);
    if is_marker(inner) || inner.starts_with('-') || !secret_value(inner, true, true) {
        return keep();
    }
    // `--token value`: a word after a flag may be the next argument
    // (`mysql --password mydb` asks for the password), so it is taken for
    // the secret when it looks like one.
    if sep != "=" && open.is_empty() && !(inner.len() >= 6 && (has_digit(inner) || inner.chars().any(|ch| ch.is_ascii_uppercase()) || inner.contains(['-', '_', '+', '/', '=', '!', '@', '#', '%', '^', '*', '~', '.']))) {
        return keep();
    }
    format!("{lead}{name}{sep}{open}{}{close}", marker(kind))
}

/// `mysql -psecret` (the password joined to the flag, as MySQL's tools
/// take it) and `sshpass -p secret`: `-p` means a port or a path to most
/// other programs, so only after them.
fn short_password(c: &Captures) -> String {
    let (prog, between, space, val) = (&c[1], &c[2], &c[3], &c[4]);
    let (open, inner, close) = unquote(val);
    // `mysql -p` with a space after it asks for the password: what follows is the database.
    let joined = space.is_empty() || prog == "sshpass";
    if !joined || is_marker(inner) || inner.starts_with(['$', '-']) || inner.is_empty() {
        return c[0].to_string();
    }
    format!("{prog}{between}-p{space}{open}{}{close}", marker("password"))
}

/// `NAME value` with no `=` or `:` between them (a Dockerfile's `ENV NAME
/// value`, a dashboard copied as text): an environment variable's name for
/// a secret, then a long value with letters and digits.
fn spaced(c: &Captures) -> String {
    let (name, space, val) = (&c[1], &c[2], &c[3]);
    let Some(kind) = secret_name(name).filter(|k| *k != "auth") else { return c[0].to_string() };
    if is_marker(val) || !has_digit(val) || !val.chars().any(|ch| ch.is_ascii_alphabetic()) || !secret_value(val, false, true) {
        return c[0].to_string();
    }
    format!("{name}{space}{}", marker(kind))
}

/// The value of a quoted string, and its quotes.
fn unquote(v: &str) -> (&str, &str, &str) {
    for q in ["\\\"", "\"", "'"] {
        if v.len() >= 2 * q.len() && v.starts_with(q) && v.ends_with(q) {
            return (q, &v[q.len()..v.len() - q.len()], q);
        }
    }
    ("", v, "")
}

/// Keys that do not open anything when the word before them says so.
const NOT_SECRET_KEYS: &[&str] = &[
    "primary", "sort", "sorting", "partition", "foreign", "public", "pub", "cache", "idempotency", "unique", "hash", "row", "map", "lookup", "group", "grouping", "index",
    "shard", "routing", "composite", "compound", "natural", "surrogate", "dedup", "dedupe", "join", "cursor", "page", "object", "type", "kind", "hot", "short", "sub",
    "list", "next", "prev", "first", "last", "sequence", "seq", "order", "range", "batch", "entity", "record", "dict", "search", "item", "node", "column", "field",
    "table", "host", "file", "s3", "translation", "i18n", "message", "label", "config", "setting", "env", "react", "foreign", "registry", "music", "piano",
];

/// Words that may follow the word for a secret in its name and do not make
/// it something else: `GITHUB_TOKEN_RO`, `DB_PASSWORD_STAGING`, `KEY_OLD`.
const SECRET_QUALIFIERS: &[&str] = &[
    "old", "new", "prev", "next", "current", "prod", "production", "staging", "stage", "dev", "test", "qa", "local", "ro", "rw", "readonly", "admin", "root",
    "main", "backup", "alt", "secondary", "b64", "base64", "raw", "plain", "value",
];

/// What kind of secret a name says its value is, if any: its last word
/// (split at `_`, `-`, `.` and camel case; after the qualifiers and numbers
/// that may follow it: `STAGING_PASS2`, `TOKEN_2`, `API_KEY_PROD`) is a word
/// for a secret, or ends in one (`PGPASSWORD`), or is `pass` after another
/// word (`DB_PASS`, not Go's `--- PASS: TestX`), or is `key` after a word that
/// makes it one (`api_key`, `AWS_SECRET_ACCESS_KEY`, `apiKey`, but not
/// `primary_key`).
fn secret_name(name: &str) -> Option<&'static str> {
    let mut words = words(name);
    // A number or a qualifier after the word for a secret (CTX-35, LENA-10:
    // `STAGING_PASS2=` was stored as it was).
    while words.len() > 1 && words.last().is_some_and(|w| SECRET_QUALIFIERS.contains(&w.as_str()) || is_version(w)) {
        words.pop();
    }
    if let Some(w) = words.last_mut() {
        let kept = w.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        if kept > 0 {
            w.truncate(kept);
        }
    }
    let last = words.last()?.as_str();
    let has = |w: &str| words.iter().any(|x| x == w);
    let kind = if last.ends_with("password") || last.ends_with("passwd") || last == "passphrase" || (last == "pass" && words.len() >= 2) {
        "password"
    } else if last.ends_with("token") {
        "token"
    } else if last.ends_with("secret") {
        "secret"
    } else if last == "credential" || last == "credentials" {
        "credential"
    } else if matches!(last, "apikey" | "accesskey" | "secretkey" | "privatekey") {
        "key"
    } else if last == "key" && words.len() >= 2 && !NOT_SECRET_KEYS.contains(&words[words.len() - 2].as_str()) {
        "key"
    } else if words.len() == 1 && last == "auth" {
        // A Docker `config.json`'s `"auth"` and an `.npmrc`'s `_auth`: a
        // user and password in base64 ([`auth_value`]).
        "auth"
    } else {
        return None;
    };
    // `AWS_SECRET_ACCESS_KEY`, and `SecretAccessKey` in what `aws sts` prints.
    let aws = (has("aws") && (has("secret") || last == "secretkey")) || (has("secret") && has("access") && last == "key");
    Some(if aws { "aws-secret-key" } else { kind })
}

/// `2`, `v2`: a number after a name's word.
fn is_version(w: &str) -> bool {
    let d = w.strip_prefix('v').unwrap_or(w);
    !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())
}

/// A name's words, lowercased: `AWS_SECRET_ACCESS_KEY`, `x-api-key`,
/// `secretAccessKey` and `APIKey` split as one would read them.
fn words(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    for (i, &ch) in chars.iter().enumerate() {
        if matches!(ch, '_' | '-' | '.') {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if ch.is_ascii_uppercase() && !cur.is_empty() {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(|c| c.is_ascii_lowercase());
            if prev.is_ascii_lowercase() || prev.is_ascii_digit() || (prev.is_ascii_uppercase() && next_lower) {
                out.push(std::mem::take(&mut cur));
            }
        }
        cur.push(ch.to_ascii_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// `UPPER_SNAKE`: an environment variable or a config key, whose value is
/// a literal (in code, a name in lower or camel case is often assigned an
/// expression).
fn is_env_name(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn has_digit(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_digit())
}

/// Whether `v`, assigned to a name that says its value is a secret of
/// `kind`, holds one ([`secret_value`]; for `auth`, [`auth_value`]).
fn holds_secret(kind: &str, v: &str, quoted: bool, env: bool) -> bool {
    if kind == "auth" {
        auth_value(v)
    } else {
        secret_value(v, quoted, env)
    }
}

/// Whether `v`, the value of an `auth` (not of `auth: required` or
/// `"auth": "oauth2"`), is credentials in base64.
fn auth_value(v: &str) -> bool {
    let v = v.trim();
    v.len() >= 12 && !is_marker(v) && v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=')) && (has_digit(v) || v.ends_with('='))
}

/// Whether `v`, assigned to a name for a secret, holds one rather than
/// naming where one comes from (`$TOKEN`, `${{ secrets.X }}`, a path, a
/// type, an expression) or being a placeholder.
fn secret_value(v: &str, quoted: bool, env: bool) -> bool {
    let v = v.trim();
    if v.is_empty() || is_marker(v) {
        return false;
    }
    // References, templates and placeholders.
    if v.starts_with('$') || v.starts_with('%') || v.starts_with("{{") || (v.starts_with('<') && v.ends_with('>')) {
        return false;
    }
    if v.chars().all(|c| matches!(c, '*' | 'x' | 'X' | '.' | '•' | '-' | '#')) {
        return false;
    }
    // Paths (`GOOGLE_APPLICATION_CREDENTIALS=/home/me/key.json`).
    if ["/", "~/", "./", "../"].iter().any(|p| v.starts_with(p)) {
        return false;
    }
    let lower = v.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "none" | "nil" | "undefined" | "required" | "optional" | "string" | "str" | "bytes" | "bool" | "boolean" | "int" | "integer" | "number"
    ) {
        return false;
    }
    if v.len() < 8 && v.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if quoted {
        // `"password": "hunter2"`; a sentence is a description, not a secret.
        return env || (v.chars().count() >= 4 && !v.chars().any(char::is_whitespace));
    }
    // Expressions and types: `get_token()`, `os.environ["X"]`, `Option<String>`, `&str`.
    if v.contains(['(', ')', '[', ']', '{', '}', '<', '>', '&', '|', '\\']) {
        return false;
    }
    if env {
        return true;
    }
    // `password = hunter2hunter2`, `token: abc-123`; not `password = self.password`.
    v.len() >= 6 && (has_digit(v) || v.contains(['-', '+', '/', '=', '!', '@', '#', '%', '^', '*', '~']))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fake secrets, built at run time so that no literal in this file has
    // the shape of a real one.
    fn rep(s: &str, n: usize) -> String {
        s.repeat(n)
    }
    fn hex(n: usize) -> String {
        "0123456789abcdef".chars().cycle().skip(3).take(n).collect()
    }
    fn alnum(n: usize) -> String {
        "aB3dE5gH7jK9mN1pQ2sT4vW6yZ8".chars().cycle().take(n).collect()
    }

    /// `text` redacted: it lost `secret`, has the marker for `kind`, and
    /// does not change when redacted again.
    fn caught(text: &str, secret: &str, kind: &str) -> String {
        let r = redact(text);
        assert!(!r.contains(secret), "{kind}: {secret:?} survived in {r:?}");
        assert!(r.contains(&marker(kind)), "{kind}: no marker in {r:?}");
        assert_eq!(redact(&r), r, "not idempotent");
        r
    }

    fn kept(text: &str) {
        assert_eq!(redact(text), text);
    }

    #[test]
    fn keys_by_their_shape() {
        let ant = format!("sk-ant-api03-{}", alnum(40));
        let r = caught(&format!("export ANTHROPIC_API_KEY={ant} && claude"), &ant, "anthropic-key");
        assert_eq!(r, "export ANTHROPIC_API_KEY=[REDACTED:anthropic-key] && claude");
        let oai = format!("sk-proj-{}", alnum(48));
        caught(&format!("OPENAI_API_KEY={oai}"), &oai, "openai-key");
        caught(&format!("key {}", format_args!("sk-{}", alnum(48))), &alnum(48), "openai-key");
        for p in ["ghp", "gho", "ghu", "ghs", "ghr"] {
            let t = format!("{p}_{}", alnum(36));
            caught(&format!("token: {t}"), &t, "github-token");
        }
        let pat = format!("github_pat_{}_{}", alnum(22), alnum(59));
        caught(&format!("gh auth login --with-token <<< {pat}"), &pat, "github-token");
        let akia = format!("AKIA{}", rep("Q7", 8));
        caught(&format!("aws_access_key_id = {akia}"), &akia, "aws-access-key-id");
        let asia = format!("ASIA{}", rep("Z3", 8));
        caught(&format!("\"AccessKeyId\": \"{asia}\""), &asia, "aws-access-key-id");
        let goog = format!("AIza{}", alnum(35));
        caught(&format!("https://maps.googleapis.com/maps/api/js?key={goog}"), &goog, "google-api-key");
        let ya = format!("ya29.{}", alnum(60));
        caught(&ya, &ya, "google-oauth-token");
        for p in ["xoxb", "xoxp", "xoxa", "xoxr", "xoxs"] {
            let t = format!("{p}-{}-{}-{}", "1234567890", "0987654321", alnum(24));
            caught(&format!("SLACK says {t}"), &t, "slack-token");
        }
        let hook = format!("https://hooks.slack.com/services/T{}/B{}/{}", rep("0A", 4), rep("1B", 4), alnum(24));
        caught(&format!("curl -X POST {hook}"), &hook, "slack-webhook");
        for p in ["sk_live", "rk_live", "sk_test"] {
            let t = format!("{p}_{}", alnum(24));
            caught(&format!("stripe.api_key = {t}"), &t, "stripe-key");
        }
        let dop = format!("dop_v1_{}", hex(64));
        caught(&format!("doctl auth init -t {dop}"), &dop, "digitalocean-token");
        let hf = format!("hf_{}", alnum(34));
        caught(&format!("huggingface-cli login --token {hf}"), &hf, "huggingface-token");
        let glpat = format!("glpat-{}", alnum(20));
        caught(&glpat, &glpat, "gitlab-token");
        let npm = format!("npm_{}", alnum(36));
        caught(&format!("//registry.npmjs.org/:_authToken={npm}"), &npm, "npm-token");
        // chloedb-server's keys.
        let ck = format!("sk_{}_{}", hex(16), hex(48));
        caught(&format!("CHLOEDB_KEY={ck}"), &ck, "chloedb-key");
        caught(&format!("{{\"secret\": \"{ck}\"}}"), &ck, "chloedb-key");
    }

    #[test]
    fn jwts_private_keys_and_headers() {
        let jwt = format!("eyJ{}.eyJ{}.{}", alnum(30), alnum(60), alnum(43));
        caught(&format!("the cookie was {jwt}; it expired"), &jwt, "jwt");
        let r = caught(&format!("curl -H 'Authorization: Bearer {jwt}' https://api"), &jwt, "jwt");
        assert!(r.contains("Authorization: Bearer [REDACTED:jwt]"), "{r}");
        let opaque = alnum(32);
        let r = caught(&format!("curl -H \"Authorization: Bearer {opaque}\" https://api"), &opaque, "bearer-token");
        assert!(r.contains("\"Authorization: Bearer [REDACTED:bearer-token]\""), "{r}");
        caught(&format!("token = 'Bearer {opaque}'"), &opaque, "bearer-token");
        let basic = "YWRtaW46aHVudGVyMmh1bnRlcjI=";
        let r = caught(&format!("Authorization: Basic {basic}"), basic, "basic-auth");
        assert_eq!(r, "Authorization: Basic [REDACTED:basic-auth]");
        caught(&format!("\"authorization\": \"{opaque}\""), &opaque, "authorization");
        let body = format!("{}\n{}\n{}", alnum(64), alnum(64), alnum(20));
        let pem = format!("-----BEGIN {}PRIVATE KEY-----\n{body}\n-----END {}PRIVATE KEY-----", "RSA ", "RSA ");
        let r = caught(&format!("cat id_rsa\n{pem}\nok"), &alnum(64), "private-key");
        assert_eq!(r, "cat id_rsa\n[REDACTED:private-key]\nok");
        // Cut off by a clip before its END line, and inside JSON.
        let cut = format!("-----BEGIN OPENSSH PRIVATE KEY-----\n{}\n{}", alnum(70), alnum(30));
        let r = caught(&format!("{cut}\n\nThat was the key."), &alnum(30), "private-key");
        assert!(r.ends_with("\n\nThat was the key."), "{r}");
        let json = format!("{{\"content\":\"-----BEGIN PRIVATE KEY-----\\n{}\\n{}", alnum(64), alnum(10));
        caught(&json, &alnum(64), "private-key");
        // A public key is not a secret.
        kept("-----BEGIN PUBLIC KEY-----\nMFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE\n-----END PUBLIC KEY-----");
    }

    #[test]
    fn passwords_in_urls_and_commands() {
        let pw = "s3cr3tpassw0rd";
        let r = caught(&format!("DATABASE_URL=postgres://app:{pw}@db.internal:5432/app"), pw, "password");
        assert_eq!(r, "DATABASE_URL=postgres://app:[REDACTED:password]@db.internal:5432/app");
        caught(&format!("git clone https://x-access-token:{pw}@github.com/o/r.git"), pw, "password");
        caught(&format!("redis://:{pw}@cache:6379"), pw, "password");
        let r = caught(&format!("curl -u admin:{pw} https://host/api"), pw, "password");
        assert_eq!(r, "curl -u admin:[REDACTED:password] https://host/api");
        caught(&format!("curl --user=admin:{pw} https://host"), pw, "password");
        kept("git clone http://local_proxy@127.0.0.1:1234/git/chloetech-ai/strata");
        kept("see https://example.com:8080/a@b and git@github.com:owner/repo.git");
        kept("docker run -u 1000:1000 image && git push -u origin main && sort -u a.txt");
        kept("postgres://user:password@localhost/db is the template");
    }

    #[test]
    fn assignments_whose_name_says_secret() {
        let aws = format!("{}/{}/{}", alnum(13), alnum(7), alnum(18));
        caught(&format!("AWS_SECRET_ACCESS_KEY={aws}"), &aws, "aws-secret-key");
        caught(&format!("aws_secret_access_key = {aws}"), &aws, "aws-secret-key");
        caught(&format!("\"SecretAccessKey\": \"{aws}\""), &aws, "aws-secret-key");
        let cases = [
            ("DB_PASSWORD=correct-horse-battery", "correct-horse", "password"),
            ("PGPASSWORD=hunter2 psql -h db", "hunter2", "password"),
            ("export MY_SECRET=s3cr3tvalue", "s3cr3tvalue", "secret"),
            ("CHLOEDB_TOKEN=root-token-1234", "root-token", "token"),
            ("{\"password\": \"correct-horse-battery\"}", "correct-horse", "password"),
            ("{\"user\":\"bob\",\"client_secret\":\"abcd1234efgh\"}", "abcd1234efgh", "secret"),
            ("password = hunter2hunter2", "hunter2hunter2", "password"),
            ("password: \"correct-horse\"", "correct-horse", "password"),
            ("POSTGRES_PASSWORD: example", "example", "password"),
            ("db.password=Tr0ub4dor&3", "Tr0ub4dor", "password"),
            ("const apiKey = \"a1b2c3d4e5f6\";", "a1b2c3d4e5f6", "key"),
            ("X-Api-Key: 9f8e7d6c5b4a", "9f8e7d6c5b4a", "key"),
            ("STRIPE_KEY='abc123def456'", "abc123def456", "key"),
            ("GOOGLE_CREDENTIALS=c2VydmljZS1hY2NvdW50", "c2VydmljZS1hY2NvdW50", "credential"),
            ("DB_PASSWORD=\"correct horse battery\"", "correct horse", "password"),
            ("access_token=ab12cd34ef56&expires_in=3600", "ab12cd34ef56", "token"),
            ("\"refreshToken\": \"1//0abc-def\"", "1//0abc-def", "token"),
            ("echo \\\"PASSWORD=\\\"hunter2\\\"\\\"", "hunter2", "password"),
            // A number or a qualifier after the word (CTX-35, LENA-10).
            ("STAGING_PASS2=plainCanary-55q", "plainCanary", "password"),
            ("DB_PASS=hunter2hunter2", "hunter2hunter2", "password"),
            ("GITHUB_TOKEN_RO=ghx-abcdef123456", "ghx-abcdef123456", "token"),
            ("API_KEY_2=9f8e7d6c5b4a", "9f8e7d6c5b4a", "key"),
            ("DB_PASSWORD_STAGING=correct-horse-battery", "correct-horse", "password"),
            ("client_secret_v2: abcd1234efgh", "abcd1234efgh", "secret"),
        ];
        for (text, secret, kind) in cases {
            caught(text, secret, kind);
        }
        let r = redact("DB_PASSWORD=correct-horse-battery\nDB_HOST=db.internal\nPWD=/home/me");
        assert_eq!(r, "DB_PASSWORD=[REDACTED:password]\nDB_HOST=db.internal\nPWD=/home/me");
        assert_eq!(redact("{\"password\": \"abcdefgh\", \"user\": \"bob\"}"), "{\"password\": \"[REDACTED:password]\", \"user\": \"bob\"}");
    }

    #[test]
    fn code_hashes_and_prose_are_kept() {
        for t in [
            "we use a token bucket for rate limits",
            "the basic idea is simple, and the bearer of bad news is the compiler",
            "Authorization: required for every request",
            "let token = get_token()?; let password = self.password.clone();",
            "pub token: Option<String>, password: &str, api_key: String,",
            "def login(password: str, token: Token) -> None:",
            "api_key = os.environ[\"OPENAI_API_KEY\"]",
            "secret = settings.SECRET_NAME",
            "TOKEN=$(gh auth token) GITHUB_TOKEN=${GITHUB_TOKEN} DB_PASSWORD=",
            "token: ${{ secrets.GITHUB_TOKEN }}",
            "GOOGLE_APPLICATION_CREDENTIALS=/home/me/.config/gcloud/key.json",
            "max_tokens=4096 token_count: 1234567 USE_TOKEN=1 keyboard: true",
            "\"primary_key\": \"id\", \"sort_key\": \"created_at\", \"key\": \"ArrowUp\"",
            "monkey: banana-split-42, hotkey: ctrl+alt+k",
            "--- PASS: TestRedactNames (0.00s)\nPASS\nok  \texample.com/pkg\t0.012s",
            "\"token\": \"The token to send with each request\"",
            "password: ********",
            "commit 3f786850e387550fdab836ed7e6dc881de23001b (HEAD -> main)",
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "id 550e8400-e29b-41d4-a716-446655440000 and the file a1b2c3d4e5f6a7b8c9d0.json",
            "task-1234567890abcdefghijk and desk-top-organiser-for-the-office",
            "sk_1_secret is a test key; prefix sk_9f8e7d6c5b4a3f2e_",
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGq me@host",
            "The previous [REDACTED] marker stays",
        ] {
            kept(t);
        }
    }

    #[test]
    fn json_values_under_secret_keys() {
        let mut v = serde_json::json!({"env": {"DB_PASSWORD": "hunter2", "HOME": "/home/me"}, "args": ["--token", "plain"], "cmd": "PGPASSWORD=hunter2 psql"});
        assert!(redact_json(&mut v));
        assert_eq!(v["env"]["DB_PASSWORD"], "[REDACTED:password]");
        assert_eq!(v["env"]["HOME"], "/home/me");
        assert_eq!(v["cmd"], "PGPASSWORD=[REDACTED:password] psql");
        assert!(!redact_json(&mut v), "idempotent");
        let mut n = std::collections::BTreeMap::new();
        count_markers(&serde_json::to_string(&v).unwrap(), &mut n);
        assert_eq!(n.get("password"), Some(&2));
    }

    fn upper(n: usize) -> String {
        "Q7Z3K9M2X5B8".chars().cycle().take(n).collect()
    }
    /// Letters only, as a random password often is: no digit to give it away.
    fn letters(n: usize) -> String {
        "RWnNtvAtsevGkbQxLp".chars().cycle().take(n).collect()
    }

    /// `text` caught as `kind` wherever it is typed (LENA-302): alone, in
    /// a sentence, after a `word:` (ANA-301), in YAML, in JSON as text and
    /// as a value.
    fn caught_everywhere(text: &str, secret: &str, kind: &str) {
        let json = serde_json::to_string(text).unwrap();
        for t in [
            text.to_string(),
            format!("Please use {text} for the deploy, then rotate it."),
            format!("here is my config: {text}"),
            format!("q3 run: {text}"),
            format!("steps:\n  - name: deploy\n    run: {text}\n    shell: bash"),
            format!("{{\"value\": {json}, \"note\": \"from the dashboard\"}}"),
            format!("{{\"tool_input\":{{\"command\":{json}}}}}"),
        ] {
            caught(&t, secret, kind);
        }
        let mut v = serde_json::json!({"output": text, "nested": [{"value": text}]});
        assert!(redact_json(&mut v), "{kind}: redact_json kept {text:?}");
        let out = v.to_string();
        assert!(!out.contains(secret) && out.contains(&marker(kind)), "{kind}: {out}");
    }

    #[test]
    fn every_shape_lena_and_ana_found_stored_as_typed() {
        // LENA-302: tokens with a fixed, documented prefix.
        let tokens = [
            ("pypi-token", format!("pypi-AgEIcHlwaS5vcmc{}", alnum(60))),
            ("vault-token", format!("hvs.{}", alnum(24))),
            ("vault-token", format!("hvb.{}", alnum(90))),
            ("age-secret-key", format!("AGE-SECRET-KEY-1{}", "QPZRY9X8GF2TVDW0S3JN54KHCE6MUA7L".chars().cycle().take(58).collect::<String>())),
            ("shopify-token", format!("shpat_{}", hex(32))),
            ("shopify-token", format!("shpss_{}", hex(32))),
            ("square-token", format!("sq0atp-{}", alnum(22))),
            ("square-token", format!("sq0csp-{}", alnum(43))),
            ("linear-key", format!("lin_api_{}", alnum(40))),
            ("postman-key", format!("PMAK-{}-{}", hex(24), hex(34))),
            ("newrelic-key", format!("NRAK-{}", upper(27))),
            ("grafana-token", format!("glsa_{}_{}", alnum(32), hex(8))),
            ("grafana-token", format!("glc_{}==", alnum(60))),
            ("sentry-token", format!("sntrys_{}", alnum(80))),
            ("sentry-token", format!("sntryu_{}", hex(64))),
            ("mailgun-key", format!("key-{}", hex(32))),
            ("doppler-token", format!("dp.st.prd.{}", alnum(43))),
            ("databricks-token", format!("dapi{}", hex(32))),
            ("supabase-token", format!("sbp_{}", hex(40))),
            ("tailscale-key", format!("tskey-auth-{}-{}", alnum(12), alnum(30))),
            ("atlassian-token", format!("ATATT3x{}", alnum(180))),
            ("pulumi-token", format!("pul-{}", hex(40))),
            ("terraform-token", format!("{}.atlasv1.{}", alnum(14), alnum(67))),
        ];
        for (kind, t) in &tokens {
            caught_everywhere(t, t, kind);
        }

        // A command line's password (LENA-302), random letters and all.
        let (pw, hx) = (letters(14), hex(32));
        for (cmd, secret, kind) in [
            (format!("--password={pw}"), &pw, "password"),
            (format!("mysqldump --user=root --password={pw} ledger > dump.sql"), &pw, "password"),
            (format!("psql --password '{pw}' -h db"), &pw, "password"),
            (format!("vault login --token={pw}"), &pw, "token"),
            (format!("huggingface-cli login --token {pw}"), &pw, "token"),
            (format!("tool --api-key {pw} --verbose"), &pw, "key"),
            (format!("tool --verbose --client-secret {pw}"), &pw, "secret"),
            (format!("mysql -u root -p{pw} ledger"), &pw, "password"),
            (format!("mysql -h db.internal -uapp -p'{pw}' -e status"), &pw, "password"),
            (format!("sshpass -p {pw} ssh deploy@host"), &pw, "password"),
            // A dashboard's or a Dockerfile's `NAME value`.
            (format!("ENV DB_PASSWORD {hx}"), &hx, "password"),
            (format!("TWILIO_AUTH_TOKEN {hx}"), &hx, "token"),
            (format!("DD_API_KEY {hx}"), &hx, "key"),
        ] {
            caught_everywhere(&cmd, secret, kind);
        }

        // ANA-301: `NAME=secret` after a `word:`; each was stored in clear.
        for (text, secret) in [
            ("q1 Note: DB_PASSWORD=hunter2", "hunter2"),
            ("q2 environment: DB_PASSWORD=hunter2", "hunter2"),
            ("q3 run: PGPASSWORD=hunter2 psql -h db", "hunter2"),
            ("r3b-11 here is my config: DB_PASSWORD=Zq8mXv2LpR4t", "Zq8mXv2LpR4t"),
            ("deploy env: STAGING_PASS2=Yk3nWq8z DB_PASSWORD=Zq8mXv2LpR4t", "Yk3nWq8z"),
            ("Staging creds for the probe: DB_PASSWORD=hunter2-probe, url ok", "hunter2-probe"),
            ("command: DB_PASSWORD=hunter2 ./migrate", "hunter2"),
            ("kubectl create secret generic db --from-literal=password=S3cr3t-pw!", "S3cr3t-pw"),
        ] {
            caught(text, secret, "password");
            caught_everywhere(text.split_once(": ").map_or(text, |(_, t)| t), secret, "password");
        }
        let r = caught("deploy env: STAGING_PASS2=Yk3nWq8z DB_PASSWORD=Zq8mXv2LpR4t", "Zq8mXv2LpR4t", "password");
        assert_eq!(r, "deploy env: STAGING_PASS2=[REDACTED:password] DB_PASSWORD=[REDACTED:password]");
        for t in ["r3b-12 prod: API_KEY=Hb5tLq9wXz2mNp7r", "prod:API_KEY=Hb5tLq9wXz2mNp7r"] {
            caught(t, "Hb5tLq9wXz2mNp7r", "key");
        }
        caught(r#"{"cmd": "PGPASSWORD=hunter2 psql -h db"}"#, "hunter2", "password");

        // A Docker config.json's `"auth"` and an `.npmrc`'s `_auth`: a user and password in base64.
        let auth = format!("{}=", alnum(39));
        let r = caught(&format!("{{\"auths\": {{\"ghcr.io\": {{\"auth\": \"{auth}\"}}}}}}"), &auth, "auth");
        assert_eq!(r, "{\"auths\": {\"ghcr.io\": {\"auth\": \"[REDACTED:auth]\"}}}");
        caught(&format!("cat ~/.docker/config.json\n{{\n  \"auths\": {{\n    \"https://index.docker.io/v1/\": {{\n      \"auth\": \"{auth}\"\n    }}\n  }}\n}}"), &auth, "auth");
        caught(&format!("//registry.example.com/:_auth={auth}"), &auth, "auth");
        let mut v = serde_json::json!({"auths": {"ghcr.io": {"auth": auth}}});
        assert!(redact_json(&mut v));
        assert_eq!(v["auths"]["ghcr.io"]["auth"], "[REDACTED:auth]");
    }

    #[test]
    fn what_only_looks_like_the_new_shapes_is_kept() {
        for t in [
            // Flags that name a secret's place, not the secret.
            "chloectx sync setup --url https://x --token-stdin --token-env CHLOEDB_KEY",
            "docker login --password-stdin < pw.txt && gh auth login --with-token < token.txt",
            "tool --token=$TOKEN --password=\"$DB_PASSWORD\" --api-key=${API_KEY} --secret=<your-secret>",
            "mysql --password mydb; mysql -u root -p ledger; mysql -p -e 'select 1'; mysql -P 3306",
            "cargo test --release --max-tokens 4096 --no-password",
            "mkdir -p build && ssh -p 2222 host && docker run -p 8080:80 img && cp -pr a b",
            // Words and ids.
            "sq0idp-abcdefghijklmnopqrstuv is the application id, key-value store, hvs. prefix",
            "auth: required, \"auth\": \"oauth2\", auth = github",
            "the GITHUB_TOKEN permissions read-all and DB_PASSWORD changed yesterday",
            "\"private_key_id\": \"0123456789abcdef0123456789abcdef01234567\"",
            "pypi-AgE is the prefix and NRAK- the New Relic one",
        ] {
            kept(t);
        }
    }

    #[test]
    fn names_split_into_words() {
        assert_eq!(words("AWS_SECRET_ACCESS_KEY"), ["aws", "secret", "access", "key"]);
        assert_eq!(words("secretAccessKey"), ["secret", "access", "key"]);
        assert_eq!(words("APIKey"), ["api", "key"]);
        assert_eq!(words("x-api-key"), ["x", "api", "key"]);
        assert_eq!(secret_name("primary_key"), None);
        assert_eq!(secret_name("monkey"), None);
        assert_eq!(secret_name("token_url"), None);
        assert_eq!(secret_name("password_hash"), None);
        assert_eq!(secret_name("PGPASSWORD"), Some("password"));
        assert_eq!(secret_name("_authToken"), Some("token"));
        assert_eq!(secret_name("STAGING_PASS2"), Some("password"));
        assert_eq!(secret_name("API_TOKEN_2"), Some("token"));
        assert_eq!(secret_name("SECRET_V2"), Some("secret"));
        // What only ends like one, or names something else, still is not.
        for name in ["PASS", "PASS_2", "BYPASS", "COMPASS", "PASS_THROUGH", "token_url", "PASSWORD_MIN_LENGTH", "max_tokens_2", "primary_key_2", "TOKEN_COUNT", "key2", "v2", "PWD", "PWD_OLD"] {
            assert_eq!(secret_name(name), None, "{name}");
        }
    }
}
