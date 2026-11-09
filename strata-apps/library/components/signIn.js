// title: Sign in
// description: A sign-in box with a switch to create an account (email, name, password), its errors shown in place; when someone is signed in already it shows who, with a sign-out button. One account works in every app on this engine.
// props: user: app.me()'s answer, to show the signed-in state instead (null or missing shows the form); mode: 'in' (default) or 'up' to start on creating an account;
//   title (default 'Sign in' or 'Create an account'); note: a line under the title (what signing in gives); onDone(user) after it worked
//   (every view of the app starts again right after, seeing the person)
function component(props) {
  css('sa-signin', `
.sa-signin{max-width:360px;width:100%;display:flex;flex-direction:column;gap:10px}
.sa-signin form{display:flex;flex-direction:column;gap:8px;align-items:stretch}
.sa-signin label{display:flex;flex-direction:column;gap:4px;font-size:13px;color:var(--muted)}
.sa-signin .err{color:var(--bad);font-size:13px;min-height:1em}
.sa-signin .switch{font-size:13px;color:var(--muted)}
.sa-signin .switch button{border:0;box-shadow:none;background:none;color:var(--accent);padding:0;cursor:pointer;font:inherit}
.sa-signin .who{display:flex;align-items:center;gap:10px}
`);
  if (props.user) {
    const u = props.user;
    return h('div', { class: 'sa-signin card' },
      h('div', { class: 'who' }, ui.avatar({ name: u.name, email: u.email, size: 36 }), h('div', { class: 'grow' }, h('b', null, u.name), h('div', { class: 'small muted' }, `${u.email} · ${u.role}`))),
      h('button', { type: 'button', onclick: async (e) => { e.target.disabled = true; await app.signOut(); } }, 'Sign out'));
  }
  let mode = props.mode === 'up' ? 'up' : 'in';
  const root = h('div', { class: 'sa-signin card' });
  function draw() {
    const email = h('input', { type: 'email', autocomplete: 'email', required: true, placeholder: 'you@example.com' });
    const name = mode === 'up' ? h('input', { autocomplete: 'name', placeholder: 'Your name' }) : null;
    const pass = h('input', { type: 'password', autocomplete: mode === 'up' ? 'new-password' : 'current-password', required: true, minlength: mode === 'up' ? 8 : null, placeholder: mode === 'up' ? 'At least 8 characters' : '' });
    const err = h('div', { class: 'err', role: 'alert' });
    const go = h('button', { class: 'primary', type: 'submit' }, mode === 'up' ? 'Create account' : 'Sign in');
    const form = h('form', { onsubmit: async (e) => {
      e.preventDefault();
      err.textContent = '';
      go.disabled = true;
      try {
        const user = mode === 'up' ? await app.signUp({ email: email.value, name: name.value, password: pass.value }) : await app.signIn(email.value, pass.value);
        if (props.onDone) props.onDone(user);
        go.textContent = 'Signed in';
      } catch (x) { err.textContent = x.message; go.disabled = false; }
    } },
    h('label', null, 'Email', email), name ? h('label', null, 'Name', name) : null, h('label', null, 'Password', pass), err, go);
    root.replaceChildren(
      h('h3', { style: 'margin:0' }, props.title || (mode === 'up' ? 'Create an account' : 'Sign in')),
      props.note ? h('div', { class: 'small muted' }, props.note) : null,
      form,
      h('div', { class: 'switch' }, mode === 'up' ? 'Have an account? ' : 'New here? ',
        h('button', { type: 'button', onclick: () => { mode = mode === 'up' ? 'in' : 'up'; draw(); } }, mode === 'up' ? 'Sign in' : 'Create an account')));
  }
  draw();
  return root;
}
