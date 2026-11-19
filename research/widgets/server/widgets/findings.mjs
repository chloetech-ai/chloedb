// What Claude found: not a source of its own, but the documents the Claude
// of the conversation gathered itself (pages from a web search, pages it
// read, files, another tool's results), handed to an app to show. An app
// whose first tool is `findings` is filled with them: app_open and
// app_make take them as the tool's arguments, and this only checks and
// tidies them. The model never calls it directly.
import { z } from 'zod'

const FACT = z.union([z.string().max(200), z.number(), z.boolean(), z.null()])

const DOCUMENT = z.object({
  title: z.string().min(1).max(300).describe('The page’s or document’s title'),
  url: z.string().max(2000).optional().describe('Where it is (https://...), when it has an address'),
  source: z.string().max(120).optional().describe('Who published it: a site, an author, a file'),
  date: z.string().max(40).optional().describe('When it was published or last changed, as written (2026-11-18, "Nov 2026")'),
  kind: z.string().max(40).optional().describe('What it is: article, review, paper, docs, listing, post, file...'),
  summary: z.string().max(2000).describe('What it says that matters for the request, in your words, 1 to 4 sentences'),
  quotes: z.array(z.string().max(600)).max(5).optional().describe('Up to 5 passages quoted exactly'),
  facts: z.record(z.string().max(60), FACT).optional().describe('Comparable values it gives, the same names across documents ({"price": 349, "rating": 4.5, "battery": "30 h"})'),
})

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

// What a model found, cut to what the tool takes: what is longer is
// shortened rather than refused, for a model whose answer is not checked
// by an MCP host (the chat's research step).
export function clip({ query, documents = [], notes } = {}) {
  const cut = (text, n) => (typeof text === 'string' ? (text.length > n ? `${text.slice(0, n - 1)}…` : text) : undefined)
  const fact = v => (typeof v === 'string' ? cut(v, 200) : typeof v === 'number' || typeof v === 'boolean' || v === null ? v : String(v ?? ''))
  return {
    query: cut(String(query ?? ''), 300) || 'what was found',
    documents: documents
      .filter(d => d && typeof d.title === 'string' && d.title.trim())
      .slice(0, 40)
      .map(d => ({
        title: cut(d.title.trim(), 300),
        url: cut(d.url, 2000),
        source: cut(d.source, 120),
        date: cut(d.date, 40),
        kind: cut(d.kind, 40),
        summary: cut(String(d.summary ?? ''), 2000),
        quotes: (Array.isArray(d.quotes) ? d.quotes : []).filter(q => typeof q === 'string' && q.trim()).slice(0, 5).map(q => cut(q, 600)),
        facts: Object.fromEntries(Object.entries(d.facts && typeof d.facts === 'object' ? d.facts : {}).slice(0, 12).map(([k, v]) => [cut(k, 60), fact(v)])),
      }))
      .map(d => Object.fromEntries(Object.entries(d).filter(([, v]) => v !== undefined))),
    ...(notes ? { notes: cut(String(notes), 1000) } : {}),
  }
}

export const findings = {
  name: 'findings',
  title: 'What Claude found',
  view: null,
  given: true,
  domains: [],
  description:
    'Not a source: what you found yourself (pages from a web search, pages or files you read, another tool’s results), handed to an app to show. ' +
    'Give the request in "query" and each document with its title, address, source, date, a summary of what it says for this request, ' +
    'passages quoted exactly, and comparable facts under the same names across documents.',
  input: {
    query: z.string().min(1).max(300).describe('What was looked for, in the person’s words'),
    documents: z.array(DOCUMENT).max(40).describe('What was found, best first'),
    notes: z.string().max(1000).optional().describe('What the documents agree or disagree on, or what is missing, in a sentence or two'),
  },
  example: {
    query: 'best noise cancelling headphones 2026',
    documents: [
      {
        title: 'The best noise-cancelling headphones we tested this year',
        url: 'https://example.com/reviews/noise-cancelling-headphones',
        source: 'example.com',
        date: '2026-10-02',
        kind: 'review',
        summary: 'Tested 14 pairs; the Sony WH-1000XM6 cancel the most noise, the Bose QC Ultra are the most comfortable.',
        quotes: ['The XM6 cut more low-frequency rumble than anything else we tried.'],
        facts: { pick: 'Sony WH-1000XM6', price: 449, battery: '30 h' },
      },
      {
        title: 'Bose QuietComfort Ultra (2nd gen) review',
        url: 'https://example.org/bose-qc-ultra-2',
        source: 'example.org',
        date: '2026-09-15',
        kind: 'review',
        summary: 'Comfortable for long flights; cancellation close to Sony’s; battery shorter.',
        facts: { pick: 'Bose QC Ultra 2', price: 429, battery: '24 h' },
      },
    ],
    notes: 'Both reviews put Sony first for cancellation and Bose first for comfort.',
  },
  async run({ query, documents = [], notes }) {
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
    const facts = [...new Set(docs.flatMap(d => Object.keys(d.facts)))]
    return { widget: 'findings', query, documents: docs, facts, notes: notes ?? null, source: 'found by Claude' }
  },
  summary(d) {
    if (!d.documents.length) return `Nothing found for "${d.query}".`
    const top = d.documents.slice(0, 5).map(x => `${x.title}${x.source ? ` (${x.source}${x.date ? `, ${x.date}` : ''})` : ''}`)
    return `${d.documents.length} document(s) for "${d.query}": ${top.join('; ')}.${d.notes ? ` ${d.notes}` : ''}`
  },
}
