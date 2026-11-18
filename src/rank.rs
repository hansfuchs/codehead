//! Review order of the changes view by a model (setting `rank`, default `claude:haiku`): files
//! in groups that do 1 thing, groups in reading order (schema / API → logic → glue → UI → docs →
//! tests → generated), a title + why per group, a why per file. 1 call per set of changed paths
//! (`<thread dir>/rank.json`); 1 Claude Code call with a replaced system prompt, 0 tools,
//! 0 MCP servers, structured output.

use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::changes::{FileDiff, Kind};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RFile {
    pub path: String,
    pub why: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub title: String,
    pub why: String,
    pub files: Vec<RFile>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rank {
    /// Hash of the set of paths it ranked.
    pub key: u64,
    pub model: String,
    pub groups: Vec<Group>,
}

/// Diff lines in the prompt, all files together.
const BUDGET: usize = 600;
/// Files above this: 0 call (path rules only).
const MAX_FILES: usize = 150;

/// Key of FILES: the set of their paths (content edits keep a rank, a new or gone file renews it).
pub fn key(files: &[FileDiff]) -> u64 {
    let paths: BTreeSet<&str> = files.iter().map(|f| f.path.as_str()).collect();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    paths.hash(&mut h);
    h.finish()
}

fn path(id: &str) -> PathBuf {
    crate::engine::store::thread_dir(id).join("rank.json")
}

pub fn cached(id: &str) -> Option<Rank> {
    std::fs::read(path(id)).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

fn store(id: &str, r: &Rank) {
    if let Ok(b) = serde_json::to_vec_pretty(r) {
        let _ = crate::engine::write_atomic(&path(id), &b);
    }
}

/// The prompt for FILES: each file's path, category guess, size, and its diff cut to a share
/// of BUDGET lines.
pub fn prompt(files: &[FileDiff]) -> String {
    let per = (BUDGET / files.len().max(1)).clamp(8, 60);
    let mut s = String::from(
        "Order these changed files for a code review: the reviewer reads top to bottom.\n\
- Group files that do 1 thing together.\n\
- Groups in reading order: data model, schema, API contracts first; then business logic; then glue (handlers, wiring, config); then UI; then docs; then tests; generated files and lockfiles last.\n\
- Group: title (max. 6 words), why (1 sentence, max. 15 words: what it changes and why it matters).\n\
- File: why (max. 12 words: what the change in it does).\n\
- Use every path below exactly once, spelled as given. 0 notes, 0 text outside the result.\n\n",
    );
    for f in files {
        s.push_str(&format!("## {} ({}, +{} −{})\n", f.path, f.cat.label(), f.add, f.del));
        let mut n = 0;
        'h: for h in &f.hunks {
            if !h.ctx.is_empty() {
                s.push_str(&format!("@@ {}\n", h.ctx));
            }
            for l in h.lines.iter().filter(|l| l.kind != Kind::Ctx) {
                if n == per {
                    s.push_str("…\n");
                    break 'h;
                }
                s.push(if l.kind == Kind::Add { '+' } else { '-' });
                s.push_str(&l.text.chars().take(160).collect::<String>());
                s.push('\n');
                n += 1;
            }
        }
        if f.binary {
            s.push_str("(binary)\n");
        }
        s.push('\n');
    }
    s
}

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {"groups": {"type": "array", "items": {"type": "object", "properties": {
            "title": {"type": "string"}, "why": {"type": "string"},
            "files": {"type": "array", "items": {"type": "object", "properties": {
                "path": {"type": "string"}, "why": {"type": "string"}}, "required": ["path", "why"]}}},
            "required": ["title", "why", "files"]}}},
        "required": ["groups"]
    })
}

/// 1st line of S, trimmed (a model note after a newline goes).
fn line1(s: &str) -> String {
    s.lines().next().unwrap_or_default().trim().to_string()
}

/// Groups of Claude Code's JSON output OUT (`structured_output`, else `result` as JSON), only
/// paths of FILES, each once.
pub fn parse(out: &str, files: &[FileDiff]) -> Result<Vec<Group>, String> {
    let v: Value = serde_json::from_str(out.trim()).map_err(|e| format!("rank: {e}"))?;
    if v["is_error"] == true {
        return Err(format!("rank: {}", line1(v["result"].as_str().unwrap_or("error"))));
    }
    let data = if v["structured_output"].is_object() {
        v["structured_output"].clone()
    } else {
        serde_json::from_str::<Value>(v["result"].as_str().unwrap_or_default()).map_err(|_| "rank: 0 structured output".to_string())?
    };
    let mut seen = BTreeSet::new();
    let groups: Vec<Group> = data["groups"]
        .as_array()
        .ok_or("rank: 0 groups")?
        .iter()
        .map(|g| Group {
            title: line1(g["title"].as_str().unwrap_or_default()),
            why: line1(g["why"].as_str().unwrap_or_default()),
            files: g["files"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|f| {
                    let p = f["path"].as_str()?.trim();
                    (files.iter().any(|x| x.path == p) && seen.insert(p.to_string())).then(|| RFile { path: p.to_string(), why: line1(f["why"].as_str().unwrap_or_default()) })
                })
                .collect(),
        })
        .filter(|g| !g.files.is_empty())
        .collect();
    if groups.is_empty() { Err("rank: 0 known paths".into()) } else { Ok(groups) }
}

/// Rank FILES of DIR with MODEL (blocking: run it on a thread). Stores the result for ID.
pub fn run(id: &str, dir: &Path, files: &[FileDiff], model: &str) -> Result<Rank, String> {
    if files.len() > MAX_FILES {
        return Err(format!("rank: {} files, max. {MAX_FILES}", files.len()));
    }
    let out = Command::new("claude")
        .current_dir(dir)
        .args(["-p", "--model", model, "--tools", "", "--strict-mcp-config", "--setting-sources", "", "--disable-slash-commands", "--no-session-persistence"])
        .args(["--system-prompt", "You order code changes for a human review. Output only the structured result: 0 notes, 0 extra text."])
        .args(["--output-format", "json", "--json-schema", &schema().to_string()])
        .arg(prompt(files))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("rank: claude: {e}"))?;
    let groups = parse(&String::from_utf8_lossy(&out.stdout), files)?;
    let r = Rank { key: key(files), model: model.to_string(), groups };
    store(id, &r);
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Vec<FileDiff> {
        crate::changes::parse("diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1 +1 @@ fn main\n-a\n+b\ndiff --git a/t/x.rs b/t/x.rs\n--- a/t/x.rs\n+++ b/t/x.rs\n@@ -1 +1 @@\n-c\n+d\n")
    }

    #[test]
    fn prompt_lists_every_file_with_its_diff() {
        let p = prompt(&files());
        assert!(p.contains("## src/a.rs (logic, +1 −1)\n@@ fn main\n-a\n+b\n"), "{p}");
        assert!(p.contains("## t/x.rs"), "{p}");
    }

    #[test]
    fn parse_takes_structured_output_known_paths_once_and_1_line_each() {
        let out = json!({"type": "result", "is_error": false, "structured_output": {"groups": [
            {"title": "Core change", "why": "a to b\n\nNote: ignore me", "files": [{"path": "src/a.rs", "why": "swaps a"}, {"path": "nope.rs", "why": "x"}]},
            {"title": "Tests", "why": "covers it", "files": [{"path": "t/x.rs", "why": "c to d"}, {"path": "src/a.rs", "why": "dup"}]},
            {"title": "Empty", "why": "", "files": []}
        ]}}).to_string();
        let g = parse(&out, &files()).unwrap();
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].why, "a to b");
        assert_eq!(g[0].files, [RFile { path: "src/a.rs".into(), why: "swaps a".into() }]);
        assert_eq!(g[1].files.len(), 1);
        let err = json!({"is_error": true, "result": "Not logged in · Please run /login"}).to_string();
        assert_eq!(parse(&err, &files()).unwrap_err(), "rank: Not logged in · Please run /login");
    }

    #[test]
    fn key_changes_with_the_set_of_paths_only() {
        let f = files();
        let mut g = f.clone();
        g[0].hunks.clear();
        assert_eq!(key(&f), key(&g));
        assert_ne!(key(&f), key(&f[..1]));
    }

    /// Live: CODEHEAD_RANK_DIR=<repo> cargo test rank_live -- --ignored --nocapture
    #[test]
    #[ignore]
    fn rank_live() {
        let dir = PathBuf::from(std::env::var("CODEHEAD_RANK_DIR").unwrap());
        let c = crate::changes::load(&dir).unwrap();
        let r = run("rank-live-0000", &dir, &c.files, "haiku").unwrap();
        for g in &r.groups {
            println!("## {} — {}", g.title, g.why);
            for f in &g.files {
                println!("  {} — {}", f.path, f.why);
            }
        }
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("rank-live-0000"));
    }
}
