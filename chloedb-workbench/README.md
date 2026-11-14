# ChloeDB Workbench

Workbench is a local app that runs many Claude Code sessions side by side.
Each one gets a real terminal (a pseudo-terminal), and most get their own git
worktree on a branch `wb/<name>`. Every session's state and events are rows
of an embedded ChloeDB database. Think of it as tmux with a friendlier UI and
a database as its source of truth. [SPEC.md](SPEC.md) is the contract that
this build follows.

## Run it

```bash
cd chloedb-workbench
cargo build --release
./target/release/chloedb-workbench serve
```

It prints a URL that carries a token (`http://127.0.0.1:7740/?token=...`).
It does not open a browser for you. Open the URL and start an agent: give
it a repository path and a task. The settings:

| flag | env | default |
|---|---|---|
| `--addr` | `CHLOEDB_WORKBENCH_ADDR` | `127.0.0.1:7740` (loopback only) |
| `--data-dir` | `CHLOEDB_WORKBENCH_HOME` | `~/.local/share/chloedb-workbench` |
| `--claude-bin` | `CHLOEDB_WORKBENCH_CLAUDE` | `claude` |
| `--max-agents` | | 8 running at once |
| `--max-depth` | | 2: an agent you start may start agents, and those may not |
| `--chloectx-bin` | `CHLOEDB_WORKBENCH_CHLOECTX` | `chloectx` (the decision trail) |
| `--token` | `CHLOEDB_WORKBENCH_TOKEN` | random |

The data dir holds:

- `workbench.chloedb`: the tables `agents` and `events`
- `sessions/<id>/`: each session's `settings.json` (its hooks) and `mcp.json`
- `worktrees/<repo>-<hash>/<name>/`: the worktrees

Workbench never edits your own Claude Code settings. Each session's hooks
are added to yours, so ChloeCtx keeps working.

When `CHLOEDB_WORKBENCH_UI_DIR=chloedb-workbench/ui` is set, the server
reads the UI from disk instead of the copy built into the binary, so UI
changes show up without a rebuild.

## What it serves

Every `/api` route except `/api/health` needs the token, sent as
`Authorization: Bearer <token>`. The stream and the terminal WebSocket can
take it as `?token=` instead. The server answers only to the hosts
`127.0.0.1`, `localhost` and `[::1]`. It refuses a request from another
origin when that request would change something.

- `GET /api/health`
- `GET /api/agents?all=1`, `POST /api/agents`, `GET /api/agents/:id`
- `GET /api/agents/:id/events?after=&limit=&data=1`
- `POST /api/agents/:id/input|resize|stop`
- `GET /api/attention`: the agents that are waiting for you or idle
- `GET /api/wait?agent=&until=idle|waiting|exited|any&timeout_s=`
- `POST /api/hook`: what `chloedb-workbench hook` posts
- `GET /api/stream`: server-sent events (`snapshot`, `agent`, `event`)
- `GET /api/agents/:id/pty`: a WebSocket with the terminal's output (its
  last 256 KiB first), which also takes keystrokes and
  `{"resize":[cols,rows]}`

`chloedb-workbench hook` is the program each session's hooks run.
`chloedb-workbench mcp` is the MCP server (`workbench`) that each session
gets. Its tools are `spawn_agent`, `list_agents`, `read_agent`,
`wait_agent`, `send_to_agent` and `stop_agent`, which let an agent start
other agents and steer them while you watch.

## The decision trail

Workbench records every session's terminal to
`sessions/<id>/term.cast` (asciicast v2, at most 64 MiB a session; past
that it stops and an event of kind `recording` says so). It keeps each
agent's `base`, the commit its worktree started from, and `started_ms`,
the zero of its recording. On top of those, and of ChloeCtx when it is
installed, these routes answer for any line an agent wrote:

- `GET /api/agents/:id/recording?from_ms=&to_ms=`: the window's events
  (`[ms, "o"|"r", data]`), the terminal's size at `from_ms`, and `before`,
  the screen's last 256 KiB before it, so a player can start mid-session
- `GET /api/agents/:id/diff`: what the agent changed since `base`, committed
  or not, with untracked files as added
- `GET /api/why?agent=<id>&path=<rel>&line=<n>` (or `repo=`): the decision
  card. It runs `chloectx blame <path>:<line> --json` in the worktree, maps
  the edit's session to its agent, walks the agent's parents, finds the
  prompt and who gave it (you, or the agent that spawned it or typed it with
  `send_to_agent`), and places Rewind 20 s before the edit's tool call. When
  ChloeCtx is missing or has no record, it answers from git blame and
  Workbench's events; `found` says where the answer came from and `missing`
  what could not be found
- `GET /api/decisions?repo=&limit=`: the repository's decision notes, through
  `chloectx sql --json`
- `POST /api/ask {repo, question}`: `chloectx ask --json`, plus the agent of
  each record whose session is one of Workbench's

`--chloectx-bin` (`CHLOEDB_WORKBENCH_CHLOECTX`, default `chloectx`) names the
ChloeCtx program. The tests use a fake one that prints canned JSON.

## Tests

```bash
cargo test --profile ci
```

The tests use a fake `claude`, a shell script that runs the hooks of the
settings file it is given, so they call no model.

## Not done yet

- Agents do not outlive the server. Stopping it ends them, and on the next
  start the agents left behind are marked `exited`. Resuming a session
  (`claude --resume`) is not wired up.
- Worktrees and `wb/*` branches are kept when an agent ends. A new agent
  with the same name picks up its worktree again. Nothing removes them.
- Claude Code's first-run questions (theme, trusting the folder) appear in
  the terminal. Answer them there.
- Not in this build: the board, capsules on handoff,
  change requests for shell actions, a desktop shell (Tauri) and team mode.
