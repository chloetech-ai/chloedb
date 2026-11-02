// Cluster: the one canvas of the cluster, for looking and for changing.
//
// Live: a tile per shard (192 shards of 3 copies fit one screen), coloured
// by one lens at a time on an SLO scale (fine is neutral, amber and red are
// not fine), grouped by shard, node or zone, the problems named first. A
// shard opens its copies, its diagram, links to its logs, profile and
// statements, and what can be done to each copy. A node or a zone can be
// failed as a what-if.
//
// Draft: drag a copy to another shard or node, drag a shard onto another to
// move slots, drag the end handle to add shards, or use the shard's menu.
// Nothing changes on a drag: the plan beside it says what would happen
// (before and after, steps with repairs first, rows to copy, how long at the
// rate the last move ran, blast radius, checks, what would be removed), and
// one Apply runs it, with a typed confirmation when a copy goes. On
// Kubernetes the plan is one StrataCluster patch; on machines of your own,
// admin calls one at a time. A plan holds only what was asked.
//
// Capacity says what the autoscaler suggests in words; Changes lists this
// cluster's events and the changes people and automation made.

import { h, s, icon, clear, fmt, card, api, every, toast, modal, pill, empty, bus, seg, store, tipOn, hideTip, ApiError, copy, debounce, confirm, can, needs, link, when, whenTitle } from "../core.js";
import * as core from "../core.js";
import { verifyWords } from "./system.js";
import { fullIn } from "../spec.js";
import { state, isCoordinator, refreshCluster, fresh, staleText, isProduction } from "../data.js";

/** A phone or a narrow window: the shell's isNarrow() once it has one, else the same breakpoint. */
const narrow = () => (typeof core.isNarrow === "function" ? core.isNarrow() : matchMedia("(max-width: 900px)").matches);

export default {
  id: "cluster", title: "Cluster", icon: "layers", group: "Operate",
  keywords: "cluster canvas fleet topology control center shards copies replicas nodes zones health cpu disk p99 version lag what if fail draft plan apply add remove promote primary drain move slots rebalance capacity autoscale changes events kubernetes operator",
  render,
  onParams: (r) => active?.onParams(r),
};

const SLOTS = 4096;
const LAG_WARN = 1000, LAG_BAD = 10000;
const ASSUMED_RATE = 20000; // rows a second, until a move has been measured
const CRIT = new Set(["down", "no_reads", "no_writes"]);

let active = null; // the view on screen, for onParams
let runner = null; // the plan being applied: it goes on when the view is left
const runWatchers = new Set();
const runChanged = () => runWatchers.forEach((f) => { try { f(); } catch (e) { console.error(e); } });

// ---------------------------------------------------------------- words
const short = (url) => (url || "").replace(/^https?:\/\//, "");
const hostOf = (url) => { try { return new URL(url).hostname; } catch { return short(url).split(":")[0]; } };
const pct0 = (v) => (v == null ? "–" : fmt.pct(v, 0));
const plural = (n, w, ws) => fmt.plural(n, w, ws);
const copies_ = (n) => plural(n, "copy", "copies");
/** Capitalised, except a copy's name (s9-2) or a URL, which keep their case. */
const cap = (t) => (!t || /^(s\d+-\d+|[a-z]+:\/\/|[a-z0-9-]+\.[a-z])/.test(t) ? t : t[0].toUpperCase() + t.slice(1));
/** 4 h 45 min, 22 min, 40 s. */
function dur(secs) {
  if (secs == null || !Number.isFinite(secs)) return "–";
  secs = Math.max(0, Math.round(secs));
  if (secs < 60) return `${secs} s`;
  const m = Math.round(secs / 60);
  if (m < 60) return `${m} min`;
  const hr = Math.floor(m / 60), mm = m % 60;
  if (hr < 48) return mm ? `${hr} h ${mm} min` : `${hr} h`;
  return `${Math.floor(hr / 24)} d ${hr % 24} h`;
}
/** 19:42 today, Nov 2 19:42 another day (the full time and zone go in a title). */
const clock = (ms) => when(ms, { sec: false });
const zoneName = () => Intl.DateTimeFormat().resolvedOptions().timeZone || "local time";
const sinceText = (ms) => (ms ? `since ${clock(ms)} (${dur((Date.now() - ms) / 1000)})` : "");
/** Slots as runs: [a, b) half-open, said inclusively (2,496–2,516). */
const runsText = (rs) => (rs || []).map(([a, b]) => (b - a === 1 ? fmt.n(a) : `${fmt.n(a)}–${fmt.n(b - 1)}`)).join(", ");
const runsCount = (rs) => (rs || []).reduce((n, [a, b]) => n + (b - a), 0);
/** 25.4k, 3.1k, 870: transactions behind. */
const kn = (v) => (v == null ? "–" : v >= 1000 ? (v / 1000).toFixed(v >= 1e5 ? 0 : 1).replace(/\.0$/, "") + "k" : fmt.n(v));
const nodeLabel = (k) => (k && k.length > 18 ? "…" + k.slice(-14) : k || "–");

/** A copy's name: its node name or s1-0, with the URL in a tooltip. */
const cname = (c) => c.name || c.id;
const cspan = (c, cls = "") => h("span", { class: "mono " + cls, title: c.url }, cname(c));

/**
 * The names copies had when they last answered, by URL: a copy that is down
 * says no name, and "s0-0" beside "shard0-b" reads as two clusters.
 */
const NAMES = (() => { try { return new Map(Object.entries(store.get("cluster.names", {}) || {})); } catch { return new Map(); } })();
function keepNames(c) {
  let changed = false;
  for (const sh of c?.shards || []) for (const r of sh.replicas || []) if (r.name && r.url && NAMES.get(r.url) !== r.name) { NAMES.set(r.url, r.name); changed = true; }
  if (!changed) return;
  while (NAMES.size > 500) NAMES.delete(NAMES.keys().next().value);
  try { store.set("cluster.names", Object.fromEntries(NAMES)); } catch { /* storage off */ }
}
/**
 * Reads the cluster now and again in a few seconds: a coordinator answers
 * from a snapshot it takes every two seconds or so, so the first read after
 * a change can still show the cluster before it.
 */
function refreshSoon() { refreshCluster(); setTimeout(() => refreshCluster(), 2500); }
/** Whether a shard serves no reads: as the server says, else what failover implies. */
const noReads = (sh) => sh.state === "down" || sh.state === "no_reads" || (sh.state === "no_writes" && (sh.serves_reads != null ? sh.serves_reads === false : sh.failover !== false));

// ---------------------------------------------------------------- the model
/** The cluster as the canvas needs it, from `/v1/cluster` (old and new shapes). */
function model(c) {
  keepNames(c);
  const shards = (c?.shards || []).map((sh, idx) => {
    const i = sh.shard ?? idx;
    const reps = sh.replicas || [];
    const isDown = (r) => r.status === "down" || r.role === "unknown";
    const live = reps.filter((r) => !isDown(r));
    // The primary: a copy that answers and takes writes. A dead old primary
    // can still say it does (finding S3-1): it is not the primary.
    let p = live.filter((r) => r.takes_writes).sort((a, b) => (b.term || 0) - (a.term || 0))[0] || null;
    if (sh.primary) p = live.find((r) => r.url === sh.primary) || p;
    const follow = {};
    for (const f of p?.detail?.health?.replication?.replicas || []) follow[f.url] = f;
    const copies = reps.map((r, k) => {
      const down = isDown(r);
      const d = r.detail || {}, l = (!down && d.load) || {}, mn = (!down && d.minute) || null;
      const cores = d.system?.cores || l.cores || mn?.cores || null;
      const f = follow[r.url];
      const primary = r === p;
      const cpu = down ? null : mn?.busy != null ? mn.busy : mn?.cpu != null && cores ? mn.cpu / cores : l.cpu != null && cores ? l.cpu / cores : null;
      return {
        url: r.url, shard: i, k, id: r.short || `s${i}-${k}`, name: r.name || NAMES.get(r.url) || null,
        node: r.node || null, zone: r.zone || null, host: hostOf(r.url),
        down, primary,
        role: down ? "down" : primary ? "primary" : "replica",
        wasPrimary: down && (r.last_role === "primary" || !!r.takes_writes),
        downSince: r.down_since_ms || null, lastSeen: r.last_seen_ms || null, error: r.error || null,
        term: r.term ?? d.health?.replication?.term ?? null, txn: r.txn ?? null,
        docs: r.docs ?? null, rtt: r.rtt_ms ?? null,
        avoided: !!r.avoided, avoidedWhy: r.avoided_why || null,
        version: r.version?.version || d.system?.version || null, image: r.version?.image || null, digest: r.version?.digest || null,
        cores, cpu,
        disk: down ? null : l.disk_used_ratio ?? mn?.disk_used_ratio ?? null,
        diskFree: l.disk_free_bytes ?? null,
        p99: down ? null : mn?.p99_ms ?? l.p99_ms ?? null,
        qps: down ? null : mn?.qps ?? l.qps ?? null,
        window: mn ? "last minute" : "last 2 s",
        lag: primary ? 0 : down || !p ? null : f ? (f.in_sync && !f.copying ? 0 : f.behind ?? null) : null,
        inSync: primary ? true : f ? !!f.in_sync && !f.copying : null,
        copying: !primary && !!f?.copying,
        alerts: d.alerts || null,
        // false: the server says it has no --backup-dir; null: it does not say.
        backupDir: "backup_dir" in r ? r.backup_dir || false : null,
        uptime: d.system?.uptime_s ?? null,
      };
    });
    const n = copies.length, up = copies.filter((x) => !x.down).length;
    const majority = sh.majority ?? up > n / 2;
    const failover = sh.failover ?? null;
    let st = sh.state || (up === 0 ? "down" : !majority ? "no_writes" : up < n ? "degraded" : "ok");
    let why = sh.why || null;
    if (!why) {
      if (st === "down") why = `None of its ${copies_(n)} answers: searches that need it fail and it takes no writes.`;
      else if (!majority) why = `${up} of ${n} copies answer, fewer than a majority: it takes no writes, and with failover on its primary serves no reads either.`;
      else if (up < n) why = `${n - up} of ${n} copies do not answer: it serves from the other ${up}.`;
    }
    const rows = Math.max(0, ...copies.filter((x) => !x.down).map((x) => x.docs || 0), 0) || Math.max(0, ...copies.map((x) => x.docs || 0));
    return {
      i, url: sh.url, slots: sh.slots || [], held: sh.slots_held ?? runsCount(sh.slots), copies,
      primary: copies.find((x) => x.primary) || null, n, up, majority, failover, state: st, why, rows,
      serves_reads: sh.serves_reads ?? (st === "ok" || st === "degraded" ? true : st === "down" ? false : null),
      takes_writes: sh.takes_writes ?? (st === "ok" || st === "degraded"),
      callRate: sh.call_rate ?? null,
    };
  });
  const all = shards.flatMap((x) => x.copies);
  // Nodes: the node a copy reports, else the host of its URL.
  const nodes = new Map();
  for (const c of all) {
    const key = c.node || c.host;
    if (!nodes.has(key)) nodes.set(key, { key, known: !!c.node, zone: c.zone, copies: [] });
    const n = nodes.get(key);
    n.copies.push(c);
    if (!n.zone && c.zone) n.zone = c.zone;
  }
  const zones = new Map();
  for (const n of nodes.values()) {
    const z = n.zone || null;
    if (!zones.has(z)) zones.set(z, { key: z, nodes: [], copies: [] });
    zones.get(z).nodes.push(n);
    zones.get(z).copies.push(...n.copies);
  }
  const versions = new Map();
  for (const c of all) if (c.version) versions.set(c.version, (versions.get(c.version) || 0) + 1);
  const vlist = [...versions.entries()].sort((a, b) => b[1] - a[1]);
  const median = (xs) => { const v = xs.filter((x) => x != null).sort((a, b) => a - b); return v.length ? v[Math.floor(v.length / 2)] : null; };
  return {
    raw: c, at: c?.at_ms || Date.now(), shards, all, nodes, zones, versions: vlist, mainVersion: vlist[0]?.[0] || null,
    epoch: c?.slot_epoch, changes: c?.changes || { ok: true }, name: c?.cluster?.name || null, env: c?.cluster?.environment || null,
    medCpu: median(shards.map((x) => shardValue("cpu", x))), medP99: median(shards.map((x) => shardValue("p99", x))),
    nodesKnown: all.some((x) => x.node), zonesKnown: all.some((x) => x.zone), nodePrefix: commonPrefix([...nodes.keys()]),
    coordinators: state.fleet?.coordinators?.length || null,
  };
}

/** What every node name starts with, to a "-" or "." (ip-10-0-), when there are several. */
function commonPrefix(keys) {
  if (keys.length < 2) return "";
  let p = keys[0];
  for (const k of keys) while (p && !k.startsWith(p)) p = p.slice(0, -1);
  const cut = Math.max(p.lastIndexOf("-"), p.lastIndexOf("."));
  return cut >= 3 ? p.slice(0, cut + 1) : "";
}

// ---------------------------------------------------------------- lenses
/** The p99 the cluster promises, from the fleet's checks, else 250 ms. */
const slo = () => state.fleet?.checks?.find?.((x) => x.id === "p99")?.slo_ms ?? state.cluster?.cluster?.slo_p99_ms ?? 250;
const LENSES = {
  health: { label: "Health", title: "Copies answering, and whatever is wrong with the shard: red when it serves no reads or takes no writes, amber when a copy is down, behind, avoided or full" },
  cpu: { label: "CPU", title: "The busiest copy's CPU as a share of its cores, over its last minute", copy: (c) => c.cpu, fmt: pct0, warn: () => 0.7, bad: () => 0.85 },
  disk: { label: "Disk", title: "How full the fullest copy's volume is", copy: (c) => c.disk, fmt: pct0, warn: () => 0.8, bad: () => 0.9 },
  p99: { label: "p99", title: "The slowest copy's 99th percentile of searches, over its last minute, against the SLO", copy: (c) => c.p99, fmt: (v) => fmt.ms(v), warn: () => slo() * 0.6, bad: () => slo() },
  version: { label: "Version", title: "The version each copy runs; the one most copies run is neutral" },
  lag: { label: "Lag", title: "Transactions a replica has still to apply from its primary", copy: (c) => (c.primary ? null : c.lag), fmt: (v) => kn(v), warn: () => LAG_WARN, bad: () => LAG_BAD },
};
function shardValue(lens, sh) {
  const L = LENSES[lens];
  if (!L.copy) return null;
  const vs = sh.copies.filter((c) => !c.down).map(L.copy).filter((v) => v != null && !Number.isNaN(v));
  return vs.length ? Math.max(...vs) : null;
}
/** 0 fine, 1 amber, 2 red, null unknown. */
function level(lens, v) {
  const L = LENSES[lens];
  if (v == null || !L.warn) return null;
  return v >= L.bad() ? 2 : v >= L.warn() ? 1 : 0;
}
/** What is wrong with a shard, worst first: {sev 0..2, short, long}. */
function issues(sh) {
  const out = [];
  if (sh.state === "down") out.push({ sev: 2, short: "down", long: sh.why });
  else if (sh.state === "no_reads") out.push({ sev: 2, short: "no reads", long: sh.why });
  else if (sh.state === "no_writes") out.push({ sev: 2, short: sh.failover === false ? "no writes" : "no majority", long: sh.why });
  else if (sh.state === "degraded") {
    const d = sh.copies.filter((c) => c.down);
    out.push({ sev: 1, short: `${d.length} down`, long: `${d.map(cname).join(", ")} ${d.length === 1 ? "does" : "do"} not answer; it serves from ${sh.up} of ${sh.n}` });
  }
  for (const c of sh.copies) {
    if (c.down) continue;
    if (c.copying) out.push({ sev: 0, short: "copying", long: `${cname(c)} is being sent a copy of the primary` });
    else if (c.lag != null && c.lag >= LAG_WARN) out.push({ sev: 1, short: `lag ${kn(c.lag)}`, long: `${cname(c)} is ${fmt.n(c.lag)} transactions behind its primary` });
    if (c.disk != null && c.disk >= 0.85) out.push({ sev: 1, short: `disk ${pct0(c.disk)}`, long: `${cname(c)}'s volume is ${pct0(c.disk)} full` });
    if (c.avoided) out.push({ sev: 1, short: "avoided", long: `the coordinator sends ${cname(c)} no searches${c.avoidedWhy ? ": " + c.avoidedWhy : " after failures"}` });
  }
  const cpu = shardValue("cpu", sh), p99 = shardValue("p99", sh);
  if (p99 != null && p99 >= slo()) out.push({ sev: 1, short: `p99 ${fmt.ms(p99)}`, long: `its slowest copy's p99 is ${fmt.ms(p99)}, over the SLO of ${fmt.ms(slo())}` });
  if (cpu != null && cpu >= 0.85) out.push({ sev: 1, short: `CPU ${pct0(cpu)}`, long: `its busiest copy uses ${pct0(cpu)} of its cores` });
  for (const c of sh.copies) for (const a of c.alerts?.firing || []) out.push({ sev: a.severity === "critical" ? 2 : 1, short: "alert", long: `${a.name || a.id} fires on ${cname(c)}` });
  return out.sort((a, b) => b.sev - a.sev);
}
const healthLevel = (sh) => Math.max(0, ...issues(sh).map((x) => x.sev));

/** Problems across the cluster, for the strip on top: worst first. */
function problems(M) {
  const out = [];
  const crit = M.shards.filter((sh) => CRIT.has(sh.state));
  for (const sh of crit) {
    const down = sh.copies.filter((c) => c.down);
    const left = sh.copies.filter((c) => !c.down);
    out.push({
      sev: 2, shards: [sh.i], label: `Shard ${sh.i} ${sh.state === "down" ? "is down" : noReads(sh) ? "serves no reads" : "takes no writes"}`,
      text: `${sh.up} of ${sh.n} copies answer`, since: Math.min(...down.map((c) => c.downSince || Infinity)),
      impact: noReads(sh) ? "searches that need it fail, and it takes no writes" : "it takes no writes: searches go on from its other copies",
      next: left.length ? `Bring ${down.map(cname).join(" or ")} back, or remove ${down.length === 1 ? "it" : "them"} so ${left.map(cname).join(", ")} ${left.length === 1 ? "has" : "have"} a majority` : `Bring a copy of shard ${sh.i} back`,
      fix: left.length ? { kind: "repair", shard: sh.i } : null,
    });
  }
  const down = M.all.filter((c) => c.down && !crit.some((sh) => sh.i === c.shard));
  if (down.length) {
    const c0 = down.slice().sort((a, b) => (a.downSince || Infinity) - (b.downSince || Infinity))[0];
    out.push({
      sev: 1, shards: [...new Set(down.map((c) => c.shard))], label: `${down.length === 1 ? "1 copy" : down.length + " copies"} down`,
      text: [cname(c0), c0.downSince ? sinceText(c0.downSince) : null].filter(Boolean).join(" ") + (down.length > 1 ? ` and ${down.length - 1} more` : ""),
      long: c0.node ? `on node ${c0.node}${c0.error ? ": " + c0.error : ""}` : null,
      since: c0.downSince, impact: "their shards serve from the copies left, with one failure less to spare",
      next: "Bring them back, or replace them", fix: { kind: "replace", shards: [...new Set(down.map((c) => c.shard))] },
    });
  }
  const lag = M.all.filter((c) => !c.down && !c.primary && c.lag != null && c.lag >= LAG_WARN).sort((a, b) => b.lag - a.lag);
  if (lag.length) out.push({ sev: 1, shards: lag.map((c) => c.shard), label: `${lag.length === 1 ? "1 replica" : lag.length + " replicas"} behind`, text: `${cname(lag[0])} · ${kn(lag[0].lag)} transactions${lag.length > 1 ? ` and ${lag.length - 1} more` : ""}`, impact: "its reads are older, and writes that wait for every copy wait for it", next: "Look at its disk and CPU, or its logs" });
  const fleetDisk = state.fleet?.checks?.find?.((x) => x.id === "disk");
  const full = M.all.filter((c) => c.disk != null && c.disk >= 0.85).sort((a, b) => b.disk - a.disk);
  if (full.length) out.push({ sev: 1, shards: [...new Set(full.map((c) => c.shard))], label: `Disk ${pct0(full[0].disk)}`, text: `${cname(full[0])}${fleetDisk?.days_to_full != null ? ` · full in ${fullIn(fleetDisk.days_to_full)}` : ""}${full.length > 1 ? ` and ${full.length - 1} more over 85%` : ""}`, impact: "a database that cannot write refuses writes", next: "Add shards so each holds less, or grow the volumes" });
  const hot = M.shards.filter((sh) => {
    const cpu = shardValue("cpu", sh), p = shardValue("p99", sh);
    return (p != null && p >= slo()) || (cpu != null && cpu >= 0.8 && M.medCpu != null && cpu >= 1.5 * M.medCpu);
  }).sort((a, b) => (shardValue("cpu", b) || 0) - (shardValue("cpu", a) || 0));
  for (const sh of hot.slice(0, 2)) {
    const cpu = shardValue("cpu", sh), p = shardValue("p99", sh);
    out.push({ sev: 1, shards: [sh.i], label: `Hot shard ${sh.i}`, text: [cpu != null ? `CPU ${pct0(cpu)}` : null, p != null ? `p99 ${fmt.ms(p)}` : null, sh.slots.length ? `slots ${runsText(sh.slots)}` : null].filter(Boolean).join(" · "), impact: "a search waits for the slowest shard it asks", next: "Move some of its slots to a quieter shard (Draft: drag it onto another)", fix: { kind: "slots", shard: sh.i } });
  }
  const avoided = M.all.filter((c) => c.avoided && !c.down);
  if (avoided.length) out.push({ sev: 1, shards: avoided.map((c) => c.shard), label: `${avoided.length === 1 ? "1 copy" : avoided.length + " copies"} avoided`, text: `${cname(avoided[0])}${avoided[0].avoidedWhy ? ": " + avoided[0].avoidedWhy : " after failures"}`, impact: "the coordinators ask the other copies first", next: "Look at its logs" });
  const firing = M.all.flatMap((c) => (c.alerts?.firing || []).map((a) => ({ c, a })));
  if (firing.length) out.push({ sev: firing.some((x) => x.a.severity === "critical") ? 2 : 1, shards: [...new Set(firing.map((x) => x.c.shard))], label: `${plural(firing.length, "alert")} firing on copies`, text: `${firing[0].a.name || firing[0].a.id} on ${cname(firing[0].c)}`, link: "#/alerts" });
  const two = M.shards.filter((sh) => sh.n === 2);
  if (two.length) out.push({ sev: 0, shards: two.map((sh) => sh.i), label: `${plural(two.length, "shard")} of 2 copies`, text: "with failover on, losing either copy stops its reads and writes", next: "Give each a third copy" });
  const copying = M.all.filter((c) => c.copying);
  if (copying.length) out.push({ sev: 0, shards: copying.map((c) => c.shard), label: `${copying.length === 1 ? "1 copy" : copying.length + " copies"} being copied`, text: copying.map(cname).slice(0, 3).join(", ") });
  return out.sort((a, b) => b.sev - a.sev);
}

// ---------------------------------------------------------------- what if
/** What failing the nodes in `failed` does to each shard. */
function whatIf(M, failed) {
  const out = { dark: [], noQuorum: [], noWrites: [], failover: [], fewer: [], lost: 0, already: [] };
  for (const sh of M.shards) {
    const hit = sh.copies.filter((c) => !c.down && failed.has(c.node || c.host));
    if (!hit.length) continue;
    out.lost += hit.length;
    const up = sh.up - hit.length;
    if (up <= 0) out.dark.push(sh);
    else if (up <= sh.n / 2) {
      // With failover on, a primary also needs a majority to serve reads.
      if (sh.failover === false) out.noWrites.push(sh); else out.noQuorum.push(sh);
      if (sh.up < sh.n) out.already.push(sh);
    } else if (sh.primary && hit.includes(sh.primary)) out.failover.push(sh);
    else out.fewer.push(sh);
  }
  return out;
}

// ---------------------------------------------------------------- moves
/** The job of moves, the same whichever server answered (finding CC-6). */
function movesInfo(m) {
  const job = m?.job || null;
  const moving = m?.moving || null;
  if (!job) return { running: !!moving, moving, job: null };
  const started = job.started_ms ?? (job.started_us ? job.started_us / 1000 : null);
  const finished = job.finished_ms ?? (job.finished_us ? job.finished_us / 1000 : null);
  const rowsDone = job.rows_done ?? job.rows_moved ?? null;
  const rowsTotal = job.rows_total ?? null;
  const elapsed = started ? ((finished || Date.now()) - started) / 1000 : null;
  const rate = job.rate_rows_s ?? (rowsDone && elapsed > 1 ? rowsDone / elapsed : null);
  let eta = job.eta_s ?? null;
  if (eta == null && rate && rowsTotal != null) eta = Math.max(0, (rowsTotal - rowsDone) / rate);
  if (eta == null && job.of && job.done && elapsed) eta = (elapsed / job.done) * (job.of - job.done);
  const held = (job.moves || []).map((x) => x.held_ms).filter((x) => x != null);
  // An older job says no state: it runs until it says when it finished.
  const running = job.state ? job.state === "running" : finished == null;
  return { running, moving, job, started, finished, rowsDone, rowsTotal, rate, eta, by: job.started_by || null, heldMs: held.length ? held.reduce((a, b) => a + b, 0) / held.length : null };
}
/** The rows a second the last move ran at, kept in this browser. */
function measuredRate(mi) {
  if (mi?.rate && mi.rowsDone > 1000) { store.set("cluster.rate", { rate: mi.rate, at: Date.now() }); return { rate: mi.rate, measured: true }; }
  const k = store.get("cluster.rate", null);
  if (k?.rate) return { rate: k.rate, measured: true, at: k.at };
  return { rate: ASSUMED_RATE, measured: false };
}

// ---------------------------------------------------------------- dialogs
/**
 * Asks before a change: focus on Cancel, `typed` must be typed first, and on
 * production (or an unknown environment) a reason the audit log keeps.
 * Resolves false or {reason}.
 */
const ask = (o) => confirm({ reason: isProduction() ? "Why: kept in the audit log with the change" : false, ...o });
const canAdmin = () => can("admin");
const needsAdmin = needs("admin");

// ---------------------------------------------------------------- the view
function render(page, r) {
  if (!isCoordinator() && state.health) return renderNotCoordinator(page);

  const P = readParams(r?.params);
  let mode = P.mode, tab = P.tab, lens = P.lens, group = P.group, sel = P.shard, selCopy = P.copy, filter = P.q;
  let problemsFirst = store.get("cluster.problemsFirst", true);
  let kube = null, moves = null, M = null;
  const failed = new Set(); let failedLabel = null;
  let draft = loadDraft();
  let pop = null; // the open popover {el, shard}
  let dragging = null;
  let serverPlan = null, serverPlanFor = null, kubePlan = null, kubePlanFor = null, kubePlanErr = null;
  let events = null, eventsErr = null, audit = null, auditErr = null, changeKind = "all";
  let alive = true;
  const managed = () => !!kube?.managed;
  const rollout = () => {
    const st = kube?.status || {};
    if (!managed()) return null;
    const pr = st.progress;
    if (pr && pr.total && pr.done < pr.total) return `a rollout runs: ${fmt.n(pr.done)} of ${fmt.n(pr.total)} copies restarted`;
    if ((st.rollout?.stale || []).length && ["Scaling", "Paused"].includes(st.phase)) return `a rollout runs: ${fmt.n(st.rollout.stale.length)} of ${fmt.n(st.rollout.copies || 0)} copies still run older settings`;
    return null;
  };

  const head = h("div", { class: "page-head cc-head" });
  const stripBox = h("div", { class: "cc-strip", role: "list", "aria-label": "Problems" });
  const body = h("div");
  page.append(head, stripBox, body);

  // ------------------------------------------------------------ params
  function writeParams() {
    const q = new URLSearchParams();
    if (mode !== "live") q.set("mode", mode);
    if (tab !== "map") q.set("tab", tab);
    if (lens !== "health") q.set("lens", lens);
    if (group !== "shard") q.set("group", group);
    if (sel != null) q.set("shard", sel);
    if (selCopy) q.set("copy", selCopy);
    if (filter) q.set("q", filter);
    const cur = new URLSearchParams(location.hash.split("?")[1] || "");
    for (const k of ["range", "from", "to"]) if (cur.has(k)) q.set(k, cur.get(k));
    const qs = q.toString();
    try { history.replaceState(null, "", location.pathname + location.search + "#/cluster" + (qs ? "?" + qs : "")); } catch { /* old browser */ }
  }
  active = {
    onParams(r2) {
      const p = readParams(r2?.params);
      mode = p.mode; tab = p.tab; lens = p.lens; group = p.group; sel = p.shard; selCopy = p.copy; filter = p.q;
      drawAll();
    },
  };

  // ------------------------------------------------------------ loading
  async function loadKube() {
    try { kube = await api("/v1/kube"); } catch (e) { kube = { managed: false, error: e }; }
    if (alive) { drawHead(); if (tab === "map") drawSide(); if (tab === "capacity") drawTab(); }
  }
  async function loadMoves() {
    try { moves = movesInfo(await api("/v1/admin/moves")); } catch (e) { moves = { error: e }; }
    if (alive) { drawStrip(); if (tab === "map") drawSide(); }
  }

  // ------------------------------------------------------------ head
  function drawHead() {
    const c = state.cluster;
    const nNodes = M ? [...M.nodes.values()].filter((n) => n.known).length : 0;
    const nZones = M ? [...M.zones.keys()].filter(Boolean).length : 0;
    const counts = M ? M.shards.map((sh) => sh.n) : [];
    const lo = counts.length ? Math.min(...counts) : 0, hi = counts.length ? Math.max(...counts) : 0;
    const sub = M ? [
      M.name ? `${M.name}${M.env ? " (" + M.env + ")" : ""}` : null,
      `${plural(M.shards.length, "shard")} × ${lo === hi ? copies_(lo) : `${lo} to ${hi} copies`}`,
      `${plural(M.all.length, "server")}${nNodes ? ` on ${plural(nNodes, "node")}` : ""}${nZones ? ` in ${plural(nZones, "zone")}` : ""}`,
      M.coordinators ? plural(M.coordinators, "coordinator") : null,
      managed() ? `run by the operator (${kube.namespace}/${kube.name})` : kube ? "on machines of your own" : null,
    ].filter(Boolean).join(" · ") : c ? "" : "Asking the coordinator…";
    const nDraft = draftCount();
    const modeSeg = seg([["live", "Live", "The cluster as it is"], ["draft", nDraft ? `Draft · ${plural(nDraft, "change")}` : "Draft", "Drag to change: nothing applies until you apply the plan"]], mode, (v) => { mode = v; closePop(); writeParams(); drawAll(); }, { "aria-label": "Live or draft", class: "seg cc-mode" + (nDraft ? " drafted" : "") });
    const tabSeg = seg([["map", "Map"], ["capacity", "Capacity"], ["changes", "Changes"]], tab, (v) => { tab = v; closePop(); writeParams(); drawAll(); }, { "aria-label": "Map, capacity or changes" });
    clear(head, h("div", {}, h("h1", {}, "Cluster"), h("p", {}, sub)), h("div", { class: "tools" }, modeSeg, tabSeg));
  }

  // ------------------------------------------------------------ the strip
  function drawStrip() {
    if (!M) { clear(stripBox); return; }
    const ps = problems(M);
    const SHOW = narrow() ? 2 : 3;
    const chips = ps.slice(0, SHOW).map((p) => {
      const b = h("button", { class: "cc-chip " + (p.sev === 2 ? "crit" : p.sev === 1 ? "warn" : "info"), role: "listitem", title: [p.long ? cap(p.long) + "." : null, p.impact ? cap(p.impact) + "." : null, p.next ? p.next + "." : null].filter(Boolean).join(" ") },
        h("span", { class: "dot " + (p.sev === 2 ? "bad" : p.sev === 1 ? "warn" : "") }), h("b", {}, p.label), p.text ? h("span", { class: "dim" }, p.text) : null);
      b.onclick = () => {
        if (p.link) { location.hash = p.link; return; }
        if (p.shards.length === 1) select(p.shards[0]);
        else { filter = p.shards.length <= 12 ? p.shards.map((i) => "s" + i).join(" ") : ""; problemsFirst = true; if (p.shards.length) select(p.shards[0]); else drawAll(); }
      };
      return b;
    });
    const serving = M.all.filter((c) => !c.down).length;
    chips.push(h("span", { class: "cc-chip plain", role: "listitem", title: "Copies that answer the coordinator now" }, h("span", { class: "dot " + (serving === M.all.length ? "ok" : "warn") }), `${fmt.n(serving)} of ${plural(M.all.length, "copy", "copies")} serving`));
    if (ps.length > SHOW) chips.push(h("button", { class: "cc-chip plain more", title: ps.slice(SHOW).map((p) => p.label).join("; "), onclick: () => { sel = null; mode = "live"; tab = "map"; writeParams(); drawAll(); side.scrollTop = 0; } }, `${ps.length - SHOW} more in What needs attention`));
    chips.push(changeChip());
    clear(stripBox, chips);
  }
  function changeChip() {
    const mi = moves && !moves.error ? moves : null;
    if (runner && !runner.done) return h("button", { class: "cc-chip info", onclick: () => { tab = "map"; drawAll(); } }, h("span", { class: "spin" }), h("b", {}, "Applying a plan"), h("span", { class: "dim" }, `step ${Math.min(runner.at + 1, runner.steps.length)} of ${runner.steps.length}`));
    if (mi?.running) return h("span", { class: "cc-chip info" }, h("span", { class: "spin" }), h("b", {}, "Slots moving"), h("span", { class: "dim" }, [mi.job ? `${fmt.n(mi.job.done)} of ${fmt.n(mi.job.of)} moves` : "a move under way", mi.eta != null ? `about ${dur(mi.eta)} left` : null, mi.by ? `by ${mi.by}` : null].filter(Boolean).join(" · ")));
    const ro = rollout();
    if (ro) return h("a", { class: "cc-chip info", href: "#/rollout" }, h("span", { class: "spin" }), h("b", {}, cap(ro.split(":")[0])), h("span", { class: "dim" }, ro.split(": ")[1] || ""));
    if (mi?.job) return h("span", { class: "cc-chip plain", title: mi.job.error || "" }, `No change running · last: ${jobWords(mi.job)} ${mi.job.state}${mi.finished ? " " + clock(mi.finished) : ""}`);
    return h("span", { class: "cc-chip plain" }, "No change running");
  }
  const jobWords = (j) => (j.kind === "rebalance" ? "evening out the slots" : /^drain/.test(j.kind) ? j.kind.replace("drain", "draining") : j.kind === "move" ? `moving ${fmt.n(j.moves?.[0]?.count || 0)} slots` : j.kind);

  // ------------------------------------------------------------ layout
  const left = h("div", { class: "cc-left" }), side = h("aside", { class: "cc-side", "aria-label": "Details" });
  const mapCard = h("section", { class: "card cc-map" });
  const toolbar = h("div", { class: "cc-toolbar" });
  const legendBox = h("div", { class: "cc-legend" });
  const attention = h("div", { class: "cc-attn" });
  const canvas = h("div", { class: "cc-canvas" });
  mapCard.append(h("div", { class: "bd" }, toolbar, legendBox, attention, canvas));
  left.append(mapCard);

  function drawAll() {
    if (tab !== "map") { sheet(null); bottom(null); }
    drawHead(); drawStrip();
    if (tab === "map") {
      clear(body, h("div", { class: "cc-main" + (mode === "draft" ? " drafting" : "") }, left, side));
      drawToolbar(); drawMap(); drawSide();
    } else drawTab();
  }

  // ------------------------------------------------------------ toolbar
  const filterInp = h("input", { class: "input sm cc-find", type: "search", placeholder: "Find a shard, copy, node, zone or version", "aria-label": "Find a shard, copy, node, zone or version" });
  filterInp.addEventListener("input", debounce(() => { filter = filterInp.value.trim(); writeParams(); drawMap(); }, 150));
  filterInp.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { const hit = matches(); if (hit.length === 1) select(hit[0]); }
    if (e.key === "Escape") { filterInp.value = ""; filter = ""; writeParams(); drawMap(); }
  });
  let pfChip = null;
  function drawToolbar() {
    if (document.activeElement !== filterInp) filterInp.value = filter;
    const setLens = (v) => { lens = v; store.set("cluster.lens", v); writeParams(); drawMap(); if (narrow()) drawSide(); };
    const setGroup = (v) => { group = v; writeParams(); drawMap(); };
    if (narrow()) {
      const pick = (label, opts, v, f) => h("label", { class: "cc-pick" }, h("span", {}, label), h("select", { class: "select sm", onchange: (e) => f(e.target.value) }, opts.map(([k, l]) => h("option", { value: k, selected: k === v }, l))));
      pfChip = null;
      clear(toolbar, pick("Colour", Object.entries(LENSES).map(([k, L]) => [k, L.label]), lens, setLens), pick("Group", [["shard", "Shard"], ["node", "Node"], ["zone", "Zone"]], group, setGroup), filterInp, mode === "draft" && managed() ? kubeSteppers() : null);
      return;
    }
    const lensSeg = seg(Object.entries(LENSES).map(([k, L]) => [k, L.label, L.title]), lens, setLens, { "aria-label": "Colour by" });
    const groupSeg = seg([["shard", "Shard"], ["node", "Node"], ["zone", "Zone"]], group, (v) => { group = v; writeParams(); drawMap(); }, { "aria-label": "Group by" });
    const pf = h("button", { class: "chip", "aria-pressed": String(problemsFirst), title: "Put the shards that need attention first", onclick: () => { problemsFirst = !problemsFirst; store.set("cluster.problemsFirst", problemsFirst); drawToolbar(); drawMap(); } }, "Problems first");
    pfChip = group === "shard" ? pf : null;
    const parts = [lensSeg, groupSeg, filterInp];
    if (mode === "draft" && managed()) parts.push(kubeSteppers());
    clear(toolbar, parts);
  }
  function kubeSteppers() {
    const sp = kube.spec || {};
    const S = draft.kube.shards ?? sp.shards ?? M?.shards.length ?? 1, R = draft.kube.replicas ?? sp.replicas ?? 3;
    const step = (label, v, f, min, max, title) => h("span", { class: "cc-step", title },
      h("span", { class: "small dim" }, label),
      h("button", { class: "btn xs", "aria-label": "Fewer " + label.toLowerCase(), disabled: v <= min, onclick: () => f(v - 1) }, "−"),
      h("b", {}, fmt.n(v)),
      h("button", { class: "btn xs", "aria-label": "More " + label.toLowerCase(), disabled: v >= max, onclick: () => f(v + 1) }, "+"));
    return h("span", { class: "row nw", style: { gap: "10px" } },
      step("Shards", S, (v) => { draft.kube.shards = v === sp.shards ? null : v; changed(); }, 1, 4096, "spec.shards: new shards are added last, and shards leave from the last"),
      step("Copies per shard", R, (v) => { draft.kube.replicas = v === sp.replicas ? null : v; changed(); }, 1, 9, "spec.replicas: the same for every shard on Kubernetes"));
  }

  function legend() {
    const L = LENSES[lens];
    const sw = (cls, text) => h("span", { class: "row nw", style: { gap: "5px" } }, h("i", { class: "cc-sw " + cls }), text);
    const pip = (cls, text) => h("span", { class: "row nw", style: { gap: "4px" } }, h("i", { class: "cc-pip " + cls }), text);
    let scale;
    if (lens === "health") scale = [sw("l1", "a copy down, behind, avoided, full or slow"), sw("l2", "no reads or no writes")];
    else if (lens === "version") scale = (M?.versions || []).slice(0, 5).map(([v, n], k) => sw(k ? "v" + Math.min(k, 4) : "l0", `${v} · ${fmt.n(n)}`));
    else scale = [sw("l0", `under ${L.fmt(L.warn())}`), sw("l1", `${L.fmt(L.warn())}+`), sw("l2", `${L.fmt(L.bad())}+${lens === "p99" ? " (the SLO)" : ""}`)];
    const what = { health: "Copies answering, when one is missing", version: "The version the copies run", cpu: "CPU of the busiest copy, last minute", disk: "Disk used on the fullest copy", p99: "p99 of the slowest copy, last minute", lag: "Transactions the furthest replica is behind" }[lens];
    return [h("b", { class: "dim", style: { fontWeight: 500 } }, what), ...scale, h("span", { class: "row nw", style: { gap: "8px" } }, pip("p", "primary"), pip("", "replica"), pip("down", "down"),
      mode === "draft" ? [pip("new", "in the plan"), pip("going", "goes")] : null),
      lens !== "health" ? h("span", { title: "Whatever is wrong with a shard outlines its tile under every lens" }, "outline: health") : null,
      h("span", { class: "grow" }), pfChip];
  }

  // ------------------------------------------------------------ the map
  function matches() {
    if (!M || !filter) return M ? M.shards.map((x) => x.i) : [];
    const words = filter.toLowerCase().split(/[\s,]+/).filter(Boolean);
    return M.shards.filter((sh) => words.some((w) => {
      const m = /^(?:s|shard)?(\d+)$/.exec(w);
      if (m && +m[1] === sh.i) return true;
      return sh.copies.some((c) => [c.id, c.name, c.node, c.zone, c.version, short(c.url)].some((x) => x && String(x).toLowerCase().includes(w)));
    })).map((x) => x.i);
  }

  let tileEls = new Map(); // shard -> tile, kept between polls so clicks land (finding CL-9)
  let gridEl = null;
  function drawMap() {
    // Grey the map when the coordinator stopped answering, and say how old it is.
    const fr = fresh("cluster");
    mapCard.classList.toggle("stale", !!M && fr.ok === false);
    clear(legendBox, legend(), M && fr.ok === false ? h("span", { class: "stale-age" }, staleText("cluster")) : null);
    if (!M) { clear(canvas, empty("layers", "Asking the shards", "")); clear(attention); return; }
    const hits = filter ? new Set(matches()) : null;
    const fate = failed.size ? fateMap() : null;
    if (group === "shard") drawShardGrid(hits, fate);
    else { tileEls = new Map(); gridEl = null; clear(attention); drawDomains(hits, fate); }
    drawWhatIf();
  }

  function fateMap() {
    const w = whatIf(M, failed), f = new Map();
    for (const sh of w.fewer) f.set(sh.i, "fewer");
    for (const sh of w.failover) f.set(sh.i, "failover");
    for (const sh of [...w.noQuorum, ...w.noWrites]) f.set(sh.i, "noquorum");
    for (const sh of w.dark) f.set(sh.i, "dark");
    return f;
  }

  /** The draft's view of a shard: its copies with what is new and what goes. */
  function draftShard(sh) {
    const goes = new Set(draftRemovals().filter((x) => x.shard === sh.i).map((x) => x.url));
    const adds = mode === "draft" && !managed() ? draft.adds.filter((a) => a.shard === sh.i) : [];
    const dropped = mode === "draft" && (managed() ? sh.i >= (draft.kube.shards ?? Infinity) : draft.drops.includes(sh.i));
    let kubeNew = 0, kubeGo = 0;
    if (mode === "draft" && managed() && draft.kube.replicas != null) { const d = draft.kube.replicas - (kube.spec?.replicas ?? sh.n); if (d > 0) kubeNew = d; else kubeGo = -d; }
    return { goes, adds, dropped, kubeNew, kubeGo };
  }

  function pips(sh, L) {
    const D = mode === "draft" ? draftShard(sh) : null;
    const list = sh.copies.map((c, k) => {
      let cls = "cc-pip" + (c.primary ? " p" : "") + (c.down ? " down" : c.copying ? " copying" : "");
      if (L && L.copy && !c.down) { const lv = level(lens, L.copy(c)); if (lv) cls += " l" + lv; }
      if (lens === "version" && c.version && c.version !== M.mainVersion) cls += " v" + Math.min(4, Math.max(1, M.versions.findIndex(([v]) => v === c.version)));
      if (D && (D.goes.has(c.url) || D.dropped || (D.kubeGo && k >= sh.n - D.kubeGo))) cls += " going";
      return h("i", { class: cls, dataset: { url: c.url }, title: `${cname(c)}: ${c.down ? "down" : c.role}${c.node ? " on " + c.node : ""}` });
    });
    if (D) {
      for (const a of D.adds) list.push(h("i", { class: "cc-pip new", title: a.replaces ? "replaces " + a.replaces : "a new copy" }));
      for (let k = 0; k < D.kubeNew; k++) list.push(h("i", { class: "cc-pip new", title: "a new copy" }));
    }
    return list;
  }

  function tileValue(sh) {
    if (lens === "health") return sh.up < sh.n ? `${sh.up}/${sh.n}` : "";
    if (lens === "version") { const vs = [...new Set(sh.copies.map((c) => c.version).filter(Boolean))]; return vs.length === 1 ? vs[0] : vs.length ? "mixed" : "–"; }
    const v = shardValue(lens, sh);
    return v == null ? "–" : LENSES[lens].fmt(v);
  }
  function tileClass(sh, hits, fate) {
    const L = LENSES[lens];
    const hl = healthLevel(sh);
    let cls = "cc-t";
    if (lens === "health") cls += " l" + hl;
    else if (lens === "version") {
      const vs = sh.copies.map((c) => c.version).filter(Boolean);
      const other = vs.find((v) => v !== M.mainVersion);
      cls += other ? " v" + Math.min(4, Math.max(1, M.versions.findIndex(([v]) => v === other))) : " l0";
    } else {
      const lv = level(lens, shardValue(lens, sh));
      cls += lv == null ? " lnone" : " l" + lv;
      if (lv === 0 && shardValue(lens, sh) >= L.warn() * 0.6) cls += " busier";
    }
    if (lens !== "health" && hl) cls += " o" + hl;
    if (sel === sh.i) cls += " sel";
    if (hits && !hits.has(sh.i)) cls += " dim";
    if (fate?.has(sh.i)) cls += " f-" + fate.get(sh.i);
    if (mode === "draft") {
      const D = draftShard(sh);
      if (D.dropped) cls += " going";
      if (draft.moves?.some((m) => m.from === sh.i || m.to === sh.i)) cls += " moving";
    }
    return cls;
  }
  function tileLabel(sh) {
    const is = issues(sh);
    return `Shard ${sh.i}: ${LENSES[lens].label} ${tileValue(sh)}; ${sh.up} of ${sh.n} copies serving${is.length ? "; " + is.map((x) => x.long).join("; ") : ""}`;
  }

  function makeTile(sh) {
    const t = h("button", { class: "cc-t", type: "button", dataset: { i: sh.i } },
      h("span", { class: "cc-tn" }, h("span", { class: "n" }), h("span", { class: "x" })),
      h("span", { class: "cc-tv" }), h("span", { class: "cc-tp" }));
    return t;
  }
  function updateTile(t, sh, hits, fate) {
    t.className = tileClass(sh, hits, fate);
    t.querySelector(".cc-tn .n").textContent = sh.i;
    const is = issues(sh)[0];
    t.querySelector(".cc-tn .x").textContent = is && is.sev && lens !== "health" ? "●" : "";
    t.querySelector(".cc-tv").textContent = tileValue(sh);
    const tp = t.querySelector(".cc-tp");
    clear(tp, pips(sh, LENSES[lens]));
    tp.classList.toggle("many", tp.children.length > 3);
    t.setAttribute("aria-label", tileLabel(sh));
    t.title = tileLabel(sh);
  }

  function drawShardGrid(hits, fate) {
    // Problems first: the shards that need attention, as wider cards.
    const attn = problemsFirst ? M.shards.map((sh) => [sh, issues(sh)]).filter(([, is]) => is.some((x) => x.sev > 0)).sort((a, b) => b[1][0].sev - a[1][0].sev || a[0].i - b[0].i) : [];
    clear(attention, attn.length ? [
      h("div", { class: "cc-sec" }, `Needs attention · ${attn.length}`),
      h("div", { class: "cc-attn-row" }, attn.slice(0, narrow() ? 12 : 5).map(([sh, is]) => {
        const b = h("button", { class: "cc-ac " + (is[0].sev === 2 ? "crit" : "warn") + (sel === sh.i ? " sel" : ""), type: "button", title: is.map((x) => x.long).join("; "), onclick: () => select(sh.i) },
          h("span", { class: "row nw between" }, h("b", { class: "nowrap" }, `shard ${sh.i}`), (() => { const p = pips(sh, LENSES[lens]); return h("span", { class: "cc-tp" + (p.length > 3 ? " many" : "") }, p); })()),
          h("span", { class: "cc-ac-why" }, is[0].short + (is.length > 1 ? ` +${is.length - 1}` : "")),
          h("span", { class: "small muted" }, lens === "health" ? `${sh.up} of ${sh.n} serving` : `${LENSES[lens].label} ${tileValue(sh)}`));
        return b;
      }), attn.length > (narrow() ? 12 : 5) ? h("button", { class: "cc-ac more", onclick: () => { filter = attn.map(([sh]) => "s" + sh.i).join(" "); writeParams(); drawToolbar(); drawMap(); } }, `+${attn.length - (narrow() ? 12 : 5)} more`, h("span", { class: "small muted" }, "show them")) : null),
      h("div", { class: "cc-sec" }, `All ${plural(M.shards.length, "shard")}, by number`)] : []);

    if (!gridEl || !gridEl.isConnected) {
      gridEl = h("div", { class: "cc-grid", role: "group", "aria-label": "Shards" });
      tileEls = new Map();
      wireGrid(gridEl);
    }
    const n = M.shards.length;
    gridEl.style.setProperty("--tile", n <= 4 ? (narrow() ? "100%" : "220px") : n <= 12 ? "150px" : n <= 48 ? "60px" : narrow() ? "40px" : "37px");
    gridEl.classList.toggle("big", n <= 12);
    const seen = new Set();
    for (const sh of M.shards) {
      let t = tileEls.get(sh.i);
      if (!t) { t = makeTile(sh); tileEls.set(sh.i, t); }
      updateTile(t, sh, hits, fate);
      if (n <= 12) bigTile(t, sh);
      seen.add(sh.i);
    }
    for (const [i, t] of tileEls) if (!seen.has(i)) { t.remove(); tileEls.delete(i); }
    const order = M.shards.map((sh) => tileEls.get(sh.i));
    // Draft: new shards after the last, and the end handle that adds them.
    const extra = [];
    if (mode === "draft") {
      const k = managed() ? Math.max(0, (draft.kube.shards ?? M.shards.length) - M.shards.length) : draft.newShards.length;
      for (let j = 0; j < k; j++) {
        const ns = !managed() ? draft.newShards[j] : null;
        extra.push(h("button", { class: "cc-t new", type: "button", title: `New shard ${M.shards.length + j}: ${copies_(ns ? ns.servers.length : (draft.kube.replicas ?? kube?.spec?.replicas ?? 3))}, in the plan`, onclick: () => { if (!managed()) openNewShardPop(j, extra[j]); } },
          h("span", { class: "cc-tn" }, h("span", { class: "n" }, M.shards.length + j)), h("span", { class: "cc-tv" }, "new"),
          h("span", { class: "cc-tp" }, Array.from({ length: ns ? ns.servers.length : (draft.kube.replicas ?? kube?.spec?.replicas ?? 3) }, () => h("i", { class: "cc-pip new" })))));
      }
      extra.push(endHandle());
    }
    const want = [...order, ...extra];
    if (gridEl.children.length !== want.length || want.some((el, k) => gridEl.children[k] !== el)) gridEl.replaceChildren(...want);
    if (canvas.firstChild !== gridEl || canvas.children.length !== 1) clear(canvas, gridEl);
  }

  /** A tile with room for the copies' names (a cluster of a few shards). */
  function bigTile(t, sh) {
    let box = t.querySelector(".cc-tc");
    if (!box) { box = h("span", { class: "cc-tc" }); t.append(box); }
    const L = LENSES[lens];
    clear(box, sh.copies.map((c) => h("span", { class: "cc-tcr" + (c.down ? " down" : "") },
      h("i", { class: "cc-pip" + (c.primary ? " p" : "") + (c.down ? " down" : c.copying ? " copying" : "") }),
      h("span", { class: "mono" }, cname(c)),
      h("span", { class: "muted" }, c.down ? "down" : c.primary ? "primary" : c.copying ? "copying" : c.lag ? `${kn(c.lag)} behind` : "in sync"),
      L.copy && !c.down ? h("span", { class: "muted" }, L.fmt(L.copy(c)) ?? "") : null)));
    t.querySelector(".cc-tv").textContent = `${tileValue(sh)}${lens === "health" ? " serving" : ""}`;
    t.querySelector(".cc-tn .n").textContent = `shard ${sh.i}`;
  }

  function endHandle() {
    const b = h("button", { class: "cc-t handle", type: "button", title: "Drag to add shards, or click to add one. Nothing applies until you apply the plan.", "aria-label": "Add a shard" }, h("span", { class: "cc-tv" }, "+"), h("span", { class: "cc-hl" }, "shard"));
    b.addEventListener("click", () => { if (b.dataset.dragged) { delete b.dataset.dragged; return; } addShards(1); });
    b.addEventListener("pointerdown", (e) => startDrag(e, { kind: "shards", el: b }));
    return b;
  }

  function wireGrid(g) {
    g.addEventListener("click", (e) => {
      const t = e.target.closest(".cc-t[data-i]");
      if (!t || t.dataset.dragged) { if (t) delete t.dataset.dragged; return; }
      const i = +t.dataset.i;
      if (mode === "draft") openPop(i, t); else select(sel === i ? null : i);
    });
    g.addEventListener("pointerdown", (e) => {
      if (mode !== "draft") return;
      const pip = e.target.closest(".cc-pip[data-url]");
      const t = e.target.closest(".cc-t[data-i]");
      if (!t) return;
      if (pip && !managed()) startDrag(e, { kind: "copy", from: +t.dataset.i, url: pip.dataset.url, el: t });
      else if (!managed()) startDrag(e, { kind: "slots", from: +t.dataset.i, el: t });
    });
  }

  // ------------------------------------------------------------ nodes and zones
  function drawDomains(hits, fate) {
    const L = LENSES[lens];
    const cell = (c) => {
      let cls = "cc-cell" + (c.primary ? " p" : "") + (c.down ? " down" : "");
      const lv = c.down ? null : lens === "health" ? healthLevel(M.shards.find((x) => x.i === c.shard)) : L.copy ? level(lens, L.copy(c)) : null;
      if (lv) cls += " l" + lv;
      if (lens === "version" && c.version && c.version !== M.mainVersion) cls += " v" + Math.min(4, Math.max(1, M.versions.findIndex(([v]) => v === c.version)));
      if (hits && !hits.has(c.shard)) cls += " dim";
      if (sel === c.shard) cls += " sel";
      if (mode === "draft" && draftRemovals().some((x) => x.url === c.url)) cls += " going";
      const e = h("button", { class: cls, type: "button", dataset: { i: c.shard, url: c.url }, "aria-label": `${cname(c)}, shard ${c.shard} ${c.down ? "down" : c.role}` });
      tipOn(e, () => h("div", {}, h("b", {}, cname(c)), h("div", { class: "small" }, `shard ${c.shard} · ${c.down ? "down " + sinceText(c.downSince) : c.role}`), L.copy && !c.down ? h("div", { class: "small" }, `${L.label} ${L.copy(c) == null ? "–" : L.fmt(L.copy(c))}`) : null, h("div", { class: "small muted" }, short(c.url))));
      return e;
    };
    const failBtn = (keys, label, small) => {
      const on = keys.length && keys.every((k) => failed.has(k));
      return h("button", { class: (small ? "cc-fail" : "btn xs") + (on ? " on" : ""), title: on ? `Bring ${label} back in the what-if` : `What if ${label} failed? A what-if: nothing is stopped.`, "aria-pressed": String(!!on), onclick: (e) => { e.stopPropagation(); if (on) keys.forEach((k) => failed.delete(k)); else { failed.clear(); keys.forEach((k) => failed.add(k)); } failedLabel = on ? null : label; drawMap(); drawSide(); } }, small ? nodeShort(keys[0]) : on ? "Restore" : "Fail it");
    };
    const nodeShort = (k) => (M.nodePrefix && k.startsWith(M.nodePrefix) ? "…" + k.slice(M.nodePrefix.length) : nodeLabel(k));
    const nodeBox = (n) => h("div", { class: "cc-node" + (n.copies.every((c) => failed.has(c.node || c.host)) ? " failed" : ""), dataset: { node: n.key } },
      h("div", { class: "cc-cells" }, n.copies.slice().sort((a, b) => a.shard - b.shard).map(cell)),
      failBtn([n.key], n.known ? `node ${n.key}` : n.key, true));
    let content;
    const unknownNote = !M.nodesKnown ? h("div", { class: "notice", style: { marginBottom: "10px" } }, "The copies report no node (STRATA_NODE or NODE_NAME): they are grouped by the host in their URL, which on one machine is the same for all.") : null;
    if (group === "node") {
      const ns = [...M.nodes.values()].sort((a, b) => a.key.localeCompare(b.key, undefined, { numeric: true }));
      content = [unknownNote, h("div", { class: "cc-nodes" }, ns.map(nodeBox))];
    } else {
      const zs = [...M.zones.values()].sort((a, b) => String(a.key ?? "~").localeCompare(String(b.key ?? "~")));
      content = [
        !M.zonesKnown ? h("div", { class: "notice", style: { marginBottom: "10px" } }, "The copies report no zone (STRATA_ZONE): every node is in one group.") : unknownNote,
        h("div", { class: "cc-zones", style: { "--zones": Math.min(3, zs.length) } }, zs.map((z) => {
          const keys = z.nodes.map((n) => n.key);
          const on = keys.every((k) => failed.has(k));
          return h("div", { class: "cc-zone" + (on ? " failed" : "") },
            h("div", { class: "cc-zone-h" }, h("b", { class: "nowrap" }, z.key ? `zone ${z.key}` : "zone unknown"), h("span", { class: "small muted ellipsis" }, `${plural(z.nodes.length, "node")} · ${copies_(z.copies.length)}`), h("span", { class: "grow" }), failBtn(keys, z.key ? `zone ${z.key}` : "every node")),
            h("div", { class: "cc-nodes" }, z.nodes.sort((a, b) => a.key.localeCompare(b.key, undefined, { numeric: true })).map(nodeBox)));
        })),
      ];
    }
    const box = h("div", { class: "cc-domains" }, h("div", { class: "small muted", style: { marginBottom: "8px" } }, `Each square is a copy (round: a primary), on the ${group === "zone" ? "node and zone" : "node"} it reports. Click a node's name, or Fail it on a zone, to see what its failure would do: nothing is stopped.`), content);
    box.addEventListener("click", (e) => { const c = e.target.closest(".cc-cell"); if (c) { selCopy = c.dataset.url; select(+c.dataset.i); } });
    box.addEventListener("pointerdown", (e) => {
      const c = e.target.closest(".cc-cell");
      if (mode === "draft" && c && !managed()) startDrag(e, { kind: "copy", from: +c.dataset.i, url: c.dataset.url, el: c });
    });
    clear(canvas, box);
  }

  // ------------------------------------------------------------ what if
  function drawWhatIf() { /* the what-if is a card on the side */ }
  function whatIfCard() {
    if (!failed.size || !M) return null;
    const w = whatIf(M, failed);
    const live = M.all.filter((c) => !c.down).length;
    const share = live ? w.lost / live : 0;
    const cpus = M.all.filter((c) => !c.down && c.cpu != null).map((c) => c.cpu);
    const mean = cpus.length ? cpus.reduce((a, b) => a + b, 0) / cpus.length : null;
    // The failover wait: the operator's spec, else what the primaries say they wait.
    const fos = [...new Set((state.cluster?.shards || []).flatMap((sh) => sh.replicas || []).map((r) => r.detail?.capability?.failover_after_ms).filter((v) => v > 0))];
    const fo = kube?.spec?.failoverAfterMs || (fos.length ? Math.max(...fos) : null);
    const ids = (list) => list.length ? h("span", { class: "muted" }, ` (${list.slice(0, 8).map((sh) => sh.i).join(", ")}${list.length > 8 ? "…" : ""})`) : null;
    const line = (list, one, many, cls) => list.length ? h("div", { class: "cc-wi-l " + cls }, h("b", {}, fmt.n(list.length)), h("span", {}, list.length === 1 ? one : many, ids(list))) : null;
    const quiet = M.shards.length - w.dark.length - w.noQuorum.length - w.noWrites.length - w.failover.length - w.fewer.length;
    return h("section", { class: "card cc-wi" + (w.dark.length || w.noQuorum.length || w.noWrites.length ? " bad" : "") },
      h("div", { class: "hd" }, h("h3", {}, `If ${failedLabel || plural(failed.size, "node")} failed`), h("span", { class: "sub" }, "what-if: nothing is stopped"), h("div", { class: "tools" }, h("button", { class: "btn sm", onclick: () => { failed.clear(); failedLabel = null; drawMap(); drawSide(); } }, "Restore"))),
      h("div", { class: "bd stack", style: { gap: "6px" } },
        line(w.dark, "shard would go dark: no copy left", "shards would go dark: no copy left", "crit"),
        line(w.noQuorum, "shard would serve no reads and take no writes: too few copies for a majority, and with failover on a primary needs one to serve reads", "shards would serve no reads and take no writes: too few copies for a majority, and with failover on a primary needs one to serve reads", "crit"),
        line(w.noWrites, "shard would take no writes: too few copies for a majority; reads go on", "shards would take no writes: too few copies for a majority; reads go on", "crit"),
        w.already.length ? h("div", { class: "small cc-bad" }, `${w.already.length === 1 ? "It is" : `${w.already.length} of them are`} already a copy short: ${w.already.slice(0, 4).map((sh) => `shard ${sh.i} (${sh.copies.filter((c) => c.down).map(cname).join(", ")} down)`).join(", ")}.`) : null,
        line(w.failover, `shard would lose its primary and fail over${fo ? ` in about ${dur(fo / 1000 + 1)}` : " in a few seconds"}: writes pause, reads go on`, `shards would lose their primary and fail over${fo ? ` in about ${dur(fo / 1000 + 1)}` : " in a few seconds"}: writes pause, reads go on`, "warn"),
        line(w.fewer, "shard would serve with a copy fewer", "shards would serve with a copy fewer", ""),
        h("div", { class: "small dim" }, `${quiet === 1 ? "1 other shard" : `${fmt.n(quiet)} other shards`} would not notice. The cluster would lose ${pct0(share)} of its serving copies${mean != null && share < 1 && share > 0 ? `; the others would take their searches, at about ${pct0(Math.min(1, mean / (1 - share)))} CPU on average (${pct0(mean)} now)` : ""}.`)));
  }

  // ------------------------------------------------------------ selecting
  function select(i) {
    sel = i;
    if (i == null) selCopy = null;
    writeParams();
    if (tab !== "map") { tab = "map"; drawAll(); return; }
    drawMap(); drawSide();
    if (i != null) tileEls.get(i)?.scrollIntoView?.({ block: "nearest" });
  }

  // ------------------------------------------------------------ side panel
  function drawSide() {
    if (!M) { clear(side, card({ body: empty("layers", "Asking the shards", "") })); return; }
    const parts = [];
    const run = progressCard();
    if (run) parts.push(run);
    const wi = mode === "live" ? whatIfCard() : null;
    const shardSel = mode === "live" && sel != null ? M.shards.find((x) => x.i === sel) : null;
    if (!narrow()) {
      if (wi) parts.push(wi);
      if (mode === "draft") parts.push(planCard());
      else if (shardSel) parts.push(shardCard(shardSel));
      else if (!wi) parts.push(attentionCard());
      clear(side, parts);
      sheet(null); bottom(null);
      return;
    }
    // A phone: what needs attention under the map; a shard, or the plan, as a
    // full-screen sheet; the what-if and the plan's summary as a bottom sheet.
    if (mode === "live") parts.push(attentionCard());
    clear(side, parts);
    if (shardSel) sheet(`Shard ${shardSel.i}`, shardCard(shardSel), () => select(null));
    else if (mode === "draft" && planOpen) sheet("Plan", planCard(), () => { planOpen = false; drawSide(); });
    else sheet(null);
    bottom(mode === "live" ? wi : mode === "draft" && !planOpen && tab === "map" ? planBar() : null);
  }

  // ------------------------------------------------------------ sheets (a phone)
  let sheetEl = null, bottomEl = null, planOpen = false;
  function sheet(title, content, onClose) {
    if (!content) { sheetEl?.remove(); sheetEl = null; return; }
    const keep = sheetEl?.dataset.title === title ? sheetEl.querySelector(".cc-sheet-bd")?.scrollTop || 0 : 0;
    if (!sheetEl) { sheetEl = h("div", { class: "cc-sheet", role: "dialog", "aria-modal": "true" }); document.body.append(sheetEl); }
    sheetEl.dataset.title = title;
    sheetEl.setAttribute("aria-label", title);
    const bd = h("div", { class: "cc-sheet-bd" }, content);
    clear(sheetEl, h("div", { class: "cc-sheet-hd" }, h("button", { class: "btn ghost", onclick: onClose }, icon("left"), "Map"), h("b", {}, title)), bd);
    bd.scrollTop = keep;
  }
  function bottom(content) {
    if (!content) { bottomEl?.remove(); bottomEl = null; return; }
    if (!bottomEl) { bottomEl = h("div", { class: "cc-bsheet" }); document.body.append(bottomEl); }
    const keep = bottomEl.scrollTop;
    clear(bottomEl, content);
    bottomEl.scrollTop = keep;
  }
  /** The plan's summary at the bottom of a phone's screen. */
  function planBar() {
    const n = draftCount();
    let sub = "Tap a shard to change it. Nothing applies until you apply the plan.";
    if (n && !managed()) { const P = ownPlan(); sub = `${plural(P.steps.length, "step")} · about ${dur(planTime(P).estimate)}${P.pre.some((x) => x.ok === false) ? " · not ready" : ""}`; }
    else if (n) sub = Object.entries(kubePatch()).map(([k, v]) => `${k} → ${v}`).join(" · ");
    return h("div", { class: "cc-planbar" }, h("div", { class: "grow" }, h("b", {}, n ? `Plan · ${plural(n, "change")}` : "Plan"), h("div", { class: "small dim" }, sub)),
      h("button", { class: "btn primary", onclick: () => { planOpen = true; drawSide(); } }, n ? "Review" : "How"));
  }

  function attentionCard() {
    const ps = problems(M);
    return card({
      title: "What needs attention", sub: ps.length ? `${plural(ps.length, "thing")}, worst first` : "nothing",
      body: ps.length ? h("div", { class: "stack", style: { gap: "10px" } }, ps.map((p) => h("div", { class: "cc-prob " + (p.sev === 2 ? "crit" : p.sev === 1 ? "warn" : "info") },
        h("div", { class: "row nw", style: { gap: "8px", alignItems: "flex-start" } }, h("span", { class: "dot " + (p.sev === 2 ? "bad" : p.sev === 1 ? "warn" : "") , style: { marginTop: "6px" } }),
          h("div", { class: "grow" },
            h("b", {}, p.label), p.text ? h("div", { class: "small dim" }, p.text) : null,
            p.long ? h("div", { class: "small dim" }, cap(p.long) + ".") : null,
            p.impact ? h("div", { class: "small" }, cap(p.impact) + ".") : null,
            p.since && Number.isFinite(p.since) ? h("div", { class: "small muted" }, cap(sinceText(p.since))) : null,
            p.next ? h("div", { class: "small", style: { marginTop: "4px" } }, h("span", { class: "muted" }, "Next: "), p.next + ".") : null,
            h("div", { class: "row", style: { gap: "6px", marginTop: "6px" } },
              p.shards.length ? h("button", { class: "btn xs", onclick: () => select(p.shards[0]) }, p.shards.length === 1 ? `Open shard ${p.shards[0]}` : `Open shard ${p.shards[0]}, the first`) : null,
              p.fix && !managed() ? h("button", { class: "btn xs", onclick: () => planFix(p.fix) }, p.fix.kind === "slots" ? "Move some slots…" : p.fix.kind === "repair" ? "Plan the repair" : "Plan replacements") : null,
              p.link ? h("a", { class: "btn xs", href: p.link }, "Open") : null))))))
        : empty("okc", "Nothing needs attention", "Every copy answers, the replicas keep up, the disks have room and no shard is hot."),
      foot: h("span", {}, "Pick a shard for its copies; switch to Node or Zone to fail one as a what-if."),
    });
  }

  // ------------------------------------------------------------ one shard
  function shardCard(sh) {
    const is = issues(sh);
    const sev = is[0]?.sev || 0;
    const ver = [...new Set(sh.copies.map((c) => c.version).filter(Boolean))];
    const copyLink = () => { copy(location.href, "Copied a link to shard " + sh.i); };
    // What is wrong, in words: the server's why only when the shard is not
    // ok (it says "2 of 2 copies answer" of a shard that is fine), with the
    // rest of its issues after it.
    const whyText = is.map((x) => String(x.long || x.short).replace(/\.$/, "")).join("; ");
    const why = is.length ? (sh.copies.some((c) => whyText.startsWith(cname(c))) ? whyText : cap(whyText)) + "." : null;
    const statePill = CRIT.has(sh.state) ? pill("bad", sh.state === "down" ? "down" : noReads(sh) ? "no reads, no writes" : "no writes") : sh.state === "degraded" ? pill("warn", `${sh.up} of ${sh.n} copies`) : pill("ok", `${sh.n} of ${sh.n} copies`);
    const rowsDown = sh.copies.filter((c) => c.down);
    return h("section", { class: "card cc-shard" },
      h("div", { class: "hd" }, h("h3", {}, `Shard ${sh.i}`), statePill, h("div", { class: "tools" },
        h("button", { class: "btn ghost icon sm", title: "Copy a link to this shard", "aria-label": "Copy a link to this shard", onclick: copyLink }, icon("link")),
        h("button", { class: "btn ghost icon sm", title: "Close", "aria-label": "Close the shard", onclick: () => select(null) }, icon("x")))),
      h("div", { class: "bd stack", style: { gap: "12px" } },
        h("div", { class: "small muted" }, [`slots ${runsText(sh.slots) || "none"} (${fmt.n(sh.held)})`, `${fmt.si(sh.rows)} rows`, ver.length ? ver.join(", ") : null].filter(Boolean).join(" · ")),
        why && sev ? h("div", { class: "cc-why " + (sev === 2 ? "crit" : "warn") }, why,
          CRIT.has(sh.state) && rowsDown.length && sh.up ? h("div", { style: { marginTop: "6px" } }, `To restore it: bring ${rowsDown.length === 1 ? cname(rowsDown[0]) : "either " + rowsDown.map(cname).join(" or ")} back, or remove ${rowsDown.length === 1 ? "it" : "both"} so ${sh.copies.filter((c) => !c.down).map(cname).join(" and ")} ${sh.up === 1 ? "serves alone (1 of 1 is a majority)" : "serve as a majority"}, then add new copies.`) : null) : null,
        shardDiagram(sh),
        copyTable(sh),
        h("div", {}, h("div", { class: "cc-sec" }, "Look"),
          h("div", { class: "row", style: { gap: "6px" } },
            h("a", { class: "btn sm", href: typeof core.logsLink === "function" ? core.logsLink({ shard: sh.i }) : link("logs", { node: sh.primary?.url || sh.copies[0]?.url, q: `shard ${sh.i}` }) }, icon("logs"), "Its logs"),
            h("a", { class: "btn sm", href: link("queries", { tab: "statements", shard: sh.i }) }, icon("queries"), "Its statements"),
            sh.primary ? h("a", { class: "btn sm", href: link("profile", { node: sh.primary.url }) }, icon("flame"), `Profile ${cname(sh.primary)}`) : null,
            h("a", { class: "btn sm", href: link("querypath", { shard: sh.i }) }, icon("route"), "Query path"))),
        h("div", {}, h("div", { class: "cc-sec" }, "Change"),
          h("div", { class: "row", style: { gap: "6px" } },
            managed() ? h("span", { class: "small dim" }, "On Kubernetes the operator keeps each shard's copies: a copy that dies is started again. Change copies per shard or shards in Draft.") : [
              rowsDown.length ? h("button", { class: "btn sm", onclick: () => planFix({ kind: CRIT.has(sh.state) ? "repair" : "replace", shard: sh.i, shards: [sh.i] }) }, icon("wrench"), CRIT.has(sh.state) ? "Plan the repair…" : `Replace ${rowsDown.map(cname).join(", ")}…`) : null,
              h("button", { class: "btn sm", onclick: () => { mode = "draft"; writeParams(); drawAll(); openPop(sh.i, tileEls.get(sh.i)); } }, icon("sliders"), "Plan a change…")]))));
  }

  /** The shard as Topology drew it, for one shard: coordinators, the shard, its copies, replication. */
  function shardDiagram(sh) {
    const W = 372, n = sh.copies.length, per = Math.min(3, Math.max(1, n)), rows = Math.ceil(n / per);
    const bw = (W - 8 * (per - 1)) / per, bh = 58, top = 92;
    const H = top + rows * (bh + 26) + 4;
    const svgEl = s("svg", { viewBox: `0 0 ${W} ${H}`, class: "cc-dia", role: "img", "aria-label": `Shard ${sh.i}: ${sh.n} copies, ${sh.up} answering` });
    const coords = M.coordinators || 1;
    const rate = sh.callRate != null ? `${fmt.si(sh.callRate)} calls/s` : null;
    svgEl.append(
      s("rect", { x: W / 2 - 90, y: 2, width: 180, height: 26, rx: 7, class: "cc-dn" }),
      s("text", { x: W / 2, y: 19, "text-anchor": "middle", class: "cc-dt" }, coords > 1 ? `${coords} coordinators` : "coordinator"),
      s("path", { d: `M${W / 2},28 L${W / 2},46`, class: "cc-de" }),
      s("rect", { x: W / 2 - 80, y: 46, width: 160, height: 24, rx: 12, class: "cc-dn shard " + (CRIT.has(sh.state) ? "crit" : sh.state === "degraded" ? "warn" : "") }),
      s("text", { x: W / 2, y: 62, "text-anchor": "middle", class: "cc-dt b" }, `shard ${sh.i}${rate ? " · " + rate : ""}`));
    sh.copies.forEach((c, k) => {
      const col = k % per, row = Math.floor(k / per);
      const x = col * (bw + 8), y = top + row * (bh + 26);
      const cls = "cc-dn copy" + (c.down ? " crit" : c.lag >= LAG_WARN || c.avoided ? " warn" : "") + (c.primary ? " primary" : "") + (selCopy === c.url ? " sel" : "");
      svgEl.append(s("path", { d: `M${W / 2},70 C${W / 2},${y - 10} ${x + bw / 2},${y - 14} ${x + bw / 2},${y}`, class: "cc-de" + (c.down ? " down" : "") }));
      const g = s("g", { transform: `translate(${x},${y})`, class: "cc-dg", tabindex: 0, role: "button", "aria-label": `${cname(c)}, ${c.down ? "down" : c.role}` });
      const lines = [
        c.down ? (c.downSince ? `down ${dur((Date.now() - c.downSince) / 1000)}` : "down") : c.primary ? "primary" : c.copying ? "being copied" : c.lag ? `${kn(c.lag)} behind` : "in sync",
        [c.node ? nodeLabel(c.node) : null, c.zone ? "zone " + c.zone : null].filter(Boolean).join(" · ") || short(c.url),
        c.down ? (c.wasPrimary ? "was the primary" : c.error || "no answer") : `p99 ${fmt.ms(c.p99)} · CPU ${pct0(c.cpu)}`,
      ];
      g.append(s("rect", { width: bw, height: bh, rx: 8, class: cls }),
        s("text", { x: 8, y: 15, class: "cc-dt b" }, cname(c)),
        ...lines.map((t, j) => s("text", { x: 8, y: 29 + j * 12, class: "cc-dt s" + (j === 0 && (c.down || c.lag >= LAG_WARN) ? (c.down ? " crit" : " warn") : "") }, String(t).slice(0, Math.floor(bw / 5.6)))));
      g.addEventListener("click", () => { selCopy = c.url; writeParams(); drawSide(); });
      svgEl.append(g);
    });
    // Replication: the primary ships to each replica.
    const p = sh.copies.findIndex((c) => c.primary);
    if (p >= 0) sh.copies.forEach((c, k) => {
      if (k === p || Math.floor(k / per) !== Math.floor(p / per)) return;
      const x1 = (p % per) * (bw + 8) + bw / 2, x2 = (k % per) * (bw + 8) + bw / 2, y = top + Math.floor(p / per) * (bh + 26) + bh;
      svgEl.append(s("path", { d: `M${x1},${y} C${x1},${y + 16} ${x2},${y + 16} ${x2},${y}`, class: "cc-de repl" + (c.down ? " down" : c.lag >= LAG_WARN ? " lag" : "") }));
    });
    return h("div", { class: "cc-dia-box" }, svgEl, h("div", { class: "small muted" }, "searches reach one copy of each shard; the primary ships each write to its replicas (the arcs)"));
  }

  function copyTable(sh) {
    const rows = sh.copies.slice().sort((a, b) => (b.primary - a.primary) || (a.down - b.down) || a.k - b.k);
    // The last column follows the lens (CPU under the CPU lens), p99 otherwise.
    const col = LENSES[lens].copy && lens !== "lag" ? lens : "p99";
    const L = LENSES[col];
    const lv = (v) => { const x = level(col, v); return x === 2 ? " cc-bad" : x === 1 ? " cc-warn" : ""; };
    return h("div", { class: "tbl-wrap" }, h("table", { class: "t compact cc-copies" },
      h("thead", {}, h("tr", {}, h("th", {}, "copy"), h("th", {}, "role"), h("th", {}, "node"), h("th", { class: "num", title: "Transactions behind its primary" }, "lag"), h("th", { class: "num", title: L.title }, L.label), h("th", {}))),
      h("tbody", {}, rows.flatMap((c) => {
        const tr = h("tr", { class: selCopy === c.url ? "sel" : "" },
          h("td", {}, cspan(c), c.version && c.version !== M.mainVersion ? h("div", { class: "small muted" }, c.version) : null),
          h("td", {}, c.down ? h("span", { class: "cc-bad" }, "down") : c.primary ? h("b", {}, "primary") : c.copying ? h("span", { class: "cc-warn" }, "copying") : h("span", { class: "dim" }, "replica"), c.term != null && !c.down ? h("span", { class: "small muted", title: "Its term: the election it last saw" }, ` t${c.term}`) : null),
          h("td", { class: "small", title: [c.node ? "node " + c.node : null, c.zone ? "zone " + c.zone : null, c.url].filter(Boolean).join(" · ") }, c.node ? (M.nodePrefix && c.node.startsWith(M.nodePrefix) ? "…" + c.node.slice(M.nodePrefix.length) : nodeLabel(c.node)) : h("span", { class: "muted" }, c.host), c.zone ? h("span", { class: "muted" }, " · " + c.zone) : null),
          h("td", { class: "num" + (c.lag >= LAG_WARN ? " cc-warn" : ""), title: !c.primary && !c.down && c.lag == null ? (sh.primary ? "Its primary does not report it" : "No primary to compare it with") : "" }, c.primary || c.down ? "–" : c.lag == null ? "?" : c.lag === 0 ? "0" : kn(c.lag)),
          h("td", { class: "num" + lv(L.copy(c)) }, c.down ? "–" : L.fmt(L.copy(c))),
          h("td", { style: { width: "28px" } }, copyMenuBtn(sh, c)));
        const note = c.down ? `${c.wasPrimary ? "Was the primary. " : ""}${cap(c.error || "no answer")}${c.downSince ? " · " + sinceText(c.downSince) : ""}` : c.avoided ? `Sent no searches${c.avoidedWhy ? ": " + c.avoidedWhy : " after failures"}` : null;
        return note ? [tr, h("tr", { class: "cc-note" }, h("td", { colspan: 6, class: "small " + (c.down ? "cc-bad" : "cc-warn") }, note))] : [tr];
      }))));
  }

  // ------------------------------------------------------------ per-copy operations
  function copyActions(sh, c) {
    const ro = rollout();
    const why = (x) => x; // a reason a thing cannot be done, said as it is
    const changes = M.changes?.ok === false ? M.changes.why : null;
    const promote = c.primary ? why("it is the primary") : c.down ? why("it does not answer") : c.copying ? why("it is being copied") : c.lag ? why(`it is ${fmt.n(c.lag)} transactions behind: wait until it catches up`) : sh.n < 2 ? why("the shard has one copy") : ro ? why(`${ro}: the operator hands primaries over`) : changes ? changes : !canAdmin() ? needsAdmin : null;
    const remove = managed() ? "the operator keeps every shard's copies: change copies per shard in Draft" : sh.n < 2 ? "the shard's last copy: remove the shard instead" : changes ? changes : !canAdmin() ? needsAdmin : null;
    const verifyNo = !c.primary ? null : unsupported.verify ? "this server cannot run it from the coordinator (a newer server can)" : sh.n < 2 ? "the shard has no replica" : !canAdmin() ? needsAdmin : null;
    const ckNo = c.down ? "it does not answer" : c.backupDir === false ? "its server has no --backup-dir" : unsupported.checkpoint ? "this server cannot run it from the coordinator (a newer server can)" : !canAdmin() ? needsAdmin : null;
    const items = [];
    if (!c.primary) items.push({ label: "Make it the primary", icon: "up", no: promote, run: () => promoteCopy(sh, c) });
    if (c.primary) items.push({ label: "Verify its replicas", icon: "okc", no: verifyNo, run: () => verifyCopy(sh, c) });
    items.push({ label: "Write a checkpoint", icon: "download", no: ckNo, run: () => checkpointCopy(sh, c) });
    // No Remove on a primary (finding CL-3): hand it over first.
    if (!c.primary) items.push({ label: c.down ? "Remove it from the shard…" : "Remove it from the shard…", icon: "trash", danger: true, no: remove, run: () => removeCopy(sh, c) });
    items.push({ label: "Its logs", icon: "logs", href: typeof core.logsLink === "function" ? core.logsLink({ server: c.url }) : link("logs", { node: c.url }) });
    if (!c.down) items.push({ label: "Profile it", icon: "flame", href: link("profile", { node: c.url }) });
    return items;
  }
  function copyMenuBtn(sh, c) {
    const b = h("button", { class: "btn ghost icon sm", "aria-label": `What to do with ${cname(c)}`, title: "Operate this copy" }, icon("dots"));
    b.onclick = () => {
      const items = copyActions(sh, c);
      const m = h("div", { class: "menu cc-menu", role: "menu", "aria-label": `${cname(c)}, shard ${sh.i}` }, h("div", { class: "cc-menu-hd" }, h("b", { class: "mono", style: { color: "var(--text)" } }, cname(c)), ` · shard ${sh.i} · ${c.down ? "down" : c.role}`), items.map((it) => it.href
        ? h("a", { class: "cc-mi", href: it.href, role: "menuitem" }, icon(it.icon), h("span", {}, it.label))
        : h("button", { class: "cc-mi" + (it.danger ? " danger" : ""), role: "menuitem", disabled: !!it.no, title: it.no ? cap(it.no) : "", onclick: () => { close(); it.run(); } }, icon(it.icon), h("span", {}, it.label, it.no ? h("span", { class: "small muted cc-mi-no" }, cap(it.no)) : null))));
      const r = b.getBoundingClientRect();
      const scrim = h("div", { style: { position: "fixed", inset: 0, zIndex: 84 }, onclick: () => close() });
      document.body.append(scrim, m);
      m.style.left = Math.max(8, Math.min(r.right - 280, innerWidth - 290)) + "px";
      m.style.top = Math.min(r.bottom + 4, innerHeight - m.offsetHeight - 8) + "px";
      const onKey = (e) => { if (e.key === "Escape") close(); };
      document.addEventListener("keydown", onKey);
      function close() { m.remove(); scrim.remove(); document.removeEventListener("keydown", onKey); b.focus(); }
      setTimeout(() => m.querySelector("button:not(:disabled),a")?.focus(), 0);
    };
    return b;
  }
  const unsupported = store.get("cluster.unsupported", {});
  async function adminCall(what, path, opts) {
    try { const j = await api(path, opts); refreshSoon(); return j; }
    catch (e) {
      // A failure says what failed first (finding CL-2).
      toast(`Not done: ${what}. ${e instanceof ApiError && e.status === 403 ? "It needs an admin key." : e.message}`, "bad", 9000);
      return null;
    }
  }
  async function promoteCopy(sh, c) {
    const ok = await ask({ title: `Make ${cname(c)} shard ${sh.i}'s primary`, body: h("div", { class: "stack", style: { gap: "6px" } },
      h("div", {}, `${cname(c)} asks the shard's copies for their votes; ${sh.primary ? cname(sh.primary) + ", the primary now," : "the primary"} grants its vote, steps down and follows it.`),
      h("div", { class: "small dim" }, "Writes to the shard pause for a moment; reads go on. It is refused while another copy holds a later transaction.")), ok: "Hand it over" });
    if (!ok) return;
    const j = await adminCall(`${cname(c)} did not become shard ${sh.i}'s primary`, `/v1/admin/shards/${sh.i}/promote`, { body: { url: c.url }, reason: ok.reason });
    if (j) toast(`${cname(c)} is shard ${sh.i}'s primary${j.term != null ? ", term " + j.term : ""}`, "ok");
  }
  async function removeCopy(sh, c) {
    const after = sh.n - 1, upAfter = sh.up - (c.down ? 0 : 1);
    const restores = CRIT.has(sh.state) && c.down && upAfter > after / 2;
    const ok = await ask({
      title: `Remove ${cname(c)} from shard ${sh.i}`, danger: true, ok: "Remove it", typed: cname(c),
      body: h("div", { class: "stack", style: { gap: "6px" } },
        h("div", {}, `Shard ${sh.i} goes from ${copies_(sh.n)} to ${after}. ${restores ? `This restores it: ${upAfter} of ${after} copies is a majority, so it serves reads and takes writes again.` : ""}`),
        h("div", { class: "small dim" }, after <= 2 ? `With ${copies_(after)}, ${after === 1 ? "losing its server loses the shard until it is back" : "losing either copy stops its writes, and with failover on its reads too"}: add a copy soon.` : "It keeps a majority through one more failure."),
        h("div", { class: "small dim" }, `${c.down ? "The server does not answer now; if it comes back" : "The server stops receiving the shard's writes and"} keeps its database; it can join a shard again only as a new server.`)),
    });
    if (!ok) return;
    const j = await adminCall(`${cname(c)} is still in shard ${sh.i}`, `/v1/admin/shards/${sh.i}/replicas?url=${encodeURIComponent(c.url)}`, { method: "DELETE", reason: ok.reason });
    if (j) toast(`${cname(c)} left shard ${sh.i}`, "ok");
  }
  async function verifyCopy(sh, c) {
    toast(`Verifying shard ${sh.i}'s replicas against ${cname(c)}…`);
    try {
      const j = await api("/v1/admin/copies/verify", { body: { url: c.url } });
      const w = verifyWords(j, (u) => cname(M.all.find((x) => x.url === u) || { id: u }));
      toast(`Shard ${sh.i}: ${w.summary}${w.tone === "ok" ? "" : ": " + w.lines.filter((l) => l.tone !== "ok").map((l) => l.text).join("; ")}`, w.tone === "ok" ? "ok" : "bad", 10000);
    } catch (e) {
      if (e.status === 404 || e.status === 405) { unsupported.verify = true; store.set("cluster.unsupported", unsupported); toast("This server cannot verify a shard's replicas from the coordinator: a newer server can.", "bad", 8000); drawSide(); }
      else toast(`Not verified: ${e.message}`, "bad", 8000);
    }
  }
  async function checkpointCopy(sh, c) {
    const name = `${cname(c)}-` + new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
    const inp = h("input", { class: "input", value: name });
    const layout = h("select", { class: "select" }, h("option", { value: "file" }, "one file"), h("option", { value: "dir" }, "a directory"));
    const go = h("button", { class: "btn primary" }, "Write it");
    const m = modal({ title: `Write a checkpoint of ${cname(c)}`, body: [h("p", { class: "dim", style: { margin: 0 } }, "A consistent copy of its database, into its server's --backup-dir. Queries and writes go on."), h("label", { class: "f" }, "Name", inp), h("label", { class: "f" }, "Layout", layout)], foot: [h("button", { class: "btn", onclick: () => m.close() }, "Cancel"), go] });
    go.onclick = async () => {
      m.close();
      try {
        const j = await api("/v1/admin/copies/checkpoint", { body: { url: c.url, name: inp.value.trim(), layout: layout.value } });
        const ck = j.checkpoint || {}, bytes = ck.bytes_written ?? ck.bytes;
        toast(`Checkpoint ${inp.value.trim()} written on ${cname(c)}: ${[ck.txn != null ? `as of write ${fmt.n(ck.txn)}` : null, ck.live_docs != null ? `${fmt.n(ck.live_docs)} rows` : null, bytes != null ? fmt.bytes(bytes) : null, ck.seconds != null ? `in ${ck.seconds < 1 ? fmt.ms(ck.seconds * 1000) : dur(ck.seconds)}` : null].filter(Boolean).join(", ") || "done"}`, "ok", 8000);
      } catch (e) {
        if (e.status === 404 || e.status === 405) { unsupported.checkpoint = true; store.set("cluster.unsupported", unsupported); toast("This server cannot write a copy's checkpoint from the coordinator: a newer server can.", "bad", 8000); drawSide(); }
        else toast(`No checkpoint: ${e.message}`, "bad", 8000);
      }
    };
  }

  // ------------------------------------------------------------ the draft
  function loadDraft() {
    const d = store.get("cluster.draft", null);
    const blank = { adds: [], removes: [], newShards: [], drops: [], moves: [], even: true, kube: { shards: null, replicas: null }, seq: 1 };
    if (!d || typeof d !== "object") return blank;
    return { ...blank, ...d, kube: { ...blank.kube, ...(d.kube || {}) } };
  }
  function saveDraft() { store.set("cluster.draft", draft); }
  function draftCount() {
    if (managed()) return (draft.kube.shards != null ? 1 : 0) + (draft.kube.replicas != null ? 1 : 0);
    return draft.adds.length + draft.removes.length + (draft.newShards.length ? 1 : 0) + draft.drops.length + draft.moves.length;
  }
  /** The copies the draft removes: those asked for, and those replaced. Nothing else (finding CC-1). */
  function draftRemovals() {
    if (managed()) return [];
    const out = draft.removes.map((x) => ({ ...x, why: "asked" }));
    for (const a of draft.adds) if (a.replaces && !out.some((x) => x.url === a.replaces)) out.push({ shard: a.shard, url: a.replaces, why: "replaced" });
    return out;
  }
  function changed() {
    // Drop what no longer applies to the cluster as it is now.
    if (M && !managed()) {
      const has = (i) => M.shards.some((sh) => sh.i === i);
      const urls = new Set(M.all.map((c) => c.url));
      draft.adds = draft.adds.filter((a) => has(a.shard) && (!a.replaces || urls.has(a.replaces)) && (!a.moveOf || urls.has(a.moveOf)));
      draft.removes = draft.removes.filter((x) => has(x.shard) && urls.has(x.url));
      draft.drops = draft.drops.filter(has);
      draft.moves = draft.moves.filter((m) => has(m.from) && (has(m.to) || m.to >= M.shards.length));
    }
    if (managed() && kube?.spec) {
      if (draft.kube.shards === kube.spec.shards) draft.kube.shards = null;
      if (draft.kube.replicas === kube.spec.replicas) draft.kube.replicas = null;
    }
    saveDraft();
    serverPlan = null; kubePlan = null;
    drawHead();
    if (tab === "map") { drawToolbar(); drawMap(); drawSide(); }
    replan();
  }
  function discard() { draft = loadDraft(); Object.assign(draft, { adds: [], removes: [], newShards: [], drops: [], moves: [], kube: { shards: null, replicas: null } }); closePop(); changed(); }
  const nextId = () => "d" + (draft.seq = (draft.seq || 1) + 1);
  const typicalCopies = () => { const ns = (M?.shards || []).map((sh) => sh.n).sort((a, b) => a - b); return ns.length ? ns[Math.floor(ns.length / 2)] : 3; };
  function addShards(k) {
    if (managed()) { draft.kube.shards = Math.max(1, (draft.kube.shards ?? kube.spec?.shards ?? M.shards.length) + k); changed(); return; }
    if (k > 0) for (let j = 0; j < k; j++) draft.newShards.push({ id: nextId(), servers: Array(typicalCopies()).fill("") });
    else draft.newShards.splice(Math.max(0, draft.newShards.length + k));
    changed();
  }
  function planFix(fix) {
    if (managed()) return;
    mode = "draft";
    if (fix.kind === "repair" || fix.kind === "replace") {
      for (const i of fix.shards || [fix.shard]) {
        const sh = M.shards.find((x) => x.i === i); if (!sh) continue;
        // Each dead copy is replaced: in a shard without a majority it leaves
        // first (that alone restores reads and writes), then its new copy joins.
        for (const c of sh.copies.filter((x) => x.down)) if (!draft.adds.some((a) => a.replaces === c.url)) draft.adds.push({ id: nextId(), shard: i, url: "", replaces: c.url });
      }
      sel = fix.shard ?? fix.shards?.[0] ?? sel;
    }
    writeParams();
    changed();
    drawAll();
    if (fix.kind === "slots") { const t = tileEls.get(fix.shard); if (t) openPop(fix.shard, t, "slots"); }
  }

  /**
   * A copy to another shard or node, the tap alternative of a drag too. A
   * server holds its shard's rows and cannot change shard: a new server joins
   * the other shard (or the same shard on that node), and this one leaves.
   */
  function moveCopy(c, { shard, node }) {
    if (c.primary) { toast(`${cname(c)} is shard ${c.shard}'s primary: hand it over first (Live, its menu), then move it`, "bad", 7000); return false; }
    draft.adds = draft.adds.filter((a) => a.replaces !== c.url && a.moveOf !== c.url);
    draft.removes = draft.removes.filter((x) => x.url !== c.url);
    if (node) draft.adds.push({ id: nextId(), shard: c.shard, url: "", replaces: c.url, node });
    else {
      if (shard === c.shard) return false;
      draft.adds.push({ id: nextId(), shard, url: "", node: c.node || null, moveOf: c.url });
      draft.removes.push({ shard: c.shard, url: c.url });
    }
    return true;
  }

  // ------------------------------------------------------------ dragging
  function startDrag(e, d) {
    if (e.button !== 0) return;
    const x0 = e.clientX, y0 = e.clientY;
    let ghost = null, over = null;
    const move = (ev) => {
      if (!ghost && Math.hypot(ev.clientX - x0, ev.clientY - y0) < 5) return;
      if (!ghost) {
        dragging = d;
        d.el.dataset.dragged = "1";
        closePop();
        const label = d.kind === "copy" ? `${cname(M.all.find((c) => c.url === d.url) || { id: "a copy" })} → another shard or node` : d.kind === "slots" ? `slots of shard ${d.from} → another shard` : "+ shards";
        ghost = h("div", { class: "cc-ghost" }, label);
        document.body.append(ghost);
        document.body.classList.add("cc-dragging");
      }
      ghost.style.left = ev.clientX + 12 + "px"; ghost.style.top = ev.clientY + 12 + "px";
      if (d.kind === "shards") {
        const g = gridEl?.getBoundingClientRect(), t = gridEl?.querySelector(".cc-t");
        if (g && t) {
          const tw = t.offsetWidth + 4, th = t.offsetHeight + 4, cols = Math.max(1, Math.floor((g.width + 4) / tw));
          const idx = Math.floor((ev.clientY - g.top) / th) * cols + Math.floor((ev.clientX - g.left) / tw);
          const base = managed() ? M.shards.length : M.shards.length;
          d.k = Math.max(1, Math.min(64, idx - base + 1));
          ghost.textContent = `+ ${plural(d.k, "shard")}`;
        }
        return;
      }
      const el = document.elementFromPoint(ev.clientX, ev.clientY);
      const target = el?.closest?.(".cc-t[data-i], .cc-node[data-node]");
      if (over && over !== target) over.classList.remove("cc-over");
      over = target && target !== d.el ? target : null;
      over?.classList.add("cc-over");
      if (over?.dataset.node) ghost.textContent = `replace on ${nodeLabel(over.dataset.node)}`;
      else if (over?.dataset.i != null) ghost.textContent = d.kind === "copy" ? `a copy to shard ${over.dataset.i}` : `slots of shard ${d.from} → shard ${over.dataset.i}`;
    };
    const up = () => {
      removeEventListener("pointermove", move); removeEventListener("pointerup", up); removeEventListener("pointercancel", up);
      document.body.classList.remove("cc-dragging");
      ghost?.remove(); over?.classList.remove("cc-over");
      if (!ghost) return;
      dragging = null;
      setTimeout(() => { delete d.el.dataset.dragged; }, 0);
      if (d.kind === "shards") { if (d.k) addShards(d.k); return; }
      if (!over) return;
      if (d.kind === "copy") {
        const c = M.all.find((x) => x.url === d.url); if (!c) return;
        if (c.primary) { toast(`${cname(c)} is shard ${c.shard}'s primary: hand it over first (Live, its menu), then move it`, "bad", 7000); return; }
        if (over.dataset.node) {
          moveCopy(c, { node: over.dataset.node });
        } else {
          const to = +over.dataset.i;
          if (to === c.shard) return;
          // A server holds its shard's rows: it cannot change shard. A new
          // server joins the other shard, and this one leaves its own.
          moveCopy(c, { shard: to });
        }
        changed();
      } else if (d.kind === "slots") {
        const to = +over.dataset.i;
        if (over.dataset.node || to === d.from) return;
        const from = M.shards.find((x) => x.i === d.from);
        const even = Math.floor(SLOTS / M.shards.length);
        const count = Math.max(1, Math.min(from.held - 1, from.held > even ? Math.max(Math.round((from.held - even) / 2), Math.round(from.held / 4)) : Math.round(from.held / 4)));
        draft.moves.push({ id: nextId(), from: d.from, to, count });
        changed();
      }
    };
    addEventListener("pointermove", move); addEventListener("pointerup", up); addEventListener("pointercancel", up);
  }

  // ------------------------------------------------------------ the shard's draft popover
  function closePop() { pop?.el.remove(); pop?.scrim?.remove(); pop = null; }
  const popKey = (e) => { if (e.key === "Escape" && pop) { closePop(); } };
  document.addEventListener("keydown", popKey);
  function placePop(el, anchor) {
    const r = anchor?.getBoundingClientRect?.() || { left: innerWidth / 2, right: innerWidth / 2, top: 120, bottom: 120 };
    document.body.append(el);
    const w = el.offsetWidth, ht = el.offsetHeight;
    let x = r.right + 8; if (x + w > innerWidth - 8) x = r.left - w - 8; if (x < 8) x = 8;
    let y = r.top; if (y + ht > innerHeight - 8) y = innerHeight - ht - 8;
    el.style.left = x + "px"; el.style.top = Math.max(60, y) + "px";
  }
  function openPop(i, anchor, focus) {
    closePop();
    const sh = M.shards.find((x) => x.i === i); if (!sh) return;
    sel = i; writeParams();
    if (managed()) {
      const el = h("div", { class: "cc-pop", role: "dialog", "aria-label": `Shard ${i}` },
        h("div", { class: "row between" }, h("b", {}, `Shard ${i}`), h("button", { class: "btn ghost icon sm", "aria-label": "Close", onclick: closePop }, icon("x"))),
        h("div", { class: "small dim" }, "On Kubernetes every shard has the same copies (spec.replicas), and the operator keeps the slots even: change copies per shard or shards in the bar above the map. A copy that dies is started again by the operator."));
      const scrim = h("div", { class: "cc-pop-scrim", onclick: closePop });
      document.body.append(scrim);
      placePop(el, anchor); pop = { el, scrim, shard: i };
      return;
    }
    const goes = new Set(draftRemovals().filter((x) => x.shard === i).map((x) => x.url));
    const adds = draft.adds.filter((a) => a.shard === i);
    const dropped = draft.drops.includes(i);
    const n = M.shards.length - draft.drops.length + draft.newShards.length;
    const again = () => { changed(); openPop(i, tileEls.get(i)); };
    const rowsFor = (c) => {
      const moved = draft.adds.find((a) => a.moveOf === c.url);
      const asked = !moved && draft.removes.some((x) => x.url === c.url);
      const replaced = draft.adds.find((a) => a.replaces === c.url);
      const tog = h("button", { class: "btn xs" + (asked || replaced || moved ? " primary" : ""), disabled: c.primary && !asked, title: c.primary ? "The primary: hand it over first (Live, its menu)" : "", onclick: () => {
        if (moved) { draft.adds = draft.adds.filter((a) => a !== moved); draft.removes = draft.removes.filter((x) => x.url !== c.url); }
        else if (replaced) draft.adds = draft.adds.filter((a) => a !== replaced);
        else if (asked) draft.removes = draft.removes.filter((x) => x.url !== c.url);
        else draft.removes.push({ shard: i, url: c.url });
        again();
      } }, asked || replaced || moved ? "Keep" : "Remove");
      const rep = c.down && !asked && !replaced ? h("button", { class: "btn xs", onclick: () => { draft.adds.push({ id: nextId(), shard: i, url: "", replaces: c.url }); again(); } }, "Replace") : null;
      return h("tr", { class: goes.has(c.url) ? "cc-going" : "" }, h("td", {}, cspan(c)), h("td", { class: "small" }, c.down ? h("span", { class: "cc-bad" }, "down") : c.primary ? "primary" : c.lag ? `${kn(c.lag)} behind` : "replica"),
        h("td", { class: "small muted" }, moved ? `moves to shard ${moved.shard}` : replaced ? "replaced by a new copy" : asked ? "goes" : ""), h("td", { class: "row nw", style: { gap: "4px", justifyContent: "flex-end" } }, rep, tog));
    };
    const others = M.shards.filter((x) => x.i !== i);
    const byCpu = others.slice().sort((a, b) => (shardValue("cpu", a) ?? 0) - (shardValue("cpu", b) ?? 0));
    const shardOpt = (x) => h("option", { value: "s" + x.i }, `shard ${x.i}${shardValue("cpu", x) != null ? ` (CPU ${pct0(shardValue("cpu", x))})` : ""}`);
    const toSel = h("select", { class: "select sm", "aria-label": "To shard" }, byCpu.map((x) => h("option", { value: x.i }, `shard ${x.i}${shardValue("cpu", x) != null ? ` (CPU ${pct0(shardValue("cpu", x))})` : ""}`)));
    const even = Math.floor(SLOTS / Math.max(1, M.shards.length));
    const cnt = h("input", { class: "input sm", type: "number", min: 1, max: Math.max(1, sh.held - 1), value: Math.max(1, Math.min(sh.held - 1, Math.max(Math.round(sh.held / 4), sh.held - even))), style: { width: "72px" }, "aria-label": "Slots to move" });
    // Moving a copy without a drag: to another shard, or to a node (a new server there replaces it).
    const movable = sh.copies.filter((c) => !c.primary);
    const copySel = h("select", { class: "select sm", "aria-label": "Copy" }, movable.map((c) => h("option", { value: c.url }, cname(c) + (c.down ? " (down)" : ""))));
    const nodes = [...M.nodes.values()].filter((x) => x.known).sort((a, b) => a.key.localeCompare(b.key, undefined, { numeric: true }));
    const whereSel = h("select", { class: "select sm", "aria-label": "Move it to" },
      h("optgroup", { label: "Another shard" }, byCpu.map(shardOpt)),
      nodes.length ? h("optgroup", { label: "This shard, on another node" }, nodes.map((x) => h("option", { value: "n" + x.key }, `node ${nodeLabel(x.key)}${x.zone ? " (zone " + x.zone + ")" : ""}`))) : null);
    const rowsNote = h("div", { class: "small muted" });
    const sayRows = () => { rowsNote.textContent = `About ${fmt.si(sh.rows * (+cnt.value || 0) / Math.max(1, sh.held))} rows go with them. Or drag this shard onto another.`; };
    cnt.oninput = sayRows; sayRows();
    const el = h("div", { class: "cc-pop", role: "dialog", "aria-label": `Shard ${i} in the draft` },
      h("div", { class: "row between nw" }, h("b", {}, `Shard ${i}`), h("span", { class: "small muted grow" }, `${copies_(sh.n)} · ${fmt.n(sh.held)} slots · ${fmt.si(sh.rows)} rows`), h("button", { class: "btn ghost icon sm", "aria-label": "Close", onclick: closePop }, icon("x"))),
      h("table", { class: "cc-pop-t" }, h("tbody", {}, sh.copies.map(rowsFor), adds.map((a) => h("tr", { class: "cc-new" }, h("td", {}, h("span", { class: "cc-newname" }, "new copy")), h("td", { class: "small" }, a.replaces ? `replaces ${cname(M.all.find((c) => c.url === a.replaces) || { id: "?" })}` : a.moveOf ? `for ${cname(M.all.find((c) => c.url === a.moveOf) || { id: "?" })}` : ""), h("td", { class: "small muted" }, a.node ? "on " + nodeLabel(a.node) : ""), h("td", {}, h("button", { class: "btn xs", onclick: () => { draft.adds = draft.adds.filter((x) => x !== a); if (a.moveOf) draft.removes = draft.removes.filter((y) => y.url !== a.moveOf); again(); } }, "Drop")))))),
      h("div", { class: "row", style: { gap: "6px" } },
        h("button", { class: "btn sm", onclick: () => { draft.adds.push({ id: nextId(), shard: i, url: "" }); again(); } }, icon("plus"), "Add a copy"),
        h("button", { class: "btn sm" + (dropped ? " primary" : ""), disabled: !dropped && n <= 1, title: "Its slots move to the other shards first, then it leaves the cluster", onclick: () => { if (dropped) draft.drops = draft.drops.filter((x) => x !== i); else draft.drops.push(i); again(); } }, dropped ? "Keep the shard" : "Remove the shard")),
      movable.length ? [h("div", { class: "cc-sec", style: { marginTop: "4px" } }, "Move a copy"),
        h("div", { class: "row cc-pop-row", style: { gap: "6px" } }, copySel, h("span", { class: "small" }, "to"), whereSel,
          h("button", { class: "btn sm", onclick: () => { const c = M.all.find((x) => x.url === copySel.value); const w = whereSel.value; if (c && moveCopy(c, w[0] === "n" ? { node: w.slice(1) } : { shard: +w.slice(1) })) again(); } }, "Add"))] : null,
      h("div", { class: "cc-sec", style: { marginTop: "4px" } }, "Move slots from it"),
      h("div", { class: "row cc-pop-row", style: { gap: "6px" } }, cnt, h("span", { class: "small" }, `of ${fmt.n(sh.held)} slots to`), toSel,
        h("button", { class: "btn sm", onclick: () => { const k = Math.max(1, Math.min(sh.held - 1, +cnt.value || 1)); draft.moves.push({ id: nextId(), from: i, to: +toSel.value, count: k }); changed(); closePop(); } }, "Add")),
      rowsNote);
    const scrim = h("div", { class: "cc-pop-scrim", onclick: closePop });
    document.body.append(scrim);
    placePop(el, anchor);
    pop = { el, scrim, shard: i };
    setTimeout(() => (focus === "slots" ? cnt : el.querySelector("button"))?.focus(), 0);
  }
  function openNewShardPop(j, anchor) {
    closePop();
    const ns = draft.newShards[j]; if (!ns) return;
    const el = h("div", { class: "cc-pop", role: "dialog", "aria-label": "A new shard" },
      h("div", { class: "row between" }, h("b", {}, `New shard ${M.shards.length + j}`), h("button", { class: "btn ghost icon sm", "aria-label": "Close", onclick: closePop }, icon("x"))),
      h("div", { class: "small dim" }, "Its servers start empty with the cluster's schema and --seq-no; the first is its primary. Give their URLs in the plan."),
      h("div", { class: "row", style: { gap: "6px" } },
        h("button", { class: "btn sm", disabled: ns.servers.length <= 1, onclick: () => { ns.servers.pop(); changed(); openNewShardPop(j, anchor); } }, "− copy"),
        h("b", {}, copies_(ns.servers.length)),
        h("button", { class: "btn sm", disabled: ns.servers.length >= 9, onclick: () => { ns.servers.push(""); changed(); openNewShardPop(j, anchor); } }, "+ copy"),
        h("span", { class: "grow" }),
        h("button", { class: "btn sm", onclick: () => { draft.newShards.splice(j, 1); changed(); closePop(); } }, "Drop it")));
    const scrim = h("div", { class: "cc-pop-scrim", onclick: closePop });
    document.body.append(scrim);
    placePop(el, anchor); pop = { el, scrim };
  }

  // ------------------------------------------------------------ the plan (own machines)
  const URL_RE = /^https?:\/\/[^\s/:]+(:\d+)?\/?$/;
  function needUrls() {
    const out = [];
    for (const a of draft.adds) out.push({ key: a.id, label: a.replaces ? `new copy of shard ${a.shard}, replacing ${cname(M.all.find((c) => c.url === a.replaces) || { id: "?" })}${a.node ? ", on " + nodeLabel(a.node) : ""}` : `new copy of shard ${a.shard}${a.node ? ", on " + nodeLabel(a.node) : ""}`, get: () => a.url, set: (v) => { a.url = v; } });
    draft.newShards.forEach((ns, j) => ns.servers.forEach((u, k) => out.push({ key: `${ns.id}.${k}`, label: `new shard ${M.shards.length + j}, ${k === 0 ? "its primary" : "replica " + k}`, get: () => ns.servers[k], set: (v) => { ns.servers[k] = v; } })));
    return out;
  }

  function ownPlan() {
    const mi = moves && !moves.error ? moves : null;
    const R = measuredRate(mi);
    const rate = R.rate;
    const steps = [];
    const removes = draftRemovals();
    const shardOf = (i) => M.shards.find((x) => x.i === i);
    const est = (rows) => rows / rate;
    const touched = new Set([...draft.adds.map((a) => a.shard), ...removes.map((x) => x.shard)]);
    const repairShards = [...touched].filter((i) => { const sh = shardOf(i); return sh && (sh.state !== "ok" || removes.some((x) => x.shard === i && M.all.find((c) => c.url === x.url)?.down)); });
    const addStep = (a) => {
      const sh = shardOf(a.shard);
      steps.push({ kind: "add", repair: repairShards.includes(a.shard), shard: a.shard, rows: sh.rows, est: est(sh.rows), url: a.url, add: a,
        text: `${a.url ? short(a.url) : "a new server"} joins shard ${a.shard} and is sent its ${fmt.si(sh.rows)} rows by ${sh.primary ? cname(sh.primary) : "its primary"}${a.replaces ? `, to replace ${cname(M.all.find((c) => c.url === a.replaces) || { id: "?" })}` : ""}; the next step waits until it is in sync`,
        call: a.url ? ["POST", `/v1/admin/shards/${a.shard}/replicas`, { url: a.url }] : null, wait: "sync" });
    };
    const removeStep = (x, repair) => {
      const c = M.all.find((y) => y.url === x.url), sh = shardOf(x.shard);
      steps.push({ kind: "remove", repair, shard: x.shard, rows: 0, est: 1, url: x.url,
        text: `${c ? cname(c) : short(x.url)} leaves shard ${x.shard}${c?.down ? " (it does not answer)" : ""}${x.why === "replaced" ? ", now that its replacement is in sync" : ""}`,
        call: ["DELETE", `/v1/admin/shards/${x.shard}/replicas?url=${encodeURIComponent(x.url)}`] });
      void sh;
    };
    // 1. Repairs first: a shard without a majority loses its dead copies
    // first (that alone restores it); one that still serves gets its new
    // copies first, so it never has fewer.
    for (const i of repairShards.sort((a, b) => a - b)) {
      const sh = shardOf(i);
      const rm = removes.filter((x) => x.shard === i);
      const deadRm = rm.filter((x) => M.all.find((c) => c.url === x.url)?.down);
      if (CRIT.has(sh.state)) { deadRm.forEach((x) => removeStep(x, true)); draft.adds.filter((a) => a.shard === i).forEach(addStep); rm.filter((x) => !deadRm.includes(x)).forEach((x) => removeStep(x, true)); }
      else { draft.adds.filter((a) => a.shard === i).forEach(addStep); rm.forEach((x) => removeStep(x, true)); }
    }
    // 2. New copies elsewhere.
    for (const a of draft.adds.filter((a) => !repairShards.includes(a.shard))) addStep(a);
    // 3. New shards.
    draft.newShards.forEach((ns, j) => steps.push({ kind: "shard", shard: M.shards.length + j, rows: 0, est: 2, servers: ns.servers,
      text: `shard ${M.shards.length + j} joins, served by ${ns.servers.every(Boolean) ? ns.servers.map(short).join(", ") : copies_(ns.servers.length) + " of new servers"} (it holds no slot yet)`,
      call: ns.servers.every(Boolean) ? ["POST", "/v1/admin/shards", { servers: ns.servers }] : null }));
    // 4. Slots asked to move.
    const held = new Map(M.shards.map((sh) => [sh.i, sh.held]));
    const picked = slotPicks();
    for (const m of draft.moves) {
      const from = shardOf(m.from); if (!from) continue;
      const slots = picked.get(m.id) || [];
      const rows = from.rows * (m.count / Math.max(1, from.held));
      held.set(m.from, held.get(m.from) - m.count); held.set(m.to, (held.get(m.to) || 0) + m.count);
      steps.push({ kind: "move", shard: m.from, to: m.to, rows, est: est(rows) + 1, slots,
        text: `${fmt.n(m.count)} slots (${runsText(slots)}) move from shard ${m.from} to shard ${m.to}: about ${fmt.si(rows)} rows`,
        call: ["POST", "/v1/admin/moves", { slots, to: m.to }], wait: "moves" });
    }
    // 5. Even out over the new shards.
    const S2 = M.shards.length + draft.newShards.length;
    if (draft.newShards.length && draft.even) {
      const target = SLOTS / (S2 - draft.drops.length);
      let moved = 0, rows = 0;
      for (const sh of M.shards) { if (draft.drops.includes(sh.i)) continue; const ex = Math.max(0, held.get(sh.i) - target); moved += ex; rows += sh.rows * ex / Math.max(1, sh.held); held.set(sh.i, held.get(sh.i) - ex); }
      for (let j = 0; j < draft.newShards.length; j++) held.set(M.shards.length + j, target);
      steps.push({ kind: "even", rows, est: est(rows) + Math.ceil(moved / 256), slots: Math.round(moved),
        text: `the slots even out over ${plural(S2 - draft.drops.length, "shard")}: about ${fmt.n(Math.round(moved))} slots and ${fmt.si(rows)} rows move to the new ${draft.newShards.length === 1 ? "shard" : "shards"}, a run of up to 256 at a time`,
        call: ["POST", "/v1/admin/rebalance", {}], wait: "moves" });
    }
    // 6. Shards that leave: the highest first (the shards after one move
    // down a place when it goes); each is drained, then removed.
    // The slots a drained shard gives up are spread over the shards that stay (about evenly).
    const stay = [...held.keys()].filter((k) => !draft.drops.includes(k));
    const drained = draft.drops.reduce((a, i) => a + (held.get(i) || 0), 0);
    if (drained && stay.length) for (const k of stay) held.set(k, held.get(k) + drained / stay.length);
    for (const i of draft.drops.slice().sort((a, b) => b - a)) {
      const sh = shardOf(i);
      steps.push({ kind: "drain", shard: i, rows: sh.rows, est: est(sh.rows) + Math.ceil(sh.held / 256),
        text: `shard ${i} drains: its ${fmt.n(held.get(i))} slots and about ${fmt.si(sh.rows)} rows move to the other shards`,
        call: ["POST", `/v1/admin/shards/${i}/drain`, {}], wait: "moves" });
      steps.push({ kind: "unshard", shard: i, rows: 0, est: 2,
        text: `shard ${i} leaves the cluster; its servers (${sh.copies.map(cname).join(", ")}) keep their databases until you stop them`,
        call: ["DELETE", `/v1/admin/shards/${i}`] });
    }
    // 7. Copies asked to go that are not repairs, last.
    for (const x of removes.filter((x) => !repairShards.includes(x.shard))) removeStep(x, false);

    const rowsToCopy = steps.reduce((a, x) => a + (x.rows || 0), 0);
    /** A shard's members and the members that answer once the plan is done. */
    function afterOf(i) {
      const sh = shardOf(i);
      const gone = removes.filter((y) => y.shard === i).map((y) => M.all.find((c) => c.url === y.url));
      const added = draft.adds.filter((a) => a.shard === i).length;
      return { n: sh.n - gone.length + added, up: sh.up - gone.filter((c) => c && !c.down).length + added };
    }
    const estimate = steps.reduce((a, x) => a + (x.est || 0), 0);
    // Checks.
    const pre = [];
    const ok = (t) => pre.push({ ok: true, text: t }), no = (t) => pre.push({ ok: false, text: t }), warn = (t) => pre.push({ ok: "warn", text: t });
    if (M.changes?.ok === false) no(`the cluster map cannot change now: ${M.changes.why}`); else ok("the cluster map can change");
    if (mi?.running) no(`slots are moving already${mi.by ? " (started by " + mi.by + ")" : ""}: wait for them, or stop them`);
    else if (mi?.moving) no("a move was left in the cluster map by a coordinator that stopped: finish it first (Finish, above)");
    else ok("no slot moves under way");
    if (runner && !runner.done) no("another plan is being applied from this page");
    const need = needUrls();
    const given = need.map((x) => (x.get() || "").trim()).filter(Boolean);
    const bad = given.filter((u) => !URL_RE.test(u));
    const dup = given.filter((u, k) => given.indexOf(u) !== k);
    const taken = given.filter((u) => M.all.some((c) => c.url === u));
    if (need.length) {
      if (given.length < need.length) no(`give the gRPC URL of ${plural(need.length - given.length, "new server")} below`);
      else if (bad.length) no(`${bad[0]} is not a URL like http://10.0.0.7:7701`);
      else if (dup.length) no(`${dup[0]} is given twice`);
      else if (taken.length) no(`${taken[0]} already serves this cluster`);
      else ok(`every new server has its URL (${plural(need.length, "server")})`);
    }
    for (const x of removes) {
      const c = M.all.find((y) => y.url === x.url);
      if (c?.primary) no(`${cname(c)} is shard ${x.shard}'s primary: hand it over first (Live, its menu)`);
    }
    for (const i of touched) {
      const sh = shardOf(i), A = afterOf(i);
      if (A.n < 1) no(`shard ${i} would have no copy left: remove the shard instead`);
      else if (A.up <= A.n / 2) no(`shard ${i} would end with ${A.up} of ${A.n} copies answering, no majority: it would take no writes${sh.failover !== false ? " and serve no reads" : ""}. ${sh.copies.some((c) => c.down) ? `Replace ${sh.copies.filter((c) => c.down).map(cname).join(", ")} in the plan first.` : ""}`.trim());
    }
    for (const i of touched) {
      const sh = shardOf(i);
      const after = afterOf(i).n;
      if (after === 2) warn(`shard ${i} ends with 2 copies: with failover on, losing either stops its reads and writes`);
      if (after === 1 && sh.n > 1) warn(`shard ${i} ends with 1 copy: losing its server loses the shard until it is back`);
    }
    for (const sh of M.shards.filter((x) => CRIT.has(x.state) && !repairShards.includes(x.i))) warn(`shard ${sh.i} has no majority and this plan does not repair it`);
    const hot = problems(M).filter((p) => p.label.startsWith("Hot shard"));
    for (const p of hot) if (!draft.moves.some((m) => m.from === p.shards[0])) warn(`shard ${p.shards[0]} stays hot: its slots are not in this plan`);
    if (S2 - draft.drops.length < 1) no("a cluster keeps one shard at least");
    if (!R.measured && rowsToCopy && serverPlan?.estimate_s == null) warn(`no move has been measured here yet: times assume ${fmt.n(ASSUMED_RATE)} rows a second`);
    if (!canAdmin()) no("applying it needs an admin key");
    // Blast radius.
    const blast = [];
    const flips = steps.filter((x) => x.kind === "move" || x.kind === "even" || x.kind === "drain").reduce((a, x) => a + (x.kind === "move" ? 1 : Math.max(1, Math.ceil((x.slots || shardOf(x.shard)?.held || 256) / 256))), 0);
    if (flips) blast.push(`Writes to the slots that move pause for ${mi?.heldMs != null ? "about " + fmt.ms(mi.heldMs) : "a fraction of a second"} as each run flips (${plural(flips, "flip")}). Reads are not interrupted.`);
    const givers = [...new Set(steps.filter((x) => x.kind === "move").map((x) => x.shard))];
    if (givers.length || steps.some((x) => x.kind === "even")) blast.push(`The shards that give slots up do extra work while they copy: ${givers.length ? givers.map((i) => "shard " + i).join(", ") : "the busiest ones"}.`);
    for (const a of draft.adds) { const sh = shardOf(a.shard); blast.push(`${sh.primary ? cname(sh.primary) : "Shard " + a.shard + "'s primary"} sends ${fmt.si(sh.rows)} rows to its new copy: it does extra work while it does.`); }
    for (const i of [...new Set(removes.map((x) => x.shard))]) {
      const sh = shardOf(i), after = afterOf(i).n;
      const gone = removes.filter((x) => x.shard === i).map((x) => M.all.find((y) => y.url === x.url)).filter(Boolean);
      const dead = gone.filter((c) => c.down), live = gone.filter((c) => !c.down);
      if (dead.length && CRIT.has(sh.state)) blast.push(dead.length === sh.n - sh.up ? `Removing ${dead.map(cname).join(" and ")} first lets shard ${i} serve again (${sh.up} of ${sh.up} is a majority) before its new copies join.` : `Shard ${i} stays without a majority until its new copies are in sync.`);
      for (const c of live) blast.push(`Shard ${i} has one copy fewer once ${cname(c)} leaves${after <= 2 ? `; with ${copies_(after)}, one more failure there stops its writes` : ""}.`);
    }
    for (const i of draft.drops) blast.push(`Shard ${i}'s rows are spread over the others: each holds more.`);
    // Before and after.
    const servers0 = M.all.length;
    const servers1 = servers0 + draft.adds.length + draft.newShards.reduce((a, ns) => a + ns.servers.length, 0) - removes.length - draft.drops.reduce((a, i) => a + shardOf(i).n - removes.filter((x) => x.shard === i).length, 0);
    const per0 = M.shards.map((sh) => sh.n);
    const per1 = [...M.shards.filter((sh) => !draft.drops.includes(sh.i)).map((sh) => sh.n - removes.filter((x) => x.shard === sh.i).length + draft.adds.filter((a) => a.shard === sh.i).length), ...draft.newShards.map((ns) => ns.servers.length)];
    const rng = (xs) => { const lo = Math.min(...xs), hi = Math.max(...xs); return lo === hi ? fmt.n(lo) : `${lo} to ${hi}`; };
    const heldAfter = [...held.entries()].filter(([i]) => !draft.drops.includes(i)).map(([, v]) => v);
    const rowsNow = M.shards.reduce((a, sh) => a + sh.rows, 0);
    const before = [
      ["servers", fmt.n(servers0), fmt.n(servers1)],
      ["shards", fmt.n(M.shards.length), fmt.n(S2 - draft.drops.length)],
      ["copies a shard", rng(per0), rng(per1)],
      ["slots a shard", rng(M.shards.map((sh) => sh.held)), heldAfter.length ? rng(heldAfter.map(Math.round)) : "–"],
      ["rows a shard, average", fmt.si(rowsNow / Math.max(1, M.shards.length)), fmt.si(rowsNow / Math.max(1, S2 - draft.drops.length))],
    ];
    for (const i of [...touched].sort((a, b) => a - b).slice(0, 4)) {
      const sh = shardOf(i), A = afterOf(i);
      before.push([`copies of shard ${i}, answering`, `${sh.up} of ${sh.n}`, `${A.up} of ${A.n}`]);
    }
    return { steps, rowsToCopy, estimate, rate: R, pre, blast, before, removes, drops: draft.drops.slice(), need };
  }

  /**
   * How long a plan takes and the rows it copies: the server's estimate when
   * it has one, plus the evening out over new shards, which the server's
   * plan leaves to a rebalance it does not count.
   */
  function planTime(P) {
    const sp = serverPlan;
    if (sp?.estimate_s == null) return { estimate: P.estimate, rows: P.rowsToCopy };
    const even = P.steps.filter((x) => x.kind === "even");
    return { estimate: sp.estimate_s + even.reduce((a, x) => a + (x.est || 0), 0), rows: (sp.rows_to_copy ?? 0) + even.reduce((a, x) => a + (x.rows || 0), 0), server: true };
  }
  /**
   * The slots each move of the draft takes from its shard: the highest, as
   * the server's own planner does, and never a slot an earlier move of the
   * same shard takes already. A map of move id to runs [start, end).
   */
  function slotPicks() {
    const taken = new Map(), out = new Map();
    for (const m of draft.moves) {
      const sh = M.shards.find((x) => x.i === m.from); if (!sh) continue;
      const t = taken.get(m.from) || new Set(); taken.set(m.from, t);
      const runs = pickSlots(sh, m.count, t);
      for (const [a, b] of runs) for (let x = a; x < b; x++) t.add(x);
      out.set(m.id, runs);
    }
    return out;
  }
  /** The slots a move takes from a shard: its highest that are not in `skip`. */
  function pickSlots(sh, count, skip = new Set()) {
    const all = [];
    for (const [a, b] of sh.slots) for (let x = a; x < b; x++) if (!skip.has(x)) all.push(x);
    const take = all.slice(Math.max(0, all.length - count));
    const out = [];
    for (const x of take) { const last = out[out.length - 1]; if (last && last[1] === x) last[1] = x + 1; else out.push([x, x + 1]); }
    return out;
  }

  // The server's own plan, when it has one (POST /v1/admin/plan).
  const replan = debounce(async () => {
    if (!M || !alive) return;
    if (managed()) return kubeReplan();
    if (!draftCount() || unsupported.plan) { serverPlan = null; return; }
    // The server's shape: each shard named with every copy it keeps or
    // gains, new shards named after the last with their servers, and a move
    // a run of slots [start, end). A server not given yet is a placeholder,
    // so the estimate still counts its rows.
    let k = 0;
    const or = (u) => (u || "").trim() || `http://new-server-${++k}.invalid`;
    const picked = slotPicks();
    const want = {
      shards: [
        ...[...new Set([...draft.adds.map((a) => a.shard), ...draftRemovals().map((x) => x.shard)])].filter((i) => !draft.drops.includes(i)).map((i) => {
          const sh = M.shards.find((x) => x.i === i);
          const goes = new Set(draftRemovals().filter((x) => x.shard === i).map((x) => x.url));
          return { shard: i, copies: [...sh.copies.filter((c) => !goes.has(c.url)).map((c) => c.url), ...draft.adds.filter((a) => a.shard === i).map((a) => or(a.url))] };
        }),
        ...draft.newShards.map((ns, j) => ({ shard: M.shards.length + j, copies: ns.servers.map(or) })),
      ],
      add_shards: draft.newShards.length, remove_shards: draft.drops.slice(),
      moves: draft.moves.flatMap((m) => (picked.get(m.id) || []).map(([a, b]) => ({ slots: [a, b], to: m.to }))),
    };
    const key = JSON.stringify(want);
    if (key === serverPlanFor) return;
    serverPlanFor = key;
    try { const j = await api("/v1/admin/plan", { body: want }); if (serverPlanFor === key) serverPlan = j; }
    catch (e) { if (e.status === 404 || e.status === 405) { unsupported.plan = true; } serverPlan = null; }
    if (alive && mode === "draft" && tab === "map") drawSideKeepFocus();
  }, 300);

  // ------------------------------------------------------------ the plan (Kubernetes)
  function kubePatch() {
    const p = {};
    if (draft.kube.shards != null && draft.kube.shards !== kube.spec?.shards) p.shards = draft.kube.shards;
    if (draft.kube.replicas != null && draft.kube.replicas !== kube.spec?.replicas) p.replicas = draft.kube.replicas;
    return p;
  }
  async function kubeReplan() {
    const p = kubePatch();
    if (!Object.keys(p).length) { kubePlan = null; kubePlanFor = null; kubePlanErr = null; if (alive && mode === "draft") drawSide(); return; }
    const key = JSON.stringify(p);
    if (key === kubePlanFor && kubePlan) return;
    kubePlanFor = key;
    try { const j = await api("/v1/admin/kube/plan", { body: { spec: p } }); if (kubePlanFor === key) { kubePlan = j; kubePlanErr = null; } }
    catch (e) { if (kubePlanFor === key) { kubePlan = null; kubePlanErr = e.message; } }
    if (alive && mode === "draft" && tab === "map") drawSide();
  }
  /**
   * What a StrataCluster patch copies, and how long that takes at the rate
   * the last move ran: new copies are sent their shard's rows by every
   * primary at once; slots move to or from shards one job at a time.
   */
  function kubeTime(patch, sp) {
    const S = sp.shards ?? M.shards.length, R = sp.replicas ?? 3;
    const S1 = patch.shards ?? S, R1 = patch.replicas ?? R;
    const rows = M.shards.reduce((a, sh) => a + sh.rows, 0);
    const per = rows / Math.max(1, M.shards.length);
    const rate = measuredRate(moves && !moves.error ? moves : null);
    let copyRows = 0, secs = 0, perShard = null;
    if (R1 > R) { copyRows += (R1 - R) * S1 * per; perShard = ((R1 - R) * per) / rate.rate; secs += perShard; }
    if (S1 !== S) { const moved = rows * Math.abs(S1 - S) / Math.max(S, S1); copyRows += moved; secs += moved / rate.rate; }
    return { rows: copyRows, secs, perShard, measured: rate.measured };
  }
  function kubeNumbers() {
    const sp = kube.spec || {};
    const S0 = sp.shards ?? M.shards.length, R0 = sp.replicas ?? 3;
    const S1 = draft.kube.shards ?? S0, R1 = draft.kube.replicas ?? R0;
    const rows = M.shards.reduce((a, sh) => a + sh.rows, 0);
    const T = kubeTime({ shards: S1, replicas: R1 }, sp);
    return { S0, R0, S1, R1, rows, rowsToCopy: T.rows, secs: T.secs, perShard: T.perShard };
  }

  // ------------------------------------------------------------ the plan card
  function planCard() {
    if (managed()) return kubePlanCard();
    const n = draftCount();
    if (!n) return card({ cls: "cc-plan", title: "Plan", sub: "nothing in the draft",
      body: h("div", { class: "stack", style: { gap: "8px" } },
        h("div", { class: "small dim" }, "Change the cluster on the map: nothing applies until you apply the plan here."),
        h("ul", { class: "cc-how" },
          h("li", {}, h("b", {}, "Click a shard"), " to add or remove its copies, remove it, or move some of its slots."),
          h("li", {}, h("b", {}, "Drag a copy"), " (a pip) onto another shard to move it there, or onto a node (Node or Zone) to replace it on that node."),
          h("li", {}, h("b", {}, "Drag a shard"), " onto another to move slots to it: the fix for a hot shard."),
          h("li", {}, h("b", {}, "Drag the end handle"), " (+ shard) to add shards; their servers' URLs go in the plan.")),
        problems(M).some((p) => p.fix) ? h("div", { class: "row", style: { gap: "6px" } }, problems(M).filter((p) => p.fix).slice(0, 3).map((p) => h("button", { class: "btn sm", onclick: () => planFix(p.fix) }, p.fix.kind === "slots" ? `Cool shard ${p.shards[0]}…` : p.fix.kind === "repair" ? `Repair shard ${p.shards[0]}` : "Replace the copies down"))) : null) });
    const P = ownPlan();
    const sp = serverPlan;
    // The server's plan may not remove anything that was not asked (finding CC-1).
    // Copies asked to go: those named, and every copy of a shard asked to go.
    const asked = new Set([...P.removes.map((x) => x.url), ...M.shards.filter((sh) => P.drops.includes(sh.i)).flatMap((sh) => sh.copies.map((c) => c.url))]);
    const extra = (sp?.removes_copies || []).filter((u) => !asked.has(u));
    if (extra.length) P.pre.unshift({ ok: false, text: `the server's plan would also remove ${extra.map((u) => cname(M.all.find((c) => c.url === u) || { id: short(u) })).join(", ")}, which you did not ask: not applied` });
    // The server's checks, less those this page says already. It marks some
    // advice ok: true (a shard left with 2 copies, a copy down it leaves so):
    // those read as warnings here.
    for (const x of sp?.preflight || []) {
      if (P.pre.some((y) => y.text === x.text)) continue;
      const two = /^shard (\d+) is left with (\d+) copies/.exec(x.text || "");
      if (two && P.pre.some((y) => y.text.startsWith(`shard ${two[1]} ends with ${two[2]} cop`))) continue;
      const advice = x.ok === true && (/: with failover|leaves it so/.test(x.text || ""));
      P.pre.push({ ok: advice ? "warn" : x.ok === true ? true : x.ok === false ? false : "warn", text: x.text });
    }
    const blocked = P.pre.find((x) => x.ok === false);
    const { estimate, rows } = planTime(P);
    const changes = changeList();
    const urlBox = P.need.length ? h("div", { class: "stack", style: { gap: "6px" } },
      h("div", { class: "cc-sec" }, "New servers"),
      h("div", { class: "small dim" }, "Start each empty, with --seq-no, the cluster's schema (--schema) and --failover-after-ms like the others; then give its gRPC URL."),
      P.need.map((x) => h("label", { class: "f" }, x.label, h("input", { class: "input sm mono", placeholder: "http://10.0.0.7:7701", value: x.get() || "", oninput: debounce((e) => { x.set(e.target.value.trim()); saveDraft(); drawSideKeepFocus(); replan(); }, 350) })))) : null;
    return h("section", { class: "card cc-plan" },
      h("div", { class: "hd" }, h("h3", {}, "Plan"), h("span", { class: "pill info" }, "draft · not applied"), h("div", { class: "tools" }, h("button", { class: "btn sm ghost", onclick: discard }, "Discard"))),
      h("div", { class: "bd" },
        h("div", { class: "cc-sec" }, "Your changes"), changes,
        urlBox,
        h("div", { class: "cc-sec" }, "Before → after"),
        h("table", { class: "cc-ba" }, h("tbody", {}, P.before.map(([k, a, b]) => h("tr", {}, h("td", {}, k), h("td", { class: "num" }, a), h("td", { class: "num to" + (a !== b ? " moved" : "") }, b))))),
        h("div", { class: "cc-sec" }, `Steps, in this order · about ${dur(estimate)}`),
        h("ol", { class: "cc-steps" }, P.steps.map((x, k) => h("li", {}, h("span", { class: "k" }, k + 1), h("span", {}, x.repair ? h("span", { class: "badge", style: { marginRight: "4px" } }, "repair") : null, cap(x.text)), h("span", { class: "t" }, x.est != null ? "~" + dur(x.est) : "")))),
        h("div", { class: "small muted" }, sp?.estimate_from && sp.estimate_s != null ? `${fmt.si(rows)} rows to copy. The server's estimate: ${sp.estimate_from}.` : `${fmt.si(rows)} rows to copy, at ${fmt.n(P.rate.rate, 0)} rows a second ${P.rate.measured ? "(the last move here)" : "(assumed: no move measured yet)"}.`),
        // This page's own words when it has some (they name the shards and
        // copies); the server's summary when it has none.
        P.blast.length || sp?.blast ? h("div", { class: "cc-blast" }, h("b", {}, "Blast radius"), P.blast.length ? P.blast.map((t) => h("div", {}, t)) : h("div", {}, sp.blast)) : null,
        h("div", { class: "cc-sec" }, "Checked before it starts"),
        h("div", { class: "cc-chk" }, P.pre.map((x) => h("div", { class: x.ok === true ? "ok" : x.ok === false ? "no" : "wn" }, h("span", { class: "m" }, x.ok === true ? "✓" : x.ok === false ? "✕" : "!"), h("span", {}, cap(x.text))))),
        h("div", { class: "cc-sec" }, "Removed"),
        P.removes.length || P.drops.length
          ? h("div", { class: "small" }, [...P.removes.map((x) => `${cname(M.all.find((c) => c.url === x.url) || { id: short(x.url) })} from shard ${x.shard} (${x.why === "replaced" ? "replaced" : "you asked"})`), ...P.drops.map((i) => `shard ${i} (you asked)`)].join("; ") + ". Nothing else.")
          : h("div", { class: "small dim" }, "Nothing is removed.")),
      h("div", { class: "ft" },
        h("button", { class: "btn sm", title: "The admin calls this plan makes, as curl commands", onclick: () => copy(P.steps.map((x) => x.call ? `curl -sf -H "authorization: Bearer $STRATA_TOKEN" -X ${x.call[0]} ${location.origin}${x.call[1]}${x.call[2] ? ` -d '${JSON.stringify(x.call[2])}'` : ""}` : `# ${x.text}`).join("\n"), "Copied the calls") }, icon("copy"), "Copy as calls"),
        h("span", { class: "grow" }),
        h("button", { class: "btn primary", disabled: !!blocked, title: blocked ? cap(blocked.text) : "", onclick: () => applyOwn(P) }, `Apply ${plural(P.steps.length, "step")}…`)));
  }
  function drawSideKeepFocus() {
    const a = document.activeElement, idx = a && side.contains(a) ? [...side.querySelectorAll("input")].indexOf(a) : -1;
    const pos = a?.selectionStart;
    drawSide();
    if (idx >= 0) { const b = side.querySelectorAll("input")[idx]; if (b) { b.focus(); try { b.setSelectionRange(pos, pos); } catch { /* not a text input */ } } }
  }
  function changeList() {
    const items = [];
    const nm = (u) => cname(M.all.find((c) => c.url === u) || { id: short(u) });
    for (const a of draft.adds) {
      if (a.moveOf) { const c = M.all.find((y) => y.url === a.moveOf); items.push({ title: `Move ${nm(a.moveOf)} to shard ${a.shard}`, sub: `a new server joins shard ${a.shard} and is sent its rows, then ${nm(a.moveOf)} leaves shard ${c?.shard ?? "?"}`, drop: () => { draft.adds = draft.adds.filter((x) => x !== a); draft.removes = draft.removes.filter((y) => y.url !== a.moveOf); } }); continue; }
      items.push({ title: a.replaces ? `Replace ${nm(a.replaces)}` : `Add a copy to shard ${a.shard}`, sub: a.replaces ? `a new copy of shard ${a.shard}${a.node ? " on " + nodeLabel(a.node) : ""}, then ${nm(a.replaces)} leaves` : `a new server joins and is sent the shard's rows${a.node ? "; on " + nodeLabel(a.node) : ""}`, drop: () => { draft.adds = draft.adds.filter((x) => x !== a); } });
    }
    for (const x of draft.removes.filter((y) => !draft.adds.some((a) => a.moveOf === y.url))) items.push({ title: `Remove ${nm(x.url)}`, sub: `from shard ${x.shard}`, drop: () => { draft.removes = draft.removes.filter((y) => y !== x); } });
    if (draft.newShards.length) items.push({ title: `Add ${plural(draft.newShards.length, "shard")}`, sub: `${M.shards.length} → ${M.shards.length + draft.newShards.length}, ${draft.newShards.map((ns) => ns.servers.length).join(", ")} copies`, drop: () => { draft.newShards = []; },
      extra: h("label", { class: "row nw small", style: { gap: "6px", marginTop: "4px" } }, h("input", { type: "checkbox", checked: draft.even, onchange: (e) => { draft.even = e.target.checked; changed(); } }), "and even the slots out over them") });
    for (const i of draft.drops) items.push({ title: `Remove shard ${i}`, sub: "its slots move to the other shards first", drop: () => { draft.drops = draft.drops.filter((x) => x !== i); } });
    for (const m of draft.moves) items.push({ title: `Move ${fmt.n(m.count)} slots`, sub: `from shard ${m.from} to shard ${m.to}`, drop: () => { draft.moves = draft.moves.filter((x) => x !== m); } });
    return h("div", { class: "cc-changes" }, items.map((it, k) => h("div", { class: "cc-ch" }, h("span", { class: "k" }, k + 1), h("div", {}, h("b", {}, it.title), h("div", { class: "small dim" }, it.sub), it.extra || null),
      h("button", { class: "btn ghost icon sm", "aria-label": "Take it out of the draft", title: "Take it out of the draft", onclick: () => { it.drop(); changed(); } }, icon("x")))));
  }

  function kubePlanCard() {
    const p = kubePatch();
    const n = Object.keys(p).length;
    const ro = rollout();
    if (!n) return card({ cls: "cc-plan", title: "Plan", sub: "nothing in the draft", body: h("div", { class: "stack", style: { gap: "8px" } },
      h("div", { class: "small dim" }, `On Kubernetes the plan is one change to the StrataCluster ${kube.namespace}/${kube.name}: its shards and its copies per shard. Use the steppers above the map, or drag the end handle to add shards.`),
      h("div", { class: "small dim" }, "The operator places copies on nodes, starts a copy that dies again, and keeps the slots even: copies and slots are not dragged one by one here.")) });
    const N = kubeNumbers();
    const mi = moves && !moves.error ? moves : null;
    const R = measuredRate(mi);
    const est = N.secs;
    const removing = N.R1 < N.R0 || N.S1 < N.S0;
    const pre = [];
    if (ro) pre.push({ ok: "warn", text: `${ro}: the operator finishes it before it makes this change` });
    if (kubePlan?.refused) pre.push({ ok: false, text: kubePlan.refused });
    if (kubePlanErr) pre.push({ ok: false, text: kubePlanErr });
    for (const w of kubePlan?.warnings || []) pre.push({ ok: "warn", text: w });
    if (!kubePlan?.refused && !kubePlanErr && kubePlan) pre.push({ ok: true, text: "the coordinator checked the patch as the operator would" });
    if (!canAdmin()) pre.push({ ok: false, text: "applying it needs an admin key" });
    const blocked = pre.find((x) => x.ok === false) || (!kubePlan && !kubePlanErr ? { text: "asking the coordinator for the operator's plan" } : null);
    const before = [["servers", fmt.n(N.S0 * N.R0), fmt.n(N.S1 * N.R1)], ["shards", fmt.n(N.S0), fmt.n(N.S1)], ["copies a shard", fmt.n(N.R0), fmt.n(N.R1)], ["slots a shard", fmt.n(Math.round(SLOTS / N.S0)), fmt.n(Math.round(SLOTS / N.S1))], ["rows a shard", fmt.si(N.rows / Math.max(1, N.S0)), fmt.si(N.rows / Math.max(1, N.S1))]];
    const blast = [];
    if (N.R1 > N.R0) blast.push(`Each shard's primary sends its rows to ${N.R1 - N.R0 === 1 ? "a new copy" : (N.R1 - N.R0) + " new copies"}: ${fmt.si(N.rowsToCopy)} rows in all, extra work on every primary while it does.`);
    if (N.S1 > N.S0) blast.push(`About ${fmt.n(Math.round(SLOTS * (N.S1 - N.S0) / N.S1))} slots move to the new shards: writes to each run pause for a fraction of a second as it flips.`);
    if (N.R1 < N.R0) blast.push(`Every shard keeps ${copies_(N.R1)}${N.R1 <= 2 ? ": with failover on, losing one more copy of a shard stops its reads and writes" : ""}. The volumes of the copies that go are deleted.`);
    if (N.S1 < N.S0) blast.push(`Shards ${N.S1} to ${N.S0 - 1} drain and leave; their volumes are deleted.`);
    return h("section", { class: "card cc-plan" },
      h("div", { class: "hd" }, h("h3", {}, "Plan"), h("span", { class: "pill info" }, "draft · not applied"), h("div", { class: "tools" }, h("button", { class: "btn sm ghost", onclick: discard }, "Discard"))),
      h("div", { class: "bd" },
        h("div", { class: "cc-sec" }, "Your change"),
        h("div", { class: "cc-changes" }, Object.entries(p).map(([k, v], j) => h("div", { class: "cc-ch" }, h("span", { class: "k" }, j + 1), h("div", {}, h("b", {}, k === "shards" ? `${v > N.S0 ? "Add" : "Remove"} ${plural(Math.abs(v - N.S0), "shard")}` : `${v > N.R0 ? "One more" : "One fewer"} cop${Math.abs(v - N.R0) === 1 ? "y" : "ies"} of every shard`), h("div", { class: "small dim" }, `spec.${k}: ${k === "shards" ? N.S0 : N.R0} → ${v}`)), h("button", { class: "btn ghost icon sm", "aria-label": "Take it out of the draft", onclick: () => { draft.kube[k] = null; changed(); } }, icon("x"))))),
        h("div", { class: "cc-sec" }, "Before → after"),
        h("table", { class: "cc-ba" }, h("tbody", {}, before.map(([k, a, b]) => h("tr", {}, h("td", {}, k), h("td", { class: "num" }, a), h("td", { class: "num to" + (a !== b ? " moved" : "") }, b))))),
        h("div", { class: "cc-sec" }, `The operator's steps${est > 0 ? ` · about ${dur(est)}` : ""}`),
        kubePlan ? h("ol", { class: "cc-steps" }, (kubePlan.steps || []).map((x, k) => h("li", {}, h("span", { class: "k" }, k + 1), h("span", {}, cap(x)), h("span", { class: "t" })))) : h("div", { class: "small muted" }, kubePlanErr || "Asking the coordinator…"),
        N.rowsToCopy ? h("div", { class: "small muted" }, `${fmt.si(N.rowsToCopy)} rows to copy at ${fmt.n(R.rate, 0)} rows a second ${R.measured ? "(the rate of the last move here)" : "(assumed: no move measured yet)"}${N.perShard ? `; each shard's primary sends its ${N.R1 - N.R0 === 1 ? "new copy" : "new copies"} its ${fmt.si(N.rows / Math.max(1, M.shards.length))} rows, every shard at once: about ${dur(N.perShard)}` : ""}${N.S1 !== N.S0 ? "; slots move one run at a time" : ""}.`) : null,
        blast.length ? h("div", { class: "cc-blast" }, h("b", {}, "Blast radius"), blast.map((t) => h("div", {}, t))) : null,
        h("div", { class: "cc-sec" }, "Checked before it starts"),
        h("div", { class: "cc-chk" }, pre.map((x) => h("div", { class: x.ok === true ? "ok" : x.ok === false ? "no" : "wn" }, h("span", { class: "m" }, x.ok === true ? "✓" : x.ok === false ? "✕" : "!"), h("span", {}, cap(x.text))))),
        h("div", { class: "cc-sec" }, "Removed"),
        h("div", { class: removing ? "small" : "small dim" }, removing ? [N.R1 < N.R0 ? `the highest copy of each shard (${plural((N.R0 - N.R1) * N.S1, "server")}, a primary handed over first)` : null, N.S1 < N.S0 ? `shards ${N.S1} to ${N.S0 - 1} and their ${plural((N.S0 - N.S1) * N.R0, "server")}` : null].filter(Boolean).join("; ") + ". Their volumes are deleted." : "Nothing is removed."),
        h("div", { class: "small muted" }, "Runs as one StrataCluster change; the operator makes it a step at a time, and its rollout gates hold it if the cluster gets unwell.")),
      h("div", { class: "ft" },
        h("button", { class: "btn sm", onclick: () => copy(JSON.stringify({ spec: p }, null, 2), "Copied the patch") }, icon("copy"), "Copy as patch"),
        h("span", { class: "grow" }),
        h("button", { class: "btn primary", disabled: !!blocked, title: blocked ? cap(blocked.text) : "", onclick: () => applyKube(p, N, removing) }, "Apply…")));
  }

  // ------------------------------------------------------------ applying
  async function applyKube(p, N, removing) {
    const words = Object.entries(p).map(([k, v]) => `${k === "shards" ? "shards" : "copies of each shard"}: ${k === "shards" ? N.S0 : N.R0} → ${v}`);
    const ok = await ask({
      title: `Change ${kube.namespace}/${kube.name}`, danger: removing, ok: "Apply", typed: removing ? `${kube.name}` : null,
      body: h("div", { class: "stack", style: { gap: "8px" } }, h("ul", { style: { margin: 0 } }, words.map((x) => h("li", {}, x))),
        h("ol", { class: "cc-steps plain" }, (kubePlan?.steps || []).map((x) => h("li", {}, cap(x)))),
        removing ? h("div", { class: "small", style: { color: "var(--crit-ink)" } }, "Copies or shards go, and their volumes are deleted.") : null),
    });
    if (!ok) return;
    try {
      kube = await api("/v1/admin/kube", { body: { spec: p }, reason: ok.reason });
      toast("The operator has the change: it makes it a step at a time", "ok", 6000);
      draft.kube = { shards: null, replicas: null }; saveDraft();
      mode = "live"; writeParams(); drawAll();
    } catch (e) { toast(`Not applied: ${e.message}`, "bad", 9000); }
  }

  async function applyOwn(P) {
    const removing = P.removes.length || P.drops.length;
    const typed = !removing ? null : P.removes.length + P.drops.length === 1 ? (P.removes[0] ? cname(M.all.find((c) => c.url === P.removes[0].url) || { id: short(P.removes[0].url) }) : `shard ${P.drops[0]}`) : `remove ${[P.removes.length ? copies_(P.removes.length) : null, P.drops.length ? plural(P.drops.length, "shard") : null].filter(Boolean).join(" and ")}`;
    const ok = await ask({
      title: `Apply ${plural(P.steps.length, "step")} to the cluster`, danger: !!removing, ok: "Apply", typed,
      body: h("div", { class: "stack", style: { gap: "8px" } },
        h("ol", { class: "cc-steps plain" }, P.steps.map((x) => h("li", {}, cap(x.text)))),
        h("div", { class: "small dim" }, `About ${dur(P.estimate)}; ${fmt.si(P.rowsToCopy)} rows to copy. The steps run one at a time from this page: keep it open. Stop halts after the step under way.`),
        removing ? h("div", { class: "small", style: { color: "var(--crit-ink)" } }, `Removed: ${[...P.removes.map((x) => cname(M.all.find((c) => c.url === x.url) || { id: short(x.url) })), ...P.drops.map((i) => "shard " + i)].join(", ")}, and nothing else.`) : null),
    });
    if (!ok) return;
    startRunner(P.steps.map((x) => ({ ...x, st: "wait" })), ok.reason);
    draft = loadDraft(); Object.assign(draft, { adds: [], removes: [], newShards: [], drops: [], moves: [] }); saveDraft();
    mode = "live"; writeParams(); drawAll();
  }

  // ------------------------------------------------------------ progress
  function progressCard() {
    const mi = moves && !moves.error ? moves : null;
    const ro = rollout();
    if (runner) {
      const R = runner;
      const cur = R.steps[R.at];
      return h("section", { class: "card cc-run" + (R.failed ? " bad" : "") },
        h("div", { class: "hd" }, h("h3", {}, R.done ? (R.failed ? "The plan stopped" : R.stopped ? "The plan was stopped" : "The plan is applied") : "Applying the plan"),
          h("span", { class: "sub", title: whenTitle(R.started) }, `started ${clock(R.started)} from this page${R.by && R.by !== "token" ? " by " + R.by : ""}`),
          h("div", { class: "tools" }, R.done ? h("button", { class: "btn sm ghost", onclick: () => { runner = null; runChanged(); } }, "Close") : h("button", { class: "btn sm", disabled: R.stopping, onclick: stopRunner }, icon("stop"), R.stopping ? "Stopping…" : "Stop"))),
        h("div", { class: "bd stack", style: { gap: "8px" } },
          h("ol", { class: "cc-steps run" }, R.steps.map((x, k) => h("li", { class: x.st }, h("span", { class: "k" }, x.st === "ok" ? "✓" : x.st === "run" ? h("span", { class: "spin" }) : x.st === "bad" ? "✕" : k + 1), h("span", {}, cap(x.text), x.note ? h("div", { class: "small muted" }, x.note) : null, x.err ? h("div", { class: "small cc-bad" }, x.err) : null), h("span", { class: "t" }, x.took != null ? dur(x.took) : "")))),
          cur && !R.done && cur.wait === "moves" && mi?.job ? movesLine(mi) : null,
          R.done ? null : h("div", { class: "small muted" }, `Rows: ${fmt.si(R.steps.slice(0, R.at).reduce((a, x) => a + (x.rows || 0), 0))} of ${fmt.si(R.steps.reduce((a, x) => a + (x.rows || 0), 0))} copied${R.etaS != null ? ` · about ${dur(R.etaS)} left` : ""}`)));
    }
    if (mi?.running || mi?.moving) {
      return h("section", { class: "card cc-run" },
        h("div", { class: "hd" }, h("h3", {}, mi.running ? "Slots moving" : "A move left in the cluster map"), h("span", { class: "sub" }, [mi.job && mi.running ? jobWords(mi.job) : null, mi.by ? "started by " + mi.by : null, mi.started ? "at " + clock(mi.started) : null].filter(Boolean).join(" · ")),
          h("div", { class: "tools" },
            mi.running && mi.job ? h("button", { class: "btn sm", disabled: !canAdmin(), title: canAdmin() ? "Stop after the move under way" : needsAdmin, onclick: stopMoves }, icon("stop"), "Stop") : null,
            !mi.running || !mi.job ? h("button", { class: "btn sm", disabled: !canAdmin(), title: "Ends the move a stopped coordinator left: completed if it had flipped, given up if not", onclick: finishMove }, "Finish it") : null)),
        h("div", { class: "bd stack", style: { gap: "6px" } }, mi.job ? movesLine(mi) : h("div", { class: "small" }, `From shard ${mi.moving.from} to shard ${mi.moving.to}, ${mi.moving.state || "under way"}: no coordinator runs it now.`)));
    }
    if (ro) return h("section", { class: "card cc-run" }, h("div", { class: "hd" }, h("h3", {}, "The operator is rolling out"), h("div", { class: "tools" }, h("a", { class: "btn sm", href: link("rollout") }, "Rollouts"))), h("div", { class: "bd small" }, kube.status?.message ? cap(kube.status.message) + "." : cap(ro) + "."));
    if (managed() && kube.status?.phase && !["Ready"].includes(kube.status.phase)) return h("section", { class: "card cc-run" + (kube.status.phase === "Failed" ? " bad" : "") }, h("div", { class: "hd" }, h("h3", {}, `The operator: ${kube.status.phase}`)), h("div", { class: "bd small" }, kube.status.message || ""));
    return null;
  }
  function movesLine(mi) {
    const j = mi.job;
    const frac = mi.rowsTotal ? mi.rowsDone / mi.rowsTotal : j.of ? j.done / j.of : 0;
    return h("div", { class: "stack", style: { gap: "4px" } },
      h("div", { class: "meter" }, h("i", { style: { width: Math.round(Math.min(1, frac) * 100) + "%" } })),
      h("div", { class: "small" }, [`${fmt.n(j.done)} of ${plural(j.of, "move")}`, `${fmt.n(j.slots_moved)} slots`, mi.rowsDone != null ? `${fmt.si(mi.rowsDone)}${mi.rowsTotal ? " of " + fmt.si(mi.rowsTotal) : ""} rows` : null, mi.rate ? `${fmt.si(mi.rate)} rows/s` : null, mi.eta != null && mi.running ? `about ${dur(mi.eta)} left` : null].filter(Boolean).join(" · ")),
      j.error ? h("div", { class: "small cc-bad" }, j.error) : null);
  }
  async function stopMoves() {
    const ok = await ask({ title: "Stop the moves", body: "The move under way finishes (or is given up before it flips); the others are not started. The slots that moved stay where they are.", ok: "Stop them", reason: false });
    if (!ok) return;
    try { moves = movesInfo(await api("/v1/admin/moves/stop", { body: {}, reason: ok.reason })); toast("The moves stop after the one under way", "ok"); } catch (e) { toast(`Not stopped: ${e.message}`, "bad", 8000); }
    drawSide();
  }
  async function finishMove() {
    const ok = await ask({ title: "Finish the move left in the cluster map", body: "A coordinator stopped while it moved slots. The move is completed if it had flipped, given up if not; nothing else changes.", ok: "Finish it" });
    if (!ok) return;
    try { const j = await api("/v1/admin/moves/finish", { body: {}, reason: ok.reason }); toast(j.finished ? `Finished: ${j.finished.was}` : "No move was left", "ok"); loadMoves(); } catch (e) { toast(`Not finished: ${e.message}`, "bad", 8000); }
  }

  // ------------------------------------------------------------ capacity
  function drawTab() {
    if (tab === "capacity") return drawCapacity();
    if (tab === "changes") return drawChanges();
  }
  function patchWords(patch, sp) {
    const S = sp.shards ?? M?.shards.length ?? 0, R = sp.replicas ?? 3;
    const rows = (M?.shards || []).reduce((a, sh) => a + sh.rows, 0);
    const R2 = measuredRate(moves && !moves.error ? moves : null);
    const out = [];
    let copyRows = 0;
    if (patch.replicas != null && patch.replicas !== R) {
      const d = patch.replicas - R;
      out.push(d > 0 ? `adds ${d === 1 ? "a copy" : d + " copies"} to each of the ${fmt.n(S)} shards: ${fmt.n(d * S)} more servers (${fmt.n(S * R)} → ${fmt.n(S * patch.replicas)})` : `removes ${-d === 1 ? "a copy" : -d + " copies"} from each of the ${fmt.n(S)} shards: ${fmt.n(-d * S)} servers fewer`);
      if (d > 0) copyRows += d * rows;
    }
    if (patch.shards != null && patch.shards !== S) {
      const d = patch.shards - S;
      out.push(d > 0 ? `adds ${plural(d, "shard")} of ${copies_(patch.replicas ?? R)}: ${fmt.n(d * (patch.replicas ?? R))} more servers; about ${fmt.n(Math.round(SLOTS * d / patch.shards))} slots move to them` : `removes ${plural(-d, "shard")}: their slots move to the others`);
      copyRows += rows * Math.abs(d) / Math.max(S, patch.shards);
    }
    if (patch.storage?.size) out.push(`grows every volume to ${patch.storage.size}`);
    const T = M ? kubeTime(patch, sp) : null;
    const t = T?.rows ? `; about ${dur(T.secs)} to copy ${fmt.si(T.rows)} rows${T.perShard ? ", the shards in parallel" : ""}${T.measured ? "" : " (at an assumed rate)"}` : "";
    void copyRows; void R2;
    return out.length ? cap(out.join("; ")) + t + "." : "Changes " + Object.keys(patch).join(", ") + ".";
  }
  function drawCapacity() {
    if (!M) { clear(body, empty("layers", "Asking the shards", "")); return; }
    const sp = kube?.spec || {}, st = kube?.status || {};
    const auto = sp.autoscale, sg = st.autoscale?.suggestion;
    const by = (f) => M.all.filter((c) => !c.down && f(c) != null).sort((a, b) => f(b) - f(a));
    const med = (xs) => { const v = xs.slice().sort((a, b) => a - b); return v.length ? v[Math.floor(v.length / 2)] : null; };
    const cpus = by((c) => c.cpu), disks = by((c) => c.disk), p99s = M.shards.map((sh) => [sh, shardValue("p99", sh)]).filter(([, v]) => v != null).sort((a, b) => b[1] - a[1]);
    const fleetDisk = state.fleet?.checks?.find?.((x) => x.id === "disk");
    const slotsPer = M.shards.map((sh) => sh.held), rowsPer = M.shards.map((sh) => sh.rows);
    const row = (k, worst, median, note, lv) => h("tr", {}, h("td", {}, k), h("td", { class: "num" + (lv === 2 ? " cc-bad" : lv === 1 ? " cc-warn" : "") }, worst), h("td", { class: "num dim" }, median), h("td", { class: "small dim" }, note));
    const auto1 = managed() ? card({
      cls: "cc-cap", title: "Autoscaling", sub: auto ? `the operator's rules: ${auto.mode === "on" ? "it acts" : auto.mode === "suggest" ? "it asks a person" : "off"}` : "no rules set",
      tools: h("a", { class: "btn sm", href: "#/autoscale" }, "Policies in Autoscaling", icon("right")),
      body: sg ? h("div", { class: "stack", style: { gap: "8px" } },
        h("div", { class: "cc-sg" }, icon("bolt"), h("div", {}, h("b", {}, "The rules suggest: "), patchWords(sg.patch || {}, sp), h("div", { class: "small dim" }, `${sg.text ? sg.text + " · " : ""}${clock(sg.atMs)}; it stands while its reason does.`))),
        h("div", { class: "row", style: { gap: "6px" } },
          h("button", { class: "btn sm primary", disabled: !canAdmin(), title: canAdmin() ? "" : needsAdmin, onclick: () => applySuggestion(sg, sp) }, icon("check"), "Apply…"),
          h("button", { class: "btn sm", disabled: !canAdmin(), onclick: dismissSuggestion }, "Dismiss"),
          h("button", { class: "btn sm ghost", onclick: () => { if (sg.patch?.shards != null) draft.kube.shards = sg.patch.shards; if (sg.patch?.replicas != null) draft.kube.replicas = sg.patch.replicas; tab = "map"; mode = "draft"; writeParams(); changed(); drawAll(); } }, "Open it as a draft")))
        : h("div", { class: "small dim" }, st.autoscale?.waiting || (auto ? "No rule fires now." : "Set rules in Autoscaling: they add copies when the copies are busy, remove them when they are quiet, and grow volumes."))
    }) : card({ cls: "cc-cap", title: "Autoscaling", sub: "machines of your own",
      tools: h("a", { class: "btn sm", href: "#/autoscale" }, "Replay rules on this load", icon("right")),
      body: h("div", { class: "small dim" }, "No autoscaler runs on machines of your own: on Kubernetes the operator runs the rules. Here the numbers below say where the room is, and a draft plans the change: add copies to busy shards, shards when the disks fill, or move slots off a hot shard.") });
    const head1 = card({
      cls: "cc-cap", title: "Headroom now", sub: "the worst copy and the median, from each copy's last minute",
      body: h("table", { class: "t compact cc-head-room" }, h("thead", {}, h("tr", {}, ["", "worst", "median", ""].map((x, k) => h("th", { class: k === 1 || k === 2 ? "num" : "" }, x)))), h("tbody", {},
        row("CPU, share of cores", cpus[0] ? `${pct0(cpus[0].cpu)} ${cname(cpus[0])}` : "–", pct0(med(cpus.map((c) => c.cpu))), cpus[0] && cpus[0].cpu >= 0.7 ? "busy copies add latency: add copies to their shards" : "", level("cpu", cpus[0]?.cpu)),
        row("Disk used", disks[0] ? `${pct0(disks[0].disk)} ${cname(disks[0])}` : "–", pct0(med(disks.map((c) => c.disk))), fleetDisk?.days_to_full != null ? `full in ${fullIn(fleetDisk.days_to_full)} at the last week's growth` : "", level("disk", disks[0]?.disk)),
        row("p99 of a shard", p99s[0] ? `${fmt.ms(p99s[0][1])} shard ${p99s[0][0].i}` : "–", fmt.ms(med(p99s.map((x) => x[1]))), `SLO ${fmt.ms(slo())}`, level("p99", p99s[0]?.[1])),
        row("Slots a shard", fmt.n(Math.max(...slotsPer)), fmt.n(med(slotsPer)), Math.max(...slotsPer) - Math.min(...slotsPer) > 1 ? `uneven: ${fmt.n(Math.min(...slotsPer))} to ${fmt.n(Math.max(...slotsPer))}` : "even"),
        row("Rows a shard", fmt.si(Math.max(...rowsPer)), fmt.si(med(rowsPer)), ""),
        row("Copies a shard", fmt.n(Math.min(...M.shards.map((sh) => sh.n))), fmt.n(med(M.shards.map((sh) => sh.n))), "fewest", M.shards.some((sh) => sh.n < 3) ? 1 : 0),
        M.zonesKnown ? row("Copies a zone", "", "", [...M.zones.values()].map((z) => `${z.key || "?"}: ${fmt.n(z.copies.length)}`).join(" · ")) : null)),
    });
    const quick = card({ cls: "cc-cap", title: "Plan more room", sub: "each opens a draft: nothing applies until you apply its plan",
      body: h("div", { class: "row", style: { gap: "6px" } },
        h("button", { class: "btn sm", onclick: () => { if (managed()) draft.kube.replicas = (sp.replicas ?? 3) + 1; else for (const sh of M.shards) draft.adds.push({ id: nextId(), shard: sh.i, url: "" }); tab = "map"; mode = "draft"; writeParams(); changed(); drawAll(); } }, icon("plus"), `A copy more for every shard (${plural(M.shards.length, "server")})`),
        h("button", { class: "btn sm", onclick: () => { addShards(Math.max(1, Math.round(M.shards.length / 4))); tab = "map"; mode = "draft"; writeParams(); drawAll(); } }, icon("plus"), `${plural(Math.max(1, Math.round(M.shards.length / 4)), "shard")} more`),
        !managed() && Math.max(...slotsPer) - Math.min(...slotsPer) > 1 ? h("button", { class: "btn sm", onclick: evenOut }, "Even out the slots…") : null) });
    clear(body, h("div", { class: "cc-cap" }, auto1, head1, quick));
  }
  async function applySuggestion(sg, sp) {
    const words = patchWords(sg.patch || {}, sp);
    const removing = (sg.patch?.replicas != null && sg.patch.replicas < (sp.replicas ?? 3)) || (sg.patch?.shards != null && sg.patch.shards < (sp.shards ?? 0));
    const ok = await ask({ title: "Apply the autoscaler's suggestion", body: h("div", { class: "stack", style: { gap: "6px" } }, h("div", {}, words), h("div", { class: "small dim" }, "The operator makes it a step at a time, held by its rollout gates; it is noted as yours in the audit log.")), ok: "Apply it", danger: removing, typed: removing ? kube.name : null });
    if (!ok) return;
    try { kube = await api("/v1/admin/kube", { body: { spec: sg.patch }, reason: ok.reason }); toast("Applied: the operator makes the change", "ok"); } catch (e) { toast(`Not applied: ${e.message}`, "bad", 8000); }
    drawCapacity();
  }
  async function dismissSuggestion() {
    try { kube = await api("/v1/admin/kube/autoscale/dismiss", { body: {} }); toast("Dismissed: the rules wait their cooldown before they ask again", "ok"); } catch (e) { toast(`Not dismissed: ${e.message}`, "bad", 8000); }
    drawCapacity();
  }
  async function evenOut() {
    const ok = await ask({ title: "Even out the slots", body: "Slots move from the shards holding the most to those holding the fewest, up to 256 at a time, while the cluster serves. Writes to each run pause for a moment as it flips.", ok: "Move them" });
    if (!ok) return;
    startRunner([{ kind: "even", text: "the slots even out over the shards", call: ["POST", "/v1/admin/rebalance", {}], wait: "moves", rows: 0, st: "wait" }], ok.reason);
    tab = "map"; writeParams(); drawAll();
  }

  // ------------------------------------------------------------ changes
  const KINDS = {
    all: ["Everything", null],
    copies: ["Copies and primaries", /^(copy_|primary_|majority_)/],
    shape: ["Shards and slots", /^(shard_|moves_|copy_added|copy_removed)/],
    rollouts: ["Rollouts and autoscaling", /^(rollout|autoscale|upgrade)$/],
    people: ["People's changes", /^(change|note)$/],
  };
  async function loadChanges() {
    const now = Date.now(), from = now - 7 * 86400e3;
    const [ev, au] = await Promise.allSettled([api(`/v1/events?from=${from}&to=${now}&limit=500`), api(`/v1/admin/audit?kind=change&from=${from}&limit=500`)]);
    if (ev.status === "fulfilled") { events = ev.value.events || []; eventsErr = null; } else { events = null; eventsErr = ev.reason; }
    if (au.status === "fulfilled") { audit = au.value.entries || []; auditErr = null; } else { audit = null; auditErr = au.reason; }
    if (alive && tab === "changes") drawChanges();
  }
  // Routes that change the cluster's shape or its copies; a replay of the
  // autoscaler's rules changes nothing (though the server audits it).
  const CLUSTER_ROUTE = /^\/v1\/admin\/(shards|moves|rebalance|kube(?!\/plan)|promote|copies|autoscale(?!\/replay))/;
  /** The shard a change was of: its object ("copy s0-1", "shard 2") or its route. */
  const shardOfChange = (object, route) => { const m = /^copy s(\d+)-|^shard (\d+)/.exec(object || "") || /\/shards\/(\d+)/.exec(route || ""); return m ? +(m[1] ?? m[2]) : null; };
  function drawChanges() {
    if (events == null && audit == null && !eventsErr && !auditErr) { clear(body, card({ body: empty("clock", "Reading the changes…", "") })); loadChanges(); return; }
    const rows = [];
    for (const e of events || []) {
      if (!/^(copy_|primary_|majority_|shard_|moves_|rollout|autoscale|upgrade|change|note|contain)/.test(e.kind)) continue;
      const d = e.detail && typeof e.detail === "object" ? e.detail : {};
      if (e.kind === "change" && e.where?.shard == null && e.where?.copy == null && !CLUSTER_ROUTE.test(d.route || "")) continue;
      const who = e.who || (/^(copy_|primary_|majority_|shard_|moves_)/.test(e.kind) ? "the cluster" : e.kind === "rollout" || e.kind === "upgrade" ? "operator" : e.kind === "autoscale" ? "autoscaler" : "–");
      // A change's title starts with who made it, which has its own column.
      const what = e.who && String(e.title || "").startsWith(e.who + ": ") ? cap(e.title.slice(e.who.length + 2)) : e.title;
      rows.push({ t: e.ts_ms, kind: e.kind, sev: d.status >= 400 ? "warning" : e.severity, who, what, rid: d.request_id || null,
        detail: typeof e.detail === "string" ? e.detail : d.text || null,
        shard: e.where?.shard ?? shardOfChange(d.object, d.route), until: e.until_ms });
    }
    // Audited changes the events do not hold already (an older server has no
    // events), and each one's reason and address for those they do.
    for (const a of audit || []) {
      if (a.kind && a.kind !== "change") continue;
      if (a.method === "GET" || !CLUSTER_ROUTE.test(a.route || "")) continue;
      const twin = rows.find((r) => (a.request_id && r.rid === a.request_id) || (!r.rid && r.who && r.who === a.who && Math.abs(r.t - a.ts_ms) < 5000));
      if (twin) { if (a.reason && !twin.detail) twin.detail = "reason: " + a.reason; if (a.addr) twin.addr = a.addr; continue; }
      if (!a.kind && a.status >= 400) continue;
      rows.push({ t: a.ts_ms, kind: "change", sev: a.status >= 400 ? "warning" : "info", who: a.actor && a.actor !== "person" ? a.actor : a.who === "token" ? "root token" : a.who,
        what: cap(a.detail || `${a.method} ${a.route}`) + (a.status >= 400 ? ` (it failed: ${a.status})` : ""), detail: a.reason ? "reason: " + a.reason : null,
        shard: shardOfChange(a.object, a.route) ?? (/shard (\d+)/.exec(a.detail || "") ? +/shard (\d+)/.exec(a.detail)[1] : null), addr: a.addr });
    }
    rows.sort((a, b) => b.t - a.t);
    const K = KINDS[changeKind];
    const shown = rows.filter((r) => !K[1] || K[1].test(r.kind));
    const kindSeg = seg(Object.entries(KINDS).map(([k, [l]]) => [k, l]), changeKind, (v) => { changeKind = v; drawChanges(); }, { "aria-label": "Which changes" });
    const note = [];
    if (eventsErr) note.push(eventsErr.status === 404 ? "This server keeps no events: copies going down and primaries changing show here on a newer server. These are the audited changes." : `Events: ${eventsErr.message}`);
    if (auditErr) note.push(auditErr.status === 403 ? "The audit log needs an admin key." : `Audit: ${auditErr.message}`);
    clear(body, card({
      cls: "cc-changes", title: "Changes", sub: `this cluster's events and changes, last 7 days, newest first · times in ${zoneName()}`,
      // Read again first: on a phone the kinds scroll sideways after it.
      tools: [h("button", { class: "btn sm", title: "Read the changes again", "aria-label": "Read the changes again", onclick: () => { events = audit = null; eventsErr = auditErr = null; drawChanges(); } }, icon("refresh")), kindSeg],
      body: h("div", { class: "stack", style: { gap: "8px" } },
        note.length ? h("div", { class: "notice" }, note.join(" ")) : null,
        shown.length ? h("div", { class: "tbl-wrap" }, h("table", { class: "t compact cc-log" },
          h("thead", {}, h("tr", {}, ["when", "who", "what", "where"].map((x) => h("th", {}, x)))),
          h("tbody", {}, shown.slice(0, 300).map((r) => h("tr", {},
            h("td", { class: "nowrap", title: whenTitle(r.t) }, clock(r.t)),
            h("td", { class: "nowrap" }, r.who || "–", r.addr ? h("div", { class: "small muted" }, r.addr) : null),
            h("td", {}, h("span", { class: "row nw", style: { gap: "6px", alignItems: "flex-start" } }, h("span", { class: "dot " + (r.sev === "critical" ? "bad" : r.sev === "warning" ? "warn" : "") , style: { marginTop: "6px" } }), h("span", {}, r.what, r.detail ? h("div", { class: "small dim" }, r.detail) : null, r.until ? h("div", { class: "small muted" }, `until ${clock(r.until)} (${dur((r.until - r.t) / 1000)})`) : null))),
            h("td", {}, r.shard != null ? h("button", { class: "btn xs", onclick: () => select(+r.shard) }, `shard ${r.shard}`) : "")))))) : empty("clock", "Nothing changed", "No event or audited change of this cluster in the last 7 days.")),
    }));
  }

  // ------------------------------------------------------------ runs
  function startRunner(steps, reason) {
    runner = { steps, reason: reason || null, at: 0, started: Date.now(), by: state.whoami?.name || null, done: false, failed: false, stopping: false, stopped: false };
    runChanged();
    run(runner);
  }
  function stopRunner() {
    if (!runner) return;
    runner.stopping = true; runChanged();
    const cur = runner.steps[runner.at];
    if (cur?.wait === "moves") api("/v1/admin/moves/stop", { body: {}, reason: runner.reason }).catch(() => {});
  }

  // ------------------------------------------------------------ lifecycle
  function update() {
    if (state.paused || dragging) return;
    if (!isCoordinator()) return;
    M = state.cluster ? model(state.cluster) : null;
    if (M && draftCount()) { /* keep the draft to what still exists */ }
    const typing = document.activeElement?.matches?.("input, select, textarea") && (side.contains(document.activeElement) || sheetEl?.contains(document.activeElement));
    if (tab === "map") { drawHead(); drawStrip(); drawToolbar(); drawMap(); if (!typing) drawSide(); }
    else { drawHead(); drawStrip(); if (tab === "capacity" && !document.querySelector(".modal")) drawCapacity(); }
  }
  const mq = matchMedia("(max-width: 900px)");
  const onNarrow = () => { if (alive) { closePop(); drawAll(); } };
  mq.addEventListener?.("change", onNarrow);
  const watcher = () => {
    if (!alive) return;
    // Once a plan ends, read the moves again so the strip says the last job.
    if (runner?.done && !runner.movesRead) { runner.movesRead = true; loadMoves(); }
    drawStrip(); if (tab === "map") drawSide();
  };
  runWatchers.add(watcher);

  M = state.cluster ? model(state.cluster) : null;
  drawAll();
  loadKube().then(() => { if (M) { changed(); } });
  let movesTimer = null;
  const pollMoves = async () => { if (!alive) return; await loadMoves(); movesTimer = setTimeout(pollMoves, (moves?.running || runner && !runner.done) ? 2500 : 15000); };
  pollMoves();
  const offs = [
    bus.on("cluster", update),
    bus.on("fleet", () => { if (alive && !state.paused) update(); }),
    bus.on("health", () => { if (!isCoordinator() && state.health) { /* a server that is not a coordinator */ clear(page); renderNotCoordinator(page); } }),
    bus.on("paused", () => { if (!state.paused) update(); }),
    bus.on("narrow", onNarrow),
    every(10000, loadKube, false),
    every(30000, () => { if (tab === "changes") loadChanges(); }, false),
  ];
  refreshCluster();
  return () => {
    alive = false; active = null;
    clearTimeout(movesTimer);
    runWatchers.delete(watcher);
    closePop(); hideTip(); sheet(null); bottom(null);
    document.removeEventListener("keydown", popKey);
    mq.removeEventListener?.("change", onNarrow);
    offs.forEach((f) => f());
  };
}

function readParams(params) {
  const g = (k) => params?.get?.(k) ?? null;
  const shard = g("shard");
  return {
    mode: g("mode") === "draft" ? "draft" : "live",
    tab: ["capacity", "changes"].includes(g("tab")) ? g("tab") : "map",
    lens: LENSES[g("lens")] ? g("lens") : store.get("cluster.lens", "health") in LENSES ? store.get("cluster.lens", "health") : "health",
    group: ["node", "zone"].includes(g("group")) ? g("group") : "shard",
    shard: shard != null && shard !== "" && !Number.isNaN(+shard) ? +shard : null,
    copy: g("copy"), q: g("q") || "",
  };
}

// ---------------------------------------------------------------- applying a plan
/** Runs the steps one at a time; waits for moves and copies properly (finding CC-6). */
async function run(R) {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  for (; R.at < R.steps.length; R.at++) {
    const x = R.steps[R.at];
    if (R.stopping) { R.stopped = true; break; }
    x.st = "run"; const t0 = Date.now(); runChanged();
    try {
      const [method, path, body] = x.call || [];
      if (!method) throw new Error("this step has no call: a server's URL is missing");
      let jobStart = null;
      if (x.wait === "moves") {
        // A job of moves started elsewhere first: wait for it.
        for (;;) { const m = movesInfo(await api("/v1/admin/moves")); if (!m.running && !m.moving) break; x.note = "waiting for the moves under way to end"; runChanged(); await sleep(2000); if (R.stopping) throw new Error("stopped"); }
        jobStart = Date.now();
      }
      await api(path, method === "DELETE" ? { method, reason: R.reason } : { body, reason: R.reason });
      if (x.wait === "moves") {
        // Done when this job is no longer running and nothing moves in the map.
        for (let k = 0; ; k++) {
          await sleep(k < 3 ? 700 : 1500);
          const m = movesInfo(await api("/v1/admin/moves"));
          const job = m.job;
          const ours = job && (!m.started || m.started >= jobStart - 5000);
          if (ours) {
            x.note = `${fmt.n(job.done)} of ${plural(job.of, "move")} · ${fmt.n(job.slots_moved)} slots${m.rowsDone != null ? ` · ${fmt.si(m.rowsDone)} rows` : ""}${m.rate ? ` · ${fmt.si(m.rate)} rows/s` : ""}${m.eta != null ? ` · about ${dur(m.eta)} left` : ""}`;
            R.etaS = m.eta;
            runChanged();
            if (job.state === "failed") throw new Error(job.error || "a move failed");
            if (job.state === "stopped") { R.stopped = true; throw new Error("stopped: the slots that moved stay where they are"); }
            if (!m.running && !m.moving) break;
          } else if (!m.running && !m.moving && k > 3) break;
        }
      } else if (x.wait === "sync") {
        // The new copy serves once it has caught up with its primary.
        for (let k = 0; ; k++) {
          await sleep(2000);
          await refreshCluster();
          const M = model(state.cluster);
          const c = M.all.find((y) => y.url === x.url);
          if (c && c.inSync && !c.copying) break;
          const sh = M.shards.find((s2) => s2.i === x.shard);
          x.note = c ? (c.copying ? `being sent the shard's rows (${fmt.si(sh?.rows)})` : c.lag != null ? `${fmt.n(c.lag)} transactions behind` : "joining") : "joining";
          R.etaS = x.rows ? Math.max(0, x.rows / measuredRate(null).rate - (Date.now() - t0) / 1000) : null;
          runChanged();
          if (R.stopping) { x.note = "still being copied: it goes on, but the plan stops here"; throw new Error("stopped"); }
        }
      }
      x.st = "ok"; x.took = (Date.now() - t0) / 1000; x.note = null;
      refreshSoon();
    } catch (e) {
      x.st = "bad"; x.err = e.message === "stopped" ? "Stopped here." : (e instanceof ApiError ? `${e.message}` : e.message); x.took = (Date.now() - t0) / 1000;
      if (e.message !== "stopped" && !R.stopped) R.failed = true; else R.stopped = true;
      R.done = true; runChanged();
      toast(R.failed ? `The plan stopped at step ${R.at + 1}: ${x.err}` : "The plan stopped", R.failed ? "bad" : "info", 9000);
      refreshCluster();
      return;
    }
  }
  R.done = true; R.etaS = null;
  runChanged();
  if (!R.stopped) toast("The plan is applied", "ok", 6000);
  refreshSoon();
}

// ---------------------------------------------------------------- not a coordinator
/** A server that is not a coordinator: what it is, and how to make a cluster. */
function renderNotCoordinator(page) {
  const hl = state.health || {};
  const box = h("div");
  page.append(h("div", { class: "page-head" }, h("div", {}, h("h1", {}, "Cluster"), h("p", {}, hl.slots ? `This server holds shard ${hl.slots.shard} of ${hl.slots.of}: the cluster is seen and changed from a coordinator.` : "This server holds one database, not a cluster."))), box);
  if (hl.slots) {
    const rep = hl.replication || {};
    clear(box, card({ title: "This server's part", body: h("dl", { class: "kv" }, [["shard", `${hl.slots.shard} of ${hl.slots.of}`], ["cluster map", "epoch " + (hl.slots.epoch ?? "–")], ["role", rep.role || "standalone"], ["term", rep.term ?? "–"], ["last transaction", fmt.n(rep.seq_no ?? hl.txn)]].flatMap(([k, v]) => [h("dt", {}, k), h("dd", {}, String(v))])) }));
    return () => {};
  }
  const name = h("input", { class: "input", value: "strata", style: { width: "140px" } });
  const shards = h("input", { class: "input", type: "number", value: 2, min: 1, style: { width: "80px" } });
  const copiesN = h("input", { class: "input", type: "number", value: 3, min: 1, max: 9, style: { width: "80px" } });
  const pre = h("pre", { class: "code", style: { maxHeight: "360px" } });
  const out = () => {
    const nm = name.value.trim() || "strata";
    pre.textContent = ["# 1. The operator, once per Kubernetes cluster:", "#    kubectl apply -f strata-server/deploy/k8s/operator/operator.yaml", "# 2. This file: kubectl apply -f cluster.yaml",
      "apiVersion: strata.chloetech.ai/v1alpha1", "kind: StrataCluster", "metadata:", `  name: ${nm}`, "spec:", `  shards: ${+shards.value || 1}`, `  replicas: ${+copiesN.value || 3}`, "  coordinators: 2", "  storage:", "    size: 10Gi", "  demo: 10000", "  failoverAfterMs: 3000", "  writeAcks: all"].join("\n");
  };
  for (const e of [name, shards, copiesN]) e.oninput = out;
  out();
  clear(box, card({ title: "Make a cluster", sub: "a cluster spreads tables over shards of a few copies each, behind coordinators",
    body: h("div", { class: "stack" },
      h("div", { class: "row", style: { gap: "14px", alignItems: "flex-end" } }, h("label", { class: "f" }, "Name", name), h("label", { class: "f" }, "Shards", shards), h("label", { class: "f" }, "Copies per shard", copiesN)),
      h("div", { class: "row between" }, h("b", {}, "On Kubernetes"), h("button", { class: "btn sm", onclick: () => copy(pre.textContent, "Copied the StrataCluster") }, icon("copy"), "Copy")), pre,
      h("div", { class: "small dim" }, "On machines of your own: start each copy with --seq-no and the same --schema, one coordinator with --shards over each shard's first copy, then add the other copies from this page on the coordinator.")) }));
  return () => {};
}
