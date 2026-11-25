//! `>` handoff: a new thread that goes on from this one with a compact handoff document instead
//! of the full history. The review model (setting `review`) writes the document from the
//! thread's log; codehead adds exact facts (branch, worktree, changed files). The new thread
//! keeps the worktree, branch, ports and agents, starts fresh sessions, and opens with the
//! document as its prompt draft: the owner edits it, adds the next goal, sends.

use std::fmt::Write as _;
use std::process::{Command, Stdio};

use crate::engine::store::{self, Kind, Thread};

/// Transcript chars sent to the model (the latest kept).
const TRANSCRIPT_MAX: usize = 80_000;

/// 1 short line of tool call NAME with INPUT: `Edit src/x.rs`, `Bash cargo test`.
pub(crate) fn tool_line(name: &str, input: &serde_json::Value) -> String {
    let arg = ["file_path", "path", "command", "pattern", "url", "query", "description"]
        .iter()
        .find_map(|key| input[*key].as_str())
        .unwrap_or_default();
    let arg: String = arg.lines().next().unwrap_or_default().chars().take(120).collect();

    format!("{name} {arg}").trim().to_string()
}

/// The thread's log as plain text: messages in full, tool calls as 1 line each.
pub(crate) fn transcript(id: &str) -> String {
    let mut out = String::new();

    for event in store::events(id) {
        let who = event.agent.clone().unwrap_or_else(|| "owner".into());

        let line = match &event.kind {
            Kind::Prompt { from, to, text } => format!("\n### {from} → {}\n{text}\n", to.join(", ")),
            Kind::Text { text } => format!("\n### {who}\n{text}\n"),
            Kind::Tool { name, input, .. } => format!("- {who} ran: {}\n", tool_line(name, input)),
            Kind::ToolResult { ok: false, output, .. } => {
                format!("  failed: {}\n", output.lines().next().unwrap_or_default())
            }
            Kind::Status { state, note } => format!("- {who} status: {state} · {note}\n"),
            Kind::Error { message } => format!("- {who} error: {message}\n"),
            Kind::Answer { label, allow, .. } => {
                format!("- owner answered: {label}{}\n", if *allow { "" } else { " (declined)" })
            }
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
pub(crate) fn facts(thread: &Thread) -> String {
    let dir = thread.work_dir();
    let git = |args: &[&str]| crate::domain::threads::git(&dir, args).1;
    let mut text = String::from("## Facts (from codehead)\n\n");

    let _ = writeln!(
        text,
        "- previous thread: {} ({})",
        thread.id,
        thread.title.lines().next().unwrap_or_default()
    );
    let _ = writeln!(text, "- directory: {}", dir.display());

    if let Some(branch) = &thread.branch {
        let _ = writeln!(text, "- branch: {branch}");
    }

    if let Some(port) = thread.port {
        let _ = writeln!(
            text,
            "- ports: {port}-{}",
            port + crate::infra::worktree_setup::PORTS - 1
        );
    }

    let status = git(&["status", "--short"]);

    if !status.trim().is_empty() {
        text.push_str("- uncommitted:\n");

        for line in status.lines().take(40) {
            let _ = writeln!(text, "  - `{}`", line.trim());
        }
    }

    let log = git(&["log", "--oneline", "-10"]);

    if !log.trim().is_empty() {
        text.push_str("- last commits:\n");

        for commit in log.lines() {
            let _ = writeln!(text, "  - {commit}");
        }
    }

    text
}

/// The prompt for the model: the template, the task, the transcript.
pub(crate) fn prompt(thread: &Thread, transcript: &str) -> String {
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
        thread.task
    )
}

/// The handoff document of thread ID by MODEL (blocking: run it on a thread).
pub(crate) fn write(id: &str, model: &str) -> Result<String, String> {
    let thread = store::load(id).ok_or_else(|| format!("no thread {id}"))?;
    let out = Command::new("claude")
        .current_dir(thread.work_dir())
        .args([
            "-p",
            "--model",
            model,
            "--tools",
            "",
            "--strict-mcp-config",
            "--setting-sources",
            "",
            "--disable-slash-commands",
            "--no-session-persistence",
        ])
        .args([
            "--system-prompt",
            "You write handoff documents between coding agents. Output only the document: 0 notes, 0 extra text.",
        ])
        .args(["--output-format", "json"])
        .arg(prompt(&thread, &transcript(id)))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("claude: {error}"))?;
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|error| format!("claude: {error}"))?;
    let text = value["result"].as_str().unwrap_or_default();

    if value["is_error"] == true || text.trim().is_empty() {
        return Err(text.lines().next().unwrap_or("0 document").to_string());
    }

    // a trailing note paragraph of the model goes
    let doc: Vec<&str> = text
        .trim()
        .split("\n\n")
        .take_while(|paragraph| !paragraph.trim_start().starts_with("Note:"))
        .collect();

    Ok(format!(
        "# Handoff from \"{}\"\n\n{}\n\n{}",
        thread.title.lines().next().unwrap_or_default(),
        doc.join("\n\n").trim(),
        facts(&thread)
    ))
}

/// The file a view reads as its prompt draft once (then removes).
pub(crate) fn draft_path(id: &str) -> std::path::PathBuf {
    store::thread_dir(id).join("draft.md")
}

/// The new thread after OLD: same place, branch, ports, agents; fresh sessions; DOC as its
/// task and its prompt draft.
pub(crate) fn start(old: &Thread, doc: &str) -> Result<Thread, String> {
    let now = crate::engine::now_ms();
    let thread = Thread {
        id: crate::engine::new_id(),
        title: format!(
            "{} (cont.)",
            old.title
                .lines()
                .next()
                .unwrap_or_default()
                .trim_end_matches(" (cont.)")
        ),
        task: doc.to_string(),
        created: now,
        updated: now,
        state: "open".into(),
        sessions: std::collections::BTreeMap::default(),
        fork: Vec::new(),
        setup: None,
        parent: Some(old.id.clone()),
        ..old.clone()
    };

    std::fs::create_dir_all(store::thread_dir(&thread.id)).map_err(|error| error.to_string())?;
    thread.save()?;
    std::fs::write(draft_path(&thread.id), format!("{doc}\n\n## Next\n\n")).map_err(|error| error.to_string())?;

    Ok(thread)
}

/// A prompt draft of thread ID waiting for its view (read once).
pub(crate) fn take_draft(id: &str) -> Option<String> {
    let path = draft_path(id);
    let draft = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);

    Some(draft)
}

#[cfg(test)]
#[path = "handoff_tests.rs"]
mod tests;
