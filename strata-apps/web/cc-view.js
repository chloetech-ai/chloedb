// The app engine's page in ChloeDB's command center (#/app/apps). The
// command center loads this file through strata-server, which forwards
// /v1/apps/apps/... to the engine, and runs render(el, ctx) with its own
// helpers: ctx.core (h, card, table, empty, failure, toast, every, ...) and
// ctx.api (the engine's API, called with the signed-in key: read scope for
// GET, admin for the rest). It imports nothing.
//
// The studio itself is not shown here. It runs code a model wrote, so it
// keeps an address of its own, away from the command center's: this page
// lists what the engine built and spent, and opens the studio with a
// one-time link (POST /api/handoff, which strata-server lets through only
// with an admin key).

export default { render };

const CSS = `
.sa-cc .lead { color: var(--muted); max-width: 46rem; }
.sa-cc .stats { display: grid; grid-template-columns: repeat(auto-fill, minmax(170px, 1fr)); gap: 10px; }
.sa-cc .stat { border: 1px solid var(--line); border-radius: 8px; padding: 10px 12px; background: var(--surface); }
.sa-cc .stat b { display: block; font-size: 18px; font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
.sa-cc .stat span { color: var(--muted); font-size: 12.5px; }
.sa-cc .stack { display: flex; flex-direction: column; gap: 14px; }
`;

function render(el, ctx) {
  const core = ctx.core;
  const { h, card, table, failure, toast, every } = core;
  el.append(h("style", {}, CSS));
  const open = async (next = "") => {
    try {
      const { url } = await ctx.api("/api/handoff", { method: "POST", body: { next } });
      window.open(url, "_blank", "noopener");
    } catch (e) {
      toast(e.status === 403 ? "Opening the studio needs an admin key" : e.message, "bad");
    }
  };
  const head = h("div", { class: "page-head" },
    h("div", {}, h("h1", {}, "App engine"),
      h("p", { class: "lead" }, "Apps built from a description: a model writes them, the engine checks each piece and keeps it in this cluster as rows, and you change an app by asking. The studio opens in a tab of its own.")),
    h("div", { class: "tools" }, h("button", { class: "btn primary", type: "button", onclick: () => open() }, "Open the studio")));
  const stats = h("div", { class: "stats" });
  const apps = table([
    { key: "name", label: "App", render: (a) => h("div", {}, h("b", {}, a.name || a.id), a.description ? h("div", { class: "small muted" }, a.description) : null) },
    { key: "head", label: "Version", num: true, render: (a) => (a.head ? `v${a.head}` : "building") },
    { key: "created", label: "Made", render: (a) => (a.created ? new Date(a.created).toLocaleDateString() : "–") },
    { key: "open", label: "", sortable: false, render: (a) => h("button", { class: "btn sm", type: "button", onclick: () => open(`#/app/${a.id}`) }, "Open") },
  ], { sortKey: "created", desc: true, empty: core.empty("info", "No apps yet", "Open the studio and describe one.") });
  const body = h("div", { class: "sa-cc stack" }, head, stats, card({ title: "Apps", body: apps, flush: true }));
  el.append(body);

  const stat = (value, label) => h("div", { class: "stat" }, h("b", {}, value), h("span", {}, label));
  const stop = every(15000, async () => {
    try {
      const [list, status] = await Promise.all([ctx.api("/api/apps"), ctx.api("/api/status")]);
      apps.update(list.apps || []);
      const s = status.spend;
      stats.replaceChildren(
        stat(String((list.apps || []).length), "apps"),
        stat(s ? `$${s.usd.toFixed(2)}` : "no limit", s ? `spent of the $${s.cap} limit` : "on what the model may spend"),
        stat(status.model || "–", "model that builds"),
        stat(status.login?.publicUrl ? new URL(status.login.publicUrl).host : "not set", "the studio's address"));
    } catch (e) {
      body.replaceChildren(head, failure(e, "The app engine"));
    }
  });
  return () => stop();
}
