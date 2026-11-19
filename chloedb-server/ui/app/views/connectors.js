// Connectors: other systems' data into tables (chloedb-server's
// crate::connectors). Each connector is a row of chloedb_connectors; its
// runs are rows of chloedb_connector_runs. This page shows each one's
// health, last runs and counts, runs one now, tries a config on real data
// without writing (a dry run), and adds, changes, turns off and deletes
// them. A push connector's key is shown once, when it is made or rotated.
// Kafka connectors stream rather than run: their section shows each one's
// consumer live (rate, lag by partition), starts and stops it, and the
// test broker (a Kafka broker in the server's process, made-up click
// events) gives one something to connect to.

import { h, icon, clear, fmt, api, toast, confirm, card, empty, failure, pill, copy, drawer, modal, every, can, needs, toggle } from "../core.js";

// A run under a millisecond reads "<1 ms" rather than "0".
// A target table, linked to its page in Tables (a click there does not open the drawer).
const tableLink = (t) => h("a", { href: `#/table/${encodeURIComponent(t)}?tab=data`, title: `Open ${t} in Tables`, onclick: (e) => e.stopPropagation() }, t);
const took = (v) => (v === 0 ? "<1 ms" : fmt.ms(v));

export default {
  id: "connectors", title: "Connectors", icon: "streams", group: "Data",
  keywords: "connectors data sources ingest prometheus jira servicenow oxidized email mail webhook alertmanager rest api pull push cdc health kafka redpanda topics stream consumer broker",
  render,
};

const HEALTH = {
  ok: ["ok", "ok"],
  warning: ["warn", "warning"],
  failing: ["failing", "failing"],
  invalid: ["failing", "invalid"],
  stale: ["warn", "stale"],
  running: ["info", "running"],
  streaming: ["ok", "streaming"],
  connecting: ["info", "connecting"],
  pending: ["pending", "not run yet"],
  waiting: ["pending", "waiting for a push"],
  disabled: ["disabled", "off"],
};

// A starting config for each kind, for the form.
const TEMPLATES = {
  prometheus: { config: { url: "http://prometheus:9090", queries: { cpu: "avg by (device) (rate(node_cpu_seconds_total{mode!=\"idle\"}[1m]))", mem: "avg by (device) (1 - node_memory_MemAvailable_bytes / node_memory_MemTotal_bytes)" }, key: ["device"] }, secrets: {} },
  jira: { config: { url: "https://example.atlassian.net", jql: "project = NET AND issuetype = Change", username: "bot@example.com" }, secrets: { token: "" } },
  servicenow: { config: { instance: "https://example.service-now.com", table: "change_request", username: "integration" }, secrets: { password: "" } },
  oxidized: { config: { url: "http://oxidized:8888" }, secrets: {} },
  mail: { config: {}, secrets: {} },
  webhook: { config: { preset: "alertmanager" }, secrets: {} },
  http_json: { config: { url: "https://api.example.com/items?since={cursor}", records: "$.items", cursor: "updated_at", headers: { Authorization: "Bearer {secret:token}" } }, secrets: { token: "" } },
  kafka: { config: { brokers: "127.0.0.1:19092", topics: "demo.clicks", start: "earliest", allow_internal: true }, secrets: {} },
};

// The test broker's made-up click events, as columns of their own table.
const DEMO_MAP = {
  id: "$.value.event_id", user_id: { path: "$.value.user", type: "KEYWORD" }, item: { path: "$.value.item", type: "KEYWORD" }, action: { path: "$.value.action", type: "KEYWORD" }, page: { path: "$.value.page", type: "KEYWORD" },
  price: { path: "$.value.price", as: "float" }, at: { path: "$.value.at", as: "time" }, kafka_partition: { path: "$.partition", as: "int" }, kafka_offset: { path: "$.offset", as: "int" },
};
const STREAM = { streaming: ["ok", "streaming"], connecting: ["info", "connecting"], starting: ["info", "starting"], retrying: ["failing", "retrying"], stopped: ["disabled", "stopped"], pending: ["pending", "waiting to start"] };
const streamPill = (c) => { const st = !c.enabled ? "stopped" : c.stream?.state || "pending"; const [k, text] = c.health === "warning" && st === "streaming" ? ["warn", "streaming, with a warning"] : STREAM[st] || ["info", st]; return pill(k, text); };
const rateText = (r) => (r == null ? "–" : r >= 100 ? `${fmt.si(r)}/s` : `${fmt.n(r, 1)}/s`);

// A kind that pulls with no schedule (every_s 0) runs by hand (DAP-18).
const every_text = (c, kinds) => {
  if (c.streams) return c.enabled ? "streams" : "stopped";
  if (c.pulls) return every_pull(c);
  const k = (kinds || []).find((x) => x.kind === c.kind);
  return k?.pull ? (k.push ? "pushed, or by hand" : "by hand") : "pushed";
};
const every_pull = (c) => (c.every_s >= 3600 ? `every ${fmt.n(c.every_s / 3600, 1)} h` : c.every_s >= 60 ? `every ${fmt.n(c.every_s / 60, 1)} min` : `every ${c.every_s} s`);
const healthPill = (c) => { const [st, text] = HEALTH[c.health] || ["info", c.health]; return pill(st, text); };
const pretty = (j) => JSON.stringify(j, null, 2);

function render(page) {
  const top = h("div"), main = h("div", { class: "stack" }), side = h("div", { class: "stack" });
  page.append(top, h("div", { class: "conn-grid" }, main, side));
  let v = null, broker = null, loaded = 0;
  // A partitions list someone opened stays open across redraws; a form
  // in the side someone is filling in is not redrawn under them.
  const openParts = new Set();
  side.addEventListener("input", () => { side.dataset.dirty = "1"; });
  async function load(quiet = false) {
    try { [v, broker] = await Promise.all([api("/v1/admin/connectors"), api("/v1/admin/kafka/test-broker").catch(() => null)]); loaded = Date.now(); }
    catch (e) { if (quiet && v) return; clear(top); clear(main, failure(e, "Connectors")); clear(side); return; }
    draw(quiet);
  }
  function draw(quiet = false) {
    const cs = v.connectors || [];
    const counts = Object.entries(v.health || {}).map(([k, n]) => { const [st, text] = HEALTH[k] || ["info", k]; return pill(st, `${n} ${text}`); });
    clear(top, h("div", { class: "row", style: { gap: "8px", flexWrap: "wrap", marginBottom: "12px" } },
      ...counts,
      v.scheduler?.on ? h("span", { class: "small dim" }, icon("clock"), " pulls run on this server") : h("span", { class: "small warn" }, icon("warn"), " ", v.scheduler?.why || "pulls do not run here")));
    const rows = cs.map((c) => {
      const st = c.state || {};
      const last = c.runs?.[0];
      return h("tr", { class: "click", onclick: () => open(c.name) },
        h("td", {}, h("div", { class: "stack", style: { gap: "2px" } }, h("b", {}, c.name), h("span", { class: "small dim" }, kindTitle(c.kind), c.note ? ` · ${c.note}` : ""))),
        h("td", { class: "mono small" }, tableLink(c.target)),
        h("td", {}, healthPill(c), st.failures > 1 ? h("span", { class: "small dim" }, ` ×${st.failures}`) : null),
        c.streams ? h("td", { class: "small", style: { whiteSpace: "nowrap" } }, c.stream?.state === "streaming" && c.stream.session_ms ? `streaming since ${fmt.ago(c.stream.session_ms).replace(/ ago$/, "")}` : st.last_run_ms ? fmt.ago(st.last_run_ms) : "never", c.stream?.last_write_ms ? h("div", { class: "dim" }, `last write ${fmt.ago(c.stream.last_write_ms)}`) : null)
          : h("td", { class: "small", style: { whiteSpace: "nowrap" } }, st.last_run_ms ? fmt.ago(st.last_run_ms) : "never", last ? h("div", { class: "dim" }, `${fmt.plural(last.rows_ || 0, "row")} · ${took(last.took_ms)}`) : null),
        h("td", { class: "num small", title: `${fmt.n(st.rows_total || 0)} rows written by ${fmt.plural(st.runs || 0, "run")}: a row upserted again counts each time, so it can be more than the rows of ${c.target}` }, fmt.si(st.rows_total || 0)),
        h("td", { class: "small dim", style: { whiteSpace: "nowrap" } }, every_text(c, v.kinds)),
        (c.streams ? c.stream?.state !== "streaming" && st.last_error : st.last_error) ? h("td", { class: "small error", style: { minWidth: "200px", maxWidth: "260px", overflowWrap: "anywhere" } }, st.last_error.slice(0, 160))
          : h("td", { class: st.last_warning ? "small warn" : "small", style: { minWidth: "200px", maxWidth: "260px", overflowWrap: "anywhere" } }, st.last_warning ? st.last_warning.slice(0, 160) : ""));
    });
    clear(main, card({
      title: "Connectors", sub: `${fmt.plural(cs.length, "connector")} · configs in ${v.tables.configs}, runs in ${v.tables.runs}`,
      tools: h("button", { class: "btn sm", onclick: () => load() }, icon("refresh"), "Refresh"),
      flush: true,
      body: cs.length ? h("div", { class: "tbl-wrap" }, h("table", { class: "t" },
        h("thead", {}, h("tr", {}, h("th", {}, "connector"), h("th", {}, "target"), h("th", {}, "health"), h("th", {}, "last run"), h("th", { class: "num", title: "Rows written by every run so far (upserts counted each time), not the rows of the table" }, "rows written"), h("th", {}, "schedule"), h("th", {}, "last error"))),
        h("tbody", {}, rows)))
        : empty("streams", "No connectors yet", "Add one: Prometheus rollups, Jira or ServiceNow tickets and changes, Oxidized configs, carriers' emails, Alertmanager or any webhook, any REST API, Kafka topics."),
    }), postgresCard(), sourcesCard());
    main.prepend(kafkaCard());
    if (!(quiet && (side.dataset.dirty || side.contains(document.activeElement)))) { delete side.dataset.dirty; clear(side, brokerCard(), addCard()); }
  }

  // ----- Kafka: connectors that stream, and the test broker

  function kafkaCard() {
    const ks = (v.connectors || []).filter((c) => c.streams);
    const add = h("button", { class: "btn sm primary" }, icon("plus"), "Add a Kafka connector");
    add.onclick = () => kafkaForm();
    if (!can("admin")) { add.disabled = true; add.title = needs("admin"); }
    const body = ks.length ? h("div", { class: "stack", style: { gap: "10px" } }, ks.map(kafkaRow))
      : empty("streams", "No Kafka connectors yet", "A Kafka connector reads topics into a table while it is on, each event once: its offsets are kept with the rows. Start the test broker to try one without Kafka.",
        broker?.running ? h("button", { class: "btn sm", onclick: () => kafkaForm(demoPrefill()) }, icon("bolt"), "Connect one to the test broker") : null);
    return card({ title: "Kafka", sub: "topics this server reads, each event written once", tools: add, body, attrs: { "data-kafka": "" } });
  }

  function kafkaRow(c) {
    const s = c.stream || {};
    const parts = s.partitions || [];
    const on = c.enabled;
    const btn = h("button", { class: "btn sm" + (on ? "" : " primary") }, icon(on ? "stop" : "play"), on ? "Stop" : "Start");
    if (!can("admin")) { btn.disabled = true; btn.title = needs("admin"); }
    btn.onclick = async (e) => {
      e.stopPropagation();
      btn.disabled = true;
      try {
        const r = await api(`/v1/admin/connectors/${encodeURIComponent(c.name)}/${on ? "stop" : "start"}`, { body: {} });
        toast(on ? `${c.name} stopped` : `${c.name} started`, "ok");
        if (r.note) toast(r.note, "info", 6000);
        await load();
      } catch (err) { toast(err.message, "bad"); btn.disabled = false; }
    };
    const cfg = c.config || {};
    const topics = Array.isArray(cfg.topics) ? cfg.topics.join(", ") : cfg.topics;
    const maxLag = Math.max(1, ...parts.map((p) => p.lag || 0));
    const stat = (label, value, title) => h("div", { class: "stack", style: { gap: "0", minWidth: "84px" }, title }, h("span", { class: "small dim" }, label), h("b", { class: "mono" }, value));
    return h("div", { class: "kafka-row", style: { border: "1px solid var(--line)", borderRadius: "8px", padding: "10px 12px" } },
      h("div", { class: "row", style: { gap: "10px", flexWrap: "wrap", alignItems: "center" } },
        h("div", { class: "stack click", style: { gap: "2px", flex: "1 1 220px", minWidth: "0" }, onclick: () => open(c.name) },
          h("b", {}, c.name),
          h("span", { class: "small dim mono", style: { overflowWrap: "anywhere" } }, `${cfg.brokers || "?"} · ${topics || "?"} → `, tableLink(c.target))),
        streamPill(c), btn),
      h("div", { class: "row", style: { gap: "18px", flexWrap: "wrap", marginTop: "8px" } },
        stat("rate", on ? rateText(s.rate_10s) : "–", "Events written a second, over the last 10 s"),
        stat("lag", s.lag != null ? fmt.si(s.lag) : "–", "Events in the topics not yet written: each partition's end less its next offset"),
        stat("events", fmt.si(s.events || 0), "Events read and written since this server started (replays and those that made no row included)"),
        stat("rows", fmt.si(s.rows || 0), "Rows written since this server started"),
        s.replays ? stat("replays", fmt.si(s.replays), "Events whose offset ChloeDB had already: skipped, not written twice") : null,
        s.unmapped ? stat("no row", fmt.si(s.unmapped), s.first_unmapped || "Events no row came of") : null,
        s.lost ? stat("lost", fmt.si(s.lost), "Events the topic's retention deleted before they were read") : null,
        stat("last write", s.last_write_ms ? fmt.ago(s.last_write_ms) : "never")),
      s.last_error && s.state !== "streaming" ? h("div", { class: "small error", style: { marginTop: "6px", overflowWrap: "anywhere" } }, icon("bad"), " ", s.last_error, s.failures > 1 ? ` (×${s.failures}, retrying)` : " (retrying)") : null,
      s.warning ? h("div", { class: "small warn", style: { marginTop: "6px", overflowWrap: "anywhere" } }, icon("warn"), " ", s.warning) : null,
      parts.length ? h("details", { style: { marginTop: "6px" }, open: openParts.has(c.name), ontoggle: (e) => { if (e.target.open) openParts.add(c.name); else openParts.delete(c.name); } }, h("summary", { class: "small dim" }, `${fmt.plural(parts.length, "partition")}`),
        h("div", { class: "tbl-wrap" }, h("table", { class: "t compact" },
          h("thead", {}, h("tr", {}, h("th", {}, "partition"), h("th", { class: "num", title: "The offset of the next event to write: ChloeDB has every one before it" }, "next"), h("th", { class: "num", title: "The partition's end (Kafka's high watermark)" }, "end"), h("th", {}, "lag"), h("th", {}, "last event"))),
          h("tbody", {}, parts.map((p) => h("tr", {},
            h("td", { class: "mono small" }, `${p.topic}/${p.partition}`),
            h("td", { class: "num mono small" }, fmt.n(p.next)), h("td", { class: "num mono small" }, fmt.n(p.high)),
            h("td", { class: "small", style: { minWidth: "120px" } }, h("div", { class: "row", style: { gap: "6px" } },
              h("div", { style: { flex: "1", height: "6px", background: "var(--line)", borderRadius: "3px", overflow: "hidden" } }, h("div", { style: { width: `${Math.round(100 * (p.lag || 0) / maxLag)}%`, height: "100%", background: p.lag > 1000 ? "var(--warn)" : "var(--accent)" } })),
              h("span", { class: "mono" }, fmt.si(p.lag || 0)))),
            h("td", { class: "small" }, p.last_event_ms ? fmt.ago(p.last_event_ms) : "–"))))))) : null);
  }

  // The test broker's address, topic and a map into a table of its own.
  const demoPrefill = () => {
    const taken = new Set((v.connectors || []).map((c) => c.name));
    let n = 1;
    while (taken.has(n === 1 ? "demo-clicks" : `demo-clicks-${n}`)) n++;
    return {
      name: n === 1 ? "demo-clicks" : `demo-clicks-${n}`, brokers: broker?.address || "127.0.0.1:19092", topics: broker?.generator?.topic || "demo.clicks",
      target: n === 1 ? "demo_clicks" : `demo_clicks_${n}`, create: true, map: DEMO_MAP, internal: true,
      sasl: broker?.sasl ? { mechanism: "PLAIN", username: broker.sasl.username } : null,
    };
  };

  function brokerCard() {
    const b = broker;
    const admin = can("admin");
    if (!b?.running) {
      const listen = h("input", { class: "input mono", value: "127.0.0.1:19092", style: { width: "100%" }, spellcheck: "false" });
      const topic = h("input", { class: "input mono", value: "demo.clicks", style: { width: "100%" }, spellcheck: "false" });
      const rate = h("input", { class: "input mono", type: "number", min: "0", value: "5", style: { width: "90px" } });
      const parts = h("input", { class: "input mono", type: "number", min: "1", max: "64", value: "3", style: { width: "70px" } });
      const user = h("input", { class: "input mono", placeholder: "none", style: { width: "100%" }, autocomplete: "off" });
      const pass = h("input", { class: "input mono", type: "password", style: { width: "100%" }, autocomplete: "new-password" });
      const go = h("button", { class: "btn primary" }, icon("play"), "Start the test broker");
      if (!admin) { go.disabled = true; go.title = needs("admin"); }
      go.onclick = async () => {
        go.disabled = true;
        try {
          const body = { listen: listen.value.trim(), topic: topic.value.trim(), rate: Number(rate.value || 0), partitions: Number(parts.value || 3) };
          if (user.value.trim()) { body.username = user.value.trim(); body.password = pass.value; }
          broker = await api("/v1/admin/kafka/test-broker", { body });
          toast(`Test broker at ${broker.address}`, "ok");
          draw();
        } catch (e) { toast(e.message, "bad"); go.disabled = false; }
      };
      const f = (label, el) => h("label", { class: "stack", style: { gap: "4px" } }, h("span", { class: "small dim" }, label), el);
      return card({ title: "Test Kafka broker", sub: "a broker in this server's process, to try a connector on", body: h("div", { class: "stack", style: { gap: "8px" } },
        h("div", { class: "small dim" }, "It speaks enough of Kafka's protocol for the connector: topics kept in memory, a topic of made-up click events written at a rate, and a box to send your own. Loopback only; it stops with the server."),
        f("Listen on", listen), f("Topic of made-up events", topic),
        h("div", { class: "row", style: { gap: "10px" } }, f("Events a second", rate), f("Partitions", parts)),
        h("details", {}, h("summary", { class: "small dim" }, "Ask clients to sign in (SASL PLAIN)"), h("div", { class: "stack", style: { gap: "6px", marginTop: "6px" } }, f("User", user), f("Password", pass))),
        h("div", { class: "row" }, go)) });
    }
    const g = b.generator || {};
    const rate = h("input", { class: "input mono", type: "number", min: "0", value: String(g.rate ?? 0), style: { width: "90px" } });
    const setRate = h("button", { class: "btn sm" }, icon("check"), "Set");
    setRate.onclick = async () => { try { broker = await api("/v1/admin/kafka/test-broker", { body: { topic: g.topic, rate: Number(rate.value || 0) } }); toast(`${fmt.n(Number(rate.value || 0), 1)} events a second`, "ok"); draw(); } catch (e) { toast(e.message, "bad"); } };
    const stopBtn = h("button", { class: "btn sm danger" }, icon("stop"), "Stop the broker");
    stopBtn.onclick = async () => {
      if (!(await confirm({ title: "Stop the test broker?", body: "Its topics and their events go. Connectors reading it fail until it starts again (their offsets stay in ChloeDB: a new broker's offsets start again from 0, so give them another prefix to read it).", ok: "Stop it", danger: true }))) return;
      try { await api("/v1/admin/kafka/test-broker", { method: "DELETE" }); broker = { running: false }; draw(); } catch (e) { toast(e.message, "bad"); }
    };
    const connect = h("button", { class: "btn sm primary" }, icon("bolt"), "Connect a connector to it");
    connect.onclick = () => kafkaForm(demoPrefill());
    const sendTopic = h("input", { class: "input mono", value: g.topic || "demo.clicks", style: { width: "100%" }, spellcheck: "false" });
    const sendKey = h("input", { class: "input mono", placeholder: "key (optional)", style: { width: "100%" }, spellcheck: "false" });
    const sendValue = h("textarea", { class: "input mono", rows: 3, spellcheck: "false", style: { width: "100%" } });
    sendValue.value = JSON.stringify({ event_id: `e-hand-${Date.now()}`, user: "u001", item: "i0042", action: "buy", page: "checkout", price: 19.5, at: new Date().toISOString() });
    const send = h("button", { class: "btn sm" }, icon("rocket"), "Send");
    send.onclick = async () => {
      let value = sendValue.value; try { value = JSON.parse(value); } catch { /* sent as text */ }
      try {
        const r = await api("/v1/admin/kafka/test-broker/produce", { body: { topic: sendTopic.value.trim(), messages: [{ key: sendKey.value.trim() || null, value }] } });
        const o = r.offsets?.[0];
        toast(`Sent to ${r.topic}/${o?.partition} at offset ${o?.offset}`, "ok");
        broker = await api("/v1/admin/kafka/test-broker"); draw();
      } catch (e) { toast(e.message, "bad"); }
    };
    if (!admin) for (const x of [setRate, stopBtn, connect, send]) { x.disabled = true; x.title = needs("admin"); }
    return card({ title: "Test Kafka broker", tools: stopBtn, body: h("div", { class: "stack", style: { gap: "10px" } },
      h("div", { class: "row", style: { gap: "8px", alignItems: "center" } }, pill("ok", b.started_ms ? `up ${fmt.ago(b.started_ms).replace(/ ago$/, "")}` : "up"), h("span", { class: "mono" }, b.address), h("button", { class: "btn sm ghost", title: "Copy the address", onclick: () => copy(b.address, "Address copied") }, icon("copy")),
        b.sasl ? h("span", { class: "small dim" }, icon("lock"), ` sign-in: ${b.sasl.username}`) : null),
      h("div", { class: "tbl-wrap" }, h("table", { class: "t compact" },
        h("thead", {}, h("tr", {}, h("th", {}, "topic"), h("th", { class: "num" }, "partitions"), h("th", { class: "num" }, "events"))),
        h("tbody", {}, (b.topics || []).map((t) => h("tr", {}, h("td", { class: "mono small" }, t.name), h("td", { class: "num small" }, t.partitions.length), h("td", { class: "num mono small", title: t.partitions.map((p) => `${p.partition}: ${p.start} to ${p.end}`).join("\n") }, fmt.n(t.messages))))))),
      h("div", { class: "stack", style: { gap: "4px" } }, h("span", { class: "small dim" }, `Made-up click events into ${g.topic}`),
        h("div", { class: "row", style: { gap: "6px", alignItems: "center" } }, rate, h("span", { class: "small dim" }, "a second"), setRate)),
      h("div", { class: "small dim" }, `${fmt.n(b.connections || 0)} connections (${fmt.n(b.open || 0)} open) · ${fmt.n(b.fetches || 0)} fetches`),
      h("div", { class: "row" }, connect),
      h("details", {}, h("summary", { class: "small dim" }, "Send an event"), h("div", { class: "stack", style: { gap: "6px", marginTop: "6px" } }, sendTopic, sendKey, sendValue, h("div", { class: "row" }, send)))) });
  }

  /** The form of a new Kafka connector, `pre` filled in. */
  function kafkaForm(pre = {}) {
    const inp = (value, attrs = {}) => h("input", { class: "input mono", value: value ?? "", spellcheck: "false", autocomplete: "off", style: { width: "100%" }, ...attrs });
    const name = inp(pre.name || "", { placeholder: "orders-in" });
    const brokers = inp(pre.brokers || "", { placeholder: "kafka-1:9092,kafka-2:9092" });
    const topics = inp(pre.topics || "", { placeholder: "orders, payments" });
    const target = inp(pre.target || "", { placeholder: "kafka_events (made for it, every event as a row)" });
    let create = pre.create ?? false, internal = pre.internal ?? true, tls = false, startNow = true;
    const createT = toggle(create, (x) => { create = x; }, "Make the table from the map if it does not exist");
    const startSel = h("select", { class: "select" }, h("option", { value: "earliest" }, "its first event (earliest)"), h("option", { value: "latest" }, "its end: new events only (latest)"));
    const formatSel = h("select", { class: "select" }, h("option", { value: "json" }, "JSON"), h("option", { value: "text" }, "text"));
    const mapIn = h("textarea", { class: "input mono", rows: 7, spellcheck: "false", style: { width: "100%", resize: "vertical" }, placeholder: "{\"id\": \"$.value.order_id\", \"total\": {\"path\": \"$.value.total\", \"as\": \"float\"}}" });
    if (pre.map) mapIn.value = JSON.stringify(pre.map, null, 2);
    const mech = h("select", { class: "select" }, ["none", "PLAIN", "SCRAM-SHA-256", "SCRAM-SHA-512"].map((m) => h("option", { value: m }, m === "none" ? "no sign-in" : m)));
    const user = inp(pre.sasl?.username || "", { placeholder: "user" });
    const pass = inp("", { type: "password", autocomplete: "new-password", placeholder: "password (kept on this server, never in a table)" });
    if (pre.sasl) mech.value = pre.sasl.mechanism;
    const prefix = inp("", { placeholder: "kafka/<name>/ (default)" });
    const tomb = h("select", { class: "select" }, h("option", { value: "skip" }, "write nothing"), h("option", { value: "delete" }, "delete the row of its key"));
    const note = h("div");
    const f = (label, el, hint) => h("label", { class: "stack", style: { gap: "4px" } }, h("span", { class: "small dim" }, label), el, hint ? h("span", { class: "small dim" }, hint) : null);
    const body = () => {
      const config = { brokers: brokers.value.trim(), topics: topics.value.trim(), start: startSel.value, format: formatSel.value, allow_internal: internal };
      if (mapIn.value.trim()) { try { config.map = JSON.parse(mapIn.value); } catch (e) { throw new Error(`the map is not JSON: ${e.message}`); } }
      if (create) config.create_target = true;
      if (tls) config.tls = true;
      if (mech.value !== "none") config.sasl = { mechanism: mech.value, username: user.value.trim() };
      if (prefix.value.trim()) config.prefix = prefix.value.trim();
      if (tomb.value !== "skip") config.tombstones = tomb.value;
      const b = { name: name.value.trim(), kind: "kafka", config, enabled: startNow };
      if (target.value.trim()) b.target = target.value.trim();
      if (mech.value !== "none" && pass.value) b.secrets = { password: pass.value };
      return b;
    };
    const tryIt = h("button", { class: "btn" }, icon("eye"), "Dry run");
    const go = h("button", { class: "btn primary" }, icon("plus"), "Add the connector");
    if (!can("admin")) { go.disabled = tryIt.disabled = true; go.title = tryIt.title = needs("admin"); }
    tryIt.onclick = async () => {
      clear(note, h("div", { class: "small dim" }, "Connecting and reading a few events…"));
      try { clear(note, dryRun(await api("/v1/admin/connectors/test", { body: body(), timeout: 120000 }))); }
      catch (e) { clear(note, h("div", { class: "error small" }, e.message)); }
    };
    go.onclick = async () => {
      go.disabled = true;
      try {
        const r = await api("/v1/admin/connectors", { body: body(), timeout: 60000 });
        toast(`${r.connector.name} added${r.made_target ? `, and its table ${r.connector.target}` : ""}${startNow ? ": it starts in a few seconds" : ""}`, "ok");
        d.close();
        await load();
      } catch (e) { clear(note, h("div", { class: "error small" }, e.message)); go.disabled = false; }
    };
    const d = drawer({ title: "Add a Kafka connector", sub: "topics into a table, each event once", wide: true, body: h("div", { class: "stack", style: { gap: "10px" } },
      h("div", { class: "small dim" }, "This server reads every partition of the topics while the connector is on, and keeps each partition's offset in ChloeDB with the rows its events made (a stream source per partition), so a restart or a replay writes no event twice. It uses no consumer group and commits nothing to Kafka."),
      f("Name (lowercase, digits, - and _)", name),
      f("Bootstrap brokers", brokers, "host:port, joined by commas"),
      f("Topics", topics, "joined by commas"),
      f("Target table", target, "empty: kafka_events, made for it, each event a row (topic, partition, offset, time, key, value as JSON)"),
      createT,
      f("A partition ChloeDB has no offset of starts at", startSel),
      f("Values are", formatSel),
      f("Map (JSON, optional)", mapIn, "column → \"$.value.field\", \"$.key\", \"$.offset\", \"$.ts\", \"{template}\" or {\"path\", \"as\"}. Without one, a JSON value's fields are the columns, and a keyed table's row without its key gets topic/partition/offset."),
      toggle(internal, (x) => { internal = x; }, "The brokers are inside this network (a private address, or this machine)"),
      h("details", {}, h("summary", { class: "small dim" }, "TLS and sign-in"), h("div", { class: "stack", style: { gap: "8px", marginTop: "8px" } },
        toggle(tls, (x) => { tls = x; }, "Connect with TLS (checked against the public roots)"),
        f("SASL", mech), user, pass)),
      h("details", {}, h("summary", { class: "small dim" }, "More"), h("div", { class: "stack", style: { gap: "8px", marginTop: "8px" } },
        f("Source names' prefix", prefix, "each partition is the stream source <prefix><topic>/<partition>; another prefix reads the topics again from the start"),
        f("An event without a value (a tombstone)", tomb))),
      toggle(startNow, (x) => { startNow = x; }, "Start it now"),
      note, h("div", { class: "row", style: { gap: "6px" } }, tryIt, go)) });
  }
  const kindTitle = (k) => (v?.kinds || []).find((x) => x.kind === k)?.title || k;

  // ----- Postgres followers (chloedb-server follow-postgres): each one's
  // state, lag and positions, as it tells this server every 2 s (ROSA-302).
  // A follower that has marks here but says nothing is shown too: it may
  // be down, older, or refused (its log says why).
  function pgNames() {
    const said = new Set((v.followers || []).map((f) => f.name));
    const marked = new Set((v.sources || []).map((s) => /^postgres\/([^/]+)\/copy$/.exec(s.name)?.[1]).filter(Boolean));
    return { said, all: [...new Set([...said, ...marked])].sort() };
  }
  const PG_STATE = { following: ["ok", "following"], copying: ["info", "copying"], starting: ["info", "starting"], retrying: ["failing", "retrying"], failed: ["failing", "failed"], stopped: ["disabled", "stopped"] };
  function postgresCard() {
    const { all } = pgNames();
    if (!all.length) return null;
    const marksOf = (n) => (v.sources || []).filter((s) => s.name.startsWith(`postgres/${n}/`));
    const one = (n) => {
      const f = (v.followers || []).find((x) => x.name === n);
      const marks = marksOf(n);
      const applied = marks.filter((s) => s.lsn && !s.name.endsWith("/copied")).sort((a, b) => b.offset - a.offset)[0];
      if (!f) {
        return h("div", { class: "stack", style: { gap: "4px" } },
          h("div", { class: "row", style: { gap: "8px", flexWrap: "wrap" } }, h("b", {}, n), pill("warn", "no report")),
          h("div", { class: "small dim" }, `This server has heard nothing from follower ${n} since it started: it is down, or an older follower, or its key may not report (its log says so). Last applied${applied ? ` ${applied.lsn}, ${applied.updated ? fmt.ago(Date.parse(applied.updated)) : "–"}` : ": nothing yet"}.`));
      }
      const [k, text] = f.silent && f.state !== "stopped" && f.state !== "failed" ? ["failing", `silent ${fmt.dur(f.age_ms / 1000)}`] : PG_STATE[f.state] || ["info", f.state];
      const lag = f.lag_s ?? (f.lag_ms || 0) / 1000;
      const kv = (label, val, title) => h("div", { class: "stack", style: { gap: "1px" }, title: title || null }, h("span", { class: "tiny dim" }, label), h("span", { class: "mono" }, val ?? "–"));
      return h("div", { class: "stack", style: { gap: "6px" } },
        h("div", { class: "row", style: { gap: "8px", flexWrap: "wrap", alignItems: "center" } }, h("b", {}, n), pill(k, text),
          f.copying ? h("span", { class: "small dim" }, `copying ${f.copying}`) : null,
          h("span", { class: "small dim" }, `${f.source || ""}${f.publication ? ` · publication ${f.publication}` : ""}${f.slot ? ` · slot ${f.slot}` : ""}`)),
        f.state === "stopped" ? h("div", { class: "small dim" }, f.why || "It stopped following.") : h("div", { class: "row", style: { gap: "18px", flexWrap: "wrap" } },
          kv("lag", lag >= 1 ? fmt.dur(lag) : `${fmt.n(lag, 1)} s`, "Since Postgres's WAL first reached a position the follower has not confirmed; while it is silent, plus how long. The alert follow-behind fires when this stays over a minute for a minute."),
          kv("behind", f.behind_bytes == null ? "–" : fmt.bytes(f.behind_bytes), "Postgres's WAL position less the follower's confirmed one: pg_wal_lsn_diff(pg_current_wal_lsn(), confirmed_flush_lsn) on Postgres reads about the same"),
          kv("applied", f.applied_lsn, "The end of the last Postgres transaction written to ChloeDB"),
          kv("confirmed", f.confirmed_lsn, "What the follower told Postgres it has (past transactions of other tables too): the slot keeps the WAL after it"),
          kv("Postgres at", f.postgres_lsn, f.postgres_error ? `Not asked: ${f.postgres_error}` : "Postgres's own WAL position (pg_current_wal_lsn()), as the follower last asked"),
          kv("tables", fmt.n(f.tables || 0)), kv("transactions", fmt.si(f.transactions || 0), "Applied since the follower started"),
          kv("last report", fmt.ago(Date.now() - (f.age_ms || 0)))),
        f.error ? h("div", { class: "small error", style: { overflowWrap: "anywhere" } }, f.error) : f.last_error ? h("div", { class: "small dim", style: { overflowWrap: "anywhere" } }, `Last error ${fmt.ago(f.last_error.at_ms)}: ${f.last_error.message}`) : null,
        f.postgres_error && !f.error ? h("div", { class: "small warn" }, `Postgres's position could not be asked: ${f.postgres_error}`) : null,
        marks.length ? h("details", {}, h("summary", { class: "small dim" }, `Its marks in _sources (${marks.length})`),
          h("div", { class: "tbl-wrap" }, h("table", { class: "t compact" }, h("tbody", {}, marks.slice(0, 60).map((s) => h("tr", {}, h("td", { class: "mono small" }, s.name), h("td", { class: "num mono small" }, s.lsn || fmt.n(s.offset)), h("td", { class: "small" }, s.updated ? fmt.ago(Date.parse(s.updated)) : "–"))))))) : null);
    };
    return card({
      title: "Postgres followers", sub: "chloedb-server follow-postgres, as each says every 2 s",
      body: h("div", { class: "stack small", style: { gap: "14px" } }, all.map(one),
        h("div", { class: "dim tiny" }, "Its lag is in /metrics (chloedb_follow_lag_seconds, chloedb_follow_behind_bytes, chloedb_follow_up) and the built-in alert follow-behind (\"A follower is behind its source\"), which a follower that stops reporting fires too.")),
    });
  }

  function sourcesCard() {
    // A Postgres follower's marks are on its card.
    const { all } = pgNames();
    const ss = (v.sources || []).filter((s) => !all.some((n) => s.name.startsWith(`postgres/${n}/`)));
    return card({
      title: "Other ways in", sub: "streams and followed databases, by their last offset",
      body: h("div", { class: "stack small", style: { gap: "8px" } },
        ss.length ? h("div", { class: "tbl-wrap" }, h("table", { class: "t compact" }, h("thead", {}, h("tr", {}, h("th", {}, "source"), h("th", { class: "num" }, "offset"), h("th", {}, "last event"))),
          h("tbody", {}, ss.slice(0, 50).map((s) => h("tr", {}, h("td", { class: "mono" }, s.name), h("td", { class: "num mono" }, s.lsn || fmt.n(s.offset)), h("td", {}, s.updated ? fmt.ago(Date.parse(s.updated)) : "–"))))))
          : h("div", { class: "dim" }, all.length ? "No other stream sources." : "No stream sources yet."),
        h("div", { class: "dim" }, "OpenTelemetry logs: ", h("span", { class: "mono" }, "POST /v1/logs"), " (the Collector's otlphttp exporter). Kafka and other streams: ", h("span", { class: "mono" }, "POST /v1/stream"), " with offsets, applied once. Postgres: ", h("span", { class: "mono" }, "chloedb-server follow-postgres"), ". Files: Import and export.")),
    });
  }

  function addCard() {
    const kinds = v.kinds || [];
    const kindSel = h("select", { class: "select", "aria-label": "Kind" }, kinds.map((k) => h("option", { value: k.kind }, k.title)));
    const name = h("input", { class: "input mono", placeholder: "jira-changes", autocomplete: "off", spellcheck: "false" });
    const target = h("input", { class: "input mono", autocomplete: "off", spellcheck: "false" });
    const everyIn = h("input", { class: "input mono", type: "number", min: "10", style: { width: "110px" } });
    const conf = h("textarea", { class: "input mono", rows: 9, spellcheck: "false", style: { width: "100%", resize: "vertical" } });
    const secrets = h("textarea", { class: "input mono", rows: 3, spellcheck: "false", "data-secret": "secrets", style: { width: "100%", resize: "vertical" } });
    const about = h("div", { class: "small dim" });
    const note = h("div");
    const fill = () => {
      const k = kinds.find((x) => x.kind === kindSel.value);
      const t = TEMPLATES[k.kind] || { config: {}, secrets: {} };
      target.placeholder = k.target;
      everyIn.value = k.every_s || "";
      everyIn.disabled = !k.pull;
      conf.value = pretty(t.config);
      secrets.value = pretty(t.secrets);
      clear(about, k.about, h("ul", { style: { margin: "6px 0 0", paddingLeft: "18px" } }, k.settings.map((s) => h("li", {}, h("span", { class: "mono" }, s.name), `: ${s.about}`))));
    };
    kindSel.onchange = fill;
    if (kinds.length) fill();
    const body = () => {
      let config, sec;
      try { config = JSON.parse(conf.value || "{}"); } catch (e) { throw new Error(`config is not JSON: ${e.message}`); }
      try { sec = JSON.parse(secrets.value || "{}"); } catch (e) { throw new Error(`secrets are not JSON: ${e.message}`); }
      for (const [k, x] of Object.entries(sec)) if (x === "") delete sec[k];
      const b = { name: name.value.trim(), kind: kindSel.value, config, secrets: sec };
      if (target.value.trim()) b.target = target.value.trim();
      if (everyIn.value && !everyIn.disabled) b.every_s = Number(everyIn.value);
      return b;
    };
    const tryIt = h("button", { class: "btn" }, icon("play"), "Dry run");
    const go = h("button", { class: "btn primary" }, icon("plus"), "Add the connector");
    if (!can("admin")) { go.disabled = tryIt.disabled = true; go.title = tryIt.title = needs("admin"); }
    tryIt.onclick = async () => {
      clear(note, h("div", { class: "small dim" }, "Reading…"));
      try {
        const b = body();
        const k = kinds.find((x) => x.kind === b.kind);
        if (!k.pull) {
          const sample = await sampleFor(b.kind);
          if (sample == null) { clear(note); return; }
          b.sample = sample;
        }
        const t = await api("/v1/admin/connectors/test", { body: b, timeout: 120000 });
        clear(note, dryRun(t));
      } catch (e) { clear(note, h("div", { class: "error small" }, e.message)); }
    };
    go.onclick = async () => {
      go.disabled = true;
      try {
        const r = await api("/v1/admin/connectors", { body: body(), timeout: 60000 });
        toast(`${r.connector.name} added${r.made_target ? `, and its table ${r.connector.target}` : ""}`, "ok");
        if (r.push_secret) showKey(r.connector, r.push_secret);
        else if (r.push_key_error) toast(r.push_key_error, "info", 6000);
        name.value = "";
        clear(note);
        await load();
      } catch (e) { clear(note, h("div", { class: "error small" }, e.message)); }
      go.disabled = false;
    };
    const field = (label, el) => h("label", { class: "stack", style: { gap: "4px" } }, h("span", { class: "small dim" }, label), el);
    return card({ title: "Add a connector", body: h("div", { class: "stack", style: { gap: "10px" } },
      field("Kind", kindSel), about,
      field("Name (lowercase, digits, - and _)", name),
      field("Target table (empty: the kind's own, made for it)", target),
      field("Pull every (seconds)", everyIn),
      field("Config (JSON; map: column → \"$.path\", \"{template}\" or a value)", conf),
      field("Secrets (JSON; kept on this server, never in a table; \"env:VAR\" reads one from its environment)", secrets),
      note, h("div", { class: "row", style: { gap: "6px" } }, tryIt, go)) });
  }

  /** What a push to try a pushed kind with: asks for it. */
  function sampleFor(kind) {
    return new Promise((resolve) => {
      const ta = h("textarea", { class: "input mono", rows: 12, spellcheck: "false", style: { width: "100%" } });
      ta.value = kind === "mail" ? "From: NOC <noc@carrier.example>\nSubject: Planned maintenance\nDate: Fri, 13 Nov 2026 03:00:00 +0000\n\nWork on circuit EXT-1001 on 20 Nov 06:00-08:00 UTC.\n"
        : pretty({ alerts: [{ status: "firing", labels: { alertname: "BGPDown", severity: "critical", instance: "r1" }, annotations: { summary: "BGP session down" }, startsAt: new Date().toISOString(), fingerprint: "abc" }] });
      let done = false;
      const ok = h("button", { class: "btn primary" }, "Try it");
      const m = modal({ title: kind === "mail" ? "A raw email to try" : "A body a system would post", body: ta, foot: [ok], wide: true, onClose: () => { if (!done) resolve(null); } });
      ok.onclick = () => { done = true; m.close(); let x = ta.value; if (kind !== "mail") { try { x = JSON.parse(x); } catch { /* sent as text */ } } resolve(x); };
    });
  }

  function dryRun(t) {
    const cols = [...new Set((t.rows || []).flatMap((r) => Object.keys(r)))];
    return h("div", { class: "stack", style: { gap: "6px" } },
      h("div", { class: "small" }, h("b", {}, fmt.plural(t.records, "record")), ` read in ${took(t.took_ms)}`, t.target_exists ? ` · rows fitted to ${t.target}` : ` · ${t.target} does not exist yet: it is made with the connector`, t.more ? " · more to read" : ""),
      t.warning ? h("div", { class: "small warn" }, icon("warn"), " ", t.warning) : null,
      t.dropped?.length ? h("div", { class: "small warn" }, icon("warn"), ` not columns of ${t.target}, left out: `, h("span", { class: "mono" }, t.dropped.join(", "))) : null,
      (t.errors || []).length ? h("div", { class: "small error" }, t.errors.slice(0, 3).join(" · ")) : null,
      cols.length ? h("div", { class: "tbl-wrap", style: { maxHeight: "260px" } }, h("table", { class: "t compact" }, h("thead", {}, h("tr", {}, cols.map((c) => h("th", {}, c)))),
        h("tbody", {}, t.rows.map((r) => h("tr", {}, cols.map((c) => h("td", { class: "mono small", style: { maxWidth: "240px", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }, title: typeof r[c] === "string" ? r[c] : JSON.stringify(r[c]) }, r[c] == null ? "" : typeof r[c] === "string" ? r[c] : JSON.stringify(r[c])))))))) : null,
      t.sample_records?.length ? h("details", {}, h("summary", { class: "small dim" }, "The records as read"), h("pre", { class: "code small", style: { maxHeight: "240px", overflow: "auto" } }, pretty(t.sample_records))) : null);
  }

  function showKey(c, secret) {
    const url = `${location.origin}/v1/connectors/${c.name}/push`;
    const curl = c.kind === "mail" ? `curl -sS -X POST ${url} \\\n  -H "Authorization: Bearer ${secret}" \\\n  -H "Content-Type: message/rfc822" --data-binary @-`
      : c.kind === "oxidized" ? `# Oxidized's config, hooks:\nhooks:\n  chloedb:\n    type: exec\n    events: [post_store]\n    cmd: 'curl -sS -X POST ${url} -H "Authorization: Bearer ${secret}" -H "Content-Type: application/json" -d "{\\"node\\":\\"$OX_NODE_NAME\\",\\"group\\":\\"$OX_NODE_GROUP\\"}"'`
      : c.config?.preset === "alertmanager" ? `# alertmanager.yml\nreceivers:\n  - name: chloedb\n    webhook_configs:\n      - url: ${url}\n        http_config:\n          authorization:\n            credentials: ${secret}`
      : `curl -sS -X POST ${url} \\\n  -H "Authorization: Bearer ${secret}" \\\n  -H "Content-Type: application/json" -d '{"id": "1", "msg": "hello"}'`;
    modal({ title: `The push key of ${c.name}`, wide: true, body: h("div", { class: "stack", style: { gap: "10px" } },
      h("div", { class: "small" }, "Shown once. It may write ", h("b", { class: "mono" }, c.target), " and nothing else; revoke or rotate it here or in Access and audit."),
      h("div", { class: "row", style: { gap: "6px" } }, h("span", { class: "mono small", style: { wordBreak: "break-all" }, "data-secret": "key" }, secret), h("button", { class: "btn sm", onclick: () => copy(secret, "Key copied") }, icon("copy"), "Copy")),
      h("pre", { class: "code small", "data-secret": "key" }, curl), h("button", { class: "btn sm", onclick: () => copy(curl, "Copied") }, icon("copy"), "Copy the snippet")) });
  }

  async function open(name) {
    let c;
    try { c = await api(`/v1/admin/connectors/${encodeURIComponent(name)}`); } catch (e) { toast(e.message, "bad"); return; }
    const out = h("div");
    const runBtn = !c.streams && (c.pulls || (v.kinds || []).find((k) => k.kind === c.kind)?.pull) ? h("button", { class: "btn sm primary" }, icon("play"), "Run now") : null;
    if (runBtn) runBtn.onclick = async () => {
      runBtn.disabled = true;
      clear(out, h("div", { class: "small dim" }, "Running…"));
      try { const r = await api(`/v1/admin/connectors/${name}/run`, { body: {}, timeout: 600000 }); clear(out, runReport(r)); load(); }
      catch (e) { clear(out, h("div", { class: "error small" }, e.message)); }
      runBtn.disabled = false;
    };
    const dry = h("button", { class: "btn sm" }, icon("eye"), "Dry run");
    dry.onclick = async () => {
      clear(out, h("div", { class: "small dim" }, "Reading…"));
      try {
        const b = { name };
        if (!c.pulls && !(v.kinds || []).find((k) => k.kind === c.kind)?.pull) { const s = await sampleFor(c.kind); if (s == null) { clear(out); return; } b.sample = s; }
        clear(out, dryRun(await api("/v1/admin/connectors/test", { body: b, timeout: 120000 })));
      } catch (e) { clear(out, h("div", { class: "error small" }, e.message)); }
    };
    const toggleBtn = c.streams ? h("button", { class: "btn sm" + (c.enabled ? "" : " primary") }, icon(c.enabled ? "stop" : "play"), c.enabled ? "Stop" : "Start")
      : h("button", { class: "btn sm" }, icon(c.enabled ? "pause" : "play"), c.enabled ? "Turn off" : "Turn on");
    toggleBtn.onclick = async () => {
      try {
        if (c.streams) await api(`/v1/admin/connectors/${name}/${c.enabled ? "stop" : "start"}`, { body: {} });
        else await api(`/v1/admin/connectors/${name}`, { body: { enabled: !c.enabled } });
        d.close(); load();
      } catch (e) { toast(e.message, "bad"); }
    };
    const keyBtn = c.pushed ? h("button", { class: "btn sm" }, icon("key"), c.push?.key_id ? "Rotate the push key" : "Make a push key") : null;
    if (keyBtn) keyBtn.onclick = async () => {
      if (!(await confirm({ title: "A new push key?", body: "The old key stops working now: the system that pushes needs the new one.", ok: "Make a new key" }))) return;
      try { const r = await api(`/v1/admin/connectors/${name}/key`, { body: {} }); showKey(c, r.push_secret); } catch (e) { toast(e.message, "bad"); }
    };
    const del = h("button", { class: "btn sm danger" }, icon("trash"), "Delete");
    del.onclick = async () => {
      if (!(await confirm({ title: `Delete ${name}?`, body: `Its config, cursor, secrets and push key go; the rows it wrote stay in ${c.target}, and its runs for their 30 days.`, ok: "Delete", danger: true, typed: name }))) return;
      try { await api(`/v1/admin/connectors/${name}`, { method: "DELETE" }); d.close(); toast(`${name} deleted`, "ok"); load(); } catch (e) { toast(e.message, "bad"); }
    };
    const editConf = h("textarea", { class: "input mono", rows: 10, spellcheck: "false", style: { width: "100%" } });
    editConf.value = pretty(c.config);
    const save = h("button", { class: "btn sm" }, icon("check"), "Save the config");
    save.onclick = async () => {
      let config; try { config = JSON.parse(editConf.value); } catch (e) { toast(`not JSON: ${e.message}`, "bad"); return; }
      try { await api(`/v1/admin/connectors/${name}`, { body: { config } }); toast("Saved", "ok"); load(); } catch (e) { toast(e.message, "bad"); }
    };
    const st = c.state || {};
    const runs = (c.runs || []).map((r) => h("tr", {},
      h("td", { class: "small", style: { whiteSpace: "nowrap" } }, r.started ? fmt.datetime(Date.parse(r.started)) : "–"),
      h("td", { class: "small" }, r.trigger),
      h("td", {}, !r.ok ? pill("failing", "failed") : r.warning ? pill("warn", "warning") : pill("ok", "ok")),
      h("td", { class: "num small" }, fmt.n(r.records || 0)), h("td", { class: "num small" }, fmt.n(r.rows_ || 0)),
      h("td", { class: "num small" }, took(r.took_ms)),
      h("td", { class: r.error ? "small error" : r.warning ? "small warn" : "small", style: { maxWidth: "300px", overflowWrap: "anywhere" } }, r.error || r.warning || (r.dropped?.length ? h("span", { class: "dim" }, `left out: ${r.dropped.join(", ")}`) : ""))));
    const sx = c.stream || {};
    const facts = c.streams ? [
      ["Kind", kindTitle(c.kind)], ["Target", h("span", { class: "mono" }, tableLink(c.target))], ["Consumer", streamPill(c)],
      ["Brokers", h("span", { class: "mono small" }, String(c.config?.brokers || ""))], ["Topics", h("span", { class: "mono small" }, Array.isArray(c.config?.topics) ? c.config.topics.join(", ") : String(c.config?.topics || ""))],
      ["Sources", h("span", { class: "mono small" }, (sx.sources || []).join(", "))],
      ["Rate", `${rateText(sx.rate_10s)} (last 10 s) · ${rateText(sx.rate_60s)} (last minute)`], ["Lag", sx.lag != null ? `${fmt.n(sx.lag)} events` : "–"],
      ["Since start", `${fmt.n(sx.events || 0)} events, ${fmt.n(sx.rows || 0)} rows, ${fmt.n(sx.replays || 0)} replays skipped, ${fmt.n(sx.unmapped || 0)} made no row${sx.lost ? `, ${fmt.n(sx.lost)} lost to retention` : ""}`],
      ["Sessions", `${fmt.n(sx.sessions || 0)}${sx.session_ms ? ` · this one since ${fmt.ago(sx.session_ms)}` : ""}`],
      ["Rows written", `${fmt.n(st.rows_total || 0)} in all`],
      ["Secrets", c.secrets?.length ? h("span", { class: "mono" }, c.secrets.join(", ")) : "none"],
      ["Changed", c.updated_ms ? `${fmt.ago(c.updated_ms)}${c.updated_by ? ` by ${c.updated_by}` : ""}` : "–"],
      sx.first_unmapped ? ["First without a row", h("span", { class: "small warn" }, sx.first_unmapped)] : null,
    ].filter(Boolean) : [
      ["Kind", kindTitle(c.kind)], ["Target", h("span", { class: "mono" }, tableLink(c.target))], ["Health", healthPill(c)], ["Schedule", every_text(c, v.kinds)],
      ["Runs", fmt.n(st.runs || 0)], ["Rows written", fmt.n(st.rows_total || 0)], ["Last ok", st.last_ok_ms ? fmt.ago(st.last_ok_ms) : "never"],
      ["Cursor", h("span", { class: "mono small", style: { wordBreak: "break-all" } }, (st.cursor || "–").slice(0, 120))],
      ["Secrets", c.secrets?.length ? h("span", { class: "mono" }, c.secrets.join(", ")) : "none"],
      ["Changed", c.updated_ms ? `${fmt.ago(c.updated_ms)}${c.updated_by ? ` by ${c.updated_by}` : ""}` : "–"],
      c.push ? ["Push to", h("span", { class: "mono small" }, `POST ${c.push.url}`)] : null,
    ].filter(Boolean);
    const d = drawer({ title: name, sub: `${kindTitle(c.kind)} into ${c.target}`, wide: true, body: h("div", { class: "stack", style: { gap: "14px" } },
      h("div", { class: "row", style: { gap: "6px", flexWrap: "wrap" } }, runBtn, dry, toggleBtn, keyBtn, del),
      c.invalid ? h("div", { class: "banner bad dt-banner" }, icon("bad"), h("div", { class: "small" }, `This config does not run: ${c.invalid}. Change it here, or delete it.`)) : null,
      st.last_error ? h("div", { class: "banner bad dt-banner" }, icon("bad"), h("div", { class: "small" }, st.last_error)) : null,
      !st.last_error && st.last_warning ? h("div", { class: "banner warn dt-banner" }, icon("warn"), h("div", { class: "small" }, `The last run went through, but: ${st.last_warning}`)) : null,
      out,
      h("table", { class: "t compact" }, h("tbody", {}, facts.map(([k, x]) => h("tr", {}, h("td", { class: "dim small", style: { width: "130px" } }, k), h("td", {}, x))))),
      card({ title: "Last runs", flush: true, body: runs.length ? h("div", { class: "tbl-wrap" }, h("table", { class: "t compact" }, h("thead", {}, h("tr", {}, ["started", "trigger", "result", "records", "rows", "took", "error"].map((x) => h("th", {}, x)))), h("tbody", {}, runs))) : empty("clock", "No runs yet") }),
      card({ title: "Config", body: h("div", { class: "stack", style: { gap: "6px" } }, editConf, h("div", { class: "row" }, save)) }),
      c.map ? card({ title: "Map in use", sub: c.config?.map ? "its own" : "the kind's or a preset for its target", body: h("pre", { class: "code small", style: { maxHeight: "260px", overflow: "auto" } }, pretty(c.map)) }) : null) });
  }

  function runReport(r) {
    return h("div", { class: "stack small", style: { gap: "4px" } },
      r.ok ? h("div", {}, pill("ok", "ok"), ` ${fmt.plural(r.records, "record")} read, ${fmt.plural(r.rows, "row")} written in ${took(r.took_ms)}`, r.more ? " · more to read: it runs again at once" : "")
        : h("div", { class: "error" }, r.error),
      r.warning ? h("div", { class: "warn" }, icon("warn"), " ", r.warning) : null,
      r.dropped?.length ? h("div", { class: "warn" }, "Left out (not columns of the target): ", h("span", { class: "mono" }, r.dropped.join(", "))) : null,
      r.skipped ? h("div", { class: "warn" }, `${fmt.plural(r.skipped, "record")} made no row: ${r.first_skip || ""}`) : null);
  }

  load();
  // Every 15 s; every 3 s while a Kafka consumer or the test broker runs.
  const stop = every(3000, () => {
    if (!page.isConnected) { stop(); return; }
    const live = broker?.running || (v?.connectors || []).some((c) => c.streams && c.enabled);
    if (live || Date.now() - loaded >= 14500) return load(true);
  }, false);
}
