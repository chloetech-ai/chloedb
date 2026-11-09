// title: Kanban board
// description: Cards in columns (to do, doing, done; or any stages) that the person drags between columns or moves with arrow buttons on a phone, with counts per column and an add button per column.
// props: columns: [{id, title, color?, limit? (a work-in-progress limit, shown when passed)}];
//   items: [{id, column (a column id), title, subtitle?, tags?: [text], color?}] in their order;
//   onMove(id, column, index) when a card is dropped in a column at a position; onOpen(item) when a card is tapped;
//   onAdd(column) to show an add button at the bottom of each column; empty: text of an empty column (default "Nothing here")
function component(props) {
  css('sa-kanban', `
.sa-kb{display:grid;grid-auto-flow:column;grid-auto-columns:minmax(230px,1fr);gap:12px;overflow-x:auto;padding-bottom:6px;align-items:start}
.sa-kb-col{background:var(--soft);border-radius:var(--radius);padding:10px;display:flex;flex-direction:column;gap:8px;min-height:90px;border:2px solid transparent}
.sa-kb-col.over{border-color:var(--accent)}
.sa-kb-col.full .sa-kb-count{color:var(--bad)}
.sa-kb-h{display:flex;align-items:center;gap:8px;font-size:12.5px;font-weight:600;color:var(--muted);text-transform:uppercase;letter-spacing:.04em}
.sa-kb-h i{width:8px;height:8px;border-radius:50%;background:var(--c,var(--muted))}
.sa-kb-count{margin-left:auto;font-weight:500}
.sa-kb-card{background:var(--card);border:1px solid var(--line);border-left:3px solid var(--c,var(--line));border-radius:8px;padding:8px 10px;display:flex;flex-direction:column;gap:4px;cursor:grab;box-shadow:var(--shadow)}
.sa-kb-card.dragging{opacity:.4}
.sa-kb-card .t{font-weight:550}
.sa-kb-card .s{font-size:12.5px;color:var(--muted)}
.sa-kb-card .mv{display:none;gap:4px;margin-top:2px}
@media (hover:none){.sa-kb-card .mv{display:flex}}
.sa-kb-empty{font-size:12.5px;color:var(--muted);text-align:center;padding:10px 0}
.sa-kb-drop{height:3px;border-radius:2px;background:var(--accent)}
`);
  const cols = props.columns || [];
  const items = (props.items || []).filter(Boolean);
  let dragId = null;
  const marker = h('div', { class: 'sa-kb-drop' });
  const move = (id, column, index) => { if (props.onMove) props.onMove(id, column, index); };
  const root = h('div', { class: 'sa-kb' });
  cols.forEach((col, ci) => {
    const mine = items.filter((it) => String(it.column) === String(col.id));
    const list = h('div', { class: 'col', style: 'gap:8px' });
    const colEl = h('div', { class: `sa-kb-col${col.limit && mine.length > col.limit ? ' full' : ''}`, style: col.color ? `--c:${col.color}` : null, 'data-col': col.id },
      h('div', { class: 'sa-kb-h' }, h('i'), h('span', null, col.title || col.id), h('span', { class: 'sa-kb-count' }, col.limit ? `${mine.length}/${col.limit}` : String(mine.length))),
      list,
      props.onAdd ? h('button', { class: 'ghost small', type: 'button', onclick: () => props.onAdd(col.id) }, '+ Add') : null);
    if (!mine.length) list.append(h('div', { class: 'sa-kb-empty' }, props.empty || 'Nothing here'));
    mine.forEach((it, i) => {
      const card = h('div', { class: 'sa-kb-card', draggable: 'true', style: it.color ? `--c:${it.color}` : null, 'data-id': it.id, tabindex: 0,
        onclick: () => props.onOpen && props.onOpen(it),
        onkeydown: (e) => { if (e.key === 'Enter' && props.onOpen) props.onOpen(it); } },
      h('span', { class: 't' }, it.title || ''),
      it.subtitle ? h('span', { class: 's' }, it.subtitle) : null,
      it.tags && it.tags.length ? h('span', { class: 'chips' }, it.tags.map((t) => h('span', { class: 'badge' }, t))) : null,
      props.onMove ? h('span', { class: 'mv' },
        ci > 0 ? h('button', { class: 'small', type: 'button', 'aria-label': `Move to ${cols[ci - 1].title}`, onclick: (e) => { e.stopPropagation(); move(it.id, cols[ci - 1].id, 0); } }, '←') : null,
        ci < cols.length - 1 ? h('button', { class: 'small', type: 'button', 'aria-label': `Move to ${cols[ci + 1].title}`, onclick: (e) => { e.stopPropagation(); move(it.id, cols[ci + 1].id, 0); } }, '→') : null) : null);
      card.addEventListener('dragstart', (e) => { dragId = it.id; card.classList.add('dragging'); e.dataTransfer.effectAllowed = 'move'; try { e.dataTransfer.setData('text/plain', String(it.id)); } catch { /* some browsers */ } });
      card.addEventListener('dragend', () => { dragId = null; card.classList.remove('dragging'); marker.remove(); root.querySelectorAll('.over').forEach((x) => x.classList.remove('over')); });
      list.append(card);
    });
    const indexAt = (y) => {
      const cards = [...list.querySelectorAll('.sa-kb-card:not(.dragging)')];
      const i = cards.findIndex((c) => { const r = c.getBoundingClientRect(); return y < r.top + r.height / 2; });
      return { cards, i: i < 0 ? cards.length : i };
    };
    colEl.addEventListener('dragover', (e) => {
      if (dragId === null || !props.onMove) return;
      e.preventDefault();
      colEl.classList.add('over');
      const { cards, i } = indexAt(e.clientY);
      if (cards[i]) cards[i].before(marker); else list.append(marker);
    });
    colEl.addEventListener('dragleave', (e) => { if (!colEl.contains(e.relatedTarget)) { colEl.classList.remove('over'); marker.remove(); } });
    colEl.addEventListener('drop', (e) => {
      if (dragId === null || !props.onMove) return;
      e.preventDefault();
      const { i } = indexAt(e.clientY);
      const id = dragId;
      marker.remove();
      colEl.classList.remove('over');
      move(id, col.id, i);
    });
    root.append(colEl);
  });
  return root;
}
