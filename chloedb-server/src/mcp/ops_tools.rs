//! The operations tools ([`crate::ops`]): what an operator reads of the
//! server, plans and dry runs, the bounded actions an `ops` key takes at
//! once with a reason, and change requests for everything bigger, which a
//! person approves ([`crate::changes`]). `tools/list` offers them to a
//! key that may see the server's views (admin, `ops`, `propose`).
//!
//! The reads are the on-call agent's own ([`crate::oncall`]), by the same
//! routes, so the two never differ.

use axum::http::Method;
use serde_json::{json, Value as Json};

use super::{route, s, Act, Failed, Shape, Tool};

/// A read the on-call agent makes too: its route.
fn read(name: &str, a: &Json) -> Result<Act, Failed> {
    match crate::oncall::read_path(name, a).map_err(Failed::Arguments)? {
        Some(path) => Ok(route(Method::GET, path, None, Shape::All)),
        None => Err(Failed::Arguments(format!("no read {name:?}"))),
    }
}

/// An action of tier 1, as the on-call agent would propose it, made at
/// once with the reason given.
fn act(propose: &str, a: &Json) -> Result<Act, Failed> {
    let reason = reason(a)?;
    let mut input = a.clone();
    input["why"] = json!(reason);
    let p = crate::oncall::proposal(propose, &input, "", "").map_err(Failed::Arguments)?.ok_or_else(|| Failed::Arguments(format!("no action {propose:?}")))?;
    let method = Method::from_bytes(p.method.as_bytes()).map_err(|e| Failed::Arguments(e.to_string()))?;
    Ok(Act::Action { method, path: p.path, body: p.body, reason })
}

fn reason(a: &Json) -> Result<String, Failed> {
    s(a, "reason").map(|r| r.trim().chars().take(500).collect()).map_err(|_| Failed::Arguments("say why: `reason` is required (a sentence; the audit log keeps it)".into()))
}

fn reason_schema() -> Json {
    json!({"type": "string", "description": "Why, in a sentence: the evidence for it. The audit log keeps it."})
}

/// `{method, path, body?}` of a request an agent asks about or proposes.
fn request(a: &Json) -> Result<Json, Failed> {
    let method = s(a, "method")?.to_ascii_uppercase();
    if !["POST", "PUT", "PATCH", "DELETE"].contains(&method.as_str()) {
        return Err(Failed::Arguments("method is POST, PUT, PATCH or DELETE: a change".into()));
    }
    let path = s(a, "path")?;
    if !path.starts_with("/v1/") {
        return Err(Failed::Arguments("path is one of this server's routes, from /v1/ (the catalog tool lists them)".into()));
    }
    let mut r = json!({"method": method, "path": path});
    if let Some(b) = a.get("body").filter(|b| !b.is_null()) {
        r["body"] = b.clone();
    }
    Ok(r)
}

fn request_schema(more: Json) -> Json {
    let mut props = json!({
        "method": {"type": "string", "enum": ["POST", "PUT", "PATCH", "DELETE"]},
        "path": {"type": "string", "description": "The route, as /v1/admin/plan/apply or /v1/sql (catalog lists them), with its query string if any."},
        "body": {"description": "The request's JSON body, if it has one: for /v1/sql, {\"sql\": \"DROP TABLE t\"}."}
    });
    if let (Some(p), Some(m)) = (props.as_object_mut(), more.as_object()) {
        p.extend(m.clone());
    }
    json!({"type": "object", "properties": props, "required": ["method", "path"]})
}

pub(super) const OPS_TOOLS: &[Tool] = &[
    Tool {
        name: "health",
        title: "Health",
        description: "Whether the server (or the cluster) is all right. With fleet: true, the cluster's headline, its checks, and its problems worst first, each with where, since when, its impact and what to look at next (next.api is the route that shows it).",
        schema: || json!({"type": "object", "properties": {"fleet": {"type": "boolean", "default": true}}}),
        read_only: true,
        destructive: false,
        act: |a| {
            let mut a = a.clone();
            if a.get("fleet").is_none() {
                a["fleet"] = json!(true);
            }
            read("health", &a)
        },
    },
    Tool {
        name: "cluster",
        title: "Cluster",
        description: "The cluster's shards and the copies of each: primary or replica, up or down, how far behind, slots.",
        schema: || json!({"type": "object", "properties": {}}),
        read_only: true,
        destructive: false,
        act: |a| read("cluster", a),
    },
    Tool {
        name: "load",
        title: "Load",
        description: "Each copy's load now: queries a second, latency, queue, CPU.",
        schema: || json!({"type": "object", "properties": {}}),
        read_only: true,
        destructive: false,
        act: |a| read("load", a),
    },
    Tool {
        name: "system",
        title: "Host",
        description: "The host: CPU, memory, disk, open files, threads, and the server's limits.",
        schema: || json!({"type": "object", "properties": {}}),
        read_only: true,
        destructive: false,
        act: |a| read("system", a),
    },
    Tool {
        name: "alerts",
        title: "Alerts",
        description: "The alert rules and their states (firing, pending, ok, silenced); with rule, that rule's state and its events over a week.",
        schema: || json!({"type": "object", "properties": {"rule": {"type": "string"}}}),
        read_only: true,
        destructive: false,
        act: |a| read("alerts", a),
    },
    Tool {
        name: "events",
        title: "Events",
        description: "The timeline over the last `minutes` (60 by default), newest first: copies down and back, new primaries, changes, rollouts, contain rules, notes. kind narrows it (a comma-separated list, as copy_down,primary_changed).",
        schema: || json!({"type": "object", "properties": {"minutes": {"type": "integer"}, "kind": {"type": "string"}}}),
        read_only: true,
        destructive: false,
        act: |a| read("events", a),
    },
    Tool {
        name: "queries",
        title: "Queries",
        description: "Queries: `running` now (with their ids, to cancel), the `slow` ones, the `recent` ones, or `stats` by statement fingerprint (calls, latency, errors, who sent them).",
        schema: || json!({"type": "object", "properties": {"which": {"type": "string", "enum": ["running", "slow", "recent", "stats"]}, "limit": {"type": "integer"}}, "required": ["which"]}),
        read_only: true,
        destructive: false,
        act: |a| read("queries", a),
    },
    Tool {
        name: "logs",
        title: "Logs",
        description: "This server's recent log lines, newest last: of a level (error, warn, info) and up, holding words.",
        schema: || json!({"type": "object", "properties": {"level": {"type": "string", "enum": ["error", "warn", "info", "debug"]}, "words": {"type": "string"}, "limit": {"type": "integer"}}}),
        read_only: true,
        destructive: false,
        act: |a| read("logs", a),
    },
    Tool {
        name: "timeseries",
        title: "Metrics",
        description: "The server's metrics, a point every few seconds, over the last `minutes` (15 by default): query rates, latency, errors, memory, disk.",
        schema: || json!({"type": "object", "properties": {"minutes": {"type": "integer"}}}),
        read_only: true,
        destructive: false,
        act: |a| read("timeseries", a),
    },
    Tool {
        name: "contain_rules",
        title: "Contain rules",
        description: "The contain rules in force: what each matches, limits or refuses, until when, who made it and why.",
        schema: || json!({"type": "object", "properties": {}}),
        read_only: true,
        destructive: false,
        act: |a| read("contain_rules", a),
    },
    Tool {
        name: "audit",
        title: "Audit log",
        description: "The changes, notes and refused requests, newest first: who (a person, or an agent and the person it acts for), the route, the reason given, the object and its before and after.",
        schema: || json!({"type": "object", "properties": {
            "kind": {"type": "string", "enum": ["change", "note", "refusal"]},
            "who": {"type": "string"}, "q": {"type": "string", "description": "Words of the entry."},
            "limit": {"type": "integer", "default": 50}
        }}),
        read_only: true,
        destructive: false,
        act: |a| {
            let mut q = vec![format!("limit={}", a.get("limit").and_then(|v| v.as_u64()).unwrap_or(50).clamp(1, 500))];
            for k in ["kind", "who", "q"] {
                if let Some(v) = a.get(k).and_then(|v| v.as_str()).filter(|v| !v.is_empty()) {
                    q.push(format!("{k}={}", super::encode(v)));
                }
            }
            Ok(route(Method::GET, format!("/v1/admin/audit?{}", q.join("&")), None, Shape::All))
        },
    },
    Tool {
        name: "catalog",
        title: "Catalog of routes",
        description: "Every route of the server: method, path, the scope it needs, its tier (0 reads, 1 bounded actions, 2 changes, 3 changes that lose data or cost money), whether it has a dry run, and the tool that calls it; and every error code with what to do about it.",
        schema: || json!({"type": "object", "properties": {"q": {"type": "string", "description": "Words of the path, as kube or contain."}}}),
        read_only: true,
        destructive: false,
        act: |a| {
            let q = a.get("q").and_then(|v| v.as_str()).map(|q| format!("?q={}", super::encode(q))).unwrap_or_default();
            Ok(route(Method::GET, format!("/v1/catalog{q}"), None, Shape::All))
        },
    },
    Tool {
        name: "jobs",
        title: "Jobs",
        description: "The long work under way or done lately, each in one shape (kind, id, state queued, running, held, succeeded, failed or stopped, progress, started and ended, and the route to follow it): a plan applied, slot moves, a rollout, the on-call agent's runs, change requests.",
        schema: || json!({"type": "object", "properties": {"kind": {"type": "string"}}}),
        read_only: true,
        destructive: false,
        act: |a| {
            let q = a.get("kind").and_then(|v| v.as_str()).map(|k| format!("?kind={}", super::encode(k))).unwrap_or_default();
            Ok(route(Method::GET, format!("/v1/admin/jobs{q}"), None, Shape::All))
        },
    },
    Tool {
        name: "runbook",
        title: "Runbook",
        description: "What to do for an alert rule, a fleet problem or a symptom: the runbook that matches (by an alert's rule id, a problem's id, or words), with what to read, what to look for, and which change to propose.",
        schema: || json!({"type": "object", "properties": {"for": {"type": "string", "description": "An alert's rule, a fleet problem's id, or words (p99, disk full, copy down)."}}, "required": ["for"]}),
        read_only: true,
        destructive: false,
        act: |a| Ok(Act::Runbook { query: s(a, "for")?.to_string() }),
    },
    Tool {
        name: "plan_reshape",
        title: "Plan a change of the cluster's shape",
        description: "The steps of changing the copies of each shard, adding or removing shards, or moving slots, on own machines: rows to copy, an estimate, what clients feel, what to check first. Changes nothing; propose_change with POST /v1/admin/plan/apply and the same body asks a person to apply it.",
        schema: || json!({"type": "object", "properties": {
            "shards": {"type": "array", "items": {"type": "object"}, "description": "[{\"shard\": 0, \"copies\": [URL, ...]}]"},
            "add_shards": {"type": "integer"}, "remove_shards": {"type": "array", "items": {"type": "integer"}},
            "moves": {"type": "array", "items": {"type": "object"}}
        }}),
        read_only: true,
        destructive: false,
        act: |a| Ok(route(Method::POST, "/v1/admin/plan", Some(a.clone()), Shape::All)),
    },
    Tool {
        name: "plan_kube",
        title: "Plan a change of the Kubernetes cluster",
        description: "What changing the StrataCluster's spec would do: {\"spec\": {...}} (any of its fields), the sizes alone ({\"shards\", \"replicas\", \"coordinators\"}), or {\"rollback\": version}. The operator's steps and what to know first; changes nothing.",
        schema: || json!({"type": "object", "properties": {"spec": {"type": "object"}, "shards": {"type": "integer"}, "replicas": {"type": "integer"}, "coordinators": {"type": "integer"}, "rollback": {"type": "integer"}}}),
        read_only: true,
        destructive: false,
        act: |a| Ok(route(Method::POST, "/v1/admin/kube/plan", Some(a.clone()), Shape::All)),
    },
    Tool {
        name: "plan_upgrade",
        title: "Plan an upgrade",
        description: "What rolling the cluster out to an image would restart and take, and how long a rollback would: to `image`, or to the newest of the tag the upgrader follows. Changes nothing.",
        schema: || json!({"type": "object", "properties": {"image": {"type": "string"}}}),
        read_only: true,
        destructive: false,
        act: |a| {
            let mut b = json!({"plan": true});
            if let Some(i) = a.get("image").and_then(|v| v.as_str()) {
                b["image"] = json!(i);
            }
            Ok(route(Method::POST, "/v1/admin/kube/upgrade", Some(b), Shape::All))
        },
    },
    Tool {
        name: "dry_run",
        title: "Dry run a change",
        description: "What a change would do, without doing it: its tier, what it would change and how much (rows, copies, tables), its blast radius, whether it can be undone, what to check first, and a plan id. Works for any route, and for SQL sent to /v1/sql.",
        schema: || request_schema(json!({})),
        read_only: true,
        destructive: false,
        act: |a| Ok(route(Method::POST, "/v1/admin/changes/preview", Some(request(a)?), Shape::All)),
    },
    Tool {
        name: "propose_change",
        title: "Propose a change",
        description: "Ask a person to make a change you may not make alone (tier 2 and 3: a plan applied, an upgrade, a spec, a DROP TABLE, a key revoked). The server dry runs it and keeps the plan; a person with an admin key approves or rejects it in the command center (On-call, Changes) within an hour, and it is applied only if the cluster still matches the plan. Answers the change request: follow it with get_change.",
        schema: || request_schema(json!({"reason": reason_schema(), "what": {"type": "string", "description": "What it does, in a few words, for the person who approves it."}})),
        read_only: false,
        destructive: false,
        act: |a| {
            let mut r = request(a)?;
            r["reason"] = json!(reason(a)?);
            if let Some(w) = a.get("what").and_then(|v| v.as_str()) {
                r["what"] = json!(w);
            }
            Ok(route(Method::POST, "/v1/admin/changes", Some(r), Shape::All))
        },
    },
    Tool {
        name: "list_changes",
        title: "List change requests",
        description: "The change requests and the on-call agent's proposals, newest first: each pending, applying, applied, failed, rejected, withdrawn or expired, with who asked, why, and who decided.",
        schema: || json!({"type": "object", "properties": {"state": {"type": "string", "enum": ["pending", "applied", "failed", "rejected", "withdrawn", "expired"]}}}),
        read_only: true,
        destructive: false,
        act: |a| {
            let q = a.get("state").and_then(|v| v.as_str()).map(|k| format!("?state={}", super::encode(k))).unwrap_or_default();
            Ok(route(Method::GET, format!("/v1/admin/changes{q}"), None, Shape::All))
        },
    },
    Tool {
        name: "get_change",
        title: "Get a change request",
        description: "One change request in full: the request, its plan, its state, who decided, and what applying it answered.",
        schema: || json!({"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]}),
        read_only: true,
        destructive: false,
        act: |a| Ok(route(Method::GET, format!("/v1/admin/changes/{}", super::encode(s(a, "id")?)), None, Shape::All)),
    },
    Tool {
        name: "contain",
        title: "Contain queries",
        description: "Limit (to `concurrency` at once) or refuse the queries of a statement fingerprint, a key, or both, for `expires_s` seconds (900 by default; an agent's rule lasts an hour at most). It ends by itself. Needs an ops or admin key and a reason.",
        schema: || json!({"type": "object", "properties": {"fingerprint": {"type": "string"}, "key": {"type": "string"}, "action": {"type": "string", "enum": ["limit", "refuse"]}, "concurrency": {"type": "integer"}, "expires_s": {"type": "integer", "maximum": 3600}, "reason": reason_schema()}, "required": ["action", "reason"]}),
        read_only: false,
        destructive: false,
        act: |a| act("propose_contain", a),
    },
    Tool {
        name: "remove_contain",
        title: "Remove a contain rule",
        description: "Remove a contain rule by its id (contain_rules lists them). Needs an ops or admin key and a reason.",
        schema: || json!({"type": "object", "properties": {"id": {"type": "string"}, "reason": reason_schema()}, "required": ["id", "reason"]}),
        read_only: false,
        destructive: false,
        act: |a| act("propose_remove_contain", a),
    },
    Tool {
        name: "cancel_query",
        title: "Cancel a query",
        description: "Cancel a running query by its id (queries with which: running lists them): it stops at its next check and its client gets 499. Needs an ops or admin key and a reason.",
        schema: || json!({"type": "object", "properties": {"id": {"type": "string"}, "reason": reason_schema()}, "required": ["id", "reason"]}),
        read_only: false,
        destructive: true,
        act: |a| act("propose_cancel_query", a),
    },
    Tool {
        name: "silence_alert",
        title: "Silence an alert",
        description: "Silence an alert rule for `minutes` (two hours at most for an agent) while its cause is known and being handled: no notification goes out meanwhile. Needs an ops or admin key and a reason.",
        schema: || json!({"type": "object", "properties": {"rule": {"type": "string"}, "minutes": {"type": "integer", "maximum": 120}, "reason": reason_schema()}, "required": ["rule", "minutes", "reason"]}),
        read_only: false,
        destructive: false,
        act: |a| act("propose_silence_alert", a),
    },
    Tool {
        name: "ack_alert",
        title: "Acknowledge an alert",
        description: "Acknowledge a firing alert: someone is on it, so a critical one is not sent again. Needs an ops or admin key and a reason, kept as the acknowledgement's note.",
        schema: || json!({"type": "object", "properties": {"rule": {"type": "string"}, "reason": reason_schema()}, "required": ["rule", "reason"]}),
        read_only: false,
        destructive: false,
        act: |a| {
            let reason = reason(a)?;
            let rule = super::encode(s(a, "rule")?);
            Ok(Act::Action { method: Method::POST, path: format!("/v1/alerts/{rule}/ack"), body: Some(json!({"note": reason})), reason })
        },
    },
    Tool {
        name: "note",
        title: "Note on the timeline",
        description: "Write a note on the events timeline (what you found, what you did), where people working the incident read it.",
        schema: || json!({"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}),
        read_only: false,
        destructive: false,
        act: |a| {
            let text = s(a, "text")?.to_string();
            Ok(Act::Action { method: Method::POST, path: "/v1/events/notes".into(), body: Some(json!({"text": text})), reason: "a note on the timeline".into() })
        },
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_carry_their_reason_and_refuse_without_one() {
        let t = OPS_TOOLS.iter().find(|t| t.name == "contain").unwrap();
        assert!(matches!((t.act)(&json!({"action": "refuse", "fingerprint": "ab12"})), Err(Failed::Arguments(_))));
        match (t.act)(&json!({"action": "refuse", "fingerprint": "ab12", "reason": "it holds every core"})).ok().unwrap() {
            Act::Action { method, path, reason, body } => {
                assert_eq!((method, path.as_str(), reason.as_str()), (Method::POST, "/v1/admin/contain", "it holds every core"));
                assert_eq!(body.unwrap()["match"]["fingerprint"], "ab12");
            }
            _ => panic!("not an action"),
        }
    }

    #[test]
    fn a_proposal_is_a_change_of_a_route() {
        let t = OPS_TOOLS.iter().find(|t| t.name == "propose_change").unwrap();
        assert!((t.act)(&json!({"method": "GET", "path": "/v1/tables", "reason": "x"})).is_err());
        assert!((t.act)(&json!({"method": "POST", "path": "http://elsewhere/", "reason": "x"})).is_err());
        assert!((t.act)(&json!({"method": "POST", "path": "/v1/sql", "body": {"sql": "DROP TABLE t"}})).is_err());
        assert!((t.act)(&json!({"method": "POST", "path": "/v1/sql", "body": {"sql": "DROP TABLE t"}, "reason": "unused since May"})).is_ok());
    }
}
