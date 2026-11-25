use super::{
    ALIASES, Block, BorderType, Borders, Form, Frame, Hit, INDENT, Line, Paragraph, Rect, SUBS, Settings, Span, Sub,
    group_head, theme,
};

impl Form {
    pub(crate) fn draw(&mut self, frame: &mut Frame, area: Rect, active: bool) {
        draw_form(self, frame, area, active);
    }
}

impl Settings {
    pub(crate) fn draw(&mut self, frame: &mut Frame, area: Rect, active: bool) {
        draw_settings(self, frame, area, active);
    }
}

struct FormRows {
    items: Vec<(usize, usize)>,
    headings: Vec<(usize, &'static str, &'static str)>,
    alias_heading: usize,
    height: usize,
}

struct FormLayout {
    all: Rect,
    body: Rect,
    label_width: u16,
    offset: usize,
}

impl FormLayout {
    fn at(&self, y: usize, height: usize) -> Option<Rect> {
        let row = y.checked_sub(self.offset)?;

        (row + height <= self.body.height as usize).then_some(Rect {
            y: self.body.y + row as u16,
            height: height as u16,
            ..self.body
        })
    }

    fn full(&self, area: Rect) -> Rect {
        Rect {
            x: self.all.x,
            width: self.all.width,
            ..area
        }
    }
}

fn form_rows(form: &Form) -> FormRows {
    let mut items = Vec::new();
    let mut headings = Vec::new();
    let mut y = 0;

    for i in 0..form.plain() {
        if let Some((_, title, description)) = form.sections.iter().find(|section| section.0 == i) {
            headings.push((y, *title, *description));
            y += 3;
        }

        items.push((y, 2));
        y += 3;
    }

    let alias_heading = y;

    if form.aliases_on {
        y += 3;
    }

    for _ in 0..super::aliases(form) {
        items.push((y, 4));
        y += 4;
    }

    if form.aliases_on {
        items.push((y, 1));
    }

    FormRows {
        items,
        headings,
        alias_heading,
        height: y + 1,
    }
}

fn form_layout(form: &mut Form, area: Rect, rows: &FormRows) -> FormLayout {
    let all = Rect {
        y: area.y + 2,
        height: area.height - 3,
        ..area
    };
    let body = Rect {
        x: all.x + INDENT,
        width: all.width.saturating_sub(INDENT),
        ..all
    };
    let (top, height) = rows.items[form.focus.min(rows.items.len() - 1)];
    let view = body.height as usize;

    if top < form.offset {
        form.offset = top;
    }

    if top + height > form.offset + view {
        form.offset = (top + height).saturating_sub(view);
    }

    form.offset = form.offset.min(rows.height.saturating_sub(view));

    FormLayout {
        all,
        body,
        label_width: 12.min(body.width / 3),
        offset: form.offset,
    }
}

fn draw_form(form: &mut Form, frame: &mut Frame, area: Rect, active: bool) {
    super::updates::sync_paths(form);
    form.paths.hide();
    form.rows.clear();

    // Clear stale input areas when scrolling; only visible inputs accept clicks.
    for input in &mut form.fields {
        input.hide();
    }

    if area.height < 5 || area.width < 16 {
        return;
    }

    draw_form_path(form, frame, area);
    let rows = form_rows(form);
    let layout = form_layout(form, area, &rows);

    draw_form_headings(form, frame, &rows, &layout);
    let palette = theme::pal();

    for i in 0..form.plain() {
        let Some(row) = layout.at(rows.items[i].0, 1) else {
            continue;
        };

        draw_plain_field(form, frame, row, i, active, &layout, &palette);
        draw_field_hint(form, frame, i, active, &layout, &rows);
    }

    for alias in 0..super::aliases(form) {
        let item = form.plain() + alias;

        let Some(alias_area) = layout.at(rows.items[item].0, 4) else {
            continue;
        };

        draw_alias_box(form, frame, alias_area, item, alias, active);
    }

    draw_alias_add(form, frame, active, &rows, &layout);
    draw_form_footer(form, frame, area, active);
}

fn draw_form_path(form: &Form, frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                if super::dirty(form) { "not applied · " } else { "" },
                theme::text().fg(theme::pal().caution()),
            ),
            Span::styled(form.path.display().to_string(), theme::muted()),
        ])),
        Rect { height: 1, ..area },
    );
}

fn draw_form_headings(form: &Form, frame: &mut Frame, rows: &FormRows, layout: &FormLayout) {
    for &(y, title, description) in &rows.headings {
        group_head(
            frame,
            layout.at(y, 1).map(|area| layout.full(area)),
            layout.at(y + 1, 1).map(|area| layout.full(area)),
            title,
            description,
        );
    }

    if form.aliases_on {
        group_head(
            frame,
            layout.at(rows.alias_heading, 1).map(|area| layout.full(area)),
            layout.at(rows.alias_heading + 1, 1).map(|area| layout.full(area)),
            ALIASES.0,
            ALIASES.1,
        );
    }
}

fn draw_plain_field(
    form: &mut Form,
    frame: &mut Frame,
    row: Rect,
    i: usize,
    active: bool,
    layout: &FormLayout,
    palette: &crate::domain::palette::Palette,
) {
    let focused = active && form.focus == i;

    frame.render_widget(
        Paragraph::new(Line::styled(
            form.specs[i].label,
            if focused { theme::accent_bold() } else { theme::muted() },
        )),
        Rect {
            width: layout.label_width,
            ..row
        },
    );

    // right of the input: a color's swatch, ↺ (back to the default) while the field is set
    let swatch = form.specs[i]
        .key
        .strip_prefix("colors.")
        .and_then(|color| palette.get(color));
    let tail = 3 + if swatch.is_some() { 3 } else { 0 };
    let input = Rect {
        x: row.x + layout.label_width,
        width: row.width.saturating_sub(layout.label_width + tail),
        ..row
    };

    frame.render_widget(Paragraph::new("").style(theme::panel()), input);
    form.fields[i].render(frame, input, focused);
    let mut x = input.right() + 1;

    if let Some(color) = swatch {
        frame.render_widget(
            Paragraph::new(Span::styled("██", ratatui::style::Style::default().fg(color))),
            Rect { x, width: 2, ..row },
        );
        x += 3;
    }

    if !form.fields[i].text().is_empty() {
        let reset_area = Rect { x, width: 1, ..row };

        frame.render_widget(
            Paragraph::new(Span::styled(
                "↺",
                if focused { theme::accent() } else { theme::muted() },
            )),
            reset_area,
        );
        form.rows.push((reset_area, Hit::Reset(i)));
    }

    form.rows.push((row, Hit::Field(i)));
}

fn draw_field_hint(form: &Form, frame: &mut Frame, i: usize, active: bool, layout: &FormLayout, rows: &FormRows) {
    let focused = active && form.focus == i;

    if let Some(hint_row) = layout.at(rows.items[i].0 + 1, 1) {
        let env = form.specs[i].env;

        let hint = (!env.is_empty())
            .then(|| std::env::var(env).ok().filter(|value| !value.is_empty()))
            .flatten()
            .map_or_else(
                || {
                    let reset = reset_hint(form, i, focused);

                    format!("{}{reset}", form.specs[i].help)
                },
                |value| format!("{env} override: {value}"),
            );

        frame.render_widget(
            Paragraph::new(Line::styled(hint, theme::muted())),
            Rect {
                x: hint_row.x + layout.label_width,
                width: hint_row.width - layout.label_width,
                ..hint_row
            },
        );
    }
}

fn reset_hint(form: &Form, i: usize, focused: bool) -> &str {
    if focused && !form.fields[i].text().is_empty() {
        " · ctrl+d / ↺: default"
    } else {
        ""
    }
}

fn draw_alias_box(form: &mut Form, frame: &mut Frame, area: Rect, item: usize, alias: usize, active: bool) {
    let selected = active && form.focus == item;
    let open = selected && form.editing.is_some();
    let border = if selected { theme::accent_bold() } else { theme::line() };

    let title = match (selected, open) {
        (true, true) => format!(" alias {} · enter: done ", alias + 1),
        (true, false) => format!(" alias {} · enter: edit · d: delete ", alias + 1),
        _ => format!(" alias {} ", alias + 1),
    };

    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(border)
        .title(Span::styled(
            title,
            if selected { theme::accent_bold() } else { theme::muted() },
        ));
    let inner = block.inner(area);

    frame.render_widget(block, area);

    if selected && !open {
        // locked + highlighted: the whole box
        frame.render_widget(Paragraph::new("").style(theme::panel()), inner);
    }

    draw_alias_inputs(form, frame, inner, alias, open);

    if selected && !open {
        // locked + highlighted: accent bar on the far left column
        theme::accent_edge(frame.buffer_mut(), area);
    }
}

fn draw_alias_inputs(form: &mut Form, frame: &mut Frame, inner: Rect, alias: usize, open: bool) {
    for (j, label) in ["name", "target"].into_iter().enumerate() {
        let i = form.plain() + alias * 2 + j;
        let row = Rect {
            y: inner.y + j as u16,
            height: 1,
            x: inner.x + 1,
            width: inner.width.saturating_sub(2),
        };
        let focused = open && form.editing == Some(i);
        let label_width = 8.min(row.width / 3);

        frame.render_widget(
            Paragraph::new(Line::styled(
                label,
                if focused { theme::accent_bold() } else { theme::muted() },
            )),
            Rect {
                width: label_width,
                ..row
            },
        );
        let input = Rect {
            x: row.x + label_width,
            width: row.width - label_width,
            ..row
        };

        draw_alias_input_background(frame, open, input);

        form.fields[i].render(frame, input, focused);
        form.rows.push((row, Hit::Field(i)));
    }
}

fn draw_alias_add(form: &mut Form, frame: &mut Frame, active: bool, rows: &FormRows, layout: &FormLayout) {
    if form.aliases_on
        && let Some(row) = layout.at(rows.items[super::add_item(form)].0, 1)
    {
        let focused = active && form.focus == super::add_item(form);

        frame.render_widget(
            Paragraph::new(Line::styled(
                " + add path alias (enter) ",
                if focused { theme::pill() } else { theme::panel() },
            )),
            row,
        );
        form.rows.push((row, Hit::Add));
    }
}

fn draw_form_footer(form: &mut Form, frame: &mut Frame, area: Rect, active: bool) {
    frame.render_widget(
        Paragraph::new(Line::styled(
            form.status.clone(),
            if form.error {
                theme::text().fg(theme::pal().error())
            } else {
                theme::muted()
            },
        )),
        Rect {
            y: area.bottom() - 1,
            height: 1,
            ..area
        },
    );

    if active && let Some(index) = form.path_focus {
        let input = &form.fields[index];

        form.paths.render(frame, input, area, input.area());
    }
}

fn draw_alias_input_background(frame: &mut Frame<'_>, open: bool, input: Rect) {
    if open {
        frame.render_widget(Paragraph::new("").style(theme::panel()), input);
    }
}

fn draw_settings(settings: &mut Settings, frame: &mut Frame, area: Rect, active: bool) {
    settings.bar_hits.clear();

    if area.height < 6 || area.width < 16 {
        return;
    }

    let mut x = area.x;
    let mut spans = Vec::new();

    for (sub, label) in SUBS {
        let selected = sub == settings.sub;

        let style = match (selected, active && settings.bar) {
            (true, true) => theme::pill(),
            (true, false) => theme::accent_bold(),
            _ => theme::muted(),
        };

        let text = format!(" {label} ");
        let width = text.chars().count() as u16;

        settings.bar_hits.push((
            Rect {
                x,
                y: area.y,
                width,
                height: 1,
            },
            sub,
        ));
        x += width + 1;
        spans.push(Span::styled(text, style));
        spans.push(Span::raw(" "));
    }

    if settings.sub == Sub::Harnesses {
        let missing = crate::features::settings::harness_tab::missing();

        if !missing.is_empty() {
            spans.push(Span::styled(
                format!(
                    "  not installed: {}",
                    missing.iter().map(|harness| harness.0).collect::<Vec<_>>().join(", ")
                ),
                theme::muted(),
            ));
        }
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), Rect { height: 1, ..area });
    let body = Rect {
        y: area.y + 2,
        height: area.height - 2,
        ..area
    };
    let inner = active && !settings.bar;

    match settings.sub {
        Sub::Codehead => settings.form.draw(frame, body, inner),
        Sub::Harnesses => settings.harness.draw(frame, body, inner),
        Sub::Mcp => settings.mcp.draw(frame, body, inner),
        Sub::Shortcuts => settings.shortcuts.draw(frame, body, inner),
    }
}
