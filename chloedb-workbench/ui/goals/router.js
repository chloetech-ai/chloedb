// The routes (JOURNEYS.md, "URL routes"): what page a hash names, and the
// old hashes as redirects. Pure: `resolve(hash, { project, agentIds })`
// gives `{ redirect }` or `{ page, ...params, query }`. `project` is the
// short name of the project in focus (the last one opened, or the most
// active), for the old routes that had none.
//
// The app is Converge: `#/` opens its board (`#/converge?project=<the
// project in focus>`, or its project picker with none). Workbench is its
// section: Workbench's home, the old Start page, is `#/workbench`, and
// Workbench's pages (`#/p/…`, `#/a/…`, `#/needs`, `#/more/…`, `#/debug/…`)
// stay where they were.

function split(hash) {
  const h = (hash || "").replace(/^#/, "");
  const qi = h.indexOf("?");
  const path = qi >= 0 ? h.slice(0, qi) : h;
  const query = new URLSearchParams(qi >= 0 ? h.slice(qi + 1) : "");
  return { parts: path.split("/").filter(Boolean).map((p) => { try { return decodeURIComponent(p); } catch { return p; } }), query, raw: qi >= 0 ? h.slice(qi + 1) : "" };
}

const MORE = ["terminals", "time", "decisions", "library", "rules", "guide", "keys", "setup", "settings"];

/** Converge's board for a project, or its project picker with none. */
export function home(project) {
  return project ? `#/converge?project=${encodeURIComponent(project)}` : "#/converge";
}

export function resolve(hash, { project = null, agentIds = [] } = {}) {
  const { parts, query, raw } = split(hash);
  const q = raw ? `?${raw}` : "";
  const p = project;
  const P = (rest = "") => (p ? `#/p/${encodeURIComponent(p)}${rest}` : "#/p");
  // #<agent id> (the old zoom)
  if (!hash || hash === "#" || hash === "#/") return { redirect: home(p) };
  if (!hash.startsWith("#/")) {
    const id = hash.slice(1);
    if (id === "decisions") return { redirect: "#/more/decisions" };
    if (agentIds.includes(id) || /^[0-9a-z]{4,12}$/.test(id)) return { redirect: `#/a/${id}` };
    return { redirect: "#/" };
  }
  const [a, b, c, d] = parts;
  switch (a) {
    case "workbench": return { page: "start", query };
    case "new": return { page: "new", step: b || "folder", query };
    case "needs": return { page: "needs", query };
    // The Simple view (goals/simple/): the board, a request, the box.
    case "s": return { page: "simple", rest: parts.slice(1), query };
    case "p":
      if (!b) return { page: "open", query };
      if (!c) return { page: "continue", project: b, query };
      if (c === "understand") return { page: "understand", project: b, query };
      if (c === "change") return { page: "change", project: b, query };
      if (c === "main") return { page: "main", project: b, query };
      if (c === "ship") return { page: "ship", project: b, agent: d || null, query };
      return { redirect: `#/p/${encodeURIComponent(b)}` };
    case "a":
      if (!b) return { redirect: "#/" };
      return { page: "agent", agent: b, tab: ["changes", "preview", "terminal", "events"].includes(c) ? c : "progress", query };
    case "debug": return { page: "debug", rest: parts.slice(1), query };
    // Optimize the harness (HARNESS.md): its pages follow their own hash.
    case "harness": return { page: "harness", rest: parts.slice(1), project: query.get("project") || p, query };
    // Converge (SPEC.md, "Seventh build"): its views follow their own hash.
    // The project is the hash's own: with none, Converge shows its project
    // picker (the routing contract), so the project in focus is not taken,
    // except for an old link to one finding (`?f=`), which needs a board.
    case "converge": return { page: "converge", rest: parts.slice(1), project: query.get("project") || (query.has("f") ? p : null), query };
    case "more":
      if (b === "harness") return { redirect: "#/harness" };
      if (b === "converge") return { redirect: home(p) };
      if (b === "tests") return { redirect: "#/debug/failing?tab=tests" };
      if (MORE.includes(b)) return { page: "more", item: b, query };
      return { page: "more", item: null, query };
    // Today's routes, for a release (JOURNEYS.md, "URL routes").
    case "mission": return { redirect: P("/change") };
    // Decide's rules and guarded paths live under More › Rules now.
    case "decide": case "rules": case "guarded": return { redirect: "#/more/rules" };
    case "away": case "catchup": return { redirect: P() };
    case "trace": case "why": return { redirect: `#/debug/code${q}` };
    case "map": return { redirect: P("/understand") };
    case "story": return { redirect: b ? `#/a/${b}` : P() };
    case "preview": return { redirect: b ? `#/a/${b}/preview` : P() };
    case "ship": return { redirect: b ? P(`/ship/${b}`) : P("/ship") };
    case "health": return { redirect: b && /^i[0-9a-f]{8}$/.test(b) ? `#/debug/failing?incident=${b}` : b ? `#/debug/failing?tab=${encodeURIComponent(b)}` : "#/debug/failing" };
    case "flow": return { redirect: "#/more/time" };
    case "library": return { redirect: `#/more/library${q}` };
    case "grid": return { redirect: "#/more/terminals" };
    case "guide": return { redirect: "#/more/guide" };
    case "zoom": return { redirect: b ? `#/a/${b}/terminal` : "#/" };
    default: return { redirect: "#/" };
  }
}
