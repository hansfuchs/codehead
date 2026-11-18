//! Fuzzy filter: each word of the query matches a text as a substring (best) or as a
//! subsequence; a score ranks the matches, char positions mark them in the drawn text.

use ratatui::style::Style;
use ratatui::text::Span;

/// Score of 1 query word in TEXT (higher: better) and the matched char positions. None: 0 match.
pub fn word(q: &str, text: &str) -> Option<(i64, Vec<usize>)> {
    if q.is_empty() {
        return Some((0, Vec::new()));
    }
    let t: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let q: Vec<char> = q.chars().flat_map(char::to_lowercase).collect();
    if t.len() != text.chars().count() {
        // a char that lowercases to 2+ chars: positions would shift; plain compare
        return plain(&q, text);
    }
    let start = |i: usize| i == 0 || !t[i - 1].is_alphanumeric() || (text.chars().nth(i).is_some_and(char::is_uppercase) && text.chars().nth(i - 1).is_some_and(char::is_lowercase));
    // substring: the earliest at a word start, else the earliest
    let subs: Vec<usize> = (0..=t.len().saturating_sub(q.len())).filter(|&i| t.len() >= q.len() && t[i..i + q.len()] == q[..]).collect();
    if let Some(&i) = subs.iter().find(|&&i| start(i)).or(subs.first()) {
        let mut s = 1000 + 10 * q.len() as i64;
        if start(i) {
            s += 300;
        }
        if i == 0 {
            s += 200;
        }
        if q.len() == t.len() {
            s += 500;
        }
        s -= (i as i64).min(100);
        return Some((s, (i..i + q.len()).collect()));
    }
    // subsequence: greedy, word starts first where they fit
    let mut pos = Vec::with_capacity(q.len());
    let mut at = 0;
    for &c in &q {
        let next = (at..t.len()).find(|&i| t[i] == c)?;
        pos.push(next);
        at = next + 1;
    }
    let mut s = 100;
    for w in pos.windows(2) {
        if w[1] == w[0] + 1 {
            s += 15;
        } else {
            s -= (w[1] - w[0]).min(20) as i64;
        }
    }
    s += pos.iter().filter(|&&i| start(i)).count() as i64 * 20;
    s -= (pos[0] as i64).min(50);
    Some((s, pos))
}

fn plain(q: &[char], text: &str) -> Option<(i64, Vec<usize>)> {
    let q: String = q.iter().collect();
    text.to_lowercase().contains(&q).then(|| (500, Vec::new()))
}

/// Every word of QUERY must match NAME or BODY. Name hits weigh 4x. Returns the score and the
/// matched char positions in NAME. None: a word matches neither.
pub fn score(query: &str, name: &str, body: &str) -> Option<(i64, Vec<usize>)> {
    let mut total = 0;
    let mut marks = Vec::new();
    for w in query.split_whitespace() {
        match (word(w, name), body_hit(w, body)) {
            (Some((s, p)), _) => {
                total += 4 * s;
                marks.extend(p);
            }
            (None, Some(s)) => total += s,
            (None, None) => return None,
        }
    }
    marks.sort_unstable();
    marks.dedup();
    Some((total, marks))
}

/// A body (file content) matches as a substring only: fuzzy over 10k chars matches anything.
/// More hits: more weight, up to 10.
fn body_hit(w: &str, body: &str) -> Option<i64> {
    if body.is_empty() || w.is_empty() {
        return None;
    }
    let lw = w.to_lowercase();
    let n = body.to_lowercase().matches(&lw).take(10).count();
    (n > 0).then(|| 50 + 10 * n as i64)
}

/// The indices of ITEMS that match QUERY, best first (stable for equal scores), with name marks.
/// An empty query: every index, in order.
pub fn rank<T>(items: &[T], query: &str, name: impl Fn(&T) -> String, body: impl Fn(&T) -> String) -> Vec<(usize, Vec<usize>)> {
    if query.trim().is_empty() {
        return (0..items.len()).map(|i| (i, Vec::new())).collect();
    }
    let mut v: Vec<(i64, usize, Vec<usize>)> = items.iter().enumerate().filter_map(|(i, it)| score(query, &name(it), &body(it)).map(|(s, p)| (s, i, p))).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    v.into_iter().map(|(_, i, p)| (i, p)).collect()
}

/// TEXT as spans: the chars at MARKS in HIT style, the rest in BASE.
pub fn spans(text: &str, marks: &[usize], base: Style, hit: Style) -> Vec<Span<'static>> {
    if marks.is_empty() {
        return vec![Span::styled(text.to_string(), base)];
    }
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut cur = String::new();
    let mut on = false;
    for (i, c) in text.chars().enumerate() {
        let m = marks.binary_search(&i).is_ok();
        if m != on && !cur.is_empty() {
            out.push(Span::styled(std::mem::take(&mut cur), if on { hit } else { base }));
        }
        on = m;
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(Span::styled(cur, if on { hit } else { base }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substring_beats_subsequence_and_word_start_beats_middle() {
        let (a, pa) = word("view", "src/view.rs").unwrap();
        let (b, _) = word("vw", "src/view.rs").unwrap();
        assert!(a > b);
        assert_eq!(pa, vec![4, 5, 6, 7]);
        let (c, _) = word("rs", "src/rs.rs").unwrap();
        let (d, _) = word("rs", "src/users.txt").unwrap();
        assert!(c > d);
        assert!(word("xyz", "src/view.rs").is_none());
    }

    #[test]
    fn every_word_must_match_and_name_hits_rank_first() {
        let files = [("src/app.rs", "fn main() { view }"), ("src/view.rs", "fn draw()"), ("README.md", "nothing")];
        let r = rank(&files, "view", |f| f.0.to_string(), |f| f.1.to_string());
        assert_eq!(r.iter().map(|x| x.0).collect::<Vec<_>>(), vec![1, 0]);
        assert_eq!(r[0].1, vec![4, 5, 6, 7]);
        assert!(rank(&files, "view nothing", |f| f.0.to_string(), |f| f.1.to_string()).is_empty());
        assert_eq!(rank(&files, " ", |f| f.0.to_string(), |f| f.1.to_string()).len(), 3);
    }

    #[test]
    fn spans_mark_the_hits() {
        let s = spans("abcd", &[1, 2], Style::default(), Style::default().bold());
        assert_eq!(s.iter().map(|s| s.content.to_string()).collect::<Vec<_>>(), vec!["a", "bc", "d"]);
    }
}
