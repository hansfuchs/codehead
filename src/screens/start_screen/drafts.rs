//! Drafts of the task screen: 1 JSON file per unsent task in ~/.cache/codehead/drafts ($`CODEHEAD_DRAFTS`),
//! written after every key that changed it. A started task (or an emptied one) drops its file.
//! The task screen lists the other drafts ("view drafts (n)"): load 1, or delete it. ctrl+s keeps a
//! draft on purpose (`kept`): it stays past `KEEP_DAYS`, the form starts over empty.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// Drafts older than this go at start (kept ones stay).
const KEEP_DAYS: u64 = 30;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Draft {
    pub id: String,
    /// Seconds since the epoch of the last write.
    pub saved: u64,
    /// Repo (else dir) name the task was written in.
    pub place: String,
    pub title: String,
    pub prompt: String,
    /// Picked notes (relative to the notes root).
    pub notes: Vec<String>,
    /// Picked agents.
    pub picks: Vec<String>,
    /// The where row's choice (its debug text), checkout / stash boxes.
    pub place_choice: String,
    pub checkout: bool,
    pub stash: bool,
    /// Pasted files: marker, path.
    pub files: Vec<(String, String)>,
    /// Context: thread id (None: a Claude Code session), transcript path, title.
    pub carry: Vec<(Option<String>, String, String)>,
    /// Saved on purpose (ctrl+s): 0 cleanup by age.
    pub kept: bool,
}

pub(crate) fn dir() -> PathBuf {
    std::env::var_os("CODEHEAD_DRAFTS")
        .filter(|dir| !dir.is_empty())
        .map_or_else(
            || crate::infra::config::home().join(".cache/codehead/drafts"),
            PathBuf::from,
        )
}

/// Id of a new draft: start time (sorts by age) + process + a count (2 tasks of 1 process).
pub(crate) fn new_id() -> String {
    static NEXT_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    format!(
        "{}-{}-{}",
        (crate::engine::now_ms() / 1000),
        std::process::id(),
        NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| items.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

impl Draft {
    /// Nothing a person wrote: 0 file for it.
    pub(crate) fn is_empty(&self) -> bool {
        self.title.trim().is_empty() && self.prompt.trim().is_empty()
    }

    /// Name in the list: the title, else the 1st line of the prompt.
    pub(crate) fn name(&self) -> String {
        let title = self.title.trim();

        let name = if title.is_empty() {
            self.prompt
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or_default()
                .trim()
        } else {
            title
        };

        name.to_string()
    }

    /// The content (0 id, 0 time): 2 drafts with the same text need 0 new write.
    pub(crate) fn content(&self) -> String {
        json!({
            "place": self.place,
            "title": self.title,
            "prompt": self.prompt,
            "notes": self.notes,
            "picks": self.picks,
            "where": self.place_choice,
            "checkout": self.checkout,
            "stash": self.stash,
            "files": self.files.iter().map(|(marker, path)| json!([marker, path])).collect::<Vec<_>>(),
            "carry": self.carry.iter().map(|(thread, path, title)| json!({"thread": thread, "path": path, "title": title})).collect::<Vec<_>>(),
            "kept": self.kept,
        })
        .to_string()
    }

    fn parse(id: &str, text: &str) -> Option<Draft> {
        let value: Value = serde_json::from_str(text).ok()?;
        let field = |key: &str| value[key].as_str().unwrap_or_default().to_string();
        let pair = |x: &Value| Some((x[0].as_str()?.to_string(), x[1].as_str()?.to_string()));

        Some(Draft {
            id: id.to_string(),
            saved: value["saved"].as_u64().unwrap_or(0),
            place: field("place"),
            title: field("title"),
            prompt: field("prompt"),
            notes: strings(&value["notes"]),
            picks: strings(&value["picks"]),
            place_choice: field("where"),
            checkout: value["checkout"].as_bool().unwrap_or(true),
            stash: value["stash"].as_bool().unwrap_or(false),
            files: value["files"]
                .as_array()
                .map(|pairs| pairs.iter().filter_map(pair).collect())
                .unwrap_or_default(),
            carry: value["carry"]
                .as_array()
                .map(|values| parse_carry(values))
                .unwrap_or_default(),
            kept: value["kept"].as_bool().unwrap_or(false),
        })
    }
}

fn parse_carry(values: &[Value]) -> Vec<(Option<String>, String, String)> {
    values
        .iter()
        .map(|entry| {
            (
                entry["thread"].as_str().map(String::from),
                entry["path"].as_str().unwrap_or_default().to_string(),
                entry["title"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

fn file(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

/// Write DRAFT (a temp file, then a rename: a crash mid-write keeps the last full draft).
pub(crate) fn save(dir: &Path, draft: &Draft) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let mut value: Value = serde_json::from_str(&draft.content()).map_err(|error| error.to_string())?;

    value["saved"] = json!((crate::engine::now_ms() / 1000));
    let tmp = dir.join(format!(".{}.tmp", draft.id));

    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;

    std::fs::rename(&tmp, file(dir, &draft.id)).map_err(|error| error.to_string())
}

pub(crate) fn remove(dir: &Path, id: &str) {
    let _ = std::fs::remove_file(file(dir, id));
}

pub(crate) fn path(dir: &Path, id: &str) -> PathBuf {
    file(dir, id)
}

/// Every draft but SKIP (the open task's own), oldest first. Drafts over `KEEP_DAYS` old: removed.
pub(crate) fn load(dir: &Path, skip: &str) -> Vec<Draft> {
    let now = crate::engine::now_ms() / 1000;
    let mut found = Vec::new();

    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();

        if path.extension().is_none_or(|x| x != "json") {
            continue;
        }

        let Some(id) = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()) else {
            continue;
        };

        let Some(draft) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|content| Draft::parse(&id, &content))
        else {
            continue;
        };

        if !draft.kept && draft.saved > 0 && now.saturating_sub(draft.saved) > KEEP_DAYS * 86400 {
            let _ = std::fs::remove_file(&path);

            continue;
        }

        if id != skip && !draft.is_empty() {
            found.push(draft);
        }
    }

    found.sort_by(|left, right| (left.saved, &left.id).cmp(&(right.saved, &right.id)));
    found
}

#[cfg(test)]
#[path = "start_screen_draft_store_tests.rs"]
mod tests;
