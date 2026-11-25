use super::{
    Agent, HANDOFF, MARK, PROMPT_HEAD, Profile, SPRITE_HEAD, SPRITE_INTRO, STYLE_MARK, VOICE_HEAD, VOICE_INTRO, roster,
};
use std::fmt::Write as _;

/// The system prompt part of agent NAME: its output style, handoff rule, sprite, voice and
/// instructions. Claude agents skip a style that is already Claude Code's global output style.
pub(crate) fn session_prompt(agent: &roster::Agent) -> String {
    let profile = Profile {
        style: agent.style.clone(),
        voice: agent.voice.clone().unwrap_or_default(),
        prompt: agent.prompt.clone().unwrap_or_default(),
        sprite: agent
            .sprite
            .as_ref()
            .and_then(|sprite| crate::domain::sprites::from_id(&sprite.id)),
    };
    let styles = crate::infra::styles::load(&crate::infra::styles::dir());
    let global = crate::infra::styles::global_name();

    // the section marks serve the editor only
    compose(&profile, &styles, &agent.harness, global.as_deref())
        .lines()
        .filter(|line| !line.starts_with(MARK))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}
/// Prompt RAW was written by codehead.
pub(crate) fn written_by_us(raw: &str) -> bool {
    raw.contains(STYLE_MARK)
}
pub(crate) fn section<'a>(raw: &'a str, mark: &str) -> Option<&'a str> {
    let start = raw.find(mark)? + mark.len();
    let rest = &raw[start..];
    let end = rest.find(MARK).unwrap_or(rest.len());

    Some(&rest[..end])
}
/// Split a profile text back into its parts. A prompt codehead did not write = system prompt only.
pub(crate) fn parse_profile(raw: Option<&str>, default_style: Option<&str>) -> Profile {
    let raw = raw.unwrap_or_default();

    let Some(after) = raw.find(STYLE_MARK).map(|i| &raw[i + STYLE_MARK.len()..]) else {
        return Profile {
            style: default_style.map(String::from),
            prompt: raw.trim().to_string(),
            ..Profile::default()
        };
    };

    let id = after.split(" -->").next().unwrap_or("none").trim();

    let strip = |text: &str, heads: &[&str]| {
        let mut rest = text.trim();

        for head in heads {
            rest = rest.strip_prefix(head).unwrap_or(rest).trim_start();
        }

        rest.trim().to_string()
    };

    Profile {
        style: (id != "none").then(|| id.to_string()),
        voice: section(raw, "<!-- codehead:voice -->")
            .map(|body| strip(body, &[VOICE_HEAD, VOICE_INTRO]))
            .unwrap_or_default(),
        prompt: section(raw, "<!-- codehead:prompt -->")
            .map(|body| strip(body, &[PROMPT_HEAD]))
            .unwrap_or_default(),
        // an old text sprite (`pcb-042`): None, the agent gets its default avatar
        sprite: section(raw, "<!-- codehead:sprite ")
            .and_then(|body| crate::domain::sprites::from_id(body.split_once(" -->")?.0.trim())),
    }
}
/// Profile text for PROFILE. The style body goes in only where the harness lacks it already
/// (Claude agents get the global `outputStyle` from ~/.claude/settings.json).
pub(crate) fn compose(
    profile: &Profile,
    styles: &[crate::infra::styles::Style],
    harness: &str,
    global: Option<&str>,
) -> String {
    let style = profile
        .style
        .as_deref()
        .and_then(|id| styles.iter().find(|candidate| candidate.id == id));

    if style.is_none()
        && profile.sprite.is_none()
        && profile.voice.trim().is_empty()
        && profile.prompt.trim().is_empty()
    {
        return String::new();
    }

    let mut text = format!(
        "<!-- codehead:style {} -->\n",
        profile.style.as_deref().unwrap_or("none")
    );

    if let Some(style) = style {
        let claude = harness.split('/').next() == Some("claude");

        if !claude || global != Some(style.name.as_str()) {
            let _ = write!(text, "# Output style: {}\n\n{}\n\n", style.name, style.body);
        }
    }

    let _ = write!(text, "<!-- codehead:handoff -->\n{HANDOFF}\n\n");

    if let Some(sprite) = &profile.sprite {
        let _ = write!(
            text,
            "<!-- codehead:sprite {} -->\n{SPRITE_HEAD}\n\n{SPRITE_INTRO}\n\n```\n{}\n```\n\n",
            sprite.id,
            sprite.rows.join("\n")
        );
    }

    if !profile.voice.trim().is_empty() {
        let _ = write!(
            text,
            "<!-- codehead:voice -->\n{VOICE_HEAD}\n\n{VOICE_INTRO}\n\n{}\n\n",
            profile.voice.trim()
        );
    }

    if !profile.prompt.trim().is_empty() {
        let _ = write!(
            text,
            "<!-- codehead:prompt -->\n{PROMPT_HEAD}\n\n{}\n\n",
            profile.prompt.trim()
        );
    }

    text.push_str("<!-- codehead:end -->");

    text
}
/// Sprite of A: saved in its profile, else the stable pool pick by name.
pub(crate) fn sprite(agent: &Agent) -> crate::domain::sprites::Sprite {
    parse_profile(agent.prompt.as_deref(), None)
        .sprite
        .unwrap_or_else(|| crate::domain::sprites::default_for(&agent.name))
}
/// Style files changed: 0 work. Every session composes its prompt from the style files at its
/// start, so nothing stored needs a rewrite.
pub(crate) fn sync(_all: &[Agent], _styles: &[crate::infra::styles::Style], _global: Option<&str>) -> Vec<String> {
    Vec::new()
}
