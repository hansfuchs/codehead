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

use crate::domain::search::{self, Msg};
use crate::features::chat::{self, Item};
use crate::ui::textarea::{Outcome, TextArea};
use crate::ui::theme;

/// The last items whose texts a refresh builds again (they grow in place).
const TAIL: usize = 4;

/// What a key asks the view to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Act {
    /// Not a key of the bar: the view's.
    Pass,
    /// Used, 0 move.
    None,
    /// Show item I (index in `Thread::items`).
    Show(usize),
    /// The bar closed.
    Close,
}

pub(crate) struct ChatSearch {
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
        ChatSearch {
            open: false,
            focus: false,
            ta: TextArea::new(true, "search the chat"),
            hits: Vec::new(),
            cur: None,
            msgs: Vec::new(),
            seen: (String::new(), 0),
            keep: None,
        }
    }
}

impl ChatSearch {
    pub(crate) fn open(&mut self) {
        self.open = true;
        self.focus = true;
        self.ta.move_end();
    }

    pub(crate) fn close(&mut self) {
        *self = ChatSearch::default();
    }

    /// The stream was built again with SHIFT items in front (the log's head): the texts and
    /// matches again, the shown match stays (its index + SHIFT).
    pub(crate) fn reset(&mut self, shift: usize) {
        self.keep = self.current().map(|i| i + shift);
        self.msgs.clear();
        self.seen.1 = usize::MAX;
        self.cur = None;
    }

    /// The query words (lowercased), 0 while closed.
    pub(crate) fn words(&self) -> Vec<String> {
        if self.open {
            search::words(&self.ta.text())
        } else {
            Vec::new()
        }
    }

    /// The shown match (item index).
    pub(crate) fn current(&self) -> Option<usize> {
        self.cur.and_then(|cur| self.hits.get(cur).copied())
    }

    pub(crate) fn is_hit(&self, item: usize) -> bool {
        self.hits.binary_search(&item).is_ok()
    }

    /// The matches again when the query or the items changed. A new query: the newest match.
    /// New items only: the shown match stays. Some(item): the match to show. The last items can
    /// grow in place (a run's calls, a flow section's lines): their texts again each time.
    pub(crate) fn refresh(&mut self, thread: &chat::Thread) -> Option<usize> {
        refresh_chat_search(self, thread)
    }

    /// The next older (OLDER) or newer match, wrapping.
    fn step(&mut self, older: bool) -> Option<usize> {
        let count = self.hits.len();

        if count == 0 {
            return None;
        }

        let cur = self.cur.unwrap_or(count - 1);

        self.cur = Some(if older {
            (cur + count - 1) % count
        } else {
            (cur + 1) % count
        });
        self.current()
    }

    /// `3/12`, `0 matches`, or empty (0 query).
    pub(crate) fn label(&self) -> String {
        if self.ta.text().trim().is_empty() {
            return String::new();
        }

        match self.cur {
            Some(cur) if !self.hits.is_empty() => format!("{}/{}", cur + 1, self.hits.len()),
            _ => "0 matches".into(),
        }
    }

    pub(crate) fn key(&mut self, key: KeyEvent, thread: &chat::Thread) -> Act {
        key_chat_search(self, key, thread)
    }

    /// The bar in R (1 row): ` find ` pill, the query, the count, the keys.
    pub(crate) fn render(&mut self, frame: &mut Frame, area: Rect) {
        render_chat_search(self, frame, area);
    }
}

/// The searched text of ITEM of THREAD.
fn item_text(thread: &chat::Thread, item: &Item) -> String {
    match item {
        Item::Prompt { text, .. } | Item::Text { text, .. } | Item::Note { text, .. } | Item::Edits { text, .. } => {
            text.clone()
        }
        Item::Run { calls, .. } => calls
            .iter()
            .map(|call| chat::call_summary(&call.name, &call.input))
            .collect::<Vec<_>>()
            .join("\n"),
        Item::Shell { command, output, .. } => format!("{command}\n{output}"),
        Item::Settled { question, answer, .. } => format!("{question}\n{answer}"),
        Item::Kid { title, text, .. } => format!("{title}\n{text}"),
        Item::Sub { sub, .. } => thread
            .subs
            .get(*sub)
            .map(|sub| format!("{}\n{}", sub.description, sub.result))
            .unwrap_or_default(),
    }
}

/// LINE with the substring hits of WORDS (lowercased) marked (accent background).
pub(crate) fn mark(line: &Line<'static>, words: &[String]) -> Line<'static> {
    let text: String = line.spans.iter().map(|span| span.content.as_ref()).collect();
    let low: Vec<char> = text
        .chars()
        .map(|character| character.to_lowercase().next().unwrap_or(character))
        .collect();
    let mut on = vec![false; low.len()];

    for word in words {
        mark_word(&low, &mut on, word);
    }

    if !on.contains(&true) {
        return line.clone();
    }

    let hl = theme::pill().add_modifier(Modifier::BOLD);
    let out: Vec<Span<'static>> = Vec::new();
    let at = 0;

    finish_mark(line, &on, hl, out, at)
}

fn mark_word(low: &[char], on: &mut [bool], word: &str) {
    let word: Vec<char> = word.chars().collect();

    if word.is_empty() || word.len() > low.len() {
        return;
    }

    for i in 0..=low.len() - word.len() {
        if low[i..i + word.len()] == word[..] {
            on[i..i + word.len()].iter_mut().for_each(|marked| *marked = true);
        }
    }
}

fn finish_mark(
    line: &Line<'static>,
    on: &[bool],
    hl: ratatui::prelude::Style,
    mut out: Vec<Span<'static>>,
    mut at: usize,
) -> Line<'static> {
    for span in &line.spans {
        mark_span(on, hl, &mut out, &mut at, span);
    }

    Line {
        spans: out,
        ..line.clone()
    }
}

fn mark_span(on: &[bool], hl: ratatui::prelude::Style, out: &mut Vec<Span<'static>>, at: &mut usize, span: &Span<'_>) {
    let chars: Vec<char> = span.content.chars().collect();
    let mut buf = String::new();
    let mut cur: Option<bool> = None;

    for (j, character) in chars.iter().enumerate() {
        push_char(
            SearchRunState {
                on,
                hl,
                out,
                at,
                buf: &mut buf,
                cur: &mut cur,
            },
            span,
            j,
            *character,
        );
    }

    if !buf.is_empty() {
        out.push(Span::styled(
            buf,
            if cur == Some(true) {
                span.style.patch(hl)
            } else {
                span.style
            },
        ));
    }

    *at += chars.len();
}

fn push_char(context: SearchRunState<'_>, span: &Span<'_>, j: usize, character: char) {
    let SearchRunState {
        on,
        hl,
        out,
        at,
        buf,
        cur,
    } = context;

    let marked = on.get(*at + j).copied().unwrap_or(false);

    if cur.is_some_and(|was| was != marked) {
        out.push(Span::styled(
            std::mem::take(buf),
            if *cur == Some(true) {
                span.style.patch(hl)
            } else {
                span.style
            },
        ));
    }

    *cur = Some(marked);
    buf.push(character);
}

#[cfg(test)]
#[path = "chat_chat_search_tests.rs"]
mod tests;

fn refresh_chat_search(bar: &mut ChatSearch, thread: &chat::Thread) -> Option<usize> {
    if !bar.open {
        return None;
    }

    let query = bar.ta.text();
    let count = thread.items.len();
    let mut grown = false;
    let mut grow: Vec<usize> = (count.saturating_sub(TAIL)..count).chain(thread.open_runs()).collect();

    grow.sort_unstable();
    grow.dedup();
    let have = count.min(bar.msgs.len());

    for i in grow.into_iter().filter(|&i| i < have) {
        let text = item_text(thread, &thread.items[i]);

        if bar.msgs[i].text != text {
            bar.msgs[i] = Msg::new("", "", "", 0, &text);
            grown = true;
        }
    }

    if query == bar.seen.0 && count == bar.seen.1 && !grown {
        return None;
    }

    let new_query = query != bar.seen.0;

    // a rebuilt stream (the log's head came in): the texts again
    if bar.msgs.len() > count || bar.seen.1 > count {
        bar.msgs.clear();
    }

    while bar.msgs.len() < count {
        let i = bar.msgs.len();

        bar.msgs
            .push(Msg::new("", "", "", 0, &item_text(thread, &thread.items[i])));
    }

    let shown = bar.keep.take().or_else(|| bar.current());
    let words = search::words(&query);

    bar.hits = (0..count).filter(|&i| search::hit(&bar.msgs[i], &words)).collect();
    bar.seen = (query, count);

    bar.cur = match shown {
        Some(item) if !new_query => bar.hits.iter().position(|&hit| hit == item),
        _ => bar.hits.len().checked_sub(1),
    };

    if new_query { bar.current() } else { None }
}

fn key_chat_search(bar: &mut ChatSearch, key: KeyEvent, thread: &chat::Thread) -> Act {
    if !bar.open || !bar.focus {
        return Act::Pass;
    }

    let plain = key.modifiers.is_empty();

    match key.code {
        KeyCode::Esc => {
            bar.close();
            return Act::Close;
        }
        KeyCode::Enter | KeyCode::Up if plain => return bar.step(true).map_or(Act::None, Act::Show),
        KeyCode::Down => return bar.step(false).map_or(Act::None, Act::Show),
        _ => {}
    }

    if matches!(bar.ta.key(key), Outcome::Ignored) {
        return Act::Pass;
    }

    bar.refresh(thread).map_or(Act::None, Act::Show)
}

fn render_chat_search(bar: &mut ChatSearch, frame: &mut Frame, area: Rect) {
    let pill = " find ";
    let label = bar.label();

    let keys = if bar.focus {
        "tab ↑: older · ⇧tab ↓: newer · esc: close"
    } else {
        "ctrl+f: back to the search"
    };

    frame.render_widget(
        Paragraph::new(Span::styled(
            pill,
            if bar.focus { theme::pill() } else { theme::muted() },
        )),
        Rect {
            width: pill.len() as u16,
            ..area
        },
    );
    let right = format!(" {label}  {keys} ");
    let rw = (right.chars().count() as u16).min(area.width / 2);
    let x = area.x + pill.len() as u16 + 1;
    let qw = area.width.saturating_sub(pill.len() as u16 + 1 + rw);

    bar.ta.render(frame, Rect { x, width: qw, ..area }, bar.focus);

    let style = if label == "0 matches" {
        theme::fg(theme::pal().caution())
    } else {
        theme::muted()
    };

    frame.render_widget(
        Paragraph::new(Span::styled(right, style)).right_aligned(),
        Rect {
            x: area.right().saturating_sub(rw),
            width: rw,
            ..area
        },
    );
}

pub(super) struct SearchRunState<'render> {
    pub(super) on: &'render [bool],
    pub(super) hl: ratatui::prelude::Style,
    pub(super) out: &'render mut Vec<Span<'static>>,
    pub(super) at: &'render mut usize,
    pub(super) buf: &'render mut String,
    pub(super) cur: &'render mut Option<bool>,
}
