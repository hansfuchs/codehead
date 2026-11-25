//! Agents for the screens: list, create, edit, delete (archive), over the TOML roster
//! (`engine::roster`). Delete hands the agent's threads to the default agent.

use ratatui::style::Color;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::domain::palette as theme;
use crate::engine::{client, roster, store};

/// Agent colors. 0 yellow: yellow is a blocked agent (a question waits on the owner).
pub(crate) const COLORS: [&str; 6] = ["red", "orange", "green", "blue", "purple", "gray"];
pub(crate) const MODES: [&str; 4] = ["autopilot", "edits", "supervised", "plan"];
/// Claude Code effort levels (`claude --effort`). "default": key unset, Claude Code decides.
pub(crate) const EFFORTS: [&str; 6] = ["default", "low", "medium", "high", "xhigh", "max"];

/// Claude Code's effort when `effortLevel` is unset (seen in its requests: `output_config.effort`).
pub(crate) const DEFAULT_EFFORT: &str = "high";

/// Latest model per alias (`claude --model opus`).
const ALIASES: [(&str, &str); 4] = [
    ("fable", "Fable 5.2"),
    ("opus", "Opus 5.5"),
    ("sonnet", "Sonnet 5.5"),
    ("haiku", "Haiku 5.5"),
];

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct Agent {
    pub name: String,
    pub harness: String,
    pub model: Option<String>,
    pub color: Option<String>,
    pub mode: String,
    /// The profile as 1 text (style, sprite, voice, instructions; see `compose`): what the
    /// agent editor shows. On disk the parts are fields of the agent's TOML file.
    pub prompt: Option<String>,
    /// Last activity on any thread, in seconds since the epoch.
    pub last_seen: Option<i64>,
}

impl Agent {
    /// `claude` or `claude/opus`.
    pub(crate) fn harness_model(&self) -> String {
        match &self.model {
            Some(message) if !message.is_empty() => format!("{}/{message}", self.harness),
            _ => self.harness.clone(),
        }
    }
}

/// A thread that 1 agent worked on.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct Use {
    pub thread_id: String,
    pub title: String,
    pub state: String,
    /// The agent has a live session on the thread (its name, else empty).
    pub live: Vec<String>,
    /// The agent is mid-turn on the thread (its name, else empty).
    pub busy: Vec<String>,
    /// Live sessions of other agents.
    pub others_live: usize,
    pub dir: String,
}

/// Agent colors.
pub(crate) fn color(name: Option<&str>) -> Color {
    match name {
        Some("purple") => Color::Magenta,
        Some("red") => Color::Red,
        Some("green") => Color::Green,
        Some("blue") => Color::Blue,
        Some("gray") => Color::Gray,
        Some("orange") => Color::Indexed(208),
        // yellow (gone: the blocked color), unknown: the accent
        _ => theme::pal().accent,
    }
}

/// Active (not archived) agents, by name: the roster files only (boot, menu: 0 thread scan).
pub(crate) fn cached() -> Vec<Agent> {
    roster_only()
}

fn from_roster(result: &roster::Agent, seen: &HashMap<String, i64>) -> Agent {
    let profile = Profile {
        style: result.style.clone(),
        voice: result.voice.clone().unwrap_or_default(),
        prompt: result.prompt.clone().unwrap_or_default(),
        sprite: result
            .sprite
            .as_ref()
            .and_then(|text_value| crate::domain::sprites::from_id(&text_value.id)),
    };
    let text = compose(&profile, &[], &result.harness, None);

    Agent {
        name: result.name.clone(),
        harness: result.harness.clone(),
        model: result.model.clone(),
        color: result.color.clone(),
        mode: result.mode.clone(),
        prompt: (!text.is_empty()).then_some(text),
        last_seen: seen.get(&result.name).copied(),
    }
}

/// Last activity per agent: the newest thread it is on.
fn last_seen() -> HashMap<String, i64> {
    let mut message: HashMap<String, i64> = HashMap::new();

    for thread in store::all() {
        for agent in &thread.agents {
            let at = (thread.updated / 1000) as i64;
            let error = message.entry(agent.clone()).or_insert(at);

            *error = (*error).max(at);
        }
    }

    message
}

pub(crate) fn load() -> Vec<Agent> {
    let seen = last_seen();

    roster::active()
        .iter()
        .map(|result| from_roster(result, &seen))
        .collect()
}

/// `load` without `last_seen` (that reads every thread.json): the roster files only. For screen
/// opens and ticks; the agents tab reads the full rows in the background.
pub(crate) fn roster_only() -> Vec<Agent> {
    roster::active()
        .iter()
        .map(|result| from_roster(result, &HashMap::new()))
        .collect()
}

/// Config agent ($`CODEHEAD_AGENT`, codehead.toml), else amon, else the first active agent.
pub(crate) fn default_agent(all: &[Agent]) -> Option<String> {
    if let Some(agent_value) = crate::infra::config::agent() {
        return Some(agent_value);
    }

    all.iter()
        .find(|agent_2| agent_2.name == "amon")
        .or(all.first())
        .map(|agent_3| agent_3.name.clone())
}

/// Threads per agent name (open + done).
pub(crate) fn usage() -> HashMap<String, Vec<Use>> {
    let mut map: HashMap<String, Vec<Use>> = HashMap::new();

    for thread in store::all() {
        let (live, busy) = if client::alive(&thread.id) {
            client::state(&thread.id)
        } else {
            Default::default()
        };

        let dir = thread.work_dir().display().to_string();

        for agent in &thread.agents {
            let mine = |value: &Vec<String>| value.iter().filter(|x| *x == agent).cloned().collect::<Vec<_>>();

            map.entry(agent.clone()).or_default().push(Use {
                thread_id: thread.id.clone(),
                title: thread.title.lines().next().unwrap_or_default().to_string(),
                state: thread.state.clone(),
                live: mine(&live),
                busy: mine(&busy),
                others_live: live.iter().filter(|x| *x != agent).count(),
                dir: dir.clone(),
            });
        }
    }

    map
}

/// Facts about agent NAME beyond its form: its file, threads, spend.
pub(crate) fn show_extra(name: &str) -> Vec<String> {
    let mut out = vec![format!(
        "file: {}",
        roster::agents_dir().join(format!("{name}.toml")).display()
    )];

    if let Some(result) = roster::get(name)
        && let Some(height) = crate::engine::harnesses::get(&result.harness)
    {
        let cmd = crate::engine::harnesses::acp_command(height, result.command.as_deref())
            .map_or_else(|| "claude -p (stream-json)".into(), |character| character.join(" "));

        let state = if crate::engine::harnesses::on_path(height.bin) {
            "installed".to_string()
        } else {
            format!("missing: {}", height.install)
        };

        out.push(format!("harness: {} · {state}", height.label));
        out.push(format!("runs: {cmd}"));
    }

    let (mut threads, mut usd) = (0usize, 0.0f64);

    for thread_value in store::all()
        .iter()
        .filter(|thread| thread.agents.iter().any(|agent_value| agent_value == name))
    {
        threads += 1;
        usd += store::events(&thread_value.id)
            .iter()
            .filter(|error| error.agent.as_deref() == Some(name))
            .map(turn_error_details)
            .sum::<f64>();
    }

    out.push(format!("threads: {threads}"));
    out.push(format!("spend: ${usd:.2}"));

    out
}

fn turn_error_details(error_value: &store::Event) -> f64 {
    if let store::Kind::TurnEnd { cost, .. } = error_value.kind {
        cost
    } else {
        0.0
    }
}

pub(crate) struct Spec {
    pub name: String,
    pub harness: String,
    pub color: Option<String>,
    pub mode: String,
    pub prompt: String,
    /// codehead MCP servers of the agent (settings › mcp servers).
    pub mcp: Vec<String>,
}

/// `claude/opus` → (claude, Some(opus)).
fn split_harness(height: &str) -> (String, Option<String>) {
    match height.split_once('/') {
        Some((height_value, message)) => (
            height_value.to_string(),
            Some(message.to_string()).filter(|message_value| !message_value.is_empty() && message_value != "default"),
        ),
        None if height.is_empty() => ("claude".into(), None),
        None => (height.to_string(), None),
    }
}

/// The profile text of the editor → the fields of R.
fn set_profile(result: &mut roster::Agent, text: &str) {
    let path = parse_profile(Some(text), None);

    result.style = path.style;
    result.voice = Some(path.voice).filter(|value| !value.trim().is_empty());
    result.prompt = Some(path.prompt).filter(|value_value| !value_value.trim().is_empty());
    result.sprite = path.sprite.map(|text_value| roster::SpriteRec {
        id: text_value.id.clone(),
        rows: text_value.rows.to_vec(),
    });
}

/// Harness H is one codehead drives.
fn known(height: &str) -> Result<(), String> {
    match crate::engine::harnesses::get(height) {
        Some(_) => Ok(()),
        None => Err(format!(
            "harness {height:?}: one of {}",
            crate::engine::harnesses::ALL
                .iter()
                .map(|height_value| height_value.name)
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

pub(crate) fn create(text: &Spec) -> Result<(), String> {
    if roster::get(&text.name).is_some() {
        return Err(format!("agent {} exists", text.name));
    }

    let (harness, model) = split_harness(&text.harness);

    known(&harness)?;
    let mut result = roster::Agent {
        name: text.name.clone(),
        harness,
        model,
        mode: text.mode.clone(),
        color: text.color.clone(),
        mcp: text.mcp.clone(),
        ..Default::default()
    };

    set_profile(&mut result, &text.prompt);

    roster::save(&result)
}

/// Pass only what changed.
pub(crate) fn edit(old: &Agent, text: &Spec) -> Result<(), String> {
    if text.name != old.name {
        roster::rename(&old.name, &text.name)?;
    }

    let mut result = roster::get(&text.name).ok_or_else(|| format!("no agent {}", text.name))?;

    if !text.harness.is_empty() && text.harness != old.harness_model() {
        let (height, message) = split_harness(&text.harness);

        known(&height)?;
        (result.harness, result.model) = (height, message);
    }

    result.mode.clone_from(&text.mode);
    result.mcp.clone_from(&text.mcp);

    if text.color.is_some() {
        result.color.clone_from(&text.color);
    }

    set_profile(&mut result, &text.prompt);

    roster::save(&result)
}

/// Changes a live session keeps from its start: harness, model, mode, prompt.
pub(crate) fn profile_changed(old: &Agent, new: &Agent) -> bool {
    (&old.harness, &old.model, &old.mode, &old.prompt) != (&new.harness, &new.model, &new.mode, &new.prompt)
}

/// What `restart` did.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Restart {
    /// Sessions ended (the next message starts the new profile, the conversation kept), and
    /// ended where the harness changed (the next message starts fresh).
    pub resumed: usize,
    pub stopped: usize,
    /// Mid-turn sessions: left as they are (old profile until their turn ends).
    pub busy: Vec<String>,
    pub failed: Vec<String>,
}

/// After an edit: the idle sessions of AGENT end, so its next message runs the new profile.
/// Busy sessions stay: ending them would cut a turn or drop a pending question.
pub(crate) fn restart(agent: &Agent, same_harness: bool, uses: &[Use], _logs: &Path) -> Restart {
    let mut result = Restart::default();

    for user_value in uses.iter().filter(|user| !user.live.is_empty()) {
        let short: String = user_value.thread_id.chars().take(12).collect();

        if !user_value.busy.is_empty() {
            result.busy.push(short);
            continue;
        }

        match client::send(
            &user_value.thread_id,
            &serde_json::json!({"op": "reload", "agent": agent.name, "fresh": !same_harness}),
        ) {
            Ok(_) if same_harness => result.resumed += 1,
            Ok(_) => result.stopped += 1,
            Err(error) => result.failed.push(format!("{short}: {error}")),
        }
    }

    result
}

// ---------- profile: base style + sprite + voice + instructions, composed into the session prompt ----------

const VOICE_HEAD: &str = "# Voice";
const VOICE_INTRO: &str = "Your own voice, on top of the output style: its structure and rules still apply. On conflict, this voice wins for tone, word choice and persona.";
const PROMPT_HEAD: &str = "# Instructions";
const SPRITE_HEAD: &str = "# Sprite";
/// Steps a harness can't do (a tool of another harness): hand off, or name the harness for codehead.
/// codehead finds `needs harness: <harness>` in a message and suggests agents with that harness.
const HANDOFF: &str = "# Handoff

A step needs a tool or access that your harness lacks. Example: Claude Code tools (Artifact, ArtifactComments, ArtifactData, claude.ai connectors) in Codex. Never stop at \"tool unavailable\":

1. An agent on the thread runs a harness with that tool (`list_agents` of the `codehead` MCP server shows harnesses): hand the step to that agent with the `send` tool. Give the file paths, URLs and the exact action.
2. 0 such agent on the thread: report the step as blocked, with the cause and 1 line in exactly this form: `needs harness: <harness> (<tool>)`, e.g. `needs harness: claude (Artifact tool)`. codehead then suggests agents with that harness to the owner.";
const SPRITE_INTRO: &str = "Your avatar in codehead, a 7x4 glyph portrait:";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Profile {
    /// Style id (file stem in codehead/output-styles), None = no base style.
    pub style: Option<String>,
    pub voice: String,
    pub prompt: String,
    /// None = 0 saved: codehead shows a stable pool pick by name.
    pub sprite: Option<crate::domain::sprites::Sprite>,
}

/// Marker prefix of the prompt sections codehead writes.
const MARK: &str = "<!-- codehead:";
const STYLE_MARK: &str = "<!-- codehead:style ";

#[cfg(test)]
#[path = "agents_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "agents_restart_tests.rs"]
mod restart_tests;

#[path = "agent_models.rs"]
mod agent_models;
pub(crate) use agent_models::*;

#[path = "agent_profiles.rs"]
mod agent_profiles;
pub(crate) use agent_profiles::*;
