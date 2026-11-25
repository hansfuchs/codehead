use super::{Duration, Frame, Line, Paragraph, Rect, Row, Sec, Source, Span, Style, SystemTime, View, theme};
use std::path::Path;

impl View {
    pub(crate) fn draw(&mut self, frame: &mut Frame, area: Rect) {
        draw_view(self, frame, area);
    }

    fn draw_text_preview(&mut self, frame: &mut Frame<'_>, preview: Rect, path: &Path) {
        let Some(lines) = self.preview_lines(path, preview.width.saturating_sub(1)) else {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    format!(" {} · reading…", self.shown(path)),
                    theme::muted(),
                )),
                Rect { height: 1, ..preview },
            );
            return;
        };

        self.text_preview.scroll = self.text_preview.scroll.min(lines.len().saturating_sub(1));
        let head = Line::from(vec![
            Span::styled(format!(" {} ", self.shown(path)), theme::bold()),
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
    }
}

fn draw_section_heading(frame: &mut Frame<'_>, area: Rect, section: Sec) {
    frame.render_widget(
        Paragraph::new(Span::styled(
            format!(" {}", section.label()),
            theme::muted().add_modifier(ratatui::style::Modifier::BOLD),
        )),
        area,
    );
}

fn draw_file_row(frame: &mut Frame<'_>, lw: u16, now: u64, hit: Style, area: Rect, position: usize, view: &mut View) {
    let (i, marks) = &view.visible[position];
    let entry = &view.files[*i];

    let tag = match entry.source {
        Source::Agent => Span::styled("✎ ", theme::fg(theme::pal().info())),
        Source::Note => Span::styled("◆ ", theme::muted()),
        Source::Named => Span::styled("↗ ", theme::muted()),
        Source::Attached(_) => Span::styled("⧉ ", theme::muted()),
    };

    let age = crate::domain::notes::ago(SystemTime::UNIX_EPOCH + Duration::from_secs(entry.at.min(now)));
    let name = view.label(entry);
    let room = usize::from(lw).saturating_sub(age.chars().count() + 5);
    let sel = position == view.at;
    let st = if sel { theme::pill() } else { Style::default() };

    // a long path: its end shown (`…` + the tail), the marks moved along
    let length = name.chars().count();

    let (fitted, marks): (String, Vec<usize>) = if length <= room || room < 2 {
        (name.clone(), marks.clone())
    } else {
        let cut = length - (room - 1);

        (
            format!("…{}", name.chars().skip(cut).collect::<String>()),
            marks
                .iter()
                .filter(|&&mark| mark >= cut)
                .map(|mark| mark - cut + 1)
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
    frame.render_widget(Paragraph::new(Line::from(spans)).style(st), area);
}

fn draw_list_row(
    frame: &mut Frame<'_>,
    context: FileListLayout,
    now: u64,
    hit: Style,
    index: usize,
    row: &Row,
    view: &mut View,
) {
    let FileListLayout { lw, list, top } = context;

    let y = list.y + (index - top) as u16;
    let area = Rect { y, height: 1, ..list };

    view.drawn.push((y, *row));

    match *row {
        Row::Gap => {}
        Row::Head(section) => {
            draw_section_heading(frame, area, section);
        }
        Row::File(position) => {
            draw_file_row(frame, lw, now, hit, area, position, view);
        }
    }
}

fn note_list_topic(hit: &mut bool, topic: &crate::infra::bus::Topic, view: &View) {
    *hit |= match &topic {
        crate::infra::bus::Topic::Thread(id, crate::infra::bus::Part::Log | crate::infra::bus::Part::Other) => {
            *id == view.id
        }
        crate::infra::bus::Topic::Work(dir) | crate::infra::bus::Topic::Rescan(dir) => view
            .source_reads
            .roots
            .iter()
            .any(|root| root.starts_with(dir) || dir.starts_with(root)),
        _ => false,
    };
}

#[derive(Clone, Copy)]
struct FileListLayout {
    lw: u16,
    list: Rect,
    top: usize,
}

fn draw_view(view: &mut View, frame: &mut Frame, area: Rect) {
    view.poll();
    let mut hit = false;

    for topic in view.source_reads.topics.try_iter() {
        note_list_topic(&mut hit, &topic, view);
    }

    view.refresh(hit);

    if view.files.is_empty() {
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

    draw_filter_header(view, frame, fr);

    view.list = list;
    view.preview = preview;

    // the divider
    for y in area.y..area.bottom() {
        frame.buffer_mut()[(area.x + lw, y)]
            .set_symbol("│")
            .set_style(theme::line());
    }

    view.drawn.clear();

    if view.visible.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled("  0 matches", theme::muted())),
            Rect { height: 1, ..list },
        );
        return;
    }

    view.at = view.at.min(view.visible.len() - 1);
    let rows = view.rows();
    let height = usize::from(list.height);

    let top = first_visible_row(&rows, view.at, height);

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());

    draw_rows_and_preview(
        view,
        frame,
        FileRowsLayout {
            lw,
            list,
            preview,
            rows: &rows,
            top,
            now,
        },
        height,
    );
}

fn draw_rows_and_preview(view: &mut View, frame: &mut Frame<'_>, file_rows_layout: FileRowsLayout<'_>, height: usize) {
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

    for (index, row) in rows.iter().enumerate().skip(top).take(height) {
        draw_list_row(frame, FileListLayout { lw, list, top }, now, hit, index, row, view);
    }

    view.source_reads
        .list_fx
        .apply(frame, list, view.source_reads.list_version);

    let Some(path) = view.cur().map(|entry| entry.path.clone()) else {
        return;
    };

    let body = Rect {
        x: preview.x + 1,
        y: preview.y + 2,
        height: preview.height.saturating_sub(2),
        width: preview.width.saturating_sub(1),
    };

    // images + pdfs: drawn by the terminal (0 dissolve over graphics)
    if view.draw_image(frame, &path, Rect { height: 1, ..preview }, body) {
        return;
    }

    view.draw_text_preview(frame, preview, &path);
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

fn draw_filter_header(view: &mut View, frame: &mut Frame, fr: Rect) {
    frame.render_widget(
        Paragraph::new(Span::styled(
            " › ",
            if view.filter_input.filtering {
                theme::accent_bold()
            } else {
                theme::muted()
            },
        )),
        Rect { width: 3, ..fr },
    );
    let query = view.filter_input.filter.text();

    let count = if query.trim().is_empty() {
        String::new()
    } else {
        format!(" {} of {}", view.visible.len(), view.files.len())
    };

    let cw = count.chars().count() as u16;

    view.filter_input.filter.render(
        frame,
        Rect {
            x: fr.x + 3,
            width: fr.width.saturating_sub(3 + cw),
            ..fr
        },
        view.filter_input.filtering,
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
struct FileRowsLayout<'rows> {
    lw: u16,
    list: Rect,
    preview: Rect,
    rows: &'rows [Row],
    top: usize,
    now: u64,
}

fn first_visible_row(rows: &[Row], cursor: usize, height: usize) -> usize {
    // the cursor row in view; the heading above the 1st file in view where it fits
    let cur_row = rows.iter().position(|row| *row == Row::File(cursor)).unwrap_or(0);
    let top = cur_row.saturating_sub(height.saturating_sub(1));

    if top > 0 && matches!(rows.get(top.saturating_sub(1)), Some(Row::Head(_))) && cur_row - (top - 1) < height {
        top - 1
    } else {
        top
    }
}
