//! Text where a query wants a vector, embedded before the query is bound.
//!
//! * `similarity(emb, 'how do I reset my password')`, `distance(emb, :q)`
//!   and `maxsim(tokens, 'some words')` with text (a literal, or a
//!   parameter that holds text rather than a JSON array) against a VECTOR
//!   field that is the embedding of other fields (`OPTIONS(embed = '..',
//!   model = '..')`): the text is embedded as a query by the field's own model
//!   ([`crate::db::Db::embed_query_in`]), the vector a query would
//!   otherwise have to compute and send itself.
//! * `similarity(emb, embed(:q))`: the same, written out.
//! * `embed('potion-base-8M', :q)`: the query vector of the text by the
//!   model named, anywhere a vector goes (against a VECTOR field of no
//!   model, in a projection).
//!
//! The text must be a value or a parameter: the model runs once, when the
//! query is planned, not once a row. A column is embedded at write time by
//! declaring its vector field `OPTIONS(embed = 'column', model = '..')`.
//!
//! Each vector takes the place of its text as a parameter of its own
//! (`:__embed_<n>`), so the binder and the plan see a vector as if the
//! caller had sent it. A SPARSE field, whose `similarity` takes its terms
//! as text, and a VECTOR field of no model are left as written.

use std::collections::HashMap;

use sqlparser::ast::{self as sql, Expr, FunctionArg, FunctionArgExpr, FunctionArguments, TableFactor};

use crate::error::{Error, Result};
use crate::query::function::{query_exprs_mut, relations_mut, visit_expr_mut};
use crate::query::sql::Params;
use crate::schema::{FieldType, Schema};
use crate::value::Value;

/// What embeds the texts: a database's models.
pub trait Embeds {
    /// The query parameter for `text` against embedded field `field` of
    /// table `table`, by the field's model: a vector, or for a multi-vector
    /// field the list of its tokens' vectors.
    fn field_query(&self, table: &str, field: &str, text: &str) -> Result<Value>;
    /// The query vector of `text` by the model named `model`.
    fn model_query(&self, model: &str, text: &str) -> Result<Vec<f32>>;
}

/// Why `embed(name, ..)` may not open model `name` from a models
/// directory `models`, if it may not: a model is named, as `llm()`'s and
/// `cross_encoder()`'s are, by a directory directly under the models
/// directory (letters, digits, `_`, `-` and `.`), or is `hash:N`, so that
/// a query reads no file elsewhere; and a model behind an API
/// (`openai/...`, an `endpoint.json`) is used only through a field it
/// embeds, so that a query does not spend a key no table was given.
pub fn refuse_model(name: &str, models: &std::path::Path) -> Option<&'static str> {
    if let Some(n) = name.strip_prefix("hash:") {
        return match n.parse::<usize>() {
            Ok(d) if (1..=crate::schema::MAX_VECTOR_DIM).contains(&d) => None,
            _ => Some(HASH_DIMS),
        };
    }
    if name.is_empty() || name.starts_with('.') || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b)) {
        return Some("a model is named by its directory under the models directory, such as 'potion-base-8M', or is the model of an embedded field");
    }
    if models.join(name).join("endpoint.json").exists() {
        return Some("a model behind an API is used by a query only through a field it embeds");
    }
    None
}

/// Why `hash:N` is refused: N out of range.
const HASH_DIMS: &str = "hash:N makes vectors of N dimensions, from 1 to 65536 (schema::MAX_VECTOR_DIM)";

/// The prefix of the parameters this writes.
pub const PARAM: &str = "__embed_";

/// Whether the text of a query may hold something [`rewrite`] embeds: a
/// test of the text, so that a query with nothing to embed is not walked.
pub fn may_embed(sql_text: &str) -> bool {
    let t = sql_text.as_bytes();
    let has = |w: &[u8]| t.windows(w.len()).any(|x| x.eq_ignore_ascii_case(w));
    has(b"embed") || ((has(b"similarity") || has(b"distance") || has(b"maxsim")) && (t.contains(&b'\'') || t.contains(&b':') || t.contains(&b'$') || t.contains(&b'?')))
}

/// `q` with its texts embedded, and `params` with their vectors; `None`
/// when it has nothing to embed.
pub fn rewrite(q: &sql::Query, catalog: &[&Schema], params: &Params, embeds: &dyn Embeds) -> Result<Option<(sql::Query, Params)>> {
    let mut out = q.clone();
    // The tables the query reads, by name and by alias.
    let mut tables: Vec<(String, String)> = Vec::new();
    relations_mut(&mut out, &mut |t| {
        if let TableFactor::Table { name, alias, .. } = t {
            let n = name.0.last().and_then(|p| p.as_ident()).map(|i| i.value.clone()).unwrap_or_default();
            if let Some(s) = catalog.iter().find(|s| s.table.eq_ignore_ascii_case(&n)) {
                tables.push((n.clone(), s.table.clone()));
                if let Some(a) = alias {
                    tables.push((a.name.value.clone(), s.table.clone()));
                }
            }
        }
    });
    let mut w = Writer { catalog, tables, params, embeds, out: params.clone(), made: HashMap::new(), failed: None };
    query_exprs_mut(&mut out, &mut |e| w.expr(e));
    if let Some(e) = w.failed {
        return Err(e);
    }
    Ok((!w.made.is_empty()).then_some((out, w.out)))
}

/// A text [`rewrite`] embeds, and what it embeds it with.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Request {
    /// By the model of embedded field `field` of `table` ([`Embeds::field_query`]).
    Field { table: String, field: String, text: String },
    /// By the model named ([`Embeds::model_query`]).
    Model { model: String, text: String },
}

/// The texts query `sql_text` embeds, for a caller that embeds them its
/// own way (a server, on its embedding cores) before [`written`]: empty
/// when there are none.
pub fn requests(sql_text: &str, catalog: &[&Schema], params: &Params) -> Result<Vec<Request>> {
    if !may_embed(sql_text) {
        return Ok(Vec::new());
    }
    struct Recorder(std::cell::RefCell<Vec<Request>>);
    impl Embeds for Recorder {
        fn field_query(&self, table: &str, field: &str, text: &str) -> Result<Value> {
            self.0.borrow_mut().push(Request::Field { table: table.into(), field: field.into(), text: text.into() });
            Ok(Value::Null)
        }
        fn model_query(&self, model: &str, text: &str) -> Result<Vec<f32>> {
            self.0.borrow_mut().push(Request::Model { model: model.into(), text: text.into() });
            Ok(Vec::new())
        }
    }
    let r = Recorder(Default::default());
    rewrite(&*super::sql::parse_query(sql_text)?, catalog, params, &r)?;
    Ok(r.0.into_inner())
}

/// Query `sql_text` with its texts embedded as `given` holds them (the
/// value of each of its [`requests`]), as text, and its parameters with
/// their vectors; `None` when it has nothing to embed.
pub fn written(sql_text: &str, catalog: &[&Schema], params: &Params, given: &HashMap<Request, Value>) -> Result<Option<(String, Params)>> {
    if !may_embed(sql_text) {
        return Ok(None);
    }
    struct Given<'a>(&'a HashMap<Request, Value>);
    impl Embeds for Given<'_> {
        fn field_query(&self, table: &str, field: &str, text: &str) -> Result<Value> {
            self.0.get(&Request::Field { table: table.into(), field: field.into(), text: text.into() }).cloned().ok_or_else(|| err(format!("no vector was given for {table}.{field}'s text")))
        }
        fn model_query(&self, model: &str, text: &str) -> Result<Vec<f32>> {
            match self.0.get(&Request::Model { model: model.into(), text: text.into() }) {
                Some(Value::Vector(v)) => Ok(v.to_vec()),
                _ => Err(err(format!("no vector was given for embed('{model}', ..)"))),
            }
        }
    }
    Ok(rewrite(&*super::sql::parse_query(sql_text)?, catalog, params, &Given(given))?.map(|(q, p)| (q.to_string(), p)))
}

struct Writer<'a> {
    catalog: &'a [&'a Schema],
    /// (name or alias, table) of each table the query reads.
    tables: Vec<(String, String)>,
    params: &'a Params,
    embeds: &'a dyn Embeds,
    out: Params,
    /// The parameter of each text embedded, by what embedded it, so that
    /// a text repeated is embedded once.
    made: HashMap<(String, String, String), String>,
    failed: Option<Error>,
}

fn err(m: impl Into<String>) -> Error {
    Error::Query(m.into())
}

/// `e` with `pre` before its message, of the same kind (a hosted model
/// that did not answer stays [`Error::Unavailable`]).
fn context(pre: &str, e: Error) -> Error {
    let m = |m: String| format!("{pre}: {m}");
    match e {
        Error::Schema(x) => Error::Schema(m(x)),
        Error::Query(x) => Error::Query(m(x)),
        Error::Busy(x) => Error::Busy(m(x)),
        Error::Timeout(x) => Error::Timeout(m(x)),
        Error::Memory(x) => Error::Memory(m(x)),
        Error::Unavailable(x) => Error::Unavailable(m(x)),
        other => other,
    }
}

fn fname(f: &sql::Function) -> String {
    f.name.0.last().and_then(|p| p.as_ident()).map(|i| i.value.to_ascii_lowercase()).unwrap_or_default()
}

/// The expressions of a call's arguments, positional and named, in order.
fn args_mut(f: &mut sql::Function) -> Vec<&mut Expr> {
    match &mut f.args {
        FunctionArguments::List(l) => l
            .args
            .iter_mut()
            .filter_map(|a| match a {
                FunctionArg::Unnamed(FunctionArgExpr::Expr(x)) => Some(x),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

impl Writer<'_> {
    fn expr(&mut self, e: &mut Expr) {
        visit_expr_mut(e, &mut |x| {
            if self.failed.is_some() {
                return false;
            }
            let Expr::Function(f) = x else { return true };
            let name = fname(f);
            let r = match name.as_str() {
                "similarity" | "distance" | "maxsim" => self.ranking(f, &name),
                "embed" => self.embed(f).map(|v| v.map(|p| *x = p)).map(|_| ()),
                _ => Ok(()),
            };
            if let Err(e) = r {
                self.failed = Some(e);
                return false;
            }
            // A call written out is a parameter now: nothing inside it.
            matches!(x, Expr::Function(_))
        });
    }

    /// `similarity(field, text)` and its kin: the text by the field's model.
    fn ranking(&mut self, f: &mut sql::Function, name: &str) -> Result<()> {
        let mut args = args_mut(f);
        let [field, v, ..] = args.as_mut_slice() else { return Ok(()) };
        let Some((table, fld)) = self.field(field) else { return Ok(()) };
        // `embed(text)`, of one argument, is the text by the field's model.
        let text = match &**v {
            Expr::Function(g) if fname(g) == "embed" && matches!(&g.args, FunctionArguments::List(l) if l.args.len() == 1) => {
                let mut g = g.clone();
                // `embed(*)` and `embed(name => ..)` have no expression to embed.
                let t = match args_mut(&mut g).as_slice() {
                    [t] => (**t).clone(),
                    _ => return Err(err(format!("embed(..) takes text, a value or a parameter, not {}: the model runs once a query. To embed a column, declare its vector field OPTIONS(embed = 'column', model = '..')", g.args))),
                };
                match self.text(&t)? {
                    Some(t) => t,
                    None => return Err(err(format!("embed(..) takes text, a value or a parameter, not {t}: the model runs once a query. To embed a column, declare its vector field OPTIONS(embed = 'column', model = '..')"))),
                }
            }
            other => match self.text(other)? {
                Some(t) => t,
                None => return Ok(()),
            },
        };
        let schema = self.schema(&table);
        let f = schema.and_then(|s| s.field(&fld));
        let Some(f) = f.filter(|f| matches!(f.ty, FieldType::Vector { .. })) else { return Ok(()) };
        let Some(model) = f.embed.as_ref().map(|e| e.model.clone()) else {
            if matches!(&**v, Expr::Function(_)) {
                return Err(err(format!("{name}({}, embed(..)): {} is not the embedding of other fields, so it has no model to embed the text with; name one, embed('model', ..)", f.name, f.name)));
            }
            return Ok(());
        };
        let key = (table.to_ascii_lowercase(), f.name.to_ascii_lowercase(), text.clone());
        let p = match self.made.get(&key) {
            Some(p) => p.clone(),
            None => {
                let value = self.embeds.field_query(&table, &f.name, &text).map_err(|e| context(&format!("{name}({}, ..) embeds its text with {model}, {}'s model", f.name, f.name), e))?;
                self.put(key, value)
            }
        };
        **v = placeholder(&p);
        Ok(())
    }

    /// `embed('model', text)`: the query vector, as a parameter.
    fn embed(&mut self, f: &mut sql::Function) -> Result<Option<Expr>> {
        let args = args_mut(f);
        match args.as_slice() {
            // Of one argument it takes its model from similarity(field, ..), which
            // writes it out before reaching here; anywhere else it has none.
            [_] => Err(err("embed(text) takes its model from the field of similarity(field, embed(text)); anywhere else, name the model: embed('potion-base-8M', text)")),
            [m, t] => {
                let Some(model) = self.text(m)? else { return Err(err(format!("embed(model, text): the model is a name, such as 'potion-base-8M', not {m}"))) };
                let Some(text) = self.text(t)? else {
                    return Err(err(format!("embed('{model}', ..) takes text, a value or a parameter, not {t}: the model runs once a query. To embed a column, declare its vector field OPTIONS(embed = 'column', model = '{model}')")));
                };
                let key = (String::new(), model.clone(), text.clone());
                let p = match self.made.get(&key) {
                    Some(p) => p.clone(),
                    None => {
                        let v = self.embeds.model_query(&model, &text).map_err(|e| context(&format!("embed('{model}', ..)"), e))?;
                        self.put(key, Value::Vector(v.into()))
                    }
                };
                Ok(Some(placeholder(&p)))
            }
            _ => Err(err("embed('model', text): the model's name and the text")),
        }
    }

    fn put(&mut self, key: (String, String, String), value: Value) -> String {
        let p = format!("{PARAM}{}", self.made.len());
        self.out.0.insert(p.clone(), value);
        self.made.insert(key, p.clone());
        p
    }

    /// The text of a literal or of a parameter that holds text; `None` for
    /// anything else, a parameter of a vector, or text that is a JSON array
    /// of numbers or of arrays of numbers (a vector, or a multi-vector
    /// field's query vectors, written as text).
    fn text(&self, e: &Expr) -> Result<Option<String>> {
        let s = match e {
            Expr::Nested(x) => return self.text(x),
            Expr::Value(v) => match &v.value {
                sql::Value::SingleQuotedString(s) | sql::Value::EscapedStringLiteral(s) | sql::Value::NationalStringLiteral(s) => s.clone(),
                sql::Value::DollarQuotedString(d) => d.value.clone(),
                sql::Value::Placeholder(p) => match self.params.0.get(p.trim_start_matches([':', '$', '@', '?'])) {
                    Some(Value::Str(s)) => s.clone(),
                    _ => return Ok(None),
                },
                _ => return Ok(None),
            },
            _ => return Ok(None),
        };
        let vectors = serde_json::from_str::<Vec<f32>>(&s).is_ok() || serde_json::from_str::<Vec<Vec<f32>>>(&s).is_ok();
        Ok((!vectors).then_some(s))
    }

    fn schema(&self, table: &str) -> Option<&Schema> {
        self.catalog.iter().find(|s| s.table.eq_ignore_ascii_case(table)).copied()
    }

    /// The table and the name of the field `e` names: `t.field` by the
    /// table or alias `t`, `field` by the one table the query reads that
    /// has a VECTOR field of that name.
    fn field(&self, e: &Expr) -> Option<(String, String)> {
        match e {
            Expr::Identifier(i) => {
                let mut with = self.tables.iter().map(|(_, t)| t).filter(|t| self.schema(t).and_then(|s| s.field(&i.value)).is_some_and(|f| matches!(f.ty, FieldType::Vector { .. })));
                let first = with.next()?.clone();
                // Two tables of the query with such a field: the binder says
                // which it would read, or that it is ambiguous.
                with.all(|t| t.eq_ignore_ascii_case(&first)).then(|| (first, i.value.clone()))
            }
            Expr::CompoundIdentifier(parts) if parts.len() >= 2 => {
                let q = &parts[parts.len() - 2].value;
                let t = self.tables.iter().find(|(n, _)| n.eq_ignore_ascii_case(q))?;
                Some((t.1.clone(), parts[parts.len() - 1].value.clone()))
            }
            Expr::Nested(x) => self.field(x),
            _ => None,
        }
    }
}

fn placeholder(p: &str) -> Expr {
    Expr::value(sql::Value::Placeholder(format!(":{p}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::sql::parse_query;

    struct Fake;
    impl Embeds for Fake {
        fn field_query(&self, table: &str, field: &str, text: &str) -> Result<Value> {
            Ok(Value::Vector(vec![text.len() as f32, table.len() as f32, field.len() as f32].into()))
        }
        fn model_query(&self, model: &str, text: &str) -> Result<Vec<f32>> {
            if model == "missing" {
                return Err(err("no such model"));
            }
            Ok(vec![text.len() as f32, model.len() as f32])
        }
    }

    fn schema() -> Schema {
        let sql_text = "CREATE TABLE docs (id KEYWORD PRIMARY KEY, body TEXT, emb VECTOR(3) OPTIONS(embed = 'body', model = 'hash:3'), raw VECTOR(2), terms SPARSE)";
        match crate::query::bind_command(sql_text, &[], &Params::new()).unwrap() {
            crate::query::Command::CreateTable { schema, .. } => schema,
            _ => unreachable!("a CREATE TABLE"),
        }
    }

    fn run(sql_text: &str, params: Params) -> Result<Option<(String, Params)>> {
        let s = schema();
        let q = parse_query(sql_text)?;
        Ok(rewrite(&q, &[&s], &params, &Fake)?.map(|(q, p)| (q.to_string(), p)))
    }

    #[test]
    fn text_against_an_embedded_field_becomes_its_vector() {
        let (q, p) = run("SELECT id FROM docs ORDER BY similarity(emb, 'reset password') DESC LIMIT 3", Params::new()).unwrap().unwrap();
        assert!(q.contains("similarity(emb, :__embed_0)"), "{q}");
        assert_eq!(p.0.get("__embed_0"), Some(&Value::Vector(vec![14.0, 4.0, 3.0].into())));
        // a parameter of text, and the same text twice is embedded once
        let (q, p) = run("SELECT id FROM docs d WHERE similarity(d.emb, :q) > 0.1 ORDER BY similarity(emb, :q) DESC", Params::new().set("q", Value::Str("abc".into()))).unwrap().unwrap();
        assert!(q.contains("similarity(d.emb, :__embed_0)") && q.contains("similarity(emb, :__embed_0)"), "{q}");
        assert_eq!(p.0.len(), 2);
        // embed(text) of one argument names the field's model
        let (q, _) = run("SELECT id FROM docs ORDER BY distance(emb, embed('x')) ASC", Params::new()).unwrap().unwrap();
        assert!(q.contains("distance(emb, :__embed_0)"), "{q}");
    }

    #[test]
    fn vectors_sparse_terms_and_fields_of_no_model_are_left() {
        assert!(run("SELECT id FROM docs ORDER BY similarity(emb, '[1, 2, 3]') DESC", Params::new()).unwrap().is_none());
        assert!(run("SELECT id FROM docs ORDER BY maxsim(emb, '[[1, 2, 3], [4, 5, 6]]') DESC", Params::new()).unwrap().is_none());
        assert!(run("SELECT id FROM docs ORDER BY similarity(emb, :v) DESC", Params::new().set("v", Value::Vector(vec![1.0, 2.0, 3.0].into()))).unwrap().is_none());
        assert!(run("SELECT id FROM docs ORDER BY similarity(terms, 'cat:1 dog:2') DESC", Params::new()).unwrap().is_none());
        assert!(run("SELECT id FROM docs ORDER BY similarity(raw, 'some words') DESC", Params::new()).unwrap().is_none());
        assert!(run("SELECT id FROM docs ORDER BY bm25(body, 'x') DESC", Params::new()).unwrap().is_none());
        let e = run("SELECT id FROM docs ORDER BY similarity(raw, embed('x')) DESC", Params::new()).unwrap_err().to_string();
        assert!(e.contains("embed('model', ..)"), "{e}");
    }

    #[test]
    fn embed_with_a_model_is_a_vector_anywhere() {
        let (q, p) = run("SELECT id FROM docs ORDER BY similarity(raw, embed('m1', :q)) DESC", Params::new().set("q", Value::Str("hello".into()))).unwrap().unwrap();
        assert!(q.contains("similarity(raw, :__embed_0)"), "{q}");
        assert_eq!(p.0.get("__embed_0"), Some(&Value::Vector(vec![5.0, 2.0].into())));
        let e = run("SELECT id FROM docs ORDER BY similarity(raw, embed('missing', 'x')) DESC", Params::new()).unwrap_err().to_string();
        assert!(e.contains("embed('missing', ..): no such model"), "{e}");
        let e = run("SELECT embed('m1', body) FROM docs", Params::new()).unwrap_err().to_string();
        assert!(e.contains("OPTIONS(embed = 'column'"), "{e}");
        let e = run("SELECT embed('x') FROM docs", Params::new()).unwrap_err().to_string();
        assert!(e.contains("name the model"), "{e}");
    }

    #[test]
    fn a_caller_embeds_the_requests_itself() {
        let s = schema();
        let sql_text = "SELECT id FROM docs ORDER BY rrf(similarity(emb, :q), similarity(raw, embed('m1', :q))) DESC";
        let p = Params::new().set("q", Value::Str("words".into()));
        let r = requests(sql_text, &[&s], &p).unwrap();
        assert_eq!(r, [Request::Field { table: "docs".into(), field: "emb".into(), text: "words".into() }, Request::Model { model: "m1".into(), text: "words".into() }]);
        let given: HashMap<Request, Value> = r.into_iter().zip([Value::Vector(vec![1.0, 0.0, 0.0].into()), Value::Vector(vec![0.0, 1.0].into())]).collect();
        let (q, out) = written(sql_text, &[&s], &p, &given).unwrap().unwrap();
        assert!(q.contains("similarity(emb, :__embed_0)") && q.contains("similarity(raw, :__embed_1)"), "{q}");
        assert_eq!(out.0.get("__embed_1"), Some(&Value::Vector(vec![0.0, 1.0].into())));
        // the text is planned again as it is written
        crate::query::compound::plan_read(&q, &[&s], &out, &|_, _| None).unwrap();
        assert!(written(sql_text, &[&s], &p, &HashMap::new()).is_err());
        assert!(requests("SELECT id FROM docs WHERE id = 'x'", &[&s], &Params::new()).unwrap().is_empty());
    }

    #[test]
    fn a_query_names_a_model_not_a_path() {
        let dir = std::env::temp_dir().join(format!("chloedb-embed-names-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("served")).unwrap();
        std::fs::write(dir.join("served").join("endpoint.json"), "{}").unwrap();
        for ok in ["potion-base-8M", "potion-retrieval-32M-int8", "my_model.v2", "hash:16"] {
            assert_eq!(refuse_model(ok, &dir), None, "{ok}");
        }
        for bad in ["/etc", "../models", "a/b", "openai/text-embedding-3-small", ".hidden", "", "hash:", "hash:x", "C:\\m", "served"] {
            assert!(refuse_model(bad, &dir).is_some(), "{bad}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_text_test_is_cheap_and_loose() {
        assert!(may_embed("SELECT * FROM t ORDER BY similarity(e, 'x') DESC"));
        assert!(may_embed("select embed('m', :q)"));
        assert!(!may_embed("SELECT id FROM t WHERE year > 3"));
    }
}
