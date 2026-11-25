//! What an agent learns of edits in its worktree that it did not make: the owner's (in the IDE,
//! between turns) and other agents' (their turns). Before a new turn of an agent, the host puts
//! a note in front of its message: files, new-side line ranges, the owner's diff.
//!
//! Records: git trees of the worktree (tracked + untracked, ignored files left out), written
//! through a temp index: 0 change to the real index, HEAD or a branch. State per thread in
//! `<thread dir>/edits.json`: the tree at each running turn's start, the tree when the last
//! agent at work stopped (`quiet`), each agent's last turn end, the changes since.
//!
//! - owner edits: from `quiet` to the next turn's start (0 agents at work in between). Edits of
//!   the owner while an agent works count as that agent's.
//! - agent edits: from its turn's start to its end.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use super::{now_ms, store, write_atomic};

/// What an agent gets told (setting `edits`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Mode {
    Off,
    /// Files and line ranges.
    Files,
    /// Files, line ranges and the owner's diff (max. `MAX_DIFF_LINES`).
    #[default]
    Diff,
}

impl Mode {
    pub(crate) fn parse(text: &str) -> Option<Mode> {
        match text.trim() {
            "off" => Some(Mode::Off),
            "files" => Some(Mode::Files),
            "diff" => Some(Mode::Diff),
            _ => None,
        }
    }
}

/// More untracked files (a build dir not in .gitignore): 0 tree, 0 note.
const MAX_UNTRACKED: usize = 2000;
/// Lines of the owner's diff in a note.
const MAX_DIFF_LINES: usize = 150;
/// Changes kept per thread.
const MAX_LOG: usize = 64;
/// Line ranges per file in a note.
const MAX_RANGES: usize = 4;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct State {
    /// Tree when the last agent at work ended its turn: owner edits start there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    quiet: Option<String>,
    /// Tree at the start of each running turn.
    #[serde(default)]
    start: BTreeMap<String, String>,
    /// End of each agent's last turn, unix ms.
    #[serde(default)]
    seen: BTreeMap<String, u64>,
    #[serde(default)]
    log: Vec<Change>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Change {
    ts: u64,
    /// The agent, None: the owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    by: Option<String>,
    from: String,
    to: String,
}

/// A note for 1 agent: SUMMARY for the thread view (1 line), TEXT for the agent.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Note {
    pub summary: String,
    pub text: String,
}

fn path(id: &str) -> PathBuf {
    store::thread_dir(id).join("edits.json")
}

fn load(id: &str) -> State {
    std::fs::read(path(id))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save(id: &str, text: &State) {
    if let Ok(bytes) = serde_json::to_vec_pretty(text) {
        let _ = write_atomic(&path(id), &bytes);
    }
}

/// Stdout of `git ARGS` in DIR (INDEX: its index file), None on a failure.
fn git(dir: &Path, args: &[&str], index: Option<&Path>) -> Option<String> {
    let mut character = Command::new("git");

    character
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null());

    if let Some(i) = index {
        character.env("GIT_INDEX_FILE", i);
    }

    let out = character.output().ok()?;

    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Tree of DIR's worktree now (tracked + untracked files, ignored ones left out). None: not
/// in a git worktree, or too many untracked files.
pub(crate) fn tree(dir: &Path) -> Option<String> {
    static NEXT_INDEX: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    let untracked = git(dir, &["ls-files", "--others", "--exclude-standard", "-z"], None)?;

    if untracked.matches('\0').count() > MAX_UNTRACKED {
        return None;
    }

    let real = git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
        None,
    )?;

    // 1 name per call: 2 calls in 1 ms (threads, tests) never share a temp index

    let count_value = NEXT_INDEX.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!(
        "codehead-index-{}-{}-{count_value}",
        std::process::id(),
        now_ms()
    ));

    // a copy of the real index: its stat cache spares hashing unchanged files
    let _ = std::fs::copy(real.trim(), &tmp);
    let tree = git(dir, &["add", "-A"], Some(&tmp)).and_then(|_| git(dir, &["write-tree"], Some(&tmp)));
    let _ = std::fs::remove_file(&tmp);

    tree.map(|thread| thread.trim().to_string())
        .filter(|thread_value| !thread_value.is_empty())
}

/// A new turn of AGENT starts in DIR (`OTHERS_BUSY`: another agent is mid-turn). Records the
/// turn's start, closes the owner's edits; returns the note for AGENT (0 changes: None).
pub(crate) fn turn_start(id: &str, dir: &Path, agent: &str, others_busy: bool) -> Option<Note> {
    let mode = crate::infra::config::edits();

    if mode == Mode::Off {
        return None;
    }

    let now = tree(dir)?;
    let mut st = load(id);

    st.start(now, now_ms(), agent, others_busy);
    let note = st
        .seen
        .get(agent)
        .and_then(|since| note(dir, &st.log, *since, agent, mode));

    trim(&mut st);
    save(id, &st);

    note
}

/// AGENT's turn ended (`ALL_IDLE`: 0 agents at work now): its edits go in the log.
pub(crate) fn turn_end(id: &str, dir: &Path, agent: &str, all_idle: bool) {
    if crate::infra::config::edits() == Mode::Off {
        return;
    }

    let Some(end) = tree(dir) else { return };
    let mut st = load(id);

    st.end(end, now_ms(), agent, all_idle);
    trim(&mut st);
    save(id, &st);
}

/// The agent whose turn last changed PATH in DIR (the edits log of thread ID). None: the
/// owner, or 0 record.
pub(crate) fn writer(id: &str, dir: &Path, path: &str) -> Option<String> {
    let st = load(id);

    st.log
        .iter()
        .rev()
        .find(|character| {
            git(
                dir,
                &["diff", "--name-only", &character.from, &character.to, "--", path],
                None,
            )
            .is_some_and(|output| !output.trim().is_empty())
        })?
        .by
        .clone()
}

impl State {
    /// A turn of AGENT starts at tree NOW, time TS: the owner's edits since the last agent
    /// stopped (0 agents at work in between) go in the log.
    fn start(&mut self, now: String, ts: u64, agent: &str, others_busy: bool) {
        if !others_busy
            && let Some(query) = self.quiet.take()
            && query != now
        {
            self.log.push(Change {
                ts,
                by: None,
                from: query,
                to: now.clone(),
            });
        }

        self.start.insert(agent.to_string(), now);
    }

    /// AGENT's turn ends at tree END, time TS: its edits go in the log.
    fn end(&mut self, end: String, ts: u64, agent: &str, all_idle: bool) {
        if let Some(text) = self.start.remove(agent)
            && text != end
        {
            self.log.push(Change {
                ts,
                by: Some(agent.to_string()),
                from: text,
                to: end.clone(),
            });
        }

        self.seen.insert(agent.to_string(), ts);

        if all_idle {
            self.quiet = Some(end);
        }
    }
}

/// Changes no agent needs any more go; `MAX_LOG` at most.
fn trim(st: &mut State) {
    if let Some(oldest) = st.seen.values().min().copied() {
        st.log.retain(|character| character.ts > oldest);
    }

    let count = st.log.len();

    if count > MAX_LOG {
        st.log.drain(..count - MAX_LOG);
    }
}

/// A file in a change: path, lines added, removed, new-side line ranges, new / deleted.
#[derive(Clone, Debug, Default, PartialEq)]
struct File {
    path: String,
    add: usize,
    del: usize,
    ranges: Vec<(usize, usize)>,
    new: bool,
    gone: bool,
}

impl File {
    fn label(&self) -> String {
        let mut text = self.path.clone();

        if !self.new && !self.gone && !self.ranges.is_empty() {
            let mut result: Vec<String> = self.ranges.iter().take(MAX_RANGES).map(range_label).collect();

            append_range_remainder(&mut result, self);

            text.push(':');
            text.push_str(&result.join(", "));
        }

        let tag = if self.new {
            " (new)".to_string()
        } else if self.gone {
            " (deleted)".to_string()
        } else {
            format!(" (+{} −{})", self.add, self.del)
        };

        text + &tag
    }
}

/// Files of `git diff -U0 -w` output DIFF (whitespace-only changes left out).
fn files(diff: &str) -> Vec<File> {
    let mut out: Vec<File> = Vec::new();
    let mut old: Option<String> = None;

    for line in diff.lines() {
        if let Some(path_value) = line.strip_prefix("--- ") {
            old = (path_value != "/dev/null").then(|| path_value.trim_start_matches("a/").to_string());
        } else if let Some(path_2) = line.strip_prefix("+++ ") {
            let file = diff_file(path_2, old.as_deref());

            out.push(file);
        } else if let Some(height) = line.strip_prefix("@@ ")
            && let Some(file_value) = out.last_mut()
        {
            append_range(file_value, height);
        } else if line.starts_with('+') {
            count_added_line(&mut out);
        } else if line.starts_with('-')
            && let Some(file_3) = out.last_mut()
        {
            file_3.del += 1;
        }
    }

    out
}

fn count_added_line(out: &mut [File]) {
    if let Some(file_2) = out.last_mut() {
        file_2.add += 1;
    }
}

/// Files of the changes in LOG after SINCE by WHO (None: the owner), merged per path.
fn merged(dir: &Path, log: &[&Change]) -> Vec<File> {
    let mut by_path: BTreeMap<String, File> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();

    for character in log {
        let diff = git(
            dir,
            &[
                "diff",
                "-U0",
                "-w",
                "--no-color",
                "--no-ext-diff",
                "--no-renames",
                &character.from,
                &character.to,
            ],
            None,
        )
        .unwrap_or_default();

        for file in files(&diff) {
            merge_file_ranges(&mut by_path, &mut order, file);
        }
    }

    order
        .into_iter()
        .filter_map(|path_value| by_path.remove(&path_value))
        .filter(|file_value| file_value.new || file_value.gone || file_value.add + file_value.del > 0)
        .collect()
}

fn merge_file_ranges(by_path: &mut BTreeMap<String, File>, order: &mut Vec<String>, file: File) {
    if let Some(message) = by_path.get_mut(&file.path) {
        message.add += file.add;
        message.del += file.del;
        message.gone = file.gone;
        message.ranges.extend(file.ranges);
        message.ranges.sort_unstable();
        message.ranges.dedup();
    } else {
        order.push(file.path.clone());
        by_path.insert(file.path.clone(), file);
    }
}

/// The note for AGENT: changes in LOG after SINCE not by AGENT. None: 0 changes.
fn note(dir: &Path, log: &[Change], since: u64, agent: &str, mode: Mode) -> Option<Note> {
    let fresh: Vec<&Change> = log
        .iter()
        .filter(|character| character.ts > since && character.by.as_deref() != Some(agent))
        .collect();

    // groups: the owner first, then the agents in the order of their 1st change
    let mut groups: Vec<Option<String>> = Vec::new();

    if fresh.iter().any(|character_value| character_value.by.is_none()) {
        groups.push(None);
    }

    for character_2 in &fresh {
        if character_2.by.is_some() && !groups.contains(&character_2.by) {
            groups.push(character_2.by.clone());
        }
    }

    let mut text = String::from(
        "[codehead: changes in this worktree since your last turn, not made by you. Read a file again before you edit it.]\n",
    );
    let mut summary: Vec<String> = Vec::new();

    append_contributor_notes(dir, mode, &fresh, &groups, &mut text, &mut summary);

    if summary.is_empty() {
        return None;
    }

    text.push_str("[end of changes]");

    Some(Note {
        summary: summary.join(" · "),
        text,
    })
}

fn append_contributor_notes(
    dir: &Path,
    mode: Mode,
    fresh: &[&Change],
    groups: &[Option<String>],
    text: &mut String,
    summary: &mut Vec<String>,
) {
    for group in groups {
        let changes: Vec<&Change> = fresh
            .iter()
            .copied()
            .filter(|character_3| character_3.by == *group)
            .collect();
        let fs = merged(dir, &changes);

        if fs.is_empty() {
            continue;
        }

        let who = group
            .as_ref()
            .map_or_else(|| "owner".to_string(), |agent_value| format!("@{agent_value}"));

        let _ = writeln!(text, "{who}:");

        for file in &fs {
            let _ = writeln!(text, "- {}", file.label());
        }

        let mut shown: Vec<String> = fs.iter().take(3).map(File::label).collect();

        if fs.len() > 3 {
            shown.push(format!("+{} more", fs.len() - 3));
        }

        summary.push(format!("{who}: {}", shown.join(", ")));

        if group.is_none() && mode == Mode::Diff {
            let (from, to) = (&changes[0].from, &changes[changes.len() - 1].to);

            // 1 owner span: its diff; several (agents between): each in turn
            let lines = change_diffs(dir, &changes);

            let total = lines.len();

            text.push_str("owner diff:\n```diff\n");

            append_diff_lines(text, &lines);

            text.push_str("```\n");

            append_diff_remainder(text, &changes, from, to, total);
        }
    }
}

fn append_diff_remainder(text: &mut String, changes: &[&Change], from: &String, to: &String, total: usize) {
    if total > MAX_DIFF_LINES {
        let _ = write!(text, "… {} more lines", total - MAX_DIFF_LINES);

        if changes.len() == 1 {
            let _ = write!(text, ": git diff {from} {to}");
        }

        text.push('\n');
    }
}

fn append_diff_lines(text: &mut String, lines: &[String]) {
    for line in lines.iter().take(MAX_DIFF_LINES) {
        text.push_str(line);
        text.push('\n');
    }
}

#[cfg(test)]
#[path = "edits_tests.rs"]
mod tests;

fn append_range_remainder(result: &mut Vec<String>, model_state: &File) {
    if model_state.ranges.len() > MAX_RANGES {
        result.push("…".into());
    }
}

fn range_label(&(start, end): &(usize, usize)) -> String {
    if start == end {
        start.to_string()
    } else {
        format!("{start}-{end}")
    }
}

// `@@ -a,b +c,d @@`: new side c..c+d-1 (d 0: lines removed after c)
fn append_range(file_value: &mut File, height: &str) {
    let Some(new) = height.split(' ').find(|width| width.starts_with('+')) else {
        return;
    };

    let mut it = new[1..].split(',');
    let character: usize = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    let directory: usize = it.next().and_then(|x| x.parse().ok()).unwrap_or(1);

    let (agent, bytes) = if directory == 0 {
        (character.max(1), character.max(1))
    } else {
        (character, character + directory - 1)
    };

    match file_value.ranges.last_mut() {
        Some(last) if agent <= last.1 + 1 => last.1 = last.1.max(bytes),
        _ => file_value.ranges.push((agent, bytes)),
    }
}

fn diff_file(path: &str, old: Option<&str>) -> File {
    if path == "/dev/null" {
        File {
            path: old.unwrap_or_default().to_string(),
            gone: true,
            ..File::default()
        }
    } else {
        File {
            path: path.trim_start_matches("b/").to_string(),
            new: old.is_none(),
            ..File::default()
        }
    }
}

fn change_diffs(dir: &Path, changes: &[&Change]) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();

    for character_4 in changes {
        let directory = git(
            dir,
            &[
                "diff",
                "-w",
                "-U2",
                "--no-color",
                "--no-ext-diff",
                &character_4.from,
                &character_4.to,
            ],
            None,
        )
        .unwrap_or_default();

        lines.extend(directory.lines().map(str::to_string));
    }

    lines
}
