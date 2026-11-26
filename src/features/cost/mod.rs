//! The tokens box of the thread view (bottom right of the header), and `codehead --cost <thread>`.
//! Compact: per harness its plan windows, per agent its context (dithered bars, 1 column grid).
//! Expanded (ctrl+o / ⌘o / a click): everything else above it: tokens, $, cache, turns, the next
//! message, every thread's cost.
//! Sources: the thread's event log (turn ends), the host's live agents, `limits.json`.

use crate::engine::client::State;
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use ratatui::{DefaultTerminal, Frame};

use crate::{domain::agents, ui::theme};

/// The plan windows' resets move with the clock: 1 read per CLOCK without a log change.
const CLOCK: Duration = Duration::from_secs(30);
const SPARK: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

#[derive(Default, Clone)]
struct AgentRow {
    name: String,
    color: Option<String>,
    /// Context fill of the agent's latest session, percent.
    ctx: Option<u64>,
    live: bool,
    /// Context tokens of the latest session: the least a next message sends.
    ctx_tokens: u64,
    /// Every number of the agent in this thread.
    usage: AgentUse,
}

type Limit = crate::engine::limits::Window;

#[derive(Default, Clone)]
pub(crate) struct Snap {
    found: bool,
    id: String,
    title: String,
    cost: f64,
    agents: Vec<AgentRow>,
    /// The plan windows of the thread's harnesses only.
    limits: Vec<Limit>,
}

/// Threads read at most for [`past_rate`] (newest first; the last 256 KB of each log).
const RATE_THREADS: usize = 40;

type Names = HashMap<String, (String, Option<String>)>;

/// The next message of the thread view: its recipients, its length (chars).
#[derive(Clone, Debug, Default)]
pub(crate) struct Next {
    pub to: Vec<String>,
    pub chars: usize,
}

/// Seconds of a cost box morph.
const MORPH_TIME: Duration = Duration::from_millis(450);

static MORPH: std::sync::Mutex<Option<Morph>> = std::sync::Mutex::new(None);

/// The cost box from FROM (what was on screen) to TO (the new box), from START. Buffers at 0, 0.
struct Morph {
    from: Buffer,
    to: Buffer,
    start: Instant,
}

impl Morph {
    /// The box now: the changed cells on their way from FROM to TO (`dissolve::mix`).
    fn shown(&self) -> Buffer {
        let progress = (self.start.elapsed().as_secs_f32() / MORPH_TIME.as_secs_f32()).min(1.0);

        crate::ui::dissolve::mix(&self.from, &self.to, progress)
    }
}

#[cfg(test)]
#[path = "cost_tests.rs"]
mod tests;

/// Per agent of a thread: sessions, token kinds, turns, $, context, last 24h.
#[derive(Clone, Debug, Default)]
pub(crate) struct AgentUse {
    pub agent: String,
    pub harness: String,
    pub sessions: u64,
    pub live: bool,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub turns: u64,
    pub usd: f64,
    /// Context of the latest session: (used, size).
    pub ctx: Option<(u64, u64)>,
    /// Last 24h: (unix time, tokens).
    pub samples: Vec<(u64, u64)>,
    /// $ per token of the last turn (every kind, cache incl.).
    pub usd_per_token: f64,
}

/// 1 thread of the every-thread list: its id, title, $, tokens, turns.
#[derive(Clone, Debug, Default)]
pub(crate) struct ThreadCost {
    pub id: String,
    pub title: String,
    pub usd: f64,
    pub tokens: u64,
    pub turns: u64,
}

/// Every thread's cost (the expanded box's last section): the top 8, the sums, the last 24h.
#[derive(Clone, Debug, Default)]
pub(crate) struct AllThreads {
    pub threads: Vec<ThreadCost>,
    pub usd: f64,
    pub tokens: u64,
    pub day_usd: f64,
}

/// What the box shows besides the snapshot: the next message, every thread's cost (expanded).
#[derive(Clone, Copy)]
pub(crate) struct Show<'a> {
    pub next: &'a Next,
    pub expanded: Option<Expanded<'a>>,
}

/// The expanded box: every thread's cost (None: still on its way), rows scrolled off its top.
#[derive(Clone, Copy)]
pub(crate) struct Expanded<'a> {
    pub all: Option<&'a AllThreads>,
    pub scroll: u16,
}

#[path = "cost_data.rs"]
mod data;
pub(crate) use data::snapshot;
use data::{fetch, names, now};

#[path = "cost_render.rs"]
mod render;
pub(crate) use render::{box_rows, draw_in, draw_last, expanded_rows, human, morphing};
use render::{dither_bar, draw_at, until, window, window_bar};

#[path = "cost_compact.rs"]
mod compact;
use compact::compact_lines;

#[path = "cost_expanded.rs"]
mod expanded;
use expanded::expanded_lines;

#[path = "cost_terminal.rs"]
mod terminal;
pub(crate) use terminal::run;

#[path = "cost_detail.rs"]
mod detail;
pub(crate) use detail::all_threads;

#[cfg(test)]
use data::{add_running, last_rate, last_window, of_harnesses};
