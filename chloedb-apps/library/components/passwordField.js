// title: Password field
// description: A password box with a button to show or hide what was typed and, for new passwords, a strength meter with a hint; for sign-in, sign-up and change-password forms.
// props: value: text to start with (default ''); onChange(text) on every keystroke; label: text above the box (default 'Password'; '' for none);
//   placeholder (default none); showStrength: true to show the strength meter and the minimum length hint, for new passwords (default false);
//   autocomplete: the input's autocomplete (default 'new-password' when showStrength, else 'current-password');
//   minLength: the fewest characters a new password needs (default 8)
function component(props) {
  css('sa-password', `
.sa-password{display:flex;flex-direction:column;gap:4px;min-width:0;width:100%}
.sa-password .sa-pw-box{display:flex;align-items:stretch;position:relative;min-width:0}
.sa-password input{flex:1;min-width:0;width:100%;padding-right:64px}
.sa-password .sa-pw-toggle{position:absolute;right:4px;top:50%;transform:translateY(-50%);border:0;background:none;box-shadow:none;color:var(--muted);font-size:.85em;padding:4px 8px;cursor:pointer;border-radius:var(--radius)}
.sa-password .sa-pw-toggle:hover,.sa-password .sa-pw-toggle:focus-visible{color:var(--fg)}
.sa-password .sa-pw-bars{display:flex;gap:4px;margin-top:2px}
.sa-password .sa-pw-bars span{flex:1;height:4px;border-radius:2px;background:var(--line);transition:background .15s}
.sa-password .sa-pw-note{display:flex;justify-content:space-between;gap:8px;font-size:.8em;color:var(--muted);min-height:1.2em}
.sa-password .sa-pw-note b{font-weight:600;color:var(--c,var(--muted))}
`);
  const minLength = Math.max(1, Math.floor(Number(props.minLength)) || 8);
  const showStrength = !!props.showStrength;
  const label = props.label === undefined || props.label === null ? 'Password' : String(props.label);
  const autocomplete = props.autocomplete || (showStrength ? 'new-password' : 'current-password');
  const uid = 'sa-pw-' + Math.random().toString(36).slice(2, 9);
  let shown = false;

  const COMMON = ['password', 'passw0rd', 'qwerty', 'azerty', '123456', '12345678', '111111', 'letmein', 'iloveyou', 'admin', 'welcome', 'monkey', 'dragon', 'abc123', 'football', 'baseball', 'sunshine', 'princess', 'trustno1'];
  const SEQS = ['0123456789', 'abcdefghijklmnopqrstuvwxyz', 'qwertyuiop', 'asdfghjkl', 'zxcvbnm'];

  function strength(pw) {
    if (!pw) return { score: -1, text: '' };
    const len = [...pw].length;
    if (len < minLength) return { score: 0, text: 'Too short' };
    const classes = [/[a-z]/, /[A-Z]/, /[0-9]/, /[^A-Za-z0-9]/].filter((r) => r.test(pw)).length;
    let score = 1;
    if (classes >= 2) score++;
    if (classes >= 3) score++;
    if (len >= 12) score++;
    if (len >= 16) score++;
    const low = pw.toLowerCase();
    const weak = /^(.)\1*$/.test(pw)
      || COMMON.some((w) => low.includes(w))
      || SEQS.some((s) => s.includes(low) || [...s].reverse().join('').includes(low))
      || new Set(low).size <= 2;
    if (weak) score = 1;
    score = Math.min(4, score);
    return { score, text: ['Too short', 'Weak', 'Fair', 'Good', 'Strong'][score] };
  }

  const COLORS = ['var(--bad)', 'var(--bad)', 'var(--warn)', 'var(--accent)', 'var(--ok)'];

  const input = h('input', {
    id: uid,
    type: 'password',
    value: props.value == null ? '' : String(props.value),
    placeholder: props.placeholder || null,
    autocomplete,
    autocapitalize: 'off',
    autocorrect: 'off',
    spellcheck: 'false',
    minlength: showStrength ? String(minLength) : null,
    'aria-describedby': showStrength ? uid + '-note' : null,
    oninput: () => {
      update();
      if (typeof props.onChange === 'function') props.onChange(input.value);
    },
  });

  const toggle = h('button', {
    type: 'button',
    class: 'sa-pw-toggle',
    'aria-controls': uid,
    'aria-pressed': 'false',
    'aria-label': 'Show password',
    onclick: () => {
      shown = !shown;
      const start = input.selectionStart, end = input.selectionEnd;
      input.type = shown ? 'text' : 'password';
      toggle.textContent = shown ? 'Hide' : 'Show';
      toggle.setAttribute('aria-pressed', shown ? 'true' : 'false');
      toggle.setAttribute('aria-label', shown ? 'Hide password' : 'Show password');
      input.focus();
      try { input.setSelectionRange(start, end); } catch (e) { /* not supported */ }
    },
  }, 'Show');

  const bars = showStrength ? h('div', { class: 'sa-pw-bars', 'aria-hidden': 'true' }, [0, 1, 2, 3].map(() => h('span'))) : null;
  const verdict = h('b');
  const hint = h('span');
  const note = showStrength ? h('div', { class: 'sa-pw-note', id: uid + '-note', 'aria-live': 'polite' }, verdict, hint) : null;

  function update() {
    if (!showStrength) return;
    const pw = input.value;
    const { score, text } = strength(pw);
    const color = score >= 0 ? COLORS[score] : 'var(--line)';
    const filled = score <= 0 ? (score === 0 ? 1 : 0) : score;
    [...bars.children].forEach((b, i) => { b.style.background = i < filled ? color : ''; });
    note.style.setProperty('--c', color);
    verdict.textContent = text ? 'Strength: ' + text : '';
    const len = [...pw].length;
    hint.textContent = len < minLength
      ? `At least ${minLength} characters${len ? ` (${minLength - len} more)` : ''}`
      : score < 3 ? 'Longer, with numbers or symbols, is stronger' : '';
  }

  const root = h('div', { class: 'sa-password' },
    label ? h('label', { for: uid }, label) : null,
    h('div', { class: 'sa-pw-box' }, input, toggle),
    bars,
    note
  );
  if (!label) input.setAttribute('aria-label', 'Password');
  update();
  return root;
}
