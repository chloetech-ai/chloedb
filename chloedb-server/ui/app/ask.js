// Ask: a question about the data, in words, from any page (T6). The
// server's agent (POST /v1/agent) reads the tables, runs read-only SQL
// and answers; the drawer shows the answer and every query it ran, each
// one a click from the workbench. It was only in the old console before.
// The page the person is on goes with the question (a table's page names
// the table), so "what is most common here" means something.

import { h, icon, clear, api, drawer, fmt, toast, sqlBlock, link, can, needs } from "./core.js";

let cfg = null;
const KEY = "chloedb.anthropic";
const history = [];
let whereOf = () => null;
/** The shell says where the person is: {title, table}. */
export function setWhereOf(f) { whereOf = f; }

async function config() {
  if (cfg) return cfg;
  try { cfg = (await api("/v1/config", { timeout: 4000 })).agent || {}; } catch { cfg = {}; }
  return cfg;
}

export async function openAsk(initial = "") {
  const c = await config();
  const where = whereOf() || {};
  const inp = h("textarea", { class: "input", rows: 2, placeholder: where.table ? `A question about ${where.table}, or any table: "which topics have the most rows?"` : "A question about your data: \"how many orders failed yesterday, by region?\"", style: { width: "100%", resize: "vertical", fontFamily: "var(--sans)", fontSize: "14px" } });
  inp.value = initial;
  let key = null;
  try { key = sessionStorage.getItem(KEY); } catch { /* private mode */ }
  const keyIn = c.server_key ? null : h("input", { class: "input sm", type: "password", placeholder: "sk-ant-… (kept in this tab only)", value: key || "", autocomplete: "off", style: { width: "100%" } });
  const go = h("button", { class: "btn primary" }, icon("sparkle"), "Ask");
  const log = h("div", { class: "ask-log" });
  const d = drawer({
    title: "Ask about your data",
    sub: `The server's agent reads the tables and runs read-only SQL to answer${where.title ? ` · from ${where.title}` : ""}`,
    wide: true,
    body: h("div", { class: "stack" },
      keyIn ? h("div", { class: "stack", style: { gap: "4px" } }, h("div", { class: "small dim" }, "This server has no Anthropic API key (ANTHROPIC_API_KEY). Give one for this tab: it is sent to this server with each question, which calls the model with it, and is not stored there."), keyIn) : null,
      inp, h("div", { class: "row" }, go, h("span", { class: "small muted" }, "Ctrl+Enter asks"), h("span", { class: "grow" }), can("write") ? null : h("span", { class: "small muted" }, `Asking ${needs("write")}`)),
      log),
  });
  const drawLog = () => clear(log, history.slice().reverse().map((x) => h("div", { class: "ask-item" },
    h("div", { class: "ask-q" }, icon("user"), h("span", {}, x.q)),
    x.pending ? h("div", { class: "small muted" }, "Thinking, and querying the tables…") : x.error ? h("div", { class: "error" }, x.error) : [
      h("div", { class: "ask-a" }, x.a.answer),
      (x.a.steps || []).length ? h("details", { class: "card-ish" }, h("summary", {}, `${fmt.plural(x.a.steps.length, "query", "queries")} it ran`),
        x.a.steps.map((s) => h("div", { class: "ask-step" }, h("div", { class: "row nw small" }, h("span", { class: s.ok ? "muted" : "error" }, s.ok ? `${fmt.plural(s.rows ?? 0, "row")} · ${fmt.ms(s.took_ms)}` : s.error), h("span", { class: "grow" }), s.sql ? h("a", { class: "btn xs", href: link("sql", { q: s.sql }), onclick: () => d.close() }, icon("sql"), "Open") : null), s.sql ? sqlBlock(s.sql) : null))) : null,
      h("div", { class: "small muted" }, `${x.a.model} · ${fmt.ms(x.a.took_ms)} · ${fmt.n((x.a.usage?.input_tokens || 0) + (x.a.usage?.output_tokens || 0))} tokens`)],
  )));
  async function ask() {
    const q = inp.value.trim();
    if (!q) return;
    const k = keyIn?.value.trim() || null;
    if (keyIn && !k) { toast("Give an Anthropic API key, or start the server with ANTHROPIC_API_KEY", "bad"); keyIn.focus(); return; }
    if (k) { try { sessionStorage.setItem(KEY, k); } catch { /* private mode */ } }
    const w = whereOf() || {};
    const item = { q, pending: true };
    history.push(item); drawLog();
    inp.value = "";
    go.disabled = true;
    try {
      const question = w.table ? `(I am looking at table ${w.table} in the command center.) ${q}` : q;
      item.a = await api("/v1/agent", { body: { question, api_key: k || undefined }, timeout: 300000 });
    } catch (e) { item.error = e.message; }
    item.pending = false; go.disabled = false; drawLog();
  }
  go.onclick = ask;
  inp.onkeydown = (e) => { if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) { e.preventDefault(); ask(); } };
  drawLog();
  setTimeout(() => inp.focus(), 30);
}
