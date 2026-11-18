//! Review order and review levels of the changes view by an agent (setting `classify`, empty:
//! the default agent; 0 agent: the `rank` model, default `claude:haiku`): files in groups that
//! do 1 thing, groups in reading order (schema / API → logic → glue → UI → docs → tests →
//! generated), a title + why per group, a why and a level (critical · high · medium · low) per
//! file. 1 call per set of changed paths (`<thread dir>/rank.json`): Claude Code with a replaced
//! system prompt, 0 tools, 0 MCP servers, structured output; Codex: `codex exec`, read-only.

use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::changes::{Cat, FileDiff, Kind};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RFile {
    pub path: String,
    pub why: String,
    /// critical, high, medium or low ("": an older rank, path rules decide).
    #[serde(default)]
    pub level: String,
}

/// How much a file's change needs a review: the tabs of the changes view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Critical,
    High,
    Medium,
    Low,
}

impl Level {
    pub const ALL: [Level; 4] = [Level::Critical, Level::High, Level::Medium, Level::Low];

    pub fn label(self) -> &'static str {
        match self {
            Level::Critical => "critical",
            Level::High => "high",
            Level::Medium => "medium",
            Level::Low => "low",
        }
    }

    pub fn parse(s: &str) -> Option<Level> {
        Level::ALL.into_iter().find(|l| l.label().eq_ignore_ascii_case(s.trim()))
    }
}

/// The level of F by path rules (before or without the agent's call).
pub fn guess(f: &FileDiff) -> Level {
    let p = f.path.to_ascii_lowercase();
    let risky = ["auth", "security", "crypto", "password", "secret", "token", "permission", "acl", "payment", "billing", "migration", "sandbox", "guard", "session"];
    match f.cat {
        Cat::Docs | Cat::Tests | Cat::Generated => Level::Low,
        _ if f.binary => Level::Low,
        _ if risky.iter().any(|w| p.contains(w)) => Level::Critical,
        Cat::Schema => Level::High,
        Cat::Logic if f.add + f.del >= 30 => Level::High,
        _ => Level::Medium,
    }
}

/// Who classifies: an agent's harness and model (None: the harness's default model).
#[derive(Clone, Debug, PartialEq)]
pub struct Runner {
    pub label: String,
    pub harness: String,
    pub model: Option<String>,
}

/// The classifier: setting `classify` (an agent; `off`: path rules), else the default agent
/// (`agent`, else amon, else the 1st agent), else the `rank` model (`off`: path rules).
pub fn runner() -> Option<Runner> {
    let set = crate::config::classify();
    // `classify = off`, or unset with `rank = off`: path rules
    if set.as_deref() == Some("off") || (set.is_none() && crate::config::rank_model().is_none()) {
        return None;
    }
    let name = set.or_else(crate::config::agent).or_else(|| {
        let all = crate::engine::roster::active();
        all.iter().find(|a| a.name == "amon").or(all.first()).map(|a| a.name.clone())
    });
    if let Some(a) = name.as_deref().and_then(crate::engine::roster::get) {
        let model = a.model.clone().filter(|m| !m.is_empty() && m != "default");
        return Some(Runner { label: a.name.clone(), harness: a.harness.clone(), model });
    }
    crate::config::rank_model().map(|m| Runner { label: format!("claude/{m}"), harness: "claude".into(), model: Some(m) })
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
const MAX_FILES: usize = 400;

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
- File: why (max. 12 words: what the change in it does) and level: critical (security, auth, money, data loss, important business logic, other changes that must not break), high (likely needs a careful review), medium (worth a look), low (docs, tests, trivial or generated).\n\
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
                    let level = Level::parse(f["level"].as_str().unwrap_or_default()).map(|l| l.label().to_string()).unwrap_or_default();
                    (files.iter().any(|x| x.path == p) && seen.insert(p.to_string())).then(|| RFile { path: p.to_string(), why: line1(f["why"].as_str().unwrap_or_default()), level })
                })
                .collect(),
        })
        .filter(|g| !g.files.is_empty())
        .collect();
    if groups.is_empty() { Err("rank: 0 known paths".into()) } else { Ok(groups) }
}

const SYSTEM: &str = "You order and classify code changes for a human review. Output only the structured result: 0 notes, 0 extra text.";

/// Rank and classify FILES of DIR with BY (blocking: run it on a thread). Stores the result for ID.
pub fn run(id: &str, dir: &Path, files: &[FileDiff], by: &Runner) -> Result<Rank, String> {
    if files.len() > MAX_FILES {
        return Err(format!("classify: {} files, max. {MAX_FILES}", files.len()));
    }
    let out = match by.harness.as_str() {
        "claude" => {
            let mut c = Command::new("claude");
            c.current_dir(dir).args(["-p", "--tools", "", "--strict-mcp-config", "--setting-sources", "", "--disable-slash-commands", "--no-session-persistence"]);
            if let Some(m) = &by.model {
                c.args(["--model", m]);
            }
            let out = c
                .args(["--system-prompt", SYSTEM])
                .args(["--output-format", "json", "--json-schema", &schema().to_string()])
                .arg(prompt(files))
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .map_err(|e| format!("classify: claude: {e}"))?;
            String::from_utf8_lossy(&out.stdout).into_owned()
        }
        "codex" => codex(dir, files, by.model.as_deref())?,
        h => return Err(format!("classify: {h} has 0 one-shot mode · setting classify: a claude or codex agent")),
    };
    let groups = parse(&out, files)?;
    let r = Rank { key: key(files), model: by.label.clone(), groups };
    store(id, &r);
    Ok(r)
}

/// `codex exec` read-only with the schema; its last message as Claude Code's JSON shape.
fn codex(dir: &Path, files: &[FileDiff], model: Option<&str>) -> Result<String, String> {
    let tmp = std::env::temp_dir().join(format!("codehead-classify-{}-{}", std::process::id(), key(files)));
    let _ = std::fs::create_dir_all(&tmp);
    let (schema_f, last) = (tmp.join("schema.json"), tmp.join("last.txt"));
    std::fs::write(&schema_f, schema().to_string()).map_err(|e| format!("classify: {e}"))?;
    let mut c = Command::new("codex");
    c.current_dir(dir).args(["exec", "--skip-git-repo-check", "--sandbox", "read-only", "--output-schema"]).arg(&schema_f).arg("--output-last-message").arg(&last);
    if let Some(m) = model {
        c.args(["-m", m]);
    }
    let st = c.arg(format!("{SYSTEM}\n\n{}", prompt(files))).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().map_err(|e| format!("classify: codex: {e}"))?;
    let text = std::fs::read_to_string(&last).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&tmp);
    if !st.success() && text.trim().is_empty() {
        return Err(format!("classify: codex exited with {st}"));
    }
    Ok(json!({"is_error": false, "result": text}).to_string())
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
            {"title": "Core change", "why": "a to b\n\nNote: ignore me", "files": [{"path": "src/a.rs", "why": "swaps a", "level": "Critical"}, {"path": "nope.rs", "why": "x"}]},
            {"title": "Tests", "why": "covers it", "files": [{"path": "t/x.rs", "why": "c to d"}, {"path": "src/a.rs", "why": "dup"}]},
            {"title": "Empty", "why": "", "files": []}
        ]}}).to_string();
        let g = parse(&out, &files()).unwrap();
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].why, "a to b");
        assert_eq!(g[0].files, [RFile { path: "src/a.rs".into(), why: "swaps a".into(), level: "critical".into() }]);
        assert_eq!(g[1].files[0].level, "", "0 level: path rules decide");
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

    #[test]
    fn path_rules_guess_a_level_and_the_schema_asks_for_one() {
        let lv = |d: &str| guess(&crate::changes::parse(d)[0]);
        assert_eq!(lv("diff --git a/src/auth/login.rs b/src/auth/login.rs\n--- a/x\n+++ b/src/auth/login.rs\n@@ -1 +1 @@\n-a\n+b\n"), Level::Critical);
        assert_eq!(lv("diff --git a/README.md b/README.md\n--- a/x\n+++ b/README.md\n@@ -1 +1 @@\n-a\n+b\n"), Level::Low);
        assert_eq!(lv("diff --git a/src/ui/x.css b/src/ui/x.css\n--- a/x\n+++ b/src/ui/x.css\n@@ -1 +1 @@\n-a\n+b\n"), Level::Medium);
        assert_eq!(Level::parse(" High "), Some(Level::High));
        assert!(schema().to_string().contains("\"level\""));
        // the setting: an agent's name, off or empty
        for ok in ["ada", "off", ""] {
            assert!(crate::config::validate("classify".into(), ok).is_ok(), "{ok}");
        }
        assert!(crate::config::validate("classify".into(), "claude:haiku").is_err());
    }

    /// Live: CODEHEAD_RANK_DIR=<repo> cargo test rank_live -- --ignored --nocapture
    #[test]
    #[ignore]
    fn rank_live() {
        let dir = PathBuf::from(std::env::var("CODEHEAD_RANK_DIR").unwrap());
        let c = crate::changes::load(&dir).unwrap();
        let r = run("rank-live-0000", &dir, &c.files, &runner().expect("a classifier")).unwrap();
        for g in &r.groups {
            println!("## {} — {}", g.title, g.why);
            for f in &g.files {
                println!("  {} [{}] — {}", f.path, f.level, f.why);
            }
        }
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("rank-live-0000"));
    }
}
