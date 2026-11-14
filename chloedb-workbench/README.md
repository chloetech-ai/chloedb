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
- Not in this build: the board, capsules on handoff, review with blame,
  change requests for shell actions, a desktop shell (Tauri) and team mode.
