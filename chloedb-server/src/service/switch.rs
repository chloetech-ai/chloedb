//! Switching clients over to a cluster that follows this one
//! ([`crate::follow`]): a hold of a cluster's clients' writes
//! (`POST /v1/admin/hold-writes`, [`HOLD`]) and a follower's promotion
//! (`POST /v1/admin/follow/promote`, [`PROMOTED`]).
//!
//! A planned switch-over holds the source's writes, waits until the
//! follower applied each source shard's feed up to its hold (the feed
//! says so, `"hold": true`, and the follower keeps it with what it
//! applied), and promotes the follower: it stops following and takes its
//! own clients' writes, and no write is lost. The promotion keeps the
//! number of each of the follower's shards' transaction that ended
//! following, after which every transaction is its own clients'; from
//! those the old source can follow it back without a copy
//! (`chloedb-server follow --reverse`), since it holds the same rows then.
//! A promotion that is not planned (the source is gone) loses the writes
//! the follower had not applied, as many as its follow process last said
//! (`behind`).

use std::sync::Arc;

use serde_json::{json, Value as Json};

use super::feed::{next_offset, own_marks_of, Marks, FOLLOWS, HOLD, HOLD_WRITES, PROMOTED};
use super::{conflict, Backend, Service, Written};
use crate::ApiError;

/// Each shard's transaction of write `w`, in shard order.
fn seqs_of(w: &Written) -> Vec<u64> {
    match w.seq_nos.is_empty() {
        true => w.seq_no.into_iter().collect(),
        false => w.seq_nos.clone(),
    }
}

/// A source operation that keeps `offset` under mark `name`.
fn mark(name: &str, offset: i64) -> Json {
    json!({"op": "source", "source": {"name": name, "offset": offset}})
}

/// What a promotion kept, of the view of cluster `c`
/// ([`Service::follow_view`]).
fn record(c: &Json) -> Json {
    json!({"promoted": c["name"], "at_ms": c["promoted_ms"], "seqs": c["promoted_seqs"], "applied": c["applied"]})
}

impl Service {
    /// Holds this cluster's clients' writes (`on`), or ends the hold: a
    /// [`HOLD`] mark on every shard, which each writes once the writes
    /// under way committed, and from then on refuses every other write
    /// (503) but a slot move's. Answers each shard's transaction of the
    /// mark: the last with a client's rows, while it holds.
    pub async fn hold_writes(self: &Arc<Self>, on: bool) -> Result<Json, ApiError> {
        if let Backend::Cluster(c) = &self.backend {
            c.need_hold_protocol("holding writes").await?;
        }
        let shards = self.sources_by_shard().await?;
        let at = next_offset(own_marks_of(&shards).filter_map(|m| m.hold).max(), on);
        let w = super::OWN.scope(true, self.write_ops(vec![mark(HOLD, at)], None)).await?;
        Ok(json!({"holding": on, "at_ms": at / 2, "seq_nos": seqs_of(&w)}))
    }

    /// Whether this cluster holds its clients' writes: every shard
    /// (`holding`), and each, since when.
    pub async fn hold_view(&self) -> Result<Json, ApiError> {
        let shards = self.sources_by_shard().await?;
        let each: Vec<Json> = own_marks_of(&shards).enumerate().map(|(i, m)| json!({"shard": i, "holding": m.holds().is_some(), "since_ms": m.holds()})).collect();
        let holding = !each.is_empty() && each.iter().all(|s| s["holding"] == true);
        Ok(json!({"holding": holding, "shards": each}))
    }

    /// Promotes this cluster, which follows cluster `name`: it stops
    /// following and takes its own clients' writes. Unless `unplanned`,
    /// only once `name` held its writes on every shard as far as this
    /// cluster applied ([`HOLD`]), so that no write is lost; `unplanned`
    /// goes on regardless, and answers how many transactions the follow
    /// process last said it had not applied (`lost`).
    ///
    /// In two writes. The first stops following, marks the promotion and
    /// holds this cluster's writes, once the follow process's under way
    /// committed: each shard's transaction of it is the last of the
    /// source's. The second keeps those numbers (`seqs`) and ends the
    /// hold. One cut short is finished by the next call, which answers
    /// what the first kept.
    pub async fn promote_follower(self: &Arc<Self>, name: &str, unplanned: bool) -> Result<Json, ApiError> {
        if !super::follow_name_ok(name) {
            return Err(ApiError::bad("a cluster's name is 1 to 64 letters, digits, '.', '_' or '-'"));
        }
        if let Backend::Cluster(c) = &self.backend {
            c.need_hold_protocol("a promotion").await?;
        }
        let view = self.follow_view().await?;
        let Some(c) = view["clusters"].as_array().into_iter().flatten().find(|c| c["name"] == name).cloned() else {
            return Err(ApiError { status: 404, message: format!("this cluster never followed {name}") });
        };
        let shards = self.sources_by_shard().await?;
        let own: Vec<Marks> = own_marks_of(&shards).collect();
        let hold = own.iter().filter_map(|m| m.hold).max();
        let planned = c["source_holds"] == true;
        let lost = if planned { Some(0) } else { c["status"]["behind"].as_u64() };
        let seqs = match c.get("promoted_seqs") {
            // Done, but the hold may not have ended on every shard.
            Some(s) => {
                if own.iter().all(|m| m.holds().is_none()) {
                    let mut r = record(&c);
                    r["already"] = json!(true);
                    return Ok(r);
                }
                s.as_array().into_iter().flatten().filter_map(Json::as_u64).collect::<Vec<u64>>()
            }
            None => {
                // A promotion cut short after its first write begins
                // again from there, as decided then.
                if c.get("promoted_ms").is_none() {
                    if c["following"] == 0 {
                        return Err(conflict(format!("this cluster does not follow {name} now (it stopped following it): it takes its own writes already")));
                    }
                    if c["copied"] != true {
                        return Err(conflict(format!("this cluster has not finished its copy of {name}: it holds part of its rows, and promoting it would leave it so")));
                    }
                    if !planned && !unplanned {
                        let unheld: Vec<String> = c["applied"].as_array().into_iter().flatten().filter(|a| a["held"] != true).map(|a| a["shard"].to_string()).collect();
                        let behind = lost.map_or("an unknown number of".to_string(), |n| n.to_string());
                        return Err(conflict(format!(
                            "{name} did not hold its writes on every shard as far as this cluster applied (shard {} not): promoting now loses the writes {name} took that this cluster has not applied ({behind} transactions, as its follow process last said). Hold them first ({HOLD_WRITES} on {name}, or chloedb-server switch-over), wait for source_holds in GET /v1/admin/follow, and promote again; if {name} is gone, promote with {{\"unplanned\": true}}",
                            unheld.join(", ")
                        )));
                    }
                }
                let at = c["promoted_ms"].as_i64().unwrap_or(crate::telemetry::now_ms() as i64);
                let stop = next_offset(own.iter().filter_map(|m| m.follows.get(name).copied()).max(), false);
                let ops = vec![mark(&format!("{FOLLOWS}{name}"), stop), mark(HOLD, next_offset(hold, true)), mark(&format!("{PROMOTED}{name}"), at)];
                let w = super::FOLLOW.scope(Some(name.into()), super::OWN.scope(true, self.write_ops(ops, None))).await?;
                seqs_of(&w)
            }
        };
        let held = self.sources_by_shard().await?;
        let end = next_offset(own_marks_of(&held).filter_map(|m| m.hold).max(), false);
        let mut ops: Vec<Json> = seqs.iter().enumerate().map(|(j, s)| mark(&format!("{PROMOTED}{name}/{j}"), *s as i64)).collect();
        ops.push(mark(HOLD, end));
        super::OWN.scope(true, self.write_ops(ops, None)).await?;
        let view = self.follow_view().await?;
        let c = view["clusters"].as_array().into_iter().flatten().find(|c| c["name"] == name).cloned().unwrap_or(c);
        let mut r = record(&c);
        r["planned"] = json!(planned);
        r["lost"] = json!(lost);
        Ok(r)
    }

    /// The clusters this one was promoted from ([`Self::promote_follower`]), the
    /// last first, with what it applied of each (`applied`) and its own
    /// shards' numbers then (`seqs`): what the old source compares with
    /// its own to follow this one back without a copy.
    pub async fn promotions(self: &Arc<Self>) -> Result<Json, ApiError> {
        let view = self.follow_view().await?;
        let mut p: Vec<Json> = view["clusters"].as_array().into_iter().flatten().filter(|c| c.get("promoted_seqs").is_some()).map(record).collect();
        p.sort_by_key(|r| std::cmp::Reverse(r["at_ms"].as_i64()));
        Ok(json!({"promotions": p}))
    }
}
