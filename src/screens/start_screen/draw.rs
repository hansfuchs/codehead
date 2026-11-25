use super::{
    App, Constraint, Field, Frame, Layout, Outcome, Paragraph, Rect, Regions, Screen, Style, Tab, draw_agent_filter,
    draw_note_filter, draw_preview, draw_preview_selection, theme, worktree_of,
};

impl App {
    pub(crate) fn draw(&mut self, frame: &mut Frame) {
        self.preview.regions = Regions::default();
        let area = frame.area();

        frame.render_widget(Paragraph::new("").style(theme::text()), area);
        let [header, _, body, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(area);
        let body = Rect {
            x: body.x + 1,
            width: body.width.saturating_sub(2),
            ..body
        };

        self.draw_header(frame, header);

        match self.compose.screen {
            Screen::Pick => self.draw_pick(frame, body),
            Screen::Compose => self.draw_compose(frame, body),
        }

        self.draw_footer(frame, footer);

        // @ dropdown: after the screen, over every row of it (modals go on top)
        if self.mention_on() {
            self.compose
                .mention
                .render(frame, &self.compose.prompt, &self.agent_list.agents, area);
            let avoid = self.compose.prompt.area();

            self.compose.command.render(frame, &self.compose.prompt, area, avoid);
            self.compose.shortcuts.render(frame, &self.compose.prompt, area, avoid);
            self.compose.paths.render(frame, &self.compose.prompt, area, avoid);
            let open = self.compose.shortcuts.is_open(&self.compose.prompt);

            crate::ui::shortcuts::render_popup(
                frame,
                &self.compose.prompt,
                self.preview.hover,
                self.compose.field == Field::Prompt,
                area,
                open,
            );
        } else {
            self.compose.shortcuts.hide();
        }

        if self.compose.screen == Screen::Compose {
            draw_anchor(self, frame, area, body);
        }

        self.draw_modal(frame, area);

        // the context menu: above / below the field in focus (the task), right of the list (a
        // list tab), never over the cursor's row
        draw_overlays(self, frame, area);

        self.preview.last_frame.clone_from(frame.buffer_mut());

        if let Some(result) = self.hover_rect() {
            theme::hover(frame.buffer_mut(), result);
        }
    }

    /// A key / click of the notifications dropdown: a jump opens the thread at the note's event.
    pub(super) fn notes_pick(&mut self, path: crate::features::notifications::Pick) {
        use crate::features::notifications::Pick;

        match path {
            Pick::Stay => {}
            Pick::Close => self.overlays.notes_panel = None,
            Pick::Jump(count) => {
                notes_pick_body(self, &count);
            }
        }
    }

    pub(super) fn draw_pick(&mut self, frame: &mut Frame, area: Rect) {
        self.merge_area = Rect::default();

        if self.tab == Tab::Merge {
            self.merge_area = area;
            self.merge_tab.draw(frame, area);
            return;
        }

        self.project_selection.projects_area = Rect::default();

        if self.tab == Tab::Projects {
            self.project_selection.projects_area = area;
            let active = !self.tabs_focus;

            self.project_selection.projects_tab.draw(frame, area, active);
            return;
        }

        self.flows_area = Rect::default();

        if self.tab == Tab::Flows {
            self.flows_area = area;
            let active = !self.tabs_focus;

            self.flows_tab.draw(frame, area, active);
            return;
        }

        if self.tab == Tab::Settings {
            draw_pick_settings(self, frame, area);
            return;
        }

        let mut area = area;

        draw_note_filter(self, frame, &mut area);

        draw_agent_filter(self, frame, &mut area);

        let (list, inner) = draw_preview(self, frame, area);

        // the preview's selection: gone once the preview shows something else
        draw_preview_selection(self, frame, inner);

        match self.tab {
            Tab::Notes => self.draw_notes(frame, list),
            Tab::Threads => {
                draw_pick_tick(self, frame, list);
            }
            Tab::Agents | Tab::New => self.draw_agents(frame, list),
            Tab::Settings | Tab::Merge | Tab::Flows | Tab::Projects => {}
        }
    }

    pub(super) fn row_style(&self, hl: bool) -> Style {
        match (
            hl,
            self.tabs_focus || self.note_list.filter_editing || self.thread_list.tlist.filtering,
        ) {
            (true, false) => theme::pill(),
            (true, true) => theme::panel(),
            _ => Style::default(),
        }
    }
}

pub(super) fn draw_overlays(app_state: &mut App, frame: &mut Frame<'_>, area: Rect) {
    let place = if app_state.compose.screen == Screen::Compose {
        draw_overlays_area_value(app_state, area)
    } else if app_state.preview.regions.list.width > 0 {
        crate::ui::context_menu::Place::Beside(app_state.preview.regions.list, app_state.preview.regions.list.y)
    } else {
        crate::ui::context_menu::Place::At(Rect {
            y: area.y + 2,
            height: 1,
            ..area
        })
    };

    if let Some(character) = app_state.overlays.ctx.as_mut() {
        character.render(frame, place, area);
    }

    if let Some(message) = app_state.overlays.menu.as_mut() {
        message.render(frame, area);
    }

    if let Some(path_value) = app_state.overlays.notes_panel.as_mut() {
        path_value.draw(frame, area);
    }
}

pub(super) fn draw_overlays_area_value(app_state: &mut App, area: Rect) -> crate::ui::context_menu::Place {
    let area_value = match app_state.compose.field {
        Field::Agents => app_state.preview.regions.task_agents,
        Field::Notes => app_state.preview.regions.notes_label,
        Field::Where => app_state.preview.regions.where_row,
        Field::Repo => app_state.preview.regions.repo_row,
        Field::Drafts => app_state.preview.regions.drafts,
        _ => Rect::default(),
    };

    crate::ui::context_menu::Place::At(if area_value.height == 0 {
        Rect {
            y: area.y + 2,
            height: 1,
            ..area
        }
    } else {
        Rect {
            height: 1,
            ..area_value
        }
    })
}

pub(super) fn draw_anchor(app_state: &mut App, frame: &mut Frame<'_>, area: Rect, body: Rect) {
    app_state.draw_drafts(frame, area);
    app_state.draw_where_menu(frame, area);
    app_state.draw_proj_menu(frame, area);
    app_state.draw_flow_menu(frame, area);
    let anchor = app_state.preview.regions.repo_chip;

    if let Some(path) = app_state.repository.repo_pick.as_mut() {
        app_state.preview.regions.repo_pick = path.draw(frame, anchor, body).unwrap_or_default();
    }
}

pub(super) fn notes_pick_body(app_state: &mut App, count: &crate::domain::notifications::Note) {
    app_state.overlays.notes_panel = None;
    crate::domain::search::jump_to(&count.thread, count.ts);

    if !count.comment.is_empty() {
        crate::domain::changes::threads::jump_to(&count.thread, &count.comment);
    }

    app_state.outcome = Some(Outcome::Resume(Box::new(worktree_of(&count.card()))));
}

pub(super) fn draw_pick_settings(app_state: &mut App, frame: &mut Frame<'_>, area: Rect) {
    if let Some(settings) = &mut app_state.settings {
        settings.draw(frame, area, !app_state.tabs_focus);
    }
}

pub(super) fn draw_pick_tick(app_state: &mut App, frame: &mut Frame<'_>, list: Rect) {
    let tick = (u128::from(crate::engine::now_ms()) / crate::ui::thread_card::WAVE_MS) as usize;

    app_state.thread_list.tlist.draw(frame, list, tick);

    // worktrees / claude: this screen's rows in the list's place
    if !app_state.thread_list.tlist.view.cards() {
        let rows = app_state.thread_list.tlist.list;

        app_state.draw_threads(frame, rows);
        app_state
            .thread_list
            .threads_fx
            .apply(frame, rows, app_state.thread_list.threads_version);
    }
}
