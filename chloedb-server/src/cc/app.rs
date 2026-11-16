//! The terminal's state, and what each key does. Nothing here draws or
//! calls the server: a key moves a selection, opens a dialog, or hands back
//! an [`Effect`] (a job to run, a pause, a switch of cluster), so that the
//! keys are tested without a terminal or a server.

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::Value as Json;

use super::api::{hub_of, Act, Data, Job, Who};
use super::model::{self, Lens};

/// The screens, in the order of their number keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Screen {
    #[default]
    Home,
    Cluster,
    Alerts,
    Queries,
    Logs,
    Tables,
    Sql,
    Timeline,
}

/// Each screen's title, and the letter that follows `g` for it, as in the web
/// command center (`g c` is Cluster there too).
pub const SCREENS: [(Screen, &str, char); 8] = [
    (Screen::Home, "Home", 'h'),
    (Screen::Cluster, "Cluster", 'c'),
    (Screen::Alerts, "Alerts", 'a'),
    (Screen::Queries, "Queries", 'q'),
    (Screen::Logs, "Logs", 'l'),
    (Screen::Tables, "Tables", 'o'),
    (Screen::Sql, "SQL", 'w'),
    (Screen::Timeline, "Timeline", 't'),
];

impl Screen {
    pub fn title(self) -> &'static str {
        SCREENS.iter().find(|s| s.0 == self).map_or("", |s| s.1)
    }

    fn index(self) -> usize {
        SCREENS.iter().position(|s| s.0 == self).unwrap_or(0)
    }

    pub fn parse(s: &str) -> Option<Screen> {
        let s = s.trim().to_ascii_lowercase();
        SCREENS.iter().find(|x| x.1.to_ascii_lowercase() == s || (s == "storage" && x.0 == Screen::Tables) || (s == "events" && x.0 == Screen::Timeline)).map(|x| x.0)
    }
}

/// The log levels the Logs screen filters by, from all of them up.
pub const LEVELS: [Option<&str>; 4] = [None, Some("info"), Some("warn"), Some("error")];

/// How the Queries screen orders statements.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum QSort {
    /// Their share of the time all statements took.
    #[default]
    Time,
    P99,
    Calls,
    Errors,
}

impl QSort {
    pub fn name(self) -> &'static str {
        match self {
            QSort::Time => "time",
            QSort::P99 => "p99",
            QSort::Calls => "calls",
            QSort::Errors => "errors",
        }
    }

    fn next(self) -> QSort {
        match self {
            QSort::Time => QSort::P99,
            QSort::P99 => QSort::Calls,
            QSort::Calls => QSort::Errors,
            QSort::Errors => QSort::Time,
        }
    }
}

/// A line of text being typed: the SQL editor, a dialog's field, `:`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Input {
    pub text: String,
    /// Where the cursor is, in chars.
    pub at: usize,
}

impl Input {
    pub fn of(text: &str) -> Input {
        Input { text: text.to_string(), at: text.chars().count() }
    }

    fn byte(&self, at: usize) -> usize {
        self.text.char_indices().nth(at).map_or(self.text.len(), |(i, _)| i)
    }

    /// Edits for a key; false when the key is not an edit.
    pub fn key(&mut self, k: &KeyEvent) -> bool {
        let n = self.text.chars().count();
        match k.code {
            KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                self.text.clear();
                self.at = 0;
            }
            KeyCode::Char('a') if k.modifiers.contains(KeyModifiers::CONTROL) => self.at = 0,
            KeyCode::Char('e') if k.modifiers.contains(KeyModifiers::CONTROL) => self.at = n,
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                let b = self.byte(self.at);
                self.text.insert(b, c);
                self.at += 1;
            }
            KeyCode::Backspace if self.at > 0 => {
                self.at -= 1;
                let b = self.byte(self.at);
                self.text.remove(b);
            }
            KeyCode::Delete if self.at < n => {
                let b = self.byte(self.at);
                self.text.remove(b);
            }
            KeyCode::Left => self.at = self.at.saturating_sub(1),
            KeyCode::Right => self.at = (self.at + 1).min(n),
            KeyCode::Home => self.at = 0,
            KeyCode::End => self.at = n,
            KeyCode::Backspace | KeyCode::Delete => {}
            _ => return false,
        }
        true
    }

    pub fn paste(&mut self, s: &str) {
        for c in s.chars().filter(|c| !c.is_control()) {
            let b = self.byte(self.at);
            self.text.insert(b, c);
            self.at += 1;
        }
    }
}

/// What a dialog's field must hold before the dialog's button works.
#[derive(Clone, Debug, PartialEq)]
pub enum Must {
    /// This text, typed: the name of what goes.
    Equal(String),
    /// A reason the audit log keeps.
    Reason,
    /// A reason if one is given.
    AnyReason,
    /// Minutes, 1 to 10,080 (a week).
    Minutes,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub label: String,
    pub input: Input,
    pub must: Must,
}

impl Field {
    pub fn ok(&self) -> bool {
        let t = self.input.text.trim();
        match &self.must {
            Must::Equal(want) => t == want,
            Must::Reason => !t.is_empty(),
            Must::AnyReason => true,
            Must::Minutes => t.parse::<u64>().is_ok_and(|m| (1..=10_080).contains(&m)),
        }
    }
}

/// What a dialog does once its fields are filled and Enter is pressed.
#[derive(Clone, Debug, PartialEq)]
pub enum Then {
    Act(Act),
    /// Silence an alert for the minutes typed.
    Silence { rule: String, name: String },
    /// Production's SQL takes writes for 15 minutes.
    AllowWrites,
}

/// A question before a change: what it does, what must be typed, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Dialog {
    pub title: String,
    pub lines: Vec<String>,
    pub fields: Vec<Field>,
    pub focus: usize,
    pub ok: String,
    /// It removes or changes what cannot be put back: red.
    pub danger: bool,
    pub then: Then,
}

impl Dialog {
    pub fn ready(&self) -> bool {
        self.fields.iter().all(Field::ok)
    }

    fn reason(&self) -> Option<String> {
        self.fields.iter().find(|f| matches!(f.must, Must::Reason | Must::AnyReason)).map(|f| f.input.text.trim().to_string()).filter(|r| !r.is_empty())
    }
}

/// What is drawn over the screen, and takes the keys while it is.
#[derive(Clone, Debug, PartialEq)]
pub enum Overlay {
    Dialog(Dialog),
    /// `:` and what is typed after it.
    Command(Input),
    /// The Logs screen's text filter.
    Filter(Input),
    /// The hub's connections, to open one (`:ctx`).
    Ctx { at: usize },
    Help,
}

/// What a key asks of the program beyond the screen's own state.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    None,
    Quit,
    /// Read again now.
    Read,
    Pause,
    Run(Job),
    /// Read from another address from now on (a hub's other connection).
    Switch(String),
    /// Say why a key did nothing, on the status bar.
    Say(String),
}

/// What the keys see of the shared state: the last answers and the key.
pub struct Seen<'a> {
    pub data: &'a Data,
    pub who: &'a Who,
    pub base: &'a str,
    pub hub: Option<&'a Result<Json, String>>,
    pub plan_running: bool,
}

/// One entry of the `:ctx` list: a label and the address it opens.
#[derive(Clone, Debug, PartialEq)]
pub struct CtxItem {
    pub name: String,
    pub detail: String,
    pub base: String,
}

/// The hub's own server, then each connection of its book.
pub fn ctx_items(base: &str, hub: Option<&Result<Json, String>>) -> Vec<CtxItem> {
    let (root, _) = hub_of(base);
    let mut out = vec![CtxItem { name: "(the hub itself)".into(), detail: model::host_of(&root), base: root.clone() }];
    if let Some(Ok(h)) = hub {
        for c in h["connections"].as_array().into_iter().flatten() {
            let Some(name) = c["name"].as_str() else { continue };
            let env = c["environment"].as_str().unwrap_or("no environment");
            let scope = c["scope"].as_str().unwrap_or("?");
            let url = c["url"].as_str().unwrap_or(if c["kube"].is_null() { "" } else { "through its Kubernetes API" });
            out.push(CtxItem { name: name.to_string(), detail: format!("{env} · key {scope} · {url}"), base: format!("{root}/c/{name}") });
        }
    }
    out
}

/// The terminal's state.
#[derive(Debug)]
pub struct App {
    pub screen: Screen,
    pub lens: Lens,
    /// The map groups copies by node rather than by shard.
    pub by_node: bool,
    pub shard: usize,
    pub copy: usize,
    pub alert: usize,
    pub query: usize,
    pub sort: QSort,
    /// Lines up from the newest; 0 follows new lines.
    pub log_back: usize,
    pub level: usize,
    pub log_q: String,
    pub table: usize,
    pub event: usize,
    pub sql: Input,
    pub sql_focus: bool,
    pub history: Vec<String>,
    pub history_at: Option<usize>,
    pub sql_row: usize,
    /// Until when production's SQL takes writes, and why.
    pub writes: Option<(Instant, String)>,
    pub overlay: Option<Overlay>,
    /// `g` was pressed: the next letter picks a screen.
    pub g: bool,
}

impl Default for App {
    fn default() -> App {
        App {
            screen: Screen::Home,
            lens: Lens::Health,
            by_node: false,
            shard: 0,
            copy: 0,
            alert: 0,
            query: 0,
            sort: QSort::Time,
            log_back: 0,
            level: 0,
            log_q: String::new(),
            table: 0,
            event: 0,
            sql: Input::default(),
            sql_focus: true,
            history: Vec::new(),
            history_at: None,
            sql_row: 0,
            writes: None,
            overlay: None,
            g: false,
        }
    }
}

/// How long writes stay allowed after `W`.
pub const WRITES_FOR: Duration = Duration::from_secs(15 * 60);

fn down(sel: &mut usize, n: usize) {
    if n > 0 {
        *sel = (*sel + 1).min(n - 1);
    }
}

fn up(sel: &mut usize) {
    *sel = sel.saturating_sub(1);
}

impl App {
    pub fn log_level(&self) -> Option<&'static str> {
        LEVELS[self.level % LEVELS.len()]
    }

    pub fn writes_allowed(&self) -> bool {
        self.writes.as_ref().is_some_and(|(until, _)| Instant::now() < *until)
    }

    /// Whether production's rules apply: a reason with each change, SQL read-only until `W`.
    pub fn production(&self, seen: &Seen) -> bool {
        model::is_production(seen.who.environment.as_deref(), seen.base)
    }

    fn reason_field(&self, seen: &Seen, prefill: &str) -> Field {
        let need = self.production(seen) || seen.who.reason_required;
        Field {
            label: if need { "Why? Kept in the audit log with the change".into() } else { "Why? (optional) Kept in the audit log".into() },
            input: Input::of(prefill),
            must: if need { Must::Reason } else { Must::AnyReason },
        }
    }

    pub fn go(&mut self, s: Screen) {
        self.screen = s;
        self.g = false;
        if s == Screen::Sql {
            self.sql_focus = true;
        }
    }

    /// The statements of the Queries screen, in the order it lists them.
    pub fn statements<'a>(&self, stats: &'a Json) -> Vec<&'a Json> {
        let mut v: Vec<&Json> = stats["statements"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
        let key = |s: &Json| -> f64 {
            match self.sort {
                QSort::Time => s["share"].as_f64().unwrap_or(0.0),
                QSort::P99 => s["p99_ms"].as_f64().unwrap_or(0.0),
                QSort::Calls => s["calls"].as_f64().unwrap_or(0.0),
                QSort::Errors => s["errors"].as_f64().unwrap_or(0.0),
            }
        };
        v.sort_by(|a, b| key(b).total_cmp(&key(a)));
        v
    }

    /// The alerts worth a look: firing, pending and silenced first, worst first.
    pub fn alerts<'a>(&self, alerts: &'a Json) -> Vec<&'a Json> {
        let rank = |a: &Json| match a["state"].as_str() {
            Some("firing") => 0,
            Some("pending") => 1,
            Some("no_data") => 3,
            Some("disabled") => 5,
            _ if !a["silenced"].is_null() => 2,
            _ => 4,
        };
        let sev = |a: &Json| match a["severity"].as_str() {
            Some("critical") => 0,
            Some("warning") => 1,
            _ => 2,
        };
        let mut v: Vec<&Json> = alerts["states"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
        v.sort_by_key(|a| (rank(a), sev(a)));
        v
    }

    /// The key, for the screen on show; the dialog or line typed into first.
    pub fn key(&mut self, k: KeyEvent, seen: &Seen) -> Effect {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && k.code == KeyCode::Char('c') {
            return Effect::Quit;
        }
        if let Some(o) = self.overlay.take() {
            return self.overlay_key(o, k, seen);
        }
        if self.screen == Screen::Sql && self.sql_focus {
            return self.sql_key(k, seen);
        }
        if self.g {
            self.g = false;
            if let KeyCode::Char(c) = k.code {
                if let Some(s) = SCREENS.iter().find(|s| s.2 == c) {
                    self.go(s.0);
                    return Effect::Read;
                }
            }
            return Effect::None;
        }
        match k.code {
            KeyCode::Char('q') => return Effect::Quit,
            KeyCode::Char('g') => {
                self.g = true;
                return Effect::None;
            }
            KeyCode::Char(c @ '1'..='8') => {
                self.go(SCREENS[c as usize - '1' as usize].0);
                return Effect::Read;
            }
            KeyCode::Tab => {
                self.go(SCREENS[(self.screen.index() + 1) % SCREENS.len()].0);
                return Effect::Read;
            }
            KeyCode::BackTab => {
                self.go(SCREENS[(self.screen.index() + SCREENS.len() - 1) % SCREENS.len()].0);
                return Effect::Read;
            }
            KeyCode::Char('?') => {
                self.overlay = Some(Overlay::Help);
                return Effect::None;
            }
            KeyCode::Char(':') => {
                self.overlay = Some(Overlay::Command(Input::default()));
                return Effect::None;
            }
            KeyCode::Char(' ') => return Effect::Pause,
            KeyCode::Char('r') if self.screen != Screen::Sql => return Effect::Read,
            KeyCode::Char('S') if seen.plan_running => {
                self.overlay = Some(Overlay::Dialog(Dialog {
                    title: "Stop the plan that runs".into(),
                    lines: vec!["The coordinator ends the step it is on, then stops: the steps after it do not run.".into(), "Resume or discard it from the web's Cluster view.".into()],
                    fields: vec![self.reason_field(seen, "")],
                    focus: 0,
                    ok: "Stop it".into(),
                    danger: false,
                    then: Then::Act(Act::StopPlan),
                }));
                return Effect::None;
            }
            _ => {}
        }
        match self.screen {
            Screen::Home => {
                if k.code == KeyCode::Enter {
                    self.go(Screen::Cluster);
                    return Effect::Read;
                }
                Effect::None
            }
            Screen::Cluster => self.cluster_key(k, seen),
            Screen::Alerts => self.alerts_key(k, seen),
            Screen::Queries => self.queries_key(k, seen),
            Screen::Logs => self.logs_key(k),
            Screen::Tables => {
                let n = seen.data.tables["tables"].as_array().map_or(0, Vec::len);
                match k.code {
                    KeyCode::Down => down(&mut self.table, n),
                    KeyCode::Up => up(&mut self.table),
                    _ => {}
                }
                Effect::None
            }
            Screen::Timeline => {
                let n = seen.data.events["events"].as_array().map_or(0, Vec::len);
                match k.code {
                    KeyCode::Down => down(&mut self.event, n),
                    KeyCode::Up => up(&mut self.event),
                    KeyCode::PageDown => self.event = (self.event + 10).min(n.saturating_sub(1)),
                    KeyCode::PageUp => self.event = self.event.saturating_sub(10),
                    _ => {}
                }
                Effect::None
            }
            Screen::Sql => match k.code {
                KeyCode::Char('i') | KeyCode::Enter => {
                    self.sql_focus = true;
                    Effect::None
                }
                KeyCode::Char('W') => self.allow_writes(seen),
                KeyCode::Down | KeyCode::PageDown => {
                    self.sql_row += if k.code == KeyCode::PageDown { 10 } else { 1 };
                    Effect::None
                }
                KeyCode::Up | KeyCode::PageUp => {
                    self.sql_row = self.sql_row.saturating_sub(if k.code == KeyCode::PageUp { 10 } else { 1 });
                    Effect::None
                }
                _ => Effect::None,
            },
        }
    }

    fn cluster_key(&mut self, k: KeyEvent, seen: &Seen) -> Effect {
        let shards = model::shards(&seen.data.cluster);
        let n = shards.len();
        match k.code {
            KeyCode::Right => {
                if n > 0 {
                    self.shard = (self.shard + 1).min(n - 1);
                }
                self.copy = 0;
            }
            KeyCode::Left => {
                self.shard = self.shard.saturating_sub(1);
                self.copy = 0;
            }
            KeyCode::Down => down(&mut self.copy, shards.get(self.shard).map_or(0, |s| s.n())),
            KeyCode::Up => up(&mut self.copy),
            KeyCode::Char('L') => self.lens = self.lens.next(),
            KeyCode::Char('G') => self.by_node = !self.by_node,
            KeyCode::Char('p') => {
                let Some(sh) = shards.get(self.shard) else { return Effect::None };
                let Some(c) = sh.copies.get(self.copy) else { return Effect::None };
                if let Some(why) = model::why_not_primary(sh, c) {
                    return self.refuse(why);
                }
                if !seen.who.can("admin") {
                    return self.refuse(format!("a handover takes an admin key; {} is {}", seen.who.name, seen.who.scope));
                }
                let now = sh.primary().map(|p| p.id.clone());
                let lines = vec![
                    match &now {
                        Some(p) => format!("{} asks the shard's copies for their votes; {p}, the primary now, grants its vote, steps down and follows it.", c.id),
                        None => format!("{} asks the shard's copies for their votes: it needs {} of {}, and {} answer.", c.id, sh.n() / 2 + 1, sh.n(), sh.up),
                    },
                    "Writes to the shard pause for a moment; reads go on. It is refused while another copy holds a later transaction.".into(),
                ];
                self.overlay = Some(Overlay::Dialog(Dialog {
                    title: format!("Make {} shard {}'s primary", c.id, sh.i),
                    lines,
                    fields: vec![self.reason_field(seen, "")],
                    focus: 0,
                    ok: "Hand it over".into(),
                    danger: false,
                    then: Then::Act(Act::Promote { shard: sh.i, url: c.url.clone(), id: c.id.clone() }),
                }));
            }
            _ => {}
        }
        Effect::None
    }

    fn alerts_key(&mut self, k: KeyEvent, seen: &Seen) -> Effect {
        let list = self.alerts(&seen.data.alerts);
        match k.code {
            KeyCode::Down => down(&mut self.alert, list.len()),
            KeyCode::Up => up(&mut self.alert),
            KeyCode::Char(c @ ('a' | 's' | 'u')) => {
                let Some(a) = list.get(self.alert) else { return Effect::None };
                let rule = a["rule"].as_str().unwrap_or_default().to_string();
                let name = a["name"].as_str().unwrap_or(&rule).to_string();
                match c {
                    'a' if a["state"] != "firing" => return self.refuse(format!("{name} is not firing: there is nothing to acknowledge")),
                    'a' => return Effect::Run(Job { act: Act::Ack { rule, name }, reason: None }),
                    'u' if a["silenced"].is_null() => return self.refuse(format!("{name} is not silenced")),
                    'u' => return Effect::Run(Job { act: Act::Unsilence { rule, name }, reason: None }),
                    _ => {
                        self.overlay = Some(Overlay::Dialog(Dialog {
                            title: format!("Silence {name}"),
                            lines: vec!["It still fires and shows here, but pages no one until the silence ends.".into()],
                            fields: vec![Field { label: "For how many minutes".into(), input: Input::of("60"), must: Must::Minutes }, Field { label: "Why? Shown with the silence".into(), input: Input::default(), must: Must::Reason }],
                            focus: 1,
                            ok: "Silence it".into(),
                            danger: false,
                            then: Then::Silence { rule, name },
                        }));
                    }
                }
            }
            _ => {}
        }
        Effect::None
    }

    fn queries_key(&mut self, k: KeyEvent, seen: &Seen) -> Effect {
        let live = seen.data.live["queries"].as_array().map_or(&[][..], Vec::as_slice);
        let n = live.len() + seen.data.stats["statements"].as_array().map_or(0, Vec::len);
        match k.code {
            KeyCode::Down => down(&mut self.query, n),
            KeyCode::Up => up(&mut self.query),
            KeyCode::Char('o') => self.sort = self.sort.next(),
            KeyCode::Char('x') => {
                let Some(q) = live.get(self.query) else { return self.refuse("x cancels a query that runs now: pick one under Running now") };
                let id = match &q["id"] {
                    Json::String(s) => s.clone(),
                    j => j.to_string(),
                };
                let sql: String = q["sql"].as_str().unwrap_or_default().chars().take(200).collect();
                self.overlay = Some(Overlay::Dialog(Dialog {
                    title: format!("Cancel query {id}"),
                    lines: vec![sql, "Its client gets an error; nothing it wrote is undone.".into()],
                    fields: vec![self.reason_field(seen, "")],
                    focus: 0,
                    ok: "Cancel it".into(),
                    danger: false,
                    then: Then::Act(Act::Cancel { id }),
                }));
            }
            _ => {}
        }
        Effect::None
    }

    fn logs_key(&mut self, k: KeyEvent) -> Effect {
        match k.code {
            KeyCode::Up => self.log_back += 1,
            KeyCode::Down => self.log_back = self.log_back.saturating_sub(1),
            KeyCode::PageUp => self.log_back += 20,
            KeyCode::PageDown => self.log_back = self.log_back.saturating_sub(20),
            KeyCode::End => self.log_back = 0,
            KeyCode::Char('f') => {
                self.level = (self.level + 1) % LEVELS.len();
                self.log_back = 0;
                return Effect::Read;
            }
            KeyCode::Char('/') => self.overlay = Some(Overlay::Filter(Input::of(&self.log_q))),
            _ => {}
        }
        Effect::None
    }

    fn sql_key(&mut self, k: KeyEvent, seen: &Seen) -> Effect {
        match k.code {
            KeyCode::Esc => self.sql_focus = false,
            KeyCode::Enter => return self.run_sql(seen),
            KeyCode::Up if !self.history.is_empty() => {
                let at = self.history_at.map_or(self.history.len() - 1, |a| a.saturating_sub(1));
                self.history_at = Some(at);
                self.sql = Input::of(&self.history[at]);
            }
            KeyCode::Down if self.history_at.is_some() => {
                let at = self.history_at.unwrap_or(0) + 1;
                if at >= self.history.len() {
                    self.history_at = None;
                    self.sql = Input::default();
                } else {
                    self.history_at = Some(at);
                    self.sql = Input::of(&self.history[at]);
                }
            }
            KeyCode::PageDown => self.sql_row += 10,
            KeyCode::PageUp => self.sql_row = self.sql_row.saturating_sub(10),
            _ => {
                self.sql.key(&k);
            }
        }
        Effect::None
    }

    fn allow_writes(&mut self, seen: &Seen) -> Effect {
        if !seen.who.can("write") {
            return self.refuse(format!("{} is a {} key: it writes nothing anywhere", seen.who.name, seen.who.scope));
        }
        self.overlay = Some(Overlay::Dialog(Dialog {
            title: "Allow writes for 15 minutes".into(),
            lines: vec![
                format!("Statements that change rows or tables run on {} until {}.", seen.who.cluster.as_deref().unwrap_or(seen.base), model::clock(model::now_ms() + WRITES_FOR.as_millis() as u64)),
                "Each still asks before it runs; the reason goes with each of them to the audit log.".into(),
            ],
            fields: vec![Field { label: "Why? Kept in the audit log with each write".into(), input: Input::default(), must: Must::Reason }],
            focus: 0,
            ok: "Allow writes".into(),
            danger: false,
            then: Then::AllowWrites,
        }));
        Effect::None
    }

    fn run_sql(&mut self, seen: &Seen) -> Effect {
        let sql = self.sql.text.trim().to_string();
        if sql.is_empty() {
            return Effect::None;
        }
        if self.history.last() != Some(&sql) {
            self.history.push(sql.clone());
        }
        self.history_at = None;
        self.sql_row = 0;
        let kind = Statement::of(&sql);
        if !kind.write {
            return Effect::Run(Job { act: Act::Sql { sql }, reason: None });
        }
        if !seen.who.can("write") {
            return self.refuse(format!("{} changes rows or tables, and {} is a {} key", kind.verb, seen.who.name, seen.who.scope));
        }
        let prod = self.production(seen);
        if prod && !self.writes_allowed() {
            self.writes = None;
            return self.refuse(format!("{} is production, read-only here: press Esc, then W to allow writes for 15 minutes, with a reason", seen.who.cluster.as_deref().unwrap_or("this cluster")));
        }
        let mut fields = Vec::new();
        if let Some(t) = &kind.typed {
            fields.push(Field { label: format!("Type {t} to run it"), input: Input::default(), must: Must::Equal(t.clone()) });
        }
        let given = self.writes.as_ref().map(|(_, r)| r.clone()).unwrap_or_default();
        fields.push(self.reason_field(seen, &given));
        let mut lines = vec![sql.chars().take(400).collect::<String>()];
        lines.push(match (kind.verb.as_str(), &kind.typed) {
            ("DROP", Some(t)) => format!("It drops {t} and every row it holds. There is no undo."),
            ("TRUNCATE", Some(t)) => format!("It empties {t}. There is no undo."),
            ("DELETE", Some(t)) => format!("It has no WHERE: it deletes every row of {t}. There is no undo."),
            (_, Some(t)) => format!("It has no WHERE: it changes every row of {t}. There is no undo."),
            _ => "It changes rows or tables, and has no undo.".into(),
        });
        self.overlay = Some(Overlay::Dialog(Dialog {
            title: format!("Run {} on {}", kind.verb, seen.who.cluster.as_deref().unwrap_or(seen.base)),
            lines,
            focus: 0,
            fields,
            ok: format!("Run {}", kind.verb),
            danger: kind.typed.is_some(),
            then: Then::Act(Act::Sql { sql }),
        }));
        Effect::None
    }

    fn refuse(&mut self, why: impl Into<String>) -> Effect {
        Effect::Say(why.into())
    }

    fn overlay_key(&mut self, o: Overlay, k: KeyEvent, seen: &Seen) -> Effect {
        match o {
            Overlay::Help => Effect::None,
            Overlay::Dialog(mut d) => match k.code {
                KeyCode::Esc => Effect::None,
                KeyCode::Tab | KeyCode::Down if !d.fields.is_empty() => {
                    d.focus = (d.focus + 1) % d.fields.len();
                    self.overlay = Some(Overlay::Dialog(d));
                    Effect::None
                }
                KeyCode::BackTab | KeyCode::Up if !d.fields.is_empty() => {
                    d.focus = (d.focus + d.fields.len() - 1) % d.fields.len();
                    self.overlay = Some(Overlay::Dialog(d));
                    Effect::None
                }
                KeyCode::Enter if d.ready() => self.dialog_done(d),
                KeyCode::Enter => {
                    // Enter on a field that is filled goes to the next one that is not.
                    if let Some(i) = d.fields.iter().position(|f| !f.ok()) {
                        d.focus = i;
                    }
                    self.overlay = Some(Overlay::Dialog(d));
                    Effect::None
                }
                _ => {
                    if let Some(f) = d.fields.get_mut(d.focus) {
                        f.input.key(&k);
                    }
                    self.overlay = Some(Overlay::Dialog(d));
                    Effect::None
                }
            },
            Overlay::Command(mut i) => match k.code {
                KeyCode::Esc => Effect::None,
                KeyCode::Enter => self.command(i.text.trim(), seen),
                _ => {
                    i.key(&k);
                    self.overlay = Some(Overlay::Command(i));
                    Effect::None
                }
            },
            Overlay::Filter(mut i) => match k.code {
                KeyCode::Esc => Effect::None,
                KeyCode::Enter => {
                    self.log_q = i.text.trim().to_string();
                    self.log_back = 0;
                    Effect::Read
                }
                _ => {
                    i.key(&k);
                    self.overlay = Some(Overlay::Filter(i));
                    Effect::None
                }
            },
            Overlay::Ctx { mut at } => {
                let items = ctx_items(seen.base, seen.hub);
                match k.code {
                    KeyCode::Esc => Effect::None,
                    KeyCode::Down => {
                        down(&mut at, items.len());
                        self.overlay = Some(Overlay::Ctx { at });
                        Effect::None
                    }
                    KeyCode::Up => {
                        up(&mut at);
                        self.overlay = Some(Overlay::Ctx { at });
                        Effect::None
                    }
                    KeyCode::Enter => match items.get(at) {
                        Some(it) if it.base != seen.base => self.switch(it.base.clone()),
                        _ => Effect::None,
                    },
                    _ => {
                        self.overlay = Some(Overlay::Ctx { at });
                        Effect::None
                    }
                }
            }
        }
    }

    fn switch(&mut self, base: String) -> Effect {
        self.shard = 0;
        self.copy = 0;
        self.alert = 0;
        self.query = 0;
        self.table = 0;
        self.event = 0;
        self.log_back = 0;
        self.writes = None;
        Effect::Switch(base)
    }

    fn dialog_done(&mut self, d: Dialog) -> Effect {
        let reason = d.reason();
        match d.then {
            Then::Act(act) => Effect::Run(Job { act, reason }),
            Then::Silence { rule, name } => {
                let minutes = d.fields.iter().find(|f| f.must == Must::Minutes).and_then(|f| f.input.text.trim().parse().ok()).unwrap_or(60);
                Effect::Run(Job { act: Act::Silence { rule, name, minutes }, reason })
            }
            Then::AllowWrites => {
                self.writes = Some((Instant::now() + WRITES_FOR, reason.unwrap_or_default()));
                self.sql_focus = true;
                Effect::None
            }
        }
    }

    /// A `:` command: a screen's name, `ctx [name]`, `lens NAME`, `writes`, `q`.
    fn command(&mut self, line: &str, seen: &Seen) -> Effect {
        let (word, rest) = line.split_once(' ').map_or((line, ""), |(a, b)| (a, b.trim()));
        match word {
            "" => Effect::None,
            "q" | "quit" | "exit" => Effect::Quit,
            "ctx" | "context" | "connections" => {
                let (root, _) = hub_of(seen.base);
                if rest.is_empty() {
                    self.overlay = Some(Overlay::Ctx { at: 0 });
                    return Effect::Run(Job { act: Act::Hub { root }, reason: None });
                }
                if rest == "-" || rest == "hub" {
                    return if root == seen.base { Effect::None } else { self.switch(root) };
                }
                let base = format!("{root}/c/{rest}");
                if base == seen.base {
                    Effect::None
                } else {
                    self.switch(base)
                }
            }
            "lens" => match Lens::parse(rest) {
                Some(l) => {
                    self.lens = l;
                    self.go(Screen::Cluster);
                    Effect::Read
                }
                None => self.refuse(format!("a lens is one of {}", model::LENSES.iter().map(|l| l.name()).collect::<Vec<_>>().join(", "))),
            },
            "writes" => {
                self.go(Screen::Sql);
                self.sql_focus = false;
                self.allow_writes(seen)
            }
            "sql" if !rest.is_empty() => {
                self.go(Screen::Sql);
                self.sql = Input::of(rest);
                self.run_sql(seen)
            }
            w => match Screen::parse(w) {
                Some(s) => {
                    self.go(s);
                    Effect::Read
                }
                None => self.refuse(format!(":{w} is no command: ? lists them")),
            },
        }
    }
}

/// What a statement is, as the web's SQL guard reads it (`sqlguard.js`):
/// its verb, whether it changes anything, and the name to type before it
/// runs (an UPDATE or DELETE with no WHERE, a DROP).
#[derive(Clone, Debug, PartialEq)]
pub struct Statement {
    pub verb: String,
    pub write: bool,
    pub typed: Option<String>,
}

const READS: [&str; 8] = ["SELECT", "WITH", "VALUES", "SHOW", "DESCRIBE", "DESC", "EXPLAIN", "TABLE"];

impl Statement {
    pub fn of(sql: &str) -> Statement {
        let words = words(sql);
        let up: Vec<String> = words.iter().map(|w| w.to_ascii_uppercase()).collect();
        let verb = up.first().cloned().unwrap_or_default();
        let write = !verb.is_empty() && !READS.contains(&verb.as_str());
        let after = |i: usize| words.get(i).cloned();
        let typed = match verb.as_str() {
            "UPDATE" if !up.contains(&"WHERE".to_string()) => after(1),
            "DELETE" if !up.contains(&"WHERE".to_string()) => {
                let from = up.iter().position(|w| w == "FROM").map_or(1, |p| p + 1);
                after(from)
            }
            "DROP" => {
                let mut i = 2;
                if up.get(2).map(String::as_str) == Some("IF") && up.get(3).map(String::as_str) == Some("EXISTS") {
                    i = 4;
                }
                after(i).map(|t| if up.get(1).map(String::as_str) == Some("TABLE") { t } else { format!("{} {t}", words.get(1).cloned().unwrap_or_default().to_ascii_lowercase()) })
            }
            "TRUNCATE" => after(if up.get(1).map(String::as_str) == Some("TABLE") { 2 } else { 1 }),
            _ => None,
        };
        Statement { verb: if verb.is_empty() { "SQL".into() } else { verb }, write, typed }
    }
}

/// The words of a statement outside its strings and comments, quotes off
/// names, with a `(` before the first skipped.
fn words(sql: &str) -> Vec<String> {
    let cs: Vec<char> = sql.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c == '-' && cs.get(i + 1) == Some(&'-') {
            while i < cs.len() && cs[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && cs.get(i + 1) == Some(&'*') {
            i += 2;
            while i < cs.len() && !(cs[i] == '*' && cs.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i += 2;
        } else if c == '\'' {
            i += 1;
            while i < cs.len() {
                if cs[i] == '\'' && cs.get(i + 1) == Some(&'\'') {
                    i += 2;
                } else if cs[i] == '\'' {
                    break;
                } else {
                    i += 1;
                }
            }
            i += 1;
        } else if c == '"' || c == '`' {
            let q = c;
            let start = i + 1;
            i += 1;
            while i < cs.len() && cs[i] != q {
                i += 1;
            }
            out.push(cs[start..i.min(cs.len())].iter().collect());
            i += 1;
        } else if c.is_alphanumeric() || c == '_' {
            let start = i;
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_' || cs[i] == '.' || cs[i] == '$') {
                i += 1;
            }
            out.push(cs[start..i].iter().collect());
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    fn ch(c: char) -> KeyEvent {
        key(KeyCode::Char(c))
    }

    fn data() -> Data {
        Data {
            cluster: json!({"shards": [{"shard": 0, "state": "ok", "primary": "http://a", "replicas": [
                {"url": "http://a", "short": "s0-0", "takes_writes": true, "term": 1, "txn": 5, "status": "ok"},
                {"url": "http://b", "short": "s0-1", "takes_writes": false, "term": 1, "txn": 5, "status": "ok"}]}]}),
            alerts: json!({"states": [{"rule": "disk", "name": "Disk nearly full", "state": "firing", "severity": "warning"}, {"rule": "ok1", "name": "Fine", "state": "ok"}]}),
            ..Data::default()
        }
    }

    fn seen<'a>(d: &'a Data, who: &'a Who, base: &'a str) -> Seen<'a> {
        Seen { data: d, who, base, hub: None, plan_running: false }
    }

    fn admin(env: Option<&str>) -> Who {
        Who { name: "ops-dana".into(), scope: "admin".into(), environment: env.map(String::from), cluster: Some("orders-prod".into()), ..Who::default() }
    }

    fn typ(app: &mut App, s: &str, seen: &Seen) {
        for c in s.chars() {
            app.key(ch(c), seen);
        }
    }

    #[test]
    fn number_keys_and_g_letters_pick_screens_as_the_web_does() {
        let d = Data::default();
        let w = admin(None);
        let s = seen(&d, &w, "http://127.0.0.1:7700");
        let mut app = App::default();
        assert_eq!(app.key(ch('2'), &s), Effect::Read);
        assert_eq!(app.screen, Screen::Cluster);
        app.key(ch('g'), &s);
        app.key(ch('l'), &s);
        assert_eq!(app.screen, Screen::Logs);
        app.key(key(KeyCode::Tab), &s);
        assert_eq!(app.screen, Screen::Tables);
        assert_eq!(app.key(ch('q'), &s), Effect::Quit);
    }

    #[test]
    fn a_handover_asks_for_a_reason_on_production_then_runs() {
        let d = data();
        let w = admin(Some("production"));
        let s = seen(&d, &w, "https://db.example.com");
        let mut app = App { screen: Screen::Cluster, ..App::default() };
        // The primary cannot be made the primary: the status bar says so.
        assert!(matches!(app.key(ch('p'), &s), Effect::Say(r) if r.contains("primary already")));
        app.key(key(KeyCode::Down), &s);
        assert_eq!(app.key(ch('p'), &s), Effect::None);
        let Some(Overlay::Dialog(dlg)) = &app.overlay else { panic!("no dialog") };
        assert_eq!(dlg.title, "Make s0-1 shard 0's primary");
        assert!(!dlg.ready());
        // Enter does nothing until the reason is there.
        assert_eq!(app.key(key(KeyCode::Enter), &s), Effect::None);
        typ(&mut app, "restart s0-0", &s);
        let e = app.key(key(KeyCode::Enter), &s);
        assert_eq!(e, Effect::Run(Job { act: Act::Promote { shard: 0, url: "http://b".into(), id: "s0-1".into() }, reason: Some("restart s0-0".into()) }));
        assert!(app.overlay.is_none());
    }

    #[test]
    fn a_read_key_is_told_it_cannot_hand_over() {
        let d = data();
        let w = Who { name: "viewer".into(), scope: "read".into(), ..Who::default() };
        let s = seen(&d, &w, "http://127.0.0.1:7700");
        let mut app = App { screen: Screen::Cluster, copy: 1, ..App::default() };
        assert!(matches!(app.key(ch('p'), &s), Effect::Say(r) if r.contains("admin key")));
        assert!(app.overlay.is_none());
    }

    #[test]
    fn production_sql_is_read_only_until_writes_are_allowed() {
        let d = data();
        let w = admin(Some("production"));
        let s = seen(&d, &w, "https://db.example.com");
        let mut app = App::default();
        app.key(ch('7'), &s);
        assert!(app.sql_focus);
        typ(&mut app, "SELECT 1", &s);
        assert_eq!(app.key(key(KeyCode::Enter), &s), Effect::Run(Job { act: Act::Sql { sql: "SELECT 1".into() }, reason: None }));
        app.sql = Input::of("DELETE FROM orders");
        assert!(matches!(app.key(key(KeyCode::Enter), &s), Effect::Say(r) if r.contains("read-only")));
        // W, with a reason, allows writes; the write then wants the table's name.
        app.key(key(KeyCode::Esc), &s);
        app.key(ch('W'), &s);
        typ(&mut app, "backfill fix", &s);
        app.key(key(KeyCode::Enter), &s);
        assert!(app.writes_allowed());
        app.sql = Input::of("DELETE FROM orders");
        assert_eq!(app.key(key(KeyCode::Enter), &s), Effect::None);
        let Some(Overlay::Dialog(dlg)) = &app.overlay else { panic!("no dialog") };
        assert!(dlg.danger && dlg.fields[0].must == Must::Equal("orders".into()));
        assert_eq!(dlg.fields[1].input.text, "backfill fix");
        typ(&mut app, "order", &s);
        assert_eq!(app.key(key(KeyCode::Enter), &s), Effect::None, "orders is not typed yet");
        typ(&mut app, "s", &s);
        assert_eq!(app.key(key(KeyCode::Enter), &s), Effect::Run(Job { act: Act::Sql { sql: "DELETE FROM orders".into() }, reason: Some("backfill fix".into()) }));
    }

    #[test]
    fn a_sandbox_writes_without_being_asked_to_allow_it() {
        let d = data();
        let w = admin(None);
        let s = seen(&d, &w, "http://127.0.0.1:7700");
        let mut app = App { screen: Screen::Sql, ..App::default() };
        app.sql = Input::of("INSERT INTO t (id) VALUES ('a')");
        assert_eq!(app.key(key(KeyCode::Enter), &s), Effect::None);
        let Some(Overlay::Dialog(dlg)) = &app.overlay else { panic!("no dialog") };
        assert!(dlg.ready(), "a reason is optional off production: {dlg:?}");
    }

    #[test]
    fn statements_are_read_as_the_web_reads_them() {
        assert_eq!(Statement::of("  -- note\n select * from t").write, false);
        assert_eq!(Statement::of("(SELECT 1)").verb, "SELECT");
        assert_eq!(Statement::of("UPDATE t SET a = 1 WHERE id = 'x'").typed, None);
        assert_eq!(Statement::of("UPDATE \"Orders\" SET a = 'WHERE'").typed.as_deref(), Some("Orders"));
        assert_eq!(Statement::of("DELETE FROM logs").typed.as_deref(), Some("logs"));
        assert_eq!(Statement::of("DROP TABLE IF EXISTS old").typed.as_deref(), Some("old"));
        assert!(Statement::of("CREATE TABLE x (id KEYWORD)").write);
    }

    #[test]
    fn alerts_are_acked_and_silenced_with_a_reason() {
        let d = data();
        let w = admin(None);
        let s = seen(&d, &w, "http://127.0.0.1:7700");
        let mut app = App { screen: Screen::Alerts, ..App::default() };
        assert_eq!(app.key(ch('a'), &s), Effect::Run(Job { act: Act::Ack { rule: "disk".into(), name: "Disk nearly full".into() }, reason: None }));
        app.key(ch('s'), &s);
        typ(&mut app, "known, fixing", &s);
        let e = app.key(key(KeyCode::Enter), &s);
        assert_eq!(e, Effect::Run(Job { act: Act::Silence { rule: "disk".into(), name: "Disk nearly full".into(), minutes: 60 }, reason: Some("known, fixing".into()) }));
    }

    #[test]
    fn ctx_lists_the_hubs_book_and_switches() {
        let d = Data::default();
        let w = admin(None);
        let hub = Ok(json!({"connections": [{"name": "prod", "url": "https://prod:7700", "environment": "production", "scope": "admin"}]}));
        let s = Seen { data: &d, who: &w, base: "http://127.0.0.1:7700", hub: Some(&hub), plan_running: false };
        let mut app = App::default();
        app.key(ch(':'), &s);
        typ(&mut app, "ctx", &s);
        assert_eq!(app.key(key(KeyCode::Enter), &s), Effect::Run(Job { act: Act::Hub { root: "http://127.0.0.1:7700".into() }, reason: None }));
        assert_eq!(app.overlay, Some(Overlay::Ctx { at: 0 }));
        app.key(key(KeyCode::Down), &s);
        assert_eq!(app.key(key(KeyCode::Enter), &s), Effect::Switch("http://127.0.0.1:7700/c/prod".into()));
        // From a connection, `:ctx -` goes back to the hub's own server.
        let s2 = Seen { base: "http://127.0.0.1:7700/c/prod", ..s };
        app.key(ch(':'), &s2);
        typ(&mut app, "ctx -", &s2);
        assert_eq!(app.key(key(KeyCode::Enter), &s2), Effect::Switch("http://127.0.0.1:7700".into()));
    }

    #[test]
    fn typing_edits_where_the_cursor_is() {
        let mut i = Input::of("SELEC 1");
        for _ in 0..2 {
            i.key(&key(KeyCode::Left));
        }
        i.key(&ch('T'));
        assert_eq!(i.text, "SELECT 1");
        i.key(&KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!((i.text.as_str(), i.at), ("", 0));
        i.paste("é\tx");
        assert_eq!(i.text, "éx");
    }
}
