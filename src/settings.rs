//! Settings form. Changes apply on leaving a field (or an alias box): codehead.toml is written, the
//! app applies it. Invalid values: 0 write, the error shows. Esc: the field as it was at entry, 0 write.
//! tab / shift+tab: next / previous field.
use std::collections::BTreeMap;
use std::path::PathBuf;

use ratatui::Frame;
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use crate::textarea::TextArea;
use crate::{config, theme};

/// A plain field: its config key, label, hint, env var that overrides it ("": none).
struct Spec {
    key: &'static str,
    label: &'static str,
    help: &'static str,
    env: &'static str,
}

const fn spec(key: &'static str, label: &'static str, help: &'static str, env: &'static str) -> Spec {
    Spec { key, label, help, env }
}

/// The plain fields, then the alias boxes.
const SPECS: [Spec; 26] = [
    spec("notes", "notes", "directory of Markdown notes, ~ expands to home", "CODEHEAD_NOTES"),
    spec("styles", "styles", "directory of agent output styles", "CODEHEAD_STYLES"),
    spec("home", "home repo", "git repo with AGENTS.md (notes, other repos): every agent reads it", "CODEHEAD_HOME_REPO"),
    spec("agent", "agent", "agent picked for a new thread", "CODEHEAD_AGENT"),
    spec("prefix", "prefix", "prefix of new branches and worktrees", "CODEHEAD_PREFIX"),
    spec("models", "models", "Claude models of the agent form, comma-separated", "CODEHEAD_MODELS"),
    spec("ports", "ports", "dev servers: auto (portless when installed), portless, or block ($CODEHEAD_PORT only)", "CODEHEAD_PORTS"),
    spec("messages", "messages", "bubbles (boxes, yours on the right) or lines (name, bar)", "CODEHEAD_MESSAGES"),
    spec("thoughts", "thoughts", "hidden, folded (a click opens) or open · ctrl+t in a thread", "CODEHEAD_THOUGHTS"),
    spec("edits", "your edits", "what an agent gets told of edits since its last turn: off, files or diff", "CODEHEAD_EDITS"),
    spec("rank", "review order", "changes view: claude:<model> groups and orders the files, off: path rules", "CODEHEAD_RANK"),
    spec("review", "reviewer", "changes view V: claude:<model> reviews the diff (read-only tools), off: 0 reviews", "CODEHEAD_REVIEW"),
    spec("editor", "editor", "IDE for ctrl+i and the changes view o: auto, or code, cursor, zed, idea …, or a path", "CODEHEAD_EDITOR"),
    spec("keys.menu", "menu", "opens the menu: F1-F12, ctrl+<letter>, alt+<letter>", "CODEHEAD_MENU_KEY"),
    spec("keys.command", "commands", "opens the command dropdown in a prompt: 1 symbol", "CODEHEAD_COMMAND_KEY"),
    spec("colors.scheme", "scheme", "preset: coral or amber", "CODEHEAD_SCHEME"),
    spec("colors.accent", "accent", "the 1 accent: titles, pills, the cursor row", ""),
    spec("colors.on_accent", "on_accent", "text on an accent pill", ""),
    spec("colors.panel", "panel", "inputs, unfocused tabs and rows", ""),
    spec("colors.hover", "hover", "the row under the mouse, the input bubble's border", ""),
    spec("colors.text", "text", "normal text", ""),
    spec("colors.muted", "muted", "hints, labels, times", ""),
    spec("colors.line", "line", "borders, dividers", ""),
    spec("colors.ok", "ok", "done, allowed", ""),
    spec("colors.danger", "danger", "errors", ""),
    spec("colors.warn", "warn", "blocked, questions", ""),
];
pub(crate) const PLAIN: usize = SPECS.len();
/// Groups: the 1st field, the title, what the group sets.
const SECTIONS: [(usize, &str, &str); 5] = [
    (0, "paths", "where codehead reads notes, output styles and the home repo"),
    (3, "new threads", "defaults of the new tab and the agent form"),
    (7, "thread view", "how messages and thoughts look, your edits, the review order, the IDE"),
    (13, "keys", "keys of every screen"),
    (15, "colors", "a preset; a set field below overrides 1 of its colors"),
];
/// The alias group, after the plain fields.
const ALIASES: (&str, &str) = ("path aliases", "short names for directories: name/ → target/ · enter: edit / done · d: delete · ctrl+n: add");
/// Fields and boxes: this many columns right of the group's heading.
const INDENT: u16 = 2;

/// Group heading on ROWS (2 rows): `TITLE ────` (accent), what it sets (muted).
fn group_head(f: &mut Frame, row: Option<Rect>, desc_row: Option<Rect>, title: &str, desc: &str) {
    if let Some(r) = row {
        let rule = usize::from(r.width).saturating_sub(title.chars().count() + 1);
        f.render_widget(Paragraph::new(Line::from(vec![Span::styled(title.to_string(), theme::accent_bold()), Span::styled(format!(" {}", "─".repeat(rule)), theme::line())])), r);
    }
    if let Some(r) = desc_row {
        f.render_widget(Paragraph::new(Line::styled(desc.to_string(), theme::muted())), r);
    }
}

pub enum Action {
    None,
    Header,
}

/// A click target of the last draw.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit {
    /// A plain field (0..PLAIN) or a field of an alias box (PLAIN..).
    Field(usize),
    /// The ↺ of plain field I: back to its default.
    Reset(usize),
    /// The "+ add path alias" row.
    Add,
}

pub struct Settings {
    path: PathBuf,
    base: Option<String>,
    saved: config::File,
    /// 6 plain fields, then 2 per alias (name, target).
    pub(crate) fields: Vec<TextArea>,
    /// Navigation item: 0..6 plain fields, 6..6+n alias boxes, 6+n the add row.
    pub(crate) focus: usize,
    /// The field of the open (unlocked) alias box.
    pub(crate) editing: Option<usize>,
    offset: usize,
    rows: Vec<(Rect, Hit)>,
    status: String,
    error: bool,
    paths: crate::paths::Paths,
    path_focus: Option<usize>,
    /// Written to codehead.toml, not yet applied by the app.
    applied: Option<config::File>,
    /// The focused field and its text at entry: Esc puts it back (0 commit).
    entry: Option<(usize, String)>,
    /// The open alias box at its unlock: (its 1st field, name, target): Esc puts both back.
    pair_entry: Option<(usize, String, String)>,
}

impl Settings {
    pub fn new(path: PathBuf) -> Self {
        let mut form = Self {
            path,
            base: None,
            saved: config::File::default(),
            fields: Vec::new(),
            focus: 0,
            editing: None,
            offset: 0,
            rows: Vec::new(),
            status: String::new(),
            error: false,
            paths: crate::paths::Paths::directory(&config::home()),
            path_focus: None,
            applied: None,
            entry: None,
            pair_entry: None,
        };
        if form.reload().is_none() {
            form.fields = (0..PLAIN).map(|_| Self::input("", "use default")).collect();
        }
        form
    }

    fn input(text: &str, hint: &str) -> TextArea {
        let mut input = TextArea::new(true, hint);
        input.set_text(text);
        input.move_start();
        input
    }

    fn aliases(&self) -> usize {
        self.fields.len().saturating_sub(PLAIN) / 2
    }

    /// 1st navigation item of each section: general, the SECTIONS, the aliases.
    fn starts(&self) -> Vec<usize> {
        SECTIONS.iter().map(|s| s.0).chain([PLAIN]).collect()
    }

    /// The add row: the last navigation item.
    fn add_item(&self) -> usize {
        PLAIN + self.aliases()
    }

    /// The text field keys go to: a plain field, or a field of the open alias box.
    fn field(&self) -> Option<usize> {
        if self.focus < PLAIN { Some(self.focus) } else { self.editing }
    }

    /// The alias box of field I (≥ 6).
    fn alias_of(i: usize) -> usize {
        (i - PLAIN) / 2
    }

    fn reload(&mut self) -> Option<config::File> {
        match config::read_file(&self.path) {
            Ok((text, file)) => {
                self.fields = SPECS.iter().map(|sp| Self::input(&file.get(sp.key).unwrap_or_default(), &format!("default: {}", Self::default_of(&file, sp.key)))).collect();
                for (name, path) in &file.aliases {
                    self.push_alias(name, path);
                }
                self.base = Some(text);
                self.path_focus = None;
                self.editing = None;
                self.paths.reset_input();
                self.saved = file.clone();
                self.focus = self.focus.min(self.add_item());
                self.status = "changes apply on leaving a field · ctrl+d / ↺: default · environment overrides file values".into();
                self.error = false;
                Some(file)
            }
            Err(e) => {
                self.status = e;
                self.error = true;
                None
            }
        }
    }

    /// The default of setting KEY, as the field's placeholder shows it. Colors: the scheme's.
    fn default_of(file: &config::File, key: &str) -> String {
        if let Some(name) = key.strip_prefix("colors.").filter(|n| *n != "scheme") {
            let scheme = file.scheme.clone().unwrap_or_else(|| theme::PRESETS[0].0.into());
            let p = theme::Palette::preset(&scheme).unwrap_or(theme::PRESETS[0].1);
            return p.get(name).map(|c| format!("{} ({scheme})", theme::hex(c))).unwrap_or_default();
        }
        config::defaults().get(key).unwrap_or_else(|| "amon, else first agent".into())
    }

    /// Plain field I back to its default: blank, applied.
    fn reset(&mut self, i: usize) {
        if i >= PLAIN || self.fields[i].text().is_empty() {
            return;
        }
        self.fields[i].set_text("");
        self.commit();
        if !self.error {
            self.status = format!("{}: default", SPECS[i].key);
        }
    }

    fn push_alias(&mut self, name: &str, target: &str) {
        self.fields.push(Self::input(name, "project:platform"));
        self.fields.push(Self::input(target, "~/projects/exampleorg/project/notes/platform/"));
    }

    fn values(&self) -> Result<config::File, String> {
        if self.fields.len() < PLAIN {
            return Err("reload config before save".into());
        }
        let value = |i: usize| {
            let text = self.fields[i].text().trim().to_string();
            (!text.is_empty()).then_some(text)
        };
        let mut aliases = BTreeMap::new();
        for pair in self.fields[PLAIN..].as_chunks::<2>().0 {
            let name = pair[0].text().trim().to_string();
            let target = pair[1].text().trim().to_string();
            // a new box, 0 text yet: not an alias
            if name.is_empty() && target.is_empty() {
                continue;
            }
            config::validate_alias(&name, &target)?;
            if aliases.insert(name.clone(), target).is_some() {
                return Err(format!("duplicate alias: {name}"));
            }
        }
        let mut file = config::File { aliases, ..config::File::default() };
        for (i, sp) in SPECS.iter().enumerate() {
            let v = value(i);
            if let Some(v) = &v {
                config::validate(sp.key.to_string(), v)?;
            }
            file.set(sp.key, v);
        }
        Ok(file)
    }

    fn dirty(&self) -> bool {
        self.values().ok().is_none_or(|value| value != self.saved)
    }

    /// Changes → codehead.toml (the app applies them). Invalid values: 0 write, the error shows.
    pub fn commit(&mut self) {
        if !self.dirty() {
            return;
        }
        let result = (|| {
            let file = self.values()?;
            let base = self.base.as_deref().ok_or("reload config before save")?;
            let text = config::save_file(&self.path, base, &file)?;
            Ok::<_, String>((text, file))
        })();
        match result {
            Ok((text, file)) => {
                self.base = Some(text);
                self.saved = file.clone();
                self.status = "settings applied".into();
                self.error = false;
                self.applied = Some(file);
            }
            Err(e) => {
                self.status = format!("not applied: {e}");
                self.error = true;
            }
        }
    }

    /// Settings written since the last call: for the app to apply.
    pub fn take_applied(&mut self) -> Option<config::File> {
        self.applied.take()
    }

    /// Focus D items on (clamped: the 1st field, the add row).
    fn step(&mut self, d: isize) {
        self.focus = (self.focus as isize + d).clamp(0, self.add_item() as isize) as usize;
    }

    fn add_alias(&mut self) {
        self.commit();
        self.push_alias("", "");
        self.focus = self.add_item() - 1;
        self.editing = Some(self.fields.len() - 2);
    }

    fn delete_alias(&mut self, k: usize) {
        let at = PLAIN + k * 2;
        if at + 2 > self.fields.len() {
            return;
        }
        self.fields.drain(at..at + 2);
        self.editing = None;
        self.focus = self.focus.min(self.add_item());
        self.commit();
    }

    pub fn key(&mut self, key: KeyEvent) -> Action {
        let (before, was_editing) = (self.field(), self.editing);
        let action = self.key_inner(key);
        // an alias box unlocked: its name + target for an Esc
        if was_editing.is_none()
            && let Some(i) = self.editing
        {
            let at = PLAIN + Self::alias_of(i) * 2;
            self.pair_entry = Some((at, self.fields[at].text(), self.fields[at + 1].text()));
        }
        if self.field() != before {
            self.focus_input();
        }
        action
    }

    /// Keyboard entry into an input starts before its existing text; its text then: Esc's target.
    pub fn focus_input(&mut self) {
        if let Some(i) = self.field() {
            self.fields[i].move_start();
        }
        self.entry = self.field().map(|i| (i, self.fields[i].text()));
        self.paths.reset_input();
    }

    /// Esc: the focused field's text as it was at entry (an open alias box: both its fields), 0
    /// commit. Esc never applies.
    fn revert(&mut self) {
        if let Some(i) = self.editing {
            if let Some((at, name, target)) = self.pair_entry.take()
                && at == PLAIN + Self::alias_of(i) * 2
                && (self.fields[at].text() != name || self.fields[at + 1].text() != target)
            {
                self.fields[at].set_text(&name);
                self.fields[at + 1].set_text(&target);
                self.status = "alias edit undone".into();
            }
            self.editing = None;
            return;
        }
        if let Some((i, text)) = self.entry.clone()
            && Some(i) == self.field()
            && self.fields[i].text() != text
        {
            self.fields[i].set_text(&text);
            self.fields[i].move_start();
            self.status = format!("{}: edit undone", SPECS.get(i).map_or("field", |s| s.key));
        }
    }

    fn key_inner(&mut self, key: KeyEvent) -> Action {
        if self.suggestions_key(key) {
            return Action::None;
        }
        let command = key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER);
        if command {
            match key.code {
                KeyCode::Char('s' | 'S') => {
                    self.commit();
                    return Action::None;
                }
                KeyCode::Char('r' | 'R') => {
                    self.applied = self.reload();
                    return Action::None;
                }
                KeyCode::Char('n' | 'N') => {
                    self.add_alias();
                    return Action::None;
                }
                KeyCode::Char('d' | 'D') if self.focus < PLAIN => {
                    self.reset(self.focus);
                    return Action::None;
                }
                KeyCode::Char('d' | 'D') if (PLAIN..self.add_item()).contains(&self.focus) => {
                    self.delete_alias(self.focus - PLAIN);
                    return Action::None;
                }
                _ => {}
            }
        }
        if let Some(i) = self.editing {
            // the open alias box: ↑↓ name / target, enter / esc: lock it again
            match key.code {
                KeyCode::Enter => {
                    self.editing = None;
                    self.commit();
                }
                // esc: the alias as it was at unlock, locked, 0 commit
                KeyCode::Esc => self.revert(),
                KeyCode::Up if key.modifiers.is_empty() && !(i - PLAIN).is_multiple_of(2) => self.editing = Some(i - 1),
                KeyCode::Down if key.modifiers.is_empty() && (i - PLAIN).is_multiple_of(2) => self.editing = Some(i + 1),
                KeyCode::Up | KeyCode::Down if key.modifiers.is_empty() => {}
                // tab: done with the alias, the next item
                KeyCode::Tab | KeyCode::BackTab => {
                    self.editing = None;
                    self.commit();
                    self.step(if key.code == KeyCode::Tab { 1 } else { -1 });
                }
                _ => {
                    self.fields[i].key(key);
                }
            }
            return Action::None;
        }
        match key.code {
            // tab / shift+tab: the next / previous field (main tabs: ctrl+←→)
            KeyCode::Tab => {
                self.commit();
                self.step(1);
            }
            KeyCode::BackTab => {
                self.commit();
                if self.focus == 0 {
                    return Action::Header;
                }
                self.step(-1);
            }
            // esc: the field's edit undone, 0 commit
            KeyCode::Esc => self.revert(),
            KeyCode::Up if key.modifiers.is_empty() => {
                self.commit();
                if self.focus == 0 {
                    return Action::Header;
                }
                self.focus -= 1;
            }
            KeyCode::Down if key.modifiers.is_empty() => {
                self.commit();
                self.focus = (self.focus + 1).min(self.add_item());
            }
            KeyCode::PageUp => {
                self.commit();
                self.focus = self.starts().into_iter().rev().find(|&s| s < self.focus).unwrap_or(0);
            }
            KeyCode::PageDown => {
                self.commit();
                self.focus = self.starts().into_iter().find(|&s| s > self.focus).unwrap_or(self.add_item());
            }
            KeyCode::Enter if self.focus == self.add_item() => self.add_alias(),
            KeyCode::Enter if self.focus >= PLAIN => self.editing = Some(PLAIN + (self.focus - PLAIN) * 2),
            KeyCode::Enter => {
                self.commit();
                self.focus = (self.focus + 1).min(self.add_item());
            }
            KeyCode::Char('d') if key.modifiers.is_empty() && (PLAIN..self.add_item()).contains(&self.focus) => {
                self.delete_alias(self.focus - PLAIN);
            }
            _ => {
                if let Some(i) = self.field() {
                    self.fields[i].key(key);
                }
            }
        }
        Action::None
    }

    pub fn paste(&mut self, text: &str) {
        if let Some(i) = self.field() {
            self.fields[i].insert(&text.replace(['\n', '\r'], ""));
        }
    }

    pub fn row_at(&self, at: Position) -> Option<Rect> {
        self.paths.row_at(at).or_else(|| {
            self.rows
                .iter()
                .find(|(r, _)| r.contains(at))
                .map(|(r, _)| *r)
        })
    }

    fn sync_paths(&mut self) {
        let focus = self.field().filter(|&i| i < 2 || (i >= PLAIN && !(i - PLAIN).is_multiple_of(2)));
        if focus != self.path_focus {
            self.paths.reset_input();
            self.path_focus = focus;
        }
    }

    pub fn suggestions_key(&mut self, key: KeyEvent) -> bool {
        self.sync_paths();
        self.path_focus
            .is_some_and(|index| self.paths.key(key, &mut self.fields[index]))
    }

    pub fn mouse(&mut self, event: MouseEvent) -> Action {
        self.sync_paths();
        if self
            .path_focus
            .is_some_and(|index| self.paths.mouse(event, &mut self.fields[index]))
        {
            return Action::None;
        }
        let at = Position {
            x: event.column,
            y: event.row,
        };
        match event.kind {
            MouseEventKind::ScrollUp => {
                self.offset = self.offset.saturating_sub(3);
                return Action::None;
            }
            MouseEventKind::ScrollDown => {
                self.offset += 3;
                return Action::None;
            }
            _ => {}
        }
        if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
            && let Some(&(_, hit)) = self.rows.iter().find(|(r, _)| r.contains(at))
        {
            match hit {
                Hit::Add => {
                    self.focus = self.add_item();
                    self.add_alias();
                    return Action::None;
                }
                Hit::Reset(i) => {
                    self.focus = i;
                    self.editing = None;
                    self.reset(i);
                    return Action::None;
                }
                Hit::Field(i) if i < PLAIN => {
                    if self.focus != i { self.commit(); }
                    self.editing = None;
                    self.focus = i;
                }
                Hit::Field(i) => {
                    let k = Self::alias_of(i);
                    if self.editing.is_none_or(|e| Self::alias_of(e) != k) { self.commit(); }
                    self.focus = PLAIN + k;
                    self.editing = Some(i);
                }
            }
        }
        if let Some(i) = self.field() {
            self.fields[i].mouse(event);
        }
        Action::None
    }

    pub fn draw(&mut self, f: &mut Frame, area: Rect, active: bool) {
        self.sync_paths();
        self.paths.hide();
        self.rows.clear();
        // Clear stale input areas when scrolling; only visible inputs accept clicks.
        for input in &mut self.fields {
            input.hide();
        }
        if area.height < 5 || area.width < 16 {
            return;
        }
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("settings", theme::accent_bold()),
                Span::styled(if self.dirty() { " · not applied" } else { "" }, theme::muted()),
            ])),
            Rect { height: 1, ..area },
        );
        f.render_widget(
            Paragraph::new(Line::styled(self.path.display().to_string(), theme::muted())),
            Rect { y: area.y + 1, height: 1, ..area },
        );
        let all = Rect { y: area.y + 3, height: area.height - 4, ..area };
        // fields and boxes: indented under their group's heading
        let body = Rect { x: all.x + INDENT, width: all.width.saturating_sub(INDENT), ..all };
        // virtual rows: per group its heading (2) + 1 blank, 3 per plain field, the alias heading
        // (2) + 1 blank, 4 per alias box, the add row
        let n = self.aliases();
        let mut items = Vec::new();
        let mut y = 0usize;
        let mut heads = Vec::new();
        for i in 0..PLAIN {
            if let Some((_, title, desc)) = SECTIONS.iter().find(|s| s.0 == i) {
                heads.push((y, *title, *desc));
                y += 3;
            }
            items.push((y, 2));
            y += 3;
        }
        let heading = y;
        y += 3;
        for _ in 0..n {
            items.push((y, 4));
            y += 4;
        }
        items.push((y, 1));
        let (top, height) = items[self.focus.min(items.len() - 1)];
        let view = body.height as usize;
        if top < self.offset {
            self.offset = top;
        }
        if top + height > self.offset + view {
            self.offset = (top + height).saturating_sub(view);
        }
        self.offset = self.offset.min((y + 1).saturating_sub(view));
        let offset = self.offset;
        // RECT at virtual row Y (HEIGHT rows), if fully in view
        let at = |y: usize, height: usize| -> Option<Rect> {
            let off = y.checked_sub(offset)?;
            (off + height <= view).then_some(Rect { y: body.y + off as u16, height: height as u16, ..body })
        };
        let label_width = 12.min(body.width / 3);
        let full = |r: Rect| Rect { x: all.x, width: all.width, ..r };
        for (hy, title, desc) in heads {
            group_head(f, at(hy, 1).map(full), at(hy + 1, 1).map(full), title, desc);
        }
        let pal = theme::pal();
        for i in 0..PLAIN {
            let Some(row) = at(items[i].0, 1) else { continue };
            let focused = active && self.focus == i;
            f.render_widget(
                Paragraph::new(Line::styled(SPECS[i].label, if focused { theme::accent_bold() } else { theme::muted() })),
                Rect { width: label_width, ..row },
            );
            // right of the input: a color's swatch, ↺ (back to the default) while the field is set
            let swatch = SPECS[i].key.strip_prefix("colors.").and_then(|n| pal.get(n));
            let tail = 3 + if swatch.is_some() { 3 } else { 0 };
            let input = Rect { x: row.x + label_width, width: row.width.saturating_sub(label_width + tail), ..row };
            f.render_widget(Paragraph::new("").style(theme::panel()), input);
            self.fields[i].render(f, input, focused);
            let mut x = input.right() + 1;
            if let Some(c) = swatch {
                f.render_widget(Paragraph::new(Span::styled("██", ratatui::style::Style::default().fg(c))), Rect { x, width: 2, ..row });
                x += 3;
            }
            if !self.fields[i].text().is_empty() {
                let r = Rect { x, width: 1, ..row };
                f.render_widget(Paragraph::new(Span::styled("↺", if focused { theme::accent() } else { theme::muted() })), r);
                self.rows.push((r, Hit::Reset(i)));
            }
            self.rows.push((row, Hit::Field(i)));
            if let Some(hint_row) = at(items[i].0 + 1, 1) {
                let env = SPECS[i].env;
                let hint = (!env.is_empty())
                    .then(|| std::env::var(env).ok().filter(|s| !s.is_empty()))
                    .flatten()
                    .map(|s| format!("{env} override: {s}"))
                    .unwrap_or_else(|| {
                        let reset = if focused && !self.fields[i].text().is_empty() { " · ctrl+d / ↺: default" } else { "" };
                        format!("{}{reset}", SPECS[i].help)
                    });
                f.render_widget(
                    Paragraph::new(Line::styled(hint, theme::muted())),
                    Rect { x: hint_row.x + label_width, width: hint_row.width - label_width, ..hint_row },
                );
            }
        }
        group_head(f, at(heading, 1).map(full), at(heading + 1, 1).map(full), ALIASES.0, ALIASES.1);
        for k in 0..n {
            let item = PLAIN + k;
            let Some(r) = at(items[item].0, 4) else { continue };
            let on = active && self.focus == item;
            let open = on && self.editing.is_some();
            let border = if on { theme::accent_bold() } else { theme::line() };
            let title = match (on, open) {
                (true, true) => format!(" alias {} · enter: done ", k + 1),
                (true, false) => format!(" alias {} · enter: edit · d: delete ", k + 1),
                _ => format!(" alias {} ", k + 1),
            };
            let block = Block::new()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(border)
                .title(Span::styled(title, if on { theme::accent_bold() } else { theme::muted() }));
            let inner = block.inner(r);
            f.render_widget(block, r);
            if on && !open {
                // locked + highlighted: the whole box
                f.render_widget(Paragraph::new("").style(theme::panel()), inner);
            }
            for (j, label) in ["name", "target"].into_iter().enumerate() {
                let i = PLAIN + k * 2 + j;
                let row = Rect { y: inner.y + j as u16, height: 1, x: inner.x + 1, width: inner.width.saturating_sub(2) };
                let focused = open && self.editing == Some(i);
                let lw = 8.min(row.width / 3);
                f.render_widget(
                    Paragraph::new(Line::styled(label, if focused { theme::accent_bold() } else { theme::muted() })),
                    Rect { width: lw, ..row },
                );
                let input = Rect { x: row.x + lw, width: row.width - lw, ..row };
                if open {
                    f.render_widget(Paragraph::new("").style(theme::panel()), input);
                }
                self.fields[i].render(f, input, focused);
                self.rows.push((row, Hit::Field(i)));
            }
        }
        if let Some(row) = at(items[self.add_item()].0, 1) {
            let focused = active && self.focus == self.add_item();
            f.render_widget(
                Paragraph::new(Line::styled(" + add path alias (ctrl+n) ", if focused { theme::pill() } else { theme::panel() })),
                row,
            );
            self.rows.push((row, Hit::Add));
        }
        f.render_widget(
            Paragraph::new(Line::styled(
                self.status.clone(),
                if self.error { theme::text().fg(theme::pal().danger) } else { theme::muted() },
            )),
            Rect { y: area.bottom() - 1, height: 1, ..area },
        );
        if active && let Some(index) = self.path_focus {
            let input = &self.fields[index];
            self.paths.render(f, input, area, input.area());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> (PathBuf, Settings) {
        let dir = std::env::temp_dir().join(format!("codehead-settings-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("codehead.toml");
        std::fs::write(&path, "# config\nnotes = '~/notes'\n").unwrap();
        let form = Settings::new(path);
        (dir, form)
    }

    fn key(form: &mut Settings, code: KeyCode) -> Action {
        form.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn file_aliases(dir: &std::path::Path) -> usize {
        config::read_file(&dir.join("codehead.toml")).unwrap().1.aliases.len()
    }

    #[test]
    fn keyboard_entry_starts_before_existing_field_text() {
        let (dir, mut form) = fixture("cursor-start");
        std::fs::write(dir.join("codehead.toml"), "notes = '~/notes/'\nstyles = '~/styles/'\nagent = 'amon'\nprefix = 'task'\nmodels = 'opus'\nmessages = 'lines'\n[aliases]\ndocs = '~/docs/'\n").unwrap();
        form.reload().unwrap();

        let at_start = |form: &mut Settings, i: usize| {
            assert_eq!(form.field(), Some(i));
            assert!(form.fields[i].path_word().is_none());
            let text = form.fields[i].text();
            for _ in text.chars() {
                form.fields[i].key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
            }
            assert_eq!(form.fields[i].selected_text(), Some(text));
            form.fields[i].move_start();
        };
        at_start(&mut form, 0);
        for i in 1..PLAIN {
            form.fields[i].move_end();
            key(&mut form, KeyCode::Down);
            if form.fields[i].text().is_empty() {
                assert_eq!(form.field(), Some(i));
            } else {
                at_start(&mut form, i);
            }
        }
        key(&mut form, KeyCode::Down);
        for i in PLAIN..PLAIN + 2 { form.fields[i].move_end(); }
        key(&mut form, KeyCode::Enter);
        at_start(&mut form, PLAIN);
        key(&mut form, KeyCode::Down);
        at_start(&mut form, PLAIN + 1);
        key(&mut form, KeyCode::Up);
        at_start(&mut form, PLAIN);
        key(&mut form, KeyCode::Enter);
        // pgup: the start of the section before (colors, keys, general)
        for _ in 0..SECTIONS.len() {
            key(&mut form, KeyCode::PageUp);
        }
        at_start(&mut form, 0);
        form.fields[0].move_end();
        form.focus_input();
        at_start(&mut form, 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn fields_sit_in_groups_under_a_heading() {
        let (dir, mut form) = fixture("groups");
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 120)).unwrap();
        terminal.draw(|f| form.draw(f, f.area(), true)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let lines: Vec<String> = (0..120).map(|y| (0..90).map(|x| buffer[(x, y)].symbol()).collect::<String>()).collect();
        let at = |s: &str| lines.iter().position(|l| l.trim_start().starts_with(s)).unwrap_or_else(|| panic!("{s}:\n{}", lines.join("\n")));
        // headings in order, each with a rule; its fields below it, indented
        let heads: Vec<usize> = ["paths ─", "new threads ─", "thread view ─", "keys ─", "colors ─", "path aliases ─"].iter().map(|h| at(h)).collect();
        assert!(heads.windows(2).all(|w| w[0] < w[1]), "{heads:?}");
        for (field, group) in [("notes", 0), ("agent", 1), ("thoughts", 2), ("menu", 3), ("scheme", 4)] {
            let y = at(field);
            assert!(heads[group] < y && y < heads[group + 1], "{field}");
            assert!(lines[y].starts_with(&format!("{}{field}", " ".repeat(INDENT as usize))), "{field}: {:?}", lines[y]);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn ctrl_d_and_the_reset_mark_set_a_field_back_to_its_default() {
        let (dir, mut form) = fixture("reset");
        let file = dir.join("codehead.toml");
        std::fs::write(&file, "messages = 'lines'\n[keys]\nmenu = 'ctrl+p'\n[colors]\nscheme = 'amber'\n").unwrap();
        form.reload().unwrap();
        form.focus = 7;
        form.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
        assert_eq!(form.fields[7].text(), "");
        let saved = config::read_file(&file).unwrap().1;
        assert_eq!((saved.messages, saved.menu_key.as_deref()), (None, Some("ctrl+p")));
        // a click on ↺ of the scheme field
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 100)).unwrap();
        terminal.draw(|f| form.draw(f, f.area(), true)).unwrap();
        let (r, _) = *form.rows.iter().find(|(_, h)| *h == Hit::Reset(15)).expect("↺ on the set scheme field");
        form.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: r.x, row: r.y, modifiers: KeyModifiers::NONE });
        assert_eq!(config::read_file(&file).unwrap().1.scheme, None);
        assert!(!form.rows.iter().any(|(_, h)| *h == Hit::Reset(7)));
        // an invalid value: 0 write, the error shows
        form.focus = 16;
        form.fields[16].set_text("red");
        form.commit();
        assert!(form.error && form.status.contains("#RRGGBB"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn alias_boxes_lock_apply_on_leave_and_delete_with_d() {
        let (dir, mut form) = fixture("save");
        assert!(!form.dirty());
        // ctrl+n: a new box, open on its name
        form.key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
        assert_eq!((form.focus, form.editing), (PLAIN, Some(PLAIN)));
        form.paste("project:platform");
        // enter locks the box: 0 target → not applied, the error shows
        key(&mut form, KeyCode::Enter);
        assert_eq!(form.editing, None);
        assert!(form.error && form.take_applied().is_none());
        assert_eq!(file_aliases(&dir), 0);
        // a locked box: ↑↓ pass it, typing does not reach its fields
        key(&mut form, KeyCode::Char('x'));
        assert_eq!(form.fields[PLAIN].text(), "project:platform");
        key(&mut form, KeyCode::Enter);
        key(&mut form, KeyCode::Down);
        assert_eq!(form.editing, Some(PLAIN + 1));
        form.paste("~/projects/exampleorg/project/notes/platform/");
        key(&mut form, KeyCode::Enter);
        let file = form.take_applied().expect("applied on lock");
        assert_eq!(file.aliases["project:platform"], "~/projects/exampleorg/project/notes/platform/");
        assert!(!form.dirty() && !form.error);
        assert!(std::fs::read_to_string(dir.join("codehead.toml")).unwrap().contains("# config"));
        // d on the highlighted box: deleted + applied
        key(&mut form, KeyCode::Char('d'));
        assert_eq!(form.aliases(), 0);
        assert_eq!(file_aliases(&dir), 0);
        assert!(form.take_applied().is_some());
        // a plain field applies when the cursor leaves it; d there is text
        form.focus = 0;
        form.fields[0].set_text("");
        form.fields[0].move_end();
        key(&mut form, KeyCode::Char('d'));
        assert_eq!(form.fields[0].text(), "d");
        form.fields[0].set_text("");
        key(&mut form, KeyCode::Down);
        assert_eq!(form.focus, 1);
        assert_eq!(config::read_file(&dir.join("codehead.toml")).unwrap().1.notes, None);
        // ↑ from the 1st field: the tabs, the edit applied
        form.focus = 0;
        assert!(matches!(key(&mut form, KeyCode::Up), Action::Header));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn form_scrolls_to_controls_and_handles_invalid_reload() {
        let (dir, mut form) = fixture("render");
        for _ in 0..10 {
            form.key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
        }
        form.editing = None;
        form.focus = form.add_item();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(50, 12)).unwrap();
        terminal.draw(|f| form.draw(f, f.area(), true)).unwrap();
        assert!(form.rows.iter().any(|(_, hit)| *hit == Hit::Add));
        assert!(form.offset > 0);
        std::fs::write(dir.join("codehead.toml"), "notes = 4\n").unwrap();
        assert!(form.reload().is_none());
        assert!(form.error);
        assert_eq!(form.fields.len(), PLAIN + 20);
        let mut invalid = Settings::new(dir.join("codehead.toml"));
        terminal.draw(|f| invalid.draw(f, f.area(), true)).unwrap();
        assert!(invalid.error);
        invalid.fields[0].set_text("~/x");
        invalid.commit();
        assert!(invalid.take_applied().is_none());
        assert_eq!(std::fs::read_to_string(dir.join("codehead.toml")).unwrap(), "notes = 4\n");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn alias_box_draws_a_border_and_highlights_when_locked() {
        let (dir, mut form) = fixture("box");
        form.key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
        form.paste("docs");
        key(&mut form, KeyCode::Down);
        form.paste("~/docs/");
        key(&mut form, KeyCode::Enter);
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 140)).unwrap();
        terminal.draw(|f| form.draw(f, f.area(), true)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let line = |y: u16| (0..80).map(|x| buffer[(x, y)].symbol()).collect::<String>();
        let top = (0..140).find(|&y| line(y).contains("alias 1 · enter: edit · d: delete")).expect("highlighted box");
        assert!(line(top).trim_start().starts_with('╭'));
        assert!(line(top + 1).contains("docs") && line(top + 2).contains("~/docs/"));
        assert!(line(top + 3).trim_start().starts_with('╰'));
        // another item focused: gray border, plain title
        key(&mut form, KeyCode::Down);
        terminal.draw(|f| form.draw(f, f.area(), true)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let x = (0..80).find(|&x| buffer[(x, top)].symbol() == "╭").unwrap();
        assert_eq!(buffer[(x, top)].fg, theme::pal().line);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn alias_targets_share_prompt_path_completion() {
        let (dir, mut form) = fixture("paths");
        std::fs::create_dir_all(dir.join("browse/child")).unwrap();
        std::fs::write(dir.join("browse/guide.md"), "guide").unwrap();
        form.key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
        form.paste("notes");
        form.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        form.paste(&format!("{}/browse/", dir.display()));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 100)).unwrap();
        terminal.draw(|f| form.draw(f, f.area(), true)).unwrap();
        let buffer = terminal.backend().buffer();
        let line = |y| (0..90).map(|x| buffer[(x, y)].symbol()).collect::<String>();
        assert!((0..100).any(|y| line(y).contains("guide.md")));
        let target = form.fields[PLAIN + 1].text();
        assert!(matches!(
            form.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Action::None
        ));
        assert_eq!(form.fields[PLAIN + 1].text(), target);
        assert_eq!((form.focus, form.editing), (PLAIN, Some(PLAIN + 1)));
        // tab: the alias locked, the next item (the add row); shift+tab: back on the alias
        assert!(matches!(
            form.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            Action::None
        ));
        assert_eq!((form.focus, form.editing), (PLAIN + 1, None));
        form.key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!((form.focus, form.editing), (PLAIN, None));
        // back into the box: a new field resets the dropdown's Escape state
        form.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        form.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        form.fields[PLAIN + 1].move_end();
        assert!(matches!(
            form.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            Action::None
        ));
        assert_eq!(
            form.fields[PLAIN + 1].text(),
            format!("{}/browse/child/", dir.display())
        );
        form.fields[PLAIN + 1].set_text(&target);
        form.fields[PLAIN + 1].move_end();
        // ↑↓ + Enter chooses a file, just like the prompt.
        form.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        form.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            form.fields[PLAIN + 1].text(),
            format!("{}/browse/guide.md ", dir.display())
        );
        form.fields[PLAIN + 1].set_text(&target);
        form.fields[PLAIN + 1].move_end();
        terminal.draw(|f| form.draw(f, f.area(), true)).unwrap();
        let buffer = terminal.backend().buffer();
        let (x, y) = (0..100)
            .find_map(|y| {
                let line = (0..90).map(|x| buffer[(x, y)].symbol()).collect::<String>();
                // a byte offset: box chars (`│`) take 3 bytes, 1 column
                line.find("guide.md").map(|i| (line[..i].chars().count() as u16, y))
            })
            .unwrap();
        assert!(matches!(
            form.mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE
            }),
            Action::None
        ));
        assert_eq!(
            form.fields[PLAIN + 1].text(),
            format!("{}/browse/guide.md ", dir.display())
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
