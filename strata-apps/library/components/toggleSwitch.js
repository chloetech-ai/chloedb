// title: Toggle switch
// description: An on/off switch with a label and an optional line of explanation, for settings and preferences (email reminders, dark mode, show completed tasks); tap the switch or its label to flip it.
// props: checked: bool, whether it is on (default false); onChange(checked) when the person flips it, with the new state
//   (it may return a Promise: the switch waits for it and flips back when it fails);
//   label: text beside the switch (optional; without it the switch is named "Toggle" for screen readers);
//   description: small muted text under the label (optional); disabled: true to show it without letting it change (default false)
function component(props) {
  css('sa-switch', `
.sa-switch{display:flex;align-items:flex-start;gap:12px;cursor:pointer;user-select:none;-webkit-tap-highlight-color:transparent;margin:0;font-weight:normal}
.sa-switch .sa-switch-text{flex:1;min-width:0;display:flex;flex-direction:column;gap:2px;padding-top:1px;overflow-wrap:anywhere}
.sa-switch .sa-switch-label{color:var(--fg);line-height:1.35}
.sa-switch .sa-switch-desc{color:var(--muted);font-size:.85em;line-height:1.35}
.sa-switch .sa-switch-box{position:relative;flex:none;width:44px;height:24px}
.sa-switch input{position:absolute;inset:0;width:100%;height:100%;margin:0;padding:0;opacity:0;cursor:inherit;z-index:1;-webkit-appearance:none;appearance:none}
.sa-switch .sa-switch-track{position:absolute;inset:0;border-radius:999px;background:var(--line);transition:background .15s ease}
.sa-switch .sa-switch-knob{position:absolute;top:3px;left:3px;width:18px;height:18px;border-radius:50%;background:var(--card);box-shadow:0 1px 3px rgba(0,0,0,.3);transition:transform .15s ease}
.sa-switch input:checked + .sa-switch-track{background:var(--accent)}
.sa-switch input:checked + .sa-switch-track .sa-switch-knob{transform:translateX(20px)}
.sa-switch input:focus-visible + .sa-switch-track{outline:2px solid var(--accent);outline-offset:2px}
.sa-switch.busy{cursor:progress}
.sa-switch.busy .sa-switch-track{opacity:.7}
.sa-switch.off{cursor:not-allowed;opacity:.55}
@media (prefers-reduced-motion: reduce){.sa-switch .sa-switch-track,.sa-switch .sa-switch-knob{transition:none}}
`);
  const disabled = !!props.disabled;
  const label = props.label != null && props.label !== '' ? String(props.label) : '';
  const description = props.description != null && props.description !== '' ? String(props.description) : '';
  const uid = 'sa-sw-' + Math.random().toString(36).slice(2, 9);

  const input = h('input', {
    type: 'checkbox',
    role: 'switch',
    checked: !!props.checked,
    disabled,
    'aria-label': label ? null : 'Toggle',
    'aria-labelledby': label ? uid + '-l' : null,
    'aria-describedby': description ? uid + '-d' : null,
  });

  const root = h('label', { class: `sa-switch${disabled ? ' off' : ''}` },
    (label || description) ? h('span', { class: 'sa-switch-text' },
      label ? h('span', { class: 'sa-switch-label', id: uid + '-l' }, label) : null,
      description ? h('span', { class: 'sa-switch-desc', id: uid + '-d' }, description) : null
    ) : null,
    h('span', { class: 'sa-switch-box' },
      input,
      h('span', { class: 'sa-switch-track', 'aria-hidden': 'true' }, h('span', { class: 'sa-switch-knob' }))
    )
  );

  input.addEventListener('change', () => {
    const next = input.checked;
    if (typeof props.onChange !== 'function') return;
    let result;
    try { result = props.onChange(next); }
    catch (e) { input.checked = !next; return; }
    if (result && typeof result.then === 'function') {
      input.disabled = true;
      input.setAttribute('aria-busy', 'true');
      root.classList.add('busy');
      const done = () => {
        input.disabled = disabled;
        input.removeAttribute('aria-busy');
        root.classList.remove('busy');
      };
      result.then(done, () => { input.checked = !next; done(); });
    }
  });

  return root;
}
