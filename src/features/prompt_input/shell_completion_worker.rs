use super::{Arc, Duration, FISH_TIMEOUT, Instant, MAX, Path, Req, Res};

/// TEXT done: a space after it, unless it goes on (`dir/`, `--flag=`).
pub(super) fn done(text: &str) -> String {
    if text.ends_with('/') || text.ends_with('=') {
        text.to_string()
    } else {
        format!("{text} ")
    }
}

/// The worker's job.
pub(super) fn work(req: Req) -> Res {
    match req {
        Req::Fish(line, dir) => {
            let rows = fish_complete(&line, &dir);

            Res::Fish(line, rows)
        }
        Req::History => Res::History(Arc::new(crate::features::prompt_input::history_search::shell_history())),
        Req::Commands => Res::Commands(Arc::new(path_commands())),
    }
}

/// fish's completions for LINE in DIR: (token, description). 0 fish, a timeout: 0 rows.
pub(super) fn fish_complete(line: &str, dir: &Path) -> Vec<(String, String)> {
    use std::io::Read;
    let child = std::process::Command::new("fish")
        .args(["--no-config", "-c", "complete -C -- $argv[1]", line])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return Vec::new() };

    let Some(mut out) = child.stdout.take() else {
        return Vec::new();
    };

    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = out.read_to_string(&mut text);

        text
    });

    let start = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() > FISH_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();

                return Vec::new();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(_) => return Vec::new(),
        }
    }

    let text = reader.join().unwrap_or_default();
    let mut seen = std::collections::HashSet::new();

    text.lines()
        .filter_map(|line| {
            let (token, description) = line.split_once('\t').unwrap_or((line, ""));

            (!token.is_empty() && seen.insert(token.to_string()))
                .then(|| (token.to_string(), description.trim().to_string()))
        })
        .take(MAX)
        .collect()
}

/// The executables on PATH, by name, once each.
pub(super) fn path_commands() -> Vec<String> {
    use std::os::unix::fs::PermissionsExt;

    let mut commands: Vec<String> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .flat_map(|dir| std::fs::read_dir(dir).into_iter().flatten().flatten())
        .filter(|entry| {
            entry
                .metadata()
                .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        })
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();

    commands.sort();
    commands.dedup();
    commands
}
