use super::{
    App, Constraint, F_BUTTONS, F_COLOR, F_EFFORT, F_HARNESS, F_MCP, F_MODE, F_MODEL, F_NAME, F_PROMPT, F_SET,
    F_SPRITE, F_VOICE, Frame, Layout, Line, Modal, Modifier, Paragraph, Rect, Span, Style, UnicodeWidthStr, agents,
    sprites, theme,
};

impl App {
    pub(super) fn draw_agent_form(&mut self, frame: &mut Frame, area: Rect) {
        let global = self.agent_list.global_style.as_deref();

        let Some(Modal::Agent(form)) = &mut self.overlays.modal else {
            return;
        };

        let title = if form.edit.is_some() { "edit agent" } else { "new agent" };
        let inner = Self::frame(frame, area, title, 90, area.height.saturating_sub(2).min(42));

        // 0-6 single-line fields, 7 note, 8 MCP servers, 10 sprite set, 11 sprite, 13-14 voice,
        // 16-17 system prompt, 19 error, 20 buttons
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(sprites::HEIGHT as u16),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(5),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

        let lab = |thread: &str, on: bool| draw_agent_form_body(thread, on);

        // single-line rows: label left, value right
        draw_form_fields(frame, form, &rows, lab);

        draw_form_note(frame, global, form, &rows);

        // MCP servers: codehead's, ←→ pick, space on / off
        draw_form_servers(frame, form, &rows, lab);

        // avatar: the category, then the live avatar; enter opens the picker
        let cat = draw_form_categories(frame, form, &rows, lab);

        draw_form_avatar(frame, form, &rows, lab, cat);

        draw_form_text(frame, form, &rows);

        let buttons = draw_form_buttons(frame, form, &rows);

        self.preview.regions.buttons = buttons;

        // the avatar picker over the form
        if let Some(Modal::Agent(form)) = &mut self.overlays.modal {
            draw_agent_form_color(frame, inner, form);
        }
    }
}

fn draw_agent_form_color(frame: &mut Frame<'_>, inner: Rect, form: &mut Box<super::AgentForm>) {
    let color = agents::color(Some(agents::COLORS[form.color]));

    if let Some(path) = form.picker.as_mut() {
        path.render(frame, inner, color);
    }
}

fn draw_agent_form_body(thread: &str, on: bool) -> Span<'static> {
    Span::styled(
        format!("{thread:<10}"),
        if on { theme::accent_bold() } else { theme::muted() },
    )
}

fn draw_form_fields(
    frame: &mut Frame<'_>,
    form: &mut Box<super::AgentForm>,
    rows: &std::rc::Rc<[Rect]>,
    lab: impl Fn(&str, bool) -> Span<'static>,
) {
    draw_form_field_list(frame, form, rows, lab);
}

fn draw_form_field_list(
    frame: &mut Frame<'_>,
    form: &mut Box<super::AgentForm>,
    rows: &std::rc::Rc<[Rect]>,
    lab: impl Fn(&str, bool) -> Span<'static>,
) {
    for (i, name) in ["name", "harness", "model", "effort", "color", "mode", "style"]
        .iter()
        .enumerate()
    {
        draw_form_field_list_row(frame, form, rows, &lab, i, name);
    }
}

fn draw_form_field_list_row(
    frame: &mut Frame<'_>,
    form: &mut Box<super::AgentForm>,
    rows: &std::rc::Rc<[Rect]>,
    lab: &impl Fn(&str, bool) -> Span<'static>,
    i: usize,
    name: &&str,
) {
    let on = form.field == i;
    let row = rows[i];

    frame.render_widget(Paragraph::new(Line::from(lab(name, on))), row);
    let vr = Rect {
        x: row.x + 10,
        width: row.width.saturating_sub(10),
        ..row
    };

    form.areas[i] = row;

    match i {
        F_MODEL if !form.is_claude() => {
            let message = form.model_id();

            let st = if on {
                theme::bold().bg(theme::pal().surface0)
            } else {
                theme::bold()
            };

            let line = Line::from(vec![
                Span::styled(if on { "▾" } else { " " }, theme::accent()),
                Span::styled(format!(" {message} "), st),
                Span::styled(format!("  {} models: enter", form.harness_name()), theme::muted()),
            ]);

            frame.render_widget(Paragraph::new(line), vr);
        }
        F_NAME | F_HARNESS => {
            frame.render_widget(Paragraph::new("").style(theme::panel()), vr);
            let ta = if i == 0 { &mut form.name } else { &mut form.harness };

            ta.render(
                frame,
                Rect {
                    x: vr.x + 1,
                    width: vr.width.saturating_sub(2),
                    ..vr
                },
                on,
            );
        }
        _ => {
            draw_form_field_list_row_labels(frame, form, i, on, vr);
        }
    }
}

fn draw_form_field_list_row_labels(
    frame: &mut Frame<'_>,
    form: &mut Box<super::AgentForm>,
    i: usize,
    on: bool,
    vr: Rect,
) {
    let labels: Vec<(String, Style)> = match i {
        F_MODEL => form
            .models
            .iter()
            .map(|message_value| (message_value.clone(), theme::text()))
            .collect(),
        F_EFFORT => agents::EFFORTS
            .iter()
            .map(|error| (error.to_string(), theme::text()))
            .collect(),
        F_COLOR => agents::COLORS
            .iter()
            .map(|style_value| {
                (
                    style_value.to_string(),
                    Style::default().fg(agents::color(Some(style_value))),
                )
            })
            .collect(),
        F_MODE => agents::MODES
            .iter()
            .map(|message_2| (message_2.to_string(), theme::text()))
            .collect(),
        _ => form
            .styles
            .iter()
            .map(|(_, count)| (count.clone(), theme::text()))
            .collect(),
    };

    let cur = [form.model, form.effort, form.color, form.mode, form.style][i - F_MODEL];
    let mut spans = vec![Span::styled(if on { "‹" } else { " " }, theme::accent())];

    for (j, (line_value, st)) in labels.into_iter().enumerate() {
        draw_form_field_list_row_labels_body(on, cur, &mut spans, j, &line_value, st);
    }

    spans.push(Span::styled(if on { "›" } else { "" }, theme::accent()));
    frame.render_widget(Paragraph::new(Line::from(spans)), vr);
}

fn draw_form_field_list_row_labels_body(
    on: bool,
    cur: usize,
    spans: &mut Vec<Span<'_>>,
    j: usize,
    line_value: &str,
    st: Style,
) {
    let st = if j == cur {
        let st = st.add_modifier(Modifier::BOLD);

        if on { st.bg(theme::pal().surface0) } else { st }
    } else {
        theme::muted()
    };

    spans.push(Span::styled(format!(" {line_value} "), st));
}

fn draw_form_note(
    frame: &mut Frame<'_>,
    global: Option<&str>,
    form: &mut Box<super::AgentForm>,
    rows: &std::rc::Rc<[Rect]>,
) {
    let note = rows[7];

    let note_text = if form.field == F_EFFORT {
        "effort: global, ~/.claude/settings.json effortLevel · every Claude agent + Claude Code".to_string()
    } else if form.field == F_HARNESS {
        "enter: installed harnesses · or type harness/model".to_string()
    } else if form.field == F_MODEL {
        format!(
            "enter: the models of {}{}",
            form.harness_name(),
            if form.is_claude() { " · ←→ cycle" } else { "" }
        )
    } else {
        form.style_note(global)
    };

    frame.render_widget(
        Paragraph::new(Line::styled(note_text, theme::muted())),
        Rect {
            x: note.x + 11,
            width: note.width.saturating_sub(11),
            ..note
        },
    );
}

fn draw_form_servers(
    frame: &mut Frame<'_>,
    form: &mut Box<super::AgentForm>,
    rows: &std::rc::Rc<[Rect]>,
    lab: impl Fn(&str, bool) -> Span<'static>,
) {
    let on = form.field == F_MCP;
    let mr = rows[8];

    form.areas[F_MCP] = mr;
    let mut spans = vec![
        lab("mcp", on),
        Span::styled(if on { "‹" } else { " " }, theme::accent()),
    ];

    if form.mcp.is_empty() {
        spans.push(Span::styled(
            " 0 servers · settings › mcp servers: add one",
            theme::muted(),
        ));
    }

    for (j, (name, sel)) in form.mcp.iter().enumerate() {
        let st = match (on && j == form.mcp_cur, *sel) {
            (true, _) => theme::bold().bg(theme::pal().surface0),
            (false, true) => theme::bold(),
            _ => theme::muted(),
        };

        spans.push(Span::styled(format!(" {} {name} ", if *sel { "☑" } else { "☐" }), st));
    }

    spans.push(Span::styled(
        if on { "›  space: on / off" } else { "" },
        theme::accent(),
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), mr);
}

fn draw_form_categories(
    frame: &mut Frame<'_>,
    form: &mut Box<super::AgentForm>,
    rows: &std::rc::Rc<[Rect]>,
    lab: impl Fn(&str, bool) -> Span<'static>,
) -> crate::domain::avatar::Cat {
    let on = form.field == F_SET;
    let tr = rows[10];

    form.areas[F_SET] = tr;
    let cat = form.sprite.avatar.cat;
    let mut spans = vec![lab("avatars", on), Span::raw(" ")];

    for character in crate::domain::avatar::CATS {
        draw_form_categories_body(on, cat, &mut spans, character);
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), tr);

    cat
}

fn draw_form_categories_body(
    on: bool,
    cat: crate::domain::avatar::Cat,
    spans: &mut Vec<Span<'_>>,
    character: crate::domain::avatar::Cat,
) {
    let st = if character == cat {
        if on {
            theme::bold().bg(theme::pal().surface0)
        } else {
            theme::bold()
        }
    } else {
        theme::muted()
    };

    spans.push(Span::styled(format!(" {} ", character.name()), st));
}

fn draw_form_avatar(
    frame: &mut Frame<'_>,
    form: &mut Box<super::AgentForm>,
    rows: &std::rc::Rc<[Rect]>,
    lab: impl Fn(&str, bool) -> Span<'static>,
    cat: crate::domain::avatar::Cat,
) {
    let on = form.field == F_SPRITE;
    let sr = rows[11];

    form.areas[F_SPRITE] = sr;
    frame.render_widget(Paragraph::new(Line::from(lab("avatar", on))), sr);
    let color = agents::color(Some(agents::COLORS[form.color]));
    let agent = form.sprite.avatar;
    let side = vec![
        vec![Span::styled(
            format!("   {}", agent.name()),
            if on { theme::bold() } else { theme::muted() },
        )],
        vec![Span::styled(
            format!(
                "   {} {}/{} · float {}x",
                cat.name(),
                agent.idx + 1,
                cat.count(),
                agent.speed
            ),
            theme::muted(),
        )],
        vec![],
        vec![Span::styled(
            "   enter: pick · r random",
            if on || form.field == F_SET {
                theme::muted()
            } else {
                Style::default().fg(theme::pal().overlay0)
            },
        )],
    ];

    frame.render_widget(
        Paragraph::new(sprites::lines(&form.sprite, color, side)),
        Rect {
            x: sr.x + 11,
            width: sr.width.saturating_sub(11),
            ..sr
        },
    );
}

fn draw_form_text(frame: &mut Frame<'_>, form: &mut Box<super::AgentForm>, rows: &std::rc::Rc<[Rect]>) {
    for (field, label_row, box_row, head) in [
        (F_VOICE, rows[13], rows[14], "voice · own tone on top of the style"),
        (F_PROMPT, rows[16], rows[17], "system prompt · standing instructions"),
    ] {
        let on = form.field == field;

        frame.render_widget(
            Paragraph::new(Line::styled(
                head,
                if on { theme::accent_bold() } else { theme::muted() },
            )),
            label_row,
        );
        frame.render_widget(Paragraph::new("").style(theme::panel()), box_row);
        form.areas[field] = box_row;

        let ta = if field == F_VOICE {
            &mut form.voice
        } else {
            &mut form.prompt
        };

        ta.render(
            frame,
            Rect {
                x: box_row.x + 1,
                width: box_row.width.saturating_sub(2),
                ..box_row
            },
            on,
        );
    }

    if let Some(error_value) = &form.error {
        frame.render_widget(
            Paragraph::new(Line::styled(
                error_value.clone(),
                Style::default().fg(theme::pal().error()),
            )),
            rows[19],
        );
    }
}

fn draw_form_buttons(frame: &mut Frame<'_>, form: &mut Box<super::AgentForm>, rows: &[Rect]) -> Vec<(Rect, bool)> {
    let bytes = rows[20];
    let labels = [
        (
            true,
            if form.edit.is_some() {
                "  save (ctrl+s)  "
            } else {
                "  create (ctrl+s)  "
            },
        ),
        (false, "  cancel (esc)  "),
    ];
    let total: u16 = labels.iter().map(|(_, line_2)| line_2.width() as u16).sum::<u16>() + 2;
    let mut x = bytes.x + bytes.width.saturating_sub(total) / 2;
    let mut buttons = Vec::new();

    for (value, line_3) in labels {
        let focused = form.field == F_BUTTONS && value == form.save;

        let style = if focused {
            theme::pill().add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(theme::pal().surface0).fg(theme::pal().text)
        };

        let area_value = Rect {
            x,
            y: bytes.y,
            width: line_3.width() as u16,
            height: 1,
        };

        frame.render_widget(Paragraph::new(line_3).style(style), area_value);
        buttons.push((area_value, value));
        x += area_value.width + 2;
    }

    buttons
}
