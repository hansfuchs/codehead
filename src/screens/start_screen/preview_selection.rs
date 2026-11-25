use super::{App, Frame, Modal, Rect, Screen, Tab, find_rows};

impl App {
    /// The clickable item under the mouse (regions of this frame): its area.
    pub(super) fn hover_rect(&self) -> Option<Rect> {
        let at = self.preview.hover?;

        if let Some(menu) = &self.overlays.menu {
            return menu.row_at(at);
        }

        let buttons = self
            .preview
            .regions
            .buttons
            .iter()
            .map(|(rect, _)| *rect)
            .find(|rect| rect.contains(at));

        match &self.overlays.modal {
            Some(Modal::Agent(form)) if form.pick.is_some() => return find_rows(&self.preview.regions.form_rows, at),
            Some(Modal::Confirm { .. } | Modal::Agent(_)) => return buttons,
            Some(_) => return None,
            None => {}
        }

        if let Some(rect) = self
            .preview
            .regions
            .tabs
            .iter()
            .map(|(rect, _)| *rect)
            .find(|rect| rect.contains(at))
        {
            return Some(rect);
        }

        if self.compose.screen == Screen::Pick && self.tab == Tab::Settings {
            return self.settings.as_ref().and_then(|settings| settings.row_at(at));
        }

        if self.compose.screen == Screen::Compose {
            return hover_task(self, at);
        }

        hover_list(self, at)
    }
}

pub(super) fn hover_list(app: &App, at: ratatui::prelude::Position) -> Option<Rect> {
    let list = app.preview.regions.list;

    if !list.contains(at) {
        return None;
    }

    if app.tab == Tab::Agents {
        return app
            .agent_list
            .acards
            .iter()
            .find(|(rect, _)| rect.contains(at))
            .map(|(rect, _)| *rect);
    }

    if app.tab == Tab::Threads {
        if let Some(row) = app.thread_list.tlist.row_at(at) {
            return Some(row);
        }

        let rows = app.thread_list.tlist.list;
        let i = at.y.checked_sub(rows.y)? as usize;

        return (!app.thread_list.tlist.view.cards()
            && rows.contains(at)
            && app.thread_list.toffset + i < app.n_threads())
        .then_some(Rect {
            y: at.y,
            height: 1,
            ..rows
        });
    }

    let i = (at.y - list.y) as usize;

    let item = match app.tab {
        Tab::Notes => app.note_list.offset + i < app.note_list.rows.len(),
        _ => false,
    };

    item.then_some(Rect {
        y: at.y,
        height: 1,
        ..list
    })
}

pub(super) fn hover_task(app: &App, at: ratatui::prelude::Position) -> Option<Rect> {
    if app.draft_selection.drafts_menu.is_some() {
        return find_rows(&app.preview.regions.draft_rows, at);
    }

    if app.repository.where_menu.is_some() {
        return find_rows(&app.preview.regions.where_rows, at);
    }

    if app.project_selection.proj_menu.is_some() {
        return find_rows(&app.preview.regions.proj_rows, at);
    }

    if app.flow_selection.flow_menu.is_some() {
        return find_rows(&app.preview.regions.flow_rows, at)
            .or_else(|| find_rows(&app.preview.regions.flow_steps, at));
    }

    if let Some(pick) = app.repository.repo_pick.as_ref() {
        return pick
            .left_rows
            .iter()
            .chain(pick.right_rows.iter())
            .map(|(rect, _)| *rect)
            .find(|rect| rect.contains(at));
    }

    if app.mention_on()
        && let Some(row) = app
            .compose
            .mention
            .row_at(at)
            .or_else(|| app.compose.command.row_at(at))
            .or_else(|| app.compose.shortcuts.row_at(at))
            .or_else(|| app.compose.paths.row_at(at))
    {
        return Some(row);
    }

    if let Some(rect) = [app.preview.regions.drafts, app.preview.regions.save_draft]
        .into_iter()
        .find(|rect| rect.contains(at))
    {
        return Some(rect);
    }

    find_rows(&app.preview.regions.opts, at)
        .or_else(|| find_rows(&app.preview.regions.menu_rows, at))
        .or_else(|| {
            [
                app.preview.regions.repo_row,
                app.preview.regions.proj_row,
                app.preview.regions.where_row,
                app.preview.regions.notes_label,
            ]
            .into_iter()
            .find(|rect| rect.contains(at))
        })
}

pub(super) fn draw_preview_selection(app: &mut App, frame: &mut Frame<'_>, inner: Rect) {
    let key = format!(
        "{:?} {} {} {} {} {} {:?} {inner:?}",
        app.tab,
        app.note_list.cursor,
        app.thread_list.tcursor,
        app.thread_list.tlist.cursor,
        app.agent_list.acursor,
        app.preview.pscroll,
        app.thread_list.tlist.view
    );

    if key != app.preview.psel_key {
        app.preview.psel_key = key;
        app.preview.psel.clear();
    }

    app.preview.psel.paint(inner, frame.buffer_mut());
}
