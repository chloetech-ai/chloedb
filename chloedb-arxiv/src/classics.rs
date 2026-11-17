//! Likely classics: recent papers that look, in their first months, like
//! the papers that went on to be among the most cited of their year and
//! area. `tools/classics.py` trains a model per window on the rows (early
//! citations from arXiv, the authors' earlier papers, how fast the
//! category and the techniques grew, team size, code, kind), checks it on
//! a year it was not trained on, and writes `ax_classics` (on the search
//! server, beside the papers) and the models' card (a note on the graph
//! server). This reads them.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::search::{self, Ctx};
use crate::strata::RowExt;

/// The note that holds the models' card.
pub const CARD: &str = "classics:model";

/// The likely classics posted in the last `days`, in an area or all,
/// each paper's row with its judgement (`classic`), the count in each
/// area, and the models' card.
pub async fn list(c: &Ctx<'_>, area: &str, days: i64, sort: &str, limit: usize) -> Value {
    let days = days.clamp(30, 730);
    let mut w = format!("created >= now() - INTERVAL '{days} days'");
    let mut p = serde_json::Map::new();
    if !area.is_empty() {
        w.push_str(" AND area = :area");
        p.insert("area".into(), json!(area));
    }
    let order = match sort {
        "new" => "created DESC",
        "early" => "early DESC, p DESC",
        _ => "p DESC, record_cites DESC",
    };
    let (picked, areas, card) = tokio::join!(
        c.soft("the likely classics", &format!("SELECT id, p, lift, win, early, record_cites, why FROM ax_classics WHERE {w} ORDER BY {order} LIMIT {}", limit.clamp(1, 100)), Value::Object(p)),
        c.soft("how many in each area", &format!("SELECT area, COUNT(*) AS n FROM ax_classics WHERE created >= now() - INTERVAL '{days} days' GROUP BY area ORDER BY n DESC"), json!({})),
        c.gsoft("the models' card", "SELECT text FROM ax_notes WHERE id = :k", json!({"k": CARD})),
    );
    let ids: Vec<String> = picked.iter().map(|r| r.s("id")).collect();
    let rows = if ids.is_empty() { Vec::new() } else { c.soft("the papers", &format!("SELECT {} FROM ax_papers WHERE id IN (:ids)", search::COLS), json!({"ids": ids})).await };
    let by: HashMap<String, Value> = rows.into_iter().map(|r| (r.s("id"), serde_json::to_value(&r).unwrap_or(Value::Null))).collect();
    let papers: Vec<Value> = picked
        .iter()
        .filter_map(|j| {
            let mut v = by.get(&j.s("id"))?.clone();
            v["classic"] = json!({"p": j.f("p"), "lift": j.f("lift"), "win": j.i("win"), "early": j.i("early"), "record_cites": j.i("record_cites"), "why": j.get("why").cloned().unwrap_or(json!([]))});
            Some(v)
        })
        .collect();
    let card: Value = card.first().and_then(|r| serde_json::from_str(&r.s("text")).ok()).unwrap_or(Value::Null);
    json!({"papers": papers, "areas": areas, "days": days, "area": area, "model": card})
}
