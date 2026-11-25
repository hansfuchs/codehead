//! Under the thread view's prompt: where the thread works. 1 entry per checkout of the thread
//! (its work dir 1st, then the dirs agents edited in): the repo, the branch now checked out
//! (read from `.git/HEAD`, 0 git call), a linked worktree or the repo's main checkout.

use std::path::{Path, PathBuf};

use ratatui::text::Span;

use crate::engine::store;
use crate::ui::theme;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Spot {
    /// The repo's name (its main checkout's dir); 0 repo: the dir's name.
    pub repo: String,
    /// The branch checked out (detached: `@` + short commit); 0 repo: None.
    pub branch: Option<String>,
    /// A linked worktree (`.git` is a file), not the main checkout.
    pub worktree: bool,
}

/// The git dir of DIR's checkout: (its top dir, the git dir, a linked worktree).
fn git_dir(dir: &Path) -> Option<(PathBuf, PathBuf, bool)> {
    for directory_value in dir.ancestors().filter(|directory| !directory.as_os_str().is_empty()) {
        let group = directory_value.join(".git");

        if group.is_dir() {
            return Some((directory_value.to_path_buf(), group, false));
        }

        if group.is_file() {
            let text = std::fs::read_to_string(&group).ok()?;
            let gd = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());

            return Some((
                directory_value.to_path_buf(),
                resolve_git_dir(directory_value, gd),
                true,
            ));
        }
    }

    None
}

/// The branch of HEAD in `GIT_DIR` (detached: `@` + 7 chars of the commit).
fn head(git_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let thread = text.trim();

    match thread.strip_prefix("ref:") {
        Some(result) => Some(result.trim().trim_start_matches("refs/heads/").to_string()),
        None => Some(format!("@{}", thread.get(..7).unwrap_or(thread))),
    }
}

/// The top dir of DIR's checkout (a linked worktree: its own top); 0 git: DIR.
pub(crate) fn top(dir: &Path) -> PathBuf {
    git_dir(dir).map_or_else(|| dir.to_path_buf(), |group| group.0)
}

/// The spot of DIR (BRANCH: the stored one, for a dir without git data).
pub(crate) fn spot(dir: &Path, branch: Option<String>) -> Spot {
    let name = |path: &Path| {
        path.file_name().map_or_else(
            || path.display().to_string(),
            |count| count.to_string_lossy().into_owned(),
        )
    };

    match git_dir(dir) {
        Some((_, gd, worktree)) => {
            let repo = crate::domain::threads::repo_root(dir).unwrap_or_else(|| dir.to_path_buf());

            Spot {
                repo: name(&repo),
                branch: head(&gd).or(branch),
                worktree,
            }
        }
        None => Spot {
            repo: name(dir),
            branch: None,
            worktree: false,
        },
    }
}

/// The spots of thread ID (dirs gone from disk: left out).
pub(crate) fn of_thread(id: &str) -> Vec<Spot> {
    let Some(thread) = store::load(id) else {
        return Vec::new();
    };

    thread
        .all_checkouts()
        .into_iter()
        .filter(|character| Path::new(&character.dir).is_dir())
        .map(|character_value| spot(Path::new(&character_value.dir), character_value.branch))
        .collect()
}

/// ` · in codehead ⎇ task/x (worktree), site ⎇ main`.
pub(crate) fn spans(spots: &[Spot]) -> Vec<Span<'static>> {
    if spots.is_empty() {
        return Vec::new();
    }

    let mut value = vec![Span::styled("  · in ", theme::muted())];

    for (i, text_value) in spots.iter().enumerate() {
        if i > 0 {
            value.push(Span::styled(", ", theme::muted()));
        }

        value.push(Span::styled(text_value.repo.clone(), theme::text()));

        if let Some(bytes) = &text_value.branch {
            value.push(Span::styled(" ⎇ ", theme::muted()));
            value.push(Span::styled(bytes.clone(), theme::fg(theme::pal().info())));
        }

        if text_value.worktree {
            value.push(Span::styled(" (worktree)", theme::muted()));
        }
    }

    value
}

#[cfg(test)]
#[path = "thread_view_workspot_tests.rs"]
mod tests;

fn resolve_git_dir(directory: &Path, path: PathBuf) -> PathBuf {
    if path.is_relative() { directory.join(path) } else { path }
}
