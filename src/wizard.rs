//! The setup wizard: full screen, 1 decision per screen, the concepts shown, not explained.
//!
//! 0 harnesses (only when 0 is ready, the dry run always) · 1 home · 2 crew · 3 try it (the
//! tutorial) · 4 next steps.
//!
//! `codehead setup --dry`: every screen, 0 writes; on exit the list of what a real run does. The
//! tutorial plays a scripted thread (the real one runs on ephemeral sub-threads, not built yet).
//! A run without `--dry`: the same screens, then the same list (the writes come with the home
//! sync and sub-threads).
//!
//! keys: enter next · esc back, always (1st screen: quit) · ctrl+c quit · each screen's own in
//! its footer. 0 modes: text (path, name, prompt) takes typing and paste at once, so esc never
//! only closes an input.

use std::io;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::avatar::{self, Avatar, Cat, Spawn, Status};
use crate::engine::harnesses;
use crate::theme;

/// Width of the wizard's column.
const COL: u16 = 68;
/// Names a new agent can get.
const NAMES: [&str; 24] = [
    "ADA", "BYTE", "CRUX", "DELTA", "ECHO", "FLUX", "HEX", "IRIS", "JUNO", "KILO", "LUX", "MOTH", "NOVA", "ORBI", "PIXL", "QUILL", "RUNE", "SABLE", "TALON", "VOLT", "WREN", "XENO", "YUKI", "ZED",
];
/// Harnesses the harness screen lists when not installed.
const OFFER: [&str; 5] = ["claude", "codex", "opencode", "cursor", "amp"];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Screen {
    Harness,
    Home,
    Crew,
    Try,
    Next,
}

const SCREENS: [Screen; 5] = [Screen::Harness, Screen::Home, Screen::Crew, Screen::Try, Screen::Next];

#[derive(Clone, Copy, PartialEq, Debug)]
enum HomeChoice {
    New,
    Clone,
    Existing,
    Skip,
}

const HOME_CHOICES: [(HomeChoice, &str, &str); 4] = [
    (HomeChoice::New, "new", "fresh repo + private GitHub repo"),
    (HomeChoice::Clone, "clone", "2nd device: your crew comes along"),
    (HomeChoice::Existing, "existing", "a git repo you have"),
    (HomeChoice::Skip, "skip", "local only, 0 sync"),
];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Role {
    Builder,
    Reviewer,
    Planner,
}

impl Role {
    fn name(self) -> &'static str {
        match self {
            Role::Builder => "builder",
            Role::Reviewer => "reviewer",
            Role::Planner => "planner",
        }
    }
}

const PRESETS: [(&str, &[Role]); 3] = [("solo", &[Role::Builder]), ("pair", &[Role::Builder, Role::Reviewer]), ("crew", &[Role::Planner, Role::Builder, Role::Reviewer])];

/// An agent of the crew screen.
struct Member {
    name: String,
    role: Role,
    harness: &'static str,
    avatar: Avatar,
    color: &'static str,
    spawn: Spawn,
}

/// Key K on line TEXT (the cursor at the end): a char, backspace, alt+backspace (the last word),
/// ctrl+u (all). true: taken.
fn edit(text: &mut String, k: &KeyEvent) -> bool {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let alt = k.modifiers.contains(KeyModifiers::ALT);
    match k.code {
        KeyCode::Char('u') if ctrl => text.clear(),
        KeyCode::Backspace if alt => {
            let t = text.trim_end().len();
            let cut = text[..t].rfind([' ', '/']).map_or(0, |i| i + 1);
            text.truncate(cut);
        }
        KeyCode::Backspace => {
            text.pop();
        }
        KeyCode::Char(c) if !ctrl && !alt => text.push(c),
        _ => return false,
    }
    true
}

pub struct Wizard {
    dry: bool,
    screen: usize,
    seed: u64,
    // harness screen
    installed: Vec<&'static str>,
    hcursor: usize,
    note: String,
    // home screen
    home: usize,
    /// The path / URL of each home choice (skip: unused).
    paths: [String; 4],
    // crew screen
    preset: usize,
    crew: Vec<Member>,
    focus: usize,
    /// The tutorial runs next (its own process, the loop in `run` starts it).
    launch: bool,
    /// Screens before this one: esc goes back to the last.
    history: Vec<usize>,
    quit: bool,
    /// Screen entered at: the title decode.
    entered: Instant,
}

impl Wizard {
    pub fn new(dry: bool) -> Wizard {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(7, |d| d.as_nanos() as u64);
        let mut w = Wizard {
            dry,
            screen: 0,
            seed,
            installed: Vec::new(),
            hcursor: 0,
            note: String::new(),
            home: 0,
            paths: ["~/codehead-home".into(), String::new(), String::new(), String::new()],
            preset: 1,
            crew: Vec::new(),
            focus: 0,
            launch: false,
            history: Vec::new(),
            quit: false,
            entered: Instant::now(),
        };
        w.detect();
        // a real run skips the harness screen when 1 harness is ready
        if !dry && !w.installed.is_empty() {
            w.screen = 1;
        }
        w.build_crew();
        w
    }

    fn rand(&mut self) -> u64 {
        // xorshift: 0 crate for a few picks
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    fn detect(&mut self) {
        self.installed = harnesses::installed().iter().map(|h| h.name).collect();
    }

    /// Harness rows: installed ones, then the offered ones not installed.
    fn harness_rows(&self) -> Vec<&'static harnesses::Harness> {
        let mut v: Vec<&'static harnesses::Harness> = self.installed.iter().filter_map(|n| harnesses::get(n)).collect();
        v.extend(OFFER.iter().filter(|n| !self.installed.contains(n)).filter_map(|n| harnesses::get(n)));
        v
    }

    fn random_avatar(&mut self) -> Avatar {
        let cat = if self.rand().is_multiple_of(3) { Cat::Geo } else { Cat::Regi };
        let idx = (self.rand() % cat.count() as u64) as usize;
        Avatar { cat, idx, ..Avatar::default() }
    }

    fn spawn(&self) -> Spawn {
        let taken: Vec<usize> = self.crew.iter().map(|m| m.spawn.anim).collect();
        Spawn::random_except(&taken)
    }

    /// The crew of the preset: names, avatars and colors new, harnesses from the installed ones
    /// (the reviewer on a 2nd harness when there is one).
    fn build_crew(&mut self) {
        let roles = PRESETS[self.preset].1;
        let first = self.installed.first().copied().unwrap_or("claude");
        let second = self.installed.get(1).copied().unwrap_or(first);
        let taken: Vec<String> = crate::engine::roster::all().into_iter().map(|a| a.name).collect();
        let mut names: Vec<&str> = NAMES.iter().copied().filter(|n| !taken.iter().any(|t| t == n)).collect();
        self.crew.clear();
        for (i, role) in roles.iter().enumerate() {
            let k = (self.rand() % names.len().max(1) as u64) as usize;
            let name = if names.is_empty() { format!("AGENT{i}") } else { names.remove(k).to_string() };
            let avatar = self.random_avatar();
            let color = crate::agents::COLORS[(i + self.preset) % 5];
            let harness = if *role == Role::Reviewer { second } else { first };
            let spawn = self.spawn();
            self.crew.push(Member { name, role: *role, harness, avatar, color, spawn });
        }
        self.focus = self.focus.min(self.crew.len() - 1);
    }

    /// Forward to screen TO (esc comes back here).
    fn go(&mut self, to: usize) {
        let to = to.min(SCREENS.len() - 1);
        if to != self.screen {
            self.history.push(self.screen);
        }
        // a fresh tutorial each time: the crew may be new
        self.launch = SCREENS[to] == Screen::Try;
        self.show(to);
    }

    /// Back to the screen before (0 before: quit).
    fn back(&mut self) {
        match self.history.pop() {
            Some(s) => self.show(s),
            None => self.quit = true,
        }
    }

    fn show(&mut self, to: usize) {
        self.screen = to;
        self.entered = Instant::now();
        self.note.clear();
        if SCREENS[self.screen] == Screen::Crew {
            for i in 0..self.crew.len() {
                self.crew[i].spawn = Spawn { anim: self.crew[i].spawn.anim, start: avatar::now() + i as f32 * 0.25 };
            }
        }
    }

    // ---------- tutorial ----------

    fn lead(&self) -> String {
        self.crew[0].name.clone()
    }

    /// The tutorial's spec: the crew, as `codehead tutorial` reads it.
    fn spec(&self) -> String {
        let crew = self.crew.iter().map(|m| crate::tutor::Member { name: m.name.clone(), role: m.role.name().into(), harness: m.harness.into(), avatar: m.avatar.id(), color: m.color.into() }).collect();
        serde_json::to_string(&crate::tutor::Spec { dry: self.dry, crew }).unwrap_or_default()
    }

    /// The tutorial ended with exit CODE: next screen, back, or quit.
    fn tutorial_ended(&mut self, code: Option<i32>) {
        match code {
            Some(crate::tutor::DONE) => self.go(self.screen + 1),
            Some(crate::tutor::QUIT) => self.quit = true,
            Some(crate::tutor::BACK) => self.back(),
            other => self.note = format!("tutorial: exit {other:?}"),
        }
    }

    // ---------- keys ----------

    fn key(&mut self, k: KeyEvent) {
        if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
            self.quit = true;
            return;
        }
        let scr = SCREENS[self.screen];
        match (scr, k.code) {
            // every screen, every state
            (_, KeyCode::Esc) => self.back(),
            (Screen::Harness, KeyCode::Up) => self.hcursor = self.hcursor.saturating_sub(1),
            (Screen::Harness, KeyCode::Down) => self.hcursor = (self.hcursor + 1).min(self.harness_rows().len().saturating_sub(1)),
            (Screen::Harness, KeyCode::Char('r')) => {
                self.detect();
                self.note = format!("{} ready", self.installed.len());
            }
            (Screen::Harness, KeyCode::Char('c')) => {
                if let Some(h) = self.harness_rows().get(self.hcursor) {
                    self.note = if crate::clip::copy(h.install) { format!("copied: {}", h.install) } else { "0 clipboard".into() };
                }
            }
            (Screen::Harness, KeyCode::Enter) => {
                if self.installed.is_empty() && !self.dry {
                    self.note = "install 1 harness first, then r".into();
                } else {
                    self.go(1);
                }
            }
            (Screen::Home, KeyCode::Up) => self.home = self.home.saturating_sub(1),
            (Screen::Home, KeyCode::Down) => self.home = (self.home + 1).min(HOME_CHOICES.len() - 1),
            (Screen::Home, KeyCode::Enter) => {
                let choice = HOME_CHOICES[self.home].0;
                if choice != HomeChoice::Skip && self.paths[self.home].trim().is_empty() {
                    self.note = if choice == HomeChoice::Clone { "type or paste the git URL".into() } else { "type a path".into() };
                    return;
                }
                // clone: its crew comes from home (the dry run shows the crew screen anyway)
                let skip_crew = choice == HomeChoice::Clone && !self.dry;
                self.go(if skip_crew { 3 } else { 2 });
            }
            (Screen::Home, _) if HOME_CHOICES[self.home].0 != HomeChoice::Skip => {
                edit(&mut self.paths[self.home], &k);
            }
            (Screen::Crew, KeyCode::Left) => {
                self.preset = (self.preset + PRESETS.len() - 1) % PRESETS.len();
                self.build_crew();
            }
            (Screen::Crew, KeyCode::Right) => {
                self.preset = (self.preset + 1) % PRESETS.len();
                self.build_crew();
            }
            (Screen::Crew, KeyCode::Tab) => self.focus = (self.focus + 1) % self.crew.len(),
            (Screen::Crew, KeyCode::BackTab) => self.focus = (self.focus + self.crew.len() - 1) % self.crew.len(),
            (Screen::Crew, KeyCode::Char(' ')) => {
                let a = self.random_avatar();
                let s = self.spawn();
                let m = &mut self.crew[self.focus];
                m.avatar = a;
                m.spawn = s;
            }
            (Screen::Crew, KeyCode::Enter) => {
                if let Some(why) = self.crew_problem() {
                    self.note = why;
                } else {
                    self.go(3);
                }
            }
            (Screen::Crew, _) => {
                let mut name = std::mem::take(&mut self.crew[self.focus].name);
                if edit(&mut name, &k) {
                    name = clean_name(&name);
                }
                self.crew[self.focus].name = name;
            }
            // the tutorial ended without its last step (or failed to start): again, or skip it
            (Screen::Try, KeyCode::Enter) => self.launch = true,
            (Screen::Try, KeyCode::Tab) => self.go(self.screen + 1),
            (Screen::Next, KeyCode::Enter) => self.quit = true,
            _ => {}
        }
    }

    /// Pasted TEXT (1st line) into the text of the screen: the home path, the agent's name, the
    /// tutorial prompt.
    fn paste(&mut self, text: &str) {
        let line = text.lines().next().unwrap_or_default().trim();
        match SCREENS[self.screen] {
            Screen::Home if HOME_CHOICES[self.home].0 != HomeChoice::Skip => self.paths[self.home].push_str(line),
            Screen::Crew => {
                let n = format!("{}{line}", self.crew[self.focus].name);
                self.crew[self.focus].name = clean_name(&n);
            }
            _ => {}
        }
    }

    /// Why the crew can't go on: an empty or a doubled name. None: fine.
    fn crew_problem(&self) -> Option<String> {
        for (i, m) in self.crew.iter().enumerate() {
            if m.name.is_empty() {
                return Some(format!("the {} needs a name", m.role.name()));
            }
            if self.crew[..i].iter().any(|o| o.name == m.name) {
                return Some(format!("2 agents named {}", m.name));
            }
        }
        None
    }

    // ---------- draw ----------

    fn draw(&self, f: &mut Frame) {
        let full = f.area();
        f.render_widget(ratatui::widgets::Clear, full);
        let w = COL.min(full.width.saturating_sub(4));
        let x = full.x + (full.width.saturating_sub(w)) / 2;
        let h = full.height.saturating_sub(2);
        let col = Rect { x, y: full.y + 1, width: w, height: h };
        let mut lines: Vec<Line> = Vec::new();
        lines.push(self.header(w));
        lines.push(Line::from(""));
        lines.push(Line::from(""));
        let (title, concept) = self.title();
        // tests read the screen: 0 decode
        let p = if cfg!(test) { 1.0 } else { (self.entered.elapsed().as_secs_f32() / 0.45).min(1.0) };
        lines.push(Line::from(avatar::decode(vec![Span::styled(format!("  {title}"), theme::accent_bold())], p, theme::accent(), theme::muted())));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(format!("  {concept}"), theme::text())));
        lines.push(Line::from(""));
        match SCREENS[self.screen] {
            Screen::Harness => self.draw_harness(&mut lines),
            Screen::Home => self.draw_home(&mut lines),
            Screen::Crew => self.draw_crew(&mut lines),
            Screen::Try => self.draw_try(&mut lines, w),
            Screen::Next => self.draw_next(&mut lines),
        }
        f.render_widget(Paragraph::new(lines), col);
        // footer: the keys (esc 1st: never cut), the note on its own row above
        let foot = Rect { x, y: full.y + full.height.saturating_sub(2), width: w, height: 1 };
        let mut spans = Vec::new();
        for (i, (k, what)) in self.keys().into_iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled("  ", theme::muted()));
            }
            spans.push(Span::styled(k, theme::bold()));
            spans.push(Span::styled(format!(" {what}"), theme::muted()));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), foot);
        if !self.note.is_empty() && foot.y > col.y {
            let row = Rect { y: foot.y - 1, ..foot };
            f.render_widget(Paragraph::new(Span::styled(format!("· {}", self.note), theme::fg(theme::pal().caution()))), row);
        }
    }

    fn header(&self, w: u16) -> Line<'static> {
        let mut spans = vec![Span::styled("  codehead", theme::bold())];
        if self.dry {
            spans.push(Span::styled("  dry run", theme::muted()));
        }
        let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let pad = (w as usize).saturating_sub(used + SCREENS.len() * 2 + 1);
        spans.push(Span::raw(" ".repeat(pad)));
        for i in 0..SCREENS.len() {
            let st = if i == self.screen { theme::accent() } else if i < self.screen { theme::text() } else { theme::muted() };
            spans.push(Span::styled(if i <= self.screen { "● " } else { "○ " }, st));
        }
        Line::from(spans)
    }

    fn title(&self) -> (&'static str, String) {
        match SCREENS[self.screen] {
            Screen::Harness => ("harnesses", "agents run on a harness. 1 needed.".into()),
            Screen::Home => ("home repo", "1 git repo: your crew, notes, settings. synced to every device.".into()),
            Screen::Crew => ("your crew", "agents = named teammates. @name calls 1 into a thread.".into()),
            Screen::Try => ("try it", "a real thread with your crew. it goes away after.".into()),
            Screen::Next => ("ready", "your crew waits on the start screen.".into()),
        }
    }

    fn keys(&self) -> Vec<(&'static str, &'static str)> {
        let esc = if self.history.is_empty() { ("esc", "quit") } else { ("esc", "back") };
        let mut v = match SCREENS[self.screen] {
            Screen::Harness => vec![("↑↓", "pick"), ("c", "copy install"), ("r", "check again"), ("enter", "next")],
            Screen::Home if HOME_CHOICES[self.home].0 == HomeChoice::Skip => vec![("↑↓", "pick"), ("enter", "next")],
            Screen::Home => vec![("↑↓", "pick"), ("type", "path"), ("enter", "next")],
            Screen::Crew => vec![("←→", "preset"), ("tab", "agent"), ("type", "name"), ("space", "avatar"), ("enter", "next")],
            Screen::Try => vec![("enter", "again"), ("tab", "skip")],
            Screen::Next => vec![("enter", "start screen")],
        };
        v.insert(0, esc);
        v
    }

    fn draw_harness(&self, lines: &mut Vec<Line<'static>>) {
        for (i, h) in self.harness_rows().iter().enumerate() {
            let ok = self.installed.contains(&h.name);
            let cur = i == self.hcursor;
            let mark = if ok { Span::styled("  ✓ ", Style::default().fg(theme::pal().success())) } else { Span::styled("  ✗ ", theme::muted()) };
            let label = Span::styled(format!("{:<14}", h.label), if cur { theme::accent_bold() } else { theme::text() });
            let what = if ok { Span::styled("ready", theme::muted()) } else { Span::styled(h.install.to_string(), if cur { theme::text() } else { theme::muted() }) };
            lines.push(Line::from(vec![Span::styled(if cur { "›" } else { " " }, theme::accent()), mark, label, what]));
        }
    }

    fn draw_home(&self, lines: &mut Vec<Line<'static>>) {
        for (i, (c, name, what)) in HOME_CHOICES.iter().enumerate() {
            let cur = i == self.home;
            lines.push(Line::from(vec![
                Span::styled(if cur { "  › " } else { "    " }, theme::accent()),
                Span::styled(format!("{name:<10}"), if cur { theme::accent_bold() } else { theme::text() }),
                Span::styled(what.to_string(), theme::muted()),
            ]));
            if cur && *c != HomeChoice::Skip {
                let hint = match c {
                    HomeChoice::Clone => "git URL",
                    _ => "path",
                };
                let text = self.paths[i].clone();
                let caret = Span::styled("▏", theme::accent());
                let row = if text.is_empty() { vec![caret, Span::styled(hint, theme::muted())] } else { vec![Span::styled(text, theme::text()), caret] };
                lines.push(Line::from([vec![Span::raw("              ")], row].concat()));
            }
        }
        lines.push(Line::from(""));
        let choice = HOME_CHOICES[self.home].0;
        if choice == HomeChoice::Skip {
            lines.push(Line::from(Span::styled("    agents + settings stay in ~/.config/codehead", theme::muted())));
            return;
        }
        lines.push(Line::from(Span::styled("    codehead writes:", theme::muted())));
        for (p, what) in [("codehead/agents/", "your crew"), ("codehead/settings.toml", "every device"), ("codehead/devices/", "1 file per device"), ("notes/", "plans + reports of agents")] {
            lines.push(Line::from(vec![Span::styled(format!("      {p:<24}"), theme::text()), Span::styled(what, theme::muted())]));
        }
        lines.push(Line::from(Span::styled("    commits + syncs by itself.", theme::muted())));
    }

    /// The 13x6 canvas of member M: the spawn while it runs, then the live sprite at SPAWN_AT.
    fn canvas(m: &Member, status: Status) -> [String; avatar::SPAWN_H] {
        if let Some(rows) = m.spawn.rows(&m.avatar, status) {
            return rows;
        }
        let live = m.avatar.rows(status, avatar::now());
        std::array::from_fn(|r| {
            let blank = " ".repeat(avatar::SPAWN_W);
            if r >= avatar::SPAWN_AT.1 && r < avatar::SPAWN_AT.1 + avatar::H {
                format!("{}{}{}", " ".repeat(avatar::SPAWN_AT.0), live[r - avatar::SPAWN_AT.1], " ".repeat(avatar::SPAWN_W - avatar::SPAWN_AT.0 - avatar::W))
            } else {
                blank
            }
        })
    }

    fn draw_crew(&self, lines: &mut Vec<Line<'static>>) {
        // presets row
        let mut row = vec![Span::raw("  ")];
        for (i, (name, _)) in PRESETS.iter().enumerate() {
            let st = if i == self.preset { theme::pill() } else { theme::muted() };
            row.push(Span::styled(format!(" {name} "), st));
            row.push(Span::raw("  "));
        }
        lines.push(Line::from(row));
        lines.push(Line::from(""));
        // the agents side by side: 1 column of 20 per agent
        const CW: usize = 22;
        let canvases: Vec<[String; avatar::SPAWN_H]> = self.crew.iter().map(|m| Self::canvas(m, Status::Idle)).collect();
        for r in 0..avatar::SPAWN_H {
            let mut spans = vec![Span::raw("  ")];
            for (i, m) in self.crew.iter().enumerate() {
                let c = Style::default().fg(crate::agents::color(Some(m.color)));
                spans.push(Span::styled(format!("   {}", canvases[i][r]), c));
                spans.push(Span::raw(" ".repeat(CW - 3 - avatar::SPAWN_W)));
            }
            lines.push(Line::from(spans));
        }
        let cell = |s: String, st: Style| vec![Span::styled(format!("{s:<CW$}"), st)];
        let mut names = vec![Span::raw("  ")];
        let mut roles = vec![Span::raw("  ")];
        let mut hs = vec![Span::raw("  ")];
        for (i, m) in self.crew.iter().enumerate() {
            let focused = i == self.focus;
            let c = Style::default().fg(crate::agents::color(Some(m.color))).add_modifier(Modifier::BOLD);
            let name = if focused { format!("{}▏", m.name) } else { m.name.clone() };
            let mark = if focused { "› " } else { "  " };
            names.extend(cell(format!("  {mark}{name}"), if focused { c } else { c.remove_modifier(Modifier::BOLD) }));
            roles.extend(cell(format!("    {}", m.role.name()), theme::text()));
            let model = crate::config::harness_opt(m.harness, "model").unwrap_or_else(|| "default".into());
            hs.extend(cell(format!("    {} · {model}", m.harness), theme::muted()));
        }
        lines.push(Line::from(names));
        lines.push(Line::from(roles));
        lines.push(Line::from(hs));
        lines.push(Line::from(""));
        let teach = match PRESETS[self.preset].0 {
            "solo" => "1 agent does it all.".to_string(),
            "pair" => format!("{} writes, {} checks. they talk on their own.", self.crew[0].name, self.crew[1].name),
            _ => format!("{} plans, {} writes, {} checks.", self.crew[0].name, self.crew[1].name, self.crew[2].name),
        };
        lines.push(Line::from(Span::styled(format!("  {teach}"), theme::muted())));
    }

    fn draw_try(&self, lines: &mut Vec<Line<'static>>, _w: u16) {
        let what = if self.note.is_empty() { "the tutorial ended before its last step." } else { "the tutorial did not start." };
        lines.push(Line::from(Span::styled(format!("  {what}"), theme::muted())));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled("  enter  ", theme::bold()), Span::styled("again", theme::text())]));
        lines.push(Line::from(vec![Span::styled("  tab    ", theme::bold()), Span::styled("skip it", theme::text())]));
    }

    fn draw_next(&self, lines: &mut Vec<Line<'static>>) {
        let lead = self.lead();
        for (k, what, hint) in [
            ("n ", "start 1st real thread", format!("{lead} takes it")),
            ("F3", "your home", "notes + plans land here".to_string()),
            ("? ", "hints again", "1 line per concept".to_string()),
        ] {
            lines.push(Line::from(vec![Span::styled(format!("  {k}  "), theme::bold()), Span::styled(format!("{what:<24}"), theme::text()), Span::styled(hint, theme::muted())]));
        }
    }

    /// What a real run does, 1 line each.
    pub fn plan(&self) -> Vec<String> {
        let mut out = Vec::new();
        let path = self.paths[self.home].clone();
        out.push(match HOME_CHOICES[self.home].0 {
            HomeChoice::New => format!("home: new {path} (git init, codehead home init, gh repo create --private when gh is logged in)"),
            HomeChoice::Clone => format!("home: git clone {path}, codehead home init, this device's file"),
            HomeChoice::Existing => format!("home: {path} (codehead home init: agents, flows, settings move in)"),
            HomeChoice::Skip => "home: 0 (agents + settings in ~/.config/codehead)".into(),
        });
        let crew: Vec<String> = self.crew.iter().map(|m| format!("{} ({}, {}, {})", m.name, m.role.name(), m.harness, m.avatar.id())).collect();
        out.push(format!("agents: {} → <home>/codehead/agents/", crew.join(", ")));
        out.push("tutorial: sandbox repo <data>/tutorial, 1 ephemeral thread, deleted at the end".into());
        out
    }
}

/// S as an agent name while it is typed: upper case, letters, digits, `-`, `_`, a letter 1st,
/// max. 32 (roster::valid_name once not empty).
fn clean_name(s: &str) -> String {
    let mut out: String = s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').map(|c| c.to_ascii_uppercase()).collect();
    while out.chars().next().is_some_and(|c| !c.is_ascii_alphabetic()) {
        out.remove(0);
    }
    out.truncate(32);
    out
}

/// `codehead setup [--dry]`.
pub fn run(dry: bool) -> ! {
    crate::restore_on_panic();
    let mut out = io::stdout();
    let mut tty = crate::Tty { enhanced: false };
    if let Err(e) = tty.enter(&mut out) {
        crate::fail(&format!("terminal: {e}"), 1);
    }
    let mut term = match ratatui::Terminal::new(crate::frames::backend()) {
        Ok(t) => t,
        Err(e) => {
            tty.leave(&mut out);
            crate::fail(&format!("terminal: {e}"), 1)
        }
    };
    let mut w = Wizard::new(dry);
    let result: io::Result<()> = (|| {
        while !w.quit {
            // the tutorial: its own process takes the terminal (a sandbox env of its own)
            if w.launch {
                w.launch = false;
                tty.release(&mut out);
                let child = std::process::Command::new(crate::engine::exe()).arg("tutorial").arg(w.spec()).stderr(std::process::Stdio::piped()).spawn();
                let (code, err) = match child {
                    Ok(c) => match c.wait_with_output() {
                        Ok(o) => (o.status.code(), String::from_utf8_lossy(&o.stderr).trim().to_string()),
                        Err(e) => (None, e.to_string()),
                    },
                    Err(e) => (None, e.to_string()),
                };
                tty.enter(&mut out)?;
                term.clear()?;
                if err.is_empty() {
                    w.tutorial_ended(code);
                } else {
                    w.note = err.lines().last().unwrap_or_default().to_string();
                }
                continue;
            }
            term.draw(|f| w.draw(f))?;
            if event::poll(Duration::from_millis(33))? {
                match event::read()? {
                    Event::Key(k) if k.kind != KeyEventKind::Release => w.key(k),
                    Event::Paste(s) => w.paste(&s),
                    _ => {}
                }
            }
        }
        Ok(())
    })();
    tty.leave(&mut out);
    if let Err(e) = result {
        crate::fail(&format!("terminal: {e}"), 1);
    }
    println!("{}", if dry { "dry run: 0 changes. a real run would:" } else { "setup: the writes are not built yet. a real run would:" });
    for l in w.plan() {
        println!("- {l}");
    }
    std::process::exit(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyModifiers as M;

    fn key(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, M::NONE)
    }

    fn text(w: &Wizard) -> String {
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 36)).unwrap();
        t.draw(|f| w.draw(f)).unwrap();
        t.backend().buffer().content().iter().map(|c| c.symbol()).collect::<Vec<_>>().chunks(90).map(|r| r.concat()).collect::<Vec<_>>().join("\n")
    }

    fn esc(w: &mut Wizard) {
        w.key(key(KeyCode::Esc));
    }

    fn typed(w: &mut Wizard, s: &str) {
        for c in s.chars() {
            w.key(key(KeyCode::Char(c)));
        }
    }

    #[test]
    fn dry_run_walks_every_screen_and_the_tutorial() {
        let mut w = Wizard::new(true);
        assert_eq!(w.screen, 0, "the dry run shows the harness screen");
        assert!(text(&w).contains("harnesses") && text(&w).contains("dry run"));
        w.key(key(KeyCode::Enter));
        assert!(text(&w).contains("home repo") && text(&w).contains("codehead/agents/"));
        w.key(key(KeyCode::Down));
        // clone: 0 URL → stays, says why
        w.key(key(KeyCode::Enter));
        assert_eq!(SCREENS[w.screen], Screen::Home);
        assert!(w.note.contains("git URL"));
        w.paste("git@github.com:me/home.git\nignored 2nd line");
        assert_eq!(w.paths[1], "git@github.com:me/home.git");
        w.key(key(KeyCode::Enter));
        assert_eq!(SCREENS[w.screen], Screen::Crew, "dry run: clone still shows the crew");
        assert_eq!(w.crew.len(), 2, "pair by default");
        w.key(key(KeyCode::Right));
        assert_eq!(w.crew.len(), 3);
        w.key(key(KeyCode::Left));
        // typing renames the focused agent at once
        w.key(KeyEvent::new(KeyCode::Char('u'), M::CONTROL));
        typed(&mut w, "1ada x");
        assert_eq!(w.crew[0].name, "ADAX", "letter 1st, upper case, 0 space");
        w.key(key(KeyCode::Enter));
        assert_eq!(SCREENS[w.screen], Screen::Try);
        assert!(w.launch, "the tutorial process starts next");

        // its spec: the crew as typed
        let spec: crate::tutor::Spec = serde_json::from_str(&w.spec()).unwrap();
        assert!(spec.dry && spec.crew[0].name == "ADAX" && spec.crew[1].role == "reviewer");

        // the tutorial ran through: the next screen
        w.launch = false;
        w.tutorial_ended(Some(crate::tutor::DONE));
        assert_eq!(SCREENS[w.screen], Screen::Next);
        assert!(w.plan().iter().any(|l| l.starts_with("home: git clone git@github.com:me/home.git")));
        w.key(key(KeyCode::Enter));
        assert!(w.quit);
    }

    #[test]
    fn esc_goes_back_1_screen_from_every_screen_and_state() {
        let mut w = Wizard::new(true);
        for i in 1..SCREENS.len() {
            w.go(i);
        }
        // the tutorial left early (esc esc in its thread): back, as esc; a typed name, a typed
        // path on the way: esc still goes back
        w.go(3);
        w.tutorial_ended(Some(crate::tutor::BACK));
        assert_eq!(SCREENS[w.screen], Screen::Next);
        w.go(3);
        let mut seen = vec![w.screen];
        while !w.quit {
            esc(&mut w);
            if !w.quit {
                seen.push(w.screen);
                if SCREENS[w.screen] == Screen::Crew {
                    typed(&mut w, "Q");
                }
                if SCREENS[w.screen] == Screen::Home {
                    typed(&mut w, "/x");
                }
            }
        }
        // 0 → 1 → 2 → 3 → 4 → (go 3 again) → back the same way
        assert_eq!(seen, vec![3, 4, 3, 2, 1, 0], "{seen:?}");
        assert!(w.quit, "esc on the 1st screen quits");
    }

    #[test]
    fn a_real_run_starting_on_home_quits_on_esc_and_skips_crew_on_clone() {
        let mut w = Wizard::new(false);
        if w.installed.is_empty() {
            return; // 0 harness on this machine: the harness screen comes 1st
        }
        assert_eq!(SCREENS[w.screen], Screen::Home);
        w.home = 1;
        w.paste("https://github.com/me/home.git");
        w.key(key(KeyCode::Enter));
        assert_eq!(SCREENS[w.screen], Screen::Try, "clone: the crew comes from home");
        esc(&mut w);
        assert_eq!(SCREENS[w.screen], Screen::Home, "back over the skipped screen");
        esc(&mut w);
        assert!(w.quit);
    }

    #[test]
    fn crew_needs_names_that_differ() {
        let mut w = Wizard::new(true);
        w.go(2);
        w.key(KeyEvent::new(KeyCode::Char('u'), M::CONTROL));
        w.key(key(KeyCode::Enter));
        assert_eq!(SCREENS[w.screen], Screen::Crew);
        assert!(w.note.contains("needs a name"), "{}", w.note);
        let other = w.crew[1].name.clone();
        typed(&mut w, &other);
        w.key(key(KeyCode::Enter));
        assert!(w.note.starts_with("2 agents named"), "{}", w.note);
        w.key(key(KeyCode::Backspace));
        w.key(key(KeyCode::Enter));
        assert_eq!(SCREENS[w.screen], Screen::Try);
        // space: a new avatar, the name stays
        let mut w = Wizard::new(true);
        w.go(2);
        let name = w.crew[0].name.clone();
        w.key(key(KeyCode::Char(' ')));
        assert_eq!(w.crew[0].name, name);
    }

    #[test]
    fn the_tutorial_screen_starts_again_skips_or_quits() {
        let mut w = Wizard::new(true);
        w.go(3);
        w.launch = false;

        // a start that failed: the reason stays on the screen, enter starts again
        w.tutorial_ended(None);
        assert!(w.note.starts_with("tutorial: exit") && text(&w).contains("did not start"));
        w.key(key(KeyCode::Enter));
        assert!(w.launch);

        // tab: skip it
        w.key(key(KeyCode::Tab));
        assert_eq!(SCREENS[w.screen], Screen::Next);

        // ctrl+c ctrl+c in the tutorial's thread: the wizard ends too
        w.go(3);
        w.tutorial_ended(Some(crate::tutor::QUIT));
        assert!(w.quit);
    }
}

#[cfg(test)]
mod look {
    use super::*;

    /// `cargo test -- --ignored wizard::look --nocapture`: every screen as text.
    #[test]
    #[ignore]
    fn print_screens() {
        let mut w = Wizard::new(true);
        let show = |w: &Wizard| {
            let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(84, 26)).unwrap();
            t.draw(|f| w.draw(f)).unwrap();
            let rows: Vec<String> = t.backend().buffer().content().iter().map(|c| c.symbol().to_string()).collect::<Vec<_>>().chunks(84).map(|r| r.concat().trim_end().to_string()).collect();
            println!("{}\n{}", rows.join("\n"), "=".repeat(84));
        };
        for i in 0..SCREENS.len() {
            w.go(i);
            // the spawn over: the sprites, not the goo
            w.crew.iter_mut().for_each(|m| m.spawn.start = avatar::now() - 1000.0);
            show(&w);
        }
    }
}
