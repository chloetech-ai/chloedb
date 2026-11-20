//! Writes of a SELECT's rows (SAM-8): `INSERT INTO t [(columns)] SELECT
//! ...` and `CREATE TABLE t [IF NOT EXISTS] [(columns)] AS SELECT ...`.
//!
//! ```sql
//! CREATE TABLE refunds (PRIMARY KEY (id)) AS
//!   SELECT id, customer, amount, notes FROM tickets WHERE MATCH(notes, 'refund OR chargeback');
//! INSERT INTO refunds (id, customer, amount, notes)
//!   SELECT id, customer, amount, notes FROM tickets WHERE ts > :since
//!   ON CONFLICT (id) DO UPDATE SET amount = excluded.amount;
//! ```
//!
//! * **The SELECT** runs as any query does, on one snapshot, with its
//!   parameters, and every row it returns is written (no row limit but its
//!   own `LIMIT`; held to the statement's memory budget).
//! * **INSERT ... SELECT** writes each row as `INSERT ... VALUES` would: to
//!   the columns it names, in order, or to the table's columns in order;
//!   the columns it leaves out take their defaults; `ON CONFLICT`, `OR
//!   REPLACE` and `OR IGNORE` apply as they do to VALUES. Each value goes in
//!   as `UPDATE ... SET` gives one ([`super::sql::assigned`]): NULL stays
//!   NULL, a whole FLOAT goes into an INT, a number into a KEYWORD as its
//!   text.
//! * **CREATE TABLE ... AS SELECT** makes a table of the SELECT's columns,
//!   named as the SELECT names them (`a.zone` is `zone`), then writes its
//!   rows. A column's type is the type of the column of a table it reads
//!   (`zone` of a KEYWORD field is KEYWORD, `notes` of a TEXT field is TEXT
//!   with its analyzer), else the type a `CAST` names (`CAST(x AS TEXT)` is
//!   TEXT, `AS VARCHAR` or `AS KEYWORD` is KEYWORD), else the type of its
//!   values (text is KEYWORD when its values are labels, none over 256
//!   bytes and 2.5 words or fewer on average, as an import types text, and
//!   TEXT otherwise), else the type of its expression (`COUNT(*)`
//!   is INT); a column of no value of an expression of no known type is
//!   KEYWORD, as PostgreSQL makes such a column text. Columns the statement
//!   defines (`CREATE TABLE t (id INT PRIMARY KEY, body TEXT) AS SELECT
//!   ...`) take their definitions, and a `PRIMARY KEY (column)` or the
//!   table's `OPTIONS(...)` are as `CREATE TABLE` takes them. If the rows
//!   cannot be written (a key taken, a NULL key), the table is dropped.
//!
//! On one database the rows are written in one transaction, and a table
//! made by `CREATE TABLE ... AS SELECT` is made before them (in a commit of
//! its own). Through a coordinator the SELECT runs as a query of the
//! cluster, then its rows go in as `INSERT`s of [`BATCH`] rows each (one row
//! each for `ON CONFLICT ... DO UPDATE`), each a write of the coordinator
//! ([`Statement::inserts`]): not one transaction.

use serde_json::{json, Value as J};
use sqlparser::ast::{self as sql, Expr, SelectItem, SetExpr};

use super::exec::ResultSet;
use super::sql::{assigned, insert_rows, CastTo, Conflict, Write};
use super::Params;
use crate::error::{Error, Result};
use crate::schema::{FieldType, Schema};
use crate::value::Value;

/// Rows an `INSERT` of [`Statement::inserts`] holds.
pub const BATCH: usize = 500;

fn err(m: impl Into<String>) -> Error {
    Error::Query(m.into())
}

/// An `INSERT ... SELECT` or a `CREATE TABLE ... AS SELECT`.
#[derive(Clone, Debug)]
pub enum Statement {
    Insert { ins: Box<sql::Insert>, table: String, query: Box<sql::Query> },
    Create { ct: Box<sql::CreateTable>, table: String, query: Box<sql::Query> },
}

/// Whether `sql_text` may be one ([`parse_statement`] says): it starts
/// with INSERT, REPLACE or CREATE and names SELECT.
pub fn may_be(sql_text: &str) -> bool {
    let first = sql_text.trim_start().split(|c: char| !c.is_ascii_alphabetic()).next().unwrap_or("");
    ["insert", "replace", "create"].iter().any(|w| first.eq_ignore_ascii_case(w)) && sql_text.as_bytes().windows(6).any(|w| w.eq_ignore_ascii_case(b"select"))
}

/// The statement `sql_text` is, if it writes a SELECT's rows: `None` for
/// any other, `INSERT ... VALUES` and an `INSERT` of `chunks_of`'s rows
/// ([`super::chunks`]) among them.
pub fn parse_statement(sql_text: &str, params: &Params) -> Result<Option<Statement>> {
    if !may_be(sql_text) {
        return Ok(None);
    }
    // A text that does not parse is refused by the statement's own path,
    // in its words.
    let Ok(stmt) = super::sql::parse_one(sql_text) else { return Ok(None) };
    match stmt {
        sql::Statement::Insert(ins) => {
            let Some(q) = ins.source.clone() else { return Ok(None) };
            if is_values(&q) || super::chunks::of_query(&q, params)?.is_some() {
                return Ok(None);
            }
            let sql::TableObject::TableName(n) = &ins.table else { return Err(err("INSERT needs a table name")) };
            let table = super::sql::table_name(n);
            Ok(Some(Statement::Insert { ins: Box::new(ins), table, query: q }))
        }
        sql::Statement::CreateTable(mut ct) => {
            let Some(q) = ct.query.take() else { return Ok(None) };
            let table = match ct.name.0.as_slice() {
                [sql::ObjectNamePart::Identifier(id)] => id.value.clone(),
                _ => return Err(err(format!("a table name is one identifier, not {}", ct.name))),
            };
            Ok(Some(Statement::Create { ct: Box::new(ct), table, query: q }))
        }
        _ => Ok(None),
    }
}

/// Whether `q` is a VALUES list, bare or with the clauses the binder
/// refuses in its own words.
fn is_values(q: &sql::Query) -> bool {
    match q.body.as_ref() {
        SetExpr::Values(_) => true,
        SetExpr::Query(inner) => is_values(inner),
        _ => false,
    }
}

impl Statement {
    /// The table it writes (or makes).
    pub fn table(&self) -> &str {
        match self {
            Statement::Insert { table, .. } | Statement::Create { table, .. } => table,
        }
    }

    /// Whether it makes its table (`CREATE TABLE ... AS SELECT`).
    pub fn creates(&self) -> bool {
        matches!(self, Statement::Create { .. })
    }

    /// Whether it says `IF NOT EXISTS`: when the table is there, it does
    /// nothing, and its SELECT does not run.
    pub fn if_not_exists(&self) -> bool {
        matches!(self, Statement::Create { ct, .. } if ct.if_not_exists)
    }

    /// Whether it says `ON CONFLICT ... DO UPDATE`.
    pub fn updates_on_conflict(&self) -> bool {
        matches!(self, Statement::Insert { ins, .. } if matches!(&ins.on, Some(sql::OnInsert::OnConflict(oc)) if matches!(oc.action, sql::OnConflictAction::DoUpdate(_))))
    }

    /// Its SELECT.
    pub fn query(&self) -> &sql::Query {
        match self {
            Statement::Insert { query, .. } | Statement::Create { query, .. } => query,
        }
    }

    /// Its SELECT's text, as a query runs it.
    pub fn query_sql(&self) -> String {
        self.query().to_string()
    }

    /// The table `CREATE TABLE ... AS SELECT` makes for the rows `rs` its
    /// SELECT returned, of a database whose tables are `tables` (the
    /// tables the SELECT reads give their columns' types).
    pub fn table_for(&self, rs: &ResultSet, tables: &[&Schema]) -> Result<Schema> {
        let Statement::Create { ct, query, .. } = self else { return Err(err("INSERT ... SELECT writes a table that is there")) };
        whole(rs)?;
        let names = column_names(rs)?;
        let hints = hints(query, &rs.columns, tables);
        let mut fields = Vec::with_capacity(names.len());
        for (c, name) in names.iter().enumerate() {
            // The statement's own definition of the column, if it has one.
            if ct.columns.iter().any(|d| d.name.value.eq_ignore_ascii_case(name)) {
                fields.push(json!({"name": name}));
                continue;
            }
            let values = rs.rows.iter().map(|r| r.get(c).unwrap_or(&Value::Null));
            let mut f = match &hints[c] {
                Hint::Field(f) => f.clone(),
                Hint::Cast(ty) => json!({"type": ty}),
                Hint::Expr(ty) => of_values(name, values)?.unwrap_or_else(|| json!({"type": ty})),
                Hint::None => of_values(name, values)?.unwrap_or_else(|| json!({"type": "keyword"})),
            };
            f["name"] = json!(name);
            fields.push(f);
        }
        super::ddl::create_table_as(ct, &fields, tables)
    }

    /// The write of the rows `rs` its SELECT returned into `schema`, the
    /// table it writes as it is now.
    pub fn write(&self, schema: &Schema, params: &Params, rs: ResultSet) -> Result<Write> {
        whole(&rs)?;
        let width = rs.columns.len();
        match self {
            Statement::Insert { ins, .. } => insert_rows(ins, schema, params, width, rs.rows),
            Statement::Create { .. } => {
                let names = column_names(&rs)?;
                let cols = names.iter().map(|n| schema.field_index(n).ok_or_else(|| err(format!("{} has no column {n}", schema.table)))).collect::<Result<Vec<usize>>>()?;
                let start = schema.defaults(crate::timestamp::now())?;
                let mut docs = Vec::with_capacity(rs.rows.len());
                for (n, row) in rs.rows.into_iter().enumerate() {
                    let mut doc = start.clone();
                    for (&f, v) in cols.iter().zip(row) {
                        doc[f] = assigned(&schema.fields[f], v).map_err(|e| e.within(&format!("row {} of the SELECT", n + 1)))?;
                    }
                    docs.push(doc);
                }
                Ok(Write::Insert { docs, conflict: Conflict::Fail })
            }
        }
    }

    /// The rows `rs` as `INSERT ... VALUES` statements of [`BATCH`] rows
    /// each, and their parameters: how a coordinator writes them, each
    /// statement routed to its rows' shards. They keep the statement's
    /// columns and its `ON CONFLICT`, `OR REPLACE` or `OR IGNORE`.
    ///
    /// An `ON CONFLICT ... DO UPDATE`, which a coordinator runs when its rows
    /// are of one shard, goes a row a statement.
    pub fn inserts(&self, rs: &ResultSet) -> Result<Vec<(String, Params)>> {
        whole(rs)?;
        let batch = if self.updates_on_conflict() { 1 } else { BATCH };
        let mut ins = match self {
            Statement::Insert { ins, .. } => (**ins).clone(),
            Statement::Create { table, .. } => {
                let cols = column_names(rs)?.iter().map(|c| super::sql::ident(c)).collect::<Vec<_>>().join(", ");
                let sql::Statement::Insert(i) = super::sql::parse_one(&format!("INSERT INTO {} ({cols}) VALUES (1)", super::sql::ident(table)))? else { return Err(err("an INSERT")) };
                i
            }
        };
        let mut out = Vec::new();
        for batch in rs.rows.chunks(batch) {
            let mut p = Params::new();
            let mut rows = Vec::with_capacity(batch.len());
            for (r, row) in batch.iter().enumerate() {
                let mut exprs = Vec::with_capacity(row.len());
                for (c, v) in row.iter().enumerate() {
                    let name = format!("r{r}_{c}");
                    p = p.set(&name, v.clone());
                    exprs.push(Expr::Value(sql::Value::Placeholder(format!(":{name}")).into()));
                }
                rows.push(exprs);
            }
            let values = sql::Values { explicit_row: false, rows };
            ins.source = Some(Box::new(sql::Query {
                with: None,
                body: Box::new(SetExpr::Values(values)),
                order_by: None,
                limit_clause: None,
                fetch: None,
                locks: Vec::new(),
                for_clause: None,
                settings: None,
                format_clause: None,
                pipe_operators: Vec::new(),
            }));
            out.push((sql::Statement::Insert(ins.clone()).to_string(), p));
        }
        Ok(out)
    }
}

/// Refuses rows a SELECT cut at its default LIMIT: one ranked by a score
/// without a LIMIT returns its best 100, and a write of those would look
/// like a write of every row that matched.
fn whole(rs: &ResultSet) -> Result<()> {
    match (rs.stats.truncated, rs.stats.implicit_limit) {
        (true, Some(n)) => Err(err(format!("the SELECT is ranked by a score without a LIMIT, so it returns its best {n} rows, not every row that matches: give it a LIMIT, or write the match in WHERE (WHERE MATCH(field, '...')) without ORDER BY"))),
        _ => Ok(()),
    }
}

/// The names of the columns of a table made of `rs`'s: a column's name as
/// the SELECT gives it, without the table it is of (`a.zone` is `zone`).
fn column_names(rs: &ResultSet) -> Result<Vec<String>> {
    let mut names: Vec<String> = Vec::with_capacity(rs.columns.len());
    for c in &rs.columns {
        let plain = match c.rsplit_once('.') {
            Some((t, n)) if !t.is_empty() && !n.is_empty() && !c.contains(['(', ' ', '\'', '"']) => n,
            _ => c.as_str(),
        };
        if plain.contains(['(', ')', ' ', '\'', '*', '+', '/', ',', '|']) {
            return Err(err(format!("column {c} of the SELECT needs a name of its own to be a table's column: {c} AS name")));
        }
        if let Some(was) = names.iter().position(|n| n.eq_ignore_ascii_case(plain)) {
            return Err(err(format!("the SELECT has two columns named {plain} ({} and {c}): name one with AS", rs.columns[was])));
        }
        names.push(plain.to_string());
    }
    Ok(names)
}

/// What the SELECT says of a column's type.
#[derive(Clone, Debug)]
enum Hint {
    /// A column of a table it reads: that field, in the short JSON form
    /// (its type and the options of its type, not what it is made from).
    Field(J),
    /// The type a CAST names.
    Cast(&'static str),
    /// The type of its expression, from its operands.
    Expr(&'static str),
    None,
}

/// A hint per column of the answer (`columns`) of query `q` over `tables`.
fn hints(q: &sql::Query, columns: &[String], tables: &[&Schema]) -> Vec<Hint> {
    let mut out = vec![Hint::None; columns.len()];
    let Some(s) = first_select(q) else { return out };
    // The tables it reads, by the name it gives them; a WITH table is not
    // one of the database's.
    let ctes: Vec<String> = q.with.iter().flat_map(|w| w.cte_tables.iter().map(|c| c.alias.name.value.to_ascii_lowercase())).collect();
    let mut from: Vec<(String, &Schema)> = Vec::new();
    for t in &s.from {
        for f in std::iter::once(&t.relation).chain(t.joins.iter().map(|j| &j.relation)) {
            if let sql::TableFactor::Table { name, alias, .. } = f {
                let n = super::sql::table_name(name);
                if ctes.contains(&n.to_ascii_lowercase()) {
                    continue;
                }
                if let Some(schema) = tables.iter().copied().find(|t| t.table.eq_ignore_ascii_case(&n)) {
                    from.push((alias.as_ref().map_or(n, |a| a.name.value.clone()), schema));
                }
            }
        }
    }
    let field_of = |e: &Expr| -> Option<(&Schema, usize)> {
        match e {
            Expr::Identifier(id) => {
                let mut found = from.iter().filter_map(|(_, s)| s.field_index(&id.value).map(|i| (*s, i)));
                let one = found.next()?;
                found.next().is_none().then_some(one)
            }
            Expr::CompoundIdentifier(parts) if parts.len() == 2 => {
                let (_, s) = from.iter().find(|(a, _)| a.eq_ignore_ascii_case(&parts[0].value))?;
                s.field_index(&parts[1].value).map(|i| (*s, i))
            }
            _ => None,
        }
    };
    // Each item's columns: one, or a table's for `*`.
    let mut items: Vec<Hint> = Vec::new();
    for item in &s.projection {
        match item {
            SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } => items.push(hint_of(e, &field_of)),
            SelectItem::Wildcard(_) => {
                for (_, schema) in &from {
                    items.extend(super::sql::star_fields(schema).map(|i| Hint::Field(short_field(schema, i))));
                }
            }
            SelectItem::QualifiedWildcard(sql::SelectItemQualifiedWildcardKind::ObjectName(n), _) => {
                let n = super::sql::table_name(n);
                match from.iter().find(|(a, _)| a.eq_ignore_ascii_case(&n)) {
                    Some((_, schema)) => items.extend(super::sql::star_fields(schema).map(|i| Hint::Field(short_field(schema, i)))),
                    None => return out,
                }
            }
            _ => return out,
        }
    }
    // Columns the answer names in another way than the items (a `*` of a
    // WITH table): each by its name, when one table read has it.
    if items.len() != columns.len() {
        for (c, name) in columns.iter().enumerate() {
            let plain = name.rsplit('.').next().unwrap_or(name);
            if let Some((schema, i)) = field_of(&Expr::Identifier(sql::Ident::new(plain))) {
                out[c] = Hint::Field(short_field(schema, i));
            }
        }
        return out;
    }
    items
}

/// The first SELECT of a query: its own, or the first of a UNION's, which
/// names and types the answer's columns.
fn first_select(q: &sql::Query) -> Option<&sql::Select> {
    fn of(b: &SetExpr) -> Option<&sql::Select> {
        match b {
            SetExpr::Select(s) => Some(s),
            SetExpr::Query(q) => of(&q.body),
            SetExpr::SetOperation { left, .. } => of(left),
            _ => None,
        }
    }
    of(&q.body)
}

/// Field `i` of `schema` as a column of another table: its type and the
/// options of its type (analyzer, positions, a vector's dimension and
/// index), not what makes it (an embedding, a derivation, the fields a
/// generated text joins), nor its key, references, default or NOT NULL.
fn short_field(schema: &Schema, i: usize) -> J {
    const KEPT: [&str; 13] = ["type", "multi", "dim", "positions", "analyzer", "substring", "text", "kind", "index", "codes", "keep_f32", "pool", "pool_factor"];
    let f = &schema.fields[i];
    let short = schema.to_short_json();
    let full = short["fields"].as_array().and_then(|fs| fs.iter().find(|x| x["name"].as_str() == Some(f.name.as_str()))).cloned().unwrap_or(J::Null);
    let mut out = serde_json::Map::new();
    for k in KEPT {
        if let Some(v) = full.get(k) {
            out.insert(k.to_string(), v.clone());
        }
    }
    if let FieldType::Vector { dim, .. } = f.ty {
        out.insert("dim".into(), json!(dim));
    }
    J::Object(out)
}

/// What expression `e` says of its type.
fn hint_of(e: &Expr, field_of: &dyn Fn(&Expr) -> Option<(&Schema, usize)>) -> Hint {
    if let Some((s, i)) = field_of(e) {
        return Hint::Field(short_field(s, i));
    }
    match e {
        Expr::Nested(x) => hint_of(x, field_of),
        Expr::Cast { data_type, .. } => cast_type(data_type).map_or(Hint::None, Hint::Cast),
        e => expr_type(e, field_of).map_or(Hint::None, Hint::Expr),
    }
}

/// The column type a CAST to `ty` makes: TEXT for `TEXT`, KEYWORD for the
/// other text types.
fn cast_type(ty: &sql::DataType) -> Option<&'static str> {
    Some(match CastTo::of(ty).ok()? {
        CastTo::Int => "int",
        CastTo::Float => "float",
        CastTo::Bool => "bool",
        CastTo::Timestamp => "timestamp",
        CastTo::Text if ty.to_string().eq_ignore_ascii_case("text") => "text",
        CastTo::Text => "keyword",
    })
}

/// The type of an expression from its operands, where they say it: a
/// number's, a comparison's (BOOL), an aggregate's.
fn expr_type(e: &Expr, field_of: &dyn Fn(&Expr) -> Option<(&Schema, usize)>) -> Option<&'static str> {
    use sql::BinaryOperator as B;
    let ty = |e: &Expr| expr_type(e, field_of);
    if let Some((s, i)) = field_of(e) {
        let f = &s.fields[i];
        return Some(match (f.logical, &f.ty) {
            (Some(crate::schema::Logical::Bool), _) => "bool",
            (Some(crate::schema::Logical::Timestamp), _) => "timestamp",
            (None, FieldType::Int) => "int",
            (None, FieldType::Float) => "float",
            (None, FieldType::Text { .. }) => "text",
            (None, FieldType::Keyword) => "keyword",
            _ => return None,
        });
    }
    let numeric = |a: Option<&'static str>, b: Option<&'static str>| match (a, b) {
        (Some("int"), Some("int")) => Some("int"),
        (Some("int" | "float"), Some("int" | "float")) => Some("float"),
        _ => None,
    };
    match e {
        Expr::Nested(x) | Expr::UnaryOp { op: sql::UnaryOperator::Minus | sql::UnaryOperator::Plus, expr: x } => ty(x),
        Expr::Cast { data_type, .. } => cast_type(data_type),
        Expr::Value(v) => match &v.value {
            sql::Value::Number(n, _) => Some(if n.contains(['.', 'e', 'E']) { "float" } else { "int" }),
            sql::Value::SingleQuotedString(_) => Some("keyword"),
            sql::Value::Boolean(_) => Some("bool"),
            _ => None,
        },
        Expr::BinaryOp { left, op, right } => match op {
            B::Plus | B::Minus | B::Multiply | B::Modulo => numeric(ty(left), ty(right)),
            B::Divide => numeric(ty(left), ty(right)).map(|_| "float"),
            B::StringConcat => Some("keyword"),
            B::Eq | B::NotEq | B::Lt | B::LtEq | B::Gt | B::GtEq | B::And | B::Or => Some("bool"),
            _ => None,
        },
        Expr::UnaryOp { op: sql::UnaryOperator::Not, .. }
        | Expr::IsNull(_)
        | Expr::IsNotNull(_)
        | Expr::IsTrue(_)
        | Expr::IsFalse(_)
        | Expr::Like { .. }
        | Expr::ILike { .. }
        | Expr::Between { .. }
        | Expr::InList { .. }
        | Expr::InSubquery { .. }
        | Expr::Exists { .. } => Some("bool"),
        Expr::Case { conditions, else_result, .. } => conditions.iter().map(|c| &c.result).chain(else_result.as_deref()).find_map(ty),
        Expr::Function(f) => {
            let name = f.name.to_string().to_ascii_lowercase();
            let args: Vec<&Expr> = match &f.args {
                sql::FunctionArguments::List(l) => l.args.iter().filter_map(|a| match a {
                    sql::FunctionArg::Unnamed(sql::FunctionArgExpr::Expr(e)) => Some(e),
                    _ => None,
                }).collect(),
                _ => Vec::new(),
            };
            let first = || args.iter().find_map(|a| ty(a));
            match name.as_str() {
                "count" | "length" | "char_length" | "character_length" | "strpos" | "extract" | "date_part" => Some("int"),
                "avg" | "stddev" | "stddev_pop" | "stddev_samp" | "variance" | "var_pop" | "var_samp" | "median" | "percentile_cont" | "bm25" | "similarity" | "score" | "ln" | "log" | "exp" | "sqrt" | "power" | "pow" => Some("float"),
                "sum" | "min" | "max" | "abs" | "coalesce" | "nullif" | "greatest" | "least" | "any_value" | "first_value" | "last_value" | "round" | "floor" | "ceil" | "ceiling" => first(),
                "lower" | "upper" | "trim" | "ltrim" | "rtrim" | "concat" | "concat_ws" | "substr" | "substring" | "replace" | "left" | "right" | "lpad" | "rpad" | "string_agg" | "md5" | "split_part" => Some("keyword"),
                "date_trunc" | "now" | "current_timestamp" | "to_timestamp" | "date" => Some("timestamp"),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The type of a column of `values` (its name `name`), in the short JSON
/// form: `None` when it holds only NULLs; refused when its values are of
/// types no column holds together.
fn of_values<'a>(name: &str, values: impl Iterator<Item = &'a Value> + Clone) -> Result<Option<J>> {
    let Some(ty) = super::exec::column_type(values.clone()) else {
        return match values.clone().find(|v| !matches!(v, Value::Null)) {
            None => Ok(None),
            Some(_) => Err(err(format!("column {name} of the SELECT holds values of several types: CAST it to one (CAST({name} AS TEXT)), or define the column: CREATE TABLE t ({name} TYPE, ...) AS SELECT ..."))),
        };
    };
    Ok(Some(match ty {
        "int" => json!({"type": "int"}),
        "float" => json!({"type": "float"}),
        "bool" => json!({"type": "bool"}),
        "timestamp" => json!({"type": "timestamp"}),
        // Labels are KEYWORD, prose TEXT, as an import of a file types its
        // text ([`crate::sqlite`]): KEYWORD when no value is over 256 bytes
        // and they are 2.5 words or fewer on average.
        "text" => {
            let texts: Vec<&str> = values.filter_map(|v| v.as_str()).collect();
            let words: usize = texts.iter().map(|t| t.split_whitespace().count()).sum();
            let labels = texts.iter().all(|t| t.len() <= 256) && words as f64 <= 2.5 * texts.len() as f64;
            json!({"type": if labels { "keyword" } else { "text" }})
        }
        "vector" => {
            let dims: Vec<usize> = values.filter_map(|v| match v {
                Value::Vector(x) => Some(x.len()),
                _ => None,
            }).collect();
            if dims.windows(2).any(|w| w[0] != w[1]) {
                return Err(err(format!("column {name} of the SELECT holds vectors of several dimensions")));
            }
            json!({"type": "vector", "dim": dims[0]})
        }
        _ => {
            // A list: of text, of whole numbers, or of vectors.
            let items: Vec<&Value> = values.flat_map(|v| v.items()).collect();
            match super::exec::column_type(items.iter().copied()) {
                Some("text") => json!({"type": "keyword", "multi": true}),
                Some("int") => json!({"type": "int", "multi": true}),
                Some("vector") => {
                    let dim = items.iter().find_map(|v| match v {
                        Value::Vector(x) => Some(x.len()),
                        _ => None,
                    });
                    json!({"type": "vector", "multi": true, "dim": dim})
                }
                _ => return Err(err(format!("column {name} of the SELECT holds lists a column does not: KEYWORD[], INT[] and VECTOR(n)[] hold lists of text, of whole numbers and of vectors"))),
            }
        }
    }))
}
