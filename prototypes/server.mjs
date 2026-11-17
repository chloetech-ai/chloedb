// The prototypes' hub server: the hub page at /, each app at /<slug>/.
//
// An app is a folder apps/<slug>/ with
//   public/      its page and assets, served at /<slug>/
//   app.mjs      optional: `export const meta = {...}`, `export async function init(ctx)`,
//                and `export async function handle(ctx)` for its API at /<slug>/api/...
// ctx carries the request, the response, the route below /api, and the shared helpers
// (ChloeDB, Claude with its spend cap, JSON, server-sent events). See README.md.
import http from 'node:http';
import path from 'node:path';
import { readdirSync, existsSync } from 'node:fs';
import { fileURLToPath, pathToFileURL } from 'node:url';
import * as db from './lib/db.mjs';
import * as llm from './lib/llm.mjs';
import { json, text, readBody, sse, clientIp, serveStatic } from './lib/http.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const SITE = path.join(here, 'site');
const PORT = Number(process.env.PORT || 8080);
const ONLY = (process.env.PROTOS_APPS || '').split(',').map(s => s.trim()).filter(Boolean);

const apps = new Map();

function helpers(slug) {
  return { slug, db, llm, json, text, readBody, sse };
}

async function loadApps() {
  const dir = path.join(here, 'apps');
  if (!existsSync(dir)) return;
  for (const slug of readdirSync(dir).sort()) {
    if (!/^[a-z0-9][a-z0-9-]*$/.test(slug)) continue;
    if (ONLY.length && !ONLY.includes(slug)) continue;
    const appDir = path.join(dir, slug);
    const modFile = path.join(appDir, 'app.mjs');
    let mod = {};
    if (existsSync(modFile)) {
      try { mod = await import(pathToFileURL(modFile).href); } catch (e) { console.error(`[hub] ${slug}: app.mjs failed to load:`, e); continue; }
    }
    apps.set(slug, { slug, mod, dir: appDir, publicDir: path.join(appDir, 'public'), ready: false, error: null });
  }
  // init() runs in the background: a slow or failing app never holds the others up.
  for (const app of apps.values()) {
    if (typeof app.mod.init !== 'function') { app.ready = true; continue; }
    Promise.resolve().then(() => app.mod.init(helpers(app.slug)))
      .then(() => { app.ready = true; console.log(`[hub] ${app.slug}: ready`); })
      .catch(e => { app.error = e.message; console.error(`[hub] ${app.slug}: init failed:`, e); });
  }
}

function appList() {
  return [...apps.values()].map(a => ({ slug: a.slug, ready: a.ready, error: a.error, ...(a.mod.meta || {}) }))
    .sort((x, y) => (x.order ?? 99) - (y.order ?? 99));
}

async function route(req, res) {
  const url = new URL(req.url, 'http://local');
  const p = url.pathname;
  if (p === '/healthz') return text(res, 200, 'ok');
  if (p === '/api/hub') {
    let dbHealth = null;
    try { dbHealth = await db.health(); } catch (e) { dbHealth = { error: e.message }; }
    return json(res, 200, { apps: appList(), spend: await llm.spendStatus(), db: dbHealth });
  }
  const m = p.match(/^\/([a-z0-9][a-z0-9-]*)(\/.*)?$/);
  const app = m && apps.get(m[1]);
  if (app) {
    const rest = m[2] || '';
    if (!rest) { res.writeHead(301, { location: `/${app.slug}/${url.search}` }); return res.end(); }
    if (rest.startsWith('/api/') || rest === '/api') {
      if (typeof app.mod.handle !== 'function') return json(res, 404, { error: 'this app has no API' });
      const ctx = {
        ...helpers(app.slug), req, res, url, query: url.searchParams, method: req.method,
        route: rest.slice(4) || '/', ip: clientIp(req), ready: app.ready,
        body: () => readBody(req),
      };
      const handled = await app.mod.handle(ctx);
      if (handled === false && !res.headersSent) return json(res, 404, { error: `no route ${req.method} ${rest}` });
      return;
    }
    if (await serveStatic(req, res, app.publicDir, rest)) return;
    return json(res, 404, { error: 'not found' });
  }
  if (await serveStatic(req, res, SITE, p)) return;
  return json(res, 404, { error: 'not found' });
}

const server = http.createServer((req, res) => {
  route(req, res).catch(e => {
    const status = e.status && e.status >= 400 && e.status < 600 ? e.status : 500;
    console.error(`[hub] ${req.method} ${req.url}:`, e.stack || e.message);
    if (!res.headersSent) json(res, status, { error: e.message, code: e.code });
    else try { res.end(); } catch {}
  });
});
server.keepAliveTimeout = 65_000;
server.requestTimeout = 0; // server-sent events stay open

await loadApps();
server.listen(PORT, () => console.log(`[hub] listening on :${PORT}; ChloeDB at ${db.BASE}; apps: ${[...apps.keys()].join(', ') || 'none'}`));
for (const sig of ['SIGTERM', 'SIGINT']) process.on(sig, () => { server.close(); process.exit(0); });
