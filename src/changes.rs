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
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::theme;

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
    for p in others.split('\0').filter(|p| !p.is_empty()).take(MAX_UNTRACKED) {
        files.push(untracked(dir, p));
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
    Ok(Changes { base: label, files, staged, stopped, ahead, behind })
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
        }
    }
}

/// Run OP in DIR: Ok(what happened) / Err(git's 1st error line).
pub fn run_op(dir: &Path, op: &Op) -> Result<String, String> {
    match op {
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
                Some(c) if c.len() == plain.len() => c.clone(),
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

/// ` + ` / ` - ` / 3 spaces in the color of KIND, on its line tint.
fn sign_span(kind: Kind) -> Span<'static> {
    let (bg, _) = tints(kind);
    let (sign, fg) = match kind {
        Kind::Add => ("+", theme::pal().ok),
        Kind::Del => ("-", theme::pal().danger),
        Kind::Ctx => (" ", theme::pal().text),
    };
    let st = Style::default().fg(fg);
    Span::styled(format!(" {sign} "), bg.map_or(st, |b| st.bg(b)))
}

/// Line tint and changed-word tint of KIND (0 for context lines).
fn tints(kind: Kind) -> (Option<ratatui::style::Color>, Option<ratatui::style::Color>) {
    let base = theme::pal().panel;
    match kind {
        Kind::Add => (Some(theme::tint(theme::pal().ok, base, 0.18)), Some(theme::tint(theme::pal().ok, base, 0.42))),
        Kind::Del => (Some(theme::tint(theme::pal().danger, base, 0.18)), Some(theme::tint(theme::pal().danger, base, 0.42))),
        Kind::Ctx => (None, None),
    }
}

/// P as spans in ROOM columns (cut with `…`), padded to ROOM with the line tint of KIND.
fn painted_spans(p: &Painted, kind: Kind, room: usize) -> Vec<Span<'static>> {
    let (bg, mark) = tints(kind);
    let style = |color, marked: bool| {
        let s = Style::default().fg(color);
        match (marked, mark, bg) {
            (true, Some(m), _) => s.bg(m),
            (_, _, Some(b)) => s.bg(b),
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
    let pad = Style::default();
    out.push(Span::styled(" ".repeat(room.saturating_sub(w)), bg.map_or(pad, |b| pad.bg(b))));
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
    /// Write a comment on LINE of FILE (the prompt takes the text).
    Comment(String, u32),
    /// Send this text to the thread (the review comments batch).
    Send(String),
    /// Write the commit message of the staged files (the prompt takes it).
    CommitMessage,
}

/// A drawn row: its line, the file it belongs to, the new-side line it points at.
#[derive(Clone, Debug)]
struct Row {
    line: Line<'static>,
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

/// A review comment on LINE (new side) of PATH, for the batch to an agent.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Comment {
    pub path: String,
    pub line: u32,
    pub text: String,
}

fn comments_path(id: &str) -> PathBuf {
    crate::engine::store::thread_dir(id).join("comments.json")
}

fn read_comments(id: &str) -> Vec<Comment> {
    std::fs::read(comments_path(id)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write_comments(id: &str, c: &[Comment]) {
    if let Ok(b) = serde_json::to_vec_pretty(c) {
        let _ = crate::engine::write_atomic(&comments_path(id), &b);
    }
}

/// The batch message of COMMENTS (0 comments: None).
pub fn batch(comments: &[Comment]) -> Option<String> {
    if comments.is_empty() {
        return None;
    }
    let mut s = String::from("Review comments on the changes (file:line, new side). Address each one:\n");
    for c in comments {
        let mut lines = c.text.lines();
        s.push_str(&format!("- `{}:{}`: {}\n", c.path, c.line, lines.next().unwrap_or_default()));
        for l in lines {
            s.push_str(&format!("  {l}\n"));
        }
    }
    Some(s.trim_end().to_string())
}

pub struct View {
    id: String,
    dir: PathBuf,
    pub changes: Changes,
    marks: HashMap<String, u64>,
    /// Boxes opened by hand (reviewed / generated: folded by default) or closed by hand.
    toggled: HashSet<String>,
    /// Side by side: None: by the room.
    side: Option<bool>,
    rows: Vec<Row>,
    built: Option<(u16, bool)>,
    /// Cursor row, 1st row in view.
    at: usize,
    top: usize,
    loading: Option<Receiver<Result<Changes, String>>>,
    loaded: Option<Instant>,
    pub error: Option<String>,
    area: Rect,
    /// After a re-read: the file the cursor was on.
    pending_cursor: Option<String>,
    /// The model's order (groups, whys) of the current set of paths; a call on its way; why 0.
    rank: Option<crate::rank::Rank>,
    ranking: Option<Receiver<Result<crate::rank::Rank, String>>>,
    rank_error: Option<String>,
    /// The rank model (setting `rank`), None: path rules.
    rank_model: Option<String>,
    /// Review comments, for 1 batch message (`C`).
    comments: Vec<Comment>,
    /// The reviewer's findings, a review on its way, why 0, the model (None: `off`).
    review: Option<crate::review::Review>,
    reviewing: Option<Receiver<Result<crate::review::Review, String>>>,
    review_error: Option<String>,
    review_model: Option<String>,
    /// A git step on its way, and what the last one did (the thread view shows it).
    op: Option<Receiver<Result<String, String>>>,
    note: Option<String>,
}

impl View {
    pub fn new(id: &str, dir: &Path) -> View {
        View {
            id: id.to_string(),
            dir: dir.to_path_buf(),
            changes: Changes::default(),
            marks: read_marks(id),
            toggled: HashSet::new(),
            side: None,
            rows: Vec::new(),
            built: None,
            at: 0,
            top: 0,
            loading: None,
            loaded: None,
            error: None,
            area: Rect::default(),
            pending_cursor: None,
            rank: crate::rank::cached(id),
            ranking: None,
            rank_error: None,
            rank_model: if cfg!(test) { None } else { crate::config::rank_model() },
            comments: read_comments(id),
            review: crate::review::cached(id),
            reviewing: None,
            review_error: None,
            review_model: if cfg!(test) { None } else { crate::config::review_model() },
            op: None,
            note: None,
        }
    }

    /// The files in the model's order (its groups first, the rest after in path-rule order);
    /// a rank of another set of paths: a new call.
    fn apply_rank(&mut self, force: bool) {
        let key = crate::rank::key(&self.changes.files);
        let fresh = self.rank.as_ref().is_some_and(|r| r.key == key);
        if (force || !fresh) && self.ranking.is_none() && !self.changes.files.is_empty()
            && let Some(model) = self.rank_model.clone()
        {
            let (tx, rx) = mpsc::channel();
            let (id, dir, files) = (self.id.clone(), self.dir.clone(), self.changes.files.clone());
            std::thread::spawn(move || {
                let _ = tx.send(crate::rank::run(&id, &dir, &files, &model));
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

    /// The model's group of PATH and the why of PATH, if the rank is current.
    fn ranked(&self, path: &str) -> Option<(usize, &crate::rank::Group, &str)> {
        let key = crate::rank::key(&self.changes.files);
        let r = self.rank.as_ref().filter(|r| r.key == key)?;
        r.groups.iter().enumerate().find_map(|(g, gr)| gr.files.iter().find(|f| f.path == path).map(|f| (g, gr, f.why.as_str())))
    }

    /// A finished rank call applies. True: the order changed.
    fn poll_rank(&mut self) -> bool {
        let Some(rx) = &self.ranking else { return false };
        let Ok(r) = rx.try_recv() else { return false };
        self.ranking = None;
        match r {
            Ok(r) => {
                let cur = self.rows.get(self.at).and_then(|r| self.changes.files.get(r.file)).map(|f| f.path.clone());
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
            "ranking…".to_string()
        } else if let Some(e) = &self.rank_error {
            format!("path order · {e}")
        } else if let Some(r) = self.rank.as_ref().filter(|r| r.key == crate::rank::key(&self.changes.files)) {
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

    fn poll_op(&mut self) -> bool {
        let Some(rx) = &self.op else { return false };
        let Ok(r) = rx.try_recv() else { return false };
        self.op = None;
        self.note = Some(match r {
            Ok(s) => format!("git: {s}"),
            Err(e) => format!("git: {e}"),
        });
        self.refresh(true);
        true
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
        let (tx, rx) = mpsc::channel();
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
                let same = c.files.iter().map(|f| f.hash).eq(self.changes.files.iter().map(|f| f.hash)) && self.error.is_none();
                if same {
                    return false;
                }
                // the cursor stays on its file
                let cur = self.rows.get(self.at).and_then(|r| self.changes.files.get(r.file)).map(|f| f.path.clone());
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
        let w = usize::from(width);
        let mut rows: Vec<Row> = vec![Row { line: self.summary(), file: 0, at: None, head: false, hunk: false, comment: None, finding: None }];
        if let Some(c) = &self.changes.stopped {
            let files = if c.is_empty() { "0 conflicts".to_string() } else { format!("{} conflicts: {}", c.len(), c.join(", ")) };
            let line = Line::from(vec![
                Span::styled(" rebase stopped ", theme::pill()),
                Span::styled(crate::nav::fit(&format!(" {files} · A: agent resolves · K: continue · X: abort"), w.saturating_sub(16)), Style::default().fg(theme::pal().warn)),
            ]);
            rows.push(Row { line, file: 0, at: None, head: false, hunk: false, comment: None, finding: None });
        }
        // group heads: the model's groups (title, why), else the path-rule categories
        let mut last: Option<(Option<usize>, Cat)> = None;
        for (i, f) in self.changes.files.iter().enumerate() {
            let g = self.ranked(&f.path);
            let here = (g.map(|x| x.0), f.cat);
            let new_group = match (last, g) {
                (None, _) => true,
                (Some((pg, _)), Some((gi, _, _))) => pg != Some(gi),
                (Some((pg, pc)), None) => pg.is_some() || pc != f.cat,
            };
            if new_group {
                let line = match g {
                    Some((gi, gr, _)) => Line::from(vec![
                        Span::styled(format!(" {} · {}", gi + 1, gr.title), theme::accent_bold()),
                        Span::styled(crate::nav::fit(&format!("  {}", gr.why), w.saturating_sub(gr.title.width() + 8)), theme::muted()),
                    ]),
                    None => Line::styled(format!(" {}", f.cat.label()), theme::muted().add_modifier(Modifier::BOLD)),
                };
                rows.push(Row { line: Line::raw(""), file: i, at: None, head: false, hunk: false, comment: None, finding: None });
                rows.push(Row { line, file: i, at: None, head: false, hunk: false, comment: None, finding: None });
            }
            last = Some(here);
            self.file_rows(&mut rows, i, f, w, side);
        }
        self.rows = rows;
        self.built = Some((width, side));
        if let Some(p) = self.pending_cursor.take()
            && let Some(i) = self.changes.files.iter().position(|f| f.path == p)
            && let Some(r) = self.rows.iter().position(|r| r.head && r.file == i)
        {
            self.at = r;
        }
        self.at = self.at.min(self.rows.len().saturating_sub(1));
        // the summary or a group head under the cursor: the 1st box
        if !self.rows.get(self.at).is_some_and(|r| r.head || r.at.is_some()) && let Some(h) = self.rows.iter().position(|r| r.head) {
            self.at = h;
        }
    }

    fn file_rows(&self, rows: &mut Vec<Row>, i: usize, f: &FileDiff, w: usize, side: bool) {
        let reviewed = self.is_reviewed(f);
        let updated = !reviewed && self.marks.contains_key(&f.path);
        let border = theme::line();
        let mark = if reviewed { Span::styled("☑ ", Style::default().fg(theme::pal().ok)) } else { Span::styled("☐ ", theme::muted()) };
        let mut name = f.path.clone();
        if let Some(o) = &f.old_path {
            name = format!("{o} → {name}");
        }
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
        let mut head = vec![Span::styled("╭─ ", border), mark, stage, Span::styled(crate::nav::fit(&name, room), theme::bold())];
        if updated {
            head.push(Span::styled(" updated", Style::default().fg(theme::pal().warn)));
        }
        let used: usize = head.iter().map(|s| s.content.width()).sum();
        let fill = w.saturating_sub(used + right.width() + 3);
        head.push(Span::styled(format!(" {}", "─".repeat(fill)), border));
        head.push(Span::styled(right, theme::muted()));
        head.push(Span::styled("─╮", border));
        let first = f.first_line();
        rows.push(Row { line: Line::from(head), file: i, at: Some(first), head: true, hunk: false, comment: None, finding: None });
        if self.folded(f) {
            rows.push(Row { line: Line::styled(format!("╰{}╯", "─".repeat(w.saturating_sub(2))), border), file: i, at: Some(first), head: false, hunk: false, comment: None, finding: None });
            return;
        }
        let inner = w.saturating_sub(4);
        let bar = |mut spans: Vec<Span<'static>>| -> Line<'static> {
            let used: usize = spans.iter().map(|s| s.content.width()).sum();
            let mut v = vec![Span::styled("│ ", border)];
            v.append(&mut spans);
            v.push(Span::raw(" ".repeat(inner.saturating_sub(used))));
            v.push(Span::styled(" │", border));
            Line::from(v)
        };
        // the model's why of the file: the box's 1st row
        if let Some((_, _, why)) = self.ranked(&f.path).filter(|x| !x.2.is_empty()) {
            let st = theme::muted().add_modifier(Modifier::ITALIC);
            rows.push(Row { line: bar(vec![Span::styled(crate::nav::fit(why, inner), st)]), file: i, at: Some(first), head: false, hunk: false, comment: None, finding: None });
        }
        if f.binary || f.hunks.is_empty() {
            let note = if f.binary { "binary file" } else { "0 line changes (mode or rename)" };
            rows.push(Row { line: bar(vec![Span::styled(note.to_string(), theme::muted())]), file: i, at: Some(first), head: false, hunk: false, comment: None, finding: None });
        }
        let num_w = f.hunks.iter().flat_map(|h| &h.lines).filter_map(|l| l.new.max(l.old)).max().unwrap_or(1).to_string().len().max(3);
        for h in &f.hunks {
            let label = format!("⋯ {}", if h.ctx.is_empty() { format!("line {}", h.new) } else { h.ctx.clone() });
            rows.push(Row { line: bar(vec![Span::styled(crate::nav::fit(&label, inner), theme::muted())]), file: i, at: Some(h.new.max(1)), head: false, hunk: true, comment: None, finding: None });
            // a new or deleted file: 1 side only, unified uses the room
            let painted = paint(f, h);
            if side && !matches!(f.status, Status::New | Status::Deleted) {
                self.side_rows(rows, i, h, &painted, inner, num_w, &bar);
            } else {
                for (l, p) in h.lines.iter().zip(&painted) {
                    let num = match l.kind {
                        Kind::Add => format!("{:>num_w$} {:>num_w$}", "", l.new.unwrap_or(0)),
                        Kind::Del => format!("{:>num_w$} {:>num_w$}", l.old.unwrap_or(0), ""),
                        Kind::Ctx => format!("{:>num_w$} {:>num_w$}", l.old.unwrap_or(0), l.new.unwrap_or(0)),
                    };
                    let room = inner.saturating_sub(num.len() + 3);
                    let mut spans = vec![Span::styled(num, theme::muted()), sign_span(l.kind)];
                    spans.extend(painted_spans(p, l.kind, room));
                    rows.push(Row { line: bar(spans), file: i, at: l.new.or(l.old), head: false, hunk: false, comment: None, finding: None });
                }
            }
        }
        // findings, then review comments: under the 1st diff row of their line, else at the box's end
        let start = rows.iter().rposition(|r| r.head && r.file == i).unwrap_or(0);
        let mut notes: Vec<(u32, Vec<Row>)> = Vec::new();
        for (fi, x) in self.review.iter().flat_map(|r| r.findings.iter().enumerate()).filter(|(_, x)| x.file == f.path) {
            let color = match x.severity.as_str() {
                "critical" => theme::pal().danger,
                "major" => theme::pal().warn,
                _ => theme::pal().muted,
            };
            let st = Style::default().fg(color);
            let head = format!("⚠ {} · ", x.severity);
            let mut v = vec![Row {
                line: bar(vec![Span::styled(head.clone(), st.add_modifier(Modifier::BOLD)), Span::styled(crate::nav::fit(&x.title, inner.saturating_sub(head.width())), st.add_modifier(Modifier::BOLD))]),
                file: i,
                at: Some(x.line),
                head: false,
                hunk: false,
                comment: None,
                finding: Some(fi),
            }];
            for l in x.body.lines().take(3) {
                v.push(Row { line: bar(vec![Span::raw("  "), Span::styled(crate::nav::fit(l, inner.saturating_sub(2)), theme::muted())]), file: i, at: Some(x.line), head: false, hunk: false, comment: None, finding: Some(fi) });
            }
            notes.push((x.line, v));
        }
        for (ci, c) in self.comments.iter().enumerate().filter(|(_, c)| c.path == f.path) {
            let st = Style::default().fg(theme::pal().accent);
            let v = c
                .text
                .lines()
                .enumerate()
                .map(|(k, l)| {
                    let head = if k == 0 { format!("✎ {}: ", c.line) } else { "  ".to_string() };
                    let room = inner.saturating_sub(head.width());
                    Row { line: bar(vec![Span::styled(head, st.add_modifier(Modifier::BOLD)), Span::styled(crate::nav::fit(l, room), st)]), file: i, at: Some(c.line), head: false, hunk: false, comment: Some(ci), finding: None }
                })
                .collect();
            notes.push((c.line, v));
        }
        let added = |r: &Row| r.comment.is_some() || r.finding.is_some();
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
        rows.push(Row { line: Line::styled(format!("╰{}╯", "─".repeat(w.saturating_sub(2))), border), file: i, at: None, head: false, hunk: false, comment: None, finding: None });
    }

    /// The review comments (for the view bar and the hint row).
    pub fn comment_count(&self) -> usize {
        self.comments.len()
    }

    /// Add a comment TEXT on LINE of PATH; the rows show it at once.
    pub fn add_comment(&mut self, path: &str, line: u32, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.comments.push(Comment { path: path.to_string(), line, text: text.to_string() });
        write_comments(&self.id, &self.comments);
        self.rebuild_here();
    }

    /// Hunk H side by side: the old side left, the new side right; a run of removed lines pairs
    /// with the added lines after it.
    #[allow(clippy::too_many_arguments)]
    fn side_rows(&self, rows: &mut Vec<Row>, i: usize, h: &Hunk, painted: &[Painted], inner: usize, num_w: usize, bar: &dyn Fn(Vec<Span<'static>>) -> Line<'static>) {
        let half = inner.saturating_sub(3) / 2;
        let cell = |k: Option<usize>, left: bool| -> Vec<Span<'static>> {
            let Some(k) = k else { return vec![Span::raw(" ".repeat(half))] };
            let l = &h.lines[k];
            let n = if left { l.old } else { l.new }.unwrap_or(0);
            let mut v = vec![Span::styled(format!("{n:>num_w$}"), theme::muted()), sign_span(l.kind)];
            v.extend(painted_spans(&painted[k], l.kind, half.saturating_sub(num_w + 3)));
            v
        };
        let mut k = 0;
        let ls = &h.lines;
        while k < ls.len() {
            if ls[k].kind == Kind::Ctx {
                let mut s = cell(Some(k), true);
                s.push(Span::styled(" │ ", theme::line()));
                s.extend(cell(Some(k), false));
                rows.push(Row { line: bar(s), file: i, at: ls[k].new, head: false, hunk: false, comment: None, finding: None });
                k += 1;
                continue;
            }
            let d = ls[k..].iter().take_while(|l| l.kind == Kind::Del).count();
            let a = ls[k + d..].iter().take_while(|l| l.kind == Kind::Add).count();
            for j in 0..d.max(a) {
                let (dk, ak) = ((j < d).then_some(k + j), (j < a).then_some(k + d + j));
                let mut s = cell(dk, true);
                s.push(Span::styled(" │ ", theme::line()));
                s.extend(cell(ak, false));
                let at = ak.and_then(|x| ls[x].new).or_else(|| (a > 0).then(|| ls[k + d].new).flatten()).or_else(|| dk.and_then(|x| ls[x].old));
                rows.push(Row { line: bar(s), file: i, at, head: false, hunk: false, comment: None, finding: None });
            }
            k += d + a;
        }
    }

    pub fn draw(&mut self, f: &mut Frame, area: Rect) {
        self.area = area;
        if self.poll_review() {
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
        }
        self.refresh(false);
        if let Some(e) = &self.error {
            f.render_widget(Paragraph::new(Line::styled(format!("  changes: {e}"), theme::muted())), area);
            return;
        }
        if self.changes.files.is_empty() {
            let msg = if self.loaded.is_none() { "  reading changes…".to_string() } else { format!("  0 changes against {}", self.changes.base) };
            f.render_widget(Paragraph::new(Line::styled(msg, theme::muted())), area);
            return;
        }
        let width = area.width.saturating_sub(1);
        self.build(width);
        let h = usize::from(area.height);
        // the cursor in view; a box header 1 row below the top when it can
        if self.at < self.top + 1 {
            self.top = self.at.saturating_sub(1);
        } else if self.at >= self.top + h {
            self.top = self.at + 1 - h;
        }
        self.top = self.top.min(self.rows.len().saturating_sub(h));
        for (y, r) in self.rows.iter().enumerate().skip(self.top).take(h) {
            let rect = Rect { x: area.x, y: area.y + (y - self.top) as u16, width, height: 1 };
            f.render_widget(Paragraph::new(r.line.clone()), rect);
            if y == self.at {
                theme::hover(f.buffer_mut(), Rect { x: area.x, width: 1, ..rect });
                f.render_widget(Paragraph::new(Span::styled("▌", theme::accent())), Rect { x: area.x + width, width: 1, ..rect });
            }
        }
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

    pub fn key(&mut self, k: KeyEvent) -> Act {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let last = self.rows.len().saturating_sub(1);
        match k.code {
            KeyCode::Down if !ctrl => self.at = (self.at + 1).min(last),
            KeyCode::Up if !ctrl => self.at = self.at.saturating_sub(1),
            KeyCode::PageDown => self.at = (self.at + self.page()).min(last),
            KeyCode::PageUp => self.at = self.at.saturating_sub(self.page()),
            KeyCode::Char('j') => self.jump(true, |r| r.head),
            KeyCode::Char('k') => self.jump(false, |r| r.head),
            KeyCode::Char('n') => self.jump(true, |r| r.hunk || r.head),
            KeyCode::Char('p') => self.jump(false, |r| r.hunk || r.head),
            KeyCode::Char('g') | KeyCode::Home => self.at = 0,
            KeyCode::Char('G') | KeyCode::End => self.at = last,
            KeyCode::Char(' ') => return self.toggle_reviewed(),
            KeyCode::Enter => {
                if let Some(p) = self.file_at().map(|f| f.path.clone()) {
                    if !self.toggled.remove(&p) {
                        self.toggled.insert(p);
                    }
                    self.rebuild_on_file();
                }
            }
            KeyCode::Char('s') => {
                let now = self.side_by_side(self.area.width.saturating_sub(1));
                self.side = Some(!now);
                self.rebuild_on_file();
                return Act::Toast(if now { "unified".into() } else { "side by side".into() });
            }
            KeyCode::Char('R') => {
                if self.rank_model.is_none() {
                    return Act::Toast("rank: off (setting rank)".into());
                }
                self.apply_rank(true);
                self.built = None;
                return Act::Toast("ranking again…".into());
            }
            KeyCode::Char('r') => {
                self.refresh(true);
                return Act::Toast("changes: reading git".into());
            }
            KeyCode::Char('a') => {
                if let Some(p) = self.file_at().map(|f| f.path.clone()) {
                    return self.start_op(Op::Stage(p));
                }
            }
            KeyCode::Char('u') => {
                if let Some(p) = self.file_at().map(|f| f.path.clone()) {
                    return self.start_op(Op::Unstage(p));
                }
            }
            KeyCode::Char('m') => {
                if !self.has_staged() {
                    return Act::Toast("0 staged files · a: stage the box".into());
                }
                return Act::CommitMessage;
            }
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
            KeyCode::Char('c') => {
                if let (Some(f), Some(r)) = (self.file_at(), self.rows.get(self.at)) {
                    return Act::Comment(f.path.clone(), r.at.unwrap_or_else(|| f.first_line()));
                }
            }
            KeyCode::Char('x') => {
                if let Some(ci) = self.rows.get(self.at).and_then(|r| r.comment) {
                    self.comments.remove(ci);
                    write_comments(&self.id, &self.comments);
                    self.rebuild_here();
                    return Act::Toast(format!("comment deleted · {} left", self.comments.len()));
                }
                if let Some(fi) = self.rows.get(self.at).and_then(|r| r.finding) {
                    self.drop_finding(fi);
                    self.rebuild_here();
                    return Act::Toast("finding dismissed".into());
                }
            }
            // y on a finding: into the comment batch (C sends it)
            KeyCode::Char('y') => {
                if let Some(fi) = self.rows.get(self.at).and_then(|r| r.finding)
                    && let Some(x) = self.review.as_ref().and_then(|r| r.findings.get(fi)).cloned()
                {
                    self.drop_finding(fi);
                    self.add_comment(&x.file, x.line, &crate::review::as_comment(&x));
                    return Act::Toast(format!("finding → comment batch · {} · C: send", self.comments.len()));
                }
            }
            KeyCode::Char('V') => return self.start_review(),
            KeyCode::Char('C') => {
                let Some(text) = batch(&self.comments) else { return Act::Toast("0 comments · c: comment on the line".into()) };
                self.comments.clear();
                write_comments(&self.id, &self.comments);
                self.built = None;
                return Act::Send(text);
            }
            KeyCode::Char('o') => {
                if let (Some(f), Some(r)) = (self.file_at(), self.rows.get(self.at)) {
                    return Act::Open(f.path.clone(), r.at.unwrap_or_else(|| f.first_line()));
                }
            }
            _ => {}
        }
        Act::None
    }

    /// The rows change (fold, layout): the cursor stays on its file's header.
    /// The rows change (a comment): the cursor stays on its row number.
    fn rebuild_here(&mut self) {
        let width = self.built.map_or(self.area.width.saturating_sub(1), |b| b.0);
        let at = self.at;
        self.built = None;
        self.build(width);
        self.at = at.min(self.rows.len().saturating_sub(1));
    }

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

    /// space: the box under the cursor reviewed / not; reviewed: the cursor goes to the next box.
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

    pub fn mouse(&mut self, m: MouseEvent) {
        let inside = m.column >= self.area.x && m.column < self.area.right() && m.row >= self.area.y && m.row < self.area.bottom();
        if !inside {
            return;
        }
        let last = self.rows.len().saturating_sub(1);
        match m.kind {
            MouseEventKind::ScrollDown => {
                self.top = (self.top + 3).min(self.rows.len().saturating_sub(usize::from(self.area.height)));
                self.at = self.at.max(self.top + 1).min(last);
            }
            MouseEventKind::ScrollUp => {
                self.top = self.top.saturating_sub(3);
                self.at = self.at.min(self.top + usize::from(self.area.height).saturating_sub(2));
            }
            MouseEventKind::Down(_) => self.at = (self.top + usize::from(m.row - self.area.y)).min(last),
            _ => {}
        }
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
        v.changes = Changes { base: "origin/main".into(), files, ..Default::default() };
        v.loaded = Some(Instant::now());
        v
    }

    fn text(v: &View) -> Vec<String> {
        v.rows.iter().map(|r| r.line.spans.iter().map(|s| s.content.as_ref()).collect::<String>()).collect()
    }

    #[test]
    fn boxes_unified_and_side_by_side_by_width() {
        let mut v = view(parse(DIFF));
        v.build(120);
        let t = text(&v);
        let h = t.iter().position(|l| l.starts_with("╭─ ☐ src/a.rs")).unwrap();
        assert!(t[h].ends_with("logic · +2 −1 ─╮") && t[0].contains("4 files"), "{t:#?}");
        assert!(t.iter().any(|l| l.contains("⋯ fn run() {")), "{t:#?}");
        assert!(t.iter().any(|l| l.contains(" 11     - let b = 2;")), "{t:#?}");
        assert!(t.iter().any(|l| l.contains("     11 + let b = 3;")), "{t:#?}");
        assert!(t.iter().filter(|l| l.starts_with(['╭', '│', '╰'])).all(|l| l.width() == 120), "every box row 120 wide: {t:#?}");
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
        let f = |p: &str, w: &str| RFile { path: p.into(), why: w.into() };
        v.rank = Some(Rank { key, model: "haiku".into(), groups: vec![
            Group { title: "Test first".into(), why: "the test drives it".into(), files: vec![f("tests/a_test.rs", "adds the test")] },
            Group { title: "Core".into(), why: "b becomes 3".into(), files: vec![f("src/a.rs", "changes b"), f("new.rs", "")] },
        ] });
        v.apply_rank(false);
        let paths: Vec<&str> = v.changes.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["tests/a_test.rs", "src/a.rs", "new.rs", "img.png"], "unranked files after the groups");
        v.build(100);
        let t = text(&v);
        assert!(t[0].ends_with("order: haiku"), "{t:#?}");
        let g1 = t.iter().position(|l| l.starts_with(" 1 · Test first  the test drives it")).unwrap();
        assert!(t[g1 + 1].contains("☐ tests/a_test.rs") && t[g1 + 2].contains("adds the test"), "{t:#?}");
        assert!(t.iter().any(|l| l.starts_with(" 2 · Core")), "{t:#?}");
        // img.png: unranked, under its category
        let img = t.iter().position(|l| l.contains("img.png")).unwrap();
        assert_eq!(t[img - 1], " logic", "{t:#?}");
        // a new set of paths: the rank no longer applies
        v.changes.files.pop();
        v.built = None;
        v.build(100);
        assert!(text(&v)[0].ends_with("path order"));
    }

    #[test]
    fn comments_sit_under_their_line_and_go_out_as_1_batch() {
        let mut v = view(parse(DIFF));
        v.id = "changes-comments-0000".into();
        v.comments.clear();
        v.build(100);
        v.add_comment("src/a.rs", 11, "why 3?\nkeep 2");
        v.add_comment("src/a.rs", 99, "outside any hunk");
        let t = text(&v);
        let at = t.iter().position(|l| l.contains("+ let b = 3;")).unwrap();
        assert!(t[at + 1].contains("✎ 11: why 3?") && t[at + 2].contains("keep 2"), "{t:#?}");
        let end = t.iter().position(|l| l.contains("outside any hunk")).unwrap();
        assert!(t[end + 1].starts_with('╰'), "orphan at the box end: {t:#?}");
        // x on a comment row deletes it
        v.at = v.rows.iter().position(|r| r.comment == Some(1)).unwrap();
        v.key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(v.comment_count(), 1);
        match v.key(KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT)) {
            Act::Send(s) => assert_eq!(s, "Review comments on the changes (file:line, new side). Address each one:\n- `src/a.rs:11`: why 3?\n  keep 2"),
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
        assert!(t[0].contains("2 findings (sonnet)"), "{t:#?}");
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
    fn rows_have_syntax_colors_line_tints_and_word_marks() {
        let mut v = view(parse(DIFF));
        v.build(100);
        let row = v.rows.iter().find(|r| text_of(r).contains("+ let b = 3;")).unwrap();
        let add_bg = tints(Kind::Add).0;
        let mark_bg = tints(Kind::Add).1;
        // `let`: a keyword color (0 plain text color), on the add tint
        let kw = row.line.spans.iter().find(|s| s.content.starts_with("let")).unwrap();
        assert_ne!(kw.style.fg, Some(theme::pal().text), "syntax color");
        assert_eq!(kw.style.bg, add_bg);
        // `3`: the changed word, the strong tint
        let three = row.line.spans.iter().find(|s| s.content.contains('3')).unwrap();
        assert_eq!(three.style.bg, mark_bg, "{:?}", row.line.spans);
        // context rows: 0 tint
        let ctx = v.rows.iter().find(|r| text_of(r).contains("let a = 1;")).unwrap();
        assert!(ctx.line.spans.iter().filter(|s| s.content.contains("let")).all(|s| s.style.bg.is_none()));
    }

    #[test]
    fn space_marks_reviewed_folds_and_a_changed_diff_shows_updated() {
        let mut v = view(parse(DIFF));
        v.build(100);
        v.at = v.rows.iter().position(|r| r.head).unwrap();
        v.toggle_reviewed();
        assert_eq!(v.progress(), (1, 4));
        let t = text(&v);
        let h = t.iter().position(|l| l.contains("☑ src/a.rs")).unwrap();
        assert!(t[h + 1].starts_with('╰'), "folded: {t:#?}");
        // the cursor went to the next box
        assert!(v.rows[v.at].head && v.rows[v.at].file == 1);
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
        let i = v.rows.iter().position(|r| text_of(r).contains("let c = 4;")).unwrap();
        v.at = i;
        assert_eq!(v.key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE)), Act::Open("src/a.rs".into(), 12));
        // j / k: box headers
        v.at = v.rows.iter().position(|r| r.head).unwrap();
        v.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert!(v.rows[v.at].head && v.rows[v.at].file == 1);
    }

    fn text_of(r: &Row) -> String {
        r.line.spans.iter().map(|s| s.content.as_ref()).collect()
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
