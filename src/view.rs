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
use ratatui::widgets::{Clear, Paragraph};
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

/// A PR for the merge tab of the next view of a thread: (thread, PR).
static OPEN_PR: std::sync::Mutex<Option<(String, crate::gh::PrRef)>> = std::sync::Mutex::new(None);

/// The next view of thread ID opens on its merge tab, PR in front.
pub fn open_pr(id: &str, pr: &crate::gh::PrRef) {
    *OPEN_PR.lock().unwrap_or_else(|e| e.into_inner()) = Some((id.to_string(), pr.clone()));
}

fn take_pr(id: &str) -> Option<crate::gh::PrRef> {
    let mut g = OPEN_PR.lock().unwrap_or_else(|e| e.into_inner());
    if g.as_ref().is_some_and(|(t, _)| t == id) { g.take().map(|x| x.1) } else { None }
}

/// Views of a thread: the view bar on row 0 (F1-F4, ctrl+1-4, alt+1-4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pane {
    #[default]
    Chat,
    Changes,
    /// The pull requests of the thread (merge.rs).
    Merge,
    Files,
}

impl Pane {
    const ALL: [Pane; 4] = [Pane::Chat, Pane::Changes, Pane::Merge, Pane::Files];
}


/// From this many columns, a thread opens in the changes view with the chat right of it: the
/// changes left at full height, the agents, the stream and the prompt right. Narrower (a window
/// that shrinks when the editor opens next to it): the chat alone.
const SPLIT_MIN: u16 = 160;
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
const MAX_HEADER_H: u16 = 12;
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
    /// The owner's shell command ended: (command, output, exit code, dir).
    Shell(String, String, i32, PathBuf),
    /// Shell mode's dir: the thread's checkout with the most line changes.
    ShellCwd(PathBuf),
    /// The log's events before the tail the view opened with (a big log: read in the
    /// background).
    Head(Vec<store::Event>),
}

/// ctrl+q: the agents box above the prompt has the keys. AT: the highlighted agent (index in
/// the box order), CUR: its menu row (flow, remove, replace with), ARMED: remove asks a 2nd enter,
/// REPLACE: the agents to replace it with (cursor, 1st row in view).
#[derive(Clone, Debug, Default, PartialEq)]
struct AgentsFocus {
    at: usize,
    cur: usize,
    armed: bool,
    replace: Option<(usize, usize)>,
}

/// The rows of the agents box's menu.
const AGENT_MENU: [&str; 3] = ["flow", "remove", "replace with"];

/// The 1st id of the menu's agent rows: 3 per agent (flow, remove, replace with).
const AGENT_ITEMS: usize = 1000;

struct View {
    id: String,
    title: String,
    cwd: PathBuf,
    tail: store::Tail,
    /// The log opened at its tail (a big log): the events read since, until the head is in.
    early: Option<Vec<store::Event>>,
    thread: stream::Thread,
    roster: Vec<crate::agents::Agent>,
    members: Vec<String>,
    colors: stream::Colors,
    names: Vec<(String, Color)>,
    /// Agents of the roster: (name, harness), for the hints of the stream.
    harness_of: Vec<(String, String)>,
    /// Per roster agent: sprite, effort label of its card (read on roster change, not per frame).
    profiles: HashMap<String, (crate::sprites::Sprite, String)>,
    /// Agents box order: members, the last @named first (members[0] stays the lead).
    order: Vec<String>,
    /// Agents @named before they joined: they join at the front.
    front: Vec<String>,
    /// Spawns under way, by agent.
    spawns: HashMap<String, crate::avatar::Spawn>,
    /// The sprite slots of the last draw (agent, its 7x4 rect): who is in view, where a spawn goes.
    slots: std::cell::RefCell<Vec<(String, Rect)>>,
    /// The flow lines of the cards of the last draw (agent, its rect): a click opens its flow box.
    flow_slots: std::cell::RefCell<Vec<(String, Rect)>>,
    /// ctrl+q: the agents box has the keys (None: the prompt or a view).
    agents_focus: Option<AgentsFocus>,
    /// The next sent message goes to this agent (a flow wait's reply), not the default.
    reply_to: Option<String>,
    /// The agents of the menu's agent rows, as the menu opened (ids stay valid after a join).
    menu_agents: Vec<String>,
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
    /// The last drawn frame: ctrl+click reads the URL under the mouse from it.
    last_frame: ratatui::buffer::Buffer,
    // ask
    /// The 1st open ask: cursor, picks, answers so far, focus.
    ask: crate::ask::Picker,
    /// Tests: the host commands, recorded (0 host process).
    #[cfg(test)]
    ops: std::cell::RefCell<Vec<Value>>,
    // prompt
    input: TextArea,
    input_rect: Rect,
    /// The IDE's repo picker (ctrl+i with several repos): cursor, 1st row in view; its drawn rows;
    /// the repos line below the prompt (its anchor).
    ide_pick: Option<(usize, usize)>,
    ide_rows: Vec<(Rect, usize)>,
    hint_rect: Rect,
    #[cfg(test)]
    ide_opened: Vec<PathBuf>,
    mention: Mention,
    /// The thread's checkouts (its work dir 1st), the one the changes view shows.
    checkouts: Vec<PathBuf>,
    checkout: usize,
    /// The owner picked the checkout (a tab, ctrl+w): it stays; else the view follows the
    /// thread's newest worktree.
    checkout_picked: bool,
    /// The repo tabs on top of the changes view (shift+←→, ctrl+w, a click): 1 per checkout.
    checkout_tabs: crate::tabgroup::TabGroup,
    checkout_labels: Vec<String>,
    /// Under the prompt: the repos, branches, worktrees the thread works in (read again every 3 s).
    spots: Vec<crate::workspot::Spot>,
    spots_at: Option<Instant>,
    /// The merge tab: the thread's pull requests.
    merge: crate::merge::Merge,
    merge_sources_at: Option<Instant>,
    /// The merge tab's sources read (git remotes, the log's PR links): off the UI thread.
    merge_reads: crate::background::Reads<(), (Vec<(String, PathBuf)>, crate::merge::Sources)>,
    /// ctrl+r in shell mode: the history search.
    hist: crate::histsearch::HistSearch,
    command: crate::command::Command,
    paths: crate::paths::Paths,
    /// Shell mode: the completions (tasks, commands, arguments), the history's suggestion.
    comp: crate::shellcomp::ShellComp,
    /// `#` of the prompt: the shortcuts.
    shortcuts: crate::shortcuts::Pick,
    paster: Paster,
    attachments: Vec<(String, PathBuf)>,
    history: Vec<String>,
    hist_at: Option<usize>,
    draft: String,
    /// A search result to scroll to (its message time); the item it found, marked until a key.
    jump: Option<u64>,
    /// The notifications dropdown (ctrl+n); the seen tracker of this thread's stream.
    notes_panel: Option<crate::notify::Panel>,
    watch: crate::notify::Watch,
    flash: Option<usize>,
    /// The prompt draft as last written to disk (`prompt_path`): 0 write without a change.
    prompt_saved: String,
    /// `@new` picked with "replace OLD": on send, OLD leaves and NEW gets the handoff.
    replace: Option<(String, String)>,
    /// A replace waiting for OLD's turn to end: (new, old, message).
    queued: Option<(String, String, String)>,
    /// Sent messages the log does not show yet: text, sent at, error.
    pending: Vec<(String, Instant, Option<String>)>,
    /// Shell mode: the owner's commands since the last message (the next one takes them to the
    /// agents); the commands running now.
    shell_ctx: Vec<String>,
    shell_running: usize,
    /// Shell mode's dir: the owner's pick (`<thread dir>/shell_cwd`), else the thread's checkout
    /// with the most line changes.
    shell_cwd: PathBuf,
    /// ↓ at the bottom of shell mode: the dir input (its own path dropdown, dirs only).
    cwd_edit: Option<TextArea>,
    cwd_paths: crate::paths::Paths,
    /// ↑↓ in shell mode: the thread's `!commands`, this one in view (None: the draft).
    shell_at: Option<usize>,
    shell_draft: String,
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
    /// The changes view with the chat right of it (wide terminals; alt+\ toggles); the view's area.
    split: bool,
    /// The view follows the width (changes + chat when wide, chat when narrow) until the owner
    /// picks a view.
    auto_pane: bool,
    /// The last draw had the changes + chat side by side.
    wide_split: bool,
    view_rect: Rect,
    /// portless: the live URLs of this thread's worktree (read every 5 s), the read on its way, the
    /// link on row 0.
    urls: Vec<String>,
    urls_read: Option<Receiver<Vec<String>>>,
    urls_at: Option<Instant>,
    url_rect: Option<(Rect, String)>,
    /// The chat above the prompt in the changes view (0 split): open; its title row (a click
    /// folds / opens it).
    mini_chat: bool,
    mini_rect: Rect,
    /// `> handoff`: the document on its way.
    handing: Option<Receiver<Result<String, String>>>,
    // chrome
    monitors: crate::monitors::Monitors,
    subagents: crate::subagents::Subagents,
    /// The running sub-threads of this thread (`>sub`): their row, their box.
    subthreads: crate::subthreads::SubThreads,
    /// The flow rows + their boxes, 1 per agent of the thread (members' order; `> flow`, the
    /// agents box, a click).
    flowbars: Vec<crate::flowbar::FlowBar>,
    /// ctrl+f in the chat: the search bar above the prompt.
    find: crate::chatsearch::ChatSearch,
    /// The search match to scroll to at the next draw (item index).
    find_to: Option<usize>,
    menu: Option<crate::menu::Menu>,
    cost: Option<crate::cost::Snap>,
    cost_at: Instant,
    toast: Option<(String, Instant)>,
    started: Instant,
    last_esc: Option<Instant>,
    /// A 1st Esc armed an interrupt: it fires after DOUBLE_ESC unless a 2nd Esc means back.
    esc_interrupt: Option<Instant>,
    last_ctrl_c: Option<Instant>,
    /// The last key press: a new ask takes the focus only after 1 s of 0 keys (and an empty prompt).
    last_key: Option<Instant>,
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
        // a big log: the 1st frame from its last 256 KiB, the rest parsed in the background
        let (tail, head_end) = if cfg!(test) { (store::Tail::new(id), 0) } else { store::Tail::from_end(id, 256 << 10) };
        if head_end > 0 {
            let (id, tx) = (id.to_string(), tx.clone());
            std::thread::spawn(move || {
                let _ = tx.send(Msg::Head(store::head(&id, head_end)));
            });
        }
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
            tail,
            early: (head_end > 0).then(Vec::new),
            thread: stream::Thread::default(),
            roster: Vec::new(),
            members: Vec::new(),
            colors: HashMap::new(),
            names: Vec::new(),
            harness_of: Vec::new(),
            profiles: HashMap::new(),
            order: Vec::new(),
            front: Vec::new(),
            spawns: HashMap::new(),
            slots: std::cell::RefCell::new(Vec::new()),
            flow_slots: std::cell::RefCell::new(Vec::new()),
            agents_focus: None,
            reply_to: None,
            menu_agents: Vec::new(),
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
            last_frame: ratatui::buffer::Buffer::default(),
            ask: crate::ask::Picker::default(),
            #[cfg(test)]
            ops: Default::default(),
            input,
            input_rect: Rect::default(),
            ide_pick: None,
            ide_rows: Vec::new(),
            hint_rect: Rect::default(),
            #[cfg(test)]
            ide_opened: Vec::new(),
            mention: Mention::default(),
            checkouts: Vec::new(),
            checkout: 0,
            checkout_picked: false,
            checkout_tabs: crate::tabgroup::TabGroup::default(),
            checkout_labels: Vec::new(),
            spots: Vec::new(),
            spots_at: None,
            merge: crate::merge::Merge::for_thread(id),
            merge_sources_at: None,
            merge_reads: crate::background::Reads::default(),
            hist: Default::default(),
            command,
            paths: crate::paths::Paths::new(cwd),
            comp: Default::default(),
            shortcuts: crate::shortcuts::Pick::default(),
            paster: Paster::new(crate::engine::data_dir().join("paste").join(id)),
            attachments: Vec::new(),
            history: Vec::new(),
            hist_at: None,
            draft: String::new(),
            prompt_saved: String::new(),
            jump: crate::search::take_jump(id),
            notes_panel: None,
            watch: crate::notify::Watch::default(),
            flash: None,
            replace: None,
            queued: None,
            pending: Vec::new(),
            shell_ctx: Vec::new(),
            shell_running: 0,
            shell_cwd: cwd.to_path_buf(),
            cwd_edit: None,
            cwd_paths: crate::paths::Paths::default(),
            shell_at: None,
            shell_draft: String::new(),
            live: Vec::new(),
            pane: Pane::Chat,
            changes: crate::changes::View::new(id, cwd),
            files: crate::files::View::new(id, cwd),
            yazi: None,
            outside: None,
            prompt_focus: false,
            bar_rects: Vec::new(),
            split: true,
            auto_pane: true,
            wide_split: false,
            view_rect: Rect::default(),
            urls: Vec::new(),
            urls_read: None,
            urls_at: None,
            url_rect: None,
            mini_chat: true,
            mini_rect: Rect::default(),
            handing: None,
            // tests: 0 shared state between parallel views
            monitors: crate::monitors::Monitors::new(id),
            subagents: Default::default(),
            subthreads: Default::default(),
            flowbars: Vec::new(),
            find: Default::default(),
            find_to: None,
            menu: None,
            cost: None,
            cost_at: Instant::now() - Duration::from_secs(60),
            toast: None,
            started: Instant::now(),
            last_esc: None,
            esc_interrupt: None,
            last_ctrl_c: None,
            last_key: None,
            exit: Exit::Back,
            mouse_exit: false,
            drag_from: None,
            tx,
            look: crate::config::messages(),
        };
        v.set_roster(roster);
        v.read();
        v.load_prompt();
        v.load_shell_cwd();
        v
    }

    // ---------- the prompt draft on disk ----------

    /// The unsent prompt: the draft under a history step, else the prompt's text; with its
    /// pasted files.
    fn prompt_draft(&self) -> String {
        let text = if self.hist_at.is_some() { self.draft.clone() } else { self.input.text() };
        // sent, not in the log yet: a crash before the host has it loses 0 messages
        let unsent: Vec<&str> = self.pending.iter().map(|(t, ..)| t.as_str()).collect();
        if text.trim().is_empty() && unsent.is_empty() {
            return String::new();
        }
        let files: Vec<Value> = self.attachments.iter().filter(|(m, _)| text.contains(m.as_str())).map(|(m, p)| json!([m, p])).collect();
        json!({"text": text, "attachments": files, "unsent": unsent}).to_string()
    }

    /// After every key, paste, click: the unsent prompt to disk, if it changed (0 text: 0 file).
    /// A crash, a closed terminal, a kill: the next view of the thread has it back.
    fn save_prompt(&mut self) {
        let d = self.prompt_draft();
        if d == self.prompt_saved {
            return;
        }
        let p = prompt_path(&self.id);
        let r = if d.is_empty() { std::fs::remove_file(&p).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) }) } else { crate::engine::write_atomic(&p, d.as_bytes()) };
        match r {
            Ok(()) => self.prompt_saved = d,
            Err(e) => self.toast(format!("prompt draft not saved: {e}")),
        }
    }

    /// The view opens: the unsent prompt of its last view back in the prompt.
    fn load_prompt(&mut self) {
        let Ok(raw) = std::fs::read_to_string(prompt_path(&self.id)) else { return };
        let Ok(v) = serde_json::from_str::<Value>(&raw) else { return };
        // messages the log never got (a crash right after the send): back in the prompt
        let sent = self.thread.sent();
        let mut parts: Vec<String> = v["unsent"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|u| !sent.iter().any(|t| t.starts_with(u) || u.starts_with(t.as_str())))
            .map(String::from)
            .collect();
        let text = v["text"].as_str().unwrap_or_default();
        if !text.trim().is_empty() {
            parts.push(text.to_string());
        }
        if parts.is_empty() {
            return;
        }
        let text = parts.join("\n\n");
        self.input.set_text(&text);
        self.input.move_end();
        self.attachments = v["attachments"].as_array().into_iter().flatten().filter_map(|a| Some((a[0].as_str()?.to_string(), PathBuf::from(a[1].as_str()?)))).collect();
        self.prompt_saved = raw;
    }

    /// The changes view's agents (the comment modal's `@` dropdown, its mentions).
    fn changes_agents(&mut self) {
        self.changes.agents = self.roster.clone();
        self.changes.members = self.members.clone();
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
                (a.name.clone(), (crate::agents::sprite(a).desynced(&a.name), effort))
            })
            .collect();
        self.roster = roster;
        self.members = store::load(&self.id).map(|t| t.agents).unwrap_or_default();
        self.mention.replaceable = self.members.clone();
        self.changes_agents();
        self.sync_order();
        self.sync_flowbars();
    }

    /// The box order after a change of the members: gone ones out, new ones in (at the front
    /// when @named, else at the end), each new one spawns. The 1st load: all spawn (the thread
    /// opens).
    fn sync_order(&mut self) {
        let first = self.order.is_empty();
        self.order.retain(|n| self.members.contains(n));
        for n in self.members.clone() {
            if self.order.contains(&n) {
                continue;
            }
            if self.front.contains(&n) {
                self.order.insert(0, n.clone());
            } else {
                self.order.push(n.clone());
            }
            let sp = self.new_spawn();
            self.spawns.insert(n, sp);
        }
        self.front.retain(|n| !self.members.contains(n));
        if first {
            self.dirty = true;
        }
    }

    /// NAMED were addressed: to the front of the box; one out of view spawns.
    fn bring_front(&mut self, named: &[String]) {
        let shown: HashSet<String> = self.slots.borrow().iter().map(|(n, _)| n.clone()).collect();
        for n in named.iter().rev() {
            match self.order.iter().position(|x| x == n) {
                Some(i) => {
                    let n = self.order.remove(i);
                    if !shown.contains(&n) {
                        let sp = self.new_spawn();
                        self.spawns.insert(n.clone(), sp);
                    }
                    self.order.insert(0, n);
                }
                None if !self.front.contains(n) => self.front.push(n.clone()),
                None => {}
            }
        }
    }

    /// A spawn for 1 agent: an animation no other agent of the thread plays now.
    fn new_spawn(&self) -> crate::avatar::Spawn {
        let taken: Vec<usize> = self.spawns.values().filter(|s| !s.done()).map(|s| s.anim).collect();
        crate::avatar::Spawn::random_except(&taken)
    }

    /// A spawn (or its text decode) runs: frames at 30 per second.
    fn spawning(&self) -> bool {
        self.spawns.values().any(|s| !s.over()) || crate::cost::morphing()
    }

    /// The color of agent NAME now: blocked (a question waits on the owner): all warn yellow,
    /// else its own.
    fn color(&self, name: &str) -> Color {
        if self.thread.asks.iter().any(|q| q.agent == name) { theme::pal().attention() } else { self.colors.get(name).copied().unwrap_or(theme::pal().accent) }
    }

    /// The agent colors now (blocked agents: yellow).
    fn colors_now(&self) -> stream::Colors {
        let mut c = self.colors.clone();
        for q in &self.thread.asks {
            c.insert(q.agent.clone(), theme::pal().attention());
        }
        c
    }

    /// The avatar status of agent NAME.
    fn avatar_status(&self, name: &str) -> crate::avatar::Status {
        use crate::avatar::Status;
        if self.thread.asks.iter().any(|q| q.agent == name) {
            Status::Blocked
        } else if self.thread.busy.contains(name) || self.thread.working(name) {
            Status::Working
        } else if self.monitors.watches.iter().any(|w| w.agent == name) {
            Status::Thinking
        } else {
            Status::Idle
        }
    }

    /// The spawns over the header (and what is below it), in the agent colors.
    fn draw_spawns(&mut self, f: &mut Frame) {
        self.spawns.retain(|_, s| !s.over());
        let area = f.area();
        for (name, slot) in self.slots.borrow().iter() {
            let Some(sp) = self.spawns.get(name) else { continue };
            let sprite = self.profiles.get(name).map(|p| p.0.clone()).unwrap_or_else(|| crate::sprites::default_for(name).desynced(name));
            let Some(rows) = sp.rows(&sprite.avatar, self.avatar_status(name)) else { continue };
            let color = self.color(name);
            let (ax, ay) = crate::avatar::SPAWN_AT;
            let (x0, y0) = (slot.x as i32 - ax as i32, slot.y as i32 - ay as i32);
            let buf = f.buffer_mut();
            for (r, row) in rows.iter().enumerate() {
                for (c, ch) in row.chars().enumerate() {
                    let (x, y) = (x0 + c as i32, y0 + r as i32);
                    // the header rows only (its last row: the input bubble's top line)
                    let bottom = (slot.y + slot.height) as i32;
                    if x < area.x as i32 || y < area.y as i32 || x >= area.right() as i32 || y >= (area.bottom() as i32).min(bottom) {
                        continue;
                    }
                    // outside the sprite: blank cells stay transparent (the cards next to it show)
                    let inside = (slot.x as i32..(slot.x + slot.width) as i32).contains(&x) && (slot.y as i32..bottom).contains(&y);
                    if !inside && (ch == ' ' || ch == '\u{2800}') {
                        continue;
                    }
                    buf[(x as u16, y as u16)].set_char(ch).set_style(theme::text().fg(color));
                }
            }
        }
    }

    /// New events of the log. True: some came.
    fn read(&mut self) -> bool {
        let evs = self.tail.read();
        if evs.is_empty() {
            return false;
        }
        if let Some(early) = self.early.as_mut() {
            early.extend(evs.iter().cloned());
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
                self.sync_order();
                self.sync_flowbars();
            }
        }
        self.place_flow_ask();
        if self.thread.asks.first().map(|a| a.id.clone()) != asks {
            self.sync_ask();
        }
        self.history = self.thread.sent();
        self.dirty = true;
        true
    }

    /// The log's head is in: the stream again from the 1st event (head, then the tail so far).
    fn take_head(&mut self, head: Vec<store::Event>) {
        let Some(early) = self.early.take() else { return };
        let mut t = stream::Thread::default();
        for e in head.iter().chain(&early) {
            t.push(e);
        }
        let asks = self.thread.asks.first().map(|a| a.id.clone());
        let shift = t.items.len().saturating_sub(self.thread.items.len());
        self.thread = t;
        // the head in front: every item index moved, the search's texts again
        self.find.reset(shift);
        self.place_flow_ask();
        if self.thread.asks.first().map(|a| a.id.clone()) != asks {
            self.sync_ask();
        }
        self.history = self.thread.sent();
        self.dirty = true;
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

    /// The mode the keys are in, for the footer's badge: (label, risky). 0 mode: None.
    fn mode(&self) -> Option<(&'static str, bool)> {
        let view_keys = !self.prompt_focus;
        if self.agents_focus.is_some() {
            Some(("agents", false))
        } else if self.find.open && self.find.focus {
            Some(("search", false))
        } else if self.pane == Pane::Changes && view_keys && let Some(m) = self.changes.mode() {
            Some((m, false))
        } else if self.pane == Pane::Files && view_keys && self.files.typing() {
            Some(("filter", false))
        } else if self.shell_mode() && (self.pane == Pane::Chat || self.prompt_focus) {
            Some(("shell", true))
        } else {
            None
        }
    }

    /// Shell mode: the prompt starts with `!`.
    fn shell_mode(&self) -> bool {
        self.input.text().starts_with('!')
    }

    /// The thread's `!commands`, oldest first.
    fn shell_cmds(&self) -> Vec<String> {
        self.thread.items.iter().filter_map(|i| if let stream::Item::Shell { command, .. } = i { Some(command.clone()) } else { None }).collect()
    }

    /// Shell mode, not in the `!commands` (↑↓): the completions and the suggestion are on.
    fn comp_on(&self) -> bool {
        self.shell_mode() && self.shell_at.is_none() && !self.hist.is_open()
    }

    /// Run COMMAND (shell mode) in the worktree, off the UI thread: $SHELL -c, stdout and stderr
    /// together, at most SHELL_TIMEOUT; Msg::Shell when it ends.
    fn run_shell(&mut self, command: String) {
        // `cd DIR` alone: shell mode's dir (every command runs in a new shell)
        if let Some(dir) = cd_target(&command) {
            self.set_shell_cwd(if dir.is_empty() { "~" } else { dir });
            return;
        }
        self.shell_running += 1;
        self.toast(format!("! {command}"));
        let (cwd, tx) = (self.shell_cwd.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let (output, code) = shell(&cwd, &command);
            let _ = tx.send(Msg::Shell(command, output, code, cwd));
        });
    }

    /// Shell mode's dir at the view's start: the owner's last pick, else (in the background) the
    /// thread's checkout with the most line changes.
    fn load_shell_cwd(&mut self) {
        let saved = std::fs::read_to_string(store::thread_dir(&self.id).join("shell_cwd")).ok().map(|s| PathBuf::from(s.trim())).filter(|p| p.is_dir());
        if let Some(p) = saved {
            self.shell_cwd = p;
            return;
        }
        let Some(t) = store::load(&self.id) else { return };
        let tx = self.tx.clone();
        let work = move || {
            if let Some(d) = busiest_checkout(&t) {
                let _ = tx.send(Msg::ShellCwd(d));
            }
        };
        if cfg!(test) {
            work();
        } else {
            std::thread::spawn(work);
        }
    }

    /// TYPED (`~/x`, `/x`, `x` from the dir now): shell mode's dir, kept for the thread. False: 0
    /// such dir.
    fn set_shell_cwd(&mut self, typed: &str) -> bool {
        let t = typed.trim().trim_matches('"');
        let p = if t == "~" {
            crate::config::home()
        } else if let Some(rest) = t.strip_prefix("~/") {
            crate::config::home().join(rest)
        } else {
            self.shell_cwd.join(t)
        };
        let Some(p) = std::fs::canonicalize(&p).ok().filter(|p| p.is_dir()) else {
            self.toast(format!("cd: 0 dir {t}"));
            return false;
        };
        let dir = store::thread_dir(&self.id);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("shell_cwd"), p.display().to_string());
        self.toast(format!("shell mode: in {}", tilde(&p)));
        self.shell_cwd = p;
        self.dirty = true;
        true
    }

    /// ↓ at the bottom of shell mode: the dir input, the dir with a `/` (its entries show).
    fn edit_cwd(&mut self) {
        let mut ta = TextArea::new(true, "a dir");
        ta.style = Some(theme::text());
        let t = tilde(&self.shell_cwd);
        let t = if t.ends_with('/') { t } else { format!("{t}/") };
        // a space in the path: quoted (the dropdown goes on inside the quote)
        ta.set_text(&if t.contains(char::is_whitespace) { format!("\"{t}") } else { t });
        self.cwd_paths = crate::paths::Paths::directory(&self.shell_cwd);
        self.cwd_paths.dirs_only = true;
        self.cwd_edit = Some(ta);
        self.dirty = true;
    }

    /// Key K for the dir input (open). Enter: the dir (0 such dir: the input stays), esc / ↑:
    /// back to the prompt.
    fn cwd_key(&mut self, k: KeyEvent) {
        let Some(mut ta) = self.cwd_edit.take() else { return };
        if self.cwd_paths.key(k, &mut ta) {
            self.cwd_edit = Some(ta);
            return;
        }
        match k.code {
            KeyCode::Esc | KeyCode::Up => {
                self.cwd_paths.reset_input();
                return;
            }
            KeyCode::Enter => {
                let typed = ta.text();
                if typed.trim().is_empty() || self.set_shell_cwd(&typed) {
                    self.cwd_paths.reset_input();
                    return;
                }
            }
            _ => {
                let _ = ta.key(k);
            }
        }
        self.cwd_edit = Some(ta);
    }

    /// ↑↓ in shell mode: the thread's `!commands` (UP: older), like a shell. True: taken (↓ past
    /// the newest: the draft again; ↓ on the draft: false).
    fn shell_history_step(&mut self, up: bool) -> bool {
        let mut cmds: Vec<String> = self.thread.items.iter().filter_map(|i| if let stream::Item::Shell { command, .. } = i { Some(command.clone()) } else { None }).collect();
        // newest kept: a repeat counts once, at its last run
        let mut seen = std::collections::HashSet::new();
        cmds.reverse();
        cmds.retain(|c| seen.insert(c.clone()));
        cmds.reverse();
        let at = match (self.shell_at, up) {
            (None, false) => return false,
            (None, true) if cmds.is_empty() => return true,
            (None, true) => {
                self.shell_draft = self.input.text();
                cmds.len() - 1
            }
            (Some(i), true) => i.saturating_sub(1),
            (Some(i), false) if i + 1 < cmds.len() => i + 1,
            (Some(_), false) => {
                self.shell_at = None;
                let d = std::mem::take(&mut self.shell_draft);
                self.input.load_text(&d);
                return true;
            }
        };
        self.shell_at = Some(at);
        self.input.load_text(&format!("! {}", cmds[at]));
        true
    }

    /// The shell COMMAND ended: into the thread's log (all see it), its output along with the
    /// owner's next message.
    fn shell_done(&mut self, command: String, output: String, code: i32, dir: &Path) {
        self.shell_running = self.shell_running.saturating_sub(1);
        let cut: String = output.chars().take(SHELL_CTX_CHARS).collect();
        let more = if cut.len() < output.len() { "\n… (cut)" } else { "" };
        self.shell_ctx.push(format!("$ {command}  (exit {code}, in {})\n```\n{}{more}\n```", dir.display(), cut.trim_end()));
        #[cfg(not(test))]
        if let Err(e) = store::append(&self.id, &store::Event::new(None, store::Kind::Shell { command, output, code, dir: Some(dir.display().to_string()) })) {
            self.toast(format!("shell: {e}"));
        }
        #[cfg(test)]
        self.thread.items.push(stream::Item::Shell { command, output, code, ts: 0, dir: Some(dir.display().to_string()) });
        self.dirty = true;
    }

    fn submit(&mut self) {
        let mut text = crate::textarea::without_empty_items(&self.input.text()).to_string();
        if text.trim().is_empty() {
            return;
        }
        // shell mode: `!command` runs here, 0 message
        if let Some(cmd) = text.strip_prefix('!') {
            let cmd = cmd.trim().to_string();
            if cmd.is_empty() {
                return;
            }
            self.input.set_text("");
            self.hist_at = None;
            self.shell_at = None;
            self.back = 0;
            self.run_shell(cmd);
            return;
        }
        // `#name` of a shortcut: its text
        text = crate::shortcuts::expand(&text);
        // from the changes view: the selected lines and the picked comments go along (once)
        if self.pane == Pane::Changes
            && let Some(c) = self.changes.context()
        {
            text = format!("{text}\n\n{c}");
            self.mini_chat = true;
            self.changes.took_context();
        }
        // from the merge tab: the pull request in view
        if self.pane == Pane::Merge
            && let Some(c) = self.merge.context()
        {
            text = format!("{text}\n\n{c}");
        }
        let files: Vec<String> = self.attachments.iter().filter(|(m, _)| text.contains(m.as_str())).map(|(_, p)| format!("- {}", p.display())).collect();
        if !files.is_empty() {
            text = format!("{text}\n\n{}\n{}", stream::ATTACHMENTS, files.join("\n"));
        }
        // `>sub task`: a sub-thread does the task, this thread waits
        if let Some(task) = crate::subthreads::task_of(&text) {
            if task.is_empty() {
                self.toast(">sub: the task goes after it (@agent, #shortcut, any text)".to_string());
                return;
            }
            self.attachments.clear();
            self.input.set_text("");
            self.hist_at = None;
            self.back = 0;
            return self.start_sub(task);
        }
        self.attachments.clear();
        self.input.set_text("");
        self.hist_at = None;
        self.back = 0;
        let replacing =self.replace.take().filter(|(new, old)| text.contains(&format!("@{new}{}", crate::mention::replaces_note(old))));
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
        self.bring_front(&named);
        self.pending.push((text.clone(), Instant::now(), None));
        // the owner's shell commands since the last message: to the agents along
        let context = if self.shell_ctx.is_empty() {
            String::new()
        } else {
            format!("[codehead: the owner ran these shell commands since the last message]\n\n{}", std::mem::take(&mut self.shell_ctx).join("\n\n"))
        };
        // a flow wait's reply: to that flow's agent (+ the @named ones)
        if let Some(a) = self.reply_to.take() {
            let mut to = named;
            if !to.contains(&a) {
                to.insert(0, a);
            }
            self.host("send", json!({"op": "prompt", "text": text, "to": to, "context": context}));
        } else if named.is_empty() {
            self.host("send", json!({"op": "prompt", "text": text, "to": self.default_to(), "context": context}));
        } else {
            self.host("send", json!({"op": "prompt", "text": text, "context": context}));
        }
    }

    /// `>sub TASK`: a new thread for TASK in this thread's checkout (same dir, worktree, branch,
    /// port), 0 history of this one, TASK's @agents (else the agents of the last message). This
    /// thread's host holds its agents' messages until the sub-thread reports its end.
    fn start_sub(&mut self, task: String) {
        let Some(main) = store::load(&self.id) else { return };
        let names: Vec<String> = self.names.iter().map(|n| n.0.clone()).chain(self.members.iter().cloned()).collect();
        let named = crate::engine::host::mentions(&task, &names);
        let agents = if named.is_empty() { self.default_to() } else { named };
        if agents.is_empty() {
            self.toast(">sub: 0 agent · @name one".to_string());
            return;
        }
        let first: String = task.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim().to_string();
        let title: String = first.chars().take(60).collect();
        let mut t = store::Thread::new(&title, &task, Path::new(&main.dir), agents.clone());
        t.worktree = main.worktree.clone();
        t.branch = main.branch.clone();
        t.port = main.port;
        t.of = Some(main.id.clone());
        if let Err(e) = t.save() {
            self.toast(format!(">sub: {e}"));
            return;
        }
        let who = format!("@{}", agents.join(" @"));
        self.host("sub_start", json!({"op": "sub_start", "id": t.id, "title": title, "text": format!("{who} · {first}")}));
        self.subthreads.started(crate::subthreads::Kid { id: t.id.clone(), title: title.clone(), agents: agents.clone(), start: t.created, busy: false, asking: false, watches: 0, steps: Vec::new() });
        let context = format!(
            "[codehead: you run in a sub-thread of the thread “{}”: the same checkout ({}), 0 history of it. The message below is your whole task. Do it, then end with your result as your last message: the thread waits for you; it gets that message.]",
            main.title.lines().next().unwrap_or_default(),
            main.work_dir().display()
        );
        // its host starts in the background: the view never waits for a harness (tests: 0 host)
        let id = t.id.clone();
        if !cfg!(test) {
            std::thread::spawn(move || {
                let _ = crate::engine::client::call(&id, &json!({"op": "prompt", "to": agents, "text": task, "context": context}));
            });
        }
        self.toast(format!("sub-thread “{title}” started · {who}"));
    }

    /// Stop sub-thread KID: its host ends, its end reported as stopped, the thread deleted.
    fn stop_sub(&mut self, kid: &str) {
        let (title, agent) = self.subthreads.kids.iter().find(|k| k.id == kid).map(|k| (k.title.clone(), k.agents.first().cloned().unwrap_or_default())).unwrap_or_default();
        let _ = crate::engine::client::send_within(kid, &json!({"op": "stop"}), Duration::from_secs(2));
        self.host("sub_done", json!({"op": "sub_done", "id": kid, "title": title, "agent": agent, "state": "stopped", "text": "stopped by the owner"}));
        let _ = store::delete(kid);
        self.subthreads.gone(kid);
        self.toast(format!("sub-thread “{title}” stopped"));
    }

    /// Lost subagent I: its master picks up its work (its transcript, its task), as a message of
    /// the owner to the master.
    fn pick_up(&mut self, i: usize) {
        let Some(s) = self.thread.subs.get(i).cloned() else { return };
        let mut text = format!("@{} your subagent “{}” ended without a report ({}). Pick up its work from its context.", s.master, s.description, s.result.lines().next().unwrap_or("lost"));
        if !s.transcript.is_empty() {
            text.push_str(&format!(" Its transcript: `{}` (its last steps show where it stopped).", s.transcript));
        }
        if !s.prompt.trim().is_empty() {
            let quoted: String = s.prompt.trim().lines().map(|l| format!("> {l}\n")).collect();
            text.push_str(&format!("\n\nIts task was:\n\n{quoted}"));
        }
        text.push_str("\nFinish the task yourself, or start a new subagent with this context.");
        self.pending.push((text.clone(), Instant::now(), None));
        self.host("send", json!({"op": "prompt", "text": text, "to": [s.master]}));
        self.toast(format!("{} picks up “{}”", s.master, s.description));
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
            let r = client::call(&id, &json!({"op": "leave", "agent": old, "to": new})).and_then(|_| client::call(&id, &json!({"op": "prompt", "to": [new], "text": prompt})));
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
        // the avatars of the agents box move
        !self.members.is_empty() || !self.thread.busy.is_empty() || self.monitors.active() || crate::subagents::Subagents::active(&self.thread.subs) || self.subthreads.active() || self.toast.as_ref().is_some_and(|(_, at)| at.elapsed() < Duration::from_secs(4))
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
            // the flow's wait: its step (flow_answer with the prompt's note, flow_role), 0 host ask
            crate::ask::Act::Answer(c) if c["id"].as_str().is_some_and(|id| id.starts_with("flow:")) => {
                let option = c["option"].as_str().unwrap_or_default().to_string();
                let empty = self.input.text().trim().is_empty();
                let id = c["id"].as_str().unwrap_or_default().to_string();
                let Some(b) = self.flowbars.iter().position(|b| b.wait_ask().is_some_and(|a| a.id == id)) else { return true };
                // reply: the prompt's text as a message to the waiting flow's agent, the wait stays
                if option == crate::flowbar::REPLY {
                    self.ask.focus = false;
                    if empty {
                        self.toast("reply: type the message in the prompt first (ctrl+enter sends it too)");
                    } else {
                        self.reply_to = Some(self.flowbars[b].agent.clone());
                        self.submit();
                        self.reply_to = None;
                    }
                    self.dirty = true;
                    return true;
                }

                // a gate's outcome back to a step: 0 send without the owner's words what to change
                if empty && option != "approve" && self.flowbars[b].waits_gate() {
                    self.ask.focus = false;
                    self.toast(format!("{option}: type what to change in the prompt first, then pick it again"));
                    self.dirty = true;
                    return true;
                }

                if let Some(act) = self.flowbars[b].wait_answer(&option) {
                    let agent = self.flowbars[b].agent.clone();
                    self.flow_act(&agent, act);
                }
                self.sync_flow_ask();
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
    /// A key of the flow box asks for ACT.
    fn flow_act(&mut self, agent: &str, act: crate::flowbar::Act) {
        match act {
            crate::flowbar::Act::None => {}
            crate::flowbar::Act::Host(op, mut c) => {
                c["agent"] = json!(agent);
                self.host(op, c);
            }
            crate::flowbar::Act::Answer(choice) => {
                // the prompt's text: the owner's note to the agent
                let note = self.input.text().trim().to_string();
                if !note.is_empty() {
                    self.input.load_text("");
                }
                self.host("flow_answer", json!({"op": "flow_answer", "agent": agent, "choice": choice, "note": note}));
                self.toast(format!("flow: {choice}"));
            }
            crate::flowbar::Act::Edit(slug) => self.exit = Exit::Home(crate::menu::Home::Flows(Some(slug))),
        }
    }

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
    /// `>reload`: the idle sessions of the thread's agents end; each agent's next message starts
    /// a new harness process (resumed: the conversation stays) with the MCP servers and settings
    /// of now. A busy agent: after its turn.
    fn reload_sessions(&mut self) {
        let Some(t) = store::load(&self.id) else { return };
        let (mut done, mut busy) = (Vec::new(), Vec::new());
        for a in &t.agents {
            if self.thread.busy.contains(a) {
                busy.push(a.clone());
                continue;
            }
            // 0 host: 0 session runs, the next message starts a new one anyway
            if crate::engine::client::send_within(&self.id, &serde_json::json!({"op": "reload", "agent": a}), Duration::from_secs(5)).is_ok() {
                done.push(a.clone());
            }
        }
        let mut msg = if done.is_empty() { "reload: 0 sessions ran · the next message starts new ones".to_string() } else { format!("reload: {} · the next message starts a new session", done.join(", ")) };
        if !busy.is_empty() {
            msg.push_str(&format!(" · mid-turn: {} (>reload after the turn)", busy.join(", ")));
        }
        self.toast(msg);
    }

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
            Pasted::Text(t) => self.input.paste(&t),
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
    /// The changes view has the keys (0 text input): every key as an escape code, for an alt
    /// tap.
    fn all_keys(&self) -> bool {
        self.pane == Pane::Changes && !self.prompt_focus && self.menu.is_none() && !self.changes.typing()
    }

    fn key(&mut self, k: KeyEvent) -> bool {
        // a modifier alone (press, release): an alt tap in the changes view, else 0 effect
        if matches!(k.code, KeyCode::Modifier(_)) {
            if self.all_keys() && self.changes.modifier(k) {
                self.dirty = true;
            }
            return false;
        }
        self.last_key = Some(Instant::now());
        self.changes.no_tap();
        self.merge.touch();
        // a key after a search jump: the mark goes
        if self.menu.is_none() && self.flash.take().is_some() {
            self.dirty = true;
        }

        // ctrl+m: the prompt, from every pane, box, filter, dropdown and the menu (kitty protocol;
        // a legacy terminal sends ctrl+m as enter)
        if k.code == KeyCode::Char('m') && k.modifiers == KeyModifiers::CONTROL {
            self.focus_prompt();
            return false;
        }

        // notifications (ctrl+n): over everything, the menu too
        if let Some(p) = self.notes_panel.as_mut() {
            let pick = p.key(k);
            return self.notes_pick(pick);
        }
        if crate::notify::is_key(&k) {
            self.menu = None;
            self.notes_panel = Some(crate::notify::Panel::open(Some(&self.id)));
            self.dirty = true;
            return false;
        }
        if let Some(m) = self.menu.as_mut() {
            let a = m.key(k);
            return self.menu_action(a);
        }
        // the menu key (ctrl+p): the menu from every field, box and dropdown
        if crate::config::menu_key().matches(&k) {
            self.open_menu();
            return false;
        }

        // ctrl+q: the agents box above the prompt (←→ an agent, its menu: flow, remove, replace)
        if k.code == KeyCode::Char('q') && k.modifiers == KeyModifiers::CONTROL {
            self.focus_agents(None);
            return false;
        }

        if self.agents_focus.is_some() {
            self.agents_key(k);
            self.dirty = true;
            return false;
        }

        // the IDE's repo picker: its keys first
        if self.ide_pick.is_some() {
            self.ide_pick_key(k);
            self.dirty = true;
            return false;
        }
        // ctrl+i / ⌘i: the selected file in the IDE (the changes / files view, 0 prompt focus),
        // else the IDE on the repo root, from every pane and field (several repos: a picker)
        if k.code == KeyCode::Char('i') && k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER) && !k.modifiers.contains(KeyModifiers::ALT) {
            let open = KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE);
            if self.pane == Pane::Changes && !self.prompt_focus && !self.changes.typing() {
                let act = self.changes.key(open);
                if act != crate::changes::Act::None {
                    self.changes_act(act);
                    return false;
                }
            }

            if self.pane == Pane::Files && !self.prompt_focus && !self.files.typing() {
                let act = self.files.key(open);
                if act != crate::files::Act::None {
                    self.files_act(act);
                    return false;
                }
            }
            self.ide_key();
            self.dirty = true;
            return false;
        }
        // ctrl+f: the chat's search bar (open: the keys back to it); another view: its filter
        if crate::textarea::is_filter_key(&k) {
            if self.pane == Pane::Chat || (self.wide_split && self.prompt_focus) {
                self.flowbars.iter_mut().for_each(crate::flowbar::FlowBar::close);
                self.find.open();
                self.find_to = self.find.current();
                self.dirty = true;
                return false;
            }

            // the prompt of another view: the view takes the keys, ctrl+f opens its filter
            self.prompt_focus = false;
        }

        // the open search bar: its keys (text, tab / ↑↓: the matches, esc: close)
        if self.find.focus {
            match self.find.key(k, &self.thread) {
                crate::chatsearch::Act::Pass => {}
                crate::chatsearch::Act::None => {
                    self.dirty = true;
                    return false;
                }
                crate::chatsearch::Act::Show(i) => {
                    self.find_show(i);
                    return false;
                }
                crate::chatsearch::Act::Close => {
                    self.dirty = true;
                    return false;
                }
            }
        }

        if let Some((agent, act)) = self.flowbars.iter_mut().find_map(|b| b.key(k).map(|a| (b.agent.clone(), a))) {
            let edit = matches!(act, crate::flowbar::Act::Edit(_));
            self.flow_act(&agent, act);
            self.dirty = true;
            return edit;
        }
        // the open sub-threads box: its keys (d: stop the selected one, enter: open it)
        if let Some(act) = self.subthreads.key(k) {
            match act {
                crate::subthreads::Act::Stop(kid) => self.stop_sub(&kid),
                crate::subthreads::Act::Open(kid) => {
                    if let Some(t) = store::load(&kid) {
                        self.exit = Exit::Open { id: t.id.clone(), dir: t.work_dir(), title: t.title.clone(), note: "sub-thread · esc esc: back to its thread".into() };
                        return true;
                    }
                }
                crate::subthreads::Act::None => {}
            }
            return false;
        }
        // the open subagents box: its keys (d: stop the selected one)
        if let Some(act) = self.subagents.key(k, &self.thread.subs) {
            if let crate::subagents::Act::Stop(id) = act {
                let what = self.thread.subs.iter().find(|s| s.id == id).map(|s| s.description.clone()).unwrap_or_default();
                self.host("sub_stop", json!({"op": "sub_stop", "id": id}));
                self.toast(format!("stopping subagent “{what}”…"));
            }
            return false;
        }
        if self.monitors.key(k) {
            if let Some(n) = self.monitors.take_note() {
                self.toast(n);
            }
            return false;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        // alt+\: the chat next to the changes view on / off (wide terminals)
        if alt && !ctrl && k.code == KeyCode::Char('\\') {
            self.split = !self.split;
            self.dirty = true;
            self.toast(if self.split { "changes: the chat on the right (≥ 160 columns)" } else { "changes: full width" });
            return false;
        }
        // views of the thread: F1-F4, ctrl+1-4 (kitty protocol), alt+1-4 (chat, changes, merge,
        // files). macOS terminals without option-as-alt send ⌥1 as `¡`: F-keys and ctrl work there
        let view_key = match k.code {
            KeyCode::F(n @ 1..=4) if k.modifiers.is_empty() => Some(usize::from(n) - 1),
            KeyCode::Char(c @ '1'..='4') if alt != ctrl => Some(usize::from(c as u8 - b'1')),
            _ => None,
        };
        if let Some(p) = view_key.and_then(|i| Pane::ALL.get(i)) {
            self.set_pane(*p);
            return false;
        }
        // ctrl+b from the prompt of the changes view: the panel's cycle; the panel with the keys:
        // the keys leave the prompt for it
        if self.pane == Pane::Changes && self.prompt_focus && ctrl && !alt && k.code == KeyCode::Char('b') {
            self.changes.toggle_panel();
            if self.changes.panel_focus() {
                self.prompt_focus = false;
            }
            self.dirty = true;
            return false;
        }
        // the files view's filter input: every key but ctrl+c (esc: clears it)
        if self.pane == Pane::Files && !self.prompt_focus && self.files.typing() && !(ctrl && k.code == KeyCode::Char('c')) {
            let act = self.files.key(k);
            self.files_act(act);
            return false;
        }
        let tab = matches!(k.code, KeyCode::Tab | KeyCode::BackTab) && !ctrl && !alt;
        // the changes view with the focus: its keys (esc, ctrl+c: as in chat; tab: the prompt)
        if self.pane != Pane::Chat && !self.prompt_focus {
            // tab: the next field, the prompt (as i)
            if tab && !(self.pane == Pane::Changes && self.changes.typing()) && !(self.pane == Pane::Merge && self.merge.typing()) {
                self.prompt_focus = true;
                return false;
            }
            // the changes view's filter, selection: esc is the view's
            let esc_here = k.code == KeyCode::Esc && ((self.pane == Pane::Changes && self.changes.takes_esc()) || (self.pane == Pane::Merge && self.merge.takes_esc()));
            let through = (k.code == KeyCode::Esc && !esc_here) || (ctrl && k.code == KeyCode::Char('c'));
            if !through {
                // i: the prompt (the changes and files views: i opens the file; ctrl+m: the prompt)
                if k.code == KeyCode::Char('i') && !ctrl && !alt && !matches!(self.pane, Pane::Changes | Pane::Files) && !(self.pane == Pane::Merge && self.merge.typing()) {
                    self.prompt_focus = true;
                    return false;
                }
                if self.pane == Pane::Merge {
                    let act = self.merge.key(k);
                    self.merge_act(act);
                    return false;
                }
                if self.pane == Pane::Changes && ctrl && k.code == KeyCode::Char('w') && !self.changes.typing() {
                    self.next_checkout();
                    return false;
                }
                // shift+←→: the repo tabs (←→ alone fold the changes)
                if self.pane == Pane::Changes && k.modifiers.contains(KeyModifiers::SHIFT) && matches!(k.code, KeyCode::Left | KeyCode::Right) && !self.changes.typing() {
                    self.load_checkouts();
                    let n = self.checkouts.len();
                    self.checkout_tabs.i = self.checkout;
                    if self.checkout_tabs.step(if k.code == KeyCode::Right { 1 } else { -1 }, n) {
                        self.pick_checkout(self.checkout_tabs.i);
                    }
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
        // an open ask with the focus: its keys before the prompt's (text in the prompt or not);
        // another view on screen: tab from the prompt goes back to that view, not to the ask;
        // the prompt with the focus and a dropdown open: tab completes there, not to the ask
        let complete = tab && !self.ask.focus && !self.thread.asks.is_empty() && self.prompt_dropdown();
        if let Some(a) = self.thread.asks.first().cloned()
            && !(tab && self.pane != Pane::Chat && self.prompt_focus)
            && !complete
        {
            let act = self.ask.key(&a, k);
            if self.ask_act(act) {
                return false;
            }
        }
        // the dir input of shell mode: every key while open
        if self.cwd_edit.is_some() {
            self.cwd_key(k);
            return false;
        }
        // ctrl+r in shell mode: the history search (its keys while open)
        if self.hist.key(k) {
            if let Some(t) = self.hist.picked.take() {
                self.input.set_text(&t);
            }
            return false;
        }
        if k.code == KeyCode::Char('r') && k.modifiers.contains(KeyModifiers::CONTROL) && self.shell_mode() {
            let cmds = self.shell_cmds();
            let text = self.input.text();
            self.hist.open(cmds, text.strip_prefix('!').unwrap_or_default().trim_start());
            return false;
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
                Some(crate::command::Action::Reload) => self.reload_sessions(),
                Some(crate::command::Action::Flow) => {
                    let agent = self.default_to().into_iter().next().unwrap_or_default();
                    self.open_flow(&agent, true);
                }
                None => {}
            }
            return false;
        }
        if self.shortcuts.key(k, &mut self.input) || self.paths.key(k, &mut self.input) {
            return false;
        }
        // shell mode: the completions, then the history's suggestion (→ / End)
        if self.comp_on() {
            let cmds = self.shell_cmds();
            if self.comp.key(k, &mut self.input, &self.shell_cwd, &cmds) || self.comp.accept(k, &mut self.input, &cmds) {
                return false;
            }
        }
        let empty = self.input.is_empty();
        match k.code {
            // the prompt in the changes view: esc gives the keys back to the view (side by side
            // with the chat: an agent at work gets interrupted first, as in the chat)
            KeyCode::Esc if self.pane != Pane::Chat && self.prompt_focus && !(self.wide_split && (!self.thread.busy.is_empty() || self.queued.is_some() || self.back > 0)) => {
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
            // tab: the next field (dropdowns took theirs above): the prompt → the view in focus
            // before it (changes, merge, files); the chat alone: 0 other field
            KeyCode::Tab | KeyCode::BackTab if tab && !self.mention_open() => {
                if self.pane != Pane::Chat {
                    self.prompt_focus = false;
                }
                return false;
            }
            KeyCode::Char('c') if ctrl && self.sel.is_some() => {
                if let Some(t) = self.selected() {
                    self.copy(&t);
                }
                self.sel = None;
                return false;
            }
            // ctrl+c: the prompt's selection → copy; text → clear it (ctrl+z brings it back); an
            // empty prompt: quit on a 2nd ctrl+c within 1 s
            KeyCode::Char('c') if ctrl => {
                if let Some(t) = self.input.selected_text() {
                    self.copy(&t);
                    return false;
                }

                if !empty {
                    self.input.load_text("");
                    self.toast("prompt cleared · ctrl+z: back");
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
                    Pasted::Text(t) => self.input.paste(&t),
                    Pasted::File { path, marker } => {
                        self.input.insert(&marker);
                        self.attachments.push((marker, path));
                    }
                    Pasted::Nothing => self.toast("clipboard: 0 image, 0 text"),
                }
                return false;
            }
            // shell mode, 1 line: enter runs it
            KeyCode::Enter if k.modifiers.is_empty() && self.shell_mode() && !self.input.text().contains('\n') => {
                self.submit();
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
            // shell mode: ↑↓ the thread's `!commands`; ↓ below them: the dir input
            KeyCode::Up if self.shell_mode() && self.hist_at.is_none() && self.input.on_first_row() => {
                self.shell_history_step(true);
                return false;
            }
            KeyCode::Down if self.shell_mode() && self.hist_at.is_none() && self.input.on_last_row() => {
                if !self.shell_history_step(false) {
                    self.edit_cwd();
                }
                return false;
            }
            // `!` on an empty prompt: shell mode, a space after the `!`
            KeyCode::Char('!') if empty && !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                self.input.insert("! ");
                return false;
            }
            // 1 backspace on the bare `! ` (or `!`): shell mode off, the prompt empty
            KeyCode::Backspace if matches!(self.input.text().as_str(), "! " | "!") && !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER) => {
                self.input.set_text("");
                self.shell_at = None;
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
            if !matches!(k.code, KeyCode::Up | KeyCode::Down) {
                self.shell_at = None;
            }
        } else if k.code == KeyCode::Enter {
            self.input.newline();
        }
        self.hist.input_changed(&self.input.text());
        false
    }

    /// Switch to view P of the thread (the owner's pick: the view stops following the width).
    fn set_pane(&mut self, p: Pane) {
        self.auto_pane = false;
        self.switch_pane(p);
    }

    /// The view follows the width of AREA: wide (and a git checkout): changes + chat, the
    /// prompt has the keys; narrow: the chat.
    fn follow_width(&mut self, area: Rect) {
        if !self.auto_pane || !self.split {
            return;
        }
        if self.checkouts.is_empty() {
            self.load_checkouts();
        }
        let wide = area.width >= SPLIT_MIN && !self.checkouts.is_empty();
        let want = if wide { Pane::Changes } else { Pane::Chat };
        if self.pane != want && matches!(self.pane, Pane::Chat | Pane::Changes) {
            self.switch_pane(want);
            // typing goes to the prompt; alt (or esc) gives the keys to the changes
            self.prompt_focus = wide;
        }
    }

    /// Switch to view P. The changes view: git read at once, keys to the view.
    fn switch_pane(&mut self, p: Pane) {
        if self.pane == p {
            return;
        }
        // the search bar shows in the chat only: another view gets the keys
        if p != Pane::Chat {
            self.find.focus = false;
        }
        self.pane = p;
        self.prompt_focus = false;
        self.dirty = true;
        if p == Pane::Files {
            self.files.refresh(true);
        }
        if p == Pane::Changes {
            self.load_checkouts();
            self.changes.refresh(true);
        }
        if p == Pane::Merge {
            self.merge_sources(true);
        }
    }

    /// The merge tab's sources: the thread's checkouts (their repos' dirs), the PR links of its
    /// log (NOW: even when read less than 60 s ago).
    fn merge_sources(&mut self, now: bool) {
        if !now && self.merge_sources_at.is_some_and(|t| t.elapsed() < Duration::from_secs(60)) {
            return;
        }
        self.merge_sources_at = Some(Instant::now());
        let id = self.id.clone();
        let read = move || {
            let Some(t) = store::load(&id) else { return (Vec::new(), crate::merge::Sources::default()) };
            let checkouts: Vec<(PathBuf, Option<String>)> = t.all_checkouts().into_iter().map(|c| (PathBuf::from(c.dir), c.branch)).collect();
            let dirs = checkouts.iter().filter_map(|(dir, _)| crate::gh::repo_of_dir(dir).map(|r| (r, dir.clone()))).collect();
            let log = std::fs::read_to_string(store::events_path(&id)).unwrap_or_default();
            (dirs, crate::merge::Sources { checkouts, links: crate::gh::pr_urls(&log) })
        };
        // tests: at once
        if cfg!(test) {
            let r = read();
            self.apply_merge_sources(r);
            return;
        }
        if !self.merge_reads.waiting(&()) {
            self.merge_reads.request((), read);
        }
    }

    fn apply_merge_sources(&mut self, (dirs, sources): (Vec<(String, PathBuf)>, crate::merge::Sources)) {
        for (repo, dir) in dirs {
            self.merge.set_dir(&repo, dir);
        }
        self.merge.set_sources(sources);
    }

    /// A finished sources read: in the merge tab. True: 1 came.
    fn poll_merge_sources(&mut self) -> bool {
        let reads = self.merge_reads.poll();
        let any = !reads.is_empty();
        for (_, r) in reads {
            self.apply_merge_sources(r);
        }
        any
    }

    /// The thread's checkouts (more of them: an agent edited in another repo / worktree).
    fn load_checkouts(&mut self) {
        if let Some(t) = store::load(&self.id) {
            let mut all: Vec<store::Checkout> = t.all_checkouts().into_iter().filter(|c| Path::new(&c.dir).is_dir()).collect();
            // a work dir without git (codehead started outside of a repo), agents in a repo: the
            // changes of the repos only
            if all.iter().any(|c| crate::threads::repo_root(Path::new(&c.dir)).is_some()) {
                all.retain(|c| crate::threads::repo_root(Path::new(&c.dir)).is_some());
            }
            let name = |c: &store::Checkout| {
                let d = Path::new(&c.dir);
                let root = crate::threads::repo_root(d).unwrap_or_else(|| d.to_path_buf());
                root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| c.dir.clone())
            };
            let names: Vec<String> = all.iter().map(name).collect();
            // 2 checkouts of 1 repo: the branch tells them apart
            self.checkout_labels = all
                .iter()
                .zip(&names)
                .map(|(c, n)| match &c.branch {
                    Some(b) if names.iter().filter(|m| *m == n).count() > 1 => format!("{n} · {b}"),
                    _ => n.clone(),
                })
                .collect();

            // an agent moved the work to a worktree of the work dir's repo (`git worktree add`):
            // the newest such worktree is the thread's work, unless the owner picked a checkout
            // only from the main checkout to a linked worktree: a thread in its own worktree stays
            // there when an agent edits the main checkout (git writes a worktree's gitdir as a
            // real path: /var → /private/var on macOS)
            let work = t.work_dir();
            let canon = |d: &Path| std::fs::canonicalize(d).ok();
            let real = |d: &Path| crate::threads::repo_root(d).and_then(|r| canon(&r));
            let main = |d: &Path| real(d).is_some_and(|r| Some(r) == canon(d));
            let repo = real(&work);
            // (a submodule's root follows to the superproject too: linked worktrees only)
            let moved = main(&work).then(|| all.iter().rposition(|c| crate::threads::linked_worktree(Path::new(&c.dir)) && real(Path::new(&c.dir)) == repo)).flatten();
            if !self.checkout_picked {
                self.checkout = moved.unwrap_or(0);
            }

            self.checkouts = all.into_iter().map(|c| PathBuf::from(c.dir)).collect();
        }

        self.checkout = self.checkout.min(self.checkouts.len().saturating_sub(1));
        // the changes view on the checkout picked (the view opened on another dir: a work dir
        // without git, the dir of the thread before)
        if let Some(d) = self.checkouts.get(self.checkout).filter(|d| **d != self.changes.dir()).cloned() {
            self.changes = crate::changes::View::new(&self.id, &d);
            self.changes_agents();
            self.changes.refresh(true);
            self.dirty = true;
        }
    }

    /// The changes view on checkout I.
    fn pick_checkout(&mut self, i: usize) {
        if i == self.checkout || i >= self.checkouts.len() {
            return;
        }

        self.checkout = i;
        self.checkout_picked = true;
        let dir = self.checkouts[i].clone();
        self.changes = crate::changes::View::new(&self.id, &dir);
        self.changes_agents();
        self.changes.refresh(true);
        self.dirty = true;
    }

    /// ctrl+w in the changes view: the thread's next checkout.
    fn next_checkout(&mut self) {
        self.load_checkouts();
        if self.checkouts.len() < 2 {
            self.toast("1 checkout: an agent's edit in another repo / worktree adds 1");
            return;
        }
        let next = (self.checkout + 1) % self.checkouts.len();
        self.pick_checkout(next);
        self.toast(format!("changes: {} ({}/{})", tilde(&self.checkouts[next]), next + 1, self.checkouts.len()));
    }

    /// Show PR in the merge tab.
    pub fn show_pr(&mut self, pr: &crate::gh::PrRef) {
        self.set_pane(Pane::Merge);
        self.merge.show(pr);
    }

    fn merge_act(&mut self, a: crate::merge::Act) {
        match a {
            crate::merge::Act::None => {}
            crate::merge::Act::Toast(t) => self.toast(t),
            crate::merge::Act::Open(file, line) => self.open_at(&file, line),
            crate::merge::Act::Prompt(text) => {
                self.back = 0;
                let to = self.default_to().join(", ");
                self.send_prompt(text);
                self.toast(format!("sent to {to}"));
            }
        }
    }

    fn changes_act(&mut self, a: crate::changes::Act) {
        match a {
            crate::changes::Act::None => {}
            crate::changes::Act::Toast(t) => self.toast(t),
            crate::changes::Act::Open(file, line) => self.open_at(&file, line),
            crate::changes::Act::ToggleChat => {
                self.mini_chat = !self.mini_chat;
                self.dirty = true;
            }
            crate::changes::Act::Send(text) => {
                self.back = 0;
                let to = self.default_to().join(", ");
                self.send_prompt(text);
                self.toast(format!("review comments sent to {to}"));
            }
            // an @agent in a comment / reply: to that agent now, the comment's thread along
            crate::changes::Act::Mention { to, text, context } => {
                self.back = 0;
                self.bring_front(&to);
                self.pending.push((text.clone(), Instant::now(), None));
                self.host("send", json!({"op": "prompt", "text": text, "to": to, "context": context}));
                self.toast(format!("comment sent to {}", to.join(", ")));
            }
            crate::changes::Act::Checkout(root) => {
                self.load_checkouts();
                match self.checkouts.iter().position(|d| crate::changes::same_dir(d, &root)) {
                    Some(i) => self.pick_checkout(i),
                    None => self.toast(format!("{}: 0 checkout of this thread", root.display())),
                }
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
        self.open_editor(|k| crate::editor::args_at(k, &cwd, file, line), Some((file, line)), format!("{file}:{line}"));
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

    /// A dropdown of the prompt shows: @, commands, # shortcuts, paths.
    fn prompt_dropdown(&mut self) -> bool {
        self.mention_open() || self.command.is_open(&self.input) || self.shortcuts.is_open(&self.input) || self.paths.is_open(&self.input)
    }

    /// A new ask may take the focus: an empty prompt and 0 keys for 1 s.
    fn ask_may_focus(&self) -> bool {
        self.input.text().trim().is_empty() && self.last_key.is_none_or(|t| t.elapsed() >= Duration::from_secs(1))
    }

    /// Bind the picker to the 1st open ask.
    fn sync_ask(&mut self) {
        // a flow's wait never takes the keys: the owner may want to write first (tab: the choices)
        let focus = self.ask_may_focus() && self.thread.asks.first().is_none_or(|a| a.tool != crate::flowbar::WAIT_TOOL);
        self.ask.sync_with(self.thread.asks.first(), focus);
    }

    /// The flows' waits (gate, loop max), 1 per agent, as the last open asks: the agents' asks
    /// first, the question picker answers them. True: the asks changed.
    fn place_flow_ask(&mut self) -> bool {
        let want: Vec<stream::Ask> = self.flowbars.iter().filter_map(crate::flowbar::FlowBar::wait_ask).collect();
        let asks = &mut self.thread.asks;
        let n = asks.len();
        let have: Vec<usize> = (0..n).filter(|&i| asks[i].tool == crate::flowbar::WAIT_TOOL).collect();
        let at_end = have.iter().enumerate().all(|(k, &i)| i == n - have.len() + k);
        if at_end && have.len() == want.len() && have.iter().zip(&want).all(|(&i, w)| asks[i].id == w.id) {
            return false;
        }

        asks.retain(|a| a.tool != crate::flowbar::WAIT_TOOL);
        asks.extend(want);
        true
    }

    /// A flow row per member, in the members' order (a new member: its row, a gone one: 0 row).
    fn sync_flowbars(&mut self) {
        let mut bars = std::mem::take(&mut self.flowbars);
        for m in &self.members {
            match bars.iter().position(|b| &b.agent == m) {
                Some(i) => self.flowbars.push(bars.remove(i)),
                None => {
                    let mut b = crate::flowbar::FlowBar::new(m);
                    b.tick(&self.id, true);
                    self.flowbars.push(b);
                }
            }
        }
    }

    /// A mouse event on the flow rows: a click that opens a box closes the others. True: used.
    fn flow_mouse(&mut self, m: MouseEvent) -> bool {
        let Some(i) = self.flowbars.iter_mut().position(|b| b.mouse(m)) else { return false };
        if self.flowbars[i].is_open() {
            for (j, b) in self.flowbars.iter_mut().enumerate() {
                if j != i {
                    b.close();
                }
            }
        }
        true
    }

    /// The flow box of AGENT open (PICK: on the flows to switch to); every other box closed.
    fn open_flow(&mut self, agent: &str, pick: bool) {
        if !self.flowbars.iter().any(|b| b.agent == agent) {
            self.sync_flowbars();
        }
        for b in &mut self.flowbars {
            if b.agent != agent {
                b.close();
                continue;
            }

            if pick { b.pick() } else { b.open() }
        }
        self.dirty = true;
    }

    /// `place_flow_ask`, then the picker on the 1st ask.
    fn sync_flow_ask(&mut self) {
        let first = self.thread.asks.first().map(|a| a.id.clone());
        if self.place_flow_ask() {
            if self.thread.asks.first().map(|a| a.id.clone()) != first {
                self.sync_ask();
            }
            self.dirty = true;
        }
    }

    /// The repo roots the thread works in (its work dir 1st), with their labels
    /// (`repo ⎇ branch`); 0 repo: the work dir.
    fn ide_dirs(&self) -> Vec<(PathBuf, String)> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        let all = store::load(&self.id).map(|t| t.all_checkouts()).unwrap_or_default();
        for c in all.iter().map(|c| PathBuf::from(&c.dir)).filter(|d| d.is_dir()).chain([self.cwd.clone()]) {
            let top = crate::workspot::top(&c);
            if !dirs.contains(&top) {
                dirs.push(top);
            }
        }
        dirs.into_iter()
            .map(|d| {
                let s = crate::workspot::spot(&d, None);
                let label = match &s.branch {
                    Some(b) => format!("{} ⎇ {b}", s.repo),
                    None => s.repo,
                };
                (d, label)
            })
            .collect()
    }

    /// ctrl+i / ⌘i: the IDE on the repo root; several repos: the picker above the repos line.
    fn ide_key(&mut self) {
        let dirs = self.ide_dirs();
        match dirs.len() {
            0 => self.open_ide(self.cwd.clone()),
            1 => self.open_ide(dirs[0].0.clone()),
            _ => self.ide_pick = Some((0, 0)),
        }
    }

    /// Keys of the IDE's repo picker: ↑↓ move, enter opens, esc closes.
    fn ide_pick_key(&mut self, k: KeyEvent) {
        let Some((cur, off)) = self.ide_pick else { return };
        let n = self.ide_dirs().len().max(1);
        match k.code {
            KeyCode::Esc => self.ide_pick = None,
            KeyCode::Up | KeyCode::Char('k') => self.ide_pick = Some(((cur + n - 1) % n, off)),
            KeyCode::Down | KeyCode::Char('j') => self.ide_pick = Some(((cur + 1) % n, off)),
            KeyCode::Enter | KeyCode::Char(' ') => self.ide_pick_open(cur),
            _ => {}
        }
    }

    /// Repo I of the picker: the IDE on it, the picker closed.
    fn ide_pick_open(&mut self, i: usize) {
        self.ide_pick = None;
        if let Some((d, _)) = self.ide_dirs().into_iter().nth(i) {
            self.open_ide(d);
        }
    }

    /// The IDE on DIR (a window with DIR open: that one).
    fn open_ide(&mut self, dir: PathBuf) {
        #[cfg(test)]
        {
            self.ide_opened.push(dir);
        }
        #[cfg(not(test))]
        self.open_editor_in(&dir, |k| crate::editor::args_dir(k, &dir), None, tilde(&dir));
    }

    /// The IDE's repo picker above the repos line (the row below the prompt), at its left.
    fn draw_ide_pick(&mut self, f: &mut Frame, bounds: Rect) {
        let Some((cur, off)) = self.ide_pick else {
            self.ide_rows.clear();
            return;
        };
        let dirs = self.ide_dirs();
        let rows: Vec<crate::dropdown::Row> = dirs.iter().map(|(d, l)| crate::dropdown::Row { text: l.clone(), detail: tilde(d), now: *d == crate::workspot::top(&self.cwd), marks: Vec::new() }).collect();
        let w = rows.iter().map(|r| r.text.chars().count() + r.detail.chars().count() + 8).max().unwrap_or(0).clamp(30, 90) as u16;
        let anchor = Rect { x: self.hint_rect.x + 2, width: w, ..self.hint_rect };
        let Some(r) = crate::dropdown::place(anchor, w, rows.len(), bounds, true) else { return };
        let (off, drawn) = crate::dropdown::list(f, r, "open in IDE", "enter: open · esc: close", &rows, cur.min(rows.len().saturating_sub(1)), off, "0 repos");
        self.ide_pick = Some((cur, off));
        self.ide_rows = drawn;
    }

    /// Start the IDE with ARGS (by its kind); the toast names WHAT, else why not; once: the trust hint.
    /// TARGET (file, line; None: the worktree): a window that has the worktree open gets it.
    fn open_editor(&mut self, args: impl Fn(crate::editor::Kind) -> Vec<std::ffi::OsString>, target: Option<(&str, u32)>, what: String) {
        let wt = self.cwd.clone();
        self.open_editor_in(&wt, args, target, what)
    }

    /// [`Self::open_editor`] for the checkout WT.
    fn open_editor_in(&mut self, wt: &Path, args: impl Fn(crate::editor::Kind) -> Vec<std::ffi::OsString>, target: Option<(&str, u32)>, what: String) {
        let Some(e) = crate::editor::find() else {
            self.toast("IDE: 0 found · setting editor (code, cursor, zed, idea …)");
            return;
        };
        // a terminal editor: full screen in this terminal (the view loop suspends codehead)
        if e.kind.in_terminal() {
            let mut argv: Vec<std::ffi::OsString> = vec![e.bin.clone().into()];
            argv.extend(args(e.kind));
            self.outside = Some(argv);
            return;
        }
        let (argv, reused) = match crate::editor::reuse_args(&e, wt, target) {
            Some(a) => (a, true),
            None => (args(e.kind), false),
        };
        let what = if reused { format!("{what} (open window)") } else { what };
        match crate::editor::spawn(&e, &argv) {
            Ok(()) => {
                let hint = crate::editor::trust_hint(wt);
                self.toast(hint.unwrap_or_else(|| format!("IDE: {what}")));
            }
            Err(err) => self.toast(format!("IDE: {err}")),
        }
    }

    /// ctrl+m: the keys to the prompt; every box, filter and dropdown that holds them lets go.
    fn focus_prompt(&mut self) {
        self.menu = None;
        self.notes_panel = None;
        self.ide_pick = None;
        self.agents_focus = None;
        self.find.focus = false;
        self.flowbars.iter_mut().for_each(crate::flowbar::FlowBar::close);
        self.ask.focus = false;
        self.prompt_focus = self.pane != Pane::Chat;
        self.dirty = true;
    }

    /// The agents box has the keys, AGENT highlighted (None: the one a message goes to now).
    fn focus_agents(&mut self, agent: Option<&str>) {
        if self.order.is_empty() {
            self.toast("0 agents on the thread · @name in the prompt brings one in");
            return;
        }

        let want = agent.map(String::from).or_else(|| self.default_to().into_iter().next());
        let at = want.and_then(|w| self.order.iter().position(|a| *a == w)).unwrap_or(0);
        self.agents_focus = Some(AgentsFocus { at, ..Default::default() });
        self.find.focus = false;
        self.flowbars.iter_mut().for_each(crate::flowbar::FlowBar::close);
        // the box shows in the chat (and next to a wide changes view)
        if self.pane != Pane::Chat && !self.wide_split {
            self.set_pane(Pane::Chat);
        }
        self.dirty = true;
    }

    /// Agents of the roster that can replace AGENT: 0 on the thread.
    fn replace_candidates(&self) -> Vec<crate::agents::Agent> {
        self.roster.iter().filter(|a| !self.members.contains(&a.name)).cloned().collect()
    }

    /// A key while the agents box has the keys.
    fn agents_key(&mut self, k: KeyEvent) {
        let Some(mut fo) = self.agents_focus.take() else { return };
        let n = self.order.len();
        if n == 0 {
            return;
        }

        fo.at = fo.at.min(n - 1);
        let agent = self.order[fo.at].clone();
        // the replace picker: the agents to take over
        if let Some((cur, off)) = fo.replace {
            let cands = self.replace_candidates();
            match k.code {
                KeyCode::Esc | KeyCode::Left => fo.replace = None,
                KeyCode::Up | KeyCode::Char('k') => fo.replace = Some((cur.saturating_sub(1), off)),
                KeyCode::Down | KeyCode::Char('j') => fo.replace = Some(((cur + 1).min(cands.len().saturating_sub(1)), off)),
                KeyCode::Enter => {
                    if let Some(new) = cands.get(cur) {
                        self.replace_agent(new.name.clone(), agent);
                        return;
                    }
                }
                _ => {}
            }
            self.agents_focus = Some(fo);
            return;
        }

        if k.code != KeyCode::Enter {
            fo.armed = false;
        }
        match k.code {
            // esc: back to the prompt
            KeyCode::Esc => return,
            KeyCode::Left | KeyCode::Char('h') => {
                fo.at = (fo.at + n - 1) % n;
                fo.cur = 0;
            }
            KeyCode::Right | KeyCode::Char('l') => {
                fo.at = (fo.at + 1) % n;
                fo.cur = 0;
            }
            KeyCode::Up | KeyCode::Char('k') => fo.cur = fo.cur.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => fo.cur = (fo.cur + 1).min(AGENT_MENU.len() - 1),
            KeyCode::Char(c @ '1'..='3') => {
                fo.cur = usize::from(c as u8 - b'1');
                self.agents_focus = Some(fo);
                return self.agents_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            }
            KeyCode::Enter => match fo.cur {
                0 => {
                    self.open_flow(&agent, false);
                    return;
                }
                1 if self.members.len() < 2 => self.toast("the thread's last agent: replace it instead"),
                1 if fo.armed => {
                    self.host("leave", json!({"op": "leave", "agent": agent}));
                    self.toast(format!("{agent} leaves the thread"));
                    return;
                }
                1 => fo.armed = true,
                _ if self.replace_candidates().is_empty() => self.toast("0 other agents · menu → new agent"),
                _ => fo.replace = Some((0, 0)),
            },
            _ => {}
        }
        self.agents_focus = Some(fo);
    }

    /// NEW takes over from OLD: OLD leaves, NEW gets the handoff (and OLD's flow). OLD mid-turn:
    /// after its turn.
    fn replace_agent(&mut self, new: String, old: String) {
        let text = format!("@{new}");
        if self.thread.busy.contains(&old) {
            self.toast(format!("{new} replaces {old} after {old}'s turn · esc: interrupt {old}"));
            self.queued = Some((new, old, text));
        } else {
            self.run_replace(new, old, text);
        }
    }

    /// The agents box's menu above the highlighted agent (or its replace picker).
    fn draw_agents_menu(&mut self, f: &mut Frame, bounds: Rect) {
        let Some(fo) = self.agents_focus.clone() else { return };
        let Some(agent) = self.order.get(fo.at).cloned() else { return };
        // the agent's card (0 drawn: a narrow or short screen): else over the prompt
        let slot = self.slots.borrow().iter().find(|(a, _)| *a == agent).map(|(_, r)| *r);
        let anchor = slot.unwrap_or(Rect { y: self.input_rect.y.saturating_sub(1), height: 1, width: 1, ..self.input_rect });
        if let Some((cur, off)) = fo.replace {
            let rows: Vec<crate::dropdown::Row> = self
                .replace_candidates()
                .iter()
                .map(|a| crate::dropdown::Row { text: a.name.clone(), detail: crate::cards::tech(a), now: false, marks: Vec::new() })
                .collect();
            let Some(r) = crate::dropdown::place(anchor, 44, rows.len(), bounds, true) else { return };
            let (off, _) = crate::dropdown::list(f, r, &format!("replace @{agent} with"), "↑↓ · enter · esc: back", &rows, cur, off, "0 other agents");
            if let Some(fo) = self.agents_focus.as_mut() {
                fo.replace = Some((cur, off));
            }
            return;
        }

        let flow = self.flowbars.iter().find(|b| b.agent == agent).and_then(|b| b.state.as_ref()).map(|st| if st.finished { format!("{} · finished", st.title) } else { format!("{} · step {}", st.title, st.node + 1) });
        let details = [
            flow.unwrap_or_else(|| "0 flow: pick one".into()),
            if fo.armed { format!("enter again: remove @{agent}") } else { "leaves the thread".into() },
            "a new agent, the flow goes along".into(),
        ];
        let rows: Vec<crate::dropdown::Row> = AGENT_MENU.iter().zip(details).map(|(t, d)| crate::dropdown::Row { text: (*t).into(), detail: d, now: false, marks: Vec::new() }).collect();
        let Some(r) = crate::dropdown::place(anchor, 50, rows.len(), bounds, true) else { return };
        crate::dropdown::list(f, r, &format!("@{agent}"), "←→ agent · ↑↓ · enter · esc", &rows, fo.cur, 0, "");
    }

    /// Search match I (item index): its tool run open, the stream scrolls to it at the next draw.
    fn find_show(&mut self, i: usize) {
        if let Some(stream::Item::Run { key, .. }) = self.thread.items.get(i) {
            self.open.insert(*key);
        }

        self.find_to = Some(i);
        self.dirty = true;
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
            Item { label: "IDE on the repo root (several repos: pick 1)".into(), key: "ctrl+i · ⌘i".into(), id: 2, section: "thread" },
            Item { label: "interrupt the agents".into(), key: "esc".into(), id: 12, section: "thread" },
            Item { label: "yazi on the worktree (its pick: into the prompt)".into(), key: "files view: y".into(), id: 14, section: "thread" },
            Item { label: "views: chat · changes · merge · files".into(), key: "F1-F4".into(), id: 15, section: "thread" },
            Item { label: "agents box: flow, remove, replace".into(), key: "ctrl+q".into(), id: 17, section: "agents" },
        ];
        // per agent of the thread: its flow, remove it, replace it (the agents box's menu)
        let mut items = items;
        self.menu_agents = self.order.clone();
        for (i, a) in self.order.iter().enumerate() {
            let flow = self.flowbars.iter().find(|b| b.agent == *a).and_then(|b| b.state.as_ref()).map_or_else(|| "0 flow".to_string(), |st| format!("{} · step {}", st.title, st.node + 1));
            for (k, label) in [format!("@{a}: flow ({flow})"), format!("@{a}: remove"), format!("@{a}: replace with …")].into_iter().enumerate() {
                items.push(Item { label, key: String::new(), id: AGENT_ITEMS + i * 3 + k, section: "agents" });
            }
        }
        items.extend([
            Item { label: "search the chat".into(), key: "ctrl+f".into(), id: 16, section: "chat" },
            Item { label: "tool calls: open / close all".into(), key: "ctrl+o".into(), id: 1, section: "chat" },
            Item { label: "thoughts: open / close all".into(), key: "ctrl+t".into(), id: 13, section: "chat" },
            Item { label: "scroll the stream".into(), key: "wheel · pgup".into(), id: 10, section: "chat" },
            Item { label: "select + copy · double click: word · click code: copy".into(), key: "drag".into(), id: 9, section: "chat" },
            Item { label: "send · new line".into(), key: "⌘/ctrl+⏎ · ⏎".into(), id: 11, section: "prompt" },
            Item { label: "agents: mention · →: replace an agent".into(), key: "@".into(), id: 4, section: "prompt" },
            Item { label: "commands: files, notes, threads, skills, branch".into(), key: crate::config::command_key().to_string(), id: 5, section: "prompt" },
            Item { label: "paste a clipboard image".into(), key: "ctrl+v".into(), id: 7, section: "prompt" },
        ]);
        // cards: read in the background by the menu (0 wait on open)
        let cards = Vec::new();
        self.menu = Some(crate::menu::Menu::new(items, Some(self.id.clone()), self.roster.clone(), cards));
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
            Action::Pr(e) => {
                self.menu = None;
                match e.thread {
                    Some(t) if t.id == self.id => self.show_pr(&e.item.pr),
                    Some(t) => {
                        open_pr(&t.id, &e.item.pr);
                        self.exit = Exit::Open { id: t.id.clone(), dir: t.dir.clone(), title: t.title.clone(), note: String::new() };
                        return true;
                    }
                    None => {
                        self.exit = Exit::Home(crate::menu::Home::Merge(Some(e.item.pr.clone())));
                        return true;
                    }
                }
            }
            Action::Open(c) => {
                self.menu = None;
                if c.id == self.id {
                    return false;
                }
                self.exit = Exit::Open { id: c.id.clone(), dir: c.dir.clone().unwrap_or_else(|| self.cwd.clone()), title: c.title, note: String::new() };
                return true;
            }
            Action::Jump(c, ts) => {
                self.menu = None;
                if c.id == self.id {
                    self.jump = Some(ts);
                    self.dirty = true;
                    return false;
                }
                crate::search::jump_to(&c.id, ts);
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
            Action::Command(14) => self.yazi = Some(self.cwd.clone()),
            Action::Command(16) => {
                self.menu = None;
                return self.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
            }
            Action::Command(17) => {
                self.menu = None;
                self.focus_agents(None);
                return false;
            }
            // an agent's row: its flow box, remove (a 2nd enter in the agents box), replace
            Action::Command(id) if id >= AGENT_ITEMS => {
                self.menu = None;
                let (i, k) = ((id - AGENT_ITEMS) / 3, (id - AGENT_ITEMS) % 3);
                let Some(agent) = self.menu_agents.get(i).cloned().filter(|a| self.members.contains(a)) else { return false };
                match k {
                    0 => self.open_flow(&agent, false),
                    _ => {
                        self.focus_agents(Some(&agent));
                        if let Some(fo) = self.agents_focus.as_mut() {
                            fo.cur = k;
                            if k == 1 {
                                fo.armed = true;
                            } else {
                                fo.replace = Some((0, 0));
                            }
                        }
                    }
                }
                return false;
            }
            Action::Command(_) => {}
        }
        self.menu = None;
        false
    }

    /// A key / click of the notifications dropdown: a jump goes to the note's event.
    fn notes_pick(&mut self, p: crate::notify::Pick) -> bool {
        use crate::notify::Pick;
        self.dirty = true;
        match p {
            Pick::Stay => false,
            Pick::Close => {
                self.notes_panel = None;
                false
            }
            Pick::Jump(n) => {
                self.notes_panel = None;
                if !n.comment.is_empty() {
                    crate::changes::threads::jump_to(&n.thread, &n.comment);
                }
                self.menu_action(crate::menu::Action::Jump(n.card(), n.ts))
            }
        }
    }

    fn mouse(&mut self, m: MouseEvent) {
        if let Some(p) = self.notes_panel.as_mut() {
            if matches!(m.kind, MouseEventKind::Down(_)) {
                let pick = p.click(ratatui::layout::Position { x: m.column, y: m.row });
                if self.notes_pick(pick) {
                    self.mouse_exit = true;
                }
            }
            return;
        }
        if let Some(menu) = self.menu.as_mut() {
            let a = menu.mouse(m);
            if self.menu_action(a) {
                self.menu = None;
                // a thread picked: the view ends
                self.mouse_exit = true;
            }
            return;
        }
        // the IDE's repo picker: a click on a repo opens it, next to it closes the picker
        if self.ide_pick.is_some() {
            if matches!(m.kind, MouseEventKind::Down(_)) {
                let at = Position { x: m.column, y: m.row };
                match self.ide_rows.iter().find(|(r, _)| r.contains(at)).map(|x| x.1) {
                    Some(i) => self.ide_pick_open(i),
                    None => self.ide_pick = None,
                }
                self.dirty = true;
            }
            return;
        }
        if self.flow_mouse(m)
            || self.subthreads.mouse(m)
            || self.subagents.mouse(m)
            || self.monitors.mouse(m)
            || self.mention.mouse(m, &mut self.input, &self.roster)
            || self.command.mouse(m, &mut self.input)
            || self.shortcuts.mouse(m, &mut self.input)
            || self.paths.mouse(m, &mut self.input)
            || (self.comp_on() && {
                let cmds = self.shell_cmds();
                self.comp.mouse(m, &mut self.input, &self.shell_cwd, &cmds)
            })
        {
            return;
        }
        let at = Position { x: m.column, y: m.row };
        self.hover = Some(at);
        let click = matches!(m.kind, MouseEventKind::Down(MouseButton::Left));
        // ctrl+click on a URL anywhere on screen: the browser
        if click
            && m.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER)
            && let Some(url) = crate::urls::at(&self.last_frame, at)
        {
            crate::merge::open_url(&url);
            self.toast(format!("open {url}"));
            return;
        }
        // the worktree's URL (portless): the browser
        if click && let Some((r, url)) = &self.url_rect
            && r.contains(at)
        {
            let url = url.clone();
            open_url(&url);
            self.toast(format!("open {url}"));
            return;
        }
        // the flow line of an agent card: its flow box opens (open: closes)
        let flow_at = self.flow_slots.borrow().iter().find(|(_, r)| r.contains(at)).map(|(a, _)| a.clone());
        if click && let Some(agent) = flow_at {
            match self.flowbars.iter_mut().find(|b| b.agent == agent && b.is_open()) {
                Some(b) => b.close(),
                None => self.open_flow(&agent, false),
            }
            self.dirty = true;
            return;
        }
        // the view bar
        if click && let Some(&(_, p)) = self.bar_rects.iter().find(|(r, _)| r.contains(at)) {
            self.set_pane(p);
            return;
        }
        // a repo tab of the changes view
        if click && self.pane == Pane::Changes && self.checkout_tabs.click(at) {
            self.pick_checkout(self.checkout_tabs.i);
            return;
        }
        let in_stream = self.stream_rect.contains(at);
        if self.pane != Pane::Chat {
            if self.view_rect.contains(at) {
                self.prompt_focus = self.prompt_focus && !click;
                if self.pane == Pane::Files {
                    self.files.mouse(m);
                } else if self.pane == Pane::Merge {
                    let act = self.merge.mouse(m);
                    self.merge_act(act);
                } else {
                    let act = self.changes.mouse(m);
                    self.changes_act(act);
                }
                return;
            }
            // the chat's title row in the changes view: folds / opens it
            if click && self.mini_rect.contains(at) {
                self.mini_chat = !self.mini_chat;
                self.dirty = true;
                return;
            }
            // the chat of the split: as the chat view below (scroll, links, selection + copy)
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
                    Some(Hit::PickUp(i)) => {
                        self.pick_up(i);
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
        let color = self.color(name);
        let (sprite, effort) = self.profiles.get(name).cloned().unwrap_or_else(|| (crate::sprites::default_for(name).desynced(name), String::new()));
        let model = self.thread.models.get(name).filter(|m| !m.is_empty()).map(|m| crate::agents::model_name(m)).or_else(|| a.map(crate::agents::model_of)).unwrap_or_default();
        let harness = a.map(|a| title_case(&a.harness)).unwrap_or_default();
        let tech = [harness, model].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
        let frame = WAVE[tick % WAVE.len()];
        let status = if self.thread.asks.iter().any(|q| q.agent == name) {
            vec![Span::styled("  ● ", theme::fg(theme::pal().attention())), Span::styled("waits on you", theme::muted())]
        } else if self.thread.busy.contains(name) {
            vec![Span::styled(format!("  {frame} "), Style::default().fg(color)), Span::styled("working…", theme::muted())]
        } else if self.thread.working(name) {
            vec![Span::styled(format!("  {frame} "), Style::default().fg(color)), Span::styled("subagents…", theme::muted())]
        } else if self.monitors.watches.iter().any(|w| w.agent == name) {
            vec![Span::styled(format!("  {frame} "), Style::default().fg(color)), Span::styled("monitoring…", theme::muted())]
        } else {
            Vec::new()
        };
        // the status right of the effort: `  ░▒▓ monitoring…`, 17 columns
        let status_w = if status.is_empty() { 0 } else { 17 };
        let mut text_w = [name.chars().count(), tech.chars().count(), effort.chars().count() + status_w].into_iter().max().unwrap_or(0);
        // the agent's flow: its step below the effort (the rows shift up), cut to the card's text width (34 columns at least)
        let flow = self.flowbars.iter().find(|b| b.agent == name).and_then(crate::flowbar::FlowBar::card_line).map(|(s, st)| {
            let s = crate::nav::fit(&s, text_w.max(34));
            text_w = text_w.max(s.chars().count());
            vec![Span::styled(format!("  {s}"), st)]
        });
        let mut last = vec![Span::styled(format!("  {effort}"), theme::muted())];
        last.extend(status);
        let mut side = vec![
            vec![],
            if self.agents_focus.as_ref().is_some_and(|fo| self.order.get(fo.at).is_some_and(|a| a == name)) {
                vec![Span::raw(" "), Span::styled(format!(" {name} "), theme::pill().add_modifier(Modifier::BOLD))]
            } else {
                vec![Span::styled(format!("  {name}"), Style::default().fg(color).add_modifier(Modifier::BOLD))]
            },
            vec![Span::styled(format!("  {tech}"), theme::muted())],
            last,
        ];
        if let Some(fl) = flow {
            side.remove(0);
            side.push(fl);
        }
        // a spawn: 0 text until the snap, then the text decodes in (width kept: 0 layout jump)
        if let Some(sp) = self.spawns.get(name) {
            let p = sp.reveal();
            let (noise, dim) = (Style::default().fg(color), theme::line());
            side = side.into_iter().map(|l| if p <= 0.0 { Vec::new() } else { crate::avatar::decode(l, p, noise, dim) }).collect();
        }
        (sprite, color, side, (crate::sprites::W + 2 + text_w + 4) as u16)
    }

    /// Header (hell's): 1 frame over the agents box (the cards) and the tokens box (COST:
    /// the pane's area, 0 width: 0 tokens box), straight lines between them.
    fn draw_header(&self, f: &mut Frame, area: Rect, cost: Rect) {
        if area.height == 0 {
            // 0 cards drawn: 0 slots (the agents box's menu then sits over the prompt)
            self.slots.borrow_mut().clear();
            self.flow_slots.borrow_mut().clear();
            return;
        }
        let cards: Vec<Card> = self.order.iter().map(|n| self.card(n)).collect();
        let token_left = cost.x.saturating_sub(2);
        let fit = if cost.width > 0 { token_left + 1 } else { area.right() }.saturating_sub(area.x);
        let outer = if cost.width == 0 { area } else { Rect { width: cards_w(&cards).min(fit), ..area } };
        // the cards sit at the bottom, right above the prompt: a taller tokens box leaves its
        // spare rows above them, not between them and the prompt
        let card_h = crate::sprites::H as u16;
        let y = (area.y + 1).max(area.bottom().saturating_sub(1 + card_h));
        let rows = Rect { y, height: area.bottom().saturating_sub(1).saturating_sub(y), ..area };
        let mut x = area.x + 2;
        let mut slots = Vec::new();
        let mut flow_slots = Vec::new();
        for ((sprite, color, side, w), name) in cards.into_iter().zip(&self.order) {
            if x + w - 1 > outer.right() {
                break;
            }
            let st = self.avatar_status(name);
            f.render_widget(Paragraph::new(crate::sprites::lines_as(&sprite, color, st, side)), Rect { x, width: w - 4, ..rows });
            slots.push((name.clone(), Rect { x, y: rows.y, width: crate::sprites::W as u16, height: crate::sprites::H as u16 }));
            if self.flowbars.iter().any(|b| b.agent == *name && b.state.is_some()) {
                let sw = crate::sprites::W as u16;
                flow_slots.push((name.clone(), Rect { x: x + sw, y: rows.y + 3, width: (w - 4).saturating_sub(sw), height: 1 }.intersection(rows)));
            }
            x += w;
        }
        *self.slots.borrow_mut() = slots;
        *self.flow_slots.borrow_mut() = flow_slots;
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
            // the search bar: matches in new items (the shown match stays)
            self.find.refresh(&self.thread);
            // live text: messages at the end for this draw only (the log brings the final ones)
            let logged = self.thread.items.len();
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
            for (agent, text) in &self.live {
                self.thread.items.push(stream::Item::Text { agent: agent.clone(), text: text.clone(), ts: now, thoughts: Vec::new(), key: 0 });
            }
            self.drawn = stream::draw(&self.thread, self.look, st.width, &self.colors_now(), &self.harness_of, &fold, &self.ask);
            self.thread.items.truncate(logged);
            self.dirty = false;
            if let Some(a) = anchor {
                self.back = back_for(&self.drawn.starts, self.drawn.rows.len(), a, h);
            }
            // a search result: its message 2 rows below the top, marked
            if let Some(ts) = self.jump.take() {
                // exact (a search hit), else the 1st message at / after it (a notification's event)
                let items = &self.thread.items;
                let at = items.iter().position(|it| item_ts(it) == Some(ts)).or_else(|| items.iter().position(|it| item_ts(it).is_some_and(|t| t >= ts))).or_else(|| items.len().checked_sub(1));
                match at {
                    Some(i) => {
                        self.back = back_for(&self.drawn.starts, self.drawn.rows.len(), (i, 0), h).saturating_add(2).min(self.drawn.rows.len().saturating_sub(h));
                        self.flash = Some(i);
                    }
                    None => self.toast("search: the message is gone from the stream"),
                }
            }

            // the search bar's match: a third of the stream below the top
            if let Some(i) = self.find_to.take().filter(|&i| i < self.drawn.starts.len()) {
                let total = self.drawn.rows.len();
                self.back = back_for(&self.drawn.starts, total, (i, 0), h).saturating_add(h / 3).min(total.saturating_sub(h));
            }
        }
        // seen: the newest message on screen (at the bottom: every event) for 1 s
        if self.menu.is_none() && self.notes_panel.is_none() {
            let ts = (self.back > 0).then(|| {
                let bottom = self.drawn.rows.len().saturating_sub(self.back).min(self.drawn.rows.len());
                let last = self.drawn.starts.iter().take(self.thread.items.len()).rposition(|&s| s < bottom);
                last.and_then(|i| self.thread.items[..=i].iter().rev().find_map(item_ts)).unwrap_or(0)
            });
            if ts != Some(0) {
                self.watch.on_screen(&self.id, ts);
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
                Some(e) => Line::styled(format!("send failed: {e}  "), Style::default().fg(theme::pal().error())),
                None if at.elapsed() > Duration::from_secs(90) => Line::styled("unconfirmed · check the host log before a retry  ", theme::muted()),
                None => Line::styled(format!("sending… {}s  ", at.elapsed().as_secs()), theme::muted()),
            };
            extra.push(if right { status.right_aligned() } else { status });
        }
        if let Some((new, old, _)) = &self.queued {
            extra.push(Line::raw(""));
            extra.push(Line::styled(format!("  {new} replaces {old} after {old}'s turn · esc: interrupt {old}"), theme::fg(theme::pal().progress())));
        }
        let h = usize::from(st.height);
        let total = self.drawn.rows.len();
        self.back = self.back.min(total.saturating_sub(h));
        let end = total - self.back;
        let start = end.saturating_sub(h);
        // the search bar open: its shown match has the marker
        if self.find.open {
            self.flash = self.find.current();
        }
        let flash = self.flash.and_then(|i| Some((*self.drawn.starts.get(i)?, self.drawn.starts.get(i + 1).copied().unwrap_or(total))));
        let mut lines: Vec<Line> = self.drawn.rows[start..end]
            .iter()
            .enumerate()
            .map(|(n, r)| match flash {
                // the found message: a marker in the left column
                Some((a, b)) if (a..b).contains(&(start + n)) => {
                    let mut l = r.line.clone();
                    l.spans.insert(0, Span::styled("▌", theme::accent()));
                    if let Some(s) = l.spans.get_mut(1)
                        && let Some(rest) = s.content.strip_prefix(|c: char| c == ' ' || c == '│' || c == '┌' || c == '└').map(String::from)
                    {
                        s.content = rest.into();
                    }
                    l
                }
                _ => r.line.clone(),
            })
            .collect();
        // the search bar's words: marked in the rows of matching items
        let words = self.find.words();
        if !words.is_empty() {
            for (n, l) in lines.iter_mut().enumerate() {
                let item = self.drawn.starts.partition_point(|&s| s <= start + n).checked_sub(1);
                if item.is_some_and(|i| self.find.is_hit(i)) {
                    *l = crate::chatsearch::mark(l, &words);
                }
            }
        }

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
                    cell.set_style(theme::pill());
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
        // a reply note's jump: the changes view on its comment, the thread unfolded
        if let Some(id) = crate::changes::threads::take_jump(&self.id) {
            self.set_pane(Pane::Changes);
            self.changes.show_comment(&id);
        }
        // members set elsewhere (a reload, tests): the box order follows
        if self.order.len() != self.members.len() || !self.members.iter().all(|m| self.order.contains(m)) {
            self.sync_order();
        }
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
        self.follow_width(area);
        // wide changes view: the changes left at full height; the chat column right of it (the
        // header, the stream, the prompt) is laid out below as the whole area
        let wide = self.pane == Pane::Changes && self.split && area.width >= SPLIT_MIN;
        self.wide_split = wide;
        let (area, changes_left) = if wide {
            let lw = area.width * 3 / 5;
            let buf = f.buffer_mut();
            for y in area.y..area.bottom() {
                buf[(area.x + lw, y)].set_symbol("│").set_style(theme::line());
            }
            (Rect { x: area.x + lw + 1, width: area.width - lw - 1, ..area }, Some(Rect { width: lw, ..area }))
        } else {
            (area, None)
        };
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
        let head_h = if area.height >= MIN_HEADER_SCREEN && !self.members.is_empty() && (self.pane == Pane::Chat || wide) { head_rows + 1 } else { 0 };
        // the bubble's top border: the header's bottom line (shared), else a row of its own. The
        // search bar (ctrl+f): 1 row right above the bubble, the header keeps its own bottom line
        let bubble_top = in_top.saturating_sub(1);
        let find_h = u16::from(self.find.open && (self.pane == Pane::Chat || wide));
        let find_rect = Rect { x: area.x + 1, y: bubble_top.saturating_sub(find_h), width: area.width.saturating_sub(2), height: find_h };
        let header = Rect { y: (bubble_top + 1).saturating_sub(head_h + 2 * find_h), height: head_h, ..area };
        let title = Rect { y: if head_h > 0 { header.y } else { find_rect.y }.saturating_sub(1), height: 1, ..area };
        // the flow rows (an open box) right above the title row: 1 per agent with a flow
        let flow_rows: Vec<u16> = self.flowbars.iter().map(|b| b.rows(area.height / 2)).collect();
        let flow_h = flow_rows.iter().sum::<u16>().min(area.height / 2);
        let flow = Rect { y: title.y.saturating_sub(flow_h), height: flow_h, ..area };
        let mon_h = self.monitors.rows(area.width, area.height / 3);
        let mon = Rect { y: flow.y.saturating_sub(mon_h), height: mon_h, ..area };
        // the subagents row (its open box) above the monitors row
        let sub_h = self.subagents.rows(&self.thread.subs, area.width, area.height / 3);
        let sub = Rect { y: mon.y.saturating_sub(sub_h), height: sub_h, ..area };
        // the sub-threads row (its open box) above the subagents row
        let kid_h = self.subthreads.rows(area.width, area.height / 3);
        let kid = Rect { y: sub.y.saturating_sub(kid_h), height: kid_h, ..area };
        let st = Rect { x: area.x, y: area.y, width: area.width, height: kid.y.saturating_sub(area.y) };
        self.stream_rect = st;
        self.view_rect = st;

        if self.pane == Pane::Files {
            self.files.draw(f, st);
        } else if self.pane == Pane::Merge {
            self.merge_sources(false);
            self.merge.draw(f, st, !self.prompt_focus);
            if let Some(n) = self.merge.note.take() {
                self.toast(n);
            }
        } else if self.pane == Pane::Chat {
            self.draw_stream(f, st);
        } else {
            // wide: the stream in the chat column, the changes left (alt+\ toggles)
            let st = if let Some(left) = changes_left {
                self.draw_stream(f, st);
                left
            } else {
                st
            };
            // narrow (0 split): the chat above the prompt, under the changes (its title row folds it)
            self.mini_rect = Rect::default();
            let st = if self.stream_rect == st && st.height >= 16 {
                let rows = if self.mini_chat { (st.height / 3).clamp(4, 14) } else { 0 };
                let title = Rect { y: st.bottom() - rows - 1, height: 1, ..st };
                let chat = Rect { y: title.bottom(), height: rows, ..st };
                let label = if self.mini_chat { "▾ chat · t: fold " } else { "▸ chat · t: open " };
                let busy = if self.thread.busy.is_empty() { String::new() } else { format!("· {} at work ", self.thread.busy.len()) };
                let text = format!("─ {label}{busy}");
                let fill = usize::from(title.width).saturating_sub(unicode_width::UnicodeWidthStr::width(text.as_str()));
                f.render_widget(Paragraph::new(Line::from(vec![Span::styled(text, theme::muted()), Span::styled("─".repeat(fill), theme::line())])), title);
                self.mini_rect = title;
                self.stream_rect = chat;
                if rows > 0 {
                    self.draw_stream(f, chat);
                }
                Rect { height: title.y - st.y, ..st }
            } else {
                st
            };
            // the repo tabs on top, a gap below them
            if self.checkouts.is_empty() {
                self.load_checkouts();
            }
            let st = if st.height > 6 && !self.checkout_labels.is_empty() {
                self.checkout_tabs.i = self.checkout;
                let labels = self.checkout_labels.clone();
                self.checkout_tabs.draw(f, Rect { height: 1, ..st }, "repo", &labels, !self.prompt_focus);
                Rect { y: st.y + 2, height: st.height - 2, ..st }
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
        }
        if mon_h > 0 {
            self.monitors.render(f, mon, self.tick());
        }
        if flow_h > 0 {
            let mut y = flow.y;
            for (b, h) in self.flowbars.iter_mut().zip(&flow_rows) {
                let h = (*h).min(flow.bottom().saturating_sub(y));
                b.render(f, Rect { y, height: h, ..flow }, true);
                y += h;
            }
        }
        if sub_h > 0 {
            let tick = self.tick();
            self.subagents.render(f, sub, tick, &self.thread.subs);
        }
        if kid_h > 0 {
            let tick = self.tick();
            self.subthreads.render(f, kid, tick);
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
                Pane::Changes => " changes ".to_string(),
                Pane::Merge => {
                    let n = self.merge.count();
                    if n == 0 { " merge ".to_string() } else { format!(" merge {n} ") }
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
        // notifications of the other threads; this thread's while scrolled back: ↓ N new
        let mut badge = crate::notify::badge(&crate::notify::unseen(Some(&self.id)));
        let here = crate::notify::unseen_of(&self.id).len();
        if here > 0 && self.back > 0 {
            badge.insert(0, Span::styled(format!("↓ {here} new  "), theme::bold().fg(theme::pal().info())));
        }
        let badge_w = usize::from(crate::notify::width(&badge));
        let badge_w = if top.width as usize > used + link_w + badge_w + 24 { badge_w } else { 0 };
        let room = usize::from(top.width.saturating_sub(4)).saturating_sub(used + link_w + badge_w + 1);
        spans.push(Span::styled(format!("{} ", crate::nav::fit(&self.title, room)), theme::bold()));
        f.render_widget(Paragraph::new(Line::from(spans)), Rect { x: top.x + 2, width: top.width.saturating_sub(2), ..top });
        if let Some((text, url)) = link
            && top.width as usize > link_w + used + 4
        {
            let r = Rect { x: top.right() - link_w as u16, width: link_w as u16, ..top };
            f.render_widget(Paragraph::new(Span::styled(text, theme::fg(theme::pal().info()).add_modifier(Modifier::UNDERLINED))), r);
            self.url_rect = Some((r, url));
        }
        if badge_w > 0 {
            let lw = if top.width as usize > link_w + used + 4 { link_w } else { 0 };
            let r = Rect { x: top.right().saturating_sub((lw + badge_w + 1) as u16), width: badge_w as u16, ..top };
            f.render_widget(Paragraph::new(Line::from(badge)), r);
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
            let cards: Vec<Card> = self.order.iter().map(|n| self.card(n)).collect();
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
                let next = crate::cost::Next { to: self.recipients(), chars: self.input.text().chars().count() };
                crate::cost::draw_in(f, cost, s, self.cost_at.elapsed(), &next);
            } else {
                // a new thread: the last box until this one's numbers come (then it morphs)
                crate::cost::draw_last(f, cost);
            }
        }

        // the input bubble: its top border the header's bottom line (├ ┴ ┤)
        let bubble = Rect { y: bubble_top, height: bubble_bottom + 1 - bubble_top, ..area };
        // the bubble's border: the accent while the prompt takes the keys (the focused pane),
        // the shell color in shell mode
        let keys = (self.pane == Pane::Chat || self.prompt_focus) && self.agents_focus.is_none() && self.menu.is_none();
        let edge = if self.shell_mode() { theme::fg(theme::shell()) } else if keys { theme::accent() } else { theme::fg(theme::pal().surface1) };
        {
            let buf = f.buffer_mut();
            let (left, right) = (bubble.x, bubble.right().saturating_sub(1));
            for x in left..=right {
                let top = buf[(x, bubble_top)].symbol().to_string();
                let sym = match top.as_str() {
                    _ if x == left => "┌",
                    _ if x == right => "┐",
                    _ => "─",
                };
                buf[(x, bubble_top)].reset();
                buf[(x, bubble_top)].set_symbol(sym).set_style(edge);
                buf[(x, bubble_bottom)].reset();
                buf[(x, bubble_bottom)].set_symbol(if x == left { "└" } else if x == right { "┘" } else { "─" }).set_style(edge);
            }
            for y in in_top..bubble_bottom {
                buf[(left, y)].set_symbol("│").set_style(edge);
                buf[(right, y)].set_symbol("│").set_style(edge);
            }
        }
        // asks: what the keys do now, in the bubble's bottom border
        if !self.thread.asks.is_empty() && self.input.is_empty() && self.ask.focus {
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
        self.input.render(f, self.input_rect, !menu_open && !ask_keys && prompt_keys && !self.find.focus);
        if find_h > 0 {
            self.find.render(f, find_rect);
        }
        let comp_cmds = if self.comp_on() { Some(self.shell_cmds()) } else { None };
        if let Some(cmds) = &comp_cmds {
            self.comp.render_suggestion(f, &self.input, cmds, self.input_rect);
        }

        // hint row: who gets the message, the send keys
        let mut to = vec![Span::styled("  → ", theme::muted())];
        for (i, name) in self.recipients().into_iter().enumerate() {
            if i > 0 {
                to.push(Span::styled(", ", theme::muted()));
            }
            to.push(Span::styled(name.clone(), Style::default().fg(self.color(&name))));
        }
        if self.members.is_empty() {
            to.clear();
        }
        let shell = self.shell_mode();
        if shell {
            let sh = Style::default().fg(theme::shell());
            to = vec![Span::styled("  ! shell mode", sh.add_modifier(Modifier::BOLD)), Span::styled(" · in ", theme::muted())];
            if self.cwd_edit.is_none() {
                to.push(Span::styled(tilde(&self.shell_cwd), theme::muted()));
            }
        } else {
            if self.shell_running > 0 {
                to.push(Span::styled(format!("  · ! {} running", self.shell_running), Style::default().fg(theme::shell())));
            }
            // where the thread works: repo ⎇ branch (worktree), per checkout
            if self.spots_at.is_none_or(|t| t.elapsed() > Duration::from_secs(3)) {
                self.spots = crate::workspot::of_thread(&self.id);
                self.spots_at = Some(Instant::now());
            }
            to.extend(crate::workspot::spans(&self.spots));
        }
        self.hint_rect = hint;
        let lead: u16 = to.iter().map(|s| s.content.chars().count() as u16).sum();
        let left_w = to_width(&hint, self.cwd_edit.is_some() && shell, self.hist.is_open(), shell, self.input.is_empty());
        f.render_widget(Paragraph::new(Line::from(to)), Rect { width: left_w, ..hint });
        // the dir input: right of `in`, half the row at most
        let mut cwd_rect = Rect::default();
        if let Some(ta) = self.cwd_edit.as_mut().filter(|_| shell) {
            cwd_rect = Rect { x: hint.x + lead, width: (hint.width / 2).saturating_sub(lead).max(10).min(hint.width.saturating_sub(lead)), ..hint };
            ta.render(f, cwd_rect, true);
        }
        let keys = hint_keys(self.cwd_edit.is_some() && shell, self.hist.is_open(), shell, self.input.is_empty());
        f.render_widget(Paragraph::new(Line::from(theme::hints(keys))).right_aligned(), Rect { x: hint.x + hint.width / 2, width: hint.width - hint.width / 2, ..hint });

        // footer: keys or toast right
        let right = match &self.toast {
            Some((m, at)) if at.elapsed() < Duration::from_secs(3) => Line::from(Span::styled(format!("{m} "), theme::fg(theme::pal().info()))),
            _ if self.agents_focus.is_some() => Line::from(theme::hints("←→ agent · ↑↓ · enter · 1-3 · esc / ctrl+m: prompt ")),
            _ if self.pane == Pane::Changes && !self.prompt_focus => Line::from(theme::hints(self.changes.hints())),
            _ if self.pane == Pane::Merge && !self.prompt_focus => Line::from(theme::hints(&self.merge.hints())),
            _ if self.pane == Pane::Files && !self.prompt_focus => Line::from(theme::hints("j k · ctrl+f filter · pgup pgdn: scroll · o e i: IDE · y: yazi · c: copy path · S S: share · ctrl+m: prompt ")),
            _ if self.pane != Pane::Chat => Line::from(theme::hints("tab / esc: back to the view · F1-F4: views ")),
            _ => Line::from(theme::hints(&format!("{}: menu · ctrl+q: agents · ctrl+f: search · F1-F4: views · esc: interrupt · esc esc: codehead ", crate::config::menu_key().label))),
        };
        // the mode the keys are in: a badge left, the keys right of it
        let badge = self.mode().map(|(label, risky)| theme::mode_badge(label, risky)).unwrap_or_default();
        let bw = (badge.iter().map(|s| s.content.chars().count()).sum::<usize>() as u16).min(foot.width);
        f.render_widget(Paragraph::new(Line::from(badge)), Rect { width: bw, ..foot });
        f.render_widget(Paragraph::new(right).right_aligned(), Rect { x: foot.x + bw, width: foot.width - bw, ..foot });

        // spawns over the header, under every dropdown and menu
        self.draw_spawns(f);

        // dropdowns over everything, then the menu
        if !menu_open {
            self.draw_agents_menu(f, area);
            self.mention.render(f, &self.input, &self.roster, area);
            self.hist.render(f, area, self.input_rect);
            self.command.render(f, &self.input, area, self.input_rect);
            self.shortcuts.render(f, &self.input, area, self.input_rect);
            self.paths.render(f, &self.input, area, self.input_rect);
            match &comp_cmds {
                Some(cmds) => self.comp.render(f, &self.input, &self.shell_cwd, cmds, area, self.input_rect),
                None => self.comp.hide(),
            }
            let open = self.shortcuts.is_open(&self.input);
            let keys = self.pane == Pane::Chat || self.prompt_focus;
            crate::shortcuts::render_popup(f, &self.input, self.hover, keys, area, open);
            match self.cwd_edit.as_ref() {
                Some(ta) if shell => self.cwd_paths.render(f, ta, area, cwd_rect),
                _ => self.cwd_paths.hide(),
            }
        }
        self.draw_ide_pick(f, area);
        if let Some(m) = self.menu.as_mut() {
            f.render_widget(Clear, Rect::default());
            // a command palette: below the view bar, centered on the screen's width
            m.render(f, f.area());
        }
        if let Some(p) = self.notes_panel.as_mut() {
            p.draw(f, f.area());
        }
        self.last_frame.clone_from(f.buffer_mut());
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
    // tokens box: column names, agents, all (2+), 2 plan windows, the next message
    (5 + n + u16::from(n > 1)).clamp(HEADER_H, MAX_HEADER_H)
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
/// trailing spaces off. None: a border row (`┌─ … ┐`, `└─ copy ─┘`).
fn content_cols(row: &str) -> Option<(usize, usize)> {
    let chars: Vec<char> = row.chars().collect();
    let t = row.trim_start();
    if t.starts_with('┌') || t.starts_with('└') {
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
    crate::all_keys(false);
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

/// The time of a stream item (a search hit names a message by its time).
fn item_ts(it: &stream::Item) -> Option<u64> {
    match it {
        stream::Item::Prompt { ts, .. } | stream::Item::Text { ts, .. } => Some(*ts),
        _ => None,
    }
}

/// The unsent prompt of thread ID (JSON: text, attachments).
fn prompt_path(id: &str) -> PathBuf {
    store::thread_dir(id).join("prompt.json")
}

/// TEXT as the prompt draft of thread ID: its next view opens with it.
pub fn save_prompt_draft(id: &str, text: &str) {
    let _ = crate::engine::write_atomic(&prompt_path(id), json!({"text": text, "attachments": []}).to_string().as_bytes());
}

pub fn run<B: Backend>(term: &mut Terminal<B>, id: &str, cwd: &Path, title: &str, note: Option<&str>, first: &str) -> Result<Exit, String> {
    run_with(term, id, cwd, title, note, first, None).map(|e| e.unwrap_or(Exit::Back))
}

/// EXIT of a sub-thread's view (MAIN: the thread it runs for): back (esc esc) goes to MAIN, not
/// to the start screen.
fn to_main(exit: Exit, main: Option<&str>) -> Exit {
    match (exit, main.and_then(store::load)) {
        (Exit::Back, Some(t)) => Exit::Open { id: t.id.clone(), dir: t.work_dir(), title: t.title.clone(), note: String::new() },
        (e, _) => e,
    }
}

/// What a hook of `run_with` sees each loop: the open pane, the menu open.
pub struct Peek {
    pub pane: Pane,
    pub menu: bool,
}

/// `run` with HOOK: called each loop (the setup tutorial's coach watches the view); true from
/// it: the view ends at once, Ok(None) (the tutorial lays its screen out again on a resize).
pub fn run_with<B: Backend>(term: &mut Terminal<B>, id: &str, cwd: &Path, title: &str, note: Option<&str>, _first: &str, mut hook: Option<&mut dyn FnMut(&Peek) -> bool>) -> Result<Option<Exit>, String> {
    let Some(me) = store::load(id) else {
        return Err(format!("no thread {id}"));
    };
    // a sub-thread: esc esc and its end go back to the thread it runs for
    let of = me.of;
    let (tx, rx): (Sender<Msg>, Receiver<Msg>) = mpsc::channel();
    let _sub = subscribe(id, tx.clone());
    crate::engine::trace("view new …");
    let opened = Instant::now();
    let mut v = View::new(id, cwd, title, tx);
    crate::engine::trace("view new");
    // the menu's merge tab picked a PR of this thread: its merge tab on it
    if let Some(pr) = take_pr(id) {
        v.show_pr(&pr);
    }
    // the view ends (any way): the base keyboard flags again
    struct AllKeysOff;
    impl Drop for AllKeysOff {
        fn drop(&mut self) {
            crate::all_keys(false);
        }
    }
    let _keys = AllKeysOff;
    // a handoff: its document waits as the prompt draft
    if let Some(d) = crate::handoff::take_draft(id) {
        v.input.set_text(&d);
        v.input.move_end();
    }
    if let Some(n) = note.filter(|n| !n.is_empty()) {
        v.toast(n.to_string());
    }
    v.save_prompt();
    crate::nav::mark_seen_now(id);
    let mut opening = Some(opened);
    let mut roster_at = Instant::now();
    let mut flows_at = Instant::now();
    let mut drawn = Instant::now() - Duration::from_secs(1);
    let mut dirty = true;
    // the tail: on a `wake` of the host; each second as a fallback (0 host, a lost line)
    let mut wake = true;
    let mut read_at = Instant::now();
    let mut notes_v = crate::notify::version();
    let mut count_sec = 0;
    loop {
        // new notifications / seen marks: the header redraws
        let nv = crate::notify::version();
        if nv != notes_v {
            notes_v = nv;
            dirty = true;
        }
        // a frame on change; animations (wave, spinners, toast) at 150 ms; else 1 per second
        // (clock, cost box, sending timer)
        let every = if v.spawning() || crate::dissolve::active() || v.menu.as_ref().is_some_and(|m| m.waiting()) { Duration::from_millis(33) } else if v.animating() { Duration::from_millis(150) } else { Duration::from_secs(1) };
        if dirty || drawn.elapsed() >= every {
            let (pane, menu) = (v.pane, v.menu.is_some());
            let _w = crate::engine::watch(move || format!("view: draw {pane:?}{}", if menu { " + menu" } else { "" }));
            term.draw(|f| v.draw(f)).map_err(|e| e.to_string())?;
            drawn = Instant::now();
            dirty = false;
            crate::engine::first_frame();
            if let Some(at) = opening.take() {
                let took = at.elapsed();
                if took > crate::engine::BUDGET {
                    crate::engine::trace(&format!("SLOW {} view open → 1st frame", took.as_millis()));
                }
            }
        }
        crate::all_keys(v.all_keys());
        if let Some(h) = hook.as_mut()
            && h(&Peek { pane: v.pane, menu: v.menu.is_some() })
        {
            return Ok(None);
        }
        for (ev, at) in crate::read_ready(Duration::from_millis(50))? {
            let tag = if crate::engine::trace_on() { format!("view {:?}{}: {ev:?}", v.pane, if v.menu.is_some() { " + menu" } else { "" }) } else { String::new() };
            let _w = crate::engine::watch(move || tag);
            crate::textarea::stamp(Some(at));
            dirty = true;
            let done = match ev {
                // a release: only a modifier's (an alt tap)
                Event::Key(k) if k.kind != KeyEventKind::Release || matches!(k.code, KeyCode::Modifier(_)) => v.key(k),
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
            // every change to disk before the next event: a crash loses 0 keys
            v.save_prompt();
            if done {
                crate::nav::mark_seen_now(id);
                return Ok(Some(to_main(std::mem::replace(&mut v.exit, Exit::Back), of.as_deref())));
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
        // a sub-thread that ended while in view (it deletes itself): its thread, the end there
        if let Some(main) = of.as_deref()
            && !store::thread_dir(id).join("thread.json").is_file()
            && let Some(t) = store::load(main)
        {
            return Ok(Some(Exit::Open { id: t.id.clone(), dir: t.work_dir(), title: t.title.clone(), note: "the sub-thread ended: its result is in the stream".into() }));
        }
        if v.handoff_due() {
            crate::nav::mark_seen_now(id);
            return Ok(Some(to_main(std::mem::replace(&mut v.exit, Exit::Back), of.as_deref())));
        }
        let pane = v.pane;
        let _w = crate::engine::watch(move || format!("view: tick {pane:?}"));
        dirty |= v.hist.poll();
        dirty |= v.comp.poll();
        dirty |= v.merge.poll(v.pane == Pane::Merge);
        dirty |= v.files.poll();
        dirty |= v.poll_merge_sources();
        while let Ok(m) = rx.try_recv() {
            dirty = true;
            match m {
                Msg::Done("send", Err(e)) => {
                    let e = e.lines().next().unwrap_or_default().to_string();
                    // an empty prompt: the message back in it (never lost), else marked failed
                    if v.input.text().trim().is_empty()
                        && let Some(i) = v.pending.iter().rposition(|p| p.2.is_none())
                    {
                        let (text, ..) = v.pending.remove(i);
                        v.input.set_text(&text);
                        v.input.move_end();
                        v.save_prompt();
                        v.toast(format!("not sent: {e} · the message is back in the prompt"));
                    } else if let Some(p) = v.pending.iter_mut().rev().find(|p| p.2.is_none()) {
                        p.2 = Some(e);
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
                Msg::Shell(cmd, out, code, dir) => {
                    v.shell_done(cmd, out, code, &dir);
                    wake = true;
                }
                Msg::Head(head) => v.take_head(head),
                Msg::ShellCwd(dir) => {
                    v.shell_cwd = dir;
                    v.dirty = true;
                }
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
        // an auto resume pending: its countdown in the stream, 1 redraw per second
        let sec = crate::engine::now_ms() / 1000;
        if !v.thread.resumes.is_empty() && sec != count_sec {
            count_sec = sec;
            v.dirty = true;
            dirty = true;
        }
        if wake || read_at.elapsed() >= Duration::from_secs(1) {
            wake = false;
            read_at = Instant::now();
            dirty |= v.read();
        }
        dirty |= v.monitors.tick(id);
        dirty |= v.subagents.tick(&v.thread.subs);
        dirty |= v.subthreads.tick(id);
        // the flow rows: 1 read of the thread per second for all of them
        let mut flows_changed = false;
        if flows_at.elapsed() >= Duration::from_secs(1) {
            flows_at = Instant::now();
            if let Some(t) = store::load(id) {
                for b in &mut v.flowbars {
                    flows_changed |= b.sync(&t, false);
                }
            }
        }
        if flows_changed {
            // a wait came / went: the question picker's ask too
            v.sync_flow_ask();
            v.dirty = true;
            dirty = true;
        }
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

    #[test]
    fn changes_follow_the_repo_when_the_work_dir_has_0_git() {
        let _g = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!("codehead-nogit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (plain, repo) = (root.join("plain"), root.join("plain/app"));
        std::fs::create_dir_all(&repo).unwrap();
        assert!(std::process::Command::new("git").args(["init", "-q"]).current_dir(&repo).status().unwrap().success());
        // codehead started in a dir without git; an agent edited in the repo below it
        let mut t = store::Thread::new("no git", "task", &plain, vec!["ada".into()]);
        t.checkouts.push(store::Checkout { dir: repo.display().to_string(), branch: None });
        std::fs::create_dir_all(store::thread_dir(&t.id)).unwrap();
        t.save().unwrap();
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new(&t.id, &plain, "no git", tx);
        v.load_checkouts();
        assert_eq!(v.checkouts, std::slice::from_ref(&repo));
        assert_eq!(v.changes.dir(), repo.as_path());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The owner writes freely while a gate waits: the prompt keeps the keys, ctrl+enter sends a
    /// message (the gate stays), `changes` with 0 words sends nothing, `reply` sends the prompt.
    #[test]
    fn a_gate_never_blocks_the_owners_message() {
        let _g = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut t = store::Thread::new("gate input", "task", Path::new("/tmp"), vec!["ada".into()]);
        let lib = crate::engine::flows::Library::load(None);
        let mut st = crate::engine::flows::run::FlowState::new(lib.flow("new-feature").unwrap(), &lib, 3, 0);
        st.wait = Some(crate::engine::flows::run::Wait::Gate);
        t.flow = Some(st);
        std::fs::create_dir_all(store::thread_dir(&t.id)).unwrap();
        t.save().unwrap();
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new(&t.id, Path::new("/tmp"), "gate input", tx);
        v.members = t.agents.clone();
        v.sync_flowbars();
        v.sync_flow_ask();
        let sends = |v: &View| v.ops.borrow().iter().filter(|o| o["op"] == "prompt").count();
        let answers = |v: &View| v.ops.borrow().iter().filter(|o| o["op"] == "flow_answer").count();

        // typed text: the prompt's; the hint says how to send and how to answer
        let s = screen(&mut v).join("\n");
        assert!(s.contains("ctrl+enter: your message (the wait stays) · tab: the choices"), "{s}");
        for c in "1 more".chars() {
            v.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(v.input.text(), "1 more", "digits and letters: the prompt's, 0 pick");
        assert_eq!(answers(&v), 0);

        // ctrl+enter: a message, the gate stays
        v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        assert_eq!((sends(&v), answers(&v)), (1, 0));
        assert!(v.thread.asks.iter().any(|a| a.tool == crate::flowbar::WAIT_TOOL), "the gate stays");

        // changes (2) with an empty prompt: 0 answer, the keys back to the prompt
        press(&mut v, &[KeyCode::Tab, KeyCode::Char('2')]);
        assert_eq!(answers(&v), 0, "{:?}", v.ops.borrow());
        assert!(!v.ask.focus);

        // reply (3): empty → 0 send; with text → a message, the gate stays
        press(&mut v, &[KeyCode::Tab, KeyCode::Char('3')]);
        assert_eq!(sends(&v), 1);
        v.input.set_text("also split the plan");
        press(&mut v, &[KeyCode::Tab, KeyCode::Char('3')]);
        assert_eq!((sends(&v), answers(&v)), (2, 0));
        assert!(v.thread.asks.iter().any(|a| a.tool == crate::flowbar::WAIT_TOOL));
        let _ = store::delete(&t.id);
    }

    #[test]
    fn changes_follow_the_worktree_an_agent_moved_the_work_to_until_the_owner_picks() {
        let _g = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!("codehead-moved-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (repo, wt) = (root.join("app"), root.join("app-feat"));
        std::fs::create_dir_all(&repo).unwrap();
        let sh = |c: &str| assert!(std::process::Command::new("sh").arg("-c").arg(c).current_dir(&repo).output().unwrap().status.success(), "{c}");
        sh("git init -q && git -c user.email=t@t -c user.name=t commit -q --allow-empty -m init && git worktree add -q -b feat ../app-feat");
        // the thread's work dir: the main checkout; its agent moved to the worktree
        let mut t = store::Thread::new("moved", "task", &repo, vec!["ada".into()]);
        t.checkouts.push(store::Checkout { dir: wt.display().to_string(), branch: Some("feat".into()) });
        std::fs::create_dir_all(store::thread_dir(&t.id)).unwrap();
        t.save().unwrap();
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new(&t.id, &repo, "moved", tx);

        v.load_checkouts();

        assert_eq!(v.checkouts, vec![repo.clone(), wt.clone()]);
        assert_eq!(v.changes.dir(), wt.as_path(), "the worktree, not the main checkout");

        // the owner picks the main checkout: it stays over a reload
        v.pick_checkout(0);
        v.load_checkouts();

        assert_eq!(v.changes.dir(), repo.as_path());

        // a thread in its own worktree, an agent edit in the main checkout: the worktree stays
        let mut t = store::Thread::new("own worktree", "task", &repo, vec!["ada".into()]);
        t.worktree = Some(wt.display().to_string());
        t.checkouts.push(store::Checkout { dir: repo.display().to_string(), branch: None });
        std::fs::create_dir_all(store::thread_dir(&t.id)).unwrap();
        t.save().unwrap();
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new(&t.id, &wt, "own worktree", tx);

        v.load_checkouts();

        assert_eq!(v.changes.dir(), wt.as_path(), "the thread's own worktree");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_changes_panel_shows_a_commit_read_only_and_esc_brings_the_changes_back() {
        use crate::changes::commits::fixture;
        let _g = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let w = fixture::repo("thread-view");
        fixture::sh(&w, "echo draft > x.txt");
        let t = store::Thread::new("commits", "task", &w, vec!["ada".into()]);
        std::fs::create_dir_all(store::thread_dir(&t.id)).unwrap();
        t.save().unwrap();
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new(&t.id, &w, "commits", tx);
        let k = |v: &mut View, c: KeyCode, m: KeyModifiers| {
            v.key(KeyEvent::new(c, m));
        };
        let wait = |v: &mut View, pat: &str| -> String {
            for _ in 0..200 {
                let s = screen(v).join("\n");
                if s.contains(pat) {
                    return s;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            panic!("0 {pat}: {}", screen(v).join("\n"));
        };

        // a draft in the prompt, then the changes view, the panel with the keys
        typed_text(&mut v, "draft");
        k(&mut v, KeyCode::Char('2'), KeyModifiers::ALT);
        // (a new read dissolves in over a few frames: each label waited for)
        wait(&mut v, "UNPUSHED");
        wait(&mut v, "COMMITS  4");
        wait(&mut v, "PUSHED    2");
        k(&mut v, KeyCode::Char('b'), KeyModifiers::CONTROL);

        // G: the last row (the oldest pushed commit); enter: its files, read only
        k(&mut v, KeyCode::Char('G'), KeyModifiers::NONE);
        k(&mut v, KeyCode::Enter, KeyModifiers::NONE);
        let all = wait(&mut v, "c1 subject · 1 files");

        assert!(all.contains("c1.txt") && all.contains("◆"), "{all}");

        k(&mut v, KeyCode::Char('c'), KeyModifiers::NONE);
        assert!(!screen(&mut v).join("\n").contains("comment on"), "0 comment modal");

        // esc: the changes back (x.txt), the draft still in the prompt
        k(&mut v, KeyCode::Esc, KeyModifiers::NONE);
        let all = screen(&mut v).join("\n");

        assert!(!all.contains("c1 subject · ") && all.contains("x.txt"), "{all}");
        assert_eq!(v.input.text(), "draft");
        fixture::drop(&w);
    }

    /// TEXT typed into the prompt.
    fn typed_text(v: &mut View, text: &str) {
        for c in text.chars() {
            v.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
    }

    #[test]
    fn the_flow_row_shows_a_gate_and_its_box_answers_and_switches() {
        let _g = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut t = store::Thread::new("flow view", "task", Path::new("/tmp"), vec!["ada".into()]);
        let lib = crate::engine::flows::Library::load(None);
        let mut st = crate::engine::flows::run::FlowState::new(lib.flow("new-feature").unwrap(), &lib, 3, 0);
        st.wait = Some(crate::engine::flows::run::Wait::Gate);
        t.flow = Some(st);
        std::fs::create_dir_all(store::thread_dir(&t.id)).unwrap();
        t.save().unwrap();
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new(&t.id, Path::new("/tmp"), "flow view", tx);
        v.members = t.agents.clone();
        v.sync_flowbars();
        // closed: 0 flow row, the card's 4th row has the step; a click on it opens the box (the
        // row of before on its bottom), a 2nd click closes it
        let s = screen(&mut v).join("\n");
        assert!(s.contains("⏸ 4/9 plan approval") && !s.contains("◆ new feature · 4/9"), "{s}");
        let card = s.lines().position(|l| l.contains("⏸ 4/9 plan approval")).unwrap();
        assert!(s.lines().nth(card - 3).is_some_and(|l| l.contains("ada")), "name 3 rows above the step: {s}");
        let r = v.flow_slots.borrow()[0].1;
        let click = MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: r.x + 3, row: r.y, modifiers: KeyModifiers::NONE };
        v.mouse(click);
        let s = screen(&mut v).join("\n");
        assert!(s.contains("◆ new feature · 4/9 plan approval · @ada") && s.contains("⏸ gate plan approval: answer below"), "{s}");
        v.mouse(click);
        let s = screen(&mut v).join("\n");
        assert!(!s.contains("◆ new feature · 4/9"), "{s}");
        // the gate: a question in the picker under the stream; it never takes the keys (the owner
        // may write first), tab gives them to it; the prompt's text goes along as the note
        v.sync_flow_ask();
        assert!(!v.ask.focus, "an empty prompt, 0 keys: the gate still waits for tab");
        v.input.set_text("split the plan");
        let s = screen(&mut v).join("\n");
        assert!(s.contains("waits on you") && s.contains("gate after “plan approval”") && s.contains("approve · go on") && s.contains("changes · back to plan") && s.contains("reply · the prompt's text"), "{s}");
        press(&mut v, &[KeyCode::Tab]);
        assert!(v.ask.focus);

        press(&mut v, &[KeyCode::Down, KeyCode::Enter]);
        let op = v.ops.borrow().iter().find(|o| o["op"] == "flow_answer").cloned().unwrap();
        assert_eq!((op["choice"].as_str(), op["note"].as_str()), (Some("changes"), Some("split the plan")));
        assert!(v.input.text().is_empty());
        assert!(v.thread.asks.is_empty(), "answered: the flow's ask goes");
        assert!(!v.ops.borrow().iter().any(|o| o["op"] == "answer"), "0 host ask answer");
        // change the flow: the box asks now / after at once, then the start step
        v.open_flow("ada", false);
        v.dirty = true;
        let s = screen(&mut v).join("\n");
        println!("{s}");
        assert!(s.contains("every step runs on @ada") && s.contains("◀"), "{s}");
        press(&mut v, &[KeyCode::Char('c'), KeyCode::Char('r'), KeyCode::Char('e'), KeyCode::Char('f'), KeyCode::Enter]);
        let s = screen(&mut v).join("\n");
        println!("{s}");
        assert!(s.contains("switch now") && s.contains("after current flow"), "{s}");
        press(&mut v, &[KeyCode::Enter, KeyCode::Down, KeyCode::Enter]);
        let op = v.ops.borrow().iter().find(|o| o["op"] == "flow_set").cloned().unwrap();
        assert_eq!((op["flow"].as_str(), op["start"].as_u64(), op["mode"].as_str()), (Some("refactor"), Some(1), Some("now")));
        let s = screen(&mut v).join("\n");
        assert!(s.contains("→ refactor at the next tool boundary") || s.contains("4/9 plan approval → refactor"), "{s}");
        let _ = std::fs::remove_dir_all(store::thread_dir(&t.id));
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
        assert!(!s.contains("› [x]") && s.contains("tab: answer the question"), "{s}");
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

    /// `>sub @agent task` + ctrl+enter: a sub-thread in the same checkout, 0 message to this
    /// thread's agents, its row at once; `>sub` alone: a hint, the text stays.
    #[test]
    fn sub_starts_a_sub_thread_of_this_thread() {
        let mut t = store::Thread::new("main", "task", Path::new("/tmp"), vec!["ada".into(), "bob".into()]);
        t.worktree = Some("/tmp/wt".into());
        t.branch = Some("task/x".into());
        std::fs::create_dir_all(store::thread_dir(&t.id)).unwrap();
        t.save().unwrap();
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new(&t.id, Path::new("/tmp"), "main", tx);
        v.members = vec!["ada".into(), "bob".into()];

        v.input.set_text(">sub");
        v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        assert!(v.ops.borrow().is_empty() && v.input.text() == ">sub", "0 task: 0 sub-thread");

        v.input.set_text(">sub @bob split the diff into commits");
        v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        let ops = v.ops.borrow().clone();
        assert_eq!(ops.len(), 1, "only the sub_start to this thread's host: {ops:?}");
        assert_eq!(ops[0]["op"], "sub_start");
        let kid = ops[0]["id"].as_str().unwrap().to_string();
        let k = store::load(&kid).unwrap();
        assert_eq!((k.of.as_deref(), k.agents.as_slice(), k.worktree.as_deref(), k.branch.as_deref()), (Some(t.id.as_str()), &["bob".to_string()][..], Some("/tmp/wt"), Some("task/x")));
        assert!(v.input.is_empty());
        assert!(v.subthreads.active() && v.subthreads.kids[0].title.contains("split the diff"));
        // the thread lists leave it out
        assert!(!store::all().iter().any(|x| x.id == kid));
        // its view: esc esc goes back to this thread, not to the start screen
        assert!(matches!(to_main(Exit::Back, k.of.as_deref()), Exit::Open { id, .. } if id == t.id));
        assert!(matches!(to_main(Exit::Back, None), Exit::Back));
        assert!(matches!(to_main(Exit::Quit, k.of.as_deref()), Exit::Quit));
        let _ = std::fs::remove_dir_all(store::thread_dir(&kid));
        let _ = std::fs::remove_dir_all(store::thread_dir(&t.id));
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

    fn view() -> View {
        let (tx, _rx) = mpsc::channel();
        View::new("panetest-0000", Path::new("/tmp"), "thread p", tx)
    }

    fn press(v: &mut View, k: KeyCode, m: KeyModifiers) -> bool {
        v.key(KeyEvent::new(k, m))
    }

    fn screen(v: &mut View) -> String {
        let mut t = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        t.draw(|f| v.draw(f)).unwrap();
        let b = t.backend().buffer().clone();
        (0..30).map(|y| (0..100).map(|x| b[(x, y)].symbol().to_string()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    /// A spawn under way (the thread just opened): the `>` dropdown stays over its goo.
    #[test]
    fn spawn_never_draws_over_the_command_dropdown() {
        fn buffer(v: &mut View, text: &str) -> ratatui::buffer::Buffer {
            v.input.set_text(text);
            let mut t = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
            t.draw(|f| v.draw(f)).unwrap();
            t.backend().buffer().clone()
        }

        let mut v = view();
        v.members = vec!["ada".into()];
        v.sync_order();
        let key = crate::config::command_key().to_string();
        let (calm, calm_menu) = (buffer(&mut v, ""), buffer(&mut v, &key));
        let areas = v.command.areas().to_vec();
        assert!(!areas.is_empty(), "the dropdown shows");

        // the goo at its densest: just before the snap
        // a frozen clock: a slow CI runner never moves the spawn past the snap between draws
        let now = crate::avatar::now();
        crate::avatar::freeze(Some(now));
        let start = now - crate::avatar::spawn::timing(0).2 * 0.95;
        v.spawns.insert("ada".into(), crate::avatar::Spawn { anim: 0, start });
        let (goo, goo_menu) = (buffer(&mut v, ""), buffer(&mut v, &key));
        let slot = v.slots.borrow()[0].1;
        assert!(slot.intersects(areas[0]), "the dropdown covers the sprite");
        let differs = (slot.top()..slot.bottom()).any(|y| (slot.left()..slot.right()).any(|x| calm[(x, y)].symbol() != goo[(x, y)].symbol()));
        assert!(differs, "the spawn draws");
        for r in &areas {
            for y in r.top()..r.bottom() {
                for x in r.left()..r.right() {
                    assert_eq!(calm_menu[(x, y)].symbol(), goo_menu[(x, y)].symbol(), "spawn over the dropdown at {x},{y}");
                }
            }
        }
        crate::avatar::freeze(None);
    }

    #[test]
    fn view_bar_shows_both_views_and_alt_digits_switch() {
        let mut v = view();
        let s = screen(&mut v);
        // the changes label may carry a count (other tests share the thread id)
        assert!(s.contains(" chat   changes") && s.contains("files  │ thread p"), "{s}");
        assert!(s.contains("F1-F4: views"), "{s}");
        // the view bar on row 0
        assert!(s.lines().next().unwrap().contains(" chat "), "{s}");
        press(&mut v, KeyCode::Char('2'), KeyModifiers::ALT);
        assert_eq!(v.pane, Pane::Changes);
        let s = screen(&mut v);
        assert!(s.contains("space: select") && s.contains("v: reviewed") && s.contains("ctrl+m: prompt"), "{s}");
        press(&mut v, KeyCode::Char('1'), KeyModifiers::ALT);
        assert_eq!(v.pane, Pane::Chat);
    }

    #[test]
    fn tab_moves_between_the_view_and_the_prompt_ctrl_p_is_the_menu() {
        let mut v = view();
        press(&mut v, KeyCode::Char('2'), KeyModifiers::ALT);
        press(&mut v, KeyCode::Tab, KeyModifiers::NONE);
        assert!(v.prompt_focus && v.menu.is_none(), "tab: the prompt, 0 menu");
        press(&mut v, KeyCode::Char('h'), KeyModifiers::NONE);
        press(&mut v, KeyCode::Tab, KeyModifiers::NONE);
        assert!(!v.prompt_focus && v.input.text() == "h", "tab: back to the view, the text stays");
        // the chat: tab never opens the menu; ctrl+p does, from every field
        press(&mut v, KeyCode::Char('1'), KeyModifiers::ALT);
        press(&mut v, KeyCode::Tab, KeyModifiers::NONE);
        assert!(v.menu.is_none());
        press(&mut v, KeyCode::Char('p'), KeyModifiers::CONTROL);
        assert!(v.menu.is_some());
        press(&mut v, KeyCode::Char('p'), KeyModifiers::CONTROL);
        assert!(v.menu.is_none(), "ctrl+p again: closed");
        // the changes view with the keys: ctrl+p opens the menu there too
        press(&mut v, KeyCode::Char('2'), KeyModifiers::ALT);
        press(&mut v, KeyCode::Char('p'), KeyModifiers::CONTROL);
        assert!(v.menu.is_some());
    }

    #[test]
    fn changes_view_keeps_letters_from_the_prompt_until_ctrl_m() {
        let mut v = view();
        press(&mut v, KeyCode::Char('2'), KeyModifiers::ALT);
        press(&mut v, KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(v.input.text(), "", "j: a view key");
        press(&mut v, KeyCode::Char('m'), KeyModifiers::CONTROL);
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
        assert!(s.contains("▾ medium") && s.contains("┌─ ☐ src/a.rs"), "{s}");
        // ↓ ↓ ↓: head → hunk → ` x` → `- a`; space selects 2 lines (the cursor goes down)
        for _ in 0..3 {
            press(&mut v, KeyCode::Down, KeyModifiers::NONE);
        }
        press(&mut v, KeyCode::Char(' '), KeyModifiers::NONE);
        press(&mut v, KeyCode::Char(' '), KeyModifiers::NONE);
        // c: the modal takes the comment on both lines
        press(&mut v, KeyCode::Char('c'), KeyModifiers::NONE);
        assert!(!v.prompt_focus && v.changes.typing());
        let s = screen(&mut v);
        assert!(s.contains("comment on src/a.rs:2 · removed 2"), "{s}");
        press(&mut v, KeyCode::Char('n'), KeyModifiers::NONE);
        press(&mut v, KeyCode::Enter, KeyModifiers::CONTROL);
        assert_eq!(v.changes.comment_count(), 1);
        let s = screen(&mut v);
        assert!(s.contains("2 · removed 2 · you: n"), "{s}");
        // ctrl+f: the filter takes letters, i too; esc leaves it (0 interrupt timer)
        press(&mut v, KeyCode::Char('f'), KeyModifiers::CONTROL);
        press(&mut v, KeyCode::Char('i'), KeyModifiers::NONE);
        assert!(!v.prompt_focus && v.input.text().is_empty());
        assert!(screen(&mut v).contains(" f  i "));
        press(&mut v, KeyCode::Esc, KeyModifiers::NONE);
        assert!(!v.changes.typing() && v.last_esc.is_none());
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("panetest-0000"));
    }
}

#[cfg(test)]
mod comment_tests {
    use super::*;

    #[test]
    fn a_leading_bang_runs_a_shell_command_its_output_goes_along_with_the_next_message() {
        let (tx, rx) = mpsc::channel();
        let mut v = View::new("shelltest-0000", Path::new("/tmp"), "t", tx);
        let mut t = Terminal::new(ratatui::backend::TestBackend::new(110, 30)).unwrap();
        t.draw(|f| v.draw(f)).unwrap();
        let shown = |t: &Terminal<ratatui::backend::TestBackend>| t.backend().buffer().content().iter().map(|c| c.symbol().to_string()).collect::<String>();
        assert!(shown(&t).contains("! for shell mode"));
        for c in "!echo hi; echo oops >&2; exit 3".chars() {
            v.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        t.draw(|f| v.draw(f)).unwrap();
        assert!(shown(&t).contains("! shell mode"), "{}", shown(&t));
        // enter runs it: 0 message to the agents
        v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(v.input.is_empty() && v.ops.borrow().is_empty());
        let Ok(Msg::Shell(cmd, out, code, dir)) = rx.recv_timeout(Duration::from_secs(10)) else { panic!("0 shell result") };
        assert_eq!((cmd.as_str(), code), ("echo hi; echo oops >&2; exit 3", 3));
        assert!(out.contains("hi") && out.contains("oops"), "{out}");
        v.shell_done(cmd, out, code, &dir);
        t.draw(|f| v.draw(f)).unwrap();
        assert!(shown(&t).contains("exit 3") && shown(&t).contains("⎿ hi") && shown(&t).contains("in /tmp"), "{}", shown(&t));
        // the next message takes the output along (for the agents only), once
        v.input.set_text("why does it fail?");
        v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        let sent = v.ops.borrow().last().cloned().unwrap();
        assert_eq!(sent["text"], "why does it fail?");
        let ctx = sent["context"].as_str().unwrap();
        assert!(ctx.contains("$ echo hi; echo oops >&2; exit 3  (exit 3, in /tmp)") && ctx.contains("oops"), "{ctx}");
        v.input.set_text("again");
        v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        assert_eq!(v.ops.borrow().last().unwrap()["context"], "");
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("shelltest-0000"));
    }

    #[test]
    fn shell_mode_completes_project_tasks_and_suggests_from_the_history() {
        let dir = std::env::temp_dir().join(format!("ch-shellcomp-view-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join("justfile"), "# build the app\nbuild:\n    true\n\n# run the tests\ntest:\n    true\n").unwrap();
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("shelltest-0002", &dir, "t", tx);
        v.shell_cwd = dir.clone();
        v.comp.history = Some(std::sync::Arc::new(Vec::new()));
        let mut t = Terminal::new(ratatui::backend::TestBackend::new(110, 30)).unwrap();
        let shown = |t: &Terminal<ratatui::backend::TestBackend>| t.backend().buffer().content().iter().map(|c| c.symbol().to_string()).collect::<String>();
        let key = |v: &mut View, c: KeyCode| {
            v.key(KeyEvent::new(c, KeyModifiers::NONE));
        };
        // `just `: its recipes at once, with their docs; tab: the 1st in the prompt, enter takes it
        for c in "!just ".chars() {
            key(&mut v, KeyCode::Char(c));
        }
        t.draw(|f| v.draw(f)).unwrap();
        assert!(shown(&t).contains("build the app") && shown(&t).contains("run the tests"), "{}", shown(&t));
        key(&mut v, KeyCode::Tab);
        key(&mut v, KeyCode::Tab);
        assert_eq!(v.input.text(), "! just test");
        key(&mut v, KeyCode::Enter);
        assert_eq!(v.input.text(), "! just test ", "enter took the row, 0 run");
        assert_eq!(v.shell_running, 0);
        // the thread's commands: the rest in gray after the cursor, → takes it
        v.thread.items.push(stream::Item::Shell { command: "just test --nocapture".into(), output: String::new(), code: 0, ts: 0, dir: None });
        t.draw(|f| v.draw(f)).unwrap();
        assert!(shown(&t).contains("! just test --nocapture"), "{}", shown(&t));
        key(&mut v, KeyCode::Right);
        assert_eq!(v.input.text(), "! just test --nocapture");
        // ↑ with text: the thread's commands, 0 dropdown
        v.input.set_text("! just b");
        key(&mut v, KeyCode::Up);
        assert_eq!(v.input.text(), "! just test --nocapture");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("shelltest-0002"));
    }

    #[test]
    fn shell_mode_cycles_its_commands_and_down_below_them_edits_the_dir() {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("shelltest-0001", Path::new("/tmp"), "t", tx);
        let key = |v: &mut View, c: KeyCode| {
            v.key(KeyEvent::new(c, KeyModifiers::NONE));
        };
        // `!` on an empty prompt: a space after it
        key(&mut v, KeyCode::Char('!'));
        assert_eq!(v.input.text(), "! ");
        // 1 backspace: shell mode off, the prompt empty
        key(&mut v, KeyCode::Backspace);
        assert!(v.input.text().is_empty() && !v.shell_mode());
        key(&mut v, KeyCode::Char('!'));
        // a command typed: backspace deletes 1 char only
        key(&mut v, KeyCode::Char('l'));
        key(&mut v, KeyCode::Backspace);
        assert_eq!(v.input.text(), "! ");
        for c in ["ls", "pwd", "ls"] {
            v.thread.items.push(stream::Item::Shell { command: c.into(), output: String::new(), code: 0, ts: 0, dir: None });
        }
        v.input.set_text("! gi");
        // ↑: newest first (a repeat once), ↓ back to the draft
        key(&mut v, KeyCode::Up);
        assert_eq!(v.input.text(), "! ls");
        key(&mut v, KeyCode::Up);
        assert_eq!(v.input.text(), "! pwd");
        key(&mut v, KeyCode::Up);
        assert_eq!(v.input.text(), "! pwd");
        key(&mut v, KeyCode::Down);
        key(&mut v, KeyCode::Down);
        assert_eq!(v.input.text(), "! gi");
        // ↓ on the draft: the dir input; enter: shell mode's dir, kept for the thread
        key(&mut v, KeyCode::Down);
        assert_eq!(v.cwd_edit.as_ref().map(TextArea::text).as_deref(), Some("/tmp/"));
        let dir = std::env::temp_dir().join(format!("ch-cwd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        v.cwd_edit.as_mut().unwrap().set_text(&dir.display().to_string());
        key(&mut v, KeyCode::Enter);
        assert!(v.cwd_edit.is_none());
        assert_eq!(v.shell_cwd, std::fs::canonicalize(&dir).unwrap());
        assert_eq!(v.input.text(), "! gi");
        // `cd ..` alone: the dir too, 0 command run
        v.input.set_text("! cd ..");
        key(&mut v, KeyCode::Enter);
        assert_eq!(v.shell_cwd, std::fs::canonicalize(std::env::temp_dir()).unwrap());
        assert_eq!(v.shell_running, 0);
        let (tx, _rx) = mpsc::channel();
        let w = View::new("shelltest-0001", Path::new("/"), "t", tx);
        assert_eq!(w.shell_cwd, std::fs::canonicalize(std::env::temp_dir()).unwrap(), "the pick stays");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("shelltest-0001"));
    }

    #[test]
    fn cd_alone_changes_the_dir_other_commands_run() {
        assert_eq!(cd_target("cd src"), Some("src"));
        assert_eq!(cd_target("cd"), Some(""));
        assert_eq!(cd_target("cd '~/x'"), Some("~/x"));
        assert_eq!(cd_target("cd src && ls"), None);
        assert_eq!(cd_target("cdk deploy"), None);
        assert_eq!(cd_target("cd a b"), None);
        assert_eq!(cd_target("cd \"My Notes/x\""), Some("My Notes/x"));
    }

    #[test]
    fn a_comment_opens_a_modal_and_the_prompt_keeps_its_draft() {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("commenttest-0000", Path::new("/tmp"), "t", tx);
        v.set_pane(Pane::Changes);
        v.changes.set_files(crate::changes::parse("diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@\n x\n-a\n+b\n y\n"));
        v.input.set_text("half a message");
        let mut t = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        t.draw(|f| v.draw(f)).unwrap();
        v.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        assert!(!v.prompt_focus && v.changes.typing(), "the modal has the keys");
        v.key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE));
        v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        assert_eq!(v.changes.comment_count(), 1);
        assert!(!v.prompt_focus && v.input.text() == "half a message");
        // esc on a typed comment: asks (the text stays); esc again cancels: 0 comment, 0
        // interrupt timer
        v.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        v.key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        v.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(v.changes.typing(), "1 esc: the comment stays");

        v.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(v.changes.comment_count(), 1);
        assert!(!v.changes.typing() && v.last_esc.is_none());
        assert_eq!(v.input.text(), "half a message");
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("commenttest-0000"));
    }

    #[test]
    fn the_commit_message_lives_in_the_panel() {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("committest-0000", Path::new("/tmp"), "t", tx);
        v.set_pane(Pane::Changes);
        v.input.set_text("draft");
        v.key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        assert!(v.changes.typing() && !v.prompt_focus);
        v.key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        // 0 staged files: 0 commit, the message stays
        v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        assert!(v.changes.typing());
        v.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!v.changes.typing() && v.input.text() == "draft" && !v.prompt_focus);
    }

    #[test]
    fn a_message_from_the_changes_view_takes_the_selection_along() {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("contexttest-0000", Path::new("/tmp"), "t", tx);
        v.set_pane(Pane::Changes);
        v.changes.set_files(crate::changes::parse("diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@\n x\n-a\n+b\n y\n"));
        let mut t = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        t.draw(|f| v.draw(f)).unwrap();
        // ↓ ↓ ↓ ↓: `+ b`; space: selected
        for _ in 0..4 {
            v.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        v.key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        v.key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::CONTROL));
        v.input.set_text("stage only these");
        v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        let sent = v.ops.borrow().last().cloned().unwrap();
        let text = sent["text"].as_str().unwrap();
        assert!(text.starts_with("stage only these\n\n[codehead: context from the changes view]") && text.contains("+ b"), "{text}");
        // the chat above the prompt shows (narrow: 0 split)
        let s = { t.draw(|f| v.draw(f)).unwrap(); t.backend().buffer().clone() };
        let rows: Vec<String> = (0..30).map(|y| (0..100).map(|x| s[(x, y)].symbol().to_string()).collect()).collect();
        assert!(rows.iter().any(|l| l.contains("▾ chat · t: fold")), "{rows:#?}");
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("contexttest-0000"));
    }

    #[test]
    fn an_agent_named_in_a_comment_gets_it_at_once_with_its_thread() {
        let id = "mentiontest-0000";
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(id));
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new(id, Path::new("/tmp"), "t", tx);
        v.members = vec!["REV".into(), "LEAD".into()];
        v.changes_agents();
        v.set_pane(Pane::Changes);
        v.changes.set_files(crate::changes::parse("diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@\n x\n-a\n+b\n y\n"));
        let mut t = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        t.draw(|f| v.draw(f)).unwrap();
        // ↓ ↓ ↓ ↓: `+ b`; c: the comment modal; a draft in the prompt stays
        v.input.set_text("my draft");
        for _ in 0..4 {
            v.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        v.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        for ch in "@REV why b?".chars() {
            v.key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

        let sent = v.ops.borrow().last().cloned().unwrap();
        assert_eq!((sent["op"].as_str(), sent["to"].clone()), (Some("prompt"), json!(["REV"])), "{sent}");
        assert!(sent["text"].as_str().unwrap().starts_with("on comment c1 `src/a.rs:2`: @REV why b?"), "{sent}");
        assert!(sent["context"].as_str().unwrap().contains("- owner: @REV why b?") && sent["context"].as_str().unwrap().contains("`comment_reply`"), "{sent}");
        assert_eq!(v.input.text(), "my draft", "the prompt's draft stays");
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(id));
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
    fn merge_is_the_3rd_view_its_keys_its_context() {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("mergetest-0000", Path::new("/tmp"), "t", tx);
        v.key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::ALT));
        assert_eq!(v.pane, Pane::Merge);
        v.merge = crate::merge::tests::with(crate::merge::tests::pr());
        // letters: the tab's keys (0 prompt text); i: the prompt
        v.key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(v.input.text(), "");
        v.key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        assert!(v.prompt_focus);
        for c in "fix the lint".chars() {
            v.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        let sent = v.pending.last().map(|p| p.0.clone()).unwrap_or_default();
        assert!(sent.starts_with("fix the lint") && sent.contains("[codehead: context from the merge tab]") && sent.contains("CI / lint"), "{sent}");
    }

    #[test]
    fn the_files_filter_takes_every_letter_and_esc() {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("filesfilter-0000", Path::new("/tmp"), "t", tx);
        v.key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::ALT));
        assert_eq!(v.pane, Pane::Files);
        v.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
        for c in "ix".chars() {
            v.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        // `i` went to the filter, not to the prompt focus
        assert!(v.files.typing() && !v.prompt_focus && v.input.text().is_empty());
        v.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!v.files.typing() && v.last_esc.is_none(), "esc: the filter only");
    }

    #[test]
    fn files_is_the_4th_view_with_its_keys() {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("filestest-0000", Path::new("/tmp"), "t", tx);
        v.key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::ALT));
        assert_eq!(v.pane, Pane::Files);
        // letters: view keys, 0 prompt text
        v.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(v.input.text(), "");
        // the menu: yazi on the worktree, for the view loop
        v.menu_action(crate::menu::Action::Command(14));
        assert_eq!(v.yazi.as_deref(), Some(Path::new("/tmp")));
        // a pick: its path into the prompt, relative to the worktree
        v.yazi_picked(&["/tmp/src/a.rs".to_string()]);
        assert_eq!(v.input.text(), "src/a.rs ");
        // alt+1 from files: back to chat
        v.key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::ALT));
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
    fn wide_changes_view_has_the_chat_right_and_alt_backslash_toggles() {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("splittest-0000", Path::new("/tmp"), "t", tx);
        v.set_pane(Pane::Changes);
        let s = screen(&mut v, 220);
        // the divider at 3/5 of the width: the changes left, the chat column right
        assert!(s[2].chars().nth(132) == Some('│'), "{}", s[2]);
        assert!(v.view_rect.x == 0 && v.view_rect.right() <= 132 && v.stream_rect.x == 133, "{:?} {:?}", v.stream_rect, v.view_rect);
        // the prompt in the chat column
        assert!(v.input_rect.x >= 133, "{:?}", v.input_rect);
        v.key(KeyEvent::new(KeyCode::Char('\\'), KeyModifiers::ALT));
        screen(&mut v, 220);
        assert_eq!(v.view_rect.x, 0, "full width");
        // narrow: 0 split
        v.key(KeyEvent::new(KeyCode::Char('\\'), KeyModifiers::ALT));
        screen(&mut v, 150);
        assert_eq!(v.view_rect.x, 0);
    }

    #[test]
    fn a_thread_outside_git_opens_in_the_chat_even_when_wide() {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("splittest-0001", Path::new("/"), "t", tx);
        screen(&mut v, 220);
        assert_eq!(v.pane, Pane::Chat);
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

    /// The chat beside the changes view (wide split): the same selection + copy.
    #[test]
    fn the_chat_of_the_split_selects_and_copies() {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("selecttest-0001", Path::new("/tmp"), "t", tx);
        v.members = vec!["ada".into()];
        v.thread.push(&store::Event { ts: 2, agent: Some("ada".into()), kind: store::Kind::Text { text: "copy me from the split".into() } });
        v.set_pane(Pane::Changes);
        let draw = |v: &mut View| {
            let mut t = Terminal::new(TestBackend::new(200, 40)).unwrap();
            t.draw(|f| v.draw(f)).unwrap();
            let b = t.backend().buffer().clone();
            (0..40).map(|y| (0..200).map(|x| b[(x, y)].symbol().to_string()).collect::<String>()).collect::<Vec<String>>()
        };
        let s = draw(&mut v);
        assert!(v.pane == Pane::Changes && v.stream_rect.width > 0, "{s:#?}");
        let y = s.iter().position(|r| r.contains("copy me")).expect("chat in the split") as u16;
        let x = s[y as usize].find("copy me").map(|b| s[y as usize][..b].chars().count()).unwrap() as u16;
        // 3 clicks: the row
        for _ in 0..3 {
            mouse(&mut v, MouseEventKind::Down(MouseButton::Left), x + 1, y);
            mouse(&mut v, MouseEventKind::Up(MouseButton::Left), x + 1, y);
        }
        let got = crate::clip::LAST.with_borrow(|l| l.clone());
        assert_eq!(got, "copy me from the split");
    }
}

/// Subagents: the row above the monitors, the box by master, stop, the report message, pick up.
#[cfg(test)]
mod subagent_tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn screen(v: &mut View) -> Vec<String> {
        let mut t = Terminal::new(TestBackend::new(100, 40)).unwrap();
        t.draw(|f| v.draw(f)).unwrap();
        let b = t.backend().buffer().clone();
        (0..40).map(|y| (0..100).map(|x| b[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string()).collect()
    }

    fn ev(agent: &str, ts: u64, kind: store::Kind) -> store::Event {
        store::Event { ts, agent: Some(agent.into()), kind }
    }

    fn start(id: &str, parent: Option<&str>, what: &str, background: bool) -> store::Kind {
        store::Kind::SubStart { id: id.into(), parent: parent.map(String::from), description: what.into(), agent_type: "Explore".into(), background, prompt: format!("do {what}") }
    }

    fn click(v: &mut View, x: u16, y: u16) {
        v.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
    }

    #[test]
    fn running_subagents_show_grouped_by_master_and_stop_with_d() {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("subtest-0000", Path::new("/tmp"), "t", tx);
        v.members = vec!["ada".into(), "bob".into()];
        let now = crate::engine::now_ms();
        v.thread.push(&ev("ada", now, start("t1", None, "scan the repo", true)));
        v.thread.push(&ev("ada", now, start("t2", Some("t1"), "read the docs", false)));
        v.thread.push(&ev("bob", now, start("t3", None, "run the tests", false)));
        v.subagents.set_now("t2", "Read docs/a.md");
        // ada's turn ended: her subagents run, so she still works
        assert!(v.thread.working("ada") && !v.thread.busy.contains("ada"));
        assert!(v.animating());
        let s = screen(&mut v);
        let row = s.iter().position(|r| r.contains("subagents (3)")).unwrap_or_else(|| panic!("{s:#?}"));
        assert!(s[row].contains("ada: scan the repo") && s[row].contains("bob: run the tests"), "{}", s[row]);
        assert!(s.iter().any(|r| r.contains("subagents…")), "the agent card: {s:#?}");
        click(&mut v, 2, row as u16);
        let s = screen(&mut v);
        let at = |x: &str| s.iter().position(|r| r.contains(x)).unwrap_or_else(|| panic!("{x}: {s:#?}"));
        let (ada, scan, docs, now_row, bob) = (at("ada · 2 subagents"), at("scan the repo · Explore · background"), at("read the docs"), at("↳ Read docs/a.md"), at("bob · 1 subagent"));
        assert!(ada < scan && scan < docs && docs < now_row && now_row < bob, "{s:#?}");
        let col = |r: &str, x: &str| r[..r.find(x).unwrap()].chars().count();
        assert!(col(&s[docs], "read") > col(&s[scan], "scan"), "nested: indented");
        assert!(s[bob - 1].trim_matches(|c| c == '│' || c == ' ').is_empty(), "a blank row between masters: {s:#?}");
        // ↓ ↓: bob's; d: asks; d again: stop it
        v.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        v.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        v.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(!v.ops.borrow().iter().any(|o| o["op"] == "sub_stop"), "1 d: 0 stop");

        v.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        let last = v.ops.borrow().last().cloned().unwrap();
        assert_eq!((last["op"].as_str(), last["id"].as_str()), (Some("sub_stop"), Some("t3")), "{last:?}");
        assert_eq!(v.input.text(), "", "d went to the box, not the prompt");
    }

    #[test]
    fn a_report_is_a_message_of_the_master_folded_and_a_lost_one_can_be_picked_up() {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new("subtest-0001", Path::new("/tmp"), "t", tx);
        v.members = vec!["ada".into()];
        v.thread.push(&ev("ada", 1_000, start("t1", None, "scan the repo", false)));
        v.thread.push(&ev("ada", 1_000, store::Kind::SubInfo { id: "t1".into(), transcript: "/x/agent-1.jsonl".into(), task: "1".into() }));
        let report: String = (1..=9).map(|i| format!("line {i}\n")).collect();
        v.thread.push(&ev("ada", 71_000, store::Kind::SubEnd { id: "t1".into(), state: "done".into(), result: report }));
        assert!(!v.thread.working("ada"));
        let s = screen(&mut v);
        let at = |s: &[String], x: &str| s.iter().position(|r| r.contains(x));
        assert!(at(&s, "↳ subagent “scan the repo” · done · 1m10s").is_some(), "{s:#?}");
        assert!(at(&s, "line 6").is_some() && at(&s, "line 7").is_none(), "folded after 6 lines: {s:#?}");
        let more = at(&s, "3 more lines").unwrap();
        assert!(!s.iter().any(|r| r.contains("subagents (")), "0 running: 0 row");
        click(&mut v, 6, more as u16);
        let s = screen(&mut v);
        assert!(at(&s, "line 9").is_some(), "opened: {s:#?}");
        // a lost one: the error and its pick up
        v.thread.push(&ev("ada", 80_000, start("t2", None, "fix the build", true)));
        v.thread.push(&ev("ada", 80_000, store::Kind::SubInfo { id: "t2".into(), transcript: "/x/agent-2.jsonl".into(), task: "2".into() }));
        v.thread.push(&ev("ada", 90_000, store::Kind::SubEnd { id: "t2".into(), state: "lost".into(), result: "the harness process of ada ended".into() }));
        v.dirty = true;
        let s = screen(&mut v);
        assert!(at(&s, "ended without a report: the harness process of ada ended").is_some(), "{s:#?}");
        let pick = at(&s, "pick up: ada goes on").unwrap();
        click(&mut v, 6, pick as u16);
        let last = v.ops.borrow().last().cloned().unwrap();
        let text = last["text"].as_str().unwrap_or_default();
        assert_eq!(last["to"], serde_json::json!(["ada"]));
        assert!(text.contains("“fix the build”") && text.contains("/x/agent-2.jsonl") && text.contains("> do fix the build"), "{text}");
    }
}

#[cfg(test)]
mod prompt_draft_tests {
    use super::*;

    #[test]
    fn the_unsent_prompt_and_unlogged_sends_survive_a_new_view() {
        let id = "drafttest-0000";
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new(id, Path::new("/tmp"), "t", tx.clone());
        v.input.set_text("half a message");
        v.save_prompt();
        // a send the host never logged (the process died before the reply)
        v.pending.push(("sent, never logged".into(), Instant::now(), None));
        v.save_prompt();
        drop(v);
        let mut v = View::new(id, Path::new("/tmp"), "t", tx.clone());
        assert_eq!(v.input.text(), "sent, never logged\n\nhalf a message");
        // an emptied prompt: 0 file
        v.input.set_text("");
        v.save_prompt();
        assert!(!prompt_path(id).exists());
        // a failed new-thread start: the task waits in the prompt
        save_prompt_draft(id, "the task");
        let v = View::new(id, Path::new("/tmp"), "t", tx);
        assert_eq!(v.input.text(), "the task");
        let _ = std::fs::remove_dir_all(store::thread_dir(id));
    }
}

/// The longest a shell-mode command runs.
const SHELL_TIMEOUT: Duration = Duration::from_secs(120);
/// The output of a shell-mode command the agents get (the thread shows up to 16 KB).
const SHELL_CTX_CHARS: usize = 8_000;
const SHELL_LOG_BYTES: usize = 16_000;

/// Run COMMAND in DIR with $SHELL -c (else sh): (stdout and stderr together, cut; exit code, -1:
/// killed or 0 start).
fn shell(dir: &Path, command: &str) -> (String, i32) {
    use std::io::Read;
    let sh = std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "sh".into());
    let Ok((mut reader, writer)) = std::io::pipe() else { return ("pipe failed".into(), -1) };
    let Ok(w2) = writer.try_clone() else { return ("pipe failed".into(), -1) };
    let child = std::process::Command::new(&sh).arg("-c").arg(command).current_dir(dir).stdin(std::process::Stdio::null()).stdout(writer).stderr(w2).spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return (format!("{sh}: {e}"), -1),
    };
    // the reader on its own thread: a full pipe never blocks the child
    let read = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = reader.by_ref().take(4 * SHELL_LOG_BYTES as u64).read_to_end(&mut buf);
        // the rest: drained, dropped
        let _ = std::io::copy(&mut reader, &mut std::io::sink());
        buf
    });
    let start = Instant::now();
    let code = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st.code().unwrap_or(-1),
            Ok(None) if start.elapsed() > SHELL_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                break -1;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break -1,
        }
    };
    let buf = read.join().unwrap_or_default();
    let text = String::from_utf8_lossy(&buf);
    // ANSI colors off: the plain text
    let text: String = crate::ansi::to_lines(&text).iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>()).collect::<Vec<_>>().join("\n");
    let mut out: String = text.chars().take(SHELL_LOG_BYTES).collect();
    if out.len() < text.len() {
        out.push_str("\n… (cut)");
    }
    if code == -1 && start.elapsed() > SHELL_TIMEOUT {
        out.push_str(&format!("\n(killed after {} s)", SHELL_TIMEOUT.as_secs()));
    }
    (out, code)
}

/// P with the home dir as `~`.
/// `cd DIR` alone (0 `;`, `&&`, `|`, `$`): DIR (empty: `cd` alone). None: another command.
fn cd_target(command: &str) -> Option<&str> {
    let c = command.trim();
    let rest = c.strip_prefix("cd").filter(|r| r.is_empty() || r.starts_with(char::is_whitespace))?.trim();
    // 1 quoted dir: spaces in it
    if let Some(q) = rest.strip_prefix('"').and_then(|r| r.strip_suffix('"')).or_else(|| rest.strip_prefix('\'').and_then(|r| r.strip_suffix('\''))) {
        return (!q.contains(['"', '\''])).then_some(q);
    }
    if rest.chars().any(|ch| matches!(ch, ';' | '&' | '|' | '$' | '`' | '>' | '<' | '(' | ')')) || rest.split_whitespace().count() > 1 {
        return None;
    }
    Some(rest.trim_matches(|ch| ch == '"' || ch == '\''))
}

/// The checkout of thread T with the most line changes: the uncommitted ones plus the commits
/// since the thread began. A tie, 0 changes: the work dir (a git repo first). None: 1 checkout.
fn busiest_checkout(t: &store::Thread) -> Option<PathBuf> {
    let all = t.all_checkouts();
    if all.len() < 2 {
        return None;
    }
    let since = format!("--since=@{}", t.created / 1000);
    let sum = |out: &str| out.lines().filter_map(|l| {
        let mut f = l.split_whitespace();
        Some(f.next()?.parse::<u64>().unwrap_or(0) + f.next()?.parse::<u64>().unwrap_or(0))
    }).sum::<u64>();
    let mut best: Option<(u64, bool, PathBuf)> = None;
    for c in all {
        let dir = PathBuf::from(&c.dir);
        let (git, _, _) = crate::threads::git(&dir, &["rev-parse", "--is-inside-work-tree"]);
        let lines = if git { sum(&crate::threads::git(&dir, &["diff", "HEAD", "--numstat"]).1) + sum(&crate::threads::git(&dir, &["log", &since, "--numstat", "--format="]).1) } else { 0 };
        if best.as_ref().is_none_or(|(l, g, _)| (lines, git) > (*l, *g)) {
            best = Some((lines, git, dir));
        }
    }
    best.map(|b| b.2)
}

/// The hint row's left part: its width, short of the keys on the right (0 overlap).
fn to_width(hint: &Rect, cwd: bool, hist: bool, shell: bool, empty: bool) -> u16 {
    let keys = hint_keys(cwd, hist, shell, empty);
    hint.width.saturating_sub((keys.chars().count() as u16 + 1).min(hint.width - hint.width / 2))
}

/// The keys at the right of the hint row.
fn hint_keys(cwd: bool, hist: bool, shell: bool, empty: bool) -> &'static str {
    if cwd {
        "tab: complete · ⏎ cd · esc / ↑: back "
    } else if hist {
        "↑↓ ctrl+r: pick · ⏎ take · esc close · type to filter "
    } else if shell {
        "⏎ run · tab complete · → suggestion · ↑↓ history · ctrl+r search · ↓ dir · output: to the agents with your next message "
    } else if empty {
        "! for shell mode · ⌘/ctrl+⏎ send · ↑↓ history "
    } else {
        "⌘/ctrl+⏎ send · ↑↓ history "
    }
}

fn tilde(p: &Path) -> String {
    let home = crate::config::home();
    match p.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.display().to_string(),
    }
}

/// ctrl+i / ⌘i: the IDE on the repo root from every pane; several repos: the picker.
#[cfg(test)]
mod ide_tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn draw(v: &mut View) -> String {
        let mut t = Terminal::new(TestBackend::new(120, 40)).unwrap();
        t.draw(|f| v.draw(f)).unwrap();
        let b = t.backend().buffer().clone();
        (0..40).map(|y| (0..120).map(|x| b[(x, y)].symbol().to_string()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn ctrl_i_opens_the_repo_root_and_several_repos_get_a_picker() {
        let _g = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!("codehead-ide-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (a, b) = (root.join("alpha"), root.join("beta"));
        for d in [&a, &b] {
            std::fs::create_dir_all(d.join("src")).unwrap();
            assert!(std::process::Command::new("git").args(["init", "-q"]).current_dir(d).status().unwrap().success());
        }
        // 1 repo, the work dir below its root: the IDE on the root, from the chat and the changes view
        let t = store::Thread::new("ide", "task", &a.join("src"), vec!["ada".into()]);
        std::fs::create_dir_all(store::thread_dir(&t.id)).unwrap();
        t.save().unwrap();
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new(&t.id, &a.join("src"), "ide", tx);
        v.input.set_text("draft");
        v.key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL));
        v.set_pane(Pane::Changes);
        v.prompt_focus = false;
        v.key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::SUPER));
        assert_eq!(v.ide_opened, [a.clone(), a.clone()]);
        assert!(v.ide_pick.is_none() && v.input.text() == "draft");
        // 2 repos: the picker above the repos line; ↓ enter: the 2nd
        let mut t = store::load(&t.id).unwrap();
        t.checkouts.push(store::Checkout { dir: b.display().to_string(), branch: None });
        t.save().unwrap();
        v.set_pane(Pane::Chat);
        v.key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL));
        assert_eq!(v.ide_pick, Some((0, 0)));
        let s = draw(&mut v);
        assert!(s.contains("open in IDE") && s.contains("alpha") && s.contains("beta"), "{s}");
        let pick = v.ide_rows.iter().map(|(r, _)| r.y).max().unwrap();
        assert!(pick < v.hint_rect.y, "above the repos line");
        v.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(v.ide_opened.last(), Some(&b));
        assert!(v.ide_pick.is_none());
        // a click on a row opens it; esc closes the picker (0 interrupt)
        v.key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL));
        let _ = draw(&mut v);
        let (r, i) = v.ide_rows[0];
        v.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: r.x + 2, row: r.y, modifiers: KeyModifiers::NONE });
        assert_eq!((v.ide_opened.last(), i), (Some(&a), 0));
        v.key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL));
        v.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(v.ide_pick.is_none() && v.last_esc.is_none());
        let _ = std::fs::remove_dir_all(store::thread_dir(&t.id));
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// Key fixes of the UX audit: a new ask never takes typed keys, ctrl+c / ctrl+y in the prompt,
/// alt+digit views, the flow gate's note, boxes that pass a key on.
#[cfg(test)]
mod key_fix_tests {
    use super::*;

    fn view(id: &str) -> View {
        let (tx, _rx) = mpsc::channel();
        View::new(id, Path::new("/tmp"), "t", tx)
    }

    fn key(v: &mut View, c: KeyCode, m: KeyModifiers) {
        v.key(KeyEvent::new(c, m));
    }

    fn typed(v: &mut View, s: &str) {
        for c in s.chars() {
            key(v, KeyCode::Char(c), KeyModifiers::NONE);
        }
    }

    /// An ask ID arrives (a permission request, or a question with options a / b).
    fn ask(v: &mut View, id: &str, question: bool) {
        let (tool, input) = if question {
            ("AskUserQuestion", json!({"questions": [{"question": "Which?", "header": "Q", "multiSelect": false, "options": [{"label": "a", "description": ""}, {"label": "b", "description": ""}]}]}))
        } else {
            ("Bash", json!({"command": "rm -rf build"}))
        };
        v.thread.push(&store::Event { ts: 1, agent: Some("ada".into()), kind: store::Kind::Ask { id: id.into(), tool: tool.into(), input } });
        v.sync_ask();
    }

    fn answers(v: &View) -> usize {
        v.ops.borrow().iter().filter(|o| o.get("allow").is_some() || o.get("answers").is_some() || o.get("label").is_some()).count()
    }

    #[test]
    fn a_new_ask_while_the_owner_types_waits_for_tab() {
        let mut v = view("keyfix-ask-0000");
        typed(&mut v, "half");
        ask(&mut v, "p1", false);
        assert!(!v.ask.focus, "text in the prompt: the ask waits");

        // space, 1, enter: the prompt's (enter: a new line), 0 answer (no surprise allow)
        typed(&mut v, " 1");
        key(&mut v, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(v.input.text(), "half 1\n");
        assert_eq!(answers(&v), 0, "{:?}", v.ops.borrow());

        // tab: the ask; enter: allow
        key(&mut v, KeyCode::Tab, KeyModifiers::NONE);
        assert!(v.ask.focus);

        key(&mut v, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(answers(&v), 1);
        assert_eq!(v.input.text(), "half 1\n", "the draft stays");
    }

    #[test]
    fn a_new_ask_takes_the_focus_on_an_empty_idle_prompt_only() {
        let mut v = view("keyfix-ask-0001");
        ask(&mut v, "p1", false);
        assert!(v.ask.focus, "empty prompt, 0 keys: the ask has the keys");

        // a key less than 1 s ago, empty prompt: the next ask waits
        v.thread.asks.clear();
        v.sync_ask();
        key(&mut v, KeyCode::PageUp, KeyModifiers::NONE);
        ask(&mut v, "p2", false);
        assert!(!v.ask.focus);
    }

    #[test]
    fn ctrl_w_in_other_edits_the_other_text_never_the_hidden_prompt() {
        let mut v = view("keyfix-ask-0002");
        ask(&mut v, "q1", true);
        assert!(v.ask.focus);

        typed(&mut v, "ab cd");
        key(&mut v, KeyCode::Char('w'), KeyModifiers::CONTROL);
        assert_eq!(v.input.text(), "", "the prompt never got a key");

        let a = v.thread.asks.first().cloned().unwrap();
        assert_eq!(v.ask.rows(&a)[2].label, "ab ");

        key(&mut v, KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert_eq!(v.ask.rows(&a)[2].label, "");
    }

    #[test]
    fn tab_completes_an_open_mention_while_an_ask_waits() {
        let mut v = view("keyfix-ask-0003");
        v.roster = vec![crate::agents::Agent { name: "ada".into(), ..Default::default() }];
        typed(&mut v, "hi @a");
        ask(&mut v, "p1", false);
        assert!(!v.ask.focus && v.mention_open());

        key(&mut v, KeyCode::Tab, KeyModifiers::NONE);
        assert!(!v.ask.focus, "tab: the dropdown's, not the ask's");
        assert_eq!(v.input.text(), "hi @ada ");
    }

    #[test]
    fn ctrl_c_clears_the_prompt_and_ctrl_z_brings_it_back() {
        let mut v = view("keyfix-ctrlc-0000");
        typed(&mut v, "a long draft");
        key(&mut v, KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(v.input.text(), "");
        assert!(v.last_ctrl_c.is_none(), "a clear is 0 step to quit");

        key(&mut v, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(v.input.text(), "a long draft");
    }

    #[test]
    fn ctrl_y_redoes_in_the_prompt_0_yazi() {
        let mut v = view("keyfix-ctrly-0000");
        typed(&mut v, "ab");
        key(&mut v, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_ne!(v.input.text(), "ab");

        key(&mut v, KeyCode::Char('y'), KeyModifiers::CONTROL);
        assert_eq!(v.input.text(), "ab");
        assert!(v.yazi.is_none());
    }

    #[test]
    fn views_switch_on_f_keys_ctrl_and_alt_digits() {
        let mut v = view("keyfix-pane-0000");
        typed(&mut v, "draft");
        key(&mut v, KeyCode::F(2), KeyModifiers::NONE);
        assert_eq!(v.pane, Pane::Changes, "F2: changes (macOS without option-as-alt)");

        key(&mut v, KeyCode::Char('1'), KeyModifiers::CONTROL);
        assert_eq!(v.pane, Pane::Chat, "ctrl+1: chat");

        key(&mut v, KeyCode::Char('4'), KeyModifiers::ALT);
        assert_eq!(v.pane, Pane::Files);

        key(&mut v, KeyCode::F(1), KeyModifiers::NONE);
        assert_eq!(v.pane, Pane::Chat);
        assert_eq!(v.input.text(), "draft", "view keys type 0 chars into the prompt");

        // a plain digit: text, 0 view switch
        key(&mut v, KeyCode::Char('2'), KeyModifiers::NONE);
        assert_eq!(v.pane, Pane::Chat);
    }

    #[test]
    fn a_flow_gate_waits_in_the_picker_behind_the_agents_asks() {
        let mut v = view("keyfix-gate-0000");
        typed(&mut v, "looks good");
        v.flowbars = vec![crate::flowbar::FlowBar::new("ada")];
        v.flowbars[0].state = Some(crate::engine::flows::run::FlowState { flow: "chore".into(), wait: Some(crate::engine::flows::run::Wait::Gate), ..Default::default() });
        v.sync_flow_ask();
        assert_eq!(v.thread.asks.len(), 1);
        assert!(!v.ask.focus, "the owner types: the gate waits for tab");

        // an agent's ask comes: it goes first, the gate stays last
        ask(&mut v, "p1", false);
        v.sync_flow_ask();
        assert_eq!(v.thread.asks.first().map(|a| a.id.as_str()), Some("p1"));
        assert_eq!(v.thread.asks.last().map(|a| a.tool.as_str()), Some(crate::flowbar::WAIT_TOOL));

        // the agent's ask answered (its event): the gate; tab, 1: approve with the note
        v.thread.asks.remove(0);
        v.sync_ask();
        key(&mut v, KeyCode::Tab, KeyModifiers::NONE);
        key(&mut v, KeyCode::Char('1'), KeyModifiers::NONE);
        let op = v.ops.borrow().iter().find(|o| o["op"] == "flow_answer").cloned().expect("flow_answer");
        assert_eq!((op["choice"].as_str(), op["note"].as_str()), (Some("approve"), Some("looks good")));
        assert!(v.thread.asks.is_empty());

        key(&mut v, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(v.input.text(), "looks good", "the note: back with ctrl+z");
    }
}

/// ctrl+f in the chat: the search bar above the prompt, its matches, the prompt's draft kept.
#[cfg(test)]
mod chat_search_tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn view(id: &str) -> View {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new(id, Path::new("/tmp"), "t", tx);
        for i in 0..40u64 {
            let text = if i % 10 == 3 { format!("message {i}: the needle is here") } else { format!("message {i}: filler text") };
            v.thread.push(&store::Event { ts: i + 1, agent: Some("ada".into()), kind: store::Kind::Text { text } });
        }
        v.dirty = true;
        v
    }

    fn key(v: &mut View, c: KeyCode, m: KeyModifiers) {
        v.key(KeyEvent::new(c, m));
    }

    fn typed(v: &mut View, s: &str) {
        for c in s.chars() {
            key(v, KeyCode::Char(c), KeyModifiers::NONE);
        }
    }

    fn screen(v: &mut View) -> Vec<String> {
        let mut t = Terminal::new(TestBackend::new(100, 30)).unwrap();
        t.draw(|f| v.draw(f)).unwrap();
        let b = t.backend().buffer().clone();
        (0..30).map(|y| (0..100).map(|x| b[(x, y)].symbol().to_string()).collect::<String>()).collect()
    }

    #[test]
    fn ctrl_f_searches_the_chat_tab_cycles_esc_closes_the_draft_stays() {
        let mut v = view("chatsearch-0000");
        typed(&mut v, "draft");
        screen(&mut v);
        key(&mut v, KeyCode::Char('f'), KeyModifiers::CONTROL);
        assert!(v.find.open && v.find.focus);

        typed(&mut v, "needle");
        assert_eq!(v.input.text(), "draft", "the query goes to the bar, not to the prompt");
        let s = screen(&mut v);
        let bar = s.iter().position(|l| l.contains(" find ")).unwrap_or_else(|| panic!("{s:#?}"));
        println!("--- chat search\n{}", s.join("\n"));
        assert!(s[bar].contains("needle") && s[bar].contains("4/4"), "{}", s[bar]);
        assert!(s.iter().any(|l| l.contains("message 33: the needle")), "the newest match in view: {s:#?}");

        // tab: the next older match, scrolled into view with the marker
        key(&mut v, KeyCode::Tab, KeyModifiers::NONE);
        let s = screen(&mut v);
        assert!(s.iter().any(|l| l.contains("message 23: the needle")), "{s:#?}");
        assert!(s.iter().any(|l| l.contains("3/4")), "{s:#?}");
        assert!(s.iter().any(|l| l.starts_with('▌') && l.contains("message 23")), "the marker: {s:#?}");

        // shift+tab: newer again; wraps past the newest to the oldest
        key(&mut v, KeyCode::BackTab, KeyModifiers::SHIFT);
        key(&mut v, KeyCode::BackTab, KeyModifiers::SHIFT);
        let s = screen(&mut v);
        assert!(s.iter().any(|l| l.contains("1/4")) && s.iter().any(|l| l.contains("message 3: the needle")), "{s:#?}");

        // esc: the bar goes, the stream stays, the keys go to the prompt
        let back = v.back;
        key(&mut v, KeyCode::Esc, KeyModifiers::NONE);
        assert!(!v.find.open && v.back == back);
        let s = screen(&mut v);
        assert!(!s.iter().any(|l| l.contains(" find ")), "{s:#?}");
        typed(&mut v, "!");
        assert_eq!(v.input.text(), "draft!");
    }

    #[test]
    fn the_footer_shows_the_mode_as_a_badge() {
        let mut v = view("chatsearch-0003");
        let last = |s: &[String]| s.last().cloned().unwrap_or_default();
        assert!(!last(&screen(&mut v)).contains(" SHELL "));
        typed(&mut v, "!ls");
        let s = screen(&mut v);
        assert!(last(&s).trim_start().starts_with("SHELL"), "{}", last(&s));
        v.input.set_text("");
        key(&mut v, KeyCode::Char('f'), KeyModifiers::CONTROL);
        let s = screen(&mut v);
        assert!(last(&s).trim_start().starts_with("SEARCH"), "{}", last(&s));
        key(&mut v, KeyCode::Esc, KeyModifiers::NONE);
        assert!(!last(&screen(&mut v)).contains("SEARCH"));
    }

    #[test]
    fn a_query_with_0_matches_says_so_and_keeps_the_stream() {
        let mut v = view("chatsearch-0001");
        screen(&mut v);
        key(&mut v, KeyCode::Char('f'), KeyModifiers::CONTROL);
        typed(&mut v, "zzqq");
        let s = screen(&mut v);
        assert!(s.iter().any(|l| l.contains("0 matches")), "{s:#?}");
        assert_eq!(v.back, 0);
    }

    #[test]
    fn ctrl_f_in_another_view_is_its_filter_not_the_chat_search() {
        let mut v = view("chatsearch-0002");
        key(&mut v, KeyCode::F(4), KeyModifiers::NONE);
        assert_eq!(v.pane, Pane::Files);
        key(&mut v, KeyCode::Char('f'), KeyModifiers::CONTROL);
        assert!(v.files.typing() && !v.find.open);
    }
}

/// ctrl+q: the agents box above the prompt has the keys: ←→ an agent, its menu above it (flow,
/// remove, replace with); ctrl+m and esc give the keys back to the prompt (its draft kept).
#[cfg(test)]
mod agents_box_tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn view(id: &str) -> View {
        let (tx, _rx) = mpsc::channel();
        let mut v = View::new(id, Path::new("/tmp"), "t", tx);
        v.members = vec!["ada".into(), "zed".into()];
        v.roster = ["ada", "zed", "nova"].iter().map(|n| crate::agents::Agent { name: (*n).into(), harness: "claude".into(), ..Default::default() }).collect();
        v.sync_order();
        v.spawns.clear();
        v.sync_flowbars();
        v
    }

    fn key(v: &mut View, c: KeyCode, m: KeyModifiers) {
        v.key(KeyEvent::new(c, m));
    }

    fn screen(v: &mut View) -> String {
        let mut t = Terminal::new(TestBackend::new(110, 34)).unwrap();
        t.draw(|f| v.draw(f)).unwrap();
        let b = t.backend().buffer().clone();
        (0..34).map(|y| (0..110).map(|x| b[(x, y)].symbol().to_string()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn ctrl_q_highlights_an_agent_and_its_menu_opens_above_it() {
        let mut v = view("agentsbox-0000");
        v.input.set_text("draft");
        key(&mut v, KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert_eq!(v.agents_focus.as_ref().map(|f| f.at), Some(0), "the lead first");
        let s = screen(&mut v);
        println!("--- agents box\n{s}");
        assert!(s.contains("@ada") && s.contains("flow") && s.contains("remove") && s.contains("replace with") && s.contains("0 flow: pick one"), "{s}");

        // →: the next agent; letters and digits never reach the prompt
        key(&mut v, KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(v.agents_focus.as_ref().map(|f| f.at), Some(1));
        key(&mut v, KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(v.input.text(), "draft");
        assert!(screen(&mut v).contains("@zed"));

        // ctrl+m: the prompt again
        key(&mut v, KeyCode::Char('m'), KeyModifiers::CONTROL);
        assert!(v.agents_focus.is_none());
        key(&mut v, KeyCode::Char('!'), KeyModifiers::NONE);
        assert_eq!(v.input.text(), "draft!");
    }

    #[test]
    fn flow_opens_that_agents_flow_picker_and_sets_it_for_that_agent() {
        let mut v = view("agentsbox-0001");
        key(&mut v, KeyCode::Char('q'), KeyModifiers::CONTROL);
        key(&mut v, KeyCode::Right, KeyModifiers::NONE);
        key(&mut v, KeyCode::Enter, KeyModifiers::NONE);
        assert!(v.agents_focus.is_none(), "the flow box has the keys");
        let s = screen(&mut v);
        assert!(s.contains("pick a flow for @zed"), "{s}");
        // a flow, then its start step: flow_set for zed
        for c in "chore".chars() {
            key(&mut v, KeyCode::Char(c), KeyModifiers::NONE);
        }
        key(&mut v, KeyCode::Enter, KeyModifiers::NONE);
        key(&mut v, KeyCode::Enter, KeyModifiers::NONE);
        let op = v.ops.borrow().iter().find(|o| o["op"] == "flow_set").cloned().unwrap();
        assert_eq!((op["flow"].as_str(), op["agent"].as_str()), (Some("chore"), Some("zed")));
    }

    #[test]
    fn remove_asks_a_2nd_enter_and_replace_picks_an_agent_off_the_thread() {
        let mut v = view("agentsbox-0002");
        key(&mut v, KeyCode::Char('q'), KeyModifiers::CONTROL);
        key(&mut v, KeyCode::Right, KeyModifiers::NONE);
        key(&mut v, KeyCode::Char('2'), KeyModifiers::NONE);
        assert!(screen(&mut v).contains("enter again: remove @zed"));
        assert!(!v.ops.borrow().iter().any(|o| o["op"] == "leave"));
        key(&mut v, KeyCode::Enter, KeyModifiers::NONE);
        let op = v.ops.borrow().iter().find(|o| o["op"] == "leave").cloned().unwrap();
        assert_eq!(op["agent"].as_str(), Some("zed"));
        assert!(v.agents_focus.is_none());

        // replace with: the agents 0 on the thread (nova)
        key(&mut v, KeyCode::Char('q'), KeyModifiers::CONTROL);
        key(&mut v, KeyCode::Char('3'), KeyModifiers::NONE);
        let s = screen(&mut v);
        assert!(s.contains("replace @ada with") && s.contains("nova") && !s.contains("│ zed"), "{s}");
        key(&mut v, KeyCode::Esc, KeyModifiers::NONE);
        assert!(v.agents_focus.as_ref().is_some_and(|f| f.replace.is_none()), "esc: back to the menu");
        key(&mut v, KeyCode::Esc, KeyModifiers::NONE);
        assert!(v.agents_focus.is_none(), "esc: the prompt");
    }

    /// ctrl+p: per agent of the thread 3 rows (flow, remove, replace with), the agents box's actions.
    #[test]
    fn the_menu_has_the_agents_box_actions_per_agent() {
        let mut v = view("agentsbox-0004");
        v.open_menu();
        let mut t = Terminal::new(TestBackend::new(110, 50)).unwrap();
        t.draw(|f| v.draw(f)).unwrap();
        let b = t.backend().buffer().clone();
        let s: String = (0..50).map(|y| (0..110).map(|x| b[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect();
        assert!(s.contains("@zed: flow (0 flow)") && s.contains("@ada: remove"), "{s}");
        // zed's flow row: its flow box on the flows
        v.menu_action(crate::menu::Action::Command(AGENT_ITEMS + 3));
        assert!(v.menu.is_none() && v.flowbars.iter().find(|b| b.agent == "zed").is_some_and(|b| b.is_open()));
        // zed's remove row: the agents box on zed, remove armed (enter removes)
        v.menu_action(crate::menu::Action::Command(AGENT_ITEMS + 4));
        assert!(v.agents_focus.as_ref().is_some_and(|f| f.at == 1 && f.cur == 1 && f.armed));
        key(&mut v, KeyCode::Enter, KeyModifiers::NONE);
        assert!(v.ops.borrow().iter().any(|o| o["op"] == "leave" && o["agent"] == "zed"));
        // ada's replace row: the replace picker
        v.menu_action(crate::menu::Action::Command(AGENT_ITEMS + 2));
        assert!(v.agents_focus.as_ref().is_some_and(|f| f.at == 0 && f.replace.is_some()));
    }

    /// reply on a gate of a non-lead agent: the message goes to that agent, the gate stays.
    #[test]
    fn reply_on_a_gate_goes_to_the_waiting_agent() {
        let mut v = view("agentsbox-0005");
        v.flowbars[1].state = Some(crate::engine::flows::run::FlowState { flow: "chore".into(), title: "chore".into(), wait: Some(crate::engine::flows::run::Wait::Gate), ..Default::default() });
        v.sync_flow_ask();
        v.input.set_text("also cover X");
        // chore's gate: approve (1), reply (2)
        key(&mut v, KeyCode::Tab, KeyModifiers::NONE);
        key(&mut v, KeyCode::Char('2'), KeyModifiers::NONE);
        let op = v.ops.borrow().iter().find(|o| o["op"] == "prompt").cloned().unwrap();
        assert_eq!(op["to"], json!(["zed"]), "{op}");
        assert!(!v.ops.borrow().iter().any(|o| o["op"] == "flow_answer"));
        assert!(v.reply_to.is_none());
        // an @name in the reply: that agent too, the waiting agent first
        v.input.set_text("check with @ada first");
        key(&mut v, KeyCode::Tab, KeyModifiers::NONE);
        key(&mut v, KeyCode::Char('2'), KeyModifiers::NONE);
        let op = v.ops.borrow().iter().rev().find(|o| o["op"] == "prompt").cloned().unwrap();
        assert_eq!(op["to"], json!(["zed", "ada"]), "{op}");
    }

    /// ctrl+f, then another view: the view gets the keys, 0 hidden search bar takes them.
    #[test]
    fn a_view_switch_takes_the_keys_from_the_search_bar() {
        let mut v = view("agentsbox-0006");
        key(&mut v, KeyCode::Char('f'), KeyModifiers::CONTROL);
        key(&mut v, KeyCode::Char('x'), KeyModifiers::NONE);
        key(&mut v, KeyCode::F(4), KeyModifiers::NONE);
        assert!(v.pane == Pane::Files && !v.find.focus);
        key(&mut v, KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(v.find.ta.text(), "x", "j went to the files view");
    }

    /// The menu's agent rows keep their agent when the order changes before the pick.
    #[test]
    fn menu_agent_rows_keep_their_agent_after_a_join() {
        let mut v = view("agentsbox-0007");
        v.open_menu();
        // nova joins at the front while the menu is open
        v.members.insert(0, "nova".into());
        v.order.insert(0, "nova".into());
        v.menu_action(crate::menu::Action::Command(AGENT_ITEMS + 4));
        let fo = v.agents_focus.clone().unwrap();
        assert_eq!(v.order[fo.at], "zed", "row 2 of the menu: zed, not the new 2nd");
    }

    #[test]
    fn ctrl_m_gives_the_prompt_back_from_the_search_bar_and_another_view() {
        let mut v = view("agentsbox-0003");
        key(&mut v, KeyCode::Char('f'), KeyModifiers::CONTROL);
        assert!(v.find.focus);
        key(&mut v, KeyCode::Char('m'), KeyModifiers::CONTROL);
        assert!(!v.find.focus && v.find.open, "the bar stays, the prompt has the keys");
        key(&mut v, KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(v.input.text(), "a");
        key(&mut v, KeyCode::F(4), KeyModifiers::NONE);
        assert!(!v.prompt_focus);
        key(&mut v, KeyCode::Char('m'), KeyModifiers::CONTROL);
        assert!(v.prompt_focus);
    }
}
