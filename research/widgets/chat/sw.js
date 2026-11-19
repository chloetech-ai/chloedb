// The page and what it loads to show itself, kept as last seen, so it
// opens with no signal (MAYA-8): asked of the network first, and from here
// only when there is none. What apps keep is the page's own (its outbox).
const CACHE = 'instant-apps-v1'
self.addEventListener('install', e => e.waitUntil(caches.open(CACHE).then(c => c.addAll(['/', '/bridge.js'])).catch(() => {}).then(() => self.skipWaiting())))
self.addEventListener('activate', e => e.waitUntil(self.clients.claim()))
// The page, the apps it lists and opens, and the conversation it restores.
const LOADS = [
  ['GET', /^\/$/],
  ['GET', /^\/bridge\.js$/],
  ['GET', /^\/api\/apps$/],
  ['GET', /^\/api\/talk$/],
  ['GET', /^\/api\/shown\/[^/]+$/],
  ['POST', /^\/api\/apps\/[^/]+\/open$/],
]
const hash = text => {
  let h = 2166136261
  for (const c of text) h = Math.imul(h ^ c.charCodeAt(0), 16777619) >>> 0
  return h.toString(36)
}
self.addEventListener('fetch', e => {
  const url = new URL(e.request.url)
  if (url.origin !== self.location.origin || !LOADS.some(([method, path]) => method === e.request.method && path.test(url.pathname))) return
  e.respondWith(
    (async () => {
      // A request with a body is kept under its address and its body.
      const body = e.request.method === 'POST' ? await e.request.clone().text() : ''
      const key = new Request(`${url.href}${body ? `${url.search ? '&' : '?'}sw-body=${hash(body)}` : ''}`)
      try {
        const res = await fetch(e.request)
        if (res.ok) await (await caches.open(CACHE)).put(key, res.clone())
        return res
      } catch (error) {
        const hit = await caches.match(key)
        if (hit) return hit
        throw error
      }
    })(),
  )
})
