/* ChloeDB showcase site: code highlighting, copy buttons, tabs, tooltips,
 * and the renderers for benchmark charts (bench-data.js) and the capability
 * matrix (compare-data.js). No dependencies. */
(function () {
  "use strict";

  var $$ = function (sel, root) { return Array.prototype.slice.call((root || document).querySelectorAll(sel)); };
  var esc = function (s) {
    return String(s).replace(/[&<>"]/g, function (c) { return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]; });
  };

  /* ---------------------------------------------------------- highlighting */
  var SQL_KW = new Set(("SELECT FROM WHERE AND OR NOT IN ORDER BY DESC ASC LIMIT OFFSET GROUP HAVING WITH AS JOIN LEFT ON " +
    "INSERT INTO VALUES UPDATE SET DELETE CREATE TABLE ALTER ADD COLUMN DROP RANK PROFILE OPTIONS PRIMARY KEY REFERENCES " +
    "DEFAULT NULL IS BETWEEN LIKE DISTINCT CASE WHEN THEN ELSE END MAX PER EXPLAIN TRUE FALSE EXISTS IF REPLACE RENAME TO " +
    "CONFLICT DO NULLS FIRST LAST INT FLOAT TEXT KEYWORD VECTOR BOOL TIMESTAMP JSON MEDIA EDGES SPARSE CURRENT_TIMESTAMP").split(" "));
  var PY_KW = new Set("import from as def return for in with if else elif not and or True False None lambda".split(" "));
  var RULES = {
    sql: /(?<com>--[^\n]*)|(?<str>'(?:[^']|'')*')|(?<par>:[A-Za-z_]\w*)|(?<num>\b\d+(?:\.\d+)?\b)|(?<fn>\b[A-Za-z_]\w*(?=\())|(?<w>\b[A-Za-z_]\w*\b)/g,
    python: /(?<com>#[^\n]*)|(?<str>"(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*')|(?<num>\b\d+(?:\.\d+)?\b)|(?<fn>\b[A-Za-z_]\w*(?=\())|(?<w>\b[A-Za-z_]\w*\b)/g,
    rust: /(?<com>\/\/[^\n]*)|(?<str>"(?:[^"\\\n]|\\.)*")|(?<num>\b\d+(?:\.\d+)?\b)|(?<fn>\b[A-Za-z_]\w*!?(?=\())|(?<w>\b(?:let|mut|fn|use|for|in|if|else|match|return|pub|struct|impl|true|false|Some|None|Ok|Err)\b)/g,
    sh: /(?<com>(?<![^\s])#[^\n]*)|(?<str>"(?:[^"\\]|\\.)*"|'[^']*')|(?<par>(?<![\w-])--?[A-Za-z][\w-]*)/g,
    yaml: /(?<com>(?<![^\s])#[^\n]*)|(?<str>"(?:[^"\\]|\\.)*"|'[^']*')|(?<fn>^[ \t-]*[\w.\/-]+(?=:))|(?<num>\b\d+(?:\.\d+)?\b)/gm,
    json: /(?<fn>"(?:[^"\\]|\\.)*"(?=\s*:))|(?<str>"(?:[^"\\]|\\.)*")|(?<num>-?\b\d+(?:\.\d+)?\b)|(?<w>\b(?:true|false|null)\b)/g
  };
  function highlight(text, lang) {
    var re = RULES[lang];
    if (!re) return esc(text);
    re.lastIndex = 0;
    var out = "", last = 0, m;
    while ((m = re.exec(text))) {
      if (m[0] === "") { re.lastIndex++; continue; }
      out += esc(text.slice(last, m.index));
      var g = m.groups, cls = null;
      if (g.com != null) cls = "com";
      else if (g.str != null) cls = "str";
      else if (g.par != null) cls = "par";
      else if (g.num != null) cls = "num";
      else if (g.fn != null) cls = "fn";
      else if (g.w != null) {
        if (lang === "sql" && SQL_KW.has(m[0])) cls = "kw";
        else if (lang === "python" && PY_KW.has(m[0])) cls = "kw";
        else if (lang === "rust" || lang === "json") cls = "kw";
      }
      out += cls ? '<span class="tok-' + cls + '">' + esc(m[0]) + "</span>" : esc(m[0]);
      last = m.index + m[0].length;
    }
    return out + esc(text.slice(last));
  }
  var LANG_LABEL = { sql: "SQL", python: "Python", sh: "Shell", yaml: "YAML", json: "JSON", rust: "Rust" };
  function wrapCode(pre) {
    var lang = pre.getAttribute("data-lang");
    var text = pre.textContent.replace(/^\n/, "").replace(/\s+$/, "");
    var html = highlight(text, lang), mark = parseInt(pre.getAttribute("data-mark") || "0", 10);
    if (mark > 0) {
      // data-mark="n": light up line n (its highlighting spans never cross a line)
      html = html.split("\n").map(function (line, i) { return i === mark - 1 ? '<span class="mark-line">' + line + "</span>" : line; }).join("\n");
    }
    pre.innerHTML = "<code>" + html + "</code>";
    if (pre.parentNode && pre.parentNode.classList && pre.parentNode.classList.contains("code")) return;
    var box = document.createElement("div");
    box.className = "code";
    var head = document.createElement("div");
    head.className = "code-head";
    head.innerHTML = "<span>" + esc(pre.getAttribute("data-title") || LANG_LABEL[lang] || lang) + '</span><button class="copy" type="button">Copy</button>';
    pre.parentNode.insertBefore(box, pre);
    box.appendChild(head);
    box.appendChild(pre);
  }
  function copyText(btn, text) {
    var done = function () { btn.textContent = "Copied"; setTimeout(function () { btn.textContent = "Copy"; }, 1400); };
    var fallback = function () {
      var pre = btn.closest(".code").querySelector("pre");
      var r = document.createRange(); r.selectNodeContents(pre);
      var s = window.getSelection(); s.removeAllRanges(); s.addRange(r);
      btn.textContent = "Selected";
      setTimeout(function () { btn.textContent = "Copy"; }, 1400);
    };
    try {
      if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(text).then(done, fallback);
      else fallback();
    } catch (e) { fallback(); }
  }
  document.addEventListener("click", function (e) {
    var btn = e.target.closest && e.target.closest(".copy");
    if (!btn) return;
    var pre = btn.closest(".code").querySelector("pre");
    copyText(btn, pre.textContent);
  });

  /* ---------------------------------------------------------- tabs */
  function initTabs(root) {
    var buttons = $$("[data-tab]", root);
    var show = function (id) {
      buttons.forEach(function (b) { b.setAttribute("aria-selected", String(b.getAttribute("data-tab") === id)); });
      $$("[data-panel]", root).forEach(function (p) { p.hidden = p.getAttribute("data-panel") !== id; });
    };
    buttons.forEach(function (b) {
      b.setAttribute("role", "tab");
      b.addEventListener("click", function () { show(b.getAttribute("data-tab")); });
    });
    if (buttons.length) show(buttons[0].getAttribute("data-tab"));
  }

  /* ---------------------------------------------------------- tooltip */
  var tip = null;
  function showTip(el, x, y) {
    var t = el.getAttribute("data-tip");
    if (!t) return;
    if (!tip) { tip = document.createElement("div"); tip.className = "tip"; tip.setAttribute("role", "tooltip"); document.body.appendChild(tip); }
    tip.innerHTML = t;
    tip.hidden = false;
    var r = tip.getBoundingClientRect();
    var left = Math.min(window.innerWidth - r.width - 12, Math.max(12, x + 14));
    var top = y + 16 + r.height > window.innerHeight - 8 ? y - r.height - 12 : y + 16;
    tip.style.left = left + "px";
    tip.style.top = Math.max(8, top) + "px";
  }
  function hideTip() { if (tip) tip.hidden = true; }
  document.addEventListener("pointerover", function (e) {
    var el = e.target.closest && e.target.closest("[data-tip]");
    if (el) showTip(el, e.clientX, e.clientY); else hideTip();
  });
  document.addEventListener("pointermove", function (e) {
    var el = e.target.closest && e.target.closest("[data-tip]");
    if (el && tip && !tip.hidden) showTip(el, e.clientX, e.clientY);
  });
  document.addEventListener("focusin", function (e) {
    var el = e.target.closest && e.target.closest("[data-tip]");
    if (el) { var r = el.getBoundingClientRect(); showTip(el, r.left, r.bottom); } else hideTip();
  });
  document.addEventListener("scroll", hideTip, { passive: true });

  /* ---------------------------------------------------------- benchmarks */
  var BENCH = window.STRATA_BENCH || null;
  var STATUS_LABEL = { measured: "Measured", earlier: "Earlier run", placeholder: "Placeholder" };

  function fmt(v, digits) {
    if (digits != null) return v.toLocaleString("en-US", { minimumFractionDigits: digits, maximumFractionDigits: digits });
    var a = Math.abs(v);
    if (a >= 1000) return Math.round(v).toLocaleString("en-US");
    if (a >= 1) return v.toPrecision(3);
    return String(Number(v.toPrecision(3)));
  }
  function cellInfo(cell, row, col, chart) {
    if (cell == null) return { kind: "ph", status: "placeholder" };
    if (typeof cell === "string" && cell.indexOf("na:") === 0) return { kind: "na", why: cell.slice(3) };
    if (typeof cell === "string" && cell.indexOf("skip:") === 0) return { kind: "skip", why: cell.slice(5) };
    var o = typeof cell === "number" ? { v: cell } : cell;
    if (o.v == null) return { kind: "ph", status: "placeholder" };
    return { kind: "v", v: o.v, p90: o.p90, p99: o.p99, note: o.note, q: o.q, status: o.s || row.s || col.status || chart.status || "measured" };
  }
  function findChart(id) {
    if (!BENCH) return null;
    for (var i = 0; i < BENCH.suites.length; i++) {
      var s = BENCH.suites[i];
      for (var j = 0; j < s.charts.length; j++) if (s.charts[j].id === id) return s.charts[j];
    }
    return null;
  }
  function renderChart(chart) {
    if (chart.type === "ops") return renderOps(chart);
    var run = chart.run && BENCH.runs[chart.run];
    var infos = chart.rows.map(function (row) {
      return row.cells.map(function (c, i) { return cellInfo(c, row, chart.columns[i] || {}, chart); });
    });
    var all = [];
    infos.forEach(function (r) { r.forEach(function (c) { if (c.kind === "v" && c.v > 0) all.push(c.v); }); });
    var lo = all.length ? Math.min.apply(null, all) / 2 : 1, hi = all.length ? Math.max.apply(null, all) : 1;
    var statuses = {};
    infos.forEach(function (r) { r.forEach(function (c) { if (c.status) statuses[c.status] = true; }); });

    var h = '<div class="chart" id="' + esc(chart.id) + '">';
    h += '<div class="chart-head"><h4>' + esc(chart.title) + (chart.sub ? '<span class="sub">' + esc(chart.sub) + "</span>" : "") + "</h4>";
    h += '<div class="chart-tools">';
    ["measured", "earlier", "placeholder"].forEach(function (s) {
      if (statuses[s]) h += '<span class="tag ' + s + '">' + (s === "earlier" && run ? esc(run.label) : STATUS_LABEL[s]) + "</span>";
    });
    h += "</div></div>";
    h += '<div class="tb"><table class="tbars"><thead><tr><th scope="col"><span class="sr">Kind</span></th>';
    chart.columns.forEach(function (col) {
      h += '<th scope="col"' + (col.self ? ' class="self"' : "") + ">" + esc(col.name) + (col.ver ? "<small>" + esc(col.ver) + "</small>" : "") + "</th>";
    });
    h += "</tr></thead><tbody>";
    chart.rows.forEach(function (row, ri) {
      var r = infos[ri];
      var nums = r.filter(function (c) { return c.kind === "v"; }).map(function (c) { return c.v; });
      var rowMax = nums.length ? Math.max.apply(null, nums) : 1;
      var better = row.better || chart.better;
      var best = nums.length > 1 ? (better === "higher" ? Math.max.apply(null, nums) : Math.min.apply(null, nums)) : null;
      var unit = row.unit != null ? row.unit : (chart.unit || "");
      var digits = row.digits != null ? row.digits : chart.digits;
      h += '<tr><th scope="row">' + esc(row.label) + (row.better && row.better !== chart.better ? '<small class="dir">' + (row.better === "higher" ? "higher is better" : "lower is better") + "</small>" : "") + "</th>";
      r.forEach(function (c, ci) {
        var col = chart.columns[ci] || {};
        var self = col.self || row.self;
        var cls = "cell" + (self ? " self" : "");
        var who = esc(col.name + (col.ver ? " " + col.ver : ""));
        if (c.kind === "ph") {
          h += '<td><div class="' + cls + ' ph" tabindex="0" data-tip="<b>' + who + "</b> · " + esc(row.label) + '<br>Placeholder: no measurement yet. The laptop benchmark measures it."><span class="val">placeholder</span><span class="bar" style="width:46%"></span></div></td>';
          return;
        }
        if (c.kind === "skip") {
          h += '<td><div class="' + cls + ' na skip" tabindex="0" data-tip="<b>' + who + "</b> · " + esc(row.label) + "<br>Not run: " + esc(c.why) + '"><span class="val">not run</span></div></td>';
          return;
        }
        if (c.kind === "na") {
          h += '<td><div class="' + cls + ' na" tabindex="0" data-tip="<b>' + who + "</b> · " + esc(row.label) + "<br>Not supported: " + esc(c.why) + '"><span class="val">' + esc(c.why) + "</span></div></td>";
          return;
        }
        var pct;
        if (chart.scale === "log") pct = (Math.log(c.v) - Math.log(lo)) / (Math.log(hi) - Math.log(lo));
        else if (chart.scale === "unit") pct = c.v;
        else pct = c.v / rowMax;
        pct = Math.max(0.02, Math.min(1, pct)) * 100;
        var val = fmt(c.v, digits) + (unit ? " " + unit : "");
        var tipText = "<b>" + who + "</b> · " + esc(row.label) + "<br>" + esc(val) + (chart.unit && !unit ? " " + esc(chart.unit) : "");
        if (c.p90 != null) tipText += " · p90 " + esc(fmt(c.p90)) + " · p99 " + esc(fmt(c.p99));
        if (c.note) tipText += "<br>" + esc(c.note);
        if (c.q) tipText += "<br>" + esc(c.q);
        tipText += "<br>" + esc(c.status === "earlier" && run ? run.label : STATUS_LABEL[c.status] || "");
        h += '<td><div class="' + cls + '" tabindex="0" data-tip="' + esc(tipText) + '"><span class="val' + (c.v === best ? " best" : "") + '">' + esc(val) + (c.note ? " *" : "") + '</span><span class="bar" style="width:' + pct.toFixed(1) + '%"></span>' + (c.q ? '<span class="q">' + esc(c.q) + "</span>" : "") + "</div></td>";
      });
      h += "</tr>";
    });
    h += "</tbody></table></div>";
    h += '<div class="chart-foot"><span class="k"><i class="s"></i>ChloeDB</span><span class="k"><i></i>others</span>';
    if (statuses.placeholder) h += '<span class="k"><i class="h"></i>placeholder, no value yet</span>';
    if (chart.scale === "row") h += "<span>bars scaled within each row</span>";
    if (chart.scale === "log") h += "<span>bars on a log scale</span>";
    h += "<span>bold: best in the row</span>";
    h += "</div>";
    if (chart.note || run) {
      h += '<div class="chart-foot">';
      if (chart.note) h += "<span>" + esc(chart.note) + "</span>";
      if (run) h += "<span>" + esc(run.machine) + ". Source: " + (run.url ? '<a href="' + esc(run.url) + '"><code>' + esc(run.source) + "</code></a>" : "<code>" + esc(run.source) + "</code>") + "</span>";
      h += "</div>";
    }
    h += "</div>";
    return h;
  }
  /* An operator matrix: what each engine can run, and how fast where it
     can. A cell is "y", "p:how", "$:how", "n:why" or "c:what to check"
     (from each engine's documentation, to be confirmed by the runs), or
     {m, v, s, note} once timed, with v in the chart's unit. */
  var OPS_LABEL = { y: "Built in", p: "Partly", $: "Paid tier only", n: "Not supported", c: "To check" };
  function renderOps(chart) {
    var run = chart.run && BENCH.runs[chart.run];
    var h = '<div class="chart ops" id="' + esc(chart.id) + '">';
    h += '<div class="chart-head"><h4>' + esc(chart.title) + (chart.sub ? '<span class="sub">' + esc(chart.sub) + "</span>" : "") + "</h4>";
    h += '<div class="chart-tools"><span class="tag ' + esc(chart.status || "placeholder") + '">' + esc(chart.status === "earlier" && run ? run.label : STATUS_LABEL[chart.status || "placeholder"]) + "</span></div></div>";
    h += '<div class="matrix-wrap"><table class="matrix"><thead><tr><th class="cap" scope="col">Operator</th>';
    chart.columns.forEach(function (col) { h += '<th scope="col"' + (col.self ? ' class="self"' : "") + ">" + esc(col.name) + (col.ver ? "<small>" + esc(col.ver) + "</small>" : "") + "</th>"; });
    h += "</tr></thead><tbody>";
    chart.rows.forEach(function (row) {
      if (row.group) { h += '<tr class="grp"><td colspan="' + (chart.columns.length + 1) + '">' + esc(row.group) + "</td></tr>"; return; }
      h += '<tr><td class="cap">' + esc(row.label) + (row.data ? ' <span class="small muted">(' + esc(row.data) + ")</span>" : "") + "</td>";
      row.cells.forEach(function (cell, ci) {
        var col = chart.columns[ci] || {};
        var o = typeof cell === "string" ? { m: cell.charAt(0), note: cell.charAt(1) === ":" ? cell.slice(2) : "" } : (cell || { m: "c" });
        var code = o.m === "$" ? "paid" : o.m;
        var label = OPS_LABEL[o.m] || o.m;
        var runs = o.m === "y" || o.m === "p" || o.m === "$";
        var time = "";
        if (runs) time = o.v != null ? '<span class="t">' + esc(fmt(o.v)) + " " + esc(chart.unit || "ms") + "</span>" : '<span class="t ph">pending</span>';
        var tipText = "<b>" + esc(col.name + (col.ver ? " " + col.ver : "")) + "</b> · " + esc(row.label) + "<br>" + esc(label) + (o.note ? ": " + esc(o.note) : "") + (runs ? "<br>" + (o.v != null ? "p50 " + esc(fmt(o.v)) + " " + esc(chart.unit || "ms") : "Timing pending: the laptop benchmark runs it") : "");
        h += "<td" + (col.self ? ' class="self"' : "") + '><span class="opc" tabindex="0" data-tip="' + esc(tipText) + '"><span class="mk ' + esc(code) + '" role="img" aria-label="' + esc(label + (o.note ? ": " + o.note : "")) + '"></span>' + time + "</span></td>";
      });
      h += "</tr>";
    });
    h += "</tbody></table></div>";
    h += '<div class="legend"><span><span class="mk y"></span>built in</span><span><span class="mk p"></span>partly, or by a documented workaround</span><span><span class="mk paid"></span>paid tier only (run on a trial licence)</span><span><span class="mk n"></span>not supported</span><span><span class="mk c"></span>to check</span><span>Under each mark: the p50 where it runs.</span></div>';
    if (chart.note || run) {
      h += '<div class="chart-foot">';
      if (chart.note) h += "<span>" + esc(chart.note) + "</span>";
      if (run) h += "<span>" + esc(run.machine) + ". Source: " + (run.url ? '<a href="' + esc(run.url) + '"><code>' + esc(run.source) + "</code></a>" : "<code>" + esc(run.source) + "</code>") + "</span>";
      h += "</div>";
    }
    h += "</div>";
    return h;
  }

  /* ---------------------------------------------------------- matrix */
  var CMP = window.STRATA_COMPARE || null;
  var MARK_LABEL = { y: "Yes", p: "Partly, by extension or by hand", n: "No", $: "Paid tier only" };
  function parseMark(s) {
    if (!s) return { code: "n", verify: false, note: "" };
    var code = s.charAt(0), rest = s.slice(1), verify = false;
    if (rest.charAt(0) === "?") { verify = true; rest = rest.slice(1); }
    var note = rest.charAt(0) === ":" ? rest.slice(1) : "";
    return { code: code, verify: verify, note: note };
  }
  function renderMatrix(el) {
    if (!CMP) return;
    var state = { group: "all" };
    try { var saved = localStorage.getItem("strata-matrix-group"); if (saved) state.group = saved; } catch (e) {}
    // data-section limits the table to one section, data-systems to some columns (and drops the group filter)
    var only = el.getAttribute("data-section"), pick = el.getAttribute("data-systems");
    var picked = pick ? pick.split(",") : null;
    var sections = CMP.sections.filter(function (sec) { return !only || sec.title === only; });
    var unverified = 0;
    sections.forEach(function (sec) { sec.rows.forEach(function (r) { Object.keys(r[1]).forEach(function (k) { if ((!picked || picked.indexOf(k) >= 0) && parseMark(r[1][k]).verify) unverified++; }); }); });

    function draw() {
      var systems = CMP.systems.filter(function (s) { return picked ? picked.indexOf(s.id) >= 0 : (state.group === "all" || s.group === "self" || s.group === state.group); });
      var h = picked ? "" : '<div class="filters"><span class="label">Compare with</span>' + CMP.groups.map(function (g) {
        return '<button type="button" class="chipbtn" data-group="' + g.id + '" aria-pressed="' + (state.group === g.id) + '">' + esc(g.label) + "</button>";
      }).join("") + "</div>";
      h += '<div class="matrix-wrap"><table class="matrix"><thead><tr><th class="cap" scope="col">Capability</th>';
      systems.forEach(function (s) { h += '<th scope="col"' + (s.id === "strata" ? ' class="self"' : "") + ">" + esc(s.name) + (s.sub ? "<small>" + esc(s.sub) + "</small>" : "") + "</th>"; });
      h += "</tr></thead><tbody>";
      sections.forEach(function (sec) {
        h += '<tr class="grp"><td colspan="' + (systems.length + 1) + '">' + esc(sec.title) + "</td></tr>";
        sec.rows.forEach(function (r) {
          h += '<tr><td class="cap">' + esc(r[0]) + "</td>";
          systems.forEach(function (s) {
            var m = parseMark(r[1][s.id]);
            var cls = s.id === "strata" ? ' class="self"' : "";
            if (m.code === "t") { h += "<td" + cls + '><span class="small">' + esc(m.note) + "</span></td>"; return; }
            var code = m.code === "$" ? "paid" : m.code;
            var label = MARK_LABEL[m.code] || m.code;
            var tipText = "<b>" + esc(s.name) + "</b> · " + esc(r[0]) + "<br>" + esc(label) + (m.note ? ": " + esc(m.note) : "") + (m.verify ? "<br><i>Not yet checked against its current documentation</i>" : "");
            h += "<td" + cls + '><span class="mk ' + code + (m.verify ? " v" : "") + '" tabindex="0" role="img" aria-label="' + esc(label + (m.note ? ": " + m.note : "") + (m.verify ? " (to verify)" : "")) + '" data-tip="' + esc(tipText) + '"></span></td>';
          });
          h += "</tr>";
        });
      });
      h += "</tbody></table></div>";
      h += '<div class="legend"><span><span class="mk y"></span>built in</span><span><span class="mk p"></span>partly, by extension or by hand</span><span><span class="mk paid"></span>paid tier only</span><span><span class="mk n"></span>no</span><span><span class="mk y v"></span>' + unverified + " cells not yet checked against each project's documentation</span><span>Hover or focus a mark for the detail.</span></div>";
      el.innerHTML = h;
      $$(".chipbtn", el).forEach(function (b) {
        b.addEventListener("click", function () {
          state.group = b.getAttribute("data-group");
          try { localStorage.setItem("strata-matrix-group", state.group); } catch (e) {}
          draw();
        });
      });
    }
    draw();
  }

  /* ---------------------------------------------------------- boot */
  function boot() {
    $$("pre[data-lang]").forEach(wrapCode);
    $$("[data-tabs]").forEach(initTabs);
    $$("[data-chart]").forEach(function (el) { var c = findChart(el.getAttribute("data-chart")); if (c) el.innerHTML = renderChart(c); });
    $$("[data-matrix]").forEach(renderMatrix);
    initLightbox();
  }

  /* ---------------------------------------------------------- screenshots open larger */
  function initLightbox() {
    var imgs = $$(".shot img, .browser img");
    if (!imgs.length || typeof HTMLDialogElement === "undefined") return;
    var d = document.createElement("dialog");
    d.className = "lightbox";
    d.setAttribute("aria-label", "Screenshot");
    d.innerHTML = '<button type="button" class="close" aria-label="Close">×</button><img alt=""><p class="cap"></p>';
    document.body.appendChild(d);
    var big = d.querySelector("img"), cap = d.querySelector(".cap");
    d.querySelector(".close").addEventListener("click", function () { d.close(); });
    d.addEventListener("click", function (e) { if (e.target === d || e.target === big) d.close(); });
    imgs.forEach(function (img) {
      img.classList.add("zoomable");
      img.tabIndex = 0;
      img.setAttribute("role", "button");
      img.setAttribute("aria-label", (img.alt || "Screenshot") + ", open larger");
      var open = function () {
        var fig = img.closest("figure"), fc = fig && fig.querySelector("figcaption");
        big.src = img.currentSrc || img.src;
        big.alt = img.alt;
        cap.textContent = fc ? fc.textContent : img.alt;
        d.showModal();
      };
      img.addEventListener("click", open);
      img.addEventListener("keydown", function (e) { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); open(); } });
    });
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", boot); else boot();
})();

/* Motion: a moving strata field behind the page heads, sections that rise
 * in as they scroll into view, numbers that count up, bars that grow, a
 * reading-progress line and a light that follows the pointer on cards.
 * Everything is decoration over a page that is complete without it: with
 * prefers-reduced-motion, or without IntersectionObserver, nothing moves
 * and nothing is hidden. */
(function () {
  "use strict";
  var root = document.documentElement;
  var mq = window.matchMedia ? window.matchMedia("(prefers-reduced-motion: reduce)") : null;
  var REDUCED = !!(mq && mq.matches);
  var IO = "IntersectionObserver" in window;
  if (!REDUCED && IO) root.classList.add("motion");
  var $$ = function (sel, r) { return Array.prototype.slice.call((r || document).querySelectorAll(sel)); };

  /* ---------- the strata field: four layers in the family hues, drifting,
     with a few points that sink from one layer to the next (a query going
     through the indexes). One canvas per head band, paused off screen. */
  var HUES = ["#3987e5", "#d95926", "#199e70", "#c98500"];
  function field(band) {
    var c = document.createElement("canvas");
    c.className = "field";
    c.setAttribute("aria-hidden", "true");
    band.insertBefore(c, band.firstChild);
    var ctx = c.getContext("2d");
    if (!ctx) return;
    var hero = band.classList.contains("hero-band");
    var W = 0, H = 0, dpr = 1, t0 = performance.now(), running = false, raf = 0;
    var dots = [];
    function size() {
      var r = band.getBoundingClientRect();
      dpr = Math.min(2, window.devicePixelRatio || 1);
      W = Math.max(1, r.width); H = Math.max(1, r.height);
      c.width = Math.round(W * dpr); c.height = Math.round(H * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      var n = Math.round(Math.min(46, W / (hero ? 30 : 46)));
      dots = [];
      for (var i = 0; i < n; i++) dots.push({ x: Math.random() * W, l: i % 4, v: 0.25 + Math.random() * 0.6, fall: Math.random() < 0.18 ? Math.random() : -1, s: 1 + Math.random() * 1.6 });
    }
    // the y of layer l at x and time t: a gentle swell, layered top to bottom
    function y(l, x, t) {
      var base = H * (hero ? 0.3 : 0.34) + l * H * (hero ? 0.15 : 0.15);
      return base + Math.sin(x / (230 + l * 40) + t * (0.22 + l * 0.05) + l * 1.7) * (14 + l * 3) + Math.sin(x / 97 - t * 0.31 + l) * 5;
    }
    function draw(now) {
      var t = (now - t0) / 1000;
      ctx.clearRect(0, 0, W, H);
      for (var l = 0; l < 4; l++) {
        // a band under the line, faint, then the line
        ctx.beginPath();
        for (var x = 0; x <= W + 8; x += 8) { var yy = y(l, x, t); if (x === 0) ctx.moveTo(x, yy); else ctx.lineTo(x, yy); }
        ctx.strokeStyle = HUES[l]; ctx.globalAlpha = hero ? 0.55 : 0.4; ctx.lineWidth = 1.4; ctx.stroke();
        ctx.lineTo(W, y(l, W, t) + 36); ctx.lineTo(0, y(l, 0, t) + 36); ctx.closePath();
        ctx.globalAlpha = hero ? 0.07 : 0.05; ctx.fillStyle = HUES[l]; ctx.fill();
      }
      for (var i = 0; i < dots.length; i++) {
        var d = dots[i];
        d.x += d.v * (REDUCED ? 0 : 1);
        if (d.x > W + 10) { d.x = -10; d.l = Math.floor(Math.random() * 4); d.fall = Math.random() < 0.18 ? 0 : -1; }
        var yy0 = y(d.l, d.x, t), yy = yy0;
        if (d.fall >= 0 && d.l < 3) {
          // sinking to the next layer
          d.fall += 0.006;
          var y1 = y(d.l + 1, d.x, t);
          var k = Math.min(1, d.fall); k = k * k * (3 - 2 * k);
          yy = yy0 + (y1 - yy0) * k;
          if (d.fall >= 1) { d.l += 1; d.fall = d.l < 3 && Math.random() < 0.5 ? 0 : -1; }
        }
        ctx.globalAlpha = 0.9;
        ctx.fillStyle = HUES[d.l];
        ctx.beginPath(); ctx.arc(d.x, yy, d.s, 0, 6.283); ctx.fill();
        ctx.globalAlpha = 0.18;
        ctx.beginPath(); ctx.arc(d.x, yy, d.s * 3.2, 0, 6.283); ctx.fill();
      }
      ctx.globalAlpha = 1;
      if (running) raf = requestAnimationFrame(draw);
    }
    function start() { if (running || REDUCED) return; running = true; raf = requestAnimationFrame(draw); }
    function stop() { running = false; cancelAnimationFrame(raf); }
    size();
    draw(performance.now());
    var rt = 0;
    window.addEventListener("resize", function () { clearTimeout(rt); rt = setTimeout(function () { size(); draw(performance.now()); }, 120); });
    if (IO) new IntersectionObserver(function (es) { es[0].isIntersecting ? start() : stop(); }).observe(band); else start();
    document.addEventListener("visibilitychange", function () { if (document.hidden) stop(); else if (band.getBoundingClientRect().bottom > 0) start(); });
  }

  /* ---------- rise in as they come into view, a sibling at a time */
  var RISE = [".block-head", ".cap", ".dcard", ".lang", ".tiles > *", ".proofstrip > *", ".fact", ".pipe", ".feat", ".principles > div",
    ".split2 > *", ".uc2 > *", ".flist li", ".fam-intro", ".chart", ".showcase", ".try", ".cta > *", ".rung", ".stage", ".sim", ".filemap", ".deploy-art", ".bento > *"].join(",");
  function rise() {
    var els = $$(RISE).filter(function (el) { return !el.closest(".hero-band, .head-band, .top, .foot, [data-panel][hidden]") && el.getBoundingClientRect().top > window.innerHeight * 0.92; });
    els.forEach(function (el) {
      el.classList.add("rv");
      var sib = 0, p = el.previousElementSibling;
      while (p && sib < 8) { if (p.classList.contains("rv")) sib++; p = p.previousElementSibling; }
      el.style.setProperty("--rv-d", sib * 70 + "ms");
    });
    var io = new IntersectionObserver(function (es) {
      es.forEach(function (e) { if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); } });
    }, { rootMargin: "0px 0px -6% 0px", threshold: 0.05 });
    els.forEach(function (el) { io.observe(el); });
    // never leave anything hidden: a jump to an anchor, a print, a resize
    window.addEventListener("beforeprint", function () { els.forEach(function (el) { el.classList.add("in"); }); });
  }

  /* ---------- numbers count up to their printed value */
  var NUM = /(\d[\d,]*(?:\.\d+)?)(?!\d|st|nd|rd|th)/g;
  function countUp(el) {
    var nodes = [], w = document.createTreeWalker(el, NodeFilter.SHOW_TEXT), n;
    while ((n = w.nextNode())) if (NUM.test(n.nodeValue)) nodes.push({ n: n, text: n.nodeValue });
    NUM.lastIndex = 0;
    if (!nodes.length) return;
    var t0 = performance.now(), dur = 1100;
    function frame(now) {
      var k = Math.min(1, (now - t0) / dur), e = 1 - Math.pow(1 - k, 3);
      nodes.forEach(function (o) {
        o.n.nodeValue = k >= 1 ? o.text : o.text.replace(NUM, function (s) {
          var dec = (s.split(".")[1] || "").length, v = parseFloat(s.replace(/,/g, "")) * e;
          return s.indexOf(",") >= 0 ? Math.round(v).toLocaleString("en-US") : v.toFixed(dec);
        });
      });
      if (k < 1) requestAnimationFrame(frame);
    }
    requestAnimationFrame(frame);
  }
  function counts() {
    var els = $$(".tiles .v, .proofstrip .v, .stat .v, [data-count]");
    var io = new IntersectionObserver(function (es) {
      es.forEach(function (e) { if (e.isIntersecting) { io.unobserve(e.target); countUp(e.target); } });
    }, { threshold: 0.6 });
    els.forEach(function (el) { io.observe(el); });
  }

  /* ---------- bars grow from zero when their chart comes into view */
  function bars() {
    var els = $$(".cell .bar, .rb-bar > i, [data-rank-mini] .bar > i");
    els.forEach(function (el) { el.classList.add("gw"); });
    var io = new IntersectionObserver(function (es) {
      es.forEach(function (e) { if (e.isIntersecting) { e.target.classList.add("grown"); io.unobserve(e.target); } });
    }, { threshold: 0 });
    els.forEach(function (el) { io.observe(el); });
  }

  /* ---------- a reading line across the top, in the four hues */
  function progress() {
    var bar = document.createElement("div");
    bar.className = "readline"; bar.setAttribute("aria-hidden", "true");
    document.body.appendChild(bar);
    var tick = false;
    function set() { tick = false; var h = document.documentElement.scrollHeight - window.innerHeight; bar.style.transform = "scaleX(" + (h > 0 ? Math.min(1, window.scrollY / h) : 0) + ")"; }
    window.addEventListener("scroll", function () { if (!tick) { tick = true; requestAnimationFrame(set); } }, { passive: true });
    set();
  }

  /* ---------- a light that follows the pointer over a card */
  function spotlight() {
    if (!window.matchMedia || !window.matchMedia("(hover: hover)").matches) return;
    document.addEventListener("pointermove", function (e) {
      var el = e.target.closest && e.target.closest(".cap, .dcard, .lang, .fact, .tiles > a, .proofstrip > a, .latest, .hl");
      if (!el) return;
      var r = el.getBoundingClientRect();
      el.style.setProperty("--mx", (e.clientX - r.left) + "px");
      el.style.setProperty("--my", (e.clientY - r.top) + "px");
    }, { passive: true });
  }

  /* ---------- the statement on the home page: its four parts lit in turn,
     or the one under the pointer */
  function statement() {
    var list = document.querySelector(".checks");
    var code = list && list.closest(".split2") && list.closest(".split2").querySelector(".code");
    if (!code) return;
    var fams = ["text", "cols", "vec", "graph"], i = 0, timer = 0, held = false;
    function light(f) {
      fams.forEach(function (g) {
        code.classList.toggle("hl-" + g, g === f);
        var li = list.querySelector("li." + g); if (li) li.classList.toggle("on", g === f);
      });
    }
    function step() { if (!held) { light(fams[i % 4]); i++; } }
    $$("li", list).forEach(function (li) {
      var f = fams.filter(function (g) { return li.classList.contains(g); })[0];
      li.tabIndex = 0;
      li.addEventListener("pointerenter", function () { held = true; light(f); });
      li.addEventListener("focus", function () { held = true; light(f); });
      li.addEventListener("pointerleave", function () { held = false; });
      li.addEventListener("blur", function () { held = false; });
    });
    if (REDUCED || !IO) return;
    new IntersectionObserver(function (es) {
      if (es[0].isIntersecting) { if (!timer) { step(); timer = setInterval(step, 2200); } }
      else { clearInterval(timer); timer = 0; light(null); }
    }, { threshold: 0.4 }).observe(code);
  }

  /* ---------- a query's six steps (Under the hood): a pulse runs through them */
  function pipe() {
    var p = document.querySelector("ol.pipe");
    if (!p || REDUCED || !IO) return;
    var li = $$("li", p), i = 0, timer = 0;
    function step() { li.forEach(function (x, j) { x.classList.toggle("on", j === i % li.length); x.classList.toggle("done", j < i % li.length); }); i++; }
    new IntersectionObserver(function (es) {
      if (es[0].isIntersecting) { if (!timer) { p.classList.add("live"); step(); timer = setInterval(step, 1100); } }
      else { clearInterval(timer); timer = 0; }
    }, { threshold: 0.3 }).observe(p);
  }

  /* ---------- the home hero: what ChloeDB can do, one statement at a time.
     Arrows and dots always work; it plays by itself only when motion is
     welcome, and holds while the pointer or focus is on it */
  function says() {
    var root = document.querySelector(".says");
    if (!root) return;
    var items = $$(".say", root), dots = root.querySelector(".says-dots"), i = 0, timer = 0, held = false, seen = true;
    var DWELL = 4200;
    items.forEach(function (it, j) {
      it.setAttribute("role", "group"); it.setAttribute("aria-roledescription", "slide"); it.setAttribute("aria-label", (j + 1) + " of " + items.length);
      var d = document.createElement("button");
      d.type = "button"; d.setAttribute("aria-label", "Statement " + (j + 1) + " of " + items.length);
      d.className = "f-" + (it.getAttribute("data-f") || "text");
      d.addEventListener("click", function () { go(j, true); });
      dots.appendChild(d);
    });
    function go(j, user) {
      var prev = i; i = (j + items.length) % items.length;
      items.forEach(function (it, k) { it.classList.toggle("on", k === i); it.classList.toggle("out", k === prev && k !== i); it.setAttribute("aria-hidden", k === i ? "false" : "true"); });
      $$("button", dots).forEach(function (d, k) { d.classList.toggle("on", k === i); d.setAttribute("aria-current", k === i ? "true" : "false"); });
      if (user) restart();
    }
    function restart() {
      clearInterval(timer); timer = 0;
      // start the lit dot's fill again
      var on = dots.querySelector(".on"); if (on) { on.classList.toggle("on", false); void on.offsetWidth; on.classList.toggle("on", true); }
      if (REDUCED || held || !seen) { root.classList.add("paused"); return; }
      root.classList.toggle("paused", false);
      timer = setInterval(function () { go(i + 1); restart(); }, DWELL);
    }
    root.querySelector(".says-prev").addEventListener("click", function () { go(i - 1, true); });
    root.querySelector(".says-next").addEventListener("click", function () { go(i + 1, true); });
    root.addEventListener("pointerenter", function () { held = true; restart(); });
    root.addEventListener("pointerleave", function () { held = false; restart(); });
    root.addEventListener("focusin", function () { held = true; restart(); });
    root.addEventListener("focusout", function () { held = false; restart(); });
    root.addEventListener("keydown", function (e) {
      if (e.key === "ArrowRight") { go(i + 1, true); e.preventDefault(); }
      if (e.key === "ArrowLeft") { go(i - 1, true); e.preventDefault(); }
    });
    document.addEventListener("visibilitychange", function () { seen = !document.hidden; restart(); });
    if (IO) new IntersectionObserver(function (es) { seen = es[0].isIntersecting; restart(); }, { threshold: 0.2 }).observe(root);
    root.style.setProperty("--dwell", DWELL + "ms");
    root.classList.add("ready");
    go(0); restart();
  }

  function boot() {
    $$(".hero-band, .head-band").forEach(field);
    statement();
    says();
    if (REDUCED || !IO) return;
    // after the other scripts have drawn their charts
    requestAnimationFrame(function () { rise(); counts(); bars(); pipe(); });
    progress();
    spotlight();
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", boot); else boot();
})();
