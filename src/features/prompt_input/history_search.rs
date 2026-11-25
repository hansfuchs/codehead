//! ctrl+r in shell mode: the command history, searched by fzf in the background (`fzf --filter`:
//! 0 TTY, 0 suspend), the matches in a dropdown above the input, the best one at the bottom
//! (right above the input). The input after `!` is the query; every change runs fzf again (an
//! older run's result is dropped). ↑ (ctrl+r) worse matches, ↓ (ctrl+n) better ones,
//! enter / tab put the match into the input, esc closes. 0 fzf on PATH: the built-in fuzzy filter.
//!
//! The history: the thread's `!commands` (newest first), then the shell's own history file
//! (fish, zsh, bash; newest first), 1 row per command.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use crate::ui::dropdown;

/// Most matches kept.
const MAX: usize = 300;

pub(crate) struct HistSearch {
    open: bool,
    /// The history (thread first), None until loaded.
    all: Option<Arc<Vec<String>>>,
    /// The query of the shown matches.
    query: String,
    matches: Vec<String>,
    cursor: usize,
    offset: usize,
    /// Run counter: a result of an older run is dropped.
    run: u64,
    tx: crate::infra::bus::Tx<(u64, String, Vec<String>)>,
    rx: Receiver<(u64, String, Vec<String>)>,
    /// The pick of the last enter: the new input text.
    pub picked: Option<String>,
    fzf: bool,
}

impl Default for HistSearch {
    fn default() -> Self {
        let (tx, rx) = crate::infra::bus::channel();

        HistSearch {
            open: false,
            all: None,
            query: String::new(),
            matches: Vec::new(),
            cursor: 0,
            offset: 0,
            run: 0,
            tx,
            rx,
            picked: None,
            fzf: crate::engine::harnesses::on_path("fzf"),
        }
    }
}

impl HistSearch {
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    /// Open with the thread's commands COMMANDS (oldest first) and QUERY (the input after `!`).
    pub(crate) fn open(&mut self, commands: Vec<String>, query: &str) {
        self.open = true;
        self.cursor = 0;
        self.offset = 0;
        let mut all: Vec<String> = commands.into_iter().rev().collect();

        all.extend(shell_history());
        let mut seen = std::collections::HashSet::new();

        all.retain(|entry| !entry.trim().is_empty() && seen.insert(entry.clone()));
        self.all = Some(Arc::new(all));
        self.search(query);
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
        self.matches.clear();
        self.all = None;
    }

    /// Run the search for QUERY in the background.
    fn search(&mut self, query: &str) {
        search_hist_search(self, query);
    }

    /// Results in. True: a redraw is due.
    pub(crate) fn poll(&mut self) -> bool {
        let mut any = false;

        while let Ok((run, query, matches)) = self.rx.try_recv() {
            self.apply_result(&mut any, run, query, matches);
        }

        any
    }

    fn apply_result(&mut self, any: &mut bool, run: u64, query: String, matches: Vec<String>) {
        if run == self.run && self.open {
            self.query = query;
            self.matches = matches;
            self.cursor = self.cursor.min(self.matches.len().saturating_sub(1));
            *any = true;
        }
    }

    /// Key K while open. INPUT: the input text after the key went to the input (the query).
    /// True: the key is taken (the input does not get it).
    pub(crate) fn key(&mut self, key: KeyEvent) -> bool {
        key_hist_search(self, key)
    }

    /// The input changed to TEXT: a new search (out of shell mode: closed).
    pub(crate) fn input_changed(&mut self, text: &str) {
        if !self.open {
            return;
        }

        match text.strip_prefix('!') {
            Some(query) if !query.contains('\n') => {
                refresh_history_query(query, self);
            }
            _ => self.close(),
        }
    }

    /// The dropdown above INPUT (inside BOUNDS).
    pub(crate) fn render(&mut self, frame: &mut Frame, bounds: Rect, input: Rect) {
        render_hist_search(self, frame, bounds, input);
    }
}

/// `fzf --filter Q` on ALL (best first). None: fzf failed.
fn fzf_filter(all: &[String], query: &str) -> Option<Vec<String>> {
    if query.trim().is_empty() {
        return Some(all.iter().take(MAX).cloned().collect());
    }

    // 1 record per command: NUL-separated (multi-line commands stay whole)
    let mut child = Command::new("fzf")
        .args(["--filter", query, "--scheme=history", "--read0", "--print0"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take()?;
    let input = all.join("\0");

    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });

    let out = child.wait_with_output().ok()?;
    let _ = writer.join();

    // exit 1: 0 matches
    if !matches!(out.status.code(), Some(0 | 1)) {
        return None;
    }

    Some(
        String::from_utf8_lossy(&out.stdout)
            .split('\0')
            .filter(|record| !record.is_empty())
            .take(MAX)
            .map(String::from)
            .collect(),
    )
}

/// The built-in fuzzy filter: every word matches, best first, newest first on a tie.
fn builtin(all: &[String], query: &str) -> Vec<String> {
    crate::ui::fuzzy::rank(all, query, std::clone::Clone::clone, |_| String::new())
        .into_iter()
        .take(MAX)
        .map(|(i, _)| all[i].clone())
        .collect()
}

/// The shell's history, newest first: fish, then zsh, then bash (each file that exists).
pub(crate) fn shell_history() -> Vec<String> {
    let home = crate::infra::config::home();
    let data = std::env::var_os("XDG_DATA_HOME").map_or_else(|| home.join(".local/share"), PathBuf::from);
    let mut out = Vec::new();

    let read = |path: &Path| {
        std::fs::read(path)
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    };

    if let Some(history) = read(&data.join("fish/fish_history")) {
        out.extend(fish(&history).into_iter().rev());
    }

    let zsh = std::env::var_os("HISTFILE")
        .map(PathBuf::from)
        .filter(|path| path.to_string_lossy().contains("zsh"))
        .unwrap_or_else(|| home.join(".zsh_history"));

    if let Some(history) = read(&zsh) {
        out.extend(zsh_lines(&history).into_iter().rev());
    }

    if let Some(history) = read(&home.join(".bash_history")) {
        out.extend(
            history
                .lines()
                .filter(|line| !line.starts_with('#'))
                .map(String::from)
                .collect::<Vec<_>>()
                .into_iter()
                .rev(),
        );
    }

    out
}

/// Commands of a fish history file (oldest first): `- cmd: …`, `\n` and `\\` escaped.
fn fish(history: &str) -> Vec<String> {
    history
        .lines()
        .filter_map(|line| line.strip_prefix("- cmd: "))
        .map(|command| command.replace("\\n", "\n").replace("\\\\", "\\"))
        .collect()
}

/// Commands of a zsh history file (oldest first): plain or extended (`: 1700000000:0;cmd`),
/// a line ending in `\` goes on in the next.
fn zsh_lines(history: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cont = false;

    for line in history.lines() {
        let body = if cont {
            line
        } else if let Some(rest) = line.strip_prefix(": ") {
            rest.split_once(';').map_or(line, |(_, command)| command)
        } else {
            line
        };

        let (body, more) = match body.strip_suffix('\\') {
            Some(stripped) => (stripped, true),
            None => (body, false),
        };

        if cont {
            append_history_continuation(&mut out, body);
        } else {
            out.push(body.to_string());
        }

        cont = more;
    }

    out
}

fn append_history_continuation(out: &mut [String], body: &str) {
    if let Some(last) = out.last_mut() {
        last.push('\n');
        last.push_str(body);
    }
}

#[cfg(test)]
#[path = "prompt_input_history_search_tests.rs"]
mod tests;

fn refresh_history_query(input: &str, history: &mut HistSearch) {
    let query = input.trim_start();

    if query != history.query {
        history.cursor = 0;
        history.search(query);
    }
}

fn select_history_match(history: &mut HistSearch) {
    if let Some(command) = history.matches.get(history.cursor) {
        history.picked = Some(format!("! {command}"));
    }
}

fn search_hist_search(history: &mut HistSearch, query: &str) {
    let Some(all) = history.all.clone() else { return };

    history.run += 1;
    let (run, tx, query, fzf) = (history.run, history.tx.clone(), query.to_string(), history.fzf);

    let work = move || {
        let matches = if fzf {
            fzf_filter(&all, &query).unwrap_or_else(|| builtin(&all, &query))
        } else {
            builtin(&all, &query)
        };

        let _ = tx.send((run, query, matches));
    };

    if cfg!(test) {
        work();
    } else {
        std::thread::spawn(work);
    }
}

fn key_hist_search(history: &mut HistSearch, key: KeyEvent) -> bool {
    if !history.open {
        return false;
    }

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match key.code {
        KeyCode::Esc => history.close(),
        // the best match sits at the bottom (right above the input): ↑ goes to worse ones
        KeyCode::Down => history.cursor = history.cursor.saturating_sub(1),
        KeyCode::Char('n') if ctrl => history.cursor = history.cursor.saturating_sub(1),
        // ctrl+r again: the next (older) match, like a shell
        KeyCode::Up => {
            history.cursor = (history.cursor + 1).min(history.matches.len().saturating_sub(1));
        }
        KeyCode::Char('r') if ctrl => {
            history.cursor = (history.cursor + 1).min(history.matches.len().saturating_sub(1));
        }
        KeyCode::PageDown => history.cursor = history.cursor.saturating_sub(dropdown::MAX_ROWS),
        KeyCode::PageUp => {
            history.cursor = (history.cursor + dropdown::MAX_ROWS).min(history.matches.len().saturating_sub(1));
        }
        KeyCode::Enter | KeyCode::Tab => {
            select_history_match(history);

            history.close();
        }
        _ => return false,
    }

    true
}

fn render_hist_search(history: &mut HistSearch, frame: &mut Frame, bounds: Rect, input: Rect) {
    if !history.open {
        return;
    }

    let width = input.width.max(30);
    let anchor = Rect {
        x: input.x,
        y: input.y,
        width,
        height: 1,
    };

    let Some(area) = dropdown::place(anchor, width, history.matches.len(), bounds, true) else {
        return;
    };

    let rows = history_rows(history);

    let note = format!(
        "{} · {} ",
        history.matches.len(),
        if history.fzf { "fzf" } else { "fuzzy" }
    );
    let count = rows.len();
    let cursor = count.saturating_sub(1).saturating_sub(history.cursor);

    // a new list: scrolled to its bottom (fit keeps the cursor in view)
    let offset = if history.cursor == 0 { count } else { history.offset };

    let (off, _) = dropdown::list(
        frame,
        area,
        dropdown::List {
            label: "history · ctrl+r",
            note: &note,
            rows: &rows,
            cursor,
            offset,
            empty: "0 matches",
        },
    );

    history.offset = off;
}

fn history_rows(history: &mut HistSearch) -> Vec<dropdown::Row> {
    let words: Vec<&str> = history.query.split_whitespace().collect();

    // best match last: at the bottom, right above the input (fzf's default layout)
    let rows: Vec<dropdown::Row> = history
        .matches
        .iter()
        .rev()
        .map(|command| {
            let text = command.replace('\n', " ⏎ ");
            let mut marks: Vec<usize> = words
                .iter()
                .filter_map(|word| crate::domain::matcher::word(word, &text))
                .flat_map(|(_, positions)| positions)
                .collect();

            marks.sort_unstable();
            marks.dedup();
            dropdown::Row {
                text,
                marks,
                ..Default::default()
            }
        })
        .collect();

    rows
}
