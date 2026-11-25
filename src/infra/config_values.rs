use super::{BTreeMap, CFG, File, PathBuf, RwLock, expand, get, layout};
use std::ops::ControlFlow;

/// Setting KEY of F (a settings field), None: unset (the default applies).
impl File {
    pub(crate) fn get(&self, key: &str) -> Option<String> {
        match key {
            "notes" => self.notes.clone(),
            "home" => self.home.clone(),
            "device" => self.device.clone(),
            "repo" => self.repo.clone(),
            "styles" => self.styles.clone(),
            "agent" => self.agent.clone(),
            "prefix" => self.prefix.clone(),
            "models" => self.models.clone(),
            "messages" => self.messages.clone(),
            "thoughts" => self.thoughts.clone(),
            "edits" => self.edits.clone(),
            "rank" => self.rank.clone(),
            "classify" => self.classify.clone(),
            "review" => self.review.clone(),
            "editor" => self.editor.clone(),
            "ports" => self.ports.clone(),
            "previews" => self.previews.clone(),
            "keys.menu" => self.menu_key.clone(),
            "keys.command" => self.command_key.clone(),
            "colors.scheme" => self.scheme.clone(),
            key_value if key_value.starts_with("harness.") => {
                let (name, field) = key_value["harness.".len()..].split_once('.')?;

                self.harness.get(name)?.get(field).cloned()
            }
            key_2 => key_2
                .strip_prefix("colors.")
                .and_then(|count| self.colors.get(count).cloned()),
        }
    }

    pub(crate) fn set(&mut self, key: &str, value: Option<String>) {
        apply_value(self, key, value);
    }
}

/// The palette of F: its scheme ($`CODEHEAD_SCHEME` first) + its color overrides.
pub(crate) fn palette_of(file: &File) -> crate::domain::palette::Palette {
    let scheme = env("CODEHEAD_SCHEME").or_else(|| file.scheme.clone());

    crate::domain::palette::Palette::build(
        scheme.as_deref(),
        file.colors.iter().map(|(key, value)| (key.as_str(), value.as_str())),
    )
}

pub(crate) fn apply(file: File) {
    crate::domain::palette::set_palette(palette_of(&file));
    *CFG.get_or_init(|| RwLock::new(File::default()))
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = file;
}

pub(crate) fn resolve_alias(path: &str) -> Option<PathBuf> {
    alias_path(&get().aliases, path)
}

pub(crate) fn alias_path(aliases: &BTreeMap<String, String>, path: &str) -> Option<PathBuf> {
    let (name, rest) = path.split_once('/')?;
    let target = aliases.get(name)?;

    Some(expand(target).join(rest))
}

/// Expand alias path tokens at send time, retaining punctuation and ordinary text.
pub(crate) fn expand_aliases(text: &str) -> String {
    expand_with(&get().aliases, text)
}

pub(super) fn expand_with(aliases: &BTreeMap<String, String>, text: &str) -> String {
    let boundary = |character: char| character.is_whitespace() || "()[]\"'`<>,".contains(character);
    let mut out = String::new();
    let mut start = 0;

    for (i, ch) in text.char_indices() {
        if boundary(ch) {
            let word = &text[start..i];

            out.push_str(
                &alias_path(aliases, word).map_or_else(|| word.to_string(), |path| path.display().to_string()),
            );
            out.push(ch);
            start = i + ch.len_utf8();
        }
    }

    let word = &text[start..];

    out.push_str(
        &alias_path(aliases, word).map_or_else(|| word.to_string(), |path_value| path_value.display().to_string()),
    );

    out
}

pub(super) fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|text| !text.is_empty())
}

pub(super) fn app_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub(crate) fn defaults() -> File {
    File {
        notes: Some(app_dir().join("notes").display().to_string()),
        styles: Some(app_dir().join("output-styles").display().to_string()),
        prefix: Some("task".into()),
        models: Some("fable,opus,sonnet,haiku".into()),
        messages: Some("bubbles".into()),
        thoughts: Some("hidden".into()),
        edits: Some("diff".into()),
        rank: Some("claude:haiku".into()),
        review: Some("claude:sonnet".into()),
        editor: Some("auto".into()),
        ports: Some("auto".into()),
        previews: Some("auto".into()),
        menu_key: Some("ctrl+p".into()),
        command_key: Some(">".into()),
        scheme: Some(crate::domain::palette::PRESETS[0].0.into()),
        ..File::default()
    }
}

/// $`CODEHEAD_NOTES`, else `notes`, else notes/ of the home repo (with its codehead dir), else
/// notes/ of the app dir.
pub(crate) fn notes() -> PathBuf {
    env("CODEHEAD_NOTES")
        .map(PathBuf::from)
        .or_else(|| get().notes.as_deref().map(expand))
        .or_else(|| layout().and_then(|directory| Some(directory.parent()?.join("notes"))))
        .unwrap_or_else(|| app_dir().join("notes"))
}

/// The home repo: $`CODEHEAD_HOME_REPO`, else `home`. None: unset or 0 dir.
pub(crate) fn home_repo() -> Option<PathBuf> {
    env("CODEHEAD_HOME_REPO")
        .map(PathBuf::from)
        .or_else(|| get().home.as_deref().filter(|text| !text.is_empty()).map(expand))
        .filter(|path| path.is_dir())
}

/// The new tab's default repo: $`CODEHEAD_REPO`, else `repo`. None: unset or 0 dir.
pub(crate) fn default_repo() -> Option<PathBuf> {
    env("CODEHEAD_REPO")
        .map(PathBuf::from)
        .or_else(|| get().repo.as_deref().filter(|text| !text.is_empty()).map(expand))
        .filter(|path| path.is_dir())
}

/// $`CODEHEAD_STYLES`, else `styles`, else styles/ of home's codehead dir (when there), else
/// output-styles/ of the app dir.
pub(crate) fn styles() -> PathBuf {
    env("CODEHEAD_STYLES")
        .map(PathBuf::from)
        .or_else(|| get().styles.as_deref().map(expand))
        .or_else(|| {
            layout()
                .map(|directory| directory.join("styles"))
                .filter(|directory_value| directory_value.is_dir())
        })
        .unwrap_or_else(|| app_dir().join("output-styles"))
}

/// $`CODEHEAD_AGENT`, else `agent`.
pub(crate) fn agent() -> Option<String> {
    env("CODEHEAD_AGENT").or_else(|| get().agent.clone().filter(|text| !text.is_empty()))
}

/// $`CODEHEAD_PREFIX`, else `prefix`, else "task".
pub(crate) fn prefix() -> String {
    env("CODEHEAD_PREFIX")
        .or_else(|| get().prefix.clone().filter(|text| !text.is_empty()))
        .unwrap_or_else(|| "task".into())
}

/// Claude Code models for the agent form: $`CODEHEAD_MODELS`, else `models` (comma-separated), else the aliases.
pub(crate) fn models() -> Vec<String> {
    let raw = env("CODEHEAD_MODELS")
        .or_else(|| get().models.clone())
        .unwrap_or_else(|| "fable,opus,sonnet,haiku".into());

    raw.split(',')
        .map(|message| message.trim().to_string())
        .filter(|message_value| !message_value.is_empty())
        .collect()
}

/// How the thread view shows thoughts: $`CODEHEAD_THOUGHTS`, else `thoughts`, else hidden.
pub(crate) fn thoughts() -> crate::domain::chat::Thoughts {
    env("CODEHEAD_THOUGHTS")
        .or_else(|| get().thoughts.clone())
        .and_then(|message| crate::domain::chat::Thoughts::parse(&message))
        .unwrap_or_default()
}

/// What an agent learns of others' edits: $`CODEHEAD_EDITS`, else `edits`, else diff.
pub(crate) fn edits() -> crate::engine::edits::Mode {
    env("CODEHEAD_EDITS")
        .or_else(|| get().edits.clone())
        .and_then(|message| crate::engine::edits::Mode::parse(&message))
        .unwrap_or_default()
}

/// The changes view's reviewer model: $`CODEHEAD_REVIEW`, else `review`, else claude:sonnet.
/// None: `off`.
pub(crate) fn review_model() -> Option<String> {
    let value = env("CODEHEAD_REVIEW")
        .or_else(|| get().review.clone())
        .unwrap_or_else(|| "claude:sonnet".into());

    value
        .strip_prefix("claude:")
        .filter(|message| !message.is_empty())
        .map(str::to_string)
}

/// Image + pdf previews in the files view: $`CODEHEAD_PREVIEWS`, else `previews`, else `auto`:
/// `auto` (the protocol from the terminal's env), `kitty`, `iterm2`, `sixel`, or `off` (text
/// previews only).
pub(crate) fn previews() -> String {
    env("CODEHEAD_PREVIEWS")
        .or_else(|| get().previews.clone())
        .map(|value| value.trim().to_lowercase())
        .filter(|value_value| !value_value.is_empty())
        .unwrap_or_else(|| "auto".into())
}

/// Dev server ports: $`CODEHEAD_PORTS`, else `ports`, else auto.
pub(crate) fn ports() -> crate::infra::portless::Mode {
    env("CODEHEAD_PORTS")
        .or_else(|| get().ports.clone())
        .and_then(|message| crate::infra::portless::Mode::parse(&message))
        .unwrap_or_default()
}

/// The `editor` setting (None: unset; `auto` applies).
pub(crate) fn editor() -> Option<String> {
    get().editor.clone().filter(|text| !text.is_empty())
}

/// The model that ranks the changes view: $`CODEHEAD_RANK`, else `rank`, else claude:haiku.
/// None: `off` (path rules only).
pub(crate) fn rank_model() -> Option<String> {
    let value = env("CODEHEAD_RANK")
        .or_else(|| get().rank.clone())
        .unwrap_or_else(|| "claude:haiku".into());

    value
        .strip_prefix("claude:")
        .filter(|message| !message.is_empty())
        .map(str::to_string)
}

/// The changes view's classifier agent: $`CODEHEAD_CLASSIFY`, else `classify`. None: unset (the
/// default agent classifies), `off`: path rules.
pub(crate) fn classify() -> Option<String> {
    env("CODEHEAD_CLASSIFY")
        .or_else(|| get().classify.clone())
        .map(|text| text.trim().to_string())
        .filter(|text_value| !text_value.is_empty())
}

/// How the thread view draws messages: $`CODEHEAD_MESSAGES`, else `messages`, else bubbles.
pub(crate) fn messages() -> crate::domain::chat::Look {
    env("CODEHEAD_MESSAGES")
        .or_else(|| get().messages.clone())
        .and_then(|message| crate::domain::chat::Look::parse(&message))
        .unwrap_or_default()
}

fn apply_value(widget_state: &mut File, key: &str, value: Option<String>) {
    match key {
        "notes" => widget_state.notes = value,
        "home" => widget_state.home = value,
        "device" => widget_state.device = value,
        "repo" => widget_state.repo = value,
        "styles" => widget_state.styles = value,
        "agent" => widget_state.agent = value,
        "prefix" => widget_state.prefix = value,
        "models" => widget_state.models = value,
        "messages" => widget_state.messages = value,
        "thoughts" => widget_state.thoughts = value,
        "edits" => widget_state.edits = value,
        "rank" => widget_state.rank = value,
        "classify" => widget_state.classify = value,
        "review" => widget_state.review = value,
        "editor" => widget_state.editor = value,
        "ports" => widget_state.ports = value,
        "previews" => widget_state.previews = value,
        "keys.menu" => widget_state.menu_key = value,
        "keys.command" => widget_state.command_key = value,
        "colors.scheme" => widget_state.scheme = value,
        key_value if key_value.starts_with("harness.") => {
            if let ControlFlow::Break(()) = set_harness_field(widget_state, value, key_value) {}
        }
        key_2 => {
            if let Some(count) = key_2.strip_prefix("colors.") {
                match value {
                    Some(value_2) => widget_state.colors.insert(count.to_string(), value_2),
                    None => widget_state.colors.remove(count),
                };
            }
        }
    }
}

fn set_harness_field(widget_state: &mut File, value: Option<String>, key_value: &str) -> ControlFlow<()> {
    let Some((name, field)) = key_value["harness.".len()..].split_once('.') else {
        return ControlFlow::Break(());
    };

    match value {
        Some(value_value) => {
            widget_state
                .harness
                .entry(name.to_string())
                .or_default()
                .insert(field.to_string(), value_value);
        }
        None => {
            remove_harness_field(widget_state, name, field);
        }
    }

    ControlFlow::Continue(())
}

fn remove_harness_field(widget_state: &mut File, name: &str, field: &str) {
    if let Some(thread) = widget_state.harness.get_mut(name) {
        thread.remove(field);

        if thread.is_empty() {
            widget_state.harness.remove(name);
        }
    }
}
