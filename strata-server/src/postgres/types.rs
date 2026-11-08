//! Postgres's types and values as Strata's: the column a Postgres column
//! becomes in a table the follower makes, a value's text (as COPY and
//! pgoutput write it) as the JSON a write takes for a Strata field, and the
//! text formats of COPY rows and array literals.
//!
//! | Postgres | Strata |
//! |---|---|
//! | `boolean` | `BOOL` |
//! | `smallint`, `integer`, `bigint`, `oid` | `INT` |
//! | `real`, `double precision`, `numeric` | `FLOAT` |
//! | `text`, `varchar`, `name`, `citext`, `xml` | `TEXT` (`KEYWORD` as the key) |
//! | `char(n)`, `uuid`, enums, and other types | `KEYWORD` (their text) |
//! | `date`, `timestamp`, `timestamptz` | `TIMESTAMP` (UTC) |
//! | `json`, `jsonb` | `JSON` (a value that is not an object as `{"value": ...}`) |
//! | arrays of text types, `uuid` or enums | `KEYWORD[]` |
//! | arrays of integers | `INT[]` |
//! | pgvector's `vector(n)`, `halfvec(n)` | `VECTOR(n)` |
//! | pgvector's `sparsevec` | `SPARSE` |
//! | `bytea`, `tsvector`, other arrays, `vector` without a dimension | left out |
//!
//! A table made in Strata beforehand chooses its own types: a value is
//! written as its field's type reads it (an `integer` into a `KEYWORD`, a
//! `real[]` into a `VECTOR(n)`), and one it cannot read stops the follower.

use serde_json::{json, Value as Json};
use strata::schema::{Field, FieldType, Logical};

/// A column's type, through domains to the type they constrain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PgType {
    pub name: String,
    pub enumerated: bool,
    /// An array's element type.
    pub elem: Option<Box<PgType>>,
}

impl PgType {
    pub fn named(name: &str) -> PgType {
        PgType { name: name.to_string(), enumerated: false, elem: None }
    }
}

fn textual(t: &PgType) -> bool {
    matches!(t.name.as_str(), "text" | "varchar" | "name" | "citext" | "bpchar" | "char" | "uuid") || t.enumerated
}

fn integral(t: &PgType) -> bool {
    matches!(t.name.as_str(), "int2" | "int4" | "int8" | "oid")
}

/// The Strata type a column of type `t` (with its `typmod`) becomes: the
/// key's, when `key`; `None` for one left out.
pub fn strata_type(t: &PgType, typmod: i32, key: bool) -> Option<String> {
    if key {
        return Some(if integral(t) { "INT" } else { "KEYWORD" }.into());
    }
    if let Some(e) = &t.elem {
        return match () {
            _ if integral(e) => Some("INT[]".into()),
            _ if textual(e) => Some("KEYWORD[]".into()),
            _ => None,
        };
    }
    if t.enumerated {
        return Some("KEYWORD".into());
    }
    Some(
        match t.name.as_str() {
            "bool" => "BOOL",
            "int2" | "int4" | "int8" | "oid" => "INT",
            "float4" | "float8" | "numeric" => "FLOAT",
            "text" | "varchar" | "name" | "citext" | "xml" => "TEXT",
            "date" | "timestamp" | "timestamptz" => "TIMESTAMP",
            "json" | "jsonb" => "JSON",
            "vector" | "halfvec" if typmod > 0 => return Some(format!("VECTOR({typmod})")),
            "sparsevec" => "SPARSE",
            "vector" | "halfvec" | "bytea" | "tsvector" | "tsquery" => return None,
            _ => "KEYWORD",
        }
        .into(),
    )
}

/// The JSON a write takes for `field` from a value's text, of Postgres type `t`.
pub fn json_of(field: &Field, t: &PgType, text: &str) -> Result<Json, String> {
    let bad = |why: &str| format!("{text:?} (a Postgres {}) {why}", t.name);
    match field.logical {
        Some(Logical::Bool) => {
            return match text {
                "t" | "true" | "1" | "yes" | "on" => Ok(json!(true)),
                "f" | "false" | "0" | "no" | "off" => Ok(json!(false)),
                _ => Err(bad("is not a BOOL")),
            }
        }
        Some(Logical::Timestamp) => {
            // Strata's timestamps are microseconds within years 0 to 9999.
            return match text {
                "infinity" | "-infinity" => Ok(Json::Null),
                t if t.ends_with(" BC") => Err(bad("is before year 1, which a Strata TIMESTAMP does not hold")),
                t => Ok(json!(t)),
            };
        }
        None => {}
    }
    if field.is_multi_vector() {
        return Err(bad("goes into a multi-vector field, which the follower does not write"));
    }
    if field.multi {
        let items: Vec<String> = match t.elem {
            Some(_) => array_items(text)?.into_iter().flatten().collect(),
            None => vec![text.to_string()],
        };
        return Ok(match field.ty {
            FieldType::Int => Json::Array(items.iter().map(|s| int_of(s).map(Json::from).ok_or_else(|| bad("holds a value that is not an INT"))).collect::<Result<_, _>>()?),
            _ => Json::Array(items.into_iter().map(Json::String).collect()),
        });
    }
    Ok(match &field.ty {
        FieldType::Int => match text {
            "t" | "true" => json!(1),
            "f" | "false" => json!(0),
            s => json!(int_of(s).ok_or_else(|| bad("is not an INT"))?),
        },
        FieldType::Float => match text.parse::<f64>() {
            Ok(x) if x.is_finite() => json!(x),
            // NaN and the infinities, which JSON does not write.
            Ok(_) => Json::Null,
            Err(_) => return Err(bad("is not a FLOAT")),
        },
        FieldType::Vector { .. } => Json::Array(floats(text).ok_or_else(|| bad("is not a vector ([1,2,3] or {1,2,3})"))?.into_iter().map(Json::from).collect()),
        FieldType::Json => match serde_json::from_str::<Json>(text) {
            Ok(o @ Json::Object(_)) => o,
            Ok(Json::Null) => Json::Null,
            Ok(v) => json!({"value": v}),
            Err(_) => return Err(bad("is not JSON")),
        },
        FieldType::Sparse => sparse(text).ok_or_else(|| bad("is not a sparse vector ({1:0.5,7:0.25}/10, or a JSON object)"))?,
        FieldType::Text { .. } | FieldType::Keyword | FieldType::Edges => json!(text),
    })
}

/// An INT from text: digits, or a number with no fraction (`12.00`).
fn int_of(s: &str) -> Option<i64> {
    s.parse::<i64>().ok().or_else(|| s.parse::<f64>().ok().filter(|x| x.fract() == 0.0 && x.abs() < 9.0e18).map(|x| x as i64))
}

/// A vector's numbers: pgvector's `[1,2,3]`, or an array's `{1,2,3}`.
fn floats(text: &str) -> Option<Vec<f64>> {
    let t = text.trim();
    let inner = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')).or_else(|| t.strip_prefix('{').and_then(|s| s.strip_suffix('}')))?;
    if inner.trim().is_empty() {
        return Some(Vec::new());
    }
    inner.split(',').map(|x| x.trim().parse::<f64>().ok()).collect()
}

/// pgvector's `sparsevec` text, `{1:0.5,7:0.25}/10`, as Strata's SPARSE
/// object; or JSON text of an object.
fn sparse(text: &str) -> Option<Json> {
    if let Ok(o @ Json::Object(_)) = serde_json::from_str::<Json>(text) {
        return Some(o);
    }
    let inner = text.trim().split_once('/').map_or(text, |(a, _)| a).trim();
    let inner = inner.strip_prefix('{')?.strip_suffix('}')?;
    let mut m = serde_json::Map::new();
    for kv in inner.split(',').filter(|s| !s.trim().is_empty()) {
        let (k, v) = kv.split_once(':')?;
        m.insert(k.trim().to_string(), json!(v.trim().parse::<f64>().ok()?));
    }
    Some(Json::Object(m))
}

/// The elements of an array literal, `{a,"b c",NULL}`, of any number of
/// dimensions, in order (`None` for NULL).
pub fn array_items(text: &str) -> Result<Vec<Option<String>>, String> {
    let bad = || format!("{text:?} is not an array literal");
    let mut s = text.trim();
    // Bounds, as [0:2]={...}.
    if s.starts_with('[') {
        s = s.split_once('=').ok_or_else(bad)?.1;
    }
    let b: Vec<char> = s.chars().collect();
    if b.first() != Some(&'{') {
        return Err(bad());
    }
    let mut out = Vec::new();
    let mut i = 0;
    let mut depth = 0;
    while i < b.len() {
        match b[i] {
            '{' => {
                depth += 1;
                i += 1;
            }
            '}' => {
                depth -= 1;
                i += 1;
            }
            ',' | ' ' => i += 1,
            '"' => {
                let mut v = String::new();
                i += 1;
                while i < b.len() && b[i] != '"' {
                    if b[i] == '\\' && i + 1 < b.len() {
                        i += 1;
                    }
                    v.push(b[i]);
                    i += 1;
                }
                if i >= b.len() {
                    return Err(bad());
                }
                i += 1;
                out.push(Some(v));
            }
            _ => {
                let mut v = String::new();
                while i < b.len() && !matches!(b[i], ',' | '}') {
                    if b[i] == '\\' && i + 1 < b.len() {
                        i += 1;
                    }
                    v.push(b[i]);
                    i += 1;
                }
                let v = v.trim_end().to_string();
                out.push(if v.eq_ignore_ascii_case("NULL") { None } else { Some(v) });
            }
        }
    }
    if depth != 0 {
        return Err(bad());
    }
    Ok(out)
}

/// The fields of a row of `COPY ... TO STDOUT` in text format (without its
/// newline): tab-separated, `\N` for NULL, with backslash escapes.
pub fn copy_fields(line: &[u8]) -> Result<Vec<Option<String>>, String> {
    let mut out = Vec::new();
    for raw in line.split(|&c| c == b'\t') {
        if raw == b"\\N" {
            out.push(None);
            continue;
        }
        let mut v = Vec::with_capacity(raw.len());
        let mut i = 0;
        while i < raw.len() {
            let c = raw[i];
            if c != b'\\' || i + 1 == raw.len() {
                v.push(c);
                i += 1;
                continue;
            }
            i += 1;
            match raw[i] {
                b'b' => v.push(8),
                b'f' => v.push(12),
                b'n' => v.push(b'\n'),
                b'r' => v.push(b'\r'),
                b't' => v.push(b'\t'),
                b'v' => v.push(11),
                b'0'..=b'7' => {
                    let mut n = 0u32;
                    let mut k = 0;
                    while k < 3 && i < raw.len() && (b'0'..=b'7').contains(&raw[i]) {
                        n = n * 8 + (raw[i] - b'0') as u32;
                        i += 1;
                        k += 1;
                    }
                    v.push(n as u8);
                    continue;
                }
                b'x' if i + 1 < raw.len() && raw[i + 1].is_ascii_hexdigit() => {
                    i += 1;
                    let mut n = 0u32;
                    let mut k = 0;
                    while k < 2 && i < raw.len() && raw[i].is_ascii_hexdigit() {
                        n = n * 16 + (raw[i] as char).to_digit(16).unwrap_or(0);
                        i += 1;
                        k += 1;
                    }
                    v.push(n as u8);
                    continue;
                }
                other => v.push(other),
            }
            i += 1;
        }
        out.push(Some(String::from_utf8(v).map_err(|_| "a COPY value that is not UTF-8".to_string())?));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(sql_type: &str) -> Field {
        match strata::query::bind_command(&format!("CREATE TABLE t (id INT PRIMARY KEY, f {sql_type})"), &[], &strata::query::Params::new()).unwrap() {
            strata::query::Command::CreateTable { schema, .. } => schema.fields[1].clone(),
            _ => unreachable!(),
        }
    }

    #[test]
    fn types_map_to_strata_columns() {
        let t = |n: &str| PgType::named(n);
        assert_eq!(strata_type(&t("int8"), -1, false).as_deref(), Some("INT"));
        assert_eq!(strata_type(&t("text"), -1, true).as_deref(), Some("KEYWORD"));
        assert_eq!(strata_type(&t("int4"), -1, true).as_deref(), Some("INT"));
        assert_eq!(strata_type(&t("varchar"), 259, false).as_deref(), Some("TEXT"));
        assert_eq!(strata_type(&t("uuid"), -1, false).as_deref(), Some("KEYWORD"));
        assert_eq!(strata_type(&t("timestamptz"), -1, false).as_deref(), Some("TIMESTAMP"));
        assert_eq!(strata_type(&t("vector"), 3, false).as_deref(), Some("VECTOR(3)"));
        assert_eq!(strata_type(&t("vector"), -1, false), None);
        assert_eq!(strata_type(&t("bytea"), -1, false), None);
        let arr = PgType { elem: Some(Box::new(t("text"))), ..t("_text") };
        assert_eq!(strata_type(&arr, -1, false).as_deref(), Some("KEYWORD[]"));
        let arr = PgType { elem: Some(Box::new(t("float4"))), ..t("_float4") };
        assert_eq!(strata_type(&arr, -1, false), None);
        let e = PgType { enumerated: true, ..t("mood") };
        assert_eq!(strata_type(&e, -1, false).as_deref(), Some("KEYWORD"));
    }

    #[test]
    fn values_read_as_their_fields_type() {
        let t = |n: &str| PgType::named(n);
        assert_eq!(json_of(&field("BOOL"), &t("bool"), "t").unwrap(), json!(true));
        assert_eq!(json_of(&field("INT"), &t("numeric"), "12.00").unwrap(), json!(12));
        assert!(json_of(&field("INT"), &t("text"), "x").is_err());
        assert_eq!(json_of(&field("FLOAT"), &t("float8"), "NaN").unwrap(), Json::Null);
        assert_eq!(json_of(&field("FLOAT"), &t("numeric"), "1.5").unwrap(), json!(1.5));
        assert_eq!(json_of(&field("TIMESTAMP"), &t("timestamptz"), "2024-01-02 03:04:05.5+00").unwrap(), json!("2024-01-02 03:04:05.5+00"));
        assert_eq!(json_of(&field("TIMESTAMP"), &t("date"), "infinity").unwrap(), Json::Null);
        assert_eq!(json_of(&field("VECTOR(3)"), &t("vector"), "[1,2.5,-3]").unwrap(), json!([1.0, 2.5, -3.0]));
        let real_arr = PgType { elem: Some(Box::new(t("float4"))), ..t("_float4") };
        assert_eq!(json_of(&field("VECTOR(2)"), &real_arr, "{0.5,1}").unwrap(), json!([0.5, 1.0]));
        let text_arr = PgType { elem: Some(Box::new(t("text"))), ..t("_text") };
        assert_eq!(json_of(&field("KEYWORD[]"), &text_arr, r#"{a,"b c",NULL,"q\"t"}"#).unwrap(), json!(["a", "b c", "q\"t"]));
        let int_arr = PgType { elem: Some(Box::new(t("int4"))), ..t("_int4") };
        assert_eq!(json_of(&field("INT[]"), &int_arr, "{{1,2},{3,4}}").unwrap(), json!([1, 2, 3, 4]));
        assert_eq!(json_of(&field("JSON"), &t("jsonb"), r#"{"a": 1}"#).unwrap(), json!({"a": 1}));
        assert_eq!(json_of(&field("JSON"), &t("jsonb"), "[1, 2]").unwrap(), json!({"value": [1, 2]}));
        assert_eq!(json_of(&field("SPARSE"), &t("sparsevec"), "{1:0.5,7:0.25}/10").unwrap(), json!({"1": 0.5, "7": 0.25}));
        assert_eq!(json_of(&field("KEYWORD"), &t("int4"), "7").unwrap(), json!("7"));
    }

    #[test]
    fn copy_rows_unescape() {
        assert_eq!(copy_fields(b"1\ta\\tb\\nc\t\\N\t\\\\x\t\\101\\x42").unwrap(), vec![Some("1".into()), Some("a\tb\nc".into()), None, Some("\\x".into()), Some("AB".into())]);
        assert_eq!(copy_fields(b"").unwrap(), vec![Some(String::new())]);
    }

    #[test]
    fn array_literals_split() {
        assert_eq!(array_items("{}").unwrap(), Vec::<Option<String>>::new());
        assert_eq!(array_items("[0:1]={x,y}").unwrap(), vec![Some("x".into()), Some("y".into())]);
        assert_eq!(array_items(r#"{"a,b",null,"NULL"}"#).unwrap(), vec![Some("a,b".into()), None, Some("NULL".into())]);
        assert!(array_items("{a").is_err());
        assert!(array_items("x").is_err());
    }
}
