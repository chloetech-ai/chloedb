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

- `workbench.chloedb`: the tables `agents` and `events`, and the levels' (`goals`, `outcomes`, `questions`, `rules`, `deliveries`, `seen`)
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

## Map, Preview and Flow

Three of the levels in [SPEC.md](SPEC.md) ("Levels (third build)") have
their backend here. Each reads Workbench's rows, git and, when it is
installed, ChloeCtx:

- `GET /api/map?repo=`: the repository as territory. You get the tracked
  files sized by lines (folders to depth 3, small files folded into "…"),
  each agent's changes since `base`, and the files it read in the last 10
  minutes. It also lists collisions: `both_changed` when two agents
  changed one file, `heading_into` when one agent changed a file another
  just read. Pins are ChloeCtx's decisions, preferences and facts on the
  files they name, at a line when the record's anchor points at one.
- `PUT /api/agents/:id/preview {url}` (or the MCP tool `set_preview`, for
  an agent that started a dev server) takes `http://127.0.0.1:<port>` or
  `http://localhost:<port>` only. `/preview/<agent>/` then shows that app
  through Workbench with an overlay: hover outlines an element, and a
  click sends it to the Workbench page instead of the app.
  `POST /api/preview/route` finds the agent whose diff made the picked
  element. Open the preview once with `?token=`; a cookie scoped to
  `/preview/` carries the token after that. Absolute links in HTML are
  rewritten to stay in the preview. An app that builds URLs in scripts
  should run under the base path `/preview/<agent>/` and set its preview
  URL with that path. The previewed page runs in Workbench's origin, so
  preview only apps you trust as you trust the agents' code.
- `GET /api/flow?repo=&minutes=60`: where the last hour went, per agent,
  as model, tools, tests, waiting on you, redo (a call that failed with an
  error seen before) and idle. It adds the tokens from the sessions'
  transcripts and suggestions from a coach. `POST /api/flow/apply {id,
  repo}` applies one. An "allow" adds a Bash prefix you approved 3 or more
  times to `permissions.allow` for agents started from then on, in that
  repository only. A "fix" (with `text`) saves the fix for a repeated
  error through `chloectx remember`. `chloectx report --json --days 1`
  comes along as `report`.

The state database also has a `marks` table (small key/value rows: the
allow-lists, applied suggestions) and `agents.preview_url`.

## The levels: goals, questions, story, away

The third build reads the fleet at higher levels than terminals. The
backend keeps new tables (`goals`, `outcomes`, `questions`, `rules`,
`deliveries`, `seen`) and gives each agent a `goal`, an `outcome` and a
`narration`: one live sentence of what it does, made after each
`PostToolUse`, `Stop` and `UserPromptSubmit` from the tail of its session's
transcript (at most its last 256 KiB, off the request path). Nothing calls
a model.

- **Goals**: `POST /api/goals {repo, text, plan: true}` makes a goal and
  starts a lead agent that breaks it into outcomes (`add_outcome`), starts
  an agent per outcome (`spawn_agent` with `outcome`) and marks them done
  (`update_outcome`). An outcome's state follows its agent's.
  `GET /api/goals/:id` adds the goal's agents, open questions, what
  ChloeCtx learned in the last day (`chloectx sql --json`) and its pulse.
- **Questions and rules**: the MCP tool `ask_human` asks you in Workbench
  and waits for the answer; a permission prompt becomes a question too
  (answering it types the option's number). Each question carries what
  `chloectx ask --json` found about it. An answer can become a rule
  (`chloectx remember decision`), which every agent of the repository is
  told, and which answers the same question when it is asked again.
- **Delivery**: `chloedb-workbench hook` prints what the server answers for
  `SessionStart`, `UserPromptSubmit` and `PostToolUse` as
  `hookSpecificOutput.additionalContext`: rules, steers
  (`POST /api/agents/:id/steer`) and an agent's outcome, each told once.
  It still never fails a hook.
- **Story**: `GET /api/agents/:id/story` reads an agent's turns as
  chapters of grouped steps, with its tokens (from the transcript's
  `usage`) and what ChloeCtx put in its context (`chloectx receipts
  --json`), déjà vus marked.
- **Away**: `POST /api/seen` and `GET /api/digest` tell what happened since
  you last looked: outcomes done, questions decided and open, what was
  learned, a line per agent, and the moments of decisions to replay.

SPEC.md, "The levels' backend as built", has the JSON shapes.

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
- The preview proxies GET, HEAD and POST but not WebSockets, so a dev
  server's hot reload does not reach the page through it.
- Not in this build: the board, capsules on handoff,
  change requests for shell actions, a desktop shell (Tauri) and team mode.
