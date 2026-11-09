// title: Email composer
// description: Write and send a plain-text email from the app: recipients with suggestions, subject and text, checked before sending, with the outcome said plainly (queued when the engine has no mail server).
// props: to: an address or [addresses] to start with; subject and text to start with; people: [{name, email}] suggested as recipients (default: the app's members);
//   onSend(msg) to send it yourself instead (msg is {to: [...], subject, text}; default app.email(msg)); onSent(result, msg) afterwards;
//   sendLabel (default 'Send'); signature: a line added under the text
function component(props) {
  css('sa-mail', `
.sa-mail{display:flex;flex-direction:column;gap:8px}
.sa-mail label{display:flex;flex-direction:column;gap:4px;font-size:13px;color:var(--muted)}
.sa-mail textarea{min-height:140px;resize:vertical;font-family:inherit}
.sa-mail .bar{display:flex;align-items:center;gap:10px;flex-wrap:wrap}
.sa-mail .msg{font-size:13px}
.sa-mail .msg.bad{color:var(--bad)}
`);
  let to = [].concat(props.to || []).flatMap((x) => String(x).split(',')).map((x) => x.trim().toLowerCase()).filter(Boolean);
  const tagsBox = h('div');
  const drawTo = (suggest) => tagsBox.replaceChildren(ui.tagInput({ value: to, lower: true, placeholder: 'name@example.com', suggestions: suggest, onChange: (v) => { to = v; } }));
  drawTo((props.people || []).map((p) => p.email).filter(Boolean));
  if (!props.people) app.people().then((ps) => drawTo((ps || []).map((p) => p.email).filter(Boolean))).catch(() => {});
  const subject = h('input', { value: props.subject || '', placeholder: 'Subject' });
  const text = h('textarea', { value: props.text || '', placeholder: 'Write your message' });
  const msg = h('span', { class: 'msg' });
  const send = h('button', { type: 'button', class: 'primary', onclick: go }, props.sendLabel || 'Send');
  async function go() {
    msg.className = 'msg bad';
    if (!to.length) { msg.textContent = 'Add who it goes to'; return; }
    const bad = to.find((x) => !lib.validate.isEmail(x));
    if (bad) { msg.textContent = `${bad} is not an email address`; return; }
    if (!subject.value.trim() && !text.value.trim()) { msg.textContent = 'Write a subject or a message'; return; }
    const body = props.signature ? `${text.value.trimEnd()}\n\n${props.signature}` : text.value;
    const m = { to: to.slice(), subject: subject.value.trim(), text: body };
    send.disabled = true;
    msg.className = 'msg muted'; msg.textContent = 'Sending…';
    try {
      const r = props.onSend ? await props.onSend(m) : await app.email(m);
      msg.className = 'msg';
      msg.textContent = r && r.status === 'held' ? 'Queued in the outbox: this engine has no mail server yet, so it waits there.' : r && r.status === 'failed' ? 'The mail server refused it.' : r && r.status === 'sending' ? 'On its way: handed to the mail server.' : 'Sent.';
      if (props.onSent) props.onSent(r, m);
    } catch (e) { msg.className = 'msg bad'; msg.textContent = e.message; }
    send.disabled = false;
  }
  return h('div', { class: 'sa-mail' }, h('label', null, 'To', tagsBox), h('label', null, 'Subject', subject), h('label', null, 'Message', text), h('div', { class: 'bar' }, send, msg));
}
