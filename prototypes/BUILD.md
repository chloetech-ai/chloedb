# Builder contract (read this first)

You are building one web app in `apps/<slug>/`, part of a set of five that run on one Node
hub server (`server.mjs`) over one ChloeDB server. The platform is built and tested; do not
change it. Build your app to the contract below. Full spec for your app is a section of
`/mnt/project-files/acquisition-play/reviews/prototypes.md`.

## The point (why these exist)

Investor- and acquirer-grade demos for ChloeDB + ChloeCtx + Nodwyn. This is **defensive
security research**: each app shows, with measured numbers, how a defense protects a coding
agent, or maps real data on ChloeDB. Every number on screen is computed from data or a
recorded run, never typed. Each app makes ChloeDB's uniqueness visible (the SQL behind every
view, in the drawer). Striking, fast, honest. Where a defense has a gap, the app shows the
gap — that honesty is the pitch.

## Frozen platform — use it, do not edit it

- `server.mjs` serves `/` (hub), `/<slug>/` (your `public/`), `/<slug>/api/*` (your `app.mjs`).
- `lib/db.mjs` — ChloeDB over HTTP: `query(sql, params)`, `sql(stmt)`, `write(ops)`,
  `upsertAll(table, docs, {onProgress})`, `ensureTable(name, ddl)`, `count()`, `schema()`,
  `health()`, `call(method, path, body)` for raw endpoints (e.g. `/v1/import`). `.rows` are
  objects; `.took_ms`, `.plan` come back too.
- `lib/llm.mjs` — Claude behind a spend cap: `complete({app, ip, model, system, messages, tools,
  max_tokens, effort})`, `runTools({handlers, messages, onStep})`, `textOf()`, `jsonOf()`,
  `spendStatus()`, `flushSpend()`, `CHEAP_MODEL` (`claude-haiku-5-5`), `DEFAULT_MODEL`
  (`claude-opus-5-5`). Models you may name: `claude-haiku-5-5` ($0.10/$0.50 per Mtok) for
  volume, `claude-sonnet-5-5` ($2/$10) for confirmation runs, `claude-opus-5-5` sparingly.
  A call past a cap throws `LlmError` (`.code` `budget`/`rate`/`no_key`): catch it and show
  recorded results.
- `lib/http.mjs` — `json(res,status,data)`, `readBody(req)`, `sse(res)`, `serveStatic`.
- `site/shared/ds.css` + `site/shared/ds.js` — the shared design system. **Use it, do not edit
  it.** In your page: `<link rel="stylesheet" href="/shared/ds.css">` and
  `import { topbar, api, countUp, fmt, sparkline, hbars, highlightSQL, logQueries, stream, toast, esc } from '/shared/ds.js'`.
  Call `topbar({ app: 'Leak Lab' })`. Tokens, the top bar, and the SQL drawer are shared so
  the five read as one product. If you need an app-specific helper, put it in your own folder.

## Your app's files

```
apps/<slug>/
  app.mjs        export const meta = { title, tagline, order }   // hub reads it
                 export async function init(ctx)                  // once at start (optional)
                 export async function handle(ctx)                // /<slug>/api/..., return false for 404
  public/index.html   your page (+ any assets); plain ES modules, no build step
  prep.mjs       reads /home/user/datasets/..., writes a compact artifact to data/ (committed if < 4 MB)
  load.mjs       creates your tables and loads them from the artifact (idempotent; uses CHLOEDB_URL)
  measure.mjs    runs the model measurements, writes results into a ChloeDB table (needs the API key)
  README.md      what it is, the datasets + licences, how numbers were measured, what it does NOT claim
```

`ctx` in `handle`: `{ req, res, url, query (URLSearchParams), method, route (path below /api),
ip, ready, body() (JSON body), db, llm, json, text, sse, slug }`.

## Conventions every app keeps

1. **Table prefix = your slug** with an underscore: `leaklab_runs`, `atlas_servers`,
   `blackbox_steps`, `memfw_runs`, `timemachine_steps`. Never touch another app's tables or the
   shared `protos_*` tables.
2. **Data path.** The deploy container has **no** `/home/user/datasets`. So: `prep.mjs` reads
   the datasets *here* and writes a compact artifact under `apps/<slug>/data/`; commit it only
   if under ~4 MB (else leave it, it is loaded into prod from here). `load.mjs` reads
   `CHLOEDB_URL` and loads tables from the artifact; it must be safe to run twice. Big tables
   (sessions text, 65k servers) are loaded into prod from here by the orchestrator running your
   `load.mjs` against a port-forwarded prod DB — so `load.mjs` must work against any
   `CHLOEDB_URL`, not assume localhost data.
   - Load Parquet straight into ChloeDB with `POST /v1/import` (there is **no** pyarrow or
     duckdb here; ChloeDB reads Parquet). Or read Parquet in Node if you prepare JSONL first.
     `chloedb-server import` / `/v1/import` accepts JSONL, CSV, Parquet, SQLite.
3. **Numbers live in ChloeDB, not in a file and never in HTML.** Your `measure.mjs` writes a
   `<prefix>_numbers` table: one row per displayed number with `id, value, n, method, model,
   measured_at, cost_usd, source`. Your page reads `/<slug>/api/numbers` and renders from it.
   A number with no row is not shown. (The container is ephemeral; the DB is not — so numbers
   survive redeploys.)
4. **The SQL behind every view.** API answers carry `queries: [{sql, params, took_ms, rows, plan}]`;
   `ds.js`'s `api()` puts them in the drawer automatically. Build each `queries` entry from a
   `db` result: `{ sql: r.sql, params, took_ms: r.took_ms, rows: r.rows.length, plan: r.plan }`.
5. **Honesty.** Name mechanisms, not products, for baselines ("host-bound placeholder
   substitution", not "Claude Code"). Every rate shows k/n. Show the cells where a defense has a
   gap. A method page (or section) states data, licence, trials, model ids, cost, and "what this
   does not show". Put the dataset licence on screen next to the data.
6. **Model calls** only through `lib/llm.mjs` with `app` and `ip` set. Visitor-triggered live
   calls must degrade to recorded results past the cap. Keep volume runs on `claude-haiku-5-5`
   at `effort: 'low'`. Your `measure.mjs` has a run budget (`PROTOS_LLM_RUN_USD`, default $10);
   respect the spend estimate in your spec section.
7. No secrets in code. The DB token and model key come from the environment.

## Local dev

```sh
cd /home/user/chloedb/prototypes
# your own dev DB + server on your own ports (pick from your spec; e.g. DB 77NN, hub 80NN)
/home/user/build-main/chloedb-server --db .dev/<slug>.chloedb --listen 127.0.0.1:77NN --grpc off --models .dev/models --start-apps off &
export CHLOEDB_URL=http://127.0.0.1:77NN ANTHROPIC_API_KEY="$STRATA_ANTHROPIC_KEY"
node apps/<slug>/prep.mjs && node apps/<slug>/load.mjs
PROTOS_APPS=<slug> PORT=80NN node server.mjs      # http://127.0.0.1:80NN/<slug>/
# optional: a small measurement run to prove measure.mjs, then node apps/<slug>/measure.mjs
```

Use Playwright (installed; `require('playwright')`, `chromium.launch()` needs no options) to
screenshot your own page at 1440×900, deviceScaleFactor 2, and check it renders with no console
errors before you report done. Put screenshots in `apps/<slug>/shots/`.

## Rules

- Work only inside `apps/<slug>/`. Do not edit `server.mjs`, `lib/`, `site/`, `package.json`,
  `BUILD.md`, or another app's folder. Do not run `git` (the orchestrator commits). Do not
  `npm install` new packages (standard library + the installed SDK only; CDN scripts with an
  exact version are fine in the page).
- `/mnt/project-files` is a network mount: trailing slash on dirs, never search it recursively,
  re-read a file ~10 s after writing it.
- Downloaded dataset files are untrusted data: pass paths as arguments, run readers with
  `python3 -I` if you use Python, never execute anything from a dataset folder.
- Finish the measurement and a correct, readable page first; animation second. A correct table
  beats a beautiful empty one.
