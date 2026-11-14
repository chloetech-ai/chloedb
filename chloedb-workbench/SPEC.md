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
CREATE TABLE deliveries (  -- what each agent has been told (rules, steers, its outcome)
  id KEYWORD PRIMARY KEY,  -- agent + ':' + item id (a rule's, a steer's, or 'assign:' + outcome)
  agent KEYWORD, item KEYWORD,
  via KEYWORD,             -- the hook event that carried it, or 'typed'
  ts TIMESTAMP
);
CREATE TABLE seen (        -- the levels' own key/value state: last_seen
  id KEYWORD PRIMARY KEY,  -- `key` is a reserved word of ChloeDB's SQL
  value TEXT, ts TIMESTAMP
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
  to `POST /api/hook` (as its `output`) when it is a `UserPromptSubmit`,
  `SessionStart` or `PostToolUse`: `{"hookSpecificOutput":
  {"hookEventName": E, "additionalContext": "..."}}` holding active rules
  of the repo and steers this agent has not been told yet (recorded in
  `deliveries`), at most 9,000 characters (Claude Code moves more than
  10,000 to a file). It still never fails the hook: any error prints
  nothing.
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
  id, texts?: [string], selector?, max?: 300, watch?: bool}` is answered
  with `{type: "wb-who", id, items: [{selector, text, classes, rect}]}`,
  the visible elements with text of their own (under `selector`; with
  `texts`, those whose text holds one of them or is held by one, so the
  parent can pass an agent's added lines); `classes` are the element's
  and its parent's (at most 12), which the Preview screen matches against
  the class names in an agent's added lines. With `watch`, the same
  answer comes again, under the same id, after the page scrolls, resizes
  or changes (debounced), until the next `wb-who`. On load it posts
  `{type: "wb-ready", url}`, and the parent asks again.
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
  takes `texts`, `selector`, `max` and `watch`; its items carry `classes`.
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

### The levels' backend as built (Mission, Decide, Story, Away)

Modules: `src/levels.rs` (the routes, what follows each hook, delivery,
steer), `src/goals.rs`, `src/questions.rs`, `src/narrate.rs` (transcripts
and sentences), `src/story.rs`, `src/digest.rs`. `server.rs` merges
`levels::routes()` under its guard. Rows' times are ISO 8601 text, as
`Agent`'s; numbers in milliseconds are named `*_ms`.

`Agent` gains `goal`, `outcome`, `narration` and `narration_ts` (ISO).
`POST /api/agents` takes `goal?` and `outcome?`: an outcome's agent works
toward its goal and the outcome becomes `active` (409 when the outcome
already has a running agent, 400 when there is no such outcome); an agent
started by an agent with a goal works toward that goal too. New event
kinds: `question` (data `{question}`), `answered` (`{question, by}`) and
`steer` (`{id, text}`); the stream carries them as any event, and no new
SSE event types are added (a page refetches `/api/questions` on them).

```
Goal     {id, repo, text, state, lead, created, done,
          outcomes_total, outcomes_done, needs_you, progress}   -- not counting dropped ones
Outcome  {id, goal, title, detail, state, agent, agent_name, agent_state,
          ord, after, progress, created, updated, updated_ms}
Question {id, agent, agent_name, outcome, repo, kind, text, context,
          options: [{label, detail?, matches_precedent: bool}], recommended,
          precedent: {text, ids, agents: {record id: Agent}, missing,
                      rules: [{id, text, option}]} | null,   -- null until chloectx answered
          state, answer, answered_by, rule, created, created_ms, answered, answered_ms}
Rule     {id, repo, text, question, ctx_id, active, created, created_ms}
Record   {id, kind: "decision"|"preference"|"fact"|"fix", text, ts, ts_ms,
          session, agent: Agent|null, files: [path]}
```

| route | body | answer |
|---|---|---|
| `POST /api/goals` | `{repo, text, plan?, model?, worktree?}` | 201 `{goal: Goal, lead: Agent\|null}` |
| `GET /api/goals?repo=` | | `{goals: [Goal]}`, newest first |
| `GET /api/goals/:id` | | `{goal, outcomes, agents, open_questions, learned: [Record], heads_up: [], pulse: [[minute_ms, count]] (60), missing}` |
| `PATCH /api/goals/:id` | `{text?, state?}` | `{goal}` |
| `GET /api/outcomes?goal=` | | `{outcomes: [Outcome]}`, in order |
| `POST /api/outcomes` | `{goal?, title, detail?, after?}` | 201 `{outcome}`; `goal` defaults to the calling agent's (`x-workbench-agent`) |
| `PATCH /api/outcomes/:id` | `{title?, detail?, state?, progress?, agent?}` | `{outcome}` |
| `POST /api/questions` | `{question, options?: [{label, detail?} \| "label"], recommended?, context?}`, header `x-workbench-agent` (or `agent` in the body) | 201 `{question}`, already answered when a rule answers it |
| `GET /api/questions?repo=&state=&agent=` | | `{questions: [Question]}`, newest first |
| `GET /api/questions/:id` | | `{question}` |
| `GET /api/questions/:id/wait?timeout_s=30` | | `{question, answered: bool}` once it is not open, or at the timeout |
| `POST /api/questions/:id/answer` | `{option?: index from 0, text?, rule?}` | `{question, rule: Rule\|null, missing}`; 409 when not open |
| `GET /api/rules?repo=` | | `{rules: [Rule]}`, oldest first |
| `POST /api/rules` | `{repo, text}` | 201 `{rule, missing}` |
| `PATCH /api/rules/:id` | `{active?, text?}` | `{rule}` |
| `POST /api/agents/:id/steer` | `{text}` | `{ok, steer: {id, text}, delivered: "typed"\|"queued"}`; 409 when not running |
| `GET /api/agents/:id/story` | | as above, plus `missing` |
| `POST /api/seen` | `{ts_ms?}` | `{ok, last_seen_ms, previous_ms}` |
| `GET /api/digest?since=&repo=` | | see below |
| `POST /api/hook` | as before | `{ok, output: {hookSpecificOutput: {hookEventName, additionalContext}} \| null}` |

Story: `chapters[].steps[]` also has `running` (a call not finished yet);
`knew[]` is `{id, kind, text, score, decision, turn, receipt, shown_ms,
deja_vu}`, each record once, where it was first shown; `deja_vu[]` is
`{text, record, when: ISO of the record's own time | null, session,
agent: {id, name} | null, shown_ms}` (`when` and `session` from one
`chloectx sql --json` on the déjà vus' ids); `tokens` sums the `usage` of
each assistant message once (Claude Code writes a line per content block,
each with the message's usage) over every transcript the agent's hooks
named, read from their last 64 MiB.

Digest: `{since_ms, until_ms, last_seen_ms, away_ms, tiles: {done, decided,
waiting, learned}, decided_by: {you, rule, terminal}, done: [Outcome],
story: [{ts, ts_ms, agent: Agent, text, chips: [{kind: "edits"|"done"|
"decided"|"rule"|"waiting", text, question?, by?}]}], questions: [open
ones, then those answered since], rules: [made since], learned: [Record],
replay: [{agent, from_ms, to_ms, at_ms, label}], happened: bool, missing}`.
`since` defaults to `last_seen`, else the last 24 hours; `learned` asks
`chloectx sql --json` in each repository that saw work (at most 5).

MCP tools added: `ask_human {question, options?, recommended?, context?}`
(answers "The user answered: …", or "A rule the user set (r…) answers it:
…", or after 30 minutes that there is no answer yet), `add_outcome {title,
detail?, after?}` (the caller's goal), `update_outcome {id, progress?,
state?, title?, detail?}` (answers the goal's outcomes, a line each), and
`spawn_agent` takes `outcome`.

ChloeCtx: `chloectx receipts --json` prints `{text, receipts: [{id, ts
(seconds), session, turn, source, decision, shown, rows: [{id, kind, tag,
rank, score, decision, text}] (the rows it showed, their text as it is
now), tokens, txn, query}]}`.

#### Deviations

1. `POST /api/hook` answers `{ok, output}`, and the hook prints `output`
   alone (never the `ok`), only for an answer of 200 within its 2 s.
2. `last_seen` is kept in a table of the levels' own, `seen (id, value,
   ts)`, not in `marks`; its key column is `id` because `key` is a
   reserved word of ChloeDB's SQL (`marks` will need the same).
   `deliveries` gains `via`.
3. Narration at `UserPromptSubmit` is "Starting on: <the prompt's first
   sentence>", since the transcript is written after the hook fires; at
   `Stop` it is the first sentence of `last_assistant_message` when the
   payload has one; at `PostToolUse` it is the transcript's tail, else a
   sentence from the hook's own tool call. It is made on a thread of its
   own, one at a time per agent (the newest hook wins), and a waiting
   agent keeps its question as its sentence.
4. An outcome's agent is told its outcome (and to use `update_outcome` and
   `ask_human`) as additional context at its `SessionStart`, not in its
   task. Rules and the outcome are told again after `/clear` or a
   compaction (`SessionStart` with source `clear` or `compact`).
5. Steers are events of kind `steer`. One for an idle agent is typed; any
   other is told at the next `SessionStart`, `UserPromptSubmit` or
   `PostToolUse` as "The user adds: …", and those still untold when a
   turn ends (`Stop`) are typed in.
6. An outcome's state: `done` and `dropped` stay once set; no agent →
   `next`; its agent waiting or with an open question → `needs_you`; its
   agent exited → `done` when it exited with code 0 at progress 100, else
   `needs_you`; otherwise (working, idle, starting) `active`. It is brought
   up to date when outcomes are read, and written back when it changed.
7. A permission prompt is a Notification with `notification_type`
   `permission_prompt` (or, without a type, the word "permission"); its
   question's `context` is the agent's last `PreToolUse` summary ("Bash:
   cargo test"); there is one open per agent, it records no `question`
   event (its Notification stands for it), and any other hook of the agent
   before your answer marks it `answered_by: terminal`.
8. A rule answers a question by itself (`answered_by: rule:<id>`) when an
   agent of the repository asks again, in the same words, the question the
   rule was made from, and that answer is one of the new options (or it
   has none).
9. `matches_precedent`: every word of the option's label (three letters or
   more, the commonest left out) is in an active rule of the repository or
   in one line of what `chloectx ask --json` printed (it gives the records'
   text as one block and their ids, not each record's kind).
10. `learned` of a goal is the last 24 hours, not the calendar day.
11. Déjà vu: a fix is an `error` record tagged `fix`; "right after a failed
    tool call" is within 120 s after it; a failed call is a `PreToolUse`
    with no `PostToolUse` before the turn ended (Claude Code fires none
    then), a `PostToolUseFailure`, or a response with `is_error`.
12. `heads_up` is always `[]` here; the map fills it (`map::collisions`,
    a TODO in `goals::detail`).
13. `--narrate-model` (the optional chapter titles and summaries by a
    model) is not built; titles are the prompt's first eight words and
    summaries the turn's last assistant text, first two sentences.
14. Extra routes: `PATCH /api/goals/:id`, `GET /api/outcomes`, `GET
    /api/questions/:id`, `POST /api/rules`.

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
  with a "Set a goal" box above it. Opened without a route after more than
  30 minutes away (`/api/digest`'s `away_ms`) when something happened, it
  opens Away; leaving the page (hidden or closed) marks it seen
  (`POST /api/seen`).
- `ui/dev/mock-server.mjs` serves fixtures for every new route (a story of
  a payments repo, as in the concepts), so each screen can be built and
  screenshotted without a model.
- `ui/dev/seed-real.mjs --start` runs the real server instead, with the
  tests' fake `claude` and `ui/dev/fake-chloectx.mjs` (which answers
  `ask`, `sql`, `receipts`, `remember`, `blame`, `report` and `wiring` in
  chloectx's formats, from a JSON file), and seeds it through the HTTP
  API: a repository, a goal with its outcomes, agents that write, read,
  fail and ask, questions with a precedent, a rule, steers, a collision
  and a preview. Re-runnable; the header says how.

## ChloeCtx is required

Most of what the levels show comes from ChloeCtx, so every session
Workbench starts is wired to it, without touching the user's settings:

- `chloectx wiring --project <session cwd> --exe <abs path>` (hidden;
  `chloectx/src/install.rs`, `wiring()`) prints `{version, exe, wired:
  "user"|"project"|null, settings: {hooks, permissions: {allow:
  ["mcp__chloectx"]}, env: {CHLOECTX_DISTILL_MODEL}}, mcp: {chloectx:
  {type, command, args: ["mcp"]}}, store, model: {ok, path, fix}}`.
  `settings` and `mcp` are `{}` when the folder is already wired by the
  user's settings or its project's, so a session is never recorded twice
  (a worktree has no `settings.local.json` of its own, so it gets them).
- At spawn (`supervisor.rs`), before a worktree is made, `wiring` is run
  for the repo; a failure answers 409 `chloectx_missing` with the fix.
  Then it is run for the session's cwd and merged (`src/ctxwire.rs`) into
  the session's `settings.json` (hooks appended per event, allow joined,
  env added) and `mcp.json` (server `chloectx` beside `workbench`).
- `GET /api/chloectx`: `{required, ok, bin, version, wired, model,
  problem, fix}`. The UI (`ui/levels/setup.js`) checks it at start and,
  when not ok, shows a sheet with the problem and the fix
  (`./install.sh --workbench`), "Check again", and a bar while unfixed.
- `--chloectx-bin` resolves a bare name on PATH, else `~/.local/bin/`.
- `./install.sh --workbench` builds and installs both programs, fetches
  the model and runs `chloectx wiring`; Claude Code's settings are left
  alone unless `--user-too` (or `--project`) asks.
- `--no-chloectx` (hidden): no wiring, no setup sheet. The tests pass it
  unless a test asks for ChloeCtx (`"+chloectx"` in `Server::start`).

## Fourth build: what only ChloeDB makes possible

The user's ask: use cases 1 to 11 of the list "unique to ChloeDB", each
with UI where it makes sense, in new screens or in the existing ones. Five
parts, each built by one agent that owns the files named here. Shared
files take only additive, one-place edits (a line in `server.rs`'s route
merges and `LEVELS`, a table const in `db.rs`, tool entries and match arms
in `mcp.rs`, a link in `index.html`'s `#lvnav`, `SCREENS` and `LEVEL_OF`
in `app.js`, tests appended at the end of `tests/workbench.rs` under the
part's own heading, fixtures as new files in `ui/dev/fixtures/`).

Everything degrades when a helper is missing (`chloectx`, `chloedb-crawl`,
Node, Playwright): the UI says what is missing and how to get it, the API
answers `{"available": false, "why": ...}`, and the tests do not need it.
No model is called without the user starting it.

| # | use case | part | backend | MCP | UI |
|---|---|---|---|---|---|
| 1 | code search for agents | Search | `src/search.rs`: a per-repo index of the worktree's files in the state DB (`code_files`, `code_chunks`: path, symbol, lines, text with a code analyzer), reindexed from `git ls-files` and the agents' edits (PostToolUse Edit/Write); `GET /api/search/code?repo&q&agent&tokens` answers chunks and, with `tokens`, a `PACK n TOKENS` text | `search_code` | **Library** screen (new, level Plan): Code tab; and `/` in the top bar opens a quick search over code, docs, decisions |
| 10 | docs on tap | Search | `src/library.rs`: the repo's dependencies (Cargo.toml, package.json, pyproject, go.mod) to doc sites; fetched by `chloedb-crawl` when it is on PATH, else a small built-in fetcher (same host, depth 1, 200 pages); pages as rows `doc_pages`; `POST /api/library/crawl`, `GET /api/library`, `GET /api/search/docs` | `search_docs` (answers with URLs to cite) | Library screen: Docs tab (each library, pages, crawl state, which agents cited what) |
| 2 | simulated users | Preview lab | `src/swarm.rs`: runs `chloedb-swarm`'s persona runner (Node, Playwright) against `/preview/:agent/`, a persona and a task; findings as rows `swarm_runs`, `swarm_findings`; a finding becomes a Decide question or a steer to the owning agent | `test_with_persona` | Preview: "Test with a persona" panel (persona, task, live steps, findings with Send to agent / Ask me) |
| 3 | runtime blame | Preview lab | `src/runtime.rs`: `POST /v1/logs` (OTLP/HTTP JSON) and the preview proxy's 5xx and the overlay's `window.onerror` as rows `runtime_errors`; each error is joined (ASOF JOIN on time, and the file in its stack) to the agent's last edit before it; `GET /api/runtime/errors` | `runtime_errors` | Preview: an errors strip; each error says "probably <agent>'s edit to <file> at 7:12" and opens the Why card for that line |
| 4 | fleet alerts as SQL | Health | `src/alerts.rs`: rules as rows `alert_rules` (a SQL over `events`, `agents`, `test_runs`, a threshold, a window) checked every 30 s; firings group into `incidents` (same agent, repo or file within 10 min) with a board (the firings, the agents' sentences, the events around them); built-in rules: a test re-run failing 5 times, tokens per hour above a limit, two agents editing one file, an agent idle while asked; actions: notify, ask, pause the agent | `list_incidents` | **Health** screen (new, level Plan): Incidents (board per incident), Rules (edit the SQL, test it now); a count on the nav |
| 5 | flaky test radar | Health | `src/testruns.rs`: every test run an agent makes (Bash PostToolUse of cargo test, npm test, pytest, go test, jest/vitest; parsed per test) as rows `test_runs` (test, agent, commit, passed, ms); p50/p99 with `approx_percentile`, pass rate, flakiness (passed and failed on one commit); `GET /api/tests` | `test_history` | Health screen: Tests tab (radar: flaky, slow, newly failing) and a chip on Story's steps that ran tests |
| 7 | blast radius | Ship | `src/blast.rs`: from an agent's diff (functions changed) to the tests that cover them (test files that name them, `test_runs`), the decisions about those lines (`chloectx blame`, the ChloeCtx graph), and the other agents touching the same files or callers; `GET /api/agents/:id/blast` | `blast_radius` | **Ship** screen (new, level Changes, per agent): a radial graph and lists; the Changes tab gets a "Blast radius" summary |
| 11 | capsule with every PR | Ship | `src/capsule.rs`: `chloectx pack` of the agent's decisions (its session, since its start) into `<session dir>/capsule`; a PR body section that links it and says how to ask it (`chloectx mount`); `POST /api/agents/:id/capsule`, `GET /api/agents/:id/pr` (title, body with the decisions, blast radius, tests) | `prepare_pr` | Ship screen: the PR draft and its capsule, "Ask the capsule" box |
| 8 | time travel | Time and guard | `src/timetravel.rs`: snapshots of the state DB every 10 min and at each goal change (`link_from`, kept a day), with the commit of each worktree at that time; `GET /api/time?at=ts` answers Mission's and Map's data as of then | | a time scrubber on Mission and Map ("what did the fleet know at 7:12") |
| 9 | guarded actions | Time and guard | `src/guarded.rs`: `change_requests` (a migration, a deploy, a push to main, any command named in the repo's guarded list); an agent's key can only propose; the user approves in Decide and Workbench runs it in the agent's worktree, keeping its output; PreToolUse denies the guarded commands with a pointer to `propose_change` | `propose_change` | Decide: "Change requests" section (command, why, diff, risk, Approve and run / Decline) |
| 6 | model routing that learns | Time and guard | `src/routing.rs`: per finished task, features (words of the task, files, repo, model, tokens, minutes, redo: reopened or steered, outcome done) as rows `task_features`; a suggestion for a new task from the nearest past tasks (BM25 over their task text) weighted by outcome and cost; `GET /api/route?repo&task` | (spawn_agent's `model` defaults to the suggestion when left out) | the New agent dialog: "Suggested: Haiku, 9 of 11 like this finished, $0.40 avg" with the alternatives |

New screens join `SCREENS` and the nav: Library (`ui/levels/library.js`),
Health (`ui/levels/health.js`), Ship (`ui/levels/ship.js`). Additions to
existing screens are separate modules that the screen imports and mounts
in one place: `ui/levels/preview-lab.js`, `ui/levels/timebar.js`,
`ui/levels/change-requests.js`, `ui/levels/route-hint.js`.

Each part first builds its UI on fixtures in the levels mock
(`ui/dev/levels-mock.mjs`) and screenshots it dark and light at 1440x900
into `/mnt/project-files/workbench/fourth/` (`<n>-<name>-{dark,light}.png`),
then the backend, then the UI against the real API.

### Health as built (rows 4 and 5)

**Capture.** `testruns::after_hook` runs after each hook is recorded
(`server.rs`'s `hook`, one line): a Bash `PostToolUse` or
`PostToolUseFailure` whose command `flow::is_test` takes for a test run is
parsed on a thread; an Edit, Write, MultiEdit or NotebookEdit becomes a row
of `file_edits`; each hook's `transcript_path` is remembered for the
tokens. Claude Code fires no `PostToolUse` for a failed Bash call, and
Workbench's sessions do not register `PostToolUseFailure`, so at `Stop`
the turn's test calls that had none are read from the transcript's
`tool_result` (with `is_error`) instead. Parsers: cargo test and nextest
(`test x ... ok|FAILED|ignored`, `<0.012s>` from `--report-time`, the
`---- x stdout ----` sections as the error), pytest (`-v` lines, `PASSED
[ 50%]`, `--durations`, the short summary, the `___ name ___` sections),
jest and vitest (✓ ✕ ↓ lines under their file and describe blocks, `●`
sections), go test (`--- PASS: TestX (0.01s)` as `pkg › TestX`). A command
none parses is one row named by the command, passed when it exited 0. A
test's `ms` is its own when the output gives it, else the run's (PreToolUse
to Post), and `own_ms` says which.

**Tables** (in `db.rs`): `test_runs` (one row per test per run: `run`,
`repo`, `agent`, `"commit"` (quoted: `commit` is a reserved word), `tree`
(a hash of `git diff HEAD` and the untracked files, `clean` without them),
`runner`, `command`, `test`, `outcome` passed|failed|skipped, `passed`,
`ms`, `own_ms`, `run_ms`, `exit_code`, `error`, `use_id`, `ts`, `seq` (the
PreToolUse's event, which Story's steps carry)); `file_edits` (`repo`,
`agent`, `file` relative to the agent's checkout, so worktrees compare,
`tool`, `ts`, `seq`); `token_use` (one row per assistant message of a
transcript, keyed by its id: `input` (with cache writes), `output`,
`cache_read`, `ts`; read incrementally before each evaluation, the last
16 MiB the first time); `alert_rules`; `alert_firings` (`rule`, `entity`,
`agent`, `file`, `value`, `summary`, `state` firing|silenced|resolved,
`started`, `last_seen`, `ended`, `misses`, `agents`, `incident`);
`incidents` (`title`, `severity`, `state` open|resolved, `agents`, `files`,
`rules`, `actions` as JSON, `needs_you`).

**Rules.** A rule is one SELECT (or WITH … SELECT, no `;`) whose rows name
`entity` and `v`, and may name `agent`, `file`, `repo`, `detail`; `:since`
(now minus the window), `:now` and `:repo` are bound when the SQL names
them. A row whose `v` passes `cmp` (`> >= < <= = !=`) and `threshold` is a
firing keyed by rule, entity and agent; one not seen in 2 runs in a row is
resolved. The rules run every 30 s, and on `POST /api/alerts/check`.
Built-ins (switched off, not deleted): `test-refail` (a test failing 5
times in an hour for one agent; ask), `tokens-hour` (input and output
tokens over 400k in an hour; notify), `same-file` (2 agents editing a path
in 30 min; notify), `idle-asked` (an agent idle or waiting while its
question has been open 10 min; notify). A firing joins an open incident of
its repository that still fires or changed in the last 10 min and shares
an agent or a file with it (one that names neither joins one of the same
rule), else opens one; an incident resolves when none of its firings
fires. Actions run once per incident and rule: **notify** sets
`needs_you`, and app.js lists those incidents in Needs you after the
agents (a click opens the board); **ask** writes a question of kind
`alert` for the agent (Pause it / Tell it to stop and report / Let it carry
on), and its answer is carried out (Esc, a steer, nothing; other words are
steered as they are); **pause** sends Esc to the agent's terminal. You
resolving an incident silences its firings until they stop, so it does not
open again at once, and expires its open questions.

**Endpoints.** `GET /api/incidents?state=open|resolved|all&repo=`
(`incidents`, `counts`, `agents`, `eval_every_s`, `last_eval_ms`);
`GET /api/incidents/:id` (`incident`, `board`: `firings`, `sentences` (each
agent's narration), `events` (the agents' events from 6 min before it to
its end, an alert's as kind `fired`), `tests` (the tests its test rules
named, with their last 20 runs), `error`, `related` (ChloeCtx's decisions,
preferences and facts naming its files, by `chloectx sql --json`, asked at
most once a minute), `window`, `missing`); `POST /api/incidents/:id/resolve`;
`POST /api/incidents/:id/act {kind: notify|ask|pause, agent?}`;
`GET|POST /api/alerts/rules`, `PATCH|DELETE /api/alerts/rules/:id` (a
built-in answers 409 to DELETE; SQL that does not run is refused with 400);
`POST /api/alerts/test {sql, cmp, threshold, window_s, repo?}` (`ok`,
`columns`, `rows` (200 at most), `count`, `ms`, `would_fire`, `params`,
`note`, `error`); `POST /api/alerts/check`. `GET /api/tests?repo=&days=7`
(`totals`, `tests` (per test: runs, pass rate, `p50_ms`, `p99_ms`, `flaky`
and why, `newly_failing`, `slow`, `last`, the last 20 runs), `commands`,
`agents`, `percentiles`, `missing`); `GET /api/tests/history?repo=&test=`;
`GET /api/tests/runs?agent=` (Story's chip). Without `repo`, the
repository most agents work in.

**Verdicts.** Flaky: passed and failed on one commit with the same tree,
or two flips within a day with no new commit. Newly failing: its last run
failed and one before it passed. Slow: p99 at least 5 s, or at least 1 s
and three times its p50.

**MCP.** `list_incidents {state?}` and `test_history {test?, days?}`
answer text, for the caller's repository.

**UI.** `ui/levels/health.js` (`#/health`, level 1, a count of open
incidents on the nav as `lvn-health`): Incidents (open and resolved; the
board: what fires with a meter against the threshold, what the agents say,
the error, the tests in it, Done about it, the events around it, what
ChloeCtx knows of its files; Pause, Ask me in Decide, Resolve), Rules (the
SQL, the tables it may read, comparison, threshold, window, severity,
action; ⌘↵ tests it now and highlights the rows it would fire on), Tests
(a radar of p50 against failure rate, the flaky, newly failing and slow
lists, every test with its runs, the recent commands). `1` `2` `3` switch
tabs. `ui/levels/test-chip.js`, mounted on Story's `run` steps: "41 passed ·
2 failed · 1 flaky" for the runs of that step, opening Health's Tests.
Fixtures: `ui/dev/fixtures/health.mjs`.

**Deviations.** p50 and p99 come from `approx_percentile(ms, 0.5)` and
`(ms, 0.99)` over `test_runs` grouped by test; when that form is not there
(an older ChloeDB) they are computed in Rust from the rows (nearest rank),
and `percentiles` says `rust`. Incidents group by a shared agent or file,
not by repository alone (that grouped every firing of a busy repository
into one). `tokens-hour` reads tokens from transcripts, so an agent
without one counts none. Firings in a repository Workbench has no agent in
carry the rule's repository, or none. Workbench's sessions do not register
`PostToolUseFailure`; adding it to `supervisor::settings_json` would let a
failed run be recorded as it ends rather than at `Stop`.

### Ship as built (use cases 7 and 11)

**Endpoints.**

- `GET /api/agents/:id/blast` (`src/blast.rs`) answers
  `{agent, repo, base, head, branch, totals, risk, files, functions, tests,
  decisions, related_files, agents, sources, missing}`:
  - `functions`: each function the diff against the agent's base touches,
    `{id: "path#name", name, kind, lang, path, line, status:
    changed|added|deleted, added, removed, callers, callers_total, tests,
    decisions, agents}`. They are found by a symbol finder per language
    (Rust, JS/TS, Python, Go, Java/Kotlin/C#, C/C++, Ruby, PHP, Swift; hand
    written, no regex crate). It runs over the new text for added lines
    and the base's text (`git show base:path`) for removed ones. Test
    files are not counted as changed functions. Callers come from
    `git grep -w` (8 per function, at most 40 functions searched).
  - `tests`: test files that name a changed function (3 points per name)
    or import its file (2), ranked, the top 20. Each is
    `{path, score, covers, why, runs}`. `runs` comes from the `test_runs`
    table (Health's) when it exists: total, passed, failed, flaky (passed
    and failed on one commit), p50_ms, last_passed. When there is no such
    table, `runs` is null, `sources.test_runs` is false and `missing`
    says so.
  - `decisions`: `{id, kind, text, ts, session, agent, files, via,
    functions, superseded, superseded_by, line, entity}`, where `via` is
    any of `blame`, `file` and `graph`:
    - `blame`: `chloectx blame <path>:<first added line> --json
      --no-replay` for up to 8 functions.
    - `file`: the repository's decision, preference and fact records that
      name a changed file (`map::file_records`).
    - `graph`: `chloectx rpc '{"op":"view","what":"graph"}'`, meaning
      notes that touch a changed file, one hop over the entities they are
      about, and `supersedes` links. The other files those notes touch are
      `related_files`.
  - `agents`: the other agents whose worktree diffs change the same files
    (`same_file`), change a file that calls a changed function
    (`calls_changed`), or are reading a changed file (`reading`), from
    `map::territory`.
  - `risk`: `{level: low|medium|high, score, line, reasons}`:
    - Points: 2 per untested function (at most 6); 3 per function deleted
      but still called; 3, 2 and 1 for the three kinds of other agent; 1
      for a flaky test and 2 for a failing one; 1 per superseded decision;
      1 for more than 10 callers.
    - Levels: low below 3, medium below 13, high from 13.
    - `line` is one sentence, for example "Medium risk: 6 functions
      changed with 5 callers; rateFor has no test that names it and
      cart-ui changes src/cart/total.ts too."
- `POST /api/agents/:id/capsule` (`src/capsule.rs`) moves an old capsule
  aside, then runs `chloectx pack <session dir>/capsule --since-days
  <since the agent started, plus a minute> --title "<name>: <task's first
  sentence>" --json` in the worktree. It answers
  `{available, capsule: {dir, title, records, by_kind, bytes, sha256,
  created, since, pack_ms, scope, mount, claude}}`, or
  `{available: false, why}`. `GET /api/agents/:id/capsule` answers the
  stored one, if its `manifest.json` is still there.
- `POST /api/agents/:id/capsule/ask {question}` answers
  `{available, via: capsule|repo, question, text, ids, agents, ms,
  missing}`:
  - With a capsule, it runs `chloectx mount <dir>` over stdio (MCP) and
    calls its `capsule_ask` tool, with no model. The ids come from the
    answer's `(capsule …, records: …)` trailer and `[id]` marks.
  - Without one, it runs `chloectx ask --json` in the worktree, and
    `missing` says that the repository's memory answered.
- `GET /api/agents/:id/pr` answers `{agent, title, branch, base_branch,
  base, body, body_file, gh, push, sections: {summary, commits, decisions,
  blast, tests}, capsule, missing}`:
  - `title`: the outcome's title, else the task's first sentence.
  - `body` (Markdown) has these sections: Summary, Decisions behind it
    (`- ◆ \`id\` kind: text`; the agent's own session's records from
    `chloectx sql` first, then those about its lines), Blast radius,
    Tests run (the agent's Bash test commands and their outcome, from its
    events), and Ask why (the `claude mcp add … -- chloectx mount <dir>`
    command).
  - It saves the body to `<session dir>/pr.md`.
  - `gh` is `gh pr create --base … --head … --title … --body-file …`.
    Workbench never pushes or opens the pull request.

**Tables.** None new. The capsule is kept as the mark `capsule:<agent>`.

**MCP.** There are two tools:
- `blast_radius {id?}` answers the risk line and the lists, as text.
- `prepare_pr {id?, capsule?}` packs the capsule unless `capsule` is
  false, then drafts the pull request. It answers the title, the body
  file, the push and `gh` commands, and the body.

Both default to the calling agent.

**UI.**

- **Ship** (`ui/levels/ship.js`; `#/ship/<agent>`; level Changes; nav link
  Ship) has two tabs:
  - **Blast radius**:
    - A risk banner with its reasons.
    - A radial SVG graph with the change at the center and rings of
      functions, tests, decisions and agents. Hovering a node dims the
      rest, and clicking a function opens the Why card.
    - An "If this merges" card.
    - Four lists: functions, tests, decisions with ◆ chips, and other
      agents.
  - **Pull request**:
    - An editable title and the body, as a preview or as Markdown.
    - Copy body, Copy gh command and Copy title, with the push and gh
      commands.
    - The Capsule card: records, size, kinds and the mount command, with
      Pack and Pack again.
    - "Ask the capsule".
    - Tests it ran.
- **Changes tab**: `ui/levels/blast-summary.js` is mounted once above the
  diff (`showBlastSummary` in app.js). It shows one row: the risk level,
  the counts of functions, tests and untested functions, a ◆ decisions
  chip, the other agents, and Ship →. Below it is the risk line. It reads
  only once it is visible, and keeps an answer for 20 s.
- **Fixtures**: `ui/dev/fixtures/ship.mjs`, with the same shapes as the
  real API.

**Deviations.**

- `chloectx pack` has no session filter. The capsule is the repository's
  memory since the agent started (`--since-days`), and its `scope` says
  so. The PR's decisions are filtered to the agent's sessions.
- `chloectx ask` takes no capsule path, so "Ask the capsule" drives
  `chloectx mount` instead.
- Changed functions come from a finder over both sides of the diff
  rather than git's hunk headers, which name the enclosing function of
  the context and miss new ones.
- `test_runs` is read only if Health created it.
