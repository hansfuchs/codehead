use super::{Backend, Path, Terminal};

pub(super) fn open_url(url: &str) {
    if crate::infra::tutorial::active().is_some() {
        return;
    }

    let bin = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let _ = std::process::Command::new(bin)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Run yazi on TARGET full screen: the terminal modes off (raw, mouse, paste, keyboard flags,
/// alternate screen), yazi, the modes on again, a full redraw. Returns yazi's picked paths.
pub(super) fn run_outside<B: Backend>(term: &mut Terminal<B>, target: &Path) -> Result<Vec<String>, String> {
    if std::env::var_os("PATH")
        .and_then(|path| {
            std::env::split_paths(&path)
                .map(|directory| directory.join("yazi"))
                .find(|path| path.is_file())
        })
        .is_none()
    {
        return Err("yazi: not on PATH (brew install yazi)".into());
    }

    let (argv, env, chooser) = crate::features::files::yazi_cmd(target);
    let argv: Vec<std::ffi::OsString> = argv.into_iter().map(Into::into).collect();
    let status = suspended(term, &argv, &env).map_err(|error| format!("yazi: {error}"))?;

    if !status.success() {
        return Err(format!(
            "yazi exited with {status} (config: {})",
            crate::features::files::yazi_config().display()
        ));
    }

    let picked = std::fs::read_to_string(&chooser).unwrap_or_default();
    let _ = std::fs::remove_file(&chooser);

    Ok(picked
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

/// Run ARGV (with ENV) full screen: codehead's terminal modes off (raw, mouse, paste,
/// keyboard flags, alternate screen), the program, the modes on again, a full redraw.
pub(super) fn suspended<B: Backend>(
    term: &mut Terminal<B>,
    argv: &[std::ffi::OsString],
    env: &[(String, String)],
) -> std::io::Result<std::process::ExitStatus> {
    use ratatui::crossterm::{cursor, event, execute, terminal};
    let mut out = std::io::stdout();
    let _ = execute!(
        out,
        event::PopKeyboardEnhancementFlags,
        event::DisableMouseCapture,
        event::DisableBracketedPaste,
        terminal::LeaveAlternateScreen,
        cursor::Show
    );
    let _ = terminal::disable_raw_mode();
    let status = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .envs(env.iter().cloned())
        .status();
    let _ = terminal::enable_raw_mode();
    let _ = execute!(
        out,
        terminal::EnterAlternateScreen,
        event::EnableMouseCapture,
        event::EnableBracketedPaste,
        event::PushKeyboardEnhancementFlags(event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    );
    let _ = term.clear();

    status
}
