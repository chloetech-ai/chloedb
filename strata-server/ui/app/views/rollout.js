// Rollout: the change the operator is making, step by step, and what may
// stop it. Its steps, measured on the cluster as they go; a timeline of
// what the operator said, with the time it was held or paused hatched; the
// health gates (a limit on a live number: one over its limit for a while
// holds the change), dragged on their own charts; pause and resume; and
// every version of the spec, by whom, with its diff and a rollback that
// shows its plan first. The Strata version card says what the servers
// run and what the registry has, and upgrades the cluster to it (by
// hand, or the upgrader on its own); a version that fails is rolled back
// and said so. A cluster of machines of your own has no operator to roll
// out with: it shows the same live numbers.

import { h, s, icon, clear, fmt, card, api, toast, confirm, bus, every, pill, empty, clamp, seg } from "../core.js";
import { state, isCoordinator } from "../data.js";
import { yaml, lineDiff } from "./control.js";

export default {
  id: "rollout", title: "Rollout", icon: "rollout", group: "Admin",
  keywords: "rollout roll out upgrade upgrader new version image tag digest registry ghcr automatic versions history rollback roll back undo pause resume hold held gate gates health canary soak verify verification preflight progress steps timeline restart operator kubernetes",
  render,
};

const same = (a, b) => JSON.stringify(a ?? null) === JSON.stringify(b ?? null);
const plural = (n, w, ws) => fmt.plural(n, w, ws);
const short = (url) => (url || "").replace(/^https?:\/\//, "");
/** An image as people read it: a digest cut to its first 12 digits. */
const shortImage = (i) => (i || "").replace(/@sha256:([0-9a-f]{12})[0-9a-f]+/, "@$1…");

/** Two digits that mean something: 64%, 2.5%, 0.01% (as the operator says it). */
function pct(v) {
  if (v == null || Number.isNaN(v)) return "–";
  const p = v * 100;
  const d = p >= 10 || p <= 0 ? 0 : p >= 1 ? 1 : Math.min(6, 1 - Math.floor(Math.log10(p)));
  return p.toFixed(d).replace(/(\.\d*?)0+$/, "$1").replace(/\.$/, "") + "%";
}

/** The gates (spec.rollout.gates), each read from the cluster's live numbers as the operator reads them. */
/** Milliseconds as a limit reads: 25 ms, 2.5 ms, 1.2 s. */
const ms = (v) => (v == null ? "–" : v >= 1000 ? fmt.ms(v) : `${Number.isInteger(v) || v >= 100 ? Math.round(v) : v.toFixed(1)} ms`);
const GATES = [
  { key: "p99Ms", label: "Search p99", what: "the coordinator's, as its clients see it", show: ms, suggest: (v) => Math.max(50, Math.ceil(((v || 20) * 3) / 10) * 10), max: 1e6 },
  { key: "errorRatio", label: "Failed searches", what: "the share of searches that fail", show: pct, suggest: () => 0.01, max: 1 },
  { key: "replicaBehind", label: "Replica lag", what: "transactions a replica is behind its primary", show: (v) => (v == null ? "–" : plural(Math.round(v), "transaction")), suggest: () => 1000, max: 1e9 },
  { key: "diskUsedRatio", label: "Fullest volume", what: "the used share of the fullest volume", show: pct, suggest: (v) => Math.min(0.99, Math.max(0.9, Math.ceil(((v || 0) + 0.02) * 100) / 100)), max: 1 },
];
const gateOf = (k) => GATES.find((g) => g.key === k);

/** The cluster's live numbers, as the operator reads them from /v1/cluster (operator::Numbers). */
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

/** A round step near a twentieth of `v`, for the − and + of a limit. */
function stepOf(g, v) {
  if (g.max === 1) return v < 0.02 ? 0.001 : v < 0.2 ? 0.005 : 0.01;
  const e = Math.pow(10, Math.floor(Math.log10(Math.max(1, v / 10))));
  return [1, 2, 5, 10].map((m) => m * e).find((x) => x >= v / 20) || e;
}
const snap = (g, v) => { const st = stepOf(g, v); return clamp(Number((Math.round(v / st) * st).toPrecision(6)), 0, g.max); };

/** What changed between two versions' specs, in words (the version's summary). */
function summary(a, b) {
  if (!a) return "The first version the operator saw";
  const out = [];
  const was = (k) => (a[k] == null ? "none" : JSON.stringify(a[k]));
  if (a.shards !== b.shards) out.push(`${b.shards} shards (was ${a.shards})`);
  if (a.replicas !== b.replicas) out.push(`${b.replicas} copies each (was ${a.replicas})`);
  if (a.coordinators !== b.coordinators) out.push(`${b.coordinators} coordinators (was ${a.coordinators})`);
  if (a.image !== b.image) out.push(`image ${shortImage(b.image)}`);
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

function render(page) {
  const head = h("div"), banner = h("div"), main = h("div", { class: "ro-main" });
  const left = h("div", { class: "ro-col" }), right = h("div", { class: "ro-col" });
  main.append(left, right);
  page.append(head, banner, main);
  if (!isCoordinator() && state.health) {
    clear(main, card({ body: h("div", { class: "stack" }, h("div", {}, "This server is not a cluster: rollouts change the servers of a coordinator's cluster."), h("a", { class: "btn sm", href: "#/cluster", style: { alignSelf: "flex-start" } }, "Make a cluster")) }));
    return;
  }

  let kube = null;            // GET /v1/kube
  let draft = null;           // the gates being edited: {gates, holdAfterSeconds, soakSeconds}
  let open = null;            // the version whose diff is open
  let busy = false;
  let typed = "";             // the image typed in the version card
  const series = {};          // gate → [[t, v]] seen on this page, beyond the coordinator's own series
  const managed = () => !!kube?.managed;
  const st = () => kube?.status || {};
  const ro = () => st().rollout || {};
  const history = () => st().history || [];
  const spec = () => kube?.spec || {};
  const rollout = () => ({ paused: false, gates: {}, holdAfterSeconds: 30, soakSeconds: 0, ...(spec().rollout || {}), gates: { ...(spec().rollout?.gates || {}) } });
  const gatesOf = (x) => ({ g: Object.fromEntries(GATES.map((g) => [g.key, x?.gates?.[g.key] ?? null])), a: x?.holdAfterSeconds, s: x?.soakSeconds });
  /** The gates being edited differ from those they were taken from. */
  const dirty = () => !!draft && !same(gatesOf(draft), gatesOf(draft.from));
  /** The draft follows the spec while it is not being edited. */
  const follow = () => { if (!draft || !dirty()) draft = { ...rollout(), from: rollout() }; };

  // ---------------------------------------------------------- loading
  async function load() {
    try { kube = await api("/v1/kube"); } catch (e) { kube = { managed: false, error: e }; }
    follow();
    draw();
  }
  function sample() {
    const n = numbers(state.cluster), t = Date.now();
    for (const g of GATES) {
      const a = (series[g.key] ||= []);
      if (n[g.key] != null) a.push([t, n[g.key]]);
      if (a.length > 600) a.splice(0, a.length - 600);
    }
  }
  /** A gate's last ten minutes: the coordinator's own series for p99 and failures, what this page saw for the rest. */
  function seriesOf(g) {
    const t0 = Date.now() - 10 * 60e3;
    if (g.key === "p99Ms" || g.key === "errorRatio") {
      const k = g.key === "p99Ms" ? "p99_ms" : "error_ratio";
      const pts = state.points.filter((p) => p.t >= t0 && typeof p[k] === "number").map((p) => [p.t, p[k]]);
      if (pts.length > 1) return pts;
    }
    return (series[g.key] || []).filter(([t]) => t >= t0);
  }

  // ---------------------------------------------------------- what is happening
  /** running | held | paused | done | rolling back | failed | degraded */
  function mode() {
    const s0 = st(), r = ro();
    if (s0.phase === "Failed") return "failed";
    if (r.failed && !r.ungated) return "version failed";
    if (r.held) return "held";
    // Paused as the spec says, before the operator has seen it too.
    if (s0.phase === "Paused" || r.paused || spec().rollout?.paused) return "paused";
    const cur = history()[history().length - 1];
    if (s0.phase === "Ready" && kube?.settled) return cur?.restores ? "rolled back" : "done";
    if (s0.phase === "Degraded") return "degraded";
    return cur?.restores && r.ungated ? "rolling back" : "running";
  }
  const TONE = { running: "info", held: "warn", paused: "disabled", done: "ok", "rolled back": "ok", "rolling back": "serious", failed: "bad", "version failed": "bad", degraded: "degraded" };
  const SAYS = { running: "rolling out", held: "held by a gate", paused: "paused", done: "done", "rolled back": "rolled back", "rolling back": "rolling back", failed: "failed", "version failed": "the version failed", degraded: "done, some servers not answering" };
  /** The version a rollback left because it failed, if the current one is such a rollback. */
  const leftFailed = () => { const hs = history(), cur = hs[hs.length - 1], prev = hs[hs.length - 2]; return cur?.restores && prev?.failed ? prev : null; };

  /**
   * The steps of the current version, from what changed since the one
   * before, in the operator's order, each with its progress on the live
   * cluster. A step the cluster does not show (the coordinators, the
   * volumes) is done once the operator is past it.
   */
  function steps() {
    const hs = history(), cur = hs[hs.length - 1], prev = hs[hs.length - 2];
    if (!cur) return [];
    const a = prev?.spec, b = cur.spec, c = state.cluster, msg = st().message || "", r = ro();
    const ready = st().phase === "Ready" && kube?.settled;
    const shards = c?.shards || [];
    const out = [];
    // title, what it does, the operator's words for it, its progress (null: not shown) and whether it is done.
    const stage = (title, sub, words, frac, done) => out.push({ title, sub, now: words.test(msg), frac, done: ready || done });
    if (!a) {
      stage("Run the cluster", `${plural(b.shards, "shard")} of ${plural(b.replicas, "copy", "copies")}`, /./, shards.length / Math.max(1, b.shards), ready);
      return finish(out);
    }
    const restarts = coordinatorConfig(a) !== coordinatorConfig(b) || serverConfig(a) !== serverConfig(b);
    if (a.image !== b.image && !cur.restores) {
      stage("Try the image on every node", `${shortImage(b.image).replace(/^.*\//, "")}: a pod of it on each node with copies, before any server restarts`, /trying the image|does not run here/, null, r.preflight === b.image);
    }
    if (a.coordinators !== b.coordinators || coordinatorConfig(a) !== coordinatorConfig(b)) {
      stage(a.coordinators !== b.coordinators ? `${plural(b.coordinators, "coordinator")}, from ${a.coordinators}` : "Restart the coordinators with the new settings", "one at a time behind the Service, a new one ready before an old one stops", /restarting the coordinators|updating the coordinators|coordinators, from/, null, false);
    }
    if (a.replicas !== b.replicas) {
      const per = shards.map((x) => (x.replicas || []).length);
      const ok = per.filter((n) => n === b.replicas).length;
      stage(`${plural(b.replicas, "copy", "copies")} a shard, from ${a.replicas}`, b.replicas > a.replicas ? "each new copy joins its shard and is sent a copy of the primary" : "the highest copy of each shard leaves it, a primary handed over first", /copies, from|joins shard|leaves shard|before .* goes/, per.length ? ok / per.length : 0, per.length > 0 && ok === per.length);
    }
    if (a.shards !== b.shards) {
      const n = shards.length, k = Math.abs(b.shards - a.shards);
      stage(b.shards > a.shards ? `Start ${plural(k, "new shard")}` : `Drain and remove ${plural(k, "shard")}`, b.shards > a.shards ? `${plural(k * b.replicas, "pod")}: ${k} of ${plural(b.replicas, "copy", "copies")}` : "each one's slots move to the others first", /new shard|joins the cluster|draining|leaves the cluster|slots off|copies of a shard that left/, 1 - Math.abs(b.shards - n) / k, n === b.shards);
      const held = shards.map((x) => x.slots_held || 0), lo = Math.min(...held), hi = Math.max(...held);
      const want = 4096 / Math.max(1, n), left = n ? held.reduce((t, v) => t + Math.abs(v - want), 0) / 2 : 0;
      const all = (4096 * k) / Math.max(a.shards, b.shards);
      stage("Even the slots out", n === b.shards ? `${fmt.n(lo)} to ${fmt.n(hi)} slots a shard; about ${fmt.n(Math.round(left))} still to move` : "once every shard has joined", /slots/, n === b.shards ? 1 - left / Math.max(1, all) : 0, n === b.shards && hi - lo <= 1 && !/slots/.test(msg));
    }
    if (a.storage?.size !== b.storage?.size) stage(`Grow the volumes to ${b.storage?.size}`, "each copy's claim, in place (the storage class must allow it)", /growing|volume/, null, false);
    if (serverConfig(a) !== serverConfig(b)) {
      const of = r.copies || shards.reduce((t, x) => t + (x.replicas || []).length, 0), left = (r.stale || []).length;
      const extra = b.replicas < 3 && spec().rollout?.extraCopies !== false ? `${b.replicas === 2 ? "a third copy with them joins" : "two extra copies with them join"} each shard first, so that most of its copies serve while each restarts; then ` : "";
      stage(`New settings on ${plural(of, "server")}`, `${extra}replicas first, then each primary after handing it over; one at a time${spec().rollout?.soakSeconds ? `, each serving ${spec().rollout.soakSeconds} s before the next` : ""}`, /new settings|restarts with|rolling out|leaves shard/, of ? (of - left) / of : 0, left === 0 && !/new settings|leaves shard/.test(msg));
    }
    if (restarts) {
      const v = r.verify, need = (spec().rollout?.verifySeconds ?? 60) * 1000;
      const frac = v?.passedMs ? 1 : v?.healthySinceMs ? clamp((Date.now() - v.healthySinceMs) / Math.max(1, need), 0, 0.99) : v ? 0.02 : null;
      stage("Verify the version", v?.failing ? `failing: ${v.failing}` : `healthy for ${need / 1000} s in a row: every copy back and in sync, the gates under their limits, a query through the coordinators`, /verifying the version/, frac, !!v?.passedMs);
    }
    if (!out.length) stage("Nothing to do on the servers", summary(a, b), /$^/, 1, true);
    return finish(out);
  }
  /** The steps before the one the operator is on are done; the progress of one the cluster does not show is half once it runs. */
  function finish(out) {
    const at = out.findIndex((x) => x.now && !x.done);
    out.forEach((x, i) => {
      if (at >= 0 && i < at && x.frac == null) x.done = true;
      if (x.done) { x.frac = 1; x.now = false; }
      if (x.frac == null) x.frac = x.now ? 0.5 : 0;
    });
    return out;
  }

  // ---------------------------------------------------------- acting
  async function change(rolloutPatch, what) {
    busy = true; draw();
    try {
      kube = await api("/v1/admin/kube", { body: { spec: { rollout: rolloutPatch } } });
      toast(what, "ok");
      draft = { ...rollout(), from: rollout() };
    } catch (e) { toast("Not changed: " + e.message, "bad", 8000); }
    busy = false; draw();
  }
  async function specChange(patch, what) {
    busy = true; draw();
    try { kube = await api("/v1/admin/kube", { body: { spec: patch } }); toast(what, "ok"); }
    catch (e) { toast("Not changed: " + e.message, "bad", 8000); }
    busy = false; draw();
  }
  /** Upgrades to `image` (the newest the upgrader knows when null), its plan confirmed first. */
  async function upgrade(image, force = false) {
    let plan;
    try { plan = await api("/v1/admin/kube/upgrade", { body: { image, plan: true, force } }); }
    catch (e) { toast("No upgrade: " + e.message, "bad", 9000); return; }
    if (plan.refused) { toast("No upgrade: " + plan.refused, "bad", 9000); return; }
    const to = image || plan.image?.image || "the newest image";
    const r0 = spec().rollout || {};
    const ok = await confirm({
      title: `Upgrade ${kube.namespace}/${kube.name}`,
      body: h("div", { class: "stack", style: { gap: "8px" } },
        h("div", {}, h("span", { class: "dim" }, "From "), h("code", { class: "ro-img" }, plan.from), h("span", { class: "dim" }, " to "), h("code", { class: "ro-img" }, to), plan.image?.revision ? h("span", { class: "dim" }, ` (commit ${plan.image.revision.slice(0, 7)})`) : null),
        h("div", {}, "Queries keep being answered all along: the operator then"),
        h("ol", { class: "ctl-steps" }, (plan.steps || []).map((x) => h("li", {}, x))),
        h("div", { class: "small dim" }, `Then it verifies the version for ${r0.verifySeconds ?? 60} s. ${(r0.onFailure || "rollback") === "rollback" ? "If a step fails (a server that does not start, a check that fails), it rolls back to the version before on its own." : "If a step fails, it holds the change for you to resume or roll back."}`),
        (plan.warnings || []).length ? h("div", { class: "small", style: { color: "var(--serious)" } }, "Mind: " + plan.warnings.join("; ") + ".") : null),
      ok: "Upgrade",
    });
    if (!ok) return;
    busy = true; draw();
    try { kube = await api("/v1/admin/kube/upgrade", { body: { image, force } }); toast("Upgrading: the operator rolls it out a server at a time", "ok"); }
    catch (e) { toast("Not upgraded: " + e.message, "bad", 9000); }
    busy = false; draw();
  }
  const pause = () => change({ paused: true }, "Paused: the operator takes no step until you resume");
  const unpause = () => change({ paused: false }, "Resumed: the operator goes on from where it paused");
  async function resume() {
    busy = true; draw();
    try { kube = await api("/v1/admin/kube/resume", { method: "POST", body: {} }); toast("Resumed: the gates count again from now", "ok"); }
    catch (e) { toast("Not resumed: " + e.message, "bad", 8000); }
    busy = false; draw();
  }
  async function saveGates() {
    const d = draft, r = rollout();
    const gates = {};
    for (const g of GATES) if (!same(d.gates[g.key], r.gates[g.key])) gates[g.key] = d.gates[g.key] ?? null;
    const patch = {};
    if (Object.keys(gates).length) patch.gates = gates;
    if (d.holdAfterSeconds !== r.holdAfterSeconds) patch.holdAfterSeconds = d.holdAfterSeconds;
    if (d.soakSeconds !== r.soakSeconds) patch.soakSeconds = d.soakSeconds;
    if (!Object.keys(patch).length) return;
    await change(patch, "The operator has the new gates");
  }
  async function rollback(v) {
    let plan;
    try { plan = await api("/v1/admin/kube/plan", { body: { rollback: v.version } }); }
    catch (e) { toast("No rollback: " + e.message, "bad", 8000); return; }
    if (plan.refused) { toast("No rollback: " + plan.refused, "bad", 8000); return; }
    const ok = await confirm({
      title: `Roll ${kube.namespace}/${kube.name} back to v${v.version}`,
      body: h("div", { class: "stack", style: { gap: "8px" } },
        h("div", {}, `The spec goes back to version ${v.version} (${fmt.datetime(v.atMs)}${v.by ? ", by " + v.by : ""}); its gates stay as they are and hold no step of the rollback. The operator then:`),
        h("ol", { class: "ctl-steps" }, (plan.steps || []).map((x) => h("li", {}, x))),
        (plan.changes || []).length ? h("div", { class: "small dim" }, "Fields: " + plan.changes.map((c) => c.field).join(", ")) : null,
        (plan.warnings || []).length ? h("div", { class: "small", style: { color: "var(--serious)" } }, "Mind: " + plan.warnings.join("; ") + ".") : null),
      ok: `Roll back to v${v.version}`, danger: true,
    });
    if (!ok) return;
    busy = true; draw();
    try { kube = await api("/v1/admin/kube", { body: { rollback: v.version } }); toast(`Rolling back to v${v.version}`, "ok"); }
    catch (e) { toast("Not rolled back: " + e.message, "bad", 8000); }
    busy = false; draw();
  }

  // ---------------------------------------------------------- drawing
  function draw() {
    if (!kube) { clear(head, h("div", { class: "dim small" }, "Loading the cluster…")); return; }
    drawHead(); drawBanner();
    if (!managed()) { drawOwn(); return; }
    if (!main.contains(left)) clear(main, left, right);
    clear(left, drawSteps(), drawTimeline());
    drawRight();
  }
  function drawRight() {
    if (!managed()) { drawOwn(); return; }
    clear(right, drawUpgrade(), drawGates(), drawVersions());
  }

  function drawHead() {
    const hs = history(), cur = hs[hs.length - 1];
    // The version to go back to: the one before, unless it failed.
    const prev = hs.slice(0, -1).reverse().find((v) => !v.failed && v.hash !== cur?.hash);
    const m = managed() ? mode() : null;
    const tools = [];
    if (managed()) {
      if (m === "held") tools.push(h("button", { class: "btn sm primary", disabled: busy, onclick: resume }, icon("play"), "Resume"));
      else if (ro().paused || spec().rollout?.paused) tools.push(h("button", { class: "btn sm primary", disabled: busy, onclick: unpause }, icon("play"), "Resume"));
      else tools.push(h("button", { class: "btn sm", disabled: busy || m === "done" || m === "rolled back", title: m === "done" ? "Nothing is rolling out" : "The operator takes no step until you resume", onclick: pause }, icon("pause"), "Pause"));
      if (prev) tools.push(h("button", { class: "btn sm", disabled: busy, onclick: () => rollback(prev) }, icon("undo"), `Roll back to v${prev.version}`));
    }
    clear(head, h("div", { class: "page-head" },
      h("div", {},
        h("h1", {}, "Rollout"),
        h("p", {}, managed()
          ? [h("b", {}, `${kube.namespace}/${kube.name}`), cur ? ` · version ${cur.version}` : "", cur ? " · " + (cur.restores ? `back to v${cur.restores}` : summary(hs[hs.length - 2]?.spec, cur.spec)) : ""]
          : "The change the operator makes, step by step, and what may stop it.")),
      h("div", { class: "tools" }, m ? pill(TONE[m], SAYS[m]) : null, ...tools)));
  }

  function drawBanner() {
    if (!managed()) { clear(banner); return; }
    const r = ro(), m = mode();
    const next = (st().message || "").split("; next: ")[1];
    if (m === "held" && r.held) {
      const g = gateOf(r.held.gate), now = numbers(state.cluster)[r.held.gate];
      const raise = g ? snap(g, Math.max(r.held.value * 1.25, (now ?? 0) * 1.25, r.held.limit + stepOf(g, r.held.limit))) : null;
      clear(banner, h("div", { class: "ro-banner held" },
        icon("warn"),
        h("div", { class: "ro-banner-t" },
          h("b", {}, `Held: ${r.held.text}.`),
          h("div", { class: "small" }, `Nothing done so far is undone${next ? "; next, " + next : ""}. ${g ? `It reads ${g.show(now)} now. ` : ""}Resume when the gate is back under its limit, raise its limit, or roll back.`)),
        h("div", { class: "row", style: { gap: "6px", flexWrap: "wrap" } },
          h("button", { class: "btn sm primary", disabled: busy, onclick: resume }, "Resume as it is"),
          g ? h("button", { class: "btn sm", disabled: busy, title: "A changed limit lifts the hold", onclick: () => change({ gates: { [g.key]: raise } }, `${g.label} may now reach ${g.show(raise)}`) }, `Raise the limit to ${g.show(raise)}`) : null)));
      return;
    }
    if (m === "paused") {
      clear(banner, h("div", { class: "ro-banner paused" }, icon("pause"),
        h("div", { class: "ro-banner-t" }, h("b", {}, "Paused."), h("div", { class: "small" }, `The operator takes no step toward the spec until you resume${next ? "; next, " + next : ""}. A copy that comes back without its data still joins its shard again.`)),
        h("button", { class: "btn sm primary", disabled: busy, onclick: unpause }, icon("play"), "Resume")));
      return;
    }
    if (m === "version failed" && r.failed) {
      const hs = history(), prev = hs[hs.length - 2];
      clear(banner, h("div", { class: "ro-banner failed" }, icon("bad"),
        h("div", { class: "ro-banner-t" },
          h("b", {}, `The version failed (${r.failed.step}): ${r.failed.text}.`),
          h("div", { class: "small" }, `No other server restarts with it; the servers it did not reach keep the version before. The rollout holds it, as onFailure: hold asks. Try it again once the cause is fixed, or roll back${prev ? " to v" + prev.version : ""}.`)),
        h("div", { class: "row", style: { gap: "6px", flexWrap: "wrap" } },
          h("button", { class: "btn sm", disabled: busy, onclick: resume }, "Try it again"),
          prev ? h("button", { class: "btn sm primary", disabled: busy, onclick: () => rollback(prev) }, icon("undo"), `Roll back to v${prev.version}`) : null)));
      return;
    }
    const bad = leftFailed();
    if (bad && (m === "rolling back" || m === "rolled back")) {
      const cur = history()[history().length - 1];
      clear(banner, h("div", { class: "ro-banner back" + (m === "rolled back" ? " over" : "") }, icon(m === "rolled back" ? "okc" : "undo"),
        h("div", { class: "ro-banner-t" },
          h("b", {}, `v${bad.version} failed, and ${cur.by === "operator" ? "the operator" : cur.by || "someone"} ${m === "rolled back" ? "rolled it back" : "is rolling it back"} to v${cur.restores}.`),
          h("div", { class: "small" }, `It failed ${bad.failed.step === "verify" ? "its verification" : "at step " + bad.failed.step}: ${bad.failed.text}.${m === "rolled back" ? " The cluster runs v" + cur.restores + " again, verified." : " No gate holds a rollback."}`))));
      return;
    }
    if (m === "rolling back") {
      clear(banner, h("div", { class: "ro-banner back" }, icon("undo"), h("div", { class: "ro-banner-t" }, h("b", {}, `Rolling back to v${history()[history().length - 1]?.restores}.`), h("div", { class: "small" }, "No gate holds a rollback, so that it goes through while the version it leaves is unwell."))));
      return;
    }
    clear(banner);
  }

  function drawSteps() {
    const list = steps();
    const m = mode();
    const body = h("div", { class: "ro-steps" }, list.map((x, i) => {
      const cls = x.done ? "done" : x.now ? (m === "held" || m === "paused" ? "held" : "now") : "wait";
      const sayIt = { done: "done", now: m === "rolling back" ? "rolling back" : "running", held: m === "held" ? "held" : "paused", wait: "next" }[cls];
      return h("div", { class: "ro-step " + cls },
        h("div", { class: "ro-step-n" }, x.done ? icon("check") : String(i + 1)),
        h("div", { class: "ro-step-t" },
          h("div", { class: "row", style: { justifyContent: "space-between", gap: "8px" } }, h("b", {}, x.title), h("span", { class: "ro-tag " + cls }, sayIt)),
          h("div", { class: "small dim" }, x.sub),
          h("div", { class: "ro-bar" }, h("i", { style: { width: `${Math.round(clamp(x.frac || 0, 0, 1) * 100)}%` } }))));
    }));
    const msg = st().message;
    return card({ title: "Steps", sub: "the operator takes them one at a time, the gates read before each", body: h("div", { class: "stack" }, body, msg ? h("div", { class: "ro-now small" }, icon("info"), h("span", {}, "The operator: " + msg)) : null) });
  }

  function drawTimeline() {
    const hs = history(), cur = hs[hs.length - 1];
    const log = (st().log || []).filter((e) => !cur || e.atMs >= cur.atMs - 1000);
    const t0 = cur?.atMs || log[0]?.atMs || Date.now(), t1 = cur?.readyMs && mode() !== "running" ? Math.max(cur.readyMs, t0 + 1000) : Date.now();
    const W = 600, H = 46, x = (t) => ((t - t0) / Math.max(1, t1 - t0)) * W;
    const svg = s("svg", { viewBox: `0 0 ${W} ${H}`, class: "ro-axis", preserveAspectRatio: "none", role: "img", "aria-label": "The rollout's time, held and paused spans hatched" });
    svg.append(s("defs", {}, s("pattern", { id: "ro-hatch", width: 6, height: 6, patternUnits: "userSpaceOnUse", patternTransform: "rotate(45)" }, s("rect", { width: 3, height: 6, fill: "var(--warn)", opacity: 0.55 }))));
    svg.append(s("rect", { x: 0, y: 14, width: W, height: 14, rx: 3, fill: "var(--surface-3)" }));
    // Held and paused spans: from an entry in phase Paused to the next that is not.
    log.forEach((e, i) => {
      if (e.phase !== "Paused") return;
      if (i > 0 && log[i - 1].phase === "Paused") return;
      const end = log.slice(i + 1).find((f) => f.phase !== "Paused")?.atMs ?? t1;
      svg.append(s("rect", { x: x(e.atMs), y: 14, width: Math.max(2, x(end) - x(e.atMs)), height: 14, fill: "url(#ro-hatch)" }, s("title", {}, `${e.message.startsWith("held") ? "Held" : "Paused"} ${fmt.time(e.atMs)} to ${fmt.time(end)}`)));
    });
    svg.append(s("rect", { x: 0, y: 14, width: x(Math.min(Date.now(), t1)), height: 14, rx: 3, fill: "var(--accent)", opacity: 0.18 }));
    for (const e of log) svg.append(s("line", { x1: x(e.atMs), x2: x(e.atMs), y1: 10, y2: 32, stroke: e.phase === "Paused" ? "var(--warn)" : e.phase === "Ready" ? "var(--good)" : "var(--accent)", "stroke-width": 1.5 }, s("title", {}, `${fmt.time(e.atMs)} ${e.message}`)));
    const axis = h("div", { class: "ro-axis-l small dim" }, h("span", {}, fmt.time(t0)), h("span", {}, fmt.time(t0 + (t1 - t0) / 2)), h("span", {}, fmt.time(t1)));
    const said = [...log].reverse().slice(0, 30).map((e) => h("div", { class: "ro-ev " + (e.phase === "Paused" ? "warn" : e.phase === "Ready" ? "ok" : e.phase === "Failed" ? "bad" : "") }, h("span", { class: "ro-ev-t" }, fmt.time(e.atMs)), h("span", {}, e.message)));
    const took = cur?.readyMs ? ` · took ${fmt.dur((cur.readyMs - cur.atMs) / 1000)}` : "";
    return card({ title: "Timeline", sub: `hatched: held or paused${took}`, body: h("div", { class: "stack" }, svg, axis, h("div", { class: "ctl-gtitle" }, "What happened"), said.length ? h("div", { class: "ro-evs" }, said) : h("div", { class: "small dim" }, "Nothing yet: the operator writes each step it takes here.")) });
  }

  function drawGates() {
    follow();
    const n = numbers(state.cluster), d = draft, r = ro();
    const edited = !same(gatesOf(d), gatesOf(rollout()));
    const setDraft = (f) => { draft = { ...draft, gates: { ...draft.gates } }; f(draft); drawRight(); };
    const rows = GATES.map((g) => {
      const lim = d.gates[g.key];
      const v = n[g.key];
      const clock = (r.gates || []).find((x) => x.name === g.key && x.limit === lim);
      const over = lim != null && v != null && v > lim;
      const holdsIn = clock?.overSinceMs ? Math.max(0, d.holdAfterSeconds - (Date.now() - clock.overSinceMs) / 1000) : null;
      const status = !managed() ? null : lim == null ? h("span", { class: "dim" }, "off") : over ? h("span", { class: "ro-over" }, holdsIn != null && !r.held ? `over · holds in ${Math.ceil(holdsIn)} s` : "over its limit") : h("span", { class: "ro-under" }, "under");
      return h("div", { class: "ro-gate" + (over ? " over" : "") + (lim == null ? " off" : "") },
        h("div", { class: "row", style: { justifyContent: "space-between", gap: "8px" } },
          h("div", {}, h("b", {}, g.label), h("div", { class: "small dim" }, g.what)),
          h("div", { style: { textAlign: "right" } }, h("b", { class: "ro-val" }, g.show(v)), h("div", { class: "small" }, status))),
        chart(g, seriesOf(g), lim, (x) => setDraft((dr) => { dr.gates[g.key] = x; })),
        managed() ? h("div", { class: "row", style: { gap: "6px", justifyContent: "space-between" } },
          lim == null
            ? h("button", { class: "btn sm", onclick: () => setDraft((dr) => { dr.gates[g.key] = snap(g, g.suggest(v)); }) }, icon("plus"), `Add a limit at ${g.show(snap(g, g.suggest(v)))}`)
            : h("div", { class: "row ctl-step", style: { gap: "4px" } },
              h("button", { class: "btn sm ctl-sq", "aria-label": "Lower the limit", onclick: () => setDraft((dr) => { dr.gates[g.key] = Math.max(0, snap(g, lim - stepOf(g, lim))); }) }, "−"),
              h("b", {}, "limit " + g.show(lim)),
              h("button", { class: "btn sm ctl-sq", "aria-label": "Raise the limit", onclick: () => setDraft((dr) => { dr.gates[g.key] = Math.min(g.max, snap(g, lim + stepOf(g, lim))); }) }, "+")),
          lim != null ? h("button", { class: "btn sm ghost", onclick: () => setDraft((dr) => { dr.gates[g.key] = null; }) }, "Remove") : null) : null);
    });
    const num = (label, key, unit, hint) => h("div", { class: "ro-num" },
      h("div", {}, h("b", {}, label), h("div", { class: "small dim" }, hint)),
      h("div", { class: "row ctl-step", style: { gap: "4px" } },
        h("button", { class: "btn sm ctl-sq", "aria-label": "Less", onclick: () => setDraft((dr) => { dr[key] = Math.max(0, dr[key] - (dr[key] > 60 ? 30 : dr[key] > 10 ? 5 : 1)); }) }, "−"),
        h("b", {}, `${d[key]} ${unit}`),
        h("button", { class: "btn sm ctl-sq", "aria-label": "More", onclick: () => setDraft((dr) => { dr[key] = dr[key] + (dr[key] >= 60 ? 30 : dr[key] >= 10 ? 5 : 1); }) }, "+")));
    const foot = managed()
      ? h("div", { class: "row", style: { gap: "8px", justifyContent: "flex-end" } },
        edited ? h("span", { class: "small dim", style: { marginRight: "auto" } }, "Gates change no server: saving them restarts nothing.") : null,
        h("button", { class: "btn sm", disabled: !edited || busy, onclick: () => { draft = { ...rollout(), from: rollout() }; drawRight(); } }, "Discard"),
        h("button", { class: "btn sm primary", disabled: !edited || busy, onclick: saveGates }, "Save gates"))
      : null;
    return card({
      title: "Health gates", sub: managed() ? `a gate over its limit for ${d.holdAfterSeconds} s holds the change` : "the live numbers a gate would read",
      body: h("div", { class: "stack", style: { gap: "10px" } }, rows, managed() ? num("Hold after", "holdAfterSeconds", "s", "how long a gate is over before it holds") : null, managed() ? num("Soak", "soakSeconds", "s", "how long a restarted copy serves before the next restarts") : null),
      foot,
    });
  }

  /** A gate's last minutes, its limit a dashed line dragged up and down. */
  function chart(g, pts, lim, onLimit) {
    const W = 300, H = 56;
    const vals = pts.map((p) => p[1]);
    const top = Math.max(lim != null ? lim * 1.4 : 0, ...vals.map((v) => v * 1.15), g.max === 1 ? 0.01 : 1);
    const y = (v) => H - 4 - (clamp(v, 0, top) / top) * (H - 8);
    const svg = s("svg", { viewBox: `0 0 ${W} ${H}`, class: "ro-chart", preserveAspectRatio: "none", role: "img", "aria-label": `${g.label}, the last minutes` });
    if (pts.length > 1) {
      const t0 = pts[0][0], t1 = pts[pts.length - 1][0], x = (t) => ((t - t0) / Math.max(1, t1 - t0)) * W;
      const d = pts.map((p, i) => `${i ? "L" : "M"}${x(p[0]).toFixed(1)},${y(p[1]).toFixed(1)}`).join("");
      svg.append(s("path", { d: `${d}L${W},${H}L0,${H}Z`, fill: "var(--accent-soft)", stroke: "none" }), s("path", { d, fill: "none", stroke: "var(--accent)", "stroke-width": 1.5, "vector-effect": "non-scaling-stroke" }));
      if (lim != null) {
        const clip = `ro-clip-${g.key}`;
        svg.append(s("clipPath", { id: clip }, s("rect", { x: 0, y: 0, width: W, height: y(lim) })), s("path", { d, fill: "none", stroke: "var(--crit)", "stroke-width": 2, "clip-path": `url(#${clip})`, "vector-effect": "non-scaling-stroke" }));
      }
    } else {
      svg.append(s("text", { x: W / 2, y: H / 2 + 4, "text-anchor": "middle", class: "ro-none" }, "no numbers yet"));
    }
    if (lim != null) {
      const line = s("line", { x1: 0, x2: W, y1: y(lim), y2: y(lim), stroke: "var(--crit)", "stroke-dasharray": "5 4", "stroke-width": 1.5, "vector-effect": "non-scaling-stroke" });
      const grip = s("rect", { x: 0, y: y(lim) - 7, width: W, height: 14, fill: "transparent", class: "ro-grip", tabindex: 0, role: "slider", "aria-label": `${g.label} limit`, "aria-valuenow": lim });
      const at = (ev) => { const b = svg.getBoundingClientRect(); return snap(g, ((H - 4 - clamp(((ev.clientY - b.top) / b.height) * H, 0, H)) / (H - 8)) * top); };
      grip.addEventListener("pointerdown", (ev) => {
        ev.preventDefault();
        grip.setPointerCapture(ev.pointerId);
        grip.dataset.drag = "1";
        const move = (e2) => { const v = at(e2); line.setAttribute("y1", y(v)); line.setAttribute("y2", y(v)); grip.setAttribute("y", y(v) - 7); svg.dataset.limit = v; };
        const upf = () => { grip.removeEventListener("pointermove", move); grip.removeEventListener("pointerup", upf); delete grip.dataset.drag; if (svg.dataset.limit) onLimit(Number(svg.dataset.limit)); };
        grip.addEventListener("pointermove", move);
        grip.addEventListener("pointerup", upf);
      });
      grip.addEventListener("keydown", (e) => {
        if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
        e.preventDefault();
        onLimit(clamp(snap(g, lim + (e.key === "ArrowUp" ? 1 : -1) * stepOf(g, lim)), 0, g.max));
      });
      svg.append(line, grip);
    }
    return svg;
  }

  function drawVersions() {
    const hs = [...history()].reverse();
    if (!hs.length) return card({ title: "Versions", body: h("div", { class: "small dim" }, managed() ? "The operator keeps each version of the spec here once it has seen one." : "Versions are kept by the operator.") });
    const cur = hs[0];
    const rows = hs.map((v, i) => {
      const prev = hs[i + 1];
      const tag = v === cur ? (mode() === "done" || mode() === "rolled back" ? "running" : mode() === "version failed" ? "failed" : "rolling out") : v.failed ? "failed" : v.version === cur.restores ? "restored" : null;
      const meta = [v.by || "the operator", fmt.datetime(v.atMs), v.readyMs ? "took " + fmt.dur((v.readyMs - v.atMs) / 1000) : v === cur || v.failed ? null : "never finished", v.restores ? `a rollback to v${v.restores}` : null].filter(Boolean).join(" · ");
      const isOpen = open === v.version;
      return h("div", { class: "ro-ver" + (v === cur ? " cur" : "") + (isOpen ? " open" : "") },
        h("button", { class: "ro-ver-h", "aria-expanded": String(isOpen), onclick: () => { open = isOpen ? null : v.version; drawRight(); } },
          h("b", { class: "ro-ver-n" }, `v${v.version}`),
          h("div", { class: "ro-ver-t" }, h("div", {}, v.restores ? `Rolled back to v${v.restores}` : summary(prev?.spec, v.spec)), h("div", { class: "small dim" }, meta), v.failed ? h("div", { class: "small ro-failed" }, `failed (${v.failed.step}): ${v.failed.text}`) : null),
          tag ? h("span", { class: "ro-tag " + (tag === "running" ? "done" : tag === "failed" ? "bad" : "now") }, tag) : null),
        isOpen ? h("div", { class: "ro-ver-b stack" },
          h("pre", { class: "ctl-diff" }, (prev ? lineDiff(yaml(prev.spec), yaml(v.spec)) : yaml(v.spec).map((l) => [" ", l])).map(([k, l]) => h("div", { class: "ctl-dl " + (k === "+" ? "add" : k === "-" ? "del" : "") }, `${k} ${l}`))),
          v !== cur && managed() ? h("button", { class: "btn sm", disabled: busy, style: { alignSelf: "flex-start" }, onclick: () => rollback(v) }, icon("undo"), `Roll back to v${v.version}`) : null) : null);
    });
    return card({ title: "Versions", sub: `every change to the spec, by whom; the last ${hs.length}`, body: h("div", { class: "ro-vers" }, rows) });
  }

  /**
   * The Strata version: the image the spec names and what the servers
   * pulled for it, the newest image of the tag the upgrader follows
   * (status.upgrade), the upgrader's mode, and an upgrade, its plan first.
   */
  function drawUpgrade() {
    const u = st().upgrade || null, want = spec().upgrade || {}, mode0 = want.mode || "off";
    const image = spec().image || "";
    const short12 = (d) => (d || "").replace(/^sha256:/, "").slice(0, 12);
    const said = (i) => i ? `${short12(i.digest)}${i.revision ? " · commit " + i.revision.slice(0, 7) : ""}${i.created ? " · built " + fmt.datetime(Date.parse(i.created)) : ""}` : "";
    const running = (u?.running || []).slice().sort((x, y) => y.servers - x.servers);
    const latest = u?.latest || null;
    const rejected = (d) => (u?.rejected || []).find((r) => r.digest === d);
    const rolling = ["running", "rolling back", "held", "paused", "version failed"].includes(mode());
    const runs = h("div", { class: "ro-up-row" },
      h("div", { class: "small dim" }, "The spec names"),
      h("code", { class: "ro-img", title: image }, image),
      running.length
        ? h("div", { class: "ro-up-run" }, running.map((r) => h("span", { class: "ro-srv" + (latest && r.digest === latest.digest ? " p" : ""), title: r.digest }, `${plural(r.servers, "server")} on ${short12(r.digest)}`)))
        : h("div", { class: "small dim" }, u ? "No server has said which image it runs yet." : "Which image each server pulled shows once the upgrader is on."),
      u?.current?.revision ? h("div", { class: "small dim" }, `commit ${u.current.revision.slice(0, 7)}`) : null);
    const newest = u ? h("div", { class: "ro-up-row" },
      h("div", { class: "small dim" }, `Newest on ${u.track || "its tag"}${u.checkedMs ? ", asked " + fmt.ago(u.checkedMs) : ""}`),
      latest ? h("div", { class: "row", style: { gap: "8px", flexWrap: "wrap" } }, h("b", { class: "mono" }, said(latest)),
        u.available ? pill("info", "new") : rejected(latest.digest) ? pill("bad", "rolled back once") : pill("ok", "the cluster runs it")) : h("div", { class: "small dim" }, "Not asked yet."),
      u.error ? h("div", { class: "small", style: { color: "var(--crit-ink)" } }, "The registry: " + u.error) : null) : null;
    const modes = seg([
      ["off", "Off", "The operator does not ask the registry: upgrade by naming an image"],
      ["suggest", "Suggest", `Every ${want.checkMinutes || 10} minutes the operator asks the registry what the tag names, and shows a new image here`],
      ["on", "Automatic", "A new image is rolled out once the cluster is ready, and one that fails is rolled back and not tried again"],
    ], mode0, (v) => specChange({ upgrade: { mode: v } }, v === "on" ? "Upgrades are automatic: a new image rolls out once the cluster is ready" : v === "suggest" ? "The operator asks the registry and shows a new image here" : "The upgrader is off"));
    if (busy) for (const b of modes.children) b.disabled = true;
    const input = h("input", { class: "input sm mono ro-up-in", value: typed, placeholder: image.replace(/[@:][^/]*$/, "") + ":TAG", "aria-label": "An image to upgrade to", style: { flex: "1 1 180px", minWidth: "0" }, oninput: () => { typed = input.value; } });
    const go = () => { const v = input.value.trim(); if (v) upgrade(v.includes(":") || v.includes("@") || v.includes("/") ? v : image.replace(/[@:][^/]*$/, "") + ":" + v); };
    input.addEventListener("keydown", (e) => { if (e.key === "Enter") go(); });
    const now = latest && u?.available
      ? h("button", { class: "btn sm primary", disabled: busy || rolling, title: rolling ? "A change is under way: upgrade once it is done" : "", onclick: () => upgrade(null) }, icon("play"), `Upgrade to ${short12(latest.digest)}`)
      : null;
    const gone = (u?.rejected || []).slice(-3).reverse();
    const log = (u?.log || []).slice(-4).reverse();
    return card({
      title: "Strata version", sub: "what runs, and what is new",
      tools: pill(mode0 === "on" ? "ok" : mode0 === "suggest" ? "info" : "disabled", mode0 === "on" ? "automatic" : mode0 === "suggest" ? "suggests" : "by hand"),
      body: h("div", { class: "stack", style: { gap: "10px" } },
        runs, newest,
        h("div", { class: "ro-num ro-up-mode" }, h("div", {}, h("b", {}, "Upgrades"), h("div", { class: "small dim" }, "the upgrader follows the image's tag")), modes),
        h("div", { class: "row", style: { gap: "6px", flexWrap: "wrap" } }, now, input, h("button", { class: "btn sm", disabled: busy || rolling, onclick: go }, "Plan an upgrade")),
        gone.length ? h("div", { class: "stack", style: { gap: "4px" } }, h("div", { class: "ctl-gtitle" }, "Rolled back, not rolled out again"),
          gone.map((r) => h("div", { class: "small ro-failed" }, h("b", { class: "mono" }, short12(r.digest)), ` · ${fmt.datetime(r.atMs)} · ${r.why}`))) : null,
        log.length ? h("div", { class: "ro-evs" }, log.map((e) => h("div", { class: "ro-ev" }, h("span", { class: "ro-ev-t" }, fmt.time(e.atMs)), h("span", {}, e.text)))) : null,
        h("div", { class: "small dim" }, "Each server restarts in turn while the others answer: the image is tried on every node first, a primary is handed over before it restarts, and the version is verified at the end.")),
    });
  }

  /** What each server runs, by version: on machines of your own the rollout is done by hand, a copy at a time. */
  function drawRunning() {
    const by = new Map();
    for (const [i, x] of (state.cluster?.shards || []).entries()) {
      for (const r of x.replicas || []) {
        const v = r.detail?.system?.version || (r.status === "down" ? "not answering" : "unknown");
        if (!by.has(v)) by.set(v, []);
        by.get(v).push({ shard: i, url: r.url, role: r.takes_writes ? "primary" : r.role, up: r.detail?.system?.uptime_s });
      }
    }
    const rows = [...by.entries()].sort((a, b) => b[1].length - a[1].length).map(([v, rs]) => h("div", { class: "ro-run" },
      h("div", { class: "row", style: { justifyContent: "space-between", gap: "8px" } }, h("b", { class: "mono" }, v), h("span", { class: "small dim" }, plural(rs.length, "server"))),
      h("div", { class: "ro-run-s" }, rs.map((r) => h("span", { class: "ro-srv" + (r.role === "primary" ? " p" : ""), title: `shard ${r.shard} · ${r.role} · up ${fmt.dur(r.up)}` }, short(r.url))))));
    return card({ title: "What each server runs", sub: "by strata-server version; a primary is outlined", body: rows.length ? h("div", { class: "stack", style: { gap: "8px" } }, rows) : h("div", { class: "small dim" }, "No servers yet.") });
  }

  function drawOwn() {
    clear(main,
      h("div", { class: "ro-col" },
        card({ body: empty("rollout", "Rollouts are the operator's", "On machines of your own, a server takes new settings when you restart it with them: restart the replicas first, then hand each primary over (Control center) and restart it, one at a time, watching these numbers. On Kubernetes the operator does this for you, held by gates on them.", h("a", { class: "btn sm", href: "#/control" }, "Open the control center")) }),
        drawRunning()),
      h("div", { class: "ro-col" }, drawGates()));
  }

  // The live numbers move with each poll of the cluster; the operator's
  // status every 3 s. Nothing redraws under a limit being dragged, or an
  // image being typed.
  const held = () => !!page.querySelector(".ro-grip[data-drag], .ro-grip:focus, .ro-up-in:focus");
  load();
  const offCluster = bus.on("cluster", () => {
    sample();
    if (!kube) return;
    drawHead(); drawBanner();
    if (managed()) clear(left, drawSteps(), drawTimeline());
    if (!held()) drawRight();
  });
  const stop = every(3000, async () => {
    const was = kube;
    try { kube = await api("/v1/kube"); } catch { return; }
    if (same(was?.status, kube.status) && same(was?.spec, kube.spec)) return;
    follow();
    if (held()) { drawHead(); drawBanner(); clear(left, drawSteps(), drawTimeline()); } else draw();
  }, false);
  return () => { stop(); offCluster(); };
}
