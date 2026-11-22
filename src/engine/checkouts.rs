//! More checkouts per thread: an agent edits a file in a git repo outside the thread's work dir
//! (another repo, another worktree) → that checkout joins the thread (`Thread::checkouts`), with
//! its branch. The merge tab finds the PRs of every checkout's branch; Claude agents get each
//! checkout as `--add-dir`.
//!
//! Edits only (Edit / Write / MultiEdit / NotebookEdit, ACP `edit` tools, Bash `cd DIR` /
//! `git -C DIR`): reads of other repos (research) add 0 checkouts.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use serde_json::Value;

use super::store;

/// The absolute paths a tool call NAME with INPUT edits (or works in).
pub fn edited(name: &str, input: &Value) -> Vec<PathBuf> {
    let s = |k: &str| input[k].as_str().map(PathBuf::from);
    let mut out = Vec::new();
    let edit = matches!(name, "Edit" | "Write" | "MultiEdit" | "NotebookEdit") || input["_kind"] == "edit";
    if edit {
        out.extend(["file_path", "notebook_path", "path"].iter().filter_map(|k| s(k)));
        // ACP: the edit's locations
        for l in input["locations"].as_array().into_iter().flatten() {
            if let Some(p) = l["path"].as_str() {
                out.push(PathBuf::from(p));
            }
        }
    }
    if name == "Bash"
        && let Some(cmd) = input["command"].as_str()
    {
        out.extend(work_dirs(cmd));
    }
    out.retain(|p| p.is_absolute());
    out
}

/// Dirs of shell command CMD: `cd DIR`, `git -C DIR`.
fn work_dirs(cmd: &str) -> Vec<PathBuf> {
    let words: Vec<&str> = cmd.split(|c: char| c.is_whitespace() || c == ';' || c == '&' || c == '|' || c == '(' || c == ')').filter(|w| !w.is_empty()).collect();
    let mut out = Vec::new();
    for w in words.windows(2) {
        if w[0] == "cd" || w[0] == "-C" {
            let d = w[1].trim_matches(|c| c == '"' || c == '\'');
            let d = d.strip_prefix("~/").map_or_else(|| PathBuf::from(d), |r| crate::config::home().join(r));
            out.push(d);
        }
    }
    out
}

/// The git toplevel of path P (a file or a dir) and its branch.
fn toplevel(p: &Path) -> Option<(PathBuf, Option<String>)> {
    let mut d = p.to_path_buf();
    while !d.is_dir() {
        d = d.parent()?.to_path_buf();
    }
    let git = |args: &[&str]| Command::new("git").arg("-C").arg(&d).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let top = PathBuf::from(git(&["rev-parse", "--show-toplevel"])?);
    let branch = git(&["rev-parse", "--abbrev-ref", "HEAD"]).filter(|b| b != "HEAD");
    Some((top, branch))
}

/// Path P sits in a linked worktree nested inside checkout C (`.claude/worktrees/x`; a submodule
/// or a nested repo: 0, they stay the checkout's). Filesystem only.
fn nested(p: &Path, c: &Path) -> bool {
    p.ancestors().take_while(|a| *a != c && a.starts_with(c)).any(crate::threads::linked_worktree)
}

/// Tool call NAME / INPUT of thread T: its edits outside T's checkouts add theirs.
pub fn note(t: &store::Thread, name: &str, input: &Value) {
    // a dir checked once per process: 0 more git calls
    static SEEN: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);
    let have: Vec<PathBuf> = t.all_checkouts().iter().map(|c| PathBuf::from(&c.dir)).collect();
    let data = super::data_dir();
    for p in edited(name, input) {
        // a worktree / repo inside a checkout (`.claude/worktrees/x`): a checkout of its own
        if have.iter().any(|c| p.starts_with(c) && !nested(&p, c)) || p.starts_with(&data) {
            continue;
        }
        let dir = if p.is_dir() { p.clone() } else { p.parent().map(Path::to_path_buf).unwrap_or_default() };
        {
            let mut g = SEEN.lock().unwrap_or_else(|e| e.into_inner());
            if !g.get_or_insert_with(HashSet::new).insert(dir) {
                continue;
            }
        }
        let Some((top, branch)) = toplevel(&p) else { continue };
        if have.contains(&top) {
            continue;
        }
        let c = store::Checkout { dir: top.display().to_string(), branch };
        let _ = store::update(&t.id, move |t| {
            t.add_checkout(c);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn edits_and_work_dirs_count_reads_do_not() {
        assert_eq!(edited("Edit", &json!({"file_path": "/r/a.rs"})), vec![PathBuf::from("/r/a.rs")]);
        assert!(edited("Read", &json!({"file_path": "/r/a.rs"})).is_empty());
        assert_eq!(edited("Bash", &json!({"command": "cd /other/repo && git status; git -C /third log"})), vec![PathBuf::from("/other/repo"), PathBuf::from("/third")]);
        assert!(edited("Bash", &json!({"command": "cd rel/dir"})).is_empty());
        assert_eq!(edited("fs", &json!({"_kind": "edit", "locations": [{"path": "/x/y"}]})), vec![PathBuf::from("/x/y")]);
    }

    #[test]
    fn checkouts_join_once() {
        let mut t = store::Thread { dir: "/a".into(), branch: Some("main".into()), ..Default::default() };
        assert!(t.add_checkout(store::Checkout { dir: "/b".into(), branch: Some("feat".into()) }));
        assert!(!t.add_checkout(store::Checkout { dir: "/b".into(), branch: None }));
        assert!(!t.add_checkout(store::Checkout { dir: "/a".into(), branch: None }));
        let all = t.all_checkouts();
        assert_eq!(all.iter().map(|c| c.dir.as_str()).collect::<Vec<_>>(), vec!["/a", "/b"]);
    }

    #[test]
    fn a_worktree_inside_a_checkout_counts_as_its_own() {
        let root = std::env::temp_dir().join(format!("codehead-nested-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let wt = root.join(".claude/worktrees/x");
        std::fs::create_dir_all(wt.join("src")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(wt.join(".git"), "gitdir: ../../../.git/worktrees/x").unwrap();

        assert!(nested(&wt.join("src/a.rs"), &root), "under the nested worktree");
        assert!(!nested(&root.join("src/a.rs"), &root), "the checkout's own file");

        // a submodule: the checkout's own
        std::fs::create_dir_all(root.join("lib")).unwrap();
        std::fs::write(root.join("lib/.git"), "gitdir: ../.git/modules/lib").unwrap();
        assert!(!nested(&root.join("lib/x.c"), &root), "a submodule");
        let _ = std::fs::remove_dir_all(&root);
    }
}
