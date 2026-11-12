// The relevance autopilot (/v1/relevance/autopilot): the rank log's queries
// replayed on one snapshot with a rank profile as it is and with the numbers
// a search finds, and the measured before and after, shown as a proposal
// before anything is applied. Its verdict is drawn from queries the search
// did not see. Claude can grade the rows no one graded first (UMBRELA's
// prompt), which the judgments table keeps. A proposal is applied only while
// the profile is the one it was measured against; the server refuses one
// that is stale.

import { h, clear, fmt, api, toast, confirm, empty, sqlBlock, failure, card, can, gate, needs } from "../core.js";
import { isProduction } from "../data.js";
import { invalidate } from "../catalog.js";

export default { id: "autopilot", title: "Relevance autopilot", icon: "relevance", group: "Labs", range: false, keywords: "relevance autopilot ranking tune proposal ndcg proof rank profile rank log judgments replay held out sign test", render };

const VERDICTS = {
  propose: ["ok", "Proposed"],
  not_significant: ["warn", "Not significant"],
  no_gain: ["muted", "No gain held out"],
  no_change: ["muted", "Nothing better"],
  slower: ["warn", "Too slow"],
  too_few_judged: ["muted", "Too few graded"],
  no_workload: ["muted", "No workload"],
};
const n4 = (x) => (x == null ? "–" : Number(x).toFixed(3));
const signed = (x) => (x == null ? "–" : (x >= 0 ? "+" : "") + Number(x).toFixed(3));

function tile(label, value, sub, tone) {
  return h("div", { class: "ap-tile" }, h("div", { class: "small muted" }, label), h("b", { class: tone ? "tone-" + tone : "" }, value), sub ? h("div", { class: "small muted" }, sub) : null);
}

function render(page) {
  const head = h("div", { class: "ap-head" });
  const body = h("div", { class: "ap-body" });
  page.append(h("div", { class: "ap" }, head, body));
  let list = [];

  const profileIn = h("input", { class: "input", placeholder: "Profile (the most logged when empty)", "aria-label": "Rank profile" });
  const sinceIn = h("input", { class: "input", type: "date", "aria-label": "Logged since" });
  const judgeIn = h("input", { type: "checkbox", checked: true, id: "ap-judge" });
  const runBtn = h("button", { class: "btn primary", onclick: () => run() }, "Run the autopilot");
  gate(runBtn, "admin");
  clear(head,
    h("div", { class: "ap-intro" },
      h("h2", {}, "Relevance autopilot"),
      h("p", { class: "small muted" }, "Replays the queries the rank log kept against one snapshot, with the rank profile as it is and with the numbers a search finds, and shows the measured before and after on queries the search did not see. Nothing changes until you apply a proposal.")),
    h("div", { class: "ap-form" }, profileIn, h("label", { class: "small muted" }, "Logged since ", sinceIn),
      h("label", { class: "small", for: "ap-judge" }, judgeIn, " Claude grades the rows no one graded"), runBtn));

  async function load(open) {
    try { list = (await api("/v1/relevance/proposals")).proposals || []; } catch (e) { clear(body, failure(e, "The proposals")); return; }
    if (!list.length) { clear(body, empty("relevance", "No proposals yet", "Log queries that rank by a rank profile (\"log\": true), grade some rows in the relevance lab or the judgments table, then run the autopilot.")); return; }
    clear(body);
    const first = open || list[0].id;
    for (const p of list) body.append(row(p, p.id === first));
  }

  function row(p, expanded) {
    const [tone, label] = VERDICTS[p.verdict] || ["muted", p.verdict];
    const detail = h("div", { class: "ap-detail" });
    const el = card({
      title: `${p.profile} on ${p.target}`,
      sub: `${fmt.ago(Date.parse(p.ts))} · ${p.status}`,
      tools: [h("span", { class: "pill " + tone }, label), h("span", { class: "small muted" }, `nDCG ${n4(p.before)} to ${n4(p.after)}`)],
      body: detail,
      attrs: { "data-proposal": p.id },
    });
    const toggle = async () => {
      if (detail.childElementCount) { clear(detail); return; }
      clear(detail, h("div", { class: "small muted" }, "Loading the proof…"));
      try { draw(detail, await api(`/v1/relevance/proposals/${encodeURIComponent(p.id)}`)); } catch (e) { clear(detail, failure(e, "The proposal")); }
    };
    el.querySelector(".hd").style.cursor = "pointer";
    el.querySelector(".hd").onclick = toggle;
    if (expanded) toggle();
    return el;
  }

  function draw(el, p) {
    const r = p.proof || {};
    const hOut = r.held_out || {}, lat = r.latency_ms || {};
    const changes = r.changes || [];
    const worse = (r.per_query || []).filter((q) => q.held_out && q.after < q.before - 1e-9).sort((a, b) => (a.after - a.before) - (b.after - b.before));
    const headline = changes.length ? changes.map((c) => `${c.knob}: ${c.from} to ${c.to}`).join(", ") : "No number moved";
    const parts = [
      h("div", { class: "ap-kicker small muted" }, `Proposal ${p.id} for table ${p.target}`),
      h("div", { class: "ap-headline" }, headline),
      h("p", { class: "small muted" }, `Replayed ${fmt.plural(r.workload?.queries ?? 0, "logged query", "logged queries")} (${r.workload?.requests ?? 0} requests, ${r.workload?.judged ?? 0} graded) against the snapshot at write ${r.snapshot ?? "–"}; the verdict is drawn from the ${hOut.queries ?? 0} held out of the search.`),
      h("div", { class: "ap-tiles" },
        tile(r.metric || "nDCG@10", `${n4(hOut.before)} to ${n4(hOut.after)}`, `${signed(hOut.delta)}, 95% interval ${signed(hOut.ci?.[0])} to ${signed(hOut.ci?.[1])}`),
        tile("Queries better, worse", `${hOut.better ?? 0} and ${hOut.worse ?? 0}`, `sign test p = ${n4(hOut.p)}`, (hOut.better ?? 0) > (hOut.worse ?? 0) ? "ok" : null),
        tile("p95 latency", `${fmt.ms(lat.before?.p95)} to ${fmt.ms(lat.after?.p95)}`, `p50 ${fmt.ms(lat.before?.p50)} to ${fmt.ms(lat.after?.p50)}`),
        tile("Rows graded in the top k", `${Math.round((r.judged_at_k?.before ?? 0) * 100)}% to ${Math.round((r.judged_at_k?.after ?? 0) * 100)}%`, `${(r.holes || []).length} not graded`)),
      h("p", { class: "small" }, r.reason || ""),
    ];
    if (p.statement) parts.push(sqlBlock(p.statement));
    const actions = h("div", { class: "ap-actions" });
    if (p.status === "open" && p.statement) {
      const apply = h("button", { class: "btn primary", onclick: () => doApply(p) }, p.verdict === "propose" ? "Apply" : "Apply anyway");
      gate(apply, "admin");
      actions.append(apply);
    }
    if (worse.length) actions.append(h("button", { class: "btn", onclick: () => showWorse(el, worse) }, `See the ${worse.length} worse`));
    if (p.status === "open") {
      const dismiss = h("button", { class: "btn", onclick: () => doDismiss(p) }, "Dismiss");
      gate(dismiss, "admin");
      actions.append(dismiss);
    }
    parts.push(actions);
    clear(el, ...parts);
  }

  function showWorse(el, worse) {
    const box = el.querySelector(".ap-worse") || el.appendChild(h("div", { class: "ap-worse" }));
    clear(box, h("table", { class: "t compact" },
      h("thead", {}, h("tr", {}, h("th", {}, "Query"), h("th", { class: "num" }, "Before"), h("th", { class: "num" }, "After"))),
      h("tbody", {}, worse.slice(0, 50).map((q) => h("tr", {}, h("td", {}, q.query), h("td", { class: "num" }, n4(q.before)), h("td", { class: "num" }, n4(q.after)))))));
  }

  async function doApply(p) {
    const body = [h("p", {}, `Every query that ranks by ${p.profile} ranks the new way from then on.` + (isProduction() ? " This cluster counts as production." : "")), sqlBlock(p.statement)];
    const ok = await confirm({ title: `Apply proposal ${p.id}?`, body, ok: "Apply it", danger: p.verdict !== "propose" });
    if (!ok) return;
    try {
      await api(`/v1/relevance/proposals/${encodeURIComponent(p.id)}/apply`, { body: { force: p.verdict !== "propose" } });
      toast(`Rank profile ${p.profile} replaced`, "ok"); invalidate(); load(p.id);
    } catch (e) { toast(e.message, "bad", 9000); }
  }

  async function doDismiss(p) {
    try { await api(`/v1/relevance/proposals/${encodeURIComponent(p.id)}/dismiss`, { body: {} }); toast(`Proposal ${p.id} dismissed`, "ok"); load(p.id); } catch (e) { toast(e.message, "bad"); }
  }

  async function run() {
    if (!can("admin")) { toast(needs("admin"), "bad"); return; }
    runBtn.disabled = true; runBtn.textContent = "Replaying…";
    const b = { judge: judgeIn.checked };
    if (profileIn.value.trim()) b.profile = profileIn.value.trim();
    if (sinceIn.value) b.since = sinceIn.value + "T00:00:00Z";
    try {
      const j = await api("/v1/relevance/autopilot", { body: b });
      const r = j.report || {};
      if (j.judged?.errors?.length) toast(`Claude could not grade every row: ${j.judged.errors[0]}`, "warn", 9000);
      if (!j.proposal) toast(r.reason || "Nothing to propose", "info", 9000);
      await load(j.proposal || undefined);
    } catch (e) { toast(e.message, "bad", 9000); }
    finally { runBtn.disabled = false; runBtn.textContent = "Run the autopilot"; }
  }

  load();
}
