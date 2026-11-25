//! The stash of a thread: lines (or whole files) the owner took out of the worktree, each with
//! the patch that puts them back. Not a git stash: a loose list per thread in
//! `<thread dir>/stash.json`. The agent that wrote the lines names an entry (MCP tool
//! `stash_name`); a restore goes to an agent, which reports it done (`stash_restored`).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub id: String,
    /// The agent's name for it; "" until the agent named it.
    #[serde(default)]
    pub name: String,
    pub path: String,
    /// `11-13 · removed 14`, "" for the whole file.
    #[serde(default)]
    pub lines: String,
    /// Applies forward on the worktree: the lines back.
    pub patch: String,
    /// Who names it (wrote the lines).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    pub ts: u64,
    /// A restore went to an agent.
    #[serde(default)]
    pub restoring: bool,
    /// The agent got asked for a name.
    #[serde(default)]
    pub asked: bool,
}

impl Entry {
    /// The name, else `path:lines`.
    pub(crate) fn title(&self) -> String {
        if self.name.is_empty() {
            self.spot()
        } else {
            self.name.clone()
        }
    }

    /// `src/a.rs:11-13`, `src/a.rs` for the whole file.
    pub(crate) fn spot(&self) -> String {
        if self.lines.is_empty() {
            self.path.clone()
        } else {
            format!("{}:{}", self.path, self.lines)
        }
    }
}

fn path(thread: &str) -> PathBuf {
    crate::engine::store::thread_dir(thread).join("stash.json")
}

pub(crate) fn read(thread: &str) -> Vec<Entry> {
    std::fs::read(path(thread))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub(crate) fn write(thread: &str, error: &[Entry]) {
    if let Ok(bytes) = serde_json::to_vec_pretty(error) {
        let _ = crate::engine::write_atomic(&path(thread), &bytes);
    }
}

/// The stash file's change time (the view re-reads on a change: an agent named an entry).
pub(crate) fn stamp(thread: &str) -> Option<std::time::SystemTime> {
    std::fs::metadata(path(thread))
        .and_then(|message| message.modified())
        .ok()
}

/// A new id: ms since the epoch, base 36 (short enough to type).
pub(crate) fn new_id() -> String {
    let mut count = crate::engine::now_ms();
    let mut text = Vec::new();

    while count > 0 {
        text.push(b"0123456789abcdefghijklmnopqrstuvwxyz"[(count % 36) as usize]);
        count /= 36;
    }

    text.reverse();
    String::from_utf8(text).unwrap_or_default()
}

/// The message that asks E's agent for a name (`@agent` first: the thread view routes it).
pub(crate) fn name_ask(error: &Entry) -> String {
    let at = error
        .agent
        .as_ref()
        .map(|agent_value| format!("@{agent_value} "))
        .unwrap_or_default();
    let patch: Vec<&str> = error
        .patch
        .lines()
        .filter(|line| !line.starts_with("diff --git") && !line.starts_with("---") && !line.starts_with("+++"))
        .take(60)
        .collect();

    format!(
        "{at}[codehead stash] The owner took `{}` out of the worktree into the stash (entry `{}`). Name the entry: call the codehead MCP tool `stash_name` with id `{}` and 2-6 words on what these lines do. Nothing else: 0 file changes, 0 restore.\n\n```diff\n{}\n```",
        error.spot(),
        error.id,
        error.id,
        patch.join("\n")
    )
}

/// The message that asks an agent to put E back (`@agent` first: E's agent).
pub(crate) fn restore_ask(error: &Entry) -> String {
    let at = error
        .agent
        .as_ref()
        .map(|agent_value| format!("@{agent_value} "))
        .unwrap_or_default();

    format!(
        "{at}[codehead stash] Restore the stash entry “{}” (`{}`, id `{}`) at its original location: apply the patch below from the worktree root (`git apply`); it fails: `git apply -C1` (its context can hold the spot of an other stash entry). It may not apply cleanly: the code around it changed since. On any conflict or doubt, ask the owner how to resolve it before you change anything; never guess. When the lines are back, call the codehead MCP tool `stash_restored` with id `{}`.\n\n```diff\n{}\n```",
        error.title(),
        error.spot(),
        error.id,
        error.id,
        error.patch.trim_end()
    )
}

/// MCP `stash_name`: entry ID of THREAD gets NAME.
pub(crate) fn set_name(thread: &str, id: &str, name: &str) -> Result<String, String> {
    let name = name
        .trim()
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .trim_matches('"')
        .to_string();

    if name.is_empty() {
        return Err("empty name".into());
    }

    let mut all = read(thread);
    let error_value = all
        .iter_mut()
        .find(|error| error.id == id)
        .ok_or_else(|| format!("0 stash entry {id}"))?;

    error_value.name = name.chars().take(60).collect();
    let done = format!("stash entry {id} named “{}”", error_value.name);

    write(thread, &all);

    Ok(done)
}

/// MCP `stash_restored`: entry ID of THREAD is back in the worktree: it leaves the stash.
pub(crate) fn restored(thread: &str, id: &str) -> Result<String, String> {
    let mut all = read(thread);
    let count = all.len();

    all.retain(|error| error.id != id);

    if all.len() == count {
        return Err(format!("0 stash entry {id}"));
    }

    write(thread, &all);

    Ok(format!("stash entry {id} removed"))
}

#[cfg(test)]
#[path = "stash_tests.rs"]
mod tests;
