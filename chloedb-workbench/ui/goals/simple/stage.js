// Where a request is (Simple view): one agent's stage on the board, from
// what Workbench already records. Pure, so that the board, the request
// page and the tests agree:
//   - Planning: running, nothing changed in its worktree yet (or it
//     started in plan mode); also an agent whose turn ended with no change
//     ("replied": it answered, or asked in words, and waits for you);
//   - Building: running with a change in its worktree;
//   - Review: its turn ended (or it exited) with a change main does not
//     have, which is today's "ready";
//   - Done: merged with nothing new since, or ended with no change.
// Needs you is not a stage: it rides on top of any of them, so a card
// keeps its column while it waits (JOURNEYS.md rule 4: the one count).
// An agent whose turn ended while its helpers (the sub-agents it started
// with Claude Code's Agent tool, in the background) still work is at work.

export const STAGES = [
  { key: "planning", title: "Planning", sub: "Reading and planning. No change yet." },
  { key: "building", title: "Building", sub: "Changing code in its own worktree." },
  { key: "review", title: "Review", sub: "Finished, with a change for you to check." },
  { key: "done", title: "Done", sub: "Merged, or ended with nothing to merge." },
];
export const STEPS = [["planning", "Plan"], ["building", "Build"], ["review", "Review"], ["done", "Done"]];

/** `a`: the agent. `o`: { group } (the projects' group: needs, working,
 *  ready, idle, finished), { merged } (Review's merge record, or null),
 *  { changed } (true when /api/review lists a change on its branch). */
export function stageOf(a, { group, merged = null, changed = false } = {}) {
  if (!a) return "done";
  // Review's merge record is kept only while its worktree has nothing new
  // (src/repos.rs), so it wins over a review list read before the merge;
  // an agent at work again after it is not done.
  const busy = a.running && (a.state === "working" || a.state === "starting" || a.state === "waiting" || helping(a));
  if (merged && !busy) return "done";
  if (merged && !changed) return "done";
  if (group === "ready") return "review";
  if (!a.running) return changed ? "review" : "done";
  if (a.state === "idle" && !helping(a)) return changed ? "review" : "planning";
  return changed ? "building" : "planning";
}

/** Whether sub-agents it started still work (the agent's `helpers`,
 *  src/helpers.rs). */
export function helping(a) {
  return !!(a && a.running && a.helpers && a.helpers.running > 0);
}

/** Whether its turn ended with no change: it answered, or asked in words. */
export function replied(a, stage) {
  return !!a && a.running && a.state === "idle" && !helping(a) && stage === "planning";
}

/** A request's title in a few words: Review's title when it has one, else
 *  its outcome's or goal's, else its task's first line. A lead's task is
 *  Workbench's own long instructions: its goal names it. */
export function titleOf(a, { reviewTitle, outcome, goal } = {}) {
  if (reviewTitle) return clip(reviewTitle, 80);
  if (outcome) return clip(outcome, 80);
  const t = String((a && a.task) || "").trim();
  if (/^You lead the work on a goal/.test(t)) {
    const m = t.match(/GOAL \([^)]*\):\s*(.+)/);
    return clip(goal || (m ? m[1] : "Lead of a goal"), 80);
  }
  if (!t) return a && a.name ? `${a.name}, started at a prompt` : "A request";
  return clip(t.split("\n")[0], 80);
}

function clip(s, n) {
  const x = String(s).replace(/\s+/g, " ").trim();
  return x.length > n ? `${x.slice(0, n - 1).replace(/\s+\S*$/, "")}…` : x;
}

/** The permission mode in Start a change's words (PERMISSION_MODES in
 *  common.js). Unknown or absent: null (the user's own settings). */
export function modeWords(m) {
  return { plan: "Plan", default: "Default", acceptEdits: "Accept edits", auto: "Auto", bypassPermissions: "Bypass permissions" }[m] || null;
}

/** Requests split into lines: one per non-empty line when `perLine`. */
export function splitRequests(text, perLine) {
  const t = String(text || "").trim();
  if (!t) return [];
  if (!perLine) return [t];
  return t.split("\n").map((s) => s.replace(/^\s*(?:[-*•]|\d+[.)])\s+/, "").trim()).filter(Boolean);
}
