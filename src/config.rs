//! Settings: ~/.config/codehead.toml ($CODEHEAD_CONFIG overrides the path); with a home repo that
//! has its codehead dir also `<home>/codehead/settings.toml` (shared) and
//! `<home>/codehead/devices/<device>.toml` (this device). Per key: env var first, then the files
//! (device, shared, codehead.toml), then the built-in default.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::RwLock;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct File {
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
    /// `[colors]` overrides: color name (theme::NAMES, or an old name of theme::ALIASES) → `#RRGGBB`.
    pub colors: BTreeMap<String, String>,
    pub aliases: BTreeMap<String, String>,
    /// `[harness.<name>]`: defaults of a harness's agents (HARNESS_FIELDS) → value.
    pub harness: BTreeMap<String, BTreeMap<String, String>>,
}

/// Fields of a `[harness.<name>]` table: the model, effort (Claude Code), the ACP command,
/// extra arguments (Claude Code).
pub const HARNESS_FIELDS: [&str; 4] = ["model", "effort", "command", "args"];

/// Field FIELD of `[harness.NAME]`, read from the file now (a long-lived host sees edits).
pub fn harness_opt(name: &str, field: &str) -> Option<String> {
    let f = read_layers(&path()).map(|l| l.merged()).unwrap_or_default();
    f.harness.get(name)?.get(field).cloned().filter(|v| !v.trim().is_empty())
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
pub(crate) fn expand(p: &str) -> PathBuf {
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
                    // an old name (`panel`, `ok`, …): kept under its slot's name
                    f.colors.insert(crate::theme::slot_name(name).to_string(), v);
                }
            }
            continue;
        }
        if k == "harness" {
            for (name, t) in v.as_table().ok_or("harness: must be a table")? {
                for (field, v) in t.as_table().ok_or_else(|| format!("harness.{name}: must be a table"))? {
                    let key = format!("harness.{name}.{field}");
                    let v = v.as_str().ok_or_else(|| format!("{key}: must be a string"))?.to_string();
                    validate(key, &v)?;
                    f.harness.entry(name.clone()).or_default().insert(field.clone(), v);
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
            "device" => f.device = Some(s),
            "repo" => f.repo = Some(s),
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
            "previews" => {
                validate("previews".into(), &s)?;
                f.previews = Some(s);
            }
            "ports" => {
                validate("ports".into(), &s)?;
                f.ports = Some(s);
            }
            "rank" => {
                validate("rank".into(), &s)?;
                f.rank = Some(s);
            }
            "classify" => {
                validate("classify".into(), &s)?;
                f.classify = Some(s);
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
    let f = read_layers(&path())?.merged();
    crate::theme::set_palette(palette_of(&f));
    CFG.get_or_init(|| RwLock::new(f))
        .read()
        .map(|c| c.clone())
        .map_err(|e| e.to_string())
}

fn get() -> File {
    load().unwrap_or_else(|e| crate::fail(&e, 2))
}

/// The plain keys of a settings file (the tables: colors, aliases, harness).
const PLAIN_KEYS: [&str; 20] = [
    "notes", "home", "device", "repo", "styles", "agent", "prefix", "models", "messages", "thoughts", "edits", "rank", "classify", "review", "editor", "ports", "previews", "keys.menu", "keys.command",
    "colors.scheme",
];

/// Where setting KEY lives once the home repo has its codehead dir: codehead.toml of this
/// machine (the pointer to home), this device's file in home, or home's shared settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Local,
    Device,
    Shared,
}

pub fn scope(key: &str) -> Scope {
    match key {
        "home" | "device" => Scope::Local,
        // paths and programs of this machine
        "notes" | "styles" | "repo" | "editor" | "ports" | "previews" => Scope::Device,
        k if k.starts_with("aliases.") => Scope::Device,
        k if k.starts_with("harness.") && k.ends_with(".command") => Scope::Device,
        _ => Scope::Shared,
    }
}

/// 1 settings file: its path, its text as read (the base of a save), its values.
#[derive(Clone, Debug)]
pub struct Layer {
    pub path: PathBuf,
    pub text: String,
    pub file: File,
}

impl Layer {
    fn read(path: PathBuf) -> Result<Layer, String> {
        let (text, file) = read_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Layer { path, text, file })
    }
}

/// The settings files of this machine: codehead.toml; with the home repo's codehead dir also
/// home's shared settings (`codehead/settings.toml`) and this device's file
/// (`codehead/devices/<device>.toml`). Per key: device, then shared, then codehead.toml.
#[derive(Clone, Debug)]
pub struct Layers {
    pub local: Layer,
    pub shared: Option<Layer>,
    pub device: Option<Layer>,
}

impl Layers {
    /// The values of all files, 1 per key.
    pub fn merged(&self) -> File {
        let mut f = self.local.file.clone();
        for l in [&self.shared, &self.device].into_iter().flatten() {
            overlay(&mut f, &l.file);
        }
        // the pointer is this machine's alone
        f.home = self.local.file.home.clone();
        f.device = self.local.file.device.clone();
        f
    }
}

/// The values of TOP over BASE.
fn overlay(base: &mut File, top: &File) {
    for k in PLAIN_KEYS {
        if let Some(v) = top.get(k) {
            base.set(k, Some(v));
        }
    }
    base.colors.extend(top.colors.clone());
    base.aliases.extend(top.aliases.clone());
    for (h, t) in &top.harness {
        base.harness.entry(h.clone()).or_default().extend(t.clone());
    }
}

/// F split by scope: [codehead.toml, shared, device].
fn split(f: &File) -> [File; 3] {
    let mut out = [File::default(), File::default(), File::default()];
    let at = |k: &str| match scope(k) {
        Scope::Local => 0,
        Scope::Shared => 1,
        Scope::Device => 2,
    };
    for k in PLAIN_KEYS {
        if let Some(v) = f.get(k) {
            out[at(k)].set(k, Some(v));
        }
    }
    out[1].colors = f.colors.clone();
    out[2].aliases = f.aliases.clone();
    for (h, t) in &f.harness {
        for (field, v) in t {
            let k = format!("harness.{h}.{field}");
            out[at(&k)].set(&k, Some(v.clone()));
        }
    }
    out
}

/// The codehead dir of home repo HOME (`<home>/codehead`), when it exists.
pub fn layout_of(home: &std::path::Path) -> Option<PathBuf> {
    let d = home.join("codehead");
    d.is_dir().then_some(d)
}

/// The home repo's codehead dir: agents, flows, mcp.toml, settings live there. None: 0 home
/// repo or 0 codehead dir in it (tests: always None, never the owner's home).
pub fn layout() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    home_repo().and_then(|h| layout_of(&h))
}

/// S as a device name: lower case, letters, digits and `-`; the host name's 1st label.
pub fn slug(s: &str) -> String {
    let first = s.trim().split('.').next().unwrap_or_default().to_lowercase();
    let mut out = String::new();
    for c in first.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "device".into() } else { out }
}

/// The host name as a device name.
pub fn host_device() -> String {
    let mut buf = [0u8; 256];
    let ok = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    slug(if ok { std::str::from_utf8(&buf[..end]).unwrap_or_default() } else { "" })
}

/// The device name of F: `device`, else the host name.
pub fn device_of(f: &File) -> String {
    f.device.as_deref().filter(|d| !d.trim().is_empty()).map(slug).unwrap_or_else(host_device)
}

/// The settings files of codehead.toml LOCAL (its `home` names the home repo; env never: a
/// test inside a codehead thread must not reach the owner's home).
pub fn read_layers(local: &std::path::Path) -> Result<Layers, String> {
    let local = Layer::read(local.to_path_buf())?;
    let Some(dir) = local.file.home.as_deref().filter(|s| !s.trim().is_empty()).map(expand).and_then(|h| layout_of(&h)) else {
        return Ok(Layers { local, shared: None, device: None });
    };
    let shared = Layer::read(dir.join("settings.toml"))?;
    let device = Layer::read(dir.join("devices").join(format!("{}.toml", device_of(&local.file))))?;
    Ok(Layers { local, shared: Some(shared), device: Some(device) })
}

/// Save F (all values, as `merged` gives them) into the files of LAYERS, each key into the file
/// of its scope; only changed files are written. A new `home` or `device`: only the pointer in
/// codehead.toml changes (the files of the new place stay). Ok: the files read again.
pub fn save_layers(layers: &Layers, f: &File) -> Result<Layers, String> {
    let (Some(shared), Some(device)) = (&layers.shared, &layers.device) else {
        save_file(&layers.local.path, &layers.local.text, f)?;
        return read_layers(&layers.local.path);
    };
    if f.home != layers.local.file.home || f.device != layers.local.file.device {
        let mut local = layers.local.file.clone();
        local.home = f.home.clone();
        local.device = f.device.clone();
        save_file(&layers.local.path, &layers.local.text, &local)?;
        return read_layers(&layers.local.path);
    }
    let [local, sh, dev] = split(f);
    for (layer, new) in [(&layers.local, local), (shared, sh), (device, dev)] {
        if layer.file != new {
            save_file(&layer.path, &layer.text, &new)?;
        }
    }
    read_layers(&layers.local.path)
}

/// Value V of setting KEY (a settings field: `messages`, `keys.menu`, `colors.accent` …): Err(why).
pub fn validate(key: String, v: &str) -> Result<(), String> {
    let v = v.trim();
    match key.as_str() {
        "messages" if crate::stream::Look::parse(v).is_none() => Err(format!("messages: bubbles or lines, not {v}")),
        "thoughts" if crate::stream::Thoughts::parse(v).is_none() => Err(format!("thoughts: hidden, folded or open, not {v}")),
        "edits" if crate::engine::edits::Mode::parse(v).is_none() => Err(format!("edits: off, files or diff, not {v}")),
        "ports" if crate::portless::Mode::parse(v).is_none() => Err(format!("ports: auto, portless or block, not {v}")),
        "previews" if !matches!(v, "auto" | "kitty" | "iterm2" | "sixel" | "off") => Err(format!("previews: auto, kitty, iterm2, sixel or off, not {v}")),
        "rank" | "review" if v != "off" && !v.strip_prefix("claude:").is_some_and(|m| !m.is_empty()) => Err(format!("{key}: off or claude:<model>, not {v}")),
        "classify" if !v.is_empty() && v != "off" && !crate::engine::roster::valid_name(v) => Err(format!("classify: an agent's name or off, not {v}")),
        "keys.menu" => menu_bind(v).map(|_| ()).ok_or_else(|| format!("keys.menu: F7-F12, ctrl+<letter> or alt+<letter> codehead binds to nothing else (free: ctrl+l, ctrl+p …), not {v}")),
        "keys.command" => command_char(v).map(|_| ()).ok_or_else(|| format!("keys.command: 1 symbol (not a letter, digit, space, @ ! # / ~), not {v}")),
        "colors.scheme" if crate::theme::Palette::preset(v).is_none() => {
            Err(format!("colors.scheme: {}, not {v}", crate::theme::PRESETS.iter().map(|p| p.0).collect::<Vec<_>>().join(" or ")))
        }
        k if k.starts_with("harness.") => {
            let mut parts = k.splitn(3, '.').skip(1);
            let (name, field) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default());
            if crate::engine::harnesses::get(name).is_none() {
                return Err(format!("{k}: unknown harness {name}"));
            }
            if !HARNESS_FIELDS.contains(&field) {
                return Err(format!("unknown key {k}"));
            }
            if field == "effort" && !v.is_empty() && !["low", "medium", "high", "xhigh", "max"].contains(&v) {
                return Err(format!("{k}: low, medium, high, xhigh or max, not {v}"));
            }
            Ok(())
        }
        k if k.starts_with("colors.") && k != "colors.scheme" => {
            let name = &k["colors.".len()..];
            if !crate::theme::NAMES.contains(&crate::theme::slot_name(name)) {
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
        // ⌘ (kitty protocol: super) = ctrl: cmd+p as ctrl+p
        let mut mods = k.modifiers;
        if mods.contains(KeyModifiers::SUPER) {
            mods.insert(KeyModifiers::CONTROL);
        }
        match (self.code, k.code) {
            (KeyCode::Char(a), KeyCode::Char(b)) => a == b.to_ascii_lowercase() && mods.intersection(core) == self.mods,
            (a, b) => a == b && mods.intersection(core) == self.mods,
        }
    }
}

/// The command trigger of S: 1 symbol, not a trigger of its own (@ agents, ! shell, # shortcuts,
/// / ~ paths).
fn command_char(s: &str) -> Option<char> {
    let mut c = s.trim().chars();
    let ch = c.next()?;
    (c.next().is_none() && !ch.is_alphanumeric() && !ch.is_whitespace() && !"@!#/~".contains(ch)).then_some(ch)
}

/// Keys codehead binds itself: a menu key on 1 of them never fires or hides a built-in.
const TAKEN: [&str; 30] = [
    "ctrl+a", "ctrl+b", "ctrl+c", "ctrl+d", "ctrl+e", "ctrl+f", "ctrl+g", "ctrl+h", "ctrl+i", "ctrl+k", "ctrl+m", "ctrl+n", "ctrl+o", "ctrl+q", "ctrl+r", "ctrl+s", "ctrl+t", "ctrl+u", "ctrl+v", "ctrl+w", "ctrl+x",
    "ctrl+y", "ctrl+z", "ctrl+.", "F1", "F2", "F3", "F4", "F5", "F6",
];

/// The menu key of S: a valid key codehead binds to nothing else (alt+b/d/f: word keys too).
fn menu_bind(s: &str) -> Option<Bind> {
    Bind::parse(s).filter(|b| !TAKEN.contains(&b.label.as_str()) && !matches!(b.label.as_str(), "alt+b" | "alt+d" | "alt+f"))
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
        ("device", &file.device),
        ("repo", &file.repo),
        ("styles", &file.styles),
        ("agent", &file.agent),
        ("prefix", &file.prefix),
        ("models", &file.models),
        ("messages", &file.messages),
        ("thoughts", &file.thoughts),
        ("edits", &file.edits),
        ("rank", &file.rank),
        ("classify", &file.classify),
        ("review", &file.review),
        ("editor", &file.editor),
        ("ports", &file.ports),
        ("previews", &file.previews),
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
    // [harness.<name>]: 1 sub-table per harness, gone when empty
    if file.harness.is_empty() {
        doc.remove("harness");
    } else {
        if !doc.contains_key("harness") {
            let mut t = toml_edit::Table::new();
            t.set_implicit(true);
            doc["harness"] = toml_edit::Item::Table(t);
        }
        let all = doc["harness"].as_table_mut().ok_or("harness: must be a table")?;
        let gone: Vec<String> = all.iter().filter(|(k, _)| !file.harness.contains_key(*k)).map(|(k, _)| k.to_string()).collect();
        for k in gone {
            all.remove(&k);
        }
        for (name, fields) in &file.harness {
            if !all.contains_key(name) {
                all.insert(name, toml_edit::Item::Table(toml_edit::Table::new()));
            }
            let t = all[name.as_str()].as_table_like_mut().ok_or_else(|| format!("harness.{name}: must be a table"))?;
            let gone: Vec<String> = t.iter().filter(|(k, _)| !fields.contains_key(*k)).map(|(k, _)| k.to_string()).collect();
            for k in gone {
                t.remove(&k);
            }
            for (k, v) in fields {
                if t.get(k).and_then(|x| x.as_str()) != Some(v) {
                    t.insert(k, toml_edit::Item::Value(toml_edit::Value::from(v.clone())));
                }
            }
        }
    }
    // 0 aliases: 0 table (a moved table leaves 0 empty [aliases] behind)
    if file.aliases.is_empty() {
        doc.remove("aliases");
    } else {
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
            k if k.starts_with("harness.") => {
                let (name, field) = k["harness.".len()..].split_once('.')?;
                self.harness.get(name)?.get(field).cloned()
            }
            k => k.strip_prefix("colors.").and_then(|n| self.colors.get(n).cloned()),
        }
    }

    pub fn set(&mut self, key: &str, v: Option<String>) {
        match key {
            "notes" => self.notes = v,
            "home" => self.home = v,
            "device" => self.device = v,
            "repo" => self.repo = v,
            "styles" => self.styles = v,
            "agent" => self.agent = v,
            "prefix" => self.prefix = v,
            "models" => self.models = v,
            "messages" => self.messages = v,
            "thoughts" => self.thoughts = v,
            "edits" => self.edits = v,
            "rank" => self.rank = v,
            "classify" => self.classify = v,
            "review" => self.review = v,
            "editor" => self.editor = v,
            "ports" => self.ports = v,
            "previews" => self.previews = v,
            "keys.menu" => self.menu_key = v,
            "keys.command" => self.command_key = v,
            "colors.scheme" => self.scheme = v,
            k if k.starts_with("harness.") => {
                let Some((name, field)) = k["harness.".len()..].split_once('.') else { return };
                match v {
                    Some(v) => {
                        self.harness.entry(name.to_string()).or_default().insert(field.to_string(), v);
                    }
                    None => {
                        if let Some(t) = self.harness.get_mut(name) {
                            t.remove(field);
                            if t.is_empty() {
                                self.harness.remove(name);
                            }
                        }
                    }
                }
            }
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

/// The menu key: $CODEHEAD_MENU_KEY, else `[keys] menu`, else ctrl+p.
pub fn menu_key() -> Bind {
    // tests: the defaults, never the machine's config
    let set = if cfg!(test) { None } else { env("CODEHEAD_MENU_KEY").or_else(|| load().ok()?.menu_key) };
    // a key codehead binds itself (F1-F6: the tabs, ctrl+n: notifications …): the default
    set.and_then(|k| menu_bind(&k)).unwrap_or_else(|| Bind::parse("ctrl+p").expect("ctrl+p parses"))
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
        ports: Some("auto".into()),
        previews: Some("auto".into()),
        menu_key: Some("ctrl+p".into()),
        command_key: Some(">".into()),
        scheme: Some(crate::theme::PRESETS[0].0.into()),
        ..File::default()
    }
}

/// $CODEHEAD_NOTES, else `notes`, else notes/ of the home repo (with its codehead dir), else
/// notes/ of the app dir.
pub fn notes() -> PathBuf {
    env("CODEHEAD_NOTES")
        .map(PathBuf::from)
        .or_else(|| get().notes.as_deref().map(expand))
        .or_else(|| layout().and_then(|d| Some(d.parent()?.join("notes"))))
        .unwrap_or_else(|| app_dir().join("notes"))
}

/// The home repo: $CODEHEAD_HOME_REPO, else `home`. None: unset or 0 dir.
pub fn home_repo() -> Option<PathBuf> {
    env("CODEHEAD_HOME_REPO")
        .map(PathBuf::from)
        .or_else(|| get().home.as_deref().filter(|s| !s.is_empty()).map(expand))
        .filter(|p| p.is_dir())
}

/// The new tab's default repo: $CODEHEAD_REPO, else `repo`. None: unset or 0 dir.
pub fn default_repo() -> Option<PathBuf> {
    env("CODEHEAD_REPO")
        .map(PathBuf::from)
        .or_else(|| get().repo.as_deref().filter(|s| !s.is_empty()).map(expand))
        .filter(|p| p.is_dir())
}

/// $CODEHEAD_STYLES, else `styles`, else styles/ of home's codehead dir (when there), else
/// output-styles/ of the app dir.
pub fn styles() -> PathBuf {
    env("CODEHEAD_STYLES")
        .map(PathBuf::from)
        .or_else(|| get().styles.as_deref().map(expand))
        .or_else(|| layout().map(|d| d.join("styles")).filter(|d| d.is_dir()))
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

/// Image + pdf previews in the files view: $CODEHEAD_PREVIEWS, else `previews`, else `auto`:
/// `auto` (the protocol from the terminal's env), `kitty`, `iterm2`, `sixel`, or `off` (text
/// previews only).
pub fn previews() -> String {
    env("CODEHEAD_PREVIEWS").or_else(|| get().previews.clone()).map(|v| v.trim().to_lowercase()).filter(|v| !v.is_empty()).unwrap_or_else(|| "auto".into())
}

/// Dev server ports: $CODEHEAD_PORTS, else `ports`, else auto.
pub fn ports() -> crate::portless::Mode {
    env("CODEHEAD_PORTS").or_else(|| get().ports.clone()).and_then(|m| crate::portless::Mode::parse(&m)).unwrap_or_default()
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

/// The changes view's classifier agent: $CODEHEAD_CLASSIFY, else `classify`. None: unset (the
/// default agent classifies), `off`: path rules.
pub fn classify() -> Option<String> {
    env("CODEHEAD_CLASSIFY").or_else(|| get().classify.clone()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
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
    fn previews_takes_auto_a_protocol_or_off() {
        for ok in ["auto", "kitty", "iterm2", "sixel", "off"] {
            assert!(validate("previews".into(), ok).is_ok(), "{ok}");
        }

        assert!(validate("previews".into(), "on").unwrap_err().contains("auto, kitty, iterm2, sixel or off"));
        assert_eq!(parse("previews = \"off\"\n").unwrap().previews.as_deref(), Some("off"));
        assert!(parse("previews = \"yes\"\n").is_err());
    }

    #[test]
    fn keys_and_colors_tables_round_trip() {
        let f = parse("[keys]\nmenu = \"ctrl+p\"\ncommand = \"%\"\n[colors]\nscheme = \"amber\"\naccent = \"#112233\"\n").unwrap();
        assert_eq!((f.menu_key.as_deref(), f.command_key.as_deref(), f.scheme.as_deref()), (Some("ctrl+p"), Some("%"), Some("amber")));
        assert_eq!(f.colors["accent"], "#112233");
        let p = palette_of(&f);
        assert_eq!(p.accent, ratatui::style::Color::Rgb(0x11, 0x22, 0x33));
        assert_eq!(p.text, crate::theme::AMBER.text);
        // written back: same tables; unset keys go
        let mut g = f.clone();
        g.set("colors.accent", None);
        g.set("keys.command", None);
        let text = document("[keys]\nmenu = \"ctrl+p\"\ncommand = \"%\"\n[colors]\nscheme = \"amber\"\naccent = \"#112233\"\n", &g).unwrap();
        let back = parse(&text).unwrap();
        assert_eq!(back, g);
        for bad in ["[keys]\nmenu = \"x\"", "[keys]\nmenu = \"ctrl+n\"", "[keys]\nmenu = \"F2\"", "[keys]\ncommand = \"#\"", "[keys]\ncommand = \"ab\"", "[colors]\nscheme = \"pink\"", "[colors]\naccent = \"red\"", "[colors]\nfoo = \"#000000\""] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        let b = Bind::parse("ctrl+p").unwrap();
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        assert!(b.matches(&KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL)));
        assert!(!b.matches(&KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)));
        // cmd+p (super) = ctrl+p
        assert!(b.matches(&KeyEvent::new(KeyCode::Char('p'), KeyModifiers::SUPER)));
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
    fn device_names_are_slugs() {
        assert_eq!(slug("Hanss-MacBook-Pro.local"), "hanss-macbook-pro");
        assert_eq!(slug(" My Mini_2 "), "my-mini-2");
        assert_eq!(slug("..."), "device");
        assert!(!host_device().is_empty());
    }

    #[test]
    fn scopes_route_paths_to_the_device_and_the_rest_to_shared() {
        assert_eq!(scope("home"), Scope::Local);
        assert_eq!(scope("editor"), Scope::Device);
        assert_eq!(scope("harness.codex.command"), Scope::Device);
        assert_eq!(scope("harness.claude.model"), Scope::Shared);
        assert_eq!(scope("colors.accent"), Scope::Shared);
        let mut f = parse("agent = 'REGI'\neditor = 'zed'\nhome = '/h'\n[harness.codex]\ncommand = 'x'\nmodel = 'm'\n[aliases]\na = '/tmp'\n").unwrap();
        f.scheme = Some("amber".into());
        let [l, s, d] = split(&f);
        assert_eq!((l.home.as_deref(), l.agent.as_deref()), (Some("/h"), None));
        assert_eq!((s.agent.as_deref(), s.scheme.as_deref(), s.get("harness.codex.model").as_deref()), (Some("REGI"), Some("amber"), Some("m")));
        assert_eq!((d.editor.as_deref(), d.get("harness.codex.command").as_deref(), d.aliases.len()), (Some("zed"), Some("x"), 1));
        // split, then merged: the same values
        let mut back = l.clone();
        overlay(&mut back, &s);
        overlay(&mut back, &d);
        assert_eq!(back, f);
    }

    #[test]
    fn layers_merge_device_over_shared_and_save_each_key_to_its_file() {
        let root = std::env::temp_dir().join(format!("codehead-layers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        std::fs::create_dir_all(home.join("codehead/devices")).unwrap();
        let local = root.join("codehead.toml");
        std::fs::write(&local, format!("home = '{}'\ndevice = 'mbp'\nprefix = 'old'\n", home.display())).unwrap();
        std::fs::write(home.join("codehead/settings.toml"), "prefix = 'task'\neditor = 'code'\n").unwrap();
        std::fs::write(home.join("codehead/devices/mbp.toml"), "# this mac\neditor = 'zed'\n").unwrap();
        let layers = read_layers(&local).unwrap();
        let f = layers.merged();
        assert_eq!((f.prefix.as_deref(), f.editor.as_deref(), f.device.as_deref()), (Some("task"), Some("zed"), Some("mbp")));

        let mut g = f.clone();
        g.editor = Some("cursor".into());
        g.messages = Some("lines".into());
        let after = save_layers(&layers, &g).unwrap();
        assert_eq!(after.merged(), g);
        let dev = std::fs::read_to_string(home.join("codehead/devices/mbp.toml")).unwrap();
        assert!(dev.contains("# this mac") && dev.contains("cursor"), "{dev}");
        let shared = std::fs::read_to_string(home.join("codehead/settings.toml")).unwrap();
        assert!(shared.contains("lines") && !shared.contains("editor"), "{shared}");
        // codehead.toml: the pointer only (its old prefix: now in shared)
        assert_eq!(after.local.file.prefix, None);

        // a new device: only the pointer changes, the new device's file starts empty
        let mut h = after.merged();
        h.device = Some("mini".into());
        h.editor = Some("idea".into());
        let moved = save_layers(&after, &h).unwrap();
        assert_eq!(moved.local.file.device.as_deref(), Some("mini"));
        let m = moved.merged();
        // editor: a device key (the save took it out of shared), mini: 0 file yet
        assert_eq!((m.editor.as_deref(), m.messages.as_deref()), (None, Some("lines")));
        assert!(!home.join("codehead/devices/mini.toml").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn layers_without_the_codehead_dir_are_codehead_toml_alone() {
        let root = std::env::temp_dir().join(format!("codehead-layers-0-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let local = root.join("codehead.toml");
        std::fs::write(&local, format!("home = '{}'\nprefix = 'x'\n", root.display())).unwrap();
        let layers = read_layers(&local).unwrap();
        assert!(layers.shared.is_none() && layers.device.is_none());
        let mut f = layers.merged();
        f.editor = Some("zed".into());
        save_layers(&layers, &f).unwrap();
        assert!(std::fs::read_to_string(&local).unwrap().contains("zed"));
        let _ = std::fs::remove_dir_all(&root);
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
