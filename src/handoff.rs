//! `>` handoff: a new thread that goes on from this one with a compact handoff document instead
//! of the full history. The review model (setting `review`) writes the document from the
//! thread's log; codehead adds exact facts (branch, worktree, changed files). The new thread
//! keeps the worktree, branch, ports and agents, starts fresh sessions, and opens with the
//! document as its prompt draft: the owner edits it, adds the next goal, sends.

use std::process::{Command, Stdio};

use crate::engine::store::{self, Kind, Thread};

/// Transcript chars sent to the model (the latest kept).
const TRANSCRIPT_MAX: usize = 80_000;

/// 1 short line of tool call NAME with INPUT: `Edit src/x.rs`, `Bash cargo test`.
fn tool_line(name: &str, input: &serde_json::Value) -> String {
    let arg = ["file_path", "path", "command", "pattern", "url", "query", "description"].iter().find_map(|k| input[*k].as_str()).unwrap_or_default();
    let arg: String = arg.lines().next().unwrap_or_default().chars().take(120).collect();
    format!("{name} {arg}").trim().to_string()
}

/// The thread's log as plain text: messages in full, tool calls as 1 line each.
pub fn transcript(id: &str) -> String {
    let mut out = String::new();
    for e in store::events(id) {
        let who = e.agent.clone().unwrap_or_else(|| "owner".into());
        let line = match &e.kind {
            Kind::Prompt { from, to, text } => format!("\n### {from} → {}\n{text}\n", to.join(", ")),
            Kind::Text { text } => format!("\n### {who}\n{text}\n"),
            Kind::Tool { name, input, .. } => format!("- {who} ran: {}\n", tool_line(name, input)),
            Kind::ToolResult { ok: false, output, .. } => format!("  failed: {}\n", output.lines().next().unwrap_or_default()),
            Kind::Status { state, note } => format!("- {who} status: {state} · {note}\n"),
            Kind::Error { message } => format!("- {who} error: {message}\n"),
            Kind::Answer { label, allow, .. } => format!("- owner answered: {label}{}\n", if *allow { "" } else { " (declined)" }),
            _ => continue,
        };
        out.push_str(&line);
    }
    if out.len() > TRANSCRIPT_MAX {
        let cut = out.len() - TRANSCRIPT_MAX;
        let cut = (cut..out.len()).find(|&i| out.is_char_boundary(i)).unwrap_or(0);
        out = format!("[… older part cut]\n{}", &out[cut..]);
    }
    out
}

/// What codehead knows exactly about thread T: branch, worktree, ports, changed files.
pub fn facts(t: &Thread) -> String {
    let dir = t.work_dir();
    let git = |args: &[&str]| crate::threads::git(&dir, args).1;
    let mut s = String::from("## Facts (from codehead)\n\n");
    s.push_str(&format!("- previous thread: {} ({})\n", t.id, t.title.lines().next().unwrap_or_default()));
    s.push_str(&format!("- directory: {}\n", dir.display()));
    if let Some(b) = &t.branch {
        s.push_str(&format!("- branch: {b}\n"));
    }
    if let Some(p) = t.port {
        s.push_str(&format!("- ports: {p}-{}\n", p + crate::setup::PORTS - 1));
    }
    let status = git(&["status", "--short"]);
    if !status.trim().is_empty() {
        s.push_str("- uncommitted:\n");
        for l in status.lines().take(40) {
            s.push_str(&format!("  - `{}`\n", l.trim()));
        }
    }
    let log = git(&["log", "--oneline", "-10"]);
    if !log.trim().is_empty() {
        s.push_str("- last commits:\n");
        for l in log.lines() {
            s.push_str(&format!("  - {l}\n"));
        }
    }
    s
}

/// The prompt for the model: the template, the task, the transcript.
pub fn prompt(t: &Thread, transcript: &str) -> String {
    format!(
        "Write the handoff document for a new thread that continues this one. The reader is a coding agent with 0 memory of this thread: give it everything it needs, nothing else.\n\
Sections, in this order, as Markdown `##` headings:\n\
## Goal — what the work is for, the owner's words where they matter.\n\
## Decisions — each with why.\n\
## State of the code — what is done and works, what is half done, what is broken; name files.\n\
## Failed approaches — what was tried and why it failed (do not retry).\n\
## Open todos — in order.\n\
## How to run and test — exact commands.\n\
## Owner preferences — how the owner wants things done, seen in this thread.\n\
Max. 600 words. Facts only, no filler. Output only the document.\n\n\
# Task of the thread\n\n{}\n\n# Thread log\n{transcript}",
        t.task
    )
}

/// The handoff document of thread ID by MODEL (blocking: run it on a thread).
pub fn write(id: &str, model: &str) -> Result<String, String> {
    let t = store::load(id).ok_or_else(|| format!("no thread {id}"))?;
    let out = Command::new("claude")
        .current_dir(t.work_dir())
        .args(["-p", "--model", model, "--tools", "", "--strict-mcp-config", "--setting-sources", "", "--disable-slash-commands", "--no-session-persistence"])
        .args(["--system-prompt", "You write handoff documents between coding agents. Output only the document: 0 notes, 0 extra text."])
        .args(["--output-format", "json"])
        .arg(prompt(&t, &transcript(id)))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("claude: {e}"))?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("claude: {e}"))?;
    let text = v["result"].as_str().unwrap_or_default();
    if v["is_error"] == true || text.trim().is_empty() {
        return Err(text.lines().next().unwrap_or("0 document").to_string());
    }
    // a trailing note paragraph of the model goes
    let doc: Vec<&str> = text.trim().split("\n\n").take_while(|p| !p.trim_start().starts_with("Note:")).collect();
    Ok(format!("# Handoff from \"{}\"\n\n{}\n\n{}", t.title.lines().next().unwrap_or_default(), doc.join("\n\n").trim(), facts(&t)))
}

/// The file a view reads as its prompt draft once (then removes).
pub fn draft_path(id: &str) -> std::path::PathBuf {
    store::thread_dir(id).join("draft.md")
}

/// The new thread after OLD: same place, branch, ports, agents; fresh sessions; DOC as its
/// task and its prompt draft.
pub fn start(old: &Thread, doc: &str) -> Result<Thread, String> {
    let now = crate::engine::now_ms();
    let t = Thread {
        id: crate::engine::new_id(),
        title: format!("{} (cont.)", old.title.lines().next().unwrap_or_default().trim_end_matches(" (cont.)")),
        task: doc.to_string(),
        created: now,
        updated: now,
        state: "open".into(),
        sessions: Default::default(),
        fork: Vec::new(),
        setup: None,
        parent: Some(old.id.clone()),
        ..old.clone()
    };
    std::fs::create_dir_all(store::thread_dir(&t.id)).map_err(|e| e.to_string())?;
    t.save()?;
    std::fs::write(draft_path(&t.id), format!("{doc}\n\n## Next\n\n")).map_err(|e| e.to_string())?;
    Ok(t)
}

/// A prompt draft of thread ID waiting for its view (read once).
pub fn take_draft(id: &str) -> Option<String> {
    let p = draft_path(id);
    let d = std::fs::read_to_string(&p).ok()?;
    let _ = std::fs::remove_file(&p);
    Some(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Live: CODEHEAD_THREAD=<id> cargo test handoff_live -- --ignored --nocapture
    #[test]
    #[ignore]
    fn handoff_live() {
        let id = std::env::var("CODEHEAD_THREAD").unwrap();
        println!("{}", write(&id, "haiku").unwrap());
    }

    #[test]
    fn tool_lines_are_short() {
        assert_eq!(tool_line("Edit", &serde_json::json!({"file_path": "src/x.rs", "old_string": "a"})), "Edit src/x.rs");
        assert_eq!(tool_line("Bash", &serde_json::json!({"command": "cargo test\nmore"})), "Bash cargo test");
        assert_eq!(tool_line("TodoWrite", &serde_json::json!({})), "TodoWrite");
    }

    #[test]
    fn start_copies_the_place_and_leaves_the_draft_once() {
        let _env = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut old = Thread::new("fix deploy", "task", Path::new("/tmp"), vec!["ada".into(), "bob".into()]);
        old.branch = Some("task/fix".into());
        old.port = Some(40020);
        old.sessions.insert("ada".into(), "s1".into());
        let t = start(&old, "# Handoff\n\n## Goal\nship it").unwrap();
        assert_eq!((t.title.as_str(), t.agents.len(), t.port, t.branch.as_deref()), ("fix deploy (cont.)", 2, Some(40020), Some("task/fix")));
        assert!(t.sessions.is_empty() && t.parent.as_deref() == Some(old.id.as_str()));
        assert!(take_draft(&t.id).unwrap().starts_with("# Handoff"));
        assert!(take_draft(&t.id).is_none(), "once");
        // a handoff of a handoff: 1 `(cont.)`
        assert_eq!(start(&t, "x").unwrap().title, "fix deploy (cont.)");
        let _ = std::fs::remove_dir_all(store::thread_dir(&t.id));
    }
}
