// How well the change graph (engine/changes.mjs) predicts an app's next
// change, on the requests of one or more engines' Strata:
//
//   node test/next-change-eval.mjs http://127.0.0.1:7961 [more Strata URLs] --scratch http://127.0.0.1:7991
//
// For each app with at least two changes, its last change is hidden; the
// graph is built from every other app (apps with the same first request, the
// same app in another engine, are left out too) and this app's earlier
// changes, in a scratch Strata (its sa_changes table is written over). A hit
// is the hidden change's kinds in the top 3. Two baselines: the kinds most
// apps made, and the kinds that most often followed the app's last ones.
import { Strata } from '../engine/strata.mjs';
import { kindsOf, changeDocs, CHANGES_SQL } from '../engine/changes.mjs';

const args = process.argv.slice(2);
const at = args.indexOf('--scratch');
const scratch = at >= 0 ? args.splice(at, 2)[1] : null;
if (!args.length || !scratch) { console.error('usage: next-change-eval.mjs STRATA_URL... --scratch SCRATCH_STRATA_URL'); process.exit(2); }

const apps = [];
for (const url of args) {
  const rows = await new Strata(url).query("SELECT app, text, source, status, created FROM sa_requests WHERE status = 'done' ORDER BY created LIMIT 5000");
  const by = new Map();
  for (const r of rows) { if (!by.has(r.app)) by.set(r.app, []); by.get(r.app).push(r); }
  for (const [app, list] of by) {
    const changes = list.slice(1).filter((r) => r.source !== 'repair').map((r) => kindsOf(r.text)).filter((k) => k.length);
    apps.push({ key: `${url}#${app}`, first: list[0].text.slice(0, 80), changes });
  }
}
const db = new Strata(scratch);
const top3 = (scores, done) => [...scores].sort((x, y) => y[1] - x[1]).map(([k]) => k).filter((k) => !done.has(k)).slice(0, 3);
const hits = { graph: 0, made: 0, followed: 0 };
let folds = 0;
const seen = new Set();
for (const a of apps.filter((x) => x.changes.length >= 2)) {
  if (seen.has(a.first)) continue;
  seen.add(a.first);
  const hidden = a.changes.at(-1);
  const mine = a.changes.slice(0, -1);
  const train = new Map(apps.filter((x) => x.first !== a.first && x.changes.length).map((x) => [x.key, x.changes]));
  train.set(a.key, mine);
  try { await db.sql('DROP TABLE sa_changes'); } catch { /* none yet */ }
  await db.sql(CHANGES_SQL);
  await db.write(changeDocs(train).map((doc) => ({ op: 'upsert', table: 'sa_changes', doc })));
  const done = new Set(mine.flat());
  const rows = await db.query("SELECT id, score() AS score FROM sa_changes WHERE kind = 'change' ORDER BY ppr(links, :s) DESC LIMIT 30", { s: [`app:${a.key}`, ...mine.at(-1).map((k) => `change:${k}`)] });
  const graph = rows.map((r) => r.id.slice('change:'.length)).filter((k) => !done.has(k)).slice(0, 3);
  const made = new Map();
  for (const ch of train.values()) for (const k of new Set(ch.flat())) made.set(k, (made.get(k) || 0) + 1);
  const followed = new Map();
  for (const ch of train.values()) ch.forEach((ks, i) => { if (ch[i + 1] && ks.some((k) => mine.at(-1).includes(k))) for (const b of ch[i + 1]) followed.set(b, (followed.get(b) || 0) + 1); });
  const lists = { graph, made: top3(made, done), followed: top3(followed, done) };
  const hit = (list) => hidden.some((k) => list.includes(k));
  for (const [k, list] of Object.entries(lists)) if (hit(list)) hits[k]++;
  folds++;
  console.log(`${a.first.slice(0, 40).padEnd(40)} next ${hidden.join(',').padEnd(24)} graph ${graph.join(',').padEnd(26)} ${hit(graph) ? 'hit' : '   '}  most made ${lists.made.join(',')}`);
}
console.log(`\n${apps.length} apps, ${seen.size} distinct; hits in the top 3 over ${folds} apps: graph ${hits.graph}, most made ${hits.made}, most followed ${hits.followed}`);
