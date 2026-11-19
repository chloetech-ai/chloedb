//! Drawing: each screen, the header, the bars and the dialogs, into a
//! ratatui buffer from the last answers. Nothing here changes state, so a
//! test draws a screen from fixed answers and reads the text back.

use std::sync::atomic::{AtomicBool, Ordering};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, BorderType, Widget};
use serde_json::Value as Json;

use super::api::{hub_of, Shared, Tone};
use super::app::{ctx_items, App, Dialog, Input, Must, Overlay, Screen, Seen, Statement, SCREENS};
use super::model::{self, Level, Lens, Shard, Slo};

/// `NO_COLOR` is set: styles are bold and reversed, never coloured.
static PLAIN: AtomicBool = AtomicBool::new(false);

pub fn plain(on: bool) {
    PLAIN.store(on, Ordering::Relaxed);
}

/// The smallest terminal the screens fit.
pub const MIN_W: u16 = 80;
pub const MIN_H: u16 = 24;

fn col(c: Color) -> Style {
    if PLAIN.load(Ordering::Relaxed) {
        Style::new()
    } else {
        Style::new().fg(c)
    }
}

fn on(fg: Color, bg: Color) -> Style {
    if PLAIN.load(Ordering::Relaxed) {
        Style::new().add_modifier(Modifier::REVERSED)
    } else {
        Style::new().fg(fg).bg(bg)
    }
}

const ACC: Color = Color::Indexed(75);
const WARN: Color = Color::Indexed(214);
const BAD: Color = Color::Indexed(203);
const OK: Color = Color::Indexed(114);
const DIM: Color = Color::Indexed(245);
const BAR: Color = Color::Indexed(236);
const INK: Color = Color::Indexed(252);
const BLACK: Color = Color::Indexed(16);

fn fg() -> Style {
    Style::new()
}
fn dim() -> Style {
    col(DIM)
}
fn acc() -> Style {
    col(ACC)
}
fn bold(s: Style) -> Style {
    s.add_modifier(Modifier::BOLD)
}
fn bar() -> Style {
    on(INK, BAR)
}
fn sel() -> Style {
    on(BLACK, Color::Indexed(153))
}
fn chip(c: Color) -> Style {
    bold(on(BLACK, c))
}
fn level(l: Level) -> Style {
    match l {
        Level::Ok => fg(),
        Level::Warn => col(WARN),
        Level::Bad => col(BAD),
    }
}
fn tone(t: Tone) -> Style {
    match t {
        Tone::Info => acc(),
        Tone::Good => col(OK),
        Tone::Bad => col(BAD),
    }
}

/// Text cut to `w` chars, with an ellipsis when cut.
pub fn fit(s: &str, w: usize) -> String {
    if s.chars().count() <= w {
        return s.to_string();
    }
    if w == 0 {
        return String::new();
    }
    let mut out: String = s.chars().take(w - 1).collect();
    out.push('…');
    out
}

/// Text padded or cut to `w` chars.
fn pad(s: &str, w: usize) -> String {
    let t = fit(s, w);
    let n = t.chars().count();
    t + &" ".repeat(w.saturating_sub(n))
}

fn rpad(s: &str, w: usize) -> String {
    let t = fit(s, w);
    " ".repeat(w.saturating_sub(t.chars().count())) + &t
}

/// Words wrapped into lines of `w` chars at most.
pub fn wrap(s: &str, w: usize) -> Vec<String> {
    let mut out = Vec::new();
    for para in s.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            let n = line.chars().count();
            if n > 0 && n + 1 + word.chars().count() > w {
                out.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
            while line.chars().count() > w {
                let head: String = line.chars().take(w).collect();
                line = line.chars().skip(w).collect();
                out.push(head);
            }
        }
        out.push(line);
    }
    out
}

/// A buffer drawn on in rows and columns, clipped to its area.
struct Canvas<'a> {
    buf: &'a mut Buffer,
    area: Rect,
}

impl Canvas<'_> {
    fn w(&self) -> u16 {
        self.area.width
    }
    fn h(&self) -> u16 {
        self.area.height
    }

    /// Writes `s` at column `x`, row `y`; the column after it.
    fn put(&mut self, x: u16, y: u16, s: &str, st: Style) -> u16 {
        if y >= self.h() || x >= self.w() {
            return x;
        }
        let (end, _) = self.buf.set_stringn(self.area.x + x, self.area.y + y, s, (self.w() - x) as usize, st);
        end - self.area.x
    }

    fn fill(&mut self, x: u16, y: u16, w: u16, st: Style) {
        if y < self.h() && x < self.w() {
            let w = w.min(self.w() - x);
            self.buf.set_style(Rect::new(self.area.x + x, self.area.y + y, w, 1), st);
            for i in 0..w {
                if let Some(c) = self.buf.cell_mut((self.area.x + x + i, self.area.y + y)) {
                    c.set_symbol(" ");
                }
            }
        }
    }

    fn boxed(&mut self, r: Rect, title: &str, st: Style) {
        let r = r.intersection(Rect::new(0, 0, self.w(), self.h()));
        if r.width < 2 || r.height < 2 {
            return;
        }
        let abs = Rect::new(self.area.x + r.x, self.area.y + r.y, r.width, r.height);
        let mut b = Block::bordered().border_type(BorderType::Rounded).border_style(st);
        if !title.is_empty() {
            b = b.title(format!(" {} ", fit(title, r.width.saturating_sub(4) as usize)));
        }
        b.render(abs, self.buf);
    }

    /// Clears a rectangle to `st`.
    fn clear(&mut self, r: Rect, st: Style) {
        for y in r.y..r.y + r.height {
            self.fill(r.x, y, r.width, st);
        }
    }

    /// An input with its cursor shown, in `w` columns, scrolled to keep the cursor in.
    fn input(&mut self, x: u16, y: u16, w: u16, i: &Input, focused: bool, st: Style) {
        let w = w as usize;
        let chars: Vec<char> = i.text.chars().collect();
        let start = (i.at + 1).saturating_sub(w);
        let shown: String = chars.iter().skip(start).take(w).collect();
        self.fill(x, y, w as u16, st);
        self.put(x, y, &shown, st);
        if focused {
            let cx = x + (i.at - start) as u16;
            let c: String = chars.get(i.at).map_or(" ".into(), |c| c.to_string());
            self.put(cx, y, &c, st.add_modifier(Modifier::REVERSED));
        }
    }
}

/// Draws the whole terminal.
pub fn draw(buf: &mut Buffer, area: Rect, app: &App, s: &Shared) {
    let mut cv = Canvas { buf, area };
    if area.width < MIN_W || area.height < MIN_H {
        let lines = wrap(&format!("Make the terminal at least {MIN_W} columns by {MIN_H} rows ({}×{} now).", area.width, area.height), area.width.saturating_sub(2).max(1) as usize);
        for (k, l) in lines.iter().enumerate() {
            cv.put(1, 1 + k as u16, l, bold(col(WARN)));
        }
        cv.put(1, 2 + lines.len() as u16, "q quits.", dim());
        return;
    }
    let seen = Seen { data: &s.data, who: &s.who, base: &s.base, hub: s.hub.as_ref(), plan_running: plan_running(&s.data.cluster) };
    header(&mut cv, app, s, &seen);
    let body = Rect::new(0, 3, cv.w(), cv.h() - 5);
    let hints = match app.screen {
        Screen::Home => home(&mut cv, body, s),
        Screen::Cluster => cluster(&mut cv, body, app, s, &seen),
        Screen::Alerts => alerts(&mut cv, body, app, s),
        Screen::Queries => queries(&mut cv, body, app, s),
        Screen::Logs => logs(&mut cv, body, app, s),
        Screen::Tables => tables(&mut cv, body, app, s),
        Screen::Sql => sql(&mut cv, body, app, s, &seen),
        Screen::Timeline => timeline(&mut cv, body, app, s),
    };
    let hints: Vec<(&str, &str)> = match &app.overlay {
        Some(Overlay::Dialog(_)) => vec![("Tab", "next field"), ("⏎", "do it"), ("Esc", "cancel")],
        Some(Overlay::Command(_) | Overlay::Filter(_)) => vec![("⏎", "go"), ("Esc", "cancel")],
        Some(Overlay::Ctx { .. }) => vec![("↑↓", "pick"), ("⏎", "open"), ("Esc", "cancel")],
        Some(Overlay::Help) => vec![("any key", "closes this")],
        None if app.screen == Screen::Sql && app.sql_focus => vec![("⏎", "run"), ("↑↓", "history"), ("PgUp PgDn", "rows"), ("Esc", "leave the editor")],
        None => hints,
    };
    footer(&mut cv, app, s, &hints);
    match &app.overlay {
        Some(Overlay::Dialog(d)) => dialog(&mut cv, d),
        Some(Overlay::Help) => help(&mut cv),
        Some(Overlay::Ctx { at }) => ctx(&mut cv, *at, &seen),
        _ => {}
    }
}

/// The screen as text, a line a row: what a test reads.
pub fn render(app: &App, s: &Shared, w: u16, h: u16) -> String {
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    draw(&mut buf, area, app, s);
    let mut out = String::new();
    for y in 0..h {
        for x in 0..w {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

pub fn plan_running(cluster: &Json) -> bool {
    matches!(cluster["plan"]["state"].as_str(), Some("running" | "stopping"))
}

fn header(cv: &mut Canvas, app: &App, s: &Shared, seen: &Seen) {
    let w = cv.w();
    cv.fill(0, 0, w, bar());
    let cl = &s.data.cluster;
    let (root, via) = hub_of(&s.base);
    let name = s.who.cluster.clone().or_else(|| cl["cluster"]["name"].as_str().map(String::from)).unwrap_or_else(|| model::host_of(&s.base));
    let env = s.who.environment.clone();
    let prod = model::is_production(env.as_deref(), &s.base);
    let env_label = env.map(|e| e.to_uppercase()).unwrap_or_else(|| if prod { "ENVIRONMENT NOT SET".into() } else { "SANDBOX".into() });
    let shards = model::shards(cl);
    let summary = if shards.is_empty() {
        if cl["role"].as_str() == Some("coordinator") { "no shards".to_string() } else { "one server".to_string() }
    } else {
        let n: usize = shards.iter().map(Shard::n).sum();
        format!("{} shards · {n} copies", shards.len())
    };
    let via_text = via.as_ref().map(|_| format!("via hub {}", model::host_of(&root)));
    let key = format!("{} · {}", s.who.name, s.who.scope);
    let live = if s.paused { "❚❚ paused" } else { "● live" };
    let clock = format!("{} {}", model::clock(model::now_ms()), model::zone());
    // What fits, the least needed dropped first: the hub, the clock's zone, the clock.
    let len = |t: &str| t.chars().count() as u16;
    let left = len("chloedb ▸ ") + len(&name) + len(&env_label) + 4 + len(&summary) + 2;
    let mut right = vec![key.clone(), live.to_string(), clock.clone()];
    let wants = |via: bool, right: &[String]| left + if via { via_text.as_deref().map_or(0, |v| len(v) + 2) } else { 0 } + right.iter().map(|r| len(r) + 3).sum::<u16>() + 2;
    let mut show_via = via_text.is_some();
    if wants(show_via, &right) > w {
        show_via = false;
    }
    if wants(show_via, &right) > w {
        right[2] = model::clock(model::now_ms());
    }
    if wants(show_via, &right) > w {
        right.pop();
    }
    let mut x = cv.put(1, 0, "chloedb", bold(on(ACC, BAR)));
    x = cv.put(x + 1, 0, &format!("▸ {name}"), bold(bar()));
    x = cv.put(x + 1, 0, &format!(" {env_label} "), if prod { chip(BAD) } else { chip(ACC) });
    x = cv.put(x + 2, 0, &summary, bar());
    if show_via {
        cv.put(x + 2, 0, via_text.as_deref().unwrap_or_default(), on(DIM, BAR));
    }
    let rtext = right.join("   ");
    let rx = w.saturating_sub(len(&rtext) + 1);
    if rx > x + 1 {
        let kx = cv.put(rx, 0, &key, if s.who.root { bold(on(BAD, BAR)) } else { bar() });
        cv.put(kx, 0, &rtext[key.len()..], bar());
    }
    // Tabs, with the alerts firing on Alerts'.
    let firing = s.data.fleet["checks"].as_array().and_then(|a| a.iter().find(|c| c["id"] == "alerts")).and_then(|c| c["firing"].as_u64()).unwrap_or(0);
    let mut x = 1;
    for (n, (sc, title, _)) in SCREENS.iter().enumerate() {
        let here = *sc == app.screen;
        x = cv.put(x, 1, &format!(" {} ", n + 1), if here { chip(ACC) } else { dim() });
        x = cv.put(x, 1, &format!(" {title}"), if here { bold(acc()) } else { fg() });
        if *sc == Screen::Alerts && firing > 0 {
            x = cv.put(x + 1, 1, &format!(" {firing} "), chip(BAD));
        }
        x += 2;
    }
    // The band: what holds for every screen now.
    let plan = &cl["plan"];
    if seen.plan_running {
        let steps = plan["steps"].as_array().map_or(0, Vec::len);
        let step = plan["steps"].as_array().and_then(|a| a.iter().find(|st| st["state"] == "running")).and_then(|st| st["text"].as_str()).unwrap_or_default().to_string();
        cv.fill(0, 2, w, chip(ACC));
        cv.put(1, 2, &fit(&format!("PLAN RUNNING on the coordinator · step {} of {steps}: {step} · it goes on if this terminal closes · S stops it", plan["done"].as_u64().unwrap_or(0) + 1), (w - 2) as usize), chip(ACC));
    } else if app.screen == Screen::Sql && app.writes_allowed() {
        let (until, why) = app.writes.clone().unwrap_or((std::time::Instant::now(), String::new()));
        let left = until.saturating_duration_since(std::time::Instant::now()).as_secs();
        cv.fill(0, 2, w, chip(WARN));
        cv.put(1, 2, &fit(&format!("WRITES ALLOWED on {name} for {} min more · {why}", left.div_ceil(60)), (w - 2) as usize), chip(WARN));
    } else if let Some(p) = plan["state"].as_str().filter(|st| matches!(*st, "failed" | "stopped" | "interrupted")) {
        cv.fill(0, 2, w, chip(WARN));
        cv.put(1, 2, &fit(&format!("The last plan {p}{}: resume or discard it from the web's Cluster view", plan["error"].as_str().map(|e| format!(" ({e})")).unwrap_or_default()), (w - 2) as usize), chip(WARN));
    }
}

fn footer(cv: &mut Canvas, app: &App, s: &Shared, hints: &[(&str, &str)]) {
    let (w, h) = (cv.w(), cv.h());
    match &app.overlay {
        Some(Overlay::Command(i)) => {
            cv.put(0, h - 2, ":", bold(acc()));
            cv.input(1, h - 2, w - 2, i, true, fg());
        }
        Some(Overlay::Filter(i)) => {
            let x = cv.put(1, h - 2, "Log lines with: ", bold(acc()));
            cv.input(x, h - 2, w - x - 1, i, true, fg());
        }
        _ => {
            let fresh = s.note.as_ref().filter(|n| n.at.elapsed().as_secs() < 10);
            let (text, st) = if let Some(n) = fresh {
                (n.text.clone(), tone(n.tone))
            } else if let Some(e) = s.errors.first() {
                (format!("! {e}{}", if s.errors.len() > 1 { format!(" (and {} more)", s.errors.len() - 1) } else { String::new() }), col(WARN))
            } else if let Some(at) = s.read_at {
                (format!("Read {} s ago from {}", at.elapsed().as_secs(), s.base), dim())
            } else {
                (format!("Reading {}…", s.base), dim())
            };
            cv.put(1, h - 2, &fit(&text, (w - 2) as usize), st);
        }
    }
    let mut x = 1;
    let mut all: Vec<(&str, &str)> = hints.to_vec();
    if app.overlay.is_none() {
        all.extend([("1-8", "screens"), (":", "command"), ("?", "keys"), ("q", "quit")]);
    }
    for (k, label) in all {
        let need = (k.chars().count() + label.chars().count() + 4) as u16;
        if x + need > w {
            break;
        }
        x = cv.put(x, h - 1, &format!(" {k} "), chip(ACC));
        x = cv.put(x, h - 1, &format!(" {label}  "), fg());
    }
}

// ---------------------------------------------------------------- Home

fn check<'a>(fleet: &'a Json, id: &str) -> &'a Json {
    fleet["checks"].as_array().and_then(|a| a.iter().find(|c| c["id"] == id)).unwrap_or(&Json::Null)
}

fn status_level(st: &Json) -> Level {
    match st.as_str() {
        Some("ok") | None => Level::Ok,
        Some("warning" | "degraded") => Level::Warn,
        _ => Level::Bad,
    }
}

fn home(cv: &mut Canvas, r: Rect, s: &Shared) -> Vec<(&'static str, &'static str)> {
    let f = &s.data.fleet;
    let w = r.width;
    if f.is_null() {
        cv.put(2, r.y + 1, "Reading the fleet's health…", dim());
        return vec![];
    }
    let lv = status_level(&f["status"]);
    cv.boxed(Rect::new(0, r.y, w, 4), "", level(lv));
    let mark = if lv == Level::Ok { "● " } else { "▲ " };
    cv.put(2, r.y + 1, &fit(&format!("{mark}{}", f["headline"].as_str().unwrap_or("…")), (w - 4) as usize), bold(level(lv)));
    let since = match f["not_ok_since_ms"].as_u64() {
        Some(t) => format!("not all right since {}", model::clock(t)),
        None => f["since_ms"].as_u64().map(|t| format!("all right since {}", model::clock(t))).unwrap_or_default(),
    };
    cv.put(4, r.y + 2, &format!("{since} · over the last {} min", f["window_s"].as_u64().unwrap_or(300) / 60), dim());
    // The cards.
    let copies = check(f, "copies");
    let p99 = check(f, "p99");
    let er = check(f, "error_ratio");
    let lag = check(f, "lag");
    let disk = check(f, "disk");
    let al = check(f, "alerts");
    let total = copies["total"].as_u64().unwrap_or(0);
    let down_n = copies["down"].as_u64().unwrap_or(0);
    let firing = al["firing"].as_u64().unwrap_or(0);
    let critical = al["critical"].as_u64().unwrap_or(0);
    let days = disk["days_to_full"].as_f64();
    let cards: Vec<(&str, String, String, Level)> = vec![
        ("Copies answering", format!("{}/{total}", total.saturating_sub(down_n)), if down_n > 0 { format!("{down_n} down") } else { "all answer".into() }, status_level(&copies["status"])),
        ("p99 latency", p99["recent"]["value_ms"].as_f64().or(p99["value_ms"].as_f64()).map_or("–".into(), model::ms), p99["slo_ms"].as_f64().map_or(String::new(), |v| format!("SLO {}", model::ms(v))), status_level(&p99["status"])),
        ("Errors", er["value"].as_f64().map_or("–".into(), model::pct), er["slo"].as_f64().map_or(String::new(), |v| format!("SLO {}", model::pct(v))), status_level(&er["status"])),
        ("Replication lag", format!("{} txn", model::count(lag["max_txn"].as_f64().unwrap_or(0.0))), lag["max_s"].as_f64().map_or(String::new(), |v| format!("{v:.0} s behind")), status_level(&lag["status"])),
        ("Fullest disk", disk["max"].as_f64().map_or("–".into(), |v| format!("{:.0}%", v * 100.0)), days.map_or(String::new(), |d| if d < 2.0 { format!("full in {:.0} h", d * 24.0) } else { format!("full in {d:.0} days") }), status_level(&disk["status"])),
        ("Alerts firing", firing.to_string(), if critical > 0 { format!("{critical} critical") } else { String::new() }, if critical > 0 { Level::Bad } else if firing > 0 { Level::Warn } else { Level::Ok }),
    ];
    let cw = (w / cards.len() as u16).max(12);
    for (n, (label, val, detail, lv)) in cards.iter().enumerate() {
        let x = n as u16 * cw;
        cv.boxed(Rect::new(x, r.y + 4, cw - 1, 5), "", if *lv == Level::Ok { dim() } else { level(*lv) });
        cv.put(x + 2, r.y + 5, &fit(label, (cw - 4) as usize), dim());
        cv.put(x + 2, r.y + 6, &fit(val, (cw - 4) as usize), bold(level(*lv)));
        cv.put(x + 2, r.y + 7, &fit(detail, (cw - 4) as usize), dim());
    }
    // What is wrong, worst first, and what to do next.
    let probs = f["problems"].as_array().map_or(&[][..], Vec::as_slice);
    let top = r.y + 9;
    let bh = r.height.saturating_sub(9);
    cv.boxed(Rect::new(0, top, w, bh), &if probs.is_empty() { "Problems".to_string() } else { format!("Problems · {}, worst first", probs.len()) }, dim());
    let mut y = top + 1;
    if probs.is_empty() {
        cv.put(2, y, "None. Every shard serves reads and takes writes.", dim());
    }
    for p in probs {
        if y + 2 >= top + bh {
            break;
        }
        let lv = if p["severity"] == "critical" { Level::Bad } else { Level::Warn };
        cv.put(2, y, &fit(&format!("▲ {}", p["title"].as_str().unwrap_or_default()), (w - 4) as usize), bold(level(lv)));
        let next = p["next"]["text"].as_str().or(p["next"].as_str()).unwrap_or_default();
        let line = [p["impact"].as_str().unwrap_or_default(), next].iter().filter(|t| !t.is_empty()).cloned().collect::<Vec<_>>().join(" · next: ");
        cv.put(4, y + 1, &fit(&line, (w - 6) as usize), acc());
        y += 3;
    }
    vec![("⏎", "cluster"), ("space", "pause")]
}

// ---------------------------------------------------------------- Cluster

const TILE_W: u16 = 18;
const TILE_H: u16 = 6;
const PANE_W: u16 = 54;

fn glyph(c: &model::Copy) -> &'static str {
    if c.down {
        "✗"
    } else if c.primary {
        "◆"
    } else {
        "●"
    }
}

fn copy_level(lens: Lens, c: &model::Copy, slo: &Slo, usual: Option<&str>) -> Level {
    if c.down {
        return Level::Bad;
    }
    match lens {
        Lens::Health if c.avoided.is_some() || c.copying => Level::Warn,
        Lens::Health => Level::Ok,
        Lens::Version if c.version.as_deref() != usual => Level::Warn,
        Lens::Version => Level::Ok,
        l => l.level(l.value(c), slo),
    }
}

fn cluster(cv: &mut Canvas, r: Rect, app: &App, s: &Shared, seen: &Seen) -> Vec<(&'static str, &'static str)> {
    let shards = model::shards(&s.data.cluster);
    let slo = Slo::of(&s.data.fleet, &s.data.cluster);
    let usual = model::usual_version(&shards);
    let w = r.width;
    // The lens bar.
    let mut x = cv.put(1, r.y, "lens ", dim());
    for l in model::LENSES {
        x = cv.put(x, r.y, &format!(" {} ", l.name()), if l == app.lens { chip(ACC) } else { fg() }) + 1;
    }
    x = cv.put(x + 2, r.y, "by ", dim());
    x = cv.put(x, r.y, if app.by_node { " node " } else { " shard " }, chip(ACC));
    cv.put(x + 2, r.y, "◆ primary ● replica ✗ down", dim());
    if shards.is_empty() {
        let role = s.data.cluster["role"].as_str().unwrap_or("server");
        cv.put(2, r.y + 2, &format!("This is {}: it has no shards to draw. Point cc at a coordinator for the map.", if role == "coordinator" { "a coordinator with no shards" } else { "a single server" }), dim());
        return vec![];
    }
    let sel_shard = app.shard.min(shards.len() - 1);
    let sh = &shards[sel_shard];
    let sel_copy = app.copy.min(sh.n().saturating_sub(1));
    let map_w = w.saturating_sub(PANE_W + 1);
    let per = (map_w / (TILE_W + 1)).max(1) as usize;
    let rows_fit = ((r.height - 2) / TILE_H).max(1) as usize;
    if app.by_node {
        let nodes = model::nodes(&shards);
        let picked = sh.copies.get(sel_copy).map(|c| c.node.clone());
        let first = nodes.iter().position(|(n, _, _)| Some(n) == picked.as_ref()).unwrap_or(0) / per;
        let skip = first.saturating_sub(rows_fit - 1) * per;
        for (k, (node, zone, copies)) in nodes.iter().enumerate().skip(skip).take(rows_fit * per) {
            let k = k - skip;
            let (tx, ty) = (1 + (k % per) as u16 * (TILE_W + 1), r.y + 2 + (k / per) as u16 * TILE_H);
            let here = Some(node) == picked.as_ref();
            let worst = copies.iter().map(|c| copy_level(app.lens, c, &slo, usual.as_deref())).max().unwrap_or(Level::Ok);
            cv.boxed(Rect::new(tx, ty, TILE_W, TILE_H), &fit(node, (TILE_W - 4) as usize), if here { bold(acc()) } else if worst == Level::Ok { dim() } else { level(worst) });
            for (j, c) in copies.iter().take(3).enumerate() {
                let j = j as u16;
                let lv = copy_level(app.lens, c, &slo, usual.as_deref());
                let val = match app.lens {
                    Lens::Health => c.role.to_string(),
                    Lens::Version => c.version.clone().unwrap_or_else(|| "–".into()),
                    l => l.fmt(l.value(c)),
                };
                let st = if c.url == sh.copies[sel_copy].url { sel() } else { level(lv) };
                // `◆ s0` at the left and the lens's value at the right, as `◆ s0   0.37ms`.
                let iw = (TILE_W - 4) as usize;
                let left = format!("{} s{}", glyph(c), c.shard);
                let val = fit(&val, iw.saturating_sub(left.chars().count() + 1));
                cv.put(tx + 2, ty + 1 + j, &format!("{left}{}", rpad(&val, iw - left.chars().count())), st);
            }
            if copies.len() > 3 {
                cv.put(tx + 2, ty + 4, &format!("+{} more", copies.len() - 3), dim());
            } else if let Some(z) = zone {
                cv.put(tx + 2, ty + 4, &fit(z, (TILE_W - 4) as usize), dim());
            }
        }
    } else {
        let row = sel_shard / per;
        let skip = row.saturating_sub(rows_fit - 1) * per;
        for (k, s2) in shards.iter().enumerate().skip(skip).take(rows_fit * per) {
            let kk = k - skip;
            let (tx, ty) = (1 + (kk % per) as u16 * (TILE_W + 1), r.y + 2 + (kk / per) as u16 * TILE_H);
            let lv = match app.lens {
                Lens::Health => model::health(s2),
                Lens::Version => {
                    if s2.copies.iter().any(|c| !c.down && c.version.as_deref() != usual.as_deref()) {
                        Level::Warn
                    } else {
                        Level::Ok
                    }
                }
                l => l.level(l.shard_value(s2), &slo).max(if s2.up == 0 { Level::Bad } else { Level::Ok }),
            };
            let here = k == sel_shard;
            cv.boxed(Rect::new(tx, ty, TILE_W, TILE_H), &format!("shard {}", s2.i), if here { bold(acc()) } else if lv == Level::Ok { dim() } else { level(lv) });
            let mut gx = tx + 2;
            for c in &s2.copies {
                gx = cv.put(gx, ty + 1, glyph(c), if c.down { col(BAD) } else { level(copy_level(app.lens, c, &slo, usual.as_deref())) }) + 1;
            }
            let val = match app.lens {
                Lens::Health => format!("{}/{} serving", s2.up, s2.n()),
                Lens::Version => s2.copies.iter().filter_map(|c| c.version.clone()).find(|v| Some(v) != usual.as_ref()).or(usual.clone()).unwrap_or_else(|| "–".into()),
                l => l.fmt(l.shard_value(s2)),
            };
            cv.put(tx + 2, ty + 2, &fit(&val, (TILE_W - 4) as usize), bold(level(lv)));
            cv.put(tx + 2, ty + 3, &fit(&s2.slots.map(|n| format!("{} slots", model::count(n as f64))).unwrap_or_default(), (TILE_W - 4) as usize), dim());
            if s2.state != "ok" {
                cv.put(tx + 2, ty + 4, &fit(&s2.state.replace('_', " "), (TILE_W - 4) as usize), level(lv));
            }
        }
        let total_rows = shards.len().div_ceil(per);
        if total_rows > rows_fit {
            cv.put(1, r.y + r.height - 1, &format!("rows {}-{} of {total_rows} · ←→ moves", skip / per + 1, (skip / per + rows_fit).min(total_rows)), dim());
        }
    }
    // The shard picked, and its copies.
    let px = w - PANE_W;
    let (py, ph) = (r.y + 1, r.height - 1);
    cv.boxed(Rect::new(px, py, PANE_W, ph), &format!("shard {} · {} of {} serving · {}", sh.i, sh.up, sh.n(), sh.state.replace('_', " ")), level(model::health(sh)).add_modifier(Modifier::BOLD));
    let iw = (PANE_W - 4) as usize;
    let idw = sh.copies.iter().map(|c| c.id.chars().count() + 2).max().unwrap_or(6).clamp(6, 17);
    cv.put(px + 2, py + 1, &format!("{} {} {} {} {} {}", pad("COPY", idw), pad("ROLE", 7), rpad("LAG", 6), rpad("P99", 7), rpad("CPU", 4), rpad("DISK", 4)), bold(dim()));
    let mut y = py + 2;
    let pct0 = |v: Option<f64>| v.map_or("–".into(), |v| format!("{:.0}%", v * 100.0));
    for (k, c) in sh.copies.iter().enumerate() {
        if y >= py + ph - 6 {
            break;
        }
        let lag = if c.primary { "–".to_string() } else { c.lag.map_or("?".into(), model::count) };
        let row = format!("{} {} {} {} {} {}", pad(&format!("{} {}", glyph(c), c.id), idw), pad(c.role, 7), rpad(&lag, 6), rpad(&c.p99.map_or("–".into(), model::ms), 7), rpad(&pct0(c.cpu), 4), rpad(&pct0(c.disk), 4));
        let st = if k == sel_copy { sel() } else if c.down { col(BAD) } else { fg() };
        cv.put(px + 2, y, &pad(&row, iw), st);
        y += 1;
    }
    y += 1;
    let c = &sh.copies[sel_copy];
    let mut facts = vec![c.url.clone()];
    if c.node != model::host_of(&c.url) {
        facts.push(format!("node {}", c.node));
    }
    if let Some(z) = &c.zone {
        facts.push(format!("zone {z}"));
    }
    let mut more = Vec::new();
    if let Some(v) = &c.version {
        more.push(format!("v{v}"));
    }
    if let Some(t) = c.term {
        more.push(format!("term {t}"));
    }
    if let Some(t) = c.txn {
        more.push(format!("txn {t}"));
    }
    if let Some(q) = c.qps {
        more.push(format!("{q:.1} q/s"));
    }
    if let Some(n) = c.rows {
        more.push(format!("{} rows", model::count(n as f64)));
    }
    let mut notes: Vec<(String, Style)> = vec![(facts.join(" · "), dim()), (more.join(" · "), dim())];
    if let Some(e) = &c.error {
        notes.push((format!("Does not answer: {e}{}", c.down_since_ms.map(|t| format!(" (since {})", model::clock(t))).unwrap_or_default()), col(BAD)));
    }
    if let Some(a) = &c.avoided {
        notes.push((format!("Sent no searches: {a}"), col(WARN)));
    }
    notes.push((sh.why.clone(), dim()));
    match model::why_not_primary(sh, c) {
        None if seen.who.can("admin") => notes.push((format!("p makes {} the primary", c.id), acc())),
        None => notes.push((format!("A handover takes an admin key ({} is {})", seen.who.name, seen.who.scope), dim())),
        Some(_) => {}
    }
    for (text, st) in notes {
        for line in wrap(&text, iw) {
            if y >= py + ph - 1 {
                break;
            }
            cv.put(px + 2, y, &line, st);
            y += 1;
        }
    }
    let mut h = vec![("←→", "shard"), ("↑↓", "copy"), ("L", "lens"), ("G", "group"), ("p", "make primary")];
    if seen.plan_running {
        h.push(("S", "stop the plan"));
    }
    h
}

// ---------------------------------------------------------------- Alerts

fn alerts(cv: &mut Canvas, r: Rect, app: &App, s: &Shared) -> Vec<(&'static str, &'static str)> {
    let list = app.alerts(&s.data.alerts);
    let w = r.width as usize;
    if s.data.alerts.is_null() {
        cv.put(2, r.y + 1, "Reading the alerts…", dim());
        return vec![];
    }
    let firing = list.iter().filter(|a| a["state"] == "firing").count();
    cv.put(1, r.y, &format!("{firing} firing · {} rules · worst first", list.len()), dim());
    cv.put(1, r.y + 1, &format!("{} {} {} {} {}", pad("STATE", 10), pad("SEVERITY", 9), pad("ALERT", 38), pad("WHERE", 22), pad("SINCE", 12)), bold(dim()));
    let detail_h = 7u16;
    let rows = (r.height.saturating_sub(3 + detail_h)) as usize;
    let at = app.alert.min(list.len().saturating_sub(1));
    let skip = at.saturating_sub(rows.saturating_sub(1));
    let now = model::now_ms();
    for (k, a) in list.iter().enumerate().skip(skip).take(rows) {
        let state = a["state"].as_str().unwrap_or("?");
        let shown = if !a["silenced"].is_null() { format!("{state}, silenced") } else if !a["ack"].is_null() && state == "firing" { "firing, ack".into() } else { state.to_string() };
        let wh: Vec<String> = a["where"].as_array().into_iter().flatten().filter_map(|p| p["short"].as_str().or(p["name"].as_str()).or(p.as_str()).map(String::from)).collect();
        let since = a["since_ms"].as_u64().filter(|_| state != "ok").map(|t| format!("{} ago", model::ago(now, t))).unwrap_or_default();
        let row = format!("{} {} {} {} {}", pad(&shown, 10), pad(a["severity"].as_str().unwrap_or(""), 9), pad(a["name"].as_str().unwrap_or(""), 38), pad(&wh.join(", "), 22), pad(&since, 12));
        let lv = match (state, a["severity"].as_str()) {
            ("firing", Some("critical")) => col(BAD),
            ("firing", _) => col(WARN),
            ("pending", _) => acc(),
            ("ok", _) => dim(),
            _ => dim(),
        };
        cv.put(1, r.y + 2 + (k - skip) as u16, &pad(&row, w - 2), if k == at { sel() } else { lv });
    }
    if let Some(a) = list.get(at) {
        let top = r.y + r.height - detail_h;
        cv.boxed(Rect::new(0, top, r.width, detail_h), a["name"].as_str().unwrap_or("alert"), dim());
        let mut lines = wrap(a["description"].as_str().unwrap_or_default(), w - 4);
        let mut facts = format!("{} {} {}", a["metric"].as_str().unwrap_or(""), a["op"].as_str().unwrap_or(""), a["threshold"]);
        if let Some(v) = a["value"].as_f64() {
            facts.push_str(&format!(" · now {v}"));
        }
        if let Some(by) = a["ack"]["by"].as_str() {
            facts.push_str(&format!(" · acknowledged by {by}"));
        }
        if let Some(until) = a["silenced"]["until_ms"].as_u64() {
            facts.push_str(&format!(" · silenced until {}", model::clock(until)));
        }
        if let Some(rb) = a["runbook_docs"].as_str().or(a["runbook"].as_str()).filter(|s| !s.is_empty()) {
            facts.push_str(&format!(" · runbook {rb}"));
        }
        lines.push(facts);
        for (k, l) in lines.iter().take((detail_h - 2) as usize).enumerate() {
            cv.put(2, top + 1 + k as u16, &fit(l, w - 4), if k + 1 == lines.len() { dim() } else { fg() });
        }
    }
    vec![("↑↓", "pick"), ("a", "acknowledge"), ("s", "silence"), ("u", "unsilence")]
}

// ---------------------------------------------------------------- Queries

fn queries(cv: &mut Canvas, r: Rect, app: &App, s: &Shared) -> Vec<(&'static str, &'static str)> {
    let w = r.width as usize;
    let live = s.data.live["queries"].as_array().map_or(&[][..], Vec::as_slice);
    let stats = app.statements(&s.data.stats);
    let mut y = r.y;
    let at = app.query.min((live.len() + stats.len()).saturating_sub(1));
    cv.put(1, y, &format!("Running now · {}", live.len()), bold(fg()));
    y += 1;
    if live.is_empty() {
        cv.put(3, y, "Nothing runs now.", dim());
        y += 1;
    }
    for (k, q) in live.iter().enumerate().take(6) {
        let who = q["key"].as_str().or(q["app"].as_str()).unwrap_or("");
        let row = format!("{} {} {} {}", rpad(&q["id"].to_string().trim_matches('"').to_string(), 10), rpad(&q["elapsed_ms"].as_f64().map_or("–".into(), model::ms), 8), pad(who, 14), q["sql"].as_str().unwrap_or_default().replace('\n', " "));
        cv.put(1, y, &pad(&row, w - 2), if k == at { sel() } else { fg() });
        y += 1;
    }
    y += 1;
    let window = match (s.data.stats["from"].as_u64(), s.data.stats["to"].as_u64()) {
        (Some(a), Some(b)) => format!("{} to {}", model::clock(a), model::clock(b)),
        _ => "the last 15 min".into(),
    };
    cv.put(1, y, &format!("Statements · {window} · sorted by {} (o)", app.sort.name()), bold(fg()));
    y += 1;
    cv.put(1, y, &format!("{} {} {} {} {} {}  STATEMENT", rpad("TIME", 6), rpad("CALLS", 8), rpad("P50", 8), rpad("P99", 8), rpad("ERRORS", 7), rpad("ROWS", 6)), bold(dim()));
    y += 1;
    let detail_h = 4u16;
    let rows = (r.y + r.height).saturating_sub(y + detail_h) as usize;
    let sat = at.saturating_sub(live.len());
    let skip = if at >= live.len() { sat.saturating_sub(rows.saturating_sub(1)) } else { 0 };
    for (k, st) in stats.iter().enumerate().skip(skip).take(rows) {
        let errs = st["errors"].as_u64().unwrap_or(0);
        let row = format!(
            "{} {} {} {} {} {}  {}",
            rpad(&st["share"].as_f64().map_or("–".into(), |v| format!("{:.0}%", v * 100.0)), 6),
            rpad(&model::count(st["calls"].as_f64().unwrap_or(0.0)), 8),
            rpad(&st["p50_ms"].as_f64().map_or("–".into(), model::ms), 8),
            rpad(&st["p99_ms"].as_f64().map_or("–".into(), model::ms), 8),
            rpad(&model::count(errs as f64), 7),
            rpad(&st["rows_per_call"].as_f64().map_or("–".into(), |v| format!("{v:.0}")), 6),
            st["fingerprint"].as_str().unwrap_or_default().replace('\n', " ")
        );
        let here = at >= live.len() && k == sat;
        cv.put(1, y, &pad(&row, w - 2), if here { sel() } else if errs > 0 { col(WARN) } else { fg() });
        y += 1;
    }
    if stats.is_empty() {
        cv.put(3, y, "No statement ran in this window.", dim());
    }
    if at >= live.len() {
        if let Some(st) = stats.get(sat) {
            let top = r.y + r.height - detail_h;
            let keys: Vec<String> = st["keys"].as_array().into_iter().flatten().take(4).map(|k| format!("{} ({})", k["key"].as_str().unwrap_or("?"), model::count(k["calls"].as_f64().unwrap_or(0.0)))).collect();
            cv.put(1, top, &fit(&format!("e.g. {}", st["sql"].as_str().unwrap_or_default().replace('\n', " ")), w - 2), dim());
            cv.put(1, top + 1, &fit(&format!("{} · mean {} · max {} · keys {}", st["kind"].as_str().unwrap_or(""), st["mean_ms"].as_f64().map_or("–".into(), model::ms), st["max_ms"].as_f64().map_or("–".into(), model::ms), keys.join(", ")), w - 2), dim());
        }
    }
    vec![("↑↓", "pick"), ("o", "sort"), ("x", "cancel a running query")]
}

// ---------------------------------------------------------------- Logs

fn logs(cv: &mut Canvas, r: Rect, app: &App, s: &Shared) -> Vec<(&'static str, &'static str)> {
    if !s.who.reads_server_log() {
        return log_tables(cv, r, app, s);
    }
    let w = r.width as usize;
    let lines = s.data.logs["lines"].as_array().map_or(&[][..], Vec::as_slice);
    let follow = if app.log_back == 0 { "following new lines".to_string() } else { format!("{} lines up · End follows", app.log_back) };
    let filter = if app.log_q.is_empty() { String::new() } else { format!(" · with \"{}\"", app.log_q) };
    cv.put(1, r.y, &format!("level {} (f){filter} (/) · {follow} · {} lines", app.log_level().unwrap_or("all"), lines.len()), dim());
    cv.put(1, r.y + 1, &format!("{} {} {} MESSAGE", pad("TIME", 11), pad("LEVEL", 5), pad("WHERE", 22)), bold(dim()));
    let n = r.height.saturating_sub(2) as usize;
    let end = lines.len().saturating_sub(app.log_back.min(lines.len().saturating_sub(n.min(lines.len()))));
    let start = end.saturating_sub(n);
    for (k, l) in lines[start..end].iter().enumerate() {
        let lvl = l["level"].as_str().unwrap_or("");
        let st = match lvl {
            "error" => bold(col(BAD)),
            "warn" => col(WARN),
            "debug" | "trace" => dim(),
            _ => fg(),
        };
        let wh = l["node"].as_str().or(l["target"].as_str()).unwrap_or("").trim_start_matches("chloedb_server::");
        let fields: Vec<String> = l["fields"].as_object().into_iter().flatten().map(|(a, b)| format!("{a}={}", b.as_str().map_or(b.to_string(), String::from))).collect();
        let text = format!("{} {} {} {} {}", pad(&l["ts_ms"].as_u64().map_or("–".into(), model::clock), 11), pad(&lvl.to_uppercase(), 5), pad(wh, 22), l["message"].as_str().unwrap_or_default(), fields.join(" "));
        cv.put(1, r.y + 2 + k as u16, &fit(&text, w - 2), st);
    }
    if lines.is_empty() && !s.data.logs.is_null() {
        cv.put(3, r.y + 2, "No line matches.", dim());
    }
    vec![("↑↓ PgUp PgDn", "scroll"), ("End", "follow"), ("f", "level"), ("/", "filter"), ("space", "pause")]
}

/// The Logs screen of a key that does not read the server's own log
/// (MARCO-313): what this screen is and which key reads it, and the log
/// tables the key reads, whose newest lines Enter opens in SQL.
fn log_tables(cv: &mut Canvas, r: Rect, app: &App, s: &Shared) -> Vec<(&'static str, &'static str)> {
    let w = r.width as usize - 2;
    let key = match &s.who.limited {
        Some(g) => format!("The key {} is limited to tables ({g})", s.who.name),
        None => format!("The key {} is a {} key", s.who.name, s.who.scope),
    };
    cv.put(1, r.y, &fit("This screen is the server's own log, which an admin key of the server reads (or an ops or propose key).", w), dim());
    let tables = model::log_tables(&s.data.log_tables);
    if s.data.log_tables.is_null() {
        cv.put(1, r.y + 1, &fit(&format!("{key}: reading the tables it reads…"), w), dim());
        return vec![];
    }
    if tables.is_empty() {
        cv.put(1, r.y + 1, &fit(&format!("{key}, and reads no table with a TIMESTAMP: it has no log table here."), w), fg());
        return vec![];
    }
    cv.put(1, r.y + 1, &fit(&format!("{key}. Its log tables, the tables with a TIMESTAMP it reads: Enter shows the newest lines in SQL."), w), fg());
    cv.put(1, r.y + 3, &format!("{} {}", pad("TABLE", 32), pad("TIME", 20)), bold(dim()));
    let at = app.log_table.min(tables.len() - 1);
    let rows = r.height.saturating_sub(6) as usize;
    let skip = at.saturating_sub(rows.saturating_sub(1));
    for (k, (t, ts)) in tables.iter().enumerate().skip(skip).take(rows) {
        cv.put(1, r.y + 4 + (k - skip) as u16, &pad(&format!("{} {}", pad(t, 32), pad(ts, 20)), w), if k == at { sel() } else { fg() });
    }
    let n = tables.len().min(rows) as u16;
    cv.put(1, r.y + 5 + n, &fit(&format!("In a browser the Log explorer searches them: {}/#/explorer", s.base), w), dim());
    vec![("↑↓", "pick a table"), ("Enter", "newest lines in SQL")]
}

// ---------------------------------------------------------------- Tables

fn tables(cv: &mut Canvas, r: Rect, app: &App, s: &Shared) -> Vec<(&'static str, &'static str)> {
    let w = r.width as usize;
    let ts = s.data.tables["tables"].as_array().map_or(&[][..], Vec::as_slice);
    if s.data.tables.is_null() {
        cv.put(2, r.y + 1, "Reading the tables…", dim());
        return vec![];
    }
    let now = model::now_ms();
    cv.put(1, r.y, &format!("{} tables · {} rows · {} on one copy of each shard", ts.len(), model::count(ts.iter().filter_map(|t| t["docs"].as_f64()).sum()), model::bytes(ts.iter().filter_map(|t| t["bytes"].as_f64()).sum())), dim());
    cv.put(1, r.y + 1, &format!("{} {} {} {} {} {} {} {}", pad("TABLE", 24), rpad("ROWS", 12), rpad("SIZE", 10), rpad("SEGS", 5), rpad("READS/MIN", 10), rpad("WRITES/MIN", 11), rpad("SKEW", 6), rpad("LAST WRITE", 11)), bold(dim()));
    let at = app.table.min(ts.len().saturating_sub(1));
    let shards_n = ts.get(at).and_then(|t| t["shards"].as_array()).map_or(0, Vec::len) as u16;
    let detail_h = (shards_n + 2).min(r.height / 2);
    let rows = r.height.saturating_sub(3 + detail_h) as usize;
    let skip = at.saturating_sub(rows.saturating_sub(1));
    for (k, t) in ts.iter().enumerate().skip(skip).take(rows) {
        let skew = t["skew"]["over_mean"].as_f64().unwrap_or(0.0);
        let row = format!(
            "{} {} {} {} {} {} {} {}",
            pad(t["table"].as_str().unwrap_or(""), 24),
            rpad(&model::count(t["docs"].as_f64().unwrap_or(0.0)), 12),
            rpad(&model::bytes(t["bytes"].as_f64().unwrap_or(0.0)), 10),
            rpad(&t["segments"].as_u64().map_or("–".into(), |n| n.to_string()), 5),
            rpad(&format!("{:.0}", t["reads_per_min"].as_f64().unwrap_or(0.0)), 10),
            rpad(&format!("{:.0}", t["writes_per_min"].as_f64().unwrap_or(0.0)), 11),
            rpad(&format!("{:.2}", 1.0 + skew), 6),
            rpad(&t["last_write_ms"].as_u64().map_or("–".into(), |m| format!("{} ago", model::ago(now, m))), 11),
        );
        cv.put(1, r.y + 2 + (k - skip) as u16, &pad(&row, w - 2), if k == at { sel() } else if skew > 0.5 { col(WARN) } else { fg() });
    }
    if let Some(t) = ts.get(at) {
        let top = r.y + 3 + ts.len().min(rows) as u16;
        let name = t["table"].as_str().unwrap_or("");
        cv.boxed(Rect::new(0, top, r.width, detail_h), &format!("{name} by shard"), dim());
        let sh = t["shards"].as_array().map_or(&[][..], Vec::as_slice);
        let max = sh.iter().filter_map(|x| x["bytes"].as_f64()).fold(1.0, f64::max);
        let bw = (w.saturating_sub(44)).min(60);
        for (k, x) in sh.iter().enumerate().take((detail_h - 2) as usize) {
            let y = top + 1 + k as u16;
            let b = x["bytes"].as_f64().unwrap_or(0.0);
            cv.put(2, y, &format!("shard {}", x["shard"]), dim());
            cv.put(12, y, &"█".repeat(((bw as f64) * b / max).round() as usize), acc());
            cv.put(13 + bw as u16, y, &format!("{} · {} rows", model::bytes(b), model::count(x["docs"].as_f64().unwrap_or(0.0))), fg());
        }
    }
    vec![("↑↓", "pick a table")]
}

// ---------------------------------------------------------------- SQL

fn sql(cv: &mut Canvas, r: Rect, app: &App, s: &Shared, seen: &Seen) -> Vec<(&'static str, &'static str)> {
    let w = r.width;
    let prod = app.production(seen);
    let name = s.who.cluster.clone().unwrap_or_else(|| model::host_of(&s.base));
    let mode = if !s.who.can("write") {
        ("read-only: the key cannot write".to_string(), dim())
    } else if !prod {
        ("writes on: a sandbox or a test cluster (each write still asks first)".to_string(), dim())
    } else if app.writes_allowed() {
        ("writes allowed for now (each asks first)".to_string(), bold(col(WARN)))
    } else {
        ("production: read-only here, W allows writes for 15 min".to_string(), bold(acc()))
    };
    let x = cv.put(1, r.y, &format!("{name} · {} · ", s.who.name), dim());
    cv.put(x, r.y, &mode.0, mode.1);
    let st = Statement::of(&app.sql.text);
    let label = if app.sql.text.trim().is_empty() { "SQL".into() } else if st.write { format!("{} (asks first)", st.verb) } else { format!("⏎ runs {}", st.verb) };
    cv.boxed(Rect::new(0, r.y + 1, w, 3), &label, if app.sql_focus { bold(acc()) } else { dim() });
    cv.input(2, r.y + 2, w - 4, &app.sql, app.sql_focus, fg());
    if app.sql.text.is_empty() && !app.sql_focus {
        cv.put(2, r.y + 2, "i or ⏎ to type a statement", dim());
    }
    let top = r.y + 4;
    if s.sql_running {
        cv.put(1, top, "Running…", acc());
        return sql_hints();
    }
    let Some(ans) = &s.sql else {
        cv.put(1, top, "Statements run as the key, on the coordinator; SELECT, EXPLAIN and SHOW run at once.", dim());
        return sql_hints();
    };
    match &ans.answer {
        Err(e) => {
            for (k, l) in wrap(e, (w - 2) as usize).iter().take(r.height.saturating_sub(5) as usize).enumerate() {
                cv.put(1, top + k as u16, l, col(BAD));
            }
        }
        Ok(j) => {
            let rows = j["rows"].as_array().map_or(&[][..], Vec::as_slice);
            let cols: Vec<String> = j["columns"].as_array().into_iter().flatten().map(|c| c.as_str().map_or(c.to_string(), String::from)).collect();
            let took = j["took_ms"].as_f64().map(model::ms).unwrap_or_default();
            if cols.is_empty() {
                let what = j["written"].as_u64().or(j["changed"].as_u64()).map(|n| format!("{n} rows changed")).unwrap_or_else(|| "Done".into());
                cv.put(1, top, &format!("{what} · {took} · at {}", model::clock(ans.at_ms)), col(OK));
                return sql_hints();
            }
            cv.put(1, top, &format!("{} rows · {took} · at {}", rows.len(), model::clock(ans.at_ms)), dim());
            let cell = |v: &Json| v.as_str().map_or_else(|| if v.is_null() { "NULL".into() } else { v.to_string() }, String::from).replace('\n', " ");
            // Each column as wide as its widest value, up to 40.
            let mut widths: Vec<usize> = cols.iter().map(|c| c.chars().count()).collect();
            for row in rows.iter().take(200) {
                for (k, v) in row.as_array().into_iter().flatten().enumerate() {
                    if let Some(wd) = widths.get_mut(k) {
                        *wd = (*wd).max(cell(v).chars().count()).min(40);
                    }
                }
            }
            let line = |vals: Vec<String>| vals.iter().zip(&widths).map(|(v, wd)| pad(v, *wd)).collect::<Vec<_>>().join(" │ ");
            cv.put(1, top + 1, &fit(&line(cols.clone()), (w - 2) as usize), bold(dim()));
            let n = r.height.saturating_sub(6) as usize;
            let first = app.sql_row.min(rows.len().saturating_sub(n));
            for (k, row) in rows.iter().skip(first).take(n).enumerate() {
                let vals: Vec<String> = row.as_array().into_iter().flatten().map(cell).collect();
                cv.put(1, top + 2 + k as u16, &fit(&line(vals), (w - 2) as usize), fg());
            }
            if rows.len() > n {
                cv.put(1, r.y + r.height - 1, &format!("rows {}-{} of {} · PgUp PgDn", first + 1, (first + n).min(rows.len()), rows.len()), dim());
            }
        }
    }
    sql_hints()
}

fn sql_hints() -> Vec<(&'static str, &'static str)> {
    vec![("i", "type"), ("↑↓ PgUp PgDn", "rows"), ("W", "allow writes")]
}

// ---------------------------------------------------------------- Timeline

fn timeline(cv: &mut Canvas, r: Rect, app: &App, s: &Shared) -> Vec<(&'static str, &'static str)> {
    let w = r.width as usize;
    let ev = s.data.events["events"].as_array().map_or(&[][..], Vec::as_slice);
    if s.data.events.is_null() {
        cv.put(2, r.y + 1, "Reading the timeline…", dim());
        return vec![];
    }
    cv.put(1, r.y, &format!("{} events, newest first · changes, alerts, primaries and failures", ev.len()), dim());
    cv.put(1, r.y + 1, &format!("{} {} TITLE", pad("TIME", 11), pad("KIND", 18)), bold(dim()));
    let detail_h = 4u16;
    let rows = r.height.saturating_sub(2 + detail_h) as usize;
    let at = app.event.min(ev.len().saturating_sub(1));
    let skip = at.saturating_sub(rows.saturating_sub(1));
    for (k, e) in ev.iter().enumerate().skip(skip).take(rows) {
        let st = match e["severity"].as_str() {
            Some("critical" | "error") => col(BAD),
            Some("warning" | "warn") => col(WARN),
            _ => fg(),
        };
        let t = e["ts_ms"].as_u64().or(e["detail"]["ts_ms"].as_u64()).or(e["id"].as_u64().map(|i| i / 1000));
        let row = format!("{} {} {}", pad(&t.map_or("–".into(), model::clock), 11), pad(e["kind"].as_str().unwrap_or(""), 18), e["title"].as_str().unwrap_or_default());
        cv.put(1, r.y + 2 + (k - skip) as u16, &pad(&fit(&row, w - 2), w - 2), if k == at { sel() } else { st });
    }
    if let Some(e) = ev.get(at) {
        let top = r.y + r.height - detail_h;
        let d = &e["detail"];
        let text = d["description"].as_str().or(d["reason"].as_str()).or(d["text"].as_str()).map(String::from).unwrap_or_else(|| e["title"].as_str().unwrap_or_default().to_string());
        let by = d["by"].as_str().map(|b| format!(" · by {b}")).unwrap_or_default();
        for (k, l) in wrap(&format!("{text}{by}"), w - 2).iter().take(detail_h as usize).enumerate() {
            cv.put(1, top + k as u16, l, dim());
        }
    }
    vec![("↑↓ PgUp PgDn", "pick")]
}

// ---------------------------------------------------------------- overlays

fn centered(cv: &Canvas, w: u16, h: u16) -> Rect {
    let w = w.min(cv.w() - 2);
    let h = h.min(cv.h() - 2);
    Rect::new((cv.w() - w) / 2, (cv.h() - h) / 2, w, h)
}

fn dialog(cv: &mut Canvas, d: &Dialog) {
    let w = 76u16.min(cv.w() - 4);
    let iw = (w - 4) as usize;
    let lines: Vec<String> = d.lines.iter().flat_map(|l| wrap(l, iw)).collect();
    let h = (lines.len() + d.fields.len() * 3 + 5) as u16;
    let r = centered(cv, w, h);
    cv.clear(r, bar());
    cv.boxed(r, &d.title, bold(if d.danger { col(BAD) } else { acc() }));
    let mut y = r.y + 1;
    for l in &lines {
        cv.put(r.x + 2, y, l, bar());
        y += 1;
    }
    y += 1;
    for (k, f) in d.fields.iter().enumerate() {
        cv.put(r.x + 2, y, &fit(&f.label, iw), bold(bar()));
        let ok = f.ok() && !f.input.text.is_empty();
        if ok && matches!(f.must, Must::Equal(_)) {
            cv.put(r.x + w - 13, y, "✓ matches", on(OK, BAR));
        }
        cv.input(r.x + 2, y + 1, (iw - 1) as u16, &f.input, k == d.focus, on(BLACK, Color::Indexed(250)));
        y += 3;
    }
    let ready = d.ready();
    let x = cv.put(r.x + 2, y, &format!(" ⏎ {} ", d.ok), if ready { chip(if d.danger { BAD } else { ACC }) } else { on(DIM, BAR) });
    cv.put(x + 2, y, if ready { "Esc cancels" } else { "fill in the fields first · Esc cancels" }, bar());
}

fn help(cv: &mut Canvas) {
    let rows = [
        ("1-8, Tab", "Home, Cluster, Alerts, Queries, Logs, Tables, SQL, Timeline"),
        ("g + letter", "the web's shortcuts: g c Cluster, g a Alerts, g l Logs, g w SQL…"),
        (":", "a command: a screen's name, ctx [name], lens NAME, writes, sql …, q"),
        ("space", "pause reading, or follow live again"),
        ("r", "read now"),
        ("Cluster", "←→ shard, ↑↓ copy, L lens, G by shard or node, p make it the primary"),
        ("Alerts", "a acknowledge, s silence for a while, u unsilence"),
        ("Queries", "o sort, x cancel a query that runs now"),
        ("Logs", "f level, / filter, ↑↓ PgUp PgDn scroll, End follows; a key without the server's log: ↑↓ a log table, Enter its newest lines"),
        ("SQL", "i type, ⏎ run, ↑↓ history, Esc leave the editor, W allow writes"),
        ("S", "stop the plan that runs on the coordinator"),
        (":ctx", "through a hub: its connections, to open another cluster"),
        ("q, Ctrl-C", "quit (a plan that runs goes on)"),
    ];
    let r = centered(cv, 84, rows.len() as u16 + 4);
    cv.clear(r, bar());
    cv.boxed(r, "Keys", bold(acc()));
    for (k, (key, what)) in rows.iter().enumerate() {
        let y = r.y + 1 + k as u16;
        cv.put(r.x + 2, y, &pad(key, 12), bold(on(ACC, BAR)));
        cv.put(r.x + 15, y, &fit(what, (r.width - 17) as usize), bar());
    }
    cv.put(r.x + 2, r.y + r.height - 2, "Every change goes through the same API and key scopes as the web command center.", on(DIM, BAR));
}

fn ctx(cv: &mut Canvas, at: usize, seen: &Seen) {
    let items = ctx_items(seen.base, seen.hub);
    let r = centered(cv, 90, items.len().max(1) as u16 + 6);
    cv.clear(r, bar());
    let (root, _) = hub_of(seen.base);
    cv.boxed(r, &format!("Clusters in the book of the hub {}", model::host_of(&root)), bold(acc()));
    let iw = (r.width - 4) as usize;
    match seen.hub {
        None => {
            cv.put(r.x + 2, r.y + 1, "Reading the hub's connections…", bar());
        }
        Some(Err(e)) => {
            for (k, l) in wrap(e, iw).iter().take(3).enumerate() {
                cv.put(r.x + 2, r.y + 1 + k as u16, l, on(BAD, BAR));
            }
        }
        Some(Ok(_)) => {}
    }
    let y0 = r.y + if matches!(seen.hub, Some(Ok(_))) { 1 } else { 4 };
    for (k, it) in items.iter().enumerate() {
        let y = y0 + k as u16;
        if y >= r.y + r.height - 1 {
            break;
        }
        let here = it.base == seen.base;
        let text = format!("{} {} {}", if here { "▸" } else { " " }, pad(&it.name, 20), it.detail);
        cv.put(r.x + 2, y, &pad(&fit(&text, iw), iw), if k == at { sel() } else { bar() });
    }
    cv.put(r.x + 2, r.y + r.height - 2, &fit("Each opens through the hub, which adds that cluster's key: this terminal never holds it.", iw), on(DIM, BAR));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cc::api::Who;
    use serde_json::json;

    fn shared() -> Shared {
        let mut s = Shared::new("https://hub.example:7700/c/prod", Who { name: "ops-dana".into(), scope: "admin".into(), cluster: Some("orders-prod".into()), environment: Some("production".into()), ..Who::default() });
        s.data.cluster = json!({"role": "coordinator", "cluster": {"name": "orders-prod"}, "plan": {"state": "running", "done": 0, "steps": [{"state": "running", "text": "Add a copy of shard 1"}]}, "shards": [
            {"shard": 0, "state": "ok", "slots_held": 2048, "replicas": [
                {"url": "http://a:7711", "short": "s0-0", "takes_writes": true, "term": 1, "txn": 9, "status": "ok", "detail": {"minute": {"p99_ms": 8.0, "busy": 0.2}}},
                {"url": "http://b:7711", "short": "s0-1", "takes_writes": false, "term": 1, "txn": 9, "status": "ok", "detail": {"minute": {"p99_ms": 9.0}}}]},
            {"shard": 1, "state": "degraded", "slots_held": 2048, "replicas": [
                {"url": "http://c:7711", "short": "s1-0", "takes_writes": true, "term": 1, "txn": 4, "status": "ok"},
                {"url": "http://d:7711", "short": "s1-1", "status": "down", "error": "connection refused"}]}]});
        s.data.fleet = json!({"status": "degraded", "headline": "Shard 1 serves from 1 of 2 copies", "window_s": 300, "checks": [{"id": "alerts", "firing": 2, "critical": 0}, {"id": "copies", "total": 4, "down": 1, "status": "warning"}],
            "problems": [{"severity": "warning", "title": "s1-1 does not answer", "impact": "shard 1 has no spare copy", "next": {"text": "Replace it in Cluster"}}]});
        s
    }

    #[test]
    fn home_says_what_is_wrong_and_what_to_do() {
        let wide = render(&App::default(), &shared(), 160, 30);
        assert!(wide.contains("via hub hub.example:7700"), "{wide}");
        let t = render(&App::default(), &shared(), 120, 30);
        assert!(t.contains("orders-prod"), "{t}");
        assert!(t.contains(" PRODUCTION "), "{t}");
        assert!(t.contains("ops-dana · admin"), "{t}");
        assert!(t.contains("Shard 1 serves from 1 of 2 copies"), "{t}");
        assert!(t.contains("Copies answering") && t.contains("3/4"), "{t}");
        assert!(t.contains("▲ s1-1 does not answer"), "{t}");
        assert!(t.contains("next: Replace it in Cluster"), "{t}");
        assert!(t.contains("PLAN RUNNING on the coordinator · step 1 of 1: Add a copy of shard 1"), "{t}");
        assert!(t.contains("Alerts  2 "), "the alert count sits on its tab: {t}");
    }

    #[test]
    fn the_map_draws_every_shard_and_the_copy_picked() {
        let app = App { screen: Screen::Cluster, shard: 1, copy: 1, ..App::default() };
        let t = render(&app, &shared(), 120, 30);
        assert!(t.contains("shard 0") && t.contains("shard 1"), "{t}");
        assert!(t.contains("◆ ●") && t.contains("◆ ✗"), "{t}");
        assert!(t.contains("1/2 serving"), "{t}");
        assert!(t.contains("shard 1 · 1 of 2 serving · degraded"), "{t}");
        assert!(t.contains("Does not answer: connection refused"), "{t}");
        assert!(t.contains("S stop the plan") || t.contains(" S "), "{t}");
        let by_node = App { by_node: true, ..app };
        let t = render(&by_node, &shared(), 120, 30);
        assert!(t.contains("d:7711") && t.contains("✗ s1") && t.contains("down"), "{t}");
    }

    #[test]
    fn a_small_terminal_is_asked_to_grow() {
        let t = render(&App::default(), &shared(), 60, 20);
        assert!(t.contains("at least 80 columns by 24") && t.contains("(60×20"), "{t}");
    }

    #[test]
    fn a_dialog_shows_its_fields_and_whether_it_may_go() {
        let mut app = App { screen: Screen::Cluster, copy: 1, ..App::default() };
        let s = shared();
        let seen = Seen { data: &s.data, who: &s.who, base: &s.base, hub: None, plan_running: false };
        app.key(ratatui::crossterm::event::KeyEvent::from(ratatui::crossterm::event::KeyCode::Char('p')), &seen);
        let t = render(&app, &s, 120, 30);
        assert!(t.contains("Make s0-1 shard 0's primary"), "{t}");
        assert!(t.contains("Why? Kept in the audit log with the change"), "{t}");
        assert!(t.contains("fill in the fields first"), "{t}");
    }

    /// MARCO-313: Logs, for a key limited to tables, says it is the
    /// server's own log and who reads it, and lists the key's log tables.
    #[test]
    fn logs_of_a_key_limited_to_tables_are_its_log_tables() {
        let mut s = shared();
        s.who = Who { name: "netops".into(), scope: "write".into(), limited: Some("logs_* write, ctx_incidents read".into()), ..Who::default() };
        s.data.log_tables = json!({"tables": [
            {"table": "logs_syslog", "short": {"fields": [{"name": "ts", "type": "timestamp"}, {"name": "body", "type": "text"}]}},
            {"table": "logs_app", "short": {"fields": [{"name": "observed", "type": "timestamp"}, {"name": "ts", "type": "timestamp"}]}},
            {"table": "ctx_incidents", "short": {"fields": [{"name": "id", "type": "keyword"}]}}]});
        assert_eq!(model::log_tables(&s.data.log_tables), [("logs_app".to_string(), "ts".to_string()), ("logs_syslog".into(), "ts".into())]);
        let t = render(&App { screen: Screen::Logs, log_table: 1, ..App::default() }, &s, 140, 30);
        assert!(t.contains("This screen is the server's own log, which an admin key of the server reads"), "{t}");
        assert!(t.contains("The key netops is limited to tables (logs_* write, ctx_incidents read)"), "{t}");
        let listed: Vec<&str> = t.lines().filter_map(|l| l.trim_start().split_whitespace().next()).filter(|w| w.starts_with("logs_") || w.starts_with("ctx_")).collect();
        assert_eq!(listed, ["logs_app", "logs_syslog"], "{t}");
        assert!(t.contains("https://hub.example:7700/c/prod/#/explorer"), "{t}");
        assert!(!t.contains("No line matches"), "{t}");
        s.data.log_tables = json!({"tables": []});
        let t = render(&App { screen: Screen::Logs, ..App::default() }, &s, 140, 30);
        assert!(t.contains("reads no table with a TIMESTAMP"), "{t}");
    }

    #[test]
    fn wrapping_keeps_words_whole() {
        assert_eq!(wrap("one two three four", 9), ["one two", "three", "four"]);
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(fit("abcdef", 4), "abc…");
    }
}
