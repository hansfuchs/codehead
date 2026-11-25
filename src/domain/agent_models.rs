use super::{ALIASES, Agent, DEFAULT_EFFORT, Path, PathBuf};

pub(crate) fn claude_settings() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".claude/settings.json")
}
/// `effortLevel` of ~/.claude/settings.json: 1 value for every Claude agent (and Claude Code itself).
pub(crate) fn effort() -> Option<String> {
    let text = std::fs::read_to_string(claude_settings()).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;

    value["effortLevel"].as_str().map(String::from)
}
/// `model` of ~/.claude/settings.json: the model of Claude agents without their own.
pub(crate) fn settings_model() -> Option<String> {
    let text = std::fs::read_to_string(claude_settings()).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;

    value["model"]
        .as_str()
        .filter(|model| !model.is_empty())
        .map(String::from)
}
/// The effort Claude agents run at: `effortLevel`, else Claude Code's default.
pub(crate) fn effort_name() -> String {
    effort().unwrap_or_else(|| DEFAULT_EFFORT.into())
}
/// Model id or alias → full name: `claude-opus-5-5`, `opus` → `Opus 5.5`; `opus[1m]` → `Opus 5.5 (1M)`.
/// Unknown: as is.
pub(crate) fn model_name(model: &str) -> String {
    let lowered = model.trim().to_lowercase();

    let (id, wide) = match lowered.strip_suffix("[1m]") {
        Some(narrow) => (narrow.to_string(), " (1M)"),
        None => (lowered, ""),
    };

    if let Some((_, full)) = ALIASES.iter().find(|(alias, _)| *alias == id) {
        return format!("{full}{wide}");
    }

    let parts: Vec<&str> = id.trim_start_matches("claude-").split('-').collect();
    let family = parts
        .iter()
        .find(|part| part.chars().all(|character| character.is_ascii_alphabetic()));

    // version: the short numbers (a date has 8 digits)
    let version: Vec<&str> = parts
        .iter()
        .copied()
        .filter(|part| !part.is_empty() && part.len() < 8 && part.chars().all(|character| character.is_ascii_digit()))
        .collect();

    match family {
        Some(name) if !version.is_empty() => {
            let mut characters = name.chars();
            let capitalized = characters
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + characters.as_str())
                .unwrap_or_default();

            format!("{capitalized} {}{wide}", version.join("."))
        }
        _ => format!("{id}{wide}"),
    }
}
/// Full model name of agent A: its own model, else (Claude) settings.json `model`, else the opus alias.
pub(crate) fn model_of(agent: &Agent) -> String {
    match agent
        .model
        .as_deref()
        .filter(|model| !model.is_empty() && *model != "default")
    {
        Some(model) => model_name(model),
        None if agent.harness == "claude" => model_name(&settings_model().unwrap_or_else(|| "opus".into())),
        None => String::new(),
    }
}
/// Set (Some) or remove (None) `effortLevel` in ~/.claude/settings.json. Other keys keep their order.
pub(crate) fn set_effort(level: Option<&str>) -> Result<(), String> {
    set_effort_in(&claude_settings(), level)
}
pub(crate) fn set_effort_in(path: &Path, level: Option<&str>) -> Result<(), String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "{}".into(),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };

    let mut value: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| format!("{}: {error} (file left as is)", path.display()))?;

    let Some(obj) = value.as_object_mut() else {
        return Err(format!("{}: not a JSON object (file left as is)", path.display()));
    };

    let old = obj
        .get("effortLevel")
        .and_then(|level| level.as_str())
        .map(String::from);

    if old.as_deref() == level {
        return Ok(());
    }

    match level {
        Some(level) => {
            obj.insert("effortLevel".into(), serde_json::Value::String(level.into()));
        }
        None => {
            obj.remove("effortLevel");
        }
    }

    let out = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())? + "\n";
    let tmp = path.with_extension("json.codehead-tmp");

    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }

    std::fs::write(&tmp, out)
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|error| format!("{}: {error}", path.display()))
}
