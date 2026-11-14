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

## Levels (third build)

The user's ask: Workbench must be significantly more than terminals in a
grid: more abstraction, with the detail there when needed, from ChloeCtx.
The answer is **semantic zoom**. Everything can be read at five levels,
and the terminal is the last of them:

| level | for the fleet | for one agent |
|---|---|---|
| Intent | **Mission** (goal, outcomes, one sentence per agent), **Away** (the digest) | its outcome and its sentence |
| Plan | **Decide** (questions with precedent, rules), **Flow** (where time went, coach) | its questions |
| Story | | **Story** (chapters per turn, grouped steps, what it knew, déjà vu) |
| Changes | **Map** (territory, collisions, ◆ decisions), **Preview** (the running app) | its diff with Why? (second build) |
| Terminal | the grid (first build) | its terminal |

`[` and `]` move one level down or up for what is in focus; the top bar
shows the level. Concept screens: `/mnt/project-files/workbench/concepts/`
(`1-mission.png` … `7-away.png`, with their HTML). The real screens take
their look (layout, density, the ChloeCtx ◆ chips), in the existing
Workbench theme, light and dark.

Nothing here calls a model on its own, except where marked *optional*
(off unless `--narrate-model` is set). Everything is derived from
Workbench's rows, the hook payloads, the session transcripts (read from
`transcript_path` in the hook payloads) and `chloectx` run in the repo.

### New tables

```sql
CREATE TABLE goals (
  id KEYWORD PRIMARY KEY,  -- 'g' + 8 hex
  repo KEYWORD, text TEXT, state KEYWORD,   -- active | done | dropped
  lead KEYWORD,            -- the agent planning it, if any
  created TIMESTAMP, done TIMESTAMP
);
CREATE TABLE outcomes (
  id KEYWORD PRIMARY KEY,  -- 'o' + 8 hex
  goal KEYWORD, title TEXT, detail TEXT,
  state KEYWORD,           -- next | active | needs_you | done | dropped
  agent KEYWORD,           -- who works on it (NULL while next)
  ord INT, after KEYWORD,  -- shown order; an outcome it waits for
  progress INT,            -- 0..100, set by the agent or derived
  created TIMESTAMP, updated TIMESTAMP
);
CREATE TABLE questions (
  id KEYWORD PRIMARY KEY,  -- 'q' + 8 hex
  agent KEYWORD, outcome KEYWORD, repo KEYWORD,
  kind KEYWORD,            -- ask (from ask_human) | permission (a Notification asking to use a tool)
  text TEXT, context TEXT,
  options TEXT,            -- JSON [{label, detail?}]
  recommended INT,
  precedent TEXT,          -- JSON: what `chloectx ask --json` found when it was asked
  state KEYWORD,           -- open | answered | expired
  answer TEXT, answered_by KEYWORD,  -- 'you' | 'rule:<id>' | 'terminal'
  rule KEYWORD,            -- the rule made from the answer, if any
  created TIMESTAMP, answered TIMESTAMP
);
CREATE TABLE rules (
  id KEYWORD PRIMARY KEY,  -- 'r' + 8 hex
  repo KEYWORD, text TEXT,
  question KEYWORD,        -- where it came from
  ctx_id KEYWORD,          -- the ChloeCtx record id (`chloectx remember decision`)
  active BOOL, created TIMESTAMP
);
CREATE TABLE deliveries (  -- what each agent has been told (rules, steers)
  id KEYWORD PRIMARY KEY,  -- agent + ':' + item id
  agent KEYWORD, item KEYWORD, ts TIMESTAMP
);
CREATE TABLE marks (       -- small key/value state: last_seen, preview urls, allow-lists
  "key" KEYWORD PRIMARY KEY, value TEXT, ts TIMESTAMP   -- quoted: KEY is reserved
);
```

`agents` gains `goal`, `outcome`, `narration` (one sentence), `narration_ts`
and `preview_url`.

### Narration (one live sentence per agent)

After each `PostToolUse`, `Stop` and `UserPromptSubmit`, the server reads
the tail of the session's transcript (the `transcript_path` of the payload;
read at most the last 256 KiB) and sets `narration` to the first sentence
of the latest assistant text block (at most 160 characters, Markdown
stripped). When the latest thing is a tool call with no text after it, it
is a sentence made from the call ("Running `cargo test -p harbor`",
"Editing src/ledger/post.rs", "Reading 4 files in src/refunds"). A waiting
agent's sentence is its question. *Optional:* with `--narrate-model M`,
each finished turn also gets a chapter title and a two-line summary from
`claude -p --model M` (one call per turn, never more than one in flight
per agent), cached in the event's data.

### Goals and outcomes (Mission)

- `POST /api/goals {repo, text, plan?: true}` makes a goal; with `plan`, it
  spawns a lead agent (name `lead`, or `lead-2`…) whose task is the goal
  plus instructions to break it into 3 to 8 outcomes with `add_outcome`,
  start an agent per outcome with `spawn_agent(outcome=…)`, ask the user
  through `ask_human` rather than in its terminal, and mark outcomes done.
- `GET /api/goals?repo=` → `{goals: [Goal]}`; `GET /api/goals/:id` →
  `{goal, outcomes: [Outcome], agents: [Agent], open_questions: n,
  learned: [Record], heads_up: [HeadsUp], pulse: [[minute_ms, count]]}`
  where `learned` is today's ChloeCtx decision, preference, fact and
  error-fix records of the repo (`chloectx sql --json`), `heads_up` the
  collisions from the map, and `pulse` the events per minute for the last
  60 minutes.
- `POST /api/outcomes {goal, title, detail?, after?}`, `PATCH
  /api/outcomes/:id {title?, state?, progress?, agent?}`.
- An outcome's state follows its agent unless set: agent working →
  `active`, waiting or with an open question → `needs_you`, exited with code
  0 after marking it → `done`.

### Questions and rules (Decide)

- MCP tool `ask_human {question, options: [{label, detail?}], recommended?,
  context?}`: posts `POST /api/questions`, then waits (polling `/api/wait`
  style, `GET /api/questions/:id/wait?timeout_s=30`, up to 30 minutes) and
  returns the answer text. The agent's state is `waiting` meanwhile, with
  `waiting_for` = the question.
- A `Notification` that asks permission for a tool also makes a question,
  `kind: permission`, options Yes / Yes, and don't ask again / No (answering
  types `1`, `2` or `3` and Enter into its terminal). Answering in the
  terminal instead marks it `answered_by: terminal`.
- On creation the server runs `chloectx ask --json -- <question>` in the
  repo and stores what it found as `precedent` (records with their agents,
  as `/api/ask` answers). When a precedent record is a Workbench rule or a
  decision whose words match an option's label, that option is marked
  `matches_precedent`.
- `GET /api/questions?repo=&state=` → `{questions: [Question]}`.
- `POST /api/questions/:id/answer {option?: n, text?, rule?: "text"}`:
  answers it (the agent's `ask_human` returns), and with `rule`, runs
  `chloectx remember decision "<rule>"` in the repo and adds a `rules` row.
- `GET /api/rules?repo=`, `PATCH /api/rules/:id {active}`.
- **Delivery.** The `hook` subcommand now prints what the server answers
  to `POST /api/hook` when it is a `UserPromptSubmit`, `SessionStart` or
  `PostToolUse`: `{"hookSpecificOutput": {"hookEventName": E,
  "additionalContext": "..."}}` holding active rules of the repo and
  steers this agent has not been told yet (recorded in `deliveries`).
  It still never fails the hook: any error prints nothing.
- `POST /api/agents/:id/steer {text}`: an idle agent gets it typed with
  Enter; a working one gets it as additional context at its next
  `PostToolUse` ("The user adds: …").

### Story (one agent)

`GET /api/agents/:id/story` →

```
{
  "agent": Agent, "intent": "its outcome's title, or its task",
  "chapters": [{
    "turn": n, "prompt": {"text", "by"}, "started_ms", "ended_ms",
    "title",            -- the optional model title, or the prompt's first words
    "summary",          -- the turn's last assistant text, first two sentences
    "state": "done" | "now",
    "steps": [{"kind": "read"|"search"|"edit"|"run"|"memory"|"agents"|"web"|"other",
               "label": "Read 12 files", "detail": "src/ledger/*, …",
               "count", "started_ms", "ms", "failed": n, "events": [seq...]}]
  }],
  "knew": [{"id", "kind", "text", "score", "turn", "deja_vu": bool}],
  "deja_vu": [{"text", "record", "when", "shown_ms"}],
  "tokens": {"input", "output", "cache_read"}       -- summed from the transcript's usage
}
```

Steps group consecutive tool events of one kind within a turn (Read, Grep,
Glob, LS → read/search; Edit, MultiEdit, Write, NotebookEdit → edit; Bash →
run, grouped by the command's first two words; `mcp__chloectx__*` → memory;
`mcp__workbench__*` → agents; WebFetch, WebSearch → web). `knew` comes from
`chloectx receipts --session <session_id> --json` (add `--json` to it if it
is missing). A recalled record of kind `fix` (ChloeCtx's error → fix), or
one recalled right after a failed tool call, is a **déjà vu**.

### Map

`GET /api/map?repo=` (`src/map.rs`) →

```
{
  "repo": "/abs/path/of/the/main/checkout",
  "tree": Node,
  "agents": [{"agent": Agent, "files": [{"path", "added", "removed", "status"}], "reading": [path]}],
  "collisions": [
    {"path", "agents": [id, id...], "kind": "both_changed", "detail"},
    {"path", "agents": [changer, reader], "kind": "heading_into", "changed_by", "read_by", "detail"}
  ],
  "pins": [{"path", "line": n|null, "record": {"id", "kind", "text", "ts"}, "agent": Agent|null}],
  "missing": ["what could not be found, in a sentence each"]
}
Node = {"name", "path", "kind": "dir", "lines", "files", "children": [Node], "collapsed"?: true}
     | {"name", "path", "kind": "file", "lines"}
     | {"name": "…", "path": "<dir>/…", "kind": "more", "lines", "files"}   -- small files folded
```

- **tree**: `git ls-files` of the main checkout with line counts (binary
  files count 0; files over 4 MiB, and all files once 256 MiB were read,
  are estimated at 40 bytes a line), reused for 30 s while HEAD and the
  index stay the same. The root is depth 0; folders to depth 3 are opened,
  deeper ones are `collapsed` with their size. In a folder, files under
  5% of its lines (and under 50 lines), and files past its 16 biggest,
  fold into one `more` entry, unless an agent changed or read them or a
  pin names them. Folders come first, biggest first, then files.
- **agents**: the repository's agents that run, or ended in the last 24
  hours, oldest first. `files` is the diff from `base` (as `/diff`,
  untracked files as added); agents that share a folder keep only the
  files their own edit calls named. `reading` is the files their `Read`
  calls named in the last 10 minutes, relative to the checkout.
- **collisions**: `both_changed` for every file in two or more agents'
  diffs; `heading_into` for every file one agent changed that another,
  which did not change it, read in the last 10 minutes. `map::collisions(sup,
  repo) -> Vec<Json>` answers the same list for Mission's `heads_up`.
- **pins**: `chloectx sql --json "SELECT id, kind, text, ts, files, ctx,
  session, tag FROM memory WHERE kind = 'decision' OR kind = 'preference'
  OR kind = 'fact' ORDER BY ts DESC LIMIT 1000"` in the main checkout. A
  pin per file a record names, when the file is tracked or in an agent's
  diff; records tagged `superseded` are left out. ChloeCtx's records carry
  no line, so `line` is the one a file entry names (`path:42`,
  `path#L42`), else the first line of the file in the main checkout that
  holds, as a word, one of the identifiers of the record's `Anchor:` line
  (`idents=` in `ctx`), else null.

### Preview

- `PUT /api/agents/:id/preview {url}` → `{agent: Agent}` (`src/preview.rs`):
  `url` is `http://127.0.0.1:<port>` or `http://localhost:<port>`,
  optionally with a path, never Workbench's own port; it is kept without
  a trailing `/` in `agents.preview_url`, and an empty or null `url`
  clears it. 400 for anything else. The agent's row goes out on the stream.
- MCP tool `set_preview {url}` (`src/mcp.rs`): the same for the calling
  agent.
- `GET|HEAD|POST /preview/:agent/<path>?<query>` proxies to
  `<preview_url>/<path>?<query>` (`/preview/:agent` redirects to
  `/preview/:agent/`). Auth: `?token=` on a GET answers `303` to the same
  address without it and sets `wb_preview=<token>; Path=/preview/;
  HttpOnly; SameSite=Strict`; after that the cookie (or a Bearer header)
  is the token. A wrong or missing token is 401, an agent with no preview
  404, an app that does not answer 502 (small HTML pages, for the frame).
  The guard checks the host, and the origin of a POST, as on every route;
  `/api` routes never take the cookie.
- What goes to the app: `Accept`, `Accept-Language`, `Content-Type`,
  `User-Agent`, conditional and range headers, `Authorization` unless it
  is a Bearer token, its own cookies (never `wb_preview`), `Origin` and
  `Referer` rewritten to the app's, and `Accept-Encoding: identity`.
  Redirects are not followed.
- What comes back: hop-by-hop headers and `Content-Length` dropped,
  `X-Frame-Options: SAMEORIGIN`, `Location` to the app (absolute or
  `/path`) rewritten to `/preview/:agent/…`, `Set-Cookie` without
  `Domain` and with its `Path` under `/preview/:agent/`. An HTML answer
  (`text/html`, not encoded) gets `<script src="/preview-overlay.js">
  </script>` before its last `</body>` (else `</html>`, else at the end),
  and its absolute-path links (`src`, `href`, `action`, `formaction`,
  `poster` values that start with one `/`) are rewritten to
  `/preview/:agent/…`. URLs that scripts build (`fetch("/x")`, a bundler's
  module imports) are not: such an app runs its dev server under a base
  path and sets the preview URL with it (`vite --base
  /preview/<agent>/` and `http://127.0.0.1:5173/preview/<agent>`), and
  then links that are already under `/preview/:agent/` are left as they
  are.
- `ui/preview-overlay.js` (compiled in, `/preview-overlay.js`, from disk
  under `CHLOEDB_WORKBENCH_UI_DIR`): in a frame, it outlines the element
  under the pointer, and a click (with its pointer and mouse events)
  never reaches the page but posts to the parent, at Workbench's origin:
  `{type: "wb-pick", selector, text (≤ 1000 chars), html (outerHTML,
  ≤ 2048 bytes), rect: {x, y, width, height}, url}`. It takes messages
  from its parent only: `{type: "wb-pick-mode", on}` turns picking off
  and on (on by default in a frame, off on its own); `{type: "wb-who",
  id, texts?: [string], selector?, max?: 300}` is answered with `{type:
  "wb-who", id, items: [{selector, text, rect}]}`, the visible elements
  with text of their own (under `selector`; with `texts`, those whose text
  holds one of them or is held by one, so the parent can pass an agent's
  added lines). On load it posts `{type: "wb-ready", url}`.
- `POST /api/preview/route {agent, pick}` →

  ```
  {"owner": Agent, "files": [path], "related": [{"id", "kind", "text", "ts", "files"}],
   "matched": [{"path", "line", "text"}], "score": n, "message": "...", "missing": [...]}
  ```

  `agent` is the preview's agent. The pick's needles are its text's
  lines of 3 characters or more and the class names and id of its HTML's
  first tag and of its selector. Each agent of the repository on the map
  scores 3 for each text needle and 1 for each class or id (as a whole
  word) on each added line of its diff; the best score is the owner
  (ties to the preview's own agent), else the preview's own agent with
  no files. `files` are the owner's matching files, best first; `related`
  ChloeCtx's decision, preference and fact records that name them (at
  most 20); `message` is the text to `/steer` the owner with (the
  selector, the text and the HTML).
- Security: the previewed page is served from Workbench's own origin
  (the overlay needs its parent, and the cookie its path), so its scripts
  could reach the Workbench page around it. Preview only an app you trust
  as you trust the agents' code; Workbench refuses any target but
  loopback, so it cannot be turned into a proxy to elsewhere.

### Flow

`GET /api/flow?repo=&minutes=60` (`src/flow.rs`; `minutes` 1 to 10080) →

```
{
  "repo": "/abs/path",
  "window": {"from_ms", "to_ms", "minutes"},
  "totals": {"model_ms", "tools_ms", "tests_ms", "waiting_ms", "redo_ms", "idle_ms"},
  "agents": [{"agent": Agent, "model_ms", "tools_ms", "tests_ms", "waiting_ms", "redo_ms", "idle_ms",
              "segments": [[start_ms, end_ms, "model"|"tools"|"tests"|"waiting"|"redo"|"idle"]],
              "tokens": {"input", "output", "cache_read"}}],
  "tokens": {"input", "output", "cache_read"},
  "suggestions": [{"id", "kind", "title", "detail", "saves": "saves ~4 min/h" | null, "saves_ms": n | null,
                   "action": {"kind", ...} | null, "applied": bool, "from"?: "chloectx"}],
  "report": {...} | null,
  "missing": [...]
}
```

- **Segments**, from each agent's events (the agents of the repository
  that ran during the window). Every event ends the stretch before it and
  starts the one it implies: `spawned`/`SessionStart` idle (or model in a
  turn), `UserPromptSubmit` model, `PreToolUse` tools (tests when an open
  Bash call runs a test runner: `cargo test|nextest`, `pytest`, `python
  -m pytest`, `npm|yarn|pnpm|bun test`, `npm run test*`, `go test`,
  `jest`, `vitest`, `mvn test|verify`, `gradle test`, `make test|check`,
  `rspec`, `phpunit`, `ctest`, `tox`, `mix test`, `dotnet test`, `swift
  test`, …, after `cd …&&` and `VAR=x`), `PostToolUse` back to tools while
  other calls are open, else model; `Notification` waiting (not an idle
  notice of a finished turn) until the next `input`, tool event or prompt;
  `Stop` idle; `exited`/`SessionEnd` nothing. Calls overlap: a PostToolUse
  ends the call of its `tool_use_id`, else the newest open one with its
  tool and input. Neighbours of one kind are joined, and segments are
  clipped to the window.
- **Failures**: a call whose PostToolUse (or `PostToolUseFailure`) says
  it failed (`tool_response.is_error` or `.error`), whose transcript
  `tool_result` of its `tool_use_id` has `is_error: true`, or that is
  still open when the turn ends (unless the user was asked about it: then
  it was refused). Its error is the hook's or the transcript's text, else
  "`<tool>` failed: `<command>`"; the key is its line that says error
  (else its first), numbers masked. **Redo**: the tool and test time of a
  failure whose key an earlier failure in the window had, of any agent.
- **Waiting** is Notification time only for now; open questions join it
  once Decide is merged.
- **tokens**: each `transcript_path` the agent's hook payloads named
  (`.jsonl` files, their last 64 MiB), the `usage` of assistant messages
  timed in the window, once per message id: `input_tokens`,
  `output_tokens`, `cache_read_input_tokens`.
- **Suggestions**, in this order:
  - `allow`, id `allow:Bash(<prefix>:*)`: a Bash call approved at a
    permission prompt (a Notification about permission while it was open,
    then its PostToolUse) 3 or more times with one prefix (the first two
    words, three after `npm run`, `python -m` and the like; never `rm`,
    `sudo`, `curl`, `git push`, shells and the like). `action: {kind:
    "allow", pattern}`; `saves` is the waiting it caused, per hour.
  - `fix`, id `fix:<hash8 of the key>`: one error 3 or more times.
    `action: {kind: "fix", error}`; `saves` is its redo time.
  - `capsule`, id `capsule:<agent>`: files one agent read 4 or more times.
    `action: {kind: "capsule", agent, files}` (it says how; nothing to
    apply).
  - `sequence`, id `sequence:<path>`: agents' edit calls on one file
    switched between agents twice or more. `action: {kind: "sequence",
    path, agents}` (nothing to apply).
  - the report's own, id `report:<i>`, `kind` as the report's, `title` its
    text, `saves_ms` its seconds, `action: null`, `from: "chloectx"`.
- `report` is `chloectx report --json --days 1` run in the main checkout,
  as it prints it, or null (and `missing` says why).
- `POST /api/flow/apply {id, repo, minutes?, text?}`:
  - `allow:…` adds the pattern to the mark `allow:<repo>` (a JSON list),
    and the supervisor writes the repository's list into
    `permissions.allow` of the settings file of every agent it starts from
    then on. → `{ok, id, applied: {kind, pattern}, allowed: [pattern],
    note}`.
  - `fix:…` needs `text`, the fix; it finds the suggestion in the window
    (`minutes`, default 60) and runs `chloectx remember fact "When this
    error comes back: “<error>”, the fix is: <text>"` in the repository.
    → `{ok, id, applied: {kind, error, text}, chloectx: "<its output>"}`;
    409 when ChloeCtx fails.
  - Anything else is 400 (it says how; nothing to apply).
  - Applied suggestions are kept in marks (`flow:<repo>:<id>`) and come
    back with `applied: true`.

#### Deviations (Map, Preview, Flow)

- `marks`' column `key` is a reserved word of ChloeDB's SQL, so it is
  quoted: `"key" KEYWORD PRIMARY KEY` (and `"key"` in every statement).
  `StateDb::mark`/`set_mark` and `Supervisor::mark`/`set_mark` wrap it.
- Map adds `repo`, `missing`, and `kind`/`files`/`collapsed` on tree
  nodes; `heading_into` adds `changed_by` and `read_by`; agent files add
  `status`.
- A preview URL may carry a path (a dev server under a base path).
- `POST /api/preview/route` adds `matched`, `score`, `message` and
  `missing`; `related` records are `{id, kind, text, ts, files}`.
- The overlay also takes `wb-pick-mode`, posts `wb-ready`, and `wb-who`
  takes `texts`, `selector` and `max`.
- Flow adds `repo`, `window.minutes`, per-agent `tokens`, `missing`, and
  on each suggestion `kind`, `saves_ms`, `applied` (and `from` for the
  report's); `apply` takes `repo` (and `text` for a fix).
- A failure's error comes from the transcript when the hooks do not say
  (Claude Code runs PostToolUse after successful calls only).

### Away

- `POST /api/seen` sets `last_seen` (the UI calls it when the page is
  visible and when it is hidden).
- `GET /api/digest?since=<ms>` (default `last_seen`) →
  `{since_ms, until_ms, tiles: {done, decided, waiting, learned},
  story: [{ts, agent: Agent|null, text, chips: [{kind, text}]}],
  questions: [Question], rules: [Rule], replay: [{agent, from_ms, to_ms,
  label}]}`: outcomes done, questions answered (by you, a rule or the
  terminal), questions still open, ChloeCtx records learned, and a story
  line per agent from its narrations and chapters; `replay` is the moments
  of decisions (each answered question and each decision record with a
  rewind), for the Rewind player to play one after another.
- The UI opens on Away instead of Mission when `last_seen` is more than 30
  minutes old and something happened since.

### UI

- `ui/app.js` keeps the shell (top bar, stream, token, terminal grid and
  zoom, decision trail). The new screens are ES modules in
  `ui/levels/`: `mission.js`, `story.js`, `decide.js`, `map.js`,
  `preview.js`, `flow.js`, `away.js`, and `common.js` (shared bits:
  agent chip, ◆ ChloeCtx chip, time, a small chart helper). Each exports
  `mount(root, ctx)` → `{update(state), unmount()}`, where `ctx` has
  `api(path, opts)`, `state` (agents, events from the stream), `go(hash)`,
  `openWhy(args)`, `openRewind(args)`, `zoom(agentId)`. The server
  compiles them in and serves `/levels/<name>.js` (and
  `/preview-overlay.js`), from disk under `CHLOEDB_WORKBENCH_UI_DIR`.
- A left rail: Mission, Decide (with the count of open questions), Map,
  Story, Preview, Flow, Away, and Terminals (the grid). Routes are hashes:
  `#/mission`, `#/decide`, `#/map`, `#/story/<agent>`, `#/preview/<agent>`,
  `#/flow`, `#/away`, `#/grid`, `#/zoom/<agent>`.
- Mission is the default screen once a goal exists; before that, the grid
  with a "Set a goal" box above it.
- `ui/dev/mock-server.mjs` serves fixtures for every new route (a story of
  a payments repo, as in the concepts), so each screen can be built and
  screenshotted without a model.
