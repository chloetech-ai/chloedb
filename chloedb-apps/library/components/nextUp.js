// title: Next up
// description: The next thing to come, made the headline: a large date block, its title, when and where in one line, how long until it, a short text and its main action, with the one after it named underneath. For the next class, match, show, meeting, departure or release on a page whose visitors come for it.
// props: item: {title, date ('YYYY-MM-DD' or an ISO time), time? (text such as '9:30', when date has none), place?, text?, badge? (a short word), id?} or null;
//   eyebrow: a short line above the title (default 'Next up'); action: {label, do} the main button (optional); secondary: {label, do} a plain second button (optional);
//   then: {title, date} the one after it, named in a line under the card (optional); countdown: true to say how long until it (default true);
//   empty: text when there is nothing to come (default 'Nothing coming up yet')
function component(props) {
  css('sa-next', `
.sa-next{display:grid;grid-template-columns:auto minmax(0,1fr);gap:20px;align-items:start;padding:22px;border:1px solid var(--line);border-radius:calc(var(--radius) * 1.5);background:var(--card)}
.sa-next .db{display:flex;flex-direction:column;align-items:center;justify-content:center;min-width:84px;padding:12px 10px;border-radius:var(--radius);background:var(--accent);color:var(--accent-fg, #fff);line-height:1}
.sa-next .db .m{font-size:.8em;font-weight:700;letter-spacing:.08em;text-transform:uppercase}
.sa-next .db .d{font-size:2.6em;font-weight:800;margin:4px 0 2px;font-variant-numeric:tabular-nums}
.sa-next .db .w{font-size:.78em;opacity:.9}
.sa-next .bd{display:flex;flex-direction:column;gap:8px;min-width:0}
.sa-next .ey{font-size:.78em;letter-spacing:.08em;text-transform:uppercase;color:var(--muted);font-weight:600}
.sa-next h3{margin:0;font-size:clamp(1.4em,4vw,2em);line-height:1.15;text-wrap:balance;overflow-wrap:anywhere}
.sa-next .meta{color:var(--muted);display:flex;flex-wrap:wrap;gap:4px 14px}
.sa-next .cd{font-weight:600;color:var(--fg)}
.sa-next .tx{margin:0;line-height:1.5;max-width:62ch}
.sa-next .ac{display:flex;flex-wrap:wrap;gap:10px;margin-top:6px}
.sa-next .ac button{min-height:44px}
.sa-next .tag{display:inline-block;margin-left:8px;padding:2px 9px;border-radius:999px;font-size:.5em;vertical-align:middle;background:color-mix(in srgb,var(--accent) 16%,transparent);color:var(--fg);font-weight:600;letter-spacing:0}
.sa-next-then{margin:10px 2px 0;color:var(--muted);font-size:.92em}
@media (max-width:560px){.sa-next{grid-template-columns:minmax(0,1fr);padding:18px}.sa-next .db{flex-direction:row;gap:8px;justify-content:flex-start;min-width:0;align-self:start;padding:8px 12px}.sa-next .db .d{font-size:1.6em;margin:0}.sa-next .ac button{flex:1 1 140px}}
`);
  const it = props.item && typeof props.item === 'object' && props.item.title != null ? props.item : null;
  if (!it) return h('p', { class: 'muted' }, props.empty == null ? 'Nothing coming up yet' : String(props.empty));
  const raw = String(it.date || '');
  const hasTime = /T\d\d:\d\d/.test(raw);
  const when = raw ? new Date(hasTime ? raw : `${raw.slice(0, 10)}T00:00:00`) : null;
  const ok = when && !Number.isNaN(when.getTime());
  const block = ok ? h('div', { class: 'db', 'aria-hidden': 'true' },
    h('span', { class: 'm' }, when.toLocaleDateString(undefined, { month: 'short' })),
    h('span', { class: 'd' }, String(when.getDate())),
    h('span', { class: 'w' }, when.toLocaleDateString(undefined, { weekday: 'short' }))) : null;
  // How long until it, in whole days (or "today", "tomorrow").
  let until = null;
  if (ok && props.countdown !== false) {
    const day = (d) => Date.UTC(d.getFullYear(), d.getMonth(), d.getDate());
    const n = Math.round((day(when) - day(new Date())) / 86400000);
    until = n === 0 ? 'Today' : n === 1 ? 'Tomorrow' : n > 1 ? `In ${n} days` : null;
  }
  const time = hasTime ? when.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' }) : it.time ? String(it.time) : null;
  const meta = [ok ? when.toLocaleDateString(undefined, { weekday: 'long', day: 'numeric', month: 'long' }) : null, time, it.place ? String(it.place) : null].filter(Boolean);
  const btn = (a, cls) => (a && a.label && typeof a.do === 'function' ? h('button', { type: 'button', class: cls, onclick: () => a.do(it) }, String(a.label)) : null);
  const then = props.then && props.then.title ? h('p', { class: 'sa-next-then' }, `Then: ${props.then.title}${props.then.date ? `, ${app.fmt.date(props.then.date)}` : ''}`) : null;
  return h('div', null,
    h('article', { class: 'sa-next', 'aria-label': `${props.eyebrow || 'Next up'}: ${it.title}` },
      block,
      h('div', { class: 'bd' },
        h('span', { class: 'ey' }, String(props.eyebrow || 'Next up')),
        h('h3', null, String(it.title), it.badge ? h('span', { class: 'tag' }, String(it.badge)) : null),
        meta.length || until ? h('div', { class: 'meta' }, meta.map((m) => h('span', null, m)), until ? h('span', { class: 'cd' }, until) : null) : null,
        it.text ? h('p', { class: 'tx' }, String(it.text)) : null,
        h('div', { class: 'ac' }, btn(props.action, 'primary'), btn(props.secondary, 'ghost')))),
    then);
}
