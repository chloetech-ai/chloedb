// Who is working on an app right now: each builder (and the owner) says so
// every few seconds while the app is open in front of them, and is "here"
// for a short while after the last time they said it. Kept in memory: it is
// about the next minute, not the app's history.

export const PRESENCE_TTL_MS = 30000;

export class Presence {
  constructor({ ttl = PRESENCE_TTL_MS, now = () => Date.now() } = {}) {
    this.ttl = ttl;
    this.now = now;
    this.apps = new Map(); // appId -> Map(personId -> {id, name, role, screen, at})
  }

  // A heartbeat; true when who is here changed (someone arrived or left).
  beat(appId, who) {
    const before = this.key(appId);
    const m = this.apps.get(appId) || new Map();
    m.set(who.id, { id: String(who.id), name: String(who.name || '').slice(0, 80), role: who.role || 'owner', screen: String(who.screen || '').slice(0, 80), at: this.now() });
    this.apps.set(appId, m);
    return before !== this.key(appId);
  }

  leave(appId, id) {
    const before = this.key(appId);
    this.apps.get(appId)?.delete(String(id));
    return before !== this.key(appId);
  }

  here(appId) {
    const m = this.apps.get(appId);
    if (!m) return [];
    const now = this.now();
    for (const [id, p] of m) if (now - p.at > this.ttl) m.delete(id);
    return [...m.values()].sort((a, b) => a.name.localeCompare(b.name)).map(({ at, ...p }) => ({ ...p, seen: new Date(at).toISOString() }));
  }

  key(appId) { return this.here(appId).map((p) => `${p.id}:${p.screen}`).join(','); }
}
