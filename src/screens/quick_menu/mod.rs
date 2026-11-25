//! The menu (ctrl+p): 1 modal for the start screen and the thread view. Tabs: quick (commands; in
//! a thread: per agent its flow, remove, replace), threads (the thread list: filter, repo + view
//! tabs, cards, actions), merge, search (the messages of every thread), tokens. Focus levels: the
//! tab bar, a tab's tab groups (threads: repos, views; merge: states), its list. ↑ at the top of a
//! list: up 1 level; ↓: down 1 level. ←→ on the tab bar (a list without a tab group too): next /
//! previous tab; on a tab group: its tabs. enter: run, a single esc or tab closes.

use std::sync::mpsc::Receiver;
use std::time::Instant;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use crate::domain::agents::Agent;
use crate::features::thread_list::{self, ThreadList};
use crate::ui::textarea::TextArea;
use crate::ui::theme;
use crate::ui::thread_card::{self, Card};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Tab {
    Quick,
    Threads,
    /// The owner's pull requests.
    Merge,
    Search,
    Tokens,
}

const TABS: [(Tab, &str); 5] = [
    (Tab::Quick, "quick"),
    (Tab::Threads, "threads"),
    (Tab::Merge, "merge"),
    (Tab::Search, "search"),
    (Tab::Tokens, "tokens"),
];

/// A tab of the start screen the menu opens.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Home {
    New,
    Settings,
    /// The threads tab (the thread in view got archived / deleted).
    Threads,
    /// The merge tab, a PR open in it (0 thread has it).
    Merge(Option<crate::infra::gh::PrRef>),
    /// The flows tab, a flow open on its rail.
    Flows(Option<String>),
}

/// What the host does after a key / click.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Action {
    /// Menu stays open.
    None,
    Close,
    /// The start screen as it was (home).
    Back,
    Home(Home),
    Open(Card),
    /// A search result: thread CARD, scrolled to its message of time TS.
    Jump(Card, u64),
    /// Quick command of the host (its id).
    Command(usize),
    /// A PR of the merge tab: its thread's merge tab, else the start screen's.
    Pr(crate::screens::merge::pr_list::Entry),
}

/// Quick command of the host: label, hotkey hint, id, section (a heading row over each section).
pub(crate) struct Item {
    pub label: String,
    pub key: String,
    pub id: usize,
    pub section: &'static str,
}

/// Where the keys go: the tab bar, the tab group of the tab (merge), the list. The threads tab:
/// Bar, else its thread list's own focus.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Level {
    Bar,
    Group,
    Body,
}

pub(crate) struct Menu {
    pub tab: Tab,
    pub level: Level,
    items: Vec<Item>,
    qcursor: usize,
    /// The threads tab.
    pub list: ThreadList,
    /// Thread in view (None: start screen).
    thread: Option<String>,
    /// The agents (their colors in the search tab).
    roster: Vec<Agent>,
    started: Instant,

    /// Merge tab: the owner's PRs.
    prs: crate::screens::merge::pr_list::PrList,

    tokens: TokenState,
    search_state: SearchState,
    layout: MenuLayout,
}

/// Time between 2 reads of the messages (search tab) while they show.
const MSGS_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

/// Quick ids of the menu's own commands.
const NEW: usize = usize::MAX;
const SETTINGS: usize = usize::MAX - 1;
const SEARCH: usize = usize::MAX - 2;
const HOME: usize = usize::MAX - 3;

fn builtins() -> Vec<Item> {
    vec![
        Item {
            label: "new thread: start screen, new tab".into(),
            key: "n".into(),
            id: NEW,
            section: "codehead",
        },
        Item {
            label: "home: the start screen (thread view: esc esc, ⌂ home)".into(),
            key: "h".into(),
            id: HOME,
            section: "codehead",
        },
        Item {
            label: "settings: start screen, settings tab".into(),
            key: String::new(),
            id: SETTINGS,
            section: "codehead",
        },
        Item {
            label: "search: the messages of every thread".into(),
            key: "f".into(),
            id: SEARCH,
            section: "codehead",
        },
    ]
}

impl Menu {
    /// ITEMS: the host's quick commands (after new + settings). THREAD: the thread in view.
    /// CARDS: thread cards the host has (empty: read in the background now).
    pub(crate) fn new(items: Vec<Item>, thread: Option<String>, roster: Vec<Agent>, cards: Vec<Card>) -> Menu {
        let mut all = builtins();

        all.extend(items);
        let mut search = TextArea::new(true, "search messages: you + agents, every thread");

        search.style = Some(theme::text());
        let mut list = ThreadList::new(thread_list::Mode::Switch, thread.clone(), None, cards);

        list.roster = roster.iter().map(|agent| agent.name.clone()).collect();

        Menu {
            tab: Tab::Quick,
            level: Level::Body,
            items: all,
            qcursor: 0,
            list,
            thread,
            roster,
            started: Instant::now(),

            prs: crate::screens::merge::pr_list::PrList::default(),

            tokens: TokenState {
                detail: None,
                detail_rx: None,
                detail_at: None,
                dscroll: 0,
            },
            search_state: SearchState {
                search,
                msgs: None,
                msgs_rx: None,
                msgs_at: None,
                hits: Vec::new(),
                hits_for: String::new(),
                scursor: 0,
                soffset: 0,
            },
            layout: MenuLayout {
                tab_rects: Vec::new(),
                rows: Vec::new(),
                frame: Rect::default(),
            },
        }
    }

    /// A background read on its way (threads, messages, tokens, PRs): the host draws often.
    pub(crate) fn waiting(&self) -> bool {
        self.list.waiting()
            || self.search_state.msgs_rx.is_some()
            || self.tokens.detail_rx.is_some()
            || (self.tab == Tab::Merge && self.prs.reading())
    }

    /// New cards (a read): the cursor stays on its thread.
    #[cfg(test)]
    fn set_cards(&mut self, cards: Vec<Card>) {
        self.list.set_cards(cards);
    }

    /// New messages (a read): the hits again, the cursor stays on its message.
    pub(crate) fn set_msgs(&mut self, msgs: Vec<crate::domain::search::Msg>) {
        let key = |msg: &crate::domain::search::Msg| (msg.thread.clone(), msg.ts, msg.from.clone());

        let picked = self
            .search_state
            .hits
            .get(self.search_state.scursor)
            .and_then(|&i| self.search_state.msgs.as_ref()?.get(i))
            .map(key);

        self.search_state.msgs = Some(msgs);
        let query = self.search_state.search.text();
        let msgs = self.search_state.msgs.as_deref().unwrap_or_default();

        self.search_state.hits = crate::domain::search::find(msgs, &query);
        self.search_state.hits_for = query;
        self.search_state.scursor = picked
            .and_then(|picked| self.search_state.hits.iter().position(|&i| key(&msgs[i]) == picked))
            .unwrap_or(0);
    }

    /// The card of thread ID (the threads tab's, else 1 from the message).
    fn card_of(&self, message: &crate::domain::search::Msg) -> Card {
        self.list
            .cards
            .iter()
            .find(|card| card.id == message.thread)
            .cloned()
            .unwrap_or_else(|| Card {
                id: message.thread.clone(),
                title: message.title.clone(),
                dir: message.dir.clone(),
                ..Default::default()
            })
    }

    fn jump(&self, hit: usize) -> Action {
        let Some(message) = self
            .search_state
            .hits
            .get(hit)
            .and_then(|&i| self.search_state.msgs.as_ref()?.get(i))
        else {
            return Action::None;
        };

        Action::Jump(self.card_of(message), message.ts)
    }

    /// What the thread list's OUT means for the host.
    fn list_out(&mut self, out: thread_list::Out) -> Action {
        match out {
            thread_list::Out::Up => {
                self.level = Level::Bar;
                Action::None
            }
            thread_list::Out::Open(card) => Action::Open(*card),
            thread_list::Out::Home => Action::Home(Home::Threads),
            thread_list::Out::None | thread_list::Out::Pass => Action::None,
        }
    }

    fn switch(&mut self, delta: isize) {
        let i = TABS.iter().position(|(tab, _)| *tab == self.tab).unwrap_or(0) as isize;

        self.tab = TABS[(i + delta).rem_euclid(TABS.len() as isize) as usize].0;
        self.level = Level::Bar;
    }

    /// Tab (BACK: shift+tab): the next / previous region of the tab: the bar, its groups
    /// (threads: repos, projects, views, status; merge: states), its list.
    fn cycle(&mut self, back: bool) {
        #[derive(Clone, Copy, PartialEq)]
        enum Region {
            Bar,
            Group,
            Repos,
            Projects,
            Views,
            Status,
            Body,
        }

        let regions: &[Region] = match self.tab {
            Tab::Threads if self.list.has_stats() => &[
                Region::Bar,
                Region::Repos,
                Region::Projects,
                Region::Views,
                Region::Status,
                Region::Body,
            ],
            Tab::Threads => &[
                Region::Bar,
                Region::Repos,
                Region::Projects,
                Region::Views,
                Region::Body,
            ],
            Tab::Merge => &[Region::Bar, Region::Group, Region::Body],
            _ => &[Region::Bar, Region::Body],
        };

        let now = match (self.level, self.tab) {
            (Level::Bar, _) => Region::Bar,
            (Level::Group, _) => Region::Group,
            (Level::Body, Tab::Threads) => match self.list.focus {
                thread_list::Focus::Repos => Region::Repos,
                thread_list::Focus::Projects => Region::Projects,
                thread_list::Focus::Views => Region::Views,
                thread_list::Focus::Status => Region::Status,
                thread_list::Focus::List => Region::Body,
            },
            (Level::Body, _) => Region::Body,
        };

        let at = regions.iter().position(|region| *region == now).unwrap_or(0) as isize;
        let to = regions[(at + if back { -1 } else { 1 }).rem_euclid(regions.len() as isize) as usize];

        self.level = match to {
            Region::Bar => Level::Bar,
            Region::Group => Level::Group,
            _ => Level::Body,
        };

        match to {
            Region::Repos => self.list.focus = thread_list::Focus::Repos,
            Region::Projects => self.list.focus = thread_list::Focus::Projects,
            Region::Views => self.list.focus = thread_list::Focus::Views,
            Region::Status => self.list.focus = thread_list::Focus::Status,
            Region::Body if self.tab == Tab::Threads => self.list.focus = thread_list::Focus::List,
            _ => {}
        }
    }

    /// The tab has a tab group (threads: repos + views, merge: states).
    fn grouped(&self) -> bool {
        matches!(self.tab, Tab::Threads | Tab::Merge)
    }

    /// The list of the tab shows its 1st row (↑: up 1 level).
    fn at_top(&self) -> bool {
        match self.tab {
            Tab::Quick => self.qcursor == 0,
            Tab::Threads => self.list.cursor == 0,
            Tab::Merge => self.prs.at_top(),
            Tab::Search => self.search_state.scursor == 0,
            Tab::Tokens => self.tokens.dscroll == 0,
        }
    }

    /// The host's quick commands again (they changed while the menu is open: an agent came,
    /// left, its flow moved on). The cursor stays on its command.
    pub(crate) fn set_items(&mut self, items: Vec<Item>) {
        let same = self.items.len() == builtins().len() + items.len()
            && self.items.iter().skip(builtins().len()).zip(&items).all(|(old, new)| {
                (old.id, &old.label, &old.key, old.section) == (new.id, &new.label, &new.key, new.section)
            });

        if same {
            return;
        }

        let picked = self.items.get(self.qcursor).map(|item| item.id);

        self.items = builtins();
        self.items.extend(items);
        self.qcursor = picked
            .and_then(|id| self.items.iter().position(|item| item.id == id))
            .unwrap_or(0)
            .min(self.items.len() - 1);
    }

    fn run_item(&mut self, i: usize) -> Action {
        match self.items.get(i).map(|item| item.id) {
            Some(NEW) => Action::Home(Home::New),
            Some(HOME) => Action::Back,
            Some(SEARCH) => {
                self.tab = Tab::Search;
                self.level = Level::Body;
                Action::None
            }
            Some(SETTINGS) => Action::Home(Home::Settings),
            Some(id) => Action::Command(id),
            None => Action::None,
        }
    }

    /// Row under AT (hover).
    pub(crate) fn row_at(&self, at: Position) -> Option<Rect> {
        let list = if self.tab == Tab::Threads {
            self.list.row_at(at)
        } else {
            None
        };

        list.or_else(|| {
            self.layout
                .rows
                .iter()
                .find(|(rect, _)| rect.contains(at))
                .map(|(rect, _)| *rect)
        })
        .or_else(|| {
            self.layout
                .tab_rects
                .iter()
                .find(|(rect, _)| rect.contains(at))
                .map(|(rect, _)| *rect)
        })
    }

    fn row_style(hl: bool) -> (Style, Style) {
        if hl {
            (
                Style::default().fg(theme::pal().on_accent).add_modifier(Modifier::BOLD),
                Style::default().fg(theme::pal().on_accent),
            )
        } else {
            (theme::accent(), theme::text())
        }
    }

    /// Rows per search hit: who · thread · time, the snippet, a gap.
    const HIT_H: u16 = 3;
}

#[cfg(test)]
#[path = "quick_menu_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "quick_menu_render_tests.rs"]
mod render_tests;

#[path = "quick_menu_draw_helpers.rs"]
mod draw_helpers;
#[path = "quick_menu_key.rs"]
mod key;
#[path = "quick_menu_key_helpers.rs"]
mod key_helpers;
#[path = "quick_menu_mouse.rs"]
mod mouse;
#[path = "quick_menu_poll.rs"]
mod poll;
#[path = "quick_menu_render.rs"]
mod render;
#[path = "quick_menu_updates.rs"]
mod updates;

struct TokenState {
    /// Tokens tab: the last read, the read running, its time, the first line in view.
    detail: Option<crate::features::cost::Detail>,
    detail_rx: Option<Receiver<crate::features::cost::Detail>>,
    detail_at: Option<Instant>,
    dscroll: u16,
}

struct SearchState {
    /// Search tab: the query, every thread's messages (read once, in the background), the hits of
    /// QUERY (indexes into MSGS) and the query they are for, cursor, 1st hit in view.
    search: TextArea,
    msgs: Option<Vec<crate::domain::search::Msg>>,
    msgs_rx: Option<Receiver<Vec<crate::domain::search::Msg>>>,
    /// Start of the last messages read: the search tab reads again every 5s (new messages).
    msgs_at: Option<Instant>,
    hits: Vec<usize>,
    hits_for: String,
    scursor: usize,
    soffset: usize,
}

struct MenuLayout {
    /// Last draw: tab labels, rows (clicks).
    tab_rects: Vec<(Rect, Tab)>,
    rows: Vec<(Rect, usize)>,
    frame: Rect,
}
