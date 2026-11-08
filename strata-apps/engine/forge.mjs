// The forge: turns a request ("a reading list where I save links and tag
// them", "add a due date to tasks") into capabilities, with their code, and
// stores them in Strata as the app's next version.

import vm from 'node:vm';
import { ask, parseAnswer, MODEL } from './llm.mjs';
import { checkFunction } from './sandbox.mjs';
import { TYPES, NAME_RE, SLUG_RE, physical } from './store.mjs';

export const SYSTEM = `You build and change small web apps for one person, through a runtime whose backbone is a database, Strata. An app is made of capabilities that the runtime stores in Strata and loads:

- table: the data the app keeps.
- view: a screen of the app, shown as a tab. JavaScript that runs in the browser, in a sandboxed frame.
- function: JavaScript that runs on the server in a sandboxed interpreter: work over many rows, logic several views share, and reactions to writes (triggers).

The person describes what they want in their own words: the first version of a new app, or a change to the app they have. Build exactly that, completely and well, and keep everything else as it is.

# Tables

Column types:
- text: prose, searched by words (titles, notes, descriptions, bodies)
- keyword: a short label you filter, group and sort by: a status, category, person, email, url, color, a reference to another row's id
- keyword[]: a list of labels (tags)
- int, float, bool
- timestamp: a moment, ISO 8601 text in and out ("2026-11-08T10:00:00Z", or a date "2026-11-08")

Every table also has id (keyword, the primary key; app.insert makes one when the row has none) and created (timestamp, set when the row is inserted). Do not declare those two. Table and column names: lowercase letters, digits and _, starting with a letter.

Tables only grow: you may add tables and add columns, never drop or rename them or change a column's type. A column you no longer need stays; leave it alone.

Choose text only for prose people search by words, and keyword for anything you compare with =, group by or sort by exactly. Sorting by a text column works too.

# SQL (Strata's dialect: read-only SELECT through app.sql)

- Strings in single quotes. Parameters are :name, bound from the params object: app.sql('SELECT * FROM tasks WHERE status = :s', {s: 'open'}).
- Filters: =, !=, <, <=, >, >=, BETWEEN, IN (...), IS NULL, IS NOT NULL on keyword, int, float, bool and timestamp columns; times compare with ISO text (created >= '2026-11-01'). A bool column alone is a condition (WHERE done, WHERE NOT done; done IS NOT TRUE also takes rows without a value).
- A keyword[] column matches when any of its values does: WHERE tags = 'work'.
- Words in text: WHERE MATCH(body, 'words') (words side by side all must appear; OR, NOT, prefix*, typo~ also work), ranked with ORDER BY bm25(body, :q) DESC. LIKE '%x%' works on text and keyword columns too (fine for small tables). lower(x) works.
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

The frame has no network: fetch, imports, external scripts, fonts and stylesheets are blocked, and so are localStorage and cookies (keep state in a table, or in variables while the view is open). confirm() and prompt() work. Images may load from https URLs the data holds.

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

# Functions

A function's code defines

    function run(input, app) { ... return result; }

a plain synchronous function: app calls return their results directly (no async, no await, no Promises). app in a function:
- app.sql(sql, params) -> rows; app.insert(table, row) -> row; app.update(table, id, fields) -> row; app.remove(table, id)
- app.call(slug, input); app.now() -> ISO time; app.log(...)
It returns any JSON value. It runs in QuickJS with a 10 s limit, no network, no files, no require or import. Date, Math, JSON, RegExp, Array and the rest of standard JavaScript work.

A function may declare a trigger, "on": {"table": "tasks", "event": "insert"} (or "update" or "delete"): it then runs after each such write with input {row, table, event}, where row is the row as stored (the old row for delete). Writes a trigger makes run their own triggers, at most 3 deep. Use a trigger when something must happen whenever rows change (keep a counter, log history, fill a derived column); use a plain function that views call for work on demand.

Keep logic in views when it is simple and only one view needs it.

# Library

Below the app you may get capabilities that other apps built, from Strata's library. When one does what is needed as it is, reuse it instead of writing it again: name it in "reuse" (it is copied into this app as it is; make sure this app has the tables and columns its code reads, adding them in "tables" when needed). When one is close but needs changes, write your own version in a code block, and say "forked_from" with its id. Ignore the ones that do not fit.

# Your answer

First <plan>...</plan> holding one JSON object, then one <code slug="...">...</code> block for each view and function you add or rewrite, the code alone (no markdown fences). Nothing else.

{
  "name": "a short name for the app (only for a new app, or when asked to rename it)",
  "summary": "what this version changed, in one or two plain sentences, written to the person",
  "tables": [{"name": "tasks", "description": "what a row is", "columns": [{"name": "title", "type": "text"}, {"name": "status", "type": "keyword"}]}],
  "views": [{"slug": "board", "title": "Board", "description": "what it shows and lets you do", "order": 1}],
  "functions": [{"slug": "weekly-summary", "description": "what it does, its input and output", "on": null}],
  "remove": [],
  "reuse": [{"id": "abc123/view/board@2", "slug": "board", "title": "Board", "order": 2}],
  "seed": {"tasks": [{"title": "...", "status": "open"}]},
  "suggestions": [{"text": "...", "why": "..."}]
}

- tables: tables to add, and existing tables you add columns to (for an existing table, list only its new columns).
- views and functions: the ones you add or rewrite, each with a code block of the same slug holding its whole code. Leave out the ones that stay as they are. Rewriting a view means writing all of it again. Keep slugs as they are (lowercase letters, digits, dashes). "order" places views (the lowest opens first).
- remove: slugs of views or functions to take out.
- reuse: capabilities from the library to copy, by id.
- seed: example rows, only for a new app, and only when examples help the person see the app working (3 to 8 rows per table, realistic, dated near today). Leave it out when the app holds the person's own data from the start (an expense log, a journal), unless they asked for examples.
- suggestions: 3 changes the person might ask for next, written as they would ask for them ("Let me snooze a task until a date"), each with a short why that refers to what they asked or did. Make them specific to this app, small enough to build in one step, and new: not what the app does already, not what was just asked.
- An app has one to five views. Do not split what belongs together across views.`;

const clip = (s, n) => (s.length > n ? `${s.slice(0, n)}\n... (${s.length - n} more characters)` : s);

function describeTable(t, count, sample) {
  const cols = ['id keyword', 'created timestamp', ...(t.spec.columns || []).map((c) => `${c.name} ${c.type}`)];
  const lines = [`- ${t.spec.name} (${count} rows): ${cols.join(', ')}${t.spec.description ? `\n  ${t.spec.description}` : ''}`];
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

export async function buildContext(engine, appId, request) {
  const { store } = engine;
  const view = await store.resolve(appId);
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
      .map((v) => `<view slug="${v.slug}" title="${v.title}" order="${v.spec.order ?? ''}" id="${v.id}">\n${v.code}\n</view>`);
    parts.push(`## Views\n${views.join('\n\n') || '(none)'}`);
    const fns = view.functions.map((f) => `<function slug="${f.slug}" on='${JSON.stringify(f.spec.on || null)}' id="${f.id}">\n${f.description ? `// ${f.description}\n` : ''}${f.code}\n</function>`);
    parts.push(`## Functions\n${fns.join('\n\n') || '(none)'}`);
    const events = await store.recentEvents(appId, 300);
    parts.push(`## What the person did lately\n${summarizeEvents(events)}`);
    const errors = events.filter((e) => e.kind === 'error').slice(0, 5);
    if (errors.length) parts.push(`## Errors lately\n${errors.map((e) => `- ${e.cap}: ${e.detail}`).join('\n')}`);
  }
  // The library: what other apps built that may fit this request.
  const lib = await store.searchCapabilities(`${request.text} ${view.app.description || ''}`, { excludeApp: appId, kinds: ['view', 'function'], limit: 4 });
  const libCaps = await store.getCapabilities(lib.map((c) => c.id));
  if (libCaps.length) {
    const entries = [];
    for (const c of libCaps) {
      const src = await store.resolve(c.app).catch(() => null);
      const tables = src ? src.tables.map((t) => `${t.spec.name}(${(t.spec.columns || []).map((x) => `${x.name} ${x.type}`).join(', ')})`).join('; ') : '';
      entries.push(`<capability id="${c.id}" kind="${c.kind}" title="${c.title}" app="${src?.app?.name || c.app}">\n// ${c.description}\n// its app's tables: ${tables}\n${clip(c.code, 5000)}\n</capability>`);
    }
    parts.push(`# Library: capabilities other apps have that may fit\n\n${entries.join('\n\n')}`);
  }
  const source = { user: 'the person asked for this', suggestion: 'the person accepted this suggestion', repair: 'the runtime reported this error; fix its cause', library: 'the person asked to bring this in from the library' }[request.source] || 'the person asked for this';
  parts.push(`# The request (${source})\n\n${request.text}`);
  return { prompt: parts.join('\n\n'), view, library: libCaps };
}

function checkView(code) {
  const problems = [];
  if (!/\basync\s+function\s+render\s*\(|\bfunction\s+render\s*\(/.test(code)) problems.push('must define async function render(root, app)');
  if (/\bimport\s+[\w{*]|\brequire\s*\(/.test(code)) problems.push('no import or require: the frame has nothing to load');
  if (/\bfetch\s*\(/.test(code)) problems.push('no fetch: the frame has no network; use app.sql and app.call');
  if (/\blocalStorage\b/.test(code)) problems.push('no localStorage: keep state in a table');
  try { new vm.Script(`(async function () {\n${code}\n})`, { filename: 'view.js' }); } catch (e) { problems.push(`syntax: ${e.message}`); }
  if (code.length > 120 * 1024) problems.push('the code is longer than 120 KiB');
  return problems;
}

// Checks a plan against the app as it is. Returns a list of problems.
export async function validate(plan, code, view, library) {
  const problems = [];
  const tables = new Map(view.tables.map((t) => [t.spec.name, t.spec]));
  for (const t of plan.tables || []) {
    if (!NAME_RE.test(t.name || '')) { problems.push(`table name ${JSON.stringify(t.name)}: lowercase letters, digits and _`); continue; }
    const have = new Map((tables.get(t.name)?.columns || []).map((c) => [c.name, c.type]));
    for (const c of t.columns || []) {
      if (!NAME_RE.test(c.name || '')) problems.push(`column ${t.name}.${c.name}: lowercase letters, digits and _`);
      else if (c.name === 'id' || c.name === 'created') problems.push(`column ${t.name}.${c.name}: every table has id and created already; do not declare them`);
      if (!TYPES[c.type]) problems.push(`column ${t.name}.${c.name}: type ${JSON.stringify(c.type)} is not one of ${Object.keys(TYPES).join(', ')}`);
      if (have.has(c.name) && have.get(c.name) !== c.type) problems.push(`column ${t.name}.${c.name} is ${have.get(c.name)} already; types never change (add a new column instead)`);
    }
  }
  const slugs = new Set();
  for (const [kind, list] of [['view', plan.views], ['function', plan.functions]]) {
    for (const item of list || []) {
      if (!SLUG_RE.test(item.slug || '')) { problems.push(`${kind} slug ${JSON.stringify(item.slug)}: lowercase letters, digits and dashes`); continue; }
      if (slugs.has(item.slug)) problems.push(`${item.slug} is given twice`);
      slugs.add(item.slug);
      const src = code[item.slug];
      if (!src || !src.trim()) { problems.push(`${kind} ${item.slug} has no <code slug="${item.slug}"> block`); continue; }
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

// Runs a request through the model and applies its answer as the next version.
export async function forge(engine, appId, request, progress = () => {}) {
  const { store } = engine;
  const started = Date.now();
  progress('Reading the app and its history from Strata');
  const { prompt, view, library } = await buildContext(engine, appId, request);
  if (library.length) progress(`Found ${library.length} capabilit${library.length === 1 ? 'y' : 'ies'} in the library that may fit: ${library.map((c) => c.title).join(', ')}`);
  let answer;
  let plan;
  let code;
  let cost = 0;
  let problems = [];
  let attemptPrompt = prompt;
  for (let attempt = 1; attempt <= 2; attempt++) {
    progress(attempt === 1 ? `Asking the model (${MODEL}) to ${view.comp.n ? 'change the app' : 'build the first version'}` : `Asking the model again to fix: ${problems.slice(0, 3).join('; ')}`);
    answer = await ask({ system: SYSTEM, prompt: attemptPrompt });
    cost += answer.cost || 0;
    progress(`The model answered in ${Math.round(answer.ms / 1000)} s`);
    try {
      ({ plan, code } = parseAnswer(answer.text));
      problems = await validate(plan, code, view, library);
    } catch (e) {
      problems = [e.message];
    }
    if (!problems.length) break;
    progress(`Checking the code: ${problems.length} problem${problems.length === 1 ? '' : 's'}`);
    attemptPrompt = `${prompt}\n\n# Your previous answer\n\n${answer.text}\n\n# Its problems\n\n${problems.map((p) => `- ${p}`).join('\n')}\n\nAnswer again, in full, with these fixed.`;
  }
  if (problems.length) {
    const err = new Error(`the model's answer did not pass the checks: ${problems.join('; ')}`);
    err.cost = cost;
    throw err;
  }
  progress('The code passed the checks; storing it in Strata');

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
    for (const c of t.columns || []) if (!columns.find((x) => x.name === c.name)) columns.push({ name: c.name, type: c.type });
    const spec = { name: t.name, description: t.description || old?.description || '', columns };
    const res = await store.ensureTable(appId, spec, existingTables);
    if (res.created) progress(`Created table ${res.created}`);
    if (res.added.length) progress(`Added ${res.added.join(', ')} to ${t.name}`);
    if (!res.created && !res.added.length && old) continue;
    const id = await store.putCapability({ app: appId, slug: t.name, kind: 'table', title: t.name, description: spec.description, spec, code: '' });
    caps.set(`table/${t.name}`, id);
    written.push(id);
  }
  for (const [kind, list] of [['view', plan.views], ['function', plan.functions]]) {
    for (const item of list || []) {
      const spec = kind === 'view' ? { order: item.order ?? 50 } : { on: item.on || null };
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
  if (!view.comp.n && plan.seed) {
    let n = 0;
    for (const [table, rows] of Object.entries(plan.seed)) {
      for (const row of (rows || []).slice(0, 20)) {
        try { await engine.insert(appId, table, row, { depth: 1 }); n++; } catch (e) { progress(`Example row skipped: ${e.message}`); }
      }
    }
    if (n) progress(`Added ${n} example rows`);
  }
  if (plan.name && (!view.comp.n || /\brename|\bname\b/i.test(request.text))) await store.updateApp(appId, { name: String(plan.name).slice(0, 60) });
  const n = await store.writeComposition(appId, [...caps.values()], request.text, plan.summary || '');
  progress(`Version ${n} is live: ${written.length} capabilit${written.length === 1 ? 'y' : 'ies'} written`);
  const suggestions = (plan.suggestions || []).filter((s) => s && s.text).slice(0, 3)
    .map((s) => ({ text: String(s.text).slice(0, 300), why: String(s.why || '').slice(0, 300), source: 'forge' }));
  if (suggestions.length) {
    await retireSuggestions(engine, appId);
    await store.putSuggestions(appId, suggestions);
  }
  return { n, summary: plan.summary || '', written, cost, ms: Date.now() - started };
}

async function retireSuggestions(engine, appId) {
  const open = await engine.store.suggestions(appId);
  const ops = open.filter((s) => s.source === 'forge').map((s) => ({ op: 'set', table: 'sa_suggestions', key: s.id, field: 'status', value: 'stale' }));
  if (ops.length) await engine.db.write(ops);
}

// ---- suggestions from use

const SUGGEST_SYSTEM = `You watch how one person uses a small app and suggest what to add or change next. You get the app (its tables, views and functions), what the person did with it lately, and capabilities other apps have in a shared library. Suggest up to 3 changes that the person's behaviour shows they would want: a step they repeat by hand, a filter they keep setting, a view they open to look for one thing, a field they keep writing into notes, something another app does that fits theirs. Each must be small enough to build in one step, new (not what the app does already), and specific. Answer with JSON only: {"suggestions": [{"text": "the change, as the person would ask for it", "why": "what in their use points to it, in plain words", "library": "the id of a library capability it would reuse, or null"}]}. When nothing in the use points anywhere, answer {"suggestions": []}.`;

export async function suggestFromUse(engine, appId) {
  const { store } = engine;
  const view = await store.resolve(appId);
  if (!view.comp.n) return [];
  const events = await store.recentEvents(appId, 400);
  const tables = [];
  for (const t of view.tables) tables.push(`- ${t.spec.name} (${await store.countRows(physical(appId, t.spec.name))} rows): ${(t.spec.columns || []).map((c) => `${c.name} ${c.type}`).join(', ')}`);
  const lib = await store.searchCapabilities(`${view.app.name} ${view.app.description} ${view.views.map((v) => v.title).join(' ')}`, { excludeApp: appId, limit: 8 });
  const prompt = [
    `# The app: ${view.app.name}\n${view.app.description || ''}`,
    `## Tables\n${tables.join('\n')}`,
    `## Views\n${view.views.map((v) => `- ${v.slug}: ${v.title}. ${v.description}`).join('\n')}`,
    `## Functions\n${view.functions.map((f) => `- ${f.slug}: ${f.description}${f.spec.on ? ` (runs on ${f.spec.on.event} ${f.spec.on.table})` : ''}`).join('\n') || '(none)'}`,
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
