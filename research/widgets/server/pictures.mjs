// Pictures for what Claude found: a photo of each item, fetched here and
// handed to the app inline (a data: address), since an app's page may
// load nothing itself. A picture comes from the address the research
// gave for the item (one it saw on a page it read), else from the item's
// own page (its og:image), else, for a thing to pick, from the Wikipedia
// article of that name. Every address is read as the page reader reads
// one: the public web only.
import { fetchJson, fetchPicture, readPage } from './reader.mjs'
import { SIMULATE } from './widgets/common.mjs'

const MAX_BYTES = 400_000 // one picture
const MAX_TOTAL = 3_000_000 // all of one answer's
const WIDTH = 330 // Wikimedia's thumbnails come in set widths; 330 is enough for a card
const cache = new Map() // address → picture or null, the newest last

// Wikimedia's pictures at a card's width, whatever width was linked.
function smaller(url) {
  const u = new URL(url)
  if (!/(^|\.)wikimedia\.org$/.test(u.hostname)) return url
  u.search = ''
  const thumb = /^(\/wikipedia\/[^/]+\/thumb\/.+?)\/\d+px-([^/]+)$/.exec(u.pathname)
  if (thumb) return `https://upload.wikimedia.org${thumb[1]}/${WIDTH}px-${thumb[2]}`
  const whole = /^\/wikipedia\/([^/]+)\/([0-9a-f]\/[0-9a-f]{2})\/([^/]+\.(jpe?g|png|webp))$/i.exec(u.pathname)
  if (whole) return `https://upload.wikimedia.org/wikipedia/${whole[1]}/thumb/${whole[2]}/${whole[3]}/${WIDTH}px-${whole[3]}`
  return url
}

// One picture as a data: address, or null.
export async function picture(url) {
  let at
  try {
    at = smaller(new URL(url).href)
  } catch {
    return null
  }
  if (cache.has(at)) return cache.get(at)
  let found = null
  try {
    const got = await fetchPicture(at, { maxBytes: MAX_BYTES })
    found = { src: `data:${got.type};base64,${got.bytes.toString('base64')}`, from: new URL(got.url).hostname.replace(/^www\./, ''), bytes: got.bytes.length }
  } catch {
    found = null
  }
  cache.set(at, found)
  if (cache.size > 500) cache.delete(cache.keys().next().value)
  return found
}

// The photo a page puts first (its og:image), or null.
async function pageImage(url) {
  try {
    const page = await readPage(url, { timeout: 5000 })
    for (const p of page.pictures.slice(0, 2)) {
      const got = await picture(p.url)
      if (got) return got
    }
  } catch {}
  return null
}

const words = text => new Set(String(text).normalize('NFD').replace(/\p{M}/gu, '').toLowerCase().match(/[\p{L}\p{N}]{3,}/gu) ?? [])

// The picture of the Wikipedia article on a thing, when its title shares
// most of the thing's words ("Bernese Mountain Dog", "Yosemite Valley").
async function wikiImage(name) {
  try {
    const q = new URLSearchParams({ action: 'query', format: 'json', generator: 'search', gsrsearch: name, gsrlimit: '3', prop: 'pageimages', piprop: 'thumbnail', pithumbsize: String(WIDTH), redirects: '1' })
    const data = await fetchJson(`https://en.wikipedia.org/w/api.php?${q}`, { timeout: 5000 })
    const pages = Object.values(data?.query?.pages ?? {}).sort((a, b) => (a.index ?? 9) - (b.index ?? 9))
    const want = words(name)
    for (const page of pages) {
      const have = words(page.title)
      const shared = [...want].filter(w => have.has(w)).length
      if (page.thumbnail?.source && want.size && shared / want.size >= 0.6) return await picture(page.thumbnail.source)
    }
  } catch {}
  return null
}

// A picture for each item ({ name, image, url }), in order; null where
// none was found. `wiki` also looks the item up on Wikipedia. It gives
// up on what is not found within `deadline` ms. With made-up data
// (WIDGETS_SIMULATE) nothing is fetched.
export async function picturesFor(items, { wiki = false, deadline = 9000 } = {}) {
  if (SIMULATE) return items.map(() => null)
  // A page several items share (a list of them) has no photo of one.
  const shared = new Set(items.map(i => i.url).filter((url, n, all) => url && all.indexOf(url) !== n))
  const one = async item => {
    if (item.image) {
      const got = await picture(item.image)
      if (got) return got
    }
    if (item.url && !shared.has(item.url)) {
      const got = await pageImage(item.url)
      if (got) return got
    }
    return wiki ? wikiImage(item.name) : null
  }
  const late = new Promise(resolve => setTimeout(() => resolve('late'), deadline).unref?.())
  const found = await Promise.all(items.map(item => Promise.race([one(item).catch(() => null), late]).then(x => (x === 'late' ? null : x))))
  // Within one answer's size, the first items keeping theirs, and each
  // picture shown once, by the first item that has it.
  let total = 0
  const seen = new Set()
  return found.map(p => {
    if (!p || seen.has(p.src)) return null
    seen.add(p.src)
    total += p.bytes
    return total > MAX_TOTAL ? null : p
  })
}
