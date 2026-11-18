//! Findings of a reviewer model on the changes view's diff (setting `review`, default
//! `claude:sonnet`, `off`: 0 reviews): 1 Claude Code call with read-only tools (Read, Grep,
//! Glob) in the worktree, a replaced system prompt, 0 MCP servers, structured output. Kept in
//! `<thread dir>/findings.json`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::changes::{FileDiff, Kind};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub file: String,
    /// New-side line.
    pub line: u32,
    /// critical, major or minor.
    pub severity: String,
    pub title: String,
    pub body: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Review {
    pub model: String,
    pub findings: Vec<Finding>,
}

/// Diff lines in the prompt, all files together.
const BUDGET: usize = 3000;

fn path(id: &str) -> PathBuf {
    crate::engine::store::thread_dir(id).join("findings.json")
}

pub fn cached(id: &str) -> Option<Review> {
    std::fs::read(path(id)).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

pub fn store(id: &str, r: &Review) {
    if let Ok(b) = serde_json::to_vec_pretty(r) {
        let _ = crate::engine::write_atomic(&path(id), &b);
    }
}

/// The diff of FILES as the reviewer reads it: per file its path and hunks with new-side line
/// numbers, cut at BUDGET lines.
pub fn prompt(files: &[FileDiff]) -> String {
    let mut s = String::from(
        "Review this change set as a senior engineer. Find real problems only: bugs, wrong logic, missing error handling, security issues, races, broken edge cases, API misuse, data loss. Skip style, naming and formatting.\n\
- Per finding: file (the path as given), line (its new-side line number, as in the `N|` prefix), severity (critical, major or minor), title (max. 10 words), body (max. 2 sentences: what breaks, how to fix).\n\
- Read files of the worktree when the diff alone leaves a doubt. 0 findings is a valid answer.\n\n",
    );
    let mut n = 0;
    'f: for f in files.iter().filter(|f| !f.binary) {
        s.push_str(&format!("## {}\n", f.path));
        for h in &f.hunks {
            s.push_str(&format!("@@ {}\n", h.ctx));
            for l in &h.lines {
                if n == BUDGET {
                    s.push_str("… (diff cut)\n");
                    break 'f;
                }
                let (sign, num) = match l.kind {
                    Kind::Add => ('+', l.new),
                    Kind::Del => ('-', None),
                    Kind::Ctx => (' ', l.new),
                };
                let num = num.map_or("    ".to_string(), |x| format!("{x:>4}"));
                s.push_str(&format!("{num}|{sign}{}\n", l.text));
                n += 1;
            }
        }
        s.push('\n');
    }
    s
}

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {"findings": {"type": "array", "items": {"type": "object", "properties": {
            "file": {"type": "string"}, "line": {"type": "integer"},
            "severity": {"type": "string", "enum": ["critical", "major", "minor"]},
            "title": {"type": "string"}, "body": {"type": "string"}},
            "required": ["file", "line", "severity", "title", "body"]}}},
        "required": ["findings"]
    })
}

/// Findings of Claude Code's JSON output OUT: only files of FILES, critical first.
pub fn parse(out: &str, files: &[FileDiff]) -> Result<Vec<Finding>, String> {
    let v: Value = serde_json::from_str(out.trim()).map_err(|e| format!("review: {e}"))?;
    if v["is_error"] == true {
        return Err(format!("review: {}", v["result"].as_str().unwrap_or("error").lines().next().unwrap_or_default()));
    }
    let data = if v["structured_output"].is_object() {
        v["structured_output"].clone()
    } else {
        serde_json::from_str::<Value>(v["result"].as_str().unwrap_or_default()).map_err(|_| "review: 0 structured output".to_string())?
    };
    let one = |x: &Value| x.as_str().unwrap_or_default().lines().next().unwrap_or_default().trim().to_string();
    let mut out: Vec<Finding> = data["findings"]
        .as_array()
        .ok_or("review: 0 findings list")?
        .iter()
        .filter_map(|f| {
            let file = f["file"].as_str()?.trim().to_string();
            files.iter().any(|x| x.path == file).then(|| Finding {
                file,
                line: f["line"].as_u64().unwrap_or(1).max(1) as u32,
                severity: one(&f["severity"]),
                title: one(&f["title"]),
                body: f["body"].as_str().unwrap_or_default().trim().to_string(),
            })
        })
        .collect();
    let rank = |s: &str| match s {
        "critical" => 0,
        "major" => 1,
        _ => 2,
    };
    out.sort_by_key(|f| rank(&f.severity));
    Ok(out)
}

/// Review FILES of DIR with MODEL (blocking: run it on a thread). Stores the result for ID.
pub fn run(id: &str, dir: &Path, files: &[FileDiff], model: &str) -> Result<Review, String> {
    let out = Command::new("claude")
        .current_dir(dir)
        .args(["-p", "--model", model, "--tools", "Read,Grep,Glob", "--strict-mcp-config", "--setting-sources", "", "--disable-slash-commands", "--no-session-persistence"])
        .args(["--system-prompt", "You review code changes for real defects. Read-only: you change 0 files. Output only the structured result: 0 notes, 0 extra text."])
        .args(["--output-format", "json", "--json-schema", &schema().to_string()])
        .arg(prompt(files))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("review: claude: {e}"))?;
    let findings = parse(&String::from_utf8_lossy(&out.stdout), files)?;
    let r = Review { model: model.to_string(), findings };
    store(id, &r);
    Ok(r)
}

/// The comment text of finding F (for the batch to an agent).
pub fn as_comment(f: &Finding) -> String {
    format!("[{}] {}: {}", f.severity, f.title, f.body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Vec<FileDiff> {
        crate::changes::parse("diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,2 +1,2 @@ fn main\n a\n-b\n+c\n")
    }

    #[test]
    fn prompt_has_new_side_numbers() {
        let p = prompt(&files());
        assert!(p.contains("## src/a.rs\n@@ fn main\n   1| a\n    |-b\n   2|+c\n"), "{p}");
    }

    #[test]
    fn parse_keeps_known_files_critical_first() {
        let out = json!({"is_error": false, "structured_output": {"findings": [
            {"file": "src/a.rs", "line": 2, "severity": "minor", "title": "t1", "body": "b1"},
            {"file": "nope.rs", "line": 1, "severity": "critical", "title": "x", "body": "x"},
            {"file": "src/a.rs", "line": 0, "severity": "critical", "title": "t2\nnote", "body": "b2"}
        ]}}).to_string();
        let f = parse(&out, &files()).unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!((f[0].title.as_str(), f[0].line), ("t2", 1));
        assert_eq!(as_comment(&f[1]), "[minor] t1: b1");
    }
}
