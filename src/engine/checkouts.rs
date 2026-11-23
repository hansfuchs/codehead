//! More checkouts per thread: an agent edits a file in a git repo outside the thread's work dir
//! (another repo, another worktree) → that checkout joins the thread (`Thread::checkouts`), with
//! its branch. The merge tab finds the PRs of every checkout's branch; Claude agents get each
//! checkout as `--add-dir`.
//!
//! Edits only (Edit / Write / MultiEdit / NotebookEdit, ACP `edit` tools, Bash commands that
//! write: `git commit`, `sed -i`, `> file`, `mv` …, in their `cd DIR` / `git -C DIR`): reads of
//! other repos (research: `cd DIR && grep`, `git -C DIR status`) add 0 checkouts.

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

/// Git subcommands that change the checkout (its files, index or branch) or publish it.
const GIT_WRITES: &[&str] = &["add", "am", "apply", "checkout", "cherry-pick", "commit", "merge", "mv", "pull", "push", "rebase", "reset", "restore", "revert", "rm", "stash", "switch"];

/// Shell commands that write their path arguments.
const SHELL_WRITES: &[&str] = &["chmod", "cp", "install", "ln", "mkdir", "mv", "patch", "rm", "rmdir", "tee", "touch", "truncate"];

/// CMD without the bodies of its heredocs (`<<EOF` … `EOF`): their lines are text, not commands.
fn strip_heredocs(cmd: &str) -> String {
    let mut out = String::new();
    let mut end: Option<String> = None;
    for line in cmd.lines() {
        if let Some(e) = &end {
            if line.trim() == e {
                end = None;
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
        if let Some(i) = line.find("<<") {
            let rest = line[i + 2..].trim_start_matches(['-', '~']).trim_start();
            let word: String = rest.chars().take_while(|c| !c.is_whitespace() && !matches!(c, ';' | '&' | '|' | ')')).collect();
            let word = word.trim_matches(|c| c == '"' || c == '\'');
            if !word.is_empty() && !word.starts_with('<') {
                end = Some(word.to_string());
            }
        }
    }
    out
}

/// Word W without its surrounding quotes.
fn unquote(w: &str) -> &str {
    for q in ['"', '\''] {
        if w.len() >= 2 && w.starts_with(q) && w.ends_with(q) {
            return &w[1..w.len() - 1];
        }
    }
    w
}

/// Path word W, absolute or under CWD (`~/` → home); 0 path: None.
fn path_of(w: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    let w = unquote(w);
    if w.is_empty() || w.starts_with('-') || w.starts_with('$') {
        return None;
    }
    if let Some(r) = w.strip_prefix("~/") {
        return Some(crate::config::home().join(r));
    }
    let p = PathBuf::from(w);
    if p.is_absolute() {
        return Some(p);
    }
    cwd.map(|c| c.join(p))
}

/// The dirs and files shell command CMD writes in: per segment (`&&`, `;`, `|` …) that writes,
/// its dir (`cd DIR` before it, `git -C DIR`) and its absolute targets (`> /x`, `mv a /x`).
/// Segments that only read (`cd DIR && grep`, `git -C DIR status`): 0 paths.
fn work_dirs(cmd: &str) -> Vec<PathBuf> {
    let cmd = strip_heredocs(cmd);
    let mut out = Vec::new();
    let mut cwd: Option<PathBuf> = None;
    let segments = cmd.split([';', '&', '|', '(', ')', '\n']);
    for seg in segments {
        let words: Vec<&str> = seg.split_whitespace().collect();
        // leading `VAR=x` assignments, `sudo`, `command`: not the command
        let Some(at) = words.iter().position(|w| !(w.contains('=') && !w.starts_with('-')) && !matches!(*w, "sudo" | "command" | "time" | "env")) else { continue };
        let (verb, args) = (words[at], &words[at + 1..]);
        if verb == "cd" {
            cwd = args.first().and_then(|w| path_of(w, cwd.as_deref()));
            continue;
        }
        let mut dir = cwd.clone();
        let mut targets = Vec::new();
        let mut writes = false;
        // redirects: `> f`, `>> f`, `2> f`, `>f`; 0 for `>&2`, `/dev/…`
        for (i, w) in words.iter().enumerate() {
            let Some(r) = w.find('>') else { continue };
            if !w[..r].chars().all(|c| c.is_ascii_digit() || c == '&') {
                continue;
            }
            let rest = w[r..].trim_start_matches('>');
            let target = if rest.is_empty() { words.get(i + 1).copied().unwrap_or_default() } else { rest };
            if target.starts_with('&') || unquote(target).starts_with("/dev/") || target.is_empty() {
                continue;
            }
            writes = true;
            targets.extend(path_of(target, None));
        }
        match verb {
            "git" => {
                let mut i = 0;
                while i < args.len() && args[i].starts_with('-') {
                    if args[i] == "-C" {
                        dir = args.get(i + 1).and_then(|w| path_of(w, cwd.as_deref()));
                    }
                    i += if matches!(args[i], "-C" | "-c") { 2 } else { 1 };
                }
                writes |= args.get(i).is_some_and(|s| GIT_WRITES.contains(s));
            }
            "sed" | "perl" => {
                // `-i`, `-i.bak`, `--in-place`; perl: `-pi`, `-pie` (lower-case flag clusters)
                let perl_flags = |w: &str| w.len() > 1 && w[1..].chars().all(|c| c.is_ascii_lowercase()) && w.contains('i');
                let in_place = |w: &&str| *w == "--in-place" || (w.starts_with('-') && !w.starts_with("--") && (w.starts_with("-i") || (verb == "perl" && perl_flags(w))));
                if args.iter().any(in_place) {
                    writes = true;
                    targets.extend(args.iter().filter(|w| unquote(w).starts_with('/') || w.starts_with("~/")).filter_map(|w| path_of(w, None)));
                }
            }
            v if SHELL_WRITES.contains(&v) => {
                writes = true;
                let paths: Vec<PathBuf> = args.iter().filter_map(|w| path_of(w, None)).collect();
                // a copy writes its last argument only, the sources stay
                match v {
                    "cp" | "install" | "ln" => targets.extend(paths.last().cloned()),
                    _ => targets.extend(paths),
                }
            }
            _ => {}
        }
        if writes {
            out.extend(dir);
            out.extend(targets);
        }
    }
    out.retain(|p| !p.starts_with("/dev"));
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
    // codehead's own files: 0 checkout; its worktrees (`worktrees/<repo>/<task>`): checkouts
    let own = |p: &Path| p.starts_with(&data) && !p.starts_with(data.join("worktrees"));
    for p in edited(name, input) {
        // a worktree / repo inside a checkout (`.claude/worktrees/x`): a checkout of its own
        if have.iter().any(|c| p.starts_with(c) && !nested(&p, c)) || own(&p) {
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

/// CHECKOUTS of T that 0 edit in EVENTS falls in (added by a read of an older version).
fn unedited(t: &store::Thread, events: &[store::Event]) -> Vec<String> {
    let paths: Vec<PathBuf> = events
        .iter()
        .filter_map(|e| match &e.kind {
            store::Kind::Tool { name, input, .. } => Some(edited(name, input)),
            _ => None,
        })
        .flatten()
        .collect();
    t.checkouts.iter().filter(|c| !paths.iter().any(|p| p.starts_with(&c.dir))).map(|c| c.dir.clone()).collect()
}

/// Thread T once: drops the checkouts its events hold 0 edit in. Changed: the thread as saved.
/// 0 checkouts: 0 write (the checkouts of later edits hold their edit).
pub fn prune(t: &store::Thread) -> Option<store::Thread> {
    if t.checkouts_pruned || t.checkouts.is_empty() {
        return None;
    }
    // a handoff thread (`parent`) took its checkouts over: their edits sit in the parents' events
    let mut events = store::events(&t.id);
    let mut up = t.parent.clone();
    let mut seen = HashSet::from([t.id.clone()]);
    while let Some(id) = up.filter(|id| seen.insert(id.clone())) {
        events.extend(store::events(&id));
        up = store::load(&id).and_then(|p| p.parent);
    }
    let gone = unedited(t, &events);
    // under the thread's lock, not `store::update`: the thread's `updated` time stays (0 jump to
    // the top of the thread list); a checkout added since the read stays, only GONE goes
    super::locked(&store::thread_dir(&t.id).join("thread.lock"), || {
        let mut t = store::load(&t.id)?;
        t.checkouts.retain(|c| !gone.contains(&c.dir));
        t.checkouts_pruned = true;
        t.save().ok()?;
        Some(t)
    })
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn edits_and_work_dirs_count_reads_do_not() {
        assert_eq!(edited("Edit", &json!({"file_path": "/r/a.rs"})), vec![PathBuf::from("/r/a.rs")]);
        assert!(edited("Read", &json!({"file_path": "/r/a.rs"})).is_empty());
        assert!(edited("Bash", &json!({"command": "cd rel/dir && git commit -m x"})).is_empty());
        assert_eq!(edited("fs", &json!({"_kind": "edit", "locations": [{"path": "/x/y"}]})), vec![PathBuf::from("/x/y")]);
    }

    fn bash(cmd: &str) -> Vec<PathBuf> {
        edited("Bash", &json!({ "command": cmd }))
    }

    fn paths(v: &[&str]) -> Vec<PathBuf> {
        v.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn shell_reads_add_0_dirs() {
        assert!(bash("cd /other/repo && git status; git -C /third log").is_empty());
        assert!(bash("cd /r/notes && ls; echo ---; cat INDEX.md; grep -i deploy x.md").is_empty());
        assert!(bash("git -C ~/.cache/x/platform status -sb | head -2 2>/dev/null").is_empty());
        assert!(bash("cd /r && cargo test 2>&1 | tail -5 >/dev/null").is_empty());
        assert!(bash("cd /r && git -c a=b fetch --quiet origin main").is_empty());
        assert!(bash("cd /r && echo 'a -> b' && perl -Mstrict -ne 'print' x").is_empty());
        assert!(bash("cd /r && git worktree add -b feat $W/r/task origin/main").is_empty());
    }

    #[test]
    fn shell_writes_add_their_dirs() {
        assert_eq!(bash("cd /other/repo && git status; git commit -m x"), paths(&["/other/repo"]));
        assert_eq!(bash("git -C /third commit -am x"), paths(&["/third"]));
        assert_eq!(bash("cd /a && cd sub && sed -i '' s/x/y/ f.rs"), paths(&["/a/sub"]));
        assert_eq!(bash("cd /a; cd /b && git rm -rq lib"), paths(&["/b"]));
        assert_eq!(bash("echo x >> /n/notes/INDEX.md && tail -1 /n/notes/INDEX.md"), paths(&["/n/notes/INDEX.md"]));
        assert_eq!(bash("cd /a && echo x > out.txt"), paths(&["/a"]));
        assert_eq!(bash("cp /src/a.rs /dst/a.rs"), paths(&["/dst/a.rs"]));
        assert_eq!(bash("cd /a && perl -pi -e 's/x/y/' f"), paths(&["/a"]));
        assert_eq!(bash("cd /a && FOO=1 git push origin x"), paths(&["/a"]));
    }

    #[test]
    fn checkouts_without_an_edit_go() {
        let co = |d: &str| store::Checkout { dir: d.into(), branch: None };
        let t = store::Thread { dir: "/w".into(), checkouts: vec![co("/notes"), co("/read"), co("/cache/clone"), co("/wt")], ..Default::default() };
        let tool = |name: &str, input: Value| store::Event::new(Some("A"), store::Kind::Tool { id: "1".into(), name: name.into(), input });
        let events = [
            tool("Bash", json!({"command": "cd /read && grep -rn x .; git -C /cache/clone status"})),
            tool("Write", json!({"file_path": "/notes/plans/p.md"})),
            tool("Bash", json!({"command": "cd /wt && git commit -m x"})),
            tool("Read", json!({"file_path": "/read/a.rs"})),
        ];
        assert_eq!(unedited(&t, &events), vec!["/read".to_string(), "/cache/clone".to_string()]);
    }

    #[test]
    fn heredoc_bodies_are_text() {
        assert!(bash("cd /a && cat <<'EOF'\nrm -rf /b\ngit commit\nEOF\ngit log").is_empty());
        assert_eq!(bash("cd /a && cat > f.md <<EOF\nrm x\nEOF"), paths(&["/a"]));
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
