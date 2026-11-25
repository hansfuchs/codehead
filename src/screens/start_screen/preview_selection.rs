use super::{App, Frame, Modal, Rect, Screen, Tab, find_rows};

impl App {
    /// The clickable item under the mouse (regions of this frame): its area.
    pub(super) fn hover_rect(&self) -> Option<Rect> {
        let at = self.preview.hover?;

        if let Some(message) = &self.overlays.menu {
            return message.row_at(at);
        }

        let buttons = self
            .preview
            .regions
            .buttons
            .iter()
            .map(|(result, _)| *result)
            .find(|result_value| result_value.contains(at));

        match &self.overlays.modal {
            Some(Modal::Agent(file)) if file.pick.is_some() => return find_rows(&self.preview.regions.form_rows, at),
            Some(Modal::Confirm { .. } | Modal::Agent(_)) => return buttons,
            Some(_) => return None,
            None => {}
        }

        if let Some(result_4) = self
            .preview
            .regions
            .tabs
            .iter()
            .map(|(result_2, _)| *result_2)
            .find(|result_3| result_3.contains(at))
        {
            return Some(result_4);
        }

        if self.compose.screen == Screen::Pick && self.tab == Tab::Settings {
            return self.settings.as_ref().and_then(|text| text.row_at(at));
        }

        if self.compose.screen == Screen::Compose {
            return hover_task(self, at);
        }

        hover_list(self, at)
    }
}

pub(super) fn hover_list(app_state: &App, at: ratatui::prelude::Position) -> Option<Rect> {
    let list = app_state.preview.regions.list;

    if !list.contains(at) {
        return None;
    }

    if app_state.tab == Tab::Agents {
        return app_state
            .agent_list
            .acards
            .iter()
            .find(|(result_10, _)| result_10.contains(at))
            .map(|(result_11, _)| *result_11);
    }

    if app_state.tab == Tab::Threads {
        if let Some(row_value) = app_state.thread_list.tlist.row_at(at) {
            return Some(row_value);
        }

        let rows = app_state.thread_list.tlist.list;
        let i = at.y.checked_sub(rows.y)? as usize;

        return (!app_state.thread_list.tlist.view.cards()
            && rows.contains(at)
            && app_state.thread_list.toffset + i < app_state.n_threads())
        .then_some(Rect {
            y: at.y,
            height: 1,
            ..rows
        });
    }

    let i = (at.y - list.y) as usize;

    let item = match app_state.tab {
        Tab::Notes => app_state.note_list.offset + i < app_state.note_list.rows.len(),
        _ => false,
    };

    item.then_some(Rect {
        y: at.y,
        height: 1,
        ..list
    })
}

pub(super) fn hover_task(app_state: &App, at: ratatui::prelude::Position) -> Option<Rect> {
    if app_state.draft_selection.drafts_menu.is_some() {
        return find_rows(&app_state.preview.regions.draft_rows, at);
    }

    if app_state.repository.where_menu.is_some() {
        return find_rows(&app_state.preview.regions.where_rows, at);
    }

    if app_state.project_selection.proj_menu.is_some() {
        return find_rows(&app_state.preview.regions.proj_rows, at);
    }

    if app_state.flow_selection.flow_menu.is_some() {
        return find_rows(&app_state.preview.regions.flow_rows, at)
            .or_else(|| find_rows(&app_state.preview.regions.flow_steps, at));
    }

    if let Some(path) = app_state.repository.repo_pick.as_ref() {
        return path
            .left_rows
            .iter()
            .chain(path.right_rows.iter())
            .map(|(result_5, _)| *result_5)
            .find(|result_6| result_6.contains(at));
    }

    if app_state.mention_on()
        && let Some(row) = app_state
            .compose
            .mention
            .row_at(at)
            .or_else(|| app_state.compose.command.row_at(at))
            .or_else(|| app_state.compose.shortcuts.row_at(at))
            .or_else(|| app_state.compose.paths.row_at(at))
    {
        return Some(row);
    }

    if let Some(result_8) = [app_state.preview.regions.drafts, app_state.preview.regions.save_draft]
        .into_iter()
        .find(|result_7| result_7.contains(at))
    {
        return Some(result_8);
    }

    find_rows(&app_state.preview.regions.opts, at)
        .or_else(|| find_rows(&app_state.preview.regions.menu_rows, at))
        .or_else(|| {
            [
                app_state.preview.regions.repo_row,
                app_state.preview.regions.proj_row,
                app_state.preview.regions.where_row,
                app_state.preview.regions.notes_label,
            ]
            .into_iter()
            .find(|result_9| result_9.contains(at))
        })
}

pub(super) fn draw_preview_selection(app_state: &mut App, frame: &mut Frame<'_>, inner: Rect) {
    let key = format!(
        "{:?} {} {} {} {} {} {:?} {inner:?}",
        app_state.tab,
        app_state.note_list.cursor,
        app_state.thread_list.tcursor,
        app_state.thread_list.tlist.cursor,
        app_state.agent_list.acursor,
        app_state.preview.pscroll,
        app_state.thread_list.tlist.view
    );

    if key != app_state.preview.psel_key {
        app_state.preview.psel_key = key;
        app_state.preview.psel.clear();
    }

    app_state.preview.psel.paint(inner, frame.buffer_mut());
}
