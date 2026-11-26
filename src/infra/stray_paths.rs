//! Paths in a work tree that are never the work of a checkout: the temp files of atomic writes
//! and editor scratch files (they live for ms to s, then vanish: a row that comes and goes), and
//! nested repos (a subagent's worktree inside the checkout: its own changes, its own view).

use std::path::Path;

/// PATH (relative or absolute, `/` separated) names a temp or scratch file:
/// - Claude Code's atomic write: `<file>.tmp.<pid>.<12 hex>`, older `<file>.tmp.<8 hex>`
/// - vim: `4913` (its write test), `<file>~` (write backup), `.<file>.swp` … `.swa`, `.swx` (swap:
///   a rewrite every 4 s while the buffer is open)
/// - emacs: `.#<file>` (lock), `#<file>#` (auto save)
pub(crate) fn transient(path: &str) -> bool {
    let name = path.trim_end_matches('/').rsplit('/').next().unwrap_or_default();

    name == "4913"
        || name.ends_with('~')
        || name.starts_with(".#")
        || (name.len() > 2 && name.starts_with('#') && name.ends_with('#'))
        || vim_swap(name)
        || atomic_temp(name)
}

/// REL (a path git listed in TOP) is a nested repo or worktree: git lists one as `<dir>/` (a
/// plain untracked dir: its files one by one), or the dir holds a `.git` entry.
pub(crate) fn nested_repository(top: &Path, rel: &str) -> bool {
    rel.ends_with('/') || top.join(rel).join(".git").exists()
}

fn vim_swap(name: &str) -> bool {
    let Some(stem) = name.strip_prefix('.') else {
        return false;
    };

    let Some((_, extension)) = stem.rsplit_once('.') else {
        return false;
    };

    let bytes = extension.as_bytes();

    // `.swx`: vim's probe of the swap dir, `.swp` … `.swa`: 1 per open copy of the file
    bytes.len() == 3 && bytes[0] == b's' && bytes[1] == b'w' && (bytes[2] == b'x' || (b'a'..=b'p').contains(&bytes[2]))
}

fn atomic_temp(name: &str) -> bool {
    let Some((_, tail)) = name.rsplit_once(".tmp.") else {
        return false;
    };

    let hex = |text: &str, count: usize| text.len() == count && text.bytes().all(|byte| byte.is_ascii_hexdigit());

    match tail.split_once('.') {
        Some((pid, suffix)) => !pid.is_empty() && pid.bytes().all(|byte| byte.is_ascii_digit()) && hex(suffix, 12),
        None => hex(tail, 8),
    }
}

#[cfg(test)]
#[path = "stray_paths_tests.rs"]
mod tests;
