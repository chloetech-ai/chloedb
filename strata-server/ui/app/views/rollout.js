// Rollouts: where a change of the cluster is, whether it is safe, and how
// to stop it, in a header read in five seconds (how many copies run the
// new version, the pace and the time left, what holds it and why, the
// actions the state allows, each with its cost); the version picker (the
// images the registry publishes, a plan, a confirmation with the restarts
// and the time it takes, then the upgrade); every version of the spec; and
// the gates that hold a change. A cluster on machines of your own has no
// operator: it shows what each server runs and how to upgrade it by hand.

import * as core from "../core.js";
import { h, s, icon, clear, fmt, card, api, toast, bus, every, empty, clamp, seg, modal, tipOn, hideTip, copy } from "../core.js";
import { state, isProduction } from "../data.js";

export default {
  id: "rollout", title: "Rollouts", icon: "rollout", group: "Change",
  keywords: "rollout roll out upgrade upgrader new version versions picker image tag commit digest registry ghcr automatic history rollback roll back undo pause resume hold held gate gates health soak verify preflight progress eta pace restart operator kubernetes",
  render, onParams,
};

// The tab switcher of the view on screen, for a link that changes ?tab=.
let onTab = null;
function onParams(r) { onTab?.(r.params.get("tab") || "now", r.params); }

// ---------------------------------------------------------------- words
const plural = (n, w, ws) => fmt.plural(n, w, ws);
const can = (scope) => (typeof core.can === "function" ? core.can(scope) : true);
const NEEDS = "needs an admin key";
/** The viewer's time zone, as a clock says it (UTC, CET, PST). */
const TZ = (() => { try { return new Intl.DateTimeFormat([], { timeZoneName: "short" }).formatToParts(new Date()).find((p) => p.type === "timeZoneName")?.value || ""; } catch { return ""; } })();
/** 19:42 today, Nov 2 19:42 another day. */
function clock(ms, sec = false) {
  if (!ms) return "–";
  const d = new Date(ms);
  const t = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: sec ? "2-digit" : undefined, hourCycle: "h23" });
  return d.toDateString() === new Date().toDateString() ? t : `${d.toLocaleDateString([], { month: "short", day: "numeric" })} ${t}`;
}
/** A time as an element: the clock, the full date and zone on hover. */
const at = (ms, sec = false) => h("span", { class: "nowrap", title: ms ? new Date(ms).toString() : "" }, clock(ms, sec));
/** A length of time in words: 14 s, 6 min, 1 h 48 min. */
function span(secs) {
  if (secs == null || !Number.isFinite(secs)) return "–";
  secs = Math.max(0, Math.round(secs));
  if (secs < 60) return `${secs} s`;
  const m = Math.round(secs / 60);
  if (m < 60) return `${m} min`;
  const hr = Math.floor(m / 60), r = m % 60;
  if (hr < 48) return r ? `${hr} h ${r} min` : `${hr} h`;
  return `${Math.round(hr / 24)} days`;
}
const ago = (ms) => (ms ? span((Date.now() - ms) / 1000) + " ago" : "never");
/** Who made a change: a person, or one of the operator's automatic parts. */
const who = (by) => (["operator", "upgrader", "autoscaler"].includes(by) ? `the ${by}` : by);

// ---------------------------------------------------------------- versions
const HEX40 = /^[0-9a-f]{40}$/;
const digestOf = (img) => (img || "").match(/@(sha256:[0-9a-f]{64})/)?.[1] || null;
const tagOf = (img) => (img || "").replace(/@.*$/, "").match(/:([^/:]+)$/)?.[1] || null;
const repoOf = (img) => (img || "").replace(/@.*$/, "").replace(/:[^/:]+$/, "");
/** The source of an image, for its commits (the label the image workflow sets). */
const sourceOf = (repo) => (/ghcr\.io\/chloetech-ai\/strata-server$/.test(repo || "") ? "https://github.com/chloetech-ai/strata" : null);
const sec = (iso) => (iso ? Date.parse(iso) || null : null);

/**
 * What is known of the images: the registry's list, the upgrader's
 * status and the version map, by digest (and by image for a tag).
 */
function catalog(reg, kube) {
  const by = new Map();
  const add = (x) => {
    if (!x) return;
    const key = x.digest || x.image;
    if (!key) return;
    const o = by.get(key) || { tags: [] };
    for (const k of ["digest", "revision", "created"]) if (x[k] && !o[k]) o[k] = x[k];
    if (x.image && (!o.image || x.image.includes("@"))) o.image = x.image;
    for (const t of [...(x.tags || []), x.tag || tagOf(x.image)]) if (t && !o.tags.includes(t)) o.tags.push(t);
    for (const k of ["current", "newer", "tracked"]) if (x[k] === true) o[k] = true;
    by.set(key, o);
    if (x.image && x.image !== key) by.set(x.image, o);
  };
  for (const x of reg?.images || []) add(x);
  add(reg?.current);
  const u = kube?.status?.upgrade;
  add(u?.latest); add(u?.current);
  for (const v of kube?.status?.versions || []) add(v);
  return by;
}
/** What is known of `image` (a reference, or a digest). */
function known(cat, image) {
  if (!image) return null;
  const d = image.startsWith("sha256:") ? image : digestOf(image);
  const o = (d && cat.get(d)) || cat.get(image);
  const t = image.startsWith("sha256:") ? null : tagOf(image);
  return { image: image.startsWith("sha256:") ? o?.image || null : image, digest: d, tags: [], ...(o || {}), ...(t && !(o?.tags || []).includes(t) ? { tags: [...(o?.tags || []), t] } : {}) };
}
/** A tag people chose (v0.9.4), not a commit or a moving one. */
const named = (v) => (v?.tags || []).find((t) => !HEX40.test(t) && !/^[0-9a-f]{40}-/.test(t) && !/^[0-9a-f]{64}$/.test(t) && t !== "latest" && t !== v.track);
const commitOf = (v) => v?.revision || (v?.tags || []).find((t) => HEX40.test(t)) || null;
/** A version's short name: its tag, else its commit, else its digest. */
function vname(v) {
  if (!v) return "unknown";
  const t = named(v), c = commitOf(v);
  if (t) return t;
  if (c) return c.slice(0, 7);
  if (v.digest) return v.digest.replace("sha256:", "").slice(0, 12);
  return (v.tags || [])[0] || (v.image || "unknown").replace(/^.*\//, "");
}
/** A version in a sentence: 5f2e523 (built Nov 2 20:16), or v0.9.4 (commit 5f2e523, built …). */
function vsaid(v, { built = true } = {}) {
  const t = named(v), c = commitOf(v), b = sec(v?.created);
  const bits = [];
  if (t && c) bits.push("commit " + c.slice(0, 7));
  if (built && b) bits.push("built " + clock(b));
  return h("span", {}, h("b", { class: "ro-vn", title: v?.image || "" }, vname(v)), bits.length ? h("span", { class: "dim" }, ` (${bits.join(", ")})`) : null);
}
const commitLink = (v) => { const src = sourceOf(repoOf(v?.image)), c = commitOf(v); return src && c ? `${src}/commit/${c}` : null; };
const compareLink = (a, b) => { const src = sourceOf(repoOf(b?.image) || repoOf(a?.image)), x = commitOf(a), y = commitOf(b); return src && x && y ? `${src}/compare/${x}...${y}` : null; };

// ---------------------------------------------------------------- gates
/** Two digits that mean something: 64%, 2.5%, 0.01% (as the operator says it). */
function pct(v) {
  if (v == null || Number.isNaN(v)) return "–";
  const p = v * 100;
  const d = p >= 10 || p <= 0 ? 0 : p >= 1 ? 1 : Math.min(6, 1 - Math.floor(Math.log10(p)));
  return p.toFixed(d).replace(/(\.\d*?)0+$/, "$1").replace(/\.$/, "") + "%";
}
const ms = (v) => (v == null ? "–" : v >= 1000 ? fmt.ms(v) : `${Number.isInteger(v) || v >= 100 ? Math.round(v) : v.toFixed(1)} ms`);
const GATES = [
  { key: "p99Ms", label: "Search p99", short: "p99", what: "the coordinator's, as its clients see it", show: ms, load: "p99_ms", suggest: (v) => Math.max(50, Math.ceil(((v || 20) * 3) / 10) * 10), max: 1e6 },
  { key: "errorRatio", label: "Failed searches", short: "errors", what: "the share of searches that fail", show: pct, load: "error_ratio", suggest: () => 0.01, max: 1 },
  { key: "replicaBehind", label: "Replica lag", short: "replica lag", what: "transactions a replica is behind its primary", show: (v) => (v == null ? "–" : plural(Math.round(v), "transaction")), load: "replica_behind_max", suggest: () => 1000, max: 1e9 },
  { key: "diskUsedRatio", label: "Fullest volume", short: "fullest volume", what: "the used share of the fullest volume", show: pct, load: "disk_used_ratio", suggest: (v) => Math.min(0.99, Math.max(0.9, Math.ceil(((v || 0) + 0.02) * 100) / 100)), max: 1 },
];
const gateOf = (k) => GATES.find((g) => g.key === k);

/** The cluster's live numbers, as the operator reads them from /v1/cluster. */
export function numbers(c) {
  const servers = (c?.shards || []).flatMap((x) => x.replicas || []).filter((r) => r.status !== "down");
  const worst = (k) => servers.reduce((m, r) => { const v = r.detail?.load?.[k]; return typeof v === "number" ? (m == null ? v : Math.max(m, v)) : m; }, null);
  const co = c?.coordinator?.load || {};
  return {
    p99Ms: typeof co.p99_ms === "number" ? co.p99_ms : worst("p99_ms"),
    errorRatio: typeof co.error_ratio === "number" ? co.error_ratio : worst("error_ratio"),
    replicaBehind: worst("replica_behind_max"),
    diskUsedRatio: worst("disk_used_ratio"),
  };
}
function stepOf(g, v) {
  if (g.max === 1) return v < 0.02 ? 0.001 : v < 0.2 ? 0.005 : 0.01;
  const e = Math.pow(10, Math.floor(Math.log10(Math.max(1, v / 10))));
  return [1, 2, 5, 10].map((m) => m * e).find((x) => x >= v / 20) || e;
}
const snap = (g, v) => { const st = stepOf(g, v); return clamp(Number((Math.round(v / st) * st).toPrecision(6)), 0, g.max); };

// ---------------------------------------------------------------- the spec
const same = (a, b) => JSON.stringify(a ?? null) === JSON.stringify(b ?? null);
/** What changed between two versions' specs, in words. */
function summary(a, b, name = (i) => vname({ image: i, tags: [tagOf(i)].filter(Boolean), digest: digestOf(i) })) {
  if (!a) return "The first version the operator saw";
  const out = [];
  const was = (k) => (a[k] == null ? "none" : JSON.stringify(a[k]));
  if (a.image !== b.image) out.push(`image ${name(a.image)} to ${name(b.image)}`);
  if (a.shards !== b.shards) out.push(`${b.shards} shards (was ${a.shards})`);
  if (a.replicas !== b.replicas) out.push(`${b.replicas} copies each (was ${a.replicas})`);
  if (a.coordinators !== b.coordinators) out.push(`${b.coordinators} coordinators (was ${a.coordinators})`);
  if (!same(a.storage, b.storage)) out.push(`volumes ${b.storage?.size}`);
  if (!same(a.args, b.args)) out.push(`server flags ${(b.args || []).join(" ") || "none"}`);
  if (!same(a.coordinatorArgs, b.coordinatorArgs)) out.push(`coordinator flags ${(b.coordinatorArgs || []).join(" ") || "none"}`);
  for (const k of ["failoverAfterMs", "writeAcks", "tokenSecret", "clusterTokenSecret", "resources", "coordinatorResources", "imagePullPolicy", "schema", "demo"]) {
    if (!same(a[k], b[k])) out.push(`${k} ${b[k] == null ? "unset" : JSON.stringify(b[k])} (was ${was(k)})`);
  }
  return out.length ? out.join(", ") : "No change to the servers";
}
const serverConfig = (x) => JSON.stringify([x?.image, x?.imagePullPolicy, x?.args, x?.resources, x?.failoverAfterMs, x?.writeAcks, x?.tokenSecret, x?.clusterTokenSecret]);
const coordinatorConfig = (x) => JSON.stringify([x?.image, x?.imagePullPolicy, x?.coordinatorResources, x?.coordinatorArgs, x?.tokenSecret, x?.clusterTokenSecret]);

/** A spec as YAML lines, for a diff. */
function yaml(spec) {
  const out = [];
  const scalar = (v) => (typeof v === "string" ? (/^[\w./:@-]+$/.test(v) && !/^(true|false|null|yes|no|on|off|~)$/i.test(v) && !/^[-+]?(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?$/.test(v) && !v.startsWith("-") ? v : JSON.stringify(v)) : JSON.stringify(v));
  const walk = (o, ind) => {
    for (const [k, v] of Object.entries(o || {})) {
      if (v === null || v === undefined || (Array.isArray(v) && !v.length)) continue;
      if (k === "schema" && ind === "  ") { out.push(`${ind}schema: {…}`); continue; }
      if (Array.isArray(v)) out.push(`${ind}${k}: [${v.map(scalar).join(", ")}]`);
      else if (typeof v === "object") { if (!Object.values(v).some((x) => x != null)) continue; out.push(`${ind}${k}:`); walk(v, ind + "  "); }
      else out.push(`${ind}${k}: ${scalar(v)}`);
    }
  };
  out.push("spec:");
  walk(spec, "  ");
  return out;
}
function lineDiff(a, b) {
  const n = a.length, m = b.length, L = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) L[i][j] = a[i] === b[j] ? L[i + 1][j + 1] + 1 : Math.max(L[i + 1][j], L[i][j + 1]);
  const out = []; let i = 0, j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && a[i] === b[j]) { out.push([" ", a[i]]); i++; j++; }
    else if (i < n && (j === m || L[i + 1][j] >= L[i][j + 1])) { out.push(["-", a[i]]); i++; }
    else { out.push(["+", b[j]]); j++; }
  }
  return out;
}
const diffBlock = (a, b) => h("pre", { class: "ro-diff" }, (a ? lineDiff(yaml(a), yaml(b)) : yaml(b).map((l) => [" ", l])).map(([k, l]) => h("div", { class: k === "+" ? "add" : k === "-" ? "del" : "" }, `${k} ${l}`)));

// ---------------------------------------------------------------- asking
/**
 * Asks before a change: resolves false, or {reason}. Cancel has the focus;
 * `reason` is "required", "optional" or absent; the reason goes to the
 * audit log with the change.
 */
function ask({ title, body, ok = "Confirm", danger = false, reason = null, wide = true }) {
  // On production every change says why, as Cluster's Apply asks (NEW-cluster-admin-13).
  if (reason === "optional" && isProduction()) reason = "required";
  return new Promise((resolve) => {
    let answered = false;
    const yes = h("button", { class: "btn " + (danger ? "danger solid" : "primary") }, ok);
    const no = h("button", { class: "btn" }, "Cancel");
    const why = reason ? h("textarea", { class: "input ro-why", rows: 2, placeholder: reason === "required" ? "Why, for the audit log and the people on call (required)" : "Why, for the audit log (optional)" }) : null;
    const sync = () => { yes.disabled = reason === "required" && !why.value.trim(); };
    if (why) { why.oninput = sync; sync(); }
    const m = modal({
      title, wide,
      body: h("div", { class: "stack ro-dlg" }, body, why ? h("label", { class: "stack", style: { gap: "4px" } }, h("span", { class: "small dim" }, reason === "required" ? "Reason" : "Reason (optional)"), why) : null),
      foot: [no, yes], onClose: () => { if (!answered) resolve(false); },
    });
    yes.onclick = () => { answered = true; m.close(); resolve({ reason: why?.value.trim() || "" }); };
    no.onclick = () => { answered = true; m.close(); resolve(false); };
    setTimeout(() => no.focus(), 0);
  });
}
/** A table of facts in a dialog: [[label, value]]. */
const facts = (rows) => h("dl", { class: "ro-facts" }, rows.filter(Boolean).flatMap(([k, v]) => [h("dt", {}, k), h("dd", {}, v)]));

// ---------------------------------------------------------------- the view
const TABS = [["now", "Now"], ["upgrades", "Versions"], ["history", "History"], ["gates", "Gates"]];

function render(page, r) {
  const pick = (t) => (TABS.some(([k]) => k === t) ? t : "now");
  let tab = pick(r.params.get("tab"));
  let kube = null, kubeErr = null;  // GET /v1/kube
  let reg = null;                   // GET /v1/admin/kube/versions: null not yet, {missing} an older server, {error}
  let busy = false;
  let filter = { q: "", newer: false };
  let openVersion = null;           // the spec version whose diff is open (History)
  let draft = null;                 // the gates being edited
  const paceSeen = [];              // [t, done] seen on this page, when the server says no pace
  const series = {};                // gate -> [[t, v]] seen on this page

  const head = h("div"), status = h("div"), body = h("div");
  page.append(head, status, body);

  const managed = () => !!kube?.managed;
  const st = () => kube?.status || {};
  const ro = () => st().rollout || {};
  const history = () => st().history || [];
  const spec = () => kube?.spec || {};
  const cat = () => catalog(reg, kube);
  const vOf = (image) => known(cat(), image);

  // ---------------------------------------------------------- the copies
  /** Every copy of the live cluster: short name, url, shard, role, node, version key, load. */
  function copies() {
    const out = [];
    for (const [i, x] of (state.cluster?.shards || []).entries()) {
      for (const [j, rr] of (x.replicas || []).entries()) {
        out.push({
          url: rr.url, short: rr.short || (typeof core.copyName === "function" ? core.copyName(rr.url, state.cluster) : `s${x.shard ?? i}-${j}`),
          shard: x.shard ?? i, role: rr.takes_writes ? "primary" : rr.role || "replica", node: rr.node || rr.name || null,
          down: rr.status === "down" || rr.role === "unknown", since: rr.down_since_ms || null, was: rr.last_role || null,
          ver: rr.version?.digest || rr.version?.image || rr.version?.version || rr.detail?.system?.version || null,
          load: rr.detail?.load || {}, up_s: rr.detail?.system?.uptime_s,
        });
      }
    }
    return out;
  }
  /** A copy's short name from the operator's url for it. */
  function nameOf(url) {
    const c = copies().find((x) => x.url === url);
    if (c) return c.short;
    const m = /(?:^|\/\/)[^/]*?-s(\d+)-(\d+)\b/.exec(url || "");
    return m ? `s${m[1]}-${m[2]}` : (url || "").replace(/^https?:\/\//, "");
  }
  /** The operator's words, with copies named the short way. */
  function said(text) {
    let t = String(text || "");
    for (const u of new Set(t.match(/https?:\/\/[^\s,;)]+/g) || [])) t = t.split(u).join(nameOf(u));
    // Images by their short name (a tag, else a commit), not their digest.
    for (const im of new Set(t.match(/(?:[a-z0-9.-]+(?::\d+)?\/)+[a-z0-9._-]+(?::[\w.-]+|@sha256:[0-9a-f]{64})/gi) || [])) t = t.split(im).join(vname(vOf(im)));
    return t;
  }

  /**
   * The version map: each version the copies run, its copies, and which
   * one the spec asks for. From the operator's `status.versions`, else
   * what the servers say of themselves.
   */
  function versionMap() {
    const sv = st().versions;
    const want = spec().image;
    const wantV = vOf(want);
    let list;
    if (Array.isArray(sv) && sv.length) {
      list = sv.map((v) => ({ key: v.digest || v.image, v: known(cat(), v.digest || v.image), count: v.count ?? (v.copies || []).length, copies: v.copies || [] }));
    } else {
      const by = new Map();
      for (const c of copies()) {
        if (c.down) continue; // not answering: no version to count, said apart (downLine)
        const k = c.ver || "unknown";
        if (!by.has(k)) by.set(k, []);
        by.get(k).push(c.short);
      }
      list = [...by.entries()].map(([k, cs]) => ({ key: k, v: k.startsWith("sha256:") || k.includes("/") ? known(cat(), k) : { tags: [k], image: null }, count: cs.length, copies: cs, fromServers: true }));
      // Without the operator's map, the upgrader's count of what the pods pulled.
      if ((!list.length || list.every((x) => x.fromServers && !x.key.startsWith("sha256:"))) && (st().upgrade?.running || []).length) {
        list = st().upgrade.running.map((x) => ({ key: x.digest, v: known(cat(), x.digest), count: x.servers, copies: [], countsOnly: true }));
      }
    }
    list.sort((a, b) => b.count - a.count);
    const isWant = (x) => !!want && ((wantV?.digest && x.key === wantV.digest) || x.key === want || (x.v?.image && x.v.image === want) || (!wantV?.digest && st().upgrade?.current?.digest === x.key));
    for (const x of list) x.target = isWant(x);
    return list;
  }

  /**
   * Copies that do not answer and shards that take no writes, in one line:
   * restarting another copy then could take a shard down.
   */
  function downLine() {
    const gone = copies().filter((c) => c.down);
    const stuck = (state.cluster?.shards || []).filter((x) => x.takes_writes === false || x.state === "no_writes");
    if (!gone.length && !stuck.length) return null;
    const first = gone.map((c) => c.since).filter(Boolean).sort((a, b) => a - b)[0];
    const words = [];
    if (gone.length) words.push(`${gone.map((c) => c.short + (c.was === "primary" ? " (the primary)" : "")).join(", ")} ${first ? `${gone.length === 1 ? "has" : "have"} not answered since ${clock(first)}` : `${gone.length === 1 ? "does" : "do"} not answer`}`);
    if (stuck.length) words.push(`shard ${stuck.map((x) => x.shard).join(", ")} ${stuck.length === 1 ? "takes" : "take"} no writes`);
    const shard = (gone[0]?.shard ?? stuck[0]?.shard);
    return h("div", { class: "ro-hd-down" }, icon("warn"),
      h("span", {}, words.join(": ") + ". Restart no other copy until ", gone.length === 1 ? "it is" : "they are", " back. ",
        h("a", { href: `#/cluster?shard=${shard}` }, `Open shard ${shard}`)));
  }

  // ---------------------------------------------------------- what is happening
  /** running | held | paused | done | rolled back | rolling back | failed | version failed | degraded */
  function mode() {
    const s0 = st(), r0 = ro();
    if (s0.phase === "Failed") return "failed";
    if (r0.failed && !r0.ungated) return "version failed";
    if (r0.held) return "held";
    if (s0.phase === "Paused" || r0.paused || spec().rollout?.paused) return "paused";
    const cur = history()[history().length - 1];
    if (s0.phase === "Ready" && kube?.settled) return cur?.restores ? "rolled back" : "done";
    if (s0.phase === "Degraded") return "degraded";
    return cur?.restores && r0.ungated ? "rolling back" : "running";
  }
  const moving = (m) => ["running", "held", "paused", "rolling back", "version failed"].includes(m);
  /** The version to go back to: the last one before the current that did not fail. */
  function backTo() {
    const hs = history(), cur = hs[hs.length - 1];
    return hs.slice(0, -1).reverse().find((v) => !v.failed && v.hash !== cur?.hash) || null;
  }

  /** Where the change is, as numbers: done of total, pace, time left. */
  function progress() {
    const p = st().progress, r0 = ro(), map = versionMap();
    const tgt = map.find((x) => x.target);
    const total = p?.total ?? r0.copies ?? ((spec().shards || 0) * (spec().replicas || 0));
    const done = p?.done ?? (r0.copies ? r0.copies - (r0.stale || []).length : tgt?.count ?? null);
    let per = p?.per_copy_s ?? null, how = p?.per_copy_s ? "server" : null;
    if (per == null && paceSeen.length > 2) {
      const [t0, d0] = paceSeen[0], [t1, d1] = paceSeen[paceSeen.length - 1];
      if (d1 > d0) { per = (t1 - t0) / 1000 / (d1 - d0); how = "page"; }
    }
    const left = total != null && done != null ? Math.max(0, total - done) : null;
    const eta = p?.eta_s ?? (per != null && left != null ? left * per : null);
    return { total, done, left, per, how, eta, started: p?.started_ms ?? history()[history().length - 1]?.atMs ?? null, phase: p?.phase || null };
  }

  /** The copy restarting now, the one held before, and since when. */
  function where() {
    const r0 = ro(), msg = st().message || "";
    const stepName = r0.step?.name || "";
    const nowUrl = stepName.startsWith("restart ") ? stepName.slice(8) : null;
    const next = msg.split("; next: ")[1] || "";
    const nextUrl = (next.match(/restarting (\S+) with/) || [])[1] || (msg.match(/^restarting (\S+) with/) || [])[1] || null;
    const waiting = (msg.match(/waiting for (.+?) to be back/) || [])[1] || null;
    return { now: nowUrl, since: r0.step?.sinceMs || null, next: nowUrl ? null : nextUrl, nextWords: next ? said(next) : null, waiting: waiting ? said(waiting) : null };
  }

  /** The gate numbers of each version's copies, worst first, and the copy that stands out. */
  function blame() {
    const map = versionMap();
    const byKey = new Map();
    for (const x of map) for (const c of x.copies) byKey.set(c, x.key);
    const groups = new Map(map.map((x) => [x.key, { x, copies: [] }]));
    for (const c of copies()) {
      const k = byKey.get(c.short) || c.ver;
      if (groups.has(k)) groups.get(k).copies.push(c);
    }
    const stat = (cs, k, f = Math.max) => { const v = cs.map((c) => c.load[k]).filter((x) => typeof x === "number"); return v.length ? f(...v) : null; };
    const median = (...v) => { const a = v.slice().sort((x, y) => x - y); return a[Math.floor(a.length / 2)]; };
    const out = [...groups.values()].filter((g) => g.copies.length).map((g) => ({
      ...g, p99: stat(g.copies, "p99_ms"), p99mid: stat(g.copies, "p99_ms", median), errors: stat(g.copies, "error_ratio"), behind: stat(g.copies, "replica_behind_max"), mem: stat(g.copies, "rss_bytes", median),
      slowest: g.copies.filter((c) => typeof c.load.p99_ms === "number").sort((a, b) => b.load.p99_ms - a.load.p99_ms)[0] || null,
    }));
    // A copy stands out when it answers far slower than the rest of its version.
    const tgt = out.find((g) => g.x.target);
    let odd = null;
    if (tgt?.slowest && tgt.p99mid != null && tgt.slowest.load.p99_ms > Math.max(2 * tgt.p99mid, tgt.p99mid + 50)) {
      const rest = tgt.copies.filter((c) => c !== tgt.slowest);
      odd = { copy: tgt.slowest, restP99: stat(rest, "p99_ms") };
    }
    return { groups: out, odd };
  }

  // ---------------------------------------------------------- loading
  async function load() {
    try { kube = await api("/v1/kube"); kubeErr = null; } catch (e) { kubeErr = e; kube = kube || { managed: false }; }
    follow();
    draw();
    loadVersions();
  }
  async function loadVersions(fresh = false) {
    try { reg = await api("/v1/admin/kube/versions?limit=40" + (fresh ? "&fresh=1" : "")); }
    catch (e) { reg = e.status === 404 || e.status === 405 ? { missing: true } : { error: e.message, status: e.status }; }
    draw();
  }
  function sample() {
    const n = numbers(state.cluster), t = Date.now();
    for (const g of GATES) {
      const a = (series[g.key] ||= []);
      if (n[g.key] != null) a.push([t, n[g.key]]);
      if (a.length > 600) a.splice(0, a.length - 600);
    }
    if (managed() && !st().progress) {
      const d = progress().done;
      if (d != null && (!paceSeen.length || paceSeen[paceSeen.length - 1][1] !== d)) paceSeen.push([t, d]);
      if (paceSeen.length > 30) paceSeen.shift();
    }
  }

  // ---------------------------------------------------------- acting
  async function patch(p, what, reason) {
    busy = true; draw();
    try { kube = await api("/v1/admin/kube", { body: { spec: p }, reason }); toast(what, "ok"); draft = null; follow(); }
    catch (e) { toast("Not changed: " + e.message, "bad", 9000); }
    busy = false; draw();
  }
  const pause = () => patch({ rollout: { paused: true } }, "Paused: no copy restarts until it is resumed");
  async function unpause() {
    const w = where();
    const ok = await ask({ title: "Resume the change?", ok: "Resume", reason: "required", body: h("div", { class: "stack" },
      h("p", {}, `The operator goes on from where it paused${w.nextWords ? ": next, " + w.nextWords : ""}. The gates stay on and hold it again if a number goes over its limit.`)) });
    if (ok) patch({ rollout: { paused: false } }, "Resumed: the operator goes on from where it paused", ok.reason);
  }
  async function resume() {
    const r0 = ro(), g = gateOf(r0.held?.gate), w = where(), now = numbers(state.cluster)[r0.held?.gate];
    const failed = !r0.held && r0.failed;
    const ok = await ask({
      title: failed ? "Try the version again?" : "Resume while the gate is over its limit?", ok: failed ? "Try it again" : "Resume anyway", reason: "required",
      body: h("div", { class: "stack" },
        failed ? h("p", {}, `It failed (${r0.failed.step}): ${said(r0.failed.text)}. Trying it again starts that step's clock again; if it fails again, it is ${(spec().rollout?.onFailure || "rollback") === "rollback" ? "rolled back" : "held again"}.`)
          : h("p", {}, `${said(r0.held?.text)}${g && now != null ? `; it reads ${g.show(now)} now` : ""}.`),
        !failed ? h("p", {}, `Resuming restarts ${w.next ? nameOf(w.next) : "the next copy"} now. The gate stays on: if ${g ? g.short : "the number"} stays over ${g ? g.show(r0.held.limit) : "its limit"} for ${spec().rollout?.holdAfterSeconds ?? 30} s, it holds again.`) : null),
    });
    if (!ok) return;
    busy = true; draw();
    try { kube = await api("/v1/admin/kube/resume", { method: "POST", body: {}, reason: ok.reason }); toast("Resumed: the gates count again from now", "ok"); }
    catch (e) { toast("Not resumed: " + e.message, "bad", 9000); }
    busy = false; draw();
  }
  async function raise() {
    const r0 = ro(), g = gateOf(r0.held?.gate);
    if (!g) return;
    const now = numbers(state.cluster)[g.key];
    let v = snap(g, Math.max(r0.held.value * 1.15, (now ?? 0) * 1.15, r0.held.limit + stepOf(g, r0.held.limit)));
    const val = h("b", { class: "ro-patch-new" });
    const drawV = () => { val.textContent = g.show(v); };
    drawV();
    const stepBtn = (d, label) => h("button", { class: "btn sm", "aria-label": label, onclick: () => { v = clamp(snap(g, v + d * stepOf(g, v)), 0, g.max); drawV(); } }, d < 0 ? "−" : "+");
    const ok = await ask({
      title: `Raise the ${g.short} gate?`, ok: "Change the gate", reason: "optional",
      body: h("div", { class: "stack" },
        h("div", { class: "ro-patch" }, h("code", {}, `spec.rollout.gates.${g.key}`), h("span", { class: "ro-patch-old" }, g.show(r0.held.limit)), "→", stepBtn(-1, "Lower"), val, stepBtn(1, "Raise")),
        h("p", {}, `No server restarts. The hold lifts and the rollout goes on; it holds again if ${g.short} stays over the new limit for ${spec().rollout?.holdAfterSeconds ?? 30} s. ${g.label} reads ${g.show(now)} now.`),
        h("p", { class: "small dim" }, "The new limit stays for later changes too, until someone changes it back (Gates)."))
    });
    if (ok) patch({ rollout: { gates: { [g.key]: v } } }, `${g.label} may now reach ${g.show(v)}`, ok.reason);
  }

  /** The cost of going back to `v`: the copies that differ restart, one at a time. */
  function rollbackCost(v, plan) {
    const map = versionMap(), p = progress();
    const backV = vOf(v?.spec?.image);
    const onBack = map.find((x) => (backV?.digest && x.key === backV.digest) || x.key === v?.spec?.image)?.count;
    const total = p.total ?? map.reduce((t, x) => t + x.count, 0);
    const n = typeof plan?.restarts === "number" ? plan.restarts : onBack != null && total != null ? total - onBack : map.filter((x) => x.target).reduce((t, x) => t + x.count, 0) || null;
    // Without a pace: as long as the version's own rollout took, for as many copies.
    const cur = history()[history().length - 1];
    const took = cur?.readyMs && total ? ((cur.readyMs - cur.atMs) / 1000) / total : null;
    const verify = spec().rollout?.verifySeconds ?? 0;
    const secs = plan?.estimate_s ?? (n != null && p.per ? n * p.per + verify : n != null && took ? n * took : null);
    return { n, secs, stay: onBack ?? null };
  }
  async function rollback(v) {
    let plan;
    try { plan = await api("/v1/admin/kube/plan", { body: { rollback: v.version } }); }
    catch (e) { toast("No rollback: " + e.message, "bad", 9000); return; }
    if (plan.refused) { toast("No rollback: " + plan.refused, "bad", 9000); return; }
    const cost = rollbackCost(v, plan), from = vOf(spec().image), to = vOf(v.spec?.image), image = v.spec?.image !== spec().image;
    const ok = await ask({
      title: image ? `Roll ${kube.namespace}/${kube.name} back to ${vname(to)}?` : `Roll ${kube.namespace}/${kube.name} back to spec v${v.version}?`,
      ok: cost.n != null ? `Roll back ${plural(cost.n, "copy", "copies")}` : "Roll back", danger: true, reason: "optional",
      body: h("div", { class: "stack" },
        facts([
          image ? ["From", vsaid(from)] : null,
          image ? ["To", h("span", {}, vsaid(to), ` · the version of spec v${v.version}, ${clock(v.atMs)}${v.by ? " by " + who(v.by) : ""}`)] : ["To", `spec v${v.version}: ${summary(history().find((x) => x.version === v.version - 1)?.spec, v.spec)}`],
          ["Restarts", cost.n != null ? `${plural(cost.n, "copy", "copies")}, one at a time${cost.stay ? `; the ${plural(cost.stay, "copy", "copies")} already on ${vname(to)} do not restart` : ""}` : "every copy whose settings differ, one at a time"],
          ["Takes", cost.secs != null ? `about ${span(cost.secs)}` : "about as long as the rollout took"],
          image ? ["On disk", `the files stay as they are. ${vname(to)} opens them only if it has every feature they use (the files list them): one that ${vname(from)} wrote since the upgrade stops a copy from starting, and says which.`] : null,
          ["Gates", "hold no step of a rollback, so that it goes through while the version it leaves is unwell"],
        ]),
        (plan.steps || []).length ? h("ol", { class: "ro-ol" }, plan.steps.map((x) => h("li", {}, said(x)))) : null,
        (plan.warnings || []).filter((w) => !/^a rollback: no gate/.test(w)).map((w) => h("div", { class: "ro-warnline" }, icon("warn"), h("span", {}, w))),
        h("p", { class: "small dim" }, "The spec goes back to that version; its rollout settings, gates and upgrade mode stay as they are now.")),
    });
    if (!ok) return;
    busy = true; draw();
    try { kube = await api("/v1/admin/kube", { body: { rollback: v.version }, reason: ok.reason }); toast(`Rolling back to ${image ? vname(to) : "spec v" + v.version}`, "ok"); }
    catch (e) { toast("Not rolled back: " + e.message, "bad", 9000); }
    busy = false; draw();
  }

  /** Upgrades to `image`: its plan first, then a confirmation with the restarts and the time. */
  async function upgrade(image, force = false) {
    let plan;
    try { plan = await api("/v1/admin/kube/upgrade", { body: { image, plan: true, force } }); }
    catch (e) {
      if (/rolled back once/.test(e.message) && !force) {
        const again = await ask({ title: "This version was rolled back once", ok: "Plan it anyway", body: h("p", {}, said(e.message) + ". The upgrader will not roll it out on its own again; you may.") });
        if (again) upgrade(image, true);
        return;
      }
      toast("No upgrade: " + e.message, "bad", 9000); return;
    }
    const from = vOf(plan.from || spec().image), to = { ...vOf(image || plan.image?.image), ...(plan.image ? { revision: plan.image.revision, created: plan.image.created, digest: plan.image.digest } : {}) };
    const map = versionMap(), p = progress();
    const toKey = to.digest || image;
    const already = map.find((x) => x.key === toKey)?.count || 0;
    const total = p.total ?? map.reduce((t, x) => t + x.count, 0) ?? 0;
    const n = typeof plan.restarts === "number" ? plan.restarts : total ? total - already : null;
    const per = p.per;
    const secs = plan.estimate_s ?? (n != null && per ? n * per : null);
    const back = plan.rollback_estimate_s ?? (n != null && per ? n * per : null);
    const r0 = spec().rollout || {};
    const cmp = compareLink(from, to);
    const refused = plan.refused;
    // An image built before the one that runs is a downgrade, said so (NEW-cluster-admin-9).
    const older = sec(to.created) && sec(from.created) && sec(to.created) < sec(from.created);
    const verb = older ? "Downgrade" : "Upgrade";
    const onFail = (r0.onFailure || "rollback") === "rollback" ? "the operator rolls the change back" : "the change is held for you";
    const ok = await ask({
      title: `${verb} ${kube.namespace}/${kube.name} to ${vname(to)}?`, ok: refused ? "Refused" : `${verb} ${n != null ? plural(n, "copy", "copies") : ""}`.trim(), reason: "optional", danger: !!older,
      body: h("div", { class: "stack" },
        h("div", { class: "ro-fromto" },
          h("div", {}, h("div", { class: "ro-k" }, "From"), vsaid(from), h("div", { class: "small dim" }, map.length ? map.map((x) => `${plural(x.count, "copy", "copies")} on ${vname(x.v)}`).join(" · ") : "")),
          h("div", { class: "ro-arrow" }, "→"),
          h("div", {}, h("div", { class: "ro-k" }, "To"), vsaid(to), h("div", { class: "small dim" }, [(to.tags || []).includes("latest") ? "the newest on latest" : null, sec(to.created) ? "built " + ago(sec(to.created)) : null].filter(Boolean).join(" · ")))),
        older ? h("div", { class: "ro-warnline" }, icon("warn"), h("span", {}, `This goes back to an older version: ${vname(to)} was built ${span((sec(from.created) - sec(to.created)) / 1000)} before ${vname(from)}, which runs now.`)) : null,
        cmp ? h("a", { href: cmp, target: "_blank", rel: "noopener", class: "small" }, icon("ext"), ` What changed between ${vname(from)} and ${vname(to)}`) : null,
        facts([
          // What the files on disk become (RO-6): the format stays; the features in use decide.
          ["On disk", older
            ? `the files stay as they are. ${vname(to)} opens them only if it has every feature they use (the files list them). A copy that lacks one does not start and says which feature; then ${onFail}.`
            : `the files stay as they are: ${vname(to)} reads them as written, and writes a newer feature only when a change uses it.`],
          ["Restarts", n != null ? `${plural(n, "copy", "copies")}, one at a time, replicas first; each primary hands over before it restarts` : "every copy, one at a time"],
          ["Takes", secs != null ? `about ${span(secs)}${per ? ` (${Math.round(per)} s a copy, as the last rollout went)` : ""}` : "unknown: this server does not estimate it"],
          ["Queries", "keep being answered all along: copies of both versions serve side by side"],
          ["Rollback", `possible, ${back != null ? "about " + span(back) : "as long again"}, while the data uses no feature ${vname(from)} lacks. ${(r0.onFailure || "rollback") === "rollback" ? "If a step fails, the operator rolls it back on its own." : "If a step fails, it is held for you to resume or roll back."}`],
          ["Verify", `${r0.verifySeconds ?? 60} s healthy at the end: every copy back and in sync, the gates under their limits`],
        ]),
        (plan.steps || []).length ? h("details", {}, h("summary", { class: "small" }, `The operator's ${plural(plan.steps.length, "step")}`), h("ol", { class: "ro-ol" }, plan.steps.map((x) => h("li", {}, said(x))))) : null,
        (plan.warnings || []).map((w) => h("div", { class: "ro-warnline" }, icon("warn"), h("span", {}, w))),
        refused ? h("div", { class: "error" }, "The operator would refuse it: " + refused) : null,
        moving(mode()) ? h("div", { class: "ro-warnline" }, icon("warn"), h("span", {}, "A change is under way: the upgrade replaces it, from where it is.")) : null),
    });
    if (!ok || refused) return;
    busy = true; draw();
    try { kube = await api("/v1/admin/kube/upgrade", { body: { image, force }, reason: ok.reason }); toast(`${older ? "Downgrading" : "Upgrading"} to ${vname(to)}: the operator restarts one copy at a time`, "ok"); setTab("now"); }
    catch (e) { toast(`Not ${older ? "downgraded" : "upgraded"}: ${e.message}`, "bad", 9000); }
    busy = false; draw();
  }
  async function setMode(v) {
    const was = spec().upgrade?.mode || "off";
    if (v === was) return;
    const said0 = { off: "Off", suggest: "Suggest", on: "Automatic" };
    const ok = await ask({
      title: `Upgrades: ${said0[v]}?`, ok: `Set to ${said0[v]}`, reason: "optional",
      body: h("div", { class: "stack" },
        h("div", { class: "ro-patch" }, h("code", {}, "spec.upgrade.mode"), h("span", { class: "ro-patch-old" }, was), "→", h("b", { class: "ro-patch-new" }, v)),
        h("p", {}, MODES.find((m) => m[0] === v)[2]),
        h("p", { class: "small dim" }, "No server restarts for this change itself.")),
    });
    if (ok) patch({ upgrade: { mode: v } }, `Upgrades: ${said0[v]}`, ok.reason);
    else draw();
  }

  // ---------------------------------------------------------- drawing
  function setTab(t) {
    tab = pick(t);
    const q = new URLSearchParams(location.hash.split("?")[1] || "");
    if (tab === "now") q.delete("tab"); else q.set("tab", tab);
    const want = "#/rollout" + (q.toString() ? "?" + q : "");
    if (location.hash !== want) window.history.replaceState(null, "", want);
    draw();
  }
  onTab = (t) => { if (pick(t) !== tab) { tab = pick(t); draw(); } };

  function draw() {
    hideTip();
    drawHead();
    if (!kube) { clear(status, h("div", { class: "small dim" }, "Loading the cluster…")); clear(body); return; }
    drawStatus();
    if (tab === "upgrades") drawUpgrades();
    else if (tab === "history") drawHistory();
    else if (tab === "gates") drawGates();
    else drawNow();
  }

  function drawHead() {
    const cur = history()[history().length - 1];
    const name = managed() ? `${kube.namespace}/${kube.name}` : state.cluster?.cluster?.name || location.host;
    const sp = spec();
    const sub = managed() && kube ? [
      h("b", {}, name),
      ` · ${plural(sp.shards, "shard")} × ${plural(sp.replicas, "copy", "copies")} · ${plural(sp.coordinators || 1, "coordinator")}`,
      cur ? [" · ", h("span", { title: "The operator numbers each change of the StrataCluster's spec; this is not a Strata version" }, `spec v${cur.version}`), ` started ${clock(cur.atMs)} ${TZ}`, cur.by ? ` by ${cur.by === "upgrader" ? "the upgrader (automatic)" : cur.by === "operator" ? "the operator (automatic)" : cur.by === "autoscaler" ? "the autoscaler (automatic)" : cur.by}` : ""] : null,
    ] : kube ? [h("b", {}, name), " · machines of your own: no operator rolls changes out"] : "";
    const tabs = seg(TABS.filter(([k]) => managed() || k !== "history").map(([k, l]) => [k, l]), tab, (v) => setTab(v), { "aria-label": "Rollouts" });
    clear(head, h("div", { class: "page-head" }, h("div", {}, h("h1", {}, "Rollouts"), h("p", {}, sub)), h("div", { class: "tools" }, tabs)));
  }

  // ---------------------------------------------------------- the status header
  function action({ label, sub, onclick, cls = "", disabled, title, href }) {
    const kids = [h("span", { class: "ro-act-l" }, label), sub ? h("small", {}, sub) : null];
    const off = disabled || busy;
    if (href) return h("a", { class: "btn ro-act " + cls, href, title }, kids);
    return h("button", { class: "btn ro-act " + cls, disabled: off, title: disabled && typeof disabled === "string" ? disabled : title, onclick }, kids);
  }
  const adminOnly = () => (can("admin") ? false : NEEDS);

  function drawStatus() {
    if (!managed()) { drawOwnStatus(); return; }
    const m = mode(), r0 = ro(), p = progress(), w = where(), map = versionMap(), hs = history();
    const cur = hs[hs.length - 1], prev = hs[hs.length - 2];
    const imageChange = !!(cur && prev && prev.spec?.image !== cur.spec?.image);
    const toV = vOf(spec().image);
    const leaving = cur?.restores ? hs.find((v) => v.version === hs[hs.length - 2]?.version) : null;
    const fromV = cur?.restores ? vOf(leaving?.spec?.image) : vOf(prev?.spec?.image);
    const back = backTo();
    const tgt = map.find((x) => x.target);
    const others = map.filter((x) => !x.target);
    const what = imageChange ? `to ${vname(toV)}` : cur ? summary(prev?.spec, cur.spec, (i) => vname(vOf(i))) : "";
    const b = blame();
    let tone, badge, title, why = [], safe = null, acts = [];
    const count = p.done != null && p.total != null ? `${fmt.n(p.done)} of ${fmt.n(p.total)} copies` : null;
    const cost = back ? rollbackCost(back) : null;
    const rollbackAct = (primary) => back ? action({
      label: imageChange || back.spec?.image !== spec().image ? `Roll back ${cost?.n != null ? plural(cost.n, "copy", "copies") + " " : ""}to ${((t) => (named(t) || commitOf(t) ? vname(t) : `spec v${back.version}'s image`))(vOf(back.spec?.image))}…` : `Roll back to spec v${back.version}…`,
      sub: [cost?.secs != null ? `about ${span(cost.secs)}` : null, "one copy at a time", "shows the plan first"].filter(Boolean).join(" · "),
      cls: primary ? "danger solid" : "danger", disabled: adminOnly(), onclick: () => rollback(back),
    }) : null;
    const g = gateOf(r0.held?.gate);
    const nextShard = w.next ? (/^s(\d+)-/.exec(nameOf(w.next)) || [])[1] : null;
    if (m === "held") {
      tone = "held"; badge = "Held";
      title = `${imageChange ? "Upgrade " + what : "The change"} is held${nextShard != null ? ` at shard ${nextShard} of ${spec().shards}` : ""}`;
      const now = numbers(state.cluster)[r0.held.gate];
      why.push(h("span", {}, `${g ? g.label : "A gate"} went over its ${g ? g.show(r0.held.limit) : ""} limit at `, h("b", {}, at(r0.held.atMs)), ` (${ago(r0.held.atMs)})`, now != null && g ? [": ", h("b", {}, g.show(now)), " now"] : "", ". "));
      if (b.odd && r0.held.gate === "p99Ms") why.push(h("span", {}, "It comes from one copy, ", h("a", { class: "ro-in", href: `#/cluster?shard=${b.odd.copy.shard}`, title: b.odd.copy.url }, h("b", {}, b.odd.copy.short)), ` (${ms(b.odd.copy.load.p99_ms)}, on ${vname(tgt?.v)}${b.odd.copy.up_s ? " since " + clock(Date.now() - b.odd.copy.up_s * 1000) : ""}); the other copies on ${vname(tgt?.v)} answer within ${ms(b.odd.restP99)}.`));
      safe = `Nothing is restarting.${others.length && tgt ? ` Copies on ${vname(tgt.v)} and ${others.map((x) => vname(x.v)).join(", ")} serve reads and writes side by side;` : ""} staying held is safe.`;
      if (b.odd) acts.push(action({ label: `Open ${b.odd.copy.short}`, sub: "its shard, logs and profile", cls: "primary", href: `#/cluster?shard=${b.odd.copy.shard}` }));
      acts.push(rollbackAct(false));
      acts.push(action({ label: "Resume anyway…", sub: "asks for a reason · the gate stays on", disabled: adminOnly(), onclick: resume }));
      if (g) acts.push(action({ label: "Raise the limit…", sub: "shows the change to the gate first", cls: "ghost", disabled: adminOnly(), onclick: raise }));
    } else if (m === "paused") {
      tone = "paused"; badge = "Paused";
      title = `${imageChange ? "Upgrade " + what : "The change"} is paused${count ? " at " + count : ""}`;
      const pausedAt = [...(st().log || [])].reverse().find((e) => /^paused/.test(e.message))?.atMs;
      why.push(`Paused${pausedAt ? " at " + clock(pausedAt) : ""} (spec.rollout.paused).${w.nextWords ? " Next: " + w.nextWords + "." : ""}`);
      safe = `No copy restarts until it is resumed.${others.length ? " Copies on both versions serve side by side." : ""}`;
      acts.push(action({ label: "Resume…", sub: "asks for a reason", cls: "primary", disabled: adminOnly(), onclick: unpause }));
      acts.push(rollbackAct(false));
    } else if (m === "version failed") {
      tone = "failed"; badge = "Failed";
      title = `${imageChange ? vname(toV) : "The change"} failed ${r0.failed.step === "verify" ? "its verification" : "at step " + r0.failed.step}`;
      why.push(`${said(r0.failed.text)} (${clock(r0.failed.atMs)}). `);
      if (b.odd && /p99/.test(r0.failed.text)) why.push(h("span", {}, "It comes from one copy, ", h("a", { class: "ro-in", href: `#/cluster?shard=${b.odd.copy.shard}`, title: b.odd.copy.url }, h("b", {}, b.odd.copy.short)), ` (${ms(b.odd.copy.load.p99_ms)}); the other copies on ${vname(tgt?.v)} answer within ${ms(b.odd.restP99)}. `));
      safe = `The operator holds it (onFailure: hold): no other copy restarts with it.${others.length ? " Both versions serve side by side until you roll back or try again." : ""}`;
      if (b.odd) acts.push(action({ label: `Open ${b.odd.copy.short}`, sub: "its shard, logs and profile", href: `#/cluster?shard=${b.odd.copy.shard}` }));
      acts.push(rollbackAct(true));
      acts.push(action({ label: "Try it again…", sub: "asks for a reason", disabled: adminOnly(), onclick: resume }));
    } else if (m === "rolling back") {
      tone = "back"; badge = "Rolling back";
      const failed = hs.find((v) => v.version === hs[hs.length - 2]?.version);
      title = `Rolling back to ${vname(vOf(spec().image))}${count ? `: ${count} back` : ""}`;
      if (failed?.failed) why.push(`${vname(fromV)} failed ${failed.failed.step === "verify" ? "its verification" : "at " + failed.failed.step} at ${clock(failed.failed.atMs)}: ${said(failed.failed.text)}. `);
      why.push(`${cur.by === "operator" ? "The operator rolls it back on its own" : `${cur.by || "Someone"} rolls it back`}; no gate holds a rollback.`);
      if (w.now) why.push(` Now restarting ${nameOf(w.now)}.`);
      safe = "Copies of both versions serve side by side while it runs.";
      acts.push(action({ label: "Pause the rollback", sub: "copies keep the version they run", disabled: adminOnly(), onclick: pause }));
    } else if (m === "running") {
      tone = "run"; badge = "Rolling out";
      title = imageChange ? `Upgrading ${kube.name} ${what}${count ? `: ${count} done` : ""}` : `Changing ${kube.name}: ${what}`;
      const nowName = w.now ? nameOf(w.now) : null, nowC = copies().find((c) => c.short === nowName);
      if (nowName) why.push(h("span", {}, "Now restarting ", h("b", {}, nowName), nowC ? ` (${nowC.role}${nowC.node ? " on " + nowC.node : ""}${w.since ? ", " + span((Date.now() - w.since) / 1000) + " in" : ""})` : "", ". "));
      else if (w.waiting) why.push(`Waiting for ${w.waiting} to be back and caught up. `);
      else if (st().message) why.push(said(st().message) + ". ");
      const gatesOn = GATES.filter((x) => spec().rollout?.gates?.[x.key] != null);
      safe = `${gatesOn.length ? `${gatesOn.length === 1 ? "The gate is" : `All ${gatesOn.length} gates are`} under ${gatesOn.length === 1 ? "its limit" : "their limits"}.` : "No gate is set: nothing holds it but a pause."}${others.length ? " Mixed versions are expected while it runs." : ""}`;
      acts.push(action({ label: "Pause", sub: nowName ? `after ${nowName} is back; nothing else restarts` : "the operator takes no further step", disabled: adminOnly(), onclick: pause }));
      acts.push(rollbackAct(false));
    } else if (m === "failed") {
      tone = "failed"; badge = "Failed";
      title = "The operator cannot change the cluster";
      why.push(said(st().message || "") + ".");
      acts.push(rollbackAct(false));
    } else {
      // Done, rolled back or degraded: what runs, honestly.
      const all = map.length === 1 && map[0].target;
      tone = m === "degraded" ? "warn" : all ? "ok" : "warn";
      badge = m === "rolled back" ? "Rolled back" : m === "degraded" ? "Degraded" : all ? "Done" : "Mixed";
      if (all) title = `${kube.name} runs ${vname(map[0].v)} on ${map[0].countsOnly ? "" : "all "}${plural(map[0].count, "copy", "copies")}`;
      else if (map.length) title = `Copies run ${plural(map.length, "version")}: ${map.map((x) => `${fmt.n(x.count)} on ${vname(x.v)}`).join(", ")}`;
      else title = `${kube.name} is as its spec says`;
      if (m === "rolled back") {
        const failed = hs.find((v) => v.version === hs[hs.length - 2]?.version);
        why.push(`${vname(fromV)} ${failed?.failed ? `failed ${failed.failed.step === "verify" ? "its verification" : "at " + failed.failed.step}: ${said(failed.failed.text)}, and ` : ""}${cur.by === "operator" ? "the operator" : cur.by || "someone"} rolled it back (${clock(cur.atMs)} to ${clock(cur.readyMs)}). The upgrader does not roll it out again.`);
      } else if (cur && imageChange) {
        why.push(`Upgraded from ${vname(fromV)}${cur.by ? " by " + who(cur.by) : ""}, ${clock(cur.atMs)} to ${clock(cur.readyMs)}${cur.readyMs ? ` (${span((cur.readyMs - cur.atMs) / 1000)})` : ""}${spec().rollout?.verifySeconds ? ", verified" : ""}.`);
      } else if (cur) why.push(`Last change: spec v${cur.version}, ${summary(prev?.spec, cur.spec, (i) => vname(vOf(i)))} (${clock(cur.atMs)}${cur.by ? ", by " + who(cur.by) : ""}).`);
      if (!all && map.length > 1) safe = "Copies on another version than the spec asks for: the operator restarts them on its next step, unless a change is paused.";
      if (m === "degraded") safe = "Some copies do not answer: " + said(st().message || "") + ".";
      const u = st().upgrade;
      if (u?.available && u.latest) acts.push(action({ label: `Upgrade to ${vname(vOf(u.latest.image))}…`, sub: "newer on " + (tagOf(u.track) || "its tag") + " · shows the plan first", cls: "primary", disabled: adminOnly(), onclick: () => upgrade(u.latest.image) }));
      acts.push(action({ label: "Pick a version…", sub: "the published images, newest first", cls: u?.available ? "" : "primary", onclick: () => setTab("upgrades") }));
      if (back && m !== "rolled back") acts.push(rollbackAct(false));
    }
    acts = acts.filter(Boolean);
    // On the other tabs, one line: where it is, and the same actions.
    if (tab !== "now") {
      clear(status, h("section", { class: "ro-hd compact " + tone, "aria-live": "polite" },
        h("div", { class: "ro-hd-title" }, h("span", { class: "ro-badge" }, badge), h("span", {}, title),
          h("button", { class: "btn xs ghost", onclick: () => setTab("now") }, "Details")),
        h("div", { class: "ro-hd-acts" }, acts.filter((x) => !x.classList.contains("ghost")).slice(0, 3))));
      return;
    }
    const bar = moving(m) ? progressBar(m, p) : null;
    clear(status, h("section", { class: "ro-hd " + tone, "aria-live": "polite" },
      h("div", { class: "ro-hd-t" },
        h("div", { class: "ro-hd-title" }, h("span", { class: "ro-badge" }, badge), h("span", {}, title)),
        why.length ? h("div", { class: "ro-hd-why" }, why) : null,
        safe ? h("div", { class: "ro-hd-safe" }, safe) : null,
        downLine(),
        versionStrip(map, p, !moving(m)),
        bar),
      h("div", { class: "ro-hd-acts" }, acts)));
  }

  /** "97 on 5f2e523 · 47 on da65b3d": the version split, as a bar. */
  function versionStrip(map, p, bar = true) {
    if (!map.length) return null;
    const total = map.reduce((t, x) => t + x.count, 0) || 1;
    const items = map.map((x, i) => {
      const cls = x.target ? "t" : "o" + Math.min(i, 3);
      return { x, cls };
    });
    return h("div", { class: "ro-split" },
      bar ? h("div", { class: "ro-split-bar", role: "img", "aria-label": map.map((x) => `${x.count} copies on ${vname(x.v)}`).join(", ") }, items.map(({ x, cls }) => h("i", { class: cls, style: { width: (x.count / total * 100) + "%" } }))) : null,
      h("div", { class: "ro-split-l" }, items.map(({ x, cls }) => h("span", { class: "row nw" }, h("i", { class: "ro-sw " + cls }), h("b", {}, fmt.n(x.count)), " on ", vsaid(x.v, { built: false }), x.target ? h("span", { class: "dim" }, " (the spec)") : null)),
        p.total && map.reduce((t, x) => t + x.count, 0) < p.total ? h("span", { class: "dim" }, `${fmt.n(p.total - total)} not answering`) : null));
  }

  /** The steps of the change as one bar, with the copies most of it. */
  function progressBar(m, p) {
    const list = steps();
    const heavy = list.some((x) => x.copies);
    const flex = (x) => (!heavy || x.copies ? { flex: "1 1 0" } : { flex: "0 0 auto", width: "156px" });
    const cls = m === "held" || m === "paused" || m === "version failed" ? "held" : m === "rolling back" ? "back" : "run";
    const segs = list.map((x) => h("div", { class: "ro-pb-seg" + (x.copies ? " big" : ""), style: flex(x), title: x.title },
      h("i", { class: x.done ? "done" : x.now || x.frac ? cls : "", style: { width: Math.round(clamp(x.frac || 0, 0, 1) * 100) + "%" } })));
    const ticks = list.map((x) => h("div", { class: "ro-pb-tick" + (x.done ? " done" : x.now ? " now" : "") + (x.copies ? " big" : ""), style: flex(x), title: x.title },
      x.done ? icon("check") : null, h("b", {}, x.copies ? x.title : x.short || x.title), x.copies && p.left ? h("span", { class: "dim" }, ` · ${fmt.n(p.left)} to go`) : null, x.when ? h("span", { class: "dim" }, " " + x.when) : null));
    const eta = p.eta != null && m === "running" ? h("div", { class: "ro-pb-eta" }, h("span", { class: "dim" }, p.per ? `${Math.round(p.per)} s a copy${p.how === "page" ? " (seen here)" : ""} · ${plural(p.left, "copy", "copies")} left · ` : ""), "ETA ", h("b", {}, clock(Date.now() + p.eta * 1000)), h("span", { class: "dim" }, ` ${TZ} (${span(p.eta)})`))
      : p.eta != null && (m === "held" || m === "paused") ? h("div", { class: "ro-pb-eta" }, h("span", { class: "dim" }, `after a resume: ${plural(p.left, "copy", "copies")} left, about `), h("b", {}, span(p.eta)))
        : p.eta != null && m === "rolling back" ? h("div", { class: "ro-pb-eta" }, "back in about ", h("b", {}, span(p.eta)), h("span", { class: "dim" }, ` (${clock(Date.now() + p.eta * 1000)} ${TZ})`)) : null;
    return h("div", { class: "ro-pb" }, h("div", { class: "ro-pb-bar" }, segs), h("div", { class: "ro-pb-ticks" }, ticks), eta);
  }

  /**
   * The steps of the current version, in the operator's order, each with
   * its progress on the live cluster and its words in the operator's log.
   */
  function steps() {
    const hs = history(), cur = hs[hs.length - 1], prev = hs[hs.length - 2];
    if (!cur) return [];
    const a = prev?.spec, b = cur.spec, msg = st().message || "", r0 = ro(), p = progress();
    const ready = st().phase === "Ready" && kube?.settled;
    const log = (st().log || []).filter((e) => e.atMs >= cur.atMs - 1000);
    const firstSaid = (re) => log.find((e) => re.test(e.message))?.atMs;
    const out = [];
    const stage = (o) => out.push({ ...o, now: o.re.test(msg), done: ready || o.done });
    if (!a) { stage({ title: "Run the cluster", short: "start", re: /./, frac: null, done: ready }); return finish(out); }
    if (a.image !== b.image && !cur.restores) {
      const t = firstSaid(/trying the image/);
      stage({ title: "Try the image on every node", short: "preflight", re: /trying the image|does not run here/, frac: null, done: r0.preflight === b.image, when: t ? clock(t) : null });
    }
    if (a.coordinators !== b.coordinators || coordinatorConfig(a) !== coordinatorConfig(b)) {
      const t = firstSaid(/coordinators/);
      stage({ title: a.coordinators !== b.coordinators ? `${plural(b.coordinators, "coordinator")}, from ${a.coordinators}` : `${plural(b.coordinators, "coordinator")}`, short: plural(b.coordinators, "coordinator"), re: /restarting the coordinators|updating the coordinators|coordinators, from/, frac: null, done: false, when: t ? clock(t) : null });
    }
    if (a.replicas !== b.replicas) {
      const per = (state.cluster?.shards || []).map((x) => (x.replicas || []).length), ok = per.filter((n) => n === b.replicas).length;
      stage({ title: `${plural(b.replicas, "copy", "copies")} a shard, from ${a.replicas}`, short: `${b.replicas} copies a shard`, re: /copies, from|joins shard|leaves shard|before .* goes/, frac: per.length ? ok / per.length : 0, done: per.length > 0 && ok === per.length });
    }
    if (a.shards !== b.shards) {
      const n = (state.cluster?.shards || []).length, k = Math.abs(b.shards - a.shards);
      stage({ title: b.shards > a.shards ? `Start ${plural(k, "new shard")}` : `Drain and remove ${plural(k, "shard")}`, short: `${b.shards} shards`, re: /new shard|joins the cluster|draining|leaves the cluster|slots off/, frac: 1 - Math.abs(b.shards - n) / k, done: n === b.shards });
      stage({ title: "Even the slots out", short: "slots", re: /slots/, frac: null, done: n === b.shards && !/slots/.test(msg) });
    }
    if (a.storage?.size !== b.storage?.size) stage({ title: `Grow the volumes to ${b.storage?.size}`, short: "volumes", re: /growing|volume/, frac: null, done: false });
    if (serverConfig(a) !== serverConfig(b)) {
      const of = p.total || 0, d = p.done || 0;
      stage({ title: cur.restores ? `${fmt.n(d)} of ${fmt.n(of)} copies back on ${vname(vOf(b.image))}` : `${fmt.n(d)} of ${fmt.n(of)} copies on ${a.image !== b.image ? vname(vOf(b.image)) : "the new settings"}`, short: `${fmt.n(d)} of ${fmt.n(of)} copies`, re: /new settings|restarts with|rolling out|rolling back|leaves shard|handing the primary/, frac: of ? d / of : 0, done: of > 0 && d >= of && !/new settings|leaves shard/.test(msg), copies: true });
    }
    if (serverConfig(a) !== serverConfig(b) || coordinatorConfig(a) !== coordinatorConfig(b)) {
      const v = r0.verify, need = (spec().rollout?.verifySeconds ?? 60) * 1000;
      const frac = v?.passedMs ? 1 : v?.healthySinceMs ? clamp((Date.now() - v.healthySinceMs) / Math.max(1, need), 0, 0.99) : v ? 0.02 : null;
      stage({ title: `Verify (${need / 1000} s healthy)`, short: `verify (${need / 1000} s)`, re: /verifying the version/, frac, done: !!v?.passedMs });
    }
    if (!out.length) stage({ title: "Nothing to do on the servers", short: "nothing to do", re: /$^/, frac: 1, done: true });
    return finish(out);
  }
  function finish(out) {
    const at0 = out.findIndex((x) => x.now && !x.done);
    out.forEach((x, i) => {
      if (at0 >= 0 && i < at0 && !x.copies) x.done = true;
      if (x.done) { x.frac = 1; x.now = false; }
      if (x.frac == null) x.frac = x.now ? 0.5 : 0;
    });
    return out;
  }

  // ---------------------------------------------------------- Now
  function drawNow() {
    if (!managed()) { drawOwnNow(); return; }
    const m = mode();
    const left = h("div", { class: "ro-col" }), right = h("div", { class: "ro-col" });
    left.append(mapCard());
    if (moving(m) || versionMap().length > 1) left.append(blameCard());
    const hs = history(), cur = hs[hs.length - 1], prev = hs[hs.length - 2];
    if (cur && prev && prev.spec?.image === cur.spec?.image && moving(m)) left.append(stepsCard());
    right.append(nowNextCard(), changeCard(), timelineCard());
    clear(body, h("div", { class: "ro-main" }, left, right));
  }

  /** One column a shard, one bar a copy (top: the primary), coloured by version. */
  function mapCard() {
    const map = versionMap(), m = mode(), w = where(), b = blame(), r0 = ro();
    const byCopy = new Map();
    map.forEach((x, i) => x.copies.forEach((c) => byCopy.set(c, { x, i })));
    const nowName = w.now ? nameOf(w.now) : null, nextName = w.next ? nameOf(w.next) : null;
    const live = copies(), byName = new Map(live.map((c) => [c.short, c]));
    const shards = Math.max(spec().shards || 0, (state.cluster?.shards || []).length);
    const per = spec().replicas || Math.max(1, ...((state.cluster?.shards || []).map((x) => (x.replicas || []).length)));
    const gateShard = m === "held" && b.odd ? b.odd.copy.shard : null;
    const verCls = (x, i) => (x.target ? "t" : "o" + Math.min(i, 3));
    if (!map.length || map.every((x) => x.countsOnly)) {
      return card({ title: "Where each copy is", sub: "by version", body: map.length ? h("div", { class: "small dim" }, "This server counts copies by version but does not say which copy runs which: ", map.map((x) => `${fmt.n(x.count)} on ${vname(x.v)}`).join(", "), ".") : h("div", { class: "small dim" }, "No copy has said which version it runs yet.") });
    }
    const cells = [];
    for (let i = 0; i < shards; i++) {
      const bars = [];
      for (let j = 0; j < per; j++) {
        const nm = `s${i}-${j}`, v = byCopy.get(nm), c = byName.get(nm);
        let cls = v ? verCls(v.x, v.i) : "u";
        if (c?.down) cls = "down";
        if (nm === nowName) cls += " now";
        if (nm === nextName && (m === "held" || m === "paused" || m === "version failed")) cls += " wait";
        const bar = h("i", { class: cls });
        tipOn(bar, () => h("div", { class: "stack", style: { gap: "2px" } },
          h("b", {}, nm, c ? ` · ${c.role}` : ""),
          h("div", {}, v ? ["on ", vname(v.x.v), v.x.target ? " (the spec)" : ""] : "version unknown"),
          nm === nowName ? h("div", {}, "restarting now") : nm === nextName ? h("div", {}, "the next to restart") : null,
          c && typeof c.load.p99_ms === "number" ? h("div", { class: "dim" }, `p99 ${ms(c.load.p99_ms)}${c.node ? " · " + c.node : ""}`) : null,
          c?.url ? h("div", { class: "dim mono small" }, c.url) : null));
        bars.push(bar);
      }
      const cls = "ro-map-s" + (i === gateShard ? " hot" : "") + (nextName && nextName.startsWith(`s${i}-`) && (m === "held" || m === "paused") ? " here" : "");
      cells.push(h("a", { class: cls, href: `#/cluster?shard=${i}`, "aria-label": `shard ${i}` }, bars, h("span", { class: "ro-map-n" }, i % 8 === 0 || i === gateShard || (nextName && nextName.startsWith(`s${i}-`)) ? String(i) : "")));
    }
    const legend = [
      ...map.map((x, i) => h("span", {}, h("i", { class: "ro-sw " + verCls(x, i) }), `on ${vname(x.v)} (${fmt.n(x.count)})`)),
      nowName ? h("span", {}, h("i", { class: "ro-sw t now" }), `restarting now (${nowName})`) : null,
      nextName && (m === "held" || m === "paused") ? h("span", {}, h("i", { class: "ro-sw wait" }), `${m} before restarting (${nextName})`) : null,
      gateShard != null ? h("span", {}, h("i", { class: "ro-sw hot" }), `shard over a gate (s${gateShard})`) : null,
      live.some((c) => c.down) ? h("span", {}, h("i", { class: "ro-sw down" }), "not answering") : null,
    ];
    const fromTo = map.length > 1 ? map.filter((x) => !x.target).map((x) => vname(x.v)).join(", ") + " → " + vname(map.find((x) => x.target)?.v) : "";
    return card({
      title: "Where each copy is", sub: `one column a shard, one bar a copy (top: the primary)${fromTo ? " · " + fromTo : ""}`, cls: "ro-mapcard",
      body: h("div", { class: "stack", style: { gap: "10px" } },
        h("div", { class: "ro-map", style: { "--per": per } }, cells),
        h("div", { class: "ro-legend" }, legend),
        r0.copies ? h("div", { class: "small dim", style: { textAlign: "right" } }, "order: shard by shard, the replicas first, then the primary hands over") : null),
    });
  }

  /** The gate numbers split by version: is the new version to blame? */
  function blameCard() {
    const b = blame(), gates = spec().rollout?.gates || {};
    if (!b.groups.length) return card({ title: "Is the new version to blame?", body: h("div", { class: "small dim" }, "The copies have not said their numbers yet.") });
    const cols = b.groups.slice().sort((x, y) => (y.x.target ? 1 : 0) - (x.x.target ? 1 : 0));
    const tgt = cols.find((g) => g.x.target);
    const withoutOdd = b.odd && tgt ? tgt.copies.filter((c) => c !== b.odd.copy) : null;
    const st2 = (cs, k, f = Math.max) => { const v = cs.map((c) => c.load[k]).filter((x) => typeof x === "number"); return v.length ? f(...v) : null; };
    const heads = [...cols.map((g) => `on ${vname(g.x.v)} (${fmt.n(g.copies.length)})`)];
    if (withoutOdd) heads.splice(1, 0, `on ${vname(tgt.x.v)} without ${b.odd.copy.short}`);
    const rows = [
      ["p99, worst copy", "p99_ms", ms, gates.p99Ms],
      ["errors, worst copy", "error_ratio", pct, gates.errorRatio],
      ["replica behind, max", "replica_behind_max", (v) => (v == null ? "–" : fmt.n(Math.round(v)) + " txn"), gates.replicaBehind],
      ["memory a copy, median", "rss_bytes", fmt.bytes, null],
    ];
    const median = (...v) => { const a = v.slice().sort((x, y) => x - y); return a[Math.floor(a.length / 2)]; };
    const cell = (cs, k, show, lim) => { const v = k === "rss_bytes" ? st2(cs, k, median) : st2(cs, k); return h("td", { class: "num" + (lim != null && v != null && v > lim ? " bad" : "") }, show(v)); };
    return card({
      title: tgt && cols.length > 1 && !["rolling back", "rolled back"].includes(mode()) ? "Is the new version to blame?" : "The gate numbers by version", sub: "the copies' own numbers, as they say them now",
      body: h("div", { class: "tbl-wrap" }, h("table", { class: "ro-cmp" },
        h("thead", {}, h("tr", {}, h("th", {}, ""), heads.map((x) => h("th", { class: "num" }, x)), h("th", { class: "num" }, "gate"))),
        h("tbody", {}, rows.map(([label, k, show, lim]) => {
          const cs = cols.map((g) => cell(g.copies, k, show, lim));
          if (withoutOdd) cs.splice(1, 0, cell(withoutOdd, k, show, lim));
          return h("tr", {}, h("td", {}, label), cs, h("td", { class: "num dim" }, lim != null ? show(lim) : "no gate"));
        })))),
    });
  }

  function stepsCard() {
    const list = steps(), m = mode();
    return card({ title: "Steps", sub: "the operator takes them one at a time, the gates read before each", body: h("div", { class: "ro-steps" }, list.map((x, i) => {
      const cls = x.done ? "done" : x.now ? (m === "held" || m === "paused" ? "held" : "now") : "wait";
      return h("div", { class: "ro-step " + cls }, h("div", { class: "ro-step-n" }, x.done ? icon("check") : String(i + 1)),
        h("div", { class: "ro-step-t" }, h("b", {}, x.title), h("div", { class: "ro-bar" }, h("i", { style: { width: `${Math.round(clamp(x.frac || 0, 0, 1) * 100)}%` } }))));
    })) });
  }

  function nowNextCard() {
    const m = mode(), w = where(), p = progress(), r0 = ro(), back = backTo();
    const rows = [];
    const nowName = w.now ? nameOf(w.now) : null, c = copies().find((x) => x.short === nowName);
    if (m === "held" || m === "version failed") rows.push(["Stopped", h("span", {}, "before restarting ", h("b", {}, w.next ? nameOf(w.next) : "the next copy"), "."), ...(p.done != null ? [` ${fmt.n(p.done)} copies are done.`] : [])]);
    else if (m === "paused") rows.push(["Paused", `before ${w.next ? nameOf(w.next) : "the next step"}.`]);
    else if (nowName) rows.push(["Now", h("span", {}, h("b", {}, nowName), c ? ` ${c.role}${c.node ? " on " + c.node : ""}` : "", w.since ? ` · ${span((Date.now() - w.since) / 1000)} in` : "", c && c.load.replica_behind_max ? ` · ${fmt.n(c.load.replica_behind_max)} behind` : "")]);
    else if (moving(m)) rows.push(["Now", said(st().message || "–")]);
    else rows.push(["Now", "Nothing restarts: the cluster is as its spec says."]);
    if (r0.held) {
      const g = gateOf(r0.held.gate);
      rows.push(["Gate", h("span", {}, h("b", {}, `${g?.short || r0.held.gate} ${g ? g.show(r0.held.value) : r0.held.value}`), ` over ${g ? g.show(r0.held.limit) : r0.held.limit} for ${span((Date.now() - r0.held.atMs) / 1000)} (it holds after ${spec().rollout?.holdAfterSeconds ?? 30} s over). Resumes on its own: no.`)]);
    }
    if (moving(m) && w.nextWords) rows.push(["Next", w.nextWords]);
    else if (moving(m) && (r0.stale || []).length) {
      const left = r0.stale.filter((u) => u !== w.now).map(nameOf);
      if (left.length) rows.push(["Next", `${left.slice(0, 3).join(", then ")}${left.length > 3 ? ` … ${plural(left.length, "restart")} left` : ""}`]);
    }
    if (moving(m) && p.left != null) rows.push(["Pace", p.per ? `${Math.round(p.per)} s a copy${p.how === "page" ? " (seen on this page)" : ""} · ${plural(p.left, "copy", "copies")} left${p.eta != null && (m === "running" || m === "rolling back") ? ` · ETA ${clock(Date.now() + p.eta * 1000)} ${TZ}` : p.eta != null ? ` · about ${span(p.eta)} once it goes on` : ""}` : `${plural(p.left, "copy", "copies")} left · this server does not say the pace yet`]);
    if (back && moving(m) && m !== "rolling back") {
      const cost = rollbackCost(back);
      rows.push(["Undo", `Roll back restarts ${cost.n != null ? "the " + plural(cost.n, "copy", "copies") + " on " + vname(vOf(spec().image)) : "the copies already restarted"} again, on ${vname(vOf(back.spec?.image))}${cost.secs != null ? ` (about ${span(cost.secs)})` : ""}.`]);
    }
    return card({ title: "Now and next", body: h("dl", { class: "ro-nn" }, rows.flatMap(([k, v, ...more]) => [h("dt", {}, k), h("dd", {}, v, ...more)])) });
  }

  function changeCard() {
    const hs = history(), cur = hs[hs.length - 1], prev = hs[hs.length - 2];
    if (!cur) return card({ title: "What changes", body: h("div", { class: "small dim" }, "The operator has seen no version of the spec yet.") });
    const imageChange = prev && prev.spec?.image !== cur.spec?.image;
    const a = vOf(prev?.spec?.image), b = vOf(cur.spec?.image);
    const cmp = imageChange ? compareLink(a, b) : null;
    const rows = [
      ["Spec", h("span", {}, prev ? `v${prev.version} → v${cur.version}` : `v${cur.version}`, h("span", { class: "dim" }, " (the operator's count of changes to the StrataCluster)"))],
      imageChange ? ["Version", h("span", {}, vsaid(a), " → ", vsaid(b), cmp ? [" · ", h("a", { class: "ro-in", href: cmp, target: "_blank", rel: "noopener" }, "what changed")] : null)] : null,
      ["Fields", cur.restores ? `back to spec v${cur.restores}` : summary(prev?.spec, cur.spec, (i) => vname(vOf(i)))],
      ["Rollback", backTo() ? `possible: spec v${backTo().version} is kept` : "no earlier version to go back to"],
      imageChange ? ["Image", h("span", { class: "row nw" }, h("code", { class: "ro-img1", title: cur.spec.image }, cur.spec.image.replace(/^([^/]+\/)[^@:]*?([^/@:]+)([@:])/, "$1…/$2$3").replace(/(sha256:[0-9a-f]{12})[0-9a-f]+/, "$1…")), h("button", { class: "btn xs ghost", title: "Copy the image", onclick: () => copy(cur.spec.image, "Image copied") }, icon("copy")))] : null,
    ].filter(Boolean);
    return card({ title: moving(mode()) ? "What changes" : "The last change", body: h("dl", { class: "ro-nn" }, rows.flatMap(([k, v]) => [h("dt", {}, k), h("dd", {}, v)])) });
  }

  function timelineCard() {
    const cur = history()[history().length - 1];
    const log = (st().log || []).filter((e) => !cur || e.atMs >= cur.atMs - 60e3).slice().reverse().slice(0, 12);
    const cls = (e) => (e.phase === "Paused" ? "warn" : e.phase === "Ready" ? "ok" : e.phase === "Failed" ? "bad" : "");
    return card({
      title: "Timeline", sub: `the operator's words, local time (${TZ})`, tools: [h("button", { class: "btn xs ghost", onclick: () => setTab("history") }, "Full log")],
      body: log.length ? h("div", { class: "ro-tl" }, log.map((e) => h("div", { class: "ro-tl-e " + cls(e) }, h("span", { class: "ro-tl-t", title: new Date(e.atMs).toString() }, clock(e.atMs, true)), h("span", {}, said(e.message))))) : h("div", { class: "small dim" }, "Nothing yet: the operator writes each step it takes here."),
    });
  }

  // ---------------------------------------------------------- own machines
  function drawOwnStatus() {
    const map = versionMap();
    const all = map.length === 1;
    const total = map.reduce((t, x) => t + x.count, 0);
    const nDown = copies().filter((c) => c.down).length;
    const title = !map.length ? "No copy has said which version it runs yet" : all ? (nDown ? `Every copy that answers runs ${vname(map[0].v)} (${total} of ${total + nDown})` : `Every copy runs ${vname(map[0].v)} (${plural(total, "copy", "copies")})`) : `Copies run ${plural(map.length, "version")}: ${map.map((x) => `${fmt.n(x.count)} on ${vname(x.v)}`).join(", ")}`;
    const kerr = kubeErr && kubeErr.status !== 404 ? h("div", { class: "small dim" }, "The operator's status: " + kubeErr.message) : null;
    const down = copies().some((c) => c.down);
    const tone = down ? "failed" : all || !map.length ? "ok" : "warn";
    const badge = down ? "Copy down" : all ? "One version" : map.length ? "Mixed" : "Unknown";
    // On the other tabs, one line: what runs, and whether a copy is down.
    if (tab !== "now") {
      clear(status, h("section", { class: "ro-hd compact " + tone, "aria-live": "polite" },
        h("div", { class: "ro-hd-title" }, h("span", { class: "ro-badge" }, badge), h("span", {}, title),
          h("button", { class: "btn xs ghost", onclick: () => setTab("now") }, "Details")),
        h("div", { class: "ro-hd-acts" }, action({ label: "Open the cluster", sub: "hand a primary over, see each copy", href: "#/cluster" }))));
      return;
    }
    clear(status, h("section", { class: "ro-hd " + tone },
      h("div", { class: "ro-hd-t" },
        h("div", { class: "ro-hd-title" }, h("span", { class: "ro-badge" }, badge), h("span", {}, title)),
        h("div", { class: "ro-hd-why" }, "No operator runs this cluster: a server takes a new version when you restart it with it. Restart the replicas first, then hand each primary over and restart it, one copy at a time."),
        all || !map.length ? null : h("div", { class: "ro-hd-safe" }, "Copies of different versions serve side by side; finish the upgrade (or go back) before the next change."),
        downLine(), kerr, versionStrip(map, { total })),
      h("div", { class: "ro-hd-acts" },
        reg?.missing ? action({ label: "How to upgrade…", sub: "one copy at a time, by hand", cls: "primary", onclick: () => setTab("upgrades") })
          : action({ label: "Pick a version…", sub: "the published images, and how to run one", cls: "primary", onclick: () => setTab("upgrades") }),
        action({ label: "Open the cluster", sub: "hand a primary over, see each copy", href: "#/cluster" }))));
  }
  function drawOwnNow() {
    const map = versionMap();
    const rows = map.map((x, i) => h("div", { class: "ro-run" },
      h("div", { class: "row between" }, h("span", { class: "row nw" }, h("i", { class: "ro-sw o" + Math.min(i, 3) }), vsaid(x.v)), h("span", { class: "small dim" }, plural(x.count, "copy", "copies"))),
      h("div", { class: "ro-run-s" }, x.copies.map((c) => { const cc = copies().find((y) => y.short === c); return h("span", { class: "ro-srv" + (cc?.role === "primary" ? " p" : ""), title: cc ? `${cc.url} · ${cc.role}${cc.up_s ? " · up " + span(cc.up_s) : ""}` : c }, c); }))));
    const gone = copies().filter((c) => c.down);
    if (gone.length) rows.push(h("div", { class: "ro-run" },
      h("div", { class: "row between" }, h("span", { class: "row nw" }, h("i", { class: "ro-sw down" }), "not answering"), h("span", { class: "small dim" }, plural(gone.length, "copy", "copies"))),
      h("div", { class: "ro-run-s" }, gone.map((c) => h("span", { class: "ro-srv down", title: `${c.url}${c.was ? " · was " + c.was : ""}${c.since ? " · down since " + clock(c.since) : ""}` }, c.short)))));
    clear(body, h("div", { class: "ro-main" },
      h("div", { class: "ro-col" }, card({ title: "What each copy runs", sub: "by version; a primary is outlined", body: rows.length ? h("div", { class: "stack", style: { gap: "8px" } }, rows) : h("div", { class: "small dim" }, "No copies yet.") })),
      h("div", { class: "ro-col" }, howCard())));
  }
  function howCard(v) {
    const image = v?.image || (reg?.repository ? `${reg.repository}:latest` : "ghcr.io/chloetech-ai/strata-server:latest");
    const cmds = [`docker pull ${image}`, "# then, for each replica, one at a time: stop it, start it with the new image, wait until Cluster shows it back and caught up", "# then hand each primary over (Cluster, the copy, Make primary) and restart it the same way", "# the coordinators last"];
    return card({
      title: "Upgrade machines of your own", sub: "by hand, one copy at a time",
      body: h("div", { class: "stack" },
        h("ol", { class: "ro-ol" },
          h("li", {}, "Get the new version: pull its image, or build the commit (", h("code", {}, "cargo build --release --bin strata-server"), ")."),
          h("li", {}, "Restart each replica with it, one at a time, and wait until it is back and caught up."),
          h("li", {}, "Hand each primary over to a restarted copy, then restart it the same way."),
          h("li", {}, "Restart the coordinators last; they talk to copies of both versions meanwhile.")),
        h("div", { class: "ro-cmds" }, cmds.map((c) => h("div", { class: "row nw" }, h("code", { class: "mono small grow" }, c), c.startsWith("#") ? null : h("button", { class: "btn xs ghost", title: "Copy", onclick: () => copy(c) }, icon("copy"))))),
        h("div", { class: "small dim" }, "On Kubernetes, Strata's operator does this for you: tried on every node first, held by gates, verified, rolled back when a check fails.")),
    });
  }

  // ---------------------------------------------------------- Versions (the picker)
  const MODES = [
    ["off", "Off", "The operator never asks the registry. Upgrade by picking a version here."],
    ["suggest", "Suggest", "Every few minutes the operator asks the registry what the tag it follows names, and offers a newer image here. Nothing restarts until someone upgrades."],
    ["on", "Automatic", "A newer image the tag names rolls out on its own once the cluster is ready, held by the gates; one that fails is rolled back and not tried again."],
  ];
  function drawUpgrades() {
    const left = h("div", { class: "ro-col" }, pickerCard());
    const right = h("div", { class: "ro-col" });
    if (managed()) right.append(modeCard());
    else right.append(howCard());
    const rej = st().upgrade?.rejected || [];
    if (rej.length) right.append(card({ title: "Rolled back, not rolled out again", sub: "the upgrader skips them; you may still pick one", body: h("div", { class: "stack", style: { gap: "6px" } }, rej.slice().reverse().map((x) => h("div", { class: "small" }, vsaid(vOf(x.digest), { built: false }), h("span", { class: "dim" }, ` · ${clock(x.atMs)} · `), said(x.why)))) }));
    const log = (st().upgrade?.log || []).slice(-6).reverse();
    if (log.length) right.append(card({ title: "What the upgrader did", body: h("div", { class: "ro-tl" }, log.map((e) => h("div", { class: "ro-tl-e" }, h("span", { class: "ro-tl-t" }, clock(e.atMs)), h("span", {}, said(e.text))))) }));
    clear(body, h("div", { class: "ro-main wide" }, left, right));
  }

  function pickerCard() {
    const m = managed() ? mode() : null;
    const readOnly = !managed();
    if (!reg) return card({ title: "Pick a version", body: h("div", { class: "small dim" }, "Asking the registry for its images…") });
    if (reg.missing) return oldPicker();
    if (reg.error && !(reg.images || []).length) return card({ title: "Pick a version", body: h("div", { class: "stack" }, h("div", { class: "error" }, "The registry: " + reg.error), h("button", { class: "btn sm", style: { alignSelf: "flex-start" }, onclick: () => loadVersions(true) }, icon("refresh"), "Ask again")) });
    // One row an image (its tags together), newest first.
    const rows = new Map();
    for (const x of reg.images || []) {
      const k = x.digest || x.image;
      const o = rows.get(k) || { ...x, tags: [] };
      for (const t of [...(x.tags || []), x.tag]) if (t && !o.tags.includes(t)) o.tags.push(t);
      for (const f of ["current", "newer", "tracked"]) o[f] = o[f] || x[f];
      if (x.tag && !HEX40.test(x.tag) && x.tag !== "latest") o.image = x.image;
      rows.set(k, o);
    }
    const map = versionMap();
    const runs = new Map(map.map((x) => [x.key, x.count]));
    const rejected = new Set((st().upgrade?.rejected || []).map((x) => x.digest));
    let list = [...rows.values()].sort((a, b) => (sec(b.created) || 0) - (sec(a.created) || 0));
    const q = filter.q.trim().toLowerCase();
    if (q) list = list.filter((x) => [...x.tags, x.revision, x.digest].some((t) => (t || "").toLowerCase().includes(q)));
    if (filter.newer) list = list.filter((x) => x.newer || x.current);
    const newer = [...rows.values()].filter((x) => x.newer).length;
    const curRow = [...rows.values()].find((x) => x.current);
    const find = h("input", { class: "input sm ro-find", placeholder: "Filter by tag or commit", value: filter.q, "aria-label": "Filter the versions" });
    find.oninput = () => { filter.q = find.value; const pos = find.selectionStart; drawUpgrades(); const n = body.querySelector(".ro-find"); n?.focus(); n?.setSelectionRange(pos, pos); };
    const only = h("label", { class: "row nw small ro-only" }, h("input", { type: "checkbox", checked: filter.newer, onchange: (e) => { filter.newer = e.target.checked; drawUpgrades(); } }), "only newer");
    // A server that reports no image digest (machines of your own) says its
    // version number: the image whose tag is that number runs it.
    const runOf = (x) => runs.get(x.digest) || (x.image && runs.get(x.image)) || x.tags.map((t) => runs.get(t) || runs.get(t.replace(/^v(?=\d)/, ""))).find(Boolean) || 0;
    const tr = (x) => {
      const v = { ...x, track: reg.track };
      const isRun = runOf(x);
      const badges = [
        x.current ? h("span", { class: "ro-chip cur", title: "The image the cluster's spec names" }, "current") : null,
        isRun && !x.current ? h("span", { class: "ro-chip run" }, `${fmt.n(isRun)} copies run it`) : null,
        x.newer ? h("span", { class: "ro-chip new" }, "newer") : null,
        x.tracked || x.tags.includes(reg.track) ? h("span", { class: "ro-chip trk", title: `The tag the upgrader follows: ${reg.track}` }, reg.track || "tracked") : null,
        rejected.has(x.digest) ? h("span", { class: "ro-chip bad", title: "The operator rolled it back once" }, "rolled back once") : null,
      ];
      const cl = commitLink(v);
      // Machines of your own: the command that fetches this very image.
      const pull = `docker pull ${x.digest && reg.repository ? `${reg.repository}@${x.digest}` : x.image}`;
      const pickBtn = readOnly ? h("button", { class: "btn sm", title: pull, onclick: () => copy(pull, "docker pull command copied") }, icon("copy"), "Pull command") : x.current ? h("span", { class: "small dim" }, "runs now") : h("button", {
        class: "btn sm" + (x.newer && x === list.find((y) => y.newer) ? " primary" : ""), disabled: busy || !can("admin"), title: can("admin") ? "Shows the plan first: nothing restarts yet" : NEEDS,
        // Pinned by digest, as the upgrader does: every copy runs the same bits even if a tag moves on.
        onclick: () => upgrade(x.digest && reg.repository ? `${reg.repository}@${x.digest}` : x.image),
      }, x.newer ? "Upgrade…" : "Go back to it…");
      return h("tr", { class: x.current || (readOnly && isRun) ? "cur" : "" },
        h("td", { class: "ro-pk-v" }, h("div", { class: "row nw" }, h("b", { class: "ro-vn mono" }, vname(v)), named(v) && commitOf(v) ? h("span", { class: "small dim mono" }, commitOf(v).slice(0, 7)) : null, cl ? h("a", { href: cl, target: "_blank", rel: "noopener", title: "The commit", class: "ro-ext" }, icon("ext")) : null)),
        h("td", { class: "nowrap ro-pk-d" }, x.created ? [at(sec(x.created)), h("span", { class: "small dim" }, " · " + ago(sec(x.created)))] : h("span", { class: "dim" }, "–")),
        h("td", { class: "ro-pk-b" }, h("div", { class: "row" }, badges)),
        h("td", { class: "small dim mono ro-pk-g", title: x.digest }, (x.digest || "").replace("sha256:", "").slice(0, 12)),
        h("td", { class: "num ro-pk-a" }, pickBtn));
    };
    const cmp = curRow && list.find((x) => x.newer) ? compareLink({ ...curRow }, { ...list.find((x) => x.newer) }) : null;
    return card({
      title: "Pick a version", sub: reg.repository,
      tools: [h("button", { class: "btn sm ghost ro-tool", "aria-label": "Ask the registry again", title: "Ask the registry again", onclick: () => { reg = null; draw(); loadVersions(true); } }, icon("refresh"))],
      body: h("div", { class: "stack", style: { gap: "10px" } },
        h("div", { class: "row" }, find, only),
        h("div", { class: "small" }, curRow ? ["The cluster runs ", h("b", {}, vname({ ...curRow, track: reg.track })), curRow.created ? `, built ${clock(sec(curRow.created))} ${TZ}` : "", ". ", newer ? h("b", {}, `${plural(newer, "newer build")}.`) : "Nothing newer is published.", cmp ? [" ", h("a", { href: cmp, target: "_blank", rel: "noopener" }, "What changed since")] : null]
          : map.length ? ["Your servers run ", map.map((x, i) => [i ? ", " : "", h("b", {}, vname(x.v)), ` (${plural(x.count, "copy", "copies")})`]), ". ",
            list.some(runOf) ? "Its row is marked." : "They report no image digest and no image here has that tag, so no row is marked: start them with STRATA_IMAGE set to the image they run.", " Published images, newest first."]
          : "Published images, newest first."),
        readOnly ? h("div", { class: "notice" }, "This server is not run by the Kubernetes operator, so nothing here changes the cluster. Copy an image's pull command, then restart your machines with it one copy at a time.") : null,
        moving(m) ? h("div", { class: "ro-warnline" }, icon("warn"), h("span", {}, "A change is under way: picking a version replaces it, from where it is.")) : null,
        reg.error ? h("div", { class: "small dim" }, "Some answers are old: " + reg.error) : null,
        h("div", { class: "tbl-wrap", style: { maxHeight: "560px" } }, h("table", { class: "t compact ro-pk" },
          h("thead", {}, h("tr", {}, h("th", {}, "version"), h("th", {}, `built (${TZ})`), h("th", {}, ""), h("th", {}, "digest"), h("th", {}, ""))),
          h("tbody", {}, list.length ? list.map(tr) : h("tr", {}, h("td", { colspan: 5 }, empty("search", "No version matches", q ? `Nothing has "${q}" in its tag or commit.` : "")))))),
        readOnly ? null : h("div", { class: "small dim" }, "Picking one shows the plan first: the restarts, the time it takes and how to go back. Nothing restarts until you confirm.")),
    });
  }
  /** An older server: no list of versions; name an image, or take the upgrader's newest. */
  function oldPicker() {
    const u = st().upgrade || null, image = spec().image || "";
    const input = h("input", { class: "input sm mono", placeholder: image.replace(/[@:][^/]*$/, "") + ":TAG", "aria-label": "An image to upgrade to", style: { flex: "1 1 240px", minWidth: 0 } });
    const go = () => { const v = input.value.trim(); if (v) upgrade(v.includes(":") || v.includes("@") || v.includes("/") ? v : image.replace(/[@:][^/]*$/, "") + ":" + v); };
    input.addEventListener("keydown", (e) => { if (e.key === "Enter") go(); });
    return card({
      title: "Pick a version", sub: "this server does not list the registry's images",
      body: h("div", { class: "stack" },
        u?.latest ? h("div", { class: "row" }, h("span", {}, "The newest on ", h("b", {}, tagOf(u.track) || "its tag"), ": ", vsaid(vOf(u.latest.image))), u.available && managed() ? h("button", { class: "btn sm primary", disabled: busy || !can("admin"), onclick: () => upgrade(null) }, "Upgrade…") : h("span", { class: "small dim" }, "the cluster runs it")) : null,
        managed() ? h("div", { class: "row" }, input, h("button", { class: "btn sm", disabled: busy || !can("admin"), onclick: go }, "Plan an upgrade")) : null,
        h("div", { class: "small dim" }, "A newer server lists every published image here, with its commit and build date.")),
    });
  }
  function modeCard() {
    const want = spec().upgrade || {}, mode0 = want.mode || "off", u = st().upgrade || {};
    const sw = seg(MODES.map(([v, l]) => [v, l]), mode0, (v) => setMode(v), { "aria-label": "Upgrades" });
    if (busy || !can("admin")) for (const b of sw.children) { b.disabled = true; b.title = can("admin") ? "" : NEEDS; }
    return card({
      title: "New versions", sub: `the upgrader follows ${tagOf(u.track || want.track) || "the image's tag"}`,
      body: h("div", { class: "stack", style: { gap: "10px" } },
        sw,
        h("div", { class: "ro-modes" }, MODES.map(([v, l, t]) => h("div", { class: "ro-mode" + (v === mode0 ? " on" : "") }, h("b", {}, l), h("span", { class: "small dim" }, t)))),
        h("div", { class: "small dim" }, [
          u.checkedMs ? `Asked the registry ${ago(u.checkedMs)}` : mode0 === "off" ? "The registry is not asked while it is off" : "Not asked yet",
          want.checkMinutes ? `, every ${want.checkMinutes} min` : "",
          want.window ? `; restarts only in ${want.window} ${TZ}` : "",
          ".",
        ]),
        u.error ? h("div", { class: "small", style: { color: "var(--crit-ink)" } }, "The registry: " + u.error) : null),
    });
  }

  // ---------------------------------------------------------- History
  function drawHistory() {
    const hs = [...history()].reverse(), m = mode();
    const cur = hs[0];
    const rows = hs.map((v, i) => {
      const prev = hs[i + 1];
      const tag = v === cur ? (m === "done" || m === "rolled back" ? "running" : m === "version failed" ? "failed" : "rolling out") : v.failed ? "failed" : v.version === cur.restores ? "restored" : null;
      const image = prev && prev.spec?.image !== v.spec?.image;
      const what = v.restores ? `Rolled back to spec v${v.restores}` : summary(prev?.spec, v.spec, (i2) => vname(vOf(i2)));
      const meta = [v.by ? (["operator", "upgrader", "autoscaler"].includes(v.by) ? `the ${v.by} (automatic)` : v.by) : "the operator", `${clock(v.atMs)} ${TZ}`, v.readyMs ? "took " + span((v.readyMs - v.atMs) / 1000) : v === cur || v.failed ? null : "never finished"].filter(Boolean).join(" · ");
      const isOpen = openVersion === v.version;
      const cost = v !== cur ? rollbackCost(v) : null;
      return h("div", { class: "ro-ver" + (v === cur ? " cur" : "") + (isOpen ? " open" : "") },
        h("button", { class: "ro-ver-h", "aria-expanded": String(isOpen), onclick: () => { openVersion = isOpen ? null : v.version; drawHistory(); } },
          h("b", { class: "ro-ver-n", title: "The operator's count of changes to the spec; not a Strata version" }, `spec v${v.version}`),
          h("div", { class: "ro-ver-t" }, h("div", {}, what), image ? h("div", { class: "small" }, vsaid(vOf(v.spec.image))) : null, h("div", { class: "small dim" }, meta), v.failed ? h("div", { class: "small ro-failed" }, `failed (${v.failed.step}): ${said(v.failed.text)}`) : null),
          tag ? h("span", { class: "ro-tag " + (tag === "running" ? "done" : tag === "failed" ? "bad" : "now") }, tag) : null),
        isOpen ? h("div", { class: "ro-ver-b stack" }, diffBlock(prev?.spec, v.spec),
          v !== cur ? h("button", { class: "btn sm danger", disabled: busy || !can("admin"), title: can("admin") ? "" : NEEDS, style: { alignSelf: "flex-start" }, onclick: () => rollback(v) }, icon("undo"), `Roll back to spec v${v.version}${cost?.n != null ? ` (${plural(cost.n, "restart")}${cost.secs ? ", about " + span(cost.secs) : ""})` : ""}…`) : null) : null);
    });
    const log = (st().log || []).slice().reverse();
    clear(body, h("div", { class: "ro-main" },
      h("div", { class: "ro-col" }, card({ title: "Versions of the spec", sub: `each change to the StrataCluster, by whom; the last ${hs.length}`, body: hs.length ? h("div", { class: "ro-vers" }, rows) : h("div", { class: "small dim" }, "The operator keeps each version of the spec here once it has seen one.") })),
      h("div", { class: "ro-col" }, card({ title: "What the operator said", sub: `its last ${log.length} messages, local time (${TZ})`, body: log.length ? h("div", { class: "ro-tl", style: { maxHeight: "620px" } }, log.map((e) => h("div", { class: "ro-tl-e " + (e.phase === "Paused" ? "warn" : e.phase === "Ready" ? "ok" : e.phase === "Failed" ? "bad" : "") }, h("span", { class: "ro-tl-t", title: new Date(e.atMs).toString() }, clock(e.atMs, true)), h("span", {}, said(e.message))))) : h("div", { class: "small dim" }, "Nothing yet.") }))));
  }

  // ---------------------------------------------------------- Gates
  const rolloutSpec = () => ({ paused: false, gates: {}, holdAfterSeconds: 30, soakSeconds: 0, ...(spec().rollout || {}), gates: { ...(spec().rollout?.gates || {}) } });
  const gatesOf = (x) => ({ g: Object.fromEntries(GATES.map((g) => [g.key, x?.gates?.[g.key] ?? null])), a: x?.holdAfterSeconds, s: x?.soakSeconds });
  function follow() { if (!draft || same(gatesOf(draft), gatesOf(draft.from))) draft = { ...rolloutSpec(), from: rolloutSpec() }; }
  function seriesOf(g) {
    const t0 = Date.now() - 10 * 60e3;
    if (g.key === "p99Ms" || g.key === "errorRatio") {
      const k = g.key === "p99Ms" ? "p99_ms" : "error_ratio";
      const pts = (state.points || []).filter((p) => p.t >= t0 && typeof p[k] === "number").map((p) => [p.t, p[k]]);
      if (pts.length > 1) return pts;
    }
    return (series[g.key] || []).filter(([t]) => t >= t0);
  }
  async function saveGates(reason) {
    const d = draft, r0 = rolloutSpec(), gates = {};
    for (const g of GATES) if (!same(d.gates[g.key], r0.gates[g.key])) gates[g.key] = d.gates[g.key] ?? null;
    const p = {};
    if (Object.keys(gates).length) p.gates = gates;
    if (d.holdAfterSeconds !== r0.holdAfterSeconds) p.holdAfterSeconds = d.holdAfterSeconds;
    if (d.soakSeconds !== r0.soakSeconds) p.soakSeconds = d.soakSeconds;
    if (!Object.keys(p).length) return;
    const lines = [
      ...Object.entries(gates).map(([k, v]) => [`spec.rollout.gates.${k}`, gateOf(k).show(r0.gates[k]) || "off", v == null ? "off" : gateOf(k).show(v)]),
      ...(p.holdAfterSeconds != null ? [["spec.rollout.holdAfterSeconds", `${r0.holdAfterSeconds} s`, `${p.holdAfterSeconds} s`]] : []),
      ...(p.soakSeconds != null ? [["spec.rollout.soakSeconds", `${r0.soakSeconds} s`, `${p.soakSeconds} s`]] : []),
    ];
    const ok = await ask({ title: "Change the gates?", ok: "Change them", reason: "optional", body: h("div", { class: "stack" }, lines.map(([k, a, b]) => h("div", { class: "ro-patch" }, h("code", {}, k), h("span", { class: "ro-patch-old" }, a), "→", h("b", { class: "ro-patch-new" }, b))), h("p", {}, "No server restarts. The gates read the live numbers before each step of a change.")) });
    if (ok) patch({ rollout: p }, "The operator has the new gates", ok.reason);
  }
  function drawGates() {
    follow();
    const n = numbers(state.cluster), d = draft, r0 = ro();
    const edited = !same(gatesOf(d), gatesOf(rolloutSpec()));
    const setDraft = (f) => { draft = { ...draft, gates: { ...draft.gates } }; f(draft); drawGates(); };
    const off = !managed() || !can("admin");
    const rows = GATES.map((g) => {
      const lim = d.gates[g.key], v = n[g.key];
      const clockOf = (r0.gates || []).find((x) => x.name === g.key && x.limit === lim);
      const over = lim != null && v != null && v > lim;
      const holdsIn = clockOf?.overSinceMs ? Math.max(0, d.holdAfterSeconds - (Date.now() - clockOf.overSinceMs) / 1000) : null;
      const status0 = !managed() ? null : lim == null ? h("span", { class: "dim" }, "off") : over ? h("span", { class: "ro-over" }, holdsIn != null && !r0.held ? `over · holds in ${Math.ceil(holdsIn)} s` : "over its limit") : h("span", { class: "ro-under" }, "under");
      return h("div", { class: "ro-gate" + (over ? " over" : "") + (lim == null ? " off" : "") },
        h("div", { class: "row between" }, h("div", {}, h("b", {}, g.label), h("div", { class: "small dim" }, g.what)), h("div", { style: { textAlign: "right" } }, h("b", { class: "ro-val" }, g.show(v)), h("div", { class: "small" }, status0))),
        gateChart(g, seriesOf(g), lim, off ? null : (x) => setDraft((dr) => { dr.gates[g.key] = x; })),
        managed() ? h("div", { class: "row between" },
          lim == null
            ? h("button", { class: "btn sm", disabled: off, onclick: () => setDraft((dr) => { dr.gates[g.key] = snap(g, g.suggest(v)); }) }, icon("plus"), `Add a limit at ${g.show(snap(g, g.suggest(v)))}`)
            : h("div", { class: "row nw" },
              h("button", { class: "btn sm", disabled: off, "aria-label": "Lower the limit", onclick: () => setDraft((dr) => { dr.gates[g.key] = Math.max(0, snap(g, lim - stepOf(g, lim))); }) }, "−"),
              h("b", {}, "limit " + g.show(lim)),
              h("button", { class: "btn sm", disabled: off, "aria-label": "Raise the limit", onclick: () => setDraft((dr) => { dr.gates[g.key] = Math.min(g.max, snap(g, lim + stepOf(g, lim))); }) }, "+")),
          lim != null ? h("button", { class: "btn sm ghost", disabled: off, onclick: () => setDraft((dr) => { dr.gates[g.key] = null; }) }, "Remove") : null) : null);
    });
    const num = (label, key, hint) => h("div", { class: "ro-num" }, h("div", {}, h("b", {}, label), h("div", { class: "small dim" }, hint)),
      h("div", { class: "row nw" },
        h("button", { class: "btn sm", disabled: off, "aria-label": "Less", onclick: () => setDraft((dr) => { dr[key] = Math.max(0, dr[key] - (dr[key] > 60 ? 30 : dr[key] > 10 ? 5 : 1)); }) }, "−"),
        h("b", {}, `${d[key]} s`),
        h("button", { class: "btn sm", disabled: off, "aria-label": "More", onclick: () => setDraft((dr) => { dr[key] = dr[key] + (dr[key] >= 60 ? 30 : dr[key] >= 10 ? 5 : 1); }) }, "+")));
    const foot = managed() ? h("div", { class: "row", style: { justifyContent: "flex-end" } },
      edited ? h("span", { class: "small dim", style: { marginRight: "auto" } }, "Gates change no server: saving them restarts nothing.") : null,
      h("button", { class: "btn sm", disabled: !edited || busy, onclick: () => { draft = { ...rolloutSpec(), from: rolloutSpec() }; drawGates(); } }, "Discard"),
      h("button", { class: "btn sm primary", disabled: !edited || busy || off, title: off ? NEEDS : "", onclick: () => saveGates() }, "Save the gates…")) : null;
    clear(body, h("div", { class: "ro-main" },
      h("div", { class: "ro-col" }, card({
        title: "Health gates", sub: managed() ? `a gate over its limit for ${d.holdAfterSeconds} s holds the change, before the next copy restarts` : "the live numbers a gate would read; gates are the operator's",
        body: h("div", { class: "ro-gates" }, rows), foot,
      })),
      h("div", { class: "ro-col" }, managed() ? card({ title: "How a change goes", body: h("div", { class: "stack" }, num("Hold after", "holdAfterSeconds", "how long a gate is over its limit before it holds"), num("Soak", "soakSeconds", "how long a restarted copy serves before the next restarts"),
        h("div", { class: "small dim" }, `Verify: ${spec().rollout?.verifySeconds ?? 60} s healthy at the end. On failure: ${(spec().rollout?.onFailure || "rollback") === "rollback" ? "roll back on its own" : "hold for a person"}. A step may take ${spec().rollout?.progressDeadlineSeconds ?? 600} s before the version fails.`)) }) : howCard())));
  }
  function gateChart(g, pts, lim, onLimit) {
    const W = 300, H = 56;
    const vals = pts.map((p) => p[1]);
    const top = Math.max(lim != null ? lim * 1.4 : 0, ...vals.map((v) => v * 1.15), g.max === 1 ? 0.01 : 1);
    const y = (v) => H - 4 - (clamp(v, 0, top) / top) * (H - 8);
    const svg = s("svg", { viewBox: `0 0 ${W} ${H}`, class: "ro-chart", preserveAspectRatio: "none", role: "img", "aria-label": `${g.label}, the last ten minutes` });
    if (pts.length > 1) {
      const t0 = pts[0][0], t1 = pts[pts.length - 1][0], x = (t) => ((t - t0) / Math.max(1, t1 - t0)) * W;
      const d = pts.map((p, i) => `${i ? "L" : "M"}${x(p[0]).toFixed(1)},${y(p[1]).toFixed(1)}`).join("");
      svg.append(s("path", { d: `${d}L${W},${H}L0,${H}Z`, fill: "var(--accent-soft)", stroke: "none" }), s("path", { d, fill: "none", stroke: "var(--accent)", "stroke-width": 1.5, "vector-effect": "non-scaling-stroke" }));
      if (lim != null) {
        const clip = `ro-clip-${g.key}`;
        svg.append(s("clipPath", { id: clip }, s("rect", { x: 0, y: 0, width: W, height: y(lim) })), s("path", { d, fill: "none", stroke: "var(--crit)", "stroke-width": 2, "clip-path": `url(#${clip})`, "vector-effect": "non-scaling-stroke" }));
      }
    } else svg.append(s("text", { x: W / 2, y: H / 2 + 4, "text-anchor": "middle", class: "ro-none" }, "no numbers yet"));
    if (lim != null) {
      const line = s("line", { x1: 0, x2: W, y1: y(lim), y2: y(lim), stroke: "var(--crit)", "stroke-dasharray": "5 4", "stroke-width": 1.5, "vector-effect": "non-scaling-stroke" });
      svg.append(line);
      if (onLimit) {
        const grip = s("rect", { x: 0, y: y(lim) - 7, width: W, height: 14, fill: "transparent", class: "ro-grip", tabindex: 0, role: "slider", "aria-label": `${g.label} limit`, "aria-valuenow": lim });
        const atY = (ev) => { const b = svg.getBoundingClientRect(); return snap(g, ((H - 4 - clamp(((ev.clientY - b.top) / b.height) * H, 0, H)) / (H - 8)) * top); };
        grip.addEventListener("pointerdown", (ev) => {
          ev.preventDefault(); grip.setPointerCapture(ev.pointerId); grip.dataset.drag = "1";
          const move = (e2) => { const v = atY(e2); line.setAttribute("y1", y(v)); line.setAttribute("y2", y(v)); grip.setAttribute("y", y(v) - 7); svg.dataset.limit = v; };
          const upf = () => { grip.removeEventListener("pointermove", move); grip.removeEventListener("pointerup", upf); delete grip.dataset.drag; if (svg.dataset.limit) onLimit(Number(svg.dataset.limit)); };
          grip.addEventListener("pointermove", move); grip.addEventListener("pointerup", upf);
        });
        grip.addEventListener("keydown", (e) => { if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return; e.preventDefault(); onLimit(clamp(snap(g, lim + (e.key === "ArrowUp" ? 1 : -1) * stepOf(g, lim)), 0, g.max)); });
        svg.append(grip);
      }
    }
    return svg;
  }

  // ---------------------------------------------------------- polling
  // The operator's status every 3 s; nothing redraws under a limit being
  // dragged, a version filter being typed, or a dialog.
  const holding = () => !!page.querySelector(".ro-grip[data-drag], .ro-grip:focus, .ro-find:focus") || !!document.querySelector(".modal");
  load();
  const offCluster = bus.on("cluster", () => {
    sample();
    if (!kube || holding()) return;
    drawStatus();
    if (tab === "now") drawNow();
  });
  // A rollout is a change the person waits on: it keeps updating while the page is paused.
  const stop = every(3000, async () => {
    const was = kube;
    try { kube = await api("/v1/kube"); kubeErr = null; } catch (e) { kubeErr = e; return; }
    if (same(was?.status, kube.status) && same(was?.spec, kube.spec)) return;
    follow();
    if (holding()) return;
    drawHead(); drawStatus();
    if (tab === "now") drawNow(); else if (tab === "history") drawHistory(); else if (tab === "upgrades") drawUpgrades();
  }, false, { always: true });
  const stopReg = every(10 * 60e3, () => loadVersions(), false);
  return () => { stop(); stopReg(); offCluster(); hideTip(); onTab = null; };
}
