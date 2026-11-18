//! Settings form. Changes apply on leaving a field (or an alias box): codehead.toml is written, the
//! app applies it. Invalid values: 0 write, the error shows.
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

const KEYS: [&str; 5] = ["notes", "styles", "agent", "prefix", "models"];
const HELP: [&str; 5] = [
    "directory of Markdown notes, ~ expands to home",
    "directory of agent output styles",
    "default agent name",
    "prefix for new branches and worktrees",
    "Claude models, comma-separated",
];
const ENVS: [&str; 5] = [
    "CODEHEAD_NOTES",
    "CODEHEAD_STYLES",
    "CODEHEAD_AGENT",
    "CODEHEAD_PREFIX",
    "CODEHEAD_MODELS",
];

pub enum Action {
    None,
    NextTab,
    PrevTab,
    Header,
}

/// A click target of the last draw.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit {
    /// A plain field (0..5) or a field of an alias box (5..).
    Field(usize),
    /// The "+ add path alias" row.
    Add,
}

pub struct Settings {
    path: PathBuf,
    base: Option<String>,
    saved: config::File,
    /// 5 plain fields, then 2 per alias (name, target).
    fields: Vec<TextArea>,
    /// Navigation item: 0..5 plain fields, 5..5+n alias boxes, 5+n the add row.
    focus: usize,
    /// The field of the open (unlocked) alias box.
    editing: Option<usize>,
    offset: usize,
    rows: Vec<(Rect, Hit)>,
    status: String,
    error: bool,
    paths: crate::paths::Paths,
    path_focus: Option<usize>,
    /// Written to codehead.toml, not yet applied by the app.
    applied: Option<config::File>,
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
        };
        if form.reload().is_none() {
            form.fields = (0..5).map(|_| Self::input("", "use default")).collect();
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
        self.fields.len().saturating_sub(5) / 2
    }

    /// The add row: the last navigation item.
    fn add_item(&self) -> usize {
        5 + self.aliases()
    }

    /// The text field keys go to: a plain field, or a field of the open alias box.
    fn field(&self) -> Option<usize> {
        if self.focus < 5 { Some(self.focus) } else { self.editing }
    }

    /// The alias box of field I (≥ 5).
    fn alias_of(i: usize) -> usize {
        (i - 5) / 2
    }

    fn reload(&mut self) -> Option<config::File> {
        match config::read_file(&self.path) {
            Ok((text, file)) => {
                let defaults = config::defaults();
                self.fields = [
                    &file.notes,
                    &file.styles,
                    &file.agent,
                    &file.prefix,
                    &file.models,
                ]
                .into_iter()
                .zip([
                    defaults.notes,
                    defaults.styles,
                    defaults.agent,
                    defaults.prefix,
                    defaults.models,
                ])
                .map(|(value, default)| {
                    Self::input(
                        value.as_deref().unwrap_or_default(),
                        &format!(
                            "default: {}",
                            default.unwrap_or_else(|| "amon, else first agent".into())
                        ),
                    )
                })
                .collect();
                for (name, path) in &file.aliases {
                    self.push_alias(name, path);
                }
                self.base = Some(text);
                self.path_focus = None;
                self.editing = None;
                self.paths.reset_input();
                self.saved = file.clone();
                self.focus = self.focus.min(self.add_item());
                self.status = "changes apply on leaving a field · blank field: use default · environment overrides file values".into();
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

    fn push_alias(&mut self, name: &str, target: &str) {
        self.fields.push(Self::input(name, "project:platform"));
        self.fields.push(Self::input(target, "~/projects/exampleorg/project/notes/platform/"));
    }

    fn values(&self) -> Result<config::File, String> {
        if self.fields.len() < 5 {
            return Err("reload config before save".into());
        }
        let value = |i: usize| {
            let text = self.fields[i].text().trim().to_string();
            (!text.is_empty()).then_some(text)
        };
        let mut aliases = BTreeMap::new();
        for pair in self.fields[5..].as_chunks::<2>().0 {
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
        Ok(config::File {
            notes: value(0),
            styles: value(1),
            agent: value(2),
            prefix: value(3),
            models: value(4),
            aliases,
        })
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

    fn add_alias(&mut self) {
        self.commit();
        self.push_alias("", "");
        self.focus = self.add_item() - 1;
        self.editing = Some(self.fields.len() - 2);
    }

    fn delete_alias(&mut self, k: usize) {
        let at = 5 + k * 2;
        if at + 2 > self.fields.len() {
            return;
        }
        self.fields.drain(at..at + 2);
        self.editing = None;
        self.focus = self.focus.min(self.add_item());
        self.commit();
    }

    pub fn key(&mut self, key: KeyEvent) -> Action {
        let before = self.field();
        let action = self.key_inner(key);
        if self.field() != before {
            self.focus_input();
        }
        action
    }

    /// Keyboard entry into an input starts before its existing text.
    pub fn focus_input(&mut self) {
        if let Some(i) = self.field() {
            self.fields[i].move_start();
        }
        self.paths.reset_input();
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
                KeyCode::Char('d' | 'D') if (5..self.add_item()).contains(&self.focus) => {
                    self.delete_alias(self.focus - 5);
                    return Action::None;
                }
                _ => {}
            }
        }
        if let Some(i) = self.editing {
            // the open alias box: ↑↓ name / target, enter / esc: lock it again
            match key.code {
                KeyCode::Enter | KeyCode::Esc => {
                    self.editing = None;
                    self.commit();
                }
                KeyCode::Up if key.modifiers.is_empty() && (i - 5) % 2 == 1 => self.editing = Some(i - 1),
                KeyCode::Down if key.modifiers.is_empty() && (i - 5) % 2 == 0 => self.editing = Some(i + 1),
                KeyCode::Up | KeyCode::Down if key.modifiers.is_empty() => {}
                KeyCode::Tab | KeyCode::BackTab => {
                    self.editing = None;
                    self.commit();
                    return if key.code == KeyCode::Tab { Action::NextTab } else { Action::PrevTab };
                }
                _ => {
                    self.fields[i].key(key);
                }
            }
            return Action::None;
        }
        match key.code {
            KeyCode::Tab => {
                self.commit();
                return Action::NextTab;
            }
            KeyCode::BackTab => {
                self.commit();
                return Action::PrevTab;
            }
            KeyCode::Esc => self.commit(),
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
                self.focus = self.focus.saturating_sub(5);
            }
            KeyCode::PageDown => {
                self.commit();
                self.focus = (self.focus + 5).min(self.add_item());
            }
            KeyCode::Enter if self.focus == self.add_item() => self.add_alias(),
            KeyCode::Enter if self.focus >= 5 => self.editing = Some(5 + (self.focus - 5) * 2),
            KeyCode::Enter => {
                self.commit();
                self.focus = (self.focus + 1).min(self.add_item());
            }
            KeyCode::Char('d') if key.modifiers.is_empty() && (5..self.add_item()).contains(&self.focus) => {
                self.delete_alias(self.focus - 5);
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
        let focus = self.field().filter(|&i| i < 2 || (i >= 6 && i.is_multiple_of(2)));
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
                Hit::Field(i) if i < 5 => {
                    if self.focus != i { self.commit(); }
                    self.editing = None;
                    self.focus = i;
                }
                Hit::Field(i) => {
                    let k = Self::alias_of(i);
                    if self.editing.is_none_or(|e| Self::alias_of(e) != k) { self.commit(); }
                    self.focus = 5 + k;
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
        let body = Rect { y: area.y + 3, height: area.height - 4, ..area };
        // virtual rows: 3 per plain field, the alias heading (3), 4 per alias box, the add row
        let n = self.aliases();
        let mut items = Vec::new();
        let mut y = 0usize;
        for _ in 0..5 {
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
        for i in 0..5 {
            let Some(row) = at(items[i].0, 1) else { continue };
            let focused = active && self.focus == i;
            f.render_widget(
                Paragraph::new(Line::styled(KEYS[i], if focused { theme::accent_bold() } else { theme::muted() })),
                Rect { width: label_width, ..row },
            );
            let input = Rect { x: row.x + label_width, width: row.width - label_width, ..row };
            f.render_widget(Paragraph::new("").style(theme::panel()), input);
            self.fields[i].render(f, input, focused);
            self.rows.push((row, Hit::Field(i)));
            if let Some(hint_row) = at(items[i].0 + 1, 1) {
                let hint = std::env::var(ENVS[i])
                    .ok()
                    .filter(|s| !s.is_empty())
                    .map(|s| format!("{} override: {s}", ENVS[i]))
                    .unwrap_or_else(|| HELP[i].into());
                f.render_widget(
                    Paragraph::new(Line::styled(hint, theme::muted())),
                    Rect { x: hint_row.x + label_width, width: hint_row.width - label_width, ..hint_row },
                );
            }
        }
        if let Some(row) = at(heading, 1) {
            f.render_widget(Paragraph::new(Line::styled("path aliases", theme::bold())), row);
        }
        if let Some(row) = at(heading + 1, 1) {
            f.render_widget(
                Paragraph::new(Line::styled("name/ → target/ · enter: edit / done · d: delete · ctrl+n: add", theme::muted())),
                row,
            );
        }
        for k in 0..n {
            let item = 5 + k;
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
                let i = 5 + k * 2 + j;
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
                if self.error { theme::text().fg(theme::DANGER) } else { theme::muted() },
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
        std::fs::write(dir.join("codehead.toml"), "notes = '~/notes/'\nstyles = '~/styles/'\nagent = 'amon'\nprefix = 'task'\nmodels = 'opus'\n[aliases]\ndocs = '~/docs/'\n").unwrap();
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
        for i in 1..5 {
            form.fields[i].move_end();
            key(&mut form, KeyCode::Down);
            at_start(&mut form, i);
        }
        key(&mut form, KeyCode::Down);
        for i in 5..7 { form.fields[i].move_end(); }
        key(&mut form, KeyCode::Enter);
        at_start(&mut form, 5);
        key(&mut form, KeyCode::Down);
        at_start(&mut form, 6);
        key(&mut form, KeyCode::Up);
        at_start(&mut form, 5);
        key(&mut form, KeyCode::Enter);
        key(&mut form, KeyCode::PageUp);
        at_start(&mut form, 0);
        form.fields[0].move_end();
        form.focus_input();
        at_start(&mut form, 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn alias_boxes_lock_apply_on_leave_and_delete_with_d() {
        let (dir, mut form) = fixture("save");
        assert!(!form.dirty());
        // ctrl+n: a new box, open on its name
        form.key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
        assert_eq!((form.focus, form.editing), (5, Some(5)));
        form.paste("project:platform");
        // enter locks the box: 0 target → not applied, the error shows
        key(&mut form, KeyCode::Enter);
        assert_eq!(form.editing, None);
        assert!(form.error && form.take_applied().is_none());
        assert_eq!(file_aliases(&dir), 0);
        // a locked box: ↑↓ pass it, typing does not reach its fields
        key(&mut form, KeyCode::Char('x'));
        assert_eq!(form.fields[5].text(), "project:platform");
        key(&mut form, KeyCode::Enter);
        key(&mut form, KeyCode::Down);
        assert_eq!(form.editing, Some(6));
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
        assert_eq!(form.fields.len(), 25);
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
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 40)).unwrap();
        terminal.draw(|f| form.draw(f, f.area(), true)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let line = |y: u16| (0..80).map(|x| buffer[(x, y)].symbol()).collect::<String>();
        let top = (0..40).find(|&y| line(y).contains("alias 1 · enter: edit · d: delete")).expect("highlighted box");
        assert!(line(top).trim_start().starts_with('╭'));
        assert!(line(top + 1).contains("docs") && line(top + 2).contains("~/docs/"));
        assert!(line(top + 3).trim_start().starts_with('╰'));
        // another item focused: gray border, plain title
        key(&mut form, KeyCode::Down);
        terminal.draw(|f| form.draw(f, f.area(), true)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let x = (0..80).find(|&x| buffer[(x, top)].symbol() == "╭").unwrap();
        assert_eq!(buffer[(x, top)].fg, theme::LINE);
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
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 36)).unwrap();
        terminal.draw(|f| form.draw(f, f.area(), true)).unwrap();
        let buffer = terminal.backend().buffer();
        let line = |y| (0..90).map(|x| buffer[(x, y)].symbol()).collect::<String>();
        assert!((0..36).any(|y| line(y).contains("guide.md")));
        let target = form.fields[6].text();
        assert!(matches!(
            form.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Action::None
        ));
        assert_eq!(form.fields[6].text(), target);
        assert_eq!((form.focus, form.editing), (5, Some(6)));
        assert!(matches!(
            form.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            Action::NextTab
        ));
        // back into the box: a new field resets the dropdown's Escape state
        form.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        form.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        form.fields[6].move_end();
        assert!(matches!(
            form.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            Action::None
        ));
        assert_eq!(
            form.fields[6].text(),
            format!("{}/browse/child/", dir.display())
        );
        form.fields[6].set_text(&target);
        form.fields[6].move_end();
        // ↑↓ + Enter chooses a file, just like the prompt.
        form.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        form.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            form.fields[6].text(),
            format!("{}/browse/guide.md ", dir.display())
        );
        form.fields[6].set_text(&target);
        form.fields[6].move_end();
        terminal.draw(|f| form.draw(f, f.area(), true)).unwrap();
        let buffer = terminal.backend().buffer();
        let (x, y) = (0..36)
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
            form.fields[6].text(),
            format!("{}/browse/guide.md ", dir.display())
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
