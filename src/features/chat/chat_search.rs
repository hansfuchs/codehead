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
        self.cur.and_then(|character| self.hits.get(character).copied())
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

        let character = self.cur.unwrap_or(count - 1);

        self.cur = Some(if older {
            (character + count - 1) % count
        } else {
            (character + 1) % count
        });
        self.current()
    }

    /// `3/12`, `0 matches`, or empty (0 query).
    pub(crate) fn label(&self) -> String {
        if self.ta.text().trim().is_empty() {
            return String::new();
        }

        match self.cur {
            Some(character) if !self.hits.is_empty() => format!("{}/{}", character + 1, self.hits.len()),
            _ => "0 matches".into(),
        }
    }

    pub(crate) fn key(&mut self, key_value: KeyEvent, thread: &chat::Thread) -> Act {
        key_chat_search(self, key_value, thread)
    }

    /// The bar in R (1 row): ` find ` pill, the query, the count, the keys.
    pub(crate) fn render(&mut self, frame: &mut Frame, area: Rect) {
        render_chat_search(self, frame, area);
    }
}

/// The searched text of item IT of T.
fn item_text(thread: &chat::Thread, it: &Item) -> String {
    match it {
        Item::Prompt { text, .. } | Item::Text { text, .. } | Item::Note { text, .. } | Item::Edits { text, .. } => {
            text.clone()
        }
        Item::Run { calls, .. } => calls
            .iter()
            .map(|character| chat::call_summary(&character.name, &character.input))
            .collect::<Vec<_>>()
            .join("\n"),
        Item::Shell { command, output, .. } => format!("{command}\n{output}"),
        Item::Settled { question, answer, .. } => format!("{question}\n{answer}"),
        Item::Kid { title, text, .. } => format!("{title}\n{text}"),
        Item::Sub { sub, .. } => thread
            .subs
            .get(*sub)
            .map(|text_value| format!("{}\n{}", text_value.description, text_value.result))
            .unwrap_or_default(),
    }
}

/// LINE with the substring hits of WORDS (lowercased) marked (accent background).
pub(crate) fn mark(line: &Line<'static>, words: &[String]) -> Line<'static> {
    let text: String = line
        .spans
        .iter()
        .map(|text_value| text_value.content.as_ref())
        .collect();
    let low: Vec<char> = text
        .chars()
        .map(|character| character.to_lowercase().next().unwrap_or(character))
        .collect();
    let mut on = vec![false; low.len()];

    for width in words {
        mark_width_value(&low, &mut on, width);
    }

    if !on.contains(&true) {
        return line.clone();
    }

    let hl = theme::pill().add_modifier(Modifier::BOLD);
    let out: Vec<Span<'static>> = Vec::new();
    let at = 0;

    finish_mark(line, &on, hl, out, at)
}

fn mark_width_value(low: &[char], on: &mut [bool], width: &str) {
    let width_value: Vec<char> = width.chars().collect();

    if width_value.is_empty() || width_value.len() > low.len() {
        return;
    }

    for i in 0..=low.len() - width_value.len() {
        if low[i..i + width_value.len()] == width_value[..] {
            on[i..i + width_value.len()].iter_mut().for_each(|bytes| *bytes = true);
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
    for text_2 in &line.spans {
        finish_mark_chars(on, hl, &mut out, &mut at, text_2);
    }

    Line {
        spans: out,
        ..line.clone()
    }
}

fn finish_mark_chars(
    on: &[bool],
    hl: ratatui::prelude::Style,
    out: &mut Vec<Span<'static>>,
    at: &mut usize,
    text_2: &Span<'_>,
) {
    let chars: Vec<char> = text_2.content.chars().collect();
    let mut buf = String::new();
    let mut cur: Option<bool> = None;

    for (j, character_value) in chars.iter().enumerate() {
        finish_mark_chars_message(
            SearchRunState {
                on,
                hl,
                out,
                at,
                buf: &mut buf,
                cur: &mut cur,
            },
            text_2,
            j,
            *character_value,
        );
    }

    if !buf.is_empty() {
        out.push(Span::styled(
            buf,
            if cur == Some(true) {
                text_2.style.patch(hl)
            } else {
                text_2.style
            },
        ));
    }

    *at += chars.len();
}

fn finish_mark_chars_message(context: SearchRunState<'_>, text_2: &Span<'_>, j: usize, character_value: char) {
    let SearchRunState {
        on,
        hl,
        out,
        at,
        buf,
        cur,
    } = context;

    let message = on.get(*at + j).copied().unwrap_or(false);

    if cur.is_some_and(|x| x != message) {
        out.push(Span::styled(
            std::mem::take(buf),
            if *cur == Some(true) {
                text_2.style.patch(hl)
            } else {
                text_2.style
            },
        ));
    }

    *cur = Some(message);
    buf.push(character_value);
}

#[cfg(test)]
#[path = "chat_chat_search_tests.rs"]
mod tests;

fn refresh_chat_search(state: &mut ChatSearch, thread: &chat::Thread) -> Option<usize> {
    if !state.open {
        return None;
    }

    let query = state.ta.text();
    let count = thread.items.len();
    let mut grown = false;
    let mut grow: Vec<usize> = (count.saturating_sub(TAIL)..count).chain(thread.open_runs()).collect();

    grow.sort_unstable();
    grow.dedup();
    let have = count.min(state.msgs.len());

    for i in grow.into_iter().filter(|&i| i < have) {
        let text = item_text(thread, &thread.items[i]);

        if state.msgs[i].text != text {
            state.msgs[i] = Msg::new("", "", "", 0, &text);
            grown = true;
        }
    }

    if query == state.seen.0 && count == state.seen.1 && !grown {
        return None;
    }

    let new_query = query != state.seen.0;

    // a rebuilt stream (the log's head came in): the texts again
    if state.msgs.len() > count || state.seen.1 > count {
        state.msgs.clear();
    }

    while state.msgs.len() < count {
        let i = state.msgs.len();

        state
            .msgs
            .push(Msg::new("", "", "", 0, &item_text(thread, &thread.items[i])));
    }

    let shown = state.keep.take().or_else(|| state.current());
    let words = search::words(&query);

    state.hits = (0..count).filter(|&i| search::hit(&state.msgs[i], &words)).collect();
    state.seen = (query, count);

    state.cur = match shown {
        Some(text_value) if !new_query => state.hits.iter().position(|&height| height == text_value),
        _ => state.hits.len().checked_sub(1),
    };

    if new_query { state.current() } else { None }
}

fn key_chat_search(state: &mut ChatSearch, key_value: KeyEvent, thread: &chat::Thread) -> Act {
    if !state.open || !state.focus {
        return Act::Pass;
    }

    let none = key_value.modifiers.is_empty();

    match key_value.code {
        KeyCode::Esc => {
            state.close();
            return Act::Close;
        }
        KeyCode::Enter | KeyCode::Up if none => return state.step(true).map_or(Act::None, Act::Show),
        KeyCode::Down => return state.step(false).map_or(Act::None, Act::Show),
        _ => {}
    }

    if matches!(state.ta.key(key_value), Outcome::Ignored) {
        return Act::Pass;
    }

    state.refresh(thread).map_or(Act::None, Act::Show)
}

fn render_chat_search(state: &mut ChatSearch, frame: &mut Frame, area: Rect) {
    let pill = " find ";
    let label = state.label();

    let keys = if state.focus {
        "tab ↑: older · ⇧tab ↓: newer · esc: close"
    } else {
        "ctrl+f: back to the search"
    };

    frame.render_widget(
        Paragraph::new(Span::styled(
            pill,
            if state.focus { theme::pill() } else { theme::muted() },
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

    state.ta.render(frame, Rect { x, width: qw, ..area }, state.focus);

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
