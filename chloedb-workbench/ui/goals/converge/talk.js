// "Talk to the agent": the thread above, the box at the sheet's foot.
//   GET  <base>/talk -> { messages: [{id, ts, who, text, agent}], agent: {id, name, state, doing} | null }
//   POST <base>/talk {text} -> { message, agent, started }
// <base> is /api/converge/findings/:id (a card) or /api/converge/runs/:id
// (a persona's run). A server without it says so, and the box waits.
import { h, put, toast, toMs, doingWords } from "/goals/converge/util.js";
import { autoGrow } from "/goals/converge/kit.js";

const KIND_WORD = { repro: "Repro agent", fix: "Fix agent", scope: "Planning agent", card: "Card agent" };
const clock = (t) => { const ms = toMs(t); return ms ? new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : ""; };

/** `o`: { api, base, placeholder, intro, agentWord, onSent } */
export function talk(o) {
  const T = { msgs: [], pending: [], agent: null, missing: false, sig: "", busy: false, alive: true };
  const thread = h("div", { class: "cv-thread", "aria-live": "polite" });
  const status = h("div", { class: "cv-agent" });
  const ta = h("textarea", { rows: "1", placeholder: o.placeholder || "Talk to the agent", "aria-label": o.placeholder || "Talk to the agent", enterkeyhint: "send" });
  const fit = autoGrow(ta);
  const send = h("button", { class: "cv-btn pri", type: "button", onclick: () => go() }, "Send");
  ta.addEventListener("keydown", (e) => { if (e.key === "Enter" && !e.shiftKey && !e.isComposing && window.innerWidth > 640) { e.preventDefault(); go(); } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) { e.preventDefault(); go(); } });
  const box = h("div", { class: "cv-tbox" }, ta, send);
  const section = h("section", { class: "cv-talk", "aria-label": "Talk to the agent" }, h("span", { class: "cv-label" }, "Talk to the agent"), status, thread);

  function paint() {
    const a = T.agent;
    const on = a && ["starting", "working", "waiting"].includes(a.state);
    put(status, T.missing ? h("span", { class: "cv-tnote" }, o.missingText || "This server does not take notes yet; it comes with the next build.")
      : a ? [h("span", { class: on ? "cv-live" : null }), h("span", { class: "s" }, `${KIND_WORD[a.kind] || (a.name && !/^[a-z]+-[0-9a-f]{6,}$/.test(a.name) ? a.name : null) || o.agentWord || "Agent"} · ${on ? doingWords(a.doing) || a.state : a.state === "exited" || !a.state ? "finished" : a.state}`)]
        : h("span", { class: "cv-tnote" }, o.intro || "Ask a question or give a note; an agent picks it up."));
    status.classList.toggle("on", !!on);
    const all = [...T.msgs, ...T.pending.filter((p) => !T.msgs.some((m) => m.who === "you" && m.text === p.text))];
    put(thread, all.map((m) => {
      const you = m.who === "you";
      return h("div", { class: `cv-msg ${you ? "m-you" : "m-ag"}${m.pending ? " m-pend" : ""}` },
        h("span", { class: "m-w" }, you ? `You${m.ts ? " · " + clock(m.ts) : ""}` : `${!m.who || /^[a-z]+-[0-9a-f]{6,}$/.test(m.who) ? KIND_WORD[T.agent && T.agent.kind] || o.agentWord || "Agent" : m.who}${m.ts ? " · " + clock(m.ts) : ""}`), m.text);
    }));
    send.disabled = T.busy || T.missing ? true : null;
    box.hidden = T.missing;
  }

  async function refresh() {
    if (!T.alive || T.missing) return;
    try {
      const d = await o.api(`${o.base}/talk`);
      const sig = JSON.stringify([d.messages, d.agent]);
      if (sig === T.sig) return;
      T.sig = sig;
      T.msgs = Array.isArray(d.messages) ? d.messages : [];
      T.agent = d.agent && typeof d.agent === "object" ? d.agent : null;
      T.pending = T.pending.filter((p) => !T.msgs.some((m) => m.who === "you" && m.text === p.text));
    } catch (err) {
      if (err.status === 404 || err.status === 405) T.missing = true;
    }
    if (T.alive) paint();
  }

  async function go() {
    const text = ta.value.trim();
    if (!text || T.busy) return;
    T.busy = true;
    const p = { who: "you", text, ts: Date.now(), pending: true };
    T.pending.push(p);
    ta.value = ""; fit();
    paint();
    try {
      const r = await o.api(`${o.base}/talk`, { method: "POST", body: { text } });
      if (r && r.agent && typeof r.agent === "object") T.agent = r.agent;
      if (r && r.started) toast(`${(r.agent && r.agent.name) || "An agent"} is on it`);
      if (o.onSent) o.onSent(r);
    } catch (err) {
      T.pending = T.pending.filter((x) => x !== p);
      ta.value = text; fit();
      if (err.status === 404 || err.status === 405) { T.missing = true; toast(o.missingText || "This server does not take notes yet"); }
      else if (err.status === 409) toast("Checking the repro: try again in a moment");
      else toast(`Not sent: ${err.message || err}`);
    }
    T.busy = false;
    T.sig = "";
    await refresh();
    thread.lastElementChild?.scrollIntoView({ block: "nearest" });
  }

  paint();
  refresh();
  return { section, box, refresh, focus: (text) => { if (text != null) ta.value = text; fit(); ta.focus(); }, stop: () => { T.alive = false; } };
}
