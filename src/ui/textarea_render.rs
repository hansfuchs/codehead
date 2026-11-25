use super::{
    Frame, Line, Modifier, Paragraph, Pos, Position, Rect, Span, TextArea, fence_colors, list_item, mention_colors,
    theme,
};

impl TextArea {
    pub(crate) fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        self.area = area;
        self.vis.clear();
        self.layout();
        let width = area.width.saturating_sub(1) as usize;
        let height_value = area.height as usize;
        let (crow, cx) = self.visual_of(self.cur);

        if self.follow {
            self.follow_cursor(height_value, crow);
        }

        self.scroll = self.scroll.min(self.vis.len().saturating_sub(1));
        let base = self.style.unwrap_or_else(theme::panel);

        frame.render_widget(Paragraph::new("").style(base), area);

        if self.is_empty() && !self.placeholder.is_empty() {
            frame.render_widget(
                Paragraph::new(self.placeholder.as_str()).style(base.fg(theme::pal().subtext0)),
                area,
            );
        } else {
            draw_text_rows(frame, area, height_value, base, self);
        }

        if focused && crow >= self.scroll && crow < self.scroll + height_value {
            frame.set_cursor_position(Position {
                x: area.x + cx.min(width) as u16,
                y: area.y + (crow - self.scroll) as u16,
            });
        }
    }

    fn follow_cursor(&mut self, height_value: usize, crow: usize) {
        if crow < self.scroll {
            self.scroll = crow;
        } else if crow >= self.scroll + height_value {
            self.scroll = crow + 1 - height_value;
        }
    }
}

fn draw_text_rows(
    frame: &mut Frame<'_>,
    area: Rect,
    height_value: usize,
    base: ratatui::prelude::Style,
    widget_state: &mut TextArea,
) {
    let sel = widget_state.range();
    let sel_style = theme::pill();
    let mut lines = Vec::new();

    let (code, fence) = if widget_state.single {
        (Vec::new(), Vec::new())
    } else {
        fence_colors(&widget_state.lines)
    };

    for text in widget_state.vis.iter().skip(widget_state.scroll).take(height_value) {
        draw_text_row(
            widget_state,
            &mut lines,
            text,
            TextRowStyles {
                base,
                sel,
                sel_style,
                code: &code,
                fence: &fence,
            },
        );
    }

    frame.render_widget(Paragraph::new(lines).style(base), area);
}

fn draw_text_row(widget_state: &TextArea, lines: &mut Vec<Line<'_>>, text: &super::Seg, settings: TextRowStyles<'_>) {
    let TextRowStyles {
        base,
        sel,
        sel_style,
        code,
        fence,
    } = settings;

    let line = &widget_state.lines[text.row];
    let colors = mention_colors(line, &widget_state.marks);
    let code_row = code.get(text.row).and_then(|character| character.as_ref());
    let fence_row = fence.get(text.row).copied().unwrap_or(false);

    // `- ` / `* ` / `+ ` items outside code show as `•` (the text keeps the marker)
    let bullet = (code_row.is_none() && !fence_row && !widget_state.single)
        .then(|| bullet_column(line))
        .flatten();

    let mut spans: Vec<Span> = Vec::new();
    let (mut buf, mut buf_style) = (String::new(), base);

    for (col, &ch) in line.iter().enumerate().take(text.end).skip(text.start) {
        let path = Pos { row: text.row, col };

        let st = if sel.is_some_and(|(agent, bytes)| path >= agent && path < bytes) {
            sel_style
        } else if let Some(character_2) = colors[col] {
            base.fg(character_2).add_modifier(Modifier::BOLD)
        } else if let Some(character_3) = code_row.and_then(|result| result.get(col)) {
            base.fg(*character_3)
        } else if fence_row {
            base.fg(theme::pal().subtext0)
        } else {
            base
        };

        if st != buf_style && !buf.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut buf), buf_style));
        }

        buf_style = st;
        buf.push(if bullet == Some(col) { '•' } else { ch });
    }

    // a selected line break shows as 1 selected cell
    let nl_selected = text.last
        && sel.is_some_and(|(agent_value, bytes_value)| {
            Pos {
                row: text.row,
                col: text.end,
            } >= agent_value
                && Pos {
                    row: text.row,
                    col: text.end,
                } < bytes_value
        });

    if !buf.is_empty() {
        spans.push(Span::styled(buf, buf_style));
    }

    if nl_selected {
        spans.push(Span::styled(" ", sel_style));
    }

    lines.push(Line::from(spans));
}

fn bullet_column(line: &[char]) -> Option<usize> {
    list_item(line)
        .filter(|(_, width_value, _)| {
            *width_value <= 6
                && matches!(
                    line.get(
                        line.iter()
                            .take_while(|character_value| **character_value == ' ')
                            .count()
                    ),
                    Some('-' | '*' | '+')
                )
        })
        .map(|(indent, _, _)| indent)
}

#[derive(Clone, Copy)]
struct TextRowStyles<'context> {
    base: ratatui::prelude::Style,
    sel: Option<(Pos, Pos)>,
    sel_style: ratatui::prelude::Style,
    code: &'context [Option<Vec<ratatui::prelude::Color>>],
    fence: &'context [bool],
}
