//! Threads tab rows: every thread (in a worktree or in place), then worktrees of the repo without a thread.
//! Delete = thread + (linked worktree only) worktree + safe branch.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::engine::{client, store};

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::theme;

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
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
    /// An agent works now.
    #[serde(default)]
    pub busy: bool,
    /// BUSY only by a monitor or background command of a live agent (0 turn, 0 sub-thread).
    #[serde(default)]
    pub watching: bool,
    /// Why the owner is needed: `question`, `error: …`, `setup failed`.
    #[serde(default)]
    pub needs: Option<String>,
    /// Changes against the base: files, lines added, removed.
    #[serde(default)]
    pub stat: Option<(usize, usize, usize)>,
    #[serde(default)]
    pub agents: Vec<String>,
    /// Last status per agent: (agent, state, note).
    #[serde(default)]
    pub notes: Vec<(String, String, String)>,
    #[serde(default)]
    pub task: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub setup: Option<String>,
    #[serde(default)]
    pub parent: Option<String>,
    /// portless: the worktree's live URLs.
    #[serde(default)]
    pub urls: Vec<String>,
    /// The owner's repo pick (`store::Thread::repo`): wins over DIR / WORKTREE.
    #[serde(default)]
    pub repo: Option<String>,
}

/// From the tail of thread ID's log: an open question, the last error (a turn that ended in
/// one, after the last owner message), the last status per agent.
fn activity(id: &str) -> (bool, Option<String>, Vec<(String, String, String)>) {
    use crate::engine::store::Kind;
    let mut open: Vec<(String, String)> = Vec::new();
    let mut error: Option<String> = None;
    let mut notes: Vec<(String, String, String)> = Vec::new();
    // the last 400 events: the last 512 KiB of the log hold them (0 full read of a 10 MB log)
    let tail = crate::engine::store::events_tail(id, 512 << 10);
    for e in tail.into_iter().rev().take(400).collect::<Vec<_>>().into_iter().rev() {
        let a = e.agent.clone().unwrap_or_default();
        match e.kind {
            Kind::Ask { id, .. } => open.push((a, id)),
            Kind::Answer { id, .. } => open.retain(|x| x.1 != id),
            Kind::TurnEnd { error: err, .. } => {
                open.retain(|x| x.0 != a);
                error = err.filter(|m| m != crate::engine::host::INTERRUPTED && m != "session ended").map(|m| format!("{a}: {}", m.lines().next().unwrap_or_default()));
            }
            Kind::Error { message } => error = Some(format!("{a}: {}", message.lines().next().unwrap_or_default())),
            Kind::Prompt { from, .. } if from == "owner" => error = None,
            Kind::Status { state, note } => {
                notes.retain(|x| x.0 != a);
                notes.push((a, state, note));
            }
            _ => {}
        }
    }
    (!open.is_empty(), error, notes)
}

/// Changes of DIR against its merge-base with origin/<default> (else HEAD), untracked files
/// counted as files: (files, added, removed). None: 0 git.
fn shortstat(dir: &Path) -> Option<(usize, usize, usize)> {
    if !git(dir, &["rev-parse", "--is-inside-work-tree"]).0 {
        return None;
    }
    let def = ["origin/HEAD", "origin/main", "origin/master"].into_iter().find(|r| git(dir, &["rev-parse", "--verify", "--quiet", r]).0);
    let base = def.and_then(|d| Some(git(dir, &["merge-base", "HEAD", d]).1.trim().to_string()).filter(|s| !s.is_empty())).unwrap_or_else(|| "HEAD".into());
    let s = git(dir, &["diff", "--shortstat", &base]).1;
    let num = |key: &str| s.split(',').find(|p| p.contains(key)).and_then(|p| p.split_whitespace().next()).and_then(|n| n.parse::<usize>().ok()).unwrap_or(0);
    let untracked = git(dir, &["ls-files", "--others", "--exclude-standard"]).1.lines().count();
    Some((num("file") + untracked, num("insertion"), num("deletion")))
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Worktree {
    pub path: String,
    pub branch: String,
    pub thread: Option<Thread>,
    pub unpushed: usize,
    pub when: String,
    /// A linked git worktree (delete may remove it). False: thread works in place.
    pub linked: bool,
    /// Main checkout of the row's git repo (repo filter). None: outside any repo.
    #[serde(default)]
    pub repo: Option<PathBuf>,
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

/// DIR is a linked worktree (`git worktree add`): its `.git` file points into
/// `<repo>/.git/worktrees/` (a submodule's points into `.git/modules/`: 0). Filesystem only.
pub fn linked_worktree(dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(dir.join(".git")) else { return false };
    let Some(gitdir) = text.trim().strip_prefix("gitdir:").map(|g| PathBuf::from(g.trim())) else { return false };
    let names: Vec<String> = gitdir.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    names.windows(2).any(|w| w[0] == ".git" && w[1] == "worktrees")
}

/// Main checkout of the git repo that holds PATH, by the file system only (0 git runs: the threads
/// tabs call it per row): the nearest `.git` dir, or for a linked worktree the repo its `.git`
/// file points to (`gitdir: <repo>/.git/worktrees/<name>`).
pub fn repo_root(path: &Path) -> Option<PathBuf> {
    for d in path.ancestors().filter(|d| !d.as_os_str().is_empty()) {
        let g = d.join(".git");
        if g.is_dir() {
            return Some(d.to_path_buf());
        }
        if g.is_file() {
            let text = std::fs::read_to_string(&g).ok()?;
            let gitdir = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
            let gitdir = if gitdir.is_relative() { d.join(gitdir) } else { gitdir };
            let main = gitdir.ancestors().find(|a| a.file_name().is_some_and(|n| n == ".git")).and_then(Path::parent);
            return Some(main.map_or_else(|| d.to_path_buf(), Path::to_path_buf));
        }
    }
    None
}

/// Repo of a thread: the owner's pick, else its project dir's, else its worktree's.
pub fn repo_of_thread(t: &Thread) -> Option<PathBuf> {
    if let Some(r) = t.repo.as_deref().filter(|r| !r.is_empty()) {
        return Some(PathBuf::from(r));
    }
    [t.dir.as_deref(), t.worktree.as_deref()].into_iter().flatten().find_map(|p| repo_root(Path::new(p)))
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

/// Branch checked out at PATH ("" when detached), by the file system only (a git run: ~13 ms):
/// the HEAD of its checkout (a linked worktree: the HEAD in its git dir).
pub fn current_branch(path: &Path) -> String {
    let Some(top) = toplevel(path) else { return String::new() };
    let g = top.join(".git");
    let head = if g.is_dir() {
        g.join("HEAD")
    } else {
        let Ok(text) = std::fs::read_to_string(&g) else { return String::new() };
        let Some(dir) = text.trim().strip_prefix("gitdir:").map(str::trim) else { return String::new() };
        let dir = PathBuf::from(dir);
        (if dir.is_relative() { top.join(dir) } else { dir }).join("HEAD")
    };
    std::fs::read_to_string(head).ok().and_then(|h| h.trim().strip_prefix("ref: refs/heads/").map(String::from)).unwrap_or_default()
}

/// Top dir of the checkout (main or linked worktree) that holds PATH, by the file system only:
/// the nearest dir with a `.git` dir or file.
pub fn toplevel(path: &Path) -> Option<PathBuf> {
    path.ancestors().filter(|d| !d.as_os_str().is_empty()).find(|d| d.join(".git").exists()).map(Path::to_path_buf)
}

/// DIR on the default branch (main, else master) with an upstream: `git pull --ff-only` first
/// (30 s max, 0 prompt). None: another branch or 0 upstream (0 pull). Err: git's last line.
pub fn update_default(dir: &Path) -> Option<Result<String, String>> {
    let b = current_branch(dir);
    if b != "main" && b != "master" {
        return None;
    }
    if !git(dir, &["rev-parse", "--abbrev-ref", "@{upstream}"]).0 {
        return None;
    }
    let argv = ["git", "pull", "--ff-only", "--quiet"].map(String::from);
    let env = [("GIT_TERMINAL_PROMPT".to_string(), "0".to_string())];
    let (ok, out) = crate::setup::run(&argv, dir, &env, std::time::Duration::from_secs(30));
    let last = out.lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or("git pull failed").to_string();
    Some(if ok { Ok(b) } else { Err(last) })
}

/// Checkouts (main checkout included) by branch: branch → path. The file system first (the
/// repo's `HEAD`, `worktrees/*/{HEAD,gitdir}`: 0 git spawn on the UI thread), else `git
/// worktree list`.
pub fn checkouts(repo: &Path) -> std::collections::HashMap<String, String> {
    checkouts_fs(repo).unwrap_or_else(|| checkouts_git(repo))
}

/// The common git dir of the checkout at REPO (a linked worktree: its main repo's). None: 0 git
/// repo there, or a layout the file system read does not know.
fn common_dir(repo: &Path) -> Option<PathBuf> {
    let top = toplevel(repo)?;
    let g = top.join(".git");
    if g.is_dir() {
        return Some(g);
    }
    let text = std::fs::read_to_string(&g).ok()?;
    let dir = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
    let dir = if dir.is_relative() { top.join(dir) } else { dir };
    let common = std::fs::read_to_string(dir.join("commondir")).ok()?;
    let common = PathBuf::from(common.trim());
    Some(if common.is_relative() { dir.join(common) } else { common })
}

fn checkouts_fs(repo: &Path) -> Option<std::collections::HashMap<String, String>> {
    let common = std::fs::canonicalize(common_dir(repo)?).ok()?;
    // a bare repo (0 `.git` dir name): git knows it better
    if common.file_name()? != ".git" {
        return None;
    }
    let branch = |head: &Path| std::fs::read_to_string(head).ok().and_then(|h| h.trim().strip_prefix("ref: refs/heads/").map(String::from));
    let mut map = std::collections::HashMap::new();
    if let Some(b) = branch(&common.join("HEAD")) {
        map.insert(b, common.parent()?.display().to_string());
    }
    if let Ok(rd) = std::fs::read_dir(common.join("worktrees")) {
        for e in rd.flatten() {
            let d = e.path();
            let Some(b) = branch(&d.join("HEAD")) else { continue };
            let Ok(gitdir) = std::fs::read_to_string(d.join("gitdir")) else { continue };
            let Some(wt) = Path::new(gitdir.trim()).parent() else { continue };
            map.insert(b, wt.display().to_string());
        }
    }
    Some(map)
}

fn checkouts_git(repo: &Path) -> std::collections::HashMap<String, String> {
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

/// Every thread, newest first, with git stats and live URLs (threads tab).
pub fn all_threads() -> Vec<Thread> {
    scan(true)
}

/// Every thread, newest first, 0 git stats, 0 URLs: thread header, mention menu, boot.
pub fn all_threads_lite() -> Vec<Thread> {
    scan(false)
}

/// F over ITEMS on up to 8 OS threads, order kept.
pub fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 8);
    if items.len() < 2 || n == 1 {
        return items.iter().map(&f).collect();
    }
    let size = items.len().div_ceil(n);
    std::thread::scope(|s| {
        let parts: Vec<_> = items.chunks(size).map(|c| s.spawn(|| c.iter().map(&f).collect::<Vec<R>>())).collect();
        parts.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    })
}

fn scan(full: bool) -> Vec<Thread> {
    // portless: 1 read of the live routes for every thread
    let routes = if full && crate::portless::active() { crate::portless::routes() } else { Vec::new() };
    let list = crate::engine::store::all();
    // git stats: once per work dir (threads share dirs), archived threads skipped
    let stats: std::collections::HashMap<PathBuf, Option<(usize, usize, usize)>> = if full {
        let mut dirs: Vec<PathBuf> = list.iter().filter(|t| t.state != "archived" && t.state != "deleted").map(|t| t.work_dir()).collect();
        dirs.sort();
        dirs.dedup();
        let s = par_map(&dirs, |d| shortstat(d));
        dirs.into_iter().zip(s).collect()
    } else {
        Default::default()
    };
    par_map(&list, |t| {
            let t = t.clone();
            let alive = crate::engine::client::alive(&t.id);
            let (live, busy) = if alive { crate::engine::client::state(&t.id) } else { (Vec::new(), Vec::new()) };
            // work goes on without a turn: a running sub-thread, a monitor or background command
            let turn = !busy.is_empty() || !t.kids.is_empty();
            let watching = !turn && alive && crate::monitors::watching(&t.id, &live);
            let busy = turn || watching;
            let (ask, error, notes) = activity(&t.id);
            let needs = if ask {
                Some("question".to_string())
            } else if t.setup.as_deref() == Some("failed") {
                Some("setup failed".to_string())
            } else {
                error.map(|e| format!("error: {e}"))
            };
            Thread {
                live: live.len(),
                busy,
                watching,
                needs,
                stat: stats.get(&t.work_dir()).copied().flatten(),
                agents: t.agents.clone(),
                notes,
                task: t.task.clone(),
                port: t.port,
                setup: t.setup.clone(),
                parent: t.parent.clone(),
                urls: t.worktree.as_ref().and(t.branch.as_deref()).map(|b| crate::portless::urls_of(&routes, &crate::portless::prefix(b))).unwrap_or_default(),
                updated: t.updated / 1000,
                repo: t.repo.clone(),
                worktree: t.worktree.clone(),
                branch: t.branch.clone(),
                dir: Some(t.dir.clone()),
                title: t.title,
                state: t.state,
                id: t.id,
            }
    })
}

/// The path of REPO's codehead worktree on BRANCH.
pub fn worktree_path(repo: &Path, branch: &str) -> PathBuf {
    let name = repo.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "repo".into());
    crate::engine::data_dir().join("worktrees").join(name).join(branch.replace('/', "-"))
}

/// A new linked worktree of REPO on BRANCH (made from HEAD when it does not exist):
/// `<data>/worktrees/<repo>/<branch>`. Returns its path.
pub fn add_worktree(repo: &Path, branch: &str) -> Result<PathBuf, String> {
    let path = worktree_path(repo, branch);
    if path.exists() {
        return Err(format!("{} exists", path.display()));
    }
    let p = path.display().to_string();
    let exists = git(repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).0;
    // a new branch starts from origin/<default> (fetched), else HEAD
    let base = if exists { String::new() } else { crate::setup::new_branch_base(repo) };
    let args: Vec<&str> = if exists { vec!["worktree", "add", &p, branch] } else { vec!["worktree", "add", "--no-track", "-b", branch, &p, &base] };
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
        let rp = repo_of_thread(&t);
        let row = match (wt, &t.worktree) {
            (Some((path, branch)), _) => {
                used.insert(path.clone());
                let p = Path::new(path);
                Worktree { unpushed: unpushed(branch, p), when: when_of(p), path: path.clone(), branch: branch.clone(), linked: true, thread: Some(t), repo: rp }
            }
            // worktree of another repo
            (None, Some(path)) if Path::new(path).exists() => {
                let branch = t.branch.clone().unwrap_or_else(|| "(detached)".into());
                let p = Path::new(path);
                Worktree { unpushed: unpushed(&branch, p), when: when_of(p), path: path.clone(), branch, linked: true, thread: Some(t), repo: rp }
            }
            _ => {
                let when = crate::notes::ago(std::time::UNIX_EPOCH + std::time::Duration::from_secs(t.updated));
                Worktree { path: t.dir.clone().unwrap_or_default(), branch: "in place".into(), unpushed: 0, when, linked: false, thread: Some(t), repo: rp }
            }
        };
        out.push(row);
    }
    for (path, branch) in wts {
        if used.contains(&path) {
            continue;
        }
        let p = Path::new(&path);
        out.push(Worktree { thread: None, unpushed: unpushed(&branch, p), when: when_of(p), path, branch, linked: true, repo: repo.map(Path::to_path_buf) });
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
            if let Some(n) = &t.needs {
                out.push(Line::styled(format!("needs you: {n}"), theme::fg(theme::pal().attention())));
            }
            // agents: the last status each reported
            if !t.agents.is_empty() {
                out.push(Line::raw(""));
                for a in &t.agents {
                    let note = t.notes.iter().find(|x| x.0 == *a).map(|x| if x.2.is_empty() { x.1.clone() } else { format!("{} · {}", x.1, x.2) }).unwrap_or_default();
                    out.push(Line::from(vec![Span::styled(format!("{a:<10}"), theme::bold()), Span::styled(note, theme::muted())]));
                }
            }
            let mut facts: Vec<String> = Vec::new();
            if let Some((files, add, del)) = t.stat.filter(|s| s.0 > 0) {
                facts.push(format!("changes {files} files +{add} −{del}"));
            }
            if let Some(p) = t.port {
                facts.push(format!("ports {p}-{}", p + crate::setup::PORTS - 1));
            }
            if let Some(s) = &t.setup {
                facts.push(format!("setup {s}"));
            }
            for u in &t.urls {
                facts.push(format!("↗ {}", u.trim_start_matches("https://")));
            }
            if let Some(p) = &t.parent {
                facts.push(format!("continues {}", p.chars().take(12).collect::<String>()));
            }
            if !facts.is_empty() {
                out.push(Line::raw(""));
                out.push(Line::styled(facts.join(" · "), theme::muted()));
            }
            // the task: its 1st lines
            let task: Vec<&str> = t.task.lines().filter(|l| !l.trim().is_empty()).take(5).collect();
            if !task.is_empty() {
                out.push(Line::raw(""));
                out.push(Line::styled("task", theme::muted()));
                out.extend(task.iter().map(|l| Line::raw(l.chars().take(200).collect::<String>())));
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
    let col = |bad: bool| Style::default().fg(if bad { theme::pal().error() } else { theme::pal().success() });
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
    let danger = Style::default().fg(theme::pal().error());
    let warn = theme::fg(theme::pal().caution());
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
        // the teardown script (docker down, test DB …) before the worktree goes
        if let Some(t) = &w.thread
            && let Some(st) = store::load(&t.id)
            && let Some((label, ok, output)) = crate::setup::run_teardown(&st)
        {
            let st = if ok { theme::muted() } else { warn };
            out.push(Line::styled(format!("  {label}: {}", if ok { "ran" } else { "failed" }), st));
            if !ok {
                out.extend(output.lines().rev().take(3).collect::<Vec<_>>().into_iter().rev().map(|l| Line::styled(format!("    {l}"), theme::muted())));
            }
        }
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

/// The delete row of thread ID, from its record: its linked worktree (on disk), else its dir (in
/// place: delete keeps the dir; an archived thread: its branch stays).
pub fn row_of(id: &str) -> Option<Worktree> {
    let t = store::load(id)?;
    let wt = t.worktree.clone().filter(|p| Path::new(p).is_dir());
    let lite = Thread { id: t.id.clone(), title: t.title.clone(), state: t.state.clone(), dir: Some(t.dir.clone()), worktree: t.worktree.clone(), branch: t.branch.clone(), repo: t.repo.clone(), ..Default::default() };
    let repo = repo_of_thread(&lite);
    let branch = match &wt {
        Some(p) => t.branch.clone().filter(|b| !b.is_empty()).unwrap_or_else(|| Some(current_branch(Path::new(p))).filter(|b| !b.is_empty()).unwrap_or_else(|| "(detached)".into())),
        None => "in place".into(),
    };
    Some(Worktree { path: wt.clone().unwrap_or_else(|| t.dir.clone()), branch, linked: wt.is_some(), thread: Some(lite), unpushed: 0, when: String::new(), repo })
}

/// Archive thread ID: its host stops; a linked worktree goes (the teardown script first,
/// uncommitted files → git stash), its branch stays in `worktree_pending`: the next host start (an
/// unarchive, an open) makes the worktree again. In place: the thread only. Ok: what happened.
pub fn archive(id: &str) -> Result<String, String> {
    let t = store::load(id).ok_or_else(|| format!("no thread {id}"))?;
    if t.state == "archived" {
        return Ok("archived already".into());
    }
    if !t.kids.is_empty() {
        return Err(format!("{} sub-threads run: stop them first", t.kids.len()));
    }
    let wt = t.worktree.clone().filter(|p| Path::new(p).is_dir());
    // checks first: a refused archive leaves the thread as it was
    let branch = match &wt {
        Some(p) => {
            let b = t.branch.clone().filter(|b| !b.is_empty()).unwrap_or_else(|| current_branch(Path::new(p)));
            if b.is_empty() || b == "(detached)" {
                return Err("detached HEAD: 0 branch keeps its commits. check out a branch first".into());
            }
            Some(b)
        }
        None => None,
    };
    if client::alive(&t.id) {
        let _ = client::send_within(&t.id, &serde_json::json!({"op": "stop"}), std::time::Duration::from_secs(3));
    }
    let mut done = Vec::new();
    let repo = repo_root(Path::new(&t.dir)).or_else(|| wt.as_deref().and_then(|p| repo_of(Path::new(p))));
    if let (Some(p), Some(b)) = (&wt, &branch) {
        let path = Path::new(p);
        if let Some((label, ok, _)) = crate::setup::run_teardown(&t) {
            done.push(format!("{label}: {}", if ok { "ran" } else { "failed" }));
        }
        let (_, dirty, _) = git(path, &["status", "--porcelain"]);
        let n = dirty.lines().count();
        if n > 0 {
            let msg = format!("codehead: {b} before archive");
            let (ok, _, err) = git(path, &["stash", "push", "--include-untracked", "-m", &msg]);
            if !ok {
                return Err(format!("stash failed: {}", err.trim()));
            }
            done.push(format!("{n} uncommitted files → git stash \"{msg}\""));
        }
        let main = repo_of(path).or_else(|| repo.clone()).ok_or_else(|| format!("{p}: 0 git repo"))?;
        let (ok, _, err) = git(&main, &["worktree", "remove", "--force", p]);
        if !ok {
            return Err(format!("git worktree remove: {}", err.trim()));
        }
        done.push(format!("worktree removed · branch {b} kept"));
    }
    let repo_s = repo.map(|r| r.display().to_string());
    store::update(&t.id, |s| {
        s.state = "archived".into();
        if let Some(b) = &branch {
            s.worktree = None;
            s.worktree_pending = Some(b.clone());
            s.port = None;
            s.setup = None;
        }
        s.resume.clear();
        if s.repo.is_none() {
            s.repo = repo_s;
        }
    })?;
    done.insert(0, "archived".into());
    Ok(done.join(" · "))
}

/// Thread ID back from the archive: open; its host starts and makes the worktree again (its
/// branch), the 1st turn waits for it.
pub fn unarchive(id: &str) -> Result<String, String> {
    let t = store::update(id, |t| {
        if t.state == "archived" {
            t.state = "open".into();
        }
    })?;
    match &t.worktree_pending {
        Some(b) => {
            client::ensure(&t.id)?;
            Ok(format!("unarchived · worktree on {b}: the host makes it"))
        }
        None => Ok("unarchived".into()),
    }
}

/// The owner's edit of a thread: title, repo (None: the repo of its dirs), linked repos (the
/// checkouts kept), agents (lead first).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Edit {
    pub title: String,
    pub repo: Option<String>,
    pub checkouts: Vec<String>,
    pub agents: Vec<String>,
}

/// Save edit E of thread ID. A live host: agents that leave / join go through it (their
/// sessions stop / start); else thread.json only. Ok: what changed.
pub fn edit(id: &str, e: &Edit) -> Result<String, String> {
    let t = store::load(id).ok_or_else(|| format!("no thread {id}"))?;
    let mut changed = Vec::new();
    let title = e.title.trim().to_string();
    if !title.is_empty() && title != t.title {
        changed.push("title");
    }
    let derived = repo_root(Path::new(&t.dir)).or_else(|| t.worktree.as_deref().and_then(|p| repo_root(Path::new(p)))).map(|p| p.display().to_string());
    // the pick of the repo of its dirs: 0 override
    let repo = e.repo.clone().filter(|r| !r.is_empty() && Some(r) != derived.as_ref());
    if repo != t.repo {
        changed.push("repo");
    }
    let keep: Vec<store::Checkout> = t.checkouts.iter().filter(|c| e.checkouts.contains(&c.dir)).cloned().collect();
    if keep.len() != t.checkouts.len() {
        changed.push("linked repos");
    }
    let gone: Vec<String> = t.agents.iter().filter(|a| !e.agents.contains(a)).cloned().collect();
    let new: Vec<String> = e.agents.iter().filter(|a| !t.agents.contains(a)).cloned().collect();
    if e.agents.is_empty() {
        return Err("1 agent at least".into());
    }
    let live = client::alive(&t.id);
    if live {
        for a in &gone {
            client::call(&t.id, &serde_json::json!({"op": "leave", "agent": a}))?;
        }
        for a in &new {
            client::call(&t.id, &serde_json::json!({"op": "join", "agent": a}))?;
        }
    }
    if !gone.is_empty() || !new.is_empty() || e.agents != t.agents {
        changed.push("agents");
    }
    store::update(&t.id, |s| {
        if !title.is_empty() {
            s.title = title.clone();
        }
        s.repo = repo.clone();
        s.checkouts = keep.clone();
        // the order of E (lead first); a join of a live host added its agent already
        s.agents = e.agents.clone();
    })?;
    Ok(if changed.is_empty() { "0 changes".into() } else { format!("saved: {}", changed.join(", ")) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A worktree thread: archive removes its worktree (uncommitted files → stash), keeps the
    /// branch, remembers it for the next host start; unarchive opens it again.
    #[test]
    fn archive_removes_the_worktree_keeps_the_branch() {
        let base = std::env::temp_dir().join(format!("ch-archive-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let sh = |c: &str| assert!(Command::new("sh").arg("-c").arg(c).current_dir(&base).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap().success(), "{c}");
        sh("git init -q -b main r && cd r && git -c user.email=t@t -c user.name=t commit -q --allow-empty -m 1 && git worktree add -q -b feat ../w && echo x > ../w/new.txt");
        let repo = base.join("r");
        let wt = base.join("w");
        let mut t = store::Thread::new("archive me", "task", &repo, vec!["ada".into()]);
        t.worktree = Some(wt.display().to_string());
        t.branch = Some("feat".into());
        t.port = Some(4100);
        t.save().unwrap();

        let out = archive(&t.id).unwrap();
        assert!(out.contains("worktree removed") && out.contains("git stash"), "{out}");
        assert!(!wt.exists());
        assert!(git(&repo, &["rev-parse", "--verify", "--quiet", "refs/heads/feat"]).0, "branch kept");
        let a = store::load(&t.id).unwrap();
        assert_eq!((a.state.as_str(), a.worktree.as_deref(), a.worktree_pending.as_deref(), a.port), ("archived", None, Some("feat"), None));
        assert_eq!(a.repo.as_deref().map(PathBuf::from).and_then(|p| std::fs::canonicalize(p).ok()), std::fs::canonicalize(&repo).ok());
        assert!(store::latest().is_none_or(|l| l.id != t.id), "plain codehead: 0 archived thread");

        // unarchive: open again; 0 host binary in tests, the record is what counts
        let _ = unarchive(&t.id);
        assert_eq!(store::load(&t.id).unwrap().state, "open");
        let _ = store::delete(&t.id);
        let _ = std::fs::remove_dir_all(base);
    }

    /// The edit: title, a repo override (the repo of its dir: 0 override), linked repos kept.
    #[test]
    fn edit_sets_title_repo_and_linked_repos() {
        let dir = std::env::temp_dir().join(format!("ch-edit-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut t = store::Thread::new("old", "task", &dir, vec!["ada".into()]);
        t.checkouts = vec![store::Checkout { dir: "/a".into(), branch: None }, store::Checkout { dir: "/b".into(), branch: None }];
        t.save().unwrap();
        let e = Edit { title: "new title".into(), repo: Some("/repos/other".into()), checkouts: vec!["/b".into()], agents: vec!["ada".into()] };
        let out = edit(&t.id, &e).unwrap();
        assert!(out.contains("title") && out.contains("repo") && out.contains("linked repos"), "{out}");
        let s = store::load(&t.id).unwrap();
        assert_eq!((s.title.as_str(), s.repo.as_deref(), s.checkouts.len()), ("new title", Some("/repos/other"), 1));
        let lite = Thread { repo: s.repo.clone(), dir: Some(s.dir.clone()), ..Default::default() };
        assert_eq!(repo_of_thread(&lite), Some(PathBuf::from("/repos/other")));
        assert!(edit(&t.id, &Edit { agents: Vec::new(), ..e }).is_err(), "0 agents refused");
        let _ = store::delete(&t.id);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_start_on_main_pulls_main_first_other_branches_not() {
        let base = std::env::temp_dir().join(format!("ch-update-main-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let sh = |c: &str| assert!(Command::new("sh").arg("-c").arg(c).current_dir(&base).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap().success(), "{c}");
        let cfg = "git config user.email t@t && git config user.name t";
        sh(&format!("git init -q --bare -b main origin.git && git clone -q origin.git a && cd a && {cfg} && git commit -q --allow-empty -m 1 && git push -q origin main"));
        sh(&format!("git clone -q origin.git b && cd b && {cfg} && git commit -q --allow-empty -m 2 && git push -q origin main"));
        let a = base.join("a");
        let head = |d: &Path| git(d, &["rev-parse", "HEAD"]).1;
        assert_ne!(head(&a), head(&base.join("b")));
        assert!(matches!(update_default(&a), Some(Ok(b)) if b == "main"));
        assert_eq!(head(&a), head(&base.join("b")), "main pulled");
        // another branch: 0 pull; a branch without upstream: 0 pull
        sh("cd a && git switch -q -c feat");
        assert!(update_default(&a).is_none());
        sh("cd a && git switch -q main && git branch -q --unset-upstream");
        assert!(update_default(&a).is_none());
        // diverged: Err with git's line
        sh("cd a && git branch -q -u origin/main && git commit -q --allow-empty -m local && cd ../b && git commit -q --allow-empty -m 3 && git push -q origin main");
        assert!(matches!(update_default(&a), Some(Err(e)) if !e.is_empty()));
        let _ = std::fs::remove_dir_all(base);
    }

    /// The file system read of the checkouts: the same map as `git worktree list`, from the main
    /// checkout and from a linked worktree (a detached worktree: 0 entry in both).
    #[test]
    fn checkouts_by_the_file_system_match_git() {
        let base = std::env::temp_dir().join(format!("codehead-checkouts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let sh = |c: &str| assert!(std::process::Command::new("sh").arg("-c").arg(c).current_dir(&base).output().unwrap().status.success(), "{c}");
        sh("git init -q -b main r && cd r && git -c user.email=t@t -c user.name=t commit -q --allow-empty -m 1 && git worktree add -q -b feat ../w && git worktree add -q --detach ../d");
        for dir in [base.join("r"), base.join("w"), base.join("r/sub")] {
            let _ = std::fs::create_dir_all(&dir);
            let fs = checkouts_fs(&dir).expect("fs read");
            let canon = |m: std::collections::HashMap<String, String>| m.into_iter().map(|(b, p)| (b, std::fs::canonicalize(p).unwrap())).collect::<std::collections::BTreeMap<_, _>>();
            assert_eq!(canon(fs.clone()), canon(checkouts_git(&dir)), "{}", dir.display());
            assert_eq!(fs.len(), 2);
        }
        assert!(checkouts_fs(&base).is_none(), "0 repo");
        let _ = std::fs::remove_dir_all(base);
    }
}
