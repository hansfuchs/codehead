use super::{App, Field, Instant, MouseEvent, MouseEventKind, Position, Screen};

pub(super) fn mouse_compose(app: &mut App, event: MouseEvent, at: Position, down: bool) -> bool {
    if app.compose.screen != Screen::Compose {
        return false;
    }

    // drafts dropdown: a click on a row loads it, a click next to it closes it
    if mouse_compose_menus(app, at, down) {
        return true;
    }

    if mouse_compose_fields(app, event, at, down) {
        return true;
    }

    if mouse_compose_agents(app, event, at, down) {
        return true;
    }

    app.compose.title.mouse(event);
    app.compose.prompt.mouse(event);

    for ta in [&mut app.compose.title, &mut app.compose.prompt] {
        if let Some(chars) = ta.copied.take() {
            app.overlays.toast = Some((format!("copied {chars} chars"), Instant::now(), false));
        }
    }

    true
}

pub(super) fn mouse_compose_menus(app: &mut App, at: Position, down: bool) -> bool {
    if app.draft_selection.drafts_menu.is_some() {
        if down {
            match app
                .preview
                .regions
                .draft_rows
                .iter()
                .find(|(rect, _)| rect.contains(at))
            {
                Some(&(_, i)) => app.load_draft(i),
                None => app.draft_selection.drafts_menu = None,
            }
        }

        return true;
    }

    // flow dropdown: a click on a flow opens its steps, on a step picks it, next to them closes it
    if app.flow_selection.flow_menu.is_some() {
        if down {
            app.flow_menu_click(at);
        }

        return true;
    }

    // project dropdown: a click on a row picks it, a click next to it closes it
    if app.project_selection.proj_menu.is_some() {
        if down {
            match app.preview.regions.proj_rows.iter().find(|(rect, _)| rect.contains(at)) {
                Some(&(_, i)) => app.toggle_project(i, true),
                None => app.project_selection.proj_menu = None,
            }
        }

        return true;
    }

    // where dropdown: a click on a row picks it, a click next to it closes it
    if app.repository.where_menu.is_some() {
        if down {
            match app
                .preview
                .regions
                .where_rows
                .iter()
                .find(|(rect, _)| rect.contains(at))
            {
                Some(&(_, i)) => app.pick_where(i),
                None => app.repository.where_menu = None,
            }
        }

        return true;
    }

    // repo picker: a click on a dir moves there (left: current dir, right: into it), a
    // click outside of it closes it
    if let Some(picker) = app.repository.repo_pick.as_mut() {
        if down && !picker.click(at.x, at.y) && !app.preview.regions.repo_pick.contains(at) {
            app.repository.repo_pick = None;
        }

        return true;
    }

    false
}

pub(super) fn mouse_compose_fields(app: &mut App, event: MouseEvent, at: Position, down: bool) -> bool {
    if down && app.preview.regions.drafts.contains(at) {
        app.compose.field = Field::Drafts;
        app.draft_selection.draft_btn = 0;
        app.open_drafts();
        return true;
    }

    if down && app.preview.regions.save_draft.contains(at) {
        app.compose.field = Field::Drafts;
        app.draft_selection.draft_btn = 1;
        app.save_draft();
        return true;
    }

    if app.mention_on() {
        if down
            && app
                .compose
                .mention
                .mouse(event, &mut app.compose.prompt, &app.agent_list.agents)
        {
            return true;
        }

        if app.compose.command.mouse(event, &mut app.compose.prompt)
            || app.compose.shortcuts.mouse(event, &mut app.compose.prompt)
            || app.compose.paths.mouse(event, &mut app.compose.prompt)
        {
            return true;
        }
    }

    if down && let Some(&(_, i)) = app.preview.regions.opts.iter().find(|(rect, _)| rect.contains(at)) {
        app.compose.field = Field::Opts;
        app.toggle_opt(i);
        return true;
    }

    if mouse_task_focus(app, at, down) {
        return true;
    }

    false
}

pub(super) fn mouse_compose_agents(app: &mut App, event: MouseEvent, at: Position, down: bool) -> bool {
    if down && let Some(&(_, i)) = app.preview.regions.menu_rows.iter().find(|(rect, _)| rect.contains(at)) {
        app.compose.field = Field::Agents;
        app.compose.pick_cursor = i;
        app.toggle_pick();
    }

    if app.preview.regions.task_agents.contains(at)
        && let Some(handled) = scroll_agent_grid(app, event)
    {
        return handled;
    }

    false
}

pub(super) fn scroll_agent_grid(app: &mut App, event: MouseEvent) -> Option<bool> {
    let dy = match event.kind {
        MouseEventKind::ScrollUp => -1,
        MouseEventKind::ScrollDown => 1,
        _ => 0,
    };

    if dy != 0 {
        app.compose.field = Field::Agents;

        if let Some(i) = App::grid_step(&app.compose.pick_places, app.compose.pick_cursor, 0, dy) {
            app.compose.pick_cursor = i;
        }

        return Some(true);
    }

    None
}

pub(super) fn mouse_task_focus(app: &mut App, at: Position, down: bool) -> bool {
    if down {
        if app.compose.title.area().contains(at) {
            app.compose.field = Field::Title;
        } else if app.compose.prompt.area().contains(at) {
            app.compose.field = Field::Prompt;
        } else if app.preview.regions.repo_row.contains(at) {
            app.compose.field = Field::Repo;
            app.open_repo_pick();
            return true;
        } else if app.preview.regions.proj_row.contains(at) && app.cfg.repo.is_some() {
            app.compose.field = Field::Projects;
            app.open_projects();
            return true;
        } else if app.preview.regions.where_row.contains(at) {
            app.compose.field = Field::Where;
            app.open_where();
            return true;
        } else if app.preview.regions.notes_label.contains(at) {
            app.compose.field = Field::Notes;
            app.open_notes();
            return true;
        }
    }

    false
}
