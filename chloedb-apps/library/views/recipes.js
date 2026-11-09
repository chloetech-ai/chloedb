// title: Recipes
// description: A recipe box of cards with photo, time and servings, searchable by name or ingredient and filterable by tag, with a recipe page that scales the ingredients to the servings you need and numbers the steps.
// tables: recipes(title text, photo text, minutes int, servings int, ingredients text, steps text, tags keyword[])
async function render(root, app) {
  const style = h('style', null, `
    .recipes .rcard { cursor: pointer; display: flex; flex-direction: column; gap: 6px; padding: 10px; }
    .recipes .rcard:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
    .recipes .rph { aspect-ratio: 4 / 3; border-radius: var(--radius); background: var(--line); display: flex; align-items: center; justify-content: center; font-size: 2.2rem; }
    .recipes .rmeta { display: flex; gap: 10px; flex-wrap: wrap; }
    .recipes .ring { margin: 0; padding-left: 1.2em; display: flex; flex-direction: column; gap: 6px; }
    .recipes .ring li.rhead { list-style: none; margin: 8px 0 0 -1.2em; font-weight: 600; }
    .recipes .rsteps { margin: 0; padding-left: 1.4em; display: flex; flex-direction: column; gap: 10px; }
    .recipes .rsteps li { line-height: 1.5; }
    .recipes .rserv { display: flex; align-items: center; gap: 8px; }
    .recipes .rserv .n { min-width: 6.5em; text-align: center; }
    .recipes .rhero { max-width: 720px; }
  `);
  root.classList.add('recipes');

  let mode = 'list';
  let q = '';
  let tag = null;
  let currentId = null;
  let shown = null;
  let allTags = [];
  let listSeq = 0;
  let detailSeq = 0;

  const grid = h('div');
  const chipsEl = h('div');
  const countEl = h('small', { class: 'muted' });
  const listEl = h('div', { class: 'col' },
    h('div', { class: 'row' },
      h('h1', { class: 'grow' }, 'Recipes'),
      h('button', { class: 'primary', onclick: () => openForm(null) }, 'Add recipe')),
    h('div', { class: 'toolbar' },
      h('div', { class: 'grow' }, ui.searchBox({
        placeholder: 'Search by name or ingredient',
        onSearch: (text) => { q = text; if (text) app.track('search', text); loadList(); },
      }))),
    chipsEl, countEl, grid);
  const detailEl = h('div', { class: 'col' });
  const formEl = h('div', { class: 'col' });
  root.append(style, listEl, detailEl, formEl);

  function show(m) {
    mode = m;
    listEl.hidden = m !== 'list';
    detailEl.hidden = m !== 'detail';
    formEl.hidden = m !== 'edit';
    window.scrollTo(0, 0);
  }

  function duration(min) {
    if (!(min > 0)) return '';
    return lib.duration.format(min * 60) || `${min} min`;
  }

  function matchExpr(text) {
    const words = (text || '').toLowerCase().match(/[\p{L}\p{N}]+/gu) || [];
    return words.map((w) => w + '*').join(' ');
  }

  // ---- list ----
  async function loadList() {
    const seq = ++listSeq;
    const where = [];
    const params = {};
    const expr = matchExpr(q);
    if (expr) { where.push('(MATCH(title, :q1) OR MATCH(ingredients, :q2))'); params.q1 = expr; params.q2 = expr; }
    if (tag) { where.push('tags = :tag'); params.tag = tag; }
    let rows, tagRows, total;
    try {
      [rows, tagRows, total] = await Promise.all([
        app.sql(`SELECT id, title, photo, minutes, servings, tags FROM recipes ${where.length ? 'WHERE ' + where.join(' AND ') : ''} ORDER BY title ASC LIMIT 200`, params),
        app.sql('SELECT tags FROM recipes WHERE tags IS NOT NULL LIMIT 500'),
        app.sql('SELECT COUNT(*) AS n FROM recipes'),
      ]);
    } catch (e) {
      if (seq !== listSeq) return;
      grid.replaceChildren(h('p', { class: 'empty' }, 'Could not search with that text. Try other words.'));
      return;
    }
    if (seq !== listSeq) return;
    const n = total[0] ? total[0].n : 0;

    const counts = {};
    for (const r of tagRows) for (const t of (r.tags || [])) if (t) counts[t] = (counts[t] || 0) + 1;
    allTags = Object.keys(counts).sort((a, b) => counts[b] - counts[a] || a.localeCompare(b));
    if (tag && !counts[tag]) counts[tag] = 0;
    const tagOptions = Object.keys(counts)
      .sort((a, b) => counts[b] - counts[a] || a.localeCompare(b))
      .slice(0, 20)
      .map((t) => ({ value: t, label: t, count: counts[t] }));
    if (tag && !tagOptions.some((o) => o.value === tag)) tagOptions.push({ value: tag, label: tag, count: counts[tag] });
    chipsEl.replaceChildren(tagOptions.length ? ui.filterChips({
      options: tagOptions,
      value: tag,
      onChange: (v) => { tag = v || null; app.track('filter', tag ? `tag=${tag}` : 'tag=all'); loadList(); },
    }) : null);

    if (n === 0) {
      countEl.textContent = '';
      chipsEl.replaceChildren();
      grid.replaceChildren(ui.emptyState({
        icon: '🍲', title: 'No recipes yet',
        text: 'Add your first recipe with its ingredients and steps.',
        action: { label: 'Add recipe', onClick: () => openForm(null) },
      }));
      return;
    }
    if (!rows.length) {
      countEl.textContent = '';
      grid.replaceChildren(ui.emptyState({
        icon: '🔎', title: 'No recipes match',
        text: 'Try another word, or clear the search and the tag.',
        compact: true,
      }));
      return;
    }
    countEl.textContent = rows.length >= 200
      ? 'Showing the first 200 — search to narrow down'
      : (q || tag ? lib.text.plural(rows.length, 'recipe') + ' found' : lib.text.plural(n, 'recipe'));
    grid.replaceChildren(h('div', { class: 'grid' }, rows.map(card)));
  }

  function card(r) {
    const meta = [
      r.minutes > 0 ? h('span', null, '⏱ ', duration(r.minutes)) : null,
      r.servings > 0 ? h('span', null, '🍽 ', lib.text.plural(r.servings, 'serving')) : null,
    ];
    return h('div', {
      class: 'card rcard', tabindex: 0, role: 'button',
      onclick: () => openDetail(r.id),
      onkeydown: (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); openDetail(r.id); } },
    },
      r.photo ? ui.image({ src: r.photo, alt: r.title || '', ratio: 4 / 3 }) : h('div', { class: 'rph' }, '🍽'),
      h('div', { class: 'title clip' }, r.title || 'Untitled'),
      h('div', { class: 'rmeta small muted' }, meta),
      (r.tags && r.tags.length) ? h('div', { class: 'row small' }, r.tags.slice(0, 3).map((t) => h('span', { class: 'badge' }, t))) : null);
  }

  // ---- detail ----
  function openDetail(id) {
    currentId = id;
    shown = null;
    detailEl.replaceChildren(h('p', { class: 'muted' }, 'Loading…'));
    show('detail');
    loadDetail();
  }

  function backToList() {
    currentId = null;
    show('list');
    loadList();
  }

  async function loadDetail() {
    const seq = ++detailSeq;
    const rows = await app.sql('SELECT * FROM recipes WHERE id = :id LIMIT 1', { id: currentId });
    if (seq !== detailSeq || mode !== 'detail') return;
    if (!rows.length) { backToList(); return; }
    drawDetail(rows[0]);
  }

  const FRACTIONS = [[0, ''], [1 / 8, '1/8'], [1 / 4, '1/4'], [1 / 3, '1/3'], [3 / 8, '3/8'], [1 / 2, '1/2'],
    [5 / 8, '5/8'], [2 / 3, '2/3'], [3 / 4, '3/4'], [7 / 8, '7/8'], [1, '']];

  function fmtAmount(n) {
    if (!isFinite(n) || n <= 0) return '';
    if (n >= 10) return String(Math.round(n));
    let whole = Math.floor(n);
    const frac = n - whole;
    let best = FRACTIONS[0];
    for (const f of FRACTIONS) if (Math.abs(f[0] - frac) < Math.abs(best[0] - frac)) best = f;
    if (Math.abs(best[0] - frac) <= 0.03) {
      if (best[0] === 1) { whole += 1; best = FRACTIONS[0]; }
      if (!best[1]) return String(whole);
      return whole ? `${whole} ${best[1]}` : best[1];
    }
    return String(Math.round(n * 10) / 10);
  }

  function scaleLine(line, factor) {
    if (factor === 1) return line;
    let qn = null;
    try { qn = lib.text.quantity(line); } catch (e) { qn = null; }
    if (!qn || qn.amount == null || !isFinite(qn.amount)) return line;
    const amt = fmtAmount(qn.amount * factor);
    if (!amt) return line;
    return [amt, qn.unit, qn.name].filter((x) => x != null && String(x).trim() !== '').join(' ');
  }

  function splitLines(text) {
    return (text || '').split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
  }

  function drawDetail(r) {
    const base = r.servings > 0 ? r.servings : null;
    if (shown == null) shown = base;
    const factor = base ? shown / base : 1;

    const ingredients = splitLines(r.ingredients).map((l) => l.replace(/^[-*•·]\s+/, ''));
    const steps = splitLines(r.steps).map((l) => l.replace(/^(step\s*)?\d+\s*[.):-]\s*/i, '').replace(/^[-*•·]\s+/, '')).filter(Boolean);

    const setServings = (n) => {
      shown = Math.max(1, Math.min(999, n));
      app.track('scale', `${shown} of ${base} servings`);
      drawDetail(r);
    };

    const servingsCtl = base ? h('div', { class: 'rserv' },
      h('button', { class: 'icon', 'aria-label': 'Fewer servings', disabled: shown <= 1, onclick: () => setServings(shown - 1) }, '−'),
      h('span', { class: 'n' }, lib.text.plural(shown, 'serving')),
      h('button', { class: 'icon', 'aria-label': 'More servings', onclick: () => setServings(shown + 1) }, '+'),
      shown !== base ? h('button', { class: 'ghost small', onclick: () => setServings(base) }, `Reset to ${base}`) : null) : null;

    const ingList = ingredients.length
      ? h('ul', { class: 'ring' }, ingredients.map((l) => /:$/.test(l)
        ? h('li', { class: 'rhead' }, l.replace(/:$/, ''))
        : h('li', null, scaleLine(l, factor))))
      : h('p', { class: 'muted' }, 'No ingredients written down yet.');

    const stepList = steps.length
      ? h('ol', { class: 'rsteps' }, steps.map((s) => h('li', null, s)))
      : h('p', { class: 'muted' }, 'No steps written down yet.');

    detailEl.replaceChildren(
      h('div', { class: 'row' },
        h('button', { class: 'ghost', onclick: backToList }, '← All recipes'),
        h('div', { class: 'right row' },
          h('button', { onclick: () => openForm(r) }, 'Edit'),
          ui.confirmButton({
            label: 'Delete',
            onConfirm: async () => {
              await app.remove('recipes', r.id);
              app.toast('Recipe deleted');
              backToList();
            },
          }))),
      h('h1', null, r.title || 'Untitled'),
      h('div', { class: 'row' },
        r.minutes > 0 ? h('span', { class: 'badge info' }, '⏱ ', duration(r.minutes)) : null,
        base ? h('span', { class: 'badge' }, 'Makes ', lib.text.plural(base, 'serving')) : null,
        (r.tags || []).map((t) => h('span', { class: 'badge' }, t))),
      r.photo ? h('div', { class: 'rhero' }, ui.image({ src: r.photo, alt: r.title || '', ratio: 16 / 9 })) : null,
      h('div', { class: 'split' },
        h('div', { class: 'card col' },
          h('div', { class: 'row' }, h('h2', { class: 'grow' }, 'Ingredients')),
          servingsCtl,
          factor !== 1 ? h('small', { class: 'muted' }, `Amounts scaled from ${lib.text.plural(base, 'serving')}.`) : null,
          ingList),
        h('div', { class: 'card col' },
          h('h2', null, 'Steps'),
          stepList)));
  }

  // ---- form ----
  function openForm(r) {
    const editing = r || null;
    const toInt = (v) => {
      const n = parseInt(v, 10);
      return isFinite(n) && n > 0 ? n : null;
    };
    formEl.replaceChildren(
      h('h1', null, editing ? 'Edit recipe' : 'New recipe'),
      h('div', { class: 'card' }, ui.form({
        fields: [
          { name: 'title', label: 'Title', required: true, placeholder: 'Lemon garlic chicken' },
          { name: 'photo', label: 'Photo link', type: 'url', placeholder: 'https://…', help: 'A link to a picture of the dish (optional)' },
          { name: 'minutes', label: 'Total time in minutes', type: 'number', min: 0, step: 1 },
          { name: 'servings', label: 'Servings', type: 'number', min: 1, step: 1 },
          { name: 'tags', label: 'Tags', type: 'tags', suggestions: allTags },
          { name: 'ingredients', label: 'Ingredients', type: 'textarea', placeholder: '2 cups flour\n1 tsp salt\n3 eggs', help: 'One per line, amount first so it can be scaled. A line ending in ":" starts a group.' },
          { name: 'steps', label: 'Steps', type: 'textarea', placeholder: 'Preheat the oven to 200°C.\nMix the dry ingredients.', help: 'One step per line; they are numbered for you.' },
        ],
        values: editing ? {
          title: editing.title || '', photo: editing.photo || '',
          minutes: editing.minutes ?? '', servings: editing.servings ?? '',
          tags: editing.tags || [], ingredients: editing.ingredients || '', steps: editing.steps || '',
        } : { tags: [] },
        submitLabel: editing ? 'Save changes' : 'Add recipe',
        onCancel: () => { if (editing) openDetail(editing.id); else backToList(); },
        onSubmit: async (v) => {
          const title = (v.title || '').trim();
          if (!title) throw new Error('Give the recipe a title.');
          let photo = (v.photo || '').trim();
          if (photo) {
            photo = lib.validate.url(photo);
            if (!photo) throw new Error('That photo link does not look like a web address.');
          }
          const tags = Array.from(new Set((Array.isArray(v.tags) ? v.tags : String(v.tags || '').split(','))
            .map((t) => String(t).trim().toLowerCase()).filter(Boolean)));
          const row = {
            title, photo: photo || null,
            minutes: toInt(v.minutes), servings: toInt(v.servings),
            ingredients: (v.ingredients || '').trim(), steps: (v.steps || '').trim(), tags,
          };
          if (editing) {
            await app.update('recipes', editing.id, row);
            app.toast('Recipe saved');
            openDetail(editing.id);
          } else {
            const saved = await app.insert('recipes', row);
            app.toast('Recipe added');
            openDetail(saved.id);
          }
        },
      })));
    show('edit');
  }

  app.onChange(['recipes'], () => {
    if (mode === 'list') loadList();
    else if (mode === 'detail') loadDetail();
  });

  if (app.params && app.params.id) openDetail(app.params.id);
  else show('list');
  loadList();
}
