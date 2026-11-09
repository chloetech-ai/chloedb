// title: CSV
// description: Read and write CSV (spreadsheets, bank exports, contact lists): quoted fields, commas or semicolons or tabs, line breaks inside quotes, and typed values.
// api: parse(text, {header = true, delimiter (default: guessed from the first line)}) -> rows as objects keyed by the header (or arrays when header is false);
//   columns(text) -> the header names;
//   typed(rows) -> the same rows with '12.5' as numbers, 'true'/'false'/'yes'/'no' as booleans and '' as null (dates stay text);
//   stringify(rows, {columns (default: every key in order of appearance), delimiter = ','}) -> CSV text with a header line
function helper() {
  function guess(text) {
    const line = text.split(/\r?\n/, 1)[0] || '';
    const counts = [',', ';', '\t', '|'].map((d) => [d, line.split(d).length - 1]);
    counts.sort((a, b) => b[1] - a[1]);
    return counts[0][1] > 0 ? counts[0][0] : ',';
  }
  function rowsOf(text, delimiter) {
    const out = [];
    let row = [];
    let field = '';
    let quoted = false;
    let i = 0;
    const s = String(text || '').replace(/^﻿/, '');
    while (i < s.length) {
      const c = s[i];
      if (quoted) {
        if (c === '"') {
          if (s[i + 1] === '"') { field += '"'; i += 2; continue; }
          quoted = false; i++; continue;
        }
        field += c; i++; continue;
      }
      if (c === '"' && field === '') { quoted = true; i++; continue; }
      if (c === delimiter) { row.push(field); field = ''; i++; continue; }
      if (c === '\r' || c === '\n') {
        row.push(field); field = '';
        if (row.length > 1 || row[0] !== '') out.push(row);
        row = [];
        i += c === '\r' && s[i + 1] === '\n' ? 2 : 1;
        continue;
      }
      field += c; i++;
    }
    if (field !== '' || row.length) { row.push(field); if (row.length > 1 || row[0] !== '') out.push(row); }
    return out;
  }
  function parse(text, { header = true, delimiter } = {}) {
    const d = delimiter || guess(String(text || ''));
    const rows = rowsOf(text, d);
    if (!header) return rows;
    if (!rows.length) return [];
    const seen = new Map();
    const names = rows[0].map((h, i) => {
      let n = String(h).trim() || `column${i + 1}`;
      const k = seen.get(n) || 0;
      seen.set(n, k + 1);
      if (k) n = `${n}_${k + 1}`;
      return n;
    });
    return rows.slice(1).map((r) => Object.fromEntries(names.map((n, i) => [n, r[i] === undefined ? '' : r[i]])));
  }
  function columns(text) { const rows = rowsOf(text, guess(String(text || ''))); return rows.length ? rows[0].map((h) => String(h).trim()) : []; }
  function typedValue(v) {
    if (typeof v !== 'string') return v;
    const t = v.trim();
    if (t === '') return null;
    if (/^(true|yes)$/i.test(t)) return true;
    if (/^(false|no)$/i.test(t)) return false;
    if (/^-?(\d+|\d{1,3}(,\d{3})+)(\.\d+)?$/.test(t) && !/^0\d/.test(t)) return Number(t.replace(/,/g, ''));
    return v;
  }
  function typed(rows) { return (rows || []).map((r) => (Array.isArray(r) ? r.map(typedValue) : Object.fromEntries(Object.entries(r).map(([k, v]) => [k, typedValue(v)])))); }
  function stringify(rows, { columns: cols, delimiter = ',' } = {}) {
    const list = rows || [];
    const names = cols || [...new Set(list.flatMap((r) => Object.keys(r || {})))];
    const cell = (v) => {
      if (v === null || v === undefined) return '';
      const s = Array.isArray(v) ? v.join('; ') : typeof v === 'object' ? JSON.stringify(v) : String(v);
      return /["\r\n]/.test(s) || s.includes(delimiter) || /^\s|\s$/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
    };
    return [names.map(cell).join(delimiter), ...list.map((r) => names.map((n) => cell(r == null ? '' : r[n])).join(delimiter))].join('\r\n') + '\r\n';
  }
  return { parse, columns, typed, stringify };
}
