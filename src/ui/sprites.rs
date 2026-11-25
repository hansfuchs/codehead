//! Agent sprites: animated 7x4 braille avatars (avatar/), 3 categories: regi (the default), geo
//! and geob. Choice per agent: its profile (`agents::Profile`), else a stable regi pick by name. New
//! agent: a random regi avatar. ROWS: the rest frame (every eye on, idle), the text an agent's
//! prompt shows; the screens draw the live frame (`frame`, `lines`).

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::domain::avatar::Status;

/// H lines of the live frame (status S) in COLOR, SIDE[i] appended to row i. Each cell dimmed
/// toward the background by its tone (regi: a line that moves by less than 1 dot fades first);
/// 1 span per run of cells in 1 tone.
pub(crate) fn lines_as(text: &Sprite, color: Color, st: Status, side: Vec<Vec<Span<'static>>>) -> Vec<Line<'static>> {
    let mut side = side.into_iter();
    let (rows, tones) = text.avatar.toned(st, crate::domain::avatar::now());
    let base = crate::ui::theme::pal().base;

    rows.into_iter()
        .enumerate()
        .map(|(y, style_value)| lines_as_spans(color, &mut side, &tones, base, y, &(style_value)))
        .collect()
}

fn lines_as_spans(
    color: Color,
    side: &mut std::vec::IntoIter<Vec<Span<'static>>>,
    tones: &[f32],
    base: Color,
    y: usize,
    style_value: &str,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let (mut run, mut tone) = (String::new(), 1.0);

    let style = |key: f32| {
        Style::default().fg(if key < 1.0 {
            crate::ui::theme::tint(color, base, key)
        } else {
            color
        })
    };

    for (x, ch) in style_value.chars().enumerate() {
        let key_value = tones.get(y * WIDTH + x).copied().unwrap_or(1.0);

        if key_value.partial_cmp(&tone) != Some(std::cmp::Ordering::Equal) && !run.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut run), style(tone)));
        }

        tone = key_value;
        run.push(ch);
    }

    spans.push(Span::styled(run, style(tone)));
    spans.extend(side.next().unwrap_or_default());

    Line::from(spans)
}

/// H lines of the live frame, idle.
pub(crate) fn lines(text: &Sprite, color: Color, side: Vec<Vec<Span<'static>>>) -> Vec<Line<'static>> {
    lines_as(text, color, Status::Idle, side)
}

#[cfg(test)]
#[path = "sprites_tests.rs"]
mod tests;

pub(crate) use crate::domain::sprites::*;

#[cfg(test)]
use crate::domain::avatar::Cat;
