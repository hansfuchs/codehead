//! The owner's pull requests (every repo, every state), grouped by repo (the repo of the newest
//! PR first), newest first in a repo: the start screen's merge tab and the menu's. Each PR with
//! its thread, if one has it: a checkout of the thread on the PR's branch, or the PR among the
//! thread's PRs the merge tab found (`<data>/prs/threads.json`).
//!
//! Read in the background: the last list from `<data>/prs/mine.json` at once, GitHub again when
//! it is older than 2 min (r: now, 3 s apart).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::Color;
use serde_json::{Value, json};

use crate::infra::gh::{self, Item, PrRef, State};
use crate::ui::theme;

const STALE: Duration = Duration::from_secs(120);
const MIN_MANUAL: Duration = Duration::from_secs(3);

/// A thread of a PR.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Link {
    pub id: String,
    pub title: String,
    pub dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    pub item: Item,
    pub thread: Option<Link>,
}

/// What a key / click picked.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Pick {
    None,
    Open(Entry),
    Toast(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Filter {
    #[default]
    Open,
    Merged,
    Closed,
    All,
}

impl Filter {
    /// The state tabs, in order.
    const ALL: [Filter; 4] = [Filter::Open, Filter::Merged, Filter::Closed, Filter::All];

    fn keeps(self, text: State) -> bool {
        match self {
            Filter::All => true,
            Filter::Open => matches!(text, State::Open | State::Draft),
            Filter::Merged => text == State::Merged,
            Filter::Closed => text == State::Closed,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Filter::All => "all",
            Filter::Open => "open",
            Filter::Merged => "merged",
            Filter::Closed => "closed",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum RowKind {
    Repo(String, usize),
    Pr(usize),
}

#[derive(Default)]
pub(crate) struct PrList {
    entries: Vec<Entry>,
    read: ReadState,
    filter: Filter,
    /// The state tabs (←→): open, merged, closed, all.
    tabs: crate::ui::tab_group::TabGroup,
    /// Index into the PR rows (headers skipped).
    cursor: usize,
    offset: usize,
    /// The view follows the cursor (a key moved it); a mouse scroll frees the view.
    free: bool,
    rows: Vec<(Rect, usize)>,
    /// Data version: up with each read that brings new PRs (the list dissolves in).
    version: u64,
    fx: crate::ui::dissolve::Dissolve,
}

#[derive(Default)]
struct ReadState {
    receiver: Option<Receiver<Result<Vec<Entry>, String>>>,
    at: Option<Instant>,
    cached: bool,
    error: Option<String>,
}

impl PrList {
    /// Tests: ENTRIES as read.
    #[cfg(test)]
    pub(crate) fn with(entries: Vec<Entry>) -> PrList {
        PrList {
            entries,
            read: ReadState {
                at: Some(Instant::now()),
                cached: true,
                ..ReadState::default()
            },
            ..Default::default()
        }
    }

    /// The list shows: the cache at once, a read when it is stale.
    pub(crate) fn ensure(&mut self) {
        if !self.read.cached && !cfg!(test) {
            self.read.cached = true;
            self.entries = cached();
        }

        if self.read.receiver.is_none() && self.read.at.is_none_or(|thread| thread.elapsed() > STALE) {
            self.start();
        }
    }

    fn start(&mut self) {
        self.read.at = Some(Instant::now());

        if cfg!(test) {
            return;
        }

        let (tx, rx) = crate::infra::bus::channel();

        std::thread::spawn(move || {
            let _ = tx.send(load());
        });

        self.read.receiver = Some(rx);
    }

    /// A finished read applies. True: a redraw is due.
    /// A read is on its way.
    pub(crate) fn reading(&self) -> bool {
        self.read.receiver.is_some()
    }

    pub(crate) fn poll(&mut self) -> bool {
        poll_entries(self)
    }

    /// The rows: per repo (the newest PR's repo first) a head, its PRs newest first.
    fn layout(&self) -> (Vec<RowKind>, Vec<usize>) {
        layout_rows(self)
    }

    /// The cursor on the 1st PR (↑ leaves the list).
    pub(crate) fn at_top(&self) -> bool {
        self.cursor == 0
    }

    /// AT on a state tab (last draw).
    pub(crate) fn tab_at(&self, at: Position) -> bool {
        self.tabs.hit(at)
    }

    pub(crate) fn key(&mut self, key_value: KeyEvent) -> Pick {
        handle_key(self, key_value)
    }

    pub(crate) fn mouse(&mut self, event: MouseEvent) -> Pick {
        handle_mouse(self, event)
    }

    /// The list in AREA: the cache at once, a read's new PRs dissolve in.
    pub(crate) fn draw(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        self.draw_levels(frame, area, focused, focused);
    }

    /// The list in AREA, `TABS_ON`: the state tabs have the keys, `ROWS_ON`: the PR rows have them.
    pub(crate) fn draw_levels(&mut self, frame: &mut Frame, area: Rect, tabs_on: bool, rows_on: bool) {
        self.draw_now(frame, area, tabs_on, rows_on);
        let list = Rect {
            y: area.y + 3,
            height: area.height.saturating_sub(3),
            ..area
        };

        self.fx.apply(frame, list, self.version);
    }

    fn draw_now(&mut self, frame: &mut Frame, area: Rect, tabs_on: bool, focused: bool) {
        draw_now(self, frame, area, tabs_on, focused);
    }
}

fn state_mark(text: State) -> (&'static str, Color) {
    match text {
        State::Open => ("●", theme::pal().success()),
        State::Draft => ("◌", theme::pal().subtext0),
        State::Merged => ("◆", theme::pal().merged()),
        State::Closed => ("✕", theme::pal().error()),
    }
}

#[cfg(test)]
#[path = "merge_pr_list_tests.rs"]
mod tests;

pub(crate) use crate::domain::pull_requests::{remember, remembered};

fn poll_entries(list: &mut PrList) -> bool {
    let Some(rx) = &list.read.receiver else {
        return false;
    };

    let Ok(result) = rx.try_recv() else { return false };

    list.read.receiver = None;

    match result {
        Ok(entries_value) => {
            if entries_value != list.entries {
                list.version += 1;
            }

            list.entries = entries_value;
            list.read.error = None;
        }
        Err(error_3) => list.read.error = Some(error_3),
    }

    true
}

fn layout_rows(list: &PrList) -> (Vec<RowKind>, Vec<usize>) {
    let mut repos: Vec<(String, u64)> = Vec::new();

    for entry in list
        .entries
        .iter()
        .filter(|candidate| list.filter.keeps(candidate.item.state))
    {
        match repos.iter_mut().find(|group| group.0 == entry.item.pr.repo) {
            Some(existing_repo) => existing_repo.1 = existing_repo.1.max(entry.item.created),
            None => repos.push((entry.item.pr.repo.clone(), entry.item.created)),
        }
    }

    repos.sort_by(|agent, bytes| bytes.1.cmp(&agent.1).then_with(|| agent.0.cmp(&bytes.0)));
    let mut rows = Vec::new();
    let mut prs = Vec::new();

    for (repo, _) in repos {
        let mut of: Vec<usize> = (0..list.entries.len())
            .filter(|&i| list.entries[i].item.pr.repo == repo && list.filter.keeps(list.entries[i].item.state))
            .collect();

        of.sort_by(|&agent_value, &bytes_value| {
            list.entries[bytes_value]
                .item
                .created
                .cmp(&list.entries[agent_value].item.created)
        });

        rows.push(RowKind::Repo(repo, of.len()));

        for i in of {
            rows.push(RowKind::Pr(i));
            prs.push(i);
        }
    }

    (rows, prs)
}

fn handle_key(list: &mut PrList, key: KeyEvent) -> Pick {
    let (_, prs) = list.layout();
    let count = prs.len();

    list.free = false;

    match key.code {
        KeyCode::Up | KeyCode::Char('k') => list.cursor = list.cursor.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => {
            list.cursor = (list.cursor + 1).min(count.saturating_sub(1));
        }
        KeyCode::PageUp => list.cursor = list.cursor.saturating_sub(10),
        KeyCode::PageDown => list.cursor = (list.cursor + 10).min(count.saturating_sub(1)),
        KeyCode::Home | KeyCode::Char('g') => list.cursor = 0,
        KeyCode::End | KeyCode::Char('G') => list.cursor = count.saturating_sub(1),
        KeyCode::Left | KeyCode::Right => {
            list.tabs.i = Filter::ALL.iter().position(|file| *file == list.filter).unwrap_or(0);
            list.tabs
                .step(if key.code == KeyCode::Right { 1 } else { -1 }, Filter::ALL.len());
            list.filter = Filter::ALL[list.tabs.i];
            list.cursor = 0;
            list.offset = 0;
        }
        KeyCode::Char('r') => {
            if list.read.receiver.is_some() || list.read.at.is_some_and(|thread| thread.elapsed() < MIN_MANUAL) {
                return Pick::Toast("pull requests: a read on its way".into());
            }

            list.start();
            return Pick::Toast("pull requests: reading GitHub".into());
        }
        KeyCode::Enter => {
            if let Some(entry) = prs.get(list.cursor).map(|&i| list.entries[i].clone()) {
                return Pick::Open(entry);
            }
        }
        _ => {}
    }

    Pick::None
}

fn handle_mouse(list: &mut PrList, event: MouseEvent) -> Pick {
    let at = Position {
        x: event.column,
        y: event.row,
    };

    match event.kind {
        MouseEventKind::ScrollDown => {
            list.free = true;
            list.offset += 3;
        }
        MouseEventKind::ScrollUp => {
            list.free = true;
            list.offset = list.offset.saturating_sub(3);
        }
        MouseEventKind::Down(MouseButton::Left) => return click_list(list, at),
        _ => {}
    }

    Pick::None
}

fn click_list(list: &mut PrList, at: Position) -> Pick {
    if list.tabs.click(at) {
        list.filter = Filter::ALL[list.tabs.i.min(Filter::ALL.len() - 1)];
        list.cursor = 0;
        list.offset = 0;
        return Pick::None;
    }

    if let Some(&(_, row_index)) = list.rows.iter().find(|(area, _)| area.contains(at)) {
        list.free = true;

        if list.cursor == row_index {
            return list.key(KeyEvent::from(KeyCode::Enter));
        }

        list.cursor = row_index;
    }

    Pick::None
}

#[path = "pr_list_draw.rs"]
mod drawing;
use drawing::draw_now;

#[path = "pr_list_store.rs"]
mod store;
use store::{cached, load};
#[cfg(test)]
use store::{item_json, item_of};
