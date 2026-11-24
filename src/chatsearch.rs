//! ctrl+f in the chat: a search bar above the prompt. Each query word must match a message (the
//! rule of the menu's search, `search.rs`): a substring, else fuzzy inside 1 word. Searched: the
//! owner's and the agents' messages, notes, flow lines, shell commands + output, answered asks, the tool calls
//! of a run (their 1-line summary), subagent reports. The matches go in stream order; the bar
//! starts on the newest one. tab / enter / ↑: the next older match, shift+tab / ↓: the next newer
//! one (both wrap). esc: the bar closes, the stream stays where it is.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::search::{self, Msg};
use crate::stream::{self, Item};
use crate::textarea::{Outcome, TextArea};
use crate::theme;

/// The last items whose texts a refresh builds again (they grow in place).
const TAIL: usize = 4;

/// What a key asks the view to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    /// Not a key of the bar: the view's.
    Pass,
    /// Used, 0 move.
    None,
    /// Show item I (index in `Thread::items`).
    Show(usize),
    /// The bar closed.
    Close,
}

pub struct ChatSearch {
    pub open: bool,
    /// The bar takes the keys (else the prompt does; ctrl+f gives them back).
    pub focus: bool,
    pub ta: TextArea,
    /// Items that match, in stream order.
    hits: Vec<usize>,
    /// Index in HITS of the shown match.
    cur: Option<usize>,
    /// The text of each item (the match input), built once per item.
    msgs: Vec<Msg>,
    /// The query and the item count of the last match run.
    seen: (String, usize),
    /// After a reset: the item of the shown match (the next refresh shows it again).
    keep: Option<usize>,
}

impl Default for ChatSearch {
    fn default() -> Self {
        ChatSearch { open: false, focus: false, ta: TextArea::new(true, "search the chat"), hits: Vec::new(), cur: None, msgs: Vec::new(), seen: (String::new(), 0), keep: None }
    }
}

impl ChatSearch {
    pub fn open(&mut self) {
        self.open = true;
        self.focus = true;
        self.ta.move_end();
    }

    pub fn close(&mut self) {
        *self = ChatSearch::default();
    }

    /// The stream was built again with SHIFT items in front (the log's head): the texts and
    /// matches again, the shown match stays (its index + SHIFT).
    pub fn reset(&mut self, shift: usize) {
        self.keep = self.current().map(|i| i + shift);
        self.msgs.clear();
        self.seen.1 = usize::MAX;
        self.cur = None;
    }

    /// The query words (lowercased), 0 while closed.
    pub fn words(&self) -> Vec<String> {
        if self.open { search::words(&self.ta.text()) } else { Vec::new() }
    }

    /// The shown match (item index).
    pub fn current(&self) -> Option<usize> {
        self.cur.and_then(|c| self.hits.get(c).copied())
    }

    pub fn is_hit(&self, item: usize) -> bool {
        self.hits.binary_search(&item).is_ok()
    }

    /// The matches again when the query or the items changed. A new query: the newest match.
    /// New items only: the shown match stays. Some(item): the match to show. The last items can
    /// grow in place (a run's calls, a flow section's lines): their texts again each time.
    pub fn refresh(&mut self, t: &stream::Thread) -> Option<usize> {
        if !self.open {
            return None;
        }

        let q = self.ta.text();
        let n = t.items.len();
        let mut grown = false;
        let mut grow: Vec<usize> = (n.saturating_sub(TAIL)..n).chain(t.open_runs()).collect();
        grow.sort_unstable();
        grow.dedup();
        let have = n.min(self.msgs.len());
        for i in grow.into_iter().filter(|&i| i < have) {
            let text = item_text(t, &t.items[i]);
            if self.msgs[i].text != text {
                self.msgs[i] = Msg::new("", "", "", 0, &text);
                grown = true;
            }
        }
        if q == self.seen.0 && n == self.seen.1 && !grown {
            return None;
        }

        let new_query = q != self.seen.0;
        // a rebuilt stream (the log's head came in): the texts again
        if self.msgs.len() > n || self.seen.1 > n {
            self.msgs.clear();
        }
        while self.msgs.len() < n {
            let i = self.msgs.len();
            self.msgs.push(Msg::new("", "", "", 0, &item_text(t, &t.items[i])));
        }
        let shown = self.keep.take().or_else(|| self.current());
        let words = search::words(&q);
        self.hits = (0..n).filter(|&i| search::hit(&self.msgs[i], &words)).collect();
        self.seen = (q, n);
        self.cur = match shown {
            Some(s) if !new_query => self.hits.iter().position(|&h| h == s),
            _ => self.hits.len().checked_sub(1),
        };
        if new_query { self.current() } else { None }
    }

    /// The next older (OLDER) or newer match, wrapping.
    fn step(&mut self, older: bool) -> Option<usize> {
        let n = self.hits.len();
        if n == 0 {
            return None;
        }

        let c = self.cur.unwrap_or(n - 1);
        self.cur = Some(if older { (c + n - 1) % n } else { (c + 1) % n });
        self.current()
    }

    /// `3/12`, `0 matches`, or empty (0 query).
    pub fn label(&self) -> String {
        if self.ta.text().trim().is_empty() {
            return String::new();
        }

        match self.cur {
            Some(c) if !self.hits.is_empty() => format!("{}/{}", c + 1, self.hits.len()),
            _ => "0 matches".into(),
        }
    }

    pub fn key(&mut self, k: KeyEvent, t: &stream::Thread) -> Act {
        if !self.open || !self.focus {
            return Act::Pass;
        }

        let none = k.modifiers.is_empty();
        match k.code {
            KeyCode::Esc => {
                self.close();
                return Act::Close;
            }
            KeyCode::Enter | KeyCode::Up if none => return self.step(true).map_or(Act::None, Act::Show),
            KeyCode::Down => return self.step(false).map_or(Act::None, Act::Show),
            _ => {}
        }
        if matches!(self.ta.key(k), Outcome::Ignored) {
            return Act::Pass;
        }

        self.refresh(t).map_or(Act::None, Act::Show)
    }

    /// The bar in R (1 row): ` find ` pill, the query, the count, the keys.
    pub fn render(&mut self, f: &mut Frame, r: Rect) {
        let pill = " find ";
        let label = self.label();
        let keys = if self.focus { "tab ↑: older · ⇧tab ↓: newer · esc: close" } else { "ctrl+f: back to the search" };
        f.render_widget(Paragraph::new(Span::styled(pill, if self.focus { theme::pill() } else { theme::muted() })), Rect { width: pill.len() as u16, ..r });
        let right = format!(" {label}  {keys} ");
        let rw = (right.chars().count() as u16).min(r.width / 2);
        let x = r.x + pill.len() as u16 + 1;
        let qw = r.width.saturating_sub(pill.len() as u16 + 1 + rw);
        self.ta.render(f, Rect { x, width: qw, ..r }, self.focus);
        let style = if label == "0 matches" { theme::fg(theme::pal().caution()) } else { theme::muted() };
        f.render_widget(Paragraph::new(Span::styled(right, style)).right_aligned(), Rect { x: r.right().saturating_sub(rw), width: rw, ..r });
    }
}

/// The searched text of item IT of T.
fn item_text(t: &stream::Thread, it: &Item) -> String {
    match it {
        Item::Prompt { text, .. } | Item::Text { text, .. } | Item::Note { text, .. } | Item::Edits { text, .. } => text.clone(),
        Item::Run { calls, .. } => calls.iter().map(|c| stream::call_summary(&c.name, &c.input)).collect::<Vec<_>>().join("\n"),
        Item::Shell { command, output, .. } => format!("{command}\n{output}"),
        Item::Settled { question, answer, .. } => format!("{question}\n{answer}"),
        Item::Kid { title, text, .. } => format!("{title}\n{text}"),
        Item::Sub { sub, .. } => t.subs.get(*sub).map(|s| format!("{}\n{}", s.description, s.result)).unwrap_or_default(),
    }
}

/// LINE with the substring hits of WORDS (lowercased) marked (accent background).
pub fn mark(line: &Line<'static>, words: &[String]) -> Line<'static> {
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    let low: Vec<char> = text.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect();
    let mut on = vec![false; low.len()];
    for w in words {
        let w: Vec<char> = w.chars().collect();
        if w.is_empty() || w.len() > low.len() {
            continue;
        }
        for i in 0..=low.len() - w.len() {
            if low[i..i + w.len()] == w[..] {
                on[i..i + w.len()].iter_mut().for_each(|b| *b = true);
            }
        }
    }
    if !on.contains(&true) {
        return line.clone();
    }

    let hl = theme::pill().add_modifier(Modifier::BOLD);
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut at = 0;
    for s in &line.spans {
        let chars: Vec<char> = s.content.chars().collect();
        let mut buf = String::new();
        let mut cur: Option<bool> = None;
        for (j, c) in chars.iter().enumerate() {
            let m = on.get(at + j).copied().unwrap_or(false);
            if cur.is_some_and(|x| x != m) {
                out.push(Span::styled(std::mem::take(&mut buf), if cur == Some(true) { s.style.patch(hl) } else { s.style }));
            }
            cur = Some(m);
            buf.push(*c);
        }
        if !buf.is_empty() {
            out.push(Span::styled(buf, if cur == Some(true) { s.style.patch(hl) } else { s.style }));
        }
        at += chars.len();
    }
    Line { spans: out, ..line.clone() }
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::KeyModifiers;
    use super::*;
    use crate::engine::store::{Event, Kind};

    fn thread(texts: &[&str]) -> stream::Thread {
        let mut t = stream::Thread::default();
        for (i, x) in texts.iter().enumerate() {
            t.push(&Event { ts: i as u64 + 1, agent: Some("ada".into()), kind: Kind::Text { text: x.to_string() } });
        }
        t
    }

    fn typed(s: &mut ChatSearch, t: &stream::Thread, text: &str) -> Act {
        let mut last = Act::None;
        for c in text.chars() {
            last = s.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), t);
        }
        last
    }

    #[test]
    fn matches_cycle_from_the_newest_and_wrap() {
        let t = thread(&["deploy the app", "unrelated", "the deploy failed", "redeployed it"]);
        let mut s = ChatSearch::default();
        s.open();
        assert_eq!(typed(&mut s, &t, "deploy"), Act::Show(3), "the newest match first");
        assert_eq!(s.label(), "3/3");
        let older = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(s.key(older, &t), Act::Show(2));
        assert_eq!(s.key(older, &t), Act::Show(0));
        assert_eq!(s.key(older, &t), Act::Show(3), "wraps");
        assert_eq!(s.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &t), Act::Show(0));
        assert!(s.is_hit(2) && !s.is_hit(1));
    }

    #[test]
    fn zero_matches_label_and_esc_closes() {
        let t = thread(&["hello"]);
        let mut s = ChatSearch::default();
        s.open();
        typed(&mut s, &t, "zzz");
        assert_eq!(s.label(), "0 matches");
        assert_eq!(s.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &t), Act::Close);
        assert!(!s.open && s.ta.text().is_empty());
    }

    #[test]
    fn new_items_keep_the_shown_match() {
        let mut t = thread(&["fix a", "fix b"]);
        let mut s = ChatSearch::default();
        s.open();
        typed(&mut s, &t, "fix");
        s.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &t);
        assert_eq!(s.current(), Some(0));
        t.push(&Event { ts: 9, agent: Some("ada".into()), kind: Kind::Text { text: "fix c".into() } });
        assert_eq!(s.refresh(&t), None, "a new message: 0 jump");
        assert_eq!((s.current(), s.label().as_str()), (Some(0), "1/3"));
    }

    /// A rebuilt stream (the log's head in front) and items that grow in place: the matches
    /// follow (0 stale text by index).
    #[test]
    fn a_rebuilt_stream_and_grown_items_keep_the_matches_right() {
        let t = thread(&["fix it", "other"]);
        let mut s = ChatSearch::default();
        s.open();
        typed(&mut s, &t, "fix");
        assert!(s.is_hit(0));
        // the head comes in front: the hit is item 2 now
        let t = thread(&["head 1", "head 2", "fix it", "other"]);
        s.reset(2);
        s.refresh(&t);
        assert!(s.is_hit(2) && !s.is_hit(0), "{:?}", s.hits);
        assert_eq!(s.label(), "1/1");
        assert_eq!(s.current(), Some(2), "the shown match stays");
        // an open run further back than the last items gets a call: found
        let mut t = stream::Thread::default();
        let tool = |id: &str, file: &str| Event { ts: 1, agent: Some("ada".into()), kind: Kind::Tool { id: id.into(), name: "Read".into(), input: serde_json::json!({"file_path": file}) } };
        t.push(&tool("1", "src/a.rs"));
        for i in 0..6 {
            t.push(&Event { ts: 2, agent: Some("zed".into()), kind: Kind::Text { text: format!("zed {i}") } });
        }
        let mut s = ChatSearch::default();
        s.open();
        typed(&mut s, &t, "special");
        assert_eq!(s.label(), "0 matches");
        t.push(&tool("2", "src/special.rs"));
        s.refresh(&t);
        assert!(s.is_hit(0), "{} items", t.items.len());
    }

    #[test]
    fn mark_splits_spans_at_the_hits() {
        let l = Line::from(vec![Span::raw("the De"), Span::raw("ploy ok")]);
        let m = mark(&l, &["deploy".into()]);
        let text: Vec<&str> = m.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, vec!["the ", "De", "ploy", " ok"]);
        assert!(m.spans[1].style.bg.is_some() && m.spans[0].style.bg.is_none());
    }
}
