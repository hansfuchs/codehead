//! `>` in a prompt input: a command dropdown above the prompt box (never over it), filtered as the
//! text after the `>` is typed. Levels, each a dropdown beside its parent (0 room beside: above them):
//! - `mention` → kinds (files, notes, threads, artifacts) → the items of the kind: oldest on top,
//!   the latest at the bottom and current; typed text: fuzzy matches, the best at the bottom.
//!   enter / tab inserts the item (relative file path, note path, thread name, artifact url).
//! - `branch` (thread view): the thread copied into a new one, the view switches to it.
//! - `skills` → the skills of the thread's harnesses, with their description; enter inserts `/name`.
//!
//! ↑↓ move (items: the item goes into the text at once, a live preview), enter / tab open the next level. esc (or ← / ⌫ on an empty filter): 1 level back; on
//! the 1st level: closed until the next `>`.

use std::path::{Path, PathBuf};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::dropdown;
use crate::textarea::{Pos, TextArea};
use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    Files,
    Notes,
    Threads,
    Artifacts,
    Skills,
}

/// A command the host runs (it has the thread).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Branch,
    Handoff,
    Reload,
    /// The flow box of the agent a message goes to now, on the flows (change / set its flow).
    Flow,
}

/// A dropdown of the open path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Level {
    Commands,
    /// Kinds of `mention`.
    Kinds,
    Items(Kind),
}

const MENTION: usize = 0;
const BRANCH: usize = 1;
const SKILLS: usize = 2;
const HANDOFF: usize = 3;
const RELOAD: usize = 4;
const FLOW: usize = 5;
const SUB: usize = 6;
/// 1st level: name, what it does.
const COMMANDS: [(&str, &str); 7] = [("mention", "path · name · url"), ("branch", "copy of this thread"), ("skills", "of the harnesses"), ("handoff", "new thread from a summary of this one"), ("reload", "new harness sessions: MCP servers, settings"), ("flow", "set / change the flow of the agent a message goes to"), ("sub", "sub-thread: 0 history, same checkout, reports here")];
/// 2nd level of `mention`: kind, name, what goes into the text.
const KINDS: [(Kind, &str, &str); 4] =
    [(Kind::Files, "files", "relative path"), (Kind::Notes, "notes", "path"), (Kind::Threads, "threads", "name"), (Kind::Artifacts, "artifacts", "url")];

/// Widest item dropdown (columns); skills (name + description): wider.
const MAX_W: usize = 72;
const MAX_W_SKILLS: usize = 110;
/// Most rows of the description box above the skills.
const INFO_ROWS: usize = 6;
/// Most files listed (the newest win).
const MAX_FILES: usize = 50_000;

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Item {
    /// Shown, and what the typed text matches.
    pub label: String,
    /// Shown right, gray (age, place, description), cut to the room.
    pub detail: String,
    /// Goes into the text.
    pub insert: String,
    /// Seconds since the epoch (skills: the row number): the order.
    pub time: u64,
    /// Long text of the current row, in a box above the dropdown (skills: the description).
    pub info: String,
}

#[derive(Default)]
pub struct Command {
    /// The `>` this state belongs to. Another `>`: 1st level again.
    at: Option<Pos>,
    /// esc on the 1st level of the `>` at this position: closed until another `>`.
    closed: Option<Pos>,
    /// Open levels below the commands (`Kinds`, `Items`).
    path: Vec<Level>,
    /// Per level: the row (menus: its index in COMMANDS / KINDS once a submenu is open), the 1st row in view.
    cursor: [usize; 3],
    offset: [usize; 3],
    /// Typed text the cursor of the open level belongs to (new text: the cursor goes to its default).
    seen: Option<(usize, String)>,
    items: Vec<Item>,
    /// Items in view order for the filter text (index into `items`).
    matched: (String, Vec<usize>),
    /// Widest label + detail of the items.
    items_w: usize,
    /// The artifact scan runs: the items come later.
    loading: bool,
    reads: crate::background::Reads<String, Vec<Item>>,
    cache: HashMap<String, Vec<Item>>,
    source: Option<String>,
    title_reads: crate::background::Reads<String, Option<String>>,
    title_checked: HashSet<String>,
    /// ↑↓ on items: the item in the text in place of `>query` (the typed query: the filter).
    preview: dropdown::Preview,
    /// Rows of the last draw (mouse): area, level, row.
    rows: Vec<(Rect, usize, usize)>,
    /// Dropdown areas of the last draw.
    areas: Vec<Rect>,
    /// Files: the repo / worktree (else the dir) they come from.
    pub cwd: PathBuf,
    /// A thread is open: `branch` shows.
    pub can_branch: bool,
    /// Harnesses of the thread's agents (task screen: of the picked agents), for `skills`.
    pub harnesses: Vec<String>,
    /// `branch` picked: the host takes it.
    pub action: Option<Action>,
}

fn secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn age(time: u64) -> String {
    if time == 0 { String::new() } else { crate::notes::ago(UNIX_EPOCH + Duration::from_secs(time)) }
}

/// Fuzzy score of Q in a path-like LABEL, like the VS Code file picker: Q's chars in order, a run
/// and a word start score more, a match in the last path part counts double. None: 0 match.
pub fn score(q: &str, label: &str) -> Option<i64> {
    let q = q.to_lowercase();
    let whole = crate::notes::fuzzy(&q, label)?;
    let base = label.rsplit('/').next().unwrap_or(label);
    let in_base = crate::notes::fuzzy(&q, base).map_or(0, |s| s * 2);
    // a shorter label with the same match: closer to what was typed
    Some(whole.max(in_base) * 4 - (label.chars().count() as i64).min(200) / 8)
}

/// Items in view order for Q: all by time (oldest first, the latest last); with text: the matches,
/// the best last (ties: the newer last). A match in the long text (`info`) only: before the others.
pub fn matches(items: &[Item], q: &str) -> Vec<usize> {
    if q.is_empty() {
        return (0..items.len()).collect();
    }
    let in_info = |it: &Item| if it.info.is_empty() { None } else { crate::notes::fuzzy(&q.to_lowercase(), &it.info).filter(|s| *s >= 100).map(|s| s - 10_000) };
    let mut v: Vec<(i64, usize)> = items.iter().enumerate().filter_map(|(i, it)| score(q, &it.label).or_else(|| in_info(it)).map(|s| (s, i))).collect();
    v.sort_by_key(|(s, i)| (*s, items[*i].time, *i));
    v.into_iter().map(|(_, i)| i).collect()
}

/// Skills of HARNESSES as items, by harness + name (the 1st on top). `/name` goes into the text
/// (Codex: `$name`).
pub fn skills(harnesses: &[String], cwd: &Path) -> Vec<Item> {
    let all = crate::skills::load(harnesses, cwd);
    let many = all.iter().any(|s| s.harness != all[0].harness);
    all.into_iter()
        .enumerate()
        .map(|(i, s)| {
            let sigil = if s.harness == "codex" { '$' } else { '/' };
            let label = if many { format!("{} · {}", s.name, s.harness) } else { s.name.clone() };
            Item { label, detail: s.description.clone(), insert: format!("{sigil}{}", s.name), time: i as u64, info: s.description }
        })
        .collect()
}

/// Files of the git repo / worktree that holds CWD (tracked + untracked, ignored ones left out),
/// paths relative to its top dir. 0 git: the files below CWD. Oldest first.
pub fn files(cwd: &Path) -> Vec<Item> {
    let (ok, top, _) = crate::threads::git(cwd, &["rev-parse", "--show-toplevel"]);
    let mut paths: Vec<(u64, String)> = Vec::new();
    if ok && !top.trim().is_empty() {
        let root = PathBuf::from(top.trim());
        let (_, out, _) = crate::threads::git(&root, &["ls-files", "-z", "--cached", "--others", "--exclude-standard"]);
        for rel in out.split('\0').filter(|p| !p.is_empty()) {
            // a deleted, not yet committed file: listed by git, gone on disk
            if let Ok(m) = std::fs::metadata(root.join(rel)) {
                paths.push((m.modified().map(secs).unwrap_or(0), rel.to_string()));
            }
        }
    } else {
        walk(cwd, cwd, 0, &mut paths);
    }
    paths.sort();
    paths.dedup_by(|a, b| a.1 == b.1);
    let skip = paths.len().saturating_sub(MAX_FILES);
    paths.into_iter().skip(skip).map(|(time, rel)| Item { detail: age(time), insert: rel.clone(), label: rel, time, info: String::new() }).collect()
}

/// 0 git: files below DIR (hidden entries and build dirs left out, max. 6 levels).
fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<(u64, String)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if out.len() >= MAX_FILES {
            return;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || matches!(name.as_str(), "node_modules" | "target" | "dist" | "build" | "__pycache__") {
            continue;
        }
        let Ok(meta) = e.metadata() else { continue };
        let path = e.path();
        if meta.is_dir() {
            if depth < 6 {
                walk(root, &path, depth + 1, out);
            }
        } else {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().into_owned();
            out.push((meta.modified().map(secs).unwrap_or(0), rel));
        }
    }
}

/// Notes of the notes root: shown relative, inserted as the full path. Oldest first.
pub fn notes(root: &Path) -> Vec<Item> {
    let n = crate::notes::Notes::load(root);
    let mut v: Vec<Item> = n
        .files
        .iter()
        .map(|rel| {
            let time = secs(n.mtime(rel));
            Item { label: rel.clone(), detail: age(time), insert: n.abs(rel).display().to_string(), time, info: String::new() }
        })
        .collect();
    v.sort_by_key(|i| i.time);
    v
}

/// Every thread: its name (1st line of the title). Oldest first.
fn threads() -> Vec<Item> {
    let mut v: Vec<Item> = crate::threads::all_threads_lite()
        .into_iter()
        .filter(|t| !t.title.trim().is_empty())
        .map(|t| {
            let name = t.title.lines().next().unwrap_or_default().trim().to_string();
            let place = t.branch.clone().or_else(|| t.dir.as_deref().and_then(|d| Path::new(d).file_name()).map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
            let detail = [place, age(t.updated)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
            Item { label: name.clone(), detail, insert: name, time: t.updated, info: String::new() }
        })
        .collect();
    v.sort_by_key(|i| i.time);
    v
}

/// Artifact urls (`https://claude.ai/…artifact/<id>`) in TEXT, the share key (`?sk=…`) left out.
pub fn artifact_urls(text: &[u8]) -> Vec<String> {
    static RE: std::sync::OnceLock<regex::bytes::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::bytes::Regex::new(r"https://claude\.ai/(?:[a-z]+/)?artifact/[A-Za-z0-9_-]{8,}").expect("valid regex"));
    let mut v: Vec<String> = re.find_iter(text).map(|m| String::from_utf8_lossy(m.as_bytes()).into_owned()).collect();
    v.sort();
    v.dedup();
    v
}

/// Titles explicitly attached to artifact links in decoded transcript text.
fn artifact_titles(text: &str, titles: &mut HashMap<String, String>) {
    static LINKS: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = LINKS.get_or_init(|| regex::Regex::new(r"\[([^\]\n]+)\]\(<?(https://claude\.ai/(?:[a-z]+/)?artifact/[A-Za-z0-9_-]{8,})[^)]*\)").unwrap());
    for c in re.captures_iter(text) {
        let title = c[1].trim();
        if !title.is_empty() && !title.contains("claude.ai/") && !title.starts_with("http") {
            titles.insert(c[2].to_string(), title.to_string());
        }
    }
}

fn transcript_titles(value: &serde_json::Value, titles: &mut HashMap<String, String>, depth: usize) {
    if depth > 24 { return; }
    match value {
        serde_json::Value::String(text) => {
            artifact_titles(text, titles);
            if let Ok(decoded) = serde_json::from_str::<serde_json::Value>(text) {
                transcript_titles(&decoded, titles, depth + 1);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values { transcript_titles(value, titles, depth + 1); }
        }
        serde_json::Value::Object(values) => {
            // Only a direct artifact URL/title pair, never the enclosing issue's title.
            if let Some(title) = values.get("title").or_else(|| values.get("name")).and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()) {
                for key in ["url", "uri", "href", "artifact_url", "artifactUrl"] {
                    if let Some(link) = values.get(key).and_then(|v| v.as_str()) {
                        for url in artifact_urls(link.as_bytes()) { titles.insert(url, title.trim().to_string()); }
                    }
                }
            }
            for value in values.values() { transcript_titles(value, titles, depth + 1); }
        }
        _ => {}
    }
}

fn page_title(html: &str) -> Option<String> {
    static TITLE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = TITLE.get_or_init(|| regex::Regex::new(r"(?s)<[tT][iI][tT][lL][eE][^>]*>(.*?)</[tT][iI][tT][lL][eE]>").unwrap());
    let raw = re.captures(html)?.get(1)?.as_str();
    let title = raw.replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'").replace("&lt;", "<").replace("&gt;", ">");
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = title.strip_suffix(" - Claude").or_else(|| title.strip_suffix(" | Claude")).unwrap_or(&title).trim();
    if title.is_empty() || ["claude", "claude artifact", "claude artifacts", "just a moment...", "access denied"].contains(&title.to_lowercase().as_str()) {
        None
    } else { Some(title.to_string()) }
}

/// Public metadata only, bounded time/body size. Never send browser credentials.
fn remote_artifact_title(url: &str) -> Option<String> {
    let name = format!("artifact-title-{}", crate::background::scope(Path::new(url)));
    let now = secs(SystemTime::now());
    if let Some((at, title)) = crate::background::snapshot::<(u64, Option<String>)>(&name)
        && now.saturating_sub(at) < 3600
    { return title; }
    let title = std::process::Command::new("curl")
        .args(["--silent", "--fail", "--location", "--proto", "=https", "--proto-redir", "=https", "--connect-timeout", "2", "--max-time", "5", "--max-filesize", "2097152", url])
        .stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()
        .filter(|o| o.status.success()).and_then(|o| page_title(&String::from_utf8_lossy(&o.stdout)));
    crate::background::save(&name, &(now, &title));
    title
}

pub fn scan_artifacts(root: &Path) -> Vec<Item> {
    let mut found: std::collections::HashMap<String, (u64, String)> = std::collections::HashMap::new();
    let mut titles = HashMap::new();
    let mut dirs = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = dirs.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                if depth < 3 {
                    dirs.push((path, depth + 1));
                }
                continue;
            }
            if path.extension().is_none_or(|x| x != "jsonl") {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else { continue };
            if artifact_urls(&bytes).is_empty() { continue; }
            for line in bytes.split(|b| *b == b'\n') {
                if let Ok(value) = serde_json::from_slice(line) { transcript_titles(&value, &mut titles, 0); }
            }
            let time = meta.modified().map(secs).unwrap_or(0);
            // project dir `-Users-me-projects-app` → `app`
            let project = path.strip_prefix(root).ok().and_then(|p| p.components().next()).map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default();
            let place = project.rsplit('-').next().unwrap_or_default().to_string();
            for url in artifact_urls(&bytes) {
                let e = found.entry(url).or_insert((0, String::new()));
                if time >= e.0 {
                    *e = (time, place.clone());
                }
            }
        }
    }
    let mut v: Vec<Item> = found
        .into_iter()
        .map(|(url, (time, place))| {
            let name = format!("artifact-title-{}", crate::background::scope(Path::new(&url)));
            let saved = crate::background::snapshot::<(u64, Option<String>)>(&name).and_then(|(_, title)| title);
            let label = titles.remove(&url).or(saved).unwrap_or_else(|| format!("untitled artifact · {}", url.rsplit('/').next().unwrap_or_default()));
            let detail = [place, age(time)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
            Item { label, detail, insert: url, time, info: String::new() }
        })
        .collect();
    v.sort_by(|a, b| (a.time, &a.label).cmp(&(b.time, &b.label)));
    v
}

/// The last W columns of S (`…` in front when cut): a path keeps its file name.
fn tail(s: &str, w: usize) -> String {
    if s.width() <= w {
        return s.to_string();
    }
    let mut out: Vec<char> = Vec::new();
    let mut used = 1;
    for c in s.chars().rev() {
        let cw = c.width().unwrap_or(0);
        if used + cw > w {
            break;
        }
        used += cw;
        out.push(c);
    }
    out.push('…');
    out.iter().rev().collect()
}

/// The first W columns of S (`…` at the end when cut).
fn head(s: &str, w: usize) -> String {
    if s.width() <= w {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 1;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if used + cw > w {
            break;
        }
        used += cw;
        out.push(c);
    }
    out.push('…');
    out
}

impl Command {
    pub fn new(cwd: &Path) -> Command {
        Command { cwd: cwd.to_path_buf(), ..Default::default() }
    }

    /// The `>` under the cursor + the text after it. None: closed (state back to the 1st level).
    fn sync(&mut self, ta: &TextArea) -> Option<(Pos, String)> {
        // a live preview in the text: the `>query` typed before it stays open
        if let Some((at, typed)) = self.preview.typed(ta)
            && self.at == Some(at)
        {
            return Some((at, typed.chars().skip(1).collect()));
        }
        let found = ta.command();
        // the cursor left the closed `>`: a `>` typed there later opens again
        if found.is_none() {
            self.closed = None;
        }
        let Some((at, q)) = found.filter(|(at, _)| self.closed != Some(*at)) else {
            self.reset();
            return None;
        };
        if self.at != Some(at) {
            self.reset();
            self.at = Some(at);
        }
        Some((at, q))
    }

    fn reset(&mut self) {
        self.at = None;
        self.path.clear();
        self.cursor = [0; 3];
        self.offset = [0; 3];
        self.seen = None;
        self.items.clear();
        self.matched = (String::new(), Vec::new());
        self.loading = false;
        self.source = None;
    }

    /// The artifact scan runs: draw again soon.
    pub fn busy(&self) -> bool {
        self.loading || self.reads.pending() > 0 || self.title_reads.pending() > 0
    }

    /// Index of the open level (0: commands).
    fn depth(&self) -> usize {
        self.path.len()
    }

    fn level(&self, d: usize) -> Level {
        if d == 0 { Level::Commands } else { self.path[d - 1] }
    }

    /// Rows of menu LEVEL that match Q (prefix first, then names that hold Q): indices into
    /// COMMANDS / KINDS. `branch`: only with a thread.
    fn menu(&self, level: Level, q: &str) -> Vec<usize> {
        let names: Vec<(usize, &str)> = match level {
            Level::Commands => COMMANDS.iter().enumerate().filter(|(i, _)| !matches!(*i, BRANCH | HANDOFF | RELOAD | FLOW | SUB) || self.can_branch).map(|(i, c)| (i, c.0)).collect(),
            _ => KINDS.iter().enumerate().map(|(i, k)| (i, k.1)).collect(),
        };
        let q = q.to_lowercase();
        let mut v: Vec<usize> = names.iter().filter(|(_, n)| n.starts_with(&q)).map(|(i, _)| *i).collect();
        v.extend(names.iter().filter(|(_, n)| !n.starts_with(&q) && crate::notes::fuzzy(&q, n).is_some()).map(|(i, _)| *i));
        v
    }

    fn load(&mut self, kind: Kind) {
        let cwd = self.cwd.clone();
        let harnesses = self.harnesses.clone();
        let root = crate::config::notes();
        let scope = format!("{kind:?}:{cwd:?}:{root:?}:{harnesses:?}");
        let name = format!("mention-{}", crate::background::scope(Path::new(&scope)));
        let cached = self.cache.entry(scope.clone()).or_insert_with(|| crate::background::snapshot(&name).unwrap_or_default());
        self.items = cached.clone();
        self.loading = self.items.is_empty();
        self.source = Some(scope.clone());
        self.reads.request(scope, move || {
            let items = match kind {
                Kind::Files => files(&cwd),
                Kind::Notes => notes(&root),
                Kind::Threads => threads(),
                Kind::Skills => skills(&harnesses, &cwd),
                Kind::Artifacts => scan_artifacts(&crate::config::home().join(".claude/projects")),
            };
            crate::background::save(&name, &items);
            items
        });
        self.set_items();
    }

    fn set_items(&mut self) {
        self.items_w = self.items.iter().map(|i| i.label.width() + if i.detail.is_empty() { 0 } else { i.detail.width() + 2 }).max().unwrap_or(0);
        self.matched = (String::new(), (0..self.items.len()).collect());
        self.seen = None;
    }

    fn update_items(&mut self, items: Vec<Item>) {
        if self.items.is_empty() {
            self.items = items;
            self.set_items();
            return;
        }
        let d = self.depth();
        let keep = self.matched.1.get(self.cursor[d]).and_then(|&i| self.items.get(i)).map(|i| i.insert.clone());
        let q = self.matched.0.clone();
        self.items = items;
        self.items_w = self.items.iter().map(|i| i.label.width() + if i.detail.is_empty() { 0 } else { i.detail.width() + 2 }).max().unwrap_or(0);
        let mut matched = matches(&self.items, &q);
        if Self::from_top(self.level(d)) && !q.is_empty() { matched.reverse(); }
        self.cursor[d] = keep.and_then(|s| matched.iter().position(|&i| self.items[i].insert == s)).unwrap_or_else(|| self.cursor[d].min(matched.len().saturating_sub(1)));
        self.matched = (q, matched);
    }

    #[cfg(test)]
    fn set_test_items(&mut self, kind: Kind, items: Vec<Item>) {
        self.path = if kind == Kind::Skills { vec![Level::Items(kind)] } else { vec![Level::Kinds, Level::Items(kind)] };
        self.items = items;
        self.set_items();
    }

    /// Skills read top-down (a → z, the 1st row current); the other lists: the latest at the bottom, current.
    fn from_top(level: Level) -> bool {
        !matches!(level, Level::Items(k) if k != Kind::Skills)
    }

    /// Rows of the open level for Q, the cursor on its default after new text.
    fn settle(&mut self, q: &str) -> usize {
        for (source, items) in self.reads.poll() {
            self.cache.insert(source.clone(), items.clone());
            if self.source.as_ref() == Some(&source) {
                self.loading = false;
                self.update_items(items);
            }
        }
        for (url, title) in self.title_reads.poll() {
            if let Some(title) = title {
                for items in self.cache.values_mut() {
                    for item in items.iter_mut().filter(|i| i.insert == url) { item.label = title.clone(); }
                }
                let mut items = self.items.clone();
                for item in items.iter_mut().filter(|i| i.insert == url) { item.label = title.clone(); }
                self.update_items(items);
            }
        }
        if self.level(self.depth()) == Level::Items(Kind::Artifacts) {
            for item in &self.items {
                if self.title_reads.pending() >= 4 { break; }
                if item.label.starts_with("untitled artifact · ") && self.title_checked.insert(item.insert.clone()) {
                    let url = item.insert.clone();
                    self.title_reads.request(url.clone(), move || remote_artifact_title(&url));
                }
            }
        }
        let d = self.depth();
        let level = self.level(d);
        let n = if let Level::Items(_) = level {
            if self.matched.0 != q {
                let mut m = matches(&self.items, q);
                // top-down list: the best match on top
                if Self::from_top(level) && !q.is_empty() {
                    m.reverse();
                }
                self.matched = (q.to_string(), m);
            }
            self.matched.1.len()
        } else {
            self.menu(level, q).len()
        };
        if self.seen.as_ref().is_none_or(|(sd, sq)| *sd != d || sq != q) {
            let top = Self::from_top(level);
            self.cursor[d] = if top { 0 } else { n.saturating_sub(1) };
            self.offset[d] = if top { 0 } else { n.saturating_sub(dropdown::MAX_ROWS) };
            self.seen = Some((d, q.to_string()));
        }
        self.cursor[d] = self.cursor[d].min(n.saturating_sub(1));
        n
    }

    /// 1 level deeper, the command's action, or the item into the text.
    fn activate(&mut self, ta: &mut TextArea, at: Pos, q: &str) {
        let d = self.depth();
        match self.level(d) {
            Level::Commands => {
                let Some(&i) = self.menu(Level::Commands, q).get(self.cursor[0]) else { return };
                self.cursor[0] = i;
                match i {
                    BRANCH => {
                        // the `>text` goes, the host branches the thread
                        ta.replace_to_cursor(at, "");
                        self.reset();
                        self.action = Some(Action::Branch);
                        return;
                    }
                    HANDOFF => {
                        ta.replace_to_cursor(at, "");
                        self.reset();
                        self.action = Some(Action::Handoff);
                        return;
                    }
                    RELOAD => {
                        ta.replace_to_cursor(at, "");
                        self.reset();
                        self.action = Some(Action::Reload);
                        return;
                    }
                    FLOW => {
                        ta.replace_to_cursor(at, "");
                        self.reset();
                        self.action = Some(Action::Flow);
                        return;
                    }
                    SUB => {
                        // `>sub ` stays in the text: the task follows (@agent, #shortcut, any
                        // text); the send starts the sub-thread
                        ta.replace_to_cursor(at, ">sub ");
                        self.reset();
                        return;
                    }
                    SKILLS => {
                        self.path.push(Level::Items(Kind::Skills));
                        self.load(Kind::Skills);
                    }
                    _ => self.path.push(Level::Kinds),
                }
            }
            Level::Kinds => {
                let Some(&i) = self.menu(Level::Kinds, q).get(self.cursor[1]) else { return };
                self.cursor[1] = i;
                self.path.push(Level::Items(KINDS[i].0));
                self.load(KINDS[i].0);
            }
            Level::Items(_) => {
                let Some(item) = self.matched.1.get(self.cursor[d]).and_then(|&i| self.items.get(i)) else { return };
                ta.replace_to_cursor(at, &format!("{} ", item.insert));
                self.reset();
                return;
            }
        }
        self.seen = None;
        if !q.is_empty() {
            ta.replace_to_cursor(at, &crate::config::command_key().to_string());
        }
    }

    /// 1 level back; on the 1st: closed for this `>`.
    fn back(&mut self, ta: &mut TextArea, at: Pos, q: &str) {
        if self.path.pop().is_none() {
            self.closed = Some(at);
            self.reset();
            return;
        }
        // the parent keeps its row; `seen` holds the empty filter the parent comes back with
        self.seen = Some((self.depth(), String::new()));
        if !q.is_empty() {
            ta.replace_to_cursor(at, &crate::config::command_key().to_string());
        }
    }

    #[cfg(test)]
    fn open_level(&mut self, ta: &TextArea) -> Option<usize> {
        self.sync(ta).map(|_| self.depth())
    }

    /// Dropdown areas of the last draw.
    #[cfg(test)]
    pub fn areas(&self) -> &[Rect] {
        &self.areas
    }

    /// The dropdown shows (as `key` sees it: a deeper level, or 1+ command matching the text).
    pub fn is_open(&mut self, ta: &TextArea) -> bool {
        let Some((_, q)) = self.sync(ta) else { return false };
        self.settle(&q) > 0 || self.depth() > 0
    }

    /// Key for the dropdown. true: used (the input must not see it).
    pub fn key(&mut self, k: KeyEvent, ta: &mut TextArea) -> bool {
        let Some((at, q)) = self.sync(ta) else { return false };
        let n = self.settle(&q);
        let d = self.depth();
        // 1st level, 0 commands match the text: not a command, keys pass
        if d == 0 && n == 0 {
            return false;
        }
        let plain = k.modifiers.is_empty();
        let menu = !matches!(self.level(d), Level::Items(_));
        match k.code {
            KeyCode::Up | KeyCode::Down if plain && n > 0 => {
                self.cursor[d] = if k.code == KeyCode::Up { (self.cursor[d] + n - 1) % n } else { (self.cursor[d] + 1) % n };
                // items: the item in the text at once
                if !menu && let Some(item) = self.matched.1.get(self.cursor[d]).and_then(|&i| self.items.get(i)) {
                    let typed = format!("{}{q}", crate::config::command_key());
                    let insert = item.insert.clone();
                    self.preview.show(ta, at, &typed, &insert);
                }
            }
            KeyCode::PageUp if plain && n > 0 => self.cursor[d] = self.cursor[d].saturating_sub(dropdown::MAX_ROWS),
            KeyCode::PageDown if plain && n > 0 => self.cursor[d] = (self.cursor[d] + dropdown::MAX_ROWS).min(n - 1),
            KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown if plain => {}
            KeyCode::Tab | KeyCode::Enter if plain => self.activate(ta, at, &q),
            KeyCode::Right if plain && menu && q.is_empty() && !(d == 0 && self.menu(Level::Commands, "").get(self.cursor[0]) == Some(&BRANCH)) => self.activate(ta, at, &q),
            KeyCode::Esc if plain => self.back(ta, at, &q),
            // a filter char after a preview: the typed query again, the char after it
            KeyCode::Char(c) if plain && !c.is_whitespace() && self.preview.revert(ta) => return false,
            KeyCode::Left | KeyCode::Backspace if plain && d > 0 && q.is_empty() => self.back(ta, at, &q),
            _ => return false,
        }
        true
    }


    /// Row of the last draw at P (hover).
    pub fn row_at(&self, p: Position) -> Option<Rect> {
        self.rows.iter().map(|(r, _, _)| *r).find(|r| r.contains(p))
    }

    /// Click on a row: that level's row, opened / inserted. Wheel over the open level: its cursor.
    /// true: used.
    pub fn mouse(&mut self, m: MouseEvent, ta: &mut TextArea) -> bool {
        let p = Position { x: m.column, y: m.row };
        if !self.areas.iter().any(|r| r.contains(p)) {
            return false;
        }
        let Some((at, q)) = self.sync(ta) else { return false };
        let n = self.settle(&q);
        let d = self.depth();
        match m.kind {
            MouseEventKind::ScrollUp if n > 0 => self.cursor[d] = self.cursor[d].saturating_sub(3),
            MouseEventKind::ScrollDown if n > 0 => self.cursor[d] = (self.cursor[d] + 3).min(n - 1),
            MouseEventKind::Down(MouseButton::Left) => {
                let Some((_, level, row)) = self.rows.iter().find(|(r, _, _)| r.contains(p)).copied() else { return true };
                if level < d {
                    // a row of a parent: back to it, then its submenu (rows of a parent: unfiltered)
                    self.path.truncate(level);
                    self.cursor[level] = row;
                    self.seen = Some((level, String::new()));
                    if !q.is_empty() {
                        ta.replace_to_cursor(at, &crate::config::command_key().to_string());
                    }
                    self.activate(ta, at, "");
                } else {
                    self.cursor[d] = row;
                    self.activate(ta, at, &q);
                }
            }
            _ => {}
        }
        true
    }

    /// The dropdowns for the `>` of TA (TA rendered this frame), inside BOUNDS and never over AVOID
    /// (the prompt box): above it (0 room there: below it). The 1st at the column of the `>`, each
    /// next level beside its parent; 0 room beside: above the dropdowns.
    pub fn render(&mut self, f: &mut Frame, ta: &TextArea, bounds: Rect, avoid: Rect) {
        self.rows.clear();
        self.areas.clear();
        let Some((at, q)) = self.sync(ta) else { return };
        let n = self.settle(&q);
        let depth = self.depth();
        if depth == 0 && n == 0 {
            return;
        }
        let Some(p) = ta.screen_pos(at) else { return };
        // the room above the prompt box; less than 5 rows there and more below: below it
        let above = Rect { height: avoid.y.saturating_sub(bounds.y).min(bounds.height), ..bounds };
        let below_y = avoid.bottom().clamp(bounds.y, bounds.bottom());
        let below = Rect { y: below_y, height: bounds.bottom() - below_y, ..bounds };
        // above: more room there, or room for a full list (12 rows)
        let up = above.height >= 12 || above.height >= below.height;
        let room = if up { above } else { below };
        let mut parent: Option<(Rect, u16)> = None;
        let mut stack: Option<Rect> = None;
        for d in 0..=depth {
            let level = self.level(d);
            let active = d == depth;
            let lq = if active { q.as_str() } else { "" };
            let items = matches!(level, Level::Items(_));
            let menu: Vec<usize> = if items { Vec::new() } else { self.menu(level, lq) };
            let len = if items { self.matched.1.len() } else { menu.len() };
            let cursor = if !items && !active { menu.iter().position(|&i| i == self.cursor[d]).unwrap_or(0) } else { self.cursor[d] };
            let (label, w) = match level {
                Level::Commands => ("commands".to_string(), menu.iter().map(|&i| COMMANDS[i].0.width() + COMMANDS[i].1.width()).max().unwrap_or(0) + 8),
                Level::Kinds => (COMMANDS[MENTION].0.to_string(), KINDS.iter().map(|k| k.1.width() + k.2.width()).max().unwrap_or(0) + 8),
                Level::Items(Kind::Skills) => (COMMANDS[SKILLS].0.to_string(), (self.items_w + 4).clamp(40, MAX_W_SKILLS)),
                Level::Items(kind) => (format!("{} › {}", COMMANDS[MENTION].0, KINDS.iter().find(|k| k.0 == kind).map_or("items", |k| k.1)), (self.items_w + 4).clamp(28, MAX_W)),
            };
            let rows = len.max(1);
            let area = match (parent, stack) {
                (Some((r, y)), Some(st)) => dropdown::place_beside(r, y, w as u16, rows, room, st),
                _ if up => dropdown::place(Rect { x: p.x, y: avoid.y, width: 1, height: 1 }, w as u16, rows, room, true),
                _ => dropdown::place(Rect { x: p.x, y: avoid.bottom().saturating_sub(1), width: 1, height: 1 }, w as u16, rows, room, false),
            };
            let Some(area) = area else { return };
            let shown = (area.height as usize).saturating_sub(2);
            self.offset[d] = dropdown::fit(self.offset[d], cursor, len, shown);
            let off = self.offset[d];
            let note = match level {
                Level::Items(_) if self.loading => "scan…".to_string(),
                Level::Items(_) if !lq.is_empty() => format!("{len} of {}", self.items.len()),
                Level::Items(_) => format!("{len}"),
                _ => String::new(),
            };
            let inner = dropdown::frame(f, area, &label, &note, off > 0, off + shown < len, active);
            self.areas.push(area);
            stack = Some(stack.map_or(area, |s| s.union(area)));
            if len == 0 {
                let text = if self.loading { " scan of the transcripts…" } else if lq.is_empty() { " 0 entries" } else { " 0 matches" };
                f.render_widget(Paragraph::new(Line::styled(text, theme::muted())), Rect { height: inner.height.min(1), ..inner });
            }
            let mut cur_y = inner.y;
            for i in 0..shown.min(len.saturating_sub(off)) {
                let idx = off + i;
                let row = Rect { y: inner.y + i as u16, height: 1, ..inner };
                let hl = idx == cursor;
                if hl {
                    cur_y = row.y;
                }
                let (text, detail, submenu) = match level {
                    Level::Commands => (COMMANDS[menu[idx]].0.to_string(), COMMANDS[menu[idx]].1.to_string(), !matches!(menu[idx], BRANCH | HANDOFF | RELOAD | FLOW | SUB)),
                    Level::Kinds => (KINDS[menu[idx]].1.to_string(), KINDS[menu[idx]].2.to_string(), true),
                    Level::Items(_) => {
                        let it = &self.items[self.matched.1[idx]];
                        (it.label.clone(), it.detail.clone(), false)
                    }
                };
                let st = match (hl, active) {
                    (true, true) => theme::pill().add_modifier(Modifier::BOLD),
                    (true, false) => Style::default().bg(theme::pal().panel).fg(theme::pal().accent).add_modifier(Modifier::BOLD),
                    _ => Style::default().fg(theme::pal().text),
                };
                let dim = if hl && active { st.remove_modifier(Modifier::BOLD) } else { st.fg(theme::pal().muted).remove_modifier(Modifier::BOLD) };
                // menus: a `›` on the right (a submenu opens)
                let mark = if submenu { " ›" } else if items { "" } else { "  " };
                let room_w = (row.width as usize).saturating_sub(2 + mark.width());
                // the label first (a path keeps its end); the detail gets the rest, cut at its end
                let text = tail(&text, room_w);
                let left = room_w - text.width();
                let detail = if detail.is_empty() || left < 6 { String::new() } else { head(&detail, left - 2) };
                let pad = left.saturating_sub(if detail.is_empty() { 0 } else { detail.width() + 2 });
                let mut spans = vec![Span::styled(format!(" {text}"), st)];
                // skills: the description right after the name; the others: at the right edge
                if matches!(level, Level::Items(Kind::Skills)) {
                    if !detail.is_empty() {
                        spans.push(Span::styled(format!("  {detail}"), dim));
                    }
                    spans.push(Span::styled(" ".repeat(pad), st));
                } else {
                    spans.push(Span::styled(" ".repeat(pad), st));
                    if !detail.is_empty() {
                        spans.push(Span::styled(format!("  {detail}"), dim));
                    }
                }
                spans.push(Span::styled(format!("{mark} "), dim));
                f.render_widget(Paragraph::new(Line::from(spans)).style(st), row);
                self.rows.push((row, d, idx));
            }
            parent = Some((area, cur_y));
            // the long text of the current row (a skill's description): a box above the dropdown
            if active
                && items
                && let Some(it) = self.matched.1.get(cursor).and_then(|&i| self.items.get(i))
                && !it.info.is_empty()
                && area.width > 8
            {
                let chars: Vec<char> = it.info.chars().collect();
                let lines: Vec<String> = crate::textarea::wrap(&chars, area.width as usize - 4).into_iter().take(INFO_ROWS).map(|(a, b)| chars[a..b].iter().collect::<String>().trim_end().to_string()).collect();
                let h = lines.len() as u16 + 2;
                let top = stack.map_or(area.y, |s| s.y);
                let spot = if top >= room.y + h {
                    Some(top - h)
                } else if area.bottom() + h <= room.bottom() && stack.is_some_and(|s| s.bottom() <= area.bottom()) {
                    Some(area.bottom())
                } else {
                    None
                };
                if let Some(y) = spot {
                    let r = Rect { y, height: h, ..area };
                    let inner = dropdown::frame(f, r, &it.label, "", false, false, false);
                    f.render_widget(Paragraph::new(lines.into_iter().map(|l| Line::styled(format!(" {l}"), theme::text())).collect::<Vec<_>>()), inner);
                    self.areas.push(r);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyModifiers;

    fn key(c: &mut Command, ta: &mut TextArea, code: KeyCode) -> bool {
        c.key(KeyEvent::new(code, KeyModifiers::NONE), ta)
    }

    fn item(label: &str, time: u64) -> Item {
        Item { label: label.into(), insert: label.into(), time, ..Default::default() }
    }

    fn screen(c: &mut Command, ta: &mut TextArea, w: u16, h: u16, input: Rect) -> String {
        let backend = ratatui::backend::TestBackend::new(w, h);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| {
            ta.render(f, input, true);
            c.render(f, ta, f.area(), input);
        })
        .unwrap();
        let buf = term.backend().buffer().clone();
        (0..h).map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn fuzzy_like_a_file_picker() {
        let items = vec![item("src/app.rs", 5), item("src/textarea.rs", 2), item("docs/agent-ascii-art.md", 3), item("src/theme.rs", 4)];
        // 0 text: by time, the latest last
        assert_eq!(matches(&items, ""), [0, 1, 2, 3]);
        // chars in order, across the path: the best match last
        let m = matches(&items, "txa");
        assert_eq!(m.last().map(|&i| items[i].label.as_str()), Some("src/textarea.rs"));
        let m = matches(&items, "srcth");
        assert_eq!(m.iter().map(|&i| items[i].label.as_str()).collect::<Vec<_>>(), ["src/theme.rs"]);
        // a match in the file name beats a match in the dirs
        let items = vec![item("app/x.rs", 9), item("src/app.rs", 1)];
        assert_eq!(matches(&items, "app").last(), Some(&1));
        assert!(matches(&items, "zzz").is_empty());
    }

    #[test]
    fn levels_open_filter_insert_and_go_back() {
        let mut ta = TextArea::new(false, "");
        let mut c = Command::default();
        ta.set_text("see >");
        ta.move_end();
        assert_eq!(c.open_level(&ta), Some(0));
        // typed text filters the 1st level; enter opens the kinds, the text after `>` goes
        ta.insert("me");
        assert!(key(&mut c, &mut ta, KeyCode::Enter));
        assert_eq!((c.open_level(&ta), ta.text().as_str()), (Some(1), "see >"));
        // esc: 1 level back
        assert!(key(&mut c, &mut ta, KeyCode::Esc));
        assert_eq!(c.open_level(&ta), Some(0));
        assert!(key(&mut c, &mut ta, KeyCode::Enter));
        assert_eq!(c.menu(Level::Kinds, "thr"), [2]);
        // the items come from disk, replaced here
        c.set_test_items(Kind::Files, vec![item("src/app.rs", 1), item("src/view.rs", 2), item("README.md", 3)]);
        // the latest (last) row is current
        c.settle("");
        assert_eq!(c.cursor[2], 2);
        assert!(key(&mut c, &mut ta, KeyCode::Up));
        assert_eq!(c.cursor[2], 1);
        // typed text: fuzzy, the best match current
        for ch in "app".chars() {
            assert!(!key(&mut c, &mut ta, KeyCode::Char(ch)));
            ta.insert(&ch.to_string());
        }
        assert!(key(&mut c, &mut ta, KeyCode::Enter));
        assert_eq!(ta.text(), "see src/app.rs ");
        assert_eq!(c.open_level(&ta), None);
        // esc on the 1st level: closed for this `>` only
        ta.set_text(">");
        ta.move_end();
        assert!(key(&mut c, &mut ta, KeyCode::Esc));
        assert_eq!(c.open_level(&ta), None);
        assert!(!key(&mut c, &mut ta, KeyCode::Enter));
        ta.insert(" >");
        assert_eq!(c.open_level(&ta), Some(0));
        // `> ` (a quote), `->`, 0 matching commands: 0 dropdown, keys pass
        ta.set_text("> ");
        ta.move_end();
        assert_eq!(c.open_level(&ta), None);
        ta.set_text("a->");
        ta.move_end();
        assert_eq!(c.open_level(&ta), None);
        ta.set_text(">zz");
        ta.move_end();
        assert!(!key(&mut c, &mut ta, KeyCode::Enter));
    }

    #[test]
    fn backspace_on_an_empty_filter_goes_back() {
        let mut ta = TextArea::new(false, "");
        let mut c = Command::default();
        ta.set_text(">");
        ta.move_end();
        assert!(key(&mut c, &mut ta, KeyCode::Enter));
        assert_eq!(c.open_level(&ta), Some(1));
        assert!(key(&mut c, &mut ta, KeyCode::Backspace));
        assert_eq!((c.open_level(&ta), ta.text().as_str()), (Some(0), ">"));
        // 1st level: backspace is the input's (removes the `>`)
        assert!(!key(&mut c, &mut ta, KeyCode::Backspace));
    }

    #[test]
    fn branch_only_with_a_thread_and_skills_insert_their_name() {
        let mut ta = TextArea::new(false, "");
        let mut c = Command::default();
        // 0 thread (task screen): 0 branch
        assert_eq!(c.menu(Level::Commands, ""), [MENTION, SKILLS]);
        c.can_branch = true;
        assert_eq!(c.menu(Level::Commands, ""), [MENTION, BRANCH, SKILLS, HANDOFF, RELOAD, FLOW, SUB]);
        // `>su` enter: `>sub ` stays, 0 action (the send starts the sub-thread)
        ta.set_text(">su");
        ta.move_end();
        assert!(key(&mut c, &mut ta, KeyCode::Enter));
        assert_eq!((ta.text().as_str(), c.action.take()), (">sub ", None));
        // `>rel` enter: the reload action
        ta.set_text(">rel");
        ta.move_end();
        assert!(key(&mut c, &mut ta, KeyCode::Enter));
        assert_eq!((ta.text().as_str(), c.action.take()), ("", Some(Action::Reload)));
        ta.set_text(">flo");
        ta.move_end();
        assert!(key(&mut c, &mut ta, KeyCode::Enter));
        assert_eq!((ta.text().as_str(), c.action.take()), ("", Some(Action::Flow)));
        // `>hand` enter: the handoff action
        ta.set_text(">hand");
        ta.move_end();
        assert!(key(&mut c, &mut ta, KeyCode::Enter));
        assert_eq!((ta.text().as_str(), c.action.take()), ("", Some(Action::Handoff)));
        // `>br` enter: the text goes, the host gets the action
        ta.set_text("x >br");
        ta.move_end();
        assert!(key(&mut c, &mut ta, KeyCode::Enter));
        assert_eq!((ta.text().as_str(), c.action.take()), ("x ", Some(Action::Branch)));
        // skills: 1 level below the commands, top-down, the 1st row current
        ta.set_text(">");
        ta.move_end();
        assert_eq!(c.open_level(&ta), Some(0));
        let skill = |n: &str, d: &str, i: u64| Item { label: n.into(), detail: d.into(), insert: format!("/{n}"), time: i, info: d.into() };
        c.set_test_items(Kind::Skills, vec![skill("antislop", "Detect and fix AI writing patterns in text.", 0), skill("tmux", "Use the tmux CLI to manage sessions.", 1), skill("xlsx", "Spreadsheet files.", 2)]);
        assert_eq!((c.settle(""), c.cursor[1]), (3, 0));
        let s = screen(&mut c, &mut ta, 100, 24, Rect::new(0, 18, 100, 4));
        // name + description in the row, the full description of the current row in a box above
        assert!(s.contains("┌ skills ") && s.contains("tmux  Use the tmux CLI to manage sessions."), "{s}");
        assert!(s.contains("┌ antislop ") && s.contains("│ Detect and fix AI writing patterns in text."), "{s}");
        // typed text: the name first, a match in the description too; the best on top
        ta.insert("spread");
        assert_eq!(c.settle("spread"), 1);
        assert!(key(&mut c, &mut ta, KeyCode::Enter));
        assert_eq!(ta.text(), "/xlsx ");
    }

    #[test]
    fn files_notes_artifacts_from_disk() {
        let dir = std::env::temp_dir().join(format!("codehead-cmd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::create_dir_all(dir.join("node_modules")).unwrap();
        std::fs::write(dir.join("sub/a.md"), "a").unwrap();
        std::fs::write(dir.join("b.txt"), "b").unwrap();
        std::fs::write(dir.join("node_modules/c.js"), "c").unwrap();
        // 0 git: the files below the dir, relative
        let mut f: Vec<String> = files(&dir).into_iter().map(|i| i.insert).collect();
        f.sort();
        assert_eq!(f, ["b.txt", "sub/a.md"]);
        // notes: .md files, shown relative, inserted as the full path
        let n = notes(&dir);
        assert_eq!(n.len(), 1);
        assert_eq!((n[0].label.as_str(), n[0].insert.as_str()), ("sub/a.md", dir.join("sub/a.md").display().to_string().as_str()));
        // artifacts: urls in transcripts, 1 row per url, 0 share key
        std::fs::create_dir_all(dir.join("-Users-me-app")).unwrap();
        std::fs::write(
            dir.join("-Users-me-app/s.jsonl"),
            "{\"t\":\"see https://claude.ai/artifact/DsbWsZWkLy5uQJbKuyXwue?sk=abc and https://claude.ai/code/artifact/683e1f81-0b14-4581-9bd1-d1f9a90b6eb5\"}\n{\"t\":\"https://claude.ai/artifact/DsbWsZWkLy5uQJbKuyXwue\"}",
        )
        .unwrap();
        let mut a: Vec<String> = scan_artifacts(&dir).into_iter().map(|i| i.insert).collect();
        a.sort();
        assert_eq!(a, ["https://claude.ai/artifact/DsbWsZWkLy5uQJbKuyXwue", "https://claude.ai/code/artifact/683e1f81-0b14-4581-9bd1-d1f9a90b6eb5"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn artifact_labels_use_titles_and_insert_urls() {
        let first = "https://claude.ai/artifact/first12345";
        let second = "https://claude.ai/code/artifact/second12345";
        let third = "https://claude.ai/artifact/third12345";
        let mut titles = HashMap::new();
        let nested = serde_json::json!({"message": {"content": [{"text": serde_json::json!({
            "description": format!("[Search field design]({first}?sk=secret) [https://claude.ai/artifact/third12345]({third})")
        }).to_string()}, {"title": "Release dashboard", "url": second}]}});
        transcript_titles(&nested, &mut titles, 0);
        assert_eq!(titles.get(first).map(String::as_str), Some("Search field design"));
        assert_eq!(titles.get(second).map(String::as_str), Some("Release dashboard"));
        assert!(!titles.contains_key(third));
        let mut c = Command::default();
        let mut ta = TextArea::new(false, "");
        ta.set_text(">");
        ta.move_end();
        key(&mut c, &mut ta, KeyCode::Enter);
        c.set_test_items(Kind::Artifacts, vec![Item { label: titles[first].clone(), insert: first.into(), ..Default::default() }]);
        let screen = screen(&mut c, &mut ta, 100, 24, Rect::new(0, 18, 100, 4));
        assert!(screen.contains("Search field design"), "{screen}");
        assert!(!screen.contains("claude.ai/"), "{screen}");
        assert!(key(&mut c, &mut ta, KeyCode::Enter));
        assert_eq!(ta.text(), format!("{first} "));
    }

    #[test]
    fn artifact_page_titles_skip_generic_or_blocked_pages() {
        assert_eq!(page_title("<title>Search &amp; filters - Claude</title>"), Some("Search & filters".into()));
        assert_eq!(page_title("<title>Claude Artifact</title>"), None);
        assert_eq!(page_title("<title>Just a moment...</title>"), None);
    }

    #[test]
    fn dropdowns_stay_above_the_prompt_box() {
        let mut ta = TextArea::new(false, "");
        let mut c = Command::default();
        // a prompt of 3 rows, the `>` on its last row
        ta.set_text("line 1\nline 2\nx >");
        ta.move_end();
        key(&mut c, &mut ta, KeyCode::Enter);
        c.set_test_items(Kind::Files, (0..30).map(|i| item(&format!("src/file-{i:02}.rs"), i)).collect());
        let input = Rect::new(0, 20, 100, 5);
        let all = screen(&mut c, &mut ta, 100, 30, input);
        for label in ["┌ commands ", "┌ mention ", "┌ mention › files "] {
            assert!(all.contains(label), "{label}:\n{all}");
        }
        // 30 files, 10 in view: the latest at the bottom, ▲ (rows above), 0 ▼
        assert!(all.contains("file-29") && all.contains("file-20") && !all.contains("file-19"), "{all}");
        assert!(all.contains('▲') && !all.contains('▼'), "{all}");
        // 3 dropdowns, left to right, all above the prompt box (its 3 rows stay in view)
        assert_eq!(c.areas.len(), 3);
        assert!(c.areas[0].right() <= c.areas[1].x && c.areas[1].right() <= c.areas[2].x, "{:?}", c.areas);
        assert!(c.areas.iter().all(|r| r.bottom() <= input.y), "{:?}", c.areas);
        assert!(all.contains("line 1") && all.contains("line 2") && all.contains("x >"), "{all}");
        // narrow screen (0 room beside): the files go above the 2 menus, not over them or the prompt
        let input = Rect::new(0, 20, 60, 5);
        let all = screen(&mut c, &mut ta, 60, 30, input);
        let [a, b, files] = c.areas[..] else { panic!("{:?}", c.areas) };
        assert!(files.bottom() <= a.y.min(b.y) && files.right() <= 60, "{:?}", c.areas);
        assert!(!files.intersects(a) && !files.intersects(b) && b.bottom() <= input.y, "{:?}", c.areas);
        assert!(all.contains("┌ commands ") && all.contains("┌ mention ") && all.contains("x >"), "{all}");
        // 0 room above the prompt box: below it
        let input = Rect::new(0, 1, 100, 3);
        ta.set_text(">");
        ta.move_end();
        screen(&mut c, &mut ta, 100, 30, input);
        assert!(c.areas[0].y >= input.bottom(), "{:?}", c.areas);
    }
}
