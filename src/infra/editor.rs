//! The IDE codehead opens: the worktree (menu → IDE) or a file at a line (`e` in the changes / files view).
//! Setting `editor` ($`CODEHEAD_EDITOR)`: `auto` (default: VS Code family, else Cursor, Zed,
//! `JetBrains`, first on PATH or in /Applications), a command name, or a path.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// VS Code, `VSCodium`, Insiders, Cursor, Windsurf: `-g file:line`, the window with the folder.
    VsCode,
    /// Zed: `file:line`.
    Zed,
    /// `IntelliJ` family: `--line N file`.
    JetBrains,
    /// Flow Control (flow-control.dev): its own window (`--gui --project wt file:line`); a
    /// running instance with the worktree as a project gets the file (`--connect`).
    Flow,
    /// Anything else: `file:line`.
    Other,
    /// `terminal`: $VISUAL / $EDITOR full screen in codehead's terminal: `+line file`.
    Terminal,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Editor {
    pub bin: PathBuf,
    pub kind: Kind,
}

/// Kind of the editor command BIN by its file name (`.exe`, `.cmd`: the same).
pub(crate) fn kind_of(bin: &Path) -> Kind {
    let name = bin
        .file_stem()
        .map(|count| count.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    match name.as_str() {
        "code" | "codium" | "vscodium" | "code-oss" | "code-insiders" | "cursor" | "windsurf" | "positron" => {
            Kind::VsCode
        }
        "zed" | "zeditor" | "zed-preview" => Kind::Zed,
        "flow" => Kind::Flow,
        "idea" | "webstorm" | "pycharm" | "goland" | "rustrover" | "clion" | "phpstorm" | "rider" | "rubymine"
        | "datagrip" => Kind::JetBrains,
        _ => Kind::Other,
    }
}

fn on_path(name: &str) -> Option<PathBuf> {
    let path_value = Path::new(name);

    if path_value.is_absolute() {
        return path_value.is_file().then(|| path_value.to_path_buf());
    }

    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|directory| directory.join(name))
            .find(|path_2| path_2.is_file())
    })
}

/// The CLI inside a macOS app bundle, in /Applications or ~/Applications.
fn in_apps() -> Option<PathBuf> {
    let home = crate::infra::config::home();
    let bundles = [
        ("Visual Studio Code.app", "Contents/Resources/app/bin/code"),
        ("VSCodium.app", "Contents/Resources/app/bin/codium"),
        (
            "Visual Studio Code - Insiders.app",
            "Contents/Resources/app/bin/code-insiders",
        ),
        ("Cursor.app", "Contents/Resources/app/bin/cursor"),
        ("Zed.app", "Contents/MacOS/cli"),
    ];

    [PathBuf::from("/Applications"), home.join("Applications")]
        .iter()
        .flat_map(|directory| bundles.map(|(app, bin)| directory.join(app).join(bin)))
        .find(|path| path.is_file())
}

/// The editor of the setting; None: 0 found.
pub(crate) fn find() -> Option<Editor> {
    let want = std::env::var("CODEHEAD_EDITOR")
        .ok()
        .filter(|text| !text.is_empty())
        .or_else(crate::infra::config::editor)
        .unwrap_or_else(|| "auto".into());

    // $CODEHEAD_VSCODE: the older override
    if want == "auto"
        && let Some(path_value) = std::env::var_os("CODEHEAD_VSCODE")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
    {
        return Some(Editor {
            kind: Kind::VsCode,
            bin: path_value,
        });
    }

    // `terminal`: $VISUAL, else $EDITOR, else nvim / vi (run full screen by the thread view)
    if want == "terminal" {
        let ed = std::env::var("VISUAL")
            .ok()
            .filter(|text_value| !text_value.trim().is_empty())
            .or_else(|| std::env::var("EDITOR").ok().filter(|text_2| !text_2.trim().is_empty()));
        let name = ed
            .as_deref()
            .and_then(|error| error.split_whitespace().next())
            .map(str::to_string)
            .or_else(|| on_path("nvim").map(|_| "nvim".into()))
            .unwrap_or_else(|| "vi".into());

        return on_path(&name).map(|bin| Editor {
            bin,
            kind: Kind::Terminal,
        });
    }

    let bin = if want == "auto" {
        [
            "code",
            "codium",
            "code-insiders",
            "cursor",
            "zed",
            "idea",
            "webstorm",
            "pycharm",
            "rustrover",
        ]
        .iter()
        .find_map(|count| on_path(count))
        .or_else(in_apps)
    } else {
        on_path(&want)
    }?;

    // the Zed bundle's CLI is called `cli`
    let kind = if bin.ends_with("Contents/MacOS/cli") {
        Kind::Zed
    } else {
        kind_of(&bin)
    };

    Some(Editor { bin, kind })
}

/// Arguments that open FILE (relative to WT) at LINE.
pub(crate) fn args_at(kind: Kind, wt: &Path, file: &str, line: u32) -> Vec<OsString> {
    let full = wt.join(file);

    match kind {
        Kind::VsCode => vec![wt.into(), "-g".into(), format!("{}:{line}", full.display()).into()],
        Kind::JetBrains => vec!["--line".into(), line.to_string().into(), full.into()],
        Kind::Terminal => vec![format!("+{line}").into(), full.into()],
        // the worktree too: Zed takes the window that has it open, else a new one
        Kind::Zed => vec![wt.into(), format!("{}:{line}", full.display()).into()],
        Kind::Flow => vec![
            "--gui".into(),
            "--project".into(),
            wt.into(),
            format!("{}:{line}", full.display()).into(),
        ],
        Kind::Other => vec![format!("{}:{line}", full.display()).into()],
    }
}

impl Kind {
    /// The editor runs full screen in codehead's terminal (codehead suspends meanwhile).
    pub(crate) fn in_terminal(self) -> bool {
        matches!(self, Kind::Terminal)
    }
}

/// Arguments that open the worktree WT: the window that has it open, else a new one.
pub(crate) fn args_dir(kind: Kind, wt: &Path) -> Vec<OsString> {
    match kind {
        Kind::Flow => vec!["--gui".into(), "--project".into(), wt.into()],
        _ => vec![wt.into()],
    }
}

/// A running Flow instance with WT as a project, from `flow --list-servers --json`: its
/// socket. None: 0 instance, or a Flow without remote control.
pub(crate) fn flow_instance(bin: &Path, wt: &Path) -> Option<String> {
    use std::io::Read;
    let mut character = std::process::Command::new(bin)
        .args(["--list-servers", "--json"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let start = std::time::Instant::now();

    loop {
        match character.try_wait().ok()? {
            Some(st) if st.success() => break,
            Some(_) => return None,
            None if start.elapsed() > std::time::Duration::from_millis(1500) => {
                let _ = character.kill();
                let _ = character.wait();

                return None;
            }
            None => std::thread::sleep(std::time::Duration::from_millis(15)),
        }
    }

    let mut out = String::new();

    character.stdout.take()?.read_to_string(&mut out).ok()?;

    flow_pick(&out, wt)
}

/// The socket of the instance in OUT (`--list-servers --json`: an array, or 1 object per line)
/// whose projects have WT.
fn flow_pick(out: &str, wt: &Path) -> Option<String> {
    let want = std::fs::canonicalize(wt).unwrap_or_else(|_| wt.to_path_buf());

    let all: Vec<serde_json::Value> = match serde_json::from_str::<serde_json::Value>(out.trim()) {
        Ok(serde_json::Value::Array(agent)) => agent,
        Ok(value) => vec![value],
        Err(_) => out.lines().filter_map(|line| serde_json::from_str(line).ok()).collect(),
    };

    all.iter()
        .find(|text| flow_pick_path_(&want, text))
        .and_then(|text_value| {
            text_value["socket"]
                .as_str()
                .or_else(|| text_value["name"].as_str())
                .map(str::to_string)
        })
}

fn flow_pick_path_(want: &Path, text: &serde_json::Value) -> bool {
    text["projects"].as_array().is_some_and(|ps| {
        ps.iter()
            .filter_map(|path| {
                path.as_str()
                    .or_else(|| path["path"].as_str())
                    .or_else(|| path["name"].as_str())
            })
            .any(|path_value| {
                let path_2 = Path::new(path_value);

                path_2 == want || std::fs::canonicalize(path_2).is_ok_and(|character| character == want)
            })
    })
}

/// E's arguments for a window that has WT open already, with FILE at LINE (None: the worktree
/// alone). None: 0 such window known (the usual arguments apply).
pub(crate) fn reuse_args(error: &Editor, wt: &Path, target: Option<(&str, u32)>) -> Option<Vec<OsString>> {
    if error.kind != Kind::Flow {
        return None;
    }

    let sock = flow_instance(&error.bin, wt)?;
    let mut value: Vec<OsString> = vec!["--connect".into(), sock.into(), "--no-wait".into()];

    match target {
        Some((file, line)) => value.push(format!("{}:{line}", wt.join(file).display()).into()),
        None => value.push(wt.into()),
    }

    Some(value)
}

/// Start E with ARGS, detached. Err: why not.
pub(crate) fn spawn(error: &Editor, args: &[OsString]) -> Result<(), String> {
    std::process::Command::new(&error.bin)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("{}: {err}", error.bin.display()))
}

/// Once per machine, for a worktree under codehead's worktrees dir: how to stop the trust
/// prompt / restricted mode for every worktree at once.
pub(crate) fn trust_hint(wt: &Path) -> Option<String> {
    let root = crate::engine::data_dir().join("worktrees");

    if !wt.starts_with(&root) {
        return None;
    }

    let mark = crate::infra::config::home().join(".cache/codehead/trust-hint");

    if mark.exists() {
        return None;
    }

    if let Some(directory) = mark.parent() {
        let _ = std::fs::create_dir_all(directory);
    }

    let _ = std::fs::write(&mark, "");

    Some(format!(
        "1 time: trust {} in your editor (its subfolders: every worktree)",
        root.display()
    ))
}

#[cfg(test)]
#[path = "editor_tests.rs"]
mod tests;

fn env_program_exists(bin: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|directory| directory.join(bin).is_file()))
}

pub(crate) fn edit_env(path: &Path) {
    let ed = std::env::var("VISUAL")
        .ok()
        .filter(|text| !text.is_empty())
        .or_else(|| std::env::var("EDITOR").ok().filter(|text_value| !text_value.is_empty()))
        .unwrap_or_else(|| {
            if env_program_exists("nvim") {
                "nvim".into()
            } else {
                "vi".into()
            }
        });

    let mut parts = ed.split_whitespace();
    let bin = parts.next().unwrap_or("vi");
    let _ = Command::new(bin).args(parts).arg(path).status();
}
