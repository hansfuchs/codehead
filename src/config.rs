//! Device config: ~/.config/codehead.toml ($CODEHEAD_CONFIG overrides the path).
//! Per key: env var first, then the file, then the built-in default.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::RwLock;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct File {
    pub notes: Option<String>,
    pub styles: Option<String>,
    pub agent: Option<String>,
    pub prefix: Option<String>,
    pub models: Option<String>,
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
            "styles" => f.styles = Some(s),
            "agent" => f.agent = Some(s),
            "prefix" => f.prefix = Some(s),
            "models" => f.models = Some(s),
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
    CFG.get_or_init(|| RwLock::new(f))
        .read()
        .map(|c| c.clone())
        .map_err(|e| e.to_string())
}

fn get() -> File {
    load().unwrap_or_else(|e| crate::fail(&e, 2))
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
        ("styles", &file.styles),
        ("agent", &file.agent),
        ("prefix", &file.prefix),
        ("models", &file.models),
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

pub fn apply(file: File) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
