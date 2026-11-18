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
    Ok(Changes { base: label, files })
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
        }
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
        let mut rows: Vec<Row> = Vec::new();
        for (i, f) in self.changes.files.iter().enumerate() {
            if i > 0 && self.changes.files[i - 1].cat != f.cat {
                rows.push(Row { line: Line::styled(format!(" {}", f.cat.label()), theme::muted().add_modifier(Modifier::BOLD)), file: i, at: None, head: false, hunk: false });
            }
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
        let mut head = vec![Span::styled("╭─ ", border), mark, Span::styled(crate::nav::fit(&name, room), theme::bold())];
        if updated {
            head.push(Span::styled(" updated", Style::default().fg(theme::pal().warn)));
        }
        let used: usize = head.iter().map(|s| s.content.width()).sum();
        let fill = w.saturating_sub(used + right.width() + 3);
        head.push(Span::styled(format!(" {}", "─".repeat(fill)), border));
        head.push(Span::styled(right, theme::muted()));
        head.push(Span::styled("─╮", border));
        let first = f.first_line();
        rows.push(Row { line: Line::from(head), file: i, at: Some(first), head: true, hunk: false });
        if self.folded(f) {
            rows.push(Row { line: Line::styled(format!("╰{}╯", "─".repeat(w.saturating_sub(2))), border), file: i, at: Some(first), head: false, hunk: false });
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
        if f.binary || f.hunks.is_empty() {
            let note = if f.binary { "binary file" } else { "0 line changes (mode or rename)" };
            rows.push(Row { line: bar(vec![Span::styled(note.to_string(), theme::muted())]), file: i, at: Some(first), head: false, hunk: false });
        }
        let num_w = f.hunks.iter().flat_map(|h| &h.lines).filter_map(|l| l.new.max(l.old)).max().unwrap_or(1).to_string().len().max(3);
        for h in &f.hunks {
            let label = format!("⋯ {}", if h.ctx.is_empty() { format!("line {}", h.new) } else { h.ctx.clone() });
            rows.push(Row { line: bar(vec![Span::styled(crate::nav::fit(&label, inner), theme::muted())]), file: i, at: Some(h.new.max(1)), head: false, hunk: true });
            // a new or deleted file: 1 side only, unified uses the room
            if side && !matches!(f.status, Status::New | Status::Deleted) {
                self.side_rows(rows, i, h, inner, num_w, &bar);
            } else {
                for l in &h.lines {
                    let (sign, st, num) = match l.kind {
                        Kind::Add => ("+", Style::default().fg(theme::pal().ok), format!("{:>num_w$} {:>num_w$}", "", l.new.unwrap_or(0))),
                        Kind::Del => ("-", Style::default().fg(theme::pal().danger), format!("{:>num_w$} {:>num_w$}", l.old.unwrap_or(0), "")),
                        Kind::Ctx => (" ", theme::text(), format!("{:>num_w$} {:>num_w$}", l.old.unwrap_or(0), l.new.unwrap_or(0))),
                    };
                    let room = inner.saturating_sub(num.len() + 3);
                    let at = l.new.or(l.old);
                    rows.push(Row {
                        line: bar(vec![Span::styled(num, theme::muted()), Span::styled(format!(" {sign} "), st), Span::styled(crate::nav::fit(&l.text, room), st)]),
                        file: i,
                        at,
                        head: false,
                        hunk: false,
                    });
                }
            }
        }
        rows.push(Row { line: Line::styled(format!("╰{}╯", "─".repeat(w.saturating_sub(2))), border), file: i, at: None, head: false, hunk: false });
    }

    /// Hunk H side by side: the old side left, the new side right; a run of removed lines pairs
    /// with the added lines after it.
    fn side_rows(&self, rows: &mut Vec<Row>, i: usize, h: &Hunk, inner: usize, num_w: usize, bar: &dyn Fn(Vec<Span<'static>>) -> Line<'static>) {
        let half = inner.saturating_sub(3) / 2;
        let cell = |l: Option<&DLine>, left: bool| -> Vec<Span<'static>> {
            let Some(l) = l else { return vec![Span::raw(" ".repeat(half))] };
            let (st, sign) = match l.kind {
                Kind::Add => (Style::default().fg(theme::pal().ok), "+"),
                Kind::Del => (Style::default().fg(theme::pal().danger), "-"),
                Kind::Ctx => (theme::text(), " "),
            };
            let n = if left { l.old } else { l.new }.unwrap_or(0);
            let room = half.saturating_sub(num_w + 3);
            let text = crate::nav::fit(&l.text, room);
            let pad = room.saturating_sub(text.width());
            vec![Span::styled(format!("{n:>num_w$}"), theme::muted()), Span::styled(format!(" {sign} "), st), Span::styled(text, st), Span::raw(" ".repeat(pad))]
        };
        let mut k = 0;
        let ls = &h.lines;
        while k < ls.len() {
            if ls[k].kind == Kind::Ctx {
                let mut s = cell(Some(&ls[k]), true);
                s.push(Span::styled(" │ ", theme::line()));
                s.extend(cell(Some(&ls[k]), false));
                rows.push(Row { line: bar(s), file: i, at: ls[k].new, head: false, hunk: false });
                k += 1;
                continue;
            }
            let dels: Vec<&DLine> = ls[k..].iter().take_while(|l| l.kind == Kind::Del).collect();
            let adds: Vec<&DLine> = ls[k + dels.len()..].iter().take_while(|l| l.kind == Kind::Add).collect();
            for j in 0..dels.len().max(adds.len()) {
                let (d, a) = (dels.get(j).copied(), adds.get(j).copied());
                let mut s = cell(d, true);
                s.push(Span::styled(" │ ", theme::line()));
                s.extend(cell(a, false));
                let at = a.and_then(|a| a.new).or_else(|| adds.first().and_then(|a| a.new)).or_else(|| d.and_then(|d| d.old));
                rows.push(Row { line: bar(s), file: i, at, head: false, hunk: false });
            }
            k += dels.len() + adds.len();
        }
    }

    pub fn draw(&mut self, f: &mut Frame, area: Rect) {
        self.area = area;
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
            KeyCode::Char('r') => {
                self.refresh(true);
                return Act::Toast("changes: reading git".into());
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
        v.changes = Changes { base: "origin/main".into(), files };
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
        assert!(t[0].starts_with("╭─ ☐ src/a.rs") && t[0].ends_with("logic · +2 −1 ─╮"), "{t:#?}");
        assert!(t.iter().any(|l| l.contains("⋯ fn run() {")), "{t:#?}");
        assert!(t.iter().any(|l| l.contains(" 11     - let b = 2;")), "{t:#?}");
        assert!(t.iter().any(|l| l.contains("     11 + let b = 3;")), "{t:#?}");
        assert!(t.iter().filter(|l| !l.starts_with(' ')).all(|l| l.width() == 120), "every box row 120 wide: {t:#?}");
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
    fn space_marks_reviewed_folds_and_a_changed_diff_shows_updated() {
        let mut v = view(parse(DIFF));
        v.build(100);
        v.at = 0;
        v.toggle_reviewed();
        assert_eq!(v.progress(), (1, 4));
        let t = text(&v);
        assert!(t[0].contains("☑ src/a.rs") && t[1].starts_with('╰'), "folded: {t:#?}");
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
        v.at = 0;
        v.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert!(v.rows[v.at].head && v.rows[v.at].file == 1);
    }

    fn text_of(r: &Row) -> String {
        r.line.spans.iter().map(|s| s.content.as_ref()).collect()
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
}

/// Live look: CODEHEAD_CHANGES_DIR=<repo> cargo test changes_live -- --ignored --nocapture
#[cfg(test)]
mod live {
    use super::*;

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
