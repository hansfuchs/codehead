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

fn quote_line(frame: &mut Frame<'_>, bg: ratatui::prelude::Color, cw: u16, line: &QLine, row: Rect) {
    match line {
        QLine::Gap => {}
        QLine::Path(path) => frame.render_widget(
            Paragraph::new(Line::styled(
                crate::ui::thread_card::fit(path, usize::from(cw)),
                theme::bold(),
            )),
            row,
        ),
        QLine::Code(character) => {
            let st = match character.chars().next() {
                Some('+') => Style::default().fg(theme::pal().success()),
                Some('-') => Style::default().fg(theme::pal().error()),
                _ => theme::muted(),
            };

            frame.render_widget(
                Paragraph::new(Line::styled(
                    crate::ui::thread_card::fit(character, usize::from(cw)),
                    st,
                ))
                .style(Style::default().bg(bg)),
                row,
            );
        }
        QLine::Note(head, thread) => frame.render_widget(
            Paragraph::new(Line::styled(
                crate::ui::thread_card::fit(thread, usize::from(cw)),
                if *head { theme::bold() } else { theme::text() },
            )),
            row,
        ),
    }
}

fn comment_frame(frame: &mut Frame<'_>, title: &mut String, target: &mut Target, width: u16, area_value: Rect) {
    let head = match target {
        Target::New => format!(" comment on {title} "),
        Target::Edit(id) => format!(" edit comment {id} · {title} "),
        Target::EditReply(id, result) => format!(" edit reply {} on {id} · {title} ", *result + 1),
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
        area_value,
    );
}

fn comment_severity(
    frame: &mut Frame<'_>,
    target: &mut Target,
    level: &mut super::rank::Level,
    on_level: &mut bool,
    inner: Rect,
    yy: &mut u16,
) {
    let mut spans = vec![Span::styled(
        "severity ",
        if *on_level {
            theme::accent_bold()
        } else {
            theme::muted()
        },
    )];

    for line in crate::domain::changes::rank::Level::ALL {
        let st = severity_style(line, *level, *on_level);

        spans.push(Span::styled(format!(" {} ", line.label()), st));
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
            y: *yy,
            height: 1,
            ..inner
        },
    );
    *yy += 2;
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

fn comment(frame: &mut Frame, area: Rect, modal: &mut Modal, view_state: &mut ModalDraw<'_>) {
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

    let tw = width.saturating_sub(4);
    let th = (text.rows_for(tw) as u16).clamp(3, 8);

    // the selected lines: at most half the screen (and 24 rows); more: they scroll
    let room = (area.height.saturating_sub(th + 8) as usize)
        .min(usize::from(area.height / 2).max(3))
        .min(24);
    let qh = quote.len().min(room);

    *scroll = (*scroll).min(quote.len() - qh);
    let height = (qh as u16 + th + 7 + u16::from(qh > 0)).min(area.height);
    let y = area.y + (area.height.saturating_sub(height)) / 3;
    let area_value = Rect { x, y, width, height };

    // a modal: the screen behind it fades
    theme::dim(frame.buffer_mut(), area);
    frame.render_widget(Clear, area_value);

    comment_frame(frame, title, target, width, area_value);
    let inner = Rect {
        x: area_value.x + 2,
        y: area_value.y + 1,
        width: width.saturating_sub(4),
        height: height.saturating_sub(2),
    };
    let mut yy = comment_quote(frame, quote, scroll, qh, inner, view_state);

    // the severity row: the 4 levels, the picked one a pill (accent: the row has the keys)
    comment_severity(frame, target, level, on_level, inner, &mut yy);
    let tr = Rect {
        y: yy,
        height: th.min(inner.bottom().saturating_sub(yy + 1)),
        ..inner
    };

    text.render(frame, tr, !*on_level);
    view_state.mention.render(frame, text, view_state.agents, area);
    comment_buttons(frame, target, inner, view_state);
}

fn comment_quote(
    frame: &mut Frame<'_>,
    quote: &mut [QLine],
    scroll: &mut usize,
    qh: usize,
    inner: Rect,
    view_state: &mut ModalDraw<'_>,
) -> u16 {
    let bg = code_bg();
    let mut yy = inner.y;

    // 1 column for the scrollbar when the lines scroll
    let bar = quote.len() > qh;
    let cw = inner.width.saturating_sub(u16::from(bar));

    *view_state.quote_rect = Rect {
        y: yy,
        height: qh as u16,
        ..inner
    };

    for line in quote.iter().skip(*scroll).take(qh) {
        let row = Rect {
            y: yy,
            height: 1,
            width: cw,
            ..inner
        };

        quote_line(frame, bg, cw, line, row);

        yy += 1;
    }

    if bar {
        // the thumb: the visible part of the lines
        let count = quote.len();
        let len = (qh * qh).div_ceil(count).max(1);
        let at = (*scroll * qh) / count;

        draw_quote_scrollbar(frame, qh, inner, cw, len, at);
    }

    if qh > 0 {
        yy += 1;
    }

    yy
}

fn draw_quote_scrollbar(frame: &mut Frame<'_>, qh: usize, inner: Rect, cw: u16, len: usize, at: usize) {
    for key in 0..qh {
        let on = key >= at && key < at + len;
        let cell = Span::styled(
            if on { "┃" } else { "│" },
            if on { theme::accent() } else { theme::line() },
        );

        frame.render_widget(
            Paragraph::new(cell),
            Rect {
                x: inner.x + cw,
                y: inner.y + key as u16,
                width: 1,
                height: 1,
            },
        );
    }
}

fn comment_buttons(frame: &mut Frame<'_>, target: &mut Target, inner: Rect, view_state: &mut ModalDraw<'_>) {
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
    view_state.buttons.push((
        Rect {
            width: save.len() as u16,
            ..hint
        },
        true,
    ));
    view_state.buttons.push((
        Rect {
            x: hint.x + save.len() as u16 + 1,
            width: cancel.len() as u16,
            ..hint
        },
        false,
    ));
}

fn confirm(frame: &mut Frame, area: Rect, modal: &mut Modal, view_state: &mut ModalDraw<'_>) {
    let Modal::Confirm { question, yes, .. } = modal else {
        return;
    };

    let width = area.width.saturating_sub(4).clamp(20, 110);

    // the question's 1st line, then its list (commits: 1 per line), muted
    let lines: Vec<&str> = question.lines().collect();
    let count = lines.len().max(1) as u16;
    let width = width.min(70);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let height = (count + 4).min(area.height);
    let area_value = Rect {
        x,
        y: area.y + (area.height / 3).min(area.height.saturating_sub(height)),
        width,
        height,
    };

    // a modal: the screen behind it fades
    theme::dim(frame.buffer_mut(), area);
    frame.render_widget(Clear, area_value);
    frame.render_widget(
        Block::bordered().border_style(theme::accent()).style(theme::text()),
        area_value,
    );
    let inner = Rect {
        x: area_value.x + 2,
        y: area_value.y + 1,
        width: width.saturating_sub(4),
        height: height.saturating_sub(2),
    };

    for (i, line) in lines
        .iter()
        .enumerate()
        .take(usize::from(inner.height.saturating_sub(2)))
    {
        let st = if i == 0 { theme::text() } else { theme::muted() };

        frame.render_widget(
            Paragraph::new(Line::styled(
                crate::ui::thread_card::fit(line, usize::from(inner.width)),
                st,
            )),
            Rect {
                y: inner.y + i as u16,
                height: 1,
                ..inner
            },
        );
    }

    confirm_buttons(frame, yes, inner, view_state);
}

fn confirm_buttons(frame: &mut Frame<'_>, yes: &mut bool, inner: Rect, view_state: &mut ModalDraw<'_>) {
    let (ys, ns) = if *yes {
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
            Span::styled(" yes ", ys),
            Span::raw("  "),
            Span::styled(" no ", ns),
            Span::styled("   y / n · ←→ · enter", theme::muted()),
        ])),
        row,
    );
    view_state.buttons.push((Rect { width: 5, ..row }, true));
    view_state.buttons.push((
        Rect {
            x: row.x + 7,
            width: 4,
            ..row
        },
        false,
    ));
}
