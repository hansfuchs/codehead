// Completions in shell mode (`!` in the prompt), like an IDE or fish:
//
// - project tasks: recipes of the nearest justfile, scripts of the nearest package.json (run with
//   the package manager of its lockfile), targets of the nearest Makefile. As the 1st word
//   (`bu` → `just build`), and after their runner (`just `, `pnpm `, `npm run `, `make `).
// - commands and their arguments: fish's own completions (`fish --no-config -c 'complete -C …'`,
//   in the background, descriptions included), commands ranked by their use in the history.
//   0 fish on PATH: the commands on PATH (1st word only).
// - a suggestion from the history after the cursor (gray, fish's autosuggestion): → / End
//   take it, alt+→ its next word (ctrl+f: the chat search).
//
// The dropdown opens by itself once a word is typed (after a runner: at once), tab opens it for
// any word. ↑↓ move (the row's text into the prompt at once), tab takes the row, enter only after
// ↑↓ (else enter runs the command). esc: closed for this word. A word with a `/`: the path
/// dropdown's.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use ratatui::text::Span;

use crate::ui::dropdown;
use crate::ui::textarea::{Pos, TextArea};
use crate::ui::theme;

/// Project tasks are read again after this time.
const FRESH: Duration = Duration::from_secs(3);
/// The longest a fish completion runs.
const FISH_TIMEOUT: Duration = Duration::from_millis(1500);
/// Most rows kept per source.
const MAX: usize = 200;

/// A task of the project: its runner (`just`, `pnpm`, `make`), name, command line, doc.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Task {
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

#[derive(Default)]
pub(crate) struct CommandCatalog {
    /// The history (newest first) and the commands on PATH, None until loaded.
    pub history: Option<Arc<Vec<String>>>,
    asked_history: bool,
    commands: Option<Arc<Vec<String>>>,
    asked_commands: bool,
}

type CommandHistoryKey = (usize, bool);
type CommandUsage = Arc<HashMap<String, usize>>;
type Suggestion = (CommandHistoryKey, String, Option<String>);

pub(crate) struct ShellComp {
    fish: bool,
    tx: Option<Sender<Req>>,
    res_tx: crate::infra::bus::Tx<Res>,
    rx: Receiver<Res>,
    /// fish's rows per command line (the last few).
    fish_rows: HashMap<String, Vec<(String, String)>>,
    asked: String,
    pub(crate) catalog: CommandCatalog,
    tasks: Option<(PathBuf, Instant, Vec<Task>)>,
    /// Per (the thread's `!commands`, history loaded): the uses of each command, and the last
    /// suggestion (the line, the rest).
    uses: Option<(CommandHistoryKey, CommandUsage)>,
    sugg: Option<Suggestion>,
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
        let (res_tx, rx) = crate::infra::bus::channel();

        ShellComp {
            fish: crate::engine::harnesses::on_path("fish"),
            tx: None,
            res_tx,
            rx,
            fish_rows: HashMap::new(),
            asked: String::new(),
            catalog: crate::features::prompt_input::shell_completion::CommandCatalog {
                history: None,
                asked_history: false,
                commands: None,
                asked_commands: false,
            },

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

impl ShellComp {
    /// Send REQ to the worker (tests: run in place).
    fn ask(&mut self, req: Req) {
        ask_shell_comp(self, req);
    }

    /// The project's tasks for DIR (shell mode's dir), read again after FRESH.
    pub(crate) fn tasks(&mut self, dir: &Path) -> Vec<Task> {
        match &self.tasks {
            Some((directory, at, thread)) if directory == dir && at.elapsed() < FRESH => thread.clone(),
            _ => {
                let thread_value = tasks(dir);

                self.tasks = Some((dir.to_path_buf(), Instant::now(), thread_value.clone()));
                thread_value
            }
        }
    }

    /// fish's rows of an earlier line this one extends in the same word (0 flicker while fish runs).
    fn stale(&self, ctx: &Ctx) -> Vec<(String, String)> {
        stale_shell_comp(self, ctx)
    }

    /// The open dropdown: the context (the typed word: the filter while a preview is in the
    /// text) and its rows. None: closed.
    fn open(&mut self, ta: &TextArea, dir: &Path, thread: &[String]) -> Option<(Ctx, Vec<Entry>)> {
        open_shell_comp(self, ta, dir, thread)
    }

    fn pick(&mut self, ta: &mut TextArea, ctx: &Ctx, error: &Entry) {
        ta.replace_to_cursor(ctx.at, &error.text);
        self.preview.clear();
        self.forced = None;
    }

    /// Not drawn this frame: 0 rows for clicks.
    pub(crate) fn hide(&mut self) {
        self.rows.clear();
        self.area = Rect::default();
    }

    /// The history's suggestion for TA (the cursor at the text's end): the rest of the newest
    /// command (THREAD first, then the shell's history) that starts with the typed line.
    pub(crate) fn suggestion(&mut self, ta: &TextArea, thread: &[String]) -> Option<String> {
        suggestion_shell_comp(self, ta, thread)
    }

    /// → / End: the suggestion into TA; alt+→: its next word. True: used.
    pub(crate) fn accept(&mut self, key: KeyEvent, ta: &mut TextArea, thread: &[String]) -> bool {
        accept_shell_comp(self, key, ta, thread)
    }
}

#[cfg(test)]
#[path = "prompt_input_shell_completion_tests.rs"]
mod tests;

#[path = "shell_completion_draw_helpers.rs"]
mod draw_helpers;
#[path = "shell_completion_key.rs"]
mod key;
#[path = "shell_completion_mouse.rs"]
mod mouse;
#[path = "shell_completion_poll.rs"]
mod poll;
#[path = "shell_completion_render.rs"]
mod render;

fn own(thread_5: &Task, result: &str) -> bool {
    if result == "package" {
        !matches!(thread_5.runner, "just" | "make")
    } else {
        thread_5.runner == result
    }
}

#[path = "shell_completion_worker.rs"]
mod worker;
use worker::{done, work};

#[path = "shell_completion_tasks.rs"]
mod tasks;
pub(crate) use tasks::tasks;

#[cfg(test)]
use worker::fish_complete;

fn ask_shell_comp(shell: &mut ShellComp, req: Req) {
    if cfg!(test) {
        let _ = shell.res_tx.send(work(req));

        return;
    }

    let tx = shell.tx.get_or_insert_with(|| {
        let (tx, rx) = channel::<Req>();
        let out = shell.res_tx.clone();

        std::thread::spawn(move || {
            run_requests(&rx, &out);
        });

        tx
    });

    let _ = tx.send(req);
}

fn run_requests(rx: &Receiver<Req>, out: &crate::infra::bus::Tx<Res>) {
    while let Ok(mut req) = rx.recv() {
        // fish: only the newest line counts (typed on meanwhile)
        let mut later = Vec::new();

        while let Ok(result) = rx.try_recv() {
            match (result, &req) {
                (result_value @ Req::Fish(..), Req::Fish(..)) => req = result_value,
                (result_2, _) => later.push(result_2),
            }
        }

        if std::iter::once(req)
            .chain(later)
            .any(|request| out.send(work(request)).is_err())
        {
            return;
        }
    }
}

fn stale_shell_comp(shell: &ShellComp, ctx: &Ctx) -> Vec<(String, String)> {
    let head = &ctx.line[..ctx.line.len() - ctx.word.len()];

    shell
        .fish_rows
        .iter()
        .filter(|(line_value, _)| {
            line_value.starts_with(head)
                && ctx.line.starts_with(line_value.as_str())
                && !line_value[head.len()..].contains(' ')
        })
        .max_by_key(|(line_2, _)| line_2.len())
        .map(|(_, result)| {
            result
                .iter()
                .filter(|(thread, _)| thread.starts_with(&ctx.word))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

fn open_shell_comp(shell: &mut ShellComp, ta: &TextArea, dir: &Path, thread: &[String]) -> Option<(Ctx, Vec<Entry>)> {
    // a live preview in the text: the word as typed before it (the filter)
    let ctx = match shell.preview.typed(ta) {
        Some((path, typed)) if path.row == 0 && context(ta).is_some() => {
            let head: String = ta.text().chars().take(path.col).collect();
            let text = format!("{head}{typed}");

            context_of(&text, text.chars().count())
        }
        _ => context(ta),
    }?;

    if shell.closed.is_some_and(|path_value| path_value != ctx.at) {
        shell.closed = None;
    }

    if shell.forced.is_some_and(|path_2| path_2 != ctx.at) {
        shell.forced = None;
    }

    if shell.closed == Some(ctx.at) {
        return None;
    }

    let forced = shell.forced == Some(ctx.at);
    let runner = runner_of(&ctx.before).is_some();

    if ctx.word.is_empty() && !forced && !runner {
        return None;
    }

    let entries = self::entries(&ctx, dir, thread, shell);

    // 0 rows, or the word is the 1 row already
    if entries.is_empty() || (entries.len() == 1 && entries[0].text.trim_end() == ctx.word) {
        return None;
    }

    if shell.seen != ctx.line {
        shell.seen.clone_from(&ctx.line);
        shell.cursor = 0;
        shell.offset = 0;
        shell.active = false;
    }

    shell.cursor = shell.cursor.min(entries.len() - 1);

    Some((ctx, entries))
}

fn suggestion_shell_comp(shell: &mut ShellComp, ta: &TextArea, thread: &[String]) -> Option<String> {
    let text = ta.text();
    let line = text.strip_prefix('!')?.trim_start();
    let (row, col) = ta.cursor();

    if line.is_empty() || text.contains('\n') || row != 0 || col != text.chars().count() {
        return None;
    }

    let hist = history(shell);
    let key = (thread.len(), shell.catalog.history.is_some());

    if let Some((key_value, line_value, rest)) = &shell.sugg
        && *key_value == key
        && line_value == line
    {
        return rest.clone();
    }

    let rest = thread
        .iter()
        .rev()
        .chain(hist.iter())
        .find(|character| character.len() > line.len() && character.starts_with(line) && !character.contains('\n'))
        .map(|character_value| character_value[line.len()..].to_string());

    shell.sugg = Some((key, line.to_string(), rest.clone()));

    rest
}

fn accept_shell_comp(shell: &mut ShellComp, key: KeyEvent, ta: &mut TextArea, thread: &[String]) -> bool {
    let alt = key.modifiers == KeyModifiers::ALT;
    let all = matches!(
        (key.code, key.modifiers),
        (KeyCode::Right | KeyCode::End, KeyModifiers::NONE)
    );

    if !(all || alt && key.code == KeyCode::Right) {
        return false;
    }

    let Some(rest) = shell.suggestion(ta, thread) else {
        return false;
    };

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

#[path = "shell_completion_context.rs"]
mod context;
use context::{context, context_of, runner_of};

#[path = "shell_completion_catalog.rs"]
mod catalog;
#[cfg(test)]
use catalog::texts;
use catalog::{entries, history};
