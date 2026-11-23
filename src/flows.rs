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

use std::path::PathBuf;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::engine::flows::{self, Flow, Gate, Layer, Library, Loop, Node, Step};
use crate::flowrail;
use crate::textarea::{Outcome, TextArea};
use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sub {
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
pub enum Out {
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

pub struct FlowsTab {
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
    shortcuts: crate::shortcuts::Pick,
    list_rect: Rect,
    rows: Vec<(Rect, usize)>,
    rail_rows: Vec<(Rect, usize)>,
    sub_rects: Vec<(Rect, Sub)>,
    field_rects: Vec<(Rect, usize)>,
}

fn input(single: bool, placeholder: &str, text: &str) -> TextArea {
    let mut t = TextArea::new(single, placeholder);
    t.style = Some(theme::panel());
    t.set_text(text);
    t.move_end();
    t
}

impl FlowsTab {
    pub fn new(dir: Option<PathBuf>) -> FlowsTab {
        let mut t = FlowsTab {
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
            shortcuts: crate::shortcuts::Pick::default(),
            list_rect: Rect::default(),
            rows: Vec::new(),
            rail_rows: Vec::new(),
            sub_rects: Vec::new(),
            field_rects: Vec::new(),
        };
        t.reload();
        t
    }

    /// The tab shows: the files read again.
    pub fn enter(&mut self) {
        self.reload();
    }

    /// Open flow SLUG on the rail.
    pub fn open(&mut self, slug: &str) {
        self.sub = Sub::Flows;
        self.filter.clear();
        self.reload();
        if let Some(i) = self.lib.flows.iter().position(|f| f.slug == slug) {
            self.at = i;
            self.load_flow();
            self.focus = Focus::Rail;
        }
    }

    /// Keys go to an input (the app keeps its letter keys away).
    pub fn typing(&self) -> bool {
        self.filtering || self.ask.is_some() || self.picker.is_some() || self.editor.is_some() || self.target.is_some() || self.focus != Focus::List
    }

    fn reload(&mut self) {
        self.lib = Library::load(self.dir.as_deref());
        let n = self.list().len();
        self.at = self.at.min(n.saturating_sub(1));
        if self.sub == Sub::Flows {
            let keep = self.flow.as_ref().map(|f| f.slug.clone());
            match keep.and_then(|s| self.lib.flows.iter().find(|f| f.slug == s).cloned()) {
                Some(f) if self.focus != Focus::List => self.flow = Some(f),
                _ => self.load_flow(),
            }
        }
    }

    /// Indexes of the list rows (flows or steps), the filter applied.
    fn list(&self) -> Vec<usize> {
        let q = self.filter.to_lowercase();
        match self.sub {
            Sub::Flows => (0..self.lib.flows.len()).filter(|&i| q.is_empty() || self.lib.flows[i].name.to_lowercase().contains(&q) || self.lib.flows[i].slug.contains(&q)).collect(),
            Sub::Steps => (0..self.lib.steps.len()).filter(|&i| q.is_empty() || self.lib.steps[i].name.contains(&q) || self.lib.steps[i].description.to_lowercase().contains(&q)).collect(),
        }
    }

    fn load_flow(&mut self) {
        self.flow = self.list().get(self.at).and_then(|&i| self.lib.flows.get(i)).cloned().filter(|_| self.sub == Sub::Flows);
        self.node = 0;
        self.rail_off = 0;
    }

    fn cur_step(&self) -> Option<&Step> {
        (self.sub == Sub::Steps).then(|| self.list().get(self.at).and_then(|&i| self.lib.steps.get(i))).flatten()
    }

    fn fail(&mut self, e: impl Into<String>) {
        self.error = Some(e.into());
    }

    /// The open flow to disk (a built-in: a user file over it), the library read again.
    fn save_flow(&mut self) {
        let Some(f) = self.flow.clone() else { return };
        match self.lib.save_flow(&f) {
            Ok(_) => {
                self.error = None;
                self.lib = Library::load(self.dir.as_deref());
                if let Some(nf) = self.lib.flows.iter().find(|x| x.slug == f.slug).cloned() {
                    self.flow = Some(nf);
                }
            }
            Err(e) => self.fail(format!("save: {e}")),
        }
    }

    fn save_step(&mut self, s: &Step) -> bool {
        match self.lib.save_step(s) {
            Ok(_) => {
                self.lib = Library::load(self.dir.as_deref());
                if let Some(f) = self.flow.as_ref().and_then(|f| self.lib.flows.iter().find(|x| x.slug == f.slug)).cloned() {
                    self.flow = Some(f);
                }
                true
            }
            Err(e) => {
                self.fail(format!("save: {e}"));
                false
            }
        }
    }

    // ---------- keys ----------

    pub fn key(&mut self, k: KeyEvent) -> Out {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if !matches!(k.code, KeyCode::Char('d')) || ctrl {
            self.armed = false;
        }
        if !(ctrl && k.code == KeyCode::Char('d')) {
            self.reset_armed = false;
        }
        if self.ask.is_some() {
            self.ask_key(k);
            return self.out();
        }
        if self.picker.is_some() {
            self.picker_key(k);
            return self.out();
        }
        if let Some(t) = self.target {
            self.target_key(k, t);
            return self.out();
        }
        if self.filtering {
            match k.code {
                KeyCode::Esc => {
                    self.filter.clear();
                    self.filtering = false;
                }
                KeyCode::Enter | KeyCode::Down => self.filtering = false,
                KeyCode::Backspace | KeyCode::Char('u' | 'w' | 'h') if crate::textarea::delete_key(&mut self.filter, &k) => {}
                KeyCode::Char(c) if !ctrl => self.filter.push(c),
                _ => {}
            }
            self.at = 0;
            self.load_flow();
            return Out::None;
        }
        match self.focus {
            Focus::List => return self.list_key(k),
            Focus::Rail => self.rail_key(k),
            Focus::Editor => self.editor_key(k),
        }
        self.out()
    }

    fn out(&mut self) -> Out {
        match self.note.take() {
            Some(n) => Out::Toast(n),
            None => Out::None,
        }
    }

    fn list_key(&mut self, k: KeyEvent) -> Out {
        let n = self.list().len();
        match k.code {
            _ if crate::textarea::is_filter_key(&k) => self.filtering = true,
            KeyCode::Up | KeyCode::Char('k') if self.at == 0 => return Out::Up,
            KeyCode::Up | KeyCode::Char('k') => {
                self.at -= 1;
                self.load_flow();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.at = (self.at + 1).min(n.saturating_sub(1));
                self.load_flow();
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l') => {
                self.sub = if self.sub == Sub::Flows { Sub::Steps } else { Sub::Flows };
                self.at = 0;
                self.filter.clear();
                self.reload();
            }
            KeyCode::Enter => match self.sub {
                Sub::Flows if self.flow.is_some() => {
                    self.focus = Focus::Rail;
                    self.node = 0;
                }
                Sub::Steps => {
                    if let Some(s) = self.cur_step().cloned() {
                        self.open_step_editor(&s);
                    }
                }
                _ => {}
            },
            KeyCode::Char('n') => self.ask = Some((if self.sub == Sub::Flows { Ask::NewFlow } else { Ask::NewStep }, input(true, "name", ""))),
            KeyCode::Char('D') => {
                let base = match self.sub {
                    Sub::Flows => self.flow.as_ref().map(|f| format!("{} copy", f.name)),
                    Sub::Steps => self.cur_step().map(|s| self.lib.free_step_name(&format!("{}-copy", s.name))),
                };
                if let Some(b) = base {
                    self.ask = Some((if self.sub == Sub::Flows { Ask::DuplicateFlow } else { Ask::DuplicateStep }, input(true, "name", &b)));
                }
            }
            KeyCode::Char('R') => {
                let cur = match self.sub {
                    Sub::Flows => self.flow.as_ref().map(|f| f.name.clone()),
                    Sub::Steps => self.cur_step().map(|s| s.name.clone()),
                };
                if let Some(c) = cur {
                    self.ask = Some((if self.sub == Sub::Flows { Ask::RenameFlow } else { Ask::RenameStep }, input(true, "name", &c)));
                }
            }
            // ctrl+d ctrl+d: an edited built-in back to its default (as ctrl+d in settings)
            KeyCode::Char('d') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                if !self.reset_armed {
                    self.reset_armed = true;
                    self.note = Some("ctrl+d again: back to the built-in (your edits go)".into());
                    return self.out();
                }
                self.reset_armed = false;
                self.reset();
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                if !self.armed {
                    self.armed = true;
                    return Out::None;
                }
                self.armed = false;
                self.delete();
            }
            // r: read the flow and step files again, as on every screen
            KeyCode::Char('r') => {
                self.reload();
                self.note = Some("flows: read again".into());
            }
            KeyCode::Char('m') if self.sub == Sub::Flows => self.move_layer(),
            _ => {}
        }
        self.out()
    }

    fn delete(&mut self) {
        match self.sub {
            Sub::Flows => {
                let Some(f) = self.flow.clone() else { return };
                if f.layer == Layer::Builtin {
                    return self.fail("a built-in flow stays: edit it (a user copy), or duplicate it");
                }
                if let Err(e) = self.lib.remove_flow_file(&f.slug, f.layer) {
                    return self.fail(e);
                }
                self.note = Some(if f.edited { format!("{}: back to the built-in", f.name) } else { format!("flow {} deleted", f.name) });
            }
            Sub::Steps => {
                let Some(s) = self.cur_step().cloned() else { return };
                if s.edited {
                    return self.fail("an edited built-in: ctrl+d resets it");
                }
                match self.lib.delete_step(&s.name) {
                    Ok(n) => self.note = Some(format!("step {} deleted · {n} flows keep its text as a one-off", s.name)),
                    Err(e) => return self.fail(e),
                }
            }
        }
        self.error = None;
        self.reload();
    }

    /// An edited built-in back to the built-in.
    fn reset(&mut self) {
        let (name, layer, edited) = match self.sub {
            Sub::Flows => match &self.flow {
                Some(f) => (f.slug.clone(), f.layer, f.edited),
                None => return,
            },
            Sub::Steps => match self.cur_step() {
                Some(s) => (s.name.clone(), s.layer, s.edited),
                None => return,
            },
        };
        if !edited {
            return self.fail("0 edits to reset: only an edited built-in resets");
        }
        let r = if self.sub == Sub::Flows { self.lib.remove_flow_file(&name, layer) } else { self.lib.remove_step_file(&name, layer) };
        match r {
            Ok(()) => self.note = Some(format!("{name}: back to the built-in")),
            Err(e) => return self.fail(e),
        }
        self.flow = None;
        self.reload();
    }

    /// The flow from the user layer to the repo (`.codehead/flows`) and back.
    fn move_layer(&mut self) {
        let Some(mut f) = self.flow.clone() else { return };
        if self.lib.repo.is_none() {
            return self.fail("0 repo here: start codehead in a repo for repo flows");
        }
        let old = f.layer;
        f.layer = if old == Layer::Repo { Layer::User } else { Layer::Repo };
        if let Err(e) = self.lib.save_flow(&f) {
            return self.fail(e);
        }
        if old != Layer::Builtin {
            let _ = self.lib.remove_flow_file(&f.slug, old);
        }
        self.note = Some(format!("{} → {} flows", f.name, f.layer.label()));
        self.flow = Some(f);
        self.reload();
    }

    fn rail_key(&mut self, k: KeyEvent) {
        let Some(f) = self.flow.clone() else {
            self.focus = Focus::List;
            return;
        };
        let n = f.steps.len();
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        match k.code {
            KeyCode::Esc | KeyCode::Left => self.focus = Focus::List,
            KeyCode::Up | KeyCode::Char('k') if alt => self.move_node(-1),
            KeyCode::Down | KeyCode::Char('j') if alt => self.move_node(1),
            KeyCode::Char('K') => self.move_node(-1),
            KeyCode::Char('J') => self.move_node(1),
            KeyCode::Up | KeyCode::Char('k') => self.node = self.node.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.node = (self.node + 1).min(n.saturating_sub(1)),
            KeyCode::Char('a') | KeyCode::Char('n') => self.picker = Some((input(true, "step name, or text of a one-off step", ""), 0)),
            KeyCode::Enter | KeyCode::Char('e') if n > 0 => self.open_node_editor(self.node),
            KeyCode::Char('d') | KeyCode::Delete if n > 0 => {
                if !self.armed {
                    self.armed = true;
                    return;
                }
                self.armed = false;
                if let Some(fl) = self.flow.as_mut() {
                    fl.steps.remove(self.node);
                }
                self.node = self.node.min(n.saturating_sub(2));
                self.save_flow();
            }
            KeyCode::Char('g') if n > 0 => {
                let lib_gate = f.steps[self.node].uses.as_deref().and_then(|u| self.lib.step(u)).is_some_and(|s| s.gate);
                if let Some(nd) = self.flow.as_mut().and_then(|fl| fl.steps.get_mut(self.node)) {
                    let on = match &nd.gate {
                        Some(Gate::Flag(b)) => *b,
                        Some(Gate::Edges(_)) => true,
                        None => lib_gate,
                    };
                    nd.gate = if on { if lib_gate { Some(Gate::Flag(false)) } else { None } } else if lib_gate { None } else { Some(Gate::Flag(true)) };
                }
                self.save_flow();
            }
            KeyCode::Char('L') | KeyCode::Char('l') if n > 0 && !shift || k.code == KeyCode::Char('L') => {
                // a loop there: it goes; else pick its target
                if f.steps[self.node].loops.is_some() {
                    if let Some(nd) = self.flow.as_mut().and_then(|fl| fl.steps.get_mut(self.node)) {
                        nd.loops = None;
                    }
                    self.save_flow();
                } else if n > 0 {
                    self.target = Some(Target::Loop(self.node));
                }
            }
            KeyCode::Char('o') if n > 0 => self.target = Some(Target::Outcome(self.node)),
            KeyCode::Char('O') if n > 0 => {
                if let Some(nd) = self.flow.as_mut().and_then(|fl| fl.steps.get_mut(self.node))
                    && matches!(nd.gate, Some(Gate::Edges(_)))
                {
                    nd.gate = Some(Gate::Flag(true));
                }
                self.save_flow();
            }
            KeyCode::Char('+') | KeyCode::Char('-') if n > 0 => {
                let d: i64 = if k.code == KeyCode::Char('+') { 1 } else { -1 };
                if let Some(l) = self.flow.as_mut().and_then(|fl| fl.steps.get_mut(self.node)).and_then(|nd| nd.loops.as_mut()) {
                    l.max = (i64::from(l.max) + d).clamp(1, 99) as u32;
                }
                self.save_flow();
            }
            KeyCode::Char('s') if n > 0 => {
                if f.steps[self.node].uses.is_some() {
                    return self.fail("a shared step already");
                }
                let base = self.lib.free_step_name(&flows::slug(&flows::node_title(&f.steps[self.node])));
                self.ask = Some((Ask::Promote(self.node), input(true, "step name", &base)));
            }
            _ => {}
        }
    }

    fn move_node(&mut self, d: isize) {
        let Some(fl) = self.flow.as_mut() else { return };
        let to = self.node as isize + d;
        if to < 0 || to as usize >= fl.steps.len() {
            return;
        }
        // keep the ids of the moved nodes (loops and outcomes point to ids)
        let ids = flows::node_ids(fl);
        for (i, nd) in fl.steps.iter_mut().enumerate() {
            if nd.id.is_none() && nd.uses.is_none() {
                nd.id = Some(ids[i].clone());
            }
        }
        fl.steps.swap(self.node, to as usize);
        self.node = to as usize;
        self.save_flow();
    }

    fn target_key(&mut self, k: KeyEvent, t: Target) {
        let n = self.flow.as_ref().map_or(0, |f| f.steps.len());
        match k.code {
            KeyCode::Esc => {
                let (Target::Loop(i) | Target::Outcome(i)) = t;
                self.node = i;
                self.target = None;
            }
            KeyCode::Up | KeyCode::Char('k') => self.node = self.node.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.node = (self.node + 1).min(n.saturating_sub(1)),
            KeyCode::Enter => {
                self.target = None;
                match t {
                    Target::Loop(from) => {
                        if self.node > from {
                            self.node = from;
                            return self.fail("a loop goes back: pick this step or one above it");
                        }
                        let to = self.node;
                        self.node = from;
                        self.ask = Some((Ask::LoopUntil(from, to), input(true, "until: the condition that ends the loop", "")));
                    }
                    Target::Outcome(from) => {
                        let to = self.node;
                        self.node = from;
                        self.ask = Some((Ask::Outcome(from, to), input(true, "outcome label, e.g. changes", "changes")));
                    }
                }
            }
            _ => {}
        }
    }

    /// An id for node I (set when it has none): loops and outcomes point to it.
    fn id_of(&mut self, i: usize) -> String {
        let Some(fl) = self.flow.as_mut() else { return String::new() };
        let ids = flows::node_ids(fl);
        if fl.steps[i].id.is_none() && fl.steps[i].uses.as_deref() != Some(ids[i].as_str()) {
            fl.steps[i].id = Some(ids[i].clone());
        }
        ids[i].clone()
    }

    fn ask_key(&mut self, k: KeyEvent) {
        let Some((what, mut ta)) = self.ask.take() else { return };
        match k.code {
            KeyCode::Esc => {
                self.error = None;
            }
            KeyCode::Enter => {
                let v = ta.text().trim().to_string();
                if v.is_empty() && !matches!(what, Ask::LoopUntil(..)) {
                    self.ask = Some((what, ta));
                    return self.fail("empty");
                }
                if let Err(e) = self.apply_ask(&what, &v) {
                    self.ask = Some((what, ta));
                    return self.fail(e);
                }
                self.error = None;
            }
            _ => {
                ta.key(k);
                self.ask = Some((what, ta));
            }
        }
    }

    fn apply_ask(&mut self, what: &Ask, v: &str) -> Result<(), String> {
        match what {
            Ask::NewFlow | Ask::DuplicateFlow => {
                let slug = self.lib.free_flow_slug(&flows::slug(v));
                let mut f = match what {
                    Ask::DuplicateFlow => self.flow.clone().unwrap_or_default(),
                    _ => Flow::default(),
                };
                f.slug = slug.clone();
                f.name = v.to_string();
                f.layer = if f.layer == Layer::Repo { Layer::Repo } else { Layer::User };
                f.edited = false;
                self.lib.save_flow(&f)?;
                self.filter.clear();
                self.reload();
                self.at = self.list().iter().position(|&i| self.lib.flows[i].slug == slug).unwrap_or(0);
                self.load_flow();
                self.focus = Focus::Rail;
                if f.steps.is_empty() {
                    self.picker = Some((input(true, "step name, or text of a one-off step", ""), 0));
                }
            }
            Ask::RenameFlow => {
                if let Some(f) = self.flow.as_mut() {
                    f.name = v.to_string();
                }
                self.save_flow();
            }
            Ask::NewStep | Ask::DuplicateStep => {
                let name = flows::slug(v);
                if self.lib.step(&name).is_some() {
                    return Err(format!("step {name} exists already"));
                }
                let mut s = match what {
                    Ask::DuplicateStep => self.cur_step().cloned().unwrap_or_default(),
                    _ => Step::default(),
                };
                s.name = name.clone();
                s.layer = Layer::User;
                s.edited = false;
                self.lib.save_step(&s)?;
                self.filter.clear();
                self.reload();
                self.at = self.list().iter().position(|&i| self.lib.steps[i].name == name).unwrap_or(0);
                if let Some(s) = self.lib.step(&name).cloned() {
                    self.open_step_editor(&s);
                }
            }
            Ask::RenameStep => {
                let Some(s) = self.cur_step().cloned() else { return Ok(()) };
                let new = flows::slug(v);
                self.lib.rename_step(&s.name, &new)?;
                self.reload();
                self.at = self.list().iter().position(|&i| self.lib.steps[i].name == new).unwrap_or(0);
                self.note = Some(format!("step {} → {new}: every flow follows", s.name));
            }
            Ask::Promote(i) => {
                let name = flows::slug(v);
                if self.lib.step(&name).is_some() {
                    return Err(format!("step {name} exists already"));
                }
                let Some(nd) = self.flow.as_ref().and_then(|f| f.steps.get(*i)).cloned() else { return Ok(()) };
                let (s, node) = flows::promote(&nd, &name);
                self.lib.save_step(&s)?;
                if let Some(f) = self.flow.as_mut() {
                    f.steps[*i] = node;
                }
                self.save_flow();
                self.note = Some(format!("step {name} saved: a for it in any flow"));
            }
            Ask::LoopUntil(from, to) => {
                let to_id = self.id_of(*to);
                if let Some(nd) = self.flow.as_mut().and_then(|f| f.steps.get_mut(*from)) {
                    nd.loops = Some(Loop { to: to_id, until: v.to_string(), max: 3 });
                }
                self.save_flow();
            }
            Ask::Outcome(from, to) => {
                if !flows::valid_name(v) {
                    return Err("label: letters, digits, - and _".into());
                }
                let to_id = self.id_of(*to);
                if let Some(nd) = self.flow.as_mut().and_then(|f| f.steps.get_mut(*from)) {
                    let mut m = match nd.gate.take() {
                        Some(Gate::Edges(m)) => m,
                        _ => Default::default(),
                    };
                    m.insert(v.to_string(), to_id);
                    nd.gate = Some(Gate::Edges(m));
                }
                self.save_flow();
            }
        }
        Ok(())
    }

    fn pick_rows(&self, q: &str) -> Vec<PickRow> {
        let q = q.trim();
        let mut v = Vec::new();
        let ql = q.to_lowercase();
        let mut hits: Vec<(i64, String)> = self
            .lib
            .steps
            .iter()
            .filter_map(|s| {
                if ql.is_empty() {
                    return Some((0, s.name.clone()));
                }
                let score = if s.name == ql { 1000 } else if s.name.starts_with(&ql) { 500 } else if s.name.contains(&ql) { 300 } else if s.description.to_lowercase().contains(&ql) { 100 } else { return None };
                Some((score, s.name.clone()))
            })
            .collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let exact = hits.first().is_some_and(|h| h.0 == 1000);
        if exact {
            v.extend(hits.into_iter().map(|h| PickRow::Step(h.1)));
            v.push(PickRow::OneOff(q.to_string()));
        } else {
            v.extend(hits.into_iter().map(|h| PickRow::Step(h.1)));
            if !q.is_empty() {
                v.insert(0, PickRow::OneOff(q.to_string()));
                v.insert(1, PickRow::NewStep(flows::slug(q)));
            }
        }
        v
    }

    fn picker_key(&mut self, k: KeyEvent) {
        let Some((mut ta, mut cur)) = self.picker.take() else { return };
        let rows = self.pick_rows(&ta.text());
        match k.code {
            KeyCode::Esc => return,
            KeyCode::Up => cur = cur.saturating_sub(1),
            KeyCode::Down => cur = (cur + 1).min(rows.len().saturating_sub(1)),
            KeyCode::Enter | KeyCode::Tab => {
                let Some(row) = rows.get(cur).cloned() else {
                    self.picker = Some((ta, cur));
                    return;
                };
                let node = match &row {
                    PickRow::Step(s) => Node { uses: Some(s.clone()), ..Default::default() },
                    PickRow::OneOff(t) => Node { text: Some(t.clone()), ..Default::default() },
                    PickRow::NewStep(name) => {
                        let s = Step { name: name.clone(), text: ta.text().trim().to_string(), layer: Layer::User, ..Default::default() };
                        if !self.save_step(&s) {
                            self.picker = Some((ta, cur));
                            return;
                        }
                        Node { uses: Some(name.clone()), ..Default::default() }
                    }
                };
                let at = match self.flow.as_ref() {
                    Some(f) if f.steps.is_empty() => 0,
                    Some(_) => self.node + 1,
                    None => return,
                };
                if let Some(f) = self.flow.as_mut() {
                    f.steps.insert(at, node);
                }
                self.node = at;
                self.save_flow();
                // a new text: straight into its editor
                if !matches!(row, PickRow::Step(_)) {
                    self.open_node_editor(at);
                }
                return;
            }
            _ => {
                ta.key(k);
                cur = 0;
            }
        }
        self.picker = Some((ta, cur));
    }

    // ---------- editors ----------

    fn open_step_editor(&mut self, s: &Step) {
        self.editor = Some(Editor {
            what: Edit::Step(s.name.clone()),
            fields: vec![
                ("description", input(true, "1 line: what the step does", &s.description), false),
                ("done when", input(true, "exit criteria", &s.done), false),
                ("needs", input(true, "skill:<name>, mcp:<server>, harness:<name>", &s.needs.join(", ")), false),
                ("text", input(false, "the instruction the agent gets · #shortcuts expand at the step's start", &s.text), false),
            ],
            field: 4,
            gate: s.gate,
            gate0: s.gate,
        });
        self.focus = Focus::Editor;
    }

    fn open_node_editor(&mut self, i: usize) {
        let Some(f) = self.flow.clone() else { return };
        let Some(n) = f.steps.get(i).cloned() else { return };
        let r = flows::resolve(&f, &self.lib).get(i).cloned().unwrap_or_default();
        let shared = n.uses.clone().filter(|u| self.lib.step(u).is_some());
        let fields = match &shared {
            Some(u) => {
                let s = self.lib.step(u).cloned().unwrap_or_default();
                vec![
                    ("label", input(true, "the step's name in this flow (empty: the step name)", n.label.as_deref().unwrap_or_default()), false),
                    ("note", input(false, "text for this flow only, after the step's text", n.note.as_deref().unwrap_or_default()), false),
                    ("done when", input(true, "exit criteria", &s.done), true),
                    ("text", input(false, "the step's instruction", &s.text), true),
                ]
            }
            None => vec![
                ("label", input(true, "short name", n.label.as_deref().unwrap_or_default()), false),
                ("done when", input(true, "exit criteria", n.done.as_deref().unwrap_or_default()), false),
                ("text", input(false, "the instruction · #shortcuts expand at the step's start", n.text.as_deref().unwrap_or_default()), false),
            ],
        };
        let field = fields.len() - 1;
        self.editor = Some(Editor { what: Edit::Node(i, shared), fields, field, gate: r.gate, gate0: r.gate });
        self.focus = Focus::Editor;
    }

    /// The editor's inputs to disk. False: an error (the editor stays).
    fn store_editor(&mut self) -> bool {
        let Some(ed) = self.editor.as_ref() else { return true };
        let vals: Vec<(&'static str, String)> = ed.fields.iter().map(|(l, t, _)| (*l, t.text())).collect();
        let (what, ed_gate) = (ed.what.clone(), ed.gate);
        let val = |k: &str| vals.iter().find(|(l, _)| *l == k).map(|(_, t)| t.clone()).unwrap_or_default();
        let opt = |s: String| Some(s.trim().to_string()).filter(|x| !x.is_empty());
        match what {
            Edit::Step(name) => {
                let Some(mut s) = self.lib.step(&name).cloned() else { return true };
                let new = Step {
                    description: val("description").trim().to_string(),
                    done: val("done when").trim().to_string(),
                    needs: val("needs").split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
                    text: val("text").trim_end().to_string(),
                    gate: ed_gate,
                    ..s.clone()
                };
                if new != s {
                    s = new;
                    if !self.save_step(&s) {
                        return false;
                    }
                }
            }
            Edit::Node(i, shared) => {
                let gate = ed_gate;
                if let Some(u) = &shared
                    && let Some(mut s) = self.lib.step(u).cloned()
                {
                    let (done, text) = (val("done when").trim().to_string(), val("text").trim_end().to_string());
                    if s.done != done || s.text != text {
                        s.done = done;
                        s.text = text;
                        if !self.save_step(&s) {
                            return false;
                        }
                    }
                }
                let lib_gate = shared.as_deref().and_then(|u| self.lib.step(u)).is_some_and(|s| s.gate);
                let Some(f) = self.flow.as_mut() else { return true };
                let Some(n) = f.steps.get_mut(i) else { return true };
                n.label = opt(val("label"));
                if shared.is_some() {
                    n.note = opt(val("note"));
                } else {
                    n.done = opt(val("done when"));
                    n.text = Some(val("text").trim_end().to_string());
                }
                n.gate = match (&n.gate, gate) {
                    (Some(Gate::Edges(e)), true) => Some(Gate::Edges(e.clone())),
                    (_, g) if g == lib_gate => None,
                    (_, g) => Some(Gate::Flag(g)),
                };
                self.save_flow();
            }
        }
        true
    }

    /// The shared step of node I as a one-off of this flow (its text copied in).
    fn fork(&mut self, i: usize) {
        let Some(n) = self.flow.as_ref().and_then(|f| f.steps.get(i)).cloned() else { return };
        let Some(s) = n.uses.as_deref().and_then(|u| self.lib.step(u)).cloned() else { return };
        if let Some(f) = self.flow.as_mut() {
            f.steps[i] = flows::inline(&n, &s);
        }
        self.save_flow();
        self.note = Some(format!("{}: a one-off of this flow now", s.name));
        self.open_node_editor(i);
    }

    fn editor_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let back = |t: &mut FlowsTab| t.focus = if t.sub == Sub::Flows { Focus::Rail } else { Focus::List };
        // the `#` dropdown of the field
        if let Some(ed) = self.editor.as_mut() {
            let ta = &mut ed.fields[ed.field].1;
            if self.shortcuts.key(k, ta) {
                return;
            }
        }
        let Some(ed) = self.editor.as_mut() else { return back(self) };
        let n = ed.fields.len();
        match k.code {
            // esc: closes without a save; edits only on a 2nd esc (ctrl+s saves)
            KeyCode::Esc => {
                let dirty = ed.gate != ed.gate0 || ed.fields.iter().any(|(_, t, _)| t.edited());
                if !crate::textarea::esc_drops(&mut self.esc_at, dirty) {
                    self.note = Some("esc again: drop the edits · ctrl+s: save".into());
                    return;
                }
                self.editor = None;
                self.shortcuts.hide();
                back(self);
            }
            KeyCode::Char('s') if ctrl => {
                if self.store_editor() {
                    self.editor = None;
                    back(self);
                }
            }
            KeyCode::Char('g') if ctrl => ed.gate = !ed.gate,
            KeyCode::Char('f') if ctrl => {
                if let Edit::Node(i, Some(_)) = ed.what {
                    self.editor = None;
                    self.fork(i);
                }
            }
            KeyCode::Tab | KeyCode::Down if k.code == KeyCode::Tab || ed.fields[ed.field].1.on_last_row() => ed.field = (ed.field + 1) % n,
            KeyCode::BackTab => ed.field = (ed.field + n - 1) % n,
            KeyCode::Up if ed.fields[ed.field].1.on_first_row() => ed.field = ed.field.saturating_sub(1),
            _ => {
                let ta = &mut ed.fields[ed.field].1;
                if !matches!(ta.key(k), Outcome::Handled) && k.code == KeyCode::Enter {
                    if ta.is_single() {
                        ed.field = (ed.field + 1) % n;
                    } else {
                        ta.newline();
                    }
                }
            }
        }
    }

    pub fn paste(&mut self, text: &str) {
        if let Some((_, ta)) = self.ask.as_mut() {
            ta.insert(text);
        } else if let Some((ta, _)) = self.picker.as_mut() {
            ta.insert(text);
        } else if let Some(ed) = self.editor.as_mut() {
            ed.fields[ed.field].1.insert(text);
        }
    }

    pub fn mouse(&mut self, m: MouseEvent) -> Out {
        let p = Position { x: m.column, y: m.row };
        let click = matches!(m.kind, MouseEventKind::Down(MouseButton::Left));
        if let Some(ed) = self.editor.as_mut()
            && let Some(&(r, i)) = self.field_rects.iter().find(|(r, _)| r.contains(p))
        {
            let _ = r;
            if click {
                ed.field = i;
            }
            ed.fields[i].1.mouse(m);
            return Out::None;
        }
        if !click {
            return Out::None;
        }
        if let Some(&(_, s)) = self.sub_rects.iter().find(|(r, _)| r.contains(p)) {
            if s != self.sub && self.store_editor() {
                self.editor = None;
                self.sub = s;
                self.at = 0;
                self.focus = Focus::List;
                self.reload();
            }
            return Out::None;
        }
        if let Some(&(_, i)) = self.rows.iter().find(|(r, _)| r.contains(p)) {
            if self.store_editor() {
                self.editor = None;
                self.at = i;
                self.focus = Focus::List;
                self.load_flow();
            }
            return Out::None;
        }
        if let Some(&(_, i)) = self.rail_rows.iter().find(|(r, _)| r.contains(p))
            && self.store_editor()
        {
            self.editor = None;
            self.node = i;
            self.focus = Focus::Rail;
        }
        Out::None
    }

    pub fn help(&self) -> &'static [(&'static str, &'static str)] {
        if self.ask.is_some() {
            return &[("enter", "ok"), ("esc", "cancel")];
        }
        if self.picker.is_some() {
            return &[("type", "find a step / write a one-off"), ("↑↓", "move"), ("enter", "add below"), ("esc", "close")];
        }
        if self.target.is_some() {
            return &[("↑↓", "pick the target step"), ("enter", "ok"), ("esc", "cancel")];
        }
        if self.filtering {
            return &[("type", "filter"), ("enter", "done"), ("esc", "clear")];
        }
        match self.focus {
            Focus::List if self.armed => &[("d", "again: delete"), ("any key", "keep")],
            Focus::List if self.sub == Sub::Flows => &[("↑↓", "move"), ("enter", "rail"), ("←→", "steps"), ("n", "new"), ("D", "duplicate"), ("R", "rename"), ("d d", "delete"), ("r", "read again"), ("ctrl+d", "reset"), ("m", "user ↔ repo"), ("ctrl+f", "filter")],
            Focus::List => &[("↑↓", "move"), ("enter", "edit"), ("←→", "flows"), ("n", "new"), ("D", "duplicate"), ("R", "rename"), ("d d", "delete"), ("r", "read again"), ("ctrl+d", "reset"), ("ctrl+f", "filter")],
            Focus::Rail if self.armed => &[("d", "again: remove the step"), ("any key", "keep")],
            Focus::Rail => &[("a", "add"), ("enter", "edit"), ("J K", "move"), ("d d", "remove"), ("g", "gate"), ("o / O", "outcome / clear"), ("L", "loop"), ("+ -", "max"), ("s", "save as step"), ("esc", "list")],
            Focus::Editor => &[("tab", "next field"), ("ctrl+g", "gate"), ("ctrl+f", "fork shared → one-off"), ("#", "shortcuts"), ("ctrl+s", "save"), ("esc", "close")],
        }
    }

    // ---------- draw ----------

    pub fn draw(&mut self, f: &mut Frame, area: Rect, active: bool) {
        self.rows.clear();
        self.rail_rows.clear();
        self.sub_rects.clear();
        self.field_rects.clear();
        if area.height < 6 || area.width < 40 {
            return;
        }
        // sub-tabs + filter
        let mut x = area.x;
        let mut spans = Vec::new();
        for (s, label) in [(Sub::Flows, "flows"), (Sub::Steps, "steps")] {
            let on = s == self.sub;
            let st = if on { if active && self.focus == Focus::List { theme::pill().add_modifier(Modifier::BOLD) } else { theme::panel().fg(theme::pal().accent).add_modifier(Modifier::BOLD) } } else { theme::muted() };
            let w = label.width() as u16 + 2;
            self.sub_rects.push((Rect { x, y: area.y, width: w, height: 1 }, s));
            x += w + 1;
            spans.push(Span::styled(format!(" {label} "), st));
            spans.push(Span::raw(" "));
        }
        if self.filtering || !self.filter.is_empty() {
            spans.push(Span::styled(format!("  /{}{}", self.filter, if self.filtering { "▏" } else { "" }), theme::accent()));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), Rect { height: 1, ..area });
        let body = Rect { y: area.y + 2, height: area.height - 2, ..area };
        let lw = (body.width / 3).clamp(24, 40);
        let list = Rect { width: lw, ..body };
        self.list_rect = list;
        self.draw_list(f, list, active);
        let right = Rect { x: list.right() + 2, width: body.width.saturating_sub(lw + 2), ..body };
        match self.sub {
            Sub::Flows => self.draw_flow(f, right, active),
            Sub::Steps => self.draw_step(f, right, active),
        }
        if let Some(e) = self.error.clone() {
            f.render_widget(Paragraph::new(Line::styled(crate::nav::fit(&e, usize::from(area.width)), Style::default().fg(theme::pal().error()))), Rect { y: area.bottom() - 1, height: 1, ..area });
        }
        self.draw_ask(f, right);
        self.draw_picker(f, right, area);
    }

    fn draw_list(&mut self, f: &mut Frame, list: Rect, active: bool) {
        let idx = self.list();
        let shown = usize::from(list.height);
        let off = crate::dropdown::fit(0, self.at, idx.len(), shown);
        for (k, &i) in idx.iter().enumerate().skip(off).take(shown) {
            let r = Rect { y: list.y + (k - off) as u16, height: 1, ..list };
            let on = k == self.at;
            let (label, detail) = match self.sub {
                Sub::Flows => {
                    let fl = &self.lib.flows[i];
                    let tag = match (fl.layer, fl.edited) {
                        (Layer::Repo, _) => "repo",
                        (_, true) => "✎",
                        (Layer::User, _) => "★",
                        _ => "",
                    };
                    (fl.name.clone(), format!("{tag} {}", fl.steps.len()))
                }
                Sub::Steps => {
                    let s = &self.lib.steps[i];
                    let n = self.lib.users_of(&s.name).len();
                    let tag = match (s.layer, s.edited) {
                        (Layer::Repo, _) => "repo ",
                        (_, true) => "✎ ",
                        _ => "",
                    };
                    (s.name.clone(), format!("{tag}{n}"))
                }
            };
            let st = match (on, active && self.focus == Focus::List && !self.filtering) {
                (true, true) => theme::pill().add_modifier(Modifier::BOLD),
                (true, false) => theme::panel().add_modifier(Modifier::BOLD),
                _ => theme::text(),
            };
            let dw = detail.width() + 1;
            let label = crate::nav::fit(&format!(" {label}"), usize::from(r.width).saturating_sub(dw + 1));
            let pad = usize::from(r.width).saturating_sub(label.width() + dw);
            let line = Line::from(vec![Span::styled(format!("{label}{}", " ".repeat(pad)), st), Span::styled(format!("{detail} "), if on { st } else { theme::muted() })]);
            f.render_widget(Paragraph::new(line), r);
            self.rows.push((r, k));
        }
        if idx.is_empty() {
            f.render_widget(Paragraph::new(Line::styled(" 0 match · n: new", theme::muted())), Rect { height: 1, ..list });
        }
    }

    fn draw_flow(&mut self, f: &mut Frame, area: Rect, active: bool) {
        let Some(fl) = self.flow.clone() else {
            f.render_widget(Paragraph::new(Line::styled("n: a new flow", theme::muted())), Rect { height: 1, ..area });
            return;
        };
        let nodes = flows::resolve(&fl, &self.lib);
        let layer = match (fl.layer, fl.edited) {
            (_, true) => "edited built-in · ctrl+d: reset".to_string(),
            (l, _) => l.label().to_string(),
        };
        let head = Line::from(vec![
            Span::styled(fl.name.clone(), theme::bold()),
            Span::styled(format!(" · {} steps · {layer}", nodes.len()), theme::muted()),
        ]);
        f.render_widget(Paragraph::new(head), Rect { height: 1, ..area });
        if !fl.description.is_empty() {
            f.render_widget(Paragraph::new(Line::styled(crate::nav::fit(&fl.description, usize::from(area.width)), theme::muted())), Rect { y: area.y + 1, height: 1, ..area });
        }
        let ed_h = if self.editor.is_some() { (area.height / 2).max(10).min(area.height.saturating_sub(6)) } else { 0 };
        let errs: Vec<(usize, String)> = nodes.iter().enumerate().flat_map(|(i, n)| n.errors.iter().map(move |e| (i, e.clone()))).chain(nodes.iter().enumerate().flat_map(|(i, n)| flows::unknown_shortcuts(&n.text).into_iter().map(move |s| (i, format!("#{s}: 0 shortcut"))))).collect();
        let err_h = errs.len().min(3) as u16;
        let rail = Rect { y: area.y + 3, height: area.height.saturating_sub(3 + ed_h + err_h + u16::from(ed_h > 0)), ..area };
        let rows: Vec<flowrail::Row> = rail_rows(&nodes)
            .into_iter()
            .enumerate()
            .map(|(i, mut row)| {
                if errs.iter().any(|(j, _)| *j == i) {
                    row.tags.push(("!".into(), Style::default().fg(theme::pal().error())));
                }
                if self.target.is_some() && i == self.node {
                    row.title_style = theme::accent_bold();
                }
                row
            })
            .collect();
        let arcs = flowrail::arcs(&nodes, |_| 0);
        if rows.is_empty() {
            f.render_widget(Paragraph::new(Line::styled("0 steps · a: add a step", theme::muted())), Rect { height: 1, ..rail });
        }
        self.rail_off = crate::dropdown::fit(self.rail_off, self.node, rows.len(), usize::from(rail.height));
        let focus = active && matches!(self.focus, Focus::Rail);
        let cur = (self.focus != Focus::List).then_some(self.node);
        self.rail_rows = flowrail::draw(f.buffer_mut(), rail, &rows, &arcs, cur, focus, self.rail_off);
        if let Some(t) = self.target {
            let what = match t {
                Target::Loop(i) => format!("loop of {}: back to which step? ↑↓ enter", nodes.get(i).map_or("", |n| n.title.as_str())),
                Target::Outcome(i) => format!("gate outcome of {}: to which step? ↑↓ enter", nodes.get(i).map_or("", |n| n.title.as_str())),
            };
            f.render_widget(Paragraph::new(Line::styled(what, theme::accent_bold())), Rect { y: area.y + 2, height: 1, ..area });
        }
        let mut y = rail.bottom();
        for (i, e) in errs.iter().take(3) {
            f.render_widget(Paragraph::new(Line::styled(crate::nav::fit(&format!("! {}: {e}", i + 1), usize::from(area.width)), Style::default().fg(theme::pal().error()))), Rect { y, height: 1, ..area });
            y += 1;
        }
        if ed_h > 0 {
            let r = Rect { y: y + 1, height: ed_h, ..area };
            self.draw_editor(f, r, active);
        }
    }

    fn draw_step(&mut self, f: &mut Frame, area: Rect, active: bool) {
        if self.editor.is_some() {
            self.draw_editor(f, area, active);
            return;
        }
        let Some(s) = self.cur_step().cloned() else {
            f.render_widget(Paragraph::new(Line::styled("n: a new step", theme::muted())), Rect { height: 1, ..area });
            return;
        };
        let users: Vec<String> = self.lib.users_of(&s.name).iter().map(|f| f.name.clone()).collect();
        let mut lines = vec![
            Line::from(vec![Span::styled(s.name.clone(), theme::bold()), Span::styled(format!(" · {}{}", if s.edited { "edited built-in" } else { s.layer.label() }, if s.gate { " · gate" } else { "" }), theme::muted())]),
            Line::styled(s.description.clone(), theme::muted()),
            Line::raw(""),
        ];
        let kv = |k: &str, v: String| Line::from(vec![Span::styled(format!("{k:<10}"), theme::muted()), Span::styled(v, theme::text())]);
        lines.push(kv("done when", s.done.clone()));
        if !s.needs.is_empty() {
            lines.push(kv("needs", s.needs.join(", ")));
        }
        lines.push(kv("used in", if users.is_empty() { "0 flows".into() } else { users.join(", ") }));
        lines.push(Line::raw(""));
        for l in s.text.lines() {
            lines.push(Line::styled(l.to_string(), theme::text()));
        }
        f.render_widget(Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }), area);
    }

    fn draw_editor(&mut self, f: &mut Frame, r: Rect, active: bool) {
        let Some(ed) = self.editor.as_mut() else { return };
        let (title, warn) = match &ed.what {
            Edit::Step(n) => {
                let users: Vec<String> = self.lib.users_of(n).iter().map(|f| f.name.clone()).collect();
                (format!(" step {n} "), (!users.is_empty()).then(|| format!("! an edit changes {} flows: {}", users.len(), list_names(&users))))
            }
            Edit::Node(i, Some(u)) => {
                let users: Vec<String> = self.lib.users_of(u).iter().map(|f| f.name.clone()).collect();
                (format!(" {} · shared step {u} · used in {} flows ", i + 1, users.len()), Some(format!("! done when + text: an edit changes {}: {} · ctrl+f: fork as a one-off", if users.len() == 1 { "1 flow".to_string() } else { format!("{} flows", users.len()) }, list_names(&users))))
            }
            Edit::Node(i, None) => (format!(" {} · one-off step ", i + 1), None),
        };
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(theme::accent())
            .title(Span::styled(title, theme::accent_bold()))
            .title_bottom(Line::styled(format!(" gate {} · ctrl+g · tab next · ctrl+s save · esc close ", if ed.gate { "on" } else { "off" }), theme::muted()).right_aligned());
        let inner = block.inner(r);
        f.render_widget(Clear, r);
        f.render_widget(block, r);
        let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner };
        let mut y = inner.y;
        if let Some(w) = warn {
            f.render_widget(Paragraph::new(Line::styled(crate::nav::fit(&w, usize::from(inner.width)), theme::fg(theme::pal().caution()))), Rect { y, height: 1, ..inner });
            y += 1;
        }
        let n = ed.fields.len();
        let singles = ed.fields.iter().filter(|(_, t, _)| t.is_single()).count() as u16;
        let multis = (n as u16).saturating_sub(singles).max(1);
        let room = inner.bottom().saturating_sub(y);
        let multi_h = (room.saturating_sub(singles * 2) / multis).saturating_sub(1).max(2);
        let mut cur_rect = Rect::default();
        for (i, (label, ta, shared)) in ed.fields.iter_mut().enumerate() {
            if y >= inner.bottom() {
                break;
            }
            let on = i == ed.field;
            let lst = if on { theme::accent_bold() } else { theme::muted() };
            let label = if *shared { format!("{label} (shared)") } else { label.to_string() };
            f.render_widget(Paragraph::new(Line::styled(label, lst)), Rect { y, height: 1, ..inner });
            y += 1;
            let h = if ta.is_single() { 1 } else { multi_h.min(inner.bottom().saturating_sub(y)) };
            let tr = Rect { y, height: h.max(1).min(inner.bottom().saturating_sub(y).max(1)), ..inner };
            ta.render(f, tr, active && on && self.focus == Focus::Editor);
            self.field_rects.push((tr, i));
            if on {
                cur_rect = tr;
            }
            y += tr.height;
        }
        let ta = &ed.fields[ed.field].1;
        self.shortcuts.render(f, ta, f.area(), cur_rect);
    }

    fn draw_ask(&mut self, f: &mut Frame, right: Rect) {
        let Some((what, ta)) = self.ask.as_mut() else { return };
        let label = match what {
            Ask::NewFlow => "new flow: name".to_string(),
            Ask::DuplicateFlow => "duplicate flow: name".into(),
            Ask::RenameFlow => "flow name".into(),
            Ask::NewStep => "new step: name".into(),
            Ask::DuplicateStep => "duplicate step: name".into(),
            Ask::RenameStep => "rename step (every flow follows)".into(),
            Ask::Promote(_) => "save as shared step: name".into(),
            Ask::LoopUntil(..) => "loop: until (the condition that ends it)".into(),
            Ask::Outcome(..) => "gate outcome: label".into(),
        };
        let w = right.width.min(64);
        let r = Rect { x: right.x, y: right.y + 2, width: w, height: 4 };
        f.render_widget(Clear, r);
        let block = Block::new().borders(Borders::ALL).border_type(BorderType::Plain).border_style(theme::accent()).title(Span::styled(format!(" {label} "), theme::accent_bold())).title_bottom(Line::styled(" enter ok · esc cancel ", theme::muted()).right_aligned());
        let inner = block.inner(r);
        f.render_widget(block, r);
        let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), height: 1, ..inner };
        ta.render(f, inner, true);
    }

    fn draw_picker(&mut self, f: &mut Frame, right: Rect, bounds: Rect) {
        let Some((ta, cur)) = self.picker.as_ref() else { return };
        let rows = self.pick_rows(&ta.text());
        let cur = *cur;
        let w = right.width.min(80);
        let h = (rows.len() as u16 + 4).min(bounds.height.saturating_sub(4)).max(6);
        let r = Rect { x: right.x, y: right.y + 2, width: w, height: h.min(right.height.saturating_sub(2)) };
        f.render_widget(Clear, r);
        let block = Block::new().borders(Borders::ALL).border_type(BorderType::Plain).border_style(theme::accent()).title(Span::styled(" add step ", theme::accent_bold())).title_bottom(Line::styled(" type · ↑↓ · enter add · esc ", theme::muted()).right_aligned());
        let inner = block.inner(r);
        f.render_widget(block, r);
        let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner };
        if let Some((ta, _)) = self.picker.as_mut() {
            ta.render(f, Rect { height: 1, ..inner }, true);
        }
        let list = Rect { y: inner.y + 2, height: inner.height.saturating_sub(2), ..inner };
        let off = crate::dropdown::fit(0, cur, rows.len(), usize::from(list.height));
        for (k, row) in rows.iter().enumerate().skip(off).take(usize::from(list.height)) {
            let rr = Rect { y: list.y + (k - off) as u16, height: 1, ..list };
            let on = k == cur;
            let (label, detail) = match row {
                PickRow::OneOff(t) => (format!("◇ one-off: {t}"), "this flow only · s later: save as step".to_string()),
                PickRow::NewStep(n) => (format!("+ new step {n}"), "shared: every flow can use it".to_string()),
                PickRow::Step(n) => {
                    let s = self.lib.step(n);
                    (format!("✓ {n}"), s.map(|s| format!("{} · {} flows", s.description, self.lib.users_of(n).len())).unwrap_or_default())
                }
            };
            let st = if on { theme::pill().add_modifier(Modifier::BOLD) } else { theme::text() };
            let lw = 32.min(usize::from(rr.width) / 2);
            let line = Line::from(vec![Span::styled(format!("{:<lw$} ", crate::nav::fit(&label, lw)), st), Span::styled(crate::nav::fit(&detail, usize::from(rr.width).saturating_sub(lw + 1)), if on { st } else { theme::muted() })]);
            f.render_widget(Paragraph::new(line).style(if on { st } else { Style::default() }), rr);
        }
    }
}

/// The rail rows of NODES: ✓ a library step (◇ one-off), the gate and
/// loop tags (flowbar::rail_row). The flows tab and the new thread's flow dropdown draw them.
pub fn rail_rows(nodes: &[flows::Resolved]) -> Vec<flowrail::Row> {
    nodes
        .iter()
        .map(|n| {
            let (mark, st) = if n.step.is_some() { ("✓", Style::default().fg(theme::pal().success())) } else { ("◇", theme::muted()) };
            let mut row = crate::flowbar::rail_row(n, mark, st);
            if n.step.is_none() {
                row.tags.retain(|(t, _)| t != "◇");
                row.tags.push(("one-off".into(), theme::muted()));
            }
            row
        })
        .collect()
}

/// The head of flow FL (its NODES): name · n steps · layer.
pub fn head_line(fl: &flows::Flow, nodes: &[flows::Resolved]) -> Line<'static> {
    let layer = match (fl.layer, fl.edited) {
        (_, true) => "edited built-in".to_string(),
        (l, _) => l.label().to_string(),
    };
    Line::from(vec![Span::styled(fl.name.clone(), theme::bold()), Span::styled(format!(" · {} steps · {layer}", nodes.len()), theme::muted())])
}

fn list_names(v: &[String]) -> String {
    if v.len() <= 3 { v.join(", ") } else { format!("{}, +{}", v[..3].join(", "), v.len() - 3) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    fn typed(t: &mut FlowsTab, s: &str) {
        for c in s.chars() {
            t.key(k(KeyCode::Char(c)));
        }
    }

    #[test]
    fn a_new_flow_gets_steps_a_loop_and_a_promoted_one_off() {
        let _g = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = std::fs::remove_dir_all(flows::user_dir("flows").parent().unwrap());
        let mut t = FlowsTab::new(None);
        // n: name → the rail with the step picker open
        t.key(k(KeyCode::Char('n')));
        typed(&mut t, "my flow");
        t.key(k(KeyCode::Enter));
        assert!(t.picker.is_some() && t.focus == Focus::Rail);
        typed(&mut t, "plan");
        t.key(k(KeyCode::Enter));
        // the 2nd step: a one-off, its editor opens
        t.key(k(KeyCode::Char('a')));
        typed(&mut t, "check the docs");
        t.key(k(KeyCode::Enter));
        assert!(matches!(t.editor.as_ref().map(|e| &e.what), Some(Edit::Node(1, None))));
        t.key(k(KeyCode::Esc));
        // L on the one-off: back to step 1, until text
        t.key(k(KeyCode::Char('L')));
        t.key(k(KeyCode::Up));
        t.key(k(KeyCode::Enter));
        typed(&mut t, "docs match");
        t.key(k(KeyCode::Enter));
        let lib = Library::load(None);
        let f = lib.flow("my-flow").unwrap();
        assert_eq!(f.layer, Layer::User);
        let r = flows::resolve(f, &lib);
        assert_eq!(r.len(), 2);
        assert_eq!(r[1].loops.as_ref().map(|l| (l.0, l.1.as_str(), l.2)), Some((0, "docs match", 3)));
        // s: the one-off becomes a shared step
        t.key(k(KeyCode::Char('s')));
        t.key(k(KeyCode::Enter));
        let lib = Library::load(None);
        assert_eq!(lib.flow("my-flow").unwrap().steps[1].uses.as_deref(), Some("check-the-docs"));
        assert_eq!(lib.step("check-the-docs").unwrap().text, "check the docs");
        let _ = std::fs::remove_dir_all(flows::user_dir("flows").parent().unwrap());
    }

    #[test]
    fn editing_a_shared_step_from_a_flow_changes_it_for_every_flow() {
        let _g = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = std::fs::remove_dir_all(flows::user_dir("flows").parent().unwrap());
        let mut t = FlowsTab::new(None);
        t.open("chore");
        // enter on step 1 (implement): the text field has the focus
        t.key(k(KeyCode::Enter));
        assert!(matches!(t.editor.as_ref().map(|e| &e.what), Some(Edit::Node(0, Some(_)))));
        typed(&mut t, " ZZ");

        // esc: 0 save, the editor stays (edits); ctrl+s saves
        t.key(k(KeyCode::Esc));
        assert!(t.editor.is_some() && !Library::load(None).step("implement").unwrap().text.ends_with(" ZZ"));

        t.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        let lib = Library::load(None);
        assert!(lib.step("implement").unwrap().text.ends_with(" ZZ"));
        assert!(lib.step("implement").unwrap().edited);
        // the other flows see it too
        let nf = lib.flow("new-feature").unwrap();
        assert!(flows::resolve(nf, &lib).iter().any(|n| n.text.ends_with(" ZZ")));
        let _ = std::fs::remove_dir_all(flows::user_dir("flows").parent().unwrap());
    }
}
