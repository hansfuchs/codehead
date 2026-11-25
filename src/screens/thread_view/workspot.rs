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
    for top in dir.ancestors().filter(|ancestor| !ancestor.as_os_str().is_empty()) {
        let dot_git = top.join(".git");

        if dot_git.is_dir() {
            return Some((top.to_path_buf(), dot_git, false));
        }

        if dot_git.is_file() {
            let text = std::fs::read_to_string(&dot_git).ok()?;
            let gitdir = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());

            return Some((top.to_path_buf(), resolve_git_dir(top, gitdir), true));
        }
    }

    None
}

/// The branch of HEAD in `GIT_DIR` (detached: `@` + 7 chars of the commit).
fn head(git_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = text.trim();

    match head.strip_prefix("ref:") {
        Some(reference) => Some(reference.trim().trim_start_matches("refs/heads/").to_string()),
        None => Some(format!("@{}", head.get(..7).unwrap_or(head))),
    }
}

/// The top dir of DIR's checkout (a linked worktree: its own top); 0 git: DIR.
pub(crate) fn top(dir: &Path) -> PathBuf {
    git_dir(dir).map_or_else(|| dir.to_path_buf(), |(top, _, _)| top)
}

/// The spot of DIR (BRANCH: the stored one, for a dir without git data).
pub(crate) fn spot(dir: &Path, branch: Option<String>) -> Spot {
    let name = |path: &Path| {
        path.file_name().map_or_else(
            || path.display().to_string(),
            |file_name| file_name.to_string_lossy().into_owned(),
        )
    };

    match git_dir(dir) {
        Some((_, gitdir, worktree)) => {
            let repo = crate::domain::threads::repo_root(dir).unwrap_or_else(|| dir.to_path_buf());

            Spot {
                repo: name(&repo),
                branch: head(&gitdir).or(branch),
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
        .filter(|checkout| Path::new(&checkout.dir).is_dir())
        .map(|checkout| spot(Path::new(&checkout.dir), checkout.branch))
        .collect()
}

/// ` · in codehead ⎇ task/x (worktree), site ⎇ main`.
pub(crate) fn spans(spots: &[Spot]) -> Vec<Span<'static>> {
    if spots.is_empty() {
        return Vec::new();
    }

    let mut spans = vec![Span::styled("  · in ", theme::muted())];

    for (i, spot) in spots.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(", ", theme::muted()));
        }

        spans.push(Span::styled(spot.repo.clone(), theme::text()));

        if let Some(branch) = &spot.branch {
            spans.push(Span::styled(" ⎇ ", theme::muted()));
            spans.push(Span::styled(branch.clone(), theme::fg(theme::pal().info())));
        }

        if spot.worktree {
            spans.push(Span::styled(" (worktree)", theme::muted()));
        }
    }

    spans
}

#[cfg(test)]
#[path = "thread_view_workspot_tests.rs"]
mod tests;

fn resolve_git_dir(top: &Path, gitdir: PathBuf) -> PathBuf {
    if gitdir.is_relative() { top.join(gitdir) } else { gitdir }
}
