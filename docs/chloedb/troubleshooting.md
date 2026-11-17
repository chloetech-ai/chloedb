# Troubleshooting

The errors people meet first with ChloeDB, each with its message as the program prints it, what it means and what to do. Search this page for a few words of your message. These are the messages of release 0.3.7.

`chloedb-server doctor` checks this machine for the usual causes at once: its version against the latest release, the Python package, the models folder, port 7700 and glibc. It changes nothing, prints a `fix:` line under each check that is not `ok`, and exits 1 when one fails:

```text
$ chloedb-server doctor --offline
ok   version: chloedb-server 0.3.7 (the latest release was not looked up)
ok   python package: chloedb 0.3.7
warn models: no models in /home/me/.local/share/chloedb/models: a table whose vectors ChloeDB embeds needs its model
       fix: chloedb-server fetch-model potion-retrieval-32M   (into /home/me/.local/share/chloedb/models)
ok   port: 7700 is free (the guides' servers listen there)
ok   glibc: 2.39: the prebuilt programs and wheels run here
```

Without `--offline` it also asks GitHub which release is the latest, and says so when it cannot. `--python` names the interpreter whose package it checks (`python3` by default), `--models` the folder and `--port` the port.

## Installing

### The install script stops: `could not find a chloedb-server release`

```text
curl: (22) The requested URL returned error: 403
chloedb install: could not find a chloedb-server release on https://github.com/chloetech-ai/chloedb/releases (set CHLOEDB_VERSION to pick one)
```

Without `CHLOEDB_VERSION`, the script asks GitHub's API (`api.github.com`) which release is the newest. That request failed: a proxy or firewall blocks `api.github.com`, or your address has used up GitHub's 60 requests an hour for clients without an account (a shared office or school network, a CI runner). The archives come from `github.com` itself, which usually still works, so name the version. The variable goes after the pipe, where `sh` reads it; in front of `curl` it reaches only `curl`, and the script fails the same way:

```sh
curl -fsSL https://chloetech-ai.github.io/chloedb/install.sh | CHLOEDB_VERSION=0.3.7 sh
```

Or install the same server from PyPI, which asks GitHub nothing: `pipx install chloedb-server`, `uv tool install chloedb-server`, `pip install chloedb-server` in a virtualenv, or `uvx chloedb-server` to run it without installing it (macOS 11+, and Linux with glibc 2.34 or later). The [quickstart](../quickstart.md#1-install-the-server) has the other ways, by hand included.

### The install script's other messages

| message | what to do |
|---|---|
| `the prebuilt server needs glibc 2.35 or later (Ubuntu 22.04, Debian 12), and this machine has 2.31: ...` | the prebuilt archives need a newer Linux; the PyPI wheels need glibc 2.34; use the Docker image (`ghcr.io/chloetech-ai/chloedb-server`), or build from source |
| `this Linux has no glibc (Alpine and other musl systems): ...` | use the Docker image, or build from source |
| `Windows has no prebuilt server yet: run this inside WSL 2, or use Docker ...` | run the script inside WSL 2 |
| `could not download https://github.com/.../chloedb-server-v0.3.9/... (is 0.3.9 a released version, and does it have chloedb-server? ...)` | `CHLOEDB_VERSION` names a version that has no release, or `--with` a program that release does not have: see the [releases](https://github.com/chloetech-ai/chloedb/releases) |
| `/home/me/.local/bin is not on your PATH. To add it, put this line in your shell's profile (~/.zshrc, ~/.bashrc): ...` | not an error: the server is installed; add the line it prints, then open a new terminal |

### `chloedb-server: command not found`

```text
bash: chloedb-server: command not found
```

The folder the server was installed into is not on your `PATH`: `~/.local/bin` for the install script (it printed the line to add to your shell's profile), the virtualenv's `bin` for pip (activate it: `. .venv/bin/activate`), or `pipx ensurepath` for pipx. Until then, run it by its path, `~/.local/bin/chloedb-server`.

### `ModuleNotFoundError: No module named 'chloedb'`

The Python that runs your script is not the one `chloedb` was installed into: activate the virtualenv (`. .venv/bin/activate`), or install it with that interpreter, `python3 -m pip install chloedb`. `chloedb-server doctor --python PATH` checks a given interpreter.

### `ImportError: This is chloedb 0.0.3, a placeholder ...`

pip installed the placeholder that held the name on PyPI, which it falls back to when ChloeDB has no wheel for this platform or this Python. The wheels are for CPython 3.10 or later on macOS 11 or later and on Linux x86-64 and aarch64 with glibc 2.28 or later. Check `python3 --version`; elsewhere (Windows outside WSL 2, Alpine, an older Python) [build from source](python.md#from-source). On a machine the wheels cover, `pip install --upgrade chloedb` replaces an old placeholder.

## Opening a database

### `busy: the database is open elsewhere`

```text
chloedb._native.BusyError: busy: the database is open elsewhere
ERROR chloedb_server: open recipes.chloedb: busy: the database is open elsewhere
```

One process at a time can open a database to write it, and the file is open to write already: in another process (a script still running, a chloedb-server serving it, the second process of Flask's debug reloader, another web worker), or in this process, by an earlier `open` of the same path that was not closed (a module imported twice, a notebook cell run again, a hot reload in Next.js). The message is the same in every case.

- In one process, open the database once and share it; close it before opening it again.
- To read it from other processes while one writes, open it read-only there: `chloedb.open(path, read_only=True)` in Python, `Database.open(path, { readOnly: true })` in JavaScript.
- To write from several processes, serve the file with `chloedb-server --db path` and use the server's clients (`chloedb.connect(url)`), as [Build a search page](../guide/search-page.md) does.
- `chloedb-server --db path` stops with the second line above while a script has the file open: stop the script first, or let it use the server too.

[Python's common errors](python.md#busyerror-busy-the-database-is-open-elsewhere) has more on each case.

### `corrupt database: no valid meta slot`

```text
_chloedb.ChloeDBError: corrupt database: no valid meta slot
ERROR chloedb_server: open recipes.jsonl: corrupt database: no valid meta slot
```

Most often the file is not a ChloeDB database at all: a JSON Lines file, a CSV or a SQLite database given where a database was expected. ChloeDB leaves that file as it was, but makes an empty `<file>-readers` folder beside it, which you can delete. To load such a file's rows, give it to a database: `db.import_file(path)` (JSON Lines, JSON, CSV, TSV), `db.import_sqlite(path)` or `db.import_parquet(path)` in Python, `POST /v1/import` or the command center's import on a server ([loading rows](server.md#loading-rows)). A ChloeDB database that is really damaged says which file and what is damaged, and how to recover ([damaged files](python.md#backing-up-and-damaged-files)).

### `does not exist: pass a schema to create it`

```text
_chloedb.ChloeDBError: books.chloedb does not exist: pass a schema to create it
ERROR chloedb_server: rec.chloedb does not exist: pass --schema (a JSON file, or the JSON) to create it
```

The first is `chloedb.open(path)` without a schema where no database is: a misspelt path, or a script started from another folder (a relative path is read from the current directory). Give the right path, or the schema to create it. The second is `chloedb-server import --db new.chloedb rows.jsonl`: a JSON Lines, JSON or CSV file does not say its types, so a new database needs `--schema`; a SQLite or Parquet file creates its tables itself. Or create the table first (`CREATE TABLE`, from Python or on a server) and import into it: `chloedb-server import` adds the rows to the database's first table, or `--table`. A running server takes files through `POST /v1/import` and the command center's **Import a file**, which proposes the types from the file's values.

## Models and vectors

### `model potion-retrieval-32M: no .../model.safetensors (fetch it first)`

```text
{'emb': 'schema error: model potion-retrieval-32M: no /home/me/.local/share/chloedb/models/potion-retrieval-32M/model.safetensors (fetch it first)'}
```

A vector field names an embedding model that is not in the models folder. Python's `db.embed_errors` says so (above), a server's `GET /v1/schema` says it as each embedded field's `error`, with `"loaded": false`, and `GET /v1/models` lists what would open. Meanwhile the writes and queries that need the model fail:

```text
chloedb._native.BadRequestError: schema error: notes.emb is embedded by model potion-retrieval-32M, which is not open: the row would be written without its vector, which similarity() never finds. ...
chloedb._native.BadRequestError: query error: no embedded field has its model open
parameter :v: no table has a field this server can embed text for; pass a vector
```

Fetch it once: `chloedb-server fetch-model potion-retrieval-32M` (about 130 MB). The models folder is `$CHLOEDB_MODELS` when that is set, else `~/.local/share/chloedb/models` (`~/.local/share/strata/models` on a machine that has that folder from before the rename); a server takes `--models DIR`, Python `chloedb.open(..., models=DIR)` and JavaScript the `models` option. Open the database again after fetching it; a running server opens a model fetched through it (its **Models** page, or `POST /v1/admin/models/fetch`). A server fetches a model it knows by itself when a table names it (unless it runs with `--no-fetch-models`, for a machine that cannot reach huggingface.co), and fills in the vectors of the rows written meanwhile; the error shows until the download ends, and `GET /v1/schema` then says `"loaded": true`.

### A BERT model such as bge-small does not run

```text
ERROR chloedb_server: schema error: model bge-small-en-v1.5 is a BERT model: build with the `bert` feature
```

The prebuilt programs (the install script's archives, the PyPI wheels, the npm package and the Docker image) are built without the `bert` feature, so the BERT models ChloeDB knows do not run in them: `bge-small-en-v1.5`, `snowflake-arctic-embed-m-v1.5`, the ColBERT model `answerai-colbert-small-v1`, and the cross-encoders `ms-marco-MiniLM-L-6-v2` and `bge-reranker-base`. `chloedb-server fetch-model bge-small-en-v1.5` downloads the model (about 133 MB) and then stops with the message above, and a table that names one lists it in Python's `db.embed_errors`, which refuses the writes that need it, while a server stores those rows without their vectors. Models of images, sound and ONNX exports need the `vision`, `audio` and `onnx` features, also left out of the prebuilt programs. What to do instead:

- **A model that runs in every build**: `potion-retrieval-32M` (Model2Vec, 512 dimensions, the one the guides use) or the smaller `potion-base-8M`, and their int8 copies ([embedded vector fields](server.md#embedded-vector-fields)).
- **A hosted model**: `openai/text-embedding-3-small`, `cohere/embed-v4.0`, `voyage/voyage-3.5`, or `ollama/nomic-embed-text` from an Ollama on your own machine, with no key ([hosted models](server.md#hosted-models)). The prebuilt programs call them.
- **Your own vectors**: make them with any library (fastembed, sentence-transformers) and write them into a `VECTOR` field with its `dim` and no `embed`, then pass the query's vector the same way (`ORDER BY similarity(emb, :v)`, with `v` a list of floats or a numpy array).
- **A build from source with the feature**: `cargo build --release --features bert` in `chloedb-server/` (`--features accelerate` on a Mac), or `MATURIN_PEP517_ARGS="--features bert" pip install ./chloedb-py` for Python ([from source](python.md#from-source)).

## Running a server

### `the address is in use`

```text
ERROR chloedb_server: listen on 127.0.0.1:7700: the address is in use, by another chloedb-server or another program (lsof -i :7700 names it): stop it, or give --listen another port
ERROR chloedb_server: listen on 127.0.0.1:7701: the address is in use, by another chloedb-server or another program (lsof -i :7701 names it): stop it, or give --grpc another port
```

Another program has the port: often a chloedb-server started earlier and still running. `lsof -i :7700` names it. A server listens on two ports, HTTP on 7700 and gRPC on 7701, so a second server on the same machine needs both moved: `--listen 127.0.0.1:7710 --grpc 127.0.0.1:7711`, or `--grpc off` when nothing uses gRPC.

### `is reachable beyond this machine: set --token`

```text
ERROR chloedb_server: 0.0.0.0:7700 is reachable beyond this machine: set --token (or $CHLOEDB_TOKEN), make an API key, or pass --no-token to serve it open
```

A server that listens on an address other machines can reach (`0.0.0.0`, a LAN address, inside a container) must have a token or an API key, so that nobody else can change it. Give it `--token` (or `$CHLOEDB_TOKEN`) and send the token from every client, or listen on `127.0.0.1` ([security](server.md#security)).

### `401`: `missing or wrong token`

```text
{"error":"missing or wrong token","error_code":"unauthenticated","hint":"send Authorization: Bearer <key>; a key that expired or was revoked needs a new one",...}
```

The server has a token or API keys, and the request sent none, or one it does not know. Send it as `Authorization: Bearer <key>`: `curl -H 'authorization: Bearer ...'`, `chloedb.connect(url, token=...)` in Python and `connect(url, { token })` in JavaScript, or set `$CHLOEDB_TOKEN`, which both clients read. Python raises it as `UnauthorizedError: missing or wrong token (HTTP 401)`. A key that is known but may not do what was asked gets 403 instead, naming the scope it needs: `a write statement needs a key with the write scope; "page" has read`.

### `a page from http://localhost:3000 cannot change this server`

A browser page of another origin called the server directly, and chloedb-server refuses those (403), reads included: call it from your app's own server code instead, or serve the page from the server's own origin ([JavaScript in a browser](javascript.md)).

### `URLError: <urlopen error [Errno 111] Connection refused>`

A Python client found no server at its URL: start the server, or check the address and port its log printed (`HTTP and UI on http://127.0.0.1:7700/`). It is an `OSError`, not a `ChloeDBError` ([Python's common errors](python.md#urlerror-urlopen-error-errno-111-connection-refused)).

## ChloeCtx

`chloectx doctor` prints a line per check, `ok`, `warn` or `FAIL`, and each `FAIL` line says how to fix it: [Install ChloeCtx](../chloectx/setup.md#troubleshooting) goes through them.
