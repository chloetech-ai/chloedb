# Build a search page

This page builds the search page of a small shop in Python: a search box that forgives typos, the words it matched highlighted in the results, "did you mean" under a misspelt search, a filter by category with the count of each, and "more like this" under every product. The web page runs on Python's own `http.server`, so it needs nothing but the standard library and `chloedb`; the [Flask version](#the-same-page-in-flask) at the end uses the same functions.

<!-- run: needs chloedb-server -->

You need Python 3.10 or later with the `chloedb` package and `chloedb-server` on your `PATH`. The package's `server` extra installs both, prebuilt (from 0.3.7):

```sh
# install
python3 -m venv .venv && . .venv/bin/activate
pip install "chloedb[server]"     # chloedb, and chloedb-server of the same release
```

## Why the page talks to a server

Two parts of a search page are routes of [chloedb-server](../chloedb/server.md#http-api), not methods of a database opened in your process: highlighting (`POST /v1/highlight`) and did you mean (`POST /v1/correct`). A web app also tends to run as more than one process: Flask's debug reloader runs your module twice, and gunicorn starts several workers. Only one process at a time can open a database file to write it, and a second one fails with `BusyError: busy: the database is open elsewhere`.

So the catalogue lives in chloedb-server, and the page is its client through `chloedb.connect`, which has the same methods as a database in your process (`query`, `insert`, `search`, `get`). Every process of the app can connect, and none of them holds the file. A page that needs neither highlights nor did you mean can skip the server: each process opens the file with `chloedb.open(path, read_only=True)`, and one other process writes it ([Python](../chloedb/python.md#a-database-in-your-process)).

## The catalogue

Seventeen products, one JSON object a line:

```json file=products.jsonl
{"id": "p01", "name": "Waterproof hiking jacket", "description": "A light shell jacket that keeps the rain out on long hikes.", "category": "jackets", "price": 129.0}
{"id": "p02", "name": "Insulated winter jacket", "description": "A warm down jacket for cold city winters.", "category": "jackets", "price": 189.0}
{"id": "p03", "name": "Packable rain jacket", "description": "A waterproof jacket that folds into its own pocket.", "category": "jackets", "price": 89.0}
{"id": "p04", "name": "Fleece jacket", "description": "A soft fleece layer for cold mornings and campfires.", "category": "jackets", "price": 69.0}
{"id": "p05", "name": "Kids' rain jacket", "description": "A bright waterproof jacket with a hood, for children.", "category": "jackets", "price": 49.0}
{"id": "p06", "name": "Softshell cycling jacket", "description": "A windproof jacket for cool rides, with reflective trim.", "category": "jackets", "price": 109.0}
{"id": "p07", "name": "Trail running shoes", "description": "Grippy, waterproof shoes for muddy trails.", "category": "shoes", "price": 99.0}
{"id": "p08", "name": "Leather office shoes", "description": "Classic leather shoes for the office.", "category": "shoes", "price": 149.0}
{"id": "p09", "name": "Hiking boots", "description": "Waterproof leather boots with ankle support for rocky paths.", "category": "shoes", "price": 159.0}
{"id": "p10", "name": "Wool hiking socks", "description": "Warm socks that dry fast on wet hikes.", "category": "shoes", "price": 15.0}
{"id": "p11", "name": "Rain cover for backpacks", "description": "A waterproof cover that fits 20 to 40 litre packs.", "category": "gear", "price": 19.0}
{"id": "p12", "name": "Daypack, 25 litres", "description": "A light backpack for day hikes, with a pocket for a water bladder.", "category": "gear", "price": 75.0}
{"id": "p13", "name": "Headlamp", "description": "A bright rechargeable headlamp for night hikes and camping.", "category": "gear", "price": 35.0}
{"id": "p14", "name": "Camping stove", "description": "A small gas stove that boils a litre of water in four minutes.", "category": "gear", "price": 59.0}
{"id": "p15", "name": "Down sleeping bag", "description": "A warm sleeping bag for cold nights in the mountains.", "category": "gear", "price": 219.0}
{"id": "p16", "name": "Insulated water bottle", "description": "Keeps drinks cold for a day or hot for twelve hours.", "category": "gear", "price": 25.0}
{"id": "p17", "name": "Trekking poles", "description": "Light aluminium poles that take the load off your knees on steep descents.", "category": "gear", "price": 65.0}
```

"More like this" compares products by meaning, with a vector that ChloeDB computes from each product's name and description by a small embedding model. Fetch the model once (about 130 MB, into `~/.local/share/chloedb/models`, or wherever `CHLOEDB_MODELS` says), then start the server on a new database file. It listens on `127.0.0.1:7700`, which only this machine can reach:

```sh
chloedb-server fetch-model potion-retrieval-32M
chloedb-server --db shop.chloedb > server.log 2>&1 &
server=$!
until curl -sf http://127.0.0.1:7700/livez > /dev/null; do sleep 0.2; done
```

On your own machine, run `chloedb-server --db shop.chloedb` in a terminal of its own instead, and leave it running.

The table has the fields to show (`name`, `description`, `price`), a keyword to filter on (`category`), `body`, a text field generated from the name and the description, which is what the search box searches, and `emb`, the vector:

```python file=load.py
import json
import chloedb

db = chloedb.connect("http://127.0.0.1:7700")
db.sql("""
    CREATE TABLE products (
        id KEYWORD PRIMARY KEY,
        name TEXT,
        description TEXT,
        category KEYWORD,
        price FLOAT,
        body TEXT OPTIONS(from = 'name, description'),
        emb VECTOR OPTIONS(embed = 'name, description', model = 'potion-retrieval-32M')
    )
""")
with open("products.jsonl") as f:
    print(db.insert([json.loads(line) for line in f], table="products"), "products")
```

```sh
python3 load.py
```

```text output
17 products
```

## The queries, as functions

Everything the page asks the database is in one module. Values always go in as parameters (`:q`, `:category`), never into the SQL text, so nothing a visitor types can change a statement:

```python file=shop.py
"""The shop's search: what the page asks chloedb-server, as functions."""
import json
import urllib.request

import chloedb

SERVER = "http://127.0.0.1:7700"
db = chloedb.connect(SERVER)


def where(q, category=None, max_price=None):
    """The WHERE clause and parameters of a search for q."""
    conds, params = ["MATCH(body, :q, syntax => 'plain', fuzzy => 'auto')"], {"q": q}
    if category:
        conds.append("category = :category")
        params["category"] = category
    if max_price is not None:
        conds.append("price <= :max_price")
        params["max_price"] = max_price
    return " AND ".join(conds), params


def search(q, category=None, max_price=None, limit=10):
    """The products with q's words, typos forgiven, best match first."""
    cond, params = where(q, category, max_price)
    sql = f"""SELECT id, name, description, category, price FROM products
              WHERE {cond} ORDER BY bm25(body, :q) DESC LIMIT :limit"""
    return db.query(sql, params, limit=limit).dicts()


def categories(q, max_price=None):
    """How many products each category has for q: the filter's counts."""
    cond, params = where(q, max_price=max_price)
    sql = f"""SELECT category, COUNT(*) AS n FROM products
              WHERE {cond} GROUP BY category ORDER BY n DESC, category"""
    return db.query(sql, params).rows


def more_like(key, limit=3):
    """The products nearest in meaning to the product with this key."""
    sql = """SELECT id, name, price FROM products WHERE id <> :key
             ORDER BY similarity(emb, :v) DESC LIMIT :limit"""
    return db.query(sql, key=key, v={"vector_of": key}, limit=limit).dicts()


def post(route, body):
    """Sends a JSON body to a route of the server and returns its answer."""
    request = urllib.request.Request(SERVER + route, data=json.dumps(body).encode(),
                                     headers={"content-type": "application/json"})
    with urllib.request.urlopen(request) as answer:
        return json.load(answer)


def highlight(q, texts):
    """Each text, HTML-escaped, with the words q matches between <mark> tags."""
    answer = post("/v1/highlight", {"table": "products", "field": "body",
                                    "query": q, "texts": texts, "size": 0})
    return answer["texts"]


def did_you_mean(q):
    """q with its misspelt words corrected, or None when it has none."""
    answer = post("/v1/correct", {"table": "products", "field": "body", "text": q})
    return answer["text"] if answer["changed"] else None
```

The sections below try each function from Python before the page puts them together.

### Typos

`MATCH(body, :q, syntax => 'plain', fuzzy => 'auto')` keeps the products that have every word of the search, each spelt as typed or an edit away (two edits for words of six letters or more; none for words of one or two letters). `syntax => 'plain'` reads the search as its words alone, so what a visitor types is never an error: a stray quote or bracket (`rain jacket"`) is left out, and `-`, `OR` and `*` are no operators. `bm25(body, :q)` ranks them, and since it ranks the same text on the same field, it reads the words as the `MATCH` does and ranks every product the `MATCH` keeps, those spelt as typed first ([typos](../chloedb/query-language.md#typos-and-synonyms)). So "jaket" finds every jacket:

```python
import shop

for p in shop.search("jaket"):
    print(p["id"], p["name"], p["price"])
```

```text output
p05 Kids' rain jacket 49.0
p02 Insulated winter jacket 189.0
p03 Packable rain jacket 89.0
p06 Softshell cycling jacket 109.0
p01 Waterproof hiking jacket 129.0
p04 Fleece jacket 69.0
```

Typos are forgiven where a query asks for it: `db.search(text)` asks for it itself (`typos=False` turns it off), as the search box on a table's page in the command center does, and a query of your own should send the box's text with `fuzzy => 'auto'`, as `shop.search` does. Every word of the search must be there; for a box where any word will do, rank with `bm25(body, :q, fuzzy => 'auto')` and leave the `MATCH` out (a ranked `bm25` returns the rows that have any of its words).

### A filter, with its counts

The filter is one more condition in the `WHERE`, and its counts are a `GROUP BY` over the same conditions, so each count is the number of products its link shows:

```python
import shop

print(shop.categories("waterprof"))
for p in shop.search("waterprof", category="shoes", max_price=150):
    print(p["id"], p["name"], p["price"])
```

```text output
[['jackets', 3], ['shoes', 2], ['gear', 1]]
p07 Trail running shoes 99.0
```

### Highlights

`POST /v1/highlight` marks the words a search matches in any texts you send it, as the field's analyzer reads them, so "waterprof" marks *Waterproof* and "jaket" marks *jacket*. The answer is HTML-escaped, with `<mark>` tags around each match, ready to put in a page (`size` cuts each text to the stretch of about that many characters with the most matches; `0` keeps it whole):

```python
import shop

hits = shop.search("waterprof jaket")
for text in shop.highlight("waterprof jaket", [p["name"] for p in hits] + [p["description"] for p in hits]):
    print(text)
```

```text output
Kids&#x27; rain <mark>jacket</mark>
Packable rain <mark>jacket</mark>
<mark>Waterproof</mark> hiking <mark>jacket</mark>
A bright <mark>waterproof</mark> <mark>jacket</mark> with a hood, for children.
A <mark>waterproof</mark> <mark>jacket</mark> that folds into its own pocket.
A light shell <mark>jacket</mark> that keeps the rain out on long hikes.
```

Highlighting reads typos a little more strictly than the search does: a word of four letters or fewer is marked only where it is spelt as typed.

### Did you mean

`POST /v1/correct` replaces the words of a search that no product has with the word of the field they most likely meant. It suggests a word only when it is fairly sure, which on a small catalogue means a word that many products use: one edit from what was typed, and in five products or more ([did you mean](../chloedb/query-language.md#did-you-mean-correcting-a-querys-words)). Six products say "jacket", and two say "leather", so the first typo gets a suggestion and the second does not, although the search finds both:

```python
import shop

print(shop.did_you_mean("waterprof jaket"))
print(shop.did_you_mean("lether"))
print([p["name"] for p in shop.search("lether")])
```

```text output
waterproof jacket
None
['Leather office shoes', 'Hiking boots']
```

### More like this

`{"vector_of": key}` is the stored vector of the row with that key, so ordering by `similarity(emb, :v)` lists the products nearest it in meaning. `id <> :key` leaves out the product itself, and any other condition (the same category, in stock) goes in the same `WHERE`:

```python
import shop

for p in shop.more_like("p01"):
    print(p["id"], p["name"])
```

```text output
p03 Packable rain jacket
p05 Kids' rain jacket
p11 Rain cover for backpacks
```

## The page

`app.py` serves the page at `http://127.0.0.1:8000`: a form with the search box and a highest price, did you mean, the category links with their counts, and the results with their words marked and a "More like this" link each. It escapes everything it did not get from `/v1/highlight`, which escapes for it, and shows an error page when chloedb-server does not answer:

```python file=app.py
"""The shop's search page, on Python's own web server."""
import html
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlencode, urlparse

import chloedb
import shop

FORM = """<!doctype html>
<meta charset="utf-8"><title>Shop search</title>
<style>mark {{ background: #fde68a; }} li {{ margin: 0.6em 0; }}</style>
<form><input name="q" value="{q}" placeholder="Search the shop" autofocus>
<input name="max_price" value="{max_price}" placeholder="Highest price" size="10"> <button>Search</button></form>"""


def link(text, **params):
    """A link to this page with these parameters."""
    return f'<a href="/?{html.escape(urlencode(params))}">{html.escape(text)}</a>'


def page(q, category, max_price, like):
    lines = [FORM.format(q=html.escape(q), max_price=html.escape(max_price))]
    if like:
        lines.append("<h2>More like this</h2>")
        for p in shop.more_like(like, limit=5):
            lines.append(f"<li>{html.escape(p['name'])}, ${p['price']:.2f}</li>")
        return "\n".join(lines)
    if not q:
        return "\n".join(lines)
    try:
        price = float(max_price) if max_price else None
    except ValueError:
        price = None
    suggestion = shop.did_you_mean(q)
    if suggestion:
        lines.append(f"<p>Did you mean {link(suggestion, q=suggestion)}?</p>")
    counts = [link(f"{c} ({n})", q=q, category=c, max_price=max_price) for c, n in shop.categories(q, price)]
    lines.append("<p>" + " · ".join([link("All", q=q, max_price=max_price)] + counts) + "</p>")
    hits = shop.search(q, category=category or None, max_price=price)
    marked = shop.highlight(q, [p["name"] for p in hits] + [p["description"] for p in hits]) if hits else []
    for p, name, description in zip(hits, marked, marked[len(hits):]):
        lines.append(f"<li><b>{name}</b> ${p['price']:.2f}: {description} {link('More like this', like=p['id'])}</li>")
    if not hits:
        lines.append("<p>No products have those words.</p>")
    return "\n".join(lines)


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        url = urlparse(self.path)
        if url.path != "/":
            self.send_error(404)
            return
        args = {k: v[0] for k, v in parse_qs(url.query).items()}
        try:
            text = page(args.get("q", "").strip(), args.get("category", ""),
                        args.get("max_price", ""), args.get("like", ""))
            body = (text + "\n").encode()
        except (chloedb.ChloeDBError, OSError) as e:  # OSError: chloedb-server does not answer
            self.send_error(503, "The search is not available", str(e))
            return
        self.send_response(200)
        self.send_header("content-type", "text/html; charset=utf-8")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


if __name__ == "__main__":
    ThreadingHTTPServer(("127.0.0.1", 8000), Handler).serve_forever()
```

Start it, and ask it for a search with a typo, as a browser would:

```sh
python3 app.py > app.log 2>&1 &
app=$!
until curl -sf http://127.0.0.1:8000/ > /dev/null; do sleep 0.2; done
curl -s 'http://127.0.0.1:8000/?q=waterprof+jaket&max_price=100'
```

```text output
<!doctype html>
...
<p>Did you mean <a href="/?q=waterproof+jacket">waterproof jacket</a>?</p>
<p><a href="/?q=waterprof+jaket&amp;max_price=100">All</a> · <a href="/?q=waterprof+jaket&amp;category=jackets&amp;max_price=100">jackets (2)</a></p>
<li><b>Kids&#x27; rain <mark>jacket</mark></b> $49.00: A bright <mark>waterproof</mark> <mark>jacket</mark> with a hood, for children. <a href="/?like=p05">More like this</a></li>
<li><b>Packable rain <mark>jacket</mark></b> $89.00: A <mark>waterproof</mark> <mark>jacket</mark> that folds into its own pocket. <a href="/?like=p03">More like this</a></li>
```

Open [http://127.0.0.1:8000](http://127.0.0.1:8000) in a browser and try "sleping bag", "lether", "warm" or "hiking boots" with a highest price. The server's own command center, at [http://127.0.0.1:7700](http://127.0.0.1:7700), shows the same table and the queries the page sent (**Queries**).

The browser talks only to the app, and the app to chloedb-server. That keeps the server's address and keys out of the page, and it is also the only way that works: chloedb-server refuses requests from pages of other sites, reads included ([security](../chloedb/server.md#security)).

When you are done, stop both (the catalogue stays in `shop.chloedb`):

```sh
kill $app $server
```

## The same page in Flask

The functions of `shop.py` drop into any web framework. This is the page in [Flask](https://flask.palletsprojects.com) (`pip install flask`, then `flask --app flask_app run --debug`), shortened to the search and "more like this". It is shown, not run with this page, which installs nothing but `chloedb`:

```python file=flask_app.py
from flask import Flask, render_template_string, request

import shop

app = Flask(__name__)

PAGE = """<!doctype html>
<form><input name="q" value="{{ q }}" autofocus> <button>Search</button></form>
{% if suggestion %}<p>Did you mean <a href="/?q={{ suggestion | urlencode }}">{{ suggestion }}</a>?</p>{% endif %}
{% for p, name in hits %}
  <li><b>{{ name | safe }}</b> ${{ "%.2f" | format(p.price) }} <a href="/like/{{ p.id }}">More like this</a></li>
{% endfor %}
{% for p in similar %}<li>{{ p.name }}, ${{ "%.2f" | format(p.price) }}</li>{% endfor %}
"""


@app.get("/")
def index():
    q = request.args.get("q", "").strip()
    hits = shop.search(q) if q else []
    names = shop.highlight(q, [p["name"] for p in hits]) if hits else []
    return render_template_string(PAGE, q=q, suggestion=q and shop.did_you_mean(q), hits=zip(hits, names), similar=[])


@app.get("/like/<key>")
def like(key):
    return render_template_string(PAGE, q="", suggestion=None, hits=[], similar=shop.more_like(key, limit=5))
```

`| safe` is for the highlighted names alone, which `/v1/highlight` has escaped already; Jinja escapes the rest. With `--debug`, Flask runs this module in two processes, one that watches the files and one that serves. Both are clients of the one chloedb-server, so the second starts as the first did. Had the module opened the file itself with `chloedb.open("shop.chloedb")`, the second would have failed with `BusyError: busy: the database is open elsewhere`, since the first holds it, and every request with it ([troubleshooting](../chloedb/troubleshooting.md#busy-the-database-is-open-elsewhere)). For an asyncio framework such as FastAPI, `chloedb.AsyncClient` has the same methods as coroutines ([from asyncio](../chloedb/python.md#from-asyncio)).

## Before other people use it

- Give chloedb-server a token before it listens beyond this machine, and the app a key: `chloedb.connect(SERVER, token=...)`, or `$CHLOEDB_TOKEN` ([security](../chloedb/server.md#security)). A key that can only read the products table is enough for this page.
- Change a product with `UPDATE` or `upsert`: `upsert` replaces the whole row, so it needs every field, while `UPDATE products SET price = 79 WHERE id = 'p03'` changes one. Writes are searchable on the next query.
- [Troubleshooting](../chloedb/troubleshooting.md) has the errors people meet first, each with its message and what to do.

## Next

- [The search guide](search.md): phrases, prefixes as you type, vectors, hybrid ranking and results spread across a field.
- [Python](../chloedb/python.md): every method of `chloedb.connect` and `chloedb.open`, and their errors.
- [The SQL reference](../chloedb/query-language.md): boosts by a column (`bm25(body, :q) * (1 + 0.1 * rating)`), synonyms and rank profiles.
