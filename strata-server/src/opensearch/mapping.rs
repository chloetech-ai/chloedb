//! An OpenSearch index as a Strata table: its mapping's fields, the
//! table's columns, and documents both ways.
//!
//! A table of an index holds `_id` (its key), `_version`, `_hash` (the
//! id's hash, an order of the documents that scrolls page through and
//! that breaks ties of sorts) and `_source`, a
//! JSON field with the document, whose index answers `term`, `terms`,
//! `range`, `exists`, `prefix` and the aggregations on every keyword,
//! number and boolean, mapped or not. Fields that need more get columns of
//! their own, named by their path: `text` a TEXT column (its analyzer
//! `english` or `simple`), `date` a TIMESTAMP column, `knn_vector` a
//! VECTOR column. A text field without a keyword sub-field and a vector
//! are kept in their columns alone, not in `_source`, and put back into
//! the document read.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value as Json};

use super::OsError;

/// The table that holds the indexes' names, mappings and aliases.
pub const META: &str = "opensearch_indices";

/// The key, version and document columns of an index's table.
pub const ID: &str = "_id";
pub const VERSION: &str = "_version";
pub const HASH: &str = "_hash";
pub const SOURCE: &str = "_source";

/// A document's place in its index's order ([`HASH`]): its id's xxh3,
/// 63 bits.
pub fn hash_of(id: &str) -> i64 {
    (twox_hash::XxHash3_64::oneshot(id.as_bytes()) >> 1) as i64
}

/// The JSON field's limits ([`strata::json`]): a document past them is
/// refused with the limit it passed, before the write.
const MAX_DEPTH: usize = 16;
const MAX_PATH: usize = 256;
const MAX_VALUES: usize = 1000;

/// What a field of a mapping is, as far as queries are concerned.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// A TEXT column of this analyzer.
    Text { analyzer: &'static str },
    /// A string in `_source`.
    Keyword,
    /// A number in `_source` (`long`, `integer`, `short`, `byte`).
    Long,
    /// A number in `_source` (`double`, `float`, `half_float`, `scaled_float`).
    Double,
    /// A boolean in `_source`.
    Bool,
    /// A TIMESTAMP column (the value as given stays in `_source` too).
    Date,
    /// A VECTOR column of this many dimensions.
    Vector { dim: usize },
    /// Kept in `_source` and found by its terms there (`ip`, `geo_point`,
    /// `flat_object`...).
    Other,
}

/// A field a query can name: `title`, `title.keyword`, `user.name`.
#[derive(Clone, Debug)]
pub struct Field {
    pub kind: Kind,
    /// The type as the mapping names it (`text`, `long`).
    pub ty: String,
    /// The document path its value comes from: the field's own, or for a
    /// sub-field (`title.keyword`) its parent's.
    pub source: String,
}

impl Field {
    /// The column that holds it, if it has one.
    pub fn column(&self, name: &str) -> Option<String> {
        matches!(self.kind, Kind::Text { .. } | Kind::Date | Kind::Vector { .. }).then(|| name.to_string())
    }
}

/// An index: its name, table and mapping.
#[derive(Clone, Debug)]
pub struct Index {
    pub name: String,
    pub table: String,
    pub uuid: String,
    pub created_ms: i64,
    /// The mapping as stored (`{"properties": {...}, "dynamic": ...}`).
    pub mappings: Json,
    pub settings: Json,
    pub aliases: Vec<String>,
    /// Every field a query may name, by name.
    pub fields: BTreeMap<String, Field>,
}

impl Index {
    pub fn new(name: &str, table: &str, uuid: &str, created_ms: i64, mappings: Json, settings: Json, aliases: Vec<String>) -> Result<Index, OsError> {
        let mut fields = BTreeMap::new();
        if let Some(props) = mappings.get("properties") {
            walk_properties(props, "", &settings, &mut fields)?;
        }
        Ok(Index { name: name.into(), table: table.into(), uuid: uuid.into(), created_ms, mappings, settings, aliases, fields })
    }

    /// The index's `dynamic` setting: `true` (the default), `false` or
    /// `strict`.
    pub fn dynamic(&self) -> &str {
        match self.mappings.get("dynamic") {
            Some(Json::Bool(false)) => "false",
            Some(Json::String(s)) if s == "false" || s == "strict" => s.as_str(),
            _ => "true",
        }
    }

    /// The fields with columns, in name order: (column, kind).
    pub fn columns(&self) -> Vec<(String, Kind)> {
        self.fields.iter().filter_map(|(n, f)| f.column(n).map(|c| (c, f.kind.clone()))).collect()
    }

    /// Whether the string at document path `p` is kept in its TEXT
    /// column alone: a text field with no other field (a keyword
    /// sub-field) reading it from `_source`.
    pub fn stripped_text(&self, p: &str) -> bool {
        let text = matches!(self.fields.get(p), Some(Field { kind: Kind::Text { .. }, source, .. }) if source == p);
        text && !self.fields.values().any(|f| f.source == p && !matches!(f.kind, Kind::Text { .. }))
    }

    /// The text columns that hold a string their document no longer
    /// does, and the vector columns: read back into `_source`.
    pub fn kept_apart(&self) -> Vec<String> {
        self.fields.iter().filter(|(n, f)| (f.source == **n && self.stripped_text(n)) || matches!(f.kind, Kind::Vector { .. })).map(|(n, _)| n.clone()).collect()
    }

    /// The row's columns to read for documents: `_id`, `_version`,
    /// `_source`, then [`Self::kept_apart`].
    pub fn read_columns(&self) -> Vec<String> {
        let mut c = vec![ID.to_string(), VERSION.to_string(), SOURCE.to_string()];
        c.extend(self.kept_apart());
        c
    }

    /// The index as `GET /index` answers it.
    pub fn json(&self) -> Json {
        let aliases: Map<String, Json> = self.aliases.iter().map(|a| (a.clone(), json!({}))).collect();
        let mut settings = if self.settings.is_object() { self.settings.clone() } else { json!({}) };
        let mut index = settings.get("index").cloned().filter(Json::is_object).unwrap_or_else(|| json!({}));
        index["uuid"] = json!(self.uuid);
        index["provided_name"] = json!(self.name);
        index["creation_date"] = json!(self.created_ms.to_string());
        for (k, v) in [("number_of_shards", "1"), ("number_of_replicas", "0")] {
            if index.get(k).is_none() {
                index[k] = json!(settings.get(k).cloned().unwrap_or(json!(v)));
            }
        }
        index["version"] = json!({"created": "136327827"});
        settings["index"] = index;
        json!({"aliases": aliases, "mappings": self.mappings, "settings": settings})
    }

    /// The row of [`META`] that keeps it.
    pub fn meta_row(&self) -> Json {
        let meta = json!({"table": self.table, "uuid": self.uuid, "created_ms": self.created_ms, "mappings": self.mappings, "settings": self.settings});
        json!({"name": self.name, "aliases": self.aliases, "meta": meta.to_string()})
    }

    /// The index a [`META`] row keeps.
    pub fn of_meta(name: &str, aliases: Vec<String>, meta: &str) -> Result<Index, OsError> {
        let m: Json = serde_json::from_str(meta).map_err(|e| OsError::internal(format!("index {name}: its stored mapping is not JSON: {e}")))?;
        Index::new(name, m["table"].as_str().unwrap_or_default(), m["uuid"].as_str().unwrap_or_default(), m["created_ms"].as_i64().unwrap_or(0), m["mappings"].clone(), m["settings"].clone(), aliases)
    }
}

/// The analyzer a mapping's analyzer name comes to: `english` for an
/// English analyzer, or a custom one that stems; `simple` (lowercased
/// words, nothing dropped or stemmed) for every other, `standard`
/// included.
fn analyzer_of(name: Option<&str>, settings: &Json) -> &'static str {
    let Some(name) = name else { return "simple" };
    if name == "english" {
        return "english";
    }
    let custom = &settings["analysis"]["analyzer"][name];
    let custom = if custom.is_null() { &settings["index"]["analysis"]["analyzer"][name] } else { custom };
    let stems = |f: &Json| match f.as_str() {
        Some(f) => matches!(f, "porter_stem" | "stemmer" | "snowball" | "kstem" | "english_stemmer"),
        None => false,
    };
    let filters = custom.get("filter").and_then(Json::as_array).map(|a| a.iter().any(stems)).unwrap_or(false);
    if filters || custom.get("type").and_then(Json::as_str) == Some("english") {
        "english"
    } else {
        "simple"
    }
}

/// The kind of a mapping type, or an error for one Strata cannot hold.
fn kind_of(ty: &str, spec: &Json, settings: &Json) -> Result<Kind, OsError> {
    Ok(match ty {
        "text" | "match_only_text" | "search_as_you_type" => Kind::Text { analyzer: analyzer_of(spec.get("analyzer").and_then(Json::as_str), settings) },
        "keyword" | "constant_keyword" | "wildcard" | "version" => Kind::Keyword,
        "long" | "integer" | "short" | "byte" | "unsigned_long" => Kind::Long,
        "double" | "float" | "half_float" | "scaled_float" => Kind::Double,
        "boolean" => Kind::Bool,
        "date" | "date_nanos" => Kind::Date,
        "knn_vector" | "dense_vector" => {
            let dim = spec.get("dimension").or_else(|| spec.get("dims")).and_then(Json::as_u64).ok_or_else(|| OsError::mapper(format!("a {ty} field needs its dimension")))?;
            if dim == 0 || dim > 4096 {
                return Err(OsError::mapper(format!("a {ty} field has 1 to 4096 dimensions, not {dim}")));
            }
            Kind::Vector { dim: dim as usize }
        }
        "ip" | "geo_point" | "geo_shape" | "flat_object" | "flattened" | "binary" | "token_count" | "integer_range" | "long_range" | "float_range" | "double_range" | "date_range" | "ip_range" | "alias" | "join" | "percolator" | "rank_feature" | "rank_features" => Kind::Other,
        other => return Err(OsError::mapper(format!("no handler for type [{other}]: Strata maps text, keyword, numbers, boolean, date, knn_vector, object and nested fields"))),
    })
}

/// Adds the fields of `props` (a mapping's `properties`) under `prefix`.
fn walk_properties(props: &Json, prefix: &str, settings: &Json, out: &mut BTreeMap<String, Field>) -> Result<(), OsError> {
    let Some(props) = props.as_object() else {
        return Err(OsError::mapper("properties is an object of fields"));
    };
    for (name, spec) in props {
        if name.is_empty() {
            return Err(OsError::mapper("a field name is empty"));
        }
        let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}.{name}") };
        let ty = spec.get("type").and_then(Json::as_str).unwrap_or("object");
        if ty == "object" || ty == "nested" {
            if let Some(p) = spec.get("properties") {
                walk_properties(p, &path, settings, out)?;
            }
            continue;
        }
        let kind = kind_of(ty, spec, settings)?;
        out.insert(path.clone(), Field { kind, ty: ty.to_string(), source: path.clone() });
        if let Some(subs) = spec.get("fields").and_then(Json::as_object) {
            for (sub, s) in subs {
                let sty = s.get("type").and_then(Json::as_str).unwrap_or("keyword");
                let kind = kind_of(sty, s, settings)?;
                if matches!(kind, Kind::Vector { .. }) {
                    return Err(OsError::mapper("a vector is not a sub-field"));
                }
                out.insert(format!("{path}.{sub}"), Field { kind, ty: sty.to_string(), source: path.clone() });
            }
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ tables

/// A name in double quotes, as SQL takes any name.
pub fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Text as a SQL string.
pub fn lit(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// The table of an index named `name`: `os_` and the name, its other
/// characters as `_` and then, if any were, a hash of the name, so that
/// two names never share a table.
pub fn table_of(name: &str) -> String {
    let clean: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c.to_ascii_lowercase() } else { '_' }).collect();
    if clean == name {
        return format!("os_{clean}");
    }
    format!("os_{clean}_{:08x}", twox_hash::XxHash3_64::oneshot(name.as_bytes()) as u32)
}

/// Checks an index name as OpenSearch does.
pub fn check_name(name: &str) -> Result<(), OsError> {
    let bad = |why: &str| Err(OsError::new(400, "invalid_index_name_exception", format!("Invalid index name [{name}], {why}")).index(name));
    if name.is_empty() || name == "." || name == ".." {
        return bad("must not be empty, '.' or '..'");
    }
    if name.len() > 255 {
        return bad("index name is too long, (> 255)");
    }
    if name.starts_with(['_', '-', '+']) {
        return bad("must not start with '_', '-', or '+'");
    }
    if name.chars().any(|c| c.is_uppercase()) {
        return bad("must be lowercase");
    }
    if name.contains(['\\', '/', '*', '?', '"', '<', '>', '|', ' ', ',', '#', ':']) {
        return bad("must not contain the following characters [ , \", *, \\, <, |, ,, >, /, ?, #, :]");
    }
    Ok(())
}

/// The column of a field as `CREATE TABLE` and `ALTER TABLE` write it.
pub fn column_sql(name: &str, kind: &Kind) -> String {
    match kind {
        Kind::Text { analyzer } => format!("{} TEXT OPTIONS(analyzer = '{analyzer}')", ident(name)),
        Kind::Date => format!("{} TIMESTAMP", ident(name)),
        Kind::Vector { dim } => format!("{} VECTOR({dim})", ident(name)),
        _ => unreachable!("only text, date and vector fields have columns"),
    }
}

/// `CREATE TABLE` of an index.
pub fn create_sql(ix: &Index) -> String {
    let mut cols = vec![format!("{} KEYWORD PRIMARY KEY", ident(ID)), format!("{} INT", ident(VERSION)), format!("{} INT", ident(HASH)), format!("{} JSON", ident(SOURCE))];
    cols.extend(ix.columns().iter().map(|(c, k)| column_sql(c, k)));
    format!("CREATE TABLE {} ({})", ix.table, cols.join(", "))
}

/// `CREATE TABLE` of [`META`].
pub fn create_meta_sql() -> String {
    format!("CREATE TABLE IF NOT EXISTS {META} (name KEYWORD PRIMARY KEY, aliases KEYWORD[], meta TEXT OPTIONS(indexed = false))")
}

// ------------------------------------------------------------------ documents

/// A document with dotted keys made objects (`{"a.b": 1}` is
/// `{"a": {"b": 1}}`), as OpenSearch reads them.
pub fn expand_dots(doc: Map<String, Json>) -> Result<Map<String, Json>, OsError> {
    let mut out = Map::new();
    for (k, v) in doc {
        let v = match v {
            Json::Object(m) => Json::Object(expand_dots(m)?),
            Json::Array(xs) => Json::Array(xs.into_iter().map(|x| if let Json::Object(m) = x { expand_dots(m).map(Json::Object) } else { Ok(x) }).collect::<Result<_, _>>()?),
            v => v,
        };
        let keys: Vec<&str> = k.split('.').collect();
        if keys.iter().any(|k| k.is_empty()) {
            return Err(OsError::mapper(format!("field name cannot be empty or hold an empty part: [{k}]")));
        }
        insert_path(&mut out, &keys, v)?;
    }
    Ok(out)
}

fn insert_path(m: &mut Map<String, Json>, keys: &[&str], v: Json) -> Result<(), OsError> {
    if keys.len() == 1 {
        match (m.get_mut(keys[0]), v) {
            (Some(Json::Object(a)), Json::Object(b)) => {
                for (k, x) in b {
                    insert_path(a, &[k.as_str()], x)?;
                }
            }
            (_, v) => {
                m.insert(keys[0].to_string(), v);
            }
        }
        return Ok(());
    }
    let next = m.entry(keys[0].to_string()).or_insert_with(|| json!({}));
    if !next.is_object() {
        *next = json!({});
    }
    insert_path(next.as_object_mut().expect("an object"), &keys[1..], v)
}

/// The values at document path `p` (`a.b`), through arrays of objects,
/// leaves of arrays flattened.
pub fn values_at<'a>(doc: &'a Json, p: &str) -> Vec<&'a Json> {
    let mut cur = vec![doc];
    for k in p.split('.') {
        let mut next = Vec::new();
        for c in cur {
            match c {
                Json::Object(m) => {
                    if let Some(v) = m.get(k) {
                        next.push(v);
                    }
                }
                Json::Array(xs) => {
                    for x in xs {
                        if let Json::Object(m) = x {
                            if let Some(v) = m.get(k) {
                                next.push(v);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        cur = next;
    }
    let mut out = Vec::new();
    for c in cur {
        match c {
            Json::Array(xs) => out.extend(xs.iter().filter(|x| !x.is_null())),
            Json::Null => {}
            c => out.push(c),
        }
    }
    out
}

/// Takes the value at path `p` out of `doc`, if objects alone lead to it.
fn take_path(doc: &mut Map<String, Json>, p: &str) -> Option<Json> {
    let keys: Vec<&str> = p.split('.').collect();
    let mut m = doc;
    for k in &keys[..keys.len() - 1] {
        m = m.get_mut(*k)?.as_object_mut()?;
    }
    m.remove(*keys.last()?)
}

/// Puts `v` at path `p` of `doc` unless something is there.
fn put_path(doc: &mut Map<String, Json>, p: &str, v: Json) {
    let keys: Vec<&str> = p.split('.').collect();
    let mut m = doc;
    for k in &keys[..keys.len() - 1] {
        let next = m.entry(k.to_string()).or_insert_with(|| json!({}));
        let Some(o) = next.as_object_mut() else { return };
        m = o;
    }
    m.entry(keys[keys.len() - 1].to_string()).or_insert(v);
}

/// A row of an index's table for a document: its write operation's
/// `doc`. Checks the document against the mapping (a date that is not
/// one, a vector of the wrong length) and the JSON field's limits.
pub fn row_of(ix: &Index, id: &str, version: i64, doc: &Map<String, Json>) -> Result<Json, OsError> {
    let mut source = doc.clone();
    let mut row = Map::new();
    row.insert(ID.into(), json!(id));
    row.insert(VERSION.into(), json!(version));
    row.insert(HASH.into(), json!(hash_of(id)));
    let whole = Json::Object(doc.clone());
    for (name, f) in &ix.fields {
        let vals = values_at(&whole, &f.source);
        if vals.is_empty() {
            continue;
        }
        match &f.kind {
            Kind::Text { .. } => {
                let text: Vec<String> = vals.iter().map(|v| match v {
                    Json::String(s) => s.clone(),
                    other => other.to_string(),
                }).collect();
                row.insert(name.clone(), json!(text.join("\n")));
            }
            Kind::Date => {
                if vals.len() > 1 {
                    return Err(OsError::mapper(format!("failed to parse field [{name}] of type [date]: Strata holds one date a field")));
                }
                let t = parse_date(vals[0]).map_err(|why| OsError::mapper(format!("failed to parse field [{name}] of type [date] in document with id '{id}': {why}")))?;
                row.insert(name.clone(), json!(strata::timestamp::format(t)));
            }
            Kind::Vector { dim } => {
                let Some(Json::Array(xs)) = take_path(&mut source, &f.source) else {
                    return Err(OsError::mapper(format!("failed to parse field [{name}] of type [knn_vector]: a vector is an array of numbers, at a path of objects")));
                };
                if xs.len() != *dim || !xs.iter().all(Json::is_number) {
                    return Err(OsError::mapper(format!("Vector dimension mismatch. Expected: {dim}, Given: {} (field [{name}])", xs.len())));
                }
                row.insert(name.clone(), Json::Array(xs));
            }
            Kind::Long | Kind::Double => {
                for v in &vals {
                    let ok = v.is_number() || v.as_str().is_some_and(|s| s.trim().parse::<f64>().is_ok());
                    if !ok {
                        return Err(OsError::mapper(format!("failed to parse field [{name}] of type [{}] in document with id '{id}'. Preview of field's value: '{v}'", f.ty)));
                    }
                }
            }
            Kind::Bool => {
                for v in &vals {
                    let ok = v.is_boolean() || matches!(v.as_str(), Some("true" | "false" | ""));
                    if !ok {
                        return Err(OsError::mapper(format!("failed to parse field [{name}] of type [boolean] in document with id '{id}'. Preview of field's value: '{v}'")));
                    }
                }
            }
            Kind::Keyword | Kind::Other => {}
        }
    }
    // Strings kept in their text column alone.
    for name in ix.kept_apart() {
        if matches!(ix.fields[&name].kind, Kind::Text { .. }) {
            if matches!(peek_path(&source, &name), Some(Json::String(_))) {
                take_path(&mut source, &name);
            }
        }
    }
    check_limits(&source)?;
    row.insert(SOURCE.into(), Json::Object(source));
    Ok(Json::Object(row))
}

fn peek_path<'a>(doc: &'a Map<String, Json>, p: &str) -> Option<&'a Json> {
    let keys: Vec<&str> = p.split('.').collect();
    let mut m = doc;
    for k in &keys[..keys.len() - 1] {
        m = m.get(*k)?.as_object()?;
    }
    m.get(*keys.last()?)
}

/// The document of a row read with [`Index::read_columns`]: `_source`,
/// with the strings and vectors kept apart put back.
pub fn doc_of(ix: &Index, columns: &[String], row: &[Json]) -> (String, i64, Json) {
    let get = |c: &str| columns.iter().position(|x| x == c).and_then(|i| row.get(i));
    let id = get(ID).and_then(Json::as_str).unwrap_or_default().to_string();
    let version = get(VERSION).and_then(Json::as_i64).unwrap_or(1);
    let mut source = match get(SOURCE) {
        Some(Json::String(s)) => serde_json::from_str::<Map<String, Json>>(s).unwrap_or_default(),
        Some(Json::Object(m)) => m.clone(),
        _ => Map::new(),
    };
    for name in ix.kept_apart() {
        let v = match get(&name) {
            Some(Json::Null) | None => continue,
            Some(Json::Array(xs)) => Json::Array(xs.iter().map(|x| x.as_f64().map(|f| json!(shortest(f as f32))).unwrap_or(Json::Null)).collect()),
            Some(v) => v.clone(),
        };
        put_path(&mut source, &ix.fields[&name].source, v);
    }
    (id, version, Json::Object(source))
}

/// A float32 as the shortest decimal that reads back as it, as a double:
/// `0.1`, not `0.10000000149011612`.
fn shortest(x: f32) -> f64 {
    x.to_string().parse::<f64>().unwrap_or(x as f64)
}

/// Refuses a document the JSON field would ([`strata::json`]), naming the
/// limit, before it reaches the writer.
fn check_limits(doc: &Map<String, Json>) -> Result<(), OsError> {
    fn walk(v: &Json, path: usize, depth: usize, n: &mut usize) -> Result<(), OsError> {
        *n += 1;
        if *n > MAX_VALUES {
            return Err(OsError::mapper(format!("Strata keeps at most {MAX_VALUES} values and objects in a document")));
        }
        match v {
            Json::Object(m) => {
                if depth > MAX_DEPTH {
                    return Err(OsError::mapper(format!("Strata keeps objects nested at most {MAX_DEPTH} deep")));
                }
                for (k, x) in m {
                    if k.contains('\0') {
                        return Err(OsError::mapper("a field name holds the character \\0"));
                    }
                    let len = path + if path > 0 { 1 } else { 0 } + strata::json::escape(k).len();
                    if len > MAX_PATH {
                        return Err(OsError::mapper(format!("Strata keeps field paths of at most {MAX_PATH} bytes")));
                    }
                    walk(x, len, depth + 1, n)?;
                }
            }
            Json::Array(xs) => {
                for x in xs {
                    walk(x, path, depth, n)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut n = 0;
    for (k, x) in doc {
        if k.is_empty() {
            return Err(OsError::mapper("field name cannot be an empty string"));
        }
        let len = strata::json::escape(k).len();
        if len > MAX_PATH {
            return Err(OsError::mapper(format!("Strata keeps field paths of at most {MAX_PATH} bytes")));
        }
        walk(x, len, 1, &mut n)?;
    }
    Ok(())
}

// ------------------------------------------------------------------ dynamic mapping

/// Fields of `doc` the mapping lacks, as `dynamic: true` adds them:
/// strings `text` with a `keyword` sub-field (or `date`, if they read as
/// one), whole numbers `long`, others `float`, booleans `boolean`,
/// objects their fields. Returns the new properties, merged into the
/// mapping's, or `None` when nothing is new.
pub fn new_fields(ix: &Index, doc: &Map<String, Json>) -> Option<Json> {
    fn walk(m: &Map<String, Json>, prefix: &str, ix: &Index, props: &mut Map<String, Json>, changed: &mut bool) {
        for (k, v) in m {
            let path = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
            let first = match v {
                Json::Array(xs) => match xs.iter().find(|x| !x.is_null()) {
                    Some(x) => x,
                    None => continue,
                },
                v => v,
            };
            if let Json::Object(sub) = first {
                // An object mapped as something else (a vector's parent
                // cannot be one) is left as it is.
                if ix.fields.contains_key(&path) {
                    continue;
                }
                let entry = props.entry(k.clone()).or_insert_with(|| json!({"properties": {}}));
                if entry.get("type").is_some_and(|t| t != "object" && t != "nested") {
                    continue;
                }
                if !entry.get("properties").is_some_and(Json::is_object) {
                    entry["properties"] = json!({});
                }
                let inner = entry["properties"].as_object_mut().expect("properties");
                walk(sub, &path, ix, inner, changed);
                continue;
            }
            if ix.fields.contains_key(&path) || props.contains_key(k) {
                continue;
            }
            let spec = match first {
                Json::String(s) if looks_like_date(s) => json!({"type": "date"}),
                Json::String(_) => json!({"type": "text", "fields": {"keyword": {"type": "keyword", "ignore_above": 256}}}),
                Json::Number(n) if n.is_i64() || n.is_u64() => json!({"type": "long"}),
                Json::Number(_) => json!({"type": "float"}),
                Json::Bool(_) => json!({"type": "boolean"}),
                _ => continue,
            };
            props.insert(k.clone(), spec);
            *changed = true;
        }
    }
    let mut mappings = ix.mappings.clone();
    if !mappings.is_object() {
        mappings = json!({});
    }
    if !mappings.get("properties").is_some_and(Json::is_object) {
        mappings["properties"] = json!({});
    }
    let mut changed = false;
    walk(doc, "", ix, mappings["properties"].as_object_mut().expect("properties"), &mut changed);
    changed.then_some(mappings)
}

/// Whether a string reads as a date by OpenSearch's default dynamic date
/// detection (`strict_date_optional_time`): `2026-11-08`,
/// `2026-11-08T10:00:00Z`, `2026-11-08T10:00:00.123+02:00`.
fn looks_like_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 10 || !b[..4].iter().all(u8::is_ascii_digit) || b[4] != b'-' || !b[5..7].iter().all(u8::is_ascii_digit) || b[7] != b'-' || !b[8..10].iter().all(u8::is_ascii_digit) {
        return false;
    }
    b.len() == 10 || ((b[10] == b'T' || b[10] == b't') && strata::timestamp::parse(s).is_some())
}

/// A date as a field holds it: ISO 8601 text (a date, or a date and a
/// time), or milliseconds since 1970 (`epoch_millis`), as a number or
/// digits. Microseconds since 1970.
pub fn parse_date(v: &Json) -> Result<i64, String> {
    match v {
        Json::Number(n) => {
            let ms = n.as_f64().ok_or("not a number")?;
            Ok((ms * 1000.0).round() as i64)
        }
        Json::String(s) => {
            let t = s.trim();
            if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit() || c == '-') && !t[1..].contains('-') {
                let ms: i64 = t.parse().map_err(|_| format!("[{s}] is not milliseconds"))?;
                return Ok(ms.saturating_mul(1000));
            }
            strata::timestamp::parse(t).ok_or_else(|| format!("[{s}] is not a date (ISO 8601, or milliseconds since 1970)"))
        }
        other => Err(format!("[{other}] is not a date")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_of_names() {
        assert_eq!(table_of("books"), "os_books");
        let t = table_of("gitea_codes.v1");
        assert!(t.starts_with("os_gitea_codes_v1_"), "{t}");
        assert_ne!(table_of("a.b"), table_of("a-b"));
    }

    #[test]
    fn dots_expand() {
        let d = expand_dots(serde_json::from_str(r#"{"a.b": 1, "a": {"c": 2}, "x": [{"y.z": 3}]}"#).unwrap()).unwrap();
        assert_eq!(Json::Object(d), json!({"a": {"b": 1, "c": 2}, "x": [{"y": {"z": 3}}]}));
    }

    #[test]
    fn text_kept_apart_and_back() {
        let ix = Index::new(
            "i",
            "os_i",
            "u",
            0,
            json!({"properties": {"title": {"type": "text", "fields": {"raw": {"type": "keyword"}}}, "body": {"type": "text", "analyzer": "english"}, "at": {"type": "date"}, "v": {"type": "knn_vector", "dimension": 2}, "n": {"type": "long"}}}),
            json!({}),
            vec![],
        )
        .unwrap();
        assert_eq!(ix.fields["body"].kind, Kind::Text { analyzer: "english" });
        assert_eq!(ix.fields["title.raw"].source, "title");
        assert!(!ix.stripped_text("title"));
        assert!(ix.stripped_text("body"));
        let doc: Map<String, Json> = serde_json::from_value(json!({"title": "Hi", "body": "long words", "at": "2026-11-08", "v": [0.1, 0.2], "n": 5})).unwrap();
        let row = row_of(&ix, "1", 1, &doc).unwrap();
        assert_eq!(row["_source"], json!({"title": "Hi", "at": "2026-11-08", "n": 5}));
        assert_eq!(row["body"], json!("long words"));
        assert_eq!(row["at"], json!("2026-11-08T00:00:00Z"));
        let cols = ix.read_columns();
        let vals: Vec<Json> = cols.iter().map(|c| if c == "_source" { json!(row["_source"].to_string()) } else { row[c].clone() }).collect();
        let (id, v, back) = doc_of(&ix, &cols, &vals);
        assert_eq!((id.as_str(), v), ("1", 1));
        assert_eq!(back, Json::Object(doc));
    }

    #[test]
    fn dynamic_fields() {
        let ix = Index::new("i", "os_i", "u", 0, json!({"properties": {"a": {"type": "keyword"}}}), json!({}), vec![]).unwrap();
        let doc: Map<String, Json> = serde_json::from_value(json!({"a": "x", "b": "words", "c": 3, "d": 1.5, "e": true, "f": {"g": "2026-01-01"}, "h": [1, 2]})).unwrap();
        let m = new_fields(&ix, &doc).unwrap();
        let p = &m["properties"];
        assert_eq!(p["b"]["type"], "text");
        assert_eq!(p["b"]["fields"]["keyword"]["type"], "keyword");
        assert_eq!(p["c"]["type"], "long");
        assert_eq!(p["d"]["type"], "float");
        assert_eq!(p["e"]["type"], "boolean");
        assert_eq!(p["f"]["properties"]["g"]["type"], "date");
        assert_eq!(p["h"]["type"], "long");
        let ix2 = Index::new("i", "os_i", "u", 0, m, json!({}), vec![]).unwrap();
        assert!(new_fields(&ix2, &doc).is_none());
    }

    #[test]
    fn dates() {
        assert_eq!(parse_date(&json!(1000)).unwrap(), 1_000_000);
        assert_eq!(parse_date(&json!("1000")).unwrap(), 1_000_000);
        assert!(parse_date(&json!("2026-11-08T10:00:00Z")).is_ok());
        assert!(parse_date(&json!("nope")).is_err());
        assert!(looks_like_date("2026-11-08"));
        assert!(!looks_like_date("2026-11-08 is a day"));
    }
}
