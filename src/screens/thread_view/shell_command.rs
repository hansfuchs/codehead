use super::{Duration, Instant, Path, PathBuf, SHELL_LOG_BYTES, SHELL_TIMEOUT, store};
use std::fmt::Write;

/// Run COMMAND in DIR with $SHELL -c (else sh): (stdout and stderr together, cut; exit code, -1:
/// killed or 0 start).
pub(super) fn shell(dir: &Path, command: &str) -> (String, i32) {
    let sh = std::env::var("SHELL")
        .ok()
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "sh".into());

    let Ok((reader, writer)) = std::io::pipe() else {
        return ("pipe failed".into(), -1);
    };

    let Ok(w2) = writer.try_clone() else {
        return ("pipe failed".into(), -1);
    };

    let child = std::process::Command::new(&sh)
        .arg("-c")
        .arg(command)
        .current_dir(dir)
        .env("PATH", crate::infra::pushlock::path_with_shim())
        .stdin(std::process::Stdio::null())
        .stdout(writer)
        .stderr(w2)
        .spawn();

    let mut child = match child {
        Ok(character) => character,
        Err(error) => return (format!("{sh}: {error}"), -1),
    };

    let read = std::thread::spawn(move || read_shell(reader));

    let start = Instant::now();

    let code = wait_shell(&mut child, start);

    let buf = read.join().unwrap_or_default();
    let out = shell_output(&buf, code, start);

    (out, code)
}

/// COMMAND runs `git push` (any of its parts: `;`, `&&`, `||`, `|`, new lines; `git -C x push`).
pub(super) fn git_push(command: &str) -> bool {
    command.split(['\n', ';', '&', '|']).any(git_push_width)
}

fn git_push_width(part: &str) -> bool {
    let mut width = part
        .split_whitespace()
        .skip_while(|thread| thread.contains('=') && !thread.starts_with('-'));

    if width
        .next()
        .is_none_or(|thread| thread != "git" && !thread.ends_with("/git"))
    {
        return false;
    }

    // the 1st word that is no option nor an option's value: the subcommand
    let mut skip = false;

    for thread in width {
        if skip {
            skip = false;
            continue;
        }

        if thread.starts_with('-') {
            skip = matches!(thread, "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace");
            continue;
        }

        return thread == "push";
    }

    false
}

/// P with the home dir as `~`.
/// `cd DIR` alone (0 `;`, `&&`, `|`, `$`): DIR (empty: `cd` alone). None: another command.
pub(super) fn cd_target(command: &str) -> Option<&str> {
    let character = command.trim();
    let rest = character
        .strip_prefix("cd")
        .filter(|result| result.is_empty() || result.starts_with(char::is_whitespace))?
        .trim();

    // 1 quoted dir: spaces in it
    if let Some(query) = rest
        .strip_prefix('"')
        .and_then(|result| result.strip_suffix('"'))
        .or_else(|| rest.strip_prefix('\'').and_then(|result| result.strip_suffix('\'')))
    {
        return (!query.contains(['"', '\''])).then_some(query);
    }

    if rest
        .chars()
        .any(|ch| matches!(ch, ';' | '&' | '|' | '$' | '`' | '>' | '<' | '(' | ')'))
        || rest.split_whitespace().count() > 1
    {
        return None;
    }

    Some(rest.trim_matches(|ch| ch == '"' || ch == '\''))
}

/// The checkout of thread T with the most line changes: the uncommitted ones plus the commits
/// since the thread began. A tie, 0 changes: the work dir (a git repo first). None: 1 checkout.
pub(super) fn busiest_checkout(thread: &store::Thread) -> Option<PathBuf> {
    let all = thread.all_checkouts();

    if all.len() < 2 {
        return None;
    }

    let since = format!("--since=@{}", thread.created / 1000);

    let sum = |out: &str| {
        out.lines()
            .filter_map(|line| {
                let mut file = line.split_whitespace();

                Some(file.next()?.parse::<u64>().unwrap_or(0) + file.next()?.parse::<u64>().unwrap_or(0))
            })
            .sum::<u64>()
    };

    let mut best: Option<(u64, bool, PathBuf)> = None;

    for character in all {
        let dir = PathBuf::from(&character.dir);
        let (git, _, _) = crate::domain::threads::git(&dir, &["rev-parse", "--is-inside-work-tree"]);

        let lines = if git {
            sum(&crate::domain::threads::git(&dir, &["diff", "HEAD", "--numstat"]).1)
                + sum(&crate::domain::threads::git(&dir, &["log", &since, "--numstat", "--format="]).1)
        } else {
            0
        };

        if best
            .as_ref()
            .is_none_or(|(line, group, _)| (lines, git) > (*line, *group))
        {
            best = Some((lines, git, dir));
        }
    }

    best.map(|bytes| bytes.2)
}

fn read_shell(mut reader: std::io::PipeReader) -> Vec<u8> {
    use std::io::Read;
    let mut buf = Vec::new();
    let _ = reader.by_ref().take(4 * SHELL_LOG_BYTES as u64).read_to_end(&mut buf);

    // the rest: drained, dropped
    let _ = std::io::copy(&mut reader, &mut std::io::sink());

    buf
}
fn wait_shell(child: &mut std::process::Child, start: Instant) -> i32 {
    loop {
        match child.try_wait() {
            Ok(Some(st)) => break st.code().unwrap_or(-1),
            Ok(None) if start.elapsed() > SHELL_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();

                break -1;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break -1,
        }
    }
}
fn shell_output(buf: &[u8], code: i32, start: Instant) -> String {
    let text = String::from_utf8_lossy(buf);

    // ANSI colors off: the plain text
    let text: String = crate::ui::ansi::to_lines(&text)
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|text_value| text_value.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut out: String = text.chars().take(SHELL_LOG_BYTES).collect();

    if out.len() < text.len() {
        out.push_str("\n… (cut)");
    }

    if code == -1 && start.elapsed() > SHELL_TIMEOUT {
        let _ = write!(out, "\n(killed after {} s)", SHELL_TIMEOUT.as_secs());
    }

    out
}
