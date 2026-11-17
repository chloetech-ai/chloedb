# Prototypes on ChloeDB

Five web apps and a hub page, served by one Node server (`server.mjs`) from one
chloedb-server that holds every app's tables. Live at prototypes.chloetech.ai behind a
password; each app at `/<slug>/`.

| app | slug | what it shows |
|---|---|---|
| [MCP Atlas](apps/atlas/README.md) | `atlas` | every server in the MCP registry, by the credential it declares, and the Nodwyn binding that would contain it |
| [Leak Lab](apps/leaklab/README.md) | `leaklab` | 24 exfiltration channels against four ways of holding a secret |
| [Black Box](apps/blackbox/README.md) | `blackbox` | public agent sessions as hash-chained rows, with blame from a patch line to the step that wrote it |
| [Memory Firewall](apps/memfw/README.md) | `memfw` | a poisoned page written into two memories that differ only in provenance |
| [Policy Time Machine](apps/timemachine/README.md) | `timemachine` | a rule written as SQL, replayed over recorded sessions and labelled attacks before it ships |

```
prototypes/
  server.mjs          the hub server: / is site/, /<slug>/ is apps/<slug>/public, /<slug>/api/* is apps/<slug>/app.mjs
  lib/db.mjs          ChloeDB over HTTP: query(), sql(), write(), upsertAll(), ensureTable(), count()
  lib/llm.mjs         Claude behind a spend cap: complete(), runTools(), textOf(), jsonOf(), spendStatus()
  lib/http.mjs        json(), readBody(), sse(), serveStatic()
  site/               the hub page; site/shared/ds.css and ds.js are the shared design system
  apps/<slug>/        one app: public/ (its page), app.mjs (its API), load.mjs (its data), README.md
  deploy/             Kubernetes manifests (namespace prototypes)
```

## Run it locally

```sh
cd prototypes && npm install
# a ChloeDB server for development (any port; the prebuilt binary is enough)
chloedb-server --db .dev/protos.chloedb --listen 127.0.0.1:7799 --grpc off --models .dev/models --start-apps off &
export CHLOEDB_URL=http://127.0.0.1:7799 ANTHROPIC_API_KEY=...   # the key only for live model calls
node apps/<slug>/load.mjs          # loads the app's tables (idempotent)
PORT=8090 node server.mjs          # http://127.0.0.1:8090/<slug>/
PROTOS_APPS=leaklab PORT=8091 node server.mjs   # only some apps
```

## An app

`apps/<slug>/app.mjs` may export:

```js
export const meta = { title: 'Leak Lab', tagline: '...', order: 2 };   // the hub reads it from /api/hub
export async function init(ctx) { /* runs once at start, in the background */ }
export async function handle(ctx) {                                    // /<slug>/api/...
  if (ctx.route === '/matrix' && ctx.method === 'GET') {
    const r = await ctx.db.query('SELECT ... FROM leaklab_runs ...', {});
    return ctx.json(ctx.res, 200, { rows: r.rows, queries: [q(r)] });
  }
  return false;                                                        // 404
}
const q = r => ({ sql: r.sql, took_ms: r.took_ms, rows: r.rows.length, plan: r.plan });
```

`ctx` has `req`, `res`, `url`, `query` (URLSearchParams), `method`, `route` (the path below
`/api`), `ip`, `body()` (the JSON body), `db`, `llm`, `json(res, status, data)`, `sse(res)`.

Conventions every app keeps:

- **Tables are prefixed with the slug** (`leaklab_runs`), and `load.mjs` creates and fills
  them idempotently (`db.ensureTable`, `db.upsertAll`). Data an app needs is prepared by a
  script in the app's folder from the sources it names; files over a few MB stay out of git.
- **The SQL behind every view.** API answers carry `queries: [{sql, params, took_ms, rows, plan}]`;
  `ds.js`'s `api()` shows them in the SQL drawer, with the server's own time.
- **Live model calls go through `lib/llm.mjs`** with `app` and `ip` set, so the spend cap and the
  per-visitor limit apply. When a call throws `LlmError` (`budget`, `rate`, `no_key`), the app shows
  recorded results instead and says so. Measurements are run offline by a script, stored in the
  app's tables, and shown with how they were made (model, number of trials, date).
- **Honest numbers.** Every number on a page is computed from rows (or a recorded run) and says
  where it comes from in a provenance line. Nothing is typed in by hand.
- **The page** starts with `<link rel="stylesheet" href="/shared/ds.css">` and
  `import { topbar } from '/shared/ds.js'; topbar({ app: 'Leak Lab' })`. Plain ES modules, no build
  step; libraries from a CDN with an exact version if needed.
- No secrets in the code: the database token and the model key come from the environment.

## Deploy

The Deployment `site` in namespace `prototypes` clones this branch at start, installs the
dependencies and runs `server.mjs`; `kubectl -n prototypes rollout restart deploy/site` picks up
a new push. The database is the Deployment `protos-db` (chloedb-server, data on the node's disk).
Tables are loaded by running an app's `load.mjs` with `CHLOEDB_URL` pointed at it through
`kubectl -n prototypes port-forward svc/protos-db 7700`.
