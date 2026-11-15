//! `chloedb-server cc`: the command center in a terminal. It reads the
//! routes the web command center reads (`/v1/cluster`, `/v1/health/fleet`,
//! `/v1/alerts`, `/v1/queries/stats`, `/v1/logs`, `/v1/index/tables`,
//! `/v1/events`) every two seconds and sends its changes to the same
//! routes, with the same key: it needs nothing new on the server, and a
//! key may do here exactly what it may do there.
//!
//! The key comes from `CHLOEDB_KEY` (or `CHLOEDB_TOKEN`) or a prompt that
//! does not echo, never from a flag, so it stays out of the shell's history
//! and `ps`. Through a hub, the URL is the connection's
//! (`https://hub:7700/c/prod`) and the key is the hub's: the hub adds the
//! cluster's own key, which this terminal never holds, and `:ctx` lists the
//! hub's other connections.
//!
//! The screens: [`view`]; their keys: [`app`]; the calls: [`api`]; what a
//! copy and a shard are, as the web's Cluster view reads them: [`model`].

pub mod api;
pub mod app;
pub mod model;
pub mod view;

use std::io::IsTerminal;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyEventKind};
use tokio::sync::{mpsc, Notify};

use api::{lock, Api, Job, Shared, Tone, Who};
use app::{App, Effect};

/// The key from the environment: `CHLOEDB_KEY`, else `CHLOEDB_TOKEN` (and
/// their old `STRATA_` names).
pub fn key_from_env() -> Option<String> {
    ["CHLOEDB_KEY", "CHLOEDB_TOKEN"].iter().find_map(|v| chloedb::env::var(v).ok()).map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
}

/// Asks for the key on the terminal without echoing it; empty for none
/// (a server on loopback with no token).
fn ask_key(url: &str) -> Result<Option<String>, String> {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};
    use ratatui::crossterm::terminal;
    use std::io::Write as _;
    eprint!("Key for {url} (sk_… or the server's token; not shown; ⏎ for none): ");
    let _ = std::io::stderr().flush();
    terminal::enable_raw_mode().map_err(|e| format!("the terminal: {e}"))?;
    let mut key = String::new();
    let r = loop {
        match event::read() {
            Ok(Event::Key(k)) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Enter => break Ok(()),
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => break Err("cancelled".to_string()),
                KeyCode::Esc => break Err("cancelled".to_string()),
                KeyCode::Backspace => {
                    key.pop();
                }
                KeyCode::Char(c) => key.push(c),
                _ => {}
            },
            Ok(Event::Paste(p)) => key.push_str(p.trim()),
            Ok(_) => {}
            Err(e) => break Err(e.to_string()),
        }
    };
    let _ = terminal::disable_raw_mode();
    eprintln!();
    r?;
    Ok(Some(key.trim().to_string()).filter(|k| !k.is_empty()))
}

/// Runs the terminal until `q`: reads `url` every `every`, with the key
/// from the environment or the prompt, trusting `ca` beside the system's
/// authorities for an https URL.
pub async fn run(url: &str, ca: Option<Vec<u8>>, every: Duration) -> Result<(), String> {
    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        return Err("cc draws in a terminal, and this is not one: for a script, use `chloedb-server admin`".into());
    }
    view::plain(std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()));
    let url = url.trim_end_matches('/').to_string();
    let key = match key_from_env() {
        Some(k) => Some(k),
        None => ask_key(&url)?,
    };
    let api = Api::new(&url, key, ca.as_deref())?;
    let who = match api.get("/v1/whoami").await {
        Ok(j) => Who::of(&j),
        Err(e) if e.status == Some(401) => return Err(format!("{url} refused the key: {}. Give another in CHLOEDB_KEY, or at the prompt.", e.message)),
        Err(e) if e.status == Some(404) => return Err(format!("{url} has no /v1/whoami: is it a ChloeDB server? Through a hub, the URL is https://hub:7700/c/<connection>")),
        Err(e) => return Err(e.to_string()),
    };
    let shared = Arc::new(Mutex::new(Shared::new(&url, who)));
    let wake = Arc::new(Notify::new());
    let (tx, mut rx) = mpsc::unbounded_channel::<Job>();
    let poller = tokio::spawn(api::poll(api.clone(), shared.clone(), wake.clone(), every));
    let jobs = {
        let (api, shared, wake) = (api.clone(), shared.clone(), wake.clone());
        tokio::spawn(async move {
            while let Some(job) = rx.recv().await {
                tokio::spawn(api::run_job(api.clone(), shared.clone(), wake.clone(), job));
            }
        })
    };
    let ui = {
        let (shared, wake) = (shared.clone(), wake.clone());
        tokio::task::spawn_blocking(move || screen(shared, wake, tx))
    };
    let r = ui.await.map_err(|e| format!("the terminal: {e}"))?;
    poller.abort();
    jobs.abort();
    r
}

/// The terminal's loop: draw, wait a quarter second for a key, act on it.
fn screen(shared: Arc<Mutex<Shared>>, wake: Arc<Notify>, tx: mpsc::UnboundedSender<Job>) -> Result<(), String> {
    let mut term = ratatui::try_init().map_err(|e| format!("the terminal: {e}"))?;
    let _ = ratatui::crossterm::execute!(std::io::stdout(), event::EnableBracketedPaste);
    let mut app = App::default();
    let r = (|| -> Result<(), String> {
        loop {
            {
                let s = lock(&shared);
                term.draw(|f| {
                    let area = f.area();
                    view::draw(f.buffer_mut(), area, &app, &s);
                })
                .map_err(|e| e.to_string())?;
            }
            if !event::poll(Duration::from_millis(250)).map_err(|e| e.to_string())? {
                continue;
            }
            let ev = event::read().map_err(|e| e.to_string())?;
            let effect = match ev {
                Event::Key(k) if k.kind != KeyEventKind::Release => {
                    let s = lock(&shared);
                    let seen = app::Seen { data: &s.data, who: &s.who, base: &s.base, hub: s.hub.as_ref(), plan_running: view::plan_running(&s.data.cluster) };
                    app.key(k, &seen)
                }
                Event::Paste(p) => {
                    paste(&mut app, &p);
                    Effect::None
                }
                _ => Effect::None,
            };
            let mut s = lock(&shared);
            // The poller reads what the screen on show needs.
            let want = api::Want { screen: app.screen, log_level: app.log_level().map(String::from), log_q: app.log_q.clone() };
            let changed = s.want != want;
            s.want = want;
            match effect {
                Effect::None => {}
                Effect::Quit => return Ok(()),
                Effect::Read => wake.notify_one(),
                Effect::Pause => {
                    s.paused = !s.paused;
                    let text = if s.paused { "Paused: the screens hold what they show. Space follows live again." } else { "Live again." };
                    s.say(text, Tone::Info);
                    wake.notify_one();
                }
                Effect::Say(t) => s.say(t, Tone::Bad),
                Effect::Run(job) => {
                    if matches!(job.act, api::Act::Hub { .. }) {
                        s.hub = None;
                    }
                    let _ = tx.send(job);
                }
                Effect::Switch(base) => {
                    s.base = base.clone();
                    s.gen += 1;
                    s.data = api::Data::default();
                    s.errors.clear();
                    s.read_at = None;
                    s.sql = None;
                    s.say(format!("Opening {base}"), Tone::Info);
                    let _ = tx.send(Job { act: api::Act::WhoAmI, reason: None });
                    wake.notify_one();
                }
            }
            if changed {
                wake.notify_one();
            }
        }
    })();
    let _ = ratatui::crossterm::execute!(std::io::stdout(), event::DisableBracketedPaste);
    ratatui::restore();
    r
}

/// Pasted text goes to whatever is being typed into.
fn paste(app: &mut App, text: &str) {
    match &mut app.overlay {
        Some(app::Overlay::Dialog(d)) => {
            if let Some(f) = d.fields.get_mut(d.focus) {
                f.input.paste(text);
            }
        }
        Some(app::Overlay::Command(i) | app::Overlay::Filter(i)) => i.paste(text),
        _ if app.screen == app::Screen::Sql && app.sql_focus => app.sql.paste(&text.replace(['\n', '\r'], " ")),
        _ => {}
    }
}
