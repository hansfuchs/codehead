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
        .filter(|message| !message.is_empty())
        .map(String::from)
}
/// The effort Claude agents run at: `effortLevel`, else Claude Code's default.
pub(crate) fn effort_name() -> String {
    effort().unwrap_or_else(|| DEFAULT_EFFORT.into())
}
/// Model id or alias → full name: `claude-opus-5-5`, `opus` → `Opus 5.5`; `opus[1m]` → `Opus 5.5 (1M)`.
/// Unknown: as is.
pub(crate) fn model_name(message: &str) -> String {
    let message_value = message.trim().to_lowercase();

    let (message_2, wide) = match message_value.strip_suffix("[1m]") {
        Some(x) => (x.to_string(), " (1M)"),
        None => (message_value, ""),
    };

    if let Some((_, count)) = ALIASES.iter().find(|(agent, _)| *agent == message_2) {
        return format!("{count}{wide}");
    }

    let parts: Vec<&str> = message_2.trim_start_matches("claude-").split('-').collect();
    let family = parts
        .iter()
        .find(|path| path.chars().all(|character| character.is_ascii_alphabetic()));

    // version: the short numbers (a date has 8 digits)
    let version: Vec<&str> = parts
        .iter()
        .copied()
        .filter(|path_value| {
            !path_value.is_empty()
                && path_value.len() < 8
                && path_value
                    .chars()
                    .all(|character_value| character_value.is_ascii_digit())
        })
        .collect();

    match family {
        Some(file) if !version.is_empty() => {
            let mut character_2 = file.chars();
            let file_value = character_2
                .next()
                .map(|x| x.to_uppercase().collect::<String>() + character_2.as_str())
                .unwrap_or_default();

            format!("{file_value} {}{wide}", version.join("."))
        }
        _ => format!("{message_2}{wide}"),
    }
}
/// Full model name of agent A: its own model, else (Claude) settings.json `model`, else the opus alias.
pub(crate) fn model_of(agent: &Agent) -> String {
    match agent
        .model
        .as_deref()
        .filter(|message| !message.is_empty() && *message != "default")
    {
        Some(message_value) => model_name(message_value),
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
        Ok(thread) => thread,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "{}".into(),
        Err(error_value) => return Err(format!("{}: {error_value}", path.display())),
    };

    let mut value: serde_json::Value =
        serde_json::from_str(&text).map_err(|error_2| format!("{}: {error_2} (file left as is)", path.display()))?;

    let Some(obj) = value.as_object_mut() else {
        return Err(format!("{}: not a JSON object (file left as is)", path.display()));
    };

    let old = obj.get("effortLevel").and_then(|x| x.as_str()).map(String::from);

    if old.as_deref() == level {
        return Ok(());
    }

    match level {
        Some(line) => {
            obj.insert("effortLevel".into(), serde_json::Value::String(line.into()));
        }
        None => {
            obj.remove("effortLevel");
        }
    }

    let out = serde_json::to_string_pretty(&value).map_err(|error_3| error_3.to_string())? + "\n";
    let tmp = path.with_extension("json.codehead-tmp");

    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }

    std::fs::write(&tmp, out)
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|error_4| format!("{}: {error_4}", path.display()))
}
