# Crawl a site into search

`chloedb-crawl` reads a site, finds the fields its pages carry (JSON-LD, meta tags, spec tables, the site's own template), makes a table from them and fills it. This recipe crawls a small shop, served on your machine so that it runs anywhere, and searches the table it made with SQL.

<!-- run: needs chloedb-server chloedb-crawl -->

You need `chloedb-server` and `chloedb-crawl` on your `PATH`. <!-- since 0.3.1 --> From release 0.3.1, the installer puts both there prebuilt, with nothing to compile:

```sh
# install
curl -fsSL https://chloetech-ai.github.io/chloedb/install.sh | sh -s -- --with crawl
```

Release 0.3.0 and earlier have no prebuilt crawler. With those, or to build from source, use a checkout of the repository (Rust and a C compiler, several minutes): `(cd chloedb-server && cargo build --release) && (cd chloedb-crawl && cargo build --release)`, then add both `target/release` directories to `PATH`.

## A site to crawl

A shop with a home page that links to six products. Each product page has its fields in JSON-LD, as most shops do. This script writes the site's pages into `shop/`:

```python file=make_shop.py
import json
import pathlib

PRODUCTS = [
    ("trail-runner", "Trail Runner 2", "Running shoe with a grippy sole for muddy trails.", 89.0, "Shoes", "InStock"),
    ("city-walker", "City Walker", "Light everyday shoe with a breathable mesh upper.", 59.0, "Shoes", "InStock"),
    ("rain-shell", "Rain Shell Jacket", "Waterproof jacket that packs into its own pocket.", 120.0, "Jackets", "OutOfStock"),
    ("down-parka", "Down Parka", "Warm winter parka with a waterproof outer layer.", 240.0, "Jackets", "InStock"),
    ("wool-socks", "Merino Wool Socks", "Soft socks that stay warm when wet.", 18.0, "Accessories", "InStock"),
    ("dry-bag", "Dry Bag 20L", "Roll-top waterproof bag for kayaking and rain.", 32.0, "Accessories", "InStock"),
]

site = pathlib.Path("shop")
(site / "p").mkdir(parents=True, exist_ok=True)
links = "".join(f'<li><a href="/p/{slug}.html">{name}</a></li>' for slug, name, *_ in PRODUCTS)
(site / "index.html").write_text(
    f"<!doctype html><html><head><title>Northwind Outdoor</title></head>"
    f"<body><h1>Northwind Outdoor</h1><ul>{links}</ul></body></html>")
for slug, name, text, price, category, stock in PRODUCTS:
    ld = {"@context": "https://schema.org", "@type": "Product", "name": name, "category": category,
          "description": text,
          "offers": {"@type": "Offer", "price": f"{price:.2f}", "priceCurrency": "EUR",
                     "availability": f"https://schema.org/{stock}"}}
    (site / "p" / f"{slug}.html").write_text(
        f"<!doctype html><html><head><title>{name} | Northwind Outdoor</title>"
        f'<script type="application/ld+json">{json.dumps(ld)}</script></head>'
        f'<body><nav><a href="/">Home</a></nav>'
        f'<main><h1>{name}</h1><p>{text}</p><p class="price">EUR {price:.2f}</p></main></body></html>')
print(len(list(site.rglob("*.html"))), "pages")
```

```sh
python3 make_shop.py
```

```text output
7 pages
```

Serve it on port 8099:

```sh
python3 -m http.server 8099 --bind 127.0.0.1 --directory shop > shop.log 2>&1 &
until curl -sf http://127.0.0.1:8099/ > /dev/null; do sleep 0.2; done
```

## Look before you crawl

`preview` reads a few pages and prints, as JSON, the table it would make and why, writing nothing. Its `sql` is the table. The crawler fetches the public internet only, so a site on your own machine or network has to be named with `--allow-host` ([reach](../chloedb/crawler.md#what-it-does-with-a-page)):

```sh
chloedb-crawl preview http://127.0.0.1:8099/ --pages 7 --allow-host 127.0.0.1:8099 > preview.json
python3 -c 'import json; p = json.load(open("preview.json")); print(len(p["pages"]), "pages read"); print(p["sql"])'
```

```text output
7 pages read
CREATE TABLE IF NOT EXISTS site_127_0_0_1 (
  url KEYWORD PRIMARY KEY,
  host KEYWORD,
  depth INT,
  status INT,
  fetched TIMESTAMP,
  bytes INT,
  fetch_ms INT,
  title TEXT,
  body TEXT,
  words INT,
  links INT,
  description TEXT,
  headings TEXT,
  kind KEYWORD,
  category KEYWORD,
  name KEYWORD,
  offers_availability KEYWORD,
  offers_price FLOAT,
  offers_pricecurrency KEYWORD,
  offers_type KEYWORD,
  price FLOAT,
  data JSON
)
```

## Crawl

`crawl --local DIR` starts a cluster of its own in `DIR` (a coordinator on `127.0.0.1:7700` and a shard), crawls into it, prints its progress and then the table it made, and stops the cluster. `--table` names the table, which is otherwise named after the site's host. On a site you don't run, keep the defaults, which space the requests out and obey robots.txt; this one is ours, so it can go faster:

```sh
chloedb-crawl crawl http://127.0.0.1:8099/ --local ./crawl-cluster --allow-host 127.0.0.1:8099 \
  --table shop --per-host 4 --delay-ms 0 --max-pages 50
```

```text output
...
CREATE TABLE IF NOT EXISTS shop (
  url KEYWORD PRIMARY KEY,
  host KEYWORD,
  depth INT,
  status INT,
  fetched TIMESTAMP,
  bytes INT,
  fetch_ms INT,
  title TEXT,
  body TEXT,
  words INT,
  links INT,
  description TEXT,
  headings TEXT,
  kind KEYWORD,
  category KEYWORD,
  name KEYWORD,
  offers_availability KEYWORD,
  offers_price FLOAT,
  offers_pricecurrency KEYWORD,
  offers_type KEYWORD,
  price FLOAT,
  data JSON
)

{"table":"shop","phase":"done","counts":{"fetched":7,"pages":7,"written":7,...
```

## Search it

The same directory serves the same cluster again. `serve` starts it with the crawler's API beside it, which the command center at `http://127.0.0.1:7700` shows under **Apps → Crawler**:

```sh
chloedb-crawl serve --local ./crawl-cluster > serve.log 2>&1 &
until curl -sf http://127.0.0.1:7700/v1/health > /dev/null; do sleep 0.2; done
```

The table has a column for each field most pages had: the page's own (`url`, `title`, `body`, ...), those of the JSON-LD, such as `offers_price`, and `price`, which it learned from the site's own template (the `<p class="price">` every product page has). Ask the server which:

```sh
curl -s http://127.0.0.1:7700/v1/query -H 'content-type: application/json' \
  -d '{"sql": "SELECT * FROM shop LIMIT 1"}' | python3 -c 'import json, sys; print(json.load(sys.stdin)["columns"])'
```

```text output
['url', 'host', 'depth', 'status', 'fetched', 'bytes', 'fetch_ms', 'title', 'body', 'words', 'links', 'description', 'headings', 'kind', 'category', 'name', 'offers_availability', 'offers_price', 'offers_pricecurrency', 'offers_type', 'price', 'data']
```

Then search it like any table: full text over the pages' text, filtered and ranked by the fields the crawler found.

```sh
curl -s http://127.0.0.1:7700/v1/query -H 'content-type: application/json' -d @- <<'JSON' | python3 -c 'import json, sys; [print(*r, sep=" | ") for r in json.load(sys.stdin)["rows"]]'
{"sql": "SELECT title, offers_price, offers_availability FROM shop WHERE MATCH(body, 'waterproof') ORDER BY bm25(body, 'waterproof') DESC LIMIT 5"}
JSON
```

```text output
Down Parka | Northwind Outdoor | 240.0 | https://schema.org/InStock
Dry Bag 20L | Northwind Outdoor | 32.0 | https://schema.org/InStock
Rain Shell Jacket | Northwind Outdoor | 120.0 | https://schema.org/OutOfStock
```

## Next

- [The crawler](../chloedb/crawler.md): every option, the fields it finds, how it grows a cluster, and its API.
- [The page for site owners](../chloedb/crawler-bot.md): what the crawler sends and how to keep it off a site.
- `--embed potion-base-8M` embeds each page's title and body into a vector field, `emb`, for the [hybrid search](search.md#both-hybrid-ranking) the search guide shows.
