// The built-in library, shipped in chloedb-apps/library/ and stored in Strata
// at start-up:
// - components/ and helpers/: shared pieces, like the ones apps write, so the
//   forge finds them and apps pin their versions;
// - views/ and functions/: starters, whole screens and server functions an app
//   copies as they are (with the tables a view's header names), found by the
//   forge like another app's views and functions.
//
// - patterns/: screen patterns (engine/patterns.mjs), one JSON file each,
//   stored as rows of kind "pattern" that the forge expands from; apps may
//   add more, as new rows, and a changed file is a new version.
//
// Each file is the piece's code, after a header of `// key: value` lines
// (title and description; props for a component, api for a helper, tables
// for a view, input for a function; a line starting `//   ` continues the
// value above it).

import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { LIB, COMPONENT_RE, SLUG_RE, NAME_RE, TYPES } from './store.mjs';

export function parsePiece(text) {
  const lines = text.split('\n');
  const meta = {};
  let key = null;
  let i = 0;
  for (; i < lines.length; i++) {
    const line = lines[i];
    const m = line.match(/^\/\/ (title|description|props|api|tables|input): ?(.*)$/);
    if (m) { key = m[1]; meta[key] = m[2].trim(); continue; }
    const more = line.match(/^\/\/ {2,}(.*)$/);
    if (more && key) { meta[key] += `\n${more[1].trim()}`; continue; }
    if (/^\/\/\s*$/.test(line) && key) continue;
    break;
  }
  return { meta, code: lines.slice(i).join('\n').trim() + '\n' };
}

// 'notes(title text, tags keyword[]); note_links(note keyword, url text)' ->
// [{name, columns: [{name, type}]}]
export function parseTables(text) {
  const out = [];
  for (const part of String(text || '').split(';')) {
    const m = part.trim().match(/^([a-z][a-z0-9_]*)\s*\(([\s\S]*)\)$/);
    if (!m) { if (part.trim()) throw new Error(`cannot read the table ${JSON.stringify(part.trim())}: write name(column type, ...)`); continue; }
    const columns = m[2].split(',').map((c) => c.trim()).filter(Boolean).map((c) => {
      const [name, type] = c.split(/\s+/);
      if (!NAME_RE.test(name || '') || !TYPES[type]) throw new Error(`table ${m[1]}: column ${JSON.stringify(c)} is not "name type" with one of ${Object.keys(TYPES).join(', ')}`);
      return { name, type };
    });
    out.push({ name: m[1], columns });
  }
  return out;
}

const FOLDERS = [['component', 'components', COMPONENT_RE, 'camelCase'], ['helper', 'helpers', COMPONENT_RE, 'camelCase'],
  ['view', 'views', SLUG_RE, 'lowercase with dashes'], ['function', 'functions', SLUG_RE, 'lowercase with dashes']];

export async function readLibrary(dir) {
  const out = [];
  let patterns = [];
  try { patterns = (await readdir(path.join(dir, 'patterns'))).filter((f) => f.endsWith('.json')).sort(); } catch { /* none */ }
  for (const f of patterns) {
    const name = f.slice(0, -5);
    if (!SLUG_RE.test(name)) throw new Error(`library/patterns/${f}: the name must be lowercase with dashes`);
    const p = JSON.parse(await readFile(path.join(dir, 'patterns', f), 'utf8'));
    out.push({ kind: 'pattern', name, meta: { title: p.title || name, description: p.about || '' }, code: `${JSON.stringify(p, null, 1)}\n` });
  }
  for (const [kind, sub, re, how] of FOLDERS) {
    let files = [];
    try { files = (await readdir(path.join(dir, sub))).filter((f) => f.endsWith('.js')).sort(); } catch { continue; }
    for (const f of files) {
      const name = f.slice(0, -3);
      if (!re.test(name)) throw new Error(`library/${sub}/${f}: the name must be ${how}`);
      const { meta, code } = parsePiece(await readFile(path.join(dir, sub, f), 'utf8'));
      const piece = { kind, name, meta, code };
      if (kind === 'view') piece.tables = parseTables(meta.tables);
      out.push(piece);
    }
  }
  return out;
}

// Stores each built-in piece that the library lacks, and a new version of one
// whose shipped code changed. A piece of the same name that an app's build
// wrote is left alone.
export async function seedLibrary(store, dir, log = () => {}) {
  const pieces = await readLibrary(dir);
  const shared = await store.shared();
  const starters = await store.starters();
  const patterns = await store.patterns();
  let added = 0;
  let updated = 0;
  for (const p of pieces) {
    const starter = p.kind === 'view' || p.kind === 'function';
    const cur = p.kind === 'pattern' ? patterns.get(p.name) : (starter ? starters : shared).get(`${p.kind}/${p.name}`);
    if (cur && cur.author !== 'builtin') { log(`built-in ${p.kind} ${p.name} skipped: the library has one by that name from an app`); continue; }
    if (cur) {
      const [full] = await store.getCapabilities([cur.id]);
      if (full?.code === p.code && (!starter || JSON.stringify(full.spec?.tables || []) === JSON.stringify(p.tables || []))) continue;
    }
    const spec = p.kind === 'pattern' ? { params: Object.keys(JSON.parse(p.code).params || {}), change: cur ? 'the built-in library was updated' : '' }
      : starter ? { tables: p.tables || [], input: p.meta.input || '', starter: true }
        : { props: p.meta.props || p.meta.api || '', change: cur ? 'the built-in library was updated' : '' };
    await store.putCapability({ app: LIB, slug: p.name, kind: p.kind, title: p.meta.title || p.name, description: p.meta.description || '',
      code: p.code, spec, author: 'builtin' });
    if (cur) updated++; else added++;
  }
  if (added || updated) log(`built-in library: ${added} added, ${updated} updated (${pieces.length} in all)`);
  return { added, updated, total: pieces.length };
}
