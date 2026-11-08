//! The values that cross between Java and Rust: one tagged binary form,
//! little-endian, which `ai.chloetech.strata.Codec` writes and reads on the
//! Java side. A call's arguments are one value (a list) and its answer is
//! another, so the native boundary is a few functions taking and giving
//! bytes, whatever the method, and no JNI call is made per value.
//!
//! | tag | value | then |
//! |---|---|---|
//! | 0 | null | |
//! | 1, 2 | false, true | |
//! | 3 | integer | i64 |
//! | 4 | float | f64 |
//! | 5 | string | u32 length, UTF-8 bytes |
//! | 6 | vector | u32 count, f32 each |
//! | 7 | list | u32 count, values |
//! | 8 | map | u32 count, (u32 length, UTF-8 key, value) each |
//! | 9 | time | i64 microseconds since 1970-01-01T00:00:00Z |
//! | 10 | bytes | u32 length, bytes |

use serde_json::{Map, Number, Value as Json};
use strata::Value;

const NULL: u8 = 0;
const FALSE: u8 = 1;
const TRUE: u8 = 2;
const INT: u8 = 3;
const FLOAT: u8 = 4;
const STR: u8 = 5;
const VECTOR: u8 = 6;
const LIST: u8 = 7;
const MAP: u8 = 8;
const TIME: u8 = 9;
const BYTES: u8 = 10;

/// A value from Java.
#[derive(Debug, Clone, PartialEq)]
pub enum V {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// A `float[]` or `double[]`.
    Vector(Vec<f32>),
    List(Vec<V>),
    /// In the map's order.
    Map(Vec<(String, V)>),
    /// An `Instant` (or a date or time Java turned into one).
    Time(i64),
    /// A `byte[]`.
    Bytes(Vec<u8>),
}

/// Reads one value from `b`, which must hold nothing after it.
pub fn decode(b: &[u8]) -> Result<V, String> {
    let mut r = Reader { b, at: 0 };
    let v = r.value(0)?;
    if r.at != b.len() {
        return Err(format!("{} bytes after the value", b.len() - r.at));
    }
    Ok(v)
}

/// Nesting deeper than this is refused rather than overflowing the stack.
const DEPTH: usize = 128;

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.b.len()).ok_or_else(|| format!("the value ends early, at byte {}", self.b.len()))?;
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<usize, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("four bytes")) as usize)
    }

    fn i64(&mut self) -> Result<i64, String> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().expect("eight bytes")))
    }

    fn str(&mut self) -> Result<String, String> {
        let n = self.u32()?;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| "a string is not UTF-8".to_string())
    }

    /// A count of items each at least `min` bytes long, checked against
    /// what is left, so that a damaged count cannot ask for gigabytes.
    fn count(&mut self, min: usize) -> Result<usize, String> {
        let n = self.u32()?;
        if n.saturating_mul(min) > self.b.len() - self.at {
            return Err(format!("a count of {n} is more than the bytes left hold"));
        }
        Ok(n)
    }

    fn value(&mut self, depth: usize) -> Result<V, String> {
        if depth > DEPTH {
            return Err(format!("values nest deeper than {DEPTH}"));
        }
        Ok(match self.u8()? {
            NULL => V::Null,
            FALSE => V::Bool(false),
            TRUE => V::Bool(true),
            INT => V::Int(self.i64()?),
            FLOAT => V::Float(f64::from_bits(self.i64()? as u64)),
            STR => V::Str(self.str()?),
            VECTOR => {
                let n = self.count(4)?;
                V::Vector(self.take(n * 4)?.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().expect("four bytes"))).collect())
            }
            LIST => {
                let n = self.count(1)?;
                V::List((0..n).map(|_| self.value(depth + 1)).collect::<Result<_, _>>()?)
            }
            MAP => {
                let n = self.count(5)?;
                V::Map((0..n).map(|_| Ok((self.str()?, self.value(depth + 1)?))).collect::<Result<_, String>>()?)
            }
            TIME => V::Time(self.i64()?),
            BYTES => {
                let n = self.count(1)?;
                V::Bytes(self.take(n)?.to_vec())
            }
            t => return Err(format!("unknown tag {t} at byte {}", self.at - 1)),
        })
    }
}

impl V {
    /// As JSON, the form rows, schemas and options are read in: a vector is
    /// a list of numbers, a time its ISO 8601 text.
    pub fn to_json(&self) -> Result<Json, String> {
        Ok(match self {
            V::Null => Json::Null,
            V::Bool(b) => Json::Bool(*b),
            V::Int(i) => Json::Number((*i).into()),
            V::Float(f) => Number::from_f64(*f).map_or(Json::Null, Json::Number),
            V::Str(s) => Json::String(s.clone()),
            V::Vector(xs) => Json::Array(xs.iter().map(|&x| Number::from_f64(x as f64).map_or(Json::Null, Json::Number)).collect()),
            V::List(xs) => Json::Array(xs.iter().map(V::to_json).collect::<Result<_, _>>()?),
            V::Map(kv) => {
                let mut m = Map::new();
                for (k, v) in kv {
                    m.insert(k.clone(), v.to_json()?);
                }
                Json::Object(m)
            }
            V::Time(t) => Json::String(strata::timestamp::format(*t)),
            V::Bytes(_) => return Err("bytes are only an image or a sound to embed, not a value to store".into()),
        })
    }

    pub fn is_null(&self) -> bool {
        matches!(self, V::Null)
    }

    /// The value's kind, for errors.
    pub fn kind(&self) -> &'static str {
        match self {
            V::Null => "null",
            V::Bool(_) => "a boolean",
            V::Int(_) => "an integer",
            V::Float(_) => "a float",
            V::Str(_) => "a string",
            V::Vector(_) => "a vector",
            V::List(_) => "a list",
            V::Map(_) => "a map",
            V::Time(_) => "a time",
            V::Bytes(_) => "bytes",
        }
    }
}

/// Writes values for Java to read.
#[derive(Default)]
pub struct Writer {
    pub b: Vec<u8>,
}

impl Writer {
    fn len(&mut self, n: usize) {
        self.b.extend_from_slice(&(n as u32).to_le_bytes());
    }

    pub fn null(&mut self) {
        self.b.push(NULL);
    }

    pub fn bool(&mut self, x: bool) {
        self.b.push(if x { TRUE } else { FALSE });
    }

    pub fn int(&mut self, x: i64) {
        self.b.push(INT);
        self.b.extend_from_slice(&x.to_le_bytes());
    }

    pub fn float(&mut self, x: f64) {
        self.b.push(FLOAT);
        self.b.extend_from_slice(&x.to_le_bytes());
    }

    pub fn str(&mut self, s: &str) {
        self.b.push(STR);
        self.key(s);
    }

    /// A map's key: a string without its tag.
    pub fn key(&mut self, s: &str) {
        self.len(s.len());
        self.b.extend_from_slice(s.as_bytes());
    }

    pub fn vector(&mut self, xs: &[f32]) {
        self.b.push(VECTOR);
        self.len(xs.len());
        for x in xs {
            self.b.extend_from_slice(&x.to_le_bytes());
        }
    }

    /// A list of `n` values, which the caller writes next.
    pub fn list(&mut self, n: usize) {
        self.b.push(LIST);
        self.len(n);
    }

    /// A map of `n` entries, each a [`Writer::key`] and a value, which the
    /// caller writes next.
    pub fn map(&mut self, n: usize) {
        self.b.push(MAP);
        self.len(n);
    }

    pub fn time(&mut self, micros: i64) {
        self.b.push(TIME);
        self.b.extend_from_slice(&micros.to_le_bytes());
    }

    /// A stored value.
    pub fn value(&mut self, v: &Value) {
        match v {
            Value::Null => self.null(),
            Value::Int(i) => self.int(*i),
            Value::Float(f) => self.float(*f),
            Value::Str(s) => self.str(s),
            Value::Vector(xs) => self.vector(xs),
            Value::List(xs) => {
                self.list(xs.len());
                xs.iter().for_each(|x| self.value(x));
            }
            Value::Bool(b) => self.bool(*b),
            Value::Timestamp(t) => self.time(*t),
        }
    }

    /// A value from Java, as it came.
    pub fn v(&mut self, v: &V) {
        match v {
            V::Null => self.null(),
            V::Bool(b) => self.bool(*b),
            V::Int(i) => self.int(*i),
            V::Float(f) => self.float(*f),
            V::Str(s) => self.str(s),
            V::Vector(xs) => self.vector(xs),
            V::List(xs) => {
                self.list(xs.len());
                xs.iter().for_each(|x| self.v(x));
            }
            V::Map(kv) => {
                self.map(kv.len());
                for (k, x) in kv {
                    self.key(k);
                    self.v(x);
                }
            }
            V::Time(t) => self.time(*t),
            V::Bytes(b) => {
                self.b.push(BYTES);
                self.len(b.len());
                self.b.extend_from_slice(b);
            }
        }
    }

    /// JSON: objects become maps, arrays lists.
    pub fn json(&mut self, j: &Json) {
        match j {
            Json::Null => self.null(),
            Json::Bool(b) => self.bool(*b),
            Json::Number(n) => match n.as_i64() {
                Some(i) => self.int(i),
                None => self.float(n.as_f64().unwrap_or(f64::NAN)),
            },
            Json::String(s) => self.str(s),
            Json::Array(xs) => {
                self.list(xs.len());
                xs.iter().for_each(|x| self.json(x));
            }
            Json::Object(m) => {
                self.map(m.len());
                for (k, v) in m {
                    self.key(k);
                    self.json(v);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_reads_back() {
        let mut w = Writer::default();
        w.map(3);
        w.key("a");
        w.list(4);
        w.null();
        w.bool(true);
        w.int(-7);
        w.float(2.5);
        w.key("v");
        w.vector(&[1.0, -0.5]);
        w.key("é");
        w.time(1_700_000_000_000_000);
        let v = decode(&w.b).unwrap();
        let mut again = Writer::default();
        again.v(&v);
        assert_eq!(again.b, w.b);
        assert_eq!(
            v,
            V::Map(vec![
                ("a".into(), V::List(vec![V::Null, V::Bool(true), V::Int(-7), V::Float(2.5)])),
                ("v".into(), V::Vector(vec![1.0, -0.5])),
                ("é".into(), V::Time(1_700_000_000_000_000)),
            ])
        );
    }

    #[test]
    fn damaged_bytes_are_refused() {
        assert!(decode(&[]).is_err());
        assert!(decode(&[42]).unwrap_err().contains("unknown tag"));
        // A list of four billion values in five bytes.
        assert!(decode(&[LIST, 0xff, 0xff, 0xff, 0xff]).unwrap_err().contains("count"));
        assert!(decode(&[STR, 3, 0, 0, 0, b'a']).unwrap_err().contains("ends early"));
        assert!(decode(&[NULL, NULL]).unwrap_err().contains("after"));
        let mut deep = vec![LIST, 1, 0, 0, 0].repeat(DEPTH + 2);
        deep.push(NULL);
        assert!(decode(&deep).unwrap_err().contains("deeper"));
    }

    #[test]
    fn json_keeps_integers_exact() {
        let j: Json = serde_json::json!({"n": 9007199254740993_i64, "x": 0.1, "s": ["a"]});
        let mut w = Writer::default();
        w.json(&j);
        assert_eq!(decode(&w.b).unwrap().to_json().unwrap(), j);
    }
}
