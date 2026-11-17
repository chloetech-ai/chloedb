// Converge's shared words and helpers: the steps of the flow, severities,
// times, the one status line and the thin progress bar every card carries.
import { h } from "/levels/common.js";

export { h };

export const THEME_WORDS = { onboarding: "Onboarding", reliability: "Reliability", security: "Security", scalability: "Scalability", trust: "Trust", correctness: "Correctness", usability: "Usability", performance: "Performance", observability: "Observability", docs: "Docs" };
export const SEV = { 4: "blocker", 3: "major", 2: "minor", 1: "papercut" };
export const KINDS = ["bug", "gap", "docs", "ux", "opportunity", "feature"];
export const STAGES = ["found", "reproduce", "scope", "prioritize", "develop", "verify", "review", "merged"];
export const STAGE_NAME = { found: "Found", reproduce: "Reproducing", scope: "Planning", prioritize: "Ready to build", develop: "Building", verify: "Verifying", review: "Ready to merge", merged: "Merged" };

/** The five steps of the flow, each over one or two of the eight stages. */
export const STEPS = [
  { step: "problems", name: "Problems", stages: ["found"], hint: "What the personas hit. Pick what to reproduce." },
  { step: "reproducing", name: "Reproducing", stages: ["reproduce"], hint: "An agent writes a script that shows the problem." },
  { step: "ready", name: "Ready to build", stages: ["scope", "prioritize"], hint: "Planned and scored. Pick what to build." },
  { step: "building", name: "Building", stages: ["develop", "verify"], hint: "A fix agent works; the checks and the persona verify." },
  { step: "merge", name: "Ready to merge", stages: ["review"], hint: "A pull request waits for your approval." },
];
export const stepOf = (stage) => STEPS.find((s) => s.stages.includes(stage)) || null;

export const BLOCK_KIND = { credentials: "Credentials", publish: "Publish", account: "Account", external: "Outside service", decision: "Decision", other: "Your step" };
/** A finding blocked on the person (`blocked`, guarded: an older server sends none). */
export const blockedOf = (f) => (f && f.state === "active" && f.blocked && typeof f.blocked === "object" ? f.blocked : null);
export const blockStep = (f, b) => String((b && (b.you_do || b.what)) || (f.needs_you || "").replace(/^Blocked:\s*/i, "") || "A step only you can take.");
/** Waiting for a free agent slot: the server's `queued`, or an older server's needs-you that says so. */
export function queuedOf(f) {
  if (!f || f.state !== "active" || blockedOf(f)) return null;
  if (typeof f.queued === "string" && f.queued) return f.queued;
  const m = /(\d+) agents are running, the most allowed/.exec(f.needs_you || "");
  return m ? `waiting for a free agent slot (${m[1]} of ${m[1]})` : null;
}
/** Does it read as a shell command (shown in monospace, with Copy)? */
export const cmdOfStep = (s) => {
  const x = String(s || "").trim();
  const bt = /`([^`]+)`/.exec(x);
  if (bt) return bt[1];
  const i = x.indexOf(": ");
  return (i >= 0 ? x.slice(i + 2).trim() : x).replace(/^\$\s*/, "");
};
export const looksLikeCmd = (s) => {
  const x = String(s || "").trim();
  if (!x || x.length > 400) return false;
  return /`[^`]+`/.test(x) || /^(gh|git|npm|npx|pnpm|yarn|cargo|pip3?|python3?|uv|docker|kubectl|curl|brew|make|sh|bash|export|aws|gcloud|az|chloectx|chloedb[\w-]*|\.\/\S+)(\s|$)/.test(cmdOfStep(x));
};

export const toMs = (t) => { if (t == null || t === "") return null; const n = typeof t === "number" ? t : /^\d+$/.test(String(t)) ? Number(t) : Date.parse(t); return Number.isFinite(n) && n > 0 ? n : null; };
export const ago = (t) => {
  const ms = toMs(t);
  if (!ms) return "";
  const s = Math.max(0, (Date.now() - ms) / 1000);
  return s < 60 ? "now" : s < 3600 ? `${Math.round(s / 60)} min` : s < 86400 ? `${Math.round(s / 3600)} h` : `${Math.round(s / 86400)} d`;
};
export const usd = (x) => (x == null ? "–" : `$${Number(x).toFixed(2)}`);
export const enc = encodeURIComponent;
export const secsWords = (ms) => { if (!Number.isFinite(ms) || ms < 0) return ""; const s = Math.round(ms / 1000); return s < 60 ? `${s} s` : s < 3600 ? `${Math.floor(s / 60)} min` : `${Math.floor(s / 3600)} h ${Math.floor((s % 3600) / 60)} min`; };
export const firstSentences = (t, n = 2) => { const x = String(t || "").replace(/\s+/g, " ").trim(); const m = x.match(new RegExp(`^(.*?[.!?])(\\s|$)${n > 1 ? "(.*?[.!?])?(\\s|$)" : ""}`)); const out = m ? (m[1] + (m[3] ? " " + m[3] : "")) : x; return out.length > 220 ? out.slice(0, 218) + "…" : out; };

/** replaceChildren, flattening arrays and leaving out null, as h() does. */
export const put = (el, ...kids) => el.replaceChildren(...kids.flat(Infinity).filter((k) => k != null && k !== false).map((k) => (k instanceof Node ? k : document.createTextNode(String(k)))));

/** A short message at the bottom; with `undo`, a button that calls it. */
export function toast(text, o = {}) {
  document.querySelectorAll(".cv-toast").forEach((t) => t.remove());
  const t = h("div", { class: "cv-toast", role: "status" }, h("span", {}, text),
    o.undo ? h("button", { type: "button", class: "cv-toast-u", onclick: () => { t.remove(); o.undo(); } }, "Undo") : null);
  document.body.append(t);
  setTimeout(() => t.remove(), o.ms || (o.undo ? 5000 : 3000));
  return t;
}

/** What an agent's last event says it is doing, in a few plain words. */
/** A command as a person would say it: its first line, without heredoc
 *  bodies, leading `cd …&&`, environment settings or long paths. */
export function cmdWords(cmd) {
  let c = String(cmd || "").split("\n")[0];
  c = c.replace(/<<-?\s*['"]?\w+['"]?.*$/, "").trim();
  for (let i = 0; i < 4; i++) c = c.replace(/^(cd\s+("[^"]*"|'[^']*'|\S+)\s*(&&|;)\s*|[A-Z_][A-Z0-9_]*=\S*\s+)/, "");
  c = c.replace(/(^|\s)(\/[^\s'"]*\/|~\/[^\s'"]*\/|\.\.?\/[^\s'"]*\/)([^\s/'"]+)/g, "$1$3").trim();
  return c;
}
const INTERNAL = /^(TaskStop|TaskOutput|TaskCreate|TaskUpdate|TaskList|TaskGet|BashOutput|KillShell|KillBash|ToolSearch|ExitPlanMode|EnterPlanMode|Monitor|SendMessage|ListMcpResources|ReadMcpResource|AskUserQuestion|Skill|SlashCommand)$/;

export function doingWords(ev) {
  if (!ev || typeof ev !== "string") return null;
  if (ev === "turn finished") return "finished its turn";
  if (ev.startsWith("prompt:")) return "reading its task";
  if (ev.startsWith("session started")) return "starting";
  const m = /^([\w.-]+)( done| failed)?(?::\s*(.*))?$/s.exec(ev);
  if (!m) return cmdWords(ev).slice(0, 80) || "working";
  const [, tool, end, detail = ""] = m;
  const base = (p) => p.split("/").filter(Boolean).pop() || p;
  const fail = end === " failed" ? " (failed)" : "";
  if (tool === "Bash") {
    const cmd = cmdWords(detail);
    const w = cmd.split(/\s+/).filter(Boolean);
    let what;
    if (!cmd || /^cd\b/.test(cmd)) what = "moving around the checkout";
    else if (/repro\.sh/.test(cmd)) what = "running its repro";
    else if (/^cargo (build|b)\b/.test(cmd)) what = "building";
    else if (/^cargo (test|t|nextest)\b/.test(cmd) || /^(pytest|npm test|npm run test|go test|node --test)/.test(cmd)) what = "running tests";
    else if (/^cargo (clippy|check|fmt)\b/.test(cmd)) what = "checking the code";
    else if (/^git (commit)/.test(cmd)) what = "committing";
    else if (/^git /.test(cmd)) what = `git ${w[1] || ""}`.trim();
    else if (/^(cat|tee|printf|echo)\b/.test(cmd) && />|\btee\b/.test(cmd)) what = (/>+\s*(\S+)/.exec(cmd) || /tee\s+(?:-a\s+)?(\S+)/.exec(cmd)) ? `writing ${base((/>+\s*(\S+)/.exec(cmd) || /tee\s+(?:-a\s+)?(\S+)/.exec(cmd))[1])}` : "writing a file";
    else if (/^(cat|head|tail|less|sed -n|wc)\b/.test(cmd)) what = w.length > 1 && !/^[<>|-]/.test(w.at(-1)) ? `reading ${base(w.at(-1))}` : "reading a file";
    else if (/^(grep|rg|find|ls|tree)\b/.test(cmd)) what = "looking around the code";
    else if (/^(mkdir|cp|mv|rm|chmod|touch)\b/.test(cmd)) what = "arranging files";
    else what = `running ${w.slice(0, 2).join(" ")}`.slice(0, 48);
    // A search that found nothing exits 1: not a failure.
    return `${what}${what === "looking around the code" ? "" : fail}`;
  }
  // The tools that manage its own background tasks and sessions: not news.
  if (INTERNAL.test(tool)) return `managing its work${fail}`;
  if (tool.startsWith("mcp__")) {
    const parts = tool.split("__");
    return `using ${parts[1] || "a tool"}${parts[2] ? ` (${parts[2].replace(/_/g, " ")})` : ""}${fail}`;
  }
  const words = { Read: "reading", Edit: "editing", MultiEdit: "editing", Write: "writing", NotebookEdit: "editing", Grep: "searching for", Glob: "looking for", WebFetch: "opening", WebSearch: "searching the web for", TodoWrite: "planning", Task: "delegating", Agent: "delegating" };
  const v = words[tool] || (/^[A-Z]/.test(tool) ? "working" : tool);
  const d = /^(Read|Edit|MultiEdit|Write|NotebookEdit)$/.test(tool) ? base(detail) : words[tool] && !/^(TodoWrite|Task|Agent)$/.test(tool) ? cmdWords(detail).slice(0, 50) : "";
  return `${v}${d ? " " + d : ""}${/^(Grep|Glob)$/.test(tool) ? "" : fail}`;
}

/** Is an agent at work on it now? */
export const agentOn = (f) => !!(f && f.agent && ["starting", "working", "waiting"].includes(f.agent_state));

/** What is happening to a finding now, in a few words (never null). */
export function happening(f, settings = {}) {
  if (!f) return "";
  if (f.state === "parked") return "parked";
  if (f.state && f.state !== "active") return f.state.replace(/_/g, " ");
  if (f.busy) return String(f.busy);
  const who = f.stage === "reproduce" ? "repro agent" : f.stage === "scope" || f.stage === "prioritize" ? "planning agent" : "fix agent";
  if (agentOn(f)) {
    if (f.agent_state === "waiting") return `${who} waits for an answer`;
    if (f.agent_state === "starting") return `${who} starting`;
    return doingWords(f.agent_doing) || (f.stage === "reproduce" ? "writing repro" : "working");
  }
  const rounds = Math.max(1, settings.max_rounds || 3);
  switch (f.stage) {
    case "found": return seenOf(f) > 1 ? `seen ${seenOf(f)}×` : "waits for your pick";
    case "reproduce": return f.needs_you ? "needs a hint or a repro" : f.repro_hash ? "repro locked" : f.repro ? "running the repro" : "waits for a repro agent";
    case "scope": return "being planned";
    case "prioritize": return f.door ? `one-way door: ${f.door}` : f.queued ? "waits for a free agent slot" : settings.develop_auto ? "planned, starts when a slot is free" : "planned, waits for your pick";
    case "develop": return f.rounds ? `round ${Math.min(f.rounds + 1, rounds)} of ${rounds}` : "checks run next";
    case "verify": return !f.persona ? "verified by its repro next" : f.busy ? "the persona replays it" : "waits for the replay (budget or a run slot)";
    case "review": return f.conflict ? "conflicts with the base" : f.ci === "pass" ? "CI passed" : f.ci === "fail" ? "CI failed" : f.pr_url ? "CI running" : f.needs_you ? "waits for you: merge it or record a pull request" : "opening the pull request";
    case "merged": return f.guard_ok === false ? "regressed" : "merged";
    default: return "";
  }
}
export const fmtScore = (x) => (x == null ? "–" : Number(x) >= 10 ? String(Math.round(Number(x))) : Number(x).toFixed(1));

/** How far along a finding is, 0..1, across the eight stages. */
export function fraction(f, settings = {}) {
  const i = Math.max(0, STAGES.indexOf(f.stage));
  if (f.stage === "merged") return 1;
  const rounds = Math.max(1, settings.max_rounds || 3);
  const inside = f.stage === "develop" ? Math.min(0.9, 0.25 + (f.rounds || 0) * (0.6 / rounds))
    : f.stage === "review" ? (f.ci === "pass" ? 0.85 : f.pr_url ? 0.5 : 0.2)
      : agentOn(f) || f.busy ? 0.6 : 0.25;
  return (i + inside) / 8;
}

/** The one status line: stage · what is happening · time; and the blocked or queued line. */
export function statusLine(f, settings) {
  const live = agentOn(f) && f.agent_state !== "waiting" || !!f.busy;
  return h("span", { class: "cv-st" },
    h("span", { class: "sg" }, STAGE_NAME[f.stage] || f.stage),
    h("span", { class: `wh${live ? " on" : ""}` }, live ? h("i", { class: "cv-pulse", "aria-hidden": "true" }) : null, happening(f, settings)),
    h("span", { class: "tm" }, ago(f.stage_since || f.updated)));
}
export function flagLine(f) {
  const b = blockedOf(f);
  if (b) return h("span", { class: "cv-blkl" }, h("b", {}, "Blocked: "), blockStep(f, b));
  const q = queuedOf(f);
  if (q) return h("span", { class: "cv-ql" }, `Queued: ${q}`);
  return null;
}
export function thinBar(f, settings) {
  const p = Math.round(fraction(f, settings) * 100);
  const b = blockedOf(f);
  return h("span", { class: `cv-thin${b ? " blk" : ""}`, role: "progressbar", "aria-valuemin": "0", "aria-valuemax": "100", "aria-valuenow": String(p), "aria-label": `${STAGE_NAME[f.stage] || f.stage}, ${p}% of the way` }, h("i", { style: { width: `${p}%` } }));
}

/** A persona's first name. */
export const firstName = (n) => String(n || "").split(",")[0];

/** Reads and writes a per-viewer convenience; never fails. */
export const local = {
  get(k) { try { return localStorage.getItem(k); } catch { return null; } },
  set(k, v) { try { localStorage.setItem(k, v); } catch { /* private window */ } },
};

/** How many reports were folded into a card: the server's `seen`, else the older `occurrences`. */
export const seenOf = (f) => { const n = Number(f && (f.seen ?? f.occurrences)); return Number.isFinite(n) && n > 0 ? n : 1; };
/** The cards that look like the same problem (`similar`, ids; tolerated absent or as objects). */
export const similarOf = (f) => (Array.isArray(f && f.similar) ? f.similar.map((x) => (typeof x === "string" ? x : x && x.id)).filter(Boolean) : []);
/** The card's evidence object, or {} (an older server sends none). */
export const evidenceOf = (f) => (f && f.evidence && typeof f.evidence === "object" ? f.evidence : {});

/** The three decisions that are yours, and what is moving without you. */
export function inboxOf(findings) {
  const merge = [], build = [], fix = [], moving = [];
  for (const f of findings) {
    if (f.state !== "active" || f.stage === "merged") continue;
    if (blockedOf(f)) { moving.push(f); continue; }
    if (f.stage === "review" && !f.conflict && !f.busy && (f.ci === "pass" || (!f.pr_url && f.branch && f.needs_you))) merge.push(f);
    else if (f.stage === "prioritize" && !queuedOf(f)) build.push(f);
    else if (f.stage === "found") fix.push(f);
    else moving.push(f);
  }
  const sev = (a, b) => (b.pinned ? 1 : 0) - (a.pinned ? 1 : 0) || b.severity - a.severity || seenOf(b) - seenOf(a) || (toMs(b.created) || 0) - (toMs(a.created) || 0);
  merge.sort(sev);
  build.sort((a, b) => (b.pinned ? 1 : 0) - (a.pinned ? 1 : 0) || (b.score || 0) - (a.score || 0) || b.severity - a.severity);
  fix.sort(sev);
  moving.sort((a, b) => (blockedOf(b) ? 1 : 0) - (blockedOf(a) ? 1 : 0) || ((agentOn(b) || b.busy ? 1 : 0) - (agentOn(a) || a.busy ? 1 : 0)) || STAGES.indexOf(b.stage) - STAGES.indexOf(a.stage));
  return { merge, build, fix, moving, count: merge.length + build.length + fix.length };
}

/** Hide cards now and send the verb once the undo has passed (5 s). */
export function deferVerb(C, ids, verb, body, words, ms = 5000) {
  const S = C.S;
  ids.forEach((id) => { S.gone.add(id); S.sel.delete(id); });
  C.keep(); C.draw();
  let undone = false;
  const timer = setTimeout(async () => {
    if (undone) return;
    try { await C.bulk(ids, verb, body); } catch (err) { toast(err.message || String(err)); }
    ids.forEach((id) => S.gone.delete(id));
    S.sig = ""; await C.load();
  }, ms);
  toast(words, { undo: () => { undone = true; clearTimeout(timer); ids.forEach((id) => S.gone.delete(id)); C.keep(); C.draw(); }, ms });
}
