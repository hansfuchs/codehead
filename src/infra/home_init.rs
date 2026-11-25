//! The home repo's codehead dir (`<home>/codehead`): what codehead keeps in the home repo, so
//! every device that clones home gets the same crew.
//!
//! - `codehead home`: what is where (home, device, the codehead dir and its counts).
//! - `codehead home init`: makes the codehead dir, moves this machine's agents, flows, steps,
//!   mcp.toml and shortcuts.toml into it (a symlink stays at the old place: an older codehead
//!   binary still finds them), splits codehead.toml into the shared settings and this device's
//!   file. codehead.toml keeps `home` and `device`. Runs again without harm.

use std::path::Path;

use crate::infra::config;

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
pub(crate) fn run(arg: Option<&str>) -> ! {
    let local = config::path();
    let file = config::read_file(&local).map_or_else(
        |error| crate::infra::terminal::fail(&format!("{}: {error}", local.display()), 2),
        |(_, file_value)| file_value,
    );

    let Some(home) = file
        .home
        .as_deref()
        .filter(|height| !height.trim().is_empty())
        .map(config::expand)
    else {
        crate::infra::terminal::fail(
            &format!("0 home repo: set home in {} (or the settings tab)", local.display()),
            2,
        )
    };

    match arg {
        None | Some("status") => {
            for line in status(&home, &config::device_of(&file)) {
                println!("{line}");
            }
        }
        Some("init") => match init(&home, &local, &crate::engine::config_dir()) {
            Ok(lines) => {
                for line_value in lines {
                    println!("{line_value}");
                }
            }
            Err(error_value) => crate::infra::terminal::fail(&error_value, 1),
        },
        Some(other) => crate::infra::terminal::fail(
            &format!("codehead home: unknown command {other}. commands: status, init"),
            2,
        ),
    }

    std::process::exit(0)
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;

    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Entries of DIR (files or dirs), by name.
fn entries(dir: &Path) -> Vec<String> {
    let mut value: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|error| error.file_name().to_string_lossy().into_owned())
        .filter(|count| !count.starts_with('.'))
        .collect();

    value.sort();
    value
}

/// Lines of `codehead home`.
pub(crate) fn status(home: &Path, device: &str) -> Vec<String> {
    let mut out = vec![format!("home: {}", home.display())];

    if home.join(".git").exists() {
        let branch = git(home, &["branch", "--show-current"]).unwrap_or_default();
        let remote = git(home, &["remote", "get-url", "origin"]).unwrap_or_else(|| "0 remote".into());
        let dirty = git(home, &["status", "--porcelain"]).map_or(0, |text| text.lines().count());

        out.push(format!("git: {branch} · {remote} · {dirty} uncommitted"));
    } else {
        out.push("git: not a git repo".into());
    }

    out.push(format!("device: {device}"));

    let Some(dir) = config::layout_of(home) else {
        out.push(format!(
            "codehead dir: missing. codehead home init makes it: {}",
            home.join("codehead").display()
        ));
        return out;
    };

    let names = |sub: &str| {
        entries(&dir.join(sub))
            .into_iter()
            .map(|count_value| count_value.trim_end_matches(".toml").to_string())
            .collect::<Vec<_>>()
    };

    out.push(format!("codehead dir: {}", dir.display()));
    out.push(format!("agents: {}", names("agents").join(", ")));
    out.push(format!(
        "flows: {} · steps: {}",
        names("flows").len(),
        names("steps").len()
    ));
    let servers = std::fs::read_to_string(dir.join("mcp.toml"))
        .ok()
        .and_then(|thread| thread.parse::<toml::Table>().ok())
        .map_or(0, |thread_value| thread_value.len());

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

    let ok = std::process::Command::new("cp")
        .arg("-R")
        .arg(src)
        .arg(dst)
        .status()
        .is_ok_and(|text| text.success());

    if !ok {
        return Err(format!("cp {} {}: failed", src.display(), dst.display()));
    }

    if src.is_dir() {
        std::fs::remove_dir_all(src)
    } else {
        std::fs::remove_file(src)
    }
    .map_err(|error| format!("{}: {error}", src.display()))
}

/// Move NAME of CONFIG into DIR; a symlink stays at the old place. Lines of what happened.
fn move_one(config: &Path, dir: &Path, name: &str) -> Result<Vec<String>, String> {
    let (src, dst) = (config.join(name), dir.join(name));

    let Ok(meta) = std::fs::symlink_metadata(&src) else {
        return Ok(Vec::new());
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
        for error in entries(&src) {
            out.push(move_entry(&src, &dst, &error)?);
        }

        if std::fs::read_dir(&src)
            .map_err(|error_value| error_value.to_string())?
            .next()
            .is_none()
        {
            std::fs::remove_dir(&src).map_err(|error_2| format!("{}: {error_2}", src.display()))?;
        }
    } else {
        out.push(format!("kept: {} ({} exists)", src.display(), dst.display()));
    }

    if !src.exists() {
        std::os::unix::fs::symlink(&dst, &src).map_err(|error_3| format!("symlink {}: {error_3}", src.display()))?;
    }

    Ok(out)
}

/// Make the codehead dir of HOME, move the files of CONFIG (the config dir) into it, split
/// codehead.toml LOCAL. Lines of what happened.
pub(crate) fn init(home: &Path, local: &Path, config: &Path) -> Result<Vec<String>, String> {
    if !home.join(".git").exists() {
        return Err(format!("{}: not a git repo (git init first)", home.display()));
    }

    let dir = home.join("codehead");
    let mut out = Vec::new();

    std::fs::create_dir_all(dir.join("devices")).map_err(|error| format!("{}: {error}", dir.display()))?;
    let readme = dir.join("README.md");

    if !readme.exists() {
        crate::engine::write_atomic(&readme, README.as_bytes())
            .map_err(|error_value| format!("{}: {error_value}", readme.display()))?;
        out.push(format!("written: {}", readme.display()));
    }

    let attrs = home.join(".gitattributes");
    let text = std::fs::read_to_string(&attrs).unwrap_or_default();

    if !text.lines().any(|line| line.trim() == UNION) {
        let sep = if text.is_empty() || text.ends_with('\n') {
            ""
        } else {
            "\n"
        };

        crate::engine::write_atomic(&attrs, format!("{text}{sep}{UNION}\n").as_bytes())
            .map_err(|error_2| format!("{}: {error_2}", attrs.display()))?;
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
    let mut file_value = layers.merged();
    let device = config::device_of(&file_value);

    // 1st the pointer: then the layers include home's files
    let same_home = file_value
        .home
        .as_deref()
        .map(config::expand)
        .is_some_and(|height| height == home);

    if !same_home || file_value.device.as_deref() != Some(device.as_str()) {
        let mut pointer = layers.local.file.clone();

        pointer.home = Some(
            file_value
                .home
                .clone()
                .filter(|_| same_home)
                .unwrap_or_else(|| home.display().to_string()),
        );
        pointer.device = Some(device.clone());
        config::save_file(&layers.local.path, &layers.local.text, &pointer)?;
    }

    let layers = config::read_layers(local)?;

    if layers.shared.is_none() {
        return Err(format!("{}: home is not {}", local.display(), home.display()));
    }

    file_value.home.clone_from(&layers.local.file.home);
    file_value.device.clone_from(&layers.local.file.device);

    if file_value
        .notes
        .as_deref()
        .map(config::expand)
        .is_some_and(|count| count == home.join("notes"))
    {
        file_value.notes = None;
    }

    let before = layers.merged();
    let after = config::save_layers(&layers, &file_value)?;
    let mut out = vec![format!("device: {device}")];

    if after.merged() != before || layers.local.text != after.local.text {
        let path = |line: &Option<config::Layer>| {
            line.as_ref()
                .map(|line_value| line_value.path.display().to_string())
                .unwrap_or_default()
        };

        out.push(format!(
            "settings: {} + {}; {} keeps home + device",
            path(&after.shared),
            path(&after.device),
            local.display()
        ));
    }

    Ok(out)
}

#[cfg(test)]
#[path = "home_init_tests.rs"]
mod tests;

fn move_entry(src: &Path, dst: &Path, name: &str) -> Result<String, String> {
    let text = src.join(name);
    let directory = dst.join(name);

    if directory.exists() {
        return Ok(format!("kept: {} ({} exists)", text.display(), directory.display()));
    }

    move_path(&text, &directory)?;
    Ok(format!("moved: {} → {}", text.display(), directory.display()))
}
