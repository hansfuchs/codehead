/// Score of 1 query word in TEXT (higher: better) and the matched char positions. None: 0 match.
pub(crate) fn word(query: &str, text: &str) -> Option<(i64, Vec<usize>)> {
    if query.is_empty() {
        return Some((0, Vec::new()));
    }

    let chars: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let needle: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();

    if chars.len() != text.chars().count() {
        // a char that lowercases to 2+ chars: positions would shift; plain compare
        return plain(&needle, text);
    }

    let start = |i: usize| {
        i == 0
            || !chars[i - 1].is_alphanumeric()
            || (text.chars().nth(i).is_some_and(char::is_uppercase)
                && text.chars().nth(i - 1).is_some_and(char::is_lowercase))
    };

    // substring: the earliest at a word start, else the earliest
    let subs: Vec<usize> = (0..=chars.len().saturating_sub(needle.len()))
        .filter(|&i| chars.len() >= needle.len() && chars[i..i + needle.len()] == needle[..])
        .collect();

    if let Some(&i) = subs.iter().find(|&&i| start(i)).or(subs.first()) {
        let mut score = 1000 + 10 * needle.len() as i64;

        if start(i) {
            score += 300;
        }

        if i == 0 {
            score += 200;
        }

        if needle.len() == chars.len() {
            score += 500;
        }

        score -= (i as i64).min(100);
        return Some((score, (i..i + needle.len()).collect()));
    }

    // subsequence: greedy, word starts first where they fit
    let mut pos = Vec::with_capacity(needle.len());
    let mut at = 0;

    for &character in &needle {
        let next = (at..chars.len()).find(|&i| chars[i] == character)?;

        pos.push(next);
        at = next + 1;
    }

    let mut score = 100;

    for pair in pos.windows(2) {
        if pair[1] == pair[0] + 1 {
            score += 15;
        } else {
            score -= (pair[1] - pair[0]).min(20) as i64;
        }
    }

    score += pos.iter().filter(|&&i| start(i)).count() as i64 * 20;
    score -= (pos[0] as i64).min(50);

    Some((score, pos))
}

fn plain(query: &[char], text: &str) -> Option<(i64, Vec<usize>)> {
    let needle: String = query.iter().collect();

    text.to_lowercase().contains(&needle).then(|| (500, Vec::new()))
}

/// Every word of QUERY must match NAME or BODY. Name hits weigh 4x. Returns the score and the
/// matched char positions in NAME. None: a word matches neither.
pub(crate) fn score(query: &str, name: &str, body: &str) -> Option<(i64, Vec<usize>)> {
    let mut total = 0;
    let mut marks = Vec::new();

    for term in query.split_whitespace() {
        match (word(term, name), body_hit(term, body)) {
            (Some((points, positions)), _) => {
                total += 4 * points;
                marks.extend(positions);
            }
            (None, Some(points)) => total += points,
            (None, None) => return None,
        }
    }

    marks.sort_unstable();
    marks.dedup();

    Some((total, marks))
}

/// A body (file content) matches as a substring only: fuzzy over 10k chars matches anything.
/// More hits: more weight, up to 10.
fn body_hit(term: &str, body: &str) -> Option<i64> {
    if body.is_empty() || term.is_empty() {
        return None;
    }

    let lowered = term.to_lowercase();
    let hits = body.to_lowercase().matches(&lowered).take(10).count();

    (hits > 0).then(|| 50 + 10 * hits as i64)
}

/// The indices of ITEMS that match QUERY, best first (stable for equal scores), with name marks.
/// An empty query: every index, in order.
pub(crate) fn rank<T>(
    items: &[T],
    query: &str,
    name: impl Fn(&T) -> String,
    body: impl Fn(&T) -> String,
) -> Vec<(usize, Vec<usize>)> {
    if query.trim().is_empty() {
        return (0..items.len()).map(|i| (i, Vec::new())).collect();
    }

    let mut scored: Vec<(i64, usize, Vec<usize>)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| {
            score(query, &name(item), &body(item)).map(|(points, positions)| (points, i, positions))
        })
        .collect();

    scored.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
    scored.into_iter().map(|(_, i, positions)| (i, positions)).collect()
}
