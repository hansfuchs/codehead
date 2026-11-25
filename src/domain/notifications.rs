use crate::domain::palette as theme;
use crate::engine::store::{self, Event, Kind as Ev};
use ratatui::style::Color;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};
/// Kinds, most urgent first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Kind {
    /// A question / permission request waits on the owner.
    Ask,
    Blocked,
    /// The flow waits on the owner (a gate, a loop at its max, a role with 0 agent).
    Gate,
    Error,
    /// A usage limit stopped an agent.
    Limit,
    Conflict,
    /// A check of the thread's PR failed.
    Ci,
    /// A comment / review on the thread's PR, not the owner's.
    Review,
    /// An agent wrote `@owner`.
    Mention,
    /// An agent finished a step (`set_status` review): the next step waits on the owner.
    Step,
    /// An agent replied on a review comment (changes view), unseen.
    Reply,
    /// An agent's context window ≥ 90 %.
    Context,
    /// Every agent on the thread reported done.
    Done,
    /// Every PR of the thread is merged or closed (1 merged at least): archive it.
    Merged,
}

impl Kind {
    pub(crate) fn icon(self) -> &'static str {
        match self {
            Kind::Ask => "?",
            Kind::Blocked => "⏸",
            Kind::Gate => "◆",
            Kind::Error | Kind::Ci => "✗",
            Kind::Limit => "⧗",
            Kind::Conflict => "⇄",
            Kind::Review => "✎",
            Kind::Mention => "@",
            Kind::Step => "▷",
            Kind::Reply => "↳",
            Kind::Context => "◔",
            Kind::Done => "✓",
            Kind::Merged => "⇲",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Kind::Ask => "question",
            Kind::Blocked => "blocked",
            Kind::Gate => "gate",
            Kind::Error => "error",
            Kind::Limit => "usage limit",
            Kind::Conflict => "conflict",
            Kind::Ci => "ci failed",
            Kind::Review => "review",
            Kind::Mention => "mention",
            Kind::Step => "step to review",
            Kind::Reply => "reply",
            Kind::Context => "context",
            Kind::Done => "done",
            Kind::Merged => "merged",
        }
    }

    pub(crate) fn color(self) -> Color {
        let path = theme::pal();

        match self {
            Kind::Error | Kind::Conflict | Kind::Ci => path.error(),
            Kind::Ask | Kind::Blocked | Kind::Gate => path.attention(),
            Kind::Limit | Kind::Context => path.caution(),
            Kind::Review | Kind::Mention | Kind::Step | Kind::Reply => path.info(),
            Kind::Done | Kind::Merged => path.success(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Note {
    pub thread: String,
    pub title: String,
    pub dir: PathBuf,
    pub kind: Kind,
    /// The agents it is about (count in the header); empty: the thread (PR notes).
    pub agents: Vec<String>,
    /// Unix ms of the event: the jump target, the seen check.
    pub ts: u64,
    pub text: String,
    /// A reply note: the comment (the jump opens the changes view on it).
    pub comment: String,
}

impl Note {
    /// Agents it counts for (a PR note: 1).
    pub(crate) fn count(&self) -> usize {
        self.agents.len().max(1)
    }

    pub(crate) fn card(&self) -> crate::domain::thread_card::Card {
        crate::domain::thread_card::Card {
            id: self.thread.clone(),
            title: self.title.clone(),
            dir: Some(self.dir.clone()),
            ..Default::default()
        }
    }
}

// ---------- derive ----------

// ---------- state ----------

struct Cache {
    /// per thread: (log len, log mtime) → its derived notes, its last event ts
    logs: HashMap<String, LogSnapshot>,
    /// PR notes per thread (the last PR pass)
    prs: HashMap<String, Vec<Note>>,
    /// PR condition key → when codehead saw it first (stable ts across passes and restarts)
    first: HashMap<String, u64>,
    /// per thread: its comments file's time → its reply notes (unseen agent replies)
    replies: HashMap<String, (Option<SystemTime>, Vec<Note>)>,
}

static NOTES: Mutex<Vec<Note>> = Mutex::new(Vec::new());

/// Every agent on thread ID reported done, 0 turn since (the last pass's notes, seen or not).
pub(crate) fn reported_done(id: &str) -> bool {
    has_note(id, Kind::Done)
}

/// An agent on thread ID finished a step (`set_status` review), 0 turn since.
pub(crate) fn reported_review(id: &str) -> bool {
    has_note(id, Kind::Step)
}

fn has_note(id: &str, key: Kind) -> bool {
    NOTES.lock().is_ok_and(|count| {
        count
            .iter()
            .any(|count_value| count_value.thread == id && count_value.kind == key)
    })
}

/// Every PR of a thread done, 1 merged at least: STATES of its PRs (several repos too).
pub(crate) fn all_merged(states: &[crate::infra::gh::State]) -> bool {
    use crate::infra::gh::State;
    states.contains(&State::Merged) && states.iter().all(|text| matches!(text, State::Merged | State::Closed))
}

static LAST: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
static SEEN: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
/// Up with each change of the notes or the seen marks: the views redraw.
static VERSION: AtomicU64 = AtomicU64::new(0);

fn dir() -> PathBuf {
    crate::engine::data_dir().join("notify")
}

fn load_map(name: &str) -> HashMap<String, u64> {
    std::fs::read(dir().join(name))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save_map(name: &str, message: &HashMap<String, u64>) {
    if cfg!(test) {
        return;
    }

    let _ = std::fs::create_dir_all(dir());

    if let Ok(bytes) = serde_json::to_vec(message) {
        let _ = crate::engine::write_atomic(&dir().join(name), &bytes);
    }
}

fn seen() -> &'static Mutex<HashMap<String, u64>> {
    SEEN.get_or_init(|| {
        Mutex::new(if cfg!(test) {
            HashMap::new()
        } else {
            load_map("seen.json")
        })
    })
}

pub(crate) fn version() -> u64 {
    VERSION.load(Ordering::Relaxed)
}

/// The owner saw thread THREAD up to TS (unix ms): its notes up to TS go.
pub(crate) fn mark_seen(thread: &str, ts: u64) {
    let Ok(mut text) = seen().lock() else { return };

    if text.get(thread).is_some_and(|&x| x >= ts) {
        return;
    }

    text.insert(thread.to_string(), ts);
    save_map("seen.json", &text);
    VERSION.fetch_add(1, Ordering::Relaxed);
}

/// The owner saw all of thread THREAD (its stream at the bottom).
pub(crate) fn mark_all_seen(thread: &str) {
    let last = LAST.get().and_then(|message| {
        message
            .lock()
            .ok()
            .and_then(|message_value| message_value.get(thread).copied())
    });

    if let Some(ts) = last {
        mark_seen(thread, ts);
    }
}

/// Unseen notes, most urgent first, then the newest; EXCEPT: the thread in view (0 notes of it).
pub(crate) fn unseen(except: Option<&str>) -> Vec<Note> {
    let text_value = seen().lock().map(|text| text.clone()).unwrap_or_default();
    let mut value: Vec<Note> = NOTES.lock().map(|count| count.clone()).unwrap_or_default();

    // a reply note: unseen until the owner sees the reply in the changes view (0 seen mark)
    value.retain(|count_value| {
        Some(count_value.thread.as_str()) != except
            && (count_value.kind == Kind::Reply
                || text_value
                    .get(&count_value.thread)
                    .is_none_or(|&at| count_value.ts > at))
    });

    value.sort_by(|agent, bytes| agent.kind.cmp(&bytes.kind).then(bytes.ts.cmp(&agent.ts)));
    value
}

/// Unseen notes of thread THREAD (the thread view: the marker while scrolled back).
pub(crate) fn unseen_of(thread: &str) -> Vec<Note> {
    let text_value = seen()
        .lock()
        .ok()
        .and_then(|text| text.get(thread).copied())
        .unwrap_or(0);

    NOTES
        .lock()
        .map(|count| {
            count
                .iter()
                .filter(|count_value| {
                    count_value.thread == thread && count_value.kind != Kind::Reply && count_value.ts > text_value
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Time between 2 PR passes (GitHub sends 0 push to codehead).
const PR_EVERY: Duration = Duration::from_secs(180);

// ---------- header ----------

#[cfg(test)]
#[path = "notifications_tests.rs"]
mod tests;

#[path = "notification_events.rs"]
mod notification_events;
pub(crate) use notification_events::*;

#[path = "notification_worker.rs"]
mod notification_worker;
pub(crate) use notification_worker::*;

struct LogSnapshot {
    stamp: (u64, SystemTime),
    notes: Vec<Note>,
    latest: u64,
}
