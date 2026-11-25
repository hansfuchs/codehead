use super::{
    Agent, HANDOFF, MARK, PROMPT_HEAD, Profile, SPRITE_HEAD, SPRITE_INTRO, STYLE_MARK, VOICE_HEAD, VOICE_INTRO, roster,
};
use std::fmt::Write as _;

/// The system prompt part of agent NAME: its output style, handoff rule, sprite, voice and
/// instructions. Claude agents skip a style that is already Claude Code's global output style.
pub(crate) fn session_prompt(result: &roster::Agent) -> String {
    let profile = Profile {
        style: result.style.clone(),
        voice: result.voice.clone().unwrap_or_default(),
        prompt: result.prompt.clone().unwrap_or_default(),
        sprite: result
            .sprite
            .as_ref()
            .and_then(|text| crate::domain::sprites::from_id(&text.id)),
    };
    let styles = crate::infra::styles::load(&crate::infra::styles::dir());
    let global = crate::infra::styles::global_name();

    // the section marks serve the editor only
    compose(&profile, &styles, &result.harness, global.as_deref())
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
        let mut thread = text.trim();

        for height in heads {
            thread = thread.strip_prefix(height).unwrap_or(thread).trim_start();
        }

        thread.trim().to_string()
    };

    Profile {
        style: (id != "none").then(|| id.to_string()),
        voice: section(raw, "<!-- codehead:voice -->")
            .map(|thread_value| strip(thread_value, &[VOICE_HEAD, VOICE_INTRO]))
            .unwrap_or_default(),
        prompt: section(raw, "<!-- codehead:prompt -->")
            .map(|thread_2| strip(thread_2, &[PROMPT_HEAD]))
            .unwrap_or_default(),
        // an old text sprite (`pcb-042`): None, the agent gets its default avatar
        sprite: section(raw, "<!-- codehead:sprite ")
            .and_then(|thread_3| crate::domain::sprites::from_id(thread_3.split_once(" -->")?.0.trim())),
    }
}
/// Profile text for PROFILE. The style body goes in only where the harness lacks it already
/// (Claude agents get the global `outputStyle` from ~/.claude/settings.json).
pub(crate) fn compose(
    path: &Profile,
    styles: &[crate::infra::styles::Style],
    harness: &str,
    global: Option<&str>,
) -> String {
    let style = path
        .style
        .as_deref()
        .and_then(|id| styles.iter().find(|text| text.id == id));

    if style.is_none() && path.sprite.is_none() && path.voice.trim().is_empty() && path.prompt.trim().is_empty() {
        return String::new();
    }

    let mut text_value = format!("<!-- codehead:style {} -->\n", path.style.as_deref().unwrap_or("none"));

    if let Some(st) = style {
        let claude = harness.split('/').next() == Some("claude");

        if !claude || global != Some(st.name.as_str()) {
            let _ = write!(text_value, "# Output style: {}\n\n{}\n\n", st.name, st.body);
        }
    }

    let _ = write!(text_value, "<!-- codehead:handoff -->\n{HANDOFF}\n\n");

    if let Some(sp) = &path.sprite {
        let _ = write!(
            text_value,
            "<!-- codehead:sprite {} -->\n{SPRITE_HEAD}\n\n{SPRITE_INTRO}\n\n```\n{}\n```\n\n",
            sp.id,
            sp.rows.join("\n")
        );
    }

    if !path.voice.trim().is_empty() {
        let _ = write!(
            text_value,
            "<!-- codehead:voice -->\n{VOICE_HEAD}\n\n{VOICE_INTRO}\n\n{}\n\n",
            path.voice.trim()
        );
    }

    if !path.prompt.trim().is_empty() {
        let _ = write!(
            text_value,
            "<!-- codehead:prompt -->\n{PROMPT_HEAD}\n\n{}\n\n",
            path.prompt.trim()
        );
    }

    text_value.push_str("<!-- codehead:end -->");

    text_value
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
