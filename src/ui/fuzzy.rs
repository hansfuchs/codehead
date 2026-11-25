//! Fuzzy filter: each word of the query matches a text as a substring (best) or as a
//! subsequence; a score ranks the matches, char positions mark them in the drawn text.

use ratatui::style::Style;
use ratatui::text::Span;

/// TEXT as spans: the chars at MARKS in HIT style, the rest in BASE.
pub(crate) fn spans(text: &str, marks: &[usize], base: Style, hit: Style) -> Vec<Span<'static>> {
    if marks.is_empty() {
        return vec![Span::styled(text.to_string(), base)];
    }

    let mut out: Vec<Span<'static>> = Vec::new();
    let mut cur = String::new();
    let mut on = false;

    for (i, character) in text.chars().enumerate() {
        spans_message(
            &mut out,
            &mut cur,
            &mut on,
            i,
            character,
            SpanStyles { marks, base, hit },
        );
    }

    if !cur.is_empty() {
        out.push(Span::styled(cur, if on { hit } else { base }));
    }

    out
}

fn spans_message(
    out: &mut Vec<Span<'static>>,
    cur: &mut String,
    on: &mut bool,
    i: usize,
    character: char,
    settings: SpanStyles<'_>,
) {
    let SpanStyles { marks, base, hit } = settings;

    let marked = marks.binary_search(&i).is_ok();

    if marked != *on && !cur.is_empty() {
        out.push(Span::styled(std::mem::take(cur), if *on { hit } else { base }));
    }

    *on = marked;
    cur.push(character);
}

#[cfg(test)]
#[path = "fuzzy_tests.rs"]
mod tests;

pub(crate) use crate::domain::matcher::*;

#[derive(Clone, Copy)]
struct SpanStyles<'context> {
    marks: &'context [usize],
    base: Style,
    hit: Style,
}
