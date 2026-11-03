// The views, and the navigation: four groups for operating a fleet, then
// Labs (the tools for search engineers), collapsed and pinnable.
//
// A view module exports { id, title, group, icon, render(el, route), onParams?,
// badge?() -> { n, tone } | null, keywords?, full? }. NAV fixes the order, the
// names the navigation shows and the shortcuts; a view a module does not
// provide (yet) is left out of the navigation.

import home from "./overview.js";
import timeline from "./timeline.js";
import alerts from "./alerts.js";
import cluster from "./cluster.js";
import logs from "./logs.js";
import queries from "./queries.js";
import statement from "./statement.js";
import performance from "./performance.js";
import profile from "./profile.js";
import querypath from "./querypath.js";
import tables from "./tables.js";
import inspector from "./inspector.js";
import rollout from "./rollout.js";
import autoscale from "./autoscale.js";
import access from "./access.js";
import system from "./system.js";
import cloud from "./cloud.js";
import sql from "./sql.js";
import schema from "./schema.js";
import importer from "./import.js";
import builder from "./builder.js";
import relevance from "./relevance.js";
import recsys from "./recsys.js";
import sid from "./sid.js";
import autocomplete from "./autocomplete.js";
import media from "./media.js";
import graph from "./graph.js";
import classic from "./classic.js";

export const VIEWS = [home, timeline, alerts, cluster, logs, queries, statement, performance, profile, querypath, tables, inspector, rollout, autoscale, access, system, cloud, sql, schema, importer, builder, relevance, recsys, sid, autocomplete, media, graph, classic];

export const GROUPS = ["Operate", "Diagnose", "Change", "Data", "Labs"];

/**
 * The navigation, in order: [group, [{id, title, icon, key, scope}]]. `key`
 * is the letter after "g"; `scope` the key scope that changes things there
 * (the entry says so when the signed-in key has less).
 */
export const NAV = [
  ["Operate", [
    { id: "home", title: "Home", icon: "overview", key: "h" },
    { id: "alerts", title: "Alerts", icon: "alerts", key: "a" },
    { id: "cluster", title: "Cluster", icon: "cluster", key: "c" },
    { id: "timeline", title: "Timeline", icon: "timeline", key: "t" },
    { id: "logs", title: "Logs", icon: "logs", key: "l" },
    { id: "queries", title: "Queries", icon: "queries", key: "q" },
    { id: "performance", title: "Performance", icon: "perf", key: "p" },
  ]],
  ["Diagnose", [
    { id: "profile", title: "Profiler", icon: "flame", key: "r" },
    { id: "querypath", title: "Query path", icon: "route", key: "x" },
    { id: "tables", title: "Tables and storage", icon: "table", key: "o", fallback: "inspector" },
  ]],
  ["Change", [
    { id: "rollout", title: "Rollouts", icon: "rollout", key: "u", scope: "admin" },
    { id: "autoscale", title: "Autoscaling", icon: "autoscale", scope: "admin" },
    { id: "access", title: "Access and audit", icon: "access", key: "k", scope: "admin" },
    { id: "system", title: "Settings", icon: "system", key: "s", scope: "admin" },
    { id: "cloud", title: "Cloud setup", icon: "cloud", scope: "admin" },
  ]],
  ["Data", [
    { id: "sql", title: "SQL workbench", icon: "sql", key: "w" },
    { id: "schema", title: "Schema", icon: "schema", key: "d" },
    { id: "import", title: "Import and export", icon: "import", key: "i", scope: "write" },
  ]],
  ["Labs", [
    { id: "builder", title: "Query builder", icon: "builder", key: "b" },
    { id: "relevance", title: "Relevance lab", icon: "relevance" },
    { id: "recsys", title: "Recsys eval", icon: "recsys" },
    { id: "sid", title: "Semantic IDs", icon: "sid" },
    { id: "autocomplete", title: "Autocomplete", icon: "autocomplete" },
    { id: "media", title: "Media search", icon: "media" },
    { id: "graph", title: "Graph explorer", icon: "graph" },
    { id: "classic", title: "Query console", icon: "classic" },
  ]],
];

/**
 * Old addresses and where they go now: [from, to, params mapper?]. A route
 * whose view does not exist (yet) falls back as NAV says (`fallback`).
 */
export const REDIRECTS = {
  overview: "home",
  topology: "cluster",
  fleet: "cluster",
  control: "cluster",
  settings: "system",
  storage: "tables",
};
