//! Device config: ~/.config/codehead.toml ($CODEHEAD_CONFIG overrides the path).
//! Per key: env var first, then the file, then the built-in default.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::RwLock;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct File {
    pub notes: Option<String>,
    /// Home repo: a git repo with AGENTS.md (steering, notes, other repos) every agent reads.
    pub home: Option<String>,
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
    /// Changes view reviewer: `claude:<model>` (default `claude:sonnet`), `off`.
    pub review: Option<String>,
    /// The IDE: `auto` (default), a command name (code, cursor, zed, idea …) or a path.
    pub editor: Option<String>,
    /// `[keys] menu`: the menu key (F2).
    pub menu_key: Option<String>,
    /// `[keys] command`: the char that opens the command dropdown (`>`).
    pub command_key: Option<String>,
    /// `[colors] scheme`: a preset (coral, amber).
    pub scheme: Option<String>,
    /// `[colors]` overrides: color name (theme::NAMES) → `#RRGGBB`.
    pub colors: BTreeMap<String, String>,
    pub aliases: BTreeMap<String, String>,
}

static CFG: OnceLock<RwLock<File>> = OnceLock::new();

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

pub fn path() -> PathBuf {
    std::env::var_os("CODEHEAD_CONFIG")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config/codehead.toml"))
}

/// `~` and `~/x` → $HOME; relative paths resolve against $HOME.
fn expand(p: &str) -> PathBuf {
    let rest = p
        .strip_prefix("~/")
        .or(if p == "~" { Some("") } else { None });
    match rest {
        Some(r) => home().join(r),
        None => home().join(p),
    }
}

fn parse(text: &str) -> Result<File, String> {
    let t: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
    let mut f = File::default();
    for (k, v) in t {
        if k == "keys" {
            for (name, v) in v.as_table().ok_or("keys: must be a table")? {
                let v = v.as_str().ok_or_else(|| format!("keys.{name}: must be a string"))?.to_string();
                validate("keys.".to_string() + name, &v)?;
                match name.as_str() {
                    "menu" => f.menu_key = Some(v),
                    "command" => f.command_key = Some(v),
                    _ => return Err(format!("unknown key keys.{name}")),
                }
            }
            continue;
        }
        if k == "colors" {
            for (name, v) in v.as_table().ok_or("colors: must be a table")? {
                let v = v.as_str().ok_or_else(|| format!("colors.{name}: must be a string"))?.to_string();
                validate("colors.".to_string() + name, &v)?;
                if name == "scheme" {
                    f.scheme = Some(v);
                } else {
                    f.colors.insert(name.clone(), v);
                }
            }
            continue;
        }
        if k == "aliases" {
            let aliases = v.as_table().ok_or("aliases: must be a table")?;
            for (name, target) in aliases {
                validate_alias(
                    name,
                    target
                        .as_str()
                        .ok_or_else(|| format!("alias {name}: must be a string"))?,
                )?;
                f.aliases
                    .insert(name.clone(), target.as_str().unwrap().to_string());
            }
            continue;
        }
        let Some(s) = v.as_str().map(str::to_string) else {
            return Err(format!("{k}: must be a string"));
        };
        match k.as_str() {
            "notes" => f.notes = Some(s),
            "home" => f.home = Some(s),
            "styles" => f.styles = Some(s),
            "agent" => f.agent = Some(s),
            "prefix" => f.prefix = Some(s),
            "models" => f.models = Some(s),
            "thoughts" => {
                validate("thoughts".into(), &s)?;
                f.thoughts = Some(s);
            }
            "messages" => {
                validate("messages".into(), &s)?;
                f.messages = Some(s);
            }
            "edits" => {
                validate("edits".into(), &s)?;
                f.edits = Some(s);
            }
            "editor" => f.editor = Some(s),
            "rank" => {
                validate("rank".into(), &s)?;
                f.rank = Some(s);
            }
            "review" => {
                validate("review".into(), &s)?;
                f.review = Some(s);
            }
            _ => return Err(format!("unknown key {k}")),
        }
    }
    Ok(f)
}

/// Missing file → defaults. Broken file → error (codehead refuses to start on a typo).
pub fn load() -> Result<File, String> {
    if let Some(c) = CFG.get() {
        return c.read().map(|c| c.clone()).map_err(|e| e.to_string());
    }
    let p = path();
    let f = match std::fs::read_to_string(&p) {
        Ok(text) => parse(&text).map_err(|e| format!("{}: {e}", p.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => File::default(),
        Err(e) => return Err(format!("{}: {e}", p.display())),
    };
    crate::theme::set_palette(palette_of(&f));
    CFG.get_or_init(|| RwLock::new(f))
        .read()
        .map(|c| c.clone())
        .map_err(|e| e.to_string())
}

fn get() -> File {
    load().unwrap_or_else(|e| crate::fail(&e, 2))
}

/// Value V of setting KEY (a settings field: `messages`, `keys.menu`, `colors.accent` …): Err(why).
pub fn validate(key: String, v: &str) -> Result<(), String> {
    let v = v.trim();
    match key.as_str() {
        "messages" if crate::stream::Look::parse(v).is_none() => Err(format!("messages: bubbles or lines, not {v}")),
        "thoughts" if crate::stream::Thoughts::parse(v).is_none() => Err(format!("thoughts: hidden, folded or open, not {v}")),
        "edits" if crate::engine::edits::Mode::parse(v).is_none() => Err(format!("edits: off, files or diff, not {v}")),
        "rank" | "review" if v != "off" && !v.strip_prefix("claude:").is_some_and(|m| !m.is_empty()) => Err(format!("{key}: off or claude:<model>, not {v}")),
        "keys.menu" => Bind::parse(v).map(|_| ()).ok_or_else(|| format!("keys.menu: F1-F12, ctrl+<letter> or alt+<letter>, not {v}")),
        "keys.command" => command_char(v).map(|_| ()).ok_or_else(|| format!("keys.command: 1 symbol (not a letter, digit, space or @), not {v}")),
        "colors.scheme" if crate::theme::Palette::preset(v).is_none() => {
            Err(format!("colors.scheme: {}, not {v}", crate::theme::PRESETS.iter().map(|p| p.0).collect::<Vec<_>>().join(" or ")))
        }
        k if k.starts_with("colors.") && k != "colors.scheme" => {
            let name = &k["colors.".len()..];
            if !crate::theme::NAMES.contains(&name) {
                return Err(format!("unknown key {k}"));
            }
            crate::theme::parse_hex(v).map(|_| ()).ok_or_else(|| format!("{k}: #RRGGBB, not {v}"))
        }
        _ => Ok(()),
    }
}

/// A key of the keyboard as a setting: `F2`, `ctrl+p`, `alt+m`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bind {
    pub code: ratatui::crossterm::event::KeyCode,
    pub mods: ratatui::crossterm::event::KeyModifiers,
    /// As the hints show it.
    pub label: String,
}

impl Bind {
    pub fn parse(s: &str) -> Option<Bind> {
        use ratatui::crossterm::event::{KeyCode, KeyModifiers};
        let s = s.trim();
        let lower = s.to_ascii_lowercase();
        if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()).filter(|n| (1..=12).contains(n)) {
            return Some(Bind { code: KeyCode::F(n), mods: KeyModifiers::NONE, label: format!("F{n}") });
        }
        let (mods, rest, name) = if let Some(r) = lower.strip_prefix("ctrl+") {
            (KeyModifiers::CONTROL, r, "ctrl")
        } else {
            (KeyModifiers::ALT, lower.strip_prefix("alt+")?, "alt")
        };
        let mut chars = rest.chars();
        let c = chars.next().filter(|c| c.is_ascii_lowercase())?;
        chars.next().is_none().then(|| Bind { code: KeyCode::Char(c), mods, label: format!("{name}+{c}") })
    }

    /// K is this key.
    pub fn matches(&self, k: &ratatui::crossterm::event::KeyEvent) -> bool {
        use ratatui::crossterm::event::{KeyCode, KeyModifiers};
        let core = KeyModifiers::CONTROL | KeyModifiers::ALT;
        match (self.code, k.code) {
            (KeyCode::Char(a), KeyCode::Char(b)) => a == b.to_ascii_lowercase() && k.modifiers.intersection(core) == self.mods,
            (a, b) => a == b && k.modifiers.intersection(core) == self.mods,
        }
    }
}

/// The command trigger of S: 1 symbol.
fn command_char(s: &str) -> Option<char> {
    let mut c = s.trim().chars();
    let ch = c.next()?;
    (c.next().is_none() && !ch.is_alphanumeric() && !ch.is_whitespace() && ch != '@').then_some(ch)
}

pub fn validate_alias(name: &str, target: &str) -> Result<(), String> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-.:".contains(c))
        || matches!(name, "." | "..")
    {
        return Err("alias name: use letters, numbers, colon, dot, dash, underscore".into());
    }
    if target.trim().is_empty() || target.contains(['\n', '\r', '\0']) {
        return Err(format!("alias {name}: path required"));
    }
    Ok(())
}

pub fn read_file(path: &std::path::Path) -> Result<(String, File), String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.to_string()),
    };
    let file = parse(&text)?;
    Ok((text, file))
}

fn document(base: &str, file: &File) -> Result<String, String> {
    let mut doc = base
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| e.to_string())?;
    for (key, value) in [
        ("notes", &file.notes),
        ("home", &file.home),
        ("styles", &file.styles),
        ("agent", &file.agent),
        ("prefix", &file.prefix),
        ("models", &file.models),
        ("messages", &file.messages),
        ("thoughts", &file.thoughts),
        ("edits", &file.edits),
        ("rank", &file.rank),
        ("review", &file.review),
        ("editor", &file.editor),
    ] {
        if let Some(value) = value {
            if doc.get(key).and_then(|v| v.as_str()) != Some(value) {
                let mut new = toml_edit::Value::from(value.clone());
                if let Some(old) = doc.get(key).and_then(|v| v.as_value()) {
                    *new.decor_mut() = old.decor().clone();
                }
                doc[key] = toml_edit::Item::Value(new);
            }
        } else {
            doc.remove(key);
        }
    }
    let keys: BTreeMap<String, String> = [("menu", &file.menu_key), ("command", &file.command_key)]
        .into_iter()
        .filter_map(|(k, v)| v.clone().map(|v| (k.to_string(), v)))
        .collect();
    let mut colors = file.colors.clone();
    if let Some(s) = &file.scheme {
        colors.insert("scheme".into(), s.clone());
    }
    for (name, table) in [("keys", &keys), ("colors", &colors)] {
        if table.is_empty() {
            doc.remove(name);
            continue;
        }
        if !doc.contains_key(name) {
            doc[name] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let t = doc[name].as_table_like_mut().ok_or_else(|| format!("{name}: must be a table"))?;
        let gone: Vec<String> = t.iter().filter(|(k, _)| !table.contains_key(*k)).map(|(k, _)| k.to_string()).collect();
        for k in gone {
            t.remove(&k);
        }
        for (k, v) in table {
            if t.get(k).and_then(|x| x.as_str()) != Some(v) {
                t.insert(k, toml_edit::Item::Value(toml_edit::Value::from(v.clone())));
            }
        }
    }
    if !file.aliases.is_empty() || doc.contains_key("aliases") {
        if !doc.contains_key("aliases") {
            doc["aliases"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let table = doc["aliases"]
            .as_table_like_mut()
            .ok_or("aliases: must be a table")?;
        let removed: Vec<String> = table
            .iter()
            .filter(|(k, _)| !file.aliases.contains_key(*k))
            .map(|(k, _)| k.to_string())
            .collect();
        for key in removed {
            table.remove(&key);
        }
        for (key, target) in &file.aliases {
            if table.get(key).and_then(|v| v.as_str()) != Some(target) {
                let mut new = toml_edit::Value::from(target.clone());
                if let Some(old) = table.get(key).and_then(|v| v.as_value()) {
                    *new.decor_mut() = old.decor().clone();
                }
                if let Some(old) = table.get_mut(key) {
                    *old = toml_edit::Item::Value(new);
                } else {
                    table.insert(key, toml_edit::Item::Value(new));
                }
            }
        }
    }
    let out = doc.to_string();
    parse(&out)?;
    Ok(out)
}

/// Preserve comments; reject a concurrent edit; replace the file atomically.
pub fn save_file(path: &std::path::Path, base: &str, file: &File) -> Result<String, String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let path = target.as_path();
    let text = document(base, file)?;
    let (current, _) = read_file(path)?;
    if current != base {
        return Err("config changed outside codehead; reload before save".into());
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("{}.{}.tmp", std::process::id(), seq));
    let result = (|| {
        let mut out = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        out.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        out.sync_all().map_err(|e| e.to_string())?;
        // Check again after preparing the replacement.
        if read_file(path)?.0 != base {
            return Err("config changed outside codehead; reload before save".into());
        }
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
        Ok(text)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Setting KEY of F (a settings field), None: unset (the default applies).
impl File {
    pub fn get(&self, key: &str) -> Option<String> {
        match key {
            "notes" => self.notes.clone(),
            "home" => self.home.clone(),
            "styles" => self.styles.clone(),
            "agent" => self.agent.clone(),
            "prefix" => self.prefix.clone(),
            "models" => self.models.clone(),
            "messages" => self.messages.clone(),
            "thoughts" => self.thoughts.clone(),
            "edits" => self.edits.clone(),
            "rank" => self.rank.clone(),
            "review" => self.review.clone(),
            "editor" => self.editor.clone(),
            "keys.menu" => self.menu_key.clone(),
            "keys.command" => self.command_key.clone(),
            "colors.scheme" => self.scheme.clone(),
            k => k.strip_prefix("colors.").and_then(|n| self.colors.get(n).cloned()),
        }
    }

    pub fn set(&mut self, key: &str, v: Option<String>) {
        match key {
            "notes" => self.notes = v,
            "home" => self.home = v,
            "styles" => self.styles = v,
            "agent" => self.agent = v,
            "prefix" => self.prefix = v,
            "models" => self.models = v,
            "messages" => self.messages = v,
            "thoughts" => self.thoughts = v,
            "edits" => self.edits = v,
            "rank" => self.rank = v,
            "review" => self.review = v,
            "editor" => self.editor = v,
            "keys.menu" => self.menu_key = v,
            "keys.command" => self.command_key = v,
            "colors.scheme" => self.scheme = v,
            k => {
                if let Some(n) = k.strip_prefix("colors.") {
                    match v {
                        Some(v) => self.colors.insert(n.to_string(), v),
                        None => self.colors.remove(n),
                    };
                }
            }
        }
    }
}

/// The palette of F: its scheme ($CODEHEAD_SCHEME first) + its color overrides.
pub fn palette_of(f: &File) -> crate::theme::Palette {
    let scheme = env("CODEHEAD_SCHEME").or_else(|| f.scheme.clone());
    crate::theme::Palette::build(scheme.as_deref(), f.colors.iter().map(|(k, v)| (k.as_str(), v.as_str())))
}

/// The menu key: $CODEHEAD_MENU_KEY, else `[keys] menu`, else F2.
pub fn menu_key() -> Bind {
    // tests: the defaults, never the machine's config
    let set = if cfg!(test) { None } else { env("CODEHEAD_MENU_KEY").or_else(|| load().ok()?.menu_key) };
    set.and_then(|k| Bind::parse(&k)).unwrap_or_else(|| Bind::parse("F2").expect("F2 parses"))
}

/// The command trigger: $CODEHEAD_COMMAND_KEY, else `[keys] command`, else `>`.
pub fn command_key() -> char {
    let set = if cfg!(test) { None } else { env("CODEHEAD_COMMAND_KEY").or_else(|| load().ok()?.command_key) };
    set.and_then(|k| command_char(&k)).unwrap_or('>')
}

pub fn apply(file: File) {
    crate::theme::set_palette(palette_of(&file));
    *CFG.get_or_init(|| RwLock::new(File::default()))
        .write()
        .unwrap_or_else(|e| e.into_inner()) = file;
}

pub fn resolve_alias(path: &str) -> Option<PathBuf> {
    alias_path(&get().aliases, path)
}

pub fn alias_path(aliases: &BTreeMap<String, String>, path: &str) -> Option<PathBuf> {
    let (name, rest) = path.split_once('/')?;
    let target = aliases.get(name)?;
    Some(expand(target).join(rest))
}

/// Expand alias path tokens at send time, retaining punctuation and ordinary text.
pub fn expand_aliases(text: &str) -> String {
    expand_with(&get().aliases, text)
}

fn expand_with(aliases: &BTreeMap<String, String>, text: &str) -> String {
    let boundary = |c: char| c.is_whitespace() || "()[]\"'`<>,".contains(c);
    let mut out = String::new();
    let mut start = 0;
    for (i, ch) in text.char_indices() {
        if boundary(ch) {
            let word = &text[start..i];
            out.push_str(
                &alias_path(aliases, word)
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| word.to_string()),
            );
            out.push(ch);
            start = i + ch.len_utf8();
        }
    }
    let word = &text[start..];
    out.push_str(
        &alias_path(aliases, word)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| word.to_string()),
    );
    out
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

fn app_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn defaults() -> File {
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
        menu_key: Some("F2".into()),
        command_key: Some(">".into()),
        scheme: Some(crate::theme::PRESETS[0].0.into()),
        ..File::default()
    }
}

/// $CODEHEAD_NOTES, else `notes`, else notes/ of the app dir.
pub fn notes() -> PathBuf {
    env("CODEHEAD_NOTES")
        .map(PathBuf::from)
        .or_else(|| get().notes.as_deref().map(expand))
        .unwrap_or_else(|| app_dir().join("notes"))
}

/// The home repo: $CODEHEAD_HOME_REPO, else `home`. None: unset or 0 dir.
pub fn home_repo() -> Option<PathBuf> {
    env("CODEHEAD_HOME_REPO")
        .map(PathBuf::from)
        .or_else(|| get().home.as_deref().filter(|s| !s.is_empty()).map(expand))
        .filter(|p| p.is_dir())
}

/// $CODEHEAD_STYLES, else `styles`, else output-styles/ of the app dir.
pub fn styles() -> PathBuf {
    env("CODEHEAD_STYLES")
        .map(PathBuf::from)
        .or_else(|| get().styles.as_deref().map(expand))
        .unwrap_or_else(|| app_dir().join("output-styles"))
}

/// $CODEHEAD_AGENT, else `agent`.
pub fn agent() -> Option<String> {
    env("CODEHEAD_AGENT").or_else(|| get().agent.clone().filter(|s| !s.is_empty()))
}

/// $CODEHEAD_PREFIX, else `prefix`, else "task".
pub fn prefix() -> String {
    env("CODEHEAD_PREFIX")
        .or_else(|| get().prefix.clone().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "task".into())
}

/// Claude Code models for the agent form: $CODEHEAD_MODELS, else `models` (comma-separated), else the aliases.
pub fn models() -> Vec<String> {
    let raw = env("CODEHEAD_MODELS")
        .or_else(|| get().models.clone())
        .unwrap_or_else(|| "fable,opus,sonnet,haiku".into());
    raw.split(',')
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .collect()
}

/// How the thread view shows thoughts: $CODEHEAD_THOUGHTS, else `thoughts`, else hidden.
pub fn thoughts() -> crate::stream::Thoughts {
    env("CODEHEAD_THOUGHTS")
        .or_else(|| get().thoughts.clone())
        .and_then(|m| crate::stream::Thoughts::parse(&m))
        .unwrap_or_default()
}

/// What an agent learns of others' edits: $CODEHEAD_EDITS, else `edits`, else diff.
pub fn edits() -> crate::engine::edits::Mode {
    env("CODEHEAD_EDITS")
        .or_else(|| get().edits.clone())
        .and_then(|m| crate::engine::edits::Mode::parse(&m))
        .unwrap_or_default()
}

/// The changes view's reviewer model: $CODEHEAD_REVIEW, else `review`, else claude:sonnet.
/// None: `off`.
pub fn review_model() -> Option<String> {
    let v = env("CODEHEAD_REVIEW").or_else(|| get().review.clone()).unwrap_or_else(|| "claude:sonnet".into());
    v.strip_prefix("claude:").filter(|m| !m.is_empty()).map(str::to_string)
}

/// The `editor` setting (None: unset; `auto` applies).
pub fn editor() -> Option<String> {
    get().editor.clone().filter(|s| !s.is_empty())
}

/// The model that ranks the changes view: $CODEHEAD_RANK, else `rank`, else claude:haiku.
/// None: `off` (path rules only).
pub fn rank_model() -> Option<String> {
    let v = env("CODEHEAD_RANK").or_else(|| get().rank.clone()).unwrap_or_else(|| "claude:haiku".into());
    v.strip_prefix("claude:").filter(|m| !m.is_empty()).map(str::to_string)
}

/// How the thread view draws messages: $CODEHEAD_MESSAGES, else `messages`, else bubbles.
pub fn messages() -> crate::stream::Look {
    env("CODEHEAD_MESSAGES")
        .or_else(|| get().messages.clone())
        .and_then(|m| crate::stream::Look::parse(&m))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_colors_tables_round_trip() {
        let f = parse("[keys]\nmenu = \"ctrl+p\"\ncommand = \"!\"\n[colors]\nscheme = \"amber\"\naccent = \"#112233\"\n").unwrap();
        assert_eq!((f.menu_key.as_deref(), f.command_key.as_deref(), f.scheme.as_deref()), (Some("ctrl+p"), Some("!"), Some("amber")));
        assert_eq!(f.colors["accent"], "#112233");
        let p = palette_of(&f);
        assert_eq!(p.accent, ratatui::style::Color::Rgb(0x11, 0x22, 0x33));
        assert_eq!(p.text, crate::theme::AMBER.text);
        // written back: same tables; unset keys go
        let mut g = f.clone();
        g.set("colors.accent", None);
        g.set("keys.command", None);
        let text = document("[keys]\nmenu = \"ctrl+p\"\ncommand = \"!\"\n[colors]\nscheme = \"amber\"\naccent = \"#112233\"\n", &g).unwrap();
        let back = parse(&text).unwrap();
        assert_eq!(back, g);
        for bad in ["[keys]\nmenu = \"x\"", "[keys]\ncommand = \"ab\"", "[colors]\nscheme = \"pink\"", "[colors]\naccent = \"red\"", "[colors]\nfoo = \"#000000\""] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        let b = Bind::parse("ctrl+p").unwrap();
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        assert!(b.matches(&KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL)));
        assert!(!b.matches(&KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)));
        assert!(Bind::parse("F2").unwrap().matches(&KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE)));
    }

    #[test]
    fn parses_keys() {
        let f = parse("notes = \"~/n\"\nagent = \"amon\"").unwrap();
        assert_eq!(f.notes.as_deref(), Some("~/n"));
        assert_eq!(f.agent.as_deref(), Some("amon"));
    }

    #[test]
    fn rejects_unknown_and_non_string() {
        assert!(parse("note = \"x\"").is_err());
        assert!(parse("notes = 1").is_err());
    }

    #[test]
    fn expands_home() {
        let h = home();
        assert_eq!(expand("~/a"), h.join("a"));
        assert_eq!(expand("~"), h);
        assert_eq!(expand("/abs"), PathBuf::from("/abs"));
        assert_eq!(expand("rel"), h.join("rel"));
    }

    #[test]
    fn aliases_parse_resolve_and_expand_at_token_boundaries() {
        let f = parse(
            "[aliases]\n\"project:platform\" = \"~/notes/platform/\"\nshort = \"/tmp/shared\"\n",
        )
        .unwrap();
        assert_eq!(
            alias_path(&f.aliases, "project:platform/reports/a.md"),
            Some(home().join("notes/platform/reports/a.md"))
        );
        assert_eq!(
            alias_path(&f.aliases, "short/"),
            Some(PathBuf::from("/tmp/shared/"))
        );
        assert_eq!(alias_path(&f.aliases, "unknown/a"), None);
        assert_eq!(alias_path(&f.aliases, "shorter/a"), None);
        let text = "read `project:platform/a.md`, (short/b.md) https://example.com/short/a";
        assert_eq!(
            expand_with(&f.aliases, text),
            format!(
                "read `{}/a.md`, (/tmp/shared/b.md) https://example.com/short/a",
                home().join("notes/platform").display()
            )
        );
        for invalid in [
            "aliases = 'x'",
            "[aliases]\nwrong = 2",
            "[aliases]\n'bad/name' = '/tmp'",
            "[aliases]\nempty = ''",
        ] {
            assert!(parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn saves_preserve_comments_and_reject_external_changes() {
        let dir = std::env::temp_dir().join(format!("codehead-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("codehead.toml");
        let base = "# user settings\nnotes = '~/old' # notes comment\nprefix = 'task'\n\n[aliases]\n# shared notes\n\"project:platform\" = '~/old/' # alias comment\n";
        std::fs::write(&path, base).unwrap();
        let mut file = parse(base).unwrap();
        file.notes = Some("~/new".into());
        file.aliases
            .insert("project:platform".into(), "~/new/".into());
        let saved = save_file(&path, base, &file).unwrap();
        for comment in [
            "# user settings",
            "# notes comment",
            "# shared notes",
            "# alias comment",
        ] {
            assert!(saved.contains(comment), "{saved}");
        }
        assert_eq!(parse(&saved).unwrap(), file);
        std::fs::write(&path, "prefix = 'external'\n").unwrap();
        assert!(
            save_file(&path, &saved, &file)
                .unwrap_err()
                .contains("changed outside codehead")
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "prefix = 'external'\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
