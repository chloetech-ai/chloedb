// title: Location picker
// description: Find a place by address, or tap the map, and get back its coordinates and a label; shows the chosen place on a map with a pin.
// props: value: {lat, lng, label} or null; onChange({lat, lng, label}); placeholder (default 'Search an address or place');
//   height of the map in px (default 220); zoom when a place is chosen (default 15)
function component(props) {
  css('sa-locpick', `
.sa-locpick{display:flex;flex-direction:column;gap:8px}
.sa-locpick form{display:flex;gap:6px}
.sa-locpick form input{flex:1}
.sa-locpick .hits{display:flex;flex-direction:column;border:1px solid var(--line);border-radius:8px;overflow:hidden}
.sa-locpick .hits button{border:0;border-bottom:1px solid var(--line);box-shadow:none;border-radius:0;background:var(--card);text-align:left;padding:8px 10px;font-size:13.5px}
.sa-locpick .hits button:last-child{border-bottom:0}
.sa-locpick .hits button:hover{background:var(--soft)}
`);
  let value = props.value && Number.isFinite(Number(props.value.lat)) ? { lat: Number(props.value.lat), lng: Number(props.value.lng), label: props.value.label || '' } : null;
  const z = Number(props.zoom) || 15;
  const q = h('input', { type: 'search', placeholder: props.placeholder || 'Search an address or place', value: value ? value.label : '' });
  const go = h('button', { type: 'submit' }, 'Find');
  const hits = h('div', { class: 'hits', hidden: true });
  const status = h('div', { class: 'small muted' });
  const mapBox = h('div');
  const choose = (v) => {
    value = v;
    hits.hidden = true;
    q.value = v.label || '';
    status.textContent = '';
    drawMap();
    if (props.onChange) props.onChange({ ...v });
  };
  function drawMap() {
    mapBox.replaceChildren(ui.map({ height: Number(props.height) || 220, pins: value ? [{ ...value, id: 'here' }] : [], selected: 'here', center: value || undefined, zoom: value ? z : undefined,
      onClick: (ll) => choose({ lat: Math.round(ll.lat * 1e5) / 1e5, lng: Math.round(ll.lng * 1e5) / 1e5, label: `${ll.lat.toFixed(5)}, ${ll.lng.toFixed(5)}` }) }));
  }
  const form = h('form', { onsubmit: async (e) => {
    e.preventDefault();
    const text = q.value.trim();
    if (text.length < 3) return;
    const ll = lib.geo.parse(text);
    if (ll) return choose({ ...ll, label: text });
    go.disabled = true;
    status.textContent = 'Searching…';
    try {
      const places = await app.geocode(text);
      status.textContent = places.length ? '' : 'Nothing found: try a fuller address';
      hits.replaceChildren(...places.map((p) => h('button', { type: 'button', onclick: () => choose(p) }, p.label)));
      hits.hidden = !places.length;
      if (places.length === 1) choose(places[0]);
    } catch (x) { status.textContent = x.message; }
    go.disabled = false;
  } }, q, go);
  drawMap();
  return h('div', { class: 'sa-locpick' }, form, status, hits, mapBox, h('div', { class: 'small muted' }, 'Or tap the map to drop the pin there.'));
}
