//! The flows tab of the start screen: 2 sub-tabs, every edit saved at once (0 save buttons).
//! - flows: the flows left (built-in, user, repo), the rail of the picked flow right. On the
//!   rail: a add a step (fuzzy picker: a shared step, a new shared step, a one-off), enter edit,
//!   J K move, d remove, g gate, o gate outcome, L loop, + - loop max, s save a one-off as
//!   a shared step. A shared step edited from a flow changes every flow that uses it (the editor
//!   says which); f forks it into a one-off of this flow.
//! - steps: the step library: usage count, n new, enter edit, R rename (flows follow), D
//!   duplicate, d d delete (each flow keeps the text as a one-off), ctrl+d ctrl+d resets an edited built-in, r reads the files again.
//!
//! ←→ on a list: the other sub-tab. ctrl+f filters the list.

pub(crate) mod flow_rail;

pub(crate) mod flow_bar;

use std::path::PathBuf;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::engine::flows::{self, Flow, Gate, Layer, Library, Loop, Node, Step};
use crate::ui::textarea::{Outcome, TextArea};
use crate::ui::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sub {
    Flows,
    Steps,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    List,
    Rail,
    Editor,
}

/// What the tab asks the app for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Out {
    None,
    Toast(String),
    /// ↑ on the top row: the tab bar takes the keys.
    Up,
}

/// A 1-line input for a name, a loop's condition, a gate outcome.
#[derive(Clone, Debug, PartialEq)]
enum Ask {
    NewFlow,
    DuplicateFlow,
    RenameFlow,
    NewStep,
    DuplicateStep,
    RenameStep,
    /// Save node I (a one-off) as a shared step.
    Promote(usize),
    /// The loop of node FROM back to TO: its condition.
    LoopUntil(usize, usize),
    /// A gate outcome of node FROM to TO: its label.
    Outcome(usize, usize),
}

/// Picking a node on the rail for a loop / gate outcome of node FROM.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    Loop(usize),
    Outcome(usize),
}

/// What the editor edits.
#[derive(Clone, Debug, PartialEq)]
enum Edit {
    Step(String),
    /// Node I of the open flow; SHARED: the step it uses.
    Node(usize, Option<String>),
}

struct Editor {
    what: Edit,
    /// (label, input, shared: an edit reaches every flow of the step).
    fields: Vec<(&'static str, TextArea, bool)>,
    field: usize,
    gate: bool,
    /// The gate when the editor opened (a toggle counts as an edit).
    gate0: bool,
}

/// A row of the add-step picker.
#[derive(Clone, Debug, PartialEq)]
enum PickRow {
    OneOff(String),
    NewStep(String),
    Step(String),
}

pub(crate) struct FlowsTab {
    sub: Sub,
    lib: Library,
    dir: Option<PathBuf>,
    at: usize,
    filter: String,
    filtering: bool,
    focus: Focus,
    /// The flow on the rail (a copy; every change saved).
    flow: Option<Flow>,
    node: usize,
    rail_off: usize,
    editor: Option<Editor>,
    picker: Option<(TextArea, usize)>,
    ask: Option<(Ask, TextArea)>,
    target: Option<Target>,
    armed: bool,
    /// A 1st ctrl+d: a 2nd resets the edited built-in.
    reset_armed: bool,
    /// The last esc in the editor: a 2nd within 2 s drops its edits.
    esc_at: Option<std::time::Instant>,
    error: Option<String>,
    note: Option<String>,
    shortcuts: crate::ui::shortcuts::Pick,
    hit_regions: HitRegionsState,
}

fn input(single: bool, placeholder: &str, text: &str) -> TextArea {
    let mut area = TextArea::new(single, placeholder);

    area.style = Some(theme::panel());
    area.set_text(text);
    area.move_end();
    area
}

impl FlowsTab {
    pub(crate) fn new(dir: Option<PathBuf>) -> FlowsTab {
        let mut tab = Self {
            sub: Sub::Flows,
            lib: Library::default(),
            dir,
            at: 0,
            filter: String::new(),
            filtering: false,
            focus: Focus::List,
            flow: None,
            node: 0,
            rail_off: 0,
            editor: None,
            picker: None,
            ask: None,
            target: None,
            armed: false,
            reset_armed: false,
            esc_at: None,
            error: None,
            note: None,
            shortcuts: crate::ui::shortcuts::Pick::default(),
            hit_regions: HitRegionsState {
                list_rect: Rect::default(),
                rows: Vec::new(),
                rail_rows: Vec::new(),
                sub_rects: Vec::new(),
                field_rects: Vec::new(),
            },
        };

        tab.reload();
        tab
    }

    /// The tab shows: the files read again.
    pub(crate) fn enter(&mut self) {
        self.reload();
    }

    /// Open flow SLUG on the rail.
    pub(crate) fn open(&mut self, slug: &str) {
        self.sub = Sub::Flows;
        self.filter.clear();
        self.reload();

        if let Some(i) = self.lib.flows.iter().position(|flow| flow.slug == slug) {
            self.at = i;
            self.load_flow();
            self.focus = Focus::Rail;
        }
    }

    /// Keys go to an input (the app keeps its letter keys away).
    pub(crate) fn typing(&self) -> bool {
        self.filtering
            || self.ask.is_some()
            || self.picker.is_some()
            || self.editor.is_some()
            || self.target.is_some()
            || self.focus != Focus::List
    }

    fn reload(&mut self) {
        self.lib = Library::load(self.dir.as_deref());
        let count = self.list().len();

        self.at = self.at.min(count.saturating_sub(1));

        if self.sub == Sub::Flows {
            reload_keep(self);
        }
    }

    /// Indexes of the list rows (flows or steps), the filter applied.
    fn list(&self) -> Vec<usize> {
        let query = self.filter.to_lowercase();

        match self.sub {
            Sub::Flows => (0..self.lib.flows.len())
                .filter(|&i| {
                    query.is_empty()
                        || self.lib.flows[i].name.to_lowercase().contains(&query)
                        || self.lib.flows[i].slug.contains(&query)
                })
                .collect(),
            Sub::Steps => (0..self.lib.steps.len())
                .filter(|&i| {
                    query.is_empty()
                        || self.lib.steps[i].name.contains(&query)
                        || self.lib.steps[i].description.to_lowercase().contains(&query)
                })
                .collect(),
        }
    }

    fn cur_step(&self) -> Option<&Step> {
        (self.sub == Sub::Steps)
            .then(|| self.list().get(self.at).and_then(|&i| self.lib.steps.get(i)))
            .flatten()
    }

    // ---------- keys ----------

    // ---------- editors ----------

    // ---------- draw ----------
}

/// The rail rows of NODES: ✓ a library step (◇ one-off), the gate and
/// loop tags (`flow_bar::rail_row`). The flows tab and the new thread's flow dropdown draw them.
pub(crate) fn rail_rows(nodes: &[flows::Resolved]) -> Vec<flow_rail::Row> {
    nodes
        .iter()
        .map(|node| {
            let (mark, mark_style) = if node.step.is_some() {
                ("✓", Style::default().fg(theme::pal().success()))
            } else {
                ("◇", theme::muted())
            };

            let mut row = crate::features::flows::flow_bar::rail_row(node, mark, mark_style);

            if node.step.is_none() {
                row.tags.retain(|(tag, _)| tag != "◇");
                row.tags.push(("one-off".into(), theme::muted()));
            }

            row
        })
        .collect()
}

/// The head of FLOW (its NODES): name · n steps · layer.
pub(crate) fn head_line(flow: &flows::Flow, nodes: &[flows::Resolved]) -> Line<'static> {
    let layer = match (flow.layer, flow.edited) {
        (_, true) => "edited built-in".to_string(),
        (layer, _) => layer.label().to_string(),
    };

    Line::from(vec![
        Span::styled(flow.name.clone(), theme::bold()),
        Span::styled(format!(" · {} steps · {layer}", nodes.len()), theme::muted()),
    ])
}

fn list_names(names: &[String]) -> String {
    if names.len() <= 3 {
        names.join(", ")
    } else {
        format!("{}, +{}", names[..3].join(", "), names.len() - 3)
    }
}

#[cfg(test)]
#[path = "flows_tests.rs"]
mod tests;

#[path = "flows_draw.rs"]
mod draw;
#[path = "flows_draw_helpers.rs"]
mod draw_helpers;
#[path = "flows_key.rs"]
mod key;
#[path = "flows_key_helpers.rs"]
mod key_helpers;
#[path = "flows_mouse.rs"]
mod mouse;
#[path = "flows_updates.rs"]
mod updates;

mod storage;

mod edits;

mod picker;

mod editor;

mod help;

struct HitRegionsState {
    list_rect: Rect,
    rows: Vec<(Rect, usize)>,
    rail_rows: Vec<(Rect, usize)>,
    sub_rects: Vec<(Rect, Sub)>,
    field_rects: Vec<(Rect, usize)>,
}

fn reload_keep(tab: &mut FlowsTab) {
    let keep = tab.flow.as_ref().map(|flow| flow.slug.clone());

    match keep.and_then(|slug| tab.lib.flows.iter().find(|flow| flow.slug == slug).cloned()) {
        Some(flow) if tab.focus != Focus::List => tab.flow = Some(flow),
        _ => tab.load_flow(),
    }
}
