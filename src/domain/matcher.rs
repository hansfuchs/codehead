/// Score of 1 query word in TEXT (higher: better) and the matched char positions. None: 0 match.
pub(crate) fn word(query: &str, text: &str) -> Option<(i64, Vec<usize>)> {
    if query.is_empty() {
        return Some((0, Vec::new()));
    }

    let thread: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let query_value: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();

    if thread.len() != text.chars().count() {
        // a char that lowercases to 2+ chars: positions would shift; plain compare
        return plain(&query_value, text);
    }

    let start = |i: usize| {
        i == 0
            || !thread[i - 1].is_alphanumeric()
            || (text.chars().nth(i).is_some_and(char::is_uppercase)
                && text.chars().nth(i - 1).is_some_and(char::is_lowercase))
    };

    // substring: the earliest at a word start, else the earliest
    let subs: Vec<usize> = (0..=thread.len().saturating_sub(query_value.len()))
        .filter(|&i| thread.len() >= query_value.len() && thread[i..i + query_value.len()] == query_value[..])
        .collect();

    if let Some(&i) = subs.iter().find(|&&i| start(i)).or(subs.first()) {
        let mut text_value = 1000 + 10 * query_value.len() as i64;

        if start(i) {
            text_value += 300;
        }

        if i == 0 {
            text_value += 200;
        }

        if query_value.len() == thread.len() {
            text_value += 500;
        }

        text_value -= (i as i64).min(100);
        return Some((text_value, (i..i + query_value.len()).collect()));
    }

    // subsequence: greedy, word starts first where they fit
    let mut pos = Vec::with_capacity(query_value.len());
    let mut at = 0;

    for &character in &query_value {
        let next = (at..thread.len()).find(|&i| thread[i] == character)?;

        pos.push(next);
        at = next + 1;
    }

    let mut text_2 = 100;

    for width in pos.windows(2) {
        if width[1] == width[0] + 1 {
            text_2 += 15;
        } else {
            text_2 -= (width[1] - width[0]).min(20) as i64;
        }
    }

    text_2 += pos.iter().filter(|&&i| start(i)).count() as i64 * 20;
    text_2 -= (pos[0] as i64).min(50);

    Some((text_2, pos))
}

fn plain(query: &[char], text: &str) -> Option<(i64, Vec<usize>)> {
    let query_value: String = query.iter().collect();

    text.to_lowercase().contains(&query_value).then(|| (500, Vec::new()))
}

/// Every word of QUERY must match NAME or BODY. Name hits weigh 4x. Returns the score and the
/// matched char positions in NAME. None: a word matches neither.
pub(crate) fn score(query: &str, name: &str, body: &str) -> Option<(i64, Vec<usize>)> {
    let mut total = 0;
    let mut marks = Vec::new();

    for width in query.split_whitespace() {
        match (word(width, name), body_hit(width, body)) {
            (Some((text, path)), _) => {
                total += 4 * text;
                marks.extend(path);
            }
            (None, Some(text_value)) => total += text_value,
            (None, None) => return None,
        }
    }

    marks.sort_unstable();
    marks.dedup();

    Some((total, marks))
}

/// A body (file content) matches as a substring only: fuzzy over 10k chars matches anything.
/// More hits: more weight, up to 10.
fn body_hit(width: &str, body: &str) -> Option<i64> {
    if body.is_empty() || width.is_empty() {
        return None;
    }

    let lw = width.to_lowercase();
    let count_value = body.to_lowercase().matches(&lw).take(10).count();

    (count_value > 0).then(|| 50 + 10 * count_value as i64)
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

    let mut value: Vec<(i64, usize, Vec<usize>)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, it)| score(query, &name(it), &body(it)).map(|(text, path)| (text, i, path)))
        .collect();

    value.sort_by(|agent, bytes| bytes.0.cmp(&agent.0).then(agent.1.cmp(&bytes.1)));
    value.into_iter().map(|(_, i, path_value)| (i, path_value)).collect()
}
