// "From GitHub" on Open a project (`#/p`): owner/repo or an address,
// cloned into Workbench's data dir, forked when you cannot push to it, and
// read for its commands (POST /api/repos/import, src/import.rs). What was
// detected is shown to confirm before Converge writes personas from it.
import { h, useStyle, btn, label } from "/levels/common.js";

const enc = encodeURIComponent;
const STEPS = { starting: "Starting", cloning: "Cloning", fetching: "Fetching", checking: "Checking access", forking: "Forking", detecting: "Detecting", done: "Opened" };

export function githubImport(ctx) {
  useStyle("g-ghi", CSS);
  const S = { busy: false, steps: [], err: null, r: null, fork: true };
  const input = h("input", { class: "field", type: "text", spellcheck: "false", autocomplete: "off", placeholder: "owner/repo  or  https://github.com/owner/repo", "aria-label": "GitHub repository" });
  const go = h("button", { class: "btn primary", type: "submit" }, "Import");
  const out = h("div", { class: "ghi-out", "aria-live": "polite" });
  const form = h("form", { class: "ghi-row", onsubmit: (e) => { e.preventDefault(); run(); } }, input, go);
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

  async function run() {
    const source = input.value.trim();
    if (!source || S.busy) return;
    Object.assign(S, { busy: true, steps: [], err: null, r: null });
    draw();
    try {
      const j = await ctx.api("/api/repos/import", { method: "POST", body: { source, async: true, fork: S.fork } });
      for (;;) {
        await sleep(600);
        const v = await ctx.api(`/api/repos/import/${enc(j.id)}`);
        S.steps = v.steps || [];
        draw();
        if (v.state === "done") { S.r = v.result; break; }
        if (v.state === "failed") { S.err = v.error || "The import failed"; break; }
      }
    } catch (err) { S.err = err.message || String(err); }
    S.busy = false;
    if (S.r && ctx.refreshProjects) await ctx.refreshProjects().catch(() => {});
    draw();
  }

  function draw() {
    go.disabled = S.busy;
    input.disabled = S.busy;
    const steps = S.steps.length ? h("ol", { class: "ghi-steps" }, S.steps.map((s, i) => h("li", { class: i === S.steps.length - 1 && S.busy ? "now" : "ok" }, h("b", {}, STEPS[s.step] || s.step), " ", s.text))) : S.busy ? h("p", { class: "ghi-dim" }, "Starting…") : null;
    out.replaceChildren(...[steps, S.err ? h("div", { class: "rp-err", role: "alert" }, S.err) : null, S.r ? confirm(S.r) : null].filter(Boolean));
  }

  function confirm(r) {
    const d = r.detected || {};
    const test = h("input", { class: "field", value: (r.settings && r.settings.test_cmd) || d.test_cmd || "", placeholder: "no test command found: type one, such as make test", "aria-label": "Test command", spellcheck: "false" });
    const row = (k, v) => h("div", { class: "ghi-kv" }, h("span", { class: "k" }, k), h("span", { class: "v" }, v));
    const open = async () => {
      const cmd = test.value.trim();
      const was = (r.settings && r.settings.test_cmd) || "";
      try {
        if (cmd !== was) await ctx.api("/api/converge/settings", { method: "PUT", body: { repo: r.path, settings: { test_cmd: cmd || null } } });
        ctx.go(`#/converge/personas?project=${enc(r.project)}`);
      } catch (err) { S.err = err.message || String(err); draw(); }
    };
    return h("div", { class: "pane ghi-done" },
      label(`${r.project}${r.reused ? " (updated)" : ""}`),
      r.untrusted ? h("div", { class: "ghi-warn", role: "note" }, h("b", {}, "Code you do not own. "), r.warning || "Personas and agents will run it on this machine.", " Fix agents ask before any command their list does not allow.") : null,
      row("From", r.forked ? `${r.upstream}, forked to ${r.fork}: pull requests go from your fork to ${r.upstream}` : r.source || ""),
      row("Folder", r.path),
      row("Language", d.language || "not recognised"),
      h("div", { class: "ghi-kv" }, h("span", { class: "k" }, "Tests"), test),
      row("Runs with", d.run_cmd || "no run command found"),
      row("CI", d.ci ? "GitHub Actions workflows" : "none found"),
      row("Base branch", (r.settings && r.settings.base) || r.default_branch || ""),
      (r.notes || []).map((n) => h("p", { class: "ghi-dim" }, n)),
      h("div", { class: "ghi-row" }, btn("Write personas from the code", { primary: true, onclick: open }), btn("Open the project", { href: `#/p/${r.project}` })));
  }

  return h("div", { class: "ghi" }, label("From GitHub"), h("p", { class: "ghi-dim" }, "A repository on GitHub, cloned into Workbench's data folder. If you cannot push to it, Workbench forks it so pull requests come from your fork."), form, h("label", { class: "g-tick" }, h("input", { type: "checkbox", checked: true, onchange: (e) => { S.fork = e.target.checked; } }), h("span", {}, h("b", {}, "Fork it if I cannot push"), " Pull requests then go from your fork to the original.")), out);
}

const CSS = `
.ghi{display:flex;flex-direction:column;gap:var(--s-2)}
.ghi-row{display:flex;gap:var(--s-2);flex-wrap:wrap;align-items:center}.ghi-row .field{flex:1;min-width:220px}
.ghi-dim{margin:0;color:var(--text-3);font-size:var(--fs-sm)}
.ghi-steps{margin:0;padding-left:1.2em;font-size:var(--fs-sm)}.ghi-steps li.now b::after{content:" …"}
.ghi-done{padding:var(--s-3) var(--s-4);display:flex;flex-direction:column;gap:var(--s-2)}
.ghi-kv{display:flex;gap:var(--s-2);align-items:baseline;flex-wrap:wrap}.ghi-kv .k{min-width:96px;color:var(--text-3);font-size:var(--fs-sm)}.ghi-kv .v{overflow-wrap:anywhere;font-family:var(--mono);font-size:var(--fs-sm)}.ghi-kv .field{flex:1;min-width:220px;font-family:var(--mono)}
.ghi-warn{padding:var(--s-2) var(--s-3);border-radius:6px;border:var(--hair) solid var(--warn);background:color-mix(in srgb,var(--warn) 12%,transparent);font-size:var(--fs-sm)}
`;
