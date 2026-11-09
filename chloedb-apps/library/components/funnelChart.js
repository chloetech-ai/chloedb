// title: Funnel chart
// description: Stages that narrow (a sales pipeline, sign-up steps, a hiring process) as centred bars with each stage's count and the share that carried on from the stage before, with an overall rate and stages that can be tapped.
// props: stages: [{label, value: number, color?: css color (default the accent, fading stage by stage)}] in funnel order, first stage widest;
//   format(n): how counts read (default app.fmt.number); showRate: true (default) to show the conversion from each stage to the next and overall;
//   onSelect(label) when a stage is tapped (stages are buttons only then); empty: text when there are no stages (default "Nothing to show yet")
function component(props) {
  css('sa-funnel', `
.sa-funnel{margin:0;padding:0;list-style:none;display:flex;flex-direction:column;gap:0;--lw:9em}
.sa-funnel li{margin:0;padding:0;list-style:none}
.sa-funnel .sa-row,.sa-funnel .sa-rate{display:grid;grid-template-columns:minmax(0,var(--lw)) minmax(0,1fr) auto;align-items:center;gap:8px}
.sa-funnel .sa-row{width:100%;margin:0;padding:4px 6px;border:0;background:none;box-shadow:none;color:var(--fg);font:inherit;text-align:left;border-radius:var(--radius)}
.sa-funnel button.sa-row{cursor:pointer}
.sa-funnel button.sa-row:hover,.sa-funnel button.sa-row:focus-visible{background:color-mix(in srgb,var(--fg) 7%,transparent)}
.sa-funnel .sa-label{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.sa-funnel .sa-track{display:flex;justify-content:center;height:28px}
.sa-funnel .sa-bar{height:100%;min-width:4px;border-radius:4px;background:var(--c);opacity:var(--o);transition:width .3s}
.sa-funnel .sa-val{font-weight:600;font-variant-numeric:tabular-nums;text-align:right;white-space:nowrap}
.sa-funnel .sa-rate{padding:0 6px;font-size:.8em;color:var(--muted);line-height:1.6}
.sa-funnel .sa-rate span:nth-child(2){text-align:center;white-space:nowrap}
.sa-funnel .sa-total{padding:6px 6px 0;margin-top:4px;border-top:1px solid var(--line);font-size:.85em;color:var(--muted);display:flex;justify-content:space-between;gap:8px;flex-wrap:wrap}
.sa-funnel .sa-total b{color:var(--fg);font-variant-numeric:tabular-nums}
@media (max-width:480px){.sa-funnel{--lw:6.5em}}
`);
  const fmt = typeof props.format === 'function'
    ? props.format
    : (n) => (app && app.fmt && app.fmt.number ? app.fmt.number(n) : String(n));
  const showRate = props.showRate !== false;
  const select = typeof props.onSelect === 'function' ? props.onSelect : null;
  const stages = (Array.isArray(props.stages) ? props.stages : [])
    .filter((s) => s && s.label != null)
    .map((s) => {
      const v = Number(s.value);
      return { label: String(s.label), value: Number.isFinite(v) && v > 0 ? v : 0, color: s.color };
    });

  if (!stages.length) return h('div', { class: 'empty' }, props.empty || 'Nothing to show yet');

  const max = Math.max(...stages.map((s) => s.value));
  const pct = (part, whole) => {
    if (!(whole > 0)) return null;
    const p = (part / whole) * 100;
    return p === 0 || p >= 10 ? `${Math.round(p)}%` : `${p.toFixed(1)}%`;
  };

  const root = h('ol', { class: 'sa-funnel', 'aria-label': 'Funnel' });
  stages.forEach((s, i) => {
    const prev = i > 0 ? stages[i - 1] : null;
    const rate = prev ? pct(s.value, prev.value) : null;
    const width = max > 0 ? (s.value / max) * 100 : 0;
    const style = `--c:${s.color || 'var(--accent)'};--o:${s.color ? 1 : Math.max(0.45, 1 - i * 0.12)}`;
    const text = `${s.label}: ${fmt(s.value)}${showRate && prev ? `, ${rate == null ? 'no rate' : rate} of ${prev.label}` : ''}`;
    const cells = [
      h('span', { class: 'sa-label', title: s.label }, s.label),
      h('span', { class: 'sa-track', 'aria-hidden': 'true' }, h('span', { class: 'sa-bar', style: `width:${width}%` })),
      h('span', { class: 'sa-val' }, fmt(s.value)),
    ];
    root.append(h('li', null,
      showRate && prev
        ? h('div', { class: 'sa-rate', 'aria-hidden': 'true' },
            h('span'), h('span', null, rate == null ? '↓ —' : `↓ ${rate}`), h('span'))
        : null,
      select
        ? h('button', { type: 'button', class: 'sa-row', style, 'aria-label': text, onclick: () => select(s.label) }, cells)
        : h('div', { class: 'sa-row', style, 'aria-label': text }, cells)
    ));
  });

  if (showRate && stages.length > 1) {
    const first = stages[0], last = stages[stages.length - 1];
    const overall = pct(last.value, first.value);
    root.append(h('li', { class: 'sa-total' },
      h('span', null, `${first.label} → ${last.label}`),
      h('span', null, 'Overall ', h('b', null, overall == null ? '—' : overall))));
  }
  return root;
}
