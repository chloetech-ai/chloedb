// title: Uploader
// description: A drop zone for files: drop them, paste them or pick them, see each upload's progress and a preview, and get back the stored files (id, name, type, size, url) to keep in a row.
// props: onUpload(files) with [{id, name, type, size, url}] once each batch is stored; accept (as on an input: 'image/*', '.pdf,.csv'; default any);
//   multiple (default true); maxMB (default 15, the engine's limit); label (default 'Drop files here or choose them');
//   compact: true for a single button instead of a zone; paste: true to take files pasted anywhere in the page (default false)
function component(props) {
  css('sa-upload', `
.sa-upload{border:2px dashed var(--line);border-radius:12px;padding:18px;text-align:center;color:var(--muted);cursor:pointer;transition:border-color .12s,background .12s;display:flex;flex-direction:column;gap:8px;align-items:center}
.sa-upload.over{border-color:var(--accent);background:color-mix(in srgb,var(--accent) 8%,transparent)}
.sa-upload .big{font-size:22px;line-height:1}
.sa-upload .items{display:flex;flex-wrap:wrap;gap:8px;justify-content:center;width:100%}
.sa-upload .it{display:flex;align-items:center;gap:6px;font-size:12.5px;background:var(--soft);border-radius:8px;padding:4px 8px;max-width:240px}
.sa-upload .it img{width:28px;height:28px;object-fit:cover;border-radius:4px}
.sa-upload .it.bad{color:var(--bad)}
`);
  const maxMB = Number(props.maxMB) || 15;
  const multiple = props.multiple !== false;
  const input = h('input', { type: 'file', accept: props.accept || null, multiple, hidden: true });
  const items = h('div', { class: 'items' });
  const ok = (f) => {
    if (f.size > maxMB * 1024 * 1024) return `${f.name}: larger than ${maxMB} MB`;
    if (props.accept) {
      const rules = String(props.accept).split(',').map((s) => s.trim().toLowerCase()).filter(Boolean);
      const name = f.name.toLowerCase(); const type = (f.type || '').toLowerCase();
      if (!rules.some((r) => (r.startsWith('.') ? name.endsWith(r) : r.endsWith('/*') ? type.startsWith(r.slice(0, -1)) : type === r))) return `${f.name}: not a kind of file this takes`;
    }
    return null;
  };
  async function take(list) {
    let files = Array.from(list || []);
    if (!files.length) return;
    if (!multiple) files = files.slice(0, 1);
    const done = [];
    await Promise.all(files.map(async (f) => {
      const problem = ok(f);
      const row = h('span', { class: `it${problem ? ' bad' : ''}` }, f.type.startsWith('image/') && !problem ? h('img', { src: URL.createObjectURL(f), alt: '' }) : null, h('span', { class: 'clip' }, problem || `${f.name} · uploading…`));
      items.append(row);
      if (problem) return;
      try {
        const stored = await app.upload(f);
        row.lastChild.textContent = f.name;
        done.push(stored);
      } catch (e) { row.classList.add('bad'); row.lastChild.textContent = `${f.name}: ${e.message}`; }
    }));
    setTimeout(() => { for (const r of [...items.children]) if (!r.classList.contains('bad')) r.remove(); }, 2500);
    if (done.length && props.onUpload) props.onUpload(done);
  }
  input.addEventListener('change', () => { take(input.files); input.value = ''; });
  if (props.paste) document.addEventListener('paste', (e) => { if (e.clipboardData && e.clipboardData.files.length) take(e.clipboardData.files); });
  if (props.compact) {
    return h('span', { style: 'display:inline-flex;gap:8px;align-items:center;flex-wrap:wrap' }, input, h('button', { type: 'button', onclick: () => input.click() }, props.label || (multiple ? 'Add files' : 'Add a file')), items);
  }
  const zone = h('div', { class: 'sa-upload', tabindex: 0, role: 'button', 'aria-label': props.label || 'Upload files', onclick: () => input.click(), onkeydown: (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); input.click(); } } },
    input, h('div', { class: 'big' }, '⇪'), h('div', null, props.label || (multiple ? 'Drop files here or choose them' : 'Drop a file here or choose one')),
    h('div', { class: 'small' }, `Up to ${maxMB} MB${props.accept ? ` · ${props.accept}` : ''}`), items);
  zone.addEventListener('dragover', (e) => { e.preventDefault(); zone.classList.add('over'); });
  zone.addEventListener('dragleave', () => zone.classList.remove('over'));
  zone.addEventListener('drop', (e) => { e.preventDefault(); zone.classList.remove('over'); take(e.dataTransfer.files); });
  return zone;
}
