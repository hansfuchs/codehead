//! Agents: 1 TOML file per agent in `<shared>/agents/<name>.toml` (`engine::shared_dir`). A file a person edits by
//! hand is the same file codehead writes.
//!
//! ```toml
//! harness = "claude"
//! model = "opus"
//! effort = "high"
//! mode = "autopilot"   # autopilot | edits | supervised | plan
//! color = "blue"
//! style = "mecha"
//! voice = "…"
//! prompt = """…"""        # instructions
//! sprite = { id = "regi:31 eyes=3", rows = [...] }
//! ```

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Agent {
    /// The file name, not stored in the file.
    #[serde(skip)]
    pub name: String,
    #[serde(default = "claude")]
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default = "autopilot")]
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Output style id (file stem in the styles dir).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    /// Tone and persona, on top of the style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    /// Instructions: the agent's own system prompt part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Avatar: its id (`regi:31 eyes=3 speed=0.5`), the 4 rows of its rest frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sprite: Option<SpriteRec>,
    /// The ACP command for this agent (any harness but claude), instead of the harness's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Skills the agent loads (names; empty: all of its harness).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    /// codehead MCP servers the agent gets (names in `mcp.toml`; empty: 0).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp: Vec<String>,
    /// Archived agents keep their file (old threads still show them) but leave every list.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archived: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct SpriteRec {
    pub id: String,
    pub rows: Vec<String>,
}

fn claude() -> String {
    "claude".into()
}

fn autopilot() -> String {
    "autopilot".into()
}

pub(crate) fn agents_dir() -> PathBuf {
    super::shared_dir().join("agents")
}

/// Agent names: a letter, then letters, digits, `-` or `_`; max. 32 chars.
pub(crate) fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();

    chars.next().is_some_and(|first| first.is_ascii_alphabetic())
        && name.len() <= 32
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_')
}

/// Agent NAME (archived too).
pub(crate) fn get(name: &str) -> Option<Agent> {
    if !valid_name(name) {
        return None;
    }

    let text = std::fs::read_to_string(agents_dir().join(format!("{name}.toml"))).ok()?;
    let mut agent: Agent = toml::from_str(&text).ok()?;

    agent.name = name.to_string();
    Some(agent)
}

/// Every agent, archived too, by name.
pub(crate) fn all() -> Vec<Agent> {
    let Ok(entries) = std::fs::read_dir(agents_dir()) else {
        return Vec::new();
    };

    let mut agents: Vec<Agent> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();

            if path.extension()? != "toml" {
                return None;
            }

            get(&path.file_stem()?.to_string_lossy())
        })
        .collect();

    agents.sort_by(|left, right| left.name.cmp(&right.name));
    agents
}

/// Active agents, by name.
pub(crate) fn active() -> Vec<Agent> {
    all().into_iter().filter(|agent| !agent.archived).collect()
}

/// Write AGENT (create or replace).
pub(crate) fn save(agent: &Agent) -> Result<(), String> {
    if !valid_name(&agent.name) {
        return Err(format!(
            "agent name {:?}: a letter, then letters, digits, - or _ (max. 32)",
            agent.name
        ));
    }

    let text = toml::to_string_pretty(agent).map_err(|error| error.to_string())?;
    let path = agents_dir().join(format!("{}.toml", agent.name));

    super::write_atomic(&path, text.as_bytes()).map_err(|error| format!("{}: {error}", path.display()))
}

/// Rename agent OLD to NEW: the file, and the agent's place on every thread.
pub(crate) fn rename(old: &str, new: &str) -> Result<(), String> {
    if !valid_name(new) {
        return Err(format!(
            "agent name {new:?}: a letter, then letters, digits, - or _ (max. 32)"
        ));
    }

    let dir = agents_dir();

    if dir.join(format!("{new}.toml")).exists() {
        return Err(format!("agent {new} exists"));
    }

    std::fs::rename(dir.join(format!("{old}.toml")), dir.join(format!("{new}.toml")))
        .map_err(|error| error.to_string())?;

    for thread in super::store::all().into_iter().filter(|thread| {
        thread.agents.iter().any(|agent| agent == old)
            || thread.sessions.contains_key(old)
            || thread.flows.contains_key(old)
    }) {
        let _ = super::store::update(&thread.id, |stored| stored.rename_agent(old, new));
    }

    Ok(())
}

/// Claude Code `--permission-mode` of codehead mode MODE.
pub(crate) fn permission_mode(mode: &str) -> &'static str {
    match mode {
        "edits" => "acceptEdits",
        "supervised" => "default",
        "plan" => "plan",
        // autopilot: Claude Code's auto mode, its classifier refuses the risky calls
        _ => "auto",
    }
}

#[cfg(test)]
#[path = "roster_tests.rs"]
mod tests;
