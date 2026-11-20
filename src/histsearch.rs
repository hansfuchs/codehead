//! ctrl+r in shell mode: the command history, searched by fzf in the background (`fzf --filter`:
//! 0 TTY, 0 suspend), the matches in a dropdown above the input, the best one at the bottom
//! (right above the input). The input after `!` is the query; every change runs fzf again (an
//! older run's result is dropped). ↑ (ctrl+r, ctrl+p) worse matches, ↓ (ctrl+n) better ones,
//! enter / tab put the match into the input, esc closes. 0 fzf on PATH: the built-in fuzzy filter.
//!
//! The history: the thread's `!commands` (newest first), then the shell's own history file
//! (fish, zsh, bash; newest first), 1 row per command.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use crate::dropdown;

/// Most matches kept.
const MAX: usize = 300;

pub struct HistSearch {
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
    tx: Sender<(u64, String, Vec<String>)>,
    rx: Receiver<(u64, String, Vec<String>)>,
    /// The pick of the last enter: the new input text.
    pub picked: Option<String>,
    fzf: bool,
}

impl Default for HistSearch {
    fn default() -> Self {
        let (tx, rx) = channel();
        HistSearch { open: false, all: None, query: String::new(), matches: Vec::new(), cursor: 0, offset: 0, run: 0, tx, rx, picked: None, fzf: crate::engine::harnesses::on_path("fzf") }
    }
}

impl HistSearch {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Open with the thread's commands THREAD (oldest first) and QUERY (the input after `!`).
    pub fn open(&mut self, thread: Vec<String>, query: &str) {
        self.open = true;
        self.cursor = 0;
        self.offset = 0;
        let mut all: Vec<String> = thread.into_iter().rev().collect();
        all.extend(shell_history());
        let mut seen = std::collections::HashSet::new();
        all.retain(|c| !c.trim().is_empty() && seen.insert(c.clone()));
        self.all = Some(Arc::new(all));
        self.search(query);
    }

    pub fn close(&mut self) {
        self.open = false;
        self.matches.clear();
        self.all = None;
    }

    /// Run the search for QUERY in the background.
    fn search(&mut self, query: &str) {
        let Some(all) = self.all.clone() else { return };
        self.run += 1;
        let (run, tx, q, fzf) = (self.run, self.tx.clone(), query.to_string(), self.fzf);
        let work = move || {
            let m = if fzf { fzf_filter(&all, &q).unwrap_or_else(|| builtin(&all, &q)) } else { builtin(&all, &q) };
            let _ = tx.send((run, q, m));
        };
        if cfg!(test) {
            work();
        } else {
            std::thread::spawn(work);
        }
    }

    /// Results in. True: a redraw is due.
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok((run, q, m)) = self.rx.try_recv() {
            if run == self.run && self.open {
                self.query = q;
                self.matches = m;
                self.cursor = self.cursor.min(self.matches.len().saturating_sub(1));
                any = true;
            }
        }
        any
    }

    /// Key K while open. INPUT: the input text after the key went to the input (the query).
    /// True: the key is taken (the input does not get it).
    pub fn key(&mut self, k: KeyEvent) -> bool {
        if !self.open {
            return false;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc => self.close(),
            // the best match sits at the bottom (right above the input): ↑ goes to worse ones
            KeyCode::Down => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Char('n') if ctrl => self.cursor = self.cursor.saturating_sub(1),
            // ctrl+r again: the next (older) match, like a shell
            KeyCode::Up => self.cursor = (self.cursor + 1).min(self.matches.len().saturating_sub(1)),
            KeyCode::Char('p' | 'r') if ctrl => self.cursor = (self.cursor + 1).min(self.matches.len().saturating_sub(1)),
            KeyCode::PageDown => self.cursor = self.cursor.saturating_sub(dropdown::MAX_ROWS),
            KeyCode::PageUp => self.cursor = (self.cursor + dropdown::MAX_ROWS).min(self.matches.len().saturating_sub(1)),
            KeyCode::Enter | KeyCode::Tab => {
                if let Some(m) = self.matches.get(self.cursor) {
                    self.picked = Some(format!("! {m}"));
                }
                self.close();
            }
            _ => return false,
        }
        true
    }

    /// The input changed to TEXT: a new search (out of shell mode: closed).
    pub fn input_changed(&mut self, text: &str) {
        if !self.open {
            return;
        }
        match text.strip_prefix('!') {
            Some(q) if !q.contains('\n') => {
                let q = q.trim_start();
                if q != self.query {
                    self.cursor = 0;
                    self.search(q);
                }
            }
            _ => self.close(),
        }
    }

    /// The dropdown above INPUT (inside BOUNDS).
    pub fn render(&mut self, f: &mut Frame, bounds: Rect, input: Rect) {
        if !self.open {
            return;
        }
        let w = input.width.max(30);
        let anchor = Rect { x: input.x, y: input.y, width: w, height: 1 };
        let Some(r) = dropdown::place(anchor, w, self.matches.len(), bounds, true) else { return };
        let words: Vec<&str> = self.query.split_whitespace().collect();
        // best match last: at the bottom, right above the input (fzf's default layout)
        let rows: Vec<dropdown::Row> = self
            .matches
            .iter()
            .rev()
            .map(|m| {
                let text = m.replace('\n', " ⏎ ");
                let mut marks: Vec<usize> = words.iter().filter_map(|q| crate::fuzzy::word(q, &text)).flat_map(|(_, p)| p).collect();
                marks.sort_unstable();
                marks.dedup();
                dropdown::Row { text, marks, ..Default::default() }
            })
            .collect();
        let note = format!("{} · {} ", self.matches.len(), if self.fzf { "fzf" } else { "fuzzy" });
        let n = rows.len();
        let cur = n.saturating_sub(1).saturating_sub(self.cursor);
        // a new list: scrolled to its bottom (fit keeps the cursor in view)
        let offset = if self.cursor == 0 { n } else { self.offset };
        let (off, _) = dropdown::list(f, r, "history · ctrl+r", &note, &rows, cur, offset, "0 matches");
        self.offset = off;
    }
}

/// `fzf --filter Q` on ALL (best first). None: fzf failed.
fn fzf_filter(all: &[String], q: &str) -> Option<Vec<String>> {
    if q.trim().is_empty() {
        return Some(all.iter().take(MAX).cloned().collect());
    }
    // 1 record per command: NUL-separated (multi-line commands stay whole)
    let mut child = Command::new("fzf").args(["--filter", q, "--scheme=history", "--read0", "--print0"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut stdin = child.stdin.take()?;
    let input = all.join("\0");
    let w = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let out = child.wait_with_output().ok()?;
    let _ = w.join();
    // exit 1: 0 matches
    if !matches!(out.status.code(), Some(0 | 1)) {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).split('\0').filter(|s| !s.is_empty()).take(MAX).map(String::from).collect())
}

/// The built-in fuzzy filter: every word matches, best first, newest first on a tie.
fn builtin(all: &[String], q: &str) -> Vec<String> {
    crate::fuzzy::rank(all, q, |s| s.clone(), |_| String::new()).into_iter().take(MAX).map(|(i, _)| all[i].clone()).collect()
}

/// The shell's history, newest first: fish, then zsh, then bash (each file that exists).
fn shell_history() -> Vec<String> {
    let home = crate::config::home();
    let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share"));
    let mut out = Vec::new();
    let read = |p: &Path| std::fs::read(p).ok().map(|b| String::from_utf8_lossy(&b).into_owned());
    if let Some(t) = read(&data.join("fish/fish_history")) {
        out.extend(fish(&t).into_iter().rev());
    }
    let zsh = std::env::var_os("HISTFILE").map(PathBuf::from).filter(|p| p.to_string_lossy().contains("zsh")).unwrap_or_else(|| home.join(".zsh_history"));
    if let Some(t) = read(&zsh) {
        out.extend(zsh_lines(&t).into_iter().rev());
    }
    if let Some(t) = read(&home.join(".bash_history")) {
        out.extend(t.lines().filter(|l| !l.starts_with('#')).map(String::from).collect::<Vec<_>>().into_iter().rev());
    }
    out
}

/// Commands of a fish history file (oldest first): `- cmd: …`, `\n` and `\\` escaped.
fn fish(t: &str) -> Vec<String> {
    t.lines().filter_map(|l| l.strip_prefix("- cmd: ")).map(|c| c.replace("\\n", "\n").replace("\\\\", "\\")).collect()
}

/// Commands of a zsh history file (oldest first): plain or extended (`: 1700000000:0;cmd`),
/// a line ending in `\` goes on in the next.
fn zsh_lines(t: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cont = false;
    for l in t.lines() {
        let body = if cont {
            l
        } else if let Some(rest) = l.strip_prefix(": ") {
            rest.split_once(';').map_or(l, |(_, c)| c)
        } else {
            l
        };
        let (body, more) = match body.strip_suffix('\\') {
            Some(b) => (b, true),
            None => (body, false),
        };
        if cont {
            if let Some(last) = out.last_mut() {
                last.push('\n');
                last.push_str(body);
            }
        } else {
            out.push(body.to_string());
        }
        cont = more;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_files_parse() {
        assert_eq!(fish("- cmd: ls -la\n  when: 1\n- cmd: echo a\\nb\n"), vec!["ls -la", "echo a\nb"]);
        assert_eq!(zsh_lines(": 1700000000:0;git status\nplain\n: 1:0;for x in 1\\\ndo\n"), vec!["git status", "plain", "for x in 1\ndo"]);
    }

    #[test]
    fn the_thread_comes_first_dupes_go_and_enter_picks() {
        let mut h = HistSearch { fzf: false, ..Default::default() };
        h.open(vec!["cargo test".into(), "just build".into()], "");
        h.poll();
        assert_eq!(&h.matches[..2], &["just build".to_string(), "cargo test".to_string()]);
        h.input_changed("!cargo");
        h.poll();
        assert_eq!(h.matches.first().map(String::as_str), Some("cargo test"));
        assert!(h.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        assert_eq!(h.picked.as_deref(), Some("! cargo test"));
        assert!(!h.is_open());
    }

    #[test]
    fn out_of_shell_mode_closes() {
        let mut h = HistSearch { fzf: false, ..Default::default() };
        h.open(vec![], "x");
        h.input_changed("no bang");
        assert!(!h.is_open());
    }

    #[test]
    fn fzf_ranks_when_present() {
        if !crate::engine::harnesses::on_path("fzf") {
            return;
        }
        let all: Vec<String> = ["git push", "cargo build --release", "echo cbr"].map(String::from).to_vec();
        let m = fzf_filter(&all, "cargo rel").unwrap();
        assert_eq!(m, vec!["cargo build --release"]);
        assert_eq!(fzf_filter(&all, "zzzz").unwrap(), Vec::<String>::new());
    }
}
