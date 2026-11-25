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
    popup_state: PopupState,
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
            popup_state: PopupState {
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
fn choices(wait: &Wait, node: Option<&Resolved>) -> Vec<(String, String)> {
    match wait {
        Wait::Gate => gate_choices(node),
        Wait::LoopMax => vec![
            ("again".into(), "1 more round".into()),
            ("next".into(), "move on".into()),
        ],
    }
}

fn gate_choices(node: Option<&Resolved>) -> Vec<(String, String)> {
    let mut choices = vec![("approve".to_string(), "approve · go on".to_string())];

    if let Some(node) = node {
        for (key, _) in &node.edges {
            choices.push((key.clone(), key.clone()));
        }
    }

    choices
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
        self.state.as_ref().is_some_and(|run| run.wait == Some(Wait::Gate))
    }

    /// The flow's wait as an ask for the question picker under the stream: a gate (approve or an
    /// outcome), a loop at its max (1 more round, move on). Its choices: `_options` (id, name); the prompt's text goes along as the note.
    /// None: 0 wait.
    pub(crate) fn wait_ask(&self) -> Option<crate::domain::chat::Ask> {
        wait_ask_flow_bar(self)
    }

    /// The answer OPTION (an id of `wait_ask`): the flow's step to send, the wait gone here.
    pub(crate) fn wait_answer(&mut self, option: &str) -> Option<Act> {
        let run = self.state.as_mut()?;

        run.wait.as_ref()?;
        run.wait = None;
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
            Some(run) => {
                self.popup_state.cur = run.node;
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
            .and_then(|run| self.lib.flows.iter().position(|flow| flow.slug == run.flow))
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
        self.state.as_ref().is_some_and(|run| !run.finished)
    }

    fn len(&self) -> usize {
        match &self.popup_state.mode {
            Mode::Steps => self.nodes.len(),
            Mode::Pick(query) => self.picks(query).len(),
            Mode::How(_) => 3,
            Mode::Start(slug) => self.lib.flow(slug).map_or(0, |flow| flow.steps.len()),
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
            Mode::Steps if self.state.as_ref().is_some_and(|run| run.wait.is_some()) => (
                format!("flow · {} · @{}", self.name, self.agent),
                "the wait: answer below the stream · ↑↓ · enter text · s skip · j jump · c change · e edit · esc",
            ),
            Mode::Steps
                if self
                    .state
                    .as_ref()
                    .is_some_and(|run| run.pending.is_some() || run.queued.is_some()) =>
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

        for (row, (label, detail)) in rows
            .iter()
            .enumerate()
            .skip(self.popup_state.offset)
            .take(usize::from(area.height))
        {
            draw_list_row(frame, area, focus, row, label, detail, self);
        }
    }
}

fn gate_choice(back: impl Fn(&str) -> String, choice: &str, label: &str) -> (String, String) {
    (choice.to_string(), format!("{label}{}", back(choice)))
}

/// A rail row of node N: MARK, its title, its tags (gate, one-off).
pub(crate) fn rail_row(node: &Resolved, mark: &str, mark_style: Style) -> flow_rail::Row {
    let mut tags = Vec::new();

    if node.gate {
        tags.push(("⏸".to_string(), theme::fg(theme::pal().attention())));
    }

    if node.step.is_none() {
        tags.push(("◇".to_string(), theme::muted()));
    }

    flow_rail::Row {
        mark: (mark.to_string(), mark_style),
        title: node.title.clone(),
        title_style: Style::default(),
        tags,
    }
}

fn dur(ms: u64) -> String {
    let secs = ms / 1000;

    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h{:02}", secs / 3600, secs / 60 % 60)
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

fn reload_lib(force: bool, bar: &mut FlowBar) {
    if force
        || bar
            .lib_at
            .is_none_or(|loaded| loaded.elapsed() > Duration::from_secs(5))
    {
        bar.lib = Library::load(Some(&bar.dir));
        bar.lib_at = Some(Instant::now());
    }

    bar.nodes.clear();
    bar.warnings.clear();
    bar.name.clear();

    if let Some(run) = &bar.state {
        bar.name = run.title.clone();

        if let Some(flow) = bar.lib.flow(&run.flow) {
            bar.name = flow.name.clone();
            bar.nodes = flows::resolve(flow, &bar.lib);
            let roster = crate::engine::roster::active();

            bar.warnings = flows::run::warnings(&bar.nodes, &bar.agent, &roster, &bar.dir);
        }
    }
}

fn draw_list_row(
    frame: &mut Frame<'_>,
    area: Rect,
    focus: bool,
    row: usize,
    label: &str,
    detail: &str,
    bar: &mut FlowBar,
) {
    let row_area = Rect {
        y: area.y + (row - bar.popup_state.offset) as u16,
        height: 1,
        ..area
    };
    let on = row == bar.popup_state.cur;

    let style = if on {
        if focus {
            theme::pill().add_modifier(Modifier::BOLD)
        } else {
            theme::panel().add_modifier(Modifier::BOLD)
        }
    } else {
        theme::text()
    };

    let label_width = 28.min(usize::from(row_area.width) / 2);
    let line = Line::from(vec![
        Span::styled(
            format!(" {:<label_width$} ", crate::ui::thread_card::fit(label, label_width)),
            style,
        ),
        Span::styled(
            crate::ui::thread_card::fit(detail, usize::from(row_area.width).saturating_sub(label_width + 3)),
            if on { style } else { theme::muted() },
        ),
    ]);

    frame.render_widget(
        Paragraph::new(line).style(if on { style } else { Style::default() }),
        row_area,
    );
    bar.popup_state.rows.push((row_area, row));
}

#[path = "flow_bar_text.rs"]
mod text;

struct PopupState {
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

fn wait_ask_flow_bar(bar: &FlowBar) -> Option<crate::domain::chat::Ask> {
    let run = bar.state.as_ref()?;
    let wait = run.wait.as_ref()?;
    let node = bar.nodes.get(run.node);
    let step = node.map_or_else(|| format!("step {}", run.node + 1), |node| node.title.clone());

    let flow = if run.title.is_empty() {
        run.flow.clone()
    } else {
        run.title.clone()
    };

    // an outcome: where it goes back to
    let back = |choice: &str| {
        node.and_then(|node| node.edges.iter().find(|(key, _)| key == choice))
            .and_then(|(_, j)| bar.nodes.get(*j))
            .map(|target| format!(" · back to {}", target.title))
            .unwrap_or_default()
    };

    let (kind, question, options): (&str, String, Vec<(String, String)>) = match wait {
        Wait::Gate => {
            let mut choices: Vec<(String, String)> = choices(wait, node)
                .into_iter()
                .map(|(choice, label)| gate_choice(back, &choice, &label))
                .collect();

            choices.push((
                REPLY.into(),
                "reply · the prompt's text to the agent, the gate stays open".into(),
            ));
            (
                "gate",
                format!("gate after “{step}”: go on? (text in the prompt: your note)"),
                choices,
            )
        }
        Wait::LoopMax => {
            let mut choices = choices(wait, node);

            choices.push((
                REPLY.into(),
                "reply · the prompt's text to the agent, the wait stays".into(),
            ));
            (
                "loop",
                format!("“{step}” ran its max rounds (text in the prompt: your note)"),
                choices,
            )
        }
    };

    let options_json: Vec<Value> = options
        .into_iter()
        .map(|(id, name)| json!({"id": id, "name": name, "kind": "flow"}))
        .collect();

    Some(crate::domain::chat::Ask {
        id: format!("flow:{}:{}:{}:{kind}", bar.agent, run.flow, run.node),
        agent: format!("flow {flow} of {}", bar.agent),
        tool: WAIT_TOOL.into(),
        input: json!({"question": question, "_options": options_json}),
        ts: run.node_started,
    })
}
