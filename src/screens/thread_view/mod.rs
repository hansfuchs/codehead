//! Thread view (hell's layout): the stream on top (every message, tool run and open question of
//! the thread, from its event log), the title row, the header (agents box + tokens box in 1
//! frame), the input bubble (its top border the header's bottom line), the hint row, the footer.
//! Messages go to the thread's host (`engine::client`); the host starts on the first one.

pub(crate) mod workspot;

mod agents;
mod agents_menu;
mod agents_state;
mod branch;
mod checkout_state;
mod context_menu;
mod cost_box_state;
mod draw;
mod draw_activity;
mod draw_bar;
mod draw_bounds;
mod draw_footer;
mod draw_header_cost;
mod draw_hints;
mod draw_layout;
mod draw_overlays;
mod draw_pane;
mod draw_prompt;
use draw_layout::{ActivityLayout, PromptLayout};
mod editor;
mod events;
mod flow;
mod focus;
mod header;
mod header_labels;
mod ide_state;
mod initialize;
mod layout_state;
mod merge_state;
mod mini_chat_state;
mod panes_state;
mod prompt_state;
mod selection_state;
mod shell_state;
mod stream_state;
mod urls_state;
use agents_state::AgentState;
use checkout_state::CheckoutState;
use cost_box_state::CostBox;
use ide_state::IdeState;
use layout_state::LayoutState;
use merge_state::MergeState;
use mini_chat_state::MiniChatState;
use panes_state::PaneState;
use prompt_state::PromptState;
use selection_state::SelectionState;
use shell_state::ShellState;
use stream_state::StreamState;
use urls_state::UrlState;
mod keys;
mod keys_activity;
mod keys_controls;
mod keys_copy;
mod keys_escape;
mod keys_global;
mod keys_history;
mod keys_panes;
mod keys_prompt;
mod keys_scroll;
mod keys_search;
mod mouse;
mod mouse_chrome;
mod mouse_click;
mod mouse_overlays;
mod mouse_stream;
mod outside;
mod panes;
mod prompt_draft;
mod prompt_submit;
mod quick_menu;
mod runtime;
mod runtime_frame;
mod runtime_messages;
mod runtime_refresh;
use runtime::Runtime;
mod selection;
mod shell;
mod shell_command;
mod stream;
mod stream_cache;
mod stream_geometry;
mod stream_overlays;
mod stream_pending;
mod stream_rows;
mod stream_selection;
mod subscription;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use serde_json::{Value, json};

use crate::engine::{client, store};
use crate::features::chat::{self, Hit};
use crate::features::prompt_input::attach::{Pasted, Paster};
use crate::ui::mention::Mention;
use crate::ui::textarea::{Outcome, TextArea};
use crate::ui::theme;

use header_labels::{
    cards_w, effort_label, header_rows, hint_keys, home_label, input_width, tilde, title_case, to_width,
};
use outside::{open_url, run_outside, suspended};
#[cfg(test)]
pub(crate) use runtime::open_1st_frame;
#[cfg(test)]
use runtime::to_main;
pub(crate) use runtime::{run, run_with};
use shell_command::{busiest_checkout, cd_target, git_push, shell};
use stream_geometry::{back_for, content_cols, item_ts, top_anchor, word_at};
use subscription::subscribe;

pub(crate) enum Exit {
    /// ctrl+c twice: codehead closes.
    Quit,
    /// Menu: a tab of the start screen.
    Home(crate::screens::quick_menu::Home),
    /// Esc esc: back to the start screen.
    Back,
    /// Another thread's view (menu, `> branch`).
    Open {
        id: String,
        dir: PathBuf,
        title: String,
        note: String,
    },
}

/// A PR for the merge tab of the next view of a thread: (thread, PR).
static OPEN_PR: std::sync::Mutex<Option<(String, crate::infra::gh::PrRef)>> = std::sync::Mutex::new(None);

/// The next view of thread ID opens on its merge tab, PR in front.
pub(crate) fn open_pr(id: &str, pr: &crate::infra::gh::PrRef) {
    *OPEN_PR.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some((id.to_string(), pr.clone()));
}

fn take_pr(id: &str) -> Option<crate::infra::gh::PrRef> {
    let mut open = OPEN_PR.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

    if open.as_ref().is_some_and(|(thread, _)| thread == id) {
        open.take().map(|(_, pr)| pr)
    } else {
        None
    }
}

/// Views of a thread: the view bar on row 0 (F1-F4, ctrl+1-4, alt+1-4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Pane {
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
/// The expanded tokens box (borders incl.) at least: room for the agents table.
const EXPANDED_WIDTH: u16 = 66;
/// Header: the sprite rows + its top border at least, max. this many rows.
const HEADER_H: u16 = crate::ui::sprites::HEIGHT as u16 + 1;
const MAX_HEADER_H: u16 = 12;
/// Below this height: 0 header.
const MIN_HEADER_SCREEN: u16 = 24;
/// Handoff to a replacing agent: the replaced agent's last messages, max. this many chars.
const HANDOFF_CHARS: usize = 12_000;
const WAVE: [&str; 4] = ["░▒▓", "▒▓▒", "▓▒░", "▒░▒"];

enum Msg {
    /// A command to the host ended: what it was, Err(why).
    Done(&'static str, Result<Value, String>),
    Cost(Box<crate::features::cost::Snap>),
    /// Every thread's cost (the expanded tokens box).
    AllThreads(Box<crate::features::cost::AllThreads>),
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
    /// A 1-off action off the UI thread ended: its toast.
    Toast(String),
}

/// tab: the agents box above the prompt has the keys. AT: the highlighted agent (index in
/// the box order), CUR: its menu row (flow, remove, replace with), ARMED: remove asks a 2nd enter,
/// REPLACE: the agents to replace it with (cursor, 1st row in view).
#[derive(Clone, Debug, Default, PartialEq)]
struct AgentsFocus {
    at: usize,
    cur: usize,
    armed: bool,
    replace: Option<(usize, usize)>,
}

/// A region of the thread view: tab / shift+tab walk them (the focus ring), arrows move inside.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Region {
    Prompt,
    /// The changes, merge or files view.
    View,
    /// The source control panel of the changes view.
    Panel,
    /// The repo tabs of the changes view (2+ checkouts).
    Repos,
    /// The picker of an open question.
    Ask,
    /// The agents box above the prompt.
    Agents,
}

/// A row of the context menu: a key for the region in focus, a quick command of the menu
/// (its id), the shell history search.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ctx {
    Key(KeyEvent),
    /// A command of the view in focus with 0 key of its own.
    Cmd(&'static str),
    /// A ship step of the changes panel (its index).
    Step(usize),
    Menu(usize),
    Hist,
}

/// A view's context menu action as the thread view's.
fn ctx_of(action: &crate::ui::context_menu::Act) -> Ctx {
    match action {
        crate::ui::context_menu::Act::Key(key) => Ctx::Key(*key),
        crate::ui::context_menu::Act::Cmd(command) => Ctx::Cmd(command),
        crate::ui::context_menu::Act::Step(i) => Ctx::Step(*i),
    }
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
    thread: chat::Thread,

    /// The 1st open ask: cursor, picks, answers so far, focus.
    ask: crate::features::chat::ask::Picker,
    /// Tests: the host commands, recorded (0 host process).
    #[cfg(test)]
    ops: std::cell::RefCell<Vec<Value>>,

    /// The notifications dropdown (ctrl+n); the seen tracker of this thread's stream.
    notes_panel: Option<crate::features::notifications::Panel>,
    watch: crate::features::notifications::Watch,

    /// `> handoff`: the document on its way.
    handing: Option<Receiver<Result<String, String>>>,
    /// `>branch`: the copy of the thread (its log: up to MBs) off the UI thread; its end opens it.
    forking: Option<Receiver<Result<store::Thread, String>>>,

    monitors: crate::features::activity_rows::monitors::Monitors,
    subagents: crate::features::activity_rows::subagents::Subagents,
    /// The running sub-threads of this thread (`>sub`): their row, their box.
    subthreads: crate::features::activity_rows::subthreads::SubThreads,
    /// The flow rows + their boxes, 1 per agent of the thread (members' order; `> flow`, the
    /// agents box, a click).
    flowbars: Vec<crate::features::flows::flow_bar::FlowBar>,
    /// ctrl+f in the chat: the search bar above the prompt.
    find: crate::features::chat::chat_search::ChatSearch,
    /// The search match to scroll to at the next draw (item index).
    find_to: Option<usize>,
    menu: Option<crate::screens::quick_menu::Menu>,
    /// The context menu (ctrl+.): the actions of the region in focus.
    ctx: Option<crate::ui::context_menu::Menu<Ctx>>,
    cost_box: CostBox,
    toast: Option<(String, Instant)>,
    started: Instant,
    last_esc: Option<Instant>,
    /// A 1st Esc armed an interrupt: it fires after `DOUBLE_ESC` unless a 2nd Esc means back.
    esc_interrupt: Option<Instant>,
    last_ctrl_c: Option<Instant>,
    /// The last key press: a new ask takes the focus only after 1 s of 0 keys (and an empty prompt).
    last_key: Option<Instant>,
    exit: Exit,
    /// A menu click switched threads: the view ends with EXIT.
    mouse_exit: bool,

    tx: crate::infra::bus::Tx<Msg>,

    agents: AgentState,
    stream: StreamState,
    selection: SelectionState,
    prompt: PromptState,
    shell_state: ShellState,
    checkout_state: CheckoutState,
    panes: PaneState,
    layout: LayoutState,
    ide: IdeState,
    merge_state: MergeState,
    urls_state: UrlState,
    mini_chat_state: MiniChatState,
}

/// Header card: sprite, color, the lines right of the sprite, width.
struct Card {
    sprite: crate::domain::sprites::Sprite,
    color: Color,
    side: Vec<Vec<Span<'static>>>,
    width: u16,
}

/// What a hook of `run_with` sees each loop: the open pane, the menu open, an IDE open asked
/// (the tutorial's mock).
pub(crate) struct Peek {
    pub pane: Pane,
    pub menu: bool,
    pub ide: bool,
}

#[cfg(test)]
#[path = "thread_view_scroll_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "thread_view_ask_tests.rs"]
mod ask_tests;

#[cfg(test)]
#[path = "thread_view_pane_tests.rs"]
mod pane_tests;

#[cfg(test)]
#[path = "thread_view_comment_tests.rs"]
mod comment_tests;

#[cfg(test)]
#[path = "thread_view_default_to_tests.rs"]
mod default_to_tests;

#[cfg(test)]
#[path = "thread_view_files_pane_tests.rs"]
mod files_pane_tests;

#[cfg(test)]
#[path = "thread_view_split_tests.rs"]
mod split_tests;

#[cfg(test)]
#[path = "thread_view_select_tests.rs"]
mod select_tests;

#[cfg(test)]
#[path = "thread_view_section_tests.rs"]
mod section_tests;

#[cfg(test)]
#[path = "thread_view_subagent_tests.rs"]
mod subagent_tests;

#[cfg(test)]
#[path = "thread_view_prompt_draft_tests.rs"]
mod prompt_draft_tests;

/// The longest a shell-mode command runs.
const SHELL_TIMEOUT: Duration = Duration::from_secs(120);
/// The output of a shell-mode command the agents get (the thread shows up to 16 KB).
const SHELL_CTX_CHARS: usize = 8_000;
const SHELL_LOG_BYTES: usize = 16_000;

#[cfg(test)]
#[path = "thread_view_ide_tests.rs"]
mod ide_tests;

#[cfg(test)]
#[path = "thread_view_key_fix_tests.rs"]
mod key_fix_tests;

#[cfg(test)]
#[path = "thread_view_chat_search_tests.rs"]
mod chat_search_tests;

#[cfg(test)]
#[path = "thread_view_agents_box_tests.rs"]
mod agents_box_tests;

#[cfg(test)]
#[path = "thread_view_git_push_tests.rs"]
mod git_push_tests;

#[cfg(test)]
#[path = "thread_view_repo_row_tests.rs"]
mod repo_row_tests;

#[cfg(test)]
#[path = "thread_view_checkout_tests.rs"]
mod checkout_tests;

#[cfg(test)]
#[path = "thread_view_flow_gate_tests.rs"]
mod flow_gate_tests;

use crate::domain::drafts::prompt_path;
#[cfg(test)]
use crate::domain::drafts::save_prompt_draft;
