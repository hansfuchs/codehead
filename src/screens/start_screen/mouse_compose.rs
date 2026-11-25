use super::{App, Field, Instant, MouseEvent, MouseEventKind, Position, Screen};

pub(super) fn mouse_compose(app_state: &mut App, event: MouseEvent, at: Position, down: bool) -> bool {
    if app_state.compose.screen != Screen::Compose {
        return false;
    }

    // drafts dropdown: a click on a row loads it, a click next to it closes it
    if mouse_compose_menus(app_state, at, down) {
        return true;
    }

    if mouse_compose_fields(app_state, event, at, down) {
        return true;
    }

    if mouse_compose_agents(app_state, event, at, down) {
        return true;
    }

    app_state.compose.title.mouse(event);
    app_state.compose.prompt.mouse(event);

    for ta in [&mut app_state.compose.title, &mut app_state.compose.prompt] {
        if let Some(count_2) = ta.copied.take() {
            app_state.overlays.toast = Some((format!("copied {count_2} chars"), Instant::now(), false));
        }
    }

    true
}

pub(super) fn mouse_compose_menus(app_state: &mut App, at: Position, down: bool) -> bool {
    if app_state.draft_selection.drafts_menu.is_some() {
        if down {
            match app_state
                .preview
                .regions
                .draft_rows
                .iter()
                .find(|(result_4, _)| result_4.contains(at))
            {
                Some(&(_, i)) => app_state.load_draft(i),
                None => app_state.draft_selection.drafts_menu = None,
            }
        }

        return true;
    }

    // flow dropdown: a click on a flow opens its steps, on a step picks it, next to them closes it
    if app_state.flow_selection.flow_menu.is_some() {
        if down {
            app_state.flow_menu_click(at);
        }

        return true;
    }

    // project dropdown: a click on a row picks it, a click next to it closes it
    if app_state.project_selection.proj_menu.is_some() {
        if down {
            match app_state
                .preview
                .regions
                .proj_rows
                .iter()
                .find(|(result_5, _)| result_5.contains(at))
            {
                Some(&(_, i)) => app_state.toggle_project(i, true),
                None => app_state.project_selection.proj_menu = None,
            }
        }

        return true;
    }

    // where dropdown: a click on a row picks it, a click next to it closes it
    if app_state.repository.where_menu.is_some() {
        if down {
            match app_state
                .preview
                .regions
                .where_rows
                .iter()
                .find(|(result_6, _)| result_6.contains(at))
            {
                Some(&(_, i)) => app_state.pick_where(i),
                None => app_state.repository.where_menu = None,
            }
        }

        return true;
    }

    // repo picker: a click on a dir moves there (left: current dir, right: into it), a
    // click outside of it closes it
    if let Some(path) = app_state.repository.repo_pick.as_mut() {
        if down && !path.click(at.x, at.y) && !app_state.preview.regions.repo_pick.contains(at) {
            app_state.repository.repo_pick = None;
        }

        return true;
    }

    false
}

pub(super) fn mouse_compose_fields(app_state: &mut App, event: MouseEvent, at: Position, down: bool) -> bool {
    if down && app_state.preview.regions.drafts.contains(at) {
        app_state.compose.field = Field::Drafts;
        app_state.draft_selection.draft_btn = 0;
        app_state.open_drafts();
        return true;
    }

    if down && app_state.preview.regions.save_draft.contains(at) {
        app_state.compose.field = Field::Drafts;
        app_state.draft_selection.draft_btn = 1;
        app_state.save_draft();
        return true;
    }

    if app_state.mention_on() {
        if down
            && app_state
                .compose
                .mention
                .mouse(event, &mut app_state.compose.prompt, &app_state.agent_list.agents)
        {
            return true;
        }

        if app_state.compose.command.mouse(event, &mut app_state.compose.prompt)
            || app_state.compose.shortcuts.mouse(event, &mut app_state.compose.prompt)
            || app_state.compose.paths.mouse(event, &mut app_state.compose.prompt)
        {
            return true;
        }
    }

    if down
        && let Some(&(_, i)) = app_state
            .preview
            .regions
            .opts
            .iter()
            .find(|(result_7, _)| result_7.contains(at))
    {
        app_state.compose.field = Field::Opts;
        app_state.toggle_opt(i);
        return true;
    }

    if mouse_task_focus(app_state, at, down) {
        return true;
    }

    false
}

pub(super) fn mouse_compose_agents(app_state: &mut App, event: MouseEvent, at: Position, down: bool) -> bool {
    if down
        && let Some(&(_, i)) = app_state
            .preview
            .regions
            .menu_rows
            .iter()
            .find(|(result_8, _)| result_8.contains(at))
    {
        app_state.compose.field = Field::Agents;
        app_state.compose.pick_cursor = i;
        app_state.toggle_pick();
    }

    if app_state.preview.regions.task_agents.contains(at)
        && let Some(value) = mouse_compose_agents_body(app_state, event)
    {
        return value;
    }

    false
}

pub(super) fn mouse_compose_agents_body(app_state: &mut App, event: MouseEvent) -> Option<bool> {
    let dy = match event.kind {
        MouseEventKind::ScrollUp => -1,
        MouseEventKind::ScrollDown => 1,
        _ => 0,
    };

    if dy != 0 {
        app_state.compose.field = Field::Agents;

        if let Some(i) = App::grid_step(&app_state.compose.pick_places, app_state.compose.pick_cursor, 0, dy) {
            app_state.compose.pick_cursor = i;
        }

        return Some(true);
    }

    None
}

pub(super) fn mouse_task_focus(app_state: &mut App, at: Position, down: bool) -> bool {
    if down {
        if app_state.compose.title.area().contains(at) {
            app_state.compose.field = Field::Title;
        } else if app_state.compose.prompt.area().contains(at) {
            app_state.compose.field = Field::Prompt;
        } else if app_state.preview.regions.repo_row.contains(at) {
            app_state.compose.field = Field::Repo;
            app_state.open_repo_pick();
            return true;
        } else if app_state.preview.regions.proj_row.contains(at) && app_state.cfg.repo.is_some() {
            app_state.compose.field = Field::Projects;
            app_state.open_projects();
            return true;
        } else if app_state.preview.regions.where_row.contains(at) {
            app_state.compose.field = Field::Where;
            app_state.open_where();
            return true;
        } else if app_state.preview.regions.notes_label.contains(at) {
            app_state.compose.field = Field::Notes;
            app_state.open_notes();
            return true;
        }
    }

    false
}
