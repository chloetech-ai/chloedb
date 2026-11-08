// The forge: turns a request ("a reading list where I save links and tag
// them", "add a due date to tasks") into capabilities, with their code, and
// stores them in Strata as the app's next version.

import vm from 'node:vm';
import { ask, parseAnswer, MODEL } from './llm.mjs';
import { checkFunction } from './sandbox.mjs';
import { TYPES, NAME_RE, SLUG_RE, COMPONENT_RE, LIB, SHARED_KINDS, physical } from './store.mjs';
import { SHARED, sharedRefs, appNode } from './graph.mjs';
import { Trace, planDecisions } from './trace.mjs';
import { rewriteSql } from './sql.mjs';
import { expandPlan, expandScreen, patternsDoc } from './patterns.mjs';
import { compileStatements, compileStatement, STATEMENT_DOC } from './statement.mjs';

const BASE_SYSTEM = `You build and change small web apps for one person, through a runtime whose backbone is a database, Strata. An app is made of capabilities that the runtime stores in Strata and loads:

- table: the data the app keeps.
- view: a screen of the app, shown as a tab. JavaScript that runs in the browser, in a sandboxed frame.
- function: JavaScript that runs on the server in a sandboxed interpreter: work over many rows, logic several views share, and reactions to writes (triggers).
- component: a piece of interface that every app shares through the library (a month calendar, a bar chart, a date picker, a tag input). Views call components; the library holds one copy that every app uses.
- helper: plain functions every app shares through the library (dates, streaks, repeating schedules, statistics, money, CSV). Views, components and functions call them.

The person describes what they want in their own words: the first version of a new app, or a change to the app they have. Build exactly that, completely and well, and keep everything else as it is.

# Tables

Column types:
- text: prose, searched by words (titles, notes, descriptions, bodies)
- keyword: a short label you filter, group and sort by: a status, category, person, email, url, color, a reference to another row's id
- keyword[]: a list of labels (tags)
- int, float, bool
- timestamp: a moment, ISO 8601 text in and out ("2026-11-08T10:00:00Z"). For a calendar day without a time (a due date, the day of a check-in, a birthday) use a keyword holding "YYYY-MM-DD" instead: it compares and sorts correctly, does not shift with time zones, and app.today() gives today's in the person's time zone
- meaning: what other text columns of the row mean, as a vector Strata computes when the row is written (and for rows written before the column): {"name": "meaning", "type": "meaning", "of": ["title", "body"]}. Never write it; search by it (below). Use it when people look for things by what they are about rather than their exact words: notes, recipes, questions, products, documents.

A links table holds links between rows of another table of the app, for graphs: {"name": "note_links", "links": "notes", "columns": [{"name": "kind", "type": "keyword"}]}. It has src and dst (the ids of the rows linked, from src to dst) besides the columns you give; insert a row to link two rows (app.insert('note_links', {src: a.id, dst: b.id, kind: 'cites'})), remove it to unlink.

Every table also has id (keyword, the primary key; app.insert makes one when the row has none) and created (timestamp, set when the row is inserted). Do not declare those two. Table and column names: lowercase letters, digits and _, starting with a letter, and not a word of SQL such as key, index, order, group, check or default.

Tables only grow: you may add tables and add columns, never drop or rename them or change a column's type. A column you no longer need stays; leave it alone.

Choose text only for prose people search by words, and keyword for anything you compare with =, group by or sort by exactly. Sorting by a text column works too.

# SQL (Strata's dialect: read-only SELECT through app.sql)

- Strings in single quotes. Parameters are :name, bound from the params object: app.sql('SELECT * FROM tasks WHERE status = :s', {s: 'open'}).
- Filters on int, float and timestamp columns: =, !=, <, <=, >, >=, BETWEEN, IN (...); times compare with ISO text (created >= '2026-11-01').
- Filters on keyword columns: =, !=, IN (...) and LIKE 'prefix%' only, never <, >, <=, >= or BETWEEN (Strata refuses ranges on keywords). For calendar days kept as "YYYY-MM-DD" keywords, take a month with day LIKE '2026-11-%' and a few days with day IN (...); for any other range, read the rows and compare in JavaScript.
- IS NULL and IS NOT NULL on any column. A bool column alone is a condition (WHERE done, WHERE NOT done). To take rows where a bool is false or has no value, write (done = false OR done IS NULL); IS TRUE and IS NOT TRUE are refused once combined with other conditions.
- A keyword[] column matches when any of its values does: WHERE tags = 'work'.
- Words in text: WHERE MATCH(body, 'words') (words side by side all must appear; OR, NOT, prefix*, typo~ also work), ranked with ORDER BY bm25(body, :q) DESC. LIKE '%x%' works on text and keyword columns too (fine for small tables). lower(x) works.
- By meaning: ORDER BY similarity(meaning, :q) DESC LIMIT 20, with the words as {q: {embed: 'what to look for', field: 'meaning'}}. Rows like a row: the same, embedding that row's text, and WHERE id != :id. Words and meaning together, best of both: ORDER BY rrf(bm25(body, :t), similarity(meaning, :q)) DESC (with t the words and q their embedding). similarity scores from -1 to 1; above about 0.5 is close.
- Links: WHERE REACHABLE(note_links, :id, hops => 2, dir => 'both') walks from the row :id along a links table (dir 'out' follows src to dst, 'in' the other way, 'both' either), and ORDER BY hops() puts the nearest first (hops() is each row's distance). ORDER BY ppr(note_links, :id) DESC ranks rows by how close they are through the links (personalized PageRank), and works inside rrf too. The seed can be a list of ids, ('a', 'b'), or a subquery.
- Completions as the person types: WHERE PREFIX(tag, :typed, fuzzy => 'auto') on a keyword column (also a keyword[]), which forgives a typo; LIKE 'p%' without typos.
- Counts for the results of a search (facets): SELECT source, COUNT(*) AS n FROM notes WHERE MATCH(body, :q) GROUP BY source ORDER BY n DESC.
- GROUP BY keyword, int, float or bool columns, or date_trunc('day' | 'week' | 'month', a timestamp). Never GROUP BY a text column. Aggregates: COUNT(*), COUNT(DISTINCT c), SUM, AVG, MIN, MAX. HAVING. Name aggregates with AS.
- ORDER BY any column or alias, ASC or DESC, several keys; always end a query that returns rows with LIMIT (at most 500).
- JOIN ... ON, LEFT JOIN, subqueries, WITH, UNION, CASE, coalesce, round, abs, length, strftime('%Y-%m-%d', t), EXTRACT(dow FROM t), date_trunc, now(), CAST(x AS INT).
- No INSERT, UPDATE or DELETE in app.sql: write with app.insert, app.update, app.remove.
- Counts come back as numbers; timestamps as ISO text; bools as true or false; keyword[] as arrays.

# Views

A view's code defines

    async function render(root, app) { ... }

called once when the view opens, with root an empty <div>. Build the DOM with the helper h(tag, props, ...children) (props: attributes, class, style as text or object, on<event> handlers such as onclick, oninput, onsubmit, and value/checked set as properties; children: strings, numbers, nodes, arrays, null) or with plain DOM calls (here append, prepend and replaceChildren also take arrays and skip null, as h() does). svg(tag, props, ...children) makes SVG elements (charts: draw them yourself with svg()).

app in a view (every call returns a Promise):
- app.sql(sql, params) -> array of row objects
- app.insert(table, row) -> the stored row (with id and created)
- app.update(table, id, fields) -> the stored row (fields not given stay as they are)
- app.remove(table, id)
- app.call(functionSlug, input) -> the function's result
- app.onChange(tables, fn): fn(changedTables) runs after any write to those tables (an array of names, or '*'), made anywhere: this view, another view, a function, another window, another client of the database. Re-query there and redraw what shows the data. Do not redraw a form someone is typing in: keep forms out of the part you redraw.
- app.open(viewSlug, params): switches to another view of the app, which reads them as app.params (an object, {} when none)
- app.track(action, detail): records something meaningful the person did (app.track('filter', 'status=done'), app.track('search', q)). Opening a view, inserts, updates and removes are recorded already. What is recorded is how the app learns what to offer next.
- app.toast(text): a short message at the bottom
- app.fmt.ago(iso) ("3 h ago"), app.fmt.date(iso) ("8 Nov 2026"), app.fmt.time(iso), app.fmt.datetime(iso), app.fmt.number(n), app.fmt.money(n, currency='USD')
- app.today(): today's date as "YYYY-MM-DD"; app.now(): the time as ISO text

The frame has no network: fetch, imports, external scripts, fonts and stylesheets are blocked, and so are localStorage and cookies (keep state in a table, or in variables while the view is open). confirm() and prompt() work. Images may load from https URLs the data holds, and from files the app stores (app.fileUrl(id), below).

Styling: a stylesheet is already there, and plain elements look good without classes: h1 to h3, p, a, button, input, select, textarea, table (with thead), label, details, progress, hr, small, code. A <form> lays out its children in a row. Classes:
- layout: .row (flex row, gap, wraps), .col (flex column, gap), .grow (takes the free space), .grid (responsive grid of cards), .split (two columns that stack on phones), .right (pushes to the end of a row), .center
- surfaces: .card, .list (rows with separators; put .item on each row), .empty (empty state, centred, muted), .toolbar (a row of controls above content), .section
- text: .muted, .small, .big, .title, .strike (done items), .mono, .nowrap, .clip (one line, ellipsis)
- buttons: .primary, .danger, .ghost, .small, .icon (a square icon button); .chips with .chip children (toggles; add .on)
- badges: .badge plus .ok, .warn, .bad, .info, or style="--c: #hex" for any color
- numbers: .kpis with .kpi children, each a .value and a .label
- a board: .columns with .column children (kanban), cards inside
CSS variables: --bg, --fg, --muted, --line, --card, --accent, --ok, --warn, --bad, --radius. The page has light and dark themes: use the variables, never fixed white or black. Add a <style> element in render for anything more.

Make views complete and pleasant: a heading, real empty states that say what to do, forms that submit with Enter and clear after, disabled buttons while saving, sensible sort order, the date or time where it helps, delete with confirm(). They must work at 360 px wide. Never show raw ids. No placeholder or lorem ipsum content.

# Components (shared by every app)

Build screens from components, so that what one app builds, every app can use. A view calls a component as ui.calendar(props), which returns an element to put in the page.

- Use the shared components listed below whenever one does the job, as they are. Read their props.
- When a view needs a piece of interface that is not about this app's own data, one another app could use too (a calendar, a chart, a date or time picker, a tag input, a rating, a progress ring, a timeline, a streak counter, a kanban board, a list with search and filters, a stat tile), write it as a new component and call it from the view instead of writing it inside the view. The view keeps what is particular to the app: its queries, its forms, its words.
- When a component almost fits, write a new version of it, with the same name, that adds what you need as new props, optional, with defaults that keep what it did before. Never remove or rename a prop or change what one means: other apps use it. This app moves to the new version; other apps keep theirs until they choose to move.
- A component's code defines

    function component(props) { ...; return element; }

  It gets everything through props: the data to show, as plain values and arrays (items: [{date: '2026-11-08', label: 'Gym', color: '#3b82f6'}]), and functions to call when the person does something (onSelect(date), onChange(value)). It never reads or writes tables: no app.sql, app.insert, app.update, app.remove, app.call or app.onChange in a component. The view queries, passes the rows in, and saves what the callbacks report.
- In a component: h(), svg(), app.fmt, app.today(), app.now(), the stylesheet's classes and variables, other components as ui.<name>(props), and css(name, text), which adds a stylesheet once per name (scope its rules under a class named after the component, like .calendar).
- State the person changes and the view may want to keep across redraws (the month shown, the tab open, the sort) is a prop with a callback (month and onMonth), with a default when not given.
- One job per component, small enough to use from its props alone; build bigger ones from smaller ones. Name: camelCase, a noun for what it is (calendar, barChart, tagInput), never the app's subject (not habitCalendar). Describe its props precisely: names, types, defaults, callbacks and their arguments.

# Helpers (shared by every app)

A helper is a set of plain functions in the library, called as lib.NAME.fn(...) from views, components and server functions alike: lib.dates.addDays('2026-11-08', 3), lib.streak.current(days, {today}). Use the shared helpers listed below instead of writing the same logic again: date arithmetic, streaks, repeating schedules, sums and averages, money, CSV, text. A helper's code defines

    function helper() { ...; return { fnA, fnB }; }

returning an object of functions. Helpers get no app and no page (no app.*, document, window, ui, h): plain values in, plain values out, synchronous, so they run in the browser and in the server sandbox alike. Write a new helper (in "helpers") for logic that is not about one app's data and that another app could use; a new version may add functions and optional arguments, never change what an existing function returns.

# Services: people, files, email, places

Every app gets these from the runtime. In views and components each call returns a Promise; the ones marked (functions too) also work in functions, where they return directly.
- People. app.me() -> the signed-in person {id, name, email, role} or null. app.signIn(email, password), app.signUp({email, name, password}) and app.signOut(): every view of the app starts again afterwards, seeing the new person. app.people() -> the app's members [{id, name, email, role}] (functions too). One account works in every app here; the first person to sign in to an app is its owner, the others are members. In a function, app.user is the person whose action led to the call, or null. To record who did something, keep the person's id in a keyword column (author, assignee) and show names from app.people(). Use the shared components for this (ui.signIn, ui.userMenu, ui.avatar, ui.peoplePicker) and lib.access for who may do what. An app that only one person uses needs none of it.
- Files. app.upload(file) takes a File from <input type="file"> or a drop (a FileList or an array uploads each) and returns {id, name, type, size, url}; files are at most 15 MB. Keep the id in a keyword column, or a keyword[] for several. app.fileUrl(id) is the URL to show it (an img's src, a link); app.fileUrl(id, {download: true}) downloads it. app.files(ids) -> [{id, name, type, size, url}] (functions too). The shared ui.uploader, ui.image, ui.gallery, ui.imageViewer and ui.fileList do the rest.
- Email. app.email({to, subject, text}) puts a plain-text email in the app's outbox and returns {id, status} (functions too; from a view only when someone is signed in). At most 10 recipients, and 30 emails an hour per app. Without a mail server the outbox holds it (status "held"): tell the person it is queued, never that it was delivered.
- Places. app.geocode(address) -> up to 5 matches [{label, lat, lng}] (functions too). app.tiles is the URL template of map tiles; ui.map and ui.locationPicker use it, with lib.geo for distances.
Components may call these services (ui.signIn, ui.uploader and ui.map do), but still never the app's tables. Helpers call none of them.

# Functions

A function's code defines

    function run(input, app) { ... return result; }

a plain synchronous function: app calls return their results directly (no async, no await, no Promises). app in a function:
- app.sql(sql, params) -> rows; app.insert(table, row) -> row; app.update(table, id, fields) -> row; app.remove(table, id)
- app.call(slug, input); app.now() -> ISO time; app.log(...)
- app.user (the signed-in person or null), app.people(), app.files(ids), app.email(msg), app.geocode(address): the services above
It returns any JSON value. It runs in QuickJS with a 10 s limit, no network, no files, no require or import. Date, Math, JSON, RegExp, Array and the rest of standard JavaScript work.

A function may declare a trigger, "on": {"table": "tasks", "event": "insert"} (or "update" or "delete"): it then runs after each such write with input {row, table, event}, where row is the row as stored (the old row for delete). Writes a trigger makes run their own triggers, at most 3 deep. Use a trigger when something must happen whenever rows change (keep a counter, log history, fill a derived column); use a plain function that views call for work on demand.

Keep logic in views when it is simple and only one view needs it.

# Library

Below the app you may get capabilities that other apps built, and built-in starters (common screens and functions, written to be copied), from Strata's library. When one does what is needed as it is, reuse it instead of writing it again: name it in "reuse" (it is copied into this app as it is; make sure this app has the tables and columns its code reads, adding them in "tables" when needed). When one is close but needs changes, write your own version in a code block, and say "forked_from" with its id. Ignore the ones that do not fit.

# Your answer

First <plan>...</plan> holding one JSON object, then one <code slug="...">...</code> block for each view and function you add or rewrite, one <code slug="ui:NAME">...</code> block for each component and one <code slug="lib:NAME">...</code> block for each helper you add or write a new version of, the code alone (no markdown fences). Nothing else.

{
  "name": "a short name for the app (only for a new app, or when asked to rename it)",
  "summary": "what this version changed, in one or two plain sentences, written to the person",
  "tables": [{"name": "tasks", "description": "what a row is", "columns": [{"name": "title", "type": "text"}, {"name": "status", "type": "keyword"}]}],
  "views": [{"slug": "board", "title": "Board", "description": "what it shows and lets you do", "order": 1}],
  "functions": [{"slug": "weekly-summary", "description": "what it does, its input and output", "on": null}],
  "components": [{"name": "calendar", "title": "Month calendar", "description": "what it shows and lets the person do", "props": "month: 'YYYY-MM', default this month; onMonth(month): the person moved to another month; items: [{date: 'YYYY-MM-DD', label, color}], ...", "change": "for a new version of an existing component: what it adds"}],
  "helpers": [{"name": "recurrence", "title": "Repeating schedules", "description": "what it computes", "api": "next(rule, after) -> 'YYYY-MM-DD': ...; each function with its arguments, defaults and what it returns", "change": "for a new version: what it adds"}],
  "upgrade": [],
  "remove": [],
  "reuse": [{"id": "abc123/view/board@2", "slug": "board", "title": "Board", "order": 2}],
  "seed": {"tasks": [{"title": "...", "status": "open"}]},
  "suggestions": [{"text": "...", "why": "..."}]
}

- tables: tables to add, and existing tables you add columns to (for an existing table, list only its new columns).
- views and functions: the ones you add or rewrite, each with a code block of the same slug holding its whole code. Leave out the ones that stay as they are. Rewriting a view means writing all of it again. Keep slugs as they are (lowercase letters, digits, dashes). "order" places views (the lowest opens first).
- components and helpers: the ones you add, or write a new version of, each with a <code slug="ui:NAME"> or <code slug="lib:NAME"> block holding its whole code. The components and helpers the code calls are found from it: the app keeps the version of each it has, and takes the newest of one it did not use before.
- upgrade: names of components or helpers this app uses at an older version (see the list) that it should move to the newest.
- remove: slugs of views or functions to take out.
- reuse: capabilities from the library to copy, by id.
- seed: example rows, only for a new app, and only when examples help the person see the app working (3 to 8 rows per table, realistic, dated near today). Leave it out when the app holds the person's own data from the start (an expense log, a journal), unless they asked for examples.
- suggestions: 3 changes the person might ask for next, written as they would ask for them ("Let me snooze a task until a date"), each with a short why that refers to what they asked, or to what they did as recorded above; never claim a habit the record does not show. Make them specific to this app, small enough to build in one step, and new: not what the app does already, not what was just asked, and not one of the suggestions made before (listed above with what became of them).
- An app has one to five views. Do not split what belongs together across views.`;

// Screens: views written as data (web/screen.js draws them). On unless
// STRATA_APPS_SCREENS=off, which keeps every view as code.
export const SCREENS = process.env.STRATA_APPS_SCREENS !== 'off';

const SCREENS_DOC = `# Screens: views written as data

Write each view as a screen unless it needs something a screen cannot say. A screen is JSON, given in the view's plan entry as "screen", with no code block: the runtime fetches its data from Strata, draws its blocks with the shared components, runs its queries again after every write, and redraws when its state changes. A screen is a fraction of the code of a view and has none of its wiring to get wrong, so it is the default. Example:

{
  "state": {"filter": "all", "editing": null},
  "data": {
    "guests": "SELECT * FROM guests ORDER BY name LIMIT 500",
    "meals": {"sql": "SELECT meal, COUNT(*) AS n FROM guests WHERE rsvp = :r GROUP BY meal", "params": {"r": "yes"}}
  },
  "let": {"shown": "=guests.filter(g => state.filter === 'all' || g.rsvp === state.filter)"},
  "body": [
    {"h": "Guests"},
    {"kpis": [{"label": "Coming", "value": "=guests.filter(g => g.rsvp === 'yes').length"}, {"label": "Waiting", "value": "=guests.filter(g => g.rsvp === 'waiting').length"}]},
    {"form": "guests", "fields": [{"name": "name", "required": true}, {"name": "rsvp", "label": "RSVP", "type": "select", "options": ["yes", "no", "waiting"]}, {"name": "table_no", "label": "Table", "type": "number"}],
     "submit": "Add guest", "edit": "=state.editing", "after": "=() => set({editing: null})", "cancel": "=() => set({editing: null})"},
    {"chips": "filter", "options": ["all", "yes", "no", "waiting"]},
    {"table": "=shown", "columns": [{"key": "name"}, {"key": "rsvp", "label": "RSVP", "badge": true}, {"key": "table_no", "label": "Table"}],
     "actions": [{"label": "Edit", "do": "=g => set({editing: g})"}, {"label": "Delete", "do": "=g => app.remove('guests', g.id)", "confirm": "Delete this guest?"}],
     "empty": "No guests yet: add the first one above."},
    {"ui": "barChart", "props": {"data": "=meals.map(m => ({label: m.meal, value: m.n}))"}}
  ]
}

Values: a string that starts with "=" is a JavaScript expression; anything else is taken as it is. An expression sees each "data" entry by name (its rows), each "let" by name (computed in order, after the data), state, set(changes) (merges into state and redraws), app (as in a view: app.insert, app.update, app.remove, app.call, app.open, app.toast, app.fmt, app.today(), app.me()...), lib (the helpers), ui, h, params (app.params), today, and, inside "each", the item by its name. A handler is an expression that gives a function ("=g => app.remove('guests', g.id)"); after it finishes the screen fetches its data again. Callbacks in a component's props are handlers too.

data: name -> a SELECT (as for app.sql), or {"sql": "...", "params": {"name": value}} (values may be expressions, such as "=state.month": the data is fetched again when the state changes), or "=expression" for anything else (a Promise is awaited: "=app.call('totals', {month: state.month})", "=app.people()").

Blocks, in "body" and in every block that holds blocks:
- a plain string (a paragraph); {"h": "text", "level": 1 | 2 | 3}; {"text": "...", "muted": true, "small": true}
- {"ui": "name", "props": {...}}: a shared component, with its props as the library lists them
- {"row": [blocks]}, {"col": [blocks]}, {"grid": [blocks]} (cards that wrap), {"split": [blocks]} (two columns, stacked on phones), {"card": [blocks], "title": "..."}, {"section": [blocks], "title": "..."}
- {"if": "=expr", "then": [blocks], "else": [blocks]}
- {"each": "=list", "as": "item", "body": [blocks], "empty": "text when the list is empty", "class": "list" | "grid" | "row"}
- {"kpis": [{"label": "...", "value": "...", "hint": "..."}]}: big numbers
- {"chips": "stateKey", "options": [...]}: picks one value of state (options: strings or {"value", "label"}); {"input": "stateKey", "type": "search" | "text" | "select" | "date" | "number" | "checkbox", "placeholder": "...", "options": [...], "label": "..."}: an input bound to state
- {"button": "label", "do": "=handler", "class": "primary" | "ghost" | "danger" | "small", "confirm": "a question to confirm first"}
- {"form": "table", "fields": [{"name", "label", "type", "options", "required", "placeholder", "value"}], "submit": "label", "set": {"column": "=value"} (more columns to store, such as the project the form adds to), "edit": "=the row to edit, or null", "after": "=handler(storedRow)", "cancel": "=handler", "layout": "col"}: adds a row, or saves the row given in edit; it keeps what is typed across redraws. Field types: text (the default), textarea, number, date ("YYYY-MM-DD", for a keyword column), time, datetime (a timestamp column), select (options), checkbox (bool), tags (keyword[]), email, url, color, file (uploads it and stores its id), rating (1 to 5).
- {"table": "=rows", "columns": [{"key": "column", "label": "...", "value": "=r => ...", "badge": true, "format": "date" | "datetime" | "ago" | "money" | "number" | "percent" | "stars" | "bool"}], "actions": [{"label": "...", "do": "=r => ...", "confirm": "...", "class": "..."}], "onRow": "=r => ...", "empty": "..."}
- {"code": "async function render(root, ctx) { ... }"}: for a part the blocks cannot say, JavaScript as in a view (h, svg, app, ui, lib), drawn again whenever the screen redraws; ctx holds state, set, refresh() and the data and lets by name.

Keep expressions short: do work over many rows in SQL (GROUP BY, SUM) or in a function, and logic another app could use in a helper. Write a code view (a code block, no "screen") only when most of the screen would be "code" blocks: a game, a drawing surface, an editor of its own.`;

const SCREEN_PLAN = `
- views: a screen view carries its whole screen in the plan entry ("screen": {...}) and has no code block. A code view has no "screen" and a code block of its slug.
- To change a screen the app has, give "patch" in its plan entry instead of "screen": JSON Patch operations on the screen as the app shows it, with the paths shown there: {"op": "add" | "replace" | "remove" | "move" | "copy", "path": "/body/3", "value": ...} ("from" for move and copy; "/body/2/columns/-" adds at the end of a list; an add at /body/3 puts the block before the one there). A patch is far shorter than the screen: use it whenever most of the screen stays; give the whole "screen" when most of it changes.`;

// CREATE SCREEN statements (engine/statement.mjs): screen views written as
// SQL-shaped statements in their code blocks. On with STRATA_APPS_STATEMENTS=on.
export const STATEMENTS = SCREENS && process.env.STRATA_APPS_STATEMENTS === 'on';

const STATEMENT_PLAN = `
- views: a screen view has a code block of its slug holding its CREATE SCREEN statement, and no "screen" in its plan entry (a mostly custom screen may instead carry a JSON "screen" in its plan entry, with no code block). A code view has a code block of its slug with its JavaScript.
- To change a screen the app has, give "patch" in its plan entry instead of a statement: JSON Patch operations on the screen as the app shows it (as JSON), with the paths shown there: {"op": "add" | "replace" | "remove" | "move" | "copy", "path": "/body/3", "value": ...} ("from" for move and copy; "/body/2/columns/-" adds at the end of a list; an add at /body/3 puts the block before the one there). A patch is far shorter: use it whenever most of the screen stays; write a new CREATE SCREEN statement when most of it changes.`;

export const SYSTEM = !SCREENS ? BASE_SYSTEM : STATEMENTS
  ? BASE_SYSTEM.replace('# Components (shared by every app)', `${SCREENS_DOC}\n\n${patternsDoc()}\n\n${STATEMENT_DOC}\n\n# Components (shared by every app)`)
    .replace('- An app has one to five views.', `${STATEMENT_PLAN.trim()}\n- An app has one to five views.`)
    .replace('one <code slug="...">...</code> block for each view and function you add or rewrite', 'one <code slug="...">...</code> block for each view and function you add or rewrite (a screen view\'s block holds its CREATE SCREEN statement)')
  : BASE_SYSTEM.replace('# Components (shared by every app)', `${SCREENS_DOC}\n\n${patternsDoc()}\n\n# Components (shared by every app)`)
    .replace('"views": [{"slug": "board", "title": "Board", "description": "what it shows and lets you do", "order": 1}],', '"views": [{"slug": "board", "title": "Board", "description": "what it shows and lets you do", "order": 1, "screen": {"data": {}, "body": []}}],')
    .replace('- An app has one to five views.', `${SCREEN_PLAN.trim()}\n- An app has one to five views.`)
    .replace('one <code slug="...">...</code> block for each view and function you add or rewrite', 'one <code slug="...">...</code> block for each code view and function you add or rewrite (a screen view has its screen in the plan instead)');

// A plan's screens made ready to check: statements compiled, patches applied
// to the screens the app has, patterns expanded, and each screen put in the
// code of its view (as JSON). Returns problems.
export function prepareScreens(plan, code, view) {
  const problems = [...compileStatements(plan, code), ...patchScreens(plan, view), ...expandPlan(plan)];
  screensToCode(plan, code);
  return problems;
}

// Columns of a table, as the app has it and as the plan adds to it.
function columnsFor(view, plan) {
  return (name) => {
    const t = view.tables.find((x) => x.spec.name === name);
    const p = (plan.tables || []).find((x) => x.name === name);
    if (!t && !p) return null;
    return new Set(['id', 'created', ...(t?.spec.links || p?.links ? ['src', 'dst'] : []), ...(t?.spec.columns || []).map((c) => c.name), ...(p?.columns || []).map((c) => c.name)]);
  };
}

// A screen as the model sees it in an app: one part per line after its JSON
// Pointer, so a patch can name the part it changes.
export function screenLines(code) {
  let s;
  try { s = JSON.parse(code); } catch { return code; }
  const esc = (k) => String(k).replace(/~/g, '~0').replace(/\//g, '~1');
  const out = [];
  for (const [k, v] of Object.entries(s)) {
    if (Array.isArray(v)) v.forEach((b, i) => out.push(`/${esc(k)}/${i}: ${JSON.stringify(b)}`));
    else if (v && typeof v === 'object' && Object.keys(v).length) for (const [kk, vv] of Object.entries(v)) out.push(`/${esc(k)}/${esc(kk)}: ${JSON.stringify(vv)}`);
    else out.push(`/${esc(k)}: ${JSON.stringify(v)}`);
  }
  return out.join('\n');
}

// JSON Patch (RFC 6902: add, remove, replace, move, copy) on a copy of doc.
// It is lenient where that cannot go wrong: an add creates missing objects on
// its path, and a replace of a missing key of an object sets it.
export function applyPatch(doc, ops) {
  if (!Array.isArray(ops)) throw new Error('a patch is a list of operations');
  const out = structuredClone(doc);
  const parse = (path) => {
    if (typeof path !== 'string' || !path.startsWith('/')) throw new Error(`the path ${JSON.stringify(path)} must start with /`);
    return path.slice(1).split('/').map((p) => p.replace(/~1/g, '/').replace(/~0/g, '~'));
  };
  const walk = (parts, create) => {
    let cur = out;
    for (const [i, p] of parts.slice(0, -1).entries()) {
      let next = Array.isArray(cur) ? cur[Number(p)] : cur[p];
      if ((next === undefined || next === null) && create && !Array.isArray(cur)) { next = /^\d+$|^-$/.test(parts[i + 1]) ? [] : {}; cur[p] = next; }
      if (next === undefined || next === null || typeof next !== 'object') throw new Error(`there is nothing at /${parts.slice(0, i + 1).join('/')}`);
      cur = next;
    }
    return [cur, parts[parts.length - 1]];
  };
  const index = (arr, key, extra = 0) => {
    const i = key === '-' ? arr.length : Number(key);
    if (!Number.isInteger(i) || i < 0 || i >= arr.length + extra) throw new Error(`index ${key} is outside the list (it has ${arr.length})`);
    return i;
  };
  const get = (path) => {
    const [parent, key] = walk(parse(path));
    const v = Array.isArray(parent) ? parent[index(parent, key)] : parent[key];
    if (v === undefined) throw new Error(`there is nothing at ${path}`);
    return v;
  };
  const remove = (path) => {
    const [parent, key] = walk(parse(path));
    if (Array.isArray(parent)) return parent.splice(index(parent, key), 1)[0];
    if (!(key in parent)) throw new Error(`there is nothing at ${path}`);
    const v = parent[key];
    delete parent[key];
    return v;
  };
  const add = (path, value) => {
    const [parent, key] = walk(parse(path), true);
    if (Array.isArray(parent)) parent.splice(index(parent, key, 1), 0, value);
    else parent[key] = value;
  };
  ops.forEach((op, n) => {
    try {
      if (!op || typeof op !== 'object') throw new Error('an operation is an object');
      if (op.op === 'add') add(op.path, op.value);
      else if (op.op === 'remove') remove(op.path);
      else if (op.op === 'replace') {
        const [parent, key] = walk(parse(op.path));
        if (Array.isArray(parent)) parent[index(parent, key)] = op.value;
        else parent[key] = op.value;
      } else if (op.op === 'move') add(op.path, remove(op.from));
      else if (op.op === 'copy') add(op.path, structuredClone(get(op.from)));
      else throw new Error(`op ${JSON.stringify(op.op)}: use add, remove, replace, move or copy`);
    } catch (e) { throw new Error(`patch[${n}] (${op?.op} ${op?.path}): ${e.message}`); }
  });
  return out;
}

// A plan's patches, applied to the screens the app has: each becomes the
// entry's whole screen. Returns problems.
export function patchScreens(plan, view) {
  const problems = [];
  for (const v of plan?.views || []) {
    if (!v || v.patch === undefined) continue;
    const cur = view.views.find((x) => x.slug === v.slug);
    if (!cur || !cur.spec?.screen) problems.push(`view ${v.slug}: "patch" changes a screen the app has; give a whole "screen" (or a code block)`);
    else if (v.screen) problems.push(`view ${v.slug}: give "patch" or "screen", not both`);
    else {
      try {
        v.screen = applyPatch(JSON.parse(cur.code), v.patch);
        v.patched = Array.isArray(v.patch) ? v.patch.length : 0;
        if (v.title === undefined) v.title = cur.title;
        if (v.order === undefined) v.order = cur.spec.order;
        if (v.description === undefined) v.description = cur.description;
      } catch (e) { problems.push(`view ${v.slug}: ${e.message}`); }
    }
    delete v.patch;
  }
  return problems;
}

// Watches an answer as it streams in and reports each screen the moment its
// plan entry is complete (a patch is applied to the screen the app has), so
// the studio can draw it while the model writes the rest.
export function screenWatcher(view, onScreen) {
  const sent = new Set();
  let last = 0;
  const handle = (json) => {
    let v;
    try { v = JSON.parse(json); } catch { return; }
    if (!v || typeof v.slug !== 'string' || sent.has(v.slug)) return;
    const cur = view.views.find((x) => x.slug === v.slug && x.spec?.screen);
    let screen = v.screen && typeof v.screen === 'object' ? v.screen : null;
    if (!screen && Array.isArray(v.patch) && cur) { try { screen = applyPatch(JSON.parse(cur.code), v.patch); } catch { return; } }
    if (!screen || !Array.isArray(screen.body)) return;
    sent.add(v.slug);
    onScreen({ slug: v.slug, title: v.title || cur?.title || v.slug, order: v.order ?? cur?.spec?.order ?? 50, screen });
  };
  return (seen, text) => {
    const now = Date.now();
    if (!text || now - last < 600) return;
    last = now;
    // A CREATE SCREEN statement, once its code block is complete.
    for (const m of text.matchAll(/<code slug="([a-z][a-z0-9-]*)">\s*(CREATE\s+SCREEN\b[\s\S]*?)<\/code>/gi)) {
      if (sent.has(m[1])) continue;
      try {
        const c = compileStatement(m[2]);
        const v = (text.match(new RegExp(`\\{[^{}]*"slug"\\s*:\\s*"${m[1]}"[^{}]*\\}`)) || [])[0];
        let entry = {};
        try { entry = v ? JSON.parse(v) : {}; } catch { /* the plan entry is not complete JSON on its own */ }
        sent.add(m[1]);
        onScreen({ slug: m[1], title: entry.title || c.title || m[1], order: entry.order ?? 50, screen: c.screen });
      } catch { /* not complete or not right yet */ }
    }
    const key = text.indexOf('"views"');
    const open = key < 0 ? -1 : text.indexOf('[', key);
    if (open < 0) return;
    let depth = 0;
    let inStr = false;
    let esc = false;
    let start = -1;
    for (let i = open + 1; i < text.length; i++) {
      const c = text[i];
      if (inStr) { if (esc) esc = false; else if (c === '\\') esc = true; else if (c === '"') inStr = false; continue; }
      if (c === '"') inStr = true;
      else if (c === '{') { if (!depth) start = i; depth++; } else if (c === '}') { depth--; if (!depth && start >= 0) { handle(text.slice(start, i + 1)); start = -1; } } else if (c === ']' && !depth) break;
    }
  };
}

const tee = (...fns) => (seen, text) => { for (const f of fns) if (f) f(seen, text); };
const previewer = (view, progress) => screenWatcher(view, (p) => progress(`Drew the screen ${p.title} while the rest is written`, { preview: { ...p, screen: expandScreen(p.screen).screen } }));

// A plan's screens become the code of their views (JSON text), so the rest
// of the forge handles them as it handles code.
export function screensToCode(plan, code) {
  for (const v of plan?.views || []) {
    if (!v) continue;
    if (!(v.screen && typeof v.screen === 'object') && typeof code[v.slug] === 'string' && code[v.slug].trim().startsWith('{')) {
      try { v.screen = JSON.parse(code[v.slug]); } catch { /* checked as code */ }
    }
    if (v.screen && typeof v.screen === 'object') code[v.slug] = JSON.stringify(v.screen, null, 1);
    else delete v.screen;
  }
  return code;
}

// Runs each screen's queries against the app's tables as they will be
// (names: the tables after this version), the way the screen runs them when
// it opens: expression parameters are worked out from its first state, or
// null. Returns the queries that fail.
export async function dryRunScreens(db, appId, names, plan) {
  const problems = [];
  const today = new Date().toISOString().slice(0, 10);
  const nothing = new Proxy(() => null, { get: () => nothing, apply: () => null });
  const guess = (x, state) => {
    if (typeof x !== 'string' || !x.startsWith('=')) return x;
    try { return new Function('s', `with (s) { return (${x.slice(1)}\n); }`)({ state, today, params: {}, app: { today: () => today, now: () => new Date().toISOString(), me: () => null }, lib: nothing }) ?? null; } catch { return null; }
  };
  for (const v of plan.views || []) {
    if (!v || !v.screen) continue;
    const state = v.screen.state || {};
    for (const [name, q] of Object.entries(v.screen.data || {})) {
      const sql = typeof q === 'string' ? q : q && q.sql;
      if (typeof sql !== 'string' || sql.startsWith('=')) continue;
      const params = {};
      for (const [k, x] of Object.entries((q && typeof q === 'object' && q.params) || {})) params[k] = guess(x, state);
      // A parameter worked out from what only the running screen has (its
      // data, a helper) cannot be known here: such a query is not run, so
      // that a right query is never sent back as wrong.
      if (Object.entries((q && typeof q === 'object' && q.params) || {}).some(([k, x]) => typeof x === 'string' && x.startsWith('=') && params[k] == null)) continue;
      try {
        const { sql: phys } = rewriteSql(sql, names, (t) => physical(appId, t));
        await db.query(phys, params);
      } catch (e) { problems.push(`view ${v.slug}: data.${name} (${sql.replace(/\s+/g, ' ').slice(0, 200)}) fails: ${String(e.message).replace(/^strata \/v1\/query: /, '').slice(0, 300)}`); }
    }
  }
  return problems;
}

const SCREEN_BLOCKS = ['h', 'text', 'ui', 'row', 'col', 'grid', 'split', 'card', 'section', 'if', 'each', 'kpis', 'chips', 'input', 'button', 'form', 'table', 'code'];
const NESTED = { row: 'row', col: 'col', grid: 'grid', split: 'split', card: 'card', section: 'section', then: 'then', else: 'else', body: 'body' };

// Checks a screen: its shape, that every expression parses, that its SQL is a
// SELECT, and that its forms write tables and columns the app has.
export function checkScreen(screen, columnsOf = () => null) {
  const problems = [];
  if (!screen || typeof screen !== 'object' || Array.isArray(screen)) return ['the screen must be a JSON object with "body"'];
  if (!Array.isArray(screen.body)) problems.push('the screen has no "body" list');
  const expr = (v, where) => {
    if (typeof v !== 'string' || !v.startsWith('=')) return;
    try { new Function('s', `with (s) { return (${v.slice(1)}\n); }`); } catch (e) { problems.push(`${where}: the expression ${JSON.stringify(v.slice(0, 120))} does not parse (${e.message})`); }
  };
  const walkValues = (v, where) => {
    if (typeof v === 'string') expr(v, where);
    else if (Array.isArray(v)) v.forEach((x, i) => walkValues(x, `${where}[${i}]`));
    else if (v && typeof v === 'object') for (const [k, x] of Object.entries(v)) if (k !== 'code') walkValues(x, `${where}.${k}`);
  };
  for (const [name, q] of Object.entries(screen.data || {})) {
    const sql = typeof q === 'string' ? q : q && q.sql;
    if (typeof sql !== 'string') { problems.push(`data.${name}: give a SELECT, {"sql", "params"} or an "=expression"`); continue; }
    if (sql.startsWith('=')) expr(sql, `data.${name}`);
    else if (!/^\s*(select|with)\b/i.test(sql)) problems.push(`data.${name}: only SELECT (or WITH) queries; write with app.insert, app.update and app.remove`);
  }
  walkValues(screen.state || {}, 'state');
  walkValues(screen.let || {}, 'let');
  const walk = (list, path) => {
    if (list == null) return;
    if (!Array.isArray(list)) list = [list];
    list.forEach((b, i) => {
      const where = `${path}[${i}]`;
      if (b == null || typeof b === 'string') { if (typeof b === 'string') expr(b, where); return; }
      if (typeof b !== 'object' || Array.isArray(b)) { problems.push(`${where}: a block is an object or a string`); return; }
      const kind = SCREEN_BLOCKS.find((k) => b[k] !== undefined);
      if (!kind) { problems.push(`${where}: unknown block ${JSON.stringify(Object.keys(b))}; blocks are ${SCREEN_BLOCKS.join(', ')}`); return; }
      if (kind === 'code') {
        if (typeof b.code !== 'string' || !/\bfunction\s+render\s*\(/.test(b.code)) problems.push(`${where}: a code block defines async function render(root, ctx)`);
        else { try { new vm.Script(`(async function () {\n${b.code}\n})`); } catch (e) { problems.push(`${where}: code: ${e.message}`); } }
        return;
      }
      if (kind === 'form') {
        const cols = columnsOf(b.form);
        if (!cols) problems.push(`${where}: the form writes table ${JSON.stringify(b.form)}, which the app does not have`);
        else for (const f of b.fields || []) { const n = typeof f === 'string' ? f : f && f.name; if (n && !cols.has(n)) problems.push(`${where}: the form's field ${n} is not a column of ${b.form} (add it in "tables")`); }
      }
      for (const [k, v] of Object.entries(b)) {
        if (NESTED[k] && k !== kind && !['then', 'else', 'body'].includes(k)) continue;
        if (['row', 'col', 'grid', 'split', 'card', 'section', 'then', 'else', 'body'].includes(k) && Array.isArray(v)) walk(v, `${where}.${k}`);
        else walkValues(v, `${where}.${k}`);
      }
    });
  };
  walk(screen.body, 'body');
  if (JSON.stringify(screen).length > 120 * 1024) problems.push('the screen is longer than 120 KiB');
  return problems;
}

// Words of SQL that Strata's parser takes as keywords, so tables and columns
// cannot be named after them.
const RESERVED = new Set(('key index unique primary foreign check constraint references default select from where group order by limit offset ' +
  'join left right inner outer cross on as and or not null true false in is like between case when then else end union intersect except with ' +
  'having distinct all any exists table create alter drop insert update delete values set into match asc desc cast interval').split(' '));

const clip = (s, n) => (s.length > n ? `${s.slice(0, n)}\n... (${s.length - n} more characters)` : s);

function describeTable(t, count, sample) {
  const cols = ['id keyword', 'created timestamp', ...(t.spec.columns || []).map((c) => `${c.name} ${c.type}${c.type === 'meaning' ? ` of ${[].concat(c.of || []).join(', ')}` : ''}`)];
  const lines = [`- ${t.spec.name} (${count} rows)${t.spec.links ? `, links between rows of ${t.spec.links} (src -> dst)` : ''}: ${cols.join(', ')}${t.spec.description ? `\n  ${t.spec.description}` : ''}`];
  if (sample.length) lines.push(`  latest rows: ${clip(JSON.stringify(sample), 1500)}`);
  return lines.join('\n');
}

export function summarizeEvents(events) {
  if (!events.length) return 'Nothing yet.';
  const by = new Map();
  for (const e of events) {
    const key = e.kind === 'open' ? `opened the ${e.detail} view`
      : e.kind === 'insert' ? `added a row to ${e.detail}`
        : e.kind === 'update' ? `changed ${e.detail}`
          : e.kind === 'delete' ? `deleted a row of ${e.detail}`
            : e.kind === 'call' ? `ran the function ${e.detail}`
              : e.kind === 'error' ? null
                : `${e.kind}${e.detail ? ` ${e.detail}` : ''}`;
    if (!key) continue;
    by.set(key, (by.get(key) || 0) + 1);
  }
  const lines = [...by.entries()].sort((a, b) => b[1] - a[1]).slice(0, 25).map(([k, n]) => `- ${k}: ${n}x`);
  const first = events[events.length - 1]?.ts;
  return `${events.length} events since ${first}:\n${lines.join('\n')}`;
}

// How many of the library's pieces a prompt names as the best fits, and how
// many of those it gives with their props (the system prompt lists every
// piece by name and title).
const FULL_ENTRIES = 30;
const PROPS_ENTRIES = 12;

export async function buildContext(engine, appId, request) {
  const { store } = engine;
  const view = await store.resolve(appId);
  const shared = await store.shared();
  const parts = [];
  if (!view.comp.n) {
    parts.push('# The app\n\nThe app is new: nothing exists yet. Build its first version from the request.');
  } else {
    parts.push(`# The app now: "${view.app.name}", version ${view.comp.n}`);
    if (view.app.description) parts.push(`First described as: ${view.app.description}`);
    const tables = [];
    for (const t of view.tables) {
      const name = physical(appId, t.spec.name);
      const count = await store.countRows(name);
      let sample = [];
      try { sample = await engine.db.query(`SELECT * FROM ${name} ORDER BY created DESC LIMIT 3`); } catch { /* none */ }
      tables.push(describeTable(t, count, sample));
    }
    parts.push(`## Tables\n${tables.join('\n') || '(none)'}`);
    const views = view.views.slice().sort((a, b) => (a.spec.order ?? 99) - (b.spec.order ?? 99))
      .map((v) => `<view slug="${v.slug}" title="${v.title}" order="${v.spec.order ?? ''}" id="${v.id}"${v.spec.screen ? ' format="screen"' : ''}>\n${v.spec.screen ? screenLines(v.code) : v.code}\n</view>`);
    parts.push(`## Views\n${views.some((v) => v.includes('format="screen"')) ? 'A view with format="screen" is a screen, shown one part per line after its path. To change it, give "patch" (operations on those paths) or its whole "screen" in its plan entry; to turn it into a code view, give a code block and no "screen".\n\n' : ''}${views.join('\n\n') || '(none)'}`);
    const fns = view.functions.map((f) => `<function slug="${f.slug}" on='${JSON.stringify(f.spec.on || null)}' id="${f.id}">\n${f.description ? `// ${f.description}\n` : ''}${f.code}\n</function>`);
    parts.push(`## Functions\n${fns.join('\n\n') || '(none)'}`);
    // Code only for the pieces this app wrote: the others (built-in, or from
    // another app) are used by their props, and their code is long.
    const mine = view.shared.map((c) => {
      const newest = shared.get(`${c.kind}/${c.slug}`);
      const note = newest && newest.version > Number(c.version) ? ` newest="${newest.version}" (the newest adds: ${newest.spec?.change || 'see the library'})` : '';
      const own = c.spec?.by === appId;
      return `<${c.kind} name="${c.slug}" version="${c.version}"${note}${own ? '' : ` by="${c.author === 'builtin' ? 'built in' : 'another app'}"`}>\n// ${c.title}: ${c.description}\n// ${c.kind === 'helper' ? 'api' : 'props'}: ${c.spec?.props || ''}${own ? `\n${c.code}` : ''}\n</${c.kind}>`;
    });
    parts.push(`## Components and helpers this app uses (shared with other apps)\nThe code of the ones this app did not write is left out: use them by their props and api. To change one, write a new version of it from its props and description.\n\n${mine.join('\n\n') || '(none)'}`);
    const live = new Set(view.comp.caps);
    const events = (await store.recentEvents(appId, 300)).filter((e) => e.kind !== 'error' || live.has(e.cap));
    parts.push(`## What the person did lately\n${summarizeEvents(events)}`);
    const errors = events.filter((e) => e.kind === 'error').slice(0, 5);
    if (errors.length) parts.push(`## Errors lately, from the code that runs now\n${errors.map((e) => `- ${e.cap}: ${e.detail}`).join('\n')}`);
    const history = await store.suggestionHistory(appId, 30);
    if (history.length) parts.push(`## Suggestions made before\n${history.map((h) => `- [${h.status === 'taken' ? 'taken' : h.status === 'dismissed' ? 'dismissed' : h.status === 'open' ? 'still open' : 'replaced'}] ${h.text}`).join('\n')}`);
  }
  // Shared components and helpers: all of them by name and props, and, for a
  // change to an app that exists, the code of the few that fit best (to use a
  // piece the model needs its props; it needs the code only to write a new
  // version of it, which a first version almost never does, and every
  // character here is read on every build). Fit is two rankings fused:
  // BM25 of the request over what pieces are and do, and PageRank over the
  // graph from this app and the best text matches, which finds the pieces
  // that go with them (used by the same views, called by them).
  const own = new Set(view.shared.map((c) => `${c.kind}/${c.slug}`));
  let libIndex = -1;
  const libEntries = new Map();
  const others = [...shared.entries()].filter(([k]) => !own.has(k)).map(([, c]) => c);
  if (others.length) {
    const users = await store.sharedUsers();
    const key = (c) => `${c.kind}/${c.slug}`;
    const hits = await store.searchCapabilities(`${request.text} ${view.app.description || ''}`, { kinds: SHARED_KINDS, limit: FULL_ENTRIES });
    const near = await store.related([appNode(appId), ...hits.slice(0, 3).map((h) => `${LIB}/${key(h)}`)], { limit: FULL_ENTRIES });
    const fused = new Map();
    const vote = (k, i) => fused.set(k, (fused.get(k) || 0) + 1 / (60 + i));
    hits.forEach((h, i) => vote(key(h), i));
    near.forEach((r, i) => vote(key(r), i));
    const top = [...fused.entries()].sort((a, b) => b[1] - a[1]).slice(0, FULL_ENTRIES);
    const rank = new Map(top.map(([k], i) => [k, i]));
    others.sort((a, b) => (rank.get(key(a)) ?? 99) - (rank.get(key(b)) ?? 99) || (a.kind === b.kind ? 0 : a.kind === 'helper' ? 1 : -1));
    libIndex = parts.length;
    const best = [];
    const more = [];
    for (const [i, c] of others.entries()) {
      const used = (users.get(key(c)) || []).map((u) => `${u.name} (v${u.version})`).join(', ') || 'no app yet';
      libEntries.set(key(c), `<${c.kind} name="${c.slug}" version="${c.version}" title="${c.title}" used_by="${used}">\n// ${c.description}\n// ${c.kind === 'helper' ? 'api' : 'props'}: ${c.spec?.props || ''}\n</${c.kind}>`);
      if (i < PROPS_ENTRIES) best.push(libEntries.get(key(c)));
      else if (i < FULL_ENTRIES) more.push(`${c.kind === 'helper' ? 'lib' : 'ui'}.${c.slug}`);
    }
    // Every piece is named in the system prompt (the same for every call, so
    // it is read from the prompt cache); here, the best fits with their props.
    parts.push(`# Shared components (ui.NAME(props)) and helpers (lib.NAME.fn(...)) that fit this request best\n\nThe best first: by the request's words, and by what goes with the pieces this app and the request's best matches use (other views use them together, or they call each other). The rest of the library is named in "The library" above; to use one of those, name it in "uses" (its writer gets its props).\n\n${best.join('\n\n')}${more.length ? `\n\nAlso fitting: ${more.join(', ')}.` : ''}`);
  } else if (!own.size) {
    parts.push('# Shared components and helpers in the library\n\nNone yet: the ones you write now start it.');
  }
  // The library: whole views and functions other apps built that may fit this request.
  const lib = await store.searchCapabilities(`${request.text} ${view.app.description || ''}`, { excludeApp: appId, kinds: ['view', 'function'], limit: 3 });
  const libCaps = await store.getCapabilities(lib.map((c) => c.id));
  let capIndex = -1;
  const capEntries = new Map();
  if (libCaps.length) {
    const entries = [];
    for (const c of libCaps) {
      const starter = c.app === LIB;
      const src = starter ? null : await store.resolve(c.app).catch(() => null);
      const specs = starter ? (c.spec?.tables || []) : src ? src.tables.map((t) => t.spec) : [];
      const tables = specs.map((t) => `${t.name}(${(t.columns || []).map((x) => `${x.name} ${x.type}`).join(', ')})`).join('; ');
      const about = starter
        ? (c.kind === 'view' ? `// a built-in starter screen; reusing it adds its tables to the app when missing: ${tables || 'none'}` : `// a built-in starter function, for any table; input: ${c.spec?.input || ''}`)
        : `// its app's tables: ${tables}`;
      const head = `<capability id="${c.id}" kind="${c.kind}" title="${c.title}" app="${starter ? 'built-in starter' : src?.app?.name || c.app}">\n// ${c.description}\n${about}`;
      // A starter is reused as it is, so its header says enough to choose it;
      // its code goes to the writer of a version forked from it.
      entries.push(starter ? `${head}\n</capability>` : `${head}\n${clip(c.code, 5000)}\n</capability>`);
      capEntries.set(c.id, `${head}\n${clip(c.code, 5000)}\n</capability>`);
    }
    capIndex = parts.length;
    parts.push(`# Library: capabilities other apps have that may fit\n\n${entries.join('\n\n')}`);
  }
  const source = { user: 'the person asked for this', suggestion: 'the person accepted this suggestion', repair: 'the runtime reported this error; fix its cause', library: 'the person asked to bring this in from the library' }[request.source] || 'the person asked for this';
  parts.push(`# The request (${source})\n\n${request.text}`);
  // The same context for one piece's writer: of the library's shared pieces,
  // only the ones the plan says it calls.
  // Of the library's whole views and functions, only the one it is forked
  // from, if any.
  const forPiece = (uses = [], forkedFrom = null) => {
    const copy = parts.slice();
    if (libIndex >= 0) {
      const keys = uses.map((u) => String(u).replace(/^ui[:.]/, 'component/').replace(/^lib[:.]/, 'helper/'));
      const picked = keys.map((k) => libEntries.get(k)).filter(Boolean);
      copy[libIndex] = `# Shared components (ui.NAME(props)) and helpers (lib.NAME.fn(...)) from the library that this piece calls\n\n${picked.join('\n\n') || '(none)'}`;
    }
    if (capIndex >= 0) {
      const src = forkedFrom && capEntries.get(forkedFrom);
      copy[capIndex] = src ? `# Library: the capability this piece is forked from\n\n${src}` : '';
    }
    return copy.filter(Boolean).join('\n\n');
  };
  return { prompt: parts.join('\n\n'), forPiece, view, library: libCaps, shared, system: systemWith(shared) };
}

// The system prompt with the whole library of shared pieces, by name and
// props, in a fixed order: it changes only when a piece is added or gets a
// new version, so calls in a row read it from the prompt cache.
export function systemWith(shared) {
  const list = [...shared.values()].sort((a, b) => (a.kind === b.kind ? (a.slug < b.slug ? -1 : 1) : a.kind === 'component' ? -1 : 1));
  if (!list.length) return SYSTEM;
  const line = (c) => `${c.kind === 'helper' ? 'lib' : 'ui'}.${c.slug} (${c.title})`;
  return `${SYSTEM}\n\n# The library: every shared component and helper, by name\n\n${list.map(line).join(', ')}.`;
}

export { SHARED, sharedRefs };

// The components a piece of code calls, by name.
export function uiRefs(code) {
  return new Set([...sharedRefs(code)].filter((k) => k.startsWith('component/')).map((k) => k.slice('component/'.length)));
}

export function checkShared(kind, code) {
  const problems = [];
  if (!SHARED[kind].define.test(code)) problems.push(`must define ${SHARED[kind].defines}`);
  const data = code.match(/\bapp\s*\.\s*(sql|insert|update|remove|call|onChange|open|track)\b/);
  if (data) problems.push(kind === 'component'
    ? `uses app.${data[1]}: a component gets its data through props and reports what the person does through callbacks`
    : `uses app.${data[1]}: a helper gets plain values and returns plain values`);
  if (kind === 'helper') {
    const page = code.match(/\b(document|window|localStorage)\b|(?<![\w$.])(ui|h|svg|css)\s*[.(]/);
    if (page) problems.push(`uses ${page[1] || page[2]}: helpers also run on the server, where there is no page`);
    const svc = !data && code.match(/(?<![\w$.])app\s*\.\s*(\w+)/);
    if (svc) problems.push(`uses app.${svc[1]}: a helper gets no app; take what it needs as arguments`);
    if (/\bawait\b|\basync\b/.test(code)) problems.push('a helper is synchronous: no async or await');
  }
  if (/\bimport\s*\(|\brequire\s*\(/.test(code)) problems.push('no import or require');
  if (/\bfetch\s*\(/.test(code)) problems.push('no fetch: there is no network');
  if (kind === 'component' && /\blocalStorage\b/.test(code)) problems.push('no localStorage');
  try { new vm.Script(`(function () {\n${code}\n})`, { filename: `${kind}.js` }); } catch (e) { problems.push(`syntax: ${e.message}`); }
  if (code.length > 60 * 1024) problems.push(`the code is longer than 60 KiB: split it`);
  return problems;
}

// Pins the shared pieces an app's code calls, following pieces that call
// others: one written in this version, else the version the app had (unless
// upgraded), else the newest in the library. Keys are kind/name.
export async function pinShared(store, rootIds, { written = new Map(), had = new Map(), upgrade = new Set(), shared } = {}) {
  const lib = shared || await store.shared();
  const codeOf = async (id) => (await store.getCapabilities([id]))[0]?.code || '';
  const want = new Map();
  const queue = [];
  for (const id of rootIds) queue.push(...sharedRefs(await codeOf(id)));
  while (queue.length) {
    const key = queue.shift();
    if (want.has(key)) continue;
    const name = key.split('/')[1];
    const id = written.get(key) || (!upgrade.has(name) && !upgrade.has(key) && had.get(key)) || lib.get(key)?.id;
    if (!id) continue;
    want.set(key, id);
    queue.push(...sharedRefs(await codeOf(id)));
  }
  return want;
}

export function checkView(code) {
  const problems = [];
  if (!/\basync\s+function\s+render\s*\(|\bfunction\s+render\s*\(/.test(code)) problems.push('must define async function render(root, app)');
  if (/\bimport\s*\(|\brequire\s*\(/.test(code)) problems.push('no import or require: the frame has nothing to load');
  if (/\bfetch\s*\(/.test(code)) problems.push('no fetch: the frame has no network; use app.sql and app.call');
  if (/\blocalStorage\b/.test(code)) problems.push('no localStorage: keep state in a table');
  try { new vm.Script(`(async function () {\n${code}\n})`, { filename: 'view.js' }); } catch (e) { problems.push(`syntax: ${e.message}`); }
  if (code.length > 120 * 1024) problems.push('the code is longer than 120 KiB');
  return problems;
}

// Checks a plan against the app as it is. Returns a list of problems.
export async function validate(plan, code, view, library, shared = new Map()) {
  const problems = [];
  const tables = new Map(view.tables.map((t) => [t.spec.name, t.spec]));
  for (const t of plan.tables || []) {
    if (!NAME_RE.test(t.name || '')) { problems.push(`table name ${JSON.stringify(t.name)}: lowercase letters, digits and _`); continue; }
    if (RESERVED.has(t.name)) problems.push(`table name ${t.name} is a word of SQL; choose another`);
    const have = new Map((tables.get(t.name)?.columns || []).map((c) => [c.name, c.type]));
    for (const c of t.columns || []) {
      if (!NAME_RE.test(c.name || '')) problems.push(`column ${t.name}.${c.name}: lowercase letters, digits and _`);
      else if (c.name === 'id' || c.name === 'created') problems.push(`column ${t.name}.${c.name}: every table has id and created already; do not declare them`);
      else if (RESERVED.has(c.name)) problems.push(`column ${t.name}.${c.name}: ${c.name} is a word of SQL; choose another name (such as ${t.name.replace(/s$/, '')}_${c.name} or sort_${c.name})`);
      if (!TYPES[c.type]) problems.push(`column ${t.name}.${c.name}: type ${JSON.stringify(c.type)} is not one of ${Object.keys(TYPES).join(', ')}`);
      if (have.has(c.name) && have.get(c.name) !== c.type) problems.push(`column ${t.name}.${c.name} is ${have.get(c.name)} already; types never change (add a new column instead)`);
      if (c.type === 'meaning') {
        const of = [].concat(c.of || []);
        const textCols = new Map([...(tables.get(t.name)?.columns || []), ...(t.columns || [])].map((x) => [x.name, x.type]));
        if (!of.length) problems.push(`column ${t.name}.${c.name}: a meaning column says which text columns it means: "of": ["title", "body"]`);
        for (const o of of) if (!['text', 'keyword'].includes(textCols.get(o))) problems.push(`column ${t.name}.${c.name}: "of" names ${o}, which is not a text or keyword column of ${t.name}`);
      }
    }
    if (t.links) {
      const target = tables.has(t.links) || (plan.tables || []).some((x) => x.name === t.links);
      if (!target) problems.push(`table ${t.name}: "links" names ${t.links}, which the app does not have`);
      if (tables.has(t.name) && !tables.get(t.name).links) problems.push(`table ${t.name} exists already as a plain table; a links table must be a new table`);
    }
  }
  const slugs = new Set();
  for (const [kind, list] of [['view', plan.views], ['function', plan.functions]]) {
    for (const item of list || []) {
      if (!SLUG_RE.test(item.slug || '')) { problems.push(`${kind} slug ${JSON.stringify(item.slug)}: lowercase letters, digits and dashes`); continue; }
      if (slugs.has(item.slug)) problems.push(`${item.slug} is given twice`);
      slugs.add(item.slug);
      if (kind === 'view' && item.screen) {
        for (const x of checkScreen(item.screen, columnsFor(view, plan))) problems.push(`view ${item.slug}: ${x}`);
        continue;
      }
      const src = code[item.slug];
      if (!src || !src.trim()) { problems.push(`${kind} ${item.slug} has no <code slug="${item.slug}"> block`); continue; }
      if (kind === 'view' && /^\s*CREATE\s+SCREEN\b/i.test(src)) continue; // its problems came from compiling it
      const p = kind === 'view' ? checkView(src) : await checkFunction(src);
      for (const x of p) problems.push(`${kind} ${item.slug}: ${x}`);
      if (kind === 'function' && item.on) {
        const on = item.on;
        const known = tables.has(on.table) || (plan.tables || []).some((t) => t.name === on.table);
        if (!known) problems.push(`function ${item.slug}: its trigger names table ${on.table}, which the app does not have`);
        if (!['insert', 'update', 'delete'].includes(on.event || 'insert')) problems.push(`function ${item.slug}: trigger event is insert, update or delete`);
      }
    }
  }
  const known = new Set([...shared.keys(), ...view.shared.map((c) => `${c.kind}/${c.slug}`)]);
  const writtenCode = [];
  for (const [kind, { ns, plural }] of Object.entries(SHARED)) {
    for (const c of plan[plural] || []) {
      if (!COMPONENT_RE.test(c.name || '')) { problems.push(`${kind} name ${JSON.stringify(c.name)}: camelCase letters and digits, starting with a lowercase letter`); continue; }
      known.add(`${kind}/${c.name}`);
      const src = code[`${ns}:${c.name}`];
      if (!src || !src.trim()) { problems.push(`${kind} ${c.name} has no <code slug="${ns}:${c.name}"> block`); continue; }
      for (const x of checkShared(kind, src)) problems.push(`${kind} ${c.name}: ${x}`);
      writtenCode.push([`${kind} ${c.name}`, src, kind]);
    }
  }
  for (const v of plan.views || []) writtenCode.push([`view ${v.slug}`, code[v.slug], 'view']);
  for (const f of plan.functions || []) writtenCode.push([`function ${f.slug}`, code[f.slug], 'function']);
  for (const [what, src, kind] of writtenCode) {
    for (const key of sharedRefs(src)) {
      const [k, name] = key.split('/');
      if ((kind === 'function' || kind === 'helper') && k === 'component') { problems.push(`${what} calls ui.${name}: components are for views, ${kind}s run where there is no page`); continue; }
      if (!known.has(key)) problems.push(`${what} calls ${SHARED[k].ns}.${name}, which is not a shared ${k}; write it (in "${SHARED[k].plural}") or use one that exists`);
    }
  }
  for (const name of plan.upgrade || []) if (!view.shared.some((c) => c.slug === name)) problems.push(`upgrade ${name}: the app does not use a shared piece of that name`);
  const libIds = new Set(library.map((c) => c.id));
  for (const r of plan.reuse || []) {
    if (!libIds.has(r.id)) problems.push(`reuse ${r.id}: not in the library given`);
  }
  const existing = new Set([...view.views, ...view.functions].map((c) => c.slug));
  for (const s of plan.remove || []) if (!existing.has(s)) problems.push(`remove ${s}: the app has no view or function ${s}`);
  const viewsAfter = new Set([...view.views.map((v) => v.slug), ...(plan.views || []).map((v) => v.slug), ...(plan.reuse || []).filter((r) => library.find((c) => c.id === r.id)?.kind === 'view').map((r) => r.slug)]);
  for (const s of plan.remove || []) viewsAfter.delete(s);
  if (!viewsAfter.size) problems.push('the app would have no view');
  if (!view.comp.n && !(plan.tables || []).length) problems.push('a new app needs at least one table');
  return problems;
}

// A reused starter view brings its tables: the ones the app and the plan
// lack are added to the plan, and so are missing columns of ones they have.
export function completePlan(plan, view, library) {
  if (!plan || !Array.isArray(plan.reuse)) return plan;
  const have = new Map(view.tables.map((t) => [t.spec.name, new Set((t.spec.columns || []).map((c) => c.name))]));
  plan.tables = Array.isArray(plan.tables) ? plan.tables : [];
  for (const r of plan.reuse) {
    const src = library.find((c) => c.id === r.id);
    if (!src || src.app !== LIB) continue;
    for (const t of src.spec?.tables || []) {
      let entry = plan.tables.find((x) => x.name === t.name);
      const known = new Set([...(have.get(t.name) || []), ...((entry?.columns || []).map((c) => c.name))]);
      const missing = (t.columns || []).filter((c) => !known.has(c.name));
      if (!missing.length) continue;
      if (!entry) { entry = { name: t.name, description: `from the starter ${src.title}`, columns: [] }; plan.tables.push(entry); }
      entry.columns = [...(entry.columns || []), ...missing];
    }
  }
  return plan;
}

// Reports how far the model has got, at most every 5 s, on one line that
// each report replaces.
function writing(progress, what = 'The model') {
  const t0 = Date.now();
  let last = 0;
  return ({ chars, thinking }) => {
    const now = Date.now();
    if (now - last < 5000) return;
    last = now;
    const secs = Math.round((now - t0) / 1000);
    progress(chars ? `${what} is writing: ${chars.toLocaleString('en-US')} characters so far (${secs} s)` : `${what} is thinking (${secs} s${thinking ? `, ${thinking.toLocaleString('en-US')} characters` : ''})`, { replace: true });
  };
}

// A build in two steps, for apps with several pieces: the model plans the
// version (tables, and every view, function, component and helper with a
// spec), and then writes each piece in its own call, all at once. A big app
// is written in the time of its biggest piece instead of all of them, and a
// piece that fails the checks is written again alone.
export const SPLIT = process.env.STRATA_APPS_SPLIT || 'first';

const PLAN_STEP = `# This step: the plan alone

Answer with the <plan> alone this time, and no code blocks${STATEMENTS ? ' (but for the screens\' CREATE SCREEN statements)' : ''}. Each view, function, component and helper in it is then written by its own writer, all at once, and each writer sees this plan and the request but nothing the others write. So the plan is the contract between them:
- Give every view, function, component and helper a "spec": what it shows or computes, in enough detail to be written alone. For a view: the tables and columns it reads and writes, the functions it calls with their input and output, and the components and helpers it calls with the props or arguments it passes. For a function: its input and what it returns. For content (lessons, explanations, questions, texts), an outline of it; the writer writes it out in full.
- Give each one "uses": the shared components and helpers its code calls, as "ui:name" and "lib:name", new ones and ones from the library.
- Give components "props" and helpers "api" exactly as their code will take them, with every argument and what each function returns.
- Keep the scope to what the request asks: the fewest pieces, each the smallest that does its job well. Content gets an outline of what it covers, sized to the request (a short course is 4 to 6 lessons, a quiz 5 to 8 questions, unless the person asked for more).
- Keep "summary", "seed" and "suggestions" as always.${STATEMENTS ? '\n- A screen view has no writer: after the plan, give its CREATE SCREEN statement now, complete, in a <code slug="..."> block of its slug (the only code blocks of this answer). Only code views, functions, components and helpers are written after the plan.' : SCREENS ? '\n- A screen view has no writer: give its whole "screen" in its plan entry now, complete. Only code views, functions, components and helpers are written after the plan.' : ''}`;

const pieceStep = (what, slug) => `# This step: write ${what}

Write the code of ${what} alone, as the plan above and its spec say: what the spec lists and nothing beyond it, in compact code. Other writers write the other pieces at the same time, from the same plan: call them exactly as the plan gives them (components by their props, helpers by their api, functions by their input and output, tables by their columns). Answer with one <code slug="${slug}">...</code> block holding the whole code, and nothing else.`;

// The pieces a plan says to write, each with the slug of its code block.
function planPieces(plan) {
  const out = [];
  for (const v of plan.views || []) if (!v.screen) out.push({ what: `the view ${v.slug}`, label: `view ${v.slug}`, slug: v.slug, item: v });
  for (const f of plan.functions || []) out.push({ what: `the function ${f.slug}`, label: `function ${f.slug}`, slug: f.slug, item: f });
  for (const [kind, { ns, plural }] of Object.entries(SHARED)) {
    for (const c of plan[plural] || []) out.push({ what: `the shared ${kind} ${c.name}`, label: `${kind} ${c.name}`, slug: `${ns}:${c.name}`, item: c });
  }
  return out;
}

// Runs fn over items, at most n at a time.
async function pool(items, n, fn) {
  const out = new Array(items.length);
  let next = 0;
  await Promise.all(Array.from({ length: Math.min(n, items.length) }, async () => {
    while (next < items.length) { const i = next++; out[i] = await fn(items[i], i); }
  }));
  return out;
}

async function buildSplit({ prompt, forPiece, view, library, shared, system = SYSTEM }, progress, tr = new Trace()) {
  let cost = 0;
  let plan;
  let problems = [];
  let decidedAt = 0;
  let planPrompt = `${prompt}\n\n${PLAN_STEP}`;
  for (let attempt = 1; attempt <= 2; attempt++) {
    progress(attempt === 1 ? `Asking the model (${MODEL}) to plan the first version` : `Asking the model again to fix the plan: ${problems.slice(0, 3).join('; ')}`);
    const span = tr.start('plan', attempt === 1 ? 'The model plans the first version' : 'The model plans again, to fix the plan');
    const answer = await ask({ system, prompt: planPrompt, onWrite: tee(writing(progress, 'The model'), previewer(view, progress)) });
    tr.call(span, answer, system.length + planPrompt.length);
    cost += answer.cost || 0;
    await tr.span('check', 'Check the plan', async (c) => {
      try {
        const parsed = parseAnswer(answer.text);
        plan = parsed.plan;
        // The plan's CREATE SCREEN statements come with it; other code waits for the writers.
        const statements = Object.fromEntries(Object.entries(parsed.code || {}).filter(([, c]) => /^\s*CREATE\s+SCREEN\b/i.test(c)));
        const preparing = prepareScreens(plan, statements, view);
        completePlan(plan, view, library);
        problems = [...preparing, ...(await validate(plan, {}, view, library, shared)).filter((p) => !/has no <code slug=/.test(p))];
      } catch (e) { problems = [e.message]; }
      c.problems = problems.slice(0, 8);
    });
    if (!problems.length) { decidedAt = span.end; progress(`The model planned it in ${Math.round(answer.ms / 1000)} s`); break; }
    planPrompt = `${prompt}\n\n${PLAN_STEP}\n\n# Your previous plan\n\n${answer.text}\n\n# Its problems\n\n${problems.map((p) => `- ${p}`).join('\n')}\n\nAnswer again with the whole plan, these fixed.`;
  }
  if (problems.length) return { plan, code: {}, cost, problems, decidedAt };
  const pieces = planPieces(plan);
  const code = screensToCode(plan, {});
  const planText = `# The plan, decided\n\n<plan>\n${JSON.stringify(plan, null, 1)}\n</plan>`;
  const write = async (pieces, notes = new Map()) => {
    const seen = new Map();
    const t0 = Date.now();
    let last = 0;
    const done = [];
    progress(`Writing ${pieces.length === 1 ? pieces[0].label : `${pieces.length} pieces at once: ${pieces.map((p) => p.label).join(', ')}`}`);
    await pool(pieces, 6, async (pc) => {
      const fix = notes.get(pc.slug);
      const span = tr.start(fix ? 'fix' : 'write', fix ? `${pc.label}, again` : pc.label, { piece: pc.slug, ...(fix ? { problems: fix.problems.slice(0, 4) } : {}) });
      const extra = fix ? `\n\n# Your previous code for it\n\n<code slug="${pc.slug}">\n${fix.code}\n</code>\n\n# Its problems\n\n${fix.problems.map((p) => `- ${p}`).join('\n')}\n\nWrite it again, whole, with these fixed.` : '';
      const piecePrompt = `${forPiece(pc.item.uses, pc.item.forked_from)}\n\n${planText}\n\n${pieceStep(pc.what, pc.slug)}${extra}`;
      const answer = await ask({
        system,
        prompt: piecePrompt,
        onWrite: ({ chars }) => {
          seen.set(pc.slug, chars);
          const now = Date.now();
          if (now - last < 5000) return;
          last = now;
          const total = [...seen.values()].reduce((a, b) => a + b, 0);
          progress(`The model is writing ${pieces.length - done.length} of ${pieces.length}: ${total.toLocaleString('en-US')} characters so far (${Math.round((now - t0) / 1000)} s)${done.length ? `; done: ${done.join(', ')}` : ''}`, { replace: true });
        },
      });
      tr.call(span, answer, system.length + piecePrompt.length);
      cost += answer.cost || 0;
      const blocks = parseAnswer(`<plan>{}</plan>\n${answer.text}`).code;
      code[pc.slug] = blocks[pc.slug] ?? Object.values(blocks)[0] ?? '';
      done.push(pc.label);
      progress(`Wrote ${pc.label} in ${Math.round(answer.ms / 1000)} s`);
    });
  };
  await write(pieces);
  problems = await tr.span('check', 'Check the code', async (c) => { const p = [...prepareScreens(plan, code, view), ...await validate(plan, code, view, library, shared)]; c.problems = p.slice(0, 8); return p; });
  if (problems.length) {
    const notes = new Map();
    for (const pc of pieces) {
      const mine = problems.filter((p) => p.startsWith(`${pc.label}:`) || p.startsWith(`${pc.label} `));
      if (mine.length) notes.set(pc.slug, { code: code[pc.slug] || '', problems: mine });
    }
    const again = pieces.filter((pc) => notes.has(pc.slug));
    progress(`Checking the code: ${problems.length} problem${problems.length === 1 ? '' : 's'} (${problems.slice(0, 3).join('; ').slice(0, 400)})${again.length ? `; writing ${again.map((p) => p.label).join(', ')} again` : ''}`);
    if (again.length) {
      await write(again, notes);
      problems = await tr.span('check', 'Check the code again', async (c) => { const p = [...prepareScreens(plan, code, view), ...await validate(plan, code, view, library, shared)]; c.problems = p.slice(0, 8); return p; });
    }
  }
  return { plan, code, cost, problems, decidedAt };
}

// Runs a request through the model and applies its answer as the next version.
export async function forge(engine, appId, request, progress = () => {}) {
  const tr = new Trace();
  try {
    return await forgeTraced(engine, appId, request, progress, tr);
  } catch (e) {
    e.trace = tr.toJSON();
    throw e;
  }
}

async function forgeTraced(engine, appId, request, progress, tr) {
  const { store } = engine;
  const started = Date.now();
  progress('Reading the app and its history from Strata');
  const context = await tr.span('strata', 'Read the app, its data and the library from Strata', () => buildContext(engine, appId, request));
  const { prompt, view, library, shared, system } = context;
  if (library.length) progress(`Found ${library.length} capabilit${library.length === 1 ? 'y' : 'ies'} in the library that may fit: ${library.map((c) => c.title).join(', ')}`);
  let answer;
  let plan;
  let code;
  let cost = 0;
  let problems = [];
  let attemptPrompt = prompt;
  let decidedAt = 0;
  const split = SPLIT === 'always' || (SPLIT === 'first' && !view.comp.n);
  if (split) ({ plan, code, cost, problems, decidedAt } = await buildSplit(context, progress, tr));
  for (let attempt = 1; attempt <= 2 && !split; attempt++) {
    progress(attempt === 1 ? `Asking the model (${MODEL}) to ${view.comp.n ? 'change the app' : 'build the first version'}` : `Asking the model again to fix: ${problems.slice(0, 3).join('; ')}`);
    const span = tr.start('model', attempt === 1 ? (view.comp.n ? 'The model plans and writes the change' : 'The model plans and writes the first version') : 'The model answers again, to fix problems');
    answer = await ask({ system, prompt: attemptPrompt, onWrite: tee(writing(progress), previewer(view, progress)) });
    tr.call(span, answer, system.length + attemptPrompt.length);
    cost += answer.cost || 0;
    progress(`The model answered in ${Math.round(answer.ms / 1000)} s`);
    await tr.span('check', 'Check the code', async (c) => {
      try {
        ({ plan, code } = parseAnswer(answer.text));
        const preparing = prepareScreens(plan, code, view);
        completePlan(plan, view, library);
        problems = [...preparing, ...await validate(plan, code, view, library, shared)];
      } catch (e) {
        problems = [e.message];
      }
      c.problems = problems.slice(0, 8);
    });
    if (!problems.length) { decidedAt = span.end; break; }
    progress(`Checking the code: ${problems.length} problem${problems.length === 1 ? '' : 's'}`);
    attemptPrompt = `${prompt}\n\n# Your previous answer\n\n${answer.text}\n\n# Its problems\n\n${problems.map((p) => `- ${p}`).join('\n')}\n\nAnswer again, in full, with these fixed.`;
  }
  if (problems.length) {
    const err = new Error(`the model's answer did not pass the checks: ${problems.join('; ')}`);
    err.cost = cost;
    throw err;
  }
  progress('The code passed the checks; storing it in Strata');
  let storing = tr.start('strata', 'Store the code and the new version in Strata');

  // Tables first: data the new code reads must exist when it goes live.
  const caps = new Map(view.comp.caps.map((id) => {
    const c = store.capCache.get(id);
    return [`${c.kind}/${c.slug}`, id];
  }));
  const existingTables = new Set(await store.tableNames());
  const tableSpecs = new Map(view.tables.map((t) => [t.spec.name, t.spec]));
  const written = [];
  for (const t of plan.tables || []) {
    const old = tableSpecs.get(t.name);
    const columns = [...(old?.columns || [])];
    for (const c of t.columns || []) if (!columns.find((x) => x.name === c.name)) columns.push({ name: c.name, type: c.type, ...(c.type === 'meaning' ? { of: [].concat(c.of || []) } : {}) });
    const links = old?.links || t.links || null;
    if (links) for (const n of ['src', 'dst']) if (!columns.find((x) => x.name === n)) columns.unshift({ name: n, type: 'keyword' });
    const spec = { name: t.name, description: t.description || old?.description || '', columns, ...(links ? { links } : {}) };
    const res = await store.ensureTable(appId, spec, existingTables);
    if (res.created) progress(`Created table ${res.created}`);
    if (res.added.length) progress(`Added ${res.added.join(', ')} to ${t.name}`);
    if (!res.created && !res.added.length && old) continue;
    const id = await store.putCapability({ app: appId, slug: t.name, kind: 'table', title: t.name, description: spec.description, spec, code: '' });
    caps.set(`table/${t.name}`, id);
    written.push(id);
  }
  // Screens: their queries run now against the tables as they are, and the
  // model gets one try to fix the screens whose queries fail.
  if ((plan.views || []).some((v) => v.screen)) {
    const names = [...new Set([...view.tables.map((t) => t.spec.name), ...(plan.tables || []).map((t) => t.name)])];
    const run = (label) => tr.span('check', label, async (c) => { const p = await dryRunScreens(engine.db, appId, names, plan); c.problems = p.slice(0, 8); return p; });
    let failed = await run("Run the screens' queries");
    if (failed.length) {
      tr.end(storing);
      progress(`Running the screens' queries: ${failed.length} failed (${failed.slice(0, 2).join('; ').slice(0, 300)}); asking the model to fix them`);
      const span = tr.start('fix', 'The model fixes the screens whose queries fail', { problems: failed.slice(0, 4) });
      const fixPrompt = `${prompt}\n\n# Your plan\n\n<plan>\n${JSON.stringify(plan, null, 1)}\n</plan>\n\n# These queries failed when run against the app's tables\n\n${failed.map((p) => `- ${p}`).join('\n')}\n\n# This step\n\nAnswer with a <plan> holding only "views": each screen view above whose queries failed, whole, with its queries fixed (the same slug, title and order). No code blocks, nothing else.`;
      try {
        const answer = await ask({ system, prompt: fixPrompt, onWrite: writing(progress) });
        tr.call(span, answer, system.length + fixPrompt.length);
        cost += answer.cost || 0;
        const fixed = parseAnswer(answer.text).plan;
        const cols = columnsFor(view, plan);
        for (const v of fixed.views || []) {
          const i = (plan.views || []).findIndex((x) => x.slug === v.slug);
          if (i < 0 || !v.screen || typeof v.screen !== 'object') continue;
          const ex = expandScreen(v.screen);
          if (ex.problems.length || checkScreen(ex.screen, cols).length) continue;
          v.screen = ex.screen;
          plan.views[i] = { ...plan.views[i], screen: v.screen };
          code[v.slug] = JSON.stringify(v.screen, null, 1);
        }
      } catch (e) { if (!span.end) tr.end(span, { error: e.message }); }
      failed = await run("Run the screens' queries again");
      progress(failed.length ? `Still failing, stored as they are (the runtime reports them): ${failed.slice(0, 2).join('; ').slice(0, 300)}` : "The screens' queries run");
      storing = tr.start('strata', 'Store the code and the new version in Strata');
    }
  }
  for (const [kind, list] of [['view', plan.views], ['function', plan.functions]]) {
    for (const item of list || []) {
      const spec = kind === 'view' ? { order: item.order ?? 50, ...(item.screen ? { screen: true } : {}) } : { on: item.on || null };
      const id = await store.putCapability({ app: appId, slug: item.slug, kind, title: item.title || item.slug, description: item.description || '',
        code: code[item.slug], spec, forked_from: item.forked_from || null, author: request.source === 'repair' ? 'repair' : 'forge' });
      caps.set(`${kind}/${item.slug}`, id);
      written.push(id);
    }
  }
  for (const r of plan.reuse || []) {
    const src = library.find((c) => c.id === r.id);
    const slug = r.slug || src.slug;
    const spec = { ...src.spec, ...(src.kind === 'view' && r.order !== undefined ? { order: r.order } : {}) };
    const id = await store.putCapability({ app: appId, slug, kind: src.kind, title: r.title || src.title, description: src.description,
      code: src.code, spec, forked_from: src.id, author: 'library' });
    caps.set(`${src.kind}/${slug}`, id);
    written.push(id);
    progress(`Reused ${src.title} from the library (${src.id})`);
  }
  for (const s of plan.remove || []) {
    caps.delete(`view/${s}`);
    caps.delete(`function/${s}`);
  }
  // Components and helpers go to the shared library; then the app pins what
  // its views and functions call.
  const writtenShared = new Map();
  const newVersions = [];
  for (const [kind, { ns, plural }] of Object.entries(SHARED)) {
    for (const c of plan[plural] || []) {
      const prev = shared.get(`${kind}/${c.name}`);
      const id = await store.putCapability({ app: LIB, slug: c.name, kind, title: c.title || c.name, description: c.description || '',
        code: code[`${ns}:${c.name}`], spec: { props: String(c.props || c.api || ''), change: prev ? String(c.change || '') : '', by: appId }, author: request.source === 'repair' ? 'repair' : 'forge' });
      writtenShared.set(`${kind}/${c.name}`, id);
      written.push(id);
      const version = Number(id.split('@').pop());
      progress(prev ? `Wrote version ${version} of the shared ${kind} ${c.name}` : `Wrote a new shared ${kind}, ${c.name}`);
      if (prev) newVersions.push({ kind, name: c.name, id, version, title: c.title || prev.title, change: String(c.change || '') });
    }
  }
  const had = new Map(view.shared.map((c) => [`${c.kind}/${c.slug}`, c.id]));
  const roots = [...caps.entries()].filter(([k]) => k.startsWith('view/') || k.startsWith('function/')).map(([, id]) => id);
  const pinned = await pinShared(store, roots, { written: writtenShared, had, upgrade: new Set(plan.upgrade || []), shared });
  for (const k of [...caps.keys()]) if (SHARED_KINDS.some((kind) => k.startsWith(`${kind}/`))) caps.delete(k);
  for (const [key, id] of pinned) {
    caps.set(key, id);
    const [kind, name] = key.split('/');
    if (!writtenShared.has(key) && had.get(key) !== id) progress(`Uses the shared ${kind} ${name} (version ${id.split('@').pop()})`);
  }
  const n = await store.writeComposition(appId, [...caps.values()], request.text, plan.summary || '');
  tr.end(storing, { written: written.length });
  tr.decisions.push(...planDecisions({ plan, view, library, shared, pinned, written: writtenShared, code, at: decidedAt }));
  progress(`Version ${n} is live: ${written.length} capabilit${written.length === 1 ? 'y' : 'ies'} written`);
  // Example rows go in once the tables are part of the live version.
  if (!view.comp.n && plan.seed) {
    const seeding = tr.start('strata', 'Add example rows');
    let added = 0;
    for (const [table, rows] of Object.entries(plan.seed)) {
      for (const row of (rows || []).slice(0, 20)) {
        try { await engine.insert(appId, table, row, { depth: 1 }); added++; } catch (e) { progress(`Example row skipped: ${e.message}`); }
      }
    }
    if (added) progress(`Added ${added} example rows`);
    tr.end(seeding, { rows: added });
  }
  await offerNewVersions(engine, appId, newVersions, progress);
  const suggestions = (plan.suggestions || []).filter((s) => s && s.text).slice(0, 3)
    .map((s) => ({ text: String(s.text).slice(0, 300), why: String(s.why || '').slice(0, 300), source: 'forge' }));
  if (suggestions.length) {
    await retireSuggestions(engine, appId);
    await store.putSuggestions(appId, suggestions);
  }
  return { n, summary: plan.summary || '', written, cost, ms: Date.now() - started, trace: tr.toJSON() };
}

// A new version of a shared component or helper is offered to every other
// app that uses an older one: taking it needs no model, the code is in Strata.
async function offerNewVersions(engine, appId, newVersions, progress) {
  if (!newVersions.length) return;
  const { store } = engine;
  const from = (await store.getApp(appId))?.name || appId;
  for (const nv of newVersions) {
    // Who depends on the piece, directly or through other pieces: the graph
    // walked backwards from it. The version each app has is in its pins.
    const deps = await store.dependents(`${LIB}/${nv.kind}/${nv.name}`).catch(() => []);
    const users = [];
    for (const d of deps.filter((x) => x.kind === 'app' && x.app !== appId)) {
      const v = await store.resolve(d.app).catch(() => null);
      const pin = v?.shared.find((c) => c.kind === nv.kind && c.slug === nv.name);
      if (pin) users.push({ app: d.app, name: v.app.name, version: Number(pin.version) });
    }
    for (const u of users) {
      if (u.app === appId || u.version >= nv.version) continue;
      const open = (await store.suggestions(u.app)).filter((s) => s.source === 'component' && String(s.cap || '').startsWith(`${LIB}/${nv.kind}/${nv.name}@`));
      if (open.length) await engine.db.write(open.map((s) => ({ op: 'set', table: 'sa_suggestions', key: s.id, field: 'status', value: 'stale' })));
      await store.putSuggestions(u.app, [{ text: `Use the newer ${nv.title} (version ${nv.version}, made for ${from})`, why: nv.change ? `It adds: ${nv.change}` : `A newer version of a shared ${nv.kind} this app uses (it has version ${u.version}).`, source: 'component', cap: nv.id }]);
      progress(`Offered ${nv.name} version ${nv.version} to ${u.name}, which has version ${u.version}`);
    }
  }
}

// Moves an app to another version of a shared component or helper, as a new
// version of the app. No model call: the code is in Strata already.
export async function useSharedVersion(engine, appId, capId, progress = () => {}) {
  const started = Date.now();
  const { store } = engine;
  const view = await store.resolve(appId);
  const [cap] = await store.getCapabilities([capId]);
  if (!cap || !SHARED_KINDS.includes(cap.kind)) throw new Error(`no shared component or helper ${capId}`);
  progress(`Moving ${cap.title} to version ${cap.version}: the code is in Strata already, so no model is needed`);
  const caps = new Map(view.caps.map((c) => [`${c.kind}/${c.slug}`, c.id]));
  const had = new Map(view.shared.map((c) => [`${c.kind}/${c.slug}`, c.id]));
  const roots = [...view.views, ...view.functions].map((c) => c.id);
  const pinned = await pinShared(store, roots, { written: new Map([[`${cap.kind}/${cap.slug}`, cap.id]]), had });
  for (const k of [...caps.keys()]) if (SHARED_KINDS.some((kind) => k.startsWith(`${kind}/`))) caps.delete(k);
  for (const [key, id] of pinned) caps.set(key, id);
  const summary = `${cap.title} is now version ${cap.version}${cap.spec?.change ? `: ${String(cap.spec.change).replace(/\.\s*$/, '')}` : ''}.`;
  const n = await store.writeComposition(appId, [...caps.values()], `Use ${cap.title} version ${cap.version}`, summary);
  progress(`Version ${n} is live`);
  return { n, summary, written: [], cost: 0, ms: Date.now() - started };
}
export const useComponentVersion = useSharedVersion;

async function retireSuggestions(engine, appId) {
  const open = await engine.store.suggestions(appId);
  const ops = open.filter((s) => s.source === 'forge').map((s) => ({ op: 'set', table: 'sa_suggestions', key: s.id, field: 'status', value: 'stale' }));
  if (ops.length) await engine.db.write(ops);
}

// ---- suggestions from use

const SUGGEST_SYSTEM = `You watch how one person uses a small app and suggest what to add or change next. You get the app (its tables, views and functions), what the person did with it lately, and capabilities other apps have in a shared library. Suggest up to 3 changes that the person's behaviour shows they would want: a step they repeat by hand, a filter they keep setting, a view they open to look for one thing, a field they keep writing into notes, something another app does that fits theirs. Each must be small enough to build in one step, new (not what the app does already: read the versions, which say what each change added), specific, and not one of the suggestions made before. The why names what in the record points to it; never claim a habit the record does not show. Errors listed come from the code that runs now. Answer with JSON only: {"suggestions": [{"text": "the change, as the person would ask for it", "why": "what in their use points to it, in plain words", "library": "the id of a library capability it would reuse, or null"}]}. When nothing in the use points anywhere, answer {"suggestions": []}.`;

export async function suggestFromUse(engine, appId) {
  const { store } = engine;
  const view = await store.resolve(appId);
  if (!view.comp.n) return [];
  const live = new Set(view.comp.caps);
  const events = (await store.recentEvents(appId, 400)).filter((e) => e.kind !== 'error' || live.has(e.cap));
  const versions = await store.compositions(appId);
  const history = await store.suggestionHistory(appId, 40);
  const tables = [];
  for (const t of view.tables) tables.push(`- ${t.spec.name} (${await store.countRows(physical(appId, t.spec.name))} rows): ${(t.spec.columns || []).map((c) => `${c.name} ${c.type}`).join(', ')}`);
  const lib = await store.searchCapabilities(`${view.app.name} ${view.app.description} ${view.views.map((v) => v.title).join(' ')}`, { excludeApp: appId, limit: 8 });
  const prompt = [
    `# The app: ${view.app.name}\n${view.app.description || ''}`,
    `## Tables\n${tables.join('\n')}`,
    `## Views\n${view.views.map((v) => `- ${v.slug}: ${v.title}. ${v.description}`).join('\n')}`,
    `## Functions\n${view.functions.map((f) => `- ${f.slug}: ${f.description}${f.spec.on ? ` (runs on ${f.spec.on.event} ${f.spec.on.table})` : ''}`).join('\n') || '(none)'}`,
    `## Versions, oldest first (what each change added)\n${versions.slice().reverse().map((c) => `- v${c.n}: ${c.summary || c.request}`).join('\n')}`,
    `## Suggestions made before\n${history.map((h) => `- [${h.status}] ${h.text}`).join('\n') || '(none)'}`,
    `## What the person did\n${summarizeEvents(events)}`,
    `## Recent actions in order (newest first)\n${events.slice(0, 60).map((e) => `- ${e.ts} ${e.kind} ${e.detail}`).join('\n')}`,
    `## Library\n${lib.map((c) => `- ${c.id} (${c.kind}): ${c.title}. ${c.description}`).join('\n') || '(empty)'}`,
  ].join('\n\n');
  const answer = await ask({ system: SUGGEST_SYSTEM, prompt });
  let out;
  try {
    const m = answer.text.match(/\{[\s\S]*\}/);
    out = JSON.parse(m ? m[0] : answer.text);
  } catch { return []; }
  const items = (out.suggestions || []).filter((s) => s && s.text).slice(0, 3)
    .map((s) => ({ text: String(s.text).slice(0, 300), why: String(s.why || '').slice(0, 300), source: 'use', cap: s.library || '' }));
  if (items.length) await store.putSuggestions(appId, items);
  return items;
}

// A person's own change to a screen, with no model: a patch on the screen as
// the app has it (or a whole screen), expanded, checked and its queries run,
// then stored as the view's next version in a new version of the app. A
// screen that fails a check is not stored; the problems come back.
export async function editScreen(engine, appId, slug, { patch, screen, note } = {}) {
  const store = engine.store;
  const view = await store.resolve(appId);
  const cur = view.views.find((v) => v.slug === slug);
  if (!cur || !cur.spec?.screen) throw Object.assign(new Error(`the app has no screen ${slug}`), { status: 404 });
  let next;
  try { next = screen && typeof screen === 'object' ? screen : applyPatch(JSON.parse(cur.code), Array.isArray(patch) ? patch : []); } catch (e) { throw Object.assign(new Error(e.message), { status: 400 }); }
  const ex = expandScreen(next);
  const problems = [...ex.problems, ...checkScreen(ex.screen, columnsFor(view, {}))];
  if (!problems.length) problems.push(...await dryRunScreens(engine.db, appId, view.tables.map((t) => t.spec.name), { views: [{ slug, screen: ex.screen }] }));
  if (problems.length) throw Object.assign(new Error(problems.slice(0, 4).join('; ')), { status: 400, problems });
  const code = JSON.stringify(ex.screen, null, 1);
  if (code === cur.code) return { n: view.comp.n, unchanged: true };
  // Shared components the screen names must exist; the app pins the ones it
  // did not use before (a pattern may bring the kanban, the bar chart...).
  const shared = await store.shared();
  const missing = [...sharedRefs(code)].filter((k) => !shared.has(k));
  if (missing.length) throw Object.assign(new Error(`the library has no ${missing.map((k) => k.replace('component/', 'ui.').replace('helper/', 'lib.')).join(', ')}`), { status: 400 });
  const id = await store.putCapability({ app: appId, slug, kind: 'view', title: cur.title, description: cur.description, code, spec: cur.spec, author: 'person' });
  const own = view.comp.caps.filter((c) => !view.shared.some((x) => x.id === c)).map((c) => (c === cur.id ? id : c));
  const roots = own.filter((c) => /\/(view|function)\//.test(c));
  const pinned = await pinShared(store, roots, { had: new Map(view.shared.map((c) => [`${c.kind}/${c.slug}`, c.id])), shared });
  const text = String(note || `Edited the screen ${cur.title} by hand`).slice(0, 300);
  const n = await store.writeComposition(appId, [...own, ...pinned.values()], text, text);
  return { n, cap: id, patterns: ex.used };
}
