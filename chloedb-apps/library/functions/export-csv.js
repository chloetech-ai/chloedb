// title: Export CSV
// description: Exports the rows of any table of the app as CSV text, with chosen columns, an optional filter and sort order, using lib.csv.
// input: {table: name (required), columns?: [names] (default every column, in the table's order), where?: SQL condition such as "status = :s" (default none), params?: {name: value} bound to the :names in where, order?: a column, optionally followed by ' desc' or ' asc' (default 'created')} -> {csv, rows: how many rows, filename: '<table>-YYYY-MM-DD.csv', truncated: true when more than 500 rows matched} or {error}
function run(input, app) {
  input = input || {};
  var NAME = /^[a-z][a-z0-9_]*$/;
  var LIMIT = 500;

  var table = String(input.table || '').trim();
  if (!NAME.test(table)) return { error: 'Give the name of a table (lowercase letters, digits and _).' };

  var columns = null;
  if (input.columns != null) {
    if (!Array.isArray(input.columns) || input.columns.length === 0) return { error: 'columns must be a list of column names.' };
    columns = [];
    for (var i = 0; i < input.columns.length; i++) {
      var c = String(input.columns[i] || '').trim();
      if (!NAME.test(c)) return { error: 'Not a valid column name: "' + input.columns[i] + '".' };
      if (columns.indexOf(c) === -1) columns.push(c);
    }
  }

  var orderCol = 'created', orderDir = 'ASC';
  if (input.order != null && String(input.order).trim() !== '') {
    var m = /^([a-z][a-z0-9_]*)(?:\s+(asc|desc))?$/i.exec(String(input.order).trim());
    if (!m || !NAME.test(m[1])) return { error: 'order must be a column name, optionally followed by asc or desc.' };
    orderCol = m[1];
    orderDir = (m[2] || 'asc').toUpperCase();
  }

  var where = input.where == null ? '' : String(input.where).trim();
  if (where.indexOf(';') !== -1) return { error: 'where must be a single condition, without ";".' };
  var params = input.params && typeof input.params === 'object' && !Array.isArray(input.params) ? input.params : {};

  // The table exists?
  try {
    app.sql('SELECT id FROM ' + table + ' LIMIT 1', {});
  } catch (e) {
    return { error: 'There is no table "' + table + '".' };
  }

  var sql = 'SELECT ' + (columns ? columns.join(', ') : '*') + ' FROM ' + table +
    (where ? ' WHERE ' + where : '') +
    ' ORDER BY ' + orderCol + ' ' + orderDir +
    ' LIMIT ' + (LIMIT + 1);

  var result;
  try {
    result = app.sql(sql, params);
  } catch (e) {
    return { error: 'Could not read "' + table + '": ' + (e && e.message ? e.message : String(e)) };
  }

  var truncated = result.length > LIMIT;
  if (truncated) result = result.slice(0, LIMIT);

  if (!columns) {
    columns = [];
    for (var r = 0; r < result.length; r++) {
      for (var k in result[r]) if (columns.indexOf(k) === -1) columns.push(k);
    }
  }

  function cell(v) {
    if (v === null || v === undefined) return '';
    if (Array.isArray(v)) return v.map(function (x) { return x === null || x === undefined ? '' : String(x); }).join('; ');
    if (v instanceof Date) return v.toISOString();
    if (typeof v === 'object') return JSON.stringify(v);
    if (typeof v === 'boolean') return v ? 'true' : 'false';
    return String(v);
  }

  var out = result.map(function (row) {
    var o = {};
    for (var j = 0; j < columns.length; j++) o[columns[j]] = cell(row[columns[j]]);
    return o;
  });

  var csv = columns.length ? lib.csv.stringify(out, { columns: columns }) : '';
  var day = String(app.now() || '').slice(0, 10);

  return {
    csv: csv,
    rows: out.length,
    filename: table + (day ? '-' + day : '') + '.csv',
    truncated: truncated
  };
}
