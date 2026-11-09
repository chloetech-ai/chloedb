// title: Poll
// description: A question to vote on with one choice among several options, then the results as bars with each option's share and the total (team lunch spot, meeting time, which feature next, club decisions).
// props: question: text of the question (optional); options: [{id, label, votes (number, default 0; include the person's own vote)}];
//   voted: the id of the option the person chose, or null when they have not voted (default null);
//   onVote(id) when the person picks an option (may return a promise; the options are disabled until it settles); without onVote the poll only shows its results;
//   allowChange: true to let the person pick another option after voting (default true);
//   showResults: 'after' to show the bars only once the person has voted (default) or 'always' to show them before too;
//   the total number of votes is always shown under the options
function component(props) {
  css('sa-poll', `
.sa-poll{display:flex;flex-direction:column;gap:8px;max-width:100%}
.sa-poll h3{margin:0;overflow-wrap:anywhere}
.sa-poll .sa-opts{display:flex;flex-direction:column;gap:6px}
.sa-poll .sa-opt{position:relative;display:flex;align-items:center;gap:8px;width:100%;min-height:42px;margin:0;padding:8px 12px;border:1px solid var(--line);border-radius:var(--radius);background:var(--card);color:var(--fg);font:inherit;text-align:left;box-shadow:none;overflow:hidden;box-sizing:border-box}
.sa-poll button.sa-opt{cursor:pointer}
.sa-poll button.sa-opt:hover:not(:disabled){border-color:var(--accent)}
.sa-poll button.sa-opt:disabled{cursor:default;opacity:.7}
.sa-poll .sa-bar{position:absolute;left:0;top:0;bottom:0;background:color-mix(in srgb,var(--accent) 16%,transparent);transition:width .3s ease;pointer-events:none}
.sa-poll .lead .sa-bar{background:color-mix(in srgb,var(--accent) 28%,transparent)}
.sa-poll .mine{border-color:var(--accent)}
.sa-poll .mine .sa-lbl{font-weight:600}
.sa-poll .sa-lbl{position:relative;flex:1;min-width:0;overflow-wrap:anywhere}
.sa-poll .sa-check{position:relative;color:var(--accent);font-weight:700}
.sa-poll .sa-pct{position:relative;white-space:nowrap;font-variant-numeric:tabular-nums;color:var(--muted)}
.sa-poll .mine .sa-pct,.sa-poll .lead .sa-pct{color:var(--fg)}
.sa-poll .sa-foot{color:var(--muted)}
`);
  const options = (Array.isArray(props.options) ? props.options : []).filter((o) => o && o.id != null);
  const voted = props.voted == null || props.voted === '' ? null : String(props.voted);
  const canVote = typeof props.onVote === 'function';
  const allowChange = props.allowChange !== false;
  const hasVoted = voted != null && options.some((o) => String(o.id) === voted);
  const results = props.showResults === 'always' || hasVoted || !canVote;
  const open = canVote && (!hasVoted || allowChange);

  const counts = options.map((o) => Math.max(0, Math.floor(Number(o.votes) || 0)));
  const total = counts.reduce((a, b) => a + b, 0);
  const top = counts.length ? Math.max(...counts) : 0;
  const plural = (n) => `${n} vote${n === 1 ? '' : 's'}`;

  const root = h('div', { class: 'sa-poll' });
  if (props.question) root.append(h('h3', null, String(props.question)));

  if (!options.length) {
    root.append(h('div', { class: 'empty' }, 'No options yet'));
    return root;
  }

  const buttons = [];
  let busy = false;
  const pick = (id) => {
    if (busy || (hasVoted && String(id) === voted)) return;
    busy = true;
    buttons.forEach((b) => { b.disabled = true; });
    const done = () => { busy = false; buttons.forEach((b) => { b.disabled = false; }); };
    try {
      Promise.resolve(props.onVote(id)).then(done, done);
    } catch (e) {
      done();
      throw e;
    }
  };

  const list = h('div', {
    class: 'sa-opts',
    role: open ? 'radiogroup' : 'list',
    'aria-label': props.question ? String(props.question) : 'Poll',
  });

  options.forEach((o, i) => {
    const n = counts[i];
    const pct = total ? Math.round((n / total) * 100) : 0;
    const mine = hasVoted && String(o.id) === voted;
    const lead = results && total > 0 && n === top;
    const label = o.label == null ? '' : String(o.label);
    const cls = `sa-opt${mine ? ' mine' : ''}${lead ? ' lead' : ''}`;
    const aria = results ? `${label}: ${pct}%, ${plural(n)}${mine ? ', your vote' : ''}` : label;
    const children = [
      results ? h('span', { class: 'sa-bar', style: `width:${pct}%`, 'aria-hidden': 'true' }) : null,
      h('span', { class: 'sa-lbl' }, label),
      mine ? h('span', { class: 'sa-check', 'aria-hidden': 'true' }, '✓') : null,
      results ? h('span', { class: 'sa-pct', title: plural(n), 'aria-hidden': 'true' }, `${pct}%`) : null,
    ];
    if (open) {
      const b = h('button', {
        type: 'button', class: cls, role: 'radio',
        'aria-checked': mine ? 'true' : 'false', 'aria-label': aria,
        onclick: () => pick(o.id),
      }, children);
      buttons.push(b);
      list.append(b);
    } else {
      list.append(h('div', { class: cls, role: 'listitem', 'aria-label': aria }, children));
    }
  });
  root.append(list);

  let foot = plural(total);
  if (!results) foot += ' · Vote to see the results';
  else if (hasVoted && open) foot += ' · Tap another option to change your vote';
  root.append(h('small', { class: 'sa-foot', 'aria-live': 'polite' }, foot));
  return root;
}
