// Views and functions write SQL with the table names their app declared
// (`tasks`); in Strata the table is `app_<app>_tasks`. This rewrites the
// names after FROM and JOIN, keeping the declared name as the alias so that
// `tasks.title` still reads, and refuses anything but one SELECT over the
// app's own tables.

const RESERVED = new Set(`WHERE JOIN LEFT RIGHT INNER OUTER FULL CROSS ON USING GROUP ORDER LIMIT OFFSET HAVING UNION
  INTERSECT EXCEPT QUALIFY WINDOW AS NATURAL SELECT FROM WITH AND OR NOT`.split(/\s+/));

// Splits SQL into code and quoted parts, so rewrites never touch strings.
function segments(sql) {
  const out = [];
  let i = 0;
  let start = 0;
  while (i < sql.length) {
    const c = sql[i];
    if (c === "'" || c === '"') {
      if (i > start) out.push({ code: true, text: sql.slice(start, i) });
      let j = i + 1;
      for (; j < sql.length; j++) {
        if (sql[j] === c) {
          if (sql[j + 1] === c) { j++; continue; }
          break;
        }
      }
      out.push({ code: false, text: sql.slice(i, j + 1) });
      i = j + 1;
      start = i;
    } else if (c === '-' && sql[i + 1] === '-') {
      if (i > start) out.push({ code: true, text: sql.slice(start, i) });
      let j = sql.indexOf('\n', i);
      if (j < 0) j = sql.length;
      i = j;
      start = i;
    } else {
      i++;
    }
  }
  if (start < sql.length) out.push({ code: true, text: sql.slice(start) });
  return out;
}

export function rewriteSql(sql, tables, physical) {
  const text = String(sql || '').trim().replace(/;\s*$/, '');
  if (!text) throw new Error('empty SQL');
  const segs = segments(text);
  const code = segs.filter((s) => s.code).map((s) => s.text).join(' ');
  if (code.includes(';')) throw new Error('one statement at a time');
  const first = code.trim().split(/\s+/)[0]?.toUpperCase();
  if (first !== 'SELECT' && first !== 'WITH' && first !== '(') {
    throw new Error('app.sql runs SELECT queries only; write rows with app.insert, app.update and app.remove');
  }
  const ctes = new Set();
  for (const m of code.matchAll(/(?:\bWITH|,)\s+([A-Za-z_][A-Za-z0-9_]*)\s+AS\s*\(/gi)) ctes.add(m[1].toLowerCase());
  const names = new Set(tables);
  const used = new Set();
  // The alias is looked at, not consumed, so `FROM a JOIN b` rewrites both.
  const re = /\b(FROM|JOIN)(\s+)([A-Za-z_][A-Za-z0-9_]*)\b(?=(\s+AS\b|\s+([A-Za-z_][A-Za-z0-9_]*))?)/gi;
  const out = segs.map((s) => {
    if (!s.code) return s.text;
    return s.text.replace(re, (all, kw, sp, name, after, alias, offset, str) => {
      // A parameter (:from, :join) or a column (t.from) is not the keyword.
      if (offset > 0 && /[:@$.]/.test(str[offset - 1])) return all;
      const before = str.slice(Math.max(0, offset - 40), offset);
      // EXTRACT(dow FROM at), x IS DISTINCT FROM y, SUBSTRING(s FROM 2)
      if (kw.toUpperCase() === 'FROM' && /(EXTRACT\s*\(\s*\w+|DISTINCT|SUBSTRING\s*\([^()]*|TRIM\s*\([^()]*)\s*$/i.test(before)) return all;
      const lower = name.toLowerCase();
      if (ctes.has(lower)) return all;
      if (!names.has(lower)) {
        const list = [...names].join(', ') || 'none yet';
        throw new Error(`no table ${name} in this app (its tables: ${list})`);
      }
      used.add(lower);
      const hasAlias = after && (/^\s+AS$/i.test(after) || (alias && !RESERVED.has(alias.toUpperCase())));
      return `${kw}${sp}${physical(lower)}${hasAlias ? '' : ` AS ${lower}`}`;
    });
  }).join('');
  return { sql: out, tables: [...used] };
}
