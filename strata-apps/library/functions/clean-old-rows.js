// title: Clean old rows
// description: Finds the rows of a table older than a number of days, optionally narrowed by equality filters, and removes them (at most 500 a run) unless it is a dry run.
// input: {table, column = 'created' (a timestamp, or a keyword holding 'YYYY-MM-DD' days or ISO times), days (>= 0), where? ({column: value}; an array value means IN, null means IS NULL), dryRun = true} -> {matched, removed, ids}: matched counts the rows older than now minus days (for a keyword column, at most the 500 oldest are looked at), ids are the rows removed (or that would be, on a dry run), oldest first; {error} when the input is wrong or the table or a column does not exist
function run(input, app) {
  input = input || {};
  var NAME = /^[a-z][a-z0-9_]*$/;
  var LIMIT = 500;
  var table = input.table;
  var column = input.column == null || input.column === '' ? 'created' : input.column;
  var dryRun = input.dryRun !== false;

  if (typeof table !== 'string' || !NAME.test(table)) {
    return { error: 'table must be a name of lowercase letters, digits and _' };
  }
  if (typeof column !== 'string' || !NAME.test(column)) {
    return { error: 'column must be a name of lowercase letters, digits and _' };
  }
  var days = Number(input.days);
  if (input.days == null || input.days === '' || typeof input.days === 'boolean' || !isFinite(days) || days < 0) {
    return { error: 'days must be a number of days, 0 or more' };
  }
  var where = input.where == null ? {} : input.where;
  if (typeof where !== 'object' || Array.isArray(where)) {
    return { error: 'where must be an object of {column: value}' };
  }

  var conds = [column + ' IS NOT NULL'];
  var params = {};
  var cols = ['id', column];
  var keys = Object.keys(where);
  for (var i = 0; i < keys.length; i++) {
    var k = keys[i];
    var v = where[k];
    if (!NAME.test(k)) return { error: 'where: "' + k + '" is not a valid column name' };
    if (cols.indexOf(k) < 0) cols.push(k);
    if (v === null || v === undefined) {
      conds.push(k + ' IS NULL');
    } else if (Array.isArray(v)) {
      if (v.length === 0) return { matched: 0, removed: 0, ids: [] };
      var names = [];
      for (var j = 0; j < v.length; j++) {
        params['w' + i + '_' + j] = v[j];
        names.push(':w' + i + '_' + j);
      }
      conds.push(k + ' IN (' + names.join(', ') + ')');
    } else {
      params['w' + i] = v;
      conds.push(k + ' = :w' + i);
    }
  }

  try {
    app.sql('SELECT ' + cols.join(', ') + ' FROM ' + table + ' LIMIT 1');
  } catch (e) {
    return { error: 'No table "' + table + '" with columns ' + cols.join(', ') + ': ' + String(e && e.message || e) };
  }

  var nowMs = Date.parse(app.now());
  if (isNaN(nowMs)) nowMs = Date.now();
  var cutoffMs = nowMs - days * 86400000;
  var cutoff = new Date(cutoffMs).toISOString();
  var cutoffDay = cutoff.slice(0, 10);
  var base = ' FROM ' + table + ' WHERE ' + conds.join(' AND ');

  var matched, ids;
  try {
    // Timestamp column: Strata filters the range itself.
    var p = Object.assign({}, params, { cutoff: cutoff });
    var older = base + ' AND ' + column + ' < :cutoff';
    var count = app.sql('SELECT COUNT(*) AS n' + older, p);
    matched = Number(count[0] && count[0].n) || 0;
    ids = app.sql('SELECT id' + older + ' ORDER BY ' + column + ' ASC LIMIT ' + LIMIT, p)
      .map(function (r) { return r.id; });
  } catch (e) {
    // Keyword column (no ranges allowed): read the oldest rows and compare here.
    // Days and ISO times sort in time order, so the oldest come first.
    var rows;
    try {
      rows = app.sql('SELECT id, ' + column + ' AS v' + base + ' ORDER BY ' + column + ' ASC LIMIT ' + LIMIT, params);
    } catch (e2) {
      return { error: String(e2 && e2.message || e2) };
    }
    var isOld = function (v) {
      if (typeof v !== 'string') return false;
      if (/^\d{4}-\d{2}-\d{2}$/.test(v)) return v < cutoffDay;
      var t = Date.parse(v);
      return !isNaN(t) && t < cutoffMs;
    };
    ids = rows.filter(function (r) { return isOld(r.v); }).map(function (r) { return r.id; });
    matched = ids.length;
  }

  if (dryRun) return { matched: matched, removed: 0, ids: ids };

  var removed = [];
  for (var n = 0; n < ids.length; n++) {
    try {
      app.remove(table, ids[n]);
      removed.push(ids[n]);
    } catch (e) {
      return { matched: matched, removed: removed.length, ids: removed, error: String(e && e.message || e) };
    }
  }
  return { matched: matched, removed: removed.length, ids: removed };
}
