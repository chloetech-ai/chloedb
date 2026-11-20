# Build with an AI agent

An agent such as Claude Code can do most of what this documentation describes for you. It can load a file, make it searchable by words and by meaning, answer questions from your rows with citations, write the search page or the code of your app, find out why a query is slow, and look after a server. This page shows how to connect one, the prompts that work, and what a good agent does with each, so you can tell when it goes wrong. The server gives your agent the same prompts as slash commands, and the documentation it needs as a tool.

<!-- run: needs chloedb-server -->

## Connect your agent

Start a server, if one is not running yet. With no flags it serves `chloedb.chloedb` in the current directory, at `http://127.0.0.1:7700`:

```sh
chloedb-server > server.log 2>&1 &
until curl -sf http://127.0.0.1:7700/livez > /dev/null; do sleep 0.2; done
```

Then add it to your agent as an MCP server. For Claude Code:

```sh norun
claude mcp add --transport http chloedb http://127.0.0.1:7700/v1/mcp
```

A server with a token needs a key in the header: `--header "Authorization: Bearer $CHLOEDB_KEY"`. A client that starts its MCP servers as commands (Claude Desktop, Cursor and most others) runs `chloedb-server mcp`, which passes each message on to the server:

```json norun
{
  "mcpServers": {
    "chloedb": {
      "command": "chloedb-server",
      "args": ["mcp", "--url", "http://127.0.0.1:7700"],
      "env": {"CHLOEDB_KEY": "your key, or leave it out for a server without a token"}
    }
  }
}
```

What the agent can change is decided by the key it connects with, not by what you ask it: a read key explores, a write key loads rows, and an admin key also makes tables ([agents over MCP](../chloedb/server.md#agents-over-mcp)). Even with an admin key, an agent cannot drop a table or delete every row on its own. The server refuses, and the agent files the change for you to approve ([agents and administration](../chloedb/server.md#agents-and-administration)).

The server tells a connected agent the prompts below. In Claude Code they are slash commands, such as `/mcp__chloedb__load-a-file data/movies.csv`, each with the steps listed under it. You can also list them yourself:

```sh
curl -s http://127.0.0.1:7700/v1/mcp -H 'content-type: application/json' \
  -d '{"jsonrpc": "2.0", "id": 1, "method": "prompts/list"}' \
  | python3 -c 'import json, sys; print("\n".join(p["name"] for p in json.load(sys.stdin)["result"]["prompts"]))'
```

```text output
explore
load-a-file
search
answer-with-citations
search-page
app-code
change-a-table
slow-query
check-the-server
```

## Prompts that work

Each prompt below is written as you would type it. Under it is what a good agent does, and the server sends those steps along when you use the slash command. Words in braces, such as `{path}`, are yours to fill in.

### Explore what is there

<!-- prompt: explore -->

```text prompt=explore
Look at what is in my ChloeDB and tell me what I could search for, with an example query for each table.
```

1. `whoami` says what the key may do, and `list_tables` gives each table's `CREATE TABLE` statement, its key, its vector fields and their models.
2. A few rows of each table (`SELECT * FROM t LIMIT 3`), and the values of its `KEYWORD` fields (`SELECT genre, COUNT(*) FROM t GROUP BY genre`).
3. For each table, a search by its words (`MATCH` and `bm25` over its `TEXT` fields) and, when it has an embedded `VECTOR` field, one by meaning (`similarity`), each run once so the example is known to work.
4. ChloeDB's SQL is its own dialect, not Postgres. Before writing anything unusual, the agent looks up the syntax with `search_docs` and checks the statement with `check_sql`.

### Load a file and search it by meaning

<!-- prompt: load-a-file; path = the file to load: CSV, TSV, JSON or JSON Lines -->

```text prompt=load-a-file
Load {path} into ChloeDB as a table I can search by words and by meaning.
```

1. It looks at the file's first lines and its size, and names the table after the file unless you said otherwise.
2. A file of up to a few megabytes goes through the `import` tool, its text as `data`, with `create: true`. The table is made from the file's columns: numbers become `INT` or `FLOAT`, short labels `KEYWORD`, and prose `TEXT`. A cell that says it has no value (`N/A`, `null`, `-`) in a column of numbers or dates is `NULL`, so nothing needs cleaning first. A table with prose also gets `emb`, a vector of that prose by `potion-retrieval-32M`, which the server downloads once (about 130 MB) and fills in. The answer's `create_table` is the statement it made.
3. A larger file is sent from the shell instead, which needs an admin key on a server with a token: `curl -s --data-binary @{path} -H 'content-type: text/csv' 'http://127.0.0.1:7700/v1/import?table=NAME'`. The content type is `text/tab-separated-values` for TSV and `application/x-ndjson` for JSON Lines. With the server stopped, `chloedb-server import {path} --db chloedb.chloedb` does the same ([loading rows](../chloedb/server.md#loading-rows)).
4. Search by meaning works once the model has arrived. Until then such a query answers 503 `unavailable`, saying how far the download is, or, when it failed, when the server tries again by itself. The agent waits, and `list_models` shows the download, rather than fetching the model itself. Search by words works at once.
5. It ends with one search by words and one by meaning, so you see the table work.

### Search, and say why each row matched

<!-- prompt: search; table = the table to search; question = what to look for -->

```text prompt=search
Search the {table} table for "{question}", by meaning and by words together, and tell me why each result matched.
```

1. Words and meaning fused: `SELECT ... FROM {table} ORDER BY rrf(bm25(body, :q, fuzzy => 'auto'), similarity(emb, :q)) DESC LIMIT 10`, with `:q` the text itself, which the server embeds with the field's own model. `body` stands for the table's `TEXT` fields, and `emb` for its vector field (`list_tables` names both).
2. `fuzzy => 'auto'` takes typos: a word in no row is read as the words near it.
3. A condition on structured fields goes in `WHERE` (`genre = 'scifi' AND year > 2000`), not in the text.
4. To say why a row matched, the agent runs `explain` with `hits`, which gives each top row's scores, or compares the row's words with the question's ([ranking](../chloedb/query-language.md#rrfs1-s2--reciprocal-rank-fusion)).
5. `MATCH(field, 'a b c')` keeps only rows with every word. When it finds nothing, the answer's `hint` suggests `OR` or `min => 2`.

### Answer a question from my rows, with citations

<!-- prompt: answer-with-citations; question = the question to answer -->

```text prompt=answer-with-citations
Answer this from the rows in my ChloeDB, citing the rows each claim comes from: {question}
```

1. It finds the rows as in the search above, eight or so, with their keys.
2. It answers from those rows only, marking each claim with its row as `[1]`, `[2]`, and lists the rows cited with their keys. When the rows do not answer the question, it says so rather than answering from what it knows.
3. In your own code, Python's `db.ask(question, table=...)` does the same in one call. It returns the answer with its `sources`, written by a language model through the query's `llm()`, with `ANTHROPIC_API_KEY` ([Python](../chloedb/python.md#querying)). A whole pipeline, with documents split into chunks and grounded claims, is in [Retrieval for RAG](rag.md).

### Build a search page

<!-- prompt: search-page; table = the table the page searches -->

```text prompt=search-page
Build me a search page for the {table} table: a search box that forgives typos and finds things by meaning, with filters, that I open in my browser.
```

1. **The page cannot call chloedb-server from the browser.** The server refuses requests from a page on another origin, a page opened from a file included, and sends no CORS headers. A page written to `fetch` the server's `/v1/query` therefore fails with a CORS error. The page calls a small app instead, and the app calls the server ([why](../chloedb/javascript.md#why-a-page-cannot-call-chloedb-server)).
2. The app can be Python's standard library and `chloedb` (`pip install chloedb`): `chloedb.connect("http://127.0.0.1:7700")` and an `http.server` handler that serves the page and answers `/search?q=` with `db.search(q, table="{table}", where=..., limit=20)`. `db.search` fuses words and meaning and forgives typos. [Build a search page](search-page.md) is that app in full, with highlights, "did you mean", filters with counts, and "more like this".
3. In a JavaScript app, the route does the same with the `chloedb` package's client ([a search box in a web app](../chloedb/javascript.md#a-search-box-in-a-web-app)).
4. Filters come from the table's `KEYWORD` fields, with their counts (`SELECT genre, COUNT(*) FROM {table} WHERE MATCH(...) GROUP BY genre`).
5. It starts the app, opens the page (or fetches it with `curl`), runs a search with a typo in it, and gives you the address.

### Write the code of my app

<!-- prompt: app-code; language = Python, JavaScript, Rust or Java; task = what the code should do -->

```text prompt=app-code
Write {language} code that uses ChloeDB to {task}.
```

1. It chooses between the database in the app's own process (`chloedb.open(path)` in Python, `open()` in JavaScript, `Db` in Rust) and a client of a running server (`chloedb.connect(url)`). It uses the server when more than one process uses the data, or when the command center should show it.
2. It looks up the client's methods in that language's page, and does not guess them from other databases: [Python](../chloedb/python.md), [JavaScript](../chloedb/javascript.md), [Rust](../chloedb/rust.md), [Java](../chloedb/java.md).
3. Queries take named parameters (`:q`) bound in a mapping, never values pasted into the SQL.
4. It runs the code once against a real database and shows what it printed.

### Change a table

<!-- prompt: change-a-table; table = the table; change = what to change -->

```text prompt=change-a-table
In ChloeDB, change the {table} table: {change}.
```

1. It reads the table's statement (`describe_table`) and finds the syntax with `search_docs` (`ALTER TABLE ... ADD`, a field's options, an embedded field).
2. It checks the statement with `check_sql` before running it with `execute`. An added field fills in for the rows already there, and an embedded vector field computes its vectors in the background.
3. It does not drop a table or a field, or delete or update every row, on its own: the server refuses an agent that tries (`change_request_required`). It asks you, or files the change for an admin to approve with `propose_change`.

### Find out why a query is slow

<!-- prompt: slow-query; sql = the query -->

```text prompt=slow-query
This ChloeDB query is slow. Find out why and make it faster: {sql}
```

1. `explain` shows how the query runs: the plan of each segment, the rows a filter kept, and whether ranking stopped early (`MaxScore`). `queries` shows the latest and slowest statements, with their times.
2. The usual cause is a condition the indexes cannot answer, which tests every row the other conditions keep: a function or arithmetic of a column (`lower(brand) = 'acme'`), a `LIKE '%tent%'` or an `ILIKE` on a field without a substring index, or a comparison of a `TEXT` field's stored text ([statement shape](../chloedb/query-language.md#statement-shape)). Others are a ranking by an expression that every row must compute, and a `LIMIT` much larger than the page shows.
3. It changes one thing at a time, and times the query before and after with the same parameters.

### Check the server

<!-- prompt: check-the-server -->

```text prompt=check-the-server
Check my ChloeDB server's health and tell me what needs attention, and what to do about each thing.
```

1. `health`, `alerts` and `events` (with an admin, `ops` or `propose` key) say how the server is and what has changed.
2. For each problem, `runbook` finds what to do.
3. It acts only within what its key allows alone: a contain rule, a cancelled query, an alert silenced for a while, each with a reason. Anything bigger it files with `propose_change` for a person to approve ([agents and administration](../chloedb/server.md#agents-and-administration)).

## What every agent should know

The server tells a connected agent these at the start. They are written here for an agent that reads this page instead.

- **The SQL is ChloeDB's own.** `MATCH(field, 'words')` filters by text, `ORDER BY bm25(field, 'words') DESC` ranks by it, `similarity(emb, :q)` ranks by meaning, and `rrf(...)` fuses rankings. `search_docs` finds the syntax of anything else in the [SQL reference](../chloedb/query-language.md), and `check_sql` checks a statement without running it.
- **Errors say what to do.** Every error has a stable `error_code`, whether trying again can work (`retryable`), a `hint`, and the documentation section that explains it (`docs`). A retryable error is waited out, not worked around ([error codes](../chloedb/server.md#agents-and-administration)).
- **Meaning comes by itself.** A table made from a file with prose gets a vector of it, which the server fills in once its model has downloaded. Nothing needs to be fetched or configured by hand.
- **Pages call an app, not the server.** See [Build a search page](#build-a-search-page) above.
- **Writes are safe to send again with a key.** A write through a coordinator that answers `write_in_doubt` is sent again with the answer's `idempotency_key` ([failover](../chloedb/server.md#failover)).
- **Ask before anything that loses data.** The server holds such a change for a person anyway.

## Tell your coding agent about ChloeDB

A coding agent working on your app, with no MCP server, reads the documentation as text:

- [llms.txt](https://chloetech-ai.github.io/chloedb/llms.txt) lists every page with a line on what it covers.
- [llms-full.txt](https://chloetech-ai.github.io/chloedb/llms-full.txt) is the whole documentation in one file.

For a project that uses ChloeDB, a few lines in its `CLAUDE.md` or `AGENTS.md` keep the agent on the right path:

```text norun
This project uses ChloeDB (https://chloetech-ai.github.io/chloedb/llms.txt), a search database
with its own SQL dialect, not Postgres: MATCH and bm25 for words, similarity for meaning, rrf to
fuse them. Look up syntax in its SQL reference rather than guessing. The server runs at
http://127.0.0.1:7700 (MCP: /v1/mcp). Browser pages call our app's routes, never the server.
```
