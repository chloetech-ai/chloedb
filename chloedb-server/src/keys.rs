//! API keys with scopes, and the audit log of what they did.
//!
//! The server's `--token` is an operator's credential: every route and
//! every gRPC call. The servers of a cluster call each other with the
//! cluster's token (`--cluster-token`), which clients never hold: the
//! calls between servers (`Replicate`, `Restore`, `Verify`, `Slots`,
//! `Configure`, the slot moves, `Keys`) take only it, and the `--token`
//! may not make them. A server without a cluster token takes its
//! `--token` for both, as servers did before cluster tokens. An API key
//! has a scope, and clients get keys rather than a token:
//!
//! | scope | may |
//! |---|---|
//! | `read` | query (SELECT, explain, embed, term statistics), read schemas, health, metrics and the series |
//! | `write` | that, and write rows (`/v1/write`, `/v1/import`, `/v1/stream`, `INSERT`, `UPDATE`, `DELETE`, logged queries) |
//! | `admin` | that, and change the server: tables and rank profiles, synonyms, checkpoints, vacuum, promotion, keys, alerts, log levels; see the running queries, logs and the audit log, and cancel queries |
//!
//! No key, admin ones included, may call the internal gRPC methods: only
//! the root credential ([`Principal::root`]) may.
//!
//! A route or method says the least scope it needs; what a request then
//! does is checked again by the service ([`crate::service`]), the same
//! for HTTP and gRPC: a statement by its kind (a query read, `INSERT`,
//! `UPDATE` and `DELETE` write, `CREATE` and `DROP` of tables and rank
//! profiles admin), a logged query write (and admin when it creates its
//! log's table), a write plan write, an import that creates tables admin.
//! The audit log keeps the admin's statements and every refusal, over
//! both.
//!
//! A key is `sk_<id>_<secret>`; the server keeps its id, name, scope and
//! the SHA-256 of its secret (never the secret) in its keys file, and
//! compares in constant time. A server with no token, no cluster token
//! and no key is open: every request is the root's. Its first key, which
//! must be an admin key, closes it. Such a server keeps an admin key that
//! never expires: it refuses to make or revoke a key that would leave it
//! none, since once its last admin key expired no one could manage it.
//!
//! **Keys across a cluster.** The servers of a cluster hold the same keys
//! ([`crate::keysync`]): a coordinator merges its keys with every server
//! of every shard, and a primary with its replicas, every few seconds and
//! as soon as its keys change. A merge keeps every key of either side, by
//! id, and every revocation, which wins: a key revoked anywhere is revoked
//! wherever it is merged ([`KeySet`]). So a key made on any server
//! reaches the others, a revocation spreads, a coordinator that starts
//! again with its keys in memory takes them back from its shards, and a
//! replica promoted to primary holds its primary's keys. A server takes
//! keys from others only when callers must prove themselves to it (it has
//! a token or a cluster token): keys would close an open server to the
//! servers that gave them. For the same reason a server of a cluster with
//! neither refuses to make a key ([`Keys::create`]).

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};
use sha2::{Digest, Sha256};

use crate::auth::{same_secret, Secret};
use crate::telemetry::now_ms;
use crate::ApiError;

/// What a credential may do, each scope all that the one before may.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// The feed alone, for another cluster to follow this one
    /// (`/v1/feed`), and whether the server is up (`/v1/health`,
    /// `/v1/whoami`): every row, as it is written, but no query.
    Feed,
    Read,
    Write,
    Admin,
}

impl Scope {
    pub fn parse(s: &str) -> Option<Scope> {
        match s.to_ascii_lowercase().as_str() {
            "feed" => Some(Scope::Feed),
            "read" => Some(Scope::Read),
            "write" => Some(Scope::Write),
            "admin" => Some(Scope::Admin),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Scope::Feed => "feed",
            Scope::Read => "read",
            Scope::Write => "write",
            Scope::Admin => "admin",
        }
    }
}

/// What a key for operations may do beyond reading tables, the server's
/// own views and changes by tier ([`crate::ops`]): `propose` reads every
/// view an operator has and files change requests for a person to
/// approve; `ops` also does what is bounded and undoes itself (a contain
/// rule, a query cancelled, an alert silenced), with a reason. Neither
/// writes a row: on tables such a key is a read key, and a server from
/// before roles takes it for one (its `scope` is kept as `read`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Propose,
    Ops,
}

impl Role {
    pub fn parse(s: &str) -> Option<Role> {
        match s.to_ascii_lowercase().as_str() {
            "propose" => Some(Role::Propose),
            "ops" => Some(Role::Ops),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Role::Propose => "propose",
            Role::Ops => "ops",
        }
    }
}

/// Who made a request, as its credential says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Principal {
    /// "token" (the root), "open" (no credential needed), or a key's name.
    pub name: String,
    /// The key's id, for a key.
    pub key: Option<String>,
    pub scope: Scope,
    /// The root credential: may call the internal gRPC methods. The
    /// cluster token; the token of a server without one; anyone on an
    /// open server.
    pub root: bool,
    /// A key limited to some tables ([`Grant`]): what it may do to each,
    /// and nothing on the server beyond them. `None`: every table, at
    /// `scope`.
    pub tables: Option<Arc<[Grant]>>,
    /// A key for operations ([`Role`]).
    pub role: Option<Role>,
    /// A key made for an agent (`agent: true`), and the person it acts
    /// for: the audit log names both, and an agent never approves a
    /// change ([`crate::ops`]).
    pub agent: bool,
    pub on_behalf_of: Option<Arc<str>>,
    /// What a key limited to tables may do beyond its tables' routes
    /// ([`Allow`]); `None` for any other.
    pub allow: Option<Arc<Allow>>,
}

/// What a key limited to tables may reach beyond the routes of its tables
/// ([`crate::scoped::route_ok`]), as its maker gave it or a preset
/// ([`preset`]) expanded: groups of the server's routes
/// ([`ROUTE_GROUPS`]), and apps that do not check a caller's tables
/// themselves, which it may read through (`GET`) only if it reads every
/// table. It never widens what the key may do to a table, nor its scope: a
/// route of a group that changes something still needs the scope the
/// route does ([`crate::http::route_scope`]). A server from before it, or
/// from before one of its groups, reads such a key without them: the key
/// may do less there, never more.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Allow {
    /// The preset it was made from, for showing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub apps: Vec<String>,
    /// A key bound to one tenant of shared indexes on the OpenSearch API
    /// ([`Tenant`], KENJI-402): its only allowance, on a key limited to no
    /// table at all. A server from before it drops it and reads the key
    /// as one of no table, which opens nothing ([`Key::principal`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<Tenant>,
}

/// What a key bound to a tenant reaches (KENJI-402): the documents of
/// `indices` whose keyword field `field` is `value`, through the
/// OpenSearch API alone, as document-level security in OpenSearch's
/// security plugin does. Every read of those indexes is filtered by
/// `field = value` (searches, aggregations and `global` among them, counts,
/// gets by id, `_mget`, scrolls, `_msearch`, deletes by query); every
/// document it writes has `field` equal to `value`, and a document whose
/// `field` is another's (or none) it neither replaces, updates nor deletes
/// ([`crate::opensearch`]). It names indexes by name only (no pattern, no
/// alias), adds no field to their mappings, and changes no index, alias or
/// setting. Everywhere else the filter is not enforced, so the key is
/// refused: the HTTP API but `/v1/health` and `/v1/whoami`, gRPC, MCP, the
/// feed and apps. Its scope is `read` or `write`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tenant {
    pub indices: Vec<String>,
    pub field: String,
    pub value: String,
}

impl Tenant {
    /// A binding from the API: `{"index": "tickets", "field": "tenant_id",
    /// "value": "t007"}` (`indices` for several).
    pub fn of(j: &Json) -> Result<Tenant, ApiError> {
        const FIELDS: &[&str] = &["index", "indices", "field", "value"];
        let Json::Object(m) = j else { return Err(ApiError::bad("tenant is {\"index\": \"tickets\", \"field\": \"tenant_id\", \"value\": \"t007\"}")) };
        if let Some(k) = m.keys().find(|k| !FIELDS.contains(&k.as_str())) {
            return Err(ApiError::bad(format!("{k:?} is not a field of a tenant: index (or indices), field and value")));
        }
        let mut indices: Vec<String> = match (m.get("index"), m.get("indices")) {
            (Some(Json::String(s)), None) => vec![s.clone()],
            (None, Some(Json::Array(a))) => a.iter().map(|v| v.as_str().map(String::from).ok_or_else(|| ApiError::bad("indices is a list of index names"))).collect::<Result<_, _>>()?,
            _ => return Err(ApiError::bad("a tenant names its index, as \"index\": \"tickets\" (or \"indices\": [...], not both)")),
        };
        indices.sort();
        indices.dedup();
        if indices.is_empty() || indices.len() > 16 {
            return Err(ApiError::bad("a tenant names 1 to 16 indexes"));
        }
        for i in &indices {
            // OpenSearch's index names: lowercase, no pattern, no alias's
            // `-x`, `_x` or `.x` (the server's own), no separator.
            let ok = !i.is_empty() && i.len() <= 255 && !i.starts_with(['-', '_', '.', '+']) && i.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.' | '+'));
            if !ok {
                return Err(ApiError::bad(format!("{i:?} is not an index's name: lowercase letters, digits, '-', '_', '.' and '+', no pattern")));
            }
        }
        let field = m.get("field").and_then(Json::as_str).map(str::trim).unwrap_or_default().to_string();
        if field.is_empty() || field.len() > 128 || field.starts_with('_') || !field.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')) {
            return Err(ApiError::bad("a tenant's field is the keyword field that holds the tenant, as \"tenant_id\" (letters, digits, '_', '.', '-')"));
        }
        let value = match m.get("value") {
            Some(Json::String(s)) if !s.is_empty() && s.chars().count() <= 256 => s.clone(),
            _ => return Err(ApiError::bad("a tenant's value is the text its field holds for this tenant, 1 to 256 characters")),
        };
        Ok(Tenant { indices, field, value })
    }

    pub fn json(&self) -> Json {
        json!({"indices": self.indices, "field": self.field, "value": self.value})
    }

    /// In words: `tickets where tenant_id = "t007"`.
    pub fn text(&self) -> String {
        format!("{} where {} = {:?}", self.indices.join(", "), self.field, self.value)
    }
}

/// The groups of routes a key limited to tables may be given
/// ([`Allow::routes`]), each a list of `(method, route)` (`GET` takes
/// `HEAD` too):
///
/// - `fleet` reads the fleet's health, events, metrics and charts;
/// - `alerts` is on-call's (MARCO-301): one alert's page with its events,
///   the runbooks compiled in, and acknowledging, silencing and noting an
///   alert, which need the write scope as for any key. Not the rules nor
///   the routes (an admin's), nor a backtest, nor the server's own log.
///
/// `GET /v1/alerts`, the list of what fires, is every limited key's
/// ([`crate::scoped::route_ok`]).
pub const ROUTE_GROUPS: &[(&str, &[(&str, &str)], &str)] = &[
    (
        "fleet",
        &[("GET", "/v1/health/fleet"), ("GET", "/v1/events"), ("GET", "/metrics"), ("GET", "/v1/timeseries"), ("GET", "/v1/coordinators")],
        "the fleet's health, its events (which name every table and who changed what), its metrics and charts",
    ),
    (
        "alerts",
        &[
            ("GET", "/v1/alerts/:rule"),
            ("GET", "/v1/runbooks"),
            ("POST", "/v1/alerts/:rule/ack"),
            ("POST", "/v1/alerts/:rule/silence"),
            ("DELETE", "/v1/alerts/:rule/silence"),
            ("POST", "/v1/alerts/:rule/notes"),
        ],
        "an alert's page and the runbooks, and acknowledging, silencing and noting an alert (with a key that writes); not the rules or the routes",
    ),
];

impl Allow {
    /// Whether it opens `method route`.
    pub fn route(&self, method: &axum::http::Method, route: &str) -> bool {
        let m = if method == axum::http::Method::HEAD { "GET" } else { method.as_str() };
        ROUTE_GROUPS.iter().any(|(g, routes, _)| self.routes.iter().any(|r| r == g) && routes.contains(&(m, route)))
    }

    pub fn json(&self) -> Json {
        let mut j = json!({"preset": self.preset, "routes": self.routes, "apps": self.apps});
        if let Some(t) = &self.tenant {
            j["tenant"] = t.json();
        }
        j
    }

    /// An allowance from the API: `routes` (names of [`ROUTE_GROUPS`]) and
    /// `apps` (apps' names).
    pub fn of(routes: Option<&Json>, apps: Option<&Json>) -> Result<Allow, ApiError> {
        let names = |j: Option<&Json>, what: &str| -> Result<Vec<String>, ApiError> {
            match j {
                None | Some(Json::Null) => Ok(Vec::new()),
                Some(Json::Array(a)) => a.iter().map(|v| v.as_str().map(|s| s.trim().to_ascii_lowercase()).ok_or_else(|| ApiError::bad(format!("{what} is a list of names")))).collect(),
                Some(_) => Err(ApiError::bad(format!("{what} is a list of names"))),
            }
        };
        let routes = names(routes, "routes")?;
        if let Some(r) = routes.iter().find(|r| !ROUTE_GROUPS.iter().any(|(g, _, _)| g == r)) {
            return Err(ApiError::bad(format!("{r:?} is not a group of routes: they are {}", ROUTE_GROUPS.iter().map(|(g, _, w)| format!("{g} ({w})")).collect::<Vec<_>>().join(", "))));
        }
        let apps = names(apps, "apps")?;
        if let Some(a) = apps.iter().find(|a| a.is_empty() || a.len() > 64 || !a.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')) {
            return Err(ApiError::bad(format!("{a:?} is not an app's name")));
        }
        Ok(Allow { preset: None, routes, apps, tenant: None })
    }

    fn is_empty(&self) -> bool {
        self.preset.is_none() && self.routes.is_empty() && self.apps.is_empty() && self.tenant.is_none()
    }
}

/// The presets of keys limited to tables, for the jobs they are for: each
/// is its grants and allowance, as a key could be given them by hand.
pub const PRESETS: &[(&str, &str)] = &[
    ("analyst", "reads every table, and is admin of the prefix's (its own results); makes its own workbooks in Sheets, whose queries read as the key may"),
    ("oncall", "writes logs (OTLP too) into the prefix's tables and notes into ChloeCtx spaces ctx_<prefix>, reads the incidents, sees the fleet's health, events, alerts and metrics, and acknowledges, silences and notes alerts"),
    ("ml-app", "is admin of the prefix's tables: queries logged into them, their rank logs, plans, PACK, and the models of their fields"),
    ("app-owner", "is admin of the prefix's tables: app_* builds, changes and publishes apps in the app creator, app_<id>_* one app"),
];

/// Preset `name` ([`PRESETS`]) over the tables `prefix` names (`team_*`):
/// its grants and allowance.
pub fn preset(name: &str, prefix: &str) -> Result<(Vec<Grant>, Allow), ApiError> {
    let prefix = prefix.trim().to_ascii_lowercase();
    let stem = prefix.strip_suffix('*').filter(|s| !s.is_empty()).ok_or_else(|| ApiError::bad(format!("a preset's prefix names tables by their start and *, as team_*, not {prefix:?}")))?;
    let own = Grant::new(&prefix, Scope::Admin)?;
    let g = |t: &str, s: Scope| Grant::new(t, s);
    let mut allow = Allow { preset: Some(name.to_string()), ..Default::default() };
    let grants = match name {
        "analyst" => {
            allow.apps = vec!["sheets".into()];
            vec![g("*", Scope::Read)?, own]
        }
        "oncall" => {
            allow.routes = vec!["fleet".into(), "alerts".into()];
            let space = format!("ctx_{}*", stem.strip_prefix("ctx_").unwrap_or(stem));
            vec![g(&prefix, Scope::Write)?, g(&space, Scope::Write)?, g("ctx_incidents", Scope::Read)?]
        }
        "ml-app" => vec![own],
        "app-owner" => {
            allow.apps = vec!["apps".into()];
            vec![own]
        }
        _ => return Err(ApiError::bad(format!("{name:?} is not a preset: they are {}", PRESETS.iter().map(|(n, w)| format!("{n} ({w})")).collect::<Vec<_>>().join("; ")))),
    };
    Ok((grants, allow))
}

/// The start of a connector's push key's name ([`Principal::push_only`]).
pub const PUSH_KEY_PREFIX: &str = "connector:";

impl Principal {
    pub fn open() -> Principal {
        Principal::admin("open", true)
    }

    /// An admin of every table, by `name`: no key, no role, not an agent.
    pub fn admin(name: &str, root: bool) -> Principal {
        Principal { name: name.into(), key: None, scope: Scope::Admin, root, tables: None, role: None, agent: false, on_behalf_of: None, allow: None }
    }

    /// The `--token`'s holder: the root on a server without a cluster
    /// token (`root`), else an admin.
    fn token(root: bool) -> Principal {
        Principal::admin("token", root)
    }

    /// Another server of the cluster, by the cluster's token.
    pub fn cluster() -> Principal {
        Principal::admin("cluster", true)
    }

    /// Whether this is anyone, on a server with no credential to check.
    pub fn is_open(&self) -> bool {
        self.key.is_none() && self.name == "open"
    }

    /// Refuses (403) what needs `need` and this credential may not do.
    pub fn may(&self, need: Scope, what: &str) -> Result<(), ApiError> {
        match self.scope >= need {
            true => Ok(()),
            false => Err(ApiError { code: Some("scope_refused"), status: 403, message: format!("{what} needs a key with the {} scope; {:?} has {}", need.name(), self.name, self.scope_name()) }),
        }
    }

    /// Refuses (403) a route a key for operations does not reach (CCW-11):
    /// it names the least key that would, `ops` when that role reaches the
    /// route (`ops_would`), and the change request a person approves, so
    /// that an agent does not ask for an admin key it does not need.
    pub fn may_by_role(&self, need: Scope, what: &str, ops_would: bool) -> Result<(), ApiError> {
        let Some(role) = self.role.filter(|_| self.scope < need) else { return self.may(need, what) };
        let least = if ops_would && role < Role::Ops { "an ops key".to_string() } else { format!("a key with the {} scope", need.name()) };
        Err(ApiError { code: Some("scope_refused"), status: 403, message: format!("{what} needs {least} ({:?} is {}), or a change request (POST /v1/admin/changes) for a person to approve", self.name, role.name()) })
    }

    /// The scope as keys are made with it: `ops` or `propose` for a key
    /// with a role, else its scope.
    pub fn scope_name(&self) -> &'static str {
        self.role.map_or(self.scope.name(), Role::name)
    }

    /// An agent: a key made for one, or a key with a role.
    pub fn is_agent(&self) -> bool {
        self.agent
    }

    pub fn json(&self) -> Json {
        let mut j = json!({"name": self.name, "key": self.key, "scope": self.scope_name(), "root": self.root});
        if let Some(t) = &self.tables {
            j["tables"] = grants_json(t);
            // What its scope means: its tables', not the server's.
            j["limited"] = json!(true);
        }
        if let Some(a) = &self.allow {
            if let Some(p) = &a.preset {
                j["preset"] = json!(p);
            }
            j["allow"] = a.json();
        }
        if let Some(t) = self.tenant() {
            j["tenant"] = t.json();
        }
        if let Some(r) = self.role {
            j["role"] = json!(r.name());
            j["data_scope"] = json!(self.scope.name());
        }
        if self.agent {
            j["agent"] = json!(true);
        }
        if let Some(p) = &self.on_behalf_of {
            j["on_behalf_of"] = json!(&**p);
        }
        j
    }

    /// Whether a key limited to tables was given route `route`
    /// ([`Allow::routes`]).
    pub fn allowed_route(&self, method: &axum::http::Method, route: &str) -> bool {
        self.allow.as_ref().is_some_and(|a| a.route(method, route))
    }

    /// Whether it reads every table: a key of every table of read scope or
    /// more, or one limited to tables with a grant of `*`.
    pub fn reads_all(&self) -> bool {
        match &self.tables {
            None => self.scope >= Scope::Read,
            Some(g) => g.iter().any(|g| g.table == "*" && g.scope >= Scope::Read),
        }
    }

    /// A key limited to some tables ([`Grant`]).
    pub fn limited(&self) -> bool {
        self.tables.is_some()
    }

    /// The tenant a key is bound to on the OpenSearch API ([`Tenant`]);
    /// such a key is limited to no table.
    pub fn tenant(&self) -> Option<&Tenant> {
        self.tables.as_ref()?;
        self.allow.as_deref().and_then(|a| a.tenant.as_ref())
    }

    /// A connector's push key (`connector:<name>`, limited to its target,
    /// made by [`crate::connectors`]): it writes its table and reads
    /// nothing, not even that table ([`crate::scoped::push_route_ok`]), so
    /// that the key a sending system holds, if it leaks, reads no alert or
    /// ticket. Known by its name, which only narrows what a key may do.
    pub fn push_only(&self) -> bool {
        self.tables.is_some() && self.name.starts_with(PUSH_KEY_PREFIX)
    }

    /// What this credential may do to table `table`: its scope, or for a
    /// key limited to tables the most its grants that name the table give
    /// (`None`: nothing).
    pub fn table_scope(&self, table: &str) -> Option<Scope> {
        match &self.tables {
            None => Some(self.scope),
            Some(g) => granted(g, table),
        }
    }

    /// Refuses (403) what needs `need` on table `table` and this credential
    /// may not do there.
    pub fn may_table(&self, need: Scope, table: &str, what: &str) -> Result<(), ApiError> {
        match self.table_scope(table) {
            Some(s) if s >= need => Ok(()),
            Some(s) => Err(ApiError { code: None, status: 403, message: format!("{what} needs the {} scope on table {table}; the key {:?} has {} there", need.name(), self.name, s.name()) }),
            None => Err(ApiError { code: None, status: 403, message: format!("{what} touches table {table}, which the key {:?} may not: it is limited to {}", self.name, self.tables.as_deref().map(grants_text).unwrap_or_default()) }),
        }
    }

    /// Refuses (403) what is the server's rather than a table's, for a key
    /// limited to tables.
    pub fn may_server(&self, what: &str) -> Result<(), ApiError> {
        match &self.tables {
            None => Ok(()),
            Some(g) => Err(ApiError { code: None, status: 403, message: format!("{what} is not for a key limited to tables: the key {:?} may touch only {}", self.name, grants_text(g)) }),
        }
    }
}

/// What a key limited to tables may do to some of them: `table` is a
/// table's name (`chloectx_memory`), a prefix and `*` (`crawl_*`, every
/// table whose name starts with `crawl_`), or `*` (every table), in any
/// case; `scope` is what it may do there. `admin` on tables is `CREATE`,
/// `ALTER` and `DROP` of them and their rank profiles, never the server's
/// own admin (keys, shards, functions, settings).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub table: String,
    pub scope: Scope,
}

impl Grant {
    pub fn new(table: &str, scope: Scope) -> Result<Grant, ApiError> {
        let table = table.trim().to_ascii_lowercase();
        let name = table.strip_suffix('*').unwrap_or(&table);
        if table.len() > 128 || (name.is_empty() && table != "*") || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(ApiError::bad(format!("{table:?} is not a table grant: a table's name, a prefix of names and * (crawl_*), or * for every table")));
        }
        if scope == Scope::Feed {
            return Err(ApiError::bad("a table grant's scope is read, write or admin: the feed is every table's, for another cluster"));
        }
        Ok(Grant { table, scope })
    }

    /// `crawl_*=admin` (a scope of read when none is given).
    pub fn parse(s: &str) -> Result<Grant, ApiError> {
        let (table, scope) = s.split_once('=').unwrap_or((s, "read"));
        let scope = Scope::parse(scope.trim()).ok_or_else(|| ApiError::bad(format!("{s:?}: a table grant is table=scope, the scope read, write or admin")))?;
        Grant::new(table, scope)
    }

    /// Whether it names table `table`.
    pub fn covers(&self, table: &str) -> bool {
        let table = table.to_ascii_lowercase();
        match self.table.strip_suffix('*') {
            Some(prefix) => table.starts_with(prefix),
            None => table == self.table,
        }
    }
}

/// The header with which an app that checks its callers runs a statement
/// for one ([`Principal::narrowed`]): the caller's tables, as
/// `x-chloedb-tables` told the app (`*=read`, `team_*=admin`).
pub const AS_TABLES: &str = "x-chloedb-as-tables";

impl Principal {
    /// This credential, narrowed to `grants` (`table=scope,...`, as
    /// [`AS_TABLES`] gives them), each no wider than its scope: what an
    /// app runs for a caller (a sheet's query) is then checked as the
    /// caller's key would be, by the tables it reads and writes. It only
    /// ever narrows: a key already limited to tables (the key a server
    /// gives each app it starts, [`crate::app_procs::grants`]) keeps, on
    /// each table, the lower of its own scope there and the caller's, and
    /// nothing where either has none ([`intersect`]; DAT-5: refusing such
    /// a key refused every caller of the crawler and Sheets a server
    /// starts); the narrowed credential is never the root.
    pub fn narrowed(&self, grants: &str) -> Result<Principal, ApiError> {
        let mut g = Vec::new();
        for s in grants.split(',').filter(|s| !s.trim().is_empty()) {
            let mut x = Grant::parse(s)?;
            x.scope = x.scope.min(self.scope);
            g.push(x);
        }
        if let Some(own) = &self.tables {
            g = intersect(own, &g);
        }
        let mut p = self.clone();
        p.scope = widest(&g).min(self.scope);
        p.tables = Some(g.into());
        p.allow = None;
        p.root = false;
        Ok(p)
    }
}

/// Tables of the server's own whose rows an admin acts on, so that a
/// grant by a pattern (`*`, `rel*`) reads them but does not write them:
/// only a grant that names one does. Applying a relevance proposal runs
/// the rank profile in its row ([`chloedb::autopilot::check_apply`]).
const NAMED_TO_WRITE: &[&str] = &[chloedb::autopilot::PROPOSALS];

/// The most `grants` give on table `table`.
pub fn granted(grants: &[Grant], table: &str) -> Option<Scope> {
    let own = NAMED_TO_WRITE.iter().any(|t| t.eq_ignore_ascii_case(table));
    grants
        .iter()
        .filter(|g| g.covers(table))
        .map(|g| if own && g.table.ends_with('*') { g.scope.min(Scope::Read) } else { g.scope })
        .max()
}

/// Grants that give on each table the lower of what `a` and `b` give there
/// ([`granted`]), and nothing where either gives nothing: each grant of
/// `a` with each of `b` that names tables in common gives those, at the
/// lower of the two scopes. (The most of such lows on a table is the low
/// of the two mosts.) A table of [`NAMED_TO_WRITE`] that only one side
/// names, the other covering it by a pattern, is read only, as it is on
/// that side.
pub fn intersect(a: &[Grant], b: &[Grant]) -> Vec<Grant> {
    let mut out: Vec<Grant> = Vec::new();
    for x in a {
        for y in b {
            let Some(table) = common(&x.table, &y.table) else { continue };
            let mut scope = x.scope.min(y.scope);
            let by_pattern = x.table.ends_with('*') || y.table.ends_with('*');
            if !table.ends_with('*') && by_pattern && NAMED_TO_WRITE.iter().any(|t| t.eq_ignore_ascii_case(&table)) {
                scope = scope.min(Scope::Read);
            }
            match out.iter_mut().find(|g| g.table == table) {
                Some(g) => g.scope = g.scope.max(scope),
                None => out.push(Grant { table, scope }),
            }
        }
    }
    out
}

/// The tables two grants' names both cover, as one name (a table's, or a
/// prefix and `*`), if any.
fn common(a: &str, b: &str) -> Option<String> {
    match (a.strip_suffix('*'), b.strip_suffix('*')) {
        (None, None) => (a == b).then(|| a.to_string()),
        (Some(p), None) => b.starts_with(p).then(|| b.to_string()),
        (None, Some(q)) => a.starts_with(q).then(|| a.to_string()),
        (Some(p), Some(q)) if p.starts_with(q) => Some(a.to_string()),
        (Some(p), Some(q)) if q.starts_with(p) => Some(b.to_string()),
        _ => None,
    }
}

/// Grants as the API shows them: `{"crawl_*": "admin"}`.
pub fn grants_json(grants: &[Grant]) -> Json {
    Json::Object(grants.iter().map(|g| (g.table.clone(), json!(g.scope.name()))).collect())
}

/// Grants in words: `crawl_* (admin), * (read)`.
pub fn grants_text(grants: &[Grant]) -> String {
    grants.iter().map(|g| format!("{} ({})", g.table, g.scope.name())).collect::<Vec<_>>().join(", ")
}

/// Grants from the API: `{"crawl_*": "admin", "*": "read"}`, or a list of
/// `table=scope`.
pub fn grants_of(j: &Json) -> Result<Vec<Grant>, ApiError> {
    let grants = match j {
        Json::Object(m) => m.iter().map(|(t, s)| Grant::new(t, s.as_str().and_then(Scope::parse).ok_or_else(|| ApiError::bad(format!("the scope of table grant {t} is read, write or admin")))?)).collect::<Result<Vec<_>, _>>()?,
        Json::Array(a) => a.iter().map(|g| g.as_str().ok_or_else(|| ApiError::bad("a table grant is \"table=scope\"")).and_then(Grant::parse)).collect::<Result<Vec<_>, _>>()?,
        Json::String(s) => s.split(',').filter(|g| !g.trim().is_empty()).map(Grant::parse).collect::<Result<Vec<_>, _>>()?,
        _ => return Err(ApiError::bad("tables is {\"table\": \"scope\"}, as {\"crawl_*\": \"admin\", \"*\": \"read\"}")),
    };
    if grants.is_empty() {
        return Err(ApiError::bad("a key limited to tables names one at least"));
    }
    if grants.len() > 64 {
        return Err(ApiError::bad("a key is limited to 64 table grants at most"));
    }
    Ok(grants)
}

/// The most any of `grants` gives: a limited key's scope, which routes are
/// checked against before its tables are.
fn widest(grants: &[Grant]) -> Scope {
    grants.iter().map(|g| g.scope).max().unwrap_or(Scope::Read)
}

/// How the keys file writes the digest of a key limited to tables: after
/// this mark, so that a server from before limits, which would read the
/// key as one of every table (it knows no `tables`), never takes its
/// secret ([`stored_digest`]).
const LIMITED: &str = "t:";

/// The same for a key another key made ([`Key::parent`],
/// [`Keys::create_delegated`]): a server from before such keys, which
/// would not ask whether the key that made it still holds, never takes
/// its secret either.
const DELEGATED: &str = "d:";

/// The same for a person's session ([`Key::session`],
/// [`Keys::create_session`]): a server from before sessions, which would
/// take one for a plain key (no idle limit, no check of its allow list),
/// never takes its secret either.
const SESSION: &str = "s:";

/// The digest of `secret` as a key keeps it ([`LIMITED`], [`DELEGATED`]):
/// for a key limited to tables (`limited`), made by another key
/// (`delegated`, which is limited too).
fn stored_digest(secret: &str, limited: bool, delegated: bool) -> String {
    match (limited, delegated) {
        (_, true) => format!("{DELEGATED}{}", sha256(secret)),
        (true, false) => format!("{LIMITED}{}", sha256(secret)),
        (false, false) => sha256(secret),
    }
}

/// A stored digest without its mark.
fn unmarked(stored: &str) -> &str {
    stored.strip_prefix(LIMITED).or_else(|| stored.strip_prefix(DELEGATED)).or_else(|| stored.strip_prefix(SESSION)).unwrap_or(stored)
}

/// Whether a stored digest is the digest `digest` ([`stored_digest`]).
fn digest_is(stored: &str, digest: &str) -> bool {
    same_secret(digest.as_bytes(), unmarked(stored).as_bytes())
}

/// The most keys one key may make ([`Keys::create_delegated`]).
pub const DELEGATED_MAX: usize = 4096;

/// Whether grant `child` asks for no more than `parent` gives: no more
/// scope, and tables `parent` names (`app_x_*` within `app_*`, `notes`
/// within `no*` or `*`).
pub fn within(child: &Grant, parent: &Grant) -> bool {
    if child.scope > parent.scope {
        return false;
    }
    match (parent.table.strip_suffix('*'), child.table.strip_suffix('*')) {
        (Some(p), Some(c)) => c.starts_with(p),
        (Some(p), None) => child.table.starts_with(p),
        (None, Some(_)) => false,
        (None, None) => child.table == parent.table,
    }
}

/// A key as the keys file keeps it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Key {
    pub id: String,
    pub name: String,
    pub scope: Scope,
    pub created_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_ms: Option<u64>,
    /// The SHA-256 of the secret, in hex.
    pub sha256: String,
    #[serde(default)]
    pub last_used_ms: u64,
    /// Who answers for it (a person or team), and what it is for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The address it was last used from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_addr: Option<String>,
    /// The tables it is limited to ([`Grant`]); none: every table, at
    /// `scope`. Its digest is then kept after a mark ([`LIMITED`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tables: Option<Vec<Grant>>,
    /// What it may reach beyond its tables' routes, for a key limited to
    /// tables ([`Allow`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow: Option<Allow>,
    /// The id of the key that made it ([`Keys::create_delegated`]): it
    /// opens nothing once that key is revoked or has expired, and its
    /// tables are within that key's. Its digest is kept after a mark of
    /// its own ([`DELEGATED`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// A key for operations ([`Role`]); its `scope` is then `read`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    /// Made for an agent, and the person it acts for.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub agent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_behalf_of: Option<String>,
    /// What one statement it sends may cost ([`chloedb::query::budget`]):
    /// a request may ask for less, never more, and the server's
    /// `--statement-budget` holds as well. A key another key made has its
    /// maker's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<chloedb::query::budget::Budget>,
    /// A person's session, by how they signed in (`google`,
    /// [`crate::signin`]): its owner is their address, it ends when it has
    /// not been used for [`SESSION_IDLE_MS`], and each request checks
    /// that the server still lets that address in. Its digest is kept
    /// after a mark of its own ([`SESSION`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

/// How long a person's session lasts unused ([`Key::session`]).
pub const SESSION_IDLE_MS: u64 = 12 * 3600 * 1000;

/// How long a person's session lasts at most, used or not.
pub const SESSION_MAX_MS: u64 = 7 * 24 * 3600 * 1000;

/// The sessions one person keeps at once (a phone, a laptop, ...): a new
/// one past them ends their oldest.
pub const SESSIONS_PER_PERSON: usize = 8;

/// What a key made for an agent or for operations has beyond a plain key
/// ([`Keys::create_full`]).
#[derive(Clone, Debug, Default)]
pub struct AgentKey {
    pub role: Option<Role>,
    pub agent: bool,
    pub on_behalf_of: Option<String>,
    /// What a key limited to tables reaches beyond them ([`Allow`]).
    pub allow: Option<Allow>,
    /// Its statement budget ([`Key::budget`]).
    pub budget: Option<chloedb::query::budget::Budget>,
}

impl Key {
    /// Its scope as the API shows it: `ops` or `propose` for a key with
    /// a role.
    pub fn scope_name(&self) -> &'static str {
        self.role.map_or(self.scope.name(), Role::name)
    }

    /// What the API shows of it.
    pub fn json(&self) -> Json {
        let mut j = json!({"id": self.id, "name": self.name, "scope": self.scope_name(), "created_ms": self.created_ms, "expires_ms": self.expires_ms, "last_used_ms": self.last_used_ms, "prefix": format!("sk_{}_", self.id),
            "owner": self.owner, "note": self.note, "last_addr": self.last_addr});
        if let Some(t) = &self.tables {
            j["tables"] = grants_json(t);
            // Its scope is its tables', not the server's.
            j["limited"] = json!(true);
        }
        if let Some(a) = self.allow.as_ref().filter(|_| self.tables.is_some()) {
            if let Some(p) = &a.preset {
                j["preset"] = json!(p);
            }
            j["allow"] = a.json();
            if let Some(t) = &a.tenant {
                j["tenant"] = t.json();
            }
        }
        if let Some(r) = self.role {
            j["role"] = json!(r.name());
        }
        if self.agent {
            j["agent"] = json!(true);
        }
        if let Some(p) = &self.on_behalf_of {
            j["on_behalf_of"] = json!(p);
        }
        if let Some(p) = &self.parent {
            j["parent"] = json!(p);
        }
        if let Some(b) = self.budget.filter(|b| !b.is_none()) {
            j["budget"] = json!(b);
        }
        if let Some(p) = &self.session {
            j["session"] = json!(p);
            j["idle_ends_ms"] = json!(self.last_used_ms.max(self.created_ms) + SESSION_IDLE_MS);
        }
        j
    }

    /// Who it makes a request, if its digest and its limits agree: a key
    /// whose digest is marked limited but that lost its tables (a server
    /// from before limits wrote it back without them) makes no one, nor
    /// does one marked as another key's that lost its parent, or one with
    /// a parent but not so marked. (Whether the parent still holds is
    /// [`Keys::find`]'s.)
    fn principal(&self) -> Option<Principal> {
        let delegated = self.sha256.starts_with(DELEGATED);
        if delegated != self.parent.is_some() {
            return None;
        }
        // A session is a person's, of every table, made by no key.
        if self.sha256.starts_with(SESSION) != self.session.is_some() || (self.session.is_some() && (self.tables.is_some() || self.parent.is_some() || self.role.is_some())) {
            return None;
        }
        let marked = self.sha256.starts_with(LIMITED) || delegated;
        // A key bound to a tenant (KENJI-402) is limited to no table: a
        // server from before tenants, which drops the binding, reads it as
        // a key of no table and lets it in nowhere.
        let tenant = self.allow.as_ref().is_some_and(|a| a.tenant.is_some());
        let tables: Option<Arc<[Grant]>> = match (&self.tables, marked) {
            (Some(t), true) if t.is_empty() && tenant && !delegated && self.role.is_none() => Some(t.clone().into()),
            (Some(t), true) if !t.is_empty() && !tenant => Some(t.clone().into()),
            (None, false) => None,
            _ => return None,
        };
        let mut scope = match tables.as_deref() {
            Some([]) => self.scope.min(Scope::Write),
            t => t.map_or(self.scope, |t| widest(t).min(self.scope)),
        };
        // A role reads tables, and writes none.
        if self.role.is_some() {
            scope = scope.min(Scope::Read);
        }
        // An allowance is a limited key's alone.
        let allow = self.allow.clone().filter(|_| tables.is_some()).map(Arc::new);
        Some(Principal { name: self.name.clone(), key: Some(self.id.clone()), scope, root: false, tables, role: self.role, agent: self.agent || self.role.is_some(), on_behalf_of: self.on_behalf_of.as_deref().map(Into::into), allow })
    }
}

/// A revoked key's id, kept so that the revocation wins wherever keys are
/// merged; and what the key was, so that Access still names it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revoked {
    pub id: String,
    pub revoked_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_by: Option<String>,
    /// The digest of its secret, to name the key a refused request still
    /// sends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

impl Revoked {
    /// What the API shows of it.
    pub fn json(&self) -> Json {
        json!({"id": self.id, "name": self.name, "scope": self.scope, "owner": self.owner, "revoked_ms": self.revoked_ms, "revoked_by": self.revoked_by, "prefix": format!("sk_{}_", self.id)})
    }
}

/// Keys and the ids of revoked ones: what a keys file holds and the
/// `Keys` call carries, as JSON.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct KeySet {
    pub keys: Vec<Key>,
    #[serde(default)]
    pub revoked: Vec<Revoked>,
}

impl KeySet {
    /// A keys file or a call's JSON: a set, or the list of keys that keys
    /// files held before revocations were kept.
    pub fn from_json(b: &[u8]) -> Result<KeySet, String> {
        match serde_json::from_slice::<Json>(b).map_err(|e| e.to_string())? {
            Json::Array(keys) => Ok(KeySet { keys: serde_json::from_value(Json::Array(keys)).map_err(|e| e.to_string())?, revoked: Vec::new() }),
            j => serde_json::from_value(j).map_err(|e| e.to_string()),
        }
    }

    fn is_revoked(&self, id: &str) -> bool {
        self.revoked.iter().any(|r| r.id == id)
    }

    /// This set with `other` merged in, and whether its keys or
    /// revocations changed: every revocation of either, and every key of
    /// either, by id, but the revoked ones. Of two keys with one id and
    /// other secrets (two servers drew the same 64 random bits), the one
    /// with the smaller digest stays, so that every server keeps the same.
    /// The time a key was last used is the later of the two, and the time
    /// it ends the earlier (a key rotated on one server ends on every
    /// other, and none that still holds it unended brings it back).
    pub fn merged(&self, other: &KeySet) -> (KeySet, bool) {
        let mut next = self.clone();
        let mut changed = false;
        for r in &other.revoked {
            if !next.is_revoked(&r.id) {
                next.revoked.push(r.clone());
                changed = true;
            }
        }
        let before = next.keys.len();
        let revoked = std::mem::take(&mut next.revoked);
        next.keys.retain(|k| !revoked.iter().any(|r| r.id == k.id));
        next.revoked = revoked;
        changed |= next.keys.len() != before;
        for k in &other.keys {
            if next.is_revoked(&k.id) {
                continue;
            }
            match next.keys.iter_mut().find(|x| x.id == k.id) {
                Some(x) if x.sha256 == k.sha256 => {
                    if k.last_used_ms > x.last_used_ms {
                        x.last_addr = k.last_addr.clone();
                    }
                    x.last_used_ms = x.last_used_ms.max(k.last_used_ms);
                    let ends = match (x.expires_ms, k.expires_ms) {
                        (Some(a), Some(b)) => Some(a.min(b)),
                        (a, b) => a.or(b),
                    };
                    if ends != x.expires_ms {
                        x.expires_ms = ends;
                        changed = true;
                    }
                    // A server from before limits sent it back without its
                    // tables: the limits stay.
                    if x.tables.is_none() && k.tables.is_some() {
                        x.tables = k.tables.clone();
                        changed = true;
                    }
                    // Likewise its allowance, which such a server drops.
                    if x.allow.is_none() && k.allow.is_some() {
                        x.allow = k.allow.clone();
                        changed = true;
                    }
                    // And a tenant's binding, from an allowance it kept.
                    if let (Some(xa), Some(t)) = (x.allow.as_mut(), k.allow.as_ref().and_then(|a| a.tenant.as_ref())) {
                        if xa.tenant.is_none() {
                            xa.tenant = Some(t.clone());
                            changed = true;
                        }
                    }
                    // So do its role and whom it acts for.
                    if x.role.is_none() && k.role.is_some() {
                        x.role = k.role;
                        changed = true;
                    }
                    if !x.agent && k.agent {
                        x.agent = true;
                        changed = true;
                    }
                    if x.on_behalf_of.is_none() && k.on_behalf_of.is_some() {
                        x.on_behalf_of = k.on_behalf_of.clone();
                        changed = true;
                    }
                    // Its budget is the tighter of the two (one lowered on
                    // either side; a server from before budgets drops it).
                    if let Some(b) = k.budget {
                        let tighter = x.budget.unwrap_or_default().within(b);
                        if x.budget != Some(tighter) {
                            x.budget = Some(tighter);
                            changed = true;
                        }
                    }
                    // And without the key that made it.
                    if x.parent.is_none() && k.parent.is_some() {
                        x.parent = k.parent.clone();
                        changed = true;
                    }
                }
                Some(x) => {
                    if k.sha256 < x.sha256 {
                        *x = k.clone();
                        changed = true;
                    }
                }
                None => {
                    next.keys.push(k.clone());
                    changed = true;
                }
            }
        }
        (next, changed)
    }
}

/// How the last merge of keys with a server went ([`crate::keysync`]).
#[derive(Clone, Debug, Serialize)]
pub struct Peer {
    pub at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The server's keys, and the file that keeps them (none: in memory, lost
/// when the server stops, unless other servers of its cluster hold them).
pub struct Keys {
    set: Mutex<KeySet>,
    /// Keys of the apps this server starts ([`Self::app_key`]): in memory
    /// only, never written to the file nor given to other servers.
    ephemeral: Mutex<Vec<Key>>,
    file: Option<PathBuf>,
    /// Woken when the keys change, for [`crate::keysync`] to pass them on.
    pub changed: tokio::sync::Notify,
    /// The servers keys are merged with, and how the last merge went.
    peers: Mutex<BTreeMap<String, Peer>>,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn sha256(s: &str) -> String {
    hex(&Sha256::digest(s.as_bytes()))
}

/// `n` random bytes from the operating system, in hex (a token).
pub fn random_hex(n: usize) -> Result<String, ApiError> {
    Ok(hex(&random(n)?))
}

/// `n` random bytes from the operating system.
fn random(n: usize) -> Result<Vec<u8>, ApiError> {
    use std::io::Read;
    let mut b = vec![0; n];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).map_err(|e| ApiError::internal(format!("no random bytes for a key: {e}")))?;
    Ok(b)
}

impl Keys {
    /// The keys file of a database: `<db>.keys.json`.
    pub fn file_of(db: &std::path::Path) -> PathBuf {
        let mut p = db.as_os_str().to_owned();
        p.push(".keys.json");
        p.into()
    }

    /// No keys, kept in memory.
    pub fn memory() -> Keys {
        Keys::of(KeySet::default(), None)
    }

    fn of(set: KeySet, file: Option<PathBuf>) -> Keys {
        Keys { set: Mutex::new(set), ephemeral: Mutex::new(Vec::new()), file, changed: tokio::sync::Notify::new(), peers: Mutex::new(BTreeMap::new()) }
    }

    /// The keys of `file`, which a new key is written to (created when the
    /// first key is).
    pub fn open(file: PathBuf) -> Result<Keys, String> {
        let set = match std::fs::read(&file) {
            Ok(b) => KeySet::from_json(&b).map_err(|e| format!("{}: {e}", file.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => KeySet::default(),
            Err(e) => return Err(format!("{}: {e}", file.display())),
        };
        Ok(Keys::of(set, Some(file)))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, KeySet> {
        self.set.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn file(&self) -> Option<&std::path::Path> {
        self.file.as_deref()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().keys.is_empty()
    }

    /// The statement budget of key `id` ([`Key::budget`]); none for a key
    /// it does not hold.
    pub fn budget_of(&self, id: &str) -> chloedb::query::budget::Budget {
        self.lock().keys.iter().find(|k| k.id == id).and_then(|k| k.budget).unwrap_or_default()
    }

    /// Lowers key `id`'s statement budget (its id or name) to within
    /// `budget`, and that of every key it made; `None` when it holds no
    /// such key, else the budget it has now. A budget is lowered, never
    /// raised, as an expiry is brought nearer: the servers of a cluster
    /// merge a key's budgets to the tighter ([`KeySet::merged`]), so a
    /// raise would not reach them. A key that needs more is a new key.
    pub fn lower_budget(&self, id: &str, budget: chloedb::query::budget::Budget) -> Result<Option<chloedb::query::budget::Budget>, ApiError> {
        let mut set = self.lock();
        let mut next = set.clone();
        let Some(k) = next.keys.iter_mut().find(|k| k.id == id || k.name == id) else { return Ok(None) };
        let now = k.budget.unwrap_or_default().within(budget);
        k.budget = Some(now).filter(|b| !b.is_none());
        let kid = k.id.clone();
        for c in next.keys.iter_mut().filter(|c| c.parent.as_deref() == Some(kid.as_str())) {
            c.budget = Some(c.budget.unwrap_or_default().within(now)).filter(|b| !b.is_none());
        }
        self.save(&next)?;
        *set = next;
        drop(set);
        self.changed.notify_one();
        Ok(Some(now))
    }

    pub fn list(&self) -> Vec<Json> {
        let mut out: Vec<Json> = self.lock().keys.iter().map(Key::json).collect();
        out.extend(self.ephemeral().iter().map(|k| {
            let mut j = k.json();
            j["ephemeral"] = json!(true);
            j
        }));
        out
    }

    fn ephemeral(&self) -> std::sync::MutexGuard<'_, Vec<Key>> {
        self.ephemeral.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A key for app `app`, which this server starts and hands it to:
    /// limited to `tables` ([`Grant`]), named and owned `app:<app>`, kept
    /// in memory only (not in the keys file, not given to the other
    /// servers of a cluster) and in place of the app's last one, so that
    /// each start of the app has a new key and the server's stop ends it.
    /// It does not close an open server: a request with no credential
    /// stays anyone's, one with this key is the app's. Its secret, not
    /// kept, is the answer.
    pub fn app_key(&self, app: &str, tables: Vec<Grant>) -> Result<String, ApiError> {
        if app.is_empty() || app.len() > 48 || !app.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') {
            return Err(ApiError::bad(format!("{app:?} is not an app's name: lowercase letters, digits, - and _")));
        }
        if tables.is_empty() {
            return Err(ApiError::bad("an app's key names one table grant at least"));
        }
        self.ephemeral_key(&format!("app:{app}"), format!("made by this server for the app {app} it starts"), tables)
    }

    /// A key named `name` that this server makes for work of its own (an
    /// app it starts, a connector's run): limited to `tables`, kept in
    /// memory only, in place of the last one of that name. Its secret, not
    /// kept, is the answer.
    pub fn ephemeral_key(&self, name: &str, note: String, tables: Vec<Grant>) -> Result<String, ApiError> {
        if tables.is_empty() {
            return Err(ApiError::bad("a key made by the server names one table grant at least"));
        }
        let id = hex(&random(8)?);
        let secret = hex(&random(24)?);
        let full = format!("sk_{id}_{secret}");
        let scope = widest(&tables);
        let key = Key { id, name: name.to_string(), scope, created_ms: now_ms(), expires_ms: None, sha256: stored_digest(&secret, true, false), last_used_ms: 0, owner: Some(name.to_string()), note: Some(note), last_addr: None, tables: Some(tables), parent: None, role: None, agent: false, on_behalf_of: None, allow: None, budget: None, session: None };
        let mut e = self.ephemeral();
        e.retain(|k| k.name != name);
        e.push(key);
        Ok(full)
    }

    /// Ends the key named `name` that [`Self::ephemeral_key`] made.
    pub fn end_ephemeral(&self, name: &str) -> bool {
        let mut e = self.ephemeral();
        let before = e.len();
        e.retain(|k| k.name != name);
        e.len() != before
    }

    /// Ends app `app`'s key ([`Self::app_key`]); whether it had one.
    pub fn end_app_key(&self, app: &str) -> bool {
        let name = format!("app:{app}");
        let mut e = self.ephemeral();
        let before = e.len();
        e.retain(|k| k.name != name);
        e.len() != before
    }

    /// The keys and revocations, to give another server.
    pub fn set(&self) -> KeySet {
        self.lock().clone()
    }

    /// Merges `other` into these keys ([`KeySet::merged`]), keeping the
    /// result in the file when it changed (and waking [`Self::changed`]).
    /// Whether it did.
    pub fn merge(&self, other: &KeySet) -> Result<bool, ApiError> {
        let mut set = self.lock();
        let (next, changed) = set.merged(other);
        if changed {
            self.save(&next)?;
        }
        *set = next;
        drop(set);
        if changed {
            self.changed.notify_one();
        }
        Ok(changed)
    }

    /// Records how the last round of merges went with each server
    /// ([`crate::keysync`]): the error, for one that failed.
    pub fn synced(&self, peers: Vec<(String, Option<String>)>) {
        let at_ms = now_ms();
        *self.peers.lock().unwrap_or_else(|e| e.into_inner()) = peers.into_iter().map(|(url, error)| (url, Peer { at_ms, error })).collect();
    }

    /// The servers keys were last merged with: `{url: {at_ms, error?}}`.
    pub fn peers(&self) -> Json {
        json!(*self.peers.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Writes the keys to the file, through a new file renamed over it,
    /// readable by its owner only.
    fn save(&self, set: &KeySet) -> Result<(), ApiError> {
        let Some(path) = &self.file else { return Ok(()) };
        let tmp = path.with_extension("tmp");
        let body = serde_json::to_vec_pretty(set).map_err(ApiError::internal)?;
        let write = || -> std::io::Result<()> {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
            f.write_all(&body)?;
            f.sync_all()?;
            std::fs::rename(&tmp, path)
        };
        write().map_err(|e| ApiError::internal(format!("write {}: {e}", path.display())))
    }

    /// A new key; its secret, which is not kept, is in the answer only.
    /// On a server without a token or a cluster token (`token` false), the
    /// first key must be an admin key, and the keys must keep one admin
    /// key that never expires ([`managed`]); such a server refuses a key
    /// while it serves in a cluster (`in_cluster`), since the key would
    /// close it to the servers that call it, which send no credential.
    pub fn create(&self, name: &str, scope: Scope, expires_ms: Option<u64>, token: bool, in_cluster: bool) -> Result<(Key, String), ApiError> {
        self.create_owned(name, scope, expires_ms, None, None, token, in_cluster)
    }

    /// [`Self::create`], with who answers for the key and what it is for.
    #[allow(clippy::too_many_arguments)]
    pub fn create_owned(&self, name: &str, scope: Scope, expires_ms: Option<u64>, owner: Option<String>, note: Option<String>, token: bool, in_cluster: bool) -> Result<(Key, String), ApiError> {
        self.create_limited(name, scope, None, expires_ms, owner, note, token, in_cluster)
    }

    /// [`Self::create_owned`], limited to `tables` when given
    /// ([`Grant`]): its scope is then the most its grants give, and no
    /// more than `scope`. Such a key is never a server's first: it does not
    /// manage the server.
    #[allow(clippy::too_many_arguments)]
    pub fn create_limited(&self, name: &str, scope: Scope, tables: Option<Vec<Grant>>, expires_ms: Option<u64>, owner: Option<String>, note: Option<String>, token: bool, in_cluster: bool) -> Result<(Key, String), ApiError> {
        self.create_full(name, scope, tables, expires_ms, owner, note, AgentKey::default(), token, in_cluster)
    }

    /// [`Self::create_limited`], for an agent or for operations
    /// ([`AgentKey`]): a key with a role reads tables and writes none, and
    /// is never a server's first.
    #[allow(clippy::too_many_arguments)]
    pub fn create_full(&self, name: &str, scope: Scope, tables: Option<Vec<Grant>>, expires_ms: Option<u64>, owner: Option<String>, note: Option<String>, more: AgentKey, token: bool, in_cluster: bool) -> Result<(Key, String), ApiError> {
        let allow = more.allow.clone().filter(|a| !a.is_empty());
        // A key bound to a tenant (KENJI-402): limited to no table, with no
        // other allowance, reading or writing its tenant's documents.
        let mut tables = tables;
        if let Some(a) = allow.as_ref().filter(|a| a.tenant.is_some()) {
            if tables.is_some() || a.preset.is_some() || !a.routes.is_empty() || !a.apps.is_empty() {
                return Err(ApiError::bad("a key bound to a tenant reaches its tenant's documents on the OpenSearch API and nothing else: give it no tables, routes, apps or preset"));
            }
            if more.role.is_some() || !(scope == Scope::Read || scope == Scope::Write) {
                return Err(ApiError::bad("a key bound to a tenant has the read or the write scope"));
            }
            tables = Some(Vec::new());
        }
        let tenant = tables.as_ref().is_some_and(|t| t.is_empty());
        if allow.is_some() && tables.is_none() {
            return Err(ApiError::bad("routes, apps and presets are for a key limited to tables: a key of every table reaches every route its scope allows"));
        }
        let name = name.trim();
        let scope = if more.role.is_some() { scope.min(Scope::Read) } else { scope };
        if let Some(p) = &more.on_behalf_of {
            if p.trim().is_empty() || p.chars().count() > 200 {
                return Err(ApiError::bad("on_behalf_of names a person: 1 to 200 characters"));
            }
        }
        if name.starts_with("app:") {
            return Err(ApiError::bad("a key's name does not start with app: (the names of the keys a server makes for the apps it starts)"));
        }
        if !tenant && tables.as_ref().is_some_and(|t| t.is_empty()) {
            return Err(ApiError::bad("a key limited to tables names one at least"));
        }
        let scope = match tables.as_deref() {
            Some(t) if !tenant => widest(t).min(scope),
            _ => scope,
        };
        if name.is_empty() || name.len() > 64 {
            return Err(ApiError::bad("a key's name has 1 to 64 characters"));
        }
        if !token && in_cluster {
            return Err(ApiError {
                code: None, status: 409,
                message: "this server serves in a cluster and has no token: a key would close it to the servers that call it. Give the cluster's servers a --cluster-token, or make the key on the coordinator".into(),
            });
        }
        let mut set = self.lock();
        if !token && set.keys.is_empty() && (scope != Scope::Admin || tables.is_some() || more.agent) {
            return Err(ApiError::bad("this server has no token and no key: its first key turns on authentication, so it must be an admin key of every table"));
        }
        if set.keys.iter().any(|k| k.name == name) {
            return Err(ApiError { code: None, status: 409, message: format!("a key named {name:?} exists") });
        }
        let id = hex(&random(8)?);
        let secret = hex(&random(24)?);
        let full = format!("sk_{id}_{secret}");
        let key = Key {
            id,
            name: name.to_string(),
            scope,
            created_ms: now_ms(),
            expires_ms,
            sha256: stored_digest(&secret, tables.is_some(), false),
            last_used_ms: 0,
            owner,
            note,
            last_addr: None,
            tables,
            parent: None,
            role: more.role,
            agent: more.agent || more.role.is_some(),
            on_behalf_of: more.on_behalf_of.map(|p| p.trim().to_string()),
            allow,
            budget: more.budget.filter(|b| !b.is_none()),
            session: None,
        };
        let mut next = set.clone();
        next.keys.push(key.clone());
        if !token && !managed(&next.keys) {
            return Err(ApiError::bad(
                "this server has no token, so one of its admin keys must never expire: once the last one expired, no one could manage the server. Make an admin key without expires_days first",
            ));
        }
        self.save(&next)?;
        *set = next;
        drop(set);
        self.changed.notify_one();
        Ok((key, full))
    }

    /// What a key an agent makes ([`crate::ops::is_agent`]) with `POST
    /// /v1/admin/keys` may be (LENA-2, CLU-4): an agent's key too, acting
    /// for the person its maker acts for, expiring no later than its maker
    /// and with a statement budget no looser, so that an agent never makes
    /// a key that escapes the fence it is in (an agent's key does not
    /// approve changes nor act at tier 3 alone, [`crate::ops`]). `asked`
    /// is what the request said of `agent`, if anything; `more` and
    /// `expires_ms` are made so. A person makes any other key, or approves
    /// a change request for one.
    pub fn fence_agents_key(&self, by: &Principal, asked: Option<bool>, more: &mut AgentKey, expires_ms: &mut Option<u64>) -> Result<(), ApiError> {
        let refuse = |m: String| ApiError::coded(403, "change_request_required", m);
        if asked == Some(false) {
            return Err(refuse(format!(
                "{:?} is an agent's key: the keys it makes are agents' keys too (\"agent\": true); a person makes any other, or approves a change request (POST /v1/admin/changes) for one",
                by.name
            )));
        }
        more.agent = true;
        if let Some(p) = by.on_behalf_of.as_deref() {
            match more.on_behalf_of.as_deref().map(str::trim) {
                Some(q) if q != p => return Err(refuse(format!("the keys {:?} makes act for {p}, as it does, not for {q}", by.name))),
                _ => more.on_behalf_of = Some(p.to_string()),
            }
        }
        let maker = by.key.as_deref().and_then(|id| self.lock().keys.iter().find(|k| k.id == id).cloned());
        if let Some(m) = maker {
            if let Some(e) = m.expires_ms {
                *expires_ms = Some(expires_ms.map_or(e, |x| x.min(e)));
            }
            if let Some(b) = m.budget {
                more.budget = Some(more.budget.map_or(b, |x| x.within(b)));
            }
        }
        Ok(())
    }

    /// Whether key `id` is an agent's (or has a role), for [`Self::rotate`]
    /// by an agent; `None` for no such key.
    pub fn is_agents(&self, id: &str) -> Option<bool> {
        self.lock().keys.iter().find(|k| k.id == id).map(|k| k.agent || k.role.is_some())
    }

    /// A session for the person `email`, who signed in by `provider`
    /// (`google`, [`crate::signin`]), at `scope`: a key of every table,
    /// owned by their address, that lasts [`SESSION_MAX_MS`] at most and
    /// [`SESSION_IDLE_MS`] unused, kept and synced as the other keys are,
    /// so that every coordinator takes it and a revocation ends it
    /// everywhere. Past [`SESSIONS_PER_PERSON`] of theirs, their oldest is
    /// revoked. It goes in a cookie, never in a page: its secret, not
    /// kept, is the answer's. A server with no token and no admin key that
    /// never expires makes none (a session would close an open server, or
    /// be its only admin until it ends).
    pub fn create_session(&self, provider: &str, email: &str, scope: Scope, token: bool, in_cluster: bool) -> Result<(Key, String), ApiError> {
        let email = email.trim().to_ascii_lowercase();
        if email.is_empty() || email.len() > 254 || !email.contains('@') || email.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(ApiError::bad("a session is a person's: their address"));
        }
        if !matches!(provider, "google") {
            return Err(ApiError::bad(format!("no sign-in by {provider:?}")));
        }
        let mut set = self.lock();
        if !token && (in_cluster || !managed(&set.keys)) {
            return Err(ApiError {
                code: None, status: 409,
                message: "this server has no token and no admin key that never expires: a sign-in would close it, or be its only admin until the session ends. Give it a --token (or an admin key) first".into(),
            });
        }
        let now = now_ms();
        let id = hex(&random(8)?);
        let secret = hex(&random(24)?);
        let full = format!("sk_{id}_{secret}");
        // The name says whose it is, and which of theirs (names are unique).
        let short: String = email.chars().take(48).collect();
        let key = Key {
            id: id.clone(),
            name: format!("{short} ({})", &id[..6]),
            scope,
            created_ms: now,
            expires_ms: Some(now + SESSION_MAX_MS),
            sha256: format!("{SESSION}{}", sha256(&secret)),
            last_used_ms: now,
            owner: Some(email.clone()),
            note: Some(format!("signed in with {}", if provider == "google" { "Google" } else { provider })),
            last_addr: caller().and_then(|c| c.addr),
            tables: None,
            parent: None,
            role: None,
            agent: false,
            on_behalf_of: None,
            allow: None,
            budget: None,
            session: Some(provider.to_string()),
        };
        let mut next = set.clone();
        // Their sessions, the oldest first: those past the limit end.
        let mut theirs: Vec<Key> = next.keys.iter().filter(|k| k.session.is_some() && k.owner.as_deref() == Some(email.as_str())).cloned().collect();
        theirs.sort_by_key(|k| k.created_ms);
        let over = (theirs.len() + 1).saturating_sub(SESSIONS_PER_PERSON);
        for old in theirs.into_iter().take(over) {
            next.keys.retain(|k| k.id != old.id);
            let scope = Some(old.scope_name().into());
            next.revoked.push(Revoked { id: old.id, revoked_ms: now, name: Some(old.name), scope, owner: old.owner, revoked_by: Some(format!("{email}'s newer sign-in")), sha256: Some(old.sha256) });
        }
        next.keys.push(key.clone());
        self.save(&next)?;
        *set = next;
        drop(set);
        self.changed.notify_one();
        Ok((key, full))
    }

    /// Whether key `id` is one these keys hold or revoked.
    pub fn knows(&self, id: &str) -> bool {
        let set = self.lock();
        set.keys.iter().any(|k| k.id == id) || set.is_revoked(id)
    }

    /// The person and the way they signed in of session `id`
    /// ([`Self::create_session`]); `None` for any other key.
    pub fn session(&self, id: &str) -> Option<(String, String)> {
        let set = self.lock();
        let k = set.keys.iter().find(|k| k.id == id)?;
        Some((k.owner.clone()?, k.session.clone()?))
    }

    /// A key made by key `by` (a request's principal) for something of
    /// its own, such as an app of an app engine: limited to `tables`, each
    /// of them within a grant of `by` with the admin scope there (`by` may
    /// manage those tables, so it may hand them on), named
    /// `<by's name>/<name>`, owned by `by`'s name, and opening nothing once
    /// `by` is revoked or expires ([`Key::parent`]). A key of that name
    /// that `by` made is revoked in its place, so that asking again is a
    /// new secret. `by` is a key of this server's set (not a token, not an
    /// app's key in memory, not a key another key made), with fewer than
    /// [`DELEGATED_MAX`] such keys. Its secret, not kept, is the answer's.
    pub fn create_delegated(&self, by: &Principal, name: &str, tables: Vec<Grant>, note: Option<String>, in_cluster_without_token: bool) -> Result<(Key, String), ApiError> {
        let name = name.trim();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') {
            return Err(ApiError::bad(format!("{name:?} is not the name of a key made by a key: letters, digits, '-', '_' and '.'")));
        }
        if tables.is_empty() || tables.len() > 64 {
            return Err(ApiError::bad("a key made by a key names 1 to 64 table grants"));
        }
        if in_cluster_without_token {
            return Err(ApiError { code: None, status: 409, message: "this server serves in a cluster and has no token: make the key on the coordinator".into() });
        }
        let refuse = |m: String| ApiError { code: None, status: 403, message: m };
        let Some(parent_id) = by.key.clone() else {
            return Err(refuse(format!("{:?} is not a key: a key makes keys of its own here; the token's holder makes keys with POST /v1/admin/keys", by.name)));
        };
        if self.ephemeral().iter().any(|k| k.id == parent_id) {
            return Err(refuse(format!("{:?} is a key this server made for an app it starts, kept in memory: it makes no keys", by.name)));
        }
        let mut set = self.lock();
        let parent = set.keys.iter().find(|k| k.id == parent_id).cloned().ok_or_else(|| refuse(format!("no key {parent_id} here")))?;
        if parent.parent.is_some() {
            return Err(refuse(format!("{:?} was made by another key: it makes none in turn", parent.name)));
        }
        let held: Vec<Grant> = match &parent.tables {
            Some(t) => t.clone(),
            None => vec![Grant { table: "*".into(), scope: parent.scope }],
        };
        for g in &tables {
            if !held.iter().any(|h| h.scope == Scope::Admin && within(g, h)) {
                return Err(refuse(format!("{} ({}) is not within a table grant {:?} has the admin scope on ({})", g.table, g.scope.name(), parent.name, grants_text(&held))));
            }
        }
        let full_name = format!("{}/{name}", parent.name);
        if full_name.len() > 128 {
            return Err(ApiError::bad("a key made by a key has a name of 128 characters at most, with its maker's"));
        }
        let before: Vec<Key> = set.keys.iter().filter(|k| k.parent.as_deref() == Some(parent_id.as_str()) && k.name == full_name).cloned().collect();
        if set.keys.iter().any(|k| k.name == full_name && k.parent.as_deref() != Some(parent_id.as_str())) {
            return Err(ApiError { code: None, status: 409, message: format!("a key named {full_name:?} exists, made otherwise") });
        }
        if set.keys.iter().filter(|k| k.parent.as_deref() == Some(parent_id.as_str())).count() - before.len() >= DELEGATED_MAX {
            return Err(ApiError { code: None, status: 409, message: format!("{:?} has made {DELEGATED_MAX} keys, the most: revoke some first", parent.name) });
        }
        let id = hex(&random(8)?);
        let secret = hex(&random(24)?);
        let full = format!("sk_{id}_{secret}");
        let now = now_ms();
        let scope = widest(&tables);
        let key = Key { id, name: full_name, scope, created_ms: now, expires_ms: parent.expires_ms, sha256: stored_digest(&secret, true, true), last_used_ms: 0, owner: Some(parent.name.clone()), note, last_addr: None, tables: Some(tables), parent: Some(parent_id), role: None, agent: parent.agent, on_behalf_of: parent.on_behalf_of.clone(), allow: None, budget: parent.budget, session: None };
        let mut next = set.clone();
        next.keys.retain(|k| !before.iter().any(|b| b.id == k.id));
        for b in before {
            next.revoked.push(Revoked { id: b.id, revoked_ms: now, name: Some(b.name), scope: Some(b.scope.name().into()), owner: b.owner, revoked_by: Some(format!("{} (made again)", parent.name)), sha256: Some(b.sha256) });
        }
        next.keys.push(key.clone());
        self.save(&next)?;
        *set = next;
        drop(set);
        self.changed.notify_one();
        Ok((key, full))
    }

    /// The keys key `parent` made ([`Self::create_delegated`]).
    pub fn delegated(&self, parent: &str) -> Vec<Key> {
        self.lock().keys.iter().filter(|k| k.parent.as_deref() == Some(parent)).cloned().collect()
    }

    /// Revokes key `id`; false if there is none. On a server without a
    /// token (`token` false), its last admin key that never expires is not
    /// revoked: that would open the server, or leave it to be locked out
    /// when the others expire.
    /// The revocation is kept, so that it wins wherever these keys are
    /// merged.
    pub fn revoke(&self, id: &str, token: bool) -> Result<bool, ApiError> {
        self.revoke_by(id, token, None)
    }

    /// [`Self::revoke`], by `by` (a key's name, for the list of revoked
    /// keys).
    pub fn revoke_by(&self, id: &str, token: bool, by: Option<String>) -> Result<bool, ApiError> {
        {
            // An app's key: ended here, where alone it is.
            let mut e = self.ephemeral();
            if let Some(i) = e.iter().position(|k| k.id == id) {
                e.remove(i);
                return Ok(true);
            }
        }
        let mut set = self.lock();
        let mut next = set.clone();
        let Some(k) = set.keys.iter().find(|k| k.id == id).cloned() else { return Ok(false) };
        // The keys it made go with it, so that the revocation of each is
        // kept and spreads as its own.
        let gone: Vec<Key> = set.keys.iter().filter(|c| c.id == id || c.parent.as_deref() == Some(id)).cloned().collect();
        next.keys.retain(|c| !gone.iter().any(|g| g.id == c.id));
        let at = now_ms();
        for g in gone {
            let by = match g.id == k.id {
                true => by.clone(),
                false => Some(format!("{} (revoked with the key that made it, {})", by.clone().unwrap_or_else(|| "?".into()), k.name)),
            };
            let scope = Some(g.scope_name().into());
            next.revoked.push(Revoked { id: g.id, revoked_ms: at, name: Some(g.name), scope, owner: g.owner, revoked_by: by, sha256: Some(g.sha256) });
        }
        if !token && !managed(&next.keys) {
            return Err(ApiError {
                code: None, status: 409,
                message: "this is the last admin key that never expires on a server without a token: revoking it would leave no one to manage the server once the others expire (or open it). Make another admin key without expires_days first".into(),
            });
        }
        self.save(&next)?;
        *set = next;
        drop(set);
        self.changed.notify_one();
        Ok(true)
    }

    /// The revoked keys, the latest first.
    pub fn revoked(&self) -> Vec<Json> {
        let mut v: Vec<Revoked> = self.lock().revoked.clone();
        v.sort_by(|a, b| b.revoked_ms.cmp(&a.revoked_ms));
        v.iter().map(Revoked::json).collect()
    }

    /// A new key in place of key `id`: of its name, scope, owner and note,
    /// with the old one's lifetime counted from now (none if it had none);
    /// the old one expires `grace_ms` from now (or when it did, if sooner).
    /// Its secret, not kept, is in the answer only.
    pub fn rotate(&self, id: &str, grace_ms: u64, token: bool) -> Result<(Key, String, Key), ApiError> {
        let mut set = self.lock();
        let now = now_ms();
        let old = set.keys.iter().find(|k| k.id == id).cloned().ok_or_else(|| ApiError { code: None, status: 404, message: format!("no key {id}") })?;
        if old.expires_ms.is_some_and(|e| now >= e) {
            return Err(ApiError { code: None, status: 409, message: format!("key {} has expired: make a new one instead", old.name) });
        }
        if old.session.is_some() {
            return Err(ApiError { code: None, status: 409, message: format!("{} is a person's session: it is not rotated (revoke it, and they sign in again)", old.name) });
        }
        let new_id = hex(&random(8)?);
        let secret = hex(&random(24)?);
        let full = format!("sk_{new_id}_{secret}");
        let expires_ms = old.expires_ms.map(|e| now + e.saturating_sub(old.created_ms));
        let key = Key { id: new_id, sha256: stored_digest(&secret, old.tables.is_some(), old.parent.is_some()), created_ms: now, expires_ms, last_used_ms: 0, last_addr: None, ..old.clone() };
        let mut next = set.clone();
        let until = now + grace_ms;
        let mut ending = old.clone();
        ending.expires_ms = Some(old.expires_ms.map_or(until, |e| e.min(until)));
        if let Some(k) = next.keys.iter_mut().find(|k| k.id == id) {
            k.expires_ms = ending.expires_ms;
        }
        next.keys.push(key.clone());
        if !token && !managed(&next.keys) {
            return Err(ApiError {
                code: None, status: 409,
                message: "this server has no token, so one of its admin keys must never expire; rotating this one would leave none. Make an admin key without expires_days first".into(),
            });
        }
        self.save(&next)?;
        *set = next;
        drop(set);
        self.changed.notify_one();
        Ok((key, full, ending))
    }

    /// The key a refused credential names, if it is a key these keys knew
    /// (expired or revoked too) with that secret: its id, its name and
    /// why it no longer opens anything.
    pub fn identify(&self, authorization: Option<&str>) -> Option<(String, String, &'static str)> {
        let rest = authorization?.strip_prefix("Bearer ")?.strip_prefix("sk_")?;
        let (id, secret) = rest.split_once('_')?;
        let digest = sha256(secret);
        let set = self.lock();
        if let Some(k) = set.keys.iter().find(|k| k.id == id && digest_is(&k.sha256, &digest)) {
            let why = if k.expires_ms.is_some_and(|e| now_ms() >= e) { "expired" } else { "valid" };
            return Some((k.id.clone(), k.name.clone(), why));
        }
        let r = set.revoked.iter().find(|r| r.id == id && r.sha256.as_ref().is_none_or(|d| digest_is(d, &digest)))?;
        Some((r.id.clone(), r.name.clone().unwrap_or_else(|| format!("key {}", r.id)), "revoked"))
    }

    /// Whether an admin key that never expires is among these keys: on a
    /// server without a token, the credential sure to manage it
    /// ([`managed`]).
    pub fn managed(&self) -> bool {
        managed(&self.lock().keys)
    }

    /// The key a bearer credential names, if it is one of these, not
    /// expired.
    fn find(&self, given: &str) -> Option<Principal> {
        let rest = given.strip_prefix("sk_")?;
        let (id, secret) = rest.split_once('_')?;
        let digest = sha256(secret);
        let mut set = self.lock();
        let now = now_ms();
        let i = set.keys.iter().position(|k| k.id == id)?;
        let k = &set.keys[i];
        if !digest_is(&k.sha256, &digest) || k.expires_ms.is_some_and(|e| now >= e) {
            return None;
        }
        // A person's session ends unused, as well as at its end.
        if k.session.is_some() && now.saturating_sub(k.last_used_ms) >= SESSION_IDLE_MS {
            return None;
        }
        // A key another key made opens nothing once that key no longer
        // does (revoked, expired, or itself refused).
        if let Some(parent) = &k.parent {
            let p = set.keys.iter().find(|p| &p.id == parent)?;
            if p.parent.is_some() || p.expires_ms.is_some_and(|e| now >= e) || p.principal().is_none() {
                return None;
            }
        }
        let k = &mut set.keys[i];
        // Kept in memory; written with the next change of keys.
        k.last_used_ms = now;
        if let Some(addr) = caller().and_then(|c| c.addr) {
            k.last_addr = Some(addr);
        }
        k.principal()
    }

    /// The app key a bearer credential names ([`Self::app_key`]), if it is
    /// one of this server's.
    fn find_app(&self, given: &str) -> Option<Principal> {
        let (id, secret) = given.strip_prefix("sk_")?.split_once('_')?;
        let digest = sha256(secret);
        let mut e = self.ephemeral();
        let k = e.iter_mut().find(|k| k.id == id && digest_is(&k.sha256, &digest))?;
        k.last_used_ms = now_ms();
        if let Some(addr) = caller().and_then(|c| c.addr) {
            k.last_addr = Some(addr);
        }
        k.principal()
    }

    /// Who an `Authorization` header value makes a request: another
    /// server of the cluster (by the `cluster` token), the `token`'s
    /// holder, a key's, or (with no token, no cluster token and no key)
    /// anyone. `None`: a wrong or missing credential.
    pub fn resolve(&self, token: Option<&Secret>, cluster: Option<&Secret>, authorization: Option<&str>) -> Option<Principal> {
        // An app's key is the app's, on an open server too.
        if let Some(p) = authorization.and_then(|v| v.strip_prefix("Bearer ")).and_then(|g| self.find_app(g)) {
            return Some(p);
        }
        if token.is_none() && cluster.is_none() && self.is_empty() {
            return Some(Principal::open());
        }
        if cluster.is_some_and(|c| c.admits(authorization)) {
            return Some(Principal::cluster());
        }
        if token.is_some_and(|t| t.admits(authorization)) {
            return Some(Principal::token(cluster.is_none()));
        }
        let given = authorization.and_then(|v| v.strip_prefix("Bearer "))?;
        self.find(given)
    }
}

/// Whether `keys` hold an admin key that never expires.
fn managed(keys: &[Key]) -> bool {
    keys.iter().any(|k| k.scope == Scope::Admin && k.tables.is_none() && k.expires_ms.is_none() && !k.agent)
}

tokio::task_local! {
    static NOTE: std::cell::RefCell<(Option<String>, Change)>;
}

/// Runs `f`, a request, with room for what the audit log is to say of it
/// ([`note`]): its output, and that note.
pub fn noting<F: std::future::Future>(f: F) -> impl std::future::Future<Output = (F::Output, Option<String>)> {
    use futures::FutureExt;
    noting_change(f).map(|(out, note, _)| (out, note))
}

/// [`noting`], with what the request changed ([`note_change`]).
pub fn noting_change<F: std::future::Future>(f: F) -> impl std::future::Future<Output = (F::Output, Option<String>, Change)> {
    use futures::FutureExt;
    // The note is taken as `f` ends, while the scope still holds it.
    NOTE.scope(
        std::cell::RefCell::new((None, Change::default())),
        f.map(|out| {
            let (note, change) = NOTE.with(|n| std::mem::take(&mut *n.borrow_mut()));
            (out, note, change)
        }),
    )
}

/// Says what the request being served changed, for its audit entry: the
/// object, and it before and after. Outside a request, nothing.
pub fn note_change(c: Change) {
    let _ = NOTE.try_with(|n| n.borrow_mut().1 = c);
}

/// Says what the audit log keeps of the request being served (a table's
/// statement, the tables an import creates), which audits it whatever its
/// route or method. Outside a request ([`noting`]), nothing.
pub fn note(detail: impl Into<String>) {
    let _ = NOTE.try_with(|n| n.borrow_mut().0 = Some(detail.into()));
}

/// The client header of the command center's own requests
/// (`x-strata-client: command-center`).
pub const COMMAND_CENTER: &str = "command-center";

/// Who is calling, as the request says: the address it came from (its
/// peer, or the first address of `X-Forwarded-For` when the peer is a
/// proxy the server trusts, `--trust-proxy`), the client (the command
/// center, by its `x-strata-client` header), the app (`x-strata-app`, else
/// the product of its `User-Agent`), and the reason given for a change
/// (`x-strata-reason`).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Caller {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub addr: Option<String>,
    /// What an agent says of itself (`x-chloedb-agent-model`, `-session`,
    /// `-run`), for the audit log ([`crate::ops::agent_of`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_session: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_run: Option<String>,
    /// The change request a request applies (`x-chloedb-change`, set by
    /// the server itself when a person approves one).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The person a hub's call is for, as the hub says
    /// ([`crate::hub::PERSON`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub person: Option<String>,
}

impl Caller {
    /// One of the command center's own requests.
    pub fn command_center(&self) -> bool {
        self.client.as_deref() == Some(COMMAND_CENTER)
    }

    /// The client as the audit log names it: the command center, or the
    /// app.
    pub fn named(&self) -> Option<String> {
        self.client.clone().or_else(|| self.app.clone())
    }
}

/// The product of a `User-Agent` (`nightly-reports/2.3 (linux)` is
/// `nightly-reports`).
pub fn product(user_agent: &str) -> Option<String> {
    let p = user_agent.split_whitespace().next()?.split('/').next()?.trim();
    (!p.is_empty()).then(|| p.chars().take(64).collect())
}

tokio::task_local! {
    static CALLER: Caller;
}

/// Runs `f`, a request, as `caller`'s ([`caller`]).
pub fn calling<F: std::future::Future>(caller: Caller, f: F) -> impl std::future::Future<Output = F::Output> {
    CALLER.scope(caller, f)
}

/// Who is calling in the request being served (`None` outside one).
pub fn caller() -> Option<Caller> {
    CALLER.try_with(Caller::clone).ok()
}

/// What an audit entry is: a change, a refusal (401, 403), or a note.
pub const CHANGE: &str = "change";
pub const REFUSAL: &str = "refusal";
pub const NOTE_KIND: &str = "note";

/// What the audit log keeps of a request.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Entry {
    pub ts_ms: u64,
    /// The key's name; "root token" for the token; "cluster" for another
    /// server; "open" on a server without credentials.
    pub who: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_deserializing)]
    pub scope: Option<&'static str>,
    pub method: String,
    pub route: String,
    pub status: u16,
    pub ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// [`CHANGE`], [`REFUSAL`] or [`NOTE_KIND`] (empty in entries from
    /// before kinds: read as a change, or a refusal by its status).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    /// The client's address, the client, and the reason it gave
    /// ([`Caller`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addr: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// What was changed, in words ("shard 2", "key oncall-bot", "alert
    /// rules", "table products").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    /// The object before and after, for rules, settings and spec patches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<Json>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<Json>,
    /// Who acted: "person" (a request), "agent" (an agent's request,
    /// [`crate::ops::is_agent`]), or the server's own "operator",
    /// "autoscaler" or "upgrader".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub actor: String,
    /// An agent's request: what it said of itself (`{model?, session?,
    /// run?, via?}`, [`crate::ops::agent_of`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<Json>,
    /// The person an agent's key acts for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_behalf_of: Option<String>,
    /// The change request this request applied ([`crate::changes`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_request: Option<String>,
    /// A call through a hub: the person signed in to it who made the
    /// call, as the hub says ([`crate::hub::PERSON`]); `who` is the hub's
    /// key, which is what was checked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person: Option<String>,
}

impl Entry {
    /// Its kind, for entries from before kinds too.
    pub fn kind(&self) -> &str {
        match self.kind.as_str() {
            "" if self.status == 401 || self.status == 403 => REFUSAL,
            "" => CHANGE,
            k => k,
        }
    }
}

/// What a handler adds to the audit entry of its request, beside its
/// detail ([`note_change`]): its kind (a change by default), the object,
/// it before and after, and who acted (a person by default).
#[derive(Clone, Debug, Default)]
pub struct Change {
    pub kind: Option<&'static str>,
    pub object: Option<String>,
    pub before: Option<Json>,
    pub after: Option<Json>,
    pub actor: Option<&'static str>,
}

/// The name the audit log gives a credential: a key's name, "root token"
/// for the token.
pub fn audit_name(p: &Principal) -> String {
    match (p.key.is_none(), p.name.as_str()) {
        (true, "token") => "root token".into(),
        _ => p.name.clone(),
    }
}

/// The requests that changed something or were refused: the latest in
/// memory, and every one appended to a file of JSON lines if given.
pub struct Audit {
    latest: Mutex<VecDeque<Arc<Entry>>>,
    file: Option<Mutex<std::fs::File>>,
    path: Option<PathBuf>,
}

/// Entries kept in memory.
pub const AUDIT: usize = 20_000;

impl Audit {
    pub fn new(path: Option<PathBuf>) -> Result<Audit, String> {
        let file = match &path {
            Some(p) => Some(Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(p).map_err(|e| format!("--audit-log {}: {e}", p.display()))?)),
            None => None,
        };
        Ok(Audit { latest: Mutex::new(VecDeque::with_capacity(AUDIT)), file, path })
    }

    pub fn path(&self) -> Option<&std::path::Path> {
        self.path.as_deref()
    }

    pub fn add(&self, e: Entry) {
        if let Some(f) = &self.file {
            use std::io::Write;
            let mut f = f.lock().unwrap_or_else(|e| e.into_inner());
            if let Ok(line) = serde_json::to_string(&e) {
                if let Err(err) = writeln!(f, "{line}") {
                    tracing::error!(error = %err, "could not append to the audit log");
                }
            }
        }
        let mut l = self.latest.lock().unwrap_or_else(|e| e.into_inner());
        if l.len() == AUDIT {
            l.pop_front();
        }
        l.push_back(Arc::new(e));
    }

    /// The latest entries, the newest first.
    pub fn latest(&self, limit: usize) -> Vec<Arc<Entry>> {
        self.latest.lock().unwrap_or_else(|e| e.into_inner()).iter().rev().take(limit).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limited_to(name: &str, grants: &[(&str, Scope)]) -> Principal {
        let g: Vec<Grant> = grants.iter().map(|(t, s)| Grant::new(t, *s).unwrap()).collect();
        Principal { name: name.into(), key: Some("k1".into()), scope: widest(&g), root: false, tables: Some(g.into()), role: None, agent: false, on_behalf_of: None, allow: None }
    }

    /// A person's session: of every table, owned by their address, ended
    /// unused after [`SESSION_IDLE_MS`] and at [`SESSION_MAX_MS`], eight a
    /// person at most, never rotated; and a server from before sessions,
    /// which keeps no `session`, takes no session's secret.
    #[test]
    fn a_session_ends_unused_and_an_older_server_takes_none() {
        let keys = Keys::memory();
        // An open server makes none: the session would close it.
        assert_eq!(keys.create_session("google", "ro@example.com", Scope::Admin, false, false).unwrap_err().status, 409);
        let (k, secret) = keys.create_session("google", "Ro@Example.com", Scope::Admin, true, false).unwrap();
        assert_eq!((k.owner.as_deref(), k.session.as_deref(), k.scope, k.tables.is_none()), (Some("ro@example.com"), Some("google"), Scope::Admin, true));
        assert!(k.name.starts_with("ro@example.com (") && k.sha256.starts_with(SESSION), "{k:?}");
        assert_eq!(k.expires_ms, Some(k.created_ms + SESSION_MAX_MS));
        let p = keys.find(&secret).expect("taken");
        assert_eq!((p.scope, p.agent, p.key.as_deref()), (Scope::Admin, false, Some(k.id.as_str())));
        assert_eq!(keys.session(&k.id), Some(("ro@example.com".into(), "google".into())));
        assert!(keys.knows(&k.id) && !keys.knows("nope"));
        assert_eq!(keys.rotate(&k.id, 0, true).unwrap_err().status, 409);
        // Unused for 12 hours: ended.
        keys.lock().keys.iter_mut().find(|x| x.id == k.id).unwrap().last_used_ms = now_ms() - SESSION_IDLE_MS - 1;
        assert!(keys.find(&secret).is_none());
        // Written back by a server from before sessions (no `session`): its
        // mark keeps it from being taken as a plain key, here and there.
        let (k2, s2) = keys.create_session("google", "ro@example.com", Scope::Read, true, false).unwrap();
        let stripped = Key { session: None, ..k2.clone() };
        assert!(stripped.principal().is_none());
        assert!(!digest_is_unmarked_by_old_servers(&k2.sha256, &sha256(s2.rsplit('_').next().unwrap())));
        // Eight a person: a ninth ends the oldest.
        for _ in 0..SESSIONS_PER_PERSON {
            keys.create_session("google", "ro@example.com", Scope::Read, true, false).unwrap();
        }
        let theirs = keys.lock().keys.iter().filter(|x| x.owner.as_deref() == Some("ro@example.com")).count();
        assert_eq!(theirs, SESSIONS_PER_PERSON);
        assert!(!keys.lock().keys.iter().any(|x| x.id == k.id || x.id == k2.id) && keys.knows(&k2.id), "the oldest revoked");
        assert!(keys.find(&s2).is_none());
    }

    /// How a server from before sessions compared a digest: without the
    /// marks of limited keys and keys other keys made, and no other.
    fn digest_is_unmarked_by_old_servers(stored: &str, digest: &str) -> bool {
        let old = stored.strip_prefix(LIMITED).or_else(|| stored.strip_prefix(DELEGATED)).unwrap_or(stored);
        same_secret(digest.as_bytes(), old.as_bytes())
    }

    /// DAT-5: the key a server gives an app it starts is limited to the
    /// app's tables, and narrowed to a caller's tables it keeps on each
    /// table the lower of the two scopes, and nothing where either has
    /// none: never wider than the app's key, nor than the caller's.
    #[test]
    fn a_limited_key_narrowed_to_a_callers_tables_keeps_what_both_give() {
        use Scope::{Admin, Read, Write};
        let sheets = limited_to("app:sheets", &[("sheets_*", Admin), ("*", Read)]);
        let at = |p: &Principal, t: &str| p.table_scope(t);
        // The root: what the app's key gives.
        let root = sheets.narrowed("*=admin").unwrap();
        assert_eq!((at(&root, "sheets_books"), at(&root, "docs"), root.scope, root.root), (Some(Admin), Some(Read), Admin, false));
        // A writer: writes Sheets' tables, makes none, reads the rest.
        let writer = sheets.narrowed("*=write").unwrap();
        assert_eq!((at(&writer, "sheets_cells"), at(&writer, "docs"), writer.scope), (Some(Write), Some(Read), Write));
        // A key of some tables: only those, at the lower scope.
        let some = sheets.narrowed("sheets_*=write,docs=read").unwrap();
        assert_eq!((at(&some, "sheets_books"), at(&some, "docs"), at(&some, "other")), (Some(Write), Some(Read), None));
        // A prefix within the app's, and one around it.
        let team = sheets.narrowed("sheets_team_*=admin,team_*=admin").unwrap();
        assert_eq!((at(&team, "sheets_team_a"), at(&team, "sheets_books"), at(&team, "team_x")), (Some(Admin), None, Some(Read)));
        // A reader cannot write through the app.
        let reader = sheets.narrowed("*=read").unwrap();
        assert_eq!((at(&reader, "sheets_books"), reader.scope), (Some(Read), Read));
        // Nothing in common: no table at all.
        let crawler = limited_to("app:crawler", &[("crawl_*", Admin)]);
        let none = crawler.narrowed("docs=admin").unwrap();
        assert_eq!((at(&none, "docs"), at(&none, "crawl_x")), (None, None));
        let root = crawler.narrowed("*=admin").unwrap();
        assert_eq!((at(&root, "crawl_example_com"), at(&root, "docs")), (Some(Admin), None));
        // A table an admin's grant by a pattern only reads stays read
        // only when the caller names it.
        let wide = limited_to("app:x", &[("*", Admin)]);
        let named = wide.narrowed(&format!("{}=admin", chloedb::autopilot::PROPOSALS)).unwrap();
        assert_eq!(at(&named, chloedb::autopilot::PROPOSALS), Some(Read));
        // A bad grant is refused, not ignored.
        assert!(sheets.narrowed("docs=sometimes").is_err());
    }

    /// A key bound to a tenant (KENJI-402): limited to no table, marked as
    /// limited (a server from before limits never takes it), read or write
    /// at most; a copy without its binding (a server from before tenants
    /// dropped it) opens nothing, and a merge with a copy that kept it puts
    /// the binding back.
    #[test]
    fn a_tenant_key_is_a_key_of_no_table_bound_on_the_opensearch_api() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Keys::open(dir.path().join("keys.json")).unwrap();
        let t = Tenant::of(&json!({"index": "tickets", "field": "tenant_id", "value": "acme"})).unwrap();
        let bound = |t: &Tenant| AgentKey { allow: Some(Allow { tenant: Some(t.clone()), ..Default::default() }), ..Default::default() };
        assert!(keys.create_full("x", Scope::Admin, None, None, None, None, bound(&t), true, false).is_err());
        assert!(keys.create_full("x", Scope::Write, Some(vec![Grant::parse("a=read").unwrap()]), None, None, None, bound(&t), true, false).is_err());
        let (k, secret) = keys.create_full("tenant-acme", Scope::Write, None, None, None, None, bound(&t), true, false).unwrap();
        assert_eq!((k.tables.as_deref(), k.sha256.starts_with(LIMITED)), (Some(&[][..]), true));
        let p = keys.resolve(None, None, Some(&format!("Bearer {secret}"))).unwrap();
        assert_eq!((p.tenant(), p.scope, p.table_scope("os_tickets"), p.reads_all()), (Some(&t), Scope::Write, None, false));
        assert_eq!(p.json()["tenant"]["value"], "acme");
        // Narrowed, it is bound no more: a key of no table.
        assert!(p.narrowed("*=read").unwrap().tenant().is_none());
        // Without the binding: nothing.
        let mut lost = k.clone();
        lost.allow.as_mut().unwrap().tenant = None;
        assert!(lost.principal().is_none());
        // Merged with a copy that kept it: bound again.
        let older = KeySet { keys: vec![lost], revoked: vec![] };
        let (merged, changed) = older.merged(&KeySet { keys: vec![k.clone()], revoked: vec![] });
        assert!(changed && merged.keys[0].principal().is_some_and(|p| p.tenant() == Some(&t)));
        // Bad bindings.
        for bad in [json!({"index": "t*", "field": "f", "value": "v"}), json!({"index": "_all", "field": "f", "value": "v"}), json!({"index": "t", "field": "_id", "value": "v"}), json!({"index": "t", "field": "f", "value": ""}), json!({"indices": [], "field": "f", "value": "v"})] {
            assert!(Tenant::of(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_rotated_key_lives_as_long_as_the_old_one_did_from_now() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Keys::open(dir.path().join("keys.json")).unwrap();
        keys.create("ops", Scope::Admin, None, true, false).unwrap();
        let day = 86_400_000;
        let (old, _) = keys.create("ci", Scope::Write, Some(now_ms() + 30 * day), true, false).unwrap();
        let (new, secret, ending) = keys.rotate(&old.id, 15 * 60_000, true).unwrap();
        let lifetime = new.expires_ms.unwrap() - new.created_ms;
        assert!(lifetime.abs_diff(old.expires_ms.unwrap() - old.created_ms) < 1_000, "{lifetime}");
        assert!(ending.expires_ms.unwrap() <= now_ms() + 15 * 60_000);
        assert_eq!((new.name.as_str(), new.scope), ("ci", Scope::Write));
        assert!(secret.starts_with(&format!("sk_{}_", new.id)));
        // A key that never expired: its successor never does either.
        let (forever, _) = keys.create("dash", Scope::Read, None, true, false).unwrap();
        assert_eq!(keys.rotate(&forever.id, 0, true).unwrap().0.expires_ms, None);
    }

    #[test]
    fn keys_resolve_to_their_scope_and_revoke() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Keys::open(dir.path().join("keys.json")).unwrap();
        // Open: anyone; the first key of an open server is an admin key.
        assert_eq!(keys.resolve(None, None, None), Some(Principal::open()));
        assert!(keys.create("reader", Scope::Read, None, false, false).is_err());
        let (admin, admin_secret) = keys.create("ops", Scope::Admin, None, false, false).unwrap();
        let (reader, secret) = keys.create("reader", Scope::Read, None, false, false).unwrap();
        assert_eq!(keys.create("reader", Scope::Read, None, false, false).unwrap_err().status, 409);
        // Closed now.
        assert_eq!(keys.resolve(None, None, None), None);
        let p = keys.resolve(None, None, Some(&format!("Bearer {secret}"))).unwrap();
        assert_eq!((p.name.as_str(), p.scope, p.root), ("reader", Scope::Read, false));
        assert!(p.may(Scope::Read, "a query").is_ok());
        assert_eq!(p.may(Scope::Write, "a write").unwrap_err().status, 403);
        assert_eq!(keys.resolve(None, None, Some(&format!("Bearer {admin_secret}"))).unwrap().scope, Scope::Admin);
        // A wrong secret for a real id, and the token.
        assert_eq!(keys.resolve(None, None, Some(&format!("Bearer sk_{}_00", reader.id))), None);
        let token = Secret::new("t0k");
        assert!(keys.resolve(Some(&token), None, Some("Bearer t0k")).unwrap().root);
        // Kept in the file, without the secret.
        let text = std::fs::read_to_string(dir.path().join("keys.json")).unwrap();
        assert!(!text.contains(secret.rsplit_once('_').unwrap().1) && text.contains(&reader.sha256));
        let again = Keys::open(dir.path().join("keys.json")).unwrap();
        assert_eq!(again.resolve(None, None, Some(&format!("Bearer {secret}"))).unwrap().name, "reader");
        assert!(again.revoke(&reader.id, false).unwrap());
        assert!(!again.revoke(&reader.id, false).unwrap());
        // The last admin key stays, but for a server with a token.
        assert_eq!(again.revoke(&admin.id, false).unwrap_err().status, 409);
        assert_eq!(again.resolve(None, None, Some(&format!("Bearer {secret}"))), None);
        assert_eq!(again.list().len(), 1);
        assert_eq!(again.list()[0]["id"], admin.id);
        // Expired.
        let (_, old) = again.create("old", Scope::Read, Some(now_ms() - 1), false, false).unwrap();
        assert_eq!(again.resolve(None, None, Some(&format!("Bearer {old}"))), None);
    }

    /// A server without a token keeps an admin key that never expires: its
    /// last admin key expiring would lock everyone out (BUG-N5).
    #[test]
    fn a_server_without_a_token_keeps_an_admin_key_that_never_expires() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Keys::open(dir.path().join("keys.json")).unwrap();
        let soon = Some(now_ms() + 60_000);
        // Its first admin key may not expire...
        let e = keys.create("ops", Scope::Admin, soon, false, false).unwrap_err();
        assert!(e.status == 400 && e.message.contains("never expire"), "{}", e.message);
        assert!(keys.list().is_empty() && !dir.path().join("keys.json").exists());
        let (forever, _) = keys.create("ops", Scope::Admin, None, false, false).unwrap();
        assert!(keys.managed());
        // ...others may, once one does not.
        let (brief, _) = keys.create("ops-brief", Scope::Admin, soon, false, false).unwrap();
        keys.create("reader", Scope::Read, soon, false, false).unwrap();
        // The one that never expires is not revoked while it is the only
        // one; the expiring one is.
        let e = keys.revoke(&forever.id, false).unwrap_err();
        assert!(e.status == 409 && e.message.contains("never expires"), "{}", e.message);
        assert!(keys.revoke(&brief.id, false).unwrap());
        // With a token, the token manages the server: keys may all expire.
        let with = Keys::open(dir.path().join("with-token.json")).unwrap();
        let (only, _) = with.create("ops", Scope::Admin, soon, true, false).unwrap();
        assert!(with.revoke(&only.id, true).unwrap());
    }

    /// The cluster token is the root's, and the token then an admin's;
    /// without a cluster token, the token is the root's.
    #[test]
    fn a_cluster_token_is_the_root_and_the_token_an_admin() {
        let keys = Keys::memory();
        let (t, c) = (Secret::new("admin-t"), Secret::new("cluster-c"));
        let who = |token, cluster, auth: &str| keys.resolve(token, cluster, Some(auth));
        assert_eq!(who(Some(&t), Some(&c), "Bearer cluster-c"), Some(Principal::cluster()));
        let p = who(Some(&t), Some(&c), "Bearer admin-t").unwrap();
        assert!(p.scope == Scope::Admin && !p.root);
        assert!(who(Some(&t), None, "Bearer admin-t").unwrap().root);
        assert_eq!(who(None, Some(&c), "Bearer admin-t"), None);
        // A cluster token alone closes a server.
        assert_eq!(keys.resolve(None, Some(&c), None), None);
        assert!(Principal::open().is_open() && !Principal::cluster().is_open());
        // A server of a cluster with neither makes no key.
        let e = keys.create("ops", Scope::Admin, None, false, true).unwrap_err();
        assert!(e.status == 409 && e.message.contains("--cluster-token"), "{}", e.message);
        assert!(keys.create("ops", Scope::Admin, None, true, true).is_ok());
    }

    /// Merging keeps every key and every revocation, which wins; merging
    /// again changes nothing, and the order of merges does not matter.
    #[test]
    fn key_sets_merge_and_revocations_win() {
        let dir = tempfile::tempdir().unwrap();
        let a = Keys::open(dir.path().join("a.json")).unwrap();
        let b = Keys::memory();
        let (ka, sa) = a.create("from-a", Scope::Read, None, true, false).unwrap();
        let (kb, sb) = b.create("from-b", Scope::Write, None, true, false).unwrap();
        let (gone, sg) = b.create("gone", Scope::Read, None, true, false).unwrap();
        let admits = |k: &Keys, s: &str| k.resolve(None, None, Some(&format!("Bearer {s}"))).is_some();
        assert!(a.merge(&b.set()).unwrap());
        assert!(!a.merge(&b.set()).unwrap());
        assert!(b.merge(&a.set()).unwrap());
        for k in [&a, &b] {
            assert!(admits(k, &sa) && admits(k, &sb) && admits(k, &sg));
        }
        // Revoked on b; a, which still holds the key, does not bring it back.
        assert!(b.revoke(&gone.id, true).unwrap());
        assert!(!b.merge(&a.set()).unwrap());
        assert!(!admits(&b, &sg));
        assert!(a.merge(&b.set()).unwrap());
        assert!(!admits(&a, &sg) && admits(&a, &sa));
        assert_eq!(a.list().len(), 2);
        // Kept in a's file, the revocation too.
        let again = Keys::open(dir.path().join("a.json")).unwrap();
        assert_eq!(again.set().revoked, b.set().revoked);
        assert!(admits(&again, &sb) && !admits(&again, &sg));
        // A keys file from before revocations were kept: a list of keys.
        std::fs::write(dir.path().join("old.json"), serde_json::to_vec(&vec![ka]).unwrap()).unwrap();
        assert!(admits(&Keys::open(dir.path().join("old.json")).unwrap(), &sa));
        // One id with two secrets: every server keeps the same one.
        let other = Key { sha256: "0".repeat(64), ..kb.clone() };
        let x = KeySet { keys: vec![other], revoked: vec![] };
        let y = KeySet { keys: vec![kb], revoked: vec![] };
        assert_eq!(x.merged(&y).0.keys[0].sha256, "0".repeat(64));
        assert_eq!(y.merged(&x).0.keys[0].sha256, "0".repeat(64));
    }

    /// D-B1: a key rotated on one server, its old secret ending at once,
    /// ends on every server it merges with, and on a coordinator that
    /// starts again with no keys file and takes them back from its shards;
    /// a server that still holds the key unexpired does not revive it.
    #[test]
    fn a_rotated_keys_end_reaches_every_server() {
        let coordinator = Keys::memory();
        let shard = Keys::memory();
        coordinator.create("ops", Scope::Admin, None, true, false).unwrap();
        let (leaked, secret) = coordinator.create("ci", Scope::Write, None, true, false).unwrap();
        let admits = |k: &Keys| k.resolve(None, None, Some(&format!("Bearer {secret}"))).is_some();
        assert!(shard.merge(&coordinator.set()).unwrap());
        assert!(admits(&shard));
        coordinator.rotate(&leaked.id, 0, true).unwrap();
        assert!(!admits(&coordinator));
        // The shard takes the end, and says its keys changed.
        assert!(shard.merge(&coordinator.set()).unwrap());
        assert!(!admits(&shard), "the shard still takes the rotated key");
        // Merged back, the shard's copy does not lift it.
        assert!(!coordinator.merge(&shard.set()).unwrap());
        assert!(!admits(&coordinator));
        // A coordinator that starts again takes its keys from the shard.
        let again = Keys::memory();
        again.merge(&shard.set()).unwrap();
        assert!(!admits(&again), "a restarted coordinator revived the rotated key");
        // The earlier end wins whichever side holds it.
        let late = Key { expires_ms: Some(now_ms() + 86_400_000), ..leaked.clone() };
        let early = Key { expires_ms: Some(now_ms() - 1), ..leaked.clone() };
        for (a, b) in [(&late, &early), (&early, &late)] {
            let x = KeySet { keys: vec![a.clone()], revoked: vec![] };
            let y = KeySet { keys: vec![b.clone()], revoked: vec![] };
            assert_eq!(x.merged(&y).0.keys[0].expires_ms, early.expires_ms);
        }
        let forever = KeySet { keys: vec![leaked.clone()], revoked: vec![] };
        let ending = KeySet { keys: vec![late.clone()], revoked: vec![] };
        let (merged, changed) = forever.merged(&ending);
        assert_eq!((merged.keys[0].expires_ms, changed), (late.expires_ms, true));
        let (merged, changed) = ending.merged(&forever);
        assert_eq!((merged.keys[0].expires_ms, changed), (late.expires_ms, false));
    }

    /// A key limited to tables: its scope is the most its grants give, its
    /// digest is marked so that a server from before limits never takes
    /// it, and a copy that lost its tables (written back by such a server)
    /// opens nothing, and gets them back when merged.
    #[test]
    fn a_key_limited_to_tables_keeps_its_limits() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Keys::open(dir.path().join("keys.json")).unwrap();
        let grants = vec![Grant::parse("crawl_*=admin").unwrap(), Grant::parse("Docs=read").unwrap()];
        // Not a server's first key: it would not manage the server.
        assert_eq!(keys.create_limited("crawler", Scope::Admin, Some(grants.clone()), None, None, None, false, false).unwrap_err().status, 400);
        keys.create("ops", Scope::Admin, None, false, false).unwrap();
        let (k, secret) = keys.create_limited("crawler", Scope::Admin, Some(grants.clone()), None, None, None, false, false).unwrap();
        assert!(k.sha256.starts_with(LIMITED) && k.scope == Scope::Admin);
        let p = keys.resolve(None, None, Some(&format!("Bearer {secret}"))).unwrap();
        assert!(p.limited() && !p.root);
        assert_eq!((p.table_scope("crawl_x"), p.table_scope("CRAWL_X"), p.table_scope("docs"), p.table_scope("secrets")), (Some(Scope::Admin), Some(Scope::Admin), Some(Scope::Read), None));
        assert!(p.may_table(Scope::Write, "docs", "a write").unwrap_err().message.contains("read there"));
        assert!(p.may_server("vacuum").is_err());
        assert_eq!(p.json()["tables"], json!({"crawl_*": "admin", "docs": "read"}));
        // It does not count as the key that manages a server without a token.
        let (only, _) = Keys::memory().create("ops", Scope::Admin, None, true, false).unwrap();
        assert!(!managed(&[Key { tables: Some(grants.clone()), ..only.clone() }]) && managed(&[only]));
        // A scope given caps it.
        let (capped, _) = keys.create_limited("reader", Scope::Read, Some(grants.clone()), None, None, None, false, false).unwrap();
        assert_eq!(capped.scope, Scope::Read);
        // Rotated, the new key is limited and marked too.
        let (new, new_secret, _) = keys.rotate(&k.id, 0, false).unwrap();
        assert!(new.sha256.starts_with(LIMITED) && new.tables == Some(grants.clone()));
        assert!(keys.resolve(None, None, Some(&format!("Bearer {new_secret}"))).unwrap().limited());
        // A server from before limits: its digest is never the secret's.
        let digest = sha256(new_secret.rsplit_once('_').unwrap().1);
        assert!(!same_secret(digest.as_bytes(), new.sha256.as_bytes()));
        // ...and it writes the key back without its tables.
        let stripped = Key { tables: None, ..new.clone() };
        let old = Keys::memory();
        old.merge(&KeySet { keys: vec![stripped.clone()], revoked: vec![] }).unwrap();
        assert_eq!(old.resolve(None, None, Some(&format!("Bearer {new_secret}"))), None);
        // Merged with a copy that has them, the limits come back.
        assert!(old.merge(&keys.set()).unwrap());
        assert!(old.resolve(None, None, Some(&format!("Bearer {new_secret}"))).unwrap().limited());
        let (merged, _) = keys.set().merged(&KeySet { keys: vec![stripped], revoked: vec![] });
        assert_eq!(merged.keys.iter().find(|x| x.id == new.id).unwrap().tables, Some(grants));
        // Bad grants.
        for bad in ["", "a.b", "*x", "crawl_*=feed", "t=root"] {
            assert!(Grant::parse(bad).is_err(), "{bad}");
        }
        assert!(grants_of(&json!({})).is_err() && grants_of(&json!("a=read, b_*=write")).unwrap().len() == 2);
        assert!(keys.create_limited("app:x", Scope::Read, None, None, None, None, false, false).is_err());
    }

    /// An app's key: in memory only, limited, replaced at the app's next
    /// start, and no reason to close an open server.
    #[test]
    fn a_key_makes_keys_within_its_tables_that_end_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Keys::open(dir.path().join("keys.json")).unwrap();
        let g = |s: &str| Grant::parse(s).unwrap();
        let bearer = |s: &str| format!("Bearer {s}");
        let (engine, engine_secret) = keys.create_limited("apps-app", Scope::Admin, Some(vec![g("sa_*=admin"), g("app_*=admin"), g("other=read")]), None, None, None, true, false).unwrap();
        let by = keys.resolve(None, None, Some(&bearer(&engine_secret))).unwrap();
        // Within its admin grants: a prefix within a prefix, a name within one.
        let (child, secret) = keys.create_delegated(&by, "app-abc123", vec![g("app_abc123_*=write"), g("sa_numbers=read")], None, false).unwrap();
        assert_eq!((child.name.as_str(), child.parent.as_deref(), child.owner.as_deref()), ("apps-app/app-abc123", Some(engine.id.as_str()), Some("apps-app")));
        assert!(child.sha256.starts_with(DELEGATED));
        let p = keys.resolve(None, None, Some(&bearer(&secret))).unwrap();
        assert_eq!(p.table_scope("app_abc123_tasks"), Some(Scope::Write));
        assert_eq!(p.table_scope("app_other1_tasks"), None);
        assert_eq!(p.table_scope("sa_people"), None);
        // Not beyond them: another prefix, more scope, a grant it reads only.
        for bad in [vec![g("*=read")], vec![g("ap*=write")], vec![g("other=read")], vec![g("crawl_x=read")]] {
            assert_eq!(keys.create_delegated(&by, "x", bad.clone(), None, false).unwrap_err().status, 403, "{bad:?}");
        }
        // A key it made makes none; nor does the token or an open server.
        assert_eq!(keys.create_delegated(&p, "y", vec![g("app_abc123_x=read")], None, false).unwrap_err().status, 403);
        assert_eq!(keys.create_delegated(&Principal::open(), "y", vec![g("app_x=read")], None, false).unwrap_err().status, 403);
        // Again by the same name: a new secret, the old one revoked.
        let (again, secret2) = keys.create_delegated(&by, "app-abc123", vec![g("app_abc123_*=write")], None, false).unwrap();
        assert_ne!(again.id, child.id);
        assert_eq!(keys.resolve(None, None, Some(&bearer(&secret))), None);
        assert!(keys.resolve(None, None, Some(&bearer(&secret2))).is_some());
        assert_eq!(keys.delegated(&engine.id).len(), 1);
        // Kept in the file with its parent; then it ends with its maker.
        let back = Keys::open(dir.path().join("keys.json")).unwrap();
        assert!(back.resolve(None, None, Some(&bearer(&secret2))).is_some());
        assert!(back.revoke(&engine.id, true).unwrap());
        assert_eq!(back.resolve(Some(&Secret::new("t")), None, Some(&bearer(&secret2))), None);
        assert!(back.revoked().iter().any(|r| r["id"] == again.id.as_str()));
        // An unlimited admin key makes keys of any tables; a read key none.
        let (_, admin) = keys.create("ops", Scope::Admin, None, true, false).unwrap();
        let ops = keys.resolve(None, None, Some(&bearer(&admin))).unwrap();
        assert!(keys.create_delegated(&ops, "dash", vec![g("anything=read")], None, false).is_ok());
        let (_, read) = keys.create("reader", Scope::Read, None, true, false).unwrap();
        let reader = keys.resolve(None, None, Some(&bearer(&read))).unwrap();
        assert_eq!(keys.create_delegated(&reader, "z", vec![g("anything=read")], None, false).unwrap_err().status, 403);
    }

    #[test]
    fn a_key_made_by_a_key_needs_its_maker_wherever_it_is_merged() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Keys::open(dir.path().join("keys.json")).unwrap();
        let (_, maker) = keys.create("ops", Scope::Admin, None, true, false).unwrap();
        let by = keys.resolve(None, None, Some(&format!("Bearer {maker}"))).unwrap();
        let (child, secret) = keys.create_delegated(&by, "app", vec![Grant::parse("app_*=write").unwrap()], None, false).unwrap();
        // A server from before such keys writes it back without its parent:
        // it opens nothing there (its mark), and the parent comes back here.
        let mut old = keys.set();
        for k in old.keys.iter_mut() {
            k.parent = None;
        }
        let other = Keys::memory();
        other.merge(&old).unwrap();
        assert_eq!(other.resolve(Some(&Secret::new("t")), None, Some(&format!("Bearer {secret}"))), None);
        keys.merge(&old).unwrap();
        assert!(keys.resolve(None, None, Some(&format!("Bearer {secret}"))).is_some());
        // A set that has the child but not its maker opens nothing.
        let alone = Keys::memory();
        alone.merge(&KeySet { keys: keys.set().keys.into_iter().filter(|k| k.id == child.id).collect(), revoked: vec![] }).unwrap();
        assert_eq!(alone.resolve(Some(&Secret::new("t")), None, Some(&format!("Bearer {secret}"))), None);
    }

    #[test]
    fn an_apps_key_is_kept_in_memory() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Keys::open(dir.path().join("keys.json")).unwrap();
        let secret = keys.app_key("sheets", vec![Grant::parse("sheets_*=admin").unwrap(), Grant::parse("*=read").unwrap()]).unwrap();
        let p = keys.resolve(None, None, Some(&format!("Bearer {secret}"))).unwrap();
        assert_eq!((p.name.as_str(), p.scope, p.table_scope("orders")), ("app:sheets", Scope::Admin, Some(Scope::Read)));
        // Still open to anyone else; nothing written, nothing to sync.
        assert_eq!(keys.resolve(None, None, None), Some(Principal::open()));
        assert!(keys.is_empty() && keys.set().keys.is_empty() && !dir.path().join("keys.json").exists());
        assert_eq!(keys.list()[0]["ephemeral"], true);
        // On a closed server too.
        let token = Secret::new("t");
        assert!(keys.resolve(Some(&token), None, Some(&format!("Bearer {secret}"))).unwrap().limited());
        // The next start's key ends this one; a revoke ends it as well.
        let next = keys.app_key("sheets", vec![Grant::parse("sheets_*=write").unwrap()]).unwrap();
        assert_eq!(keys.resolve(Some(&token), None, Some(&format!("Bearer {secret}"))), None);
        let id = keys.list()[0]["id"].as_str().unwrap().to_string();
        assert!(keys.revoke(&id, true).unwrap());
        assert_eq!(keys.resolve(Some(&token), None, Some(&format!("Bearer {next}"))), None);
        assert!(keys.revoked().is_empty());
        assert!(keys.app_key("Bad Name", vec![Grant::parse("a=read").unwrap()]).is_err());
        assert!(keys.app_key("x", vec![]).is_err());
    }

    #[test]
    fn the_audit_log_appends_to_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let a = Audit::new(Some(dir.path().join("audit.jsonl"))).unwrap();
        let e = |status| Entry { ts_ms: 1, who: "ops".into(), key: None, scope: Some("admin"), method: "POST".into(), route: "/v1/admin/vacuum".into(), status, ms: 1.0, request_id: None, detail: None, ..Default::default() };
        a.add(e(200));
        a.add(e(403));
        assert_eq!(a.latest(10)[0].status, 403);
        let text = std::fs::read_to_string(dir.path().join("audit.jsonl")).unwrap();
        assert_eq!(text.lines().count(), 2);
    }
}
