// Whether a session did its task, from hard checks on what it left behind:
// the sandbox's data (SQL run with the root token, after the session), its
// API (an HTTP call and a test of the answer), and the person's answer. A
// session succeeds when it said it was done and every check passes. The
// truth a check compares with is read from the same sandbox, so data drawn
// at random still has one right answer.
//
//   { "kind": "sql", "sql": "SELECT COUNT(*) AS n FROM tickets", "expect": { "n": 24 } }
//   { "kind": "http", "path": "/v1/admin/keys", "test": "body.keys.some(k => k.scope === 'read')" }
//   { "kind": "answer_includes", "any": ["t-7", "t-12"], "at_least": 1 }
//   { "kind": "answer_matches_sql", "sql": "SELECT status, COUNT(*) AS n ...", "columns": ["n"] }

const norm = (s) => String(s).toLowerCase().replace(/[\s,_]+/g, ' ');

/** `{{now}}` and `{{days_ago:7}}` in a check's SQL, as ISO 8601 times. */
export function fill(sql, now = Date.now()) {
  return sql.replace(/\{\{now\}\}/g, new Date(now).toISOString())
    .replace(/\{\{days_ago:(\d+(?:\.\d+)?)\}\}/g, (_, d) => new Date(now - Number(d) * 86400000).toISOString());
}

/** Does the answer hold this value, as a word or a number (1,234 = 1234)? */
export function holds(answer, v) {
  const a = norm(answer);
  if (typeof v === 'number' || /^-?\d+(\.\d+)?$/.test(String(v))) {
    const n = Number(v);
    const nums = (String(answer).replace(/(\d),(?=\d{3}\b)/g, '$1').match(/-?\d+(\.\d+)?/g) || []).map(Number);
    return nums.some((x) => Math.abs(x - n) < 1e-6 || (Math.abs(n) >= 1 && !Number.isInteger(n) && Math.abs(x - n) < 0.01 * Math.abs(n)));
  }
  const w = norm(v).trim();
  return new RegExp(`(^|[^a-z0-9-])${w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}($|[^a-z0-9-])`).test(a);
}

async function one(check, { sandbox, answer }) {
  switch (check.kind) {
    case 'sql': {
      const rows = await sandbox.sql(fill(check.sql));
      const row = rows?.[0] || {};
      if (check.expect) {
        const bad = Object.entries(check.expect).filter(([k, v]) => String(row[k]) !== String(v));
        return { ok: !bad.length, detail: bad.length ? `expected ${JSON.stringify(check.expect)}, got ${JSON.stringify(row)}` : 'as expected' };
      }
      if (check.min_rows != null) return { ok: rows.length >= check.min_rows, detail: `${rows.length} rows` };
      return { ok: rows.length > 0, detail: `${rows.length} rows` };
    }
    case 'http': {
      const { status, body } = await sandbox.call(check.method || 'GET', check.path, check.body, { raw: true });
      // Scenario files are written by the cluster's admins, as its command line is.
      const ok = status < 400 && (!check.test || Boolean(new Function('body', `return (${check.test});`)(body)));
      return { ok, detail: `HTTP ${status}` };
    }
    case 'answer_includes': {
      const vals = check.any || check.all || [];
      const hit = vals.filter((v) => holds(answer, v));
      const need = check.all ? vals.length : (check.at_least ?? 1);
      return { ok: hit.length >= need, detail: `${hit.length} of ${vals.length} named (${need} needed)` };
    }
    case 'answer_matches_sql': {
      const rows = await sandbox.sql(fill(check.sql));
      const want = rows.flatMap((r) => (check.columns || Object.keys(r)).map((c) => r[c])).filter((v) => v != null);
      const miss = want.filter((v) => !holds(answer, v));
      return { ok: want.length > 0 && !miss.length, detail: miss.length ? `missing ${miss.slice(0, 6).join(', ')} (truth: ${want.slice(0, 12).join(', ')})` : `all ${want.length} values named` };
    }
    default:
      return { ok: false, detail: `unknown check ${check.kind}` };
  }
}

/** Run a task's checks; answers { success, results }. */
export async function runChecks(task, { sandbox, outcome, answer }) {
  const results = [];
  for (const c of task.checks || []) {
    try { results.push({ kind: c.kind, name: c.name || c.kind, ...(await one(c, { sandbox, answer })) }); } catch (e) { results.push({ kind: c.kind, name: c.name || c.kind, ok: false, detail: `check failed to run: ${e.message}` }); }
  }
  return { success: outcome === 'done' && results.every((r) => r.ok), results };
}
