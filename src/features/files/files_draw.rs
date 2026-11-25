use super::{Duration, Frame, Line, Paragraph, Rect, Row, Sec, Source, Span, Style, SystemTime, View, theme};
use std::ops::ControlFlow;
use std::path::Path;

impl View {
    pub(crate) fn draw(&mut self, frame: &mut Frame, area: Rect) {
        draw_view(self, frame, area);
    }

    fn finish_draw(&mut self, frame: &mut Frame<'_>, preview: Rect, path_value: &Path) -> ControlFlow<()> {
        let Some(lines) = self.preview_lines(path_value, preview.width.saturating_sub(1)) else {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    format!(" {} · reading…", self.shown(path_value)),
                    theme::muted(),
                )),
                Rect { height: 1, ..preview },
            );
            return ControlFlow::Break(());
        };

        self.text_preview.scroll = self.text_preview.scroll.min(lines.len().saturating_sub(1));
        let head = Line::from(vec![
            Span::styled(format!(" {} ", self.shown(path_value)), theme::bold()),
            Span::styled(
                format!(
                    "{} lines · e: IDE · ctrl+.: yazi, copy path · ctrl+f: filter",
                    lines.len()
                ),
                theme::muted(),
            ),
        ]);

        frame.render_widget(Paragraph::new(head), Rect { height: 1, ..preview });
        let body: Vec<Line> = lines
            .into_iter()
            .skip(self.text_preview.scroll)
            .take(usize::from(preview.height.saturating_sub(2)))
            .collect();

        frame.render_widget(
            Paragraph::new(body),
            Rect {
                x: preview.x + 1,
                y: preview.y + 2,
                height: preview.height.saturating_sub(2),
                width: preview.width.saturating_sub(1),
            },
        );
        self.text_preview
            .preview_fx
            .apply(frame, preview, self.text_preview.preview_version);

        ControlFlow::Continue(())
    }
}

fn draw_row_head_text_value(frame: &mut Frame<'_>, area_value: Rect, text_value: Sec) {
    frame.render_widget(
        Paragraph::new(Span::styled(
            format!(" {}", text_value.label()),
            theme::muted().add_modifier(ratatui::style::Modifier::BOLD),
        )),
        area_value,
    );
}

fn draw_row_file_key(
    frame: &mut Frame<'_>,
    lw: u16,
    now: u64,
    hit: Style,
    area_value: Rect,
    key: usize,
    view_state: &mut View,
) {
    let (i, marks) = &view_state.visible[key];
    let error = &view_state.files[*i];

    let tag = match error.source {
        Source::Agent => Span::styled("✎ ", theme::fg(theme::pal().info())),
        Source::Note => Span::styled("◆ ", theme::muted()),
        Source::Named => Span::styled("↗ ", theme::muted()),
        Source::Attached(_) => Span::styled("⧉ ", theme::muted()),
    };

    let age = crate::domain::notes::ago(SystemTime::UNIX_EPOCH + Duration::from_secs(error.at.min(now)));
    let name = view_state.label(error);
    let room = usize::from(lw).saturating_sub(age.chars().count() + 5);
    let sel = key == view_state.at;
    let st = if sel { theme::pill() } else { Style::default() };

    // a long path: its end shown (`…` + the tail), the marks moved along
    let count_2 = name.chars().count();

    let (fitted, marks): (String, Vec<usize>) = if count_2 <= room || room < 2 {
        (name.clone(), marks.clone())
    } else {
        let cut = count_2 - (room - 1);

        (
            format!("…{}", name.chars().skip(cut).collect::<String>()),
            marks
                .iter()
                .filter(|&&message| message >= cut)
                .map(|message_value| message_value - cut + 1)
                .collect(),
        )
    };

    let marks = &marks[..];
    let mut spans = vec![Span::raw(" "), tag];

    spans.extend(crate::ui::fuzzy::spans(
        &fitted,
        marks,
        if sel { st } else { theme::text() },
        if sel {
            st.add_modifier(ratatui::style::Modifier::UNDERLINED)
        } else {
            hit
        },
    ));
    spans.push(Span::styled(format!(" {age}"), theme::muted()));
    frame.render_widget(Paragraph::new(Line::from(spans)).style(st), area_value);
}

fn draw_area_value(
    frame: &mut Frame<'_>,
    context: FileListLayout,
    now: u64,
    hit: Style,
    count_value: usize,
    row: &Row,
    view_state: &mut View,
) {
    let FileListLayout { lw, list, top } = context;

    let y = list.y + (count_value - top) as u16;
    let area_value = Rect { y, height: 1, ..list };

    view_state.drawn.push((y, *row));

    match *row {
        Row::Gap => {}
        Row::Head(text_value) => {
            draw_row_head_text_value(frame, area_value, text_value);
        }
        Row::File(key) => {
            draw_row_file_key(frame, lw, now, hit, area_value, key, view_state);
        }
    }
}

fn draw_body(hit: &mut bool, thread: &crate::infra::bus::Topic, view_state: &View) {
    *hit |= match &thread {
        crate::infra::bus::Topic::Thread(id, crate::infra::bus::Part::Log | crate::infra::bus::Part::Other) => {
            *id == view_state.id
        }
        crate::infra::bus::Topic::Work(width_value) | crate::infra::bus::Topic::Rescan(width_value) => view_state
            .source_reads
            .roots
            .iter()
            .any(|result| result.starts_with(width_value) || width_value.starts_with(result)),
        _ => false,
    };
}

#[derive(Clone, Copy)]
struct FileListLayout {
    lw: u16,
    list: Rect,
    top: usize,
}

fn draw_view(state: &mut View, frame: &mut Frame, area: Rect) {
    state.poll();
    let mut hit = false;

    for thread in state.source_reads.topics.try_iter() {
        draw_body(&mut hit, &thread, state);
    }

    state.refresh(hit);

    if state.files.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "  0 files yet: the files the agents write, and the home repo's notes of this thread · menu: yazi",
                theme::muted(),
            )),
            area,
        );
        return;
    }

    let (lw, fr, list, preview) = file_layout(area);

    draw_filter_header(state, frame, fr);

    state.list = list;
    state.preview = preview;

    // the divider
    for y in area.y..area.bottom() {
        frame.buffer_mut()[(area.x + lw, y)]
            .set_symbol("│")
            .set_style(theme::line());
    }

    state.drawn.clear();

    if state.visible.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled("  0 matches", theme::muted())),
            Rect { height: 1, ..list },
        );
        return;
    }

    state.at = state.at.min(state.visible.len() - 1);
    let rows = state.rows();
    let height_value = usize::from(list.height);

    let top = first_visible_row(&rows, state.at, height_value);

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |directory| directory.as_secs());

    if let ControlFlow::Break(()) = finish_draw_view(
        state,
        frame,
        FileRowsLayout {
            lw,
            list,
            preview,
            rows: &rows,
            top,
            now,
        },
        height_value,
    ) {}
}

fn finish_draw_view(
    state: &mut View,
    frame: &mut Frame<'_>,
    file_rows_layout: FileRowsLayout<'_>,
    height_value: usize,
) -> ControlFlow<()> {
    let FileRowsLayout {
        lw,
        list,
        preview,
        rows,
        top,
        now,
    } = file_rows_layout;

    let hit = Style::default()
        .fg(theme::pal().accent)
        .add_modifier(ratatui::style::Modifier::BOLD | ratatui::style::Modifier::UNDERLINED);

    for (count_value, row) in rows.iter().enumerate().skip(top).take(height_value) {
        draw_area_value(
            frame,
            FileListLayout { lw, list, top },
            now,
            hit,
            count_value,
            row,
            state,
        );
    }

    state
        .source_reads
        .list_fx
        .apply(frame, list, state.source_reads.list_version);

    let Some(path_value) = state.cur().map(|error_value| error_value.path.clone()) else {
        return ControlFlow::Break(());
    };

    let body = Rect {
        x: preview.x + 1,
        y: preview.y + 2,
        height: preview.height.saturating_sub(2),
        width: preview.width.saturating_sub(1),
    };

    if state.draw_image(frame, &path_value, Rect { height: 1, ..preview }, body) {
        return ControlFlow::Break(());
    }

    // images + pdfs: drawn by the terminal (0 dissolve over graphics)

    if let ControlFlow::Break(()) = state.finish_draw(frame, preview, &path_value) {}

    ControlFlow::Continue(())
}

fn file_layout(area: Rect) -> (u16, Rect, Rect, Rect) {
    let lw = (area.width * 2 / 5).clamp(24, 60).min(area.width);

    // the filter row on top of the list, a blank row below it
    let fr = Rect {
        width: lw,
        height: 1,
        ..area
    };
    let list = Rect {
        y: area.y + 2,
        height: area.height.saturating_sub(2),
        width: lw,
        ..area
    };
    let preview = Rect {
        x: area.x + lw + 1,
        width: area.width.saturating_sub(lw + 1),
        ..area
    };

    (lw, fr, list, preview)
}

fn draw_filter_header(state: &mut View, frame: &mut Frame, fr: Rect) {
    frame.render_widget(
        Paragraph::new(Span::styled(
            " › ",
            if state.filter_input.filtering {
                theme::accent_bold()
            } else {
                theme::muted()
            },
        )),
        Rect { width: 3, ..fr },
    );
    let query = state.filter_input.filter.text();

    let count = if query.trim().is_empty() {
        String::new()
    } else {
        format!(" {} of {}", state.visible.len(), state.files.len())
    };

    let cw = count.chars().count() as u16;

    state.filter_input.filter.render(
        frame,
        Rect {
            x: fr.x + 3,
            width: fr.width.saturating_sub(3 + cw),
            ..fr
        },
        state.filter_input.filtering,
    );

    if cw > 0 {
        frame.render_widget(
            Paragraph::new(Span::styled(count, theme::muted())),
            Rect {
                x: fr.right().saturating_sub(cw),
                width: cw,
                ..fr
            },
        );
    }
}

#[derive(Clone, Copy)]
struct FileRowsLayout<'reference_0> {
    lw: u16,
    list: Rect,
    preview: Rect,
    rows: &'reference_0 [Row],
    top: usize,
    now: u64,
}

fn first_visible_row(rows: &[Row], cursor: usize, height_value: usize) -> usize {
    // the cursor row in view; the heading above the 1st file in view where it fits
    let cur_row = rows
        .iter()
        .position(|result_value| *result_value == Row::File(cursor))
        .unwrap_or(0);
    let top = cur_row.saturating_sub(height_value.saturating_sub(1));

    if top > 0 && matches!(rows.get(top.saturating_sub(1)), Some(Row::Head(_))) && cur_row - (top - 1) < height_value {
        top - 1
    } else {
        top
    }
}
