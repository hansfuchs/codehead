//! `codehead --cost <thread>`: the cost pane of the thread view (bottom right).
//! Scoped to 1 thread: per agent tokens, $, a context line, then the plan limits (line bars).
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
    tokens: u64,
    usd: f64,
    /// Context fill of the agent's latest session, percent.
    ctx: Option<u64>,
    live: bool,
    /// Saved in the agent prompt, else the stable pool pick (`agents::sprite`).
    sprite: Option<crate::domain::sprites::Sprite>,
    /// Context tokens of the latest session: the least a next message sends.
    ctx_tokens: u64,
    /// $ per token of the last turn (every kind, cache incl.).
    usd_per_token: f64,
}

type Limit = crate::engine::limits::Window;

#[derive(Default)]
pub(crate) struct Snap {
    found: bool,
    status: String,
    cost: f64,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    turns: u64,
    agents: Vec<AgentRow>,
    limits: Vec<Limit>,
}

/// Threads read at most for [`past_rate`] (newest first; the last 256 KB of each log).
const RATE_THREADS: usize = 40;

type Names = HashMap<String, (String, Option<String>, crate::domain::sprites::Sprite)>;

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

/// The tokens tab: the thread in view (None: start screen), every thread's cost, plan limits.
#[derive(Clone, Debug, Default)]
pub(crate) struct Detail {
    pub agents: Vec<AgentUse>,
    /// Thread ids by cost: (id, $, tokens, turns). The top 8.
    pub threads: Vec<(String, f64, u64, u64)>,
    /// $ and tokens of every thread, and of the last 24h (sessions updated since).
    pub all_usd: f64,
    pub all_tokens: u64,
    pub day_usd: f64,
    pub limits: Vec<(String, String, f64, Option<u64>)>,
}

#[cfg(test)]
#[path = "cost_detail_tests.rs"]
mod detail_tests;

#[path = "cost_data.rs"]
mod data;
use data::{fetch, names, now};
pub(crate) use data::{limits, snapshot, uses};

#[path = "cost_render.rs"]
mod render;
use render::{bar, draw, until, window};
pub(crate) use render::{draw_in, draw_last, human, morphing};

#[path = "cost_terminal.rs"]
mod terminal;
pub(crate) use terminal::run;

#[path = "cost_detail.rs"]
mod detail;
pub(crate) use detail::{detail, detail_lines};

#[cfg(test)]
use data::{add_running, last_rate};
#[cfg(test)]
use render::draw_at;
