//! A flow row of the thread view (above the subagents row), 1 per agent with a flow: the
//! agent's flow, its current step, a wait (gate, loop max), a pending / queued switch, warnings.
//! A click (or the agents box: flow) opens its box:
//! - steps: the rail (marks, arcs), enter: the step's text; s skip, j jump, c change the flow,
//!   e edit it in the flows screen, d d removes it.
//! - change flow: the flows (typed text filters) → `switch now` / `after current flow` / cancel
//!   (asked at once in the same box: 0 frame where the chat waits on a hidden choice) → the start
//!   step. `now`: the host stops the running turn at its next tool boundary.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use serde_json::{Value, json};

use crate::engine::flows::run::{FlowState, Wait};
use crate::engine::flows::{self, Library, Resolved};
use crate::features::flows::flow_rail;
use crate::ui::theme;

/// What a key asks the view to do.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Act {
    None,
    /// A host command (op, command).
    Host(&'static str, Value),
    /// Answer the wait with CHOICE: the prompt's text goes along as the note.
    Answer(String),
    /// Open the flows screen on flow SLUG.
    Edit(String),
}

#[derive(Clone, Debug, PartialEq)]
enum Mode {
    Steps,
    /// The flows to switch to; typed filter.
    Pick(String),
    /// A flow runs: now, after, cancel.
    How(String),
    /// The start step of the new flow (MODE `now` / `set`).
    Start(String),
}

pub(crate) struct FlowBar {
    /// The agent the flow belongs to.
    pub agent: String,
    /// enter on a step: its text below the rail.
    detail: bool,
    pub state: Option<FlowState>,
    nodes: Vec<Resolved>,
    name: String,
    dir: PathBuf,
    lib: Library,
    warnings: Vec<(usize, String)>,
    updated: u64,
    read_at: Option<Instant>,
    lib_at: Option<Instant>,
    popup_state: PopupStateState,
}

impl Default for FlowBar {
    fn default() -> Self {
        Self {
            agent: String::new(),
            detail: false,
            state: None,
            nodes: Vec::new(),
            name: String::new(),
            dir: PathBuf::new(),
            lib: Library::default(),
            warnings: Vec::new(),
            updated: 0,
            read_at: None,
            lib_at: None,
            popup_state: PopupStateState {
                open: false,
                del_armed: false,
                mode: Mode::Steps,
                cur: 0,
                offset: 0,
                bar: Rect::default(),
                popup: Rect::default(),
                rows: Vec::new(),
                hover: false,
            },
        }
    }
}

/// Choices of wait W at node N: (choice sent, label).
fn choices(width: &Wait, count: Option<&Resolved>) -> Vec<(String, String)> {
    match width {
        Wait::Gate => choices_wait_gate(count),
        Wait::LoopMax => vec![
            ("again".into(), "1 more round".into()),
            ("next".into(), "move on".into()),
        ],
    }
}

fn choices_wait_gate(count: Option<&Resolved>) -> Vec<(String, String)> {
    let mut value = vec![("approve".to_string(), "approve · go on".to_string())];

    if let Some(count_value) = count {
        for (key, _) in &count_value.edges {
            value.push((key.clone(), key.clone()));
        }
    }

    value
}

impl FlowBar {
    /// The flow row of AGENT.
    pub(crate) fn new(agent: &str) -> FlowBar {
        FlowBar {
            agent: agent.to_string(),
            ..Default::default()
        }
    }

    /// The flow waits at a gate.
    pub(crate) fn waits_gate(&self) -> bool {
        self.state.as_ref().is_some_and(|text| text.wait == Some(Wait::Gate))
    }

    /// The flow's wait as an ask for the question picker under the stream: a gate (approve or an
    /// outcome), a loop at its max (1 more round, move on). Its choices: `_options` (id, name); the prompt's text goes along as the note.
    /// None: 0 wait.
    pub(crate) fn wait_ask(&self) -> Option<crate::domain::chat::Ask> {
        wait_ask_flow_bar(self)
    }

    /// The answer OPTION (an id of `wait_ask`): the flow's step to send, the wait gone here.
    pub(crate) fn wait_answer(&mut self, option: &str) -> Option<Act> {
        let st = self.state.as_mut()?;

        st.wait.as_ref()?;
        st.wait = None;
        Some(Act::Answer(option.to_string()))
    }

    /// The agent's flow of thread T (read by the caller: 1 read for every row). FORCE: also when
    /// T is not newer. True: the flow changed.
    pub(crate) fn sync(&mut self, thread: &crate::engine::store::Thread, force: bool) -> bool {
        if thread.updated == self.updated && !force {
            return false;
        }

        self.updated = thread.updated;
        let flow = thread.flow_of(&self.agent);
        let changed = self.state != flow;

        self.state = flow;
        self.dir = thread.work_dir();
        reload_lib(force, self);

        changed
    }

    pub(crate) fn close(&mut self) {
        self.popup_state.open = false;
    }

    pub(crate) fn is_open(&self) -> bool {
        self.popup_state.open
    }

    /// Open on the steps (the current one under the cursor), or on the wait's choices.
    pub(crate) fn open(&mut self) {
        self.popup_state.open = true;
        self.detail = false;
        self.lib_at = None;
        let dir = self.dir.clone();

        self.lib = Library::load(Some(&dir));
        reload_lib(false, self);

        match &self.state {
            None => self.pick(),
            // the steps (a wait: the question picker under the stream answers it; a: here)
            Some(st) => {
                self.popup_state.cur = st.node;
                self.popup_state.mode = Mode::Steps;
            }
        }
    }

    /// Open on the flows to switch to (`> flow`).
    pub(crate) fn pick(&mut self) {
        self.popup_state.open = true;
        self.popup_state.mode = Mode::Pick(String::new());
        self.popup_state.cur = self
            .state
            .as_ref()
            .and_then(|text| self.lib.flows.iter().position(|file| file.slug == text.flow))
            .unwrap_or(0);
        self.popup_state.offset = 0;
    }

    fn picks(&self, filter: &str) -> Vec<usize> {
        let query = filter.to_lowercase();

        (0..self.lib.flows.len())
            .filter(|&i| {
                query.is_empty()
                    || self.lib.flows[i].name.to_lowercase().contains(&query)
                    || self.lib.flows[i].slug.contains(&query)
            })
            .collect()
    }

    fn running(&self) -> bool {
        self.state.as_ref().is_some_and(|text| !text.finished)
    }

    fn len(&self) -> usize {
        match &self.popup_state.mode {
            Mode::Steps => self.nodes.len(),
            Mode::Pick(query) => self.picks(query).len(),
            Mode::How(_) => 3,
            Mode::Start(slug) => self.lib.flow(slug).map_or(0, |file| file.steps.len()),
        }
    }

    /// Rows for the flow area: 0 (closed: the agent card shows the step) or the row + the open
    /// box (0 flow: the row says so).
    pub(crate) fn rows(&self, max: u16) -> u16 {
        if !self.popup_state.open {
            return 0;
        }

        let body = self.len().max(1) as u16 + 4 + if self.detail { 8 } else { 0 } + self.warnings.len().min(4) as u16;

        (1 + body).min(max.max(1))
    }

    fn frame_text(&self) -> (String, &'static str) {
        match &self.popup_state.mode {
            Mode::Steps if self.state.as_ref().is_some_and(|text| text.wait.is_some()) => (
                format!("flow · {} · @{}", self.name, self.agent),
                "the wait: answer below the stream · ↑↓ · enter text · s skip · j jump · c change · e edit · esc",
            ),
            Mode::Steps
                if self
                    .state
                    .as_ref()
                    .is_some_and(|text_value| text_value.pending.is_some() || text_value.queued.is_some()) =>
            {
                (
                    format!("flow · {} · @{}", self.name, self.agent),
                    "↑↓ · enter text · u undo switch · s skip · j jump · c change · e edit · esc",
                )
            }
            Mode::Steps => (
                format!("flow · {} · @{}", self.name, self.agent),
                "↑↓ · enter text · s skip · j jump here · c change · e edit · d d remove · esc",
            ),
            Mode::Pick(_) => (
                format!("pick a flow for @{}", self.agent),
                "type: filter · ↑↓ · enter · esc",
            ),
            Mode::How(_) => (format!("change the flow of @{}", self.agent), "↑↓ · enter · esc: back"),
            Mode::Start(_) => ("start step".into(), "↑↓ · enter: switch · esc: back"),
        }
    }

    fn list(&mut self, frame: &mut Frame, area: Rect, rows: &[(String, String)], focus: bool) {
        self.popup_state.offset = crate::ui::dropdown::fit(
            self.popup_state.offset,
            self.popup_state.cur,
            rows.len(),
            usize::from(area.height),
        );

        for (key, (label, detail)) in rows
            .iter()
            .enumerate()
            .skip(self.popup_state.offset)
            .take(usize::from(area.height))
        {
            list_area_value(frame, area, focus, key, label, detail, self);
        }
    }
}

fn wait_ask_body(back: impl Fn(&str) -> String, character_value: &str, line: &str) -> (String, String) {
    (character_value.to_string(), format!("{line}{}", back(character_value)))
}

/// A rail row of node N: MARK, its title, its tags (gate, one-off).
pub(crate) fn rail_row(count: &Resolved, mark: &str, ms: Style) -> flow_rail::Row {
    let mut tags = Vec::new();

    if count.gate {
        tags.push(("⏸".to_string(), theme::fg(theme::pal().attention())));
    }

    if count.step.is_none() {
        tags.push(("◇".to_string(), theme::muted()));
    }

    flow_rail::Row {
        mark: (mark.to_string(), ms),
        title: count.title.clone(),
        title_style: Style::default(),
        tags,
    }
}

fn dur(ms: u64) -> String {
    let text = ms / 1000;

    if text < 60 {
        format!("{text}s")
    } else if text < 3600 {
        format!("{}m", text / 60)
    } else {
        format!("{}h{:02}", text / 3600, text / 60 % 60)
    }
}

#[cfg(test)]
#[path = "flows_flow_bar_wait_tests.rs"]
mod wait_tests;

pub(crate) use crate::engine::flows::run::{REPLY, WAIT_TOOL};

#[path = "flow_bar_draw_helpers.rs"]
mod draw_helpers;
#[path = "flow_bar_key.rs"]
mod key;
#[path = "flow_bar_mouse.rs"]
mod mouse;
#[path = "flow_bar_render.rs"]
mod render;
#[path = "flow_bar_tick.rs"]
mod tick;

fn reload_lib(force: bool, view_state: &mut FlowBar) {
    if force
        || view_state
            .lib_at
            .is_none_or(|thread| thread.elapsed() > Duration::from_secs(5))
    {
        view_state.lib = Library::load(Some(&view_state.dir));
        view_state.lib_at = Some(Instant::now());
    }

    view_state.nodes.clear();
    view_state.warnings.clear();
    view_state.name.clear();

    if let Some(st) = &view_state.state {
        view_state.name = st.title.clone();

        if let Some(file) = view_state.lib.flow(&st.flow) {
            view_state.name = file.name.clone();
            view_state.nodes = flows::resolve(file, &view_state.lib);
            let roster = crate::engine::roster::active();

            view_state.warnings = flows::run::warnings(&view_state.nodes, &view_state.agent, &roster, &view_state.dir);
        }
    }
}

fn list_area_value(
    frame: &mut Frame<'_>,
    area: Rect,
    focus: bool,
    key: usize,
    label: &str,
    detail: &str,
    view_state: &mut FlowBar,
) {
    let area_value = Rect {
        y: area.y + (key - view_state.popup_state.offset) as u16,
        height: 1,
        ..area
    };
    let on = key == view_state.popup_state.cur;

    let st = if on {
        if focus {
            theme::pill().add_modifier(Modifier::BOLD)
        } else {
            theme::panel().add_modifier(Modifier::BOLD)
        }
    } else {
        theme::text()
    };

    let lw = 28.min(usize::from(area_value.width) / 2);
    let line = Line::from(vec![
        Span::styled(format!(" {:<lw$} ", crate::ui::thread_card::fit(label, lw)), st),
        Span::styled(
            crate::ui::thread_card::fit(detail, usize::from(area_value.width).saturating_sub(lw + 3)),
            if on { st } else { theme::muted() },
        ),
    ]);

    frame.render_widget(
        Paragraph::new(line).style(if on { st } else { Style::default() }),
        area_value,
    );
    view_state.popup_state.rows.push((area_value, key));
}

#[path = "flow_bar_text.rs"]
mod text;

struct PopupStateState {
    open: bool,
    /// `d d`: the 1st `d` armed the flow's removal.
    del_armed: bool,
    mode: Mode,
    cur: usize,
    offset: usize,
    bar: Rect,
    popup: Rect,
    rows: Vec<(Rect, usize)>,
    hover: bool,
}

fn wait_ask_flow_bar(receiver_state: &FlowBar) -> Option<crate::domain::chat::Ask> {
    let st = receiver_state.state.as_ref()?;
    let width = st.wait.as_ref()?;
    let node = receiver_state.nodes.get(st.node);
    let step = node.map_or_else(|| format!("step {}", st.node + 1), |count| count.title.clone());

    let flow = if st.title.is_empty() {
        st.flow.clone()
    } else {
        st.title.clone()
    };

    // an outcome: where it goes back to
    let back = |character: &str| {
        node.and_then(|count_value| count_value.edges.iter().find(|(key, _)| key == character))
            .and_then(|(_, j)| receiver_state.nodes.get(*j))
            .map(|thread| format!(" · back to {}", thread.title))
            .unwrap_or_default()
    };

    let (kind, question, options): (&str, String, Vec<(String, String)>) = match width {
        Wait::Gate => {
            let mut value: Vec<(String, String)> = choices(width, node)
                .into_iter()
                .map(|(character_value, line)| wait_ask_body(back, &character_value, &line))
                .collect();

            value.push((
                REPLY.into(),
                "reply · the prompt's text to the agent, the gate stays open".into(),
            ));
            (
                "gate",
                format!("gate after “{step}”: go on? (text in the prompt: your note)"),
                value,
            )
        }
        Wait::LoopMax => {
            let mut value_value = choices(width, node);

            value_value.push((
                REPLY.into(),
                "reply · the prompt's text to the agent, the wait stays".into(),
            ));
            (
                "loop",
                format!("“{step}” ran its max rounds (text in the prompt: your note)"),
                value_value,
            )
        }
    };

    let opts: Vec<Value> = options
        .into_iter()
        .map(|(id, name)| json!({"id": id, "name": name, "kind": "flow"}))
        .collect();

    Some(crate::domain::chat::Ask {
        id: format!("flow:{}:{}:{}:{kind}", receiver_state.agent, st.flow, st.node),
        agent: format!("flow {flow} of {}", receiver_state.agent),
        tool: WAIT_TOOL.into(),
        input: json!({"question": question, "_options": opts}),
        ts: st.node_started,
    })
}
