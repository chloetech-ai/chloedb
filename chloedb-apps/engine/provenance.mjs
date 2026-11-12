// Where an app's code came from: how much of it the engine generated from a
// CREATE statement the agent wrote (CREATE TABLE, CREATE SCREEN, CREATE
// RULE), how much the agent wrote from scratch (JavaScript, or a screen or
// rule written out as JSON), how much was reused from the library as it is,
// and how much a person edited by hand.
//
// The measure is lines, blank lines left out, counted twice for each piece:
//
// - written: the lines its author wrote (a statement, or the code);
// - runs: the lines of the form the engine runs (a screen's JSON with its
//   patterns expanded, a rule's steps, the code of a JavaScript piece),
//   each JSON form printed the same way (two-space indent) so pieces
//   compare.
//
// Any other piece stored as a statement of its own (CREATE SOURCE, a rule
// that runs on a schedule, CREATE RULE ... EVERY) counts as declared too.
//
// So for a CREATE SCREEN of 12 lines that expands into 140 lines of screen,
// written is 12 and runs is 140: the engine generated 128. The app's share
// of each origin is its share of the lines that run; "leverage" is the lines
// that run per line written, for the pieces written as statements.

import { SHARED_KINDS } from './store.mjs';

export const ORIGINS = {
  create: 'Generated from CREATE statements',
  scratch: 'Written from scratch by the agent',
  reused: 'Reused from the library',
  hand: 'Edited by hand',
};

const lines = (s) => String(s || '').split('\n').filter((l) => l.trim()).length;
const pretty = (code) => { try { return JSON.stringify(JSON.parse(code), null, 2); } catch { return String(code || ''); } };
const withoutPhotos = (s) => String(s || '').replace(/\n?^PHOTOS\s+\[[\s\S]*?^\](?=\s*$|\n)/m, '').trim();

// One piece: {origin, how, wrote, runs, language}.
export function pieceOrigin(c, appId) {
  const spec = c.spec || {};
  const shared = SHARED_KINDS.includes(c.kind);
  const fork = c.forked_from ? String(c.forked_from).replace(/@\d+$/, '') : null;
  const repaired = c.author === 'repair' ? ', repaired by the engine after it failed' : '';
  const js = { wrote: lines(c.code), runs: lines(c.code), language: 'javascript' };
  // Not written for this app: copied from the library, built in, or a shared
  // piece another app wrote.
  if (c.author === 'builtin') return { origin: 'reused', how: shared ? `built-in shared ${c.kind}, used as it is` : 'built in, used as it is', ...js, wrote: 0, language: shared ? 'javascript' : c.kind };
  if (c.author === 'library') return { origin: 'reused', how: fork?.startsWith('lib/') ? 'built-in starter, copied as it is' : `copied as it is from ${fork || 'another app'}`, ...measure(c), wrote: 0 };
  if (shared && spec.by && spec.by !== appId) return { origin: 'reused', how: `shared ${c.kind} another app wrote, used as it is`, ...js, wrote: 0 };
  if (c.author === 'person') return { origin: 'hand', how: 'edited by hand in the studio', ...measure(c) };
  const m = measure(c);
  if (m.statement) {
    const patterns = spec.patterns?.length ? ` from the pattern${spec.patterns.length > 1 ? 's' : ''} ${spec.patterns.join(', ')}` : '';
    return { origin: 'create', how: `${m.statement}${patterns}${repaired}`, ...m };
  }
  const forked = fork ? `, forked from ${fork}` : '';
  const what = m.language === 'json' ? `${c.kind === 'view' ? 'screen' : 'rule'} written out as JSON` : shared ? `shared ${c.kind} written in JavaScript for this app` : `${c.kind === 'view' ? 'view' : 'function'} written in JavaScript`;
  return { origin: 'scratch', how: `${what}${forked}${repaired}`, ...m };
}

// The lines written and run of a piece, and the statement it was written as.
function measure(c) {
  const spec = c.spec || {};
  if (c.kind === 'table') {
    const n = lines(c.sql || c.code);
    return { statement: 'CREATE TABLE', wrote: n, runs: n, language: 'sql' };
  }
  if (c.kind === 'view' && spec.screen) {
    const runs = lines(pretty(c.code));
    if (typeof spec.statement === 'string' && !spec.derived) return { statement: 'CREATE SCREEN', wrote: lines(withoutPhotos(spec.statement)), runs, language: 'sql' };
    return { statement: null, wrote: runs, runs, language: 'json' };
  }
  if (c.kind === 'function' && spec.rule) {
    const runs = lines(pretty(c.code));
    if (typeof spec.statement === 'string' && !spec.derived) return { statement: 'CREATE RULE', wrote: lines(spec.statement), runs, language: 'sql' };
    return { statement: null, wrote: runs, runs, language: 'json' };
  }
  // Any other piece stored as a statement of its own (CREATE SOURCE, and
  // what comes after it), not JavaScript or JSON wrapped in a CREATE.
  const sql = String(c.sql || '');
  const head = sql.match(STATEMENT);
  if (head && !WRAPPED.test(sql) && !spec.derived) return { statement: `CREATE ${head[1].toUpperCase()}`, wrote: lines(spec.statement || sql), runs: lines(c.code ? pretty(c.code) : sql), language: 'sql' };
  return { statement: null, wrote: lines(c.code), runs: lines(c.code), language: c.kind === 'pattern' ? 'json' : 'javascript' };
}
const STATEMENT = /^\s*(?:--[^\n]*\n\s*)*CREATE\s+(?:OR\s+REPLACE\s+)?([A-Z]+)\b/i;
const WRAPPED = /\bLANGUAGE\s+(?:javascript|json)\s+AS\s+\$[A-Za-z0-9_]*\$/i;

// The whole app (a resolved view of one version): totals by origin and the
// pieces, the biggest first.
export function provenance(view, appId = view.app?.id) {
  const caps = [...(view.tables || []), ...(view.views || []), ...(view.functions || []), ...(view.components || []), ...(view.helpers || [])];
  const pieces = caps.map((c) => {
    const o = pieceOrigin(c, appId);
    const name = c.kind === 'table' ? c.spec?.name || c.slug : c.slug;
    return { id: c.id, kind: c.kind, slug: c.slug, name, title: c.title || c.slug, version: Number(c.version), author: c.author || null,
      origin: o.origin, how: o.how, statement: o.statement || null, language: o.language, wrote: o.wrote, runs: o.runs, generated: Math.max(0, o.runs - o.wrote) };
  });
  const totals = Object.fromEntries(Object.keys(ORIGINS).map((k) => [k, { label: ORIGINS[k], pieces: 0, wrote: 0, runs: 0 }]));
  for (const p of pieces) { const t = totals[p.origin]; t.pieces++; t.wrote += p.wrote; t.runs += p.runs; }
  const runs = pieces.reduce((s, p) => s + p.runs, 0);
  for (const t of Object.values(totals)) t.share = runs ? t.runs / runs : 0;
  const c = totals.create;
  const order = { create: 0, scratch: 1, hand: 2, reused: 3 };
  pieces.sort((a, b) => order[a.origin] - order[b.origin] || b.runs - a.runs || a.name.localeCompare(b.name));
  return {
    version: view.comp?.n ?? null,
    runs,
    wrote: pieces.reduce((s, p) => s + p.wrote, 0),
    // The agent's own lines: its statements and its code.
    agent: { wrote: c.wrote + totals.scratch.wrote, create: c.wrote, scratch: totals.scratch.wrote },
    leverage: c.wrote ? c.runs / c.wrote : null,
    totals,
    pieces,
  };
}
