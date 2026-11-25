//! Thread cards: the thread header of the thread view and the threads tab of the F2 menu.
//! Sources: the thread records, the hosts (agents at work), the event logs (last agent event).
//! Unread: an agent event newer than the last time codehead showed the thread (~/.cache/codehead/seen.json).

use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Card {
    pub id: String,
    pub title: String,
    pub state: String,
    #[serde(flatten)]
    pub agents: AgentActivity,
    #[serde(flatten)]
    pub completion: Completion,
    pub unread: bool,
    /// Last activity, unix ms: the last agent item, else the thread's update time.
    pub activity: u64,
    /// Worktree (else project dir) of the thread: the thread view works there.
    pub dir: Option<PathBuf>,
    pub branch: String,
    /// Main checkout of the thread's git repo (repo filter of the threads tab).
    pub repo: Option<PathBuf>,
    /// The prompt holds an unsent message (the thread view's draft on disk).
    pub draft: bool,
    /// Work not in the base yet (open) or 0 left (shipped: ready to archive). None: 0 read yet.
    #[serde(default)]
    pub land: Option<crate::domain::landed::Land>,
    /// DIR is a linked worktree codehead made (delete / archive remove it).
    #[serde(default)]
    pub worktree: bool,
    /// Projects of the thread (slugs of its repo's projects).
    #[serde(default)]
    pub projects: Vec<String>,
}

impl Card {
    pub(crate) fn archived(&self) -> bool {
        self.state == "archived"
    }

    /// 0 work left to land: the branch landed / abandoned, or 0 code and every agent done (a new
    /// thread has 0 code too: not shipped). On main: never; a step to review: not yet.
    pub(crate) fn shipped(&self) -> bool {
        use crate::domain::landed::Land;

        if self.completion.review {
            return false;
        }

        match &self.land {
            Some(Land::Landed | Land::Abandoned(_)) => true,
            Some(Land::NoCode) => self.completion.done,
            _ => false,
        }
    }

    /// 0 code, agents not done, idle 3 days at NOW (unix ms): the owner decides.
    pub(crate) fn stale(&self, now: u64) -> bool {
        crate::domain::landed::stale(self.land.as_ref(), self.completion.done, self.activity, now)
    }
}

impl Card {
    /// 1st line of the title, "untitled" for none.
    pub(crate) fn name(&self) -> String {
        let thread = self.title.lines().next().unwrap_or_default().trim();

        if thread.is_empty() {
            "untitled".into()
        } else {
            thread.to_string()
        }
    }
}

fn seen_path() -> PathBuf {
    crate::infra::config::home().join(".cache/codehead/seen.json")
}

fn read_seen() -> HashMap<String, u64> {
    std::fs::read_to_string(seen_path())
        .ok()
        .and_then(|thread| serde_json::from_str(&thread).ok())
        .unwrap_or_default()
}

fn write_seen(seen: &HashMap<String, u64>) {
    let path = seen_path();

    if let Some(directory) = path.parent() {
        let _ = std::fs::create_dir_all(directory);
    }

    if let Ok(thread) = serde_json::to_string(seen) {
        let tmp = path.with_extension("tmp");

        if std::fs::write(&tmp, thread).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |directory| directory.as_millis() as u64)
}

/// Thread ID shown now: its agent items so far are read.
pub(crate) fn mark_seen(id: &str) {
    mark_seen_at(id, false);
}

/// The view ends: its thread is read up to now, 0 throttle.
pub(crate) fn mark_seen_now(id: &str) {
    mark_seen_at(id, true);
}

fn mark_seen_at(id: &str, force: bool) {
    if id.is_empty() {
        return;
    }

    let mut seen = read_seen();
    let now = now_ms();

    // the thread view marks every 3s: 1 write per 15s is enough (its last mark comes at its end)
    if !force && seen.get(id).is_some_and(|thread| now.saturating_sub(*thread) < 15_000) {
        return;
    }

    seen.insert(id.to_string(), now);
    write_seen(&seen);
}

/// Thread ID has an unsent message: its prompt draft (`prompt.json`, written by the thread view
/// after each key, gone once empty) has text, or a send the host has not taken yet.
pub(crate) fn has_draft(id: &str) -> bool {
    let Ok(raw) = std::fs::read_to_string(crate::engine::store::thread_dir(id).join("prompt.json")) else {
        return false;
    };

    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return false;
    };

    let text = value["text"].as_str().is_some_and(|thread| !thread.trim().is_empty());
    let unsent = value["unsent"].as_array().is_some_and(|user| !user.is_empty());

    text || unsent
}

/// Time of the last agent event of thread ID (its text, error, question), unix ms. Reads
/// only the tail of the log.
pub(crate) fn last_item(id: &str) -> Option<u64> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(crate::engine::store::events_path(id)).ok()?;
    let len = file.metadata().ok()?.len();

    file.seek(SeekFrom::Start(len.saturating_sub(64 << 10))).ok()?;
    let mut buf = String::new();

    file.read_to_string(&mut buf).ok()?;

    buf.lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<crate::engine::store::Event>(line).ok())
        .find_map(|error| {
            use crate::engine::store::Kind;
            matches!(error.kind, Kind::Text { .. } | Kind::Error { .. } | Kind::Ask { .. }).then_some(error.ts)
        })
}

/// Every thread (archived ones too: `Card::archived`), newest activity first.
pub(crate) fn load() -> Vec<Card> {
    let threads = crate::domain::threads::all_threads_lite();
    let mut seen = read_seen();
    let mut new_seen = false;

    let mut cards: Vec<Card> = threads
        .into_iter()
        .filter(|thread| thread.state != "deleted")
        .map(|path_value| {
            let last = last_item(&path_value.id);

            let unread = match (last, seen.get(&path_value.id)) {
                (Some(at), Some(text)) => at > *text,
                // a thread codehead never showed: its items so far count as read
                (Some(at), None) => {
                    seen.insert(path_value.id.clone(), at);
                    new_seen = true;
                    false
                }
                (None, _) => false,
            };

            let dir = [path_value.worktree.clone(), path_value.dir.clone()]
                .into_iter()
                .flatten()
                .map(PathBuf::from)
                .find(|path_2| path_2.is_dir());

            Card {
                repo: crate::domain::threads::repo_of_thread(&path_value),
                agents: crate::domain::thread_card::AgentActivity {
                    busy: path_value.busy,
                    monitoring: path_value.watching,
                    blocked: path_value.needs.as_deref() == Some("question"),
                    live: path_value.live,
                },

                unread,
                activity: last.unwrap_or(0).max(path_value.updated * 1000),
                dir,
                branch: path_value.branch.clone().unwrap_or_default(),
                draft: has_draft(&path_value.id),
                land: crate::domain::landed::of(&path_value.id),
                completion: crate::domain::thread_card::Completion {
                    done: crate::domain::notifications::reported_done(&path_value.id),
                    review: crate::domain::notifications::reported_review(&path_value.id),
                },

                worktree: path_value
                    .worktree
                    .as_deref()
                    .is_some_and(|path_3| std::path::Path::new(path_3).is_dir()),
                projects: path_value.projects,
                id: path_value.id,
                title: path_value.title,
                state: path_value.state,
            }
        })
        .collect();

    if new_seen {
        write_seen(&seen);
    }

    cards.sort_by_key(|character| std::cmp::Reverse(character.activity));

    cards
}

/// Fuzzy filter (title, branch) of CARDS: best match first; empty Q: all, order kept.
pub(crate) fn filter(cards: &[Card], query: &str) -> Vec<usize> {
    // `kind:fix`, `area:menu`, `project:slug`: exact filters; the other words: the fuzzy match
    let mut words = Vec::new();
    let mut keep = Vec::new();

    for word in query.split_whitespace() {
        if let Some(filter) = CardFilter::parse(word) {
            keep.push(filter);
        } else {
            words.push(word);
        }
    }

    let query_value = words.join(" ");
    let passes = |character_3: &Card| keep.iter().all(|filter| filter.matches(character_3));

    if query_value.is_empty() {
        return (0..cards.len()).filter(|&i| passes(&cards[i])).collect();
    }

    let mut hits: Vec<(i64, usize)> = cards
        .iter()
        .enumerate()
        .filter(|(_, character_4)| passes(character_4))
        .filter_map(|(i, character_5)| {
            crate::domain::fuzzy::fuzzy(&query_value, &format!("{} {}", character_5.name(), character_5.branch))
                .map(|text| (text, i))
        })
        .collect();

    hits.sort_by_key(|(text_value, i)| (std::cmp::Reverse(*text_value), *i));

    hits.into_iter().map(|(_, i)| i).collect()
}

enum CardFilter {
    Kind(String),
    Area(String),
    Project(String),
}

impl CardFilter {
    fn parse(word: &str) -> Option<Self> {
        let (key, value) = word.split_once(':')?;

        if value.is_empty() {
            return None;
        }

        match key {
            "kind" => Some(Self::Kind(value.to_lowercase())),
            "area" => Some(Self::Area(value.to_lowercase())),
            "project" => Some(Self::Project(value.to_lowercase())),
            _ => None,
        }
    }

    fn matches(&self, card: &Card) -> bool {
        match self {
            Self::Kind(kind) => crate::domain::projects::kind_of(&card.branch) == Some(kind.as_str()),
            Self::Area(area) => crate::domain::projects::area_of(&card.title).as_deref() == Some(area.as_str()),
            Self::Project(project) => card.projects.iter().any(|slug| slug.starts_with(project)),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AgentActivity {
    /// Live sessions.
    pub live: usize,
    /// An agent works (session running / starting, or a Claude transcript mid-turn).
    pub busy: bool,
    /// BUSY only by a monitor or background command: `monitoring…`, not `working…`.
    #[serde(default)]
    pub monitoring: bool,
    /// A question (an ask) waits on the owner: blocked, also while its turn runs.
    pub blocked: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Completion {
    /// Every agent on the thread reported done (0 turn since).
    #[serde(default)]
    pub done: bool,
    /// An agent finished a step, the next one waits on the owner (`set_status` review).
    #[serde(default)]
    pub review: bool,
}

#[cfg(test)]
#[path = "thread_card_serialization_tests.rs"]
mod serialization_tests;
