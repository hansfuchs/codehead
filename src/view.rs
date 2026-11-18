//! Thread view (hell's layout): the stream on top (every message, tool run and open question of
//! the thread, from its event log), the title row, the header (agents box + tokens box in 1
//! frame), the input bubble (its top border the header's bottom line), the hint row, the footer.
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

/// Views of a thread: the view bar in the title row (shift+tab: next, alt+1 / alt+2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pane {
    #[default]
    Chat,
    Changes,
}

impl Pane {
    const ALL: [Pane; 2] = [Pane::Chat, Pane::Changes];

    fn next(self) -> Pane {
        let i = Pane::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Pane::ALL[(i + 1) % Pane::ALL.len()]
    }
}

/// Thread header open (tab toggles it; kept across the thread switches it makes).
static NAV_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The thread opened first from the start screen: the header's 1st card (kept across switches).
static NAV_FIRST: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// Max. width of a header card (cards fit their titles).
const NAV_CARD_W: u16 = 32;

/// A thread view from the start screen: header closed, 0 first thread.
pub fn reset_nav() {
    NAV_ON.store(false, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut f) = NAV_FIRST.lock() {
        f.clear();
    }
}

/// The header's 1st thread (ID: set as the first when 0 is set).
fn nav_first(id: &str) -> String {
    let Ok(mut f) = NAV_FIRST.lock() else { return id.to_string() };
    if f.is_empty() {
        *f = id.to_string();
    }
    f.clone()
}

/// 2 Esc presses within this time: back to the start screen. A single Esc interrupts once this time ends without a 2nd one.
const DOUBLE_ESC: Duration = Duration::from_millis(500);
/// Most rows the prompt grows to; longer text scrolls.
const INPUT_ROWS: u16 = 8;
/// Below this width: 0 tokens box (the agents box takes the header row).
const MIN_COST_WIDTH: u16 = 70;
/// Narrowest / widest tokens box (borders incl.); squeezed: a thin screen with 1 card.
const MIN_COST_BOX: u16 = 48;
const MAX_COST_BOX: u16 = 66;
const SQUEEZED_COST_BOX: u16 = 30;
/// Header: the sprite rows + its top border at least, max. this many rows.
const HEADER_H: u16 = crate::sprites::H as u16 + 1;
const MAX_HEADER_H: u16 = 10;
/// Below this height: 0 header.
const MIN_HEADER_SCREEN: u16 = 24;
/// Handoff to a replacing agent: the replaced agent's last messages, max. this many chars.
const HANDOFF_CHARS: usize = 12_000;
const WAVE: [&str; 4] = ["░▒▓", "▒▓▒", "▓▒░", "▒░▒"];

enum Msg {
    /// A command to the host ended: what it was, Err(why).
    Done(&'static str, Result<Value, String>),
    Cost(Box<crate::cost::Snap>),
    /// The host logged events: read the tail.
    Wake,
    /// Live text of an agent (token chunk): (agent, text).
    Delta(String, String),
    /// A (new) subscription: the live text comes again from the host.
    Subscribed,
    /// All live text of an agent so far: (agent, text).
    Live(String, String),
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
    /// Per roster agent: sprite, effort label of its card (read on roster change, not per frame).
    profiles: HashMap<String, (crate::sprites::Sprite, String)>,
    // stream
    drawn: stream::Drawn,
    dirty: bool,
    width: u16,
    /// Rows scrolled up from the bottom (0: live).
    back: usize,
    open: HashSet<usize>,
    all_runs: bool,
    /// Thoughts: how they show (setting), the open folds, ctrl+t: all open.
    thoughts: stream::Thoughts,
    open_thoughts: HashSet<usize>,
    all_thoughts: bool,
    stream_rect: Rect,
    /// Drag selection in the stream: (row, col) of its start and end, in drawn rows.
    sel: Option<((usize, u16), (usize, u16))>,
    dragging: bool,
    /// Last click in the stream: time, (row, col), count (2: word, 3: row).
    clicks: Option<(Instant, (usize, u16), u8)>,
    /// Mouse position: the clickable row under it gets the hover bg.
    hover: Option<Position>,
    // ask
    /// The 1st open ask: cursor, picks, answers so far, focus.
    ask: crate::ask::Picker,
    /// Tests: the host commands, recorded (0 host process).
    #[cfg(test)]
    ops: std::cell::RefCell<Vec<Value>>,
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
    /// Per agent: live text not logged yet (token chunks), shown at the end of the stream.
    live: Vec<(String, String)>,
    // views of the thread (view bar): chat or changes; the prompt's focus outside chat
    pane: Pane,
    changes: crate::changes::View,
    prompt_focus: bool,
    bar_rects: Vec<(Rect, Pane)>,
    /// A review comment in the prompt: file, line, the draft the prompt had before.
    commenting: Option<(String, u32, String)>,
    // thread header: cards of the open threads (tab)
    nav_on: bool,
    nav_cards: Vec<crate::nav::Card>,
    /// Cursor in the header order (nav_order).
    nav_at: usize,
    /// The 1st column of the card strip in view.
    nav_offset: u16,
    nav_first: String,
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
    /// A 1st Esc armed an interrupt: it fires after DOUBLE_ESC unless a 2nd Esc means back.
    esc_interrupt: Option<Instant>,
    last_ctrl_c: Option<Instant>,
    exit: Exit,
    /// The header switched threads: the view ends with EXIT.
    nav_switch: bool,
    /// A click on a card switched threads.
    nav_click: bool,
    /// Where the drag started (drawn row, col).
    drag_from: Option<(usize, u16)>,
    tx: Sender<Msg>,
    /// How the stream draws messages (setting `messages`).
    look: stream::Look,
}

impl View {
    fn new(id: &str, cwd: &Path, title: &str, tx: Sender<Msg>) -> View {
        let roster = crate::agents::load();
        let mut input = TextArea::new(false, "message the thread · @name for someone else");
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
            profiles: HashMap::new(),
            drawn: stream::Drawn::default(),
            dirty: true,
            width: 0,
            back: 0,
            open: HashSet::new(),
            all_runs: false,
            thoughts: crate::config::thoughts(),
            open_thoughts: HashSet::new(),
            all_thoughts: false,
            stream_rect: Rect::default(),
            sel: None,
            dragging: false,
            clicks: None,
            hover: None,
            ask: crate::ask::Picker::default(),
            #[cfg(test)]
            ops: Default::default(),
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
            live: Vec::new(),
            pane: Pane::Chat,
            changes: crate::changes::View::new(id, cwd),
            prompt_focus: false,
            bar_rects: Vec::new(),
            commenting: None,
            // tests: 0 shared state between parallel views
            nav_on: !cfg!(test) && NAV_ON.load(std::sync::atomic::Ordering::Relaxed),
            nav_cards: Vec::new(),
            nav_at: 0,
            nav_offset: 0,
            nav_first: nav_first(id),
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
            esc_interrupt: None,
            last_ctrl_c: None,
            exit: Exit::Back,
            nav_switch: false,
            nav_click: false,
            drag_from: None,
            tx,
            look: crate::config::messages(),
        };
        v.set_roster(roster);
        v.read();
        v
    }

    /// The agents (each 5 s, join, leave): the stream redraws only when its inputs changed.
    fn set_roster(&mut self, roster: Vec<crate::agents::Agent>) {
        let colors: stream::Colors = roster.iter().map(|a| (a.name.clone(), crate::agents::color(a.color.as_deref()))).collect();
        let harness_of: Vec<(String, String)> = roster.iter().map(|a| (a.name.clone(), a.harness.clone())).collect();
        if colors != self.colors || harness_of != self.harness_of {
            self.dirty = true;
        }
        self.colors = colors;
        self.harness_of = harness_of;
        self.names = roster.iter().map(|a| (a.name.clone(), crate::agents::color(a.color.as_deref()))).collect();
        self.input.marks = self.names.clone();
        let global = crate::agents::effort_name();
        self.profiles = roster
            .iter()
            .map(|a| {
                let effort = crate::engine::roster::get(&a.name).and_then(|r| r.effort).or_else(|| (a.harness == "claude").then(|| global.clone())).filter(|e| e != "default").map(|e| effort_label(&e)).unwrap_or_default();
                (a.name.clone(), (crate::agents::sprite(a), effort))
            })
            .collect();
        self.roster = roster;
        self.members = store::load(&self.id).map(|t| t.agents).unwrap_or_default();
        self.mention.replaceable = self.members.clone();
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
            // the logged text (or the turn's end) replaces the live chunks of its agent
            if matches!(e.kind, store::Kind::Text { .. } | store::Kind::Tool { .. } | store::Kind::TurnEnd { .. })
                && let Some(a) = &e.agent
            {
                self.live.retain(|(n, _)| n != a);
            }
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
            self.ask.sync(self.thread.asks.first());
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
        #[cfg(test)]
        {
            let _ = what;
            self.ops.borrow_mut().push(c);
        }
        #[cfg(not(test))]
        let (id, tx) = (self.id.clone(), self.tx.clone());
        #[cfg(not(test))]
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Done(what, client::call(&id, &c)));
        });
    }

    // ---------- send ----------

    fn submit(&mut self) {
        // a review comment: into the batch, the draft back, the keys back to the changes view
        if let Some((file, line, draft)) = self.commenting.take() {
            let text = self.input.text();
            self.changes.add_comment(&file, line, &text);
            self.input.set_text(&draft);
            self.prompt_focus = false;
            self.toast(format!("comment on {file}:{line} · {} in the batch · C: send", self.changes.comment_count()));
            return;
        }
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
        let replacing = self.replace.take().filter(|(new, old)| text.contains(&format!("@{new}{}", crate::mention::replaces_note(old))));
        // the `(replaces @old)` note: for the owner only
        let text = crate::mention::strip_replaces(&text);
        if let Some((new, old)) = replacing {
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

    /// Something on screen moves: an agent at work (wave), monitors, the threads row, a toast.
    fn animating(&self) -> bool {
        !self.thread.busy.is_empty() || self.monitors.active() || self.nav_on || self.toast.as_ref().is_some_and(|(_, at)| at.elapsed() < Duration::from_secs(4))
    }

    /// Fire an interrupt armed by a single Esc once DOUBLE_ESC ends. True: state changed.
    fn tick_esc(&mut self) -> bool {
        if self.esc_interrupt.is_some_and(|t| t.elapsed() >= DOUBLE_ESC) {
            self.esc_interrupt = None;
            self.interrupt();
            return true;
        }
        false
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

    /// A picker result: redraw, or the answer to the host. True: the picker took the key.
    fn ask_act(&mut self, act: crate::ask::Act) -> bool {
        match act {
            crate::ask::Act::Pass => false,
            crate::ask::Act::Changed => {
                self.dirty = true;
                true
            }
            crate::ask::Act::Answer(c) => {
                self.ask.sent = true;
                self.dirty = true;
                self.host("answer", c);
                true
            }
        }
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
            return self.nav_switch;
        }
        // views of the thread: shift+tab the next one, alt+1 / alt+2 chat / changes
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        if k.code == KeyCode::BackTab && !ctrl && !alt && !self.mention_open() {
            self.set_pane(self.pane.next());
            return false;
        }
        if alt && !ctrl && let KeyCode::Char(c @ '1'..='9') = k.code {
            if let Some(p) = Pane::ALL.get(usize::from(c as u8 - b'1')) {
                self.set_pane(*p);
            }
            return false;
        }
        // the changes view with the focus: its keys (esc, tab, ctrl+c and the menu key: as in chat)
        if self.pane == Pane::Changes && !self.prompt_focus {
            let through = matches!(k.code, KeyCode::Esc | KeyCode::Tab) || (ctrl && k.code == KeyCode::Char('c')) || crate::config::menu_key().matches(&k);
            if !through {
                if k.code == KeyCode::Char('i') && !ctrl && !alt {
                    self.prompt_focus = true;
                    return false;
                }
                let act = self.changes.key(k);
                self.changes_act(act);
                return false;
            }
        }
        // an open ask with the focus: its keys before the prompt's (text in the prompt or not)
        if let Some(a) = self.thread.asks.first().cloned() {
            let act = self.ask.key(&a, k);
            if self.ask_act(act) {
                return false;
            }
        }
        // dropdowns first: they own ↑↓, tab, enter, esc while open
        if self.mention.key(k, &mut self.input, &self.roster) {
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
        match k.code {
            _ if crate::config::menu_key().matches(&k) => {
                self.open_menu();
                return false;
            }
            // the prompt in the changes view: esc gives the keys back to the view
            KeyCode::Esc if self.pane == Pane::Changes && self.prompt_focus => {
                if let Some((_, _, draft)) = self.commenting.take() {
                    self.input.set_text(&draft);
                }
                self.prompt_focus = false;
                return false;
            }
            KeyCode::Esc => {
                if self.last_esc.is_some_and(|t| t.elapsed() < DOUBLE_ESC) {
                    self.esc_interrupt = None;
                    self.exit = Exit::Back;
                    return true;
                }
                self.last_esc = Some(Instant::now());
                if self.sel.take().is_some() {
                } else if self.back > 0 {
                    self.back = 0;
                } else if !self.thread.busy.is_empty() || self.queued.is_some() {
                    // a 2nd Esc within DOUBLE_ESC means back: the interrupt waits for it
                    self.esc_interrupt = Some(Instant::now());
                }
                return false;
            }
            // tab: the thread header (prompt text or not; dropdowns took theirs above)
            KeyCode::Tab if !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) && !self.mention_open() => {
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
            KeyCode::Char('t') if ctrl => {
                self.toggle_thoughts();
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
            // ctrl+s: save only (the thread view keeps the prompt by itself), never send
            KeyCode::Char('s') if ctrl => {
                self.toast("send: ctrl+enter (⌘enter, alt+enter) · the prompt stays as a draft");
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

    /// Switch to view P of the thread. The changes view: git read at once, keys to the view.
    fn set_pane(&mut self, p: Pane) {
        if self.pane == p {
            return;
        }
        self.pane = p;
        self.prompt_focus = false;
        self.dirty = true;
        if p == Pane::Changes {
            self.changes.refresh(true);
        }
    }

    fn changes_act(&mut self, a: crate::changes::Act) {
        match a {
            crate::changes::Act::None => {}
            crate::changes::Act::Toast(t) => self.toast(t),
            crate::changes::Act::Open(file, line) => self.open_at(&file, line),
            crate::changes::Act::Comment(file, line) => {
                // the prompt takes the comment; its draft waits
                let draft = self.input.text();
                self.input.set_text("");
                self.commenting = Some((file, line, draft));
                self.prompt_focus = true;
            }
            crate::changes::Act::Send(text) => {
                self.back = 0;
                self.pending.push((text.clone(), Instant::now(), None));
                self.host("send", json!({"op": "prompt", "text": text}));
                self.toast("review comments sent to the lead");
            }
        }
    }

    /// VS Code (family) at FILE:LINE of the worktree: the window that has it open, else a new one.
    fn open_at(&mut self, file: &str, line: u32) {
        let Some(cli) = vscode_cli() else {
            self.toast("VS Code: 0 CLI found (code, codium, code-insiders; $CODEHEAD_VSCODE)");
            return;
        };
        let target = format!("{}:{line}", self.cwd.join(file).display());
        let ok = std::process::Command::new(&cli)
            .arg(&self.cwd)
            .arg("-g")
            .arg(&target)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok();
        self.toast(if ok { format!("IDE: {file}:{line}") } else { format!("VS Code: {} did not start", cli.display()) });
    }

    fn mention_open(&self) -> bool {
        self.mention.is_open(&self.input, &self.roster)
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
            self.nav_at = self.nav_here();
        }
    }

    /// Header order: the thread opened first, then the open threads by activity. Before the 1st
    /// read: this thread alone.
    fn nav_order(&self) -> Vec<crate::nav::Card> {
        let order = crate::nav::header_order(&self.nav_cards, &self.nav_first, &self.id);
        if order.iter().any(|c| c.id == self.id) {
            return order;
        }
        let mut v = vec![crate::nav::Card { id: self.id.clone(), title: self.title.clone(), state: "open".into(), live: 1, dir: Some(self.cwd.clone()), ..Default::default() }];
        v.extend(order);
        v
    }

    /// Index of this thread in the header order.
    fn nav_here(&self) -> usize {
        self.nav_order().iter().position(|c| c.id == self.id).unwrap_or(0)
    }

    /// ← (D -1) / → (D 1): the header cursor moves (wraps); enter switches.
    fn nav_step(&mut self, d: isize) {
        let n = self.nav_order().len();
        if n <= 1 {
            self.toast("0 other open threads");
            return;
        }
        let from = self.nav_at.min(n - 1);
        self.nav_at = (from as isize + d).rem_euclid(n as isize) as usize;
    }

    /// Thread cards, read in the background every 3s (NOW: at once).
    fn load_nav(&mut self, now: bool) {
        if let Some(rx) = &self.nav_read
            && let Ok(cards) = rx.try_recv()
        {
            // the cursor stays on its thread when the order changes
            let at = self.nav_order().get(self.nav_at).map(|c| c.id.clone());
            self.nav_cards = cards;
            self.nav_at = at.and_then(|id| self.nav_order().iter().position(|c| c.id == id)).unwrap_or_else(|| self.nav_here());
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
            KeyCode::Left => self.nav_step(-1),
            KeyCode::Right => self.nav_step(1),
            KeyCode::Enter => {
                self.nav_open(self.nav_at);
            }
            KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => self.set_nav(false),
            _ => return false,
        }
        true
    }

    /// Switch to card I of the header order (this thread: the header closes). True: the view ends.
    fn nav_open(&mut self, i: usize) -> bool {
        let Some(c) = self.nav_order().get(i).cloned() else { return false };
        if c.id == self.id {
            self.set_nav(false);
            return false;
        }
        self.nav_switch = true;
        self.exit = Exit::Open { id: c.id.clone(), dir: c.dir.clone().unwrap_or_else(|| self.cwd.clone()), title: c.title, note: String::new() };
        true
    }

    /// Thread header in AREA (CARD_H + 1 rows): cards fit to their titles (max. NAV_CARD_W), side
    /// by side on a strip that scrolls by columns: a card cut at the edge shows open on that side
    /// (only with room for part of its title). Below the cards: `n+` / `+n`, the cards out of view.
    fn draw_nav(&mut self, f: &mut Frame, area: Rect) {
        self.nav_rects.clear();
        if area.height < crate::nav::CARD_H || area.width < 8 {
            return;
        }
        let order = self.nav_order();
        let n = order.len();
        let here = order.iter().position(|c| c.id == self.id).unwrap_or(0);
        let sel = self.nav_at.min(n - 1);
        let row = Rect { height: crate::nav::CARD_H, ..area };
        // strip positions: x of each card, 1 column between cards
        let widths: Vec<u16> = order.iter().map(|c| crate::nav::card_width(c, NAV_CARD_W.min(row.width))).collect();
        let mut xs = Vec::with_capacity(n);
        let mut x = 0u16;
        for w in &widths {
            xs.push(x);
            x = x.saturating_add(w + 1);
        }
        let total = x.saturating_sub(1);
        // scroll: the picked card fully in view, 3 columns of its neighbor too where possible
        let (sx, sw) = (xs[sel], widths[sel]);
        let peek = 3;
        let mut off = self.nav_offset;
        if sx < off + peek.min(sx) {
            off = sx.saturating_sub(peek);
        }
        if sx + sw + peek > off + row.width {
            off = (sx + sw + peek).saturating_sub(row.width);
        }
        off = off.min(total.saturating_sub(row.width));
        self.nav_offset = off;
        let tick = self.tick();
        let (mut left, mut right) = (0, 0);
        for i in 0..n {
            let (cx, cw) = (xs[i], widths[i]);
            // visible columns of card i
            let a = cx.max(off);
            let b = (cx + cw).min(off + row.width);
            let cut = a > cx || b < cx + cw;
            if cut {
                if cx < off { left += 1 } else { right += 1 }
            }
            // a cut card: only with room for its border, a space and 3 title columns
            if b <= a || (cut && b - a < 6) {
                continue;
            }
            let mut card = ratatui::buffer::Buffer::empty(Rect::new(0, 0, cw, row.height));
            let ca = card.area;
            crate::nav::draw_card(&mut card, ca, &order[i], i == sel, i == here, false, tick);
            let dst = Rect { x: row.x + (a - off), width: b - a, ..row };
            let buf = f.buffer_mut();
            for dy in 0..row.height {
                for dx in 0..dst.width {
                    buf[(dst.x + dx, dst.y + dy)] = card[(a - cx + dx, dy)].clone();
                }
            }
            self.nav_rects.push((dst, i));
        }
        if area.height <= crate::nav::CARD_H {
            return;
        }
        let mid = Rect { y: area.y + crate::nav::CARD_H, height: 1, ..area };
        if left > 0 {
            f.render_widget(Paragraph::new(Span::styled(format!("{left}+"), theme::accent())), Rect { width: mid.width / 2, ..mid });
        }
        if right > 0 {
            f.render_widget(Paragraph::new(Span::styled(format!("+{right}"), theme::accent())).right_aligned(), Rect { x: mid.x + mid.width / 2, width: mid.width - mid.width / 2, ..mid });
        }
    }

    fn scroll(&mut self, delta: isize) {
        let max = self.drawn.rows.len().saturating_sub(usize::from(self.stream_rect.height));
        self.back = (self.back as isize + delta).clamp(0, max as isize) as usize;
    }

    /// ctrl+t: every thought open / folded. Hidden (setting): shown folded first.
    fn toggle_thoughts(&mut self) {
        if self.thoughts == stream::Thoughts::Hidden {
            self.thoughts = stream::Thoughts::Folded;
            self.all_thoughts = true;
            self.toast("thoughts shown · setting: thoughts");
        } else {
            self.all_thoughts = !self.all_thoughts;
        }
        self.open_thoughts.clear();
        self.dirty = true;
    }

    fn open_menu(&mut self) {
        use crate::menu::Item;
        let items = vec![
            Item { label: format!("thread header: {}", if self.nav_on { "close" } else { "open" }), key: "tab".into(), id: 0 },
            Item { label: "tool calls: open / close all".into(), key: "ctrl+o".into(), id: 1 },
            Item { label: "thoughts: open / close all".into(), key: "ctrl+t".into(), id: 13 },
            Item { label: "VS Code on the worktree".into(), key: "ctrl+i".into(), id: 2 },
            Item { label: "back to the start screen".into(), key: "esc esc".into(), id: 3 },
            Item { label: "agents: mention · →: replace an agent".into(), key: "@".into(), id: 4 },
            Item { label: "commands: files, notes, threads, skills, branch".into(), key: crate::config::command_key().to_string(), id: 5 },
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
            Action::Command(13) => self.toggle_thoughts(),
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
                self.input.insert(&format!("{gap}{}", if id == 4 { "@".to_string() } else { crate::config::command_key().to_string() }));
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
            || self.mention.mouse(m, &mut self.input, &self.roster)
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
        let click = matches!(m.kind, MouseEventKind::Down(MouseButton::Left));
        // the view bar
        if click && let Some(&(_, p)) = self.bar_rects.iter().find(|(r, _)| r.contains(at)) {
            self.set_pane(p);
            return;
        }
        let in_stream = self.stream_rect.contains(at);
        if self.pane == Pane::Changes {
            if in_stream {
                self.prompt_focus = self.prompt_focus && !click;
                self.changes.mouse(m);
                return;
            }
            if click && self.input_rect.contains(at) {
                self.prompt_focus = true;
            }
        }
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
                    Some(Hit::Thought(k)) => {
                        if !self.open_thoughts.remove(&k) {
                            self.open_thoughts.insert(k);
                        }
                        self.dirty = true;
                        return;
                    }
                    Some(Hit::Run(k)) => {
                        if !self.open.remove(&k) {
                            self.open.insert(k);
                        }
                        self.dirty = true;
                        return;
                    }
                    Some(Hit::Choice(i)) => {
                        if let Some(a) = self.thread.asks.first().cloned()
                            && !self.ask.sent
                        {
                            let act = self.ask.click(&a, i);
                            self.ask_act(act);
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
                    // a click in the prompt: the keys go to it (tab: back to the ask)
                    if matches!(m.kind, MouseEventKind::Down(_)) && self.ask.focus {
                        self.ask.focus = false;
                        self.dirty = true;
                    }
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

    /// Header card of agent NAME (hell's): sprite, color, the lines right of the sprite (name,
    /// `Claude · Opus 5.5`, effort + status), width incl. 4 columns gap to the next card.
    fn card(&self, name: &str) -> Card {
        let tick = self.tick();
        let a = self.roster.iter().find(|a| a.name == name);
        let color = self.colors.get(name).copied().unwrap_or(theme::pal().accent);
        let (sprite, effort) = self.profiles.get(name).cloned().unwrap_or_else(|| (crate::sprites::default_for(name), String::new()));
        let model = self.thread.models.get(name).filter(|m| !m.is_empty()).map(|m| crate::agents::model_name(m)).or_else(|| a.map(crate::agents::model_of)).unwrap_or_default();
        let harness = a.map(|a| title_case(&a.harness)).unwrap_or_default();
        let tech = [harness, model].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
        let frame = WAVE[tick % WAVE.len()];
        let status = if self.thread.asks.iter().any(|q| q.agent == name) {
            vec![Span::styled("  ● ", Style::default().fg(theme::pal().warn)), Span::styled("waits on you", theme::muted())]
        } else if self.thread.busy.contains(name) {
            vec![Span::styled(format!("  {frame} "), Style::default().fg(color)), Span::styled("working…", theme::muted())]
        } else if self.monitors.watches.iter().any(|w| w.agent == name) {
            vec![Span::styled(format!("  {frame} "), Style::default().fg(color)), Span::styled("monitoring…", theme::muted())]
        } else {
            Vec::new()
        };
        // the status right of the effort: `  ░▒▓ monitoring…`, 17 columns
        let status_w = if status.is_empty() { 0 } else { 17 };
        let text_w = [name.chars().count(), tech.chars().count(), effort.chars().count() + status_w].into_iter().max().unwrap_or(0);
        let mut last = vec![Span::styled(format!("  {effort}"), theme::muted())];
        last.extend(status);
        let side = vec![
            vec![],
            vec![Span::styled(format!("  {name}"), Style::default().fg(color).add_modifier(Modifier::BOLD))],
            vec![Span::styled(format!("  {tech}"), theme::muted())],
            last,
        ];
        (sprite, color, side, (crate::sprites::W + 2 + text_w + 4) as u16)
    }

    /// Header (hell's): 1 rounded frame over the agents box (the cards) and the tokens box (COST:
    /// the pane's area, 0 width: 0 tokens box), straight lines between them.
    fn draw_header(&self, f: &mut Frame, area: Rect, cost: Rect) {
        if area.height == 0 {
            return;
        }
        let cards: Vec<Card> = self.members.iter().map(|n| self.card(n)).collect();
        let token_left = cost.x.saturating_sub(2);
        let fit = if cost.width > 0 { token_left + 1 } else { area.right() }.saturating_sub(area.x);
        let outer = if cost.width == 0 { area } else { Rect { width: cards_w(&cards).min(fit), ..area } };
        let block = Block::new().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(theme::line());
        f.render_widget(block, area);
        if cost.width > 0 {
            let buf = f.buffer_mut();
            for x in [outer.right().saturating_sub(1), token_left] {
                if x <= area.x || x + 1 >= area.right() {
                    continue;
                }
                for y in area.y..area.bottom() {
                    let sym = if y == area.y { "┬" } else if y + 1 == area.bottom() { "┴" } else { "│" };
                    buf[(x, y)].set_symbol(sym).set_style(theme::line());
                }
            }
        }
        let rows = Rect { y: area.y + 1, height: area.height.saturating_sub(2), ..area };
        let mut x = area.x + 2;
        for (sprite, color, side, w) in cards {
            if x + w - 1 > outer.right() {
                break;
            }
            f.render_widget(Paragraph::new(crate::sprites::lines(&sprite, color, side)), Rect { x, width: w - 4, ..rows });
            x += w;
        }
    }

    /// Who gets a message now: the @named agents of the input, else the lead.
    fn recipients(&self) -> Vec<String> {
        let text = self.input.text();
        let named = crate::engine::host::mentions(&crate::mention::strip_replaces(&text), &self.members);
        if named.is_empty() { self.members.first().cloned().into_iter().collect() } else { named }
    }

    /// The stream in ST: the thread's items, sends below them, waves, selection, hover.
    fn draw_stream(&mut self, f: &mut Frame, st: Rect) {
        // stream
        if self.dirty || self.width != st.width {
            // scrolled back: the top row stays where it is (new events, folds, a resize)
            let h = usize::from(st.height);
            let anchor = (self.back > 0).then(|| top_anchor(&self.drawn.starts, self.drawn.rows.len(), self.back, h)).flatten();
            self.width = st.width;
            let fold = stream::Fold { runs: &self.open, all_runs: self.all_runs, thoughts: self.thoughts, open_thoughts: &self.open_thoughts, all_thoughts: self.all_thoughts };
            // live text: messages at the end for this draw only (the log brings the final ones)
            let logged = self.thread.items.len();
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
            for (agent, text) in &self.live {
                self.thread.items.push(stream::Item::Text { agent: agent.clone(), text: text.clone(), ts: now, thoughts: Vec::new(), key: 0 });
            }
            self.drawn = stream::draw(&self.thread, self.look, st.width, &self.colors, &self.harness_of, &fold, &self.ask);
            self.thread.items.truncate(logged);
            self.dirty = false;
            if let Some(a) = anchor {
                self.back = back_for(&self.drawn.starts, self.drawn.rows.len(), a, h);
            }
        }
        // sends the log does not show yet, a queued replace: below the stream
        let mut extra: Vec<Line<'static>> = Vec::new();
        let right = self.look == stream::Look::Bubbles;
        for (text, at, err) in &self.pending {
            extra.push(Line::raw(""));
            for l in text.lines().take(6) {
                // `@name`: in the agent's color, as in the stream
                let mut spans = crate::render::mention_spans(l, theme::muted(), &self.names);
                let l = if right {
                    spans.push(Span::styled(" ▐", theme::muted()));
                    Line::from(spans).right_aligned()
                } else {
                    spans.insert(0, Span::styled("▌ ", theme::muted()));
                    Line::from(spans)
                };
                extra.push(l);
            }
            let status = match err {
                Some(e) => Line::styled(format!("send failed: {e}  "), Style::default().fg(theme::pal().danger)),
                None if at.elapsed() > Duration::from_secs(90) => Line::styled("unconfirmed · check the host log before a retry  ", theme::muted()),
                None => Line::styled(format!("sending… {}s  ", at.elapsed().as_secs()), theme::muted()),
            };
            extra.push(if right { status.right_aligned() } else { status });
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
        let mut cut = 0;
        if self.back == 0 && !extra.is_empty() {
            lines.extend(extra);
            cut = lines.len().saturating_sub(h);
            lines.drain(..cut);
        }
        // bottom-aligned: a short thread sits on the title row
        let pad = h.saturating_sub(lines.len()) as u16;
        let shown = Rect { y: st.y + pad, height: st.height - pad, ..st };
        f.render_widget(Paragraph::new(lines), shown);
        // the working wave of the boxes in view
        let tick = self.tick();
        for (row, col, color) in &self.drawn.waves {
            if *row < start + cut || *row >= end {
                continue;
            }
            let y = shown.y + (row - start - cut) as u16;
            if y < shown.bottom() && st.x + col + 3 <= st.right() {
                f.render_widget(Paragraph::new(Span::styled(WAVE[tick % WAVE.len()], Style::default().fg(*color))), Rect { x: st.x + col, y, width: 3, height: 1 });
            }
        }
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
                    cell.set_style(Style::default().bg(theme::pal().accent).fg(theme::pal().on_accent));
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
                // bubbles: the hover stays inside the box's borders
                let (x0, x1) = if self.look == stream::Look::Bubbles { (st.x + 2, st.right().saturating_sub(2)) } else { (st.x, st.right()) };
                theme::hover(buf, Rect { x: x0, y: at.y, width: x1.saturating_sub(x0), height: 1 });
            }
        }
        // scrolled back: the stream's last row a fold line
        if self.back > 0 && st.height > 1 {
            let y = st.bottom() - 1;
            for x in st.x..st.right() {
                buf[(x, y)].reset();
                buf[(x, y)].set_symbol("┄").set_style(theme::line());
            }
        }
        if total == 0 {
            let note = if self.members.is_empty() { "0 agents on this thread · @name brings one in" } else { "waiting for the first event…" };
            f.render_widget(Paragraph::new(Line::styled(format!("  {note}"), theme::muted())), Rect { y: st.bottom().saturating_sub(1), height: 1, ..st });
        }

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
            self.draw_nav(f, Rect { height: nav_h.min(full.height), ..full });
        }
        // bottom up: footer, the bubble's hint row, the input bubble (its top border: the header's
        // bottom line), the header (agents + tokens box), the title row, monitors, the stream
        let foot = Rect { y: area.bottom().saturating_sub(1), height: 1, ..area };
        let hint = Rect { y: foot.y.saturating_sub(1), ..foot };
        let in_w = input_width(area.width).max(1);
        let n = (self.input.rows_for(in_w) as u16).clamp(1, INPUT_ROWS);
        let bubble_bottom = hint.y.saturating_sub(1);
        let in_top = bubble_bottom.saturating_sub(n);
        let head_rows = header_rows(self.members.len());
        // the agents box + cost box: chat only (the changes view takes the room)
        let head_h = if area.height >= MIN_HEADER_SCREEN && !self.members.is_empty() && self.pane == Pane::Chat { head_rows + 1 } else { 0 };
        // the bubble's top border: the header's bottom line (shared), else a row of its own
        let bubble_top = in_top.saturating_sub(1);
        let header = Rect { y: (bubble_top + 1).saturating_sub(head_h), height: head_h, ..area };
        let title = Rect { y: if head_h > 0 { header.y } else { bubble_top }.saturating_sub(1), height: 1, ..area };
        let mon_h = self.monitors.rows(area.width, area.height / 3);
        let mon = Rect { y: title.y.saturating_sub(mon_h), height: mon_h, ..area };
        let st = Rect { x: area.x, y: area.y, width: area.width, height: mon.y.saturating_sub(area.y) };
        self.stream_rect = st;

        if self.pane == Pane::Chat {
            self.draw_stream(f, st);
        } else {
            self.changes.draw(f, st);
        }
        if mon_h > 0 {
            self.monitors.render(f, mon, self.tick());
        }

        // title row: the view bar (views of the thread), the thread title, the stream's position, the pills
        let note = if self.back > 0 && self.pane == Pane::Chat { format!("↑{} · esc: live ", self.back) } else { String::new() };
        let mut spans: Vec<Span<'static>> = Vec::new();
        self.bar_rects.clear();
        let mut x = title.x + 2;
        for p in Pane::ALL {
            let label = match p {
                Pane::Chat if !self.thread.asks.is_empty() => format!(" chat ?{} ", self.thread.asks.len()),
                Pane::Chat => " chat ".to_string(),
                Pane::Changes => {
                    let (done, all) = self.changes.progress();
                    let n = self.changes.comment_count();
                    let c = if n > 0 { format!("✎{n} ") } else { String::new() };
                    if all == 0 { format!(" changes {c}") } else { format!(" changes {done}/{all} {c}") }
                }
            };
            let st = if p == self.pane { theme::pill() } else { theme::muted() };
            let w = unicode_width::UnicodeWidthStr::width(label.as_str()) as u16;
            self.bar_rects.push((Rect { x, y: title.y, width: w, height: 1 }, p));
            x += w + 1;
            spans.push(Span::styled(label, st));
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled("│ ", theme::line()));
        let used: usize = spans.iter().map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref())).sum();
        let room = usize::from(title.width.saturating_sub(4)).saturating_sub(note.chars().count() + 1 + used);
        spans.push(Span::styled(format!("{} ", crate::nav::fit(&self.title, room)), theme::accent_bold()));
        if !note.is_empty() {
            spans.push(Span::styled(note, theme::muted()));
        }
        if let Some((new, old)) = &self.replace {
            spans.push(Span::styled(format!(" on send: {new} replaces {old} "), theme::pill()));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), Rect { x: title.x + 2, width: title.width.saturating_sub(2), ..title });

        // header: agents box + tokens box (the cost pane inside)
        let show_cost = head_h > 0 && area.width >= MIN_COST_WIDTH;
        let cost = if show_cost {
            let cards: Vec<Card> = self.members.iter().map(|n| self.card(n)).collect();
            let (all, first) = (cards_w(&cards), cards_w(&cards[..cards.len().min(1)]));
            let room = (header.width + 1).saturating_sub(all);
            let min = MIN_COST_BOX.min((header.width + 1).saturating_sub(first)).max(SQUEEZED_COST_BOX);
            let w = room.clamp(min, MAX_COST_BOX).min(header.width);
            Rect { x: header.right() - w + 2, y: header.y + 1, width: w.saturating_sub(4), height: head_h.saturating_sub(2) }
        } else {
            Rect { width: 0, ..header }
        };
        self.draw_header(f, header, cost);
        if show_cost {
            if self.cost_at.elapsed() > Duration::from_secs(3) {
                self.cost_at = Instant::now();
                let (id, tx) = (self.id.clone(), self.tx.clone());
                std::thread::spawn(move || {
                    let _ = tx.send(Msg::Cost(Box::new(crate::cost::snapshot(&id))));
                });
            }
            if let Some(s) = &self.cost {
                crate::cost::draw_in(f, cost, s, self.cost_at.elapsed());
            }
        }

        // the input bubble: rounded, its top border the header's bottom line (├ ┴ ┤)
        let bubble = Rect { y: bubble_top, height: bubble_bottom + 1 - bubble_top, ..area };
        let edge = Style::default().fg(theme::pal().hover);
        {
            let buf = f.buffer_mut();
            let (left, right) = (bubble.x, bubble.right().saturating_sub(1));
            for x in left..=right {
                let top = buf[(x, bubble_top)].symbol().to_string();
                let sym = match top.as_str() {
                    _ if x == left => if head_h > 0 { "├" } else { "╭" },
                    _ if x == right => if head_h > 0 { "┤" } else { "╮" },
                    "╰" | "╯" | "┴" if head_h > 0 => "┴",
                    _ => "─",
                };
                buf[(x, bubble_top)].reset();
                buf[(x, bubble_top)].set_symbol(sym).set_style(edge);
                buf[(x, bubble_bottom)].reset();
                buf[(x, bubble_bottom)].set_symbol(if x == left { "╰" } else if x == right { "╯" } else { "─" }).set_style(edge);
            }
            for y in in_top..bubble_bottom {
                buf[(left, y)].set_symbol("│").set_style(edge);
                buf[(right, y)].set_symbol("│").set_style(edge);
            }
        }
        // asks: what the keys do now, in the bubble's bottom border
        if !self.thread.asks.is_empty() && self.input.is_empty() {
            let label = " ↑↓ · 1-9: pick · enter: answer ";
            let lw = label.chars().count() as u16;
            if bubble.width > lw + 6 {
                f.render_widget(Paragraph::new(Span::styled(label, theme::accent())), Rect { x: bubble.right() - 3 - lw, y: bubble_bottom, width: lw, height: 1 });
            }
        }
        self.input_rect = Rect { x: area.x + 2, y: in_top, width: in_w, height: n };
        let menu_open = self.menu.is_some();
        // an open ask with the focus: 0 cursor in the prompt (its keys go to the ask)
        let ask_keys = !self.thread.asks.is_empty() && self.ask.focus && !self.ask.sent;
        let prompt_keys = self.pane == Pane::Chat || self.prompt_focus;
        self.input.render(f, self.input_rect, !menu_open && !ask_keys && prompt_keys);

        // hint row: who gets the message, the send keys
        let mut to = vec![Span::styled("  → ", theme::muted())];
        for (i, name) in self.recipients().into_iter().enumerate() {
            if i > 0 {
                to.push(Span::styled(", ", theme::muted()));
            }
            let c = self.colors.get(&name).copied().unwrap_or(theme::pal().accent);
            to.push(Span::styled(name.to_lowercase(), Style::default().fg(c)));
        }
        if self.members.is_empty() {
            to.clear();
        }
        f.render_widget(Paragraph::new(Line::from(to)), hint);
        f.render_widget(Paragraph::new(Line::styled("⌘/ctrl+⏎ send · ↑↓ history ", theme::muted())).right_aligned(), Rect { x: hint.x + hint.width / 2, width: hint.width - hint.width / 2, ..hint });

        // footer: keys or toast right
        let right = match &self.toast {
            Some((m, at)) if at.elapsed() < Duration::from_secs(3) => Span::styled(format!("{m} "), theme::accent()),
            _ if self.nav_on => Span::styled("←→: pick thread · enter: open · tab / esc: close ", theme::accent()),
            _ if let Some((file, line, _)) = &self.commenting => Span::styled(format!("comment on {file}:{line} · ⌘/ctrl+⏎: add to the batch · esc: cancel "), theme::accent()),
            _ if self.pane == Pane::Changes && !self.prompt_focus => Span::styled("j k · space: reviewed · c: comment · C: send · o: IDE · s: layout · i: prompt ", theme::muted()),
            _ if self.pane == Pane::Changes => Span::styled("esc: back to the changes · shift+tab: chat ", theme::muted()),
            _ => Span::styled(format!("tab: threads · shift+tab: changes · esc: interrupt · esc esc: codehead · {}: menu ", crate::config::menu_key().label), theme::muted()),
        };
        f.render_widget(Paragraph::new(Line::from(right)).right_aligned(), foot);

        // dropdowns over everything, then the menu
        if !menu_open {
            self.mention.render(f, &self.input, &self.roster, area);
            self.command.render(f, &self.input, area, self.input_rect);
            self.paths.render(f, &self.input, area, self.input_rect);
        }
        if let Some(m) = self.menu.as_mut() {
            f.render_widget(Clear, Rect::default());
            m.render(f, area);
        }
    }
}

/// Header card: sprite, color, the lines right of the sprite, width.
type Card = (crate::sprites::Sprite, Color, Vec<Vec<Span<'static>>>, u16);

/// The top row of a stream of TOTAL rows, BACK rows scrolled back, H rows high, as (item, row in
/// the item). STARTS: the 1st row of each item.
fn top_anchor(starts: &[usize], total: usize, back: usize, h: usize) -> Option<(usize, usize)> {
    let top = total.saturating_sub(back).saturating_sub(h);
    let item = starts.partition_point(|&s| s <= top).checked_sub(1)?;
    Some((item, top - starts[item]))
}

/// `back` that puts ANCHOR (item, row in the item) on the top row again: 0 at the bottom.
fn back_for(starts: &[usize], total: usize, (item, off): (usize, usize), h: usize) -> usize {
    let Some(&start) = starts.get(item) else { return 0 };
    // a folded item: its last row
    let next = starts.get(item + 1).copied().unwrap_or(total);
    let top = (start + off).min(next.saturating_sub(1).max(start));
    total.saturating_sub(top + h)
}

/// Agents box of CARDS: left border + 1 space, the cards, 2 spaces + right border (the last card's
/// 4 gap columns hold 3 of those). 0 cards: 0.
fn cards_w(cards: &[Card]) -> u16 {
    if cards.is_empty() { 0 } else { cards.iter().map(|c| c.3).sum::<u16>() + 1 }
}

/// Header rows (inside its frame + its top border) for N agents.
fn header_rows(n: usize) -> u16 {
    let n = n.max(1) as u16;
    (3 + n + u16::from(n > 1)).clamp(HEADER_H, MAX_HEADER_H)
}

/// Width of the input in its bubble: `│ ` left, ` │` right.
fn input_width(cols: u16) -> u16 {
    cols.saturating_sub(4)
}

/// WORD with its 1st letter upper case: `claude` → `Claude`.
fn title_case(word: &str) -> String {
    let mut c = word.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

/// Effort level as the header shows it: `medium` → `Medium`, `xhigh` → `XHigh`.
fn effort_label(level: &str) -> String {
    if level == "xhigh" { "XHigh".into() } else { title_case(level) }
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
/// Ends the subscriber thread of a view when the view ends.
struct Unsub(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl Drop for Unsub {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Lines of the host of thread ID (`live`, `delta`, `wake`) → TX. 0 host: a retry each second.
/// Never starts a host.
fn subscribe(id: &str, tx: Sender<Msg>) -> Unsub {
    use std::io::{BufRead, BufReader, Write};
    use std::sync::atomic::Ordering;
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done = stop.clone();
    let path = crate::engine::host::socket(id);
    std::thread::spawn(move || {
        while !done.load(Ordering::Relaxed) {
            if let Ok(mut s) = std::os::unix::net::UnixStream::connect(&path)
                && s.set_read_timeout(Some(Duration::from_secs(1))).is_ok()
                && writeln!(s, "{}", json!({"op": "subscribe"})).is_ok()
            {
                // events before the subscription: read them
                if tx.send(Msg::Subscribed).is_err() {
                    return;
                }
                let mut r = BufReader::new(s);
                let mut line = String::new();
                while !done.load(Ordering::Relaxed) {
                    match r.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            let v: Value = serde_json::from_str(line.trim()).unwrap_or_default();
                            line.clear();
                            let m = match v["op"].as_str() {
                                Some("wake") => Msg::Wake,
                                Some("delta") => Msg::Delta(v["agent"].as_str().unwrap_or_default().into(), v["text"].as_str().unwrap_or_default().into()),
                                Some("live") => Msg::Live(v["agent"].as_str().unwrap_or_default().into(), v["text"].as_str().unwrap_or_default().into()),
                                _ => continue,
                            };
                            if tx.send(m).is_err() {
                                return;
                            }
                        }
                        // the read timeout: a check of `done`, the partial line stays in LINE
                        Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                        Err(_) => break,
                    }
                }
                continue;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });
    Unsub(stop)
}

pub fn run<B: Backend>(term: &mut Terminal<B>, id: &str, cwd: &Path, title: &str, note: Option<&str>, _first: &str) -> Result<Exit, String> {
    if store::load(id).is_none() {
        return Err(format!("no thread {id}"));
    }
    let (tx, rx): (Sender<Msg>, Receiver<Msg>) = mpsc::channel();
    let _sub = subscribe(id, tx.clone());
    let mut v = View::new(id, cwd, title, tx);
    if let Some(n) = note.filter(|n| !n.is_empty()) {
        v.toast(n.to_string());
    }
    crate::nav::mark_seen_now(id);
    let mut roster_at = Instant::now();
    let mut drawn = Instant::now() - Duration::from_secs(1);
    let mut dirty = true;
    // the tail: on a `wake` of the host; each second as a fallback (0 host, a lost line)
    let mut wake = true;
    let mut read_at = Instant::now();
    loop {
        // a frame on change; animations (wave, spinners, toast) at 150 ms; else 1 per second
        // (clock, cost box, sending timer)
        let every = if v.animating() { Duration::from_millis(150) } else { Duration::from_secs(1) };
        if dirty || drawn.elapsed() >= every {
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
                // the answer failed: the picker takes keys again
                Msg::Done("answer", Err(e)) => {
                    v.ask.sent = false;
                    v.toast(format!("answer: {e}"));
                }
                Msg::Done(what, Err(e)) => v.toast(format!("{what}: {e}")),
                Msg::Done("join" | "leave" | "replace", Ok(_)) => {
                    let r = v.roster.clone();
                    v.set_roster(r);
                }
                Msg::Done(..) => {}
                Msg::Cost(s) => v.cost = Some(*s),
                Msg::Wake => wake = true,
                Msg::Subscribed => {
                    v.live.clear();
                    v.dirty = true;
                    wake = true;
                }
                Msg::Live(agent, text) => {
                    v.live.retain(|(a, _)| *a != agent);
                    v.live.push((agent, text));
                    v.dirty = true;
                }
                Msg::Delta(agent, text) => {
                    match v.live.iter_mut().find(|(a, _)| *a == agent) {
                        Some((_, t)) => t.push_str(&text),
                        None => v.live.push((agent, text)),
                    }
                    v.dirty = true;
                }
            }
        }
        dirty |= v.tick_esc();
        if wake || read_at.elapsed() >= Duration::from_secs(1) {
            wake = false;
            read_at = Instant::now();
            dirty |= v.read();
        }
        dirty |= v.monitors.tick(id);
        if roster_at.elapsed() > Duration::from_secs(5) {
            roster_at = Instant::now();
            v.set_roster(crate::agents::load());
            crate::nav::mark_seen(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrolled_back_keeps_the_top_row_on_new_rows_and_folds() {
        // 3 items of 10 rows; 30 rows, 5 high, 12 back: top row 13 = item 1, row 3
        let starts = [0, 10, 20];
        let a = top_anchor(&starts, 30, 12, 5).unwrap();
        assert_eq!(a, (1, 3));
        // 8 new rows at the end: back grows by 8
        assert_eq!(back_for(&starts, 38, a, 5), 20);
        // item 0 opened (+6 rows): the anchor moves with item 1
        assert_eq!(back_for(&[0, 16, 26], 36, a, 5), 12);
        // item 1 folded to 2 rows: its last row on top
        assert_eq!(back_for(&[0, 10, 12], 22, a, 5), 6);
        // the anchor near the end: 0 = follow the bottom
        assert_eq!(back_for(&starts, 30, (2, 9), 5), 0);
        assert_eq!(top_anchor(&[], 0, 3, 5), None);
    }
}


/// The open ask in the view: key routing (prompt text or not), drawing, clicks.
#[cfg(test)]
mod ask_tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn view(multi: bool) -> View {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("asktest-0000", Path::new("/tmp"), "t", tx);
        let input = json!({"questions": [{"question": "Which keys?", "header": "Keys", "multiSelect": multi, "options": [
            {"label": "alpha", "description": "1st"}, {"label": "beta", "description": "2nd"}, {"label": "gamma", "description": "3rd"}]}]});
        v.thread.push(&store::Event { ts: 1, agent: Some("ada".into()), kind: store::Kind::Ask { id: "q1".into(), tool: "AskUserQuestion".into(), input } });
        v.ask.sync(v.thread.asks.first());
        v
    }

    fn press(v: &mut View, codes: &[KeyCode]) {
        for &c in codes {
            v.key(KeyEvent::new(c, KeyModifiers::NONE));
        }
    }

    fn screen(v: &mut View) -> Vec<String> {
        let mut term = Terminal::new(TestBackend::new(90, 40)).unwrap();
        term.draw(|f| v.draw(f)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>()).collect()
    }

    fn answers(v: &View) -> Vec<Value> {
        v.ops.borrow().iter().filter(|o| o["op"] == "answer").cloned().collect()
    }

    #[test]
    fn text_in_the_prompt_does_not_block_the_ask() {
        let mut v = view(true);
        v.input.set_text("a kept draft");
        press(&mut v, &[KeyCode::Down, KeyCode::Char(' '), KeyCode::Down, KeyCode::Char(' '), KeyCode::Enter]);
        let a = answers(&v);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0]["answers"]["Which keys?"], "beta, gamma");
        // the draft: untouched
        assert_eq!(v.input.text(), "a kept draft");
        // sent: the next keys go to the prompt, 0 2nd answer
        press(&mut v, &[KeyCode::Enter, KeyCode::Char('!')]);
        assert_eq!(answers(&v).len(), 1);
        assert!(v.input.text().ends_with('!'));
    }

    #[test]
    fn tab_gives_the_keys_to_the_prompt_and_back() {
        let mut v = view(true);
        press(&mut v, &[KeyCode::Tab, KeyCode::Char('h'), KeyCode::Char('i'), KeyCode::Char(' ')]);
        assert_eq!(v.input.text(), "hi ");
        assert!(v.ask.rows(v.thread.asks.first().unwrap()).iter().all(|r| !r.picked));
        // tab: back to the ask (prompt text or not); space picks there
        press(&mut v, &[KeyCode::Tab, KeyCode::Char(' ')]);
        assert!(v.ask.rows(v.thread.asks.first().unwrap())[0].picked);
        assert_eq!(v.input.text(), "hi ");
        assert!(!v.nav_on, "tab with an open ask: 0 thread header");
    }

    #[test]
    fn the_ask_box_shows_boxes_cursor_other_and_hint() {
        let mut v = view(true);
        v.input.set_text("draft");
        press(&mut v, &[KeyCode::Down, KeyCode::Char(' ')]);
        let s = screen(&mut v).join("\n");
        assert!(s.contains("[ ] 1. alpha"), "{s}");
        assert!(s.contains("› [x] 2. beta"), "{s}");
        assert!(s.contains("[ ] 4. Other: type an answer"), "{s}");
        assert!(s.contains("space: pick · enter: answer · type: other · tab: prompt"), "{s}");
        // letters: into Other, the cursor there
        press(&mut v, &[KeyCode::Char('z'), KeyCode::Char('z')]);
        let s = screen(&mut v).join("\n");
        assert!(s.contains("› [x] 4. Other: zz▏"), "{s}");
        // focus on the prompt: 0 cursor row, the hint says how back
        press(&mut v, &[KeyCode::Tab]);
        let s = screen(&mut v).join("\n");
        assert!(!s.contains("› [x]") && s.contains("tab: back to the question"), "{s}");
    }

    #[test]
    fn single_pick_answers_with_space_enter_digits_or_a_click() {
        let mut v = view(false);
        press(&mut v, &[KeyCode::Down, KeyCode::Down, KeyCode::Char(' ')]);
        assert_eq!(answers(&v)[0]["answers"]["Which keys?"], "gamma");
        let mut v = view(false);
        press(&mut v, &[KeyCode::Char('2')]);
        assert_eq!(answers(&v)[0]["answers"]["Which keys?"], "beta");
        // a click on the row of "alpha"
        let mut v = view(false);
        let rows = screen(&mut v);
        let y = rows.iter().position(|r| r.contains("1. alpha")).unwrap() as u16;
        v.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 10, row: y, modifiers: KeyModifiers::NONE });
        assert_eq!(answers(&v)[0]["answers"]["Which keys?"], "alpha");
    }

    #[test]
    fn a_pending_send_colors_its_mentions() {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("asktest-0002", Path::new("/tmp"), "t", tx);
        v.names = vec![("ada".into(), Color::Rgb(200, 10, 10))];
        v.pending.push(("@ada go on".into(), Instant::now(), None));
        let mut term = Terminal::new(TestBackend::new(60, 20)).unwrap();
        term.draw(|f| v.draw(f)).unwrap();
        let b = term.backend().buffer().clone();
        let (x, y) = (0..20u16).find_map(|y| (0..60u16).find(|&x| b[(x, y)].symbol() == "@" && b[(x + 1, y)].symbol() == "a").map(|x| (x, y))).expect("0 @ada on screen");
        assert!((x..x + 4).all(|x| b[(x, y)].fg == Color::Rgb(200, 10, 10)));
        assert_ne!(b[(x + 5, y)].fg, Color::Rgb(200, 10, 10));
    }

    #[test]
    fn ctrl_s_never_sends_ctrl_enter_does() {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("asktest-0001", Path::new("/tmp"), "t", tx);
        v.input.set_text("hello agents");
        v.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(v.ops.borrow().is_empty());
        assert_eq!(v.input.text(), "hello agents");
        assert!(v.toast.as_ref().is_some_and(|(m, _)| m.contains("ctrl+enter")));
        v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        assert!(v.ops.borrow().iter().any(|o| o["op"] == "prompt" && o["text"].as_str().is_some_and(|t| t.contains("hello agents"))), "{:?}", v.ops.borrow());
    }

    #[test]
    fn esc_and_the_menu_key_stay_the_views() {
        let mut v = view(false);
        press(&mut v, &[KeyCode::Esc]);
        assert!(v.last_esc.is_some());
        assert!(answers(&v).is_empty());
        // ctrl+c on an empty prompt: the quit arm, 0 ask key
        v.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(v.last_ctrl_c.is_some());
    }
}

/// The thread header: card strip, hidden counts, tab toggle, ←→ pick, enter switch.
#[cfg(test)]
mod nav_tests {
    use super::*;
    use crate::nav::Card;
    use ratatui::backend::TestBackend;

    fn card(id: &str, act: u64) -> Card {
        Card { id: id.into(), title: format!("thread {id}"), state: "open".into(), live: 1, activity: act, ..Default::default() }
    }

    fn view() -> View {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("a", Path::new("/tmp"), "thread a", tx);
        v.nav_first = "a".into();
        v.nav_cards = vec![card("b", 50), card("c", 40), card("a", 30), card("d", 20), card("e", 10)];
        v.nav_cards[1].unread = true;
        v.nav_cards[0].busy = true;
        // 0 background read: the cards above stay
        v.nav_time = Instant::now();
        v
    }

    fn draw(v: &mut View) -> Vec<String> {
        let mut t = Terminal::new(TestBackend::new(80, 5)).unwrap();
        t.draw(|f| v.draw_nav(f, Rect::new(0, 0, 80, 5))).unwrap();
        let b = t.backend().buffer().clone();
        (0..5).map(|y| (0..80).map(|x| b[(x, y)].symbol().to_string()).collect::<String>()).collect()
    }

    #[test]
    fn header_cards_fit_titles_and_count_the_hidden_ones() {
        let mut v = view();
        let order: Vec<String> = v.nav_order().into_iter().map(|c| c.id).collect();
        assert_eq!(order, ["a", "b", "c", "d", "e"]);
        let rows = draw(&mut v);
        let s = rows.join("\n");
        // cards fit their titles: 16 columns each, the 1st at column 0
        assert!(rows[0].starts_with("╭ here ────────╮ ╭"), "{s}");
        // the 5th card cut at the right edge: open there, its count below
        assert!(rows[1].trim_end().ends_with("thread e") && !rows[0].ends_with('╮') && rows[4].trim_end().ends_with("+1"), "{s}");
        assert!(s.contains("working…") && s.contains('●'), "{s}");
        // → × 4: the cursor on e, the strip scrolls: the 1st card cut on the left
        for _ in 0..4 {
            v.nav_step(1);
        }
        let rows = draw(&mut v);
        let s = rows.join("\n");
        assert!(rows[1].contains("thread e") && rows[4].starts_with("1+"), "{s}");
        assert!(!rows[0].starts_with('╭'), "the left card open: {s}");
        // → on the last card: wraps to the 1st
        v.nav_step(1);
        assert_eq!(v.nav_at, 0);
    }

    #[test]
    fn tab_toggles_the_header_with_prompt_text_and_enter_switches() {
        let mut v = view();
        v.input.set_text("half a message");
        v.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(v.nav_on, "tab with prompt text: header open");
        assert_eq!(v.input.text(), "half a message");
        v.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(!v.nav_on, "tab again: header closed");
        // shift+tab: the next view of the thread, 0 header
        v.key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert!(!v.nav_on && v.pane == Pane::Changes, "shift+tab: changes view");
        v.key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(v.pane, Pane::Chat);
        v.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(v.nav_on, "tab: header open");
        // enter on this thread: the header closes, the view stays
        assert!(!v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        assert!(!v.nav_on);
        // → → enter: switch to c
        v.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        v.nav_cards = vec![card("b", 50), card("c", 40), card("a", 30)];
        v.key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        v.key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert!(v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        assert!(matches!(&v.exit, Exit::Open { id, .. } if id == "c"));
        reset_nav();
    }
}

/// Views of a thread: the view bar, keys to the changes view, the prompt's focus.
#[cfg(test)]
mod pane_tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn view() -> View {
        let (tx, _rx) = mpsc::channel();
        View::new("panetest-0000", Path::new("/tmp"), "thread p", tx)
    }

    fn press(v: &mut View, k: KeyCode, m: KeyModifiers) -> bool {
        v.key(KeyEvent::new(k, m))
    }

    fn screen(v: &mut View) -> String {
        let mut t = Terminal::new(TestBackend::new(100, 30)).unwrap();
        t.draw(|f| v.draw(f)).unwrap();
        let b = t.backend().buffer().clone();
        (0..30).map(|y| (0..100).map(|x| b[(x, y)].symbol().to_string()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn view_bar_shows_both_views_and_alt_digits_switch() {
        let mut v = view();
        let s = screen(&mut v);
        assert!(s.contains(" chat   changes  │ thread p"), "{s}");
        assert!(s.contains("shift+tab: changes"), "{s}");
        press(&mut v, KeyCode::Char('2'), KeyModifiers::ALT);
        assert_eq!(v.pane, Pane::Changes);
        let s = screen(&mut v);
        assert!(s.contains("space: reviewed") && s.contains("i: prompt"), "{s}");
        press(&mut v, KeyCode::Char('1'), KeyModifiers::ALT);
        assert_eq!(v.pane, Pane::Chat);
    }

    #[test]
    fn changes_view_keeps_letters_from_the_prompt_until_i() {
        let mut v = view();
        press(&mut v, KeyCode::BackTab, KeyModifiers::SHIFT);
        press(&mut v, KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(v.input.text(), "", "j: a view key");
        press(&mut v, KeyCode::Char('i'), KeyModifiers::NONE);
        assert!(v.prompt_focus);
        press(&mut v, KeyCode::Char('h'), KeyModifiers::NONE);
        assert_eq!(v.input.text(), "h");
        // esc: the keys back to the view, the text stays
        press(&mut v, KeyCode::Esc, KeyModifiers::NONE);
        assert!(!v.prompt_focus && v.input.text() == "h");
        assert!(v.last_esc.is_none(), "0 interrupt / back timer");
    }
}

#[cfg(test)]
mod comment_tests {
    use super::*;

    #[test]
    fn a_comment_borrows_the_prompt_and_gives_the_draft_back() {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("commenttest-0000", Path::new("/tmp"), "t", tx);
        v.set_pane(Pane::Changes);
        v.input.set_text("half a message");
        v.changes_act(crate::changes::Act::Comment("src/a.rs".into(), 12));
        assert!(v.prompt_focus && v.input.text().is_empty());
        v.key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE));
        v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        assert_eq!(v.changes.comment_count(), 1);
        assert!(!v.prompt_focus && v.input.text() == "half a message" && v.commenting.is_none());
        // esc cancels: 0 comment, the draft back
        v.changes_act(crate::changes::Act::Comment("src/a.rs".into(), 13));
        v.key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        v.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(v.changes.comment_count(), 1);
        assert_eq!(v.input.text(), "half a message");
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("commenttest-0000"));
    }
}
