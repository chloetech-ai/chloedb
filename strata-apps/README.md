# strata-apps: apps that are built from a description and grow

A prototype of the idea in the doc
[Strata as a self-extending app engine](https://claude.ai/artifact/K7nVdw9VKT4d9u9QGDb35e):
an app is data in Strata, made of capabilities (tables, views, server
functions) that a model writes and the engine checks, stores and runs, and of
shared components and helpers that every app uses from one library, linked to
each other in a graph that Strata walks.

- **Start from a description.** "A reading list: I paste a link with a title
  and a note, tag it, and mark it read." The forge asks the model for the
  first version: tables, the views (screens) and any server functions, with
  their code. The engine checks the code, creates the tables, stores every
  capability as a row in Strata and puts the app live.
- **Ask for changes.** "Let me star a link", "add a Stats tab with a weekly
  chart". Each change is a new version of the app: the changed capabilities
  are new rows, and a version is a row listing the capabilities it is made
  of, so going back is one click. Tables only grow, so data survives every
  change and every step back.
- **It suggests, from what you do.** Each build proposes three next steps,
  and the engine records what you do (views opened, rows added and changed,
  actions the views report). After 25 actions, or when you ask, it reads that
  use and suggests changes that it points to.
- **It shares.** Pieces of interface that are not about one app's data (a
  month calendar, a chart, a day strip, a checklist) are written as shared
  components: one copy in Strata that views call as `ui.calendar(props)`.
  The forge is shown every component with its props and uses them before
  writing anything, so a second app asking for a calendar gets the first
  app's. A component takes its data through props and never touches tables,
  so it fits any app. Each app pins the version it uses; when one app needs
  more from a component, the forge writes a new version (new props only,
  old ones keep their meaning) and the other apps are offered it, which
  takes a fraction of a second and no model call. Plain logic shared the same
  way is a **helper**, called as `lib.dates.addDays(day, 1)` in views,
  components and server functions alike.
- **It starts with a library.** 50 pieces written by hand ship in `library/`
  and are stored in Strata at start-up like any other. Components: a
  calendar, a day strip, an activity heatmap, bar, line and donut charts, a
  sparkline, stat tiles, a progress ring, a data table, a kanban board, a
  timeline, a checklist, a form, a tag input, filter chips, rating stars, a
  timer, sign-in, an account menu, avatars, a people picker, an uploader,
  images, a gallery, a full-screen image viewer, a file list, an email
  composer, a map, a location picker, Markdown (shown and edited), comments,
  reactions, a drawing and signature pad, booking slots and a dialog.
  Helpers: dates, streaks, repeating schedules, statistics, money, CSV, text,
  geo, validation, colour, Markdown, durations, units and access rules. A
  changed file is stored as a new version at the next start.
- **Every app gets services.** People sign in (one account for every app on
  the engine; the first person in an app is its owner), upload files, send
  email and find places, through `app.me()`, `app.signIn()`, `app.upload()`,
  `app.email()`, `app.geocode()` and the rest, in views and in server
  functions (where `app.user` is the person whose action led to the call).
  Each app also has a page of its own at `/a/<id>`, without the studio
  around it, for a phone or to hand to someone.
- **It knows what goes with what.** Every app, view, function, table and
  shared piece is a node of `sa_graph`, and what each one uses, reads, calls
  or opens is its `EDGES` field. The forge ranks the library for a request by
  BM25 over what pieces do fused with personalized PageRank from the pieces
  the app and the best text matches use (pieces other views use together,
  pieces that call each other). A new version of a piece is offered to the
  apps found by walking the graph backwards from it (`REACHABLE ... dir =>
  'in'`), through the pieces that use it. The Library's Graph view and each
  app's Map draw it and ask Strata about any node.
- **It reuses whole screens too.** The forge searches the library of what
  every app runs now (BM25 over what each capability is and its code) and
  can reuse a view or function as it is, or fork it.
- **It repairs itself.** A view or function that fails at run time is sent
  back to the forge with its error, once per version, and the fixed version
  replaces it while the app stays open.
- **Everything is live.** The engine follows Strata's feed of committed
  writes, so every open window sees new rows and new versions as they land,
  whoever wrote them (another window, a trigger, another client of the
  database).

## Run it

You need Node.js 20 or later, Claude Code signed in (`claude` on the PATH:
the engine calls `claude -p`, so it uses your Claude Code sign-in and needs no
API key of its own), and strata-server, which `run.sh` builds when it finds
none (Rust needed for that).

```bash
cd strata-apps
./run.sh                     # http://127.0.0.1:7730/
```

`run.sh` starts strata-server on `127.0.0.1:7720` with its database in
`strata-apps/data/apps.strata`, and the engine on `127.0.0.1:7730`. Options:

| | |
|---|---|
| `--port N`, `--host H` | where the studio listens |
| `--data DIR` (or `STRATA_APPS_DATA`) | where the engine keeps its database |
| `--strata URL` (or `STRATA_URL`, with `STRATA_TOKEN`) | use a strata-server that is already running (started with `--seq-no` for live updates) |
| `STRATA_SERVER_BIN` | the strata-server to start |
| `STRATA_APPS_MODEL` | the model (default `claude-opus-5-5`) |
| `STRATA_APPS_CLAUDE` | the Claude Code binary (default `claude`) |
| `STRATA_APPS_EFFORT` | the model's effort level (default: the model's own) |
| `STRATA_APPS_IDLE_S`, `STRATA_APPS_TIMEOUT_S` | a build stops when the model writes nothing for this many seconds (default 180), or after this many in all (default 1200) |
| `STRATA_APPS_AUTOREPAIR=0` | do not send failing capabilities back to the forge |
| `SMTP_URL`, `SMTP_FROM` | a mail server (`smtps://user:pass@host:465`, or `smtp://` with STARTTLS when offered) and the From address; without them email waits in the outbox, marked held |
| `STRATA_APPS_GEOCODER` | the address search (default OpenStreetMap's Nominatim, at most one request a second, cached; `off` turns it off) |
| `STRATA_APPS_TILES` | the map tile URL template (default OpenStreetMap's, fetched by the engine with a user agent and kept under `DATA/tiles`; `off` turns maps off) |

A first version of a small app takes the model 45 seconds to 1.5 minutes; a bigger one, such as a course with lessons, a playground and a quiz, takes up to 8 minutes (most of it thinking, and the rest writing 50,000 characters of code). The studio shows how far the model has got while it writes. A change takes 25 to 50 seconds.

## What is where

| | |
|---|---|
| `engine/server.mjs` | HTTP API, live updates (server-sent events), the request queue, repairs, suggestions from use |
| `engine/forge.mjs` | the forge: the prompt, the context it reads from Strata (the app's code, tables, sample rows, what you did, library matches), the checks, and applying a plan as the next version |
| `engine/engine.mjs` | the runtime: SQL, inserts, updates, removes and function calls on an app's data; triggers; events; following Strata's feed |
| `engine/store.mjs` | the system tables in Strata and the app's tables |
| `engine/graph.mjs` | what each capability links to, read from its code, and the rows of `sa_graph` |
| `engine/library.mjs`, `library/` | the built-in components and helpers, one file each, seeded into Strata at start-up |
| `engine/sql.mjs` | rewrites `FROM tasks` to the app's physical table and refuses anything but one `SELECT` over the app's own tables |
| `engine/services.mjs` | people and sessions (scrypt passwords, session cookie), files (bytes under `DATA/files`, at most 15 MB, served with a sandbox policy and `nosniff`), the email outbox and a small SMTP client, address search |
| `engine/sandbox.mjs` | server functions run in QuickJS compiled to WebAssembly: a fresh interpreter per call, 64 MiB, 10 s, no network, files or Node APIs, `app.*` calls only |
| `engine/llm.mjs` | the model, through `claude -p` with tools, settings and MCP switched off |
| `web/` | the studio, and the runtime (`frame.js`, `kit.css`) each view runs with in a sandboxed frame |

In Strata:

| table | holds |
|---|---|
| `sa_apps` | each app and its live version (`head`) |
| `sa_capabilities` | every capability ever written, one row per version, with its code: `id` is `app/kind/slug@version`, and `lib/component/name@version` or `lib/helper/name@version` for a shared piece |
| `sa_compositions` | each version of each app: the capability ids it is made of (the versions of shared pieces it pins included), the request and a summary |
| `sa_graph` | one row per app, per live table, view and function, and per shared piece (by name, `live` naming its version); `links` (an `EDGES` field) holds labelled edges: `has`, `uses`, `data`, `on`, `calls`, `opens`, `forked` |
| `sa_requests` | requests to the forge, with their progress log, cost and errors |
| `sa_events` | what people did in each app |
| `sa_suggestions` | suggestions, from builds and from use |
| `sa_people`, `sa_members`, `sa_sessions` | accounts (one per email, for every app), who belongs to which app and with what role, and sessions (by the hash of their token) |
| `sa_files` | each uploaded file's name, type, size and who uploaded it; the bytes sit in `DATA/files/<app>/<id>` |
| `sa_outbox` | every email an app sent, with its status: held, sending, sent or failed |
| `app_<app>_<table>` | each app's data |

## How views and functions run

A **view** is JavaScript that defines `async function render(root, app)`. The
studio runs it in an `<iframe sandbox="allow-scripts ...">` with no same
origin and a content security policy that blocks the network, so a view can
reach data only through `app`, whose calls go to the studio as messages and
from there to the engine: `app.sql` (one `SELECT`, over the app's own tables),
`app.insert`, `app.update`, `app.remove`, `app.call`, `app.onChange`,
`app.open`, `app.track`, and the services: `app.me`, `app.signIn`,
`app.signUp`, `app.signOut`, `app.people`, `app.upload`, `app.files`,
`app.email`, `app.geocode`. Files and map tiles load from the engine by URL
(`app.fileUrl(id)`, `app.tiles`), the one origin the frame's policy allows
images from.

A **component** is JavaScript that defines `function component(props)` and
returns an element. Every frame gets the components its app pins, each in its
own script and loaded on first call; an error inside one is reported (and
repaired) against the component. Components may call other components and
helpers, and add a stylesheet once with `css(name, text)`.

A **helper** is JavaScript that defines `function helper()` returning an
object of plain functions, called as `lib.name.fn(...)`. It has no page and no
data calls, and is synchronous, so the same code runs in a view's frame and in
a server function's QuickJS.

A **function** is JavaScript that defines `function run(input, app)`. It runs
on the server in QuickJS (WebAssembly), with the same `app` calls answered by
the engine synchronously, and may be a trigger (`on: {table, event}`) that
runs after each insert, update or delete of a table.

The forge's checks: the plan's shape, names and types; types of existing
columns never change; every view parses and defines `render`, uses no
`fetch`, `import` or `localStorage`; every component defines `component`
and uses no `app.sql`, `app.insert` or other data call (services are
allowed); every helper uses no `app` at all; every `ui.name` a
view or component calls exists; every function compiles in QuickJS,
defines `run` and uses no `async`; triggers name tables that exist. An answer
that fails gets one more try with the problems listed.

## What it is not (yet)

A prototype on one machine: people can sign in to apps, but the studio
itself has no sign-in (anyone who can reach it can change any app), app data
is not yet limited by row rules on the server (`lib.access` checks in views
and functions only), there is no limit on what a request costs, and a capability that
passes the checks can still be wrong (the repair loop and going back are the
safety net). The doc lists what a real version needs.

## Tests

```bash
npm test     # the SQL rewrite, the answer parser, the plan checks, the sandbox, pinning, the built-in library, the graph's edges,
             # functions seeing the signed-in person and the services, passwords, and SMTP against a fake mail server
```
