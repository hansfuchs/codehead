//! Findings of a reviewer model on the changes view's diff (setting `review`, default
//! `claude:sonnet`, `off`: 0 reviews): 1 Claude Code call with read-only tools (Read, Grep,
//! Glob) in the worktree, a replaced system prompt, 0 MCP servers, structured output. Kept in
//! `<thread dir>/findings.json`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::features::changes::{FileDiff, Kind};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Finding {
    pub file: String,
    /// New-side line.
    pub line: u32,
    /// critical, major or minor.
    pub severity: String,
    pub title: String,
    pub body: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Review {
    pub model: String,
    pub findings: Vec<Finding>,
}

/// Diff lines in the prompt, all files together.
const BUDGET: usize = 3000;

fn path(id: &str) -> PathBuf {
    crate::engine::store::thread_dir(id).join("findings.json")
}

pub(crate) fn cached(id: &str) -> Option<Review> {
    std::fs::read(path(id))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

pub(crate) fn store(id: &str, result: &Review) {
    if let Ok(bytes) = serde_json::to_vec_pretty(result) {
        let _ = crate::engine::write_atomic(&path(id), &bytes);
    }
}

pub(crate) fn prompt(files: &[FileDiff]) -> String {
    let mut prompt = ReviewPrompt {
        text: INTRO.to_string(),
        count: 0,
    };

    for file in files.iter().filter(|file| !file.binary) {
        if !prompt.file(file) {
            break;
        }
    }

    prompt.text
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
pub(crate) fn parse(out: &str, files: &[FileDiff]) -> Result<Vec<Finding>, String> {
    let value: Value = serde_json::from_str(out.trim()).map_err(|error| format!("review: {error}"))?;

    if value["is_error"] == true {
        return Err(format!(
            "review: {}",
            value["result"]
                .as_str()
                .unwrap_or("error")
                .lines()
                .next()
                .unwrap_or_default()
        ));
    }

    let data = if value["structured_output"].is_object() {
        value["structured_output"].clone()
    } else {
        serde_json::from_str::<Value>(value["result"].as_str().unwrap_or_default())
            .map_err(|_| "review: 0 structured output".to_string())?
    };

    let one = |x: &Value| {
        x.as_str()
            .unwrap_or_default()
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_string()
    };

    let mut out: Vec<Finding> = data["findings"]
        .as_array()
        .ok_or("review: 0 findings list")?
        .iter()
        .filter_map(|file_value| {
            let file = file_value["file"].as_str()?.trim().to_string();

            files.iter().any(|x| x.path == file).then(|| Finding {
                file,
                line: file_value["line"].as_u64().unwrap_or(1).max(1) as u32,
                severity: one(&file_value["severity"]),
                title: one(&file_value["title"]),
                body: file_value["body"].as_str().unwrap_or_default().trim().to_string(),
            })
        })
        .collect();

    let rank = |text: &str| match text {
        "critical" => 0,
        "major" => 1,
        _ => 2,
    };

    out.sort_by_key(|file_2| rank(&file_2.severity));

    Ok(out)
}

/// Review FILES of DIR with MODEL (blocking: run it on a thread). Stores the result for ID.
pub(crate) fn run(id: &str, dir: &Path, files: &[FileDiff], model: &str) -> Result<Review, String> {
    let out = Command::new("claude")
        .current_dir(dir)
        .args(["-p", "--model", model, "--tools", "Read,Grep,Glob", "--strict-mcp-config", "--setting-sources", "", "--disable-slash-commands", "--no-session-persistence"])
        .args(["--system-prompt", "You review code changes for real defects. Read-only: you change 0 files. Output only the structured result: 0 notes, 0 extra text."])
        .args(["--output-format", "json", "--json-schema", &schema().to_string()])
        .arg(prompt(files))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("review: claude: {error}"))?;
    let findings = parse(&String::from_utf8_lossy(&out.stdout), files)?;
    let result = Review {
        model: model.to_string(),
        findings,
    };

    store(id, &result);
    Ok(result)
}

/// The comment text of finding F (for the batch to an agent).
pub(crate) fn as_comment(file: &Finding) -> String {
    format!("[{}] {}: {}", file.severity, file.title, file.body)
}

const INTRO: &str = "Review this change set as a senior engineer. Find real problems only: bugs, wrong logic, missing error handling, security issues, races, broken edge cases, API misuse, data loss. Skip style, naming and formatting.\n\
- Per finding: file (the path as given), line (its new-side line number, as in the `N|` prefix), severity (critical, major or minor), title (max. 10 words), body (max. 2 sentences: what breaks, how to fix).\n\
- Read files of the worktree when the diff alone leaves a doubt. 0 findings is a valid answer.\n\n";

struct ReviewPrompt {
    text: String,
    count: usize,
}

impl ReviewPrompt {
    fn file(&mut self, file: &FileDiff) -> bool {
        let _ = writeln!(self.text, "## {}", file.path);
        let complete = file.hunks.iter().all(|hunk| self.hunk(hunk));

        if !complete {
            return false;
        }

        self.text.push('\n');
        true
    }

    fn hunk(&mut self, hunk: &crate::domain::changes::Hunk) -> bool {
        let _ = writeln!(self.text, "@@ {}", hunk.ctx);

        hunk.lines.iter().all(|line| self.line(line))
    }

    fn line(&mut self, line: &crate::domain::changes::DLine) -> bool {
        if self.count == BUDGET {
            self.text.push_str("… (diff cut)\n");
            return false;
        }

        let (sign, number) = match line.kind {
            Kind::Add => ('+', line.new),
            Kind::Del => ('-', None),
            Kind::Ctx => (' ', line.new),
        };

        let number = number.map_or("    ".to_string(), |number| format!("{number:>4}"));
        let _ = writeln!(self.text, "{number}|{sign}{}", line.text);

        self.count += 1;

        true
    }
}

#[cfg(test)]
#[path = "changes_review_tests.rs"]
mod tests;
