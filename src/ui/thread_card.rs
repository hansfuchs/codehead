pub(crate) use crate::domain::thread_card::{Card, filter, load, mark_seen, mark_seen_now};
use crate::ui::theme;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget};
/// "3m ago"-style age of unix ms AT.
pub(crate) fn ago(at: u64) -> String {
    if at == 0 {
        return String::new();
    }

    crate::domain::notes::ago(std::time::UNIX_EPOCH + std::time::Duration::from_millis(at))
}

/// Working wave frames (as the agent header), 1 per `WAVE_MS`.
pub(crate) const WAVE: [&str; 4] = ["░▒▓", "▒▓▒", "▓▒░", "▒░▒"];
pub(crate) const WAVE_MS: u128 = 150;

/// Rows of a card: border, title, status, border.
pub(crate) const CARD_H: u16 = 4;

/// Rows of card C: `CARD_H`, +1 for its projects line.
pub(crate) fn card_h(card: &Card) -> u16 {
    CARD_H + u16::from(!card.projects.is_empty())
}

/// S cut to W display columns (… at the cut).
pub(crate) fn fit(text: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthChar;

    if unicode_width::UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }

    let mut out = String::new();
    let mut used = 0;

    for character in text.chars() {
        let cw = character.width().unwrap_or(0);

        if used + cw + 1 > width {
            break;
        }

        out.push(character);
        used += cw;
    }

    out.push('…');

    out
}

pub(crate) fn draw_card(
    buf: &mut ratatui::buffer::Buffer,
    area: ratatui::layout::Rect,
    card: &Card,
    look: Look,
    tick: usize,
) {
    let Look { cur, here, age } = look;

    if area.width < 6 || area.height < CARD_H {
        return;
    }

    let border = if cur { theme::accent_bold() } else { theme::line() };
    let mut block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(border)
        .style(theme::text());

    if here {
        block = block.title(Span::styled(
            " here ",
            if cur { theme::accent_bold() } else { theme::muted() },
        ));
    }

    let inner = block.inner(area);

    block.render(area, buf);
    let width = inner.width.saturating_sub(2) as usize;

    let title_st = if card.unread || cur {
        theme::bold()
    } else {
        theme::text()
    };

    let mut status = card_status(card, tick);

    if age {
        append_age(card, width, &mut status);
    }

    let mut lines = vec![Line::from(Span::styled(fit(&card.name(), width), title_st))];

    // the projects: own line, below the title, above the status line
    if !card.projects.is_empty() {
        lines.push(Line::from(Span::styled(
            fit(&card.projects.join(" · "), width),
            theme::muted(),
        )));
    }

    lines.push(Line::from(status));
    let body = ratatui::layout::Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };

    Paragraph::new(lines).render(body, buf);

    if cur {
        theme::accent_edge(buf, area);
    }

    draw_marks(buf, area, card);
}

#[cfg(test)]
#[path = "thread_card_tests.rs"]
mod tests;

#[cfg(test)]
use crate::domain::thread_card::has_draft;

#[derive(Clone, Copy)]
pub(crate) struct Look {
    pub cur: bool,
    pub here: bool,
    pub age: bool,
}

fn card_status(card: &Card, tick: usize) -> Vec<Span<'static>> {
    let open = card.land.as_ref().filter(|land| land.open());

    if card.archived() {
        archived_status(card)
    } else if card.agents.blocked {
        vec![Span::styled("? blocked", theme::fg(theme::pal().attention()))]
    } else if card.agents.busy {
        let word = if card.agents.monitoring {
            "monitoring…"
        } else {
            "working…"
        };

        vec![
            Span::styled(
                format!("{} ", WAVE[tick % WAVE.len()]),
                theme::fg(theme::pal().progress()),
            ),
            Span::styled(word, theme::muted()),
        ]
    } else if card.completion.review {
        vec![Span::styled("▷ step to review", theme::fg(theme::pal().info()))]
    } else if let Some(land) = open {
        vec![Span::styled(
            format!("◇ {}", land.badge()),
            theme::fg(theme::pal().caution()),
        )]
    } else if let Some(land) = card.land.as_ref().filter(|_| card.shipped()) {
        let st = if *land == crate::domain::landed::Land::Landed {
            theme::fg(theme::pal().success())
        } else {
            theme::muted()
        };

        vec![Span::styled(format!("⇲ {}", land.badge()), st)]
    } else if card.stale(crate::engine::now_ms()) {
        stale_status(card)
    } else if card.agents.live == 0 {
        vec![Span::styled("◦ ended", theme::muted())]
    } else {
        vec![Span::styled("◦ idle", theme::muted())]
    }
}

fn append_age(card: &Card, width: usize, status: &mut Vec<Span<'static>>) {
    let used: usize = status.iter().map(ratatui::prelude::Span::width).sum();
    let tail = [ago(card.activity), card.branch.clone()]
        .into_iter()
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");

    if !tail.is_empty() && used + 3 < width {
        status.push(Span::styled(fit(&format!(" · {tail}"), width - used), theme::muted()));
    }
}

fn draw_marks(buf: &mut ratatui::buffer::Buffer, area: ratatui::layout::Rect, card: &Card) {
    if card.unread {
        let dot = ratatui::layout::Rect {
            x: area.right().saturating_sub(3),
            y: area.y,
            width: 1,
            height: 1,
        };

        Paragraph::new(Span::styled(
            "●",
            theme::fg(theme::pal().info()).add_modifier(Modifier::BOLD),
        ))
        .render(dot, buf);
    }

    // an unsent message in the prompt: on the bottom border, right
    let tag = " ✎ unsent ";

    if card.draft && area.width >= tag.chars().count() as u16 + 8 {
        let width = tag.chars().count() as u16;
        let at = ratatui::layout::Rect {
            x: area.right() - width - 3,
            y: area.bottom() - 1,
            width,
            height: 1,
        };

        Paragraph::new(Span::styled(
            tag,
            theme::fg(theme::pal().caution()).add_modifier(Modifier::BOLD),
        ))
        .render(at, buf);
    }
}

fn archived_status(card: &Card) -> Vec<Span<'static>> {
    let open = card.land.as_ref().filter(|land| land.open());
    let mut spans = vec![Span::styled("▣ archived", theme::muted())];

    if let Some(land) = open {
        spans.push(Span::styled(
            format!(" · ◇ {}", land.badge()),
            theme::fg(theme::pal().caution()),
        ));
    }

    spans
}

fn stale_status(card: &Card) -> Vec<Span<'static>> {
    let days = crate::domain::landed::archive_in_days(card.activity, crate::engine::now_ms());

    let what = if card.land == Some(crate::domain::landed::Land::Main) {
        "on main"
    } else {
        "0 code"
    };

    vec![Span::styled(
        format!("◌ {what} · auto-archive in {days}d"),
        theme::fg(theme::pal().caution()),
    )]
}
