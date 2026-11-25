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

fn tail(path: &Path, max_bytes: u64) -> String {
    let Ok(mut file) = std::fs::File::open(path) else {
        return String::new();
    };

    let len = file.metadata().map_or(0, |meta| meta.len());
    let start = len.saturating_sub(max_bytes);

    if file.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }

    let mut buf = Vec::new();
    let _ = file.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).into_owned();

    // 1st line of a cut file is partial
    if start > 0 {
        text.split_once('\n')
            .map(|(_, rest)| rest.to_string())
            .unwrap_or_default()
    } else {
        text
    }
}

fn texts(content: &serde_json::Value) -> Vec<String> {
    match content {
        serde_json::Value::String(text) => vec![text.clone()],
        serde_json::Value::Array(blocks) => blocks
            .iter()
            .filter(|block| block["type"] == "text")
            .filter_map(|block| block["text"].as_str().map(String::from))
            .collect(),
        _ => vec![],
    }
}

/// Fill title, cwd, branch, last response from transcript TEXT (newest lines win).
fn parse(text: &str, session: &mut Session) {
    let mut last: Vec<String> = Vec::new();
    let mut done = false;
    let mut prompt = String::new();

    for line in text.lines().rev() {
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        let kind = entry["type"].as_str().unwrap_or_default();

        if session.cwd.is_empty()
            && let Some(cwd) = entry["cwd"].as_str()
        {
            session.cwd = cwd.to_string();
            session.branch = entry["gitBranch"].as_str().unwrap_or_default().to_string();
        }

        match kind {
            "ai-title" if session.title.is_empty() => {
                session.title = entry["aiTitle"].as_str().unwrap_or_default().to_string();
            }
            "last-prompt" if prompt.is_empty() => {
                prompt = entry["lastPrompt"].as_str().unwrap_or_default().to_string();
            }
            "assistant" if !done && entry["isSidechain"] != true => {
                let mut texts = texts(&entry["message"]["content"]);

                texts.reverse();
                last.extend(texts);
            }
            // a user line (prompt or tool result) ends the final turn's text
            "user" if !done && !last.is_empty() && entry["isSidechain"] != true => done = true,
            _ => {}
        }

        if done && !session.title.is_empty() && !session.cwd.is_empty() {
            break;
        }
    }

    last.reverse();
    session.last = last.join("\n\n").trim().to_string();

    if session.title.is_empty() {
        session.title = prompt.lines().next().unwrap_or_default().to_string();
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
        .filter(|entry| entry.path().is_dir())
        .flat_map(|dir| std::fs::read_dir(dir.path()).into_iter().flatten().flatten())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "jsonl"))
        .filter(|path| {
            path.file_stem()
                .is_some_and(|stem| !owned.contains(&*stem.to_string_lossy()))
        })
        .filter_map(|path| Some((path.metadata().ok()?.modified().ok()?, path)))
        .collect();

    files.sort_by_key(|file| std::cmp::Reverse(file.0));
    cache.retain(|path, _| files.iter().any(|(_, cached)| cached == path));
    let mut out = Vec::new();

    for (mtime, path) in files {
        if let ControlFlow::Break(()) = push_session(limit, &mut cache, &mut out, mtime, path) {
            break;
        }
    }

    out
}

fn push_session(
    limit: usize,
    cache: &mut SessionCache,
    out: &mut Vec<Session>,
    mtime: SystemTime,
    path: PathBuf,
) -> ControlFlow<()> {
    if out.len() >= limit {
        return ControlFlow::Break(());
    }

    let len = path.metadata().map_or(0, |meta| meta.len());

    let parsed = match cache.get(&path) {
        Some((time, size, session)) if *time == mtime && *size == len => session.clone(),
        _ => {
            let mut session = Session {
                id: path.file_stem().unwrap_or_default().to_string_lossy().into_owned(),
                path: path.clone(),
                ..Default::default()
            };

            parse(&tail(&path, TAIL), &mut session);

            if session.last.is_empty() {
                parse(&tail(&path, BIG), &mut session);
            }

            session.repo = crate::domain::threads::repo_root(Path::new(&session.cwd));
            let session = (!session.last.is_empty() && !session.cwd.is_empty()).then_some(session);

            cache.insert(path, (mtime, len, session.clone()));
            session
        }
    };

    if let Some(mut session) = parsed {
        session.when = crate::domain::notes::ago(mtime);
        out.push(session);
    }

    ControlFlow::Continue(())
}

/// Session from a transcript file; None: 0 responses in it.
pub(crate) fn from_transcript(path: &Path) -> Option<Session> {
    let mut session = Session {
        id: path.file_stem()?.to_string_lossy().into_owned(),
        ..Default::default()
    };

    if let Ok(modified) = path.metadata().and_then(|meta| meta.modified()) {
        session.when = crate::domain::notes::ago(modified);
    }

    parse(&tail(path, TAIL), &mut session);

    if session.last.is_empty() {
        parse(&tail(path, BIG), &mut session);
    }

    session.path = path.to_path_buf();

    (!session.last.is_empty()).then_some(session)
}

/// Transcript of session SID under ROOT (`~/.claude/projects`): `ROOT/*/<SID>.jsonl`. A resume in
/// another dir (the thread moved repo) leaves a stale copy behind: the newest file (mtime) wins.
pub(crate) fn session_file(root: &Path, sid: &str) -> Option<PathBuf> {
    let name = format!("{sid}.jsonl");

    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|dir| dir.path().join(&name))
        .filter_map(|path| {
            Some((
                std::fs::metadata(&path)
                    .ok()
                    .filter(std::fs::Metadata::is_file)?
                    .modified()
                    .ok()?,
                path,
            ))
        })
        .max_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, path)| path)
}

/// Transcript of a thread's lead session: ~/.claude/projects/*/<session id>.jsonl.
fn thread_transcript(id: &str) -> Option<PathBuf> {
    let thread = crate::engine::store::load(id)?;
    let session = thread
        .lead()
        .and_then(|lead| thread.sessions.get(lead))
        .or_else(|| thread.sessions.values().next())?;
    let root = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".claude/projects");

    session_file(&root, session)
}

/// Context of a thread: the last response of its lead's Claude session.
pub(crate) fn of_thread(id: &str, title: &str) -> Option<Session> {
    let mut session = from_transcript(&thread_transcript(id)?)?;

    session.thread = Some(id.to_string());
    session.title = title.lines().next().unwrap_or_default().to_string();
    Some(session)
}

/// Thread context block: the session's last response.
pub(crate) fn context(session: &Session) -> String {
    match &session.thread {
        Some(thread) => format!(
            "Context: the last response of codehead thread \"{}\" ({}, {}). Pick up from there:\n\n{}",
            session.title,
            thread.chars().take(12).collect::<String>(),
            transcript_hint(session),
            session.last
        ),
        None => format!(
            "Context: the last response of Claude Code session {} ({}). Pick up from there:\n\n{}",
            session.id,
            transcript_hint(session),
            session.last
        ),
    }
}

impl Session {
    /// Key in the context list: thread id, else session id.
    pub(crate) fn key(&self) -> String {
        self.thread.clone().unwrap_or_else(|| self.id.clone())
    }

    /// Dir name, e.g. `collection`.
    pub(crate) fn place(&self) -> String {
        Path::new(&self.cwd)
            .file_name()
            .map_or_else(|| self.cwd.clone(), |name| name.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
#[path = "claude_tests.rs"]
mod tests;

fn transcript_hint(session: &Session) -> String {
    format!("transcript: {}", session.path.display())
}

type SessionCache = std::collections::HashMap<PathBuf, (SystemTime, u64, Option<Session>)>;
