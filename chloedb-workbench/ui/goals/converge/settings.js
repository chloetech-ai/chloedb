// Autopilot settings (under "⋯"): its limits, cadence and checks, what fix
// agents may do and what runs on its own. Save sits in a sticky bar.
import { h } from "/goals/converge/util.js";
import { stepHead } from "/goals/converge/rows.js";

export function settingsScreen(C) {
  const d = C.S.d;
    const st = { ...d.settings };
    const field = (k, l, hint, type = "number") => {
      const inp = h("input", { type, id: `cv-s-${k}`, value: st[k] == null ? "" : String(st[k]), step: type === "number" ? "any" : null });
      inp.addEventListener("change", () => { st[k] = type === "number" ? Number(inp.value) : (inp.value || null); });
      return h("div", { class: "cv-f" }, h("label", { for: `cv-s-${k}` }, l), inp, h("span", { class: "hint" }, hint));
    };
    const list = (k, l, hint) => {
      const ta = h("textarea", { class: "cv-ta", id: `cv-s-${k}`, spellcheck: "false", style: { minHeight: "140px" } });
      ta.value = (st[k] || []).join("\n");
      ta.addEventListener("change", () => { st[k] = ta.value.split("\n").map((x) => x.trim()).filter(Boolean); });
      return h("div", { class: "cv-f" }, h("label", { for: `cv-s-${k}` }, l), ta, h("span", { class: "hint" }, hint));
    };
    const tog = (k, l, hint) => {
      const inp = h("input", { type: "checkbox", class: "cv-cb", id: `cv-s-${k}`, checked: st[k] ? true : null });
      inp.addEventListener("change", () => { st[k] = inp.checked; });
      return h("div", { class: "cv-f" }, h("label", { for: `cv-s-${k}` }, inp, " ", l), h("span", { class: "hint" }, hint));
    };
    const panels = h("div", { class: "cv-panels" },
      h("div", { class: "cv-panel" }, h("h3", {}, "Limits"), h("div", { class: "sub" }, "The autopilot never goes past these"),
        field("budget_usd_day", "Model budget per day (USD)", "Persona runs, scoping, new personas, and the fix, repro and chat agents. A spent budget pauses new work."),
        field("runs_at_once", "Persona runs at once", "Each in its own checkout."),
        field("wip", "Findings in development at once", "Each agent on its own worktree and branch."),
        field("max_rounds", "Rounds before it asks you", "Develop and verify rounds without passing."),
        field("repro_agents", "Repro agents at once", "A finding in Reproduce without a script gets an agent to write one; 0 leaves it to you.")),
      h("div", { class: "cv-panel" }, h("h3", {}, "Cadence and checks"), h("div", { class: "sub" }, "When personas play and what a fix must pass"),
        field("tick_s", "Autopilot tick (seconds)", "How often it advances what it can."),
        field("replay_every_h", "Replay each persona every (hours)", "Plus once after each merge."),
        field("repro_runs", "Repro runs", "How many times a repro must fail on the base branch, and pass on the fix."),
        field("test_cmd", "Test command", "Run in the agent's worktree by the develop gate, e.g. cd chloedb-workbench && cargo test --profile ci", "text"),
        field("base", "Base branch", "Where findings are reproduced and fixes merge (empty: the checkout's branch).", "text"),
        field("model", "Persona model", "For persona runs, scoping and the generator.", "text")),
      h("div", { class: "cv-panel" }, h("h3", {}, "What fix agents may do"), h("div", { class: "sub" }, "Set once, so a run does not stop at every new command. Each agent works in its own worktree; pushing and pull requests stay Converge's own."),
        field("dev_permission_mode", "Permission mode", "auto: Claude Code's classifier answers what the lists do not. default: those wait for you in Needs you.", "text"),
        list("dev_allow", "Allowed without asking", "One Claude Code rule a line, e.g. Bash(cargo:*)"),
        list("dev_deny", "Never", "One rule a line")),
      h("div", { class: "cv-panel" }, h("h3", {}, "What runs on its own"), h("div", { class: "sub" }, "Merges and one-way doors always wait for you"),
        tog("enabled", "Autopilot", "Ticks on its own, every tick."),
        tog("found_auto", "Move new findings to Reproduce", "Off: each new finding waits in Found until you choose Reproduce or Dismiss."),
        tog("develop_auto", "Start development on its own", "Off: a scoped finding waits in Prioritize until you choose Start development."),
        tog("follow_picks", "Carry on what you pick", "With the autopilot off, a finding you picked still moves to your next decision; personas play only with it on."),
        tog("generate", "Generate new personas", "When new serious findings dry up, one aimed at the least-played part."),
        tog("retire", "Retire stale personas", "After 3 runs with nothing new, or a repro rate under 40%."),
        tog("open_pr", "Open pull requests", "Push the branch and open its pull request with gh at review.")));
  const on = !!(d.settings || {}).enabled;
  return h("div", {},
    stepHead(C, "Autopilot", null, "What moves by itself, and the limits it keeps. Merges and one-way doors always wait for you."),
    h("div", { class: "cv-bar", style: { marginBottom: "16px" } },
      h("button", { class: "cv-btn", type: "button", onclick: () => C.act(() => C.api("/api/converge/settings", { method: "PUT", body: { repo: d.repo, enabled: !on } }), on ? "Autopilot paused: running steps finish" : "Autopilot on") }, on ? "Pause the autopilot" : "Turn the autopilot on"),
      h("button", { class: "cv-btn", type: "button", onclick: () => C.act(() => C.api("/api/converge/tick", { method: "POST", body: { repo: d.repo } }), "Tick started: what can move now, moves") }, "Run a tick now")),
    panels,
    h("div", { class: "cv-bulk" }, h("span", { class: "n" }, "Changes apply from the next tick."),
      h("button", { class: "cv-btn pri", type: "button", onclick: () => C.act(() => C.api("/api/converge/settings", { method: "PUT", body: { repo: d.repo, settings: st } }), "Settings saved") }, "Save")));
}
