//! Thread view: the thread's agents on top, the stream (every message, tool run and open
//! question of the thread, from its event log), the prompt below, the cost box beside it.
//! Messages go to the thread's host (`engine::client`); the host starts on the first one.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;
use serde_json::{Value, json};

use crate::attach::{Pasted, Paster};
use crate::engine::{client, store};
use crate::mention::Mention;
use crate::stream::{self, Hit};
use crate::textarea::{Outcome, TextArea};
use crate::theme;

pub enum Exit {
    /// ctrl+c twice: codehead closes.
    Quit,
    /// Menu: a tab of the start screen.
    Home(crate::menu::Home),
    /// Esc esc: back to the start screen.
    Back,
    /// Another thread's view (menu, `> branch`).
    Open { id: String, dir: PathBuf, title: String, note: String },
}

/// Thread header open (tab toggles it; kept across the thread switches it makes).
static NAV_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A thread view from the start screen: header closed.
pub fn reset_nav() {
    NAV_ON.store(false, std::sync::atomic::Ordering::Relaxed);
}

/// 2 Esc presses within this time: back to the start screen (the 1st one interrupts).
const DOUBLE_ESC: Duration = Duration::from_millis(500);
/// Most rows the prompt grows to; longer text scrolls.
const INPUT_ROWS: u16 = 8;
/// The cost box: shown when the screen is this wide.
const COST_MIN_SCREEN: u16 = 100;
const COST_W: u16 = 44;
/// Handoff to a replacing agent: the replaced agent's last messages, max. this many chars.
const HANDOFF_CHARS: usize = 12_000;
const WAVE: [&str; 4] = ["░▒▓", "▒▓▒", "▓▒░", "▒░▒"];

enum Msg {
    /// A command to the host ended: what it was, Err(why).
    Done(&'static str, Result<Value, String>),
    Cost(Box<crate::cost::Snap>),
}

struct View {
    id: String,
    title: String,
    cwd: PathBuf,
    tail: store::Tail,
    thread: stream::Thread,
    roster: Vec<crate::agents::Agent>,
    members: Vec<String>,
    colors: stream::Colors,
    names: Vec<(String, Color)>,
    /// Agents of the roster: (name, harness), for the hints of the stream.
    harness_of: Vec<(String, String)>,
    // stream
    drawn: stream::Drawn,
    dirty: bool,
    width: u16,
    /// Rows scrolled up from the bottom (0: live).
    back: usize,
    open: HashSet<usize>,
    all_runs: bool,
    stream_rect: Rect,
    /// Drag selection in the stream: (row, col) of its start and end, in drawn rows.
    sel: Option<((usize, u16), (usize, u16))>,
    dragging: bool,
    /// Last click in the stream: time, (row, col), count (2: word, 3: row).
    clicks: Option<(Instant, (usize, u16), u8)>,
    /// Mouse position: the clickable row under it gets the hover bg.
    hover: Option<Position>,
    // ask
    ask_at: usize,
    picks: HashSet<usize>,
    /// Answers so far of a multi-question ask: question → label.
    answers: Vec<(String, String)>,
    // prompt
    input: TextArea,
    input_rect: Rect,
    mention: Mention,
    command: crate::command::Command,
    paths: crate::paths::Paths,
    paster: Paster,
    attachments: Vec<(String, PathBuf)>,
    history: Vec<String>,
    hist_at: Option<usize>,
    draft: String,
    /// `@new` picked with "replace OLD": on send, OLD leaves and NEW gets the handoff.
    replace: Option<(String, String)>,
    /// A replace waiting for OLD's turn to end: (new, old, message).
    queued: Option<(String, String, String)>,
    /// Sent messages the log does not show yet: text, sent at, error.
    pending: Vec<(String, Instant, Option<String>)>,
    // thread header: cards of the open threads (tab)
    nav_on: bool,
    nav_cards: Vec<crate::nav::Card>,
    nav_at: usize,
    nav_rects: Vec<(Rect, usize)>,
    nav_read: Option<Receiver<Vec<crate::nav::Card>>>,
    nav_time: Instant,
    // chrome
    monitors: crate::monitors::Monitors,
    menu: Option<crate::menu::Menu>,
    cost: Option<crate::cost::Snap>,
    cost_at: Instant,
    toast: Option<(String, Instant)>,
    started: Instant,
    last_esc: Option<Instant>,
    last_ctrl_c: Option<Instant>,
    exit: Exit,
    /// The header switched threads: the view ends with EXIT.
    nav_switch: bool,
    /// A click on a card switched threads.
    nav_click: bool,
    /// Where the drag started (drawn row, col).
    drag_from: Option<(usize, u16)>,
    tx: Sender<Msg>,
}

impl View {
    fn new(id: &str, cwd: &Path, title: &str, tx: Sender<Msg>) -> View {
        let roster = crate::agents::load();
        let mut input = TextArea::new(false, "message the thread · @name: an agent · ⌘/ctrl+⏎ sends");
        input.style = Some(theme::text());
        let mut command = crate::command::Command::new(cwd);
        command.can_branch = true;
        command.harnesses = roster.iter().map(|a| a.harness.clone()).collect();
        let mut v = View {
            id: id.to_string(),
            title: title.lines().next().unwrap_or_default().to_string(),
            cwd: cwd.to_path_buf(),
            tail: store::Tail::new(id),
            thread: stream::Thread::default(),
            roster: Vec::new(),
            members: Vec::new(),
            colors: HashMap::new(),
            names: Vec::new(),
            harness_of: Vec::new(),
            drawn: stream::Drawn::default(),
            dirty: true,
            width: 0,
            back: 0,
            open: HashSet::new(),
            all_runs: false,
            stream_rect: Rect::default(),
            sel: None,
            dragging: false,
            clicks: None,
            hover: None,
            ask_at: 0,
            picks: HashSet::new(),
            answers: Vec::new(),
            input,
            input_rect: Rect::default(),
            mention: Mention::default(),
            command,
            paths: crate::paths::Paths::new(cwd),
            paster: Paster::new(crate::engine::data_dir().join("paste").join(id)),
            attachments: Vec::new(),
            history: Vec::new(),
            hist_at: None,
            draft: String::new(),
            replace: None,
            queued: None,
            pending: Vec::new(),
            nav_on: NAV_ON.load(std::sync::atomic::Ordering::Relaxed),
            nav_cards: Vec::new(),
            nav_at: 0,
            nav_rects: Vec::new(),
            nav_read: None,
            nav_time: Instant::now() - Duration::from_secs(60),
            monitors: crate::monitors::Monitors::new(id),
            menu: None,
            cost: None,
            cost_at: Instant::now() - Duration::from_secs(60),
            toast: None,
            started: Instant::now(),
            last_esc: None,
            last_ctrl_c: None,
            exit: Exit::Back,
            nav_switch: false,
            nav_click: false,
            drag_from: None,
            tx,
        };
        v.set_roster(roster);
        v.read();
        v
    }

    fn set_roster(&mut self, roster: Vec<crate::agents::Agent>) {
        self.colors = roster.iter().map(|a| (a.name.clone(), crate::agents::color(a.color.as_deref()))).collect();
        self.names = roster.iter().map(|a| (a.name.clone(), crate::agents::color(a.color.as_deref()))).collect();
        self.harness_of = roster.iter().map(|a| (a.name.clone(), a.harness.clone())).collect();
        self.input.marks = self.names.clone();
        self.roster = roster;
        self.members = store::load(&self.id).map(|t| t.agents).unwrap_or_default();
        self.mention.replaceable = self.members.clone();
        self.dirty = true;
    }

    /// New events of the log. True: some came.
    fn read(&mut self) -> bool {
        let evs = self.tail.read();
        if evs.is_empty() {
            return false;
        }
        let asks = self.thread.asks.first().map(|a| a.id.clone());
        for e in &evs {
            self.thread.push(e);
            if let store::Kind::Prompt { from, text, .. } = &e.kind
                && from == "owner"
            {
                self.pending.retain(|(t, _, _)| !text.starts_with(t.as_str()) && !t.starts_with(text.as_str()));
            }
            if let (store::Kind::TurnEnd { .. }, Some((_, old, _))) = (&e.kind, &self.queued)
                && e.agent.as_deref() == Some(old.as_str())
                && let Some((new, old, text)) = self.queued.take()
            {
                self.run_replace(new, old, text);
            }
            if matches!(e.kind, store::Kind::Join | store::Kind::Leave) {
                self.members = store::load(&self.id).map(|t| t.agents).unwrap_or_default();
                self.mention.replaceable = self.members.clone();
            }
        }
        if self.thread.asks.first().map(|a| a.id.clone()) != asks {
            self.ask_at = 0;
            self.picks.clear();
            self.answers.clear();
        }
        self.history = self.thread.sent();
        self.dirty = true;
        true
    }

    fn toast(&mut self, m: impl Into<String>) {
        self.toast = Some((m.into(), Instant::now()));
    }

    /// Command C to the host, off the UI thread; the reply comes back as Msg::Done(WHAT).
    fn host(&self, what: &'static str, c: Value) {
        let (id, tx) = (self.id.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Done(what, client::call(&id, &c)));
        });
    }

    // ---------- send ----------

    fn submit(&mut self) {
        let mut text = self.input.text().trim_end().to_string();
        if text.trim().is_empty() {
            return;
        }
        let files: Vec<String> = self.attachments.iter().filter(|(m, _)| text.contains(m.as_str())).map(|(_, p)| format!("- {}", p.display())).collect();
        if !files.is_empty() {
            text = format!("{text}\n\n{}\n{}", stream::ATTACHMENTS, files.join("\n"));
        }
        self.attachments.clear();
        self.input.set_text("");
        self.hist_at = None;
        self.back = 0;
        if let Some((new, old)) = self.replace.take()
            && text.contains(&format!("@{new}"))
        {
            // OLD mid-turn: the replace waits for its turn to end (esc interrupts it)
            if self.thread.busy.contains(&old) {
                self.toast(format!("{new} replaces {old} after {old}'s turn · esc: interrupt {old}"));
                self.queued = Some((new, old, text));
            } else {
                self.run_replace(new, old, text);
            }
            return;
        }
        self.pending.push((text.clone(), Instant::now(), None));
        self.host("send", json!({"op": "prompt", "text": text}));
    }

    /// NEW takes over from OLD: a handoff file (task, recent messages, changed files), OLD
    /// leaves, NEW gets TEXT with the file's path.
    fn run_replace(&mut self, new: String, old: String, text: String) {
        let convo = self.thread.last_of(&old, HANDOFF_CHARS);
        let task = store::load(&self.id).map(|t| t.task).unwrap_or_default();
        let changed = crate::threads::git(&self.cwd, &["status", "--short"]).1;
        let changed: String = changed.lines().take(80).map(|l| format!("{l}\n")).collect();
        let dir = crate::config::home().join(".cache/codehead/handoff");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join(format!("{}-{}-{new}.md", &self.id[..self.id.len().min(12)], crate::engine::now_ms() / 1000));
        let doc = format!(
            "# Handoff: {old} → {new}\n\n{old} worked on this thread before you. It left the thread; you take over.\n\n## Task of the thread\n\n{task}\n\n## Recent messages of {old}\n\n{convo}\n\n## Changed files (git status --short in {})\n\n```\n{}```\n",
            self.cwd.display(),
            if changed.is_empty() { "0 changes\n".to_string() } else { changed }
        );
        if let Err(e) = std::fs::write(&file, doc) {
            self.toast(format!("handoff file: {e}"));
            return;
        }
        let body = text.strip_prefix(&format!("@{new}")).map(str::trim_start).unwrap_or(&text).to_string();
        let prompt = format!("{body}\n\n(You take over this thread from {old}. Read {} first: the task, the recent messages, the changed files.)", file.display());
        let (id, tx) = (self.id.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let r = client::call(&id, &json!({"op": "leave", "agent": old})).and_then(|_| client::call(&id, &json!({"op": "prompt", "to": [new], "text": prompt})));
            let _ = tx.send(Msg::Done("replace", r));
        });
        self.toast("replacing…");
    }

    /// Plain text of the selection.
    fn selected(&self) -> Option<String> {
        let ((r0, c0), (r1, c1)) = self.sel?;
        let ((r0, c0), (r1, c1)) = if (r0, c0) <= (r1, c1) { ((r0, c0), (r1, c1)) } else { ((r1, c1), (r0, c0)) };
        let mut out = Vec::new();
        for r in r0..=r1.min(self.drawn.rows.len().saturating_sub(1)) {
            let text: String = self.drawn.rows[r].line.spans.iter().map(|s| s.content.as_ref()).collect();
            let chars: Vec<char> = text.chars().collect();
            let (a, b) = (if r == r0 { usize::from(c0) } else { 0 }, if r == r1 { usize::from(c1) + 1 } else { chars.len() });
            let piece: String = chars.get(a.min(chars.len())..b.min(chars.len())).map(|c| c.iter().collect()).unwrap_or_default();
            out.push(unframe(piece.trim_end()));
        }
        let t = out.join("\n");
        (!t.trim().is_empty()).then_some(t)
    }

    fn copy(&mut self, text: &str) {
        let ok = crate::clip::copy(text);
        self.toast(if ok { format!("copied {} chars", text.chars().count()) } else { "0 clipboard tools found (wl-copy, xclip, xsel)".into() });
    }

    fn interrupt(&mut self) {
        if self.thread.busy.is_empty() && self.queued.is_none() {
            return;
        }
        if let Some((new, old, _)) = self.queued.take() {
            self.toast(format!("replace {old} → {new}: dropped"));
        }
        self.host("interrupt", json!({"op": "interrupt"}));
        self.toast("interrupt…");
    }

    /// Answer the open ask with choice I (multi: the picks).
    fn answer(&mut self, i: Option<usize>) {
        let Some(a) = self.thread.asks.first().cloned() else { return };
        let qs = a.questions();
        let own = a.options();
        let c = if qs.is_empty() && !own.is_empty() {
            let Some((id, name, kind)) = own.get(i.unwrap_or(self.ask_at)).cloned() else { return };
            json!({"op": "answer", "id": a.id, "allow": kind.starts_with("allow"), "option": id, "label": name})
        } else if qs.is_empty() {
            let allow = i.unwrap_or(self.ask_at) == 0;
            json!({"op": "answer", "id": a.id, "allow": allow, "label": if allow { "allow" } else { "deny" }})
        } else {
            let Some((q, _, opts, multi)) = qs.get(self.answers.len()) else { return };
            let label = if *multi && i.is_none() && !self.picks.is_empty() {
                let mut p: Vec<usize> = self.picks.iter().copied().collect();
                p.sort();
                p.iter().filter_map(|&k| opts.get(k).map(|o| o.0.clone())).collect::<Vec<_>>().join(", ")
            } else {
                match opts.get(i.unwrap_or(self.ask_at)) {
                    Some(o) => o.0.clone(),
                    None => return,
                }
            };
            self.answers.push((q.clone(), label));
            self.ask_at = 0;
            self.picks.clear();
            self.dirty = true;
            if self.answers.len() < qs.len() {
                return;
            }
            let answers: serde_json::Map<String, Value> = self.answers.iter().map(|(q, l)| (q.clone(), Value::String(l.clone()))).collect();
            let label = self.answers.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>().join(" · ");
            json!({"op": "answer", "id": a.id, "allow": true, "answers": answers, "label": label})
        };
        self.host("answer", c);
    }

    fn choices(&self) -> usize {
        let Some(a) = self.thread.asks.first() else { return 0 };
        match a.questions().get(self.answers.len()) {
            Some((_, _, o, _)) => o.len(),
            None => a.options().len().max(2),
        }
    }

    fn multi(&self) -> bool {
        self.thread.asks.first().and_then(|a| a.questions().get(self.answers.len()).map(|q| q.3)).unwrap_or(false)
    }

    /// `> branch`: a copy of the thread, its view next.
    fn branch(&mut self) -> bool {
        if !self.thread.busy.is_empty() {
            self.toast("branch: wait for the turn to end");
            return false;
        }
        match store::fork(&self.id) {
            Ok(t) => {
                self.exit = Exit::Open { id: t.id.clone(), dir: t.work_dir(), title: t.title.clone(), note: format!("branched from {}", &self.id[..self.id.len().min(12)]) };
                true
            }
            Err(e) => {
                self.toast(format!("branch: {e}"));
                false
            }
        }
    }

    fn paste(&mut self, text: &str) {
        match self.paster.paste(text) {
            Pasted::Text(t) => self.input.insert(&t),
            Pasted::File { path, marker } => {
                self.input.insert(&marker);
                self.attachments.push((marker, path));
            }
            Pasted::Nothing => self.toast("clipboard: 0 image, 0 text"),
        }
    }

    fn history_step(&mut self, up: bool) {
        if self.history.is_empty() {
            return;
        }
        let at = match (self.hist_at, up) {
            (None, true) => {
                self.draft = self.input.text();
                Some(self.history.len() - 1)
            }
            (None, false) => return,
            (Some(0), true) => Some(0),
            (Some(i), true) => Some(i - 1),
            (Some(i), false) if i + 1 < self.history.len() => Some(i + 1),
            (Some(_), false) => None,
        };
        self.hist_at = at;
        let text = at.map(|i| self.history[i].clone()).unwrap_or_else(|| self.draft.clone());
        self.input.load_text(&text);
    }

    // ---------- keys ----------

    /// True: the view ends (EXIT set).
    fn key(&mut self, k: KeyEvent) -> bool {
        if let Some(m) = self.menu.as_mut() {
            let a = m.key(k);
            return self.menu_action(a);
        }
        if self.monitors.key(k) {
            if let Some(n) = self.monitors.take_note() {
                self.toast(n);
            }
            return false;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        // thread header open: it owns ←→, enter, esc
        if self.nav_on && self.nav_key(k) {
            return self.nav_switch || (k.code == KeyCode::Enter && !self.nav_on);
        }
        // dropdowns first: they own ↑↓, tab, enter, esc while open
        if self.mention.key(k, &mut self.input, &self.names) {
            if let Some((new, Some(old))) = self.mention.picked.take() {
                self.toast(format!("on send: {new} replaces {old}"));
                self.replace = Some((new, old));
            }
            return false;
        }
        if self.command.key(k, &mut self.input) {
            if self.command.action.take() == Some(crate::command::Action::Branch) {
                return self.branch();
            }
            return false;
        }
        if self.paths.key(k, &mut self.input) {
            return false;
        }
        let empty = self.input.is_empty();
        let asking = !self.thread.asks.is_empty();
        match k.code {
            KeyCode::F(2) => {
                self.open_menu();
                return false;
            }
            KeyCode::Esc => {
                if self.last_esc.is_some_and(|t| t.elapsed() < DOUBLE_ESC) {
                    self.exit = Exit::Back;
                    return true;
                }
                self.last_esc = Some(Instant::now());
                if self.sel.take().is_some() {
                } else if self.back > 0 {
                    self.back = 0;
                } else {
                    self.interrupt();
                }
                return false;
            }
            KeyCode::Tab if empty && !self.mention_open() => {
                self.set_nav(!self.nav_on);
                return false;
            }
            KeyCode::Char('c') if ctrl && self.sel.is_some() => {
                if let Some(t) = self.selected() {
                    self.copy(&t);
                }
                self.sel = None;
                return false;
            }
            KeyCode::Char('c') if ctrl => {
                if !empty {
                    self.input.set_text("");
                    return false;
                }
                if self.last_ctrl_c.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) {
                    self.exit = Exit::Quit;
                    return true;
                }
                self.last_ctrl_c = Some(Instant::now());
                self.toast("ctrl+c again: quit");
                return false;
            }
            KeyCode::Char('o') if ctrl => {
                self.all_runs = !self.all_runs;
                self.open.clear();
                self.dirty = true;
                return false;
            }
            KeyCode::Char('v') if ctrl => {
                let p = self.paster.clipboard();
                match p {
                    Pasted::Text(t) => self.input.insert(&t),
                    Pasted::File { path, marker } => {
                        self.input.insert(&marker);
                        self.attachments.push((marker, path));
                    }
                    Pasted::Nothing => self.toast("clipboard: 0 image, 0 text"),
                }
                return false;
            }
            KeyCode::Char('i') if ctrl || k.modifiers.contains(KeyModifiers::SUPER) => {
                self.open_vscode();
                return false;
            }
            KeyCode::Enter if ctrl || k.modifiers.contains(KeyModifiers::ALT) || k.modifiers.contains(KeyModifiers::SUPER) => {
                self.submit();
                return false;
            }
            KeyCode::Char('s') if ctrl => {
                self.submit();
                return false;
            }
            KeyCode::PageUp => {
                self.scroll(self.stream_rect.height.saturating_sub(2) as isize);
                return false;
            }
            KeyCode::PageDown => {
                self.scroll(-(self.stream_rect.height.saturating_sub(2) as isize));
                return false;
            }
            KeyCode::Up | KeyCode::Down if k.modifiers.contains(KeyModifiers::SHIFT) => {
                self.scroll(if k.code == KeyCode::Up { 3 } else { -3 });
                return false;
            }
            // an open ask: an empty prompt's ↑↓ / digits / space / enter answer it
            KeyCode::Up if asking && empty => {
                self.ask_at = self.ask_at.saturating_sub(1);
                self.dirty = true;
                return false;
            }
            KeyCode::Down if asking && empty => {
                self.ask_at = (self.ask_at + 1).min(self.choices().saturating_sub(1));
                self.dirty = true;
                return false;
            }
            KeyCode::Char(' ') if asking && empty && self.multi() => {
                if !self.picks.remove(&self.ask_at) {
                    self.picks.insert(self.ask_at);
                }
                self.dirty = true;
                return false;
            }
            KeyCode::Char(c @ '1'..='9') if asking && empty => {
                let i = c as usize - '1' as usize;
                if i < self.choices() {
                    self.ask_at = i;
                    if self.multi() {
                        if !self.picks.remove(&i) {
                            self.picks.insert(i);
                        }
                        self.dirty = true;
                    } else {
                        self.answer(Some(i));
                    }
                }
                return false;
            }
            KeyCode::Enter if asking && empty => {
                self.answer(None);
                return false;
            }
            KeyCode::Up if self.input.on_first_row() && (empty || self.hist_at.is_some()) => {
                self.history_step(true);
                return false;
            }
            KeyCode::Down if self.hist_at.is_some() && self.input.on_last_row() => {
                self.history_step(false);
                return false;
            }
            _ => {}
        }
        // enter on an empty prompt: 0 empty lines
        if k.code == KeyCode::Enter && empty {
            return false;
        }
        if let Outcome::Handled = self.input.key(k) {
            self.hist_at = None;
        } else if k.code == KeyCode::Enter {
            self.input.newline();
        }
        false
    }

    fn mention_open(&self) -> bool {
        self.mention.is_open(&self.input, &self.names)
    }

    fn open_vscode(&mut self) {
        let Some(cli) = vscode_cli() else {
            self.toast("VS Code: 0 CLI found (code, codium, code-insiders; $CODEHEAD_VSCODE)");
            return;
        };
        let ok = std::process::Command::new(&cli).arg("-n").arg(&self.cwd).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().is_ok();
        self.toast(if ok { format!("VS Code: {}", self.cwd.display()) } else { format!("VS Code: {} did not start", cli.display()) });
    }

    // ---------- thread header ----------

    fn set_nav(&mut self, on: bool) {
        self.nav_on = on;
        NAV_ON.store(on, std::sync::atomic::Ordering::Relaxed);
        if on {
            self.load_nav(true);
            self.nav_at = self.nav_cards.iter().position(|c| c.id == self.id).unwrap_or(0);
        }
    }

    /// Thread cards, read in the background every 3s (NOW: at once).
    fn load_nav(&mut self, now: bool) {
        if let Some(rx) = &self.nav_read
            && let Ok(cards) = rx.try_recv()
        {
            self.nav_cards = cards.into_iter().filter(|c| c.state == "open" || c.id == self.id).collect();
            self.nav_read = None;
        }
        if self.nav_read.is_none() && (now || self.nav_time.elapsed() > Duration::from_secs(3)) {
            self.nav_time = Instant::now();
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(crate::nav::load());
            });
            self.nav_read = Some(rx);
        }
    }

    /// A key on the open header. True: used.
    fn nav_key(&mut self, k: KeyEvent) -> bool {
        match k.code {
            KeyCode::Left => self.nav_at = self.nav_at.saturating_sub(1),
            KeyCode::Right => self.nav_at = (self.nav_at + 1).min(self.nav_cards.len().saturating_sub(1)),
            KeyCode::Enter => return self.nav_open(self.nav_at),
            KeyCode::Esc | KeyCode::Tab | KeyCode::Up | KeyCode::Down => self.set_nav(false),
            _ => return false,
        }
        true
    }

    /// Switch to card I. True: the view ends.
    fn nav_open(&mut self, i: usize) -> bool {
        let Some(c) = self.nav_cards.get(i).cloned() else { return false };
        if c.id == self.id {
            self.set_nav(false);
            return true;
        }
        self.nav_switch = true;
        self.exit = Exit::Open { id: c.id.clone(), dir: c.dir.clone().unwrap_or_else(|| self.cwd.clone()), title: c.title, note: String::new() };
        true
    }

    fn draw_nav(&mut self, f: &mut Frame, area: Rect) {
        self.nav_rects.clear();
        let tick = self.tick();
        let mut x = area.x + 1;
        let w = 32u16;
        let start = self.nav_at.saturating_sub(usize::from(area.width / (w + 1)).saturating_sub(1));
        for (i, c) in self.nav_cards.iter().enumerate().skip(start) {
            if x + w > area.right() {
                break;
            }
            let r = Rect { x, y: area.y, width: w, height: crate::nav::CARD_H };
            crate::nav::draw_card(f.buffer_mut(), r, c, i == self.nav_at, c.id == self.id, true, tick);
            self.nav_rects.push((r, i));
            x += w + 1;
        }
        if self.nav_cards.is_empty() {
            f.render_widget(Paragraph::new(Line::styled("  reading threads…", theme::muted())), area);
        }
    }

    fn scroll(&mut self, delta: isize) {
        let max = self.drawn.rows.len().saturating_sub(usize::from(self.stream_rect.height));
        self.back = (self.back as isize + delta).clamp(0, max as isize) as usize;
    }

    fn open_menu(&mut self) {
        use crate::menu::Item;
        let items = vec![
            Item { label: format!("thread header: {}", if self.nav_on { "close" } else { "open" }), key: "tab".into(), id: 0 },
            Item { label: "tool calls: open / close all".into(), key: "ctrl+o".into(), id: 1 },
            Item { label: "VS Code on the worktree".into(), key: "ctrl+i".into(), id: 2 },
            Item { label: "back to the start screen".into(), key: "esc esc".into(), id: 3 },
            Item { label: "agents: mention, add (←→: +), replace (⇄)".into(), key: "@".into(), id: 4 },
            Item { label: "commands: files, notes, threads, skills, branch".into(), key: ">".into(), id: 5 },
            Item { label: "paste a clipboard image".into(), key: "ctrl+v".into(), id: 7 },
            Item { label: "select + copy · double click: word · click code: copy".into(), key: "drag".into(), id: 9 },
            Item { label: "scroll the stream".into(), key: "wheel · pgup".into(), id: 10 },
            Item { label: "send · new line".into(), key: "⌘/ctrl+⏎ · ⏎".into(), id: 11 },
            Item { label: "interrupt the agents".into(), key: "esc".into(), id: 12 },
        ];
        let cards = crate::nav::load();
        self.menu = Some(crate::menu::Menu::new(items, Some(self.id.clone()), self.members.clone(), self.roster.clone(), cards));
    }

    fn menu_action(&mut self, a: crate::menu::Action) -> bool {
        use crate::menu::Action;
        match a {
            Action::None => return false,
            Action::Close => {}
            Action::Home(h) => {
                self.exit = Exit::Home(h);
                return true;
            }
            Action::Open(c) => {
                self.menu = None;
                if c.id == self.id {
                    return false;
                }
                self.exit = Exit::Open { id: c.id.clone(), dir: c.dir.clone().unwrap_or_else(|| self.cwd.clone()), title: c.title, note: String::new() };
                return true;
            }
            Action::Command(0) => {
                let on = !self.nav_on;
                self.set_nav(on);
            }
            Action::Command(1) => {
                self.all_runs = !self.all_runs;
                self.dirty = true;
            }
            Action::Command(2) => {
                self.menu = None;
                return self.key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL));
            }
            Action::Command(3) => {
                self.exit = Exit::Back;
                return true;
            }
            Action::Command(id @ (4 | 5)) => {
                self.menu = None;
                self.input.move_end();
                let text = self.input.text();
                let gap = if text.is_empty() || text.ends_with(char::is_whitespace) { "" } else { " " };
                self.input.insert(&format!("{gap}{}", if id == 4 { "@" } else { ">" }));
                return false;
            }
            Action::Command(7) => {
                self.menu = None;
                return self.key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));
            }
            Action::Command(12) => self.interrupt(),
            Action::Command(_) => {}
            Action::Remove(name) => {
                self.host("leave", json!({"op": "leave", "agent": name}));
                self.toast(format!("{name} leaves…"));
                return false;
            }
            Action::Add(name) => {
                self.host("join", json!({"op": "join", "agent": name}));
                self.toast(format!("{name} joins…"));
                return false;
            }
        }
        self.menu = None;
        false
    }

    fn mouse(&mut self, m: MouseEvent) {
        if let Some(menu) = self.menu.as_mut() {
            let a = menu.mouse(m);
            if self.menu_action(a) {
                self.menu = None;
            }
            return;
        }
        if self.monitors.mouse(m)
            || self.mention.mouse(m, &mut self.input, &self.names)
            || self.command.mouse(m, &mut self.input)
            || self.paths.mouse(m, &mut self.input)
        {
            return;
        }
        let at = Position { x: m.column, y: m.row };
        self.hover = Some(at);
        // the thread header's cards
        if self.nav_on && matches!(m.kind, MouseEventKind::Down(MouseButton::Left))
            && let Some(&(_, i)) = self.nav_rects.iter().find(|(r, _)| r.contains(at))
        {
            if self.nav_open(i) {
                self.nav_click = true;
            }
            return;
        }
        let in_stream = self.stream_rect.contains(at);
        match m.kind {
            MouseEventKind::ScrollUp if in_stream => self.scroll(3),
            MouseEventKind::ScrollDown if in_stream => self.scroll(-3),
            MouseEventKind::Down(MouseButton::Left) if in_stream => {
                let Some(row) = self.row_at(at.y) else {
                    self.sel = None;
                    return;
                };
                let col = at.x - self.stream_rect.x;
                let r = &self.drawn.rows[row];
                if let Some(url) = r.links.iter().find(|(a, b, _)| col >= *a && col < *b).map(|l| l.2.clone()) {
                    open_url(&url);
                    self.toast(format!("open {url}"));
                    return;
                }
                if let Some(t) = r.copies.iter().find(|(a, b, _)| col >= *a && col < *b).map(|c| c.2.clone()) {
                    self.copy(&t);
                    return;
                }
                match self.drawn.hits.get(&row).cloned() {
                    Some(Hit::Run(k)) => {
                        if !self.open.remove(&k) {
                            self.open.insert(k);
                        }
                        self.dirty = true;
                        return;
                    }
                    Some(Hit::Choice(i)) => {
                        self.ask_at = i;
                        if self.multi() {
                            if !self.picks.remove(&i) {
                                self.picks.insert(i);
                            }
                            self.dirty = true;
                        } else {
                            self.answer(Some(i));
                        }
                        return;
                    }
                    None => {}
                }
                // selection: 1 click starts it, 2 select the word, 3 the row
                let n = match self.clicks {
                    Some((t, p, n)) if t.elapsed() < Duration::from_millis(400) && p == (row, col) => n % 3 + 1,
                    _ => 1,
                };
                self.clicks = Some((Instant::now(), (row, col), n));
                let text: String = r.line.spans.iter().map(|s| s.content.as_ref()).collect();
                self.sel = match n {
                    2 => {
                        let (a, b) = word_at(&text, col);
                        Some(((row, a), (row, b)))
                    }
                    3 => Some(((row, 0), (row, text.chars().count().saturating_sub(1) as u16))),
                    _ => None,
                };
                if let Some(t) = self.sel.and_then(|_| self.selected()) {
                    self.copy(&t);
                }
                self.dragging = n == 1;
                self.drag_from = Some((row, col));
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging => {
                // past the top / bottom edge: scroll along
                if at.y < self.stream_rect.y {
                    self.scroll(1);
                } else if at.y >= self.stream_rect.bottom() {
                    self.scroll(-1);
                }
                let y = at.y.clamp(self.stream_rect.y, self.stream_rect.bottom().saturating_sub(1));
                if let (Some(from), Some(row)) = (self.drag_from, self.row_at(y)) {
                    self.sel = Some((from, (row, at.x.saturating_sub(self.stream_rect.x))));
                }
            }
            MouseEventKind::Up(MouseButton::Left) if self.dragging => {
                self.dragging = false;
                if self.sel.is_some_and(|(a, b)| a != b)
                    && let Some(t) = self.selected()
                {
                    self.copy(&t);
                }
            }
            _ => {
                if self.input_rect.contains(at) || self.input.area().contains(at) {
                    self.input.mouse(m);
                    if let Some(n) = self.input.copied.take() {
                        self.toast(format!("copied {n} chars"));
                    }
                }
            }
        }
    }

    /// Index in drawn.rows of screen row Y of the stream.
    fn row_at(&self, y: u16) -> Option<usize> {
        if y < self.stream_rect.y || y >= self.stream_rect.bottom() {
            return None;
        }
        let h = usize::from(self.stream_rect.height);
        let total = self.drawn.rows.len();
        let end = total.saturating_sub(self.back);
        let start = end.saturating_sub(h);
        // bottom-aligned: a short stream starts lower
        let pad = h.saturating_sub(end - start);
        let off = usize::from(y - self.stream_rect.y).checked_sub(pad)?;
        let i = start + off;
        (i < end).then_some(i)
    }

    // ---------- draw ----------

    fn tick(&self) -> usize {
        (self.started.elapsed().as_millis() / 150) as usize
    }

    fn header(&self, width: u16) -> Vec<Line<'static>> {
        let tick = self.tick();
        let mut cards: Vec<Vec<Line<'static>>> = Vec::new();
        for name in &self.members {
            let a = self.roster.iter().find(|a| &a.name == name);
            let color = self.colors.get(name).copied().unwrap_or(theme::ACCENT);
            let sprite = a.map(crate::agents::sprite).unwrap_or_else(|| crate::sprites::default_for(name));
            let mut model = self.thread.models.get(name).filter(|m| !m.is_empty()).map(|m| crate::agents::model_name(m)).or_else(|| a.map(crate::agents::model_of)).unwrap_or_default();
            let effort = crate::engine::roster::get(name).and_then(|r| r.effort).or_else(|| a.filter(|a| a.harness == "claude").map(|_| crate::agents::effort_name()));
            if let Some(e) = effort.filter(|e| e != "default") {
                model = if model.is_empty() { e } else { format!("{model} · {e}") };
            }
            let state = if self.thread.asks.iter().any(|q| &q.agent == name) {
                Span::styled("waits on you", Style::default().fg(theme::WARN))
            } else if self.thread.busy.contains(name) {
                Span::styled(format!("{} working", WAVE[tick % WAVE.len()]), Style::default().fg(color))
            } else {
                match self.thread.status.get(name) {
                    Some((s, _)) if s == "blocked" => Span::styled("blocked", Style::default().fg(theme::WARN)),
                    Some((s, _)) if s == "done" => Span::styled("done", Style::default().fg(theme::OK)),
                    _ => Span::styled("idle", theme::muted()),
                }
            };
            let lead = if self.members.first() == Some(name) { " ★" } else { "" };
            let side = vec![
                vec![Span::raw(" "), Span::styled(format!("{name}{lead}"), Style::default().fg(color).add_modifier(Modifier::BOLD))],
                vec![Span::raw(" "), Span::styled(model, theme::muted())],
                vec![Span::raw(" "), state],
                vec![],
            ];
            cards.push(crate::sprites::lines(&sprite, color, side));
        }
        // cards side by side, as many as fit
        let mut rows: Vec<Vec<Span<'static>>> = vec![Vec::new(); crate::sprites::H];
        let mut used = 0usize;
        for c in cards {
            let w = c.iter().map(|l| l.width()).max().unwrap_or(0) + 3;
            if used + w > usize::from(width) && used > 0 {
                break;
            }
            for (r, l) in rows.iter_mut().zip(c) {
                let lw = l.width();
                r.extend(l.spans);
                r.push(Span::raw(" ".repeat(w - lw)));
            }
            used += w;
        }
        rows.into_iter().map(Line::from).collect()
    }

    fn draw(&mut self, f: &mut Frame) {
        let full = f.area();
        f.render_widget(Paragraph::new("").style(theme::text()), full);
        if self.nav_on {
            self.load_nav(false);
        }
        let nav_h = if self.nav_on { crate::nav::CARD_H + 1 } else { 0 };
        let area = Rect { y: full.y + nav_h, height: full.height.saturating_sub(nav_h), ..full };
        if self.nav_on {
            self.draw_nav(f, Rect { height: crate::nav::CARD_H, ..full });
        }
        let head_h = if self.members.is_empty() { 0 } else { crate::sprites::H as u16 };
        let show_cost = area.width >= COST_MIN_SCREEN;
        let in_w = area.width.saturating_sub(if show_cost { COST_W + 1 } else { 0 }).saturating_sub(4).max(10);
        let in_h = (self.input.rows_for(in_w) as u16).clamp(1, INPUT_ROWS) + 2;
        let bottom_h = if show_cost { in_h.max(6) } else { in_h };
        let mon_h = self.monitors.rows(area.width, area.height / 3);
        // title row, header, stream, monitors, prompt, footer
        let title = Rect { height: 1, ..area };
        let head = Rect { y: area.y + 1, height: head_h, ..area };
        let foot = Rect { y: area.bottom().saturating_sub(1), height: 1, ..area };
        let bottom = Rect { y: foot.y.saturating_sub(bottom_h), height: bottom_h, ..area };
        let mon = Rect { y: bottom.y.saturating_sub(mon_h), height: mon_h, ..area };
        let st = Rect { x: area.x + 1, y: head.bottom() + u16::from(head_h > 0), width: area.width.saturating_sub(2), height: mon.y.saturating_sub(head.bottom() + u16::from(head_h > 0)) };
        self.stream_rect = st;

        // title: thread name, where, position
        let mut spans = vec![Span::styled(format!(" {} ", crate::nav::fit(&self.title, usize::from(area.width / 2))), theme::accent_bold())];
        spans.push(Span::styled(format!(" {}", self.id.chars().take(12).collect::<String>()), theme::line()));
        if self.back > 0 {
            spans.push(Span::styled(format!("  ↑{} · esc: live", self.back), theme::pill()));
        }
        if let Some((new, old)) = &self.replace {
            spans.push(Span::styled(format!("  on send: {new} replaces {old} "), theme::pill()));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), title);
        if head_h > 0 {
            f.render_widget(Paragraph::new(self.header(area.width)), Rect { x: area.x + 1, width: area.width.saturating_sub(1), ..head });
        }

        // stream
        if self.dirty || self.width != st.width {
            self.width = st.width;
            self.drawn = stream::draw(&self.thread, st.width, &self.colors, &self.harness_of, &self.open, self.all_runs, self.ask_at, &self.picks, &self.answers.iter().map(|a| a.0.clone()).collect::<Vec<_>>());
            self.dirty = false;
        }
        // sends the log does not show yet, a queued replace: below the stream
        let mut extra: Vec<Line<'static>> = Vec::new();
        for (text, at, err) in &self.pending {
            extra.push(Line::raw(""));
            for l in text.lines().take(6) {
                extra.push(Line::from(vec![Span::styled("▌ ", theme::muted()), Span::styled(l.to_string(), theme::muted())]));
            }
            let status = match err {
                Some(e) => Span::styled(format!("  send failed: {e}"), Style::default().fg(theme::DANGER)),
                None if at.elapsed() > Duration::from_secs(90) => Span::styled("  unconfirmed · check the host log before a retry", theme::muted()),
                None => Span::styled(format!("  sending… {}s", at.elapsed().as_secs()), theme::muted()),
            };
            extra.push(Line::from(status));
        }
        if let Some((new, old, _)) = &self.queued {
            extra.push(Line::raw(""));
            extra.push(Line::styled(format!("  {new} replaces {old} after {old}'s turn · esc: interrupt {old}"), theme::accent()));
        }
        let h = usize::from(st.height);
        let total = self.drawn.rows.len();
        self.back = self.back.min(total.saturating_sub(h));
        let end = total - self.back;
        let start = end.saturating_sub(h);
        let mut lines: Vec<Line> = self.drawn.rows[start..end].iter().map(|r| r.line.clone()).collect();
        if self.back == 0 && !extra.is_empty() {
            lines.extend(extra);
            let cut = lines.len().saturating_sub(h);
            lines.drain(..cut);
        }
        // bottom-aligned: a short thread sits on the prompt
        let pad = h.saturating_sub(lines.len()) as u16;
        let shown = Rect { y: st.y + pad, height: st.height - pad, ..st };
        f.render_widget(Paragraph::new(lines), shown);
        // selection: reverse video; hover: the clickable row under the mouse
        let buf = f.buffer_mut();
        if let Some(((r0, c0), (r1, c1))) = self.sel {
            let ((r0, c0), (r1, c1)) = if (r0, c0) <= (r1, c1) { ((r0, c0), (r1, c1)) } else { ((r1, c1), (r0, c0)) };
            for y in shown.y..shown.bottom() {
                let Some(r) = self.row_at(y) else { continue };
                if r < r0 || r > r1 {
                    continue;
                }
                let (a, b) = (if r == r0 { c0 } else { 0 }, if r == r1 { c1 } else { st.width.saturating_sub(1) });
                for x in a..=b.min(st.width.saturating_sub(1)) {
                    let cell = &mut buf[(st.x + x, y)];
                    cell.set_style(Style::default().bg(theme::ACCENT).fg(theme::ON_ACCENT));
                }
            }
        }
        if let Some(at) = self.hover.filter(|p| shown.contains(*p))
            && let Some(r) = self.row_at(at.y)
        {
            let col = at.x - st.x;
            let row = &self.drawn.rows[r];
            let hit = self.drawn.hits.contains_key(&r);
            let span = row.links.iter().chain(row.copies.iter()).find(|(a, b, _)| col >= *a && col < *b).map(|(a, b, _)| (*a, *b));
            if let Some((a, b)) = span {
                theme::hover(buf, Rect { x: st.x + a, y: at.y, width: b - a, height: 1 });
            } else if hit {
                theme::hover(buf, Rect { x: st.x, y: at.y, width: st.width, height: 1 });
            }
        }
        if total == 0 {
            let note = if self.members.is_empty() { "0 agents on this thread · @name brings one in" } else { "waiting for the first event…" };
            f.render_widget(Paragraph::new(Line::styled(note, theme::muted())), Rect { y: st.bottom().saturating_sub(1), height: 1, ..st });
        }

        if mon_h > 0 {
            self.monitors.render(f, mon, self.tick());
        }

        // prompt box (+ cost box right of it)
        let pw = if show_cost { area.width - COST_W - 1 } else { area.width };
        let pbox = Rect { y: bottom.bottom() - in_h, height: in_h, width: pw, ..bottom };
        let busy = !self.thread.busy.is_empty();
        let label = if !self.thread.asks.is_empty() && self.input.is_empty() {
            " ↑↓ · 1-9: pick · enter: answer ".to_string()
        } else if busy {
            format!(" {} working · esc: interrupt ", WAVE[self.tick() % WAVE.len()])
        } else {
            String::new()
        };
        let block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(theme::line()).title_bottom(Line::styled(label, theme::accent()).right_aligned());
        f.render_widget(block, pbox);
        f.render_widget(Paragraph::new(Span::styled("›", theme::accent())), Rect { x: pbox.x + 1, y: pbox.y + 1, width: 1, height: 1 });
        self.input_rect = Rect { x: pbox.x + 2, y: pbox.y + 1, width: pbox.width.saturating_sub(4), height: in_h - 2 };
        let menu_open = self.menu.is_some();
        self.input.render(f, self.input_rect, !menu_open);
        if show_cost {
            if self.cost_at.elapsed() > Duration::from_secs(3) {
                self.cost_at = Instant::now();
                let (id, tx) = (self.id.clone(), self.tx.clone());
                std::thread::spawn(move || {
                    let _ = tx.send(Msg::Cost(Box::new(crate::cost::snapshot(&id))));
                });
            }
            let cbox = Rect { x: area.right() - COST_W, width: COST_W, ..bottom };
            if let Some(s) = &self.cost {
                crate::cost::draw_in(f, cbox, s, self.cost_at.elapsed());
            }
        }

        // footer
        let right = match &self.toast {
            Some((m, at)) if at.elapsed() < Duration::from_secs(3) => Span::styled(format!("{m} "), theme::accent()),
            _ => Span::styled("esc: interrupt · esc esc: back · ctrl+o: tool calls · F2: menu ", theme::muted()),
        };
        f.render_widget(Paragraph::new(Line::from(right)).right_aligned(), foot);

        // dropdowns over everything, then the menu
        if !menu_open {
            self.mention.render(f, &self.input, &self.names, area);
            self.command.render(f, &self.input, area, self.input_rect);
            self.paths.render(f, &self.input, area, self.input_rect);
        }
        if let Some(m) = self.menu.as_mut() {
            f.render_widget(Clear, Rect::default());
            m.render(f, area);
        }
    }
}

/// The VS Code CLI: $CODEHEAD_VSCODE, else code / codium / code-insiders on PATH, else the
/// macOS app bundles.
fn vscode_cli() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("CODEHEAD_VSCODE").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    let names = ["code", "codium", "code-insiders"];
    let on_path = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).flat_map(|d| names.map(|n| d.join(n))).find(|p| p.is_file()));
    let home = crate::config::home();
    let bundles = [("Visual Studio Code.app", "code"), ("VSCodium.app", "codium"), ("Visual Studio Code - Insiders.app", "code-insiders")];
    on_path.or_else(|| {
        [PathBuf::from("/Applications"), home.join("Applications")]
            .iter()
            .flat_map(|d| bundles.map(|(app, bin)| d.join(app).join("Contents/Resources/app/bin").join(bin)))
            .find(|p| p.is_file())
    })
}

/// A stream row's text without the message bar on its left.
fn unframe(row: &str) -> String {
    for bar in ["│     ", "│   ", "│ ", "┆ ", "▌ ", "┃ "] {
        if let Some(rest) = row.strip_prefix(bar) {
            return rest.to_string();
        }
    }
    row.to_string()
}

/// Column range of the word at COL of TEXT.
fn word_at(text: &str, col: u16) -> (u16, u16) {
    let chars: Vec<char> = text.chars().collect();
    let c = usize::from(col).min(chars.len().saturating_sub(1));
    let word = |ch: char| !ch.is_whitespace() && !"│┆▌┃".contains(ch);
    if chars.get(c).is_none_or(|ch| !word(*ch)) {
        return (col, col);
    }
    let a = (0..=c).rev().take_while(|&i| word(chars[i])).last().unwrap_or(c);
    let b = (c..chars.len()).take_while(|&i| word(chars[i])).last().unwrap_or(c);
    (a as u16, b as u16)
}

fn open_url(url: &str) {
    let bin = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let _ = std::process::Command::new(bin).arg(url).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
}

/// The view of thread ID until it ends. NOTE: a toast at the start (why the view opened).
pub fn run<B: Backend>(term: &mut Terminal<B>, id: &str, cwd: &Path, title: &str, note: Option<&str>, _first: &str) -> Result<Exit, String> {
    if store::load(id).is_none() {
        return Err(format!("no thread {id}"));
    }
    let (tx, rx): (Sender<Msg>, Receiver<Msg>) = mpsc::channel();
    let mut v = View::new(id, cwd, title, tx);
    if let Some(n) = note.filter(|n| !n.is_empty()) {
        v.toast(n.to_string());
    }
    crate::nav::mark_seen_now(id);
    let mut roster_at = Instant::now();
    let mut drawn = Instant::now() - Duration::from_secs(1);
    let mut dirty = true;
    loop {
        if dirty || drawn.elapsed() >= Duration::from_millis(150) {
            term.draw(|f| v.draw(f)).map_err(|e| e.to_string())?;
            drawn = Instant::now();
            dirty = false;
        }
        for (ev, at) in crate::read_ready(Duration::from_millis(50))? {
            crate::textarea::stamp(Some(at));
            dirty = true;
            let done = match ev {
                Event::Key(k) if k.kind != KeyEventKind::Release => v.key(k),
                Event::Mouse(m) => {
                    v.mouse(m);
                    v.nav_click
                }
                Event::Paste(s) => {
                    v.paste(&s);
                    false
                }
                _ => false,
            };
            crate::textarea::stamp(None);
            if done {
                crate::nav::mark_seen_now(id);
                return Ok(std::mem::replace(&mut v.exit, Exit::Back));
            }
        }
        while let Ok(m) = rx.try_recv() {
            dirty = true;
            match m {
                Msg::Done("send", Err(e)) => {
                    if let Some(p) = v.pending.iter_mut().rev().find(|p| p.2.is_none()) {
                        p.2 = Some(e.lines().next().unwrap_or_default().to_string());
                    }
                }
                Msg::Done(what, Err(e)) => v.toast(format!("{what}: {e}")),
                Msg::Done("join" | "leave" | "replace", Ok(_)) => {
                    let r = v.roster.clone();
                    v.set_roster(r);
                }
                Msg::Done(..) => {}
                Msg::Cost(s) => v.cost = Some(*s),
            }
        }
        dirty |= v.read();
        dirty |= v.monitors.tick(id);
        if roster_at.elapsed() > Duration::from_secs(5) {
            roster_at = Instant::now();
            v.set_roster(crate::agents::load());
            crate::nav::mark_seen(id);
        }
    }
}
