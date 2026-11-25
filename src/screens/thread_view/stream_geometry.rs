use super::chat;

/// The top row of a stream of TOTAL rows, BACK rows scrolled back, HEIGHT rows high, as (item, row in
/// the item). STARTS: the 1st row of each item.
pub(super) fn top_anchor(starts: &[usize], total: usize, back: usize, height: usize) -> Option<(usize, usize)> {
    let top = total.saturating_sub(back).saturating_sub(height);
    let item = starts.partition_point(|&start| start <= top).checked_sub(1)?;

    Some((item, top - starts[item]))
}

/// `back` that puts ANCHOR (item, row in the item) on the top row again: 0 at the bottom.
pub(super) fn back_for(starts: &[usize], total: usize, (item, off): (usize, usize), height: usize) -> usize {
    let Some(&start) = starts.get(item) else { return 0 };

    // a folded item: its last row
    let next = starts.get(item + 1).copied().unwrap_or(total);
    let top = (start + off).min(next.saturating_sub(1).max(start));

    total.saturating_sub(top + height)
}

/// A stream row's text without the message bar on its left.
/// The text columns of a stream row (chars, end exclusive): inside its box or right of its bar,
/// trailing spaces off. None: a border row (`┌─ … ┐`, `└─ copy ─┘`).
pub(super) fn content_cols(row: &str) -> Option<(usize, usize)> {
    let chars: Vec<char> = row.chars().collect();
    let trimmed = row.trim_start();

    if trimmed.starts_with('┌') || trimmed.starts_with('└') {
        return None;
    }

    let bar = |character: char| "│┃▌┆".contains(character);

    let Some(first) = chars.iter().position(|character| !character.is_whitespace()) else {
        return Some((0, 0));
    };

    let start = if bar(chars[first]) {
        first + 1 + usize::from(chars.get(first + 1) == Some(&' '))
    } else {
        0
    };

    let last = chars
        .iter()
        .rposition(|character| !character.is_whitespace())
        .unwrap_or(first);

    let mut end = if last > first && bar(chars[last]) {
        last
    } else {
        last + 1
    };

    while end > start && chars[end - 1] == ' ' {
        end -= 1;
    }

    Some((start, end.max(start)))
}

/// Column range of the word at COL of TEXT.
pub(super) fn word_at(text: &str, col: u16) -> (u16, u16) {
    let chars: Vec<char> = text.chars().collect();
    let at = usize::from(col).min(chars.len().saturating_sub(1));
    let word = |ch: char| !ch.is_whitespace() && !"│┆▌┃".contains(ch);

    if chars.get(at).is_none_or(|ch| !word(*ch)) {
        return (col, col);
    }

    let start = (0..=at).rev().take_while(|&i| word(chars[i])).last().unwrap_or(at);
    let end = (at..chars.len()).take_while(|&i| word(chars[i])).last().unwrap_or(at);

    (start as u16, end as u16)
}

/// The time of a stream item (a search hit names a message by its time).
pub(super) fn item_ts(item: &chat::Item) -> Option<u64> {
    match item {
        chat::Item::Prompt { ts, .. } | chat::Item::Text { ts, .. } => Some(*ts),
        _ => None,
    }
}
