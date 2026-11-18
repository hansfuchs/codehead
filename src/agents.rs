//! Agents for the screens: list, create, edit, delete (archive), over the TOML roster
//! (`engine::roster`). Delete hands the agent's threads to the default agent.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::engine::{client, roster, store};
use crate::theme;

pub const COLORS: [&str; 7] = ["red", "orange", "yellow", "green", "blue", "purple", "gray"];
pub const MODES: [&str; 4] = ["autopilot", "edits", "supervised", "plan"];
/// Claude Code effort levels (`claude --effort`). "default": key unset, Claude Code decides.
pub const EFFORTS: [&str; 6] = ["default", "low", "medium", "high", "xhigh", "max"];

fn claude_settings() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".claude/settings.json")
}

/// `effortLevel` of ~/.claude/settings.json: 1 value for every Claude agent (and Claude Code itself).
pub fn effort() -> Option<String> {
    let text = std::fs::read_to_string(claude_settings()).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v["effortLevel"].as_str().map(String::from)
}

/// `model` of ~/.claude/settings.json: the model of Claude agents without their own.
pub fn settings_model() -> Option<String> {
    let text = std::fs::read_to_string(claude_settings()).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v["model"].as_str().filter(|m| !m.is_empty()).map(String::from)
}

/// Claude Code's effort when `effortLevel` is unset (seen in its requests: output_config.effort).
pub const DEFAULT_EFFORT: &str = "high";

/// The effort Claude agents run at: `effortLevel`, else Claude Code's default.
pub fn effort_name() -> String {
    effort().unwrap_or_else(|| DEFAULT_EFFORT.into())
}

/// Latest model per alias (`claude --model opus`).
const ALIASES: [(&str, &str); 4] = [("fable", "Fable 5.2"), ("opus", "Opus 5.5"), ("sonnet", "Sonnet 5.5"), ("haiku", "Haiku 5.5")];

/// Model id or alias → full name: `claude-opus-5-5`, `opus` → `Opus 5.5`; `opus[1m]` → `Opus 5.5 (1M)`.
/// Unknown: as is.
pub fn model_name(m: &str) -> String {
    let m = m.trim().to_lowercase();
    let (m, wide) = match m.strip_suffix("[1m]") {
        Some(x) => (x.to_string(), " (1M)"),
        None => (m, ""),
    };
    if let Some((_, n)) = ALIASES.iter().find(|(a, _)| *a == m) {
        return format!("{n}{wide}");
    }
    let parts: Vec<&str> = m.trim_start_matches("claude-").split('-').collect();
    let family = parts.iter().find(|p| p.chars().all(|c| c.is_ascii_alphabetic()));
    // version: the short numbers (a date has 8 digits)
    let version: Vec<&str> = parts.iter().copied().filter(|p| !p.is_empty() && p.len() < 8 && p.chars().all(|c| c.is_ascii_digit())).collect();
    match family {
        Some(f) if !version.is_empty() => {
            let mut c = f.chars();
            let f = c.next().map(|x| x.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default();
            format!("{f} {}{wide}", version.join("."))
        }
        _ => format!("{m}{wide}"),
    }
}

/// Full model name of agent A: its own model, else (Claude) settings.json `model`, else the opus alias.
pub fn model_of(a: &Agent) -> String {
    match a.model.as_deref().filter(|m| !m.is_empty() && *m != "default") {
        Some(m) => model_name(m),
        None if a.harness == "claude" => model_name(&settings_model().unwrap_or_else(|| "opus".into())),
        None => String::new(),
    }
}

/// Set (Some) or remove (None) `effortLevel` in ~/.claude/settings.json. Other keys keep their order.
pub fn set_effort(level: Option<&str>) -> Result<(), String> {
    set_effort_in(&claude_settings(), level)
}

fn set_effort_in(path: &Path, level: Option<&str>) -> Result<(), String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "{}".into(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let mut v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e} (file left as is)", path.display()))?;
    let Some(obj) = v.as_object_mut() else { return Err(format!("{}: not a JSON object (file left as is)", path.display())) };
    let old = obj.get("effortLevel").and_then(|x| x.as_str()).map(String::from);
    if old.as_deref() == level {
        return Ok(());
    }
    match level {
        Some(l) => {
            obj.insert("effortLevel".into(), serde_json::Value::String(l.into()));
        }
        None => {
            obj.remove("effortLevel");
        }
    }
    let out = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())? + "\n";
    let tmp = path.with_extension("json.codehead-tmp");
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&tmp, out).and_then(|_| std::fs::rename(&tmp, path)).map_err(|e| format!("{}: {e}", path.display()))
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Agent {
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
    pub fn harness_model(&self) -> String {
        match &self.model {
            Some(m) if !m.is_empty() => format!("{}/{m}", self.harness),
            _ => self.harness.clone(),
        }
    }
}

/// A thread that 1 agent worked on.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Use {
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
pub fn color(name: Option<&str>) -> Color {
    match name {
        Some("purple") => Color::Magenta,
        Some("yellow") => Color::Yellow,
        Some("red") => Color::Red,
        Some("green") => Color::Green,
        Some("blue") => Color::Blue,
        Some("gray") => Color::Gray,
        Some("orange") => Color::Indexed(208),
        _ => theme::ACCENT,
    }
}

/// Active (not archived) agents, by name.
pub fn cached() -> Vec<Agent> {
    load()
}

fn from_roster(r: &roster::Agent, seen: &HashMap<String, i64>) -> Agent {
    let profile = Profile {
        style: r.style.clone(),
        voice: r.voice.clone().unwrap_or_default(),
        prompt: r.prompt.clone().unwrap_or_default(),
        sprite: r.sprite.as_ref().map(|s| crate::sprites::from_text(&s.id, &s.rows.join("\n"))),
    };
    let text = compose(&profile, &[], &r.harness, None);
    Agent {
        name: r.name.clone(),
        harness: r.harness.clone(),
        model: r.model.clone(),
        color: r.color.clone(),
        mode: r.mode.clone(),
        prompt: (!text.is_empty()).then_some(text),
        last_seen: seen.get(&r.name).copied(),
    }
}

/// Last activity per agent: the newest thread it is on.
fn last_seen() -> HashMap<String, i64> {
    let mut m: HashMap<String, i64> = HashMap::new();
    for t in store::all() {
        for a in &t.agents {
            let at = (t.updated / 1000) as i64;
            let e = m.entry(a.clone()).or_insert(at);
            *e = (*e).max(at);
        }
    }
    m
}

pub fn load() -> Vec<Agent> {
    let seen = last_seen();
    roster::active().iter().map(|r| from_roster(r, &seen)).collect()
}

/// Threads per agent name (open + done).
pub fn usage() -> HashMap<String, Vec<Use>> {
    let mut map: HashMap<String, Vec<Use>> = HashMap::new();
    for t in store::all() {
        let (live, busy) = if client::alive(&t.id) { client::state(&t.id) } else { Default::default() };
        let dir = t.work_dir().display().to_string();
        for a in &t.agents {
            let mine = |v: &Vec<String>| v.iter().filter(|x| *x == a).cloned().collect::<Vec<_>>();
            map.entry(a.clone()).or_default().push(Use {
                thread_id: t.id.clone(),
                title: t.title.lines().next().unwrap_or_default().to_string(),
                state: t.state.clone(),
                live: mine(&live),
                busy: mine(&busy),
                others_live: live.iter().filter(|x| *x != a).count(),
                dir: dir.clone(),
            });
        }
    }
    map
}


/// Facts about agent NAME beyond its form: its file, threads, spend.
pub fn show_extra(name: &str) -> Vec<String> {
    let mut out = vec![format!("file: {}", roster::agents_dir().join(format!("{name}.toml")).display())];
    if let Some(r) = roster::get(name)
        && let Some(h) = crate::engine::harnesses::get(&r.harness)
    {
        let cmd = crate::engine::harnesses::acp_command(h, r.command.as_deref()).map(|c| c.join(" ")).unwrap_or_else(|| "claude -p (stream-json)".into());
        let state = if crate::engine::harnesses::on_path(h.bin) { "installed".to_string() } else { format!("missing: {}", h.install) };
        out.push(format!("harness: {} · {state}", h.label));
        out.push(format!("runs: {cmd}"));
    }
    let (mut threads, mut usd) = (0usize, 0.0f64);
    for t in store::all().iter().filter(|t| t.agents.iter().any(|a| a == name)) {
        threads += 1;
        usd += store::events(&t.id)
            .iter()
            .filter(|e| e.agent.as_deref() == Some(name))
            .map(|e| if let store::Kind::TurnEnd { cost, .. } = e.kind { cost } else { 0.0 })
            .sum::<f64>();
    }
    out.push(format!("threads: {threads}"));
    out.push(format!("spend: ${usd:.2}"));
    out
}

pub struct Spec {
    pub name: String,
    pub harness: String,
    pub color: Option<String>,
    pub mode: String,
    pub prompt: String,
}

/// `claude/opus` → (claude, Some(opus)).
fn split_harness(h: &str) -> (String, Option<String>) {
    match h.split_once('/') {
        Some((h, m)) => (h.to_string(), Some(m.to_string()).filter(|m| !m.is_empty() && m != "default")),
        None if h.is_empty() => ("claude".into(), None),
        None => (h.to_string(), None),
    }
}

/// The profile text of the editor → the fields of R.
fn set_profile(r: &mut roster::Agent, text: &str) {
    let p = parse_profile(Some(text), None);
    r.style = p.style;
    r.voice = Some(p.voice).filter(|v| !v.trim().is_empty());
    r.prompt = Some(p.prompt).filter(|v| !v.trim().is_empty());
    r.sprite = p.sprite.map(|s| roster::SpriteRec { id: s.id.clone(), rows: s.rows.to_vec() });
}

/// Harness H is one codehead drives.
fn known(h: &str) -> Result<(), String> {
    match crate::engine::harnesses::get(h) {
        Some(_) => Ok(()),
        None => Err(format!("harness {h:?}: one of {}", crate::engine::harnesses::ALL.iter().map(|h| h.name).collect::<Vec<_>>().join(", "))),
    }
}

pub fn create(s: &Spec) -> Result<(), String> {
    if roster::get(&s.name).is_some() {
        return Err(format!("agent {} exists", s.name));
    }
    let (harness, model) = split_harness(&s.harness);
    known(&harness)?;
    let mut r = roster::Agent { name: s.name.clone(), harness, model, mode: s.mode.clone(), color: s.color.clone(), ..Default::default() };
    set_profile(&mut r, &s.prompt);
    roster::save(&r)
}

/// Pass only what changed.
pub fn edit(old: &Agent, s: &Spec) -> Result<(), String> {
    if s.name != old.name {
        roster::rename(&old.name, &s.name)?;
    }
    let mut r = roster::get(&s.name).ok_or_else(|| format!("no agent {}", s.name))?;
    if !s.harness.is_empty() && s.harness != old.harness_model() {
        let (h, m) = split_harness(&s.harness);
        known(&h)?;
        (r.harness, r.model) = (h, m);
    }
    r.mode = s.mode.clone();
    if s.color.is_some() {
        r.color = s.color.clone();
    }
    set_profile(&mut r, &s.prompt);
    roster::save(&r)
}

/// Changes a live session keeps from its start: harness, model, mode, prompt.
pub fn profile_changed(old: &Agent, new: &Agent) -> bool {
    (&old.harness, &old.model, &old.mode, &old.prompt) != (&new.harness, &new.model, &new.mode, &new.prompt)
}

/// What `restart` did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Restart {
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
pub fn restart(agent: &Agent, same_harness: bool, uses: &[Use], _logs: &Path) -> Restart {
    let mut r = Restart::default();
    for u in uses.iter().filter(|u| !u.live.is_empty()) {
        let short: String = u.thread_id.chars().take(12).collect();
        if !u.busy.is_empty() {
            r.busy.push(short);
            continue;
        }
        match client::send(&u.thread_id, &serde_json::json!({"op": "reload", "agent": agent.name, "fresh": !same_harness})) {
            Ok(_) if same_harness => r.resumed += 1,
            Ok(_) => r.stopped += 1,
            Err(e) => r.failed.push(format!("{short}: {e}")),
        }
    }
    r
}

/// Archive AGENT. It leaves its threads; a thread left with 0 agents gets FALLBACK as lead.
pub fn delete(agent: &Agent, fallback: &Agent, uses: &[Use], _logs: &Path) -> Vec<Line<'static>> {
    let mut out = vec![Line::from(vec![Span::styled(agent.name.clone(), Style::default().fg(color(agent.color.as_deref()))), Span::styled(format!("  {} threads", uses.len()), theme::muted())])];
    let danger = Style::default().fg(theme::DANGER);
    for u in uses {
        let short: String = u.thread_id.chars().take(12).collect();
        let mut line = vec![Span::styled(format!("  {short} "), theme::muted()), Span::raw(u.title.clone())];
        let left = if client::alive(&u.thread_id) {
            client::send(&u.thread_id, &serde_json::json!({"op": "leave", "agent": agent.name})).map(|_| ())
        } else {
            store::update(&u.thread_id, |t| t.agents.retain(|a| *a != agent.name)).map(|_| ())
        };
        if let Err(e) = left {
            line.push(Span::styled(format!(" · leave failed: {e}"), danger));
        }
        if store::load(&u.thread_id).is_some_and(|t| t.agents.is_empty()) {
            match store::update(&u.thread_id, |t| t.agents.push(fallback.name.clone())) {
                Ok(_) => line.push(Span::styled(format!(" · {} leads now", fallback.name), Style::default().fg(theme::OK))),
                Err(e) => line.push(Span::styled(format!(" · {e}"), danger)),
            }
        }
        out.push(Line::from(line));
    }
    let archived = roster::get(&agent.name).ok_or_else(|| format!("no agent {}", agent.name)).and_then(|mut r| {
        r.archived = true;
        roster::save(&r)
    });
    match archived {
        Ok(_) => out.push(Line::styled(format!("{} deleted (archived: file + history kept)", agent.name), Style::default().fg(theme::OK))),
        Err(e) => out.push(Line::styled(format!("archive failed: {e}"), danger)),
    }
    out
}

/// The system prompt part of agent NAME: its output style, handoff rule, sprite, voice and
/// instructions. Claude agents skip a style that is already Claude Code's global output style.
pub fn session_prompt(r: &roster::Agent) -> String {
    let profile = Profile {
        style: r.style.clone(),
        voice: r.voice.clone().unwrap_or_default(),
        prompt: r.prompt.clone().unwrap_or_default(),
        sprite: r.sprite.as_ref().map(|s| crate::sprites::from_text(&s.id, &s.rows.join("\n"))),
    };
    let styles = crate::styles::load(&crate::styles::dir());
    let global = crate::styles::global_name();
    // the section marks serve the editor only
    compose(&profile, &styles, &r.harness, global.as_deref()).lines().filter(|l| !l.starts_with(MARK)).collect::<Vec<_>>().join("\n").trim().to_string()
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
pub struct Profile {
    /// Style id (file stem in codehead/output-styles), None = no base style.
    pub style: Option<String>,
    pub voice: String,
    pub prompt: String,
    /// None = 0 saved: codehead shows a stable pool pick by name.
    pub sprite: Option<crate::sprites::Sprite>,
}

/// Marker prefix of the prompt sections codehead writes.
const MARK: &str = "<!-- codehead:";
const STYLE_MARK: &str = "<!-- codehead:style ";

/// Prompt RAW was written by codehead.
pub fn written_by_us(raw: &str) -> bool {
    raw.contains(STYLE_MARK)
}

fn section<'a>(raw: &'a str, mark: &str) -> Option<&'a str> {
    let start = raw.find(mark)? + mark.len();
    let rest = &raw[start..];
    let end = rest.find(MARK).unwrap_or(rest.len());
    Some(&rest[..end])
}

/// Split a profile text back into its parts. A prompt codehead did not write = system prompt only.
pub fn parse_profile(raw: Option<&str>, default_style: Option<&str>) -> Profile {
    let raw = raw.unwrap_or_default();
    let Some(after) = raw.find(STYLE_MARK).map(|i| &raw[i + STYLE_MARK.len()..]) else {
        return Profile { style: default_style.map(String::from), prompt: raw.trim().to_string(), ..Profile::default() };
    };
    let id = after.split(" -->").next().unwrap_or("none").trim();
    let strip = |text: &str, heads: &[&str]| {
        let mut t = text.trim();
        for h in heads {
            t = t.strip_prefix(h).unwrap_or(t).trim_start();
        }
        t.trim().to_string()
    };
    Profile {
        style: (id != "none").then(|| id.to_string()),
        voice: section(raw, "<!-- codehead:voice -->").map(|t| strip(t, &[VOICE_HEAD, VOICE_INTRO])).unwrap_or_default(),
        prompt: section(raw, "<!-- codehead:prompt -->").map(|t| strip(t, &[PROMPT_HEAD])).unwrap_or_default(),
        sprite: section(raw, "<!-- codehead:sprite ").and_then(|t| {
            let (id, body) = t.split_once(" -->")?;
            let fenced = body.split("```").nth(1)?;
            Some(crate::sprites::from_text(id.trim(), fenced.strip_prefix('\n').unwrap_or(fenced)))
        }),
    }
}

/// Profile text for PROFILE. The style body goes in only where the harness lacks it already
/// (Claude agents get the global `outputStyle` from ~/.claude/settings.json).
pub fn compose(p: &Profile, styles: &[crate::styles::Style], harness: &str, global: Option<&str>) -> String {
    let style = p.style.as_deref().and_then(|id| styles.iter().find(|s| s.id == id));
    if style.is_none() && p.sprite.is_none() && p.voice.trim().is_empty() && p.prompt.trim().is_empty() {
        return String::new();
    }
    let mut s = format!("<!-- codehead:style {} -->\n", p.style.as_deref().unwrap_or("none"));
    if let Some(st) = style {
        let claude = harness.split('/').next() == Some("claude");
        if !claude || global != Some(st.name.as_str()) {
            s.push_str(&format!("# Output style: {}\n\n{}\n\n", st.name, st.body));
        }
    }
    s.push_str(&format!("<!-- codehead:handoff -->\n{HANDOFF}\n\n"));
    if let Some(sp) = &p.sprite {
        s.push_str(&format!("<!-- codehead:sprite {} -->\n{SPRITE_HEAD}\n\n{SPRITE_INTRO}\n\n```\n{}\n```\n\n", sp.id, sp.rows.join("\n")));
    }
    if !p.voice.trim().is_empty() {
        s.push_str(&format!("<!-- codehead:voice -->\n{VOICE_HEAD}\n\n{VOICE_INTRO}\n\n{}\n\n", p.voice.trim()));
    }
    if !p.prompt.trim().is_empty() {
        s.push_str(&format!("<!-- codehead:prompt -->\n{PROMPT_HEAD}\n\n{}\n\n", p.prompt.trim()));
    }
    s.push_str("<!-- codehead:end -->");
    s
}

/// Sprite of A: saved in its profile, else the stable pool pick by name.
pub fn sprite(a: &Agent) -> crate::sprites::Sprite {
    parse_profile(a.prompt.as_deref(), None).sprite.unwrap_or_else(|| crate::sprites::default_for(&a.name))
}

/// Style files changed: 0 work. Every session composes its prompt from the style files at its
/// start, so nothing stored needs a rewrite.
pub fn sync(_all: &[Agent], _styles: &[crate::styles::Style], _global: Option<&str>) -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_by_us_marks() {
        let raw = "<!-- codehead:style mecha -->\n<!-- codehead:end -->";
        assert!(written_by_us(raw));
        assert_eq!(parse_profile(Some(raw), None).style.as_deref(), Some("mecha"));
        assert!(!written_by_us("plain prompt"));
    }

    #[test]
    fn full_model_names() {
        assert_eq!(model_name("claude-opus-5-5"), "Opus 5.5");
        assert_eq!(model_name("opus"), "Opus 5.5");
        assert_eq!(model_name("opus[1m]"), "Opus 5.5 (1M)");
        assert_eq!(model_name("claude-opus-4-1-20250805"), "Opus 4.1");
        assert_eq!(model_name("claude-3-5-sonnet-20241022"), "Sonnet 3.5");
        assert_eq!(model_name("gpt-5.5"), "gpt-5.5");
    }

    use crate::styles::Style;

    fn styles() -> Vec<Style> {
        vec![Style { id: "mecha".into(), name: "MECHA".into(), body: "BODY".into() }]
    }

    #[test]
    fn compose_parse_round_trip() {
        let sprite = Some(crate::sprites::from_text("pcb-007", "╮     ╭\n╰┬───┬╯\n ◆─┼─◆\n ╰┴┴┴╯"));
        let p = Profile { style: Some("mecha".into()), voice: "dry.\ncalls the owner boss.".into(), prompt: "you review rust.".into(), sprite };
        for (harness, global, embedded) in [("claude", Some("MECHA"), false), ("claude", None, true), ("codex", Some("MECHA"), true)] {
            let raw = compose(&p, &styles(), harness, global);
            assert_eq!(raw.contains("BODY"), embedded, "{harness} {global:?}");
            assert!(raw.contains("<!-- codehead:handoff -->") && raw.contains("needs harness: <harness> (<tool>)"));
            assert_eq!(parse_profile(Some(&raw), None), p);
        }
    }

    #[test]
    fn foreign_prompt_is_system_prompt() {
        let p = parse_profile(Some("be brief"), Some("mecha"));
        assert_eq!(p, Profile { style: Some("mecha".into()), prompt: "be brief".into(), ..Profile::default() });
        assert_eq!(compose(&Profile::default(), &styles(), "claude", None), "");
    }

    #[test]
    fn effort_keeps_other_keys() {
        let p = std::env::temp_dir().join(format!("codehead-settings-{}.json", std::process::id()));
        std::fs::write(&p, "{\n  \"zeta\": 1,\n  \"alpha\": \"x\"\n}\n").unwrap();
        set_effort_in(&p, Some("high")).unwrap();
        let t = std::fs::read_to_string(&p).unwrap();
        assert!(t.find("zeta").unwrap() < t.find("alpha").unwrap() && t.find("alpha").unwrap() < t.find("effortLevel").unwrap(), "{t}");
        set_effort_in(&p, None).unwrap();
        assert!(!std::fs::read_to_string(&p).unwrap().contains("effortLevel"));
        std::fs::write(&p, "not json").unwrap();
        assert!(set_effort_in(&p, Some("low")).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "not json");
        let _ = std::fs::remove_file(&p);
    }
}

#[cfg(test)]
mod restart_tests {
    use super::*;

    #[test]
    fn profile_change_needs_a_restart() {
        let a = Agent { name: "lilith".into(), harness: "claude".into(), mode: "plan".into(), ..Default::default() };
        assert!(!profile_changed(&a, &Agent { color: Some("red".into()), ..a.clone() }), "color: 0 restart");
        assert!(profile_changed(&a, &Agent { mode: "autopilot".into(), ..a.clone() }));
        assert!(profile_changed(&a, &Agent { model: Some("fable".into()), ..a.clone() }));
        assert!(profile_changed(&a, &Agent { prompt: Some("new".into()), ..a.clone() }));
        // busy session: left alone (0 host calls)
        let u = Use { thread_id: "t".into(), live: vec!["lilith".into()], busy: vec!["lilith".into()], ..Default::default() };
        let r = restart(&a, true, &[u], Path::new("/tmp"));
        assert_eq!(r, Restart { busy: vec!["t".into()], ..Default::default() });
    }
}
