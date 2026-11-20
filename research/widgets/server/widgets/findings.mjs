// What Claude found: not a source of its own, but what the Claude of the
// conversation gathered itself (pages from a web search, pages it read,
// files, another tool's results), shaped to what the person wants and
// handed to an app to show. For "the best headphones to buy" that is not a
// list of review pages but the headphones themselves: the bottom line,
// which one for which need, each one's facts and what each source says
// about it, with the pages behind every claim. A question gets its answer
// with the points behind it, a how-to its steps, news what happened when.
//
// show_findings (apps.mjs) takes this from the model and opens the app that
// shows it; an app whose first tool is `findings` is filled with it, and
// app_open and app_make take it as that tool's arguments. This only checks
// and tidies it.
import { z } from 'zod'
import { picturesFor } from '../pictures.mjs'

const FACT = z.union([z.string().max(200), z.number(), z.boolean(), z.null()])
const CITES = z.array(z.number().int().min(1)).max(8)

const DOCUMENT = z.object({
  title: z.string().min(1).max(300).describe('The page’s or document’s title'),
  url: z.string().max(2000).optional().describe('Where it is (https://...), when it has an address'),
  source: z.string().max(120).optional().describe('Who published it: a site, an author, a file'),
  date: z.string().max(40).optional().describe('When it was published or last changed, as written (2026-11-18, "Nov 2026"); left out when unknown'),
  kind: z.string().max(40).optional().describe('What it is: review, article, test, paper, docs, forum, listing, shop...'),
  summary: z.string().max(2000).describe('What it says that matters for the request, in your words, 1 to 3 sentences'),
  quotes: z.array(z.string().max(600)).max(5).optional().describe('Up to 5 passages quoted exactly'),
  facts: z.record(z.string().max(60), FACT).optional().describe('Values it gives about itself, when it is one of the things compared'),
})

const ITEM = z.object({
  name: z.string().min(1).max(120).describe('The thing itself: a product, a place, an option, a step, an event or a point ("Sony WH-1000XM6", "Rubicon Trail")'),
  about: z.string().max(400).optional().describe('What it is, in a line'),
  verdict: z.string().max(300).optional().describe('What the sources make of it, in a sentence: who it is for, and the catch'),
  facts: z.record(z.string().max(60), FACT).optional().describe('Its values for the criteria, under the same names as the other items ({"price": 449, "battery": "30 h", "noise cancelling": "best tested"})'),
  pros: z.array(z.string().max(160)).max(5).optional().describe('Its strengths, a few words each'),
  cons: z.array(z.string().max(160)).max(5).optional().describe('Its weaknesses, a few words each'),
  says: z.array(z.object({ doc: z.number().int().min(1).describe('The document’s number'), text: z.string().min(1).max(400).describe('What it says, quoted or close') })).max(8).optional()
    .describe('What each source says about it'),
  when: z.string().max(40).optional().describe('For news and history: when it happened'),
  group: z.string().max(60).optional().describe('For things put together from parts (an outfit, a room, a menu): the part it is ("top", "bottom", "shoes", "accessory")'),
  url: z.string().max(2000).optional().describe('Its own page (the maker’s, the place’s), when there is one'),
  image: z.string().max(2000).optional().describe('The address of a photo of it that you saw on a page you read (the thing itself, not a logo or a person), when there is one'),
})

const GOALS = ['pick', 'answer', 'overview', 'how-to', 'news']

export const INPUT = {
  query: z.string().min(1).max(300).describe('What was looked for, in a few words'),
  goal: z.enum(GOALS).optional().describe('What the person wants: pick one of several things (to buy, visit, use), an answer to a question, an overview of a subject, how to do something, or what happened'),
  answer: z.object({ text: z.string().min(1).max(800), cites: CITES.optional() }).optional()
    .describe('The bottom line for the person, in 1 to 3 sentences, as a friend who read everything would say it, citing documents by number'),
  needs: z.array(z.object({
    need: z.string().min(1).max(80).describe('A kind of need, as the person might put it ("most people", "long flights", "under $200")'),
    pick: z.string().min(1).max(120).describe('The item’s name, as in items'),
    why: z.string().max(240).optional(),
    cites: CITES.optional(),
  })).max(6).optional().describe('For a pick: the best item for each kind of need'),
  criteria: z.array(z.string().min(1).max(60)).max(8).optional().describe('The facts that matter most for this request, most important first, named as in the items’ facts'),
  items: z.array(ITEM).max(20).optional()
    .describe('The things the request is about, best first: the options to pick from, the points of an answer, the steps of a how-to, the events of the news'),
  documents: z.array(DOCUMENT).max(40).describe('The sources, best first; they are numbered from 1 in this order, and doc and cites use those numbers'),
  notes: z.string().max(1000).optional().describe('What the sources disagree on or leave out, and what to check, in a sentence or two'),
}

// Only web addresses are kept: a view links to them.
const web = url => {
  try {
    const u = new URL(url)
    return u.protocol === 'https:' || u.protocol === 'http:' ? u.href : null
  } catch {
    return null
  }
}

const hostOf = url => {
  try {
    return new URL(url).hostname.replace(/^www\./, '')
  } catch {
    return null
  }
}

const SHAPE = 'Give the request in "query"; "goal" (pick, answer, overview, how-to or news); "answer" {text, cites}, the bottom line citing documents by number; ' +
  'for a pick, "items" (the products, places or options themselves, best first, each with name, verdict, facts under the same names across items, pros, cons, ' +
  'and says: [{doc, text}] for what each source says about it, and image, the address of a photo of it seen on a page you read), "needs" [{need, pick, why}] (the best item for each kind of need) and "criteria" (the facts that ' +
  'matter, most important first); for an answer or an overview, items are its points; for a how-to, its steps; for news, its events with "when"; and "documents", ' +
  'the sources numbered from 1 (title, url, source, date, kind, a summary of what each says for this request, quotes passed exactly), with "notes" on what they ' +
  'disagree on or leave out.'

export const findings = {
  name: 'findings',
  title: 'What Claude found',
  view: null,
  given: true,
  domains: [],
  description:
    'Not a source: what you found yourself (pages from a web search, pages or files you read, another tool’s results), shaped to what the person wants and ' +
    'handed to an app to show. Answer the request with it, not with a list of pages: for "the best headphones to buy", the headphones compared, which one for ' +
    `which need, and what each source says about each. ${SHAPE}`,
  shape: SHAPE,
  input: INPUT,
  example: {
    query: 'best noise cancelling headphones 2026',
    goal: 'pick',
    answer: { text: 'Get the Sony WH-1000XM6 unless you fly a lot: then the Bose QC Ultra 2 are quieter and more comfortable for hours.', cites: [1, 2] },
    needs: [
      { need: 'most people', pick: 'Sony WH-1000XM6', why: 'The best all-round in both tests', cites: [1, 2] },
      { need: 'long flights', pick: 'Bose QC Ultra 2', why: 'Most comfortable, cancels engine rumble best', cites: [2] },
    ],
    criteria: ['price', 'noise cancelling', 'battery', 'comfort'],
    items: [
      {
        name: 'Sony WH-1000XM6',
        verdict: 'The best all-round pair; the case is bulky.',
        facts: { price: 449, 'noise cancelling': 'excellent', battery: '30 h', comfort: 'good' },
        pros: ['Cancels the most noise', 'Best sound'],
        cons: ['Bulky case'],
        says: [{ doc: 1, text: 'The XM6 cut more low-frequency rumble than anything else we tried.' }, { doc: 2, text: 'Close behind Bose for comfort.' }],
      },
      {
        name: 'Bose QC Ultra 2',
        verdict: 'The most comfortable for hours; battery is shorter.',
        facts: { price: 429, 'noise cancelling': 'excellent', battery: '24 h', comfort: 'best' },
        pros: ['Most comfortable'],
        cons: ['24 h battery'],
        says: [{ doc: 2, text: 'Comfortable for long flights; cancellation close to Sony’s.' }],
      },
    ],
    documents: [
      { title: 'The best noise-cancelling headphones we tested this year', url: 'https://example.com/reviews/noise-cancelling-headphones', source: 'example.com', date: '2026-10-02', kind: 'test', summary: 'Tested 14 pairs; Sony cancels the most noise.' },
      { title: 'Bose QuietComfort Ultra (2nd gen) review', url: 'https://example.org/bose-qc-ultra-2', source: 'example.org', date: '2026-09-15', kind: 'review', summary: 'Comfortable for long flights; battery shorter than Sony’s.' },
    ],
    notes: 'Prices are list prices; both are often on sale.',
  },
  async run({ query, goal, answer, needs = [], criteria, items = [], documents = [], notes }) {
    const docs = documents.map((d, i) => ({
      n: i + 1,
      title: d.title,
      url: d.url ? web(d.url) : null,
      source: d.source || (d.url ? hostOf(d.url) : null),
      date: d.date ?? null,
      kind: d.kind ?? null,
      summary: d.summary,
      quotes: d.quotes ?? [],
      facts: d.facts ?? {},
    }))
    const known = n => Number.isInteger(n) && n >= 1 && n <= docs.length
    const cites = list => [...new Set((list ?? []).filter(known))]
    // "Best for" as a fact says again what needs says.
    const BEST = /^best[ _-]?for$/i
    const own = facts => Object.fromEntries(Object.entries(facts ?? {}).filter(([k]) => !(needs.length && BEST.test(k))))
    const things = items.map((it, i) => ({
      n: i + 1,
      name: it.name,
      about: it.about ?? null,
      verdict: it.verdict ?? null,
      facts: own(it.facts),
      pros: it.pros ?? [],
      cons: it.cons ?? [],
      says: (it.says ?? []).filter(s => known(s.doc)),
      when: it.when ?? null,
      group: it.group ?? null,
      url: it.url ? web(it.url) : null,
      image: null,
      imageFrom: null,
    }))
    // A photo of each item, fetched here (the app's page loads nothing):
    // the one the research saw, else its own page's; for a pick, else its
    // Wikipedia article's.
    // Pieces to put together (an outfit's top, shoes) are pictured too.
    const pick = (goal ? goal === 'pick' : needs.length > 0) || items.some(it => it.group)
    const pics = await picturesFor(items.map((it, i) => ({ name: it.name, image: it.image ? web(it.image) : null, url: things[i].url })), { wiki: pick })
    things.forEach((t, i) => {
      t.image = pics[i]?.src ?? null
      t.imageFrom = pics[i]?.from ?? null
    })
    const named = name => things.find(t => t.name.toLowerCase() === String(name).toLowerCase())?.n ?? null
    // The criteria named, then any other fact the items give, most given first.
    const given = things.flatMap(t => Object.keys(t.facts))
    const counted = [...new Set(given)].sort((a, b) => given.filter(k => k === b).length - given.filter(k => k === a).length)
    const order = [...new Set([...(criteria ?? []).filter(c => given.includes(c)), ...counted])].slice(0, 10)
    return {
      widget: 'findings',
      query,
      goal: goal ?? (needs.length ? 'pick' : things.some(t => t.when) ? 'news' : things.length ? 'overview' : 'answer'),
      answer: answer ? { text: answer.text, cites: cites(answer.cites) } : null,
      needs: needs.map(x => ({ need: x.need, pick: x.pick, item: named(x.pick), why: x.why ?? null, cites: cites(x.cites) })),
      criteria: order,
      items: things,
      documents: docs,
      facts: [...new Set(docs.flatMap(d => Object.keys(d.facts)))],
      notes: notes ?? null,
      source: 'found by Claude',
    }
  },
  summary(d) {
    if (!d.documents.length && !d.items.length) return `Nothing found for "${d.query}".`
    const parts = [`For "${d.query}"${d.answer ? `: ${d.answer.text}` : '.'}`]
    if (d.needs.length) parts.push(`Best for: ${d.needs.map(x => `${x.need}, ${x.pick}`).join('; ')}.`)
    if (d.items.length) parts.push(`${d.goal === 'how-to' ? 'Steps' : d.goal === 'pick' ? 'Options' : d.goal === 'news' ? 'Events' : 'Points'}: ${d.items.slice(0, 6).map(t => t.name).join('; ')}.`)
    parts.push(`From ${d.documents.length} source(s): ${d.documents.slice(0, 6).map(x => x.source ?? x.title).join(', ')}.`)
    return parts.join(' ')
  },
}

// What a model found, cut to what the tool takes: what is longer is
// shortened rather than refused, for a model whose answer no MCP host
// checked (the chat's research step).
export function clip(found = {}) {
  const cut = (text, n) => (typeof text === 'string' && text.trim() ? (text.length > n ? `${text.slice(0, n - 1)}…` : text) : undefined)
  const list = (value, n) => (Array.isArray(value) ? value.slice(0, n) : [])
  const texts = (value, n, len) => list(value, 50).map(x => cut(x, len)).filter(Boolean).slice(0, n)
  const nums = value => list(value, 8).filter(x => Number.isInteger(x) && x >= 1)
  const fact = v => (typeof v === 'string' ? cut(v, 200) ?? '' : typeof v === 'number' || typeof v === 'boolean' || v === null ? v : String(v ?? ''))
  const facts = value => Object.fromEntries(Object.entries(value && typeof value === 'object' && !Array.isArray(value) ? value : {}).slice(0, 12).map(([k, v]) => [cut(k, 60) ?? 'value', fact(v)]))
  const tidy = object => Object.fromEntries(Object.entries(object).filter(([, v]) => v !== undefined))
  const docs = list(found.documents, 60).filter(d => d && cut(d.title, 300)).slice(0, 40)
  return tidy({
    query: cut(String(found.query ?? ''), 300) ?? 'what was found',
    goal: GOALS.includes(found.goal) ? found.goal : undefined,
    answer: found.answer && cut(found.answer.text, 800) ? tidy({ text: cut(found.answer.text, 800), cites: nums(found.answer.cites) }) : undefined,
    needs: list(found.needs, 6).filter(x => x && cut(x.need, 80) && cut(x.pick, 120)).map(x => tidy({ need: cut(x.need, 80), pick: cut(x.pick, 120), why: cut(x.why, 240), cites: nums(x.cites) })),
    criteria: texts(found.criteria, 8, 60),
    items: list(found.items, 30).filter(t => t && cut(t.name, 120)).slice(0, 20).map(t => tidy({
      name: cut(t.name, 120),
      about: cut(t.about, 400),
      verdict: cut(t.verdict, 300),
      facts: facts(t.facts),
      pros: texts(t.pros, 5, 160),
      cons: texts(t.cons, 5, 160),
      says: list(t.says, 20).filter(s => s && Number.isInteger(s.doc) && s.doc >= 1 && cut(s.text, 400)).slice(0, 8).map(s => ({ doc: s.doc, text: cut(s.text, 400) })),
      when: cut(t.when, 40),
      group: cut(t.group, 60),
      url: cut(t.url, 2000),
      image: typeof t.image === 'string' && /^https?:\/\//i.test(t.image) ? cut(t.image, 2000) : undefined,
    })),
    documents: docs.map(d => tidy({
      title: cut(d.title.trim(), 300),
      url: cut(d.url, 2000),
      source: cut(d.source, 120),
      date: cut(d.date, 40),
      kind: cut(d.kind, 40),
      summary: cut(String(d.summary ?? ''), 2000) ?? '',
      quotes: texts(d.quotes, 5, 600),
      facts: facts(d.facts),
    })),
    notes: cut(found.notes, 1000),
  })
}

// What a lookup for a kept answer found, added to it rather than put in
// its place (PRIYA-1, PRIYA-3 of round 9): every option kept stays, in
// its order and under its name (the notes on it go by its name); one the
// lookup found again takes its new facts, quotes and link; one it found
// new goes at the end; a source found again keeps its number. The kept
// answer's question, bottom line and best-for stay; what the lookup said
// is its note.
export function addFound(kept = {}, found = {}) {
  const words = name => new Set(String(name ?? '').toLowerCase().normalize('NFKD').replace(/[̀-ͯ]/g, '').split(/[^a-z0-9]+/).filter(w => w.length > 1 && !SAME_SKIP.has(w)))
  const same = (a, b) => {
    const x = words(a), y = words(b)
    if (!x.size || !y.size) return false
    const shared = [...x].filter(w => [...y].some(v => alike(w, v))).length
    return shared === Math.min(x.size, y.size) || shared / new Set([...x, ...y]).size >= 0.6
  }
  const documents = [...(kept.documents ?? [])]
  const number = new Map()
  ;(found.documents ?? []).forEach((d, i) => {
    let at = d.url ? documents.findIndex(x => x.url === d.url) : -1
    if (at < 0) {
      documents.push(d)
      at = documents.length - 1
    } else documents[at] = { ...documents[at], ...d, quotes: [...new Set([...(documents[at].quotes ?? []), ...(d.quotes ?? [])])].slice(0, 5) }
    number.set(i + 1, at + 1)
  })
  const renumber = list => (list ?? []).map(n => number.get(n)).filter(Boolean)
  const says = list => (list ?? []).map(s => ({ ...s, doc: number.get(s.doc) })).filter(s => s.doc)
  const items = (kept.items ?? []).map(item => ({ ...item }))
  for (const raw of found.items ?? []) {
    const item = { ...raw, says: says(raw.says) }
    const old = items.find(x => same(x.name, item.name))
    if (!old) {
      items.push(item)
      continue
    }
    const quoted = new Set((old.says ?? []).map(s => `${s.doc} ${s.text}`))
    Object.assign(old, {
      facts: { ...(old.facts ?? {}), ...(item.facts ?? {}) },
      says: [...(old.says ?? []), ...item.says.filter(s => !quoted.has(`${s.doc} ${s.text}`))].slice(0, 8),
      verdict: item.verdict ?? old.verdict,
      url: item.url ?? old.url,
      image: old.image ?? item.image,
    })
  }
  const criteria = [...new Set([...(kept.criteria ?? []), ...(found.criteria ?? [])])].slice(0, 8)
  const said = found.answer?.text ? found.answer.text : found.notes
  return {
    ...kept,
    items: items.slice(0, 20),
    documents: documents.slice(0, 40),
    criteria,
    answer: kept.answer ?? (found.answer && { ...found.answer, cites: renumber(found.answer.cites) }),
    needs: kept.needs ?? found.needs,
    notes: said ? `${said}`.slice(0, 1000) : kept.notes,
  }
}
// Two words of a product's name are the same when they are, or when the
// short one is the long one shortened ("QC" for "QuietComfort"); model
// numbers ("1000XM4", "1000XM5") only when they are equal.
const alike = (a, b) => {
  if (a === b) return true
  const [short, long] = a.length <= b.length ? [a, b] : [b, a]
  if (/\d/.test(a) || /\d/.test(b) || short.length > 4 || short[0] !== long[0]) return false
  let at = 0
  for (const c of long) if (c === short[at]) at++
  return at === short.length
}
const SAME_SKIP = new Set(['the', 'and', 'with', 'for', 'headphones', 'headphone', 'wireless', 'noise', 'cancelling', 'canceling', 'over', 'ear'])

// The options of a kept answer a request takes out ("take the AirPods Max
// off my list"): those whose name's own words are all in the request, or,
// when none is, the one most of whose words are (round 9 PRIYA-4).
export function takeOut(kept = {}, request = '') {
  const said = String(request).toLowerCase().normalize('NFKD').replace(/[\u0300-\u036f]/g, '')
  const words = name => String(name ?? '').toLowerCase().normalize('NFKD').replace(/[\u0300-\u036f]/g, '').split(/[^a-z0-9]+/).filter(w => w.length > 1 && !SAME_SKIP.has(w))
  const asked = new Set(said.split(/[^a-z0-9]+/).filter(Boolean))
  const share = item => {
    const w = words(item.name)
    const hit = x => asked.has(x) || [...asked].some(a => alike(a, x) || (/\d/.test(a) && a.length >= 3 && x.endsWith(a)))
    // A model number alone ("the XM4") picks its option.
    if (w.some(x => /\d/.test(x) && [...asked].some(a => /\d/.test(a) && a.length >= 3 && x.endsWith(a)))) return 1
    return w.length ? w.filter(hit).length / w.length : 0
  }
  const items = kept.items ?? []
  let gone = items.filter(item => share(item) === 1)
  if (!gone.length) {
    const scored = items.map(item => ({ item, s: share(item) })).filter(x => x.s >= 0.5).sort((a, b) => b.s - a.s)
    if (scored.length && (scored.length === 1 || scored[0].s > scored[1].s)) gone = [scored[0].item]
  }
  if (!gone.length) return { kept, gone: [] }
  const names = new Set(gone.map(i => i.name))
  return { kept: { ...kept, items: items.filter(i => !names.has(i.name)), needs: (kept.needs ?? []).filter(n => !names.has(n.pick)) }, gone: [...names] }
}
