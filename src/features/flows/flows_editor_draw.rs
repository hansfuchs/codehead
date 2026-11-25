use std::ops::ControlFlow;

use super::{
    Ask, Block, BorderType, Borders, Clear, Edit, EditorLayout, FlowsTab, Focus, Frame, Library, Line, Modifier,
    Paragraph, PickRow, Rect, Span, Style, list_names, theme,
};

impl FlowsTab {
    pub(crate) fn draw_editor(&mut self, frame: &mut Frame, area: Rect, active: bool) {
        draw_editor_flows_tab(self, frame, area, active);
    }

    fn draw_field(
        context: EditorLayout<'_, '_, '_, '_, '_>,
        i: usize,
        label: &mut &'static str,
        input: &mut crate::ui::textarea::TextArea,
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
        let label_style = if on { theme::accent_bold() } else { theme::muted() };

        let label = if *shared {
            format!("{label} (shared)")
        } else {
            label.to_string()
        };

        frame.render_widget(
            Paragraph::new(Line::styled(label, label_style)),
            Rect {
                y: *y,
                height: 1,
                ..inner
            },
        );
        *y += 1;

        let input_h = if input.is_single() {
            1
        } else {
            multi_h.min(inner.bottom().saturating_sub(*y))
        };

        let input_area = Rect {
            y: *y,
            height: input_h.max(1).min(inner.bottom().saturating_sub(*y).max(1)),
            ..inner
        };

        input.render(frame, input_area, active && on && focus == Focus::Editor);
        field_rects.push((input_area, i));

        if on {
            *cur_rect = input_area;
        }

        *y += input_area.height;

        ControlFlow::Continue(())
    }
    pub(crate) fn step_editor_title(lib: &Library, name: &String) -> (String, Option<String>) {
        let users: Vec<String> = lib.users_of(name).iter().map(|flow| flow.name.clone()).collect();

        (
            format!(" step {name} "),
            (!users.is_empty()).then(|| format!("! an edit changes {} flows: {}", users.len(), list_names(&users))),
        )
    }
    pub(crate) fn shared_node_editor_title(lib: &Library, i: usize, step_name: &String) -> (String, Option<String>) {
        let users: Vec<String> = lib.users_of(step_name).iter().map(|flow| flow.name.clone()).collect();

        (
            format!(" {} · shared step {step_name} · used in {} flows ", i + 1, users.len()),
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
pub(crate) fn step_pick_text(name: &String, tab: &mut FlowsTab) -> (String, String) {
    let step = tab.lib.step(name);

    (
        format!("✓ {name}"),
        step.map(|step| format!("{} · {} flows", step.description, tab.lib.users_of(name).len()))
            .unwrap_or_default(),
    )
}
pub(crate) fn draw_picker_line(
    frame: &mut Frame<'_>,
    cur: usize,
    list: Rect,
    offset: usize,
    index: usize,
    row: &PickRow,
    tab: &mut FlowsTab,
) {
    let row_area = Rect {
        y: list.y + (index - offset) as u16,
        height: 1,
        ..list
    };
    let on = index == cur;

    let (label, detail) = match row {
        PickRow::OneOff(text) => (
            format!("◇ one-off: {text}"),
            "this flow only · s later: save as step".to_string(),
        ),
        PickRow::NewStep(name) => (
            format!("+ new step {name}"),
            "shared: every flow can use it".to_string(),
        ),
        PickRow::Step(name) => step_pick_text(name, tab),
    };

    let style = if on {
        theme::pill().add_modifier(Modifier::BOLD)
    } else {
        theme::text()
    };

    let label_w = 32.min(usize::from(row_area.width) / 2);
    let line = Line::from(vec![
        Span::styled(
            format!("{:<label_w$} ", crate::ui::thread_card::fit(&label, label_w)),
            style,
        ),
        Span::styled(
            crate::ui::thread_card::fit(&detail, usize::from(row_area.width).saturating_sub(label_w + 1)),
            if on { style } else { theme::muted() },
        ),
    ]);

    frame.render_widget(
        Paragraph::new(line).style(if on { style } else { Style::default() }),
        row_area,
    );
}

fn draw_editor_flows_tab(state: &mut FlowsTab, frame: &mut Frame, area: Rect, active: bool) {
    let Some(editor) = state.editor.as_mut() else { return };

    let (title, warning) = match &editor.what {
        Edit::Step(name) => FlowsTab::step_editor_title(&state.lib, name),
        Edit::Node(i, Some(step_name)) => FlowsTab::shared_node_editor_title(&state.lib, *i, step_name),
        Edit::Node(i, None) => (format!(" {} · one-off step ", i + 1), None),
    };

    let inner = draw_editor_frame(frame, area, title, editor.gate);
    let mut y = inner.y;

    if let Some(warning) = warning {
        frame.render_widget(
            Paragraph::new(Line::styled(
                crate::ui::thread_card::fit(&warning, usize::from(inner.width)),
                theme::fg(theme::pal().caution()),
            )),
            Rect { y, height: 1, ..inner },
        );
        y += 1;
    }

    let field_count = editor.fields.len();
    let singles = editor.fields.iter().filter(|(_, input, _)| input.is_single()).count() as u16;
    let multis = (field_count as u16).saturating_sub(singles).max(1);
    let room = inner.bottom().saturating_sub(y);
    let multi_h = (room.saturating_sub(singles * 2) / multis).saturating_sub(1).max(2);
    let mut cur_rect = Rect::default();

    for (i, (label, input, shared)) in editor.fields.iter_mut().enumerate() {
        if let ControlFlow::Break(()) = FlowsTab::draw_field(
            EditorLayout {
                frame,
                active,
                field: editor.field,
                focus: state.focus,
                field_rects: &mut state.hit_regions.field_rects,
                inner,
                y: &mut y,
                multi_h,
                cur_rect: &mut cur_rect,
            },
            i,
            label,
            input,
            shared,
        ) {
            break;
        }
    }

    let input = &editor.fields[editor.field].1;

    state.shortcuts.render(frame, input, frame.area(), cur_rect);
}

fn draw_ask_flows_tab(state: &mut FlowsTab, frame: &mut Frame, right: Rect) {
    let Some((what, input)) = state.ask.as_mut() else {
        return;
    };

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

    let width = right.width.min(64);
    let area = Rect {
        x: right.x,
        y: right.y + 2,
        width,
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

    input.render(frame, inner, true);
}

fn draw_picker_flows_tab(state: &mut FlowsTab, frame: &mut Frame, right: Rect, bounds: Rect) {
    let Some((input, cur)) = state.picker.as_ref() else {
        return;
    };

    let rows = state.pick_rows(&input.text());
    let cur = *cur;
    let width = right.width.min(80);
    let height = (rows.len() as u16 + 4).min(bounds.height.saturating_sub(4)).max(6);
    let area = Rect {
        x: right.x,
        y: right.y + 2,
        width,
        height: height.min(right.height.saturating_sub(2)),
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

    if let Some((input, _)) = state.picker.as_mut() {
        input.render(frame, Rect { height: 1, ..inner }, true);
    }

    let list = Rect {
        y: inner.y + 2,
        height: inner.height.saturating_sub(2),
        ..inner
    };
    let offset = crate::ui::dropdown::fit(0, cur, rows.len(), usize::from(list.height));

    for (index, row) in rows.iter().enumerate().skip(offset).take(usize::from(list.height)) {
        draw_picker_line(frame, cur, list, offset, index, row, state);
    }
}

fn draw_editor_frame(frame: &mut Frame, area: Rect, title: String, gate: bool) -> Rect {
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
    let inner = block.inner(area);

    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    }
}
