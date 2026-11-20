"""Idea 2: a GBNF grammar of the ChloeDB SQL a model may write, made from the
live schema (GET /v1/schema). Only the SELECT subset sqlguard allows: one
statement of at most four conditions, MATCH / bm25 on TEXT fields, comparisons by type, GROUP BY,
ORDER BY, a LIMIT of at most 99, every
repetition and number bounded so greedy decoding cannot loop forever. Search words are plain words (and one
optional quoted phrase), so a MATCH can never fail to parse."""
import requests

def schema(url, table):
    s = requests.get(f"{url}/v1/schema", params={"table": table}).json()["schema"]["fields"]
    kinds = {}
    for f in s:
        ty = f["ty"]
        if f.get("logical") == "timestamp": k = "ts"
        elif isinstance(ty, dict) and "Text" in ty: k = "text"
        elif ty == "Keyword": k = "kw"
        elif ty in ("Int", "Float"): k = "num"
        else: continue
        kinds.setdefault(k, []).append(f["name"])
    return kinds

def alt(names):
    return " | ".join(f'"{n}"' for n in names)

def gbnf(url, table):
    k = schema(url, table)
    cols = [c for v in k.values() for c in v]
    return rf'''
root      ::= " SELECT " sel " FROM {table}" where? group? order? limit? "\n"
sel       ::= item (", " item)? (", " item)? (", " item)?
item      ::= (col | agg | trunc) alias?
alias     ::= " AS " ident
agg       ::= "COUNT(*)" | "COUNT(DISTINCT " col ")" | ("SUM" | "AVG" | "MAX" | "MIN") "(" numcol ")"
trunc     ::= "date_trunc('" ("hour" | "day" | "week" | "month") "', " tscol ")"
col       ::= {alt(cols)}
numcol    ::= {alt(k.get("num", []))}
kwcol     ::= {alt(k.get("kw", []))}
textcol   ::= {alt(k.get("text", []))}
tscol     ::= {alt(k.get("ts", []))}
where     ::= " WHERE " cond (" AND " cond)? (" AND " cond)? (" AND " cond)?
cond      ::= "MATCH(" textcol ", '" mq "')" | kwcol (" = " str | " IN (" str (", " str){{0,9}} ")" | " IS NULL" | " IS NOT NULL") | numcol cmp num | tscol cmp ts | tscol " BETWEEN " ts " AND " ts
cmp       ::= " = " | " > " | " >= " | " < " | " <= " | " != "
group     ::= " GROUP BY " gkey (", " gkey){{0,3}} (" HAVING " agg cmp num)?
gkey      ::= col | ident
order     ::= " ORDER BY " okey (" DESC" | " ASC")? (", " okey (" DESC" | " ASC")?){{0,3}}
okey      ::= "bm25(" textcol ", '" words "')" | agg | col | ident | [1-9]
limit     ::= " LIMIT " [1-9] [0-9]?
mq        ::= mterm (" " ("OR " | "AND " | "NOT ")? mterm){{0,7}}
mterm     ::= word | "\"" words "\""
words     ::= word (" " word){{0,7}}
word      ::= [a-zA-Z0-9.+#-]{{1,30}}
str       ::= "'" [a-zA-Z0-9._@:/ -]{{1,60}} "'"
num       ::= "-"? [0-9]{{1,9}} ("." [0-9]{{1,4}})?
ts        ::= "TIMESTAMP '" [0-9] [0-9] [0-9] [0-9] "-" [0-9] [0-9] "-" [0-9] [0-9] ("T" [0-9] [0-9] ":" [0-9] [0-9] ":" [0-9] [0-9] "Z")? "'"
ident     ::= [a-z] [a-z0-9_]{{0,30}}
'''

if __name__ == "__main__":
    print(gbnf("http://127.0.0.1:7700", "hn_stories"))
