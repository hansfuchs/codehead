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
pub(crate) fn card_h(character: &Card) -> u16 {
    CARD_H + u16::from(!character.projects.is_empty())
}

/// S cut to W display columns (… at the cut).
pub(crate) fn fit(text: &str, width_value: usize) -> String {
    use unicode_width::UnicodeWidthChar;

    if unicode_width::UnicodeWidthStr::width(text) <= width_value {
        return text.to_string();
    }

    let mut out = String::new();
    let mut used = 0;

    for character in text.chars() {
        let cw = character.width().unwrap_or(0);

        if used + cw + 1 > width_value {
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
    character: &Card,
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
    let width_value = inner.width.saturating_sub(2) as usize;

    let title_st = if character.unread || cur {
        theme::bold()
    } else {
        theme::text()
    };

    let mut status = card_status(character, tick);

    if age {
        append_age(character, width_value, &mut status);
    }

    let mut lines = vec![Line::from(Span::styled(fit(&character.name(), width_value), title_st))];

    // the projects: own line, below the title, above the status line
    if !character.projects.is_empty() {
        lines.push(Line::from(Span::styled(
            fit(&character.projects.join(" · "), width_value),
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

    draw_marks(buf, area, character);
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

fn card_status(character: &Card, tick: usize) -> Vec<Span<'static>> {
    let open = character.land.as_ref().filter(|line_value| line_value.open());

    if character.archived() {
        archived_status(character)
    } else if character.agents.blocked {
        vec![Span::styled("? blocked", theme::fg(theme::pal().attention()))]
    } else if character.agents.busy {
        let word = if character.agents.monitoring {
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
    } else if character.completion.review {
        vec![Span::styled("▷ step to review", theme::fg(theme::pal().info()))]
    } else if let Some(line_3) = open {
        vec![Span::styled(
            format!("◇ {}", line_3.badge()),
            theme::fg(theme::pal().caution()),
        )]
    } else if let Some(line_4) = character.land.as_ref().filter(|_| character.shipped()) {
        let st = if *line_4 == crate::domain::landed::Land::Landed {
            theme::fg(theme::pal().success())
        } else {
            theme::muted()
        };

        vec![Span::styled(format!("⇲ {}", line_4.badge()), st)]
    } else if character.stale(crate::engine::now_ms()) {
        stale_status(character)
    } else if character.agents.live == 0 {
        vec![Span::styled("◦ ended", theme::muted())]
    } else {
        vec![Span::styled("◦ idle", theme::muted())]
    }
}

fn append_age(character: &Card, width_value: usize, status: &mut Vec<Span<'static>>) {
    let used: usize = status.iter().map(ratatui::prelude::Span::width).sum();
    let tail = [ago(character.activity), character.branch.clone()]
        .into_iter()
        .filter(|text_value| !text_value.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");

    if !tail.is_empty() && used + 3 < width_value {
        status.push(Span::styled(
            fit(&format!(" · {tail}"), width_value - used),
            theme::muted(),
        ));
    }
}

fn draw_marks(buf: &mut ratatui::buffer::Buffer, area: ratatui::layout::Rect, character: &Card) {
    if character.unread {
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

    if character.draft && area.width >= tag.chars().count() as u16 + 8 {
        let width_2 = tag.chars().count() as u16;
        let at = ratatui::layout::Rect {
            x: area.right() - width_2 - 3,
            y: area.bottom() - 1,
            width: width_2,
            height: 1,
        };

        Paragraph::new(Span::styled(
            tag,
            theme::fg(theme::pal().caution()).add_modifier(Modifier::BOLD),
        ))
        .render(at, buf);
    }
}

fn archived_status(character: &Card) -> Vec<Span<'static>> {
    let open = character.land.as_ref().filter(|line| line.open());
    let mut value = vec![Span::styled("▣ archived", theme::muted())];

    if let Some(line_2) = open {
        value.push(Span::styled(
            format!(" · ◇ {}", line_2.badge()),
            theme::fg(theme::pal().caution()),
        ));
    }

    value
}

fn stale_status(character: &Card) -> Vec<Span<'static>> {
    let days = crate::domain::landed::archive_in_days(character.activity, crate::engine::now_ms());

    let what = if character.land == Some(crate::domain::landed::Land::Main) {
        "on main"
    } else {
        "0 code"
    };

    vec![Span::styled(
        format!("◌ {what} · auto-archive in {days}d"),
        theme::fg(theme::pal().caution()),
    )]
}
