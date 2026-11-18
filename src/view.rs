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

/// Views of a thread: the view bar on row 0 (alt/option+←→: previous / next, alt+1-3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pane {
    #[default]
    Chat,
    Changes,
    Files,
}

impl Pane {
    const ALL: [Pane; 3] = [Pane::Chat, Pane::Changes, Pane::Files];
}


/// The changes view gets the chat left of it from this many columns.
const SPLIT_MIN: u16 = 200;
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
    files: crate::files::View,
    /// yazi on this path: the view loop runs it full screen (codehead suspends).
    yazi: Option<PathBuf>,
    /// A program to run full screen (a terminal editor): the view loop runs it.
    outside: Option<Vec<std::ffi::OsString>>,
    prompt_focus: bool,
    bar_rects: Vec<(Rect, Pane)>,
    /// The changes view with the chat left of it (wide terminals; alt+\ toggles); the view's area.
    split: bool,
    view_rect: Rect,
    /// portless: the live URLs of this thread's worktree (read every 5 s), the read on its way, the
    /// link on row 0.
    urls: Vec<String>,
    urls_read: Option<Receiver<Vec<String>>>,
    urls_at: Option<Instant>,
    url_rect: Option<(Rect, String)>,
    /// A review comment in the prompt: the lines it is on, the draft the prompt had before.
    commenting: Option<(Vec<crate::changes::Spot>, String)>,
    /// A commit message in the prompt: the draft the prompt had before; a model's draft on its way.
    committing: Option<String>,
    drafting: Option<Receiver<Result<String, String>>>,
    /// `> handoff`: the document on its way.
    handing: Option<Receiver<Result<String, String>>>,
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
    /// A menu click switched threads: the view ends with EXIT.
    mouse_exit: bool,
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
            files: crate::files::View::new(id, cwd),
            yazi: None,
            outside: None,
            prompt_focus: false,
            bar_rects: Vec::new(),
            split: true,
            view_rect: Rect::default(),
            urls: Vec::new(),
            urls_read: None,
            urls_at: None,
            url_rect: None,
            commenting: None,
            committing: None,
            drafting: None,
            handing: None,
            // tests: 0 shared state between parallel views
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
            mouse_exit: false,
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
        // a commit message: commit the staged files, the draft back
        if let Some(draft) = self.committing.take() {
            let msg = self.input.text().trim().to_string();
            if msg.is_empty() {
                self.committing = Some(draft);
                self.toast("0 message · ctrl+g: draft one");
                return;
            }
            let act = self.changes.start_op(crate::changes::Op::Commit(msg));
            self.input.set_text(&draft);
            self.prompt_focus = false;
            self.drafting = None;
            self.changes_act(act);
            return;
        }
        // a review comment: into the batch, the draft back, the keys back to the changes view
        if let Some((spots, draft)) = self.commenting.take() {
            let text = self.input.text();
            self.changes.add_comment(&spots, &text);
            self.input.set_text(&draft);
            self.prompt_focus = false;
            let on = spots.iter().map(|s| s.label()).collect::<Vec<_>>().join(", ");
            self.toast(format!("comment on {on} · {} in the batch · C: send", self.changes.comment_count()));
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
        self.send_prompt(text);
    }

    /// An owner message: TEXT to its @names, else to the agents of the owner's last message.
    fn send_prompt(&mut self, text: String) {
        // @name of any agent: the host routes (and brings a new one in); else the default
        let names: Vec<String> = self.names.iter().map(|n| n.0.clone()).chain(self.members.iter().cloned()).collect();
        let named = crate::engine::host::mentions(&text, &names);
        self.pending.push((text.clone(), Instant::now(), None));
        if named.is_empty() {
            self.host("send", json!({"op": "prompt", "text": text, "to": self.default_to()}));
        } else {
            self.host("send", json!({"op": "prompt", "text": text}));
        }
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

    /// Plain text of the selection: the message's text only (0 borders, 0 time, 0 copy button);
    /// rows a wrap split joined again (Row::wrap), so the copy has the source's lines.
    fn selected(&self) -> Option<String> {
        let ((r0, c0), (r1, c1)) = self.sel?;
        let ((r0, c0), (r1, c1)) = if (r0, c0) <= (r1, c1) { ((r0, c0), (r1, c1)) } else { ((r1, c1), (r0, c0)) };
        let mut out: Vec<String> = Vec::new();
        for r in r0..=r1.min(self.drawn.rows.len().saturating_sub(1)) {
            let row = &self.drawn.rows[r];
            let text: String = row.line.spans.iter().map(|s| s.content.as_ref()).collect();
            let Some((a, b)) = content_cols(&text) else { continue };
            let chars: Vec<char> = text.chars().collect();
            let a = if r == r0 { a.max(usize::from(c0)) } else { a };
            let b = if r == r1 { b.min(usize::from(c1) + 1) } else { b };
            let piece: String = chars.get(a.min(b)..b).map(|c| c.iter().collect()).unwrap_or_default();
            let piece = piece.trim_end();
            let piece = piece.strip_prefix("↪ ").or_else(|| piece.trim_start().strip_prefix("↪ ")).unwrap_or(piece);
            match out.last_mut() {
                Some(last) if row.wrap == 1 && r != r0 => {
                    last.push(' ');
                    last.push_str(piece.trim_start());
                }
                Some(last) if row.wrap == 2 && r != r0 => last.push_str(piece.trim_start()),
                _ => out.push(piece.to_string()),
            }
        }
        // the common indent of the lines off; blank lines at the ends off
        let indent = out.iter().filter(|l| !l.trim().is_empty()).map(|l| l.len() - l.trim_start().len()).min().unwrap_or(0);
        let lines: Vec<String> = out.iter().map(|l| l.get(indent..).unwrap_or("").to_string()).collect();
        let t = lines.join("\n").trim_matches('\n').to_string();
        (!t.trim().is_empty()).then_some(t)
    }

    /// The rows of the message (stream item) at ROW: first, last.
    fn item_rows(&self, row: usize) -> (usize, usize) {
        let s = &self.drawn.starts;
        let last = self.drawn.rows.len().saturating_sub(1);
        match s.partition_point(|&x| x <= row).checked_sub(1) {
            Some(i) => (s[i], s.get(i + 1).map_or(last, |n| n.saturating_sub(1)).max(s[i])),
            None => (0, s.first().map_or(last, |n| n.saturating_sub(1))),
        }
    }

    fn copy(&mut self, text: &str) {
        let ok = crate::clip::copy(text);
        self.toast(if ok { format!("copied {} chars", text.chars().count()) } else { "0 clipboard tools found (wl-copy, xclip, xsel)".into() });
    }

    /// Something on screen moves: an agent at work (wave), monitors, the threads row, a toast.
    fn animating(&self) -> bool {
        !self.thread.busy.is_empty() || self.monitors.active() || self.toast.as_ref().is_some_and(|(_, at)| at.elapsed() < Duration::from_secs(4))
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

    /// `> handoff`: the review model writes the handoff document (on a thread); its end opens
    /// the new thread with the document as its prompt draft.
    fn handoff(&mut self) {
        if !self.thread.busy.is_empty() {
            self.toast("handoff: wait for the turn to end");
            return;
        }
        if self.handing.is_some() {
            self.toast("handoff: on its way");
            return;
        }
        let Some(model) = crate::config::review_model() else { return self.toast("handoff: the review model is off (setting review)") };
        let (tx, rx) = mpsc::channel();
        let id = self.id.clone();
        std::thread::spawn(move || {
            let _ = tx.send(crate::handoff::write(&id, &model));
        });
        self.handing = Some(rx);
        self.toast("handoff: writing the document…");
    }

    /// The handoff document came: the new thread opens. True: the view ends.
    fn handoff_due(&mut self) -> bool {
        let Some(rx) = &self.handing else { return false };
        let Ok(r) = rx.try_recv() else { return false };
        self.handing = None;
        let started = r.and_then(|doc| store::load(&self.id).ok_or_else(|| "thread gone".to_string()).and_then(|old| crate::handoff::start(&old, &doc)));
        match started {
            Ok(t) => {
                self.exit = Exit::Open { id: t.id.clone(), dir: t.work_dir(), title: t.title.clone(), note: "handoff: edit the document, add the next goal, send".into() };
                true
            }
            Err(e) => {
                self.toast(format!("handoff: {e}"));
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
        // views of the thread: F1-F3 (alt+1-3 too)
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        if let KeyCode::F(n @ 1..=9) = k.code {
            if let Some(p) = Pane::ALL.get(usize::from(n - 1)) {
                self.set_pane(*p);
            }
            return false;
        }
        // alt+\: the chat next to the changes view on / off (wide terminals)
        if alt && !ctrl && k.code == KeyCode::Char('\\') {
            self.split = !self.split;
            self.dirty = true;
            self.toast(if self.split { "changes: the chat on the left (≥ 200 columns)" } else { "changes: full width" });
            return false;
        }
        if alt && !ctrl && let KeyCode::Char(c @ '1'..='9') = k.code {
            if let Some(p) = Pane::ALL.get(usize::from(c as u8 - b'1')) {
                self.set_pane(*p);
            }
            return false;
        }
        // the files view's filter input: every key but ctrl+c (esc: clears it)
        if self.pane == Pane::Files && !self.prompt_focus && self.files.typing() && !(ctrl && k.code == KeyCode::Char('c')) {
            let act = self.files.key(k);
            self.files_act(act);
            return false;
        }
        // the changes view with the focus: its keys (esc, tab, ctrl+c and the menu key: as in chat)
        if self.pane != Pane::Chat && !self.prompt_focus {
            // the changes view's filter, selection: esc is the view's
            let esc_here = self.pane == Pane::Changes && k.code == KeyCode::Esc && self.changes.takes_esc();
            let through = (matches!(k.code, KeyCode::Esc | KeyCode::Tab) && !esc_here) || (ctrl && matches!(k.code, KeyCode::Char('c') | KeyCode::Char('y'))) || crate::config::menu_key().matches(&k);
            if !through {
                if k.code == KeyCode::Char('i') && !ctrl && !alt && !(self.pane == Pane::Changes && self.changes.typing()) {
                    self.prompt_focus = true;
                    return false;
                }
                if self.pane == Pane::Files {
                    let act = self.files.key(k);
                    self.files_act(act);
                } else {
                    let act = self.changes.key(k);
                    self.changes_act(act);
                }
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
            match self.command.action.take() {
                Some(crate::command::Action::Branch) => return self.branch(),
                Some(crate::command::Action::Handoff) => self.handoff(),
                None => {}
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
            KeyCode::Esc if self.pane != Pane::Chat && self.prompt_focus => {
                if let Some((_, draft)) = self.commenting.take() {
                    self.input.set_text(&draft);
                }
                if let Some(draft) = self.committing.take() {
                    self.input.set_text(&draft);
                    self.drafting = None;
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
            // tab: the menu (prompt text or not; dropdowns took theirs above)
            KeyCode::Tab | KeyCode::BackTab if !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) && !self.mention_open() => {
                self.open_menu();
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
            // a commit message in the prompt: ctrl+g has a model draft it from the staged diff
            KeyCode::Char('g') if ctrl && self.committing.is_some() => {
                if self.drafting.is_none() {
                    let (tx, rx) = mpsc::channel();
                    let dir = self.cwd.clone();
                    let model = crate::config::rank_model().unwrap_or_else(|| "haiku".into());
                    std::thread::spawn(move || {
                        let _ = tx.send(crate::changes::draft_message(&dir, &model));
                    });
                    self.drafting = Some(rx);
                    self.toast("drafting the commit message…");
                }
                return false;
            }
            // ctrl+y: yazi (full screen) on the worktree; its pick goes into the prompt
            KeyCode::Char('y') if ctrl => {
                self.yazi = Some(self.cwd.clone());
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
        if p == Pane::Files {
            self.files.refresh(true);
        }
        if p == Pane::Changes {
            self.changes.refresh(true);
        }
    }

    fn changes_act(&mut self, a: crate::changes::Act) {
        match a {
            crate::changes::Act::None => {}
            crate::changes::Act::Toast(t) => self.toast(t),
            crate::changes::Act::Open(file, line) => self.open_at(&file, line),
            crate::changes::Act::Comment(spots) => {
                // the prompt takes the comment; its draft waits
                let draft = self.input.text();
                self.input.set_text("");
                self.commenting = Some((spots, draft));
                self.prompt_focus = true;
            }
            crate::changes::Act::CommitMessage => {
                let draft = self.input.text();
                self.input.set_text("");
                self.committing = Some(draft);
                self.prompt_focus = true;
            }
            crate::changes::Act::Send(text) => {
                self.back = 0;
                let to = self.default_to().join(", ");
                self.send_prompt(text);
                self.toast(format!("review comments sent to {to}"));
            }
        }
    }

    fn files_act(&mut self, a: crate::files::Act) {
        match a {
            crate::files::Act::None => {}
            crate::files::Act::Toast(t) => self.toast(t),
            crate::files::Act::Open(p) => {
                let file = p.strip_prefix(&self.cwd).map_or_else(|_| p.display().to_string(), |r| r.display().to_string());
                self.open_at(&file, 1);
            }
            crate::files::Act::Yazi(p) => self.yazi = Some(p),
        }
    }

    /// yazi's pick (the chooser file): its paths into the prompt.
    fn yazi_picked(&mut self, paths: &[String]) {
        if paths.is_empty() {
            return;
        }
        let shown: Vec<String> = paths.iter().map(|p| Path::new(p).strip_prefix(&self.cwd).map_or_else(|_| p.clone(), |r| r.display().to_string())).collect();
        self.input.insert(&format!("{} ", shown.join(" ")));
        self.toast(format!("yazi: {} path(s) in the prompt", shown.len()));
    }

    /// The IDE (setting `editor`) at FILE:LINE of the worktree (VS Code: the window that has it open).
    fn open_at(&mut self, file: &str, line: u32) {
        let cwd = self.cwd.clone();
        self.open_editor(|k| crate::editor::args_at(k, &cwd, file, line), format!("{file}:{line}"));
    }

    /// portless: read this thread's live URLs every 5 s (on a thread). 0 portless, 0 worktree: 0.
    fn poll_urls(&mut self) {
        if let Some(rx) = &self.urls_read
            && let Ok(u) = rx.try_recv()
        {
            self.urls_read = None;
            self.urls = u;
        }
        if cfg!(test) || self.urls_read.is_some() || self.urls_at.is_some_and(|t| t.elapsed() < Duration::from_secs(5)) {
            return;
        }
        self.urls_at = Some(Instant::now());
        let Some(prefix) = store::load(&self.id).filter(|t| t.worktree.is_some()).and_then(|t| t.branch).map(|b| crate::portless::prefix(&b)) else { return };
        if !crate::portless::active() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(crate::portless::urls_of(&crate::portless::routes(), &prefix));
        });
        self.urls_read = Some(rx);
    }

    fn mention_open(&self) -> bool {
        self.mention.is_open(&self.input, &self.roster)
    }

    /// ctrl+i: the IDE on the thread's worktree.
    fn open_vscode(&mut self) {
        let cwd = self.cwd.clone();
        self.open_editor(|k| crate::editor::args_dir(k, &cwd), cwd.display().to_string());
    }

    /// Start the IDE with ARGS (by its kind); the toast names WHAT, else why not; once: the trust hint.
    fn open_editor(&mut self, args: impl Fn(crate::editor::Kind) -> Vec<std::ffi::OsString>, what: String) {
        let Some(e) = crate::editor::find() else {
            self.toast("IDE: 0 found · setting editor (code, cursor, zed, idea …)");
            return;
        };
        // a terminal editor: full screen in this terminal (the view loop suspends codehead)
        if e.kind == crate::editor::Kind::Terminal {
            let mut argv: Vec<std::ffi::OsString> = vec![e.bin.clone().into()];
            argv.extend(args(e.kind));
            self.outside = Some(argv);
            return;
        }
        match crate::editor::spawn(&e, &args(e.kind)) {
            Ok(()) => {
                let hint = crate::editor::trust_hint(&self.cwd);
                self.toast(hint.unwrap_or_else(|| format!("IDE: {what}")));
            }
            Err(err) => self.toast(format!("IDE: {err}")),
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
            Item { label: "back to the start screen".into(), key: "esc esc".into(), id: 3, section: "thread" },
            Item { label: "IDE on the worktree".into(), key: "ctrl+i".into(), id: 2, section: "thread" },
            Item { label: "interrupt the agents".into(), key: "esc".into(), id: 12, section: "thread" },
            Item { label: "tool calls: open / close all".into(), key: "ctrl+o".into(), id: 1, section: "chat" },
            Item { label: "thoughts: open / close all".into(), key: "ctrl+t".into(), id: 13, section: "chat" },
            Item { label: "scroll the stream".into(), key: "wheel · pgup".into(), id: 10, section: "chat" },
            Item { label: "select + copy · double click: word · click code: copy".into(), key: "drag".into(), id: 9, section: "chat" },
            Item { label: "send · new line".into(), key: "⌘/ctrl+⏎ · ⏎".into(), id: 11, section: "prompt" },
            Item { label: "agents: mention · →: replace an agent".into(), key: "@".into(), id: 4, section: "prompt" },
            Item { label: "commands: files, notes, threads, skills, branch".into(), key: crate::config::command_key().to_string(), id: 5, section: "prompt" },
            Item { label: "paste a clipboard image".into(), key: "ctrl+v".into(), id: 7, section: "prompt" },
        ];
        // cards: read in the background by the menu (0 wait on open)
        let cards = Vec::new();
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
                // a thread picked: the view ends
                self.mouse_exit = true;
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
        let click = matches!(m.kind, MouseEventKind::Down(MouseButton::Left));
        // the worktree's URL (portless): the browser
        if click && let Some((r, url)) = &self.url_rect
            && r.contains(at)
        {
            let url = url.clone();
            open_url(&url);
            self.toast(format!("open {url}"));
            return;
        }
        // the view bar
        if click && let Some(&(_, p)) = self.bar_rects.iter().find(|(r, _)| r.contains(at)) {
            self.set_pane(p);
            return;
        }
        let in_stream = self.stream_rect.contains(at);
        if self.pane != Pane::Chat {
            if self.view_rect.contains(at) {
                self.prompt_focus = self.prompt_focus && !click;
                if self.pane == Pane::Files { self.files.mouse(m) } else { self.changes.mouse(m) }
                return;
            }
            // the chat of the split: its scroll below; a click there: 0 selection
            if in_stream && click {
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
                    // a file of a message: the files view on it
                    if let Some(f) = url.strip_prefix("file://") {
                        let path = f.split("#L").next().unwrap_or(f);
                        self.files.show(Path::new(path));
                        self.set_pane(Pane::Files);
                        return;
                    }
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
                    // the selection stays in the message it started in
                    let (first, last) = self.item_rows(from.0);
                    let to = if row < first { (first, 0) } else if row > last { (last, u16::MAX) } else { (row, at.x.saturating_sub(self.stream_rect.x)) };
                    self.sel = Some((from, to));
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
        if named.is_empty() { self.default_to() } else { named }
    }

    /// Who a message without @name goes to: the agents of the owner's last message (still on the
    /// thread), else the lead.
    fn default_to(&self) -> Vec<String> {
        let last = self.thread.items.iter().rev().find_map(|i| match i {
            stream::Item::Prompt { from, to, .. } if from == "owner" => Some(to.iter().filter(|a| self.members.contains(a)).cloned().collect::<Vec<_>>()),
            _ => None,
        });
        match last {
            Some(to) if !to.is_empty() => to,
            _ => self.members.first().cloned().into_iter().collect(),
        }
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
                // the text columns of the row only (0 border, 0 margin)
                let text: String = self.drawn.rows[r].line.spans.iter().map(|s| s.content.as_ref()).collect();
                let Some((ta, tb)) = content_cols(&text) else { continue };
                if tb <= ta {
                    continue;
                }
                let (ta, tb) = (ta as u16, tb as u16 - 1);
                let (a, b) = (if r == r0 { c0.max(ta) } else { ta }, if r == r1 { c1.min(tb) } else { tb });
                if b < a {
                    continue;
                }
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
        // row 0: the view bar (views of the thread, the title)
        let top = Rect { height: full.height.min(1), ..full };
        let below = Rect { y: full.y + top.height, height: full.height - top.height, ..full };
        // a dotted line below the view bar (as the fold line above the prompt)
        let sep = (below.height > 8) as u16;
        if sep > 0 {
            let buf = f.buffer_mut();
            for x in below.x..below.right() {
                buf[(x, below.y)].set_symbol("┄").set_style(theme::line());
            }
        }
        let area = Rect { y: below.y + sep, height: below.height - sep, ..below };
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
        self.view_rect = st;

        if self.pane == Pane::Files {
            self.files.draw(f, st);
        } else if self.pane == Pane::Chat {
            self.draw_stream(f, st);
        } else {
            // wide: the chat left, the changes right (alt+\ toggles)
            let st = if self.split && st.width >= SPLIT_MIN {
                let lw = st.width * 2 / 5;
                let left = Rect { width: lw, ..st };
                self.stream_rect = left;
                self.draw_stream(f, left);
                for y in st.y..st.bottom() {
                    f.buffer_mut()[(st.x + lw, y)].set_symbol("│").set_style(theme::line());
                }
                Rect { x: st.x + lw + 1, width: st.width - lw - 1, ..st }
            } else {
                st
            };
            self.view_rect = st;
            self.changes.draw(f, st);
            if let Some(n) = self.changes.take_note() {
                self.toast(n);
            }
            // the CI fix: the failed log to the agents of the last message
            if let Some(m) = self.changes.take_send() {
                let to = self.default_to().join(", ");
                self.send_prompt(m);
                self.toast(format!("CI log sent to {to}"));
            }
            // a model's commit message draft: into the prompt (still in commit mode)
            if let Some(rx) = &self.drafting
                && let Ok(r) = rx.try_recv()
            {
                self.drafting = None;
                match r {
                    Ok(m) if self.committing.is_some() => self.input.set_text(&m),
                    Ok(_) => {}
                    Err(e) => self.toast(format!("draft: {e}")),
                }
            }
        }
        if mon_h > 0 {
            self.monitors.render(f, mon, self.tick());
        }

        // the view bar on row 0 (views of the thread, the thread title); the title row above the
        // agents box: the stream's position, the pills
        let note = if self.back > 0 && self.pane == Pane::Chat { format!("↑{} · esc: live ", self.back) } else { String::new() };
        let mut spans: Vec<Span<'static>> = Vec::new();
        self.bar_rects.clear();
        let mut x = top.x + 2;
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
                Pane::Files => {
                    let n = self.files.count();
                    if n == 0 { " files ".to_string() } else { format!(" files {n} ") }
                }
            };
            let st = if p == self.pane { theme::pill() } else { theme::muted() };
            let w = unicode_width::UnicodeWidthStr::width(label.as_str()) as u16;
            self.bar_rects.push((Rect { x, y: top.y, width: w, height: 1 }, p));
            x += w + 1;
            spans.push(Span::styled(label, st));
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled("│ ", theme::line()));
        let used: usize = spans.iter().map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref())).sum();
        // portless: the worktree's live URL on the right (a click opens it)
        self.poll_urls();
        self.url_rect = None;
        let link = self.urls.first().map(|u| {
            let host = u.trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/');
            let more = if self.urls.len() > 1 { format!(" +{}", self.urls.len() - 1) } else { String::new() };
            (format!("↗ {host}{more} "), u.clone())
        });
        let link_w = link.as_ref().map_or(0, |(t, _)| unicode_width::UnicodeWidthStr::width(t.as_str()));
        let room = usize::from(top.width.saturating_sub(4)).saturating_sub(used + link_w + 1);
        spans.push(Span::styled(format!("{} ", crate::nav::fit(&self.title, room)), theme::accent_bold()));
        f.render_widget(Paragraph::new(Line::from(spans)), Rect { x: top.x + 2, width: top.width.saturating_sub(2), ..top });
        if let Some((text, url)) = link
            && top.width as usize > link_w + used + 4
        {
            let r = Rect { x: top.right() - link_w as u16, width: link_w as u16, ..top };
            f.render_widget(Paragraph::new(Span::styled(text, theme::accent().add_modifier(Modifier::UNDERLINED))), r);
            self.url_rect = Some((r, url));
        }
        let mut low: Vec<Span<'static>> = Vec::new();
        if !note.is_empty() {
            low.push(Span::styled(note, theme::muted()));
        }
        if let Some((new, old)) = &self.replace {
            low.push(Span::styled(format!(" on send: {new} replaces {old} "), theme::pill()));
        }
        f.render_widget(Paragraph::new(Line::from(low)), Rect { x: title.x + 2, width: title.width.saturating_sub(2), ..title });

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
            _ if self.committing.is_some() => Span::styled(
                if self.drafting.is_some() { "drafting… · esc: cancel ".to_string() } else { "commit message · ctrl+g: draft by model · ⌘/ctrl+⏎: commit the staged files · esc: cancel ".to_string() },
                theme::accent(),
            ),
            _ if let Some((spots, _)) = &self.commenting => {
                let on = spots.iter().map(|s| s.label()).collect::<Vec<_>>().join(", ");
                Span::styled(format!("comment on {on} · ⌘/ctrl+⏎: add to the batch · esc: cancel "), theme::accent())
            }
            _ if self.pane == Pane::Changes && !self.prompt_focus && self.changes.typing() => Span::styled("type to filter · enter: 1st hit · esc: clear ", theme::muted()),
            _ if self.pane == Pane::Changes && !self.prompt_focus => {
                Span::styled("j k · space: select · c: comment · r: reviewed · ←→: fold · 1-4: tabs · f: filter · i: prompt ", theme::muted())
            }
            _ if self.pane == Pane::Files && !self.prompt_focus => Span::styled("j k · pgup pgdn: scroll · o: IDE · y: yazi · c: copy path · S: share · i: prompt ", theme::muted()),
            _ if self.pane != Pane::Chat => Span::styled("esc: back to the view · F1-F3: views ", theme::muted()),
            _ => Span::styled("tab: menu · F1-F3: views · esc: interrupt · esc esc: codehead ", theme::muted()),
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
            // centered on the chat history (the stream), not the whole terminal
            m.render(f, area, self.view_rect);
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

/// A stream row's text without the message bar on its left.
/// The text columns of a stream row (chars, end exclusive): inside its box or right of its bar,
/// trailing spaces off. None: a border row (`╭─ … ╮`, `╰─ copy ─╯`).
fn content_cols(row: &str) -> Option<(usize, usize)> {
    let chars: Vec<char> = row.chars().collect();
    let t = row.trim_start();
    if t.starts_with('╭') || t.starts_with('╰') {
        return None;
    }
    let bar = |c: char| "│┃▌┆".contains(c);
    let Some(l) = chars.iter().position(|c| !c.is_whitespace()) else { return Some((0, 0)) };
    let a = if bar(chars[l]) { l + 1 + usize::from(chars.get(l + 1) == Some(&' ')) } else { 0 };
    let r = chars.iter().rposition(|c| !c.is_whitespace()).unwrap_or(l);
    let mut b = if r > l && bar(chars[r]) { r } else { r + 1 };
    while b > a && chars[b - 1] == ' ' {
        b -= 1;
    }
    Some((a, b.max(a)))
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

/// Run yazi on TARGET full screen: the terminal modes off (raw, mouse, paste, keyboard flags,
/// alternate screen), yazi, the modes on again, a full redraw. Returns yazi's picked paths.
fn run_outside<B: Backend>(term: &mut Terminal<B>, target: &Path) -> Result<Vec<String>, String> {
    if std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("yazi")).find(|p| p.is_file())).is_none() {
        return Err("yazi: not on PATH (brew install yazi)".into());
    }
    let (argv, env, chooser) = crate::files::yazi_cmd(target);
    let argv: Vec<std::ffi::OsString> = argv.into_iter().map(Into::into).collect();
    let status = suspended(term, &argv, &env).map_err(|e| format!("yazi: {e}"))?;
    if !status.success() {
        return Err(format!("yazi exited with {status} (config: {})", crate::files::yazi_config().display()));
    }
    let picked = std::fs::read_to_string(&chooser).unwrap_or_default();
    let _ = std::fs::remove_file(&chooser);
    Ok(picked.lines().filter(|l| !l.is_empty()).map(str::to_string).collect())
}

/// Run ARGV (with ENV) full screen: codehead's terminal modes off (raw, mouse, paste,
/// keyboard flags, alternate screen), the program, the modes on again, a full redraw.
fn suspended<B: Backend>(term: &mut Terminal<B>, argv: &[std::ffi::OsString], env: &[(String, String)]) -> std::io::Result<std::process::ExitStatus> {
    use ratatui::crossterm::{cursor, event, execute, terminal};
    let mut out = std::io::stdout();
    let _ = execute!(out, event::PopKeyboardEnhancementFlags, event::DisableMouseCapture, event::DisableBracketedPaste, terminal::LeaveAlternateScreen, cursor::Show);
    let _ = terminal::disable_raw_mode();
    let status = std::process::Command::new(&argv[0]).args(&argv[1..]).envs(env.iter().cloned()).status();
    let _ = terminal::enable_raw_mode();
    let _ = execute!(
        out,
        terminal::EnterAlternateScreen,
        event::EnableMouseCapture,
        event::EnableBracketedPaste,
        event::PushKeyboardEnhancementFlags(event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    );
    let _ = term.clear();
    status
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
    crate::engine::trace("view new …");
    let mut v = View::new(id, cwd, title, tx);
    crate::engine::trace("view new");
    // a handoff: its document waits as the prompt draft
    if let Some(d) = crate::handoff::take_draft(id) {
        v.input.set_text(&d);
        v.input.move_end();
    }
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
            crate::engine::trace("view draw");
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
                    v.mouse_exit
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
        // yazi: full screen, codehead suspended; its pick into the prompt
        // a terminal editor: full screen, codehead suspended
        if let Some(argv) = v.outside.take() {
            match suspended(term, &argv, &[]) {
                Ok(s) if s.success() => {}
                Ok(s) => v.toast(format!("editor exited with {s}")),
                Err(e) => v.toast(format!("editor: {e}")),
            }
            dirty = true;
        }
        if let Some(target) = v.yazi.take() {
            match run_outside(term, &target) {
                Ok(picked) => v.yazi_picked(&picked),
                Err(e) => v.toast(e),
            }
            dirty = true;
        }
        if v.handoff_due() {
            crate::nav::mark_seen_now(id);
            return Ok(std::mem::replace(&mut v.exit, Exit::Back));
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
        assert!(v.menu.is_none(), "tab with an open ask: 0 menu");
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
        // the changes label may carry a count (other tests share the thread id)
        assert!(s.contains(" chat   changes") && s.contains("files  │ thread p"), "{s}");
        assert!(s.contains("F1-F3: views"), "{s}");
        // the view bar on row 0
        assert!(s.lines().next().unwrap().contains(" chat "), "{s}");
        press(&mut v, KeyCode::Char('2'), KeyModifiers::ALT);
        assert_eq!(v.pane, Pane::Changes);
        let s = screen(&mut v);
        assert!(s.contains("space: select") && s.contains("r: reviewed") && s.contains("i: prompt"), "{s}");
        press(&mut v, KeyCode::Char('1'), KeyModifiers::ALT);
        assert_eq!(v.pane, Pane::Chat);
    }

    #[test]
    fn changes_view_keeps_letters_from_the_prompt_until_i() {
        let mut v = view();
        press(&mut v, KeyCode::F(2), KeyModifiers::NONE);
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

    #[test]
    fn changes_view_selects_lines_comments_on_them_and_filters_with_i_and_esc() {
        let mut v = view();
        press(&mut v, KeyCode::Char('2'), KeyModifiers::ALT);
        v.changes.set_files(crate::changes::parse("diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@\n x\n-a\n+b\n y\n"));
        let s = screen(&mut v);
        assert!(s.contains(" 3 medium 0/1 ") && s.contains("╭─ ☐ src/a.rs"), "{s}");
        // k k k: head → hunk → ` x` → `- a`; space selects 2 lines (the cursor goes down)
        for _ in 0..3 {
            press(&mut v, KeyCode::Char('k'), KeyModifiers::NONE);
        }
        press(&mut v, KeyCode::Char(' '), KeyModifiers::NONE);
        press(&mut v, KeyCode::Char(' '), KeyModifiers::NONE);
        // c: the prompt takes the comment on both lines
        press(&mut v, KeyCode::Char('c'), KeyModifiers::NONE);
        assert!(v.prompt_focus);
        let s = screen(&mut v);
        assert!(s.contains("comment on src/a.rs:2 · removed 2"), "{s}");
        press(&mut v, KeyCode::Char('n'), KeyModifiers::NONE);
        press(&mut v, KeyCode::Enter, KeyModifiers::CONTROL);
        assert_eq!(v.changes.comment_count(), 1);
        let s = screen(&mut v);
        assert!(s.contains("✎ 2 · removed 2: n"), "{s}");
        // f: the filter takes letters, i too; esc leaves it (0 interrupt timer)
        press(&mut v, KeyCode::Char('f'), KeyModifiers::NONE);
        press(&mut v, KeyCode::Char('i'), KeyModifiers::NONE);
        assert!(!v.prompt_focus && v.input.text().is_empty());
        assert!(screen(&mut v).contains(" f  i▏"));
        press(&mut v, KeyCode::Esc, KeyModifiers::NONE);
        assert!(!v.changes.typing() && v.last_esc.is_none());
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("panetest-0000"));
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
        v.changes_act(crate::changes::Act::Comment(vec![crate::changes::Spot { path: "src/a.rs".into(), line: 12, lines: String::new() }]));
        assert!(v.prompt_focus && v.input.text().is_empty());
        v.key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE));
        v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        assert_eq!(v.changes.comment_count(), 1);
        assert!(!v.prompt_focus && v.input.text() == "half a message" && v.commenting.is_none());
        // esc cancels: 0 comment, the draft back
        v.changes_act(crate::changes::Act::Comment(vec![crate::changes::Spot { path: "src/a.rs".into(), line: 13, lines: String::new() }]));
        v.key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        v.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(v.changes.comment_count(), 1);
        assert_eq!(v.input.text(), "half a message");
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("commenttest-0000"));
    }

    #[test]
    fn a_commit_message_borrows_the_prompt_too() {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("committest-0000", Path::new("/tmp"), "t", tx);
        v.set_pane(Pane::Changes);
        v.input.set_text("draft");
        v.changes_act(crate::changes::Act::CommitMessage);
        assert!(v.prompt_focus && v.input.text().is_empty() && v.committing.is_some());
        // an empty message: 0 commit, still in commit mode
        v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        assert!(v.committing.is_some());
        v.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(v.committing.is_none() && v.input.text() == "draft" && !v.prompt_focus);
    }
}

#[cfg(test)]
mod default_to_tests {
    use super::*;

    #[test]
    fn a_message_without_a_name_goes_to_the_agents_of_the_last_one() {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("defaulttest-0000", Path::new("/tmp"), "t", tx);
        v.members = vec!["ada".into(), "bob".into()];
        // 0 owner message yet: the lead
        assert_eq!(v.default_to(), ["ada"]);
        v.thread.push(&store::Event { ts: 1, agent: None, kind: store::Kind::Prompt { from: "owner".into(), to: vec!["bob".into()], text: "@bob look".into() } });
        // an agent's message in between changes 0
        v.thread.push(&store::Event { ts: 2, agent: Some("ada".into()), kind: store::Kind::Prompt { from: "ada".into(), to: vec!["bob".into()], text: "hi".into() } });
        assert_eq!(v.default_to(), ["bob"]);
        v.input.set_text("and now?");
        assert_eq!(v.recipients(), ["bob"]);
        v.send_prompt("and now?".into());
        let ops = v.ops.borrow().clone();
        assert!(ops.iter().any(|o| o["op"] == "prompt" && o["to"] == serde_json::json!(["bob"])), "{ops:?}");
        // bob left: the lead again
        v.members = vec!["ada".into()];
        assert_eq!(v.default_to(), ["ada"]);
        // @name of a known agent: the host routes (0 `to`)
        v.names = vec![("cleo".into(), Color::Reset)];
        v.send_prompt("@cleo join".into());
        let last = v.ops.borrow().last().cloned().unwrap();
        assert!(last["to"].is_null(), "{last:?}");
    }
}

#[cfg(test)]
mod files_pane_tests {
    use super::*;

    #[test]
    fn the_files_filter_takes_every_letter_and_esc() {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("filesfilter-0000", Path::new("/tmp"), "t", tx);
        v.key(KeyEvent::new(KeyCode::F(3), KeyModifiers::NONE));
        assert_eq!(v.pane, Pane::Files);
        for c in "fix".chars() {
            v.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        // `i` went to the filter, not to the prompt focus
        assert!(v.files.typing() && !v.prompt_focus && v.input.text().is_empty());
        v.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!v.files.typing() && v.last_esc.is_none(), "esc: the filter only");
    }

    #[test]
    fn files_is_the_3rd_view_with_its_keys() {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("filestest-0000", Path::new("/tmp"), "t", tx);
        v.key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::ALT));
        assert_eq!(v.pane, Pane::Files);
        // letters: view keys, 0 prompt text
        v.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(v.input.text(), "");
        // ctrl+y: yazi on the worktree, for the view loop
        v.key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));
        assert_eq!(v.yazi.as_deref(), Some(Path::new("/tmp")));
        // a pick: its path into the prompt, relative to the worktree
        v.yazi_picked(&["/tmp/src/a.rs".to_string()]);
        assert_eq!(v.input.text(), "src/a.rs ");
        // F1 from files: back to chat
        v.key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
        assert_eq!(v.pane, Pane::Chat);
    }
}

#[cfg(test)]
mod split_tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn screen(v: &mut View, w: u16) -> Vec<String> {
        let mut t = Terminal::new(TestBackend::new(w, 30)).unwrap();
        t.draw(|f| v.draw(f)).unwrap();
        let b = t.backend().buffer().clone();
        (0..30).map(|y| (0..w).map(|x| b[(x, y)].symbol().to_string()).collect::<String>()).collect()
    }

    #[test]
    fn wide_changes_view_has_the_chat_left_and_alt_backslash_toggles() {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("splittest-0000", Path::new("/tmp"), "t", tx);
        v.set_pane(Pane::Changes);
        let s = screen(&mut v, 220);
        // the divider at 2/5 of the width, the stream's empty note left
        assert!(s[2].chars().nth(88) == Some('│'), "{}", s[2]);
        assert!(v.stream_rect.width == 88 && v.view_rect.x == 89, "{:?} {:?}", v.stream_rect, v.view_rect);
        v.key(KeyEvent::new(KeyCode::Char('\\'), KeyModifiers::ALT));
        screen(&mut v, 220);
        assert_eq!(v.view_rect.x, 0, "full width");
        // narrow: 0 split
        v.key(KeyEvent::new(KeyCode::Char('\\'), KeyModifiers::ALT));
        screen(&mut v, 150);
        assert_eq!(v.view_rect.x, 0);
    }
}

/// Mouse selection in the stream: it stays in its message, the copy has the message's lines.
#[cfg(test)]
mod select_tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn screen(v: &mut View) -> Vec<String> {
        let mut t = Terminal::new(TestBackend::new(60, 30)).unwrap();
        t.draw(|f| v.draw(f)).unwrap();
        let b = t.backend().buffer().clone();
        (0..30).map(|y| (0..60).map(|x| b[(x, y)].symbol().to_string()).collect::<String>()).collect()
    }

    fn mouse(v: &mut View, kind: MouseEventKind, x: u16, y: u16) {
        v.mouse(MouseEvent { kind, column: x, row: y, modifiers: KeyModifiers::NONE });
    }

    #[test]
    fn a_drag_stays_in_its_message_and_copies_unwrapped_lines() {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("selecttest-0000", Path::new("/tmp"), "t", tx);
        v.members = vec!["ada".into()];
        let long = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho";
        v.thread.push(&store::Event { ts: 1, agent: None, kind: store::Kind::Prompt { from: "owner".into(), to: vec!["ada".into()], text: format!("{long}\nsecond line") } });
        v.thread.push(&store::Event { ts: 2, agent: Some("ada".into()), kind: store::Kind::Text { text: "the agent answers here".into() } });
        let s = screen(&mut v);
        let y = s.iter().position(|r| r.contains("alpha")).unwrap() as u16;
        let x = s[y as usize].find("alpha").map(|b| s[y as usize][..b].chars().count()).unwrap() as u16;
        let agent = s.iter().position(|r| r.contains("agent answers")).unwrap() as u16;
        assert!(agent > y + 2, "{s:#?}");
        mouse(&mut v, MouseEventKind::Down(MouseButton::Left), x, y);
        // past the message into the agent's: clamped to the owner's message
        mouse(&mut v, MouseEventKind::Drag(MouseButton::Left), 0, agent);
        mouse(&mut v, MouseEventKind::Up(MouseButton::Left), 0, agent);
        let got = crate::clip::LAST.with_borrow(|l| l.clone());
        assert_eq!(got, format!("{long}\nsecond line"));
        // the highlight: 0 cells left of the bubble's text
        let s = screen(&mut v);
        let mut t = Terminal::new(TestBackend::new(60, 30)).unwrap();
        t.draw(|f| v.draw(f)).unwrap();
        let b = t.backend().buffer().clone();
        let lit = |yy: u16| (0..60).filter(|&xx| b[(xx, yy)].bg == theme::pal().accent).collect::<Vec<u16>>();
        let row2 = lit(y + 1);
        assert!(!row2.is_empty() && row2[0] >= x, "row {}: {row2:?}\n{s:#?}", y + 1);
        assert!(lit(agent).is_empty(), "agent row unlit");
    }
}

