//! Settings: ~/.config/codehead.toml ($`CODEHEAD_CONFIG` overrides the path); with a home repo that
//! has its codehead dir also `<home>/codehead/settings.toml` (shared) and
//! `<home>/codehead/devices/<device>.toml` (this device). Per key: env var first, then the files
//! (device, shared, codehead.toml), then the built-in default.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::RwLock;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct File {
    pub notes: Option<String>,
    /// Home repo: a git repo with AGENTS.md (steering, notes, other repos) every agent reads.
    pub home: Option<String>,
    /// This device's name: its file in the home repo (`codehead/devices/<device>.toml`). Unset: the host name.
    pub device: Option<String>,
    /// Repo of the new tab's where row; unset: the repo of the dir codehead starts in.
    pub repo: Option<String>,
    pub styles: Option<String>,
    pub agent: Option<String>,
    pub prefix: Option<String>,
    pub models: Option<String>,
    /// Thread view messages: `bubbles` (default) or `lines`.
    pub messages: Option<String>,
    /// Thread view thoughts: `hidden` (default), `folded` or `open`.
    pub thoughts: Option<String>,
    /// What an agent learns of edits in its worktree since its last turn, not its own: `off`,
    /// `files` or `diff` (default).
    pub edits: Option<String>,
    /// Changes view order: `claude:<model>` (default `claude:haiku`) ranks the files, `off`: path rules.
    pub rank: Option<String>,
    /// Changes view classifier (tabs + order): an agent's name (empty: the default agent), `off`.
    pub classify: Option<String>,
    /// Changes view reviewer: `claude:<model>` (default `claude:sonnet`), `off`.
    pub review: Option<String>,
    /// The IDE: `auto` (default), a command name (code, cursor, zed, idea …) or a path.
    pub editor: Option<String>,
    /// Dev server ports: `auto` (default: portless when on PATH), `portless`, `block`.
    pub ports: Option<String>,
    /// Files view image + pdf previews: `auto` (default: the protocol from the terminal's env),
    /// `kitty`, `iterm2`, `sixel`, or `off`.
    pub previews: Option<String>,
    /// `[keys] menu`: the menu key (ctrl+p, on every screen).
    pub menu_key: Option<String>,
    /// `[keys] command`: the char that opens the command dropdown (`>`).
    pub command_key: Option<String>,
    /// `[colors] scheme`: a preset (coral, amber).
    pub scheme: Option<String>,
    /// `[colors]` overrides: color name (`theme::NAMES`, or an old name of `theme::ALIASES`) → `#RRGGBB`.
    pub colors: BTreeMap<String, String>,
    pub aliases: BTreeMap<String, String>,
    /// `[harness.<name>]`: defaults of a harness's agents (`HARNESS_FIELDS`) → value.
    pub harness: BTreeMap<String, BTreeMap<String, String>>,
}

/// Fields of a `[harness.<name>]` table: the model, effort (Claude Code), the ACP command,
/// extra arguments (Claude Code).
pub(crate) const HARNESS_FIELDS: [&str; 4] = ["model", "effort", "command", "args"];

/// Field FIELD of `[harness.NAME]`, read from the file now (a long-lived host sees edits).
pub(crate) fn harness_opt(name: &str, field: &str) -> Option<String> {
    let file = read_layers(&path()).map(|line| line.merged()).unwrap_or_default();

    file.harness
        .get(name)?
        .get(field)
        .cloned()
        .filter(|value| !value.trim().is_empty())
}

static CFG: OnceLock<RwLock<File>> = OnceLock::new();

pub(crate) fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

pub(crate) fn path() -> PathBuf {
    std::env::var_os("CODEHEAD_CONFIG")
        .filter(|text| !text.is_empty())
        .map_or_else(|| home().join(".config/codehead.toml"), PathBuf::from)
}

/// `~` and `~/x` → $HOME; relative paths resolve against $HOME.
pub(crate) fn expand(path: &str) -> PathBuf {
    let rest = path.strip_prefix("~/").or(if path == "~" { Some("") } else { None });

    match rest {
        Some(result) => home().join(result),
        None => home().join(path),
    }
}

/// Missing file → defaults. Broken file → error (codehead refuses to start on a typo).
pub(crate) fn load() -> Result<File, String> {
    if let Some(character) = CFG.get() {
        return character
            .read()
            .map(|character_value| character_value.clone())
            .map_err(|error| error.to_string());
    }

    let file = read_layers(&path())?.merged();

    crate::domain::palette::set_palette(palette_of(&file));
    CFG.get_or_init(|| RwLock::new(file))
        .read()
        .map(|character_2| character_2.clone())
        .map_err(|error_value| error_value.to_string())
}

fn get() -> File {
    load().unwrap_or_else(|error| crate::infra::terminal::fail(&error, 2))
}

/// The plain keys of a settings file (the tables: colors, aliases, harness).
const PLAIN_KEYS: [&str; 20] = [
    "notes",
    "home",
    "device",
    "repo",
    "styles",
    "agent",
    "prefix",
    "models",
    "messages",
    "thoughts",
    "edits",
    "rank",
    "classify",
    "review",
    "editor",
    "ports",
    "previews",
    "keys.menu",
    "keys.command",
    "colors.scheme",
];

/// Value V of setting KEY (a settings field: `messages`, `keys.menu`, `colors.accent` …): Err(why).
pub(crate) fn validate(key: &str, value: &str) -> Result<(), String> {
    let value_value = value.trim();

    match key {
        "messages" if crate::domain::chat::Look::parse(value_value).is_none() => Err(format!("messages: bubbles or lines, not {value_value}")),
        "thoughts" if crate::domain::chat::Thoughts::parse(value_value).is_none() => Err(format!("thoughts: hidden, folded or open, not {value_value}")),
        "edits" if crate::engine::edits::Mode::parse(value_value).is_none() => Err(format!("edits: off, files or diff, not {value_value}")),
        "ports" if crate::infra::portless::Mode::parse(value_value).is_none() => Err(format!("ports: auto, portless or block, not {value_value}")),
        "previews" if !matches!(value_value, "auto" | "kitty" | "iterm2" | "sixel" | "off") => Err(format!("previews: auto, kitty, iterm2, sixel or off, not {value_value}")),
        "rank" | "review" if value_value != "off" && value_value.strip_prefix("claude:").is_none_or(str::is_empty) => Err(format!("{key}: off or claude:<model>, not {value_value}")),
        "classify" if !value_value.is_empty() && value_value != "off" && !crate::engine::roster::valid_name(value_value) => Err(format!("classify: an agent's name or off, not {value_value}")),
        "keys.menu" => menu_bind(value_value).map(|_| ()).ok_or_else(|| format!("keys.menu: F7-F12, ctrl+<letter> or alt+<letter> codehead binds to nothing else (free: ctrl+l, ctrl+p …), not {value_value}")),
        "keys.command" => command_char(value_value).map(|_| ()).ok_or_else(|| format!("keys.command: 1 symbol (not a letter, digit, space, @ ! # / ~), not {value_value}")),
        "colors.scheme" if crate::domain::palette::Palette::preset(value_value).is_none() => {
            Err(format!("colors.scheme: {}, not {value_value}", crate::domain::palette::PRESETS.iter().map(|path| path.0).collect::<Vec<_>>().join(" or ")))
        }
        key_value if key_value.starts_with("harness.") => {
            let mut parts = key_value.splitn(3, '.').skip(1);
            let (name, field) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default());

            if crate::engine::harnesses::get(name).is_none() {
                return Err(format!("{key_value}: unknown harness {name}"));
            }

            if !HARNESS_FIELDS.contains(&field) {
                return Err(format!("unknown key {key_value}"));
            }

            if field == "effort" && !value_value.is_empty() && !["low", "medium", "high", "xhigh", "max"].contains(&value_value) {
                return Err(format!("{key_value}: low, medium, high, xhigh or max, not {value_value}"));
            }

            Ok(())
        }
        key_2 if key_2.starts_with("colors.") && key_2 != "colors.scheme" => {
            let name = &key_2["colors.".len()..];

            if !crate::domain::palette::NAMES.contains(&crate::domain::palette::slot_name(name)) {
                return Err(format!("unknown key {key_2}"));
            }

            crate::domain::palette::parse_hex(value_value).map(|_| ()).ok_or_else(|| format!("{key_2}: #RRGGBB, not {value_value}"))
        }
        _ => Ok(()),
    }
}

/// A key of the keyboard as a setting: `F2`, `ctrl+p`, `alt+m`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Bind {
    pub code: ratatui::crossterm::event::KeyCode,
    pub mods: ratatui::crossterm::event::KeyModifiers,
    /// As the hints show it.
    pub label: String,
}

impl Bind {
    pub(crate) fn parse(text: &str) -> Option<Bind> {
        use ratatui::crossterm::event::{KeyCode, KeyModifiers};
        let text_value = text.trim();
        let lower = text_value.to_ascii_lowercase();

        if let Some(count_2) = lower
            .strip_prefix('f')
            .and_then(|count| count.parse::<u8>().ok())
            .filter(|count_value| (1..=12).contains(count_value))
        {
            return Some(Bind {
                code: KeyCode::F(count_2),
                mods: KeyModifiers::NONE,
                label: format!("F{count_2}"),
            });
        }

        let (mods, rest, name) = if let Some(result) = lower.strip_prefix("ctrl+") {
            (KeyModifiers::CONTROL, result, "ctrl")
        } else {
            (KeyModifiers::ALT, lower.strip_prefix("alt+")?, "alt")
        };

        let mut chars = rest.chars();
        let character = chars.next().filter(char::is_ascii_lowercase)?;

        chars.next().is_none().then(|| Bind {
            code: KeyCode::Char(character),
            mods,
            label: format!("{name}+{character}"),
        })
    }

    /// K is this key.
    pub(crate) fn matches(&self, key: &ratatui::crossterm::event::KeyEvent) -> bool {
        use ratatui::crossterm::event::{KeyCode, KeyModifiers};
        let core = KeyModifiers::CONTROL | KeyModifiers::ALT;

        // ⌘ (kitty protocol: super) = ctrl: cmd+p as ctrl+p
        let mut mods = key.modifiers;

        if mods.contains(KeyModifiers::SUPER) {
            mods.insert(KeyModifiers::CONTROL);
        }

        match (self.code, key.code) {
            (KeyCode::Char(agent), KeyCode::Char(bytes)) => {
                agent == bytes.to_ascii_lowercase() && mods.intersection(core) == self.mods
            }
            (agent_value, bytes_value) => agent_value == bytes_value && mods.intersection(core) == self.mods,
        }
    }
}

/// The command trigger of S: 1 symbol, not a trigger of its own (@ agents, ! shell, # shortcuts,
/// / ~ paths).
fn command_char(text: &str) -> Option<char> {
    let mut character = text.trim().chars();
    let ch = character.next()?;

    (character.next().is_none() && !ch.is_alphanumeric() && !ch.is_whitespace() && !"@!#/~".contains(ch)).then_some(ch)
}

/// Keys codehead binds itself: a menu key on 1 of them never fires or hides a built-in.
const TAKEN: [&str; 30] = [
    "ctrl+a", "ctrl+b", "ctrl+c", "ctrl+d", "ctrl+e", "ctrl+f", "ctrl+g", "ctrl+h", "ctrl+i", "ctrl+k", "ctrl+m",
    "ctrl+n", "ctrl+o", "ctrl+q", "ctrl+r", "ctrl+s", "ctrl+t", "ctrl+u", "ctrl+v", "ctrl+w", "ctrl+x", "ctrl+y",
    "ctrl+z", "ctrl+.", "F1", "F2", "F3", "F4", "F5", "F6",
];

/// The menu key of S: a valid key codehead binds to nothing else (alt+b/d/f: word keys too).
fn menu_bind(text: &str) -> Option<Bind> {
    Bind::parse(text).filter(|bytes| {
        !TAKEN.contains(&bytes.label.as_str()) && !matches!(bytes.label.as_str(), "alt+b" | "alt+d" | "alt+f")
    })
}

pub(crate) fn validate_alias(name: &str, target: &str) -> Result<(), String> {
    if name.is_empty()
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_-.:".contains(character))
        || matches!(name, "." | "..")
    {
        return Err("alias name: use letters, numbers, colon, dot, dash, underscore".into());
    }

    if target.trim().is_empty() || target.contains(['\n', '\r', '\0']) {
        return Err(format!("alias {name}: path required"));
    }

    Ok(())
}

/// The menu key: $`CODEHEAD_MENU_KEY`, else `[keys] menu`, else ctrl+p.
pub(crate) fn menu_key() -> Bind {
    // tests: the defaults, never the machine's config
    let set = if cfg!(test) {
        None
    } else {
        env("CODEHEAD_MENU_KEY").or_else(|| load().ok()?.menu_key)
    };

    // a key codehead binds itself (F1-F6: the tabs, ctrl+n: notifications …): the default
    set.and_then(|key| menu_bind(&key))
        .unwrap_or_else(|| Bind::parse("ctrl+p").expect("ctrl+p parses"))
}

/// The command trigger: $`CODEHEAD_COMMAND_KEY`, else `[keys] command`, else `>`.
pub(crate) fn command_key() -> char {
    let set = if cfg!(test) {
        None
    } else {
        env("CODEHEAD_COMMAND_KEY").or_else(|| load().ok()?.command_key)
    };

    set.and_then(|key| command_char(&key)).unwrap_or('>')
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

#[path = "config_parse.rs"]
mod parse;
use parse::parse;
#[path = "config_layers.rs"]
mod layers;
pub(crate) use layers::{Layer, Layers, device_of, layout, layout_of, read_layers, save_layers};
#[path = "config_document.rs"]
mod document;
pub(crate) use document::{read_file, save_file};
#[path = "config_values.rs"]
mod values;
use values::env;
pub(crate) use values::{
    agent, apply, classify, default_repo, defaults, editor, edits, expand_aliases, home_repo, messages, models, notes,
    palette_of, ports, prefix, previews, rank_model, resolve_alias, review_model, styles, thoughts,
};

#[cfg(test)]
use document::document;
#[cfg(test)]
use layers::{Scope, host_device, overlay, scope, slug, split};
#[cfg(test)]
use values::{alias_path, expand_with};
