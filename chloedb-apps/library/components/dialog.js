// title: Dialog
// description: A modal box over the page with a title, any content and action buttons; on a phone it slides up as a sheet. Closes with Escape, the cross or a click outside.
// props: title; body: an element, a string or an array of them; actions: [{label, primary, danger, onClick(close)}] (a click closes it unless onClick returns false);
//   onClose() when it closes; width in px (default 440); dismissable (default true).
//   It returns the overlay: put it in the page with document.body.append(ui.dialog({...})); the element's close() closes it from code.
function component(props) {
  css('sa-dialog', `
.sa-dialog{position:fixed;inset:0;z-index:1000;background:rgba(15,18,25,.45);display:flex;align-items:center;justify-content:center;padding:16px;animation:sa-dlg-in .12s ease-out}
.sa-dialog .box{background:var(--card);color:var(--fg);border:1px solid var(--line);border-radius:14px;box-shadow:0 18px 50px rgba(0,0,0,.25);width:100%;max-height:calc(100vh - 32px);display:flex;flex-direction:column;overflow:hidden}
.sa-dialog header{display:flex;align-items:center;gap:8px;padding:14px 16px 6px}
.sa-dialog header b{font-size:16px;flex:1}
.sa-dialog header button{border:0;box-shadow:none;background:none;color:var(--muted);font-size:18px;padding:2px 8px;cursor:pointer}
.sa-dialog .content{padding:6px 16px 14px;overflow:auto}
.sa-dialog footer{display:flex;justify-content:flex-end;gap:8px;padding:10px 16px 14px;border-top:1px solid var(--line)}
@keyframes sa-dlg-in{from{opacity:0}to{opacity:1}}
@media (max-width:560px){.sa-dialog{align-items:flex-end;padding:0}.sa-dialog .box{border-radius:16px 16px 0 0;max-height:88vh;max-width:none!important}}
`);
  const dismissable = props.dismissable !== false;
  let closed = false;
  const close = () => {
    if (closed) return;
    closed = true;
    document.removeEventListener('keydown', onKey);
    root.remove();
    if (props.onClose) props.onClose();
  };
  const onKey = (e) => { if (e.key === 'Escape' && dismissable) close(); };
  const body = [].concat(props.body === undefined ? [] : props.body).map((b) => (typeof b === 'string' ? h('p', null, b) : b));
  const actions = (props.actions || []).map((a) => h('button', { type: 'button', class: [a.primary ? 'primary' : '', a.danger ? 'danger' : ''].filter(Boolean).join(' ') || null,
    onclick: async () => { const r = a.onClick ? await a.onClick(close) : undefined; if (r !== false) close(); } }, a.label));
  const box = h('div', { class: 'box', role: 'dialog', 'aria-modal': 'true', 'aria-label': props.title || 'Dialog', style: { maxWidth: `${props.width || 440}px` } },
    h('header', null, h('b', null, props.title || ''), dismissable ? h('button', { type: 'button', 'aria-label': 'Close', onclick: close }, '✕') : null),
    h('div', { class: 'content' }, body),
    actions.length ? h('footer', null, actions) : null);
  const root = h('div', { class: 'sa-dialog', onclick: (e) => { if (e.target === root && dismissable) close(); } }, box);
  root.close = close;
  document.addEventListener('keydown', onKey);
  setTimeout(() => { const f = box.querySelector('input, textarea, select, button.primary'); if (f) f.focus(); }, 30);
  return root;
}
