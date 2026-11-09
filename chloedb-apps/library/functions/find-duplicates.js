// title: Find duplicate rows
// description: Groups the rows of a table that hold the same values in the given columns (ignoring case and spaces), keeps the oldest of each group and, only when asked, removes the others.
// input: {table: name, columns: [names], remove = false} -> {groups: [{key, ids, keep}], removed}: key is the shared values joined with ' | ', ids every row of the group oldest first, keep the oldest id; removed counts the rows taken out (0 unless remove is true); {error} when the input or the table is wrong
function run(input, app) {
  input = input || {};
  var NAME = /^[a-z][a-z0-9_]*$/;
  var table = input.table;
  if (typeof table !== 'string' || !NAME.test(table)) {
    return { error: 'table must be a name of lowercase letters, digits and _' };
  }
  var columns = Array.isArray(input.columns) ? input.columns : (typeof input.columns === 'string' ? [input.columns] : []);
  var seen = {};
  columns = columns.filter(function (c) {
    if (seen[c]) return false;
    seen[c] = true;
    return true;
  });
  if (!columns.length) return { error: 'columns must list at least one column' };
  for (var i = 0; i < columns.length; i++) {
    if (typeof columns[i] !== 'string' || !NAME.test(columns[i])) {
      return { error: 'Column names must be lowercase letters, digits and _: ' + String(columns[i]) };
    }
  }
  var remove = input.remove === true;

  var select = ['id', 'created'];
  columns.forEach(function (c) { if (select.indexOf(c) < 0) select.push(c); });

  // Read every row, 500 at a time (the most one query returns).
  var rows = [];
  var PAGE = 500;
  try {
    for (var offset = 0; ; offset += PAGE) {
      var page = app.sql('SELECT ' + select.join(', ') + ' FROM ' + table +
        ' ORDER BY created ASC, id ASC LIMIT ' + PAGE + ' OFFSET ' + offset, {});
      rows = rows.concat(page);
      if (page.length < PAGE) break;
    }
  } catch (e) {
    return { error: 'Could not read ' + table + ' (' + columns.join(', ') + '): ' + (e && e.message ? e.message : String(e)) };
  }

  function norm(v) {
    if (v === null || v === undefined) return '';
    if (Array.isArray(v)) return v.map(norm).filter(function (x) { return x !== ''; }).join(', ');
    return String(v).trim().replace(/\s+/g, ' ').toLowerCase();
  }

  // Oldest first: by created, rows without one last, then by id.
  rows.sort(function (a, b) {
    var ca = a.created || '', cb = b.created || '';
    if (ca !== cb) {
      if (!ca) return 1;
      if (!cb) return -1;
      return ca < cb ? -1 : 1;
    }
    var ia = String(a.id), ib = String(b.id);
    return ia < ib ? -1 : ia > ib ? 1 : 0;
  });

  var byKey = {};
  var order = [];
  rows.forEach(function (row) {
    var values = columns.map(function (c) { return norm(row[c]); });
    // Rows with nothing in every compared column are not duplicates of each other.
    if (values.every(function (v) { return v === ''; })) return;
    var k = JSON.stringify(values);
    if (!byKey[k]) {
      byKey[k] = { key: values.join(' | '), ids: [] };
      order.push(k);
    }
    byKey[k].ids.push(row.id);
  });

  var groups = [];
  order.forEach(function (k) {
    var g = byKey[k];
    if (g.ids.length > 1) groups.push({ key: g.key, ids: g.ids, keep: g.ids[0] });
  });

  var removed = 0;
  if (remove) {
    for (var gi = 0; gi < groups.length; gi++) {
      var ids = groups[gi].ids;
      for (var j = 1; j < ids.length; j++) {
        try {
          app.remove(table, ids[j]);
          removed++;
        } catch (e) {
          return { groups: groups, removed: removed, error: 'Stopped removing at a row: ' + (e && e.message ? e.message : String(e)) };
        }
      }
    }
  }

  return { groups: groups, removed: removed };
}
