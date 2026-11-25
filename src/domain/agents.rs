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
            Some(model) if !model.is_empty() => format!("{}/{model}", self.harness),
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

fn from_roster(record: &roster::Agent, seen: &HashMap<String, i64>) -> Agent {
    let profile = Profile {
        style: record.style.clone(),
        voice: record.voice.clone().unwrap_or_default(),
        prompt: record.prompt.clone().unwrap_or_default(),
        sprite: record
            .sprite
            .as_ref()
            .and_then(|sprite| crate::domain::sprites::from_id(&sprite.id)),
    };
    let text = compose(&profile, &[], &record.harness, None);

    Agent {
        name: record.name.clone(),
        harness: record.harness.clone(),
        model: record.model.clone(),
        color: record.color.clone(),
        mode: record.mode.clone(),
        prompt: (!text.is_empty()).then_some(text),
        last_seen: seen.get(&record.name).copied(),
    }
}

/// Last activity per agent: the newest thread it is on.
fn last_seen() -> HashMap<String, i64> {
    let mut seen: HashMap<String, i64> = HashMap::new();

    for thread in store::all() {
        for agent in &thread.agents {
            let at = (thread.updated / 1000) as i64;
            let newest = seen.entry(agent.clone()).or_insert(at);

            *newest = (*newest).max(at);
        }
    }

    seen
}

pub(crate) fn load() -> Vec<Agent> {
    let seen = last_seen();

    roster::active()
        .iter()
        .map(|record| from_roster(record, &seen))
        .collect()
}

/// `load` without `last_seen` (that reads every thread.json): the roster files only. For screen
/// opens and ticks; the agents tab reads the full rows in the background.
pub(crate) fn roster_only() -> Vec<Agent> {
    roster::active()
        .iter()
        .map(|record| from_roster(record, &HashMap::new()))
        .collect()
}

/// Config agent ($`CODEHEAD_AGENT`, codehead.toml), else amon, else the first active agent.
pub(crate) fn default_agent(all: &[Agent]) -> Option<String> {
    if let Some(configured) = crate::infra::config::agent() {
        return Some(configured);
    }

    all.iter()
        .find(|agent| agent.name == "amon")
        .or(all.first())
        .map(|agent| agent.name.clone())
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
            let mine = |names: &Vec<String>| names.iter().filter(|name| *name == agent).cloned().collect::<Vec<_>>();

            map.entry(agent.clone()).or_default().push(Use {
                thread_id: thread.id.clone(),
                title: thread.title.lines().next().unwrap_or_default().to_string(),
                state: thread.state.clone(),
                live: mine(&live),
                busy: mine(&busy),
                others_live: live.iter().filter(|name| *name != agent).count(),
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

    if let Some(record) = roster::get(name)
        && let Some(harness) = crate::engine::harnesses::get(&record.harness)
    {
        let cmd = crate::engine::harnesses::acp_command(harness, record.command.as_deref())
            .map_or_else(|| "claude -p (stream-json)".into(), |command| command.join(" "));

        let state = if crate::engine::harnesses::on_path(harness.bin) {
            "installed".to_string()
        } else {
            format!("missing: {}", harness.install)
        };

        out.push(format!("harness: {} · {state}", harness.label));
        out.push(format!("runs: {cmd}"));
    }

    let (mut threads, mut usd) = (0usize, 0.0f64);

    for thread in store::all()
        .iter()
        .filter(|thread| thread.agents.iter().any(|agent| agent == name))
    {
        threads += 1;
        usd += store::events(&thread.id)
            .iter()
            .filter(|event| event.agent.as_deref() == Some(name))
            .map(turn_cost)
            .sum::<f64>();
    }

    out.push(format!("threads: {threads}"));
    out.push(format!("spend: ${usd:.2}"));

    out
}

fn turn_cost(event: &store::Event) -> f64 {
    if let store::Kind::TurnEnd { cost, .. } = event.kind {
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
fn split_harness(harness_model: &str) -> (String, Option<String>) {
    match harness_model.split_once('/') {
        Some((harness, model)) => (
            harness.to_string(),
            Some(model.to_string()).filter(|model| !model.is_empty() && model != "default"),
        ),
        None if harness_model.is_empty() => ("claude".into(), None),
        None => (harness_model.to_string(), None),
    }
}

/// The profile text of the editor → the fields of R.
fn set_profile(record: &mut roster::Agent, text: &str) {
    let profile = parse_profile(Some(text), None);

    record.style = profile.style;
    record.voice = Some(profile.voice).filter(|voice| !voice.trim().is_empty());
    record.prompt = Some(profile.prompt).filter(|prompt| !prompt.trim().is_empty());
    record.sprite = profile.sprite.map(|sprite| roster::SpriteRec {
        id: sprite.id.clone(),
        rows: sprite.rows.to_vec(),
    });
}

/// Harness H is one codehead drives.
fn known(harness: &str) -> Result<(), String> {
    match crate::engine::harnesses::get(harness) {
        Some(_) => Ok(()),
        None => Err(format!(
            "harness {harness:?}: one of {}",
            crate::engine::harnesses::ALL
                .iter()
                .map(|harness| harness.name)
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

pub(crate) fn create(spec: &Spec) -> Result<(), String> {
    if roster::get(&spec.name).is_some() {
        return Err(format!("agent {} exists", spec.name));
    }

    let (harness, model) = split_harness(&spec.harness);

    known(&harness)?;
    let mut record = roster::Agent {
        name: spec.name.clone(),
        harness,
        model,
        mode: spec.mode.clone(),
        color: spec.color.clone(),
        mcp: spec.mcp.clone(),
        ..Default::default()
    };

    set_profile(&mut record, &spec.prompt);

    roster::save(&record)
}

/// Pass only what changed.
pub(crate) fn edit(old: &Agent, spec: &Spec) -> Result<(), String> {
    if spec.name != old.name {
        roster::rename(&old.name, &spec.name)?;
    }

    let mut record = roster::get(&spec.name).ok_or_else(|| format!("no agent {}", spec.name))?;

    if !spec.harness.is_empty() && spec.harness != old.harness_model() {
        let (harness, model) = split_harness(&spec.harness);

        known(&harness)?;
        (record.harness, record.model) = (harness, model);
    }

    record.mode.clone_from(&spec.mode);
    record.mcp.clone_from(&spec.mcp);

    if spec.color.is_some() {
        record.color.clone_from(&spec.color);
    }

    set_profile(&mut record, &spec.prompt);

    roster::save(&record)
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

    for usage in uses.iter().filter(|usage| !usage.live.is_empty()) {
        let short: String = usage.thread_id.chars().take(12).collect();

        if !usage.busy.is_empty() {
            result.busy.push(short);
            continue;
        }

        match client::send(
            &usage.thread_id,
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
