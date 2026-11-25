//! Settings form. Changes apply on leaving a field (or an alias box): codehead.toml is written, the
//! app applies it. Invalid values: 0 write, the error shows. Esc: the field as it was at entry, 0 write.
//! tab / shift+tab: next / previous field.

pub(crate) mod mcp_tab;

pub(crate) mod harness_tab;
use std::collections::BTreeMap;
use std::path::PathBuf;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use crate::ui::textarea::TextArea;
use crate::{infra::config, ui::theme};

/// A plain field: its config key, label, hint, env var that overrides it ("": none).
#[derive(Clone, Copy)]
pub(crate) struct Spec {
    key: &'static str,
    label: &'static str,
    help: &'static str,
    env: &'static str,
}

/// Group heading on ROWS (2 rows): `TITLE ────` (accent), what it sets (muted).
fn group_head(frame: &mut Frame, row: Option<Rect>, desc_row: Option<Rect>, title: &str, description: &str) {
    if let Some(head) = row {
        let rule = usize::from(head.width).saturating_sub(title.chars().count() + 1);

        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(title.to_string(), theme::accent_bold()),
                Span::styled(format!(" {}", "─".repeat(rule)), theme::line()),
            ])),
            head,
        );
    }

    if let Some(desc) = desc_row {
        frame.render_widget(
            Paragraph::new(Line::styled(description.to_string(), theme::muted())),
            desc,
        );
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Action {
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

/// A form of codehead.toml fields: the codehead tab (plain fields + path aliases), the harnesses tab.
pub(crate) struct Form {
    path: PathBuf,
    /// `d d` on an alias box: the 1st `d` armed the delete.
    del_armed: bool,
    specs: Vec<Spec>,
    /// Groups: the 1st field, the title, what the group sets.
    sections: Vec<(usize, &'static str, &'static str)>,
    /// The path alias boxes after the plain fields.
    aliases_on: bool,
    /// Fields 0..N: directory paths (path suggestions).
    path_fields: usize,
    /// The files as read: the base of the next save.
    base: Option<config::Layers>,
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
    paths: crate::ui::paths::Paths,
    path_focus: Option<usize>,
    /// Written to codehead.toml, not yet applied by the app.
    applied: Option<config::File>,
    /// The focused field and its text at entry: Esc puts it back (0 commit).
    entry: Option<(usize, String)>,
    /// The open alias box at its unlock: (its 1st field, name, target): Esc puts both back.
    pair_entry: Option<(usize, String, String)>,
}

impl Form {
    /// The codehead tab.
    pub(crate) fn new(path: PathBuf) -> Self {
        Self::with(path, SPECS.to_vec(), SECTIONS.to_vec(), true, 2)
    }

    pub(crate) fn with(
        path: PathBuf,
        specs: Vec<Spec>,
        sections: Vec<(usize, &'static str, &'static str)>,
        aliases_on: bool,
        path_fields: usize,
    ) -> Self {
        let mut form = Self {
            path,
            del_armed: false,
            specs,
            sections,
            aliases_on,
            path_fields,
            base: None,
            saved: config::File::default(),
            fields: Vec::new(),
            focus: 0,
            editing: None,
            offset: 0,
            rows: Vec::new(),
            status: String::new(),
            error: false,
            paths: crate::ui::paths::Paths::directory(&config::home()),
            path_focus: None,
            applied: None,
            entry: None,
            pair_entry: None,
        };

        if self::reload(&mut form).is_none() {
            form.fields = (0..form.plain()).map(|_| Self::input("", "use default")).collect();
        }

        form
    }

    fn input(text: &str, hint: &str) -> TextArea {
        let mut input = TextArea::new(true, hint);

        input.set_text(text);
        input.move_start();
        input
    }

    /// The number of plain fields.
    pub(crate) fn plain(&self) -> usize {
        self.specs.len()
    }

    fn load_layers(&mut self, layers: config::Layers) -> config::File {
        load_form_layers(self, layers)
    }

    /// The default of setting KEY, as the field's placeholder shows it. Colors: the scheme's.
    fn default_of(file: &config::File, key: &str) -> String {
        if let Some(name) = key.strip_prefix("colors.").filter(|color| *color != "scheme") {
            let scheme = file.scheme.clone().unwrap_or_else(|| theme::PRESETS[0].0.into());
            let palette = theme::Palette::preset(&scheme).unwrap_or(theme::PRESETS[0].1);

            return palette
                .get(name)
                .map(|color| format!("{} ({scheme})", theme::hex(color)))
                .unwrap_or_default();
        }

        if key == "classify" {
            return "the default agent".into();
        }

        if let Some(default) = crate::features::settings::harness_tab::default_of(key) {
            return default;
        }

        config::defaults()
            .get(key)
            .unwrap_or_else(|| "amon, else first agent".into())
    }

    fn push_alias(&mut self, name: &str, target: &str) {
        self.fields.push(Self::input(name, "project:platform"));
        self.fields
            .push(Self::input(target, "~/projects/exampleorg/project/notes/platform/"));
    }

    /// Changes → codehead.toml (the app applies them). Invalid values: 0 write, the error shows.
    pub(crate) fn commit(&mut self) {
        commit_form(self);
    }

    /// Settings written since the last call: for the app to apply.
    pub(crate) fn take_applied(&mut self) -> Option<config::File> {
        self.applied.take()
    }

    /// A new alias box, open on its name (enter on + add, menu → add path alias).
    pub(crate) fn add_alias(&mut self) {
        if !self.aliases_on {
            return;
        }

        self.commit();
        self.push_alias("", "");
        self.focus = self::add_item(self) - 1;
        self.editing = Some(self.fields.len() - 2);

        // esc's target: the empty box (as `key` sets it for a box unlocked by a key)
        self.pair_entry = Some((self.fields.len() - 2, String::new(), String::new()));
        self.focus_input();
    }

    /// Keyboard entry into an input starts before its existing text; its text then: Esc's target.
    pub(crate) fn focus_input(&mut self) {
        if let Some(i) = self::field(self) {
            self.fields[i].move_start();
        }

        self.entry = self::field(self).map(|i| (i, self.fields[i].text()));
        self.paths.reset_input();
    }

    pub(crate) fn paste(&mut self, text: &str) {
        if let Some(i) = self::field(self) {
            self.fields[i].insert(&text.replace(['\n', '\r'], ""));
        }
    }

    pub(crate) fn row_at(&self, at: Position) -> Option<Rect> {
        self.paths.row_at(at).or_else(|| {
            self.rows
                .iter()
                .find(|(rect, _)| rect.contains(at))
                .map(|(rect, _)| *rect)
        })
    }
}

impl Spec {
    pub(crate) const fn new(key: &'static str, label: &'static str, help: &'static str, env: &'static str) -> Spec {
        spec(key, label, help, env)
    }
}

/// The sub-tabs of the settings tab.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Sub {
    Codehead,
    Harnesses,
    Mcp,
    Shortcuts,
}

const SUBS: [(Sub, &str); 4] = [
    (Sub::Codehead, "codehead"),
    (Sub::Harnesses, "harnesses"),
    (Sub::Mcp, "mcp servers"),
    (Sub::Shortcuts, "shortcuts"),
];

/// The settings tab: a bar of sub-tabs (codehead, harnesses, mcp servers, shortcuts), the sub-tab below.
/// ↑ from the 1st row of a sub-tab: the bar; ←→ there: another sub-tab; ↑ again: the main tabs.
pub(crate) struct Settings {
    pub(crate) form: Form,
    pub(crate) harness: Form,
    pub(crate) mcp: crate::features::settings::mcp_tab::McpTab,
    pub(crate) shortcuts: crate::ui::shortcuts::Tab,
    pub(crate) sub: Sub,
    /// The keys go to the sub-tab bar.
    pub(crate) bar: bool,
    bar_hits: Vec<(Rect, Sub)>,
}

impl Settings {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            form: Form::new(path.clone()),
            harness: crate::features::settings::harness_tab::form(path),
            mcp: crate::features::settings::mcp_tab::McpTab::new(),
            shortcuts: crate::ui::shortcuts::Tab::new(),
            sub: Sub::Codehead,
            bar: false,
            bar_hits: Vec::new(),
        }
    }

    /// Sub-tab S: the last edit of the form left applies, the form entered reads the file again
    /// (the other form may have written it).
    pub(crate) fn switch(&mut self, sub: Sub) {
        switch_settings(self, sub);
    }

    pub(crate) fn commit(&mut self) {
        match self.sub {
            Sub::Codehead => self.form.commit(),
            Sub::Harnesses => self.harness.commit(),
            Sub::Mcp | Sub::Shortcuts => {}
        }
    }

    pub(crate) fn take_applied(&mut self) -> Option<config::File> {
        let codehead = self.form.take_applied();
        let harness = self.harness.take_applied();

        harness.or(codehead)
    }

    pub(crate) fn focus_input(&mut self) {
        match self.sub {
            Sub::Codehead => self.form.focus_input(),
            Sub::Harnesses => self.harness.focus_input(),
            Sub::Mcp | Sub::Shortcuts => {}
        }
    }

    pub(crate) fn paste(&mut self, text: &str) {
        match self.sub {
            Sub::Codehead => self.form.paste(text),
            Sub::Harnesses => self.harness.paste(text),
            Sub::Mcp => self.mcp.paste(text),
            Sub::Shortcuts => self.shortcuts.paste(text),
        }
    }

    pub(crate) fn row_at(&self, at: Position) -> Option<Rect> {
        if let Some((rect, _)) = self.bar_hits.iter().find(|(rect, _)| rect.contains(at)) {
            return Some(*rect);
        }

        match self.sub {
            Sub::Codehead => self.form.row_at(at),
            Sub::Harnesses => self.harness.row_at(at),
            Sub::Mcp => None,
            Sub::Shortcuts => self.shortcuts.row_at(at),
        }
    }

    /// Keys of the help bar.
    pub(crate) fn help(&self) -> &'static [(&'static str, &'static str)] {
        help_settings(self)
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;

#[path = "settings_draw.rs"]
mod draw;
#[path = "settings_key.rs"]
mod key;
#[path = "settings_key_helpers.rs"]
mod key_helpers;
#[path = "settings_mouse.rs"]
mod mouse;
#[path = "settings_poll.rs"]
mod poll;
#[path = "settings_updates.rs"]
mod updates;

#[path = "settings_schema.rs"]
mod schema;
#[cfg(test)]
pub(crate) use schema::PLAIN;
use schema::{ALIASES, INDENT, SECTIONS, SPECS, spec};

#[path = "settings_fields.rs"]
mod fields;
use fields::{add_item, alias_of, aliases, delete_alias, dirty, field, reset, revert, starts, values};

fn reload(form: &mut Form) -> Option<config::File> {
    match config::read_layers(&form.path) {
        Ok(layers) => Some(form.load_layers(layers)),
        Err(error) => {
            form.status = error;
            form.error = true;
            None
        }
    }
}

fn append_config_aliases(file: &config::File, form: &mut Form) {
    for (name, path) in &file.aliases {
        form.push_alias(name, path);
    }
}

fn finish_config_commit(layers: config::Layers, file: config::File, form: &mut Form) {
    // a new home or device: other files, other values → the fields from them
    let moved = layers.merged() != file;

    form.base = Some(layers);
    form.saved = file.clone();
    form.status = "settings applied".into();
    form.error = false;
    form.applied = Some(file);

    if moved && let Some(reloaded) = self::reload(form) {
        form.applied = Some(reloaded);
    }
}

fn load_form_layers(form: &mut Form, layers: config::Layers) -> config::File {
    let file = layers.merged();

    form.fields = form
        .specs
        .iter()
        .map(|specification| {
            Form::input(
                &file.get(specification.key).unwrap_or_default(),
                &format!("default: {}", Form::default_of(&file, specification.key)),
            )
        })
        .collect();

    if form.aliases_on {
        append_config_aliases(&file, form);
    }

    form.base = Some(layers);
    form.path_focus = None;
    form.editing = None;
    form.paths.reset_input();
    form.saved = file.clone();
    form.focus = form.focus.min(self::add_item(form));
    form.status = "changes apply on leaving a field · ctrl+d / ↺: default · environment overrides file values".into();
    form.error = false;

    file
}

fn commit_form(form: &mut Form) {
    if !self::dirty(form) {
        return;
    }

    let result = (|| {
        let file = self::values(form)?;
        let base = form.base.as_ref().ok_or("reload config before save")?;
        let layers = config::save_layers(base, &file)?;

        Ok::<_, String>((layers, file))
    })();

    match result {
        Ok((layers, file)) => {
            finish_config_commit(layers, file, form);
        }
        Err(error) => {
            form.status = format!("not applied: {error}");
            form.error = true;
        }
    }
}

fn switch_settings(settings: &mut Settings, sub: Sub) {
    if sub == settings.sub {
        return;
    }

    settings.commit();
    settings.sub = sub;

    match sub {
        Sub::Codehead => {
            self::reload(&mut settings.form);
        }
        Sub::Harnesses => {
            self::reload(&mut settings.harness);
        }
        Sub::Mcp => settings.mcp.enter(),
        Sub::Shortcuts => settings.shortcuts.enter(),
    }
}

fn help_settings(settings: &Settings) -> &'static [(&'static str, &'static str)] {
    if settings.bar {
        return &[("←→", "sub-tab"), ("↓ enter", "in"), ("↑", "main tabs")];
    }

    match settings.sub {
        Sub::Codehead => &[
            ("↑↓", "move"),
            ("enter", "edit / done"),
            ("ctrl+d", "default"),
            ("d", "delete alias"),
            ("tab", "next field"),
            ("esc", "undo edit"),
        ],
        Sub::Harnesses => &[
            ("↑↓", "move"),
            ("tab", "next field"),
            ("ctrl+d", "default"),
            ("esc", "undo edit"),
        ],
        Sub::Mcp => settings.mcp.help(),
        Sub::Shortcuts => settings.shortcuts.help(),
    }
}
