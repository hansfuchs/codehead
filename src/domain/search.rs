//! Message search (the search tab of the menu): the owner's and the agents' messages of every
//! thread. Each query word must match a message: a substring (best, more hits weigh more), else
//! fuzzy inside 1 word of it (`dply` → `deploy`). Best first, then oldest first.

use std::sync::Mutex;
use std::{ops::ControlFlow, path::PathBuf};

use crate::engine::store::{self, Kind};

/// Max. results drawn.
pub(crate) const MAX: usize = 300;

/// A message of a thread.
#[derive(Clone, Debug, Default)]
pub(crate) struct Msg {
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
    pub(crate) fn new(thread: &str, title: &str, from: &str, ts: u64, text: &str) -> Msg {
        Msg {
            thread: thread.into(),
            title: title.into(),
            dir: None,
            from: from.into(),
            ts,
            text: text.into(),
            lower: lower(text),
        }
    }
}

fn lower(text: &str) -> String {
    text.chars()
        .map(|character| character.to_lowercase().next().unwrap_or(character))
        .collect()
}

/// The messages of every thread, oldest first.
pub(crate) fn load() -> Vec<Msg> {
    let mut out = Vec::new();

    for thread in store::all() {
        let dir = Some(thread.work_dir());
        let title = thread.title.lines().next().unwrap_or_default().to_string();

        load_thread_messages(&mut out, &thread, dir.as_ref(), &title);
    }

    out.sort_by_key(|message| message.ts);
    out
}

fn load_thread_messages(out: &mut Vec<Msg>, thread: &store::Thread, dir: Option<&PathBuf>, title: &str) {
    for event in store::events(&thread.id) {
        let (from, text) = match &event.kind {
            Kind::Prompt { from, text, .. } => (from.clone(), text.clone()),
            Kind::Text { text } => (event.agent.clone().unwrap_or_default(), text.clone()),
            _ => continue,
        };

        if let ControlFlow::Break(()) = skip_empty_text(&text) {
            continue;
        }

        let mut message = Msg::new(&thread.id, title, &from, event.ts, &text);

        message.dir = dir.cloned();
        out.push(message);
    }
}

fn skip_empty_text(text: &str) -> ControlFlow<()> {
    if text.trim().is_empty() {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

fn word_start(lower: &str, byte: usize) -> bool {
    lower[..byte]
        .chars()
        .next_back()
        .is_none_or(|character| !character.is_alphanumeric())
}

/// Score of query word W (lowercased) in M. None: 0 match.
fn word_score(word: &str, message: &Msg) -> Option<i64> {
    let hits: Vec<usize> = message.lower.match_indices(word).map(|(i, _)| i).take(10).collect();

    if let Some(&first) = hits.first() {
        let start = hits.iter().any(|&i| word_start(&message.lower, i));

        return Some(
            1000 + 10 * word.chars().count() as i64 + 15 * hits.len() as i64 + if start { 300 } else { 0 }
                - (first as i64 / 200).min(50),
        );
    }

    // fuzzy in 1 word of the message: 3+ chars, words of max. 32 chars
    if word.chars().count() < 3 {
        return None;
    }

    message
        .lower
        .split(|character: char| !character.is_alphanumeric() && character != '_' && character != '-')
        .filter(|part| !part.is_empty() && part.len() <= 32)
        .filter_map(|part| crate::domain::matcher::word(word, part).map(|(score, _)| score))
        .filter(|&score| score >= 60)
        .max()
}

/// The query words of QUERY, lowercased.
pub(crate) fn words(query: &str) -> Vec<String> {
    query.split_whitespace().map(lower).collect()
}

/// M matches every word of WORDS (from [`words`]): a substring, else fuzzy inside 1 word.
pub(crate) fn hit(message: &Msg, words: &[String]) -> bool {
    !words.is_empty() && words.iter().all(|word| word_score(word, message).is_some())
}

/// Indexes of MSGS that match QUERY: best first, then oldest first. Max. MAX.
pub(crate) fn find(msgs: &[Msg], query: &str) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(lower).collect();

    if words.is_empty() {
        return Vec::new();
    }

    let mut scored: Vec<(i64, u64, usize)> = msgs
        .iter()
        .enumerate()
        .filter_map(|(i, message)| {
            let mut total = 0;

            for word in &words {
                total += word_score(word, message)?;
            }

            Some((total, message.ts, i))
        })
        .collect();

    scored.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
    scored.truncate(MAX);

    scored.into_iter().map(|(_, _, i)| i).collect()
}

/// 1 line of M around its 1st match of QUERY, max. WIDTH chars (whitespace runs: 1 space), and
/// the char positions of the matches in it.
pub(crate) fn snippet(message: &Msg, query: &str, width: usize) -> (String, Vec<usize>) {
    let flat: String = message.text.split_whitespace().collect::<Vec<_>>().join(" ");
    let low = lower(&flat);
    let words: Vec<String> = query.split_whitespace().map(lower).collect();

    // the 1st substring hit of any word: the snippet's focus
    let focus = words
        .iter()
        .filter_map(|word| low.find(word.as_str()))
        .min()
        .map_or(0, |offset| low[..offset].chars().count());
    let chars = flat.chars().count();
    let width = width.max(8);

    let start = if chars <= width {
        0
    } else {
        focus.saturating_sub(width / 3).min(chars - width)
    };

    let lead = start > 0;
    let take = width - usize::from(lead) - usize::from(start + width < chars);
    let body: String = flat.chars().skip(start).take(take).collect();
    let text = format!(
        "{}{body}{}",
        if lead { "…" } else { "" },
        if start + take < chars { "…" } else { "" }
    );
    let tl = lower(&text);
    let mut marks: Vec<usize> = Vec::new();

    for word in &words {
        let sub: Vec<usize> = tl
            .match_indices(word.as_str())
            .flat_map(|(offset, _)| {
                let col = tl[..offset].chars().count();

                col..col + word.chars().count()
            })
            .collect();

        if !sub.is_empty() {
            marks.extend(sub);
            continue;
        }

        // fuzzy: the marks of the best word
        let mut at = 0;
        let mut best: Option<(i64, Vec<usize>)> = None;

        for token in tl.split(' ') {
            mark_snippet_match(word, at, &mut best, token);

            at += token.chars().count() + 1;
        }

        marks.extend(best.map(|found| found.1).unwrap_or_default());
    }

    marks.sort_unstable();
    marks.dedup();

    (text, marks)
}

fn mark_snippet_match(word: &str, at: usize, best: &mut Option<(i64, Vec<usize>)>, token: &str) {
    if let Some((score, positions)) = crate::domain::matcher::word(word, token)
        && best.as_ref().is_none_or(|current| score > current.0)
    {
        *best = Some((score, positions.into_iter().map(|position| position + at).collect()));
    }
}

/// A jump the next view of a thread does: (thread, message time).
static JUMP: Mutex<Option<(String, u64)>> = Mutex::new(None);

/// The next view of THREAD scrolls to its message of time TS.
pub(crate) fn jump_to(thread: &str, ts: u64) {
    if let Ok(mut jump) = JUMP.lock() {
        *jump = Some((thread.into(), ts));
    }
}

/// The jump of THREAD, once.
pub(crate) fn take_jump(thread: &str) -> Option<u64> {
    let mut jump = JUMP.lock().ok()?;

    if jump.as_ref().is_some_and(|(jumped, _)| jumped == thread) {
        jump.take().map(|(_, ts)| ts)
    } else {
        None
    }
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
