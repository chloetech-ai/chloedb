// The built-in library: components and helpers written by hand, shipped in
// strata-apps/library/ and stored in Strata at start-up like any other shared
// piece, so the forge finds them and apps pin their versions.
//
// Each file is the piece's code, after a header of `// key: value` lines
// (title, description, and props for a component or api for a helper; a line
// starting `//   ` continues the value above it).

import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { LIB, COMPONENT_RE } from './store.mjs';

export function parsePiece(text) {
  const lines = text.split('\n');
  const meta = {};
  let key = null;
  let i = 0;
  for (; i < lines.length; i++) {
    const line = lines[i];
    const m = line.match(/^\/\/ (title|description|props|api): ?(.*)$/);
    if (m) { key = m[1]; meta[key] = m[2].trim(); continue; }
    const more = line.match(/^\/\/ {2,}(.*)$/);
    if (more && key) { meta[key] += `\n${more[1].trim()}`; continue; }
    if (/^\/\/\s*$/.test(line) && key) continue;
    break;
  }
  return { meta, code: lines.slice(i).join('\n').trim() + '\n' };
}

export async function readLibrary(dir) {
  const out = [];
  for (const [kind, sub] of [['component', 'components'], ['helper', 'helpers']]) {
    let files = [];
    try { files = (await readdir(path.join(dir, sub))).filter((f) => f.endsWith('.js')).sort(); } catch { continue; }
    for (const f of files) {
      const name = f.slice(0, -3);
      if (!COMPONENT_RE.test(name)) throw new Error(`library/${sub}/${f}: the name must be camelCase`);
      const { meta, code } = parsePiece(await readFile(path.join(dir, sub, f), 'utf8'));
      out.push({ kind, name, meta, code });
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
  let added = 0;
  let updated = 0;
  for (const p of pieces) {
    const cur = shared.get(`${p.kind}/${p.name}`);
    if (cur && cur.author !== 'builtin') { log(`built-in ${p.kind} ${p.name} skipped: the library has one by that name from an app`); continue; }
    if (cur) {
      const [full] = await store.getCapabilities([cur.id]);
      if (full?.code === p.code) continue;
    }
    await store.putCapability({ app: LIB, slug: p.name, kind: p.kind, title: p.meta.title || p.name, description: p.meta.description || '',
      code: p.code, spec: { props: p.meta.props || p.meta.api || '', change: cur ? 'the built-in library was updated' : '' }, author: 'builtin' });
    if (cur) updated++; else added++;
  }
  if (added || updated) log(`built-in library: ${added} added, ${updated} updated (${pieces.length} in all)`);
  return { added, updated, total: pieces.length };
}
