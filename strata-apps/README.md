# strata-apps: apps that are built from a description and grow

A prototype of the idea in the doc
[Strata as a self-extending app engine](https://claude.ai/artifact/K7nVdw9VKT4d9u9QGDb35e):
an app is data in Strata, made of capabilities (tables, views, server
functions) that a model writes and the engine checks, stores and runs, and of
shared components and helpers that every app uses from one library, linked to
each other in a graph that Strata walks.

- **Start from a description.** "A reading list: I paste a link with a title
  and a note, tag it, and mark it read." The forge asks the model to plan the
  first version: tables, the views (screens), any server functions and new
  shared pieces, each with a spec. Then it has the model write every piece at
  once, one call each, so a big app takes the time of its biggest piece. The
  engine checks the code (a piece that fails is written again alone), creates
  the tables, stores every capability as a row in Strata and puts the app
  live.
- **Ask for changes.** "Let me star a link", "add a Stats tab with a weekly
  chart". Each change is a new version of the app: the changed capabilities
  are new rows, and a version is a row listing the capabilities it is made
  of, so going back is one click. Tables only grow, so data survives every
  change and every step back.
- **It evolves, from what people do.** Each build proposes three next steps,
  and the engine records what people do (views opened, searches, filters,
  rows added and changed, functions run), who did it and on which version. It
  counts patterns in that record (a search typed again and again, a filter set
  every visit, the same field changed row after row, two views always opened
  together, a function run by hand) and, for a new one, builds a change that
  answers it beside the live version. The owner tries it on the real data,
  then keeps or drops it (see "Evolving from use" below).
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
- **It starts with a library.** 150 pieces ship in `library/` and are stored
  in Strata at start-up like any other. The first 50 were written by hand;
  the next 100 were written by the model from a spec each and kept only after
  passing the checks a written piece must pass, their example calls (every
  helper's run as tests, in Node and in QuickJS, from `library/examples/`)
  and a render in Chromium without errors. A changed file is stored as a new
  version at the next start.
  - 86 components. Inputs: a form, tag input, filter chips, rating stars,
    date, time and date-range pickers, a colour picker, combobox, multi
    select, slider, number stepper, toggle switch, segmented control,
    password field, inline edit, editable and sortable lists, a Markdown
    editor, an uploader, a drawing pad. Layout and navigation: tabs,
    accordion, dialog, dropdown menu, stepper, pagination, search box, empty
    state, confirm button, tree view. Records: a data table, list view,
    grouped list, record card, key-value list, tag list, kanban, checklist,
    timeline, notifications, comments, chat, reactions. Time: a calendar,
    day strip, agenda, week view, Gantt chart, countdown, timer, booking
    slots, opening hours. Charts: bar, stacked bar, line, donut, histogram,
    funnel, heatmap, sparkline, gauge, progress bar and ring, stat tiles,
    leaderboard. Learning and voting: quiz question, flashcard, poll. Shops:
    product card, cart, line items, reviews. People and media: sign-in, an
    account menu, avatars, a people picker, images, a gallery, an image
    viewer, a file list, a map, a location picker, code and Markdown blocks,
    an email composer.
  - 44 helpers: dates, times of day, deadlines and business days, repeating
    schedules, scheduling free slots, streaks, statistics, forecasts,
    numbers, money, tax and tips, invoices, loans, budgets, settling shared
    expenses, inventory, grades, quizzes, spaced repetition, ranking and Elo,
    votes, nutrition, fitness, units, durations, geo, phones, addresses,
    URLs, files, CSV, text, templates, Markdown, diffs, lists, objects,
    trees, paging, IDs, random (seeded), colour, validation, access rules.
  - 20 starters an app copies as they are: screens (notes, tasks, contacts,
    inventory, expenses, events, journal, timesheet, orders, support tickets,
    recipes, workouts, goals, announcements, bookings, settings), each with
    the tables it names in its header, which a reuse adds to the app; and
    server functions for any table (export as CSV, a daily digest email,
    find duplicates, remove old rows).
- **Every app gets services.** People sign in (one account for every app on
  the engine; the first person in an app is its owner), upload files, send
  email and find places, through `app.me()`, `app.signIn()`, `app.upload()`,
  `app.email()`, `app.geocode()` and the rest, in views and in server
  functions (where `app.user` is the person whose action led to the call).
  Each app also has a page of its own at `/a/<id>`, without the studio
  around it, for a phone or to hand to someone.
- **It knows what goes with what.** Every app, view, function, table and
  shared piece is a node of `sa_graph`, and what each one uses, reads, calls
  or opens is its `EDGES` field. The forge ranks the library for a request
  by the words and meaning of what pieces do, fused with personalized
  PageRank from the pieces the app and the best matches use (pieces other views use together,
  pieces that call each other). A new version of a piece is offered to the
  apps found by walking the graph backwards from it (`REACHABLE ... dir =>
  'in'`), through the pieces that use it. The Library's Graph view and each
  app's Map draw it and ask Strata about any node.
- **It answers questions about how pieces fit.** The Library's search box
  (and `GET /api/graph/ask?q=`) takes questions such as "which screens use
  the calendar", "which apps use lib.money", "which rules write votes",
  "what runs when a vote is added" or "what does the cart screen use", and
  answers them from `sa_graph` with no model call (`engine/ask.mjs`): the
  piece is found by its name ("the progress bar", "ui.progressBar", "the
  votes table in the bakery"), else by words and meaning; the question is a
  walk of the labelled edges (`REACHABLE(links, :pieces, dir => 'in', label
  => 'uses,calls')`); writes are told from reads by the code; and a piece
  found through others says through which (an app, through its screen
  menu, through ui.cart, uses lib.money). Screens record the patterns they
  were expanded from, and patterns are nodes too, so "which screens use the
  leaderboard pattern" has an answer.
- **It reuses whole screens too.** The forge searches the library of what
  every app runs now and can reuse a screen, rule or function as it is, or
  fork it. Search is two rankings fused (reciprocal rank fusion): BM25 over
  what each piece is and its code, and the cosine of the request's
  embedding and the piece's (`sa_capabilities.meaning`, its title and
  description embedded by Strata as each version is written, with the
  model of the apps' meaning columns). So "let customers vote on which
  coffee we roast next" asked of a coffee shop finds the voting board's
  screen and its two rules, and "purchase the items in my basket" finds a
  checkout with none of its words. A piece found by meaning alone must be
  close (`STRATA_APPS_MEANING_MIN`, cosine 0.2). The plan prompt gets the
  best 3 screens, rules and functions (each saying whether its words, its
  meaning or both found it), and of the patterns apps added the best 3 in
  full and the others by name. The studio's Library search is the same
  search.
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
| `STRATA_APPS_LIBRARY` | the built-in library's folder (default `library/`) |
| `STRATA_APPS_SPLIT` | `first` (default): first versions are planned, then written piece by piece at once; `always`: changes too; `off`: one answer holds the plan and all the code |
| `STRATA_APPS_IDLE_S`, `STRATA_APPS_TIMEOUT_S` | a build stops when the model writes nothing for this many seconds (default 180), or after this many in all (default 1200) |
| `STRATA_APPS_AUTOREPAIR=0` | do not send failing capabilities back to the forge |
| `STRATA_APPS_SCREEN_MODEL` | the model for the plan step and for changes, which write the screens and rules (default `claude-sonnet-5-5`; set it to `claude-opus-5-5` for Opus everywhere); code views, functions and repairs keep `STRATA_APPS_MODEL` |
| `STRATA_APPS_RULES=off`, `STRATA_APPS_TESTS=off` | functions and triggers only as JavaScript; no tests in plans |
| `STRATA_APPS_RULE_SQL=off` | rules written as JSON steps rather than CREATE RULE statements |
| `STRATA_APPS_STATEMENTS=off` | screens written as JSON rather than CREATE SCREEN statements (still stored as the statement that makes them) |
| `STRATA_APPS_PARALLEL=off` | a first version's plan holds its screens' and rules' statements, rather than each being written by its own writer at once with the example rows and tests |
| `STRATA_APPS_SMALL_MODEL` | the model a small change (a label, a colour, a sort order, renaming, hiding or moving something) goes to first, before the usual one if its answer fails the checks (default `claude-haiku-5-5`; `off` sends every change to the usual model) |
| `STRATA_APPS_SUGGEST=inline` | the build's own answer suggests what to ask next, rather than `STRATA_APPS_SUGGEST_MODEL` (default `claude-haiku-5-5`) once the version is live |
| `STRATA_APPS_DUMP` | a folder that keeps every answer of the model, with its timing, tokens and cost, to study what it writes |
| `STRATA_APPS_OPEN_VIEWS=off` | do not open each view in a headless browser after a build |
| `SMTP_URL`, `SMTP_FROM` | a mail server (`smtps://user:pass@host:465`, or `smtp://` with STARTTLS when offered) and the From address; without them email waits in the outbox, marked held |
| `STRATA_APPS_GEOCODER` | the address search (default OpenStreetMap's Nominatim, at most one request a second, cached; `off` turns it off) |
| `STRATA_APPS_TILES` | the map tile URL template (default OpenStreetMap's, fetched by the engine with a user agent and kept under `DATA/tiles`; `off` turns maps off) |
| `STRATA_APPS_UNSPLASH_KEY` | an Unsplash access key (free at unsplash.com/developers): front pages get photos of any subject from Unsplash, credited as its guidelines ask |
| `STRATA_APPS_PHOTOS=builtin` | photos from the built-in set only (no Openverse) |
| `STRATA_APPS_EMBED_MODEL` | the embedding model of meaning columns and of the library's search (default `potion-retrieval-32M`); a Strata that cannot load it searches the library by words alone |
| `STRATA_APPS_MEANING_MIN` | the least cosine at which a piece found by meaning alone is offered (default 0.2) |

A first version of a small app takes 50 seconds to a minute; a bigger one takes 2 to 4 minutes (a CRM with a pipeline board 110 s, a course on linked lists with lessons, a playground and a quiz 216 s; written in one answer, that course took 7.5 to 9 minutes). The studio shows how far the model has got while it writes. A change takes 20 seconds to 2 minutes.

**Screens.** Most views are written as data instead of code: a screen is JSON (its state, the SELECTs it reads, values derived from them, and a body of blocks such as forms, tables, number tiles, chips, lists, shared components by name, and small `code` blocks for what blocks cannot say), and `web/screen.js` draws it, fetches its data again after every write and redraws when its state changes. The model writes a screen inside the plan, so a first version with only screens takes one call and no writers; the forge checks each screen (its shape, that every `=expression` parses, that its SQL is a SELECT, that its forms name tables and columns the app has). Views that are mostly code (a game, a drawing surface) stay code. `STRATA_APPS_SCREENS=off` turns screens off.

**Front pages.** A screen with `"page": "front"` is a landing page drawn edge to edge, not a form: a navigation bar that turns solid on scroll, a hero (over a photo with a slow Ken Burns zoom, beside one, or on the design's drifting gradient), carousels (full-width slides that move on by themselves, pause under the pointer and take swipes and the arrow keys, or a shelf of cards that snaps), photo galleries with a lightbox, features, numbers that count up, quotes, pricing, steps, an FAQ, a marquee, photo bands with parallax and a footer (`web/front.js`, `web/front.css`). Parts rise into view as the page scrolls; `*word*` in a title is drawn in the design's gradient. Each design carries a `gradient` (three colors, derived from the accent when not given) and a `motion` (calm, lively or still; still, and any reader who asks for reduced motion, gets none). The model writes the front page as the app's first view when the app faces the public (a shop, a studio, an event).

**Photos.** A block names a photo by what it shows (`"photo": "barista pouring latte art"`), and when a version is stored `engine/photos.mjs` finds one and writes its `src` (and a credit when the license asks for one): from Unsplash with `STRATA_APPS_UNSPLASH_KEY`, else from the built-in set (`library/photos.json`: 304 Unsplash photos, each checked by eye and described in words, matched on those words with the subject first, so "dark roast coffee beans" is never a dark abstract), else from Openverse (openly licensed, credited), else the design's gradient. Example rows may hold `"photo:croissant"` in a column, which becomes a URL from the built-in set: the row's query, then its category, then its name, a photo already on the page before a photo of something else. Hand edits that name a photo get one the same way.

**Screen patterns.** `library/patterns/` holds whole parts of a screen with parameters: `crud` (a table's rows with an add and edit form, search, filter chips, a done toggle, big numbers), `board` (a kanban over a status column), `detail` (one row with its details, editing and related rows), `summary` (a total and a bar per group), `week` (a row per thing with its seven days to tick, such as habits or chores), `calendar` (a month with a day's rows and a form to add one), `flashcards` (spaced repetition: a card, its answer, Again/Hard/Good/Easy), `quiz` (multiple choice with a score, saved per try), `search` (words and meaning ranked together, the picked row and rows like it) and `dashboard` (a value over the last N days against the N before, per day, by a group, and the newest rows). A screen block `{"use": "crud", "table": "guests", ...}` is expanded by `engine/patterns.mjs` before the screen is checked, and again whenever its statement is read (see "Every piece is stored as SQL" below); a screen written as JSON holds ordinary blocks that later changes patch. Patterns are rows of `sa_capabilities` (kind `pattern`, versioned like components, found by the library's search): the files are seeded into Strata at start-up, and the forge expands from the rows. A build may add a pattern (`"patterns": [{name, title, about, params, example, screen}]` in its plan, at most one): its example is expanded and checked like a screen, the plan's own screens may use it at once, and once stored every app's builds see it (the chores app added `leaderboard`; the voting app's next change used it).

**Designs.** Every app has three designs, and its owner picks one in the studio's Design tab; a person using the app on its own can pick their own under Aa (kept in their browser). A design is a few tokens (colors for light and dark, fonts, corners, density, surfaces, navigation, width; `web/design.js`), and `web/kit.css` draws everything from them as CSS variables, so switching posts a block of variables to each view and redraws nothing. The three are made by one call to a small model (`STRATA_APPS_DESIGN_MODEL`, default `claude-haiku-5-5`, about 12 s) beside an app's first build, or on first open for older apps, checked for contrast, and kept in `sa_designs`; presets stand in when the model fails. `GET/POST /api/apps/:id/designs` read and choose; `POST .../designs/new` replaces the two not in use. Forms lay themselves out: one of more than three fields, or with a long one, is a grid of labelled fields with as many columns as make even rows (five fields: three and two), long fields across, checkboxes together and the buttons below; a short one stays on one line, every field labelled when any needs a label; every control in a form has the same height.

**CREATE SCREEN.** The model writes each screen as an SQL-shaped statement in its code block (`CREATE SCREEN guests TITLE 'Guests'`, then `LIST guests`, `SEARCH name`, `FILTER rsvp IN (...)`, `FORM (...)`, `KPI 'Coming' = COUNT(*) WHERE rsvp = 'yes'`, `SUMMARY guests AS meals`, `GROUP BY meal`...). `engine/statement.mjs` compiles it into pattern blocks; the statement is about half the characters of the pattern JSON and a quarter of the screen's. Every pattern has a part: `LIST`, `BOARD`, `DETAIL`, `SUMMARY`, `WEEK`, `CALENDAR`, `FLASHCARDS`, `QUIZ`, `FIND` (search) and `DASHBOARD`, with `BLOCKS [...]` for anything else. The statement is kept with the screen as its source: the model sees it when asked for a change and answers with `edits` to it (or a new statement), and the studio's Edit tab shows it in a text box that saves through `POST /api/apps/:id/screens/:slug {"statement"}` (compiled, checked and dry-run before it is stored; a JSON patch on such a screen is refused). The photos a build finds for a screen (`"photo": "a sushi counter"` in a block) are kept in the statement too, as a `PHOTOS [...]` part. `STRATA_APPS_STATEMENTS=off` has the model write the screen's JSON instead.

**Every piece is stored as SQL.** A row of `sa_capabilities` holds the statement that makes the piece, never its compiled form (`engine/source.mjs`): a table is its `CREATE TABLE`, a screen its `CREATE SCREEN`, a rule its `CREATE RULE`, and a piece written in JavaScript or JSON is wrapped in one (`CREATE FUNCTION send-digest AFTER INSERT ON orders LANGUAGE javascript AS $$ ... $$`, `CREATE COMPONENT`, `CREATE HELPER`, `CREATE PATTERN ... LANGUAGE json`). The store compiles a row when it reads it (a screen's patterns are expanded then, so a better pattern reaches every screen that uses it) and keeps the compiled form only in memory. A screen or rule written as JSON is stored as a statement made from that JSON, checked to compile back into exactly it, and marked `derived` in its spec (it is still shown to the model and changed as JSON). On start the engine rewrites any piece an earlier engine stored compiled as its SQL, in place, when that SQL gives back exactly what it runs. The studio shows each piece's SQL (and, for a screen or rule, what it compiles into), and the Inside tab's "Whole app as SQL" gives the app as one script to copy or download (`GET /api/apps/:id/source`).

**Rules.** A function that is a query, or does a few writes (a checkout, a stock move), or a trigger that keeps a count, a total or a stock level right, is a rule (`engine/rules.mjs`): the engine runs it directly, as one transaction, with no writer and no QuickJS. The model writes it as SQL, a CREATE RULE statement that `engine/rulesql.mjs` compiles into the rule's steps:

```sql
CREATE RULE checkout AS
  REQUIRE (SELECT COUNT(*) FROM cart WHERE owner = :user.id) > 0 ELSE 'Your cart is empty';
  INSERT INTO orders (customer, status) VALUES (:user.id, 'new') AS order;
  FOR line IN (SELECT c.id, c.product, c.qty, p.stock, p.name FROM cart c JOIN products p ON p.id = c.product WHERE c.owner = :user.id) DO
    REQUIRE :line.stock >= :line.qty ELSE 'Only :line.stock left of :line.name';
    INSERT INTO order_lines (order_id, product, qty) VALUES (:order.id, :line.product, :line.qty);
    UPDATE products SET stock = stock - :line.qty WHERE id = :line.product;
    DELETE FROM cart WHERE id = :line.id;
  END;
  RETURN :order.id;

CREATE RULE count_votes AFTER INSERT OR DELETE ON votes AS
  UPDATE features SET votes = (SELECT COUNT(*) FROM votes WHERE feature = :row.feature) WHERE id = :row.feature;
```

Statements: `REQUIRE cond ELSE 'message'` (stops the rule, writes nothing, shows the message), `LET`, `INSERT ... [ON CONFLICT (cols) DO UPDATE SET ... | DO NOTHING] [AS name]`, `UPDATE` and `DELETE` (`WHERE id = ...` for one row, any other condition for each row it selects), `CALL fn(name => value)`, `FOR row IN (SELECT ...) DO ... END`, `IF ... THEN ... [ELSE ...] END` and `RETURN`; a rule that is one SELECT is a query (`RETURNS ROW | VALUE`). The statement is kept as the rule's source: the model sees it and changes it with edits, and the studio shows it. A test's `"do"` is written the same way. `STRATA_APPS_RULE_SQL=off` has the model write the JSON steps instead (`{"query": ...}` or `{"steps": [...]}`). A query rule has the shape of a SQL function with named parameters, so it can become one when Strata has them.

**Tests as data.** A plan may carry `"tests"`: steps as in rules, then SELECTs and the value or row count each must give (`engine/checks.mjs`). They run once the version is live, on the tables it creates while they are still empty, and those tables are emptied again before the example rows go in. A failing test goes to repair like an error a person saw. After each build, every view is also opened in a headless browser (when Playwright is installed), and errors there are reported the same way.

**Small changes to code.** A change to a view or function written in JavaScript can be `"edits": [{"old": "...", "new": "..."}]` in its plan entry instead of the whole code: each `old` must occur exactly once. "Make the heading pink" is one line, not a rewrite.

**Edits by hand.** The studio's Edit tab (on a screen view) renames, moves and hides parts of the screen, and reorders, renames or hides a table's columns and a form's fields. Each edit is a JSON Patch sent to `POST /api/apps/:id/screens/:slug`, which checks the screen, runs its queries and stores it as a new version without the model (about 0.2 s).

**Evolving from use.** After every 25 actions (at most every 3 minutes), or when the owner asks, the engine counts patterns in an app's events (`engine/evolve.mjs`): searches as the words someone stopped typing at (3 or more), filters set (4), the same change, add or delete one row at a time in one sitting (5 to 6), two views opened within 30 s of each other (4), a function run by hand (5). A new pattern costs one model call: the suggestions it gets name the pattern they answer. In the "build" mode (the default, `STRATA_APPS_EVOLVE` or per app in the studio: build, suggest, off) the strongest answered one is built at once as a *candidate*: a version beside the live one (`sa_candidates`), checked like any build, that the studio's **Built from use** lists with **Try it**, **Keep** and **Drop**. Trying runs the app on that version (`?v=N` on every request, the app's own page included) on the real data while everyone else stays on the live one. Keeping makes it live: as built when nothing changed since, merged over the live version when the two changed different pieces, and built again on the live version when both changed the same piece. At most 2 candidates wait at a time and 2 are built a day per app (`STRATA_APPS_EVOLVE_PER_DAY`). Each candidate names what should change in people's use if it works (fewer searches for "vegetarian"); once kept, the studio shows that count before and since. Events carry the version they happened on, so the same measure can later compare two versions served side by side.

Each build keeps its timeline. Under Versions, **Timeline** on a version shows what took the time (reading Strata, the model planning, each piece's writer, the checks, a second try, storing), what each model call cost and read, and what was decided for every piece of the app and when: used from the library as it is, copied from a starter, forked, rewritten, written from scratch, a new version of a shared piece, or kept. The top of the tab sums up every build of the app.

## What is where

| | |
|---|---|
| `engine/server.mjs` | HTTP API, live updates (server-sent events), the request queue, repairs, suggestions from use |
| `engine/forge.mjs` | the forge: the prompt, the context it reads from Strata (the app's code, tables, sample rows, what you did, library matches), the checks, and applying a plan as the next version |
| `engine/engine.mjs` | the runtime: SQL, inserts, updates, removes and function calls on an app's data; triggers; events; following Strata's feed |
| `engine/store.mjs` | the system tables in Strata and the app's tables |
| `engine/graph.mjs` | what each capability links to, read from its code, and the rows of `sa_graph` |
| `engine/library.mjs`, `library/` | the built-in library, one file each (`components/`, `helpers/`, and the starters in `views/` and `functions/`), seeded into Strata at start-up; `library/examples/` holds each piece's example (props, calls with their results, rows) |
| `engine/sql.mjs` | rewrites `FROM tasks` to the app's physical table and refuses anything but one `SELECT` over the app's own tables |
| `engine/services.mjs` | people and sessions (scrypt passwords, session cookie), files (bytes under `DATA/files`, at most 15 MB, served with a sandbox policy and `nosniff`), the email outbox and a small SMTP client, address search |
| `engine/sandbox.mjs` | server functions run in QuickJS compiled to WebAssembly: a fresh interpreter per call, 64 MiB, 10 s, no network, files or Node APIs, `app.*` calls only |
| `engine/rules.mjs` | functions and triggers written as data: their checks and their runtime |
| `engine/ask.mjs` | questions about the graph ("which screens use the calendar"), answered from `sa_graph` |
| `engine/rulesql.mjs` | CREATE RULE: rules written as SQL, compiled into those steps |
| `engine/evolve.mjs` | evolving from use: patterns in events, candidates built beside the live version, keep (merge) and drop, and each candidate's measure |
| `engine/designs.mjs`, `web/design.js` | each app's three designs: one small model call, design tokens, CSS variables |
| `web/front.js`, `web/front.css` | front pages: their blocks, motion and photos |
| `engine/photos.mjs`, `library/photos.json` | finding the photos a screen or an example row names |
| `engine/checks.mjs` | a build's tests, run on its new tables, and opening each view in a headless browser |
| `engine/llm.mjs` | the model, through `claude -p` with tools, settings and MCP switched off |
| `web/` | the studio, and the runtime (`frame.js`, `kit.css`) each view runs with in a sandboxed frame |

In Strata:

| table | holds |
|---|---|
| `sa_apps` | each app and its live version (`head`) |
| `sa_capabilities` | every capability ever written, one row per version, with its code as SQL (the statement that makes it): `id` is `app/kind/slug@version`, and `lib/component/name@version` or `lib/helper/name@version` for a shared piece |
| `sa_compositions` | each version of each app: the capability ids it is made of (the versions of shared pieces it pins included), the request and a summary |
| `sa_graph` | one row per app, per live table, view and function, and per shared piece (by name, `live` naming its version); `links` (an `EDGES` field) holds labelled edges: `has`, `uses`, `data`, `on`, `calls`, `opens`, `forked` |
| `sa_requests` | requests to the forge, with their progress log, cost and errors |
| `sa_traces` | each build's timeline, by request: its steps and model calls with their times, tokens and cost, and the decision for each piece (`engine/trace.mjs`) |
| `sa_events` | what people did in each app: who (a person, or a visitor's cookie) and on which version |
| `sa_suggestions` | suggestions, from builds and from use (with the pattern they answer) |
| `sa_candidates` | changes built from use beside the live version: the pattern, the version built, its measure, and whether it was kept |
| `sa_designs`, `sa_settings` | each app's three designs and the one in use; per-app settings (the evolve mode) |
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
npm test     # the SQL rewrite, the answer parser, the plan checks, the sandbox, pinning, the built-in library (and every helper's example calls), the graph's edges,
             # functions seeing the signed-in person and the services, passwords, and SMTP against a fake mail server
```
