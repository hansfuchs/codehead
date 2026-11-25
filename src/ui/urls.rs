//! ctrl+click on a URL anywhere on screen: the URL under the mouse, read from the last drawn
//! frame (every view: chat, changes, merge, files, menus). A URL cut over 2+ rows (a wrap at the
//! row end) is joined from the rows above and below.

use ratatui::buffer::Buffer;
use ratatui::layout::Position;

/// Chars of a URL (RFC 3986 unreserved + reserved, `%`).
fn url_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || "-._~:/?#[]@!$&'()*+,;=%".contains(character)
}

/// The text of row Y of BUF as chars, 1 per cell (wide chars: their 2nd cell skipped).
fn row(buf: &Buffer, y: u16) -> Vec<char> {
    let agent = buf.area;

    (agent.x..agent.right())
        .map(|x| {
            let text = buf[(x, y)].symbol();
            let mut it = text.chars();

            match (it.next(), it.next()) {
                (Some(character), None) => character,
                (None, _) => ' ',
                // a grapheme of 2+ chars: 0 URL char
                _ => '\u{fffd}',
            }
        })
        .collect()
}

/// The run of URL chars in CHARS around column C: (start, end), end exclusive.
fn run(chars: &[char], character: usize) -> Option<(usize, usize)> {
    if !chars.get(character).copied().is_some_and(url_char) {
        return None;
    }

    let mut agent = character;

    while agent > 0 && url_char(chars[agent - 1]) {
        agent -= 1;
    }

    let mut bytes = character + 1;

    while bytes < chars.len() && url_char(chars[bytes]) {
        bytes += 1;
    }

    Some((agent, bytes))
}

/// The 1st run of URL chars of CHARS (a wrapped row's start: after its margin).
fn first_run(chars: &[char]) -> Option<(usize, usize)> {
    let agent = chars
        .iter()
        .position(|character| url_char(*character) && !matches!(character, '│' | '┆'))?;

    run(chars, agent)
}

/// The last run of CHARS (a row that goes on below).
fn last_run(chars: &[char]) -> Option<(usize, usize)> {
    let bytes = chars.iter().rposition(|character| url_char(*character))?;

    run(chars, bytes)
}

/// The run touches the row's text end (0 non-blank cell after it but a frame / scroll bar).
fn at_end(chars: &[char], bytes: usize) -> bool {
    chars[bytes..]
        .iter()
        .all(|character| character.is_whitespace() || matches!(character, '│' | '┃' | '▐' | '█' | '░' | '┆'))
}

/// The URL at AT in BUF: a run of URL chars with `http://` / `https://` in it, its parts on the
/// rows above / below joined when the run is cut at a row end. Trailing `.,;:)'` drop.
pub(crate) fn at(buf: &Buffer, at: Position) -> Option<String> {
    let agent = buf.area;

    if !agent.contains(at) {
        return None;
    }

    let y = at.y;
    let chars = row(buf, y);
    let (text_value, error) = run(&chars, (at.x - agent.x) as usize)?;
    let mut text: String = chars[text_value..error].iter().collect();

    // the rows above: while the run starts its row and the row above ends in a run
    let mut yy = y;
    let mut start_of_row = first_run(&chars).is_some_and(|result| result.0 == text_value);

    while start_of_row && yy > agent.y && !text.contains("://") {
        let up = row(buf, yy - 1);
        let Some((ps, pe)) = last_run(&up) else { break };

        if !at_end(&up, pe) {
            break;
        }

        let piece: String = up[ps..pe].iter().collect();

        text = piece + &text;
        yy -= 1;
        start_of_row = first_run(&up).is_some_and(|result_value| result_value.0 == ps);
    }

    // the rows below: while the run ends its row
    let mut yy = y;
    let mut end = at_end(&chars, error);

    while end && yy + 1 < agent.bottom() {
        let down = row(buf, yy + 1);
        let Some((ns, ne)) = first_run(&down) else { break };
        let piece: String = down[ns..ne].iter().collect();

        // a URL-like piece (or a cut right after a URL separator) goes on the URL; a new URL or
        // a plain word (the next sentence) does not
        let urlish = piece.contains(['/', '?', '=', '&', '#', '%', '_', '~'])
            || text.ends_with(['/', '-', '_', '.', '=', '&', '?', '#']);

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
    let url = if url.contains('(') {
        url.to_string()
    } else {
        url.trim_end_matches(')').to_string()
    };

    (url.len() > "https://".len()).then_some(url)
}

#[cfg(test)]
#[path = "urls_tests.rs"]
mod tests;
