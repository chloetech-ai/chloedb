//! What a key limited to tables may do ([`crate::keys::Grant`]): which
//! routes it may call at all, and which tables a statement it sends
//! touches.
//!
//! Such a key is for an app beside the cluster (the crawler, the
//! spreadsheets, ChloeCtx, the app engine): it reads and writes its own
//! tables, perhaps reads others, and does nothing else. So its routes are
//! a list ([`route_ok`]), every other route is refused whatever its grants,
//! and each route on the list checks the tables it touches:
//!
//! - a statement ([`check_statement`]) by its kind (a query reads, an
//!   `INSERT`, `UPDATE` or `DELETE` writes its table, `CREATE`, `ALTER` and
//!   `DROP` need admin on theirs) and by every table it names, read
//!   wherever it is named: `FROM` and `JOIN`, subqueries, `WITH`, the
//!   source of an `INSERT ... SELECT`, an edge table a walk names;
//! - a write, a read by key, an import, a table's schema: by the table
//!   they name.
//!
//! The statement is parsed by ChloeDB's own parser
//! ([`chloedb::query::parse::parse`]), so that it is read here as the
//! engine reads it. What cannot be checked by its names is refused rather
//! than allowed: a table function (a stored function's body names tables
//! the text does not), a `WITH` table named as a table is, a name in parts
//! (`a.b`), a call of `llm()` (its second step is SQL a model writes) or of
//! `encode()` (a sequence model reads its own table's vectors), and a
//! stored function when one of them calls `llm()`.

use std::collections::BTreeSet;
use std::ops::ControlFlow;

use axum::http::Method;
use chloedb::query::function::FunctionSet;
use chloedb::query::sql::Command;
use sqlparser::ast::{self as sql, Visit, Visitor};

use crate::keys::{Principal, Scope};
use crate::ApiError;

/// Whether a key limited to tables may call `route` with `method` at all.
/// Each of these checks the tables it touches; the others are refused.
pub fn route_ok(method: &Method, route: &str) -> bool {
    let get = method == Method::GET || method == Method::HEAD;
    match route {
        // Whether the server is up, who the key is, and the cluster's shape.
        "/v1/health" | "/v1/whoami" | "/v1/cluster" => get,
        // The alerts that fire (ChloeCtx's "Seen before").
        "/v1/alerts" => get,
        // Tables: the listing and schemas show the tables it may read.
        "/v1/tables" | "/v1/schema" | "/v1/tables/:table/paths" => get,
        // Statements, by their tables.
        "/v1/query" | "/v1/sql" | "/v1/explain" => method == Method::POST,
        "/v1/query/cursors/:id" => method == Method::DELETE,
        // Rows, by their tables.
        "/v1/write" | "/v1/kv/get" | "/v1/import" => method == Method::POST,
        "/v1/kv/:table/:key" => get || method == Method::PUT || method == Method::DELETE,
        // Embeddings of text, and the models that make them: no table.
        "/v1/embed" => method == Method::POST,
        "/v1/models" => get,
        // The command center's feedback, by its table ([`crate::feedback::may`]).
        "/v1/feedback" => get || method == Method::POST,
        "/v1/feedback/:id" => get || method == Method::PATCH || method == Method::DELETE,
        "/v1/feedback/:id/shot" | "/v1/feedback/:id/thumb" => get,
        _ => false,
    }
}

/// Refuses (403) statement `text`, bound as `command`, if `who` (a key
/// limited to tables) may not run it: its kind's scope on the table it
/// changes, and read on every table it names. `tables` are the catalog's
/// tables, `functions` its stored functions. Anyone else may: their scope
/// was checked by the statement's kind.
pub fn check_statement(who: &Principal, text: &str, command: &Command, tables: &[String], functions: &FunctionSet) -> Result<(), ApiError> {
    if !who.limited() {
        return Ok(());
    }
    match command {
        Command::Query | Command::Explain { .. } => check_reads(who, text, tables, functions),
        Command::Write { table, .. } => {
            who.may_table(Scope::Write, table, "a write")?;
            check_reads(who, text, tables, functions)
        }
        Command::CreateTable { schema, .. } => {
            who.may_table(Scope::Admin, &schema.table, "CREATE TABLE")?;
            reads(who, text, tables, functions, std::slice::from_ref(&schema.table))
        }
        Command::DropTable { tables: dropped, .. } => {
            for t in dropped {
                who.may_table(Scope::Admin, t, "DROP TABLE")?;
            }
            reads(who, text, tables, functions, dropped)
        }
        Command::AlterTable { table, .. } => {
            if let Some(t) = table {
                who.may_table(Scope::Admin, t, "ALTER TABLE")?;
            }
            check_reads(who, text, tables, functions)
        }
        Command::CreateRankProfile { table, .. } => who.may_table(Scope::Admin, table, "CREATE RANK PROFILE"),
        Command::DropRankProfile { table: Some(t), .. } => who.may_table(Scope::Admin, t, "DROP RANK PROFILE"),
        Command::DropRankProfile { table: None, .. } => Err(ApiError { status: 403, message: "a key limited to tables names the table of the rank profile it drops: DROP RANK PROFILE name ON table".into() }),
        Command::Function(_) => who.may_server("a stored function (CREATE or DROP FUNCTION)"),
    }
}

/// Refuses (403) a query or statement `text` that names a table `who` may
/// not read, or that reads tables its names do not show ([`self`]).
pub fn check_reads(who: &Principal, text: &str, tables: &[String], functions: &FunctionSet) -> Result<(), ApiError> {
    reads(who, text, tables, functions, &[])
}

/// [`check_reads`], but for the tables `made` a statement creates or
/// drops, checked by their statement's kind.
fn reads(who: &Principal, text: &str, tables: &[String], functions: &FunctionSet, made: &[String]) -> Result<(), ApiError> {
    if !who.limited() {
        return Ok(());
    }
    let refuse = |m: String| ApiError { status: 403, message: format!("{m}; the key {:?} is limited to tables", who.name) };
    let statements = chloedb::query::parse::parse(text).map_err(|e| ApiError::bad(e.to_string()))?;
    let mut walk = Walk::default();
    for s in &statements {
        let _ = s.visit(&mut walk);
    }
    if let Some(m) = walk.refused {
        return Err(refuse(m));
    }
    let catalog: BTreeSet<String> = tables.iter().map(|t| t.to_ascii_lowercase()).collect();
    if let Some(c) = walk.ctes.iter().find(|c| catalog.contains(*c)) {
        return Err(refuse(format!("WITH {c} names a table too: name it otherwise")));
    }
    for r in &walk.relations {
        if walk.ctes.contains(r) || made.iter().any(|m| m.eq_ignore_ascii_case(r)) {
            continue;
        }
        who.may_table(Scope::Read, r, "a query")?;
        // A name its grants cover that is not a table: as the engine says it
        // (a client tells a missing table by these words), but without the
        // engine's list of the database's tables, which names tables the
        // key may not see. It is refused rather than run, since a name of
        // no table may be a table function's, whose body names tables its
        // call does not.
        if !catalog.contains(r) {
            return Err(ApiError::bad(format!("no such table: {r}")));
        }
    }
    for a in walk.arguments.iter().filter(|a| catalog.contains(*a)) {
        who.may_table(Scope::Read, a, "a walk over an edge table")?;
    }
    for c in &walk.calls {
        let base = c.split('@').next().unwrap_or(c);
        if base == chloedb::query::llm::NAME {
            return Err(refuse("a query that calls llm() runs SQL a model writes, which no key limited to tables may".into()));
        }
        if base == "encode" {
            return Err(refuse("encode() reads a sequence model's own table".into()));
        }
        if functions.calls_llm() && functions.current(base).is_some() {
            return Err(refuse(format!("{base}() is a stored function, and this database's functions call llm()")));
        }
    }
    Ok(())
}

/// Whether statement `text` calls a function `functions` holds as a
/// stored one: a coordinator learns its functions again before judging
/// such a call ([`check_reads`]), since another coordinator may have
/// changed its body (audit N-D5).
pub fn calls_stored(text: &str, functions: &FunctionSet) -> bool {
    let Ok(statements) = chloedb::query::parse::parse(text) else { return false };
    let mut walk = Walk::default();
    for s in &statements {
        let _ = s.visit(&mut walk);
    }
    walk.calls.iter().any(|c| functions.current(c.split('@').next().unwrap_or(c)).is_some())
}

/// What a statement names, as [`check_reads`] needs it.
#[derive(Default)]
struct Walk {
    /// Every table a `FROM`, `JOIN`, `INSERT`, `UPDATE`, `DELETE` or DDL
    /// names, in lowercase.
    relations: BTreeSet<String>,
    /// The `WITH` tables the statement defines.
    ctes: BTreeSet<String>,
    /// The functions it calls.
    calls: BTreeSet<String>,
    /// Names given as a call's arguments (an edge table a walk follows).
    arguments: BTreeSet<String>,
    /// Why it cannot be checked by its names.
    refused: Option<String>,
}

/// One name of a single part, in lowercase; `None` for `a.b`.
fn one_name(n: &sql::ObjectName) -> Option<String> {
    match n.0.as_slice() {
        [p] => p.as_ident().map(|i| i.value.to_ascii_lowercase()),
        _ => None,
    }
}

impl Visitor for Walk {
    type Break = ();

    fn pre_visit_query(&mut self, q: &sql::Query) -> ControlFlow<()> {
        if let Some(with) = &q.with {
            for c in &with.cte_tables {
                self.ctes.insert(c.alias.name.value.to_ascii_lowercase());
            }
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_relation(&mut self, r: &sql::ObjectName) -> ControlFlow<()> {
        match one_name(r) {
            Some(n) => drop(self.relations.insert(n)),
            None => self.refused = Some(format!("{r} is a name in parts: name a table by its name alone")),
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, t: &sql::TableFactor) -> ControlFlow<()> {
        match t {
            sql::TableFactor::Table { args: None, .. } | sql::TableFactor::Derived { .. } | sql::TableFactor::NestedJoin { .. } => {}
            sql::TableFactor::Table { name, .. } => self.refused = Some(format!("{name}(...) is a table function")),
            other => self.refused = Some(format!("{other} reads rows a key limited to tables cannot be checked for")),
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, e: &sql::Expr) -> ControlFlow<()> {
        if let sql::Expr::Function(f) = e {
            let name = f.name.0.iter().filter_map(|p| p.as_ident()).map(|i| i.value.to_ascii_lowercase()).collect::<Vec<_>>().join(".");
            self.calls.insert(name);
            if let sql::FunctionArguments::List(list) = &f.args {
                for a in &list.args {
                    let e = match a {
                        sql::FunctionArg::Unnamed(e) | sql::FunctionArg::Named { arg: e, .. } | sql::FunctionArg::ExprNamed { arg: e, .. } => e,
                    };
                    if let sql::FunctionArgExpr::Expr(sql::Expr::Identifier(i)) = e {
                        self.arguments.insert(i.value.to_ascii_lowercase());
                    }
                }
            }
        }
        ControlFlow::Continue(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::Grant;

    fn key(grants: &[&str]) -> Principal {
        let g: Vec<Grant> = grants.iter().map(|g| Grant::parse(g).unwrap()).collect();
        let scope = g.iter().map(|g| g.scope).max().unwrap();
        Principal { name: "app:test".into(), key: Some("k".into()), scope, root: false, tables: Some(g.into()) }
    }

    fn tables() -> Vec<String> {
        ["crawl_docs", "crawl_links", "secrets", "sa_people", "edges_secret"].map(String::from).to_vec()
    }

    fn reads(who: &Principal, sql: &str) -> Result<(), ApiError> {
        check_reads(who, sql, &tables(), &FunctionSet::default())
    }

    #[test]
    fn a_limited_key_reads_only_the_tables_it_names() {
        let k = key(&["crawl_*=admin"]);
        assert!(reads(&k, "SELECT * FROM crawl_docs WHERE id = 1").is_ok());
        assert!(reads(&k, "SELECT d.id FROM crawl_docs d JOIN crawl_links l ON l.src = d.id").is_ok());
        assert!(reads(&k, "SELECT * FROM CRAWL_DOCS").is_ok());
        assert!(reads(&k, "WITH x AS (SELECT * FROM crawl_docs) SELECT * FROM x").is_ok());
        // A missing table its grants cover is missing, as for any key, and
        // the answer lists no other table; one they do not cover is refused
        // whether or not it exists.
        let missing = reads(&k, "SELECT * FROM crawl_pages").unwrap_err();
        assert_eq!((missing.status, missing.message.as_str()), (400, "no such table: crawl_pages"));
        assert_eq!(reads(&k, "SELECT * FROM nowhere").unwrap_err().status, 403);
        for bad in [
            "SELECT * FROM secrets",
            "SELECT * FROM \"secrets\"",
            "SELECT * FROM Secrets",
            "SELECT * FROM crawl_docs, secrets",
            "SELECT * FROM crawl_docs JOIN secrets ON true",
            "SELECT * FROM crawl_docs WHERE id IN (SELECT id FROM secrets)",
            "SELECT (SELECT max(id) FROM secrets) FROM crawl_docs",
            "SELECT * FROM crawl_docs WHERE EXISTS (SELECT 1 FROM secrets)",
            "WITH x AS (SELECT * FROM secrets) SELECT * FROM x",
            "SELECT * FROM (SELECT * FROM secrets) s",
            "SELECT * FROM crawl_docs UNION ALL SELECT * FROM secrets",
            "SELECT * FROM main.crawl_docs",
            // A WITH table of a table's name could shadow it in one scope
            // and not in another.
            "SELECT * FROM secrets, (WITH secrets AS (SELECT 1) SELECT * FROM secrets) s",
            "WITH crawl_docs AS (SELECT 1) SELECT * FROM crawl_docs",
            // Table functions, llm(), encode().
            "SELECT * FROM recall('q')",
            "SELECT * FROM crawl_docs(1)",
            "SELECT llm('say', body) FROM crawl_docs",
            "SELECT encode(m, items) FROM crawl_docs",
            // An edge table a walk names.
            "SELECT id FROM crawl_docs WHERE REACHABLE(edges_secret, 1)",
        ] {
            let e = reads(&k, bad).expect_err(bad);
            assert_eq!(e.status, 403, "{bad}: {}", e.message);
        }
        // Not a table at all: refused too (it might be a function).
        assert_eq!(reads(&k, "SELECT * FROM nothing_here").unwrap_err().status, 403);
        // A key of every table is not checked here.
        let all = Principal { name: "ops".into(), key: None, scope: Scope::Admin, root: false, tables: None };
        assert!(reads(&all, "SELECT * FROM secrets").is_ok());
    }

    /// The schema of a table `name (id KEYWORD PRIMARY KEY)`.
    fn schema(name: &str) -> chloedb::schema::Schema {
        match chloedb::query::sql::bind_command(&format!("CREATE TABLE {name} (id KEYWORD PRIMARY KEY, body TEXT)"), &[], &Default::default()).unwrap() {
            Command::CreateTable { schema, .. } => schema,
            _ => unreachable!(),
        }
    }

    #[test]
    fn read_grants_and_write_grants() {
        let k = key(&["sheets_*=admin", "*=read"]);
        assert!(reads(&k, "SELECT * FROM secrets JOIN crawl_docs ON true").is_ok());
        let functions = FunctionSet::default();
        let (secrets, sheets_x) = (schema("secrets"), schema("sheets_x"));
        let catalog = [&secrets, &sheets_x];
        let mut names = tables();
        names.push("sheets_x".into());
        let check = |who: &Principal, sql: &str| {
            let command = chloedb::query::sql::bind_command(sql, &catalog, &Default::default()).unwrap();
            check_statement(who, sql, &command, &names, &functions)
        };
        // Statements by their kind: CREATE needs admin on the table.
        assert!(check(&k, "CREATE TABLE sheets_books (id KEYWORD PRIMARY KEY)").is_ok());
        assert_eq!(check(&k, "CREATE TABLE other (id KEYWORD PRIMARY KEY)").unwrap_err().status, 403);
        assert_eq!(check(&k, "DROP TABLE secrets").unwrap_err().status, 403);
        assert!(check(&k, "DROP TABLE sheets_x").is_ok());
        let e = check(&k, "DELETE FROM secrets").unwrap_err();
        assert!(e.status == 403 && e.message.contains("write"), "{}", e.message);
        assert!(check(&k, "DELETE FROM sheets_x WHERE id = 'a'").is_ok());
        // A write of its own table that reads one it may not.
        let r = key(&["sheets_*=admin"]);
        assert!(check(&r, "UPDATE sheets_x SET body = 'b' WHERE id = 'a'").is_ok());
        // (The binder takes no such subquery yet; its names are refused
        // first all the same.)
        assert_eq!(check_reads(&r, "DELETE FROM sheets_x WHERE id IN (SELECT id FROM secrets)", &names, &functions).unwrap_err().status, 403);
        // Read on a table is not write there.
        let w = key(&["sheets_*=read"]);
        assert_eq!(check(&w, "DELETE FROM sheets_x").unwrap_err().status, 403);
        // Functions are the server's.
        let f = "CREATE FUNCTION twice(x FLOAT) RETURNS FLOAT AS :x * 2";
        let command = chloedb::query::sql::bind_command(f, &catalog, &Default::default()).unwrap();
        assert_eq!(check_statement(&key(&["*=admin"]), f, &command, &names, &functions).unwrap_err().status, 403);
    }

    #[test]
    fn routes_of_a_limited_key() {
        assert!(route_ok(&Method::POST, "/v1/sql"));
        assert!(route_ok(&Method::GET, "/v1/tables"));
        assert!(route_ok(&Method::PUT, "/v1/kv/:table/:key"));
        for (m, r) in [(Method::POST, "/v1/admin/keys"), (Method::POST, "/v1/admin/shards"), (Method::GET, "/v1/export"), (Method::GET, "/v1/feed"), (Method::POST, "/v1/mcp"), (Method::POST, "/v1/plan"), (Method::GET, "/v1/apps/:name/*rest"), (Method::GET, "/v1/queries/recent"), (Method::POST, "/v1/alerts/:rule/ack"), (Method::GET, "/metrics")] {
            assert!(!route_ok(&m, r), "{m} {r}");
        }
    }
}
