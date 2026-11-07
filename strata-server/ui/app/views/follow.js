// Follow: the cluster this one follows (strata-server's crate::follow),
// how far behind it is, and the switch-over. What the follow view says
// (GET /v1/admin/follow): whether the cluster follows a source now, how
// far its follow process applied each source shard's feed, and what that
// process last said of itself; what holds this cluster's own writes
// (GET /v1/admin/hold-writes); and whether its operator runs the follow
// process (spec.follow in GET /v1/kube), which Pause and Resume change.
//
// Promote is the switch-over. Planned, the source holds its writes first,
// through the hub when the source is a connection in its book, and this
// cluster is promoted once it applied them up to the hold: no write is
// lost. When the source cannot be held, a promotion loses what the follow
// process had not applied, and the count is typed to confirm it.

import { h, icon, clear, fmt, api, toast, confirm, card, empty, failure, pill, every, can, needs, connectionName, modal, timeEl } from "../core.js";
import { startFollow, behindText } from "../follows.js";

export default {
  id: "follow", title: "Follow", icon: "share", group: "Change",
  keywords: "follow follower source replicate cluster copy feed lag behind switch over switchover failover promote hold writes pause resume stop disaster recovery standby",
  render,
};

const LATE_MS = 60_000;
const trim = (u) => String(u || "").replace(/\/+$/, "");
/** Whether following `c` ended in a promotion (its stop mark is written with it, a moment later). */
const promoted = (c) => !c.following && !!c.promoted_ms && c.promoted_ms + 5000 >= (c.stopped_ms || 0);

/** The state of a follow process, as a pill. */
function statePill(c) {
  const st = c.status;
  if (promoted(c)) return pill("disabled", "promoted");
  if (!c.following) return pill("disabled", "stopped");
  if (!st) return pill("warn", "no follow process");
  if (c.status_age_ms > 30_000) return pill("warn", `silent ${fmt.ago(Date.now() - c.status_age_ms).replace(" ago", "")}`);
  if (st.state === "following") return (st.lag_ms ?? 0) > LATE_MS ? pill("warn", "following, late") : pill("ok", "following");
  if (st.state === "held") return pill("bad", "held");
  if (st.state === "stopped") return pill("disabled", "stopped");
  return pill("info", st.state);
}

function render(page) {
  const top = h("div"), cur = h("div"), writes = h("div"), past = h("div");
  page.append(top, cur, writes, past);
  let alive = true, v = null, kube = null, hold = null, book = null, srcConn = null;

  async function load() {
    try { v = await api("/v1/admin/follow"); }
    catch (e) { clear(cur, failure(e, "The follow view")); return; }
    [kube, hold] = await Promise.all([api("/v1/kube").catch(() => null), api("/v1/admin/hold-writes").catch(() => null)]);
    if (connectionName && book === null) book = await api("/v1/admin/hub", { hub: true }).catch(() => ({ connections: [] }));
    if (!alive) return;
    draw();
  }

  /** The followed (or last followed) cluster: the one this page is about. */
  function main() {
    const cs = v?.clusters || [];
    return cs.find((c) => c.name === v.following) || [...cs].sort((a, b) => last(b) - last(a))[0] || null;
  }
  const last = (c) => Math.max(c.since_ms || 0, c.stopped_ms || 0, c.promoted_ms || 0);
  const spec = () => kube?.spec?.follow || null;

  /** The hub's connection that is `c`'s source, by its name or its URL. */
  function sourceOf(c) {
    const url = c?.status?.source;
    return (book?.connections || []).find((x) => x.name !== connectionName && (x.name === c?.name || (url && [...(x.urls || []), x.cluster?.public_url].some((u) => u && trim(u) === trim(url))))) || null;
  }

  function draw() {
    const c = main();
    srcConn = sourceOf(c);
    drawTop(c);
    drawCurrent(c);
    drawWrites();
    drawPast(c);
  }

  function drawTop(c) {
    if (!c) { clear(top); return; }
    const st = c.status || {};
    if (v.following) {
      const late = (st.lag_ms ?? 0) > LATE_MS || st.state === "held" || !c.status || c.status_age_ms > 30_000;
      clear(top, h("div", { class: "banner " + (late ? "warn" : "ok") }, icon("share", "lg"),
        h("div", { class: "grow" },
          h("div", { class: "big" }, `This cluster follows ${c.name}`),
          h("div", { class: "small dim" }, st.state === "copying" || st.state === "catching up" ? `${st.state}: ${fmt.n(st.rows_copied || 0)} rows copied so far` : c.status ? `${behindText(st.lag_ms)}, ${fmt.plural(st.behind || 0, "transaction")} to apply` : "No follow process said how it stands yet.",
            ". It refuses its own clients' writes and takes the source's.")),
        spec()?.paused ? pill("warn", "paused") : statePill(c)));
    } else if (promoted(c)) {
      clear(top, h("div", { class: "banner" }, icon("okc", "lg"), h("div", { class: "grow" }, h("div", { class: "big" }, `Promoted from ${c.name}`), h("div", { class: "small dim" }, "This cluster takes its own clients' writes since ", timeEl(c.promoted_ms), "."))));
    } else clear(top);
  }

  function drawCurrent(c) {
    if (!c) {
      clear(cur, card({ title: "Following", body: empty("share", "This cluster follows no other cluster", connectionName ? "Set it to follow another on All clusters, on the hub." : "Run strata-server follow, or set spec.follow where the operator runs it; on a hub, All clusters starts one.", connectionName ? h("a", { class: "btn sm", href: "/#/clusters" }, icon("cluster"), "All clusters") : null) }));
      return;
    }
    const st = c.status || {};
    const sp = spec()?.name === c.name ? spec() : null;
    const following = !!c.following;
    const tools = [];
    if (following && sp) tools.push(gated(h("button", { class: "btn sm", onclick: () => pause(c, !sp.paused) }, icon(sp.paused ? "play" : "pause"), sp.paused ? "Resume" : "Pause")));
    if (following) tools.push(gated(h("button", { class: "btn sm", onclick: () => stop(c) }, icon("stop"), "Stop following…")), gated(h("button", { class: "btn sm primary", onclick: () => promote(c) }, icon("bolt"), "Promote…")));
    const kv = h("dl", { class: "kv" },
      h("dt", {}, "source"), h("dd", { class: "mono" }, st.source || sp?.source || "?", srcConn ? h("a", { class: "btn sm ghost", style: { marginLeft: "8px" }, href: `/c/${srcConn.name}#/home` }, icon("ext"), srcConn.name) : null),
      h("dt", {}, following ? "following since" : promoted(c) ? "promoted" : "stopped"), h("dd", {}, timeEl(following ? c.since_ms : promoted(c) ? c.promoted_ms : c.stopped_ms)),
      c.copy_ms ? [h("dt", {}, "copy"), h("dd", {}, c.copied ? "made " : "begun ", timeEl(c.copy_ms), st.rows_copied ? ` · ${fmt.n(st.rows_copied)} rows` : "", st.keys_deleted ? ` · ${fmt.n(st.keys_deleted)} keys deleted` : "")] : null,
      following ? [h("dt", {}, "behind"), h("dd", {}, c.status ? `${behindText(st.lag_ms)} · ${fmt.plural(st.behind || 0, "transaction")}` : "unknown: no follow process said")] : null,
      st.written != null ? [h("dt", {}, "applied by this process"), h("dd", {}, `${fmt.n(st.written)} writes since `, timeEl(st.started_ms))] : null,
      [h("dt", {}, "source holds its writes"), h("dd", {}, c.source_holds ? pill("ok", "yes, as far as applied: a promotion loses nothing") : h("span", { class: "dim" }, "no"))],
      [h("dt", {}, "follow process"), h("dd", {}, sp ? `run by the operator (${kube?.name || "this cluster"}-follow)${sp.paused ? ", paused" : ""}` : "run outside the operator", c.status ? h("span", { class: "dim" }, ` · last said ${fmt.ago(Date.now() - c.status_age_ms)}`) : null)],
      st.error ? [h("dt", {}, "it says"), h("dd", { class: "error" }, st.error)] : null);
    const shards = (st.shards || []).length ? st.shards : (c.applied || []).map((a) => ({ shard: a.shard, applied: a.seq, term: a.term, source_holds: a.held }));
    const tbl = shards.length ? h("div", { class: "tbl-wrap" }, h("table", { class: "t" },
      h("thead", {}, h("tr", {}, h("th", {}, "source shard"), h("th", { class: "num" }, "applied"), h("th", { class: "num" }, "its last"), h("th", { class: "num" }, "behind"), h("th", { class: "num" }, "queued"), h("th", {}, "holds"), h("th", {}, ""))),
      h("tbody", {}, shards.map((s) => h("tr", {},
        h("td", {}, `shard ${s.shard}`), h("td", { class: "num" }, fmt.n(s.applied)), h("td", { class: "num" }, s.last != null ? fmt.n(s.last) : "–"),
        h("td", { class: "num" }, s.behind != null ? fmt.n(s.behind) : "–"), h("td", { class: "num" }, s.queued != null ? fmt.n(s.queued) : "–"),
        h("td", {}, s.source_holds ? pill("ok", "held") : h("span", { class: "dim" }, "–")),
        h("td", { class: "small dim" }, s.joining ? "joining" : s.stopped ? `stopped: ${s.stopped}` : "")))))) : null;
    const promo = c.promoted_ms ? h("div", { class: "small dim" }, "Promoted ", timeEl(c.promoted_ms), c.promoted_seqs ? ` at transactions ${c.promoted_seqs.join(", ")} of its shards, from which ${c.name} can follow it back without a copy.` : ".") : null;
    clear(cur, card({ title: following ? `Following ${c.name}` : `Followed ${c.name}`, sub: following ? null : promoted(c) ? "promoted from it" : "stopped", tools, body: h("div", { class: "stack" }, kv, tbl, promo) }));
  }

  function drawWrites() {
    const shards = hold?.shards || [];
    const any = shards.some((s) => s.holding);
    const since = Math.min(...shards.filter((s) => s.holding).map((s) => s.since_ms));
    const btn = any
      ? gated(h("button", { class: "btn sm", onclick: () => setHold(false) }, icon("unlock"), "End the hold…"))
      : v?.following ? null : gated(h("button", { class: "btn sm", onclick: () => setHold(true) }, icon("lock"), "Hold writes…"));
    clear(writes, card({
      title: "This cluster's writes", tools: btn ? [btn] : null,
      body: any
        ? h("div", { class: "stack" }, h("div", { class: "banner warn", style: { margin: 0 } }, icon("lock", "lg"), h("div", { class: "grow" },
            h("div", { class: "big" }, hold.holding ? "Held on every shard" : `Held on ${shards.filter((s) => s.holding).length} of ${shards.length} shards`),
            h("div", { class: "small dim" }, "Since ", timeEl(since), ": its clients' writes get 503 (\"this cluster holds its writes\"), as a switch-over to a cluster that follows it begins. A cluster that follows this one back ends the hold with its first write."))))
        : h("div", { class: "small dim" }, v?.following ? `Refused while it follows ${v.following}: "this cluster follows ${v.following}".` : "Taken. Hold them before switching over to a cluster that follows this one, so that it loses no write."),
    }));
  }

  function drawPast(c) {
    const others = (v?.clusters || []).filter((x) => x !== c);
    if (!others.length) { clear(past); return; }
    clear(past, card({
      title: "Followed before", flush: true,
      body: h("div", { class: "tbl-wrap" }, h("table", { class: "t" },
        h("thead", {}, h("tr", {}, h("th", {}, "cluster"), h("th", {}, "how it ended"), h("th", {}, "when"), h("th", { class: "num" }, "shards applied"))),
        h("tbody", {}, others.sort((a, b) => last(b) - last(a)).map((x) => h("tr", {},
          h("td", {}, h("b", {}, x.name)), h("td", {}, promoted(x) ? "promoted from it" : x.following ? "following" : "stopped"),
          h("td", {}, timeEl(Math.max(x.promoted_ms || 0, x.stopped_ms || 0) || x.since_ms)), h("td", { class: "num" }, fmt.n((x.applied || []).length))))))),
    }));
  }

  const gated = (b) => { if (!can("admin")) { b.disabled = true; b.title = needs("admin"); } return b; };

  async function pause(c, paused) {
    try {
      await api("/v1/admin/follow/pause", { body: { name: c.name, paused }, reason: `${paused ? "pause" : "resume"} following ${c.name}` });
      toast(paused ? `Paused: the follow process of ${c.name} stops; the cluster still refuses its clients' writes` : `Resumed: the follow process of ${c.name} starts again from where it was`, "ok", 6000);
    } catch (e) { toast(e.message, "bad", 9000); }
    load();
  }

  async function stop(c) {
    const ok = await confirm({
      title: `Stop following ${c.name}?`, ok: "Stop following", danger: true, reason: true,
      body: h("div", { class: "stack" },
        h("p", { class: "dim", style: { margin: 0 } }, `From then on this cluster takes its own clients' writes, and refuses the follow process's. What ${c.name} takes after this is not copied here. To take over from ${c.name} without losing its last writes, promote instead.`),
        spec()?.name === c.name ? h("p", { class: "dim small", style: { margin: 0 } }, "Its operator's follow process goes too (spec.follow).") : null),
    });
    if (!ok) return;
    try { await api("/v1/admin/follow/stop", { body: { name: c.name }, reason: ok.reason }); toast(`Stopped following ${c.name}`, "ok"); }
    catch (e) { toast(e.message, "bad", 9000); }
    load();
  }

  /** Promotes this cluster: planned (hold the source, wait, promote) when it can, else typed. */
  async function promote(c) {
    const lost = c.status?.behind;
    const src = srcConn;
    const steps = h("ol", { class: "follow-steps", style: { margin: 0 } });
    const say = h("div", { class: "stack" });
    const go = h("button", { class: "btn primary" }, icon("bolt"), "Switch over");
    const anyway = h("button", { class: "btn danger" }, "Promote, losing them…");
    const no = h("button", { class: "btn" }, "Cancel");
    let busy = false, cancelled = false;
    const step = (text, state = "todo") => h("li", { class: "follow-step " + state }, state === "done" ? icon("check") : state === "doing" ? icon("clock") : state === "bad" ? icon("warn") : null, " ", text);
    const holdable = c.source_holds || !!src;
    const plan = (at = 0, bad = null) => {
      const items = [
        c.source_holds ? `${c.name} holds its writes already, as far as this cluster applied` : src ? `hold ${c.name}'s writes (through the hub): its clients get 503 from then on` : `${c.name}'s writes held: the hub cannot reach it here`,
        `wait until this cluster applied them up to the hold (${lost != null ? fmt.plural(lost, "transaction") : "?"} to go)`,
        "promote: this cluster takes its own clients' writes, and stops following",
      ];
      clear(steps, items.map((t, i) => step(t, i < at ? "done" : i === at ? (bad ? "bad" : busy ? "doing" : "todo") : "todo")));
    };
    plan(c.source_holds ? 1 : 0);
    clear(say, holdable
      ? h("p", { class: "dim", style: { margin: 0 } }, `A planned switch-over loses no write: ${c.name} stops taking writes, this cluster applies the last of them and takes over. Then ${c.name} can follow this cluster back without a copy.`)
      : h("div", { class: "stack" },
          h("div", { class: "error" }, `${c.name} is not a cluster this hub reaches, so it cannot be held from here. Hold its writes there (Follow → Hold writes, or POST /v1/admin/hold-writes), then promote here; or promote now and lose what this cluster did not apply.`)));
    const m = modal({ title: `Promote this cluster, taking over from ${c.name}?`, body: [say, steps], foot: [no, anyway, holdable ? go : null], wide: true, onClose: () => { cancelled = true; } });
    no.onclick = () => { cancelled = true; m.close(); };
    anyway.onclick = async () => {
      const typed = lost != null ? String(lost) : c.name;
      const ok = await confirm({
        title: lost != null ? `Promote, losing ${fmt.plural(lost, "transaction")}?` : `Promote, losing an unknown number of writes?`, ok: "Promote", danger: true, typed, reason: true,
        body: lost != null ? `The follow process last said it was ${fmt.plural(lost, "transaction")} behind ${c.name}: those writes stay on ${c.name} only. If ${c.name} comes back, it rejoins with a copy (Copy afresh), which drops them.`
          : `No follow process said how far behind it is: what this cluster did not apply of ${c.name} is lost. Type the name to go on.`,
      });
      if (!ok) return;
      try {
        const r = await api("/v1/admin/follow/promote", { body: { name: c.name, unplanned: true }, reason: ok.reason, timeout: 60000 });
        m.close();
        toast(`Promoted from ${c.name}, unplanned: ${r.lost != null ? fmt.plural(r.lost, "transaction") : "an unknown number of transactions"} lost`, "ok", 9000);
      } catch (e) { clear(say, h("div", { class: "error" }, e.message)); }
      load();
    };
    go.onclick = async () => {
      if (busy) return;
      busy = true; go.disabled = true; anyway.disabled = true;
      let at = c.source_holds ? 1 : 0;
      try {
        if (!c.source_holds) {
          plan(0);
          await api(`/c/${src.name}/v1/admin/hold-writes`, { hub: true, body: { hold: true }, reason: `switch-over to ${connectionName || "its follower"}`, timeout: 30000 });
          at = 1; plan(1);
          const t0 = Date.now();
          for (;;) {
            if (cancelled) { toast(`Not promoted: ${c.name} still holds its writes. End the hold there, or promote here.`, "info", 9000); return; }
            const now = await api("/v1/admin/follow");
            const x = (now.clusters || []).find((y) => y.name === c.name);
            if (x?.source_holds) break;
            if (Date.now() - t0 > 120_000) throw new Error(`this cluster did not apply ${c.name}'s writes up to its hold in 120 s (${fmt.plural(x?.status?.behind ?? 0, "transaction")} behind): ${c.name} still holds its writes. Try again, or end the hold there.`);
            clear(steps.children[1], icon("clock"), ` wait until this cluster applied them up to the hold (${x?.status?.behind != null ? fmt.plural(x.status.behind, "transaction") : "?"} to go, ${Math.round((Date.now() - t0) / 1000)} s)`);
            await new Promise((r) => setTimeout(r, 1000));
          }
        }
        at = 2; plan(2);
        const r = await api("/v1/admin/follow/promote", { body: { name: c.name }, reason: `planned switch-over from ${c.name}`, timeout: 60000 });
        plan(3);
        clear(say, h("div", { class: "banner ok", style: { margin: 0 } }, icon("okc", "lg"), h("div", { class: "grow" },
          h("div", { class: "big" }, r.already ? `Promoted from ${c.name} already` : `Promoted from ${c.name}: no write lost`),
          h("div", { class: "small dim" }, `This cluster takes its own clients' writes. ${c.name} holds its writes still: make it follow this cluster back, without a copy, or end its hold there.`))));
        const back = src && connectionName ? h("button", { class: "btn primary" }, icon("share"), `Make ${src.name} follow this cluster back…`) : null;
        if (back) back.onclick = async () => { m.close(); const b = await api("/v1/admin/hub", { hub: true }).catch(() => book); await startFollow(b, { follower: src.name, source: connectionName, start: "reverse" }); load(); };
        const close = h("button", { class: "btn" }, "Close");
        close.onclick = () => m.close();
        m.el.querySelector(".ft").replaceChildren(close, ...(back ? [back] : []));
      } catch (e) {
        plan(at, true);
        say.append(h("div", { class: "error" }, e.message));
        busy = false; go.disabled = false; anyway.disabled = false;
      }
      load();
    };
  }

  async function setHold(on) {
    const ok = await confirm({
      title: on ? "Hold this cluster's writes?" : "End the hold of this cluster's writes?", ok: on ? "Hold writes" : "End the hold", danger: on, reason: true,
      body: on ? "Every shard refuses its clients' writes (503, \"this cluster holds its writes\") from then on, a change of tables too, until the hold ends. A cluster that follows this one applies the last of them; then promote it there. A cluster that follows this one back ends the hold with its first write."
        : "Its clients' writes are taken again. If a follower was promoted from this cluster meanwhile, both now take writes: make this one follow it back instead (Follow back), which ends the hold itself.",
    });
    if (!ok) return;
    try { await api("/v1/admin/hold-writes", { body: { hold: on }, reason: ok.reason }); toast(on ? "Held: this cluster refuses its clients' writes" : "The hold ended: this cluster takes its clients' writes", "ok", 6000); }
    catch (e) { toast(e.message, "bad", 9000); }
    load();
  }

  const stopTimer = every(3000, load);
  return () => { alive = false; stopTimer(); };
}
