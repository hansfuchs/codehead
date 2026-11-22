//! Agents: 1 TOML file per agent in `<shared>/agents/<name>.toml` (engine::shared_dir). A file a person edits by
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
pub struct Agent {
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
    /// Flow roles the agent takes by default (`reviewer`, ...): a flow maps them to it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,
    /// Archived agents keep their file (old threads still show them) but leave every list.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archived: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SpriteRec {
    pub id: String,
    pub rows: Vec<String>,
}

fn claude() -> String {
    "claude".into()
}

fn autopilot() -> String {
    "autopilot".into()
}

pub fn agents_dir() -> PathBuf {
    super::shared_dir().join("agents")
}

/// Agent names: a letter, then letters, digits, `-` or `_`; max. 32 chars.
pub fn valid_name(name: &str) -> bool {
    let mut c = name.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic()) && name.len() <= 32 && c.all(|x| x.is_ascii_alphanumeric() || x == '-' || x == '_')
}

/// Agent NAME (archived too).
pub fn get(name: &str) -> Option<Agent> {
    if !valid_name(name) {
        return None;
    }
    let text = std::fs::read_to_string(agents_dir().join(format!("{name}.toml"))).ok()?;
    let mut a: Agent = toml::from_str(&text).ok()?;
    a.name = name.to_string();
    Some(a)
}

/// Every agent, archived too, by name.
pub fn all() -> Vec<Agent> {
    let Ok(rd) = std::fs::read_dir(agents_dir()) else { return Vec::new() };
    let mut v: Vec<Agent> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension()? != "toml" {
                return None;
            }
            get(&p.file_stem()?.to_string_lossy())
        })
        .collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

/// Active agents, by name.
pub fn active() -> Vec<Agent> {
    all().into_iter().filter(|a| !a.archived).collect()
}

/// Write agent A (create or replace).
pub fn save(a: &Agent) -> Result<(), String> {
    if !valid_name(&a.name) {
        return Err(format!("agent name {:?}: a letter, then letters, digits, - or _ (max. 32)", a.name));
    }
    let text = toml::to_string_pretty(a).map_err(|e| e.to_string())?;
    let path = agents_dir().join(format!("{}.toml", a.name));
    super::write_atomic(&path, text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))
}

/// Rename agent OLD to NEW: the file, and the agent's place on every thread.
pub fn rename(old: &str, new: &str) -> Result<(), String> {
    if !valid_name(new) {
        return Err(format!("agent name {new:?}: a letter, then letters, digits, - or _ (max. 32)"));
    }
    let dir = agents_dir();
    if dir.join(format!("{new}.toml")).exists() {
        return Err(format!("agent {new} exists"));
    }
    std::fs::rename(dir.join(format!("{old}.toml")), dir.join(format!("{new}.toml"))).map_err(|e| e.to_string())?;
    for t in super::store::all().into_iter().filter(|t| t.agents.iter().any(|a| a == old) || t.sessions.contains_key(old)) {
        let _ = super::store::update(&t.id, |t| {
            for a in t.agents.iter_mut().filter(|a| *a == old) {
                *a = new.to_string();
            }
            if let Some(s) = t.sessions.remove(old) {
                t.sessions.insert(new.to_string(), s);
            }
        });
    }
    Ok(())
}

/// Claude Code `--permission-mode` of codehead mode MODE.
pub fn permission_mode(mode: &str) -> &'static str {
    match mode {
        "edits" => "acceptEdits",
        "supervised" => "default",
        "plan" => "plan",
        // autopilot: Claude Code's auto mode, its classifier refuses the risky calls
        _ => "auto",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_modes() {
        assert!(valid_name("ada"));
        assert!(valid_name("r2-d2_x"));
        assert!(!valid_name("2pac"));
        assert!(!valid_name("a/b"));
        assert!(!valid_name(""));
        assert_eq!(permission_mode("autopilot"), "auto");
        assert_eq!(permission_mode("supervised"), "default");
    }

    #[test]
    fn toml_keeps_unset_keys_out() {
        let a = Agent { name: "ada".into(), harness: "claude".into(), mode: "autopilot".into(), model: Some("opus".into()), ..Default::default() };
        let text = toml::to_string_pretty(&a).unwrap();
        assert!(text.contains("model = \"opus\""));
        assert!(!text.contains("effort"));
        assert!(!text.contains("archived"));
        let back: Agent = toml::from_str(&text).unwrap();
        assert_eq!(back.model.as_deref(), Some("opus"));
    }
}
