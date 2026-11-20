// Spotify, as the person's own account: songs found, their playlists and
// what they play, a playlist made, a song played or queued. Each person
// connects their own Spotify once (Authorization Code with PKCE, so the
// server needs only the app's client id, SPOTIFY_CLIENT_ID, and no
// secret); the tokens stay on the server (connections.mjs), and an app gets
// only the answers. SPOTIFY_ACCOUNTS and SPOTIFY_API point elsewhere for a test.
import { createHash, randomBytes } from 'node:crypto'
import { z } from 'zod'

const ACCOUNTS = () => process.env.SPOTIFY_ACCOUNTS || 'https://accounts.spotify.com'
const API = () => process.env.SPOTIFY_API || 'https://api.spotify.com/v1'
export const SPOTIFY = { service: 'spotify', name: 'Spotify', client: () => process.env.SPOTIFY_CLIENT_ID || '' }
const SCOPES = 'playlist-read-private playlist-read-collaborative playlist-modify-private playlist-modify-public user-read-playback-state user-modify-playback-state user-read-currently-playing user-read-recently-played user-top-read user-library-read'

// Not connected yet: the chat offers the person the sign-in.
export class NeedsConnect extends Error {
  constructor(service, name, extra = {}) {
    super(`Connect ${name} first: it is the person's own account.`)
    this.connect = { service, name, ...extra }
  }
}

const b64url = buf => buf.toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')

// The address the person signs in at, and what the answer is checked by.
export function spotifyStart(redirect) {
  const verifier = b64url(randomBytes(48))
  const state = b64url(randomBytes(18))
  const q = new URLSearchParams({
    client_id: SPOTIFY.client(), response_type: 'code', redirect_uri: redirect, scope: SCOPES, state,
    code_challenge_method: 'S256', code_challenge: b64url(createHash('sha256').update(verifier).digest()),
  })
  return { url: `${ACCOUNTS()}/authorize?${q}`, verifier, state }
}

async function tokenCall(form) {
  const res = await fetch(`${ACCOUNTS()}/api/token`, { method: 'POST', headers: { 'content-type': 'application/x-www-form-urlencoded' }, body: new URLSearchParams({ client_id: SPOTIFY.client(), ...form }), signal: AbortSignal.timeout(10000) })
  const out = await res.json().catch(() => ({}))
  if (!res.ok || !out.access_token) throw new Error(`Spotify refused the sign-in: ${out.error_description || out.error || res.status}`)
  return { access: out.access_token, refresh: out.refresh_token, expires: Date.now() + (Number(out.expires_in) || 3600) * 1000 - 60_000, scope: out.scope }
}

// The code Spotify sent back, as tokens, with whose account it is.
export async function spotifyFinish(code, verifier, redirect) {
  const token = await tokenCall({ grant_type: 'authorization_code', code, redirect_uri: redirect, code_verifier: verifier })
  const me = await call(token.access, 'GET', '/me')
  return { ...token, user: me.id, label: me.display_name || me.id }
}

async function call(access, method, path, body) {
  const res = await fetch(path.startsWith('http') ? path : `${API()}${path}`, {
    method,
    headers: { authorization: `Bearer ${access}`, ...(body ? { 'content-type': 'application/json' } : {}) },
    body: body ? JSON.stringify(body) : undefined,
    signal: AbortSignal.timeout(10000),
  })
  if (res.status === 204) return null
  const out = await res.json().catch(() => null)
  if (!res.ok) {
    const error = new Error(out?.error?.message ? `Spotify: ${out.error.message}` : `Spotify answered ${res.status}`)
    error.status = res.status
    error.reason = out?.error?.reason
    throw error
  }
  return out
}

// A call as the person: their token, refreshed when it ran out.
async function as(ctx, method, path, body) {
  const account = ctx?.account?.('spotify')
  let token = account ? await account.get() : null
  if (!token?.access) throw new NeedsConnect('spotify', 'Spotify')
  if (Date.now() > token.expires && token.refresh) {
    try {
      const fresh = await tokenCall({ grant_type: 'refresh_token', refresh_token: token.refresh })
      // Spotify may keep the refresh token the same and not send it again.
      token = { ...token, ...fresh, refresh: fresh.refresh || token.refresh }
      await account.set(token, token.label)
    } catch {
      await account.remove()
      throw new NeedsConnect('spotify', 'Spotify')
    }
  }
  try {
    return await call(token.access, method, path, body)
  } catch (error) {
    if (error.status === 401) {
      await account.remove()
      throw new NeedsConnect('spotify', 'Spotify')
    }
    throw error
  }
}

const art = images => (Array.isArray(images) && images.length ? (images.find(i => i.width && i.width <= 320) ?? images[0]).url : null)
const track = t =>
  t && {
    name: t.name, by: (t.artists ?? []).map(a => a.name).join(', '), album: t.album?.name ?? null, image: art(t.album?.images),
    uri: t.uri, url: t.external_urls?.spotify ?? null, minutes: t.duration_ms ? Math.round(t.duration_ms / 600) / 100 : null, explicit: Boolean(t.explicit),
  }
const playlist = p => p && { name: p.name, about: p.description || null, image: art(p.images), uri: p.uri, id: p.id, url: p.external_urls?.spotify ?? null, tracks: p.tracks?.total ?? p.items?.total ?? null, owner: p.owner?.display_name ?? null }
const artist = a => a && { name: a.name, image: art(a.images), uri: a.uri, url: a.external_urls?.spotify ?? null, genres: a.genres ?? [] }
const album = a => a && { name: a.name, by: (a.artists ?? []).map(x => x.name).join(', '), image: art(a.images), uri: a.uri, url: a.external_urls?.spotify ?? null, year: String(a.release_date ?? '').slice(0, 4) || null }

const DOMAINS = ['https://i.scdn.co', 'https://mosaic.scdn.co', 'https://image-cdn-ak.spotifycdn.com', 'https://image-cdn-fa.spotifycdn.com', 'https://wrapped-images.spotifycdn.com']
const NOTE = 'From the person’s own Spotify.'

export const spotifySearch = {
  name: 'spotify_search',
  title: 'Spotify search',
  view: null,
  service: 'spotify',
  domains: DOMAINS,
  description:
    "Songs, artists, albums or playlists on Spotify, from the person's own account (they connect it once). Give query for a search, or songs, " +
    'a list of "Title - Artist" (up to 40), to find each one\'s track: how a playlist the person asked for is put together (songs for a road ' +
    'trip, a festival, a mood). Each has name, by, album, image (its cover), uri, url and minutes; songs not found are listed under missing.',
  input: {
    query: z.string().max(200).optional().describe('What to search for: "Fred again..", "sunset house"'),
    songs: z.array(z.string().max(160)).max(40).optional().describe('Songs to find, each "Title - Artist"'),
    type: z.enum(['track', 'artist', 'album', 'playlist']).optional().describe('What to find; track when not given'),
    limit: z.number().int().min(1).max(20).optional(),
  },
  example: { songs: ['Delilah (pull me out of this) - Fred again..', 'Midnight City - M83'] },
  async run({ query, songs, type = 'track', limit = 10 }, ctx) {
    if (songs?.length) {
      const found = await Promise.all(songs.map(s => as(ctx, 'GET', `/search?${new URLSearchParams({ q: s.replace(/\s+-\s+/, ' '), type: 'track', limit: '1' })}`).then(r => track(r?.tracks?.items?.[0])).catch(error => (error instanceof NeedsConnect ? Promise.reject(error) : null))))
      return { widget: 'spotify', kind: 'track', items: found.filter(Boolean), missing: songs.filter((_, i) => !found[i]), note: NOTE }
    }
    if (!query) throw new Error('give query or songs')
    const r = await as(ctx, 'GET', `/search?${new URLSearchParams({ q: query, type, limit: String(limit) })}`)
    const map = { track, artist, album, playlist }
    return { widget: 'spotify', kind: type, query, items: (r?.[`${type}s`]?.items ?? []).filter(Boolean).map(map[type]), note: NOTE }
  },
  summary: d => `${d.items.length} ${d.kind}(s) on Spotify${d.query ? ` for "${d.query}"` : ''}: ${d.items.slice(0, 5).map(i => (i.by ? `${i.name} by ${i.by}` : i.name)).join('; ')}${d.missing?.length ? `. Not found: ${d.missing.join('; ')}` : ''}.`,
}

export const spotifyMine = {
  name: 'spotify_mine',
  title: 'My Spotify',
  view: null,
  service: 'spotify',
  domains: DOMAINS,
  description:
    "What is in the person's own Spotify: what: playlists (theirs), top_tracks or top_artists (what they play most), recent (played lately), " +
    'saved (liked songs), now (playing now), or playlist with id (a playlist\'s songs). Items as spotify_search gives them.',
  input: {
    what: z.enum(['playlists', 'top_tracks', 'top_artists', 'recent', 'saved', 'now', 'playlist']).describe('What to read'),
    id: z.string().max(200).optional().describe('For playlist: its id, uri or link'),
    range: z.enum(['short_term', 'medium_term', 'long_term']).optional().describe('For top: the last 4 weeks, 6 months or years'),
  },
  example: { what: 'top_tracks' },
  async run({ what, id, range = 'medium_term' }, ctx) {
    let items = []
    let kind = 'track'
    let title = null
    if (what === 'playlists') (kind = 'playlist'), (items = ((await as(ctx, 'GET', '/me/playlists?limit=30'))?.items ?? []).map(playlist))
    else if (what === 'top_tracks') items = ((await as(ctx, 'GET', `/me/top/tracks?limit=20&time_range=${range}`))?.items ?? []).map(track)
    else if (what === 'top_artists') (kind = 'artist'), (items = ((await as(ctx, 'GET', `/me/top/artists?limit=20&time_range=${range}`))?.items ?? []).map(artist))
    else if (what === 'recent') items = ((await as(ctx, 'GET', '/me/player/recently-played?limit=20'))?.items ?? []).map(i => ({ ...track(i.track), played: i.played_at }))
    else if (what === 'saved') items = ((await as(ctx, 'GET', '/me/tracks?limit=30'))?.items ?? []).map(i => track(i.track))
    else if (what === 'now') {
      const now = await as(ctx, 'GET', '/me/player/currently-playing')
      items = now?.item ? [{ ...track(now.item), playing: Boolean(now.is_playing) }] : []
    } else {
      const pid = String(id ?? '').match(/(?:playlist[:/])?([A-Za-z0-9]{22})/)?.[1]
      if (!pid) throw new Error('give the playlist id')
      const p = await as(ctx, 'GET', `/playlists/${pid}`)
      title = p?.name ?? null
      const list = p?.tracks?.items ?? p?.items?.items ?? []
      items = list.map(i => track(i.track ?? i.item)).filter(Boolean)
    }
    return { widget: 'spotify', kind, what, ...(title ? { title } : {}), items: items.filter(Boolean), note: NOTE }
  },
  summary: d => (d.items.length ? `${d.title ?? d.what}: ${d.items.slice(0, 6).map(i => (i.by ? `${i.name} by ${i.by}` : i.name)).join('; ')}${d.items.length > 6 ? ` and ${d.items.length - 6} more` : ''}.` : `Nothing in ${d.what} on this Spotify.`),
}

export const spotifyMake = {
  name: 'spotify_playlist_make',
  title: 'Make a Spotify playlist',
  view: null,
  service: 'spotify',
  acts: true,
  domains: DOMAINS,
  description:
    "Makes a playlist in the person's own Spotify, with the songs given by uri (from spotify_search), in that order, and answers with its " +
    'link (url). An app calls it from a button the person taps ("Save to Spotify"), never by itself, and shows the link once it answers.',
  input: {
    name: z.string().min(1).max(100).describe('The playlist’s name'),
    about: z.string().max(300).optional().describe('Its description'),
    uris: z.array(z.string().regex(/^spotify:(track|episode):[A-Za-z0-9]{22}$/)).min(1).max(100).describe('The songs, as spotify:track:... uris'),
    public: z.boolean().optional(),
  },
  example: { name: 'Coachella drive', uris: ['spotify:track:6habFhsOp2NvshLv26DqMb'] },
  async run({ name, about, uris, public: open = false }, ctx) {
    const me = await as(ctx, 'GET', '/me')
    const body = { name, description: about ?? '', public: Boolean(open) }
    let p
    try {
      p = await as(ctx, 'POST', '/me/playlists', body)
    } catch (error) {
      if (error instanceof NeedsConnect || ![403, 404, 405].includes(error.status)) throw error
      p = await as(ctx, 'POST', `/users/${encodeURIComponent(me.id)}/playlists`, body)
    }
    for (let i = 0; i < uris.length; i += 100) {
      try {
        await as(ctx, 'POST', `/playlists/${p.id}/items`, { uris: uris.slice(i, i + 100) })
      } catch (error) {
        if (error instanceof NeedsConnect || ![403, 404, 405].includes(error.status)) throw error
        await as(ctx, 'POST', `/playlists/${p.id}/tracks`, { uris: uris.slice(i, i + 100) })
      }
    }
    return { widget: 'spotify', kind: 'playlist', made: { name: p.name, url: p.external_urls?.spotify ?? null, uri: p.uri, id: p.id, tracks: uris.length }, note: NOTE }
  },
  summary: d => `Made the playlist "${d.made.name}" in Spotify with ${d.made.tracks} song(s): ${d.made.url ?? d.made.uri}.`,
}

export const spotifyPlay = {
  name: 'spotify_play',
  title: 'Play on Spotify',
  view: null,
  service: 'spotify',
  acts: true,
  domains: [],
  description:
    'Plays a song, an album or a playlist (uri) on the Spotify the person has open on a phone or computer, or adds a song to its queue ' +
    '(queue: true). Called from a button the person taps. Spotify plays only where the app is open, and playing needs Premium: the answer says so.',
  input: {
    uri: z.string().regex(/^spotify:(track|album|playlist|artist|episode):[A-Za-z0-9]{22}$/).describe('What to play'),
    queue: z.boolean().optional().describe('Add a song to the queue instead of playing it now'),
  },
  example: { uri: 'spotify:track:6habFhsOp2NvshLv26DqMb' },
  async run({ uri, queue = false }, ctx) {
    try {
      if (queue) await as(ctx, 'POST', `/me/player/queue?${new URLSearchParams({ uri })}`)
      else await as(ctx, 'PUT', '/me/player/play', /^spotify:(track|episode):/.test(uri) ? { uris: [uri] } : { context_uri: uri })
      return { widget: 'spotify', kind: 'play', uri, done: queue ? 'queued' : 'playing' }
    } catch (error) {
      if (error instanceof NeedsConnect) throw error
      if (error.status === 404 || error.reason === 'NO_ACTIVE_DEVICE') return { widget: 'spotify', kind: 'play', uri, done: 'no device', note: 'Open Spotify on your phone or computer first, then tap again.' }
      if (error.status === 403) return { widget: 'spotify', kind: 'play', uri, done: 'not allowed', note: 'Spotify plays from other apps only with Premium.' }
      throw error
    }
  },
  summary: d => (d.done === 'playing' ? `Playing ${d.uri} on Spotify.` : d.done === 'queued' ? `Added ${d.uri} to the Spotify queue.` : `Not played: ${d.note}`),
}

export const SPOTIFY_TOOLS = [spotifySearch, spotifyMine, spotifyMake, spotifyPlay]
