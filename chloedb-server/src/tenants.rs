//! Tenants (`--tenants URL`): databases kept whole in an object store
//! ([`chloedb::tenant`]), which this server opens when a request names
//! one. **Experimental**: a preview of the design in
//! `docs/design/0.7.0-file-per-agent.md`, whose layout may change without
//! a migration until that design is accepted.
//!
//! The server holds no tenant durably. Its copy of one, under
//! `--tenant-cache`, is a cache: any server pointed at the same store
//! answers for the same tenants, and of two that write one tenant at
//! once, the store refuses the later (HTTP 409, to be tried again).
//!
//! | route | does |
//! |---|---|
//! | `POST /v1/admin/tenants` | creates one: `{"id": "agent-7", "sql": ["CREATE TABLE ..."]}` |
//! | `GET /v1/admin/tenants/:id` | where it is: its epoch, base, logs and last transaction |
//! | `POST /v1/admin/tenants/:id/sql` | one statement: `{"sql": "...", "params": {}, "latest": false}` |
//! | `POST /v1/admin/tenants/:id/base` | makes a new base now (before leaving a tenant alone) |
//! | `DELETE /v1/admin/tenants/:id` | deletes it |
//!
//! All need an admin key: the routes are under `/v1/admin/` until
//! tenants have keys of their own, as the design proposes. A statement
//! that reads runs on the server's copy, which holds what this server
//! committed and what others had when it last looked; `"latest": true`
//! reads `HEAD` first (one round trip more) and sees every write
//! committed before the request. A tenant opened here stays open until
//! the server stops.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chloedb::db::SqlResult;
use chloedb::query::Params;
use chloedb::storage::ObjectStore;
use chloedb::tenant::{Tenant, TenantError, TenantOptions};
use serde_json::{json, Value as Json};

use crate::ApiError;

/// The tenants of one store, and those open here.
pub struct Tenants {
    store: Arc<dyn ObjectStore>,
    cache: PathBuf,
    /// A slot per tenant asked for; `None` until it is open. The slot's
    /// lock is held while it opens, so that one tenant is opened once
    /// (two handles must not share a cache directory).
    open: Mutex<HashMap<String, Arc<Mutex<Option<Arc<Tenant>>>>>>,
}

impl std::fmt::Debug for Tenants {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tenants").field("store", &self.store.describe()).field("cache", &self.cache).finish()
    }
}

impl From<TenantError> for ApiError {
    fn from(e: TenantError) -> ApiError {
        let status = match e {
            TenantError::Db(e) => return e.into(),
            TenantError::Missing => 404,
            // To be tried again: nothing was written.
            TenantError::Exists | TenantError::NotWriter => 409,
            TenantError::InDoubt(_) => 503,
        };
        ApiError { status, message: e.to_string() }
    }
}

/// Whether `sql` only reads, by its first word.
fn reads(sql: &str) -> bool {
    let first = sql.trim_start().split(|c: char| !c.is_ascii_alphabetic()).next().unwrap_or_default().to_ascii_uppercase();
    matches!(first.as_str(), "SELECT" | "WITH" | "EXPLAIN" | "SHOW" | "DESCRIBE" | "VALUES")
}

impl Tenants {
    /// The tenants of the store `url` names (an `s3://bucket/prefix/` URL
    /// or a directory), cached under `cache`.
    pub fn open(url: &str, cache: PathBuf) -> Result<Tenants, String> {
        let store = chloedb::storage::open_store(url).map_err(|e| format!("--tenants {url}: {e}"))?;
        // A store that does not refuse a put whose condition fails would
        // lose writes: it is probed, not believed.
        chloedb::tenant::check_store(&*store).map_err(|e| format!("--tenants: {e}"))?;
        std::fs::create_dir_all(&cache).map_err(|e| format!("--tenant-cache {}: {e}", cache.display()))?;
        Ok(Tenants { store, cache, open: Mutex::default() })
    }

    fn options(&self, id: &str) -> TenantOptions {
        TenantOptions::new(self.cache.join(id))
    }

    fn slot(&self, id: &str) -> Arc<Mutex<Option<Arc<Tenant>>>> {
        self.open.lock().unwrap_or_else(|e| e.into_inner()).entry(id.to_string()).or_default().clone()
    }

    /// Tenant `id`, opened unless it is open.
    fn tenant(&self, id: &str) -> Result<Arc<Tenant>, TenantError> {
        if !chloedb::tenant::valid_id(id) {
            return Err(TenantError::Missing);
        }
        let slot = self.slot(id);
        let mut held = slot.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = &*held {
            return Ok(t.clone());
        }
        let t = Arc::new(Tenant::open(self.store.clone(), id, self.options(id))?);
        *held = Some(t.clone());
        Ok(t)
    }

    /// Closes this server's copy of `id` when `e` says the tenant is gone.
    fn forget_if_missing<T>(&self, id: &str, r: Result<T, TenantError>) -> Result<T, TenantError> {
        if matches!(r, Err(TenantError::Missing)) {
            self.open.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
        }
        r
    }

    /// Creates tenant `id` and runs `statements` on it before it is
    /// first committed.
    pub fn create(&self, id: &str, statements: &[String]) -> Result<Json, TenantError> {
        let slot = self.slot(id);
        let mut held = slot.lock().unwrap_or_else(|e| e.into_inner());
        if held.is_some() {
            return Err(TenantError::Exists);
        }
        let t = Tenant::create(self.store.clone(), id, self.options(id), |db| {
            for s in statements {
                db.sql(s, &Params::new())?;
            }
            Ok(())
        })?;
        let described = describe(&t)?;
        *held = Some(Arc::new(t));
        Ok(described)
    }

    /// Where tenant `id` is.
    pub fn describe(&self, id: &str) -> Result<Json, TenantError> {
        let r = self.tenant(id).and_then(|t| {
            t.refresh()?;
            describe(&t)
        });
        self.forget_if_missing(id, r)
    }

    /// Runs one statement on tenant `id`: on this server's copy when it
    /// reads (after a look at the store when `latest`), committed to the
    /// store when it writes.
    pub fn sql(&self, id: &str, sql: &str, params: &Params, latest: bool) -> Result<Json, TenantError> {
        let t0 = Instant::now();
        let r = self.tenant(id).and_then(|t| {
            let run = |db: &chloedb::db::Db| db.sql(sql, params);
            let r = match (reads(sql), latest) {
                (true, true) => t.read_latest(run)?,
                (true, false) => t.read(run)?,
                (false, _) => t.write(run)?,
            };
            let took_ms = t0.elapsed().as_secs_f64() * 1e3;
            Ok(match r {
                SqlResult::Rows(rs) => crate::json::result(&rs, took_ms),
                SqlResult::Written(n) => json!({"written": n, "seq": t.head()?.last_seq, "took_ms": took_ms}),
            })
        });
        self.forget_if_missing(id, r)
    }

    /// Makes a new base of tenant `id` now.
    pub fn make_base(&self, id: &str) -> Result<Json, TenantError> {
        let r = self.tenant(id).and_then(|t| {
            t.make_base()?;
            describe(&t)
        });
        self.forget_if_missing(id, r)
    }

    /// Deletes tenant `id`; whether there was one.
    pub fn delete(&self, id: &str) -> Result<bool, TenantError> {
        if !chloedb::tenant::valid_id(id) {
            return Ok(false);
        }
        let deleted = Tenant::delete(&*self.store, id)?;
        self.open.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
        let _ = std::fs::remove_dir_all(self.cache.join(id));
        Ok(deleted)
    }
}

fn describe(t: &Tenant) -> Result<Json, TenantError> {
    let head = t.head()?;
    Ok(json!({
        "id": head.tenant,
        "experimental": true,
        "layout": head.layout,
        "incarnation": head.incarnation,
        "epoch": head.epoch,
        "writer_here": t.is_writer()?,
        "commit": head.commit,
        "last_seq": head.last_seq,
        "base": {"key": head.base.key, "seq": head.base.seq},
        "logs": head.logs.iter().map(|l| json!({"key": l.key, "first": l.first, "last": l.last, "bytes": l.bytes})).collect::<Vec<_>>(),
        "tail": {"transactions": head.tail.len(), "bytes": head.tail.iter().map(|e| e.bytes.len()).sum::<usize>()},
        "kept": head.kept.len(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statements_that_read() {
        assert!(reads("select 1") && reads("  WITH a AS (SELECT 1) SELECT * FROM a") && reads("EXPLAIN SELECT 1") && reads("SELECT(1)"));
        assert!(!reads("INSERT INTO t VALUES (1)") && !reads("create table t (id INT)") && !reads("") && !reads("DELETE FROM t"));
    }
}
