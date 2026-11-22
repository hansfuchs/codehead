//! Completions in shell mode (`!` in the prompt), like an IDE or fish:
//!
//! - project tasks: recipes of the nearest justfile, scripts of the nearest package.json (run with
//!   the package manager of its lockfile), targets of the nearest Makefile. As the 1st word
//!   (`bu` → `just build`), and after their runner (`just `, `pnpm `, `npm run `, `make `).
//! - commands and their arguments: fish's own completions (`fish --no-config -c 'complete -C …'`,
//!   in the background, descriptions included), commands ranked by their use in the history.
//!   0 fish on PATH: the commands on PATH (1st word only).
//! - a suggestion from the history after the cursor (gray, fish's autosuggestion): → / End /
//!   ctrl+f take it, alt+→ its next word.
//!
//! The dropdown opens by itself once a word is typed (after a runner: at once), tab opens it for
//! any word. ↑↓ move (the row's text into the prompt at once), tab takes the row, enter only after
//! ↑↓ (else enter runs the command). esc: closed for this word. A word with a `/`: the path
//! dropdown's.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use ratatui::text::Span;

use crate::dropdown;
use crate::textarea::{Pos, TextArea};
use crate::theme;

/// Project tasks are read again after this time.
const FRESH: Duration = Duration::from_secs(3);
/// The longest a fish completion runs.
const FISH_TIMEOUT: Duration = Duration::from_millis(1500);
/// Most rows kept per source.
const MAX: usize = 200;

/// A task of the project: its runner (`just`, `pnpm`, `make`), name, command line, doc.
#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub runner: &'static str,
    pub name: String,
    pub run: String,
    pub doc: String,
}

/// A row of the dropdown: TEXT replaces the word (a trailing space: the word is done), the
/// detail right of it (muted).
#[derive(Clone, Debug, PartialEq)]
struct Entry {
    text: String,
    detail: String,
}

/// The word at the cursor in shell mode.
#[derive(Debug)]
struct Ctx {
    /// Start of the word (what a pick replaces).
    at: Pos,
    /// The command line from its start (after `!`) to the cursor, the word as typed.
    line: String,
    word: String,
    /// The words of the last command (after `|`, `&&`, `;`) before the word.
    before: Vec<String>,
}

enum Req {
    Fish(String, PathBuf),
    History,
    Commands,
}

enum Res {
    Fish(String, Vec<(String, String)>),
    History(Arc<Vec<String>>),
    Commands(Arc<Vec<String>>),
}

pub struct ShellComp {
    fish: bool,
    tx: Option<Sender<Req>>,
    res_tx: Sender<Res>,
    rx: Receiver<Res>,
    /// fish's rows per command line (the last few).
    fish_rows: HashMap<String, Vec<(String, String)>>,
    asked: String,
    /// The history (newest first) and the commands on PATH, None until loaded.
    pub history: Option<Arc<Vec<String>>>,
    asked_history: bool,
    commands: Option<Arc<Vec<String>>>,
    asked_commands: bool,
    tasks: Option<(PathBuf, Instant, Vec<Task>)>,
    /// Per (the thread's `!commands`, history loaded): the uses of each command, and the last
    /// suggestion (the line, the rest).
    uses: Option<((usize, bool), Arc<HashMap<String, usize>>)>,
    sugg: Option<((usize, bool), String, Option<String>)>,
    /// esc on the word at this position: closed until another word; tab: open for that word.
    closed: Option<Pos>,
    forced: Option<Pos>,
    cursor: usize,
    offset: usize,
    seen: String,
    /// tab made the dropdown active: ↑↓ tab move in it (passive: ↑↓ stay the history's).
    active: bool,
    preview: dropdown::Preview,
    rows: Vec<(Rect, usize)>,
    area: Rect,
}

impl Default for ShellComp {
    fn default() -> Self {
        let (res_tx, rx) = channel();
        ShellComp {
            fish: crate::engine::harnesses::on_path("fish"),
            tx: None,
            res_tx,
            rx,
            fish_rows: HashMap::new(),
            asked: String::new(),
            history: None,
            asked_history: false,
            commands: None,
            asked_commands: false,
            tasks: None,
            uses: None,
            sugg: None,
            closed: None,
            forced: None,
            cursor: 0,
            offset: 0,
            seen: String::new(),
            active: false,
            preview: dropdown::Preview::default(),
            rows: Vec::new(),
            area: Rect::default(),
        }
    }
}

/// The context of the cursor in TA: shell mode, 1 line, 0 selection, the cursor at a word's end.
fn context(ta: &TextArea) -> Option<Ctx> {
    let text = ta.text();
    if text.contains('\n') || ta.selected_text().is_some() || ta.cursor().0 != 0 {
        return None;
    }
    context_of(&text, ta.cursor().1)
}

/// [`context`] of TEXT (1 line), the cursor at COL.
fn context_of(text: &str, col: usize) -> Option<Ctx> {
    let chars: Vec<char> = text.chars().collect();
    if !text.starts_with('!') || col == 0 || chars.get(col).is_some_and(|c| !c.is_whitespace()) {
        return None;
    }
    let mut start = col;
    while start > 1 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }
    let word: String = chars[start..col].iter().collect();
    if word.contains('/') || word.contains('"') || word.contains('\'') {
        return None;
    }
    let line: String = chars[1..col].iter().collect::<String>().trim_start().to_string();
    let head = &line[..line.len() - word.len()];
    let mut before: Vec<String> = Vec::new();
    for w in head.split_whitespace() {
        if matches!(w, "|" | "||" | "&&" | ";") {
            before.clear();
        } else {
            before.push(w.to_string());
        }
    }
    Some(Ctx { at: Pos { row: 0, col: start }, line, word, before })
}

/// The runner whose tasks go after BEFORE (the words before the cursor's word), None: 0 runner.
fn runner_of(before: &[String]) -> Option<&'static str> {
    let w: Vec<&str> = before.iter().map(String::as_str).collect();
    match w.as_slice() {
        ["just", ..] => Some("just"),
        ["make", ..] => Some("make"),
        ["npm", "run" | "run-script"] | ["pnpm"] | ["pnpm", "run"] | ["yarn"] | ["yarn", "run"] | ["bun", "run"] => Some("package"),
        _ => None,
    }
}

/// TEXT done: a space after it, unless it goes on (`dir/`, `--flag=`).
fn done(text: &str) -> String {
    if text.ends_with('/') || text.ends_with('=') { text.to_string() } else { format!("{text} ") }
}

impl ShellComp {
    /// Send REQ to the worker (tests: run in place).
    fn ask(&mut self, req: Req) {
        if cfg!(test) {
            let _ = self.res_tx.send(work(req));
            return;
        }
        let tx = self.tx.get_or_insert_with(|| {
            let (tx, rx) = channel::<Req>();
            let out = self.res_tx.clone();
            std::thread::spawn(move || {
                while let Ok(mut req) = rx.recv() {
                    // fish: only the newest line counts (typed on meanwhile)
                    let mut later = Vec::new();
                    while let Ok(r) = rx.try_recv() {
                        match (r, &req) {
                            (r @ Req::Fish(..), Req::Fish(..)) => req = r,
                            (r, _) => later.push(r),
                        }
                    }
                    for r in std::iter::once(req).chain(later) {
                        if out.send(work(r)).is_err() {
                            return;
                        }
                    }
                }
            });
            tx
        });
        let _ = tx.send(req);
    }

    /// Results in. True: a redraw is due.
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok(r) = self.rx.try_recv() {
            match r {
                Res::Fish(line, rows) => {
                    if self.fish_rows.len() > 32 {
                        self.fish_rows.clear();
                    }
                    self.fish_rows.insert(line, rows);
                }
                Res::History(h) => self.history = Some(h),
                Res::Commands(c) => self.commands = Some(c),
            }
            any = true;
        }
        any
    }

    /// The project's tasks for DIR (shell mode's dir), read again after FRESH.
    pub fn tasks(&mut self, dir: &Path) -> Vec<Task> {
        match &self.tasks {
            Some((d, at, t)) if d == dir && at.elapsed() < FRESH => t.clone(),
            _ => {
                let t = tasks(dir);
                self.tasks = Some((dir.to_path_buf(), Instant::now(), t.clone()));
                t
            }
        }
    }

    /// The history, loaded in the background on the 1st call.
    fn history(&mut self) -> Arc<Vec<String>> {
        if self.history.is_none() && !self.asked_history {
            self.asked_history = true;
            self.ask(Req::History);
            self.poll();
        }
        self.history.clone().unwrap_or_default()
    }

    /// Times each command starts a line of THREAD (the thread's `!commands`) and the history.
    fn uses(&mut self, thread: &[String]) -> Arc<HashMap<String, usize>> {
        let hist = self.history();
        let key = (thread.len(), self.history.is_some());
        if let Some((k, n)) = &self.uses
            && *k == key
        {
            return n.clone();
        }
        let mut n: HashMap<String, usize> = HashMap::new();
        for c in thread.iter().chain(hist.iter()) {
            if let Some(w) = c.split_whitespace().next() {
                *n.entry(w.to_string()).or_default() += 1;
            }
        }
        let n = Arc::new(n);
        self.uses = Some((key, n.clone()));
        n
    }

    /// The rows for CTX. None: nothing to offer.
    fn entries(&mut self, ctx: &Ctx, dir: &Path, thread: &[String]) -> Vec<Entry> {
        let word = ctx.word.as_str();
        let lower = word.to_lowercase();
        let mut out: Vec<Entry> = Vec::new();
        let tasks = self.tasks(dir);
        let first = ctx.before.is_empty();
        let runner = runner_of(&ctx.before);
        // project tasks: by prefix, then those that hold the word
        let ranked = |hit: &dyn Fn(&Task) -> (bool, bool)| -> Vec<&Task> {
            let mut a: Vec<&Task> = tasks.iter().filter(|t| hit(t).0).collect();
            a.extend(tasks.iter().filter(|t| !hit(t).0 && hit(t).1));
            a
        };
        if first {
            let hit = |t: &Task| (t.name.to_lowercase().starts_with(&lower) || t.run.starts_with(word), t.name.to_lowercase().contains(&lower));
            out.extend(ranked(&hit).into_iter().map(|t| Entry { text: done(&t.run), detail: t.doc.clone() }));
        } else if let Some(r) = runner {
            let hit = |t: &Task| {
                let own = if r == "package" { !matches!(t.runner, "just" | "make") } else { t.runner == r };
                (own && t.name.to_lowercase().starts_with(&lower), own && t.name.to_lowercase().contains(&lower))
            };
            out.extend(ranked(&hit).into_iter().map(|t| Entry { text: done(&t.name), detail: t.doc.clone() }));
        }
        // commands and arguments: fish (the 1st word: ≥ 1 char)
        if !(first && word.is_empty()) && !matches!(runner, Some("just" | "make")) {
            let mut rows: Vec<(String, String)> = Vec::new();
            if self.fish {
                if let Some(r) = self.fish_rows.get(&ctx.line) {
                    rows = r.clone();
                } else {
                    if self.asked != ctx.line {
                        self.asked = ctx.line.clone();
                        self.ask(Req::Fish(ctx.line.clone(), dir.to_path_buf()));
                        self.poll();
                    }
                    rows = self.fish_rows.get(&ctx.line).cloned().unwrap_or_else(|| self.stale(ctx));
                }
            } else if first {
                if self.commands.is_none() && !self.asked_commands {
                    self.asked_commands = true;
                    self.ask(Req::Commands);
                    self.poll();
                }
                rows = self.commands.iter().flat_map(|c| c.iter()).filter(|c| c.starts_with(word)).take(MAX).map(|c| (c.clone(), String::new())).collect();
            }
            if first {
                let uses = self.uses(thread);
                // used commands first (most used first), then fish's order
                let mut idx: Vec<usize> = (0..rows.len()).collect();
                idx.sort_by_key(|&i| std::cmp::Reverse(uses.get(&rows[i].0).copied().unwrap_or(0)));
                rows = idx.into_iter().map(|i| rows[i].clone()).collect();
            }
            for (text, detail) in rows {
                let text = done(&text);
                if !out.iter().any(|e| e.text == text) {
                    out.push(Entry { text, detail });
                }
            }
        }
        out.truncate(MAX);
        out
    }

    /// fish's rows of an earlier line this one extends in the same word (0 flicker while fish runs).
    fn stale(&self, ctx: &Ctx) -> Vec<(String, String)> {
        let head = &ctx.line[..ctx.line.len() - ctx.word.len()];
        self.fish_rows
            .iter()
            .filter(|(l, _)| l.starts_with(head) && ctx.line.starts_with(l.as_str()) && !l[head.len()..].contains(' '))
            .max_by_key(|(l, _)| l.len())
            .map(|(_, r)| r.iter().filter(|(t, _)| t.starts_with(&ctx.word)).cloned().collect())
            .unwrap_or_default()
    }

    /// The open dropdown: the context (the typed word: the filter while a preview is in the
    /// text) and its rows. None: closed.
    fn open(&mut self, ta: &TextArea, dir: &Path, thread: &[String]) -> Option<(Ctx, Vec<Entry>)> {
        // a live preview in the text: the word as typed before it (the filter)
        let ctx = match self.preview.typed(ta) {
            Some((p, typed)) if p.row == 0 && context(ta).is_some() => {
                let head: String = ta.text().chars().take(p.col).collect();
                let text = format!("{head}{typed}");
                context_of(&text, text.chars().count())
            }
            _ => context(ta),
        }?;
        if self.closed.is_some_and(|p| p != ctx.at) {
            self.closed = None;
        }
        if self.forced.is_some_and(|p| p != ctx.at) {
            self.forced = None;
        }
        if self.closed == Some(ctx.at) {
            return None;
        }
        let forced = self.forced == Some(ctx.at);
        let runner = runner_of(&ctx.before).is_some();
        if ctx.word.is_empty() && !forced && !runner {
            return None;
        }
        let entries = self.entries(&ctx, dir, thread);
        // 0 rows, or the word is the 1 row already
        if entries.is_empty() || (entries.len() == 1 && entries[0].text.trim_end() == ctx.word) {
            return None;
        }
        if self.seen != ctx.line {
            self.seen = ctx.line.clone();
            self.cursor = 0;
            self.offset = 0;
            self.active = false;
        }
        self.cursor = self.cursor.min(entries.len() - 1);
        Some((ctx, entries))
    }

    #[cfg(test)]
    fn texts(&mut self, ta: &TextArea, dir: &Path) -> Vec<String> {
        self.open(ta, dir, &[]).map(|(_, e)| e.into_iter().map(|e| e.text).collect()).unwrap_or_default()
    }

    fn pick(&mut self, ta: &mut TextArea, ctx: &Ctx, e: &Entry) {
        ta.replace_to_cursor(ctx.at, &e.text);
        self.preview.clear();
        self.forced = None;
    }

    /// Key K (shell mode's prompt TA, DIR its dir, THREAD the thread's `!commands`). True: used.
    pub fn key(&mut self, k: KeyEvent, ta: &mut TextArea, dir: &Path, thread: &[String]) -> bool {
        if !k.modifiers.is_empty() && k.modifiers != KeyModifiers::SHIFT {
            return false;
        }
        let open = match self.open(ta, dir, thread) {
            Some(o) => Some(o),
            // tab: open for this word
            None if k.code == KeyCode::Tab => {
                let Some(ctx) = context(ta) else { return false };
                self.closed = None;
                self.forced = Some(ctx.at);
                match self.open(ta, dir, thread) {
                    Some(o) => Some(o),
                    // fish still at work: taken, its rows show when in (0 rows: the key goes on)
                    None => return !(ctx.before.is_empty() && ctx.word.is_empty()) && self.fish,
                }
            }
            None => None,
        };
        let Some((ctx, entries)) = open else { return false };
        let n = entries.len();
        let show = |me: &mut Self, ta: &mut TextArea| {
            let text = entries[me.cursor].text.trim_end().to_string();
            me.preview.show(ta, ctx.at, &ctx.word, &text);
        };
        match (k.code, self.active) {
            // passive: tab takes the 1 row, else makes the dropdown active on its 1st row
            (KeyCode::Tab, false) if n == 1 => self.pick(ta, &ctx, &entries[0]),
            (KeyCode::Tab, false) => {
                self.active = true;
                self.cursor = 0;
                show(self, ta);
            }
            (KeyCode::Up | KeyCode::BackTab, true) => {
                self.cursor = (self.cursor + n - 1) % n;
                show(self, ta);
            }
            (KeyCode::Down | KeyCode::Tab, true) => {
                self.cursor = (self.cursor + 1) % n;
                show(self, ta);
            }
            (KeyCode::Enter, true) => self.pick(ta, &ctx, &entries[self.cursor]),
            (KeyCode::Esc, _) => {
                self.preview.revert(ta);
                self.closed = Some(ctx.at);
            }
            // a name char: the typed word again (the filter), the char after it
            (KeyCode::Char(c), true) if !c.is_whitespace() && self.preview.revert(ta) => return false,
            _ => return false,
        }
        true
    }

    /// Click on a row: taken. Wheel: the cursor. True: used.
    pub fn mouse(&mut self, m: MouseEvent, ta: &mut TextArea, dir: &Path, thread: &[String]) -> bool {
        let p = Position { x: m.column, y: m.row };
        if !self.area.contains(p) {
            return false;
        }
        let Some((ctx, entries)) = self.open(ta, dir, thread) else { return false };
        match m.kind {
            MouseEventKind::ScrollUp => self.cursor = self.cursor.saturating_sub(3),
            MouseEventKind::ScrollDown => self.cursor = (self.cursor + 3).min(entries.len() - 1),
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(&(_, i)) = self.rows.iter().find(|(r, _)| r.contains(p)) {
                    let e = entries[i].clone();
                    self.pick(ta, &ctx, &e);
                }
            }
            _ => {}
        }
        true
    }

    /// Not drawn this frame: 0 rows for clicks.
    pub fn hide(&mut self) {
        self.rows.clear();
        self.area = Rect::default();
    }

    /// The dropdown above AVOID (the prompt box), at the word's column, inside BOUNDS.
    pub fn render(&mut self, f: &mut Frame, ta: &TextArea, dir: &Path, thread: &[String], bounds: Rect, avoid: Rect) {
        self.hide();
        let Some((ctx, entries)) = self.open(ta, dir, thread) else { return };
        let Some(p) = ta.screen_pos(ctx.at) else { return };
        let label = match runner_of(&ctx.before) {
            Some("package") => "scripts",
            Some(r) => r,
            None if ctx.before.is_empty() => "commands",
            None => ctx.before.first().map_or("complete", String::as_str),
        };
        let w = entries.iter().map(|e| e.text.chars().count() + e.detail.chars().count().min(40) + 8).max().unwrap_or(0).clamp(28, 90) as u16;
        let Some(area) = dropdown::place(Rect { x: p.x, y: avoid.y, width: 1, height: 1 }, w, entries.len(), bounds, true) else { return };
        let n = ctx.word.chars().count();
        let rows: Vec<dropdown::Row> = entries
            .iter()
            .map(|e| {
                let text = e.text.trim_end().to_string();
                let marks = if n > 0 && text.to_lowercase().starts_with(&ctx.word.to_lowercase()) { (0..n).collect() } else { Vec::new() };
                dropdown::Row { text, detail: e.detail.clone(), marks, ..Default::default() }
            })
            .collect();
        let note = if self.active { "↑↓ tab · enter take · esc" } else if entries.len() == 1 { "tab take" } else { "tab pick" };
        let (off, rows) = dropdown::list_in(f, area, label, note, &rows, self.cursor, self.offset, "", self.active);
        self.offset = off;
        self.rows = rows;
        self.area = area;
    }

    /// The history's suggestion for TA (the cursor at the text's end): the rest of the newest
    /// command (THREAD first, then the shell's history) that starts with the typed line.
    pub fn suggestion(&mut self, ta: &TextArea, thread: &[String]) -> Option<String> {
        let text = ta.text();
        let line = text.strip_prefix('!')?.trim_start();
        let (row, col) = ta.cursor();
        if line.is_empty() || text.contains('\n') || row != 0 || col != text.chars().count() {
            return None;
        }
        let hist = self.history();
        let key = (thread.len(), self.history.is_some());
        if let Some((k, l, rest)) = &self.sugg
            && *k == key
            && l == line
        {
            return rest.clone();
        }
        let rest = thread.iter().rev().chain(hist.iter()).find(|c| c.len() > line.len() && c.starts_with(line) && !c.contains('\n')).map(|c| c[line.len()..].to_string());
        self.sugg = Some((key, line.to_string(), rest.clone()));
        rest
    }

    /// → / End / ctrl+f: the suggestion into TA; alt+→: its next word. True: used.
    pub fn accept(&mut self, k: KeyEvent, ta: &mut TextArea, thread: &[String]) -> bool {
        let alt = k.modifiers == KeyModifiers::ALT;
        let all = matches!((k.code, k.modifiers), (KeyCode::Right | KeyCode::End, KeyModifiers::NONE)) || (k.code == KeyCode::Char('f') && k.modifiers == KeyModifiers::CONTROL);
        if !(all || alt && k.code == KeyCode::Right) {
            return false;
        }
        let Some(rest) = self.suggestion(ta, thread) else { return false };
        let take = if all {
            rest
        } else {
            // the spaces, then the word after them
            let lead = rest.len() - rest.trim_start().len();
            let end = rest[lead..].find(char::is_whitespace).map_or(rest.len(), |i| lead + i);
            rest[..end].to_string()
        };
        ta.insert(&take);
        true
    }

    /// The suggestion right of the text's end in TA (rendered this frame), muted.
    pub fn render_suggestion(&mut self, f: &mut Frame, ta: &TextArea, thread: &[String], area: Rect) {
        let Some(rest) = self.suggestion(ta, thread) else { return };
        let end = Pos { row: 0, col: ta.text().chars().count() };
        let Some(p) = ta.screen_pos(end) else { return };
        if p.x >= area.right() || !area.contains(Position { x: area.x, y: p.y }) {
            return;
        }
        let room = (area.right() - p.x) as usize;
        let shown: String = rest.chars().take(room).collect();
        f.render_widget(ratatui::widgets::Paragraph::new(Span::styled(shown, theme::muted())), Rect { x: p.x, y: p.y, width: room as u16, height: 1 });
    }
}

/// The worker's job.
fn work(req: Req) -> Res {
    match req {
        Req::Fish(line, dir) => {
            let rows = fish_complete(&line, &dir);
            Res::Fish(line, rows)
        }
        Req::History => Res::History(Arc::new(crate::histsearch::shell_history())),
        Req::Commands => Res::Commands(Arc::new(path_commands())),
    }
}

/// fish's completions for LINE in DIR: (token, description). 0 fish, a timeout: 0 rows.
fn fish_complete(line: &str, dir: &Path) -> Vec<(String, String)> {
    use std::io::Read;
    let child = std::process::Command::new("fish")
        .args(["--no-config", "-c", "complete -C -- $argv[1]", line])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return Vec::new() };
    let Some(mut out) = child.stdout.take() else { return Vec::new() };
    let read = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() > FISH_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Vec::new();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(_) => return Vec::new(),
        }
    }
    let text = read.join().unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    text.lines()
        .filter_map(|l| {
            let (t, d) = l.split_once('\t').unwrap_or((l, ""));
            (!t.is_empty() && seen.insert(t.to_string())).then(|| (t.to_string(), d.trim().to_string()))
        })
        .take(MAX)
        .collect()
}

/// The executables on PATH, by name, once each.
fn path_commands() -> Vec<String> {
    use std::os::unix::fs::PermissionsExt;
    let mut v: Vec<String> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
        .filter(|e| e.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v.dedup();
    v
}

/// The tasks of the project around DIR: the nearest justfile, package.json and Makefile from
/// DIR up to the repo top (else home, else the root).
pub fn tasks(dir: &Path) -> Vec<Task> {
    let home = crate::config::home();
    let (mut just, mut pkg, mut make) = (None, None, None);
    for d in dir.ancestors() {
        let find = |names: &[&str]| names.iter().map(|n| d.join(n)).find(|p| p.is_file());
        just = just.or_else(|| find(&["justfile", "Justfile", ".justfile"]));
        pkg = pkg.or_else(|| find(&["package.json"]).map(|p| (p, d.to_path_buf())));
        make = make.or_else(|| find(&["GNUmakefile", "makefile", "Makefile"]));
        if d.join(".git").exists() || d == home {
            break;
        }
    }
    let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_default();
    let mut out = Vec::new();
    if let Some(p) = just {
        out.extend(just_recipes(&read(&p)).into_iter().map(|(name, doc)| Task { runner: "just", run: format!("just {name}"), name, doc }));
    }
    if let Some((p, root)) = pkg {
        let pm = package_manager(&root);
        out.extend(package_scripts(&read(&p)).into_iter().map(|(name, doc)| {
            let run = if matches!(pm, "pnpm" | "yarn") { format!("{pm} {name}") } else { format!("{pm} run {name}") };
            Task { runner: pm, run, name, doc }
        }));
    }
    if let Some(p) = make {
        out.extend(make_targets(&read(&p)).into_iter().map(|(name, doc)| Task { runner: "make", run: format!("make {name}"), name, doc }));
    }
    out
}

/// The package manager of the package in DIR (its lockfile, up to the repo top): pnpm, yarn,
/// bun, else npm.
fn package_manager(dir: &Path) -> &'static str {
    for d in dir.ancestors() {
        for (file, pm) in [("pnpm-lock.yaml", "pnpm"), ("pnpm-workspace.yaml", "pnpm"), ("yarn.lock", "yarn"), ("bun.lock", "bun"), ("bun.lockb", "bun"), ("package-lock.json", "npm")] {
            if d.join(file).is_file() {
                return pm;
            }
        }
        if d.join(".git").exists() {
            break;
        }
    }
    "npm"
}

/// Recipes of a justfile: (name, doc: the comment line above it or `[doc("…")]`). Private ones
/// (`_name`, `[private]`) left out.
fn just_recipes(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let (mut doc, mut private) = (String::new(), false);
    for l in text.lines() {
        if l.starts_with([' ', '\t']) {
            continue;
        }
        let t = l.trim();
        if t.is_empty() {
            (doc, private) = (String::new(), false);
            continue;
        }
        if let Some(c) = t.strip_prefix('#') {
            if !c.starts_with('!') {
                doc = c.trim().to_string();
            }
            continue;
        }
        if let Some(a) = t.strip_prefix('[') {
            private |= a.starts_with("private");
            if let Some(d) = a.strip_prefix("doc(").and_then(|d| d.split('"').nth(1)) {
                doc = d.to_string();
            }
            continue;
        }
        let body = t.strip_prefix('@').unwrap_or(t);
        let name: String = body.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-').collect();
        let rest = &body[name.len()..];
        let keyword = matches!(name.as_str(), "alias" | "set" | "export" | "import" | "mod") && rest.starts_with(' ');
        // `name args: deps`, not `name := value`
        let colon = rest.find(':').filter(|&i| !rest[i + 1..].starts_with('='));
        if !name.is_empty() && !keyword && colon.is_some() && !rest.contains(":=") && !private && !name.starts_with('_') && !out.iter().any(|(n, _)| *n == name) {
            out.push((name, std::mem::take(&mut doc)));
        }
        (doc, private) = (String::new(), false);
    }
    out
}

/// Scripts of a package.json: (name, command).
fn package_scripts(text: &str) -> Vec<(String, String)> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { return Vec::new() };
    v.get("scripts").and_then(|s| s.as_object()).map(|o| o.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string())).collect()).unwrap_or_default()
}

/// Targets of a Makefile: (name, doc: `## doc` after it, else the comment line above it).
/// Special (`.PHONY`), pattern (`%.o`) and variable targets left out.
fn make_targets(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut doc = String::new();
    for l in text.lines() {
        if l.starts_with(['\t', ' ']) {
            continue;
        }
        if let Some(c) = l.strip_prefix('#') {
            doc = c.trim_start_matches('#').trim().to_string();
            continue;
        }
        let Some(i) = l.find(':') else {
            doc.clear();
            continue;
        };
        let (head, rest) = (&l[..i], &l[i + 1..]);
        if rest.starts_with('=') || rest.starts_with(':') && rest[1..].starts_with('=') || head.contains('=') {
            doc.clear();
            continue;
        }
        let own = rest.split_once("##").map(|(_, d)| d.trim().to_string());
        for name in head.split_whitespace() {
            if name.starts_with('.') || name.contains(['%', '$', '(']) || out.iter().any(|(n, _)| n == name) {
                continue;
            }
            out.push((name.to_string(), own.clone().unwrap_or_else(|| doc.clone())));
        }
        doc.clear();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(ta: &mut TextArea, text: &str) {
        ta.set_text(text);
        ta.move_end();
    }

    fn key(c: &mut ShellComp, ta: &mut TextArea, dir: &Path, code: KeyCode) -> bool {
        c.key(KeyEvent::new(code, KeyModifiers::NONE), ta, dir, &[])
    }

    /// A project with a justfile, a package.json (pnpm) and a Makefile; 0 fish (deterministic).
    fn project(name: &str) -> (PathBuf, ShellComp) {
        let dir = std::env::temp_dir().join(format!("codehead-shellcomp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("justfile"), "set shell := [\"bash\", \"-c\"]\nversion := \"1\"\nalias b := build\n\n# build the app\nbuild target='x': deps\n    cargo build\n\n[private]\nhidden:\n    true\n\n_internal:\n    true\n\n[doc(\"run the tests\")]\n@test *args:\n    cargo test\n").unwrap();
        std::fs::write(dir.join("package.json"), r#"{"name":"x","scripts":{"dev":"vite","build:web":"vite build","lint":"eslint ."}}"#).unwrap();
        std::fs::write(dir.join("pnpm-lock.yaml"), "").unwrap();
        std::fs::write(dir.join("Makefile"), ".PHONY: all clean\nCC := cc\n# make it all\nall: main.o\n\tcc\nclean: ## remove the build\n\trm -rf out\n%.o: %.c\n\tcc\n").unwrap();
        let c = ShellComp { fish: false, commands: Some(Arc::new(vec!["bunx".into(), "busybox".into(), "cargo".into()])), history: Some(Arc::new(vec!["cargo test --release".into(), "cargo build".into()])), ..Default::default() };
        (dir, c)
    }

    #[test]
    fn project_files_parse() {
        let (dir, _) = project("parse");
        let t = tasks(&dir.join("sub"));
        let runs: Vec<(&str, &str)> = t.iter().map(|t| (t.run.as_str(), t.doc.as_str())).collect();
        assert_eq!(
            runs,
            [("just build", "build the app"), ("just test", "run the tests"), ("pnpm dev", "vite"), ("pnpm build:web", "vite build"), ("pnpm lint", "eslint ."), ("make all", "make it all"), ("make clean", "remove the build")]
        );
        std::fs::remove_file(dir.join("pnpm-lock.yaml")).unwrap();
        assert_eq!(tasks(&dir).iter().find(|t| t.name == "dev").map(|t| t.run.as_str()), Some("npm run dev"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_first_word_offers_tasks_then_commands_and_tab_takes_one() {
        let (dir, mut c) = project("first");
        let mut ta = TextArea::new(false, "");
        // `! `: 0 dropdown (↑ stays the history's); tab opens the tasks
        typed(&mut ta, "! ");
        assert!(c.texts(&ta, &dir).is_empty());
        assert!(key(&mut c, &mut ta, &dir, KeyCode::Tab));
        assert_eq!(c.texts(&ta, &dir).len(), 7);
        assert_eq!(ta.text(), "! just build", "the 1st row in the text");
        // a word: tasks by name, then the commands on PATH
        typed(&mut ta, "! bu");
        assert_eq!(c.texts(&ta, &dir), ["just build ", "pnpm build:web ", "bunx ", "busybox "]);
        // passive: enter, ↑↓ stay the input's; tab: active on the 1st row, tab / ↓ the next
        assert!(!key(&mut c, &mut ta, &dir, KeyCode::Enter));
        assert!(!key(&mut c, &mut ta, &dir, KeyCode::Down));
        assert!(key(&mut c, &mut ta, &dir, KeyCode::Tab));
        assert_eq!(ta.text(), "! just build");
        assert!(key(&mut c, &mut ta, &dir, KeyCode::Tab));
        assert_eq!(ta.text(), "! pnpm build:web");
        assert_eq!(c.texts(&ta, &dir).len(), 4, "the typed word stays the filter");
        assert!(key(&mut c, &mut ta, &dir, KeyCode::Enter));
        assert_eq!(ta.text(), "! pnpm build:web ");
        // esc: closed for this word
        typed(&mut ta, "! bu");
        assert!(key(&mut c, &mut ta, &dir, KeyCode::Esc));
        assert!(c.texts(&ta, &dir).is_empty());
        assert!(!key(&mut c, &mut ta, &dir, KeyCode::Down));
        // a path word: the path dropdown's; out of shell mode: 0 dropdown
        for t in ["! ./bu", "bu", "! cargo \"bu"] {
            typed(&mut ta, t);
            assert!(c.texts(&ta, &dir).is_empty(), "{t}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_runner_offers_its_tasks_at_once() {
        let (dir, mut c) = project("runner");
        let mut ta = TextArea::new(false, "");
        typed(&mut ta, "! just ");
        assert_eq!(c.texts(&ta, &dir), ["build ", "test "]);
        assert!(key(&mut c, &mut ta, &dir, KeyCode::Tab));
        assert!(key(&mut c, &mut ta, &dir, KeyCode::Enter));
        assert_eq!(ta.text(), "! just build ");
        typed(&mut ta, "! pnpm run b");
        assert_eq!(c.texts(&ta, &dir), ["build:web "]);
        // 1 row: tab takes it
        typed(&mut ta, "! make c");
        assert_eq!(c.texts(&ta, &dir), ["clean "]);
        assert!(key(&mut c, &mut ta, &dir, KeyCode::Tab));
        assert_eq!(ta.text(), "! make clean ");
        // after `&&`: a new command (its 1st word)
        typed(&mut ta, "! make clean && just t");
        assert_eq!(c.texts(&ta, &dir), ["test "]);
        typed(&mut ta, "! cd sub && te");
        assert_eq!(c.texts(&ta, &dir), ["just test "]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_history_suggests_the_rest_and_right_takes_it() {
        let (dir, mut c) = project("suggest");
        let mut ta = TextArea::new(false, "");
        typed(&mut ta, "! cargo t");
        assert_eq!(c.suggestion(&ta, &[]).as_deref(), Some("est --release"));
        // the thread's own commands first (newest first)
        let thread = vec!["cargo tree".to_string()];
        assert_eq!(c.suggestion(&ta, &thread).as_deref(), Some("ree"));
        // alt+→: 1 word; →: the rest
        typed(&mut ta, "! cargo");
        assert!(c.accept(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT), &mut ta, &[]));
        assert_eq!(ta.text(), "! cargo test");
        assert!(c.accept(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), &mut ta, &[]));
        assert_eq!(ta.text(), "! cargo test --release");
        assert!(!c.accept(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), &mut ta, &[]));
        // the cursor not at the end: 0 suggestion
        typed(&mut ta, "! cargo t");
        ta.move_start();
        assert!(c.suggestion(&ta, &[]).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fish_completes_arguments_with_descriptions() {
        if !crate::engine::harnesses::on_path("fish") {
            return;
        }
        let rows = fish_complete("git chec", Path::new("/tmp"));
        assert!(rows.iter().any(|(t, d)| t == "checkout" && !d.is_empty()), "{rows:?}");
        let (dir, mut c) = project("fish");
        c.fish = true;
        let mut ta = TextArea::new(false, "");
        typed(&mut ta, "! git chec");
        assert!(c.texts(&ta, &dir).contains(&"checkout ".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
