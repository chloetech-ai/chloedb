// title: Reactions
// description: Emoji reactions under a post or a photo: each emoji with its count, highlighted when it is yours, a click to add or take back yours, and a picker for the rest.
// props: counts: {emoji: number}; mine: [emojis] the person gave; onToggle(emoji, on) when they add (on true) or take back (on false);
//   choices: the emojis offered (default 👍 ❤️ 😂 🎉 😮 😢 🙏 👀); disabled: true to only show them (for someone not signed in)
function component(props) {
  css('sa-react', `
.sa-react{display:inline-flex;flex-wrap:wrap;gap:6px;align-items:center;position:relative}
.sa-react .r{display:inline-flex;align-items:center;gap:4px;border-radius:999px;padding:2px 9px;font-size:13px;background:var(--soft);border:1px solid var(--line);box-shadow:none;cursor:pointer}
.sa-react .r.on{border-color:var(--accent);background:color-mix(in srgb,var(--accent) 14%,var(--card))}
.sa-react .r b{font-weight:600;font-size:12px}
.sa-react .add{border-radius:999px;padding:2px 9px;font-size:13px;box-shadow:none}
.sa-react .pick{position:absolute;left:0;bottom:calc(100% + 6px);display:flex;gap:2px;background:var(--card);border:1px solid var(--line);border-radius:999px;padding:4px;box-shadow:0 6px 18px rgba(0,0,0,.15);z-index:30}
.sa-react .pick button{border:0;box-shadow:none;background:none;font-size:18px;padding:2px 5px;border-radius:50%;cursor:pointer}
.sa-react .pick button:hover{background:var(--soft)}
`);
  const choices = props.choices || ['👍', '❤️', '😂', '🎉', '😮', '😢', '🙏', '👀'];
  const counts = { ...(props.counts || {}) };
  const mine = new Set(props.mine || []);
  const root = h('div', { class: 'sa-react' });
  const toggle = (emoji) => {
    if (props.disabled) return;
    const on = !mine.has(emoji);
    if (on) { mine.add(emoji); counts[emoji] = (counts[emoji] || 0) + 1; } else { mine.delete(emoji); counts[emoji] = Math.max(0, (counts[emoji] || 1) - 1); }
    draw();
    if (props.onToggle) props.onToggle(emoji, on);
  };
  let picking = false;
  function draw() {
    const shown = Object.keys(counts).filter((e) => counts[e] > 0);
    root.replaceChildren(
      ...shown.map((e) => h('button', { type: 'button', class: `r${mine.has(e) ? ' on' : ''}`, disabled: !!props.disabled, 'aria-pressed': mine.has(e) ? 'true' : 'false', onclick: () => toggle(e) }, e, h('b', null, counts[e]))),
      props.disabled ? null : h('button', { type: 'button', class: 'add ghost', 'aria-label': 'Add a reaction', onclick: (ev) => { ev.stopPropagation(); picking = !picking; draw(); } }, shown.length ? '+' : '☺ React'),
      picking ? h('div', { class: 'pick' }, choices.map((e) => h('button', { type: 'button', onclick: () => { picking = false; if (!mine.has(e)) toggle(e); else draw(); } }, e))) : null);
  }
  document.addEventListener('click', (e) => { if (picking && !root.contains(e.target)) { picking = false; draw(); } });
  draw();
  return root;
}
