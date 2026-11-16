# chloedb-server

ChloeDB's server, prebuilt: a search database (full text, vectors, graphs and
SQL in one engine) over HTTP JSON and gRPC, with a command center in the
browser. This package installs the `chloedb-server` program, with no Rust and
no compiler:

```sh
pip install chloedb-server
chloedb-server --db my.chloedb            # then open http://127.0.0.1:7700
```

or run it without installing anything into your environment:

```sh
uvx chloedb-server --db my.chloedb --demo 20000   # a demo catalogue of 20,000 products
pipx run chloedb-server --version
```

From Python, the [`chloedb`](https://pypi.org/project/chloedb/) package
starts one and connects to it (`pip install "chloedb[server]"` installs both,
of the same version):

```python
import chloedb

with chloedb.serve("books.chloedb") as db:   # a Client of the server it started
    db.sql("CREATE TABLE books (id KEYWORD PRIMARY KEY, title TEXT)")
    db.insert({"id": "b1", "title": "The river and the dragon"}, table="books")
    print(db.search("dragon", table="books").dicts())
# the server stops at the end of the block
```

`python -m chloedb_server ARGS` runs the program too, for an environment whose
programs are not on the `PATH`, and `chloedb_server.find_bin()` is its path.

Each wheel holds the same binary as the release's archive for its platform:
Linux x86-64 and aarch64 (glibc 2.34 or later: Ubuntu 22.04, Debian 12, RHEL
10 and newer) and macOS 11 or later (Apple silicon and Intel). There is no
wheel for Windows (use WSL 2) or for Alpine (musl), and no source
distribution: build from [the repository](https://github.com/chloetech-ai/chloedb)
there.

The server's reference is
[docs/chloedb/server.md](https://github.com/chloetech-ai/chloedb/blob/main/docs/chloedb/server.md),
and the [quickstart](https://github.com/chloetech-ai/chloedb/blob/main/docs/quickstart.md)
goes from nothing to a first search in five minutes. Licensed under
Apache-2.0.
