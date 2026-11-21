//! ctrl+click on a URL anywhere on screen: the URL under the mouse, read from the last drawn
//! frame (every view: chat, changes, merge, files, menus). A URL cut over 2+ rows (a wrap at the
//! row end) is joined from the rows above and below.

use ratatui::buffer::Buffer;
use ratatui::layout::Position;

/// Chars of a URL (RFC 3986 unreserved + reserved, `%`).
fn url_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-._~:/?#[]@!$&'()*+,;=%".contains(c)
}

/// The text of row Y of BUF as chars, 1 per cell (wide chars: their 2nd cell skipped).
fn row(buf: &Buffer, y: u16) -> Vec<char> {
    let a = buf.area;
    (a.x..a.right())
        .map(|x| {
            let s = buf[(x, y)].symbol();
            let mut it = s.chars();
            match (it.next(), it.next()) {
                (Some(c), None) => c,
                (None, _) => ' ',
                // a grapheme of 2+ chars: 0 URL char
                _ => '\u{fffd}',
            }
        })
        .collect()
}

/// The run of URL chars in CHARS around column C: (start, end), end exclusive.
fn run(chars: &[char], c: usize) -> Option<(usize, usize)> {
    if !chars.get(c).copied().is_some_and(url_char) {
        return None;
    }
    let mut a = c;
    while a > 0 && url_char(chars[a - 1]) {
        a -= 1;
    }
    let mut b = c + 1;
    while b < chars.len() && url_char(chars[b]) {
        b += 1;
    }
    Some((a, b))
}

/// The 1st run of URL chars of CHARS (a wrapped row's start: after its margin).
fn first_run(chars: &[char]) -> Option<(usize, usize)> {
    let a = chars.iter().position(|c| url_char(*c) && !matches!(c, '│' | '┆'))?;
    run(chars, a)
}

/// The last run of CHARS (a row that goes on below).
fn last_run(chars: &[char]) -> Option<(usize, usize)> {
    let b = chars.iter().rposition(|c| url_char(*c))?;
    run(chars, b)
}

/// The run touches the row's text end (0 non-blank cell after it but a frame / scroll bar).
fn at_end(chars: &[char], b: usize) -> bool {
    chars[b..].iter().all(|c| c.is_whitespace() || matches!(c, '│' | '┃' | '▐' | '█' | '░' | '┆'))
}

/// The URL at AT in BUF: a run of URL chars with `http://` / `https://` in it, its parts on the
/// rows above / below joined when the run is cut at a row end. Trailing `.,;:)'` drop.
pub fn at(buf: &Buffer, at: Position) -> Option<String> {
    let a = buf.area;
    if !a.contains(at) {
        return None;
    }
    let y = at.y;
    let chars = row(buf, y);
    let (s, e) = run(&chars, (at.x - a.x) as usize)?;
    let mut text: String = chars[s..e].iter().collect();
    // the rows above: while the run starts its row and the row above ends in a run
    let mut yy = y;
    let mut start_of_row = first_run(&chars).is_some_and(|r| r.0 == s);
    while start_of_row && yy > a.y && !text.contains("://") {
        let up = row(buf, yy - 1);
        let Some((ps, pe)) = last_run(&up) else { break };
        if !at_end(&up, pe) {
            break;
        }
        let piece: String = up[ps..pe].iter().collect();
        text = piece + &text;
        yy -= 1;
        start_of_row = first_run(&up).is_some_and(|r| r.0 == ps);
    }
    // the rows below: while the run ends its row
    let mut yy = y;
    let mut end = at_end(&chars, e);
    while end && yy + 1 < a.bottom() {
        let down = row(buf, yy + 1);
        let Some((ns, ne)) = first_run(&down) else { break };
        let piece: String = down[ns..ne].iter().collect();
        // a URL-like piece (or a cut right after a URL separator) goes on the URL; a new URL or
        // a plain word (the next sentence) does not
        let urlish = piece.contains(['/', '?', '=', '&', '#', '%', '_', '~']) || text.ends_with(['/', '-', '_', '.', '=', '&', '?', '#']);
        if piece.contains("://") || !urlish {
            break;
        }
        text.push_str(&piece);
        yy += 1;
        end = at_end(&down, ne);
    }
    let i = text.find("https://").or_else(|| text.find("http://"))?;
    let url = text[i..].trim_end_matches(['.', ',', ';', ':', ')', '\'', ']', '!', '?']);
    // a ( before the URL: a ) in it stays only when the URL has its own (
    let url = if url.contains('(') { url.to_string() } else { url.trim_end_matches(')').to_string() };
    (url.len() > "https://".len()).then_some(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    fn buf(lines: &[&str]) -> Buffer {
        let w = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as u16;
        let mut b = Buffer::empty(Rect::new(0, 0, w, lines.len() as u16));
        for (y, l) in lines.iter().enumerate() {
            b.set_string(0, y as u16, l, ratatui::style::Style::default());
        }
        b
    }

    #[test]
    fn a_url_in_text_in_parens_and_cut_over_rows() {
        let b = buf(&["see (https://x.dev/a_b?c=1). ok", "plain words here"]);
        assert_eq!(at(&b, Position { x: 10, y: 0 }).as_deref(), Some("https://x.dev/a_b?c=1"));
        assert_eq!(at(&b, Position { x: 2, y: 0 }), None);
        assert_eq!(at(&b, Position { x: 2, y: 1 }), None);
        // a URL cut at the row end, the rest on the next row after a margin
        let b = buf(&["│ go to https://github.com/owner/re", "│ po/pull/12 now"]);
        assert_eq!(at(&b, Position { x: 12, y: 0 }).as_deref(), Some("https://github.com/owner/repo/pull/12"));
        assert_eq!(at(&b, Position { x: 4, y: 1 }).as_deref(), Some("https://github.com/owner/repo/pull/12"));
        // the next row: a new sentence, 0 join
        let b = buf(&["read https://x.dev", "and more"]);
        assert_eq!(at(&b, Position { x: 8, y: 0 }).as_deref(), Some("https://x.dev"));
    }
}
