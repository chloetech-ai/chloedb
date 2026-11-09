# ChloeDB and Engram: instructions for coding agents

This repository holds ChloeDB, an embedded search database with SQL, and
Engram, long-term memory for Claude Code built on it. This file is for a
coding agent (Claude Code or another) that was asked to set Engram up for its
user, or to work on this repository. People should start with
[README.md](README.md) and [the install guide](docs/engram/setup.md).

ChloeDB was called Strata until November 2026, and the rename lands in steps.
Until it is done, the folders, crates, binaries, `STRATA_*` settings and the
repository keep the old name: use the names as this file and the code spell
them, and say ChloeDB in prose.

## Set Engram up for the user's Claude Code

Engram plugs into Claude Code with hooks and an MCP server, so installing it
changes the user's Claude Code configuration. **Ask the user which scope they
want before running anything:**

| scope | command | what it changes |
|---|---|---|
| every session on this machine | `./install.sh` | merges hooks, a permission and `env.ENGRAM_DISTILL_MODEL` into `~/.claude/settings.json` (backed up first as `settings.json.engram-<time>.bak`) and registers the `engram` MCP server at user scope |
| one project (good for a first try) | `./install.sh --project /absolute/path` | writes the project's `.claude/settings.local.json` and `.mcp.json` only |

Both install the binary to `~/.local/bin/engram` and download a 129 MB
embedding model into `~/.local/share/engram/models`.

### 1. Check the prerequisites

Check, and do not install anything without asking the user:

```bash
uname -s                 # Darwin or Linux (on Windows, work inside WSL 2)
cargo --version          # Rust, stable; if missing, ask before installing it from https://rustup.rs
cc --version             # a C compiler (macOS: xcode-select --install; Debian/Ubuntu: build-essential)
curl --version
claude auth status       # must show "loggedIn": true
```

If `claude auth status` shows `"loggedIn": false`, ask the user to run
`claude auth login` in their own terminal (it is interactive). Engram still
records and recalls without it, but its distiller runs `claude -p` after each
turn and fails, so no decisions or rules get written down.

### 2. Install

From the repository root. It is non-interactive, and the first run compiles
for several minutes (a release build), so give the command a long timeout
(10 minutes) or run it in the background and wait for it:

```bash
./install.sh                              # or: ./install.sh --project /absolute/path
```

Options: `--distill-model NAME` (default `haiku`), `--prefix DIR` (default
`~/.local`), `--no-wire` (build and install the binary only). The script stops
at the first failed step with a message saying what is missing, ends by running
`engram doctor`, and exits non-zero if any doctor check failed.

### 3. Verify

```bash
cd /path/to/the/project && ~/.local/bin/engram doctor
```

Exit code 0 means ready. Each line is `ok`, `warn` (advisory) or `FAIL`, and a
`FAIL` line says how to fix it. Report any `FAIL` to the user rather than
editing their settings by hand.

### 4. Tell the user what happens next

- Engram takes effect in **new** Claude Code sessions. The session you are
  running in started before the hooks existed: it records nothing and has no
  `memory_*` tools until the user starts a new one.
- `~/.local/bin/engram ui` opens a local inspector of what was stored, per
  project and per session. It prints a URL with an access token; on a remote
  machine, forward the port first (`ssh -L 8765:127.0.0.1:8765 host`).
- To remove it: `~/.local/bin/engram uninstall --user` (or
  `engram uninstall --project /absolute/path`). Stores are kept under
  `~/.local/share/engram/projects/`.

### Do not

- edit `~/.claude/settings.json`, `~/.claude.json` or a project's `.mcp.json`
  by hand; `engram install` and `engram uninstall` merge their entries and
  leave everything else alone;
- commit a project's `.claude/settings.local.json` or `.mcp.json` after a
  project install unless the user wants that: both contain the absolute path
  of the engram binary on this machine;
- copy, upload or paste the contents of a store (`memory.strata`,
  `events.log`): they are transcripts of the user's sessions.

## Work on this repository

| path | what |
|---|---|
| `engram/` | the memory system (Rust crate): hooks client, per-project daemon, MCP server, recall, distiller, `engram ui`, `engram doctor` |
| `engram/demo/index.html` | the inspector page; it is compiled into the binary with `include_str!`, so rebuild after editing it |
| `strata/` | the database (Rust crate), including the posting-list study in `src/postings/` |
| `strata/fuzz/` | cargo-fuzz targets (nightly: `cargo +nightly fuzz run <target>` there) feeding ChloeDB damaged files and arbitrary SQL; `strata/tests/corrupt_input.rs` runs the same targets on stable |
| `strata-server/` | ChloeDB over HTTP JSON and gRPC, a coordinator over shards, and the UI: the command center (`ui/index.html` and the modules under `ui/app/`, listed in `src/ui.rs`) and the query console (`ui/classic.html`), compiled in with `include_str!`; `STRATA_UI_DIR=strata-server/ui` serves them from disk without a rebuild |
| `strata-crawl/` | the crawler (Rust, a client of strata-server over HTTP): `strata-crawl crawl URL --local DIR` crawls into a cluster it starts and grows; `serve` is its API, which the command center shows as an app (`strata-server --app crawler=URL`); its view is `ui/view.js`, compiled in (`STRATA_CRAWL_UI_DIR=strata-crawl/ui` reads it from disk) |
| `strata-sheets/` | the spreadsheet app (Rust, a client of strata-server over HTTP, as the crawler is): workbooks whose cells are rows of ChloeDB tables, with IronCalc's formulas, the cluster's tables opened as live sheets, SQL query blocks, xlsx, CSV and SQLite in and out; `serve --local DIR` starts a server and is its app `sheets`; its view is `ui/view.js`, compiled in (`STRATA_SHEETS_UI_DIR=strata-sheets/ui` reads it from disk); `tests/cluster.rs` runs it against a real strata-server |
| `strata-js/` | ChloeDB from JavaScript (`import { open } from "strata-db"`): a Node-API addon (napi-rs) that Node, Bun and Deno load, and a `fetch` client of strata-server (`client.js`); `npm run build -- --profile ci && npm test` there |
| `strata-py/` | ChloeDB from Python (`import strata`): a PyO3 extension built with maturin, and an HTTP client of strata-server; tests in `strata-py/tests` |
| `strata-java/` | ChloeDB from Java (`ai.chloetech.strata`, and a JDBC driver, `jdbc:strata:`, in `ai.chloetech.strata.jdbc`): a JNI library (the Rust crate, `src/*.rs`) and the Java package (`src/main/java`), built together by `mvn package` (Maven's output in `target/java`); JUnit tests in `src/test/java` |
| `.github/workflows/ci.yml` | CI: tests and clippy for the crates (strata, engram, strata-server, strata-crawl, strata-sheets) on Linux and macOS, ChloeDB's fault-injection tests (`--features fault-injection`), a relevance gate (BM25 on three BEIR collections), Miri over ChloeDB's byte views (nightly; tests sized for it are named `miri_*`), the Python and Java packages' tests, and a server smoke test; one job, `CI result`, sums them up and is the check main requires (on a pull request it does not wait for the macOS job, whose check still turns red) |
| `.github/workflows/chaos.yml` | every night (or by hand, with a list of scenarios), `strata-chaos run` plays the chaos monkey's scenarios on a local cluster on one runner, as root, and fails when a write was lost or applied twice; its results.md is the run's summary |
| `scripts/ci-local.sh` | CI's jobs on this machine (macOS or Linux), on the merge of a commit into main; `--post-status` records the result on GitHub as commit status `ci-local`, `--list` lists the jobs, and the header says what a Mac cannot test |
| `.github/CODEOWNERS`, `.github/pull_request_template.md` | the one-way doors (on-disk format, shard protocol, replication, auth), which wait for a human review; a pull request says whether it touches one |
| `docs/` | the website and the documentation: GitHub Pages publishes the folder as it is (no Jekyll, `docs/.nojekyll`), its root redirects to the website in `docs/site/`, and the Markdown pages are read on GitHub; `docs/demo/inspector.html` is a separate static copy of the inspector |
| `install.sh` | the installer described above |
| `packaging/chloedb/` | placeholder packages that hold the name `chloedb` on PyPI, npm and crates.io; `.github/workflows/reserve-names.yml` publishes them, run by hand |

Build and test each crate from its own directory (`engram` and
`strata-server` depend on `strata` by path):

```bash
cd strata && cargo test --release
cd engram && cargo test --release
cd strata-server && cargo test --release
cd strata-crawl && cargo test --release   # its crawl into a cluster needs strata-server built (or STRATA_SERVER_BIN)
cd strata-sheets && cargo test --release  # its cluster test needs strata-server built (or STRATA_SERVER_BIN)
```

strata-server's integration tests build as one test binary, `tests/all.rs`,
with a module per file: `cargo test --test all sim::` runs `tests/sim.rs`.
A new test file needs `mod <name>;` in `tests/all.rs`, or a `[[test]]` in
strata-server's Cargo.toml when it needs the process to itself (`all.rs` says
which do and why); a test fails when a file is in neither.

The Java package builds with Maven, which runs cargo for its native library
(`-Dcargo.profile=ci` skips link-time optimisation when only the tests
matter):

```bash
cd strata-java && mvn -Dcargo.profile=ci package
```

The Python package builds into a virtualenv (its client tests start
`strata-server/target/release/strata-server`, so build that first):

```bash
cd strata-py && python3 -m venv .venv && . .venv/bin/activate
pip install maturin pytest && maturin develop --release && pytest tests
```

The JavaScript package builds its addon with cargo (its client tests start
`strata-server/target/release/strata-server` too):

```bash
cd strata-js && npm run build -- --profile ci && npm test
```

CI also runs `cargo clippy --release --all-targets -- -A clippy::style -A
clippy::complexity -A clippy::perf -D warnings` in each crate.

CI runs the tests with `cargo test --profile ci`: release without link-time
optimisation and with 16 codegen units (each crate's `Cargo.toml`), which
builds ChloeDB's hundred test binaries in minutes rather than half an hour.
Use it locally too when only the tests matter; benchmarks keep `--release`.

Benchmark numbers in `docs/`, the reports and `*/results/` are measured, not
edited: change them only by re-running the benchmark, and name the machine a
number came from.

A change to ChloeDB's ranking fails CI's relevance gate until the new numbers
are committed. The test writes them:

```bash
strata/tests/fixtures/beir/fetch.sh ~/beir    # pinned by hash
cd strata && STRATA_BEIR_DIR=~/beir STRATA_RELEVANCE_GATE=update cargo test --release --test relevance_gate
```

Say in the pull request why the numbers moved.
