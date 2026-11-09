// title: Timer
// description: A big countdown or stopwatch with start, pause and reset (cooking, workouts, focus sessions, practice), which reports when a countdown ends and how long the stopwatch ran.
// props: mode: 'countdown' (default) or 'stopwatch'; seconds: the countdown's length (default 300); presets: [seconds] shown as quick picks for a countdown;
//   label: text above the time; autoStart: true to start at once; onDone() when a countdown reaches 0;
//   onStop(seconds) when the stopwatch is reset after running (the seconds it ran) or a countdown is reset early (the seconds it ran)
function component(props) {
  css('sa-timer', `
.sa-timer{display:flex;flex-direction:column;align-items:center;gap:10px;padding:16px;background:var(--card);border:1px solid var(--line);border-radius:var(--radius);box-shadow:var(--shadow)}
.sa-timer-label{color:var(--muted);font-size:13px}
.sa-timer-time{font-size:52px;font-weight:650;letter-spacing:-.02em;font-variant-numeric:tabular-nums;line-height:1}
.sa-timer.done .sa-timer-time{color:var(--ok)}
.sa-timer-bar{width:100%;max-width:280px;height:4px;border-radius:2px;background:var(--soft);overflow:hidden}
.sa-timer-bar i{display:block;height:100%;background:var(--accent)}
.sa-timer-btns,.sa-timer-presets{display:flex;gap:8px;flex-wrap:wrap;justify-content:center}
`);
  const countdown = props.mode !== 'stopwatch';
  let total = Math.max(1, Math.round(Number(props.seconds) || 300));
  let elapsed = 0;
  let startedAt = null;
  let tick = null;
  let finished = false;
  const time = h('div', { class: 'sa-timer-time', role: 'timer', 'aria-live': 'off' });
  const bar = countdown ? h('div', { class: 'sa-timer-bar' }, h('i')) : null;
  const btns = h('div', { class: 'sa-timer-btns' });
  const root = h('div', { class: 'sa-timer' }, props.label ? h('div', { class: 'sa-timer-label' }, props.label) : null, time, bar, btns);
  const now = () => elapsed + (startedAt ? (Date.now() - startedAt) / 1000 : 0);
  const fmt = (s) => { s = Math.max(0, Math.round(s)); const hh = Math.floor(s / 3600); const mm = Math.floor((s % 3600) / 60); const ss = s % 60; const p = (n) => String(n).padStart(2, '0'); return hh ? `${hh}:${p(mm)}:${p(ss)}` : `${p(mm)}:${p(ss)}`; };
  function draw() {
    const run = now();
    time.textContent = fmt(countdown ? total - run : run);
    if (bar) bar.firstChild.style.width = `${Math.min(100, (100 * run) / total)}%`;
    root.classList.toggle('done', finished);
    btns.replaceChildren(
      startedAt ? h('button', { type: 'button', onclick: pause }, 'Pause') : h('button', { class: 'primary', type: 'button', disabled: finished, onclick: start }, run > 0 && !finished ? 'Resume' : 'Start'),
      h('button', { type: 'button', disabled: run === 0 && !finished, onclick: reset }, 'Reset'));
  }
  function start() {
    if (startedAt || finished) return;
    startedAt = Date.now();
    tick = setInterval(() => {
      if (countdown && now() >= total) { elapsed = total; startedAt = null; clearInterval(tick); tick = null; finished = true; draw(); if (props.onDone) props.onDone(); return; }
      draw();
    }, 250);
    draw();
  }
  function pause() { elapsed = now(); startedAt = null; clearInterval(tick); tick = null; draw(); }
  function reset() {
    const ran = Math.round(now());
    clearInterval(tick); tick = null; startedAt = null; elapsed = 0;
    const was = finished;
    finished = false;
    draw();
    if (ran > 0 && !was && props.onStop) props.onStop(ran);
  }
  if (countdown && Array.isArray(props.presets) && props.presets.length) {
    root.append(h('div', { class: 'sa-timer-presets' }, props.presets.map((s) => h('button', { class: 'small', type: 'button', onclick: () => { reset(); total = Math.max(1, Math.round(Number(s))); draw(); } }, fmt(s)))));
  }
  draw();
  if (props.autoStart) start();
  return root;
}
