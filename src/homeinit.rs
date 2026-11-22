//! The home repo's codehead dir (`<home>/codehead`): what codehead keeps in the home repo, so
//! every device that clones home gets the same crew.
//!
//! - `codehead home`: what is where (home, device, the codehead dir and its counts).
//! - `codehead home init`: makes the codehead dir, moves this machine's agents, flows, steps,
//!   mcp.toml and shortcuts.toml into it (a symlink stays at the old place: an older codehead
//!   binary still finds them), splits codehead.toml into the shared settings and this device's
//!   file. codehead.toml keeps `home` and `device`. Runs again without harm.

use std::path::Path;

use crate::config;

/// What `init` moves from the config dir into the codehead dir.
const MOVED: [&str; 5] = ["agents", "flows", "steps", "mcp.toml", "shortcuts.toml"];

const README: &str = "# codehead

codehead keeps these files here: every device that clones this repo gets them.

- `agents/<name>.toml`: the crew (harness, model, avatar, instructions)
- `flows/`, `steps/`: own flows and flow steps
- `mcp.toml`: MCP servers; secrets live in each device's keychain (`keychain:` refs), never here
- `shortcuts.toml`: own key bindings
- `settings.toml`: settings of every device (colors, keys, models, …)
- `devices/<device>.toml`: settings of 1 device (paths, editor, ports, path aliases)

Threads, logs and caches stay on each device. Notes: `../notes/`.
";

/// The `.gitattributes` line: index files merge by keeping the lines of both sides.
const UNION: &str = "INDEX.md merge=union";

/// `codehead home [init]`.
pub fn run(arg: Option<&str>) -> ! {
    let local = config::path();
    let file = config::read_file(&local).map(|(_, f)| f).unwrap_or_else(|e| crate::fail(&format!("{}: {e}", local.display()), 2));
    let Some(home) = file.home.as_deref().filter(|h| !h.trim().is_empty()).map(config::expand) else {
        crate::fail(&format!("0 home repo: set home in {} (or the settings tab)", local.display()), 2)
    };
    match arg {
        None | Some("status") => {
            for l in status(&home, &config::device_of(&file)) {
                println!("{l}");
            }
        }
        Some("init") => match init(&home, &local, &crate::engine::config_dir()) {
            Ok(lines) => {
                for l in lines {
                    println!("{l}");
                }
            }
            Err(e) => crate::fail(&e, 1),
        },
        Some(other) => crate::fail(&format!("codehead home: unknown command {other}. commands: status, init"), 2),
    }
    std::process::exit(0)
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git").arg("-C").arg(dir).args(args).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Entries of DIR (files or dirs), by name.
fn entries(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| !n.starts_with('.')).collect();
    v.sort();
    v
}

/// Lines of `codehead home`.
pub fn status(home: &Path, device: &str) -> Vec<String> {
    let mut out = vec![format!("home: {}", home.display())];
    if home.join(".git").exists() {
        let branch = git(home, &["branch", "--show-current"]).unwrap_or_default();
        let remote = git(home, &["remote", "get-url", "origin"]).unwrap_or_else(|| "0 remote".into());
        let dirty = git(home, &["status", "--porcelain"]).map(|s| s.lines().count()).unwrap_or(0);
        out.push(format!("git: {branch} · {remote} · {dirty} uncommitted"));
    } else {
        out.push("git: not a git repo".into());
    }
    out.push(format!("device: {device}"));
    let Some(dir) = config::layout_of(home) else {
        out.push(format!("codehead dir: missing. codehead home init makes it: {}", home.join("codehead").display()));
        return out;
    };
    let names = |sub: &str| entries(&dir.join(sub)).into_iter().map(|n| n.trim_end_matches(".toml").to_string()).collect::<Vec<_>>();
    out.push(format!("codehead dir: {}", dir.display()));
    out.push(format!("agents: {}", names("agents").join(", ")));
    out.push(format!("flows: {} · steps: {}", names("flows").len(), names("steps").len()));
    let servers = std::fs::read_to_string(dir.join("mcp.toml")).ok().and_then(|t| t.parse::<toml::Table>().ok()).map_or(0, |t| t.len());
    out.push(format!("mcp servers: {servers}"));
    out.push(format!("devices: {}", names("devices").join(", ")));
    out.push(format!("notes: {}", home.join("notes").display()));
    out
}

/// Move SRC to DST: a rename, else (another file system) a copy and a delete.
fn move_path(src: &Path, dst: &Path) -> Result<(), String> {
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    let ok = std::process::Command::new("cp").arg("-R").arg(src).arg(dst).status().is_ok_and(|s| s.success());
    if !ok {
        return Err(format!("cp {} {}: failed", src.display(), dst.display()));
    }
    if src.is_dir() { std::fs::remove_dir_all(src) } else { std::fs::remove_file(src) }.map_err(|e| format!("{}: {e}", src.display()))
}

/// Move NAME of CONFIG into DIR; a symlink stays at the old place. Lines of what happened.
fn move_one(config: &Path, dir: &Path, name: &str) -> Result<Vec<String>, String> {
    let (src, dst) = (config.join(name), dir.join(name));
    let meta = match std::fs::symlink_metadata(&src) {
        Ok(m) => m,
        Err(_) => return Ok(Vec::new()),
    };
    if meta.file_type().is_symlink() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    if !dst.exists() {
        move_path(&src, &dst)?;
        out.push(format!("moved: {} → {}", src.display(), dst.display()));
    } else if src.is_dir() {
        // both: each entry the codehead dir lacks moves; the others stay
        for e in entries(&src) {
            let (s, d) = (src.join(&e), dst.join(&e));
            if d.exists() {
                out.push(format!("kept: {} ({} exists)", s.display(), d.display()));
            } else {
                move_path(&s, &d)?;
                out.push(format!("moved: {} → {}", s.display(), d.display()));
            }
        }
        if std::fs::read_dir(&src).map_err(|e| e.to_string())?.next().is_none() {
            std::fs::remove_dir(&src).map_err(|e| format!("{}: {e}", src.display()))?;
        }
    } else {
        out.push(format!("kept: {} ({} exists)", src.display(), dst.display()));
    }
    if !src.exists() {
        std::os::unix::fs::symlink(&dst, &src).map_err(|e| format!("symlink {}: {e}", src.display()))?;
    }
    Ok(out)
}

/// Make the codehead dir of HOME, move the files of CONFIG (the config dir) into it, split
/// codehead.toml LOCAL. Lines of what happened.
pub fn init(home: &Path, local: &Path, config: &Path) -> Result<Vec<String>, String> {
    if !home.join(".git").exists() {
        return Err(format!("{}: not a git repo (git init first)", home.display()));
    }
    let dir = home.join("codehead");
    let mut out = Vec::new();
    std::fs::create_dir_all(dir.join("devices")).map_err(|e| format!("{}: {e}", dir.display()))?;
    let readme = dir.join("README.md");
    if !readme.exists() {
        crate::engine::write_atomic(&readme, README.as_bytes()).map_err(|e| format!("{}: {e}", readme.display()))?;
        out.push(format!("written: {}", readme.display()));
    }
    let attrs = home.join(".gitattributes");
    let text = std::fs::read_to_string(&attrs).unwrap_or_default();
    if !text.lines().any(|l| l.trim() == UNION) {
        let sep = if text.is_empty() || text.ends_with('\n') { "" } else { "\n" };
        crate::engine::write_atomic(&attrs, format!("{text}{sep}{UNION}\n").as_bytes()).map_err(|e| format!("{}: {e}", attrs.display()))?;
        out.push(format!("written: {} ({UNION})", attrs.display()));
    }
    for name in MOVED {
        out.extend(move_one(config, &dir, name)?);
    }
    out.extend(split_settings(home, local)?);
    Ok(out)
}

/// codehead.toml LOCAL → home + device in it, the rest into the shared settings and the device
/// file. A `notes` setting that names home's notes/ goes (the default).
fn split_settings(home: &Path, local: &Path) -> Result<Vec<String>, String> {
    let layers = config::read_layers(local)?;
    let mut f = layers.merged();
    let device = config::device_of(&f);
    // 1st the pointer: then the layers include home's files
    let same_home = f.home.as_deref().map(config::expand).is_some_and(|h| h == home);
    if !same_home || f.device.as_deref() != Some(device.as_str()) {
        let mut pointer = layers.local.file.clone();
        pointer.home = Some(f.home.clone().filter(|_| same_home).unwrap_or_else(|| home.display().to_string()));
        pointer.device = Some(device.clone());
        config::save_file(&layers.local.path, &layers.local.text, &pointer)?;
    }
    let layers = config::read_layers(local)?;
    if layers.shared.is_none() {
        return Err(format!("{}: home is not {}", local.display(), home.display()));
    }
    f.home = layers.local.file.home.clone();
    f.device = layers.local.file.device.clone();
    if f.notes.as_deref().map(config::expand).is_some_and(|n| n == home.join("notes")) {
        f.notes = None;
    }
    let before = layers.merged();
    let after = config::save_layers(&layers, &f)?;
    let mut out = vec![format!("device: {device}")];
    if after.merged() != before || layers.local.text != after.local.text {
        let path = |l: &Option<config::Layer>| l.as_ref().map(|l| l.path.display().to_string()).unwrap_or_default();
        out.push(format!("settings: {} + {}; {} keeps home + device", path(&after.shared), path(&after.device), local.display()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("codehead-homeinit-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn init_moves_files_links_back_and_splits_settings() {
        let root = tmp("init");
        let (home, config) = (root.join("home"), root.join("config"));
        std::fs::create_dir_all(home.join(".git")).unwrap();
        std::fs::create_dir_all(config.join("agents")).unwrap();
        std::fs::write(config.join("agents/REGI.toml"), "harness = \"claude\"\n").unwrap();
        std::fs::write(config.join("mcp.toml"), "[linear]\nurl = \"https://x\"\n").unwrap();
        let local = config.join("codehead.toml");
        let text = format!(
            "# mine\nagent = \"REGI\"\nnotes = \"{0}/notes/\"\nhome = \"{0}\"\neditor = \"zed\"\n\n[colors]\nscheme = \"amber\"\n\n[aliases]\n\"x:y\" = \"/tmp/y\"\n",
            home.display()
        );
        std::fs::write(&local, &text).unwrap();
        let before = config::read_layers(&local).unwrap().merged();

        let lines = init(&home, &local, &config).unwrap();
        let dir = home.join("codehead");
        assert!(dir.join("agents/REGI.toml").is_file(), "{lines:?}");
        assert!(dir.join("mcp.toml").is_file());
        assert!(std::fs::symlink_metadata(config.join("agents")).unwrap().file_type().is_symlink());
        assert!(config.join("agents/REGI.toml").is_file(), "the old path still reads");
        assert!(dir.join("README.md").is_file());
        assert!(std::fs::read_to_string(home.join(".gitattributes")).unwrap().contains(UNION));

        let layers = config::read_layers(&local).unwrap();
        let (shared, device) = (layers.shared.clone().unwrap(), layers.device.clone().unwrap());
        assert_eq!(shared.file.agent.as_deref(), Some("REGI"));
        assert_eq!(shared.file.scheme.as_deref(), Some("amber"));
        assert_eq!(device.file.editor.as_deref(), Some("zed"));
        assert_eq!(device.file.aliases.get("x:y").map(String::as_str), Some("/tmp/y"));
        assert!(device.path.starts_with(dir.join("devices")));
        // codehead.toml: the pointer only
        let l = &layers.local.file;
        assert_eq!((l.agent.as_ref(), l.editor.as_ref(), l.notes.as_ref()), (None, None, None));
        assert!(l.home.is_some() && l.device.is_some());
        // the same values, notes = the default now
        let mut after = layers.merged();
        after.notes = before.notes.clone();
        after.device = None;
        assert_eq!(after, before);

        // again: 0 change
        let again = init(&home, &local, &config).unwrap();
        assert!(again.iter().all(|l| !l.starts_with("moved") && !l.starts_with("settings")), "{again:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn init_keeps_what_home_has_and_needs_git() {
        let root = tmp("keep");
        let (home, config) = (root.join("home"), root.join("config"));
        assert!(init(&home, &config.join("codehead.toml"), &config).unwrap_err().contains("not a git repo"));
        std::fs::create_dir_all(home.join(".git")).unwrap();
        std::fs::create_dir_all(home.join("codehead/agents")).unwrap();
        std::fs::write(home.join("codehead/agents/REGI.toml"), "harness = \"codex\"\n").unwrap();
        std::fs::create_dir_all(config.join("agents")).unwrap();
        std::fs::write(config.join("agents/REGI.toml"), "harness = \"claude\"\n").unwrap();
        std::fs::write(config.join("agents/ADA.toml"), "harness = \"claude\"\n").unwrap();
        std::fs::write(config.join("codehead.toml"), format!("home = \"{}\"\n", home.display())).unwrap();
        let lines = init(&home, &config.join("codehead.toml"), &config).unwrap();
        assert!(lines.iter().any(|l| l.starts_with("kept") && l.contains("REGI")), "{lines:?}");
        assert!(home.join("codehead/agents/ADA.toml").is_file());
        assert_eq!(std::fs::read_to_string(home.join("codehead/agents/REGI.toml")).unwrap(), "harness = \"codex\"\n");
        // 1 file left: the old dir stays, 0 symlink
        assert!(config.join("agents/REGI.toml").is_file() && !config.join("agents/ADA.toml").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn status_names_the_missing_dir() {
        let root = tmp("status");
        let s = status(&root, "mbp");
        assert!(s.iter().any(|l| l.starts_with("codehead dir: missing")), "{s:?}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
