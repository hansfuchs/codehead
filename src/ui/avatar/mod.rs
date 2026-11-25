use ratatui::text::Span;
const NOISE: [char; 10] = ['░', '▒', '▓', '/', '\\', '|', '-', '+', '*', '#'];
const BAND: f32 = 0.15;
pub(crate) use crate::domain::avatar::*;
/// Line SPANS, decoded to PROGRESS (0..1): noise resolves left to right (the lab `decode`).
/// Not yet reached: dim dots; the noise band: NOISE color; resolved: the span styles.
pub(crate) fn decode(
    spans: Vec<ratatui::text::Span<'static>>,
    progress: f32,
    noise: ratatui::style::Style,
    dim: ratatui::style::Style,
) -> Vec<ratatui::text::Span<'static>> {
    if progress >= 1.0 {
        return spans;
    }

    let len = spans
        .iter()
        .map(|span| span.content.chars().count())
        .sum::<usize>()
        .max(1) as f32;
    let cyc = progress * (1.0 + BAND);
    let frame = (now() * 30.0) as u64;
    let mut out = Vec::new();
    let mut i = 0usize;

    for span in spans {
        decode_span(
            &mut out,
            &mut i,
            &(span),
            DecodeFrame {
                noise,
                dim,
                len,
                cyc,
                frame,
            },
        );
    }

    out
}

fn decode_span(
    out: &mut Vec<ratatui::prelude::Span<'_>>,
    i: &mut usize,
    span: &ratatui::prelude::Span<'_>,
    settings: DecodeFrame,
) {
    let DecodeFrame {
        noise,
        dim,
        len,
        cyc,
        frame,
    } = settings;

    for ch in span.content.chars() {
        let at = *i as f32 / len;

        let cell = if ch == ' ' {
            Span::styled(" ", span.style)
        } else if cyc > at + BAND {
            Span::styled(ch.to_string(), span.style)
        } else if cyc > at {
            let pick = regi::hash(frame * 131 + *i as u64);

            Span::styled(
                NOISE[(pick * NOISE.len() as f32) as usize % NOISE.len()].to_string(),
                noise,
            )
        } else {
            Span::styled("·", dim)
        };

        out.push(cell);
        *i += 1;
    }
}

#[cfg(test)]
#[path = "avatar_tests.rs"]
mod tests;

#[derive(Clone, Copy)]
struct DecodeFrame {
    noise: ratatui::prelude::Style,
    dim: ratatui::prelude::Style,
    len: f32,
    cyc: f32,
    frame: u64,
}
