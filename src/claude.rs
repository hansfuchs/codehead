//! Claude Code sessions (~/.claude/projects/*/*.jsonl) for the threads tab.
//! A pick starts a new thread in the session's dir, with the session's last response as context.

use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ratatui::text::{Line, Span};

use crate::theme;

/// Tail bytes read per transcript; 1 retry with BIG when the tail holds 0 responses.
const TAIL: u64 = 512 * 1024;
const BIG: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Session {
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
}

fn tail(path: &Path, n: u64) -> String {
    let Ok(mut f) = std::fs::File::open(path) else { return String::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(n);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).into_owned();
    // 1st line of a cut file is partial
    if start > 0 { text.split_once('\n').map(|(_, r)| r.to_string()).unwrap_or_default() } else { text }
}

fn texts(content: &serde_json::Value) -> Vec<String> {
    match content {
        serde_json::Value::String(s) => vec![s.clone()],
        serde_json::Value::Array(a) => a.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str().map(String::from)).collect(),
        _ => vec![],
    }
}

/// Fill title, cwd, branch, last response from transcript TEXT (newest lines win).
fn parse(text: &str, s: &mut Session) {
    let mut last: Vec<String> = Vec::new();
    let mut done = false;
    let mut prompt = String::new();
    for line in text.lines().rev() {
        let Ok(d) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let kind = d["type"].as_str().unwrap_or_default();
        if s.cwd.is_empty()
            && let Some(c) = d["cwd"].as_str()
        {
            s.cwd = c.to_string();
            s.branch = d["gitBranch"].as_str().unwrap_or_default().to_string();
        }
        match kind {
            "ai-title" if s.title.is_empty() => s.title = d["aiTitle"].as_str().unwrap_or_default().to_string(),
            "last-prompt" if prompt.is_empty() => prompt = d["lastPrompt"].as_str().unwrap_or_default().to_string(),
            "assistant" if !done && d["isSidechain"] != true => {
                let mut t = texts(&d["message"]["content"]);
                t.reverse();
                last.extend(t);
            }
            // a user line (prompt or tool result) ends the final turn's text
            "user" if !done && !last.is_empty() && d["isSidechain"] != true => done = true,
            _ => {}
        }
        if done && !s.title.is_empty() && !s.cwd.is_empty() {
            break;
        }
    }
    last.reverse();
    s.last = last.join("\n\n").trim().to_string();
    if s.title.is_empty() {
        s.title = prompt.lines().next().unwrap_or_default().to_string();
    }
}

/// Claude session ids codehead's threads own (their hosts resume those themselves).
fn thread_owned() -> HashSet<String> {
    crate::engine::store::all().into_iter().flat_map(|t| t.sessions.into_values()).collect()
}

/// The LIMIT newest top-level sessions, thread-owned ones left out.
pub fn load(limit: usize) -> Vec<Session> {
    type Entry = (SystemTime, u64, Option<Session>);
    static PARSED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<PathBuf, Entry>>> = std::sync::OnceLock::new();
    let mut cache = PARSED.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    let root = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".claude/projects");
    let owned = thread_owned();
    let mut files: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .flat_map(|d| std::fs::read_dir(d.path()).into_iter().flatten().flatten())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .filter(|p| p.file_stem().is_some_and(|s| !owned.contains(&*s.to_string_lossy())))
        .filter_map(|p| Some((p.metadata().ok()?.modified().ok()?, p)))
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    cache.retain(|path, _| files.iter().any(|(_, p)| p == path));
    let mut out = Vec::new();
    for (mtime, path) in files {
        if out.len() >= limit {
            break;
        }
        let len = path.metadata().map(|m| m.len()).unwrap_or(0);
        let parsed = match cache.get(&path) {
            Some((time, size, session)) if *time == mtime && *size == len => session.clone(),
            _ => {
                let mut s = Session { id: path.file_stem().unwrap_or_default().to_string_lossy().into_owned(), path: path.clone(), ..Default::default() };
                parse(&tail(&path, TAIL), &mut s);
                if s.last.is_empty() { parse(&tail(&path, BIG), &mut s); }
                let session = (!s.last.is_empty() && !s.cwd.is_empty()).then_some(s);
                cache.insert(path, (mtime, len, session.clone()));
                session
            }
        };
        if let Some(mut s) = parsed {
            s.when = crate::notes::ago(mtime);
            out.push(s);
        }
    }
    out
}

/// Session from a transcript file; None: 0 responses in it.
pub fn from_transcript(path: &Path) -> Option<Session> {
    let mut s = Session { id: path.file_stem()?.to_string_lossy().into_owned(), ..Default::default() };
    if let Ok(m) = path.metadata().and_then(|m| m.modified()) {
        s.when = crate::notes::ago(m);
    }
    parse(&tail(path, TAIL), &mut s);
    if s.last.is_empty() {
        parse(&tail(path, BIG), &mut s);
    }
    s.path = path.to_path_buf();
    (!s.last.is_empty()).then_some(s)
}

/// Transcript of a thread's lead session: ~/.claude/projects/*/<session id>.jsonl.
fn thread_transcript(id: &str) -> Option<PathBuf> {
    let t = crate::engine::store::load(id)?;
    let session = t.lead().and_then(|l| t.sessions.get(l)).or_else(|| t.sessions.values().next())?;
    let root = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".claude/projects");
    std::fs::read_dir(&root).into_iter().flatten().flatten().map(|d| d.path().join(format!("{session}.jsonl"))).find(|p| p.is_file())
}

/// Context of a thread: the last response of its lead's Claude session.
pub fn of_thread(id: &str, title: &str) -> Option<Session> {
    let mut s = from_transcript(&thread_transcript(id)?)?;
    s.thread = Some(id.to_string());
    s.title = title.lines().next().unwrap_or_default().to_string();
    Some(s)
}

/// Thread context block: the session's last response.
pub fn context(s: &Session) -> String {
    match &s.thread {
        Some(t) => format!(
            "Context: the last response of codehead thread \"{}\" ({}, {}). Pick up from there:\n\n{}",
            s.title,
            t.chars().take(12).collect::<String>(),
            s.transcript_hint(),
            s.last
        ),
        None => format!("Context: the last response of Claude Code session {} ({}). Pick up from there:\n\n{}", s.id, s.transcript_hint(), s.last),
    }
}

impl Session {
    /// Dir name, e.g. `collection`.
    /// Key in the context list: thread id, else session id.
    pub fn key(&self) -> String {
        self.thread.clone().unwrap_or_else(|| self.id.clone())
    }

    pub fn place(&self) -> String {
        Path::new(&self.cwd).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| self.cwd.clone())
    }

    fn transcript_hint(&self) -> String {
        format!("transcript: {}", self.path.display())
    }
}

pub fn preview(s: &Session) -> Vec<Line<'static>> {
    let mut out = vec![
        Line::styled(s.title.clone(), theme::accent_bold()),
        Line::from(vec![Span::styled("dir ", theme::muted()), Span::raw(s.cwd.clone())]),
        Line::from(vec![Span::styled("branch ", theme::muted()), Span::raw(if s.branch.is_empty() { "-".to_string() } else { s.branch.clone() })]),
        Line::from(vec![Span::styled("session ", theme::muted()), Span::raw(s.id.clone()), Span::styled(format!(" · {}", s.when), theme::muted())]),
        Line::raw(""),
        Line::styled("enter: new thread in this dir, this response as context · c: add as context", theme::muted()),
        Line::raw(""),
        Line::styled("last response", theme::bold()),
    ];
    out.extend(s.last.lines().map(|l| Line::raw(l.to_string())));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_response_after_last_tool_result() {
        let t = [
            r#"{"type":"user","cwd":"/p","gitBranch":"main","message":{"content":"do it"}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"checking"},{"type":"tool_use"}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result"}]}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"DONE. part 1"}]}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"part 2"}]}}"#,
            r#"{"type":"ai-title","aiTitle":"Fix it"}"#,
        ]
        .join("\n");
        let mut s = Session::default();
        parse(&t, &mut s);
        assert_eq!(s.last, "DONE. part 1\n\npart 2");
        assert_eq!(s.title, "Fix it");
        assert_eq!(s.cwd, "/p");
        assert_eq!(s.branch, "main");
    }

    /// Live check: CODEHEAD_THREAD=<id> cargo test live_of_thread -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_of_thread() {
        let id = std::env::var("CODEHEAD_THREAD").unwrap();
        let s = of_thread(&id, "live").expect("0 responses");
        println!("{}\n{}", s.path.display(), s.last.chars().take(300).collect::<String>());
    }
}
