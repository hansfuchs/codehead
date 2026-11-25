//! Review order and review levels of the changes view by an agent (setting `classify`, empty:
//! the default agent; 0 agent: the `rank` model, default `claude:haiku`): files in groups that
//! do 1 thing, groups in reading order (schema / API → logic → glue → UI → docs → tests →
//! generated), a title + why per group, a why and a level (critical · high · medium · low) per
//! file. 1 call per set of changed paths (`<thread dir>/rank.json`): Claude Code with a replaced
//! system prompt, 0 tools, 0 MCP servers, structured output; Codex: `codex exec`, read-only.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::domain::changes::{Cat, FileDiff, Kind};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct RFile {
    pub path: String,
    pub why: String,
    /// critical, high, medium or low ("": an older rank, path rules decide).
    #[serde(default)]
    pub level: String,
}

/// How much a file's change needs a review: the tabs of the changes view. A comment's severity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Level {
    Critical,
    High,
    #[default]
    Medium,
    Low,
}

impl Level {
    pub(crate) const ALL: [Level; 4] = [Level::Critical, Level::High, Level::Medium, Level::Low];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Level::Critical => "critical",
            Level::High => "high",
            Level::Medium => "medium",
            Level::Low => "low",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Level> {
        let trimmed = text.trim();

        // the reviewer's words too: major, minor
        match trimmed.to_ascii_lowercase().as_str() {
            "major" => Some(Level::High),
            "minor" => Some(Level::Low),
            _ => Level::ALL
                .into_iter()
                .find(|level| level.label().eq_ignore_ascii_case(trimmed)),
        }
    }

    /// DELTA steps through ALL, wrapping (the comment modal's ←→).
    pub(crate) fn step(self, delta: isize) -> Level {
        let i = Level::ALL.iter().position(|level| *level == self).unwrap_or(2) as isize;

        Level::ALL[(i + delta).rem_euclid(Level::ALL.len() as isize) as usize]
    }
}

/// The level of F by path rules (before or without the agent's call).
pub(crate) fn guess(file: &FileDiff) -> Level {
    let lowered = file.path.to_ascii_lowercase();
    let risky = [
        "auth",
        "security",
        "crypto",
        "password",
        "secret",
        "token",
        "permission",
        "acl",
        "payment",
        "billing",
        "migration",
        "sandbox",
        "guard",
        "session",
    ];

    match file.cat {
        Cat::Docs | Cat::Tests | Cat::Generated => Level::Low,
        _ if file.binary => Level::Low,
        _ if risky.iter().any(|word| lowered.contains(word)) => Level::Critical,
        Cat::Schema => Level::High,
        Cat::Logic if file.add + file.del >= 30 => Level::High,
        _ => Level::Medium,
    }
}

/// Who classifies: an agent's harness and model (None: the harness's default model).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Runner {
    pub label: String,
    pub harness: String,
    pub model: Option<String>,
}

/// The classifier: setting `classify` (an agent; `off`: path rules), else the default agent
/// (`agent`, else amon, else the 1st agent), else the `rank` model (`off`: path rules).
pub(crate) fn runner() -> Option<Runner> {
    let set = crate::infra::config::classify();

    // `classify = off`, or unset with `rank = off`: path rules
    if set.as_deref() == Some("off") || (set.is_none() && crate::infra::config::rank_model().is_none()) {
        return None;
    }

    let name = set.or_else(crate::infra::config::agent).or_else(|| {
        let all = crate::engine::roster::active();

        all.iter()
            .find(|agent| agent.name == "amon")
            .or(all.first())
            .map(|agent| agent.name.clone())
    });

    if let Some(agent) = name.as_deref().and_then(crate::engine::roster::get) {
        let model = agent
            .model
            .clone()
            .filter(|model| !model.is_empty() && model != "default");

        return Some(Runner {
            label: agent.name.clone(),
            harness: agent.harness.clone(),
            model,
        });
    }

    crate::infra::config::rank_model().map(|model| Runner {
        label: format!("claude/{model}"),
        harness: "claude".into(),
        model: Some(model),
    })
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Group {
    pub title: String,
    pub why: String,
    pub files: Vec<RFile>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Rank {
    /// Hash of the set of paths it ranked.
    pub key: u64,
    pub model: String,
    pub groups: Vec<Group>,
}

/// Diff lines in the prompt, all files together.
const BUDGET: usize = 600;
/// Files above this: 0 call (path rules only).
const MAX_FILES: usize = 400;

/// Key of FILES: the set of their paths (content edits keep a rank, a new or gone file renews it).
pub(crate) fn key(files: &[FileDiff]) -> u64 {
    let paths: BTreeSet<&str> = files.iter().map(|file| file.path.as_str()).collect();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();

    paths.hash(&mut hasher);
    hasher.finish()
}

fn path(id: &str) -> PathBuf {
    crate::engine::store::thread_dir(id).join("rank.json")
}

pub(crate) fn cached(id: &str) -> Option<Rank> {
    std::fs::read(path(id))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

fn store(id: &str, rank: &Rank) {
    if let Ok(bytes) = serde_json::to_vec_pretty(rank) {
        let _ = crate::engine::write_atomic(&path(id), &bytes);
    }
}

/// The prompt for FILES: each file's path, category guess, size, and its diff cut to a share
/// of BUDGET lines.
pub(crate) fn prompt(files: &[FileDiff]) -> String {
    let per = (BUDGET / files.len().max(1)).clamp(8, 60);
    let mut text = String::from(
        "Order these changed files for a code review: the reviewer reads top to bottom.\n\
- Group files that do 1 thing together.\n\
- Groups in reading order: data model, schema, API contracts first; then business logic; then glue (handlers, wiring, config); then UI; then docs; then tests; generated files and lockfiles last.\n\
- Group: title (max. 6 words), why (1 sentence, max. 15 words: what it changes and why it matters).\n\
- File: why (max. 12 words: what the change in it does) and level: critical (security, auth, money, data loss, important business logic, other changes that must not break), high (likely needs a careful review), medium (worth a look), low (docs, tests, trivial or generated).\n\
- Use every path below exactly once, spelled as given. 0 notes, 0 text outside the result.\n\n",
    );

    for file in files {
        let _ = writeln!(
            text,
            "## {} ({}, +{} −{})",
            file.path,
            file.cat.label(),
            file.add,
            file.del
        );

        append_file_hunks(per, file, &mut text);

        if file.binary {
            text.push_str("(binary)\n");
        }

        text.push('\n');
    }

    text
}

fn append_hunk_lines(per: usize, text: &mut String, shown: &mut usize, hunk: &super::Hunk) -> bool {
    for line in hunk.lines.iter().filter(|line| line.kind != Kind::Ctx) {
        if *shown == per {
            text.push_str("…\n");
            return true;
        }

        text.push(if line.kind == Kind::Add { '+' } else { '-' });
        text.push_str(&line.text.chars().take(160).collect::<String>());
        text.push('\n');
        *shown += 1;
    }

    false
}

fn append_hunk_context(text: &mut String, hunk: &super::Hunk) {
    if !hunk.ctx.is_empty() {
        let _ = writeln!(text, "@@ {}", hunk.ctx);
    }
}

/// The result's schema (strict: Codex wants every property required, 0 extra ones).
fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {"groups": {"type": "array", "items": {"type": "object", "additionalProperties": false, "properties": {
            "title": {"type": "string"}, "why": {"type": "string"},
            "files": {"type": "array", "items": {"type": "object", "additionalProperties": false, "properties": {
                "path": {"type": "string"}, "why": {"type": "string"},
                "level": {"type": "string", "enum": ["critical", "high", "medium", "low"]}}, "required": ["path", "why", "level"]}}},
            "required": ["title", "why", "files"]}}},
        "required": ["groups"]
    })
}

/// 1st line of S, trimmed (a model note after a newline goes).
fn line1(text: &str) -> String {
    text.lines().next().unwrap_or_default().trim().to_string()
}

/// Groups of Claude Code's JSON output OUT (`structured_output`, else `result` as JSON), only
/// paths of FILES, each once.
pub(crate) fn parse(out: &str, files: &[FileDiff]) -> Result<Vec<Group>, String> {
    let value: Value = serde_json::from_str(out.trim()).map_err(|error| format!("rank: {error}"))?;

    if value["is_error"] == true {
        return Err(format!("rank: {}", line1(value["result"].as_str().unwrap_or("error"))));
    }

    let data = if value["structured_output"].is_object() {
        value["structured_output"].clone()
    } else {
        serde_json::from_str::<Value>(value["result"].as_str().unwrap_or_default())
            .map_err(|_| "rank: 0 structured output".to_string())?
    };

    let mut seen = BTreeSet::new();
    let groups: Vec<Group> = data["groups"]
        .as_array()
        .ok_or("rank: 0 groups")?
        .iter()
        .map(|group| Group {
            title: line1(group["title"].as_str().unwrap_or_default()),
            why: line1(group["why"].as_str().unwrap_or_default()),
            files: group["files"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|file| {
                    let path = file["path"].as_str()?.trim();
                    let level = Level::parse(file["level"].as_str().unwrap_or_default())
                        .map(|level| level.label().to_string())
                        .unwrap_or_default();

                    (files.iter().any(|known| known.path == path) && seen.insert(path.to_string())).then(|| RFile {
                        path: path.to_string(),
                        why: line1(file["why"].as_str().unwrap_or_default()),
                        level,
                    })
                })
                .collect(),
        })
        .filter(|group| !group.files.is_empty())
        .collect();

    if groups.is_empty() {
        Err("rank: 0 known paths".into())
    } else {
        Ok(groups)
    }
}

const SYSTEM: &str =
    "You order and classify code changes for a human review. Output only the structured result: 0 notes, 0 extra text.";

/// Rank and classify FILES of DIR with BY (blocking: run it on a thread). Stores the result for ID.
pub(crate) fn run(id: &str, dir: &Path, files: &[FileDiff], by: &Runner) -> Result<Rank, String> {
    if files.len() > MAX_FILES {
        return Err(format!("classify: {} files, max. {MAX_FILES}", files.len()));
    }

    let out = match by.harness.as_str() {
        "claude" => {
            let mut command = Command::new("claude");

            command.current_dir(dir).args([
                "-p",
                "--tools",
                "",
                "--strict-mcp-config",
                "--setting-sources",
                "",
                "--disable-slash-commands",
                "--no-session-persistence",
            ]);

            if let Some(model) = &by.model {
                command.args(["--model", model]);
            }

            let out = command
                .args(["--system-prompt", SYSTEM])
                .args(["--output-format", "json", "--json-schema", &schema().to_string()])
                .arg(prompt(files))
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .map_err(|error| format!("classify: claude: {error}"))?;

            String::from_utf8_lossy(&out.stdout).into_owned()
        }
        "codex" => codex(dir, files, by.model.as_deref())?,
        harness => {
            return Err(format!(
                "classify: {harness} has 0 one-shot mode · setting classify: a claude or codex agent"
            ));
        }
    };

    let groups = parse(&out, files)?;
    let rank = Rank {
        key: key(files),
        model: by.label.clone(),
        groups,
    };

    store(id, &rank);

    Ok(rank)
}

/// `codex exec` read-only with the schema; its last message as Claude Code's JSON shape.
fn codex(dir: &Path, files: &[FileDiff], model: Option<&str>) -> Result<String, String> {
    let tmp = std::env::temp_dir().join(format!("codehead-classify-{}-{}", std::process::id(), key(files)));
    let _ = std::fs::create_dir_all(&tmp);
    let (schema_file, last) = (tmp.join("schema.json"), tmp.join("last.txt"));

    std::fs::write(&schema_file, schema().to_string()).map_err(|error| format!("classify: {error}"))?;
    let mut command = Command::new("codex");

    command
        .current_dir(dir)
        .args([
            "exec",
            "--skip-git-repo-check",
            "--sandbox",
            "read-only",
            "--output-schema",
        ])
        .arg(&schema_file)
        .arg("--output-last-message")
        .arg(&last);

    if let Some(model) = model {
        command.args(["-m", model]);
    }

    let status = command
        .arg(format!("{SYSTEM}\n\n{}", prompt(files)))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| format!("classify: codex: {error}"))?;
    let text = std::fs::read_to_string(&last).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&tmp);

    if !status.success() && text.trim().is_empty() {
        return Err(format!("classify: codex exited with {status}"));
    }

    Ok(json!({"is_error": false, "result": text}).to_string())
}

#[cfg(test)]
#[path = "rank_tests.rs"]
mod tests;

fn append_file_hunks(per: usize, file: &FileDiff, text: &mut String) {
    let mut shown = 0;

    'hunks: for hunk in &file.hunks {
        append_hunk_context(text, hunk);

        if append_hunk_lines(per, text, &mut shown, hunk) {
            break 'hunks;
        }
    }
}
