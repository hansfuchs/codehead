use super::{Act, App, CARD_H, MouseButton, MouseEvent, MouseEventKind, Position, Row, Screen, Tab};

pub(super) fn mouse_lists(
    app_state: &mut App,
    event: MouseEvent,
    at: Position,
    list: ratatui::prelude::Rect,
    in_list: bool,
    in_preview: bool,
) {
    let mut copied = None;
    let mut took = false;

    for (on, ta) in [
        (app_state.note_list.filter_editing, &mut app_state.note_list.filter_ta),
        (
            app_state.thread_list.tlist.filtering,
            &mut app_state.thread_list.tlist.filter,
        ),
        (
            app_state.agent_list.afilter_editing,
            &mut app_state.agent_list.afilter_ta,
        ),
    ] {
        if on && ta.mouse(event) {
            took = true;
            copied = ta.copied.take();
        }
    }

    if let Some(count_3) = copied {
        app_state.toast(format!("copied {count_3} chars"), false);
    }

    if took {
        return;
    }

    // the preview: text selection (drag, 2 clicks: word, 3: row), copied at the release
    let wheel = matches!(event.kind, MouseEventKind::ScrollDown | MouseEventKind::ScrollUp);

    if (in_preview && !wheel) || app_state.preview.psel.is_dragging() {
        mouse_lists_thread(app_state, event);
        return;
    }

    if matches!(event.kind, MouseEventKind::Down(_)) {
        app_state.preview.psel.clear();
    }

    mouse_list_rows(app_state, event, at, list, in_list, in_preview);
}

pub(super) fn mouse_lists_thread(app_state: &mut App, event: MouseEvent) {
    if matches!(event.kind, MouseEventKind::Down(_)) {
        app_state.tabs_focus = false;
    }

    if let Some(thread) =
        app_state
            .preview
            .psel
            .mouse(event, app_state.preview.regions.preview, &app_state.preview.last_frame)
    {
        let ok = crate::infra::clip::copy(&thread);

        app_state.toast(
            if ok {
                format!("copied {} chars", thread.chars().count())
            } else {
                "0 clipboard tools found (wl-copy, xclip, xsel)".into()
            },
            !ok,
        );
    }
}

pub(super) fn mouse_list_rows(
    app_state: &mut App,
    event: MouseEvent,
    at: Position,
    list: ratatui::prelude::Rect,
    in_list: bool,
    in_preview: bool,
) {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) if in_list => mouse_list_click(app_state, event, at, list),
        MouseEventKind::ScrollDown if in_preview => {
            app_state.preview.pscroll = app_state.preview.pscroll.saturating_add(3);
        }
        MouseEventKind::ScrollUp if in_preview => {
            app_state.preview.pscroll = app_state.preview.pscroll.saturating_sub(3);
        }
        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp if in_list => mouse_list_wheel(app_state, event, list),
        _ => {}
    }
}

pub(super) fn mouse_list_wheel(app_state: &mut App, event: MouseEvent, list: ratatui::prelude::Rect) {
    let event_value = if matches!(event.kind, MouseEventKind::ScrollDown) {
        3
    } else {
        -3
    };

    if app_state.tab == Tab::Notes {
        app_state.move_cursor(event_value);
    } else if app_state.tab == Tab::Agents {
        // the card grid scrolls; the cursor stays on a card in view
        app_state.agent_list.aoffset = (app_state.agent_list.aoffset as isize + event_value).max(0) as usize;
        let (top, bottom) = (
            app_state.agent_list.aoffset,
            app_state.agent_list.aoffset + list.height as usize,
        );

        if let Some(&(i, _, _)) = app_state
            .agent_list
            .aplace
            .iter()
            .find(|&&(_, _, y)| y >= top && y + CARD_H as usize <= bottom)
            && !app_state.agent_list.aplace.iter().any(|&(character_3, _, y)| {
                character_3 == app_state.agent_list.acursor && y >= top && y + CARD_H as usize <= bottom
            })
        {
            app_state.agent_list.acursor = i;
        }

        app_state.preview.pscroll = 0;
    } else {
        let count_4 = app_state.n_threads() as isize;

        app_state.thread_list.tcursor =
            (app_state.thread_list.tcursor as isize + event_value).clamp(0, (count_4 - 1).max(0)) as usize;
        app_state.preview.pscroll = 0;
    }
}

pub(super) fn mouse_list_click(app_state: &mut App, event: MouseEvent, at: Position, list: ratatui::prelude::Rect) {
    app_state.tabs_focus = false;
    let i = (event.row - list.y) as usize;

    if app_state.tab == Tab::Notes {
        mouse_list_click_character_(app_state, i);
    } else if app_state.tab == Tab::Agents {
        mouse_list_click_body(app_state, at, i);
    }
}

pub(super) fn mouse_list_click_character_(app_state: &mut App, i: usize) {
    let i = app_state.note_list.offset + i;

    if i >= app_state.note_list.rows.len() {
        return;
    }

    if i == app_state.note_list.cursor {
        match &app_state.note_list.rows[i] {
            Row::Dir { collapsed, .. } => {
                let character_2 = *collapsed;

                app_state.fold(character_2);
            }
            _ => app_state.toggle_select(),
        }
    } else {
        app_state.note_list.cursor = i;
        app_state.preview.pscroll = 0;
    }
}

pub(super) fn mouse_list_click_body(app_state: &mut App, at: Position, i: usize) {
    let _ = i;

    if let Some(&(_, i)) = app_state
        .agent_list
        .acards
        .iter()
        .find(|(result_9, _)| result_9.contains(at))
    {
        if i == app_state.agent_list.acursor {
            app_state.run(Act::EditAgent);
        } else {
            app_state.agent_list.acursor = i;
            app_state.preview.pscroll = 0;
        }
    }
}

pub(super) fn mouse_thread_list(app_state: &mut App, event: MouseEvent, at: Position, down: bool) -> bool {
    if app_state.compose.screen == Screen::Pick
        && app_state.tab == Tab::Threads
        && app_state.preview.regions.list.contains(at)
        && !app_state.preview.psel.is_dragging()
    {
        mouse_thread_list_count_value(app_state, event, at, down);
        return true;
    }

    false
}

pub(super) fn mouse_thread_list_count_value(app_state: &mut App, event: MouseEvent, at: Position, down: bool) {
    use crate::features::thread_list::Out;

    if down {
        app_state.tabs_focus = false;
    }

    match app_state.thread_list.tlist.mouse(event) {
        Out::Open(character_value) => {
            app_state.open_card(&character_value);
            return;
        }
        Out::Pass if !app_state.thread_list.tlist.view.cards() && app_state.thread_list.tlist.list.contains(at) => {
            mouse_thread_list_count_value_count_value(app_state, event);
        }
        _ => {}
    }

    app_state.preview.pscroll = 0;
}

pub(super) fn mouse_thread_list_count_value_count_value(app_state: &mut App, event: MouseEvent) {
    let count_value = app_state.n_threads();

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            mouse_thread_list_count_value_count_value_body(app_state, event, count_value);
        }
        MouseEventKind::ScrollDown => {
            app_state.thread_list.tcursor = (app_state.thread_list.tcursor + 1).min(count_value.saturating_sub(1));
        }
        MouseEventKind::ScrollUp => {
            app_state.thread_list.tcursor = app_state.thread_list.tcursor.saturating_sub(1);
        }
        _ => {}
    }
}

pub(super) fn mouse_thread_list_count_value_count_value_body(
    app_state: &mut App,
    event: MouseEvent,
    count_value: usize,
) {
    let i = app_state.thread_list.toffset + (event.row - app_state.thread_list.tlist.list.y) as usize;

    if i < count_value {
        app_state.thread_list.tlist.focus = crate::features::thread_list::Focus::List;

        if i == app_state.thread_list.tcursor {
            app_state.open_thread();
        } else {
            app_state.thread_list.tcursor = i;
        }
    }
}
