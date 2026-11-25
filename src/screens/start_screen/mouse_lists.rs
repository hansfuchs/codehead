use super::{Act, App, CARD_H, MouseButton, MouseEvent, MouseEventKind, Position, Row, Screen, Tab};

pub(super) fn mouse_lists(
    app: &mut App,
    event: MouseEvent,
    at: Position,
    list: ratatui::prelude::Rect,
    in_list: bool,
    in_preview: bool,
) {
    let mut copied = None;
    let mut took = false;

    for (on, ta) in [
        (app.note_list.filter_editing, &mut app.note_list.filter_ta),
        (app.thread_list.tlist.filtering, &mut app.thread_list.tlist.filter),
        (app.agent_list.afilter_editing, &mut app.agent_list.afilter_ta),
    ] {
        if on && ta.mouse(event) {
            took = true;
            copied = ta.copied.take();
        }
    }

    if let Some(chars) = copied {
        app.toast(format!("copied {chars} chars"), false);
    }

    if took {
        return;
    }

    // the preview: text selection (drag, 2 clicks: word, 3: row), copied at the release
    let wheel = matches!(event.kind, MouseEventKind::ScrollDown | MouseEventKind::ScrollUp);

    if (in_preview && !wheel) || app.preview.psel.is_dragging() {
        mouse_preview_selection(app, event);
        return;
    }

    if matches!(event.kind, MouseEventKind::Down(_)) {
        app.preview.psel.clear();
    }

    mouse_list_rows(app, event, at, list, in_list, in_preview);
}

pub(super) fn mouse_preview_selection(app: &mut App, event: MouseEvent) {
    if matches!(event.kind, MouseEventKind::Down(_)) {
        app.tabs_focus = false;
    }

    if let Some(text) = app
        .preview
        .psel
        .mouse(event, app.preview.regions.preview, &app.preview.last_frame)
    {
        let ok = crate::infra::clip::copy(&text);

        app.toast(
            if ok {
                format!("copied {} chars", text.chars().count())
            } else {
                "0 clipboard tools found (wl-copy, xclip, xsel)".into()
            },
            !ok,
        );
    }
}

pub(super) fn mouse_list_rows(
    app: &mut App,
    event: MouseEvent,
    at: Position,
    list: ratatui::prelude::Rect,
    in_list: bool,
    in_preview: bool,
) {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) if in_list => mouse_list_click(app, event, at, list),
        MouseEventKind::ScrollDown if in_preview => {
            app.preview.pscroll = app.preview.pscroll.saturating_add(3);
        }
        MouseEventKind::ScrollUp if in_preview => {
            app.preview.pscroll = app.preview.pscroll.saturating_sub(3);
        }
        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp if in_list => mouse_list_wheel(app, event, list),
        _ => {}
    }
}

pub(super) fn mouse_list_wheel(app: &mut App, event: MouseEvent, list: ratatui::prelude::Rect) {
    let delta = if matches!(event.kind, MouseEventKind::ScrollDown) {
        3
    } else {
        -3
    };

    if app.tab == Tab::Notes {
        app.move_cursor(delta);
    } else if app.tab == Tab::Agents {
        // the card grid scrolls; the cursor stays on a card in view
        app.agent_list.aoffset = (app.agent_list.aoffset as isize + delta).max(0) as usize;
        let (top, bottom) = (app.agent_list.aoffset, app.agent_list.aoffset + list.height as usize);

        if let Some(&(i, _, _)) = app
            .agent_list
            .aplace
            .iter()
            .find(|&&(_, _, y)| y >= top && y + CARD_H as usize <= bottom)
            && !app
                .agent_list
                .aplace
                .iter()
                .any(|&(card, _, y)| card == app.agent_list.acursor && y >= top && y + CARD_H as usize <= bottom)
        {
            app.agent_list.acursor = i;
        }

        app.preview.pscroll = 0;
    } else {
        let count = app.n_threads() as isize;

        app.thread_list.tcursor = (app.thread_list.tcursor as isize + delta).clamp(0, (count - 1).max(0)) as usize;
        app.preview.pscroll = 0;
    }
}

pub(super) fn mouse_list_click(app: &mut App, event: MouseEvent, at: Position, list: ratatui::prelude::Rect) {
    app.tabs_focus = false;
    let i = (event.row - list.y) as usize;

    if app.tab == Tab::Notes {
        click_note_row(app, i);
    } else if app.tab == Tab::Agents {
        click_agent_card(app, at, i);
    }
}

pub(super) fn click_note_row(app: &mut App, i: usize) {
    let i = app.note_list.offset + i;

    if i >= app.note_list.rows.len() {
        return;
    }

    if i == app.note_list.cursor {
        match &app.note_list.rows[i] {
            Row::Dir { collapsed, .. } => {
                let collapsed = *collapsed;

                app.fold(collapsed);
            }
            _ => app.toggle_select(),
        }
    } else {
        app.note_list.cursor = i;
        app.preview.pscroll = 0;
    }
}

pub(super) fn click_agent_card(app: &mut App, at: Position, i: usize) {
    let _ = i;

    if let Some(&(_, i)) = app.agent_list.acards.iter().find(|(rect, _)| rect.contains(at)) {
        if i == app.agent_list.acursor {
            app.run(Act::EditAgent);
        } else {
            app.agent_list.acursor = i;
            app.preview.pscroll = 0;
        }
    }
}

pub(super) fn mouse_thread_list(app: &mut App, event: MouseEvent, at: Position, down: bool) -> bool {
    if app.compose.screen == Screen::Pick
        && app.tab == Tab::Threads
        && app.preview.regions.list.contains(at)
        && !app.preview.psel.is_dragging()
    {
        mouse_thread_list_view(app, event, at, down);
        return true;
    }

    false
}

pub(super) fn mouse_thread_list_view(app: &mut App, event: MouseEvent, at: Position, down: bool) {
    use crate::features::thread_list::Out;

    if down {
        app.tabs_focus = false;
    }

    match app.thread_list.tlist.mouse(event) {
        Out::Open(card) => {
            app.open_card(&card);
            return;
        }
        Out::Pass if !app.thread_list.tlist.view.cards() && app.thread_list.tlist.list.contains(at) => {
            mouse_thread_rows(app, event);
        }
        _ => {}
    }

    app.preview.pscroll = 0;
}

pub(super) fn mouse_thread_rows(app: &mut App, event: MouseEvent) {
    let count = app.n_threads();

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            click_thread_row(app, event, count);
        }
        MouseEventKind::ScrollDown => {
            app.thread_list.tcursor = (app.thread_list.tcursor + 1).min(count.saturating_sub(1));
        }
        MouseEventKind::ScrollUp => {
            app.thread_list.tcursor = app.thread_list.tcursor.saturating_sub(1);
        }
        _ => {}
    }
}

pub(super) fn click_thread_row(app: &mut App, event: MouseEvent, count: usize) {
    let i = app.thread_list.toffset + (event.row - app.thread_list.tlist.list.y) as usize;

    if i < count {
        app.thread_list.tlist.focus = crate::features::thread_list::Focus::List;

        if i == app.thread_list.tcursor {
            app.open_thread();
        } else {
            app.thread_list.tcursor = i;
        }
    }
}
