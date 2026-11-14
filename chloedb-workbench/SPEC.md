# ChloeDB Workbench: the MVP's contract

This file is the contract the first build works to: the backend (Rust,
`src/`), the UI (`ui/`) and ChloeCtx's changes are written in parallel
against it. Concept and roadmap:
[An agentic IDE on ChloeDB](https://claude.ai/artifact/UTvWqZFB3tgFgjkVLMzCZw).

Workbench is a local app that runs many Claude Code sessions side by side,
each in a real terminal (a pseudo-terminal), most often each in its own git
worktree, and keeps every session's state and events as rows of a ChloeDB
database. It is a UI-friendly tmux whose truth is a database.

The first build is a local web app (`chloedb-workbench serve` prints a URL
with a token and opens nothing by itself); a desktop shell (Tauri) can wrap
the same pages later.

## Scope of this build

1. `chloedb-workbench serve`: the supervisor, the state database, the HTTP
   API, the server-sent events stream, a WebSocket per terminal, and the UI.
2. `chloedb-workbench hook`: what each session's hooks run; it posts the
   hook's JSON to the server and never fails or prints.
3. `chloedb-workbench mcp`: an MCP server over stdio with the tools that let
   an agent start and steer other agents (`spawn_agent` and its siblings).
4. ChloeCtx (separate crate, `chloectx/`): one store per repository rather
   than per worktree, the `Notification` and `SubagentStop` hooks recorded,
   and `chloectx report`, where a project's time went.

Not in this build: the board (blackboard), capsules on handoff, review with
blame, change requests for shell actions, the Tauri shell, team mode.

## Crate layout

```
chloedb-workbench/
  Cargo.toml          # crate and binary `chloedb-workbench`; chloedb by path
  src/main.rs         # clap: serve | hook | mcp | --version
  src/db.rs           # the state database (ChloeDB, embedded)
  src/supervisor.rs   # PTYs (portable-pty), processes, scrollback rings
  src/git.rs          # worktrees
  src/server.rs       # axum: HTTP API, SSE, WebSocket, the UI's files
  src/hook.rs         # the hook subcommand
  src/mcp.rs          # the MCP subcommand (a client of the HTTP API)
  ui/index.html, ui/app.js, ui/style.css   # compiled in with include_str!
  tests/              # with a fake `claude` (a shell script), no model calls
```

`CHLOEDB_WORKBENCH_UI_DIR=chloedb-workbench/ui` serves the UI from disk
without a rebuild, as the other apps do. Tests run with
`cargo test --profile ci`.

## Settings

| flag | env | default |
|---|---|---|
| `--addr` | `CHLOEDB_WORKBENCH_ADDR` | `127.0.0.1:7740` |
| `--data-dir` | `CHLOEDB_WORKBENCH_HOME` | `~/.local/share/chloedb-workbench` |
| `--claude-bin` | `CHLOEDB_WORKBENCH_CLAUDE` | `claude` |
| `--max-agents` | | 8 running at once |
| `--max-depth` | | 2 (an agent started by an agent started by you may start no more) |
| `--token` | `CHLOEDB_WORKBENCH_TOKEN` | random, printed once in the URL |

The server listens on loopback only, requires the token on every `/api`
route (`Authorization: Bearer`, or `?token=` for the WebSocket and the
EventSource, which cannot set headers), answers only to the hosts
`127.0.0.1`, `localhost` and `[::1]`, and refuses a request that changes
something when its `Origin` is not its own.

## The state database

`<data-dir>/workbench.chloedb`, opened once by the server (one writer).

```sql
CREATE TABLE agents (
  id KEYWORD PRIMARY KEY,      -- 'a' + 8 hex
  name KEYWORD,                -- unique among running agents; [a-z0-9-], at most 32
  repo KEYWORD,                -- absolute path of the repository's main checkout
  cwd KEYWORD,                 -- where the session runs (a worktree or the repo)
  branch KEYWORD,
  task TEXT,                   -- the first prompt
  parent KEYWORD,              -- the agent that spawned it, or NULL for you
  depth INT,
  state KEYWORD,               -- starting | working | waiting | idle | exited
  state_since TIMESTAMP,
  waiting_for TEXT,            -- the notification's message while waiting
  last_event TEXT,             -- one line: what it did last
  session_id KEYWORD,          -- Claude Code's session id (set with --session-id)
  pid INT,
  exit_code INT,
  model KEYWORD,
  created TIMESTAMP,
  ended TIMESTAMP
);
CREATE TABLE events (
  id KEYWORD PRIMARY KEY,      -- agent id + ':' + seq, zero-padded
  seq INT,                     -- global, increasing
  ts TIMESTAMP,                -- milliseconds
  agent KEYWORD,
  kind KEYWORD,                -- a hook's event name, or spawned | input | exited
  tool KEYWORD,                -- the tool's name, for tool events
  summary TEXT,                -- one line for people
  data TEXT                    -- the hook's JSON, trimmed to 16 KB
);
```

State from events: `spawned` → starting; `SessionStart`,
`UserPromptSubmit`, `PreToolUse`, `PostToolUse` → working; `Notification` →
waiting (`waiting_for` = its message); `Stop` → idle (its turn is over, it
waits for a prompt); process exit or `SessionEnd` → exited. A waiting agent
that gets input or a tool event goes back to working. The attention queue is
the agents waiting or idle, oldest `state_since` first.

## How a session is started

1. A worktree, unless `worktree: false`: `git -C <repo> worktree add
   <data-dir>/worktrees/<repo-name>-<hash8>/<name> -b wb/<name> <base>`
   (`base` defaults to the repo's current `HEAD`). `repo` must be a git
   checkout; a worktree passed as `repo` is read back to its main checkout.
2. A settings file `<data-dir>/sessions/<id>/settings.json` holding hooks
   for `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`,
   `Notification`, `Stop`, `SubagentStop` and `SessionEnd`, each running
   `<this exe> hook` with a timeout of 5 s. It adds to the user's own
   settings and hooks (ChloeCtx's keep working); it never edits theirs.
3. An MCP config `<data-dir>/sessions/<id>/mcp.json` naming the server
   `workbench`: `<this exe> mcp`.
4. The process: `<claude-bin> --session-id <uuid> --settings <file>
   --mcp-config <file> [--model M] [<task>]` in a PTY of 120 x 32, cwd the
   worktree, with `CHLOEDB_WORKBENCH_URL`, `CHLOEDB_WORKBENCH_TOKEN` and
   `CHLOEDB_WORKBENCH_AGENT` in its environment (the hook and MCP
   subcommands read them).

The supervisor keeps the last 256 KiB of each terminal's output in memory
and replays it to a WebSocket that attaches. Agents do not outlive the
server in this build: stopping the server ends them (SIGHUP, then SIGKILL
after 5 s); at start, agents left `working` by a crash are marked `exited`.

## HTTP API

All JSON. Errors are `{"error": "...", "error_code": "..."}` with a fitting
status (400 bad_request, 401 unauthenticated, 404 not_found, 409 conflict,
429 limit_exceeded).

| route | body | answer |
|---|---|---|
| `GET /api/health` | | `{ok, version, agents_running, max_agents}` (no token needed) |
| `GET /api/agents?all=1` | | `{agents: [Agent]}`, running ones first; `all=1` adds exited ones |
| `POST /api/agents` | `{repo, task?, name?, branch?, base?, worktree?: true, model?, parent?}` | `{agent: Agent}` (201); 429 past `--max-agents`, 409 past `--max-depth` or a name in use |
| `GET /api/agents/:id` | | `{agent: Agent}` |
| `GET /api/agents/:id/events?after=<seq>&limit=200` | | `{events: [Event]}`, oldest first |
| `POST /api/agents/:id/input` | `{text, enter?: true}` | `{ok}`; writes to the terminal (and `\r` when `enter`) |
| `POST /api/agents/:id/resize` | `{cols, rows}` | `{ok}` |
| `POST /api/agents/:id/stop` | | `{ok}`; SIGHUP, then SIGKILL after 5 s |
| `GET /api/attention` | | `{agents: [Agent]}`: waiting or idle, oldest first |
| `GET /api/wait?agent=<id>&until=idle\|waiting\|exited\|any&timeout_s=30` | | `{agent, reached: bool}` once the state is reached (`any`: any new event), or at the timeout |
| `POST /api/hook` | the hook's stdin JSON, plus header `x-workbench-agent` | `{ok}` |
| `GET /api/stream` | | `text/event-stream`: `event: agent` with an Agent on every change, `event: event` with each new Event; a first `event: snapshot` with `{agents}` |
| `GET /api/agents/:id/pty` | WebSocket | server to client: binary frames of terminal output (the scrollback first); client to server: binary or text frames of keystrokes, and a text frame `{"resize":[cols,rows]}` |

`Agent` is the row as JSON (times as ISO 8601 text) plus `running: bool`
and `children: [id]`. `Event` is the row as JSON without `data` unless
`?data=1`.

## The hook subcommand

`chloedb-workbench hook` reads the hook's JSON from stdin, posts it to
`$CHLOEDB_WORKBENCH_URL/api/hook` with the token and agent headers, waits at
most 2 s, prints nothing and exits 0 in every case. Without the variables it
exits 0 at once.

## The MCP subcommand

`chloedb-workbench mcp`: MCP over stdio (protocol 2025-06-18, JSON-RPC
lines), a client of the HTTP API with the variables above. Its caller is
`$CHLOEDB_WORKBENCH_AGENT`, which becomes `parent` of what it spawns.

| tool | arguments | does |
|---|---|---|
| `spawn_agent` | `task`, `name?`, `worktree?` (true), `model?`, `base?` | starts an agent on the caller's repo; returns its id and name |
| `list_agents` | `all?` | the agents this caller started (all of them with `all`) |
| `read_agent` | `id`, `after?` | its state and its events after a seq, as lines |
| `wait_agent` | `id`, `until?` (`idle`), `timeout_s?` (300, at most 1800, polling `/api/wait` in 30 s steps) | waits, then answers its state and its last events |
| `send_to_agent` | `id`, `text` | types the text and Enter into its terminal |
| `stop_agent` | `id` | stops it |

Its instructions at `initialize` say that spawned agents are real sessions
the user watches, cost tokens, and should get a task that stands alone.

## The UI

One page, no build step: `ui/index.html`, `ui/app.js` (an ES module),
`ui/style.css`. xterm.js and its fit add-on from a pinned CDN URL
(`https://cdn.jsdelivr.net/npm/@xterm/xterm@5.5.0/...`,
`@xterm/addon-fit@0.10.0`). Light and dark themes from the system.

- **Top bar**: Workbench, the count of running agents, a **New agent**
  button (`n`).
- **Attention rail** (left): agents waiting or idle, oldest first, each
  with its name, why (`waiting_for`, or "turn finished"), and how long; a
  click zooms it.
- **Grid**: a tile per running agent (exited ones under a fold): name,
  state chip (starting, working, waiting for you, idle, exited), branch,
  task (two lines), last event, time in state; waiting tiles are outlined
  in the warning colour. Each tile holds a live, small, read-only terminal
  preview, or its last event when there are many tiles.
- **Zoom** (click, Enter or `z`): one agent's full terminal, interactive,
  with its events beside it; Esc goes back to the grid.
- **New agent** dialog: repo path (remembered), name, task, own worktree
  (ticked), model.
- **Prefix keys** as tmux's: `Ctrl-b` then `n` new, `z` zoom, arrows move
  between tiles, `x` stop (asks first), `?` help.
- State changes come from `/api/stream`; nothing polls.
- The token comes once in the URL (`/?token=...`), is kept in
  `sessionStorage`, and the address bar drops it.

## Decision trail (second build)

Every line an agent wrote can answer **who** decided it (which agent, started
by whom, on whose prompt), **when** (the turn, the time, the commit),
**where** (repository, worktree, branch, file and line), **what it knew**
(the memory ChloeCtx had put in its context, and what became of each record
since) and **why** (the decisions distilled from that turn). The surprise:
**Rewind**, which plays the agent's own terminal back at the moment it wrote
the line, from the recording Workbench keeps of every session.

It is built on `chloectx blame --json` (docs/chloectx/receipts.md), ChloeCtx's
decision notes and Workbench's own rows. When `chloectx` is not installed or
has no record of the line, the card still answers from Workbench's rows (the
agent, its task, the time from git) and says what it could not find.

### Backend

- **Terminal recording.** The supervisor appends every chunk of a terminal's
  output to `<data-dir>/sessions/<id>/term.cast`, asciicast v2 (a header
  line `{"version": 2, "width", "height", "timestamp"}`, then
  `[seconds_since_start, "o", "text"]` lines; resizes as `"r"` events
  `"COLSxROWS"`). Capped at 64 MiB per session (then it stops recording and
  says so in an event). Kept after the agent exits.
- `agents` gains `base` (the commit the worktree started from) and
  `started_ms`.
- `GET /api/agents/:id/recording?from_ms=&to_ms=` answers
  `{width, height, started_ms, events: [[ms_since_start, "o"|"r", data]]}`
  for that window (milliseconds since the session started), at most 8 MiB,
  plus `{before: "<the screen's bytes up to from_ms, as one string>"}`
  trimmed to the last 256 KiB, so a player can start mid-session with the
  right screen.
- `GET /api/agents/:id/diff` answers `{base, files: [{path, status,
  hunks: [{old_start, new_start, lines: [{kind: "+"|"-"|" ", text,
  new_line}]}]}]}`: what the agent changed from `base`, committed or not
  (`git diff <base>` in its worktree, untracked files included as added).
- `GET /api/why?agent=<id>&path=<rel>&line=<n>` (or `repo=` instead of
  `agent=` for a line in the main checkout): the decision card as JSON:

  ```
  {
    "line": {"path", "line", "text"},
    "where": {"repo", "worktree", "branch", "commit": {sha, date, subject} | null},
    "who": {
      "agent": Agent | null,             -- the Workbench agent whose session made the edit
      "lineage": [Agent...],             -- its parents, nearest first, up to you
      "prompt": {"text", "ts", "by": "you" | "<parent agent's name>"} | null
    },
    "when": {"edit_ts", "turn", "session"} | null,
    "knew": [{"kind", "id", "text", "score", "decision", "now": "current"|"superseded by …"|"deleted", "untrusted": bool}],
    "why": [{"kind": "decision"|"preference"|"fact", "text", "ts"}],   -- notes of that turn
    "worth_a_look": ["..."],
    "rewind": {"agent", "from_ms", "to_ms"} | null,   -- 20 s before the edit's tool call to 3 s after
    "found": "chloectx" | "git" | "workbench",         -- where the answer came from
    "missing": ["what could not be found, in a sentence each"]
  }
  ```

  It runs `chloectx blame <path>:<line> --json` in the agent's worktree (the
  `--chloectx-bin` flag, default `chloectx`), maps the blame's session id to
  the Workbench agent whose `session_id` it is, and finds the edit's tool
  call among the agent's events (by time and file) to place the rewind.
- `GET /api/decisions?repo=<path>&limit=200`: the repo's decision notes,
  newest first: `{decisions: [{id, text, ts, session, agent: Agent|null,
  superseded_by: id|null, files: [path]}]}`, from `chloectx sql --json
  "SELECT id, ts, session, text, tag, files, links FROM memory WHERE kind =
  'decision' ORDER BY ts DESC LIMIT n"` run in the repo
  (`chloectx sql --json` prints `{"text", "columns": [...], "rows":
  [[...]]}`).
- `POST /api/ask {repo, question}`: `chloectx ask --json` in the repo,
  answered as it prints it, plus the Workbench agents its records' sessions
  map to.
- All four read only, token as the other routes. Tests use a fake
  `chloectx` script that prints canned JSON.

### UI

- **Changes** tab in the zoom view, beside the terminal: the agent's diff,
  file by file. Hovering an added line shows a **Why?** button; `Alt`-click
  or `y` on the focused line opens the card.
- **Decision card** (a side sheet): a headline sentence ("fix-total wrote
  this at 7:41 PM, on your prompt, after recalling 3 notes"), then Who
  (the lineage as a chain of chips: you → lead → add-validate, each
  clickable to zoom that agent), When, Where, Why (decision notes, quoted),
  What it knew (records with a warning chip when superseded or untrusted),
  Worth a look, and **Rewind**.
- **Rewind**: an xterm player that starts from the screen as it was at
  `from_ms` and plays to the edit at 1x, 2x or 4x, with a scrubber and a
  marker where the edit happened; the moment of the edit flashes the line
  in the Changes view beside it.
- **Decisions** screen (top bar, `d`): every decision of the repo on a time
  axis, one lane per agent (and one for decisions with no agent: other
  sessions), superseded ones struck through and linked to what replaced
  them; a click opens the card for the decision with the lines that came
  from its turn. An **Ask why** box above it asks ChloeCtx in a sentence
  ("why do we round half up?") and answers with the records and their
  agents.
