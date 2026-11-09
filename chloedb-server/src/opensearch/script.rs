//! The scripts of `script_score`, in the part of Painless that scoring
//! scripts are written in, as a Strata ranking: numbers, `params.x`,
//! `doc['f'].value` (a number's column), `_score`, `+ - * /`, the
//! functions of `Math` that a ranking has (`log10`, `log`, `log1p`,
//! `exp`, `sqrt`, `pow`, `abs`, `max`, `min`), Painless's `saturation`,
//! `sigmoid` and `decayNumeric*`, and an optional `return` and `;`.

use serde_json::{Map, Value as Json};

use super::dsl::{num, Ctx};
use super::OsError;

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Name(String),
    Sym(char),
}

fn tokens(src: &str) -> Result<Vec<Tok>, OsError> {
    let c: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch.is_ascii_digit() || (ch == '.' && c.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let s = i;
            while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.' || ((c[i] == 'e' || c[i] == 'E') && i > s) || ((c[i] == '-' || c[i] == '+') && matches!(c[i - 1], 'e' | 'E'))) {
                i += 1;
            }
            let text: String = c[s..i].iter().collect();
            // Painless's suffixes: 2L, 2.0f, 2d.
            if i < c.len() && matches!(c[i], 'L' | 'l' | 'f' | 'F' | 'd' | 'D') {
                i += 1;
            }
            out.push(Tok::Num(text.parse().map_err(|_| bad(src, &format!("the number {text}")))?));
        } else if ch.is_alphabetic() || ch == '_' {
            let s = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            out.push(Tok::Name(c[s..i].iter().collect()));
        } else if ch == '\'' || ch == '"' {
            let s = i + 1;
            i += 1;
            while i < c.len() && c[i] != ch {
                i += 1;
            }
            if i == c.len() {
                return Err(bad(src, "a string without its closing quote"));
            }
            out.push(Tok::Str(c[s..i].iter().collect()));
            i += 1;
        } else if "+-*/()[].,;".contains(ch) {
            out.push(Tok::Sym(ch));
            i += 1;
        } else {
            return Err(bad(src, &format!("the character '{ch}'")));
        }
    }
    Ok(out)
}

fn bad(src: &str, what: &str) -> OsError {
    OsError::new(400, "script_exception", format!("compile error in [{src}]: ChloeDB's script_score runs numbers, params, doc['field'].value, _score, + - * /, Math.log10/log/log1p/exp/sqrt/pow/abs/max/min, saturation, sigmoid and decayNumericGauss/Exp/Linear, not {what}"))
}

struct P<'a, 'b> {
    t: Vec<Tok>,
    at: usize,
    src: &'a str,
    params: &'a Map<String, Json>,
    cx: &'a Ctx<'b>,
    /// The query's score, as `_score` reads it.
    score: &'a str,
    /// Whether `_score` was read.
    read_score: bool,
}

/// A value of a script: a constant or SQL.
#[derive(Clone, Debug)]
enum V {
    K(f64),
    E(String),
}

impl V {
    fn sql(&self) -> String {
        match self {
            V::K(x) => num(*x),
            V::E(e) => e.clone(),
        }
    }
}

impl P<'_, '_> {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.at)
    }

    fn eat(&mut self, s: char) -> bool {
        if self.peek() == Some(&Tok::Sym(s)) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn want(&mut self, s: char) -> Result<(), OsError> {
        if self.eat(s) {
            Ok(())
        } else {
            Err(bad(self.src, &format!("{:?} where '{s}' goes", self.peek())))
        }
    }

    fn name(&mut self) -> Result<String, OsError> {
        match self.t.get(self.at).cloned() {
            Some(Tok::Name(n)) => {
                self.at += 1;
                Ok(n)
            }
            t => Err(bad(self.src, &format!("{t:?} where a name goes"))),
        }
    }

    fn expr(&mut self) -> Result<V, OsError> {
        let mut a = self.term()?;
        loop {
            let op = if self.eat('+') { '+' } else if self.eat('-') { '-' } else { return Ok(a) };
            let b = self.term()?;
            a = match (a, b) {
                (V::K(x), V::K(y)) => V::K(if op == '+' { x + y } else { x - y }),
                (x, y) => V::E(format!("{} {op} {}", x.sql(), paren(&y, op == '-'))),
            };
        }
    }

    fn term(&mut self) -> Result<V, OsError> {
        let mut a = self.unary()?;
        loop {
            let op = if self.eat('*') { '*' } else if self.eat('/') { '/' } else { return Ok(a) };
            let b = self.unary()?;
            a = match (a, b) {
                (V::K(x), V::K(y)) => V::K(if op == '*' { x * y } else { x / y }),
                (x, y) => V::E(format!("{} {op} {}", paren(&x, true), paren(&y, true))),
            };
        }
    }

    fn unary(&mut self) -> Result<V, OsError> {
        if self.eat('-') {
            return Ok(match self.unary()? {
                V::K(x) => V::K(-x),
                v => V::E(format!("-1.0 * {}", paren(&v, true))),
            });
        }
        if self.eat('+') {
            return self.unary();
        }
        // A cast: (double) x, (long) x, (float) x, (int) x.
        if self.peek() == Some(&Tok::Sym('(')) && matches!(self.t.get(self.at + 1), Some(Tok::Name(n)) if matches!(n.as_str(), "double" | "float" | "long" | "int")) && self.t.get(self.at + 2) == Some(&Tok::Sym(')')) {
            self.at += 3;
            return self.unary();
        }
        self.primary()
    }

    fn args(&mut self) -> Result<Vec<V>, OsError> {
        self.want('(')?;
        let mut out = Vec::new();
        if self.eat(')') {
            return Ok(out);
        }
        loop {
            out.push(self.expr()?);
            if self.eat(')') {
                return Ok(out);
            }
            self.want(',')?;
        }
    }

    fn primary(&mut self) -> Result<V, OsError> {
        match self.t.get(self.at).cloned() {
            Some(Tok::Num(x)) => {
                self.at += 1;
                Ok(V::K(x))
            }
            Some(Tok::Sym('(')) => {
                self.at += 1;
                let v = self.expr()?;
                self.want(')')?;
                Ok(match v {
                    V::E(e) => V::E(format!("({e})")),
                    k => k,
                })
            }
            Some(Tok::Name(n)) => {
                self.at += 1;
                match n.as_str() {
                    "_score" => {
                        self.read_score = true;
                        Ok(V::E(format!("({})", self.score)))
                    }
                    "params" => {
                        let key = if self.eat('.') {
                            self.name()?
                        } else {
                            self.want('[')?;
                            let k = match self.t.get(self.at).cloned() {
                                Some(Tok::Str(s)) => s,
                                t => return Err(bad(self.src, &format!("{t:?} as a params key"))),
                            };
                            self.at += 1;
                            self.want(']')?;
                            k
                        };
                        match self.params.get(&key) {
                            Some(v) => v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).map(V::K).ok_or_else(|| bad(self.src, &format!("params.{key}, which is not a number"))),
                            None => Err(OsError::new(400, "script_exception", format!("runtime error in [{}]: params.{key} is not given", self.src))),
                        }
                    }
                    "doc" => {
                        self.want('[')?;
                        let field = match self.t.get(self.at).cloned() {
                            Some(Tok::Str(s)) => s,
                            t => return Err(bad(self.src, &format!("{t:?} as a doc field"))),
                        };
                        self.at += 1;
                        self.want(']')?;
                        self.want('.')?;
                        let what = self.name()?;
                        if what == "getValue" {
                            self.args()?;
                        } else if what != "value" {
                            return Err(bad(self.src, &format!("doc['{field}'].{what}")));
                        }
                        let col = self.cx.score_column(&field)?;
                        Ok(V::E(format!("coalesce({col}, 0)")))
                    }
                    "Math" => {
                        self.want('.')?;
                        let f = self.name()?;
                        let a = self.args()?;
                        self.call(&format!("Math.{f}"), a)
                    }
                    f => {
                        let a = self.args()?;
                        self.call(f, a)
                    }
                }
            }
            t => Err(bad(self.src, &format!("{t:?}"))),
        }
    }

    fn call(&mut self, f: &str, a: Vec<V>) -> Result<V, OsError> {
        let k: Option<Vec<f64>> = a.iter().map(|v| if let V::K(x) = v { Some(*x) } else { None }).collect();
        let s: Vec<String> = a.iter().map(V::sql).collect();
        let n = a.len();
        let arity = |want: usize| if n == want { Ok(()) } else { Err(bad(self.src, &format!("{f} of {n} arguments"))) };
        let v = match f {
            "Math.log10" => {
                arity(1)?;
                if let Some(k) = &k {
                    return Ok(V::K(k[0].log10()));
                }
                format!("log({})", s[0])
            }
            "Math.log" => {
                arity(1)?;
                if let Some(k) = &k {
                    return Ok(V::K(k[0].ln()));
                }
                format!("ln({})", s[0])
            }
            "Math.log1p" => {
                arity(1)?;
                format!("ln(1.0 + {})", s[0])
            }
            "Math.exp" | "Math.sqrt" | "Math.abs" => {
                arity(1)?;
                format!("{}({})", &f[5..], s[0])
            }
            "Math.pow" => {
                arity(2)?;
                format!("pow({}, {})", s[0], s[1])
            }
            "Math.max" | "Math.min" => {
                arity(2)?;
                if let Some(k) = &k {
                    return Ok(V::K(if f == "Math.max" { k[0].max(k[1]) } else { k[0].min(k[1]) }));
                }
                format!("{}({}, {})", if f == "Math.max" { "greatest" } else { "least" }, s[0], s[1])
            }
            "saturation" => {
                arity(2)?;
                format!("{} / ({} + {})", paren(&a[0], true), s[0], s[1])
            }
            "sigmoid" => {
                arity(3)?;
                let xa = format!("pow({}, {})", s[0], s[2]);
                format!("{xa} / ({xa} + pow({}, {}))", s[1], s[2])
            }
            "decayNumericGauss" | "decayNumericExp" | "decayNumericLinear" => {
                // (origin, scale, offset, decay, doc['f'].value)
                arity(5)?;
                let Some(col) = s[4].strip_prefix("coalesce(").and_then(|x| x.strip_suffix(", 0)")).map(String::from) else {
                    return Err(bad(self.src, &format!("{f} of a value that is not doc['field'].value")));
                };
                let Some(k) = a[..4].iter().map(|v| if let V::K(x) = v { Some(*x) } else { None }).collect::<Option<Vec<f64>>>() else {
                    return Err(bad(self.src, &format!("{f} with an origin, scale, offset or decay that is not a number")));
                };
                let shape = f.trim_start_matches("decayNumeric").to_ascii_lowercase();
                format!("decay({col}, {}, {}, offset => {}, decay => {}, shape => '{shape}')", num(k[0]), num(k[1]), num(k[2]), num(k[3]))
            }
            other => return Err(bad(self.src, &format!("the function {other}"))),
        };
        Ok(V::E(v))
    }
}

/// `v` in parentheses if it is a sum and `tight` (an operand of `*`, `/`
/// or a right operand of `-`).
fn paren(v: &V, tight: bool) -> String {
    match v {
        V::E(e) if tight && (e.contains(" + ") || e.contains(" - ")) && !(e.starts_with('(') && e.ends_with(')')) => format!("({e})"),
        v => v.sql(),
    }
}

/// The ranking a `script` object (`{"source": .., "params": {..}}`, or
/// the source alone) computes, with `_score` the query's score `score`;
/// a constant when it reads no field and no score.
pub fn compile(script: &Json, cx: &Ctx, score: &str) -> Result<Result<String, f64>, OsError> {
    let (src, params, lang) = match script {
        Json::String(s) => (s.as_str(), Map::new(), "painless"),
        Json::Object(m) => (
            m.get("source").or_else(|| m.get("inline")).and_then(Json::as_str).ok_or_else(|| OsError::parsing("[script] requires source"))?,
            m.get("params").and_then(Json::as_object).cloned().unwrap_or_default(),
            m.get("lang").and_then(Json::as_str).unwrap_or("painless"),
        ),
        _ => return Err(OsError::parsing("[script] is a source or an object with one")),
    };
    if lang != "painless" && lang != "expression" {
        return Err(OsError::new(400, "illegal_argument_exception", format!("script_lang not supported [{lang}]: ChloeDB's script_score runs a part of painless")));
    }
    let mut p = P { t: tokens(src)?, at: 0, src, params: &params, cx, score, read_score: false };
    if p.peek() == Some(&Tok::Name("return".into())) {
        p.at += 1;
    }
    let v = p.expr()?;
    p.eat(';');
    if p.at != p.t.len() {
        return Err(bad(src, &format!("{:?} after the expression", p.t[p.at])));
    }
    Ok(match v {
        V::K(x) => Err(x),
        V::E(e) => Ok(e),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opensearch::mapping::Index;
    use serde_json::json;

    #[test]
    fn scripts() {
        let ix = Index::new("i", "os_i", "u", 0, json!({"properties": {"n": {"type": "long"}, "t": {"type": "text"}}}), json!({}), vec![]).unwrap();
        let cx = Ctx::new(&ix);
        let c = |s: &str| compile(&json!({"source": s, "params": {"a": 2, "b": "3"}}), &cx, "bm25(x)");
        assert_eq!(c("Math.log10((Math.max(doc['n'].value, 0) + 1))").unwrap(), Ok("log((greatest(coalesce(\"n\", 0), 0.0) + 1.0))".into()));
        assert_eq!(c("return _score * params.a;").unwrap(), Ok("(bm25(x)) * 2.0".into()));
        assert_eq!(c("params['a'] + params.b").unwrap(), Err(5.0));
        assert_eq!(c("1 - (doc['n'].value + 2) * 3").unwrap(), Ok("1.0 - ((coalesce(\"n\", 0) + 2.0) * 3.0)".into()));
        assert_eq!(c("saturation(doc['n'].value, 10)").unwrap(), Ok("coalesce(\"n\", 0) / (coalesce(\"n\", 0) + 10.0)".into()));
        assert!(c("doc['t'].value").is_err());
        assert!(c("doc['n'].value > 1 ? 1 : 0").is_err());
        assert!(c("params.zz").is_err());
        assert!(c("Math.random()").is_err());
    }
}
