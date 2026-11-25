use super::{Frame, Line, Modal, Modifier, Paragraph, QLine, Rect, Span, Style, Target, code_bg, scm, theme};
use ratatui::widgets::{Block, Clear};

pub(super) struct ModalDraw<'a> {
    pub(super) buttons: &'a mut Vec<(Rect, bool)>,
    pub(super) quote_rect: &'a mut Rect,
    pub(super) mention: &'a mut crate::ui::mention::Mention,
    pub(super) agents: &'a [crate::domain::agents::Agent],
}

impl ModalDraw<'_> {
    pub(super) fn draw(&mut self, frame: &mut Frame, area: Rect, modal: &mut Modal) {
        match modal {
            Modal::Comment { .. } => comment(frame, area, modal, self),
            Modal::Confirm { .. } => confirm(frame, area, modal, self),
        }
    }
}

fn quote_line(frame: &mut Frame<'_>, bg: ratatui::prelude::Color, width: u16, line: &QLine, row: Rect) {
    match line {
        QLine::Gap => {}
        QLine::Path(path) => frame.render_widget(
            Paragraph::new(Line::styled(
                crate::ui::thread_card::fit(path, usize::from(width)),
                theme::bold(),
            )),
            row,
        ),
        QLine::Code(code) => {
            let style = match code.chars().next() {
                Some('+') => Style::default().fg(theme::pal().success()),
                Some('-') => Style::default().fg(theme::pal().error()),
                _ => theme::muted(),
            };

            frame.render_widget(
                Paragraph::new(Line::styled(
                    crate::ui::thread_card::fit(code, usize::from(width)),
                    style,
                ))
                .style(Style::default().bg(bg)),
                row,
            );
        }
        QLine::Note(head, note) => frame.render_widget(
            Paragraph::new(Line::styled(
                crate::ui::thread_card::fit(note, usize::from(width)),
                if *head { theme::bold() } else { theme::text() },
            )),
            row,
        ),
    }
}

fn comment_frame(frame: &mut Frame<'_>, title: &mut String, target: &mut Target, width: u16, rect: Rect) {
    let head = match target {
        Target::New => format!(" comment on {title} "),
        Target::Edit(id) => format!(" edit comment {id} · {title} "),
        Target::EditReply(id, reply) => format!(" edit reply {} on {id} · {title} ", *reply + 1),
        Target::Reply(id) => format!(" reply on {id} · {title} "),
    };

    frame.render_widget(
        Block::bordered()
            .border_style(theme::accent())
            .title(Span::styled(
                crate::ui::thread_card::fit(&head, usize::from(width.saturating_sub(4))),
                theme::accent_bold(),
            ))
            .style(theme::text()),
        rect,
    );
}

fn comment_severity(
    frame: &mut Frame<'_>,
    target: &mut Target,
    level: &mut super::rank::Level,
    on_level: &mut bool,
    inner: Rect,
    row_y: &mut u16,
) {
    let mut spans = vec![Span::styled(
        "severity ",
        if *on_level {
            theme::accent_bold()
        } else {
            theme::muted()
        },
    )];

    for candidate in crate::domain::changes::rank::Level::ALL {
        let style = severity_style(candidate, *level, *on_level);

        spans.push(Span::styled(format!(" {} ", candidate.label()), style));
        spans.push(Span::raw(" "));
    }

    spans.push(Span::styled(
        if *on_level {
            " ←→ · ↓ text"
        } else {
            " ↑ on the 1st row: severity"
        },
        theme::muted(),
    ));

    // a reply: 0 severity, the mention hint in its place
    if !target.has_level() {
        spans = vec![Span::styled(
            "@agent: it gets the thread at once · alt+↑↓: scroll the thread",
            theme::muted(),
        )];
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect {
            y: *row_y,
            height: 1,
            ..inner
        },
    );
    *row_y += 2;
}

fn severity_style(
    candidate: crate::domain::changes::rank::Level,
    selected: crate::domain::changes::rank::Level,
    focused: bool,
) -> Style {
    if candidate != selected {
        return theme::muted();
    }

    let background = if focused {
        scm::level_color(candidate)
    } else {
        theme::pal().surface0
    };

    Style::default()
        .bg(background)
        .fg(if focused {
            theme::pal().on_accent
        } else {
            scm::level_color(candidate)
        })
        .add_modifier(Modifier::BOLD)
}

fn comment(frame: &mut Frame, area: Rect, modal: &mut Modal, modal_draw: &mut ModalDraw<'_>) {
    let Modal::Comment {
        title,
        quote,
        scroll,
        target,
        text,
        level,
        on_level,
        ..
    } = modal
    else {
        return;
    };

    let width = area.width.saturating_sub(4).clamp(20, 110);
    let x = area.x + (area.width.saturating_sub(width)) / 2;

    let text_width = width.saturating_sub(4);
    let text_height = (text.rows_for(text_width) as u16).clamp(3, 8);

    // the selected lines: at most half the screen (and 24 rows); more: they scroll
    let room = (area.height.saturating_sub(text_height + 8) as usize)
        .min(usize::from(area.height / 2).max(3))
        .min(24);
    let quote_height = quote.len().min(room);

    *scroll = (*scroll).min(quote.len() - quote_height);
    let height = (quote_height as u16 + text_height + 7 + u16::from(quote_height > 0)).min(area.height);
    let y = area.y + (area.height.saturating_sub(height)) / 3;
    let rect = Rect { x, y, width, height };

    // a modal: the screen behind it fades
    theme::dim(frame.buffer_mut(), area);
    frame.render_widget(Clear, rect);

    comment_frame(frame, title, target, width, rect);
    let inner = Rect {
        x: rect.x + 2,
        y: rect.y + 1,
        width: width.saturating_sub(4),
        height: height.saturating_sub(2),
    };
    let mut row_y = comment_quote(frame, quote, scroll, quote_height, inner, modal_draw);

    // the severity row: the 4 levels, the picked one a pill (accent: the row has the keys)
    comment_severity(frame, target, level, on_level, inner, &mut row_y);
    let text_rect = Rect {
        y: row_y,
        height: text_height.min(inner.bottom().saturating_sub(row_y + 1)),
        ..inner
    };

    text.render(frame, text_rect, !*on_level);
    modal_draw.mention.render(frame, text, modal_draw.agents, area);
    comment_buttons(frame, target, inner, modal_draw);
}

fn comment_quote(
    frame: &mut Frame<'_>,
    quote: &mut [QLine],
    scroll: &mut usize,
    quote_height: usize,
    inner: Rect,
    modal_draw: &mut ModalDraw<'_>,
) -> u16 {
    let bg = code_bg();
    let mut row_y = inner.y;

    // 1 column for the scrollbar when the lines scroll
    let bar = quote.len() > quote_height;
    let width = inner.width.saturating_sub(u16::from(bar));

    *modal_draw.quote_rect = Rect {
        y: row_y,
        height: quote_height as u16,
        ..inner
    };

    for line in quote.iter().skip(*scroll).take(quote_height) {
        let row = Rect {
            y: row_y,
            height: 1,
            width,
            ..inner
        };

        quote_line(frame, bg, width, line, row);

        row_y += 1;
    }

    if bar {
        // the thumb: the visible part of the lines
        let total = quote.len();
        let thumb = (quote_height * quote_height).div_ceil(total).max(1);
        let thumb_at = (*scroll * quote_height) / total;

        draw_quote_scrollbar(frame, quote_height, inner, width, thumb, thumb_at);
    }

    if quote_height > 0 {
        row_y += 1;
    }

    row_y
}

fn draw_quote_scrollbar(
    frame: &mut Frame<'_>,
    quote_height: usize,
    inner: Rect,
    width: u16,
    thumb: usize,
    thumb_at: usize,
) {
    for row in 0..quote_height {
        let on = row >= thumb_at && row < thumb_at + thumb;
        let cell = Span::styled(
            if on { "┃" } else { "│" },
            if on { theme::accent() } else { theme::line() },
        );

        frame.render_widget(
            Paragraph::new(cell),
            Rect {
                x: inner.x + width,
                y: inner.y + row as u16,
                width: 1,
                height: 1,
            },
        );
    }
}

fn comment_buttons(frame: &mut Frame<'_>, target: &mut Target, inner: Rect, modal_draw: &mut ModalDraw<'_>) {
    let hint = Rect {
        y: inner.bottom().saturating_sub(1),
        height: 1,
        ..inner
    };
    let (save, cancel) = (" save ", " cancel ");

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(save, theme::pill()),
            Span::raw(" "),
            Span::styled(cancel, theme::muted()),
            Span::styled(
                if matches!(target, Target::Reply(_)) {
                    "  ctrl+⏎ save · esc cancel"
                } else {
                    "  ctrl+⏎ save · esc cancel · 0 text: delete"
                },
                theme::muted(),
            ),
        ])),
        hint,
    );
    modal_draw.buttons.push((
        Rect {
            width: save.len() as u16,
            ..hint
        },
        true,
    ));
    modal_draw.buttons.push((
        Rect {
            x: hint.x + save.len() as u16 + 1,
            width: cancel.len() as u16,
            ..hint
        },
        false,
    ));
}

fn confirm(frame: &mut Frame, area: Rect, modal: &mut Modal, modal_draw: &mut ModalDraw<'_>) {
    let Modal::Confirm { question, yes, .. } = modal else {
        return;
    };

    let width = area.width.saturating_sub(4).clamp(20, 110);

    // the question's 1st line, then its list (commits: 1 per line), muted
    let lines: Vec<&str> = question.lines().collect();
    let line_count = lines.len().max(1) as u16;
    let width = width.min(70);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let height = (line_count + 4).min(area.height);
    let rect = Rect {
        x,
        y: area.y + (area.height / 3).min(area.height.saturating_sub(height)),
        width,
        height,
    };

    // a modal: the screen behind it fades
    theme::dim(frame.buffer_mut(), area);
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Block::bordered().border_style(theme::accent()).style(theme::text()),
        rect,
    );
    let inner = Rect {
        x: rect.x + 2,
        y: rect.y + 1,
        width: width.saturating_sub(4),
        height: height.saturating_sub(2),
    };

    for (i, line) in lines
        .iter()
        .enumerate()
        .take(usize::from(inner.height.saturating_sub(2)))
    {
        let style = if i == 0 { theme::text() } else { theme::muted() };

        frame.render_widget(
            Paragraph::new(Line::styled(
                crate::ui::thread_card::fit(line, usize::from(inner.width)),
                style,
            )),
            Rect {
                y: inner.y + i as u16,
                height: 1,
                ..inner
            },
        );
    }

    confirm_buttons(frame, yes, inner, modal_draw);
}

fn confirm_buttons(frame: &mut Frame<'_>, yes: &mut bool, inner: Rect, modal_draw: &mut ModalDraw<'_>) {
    let (yes_style, no_style) = if *yes {
        (theme::pill(), theme::muted())
    } else {
        (theme::muted(), theme::pill())
    };

    let row = Rect {
        y: inner.bottom().saturating_sub(1),
        height: 1,
        ..inner
    };

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" yes ", yes_style),
            Span::raw("  "),
            Span::styled(" no ", no_style),
            Span::styled("   y / n · ←→ · enter", theme::muted()),
        ])),
        row,
    );
    modal_draw.buttons.push((Rect { width: 5, ..row }, true));
    modal_draw.buttons.push((
        Rect {
            x: row.x + 7,
            width: 4,
            ..row
        },
        false,
    ));
}
