use super::{
    AgentForm, App, F_HARNESS, F_MODEL, FormPick, Frame, KeyCode, KeyEvent, KeyModifiers, Modal, Position, Rect,
    UnicodeWidthStr, dropdown,
};

impl App {
    /// Rows of the harness dropdown: the installed harnesses (label, bin), the form's harness
    /// too when it is not installed. (value, row).
    pub(super) fn harness_items(form: &AgentForm) -> Vec<(String, dropdown::Row)> {
        let cur = form.harness_name();

        let mut items: Vec<(String, dropdown::Row)> = form
            .harnesses
            .iter()
            .map(|(name, label, bin)| {
                (
                    name.clone(),
                    dropdown::Row {
                        text: label.clone(),
                        detail: bin.clone(),
                        now: *name == cur,
                        marks: Vec::new(),
                    },
                )
            })
            .collect();

        if !items.iter().any(|(name, _)| *name == cur) {
            items.push((
                cur.clone(),
                dropdown::Row {
                    text: cur,
                    detail: "not installed".into(),
                    now: true,
                    marks: Vec::new(),
                },
            ));
        }

        items
    }

    /// Rows of the model dropdown of harness HARNESS (items, row), and a status: models being read, a
    /// read error. Claude Code: aliases + exact ids; others: `default` + the cached list.
    pub(super) fn model_items(
        &self,
        form: &AgentForm,
        harness: &str,
    ) -> (Vec<(String, dropdown::Row)>, Option<String>) {
        let cur = form.model_id();

        let row = |model: &crate::infra::models::Model| {
            (
                model.id.clone(),
                dropdown::Row {
                    text: model.id.clone(),
                    detail: model.name.clone(),
                    now: model.id == cur,
                    marks: Vec::new(),
                },
            )
        };

        let mut items: Vec<(String, dropdown::Row)> = Vec::new();
        let mut status = None;

        if harness == "claude" {
            items.extend(
                crate::infra::models::claude(&crate::infra::config::models())
                    .iter()
                    .map(row),
            );
        } else {
            push_cached_models(self, harness, row, &mut items, &mut status);
        }

        if !items.iter().any(|(id, _)| *id == cur) {
            items.push((
                cur.clone(),
                dropdown::Row {
                    text: cur,
                    detail: "current".into(),
                    now: true,
                    marks: Vec::new(),
                },
            ));
        }

        (items, status)
    }

    /// The rows of FORM's open dropdown for its filter, best match first.
    pub(super) fn pick_items(&self, form: &AgentForm) -> Vec<(String, dropdown::Row)> {
        let Some(pick) = &form.pick else { return Vec::new() };

        let all = if pick.model {
            self.model_items(form, &pick.harness).0
        } else {
            Self::harness_items(form)
        };

        // the text (marked), the value and detail too (substring)
        let ranked = crate::ui::fuzzy::rank(
            &all,
            &pick.query,
            |(_, row)| row.text.clone(),
            |(value, row)| format!("{value} {}", row.detail),
        );
        let mut slots: Vec<Option<(String, dropdown::Row)>> = all.into_iter().map(Some).collect();

        ranked
            .into_iter()
            .filter_map(|(i, marks)| {
                let (value, mut row) = slots[i].take()?;

                row.marks = marks;
                Some((value, row))
            })
            .collect()
    }

    /// The model list of harness HARNESS: from the cache file; 0 list or older than a day (or FORCE,
    /// `r`): read in the background.
    pub(super) fn ensure_models(&mut self, harness: &str, force: bool) {
        if harness == "claude" {
            return;
        }

        let fresh = self
            .model_catalog
            .models
            .get_or_insert_with(crate::infra::models::load)
            .get(harness)
            .is_some_and(crate::infra::models::fresh);

        if fresh && !force {
            return;
        }

        let (probe, name) = (self.model_catalog.probe, harness.to_string());

        self.model_catalog.model_errors.remove(harness);

        self.model_catalog.model_reads.request(name.clone(), move || {
            probe(&name).map(|models| crate::infra::models::store(&name, &models))
        });
    }

    /// Enter / click on the harness (MODEL: model) field: its dropdown, the cursor on the current value.
    pub(super) fn open_form_pick(&mut self, form: &mut AgentForm, model: bool) {
        let harness = form.harness_name();

        if model {
            self.ensure_models(&harness, false);
            form.field = F_MODEL;
        }

        form.pick = Some(FormPick {
            model,
            harness,
            cursor: 0,
            offset: 0,
            query: String::new(),
        });
        let cursor = self.pick_items(form).iter().position(|(_, row)| row.now).unwrap_or(0);

        if let Some(pick) = &mut form.pick {
            pick.cursor = cursor;
        }
    }

    /// Value V picked: a harness → the field, then its model list; a model → the form.
    pub(super) fn choose_form_pick(&mut self, form: &mut AgentForm, value: &str) {
        let model = form.pick.as_ref().is_some_and(|pick| pick.model);

        form.pick = None;
        form.error = None;

        if model {
            form.set_model(value);
        } else {
            form.set_harness(value);
            self.open_form_pick(form, true);
        }
    }

    /// Keys of the open harness / model dropdown: ↑↓ move, type: filter, enter: pick (models: 0
    /// match → the typed id), r (ctrl+r while typing): read the models again, esc: close.
    pub(super) fn form_pick_key(&mut self, form: &mut AgentForm, key: KeyEvent) {
        let items = self.pick_items(form);
        let count = items.len();
        let typed = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(pick) = form.pick.as_mut() else { return };
        let (model, harness, query) = (pick.model, pick.harness.clone(), pick.query.clone());

        match key.code {
            KeyCode::Esc => form.pick = None,
            KeyCode::Up if count > 0 => pick.cursor = (pick.cursor + count - 1) % count,
            KeyCode::Down if count > 0 => pick.cursor = (pick.cursor + 1) % count,
            KeyCode::PageUp => pick.cursor = pick.cursor.saturating_sub(dropdown::MAX_ROWS),
            KeyCode::PageDown if count > 0 => pick.cursor = (pick.cursor + dropdown::MAX_ROWS).min(count - 1),
            KeyCode::Enter => match items.get(pick.cursor) {
                Some((value, _)) => self.choose_form_pick(form, value),
                None if model && !query.trim().is_empty() => self.choose_form_pick(form, query.trim()),
                None => {}
            },
            KeyCode::Char('r') if model && (ctrl || query.is_empty()) => self.ensure_models(&harness, true),
            KeyCode::Backspace | KeyCode::Char('u' | 'w' | 'h')
                if crate::ui::textarea::delete_key(&mut pick.query, &key) =>
            {
                (pick.cursor, pick.offset) = (0, 0);
            }
            KeyCode::Char(character) if typed => {
                pick.query.push(character);
                (pick.cursor, pick.offset) = (0, 0);
            }
            _ => {}
        }
    }

    /// A click while the dropdown is open: a row picks it, elsewhere closes it.
    pub(super) fn form_pick_click(&mut self, form: &mut AgentForm, at: Position) {
        match self
            .preview
            .regions
            .form_rows
            .iter()
            .find(|(rect, _)| rect.contains(at))
            .map(|&(_, j)| j)
        {
            Some(j) => {
                choose_form_row(self, form, j);
            }
            None => form.pick = None,
        }
    }

    /// The open harness / model dropdown, below its field.
    pub(super) fn draw_form_pick(&mut self, frame: &mut Frame, bounds: Rect) {
        self.preview.regions.form_rows.clear();

        let Some(Modal::Agent(mut form)) = self.overlays.modal.take() else {
            return;
        };

        if let Some(pick) = form.pick.clone() {
            draw_form_pick_items(self, frame, bounds, &mut form, &pick);
        }

        self.overlays.modal = Some(Modal::Agent(form));
    }
}

impl FormPick {
    fn label(&self) -> String {
        match (self.model, self.query.is_empty()) {
            (true, true) => format!("models · {}", self.harness),
            (true, false) => format!("models · {} › {}", self.harness, self.query),
            (false, true) => "harnesses · installed".to_string(),
            (false, false) => format!("harnesses › {}", self.query),
        }
    }

    fn note(&self, status: Option<String>, count: usize, total: usize) -> String {
        match (status, self.query.is_empty()) {
            (Some(text), _) => text,
            (None, false) => format!("{count} of {total}"),
            (None, true) if self.model && self.harness != "claude" => "type: filter · r: read again".into(),
            (None, true) => "type: filter".into(),
        }
    }

    fn empty_label(&self) -> &'static str {
        if self.model && !self.query.trim().is_empty() {
            "0 matches · enter: use as typed"
        } else {
            "0 matches"
        }
    }
}

fn push_cached_models(
    app: &App,
    harness: &str,
    row: impl Fn(&crate::infra::models::Model) -> (String, dropdown::Row),
    items: &mut Vec<(String, dropdown::Row)>,
    status: &mut Option<String>,
) {
    items.push(row(&crate::infra::models::Model {
        id: "default".into(),
        name: "harness default".into(),
    }));

    if let Some(cached) = app
        .model_catalog
        .models
        .as_ref()
        .and_then(|catalog| catalog.get(harness))
    {
        items.extend(cached.models.iter().map(row));
    }

    if app.model_catalog.model_reads.waiting(&harness.to_string()) {
        *status = Some("reading models…".to_string());
    } else if let Some(error) = app.model_catalog.model_errors.get(harness) {
        *status = Some(format!("models: {error}"));
    }
}

fn draw_form_pick_items(app: &mut App, frame: &mut Frame<'_>, bounds: Rect, form: &mut AgentForm, pick: &FormPick) {
    let items = app.pick_items(form);
    let count = items.len();

    let total = if pick.model {
        app.model_items(form, &pick.harness).0.len()
    } else {
        App::harness_items(form).len()
    };

    let status = if pick.model {
        app.model_items(form, &pick.harness).1
    } else {
        None
    };

    let row = form.areas[if pick.model { F_MODEL } else { F_HARNESS }];
    let anchor = Rect {
        x: row.x + 10,
        y: row.y,
        width: 1,
        height: 1,
    };
    let rows: Vec<dropdown::Row> = items.into_iter().map(|(_, row)| row).collect();
    let width = rows
        .iter()
        .map(|row| row.text.width() + row.detail.width() + 8)
        .max()
        .unwrap_or(0)
        .clamp(44, 80) as u16;

    if let Some(area) = dropdown::place(anchor, width, count, bounds, false) {
        let cursor = pick.cursor.min(count.saturating_sub(1));

        let label = pick.label();
        let note = pick.note(status, count, total);
        let empty = pick.empty_label();

        let (offset, drawn) = dropdown::list(
            frame,
            area,
            dropdown::List {
                label: &label,
                note: &note,
                rows: &rows,
                cursor,
                offset: pick.offset,
                empty,
            },
        );

        if let Some(open) = form.pick.as_mut() {
            (open.cursor, open.offset) = (cursor, offset);
        }

        app.preview.regions.form_rows = drawn;
    }
}

fn choose_form_row(app: &mut App, form: &mut AgentForm, j: usize) {
    if let Some((value, _)) = app.pick_items(form).get(j) {
        let value = value.clone();

        app.choose_form_pick(form, &value);
    }
}
