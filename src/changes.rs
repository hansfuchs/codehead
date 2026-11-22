//! The changes view of a thread: the branch's changes against its base (commits since the
//! merge-base + staged + unstaged + untracked files), 1 review box per file, ordered by what to
//! review first (schema / API → logic → config → UI → docs → tests → generated). A box marked
//! reviewed folds; a reviewed file whose diff changed shows `updated`. Unified or side by side by
//! the room. Marks: `<thread dir>/review.json` (path → hash of the file's diff).

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::theme;

mod patch;
mod scm;
pub mod stash;

/// Side by side from this many columns.
pub const SIDE_MIN: u16 = 170;
/// Untracked files read as new-file diffs, at most.
const MAX_UNTRACKED: usize = 300;
/// An untracked file larger than this: shown as binary.
const MAX_FILE: u64 = 512 * 1024;
/// The view re-reads git this often while it is open.
const REFRESH: Duration = Duration::from_secs(4);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ctx,
    Add,
    Del,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DLine {
    pub kind: Kind,
    pub old: Option<u32>,
    pub new: Option<u32>,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hunk {
    /// The text after `@@ … @@` (the enclosing function, if git found one).
    pub ctx: String,
    pub old: u32,
    pub new: u32,
    pub lines: Vec<DLine>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Modified,
    New,
    Deleted,
    Renamed,
}

/// What a file is, in review order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Cat {
    Schema,
    Logic,
    Config,
    Ui,
    Docs,
    Tests,
    Generated,
}

impl Cat {
    pub fn label(self) -> &'static str {
        match self {
            Cat::Schema => "schema · API",
            Cat::Logic => "logic",
            Cat::Config => "config",
            Cat::Ui => "UI",
            Cat::Docs => "docs",
            Cat::Tests => "tests",
            Cat::Generated => "generated",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileDiff {
    pub path: String,
    pub old_path: Option<String>,
    pub status: Status,
    pub binary: bool,
    pub hunks: Vec<Hunk>,
    pub add: usize,
    pub del: usize,
    pub cat: Cat,
    /// Hash of the file's diff: a review mark holds while it stays.
    pub hash: u64,
}

impl FileDiff {
    fn new(path: String) -> FileDiff {
        FileDiff { cat: categorize(&path), path, old_path: None, status: Status::Modified, binary: false, hunks: Vec::new(), add: 0, del: 0, hash: 0 }
    }

    fn seal(&mut self) {
        self.add = self.hunks.iter().flat_map(|h| &h.lines).filter(|l| l.kind == Kind::Add).count();
        self.del = self.hunks.iter().flat_map(|h| &h.lines).filter(|l| l.kind == Kind::Del).count();
        self.cat = categorize(&self.path);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (&self.path, self.binary, self.status as u8).hash(&mut h);
        for l in self.hunks.iter().flat_map(|h| &h.lines).filter(|l| l.kind != Kind::Ctx) {
            (l.kind as u8, &l.text).hash(&mut h);
        }
        self.hash = h.finish();
    }

    /// The new-side line of the 1st change (deleted file: 1).
    pub fn first_line(&self) -> u32 {
        self.hunks.iter().flat_map(|h| &h.lines).find(|l| l.kind != Kind::Ctx).and_then(|l| l.new.or(l.old)).unwrap_or(1)
    }
}

/// Category of PATH by its name and dirs (heuristics; an agent ranking can replace them).
pub fn categorize(path: &str) -> Cat {
    let p = path.to_ascii_lowercase();
    let name = p.rsplit('/').next().unwrap_or(&p);
    let dirs: Vec<&str> = p.split('/').collect();
    let dirs = &dirs[..dirs.len().saturating_sub(1)];
    let ext = name.rsplit_once('.').map_or("", |(_, e)| e);
    let lock = ["cargo.lock", "package-lock.json", "pnpm-lock.yaml", "yarn.lock", "bun.lock", "bun.lockb", "poetry.lock", "go.sum", "gemfile.lock", "composer.lock", "uv.lock", "flake.lock"];
    if lock.contains(&name)
        || ext == "snap"
        || name.ends_with(".min.js")
        || name.ends_with(".min.css")
        || name.contains(".generated.")
        || name.ends_with(".pb.go")
        || dirs.iter().any(|d| ["__snapshots__", "dist", "generated", "__generated__", "vendor", "node_modules"].contains(d))
    {
        return Cat::Generated;
    }
    if dirs.iter().any(|d| ["test", "tests", "spec", "specs", "__tests__", "e2e", "fixtures", "testdata"].contains(d))
        || name.contains(".test.")
        || name.contains(".spec.")
        || name.contains("_test.")
        || name.starts_with("test_")
        || name.ends_with("_spec.rb")
    {
        return Cat::Tests;
    }
    if ["md", "mdx", "rst", "txt", "adoc"].contains(&ext) || dirs.contains(&"docs") || name.starts_with("license") || name.starts_with("changelog") {
        return Cat::Docs;
    }
    if ["sql", "proto", "graphql", "gql", "prisma"].contains(&ext)
        || dirs.iter().any(|d| d.contains("migration") || *d == "schema" || *d == "schemas")
        || name.contains("schema")
        || name.contains("openapi")
        || name.contains("swagger")
    {
        return Cat::Schema;
    }
    if ["toml", "yaml", "yml", "json", "jsonc", "ini", "cfg", "lock", "env", "example"].contains(&ext)
        || ["dockerfile", "makefile", "justfile", ".gitignore", ".editorconfig"].contains(&name)
        || name.contains(".config.")
        || dirs.first().is_some_and(|d| *d == ".github" || *d == "config" || *d == ".vscode")
    {
        return Cat::Config;
    }
    if ["css", "scss", "sass", "less", "html", "svelte", "vue", "astro", "svg"].contains(&ext)
        || dirs.iter().any(|d| ["components", "ui", "views", "pages", "styles", "templates", "public", "assets"].contains(d))
    {
        return Cat::Ui;
    }
    Cat::Logic
}

/// Files of the unified diff DIFF (`git diff`, any -U).
pub fn parse(diff: &str) -> Vec<FileDiff> {
    let mut out: Vec<FileDiff> = Vec::new();
    let mut hunk: Option<Hunk> = None;
    let (mut o, mut n) = (0u32, 0u32);
    let flush = |out: &mut Vec<FileDiff>, hunk: &mut Option<Hunk>| {
        if let (Some(h), Some(f)) = (hunk.take(), out.last_mut()) {
            f.hunks.push(h);
        }
    };
    for l in diff.lines() {
        if let Some(rest) = l.strip_prefix("diff --git ") {
            flush(&mut out, &mut hunk);
            // `a/x b/x`: the b side (with core.quotePath=false, 0 quotes for plain names)
            let path = rest.rsplit_once(" b/").map_or(rest, |(_, b)| b).trim_matches('"').to_string();
            out.push(FileDiff::new(path));
            continue;
        }
        let Some(f) = out.last_mut() else { continue };
        if hunk.is_none() || l.starts_with("@@ ") {
            if l.starts_with("new file mode") {
                f.status = Status::New;
            } else if l.starts_with("deleted file mode") {
                f.status = Status::Deleted;
            } else if let Some(p) = l.strip_prefix("rename from ") {
                f.status = Status::Renamed;
                f.old_path = Some(p.to_string());
            } else if let Some(p) = l.strip_prefix("rename to ") {
                f.path = p.to_string();
            } else if l.starts_with("Binary files ") || l.starts_with("GIT binary patch") {
                f.binary = true;
            } else if let Some(p) = l.strip_prefix("+++ ") {
                if p != "/dev/null" {
                    f.path = p.strip_prefix("b/").unwrap_or(p).to_string();
                }
            } else if let Some(h) = l.strip_prefix("@@ ") {
                flush(&mut out, &mut hunk);
                // `-a,b +c,d @@ ctx`
                let (ranges, ctx) = h.split_once(" @@").unwrap_or((h, ""));
                let num = |w: Option<&str>| w.and_then(|w| w[1..].split(',').next()).and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
                let mut ws = ranges.split(' ');
                o = num(ws.next());
                n = num(ws.next());
                hunk = Some(Hunk { ctx: ctx.trim().to_string(), old: o, new: n, lines: Vec::new() });
            }
            continue;
        }
        let Some(h) = hunk.as_mut() else { continue };
        let (kind, text) = match l.as_bytes().first() {
            Some(b'+') => (Kind::Add, &l[1..]),
            Some(b'-') => (Kind::Del, &l[1..]),
            Some(b' ') => (Kind::Ctx, &l[1..]),
            Some(b'\\') => continue,
            None => (Kind::Ctx, ""),
            _ => {
                // the next file's header lines (0 `diff --git` in some inputs)
                continue;
            }
        };
        let (lo, ln) = match kind {
            Kind::Ctx => (Some(o), Some(n)),
            Kind::Add => (None, Some(n)),
            Kind::Del => (Some(o), None),
        };
        if kind != Kind::Add {
            o += 1;
        }
        if kind != Kind::Del {
            n += 1;
        }
        h.lines.push(DLine { kind, old: lo, new: ln, text: text.replace('\t', "    ") });
    }
    flush(&mut out, &mut hunk);
    for f in &mut out {
        f.seal();
    }
    out
}

/// Review order: category, then the most changed lines first, then the path.
pub fn order(files: &mut [FileDiff]) {
    files.sort_by(|a, b| a.cat.cmp(&b.cat).then((b.add + b.del).cmp(&(a.add + a.del))).then(a.path.cmp(&b.path)));
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "core.quotePath=false"])
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("git failed").to_string())
    }
}

/// The base the branch gets compared with: its merge-base with the remote's default branch
/// (label: `origin/main`). On the default branch itself, or 0 remote: HEAD (uncommitted only).
fn base(dir: &Path) -> (String, String) {
    let head = || ("HEAD".to_string(), "HEAD".to_string());
    let def = git(dir, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| ["origin/main", "origin/master"].into_iter().find(|r| git(dir, &["rev-parse", "--verify", "--quiet", r]).is_ok()).map(String::from));
    let Some(def) = def else { return head() };
    let branch = git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    if def.ends_with(&format!("/{}", branch.trim())) {
        return head();
    }
    match git(dir, &["merge-base", "HEAD", &def]) {
        Ok(mb) if !mb.trim().is_empty() => (mb.trim().to_string(), def),
        _ => head(),
    }
}

/// A new-file diff of untracked file PATH in DIR.
fn untracked(dir: &Path, path: &str) -> FileDiff {
    let mut f = FileDiff::new(path.to_string());
    f.status = Status::New;
    let full = dir.join(path);
    let big = std::fs::metadata(&full).map(|m| m.len() > MAX_FILE).unwrap_or(true);
    match (big, std::fs::read(&full)) {
        (false, Ok(bytes)) if !bytes.contains(&0) => {
            let text = String::from_utf8_lossy(&bytes);
            let lines: Vec<DLine> = text.lines().enumerate().map(|(i, l)| DLine { kind: Kind::Add, old: None, new: Some(i as u32 + 1), text: l.replace('\t', "    ") }).collect();
            if !lines.is_empty() {
                f.hunks.push(Hunk { ctx: String::new(), old: 0, new: 1, lines });
            }
        }
        _ => f.binary = true,
    }
    f.seal();
    f
}

/// The changes of DIR's worktree against its base.
#[derive(Clone, Debug, Default)]
pub struct Changes {
    pub base: String,
    /// The base's commit (HEAD on the default branch).
    pub rev: String,
    /// Untracked paths (shown as new files).
    pub untracked: HashSet<String>,
    pub files: Vec<FileDiff>,
    /// Staged paths: true all of the file's uncommitted change, false a part.
    pub staged: HashMap<String, bool>,
    /// A rebase / merge stopped: its conflicted paths.
    pub stopped: Option<Vec<String>>,
    /// Commits ahead of / behind the base.
    pub ahead: usize,
    pub behind: usize,
}

fn z_paths(s: &str) -> Vec<String> {
    s.split('\0').filter(|p| !p.is_empty()).map(str::to_string).collect()
}

pub fn load(dir: &Path) -> Result<Changes, String> {
    git(dir, &["rev-parse", "--is-inside-work-tree"]).map_err(|_| "not a git worktree".to_string())?;
    let (rev, label) = base(dir);
    let diff = git(dir, &["diff", "-M", "--no-color", "--no-ext-diff", "-U3", &rev])?;
    let mut files = parse(&diff);
    let others = git(dir, &["ls-files", "--others", "--exclude-standard", "-z"]).unwrap_or_default();
    let mut fresh = HashSet::new();
    for p in others.split('\0').filter(|p| !p.is_empty()).take(MAX_UNTRACKED) {
        files.push(untracked(dir, p));
        fresh.insert(p.to_string());
    }
    order(&mut files);
    // staged: in the index diff; a part: also in the worktree diff
    let cached = z_paths(&git(dir, &["diff", "--cached", "--name-only", "-z"]).unwrap_or_default());
    let unstaged = z_paths(&git(dir, &["diff", "--name-only", "-z"]).unwrap_or_default());
    let staged = cached.into_iter().map(|p| (p.clone(), !unstaged.contains(&p))).collect();
    let in_progress = ["rebase-merge", "rebase-apply", "MERGE_HEAD"].iter().any(|p| git(dir, &["rev-parse", "--git-path", p]).is_ok_and(|g| {
        let g = PathBuf::from(g.trim());
        if g.is_absolute() { g.exists() } else { dir.join(g).exists() }
    }));
    let stopped = in_progress.then(|| z_paths(&git(dir, &["diff", "--name-only", "--diff-filter=U", "-z"]).unwrap_or_default()));
    let (ahead, behind) = if label == "HEAD" {
        (0, 0)
    } else {
        let lr = git(dir, &["rev-list", "--left-right", "--count", &format!("HEAD...{label}")]).unwrap_or_default();
        let mut it = lr.split_whitespace().map(|x| x.parse::<usize>().unwrap_or(0));
        (it.next().unwrap_or(0), it.next().unwrap_or(0))
    };
    Ok(Changes { base: label, rev, untracked: fresh, files, staged, stopped, ahead, behind })
}

/// The branch's pull request (gh): number, url, state, checks passed / failed / pending.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pr {
    pub number: u64,
    pub url: String,
    pub state: String,
    pub pass: usize,
    pub fail: usize,
    pub pending: usize,
}

/// The pull request of DIR's branch (gh pr view). None: 0 PR, 0 gh.
pub fn pr_state(dir: &Path) -> Option<Pr> {
    let out = Command::new("gh").current_dir(dir).args(["pr", "view", "--json", "number,url,state,statusCheckRollup"]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    parse_pr(&v)
}

fn parse_pr(v: &serde_json::Value) -> Option<Pr> {
    let mut pr = Pr { number: v["number"].as_u64()?, url: v["url"].as_str().unwrap_or_default().into(), state: v["state"].as_str().unwrap_or_default().into(), ..Default::default() };
    for c in v["statusCheckRollup"].as_array().into_iter().flatten() {
        // check runs: status + conclusion; status contexts: state
        let verdict = c["conclusion"].as_str().filter(|s| !s.is_empty()).or_else(|| c["state"].as_str()).unwrap_or_default().to_ascii_uppercase();
        match verdict.as_str() {
            "SUCCESS" | "NEUTRAL" | "SKIPPED" => pr.pass += 1,
            "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE" => pr.fail += 1,
            _ => pr.pending += 1,
        }
    }
    Some(pr)
}

/// The failed CI log of DIR's branch (the latest failed run, last 150 lines) as a message for
/// the lead.
pub fn ci_fix_message(dir: &Path) -> Result<String, String> {
    let gh = |args: &[&str]| -> Result<String, String> {
        let o = Command::new("gh").current_dir(dir).args(args).stdin(Stdio::null()).output().map_err(|e| format!("gh: {e}"))?;
        if o.status.success() { Ok(String::from_utf8_lossy(&o.stdout).into_owned()) } else { Err(String::from_utf8_lossy(&o.stderr).lines().next().unwrap_or("gh failed").to_string()) }
    };
    let branch = git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])?.trim().to_string();
    let runs: serde_json::Value = serde_json::from_str(&gh(&["run", "list", "--branch", &branch, "--limit", "10", "--json", "databaseId,conclusion,name,url"])?).map_err(|e| e.to_string())?;
    let run = runs.as_array().into_iter().flatten().find(|r| r["conclusion"] == "failure").ok_or("0 failed CI runs on this branch")?;
    let id = run["databaseId"].as_u64().ok_or("0 run id")?.to_string();
    let log = gh(&["run", "view", &id, "--log-failed"])?;
    let lines: Vec<&str> = log.lines().collect();
    let tail = lines[lines.len().saturating_sub(150)..].join("\n");
    Ok(format!(
        "CI failed on this branch: {} ({}). Find the cause, fix it, run the check locally if you can, commit and push. The failed log (last 150 lines):\n\n```text\n{tail}\n```",
        run["name"].as_str().unwrap_or("run"),
        run["url"].as_str().unwrap_or_default()
    ))
}

/// The patch of the hunk of PATH that holds new-side LINE: from the worktree-vs-index diff
/// (CACHED: the index-vs-HEAD diff, to unstage), -U0. Err: 0 such hunk.
pub fn hunk_patch(dir: &Path, path: &str, line: u32, cached: bool) -> Result<String, String> {
    let mut args = vec!["diff", "--no-color", "--no-ext-diff", "-U0"];
    if cached {
        args.push("--cached");
    }
    args.extend(["--", path]);
    let diff = git(dir, &args)?;
    pick_hunk(&diff, line).ok_or_else(|| format!("0 {} hunk at {path}:{line}", if cached { "staged" } else { "unstaged" }))
}

/// Header + the hunk of unified diff DIFF (1 file, -U0) whose new side holds LINE (a removal:
/// the line after it).
fn pick_hunk(diff: &str, line: u32) -> Option<String> {
    let mut head = String::new();
    let mut hunks: Vec<(u32, u32, String)> = Vec::new();
    for l in diff.lines() {
        if let Some(h) = l.strip_prefix("@@ ") {
            let new = h.split(' ').find(|w| w.starts_with('+'))?;
            let mut it = new[1..].split(',');
            let c: u32 = it.next()?.parse().ok()?;
            let d: u32 = it.next().and_then(|x| x.parse().ok()).unwrap_or(1);
            hunks.push((c, d, format!("{l}\n")));
        } else if let Some(h) = hunks.last_mut() {
            h.2.push_str(l);
            h.2.push('\n');
        } else {
            head.push_str(l);
            head.push('\n');
        }
    }
    let hit = hunks.into_iter().find(|(c, d, _)| if *d == 0 { line == *c || line == *c + 1 } else { line >= *c && line < c + d })?;
    Some(head + &hit.2)
}

/// A git step of the changes view (run on a thread; hooks, signing, ssh as the git CLI does).
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Stage(String),
    Unstage(String),
    Commit(String),
    Push,
    /// Fetch, then rebase on BASE (`origin/main`).
    Update(String),
    Continue,
    Abort,
    /// Stage / unstage the hunk of PATH that holds new-side LINE.
    StageHunk(String, u32),
    UnstageHunk(String, u32),
    /// Push, then `gh pr create --fill`.
    OpenPr,
    /// `gh pr merge --squash --auto` (merges when the checks pass).
    Merge,
    /// Stage / unstage these files.
    StageFiles(Vec<String>),
    UnstageFiles(Vec<String>),
    /// Stage the picked lines of each file (a part of its change).
    StageLines(Vec<(String, patch::Pick)>),
    /// Take the picked lines of each file out of the worktree into the stash of thread
    /// THREAD: (path, lines label, pick); the diff against REV.
    Stash { thread: String, rev: String, items: Vec<(String, String, patch::Pick)> },
}

impl Op {
    pub fn label(&self) -> String {
        match self {
            Op::Stage(p) => format!("staged {p}"),
            Op::Unstage(p) => format!("unstaged {p}"),
            Op::Commit(_) => "committed".into(),
            Op::Push => "pushed".into(),
            Op::Update(b) => format!("rebased on {b}"),
            Op::Continue => "rebase continued".into(),
            Op::Abort => "rebase aborted".into(),
            Op::StageHunk(p, l) => format!("staged the hunk at {p}:{l}"),
            Op::UnstageHunk(p, l) => format!("unstaged the hunk at {p}:{l}"),
            Op::OpenPr => "pull request opened".into(),
            Op::Merge => "merge on green set".into(),
            Op::StageFiles(p) => format!("staged {}", files_label(p)),
            Op::UnstageFiles(p) => format!("unstaged {}", files_label(p)),
            Op::StageLines(v) => format!("staged the selected lines of {}", files_label(&v.iter().map(|x| x.0.clone()).collect::<Vec<_>>())),
            Op::Stash { items, .. } => format!("stashed {}", files_label(&items.iter().map(|x| x.0.clone()).collect::<Vec<_>>())),
        }
    }
}

/// `src/a.rs` / `3 files`.
fn files_label(p: &[String]) -> String {
    if p.len() == 1 { p[0].clone() } else { format!("{} files", p.len()) }
}

/// `git ARGS` in DIR with STDIN as its input.
fn git_in(dir: &Path, args: &[&str], stdin: &str) -> Result<String, String> {
    use std::io::Write;
    let mut c = Command::new("git").arg("-C").arg(dir).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("git: {e}"))?;
    if let Some(mut i) = c.stdin.take() {
        let _ = i.write_all(stdin.as_bytes());
    }
    let out = c.wait_with_output().map_err(|e| format!("git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("git failed").to_string())
    }
}

/// The diff of PATH in DIR: against REV (None: the index) to the worktree. A file outside the
/// index (and outside REV): a new-file diff.
fn file_diff(dir: &Path, rev: Option<&str>, path: &str) -> Result<String, String> {
    let in_index = git(dir, &["ls-files", "--error-unmatch", "--", path]).is_ok();
    let in_rev = rev.is_some_and(|r| git(dir, &["cat-file", "-e", &format!("{r}:{path}")]).is_ok());
    if in_index || in_rev {
        let mut args = vec!["diff", "--no-color", "--no-ext-diff", "-U3"];
        args.extend(rev);
        args.extend(["--", path]);
        return git(dir, &args);
    }
    // `--no-index` exits 1 on a difference
    let out = Command::new("git").arg("-C").arg(dir).args(["diff", "--no-index", "--no-color", "--no-ext-diff", "--", "/dev/null", path]).stdin(Stdio::null()).output().map_err(|e| format!("git: {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// PICK (on the worktree's DIFF) on the index's diff CACHED: the picked lines that are staged, by
/// text, in order. None: 0 of them.
fn index_pick(diff: &str, cached: &str, pick: &patch::Pick) -> Option<patch::Pick> {
    let (Some(wt), Some(ix)) = (parse(diff).into_iter().next(), parse(cached).into_iter().next()) else { return None };
    if pick.all {
        return Some(patch::Pick { all: true, ..Default::default() });
    }
    // each change as (an add, new-side number at it, text), as `pick_of` numbers them
    let changes = |f: &FileDiff| -> Vec<(bool, u32, String)> {
        let mut v = Vec::new();
        for h in &f.hunks {
            let mut n = h.new;
            for l in &h.lines {
                match l.kind {
                    Kind::Add => {
                        v.push((true, n, l.text.clone()));
                        n += 1;
                    }
                    Kind::Ctx => n += 1,
                    Kind::Del => v.push((false, n, l.text.clone())),
                }
            }
        }
        v
    };
    let picked: Vec<(bool, String)> = changes(&wt).into_iter().filter(|(add, n, t)| if *add { pick.adds.contains(n) } else { pick.dels.contains(&(*n, t.clone())) }).map(|(a, _, t)| (a, t)).collect();
    let mut out = patch::Pick::default();
    let mut j = 0;
    for (add, n, t) in changes(&ix) {
        if let Some(k) = picked[j..].iter().position(|(a, x)| *a == add && *x == t) {
            j += k + 1;
            if add {
                out.adds.insert(n);
            } else {
                out.dels.insert((n, t));
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Stash ITEMS of DIR (diffs against REV) in THREAD's stash: each picked part out of the worktree
/// (and out of the index, where it is staged), its patch in a new entry. Ok: the entries' count.
fn stash_items(dir: &Path, thread: &str, rev: &str, items: &[(String, String, patch::Pick)]) -> Result<usize, String> {
    let mut made = Vec::new();
    for (path, lines, pick) in items {
        let diff = file_diff(dir, Some(rev), path)?;
        let Some(p) = patch::pick(&diff, pick, patch::Dir::Reverse) else { continue };
        // staged too: out of the index. The index has its own lines (0 unstaged one): the same
        // picks on its diff, else 0 match there
        let cached = git(dir, &["diff", "--cached", "--no-color", "--no-ext-diff", "-U3", rev, "--", path]).unwrap_or_default();
        let staged = index_pick(&diff, &cached, pick).and_then(|ip| patch::pick(&cached, &ip, patch::Dir::Reverse));
        git_in(dir, &["apply", "-R", "-"], &p).map_err(|e| format!("{path}: {e}"))?;
        if let Some(sp) = staged {
            git_in(dir, &["apply", "-R", "--cached", "-"], &sp).map_err(|e| format!("{path} (index): {e}"))?;
        }
        let agent = crate::engine::edits::writer(thread, dir, path);
        let id = format!("{}{}", stash::new_id(), made.len());
        made.push(stash::Entry { id, path: path.clone(), lines: lines.clone(), patch: p, agent, ts: crate::engine::now_ms(), ..Default::default() });
    }
    if made.is_empty() {
        return Err("0 changes to stash".into());
    }
    let n = made.len();
    let mut all = stash::read(thread);
    all.extend(made);
    stash::write(thread, &all);
    Ok(n)
}

/// Run OP in DIR: Ok(what happened) / Err(git's 1st error line).
pub fn run_op(dir: &Path, op: &Op) -> Result<String, String> {
    match op {
        Op::StageFiles(ps) => {
            let mut args = vec!["add", "-A", "--"];
            args.extend(ps.iter().map(String::as_str));
            git(dir, &args)
        }
        Op::UnstageFiles(ps) => {
            let mut args = vec!["restore", "--staged", "--"];
            args.extend(ps.iter().map(String::as_str));
            git(dir, &args).or_else(|_| {
                let mut args = vec!["rm", "--cached", "-q", "--"];
                args.extend(ps.iter().map(String::as_str));
                git(dir, &args)
            })
        }
        Op::StageLines(items) => {
            let mut n = 0;
            for (path, pick) in items {
                let diff = file_diff(dir, None, path)?;
                if let Some(p) = patch::pick(&diff, pick, patch::Dir::Forward) {
                    git_in(dir, &["apply", "--cached", "-"], &p).map_err(|e| format!("{path}: {e}"))?;
                    n += 1;
                }
            }
            if n == 0 { Err("0 unstaged changes in the selected lines".into()) } else { Ok(String::new()) }
        }
        Op::Stash { thread, rev, items } => stash_items(dir, thread, rev, items).map(|_| String::new()),
        Op::Stage(p) => git(dir, &["add", "-A", "--", p]),
        Op::Unstage(p) => git(dir, &["restore", "--staged", "--", p]).or_else(|_| git(dir, &["rm", "--cached", "-q", "--", p])),
        Op::Commit(msg) => {
            use std::io::Write;
            let mut c = Command::new("git").arg("-C").arg(dir).args(["commit", "-q", "-F", "-"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("git: {e}"))?;
            if let Some(mut i) = c.stdin.take() {
                let _ = i.write_all(msg.as_bytes());
            }
            let out = c.wait_with_output().map_err(|e| format!("git: {e}"))?;
            if out.status.success() {
                Ok(String::new())
            } else {
                let err = String::from_utf8_lossy(&out.stderr).into_owned() + &String::from_utf8_lossy(&out.stdout);
                Err(err.lines().find(|l| !l.trim().is_empty()).unwrap_or("commit failed").to_string())
            }
        }
        Op::Push => git(dir, &["push", "-u", "origin", "HEAD"]),
        Op::Update(base) => {
            // `origin/main`: fetch origin first; a local branch: 0 fetch
            let remote = base.split_once('/').map(|(r, _)| r).unwrap_or_default();
            if git(dir, &["remote"]).unwrap_or_default().lines().any(|r| r == remote) {
                git(dir, &["fetch", "-q", remote])?;
            }
            git(dir, &["-c", "core.editor=true", "rebase", base])
        }
        Op::Continue => git(dir, &["-c", "core.editor=true", "rebase", "--continue"]),
        Op::Abort => git(dir, &["rebase", "--abort"]),
        Op::StageHunk(p, l) | Op::UnstageHunk(p, l) => {
            use std::io::Write;
            let unstage = matches!(op, Op::UnstageHunk(..));
            let patch = hunk_patch(dir, p, *l, unstage)?;
            let mut args = vec!["apply", "--cached", "--unidiff-zero"];
            if unstage {
                args.push("-R");
            }
            args.push("-");
            let mut c = Command::new("git").arg("-C").arg(dir).args(&args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|e| format!("git: {e}"))?;
            if let Some(mut i) = c.stdin.take() {
                let _ = i.write_all(patch.as_bytes());
            }
            let out = c.wait_with_output().map_err(|e| format!("git: {e}"))?;
            if out.status.success() { Ok(String::new()) } else { Err(String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("git apply failed").to_string()) }
        }
        Op::OpenPr | Op::Merge => {
            if *op == Op::OpenPr {
                git(dir, &["push", "-u", "origin", "HEAD"])?;
            }
            let args: &[&str] = if *op == Op::OpenPr { &["pr", "create", "--fill"] } else { &["pr", "merge", "--squash", "--auto"] };
            let o = Command::new("gh").current_dir(dir).args(args).stdin(Stdio::null()).output().map_err(|e| format!("gh: {e}"))?;
            if o.status.success() {
                Ok(String::from_utf8_lossy(&o.stdout).into_owned())
            } else {
                Err(String::from_utf8_lossy(&o.stderr).lines().find(|l| !l.trim().is_empty()).unwrap_or("gh failed").to_string())
            }
        }
    }
    .map(|_| op.label())
}

/// A commit message for DIR's staged diff by MODEL (blocking): subject + body, as the repo's
/// recent subjects look. The owner edits it before the commit.
pub fn draft_message(dir: &Path, model: &str) -> Result<String, String> {
    let diff = git(dir, &["diff", "--cached", "--no-color", "--no-ext-diff", "-U2"])?;
    if diff.trim().is_empty() {
        return Err("0 staged changes".into());
    }
    let log = git(dir, &["log", "-8", "--format=%s"]).unwrap_or_default();
    let diff: String = diff.lines().take(1500).collect::<Vec<_>>().join("\n");
    let prompt = format!(
        "Write the git commit message for this staged diff.\n- Subject: max. 72 chars, imperative, the same style (prefixes like feat: / fix:) as these recent subjects:\n{log}\n- Then 1 blank line and a short body: what changed and why, as `- ` lines, max. 72 chars each.\n- Output only the message.\n\n```diff\n{diff}\n```"
    );
    let out = Command::new("claude")
        .current_dir(dir)
        .args(["-p", "--model", model, "--tools", "", "--strict-mcp-config", "--setting-sources", "", "--disable-slash-commands", "--no-session-persistence"])
        .args(["--system-prompt", "You write git commit messages. Output only the message: 0 notes, 0 fences, 0 extra text."])
        .args(["--output-format", "json"])
        .arg(prompt)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("claude: {e}"))?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("claude: {e}"))?;
    let text = v["result"].as_str().unwrap_or_default();
    if v["is_error"] == true {
        return Err(text.lines().next().unwrap_or("claude failed").to_string());
    }
    Ok(clean_message(text))
}

/// A model's commit message without fences and without a trailing note paragraph.
fn clean_message(text: &str) -> String {
    let t = text.trim().trim_start_matches("```").trim_end_matches("```").trim();
    let paras: Vec<&str> = t.split("\n\n").take_while(|p| !p.trim_start().starts_with("Note:") && !p.contains("doinstruct.tools")).collect();
    paras.join("\n\n").trim().to_string()
}

/// A diff line ready to draw: its chars in syntax colors, the chars of the changed words.
#[derive(Clone, Debug, Default, PartialEq)]
struct Painted {
    chars: Vec<(ratatui::style::Color, char)>,
    marks: Vec<bool>,
}

/// The diff text of F (its lines and hunk contexts): what the filter matches as content.
fn body_text(f: &FileDiff) -> String {
    let mut s = String::new();
    for h in &f.hunks {
        s.push_str(&h.ctx);
        s.push('\n');
        for l in &h.lines {
            s.push_str(&l.text);
            s.push('\n');
        }
    }
    s
}

/// Syntax token of PATH: its extension (`rs`, `ts` …), else `text`.
fn lang_of(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((_, e)) if !e.is_empty() => e,
        _ if name.eq_ignore_ascii_case("dockerfile") => "dockerfile",
        _ if name.eq_ignore_ascii_case("makefile") || name == "justfile" => "makefile",
        _ => "text",
    }
}

/// Word tokens of CHARS: a run of letters / digits / `_`, a run of spaces, else 1 char.
fn tokens(chars: &[char]) -> Vec<std::ops::Range<usize>> {
    let class = |c: char| if c.is_alphanumeric() || c == '_' { 0 } else if c.is_whitespace() { 1 } else { 2 };
    let mut out = Vec::new();
    let mut s = 0;
    while s < chars.len() {
        let k = class(chars[s]);
        let mut e = s + 1;
        if k != 2 {
            while e < chars.len() && class(chars[e]) == k {
                e += 1;
            }
        }
        out.push(s..e);
        s = e;
    }
    out
}

/// Changed chars of OLD and NEW (a removed line and the added line it pairs with): the tokens
/// outside their longest common token run. 0 marks when the lines share under 1/3 of their
/// tokens (a rewrite: the line tint says enough) or the lines are long.
pub fn word_marks(old: &[char], new: &[char]) -> (Vec<bool>, Vec<bool>) {
    let (to, tn) = (tokens(old), tokens(new));
    let none = || (vec![false; old.len()], vec![false; new.len()]);
    if to.len() > 300 || tn.len() > 300 {
        return none();
    }
    let eq = |a: &std::ops::Range<usize>, b: &std::ops::Range<usize>| old[a.clone()] == new[b.clone()];
    // LCS table over tokens
    let (n, m) = (to.len(), tn.len());
    let mut dp = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if eq(&to[i], &tn[j]) { dp[i + 1][j + 1] + 1 } else { dp[i + 1][j].max(dp[i][j + 1]) };
        }
    }
    let (mut keep_o, mut keep_n) = (vec![false; n], vec![false; m]);
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if eq(&to[i], &tn[j]) {
            keep_o[i] = true;
            keep_n[j] = true;
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    let solid = |t: &[std::ops::Range<usize>], c: &[char], k: &[bool]| -> (usize, usize) {
        let words: Vec<usize> = (0..t.len()).filter(|&x| !c[t[x].start].is_whitespace()).collect();
        (words.iter().filter(|&&x| k[x]).count(), words.len())
    };
    let (ko, no) = solid(&to, old, &keep_o);
    let (kn, nn) = solid(&tn, new, &keep_n);
    if (ko + kn) * 3 < no + nn {
        return none();
    }
    let mark = |t: &[std::ops::Range<usize>], len: usize, k: &[bool]| {
        let mut v = vec![false; len];
        for (x, r) in t.iter().enumerate() {
            if !k[x] {
                v[r.clone()].iter_mut().for_each(|b| *b = true);
            }
        }
        v
    };
    (mark(&to, old.len(), &keep_o), mark(&tn, new.len(), &keep_n))
}

/// The lines of hunk H of F painted: syntax colors (old side / new side highlighted apart), the
/// changed words of each removed line and the added line it pairs with.
fn paint(f: &FileDiff, h: &Hunk) -> Vec<Painted> {
    let lang = lang_of(&f.path);
    let side = |keep: Kind| -> crate::code::Lines {
        let text: Vec<&str> = h.lines.iter().filter(|l| l.kind != keep).map(|l| l.text.as_str()).collect();
        crate::code::highlight(&text.join("\n"), lang)
    };
    let (new_c, old_c) = (side(Kind::Del), side(Kind::Add));
    // the theme's colors, more saturated (a dozen colors per hunk: memo)
    let mut memo: HashMap<ratatui::style::Color, ratatui::style::Color> = HashMap::new();
    let mut vivid = |c: &Vec<(ratatui::style::Color, char)>| -> Vec<(ratatui::style::Color, char)> { c.iter().map(|&(col, ch)| (*memo.entry(col).or_insert_with(|| theme::desaturate(col, SATURATION)), ch)).collect() };
    let (mut io, mut inew) = (0, 0);
    let mut out: Vec<Painted> = h
        .lines
        .iter()
        .map(|l| {
            let src = match l.kind {
                Kind::Del => {
                    io += 1;
                    old_c.get(io - 1)
                }
                Kind::Add => {
                    inew += 1;
                    new_c.get(inew - 1)
                }
                Kind::Ctx => {
                    io += 1;
                    inew += 1;
                    new_c.get(inew - 1)
                }
            };
            let plain: Vec<char> = l.text.chars().collect();
            let chars = match src {
                Some(c) if c.len() == plain.len() => vivid(c),
                _ => plain.iter().map(|&c| (theme::pal().text, c)).collect(),
            };
            Painted { marks: vec![false; chars.len()], chars }
        })
        .collect();
    // pairs: a run of removed lines with the added lines right after it
    let ls = &h.lines;
    let mut k = 0;
    while k < ls.len() {
        if ls[k].kind != Kind::Del {
            k += 1;
            continue;
        }
        let d = ls[k..].iter().take_while(|l| l.kind == Kind::Del).count();
        let a = ls[k + d..].iter().take_while(|l| l.kind == Kind::Add).count();
        for j in 0..d.min(a) {
            let (o, n) = (k + j, k + d + j);
            let oc: Vec<char> = out[o].chars.iter().map(|x| x.1).collect();
            let nc: Vec<char> = out[n].chars.iter().map(|x| x.1).collect();
            let (mo, mn) = word_marks(&oc, &nc);
            out[o].marks = mo;
            out[n].marks = mn;
        }
        k += d + a;
    }
    out
}

/// Syntax colors: the theme's saturation times this.
const SATURATION: f32 = 1.45;

/// ` + ` / ` - ` / 3 spaces: the sign column, on the add / remove color (0 tint on the line).
fn sign_span(kind: Kind) -> Span<'static> {
    let (bg, _) = tints(kind);
    let sign = match kind {
        Kind::Add => "+",
        Kind::Del => "-",
        Kind::Ctx => " ",
    };
    let st = Style::default().fg(theme::pal().text).add_modifier(Modifier::BOLD);
    Span::styled(format!(" {sign} "), bg.map_or(st, |b| st.bg(b)))
}

/// Sign column color and changed-word tint of KIND (0 for context lines).
fn tints(kind: Kind) -> (Option<ratatui::style::Color>, Option<ratatui::style::Color>) {
    let base = theme::pal().panel;
    match kind {
        Kind::Add => (Some(theme::tint(theme::pal().ok, base, 0.7)), Some(theme::tint(theme::pal().ok, base, 0.32))),
        Kind::Del => (Some(theme::tint(theme::pal().danger, base, 0.7)), Some(theme::tint(theme::pal().danger, base, 0.32))),
        Kind::Ctx => (None, None),
    }
}

/// P as spans in ROOM columns (cut with `…`), padded to ROOM; changed words on their tint.
fn painted_spans(p: &Painted, kind: Kind, room: usize) -> Vec<Span<'static>> {
    let (_, mark) = tints(kind);
    let style = |color, marked: bool| {
        let s = Style::default().fg(color);
        match (marked, mark) {
            (true, Some(m)) => s.bg(m),
            _ => s,
        }
    };
    let total: usize = p.chars.iter().map(|x| unicode_width::UnicodeWidthChar::width(x.1).unwrap_or(0)).sum();
    let limit = if total > room { room.saturating_sub(1) } else { room };
    let mut out: Vec<Span<'static>> = Vec::new();
    let (mut run, mut key, mut w) = (String::new(), None, 0);
    for (i, &(color, c)) in p.chars.iter().enumerate() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cw > limit {
            break;
        }
        let k = (color, p.marks.get(i).copied().unwrap_or(false));
        if key != Some(k) && !run.is_empty() {
            let (c0, m0) = key.unwrap_or(k);
            out.push(Span::styled(std::mem::take(&mut run), style(c0, m0)));
        }
        key = Some(k);
        run.push(c);
        w += cw;
    }
    if let Some((c0, m0)) = key
        && !run.is_empty()
    {
        out.push(Span::styled(run, style(c0, m0)));
    }
    if total > room {
        out.push(Span::styled("…", style(theme::pal().muted, false)));
        w += 1;
    }
    out.push(Span::raw(" ".repeat(room.saturating_sub(w))));
    out
}

/// Review marks of thread ID: path → diff hash at the mark.
fn marks_path(id: &str) -> PathBuf {
    crate::engine::store::thread_dir(id).join("review.json")
}

fn read_marks(id: &str) -> HashMap<String, u64> {
    std::fs::read(marks_path(id)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write_marks(id: &str, m: &HashMap<String, u64>) {
    if let Ok(b) = serde_json::to_vec_pretty(m) {
        let _ = crate::engine::write_atomic(&marks_path(id), &b);
    }
}

/// What a key or click asks the thread view to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    None,
    /// Open FILE (relative to the worktree) at LINE in the IDE.
    Open(String, u32),
    Toast(String),
    /// Send this text to the thread (the review comments batch).
    Send(String),
    /// The chat above the prompt: open / folded.
    ToggleChat,
}

/// Lines of 1 file a comment is on: LINE (new side) shows it; LINES (`11-13, 15 · removed 14`)
/// names a selection ("": LINE alone).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spot {
    pub path: String,
    pub line: u32,
    pub lines: String,
}

impl Spot {
    /// `src/a.rs:11-13, 15`.
    pub fn label(&self) -> String {
        format!("{}:{}", self.path, if self.lines.is_empty() { self.line.to_string() } else { self.lines.clone() })
    }
}

/// What a row shows; its line is made when it comes into view.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Body {
    Conflict,
    Blank,
    /// A group head: the model's group, else the category.
    Group(Option<usize>, Cat),
    /// The filter's result count.
    Hits(usize),
    Head,
    Foot,
    Why,
    Note(&'static str),
    Hunk(usize),
    /// Unified: hunk, line.
    Diff(usize, usize),
    /// Side by side: hunk, the removed (or context) line, the added (or context) line.
    Side(usize, Option<usize>, Option<usize>),
    /// Finding I, its row K (0: the title).
    Finding(usize, usize),
    /// Comment I, its text line K.
    Comment(usize, usize),
    /// GitHub thread I of the pull request (PR mode), its line K.
    Gh(usize, usize),
}

/// A row: what it shows, the file it belongs to, the new-side line it points at.
#[derive(Clone, Debug)]
struct Row {
    body: Body,
    file: usize,
    at: Option<u32>,
    /// A box's header row.
    head: bool,
    /// A hunk's 1st row.
    hunk: bool,
    /// A review comment's row: its index.
    comment: Option<usize>,
    /// A reviewer finding's row: its index.
    finding: Option<usize>,
}

impl Row {
    fn new(body: Body, file: usize, at: Option<u32>) -> Row {
        let (comment, finding) = match body {
            Body::Comment(c, _) => (Some(c), None),
            Body::Finding(x, _) => (None, Some(x)),
            _ => (None, None),
        };
        Row { body, file, at, head: body == Body::Head, hunk: matches!(body, Body::Hunk(_)), comment, finding }
    }
}

/// A diff line of a file, for the selection: (old side, its number); added and context lines:
/// the new side.
type LKey = (bool, u32);

fn lkey(l: &DLine) -> Option<LKey> {
    match l.kind {
        Kind::Del => l.old.map(|n| (true, n)),
        _ => l.new.map(|n| (false, n)),
    }
}

/// `11-13, 15 · removed 14` of KEYS (sorted).
fn lines_label(keys: &std::collections::BTreeSet<LKey>) -> String {
    let runs = |nums: Vec<u32>| -> String {
        let mut out: Vec<String> = Vec::new();
        let mut i = 0;
        while i < nums.len() {
            let mut j = i;
            while j + 1 < nums.len() && nums[j + 1] == nums[j] + 1 {
                j += 1;
            }
            out.push(if j > i { format!("{}-{}", nums[i], nums[j]) } else { nums[i].to_string() });
            i = j + 1;
        }
        out.join(", ")
    };
    let new: Vec<u32> = keys.iter().filter(|k| !k.0).map(|k| k.1).collect();
    let old: Vec<u32> = keys.iter().filter(|k| k.0).map(|k| k.1).collect();
    match (new.is_empty(), old.is_empty()) {
        (_, true) => runs(new),
        (true, false) => format!("removed {}", runs(old)),
        (false, false) => format!("{} · removed {}", runs(new), runs(old)),
    }
}

/// A review comment on LINE (new side) of PATH (LINES: the selected lines it is on), for the
/// batch to an agent.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Comment {
    pub path: String,
    pub line: u32,
    pub text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub lines: String,
    /// The diff lines it is on, as the modal showed them (`+ let b = 3;`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub quote: String,
    /// Its severity (the panel groups the comments by it).
    #[serde(default)]
    pub level: crate::rank::Level,
    /// The agent that wrote it ("": the owner).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub by: String,
}

impl Comment {
    /// `11` or `11-13, 15`.
    fn at(&self) -> String {
        if self.lines.is_empty() { self.line.to_string() } else { self.lines.clone() }
    }
}

fn comments_path(id: &str) -> PathBuf {
    crate::engine::store::thread_dir(id).join("comments.json")
}

pub(crate) fn read_comments(id: &str) -> Vec<Comment> {
    std::fs::read(comments_path(id)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// The comments file's time (an agent wrote a comment: the view reads it again).
fn comments_stamp(id: &str) -> Option<std::time::SystemTime> {
    std::fs::metadata(comments_path(id)).and_then(|m| m.modified()).ok()
}

pub(crate) fn write_comments(id: &str, c: &[Comment]) {
    if let Ok(b) = serde_json::to_vec_pretty(c) {
        let _ = crate::engine::write_atomic(&comments_path(id), &b);
    }
}

/// MCP `comment`: AGENT's comments (`[{file, line, lines?, severity, text}]`) into the comments
/// of THREAD (the changes view reads them at its next draw). ROOT: the worktree (absolute paths
/// in it: relative). Returns the count added.
pub fn agent_comments(thread: &str, agent: &str, root: Option<&Path>, list: &serde_json::Value) -> Result<usize, String> {
    let items = list.as_array().ok_or("comments: an array required")?;
    let mut add = Vec::new();
    for c in items {
        let file = c["file"].as_str().unwrap_or_default().trim();
        let text = c["text"].as_str().unwrap_or_default().trim();
        let line = c["line"].as_u64().unwrap_or(0) as u32;
        if file.is_empty() || text.is_empty() {
            return Err("each comment needs file and text".into());
        }
        let p = Path::new(file);
        let rel = root.and_then(|r| p.strip_prefix(r).ok()).unwrap_or(p).to_string_lossy().trim_start_matches("./").to_string();
        let level = crate::rank::Level::parse(c["severity"].as_str().unwrap_or_default()).unwrap_or_default();
        add.push(Comment { path: rel, line, text: text.to_string(), lines: c["lines"].as_str().unwrap_or_default().to_string(), quote: String::new(), level, by: agent.to_string() });
    }
    let n = add.len();
    let mut all = read_comments(thread);
    all.extend(add);
    write_comments(thread, &all);
    Ok(n)
}

/// The batch message of COMMENTS (0 comments: None).
pub fn batch(comments: &[Comment]) -> Option<String> {
    if comments.is_empty() {
        return None;
    }
    let mut s = String::from("Review comments on the changes (file:line, new side). Address each one:\n");
    for c in comments {
        let mut lines = c.text.lines();
        let by = if c.by.is_empty() { String::new() } else { format!(" (by {})", c.by) };
        s.push_str(&format!("- [{}] `{}:{}`{by}: {}\n", c.level.label(), c.path, c.at(), lines.next().unwrap_or_default()));
        for l in lines {
            s.push_str(&format!("  {l}\n"));
        }
    }
    Some(s.trim_end().to_string())
}

pub struct View {
    /// Data version: up with each read that brings new diffs (they dissolve in).
    version: u64,
    fx: crate::dissolve::Dissolve,
    /// An alt press with 0 key after it yet: its release toggles the focus.
    alt_tap: bool,
    /// The last esc in the comment modal: a 2nd within 2 s drops an edited comment.
    esc_at: Option<std::time::Instant>,
    id: String,
    dir: PathBuf,
    pub changes: Changes,
    marks: HashMap<String, u64>,
    /// Boxes opened by hand (reviewed / generated: folded by default) or closed by hand.
    toggled: HashSet<String>,
    /// Side by side: None: by the room.
    side: Option<bool>,
    /// The rows of the shown files (cheap: a row's line is made when it comes into view).
    rows: Vec<Row>,
    built: Option<(u16, bool)>,
    /// Lines of the diff rows, per (file hash, hunk): syntax colors and word marks, made once.
    paints: std::cell::RefCell<HashMap<(u64, usize), std::rc::Rc<Vec<Painted>>>>,
    /// Width of the line numbers per file (by index), for the shown files.
    num_w: HashMap<usize, usize>,
    /// Cursor row, 1st row in view.
    at: usize,
    top: usize,
    loading: Option<Receiver<Result<Changes, String>>>,
    loaded: Option<Instant>,
    pub error: Option<String>,
    /// The rows' area; the summary row and the filter row above it (0 height: too little room).
    area: Rect,
    summary_rect: Rect,
    filter_rect: Rect,
    /// After a re-read: the file the cursor was on, its row in the file's rows.
    pending_cursor: Option<(String, usize)>,
    /// The agent's order (groups, whys, levels) of the current set of paths; a call on its way;
    /// why 0; the rank holds for the current paths (set by `build`).
    rank: Option<crate::rank::Rank>,
    ranking: Option<Receiver<Result<crate::rank::Rank, String>>>,
    rank_error: Option<String>,
    fresh_rank: bool,
    /// The classifier (setting `classify`), None: path rules.
    classifier: Option<crate::rank::Runner>,
    /// The level of each file (by index), reviewed / all files per level (by `build`).
    lv: Vec<crate::rank::Level>,
    counts: [(usize, usize); 4],
    /// The filter: its text, it has the keys, its hits (file index, name marks; by `build`).
    filter: String,
    /// The filter's input (cursor, selection); FILTER: its text.
    filter_ta: crate::textarea::TextArea,
    filtering: bool,
    hits: HashMap<usize, Vec<usize>>,
    /// The selected diff lines per path (space, a mouse drag): a comment goes on them.
    sel: HashMap<String, std::collections::BTreeSet<LKey>>,
    /// A mouse drag: its 1st row, the selection before it, adds (left button) or takes out
    /// (right button).
    drag: Option<(usize, HashMap<String, std::collections::BTreeSet<LKey>>, bool)>,
    /// The last right click (a 2nd one soon after: the selection clears); the last left click's
    /// row (shift+click: the rows from it).
    right_at: Option<Instant>,
    anchor: Option<usize>,
    /// The source control panel (ctrl+b).
    scm: scm::Panel,
    /// A modal over the view: a comment, a question; its buttons (yes / save: true).
    modal: Option<Modal>,
    modal_btns: Vec<(Rect, bool)>,
    /// The comment modal's selected lines on screen: the wheel scrolls them.
    modal_quote: Rect,
    /// The row index on each screen row of the rows' area (pinned rows included).
    shown_rows: Vec<usize>,
    /// The stash (read again when its file changes: an agent named an entry).
    stash: Vec<stash::Entry>,
    stash_at: Option<std::time::SystemTime>,
    /// Review comments, for 1 batch message (`C`).
    comments: Vec<Comment>,
    comments_at: Option<std::time::SystemTime>,
    /// The reviewer's findings, a review on its way, why 0, the model (None: `off`).
    review: Option<crate::review::Review>,
    reviewing: Option<Receiver<Result<crate::review::Review, String>>>,
    review_error: Option<String>,
    review_model: Option<String>,
    /// The branch's pull request (read every 30 s while the view is open), the read on its way.
    pr: Option<Pr>,
    pr_read: Option<Receiver<Option<Pr>>>,
    pr_at: Option<Instant>,
    /// The CI fix message on its way; messages for the thread (the view sends them; `@name`
    /// first: to that agent).
    ci: Option<Receiver<Result<String, String>>>,
    outbox: Vec<String>,
    /// A git step on its way, and what the last one did (the thread view shows it).
    op: Option<Receiver<Result<String, String>>>,
    note: Option<String>,
    /// A pull request's diff (the merge tab): read only, 0 git steps.
    pub pr_src: Option<PrSrc>,
}

/// The pull request of a PR-mode view: its commits (the panel's picker), the picked 1 (None:
/// every commit, against the merge base), its GitHub threads (under their lines).
#[derive(Clone, Debug, Default)]
pub struct PrSrc {
    pub pr: crate::gh::PrRef,
    pub base: String,
    /// The head commit of the last read.
    pub head: String,
    pub commits: Vec<crate::gh::Commit>,
    pub commit: Option<String>,
    pub threads: Vec<crate::gh::Thread>,
}

/// The lines of GitHub thread T (head: the author row) in W columns: the 1st comment, then
/// its replies, 6 text lines each at most.
fn gh_lines(t: &crate::gh::Thread, w: usize) -> Vec<(bool, String)> {
    let mut out = Vec::new();
    for (k, c) in std::iter::once(&t.root).chain(&t.replies).enumerate() {
        let mark = if k == 0 { "◈" } else { "  ↳" };
        let state = if k == 0 && t.resolved { " · resolved" } else if k == 0 && t.outdated { " · outdated" } else { "" };
        out.push((true, format!("{mark} {} · {}{state}", c.author, crate::gh::stamp(c.created))));
        let indent = if k == 0 { "   " } else { "     " };
        let body: Vec<&str> = c.body.lines().filter(|l| !l.trim().is_empty()).collect();
        for l in body.iter().take(6) {
            out.push((false, format!("{indent}{}", crate::nav::fit(l, w.saturating_sub(indent.len())))));
        }
        if body.len() > 6 {
            out.push((false, format!("{indent}… {} more lines", body.len() - 6)));
        }
    }
    out
}

/// The diff of pull request SRC (its picked commit, else all) as changes.
fn load_pr(src: &PrSrc) -> Result<Changes, String> {
    let text = crate::gh::diff(&src.pr, src.commit.as_deref())?;
    let mut files = parse(&text);
    order(&mut files);
    let base = match &src.commit {
        Some(c) => format!("{}^", &c[..c.len().min(7)]),
        None => src.base.clone(),
    };
    Ok(Changes { base, files, ..Default::default() })
}

impl View {
    /// The changes of pull request PR (DIR: a checkout of its repo, else any dir).
    pub fn for_pr(pr: &crate::gh::PrRef, dir: &Path) -> View {
        let mut v = View::new(&format!("pr~{}", pr.key().replace('/', "~")), dir);
        v.pr_src = Some(PrSrc { pr: pr.clone(), ..Default::default() });
        v
    }

    /// PR mode: the pull request's base, commits, threads (a new commit list: the picker keeps
    /// its pick while it is still there).
    pub fn set_pr(&mut self, p: &crate::gh::Pr) {
        let Some(src) = self.pr_src.as_mut() else { return };
        let threads: Vec<crate::gh::Thread> = p.threads.iter().filter(|t| t.path.is_some()).cloned().collect();
        let changed = src.threads != threads;
        src.base = p.base.clone();
        src.commits = p.commits.clone();
        src.threads = threads;
        if src.commit.as_ref().is_some_and(|c| !p.commits.iter().any(|x| &x.oid == c)) {
            src.commit = None;
        }
        let pushed = !src.head.is_empty() && src.head != p.head_oid;
        src.head = p.head_oid.clone();
        if changed {
            self.built = None;
        }
        if pushed {
            self.refresh(true);
        }
    }

    /// Tests: the cursor on file I's box.
    #[cfg(test)]
    pub fn jump_to_file(&mut self, i: usize) {
        self.jump_file(i);
    }

    /// PR mode: show COMMIT's diff (None: all), read now.
    pub fn pick_commit(&mut self, commit: Option<String>) {
        if let Some(src) = self.pr_src.as_mut()
            && src.commit != commit
        {
            src.commit = commit;
            self.loading = None;
            self.refresh(true);
        }
    }

    /// The checkout the view reads.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn new(id: &str, dir: &Path) -> View {
        View {
            version: 0,
            fx: Default::default(),
            id: id.to_string(),
            dir: dir.to_path_buf(),
            changes: Changes::default(),
            marks: read_marks(id),
            toggled: HashSet::new(),
            side: None,
            rows: Vec::new(),
            built: None,
            paints: Default::default(),
            num_w: HashMap::new(),
            at: 0,
            top: 0,
            loading: None,
            loaded: None,
            error: None,
            area: Rect::default(),
            summary_rect: Rect::default(),
            filter_rect: Rect::default(),
            pending_cursor: None,
            rank: crate::rank::cached(id),
            ranking: None,
            rank_error: None,
            fresh_rank: false,
            classifier: if cfg!(test) { None } else { crate::rank::runner() },
            lv: Vec::new(),
            counts: [(0, 0); 4],
            filter: String::new(),
            filter_ta: crate::textarea::TextArea::new(true, ""),
            filtering: false,
            hits: HashMap::new(),
            sel: HashMap::new(),
            drag: None,
            right_at: None,
            anchor: None,
            scm: scm::Panel::new(),
            modal: None,
            alt_tap: false,
            esc_at: None,
            modal_btns: Vec::new(),
            modal_quote: Rect::default(),
            shown_rows: Vec::new(),
            stash: stash::read(id),
            stash_at: stash::stamp(id),
            comments: read_comments(id),
            comments_at: comments_stamp(id),
            review: crate::review::cached(id),
            reviewing: None,
            review_error: None,
            review_model: if cfg!(test) { None } else { crate::config::review_model() },
            pr: None,
            pr_read: None,
            pr_at: None,
            ci: None,
            outbox: Vec::new(),
            op: None,
            note: None,
            pr_src: None,
        }
    }

    /// The files in the model's order (its groups first, the rest after in path-rule order);
    /// a rank of another set of paths: a new call.
    fn apply_rank(&mut self, force: bool) {
        let key = crate::rank::key(&self.changes.files);
        // a rank of an older codehead (0 levels): a new call too
        let fresh = self.rank.as_ref().is_some_and(|r| r.key == key && r.groups.iter().flat_map(|g| &g.files).any(|f| !f.level.is_empty()));
        if (force || !fresh) && self.ranking.is_none() && !self.changes.files.is_empty()
            && let Some(by) = self.classifier.clone()
        {
            let (tx, rx) = mpsc::channel();
            let (id, dir, files) = (self.id.clone(), self.dir.clone(), self.changes.files.clone());
            std::thread::spawn(move || {
                let _ = tx.send(crate::rank::run(&id, &dir, &files, &by));
            });
            self.ranking = Some(rx);
            self.rank_error = None;
        }
        let Some(r) = self.rank.as_ref().filter(|r| r.key == key) else { return };
        let pos: HashMap<&str, (usize, usize)> = r.groups.iter().enumerate().flat_map(|(g, gr)| gr.files.iter().enumerate().map(move |(k, f)| (f.path.as_str(), (g, k)))).collect();
        let pos: HashMap<String, (usize, usize)> = pos.into_iter().map(|(p, v)| (p.to_string(), v)).collect();
        // a stable sort: unranked files keep the path-rule order, after the groups
        self.changes.files.sort_by_key(|f| pos.get(&f.path).copied().unwrap_or((usize::MAX, 0)));
        self.built = None;
    }

    /// The model's group of PATH and the why of PATH, if the rank is current (as of `build`).
    fn ranked(&self, path: &str) -> Option<(usize, &crate::rank::Group, &str)> {
        let r = self.rank.as_ref().filter(|_| self.fresh_rank)?;
        r.groups.iter().enumerate().find_map(|(g, gr)| gr.files.iter().find(|f| f.path == path).map(|f| (g, gr, f.why.as_str())))
    }

    /// Level of each file (by index): the agent's (an older rank too), else path rules.
    fn levels(&self) -> Vec<crate::rank::Level> {
        let by: HashMap<&str, crate::rank::Level> =
            self.rank.iter().flat_map(|r| &r.groups).flat_map(|g| &g.files).filter_map(|f| crate::rank::Level::parse(&f.level).map(|l| (f.path.as_str(), l))).collect();
        self.changes.files.iter().map(|f| by.get(f.path.as_str()).copied().unwrap_or_else(|| crate::rank::guess(f))).collect()
    }

    /// The cursor's file and its row in that file's rows (0: the head).
    fn cursor_spot(&self) -> Option<(String, usize)> {
        let r = self.rows.get(self.at)?;
        let head = self.rows[..=self.at].iter().rposition(|x| x.head && x.file == r.file)?;
        Some((self.changes.files.get(r.file)?.path.clone(), self.at - head))
    }

    /// A finished rank call applies. True: the order changed.
    fn poll_rank(&mut self) -> bool {
        let Some(rx) = &self.ranking else { return false };
        let Ok(r) = rx.try_recv() else { return false };
        self.ranking = None;
        match r {
            Ok(r) => {
                let cur = self.cursor_spot();
                self.rank = Some(r);
                self.apply_rank(false);
                self.pending_cursor = cur;
            }
            Err(e) => self.rank_error = Some(e),
        }
        true
    }

    /// The row above the boxes: files, lines, base, the order's source.
    fn summary(&self) -> Line<'static> {
        let (add, del) = self.changes.files.iter().fold((0, 0), |(a, d), f| (a + f.add, d + f.del));
        let (done, all) = self.progress();
        let order = if self.ranking.is_some() {
            "classifying…".to_string()
        } else if let Some(e) = &self.rank_error {
            format!("path order · {e}")
        } else if let Some(r) = self.rank.as_ref().filter(|_| self.fresh_rank) {
            format!("order: {}", r.model)
        } else {
            "path order".to_string()
        };
        Line::from(vec![
            Span::styled(format!(" {all} files · "), theme::muted()),
            Span::styled(format!("+{add}"), Style::default().fg(theme::pal().ok)),
            Span::styled(format!(" −{del}"), Style::default().fg(theme::pal().danger)),
            Span::styled(format!(" · {done}/{all} reviewed · against {} · {order}", self.changes.base), theme::muted()),
            Span::styled(self.git_state(), theme::accent()),
        ])
    }

    /// ` · 2 staged · ↑1 ↓3 · git…`: the git side of the summary.
    fn git_state(&self) -> String {
        let mut s = String::new();
        let n = self.changes.staged.len();
        if n > 0 {
            s.push_str(&format!(" · {n} staged"));
        }
        if self.changes.ahead + self.changes.behind > 0 {
            s.push_str(&format!(" · ↑{} ↓{}", self.changes.ahead, self.changes.behind));
        }
        if self.op.is_some() {
            s.push_str(" · git…");
        }
        if let Some(p) = &self.pr {
            s.push_str(&format!(" · PR #{} {}", p.number, p.state.to_lowercase()));
            if p.pass + p.fail + p.pending > 0 {
                s.push_str(&format!(" ✓{} ✗{} ●{}", p.pass, p.fail, p.pending));
            }
        }
        if self.reviewing.is_some() {
            s.push_str(" · reviewing…");
        } else if let Some(e) = &self.review_error {
            s.push_str(&format!(" · {e}"));
        } else if let Some(r) = self.review.as_ref().filter(|r| !r.findings.is_empty()) {
            s.push_str(&format!(" · {} findings ({})", r.findings.len(), r.model));
        }
        s
    }

    /// V: the reviewer reads the diff (on a thread; minutes for a big diff).
    fn start_review(&mut self) -> Act {
        let Some(model) = self.review_model.clone() else { return Act::Toast("review: off (setting review)".into()) };
        if self.reviewing.is_some() {
            return Act::Toast("review: on its way".into());
        }
        let (tx, rx) = mpsc::channel();
        let (id, dir, files) = (self.id.clone(), self.dir.clone(), self.changes.files.clone());
        std::thread::spawn(move || {
            let _ = tx.send(crate::review::run(&id, &dir, &files, &model));
        });
        self.reviewing = Some(rx);
        self.review_error = None;
        self.built = None;
        Act::Toast("review: on its way".into())
    }

    fn poll_review(&mut self) -> bool {
        let Some(rx) = &self.reviewing else { return false };
        let Ok(r) = rx.try_recv() else { return false };
        self.reviewing = None;
        match r {
            Ok(r) => {
                self.note = Some(format!("review: {} findings", r.findings.len()));
                self.review = Some(r);
            }
            Err(e) => self.review_error = Some(e),
        }
        true
    }

    /// Drop finding I (dismissed or taken into the batch).
    fn drop_finding(&mut self, i: usize) {
        if let Some(r) = self.review.as_mut()
            && i < r.findings.len()
        {
            r.findings.remove(i);
            crate::review::store(&self.id, r);
        }
    }

    /// Run git step OP on a thread; its end re-reads git.
    pub fn start_op(&mut self, op: Op) -> Act {
        if self.pr_src.is_some() {
            return Act::Toast("a pull request's diff: read only (the merge tab's prompt tells the agents)".into());
        }
        if self.op.is_some() {
            return Act::Toast("git: 1 step at a time".into());
        }
        let (tx, rx) = mpsc::channel();
        let dir = self.dir.clone();
        let label = op.label();
        std::thread::spawn(move || {
            let _ = tx.send(run_op(&dir, &op));
        });
        self.op = Some(rx);
        self.built = None;
        Act::Toast(format!("git … {label}"))
    }

    /// What the last git step did, once (the thread view toasts it).
    pub fn take_note(&mut self) -> Option<String> {
        self.note.take()
    }

    /// A message for the thread, once (the CI fix; the thread view sends it).
    pub fn take_send(&mut self) -> Option<String> {
        (!self.outbox.is_empty()).then(|| self.outbox.remove(0))
    }

    /// The PR read (30 s) and the CI fix message: finished ones apply. True: something changed.
    fn poll_ship(&mut self) -> bool {
        let mut changed = false;
        if let Some(rx) = &self.pr_read
            && let Ok(p) = rx.try_recv()
        {
            self.pr_read = None;
            changed = self.pr != p;
            self.pr = p;
        }
        let on_branch = !cfg!(test) && self.pr_src.is_none() && self.changes.base != "HEAD" && !self.changes.base.is_empty();
        if on_branch && self.pr_read.is_none() && self.pr_at.is_none_or(|t| t.elapsed() > Duration::from_secs(30)) {
            self.pr_at = Some(Instant::now());
            let (tx, rx) = mpsc::channel();
            let dir = self.dir.clone();
            std::thread::spawn(move || {
                let _ = tx.send(pr_state(&dir));
            });
            self.pr_read = Some(rx);
        }
        if let Some(rx) = &self.ci
            && let Ok(r) = rx.try_recv()
        {
            self.ci = None;
            match r {
                Ok(m) => self.outbox.push(m),
                Err(e) => self.note = Some(format!("CI: {e}")),
            }
            changed = true;
        }
        changed
    }

    fn poll_op(&mut self) -> bool {
        let Some(rx) = &self.op else { return false };
        let Ok(r) = rx.try_recv() else { return false };
        self.op = None;
        self.note = Some(match r {
            Ok(s) => format!("git: {s}"),
            Err(e) => format!("git: {e}"),
        });
        self.refresh(true);
        self.sync_stash(true);
        true
    }

    /// The stash file changed (an agent named an entry, a stash ended): read it again; new
    /// entries: their agent gets asked for a name (NOW: read even with 0 change seen).
    fn sync_stash(&mut self, now: bool) -> bool {
        let at = stash::stamp(&self.id);
        if !now && at == self.stash_at {
            return false;
        }
        self.stash_at = at;
        self.stash = stash::read(&self.id);
        let mut asked = false;
        for e in self.stash.iter_mut().filter(|e| !e.asked && e.name.is_empty()) {
            e.asked = true;
            asked = true;
            self.outbox.push(stash::name_ask(e));
        }
        if asked {
            stash::write(&self.id, &self.stash);
            self.stash_at = stash::stamp(&self.id);
        }
        true
    }

    /// The comments to their file (the view's own write: 0 read again).
    fn save_comments(&mut self) {
        write_comments(&self.id, &self.comments);
        self.comments_at = comments_stamp(&self.id);
    }

    /// The comments file changed (an agent wrote comments): read it again.
    fn sync_comments(&mut self) {
        let at = comments_stamp(&self.id);
        if at == self.comments_at {
            return;
        }
        self.comments_at = at;
        let c = read_comments(&self.id);
        if c != self.comments {
            self.comments = c;
            self.built = None;
        }
    }

    /// Staged files exist (a commit can start).
    pub fn has_staged(&self) -> bool {
        !self.changes.staged.is_empty()
    }

    /// Read git in the background (NOW: even if the last read is fresh).
    pub fn refresh(&mut self, now: bool) {
        if self.loading.is_some() || (!now && self.loaded.is_some_and(|t| t.elapsed() < REFRESH)) {
            return;
        }
        // PR mode: a read on a new head only (gh calls are counted)
        if !now && self.pr_src.is_some() && (self.loaded.is_some() || self.error.is_some()) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        if let Some(src) = self.pr_src.clone() {
            std::thread::spawn(move || {
                let _ = tx.send(load_pr(&src));
            });
            self.loading = Some(rx);
            return;
        }
        let dir = self.dir.clone();
        std::thread::spawn(move || {
            let _ = tx.send(load(&dir));
        });
        self.loading = Some(rx);
    }

    /// A finished read applies. True: something changed.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.loading else { return false };
        let Ok(r) = rx.try_recv() else { return false };
        self.loading = None;
        self.loaded = Some(Instant::now());
        match r {
            Ok(c) => {
                // the same diffs (the view's order is the agent's, the read's the path rules'):
                // 0 new rows; the git state (staged, ahead, a stopped rebase) still applies
                let hashes = |fs: &[FileDiff]| {
                    let mut v: Vec<u64> = fs.iter().map(|f| f.hash).collect();
                    v.sort_unstable();
                    v
                };
                if hashes(&c.files) == hashes(&self.changes.files) && self.error.is_none() {
                    let git = (&c.base, &c.staged, &c.stopped, c.ahead, c.behind);
                    let changed = git != (&self.changes.base, &self.changes.staged, &self.changes.stopped, self.changes.ahead, self.changes.behind);
                    if changed {
                        self.built = self.built.filter(|_| c.stopped.is_some() == self.changes.stopped.is_some());
                        let files = std::mem::take(&mut self.changes.files);
                        self.changes = Changes { files, ..c };
                    }
                    return changed;
                }
                // the cursor stays on its row of its file
                let cur = self.cursor_spot();
                // a selection holds on files whose diff stayed
                let kept: HashSet<(&str, u64)> = c.files.iter().map(|f| (f.path.as_str(), f.hash)).collect();
                let old: HashMap<&str, u64> = self.changes.files.iter().map(|f| (f.path.as_str(), f.hash)).collect();
                self.sel.retain(|p, _| old.get(p.as_str()).is_some_and(|h| kept.contains(&(p.as_str(), *h))));
                if self.paints.borrow().len() > 4096 {
                    self.paints.borrow_mut().clear();
                }
                self.changes = c;
                self.error = None;
                self.built = None;
                self.apply_rank(false);
                self.pending_cursor = cur;
                true
            }
            Err(e) => {
                let changed = self.error.as_deref() != Some(e.as_str());
                self.error = Some(e);
                changed
            }
        }
    }

    pub fn is_reviewed(&self, f: &FileDiff) -> bool {
        self.marks.get(&f.path) == Some(&f.hash)
    }

    /// Reviewed boxes / all boxes.
    pub fn progress(&self) -> (usize, usize) {
        (self.changes.files.iter().filter(|f| self.is_reviewed(f)).count(), self.changes.files.len())
    }

    fn folded(&self, f: &FileDiff) -> bool {
        let default = self.is_reviewed(f) || f.cat == Cat::Generated || f.binary;
        default != self.toggled.contains(&f.path)
    }

    fn side_by_side(&self, width: u16) -> bool {
        self.side.unwrap_or(width >= SIDE_MIN)
    }

    fn build(&mut self, width: u16) {
        let side = self.side_by_side(width);
        if self.built == Some((width, side)) {
            return;
        }
        self.fresh_rank = self.rank.as_ref().is_some_and(|r| r.key == crate::rank::key(&self.changes.files));
        // reviewed / all per level
        let levels = self.levels();
        let mut counts = [(0, 0); 4];
        for (f, l) in self.changes.files.iter().zip(&levels) {
            counts[*l as usize].1 += 1;
            counts[*l as usize].0 += usize::from(self.is_reviewed(f));
        }
        self.counts = counts;
        // the shown files: the filter's hits (by relevance), else all by level (the panel's order)
        let shown: Vec<usize> = if self.filter.trim().is_empty() {
            self.hits.clear();
            let mut v: Vec<usize> = (0..self.changes.files.len()).collect();
            v.sort_by_key(|&i| levels[i] as usize);
            v
        } else {
            let hits = crate::fuzzy::rank(&self.changes.files, &self.filter, |f| f.path.clone(), body_text);
            self.hits = hits.iter().cloned().collect();
            hits.into_iter().map(|h| h.0).collect()
        };
        self.lv = levels;
        let mut rows: Vec<Row> = Vec::new();
        if self.changes.stopped.is_some() {
            rows.push(Row::new(Body::Conflict, 0, None));
        }
        if !self.filter.trim().is_empty() {
            rows.push(Row::new(Body::Hits(shown.len()), 0, None));
        }
        // group heads: the model's groups (title, why), else the path-rule categories; 0 heads
        // for filter hits (in relevance order)
        let mut last: Option<(Option<usize>, Cat)> = None;
        let mut num_w = HashMap::new();
        for &i in &shown {
            let f = &self.changes.files[i];
            if self.filter.trim().is_empty() {
                let g = self.ranked(&f.path).map(|x| x.0);
                let new_group = match (last, g) {
                    (None, _) => true,
                    (Some((pg, _)), Some(gi)) => pg != Some(gi),
                    (Some((pg, pc)), None) => pg.is_some() || pc != f.cat,
                };
                if new_group {
                    rows.push(Row::new(Body::Blank, i, None));
                    rows.push(Row::new(Body::Group(g, f.cat), i, None));
                }
                last = Some((g, f.cat));
            }
            if !self.folded(f) {
                num_w.insert(i, f.hunks.iter().flat_map(|h| &h.lines).filter_map(|l| l.new.max(l.old)).max().unwrap_or(1).to_string().len().max(3));
            }
            self.file_rows(&mut rows, i, f, side);
        }
        self.num_w = num_w;
        self.rows = rows;
        self.built = Some((width, side));
        // the cursor back on its file: the same row of it (a new diff: up to its last row)
        if let Some((p, off)) = self.pending_cursor.take()
            && let Some(i) = self.changes.files.iter().position(|f| f.path == p)
            && let Some(r) = self.rows.iter().position(|r| r.head && r.file == i)
        {
            let n = self.rows[r..].iter().take_while(|x| x.file == i).count();
            self.at = r + off.min(n.saturating_sub(1));
        }
        self.at = self.at.min(self.rows.len().saturating_sub(1));
        // a group head under the cursor: the 1st box
        if !self.rows.get(self.at).is_some_and(|r| r.head || r.at.is_some()) && let Some(h) = self.rows.iter().position(|r| r.head) {
            self.at = h;
        }
    }

    /// The rows of file I (F): its head, then (unfolded) the why, the hunks, findings and
    /// comments under their lines, the foot.
    fn file_rows(&self, rows: &mut Vec<Row>, i: usize, f: &FileDiff, side: bool) {
        let first = f.first_line();
        rows.push(Row::new(Body::Head, i, Some(first)));
        if self.folded(f) {
            return;
        }
        if self.ranked(&f.path).is_some_and(|x| !x.2.is_empty()) {
            rows.push(Row::new(Body::Why, i, Some(first)));
        }
        if f.binary || f.hunks.is_empty() {
            rows.push(Row::new(Body::Note(if f.binary { "binary file" } else { "0 line changes (mode or rename)" }), i, Some(first)));
        }
        // a new or deleted file: 1 side only, unified uses the room
        let side = side && !matches!(f.status, Status::New | Status::Deleted);
        for (hi, h) in f.hunks.iter().enumerate() {
            rows.push(Row::new(Body::Hunk(hi), i, Some(h.new.max(1))));
            let ls = &h.lines;
            if !side {
                rows.extend(ls.iter().enumerate().map(|(k, l)| Row::new(Body::Diff(hi, k), i, l.new.or(l.old))));
                continue;
            }
            // side by side: a run of removed lines pairs with the added lines after it
            let mut k = 0;
            while k < ls.len() {
                if ls[k].kind == Kind::Ctx {
                    rows.push(Row::new(Body::Side(hi, Some(k), Some(k)), i, ls[k].new));
                    k += 1;
                    continue;
                }
                let d = ls[k..].iter().take_while(|l| l.kind == Kind::Del).count();
                let a = ls[k + d..].iter().take_while(|l| l.kind == Kind::Add).count();
                for j in 0..d.max(a) {
                    let (dk, ak) = ((j < d).then_some(k + j), (j < a).then_some(k + d + j));
                    let at = ak.and_then(|x| ls[x].new).or_else(|| (a > 0).then(|| ls[k + d].new).flatten()).or_else(|| dk.and_then(|x| ls[x].old));
                    rows.push(Row::new(Body::Side(hi, dk, ak), i, at));
                }
                k += d + a;
            }
        }
        // findings, then review comments: under the 1st diff row of their line, else at the box's end
        let start = rows.iter().rposition(|r| r.head && r.file == i).unwrap_or(0);
        let mut notes: Vec<(u32, Vec<Row>)> = Vec::new();
        for (fi, x) in self.review.iter().flat_map(|r| r.findings.iter().enumerate()).filter(|(_, x)| x.file == f.path) {
            let n = 1 + x.body.lines().take(3).count();
            notes.push((x.line, (0..n).map(|k| Row::new(Body::Finding(fi, k), i, Some(x.line))).collect()));
        }
        for (ci, c) in self.comments.iter().enumerate().filter(|(_, c)| c.path == f.path) {
            notes.push((c.line, (0..c.text.lines().count().max(1)).map(|k| Row::new(Body::Comment(ci, k), i, Some(c.line))).collect()));
        }
        // PR mode: the GitHub threads on the file's lines
        for (ti, t) in self.pr_src.iter().flat_map(|s| s.threads.iter().enumerate()).filter(|(_, t)| t.path.as_deref() == Some(f.path.as_str())) {
            let n = gh_lines(t, usize::MAX).len();
            let line = t.line.unwrap_or(0);
            notes.push((line, (0..n).map(|k| Row::new(Body::Gh(ti, k), i, Some(line))).collect()));
        }
        let added = |r: &Row| r.comment.is_some() || r.finding.is_some() || matches!(r.body, Body::Gh(..));
        for (line, new) in notes {
            // the last row of the 1st run on that line (a removed line, then the added one)
            let hit = |r: &Row| !r.head && !r.hunk && !added(r) && r.at == Some(line);
            let at = rows[start..].iter().position(hit).map_or(rows.len(), |p| {
                let first = start + p;
                first + 1 + rows[first + 1..].iter().take_while(|r| hit(r)).count()
            });
            // after the notes already under that row
            let at = at + rows[at..].iter().take_while(|r| added(r)).count();
            rows.splice(at..at, new);
        }
        rows.push(Row::new(Body::Foot, i, None));
    }

    /// The painted lines of hunk H of F (made once per diff of the file).
    fn painted(&self, f: &FileDiff, h: usize) -> std::rc::Rc<Vec<Painted>> {
        let key = (f.hash, h);
        if let Some(p) = self.paints.borrow().get(&key) {
            return p.clone();
        }
        let p = std::rc::Rc::new(f.hunks.get(h).map(|x| paint(f, x)).unwrap_or_default());
        self.paints.borrow_mut().insert(key, p.clone());
        p
    }

    /// The line of row I in the built width.
    fn line(&self, i: usize) -> Line<'static> {
        let Some(r) = self.rows.get(i) else { return Line::raw("") };
        let w = usize::from(self.built.map_or(80, |b| b.0));
        let inner = w.saturating_sub(4);
        let border = self.border(r.file);
        let bar = |mut spans: Vec<Span<'static>>| -> Line<'static> {
            let used: usize = spans.iter().map(|s| s.content.width()).sum();
            let mut v = vec![Span::styled("│ ", border)];
            v.append(&mut spans);
            v.push(Span::raw(" ".repeat(inner.saturating_sub(used))));
            v.push(Span::styled(" │", border));
            Line::from(v)
        };
        let Some(f) = self.changes.files.get(r.file) else { return Line::raw("") };
        match r.body {
            Body::Conflict => {
                let c = self.changes.stopped.clone().unwrap_or_default();
                let files = if c.is_empty() { "0 conflicts".to_string() } else { format!("{} conflicts: {}", c.len(), c.join(", ")) };
                Line::from(vec![
                    Span::styled(" rebase stopped ", theme::pill()),
                    Span::styled(crate::nav::fit(&format!(" {files} · A: agent resolves · K: continue · X: abort"), w.saturating_sub(16)), Style::default().fg(theme::pal().warn)),
                ])
            }
            Body::Blank => Line::raw(""),
            Body::Group(g, cat) => match g.and_then(|g| self.rank.as_ref().and_then(|r| r.groups.get(g)).map(|gr| (g, gr))) {
                Some((gi, gr)) => Line::from(vec![
                    Span::styled(format!(" {} · {}", gi + 1, gr.title), theme::accent_bold()),
                    Span::styled(crate::nav::fit(&format!("  {}", gr.why), w.saturating_sub(gr.title.width() + 8)), theme::muted()),
                ]),
                None => Line::styled(format!(" {}", cat.label()), theme::muted().add_modifier(Modifier::BOLD)),
            },
            Body::Hits(n) => Line::styled(format!(" {n} files match · by name and content, every level · enter: the 1st · esc: clear"), theme::muted()),
            Body::Head => self.head_line(r.file, f, w),
            Body::Foot => Line::styled(format!("└{}┘", "─".repeat(w.saturating_sub(2))), border),
            Body::Why => {
                let why = self.ranked(&f.path).map(|x| x.2).unwrap_or_default();
                bar(vec![Span::styled(crate::nav::fit(why, inner), theme::muted().add_modifier(Modifier::ITALIC))])
            }
            Body::Note(n) => bar(vec![Span::styled(n.to_string(), theme::muted())]),
            Body::Hunk(h) => {
                let h = &f.hunks[h];
                let label = format!("⋯ {}", if h.ctx.is_empty() { format!("line {}", h.new) } else { h.ctx.clone() });
                bar(vec![Span::styled(crate::nav::fit(&label, inner), theme::muted())])
            }
            Body::Diff(h, k) => {
                let (l, p) = (&f.hunks[h].lines[k], self.painted(f, h));
                let num_w = self.num_w.get(&r.file).copied().unwrap_or(3);
                let num = match l.kind {
                    Kind::Add => format!("{:>num_w$} {:>num_w$}", "", l.new.unwrap_or(0)),
                    Kind::Del => format!("{:>num_w$} {:>num_w$}", l.old.unwrap_or(0), ""),
                    Kind::Ctx => format!("{:>num_w$} {:>num_w$}", l.old.unwrap_or(0), l.new.unwrap_or(0)),
                };
                let room = inner.saturating_sub(num.len() + 3);
                let mut spans = vec![Span::styled(num, theme::muted()), sign_span(l.kind)];
                if let Some(pl) = p.get(k) {
                    spans.extend(painted_spans(pl, l.kind, room));
                }
                bar(spans)
            }
            Body::Side(h, dk, ak) => {
                let (ls, p) = (&f.hunks[h].lines, self.painted(f, h));
                let num_w = self.num_w.get(&r.file).copied().unwrap_or(3);
                let half = inner.saturating_sub(3) / 2;
                let cell = |k: Option<usize>, left: bool| -> Vec<Span<'static>> {
                    let Some(k) = k else { return vec![Span::raw(" ".repeat(half))] };
                    let l = &ls[k];
                    let n = if left { l.old } else { l.new }.unwrap_or(0);
                    let mut v = vec![Span::styled(format!("{n:>num_w$}"), theme::muted()), sign_span(l.kind)];
                    if let Some(pl) = p.get(k) {
                        v.extend(painted_spans(pl, l.kind, half.saturating_sub(num_w + 3)));
                    }
                    v
                };
                let mut s = cell(dk, true);
                s.push(Span::styled(" │ ", theme::line()));
                s.extend(cell(ak, false));
                bar(s)
            }
            Body::Finding(fi, k) => {
                let Some(x) = self.review.as_ref().and_then(|r| r.findings.get(fi)) else { return bar(Vec::new()) };
                let color = match x.severity.as_str() {
                    "critical" => theme::pal().danger,
                    "major" => theme::pal().warn,
                    _ => theme::pal().muted,
                };
                let st = Style::default().fg(color).add_modifier(Modifier::BOLD);
                if k == 0 {
                    let head = format!("⚠ {} · ", x.severity);
                    bar(vec![Span::styled(head.clone(), st), Span::styled(crate::nav::fit(&x.title, inner.saturating_sub(head.width())), st)])
                } else {
                    let l = x.body.lines().nth(k - 1).unwrap_or_default();
                    bar(vec![Span::raw("  "), Span::styled(crate::nav::fit(l, inner.saturating_sub(2)), theme::muted())])
                }
            }
            Body::Gh(ti, k) => {
                let Some(t) = self.pr_src.as_ref().and_then(|s| s.threads.get(ti)) else { return bar(Vec::new()) };
                let lines = gh_lines(t, inner);
                let Some((head, text)) = lines.get(k) else { return bar(Vec::new()) };
                let st = Style::default().fg(theme::pal().warn);
                if *head {
                    bar(vec![Span::styled(crate::nav::fit(text, inner), st.add_modifier(Modifier::BOLD))])
                } else {
                    bar(vec![Span::styled(crate::nav::fit(text, inner), st)])
                }
            }
            Body::Comment(ci, k) => {
                let Some(c) = self.comments.get(ci) else { return bar(Vec::new()) };
                let st = Style::default().fg(theme::pal().accent);
                let l = c.text.lines().nth(k).unwrap_or_default();
                if k > 0 {
                    return bar(vec![Span::raw("  "), Span::styled(crate::nav::fit(l, inner.saturating_sub(2)), st)]);
                }
                // the 1st row: severity, lines, the agent that wrote it
                let sev = format!("{} ", c.level.label());
                let by = if c.by.is_empty() { String::new() } else { format!(" · {}", c.by) };
                let head = format!("✎ {}{by}: ", c.at());
                let room = inner.saturating_sub(head.width() + sev.width());
                bar(vec![Span::styled(sev, Style::default().fg(scm::level_color(c.level)).add_modifier(Modifier::BOLD)), Span::styled(head, st.add_modifier(Modifier::BOLD)), Span::styled(crate::nav::fit(l, room), st)])
            }
        }
    }

    /// The border style of file I's box: accent on the file of the cursor.
    /// The panel's keys on a file row: that file.
    fn border(&self, i: usize) -> Style {
        let on = match self.scm.file_at() {
            Some(f) => f == i,
            None => self.rows.get(self.at).is_some_and(|r| r.file == i && !matches!(r.body, Body::Blank | Body::Group(..) | Body::Hits(_) | Body::Conflict)),
        };
        if on { theme::accent() } else { theme::line() }
    }

    /// The head row of file I (F): reviewed mark, staged mark, path (filter hits marked), level,
    /// category, size.
    fn head_line(&self, i: usize, f: &FileDiff, w: usize) -> Line<'static> {
        let reviewed = self.is_reviewed(f);
        let updated = !reviewed && self.marks.contains_key(&f.path);
        let border = self.border(i);
        let mark = if reviewed { Span::styled("☑ ", Style::default().fg(theme::pal().ok)) } else { Span::styled("☐ ", theme::muted()) };
        let (name, skip) = match &f.old_path {
            Some(o) => (format!("{o} → {}", f.path), o.chars().count() + 3),
            None => (f.path.clone(), 0),
        };
        let tag = match f.status {
            Status::New => " new",
            Status::Deleted => " deleted",
            _ => "",
        };
        let right = if f.binary { "binary".to_string() } else { format!("+{} −{}", f.add, f.del) };
        let right = format!(" {}{tag} · {right} ", f.cat.label());
        let room = w.saturating_sub(right.width() + 8 + if updated { 9 } else { 0 });
        let stage = match self.changes.staged.get(&f.path) {
            Some(true) => Span::styled("● ", Style::default().fg(theme::pal().ok)),
            Some(false) => Span::styled("◐ ", Style::default().fg(theme::pal().warn)),
            None => Span::raw(""),
        };
        // a folded box: 1 line, 0 corners
        let (open, close) = if self.folded(f) { ("── ", "──") } else { ("┌─ ", "─┐") };
        let mut head = vec![Span::styled(open, border), mark, stage];
        let fitted = crate::nav::fit(&name, room);
        let marks: Vec<usize> = self.hits.get(&i).map(|m| m.iter().map(|x| x + skip).filter(|&x| x < fitted.chars().count()).collect()).unwrap_or_default();
        head.extend(crate::fuzzy::spans(&fitted, &marks, theme::bold(), theme::accent_bold().add_modifier(Modifier::UNDERLINED)));
        if updated {
            head.push(Span::styled(" updated", Style::default().fg(theme::pal().warn)));
        }
        let used: usize = head.iter().map(|s| s.content.width()).sum();
        let fill = w.saturating_sub(used + right.width() + 3);
        head.push(Span::styled(format!(" {}", "─".repeat(fill)), border));
        head.push(Span::styled(right, theme::muted()));
        head.push(Span::styled(close, border));
        Line::from(head)
    }

    /// The selection keys of row R: its diff lines.
    fn keys(&self, r: &Row) -> Vec<LKey> {
        let Some(f) = self.changes.files.get(r.file) else { return Vec::new() };
        let line = |h: usize, k: usize| f.hunks.get(h).and_then(|x| x.lines.get(k)).and_then(lkey);
        match r.body {
            Body::Diff(h, k) => line(h, k).into_iter().collect(),
            Body::Side(h, a, b) if a == b => a.and_then(|k| line(h, k)).into_iter().collect(),
            Body::Side(h, a, b) => a.and_then(|k| line(h, k)).into_iter().chain(b.and_then(|k| line(h, k))).collect(),
            _ => Vec::new(),
        }
    }

    /// Row I has a selected line.
    fn selected(&self, i: usize) -> bool {
        let Some(r) = self.rows.get(i) else { return false };
        let Some(set) = self.changes.files.get(r.file).and_then(|f| self.sel.get(&f.path)) else { return false };
        self.keys(r).iter().any(|k| set.contains(k))
    }

    /// Select (ON) or unselect the lines of row I.
    fn select_row(&mut self, i: usize, on: bool) {
        let Some(r) = self.rows.get(i) else { return };
        let keys = self.keys(r);
        let Some(path) = self.changes.files.get(r.file).map(|f| f.path.clone()) else { return };
        if keys.is_empty() {
            return;
        }
        let set = self.sel.entry(path.clone()).or_default();
        for k in keys {
            if on {
                set.insert(k);
            } else {
                set.remove(&k);
            }
        }
        if set.is_empty() {
            self.sel.remove(&path);
        }
    }

    /// The selection as comment spots (1 per file, in row order): each on the last selected
    /// row's line.
    fn spots(&self) -> Vec<Spot> {
        let mut out: Vec<Spot> = Vec::new();
        for (i, r) in self.rows.iter().enumerate() {
            if !self.selected(i) {
                continue;
            }
            let Some(f) = self.changes.files.get(r.file) else { continue };
            let line = r.at.unwrap_or_else(|| f.first_line());
            match out.iter_mut().find(|s| s.path == f.path) {
                Some(s) => s.line = line,
                None => out.push(Spot { path: f.path.clone(), line, lines: self.sel.get(&f.path).map(lines_label).unwrap_or_default() }),
            }
        }
        // a selected file not shown (another tab): its 1st selected line
        for (p, set) in &self.sel {
            if !out.iter().any(|s| &s.path == p)
                && let Some(k) = set.iter().find(|k| !k.0).or(set.iter().next())
            {
                out.push(Spot { path: p.clone(), line: k.1, lines: lines_label(set) });
            }
        }
        out
    }

    #[cfg(test)]
    pub fn comment_count(&self) -> usize {
        self.comments.len()
    }

    /// Add a comment TEXT on SPOTS (1 per file); the selection it was on clears; the rows show
    /// it at once.
    #[cfg(test)]
    pub fn add_comment(&mut self, spots: &[Spot], text: &str) {
        self.add_comment_at(spots, text, crate::rank::Level::default());
    }

    /// `add_comment` with severity LEVEL.
    fn add_comment_at(&mut self, spots: &[Spot], text: &str, level: crate::rank::Level) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        for s in spots {
            let quote = self.quote(s);
            self.comments.push(Comment { path: s.path.clone(), line: s.line, text: text.to_string(), lines: s.lines.clone(), quote, level, by: String::new() });
        }
        self.save_comments();
        self.sel.clear();
        self.rebuild_here();
    }

    /// The diff lines of S (`+ let b = 3;`): the selected lines of its file, else its line.
    fn quote(&self, s: &Spot) -> String {
        let Some(f) = self.changes.files.iter().find(|f| f.path == s.path) else { return String::new() };
        let sign = |k: Kind| match k {
            Kind::Add => '+',
            Kind::Del => '-',
            Kind::Ctx => ' ',
        };
        let all = f.hunks.iter().flat_map(|h| &h.lines);
        let lines: Vec<&DLine> = match self.sel.get(&s.path) {
            Some(set) => all.filter(|l| lkey(l).is_some_and(|k| set.contains(&k))).take(40).collect(),
            None => all.clone().find(|l| l.new == Some(s.line)).or_else(|| all.clone().find(|l| l.kind == Kind::Del && l.old == Some(s.line))).into_iter().collect(),
        };
        lines.iter().map(|l| format!("{} {}", sign(l.kind), l.text)).collect::<Vec<_>>().join("\n")
    }

    /// The comment modal on SPOTS (the selection, else the cursor's line).
    fn open_comment(&mut self, spots: Vec<Spot>) {
        let mut quote = Vec::new();
        for (i, s) in spots.iter().enumerate() {
            if spots.len() > 1 {
                if i > 0 {
                    quote.push(QLine::Gap);
                }
                quote.push(QLine::Path(s.label()));
            }
            quote.extend(self.quote(s).lines().map(|l| QLine::Code(l.to_string())));
        }
        let title = spots.iter().map(Spot::label).collect::<Vec<_>>().join(", ");
        self.modal = Some(Modal::Comment { title, quote, scroll: 0, edit: None, spots, text: comment_area(""), level: crate::rank::Level::default(), on_level: true });
    }

    /// The comment modal on comment CI: its text to edit (0 text: it goes).
    pub(super) fn edit_comment(&mut self, ci: usize) {
        let Some(c) = self.comments.get(ci) else { return };
        let title = format!("{}:{}", c.path, c.at());
        let quote = c.quote.lines().map(|l| QLine::Code(l.to_string())).collect();
        self.modal = Some(Modal::Comment { title, quote, scroll: 0, edit: Some(ci), spots: Vec::new(), text: comment_area(&c.text), level: c.level, on_level: false });
    }

    /// The modal's save: a new comment, an edit (0 text: deleted).
    fn save_comment(&mut self) -> Act {
        let Some(Modal::Comment { spots, edit, text, level, .. }) = self.modal.take() else { return Act::None };
        let t = text.text().trim().to_string();
        match edit {
            Some(ci) if ci < self.comments.len() => {
                if t.is_empty() {
                    self.comments.remove(ci);
                } else {
                    self.comments[ci].text = t;
                    self.comments[ci].level = level;
                }
                self.save_comments();
                self.rebuild_here();
                Act::Toast(format!("comment saved · {} in the batch", self.comments.len()))
            }
            Some(_) => Act::None,
            None if t.is_empty() => Act::Toast("0 text: 0 comment".into()),
            None => {
                self.add_comment_at(&spots, &t, level);
                let on = spots.iter().map(Spot::label).collect::<Vec<_>>().join(", ");
                Act::Toast(format!("comment on {on} · {} in the batch · ctrl+enter: send", self.comments.len()))
            }
        }
    }

    /// A key while a modal is open.
    fn modal_key(&mut self, k: KeyEvent) -> Act {
        let send = k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);

        // esc: closes; an edited comment only on a 2nd esc (esc never drops edits unasked)
        if k.code == KeyCode::Esc
            && let Some(Modal::Comment { text, .. }) = &self.modal
        {
            let dirty = text.edited();
            if !crate::textarea::esc_drops(&mut self.esc_at, dirty) {
                return Act::Toast("esc again: drop the comment · ctrl+⏎: save it".into());
            }
            self.modal = None;
            return Act::None;
        }

        match self.modal.as_mut() {
            // the severity row: ←→ cycle it, ↓ / a letter: the text
            // alt+↑↓: the selected lines scroll
            Some(Modal::Comment { scroll, .. }) if k.modifiers.contains(KeyModifiers::ALT) && matches!(k.code, KeyCode::Up | KeyCode::Down) => {
                *scroll = if k.code == KeyCode::Up { scroll.saturating_sub(1) } else { *scroll + 1 };
            }
            Some(Modal::Comment { text, level, on_level: on @ true, .. }) => match k.code {
                KeyCode::Esc => self.modal = None,
                KeyCode::Enter if send => return self.save_comment(),
                KeyCode::Left | KeyCode::Right => *level = level.step(if k.code == KeyCode::Right { 1 } else { -1 }),
                KeyCode::Up => {}
                KeyCode::Down | KeyCode::Enter | KeyCode::Tab => *on = false,
                _ => {
                    *on = false;
                    text.key(k);
                }
            },
            Some(Modal::Comment { text, scroll, on_level, .. }) => match k.code {
                KeyCode::Esc => self.modal = None,
                KeyCode::Enter if send => return self.save_comment(),
                // ↑ on the text's 1st row, shift+tab: the severity row
                KeyCode::Up if !k.modifiers.contains(KeyModifiers::ALT) && text.on_first_row() => *on_level = true,
                KeyCode::BackTab => *on_level = true,
                // alt+↑↓: the selected lines scroll
                KeyCode::Up if k.modifiers.contains(KeyModifiers::ALT) => *scroll = scroll.saturating_sub(1),
                KeyCode::Down if k.modifiers.contains(KeyModifiers::ALT) => *scroll += 1,
                _ => {
                    if !matches!(text.key(k), crate::textarea::Outcome::Handled) && k.code == KeyCode::Enter {
                        text.newline();
                    }
                }
            },
            Some(Modal::Confirm { yes, .. }) => match k.code {
                KeyCode::Esc | KeyCode::Char('n') => self.modal = None,
                KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::Char('h') | KeyCode::Char('l') => *yes = !*yes,
                KeyCode::Char('y') => return self.confirm(true),
                KeyCode::Enter => {
                    let y = *yes;
                    return self.confirm(y);
                }
                _ => {}
            },
            None => {}
        }
        Act::None
    }

    /// The question's answer YES: its step runs.
    fn confirm(&mut self, yes: bool) -> Act {
        let Some(Modal::Confirm { then, .. }) = self.modal.take() else { return Act::None };
        if !yes {
            return Act::None;
        }
        self.stash = stash::read(&self.id);
        let n = self.stash.len();
        match then {
            Then::DropStash(id) => self.stash.retain(|e| e.id != id),
            Then::ClearStash => self.stash.clear(),
        }
        stash::write(&self.id, &self.stash);
        self.stash_at = stash::stamp(&self.id);
        Act::Toast(format!("stash: {} removed · {} left", n - self.stash.len(), self.stash.len()))
    }

    /// A mouse event while a modal is open: the text takes it, a click on a button answers.
    fn modal_mouse(&mut self, m: MouseEvent) -> Act {
        let at = ratatui::layout::Position { x: m.column, y: m.row };
        let click = matches!(m.kind, MouseEventKind::Down(MouseButton::Left));
        if click && let Some(&(_, yes)) = self.modal_btns.iter().find(|(r, _)| r.contains(at)) {
            return if let Some(Modal::Comment { .. }) = self.modal { if yes { self.save_comment() } else { self.modal = None; Act::None } } else { self.confirm(yes) };
        }
        let in_quote = self.modal_quote.contains(at);
        if let Some(Modal::Comment { text, scroll, .. }) = self.modal.as_mut() {
            match m.kind {
                MouseEventKind::ScrollUp if in_quote => *scroll = scroll.saturating_sub(3),
                MouseEventKind::ScrollDown if in_quote => *scroll += 3,
                _ => {
                    text.mouse(m);
                }
            }
        }
        Act::None
    }

    fn draw_modal(&mut self, f: &mut Frame, area: Rect) {
        use ratatui::widgets::{Block, Clear};
        self.modal_btns.clear();
        let Some(modal) = self.modal.as_mut() else { return };
        let w = area.width.saturating_sub(4).clamp(20, 110);
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        match modal {
            Modal::Comment { title, quote, scroll, edit, text, level, on_level, .. } => {
                let tw = w.saturating_sub(4);
                let th = (text.rows_for(tw) as u16).clamp(3, 8);
                // the selected lines: at most half the screen (and 24 rows); more: they scroll
                let room = (area.height.saturating_sub(th + 8) as usize).min(usize::from(area.height / 2).max(3)).min(24);
                let qh = quote.len().min(room);
                *scroll = (*scroll).min(quote.len() - qh);
                let h = (qh as u16 + th + 7 + u16::from(qh > 0)).min(area.height);
                let y = area.y + (area.height.saturating_sub(h)) / 3;
                let r = Rect { x, y, width: w, height: h };
                f.render_widget(Clear, r);
                let head = if edit.is_some() { format!(" edit comment · {title} ") } else { format!(" comment on {title} ") };
                f.render_widget(Block::bordered().border_style(theme::accent()).title(Span::styled(crate::nav::fit(&head, usize::from(w.saturating_sub(4))), theme::accent_bold())).style(theme::text()), r);
                let inner = Rect { x: r.x + 2, y: r.y + 1, width: w.saturating_sub(4), height: h.saturating_sub(2) };
                let bg = code_bg();
                let mut yy = inner.y;
                // 1 column for the scrollbar when the lines scroll
                let bar = quote.len() > qh;
                let cw = inner.width.saturating_sub(u16::from(bar));
                self.modal_quote = Rect { y: yy, height: qh as u16, ..inner };
                for l in quote.iter().skip(*scroll).take(qh) {
                    let row = Rect { y: yy, height: 1, width: cw, ..inner };
                    match l {
                        QLine::Gap => {}
                        QLine::Path(p) => f.render_widget(Paragraph::new(Line::styled(crate::nav::fit(p, usize::from(cw)), theme::bold())), row),
                        QLine::Code(c) => {
                            let st = match c.chars().next() {
                                Some('+') => Style::default().fg(theme::pal().ok),
                                Some('-') => Style::default().fg(theme::pal().danger),
                                _ => theme::muted(),
                            };
                            f.render_widget(Paragraph::new(Line::styled(crate::nav::fit(c, usize::from(cw)), st)).style(Style::default().bg(bg)), row);
                        }
                    }
                    yy += 1;
                }
                if bar {
                    // the thumb: the visible part of the lines
                    let n = quote.len();
                    let len = (qh * qh).div_ceil(n).max(1);
                    let at = (*scroll * qh) / n;
                    for k in 0..qh {
                        let on = k >= at && k < at + len;
                        let cell = Span::styled(if on { "┃" } else { "│" }, if on { theme::accent() } else { theme::line() });
                        f.render_widget(Paragraph::new(cell), Rect { x: inner.x + cw, y: inner.y + k as u16, width: 1, height: 1 });
                    }
                }
                if qh > 0 {
                    yy += 1;
                }
                // the severity row: the 4 levels, the picked one a pill (accent: the row has the keys)
                let mut spans = vec![Span::styled("severity ", if *on_level { theme::accent_bold() } else { theme::muted() })];
                for l in crate::rank::Level::ALL {
                    let st = if l == *level {
                        let bg = if *on_level { scm::level_color(l) } else { theme::pal().panel };
                        Style::default().bg(bg).fg(if *on_level { theme::pal().on_accent } else { scm::level_color(l) }).add_modifier(Modifier::BOLD)
                    } else {
                        theme::muted()
                    };
                    spans.push(Span::styled(format!(" {} ", l.label()), st));
                    spans.push(Span::raw(" "));
                }
                spans.push(Span::styled(if *on_level { " ←→ · ↓ text" } else { " ↑ on the 1st row: severity" }, theme::muted()));
                f.render_widget(Paragraph::new(Line::from(spans)), Rect { y: yy, height: 1, ..inner });
                yy += 2;
                let tr = Rect { y: yy, height: th.min(inner.bottom().saturating_sub(yy + 1)), ..inner };
                text.render(f, tr, !*on_level);
                let hint = Rect { y: inner.bottom().saturating_sub(1), height: 1, ..inner };
                let (save, cancel) = (" save ", " cancel ");
                f.render_widget(Paragraph::new(Line::from(vec![Span::styled(save, theme::pill()), Span::raw(" "), Span::styled(cancel, theme::muted()), Span::styled("  ctrl+⏎ save · esc cancel · 0 text: delete", theme::muted())])), hint);
                self.modal_btns.push((Rect { width: save.len() as u16, ..hint }, true));
                self.modal_btns.push((Rect { x: hint.x + save.len() as u16 + 1, width: cancel.len() as u16, ..hint }, false));
            }
            Modal::Confirm { question, yes, .. } => {
                let w = w.min(70);
                let x = area.x + (area.width.saturating_sub(w)) / 2;
                let r = Rect { x, y: area.y + area.height / 3, width: w, height: 5.min(area.height) };
                f.render_widget(Clear, r);
                f.render_widget(Block::bordered().border_style(theme::accent()).style(theme::text()), r);
                let inner = Rect { x: r.x + 2, y: r.y + 1, width: w.saturating_sub(4), height: 3 };
                f.render_widget(Paragraph::new(Line::styled(crate::nav::fit(question, usize::from(inner.width)), theme::text())), Rect { height: 1, ..inner });
                let (ys, ns) = if *yes { (theme::pill(), theme::muted()) } else { (theme::muted(), theme::pill()) };
                let row = Rect { y: inner.y + 2, height: 1, ..inner };
                f.render_widget(Paragraph::new(Line::from(vec![Span::styled(" yes ", ys), Span::raw("  "), Span::styled(" no ", ns), Span::styled("   y / n · ←→ · enter", theme::muted())])), row);
                self.modal_btns.push((Rect { width: 5, ..row }, true));
                self.modal_btns.push((Rect { x: row.x + 7, width: 4, ..row }, false));
            }
        }
    }

    /// The selected lines per file as patch picks: (path, lines label, pick); 0 changed line
    /// picked (context only): 0 entry.
    fn picks(&self) -> Vec<(String, String, patch::Pick)> {
        let mut paths: Vec<&String> = self.sel.keys().collect();
        paths.sort();
        paths
            .into_iter()
            .filter_map(|p| {
                let f = self.changes.files.iter().find(|f| &f.path == p)?;
                let set = &self.sel[p];
                let pk = pick_of(f, set);
                (!pk.is_empty()).then(|| (p.clone(), lines_label(set), pk))
            })
            .collect()
    }

    /// Stash ITEMS: out of the worktree, into the stash (on a thread).
    fn stash_op(&mut self, items: Vec<(String, String, patch::Pick)>) -> Act {
        let rev = if self.changes.rev.is_empty() { "HEAD".to_string() } else { self.changes.rev.clone() };
        self.start_op(Op::Stash { thread: self.id.clone(), rev, items })
    }

    /// What the owner's message from the changes view takes along: the selected lines, the open
    /// comments. None: neither.
    pub fn context(&self) -> Option<String> {
        let mut s = String::new();
        if !self.sel.is_empty() {
            s.push_str("Selected lines (file:lines, new side; `removed`: old side):\n");
            let mut paths: Vec<&String> = self.sel.keys().collect();
            paths.sort();
            for p in paths {
                let spot = Spot { path: p.clone(), line: 0, lines: lines_label(&self.sel[p]) };
                s.push_str(&format!("- `{}`\n```diff\n{}\n```\n", spot.label(), self.quote(&spot)));
            }
        }
        if !self.comments.is_empty() {
            s.push_str("Open review comments (file:line, new side):\n");
            for c in &self.comments {
                s.push_str(&format!("- `{}:{}`: {}\n", c.path, c.at(), c.text.replace('\n', " / ")));
            }
        }
        (!s.is_empty()).then(|| format!("[codehead: context from the changes view]\n{}", s.trim_end()))
    }

    /// Tests: FILES as the read changes (a git read on its way: dropped).
    #[cfg(test)]
    pub fn set_files(&mut self, mut files: Vec<FileDiff>) {
        order(&mut files);
        self.changes = Changes { base: "origin/main".into(), files, ..Default::default() };
        self.marks.clear();
        self.loading = None;
        self.loaded = Some(Instant::now());
        self.built = None;
    }

    /// The cursor on the 1st row of the diff, 0 panel / modal / filter: ↑ leaves the view.
    pub fn at_top(&self) -> bool {
        self.at == 0 && !self.scm.focus && self.modal.is_none() && !self.filtering
    }

    /// The view has the letters: the filter, a modal, the commit message.
    pub fn typing(&self) -> bool {
        self.filtering || self.modal.is_some() || (self.scm.focus && self.scm.msg_focus)
    }

    /// Esc is the view's: it closes a modal, leaves the filter or the message, clears the
    /// filter, the selection or the panel's picks.
    pub fn takes_esc(&self) -> bool {
        self.typing() || !self.filter.is_empty() || !self.sel.is_empty() || (self.scm.focus && !self.scm.picked.is_empty()) || self.scm.picking()
    }

    /// The keys of the footer now.
    pub fn hints(&self) -> &'static str {
        match &self.modal {
            Some(Modal::Comment { .. }) => return "ctrl+⏎: save · esc: close (edited: esc esc) · alt+↑↓ / wheel: scroll the lines · 0 text: delete ",
            Some(Modal::Confirm { .. }) => return "y / enter: yes · n / esc: no ",
            None => {}
        }
        if self.pr_src.is_some() {
            return if self.scm.focus {
                "j k · enter on the commit: pick a commit · →: open the file · e: IDE · alt: diff "
            } else if self.filtering {
                "type to filter · enter: 1st hit · esc: clear "
            } else {
                "j k: file · space: select · c: comment · v: reviewed · alt: panel · i: prompt "
            };
        }
        if self.scm.focus && self.scm.msg_focus {
            "commit message · ctrl+⏎: commit the staged files · ctrl+g: draft by model · esc: back "
        } else if self.scm.focus {
            "j k · space: pick · e: IDE · s: stage / unstage · p: stash · →: open · x: remove · enter: restore · alt: review "
        } else if self.filtering {
            "type to filter · enter: 1st hit · esc: clear "
        } else {
            "j k: file · space: select · e: IDE · s: stage · c: comment · v: reviewed · alt: panel · i: prompt "
        }
    }

    /// The filter's text (the input along).
    fn set_filter(&mut self, t: &str) {
        self.filter = t.to_string();
        if self.filter_ta.text() != t {
            self.filter_ta.set_text(t);
        }
    }

    /// A key while the filter has the focus.
    fn filter_key(&mut self, k: KeyEvent) -> Act {
        match k.code {
            KeyCode::Esc => {
                self.filtering = false;
                self.set_filter("");
            }
            KeyCode::Enter => {
                self.filtering = false;
                self.rebuild_here();
                if let Some(h) = self.rows.iter().position(|r| r.head) {
                    self.at = h;
                    self.top = 0;
                }
                return Act::None;
            }
            // the input: cursor, words, selection, undo (a text input as the others)
            _ => {
                if !matches!(self.filter_ta.key(k), crate::textarea::Outcome::Handled) {
                    return Act::None;
                }
                let t = self.filter_ta.text();
                if t == self.filter {
                    return Act::None;
                }
                self.filter = t;
            }
        }
        self.built = None;
        self.at = 0;
        self.top = 0;
        Act::None
    }

    /// The summary row and the filter row (they stay while the rows scroll).
    fn draw_bars(&mut self, f: &mut Frame) {
        f.render_widget(Paragraph::new(self.summary()), self.summary_rect);
        let filtered = !self.filter.trim().is_empty();
        if self.filtering {
            // pill, the input, the hint at the right
            let r = self.filter_rect;
            let hint = "  enter: 1st hit · esc: clear";
            let hw = (hint.chars().count() as u16).min(r.width.saturating_sub(16));
            f.render_widget(Paragraph::new(Line::from(vec![Span::styled(" f ", theme::pill()), Span::raw(" ")])), r);
            let input = Rect { x: r.x + 4, width: r.width.saturating_sub(4 + hw), ..r };
            self.filter_ta.render(f, input, true);
            f.render_widget(Paragraph::new(Span::styled(hint, theme::muted())), Rect { x: input.right(), width: hw, ..r });
            return;
        }
        let line = if filtered {
            Line::from(vec![Span::styled(" f ", theme::muted()), Span::styled(format!(" {}", self.filter), theme::accent()), Span::styled("  f: edit · esc: clear", theme::muted())])
        } else {
            Line::styled(" f  filter by name or content", theme::muted())
        };
        f.render_widget(Paragraph::new(line), self.filter_rect);
    }

    /// The rows pinned on top while row TOP shows: its box's head (+ the why under it), once
    /// the head scrolled out of view. 0: TOP is a head, between the boxes or after one.
    fn pinned(&self, top: usize) -> Vec<usize> {
        let Some(r) = self.rows.get(top) else { return Vec::new() };
        if r.head || matches!(r.body, Body::Blank | Body::Group(..) | Body::Hits(_) | Body::Conflict | Body::Foot) {
            return Vec::new();
        }
        let Some(hi) = self.rows[..top].iter().rposition(|x| x.head && x.file == r.file) else { return Vec::new() };
        let mut v = vec![hi];
        if self.rows.get(hi + 1).is_some_and(|x| x.body == Body::Why) {
            v.push(hi + 1);
        }
        v
    }

    /// Row I of the rows in RECT: its line, the selection's tint, the code background, the cursor.
    fn draw_row(&self, f: &mut Frame, rect: Rect, i: usize, x0: u16, width: u16) {
        f.render_widget(Paragraph::new(self.line(i)), rect);
        let buf = f.buffer_mut();
        if self.selected(i) {
            // a selected line: a bar in the gutter, its text on the selection tint (the sign
            // column keeps its color)
            let sel_bg = theme::tint(theme::pal().accent, theme::pal().panel, 0.28);
            for x in rect.x + 2..rect.right().saturating_sub(2) {
                if let Some(c) = buf.cell_mut((x, rect.y))
                    && c.bg == ratatui::style::Color::Reset
                {
                    c.set_bg(sel_bg);
                }
            }
            if let Some(c) = buf.cell_mut((rect.x + 1, rect.y)) {
                c.set_symbol("▌").set_fg(theme::pal().accent);
            }
        }
        // inside a box: the code's dark background
        if self.rows.get(i).is_some_and(|r| matches!(r.body, Body::Why | Body::Note(_) | Body::Hunk(_) | Body::Diff(..) | Body::Side(..) | Body::Finding(..) | Body::Comment(..) | Body::Gh(..))) {
            let bg = code_bg();
            for x in rect.x + 1..rect.right().saturating_sub(1) {
                if let Some(c) = buf.cell_mut((x, rect.y))
                    && c.bg == ratatui::style::Color::Reset
                {
                    c.set_bg(bg);
                }
            }
        }
        // the cursor line: on the accent, as the cursor row of every list
        if i == self.at && !self.scm.focus {
            // inside the box: its borders keep their color (the picked box: the accent)
            theme::cursor_row(f.buffer_mut(), Rect { x: x0 + 1, width: width.saturating_sub(2), ..rect });
            f.render_widget(Paragraph::new(Span::styled("▌", theme::accent())), Rect { x: x0 + width, width: 1, ..rect });
        }
    }

    /// The view in AREA: a read's new diffs dissolve in.
    pub fn draw(&mut self, f: &mut Frame, area: Rect) {
        self.draw_now(f, area);
        self.fx.apply(f, area, self.version);
    }

    fn draw_now(&mut self, f: &mut Frame, area: Rect) {
        let full = area;
        if self.poll_review() {
            self.built = None;
        }
        if self.poll_ship() {
            self.built = None;
        }
        if self.poll_op() {
            self.built = None;
        }
        if self.poll_rank() {
            self.built = None;
        }
        if self.poll() {
            self.built = None;
            self.version += 1;
        }
        self.sync_stash(false);
        self.sync_comments();
        self.refresh(false);
        // the panel left (ctrl+b; room permitting), a line, the review right
        let panel = self.scm.shown && area.width >= PANEL_MIN;
        let (panel_rect, area) = if panel {
            let pw = (area.width / 4).clamp(30, 46);
            for y in area.y..area.bottom() {
                f.buffer_mut()[(area.x + pw, y)].set_symbol("│").set_style(theme::line());
            }
            (Rect { width: pw, ..area }, Rect { x: area.x + pw + 1, width: area.width - pw - 1, ..area })
        } else {
            self.scm.focus = false;
            self.scm.rect = Rect::default();
            (Rect::default(), area)
        };
        // the summary row and the filter row on top (room permitting), the rows below
        let bars = area.height >= 6;
        self.summary_rect = if bars { Rect { height: 1, ..area } } else { Rect::default() };
        self.filter_rect = if bars { Rect { y: area.y + 1, height: 1, ..area } } else { Rect::default() };
        let area = if bars { Rect { y: area.y + 2, height: area.height - 2, ..area } } else { area };
        self.area = area;
        self.shown_rows.clear();
        let msg = if let Some(e) = &self.error {
            Some(format!("  changes: {e}"))
        } else if self.changes.files.is_empty() {
            Some(if self.loaded.is_none() { "  reading changes…".to_string() } else { format!("  0 changes against {}", self.changes.base) })
        } else {
            None
        };
        let width = area.width.saturating_sub(1);
        if let Some(m) = msg {
            f.render_widget(Paragraph::new(Line::styled(m, theme::muted())), area);
        } else {
            self.build(width);
            if bars {
                self.draw_bars(f);
            }
            let h = usize::from(area.height);
            // the cursor in view; a box header 1 row below the top when it can
            if self.at < self.top + 1 {
                self.top = self.at.saturating_sub(1);
            } else if self.at >= self.top + h {
                self.top = self.at + 1 - h;
            }
            self.top = self.top.min(self.rows.len().saturating_sub(h));
            // the cursor's box head pinned on top: the cursor stays below it
            let pin = self.pinned(self.top).len();
            if pin > 0 && self.at < self.top + pin && !self.rows[self.at].head {
                self.top = self.at.saturating_sub(pin);
            }
            let pinned = self.pinned(self.top);
            // only the rows in view get made: O(rows on screen) per frame
            for y in self.top..(self.top + h).min(self.rows.len()) {
                let k = y - self.top;
                let row = pinned.get(k).copied().unwrap_or(y);
                let rect = Rect { x: area.x, y: area.y + k as u16, width, height: 1 };
                self.draw_row(f, rect, row, area.x, width);
                self.shown_rows.push(row);
            }
        }
        if panel {
            self.draw_panel(f, panel_rect);
        }
        self.draw_modal(f, full);
    }

    /// Where the IDE opens: the 1st selected line (in the cursor's file, else the 1st file with
    /// a selection), else the cursor's line; a removed line: the worktree line it sat before.
    fn open_spot(&self) -> Option<(String, u32)> {
        let cur = self.file_at();
        let sel_file = cur.filter(|f| self.sel.get(&f.path).is_some_and(|s| !s.is_empty())).or_else(|| {
            let mut seen = std::collections::HashSet::new();
            self.rows.iter().filter(|r| seen.insert(r.file)).filter_map(|r| self.changes.files.get(r.file)).find(|f| self.sel.get(&f.path).is_some_and(|s| !s.is_empty()))
        });
        if let Some(f) = sel_file {
            let keys = &self.sel[&f.path];
            let first = f.hunks.iter().flat_map(|h| worktree_lines(h).into_iter().zip(&h.lines)).filter(|(_, l)| lkey(l).is_some_and(|k| keys.contains(&k))).map(|(n, _)| n).min();
            return Some((f.path.clone(), first.unwrap_or_else(|| f.first_line()).max(1)));
        }
        let (f, r) = (cur?, self.rows.get(self.at)?);
        let line = match r.body {
            Body::Diff(h, k) => worktree_lines(&f.hunks[h]).get(k).copied(),
            Body::Side(h, dk, ak) => ak.or(dk).and_then(|k| worktree_lines(&f.hunks[h]).get(k).copied()),
            _ => r.at,
        };
        Some((f.path.clone(), line.unwrap_or_else(|| f.first_line()).max(1)))
    }

    fn file_at(&self) -> Option<&FileDiff> {
        self.rows.get(self.at).and_then(|r| self.changes.files.get(r.file))
    }

    fn jump(&mut self, down: bool, pick: impl Fn(&Row) -> bool) {
        let n = self.rows.len();
        let found = if down { (self.at + 1..n).find(|&i| pick(&self.rows[i])) } else { (0..self.at).rev().find(|&i| pick(&self.rows[i])) };
        if let Some(i) = found {
            self.at = i;
        }
    }

    fn page(&self) -> usize {
        usize::from(self.area.height.saturating_sub(2)).max(1)
    }

    /// Fold (ON) or open the box under the cursor; the cursor goes to its head.
    fn fold(&mut self, on: bool) {
        let Some(f) = self.file_at() else { return };
        if self.folded(f) != on {
            let p = f.path.clone();
            if !self.toggled.remove(&p) {
                self.toggled.insert(p);
            }
        }
        self.rebuild_on_file();
    }

    /// The keys to the panel / the review (the panel shows).
    pub fn toggle_focus(&mut self) {
        self.scm.shown = true;
        self.scm.focus = !self.scm.focus;
        self.scm.msg_focus = false;
    }

    /// A modifier key alone (the kitty keyboard protocol: `crate::all_keys`): an alt tap
    /// toggles the focus; true: it did.
    pub fn modifier(&mut self, k: KeyEvent) -> bool {
        use ratatui::crossterm::event::{KeyEventKind, ModifierKeyCode};
        let alt = matches!(k.code, KeyCode::Modifier(ModifierKeyCode::LeftAlt | ModifierKeyCode::RightAlt));
        match k.kind {
            KeyEventKind::Press if alt => self.alt_tap = true,
            KeyEventKind::Repeat if alt => {}
            KeyEventKind::Release if alt && std::mem::take(&mut self.alt_tap) && !self.typing() => {
                self.toggle_focus();
                return true;
            }
            _ => self.alt_tap = false,
        }
        false
    }

    /// A key other than a modifier: the next alt release is 0 tap.
    pub fn no_tap(&mut self) {
        self.alt_tap = false;
    }

    /// The side panel on / off (ctrl+b: from the view, from the prompt too).
    /// ctrl+b: hidden → shown with the keys; shown, the keys in the review → the keys to the
    /// panel; the panel with the keys → hidden (0 kitty protocol needed, as the alt tap is).
    pub fn toggle_panel(&mut self) {
        if self.scm.shown && !self.scm.focus {
            self.scm.focus = true;
            return;
        }

        self.scm.shown = !self.scm.shown;
        self.scm.focus = self.scm.shown;
        self.scm.msg_focus &= self.scm.shown;
    }

    /// The source control panel has the keys.
    pub fn panel_focus(&self) -> bool {
        self.scm.shown && self.scm.focus
    }

    pub fn key(&mut self, k: KeyEvent) -> Act {
        if matches!(k.code, KeyCode::Modifier(_)) {
            self.modifier(k);
            return Act::None;
        }
        self.alt_tap = false;
        if self.modal.is_some() {
            return self.modal_key(k);
        }
        if self.filtering {
            return self.filter_key(k);
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        // ctrl+b: the panel on / off; alt alone (a tap: press, release, 0 key between): the keys
        // to the panel / the review
        if ctrl && k.code == KeyCode::Char('b') {
            self.toggle_panel();
            return Act::None;
        }

        // a letter with ctrl / alt / ⌘: 0 review or panel key (alt+P never pushes); ctrl+r reads
        // again, ctrl+x deletes, as their plain letters; the commit message takes its own
        if !self.scm.msg_focus
            && let KeyCode::Char(c) = k.code
            && k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
            && !(ctrl && !k.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::SUPER) && matches!(c, 'r' | 'x'))
        {
            return Act::None;
        }

        if self.scm.focus {
            return self.panel_key(k);
        }
        let last = self.rows.len().saturating_sub(1);
        match k.code {
            // ↑↓: 1 line; j / ctrl+↑: 1 file up, k / ctrl+↓: 1 file down (the owner's pick)
            KeyCode::Down if !ctrl => self.at = (self.at + 1).min(last),
            KeyCode::Up if !ctrl => self.at = self.at.saturating_sub(1),
            KeyCode::Char('j') | KeyCode::Down => self.jump(true, |r| r.head),
            KeyCode::Char('k') | KeyCode::Up => self.jump(false, |r| r.head),
            KeyCode::PageDown => self.at = (self.at + self.page()).min(last),
            KeyCode::PageUp => self.at = self.at.saturating_sub(self.page()),
            KeyCode::Char('n') => self.jump(true, |r| r.hunk || r.head),
            KeyCode::Char('N') => self.jump(false, |r| r.hunk || r.head),
            KeyCode::Char('g') | KeyCode::Home => self.at = 0,
            KeyCode::Char('G') | KeyCode::End => self.at = last,
            // space: the line selected / not, the cursor 1 down
            KeyCode::Char(' ') => {
                let on = !self.selected(self.at);
                self.select_row(self.at, on);
                self.at = (self.at + 1).min(last);
            }
            // r (ctrl+r too): read again, as on every screen; v: reviewed
            KeyCode::Char('r') => {
                self.refresh(true);
                return Act::Toast("changes: reading git".into());
            }
            KeyCode::Char('v') => return self.toggle_reviewed(),
            KeyCode::Left => self.fold(true),
            KeyCode::Right => self.fold(false),
            KeyCode::Char('f' | '/') => self.filtering = true,
            KeyCode::Char('t') => return Act::ToggleChat,
            KeyCode::Esc => {
                if !self.filter.is_empty() {
                    self.set_filter("");
                    self.built = None;
                } else {
                    self.sel.clear();
                }
            }
            // ctrl+enter (⌘enter, alt+enter): the comment batch to the thread, as a send in the thread
            KeyCode::Enter if k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER) => {
                let Some(text) = batch(&self.comments) else { return Act::Toast("0 comments · c: comment on the line".into()) };
                self.comments.clear();
                self.save_comments();
                self.built = None;
                return Act::Send(text);
            }
            KeyCode::Enter => {
                if let Some(ci) = self.rows.get(self.at).and_then(|r| r.comment) {
                    self.edit_comment(ci);
                } else if let Some(f) = self.file_at() {
                    let on = !self.folded(f);
                    self.fold(on);
                }
            }
            KeyCode::Char('w') => {
                let now = self.side_by_side(self.area.width.saturating_sub(1));
                self.side = Some(!now);
                self.rebuild_on_file();
                return Act::Toast(if now { "unified".into() } else { "side by side".into() });
            }
            // s: the selected lines into the index (a part of each file); p: into the stash
            KeyCode::Char('s') | KeyCode::Char('p') => {
                let picks = self.picks();
                if picks.is_empty() {
                    let what = if self.sel.is_empty() { "0 lines selected · space / click: select" } else { "0 changed lines selected (context only)" };
                    return Act::Toast(what.into());
                }
                self.sel.clear();
                if k.code == KeyCode::Char('p') {
                    return self.stash_op(picks);
                }
                return self.start_op(Op::StageLines(picks.into_iter().map(|(p, _, pk)| (p, pk)).collect()));
            }
            KeyCode::Char('R') => {
                if self.classifier.is_none() {
                    return Act::Toast("classify: off (setting classify)".into());
                }
                self.apply_rank(true);
                self.built = None;
                return Act::Toast("classifying again…".into());
            }
            // a / u: on a box head the file, on a diff row its hunk (a new file: the file)
            KeyCode::Char('a') | KeyCode::Char('u') => {
                let stage = k.code == KeyCode::Char('a');
                if let (Some(f), Some(r)) = (self.file_at(), self.rows.get(self.at)) {
                    let (p, new) = (f.path.clone(), f.status == Status::New);
                    let hunk = (!r.head && !new).then_some(r.at).flatten();
                    let op = match (stage, hunk) {
                        (true, Some(l)) => Op::StageHunk(p, l),
                        (false, Some(l)) => Op::UnstageHunk(p, l),
                        (true, None) => Op::Stage(p),
                        (false, None) => Op::Unstage(p),
                    };
                    return self.start_op(op);
                }
            }
            KeyCode::Char('m') => self.focus_message(),
            KeyCode::Char('P') => return self.start_op(Op::Push),
            KeyCode::Char('U') => {
                if self.changes.base == "HEAD" {
                    return Act::Toast("0 remote base (on the default branch, or 0 origin)".into());
                }
                return self.start_op(Op::Update(self.changes.base.clone()));
            }
            KeyCode::Char('K') if self.changes.stopped.is_some() => return self.start_op(Op::Continue),
            KeyCode::Char('X') if self.changes.stopped.is_some() => return self.start_op(Op::Abort),
            KeyCode::Char('A') => {
                if let Some(c) = &self.changes.stopped {
                    return Act::Send(format!(
                        "The rebase on {} stopped with conflicts in: {}. Resolve each conflict (keep the intent of both sides), `git add` the files, then run `git -c core.editor=true rebase --continue`. Repeat until the rebase is done; report what you decided per conflict.",
                        self.changes.base,
                        c.join(", ")
                    ));
                }
            }
            // c: the comment modal on the selected lines, else on the cursor's line
            KeyCode::Char('c') => {
                let spots = self.spots();
                if !spots.is_empty() {
                    self.open_comment(spots);
                } else if let (Some(f), Some(r)) = (self.file_at(), self.rows.get(self.at)) {
                    let spot = Spot { path: f.path.clone(), line: r.at.unwrap_or_else(|| f.first_line()), lines: String::new() };
                    self.open_comment(vec![spot]);
                }
            }
            KeyCode::Char('x') => {
                if let Some(ci) = self.rows.get(self.at).and_then(|r| r.comment) {
                    self.comments.remove(ci);
                    self.save_comments();
                    self.rebuild_here();
                    return Act::Toast(format!("comment deleted · {} left", self.comments.len()));
                }
                if let Some(fi) = self.rows.get(self.at).and_then(|r| r.finding) {
                    self.drop_finding(fi);
                    self.rebuild_here();
                    return Act::Toast("finding dismissed".into());
                }
            }
            // y on a finding: into the comment batch (ctrl+enter sends it)
            KeyCode::Char('y') => {
                if let Some(fi) = self.rows.get(self.at).and_then(|r| r.finding)
                    && let Some(x) = self.review.as_ref().and_then(|r| r.findings.get(fi)).cloned()
                {
                    self.drop_finding(fi);
                    let level = crate::rank::Level::parse(&x.severity).unwrap_or_default();
                    self.comments.push(Comment { path: x.file.clone(), line: x.line, text: crate::review::as_comment(&x), lines: String::new(), quote: String::new(), level, by: String::new() });
                    self.save_comments();
                    self.rebuild_here();
                    return Act::Toast(format!("finding → comment batch · {} · ctrl+enter: send", self.comments.len()));
                }
            }
            KeyCode::Char('V') => return self.start_review(),
            // ship: open the PR (push + gh pr create --fill), merge on green, the lead fixes CI
            KeyCode::Char('O') => {
                if self.pr.is_some() {
                    return Act::Toast(format!("PR exists: {}", self.pr.as_ref().map(|p| p.url.clone()).unwrap_or_default()));
                }
                self.pr_at = None;
                return self.start_op(Op::OpenPr);
            }
            KeyCode::Char('M') => {
                if self.pr.is_none() {
                    return Act::Toast("0 PR · O: open one".into());
                }
                self.pr_at = None;
                return self.start_op(Op::Merge);
            }
            KeyCode::Char('F') => {
                if self.ci.is_some() {
                    return Act::Toast("CI: reading the log…".into());
                }
                let (tx, rx) = mpsc::channel();
                let dir = self.dir.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(ci_fix_message(&dir));
                });
                self.ci = Some(rx);
                return Act::Toast("CI: reading the failed log for the lead…".into());
            }
            // o / e: the IDE at the 1st selected line, else at the cursor's line
            KeyCode::Char('o') | KeyCode::Char('e') => {
                if let Some((path, line)) = self.open_spot() {
                    return Act::Open(path, line);
                }
            }
            _ => {}
        }
        Act::None
    }

    /// The rows change (a comment): the cursor stays on its row number.
    fn rebuild_here(&mut self) {
        let width = self.built.map_or(self.area.width.saturating_sub(1), |b| b.0);
        let at = self.at;
        self.built = None;
        self.build(width);
        self.at = at.min(self.rows.len().saturating_sub(1));
    }

    /// The rows change (fold, layout): the cursor stays on its file's header.
    fn rebuild_on_file(&mut self) {
        let file = self.rows.get(self.at).map(|r| r.file);
        let width = self.built.map_or(self.area.width.saturating_sub(1), |b| b.0);
        self.built = None;
        self.build(width);
        if let Some(i) = file
            && let Some(r) = self.rows.iter().position(|r| r.head && r.file == i)
        {
            self.at = r;
        }
    }

    /// r: the box under the cursor reviewed / not; reviewed: the cursor goes to the next box.
    fn toggle_reviewed(&mut self) -> Act {
        let Some(f) = self.file_at().cloned() else { return Act::None };
        let on = !self.is_reviewed(&f);
        if on {
            self.marks.insert(f.path.clone(), f.hash);
        } else {
            self.marks.remove(&f.path);
        }
        self.toggled.remove(&f.path);
        write_marks(&self.id, &self.marks);
        self.rebuild_on_file();
        if on {
            self.jump(true, |r| r.head);
        }
        let (done, all) = self.progress();
        Act::Toast(format!("{} · {done}/{all} reviewed", if on { "reviewed" } else { "not reviewed" }))
    }

    /// The row index under screen row Y of the rows' area (pinned rows included).
    fn row_at_y(&self, y: u16) -> Option<usize> {
        self.shown_rows.get(usize::from(y.checked_sub(self.area.y)?)).copied()
    }

    /// A click on the filter row: the filter takes the keys; on a row: the cursor there, its
    /// lines into the selection (a drag: the rows it passes; shift+click: the rows from the last
    /// click); the right button takes lines out (2 right clicks: the selection clears); a click
    /// on a comment opens it.
    pub fn mouse(&mut self, m: MouseEvent) -> Act {
        if self.modal.is_some() {
            return self.modal_mouse(m);
        }
        let at = ratatui::layout::Position { x: m.column, y: m.row };
        if self.drag.is_none() && (self.scm.rect.contains(at) || self.scm.picking()) {
            return self.panel_mouse(m);
        }
        let click = matches!(m.kind, MouseEventKind::Down(MouseButton::Left));
        // the filter input: its own mouse (cursor, selection + copy); a click on the row opens it
        if self.filtering && self.filter_ta.mouse(m) {
            self.filter_ta.copied = None;
            return Act::None;
        }
        if click && self.filter_rect.contains(at) {
            self.filtering = true;
            self.scm.focus = false;
            self.filter_ta.move_end();
            return Act::None;
        }
        let last = self.rows.len().saturating_sub(1);
        // a drag past the rows' top / bottom: the row there, the view scrolls 1
        if let (MouseEventKind::Drag(MouseButton::Left | MouseButton::Right), Some((anchor, base, add))) = (m.kind, self.drag.clone()) {
            let h = usize::from(self.area.height);
            let row = if m.row < self.area.y {
                self.top = self.top.saturating_sub(1);
                self.top
            } else if m.row >= self.area.bottom() {
                self.top = (self.top + 1).min(self.rows.len().saturating_sub(h));
                self.top + h.saturating_sub(1)
            } else {
                self.row_at_y(m.row).unwrap_or(self.top + usize::from(m.row - self.area.y))
            }
            .min(last);
            self.sel = base;
            for i in anchor.min(row)..=anchor.max(row) {
                self.select_row(i, add);
            }
            self.at = row;
            return Act::None;
        }
        if matches!(m.kind, MouseEventKind::Up(_)) {
            self.drag = None;
            return Act::None;
        }
        if !self.area.contains(at) {
            return Act::None;
        }
        match m.kind {
            MouseEventKind::ScrollDown => {
                self.top = (self.top + 3).min(self.rows.len().saturating_sub(usize::from(self.area.height)));
                self.at = self.at.max(self.top + 1).min(last);
            }
            MouseEventKind::ScrollUp => {
                self.top = self.top.saturating_sub(3);
                self.at = self.at.min(self.top + usize::from(self.area.height).saturating_sub(2));
            }
            MouseEventKind::Down(b @ (MouseButton::Left | MouseButton::Right)) => {
                self.filtering = false;
                self.scm.focus = false;
                let row = self.row_at_y(m.row).unwrap_or(self.top + usize::from(m.row - self.area.y)).min(last);
                self.at = row;
                if b == MouseButton::Right {
                    // 2 right clicks in a row: the whole selection clears
                    if self.right_at.is_some_and(|t| t.elapsed() < DOUBLE_CLICK) {
                        self.right_at = None;
                        self.sel.clear();
                        return Act::None;
                    }
                    self.right_at = Some(Instant::now());
                    self.drag = Some((row, self.sel.clone(), false));
                    self.select_row(row, false);
                    return Act::None;
                }
                if let Some(ci) = self.rows.get(row).and_then(|r| r.comment) {
                    self.edit_comment(ci);
                    return Act::None;
                }
                if m.modifiers.contains(KeyModifiers::SHIFT)
                    && let Some(a) = self.anchor
                {
                    for i in a.min(row)..=a.max(row) {
                        self.select_row(i, true);
                    }
                    return Act::None;
                }
                self.anchor = Some(row);
                self.drag = Some((row, self.sel.clone(), true));
                self.select_row(row, true);
            }
            _ => {}
        }
        Act::None
    }
}

/// The comment modal's text input with TEXT.
fn comment_area(text: &str) -> Box<crate::textarea::TextArea> {
    let mut t = crate::textarea::TextArea::new(false, "the comment · ctrl+⏎ save");
    t.style = Some(theme::panel());
    t.set_text(text);
    t.move_end();
    Box::new(t)
}

/// The code's background inside the boxes: the panel color, darker.
fn code_bg() -> ratatui::style::Color {
    theme::tint(ratatui::style::Color::Rgb(0, 0, 0), theme::pal().panel, 0.55)
}

/// The panel shows from this many columns.
const PANEL_MIN: u16 = 90;
/// 2 right clicks within this time: the selection clears.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// A modal over the changes view.
enum Modal {
    /// A comment: on SPOTS (new) or comment EDIT; the diff lines it is on.
    /// LEVEL: its severity; ON_LEVEL: the keys go to the severity row (←→), else the text.
    Comment { title: String, quote: Vec<QLine>, scroll: usize, edit: Option<usize>, spots: Vec<Spot>, text: Box<crate::textarea::TextArea>, level: crate::rank::Level, on_level: bool },
    /// A yes / no question; THEN runs on yes.
    Confirm { question: String, yes: bool, then: Then },
}

/// A line of the comment modal's selected lines: a file's path, a diff line, a gap between
/// files.
#[derive(Clone, Debug, PartialEq)]
enum QLine {
    Path(String),
    Code(String),
    Gap,
}

#[derive(Clone, Debug, PartialEq)]
enum Then {
    DropStash(String),
    ClearStash,
}

/// The patch pick of KEYS (selected lines) in F: added lines by number, removed lines by (the
/// new-side line they sit before, their text).
/// The worktree line of each line of hunk H: added and context lines their own, a removed line
/// the line it sat before.
fn worktree_lines(h: &Hunk) -> Vec<u32> {
    let mut n = h.new.max(1);
    h.lines
        .iter()
        .map(|l| match l.kind {
            Kind::Del => n,
            _ => {
                let at = l.new.unwrap_or(n);
                n = at + 1;
                at
            }
        })
        .collect()
}

fn pick_of(f: &FileDiff, keys: &std::collections::BTreeSet<LKey>) -> patch::Pick {
    let mut p = patch::Pick::default();
    for h in &f.hunks {
        let mut n = h.new;
        for l in &h.lines {
            match l.kind {
                Kind::Add => {
                    if keys.contains(&(false, l.new.unwrap_or(0))) {
                        p.adds.insert(n);
                    }
                    n += 1;
                }
                Kind::Ctx => n += 1,
                Kind::Del => {
                    if l.old.is_some_and(|o| keys.contains(&(true, o))) {
                        p.dels.insert((n, l.text.clone()));
                    }
                }
            }
        }
    }
    p
}

#[cfg(test)]
mod tests_support {
    use super::*;

    pub const DIFF: &str = "diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -10,4 +10,5 @@ fn run() {
 let a = 1;
-let b = 2;
+let b = 3;
+let c = 4;
 let d = 5;
diff --git a/tests/a_test.rs b/tests/a_test.rs
new file mode 100644
--- /dev/null
+++ b/tests/a_test.rs
@@ -0,0 +1,2 @@
+#[test]
+fn t() {}
diff --git a/old.rs b/new.rs
similarity index 100%
rename from old.rs
rename to new.rs
diff --git a/img.png b/img.png
Binary files a/img.png and b/img.png differ
";

    /// A view of DIFF, 0 stored state.
    pub fn test_view() -> View {
        let mut files = parse(DIFF);
        order(&mut files);
        let id = format!("changes-scm-{}", std::process::id());
        let mut v = View::new(&id, Path::new("/tmp"));
        v.marks.clear();
        v.comments.clear();
        v.stash.clear();
        v.changes = Changes { base: "origin/main".into(), files, ..Default::default() };
        v.loaded = Some(Instant::now());
        v
    }

    /// The view drawn at W x H: its rows of text.
    pub fn draw(v: &mut View, w: u16, h: u16) -> Vec<String> {
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        t.draw(|f| v.draw(f, f.area())).unwrap();
        let b = t.backend().buffer().clone();
        (0..h).map(|y| (0..w).map(|x| b[(x, y)].symbol().to_string()).collect()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = "diff --git a/src/a.rs b/src/a.rs
index 1..2 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -10,4 +10,5 @@ fn run() {
 let a = 1;
-let b = 2;
+let b = 3;
+let c = 4;
 let d = 5;
\\ No newline at end of file
diff --git a/tests/a_test.rs b/tests/a_test.rs
new file mode 100644
--- /dev/null
+++ b/tests/a_test.rs
@@ -0,0 +1,2 @@
+#[test]
+fn t() {}
diff --git a/old.rs b/new.rs
similarity index 100%
rename from old.rs
rename to new.rs
diff --git a/img.png b/img.png
Binary files a/img.png and b/img.png differ
";

    #[test]
    fn parse_reads_hunks_numbers_and_file_kinds() {
        let f = parse(DIFF);
        assert_eq!(f.len(), 4);
        assert_eq!((f[0].path.as_str(), f[0].add, f[0].del), ("src/a.rs", 2, 1));
        assert_eq!(f[0].hunks[0].ctx, "fn run() {");
        let ls = &f[0].hunks[0].lines;
        assert_eq!((ls[0].old, ls[0].new), (Some(10), Some(10)));
        assert_eq!((ls[1].kind, ls[1].old, ls[1].new), (Kind::Del, Some(11), None));
        assert_eq!((ls[2].kind, ls[2].new), (Kind::Add, Some(11)));
        assert_eq!((ls[4].old, ls[4].new), (Some(12), Some(13)));
        assert_eq!(ls.len(), 5, "0 line for `\\ No newline`");
        assert_eq!((f[1].status, f[1].cat), (Status::New, Cat::Tests));
        assert_eq!((f[2].status, f[2].path.as_str(), f[2].old_path.as_deref()), (Status::Renamed, "new.rs", Some("old.rs")));
        assert!(f[3].binary);
        assert_eq!(f[0].first_line(), 11);
    }

    #[test]
    fn order_puts_schema_and_logic_first_tests_and_locks_last() {
        let mk = |p: &str, n: usize| {
            let mut f = FileDiff::new(p.into());
            f.add = n;
            f
        };
        let mut v = vec![mk("Cargo.lock", 900), mk("src/ui/button.css", 5), mk("tests/x.rs", 50), mk("README.md", 3), mk("src/deploy.rs", 10), mk("migrations/001_init.sql", 2), mk("src/big.rs", 40), mk("package.json", 1)];
        order(&mut v);
        let paths: Vec<&str> = v.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["migrations/001_init.sql", "src/big.rs", "src/deploy.rs", "package.json", "src/ui/button.css", "README.md", "tests/x.rs", "Cargo.lock"]);
    }

    fn view(mut files: Vec<FileDiff>) -> View {
        order(&mut files);
        let mut v = View::new("changes-test-0000", Path::new("/tmp"));
        v.marks.clear();
        v.scm.shown = false;
        v.changes = Changes { base: "origin/main".into(), files, ..Default::default() };
        v.loaded = Some(Instant::now());
        v
    }

    fn text(v: &View) -> Vec<String> {
        (0..v.rows.len()).map(|i| text_of(v, i)).collect()
    }

    #[test]
    fn boxes_unified_and_side_by_side_by_width() {
        let mut v = view(parse(DIFF));
        v.build(120);
        let t = text(&v);
        let h = t.iter().position(|l| l.starts_with("┌─ ☐ src/a.rs")).unwrap();
        assert!(t[h].ends_with("logic · +2 −1 ─┐") && summary(&v).contains("4 files"), "{t:#?}");
        assert!(t.iter().any(|l| l.contains("⋯ fn run() {")), "{t:#?}");
        assert!(t.iter().any(|l| l.contains(" 11     - let b = 2;")), "{t:#?}");
        assert!(t.iter().any(|l| l.contains("     11 + let b = 3;")), "{t:#?}");
        assert!(t.iter().filter(|l| l.starts_with(['┌', '│', '└'])).all(|l| l.width() == 120), "every box row 120 wide: {t:#?}");
        // binary: folded
        assert!(!t.iter().any(|l| l.contains("binary file")));
        v.build(SIDE_MIN);
        let t = text(&v);
        let row = t.iter().find(|l| l.contains("let b = 2;")).unwrap();
        assert!(row.contains("- let b = 2;") && row.contains("+ let b = 3;"), "pairs on 1 row: {row}");
        assert!(t.iter().any(|l| l.contains("+ let c = 4;") && !l.contains("- ")), "{t:#?}");
        // a new file stays unified: 0 empty left half
        let new = t.iter().find(|l| l.contains("fn t() {}")).unwrap();
        assert!(!new.contains(" │ "), "{new}");
    }

    #[test]
    fn a_rank_orders_groups_and_shows_whys() {
        use crate::rank::{Group, RFile, Rank};
        let mut v = view(parse(DIFF));
        let key = crate::rank::key(&v.changes.files);
        let f = |p: &str, w: &str| RFile { path: p.into(), why: w.into(), level: "high".into() };
        v.rank = Some(Rank { key, model: "haiku".into(), groups: vec![
            Group { title: "Test first".into(), why: "the test drives it".into(), files: vec![f("tests/a_test.rs", "adds the test")] },
            Group { title: "Core".into(), why: "b becomes 3".into(), files: vec![f("src/a.rs", "changes b"), f("new.rs", "")] },
        ] });
        v.apply_rank(false);
        let paths: Vec<&str> = v.changes.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["tests/a_test.rs", "src/a.rs", "new.rs", "img.png"], "unranked files after the groups");
        v.build(100);
        let t = text(&v);
        assert!(summary(&v).ends_with("order: haiku"), "{t:#?}");
        let g1 = t.iter().position(|l| l.starts_with(" 1 · Test first  the test drives it")).unwrap();
        assert!(t[g1 + 1].contains("☐ tests/a_test.rs") && t[g1 + 2].contains("adds the test"), "{t:#?}");
        assert!(t.iter().any(|l| l.starts_with(" 2 · Core")), "{t:#?}");
        // the agent's levels: the 3 high; img.png (unranked, path rules: low) after them, under
        // its category
        assert_eq!(v.counts, [(0, 0), (0, 3), (0, 0), (0, 1)]);
        let img = t.iter().position(|l| l.contains("img.png")).unwrap();
        assert_eq!(t[img - 1], " logic", "{t:#?}");
        // a new set of paths: the rank no longer applies
        v.changes.files.pop();
        v.built = None;
        v.build(100);
        assert!(summary(&v).ends_with("path order"));
    }

    #[test]
    fn comments_sit_under_their_line_and_go_out_as_1_batch() {
        let mut v = view(parse(DIFF));
        v.id = "changes-comments-0000".into();
        v.comments.clear();
        v.build(100);
        let spot = |line: u32| vec![Spot { path: "src/a.rs".into(), line, lines: String::new() }];
        v.add_comment(&spot(11), "why 3?\nkeep 2");
        v.add_comment(&spot(99), "outside any hunk");
        let t = text(&v);
        let at = t.iter().position(|l| l.contains("+ let b = 3;")).unwrap();
        assert!(t[at + 1].contains("✎ 11: why 3?") && t[at + 2].contains("keep 2"), "{t:#?}");
        let end = t.iter().position(|l| l.contains("outside any hunk")).unwrap();
        assert!(t[end + 1].starts_with('└'), "orphan at the box end: {t:#?}");
        // x on a comment row deletes it
        v.at = v.rows.iter().position(|r| r.comment == Some(1)).unwrap();
        v.key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(v.comment_count(), 1);
        match v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)) {
            Act::Send(s) => assert_eq!(s, "Review comments on the changes (file:line, new side). Address each one:\n- [medium] `src/a.rs:11`: why 3?\n  keep 2"),
            a => panic!("{a:?}"),
        }
        assert_eq!(v.comment_count(), 0);
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("changes-comments-0000"));
    }

    #[test]
    fn findings_sit_under_their_line_y_takes_x_drops() {
        use crate::review::{Finding, Review};
        let mut v = view(parse(DIFF));
        v.id = "changes-findings-0000".into();
        v.comments.clear();
        let f = |line: u32, sev: &str, t: &str| Finding { file: "src/a.rs".into(), line, severity: sev.into(), title: t.into(), body: "what breaks".into() };
        v.review = Some(Review { model: "sonnet".into(), findings: vec![f(12, "major", "c is never read"), f(10, "minor", "a is shadowed")] });
        v.build(100);
        let t = text(&v);
        let at = t.iter().position(|l| l.contains("+ let c = 4;")).unwrap();
        assert!(t[at + 1].contains("⚠ major · c is never read") && t[at + 2].contains("what breaks"), "{t:#?}");
        assert!(summary(&v).contains("2 findings (sonnet)"), "{t:#?}");
        // y: the finding becomes a comment
        v.at = v.rows.iter().position(|r| r.finding == Some(0)).unwrap();
        v.key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
        assert_eq!(v.comment_count(), 1);
        assert_eq!(v.comments[0].text, "[major] c is never read: what breaks");
        assert_eq!(v.review.as_ref().unwrap().findings.len(), 1);
        // x: dismissed
        v.at = v.rows.iter().position(|r| r.finding == Some(0)).unwrap();
        v.key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(v.review.as_ref().unwrap().findings.is_empty());
        assert_eq!(v.comment_count(), 1, "x on a finding: 0 comment deleted");
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("changes-findings-0000"));
    }

    #[test]
    fn word_marks_hit_only_the_changed_tokens() {
        let c = |s: &str| s.chars().collect::<Vec<char>>();
        let (o, n) = word_marks(&c("let b = 2;"), &c("let b = 3;"));
        let on = |v: &[bool], s: &str| s.chars().zip(v).filter(|x| *x.1).map(|x| x.0).collect::<String>();
        assert_eq!((on(&o, "let b = 2;"), on(&n, "let b = 3;")), ("2".into(), "3".into()));
        // a rewrite: 0 marks
        let (o, n) = word_marks(&c("alpha beta gamma"), &c("one two three"));
        assert!(!o.contains(&true) && !n.contains(&true));
    }

    #[test]
    fn rows_have_vivid_syntax_colors_a_sign_column_and_word_marks() {
        let mut v = view(parse(DIFF));
        v.build(100);
        let i = (0..v.rows.len()).find(|&i| text_of(&v, i).contains("+ let b = 3;")).unwrap();
        let line = v.line(i);
        let (sign_bg, mark_bg) = tints(Kind::Add);
        // the sign column: the add color as its background; the rest: 0 line tint
        let sign = line.spans.iter().find(|s| s.content == " + ").unwrap();
        assert_eq!(sign.style.bg, sign_bg);
        // `let`: a keyword color, more saturated than the theme's
        let kw = line.spans.iter().find(|s| s.content.starts_with("let")).unwrap();
        assert_ne!(kw.style.fg, Some(theme::pal().text), "syntax color");
        assert_eq!(kw.style.bg, None, "0 tint over the line");
        let plain = crate::code::highlight("let b = 3;", "rs")[0][0].0;
        assert_eq!(kw.style.fg, Some(theme::desaturate(plain, SATURATION)));
        // `3`: the changed word, its tint
        let three = line.spans.iter().find(|s| s.content.contains('3')).unwrap();
        assert_eq!(three.style.bg, mark_bg, "{:?}", line.spans);
        // context rows: 0 tint
        let ctx = (0..v.rows.len()).find(|&i| text_of(&v, i).contains("let a = 1;")).unwrap();
        assert!(v.line(ctx).spans.iter().filter(|s| s.content.contains("let")).all(|s| s.style.bg.is_none()));
    }

    #[test]
    fn v_marks_reviewed_folds_and_a_changed_diff_shows_updated() {
        let mut v = view(parse(DIFF));
        v.build(100);
        v.at = v.rows.iter().position(|r| r.head).unwrap();
        v.key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));
        assert_eq!(v.progress(), (1, 4));
        let t = text(&v);
        let h = t.iter().position(|l| l.contains("☑ src/a.rs")).unwrap();
        assert!(t[h].starts_with("── ☑") && t[h].ends_with("──") && !t[h + 1].starts_with('│'), "folded: 1 line: {t:#?}");
        // the cursor went to the next box (medium: src/a.rs, new.rs)
        assert!(v.rows[v.at].head && v.changes.files[v.rows[v.at].file].path == "new.rs");
        // the file changes: the mark goes, `updated` shows
        let mut changed = parse(&DIFF.replace("let c = 4;", "let c = 5;"));
        order(&mut changed);
        v.changes.files = changed;
        v.built = None;
        v.build(100);
        let t = text(&v);
        let head = t.iter().find(|l| l.contains("src/a.rs")).unwrap();
        assert!(head.contains("☐ src/a.rs updated"), "{head}");
        assert_eq!(v.progress().0, 0);
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("changes-test-0000"));
    }

    #[test]
    fn o_opens_the_line_under_the_cursor() {
        let mut v = view(parse(DIFF));
        v.build(100);
        let i = (0..v.rows.len()).find(|&i| text_of(&v, i).contains("let c = 4;")).unwrap();
        v.at = i;
        assert_eq!(v.key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE)), Act::Open("src/a.rs".into(), 12));
        // a removed line: the worktree line it sat before (its old number: 11 as well here)
        let del = (0..v.rows.len()).find(|&i| text_of(&v, i).contains("- let b = 2;")).unwrap();
        v.at = del;
        assert_eq!(v.key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)), Act::Open("src/a.rs".into(), 11));
        // the selection wins: its 1st line, also with the cursor in an other file
        select(&mut v, "+ let c = 4;");
        select(&mut v, "+ fn t() {}");
        v.at = (0..v.rows.len()).find(|&i| text_of(&v, i).contains("+ fn t() {}")).unwrap();
        assert_eq!(v.key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)), Act::Open("tests/a_test.rs".into(), 2));
        v.at = v.rows.iter().position(|r| r.head).unwrap();
        assert_eq!(v.key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)), Act::Open("src/a.rs".into(), 12));
        v.sel.clear();
        // j: the next box head
        v.at = v.rows.iter().position(|r| r.head).unwrap();
        v.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert!(v.rows[v.at].head && v.changes.files[v.rows[v.at].file].path == "new.rs");
    }

    fn summary(v: &View) -> String {
        v.summary().spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn text_of(v: &View, i: usize) -> String {
        v.line(i).spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// The changes view drawn at W x H: its rows of text.
    fn screen(v: &mut View, w: u16, h: u16) -> Vec<String> {
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        t.draw(|f| v.draw(f, f.area())).unwrap();
        let b = t.backend().buffer().clone();
        (0..h).map(|y| (0..w).map(|x| b[(x, y)].symbol().to_string()).collect()).collect()
    }

    fn press(v: &mut View, c: KeyCode) -> Act {
        v.key(KeyEvent::new(c, KeyModifiers::NONE))
    }

    /// The screen row that holds S.
    fn row_of(s: &[String], pat: &str) -> usize {
        s.iter().position(|l| l.contains(pat)).unwrap_or_else(|| panic!("{pat}: {s:#?}"))
    }

    #[test]
    fn arrows_go_1_line_j_k_and_ctrl_arrows_1_file_the_box_border_lights_up() {
        let mut v = view(parse(DIFF));
        let s = screen(&mut v, 100, 30);
        // the cursor (▌ at the right edge) on the 1st box's head
        let head = row_of(&s, "☐ src/a.rs");
        let next = row_of(&s, "☐ old.rs → new.rs");
        assert!(s[head].ends_with('▌'), "{s:#?}");
        press(&mut v, KeyCode::Down);
        let s = screen(&mut v, 100, 30);
        assert!(s[head + 1].ends_with('▌') && s[head + 1].contains("⋯ fn run() {"), "↓: 1 down: {s:#?}");
        press(&mut v, KeyCode::Down);
        press(&mut v, KeyCode::Up);
        let s = screen(&mut v, 100, 30);
        assert!(s[head + 1].ends_with('▌'), "↑: 1 up: {s:#?}");
        // the focused box: accent border; the other box: the line color
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        t.draw(|f| v.draw(f, f.area())).unwrap();
        let buf = t.backend().buffer().clone();
        assert_eq!(buf[(0, head as u16 + 1)].fg, theme::pal().accent);
        assert_ne!(buf[(0, next as u16 + 1)].fg, theme::pal().accent);
        press(&mut v, KeyCode::Char('j'));
        let s = screen(&mut v, 100, 30);
        assert!(s[next].ends_with('▌'), "j: 1 file down: {s:#?}");
        press(&mut v, KeyCode::Char('k'));
        let s = screen(&mut v, 100, 30);
        assert!(s[head].ends_with('▌'), "k: 1 file up: {s:#?}");
        v.key(KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL));
        let s = screen(&mut v, 100, 30);
        assert!(s[next].ends_with('▌'), "ctrl+↓: 1 file down: {s:#?}");
        v.key(KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL));
        let s = screen(&mut v, 100, 30);
        assert!(s[head].ends_with('▌'), "ctrl+↑: 1 file up: {s:#?}");
    }

    #[test]
    fn space_selects_lines_apart_and_c_comments_on_them() {
        let mut v = view(parse(DIFF));
        v.id = "changes-select-0000".into();
        v.comments.clear();
        let s = screen(&mut v, 100, 30);
        let b2 = row_of(&s, "- let b = 2;");
        // the cursor to `- let b = 2;`, space: selected, the cursor 1 down
        while !screen(&mut v, 100, 30)[b2].ends_with('▌') {
            press(&mut v, KeyCode::Down);
        }
        press(&mut v, KeyCode::Char(' '));
        let s = screen(&mut v, 100, 30);
        assert!(s[b2 + 1].ends_with('▌'), "1 down: {s:#?}");
        // skip `+ let b = 3;`, select `+ let c = 4;`: 2 lines apart
        press(&mut v, KeyCode::Down);
        press(&mut v, KeyCode::Char(' '));
        let s = screen(&mut v, 100, 30);
        let mark = |y: usize| s[y].chars().nth(1) == Some('▌');
        assert!(mark(b2) && !mark(b2 + 1) && mark(b2 + 2), "gutter marks on the selected lines: {s:#?}");
        // the selection's tint on its text, 0 on the other lines
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        t.draw(|f| v.draw(f, f.area())).unwrap();
        let buf = t.backend().buffer().clone();
        let _ = &buf;
        let text_x = s[b2].find("let").map(|x| s[b2][..x].chars().count() as u16).unwrap();
        assert_ne!(buf[(text_x, b2 as u16)].bg, code_bg());
        assert_eq!(buf[(text_x, b2 as u16 + 1)].bg, code_bg(), "0 selection: the code background");
        // c: the modal, 1 comment on both lines; the modal shows them
        press(&mut v, KeyCode::Char('c'));
        let Some(Modal::Comment { spots, quote, .. }) = &v.modal else { panic!() };
        assert_eq!(spots, &[Spot { path: "src/a.rs".into(), line: 12, lines: "12 · removed 11".into() }]);
        assert_eq!(spots[0].label(), "src/a.rs:12 · removed 11");
        assert_eq!(quote, &[QLine::Code("- let b = 2;".into()), QLine::Code("+ let c = 4;".into())]);
        assert!(v.typing());
        let s = screen(&mut v, 100, 30);
        assert!(s.iter().any(|l| l.contains("comment on src/a.rs:12 · removed 11")) && s.iter().any(|l| l.contains("- let b = 2;")), "{s:#?}");
        for c in "both wrong".chars() {
            press(&mut v, KeyCode::Char(c));
        }
        v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        assert!(v.modal.is_none() && v.comments[0].quote == "- let b = 2;\n+ let c = 4;");
        let s = screen(&mut v, 100, 30);
        let c = row_of(&s, "✎ 12 · removed 11: both wrong");
        assert!(s[c - 1].contains("+ let c = 4;"), "under the last selected line: {s:#?}");
        assert!(!s.iter().any(|l| l.chars().nth(1) == Some('▌')), "the selection clears: {s:#?}");
        let Act::Send(b) = v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)) else { panic!() };
        assert!(b.ends_with("- [medium] `src/a.rs:12 · removed 11`: both wrong"), "{b}");
        // esc (the view's while a selection is on) clears it
        v.at = (0..v.rows.len()).find(|&i| text_of(&v, i).contains("+ let b = 3;")).unwrap();
        press(&mut v, KeyCode::Char(' '));
        assert!(v.takes_esc());
        press(&mut v, KeyCode::Esc);
        assert!(!v.takes_esc() && v.sel.is_empty());
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("changes-select-0000"));
    }

    #[test]
    fn left_folds_right_opens_the_box() {
        let mut v = view(parse(DIFF));
        let s = screen(&mut v, 100, 30);
        assert!(s.iter().any(|l| l.contains("let b = 3;")));
        press(&mut v, KeyCode::Down);
        press(&mut v, KeyCode::Down);
        press(&mut v, KeyCode::Left);
        let s = screen(&mut v, 100, 30);
        let head = row_of(&s, "☐ src/a.rs");
        assert!(s[head].starts_with("── ☐") && !s.iter().any(|l| l.contains("let b = 3;")), "folded: {s:#?}");
        assert!(s[head].ends_with('▌'), "the cursor on the head: {s:#?}");
        press(&mut v, KeyCode::Right);
        let s = screen(&mut v, 100, 30);
        assert!(s.iter().any(|l| l.contains("let b = 3;")), "open: {s:#?}");
    }

    #[test]
    fn every_level_shows_and_the_summary_stays_on_top() {
        let mut v = view(parse(DIFF));
        let s = screen(&mut v, 100, 12);
        assert!(s[0].contains("4 files") && s[1].contains("filter by name"), "{s:#?}");
        // all files, by level: src/a.rs (medium) before tests/a_test.rs (low)
        let all = v.rows.iter().filter(|r| r.head).map(|r| v.changes.files[r.file].path.clone()).collect::<Vec<_>>();
        assert_eq!(all[0], "src/a.rs");
        assert!(all.contains(&"tests/a_test.rs".to_string()) && all.contains(&"img.png".to_string()), "{all:?}");
        // scrolled into the box: the summary stays, the box head pins under it
        for _ in 0..6 {
            press(&mut v, KeyCode::Down);
        }
        v.top = v.rows.iter().position(|r| r.body == Body::Diff(0, 2)).unwrap();
        v.at = v.top + 2;
        let s = screen(&mut v, 100, 12);
        assert!(s[0].contains("4 files"), "{s:#?}");
        assert!(s[2].starts_with("┌─ ☐ src/a.rs"), "the head pinned: {s:#?}");
        assert!(!s.iter().skip(3).any(|l| l.contains("┌─ ☐ src/a.rs")), "{s:#?}");
        // a call on its way: classifying…
        let (_tx, rx) = mpsc::channel();
        v.ranking = Some(rx);
        assert!(screen(&mut v, 100, 12)[0].contains("classifying…"));
    }

    #[test]
    fn f_filters_by_name_and_content_with_marks_esc_clears_enter_jumps() {
        let mut v = view(parse(DIFF));
        screen(&mut v, 100, 30);
        press(&mut v, KeyCode::Char('f'));
        assert!(v.typing() && v.takes_esc());
        // `fn t()`: content of the test file (the low tab) only: a hit across the tabs
        for c in "fn t()".chars() {
            press(&mut v, KeyCode::Char(c));
        }
        let s = screen(&mut v, 100, 30);
        assert!(s[1].contains(" f  fn t()"), "{}", s[1]);
        assert!(s.iter().any(|l| l.contains("1 files match")), "{s:#?}");
        assert!(s.iter().any(|l| l.contains("tests/a_test.rs")) && !s.iter().any(|l| l.contains("src/a.rs")), "content hit across tabs: {s:#?}");
        press(&mut v, KeyCode::Esc);
        assert!(!v.typing() && v.filter.is_empty());
        // a name hit: its chars marked
        press(&mut v, KeyCode::Char('f'));
        for c in "test".chars() {
            press(&mut v, KeyCode::Char(c));
        }
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        t.draw(|f| v.draw(f, f.area())).unwrap();
        let buf = t.backend().buffer().clone();
        let s: Vec<String> = (0..30).map(|y| (0..100).map(|x| buf[(x, y)].symbol().to_string()).collect()).collect();
        let y = row_of(&s, "tests/a_test.rs");
        let x0 = s[y].find("tests/").map(|b| s[y][..b].chars().count() as u16).unwrap();
        // a mark: accent text, on the cursor row underlined (the row is on the accent)
        let marked = |x: u16| buf[(x, y as u16)].fg == theme::pal().accent || buf[(x, y as u16)].modifier.contains(ratatui::style::Modifier::UNDERLINED);
        assert!(marked(x0), "a marked char");
        assert!(!marked(x0 + 4), "`s` of `tests`: 0 mark");
        // enter: the cursor on the 1st hit, the keys back to the list; esc clears the filter
        press(&mut v, KeyCode::Enter);
        assert!(!v.typing() && v.rows[v.at].head && v.changes.files[v.rows[v.at].file].path == "tests/a_test.rs");
        assert!(v.takes_esc());
        press(&mut v, KeyCode::Esc);
        assert!(v.filter.is_empty());
    }

    #[test]
    fn a_re_read_of_the_same_diffs_keeps_the_rows_and_takes_the_git_state() {
        let mut v = view(parse(DIFF));
        screen(&mut v, 100, 30);
        for _ in 0..4 {
            press(&mut v, KeyCode::Down);
        }
        let at = v.at;
        let read = |v: &mut View, c: Changes| {
            let (tx, rx) = mpsc::channel();
            tx.send(Ok(c)).unwrap();
            v.loading = Some(rx);
            v.poll()
        };
        // the same files in the read's (path) order, 1 staged now
        let mut files = parse(DIFF);
        files.reverse();
        let c = Changes { base: "origin/main".into(), files, staged: HashMap::from([("src/a.rs".to_string(), true)]), ..Default::default() };
        assert!(read(&mut v, c.clone()), "the staged mark changed");
        assert!(v.built.is_some() && v.at == at, "0 new rows, the cursor stays");
        assert!(screen(&mut v, 100, 30).iter().any(|l| l.contains("☐ ● src/a.rs")));
        assert!(!read(&mut v, c), "0 change: 0 frame");
        // a changed diff of the cursor's file: the cursor stays on its row of the file
        let mut files = parse(&DIFF.replace("let d = 5;", "let d = 6;"));
        order(&mut files);
        assert!(read(&mut v, Changes { base: "origin/main".into(), files, ..Default::default() }));
        screen(&mut v, 100, 30);
        assert_eq!(v.at, at);
    }

    #[test]
    fn a_mouse_drag_selects_the_lines_it_passes() {
        let mut v = view(parse(DIFF));
        let s = screen(&mut v, 100, 30);
        let (a, b) = (row_of(&s, "- let b = 2;"), row_of(&s, "+ let c = 4;"));
        let ev = |kind, row: usize| MouseEvent { kind, column: 20, row: row as u16, modifiers: KeyModifiers::NONE };
        v.mouse(ev(MouseEventKind::Down(MouseButton::Left), a));
        v.mouse(ev(MouseEventKind::Drag(MouseButton::Left), a + 1));
        v.mouse(ev(MouseEventKind::Drag(MouseButton::Left), b));
        v.mouse(ev(MouseEventKind::Up(MouseButton::Left), b));
        let s = screen(&mut v, 100, 30);
        assert!((a..=b).all(|y| s[y].chars().nth(1) == Some('▌')), "{s:#?}");
        assert!(s[b + 1].chars().nth(1) != Some('▌'), "{s:#?}");
        assert_eq!(v.sel["src/a.rs"].len(), 3);
        // a click adds its line; a right click takes 1 out; 2 right clicks: 0 selection
        v.mouse(ev(MouseEventKind::Down(MouseButton::Left), b + 1));
        v.mouse(ev(MouseEventKind::Up(MouseButton::Left), b + 1));
        assert_eq!(v.sel["src/a.rs"].len(), 4);
        v.mouse(ev(MouseEventKind::Down(MouseButton::Right), a));
        v.mouse(ev(MouseEventKind::Up(MouseButton::Right), a));
        assert_eq!(v.sel["src/a.rs"].len(), 3);
        v.mouse(ev(MouseEventKind::Down(MouseButton::Right), b));
        assert!(v.sel.is_empty(), "a double right click clears");
        // shift+click: the rows from the last click
        v.mouse(ev(MouseEventKind::Down(MouseButton::Left), a));
        v.mouse(ev(MouseEventKind::Up(MouseButton::Left), a));
        v.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 20, row: b as u16, modifiers: KeyModifiers::SHIFT });
        assert_eq!(v.sel["src/a.rs"].len(), 3);
        // a right drag takes the rows it passes out
        v.right_at = None;
        v.mouse(ev(MouseEventKind::Down(MouseButton::Right), a));
        v.mouse(ev(MouseEventKind::Drag(MouseButton::Right), b));
        v.mouse(ev(MouseEventKind::Up(MouseButton::Right), b));
        assert!(v.sel.is_empty());
    }

    #[test]
    fn a_hunk_stages_and_unstages_alone() {
        let d = std::env::temp_dir().join(format!("codehead-hunks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let sh = |c: &str| assert!(Command::new("sh").arg("-c").arg(c).current_dir(&d).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap().success(), "{c}");
        sh("git init -q && git config user.email t@t && git config user.name t && seq 1 20 > x.txt && git add -A && git commit -qm init");
        // 2 hunks: line 3 changed, line 18 removed
        sh("seq 1 20 | sed 's/^3$/three/' | sed '/^18$/d' > x.txt");
        run_op(&d, &Op::StageHunk("x.txt".into(), 3)).unwrap();
        let cached = git(&d, &["diff", "--cached", "-U0"]).unwrap();
        assert!(cached.contains("+three") && !cached.contains("-18"), "{cached}");
        let unstaged = git(&d, &["diff", "-U0"]).unwrap();
        assert!(unstaged.contains("-18") && !unstaged.contains("+three"), "{unstaged}");
        // the removal: its line is the one after it (18 in the new file is 19)
        run_op(&d, &Op::StageHunk("x.txt".into(), 18)).unwrap();
        assert!(git(&d, &["diff", "-U0"]).unwrap().is_empty());
        run_op(&d, &Op::UnstageHunk("x.txt".into(), 3)).unwrap();
        let cached = git(&d, &["diff", "--cached", "-U0"]).unwrap();
        assert!(!cached.contains("+three") && cached.contains("-18"), "{cached}");
        assert!(run_op(&d, &Op::StageHunk("x.txt".into(), 10)).is_err(), "0 hunk at line 10");
        let _ = std::fs::remove_dir_all(d);
    }

    /// A view of a real repo D (its changes read now), the cursor's thread ID.
    fn real_view(d: &Path, id: &str) -> View {
        let mut v = View::new(id, d);
        v.scm.shown = false;
        v.changes = load(d).unwrap();
        v.loaded = Some(Instant::now());
        v.stash.clear();
        v.build(120);
        v
    }

    /// Select the rows whose text has PAT.
    fn select(v: &mut View, pat: &str) {
        let rows: Vec<usize> = (0..v.rows.len()).filter(|&i| text_of(v, i).contains(pat)).collect();
        assert!(!rows.is_empty(), "{pat}");
        for i in rows {
            v.select_row(i, true);
        }
    }

    /// The git step on its way ends (its toast note back).
    fn wait_op(v: &mut View) -> String {
        for _ in 0..200 {
            if v.poll_op() {
                return v.take_note().unwrap_or_default();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("git step: 0 end");
    }

    #[test]
    fn s_stages_only_the_selected_lines_p_stashes_them() {
        let d = std::env::temp_dir().join(format!("codehead-lines-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let sh = |c: &str| assert!(Command::new("sh").arg("-c").arg(c).current_dir(&d).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap().success(), "{c}");
        sh("git init -q && git config user.email t@t && git config user.name t && seq 1 20 > x.txt && git add -A && git commit -qm init");
        // 1 hunk, 2 changes: 3 → three, 5 → five; a new file of 3 lines
        sh("seq 1 20 | sed 's/^3$/three/;s/^5$/five/' > x.txt && printf 'a\\nb\\nc\\n' > n.txt");
        let id = format!("changes-lines-{}", std::process::id());
        let mut v = real_view(&d, &id);
        select(&mut v, "+ three");
        select(&mut v, "- 3");
        select(&mut v, "+ b");
        press(&mut v, KeyCode::Char('s'));
        assert!(wait_op(&mut v).contains("staged the selected lines"), "{:?}", v.note);
        let cached = git(&d, &["diff", "--cached", "-U0"]).unwrap();
        assert!(cached.contains("-3\n+three") && !cached.contains("five") && cached.contains("+b") && !cached.contains("+a"), "{cached}");
        assert!(v.sel.is_empty(), "the selection clears");
        // p: `five` out of the worktree, into the stash; its agent (0 known: the lead) asked for a name
        let mut v = real_view(&d, &id);
        select(&mut v, "+ five");
        select(&mut v, "- 5");
        press(&mut v, KeyCode::Char('p'));
        assert!(wait_op(&mut v).contains("stashed x.txt"), "{:?}", v.note);
        let x = std::fs::read_to_string(d.join("x.txt")).unwrap();
        assert!(x.contains("\nthree\n4\n5\n6\n"), "{x}");
        assert_eq!(v.stash.len(), 1);
        assert_eq!((v.stash[0].path.as_str(), v.stash[0].lines.as_str(), v.stash[0].asked), ("x.txt", "5 · removed 5", true));
        let ask = v.take_send().unwrap();
        assert!(ask.contains("`stash_name`") && ask.contains(&v.stash[0].id) && ask.contains("+five"), "{ask}");
        // the patch puts it back
        let p = d.join("p.diff");
        std::fs::write(&p, &v.stash[0].patch).unwrap();
        sh("git apply p.diff && rm p.diff");
        assert!(std::fs::read_to_string(d.join("x.txt")).unwrap().contains("\nfive\n"));
        // restore: the message to the agent, the entry marked
        let eid = v.stash[0].id.clone();
        let _ = v.restore(&eid);
        let msg = v.take_send().unwrap();
        assert!(msg.contains("`stash_restored`") && msg.contains("ask the owner"), "{msg}");
        assert!(v.stash[0].restoring);
        // the agent names it, then reports it restored (the MCP tools)
        stash::set_name(&id, &eid, "five as a word").unwrap();
        v.sync_stash(true);
        assert_eq!(v.stash[0].title(), "five as a word");
        stash::restored(&id, &eid).unwrap();
        v.sync_stash(true);
        assert!(v.stash.is_empty());
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&id));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn stashed_lines_go_back_to_their_spot_in_any_order_after_a_shift() {
        for order in [[0usize, 1], [1, 0]] {
            let d = std::env::temp_dir().join(format!("codehead-restore-{}-{}", std::process::id(), order[0]));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            let sh = |c: &str| assert!(Command::new("sh").arg("-c").arg(c).current_dir(&d).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap().success(), "{c}");
            sh("git init -q && git config user.email t@t && git config user.name t && seq 1 30 > x.txt && git add -A && git commit -qm init");
            // A after 5, 10 → ten, B C after 20, 25 removed; ten staged
            sh("seq 1 30 | sed 's/^5$/5\\nA/;s/^10$/ten/;s/^20$/20\\nB\\nC/;/^25$/d' > x.txt");
            let want = std::fs::read_to_string(d.join("x.txt")).unwrap();
            assert!(want.contains("5\nA\n6") && want.contains("20\nB\nC\n21") && !want.contains("\n25\n"), "{want}");
            let id = format!("changes-restore-{}-{}", std::process::id(), order[0]);
            let mut v = real_view(&d, &id);
            select(&mut v, "+ ten");
            select(&mut v, "- 10");
            press(&mut v, KeyCode::Char('s'));
            wait_op(&mut v);
            // stash 1: B (C stays) and ten (staged: out of the index too); stash 2: the removed 25
            let mut v = real_view(&d, &id);
            select(&mut v, "+ B");
            select(&mut v, "+ ten");
            select(&mut v, "- 10");
            press(&mut v, KeyCode::Char('p'));
            assert!(wait_op(&mut v).contains("stashed"), "{:?}", v.note);
            let mut v = real_view(&d, &id);
            select(&mut v, "- 25");
            press(&mut v, KeyCode::Char('p'));
            assert!(wait_op(&mut v).contains("stashed"), "{:?}", v.note);
            let x = std::fs::read_to_string(d.join("x.txt")).unwrap();
            assert!(x.contains("20\nC\n21") && x.contains("\n10\n") && x.contains("\n25\n") && x.contains("5\nA\n6"), "{x}");
            assert!(!git(&d, &["diff", "--cached"]).unwrap().contains("ten"), "ten out of the index");
            let stash = stash::read(&id);
            assert_eq!(stash.len(), 2);
            // the agent's edits since: 3 lines on top, a line between the spots
            sh("{ printf 'h1\\nh2\\nh3\\n'; sed 's/^15$/15\\nmid/' x.txt; } > y && mv y x.txt");
            for i in order {
                std::fs::write(d.join("p.diff"), &stash[i].patch).unwrap();
                // the restore's steps: `git apply`, else with 1 line of context (the context of an
                // entry can hold the spot of an other entry)
                sh("{ git apply p.diff || git apply -C1 p.diff; } && rm p.diff");
            }
            let got = std::fs::read_to_string(d.join("x.txt")).unwrap();
            let want = format!("h1\nh2\nh3\n{}", want.replace("\n15\n", "\n15\nmid\n"));
            assert_eq!(got, want);
            let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&id));
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn severity_row_cycles_and_agent_comments_group_by_severity() {
        let mut v = view(parse(DIFF));
        v.id = "changes-sev-0000".into();
        let _ = std::fs::create_dir_all(crate::engine::store::thread_dir(&v.id));
        v.build(100);
        v.at = (0..v.rows.len()).find(|&i| text_of(&v, i).contains("+ let c = 4;")).unwrap();
        press(&mut v, KeyCode::Char('c'));
        // the severity row has the keys: ←→ cycle (medium → high → critical), a letter: the text
        press(&mut v, KeyCode::Left);
        press(&mut v, KeyCode::Left);
        press(&mut v, KeyCode::Char('x'));
        let Some(Modal::Comment { level, on_level, text, .. }) = &v.modal else { panic!() };
        assert_eq!((*level, *on_level, text.text().as_str()), (crate::rank::Level::Critical, false, "x"));
        v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        assert_eq!(v.comments[0].level, crate::rank::Level::Critical);
        // an agent's comments (the MCP tool): into the file, the view reads them at its next draw
        let root = Path::new("/w");
        let n = agent_comments(&v.id, "amon", Some(root), &serde_json::json!([{"file": "/w/src/a.rs", "line": 12, "severity": "low", "text": "nit"}])).unwrap();
        assert_eq!(n, 1);
        let s = screen(&mut v, 140, 40).join("\n");
        assert_eq!(v.comments.len(), 2);
        assert_eq!((v.comments[1].path.as_str(), v.comments[1].by.as_str()), ("src/a.rs", "amon"));
        assert!(s.contains("critical") && s.contains("low"), "{s}");
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&v.id));
    }

    #[test]
    fn the_comment_modal_groups_lines_per_file_and_scrolls_them() {
        let mut v = view(parse(DIFF));
        v.id = "changes-modal-0000".into();
        v.build(100);
        select(&mut v, "+ let c = 4;");
        select(&mut v, "+ fn t() {}");
        press(&mut v, KeyCode::Char('c'));
        let Some(Modal::Comment { quote, .. }) = &v.modal else { panic!() };
        assert_eq!(quote.iter().filter(|q| matches!(q, QLine::Path(_))).count(), 2);
        assert_eq!(quote.iter().filter(|q| **q == QLine::Gap).count(), 1);
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        t.draw(|f| v.draw(f, f.area())).unwrap();
        let buf = t.backend().buffer().clone();
        // the title has the paths too: the rows after it
        let row = |pat: &str| (0..30u16).filter(|&y| (0..100u16).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>().contains(pat)).nth(1).unwrap();
        let (p1, gap) = (row("src/a.rs:12"), row("tests/a_test.rs:2") - 1);
        assert_ne!(buf[(10, p1)].bg, code_bg(), "the path: 0 code background");
        assert_eq!(buf[(10, p1 + 1)].bg, code_bg(), "the lines: the code background");
        assert_ne!(buf[(10, gap)].bg, code_bg(), "the gap between files: 0 background");
        // a short screen: the lines scroll (alt+↓), the box fits
        let long: Vec<QLine> = (0..40).map(|i| QLine::Code(format!("+ line {i}"))).collect();
        if let Some(Modal::Comment { quote, .. }) = v.modal.as_mut() {
            *quote = long;
        }
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();
        t.draw(|f| v.draw(f, f.area())).unwrap();
        let shown = |t: &ratatui::Terminal<ratatui::backend::TestBackend>| t.backend().buffer().content().iter().map(|c| c.symbol().to_string()).collect::<String>();
        assert!(shown(&t).contains("+ line 0") && !shown(&t).contains("+ line 39"));
        v.key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
        t.draw(|f| v.draw(f, f.area())).unwrap();
        assert!(!shown(&t).contains("+ line 0 ") && shown(&t).contains("+ line 1"));
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("changes-modal-0000"));
    }

    #[test]
    fn the_prompt_takes_the_selection_and_the_comments_along() {
        let mut v = view(parse(DIFF));
        v.id = "changes-context-0000".into();
        v.comments.clear();
        v.build(100);
        assert!(v.context().is_none());
        let i = (0..v.rows.len()).find(|&i| text_of(&v, i).contains("+ let c = 4;")).unwrap();
        v.select_row(i, true);
        v.comments.push(Comment { path: "src/a.rs".into(), line: 11, text: "why 3?".into(), lines: String::new(), quote: String::new(), level: Default::default(), by: String::new() });
        let c = v.context().unwrap();
        assert!(c.contains("- `src/a.rs:12`\n```diff\n+ let c = 4;\n```") && c.contains("- `src/a.rs:11`: why 3?"), "{c}");
    }

    #[test]
    fn enter_on_a_comment_edits_it_0_text_deletes_it() {
        let mut v = view(parse(DIFF));
        v.id = "changes-edit-0000".into();
        v.comments.clear();
        v.build(100);
        v.add_comment(&[Spot { path: "src/a.rs".into(), line: 11, lines: String::new() }], "old");
        assert_eq!(v.comments[0].quote, "+ let b = 3;");
        v.at = v.rows.iter().position(|r| r.comment == Some(0)).unwrap();
        press(&mut v, KeyCode::Enter);
        let Some(Modal::Comment { edit: Some(0), .. }) = &v.modal else { panic!("edit modal") };
        v.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        for c in "new".chars() {
            press(&mut v, KeyCode::Char(c));
        }
        v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        assert!(v.comments[0].text.ends_with("new"), "{:?}", v.comments);
        press(&mut v, KeyCode::Enter);
        v.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        if let Some(Modal::Comment { text, .. }) = v.modal.as_mut() {
            text.set_text("");
        }
        v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        assert!(v.comments.is_empty());
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("changes-edit-0000"));
    }

    #[test]
    fn git_steps_stage_unstage_commit_and_see_a_stopped_rebase() {
        let d = std::env::temp_dir().join(format!("codehead-gitops-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let sh = |c: &str| assert!(Command::new("sh").arg("-c").arg(c).current_dir(&d).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap().success(), "{c}");
        sh("git init -q -b main && git config user.email t@t && git config user.name t && printf 'a\\nb\\n' > x.rs && git add -A && git commit -qm init");
        sh("printf 'a\\nB\\n' > x.rs && printf 'n\\n' > y.rs");
        run_op(&d, &Op::Stage("x.rs".into())).unwrap();
        run_op(&d, &Op::Stage("y.rs".into())).unwrap();
        let c = load(&d).unwrap();
        assert_eq!((c.staged.get("x.rs"), c.staged.get("y.rs")), (Some(&true), Some(&true)));
        // a part staged: another edit after the stage
        sh("printf 'a\\nB\\nc\\n' > x.rs");
        assert_eq!(load(&d).unwrap().staged.get("x.rs"), Some(&false));
        run_op(&d, &Op::Unstage("y.rs".into())).unwrap();
        assert!(!load(&d).unwrap().staged.contains_key("y.rs"));
        assert_eq!(run_op(&d, &Op::Commit("fix: b is B\n\n- body".into())).unwrap(), "committed");
        let (_, log) = (0, git(&d, &["log", "-1", "--format=%B"]).unwrap());
        assert_eq!(log.trim(), "fix: b is B\n\n- body");
        assert!(load(&d).unwrap().stopped.is_none());
        // a conflict: 2 branches change line 1, rebase stops
        sh("git add -A && git commit -qm wip && git checkout -qb other HEAD~1 && printf 'Z\\nB\\n' > x.rs && git commit -qam other && git checkout -q main && printf 'Q\\nB\\nc\\n' > x.rs && git commit -qam main");
        assert!(run_op(&d, &Op::Update("other".into())).is_err(), "a conflict: the rebase stops");
        let c = load(&d).unwrap();
        assert_eq!(c.stopped, Some(vec!["x.rs".to_string()]));
        run_op(&d, &Op::Abort).unwrap();
        assert!(load(&d).unwrap().stopped.is_none());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn pr_checks_count_by_verdict() {
        let v = serde_json::json!({"number": 12, "url": "https://x/12", "state": "OPEN", "statusCheckRollup": [
            {"status": "COMPLETED", "conclusion": "SUCCESS"}, {"status": "COMPLETED", "conclusion": "FAILURE"},
            {"status": "IN_PROGRESS", "conclusion": ""}, {"state": "SUCCESS"}, {"state": "PENDING"}
        ]});
        assert_eq!(parse_pr(&v), Some(Pr { number: 12, url: "https://x/12".into(), state: "OPEN".into(), pass: 2, fail: 1, pending: 2 }));
        let mut view = view(parse(DIFF));
        view.pr = parse_pr(&v);
        view.build(140);
        assert!(summary(&view).contains("PR #12 open ✓2 ✗1 ●2"), "{:?}", summary(&view));
    }

    #[test]
    fn commit_drafts_lose_fences_and_notes() {
        assert_eq!(clean_message("```\nfix: x\n\n- y\n```"), "fix: x\n\n- y");
        assert_eq!(clean_message("fix: x\n\n- y\n\nNote: I could not check doinstruct.tools"), "fix: x\n\n- y");
    }

    #[test]
    fn load_reads_a_real_repo_with_untracked_files() {
        let d = std::env::temp_dir().join(format!("codehead-changes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let sh = |c: &str| assert!(Command::new("sh").arg("-c").arg(c).current_dir(&d).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap().success(), "{c}");
        sh("git init -q && git config user.email t@t && git config user.name t && printf 'a\\nb\\n' > x.rs && git add -A && git commit -qm init");
        sh("printf 'a\\nB\\n' > x.rs && printf 'new\\n' > y.md");
        let c = load(&d).unwrap();
        assert_eq!(c.base, "HEAD");
        let paths: Vec<&str> = c.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["x.rs", "y.md"]);
        assert_eq!(c.files[1].status, Status::New);
        let _ = std::fs::remove_dir_all(d);
    }

    /// A diff of N files, each with HUNKS hunks of 10 changed lines (Rust code).
    fn big_diff(n: usize, hunks: usize) -> String {
        let mut s = String::new();
        for i in 0..n {
            s.push_str(&format!("diff --git a/src/m{i}.rs b/src/m{i}.rs\n--- a/src/m{i}.rs\n+++ b/src/m{i}.rs\n"));
            for h in 0..hunks {
                let at = h * 40 + 1;
                s.push_str(&format!("@@ -{at},8 +{at},10 @@ fn f{h}() {{\n"));
                s.push_str(" let a = vec![1, 2, 3];\n");
                for k in 0..4 {
                    s.push_str(&format!("-    let x{k}_{i} = compute(a.iter().map(|v| v * {k}).sum::<i32>(), \"old {h}\");\n"));
                }
                for k in 0..6 {
                    s.push_str(&format!("+    let x{k}_{i} = compute(a.iter().map(|v| v * {k} + 1).sum::<i32>(), \"new {h}\");\n"));
                }
                s.push_str(" // done\n let b = 2;\n");
            }
        }
        s
    }

    /// Perf: cargo test --release perf_big_diff -- --ignored --nocapture
    #[test]
    #[ignore]
    fn perf_big_diff() {
        let files = parse(&big_diff(200, 10));
        let lines: usize = files.iter().flat_map(|f| &f.hunks).map(|h| h.lines.len()).sum();
        let mut v = view(files);
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 50)).unwrap();
        let t0 = Instant::now();
        t.draw(|f| v.draw(f, f.area())).unwrap();
        let first = t0.elapsed();
        let t0 = Instant::now();
        for _ in 0..200 {
            v.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
            t.draw(|f| v.draw(f, f.area())).unwrap();
        }
        let nav = t0.elapsed() / 200;
        let t0 = Instant::now();
        for _ in 0..20 {
            v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            t.draw(|f| v.draw(f, f.area())).unwrap();
        }
        let fold = t0.elapsed() / 20;
        let t0 = Instant::now();
        for _ in 0..20 {
            v.key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));
            t.draw(|f| v.draw(f, f.area())).unwrap();
        }
        let reviewed = t0.elapsed() / 20;
        let t0 = Instant::now();
        v.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        for c in "compute".chars() {
            v.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
            t.draw(|f| v.draw(f, f.area())).unwrap();
        }
        let filter = t0.elapsed() / 7;
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("changes-test-0000"));
        println!("perf: 200 files, {lines} diff lines · first draw {first:?} · 1 row down + draw {nav:?} · fold toggle + draw {fold:?} · r (reviewed) + draw {reviewed:?} · filter key + draw {filter:?}");
    }

    /// Live look: CODEHEAD_CHANGES_DIR=<repo> cargo test changes_live -- --ignored --nocapture
    #[test]
    #[ignore]
    fn changes_live() {
        let dir = PathBuf::from(std::env::var("CODEHEAD_CHANGES_DIR").unwrap());
        for w in [120u16, 180] {
            let mut v = View::new("changes-live-0000", &dir);
            v.changes = load(&dir).unwrap();
            v.loaded = Some(Instant::now());
            let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, 40)).unwrap();
            t.draw(|f| v.draw(f, f.area())).unwrap();
            let b = t.backend().buffer().clone();
            println!("--- {w} columns, base {}", v.changes.base);
            for y in 0..40 {
                println!("{}", (0..w).map(|x| b[(x, y)].symbol().to_string()).collect::<String>());
            }
        }
    }
}
