// title: Rating stars
// description: Stars to show or give a rating (books, restaurants, how a workout felt); tap a star to set it, tap the same star again to clear it; half stars when only shown.
// props: value: number (0..max); max: how many stars (default 5); onChange(value) to make it editable (0 when cleared);
//   size: pixels (default 20); color: css color of a filled star (default the warning color, a gold); label: accessible name (default "Rating")
function component(props) {
  css('sa-stars', `
.sa-stars{display:inline-flex;gap:2px;align-items:center;line-height:1}
.sa-stars button,.sa-stars span{border:0;background:none;box-shadow:none;padding:0;margin:0;font-size:var(--s);line-height:1;color:var(--line);position:relative}
.sa-stars button{cursor:pointer}
.sa-stars .on{color:var(--c)}
.sa-stars .half{background:linear-gradient(90deg,var(--c) 50%,var(--line) 50%);-webkit-background-clip:text;background-clip:text;color:transparent}
.sa-stars.edit:hover button{color:var(--c)}
.sa-stars.edit button:hover ~ button{color:var(--line)}
`);
  const max = Math.max(1, Math.min(10, Number(props.max) || 5));
  const value = Math.max(0, Math.min(max, Number(props.value) || 0));
  const edit = typeof props.onChange === 'function';
  const style = `--s:${Number(props.size) || 20}px;--c:${props.color || 'var(--warn)'}`;
  const root = h('span', { class: `sa-stars${edit ? ' edit' : ''}`, style, role: edit ? 'radiogroup' : 'img', 'aria-label': `${props.label || 'Rating'}: ${value} of ${max}` });
  for (let i = 1; i <= max; i++) {
    const cls = value >= i ? 'on' : !edit && value >= i - 0.5 ? 'half' : '';
    root.append(edit
      ? h('button', { type: 'button', class: cls || null, role: 'radio', 'aria-checked': Math.round(value) === i ? 'true' : 'false', 'aria-label': `${i} star${i === 1 ? '' : 's'}`, onclick: () => props.onChange(Math.round(value) === i ? 0 : i) }, '★')
      : h('span', { class: cls || null }, '★'));
  }
  return root;
}
