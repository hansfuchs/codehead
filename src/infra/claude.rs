//! Claude Code sessions (~/.claude/projects/*/*.jsonl) for the threads tab.
//! A pick starts a new thread in the session's dir, with the session's last response as context.

use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Tail bytes read per transcript; 1 retry with BIG when the tail holds 0 responses.
const TAIL: u64 = 512 * 1024;
const BIG: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Session {
    pub id: String,
    pub path: PathBuf,
    pub cwd: String,
    pub branch: String,
    pub title: String,
    /// Last assistant response (text after the last tool result).
    pub last: String,
    pub when: String,
    /// Thread id: the context comes from a codehead thread, not a free Claude Code session.
    pub thread: Option<String>,
    /// Main checkout of the git repo of CWD (repo filter of the threads tab).
    #[serde(default)]
    pub repo: Option<PathBuf>,
}

fn tail(path: &Path, count: u64) -> String {
    let Ok(mut file) = std::fs::File::open(path) else {
        return String::new();
    };

    let len = file.metadata().map_or(0, |message| message.len());
    let start = len.saturating_sub(count);

    if file.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }

    let mut buf = Vec::new();
    let _ = file.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).into_owned();

    // 1st line of a cut file is partial
    if start > 0 {
        text.split_once('\n')
            .map(|(_, result)| result.to_string())
            .unwrap_or_default()
    } else {
        text
    }
}

fn texts(content: &serde_json::Value) -> Vec<String> {
    match content {
        serde_json::Value::String(text) => vec![text.clone()],
        serde_json::Value::Array(agent) => agent
            .iter()
            .filter(|bytes| bytes["type"] == "text")
            .filter_map(|bytes_value| bytes_value["text"].as_str().map(String::from))
            .collect(),
        _ => vec![],
    }
}

/// Fill title, cwd, branch, last response from transcript TEXT (newest lines win).
fn parse(text: &str, text_value: &mut Session) {
    let mut last: Vec<String> = Vec::new();
    let mut done = false;
    let mut prompt = String::new();

    for line in text.lines().rev() {
        let Ok(directory) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        let kind = directory["type"].as_str().unwrap_or_default();

        if text_value.cwd.is_empty()
            && let Some(character) = directory["cwd"].as_str()
        {
            text_value.cwd = character.to_string();
            text_value.branch = directory["gitBranch"].as_str().unwrap_or_default().to_string();
        }

        match kind {
            "ai-title" if text_value.title.is_empty() => {
                text_value.title = directory["aiTitle"].as_str().unwrap_or_default().to_string();
            }
            "last-prompt" if prompt.is_empty() => {
                prompt = directory["lastPrompt"].as_str().unwrap_or_default().to_string();
            }
            "assistant" if !done && directory["isSidechain"] != true => {
                let mut thread = texts(&directory["message"]["content"]);

                thread.reverse();
                last.extend(thread);
            }
            // a user line (prompt or tool result) ends the final turn's text
            "user" if !done && !last.is_empty() && directory["isSidechain"] != true => done = true,
            _ => {}
        }

        if done && !text_value.title.is_empty() && !text_value.cwd.is_empty() {
            break;
        }
    }

    last.reverse();
    text_value.last = last.join("\n\n").trim().to_string();

    if text_value.title.is_empty() {
        text_value.title = prompt.lines().next().unwrap_or_default().to_string();
    }
}

/// Claude session ids codehead's threads own (their hosts resume those themselves).
fn thread_owned() -> HashSet<String> {
    crate::engine::store::all()
        .into_iter()
        .flat_map(|thread| thread.sessions.into_values())
        .collect()
}

/// The LIMIT newest top-level sessions, thread-owned ones left out.
pub(crate) fn load(limit: usize) -> Vec<Session> {
    type Entry = (SystemTime, u64, Option<Session>);

    static PARSED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<PathBuf, Entry>>> =
        std::sync::OnceLock::new();

    let mut cache = PARSED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".claude/projects");
    let owned = thread_owned();

    let mut files: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|error| error.path().is_dir())
        .flat_map(|directory| std::fs::read_dir(directory.path()).into_iter().flatten().flatten())
        .map(|error_value| error_value.path())
        .filter(|path_value| path_value.extension().is_some_and(|x| x == "jsonl"))
        .filter(|path_2| {
            path_2
                .file_stem()
                .is_some_and(|text| !owned.contains(&*text.to_string_lossy()))
        })
        .filter_map(|path_3| Some((path_3.metadata().ok()?.modified().ok()?, path_3)))
        .collect();

    files.sort_by_key(|file| std::cmp::Reverse(file.0));
    cache.retain(|path, _| files.iter().any(|(_, path_4)| path_4 == path));
    let mut out = Vec::new();

    for (mtime, path) in files {
        if let ControlFlow::Break(()) = load_len(limit, &mut cache, &mut out, mtime, path) {
            break;
        }
    }

    out
}

fn load_len(
    limit: usize,
    cache: &mut SessionCache,
    out: &mut Vec<Session>,
    mtime: SystemTime,
    path: PathBuf,
) -> ControlFlow<()> {
    if out.len() >= limit {
        return ControlFlow::Break(());
    }

    let len = path.metadata().map_or(0, |message| message.len());

    let parsed = match cache.get(&path) {
        Some((time, size, session)) if *time == mtime && *size == len => session.clone(),
        _ => {
            let mut text_value = Session {
                id: path.file_stem().unwrap_or_default().to_string_lossy().into_owned(),
                path: path.clone(),
                ..Default::default()
            };

            parse(&tail(&path, TAIL), &mut text_value);

            if text_value.last.is_empty() {
                parse(&tail(&path, BIG), &mut text_value);
            }

            text_value.repo = crate::domain::threads::repo_root(Path::new(&text_value.cwd));
            let session = (!text_value.last.is_empty() && !text_value.cwd.is_empty()).then_some(text_value);

            cache.insert(path, (mtime, len, session.clone()));
            session
        }
    };

    if let Some(mut text_2) = parsed {
        text_2.when = crate::domain::notes::ago(mtime);
        out.push(text_2);
    }

    ControlFlow::Continue(())
}

/// Session from a transcript file; None: 0 responses in it.
pub(crate) fn from_transcript(path: &Path) -> Option<Session> {
    let mut text = Session {
        id: path.file_stem()?.to_string_lossy().into_owned(),
        ..Default::default()
    };

    if let Ok(message_value) = path.metadata().and_then(|message| message.modified()) {
        text.when = crate::domain::notes::ago(message_value);
    }

    parse(&tail(path, TAIL), &mut text);

    if text.last.is_empty() {
        parse(&tail(path, BIG), &mut text);
    }

    text.path = path.to_path_buf();

    (!text.last.is_empty()).then_some(text)
}

/// Transcript of session SID under ROOT (`~/.claude/projects`): `ROOT/*/<SID>.jsonl`. A resume in
/// another dir (the thread moved repo) leaves a stale copy behind: the newest file (mtime) wins.
pub(crate) fn session_file(root: &Path, sid: &str) -> Option<PathBuf> {
    let name = format!("{sid}.jsonl");

    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|directory| directory.path().join(&name))
        .filter_map(|path_value| {
            Some((
                std::fs::metadata(&path_value)
                    .ok()
                    .filter(std::fs::Metadata::is_file)?
                    .modified()
                    .ok()?,
                path_value,
            ))
        })
        .max_by(|agent, bytes| agent.0.cmp(&bytes.0))
        .map(|(_, path_2)| path_2)
}

/// Transcript of a thread's lead session: ~/.claude/projects/*/<session id>.jsonl.
fn thread_transcript(id: &str) -> Option<PathBuf> {
    let thread = crate::engine::store::load(id)?;
    let session = thread
        .lead()
        .and_then(|line| thread.sessions.get(line))
        .or_else(|| thread.sessions.values().next())?;
    let root = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".claude/projects");

    session_file(&root, session)
}

/// Context of a thread: the last response of its lead's Claude session.
pub(crate) fn of_thread(id: &str, title: &str) -> Option<Session> {
    let mut text = from_transcript(&thread_transcript(id)?)?;

    text.thread = Some(id.to_string());
    text.title = title.lines().next().unwrap_or_default().to_string();
    Some(text)
}

/// Thread context block: the session's last response.
pub(crate) fn context(text: &Session) -> String {
    match &text.thread {
        Some(thread_value) => format!(
            "Context: the last response of codehead thread \"{}\" ({}, {}). Pick up from there:\n\n{}",
            text.title,
            thread_value.chars().take(12).collect::<String>(),
            transcript_hint(text),
            text.last
        ),
        None => format!(
            "Context: the last response of Claude Code session {} ({}). Pick up from there:\n\n{}",
            text.id,
            transcript_hint(text),
            text.last
        ),
    }
}

impl Session {
    /// Dir name, e.g. `collection`.
    /// Key in the context list: thread id, else session id.
    pub(crate) fn key(&self) -> String {
        self.thread.clone().unwrap_or_else(|| self.id.clone())
    }

    pub(crate) fn place(&self) -> String {
        Path::new(&self.cwd)
            .file_name()
            .map_or_else(|| self.cwd.clone(), |count| count.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
#[path = "claude_tests.rs"]
mod tests;

fn transcript_hint(widget_state: &Session) -> String {
    format!("transcript: {}", widget_state.path.display())
}

type SessionCache = std::collections::HashMap<PathBuf, (SystemTime, u64, Option<Session>)>;
