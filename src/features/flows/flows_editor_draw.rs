use std::ops::ControlFlow;

use super::{
    Ask, Block, BorderType, Borders, Clear, Edit, EditorLayout, FlowsTab, Focus, Frame, Library, Line, Modifier,
    Paragraph, PickRow, Rect, Span, Style, list_names, theme,
};

impl FlowsTab {
    pub(crate) fn draw_editor(&mut self, frame: &mut Frame, area_value: Rect, active: bool) {
        draw_editor_flows_tab(self, frame, area_value, active);
    }

    fn draw_editor_lst(
        context: EditorLayout<'_, '_, '_, '_, '_>,
        i: usize,
        label: &mut &'static str,
        ta: &mut crate::ui::textarea::TextArea,
        shared: &mut bool,
    ) -> ControlFlow<()> {
        let EditorLayout {
            frame,
            active,
            focus,
            field_rects,
            field,
            inner,
            y,
            multi_h,
            cur_rect,
        } = context;

        if *y >= inner.bottom() {
            return ControlFlow::Break(());
        }

        let on = i == field;
        let lst = if on { theme::accent_bold() } else { theme::muted() };

        let label = if *shared {
            format!("{label} (shared)")
        } else {
            label.to_string()
        };

        frame.render_widget(
            Paragraph::new(Line::styled(label, lst)),
            Rect {
                y: *y,
                height: 1,
                ..inner
            },
        );
        *y += 1;

        let height_value = if ta.is_single() {
            1
        } else {
            multi_h.min(inner.bottom().saturating_sub(*y))
        };

        let tr = Rect {
            y: *y,
            height: height_value.max(1).min(inner.bottom().saturating_sub(*y).max(1)),
            ..inner
        };

        ta.render(frame, tr, active && on && focus == Focus::Editor);
        field_rects.push((tr, i));

        if on {
            *cur_rect = tr;
        }

        *y += tr.height;

        ControlFlow::Continue(())
    }
    pub(crate) fn draw_editor_edit_step_count_value(lib: &Library, count_value: &String) -> (String, Option<String>) {
        let users: Vec<String> = lib.users_of(count_value).iter().map(|file| file.name.clone()).collect();

        (
            format!(" step {count_value} "),
            (!users.is_empty()).then(|| format!("! an edit changes {} flows: {}", users.len(), list_names(&users))),
        )
    }
    pub(crate) fn draw_editor_edit_node_i_some(lib: &Library, i: usize, user: &String) -> (String, Option<String>) {
        let users: Vec<String> = lib
            .users_of(user)
            .iter()
            .map(|file_value| file_value.name.clone())
            .collect();

        (
            format!(" {} · shared step {user} · used in {} flows ", i + 1, users.len()),
            Some(format!(
                "! done when + text: an edit changes {}: {} · ctrl+f: fork as a one-off",
                if users.len() == 1 {
                    "1 flow".to_string()
                } else {
                    format!("{} flows", users.len())
                },
                list_names(&users)
            )),
        )
    }
    pub(crate) fn draw_ask(&mut self, frame: &mut Frame, right: Rect) {
        draw_ask_flows_tab(self, frame, right);
    }
    pub(crate) fn draw_picker(&mut self, frame: &mut Frame, right: Rect, bounds: Rect) {
        draw_picker_flows_tab(self, frame, right, bounds);
    }
}
pub(crate) fn draw_picker_pickrow_step_count_value(
    count_value: &String,
    view_state: &mut FlowsTab,
) -> (String, String) {
    let text_value = view_state.lib.step(count_value);

    (
        format!("✓ {count_value}"),
        text_value
            .map(|text_2| {
                format!(
                    "{} · {} flows",
                    text_2.description,
                    view_state.lib.users_of(count_value).len()
                )
            })
            .unwrap_or_default(),
    )
}
pub(crate) fn draw_picker_line(
    frame: &mut Frame<'_>,
    cur: usize,
    list: Rect,
    off: usize,
    key: usize,
    row: &PickRow,
    view_state: &mut FlowsTab,
) {
    let rr = Rect {
        y: list.y + (key - off) as u16,
        height: 1,
        ..list
    };
    let on = key == cur;

    let (label, detail) = match row {
        PickRow::OneOff(thread) => (
            format!("◇ one-off: {thread}"),
            "this flow only · s later: save as step".to_string(),
        ),
        PickRow::NewStep(count) => (
            format!("+ new step {count}"),
            "shared: every flow can use it".to_string(),
        ),
        PickRow::Step(count_value) => draw_picker_pickrow_step_count_value(count_value, view_state),
    };

    let st = if on {
        theme::pill().add_modifier(Modifier::BOLD)
    } else {
        theme::text()
    };

    let lw = 32.min(usize::from(rr.width) / 2);
    let line = Line::from(vec![
        Span::styled(format!("{:<lw$} ", crate::ui::thread_card::fit(&label, lw)), st),
        Span::styled(
            crate::ui::thread_card::fit(&detail, usize::from(rr.width).saturating_sub(lw + 1)),
            if on { st } else { theme::muted() },
        ),
    ]);

    frame.render_widget(Paragraph::new(line).style(if on { st } else { Style::default() }), rr);
}

fn draw_editor_flows_tab(state: &mut FlowsTab, frame: &mut Frame, area_value: Rect, active: bool) {
    let Some(ed) = state.editor.as_mut() else { return };

    let (title, warn) = match &ed.what {
        Edit::Step(count_value) => FlowsTab::draw_editor_edit_step_count_value(&state.lib, count_value),
        Edit::Node(i, Some(user)) => FlowsTab::draw_editor_edit_node_i_some(&state.lib, *i, user),
        Edit::Node(i, None) => (format!(" {} · one-off step ", i + 1), None),
    };

    let inner = draw_editor_frame(frame, area_value, title, ed.gate);
    let mut y = inner.y;

    if let Some(width_value) = warn {
        frame.render_widget(
            Paragraph::new(Line::styled(
                crate::ui::thread_card::fit(&width_value, usize::from(inner.width)),
                theme::fg(theme::pal().caution()),
            )),
            Rect { y, height: 1, ..inner },
        );
        y += 1;
    }

    let count_2 = ed.fields.len();
    let singles = ed.fields.iter().filter(|(_, thread, _)| thread.is_single()).count() as u16;
    let multis = (count_2 as u16).saturating_sub(singles).max(1);
    let room = inner.bottom().saturating_sub(y);
    let multi_h = (room.saturating_sub(singles * 2) / multis).saturating_sub(1).max(2);
    let mut cur_rect = Rect::default();

    for (i, (label, ta, shared)) in ed.fields.iter_mut().enumerate() {
        if let ControlFlow::Break(()) = FlowsTab::draw_editor_lst(
            EditorLayout {
                frame,
                active,
                field: ed.field,
                focus: state.focus,
                field_rects: &mut state.hit_regions.field_rects,
                inner,
                y: &mut y,
                multi_h,
                cur_rect: &mut cur_rect,
            },
            i,
            label,
            ta,
            shared,
        ) {
            break;
        }
    }

    let ta = &ed.fields[ed.field].1;

    state.shortcuts.render(frame, ta, frame.area(), cur_rect);
}

fn draw_ask_flows_tab(state: &mut FlowsTab, frame: &mut Frame, right: Rect) {
    let Some((what, ta)) = state.ask.as_mut() else { return };

    let label = match what {
        Ask::NewFlow => "new flow: name".to_string(),
        Ask::DuplicateFlow => "duplicate flow: name".into(),
        Ask::RenameFlow => "flow name".into(),
        Ask::NewStep => "new step: name".into(),
        Ask::DuplicateStep => "duplicate step: name".into(),
        Ask::RenameStep => "rename step (every flow follows)".into(),
        Ask::Promote(_) => "save as shared step: name".into(),
        Ask::LoopUntil(..) => "loop: until (the condition that ends it)".into(),
        Ask::Outcome(..) => "gate outcome: label".into(),
    };

    let width_value = right.width.min(64);
    let area = Rect {
        x: right.x,
        y: right.y + 2,
        width: width_value,
        height: 4,
    };

    frame.render_widget(Clear, area);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(theme::accent())
        .title(Span::styled(format!(" {label} "), theme::accent_bold()))
        .title_bottom(Line::styled(" enter ok · esc cancel ", theme::muted()).right_aligned());
    let inner = block.inner(area);

    frame.render_widget(block, area);
    let inner = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        height: 1,
        ..inner
    };

    ta.render(frame, inner, true);
}

fn draw_picker_flows_tab(state: &mut FlowsTab, frame: &mut Frame, right: Rect, bounds: Rect) {
    let Some((ta, cur)) = state.picker.as_ref() else { return };
    let rows = state.pick_rows(&ta.text());
    let cur = *cur;
    let width_value = right.width.min(80);
    let height_value = (rows.len() as u16 + 4).min(bounds.height.saturating_sub(4)).max(6);
    let area = Rect {
        x: right.x,
        y: right.y + 2,
        width: width_value,
        height: height_value.min(right.height.saturating_sub(2)),
    };

    frame.render_widget(Clear, area);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(theme::accent())
        .title(Span::styled(" add step ", theme::accent_bold()))
        .title_bottom(Line::styled(" type · ↑↓ · enter add · esc ", theme::muted()).right_aligned());
    let inner = block.inner(area);

    frame.render_widget(block, area);
    let inner = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };

    if let Some((ta, _)) = state.picker.as_mut() {
        ta.render(frame, Rect { height: 1, ..inner }, true);
    }

    let list = Rect {
        y: inner.y + 2,
        height: inner.height.saturating_sub(2),
        ..inner
    };
    let off = crate::ui::dropdown::fit(0, cur, rows.len(), usize::from(list.height));

    for (key, row) in rows.iter().enumerate().skip(off).take(usize::from(list.height)) {
        draw_picker_line(frame, cur, list, off, key, row, state);
    }
}

fn draw_editor_frame(frame: &mut Frame, area_value: Rect, title: String, gate: bool) -> Rect {
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(theme::accent())
        .title(Span::styled(title, theme::accent_bold()))
        .title_bottom(
            Line::styled(
                format!(
                    " gate {} · ctrl+g · tab next · ctrl+s save · esc close ",
                    if gate { "on" } else { "off" }
                ),
                theme::muted(),
            )
            .right_aligned(),
        );
    let inner = block.inner(area_value);

    frame.render_widget(Clear, area_value);
    frame.render_widget(block, area_value);

    Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    }
}
