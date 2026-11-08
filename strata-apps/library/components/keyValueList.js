// title: Key value list
// description: Labelled details of one record (a contact's phone and email, an order's date and total, a device's settings) as label and value pairs in one or two columns that stack on phones.
// props: items: [{label: text, value: text | number | element | null, hint?: text (small muted line under the value)}] (items that are null or have no label are skipped);
//   empty: what a null, undefined or blank value shows (default '—'); columns: 1 or 2 (default 1; 2 becomes 1 below 560 px wide);
//   dense: true for tighter spacing (default false)
function component(props) {
  css('sa-kv', `
.sa-kv{display:grid;grid-template-columns:minmax(0,1fr);column-gap:24px;margin:0;padding:0}
.sa-kv.c2{grid-template-columns:repeat(2,minmax(0,1fr))}
@media (max-width:560px){.sa-kv.c2{grid-template-columns:minmax(0,1fr)}}
.sa-kv>div{display:flex;flex-direction:column;gap:2px;padding:10px 0;border-bottom:1px solid var(--line);min-width:0}
.sa-kv.dense>div{padding:5px 0;gap:0}
.sa-kv dt{margin:0;font-size:.82em;color:var(--muted);line-height:1.3}
.sa-kv dd{margin:0;color:var(--fg);overflow-wrap:anywhere;line-height:1.4}
.sa-kv dd.none{color:var(--muted)}
.sa-kv .hint{display:block;font-size:.8em;color:var(--muted);margin-top:1px}
.sa-kv.dense dt,.sa-kv.dense .hint{font-size:.78em}
`);
  const items = (Array.isArray(props.items) ? props.items : [])
    .filter((it) => it && it.label != null && String(it.label).trim() !== '');
  const empty = props.empty == null ? '—' : String(props.empty);
  const cols = Number(props.columns) === 2 && items.length > 1 ? 2 : 1;
  const isNode = (v) => v && typeof v === 'object' && typeof v.nodeType === 'number';

  const show = (v) => {
    if (v == null) return null;
    if (isNode(v)) return v;
    if (typeof v === 'number') return Number.isFinite(v) ? app.fmt.number(v) : null;
    if (typeof v === 'boolean') return v ? 'Yes' : 'No';
    const s = String(v);
    return s.trim() === '' ? null : s;
  };

  return h('dl', { class: `sa-kv${cols === 2 ? ' c2' : ''}${props.dense ? ' dense' : ''}` },
    items.map((it) => {
      const v = show(it.value);
      const hint = it.hint != null && String(it.hint).trim() !== '' ? h('small', { class: 'hint' }, String(it.hint)) : null;
      return h('div', null,
        h('dt', null, String(it.label)),
        h('dd', { class: v == null ? 'none' : null }, v == null ? empty : v, hint));
    }));
}
