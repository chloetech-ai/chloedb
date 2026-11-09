// title: Tag list
// description: Tags shown as coloured chips (labels on a note, categories of an expense, skills on a profile), each optionally with a count, tappable to filter and removable with a cross, with "+N" for the ones beyond a limit.
// props: tags: [strings or {label, color?, count?}] (empty labels are skipped; color is any css color, default a stable colour for the label from lib.color.forName);
//   onClick(tag) when a chip is tapped, tag being the item as given (chips are buttons only then); onRemove(tag) to show a remove cross on each chip;
//   max: how many chips to show before a "+N" chip that shows the rest when tapped (default all); size: 'normal' (default) or 'small';
//   label: accessible name of the list (default "Tags")
function component(props) {
  css('sa-tags', `
.sa-tags{display:flex;flex-wrap:wrap;gap:6px;align-items:center;min-width:0}
.sa-tags .t{display:inline-flex;align-items:center;max-width:100%;min-width:0;min-height:26px;border:1px solid color-mix(in srgb,var(--c) 45%,transparent);background:color-mix(in srgb,var(--c) 16%,transparent);color:var(--fg);border-radius:999px;font-size:13px;line-height:1.2;overflow:hidden}
.sa-tags .l,.sa-tags .x{display:inline-flex;align-items:center;border:0;background:none;box-shadow:none;color:inherit;font:inherit;margin:0;border-radius:0}
.sa-tags .l{gap:6px;padding:3px 10px;min-width:0;align-self:stretch}
.sa-tags .rm .l{padding-right:4px}
.sa-tags button.l,.sa-tags .x{cursor:pointer}
.sa-tags button.l:hover,.sa-tags .x:hover{background:color-mix(in srgb,var(--c) 18%,transparent)}
.sa-tags button:focus-visible{outline:2px solid var(--accent);outline-offset:-2px}
.sa-tags .d{width:8px;height:8px;border-radius:50%;background:var(--c);flex:none}
.sa-tags .s{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;min-width:0}
.sa-tags .n{color:var(--muted);font-variant-numeric:tabular-nums;flex:none}
.sa-tags .x{padding:0 9px 0 5px;align-self:stretch;font-size:15px;color:var(--muted)}
.sa-tags .x:hover{color:var(--fg)}
.sa-tags .more{--c:var(--muted);border-style:dashed;background:none}
.sa-tags .more .l{color:var(--muted)}
.sa-tags.small{gap:4px}
.sa-tags.small .t{min-height:20px;font-size:11.5px}
.sa-tags.small .l{padding:1px 7px;gap:4px}
.sa-tags.small .rm .l{padding-right:2px}
.sa-tags.small .d{width:6px;height:6px}
.sa-tags.small .x{padding:0 6px 0 3px;font-size:13px}
`);
  const colorFor = (label) => {
    try { return lib.color.forName(label); } catch (e) { return 'var(--accent)'; }
  };
  const items = (Array.isArray(props.tags) ? props.tags : []).map((t) => {
    if (t == null || t === '') return null;
    const o = typeof t === 'object' ? t : { label: String(t) };
    const label = String(o.label == null ? '' : o.label).trim();
    if (!label) return null;
    const count = o.count == null || o.count === '' || isNaN(Number(o.count)) ? null : Number(o.count);
    return { tag: t, label, color: o.color || colorFor(label), count };
  }).filter(Boolean);

  const clickable = typeof props.onClick === 'function';
  const removable = typeof props.onRemove === 'function';
  const m = Math.floor(Number(props.max));
  const max = m > 0 ? m : Infinity;
  const root = h('div', { class: `sa-tags${props.size === 'small' ? ' small' : ''}`, role: 'list', 'aria-label': props.label || 'Tags' });
  let open = false;

  const chip = (it) => {
    const name = it.count == null ? it.label : `${it.label}, ${it.count}`;
    const inner = [
      h('span', { class: 'd', 'aria-hidden': 'true' }),
      h('span', { class: 's' }, it.label),
      it.count == null ? null : h('span', { class: 'n' }, app.fmt.number(it.count)),
    ];
    return h('span', { class: `t${removable ? ' rm' : ''}`, role: 'listitem', style: `--c:${it.color}`, title: name },
      clickable
        ? h('button', { type: 'button', class: 'l', 'aria-label': name, onclick: () => props.onClick(it.tag) }, inner)
        : h('span', { class: 'l' }, inner),
      removable
        ? h('button', { type: 'button', class: 'x', 'aria-label': `Remove ${it.label}`, title: `Remove ${it.label}`,
            onclick: (e) => { e.stopPropagation(); props.onRemove(it.tag); } }, '×')
        : null);
  };

  const draw = () => {
    const shown = open ? items : items.slice(0, max);
    const rest = items.length - shown.length;
    root.replaceChildren(
      shown.map(chip),
      rest > 0
        ? h('span', { class: 't more', role: 'listitem' },
            h('button', { type: 'button', class: 'l', 'aria-label': `Show ${rest} more`,
              title: items.slice(shown.length).map((it) => it.label).join(', '),
              onclick: () => { open = true; draw(); } }, `+${rest}`))
        : null);
  };
  draw();
  return root;
}
