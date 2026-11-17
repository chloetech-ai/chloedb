// A run's steps as compact tasks (Roberto: "compact and summarized like
// tasks that you can unfold"): a `step` starts a task; what comes before
// the first is "Getting started". Each task is one line, folded; the
// running one is open. Painted in place, so a refresh keeps what is open.
import { h, put, toMs, secsWords, firstSentences } from "/goals/converge/util.js";
import { useStyle } from "/levels/common.js";

const CSS = `
.cv-iv { display: flex; flex-direction: column; gap: 10px; margin-top: 8px; }
.cv-iv-msg { display: flex; gap: 8px; align-items: flex-end; max-width: 92%; }
.cv-iv-msg .av { flex: none; width: 28px; height: 28px; border-radius: 50%; display: grid; place-items: center; background: var(--accent); color: var(--surface); font-weight: var(--w-title); font-size: var(--fs-sm); }
.cv-iv-msg .bub { background: var(--surface-2); border-radius: 14px 14px 14px 4px; padding: 8px 12px; line-height: var(--lh-sm); white-space: pre-wrap; overflow-wrap: anywhere; }
.cv-iv-msg .bub .ts { display: block; margin-top: 4px; font-size: var(--fs-xs); color: var(--muted); }
.cv-iv-msg.think .bub { background: none; border: var(--hair) dashed var(--line-2); color: var(--text-2); font-style: italic; }
.cv-iv-msg .bub .lbl { display: block; font-style: normal; font-size: var(--fs-xs); text-transform: uppercase; letter-spacing: .05em; color: var(--muted); margin-bottom: 2px; }
.cv-iv-acts { margin-left: 36px; border-left: 2px solid var(--line); padding-left: 8px; }
.cv-iv-acts summary { min-height: 32px; display: flex; align-items: center; gap: 6px; font-size: var(--fs-xs); color: var(--muted); cursor: pointer; list-style: none; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.cv-iv-acts summary::-webkit-details-marker { display: none; }
.cv-iv-acts summary::before { content: "▸"; font-size: 10px; }
.cv-iv-acts[open] summary::before { content: "▾"; }
.cv-iv-list { display: flex; flex-direction: column; gap: 2px; padding-bottom: 4px; }
.cv-iv-act { font-size: var(--fs-xs); color: var(--text-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.cv-iv-act .w { text-transform: uppercase; letter-spacing: .04em; }
.cv-iv-act code { font-family: var(--mono); }
.cv-iv-step { display: flex; align-items: center; gap: 8px; font-size: var(--fs-xs); color: var(--accent-text); text-transform: uppercase; letter-spacing: .05em; margin: 6px 0; }
.cv-iv-step::before, .cv-iv-step::after { content: ""; flex: 1; border-top: var(--hair) solid var(--accent-line); }
.cv-iv-typing { display: flex; gap: 4px; margin: 10px 0 0 36px; }
.cv-iv-typing[hidden] { display: none; }
.cv-iv-typing i { width: 7px; height: 7px; border-radius: 50%; background: var(--muted); animation: cv-pulse 1.2s infinite; }
.cv-iv-typing i:nth-child(2) { animation-delay: .2s; } .cv-iv-typing i:nth-child(3) { animation-delay: .4s; }
/* Tasks (a run) */
.cv-tasks { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
.cv-task { border: var(--hair) solid var(--line); border-radius: 8px; background: var(--surface); }
.cv-task[data-state="run"] { border-color: var(--accent-line); }
.cv-task[data-state="fail"] { border-color: var(--crit); }
.cv-task-h, .cv-wr-h, .cv-ev-h { width: 100%; min-height: 44px; display: grid; align-items: center; gap: 10px; padding: 6px 10px; border: 0; background: none; color: var(--text); font: inherit; text-align: left; cursor: pointer; }
.cv-task-h { grid-template-columns: 22px minmax(0, 1fr) auto 12px; }
.cv-task-h .tx, .cv-wr-h .tx { display: flex; flex-direction: column; min-width: 0; }
.cv-task-h .tt, .cv-wr-h .tt { font-weight: var(--w-mid); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cv-task-h .ts { font-size: var(--fs-xs); color: var(--text-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cv-task-h .du, .cv-wr-h .du { font-family: var(--mono); font-size: var(--fs-xs); color: var(--muted); white-space: nowrap; }
.cv-car { color: var(--muted); font-size: 10px; }
.cv-tasks .mk { width: 20px; height: 20px; border-radius: 50%; display: grid; place-items: center; font-size: 11px; color: #fff; background: var(--good); flex: none; }
.cv-tasks .mk.run { background: var(--accent-soft); color: var(--accent-text); }
.cv-tasks .mk.fail { background: var(--crit); }
.cv-task-b { padding: 4px 10px 12px; margin-top: 0; border-top: var(--hair) solid var(--line); }
.cv-task-b[hidden] { display: none; }
.cv-iv-msg .bub { font-size: var(--fs-body); }
`;
export const TOOL_WORDS = { Bash: "ran", Read: "read", Write: "wrote", Edit: "edited", MultiEdit: "edited", Glob: "looked for", Grep: "searched for", WebFetch: "opened", WebSearch: "searched the web for", TodoWrite: "planned" };
const NOUN = { Bash: ["ran", "command"], Read: ["read", "file"], Write: ["wrote", "file"], Edit: ["edited", "file"], MultiEdit: ["edited", "file"], Glob: ["looked for files", "time"], Grep: ["searched", "time"], WebFetch: ["opened", "page"], WebSearch: ["searched the web", "time"], TodoWrite: ["planned", "time"] };
/** One line for a run of actions: "Read 9 files · ran 2 commands". */
export function actsSummary(n) {
  const merged = {};
  for (const [tool, c] of Object.entries(n)) {
    const [verb, noun] = NOUN[tool] || [`used ${tool}`, "time"];
    const k = `${verb}|${noun}`;
    merged[k] = (merged[k] || 0) + c;
  }
  const parts = Object.entries(merged).sort((a, b) => b[1] - a[1]).map(([k, c]) => {
    const [verb, noun] = k.split("|");
    return noun === "time" ? (c > 1 ? `${verb} ${c} times` : verb) : `${verb} ${c} ${noun}${c > 1 ? "s" : ""}`;
  });
  const t = parts.join(" · ");
  return t.charAt(0).toUpperCase() + t.slice(1);
}
export function taskStyles() { useStyle("converge-tasks", CSS); }

export function groupTasks(steps) {
  const tasks = [{ key: "start", title: "Getting started", ts: toMs(steps[0]?.ts), items: [] }];
  steps.forEach((x, i) => {
    if (x.kind === "step") tasks.push({ key: `s${i}`, title: x.text || `Step ${tasks.length}`, ts: toMs(x.ts), items: [] });
    else tasks.at(-1).items.push(x);
  });
  if (tasks.length > 1 && !tasks[0].items.length) tasks.shift();
  return tasks;
}
/** A task's state, from where it is in the run. */
export function taskStates(tasks, o) {
  return tasks.map((t, i) => {
    const last = i === tasks.length - 1;
    const state = last && o.live ? "run" : last && o.failed ? "fail" : "done";
    const end = last ? (o.live ? o.now : o.ended) : tasks[i + 1].ts;
    const said = t.items.filter((x) => x.kind !== "tool" && x.kind !== "think");
    const tools = t.items.filter((x) => x.kind === "tool");
    const lastTool = tools.at(-1);
    const sum = said.length ? firstSentences(said.at(-1).text, 1)
      : state === "run" ? (lastTool ? `${TOOL_WORDS[lastTool.tool] || lastTool.tool} ${lastTool.text || ""}` : "working…")
        : "";
    return { ...t, state, dur: t.ts && end ? end - t.ts : null, sum, tools: tools.length, said: said.length };
  });
}
export const MARK = { done: "✓", fail: "✕", run: "" };
export function taskList(openMap, initial) {
  const list = h("ol", { class: "cv-tasks" });
  const nodes = new Map();
  let lastArgs = null;
  function paint(steps, o) {
    lastArgs = [steps, o];
    const tasks = taskStates(groupTasks(steps), o);
    const keys = new Set(tasks.map((t) => t.key));
    for (const [k, n] of nodes) if (!keys.has(k)) { n.el.remove(); nodes.delete(k); }
    for (const t of tasks) {
      let n = nodes.get(t.key);
      if (!n) {
        const head = h("button", { class: "cv-task-h", type: "button" });
        const body = h("div", { class: "cv-task-b cv-iv" });
        const el = h("li", { class: "cv-task" }, head, body);
        n = { el, head, body, rendered: 0 };
        head.addEventListener("click", () => { openMap.set(t.key, !n.open); if (lastArgs) paint(...lastArgs); });
        nodes.set(t.key, n);
        list.append(el);
      }
      const open = openMap.has(t.key) ? openMap.get(t.key) : t.state === "run";
      n.open = open;
      n.el.dataset.state = t.state;
      n.el.classList.toggle("open", open);
      n.head.setAttribute("aria-expanded", String(open));
      const meta = [t.tools ? `${t.tools} action${t.tools > 1 ? "s" : ""}` : null, secsWords(t.dur)].filter(Boolean).join(" · ");
      put(n.head, 
        h("span", { class: `mk ${t.state}`, "aria-label": t.state === "run" ? "working" : t.state === "fail" ? "failed" : "done" }, t.state === "run" ? h("span", { class: "cv-live" }) : MARK[t.state]),
        h("span", { class: "tx" }, h("span", { class: "tt" }, t.title), t.sum ? h("span", { class: "ts" }, t.sum) : null),
        h("span", { class: "du" }, meta),
        h("span", { class: "cv-car", "aria-hidden": "true" }, open ? "▴" : "▾"));
      n.body.hidden = !open;
      if (open && n.rendered < t.items.length) { renderItems(n.body, t.items.slice(n.rendered), initial); n.rendered = t.items.length; }
      if (open && !t.items.length && !n.body.childNodes.length) n.body.append(h("p", { class: "cv-empty" }, t.state === "run" ? "Working on it…" : "Nothing said here."));
      else if (t.items.length && n.body.firstElementChild?.classList.contains("cv-empty")) n.body.firstElementChild.remove();
    }
  }
  return { el: list, paint };
}
export function renderItems(box, items, initial) {
  for (const x of items) {
    if (x.kind === "tool") {
      // Consecutive actions fold into one line ("Read 9 files · ran 2
      // commands"); tap to see each, one line apiece.
      let g = box.lastElementChild;
      if (!g || !g.classList.contains("cv-iv-acts")) {
        g = h("details", { class: "cv-iv-acts" }, h("summary", {}), h("div", { class: "cv-iv-list" }));
        g._n = {};
        box.append(g);
      }
      g._n[x.tool] = (g._n[x.tool] || 0) + 1;
      g.querySelector("summary").textContent = actsSummary(g._n);
      g.lastElementChild.append(h("div", { class: "cv-iv-act", title: x.text || "" }, h("span", { class: "w" }, TOOL_WORDS[x.tool] || x.tool), " ", h("code", {}, x.text || "")));
    } else {
      const t = toMs(x.ts);
      box.append(h("div", { class: `cv-iv-msg${x.kind === "think" ? " think" : ""}` },
        h("span", { class: "av", "aria-hidden": "true" }, initial),
        h("div", { class: "bub" }, x.kind === "think" ? h("span", { class: "lbl" }, "thinking") : null, x.text, t ? h("span", { class: "ts" }, new Date(t).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })) : null)));
    }
  }
}
