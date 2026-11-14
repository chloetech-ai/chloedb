// What an app holds when it opens: the person's attached data (sheets of a
// workbook given as CSV text in the request), imported whole and typed, and
// example rows that line up across tables; and the figures a build checks
// against them (totals, top-N, group sums, aging buckets), computed here in
// plain code from the rows, apart from the screens' own SQL.

// ---- CSV and attached sheets

// RFC 4180-ish: commas, quotes ("" inside quotes), CRLF or LF.
export function parseCsv(text) {
  const rows = [];
  let row = [];
  let cell = '';
  let q = false;
  const s = String(text || '');
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (q) {
      if (c === '"' && s[i + 1] === '"') { cell += '"'; i++; } else if (c === '"') q = false; else cell += c;
    } else if (c === '"' && cell === '') q = true;
    else if (c === ',') { row.push(cell); cell = ''; } else if (c === '\n' || c === '\r') {
      if (c === '\r' && s[i + 1] === '\n') i++;
      row.push(cell); rows.push(row); row = []; cell = '';
    } else cell += c;
  }
  if (cell !== '' || row.length) { row.push(cell); rows.push(row); }
  return rows.filter((r) => r.some((x) => String(x).trim() !== ''));
}

export const snake = (s) => String(s || '').normalize('NFKD').replace(/[̀-ͯ]/g, '').toLowerCase()
  .replace(/&/g, ' and ').replace(/[^a-z0-9]+/g, '_').replace(/^_+|_+$/g, '').replace(/^(\d)/, 'c_$1') || 'col';

// The sheets in a request: blocks headed `Sheet "Name":` followed by CSV
// lines, up to a blank line. A request with none gives [].
export function parseAttached(text) {
  const out = [];
  const lines = String(text || '').split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const m = lines[i].match(/^\s*Sheet\s+["“']?(.+?)["”']?\s*:\s*$/i);
    if (!m) continue;
    const body = [];
    let j = i + 1;
    for (; j < lines.length && lines[j].trim() !== ''; j++) body.push(lines[j]);
    i = j;
    const grid = parseCsv(body.join('\n'));
    if (grid.length < 1 || grid[0].length < 1) continue;
    const headers = grid[0].map((h, k) => String(h).trim() || `Column ${k + 1}`);
    const rows = grid.slice(1).map((r) => Object.fromEntries(headers.map((h, k) => [h, r[k] === undefined ? '' : String(r[k]).trim()])));
    out.push({ name: m[1].trim(), headers, rows, columns: headers.map((h) => ({ header: h, name: snake(h), ...inferKind(rows.map((r) => r[h])) })) });
  }
  return out;
}

// ---- types of a sheet's columns

const CURRENCY = { '£': 'GBP', '$': 'USD', '€': 'EUR', '¥': 'JPY', '₹': 'INR' };
const MONEY_RE = /^\s*(-)?\s*([£$€¥₹])?\s*(-)?\s*(\d[\d,]*(?:\.\d+)?|\.\d+)\s*([£$€¥₹]|EUR|USD|GBP)?\s*$/i;
const NUM_RE = /^\s*-?(\d{1,3}(,\d{3})+|\d+)(\.\d+)?\s*$/;
const DAY_RE = /^\d{4}-\d{2}-\d{2}$/;
const ISO_RE = /^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}/;
const SLASH_DAY_RE = /^(\d{1,2})\/(\d{1,2})\/(\d{4})$/;

// What a column of raw values is: kind (money, number, int, date, datetime,
// bool, label, text), the column type for it, and a currency for money.
export function inferKind(values) {
  const v = values.map((x) => String(x ?? '').trim()).filter((x) => x !== '');
  if (!v.length) return { kind: 'label', type: 'keyword' };
  const all = (re) => v.every((x) => re.test(x));
  if (all(DAY_RE) || all(SLASH_DAY_RE)) return { kind: 'date', type: 'keyword' };
  if (all(ISO_RE)) return { kind: 'datetime', type: 'timestamp' };
  // A leading zero ("07700") is an identifier, not a number.
  if (v.some((x) => /^0\d/.test(x))) return { kind: 'label', type: 'keyword' };
  if (all(NUM_RE)) return v.some((x) => /\./.test(x)) ? { kind: 'number', type: 'float' } : { kind: 'int', type: 'int' };
  if (all(MONEY_RE) && v.some((x) => /[£$€¥₹]|EUR|USD|GBP/i.test(x))) {
    const sym = v.map((x) => (x.match(/[£$€¥₹]/) || [])[0] || (x.match(/EUR|USD|GBP/i) || [])[0]).find(Boolean);
    return { kind: 'money', type: 'float', currency: CURRENCY[sym] || String(sym).toUpperCase() };
  }
  if (all(/^(yes|no|true|false|y|n)$/i)) return { kind: 'bool', type: 'bool' };
  const long = v.some((x) => x.length > 60 || x.split(/\s+/).length > 7);
  return long ? { kind: 'text', type: 'text' } : { kind: 'label', type: 'keyword' };
}

// A raw cell as the value to store in a column of `type`.
export function toValue(raw, type) {
  const s = String(raw ?? '').trim();
  if (s === '') return null;
  if (type === 'float' || type === 'int') {
    const m = s.match(MONEY_RE);
    const n = m ? Number(`${m[1] || m[3] || ''}${m[4].replace(/,/g, '')}`) : Number(s.replace(/,/g, ''));
    if (!Number.isFinite(n)) return null;
    return type === 'int' ? Math.round(n) : n;
  }
  if (type === 'bool') return /^(yes|true|y|1)$/i.test(s);
  if (type === 'timestamp') {
    const d = SLASH_DAY_RE.test(s) ? new Date(s.replace(SLASH_DAY_RE, '$3-$2-$1')) : new Date(s);
    return Number.isNaN(d.getTime()) ? null : d.toISOString();
  }
  if (type === 'keyword[]') return s.split(/\s*[,;]\s*/).filter(Boolean);
  if (type === 'keyword' && SLASH_DAY_RE.test(s)) { const [, d, m, y] = s.match(SLASH_DAY_RE); return `${y}-${m.padStart(2, '0')}-${d.padStart(2, '0')}`; }
  return s;
}

// The attached data as the model reads it before planning.
export function dataPart(sheets) {
  if (!sheets.length) return '';
  const cols = (s) => s.columns.map((c) => `${c.header} (${c.kind}${c.currency ? `, ${c.currency}` : ''} -> ${c.type})`).join(', ');
  return `# The attached data

The request attaches ${sheets.length === 1 ? 'a sheet' : `${sheets.length} sheets`}. The engine imports every row of every sheet itself, typed, once the tables exist; never copy these rows into "seed".
${sheets.map((s) => `- "${s.name}": ${s.rows.length} row${s.rows.length === 1 ? '' : 's'}; columns ${cols(s)}`).join('\n')}

- Give each sheet a table in "tables", with a column for every one of its columns (named as the header in snake_case unless you map it otherwise) and of the type shown: money and numbers as float or int (never keyword, so sums and formats work), days as keyword "YYYY-MM-DD". Then say which table each sheet goes to: "import": {"Sheet name": "table"} (or {"Sheet name": {"table": "t", "columns": {"Header": "column"}}} when a column is named otherwise). Show money in the sheet's currency: app.fmt.money(n, '${sheets.flatMap((s) => s.columns).find((c) => c.currency)?.currency || 'USD'}').
- Every figure a screen computes from these rows (a total, a count, a top N, a sum per group, aging buckets) is declared in "figures" (see the plan's fields), so the engine checks the screen's query against the rows.`;
}

// ---- which table each sheet goes to

const plural = (s) => [s, `${s}s`, s.replace(/s$/, ''), s.replace(/es$/, ''), s.replace(/y$/, 'ies'), s.replace(/ies$/, 'y')];

// Maps sheets to the plan's tables (plan.import, else by name, else by the
// columns they share) and fixes the plan's new tables so every column of a
// sheet has a column of a type that holds it. Returns {imports, notes,
// problems}: imports [{sheet, table, map: [{header, column, type}]}].
export function planImports(plan, sheets, existing = new Map()) {
  const notes = [];
  const problems = [];
  const imports = [];
  if (!sheets.length) return { imports, notes, problems };
  const tables = new Map();
  for (const [name, spec] of existing) tables.set(name, { name, columns: (spec.columns || []).map((c) => ({ ...c })), old: true });
  for (const t of plan.tables || []) {
    if (!t || !t.name) continue;
    const had = tables.get(t.name);
    if (had) { for (const c of t.columns || []) if (!had.columns.some((x) => x.name === c.name)) had.columns.push(c); had.planned = t; } else tables.set(t.name, { name: t.name, columns: t.columns || [], planned: t, old: false });
  }
  const declared = plan.import && typeof plan.import === 'object' ? plan.import : {};
  const used = new Set();
  for (const sheet of sheets) {
    let want = declared[sheet.name] ?? Object.entries(declared).find(([k]) => snake(k) === snake(sheet.name))?.[1];
    let tableName = typeof want === 'string' ? want : want && typeof want === 'object' ? want.table : null;
    const colMap = want && typeof want === 'object' && want.columns && typeof want.columns === 'object' ? want.columns : {};
    if (!tableName || !tables.has(tableName)) {
      const names = plural(snake(sheet.name));
      tableName = [...tables.keys()].find((n) => !used.has(n) && names.some((x) => x === n || plural(n).includes(x)));
    }
    if (!tableName) {
      // The table sharing the most column names with the sheet.
      let best = null;
      for (const t of tables.values()) {
        if (used.has(t.name)) continue;
        const shared = sheet.columns.filter((c) => t.columns.some((x) => x.name === c.name || x.name === colMap[c.header])).length;
        if (shared >= Math.max(2, Math.ceil(sheet.columns.length / 2)) && (!best || shared > best.n)) best = { n: shared, name: t.name };
      }
      tableName = best?.name || null;
    }
    if (!tableName) { problems.push(`import: the attached sheet "${sheet.name}" has no table: give it one in "tables" and say so in "import": {"${sheet.name}": "table"}`); continue; }
    used.add(tableName);
    const t = tables.get(tableName);
    const map = [];
    for (const c of sheet.columns) {
      const named = colMap[c.header];
      let col = t.columns.find((x) => x.name === named) || t.columns.find((x) => x.name === c.name)
        || t.columns.find((x) => x.name === `${c.name}_date` || x.name === c.name.replace(/_date$/, '') || x.name.replace(/_/g, '') === c.name.replace(/_/g, ''));
      if (!col) {
        if (t.old && !t.planned) { problems.push(`import: the table ${t.name} has no column for "${c.header}" of the sheet "${sheet.name}"`); continue; }
        col = { name: c.name === 'id' || c.name === 'created' ? `${c.name}_${snake(sheet.name)}` : c.name, type: c.type };
        const target = t.planned;
        target.columns = [...(target.columns || []), col];
        t.columns.push(col);
        notes.push(`added ${col.name} (${col.type}) to ${t.name} for the sheet's "${c.header}"`);
      } else if (!t.old && ['money', 'number', 'int'].includes(c.kind) && !['float', 'int'].includes(col.type)) {
        notes.push(`${t.name}.${col.name} holds ${c.kind === 'money' ? 'money' : 'numbers'}: made it ${c.type}, not ${col.type}`);
        col.type = c.type;
        const pc = (t.planned.columns || []).find((x) => x.name === col.name);
        if (pc) pc.type = c.type;
      } else if (!t.old && c.kind === 'int' && col.type === 'int' && sheet.rows.some((r) => /\./.test(r[c.header]))) {
        col.type = 'float';
      } else if (!t.old && c.kind === 'date' && !['keyword', 'timestamp'].includes(col.type)) {
        notes.push(`${t.name}.${col.name} holds days: made it keyword ("YYYY-MM-DD"), not ${col.type}`);
        col.type = 'keyword';
        const pc = (t.planned.columns || []).find((x) => x.name === col.name);
        if (pc) pc.type = 'keyword';
      }
      map.push({ header: c.header, column: col.name, type: col.type });
    }
    imports.push({ sheet: sheet.name, table: tableName, map, rows: sheet.rows });
  }
  return { imports, notes, problems };
}

// An import's rows as fields to insert.
export const importRows = (imp) => imp.rows.map((r) => Object.fromEntries(imp.map.map((m) => [m.column, toValue(r[m.header], m.type)]).filter(([, v]) => v !== null)));

// ---- example rows that line up

const NAME_KEYS = ['name', 'title', 'label', 'code', 'email', 'number', 'subject'];
const singular = (s) => s.replace(/ies$/, 'y').replace(/(ss|ch|sh|x)es$/, '$1').replace(/s$/, '');

// Makes a plan's seed coherent: rows of a table other rows refer to get ids,
// references (a column named after another table: project, project_id) name
// those ids (a name or title is turned into the row's id; anything else is
// spread over the rows), and dates are moved near today when the model put
// them far away. Returns notes of what it changed.
export function alignSeed(seed, tables, today = new Date().toISOString().slice(0, 10)) {
  const notes = [];
  if (!seed || typeof seed !== 'object') return notes;
  const names = Object.keys(seed).filter((t) => Array.isArray(seed[t]));
  const refsOf = (col) => names.find((t) => [t, singular(t), `${singular(t)}_id`, `${t}_id`].includes(col));
  const cols = (t) => (tables.find((x) => x.name === t)?.columns || []);
  const targets = new Set();
  for (const t of names) for (const c of cols(t)) { const r = refsOf(c.name); if (r && r !== t && ['keyword', 'text', undefined].includes(c.type)) targets.add(r); }
  for (const r of targets) seed[r].forEach((row, i) => { if (row && typeof row === 'object' && (row.id === undefined || row.id === null || row.id === '')) row.id = `${singular(r).replace(/_/g, '-')}-${i + 1}`; });
  for (const t of names) {
    for (const c of cols(t)) {
      const r = refsOf(c.name);
      if (!r || r === t || !targets.has(r) || !seed[r].length) continue;
      const ids = seed[r].map((x) => String(x.id));
      let k = 0;
      let fixed = 0;
      for (const row of seed[t]) {
        if (!row || typeof row !== 'object') continue;
        const v = row[c.name];
        if (v !== undefined && v !== null && ids.includes(String(v))) continue;
        const byName = v == null ? null : seed[r].find((x) => NAME_KEYS.some((n) => typeof x[n] === 'string' && x[n].trim().toLowerCase() === String(v).trim().toLowerCase()));
        row[c.name] = byName ? String(byName.id) : ids[k++ % ids.length];
        fixed++;
      }
      if (fixed) notes.push(`${fixed} example ${t} now refer to ${r} by id`);
    }
  }
  // Dates: the middle one near today.
  const DATE = /^(\d{4}-\d{2}-\d{2})(T.*)?$/;
  const days = [];
  for (const t of names) for (const row of seed[t]) if (row && typeof row === 'object') for (const v of Object.values(row)) if (typeof v === 'string' && DATE.test(v)) days.push(Date.parse(`${v.slice(0, 10)}T00:00:00Z`));
  if (days.length) {
    days.sort((a, b) => a - b);
    const mid = days[Math.floor(days.length / 2)];
    const now = Date.parse(`${today}T00:00:00Z`);
    const shift = Math.round((now - mid) / 86400000);
    if (Math.abs(shift) > 45) {
      for (const t of names) for (const row of seed[t]) if (row && typeof row === 'object') {
        for (const [k, v] of Object.entries(row)) {
          const m = typeof v === 'string' && v.match(DATE);
          if (!m) continue;
          const d = new Date(Date.parse(`${m[1]}T00:00:00Z`) + shift * 86400000).toISOString().slice(0, 10);
          row[k] = `${d}${m[2] || ''}`;
        }
      }
      notes.push(`example dates moved ${shift} days, to center on today`);
    }
  }
  return notes;
}

// ---- figures: what a screen should show, computed from the rows

const num = (v) => (typeof v === 'number' ? v : typeof v === 'string' && v.trim() !== '' && Number.isFinite(Number(v)) ? Number(v) : null);
const same = (a, b) => Math.abs(a - b) <= Math.max(0.011, Math.abs(b) * 0.005);
const lc = (v) => String(v ?? '').trim().toLowerCase();

const match = (row, where) => Object.entries(where || {}).every(([k, w]) => {
  const v = lc(row[k]);
  if (w && typeof w === 'object' && !Array.isArray(w)) {
    if (w.not !== undefined) return ![].concat(w.not).map(lc).includes(v);
    return true;
  }
  return [].concat(w).map(lc).includes(v);
});

const measure = (fig) => (fig.count ? ['count', null] : ['sum', 'avg', 'min', 'max'].map((k) => [k, fig[k]]).find(([, c]) => typeof c === 'string') || ['count', null]);

function aggregate(rows, how, col) {
  if (how === 'count') return rows.length;
  const xs = rows.map((r) => num(r[col])).filter((x) => x !== null);
  if (!xs.length) return 0;
  if (how === 'sum') return xs.reduce((a, b) => a + b, 0);
  if (how === 'avg') return xs.reduce((a, b) => a + b, 0) / xs.length;
  return how === 'min' ? Math.min(...xs) : Math.max(...xs);
}

// The problems with a figure's declaration, against the plan's columns.
export function checkFigure(fig, columnsOf) {
  const where = `figures (${fig?.label || '?'})`;
  if (!fig || typeof fig !== 'object' || typeof fig.table !== 'string') return [`${where}: give "table"`];
  const cols = columnsOf(fig.table);
  if (!cols) return [`${where}: no table ${fig.table}`];
  const out = [];
  const [, col] = measure(fig);
  for (const c of [col, fig.by, fig.age?.of, ...Object.keys(fig.where || {})].filter((x) => typeof x === 'string')) if (!cols.has(c)) out.push(`${where}: ${fig.table} has no column ${c}`);
  return out;
}

// The figure worked out from rows: {kind: 'one', value} | {kind: 'groups',
// groups: [{key, value}]} | {kind: 'buckets', buckets: [{range, value}]}.
export function computeFigure(fig, rows, today = new Date().toISOString().slice(0, 10)) {
  const [how, col] = measure(fig);
  const kept = rows.filter((r) => match(r, fig.where));
  if (fig.age && typeof fig.age.of === 'string' && Array.isArray(fig.age.buckets)) {
    const now = Date.parse(`${today}T00:00:00Z`);
    const past = (r) => { const t = Date.parse(`${String(r[fig.age.of] || '').slice(0, 10)}T00:00:00Z`); return Number.isNaN(t) ? null : Math.round((now - t) / 86400000); };
    const buckets = fig.age.buckets.map((b) => ({ range: b, rows: [] }));
    for (const r of kept) {
      const d = past(r);
      if (d === null) continue;
      const b = buckets.find(({ range: [lo, hi] }) => (lo == null || d >= lo) && (hi == null || d <= hi));
      if (b) b.rows.push(r);
    }
    return { kind: 'buckets', buckets: buckets.map((b) => ({ range: b.range, value: aggregate(b.rows, how, col), n: b.rows.length })) };
  }
  if (typeof fig.by === 'string') {
    const groups = new Map();
    for (const r of kept) { const k = String(r[fig.by] ?? ''); if (!groups.has(k)) groups.set(k, []); groups.get(k).push(r); }
    let out = [...groups].map(([key, rs]) => ({ key, value: aggregate(rs, how, col) }));
    if (fig.top) out = out.sort((a, b) => b.value - a.value).slice(0, Number(fig.top));
    return { kind: 'groups', groups: out };
  }
  return { kind: 'one', value: aggregate(kept, how, col) };
}

const nums = (row) => Object.values(row || {}).map(num).filter((x) => x !== null);
const fmt = (n) => (Math.round(n * 100) / 100).toLocaleString('en-US');

// Whether a screen's data rows give the figure; a problem in words, or null.
export function compareFigure(fig, expected, rows) {
  const label = `"${fig.label || fig.data}"`;
  rows = Array.isArray(rows) ? rows : [];
  if (expected.kind === 'one') {
    if (!rows.length) return `${label} should be ${fmt(expected.value)} (worked out from the rows), but the query gives no row`;
    return nums(rows[0]).some((x) => same(x, expected.value)) ? null : `${label} should be ${fmt(expected.value)} (worked out from the rows), but the query gives ${JSON.stringify(rows[0]).slice(0, 200)}`;
  }
  if (expected.kind === 'groups') {
    const missing = expected.groups.filter((g) => !rows.some((r) => Object.values(r).some((v) => lc(v) === lc(g.key)) && nums(r).some((x) => same(x, g.value))));
    const extra = fig.top && rows.length > expected.groups.length ? `it gives ${rows.length} rows, not the top ${fig.top}` : '';
    if (!missing.length && !extra) {
      // The biggest first, when a top N is asked.
      if (fig.top) {
        const order = rows.map((r) => expected.groups.findIndex((g) => Object.values(r).some((v) => lc(v) === lc(g.key)))).filter((i) => i >= 0);
        const vals = order.map((i) => expected.groups[i].value);
        if (vals.some((v, i) => i && v > vals[i - 1] + 1e-9)) return `${label}: the rows are not the biggest first`;
      }
      return null;
    }
    return `${label} should give ${expected.groups.map((g) => `${g.key}: ${fmt(g.value)}`).join(', ')} (worked out from the rows)${missing.length ? `; the query lacks ${missing.slice(0, 4).map((g) => `${g.key} ${fmt(g.value)}`).join(', ')}` : ''}${extra ? `; ${extra}` : ''}; it gives ${JSON.stringify(rows.slice(0, 6)).slice(0, 300)}`;
  }
  const all = rows.flatMap(nums);
  const missing = expected.buckets.filter((b) => b.value !== 0 && !all.some((x) => same(x, b.value)));
  if (!missing.length) return null;
  const range = ([lo, hi]) => (lo == null ? `up to ${hi}` : hi == null ? `${lo}+` : `${lo}-${hi}`);
  return `${label} should give, by days past ${fig.age.of}: ${expected.buckets.map((b) => `${range(b.range)}: ${fmt(b.value)}`).join(', ')} (each row in exactly one range, worked out from the rows); the query gives ${JSON.stringify(rows.slice(0, 6)).slice(0, 300)}`;
}

// The numbers written in a page's text ("$5,300.00", "5.300 €", "28 €", "1.2k").
export function numbersIn(text) {
  const out = [];
  for (const m of String(text || '').matchAll(/-?\d[\d.,\s]*\d|-?\d/g)) {
    let s = m[0].replace(/\s(?=\d{3}\b)/g, '').trim();
    const after = String(text).slice(m.index + m[0].length, m.index + m[0].length + 2);
    const tail = /^[kK]\b/.test(after) ? 1e3 : /^M\b/.test(after) ? 1e6 : 1;
    const cands = [];
    if (/^-?\d{1,3}(,\d{3})+(\.\d+)?$/.test(s)) cands.push(Number(s.replace(/,/g, '')));
    else if (/^-?\d{1,3}(\.\d{3})+(,\d+)?$/.test(s)) cands.push(Number(s.replace(/\./g, '').replace(',', '.')));
    else if (/^-?\d+,\d{1,2}$/.test(s)) cands.push(Number(s.replace(',', '.')));
    else cands.push(Number(s.replace(/,/g, '')));
    for (const n of cands) if (Number.isFinite(n)) out.push(n * tail);
  }
  return out;
}

// What of a figure a page's text does not show: [] when it shows it all.
export function figureNotShown(expected, text) {
  const ns = numbersIn(text);
  const shows = (v) => ns.some((x) => same(x, v) || Math.round(x) === Math.round(v) || (Math.abs(v) >= 1000 && Math.abs(x - v / 1000) < 0.06 && /\dk\b/i.test(text)));
  const t = lc(text);
  if (expected.kind === 'one') return shows(expected.value) ? [] : [fmt(expected.value)];
  if (expected.kind === 'groups') return expected.groups.filter((g) => !(t.includes(lc(g.key)) && shows(g.value))).map((g) => `${g.key} ${fmt(g.value)}`);
  return expected.buckets.filter((b) => b.value !== 0 && !shows(b.value)).map((b) => fmt(b.value));
}

// ---- a build's data: what the forge imports, seeds and checks

// Readies a plan's data before its tables are stored: each attached sheet
// gets a table (one is added when the plan gave none), the new tables'
// columns hold the sheets' values, the figures name real columns, and the
// example rows line up. Returns {imports: [{table, rows}], figures, notes}.
export function prepareData(plan, view, sheets, today = new Date().toISOString().slice(0, 10)) {
  const notes = [];
  const existing = new Map((view?.tables || []).map((t) => [t.spec.name, t.spec]));
  const first = !view?.comp?.n;
  plan.tables = Array.isArray(plan.tables) ? plan.tables : [];
  let imports = [];
  if (sheets.length) {
    let r = planImports(plan, sheets, existing);
    const orphans = sheets.filter((s) => !r.imports.some((i) => i.sheet === s.name));
    for (const s of orphans) {
      let name = snake(s.name);
      while (existing.has(name) || plan.tables.some((t) => t.name === name)) name = `${name}_rows`;
      plan.tables.push({ name, description: `The rows of the attached sheet "${s.name}"`, columns: s.columns.map((c) => ({ name: c.name, type: c.type })) });
      plan.import = { ...(plan.import && typeof plan.import === 'object' ? plan.import : {}), [s.name]: name };
      notes.push(`The sheet "${s.name}" had no table: it goes to a new table ${name}`);
    }
    if (orphans.length) r = planImports(plan, sheets, existing);
    notes.push(...r.notes.map((n) => `Attached data: ${n}`));
    // Only into tables this version creates: rows already kept stay as they are.
    imports = r.imports.filter((i) => !existing.has(i.table)).map((i) => ({ table: i.table, sheet: i.sheet, rows: importRows(i) }));
    if (imports.length) notes.push(`Attached data: ${imports.map((i) => `${i.rows.length} row${i.rows.length === 1 ? '' : 's'} of "${i.sheet}" into ${i.table}`).join(', ')}`);
  }
  // The columns of every table, after this version.
  const cols = new Map([...existing].map(([n, s]) => [n, new Set((s.columns || []).map((c) => c.name).concat(['id', 'created']))]));
  for (const t of plan.tables) {
    if (!t || !t.name) continue;
    if (!cols.has(t.name)) cols.set(t.name, new Set(['id', 'created']));
    for (const c of t.columns || []) cols.get(t.name).add(c.name);
  }
  const figures = [];
  for (const f of Array.isArray(plan.figures) ? plan.figures : []) {
    const bad = checkFigure(f, (t) => cols.get(t) || null);
    if (bad.length) notes.push(`A figure is left unchecked: ${bad[0]}`); else figures.push(f);
  }
  plan.figures = figures;
  if (first && plan.seed && typeof plan.seed === 'object') {
    for (const t of Object.keys(plan.seed)) {
      if (imports.some((i) => i.table === t)) { delete plan.seed[t]; continue; }
      if (!cols.has(t) || !Array.isArray(plan.seed[t])) { delete plan.seed[t]; continue; }
      // A column the table lacks would refuse the row: dropped from it.
      plan.seed[t] = plan.seed[t].filter((r) => r && typeof r === 'object').map((r) => Object.fromEntries(Object.entries(r).filter(([k]) => cols.get(t).has(k))));
    }
    notes.push(...alignSeed(plan.seed, plan.tables, today).map((n) => `Example rows: ${n}`));
  }
  return { imports, figures, notes };
}

// The tables a SELECT reads (after FROM and JOIN).
export const tablesIn = (sql) => [...String(sql || '').matchAll(/\b(?:FROM|JOIN)\s+([A-Za-z_][A-Za-z0-9_]*)/gi)].map((m) => m[1].toLowerCase());

// Whether visitors (people not signed in) may read a table, from its ALLOW
// statements (none: open to whoever reaches the app).
export function visitorsRead(access) {
  const list = Array.isArray(access) ? access : [];
  if (!list.length) return true;
  return list.some((s) => /^\s*ALLOW\s+(.+?)\s+ON\s+\S+\s+TO\s+(everyone|public|visitors|anyone)\b(?!.*\bWHERE\b)/i.test(s) && /\b(READ|SELECT|ALL)\b/i.test(s.split(/\s+ON\s+/i)[0]));
}

// The sign-in entry a screen shows a visitor when the data it reads is kept
// to signed-in people: {title, text, points, note}, or null when it needs none.
export function gateOf(screen, accessOf, { name = '', welcome = null, ux = null, title = '' } = {}) {
  if (!screen || typeof screen !== 'object' || screen.page === 'front') return null;
  const reads = new Set();
  for (const q of Object.values(screen.data || {})) {
    // (A view written in JavaScript comes as {data: {code}}: its SQL strings are read the same way.)
    const sql = typeof q === 'string' ? q : q && typeof q === 'object' ? q.sql : null;
    if (typeof sql === 'string') for (const t of tablesIn(sql)) reads.add(t);
  }
  const closed = [...reads].filter((t) => accessOf(t) && !visitorsRead(accessOf(t)));
  if (!closed.length) return null;
  const w = welcome && typeof welcome === 'object' ? welcome : {};
  const points = (Array.isArray(w.points) ? w.points : []).map(String).filter(Boolean).slice(0, 4);
  return {
    title: String(w.title || name || title || 'Sign in').slice(0, 80),
    text: String(w.text || ux?.job || ux?.glance || `${title ? `${title}: ` : ''}sign in to see it.`).slice(0, 300),
    points,
    note: 'Sign in with your account, or create one.',
    // Names the component it draws, so the app pins it (graph.mjs sharedRefs).
    ui: 'signIn',
  };
}

// The name a request gives the app's main heading or the app itself
// ("Rename the main heading to Northwind at a glance", "call the app X"),
// or null. The header shows the app's name, so such a request renames it.
export function renameIn(text) {
  const s = String(text || '');
  const head = "(?:the\\s+|my\\s+|its\\s+)?(?:(?:main|top|page|app'?s?|site'?s?|dashboard'?s?)\\s+)?(?:heading|title|header|app|site|dashboard|name)";
  const tail = '["“\']?([^"”\'\\n]{2,40}?)["”\']?\\s*(?:[.!]|$)';
  const m = s.match(new RegExp(`\\b(?:rename|retitle|change)\\s+${head}\\s+(?:to|as|:)\\s+${tail}`, 'i'))
    || s.match(new RegExp(`\\b(?:call|name)\\s+(?:the\\s+|my\\s+)(?:app|site|dashboard)\\s+(?:to\\s+|as\\s+)?${tail}`, 'i'));
  return m ? m[1].trim() : null;
}
