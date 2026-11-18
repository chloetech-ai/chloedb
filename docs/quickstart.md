# Quickstart: a first search in five minutes

This page takes you from nothing to a full-text search with SQL in about five minutes, with nothing to compile. You install ChloeDB's server, make a table, put a few rows in it, search them, and open the command center in a browser. The last section does the same from Python, with the database inside your process and no server at all.

You need macOS 11 or later (Apple silicon or Intel), or Linux on x86-64 or aarch64 with glibc 2.35 or later (Ubuntu 22.04, Debian 12 and newer), and `curl`. On Windows, use WSL 2 or Docker.

## 1. Install the server

Pick one. Each gives you a `chloedb-server` program, prebuilt (a download of about 50 MB), in under a minute.

**The install script** puts it in `~/.local/bin` (it says what to add to your `PATH` if that folder is not on it yet). It checks the download against the release's SHA-256 and, where [GitHub's CLI](https://cli.github.com) (`gh`) is installed, its build provenance (releases from 0.3.8: that this repository's release workflow built it), and changes nothing else. [Read it](install.sh) first if you like.

```sh
# install
curl -fsSL https://chloetech-ai.github.io/chloedb/install.sh | sh
```

It asks GitHub's API (`api.github.com`) which release is the newest, and when a proxy or firewall blocks that address, or your network's address has used up GitHub's 60 requests an hour for clients without an account (an office, a school, a CI runner), it asks `github.com` and then this website instead, and says which one answered. If it stops with these lines, none answered:

```text
chloedb install: could not tell which chloedb-server release is the newest:
  https://api.github.com/repos/chloetech-ai/chloedb/releases?per_page=100: The requested URL returned error: 403
  ...
Name the version after the pipe (the releases are https://github.com/chloetech-ai/chloedb/releases, chloedb-server-v<version>; 0.3.8 was the newest when this script was written):
  curl -fsSL https://chloetech-ai.github.io/chloedb/install.sh | CHLOEDB_VERSION=<version> sh
```

A copy of the script from before 18 November 2026 asked GitHub's API alone, and stopped with `could not find a chloedb-server release on https://github.com/chloetech-ai/chloedb/releases (set CHLOEDB_VERSION to pick one)`. Either way, name the version, and the script downloads it without asking. The variable goes after the pipe, where `sh` reads it (in front of `curl`, the script never sees it):

```sh
# install
curl -fsSL https://chloetech-ai.github.io/chloedb/install.sh | CHLOEDB_VERSION=0.3.8 sh
```

**pipx, uv or pip** <!-- since 0.3.7 -->: the same server, from [PyPI](https://pypi.org/project/chloedb-server/), which asks GitHub nothing, on the same Macs and on Linux with glibc 2.34 or later. `pipx install` and `uv tool install` put `chloedb-server` on your `PATH`; `uvx chloedb-server` runs it without installing it.

```sh
# install
pipx install chloedb-server      # or: uv tool install chloedb-server
```

In a virtualenv, pip puts it in the virtualenv's `bin`, on your `PATH` while the virtualenv is active; `"chloedb[server]"` installs the Python package beside it, for the last section of this page:

```sh
# install
python3 -m venv .venv && . .venv/bin/activate
pip install chloedb-server       # or: pip install "chloedb[server]"
```

**Homebrew** is not available yet: there is no tap to install from. Use the install script above, which works on the same Macs and Linux machines.

**Docker**: skip to [ChloeDB in Docker](#chloedb-in-docker) below, which runs the same steps against the image.

**By hand**: each `chloedb-server-v<version>` [release](https://github.com/chloetech-ai/chloedb/releases) has an archive per platform (`linux-x86_64`, `linux-aarch64`, `macos-arm64`, `macos-x86_64`) with a `.sha256` beside it:

```sh
# install
v=0.3.8 p=macos-arm64
curl -fLO https://github.com/chloetech-ai/chloedb/releases/download/chloedb-server-v$v/chloedb-server-$p.tar.gz
tar xzf chloedb-server-$p.tar.gz && mkdir -p ~/.local/bin && mv chloedb-server-$p/chloedb-server ~/.local/bin/
```

From 0.3.8 each archive also has GitHub's build provenance and an SBOM of what is compiled into it: [checking what you download](../SECURITY.md#checking-what-you-download) shows the `gh attestation verify` command that checks them (the install script runs it for you where `gh` is installed).

Check that it runs:

```sh
chloedb-server --version
```

If it does not, or a later step stops with an error, [Troubleshooting](chloedb/troubleshooting.md) has the usual messages and what to do; `chloedb-server doctor` checks this machine for their causes.

## 2. Start it

A database is a file, with its write-ahead log and the server's own logs beside it (`quickstart.chloedb-wal`, `quickstart.chloedb.logs`). Give the server a path that does not exist yet and it creates an empty database there; it listens on `127.0.0.1:7700`, which only this machine can reach.

```sh
mkdir -p chloedb-quickstart && cd chloedb-quickstart
chloedb-server --db quickstart.chloedb > server.log 2>&1 &
server=$!
until curl -sf http://127.0.0.1:7700/livez > /dev/null; do sleep 0.2; done
```

Everything below talks to it over HTTP with JSON. To keep the commands short, `sql` sends the statement it reads from its input to `POST /v1/sql`, which runs any statement:

```sh
sql() { curl -sS http://127.0.0.1:7700/v1/sql -H 'content-type: application/json' --data-binary @-; echo; }
```

## 3. Make a table and fill it

Every column is indexed for what its type is for: `TEXT` for full-text search (words stemmed, so "jackets" finds "jacket"), `KEYWORD` for exact values and facets, numbers for ranges and sorting.

```sh
sql <<'EOF'
{"sql": "CREATE TABLE products (id KEYWORD PRIMARY KEY, title TEXT, body TEXT, category KEYWORD, price FLOAT)"}
EOF
```

Rows go in with SQL (`INSERT INTO products VALUES (...)`), from files (`chloedb-server import`), or as JSON documents to `POST /v1/write`:

```sh
curl -sS http://127.0.0.1:7700/v1/write -H 'content-type: application/json' --data-binary @- <<'EOF'
{"ops": [
  {"op": "upsert", "table": "products", "doc": {"id": "p1", "title": "Waterproof hiking jacket", "body": "A light shell that keeps rain out on long hikes.", "category": "outdoor", "price": 129.0}},
  {"op": "upsert", "table": "products", "doc": {"id": "p2", "title": "Insulated winter jacket", "body": "Warm down jacket for cold city winters.", "category": "outdoor", "price": 189.0}},
  {"op": "upsert", "table": "products", "doc": {"id": "p3", "title": "Trail running shoes", "body": "Grippy, waterproof shoes for muddy trails.", "category": "footwear", "price": 99.0}},
  {"op": "upsert", "table": "products", "doc": {"id": "p4", "title": "Leather office shoes", "body": "Classic shoes for the office.", "category": "footwear", "price": 149.0}},
  {"op": "upsert", "table": "products", "doc": {"id": "p5", "title": "Rain cover for backpacks", "body": "Waterproof cover that fits 20 to 40 litre packs.", "category": "outdoor", "price": 19.0}}
]}
EOF
echo
sql <<'EOF'
{"sql": "INSERT INTO products (id, title, body, category, price) VALUES ('p6', 'Wool hiking socks', 'Warm socks that dry fast on wet hikes.', 'footwear', 15.0)"}
EOF
```

Each answer says how many rows it wrote (`"written"`), and the rows are durable once it returns.

## 4. Search

The rows that mention "waterproof", best match first. `MATCH` finds them, `bm25` ranks them:

```sh
sql <<'EOF'
{"sql": "SELECT id, title, score() FROM products WHERE MATCH(body, 'waterproof') ORDER BY bm25(body, 'waterproof') DESC LIMIT 5"}
EOF
```

The answer's `rows` are the matches, in the order of its `columns` (`"objects": true` beside `"sql"` gives each row as an object instead); `stats` says how the planner ran the statement, step by step.

Full text and structured filters in one statement, which the planner runs together: waterproof things under 100 that are not shoes.

```sh
sql <<'EOF'
{"sql": "SELECT id, title, price FROM products WHERE MATCH(body, 'waterproof') AND price < 100 AND category <> 'footwear'"}
EOF
```

Typos are forgiven when you ask: `fuzzy => 1` lets each word be one edit away, so "jaket" finds the jackets.

```sh
sql <<'EOF'
{"sql": "SELECT id, title FROM products ORDER BY bm25(title, 'jaket', fuzzy => 1) DESC LIMIT 3"}
EOF
```

Facets are a `GROUP BY` over the matches:

```sh
sql <<'EOF'
{"sql": "SELECT category, COUNT(*), AVG(price) FROM products WHERE MATCH(body, 'waterproof OR warm') GROUP BY category ORDER BY 2 DESC"}
EOF
```

## 5. Open the command center

Open [http://127.0.0.1:7700](http://127.0.0.1:7700) in a browser. The command center opens on **Start here**, which walks through the first steps and ticks each one off once it is done; once they are, the same address opens **Home**, the server's health. If the crawler, Sheets or ChloeCtx were installed beside `chloedb-server` (`install.sh --with`), the server started them too, as [apps](chloedb/server.md#apps-the-server-starts) (unless it was given `--start-apps off`), and their tables (`crawl_*`, `sheets_*`, `ctx_*`) are listed apart from yours, under Tables of apps. **Tables** lists `products`: its page searches it from one box (by words, by meaning, or both), shows its rows, how full each field is, and what uses it, and **Save as a saved search** keeps the query as a stored function that SQL, HTTP, Python, JavaScript and agents (over MCP) all call. The **SQL workbench** runs any SQL you type and shows the plan the planner chose; **Ask** (Ctrl+J) answers a question in words by querying the tables, with an Anthropic API key (the server's, or one you give the tab). The older query console is still at [/classic](http://127.0.0.1:7700/classic). With `--demo 20000`, a new database starts with a catalogue of 20,000 products to try it on; here a second server, on port 7710, so that both run at once:

```sh
chloedb-server --db demo.chloedb --demo 20000 --listen 127.0.0.1:7710 --grpc off > demo.log 2>&1 &
demo=$!
until curl -sf http://127.0.0.1:7710/livez > /dev/null; do sleep 0.2; done
curl -sS http://127.0.0.1:7710/v1/sql -H 'content-type: application/json' \
  -d "{\"sql\": \"SELECT title, price FROM products WHERE MATCH(body, 'waterproof jacket') ORDER BY bm25(body, 'waterproof jacket') DESC LIMIT 5\"}"; echo
```

When you are done, stop the servers (the databases stay in their files, and the next start opens them as they were):

```sh
kill $server $demo
```

## ChloeDB from Python, with no server

The Python package runs a database inside your process, the way `sqlite3` runs SQLite. It is a prebuilt wheel for CPython 3.10 and later on the same platforms as the server: macOS 11 or later, and Linux with glibc 2.28 or later (release 0.2.0's wheels need 2.35). Install it from PyPI:

```sh
# install
python3 -m venv .venv && . .venv/bin/activate
pip install chloedb
```

```sh
python3 - <<'EOF'
import chloedb

db = chloedb.open("books.chloedb", schema={
    "table": "books", "key": "id",
    "fields": [
        {"name": "id", "type": "keyword"},
        {"name": "title", "type": "text"},
        {"name": "year", "type": "int"},
    ],
})
db.upsert([
    {"id": "b1", "title": "The river and the dragon", "year": 1999},
    {"id": "b2", "title": "Winter garden", "year": 2005},
    {"id": "b3", "title": "Dragons of the northern rivers", "year": 2011},
])
print(db.query("SELECT id, title FROM books WHERE MATCH(title, 'dragon') AND year > 2000").rows)
# a typo, forgiven: each word may be an edit or two away, by its length
print(db.query("SELECT id, title FROM books WHERE MATCH(title, :q, fuzzy => 'auto') "
               "ORDER BY bm25(title, :q) DESC", q="dragn").rows)
db.close()
EOF
```

```text output
[['b3', 'Dragons of the northern rivers']]
[['b1', 'The river and the dragon'], ['b3', 'Dragons of the northern rivers']]
```

`chloedb.open` creates the database the first time and opens it after; one process at a time may open it to write. The same methods talk to a server: `chloedb.connect("http://127.0.0.1:7700")` in place of `chloedb.open(...)`. [Python](chloedb/python.md) goes on from here, and [Build a search page](guide/search-page.md) makes a search box with typos, highlights and filters from these pieces.

## ChloeDB in Docker

The image `ghcr.io/chloetech-ai/chloedb-server` (Linux, x86-64 and arm64) keeps its database in `/data`: mount a volume there to keep it. Inside a container the server listens on all of the container's interfaces, which it allows only with a token, so pick one. Publishing the port on `127.0.0.1` keeps it to this machine.

```sh
# install
docker run -d --name chloedb --user 10001:10001 -p 127.0.0.1:7700:7700 -e CHLOEDB_TOKEN=pick-a-secret -v chloedb:/data \
  ghcr.io/chloetech-ai/chloedb-server:0.3.8
```

Then every request sends the token, and the console asks for it once:

```sh
sql() { curl -sS http://127.0.0.1:7700/v1/sql -H 'content-type: application/json' -H 'authorization: Bearer pick-a-secret' --data-binary @-; echo; }
```

and steps 3 to 5 run as written (the `/v1/write` call needs the same `authorization` header). Arguments after the image replace its default ones (`--db /data/chloedb.chloedb --listen 0.0.0.0:7700 --grpc 0.0.0.0:7701`): add `--demo 20000` by passing `--db /data/demo.chloedb --listen 0.0.0.0:7700 --demo 20000`. `docker rm -f chloedb` stops it; the volume keeps the database.

## Next

- [The query language](chloedb/query-language.md): every function, with vectors (`similarity`), hybrid ranking, graph traversal, joins and aggregates.
- [The server](chloedb/server.md): loading files (`chloedb-server import`), schemas, embedded vector fields, API keys, replication and shards.
- [Python](chloedb/python.md), [JavaScript](chloedb/javascript.md) and [Java](chloedb/java.md): the database in your process.
- [SQLite](chloedb/sqlite.md), [Parquet and S3](chloedb/parquet.md) and [Postgres](chloedb/postgres.md): bring your data in.
