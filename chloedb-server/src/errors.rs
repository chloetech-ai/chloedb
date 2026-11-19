//! Error codes: what an error is, for a program or an agent to act on
//! without reading its sentence.
//!
//! Every error answer carries, beside the sentence it always had
//! (`error`), a stable `error_code` (not `code`, which the released
//! clients read as their own class of error), whether sending the same request again
//! later can succeed (`retryable`), and for most codes a `hint` (what to
//! do next) and the section of the documentation that explains it
//! (`docs`):
//!
//! ```json
//! {"error": "no such table: orders", "error_code": "table_not_found", "retryable": false,
//!  "hint": "list the tables the key may read (GET /v1/tables, or the list_tables tool)",
//!  "docs": "https://chloetech-ai.github.io/chloedb/site/docs/server.html#schemas"}
//! ```
//!
//! An error made with a code ([`ApiError::coded`]) answers that code. The
//! others, made before codes (most of them), are told by their status and
//! their sentence ([`classify`]), so a sentence reworded may move an error
//! to its status's general code (`bad_request`, `conflict`), never to
//! another specific one. [`CODES`] lists every code: a code is never
//! renamed or reused, only added.

use serde_json::{json, Value as Json};

use crate::ApiError;

/// Where the server's reference lives on the website.
const DOCS: &str = "https://chloetech-ai.github.io/chloedb/site/docs/server.html";

/// A write through a coordinator whose shard's primary was given up on,
/// or was replaced while it was being acknowledged: it may or may not be
/// applied, and the answer names the idempotency key (`idempotency_key`)
/// that sends it again safely ([`crate::coord`]). Not retryable: the same
/// request sent again without that key may apply it twice.
pub const WRITE_IN_DOUBT: &str = "write_in_doubt";

/// One code: its name, whether the same request may succeed later, what
/// to do, and the anchor of the server reference that explains it.
pub struct Code {
    pub code: &'static str,
    pub retryable: bool,
    pub hint: &'static str,
    pub docs: &'static str,
}

const fn c(code: &'static str, retryable: bool, hint: &'static str, docs: &'static str) -> Code {
    Code { code, retryable, hint, docs }
}

/// Every code, the general ones (one a status) last.
pub const CODES: &[Code] = &[
    c("unauthenticated", false, "send Authorization: Bearer <key>; a key that expired or was revoked needs a new one", "security"),
    c("scope_refused", false, "the key may not do this: ask for a key with the scope the error names, or file a change request (POST /v1/admin/changes) for a person to approve", "security"),
    c("table_refused", false, "the key is limited to some tables (GET /v1/whoami lists them)", "security"),
    c("reason_required", false, "say why in the x-chloedb-reason header (or reason in the body); the audit log keeps it", "agents-and-administration"),
    c("limit_exceeded", false, "an agent's key may do this for a shorter time or a smaller amount: the error says the most", "agents-and-administration"),
    c("change_request_required", false, "file a change request (POST /v1/admin/changes, or the propose_change tool) for a person to approve", "agents-and-administration"),
    c("approval_refused", false, "a change is approved by a person with an admin key, not by an agent or through MCP", "agents-and-administration"),
    c("change_not_pending", false, "the change request was decided, applied or expired already: GET /v1/admin/changes/<id> says which", "agents-and-administration"),
    c("plan_stale", false, "the cluster changed since the change was planned: file it again to see the new plan", "agents-and-administration"),
    c("table_not_found", false, "list the tables the key may read (GET /v1/tables, or the list_tables tool)", "schemas"),
    c("column_not_found", false, "describe the table (GET /v1/schema?table=..., or the describe_table tool) for its fields", "schemas"),
    c("query_invalid", false, "the statement is refused as the error says (a word that does not parse, or a table, column or value it does not bind): POST /v1/sql/check, or the check_sql tool, checks one without running it", "http-api"),
    c("table_altered", true, "an ALTER TABLE (or a DROP and CREATE) of the table committed while the statement ran, again and again until its deadline, and nothing was written: send the same statement again, or with a larger timeout_ms", "schemas"),
    c("schema_invalid", false, "the schema is refused as the error says (check_sql, or POST /v1/schema/check, checks one without making it)", "schemas"),
    c("value_invalid", false, "a row's value does not fit its field, as the error says (which row, field and value): fix the value; the schema is fine as it is", "schemas"),
    c("idempotency_conflict", false, "this idempotency key was used for another request: send a new key", "writes-from-many-clients"),
    c("contained", true, "a contain rule limits or refuses these queries for a while (GET /v1/admin/contain); wait, or ask an operator", "who-sends-the-queries-and-containing-them"),
    c("overloaded", true, "the server is busy: send it again after Retry-After seconds", "limits"),
    c("deadline_exceeded", true, "the request ran past its deadline: send it with a larger timeout_ms, or make it cheaper", "deadlines-admission-and-metrics"),
    c("cancelled", false, "someone cancelled the query (DELETE /v1/queries/<id>)", "what-the-server-is-doing-now"),
    c("unavailable", true, "a copy, a shard or a hosted model did not answer: send it again after Retry-After seconds", "failover"),
    c(WRITE_IN_DOUBT, false, "the write may or may not be applied: send the same request again with the idempotency_key the answer names, which applies it once, or read it back first", "failover"),
    c("not_found", false, "nothing has that name or id", "http-api"),
    c("conflict", false, "refused in the state the server is in now, as the error says", "http-api"),
    c("too_large", false, "the request is larger than the server takes: send less at once", "limits"),
    c("unsupported_media_type", false, "send the content type the route takes", "http-api"),
    c("unprocessable", false, "the request is well formed but refused, as the error says", "http-api"),
    c("rate_limited", true, "send it again after Retry-After seconds", "limits"),
    c("not_implemented", false, "this server cannot do that", "http-api"),
    c("bad_request", false, "the request is refused as the error says", "http-api"),
    c("internal", false, "the server failed: the error and the server's logs (x-request-id) say why", "logs"),
    c("upstream", true, "another server failed to answer this one", "logs"),
];

/// The code named `code`.
pub fn lookup(code: &str) -> Option<&'static Code> {
    CODES.iter().find(|c| c.code == code)
}

/// The code of an error made without one: by its status and, for the
/// errors agents meet most, its sentence.
pub fn classify(status: u16, message: &str) -> &'static str {
    let m = message;
    let has = |s: &str| m.contains(s);
    match status {
        401 => "unauthenticated",
        403 if has("touches table") || has("on table") || has("not for a key limited to tables") || has("limited to") => "table_refused",
        403 if has("needs a key with the") || has("scope") => "scope_refused",
        403 => "scope_refused",
        429 if has("contain rule") => "contained",
        429 => "overloaded",
        499 => "cancelled",
        504 => "deadline_exceeded",
        503 => "unavailable",
        413 => "too_large",
        415 => "unsupported_media_type",
        422 if has("idempotency") => "idempotency_conflict",
        422 => "unprocessable",
        501 => "not_implemented",
        502 => "upstream",
        // Bound again until its deadline, and raced still (schema_race).
        400 if chloedb::db::altered_while_running(m) => "table_altered",
        _ if has("no such table") || has("unknown table") => "table_not_found",
        _ if has("no such column") || has("unknown field") || has("no such field") => "column_not_found",
        400 if m.starts_with("query error") || has("parse error") => "query_invalid",
        // A row's value that its field refuses, not the schema (persona
        // study PRIYA-203).
        400 if m.starts_with("schema error") && (has("bad value for") || has("of the vector is")) => "value_invalid",
        400 if m.starts_with("schema error") => "schema_invalid",
        400 => "bad_request",
        404 => "not_found",
        409 => "conflict",
        s if s >= 500 => "internal",
        _ => "bad_request",
    }
}

/// The code an error answers: its own, or [`classify`]'s.
pub fn code_of(e: &ApiError) -> &'static str {
    match e.code {
        Some(c) => c,
        None => classify(e.status, &e.message),
    }
}

/// A query that matches more rows than a query returns, or whose LIMIT and
/// OFFSET reach past them (`--max-result-window`): it parses and binds, so
/// its code's hint (`query_invalid`, how to check a statement) misleads;
/// the code stays.
const ROW_CAP: (&[&str], &str, &str) = (
    &["the most a query returns", "a query returns at most"],
    "the query matches more rows than one answer holds (--max-result-window): add a LIMIT, or read every row a page at a time (page_size, then each page's cursor)",
    "reading-past-the-window-scrolls",
);

/// The fields an error's answer carries beside `error`.
pub fn fields(e: &ApiError) -> Json {
    let code = code_of(e);
    match lookup(code) {
        Some(c) if c.code == "query_invalid" && ROW_CAP.0.iter().any(|m| e.message.contains(m)) => json!({"error_code": c.code, "retryable": c.retryable, "hint": ROW_CAP.1, "docs": format!("{DOCS}#{}", ROW_CAP.2)}),
        Some(c) => json!({"error_code": c.code, "retryable": c.retryable, "hint": c.hint, "docs": format!("{DOCS}#{}", c.docs)}),
        None => json!({"error_code": code, "retryable": false}),
    }
}

/// An error's whole answer: `{error, code, retryable, hint, docs}`.
pub fn answer(e: &ApiError) -> Json {
    let mut j = fields(e);
    j["error"] = json!(e.message);
    j
}

/// The whole table, for `GET /v1/catalog` and the documentation.
pub fn table() -> Json {
    Json::Array(CODES.iter().map(|c| json!({"code": c.code, "retryable": c.retryable, "hint": c.hint, "docs": format!("{DOCS}#{}", c.docs)})).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_unique_and_named_in_snake_case() {
        let mut seen = std::collections::HashSet::new();
        for c in CODES {
            assert!(seen.insert(c.code), "{} twice", c.code);
            assert!(c.code.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_'), "{}", c.code);
            assert!(!c.hint.is_empty() && !c.docs.is_empty(), "{}", c.code);
        }
    }

    #[test]
    fn every_classified_code_is_listed() {
        let samples = [
            (401, "missing or wrong token"),
            (403, "POST /v1/sql needs a key with the admin scope; \"bot\" has read"),
            (403, "a write touches table x, which the key \"a\" may not: it is limited to y (read)"),
            (429, "refused by contain rule c1 (refuses statement ab, for 30 s more)"),
            (429, "too many queries running; try again"),
            (499, "cancelled"),
            (504, "timeout: the query ran past its deadline"),
            (503, "shard 1: no copy answered in the last second"),
            (400, "query error: no such table: orders"),
            (400, "query error: no such column: price"),
            (400, "query error: expected FROM"),
            (400, "schema error: a field twice"),
            (400, "query error: table race was altered, or dropped and created again, while the statement ran; run it again"),
            (400, "say which statement"),
            (404, "no such table: orders"),
            (404, "no proposal p9"),
            (409, "a plan is running"),
            (413, "too large"),
            (415, "content type"),
            (422, "this idempotency key was used for another write"),
            (500, "I/O error: disk"),
            (502, "bad gateway"),
            (501, "no sampler"),
        ];
        for (s, m) in samples {
            let code = classify(s, m);
            assert!(lookup(code).is_some(), "{s} {m}: {code} is not listed");
        }
        assert_eq!(classify(400, "query error: no such table: orders"), "table_not_found");
        assert_eq!(classify(404, "no such table: orders"), "table_not_found");
        assert_eq!(classify(400, "query error: no such column: price"), "column_not_found");
        assert_eq!(classify(403, "POST /v1/sql needs a key with the admin scope; \"bot\" has read"), "scope_refused");
        assert_eq!(classify(429, "refused by contain rule c1"), "contained");
        // A row's value is not the schema (PRIYA-203).
        let nan = "schema error: row 1: schema error: bad value for v: element 1 of the vector is NaN, an infinity or null (JSON writes NaN and infinities as null): a vector's elements are finite numbers";
        assert_eq!(classify(400, nan), "value_invalid");
        assert_eq!(classify(400, "schema error: bad value for field e: element 1 of the vector is not a finite 32-bit float (from -3.4e38 to 3.4e38)"), "value_invalid");
        assert_eq!(classify(400, "schema error: a field twice"), "schema_invalid");
    }

    /// UIA-2: a query past the window parses and binds; its hint says to
    /// add a LIMIT or read it in pages, under the code it always had.
    #[test]
    fn a_query_past_the_window_is_told_to_page() {
        let e = ApiError::bad("query error: the query matches more than 10000 rows, the most a query returns; add a LIMIT");
        let j = answer(&e);
        assert_eq!(j["error_code"], "query_invalid");
        assert!(j["hint"].as_str().unwrap().contains("page_size") && !j["hint"].as_str().unwrap().contains("parse"), "{j}");
        assert!(j["docs"].as_str().unwrap().ends_with("#reading-past-the-window-scrolls"), "{j}");
        assert!(answer(&ApiError::bad("query error: expected FROM"))["hint"].as_str().unwrap().contains("parse"));
        // An OFFSET past the window is told the same (DEV-206 of the
        // persona study of 17 November 2026).
        let e = ApiError::bad("query error: LIMIT and OFFSET reach row 10010; a query returns at most 10000 rows (or read every row a page at a time: a scroll, page_size)");
        assert!(answer(&e)["hint"].as_str().unwrap().contains("page_size"), "{}", answer(&e));
    }

    /// DEV-206: the hint of a statement refused names the HTTP route that
    /// checks one first, not only the MCP tools a client over HTTP has not.
    #[test]
    fn a_refused_statement_is_told_the_route_that_checks_it() {
        let e = ApiError::bad("query error: price is a FLOAT: compare it with a number, not the text 'cheap'");
        let j = answer(&e);
        assert_eq!(j["error_code"], "query_invalid");
        let hint = j["hint"].as_str().unwrap();
        assert!(hint.contains("POST /v1/sql/check") && !hint.contains("search_docs"), "{hint}");
    }

    /// A write that an ALTER TABLE raced until its deadline wrote nothing:
    /// the same statement sent again may run, so it is retryable.
    #[test]
    fn a_write_raced_by_alter_table_is_retryable() {
        let e = ApiError::bad("query error: table race was altered, or dropped and created again, while the statement ran; run it again");
        let j = answer(&e);
        assert_eq!(e.status, 400);
        assert_eq!(j["error_code"], "table_altered");
        assert_eq!(j["retryable"], true);
    }

    #[test]
    fn a_coded_error_answers_its_code() {
        let e = ApiError::coded(409, "plan_stale", "the cluster changed");
        let j = answer(&e);
        assert_eq!(j["error_code"], "plan_stale");
        // `code` is the released clients' own class of error: never sent.
        assert!(j.get("code").is_none(), "{j}");
        assert_eq!(j["retryable"], false);
        assert_eq!(j["error"], "the cluster changed");
        assert!(j["docs"].as_str().unwrap().starts_with("https://"));
    }
}
