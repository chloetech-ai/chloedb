// What apps change next, learned across the apps of an engine. Every change
// someone asks for is sorted into kinds (a chart, a search, a reminder...),
// and the kinds go into a graph in Strata (sa_changes, an EDGES column):
//
//   app:<id>     did  -> change:<kind>   once per change of that kind
//   change:<a>   then -> change:<b>      once per time b followed a in an app
//
// The next change an app may want is ranked by personalized PageRank from
// the app and its latest kinds, both ways along the edges: from the kinds it
// did, to the other apps that did them, to what those apps did, and along
// what followed what. Kinds the app did already are left out.
//
// The same kinds say which suggestions people take: the share of each kind
// taken when it was offered (takeRates), across every app.

export const MIN_APPS = Number(process.env.STRATA_APPS_NEXT_MIN_APPS || 10);

export const CHANGES_SQL = `CREATE TABLE sa_changes (id KEYWORD PRIMARY KEY, kind KEYWORD NOT NULL, app KEYWORD, title TEXT, n INT, links EDGES)`;

// The kinds, each with the words that name it in a request. A request may be
// of up to three kinds; one of none is left out.
export const CHANGE_KINDS = [
  { id: 'chart', title: 'a chart or a dashboard', re: /\b(chart|graph|plot|dashboard|sparkline|trend|heat ?map|gantt|timeline)\b/i },
  { id: 'totals', title: 'totals, counts or averages', re: /\b(totals?|sums?|counts?|number of|average|how (much|many)|balance|left|owed?|stats?|statistics|per (month|week|category|client|genre)|kpis?)\b/i },
  { id: 'search', title: 'a search', re: /\b(search|find|look ?up|suggest (tags|as I type)|similar)\b/i },
  { id: 'filter', title: 'a filter', re: /\b(filter|only show|show only|by (status|genre|category|tag|type|priority))\b/i },
  { id: 'sort', title: 'a sort order', re: /\b(sort|order (it |them )?by|newest|oldest|most urgent|first)\b/i },
  { id: 'look', title: 'names, colors or layout', re: /\b(rename|call (it|them)|title|colou?rs?|red|green|orange|blue|grey|gray|bigger|smaller|bold|hide|move|layout|wording)\b/i },
  { id: 'status', title: 'a status or progress', re: /\b(status|mark (it |one |them )?(as )?(paid|done|read|complete)|tick|progress|stages?|board|mastered|archive)\b/i },
  { id: 'dates', title: 'dates, due dates or a calendar', re: /\b(due|deadlines?|overdue|countdown|calendar|week view|schedule|today|this (week|month)|date|opening hours|open now)\b/i },
  { id: 'notify', title: 'a reminder or an email', re: /\b(remind(er)?s?|e-?mails?|digest|notif(y|ication)|alerts?|send)\b/i },
  { id: 'export', title: 'an export, a document or an invoice', re: /\b(export|csv|download|print|pdf|invoices?|receipts?|purchase orders?|report)\b/i },
  { id: 'import', title: 'an import', re: /\b(import|upload|paste|bulk add)\b/i },
  { id: 'people', title: 'people, sharing or splitting', re: /\b(share|invite|team|family|who owes|settle|split|assign|everyone|each (kid|person|player|member)|leaderboard|guests?)\b/i },
  { id: 'field', title: 'a new field on the rows', re: /\badd (a |an )?\w+( \w+)? (field|column)\b|\b(to each|on each|with (a |an )?(rating|notes?|priority|photo|genre|tags?))\b/i },
  { id: 'relations', title: 'rows linked to other rows', re: /\b(link|related|together|list per|per (flat|deck|client|project)|for each \w+|suppliers?|contacts?)\b/i },
  { id: 'rules', title: 'a rule that checks or changes rows', re: /\b(never|cannot|can't|must|takes? (the )?stock down|limit|validate|balanced|only (one|once)|automatically|when (an?|the) \w+ (is|gets))\b/i },
  { id: 'compute', title: 'a computed number', re: /\b(predict(ed|ion)?|zones?|amortization|loan|mortgage|tax|pace|score|points|allowance|formula|estimate|interest|bmi|grade)\b/i },
  { id: 'favorite', title: 'favorites or pins', re: /\b(favou?rites?|star(red)?|pin(ned)?|bookmark)\b/i },
  { id: 'media', title: 'photos, files or a map', re: /\b(photos?|pictures?|images?|attach(ments?)?|files?|maps?|locations?)\b/i },
  { id: 'commerce', title: 'a cart, orders or payments', re: /\b(cart|checkout|orders?|buy|pay(ment)?s?|price|discount)\b/i },
  { id: 'quiz', title: 'a quiz, a game or random picks', re: /\b(quiz|game|random(ly)?|shuffle|flip|scrimmage)\b/i },
  { id: 'edit', title: 'editing or deleting rows', re: /\b(edit|delete|remove (a|an|one)|undo|change (a|an|the) \w+ after)\b/i },
];
const BY_ID = new Map(CHANGE_KINDS.map((k) => [k.id, k]));
export const kindTitle = (id) => BY_ID.get(id)?.title || id;

// The kinds a request (or a suggestion) is of, the ones named first first.
export function kindsOf(text, max = 3) {
  const t = String(text || '');
  return CHANGE_KINDS.map((k) => ({ id: k.id, at: t.search(k.re) })).filter((x) => x.at >= 0).sort((a, b) => a.at - b.at).slice(0, max).map((x) => x.id);
}

// Each app's changes in order: its requests but the first (which says what
// the app is) and repairs, the ones that were built.
export async function sequences(store, limit = 5000) {
  const rows = await store.db.query(`SELECT app, text, source, status, created FROM sa_requests WHERE status = 'done' ORDER BY created LIMIT ${Number(limit)}`);
  const by = new Map();
  for (const r of rows) {
    if (!by.has(r.app)) by.set(r.app, []);
    by.get(r.app).push(r);
  }
  const out = new Map();
  for (const [app, list] of by) {
    const changes = list.slice(1).filter((r) => r.source !== 'repair' && r.source !== 'look').map((r) => kindsOf(r.text)).filter((k) => k.length);
    if (changes.length) out.set(app, changes);
  }
  return out;
}

// The graph's rows from the sequences.
export function changeDocs(seqs) {
  const kinds = new Map(CHANGE_KINDS.map((k) => [k.id, { links: [], n: 0 }]));
  const docs = [];
  for (const [app, changes] of seqs) {
    const did = [];
    changes.forEach((ks, i) => {
      for (const k of ks) { did.push(['did', `change:${k}`]); kinds.get(k).n++; }
      const next = changes[i + 1];
      if (next) for (const a of ks) for (const b of next) if (a !== b) kinds.get(a).links.push(['then', `change:${b}`]);
    });
    docs.push({ id: `app:${app}`, kind: 'app', app, title: app, n: changes.length, links: did });
  }
  for (const [id, k] of kinds) docs.push({ id: `change:${id}`, kind: 'change', title: kindTitle(id), n: k.n, links: k.links });
  return docs;
}

export class Changes {
  constructor(store, log = () => {}) {
    this.store = store;
    this.db = store.db;
    this.log = log;
    this.apps = 0;
    this.timer = null;
  }

  async init() {
    const have = new Set((await this.db.tables()).map((t) => (typeof t === 'string' ? t : t.name || t.schema?.table)));
    if (!have.has('sa_changes')) await this.db.sql(CHANGES_SQL);
    await this.refresh();
  }

  // Writes the graph again from every app's requests (a few thousand rows).
  async refresh() {
    const seqs = await sequences(this.store);
    const docs = changeDocs(seqs);
    const old = await this.db.query('SELECT id FROM sa_changes LIMIT 10000').catch(() => []);
    const keep = new Set(docs.map((d) => d.id));
    await this.db.write([...docs.map((doc) => ({ op: 'upsert', table: 'sa_changes', doc })), ...old.filter((r) => !keep.has(r.id)).map((r) => ({ op: 'delete', table: 'sa_changes', key: r.id }))]);
    this.apps = seqs.size;
    this.seqs = seqs;
  }

  // After a change is built; several builds in a row write the graph once.
  soon() {
    clearTimeout(this.timer);
    this.timer = setTimeout(() => this.refresh().catch((e) => this.log(`changes: ${e.message}`)), 5000);
    this.timer.unref?.();
  }

  // The kinds of change this app may want next: [{kind, title, score, apps}],
  // apps being how many other apps made a change of that kind. Empty while
  // fewer than MIN_APPS other apps have changes to learn from: over 19 apps it
  // guessed the next change in its top 3 for 7 of 14, where "what most apps
  // added" guessed 9 (test/next-change-eval.mjs measures it again).
  async next(appId, { limit = 3, least = MIN_APPS } = {}) {
    const seqs = this.seqs || await sequences(this.store);
    const others = [...seqs.keys()].filter((a) => a !== appId);
    if (others.length < least) return [];
    const mine = seqs.get(appId) || [];
    const done = new Set(mine.flat());
    const seeds = [`app:${appId}`, ...(mine.at(-1) || []).map((k) => `change:${k}`)];
    if (!mine.length) seeds.shift();
    if (!seeds.length) return [];
    const rows = await this.store.walk(`SELECT id, title, score() AS score FROM sa_changes WHERE kind = 'change' ORDER BY ppr(links, :s) DESC LIMIT 30`, { s: seeds },
      'sa_changes', 'id, kind, title, links', (w) => w.byScore(w.ppr(seeds), (r) => r.kind === 'change', 30));
    const apps = (k) => others.filter((a) => seqs.get(a).some((ks) => ks.includes(k))).length;
    return rows.map((r) => ({ kind: r.id.slice('change:'.length), title: r.title, score: Number(r.score) }))
      .filter((r) => r.score > 0 && !done.has(r.kind))
      .slice(0, limit)
      .map((r) => ({ ...r, apps: apps(r.kind) }))
      .filter((r) => r.apps > 0);
  }

  // How often each kind of suggestion was taken when offered, across apps:
  // {kind: {offered, taken, rate}}. A suggestion counts once it was decided
  // (taken, dismissed, or left when the next version came: stale).
  async takeRates() {
    const rows = await this.db.query(`SELECT text, status FROM sa_suggestions WHERE status IN ('taken', 'dismissed', 'stale') LIMIT 10000`).catch(() => []);
    const by = {};
    for (const r of rows) {
      for (const k of kindsOf(r.text, 2)) {
        const x = by[k] || (by[k] = { offered: 0, taken: 0 });
        x.offered++;
        if (r.status === 'taken') x.taken++;
      }
    }
    for (const x of Object.values(by)) x.rate = x.taken / x.offered;
    return by;
  }

  // The part of a suggestion prompt on both: what apps like this one changed
  // next, and which kinds of suggestion people take.
  async promptPart(appId, opts = {}) {
    const [next, rates] = await Promise.all([this.next(appId, opts).catch(() => []), this.takeRates().catch(() => ({}))]);
    const parts = [];
    if (next.length) parts.push(`## What apps like this one changed next (learned from ${this.apps} apps here)\n${next.map((n) => `- ${n.title} (${n.apps} other app${n.apps === 1 ? '' : 's'} added one)`).join('\n')}\nOffer one of these when it fits this app.`);
    const decided = Object.entries(rates).filter(([, x]) => x.offered >= 4).sort((a, b) => b[1].rate - a[1].rate);
    if (decided.length >= 2) {
      const line = ([k, x]) => `${kindTitle(k)}: ${x.taken} of ${x.offered}`;
      const taken = decided.filter(([, x]) => x.rate >= 0.25).slice(0, 4);
      const left = decided.filter(([, x]) => x.rate < 0.1).slice(-4);
      if (taken.length || left.length) parts.push(`## Suggestions people took, by kind (all apps here)\n${taken.length ? `Taken most: ${taken.map(line).join('; ')}.` : ''}${left.length ? `${taken.length ? '\n' : ''}Rarely taken: ${left.map(line).join('; ')}.` : ''}\nPrefer the kinds people take.`);
    }
    return parts.join('\n\n');
  }

  // Suggestions in the order people are likely to take them: a kind rarely
  // taken (under one in ten of at least five offered) goes last.
  async rank(items) {
    const rates = await this.takeRates().catch(() => ({}));
    const low = (s) => kindsOf(s.text, 1).some((k) => rates[k] && rates[k].offered >= 5 && rates[k].rate < 0.1);
    return [...items.filter((s) => !low(s)), ...items.filter(low)];
  }
}

// Asks that waited for the same build, as one request: built together as
// one design rather than one change on top of another.
export function joinAsks(asks) {
  if (asks.length === 1) return asks[0];
  return `Make these ${asks.length} changes together, as one design: fold them into the screens that are there rather than adding a part for each, and keep the first screen for the job.\n${asks.map((a, i) => `${i + 1}. ${a}`).join('\n')}`.slice(0, 4000);
}
