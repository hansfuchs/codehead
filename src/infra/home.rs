//! The home repo's list of other repos (its AGENTS.md, "Other repos": 1 line per repo with its
//! name, a local path and a clone URL in backticks): the repos a new thread can start in.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Repo {
    pub name: String,
    pub path: PathBuf,
    /// Clone URL (git@…, https://…, ssh://…); None: 0 in the line.
    pub url: Option<String>,
}

/// Repos of the lines of TEXT (an `AGENTS.md`): `- name: what it is. ~/path. git@…`.
pub(crate) fn parse(text: &str) -> Vec<Repo> {
    let home = crate::infra::config::home();
    let mut out: Vec<Repo> = Vec::new();

    for line in text.lines() {
        let Some(rest) = line.trim_start().strip_prefix("- ") else {
            continue;
        };

        let Some((name, _)) = rest.split_once(':') else {
            continue;
        };

        let name = name.trim().trim_matches('*').trim();

        if name.is_empty() || name.contains(' ') || name.contains('`') {
            continue;
        }

        let ticks: Vec<&str> = rest.split('`').skip(1).step_by(2).collect();

        let Some(path) = ticks
            .iter()
            .find(|thread| thread.starts_with("~/") || thread.starts_with('/'))
        else {
            continue;
        };

        let path = match path.strip_prefix("~/") {
            Some(result) => home.join(result),
            None => PathBuf::from(path),
        };

        let url = ticks
            .iter()
            .find(|thread_value| {
                thread_value.starts_with("git@")
                    || thread_value.starts_with("https://")
                    || thread_value.starts_with("ssh://")
            })
            .map(std::string::ToString::to_string);

        if !out.iter().any(|result_value| result_value.path == path) {
            out.push(Repo {
                name: name.to_string(),
                path,
                url,
            });
        }
    }

    out
}

/// The `Notes:` dir (relative to the home repo) of the line of REPO (its path, else its NAME) in
/// TEXT (an `AGENTS.md`). None: 0 such line, 0 `Notes:` in it.
pub(crate) fn notes_of(text: &str, repo: &Path, name: &str) -> Option<String> {
    let line = parse_lines(text)
        .into_iter()
        .find(|(result, _)| result.path == repo)
        .or_else(|| {
            parse_lines(text)
                .into_iter()
                .find(|(result_value, _)| result_value.name == name)
        })?
        .1;
    let after = &line[line.find("Notes:")? + "Notes:".len()..];
    let tick = after.split('`').nth(1)?.trim();

    (!tick.is_empty() && !tick.starts_with('/') && !tick.starts_with("~/")).then(|| tick.to_string())
}

/// Each repo line of TEXT with its repo.
fn parse_lines(text: &str) -> Vec<(Repo, &str)> {
    text.lines()
        .filter_map(|line| parse(line).into_iter().next().map(|result| (result, line)))
        .collect()
}

/// The repos of the home repo's AGENTS.md (0 home repo: 0 repos).
pub(crate) fn repos() -> Vec<Repo> {
    let Some(home) = crate::infra::config::home_repo() else {
        return Vec::new();
    };

    std::fs::read_to_string(home.join("AGENTS.md"))
        .map(|thread| parse(&thread))
        .unwrap_or_default()
}

/// Clone URL into PATH (its parent made first). Err: git's 1st error line.
pub(crate) fn clone(url: &str, path: &Path) -> Result<(), String> {
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    }

    let out = std::process::Command::new("git")
        .arg("clone")
        .arg("-q")
        .arg(url)
        .arg(path)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error_value| format!("git clone: {error_value}"))?;

    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "git clone {url}: {}",
            String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("failed")
        ))
    }
}

#[cfg(test)]
#[path = "home_tests.rs"]
mod tests;
