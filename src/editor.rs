//! The IDE codehead opens: the worktree (menu → IDE) or a file at a line (`e` in the changes / files view).
//! Setting `editor` ($CODEHEAD_EDITOR): `auto` (default: VS Code family, else Cursor, Zed,
//! JetBrains, first on PATH or in /Applications), a command name, or a path.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// VS Code, VSCodium, Insiders, Cursor, Windsurf: `-g file:line`, the window with the folder.
    VsCode,
    /// Zed: `file:line`.
    Zed,
    /// IntelliJ family: `--line N file`.
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
pub struct Editor {
    pub bin: PathBuf,
    pub kind: Kind,
}

/// Kind of the editor command BIN by its file name (`.exe`, `.cmd`: the same).
pub fn kind_of(bin: &Path) -> Kind {
    let name = bin.file_stem().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    match name.as_str() {
        "code" | "codium" | "vscodium" | "code-oss" | "code-insiders" | "cursor" | "windsurf" | "positron" => Kind::VsCode,
        "zed" | "zeditor" | "zed-preview" => Kind::Zed,
        "flow" => Kind::Flow,
        "idea" | "webstorm" | "pycharm" | "goland" | "rustrover" | "clion" | "phpstorm" | "rider" | "rubymine" | "datagrip" => Kind::JetBrains,
        _ => Kind::Other,
    }
}

fn on_path(name: &str) -> Option<PathBuf> {
    let p = Path::new(name);
    if p.is_absolute() {
        return p.is_file().then(|| p.to_path_buf());
    }
    std::env::var_os("PATH").and_then(|path| std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file()))
}

/// The CLI inside a macOS app bundle, in /Applications or ~/Applications.
fn in_apps() -> Option<PathBuf> {
    let home = crate::config::home();
    let bundles = [
        ("Visual Studio Code.app", "Contents/Resources/app/bin/code"),
        ("VSCodium.app", "Contents/Resources/app/bin/codium"),
        ("Visual Studio Code - Insiders.app", "Contents/Resources/app/bin/code-insiders"),
        ("Cursor.app", "Contents/Resources/app/bin/cursor"),
        ("Zed.app", "Contents/MacOS/cli"),
    ];
    [PathBuf::from("/Applications"), home.join("Applications")].iter().flat_map(|d| bundles.map(|(app, bin)| d.join(app).join(bin))).find(|p| p.is_file())
}

/// The editor of the setting; None: 0 found.
pub fn find() -> Option<Editor> {
    let want = std::env::var("CODEHEAD_EDITOR").ok().filter(|s| !s.is_empty()).or_else(crate::config::editor).unwrap_or_else(|| "auto".into());
    // $CODEHEAD_VSCODE: the older override
    if want == "auto"
        && let Some(p) = std::env::var_os("CODEHEAD_VSCODE").map(PathBuf::from).filter(|p| p.is_file())
    {
        return Some(Editor { kind: Kind::VsCode, bin: p });
    }
    // `terminal`: $VISUAL, else $EDITOR, else nvim / vi (run full screen by the thread view)
    if want == "terminal" {
        let ed = std::env::var("VISUAL").ok().filter(|s| !s.trim().is_empty()).or_else(|| std::env::var("EDITOR").ok().filter(|s| !s.trim().is_empty()));
        let name = ed.as_deref().and_then(|e| e.split_whitespace().next()).map(str::to_string).or_else(|| on_path("nvim").map(|_| "nvim".into())).unwrap_or_else(|| "vi".into());
        return on_path(&name).map(|bin| Editor { bin, kind: Kind::Terminal });
    }
    let bin = if want == "auto" {
        ["code", "codium", "code-insiders", "cursor", "zed", "idea", "webstorm", "pycharm", "rustrover"].iter().find_map(|n| on_path(n)).or_else(in_apps)
    } else {
        on_path(&want)
    }?;
    // the Zed bundle's CLI is called `cli`
    let kind = if bin.ends_with("Contents/MacOS/cli") { Kind::Zed } else { kind_of(&bin) };
    Some(Editor { bin, kind })
}

/// Arguments that open FILE (relative to WT) at LINE.
pub fn args_at(kind: Kind, wt: &Path, file: &str, line: u32) -> Vec<OsString> {
    let full = wt.join(file);
    match kind {
        Kind::VsCode => vec![wt.into(), "-g".into(), format!("{}:{line}", full.display()).into()],
        Kind::JetBrains => vec!["--line".into(), line.to_string().into(), full.into()],
        Kind::Terminal => vec![format!("+{line}").into(), full.into()],
        // the worktree too: Zed takes the window that has it open, else a new one
        Kind::Zed => vec![wt.into(), format!("{}:{line}", full.display()).into()],
        Kind::Flow => vec!["--gui".into(), "--project".into(), wt.into(), format!("{}:{line}", full.display()).into()],
        Kind::Other => vec![format!("{}:{line}", full.display()).into()],
    }
}

impl Kind {
    /// The editor runs full screen in codehead's terminal (codehead suspends meanwhile).
    pub fn in_terminal(self) -> bool {
        matches!(self, Kind::Terminal)
    }
}

/// Arguments that open the worktree WT: the window that has it open, else a new one.
pub fn args_dir(kind: Kind, wt: &Path) -> Vec<OsString> {
    match kind {
        Kind::Flow => vec!["--gui".into(), "--project".into(), wt.into()],
        _ => vec![wt.into()],
    }
}

/// A running Flow instance with WT as a project, from `flow --list-servers --json`: its
/// socket. None: 0 instance, or a Flow without remote control.
pub fn flow_instance(bin: &Path, wt: &Path) -> Option<String> {
    use std::io::Read;
    let mut c = std::process::Command::new(bin).args(["--list-servers", "--json"]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn().ok()?;
    let start = std::time::Instant::now();
    loop {
        match c.try_wait().ok()? {
            Some(st) if st.success() => break,
            Some(_) => return None,
            None if start.elapsed() > std::time::Duration::from_millis(1500) => {
                let _ = c.kill();
                let _ = c.wait();
                return None;
            }
            None => std::thread::sleep(std::time::Duration::from_millis(15)),
        }
    }
    let mut out = String::new();
    c.stdout.take()?.read_to_string(&mut out).ok()?;
    flow_pick(&out, wt)
}

/// The socket of the instance in OUT (`--list-servers --json`: an array, or 1 object per line)
/// whose projects have WT.
fn flow_pick(out: &str, wt: &Path) -> Option<String> {
    let want = std::fs::canonicalize(wt).unwrap_or_else(|_| wt.to_path_buf());
    let all: Vec<serde_json::Value> = match serde_json::from_str::<serde_json::Value>(out.trim()) {
        Ok(serde_json::Value::Array(a)) => a,
        Ok(v) => vec![v],
        Err(_) => out.lines().filter_map(|l| serde_json::from_str(l).ok()).collect(),
    };
    all.iter()
        .find(|s| {
            s["projects"].as_array().is_some_and(|ps| {
                ps.iter().filter_map(|p| p.as_str().or_else(|| p["path"].as_str()).or_else(|| p["name"].as_str())).any(|p| {
                    let p = Path::new(p);
                    p == want || std::fs::canonicalize(p).is_ok_and(|c| c == want)
                })
            })
        })
        .and_then(|s| s["socket"].as_str().or_else(|| s["name"].as_str()).map(str::to_string))
}

/// E's arguments for a window that has WT open already, with FILE at LINE (None: the worktree
/// alone). None: 0 such window known (the usual arguments apply).
pub fn reuse_args(e: &Editor, wt: &Path, target: Option<(&str, u32)>) -> Option<Vec<OsString>> {
    if e.kind != Kind::Flow {
        return None;
    }
    let sock = flow_instance(&e.bin, wt)?;
    let mut v: Vec<OsString> = vec!["--connect".into(), sock.into(), "--no-wait".into()];
    match target {
        Some((file, line)) => v.push(format!("{}:{line}", wt.join(file).display()).into()),
        None => v.push(wt.into()),
    }
    Some(v)
}

/// Start E with ARGS, detached. Err: why not.
pub fn spawn(e: &Editor, args: &[OsString]) -> Result<(), String> {
    std::process::Command::new(&e.bin)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("{}: {err}", e.bin.display()))
}

/// Once per machine, for a worktree under codehead's worktrees dir: how to stop the trust
/// prompt / restricted mode for every worktree at once.
pub fn trust_hint(wt: &Path) -> Option<String> {
    let root = crate::engine::data_dir().join("worktrees");
    if !wt.starts_with(&root) {
        return None;
    }
    let mark = crate::config::home().join(".cache/codehead/trust-hint");
    if mark.exists() {
        return None;
    }
    if let Some(d) = mark.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(&mark, "");
    Some(format!("1 time: trust {} in your editor (its subfolders: every worktree)", root.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_per_editor_kind() {
        let wt = Path::new("/w");
        let s = |v: Vec<OsString>| v.iter().map(|x| x.to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(s(args_at(Kind::VsCode, wt, "src/a.rs", 12)), ["/w", "-g", "/w/src/a.rs:12"]);
        assert_eq!(s(args_at(Kind::Zed, wt, "src/a.rs", 12)), ["/w", "/w/src/a.rs:12"]);
        assert_eq!(s(args_at(Kind::JetBrains, wt, "src/a.rs", 12)), ["--line", "12", "/w/src/a.rs"]);
        assert_eq!(s(args_dir(Kind::VsCode, wt)), ["/w"]);
        assert_eq!(s(args_at(Kind::Terminal, wt, "src/a.rs", 12)), ["+12", "/w/src/a.rs"]);
        assert_eq!(kind_of(Path::new("/usr/local/bin/cursor")), Kind::VsCode);
        assert_eq!(kind_of(Path::new("rustrover")), Kind::JetBrains);
        assert_eq!(kind_of(Path::new("hx")), Kind::Other);
        assert_eq!(kind_of(Path::new("/usr/local/bin/flow")), Kind::Flow);
        assert_eq!(s(args_at(Kind::Flow, wt, "src/a.rs", 12)), ["--gui", "--project", "/w", "/w/src/a.rs:12"]);
        assert!(!Kind::Flow.in_terminal() && Kind::Terminal.in_terminal());
    }

    #[test]
    fn a_flow_instance_with_the_worktree_gets_the_file() {
        let list = r#"[{"name":"a","pid":1,"version":"v","socket":"/s/flow-1.sock","projects":["/other"]},{"name":"b","pid":2,"version":"v","socket":"/s/flow-2.sock","projects":["/w","/x"]}]"#;
        assert_eq!(flow_pick(list, Path::new("/w")).as_deref(), Some("/s/flow-2.sock"));
        let lines = "{\"name\":\"b\",\"socket\":\"/s/flow-2.sock\",\"projects\":[\"/w\"]}\n";
        assert_eq!(flow_pick(lines, Path::new("/w")).as_deref(), Some("/s/flow-2.sock"));
        assert_eq!(flow_pick(list, Path::new("/nope")), None);
        assert_eq!(flow_pick("", Path::new("/w")), None);
    }
}
