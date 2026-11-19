//! Message search (the search tab of the menu): the owner's and the agents' messages of every
//! thread. Each query word must match a message: a substring (best, more hits weigh more), else
//! fuzzy inside 1 word of it (`dply` → `deploy`). Best first, then oldest first.

use std::path::PathBuf;
use std::sync::Mutex;

use crate::engine::store::{self, Kind};

/// Max. results drawn.
pub const MAX: usize = 300;

/// A message of a thread.
#[derive(Clone, Debug, Default)]
pub struct Msg {
    pub thread: String,
    pub title: String,
    pub dir: Option<PathBuf>,
    /// `owner` or an agent.
    pub from: String,
    pub ts: u64,
    pub text: String,
    /// TEXT, each char lowercased to 1 char (char indexes stay TEXT's).
    lower: String,
}

impl Msg {
    pub fn new(thread: &str, title: &str, from: &str, ts: u64, text: &str) -> Msg {
        Msg { thread: thread.into(), title: title.into(), dir: None, from: from.into(), ts, text: text.into(), lower: lower(text) }
    }
}

fn lower(s: &str) -> String {
    s.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect()
}

/// The messages of every thread, oldest first.
pub fn load() -> Vec<Msg> {
    let mut out = Vec::new();
    for t in store::all() {
        let dir = Some(t.work_dir());
        let title = t.title.lines().next().unwrap_or_default().to_string();
        for e in store::events(&t.id) {
            let (from, text) = match &e.kind {
                Kind::Prompt { from, text, .. } => (from.clone(), text.clone()),
                Kind::Text { text } => (e.agent.clone().unwrap_or_default(), text.clone()),
                _ => continue,
            };
            if text.trim().is_empty() {
                continue;
            }
            let mut m = Msg::new(&t.id, &title, &from, e.ts, &text);
            m.dir = dir.clone();
            out.push(m);
        }
    }
    out.sort_by_key(|m| m.ts);
    out
}

fn word_start(lower: &str, byte: usize) -> bool {
    lower[..byte].chars().next_back().is_none_or(|c| !c.is_alphanumeric())
}

/// Score of query word W (lowercased) in M. None: 0 match.
fn word_score(w: &str, m: &Msg) -> Option<i64> {
    let hits: Vec<usize> = m.lower.match_indices(w).map(|(i, _)| i).take(10).collect();
    if let Some(&first) = hits.first() {
        let start = hits.iter().any(|&i| word_start(&m.lower, i));
        return Some(1000 + 10 * w.chars().count() as i64 + 15 * hits.len() as i64 + if start { 300 } else { 0 } - (first as i64 / 200).min(50));
    }
    // fuzzy in 1 word of the message: 3+ chars, words of max. 32 chars
    if w.chars().count() < 3 {
        return None;
    }
    m.lower.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-').filter(|t| !t.is_empty() && t.len() <= 32).filter_map(|t| crate::fuzzy::word(w, t).map(|(s, _)| s)).filter(|&s| s >= 60).max()
}

/// Indexes of MSGS that match QUERY: best first, then oldest first. Max. MAX.
pub fn find(msgs: &[Msg], query: &str) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(lower).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut v: Vec<(i64, u64, usize)> = msgs
        .iter()
        .enumerate()
        .filter_map(|(i, m)| {
            let mut total = 0;
            for w in &words {
                total += word_score(w, m)?;
            }
            Some((total, m.ts, i))
        })
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    v.truncate(MAX);
    v.into_iter().map(|(_, _, i)| i).collect()
}

/// 1 line of M around its 1st match of QUERY, max. WIDTH chars (whitespace runs: 1 space), and
/// the char positions of the matches in it.
pub fn snippet(m: &Msg, query: &str, width: usize) -> (String, Vec<usize>) {
    let flat: String = m.text.split_whitespace().collect::<Vec<_>>().join(" ");
    let low = lower(&flat);
    let words: Vec<String> = query.split_whitespace().map(lower).collect();
    // the 1st substring hit of any word: the snippet's focus
    let focus = words.iter().filter_map(|w| low.find(w.as_str())).min().map_or(0, |b| low[..b].chars().count());
    let n = flat.chars().count();
    let width = width.max(8);
    let start = if n <= width { 0 } else { focus.saturating_sub(width / 3).min(n - width) };
    let lead = start > 0;
    let take = width - usize::from(lead) - usize::from(start + width < n);
    let body: String = flat.chars().skip(start).take(take).collect();
    let text = format!("{}{body}{}", if lead { "…" } else { "" }, if start + take < n { "…" } else { "" });
    let tl = lower(&text);
    let mut marks: Vec<usize> = Vec::new();
    for w in &words {
        let sub: Vec<usize> = tl.match_indices(w.as_str()).flat_map(|(b, _)| {
            let c = tl[..b].chars().count();
            c..c + w.chars().count()
        }).collect();
        if !sub.is_empty() {
            marks.extend(sub);
            continue;
        }
        // fuzzy: the marks of the best word
        let mut at = 0;
        let mut best: Option<(i64, Vec<usize>)> = None;
        for t in tl.split(' ') {
            if let Some((s, p)) = crate::fuzzy::word(w, t)
                && best.as_ref().is_none_or(|b| s > b.0)
            {
                best = Some((s, p.into_iter().map(|x| x + at).collect()));
            }
            at += t.chars().count() + 1;
        }
        marks.extend(best.map(|b| b.1).unwrap_or_default());
    }
    marks.sort_unstable();
    marks.dedup();
    (text, marks)
}

/// A jump the next view of a thread does: (thread, message time).
static JUMP: Mutex<Option<(String, u64)>> = Mutex::new(None);

/// The next view of THREAD scrolls to its message of time TS.
pub fn jump_to(thread: &str, ts: u64) {
    if let Ok(mut j) = JUMP.lock() {
        *j = Some((thread.into(), ts));
    }
}

/// The jump of THREAD, once.
pub fn take_jump(thread: &str) -> Option<u64> {
    let mut j = JUMP.lock().ok()?;
    if j.as_ref().is_some_and(|(t, _)| t == thread) { j.take().map(|(_, ts)| ts) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msgs() -> Vec<Msg> {
        vec![
            Msg::new("t1", "deploy", "owner", 1, "fix the deploy script please"),
            Msg::new("t1", "deploy", "ada", 2, "The deploy script runs again. Redeployed twice."),
            Msg::new("t2", "docs", "owner", 3, "update the docs for the deploy"),
            Msg::new("t2", "docs", "bob", 4, "nothing here"),
        ]
    }

    #[test]
    fn every_word_must_match_best_first_then_oldest() {
        let m = msgs();
        // "deploy" at a word start in 3 messages; the 2nd has 2 hits
        assert_eq!(find(&m, "deploy"), vec![1, 0, 2]);
        assert_eq!(find(&m, "deploy docs"), vec![2]);
        // fuzzy inside 1 word
        assert_eq!(find(&m, "scrpt"), vec![0, 1]);
        assert!(find(&m, "zebra").is_empty() && find(&m, "  ").is_empty());
        // case: any
        assert_eq!(find(&m, "DEPLOY SCRIPT"), vec![1, 0]);
    }

    #[test]
    fn the_snippet_centers_the_match_and_marks_it() {
        let long = format!("{} needle {}", "hay ".repeat(40), "hay ".repeat(40));
        let m = Msg::new("t", "x", "owner", 1, &long);
        let (s, marks) = snippet(&m, "needle", 30);
        assert_eq!(s.chars().count(), 30, "{s}");
        assert!(s.starts_with('…') && s.ends_with('…') && s.contains("needle"), "{s}");
        let at = s.chars().collect::<Vec<_>>();
        assert_eq!(marks.iter().map(|&i| at[i]).collect::<String>(), "needle");
        // fuzzy marks
        let (s, marks) = snippet(&Msg::new("t", "x", "owner", 1, "run the script"), "scrpt", 40);
        let at = s.chars().collect::<Vec<_>>();
        assert_eq!(marks.iter().map(|&i| at[i]).collect::<String>(), "scrpt");
    }

    #[test]
    fn a_jump_is_for_its_thread_once() {
        jump_to("tj", 42);
        assert_eq!(take_jump("other"), None);
        assert_eq!(take_jump("tj"), Some(42));
        assert_eq!(take_jump("tj"), None);
    }
}
