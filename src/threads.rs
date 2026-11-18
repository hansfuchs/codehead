//! Threads tab rows: every thread (in a worktree or in place), then worktrees of the repo without a thread.
//! Delete = thread + (linked worktree only) worktree + safe branch.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::engine::{client, store};

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::theme;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Thread {
    pub id: String,
    pub title: String,
    pub state: String,
    pub live: usize,
    pub updated: u64,
    /// worktree path of the thread, if it has one
    pub worktree: Option<String>,
    pub branch: Option<String>,
    /// project dir (path_hint) of a thread that works in place
    pub dir: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Worktree {
    pub path: String,
    pub branch: String,
    pub thread: Option<Thread>,
    pub unpushed: usize,
    pub when: String,
    /// A linked git worktree (delete may remove it). False: thread works in place.
    pub linked: bool,
}

impl Worktree {
    /// Selection key: thread id, else path (in-place threads share paths).
    pub fn key(&self) -> String {
        self.thread.as_ref().map(|t| t.id.clone()).unwrap_or_else(|| self.path.clone())
    }

    /// Name for the list: branch of a worktree, dir name of an in-place thread.
    pub fn label(&self) -> String {
        if self.linked {
            return self.branch.clone();
        }
        Path::new(&self.path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "scratch".into())
    }
}

pub fn git(cwd: &Path, args: &[&str]) -> (bool, String, String) {
    match Command::new("git").arg("-C").arg(cwd).args(args).stdin(Stdio::null()).output() {
        Ok(o) => (o.status.success(), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => (false, String::new(), e.to_string()),
    }
}

/// Main checkout of the git repo that holds PATH (also from inside a linked worktree).
pub fn repo_of(path: &Path) -> Option<PathBuf> {
    let (ok, out, _) = git(path, &["rev-parse", "--path-format=absolute", "--git-common-dir"]);
    let common = PathBuf::from(out.trim());
    if !ok || common.file_name()? != ".git" {
        return None;
    }
    common.parent().map(Path::to_path_buf)
}

fn real(p: &str) -> String {
    std::fs::canonicalize(p).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| p.to_string())
}

/// (path, branch) of every worktree except the main checkout and tool caches.
pub fn worktrees(repo: &Path) -> Vec<(String, String)> {
    let (_, out, _) = git(repo, &["worktree", "list", "--porcelain"]);
    let main = real(&repo.to_string_lossy());
    let cache = format!("{}/", real(&format!("{}/.cache", std::env::var("HOME").unwrap_or_default())));
    let mut items = Vec::new();
    let (mut path, mut branch): (Option<String>, Option<String>) = (None, None);
    for line in out.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if let Some(p) = path.take() {
                let r = real(&p);
                if r != main && !r.starts_with(&cache) {
                    items.push((p, branch.take().unwrap_or_else(|| "(detached)".into())));
                }
            }
            branch = None;
        } else if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(p.to_string());
        } else if let Some(b) = line.strip_prefix("branch ") {
            branch = Some(b.trim_start_matches("refs/heads/").to_string());
        }
    }
    items
}

/// Local branches, newest commit first.
pub fn branches(repo: &Path) -> Vec<String> {
    let (_, out, _) = git(repo, &["for-each-ref", "--sort=-committerdate", "--format=%(refname:short)", "refs/heads"]);
    out.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect()
}

/// Branch checked out at PATH ("" when detached).
pub fn current_branch(path: &Path) -> String {
    git(path, &["branch", "--show-current"]).1.trim().to_string()
}

/// Checkouts (main checkout included) by branch: branch → path.
pub fn checkouts(repo: &Path) -> std::collections::HashMap<String, String> {
    let (_, out, _) = git(repo, &["worktree", "list", "--porcelain"]);
    let mut map = std::collections::HashMap::new();
    let mut path = None;
    for line in out.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(p.to_string());
        } else if let Some(b) = line.strip_prefix("branch ")
            && let Some(p) = &path
        {
            map.insert(b.trim_start_matches("refs/heads/").to_string(), p.clone());
        }
    }
    map
}

/// Every thread, newest first.
pub fn all_threads() -> Vec<Thread> {
    crate::engine::store::all()
        .into_iter()
        .map(|t| Thread {
            live: if crate::engine::client::alive(&t.id) { crate::engine::client::state(&t.id).0.len() } else { 0 },
            updated: t.updated / 1000,
            worktree: t.worktree.clone(),
            branch: t.branch.clone(),
            dir: Some(t.dir.clone()),
            title: t.title,
            state: t.state,
            id: t.id,
        })
        .collect()
}

/// A new linked worktree of REPO on BRANCH (made from HEAD when it does not exist):
/// `<data>/worktrees/<repo>/<branch>`. Returns its path.
pub fn add_worktree(repo: &Path, branch: &str) -> Result<PathBuf, String> {
    let name = repo.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "repo".into());
    let path = crate::engine::data_dir().join("worktrees").join(name).join(branch.replace('/', "-"));
    if path.exists() {
        return Err(format!("{} exists", path.display()));
    }
    let p = path.display().to_string();
    let exists = git(repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).0;
    let args: Vec<&str> = if exists { vec!["worktree", "add", &p, branch] } else { vec!["worktree", "add", "-b", branch, &p] };
    let (ok, _, err) = git(repo, &args);
    if ok { Ok(path) } else { Err(format!("git worktree add: {err}")) }
}

fn when_of(path: &Path) -> String {
    let (_, ct, _) = git(path, &["log", "-1", "--format=%ct"]);
    ct.trim().parse::<u64>().map(|s| crate::notes::ago(std::time::UNIX_EPOCH + std::time::Duration::from_secs(s))).unwrap_or_else(|_| "?".into())
}

pub fn unpushed(branch: &str, path: &Path) -> usize {
    let r = if branch == "(detached)" { "HEAD" } else { branch };
    let (ok, out, _) = git(path, &["rev-list", "--count", r, "--not", "--remotes"]);
    if ok { out.trim().parse().unwrap_or(0) } else { 0 }
}

/// Rows: every thread (newest first), then worktrees of REPO without a thread.
pub fn load(repo: Option<&Path>) -> Vec<Worktree> {
    let wts = repo.map(worktrees).unwrap_or_default();
    let mut used = std::collections::HashSet::new();
    let mut out = Vec::new();
    for t in all_threads() {
        let wt = t.worktree.as_ref().and_then(|p| wts.iter().find(|(wp, _)| real(wp) == real(p)));
        let row = match (wt, &t.worktree) {
            (Some((path, branch)), _) => {
                used.insert(path.clone());
                let p = Path::new(path);
                Worktree { unpushed: unpushed(branch, p), when: when_of(p), path: path.clone(), branch: branch.clone(), linked: true, thread: Some(t) }
            }
            // worktree of another repo
            (None, Some(path)) if Path::new(path).exists() => {
                let branch = t.branch.clone().unwrap_or_else(|| "(detached)".into());
                let p = Path::new(path);
                Worktree { unpushed: unpushed(&branch, p), when: when_of(p), path: path.clone(), branch, linked: true, thread: Some(t) }
            }
            _ => {
                let when = crate::notes::ago(std::time::UNIX_EPOCH + std::time::Duration::from_secs(t.updated));
                Worktree { path: t.dir.clone().unwrap_or_default(), branch: "in place".into(), unpushed: 0, when, linked: false, thread: Some(t) }
            }
        };
        out.push(row);
    }
    for (path, branch) in wts {
        if used.contains(&path) {
            continue;
        }
        let p = Path::new(&path);
        out.push(Worktree { thread: None, unpushed: unpushed(&branch, p), when: when_of(p), path, branch, linked: true });
    }
    out
}

pub fn preview_header(w: &Worktree) -> Vec<Line<'static>> {
    let title = w.thread.as_ref().map(|t| t.title.clone()).unwrap_or_else(|| w.branch.clone());
    let mut out = vec![Line::styled(title, theme::accent_bold()), Line::styled(w.path.clone(), theme::muted()), Line::raw("")];
    match &w.thread {
        Some(t) => {
            out.push(Line::from(vec![
                Span::styled("thread ", theme::muted()),
                Span::raw(t.id.chars().take(12).collect::<String>()),
                Span::styled(" · state ", theme::muted()),
                Span::raw(t.state.clone()),
                Span::styled(" · live sessions ", theme::muted()),
                Span::raw(t.live.to_string()),
            ]));
            for l in t.title.lines() {
                out.push(Line::raw(l.to_string()));
            }
        }
        None => out.push(Line::styled("thread: none", theme::muted())),
    }
    out.push(Line::raw(""));
    out
}

pub fn preview(w: &Worktree) -> Vec<Line<'static>> {
    let p = Path::new(&w.path);
    let mut out = preview_header(w);
    if !w.linked {
        out.push(Line::styled("works in place: 0 worktree of its own", theme::muted()));
        if !p.join(".git").exists() && repo_of(p).is_none() {
            return out;
        }
    }
    let (_, st, _) = git(p, &["status", "--porcelain"]);
    let st: Vec<&str> = st.lines().collect();
    let col = |bad: bool| Style::default().fg(if bad { theme::pal().danger } else { theme::pal().ok });
    out.push(Line::styled(format!("uncommitted: {} files", st.len()), col(!st.is_empty())));
    out.extend(st.iter().take(15).map(|l| Line::raw(format!("  {l}"))));
    let r = if !w.linked || w.branch == "(detached)" { "HEAD".to_string() } else { w.branch.clone() };
    let (_, log, _) = git(p, &["log", "--oneline", "-15", &r, "--not", "--remotes"]);
    let log: Vec<&str> = log.lines().collect();
    out.push(Line::raw(""));
    out.push(Line::styled(format!("unpushed commits: {}", log.len()), col(!log.is_empty())));
    out.extend(log.iter().map(|l| Line::raw(format!("  {l}"))));
    let (_, last, _) = git(p, &["log", "-1", "--format=%cr · %s"]);
    out.push(Line::raw(""));
    out.push(Line::from(vec![Span::styled("last commit: ", theme::muted()), Span::raw(last.trim().to_string())]));
    out
}

/// Stop thread ID's host, delete its records.
pub fn delete_thread(id: &str) -> bool {
    if client::alive(id) {
        let _ = client::send(id, &serde_json::json!({"op": "stop"}));
    }
    store::delete(id).is_ok()
}

/// Delete thread, remove worktree (uncommitted files → git stash), delete branch with 0 unpushed commits.
pub fn remove(repo: &Path, items: &[Worktree], logs: &Path) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let danger = Style::default().fg(theme::pal().danger);
    let warn = Style::default().fg(theme::pal().warn);
    for w in items {
        let p = Path::new(&w.path);
        out.push(Line::from(vec![Span::styled(w.label(), theme::bold()), Span::styled(format!("  {}", w.path), theme::muted())]));
        if !w.linked {
            // in place: the dir is not ours. Delete the thread only.
            if let Some(t) = &w.thread {
                let ok = delete_thread(&t.id);
                out.push(Line::raw(format!("  thread {}: {} · dir kept (in place)", t.id.chars().take(12).collect::<String>(), if ok { "deleted" } else { "delete failed" })));
            }
            continue;
        }
        if !p.exists() {
            out.push(Line::styled("  skip: worktree gone", theme::muted()));
            continue;
        }
        // the worktree's own repo (can differ from the repo codehead runs in)
        let repo = repo_of(p).unwrap_or_else(|| repo.to_path_buf());
        let repo = repo.as_path();
        let (_, dirty, _) = git(p, &["status", "--porcelain"]);
        let n_dirty = dirty.lines().count();
        if n_dirty > 0 {
            let msg = format!("codehead: {} before worktree removal", w.branch);
            let (ok, _, err) = git(p, &["stash", "push", "--include-untracked", "-m", &msg]);
            if !ok {
                out.push(Line::styled(format!("  skip: stash failed: {err}"), danger));
                continue;
            }
            out.push(Line::styled(format!("  {n_dirty} uncommitted files stashed: \"{msg}\""), warn));
        }
        if let Some(t) = &w.thread {
            let ok = delete_thread(&t.id);
            let short: String = t.id.chars().take(12).collect();
            out.push(Line::raw(format!("  thread {short}: {}", if ok { "deleted" } else { "delete failed" })));
            let _ = std::fs::remove_file(logs.join(format!("host-{}.log", t.id)));
        }
        let up = unpushed(&w.branch, p);
        if w.branch == "(detached)" && up > 0 {
            out.push(Line::styled(format!("  skip: detached HEAD with {up} unpushed commits. commits would be lost."), danger));
            continue;
        }
        let (ok, _, err) = git(repo, &["worktree", "remove", &w.path]);
        if !ok {
            out.push(Line::styled(format!("  worktree: {err}"), danger));
            continue;
        }
        out.push(Line::raw("  worktree removed"));
        if w.branch == "(detached)" {
            continue;
        }
        if up > 0 {
            out.push(Line::styled(format!("  branch kept: {up} unpushed commits"), warn));
        } else {
            let (ok, _, err) = git(repo, &["branch", "-D", &w.branch]);
            out.push(if ok { Line::raw("  branch deleted") } else { Line::styled(format!("  branch: {err}"), danger) });
        }
    }
    out
}
