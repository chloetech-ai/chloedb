// title: Leaderboard
// description: A ranked list of people or teams with a bar for each one's score, medals for the first three and the viewer highlighted even when outside the top; for step challenges, quiz scores, sales contests or chores done.
// props: rows: [{id, name, value (number), avatar? (an app file id or image URL), sub? (small text under the name)}] in any order, ranked by value, highest first;
//   format(value): how values read (default a plain number); highlight: the id of the row to highlight (the viewer), shown under the list with its rank when outside the limit;
//   limit: how many rows to show (default 10); ties share a rank (1, 1, 3) and medals go to ranks 1 to 3; rows without a number rank last and show "—";
//   onSelect(id) when a row is tapped (rows are buttons only then); empty: text when there are no rows (default "Nobody on the board yet");
//   label: accessible name of the list (default "Leaderboard")
function component(props) {
  css('sa-board', `
.sa-board{list-style:none;margin:0;padding:0;display:flex;flex-direction:column;gap:2px}
.sa-board li{margin:0;padding:0;border-radius:var(--radius)}
.sa-board .rw{display:grid;grid-template-columns:2rem 28px minmax(0,1fr) auto;gap:10px;align-items:center;width:100%;padding:8px 10px;margin:0;border:0;background:none;box-shadow:none;color:inherit;font:inherit;text-align:left;border-radius:var(--radius);box-sizing:border-box}
.sa-board button.rw{cursor:pointer}
.sa-board button.rw:hover{background:color-mix(in srgb,var(--fg) 6%,transparent)}
.sa-board button.rw:focus-visible{outline:2px solid var(--accent);outline-offset:-2px}
.sa-board li.me{background:color-mix(in srgb,var(--accent) 14%,transparent)}
.sa-board .rk{text-align:center;font-weight:600;color:var(--muted);font-variant-numeric:tabular-nums}
.sa-board .rk.medal{font-size:1.3em;line-height:1;color:inherit}
.sa-board .who{display:flex;flex-direction:column;gap:3px;min-width:0}
.sa-board .nm{font-weight:500;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.sa-board .sb{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.sa-board .bar{display:block;height:6px;border-radius:3px;background:var(--line);overflow:hidden}
.sa-board .bar i{display:block;height:100%;background:var(--accent);border-radius:3px}
.sa-board .val{font-weight:600;font-variant-numeric:tabular-nums;white-space:nowrap}
.sa-board li.gap{text-align:center;color:var(--muted);line-height:1;padding:2px 0}
`);
  const fmt = typeof props.format === 'function' ? props.format : (v) => app.fmt.number(v);
  const limit = Math.max(1, Math.floor(Number(props.limit)) || 10);
  const sel = typeof props.onSelect === 'function';
  const hi = props.highlight == null || props.highlight === '' ? null : String(props.highlight);
  const num = (v) => { if (v === null || v === undefined || v === '') return null; const n = Number(v); return Number.isFinite(n) ? n : null; };

  const rows = (Array.isArray(props.rows) ? props.rows : [])
    .filter((r) => r && r.id != null)
    .map((r, i) => ({ r, v: num(r.value), i }));
  if (!rows.length) return h('div', { class: 'empty' }, props.empty || 'Nobody on the board yet');

  rows.sort((a, b) => ((a.v === null) - (b.v === null)) || ((b.v || 0) - (a.v || 0)) || (a.i - b.i));
  let rank = 0;
  rows.forEach((x, k) => { if (k === 0 || x.v !== rows[k - 1].v) rank = k + 1; x.rank = rank; });
  const max = Math.max(0, ...rows.map((x) => x.v || 0));

  const medals = ['🥇', '🥈', '🥉'];
  const item = (x) => {
    const me = hi !== null && String(x.r.id) === hi;
    const name = x.r.name == null || x.r.name === '' ? 'Unnamed' : String(x.r.name);
    const pct = max > 0 && x.v > 0 ? Math.min(100, (x.v / max) * 100) : 0;
    const shown = x.v === null ? '—' : fmt(x.v);
    const medal = x.v !== null && x.rank <= 3;
    const inner = [
      h('span', { class: `rk${medal ? ' medal' : ''}`, 'aria-hidden': 'true' }, medal ? medals[x.rank - 1] : String(x.rank)),
      ui.avatar({ name, src: x.r.avatar || undefined, size: 28 }),
      h('span', { class: 'who' },
        h('span', { class: 'nm' }, name),
        x.r.sub != null && x.r.sub !== '' ? h('span', { class: 'sb small muted' }, String(x.r.sub)) : null,
        h('span', { class: 'bar', 'aria-hidden': 'true' }, h('i', { style: `width:${pct}%` }))),
      h('span', { class: 'val' }, shown),
    ];
    const aria = `Rank ${x.rank}: ${name}, ${typeof shown === 'string' || typeof shown === 'number' ? shown : x.v}`;
    return h('li', { class: me ? 'me' : null, 'aria-current': me ? 'true' : null },
      sel
        ? h('button', { type: 'button', class: 'rw', 'aria-label': aria, onclick: () => props.onSelect(x.r.id) }, inner)
        : h('div', { class: 'rw', 'aria-label': aria, role: 'group' }, inner));
  };

  const top = rows.slice(0, limit);
  const list = h('ol', { class: 'sa-board', 'aria-label': props.label || 'Leaderboard' }, top.map(item));
  if (hi !== null && !top.some((x) => String(x.r.id) === hi)) {
    const mine = rows.find((x) => String(x.r.id) === hi);
    if (mine) {
      if (mine !== rows[limit]) list.append(h('li', { class: 'gap', 'aria-hidden': 'true' }, '⋯'));
      list.append(item(mine));
    }
  }
  return list;
}
