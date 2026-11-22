//! Under the thread view's prompt: where the thread works. 1 entry per checkout of the thread
//! (its work dir 1st, then the dirs agents edited in): the repo, the branch now checked out
//! (read from `.git/HEAD`, 0 git call), a linked worktree or the repo's main checkout.

use std::path::{Path, PathBuf};

use ratatui::style::Style;
use ratatui::text::Span;

use crate::engine::store;
use crate::theme;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spot {
    /// The repo's name (its main checkout's dir); 0 repo: the dir's name.
    pub repo: String,
    /// The branch checked out (detached: `@` + short commit); 0 repo: None.
    pub branch: Option<String>,
    /// A linked worktree (`.git` is a file), not the main checkout.
    pub worktree: bool,
}

/// The git dir of DIR's checkout: (its top dir, the git dir, a linked worktree).
fn git_dir(dir: &Path) -> Option<(PathBuf, PathBuf, bool)> {
    for d in dir.ancestors().filter(|d| !d.as_os_str().is_empty()) {
        let g = d.join(".git");
        if g.is_dir() {
            return Some((d.to_path_buf(), g, false));
        }
        if g.is_file() {
            let text = std::fs::read_to_string(&g).ok()?;
            let gd = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
            return Some((d.to_path_buf(), if gd.is_relative() { d.join(gd) } else { gd }, true));
        }
    }
    None
}

/// The branch of HEAD in GIT_DIR (detached: `@` + 7 chars of the commit).
fn head(git_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let t = text.trim();
    match t.strip_prefix("ref:") {
        Some(r) => Some(r.trim().trim_start_matches("refs/heads/").to_string()),
        None => Some(format!("@{}", t.get(..7).unwrap_or(t))),
    }
}

/// The top dir of DIR's checkout (a linked worktree: its own top); 0 git: DIR.
pub fn top(dir: &Path) -> PathBuf {
    git_dir(dir).map_or_else(|| dir.to_path_buf(), |g| g.0)
}

/// The spot of DIR (BRANCH: the stored one, for a dir without git data).
pub fn spot(dir: &Path, branch: Option<String>) -> Spot {
    let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
    match git_dir(dir) {
        Some((_, gd, worktree)) => {
            let repo = crate::threads::repo_root(dir).unwrap_or_else(|| dir.to_path_buf());
            Spot { repo: name(&repo), branch: head(&gd).or(branch), worktree }
        }
        None => Spot { repo: name(dir), branch: None, worktree: false },
    }
}

/// The spots of thread ID (dirs gone from disk: left out).
pub fn of_thread(id: &str) -> Vec<Spot> {
    let Some(t) = store::load(id) else { return Vec::new() };
    t.all_checkouts().into_iter().filter(|c| Path::new(&c.dir).is_dir()).map(|c| spot(Path::new(&c.dir), c.branch)).collect()
}

/// ` · in codehead ⎇ task/x (worktree), site ⎇ main`.
pub fn spans(spots: &[Spot]) -> Vec<Span<'static>> {
    if spots.is_empty() {
        return Vec::new();
    }
    let mut v = vec![Span::styled("  · in ", theme::muted())];
    for (i, s) in spots.iter().enumerate() {
        if i > 0 {
            v.push(Span::styled(", ", theme::muted()));
        }
        v.push(Span::styled(s.repo.clone(), theme::text()));
        if let Some(b) = &s.branch {
            v.push(Span::styled(" ⎇ ", theme::muted()));
            v.push(Span::styled(b.clone(), Style::default().fg(theme::pal().accent)));
        }
        if s.worktree {
            v.push(Span::styled(" (worktree)", theme::muted()));
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_checkout_worktree_and_detached_head() {
        let root = std::env::temp_dir().join(format!("codehead-workspot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // the main checkout: .git/ with HEAD on a branch
        let main = root.join("app");
        std::fs::create_dir_all(main.join(".git/worktrees/wt")).unwrap();
        std::fs::write(main.join(".git/HEAD"), "ref: refs/heads/feat/x\n").unwrap();
        assert_eq!(spot(&main.join("src").join(".."), None).branch.as_deref(), Some("feat/x"));
        let s = spot(&main, None);
        assert_eq!(s, Spot { repo: "app".into(), branch: Some("feat/x".into()), worktree: false });
        // a linked worktree: .git file → its git dir, detached
        let wt = root.join("wt");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", main.join(".git/worktrees/wt").display())).unwrap();
        std::fs::write(main.join(".git/worktrees/wt/HEAD"), "0123456789abcdef\n").unwrap();
        assert_eq!(spot(&wt, None), Spot { repo: "app".into(), branch: Some("@0123456".into()), worktree: true });
        // 0 git: the dir's name, 0 branch
        let plain = root.join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        assert_eq!(spot(&plain, Some("main".into())), Spot { repo: "plain".into(), branch: None, worktree: false });
        let text: String = spans(&[spot(&main, None), spot(&wt, None)]).iter().map(|s| s.content.to_string()).collect();
        assert_eq!(text, "  · in app ⎇ feat/x, app ⎇ @0123456 (worktree)");
        let _ = std::fs::remove_dir_all(&root);
    }
}
