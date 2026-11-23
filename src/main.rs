#![recursion_limit = "256"]
//! codehead: pick notes, write the task, then work the thread with its agents in this terminal.

mod avatar;
mod agents;
mod ansi;
mod app;
mod ask;
mod attach;
mod cards;
mod changes;
mod chatsearch;
mod background;
mod claude;
mod clip;
mod code;
mod command;
mod config;
mod cost;
mod drafts;
mod handoff;
mod harnesstab;
mod gh;
mod histsearch;
mod home;
mod homeinit;
mod tutor;
mod wizard;
mod editor;
mod files;
mod graphics;
mod flowbar;
mod flowrail;
mod flows;
mod fuzzy;
mod engine;
mod dropdown;
mod mention;
mod merge;
mod mergetab;
mod models;
mod monitors;
mod nav;
mod mcplist;
mod mcptab;
mod menu;
mod notes;
mod notify;
mod paths;
#[cfg(test)]
mod perf;
mod picker;
mod prlist;
mod portless;
mod preview;
mod rank;
mod screensel;
mod search;
mod review;
mod render;
mod repofilter;
mod repopick;
mod settings;
mod shellcomp;
mod shortcuts;
mod setup;
mod skills;
mod sprites;
mod stream;
mod subagents;
mod subthreads;
mod dissolve;
mod styles;
mod tabgroup;
mod textarea;
mod theme;
mod threads;
mod workspot;
mod update;
mod urls;
mod view;

use std::io::{self, Stdout, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event, KeyEventKind, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::crossterm::{cursor, execute};

use app::{App, Config, Outcome, Suspend, Target};

const HELP: &str = "codehead: write the task (+ notes, + Claude Code context), then work the thread with its agents.

usage: codehead                    the thread updated last (0 threads: the start screen)
       codehead [-a agent[,agent…]] [-b branch] [-c] [-n] [title...]
       codehead --view <thread>   thread view of a thread (id or id prefix)
       codehead --cost <thread>   cost box of a thread, full screen
       codehead --version         print the version
       codehead home [init]       what codehead keeps in the home repo; init: move agents, flows, servers, settings there
       codehead setup [--dry]     the setup wizard; --dry: every screen, 0 writes, then the list of what a real run does
       codehead --update          install the latest release (curl -fsSL https://codehead.sh/install | sh)
  -a  agents preselected for the new thread (default: the default agent)
  -b  branch for the new worktree (default: <prefix>/<title slug>, prefix \"task\")
  -c  copy the picked note paths to the clipboard, start nothing
  -n  dry run: print what would start, start nothing
  title text prefills the title input.

config:  ~/.config/codehead.toml ($CODEHEAD_CONFIG overrides). keys, each overridden by its env var:
           notes  = \"~/notes\"   notes root ($CODEHEAD_NOTES; default: notes/ of the codehead app dir)
           styles = \"~/styles\"  output styles ($CODEHEAD_STYLES; default: output-styles/ of the app dir)
           repo   = \"~/app\"     repo of the new tab's where rows ($CODEHEAD_REPO; default: the start dir's)
           agent  = \"amon\"      default agent ($CODEHEAD_AGENT)
           prefix = \"task\"      branch prefix ($CODEHEAD_PREFIX)
           home   = \"~/home\"    home repo ($CODEHEAD_HOME_REPO). with home/codehead/ (codehead home init): settings
                                   there too, shared: codehead/settings.toml, this device: codehead/devices/<device>.toml
           device = \"mbp\"       this device's name (default: the host name)
           models = \"fable,opus,sonnet,haiku\"  Claude models ($CODEHEAD_MODELS)
           messages = \"bubbles\"  thread view messages: bubbles or lines ($CODEHEAD_MESSAGES)
           thoughts = \"hidden\"  thread view thoughts: hidden, folded or open ($CODEHEAD_THOUGHTS)
           [keys]   menu = \"ctrl+p\" ($CODEHEAD_MENU_KEY), command = \">\" ($CODEHEAD_COMMAND_KEY)
           [colors] scheme = \"lavender\" | \"coral\" | \"amber\" ($CODEHEAD_SCHEME); accent, text, … = \"#RRGGBB\"
           [aliases]
           \"project:platform\" = \"~/projects/exampleorg/project/notes/platform/\"
settings: 6th tab (F6). edit fields and path aliases; ctrl+s saves, ctrl+r discards edits / reloads.
          enter on + add (menu: add path alias) adds an alias; ctrl+d / d removes the current alias, on a
          field ctrl+d: back to its default (↺ too).
          path fields use the prompt's path suggestions: tab completes, esc closes suggestions.
          esc on a field: its edit undone (0 apply). tab / shift+tab: next / previous field.
          environment variables override file values. edits stay until saved or reloaded.
default agent: config agent, else amon, else the first agent.
         preselected for new threads, takes over threads of deleted agents.
project: git repo of the current directory. the task's where rows: repo (enter: a 2-pane picker,
         left the parent's dirs, right the current dir's, ←→ out / in, type: filter, enter: pick;
         home repos without a clone: cloned first), then branch (enter: dropdown): the current
         branch (default, works there), main, the other branches (newest commit first), a new branch
         (main checkout, git switch -c, 0 worktrees), or a new branch + worktree (-b sets this).
         below it: checkout the picked branch (else the main checkout works as is), stash local
         changes that block the checkout (git stash). a start on main / master in an existing
         checkout: git pull --ff-only in the thread before the 1st turn (a failed pull: a note
         to the agents, the thread goes on). setting repo: the where rows start on that repo.
         0 git repos: the current directory itself (thread works in place)
prompt:  @: agent names. >: commands, in dropdowns above the prompt box (never over it; a submenu
         beside its parent, 0 room there: above them). max. 10 rows, ▲ ▼ on the right border: more rows.
         mention → files (of the repo / worktree), notes, threads, artifacts (urls in the Claude
           Code transcripts) → a list, oldest on top, the latest current; typed text: fuzzy filter.
           enter: the relative path / note path / thread name / url goes into the text.
         skills → the skills of the agents' harnesses (Claude Code, Codex: SKILL.md files of the
           user, the plugins, the project) with their description. enter: /name ($name: Codex).
         esc: 1 level back.
         a path (src/, ./a/b, ~/x/, /usr/lo): the entries of that directory in a dropdown, as the
         path is typed (dirs first; hidden ones after a `.`). relative: from the repo / worktree
         top, else the dir. tab inserts the row (a dir: its entries next); enter too, after ↑↓.
         aliases work as paths: project:platform/ lists the target directory. prompts retain
         short aliases; sent messages expand aliases to full paths.
         ctrl+v: clipboard image → [Image #n], a dropped image path too; text over
         1024 chars (code: never) → [Pasted text #n +L lines]. the files go into the message as a list.
drafts:  an unsent task goes to ~/.cache/codehead/drafts ($CODEHEAD_DRAFTS) after every key. the new tab
         shows \"view drafts (n)\" on top: enter lists them; enter loads 1, d d deletes it, esc: back.
         a started thread drops its draft. drafts older than 30 days go. \"save as draft\" (right of
         it) or ctrl+s: the task as a kept draft (★ saved, 0 cleanup by age), the form starts over.
         n in the menu: an empty form, the task on screen stays a draft.
artifacts: codehead sets CLAUDE_CODE_ARTIFACT=1 for the sessions it starts: Claude Code agents can
         publish web artifacts (off by default in SDK sessions). a value set before codehead stays.
esc:     1 level back: never saves, never drops edits unasked (edited text: esc again within 2 s drops
         it; the agent form and reset ask).
keys:    1 key, 1 meaning: a letter key never acts with ctrl / alt / ⌘ held (alt+P never pushes).
         outward or destructive keys ask first: a 2nd press (d d, S S, ctrl+d ctrl+d, enter enter).
tabs:    start screen: F1-F6 (alt+1-6) switch tabs (word jumps: ctrl / alt+←→). tab / shift+tab: next /
         previous field (on a row of tabs: into its content). ←→ on a row of tabs: the next / previous tab.
quit:    esc esc on the start screen; ctrl+c 2× within 1 s (a 1st ctrl+c on text: clears it, ctrl+z back);
         menu → quit codehead. q never quits.
updates: 1 check per day (github.com releases, in the background). a newer release: 1 line after
         codehead quits. codehead --update installs it. $CODEHEAD_NO_UPDATE_CHECK=1: 0 checks.
debug:   $CODEHEAD_KEYLOG=<file>: every key of the text inputs, with its time (a report on keys the
         terminal sends in a way codehead does not expect, e.g. the macOS double-space period).
inputs:  all text fields (title, prompt, agent form, filters, thread prompt): ctrl+z / ⌘z undo,
         ctrl+shift+z / ⌘⇧z redo (ctrl+y too). ⌘ keys need a terminal with the kitty keyboard protocol.
         Markdown lists (`- `, `* `, `1. `, 2 spaces per layer): enter starts the next item at the same
         layer; enter / backspace on an empty item: 1 layer out (1st layer: the marker goes), space: 1 layer in.
start:   the new tab: the prompt first (its 1st line: the thread title, unless the title field has one),
         then title, repo, where, opts, agents (f on a card: its flow), notes. type, ctrl+enter. notes: tab to the notes field, enter.
threads: worktrees, then recent Claude Code sessions. enter on a session:
         new thread in its dir, its last response as context.
thread view: the thread's agents on top (sprite, model, working / idle / waits on you; ★ the lead),
         the stream (every message, tool run and open question, from the thread's event log), the
         prompt below, the cost box beside it (screens ≥ 100 columns).
         ctrl+q: the agents box has the keys: ←→ an agent, its menu above it (flow: its flow box,
         remove: 2 enters, replace with: an agent off the thread takes over, flow too); esc back.
         ctrl+m (kitty protocol): the prompt again, from every pane, box, filter and the menu.
         below the prompt: who gets the message, where the thread works (repo ⎇ branch, (worktree);
         1 per checkout, read again every 3 s).
         a message without @name goes to the lead; @name: to that agent (an agent not on the thread
         joins it). agents message each other with the `send` tool of codehead's MCP server.
         wheel, pgup / pgdn, shift+↑↓: scroll · esc: back to live, else interrupt (after 0.5 s) · esc esc: back.
         send: ctrl+enter (⌘enter, alt+enter); ctrl+s never sends. a question: ↑↓ space enter 1-9,
         letters: Other, tab: question ↔ prompt. a new question takes the keys only on an empty prompt
         after 1 s of 0 keys; else it waits for tab (0 typed key answers it by surprise).
         click a link: open it; ctrl+click: any URL on screen. ctrl+c: copies the prompt's selection,
         else clears the prompt (ctrl+z: back), 2× on an empty one closes codehead.
         #: shortcuts (settings → shortcuts): #name stands for its text; tab / enter puts the text in,
         a #name left in the prompt: its text on send. ↑↓ in any dropdown: the row into the text at once.
         >: commands, as in the task prompt (files: of the thread's worktree), and
         branch: a copy of the thread (events, members, dir); each agent's next session forks its
         Claude Code session (claude --resume ID --fork-session). the old thread stays.
         @: agent names. ←→ on a name: + add, or ⇄ replace an agent of the thread: on send, the old
         agent leaves and the new one gets the message with the old one's last messages.
         ctrl+i / ⌘i (every pane): the IDE on the repo root, several repos: a picker (setting editor: auto, code, cursor, zed, idea …).
         views (row 0): F1-F4 (ctrl+1-4, alt+1-4): chat, changes, merge, files. tab / i: the prompt, tab / esc:
         back to the view. yazi: files view y, menu. changes: 1 box per file in review
         order (schema, logic, config, UI, docs, tests, generated), j k box (j: down), n N hunk,
         v reviewed, enter fold, e / o IDE at the line, w side by side / unified, r read git again,
         ctrl+f filter, ctrl+b: panel shown → its keys → hidden, i / tab prompt (esc / tab back).
         S: the ship menu: push, fetch + rebase, open the PR, merge it, CI log → agents (a stopped
         rebase: the agents resolve, continue, abort); outward steps: enter enter.
         ↑ on the 1st row of a PR's diff: its views row.
         flow waits (gate, loop max, role with 0 agent): a question in the picker under the stream,
         after the agents' ones; text in the prompt goes along as the note. > flow: the flow's steps.
         ctrl+f: the search bar above the prompt (the chat's messages, notes, shell, tool calls);
         tab / enter / ↑ the next older match, shift+tab / ↓ the next newer one, esc closes.
         tool runs (▸): click opens / closes the calls (input + output); ctrl+o: all.
         thoughts (setting thoughts = hidden | folded | open): ∴ rows, a click opens 1, ctrl+t: all.
         ctrl+v pastes a clipboard image as [Image #n], a dropped image path too. pasted text over
         1024 chars (code: never): [Pasted text #n +L lines]. both go in as attachments.
         a click on a marker or a 📎 row: the files view on the file (section attachments), y: yazi.
         files view: images + pdf page 1 drawn in Ghostty, kitty, Rio, iTerm2, WezTerm, sixel
         terminals (setting previews = auto | kitty | iterm2 | sixel | off; 0 in tmux, zellij, screen).
         ⌘/ctrl+Enter (alt+Enter) sends, Enter: new line. ↑ on the 1st row of an empty
         prompt / ↓ on the last: the thread's sent messages.
         agent questions and permission requests: ↑↓ / 1-9 / space / enter on the empty prompt, or a
         click on a choice.
engine:  codehead runs the agents itself. 1 host process per thread (codehead host <id>, detached:
         agents keep working after codehead closes; it exits 15 min. after the last turn). it runs 1
         harness session per agent and resumes it on the agent's next message. harnesses: claude
         (claude -p, stream-json), codex, opencode, goose, cursor, pi, omp, devin, grok, kimi, amp,
         hermes, openclaw (Agent Client Protocol; $CODEHEAD_ACP_<NAME> or `command` in the agent
         file: another command). ACP autopilot: guarded commands (force push, sudo, …) still ask. records: ~/.local/share/codehead/threads/<id>/ (thread.json,
         events.jsonl; $CODEHEAD_HOME). agents: ~/.config/codehead/agents/<name>.toml.
         sockets: $XDG_RUNTIME_DIR/codehead. logs: ~/.local/share/codehead/logs.
         codehead ctl <thread> '<json>': 1 command to a thread's host (scripts).
menu:    ctrl+p on every screen (setting keys.menu: 1 key codehead binds to nothing else), esc / ctrl+p
         closes. tab / shift+tab: tabs (←→ too, outside threads / merge); ↓ / ↑ at the top of a list: in / out of a
         tab's sub-tabs (threads: repos, merge: states) and its list. quick: new, settings + the commands of
         the screen (keys: the keys of the screen). threads: filter (type) + thread cards, enter opens.
         agents: the agents of the thread in view (enter, enter: remove = it leaves the thread), add,
         edit → the agents tab of the start screen.";

type Term = Terminal<CrosstermBackend<Stdout>>;

#[derive(Default)]
struct Args {
    agent: Option<String>,
    branch: Option<String>,
    copy: bool,
    dry: bool,
    title: String,
    view: Option<String>,
}

fn fail(msg: &str, code: i32) -> ! {
    eprintln!("codehead: {msg}");
    std::process::exit(code)
}

fn parse() -> Args {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    let mut rest = Vec::new();
    while let Some(s) = it.next() {
        match s.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                std::process::exit(0)
            }
            "-V" | "--version" => {
                println!("codehead {}", update::VERSION);
                std::process::exit(0)
            }
            "--update" => update::run(),
            "-a" => a.agent = Some(it.next().unwrap_or_else(|| fail("-a: value required", 2))),
            "-b" => a.branch = Some(it.next().unwrap_or_else(|| fail("-b: value required", 2))),
            "-c" => a.copy = true,
            "--cost" => cost::run(&it.next().unwrap_or_else(|| fail("--cost: thread id required", 2))),
            "--view" => a.view = Some(it.next().unwrap_or_else(|| fail("--view: thread id required", 2))),
            "-n" => a.dry = true,
            "--" => {
                rest.extend(it.by_ref());
            }
            s if s.starts_with('-') && s.len() > 1 => fail(&format!("unknown option {s}\n\n{HELP}"), 2),
            _ => rest.push(s),
        }
    }
    a.title = rest.join(" ");
    a
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
}

/// Config agent ($CODEHEAD_AGENT, codehead.toml), else amon, else the first active agent.
fn default_agent(all: &[agents::Agent]) -> Option<String> {
    if let Some(a) = config::agent() {
        return Some(a);
    }
    all.iter().find(|a| a.name == "amon").or(all.first()).map(|a| a.name.clone())
}

// ---------- terminal ----------
struct Tty {
    enhanced: bool,
}

impl Tty {
    fn enter(&mut self, out: &mut Stdout) -> io::Result<()> {
        terminal::enable_raw_mode()?;
        // the terminal's palette (agent border colors), before crossterm reads the tty
        theme::query_palette();
        execute!(out, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
        // always on, also when the support query got 0 answer in time: without it shift+enter
        // comes as `ESC[27;2;13~`, which crossterm drops. Terminals without it ignore the request.
        execute!(out, PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES))?;
        self.enhanced = true;
        Ok(())
    }

    /// Input modes off, alternate screen kept (the next screen takes it over: no flash).
    fn release(&mut self, out: &mut Stdout) {
        all_keys(false);
        if self.enhanced {
            let _ = execute!(out, PopKeyboardEnhancementFlags);
            self.enhanced = false;
        }
        let _ = execute!(out, DisableMouseCapture, DisableBracketedPaste, cursor::Show);
        let _ = terminal::disable_raw_mode();
    }

    fn leave(&mut self, out: &mut Stdout) {
        self.release(out);
        let _ = execute!(out, LeaveAlternateScreen);
        if let Some(hint) = update::hint() {
            eprintln!("{hint}");
        }
    }
}

fn restore_on_panic() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let mut out = io::stdout();
        all_keys(false);
        let _ = execute!(out, PopKeyboardEnhancementFlags, DisableMouseCapture, DisableBracketedPaste, cursor::Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
        hook(info);
    }));
}

fn editor(path: &Path) {
    let ed = std::env::var("VISUAL")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("EDITOR").ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| if which("nvim") { "nvim".into() } else { "vi".into() });
    let mut parts = ed.split_whitespace();
    let bin = parts.next().unwrap_or("vi");
    let _ = Command::new(bin).args(parts).arg(path).status();
}

/// Every event ready within WAIT (the 1st one) and right after it, each with its read time.
/// Read first, handled after: the handling time of 1 key never splits a burst of the OS.
/// SIGTERM / SIGHUP came: the next `read_ready` fails, the screen loops end and restore the tty.
static SIGNALED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    SIGNALED.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn catch_signals() {
    for s in [libc::SIGTERM, libc::SIGHUP] {
        // SAFETY: the handler only stores to an atomic (async-signal-safe).
        unsafe { libc::signal(s, on_signal as *const () as libc::sighandler_t) };
    }
}

static ALL_KEYS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// ON: every key as an escape code, modifiers alone and releases too (the kitty keyboard
/// protocol: an alt tap in the changes view); off: back to the base flags. Terminals without
/// it ignore the request.
pub fn all_keys(on: bool) {
    use std::sync::atomic::Ordering;
    if ALL_KEYS.swap(on, Ordering::Relaxed) == on {
        return;
    }
    let mut out = io::stdout();
    let _ = if on {
        execute!(
            out,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                    | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
            )
        )
    } else {
        execute!(out, PopKeyboardEnhancementFlags)
    };
}

pub fn read_ready(wait: Duration) -> Result<Vec<(Event, Instant)>, String> {
    if SIGNALED.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("terminated by a signal".into());
    }
    let mut out = Vec::new();
    let mut wait = wait;
    while out.len() < 512 && event::poll(wait).map_err(|e| e.to_string())? {
        let ev = event::read().map_err(|e| e.to_string())?;
        key_log(&ev);
        out.push((ev, Instant::now()));
        wait = Duration::ZERO;
    }
    Ok(out)
}

/// $CODEHEAD_KEYLOG=1: each key event as 1 line in ~/.cache/codehead/keys.log (what the terminal
/// sends for a key).
fn key_log(ev: &Event) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var_os("CODEHEAD_KEYLOG").is_some_and(|v| !v.is_empty() && v != "0")) {
        return;
    }
    let Event::Key(k) = ev else { return };
    let path = config::home().join(".cache/codehead/keys.log");
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} {:?} {:?} {:?}", engine::now_ms(), k.code, k.modifiers, k.kind);
    }
}

// ---------- threads ----------
fn quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:@+,".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// Append 1 line to the view log (why the thread view did not open).
fn view_log(msg: &str) {
    let dir = engine::logs_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("view.log")) {
        let _ = writeln!(f, "{} {msg}", engine::now_ms() / 1000);
    }
}

/// Thread ID before its view: a done thread opens again. Its host starts with the next message.
fn wake(id: &str) {
    if engine::store::load(id).is_some_and(|t| t.state != "open") {
        let _ = engine::store::update(id, |t| t.state = "open".into());
    }
}

/// Thread view.
/// Esc esc: the retained start screen in HOME, without another process. Some(tab): the menu asked
/// for that tab of the start screen.
#[allow(clippy::too_many_arguments)]
fn open_view(term: &mut Term, tty: &mut Tty, out: &mut Stdout, id: &str, cwd: &Path, title: &str, home: &Path, note: Option<String>) -> Option<menu::Home> {
    let (mut id, mut cwd, mut title) = (id.to_string(), cwd.to_path_buf(), title.to_string());
    // the thread header keeps the thread opened first at index 0
    let first = id.clone();
    let mut note = note;
    let _ = terminal::enable_raw_mode();
    let _ = execute!(out, EnableMouseCapture, EnableBracketedPaste);
    loop {
    let _ = std::env::set_current_dir(&cwd);
    match view::run(term, &id, &cwd, &title, note.as_deref(), &first) {
        Ok(view::Exit::Quit) => {
            tty.leave(out);
            std::process::exit(0)
        }
        Ok(view::Exit::Open { id: new, dir, title: new_title, note: new_note }) => {
            note = Some(new_note);
            wake(&new);
            // the thread's own work dir (a menu row without a dir: the dir of the thread before)
            cwd = engine::store::load(&new).map(|t| t.work_dir()).filter(|p| p.is_dir()).unwrap_or(dir);
            id = new;
            title = new_title;
        }
        Ok(view::Exit::Back) => {
            // Keep App, terminal modes, lists, previews and form state in memory.
            let _ = std::env::set_current_dir(home);
            return None;
        }
        Ok(view::Exit::Home(tab)) => {
            let _ = std::env::set_current_dir(home);
            return Some(tab);
        }
        Err(e) => {
            view_log(&format!("thread {id}: view failed: {e}"));
            tty.leave(out);
            fail(&format!("thread view: {e}"), 1)
        }
    }
    }
}

/// Claude Code leaves the Artifact tools (web artifacts on claude.ai) out of SDK sessions, and
/// codehead runs its agents as SDK sessions: $CLAUDE_CODE_ARTIFACT=1 puts them back. Set here, for
/// every session codehead starts (hosts and agents inherit it). A value set by the person stays.
fn enable_artifacts() {
    if std::env::var_os("CLAUDE_CODE_ARTIFACT").is_none() {
        // SAFETY: the 1st statement of main, 0 other threads yet.
        unsafe { std::env::set_var("CLAUDE_CODE_ARTIFACT", "1") };
    }
}

fn main() {
    engine::trace("main");
    enable_artifacts();
    sub();
    // the palette before any screen (`--cost` draws in parse); a broken file fails in start
    let _ = config::load();
    let args = parse();
    update::spawn_check();
    start(args);
}

/// Subcommands without a screen: the thread host, the agents' MCP server, scripting.
fn sub() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| argv.get(i).cloned().unwrap_or_else(|| fail(&format!("{}: argument {i} missing", argv[0]), 2));
    match argv.first().map(String::as_str) {
        Some("host") => engine::host::run(&arg(1)),
        Some("mcp") => engine::mcp::run(),
        Some("home") => homeinit::run(argv.get(1).map(String::as_str)),
        Some("setup") => wizard::run(argv.iter().any(|a| a == "--dry")),
        Some("tutorial") => tutor::run(&argv[1..]),
        Some("tutor-agent") => tutor::agent(&argv[1..]),
        Some("ctl") => {
            // codehead ctl <thread> '<json command>': 1 command to the thread's host
            let c: serde_json::Value = serde_json::from_str(&arg(2)).unwrap_or_else(|e| fail(&format!("ctl: {e}"), 2));
            match engine::client::call(&arg(1), &c) {
                Ok(v) => println!("{v}"),
                Err(e) => fail(&e, 1),
            }
            std::process::exit(0)
        }
        _ => {}
    }
}

fn start(args: Args) {
    if let Err(e) = config::load() {
        fail(&e, 2);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let repo = threads::repo_root(&cwd);
    if engine::harnesses::installed().is_empty() {
        let list: Vec<String> = engine::harnesses::ALL.iter().map(|h| format!("  {} ({}): {}", h.label, h.bin, h.install)).collect();
        fail(&format!("0 harnesses on PATH: codehead's agents run on one of these:\n{}", list.join("\n")), 1);
    }
    engine::trace("harnesses");
    let mut all = agents::cached();
    let picks: Vec<String> = args.agent.iter().flat_map(|a| a.split(',')).map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect();
    if picks.iter().any(|p| !all.iter().any(|a| &a.name == p)) {
        all = agents::load();
    }
    let agent = default_agent(&all).unwrap_or_default();
    if let Some(bad) = picks.iter().find(|p| !all.iter().any(|a| &a.name == *p)) {
        fail(&format!("no agent named {bad}. agents: {}", all.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")), 2);
    }
    if args.branch.is_some() && repo.is_none() {
        fail("-b needs a git repo. cd into one first", 2);
    }
    let notes = config::notes();
    let _ = std::fs::create_dir_all(&notes);
    let cfg = Config {
        notes,
        repo,
        cwd: cwd.clone(),
        agent,
        picks,
        branch: args.branch.clone(),
        prefix: config::prefix(),
        copy_mode: args.copy,
        logs: engine::logs_dir(),
        drafts: drafts::dir(),
    };
    restore_on_panic();
    catch_signals();
    // a plan window ends: its next one starts 1 min after (1 trigger message per harness)
    if !args.dry {
        engine::warm::watch();
    }
    // agents stopped on a usage limit whose host is gone (a reboot, a kill): their host again,
    // it resumes them at the reset
    if !args.dry {
        std::thread::spawn(|| {
            for t in engine::store::all().into_iter().filter(|t| !t.resume.is_empty()) {
                let _ = engine::client::ensure(&t.id);
            }
        });
    }
    let mut out = io::stdout();
    let mut tty = Tty { enhanced: false };
    if let Err(e) = tty.enter(&mut out) {
        tty.leave(&mut out);
        fail(&format!("terminal: {e}"), 1);
    }
    let mut term = match Terminal::new(CrosstermBackend::new(io::stdout())) {
        Ok(t) => t,
        Err(e) => {
            tty.leave(&mut out);
            fail(&format!("terminal: {e}"), 1)
        }
    };

    // plain `codehead` (0 options, 0 title): the thread updated last; esc esc: the start screen
    let plain = args.view.is_none() && args.title.is_empty() && args.agent.is_none() && args.branch.is_none() && !args.copy && !args.dry;
    let latest = plain.then(engine::store::latest).flatten().map(|t| t.id);
    // the thread view first: the start screen (App) is built when it ends
    notify::start();
    let mut back: Option<Option<menu::Home>> = None;
    if let Some(id) = args.view.as_ref().or(latest.as_ref()) {
        // 1 thread.json (an id prefix: the newest match), 0 scan of every thread's host and log
        let t = engine::store::load(id).or_else(|| engine::store::all().into_iter().find(|t| t.id.starts_with(id.as_str())));
        let Some(t) = t else { tty.leave(&mut out); fail(&format!("no thread {id}"), 2) };
        let dir = [t.worktree.clone(), Some(t.dir.clone())].into_iter().flatten().map(PathBuf::from).find(|p| p.is_dir()).unwrap_or_else(|| cwd.clone());
        // --view opens a done thread again; the auto-open leaves its state as it is
        if args.view.is_some() {
            wake(&t.id);
        }
        back = Some(open_view(&mut term, &mut tty, &mut out, &t.id, &dir, &t.title, &cwd, None));
    }
    // a thread view ended: until the start screen's next frame
    let mut transition: Option<Instant> = back.is_some().then(Instant::now);
    engine::trace("app new …");
    let mut app = App::new(cfg, &args.title);
    // -b names a branch of the start dir's repo: the setting `repo` stays off then
    if args.branch.is_none()
        && let Some(r) = config::default_repo()
    {
        app.use_default_repo(&r);
    }
    engine::trace("app new");
    if let Some(tab) = back {
        app.return_from_thread();
        if let Some(tab) = tab { app.go_home(tab); }
    }
    // draws only after an event, a background read or once per second (relative times):
    // an idle start screen builds 1 frame per second, not 12
    let mut dirty = true;
    let mut notes_v = crate::notify::version();
    let mut drawn = Instant::now();
    'screens: loop {
    loop {
        // new notifications / seen marks: the header redraws
        let nv = crate::notify::version();
        if nv != notes_v {
            notes_v = nv;
            dirty = true;
        }
        // a dissolve under way: 30 frames per second
        if dirty || drawn.elapsed() >= Duration::from_secs(1) || (dissolve::active() && drawn.elapsed() >= Duration::from_millis(33)) {
            let _w = engine::watch(|| "start: draw".to_string());
            let _ = term.draw(|f| app.draw(f));
            dirty = false;
            drawn = Instant::now();
            engine::first_frame();
            if let Some(at) = transition.take() {
                let took = at.elapsed();
                if took > engine::BUDGET {
                    engine::trace(&format!("SLOW {} back to the start screen", took.as_millis()));
                }
            }
        }
        if app.outcome.is_some() {
            break;
        }
        if let Some(s) = app.suspend.take() {
            tty.leave(&mut out);
            match s {
                Suspend::Note(p) => {
                    editor(&p);
                    app.reload_notes();
                }
                Suspend::Prompt => {
                    let f = std::env::temp_dir().join(format!("codehead-prompt-{}.md", std::process::id()));
                    if std::fs::write(&f, app.prompt_text()).is_ok() {
                        editor(&f);
                        if let Ok(t) = std::fs::read_to_string(&f) {
                            app.set_prompt(t.trim_end_matches('\n'));
                        }
                        let _ = std::fs::remove_file(&f);
                    }
                }
            }
            let _ = tty.enter(&mut out);
            let _ = term.clear();
            dirty = true;
            continue;
        }
        // every waiting event before the next draw: a burst (OS text substitution) stays 1 burst
        let mut wait = Duration::from_millis(if dissolve::active() { 33 } else { 80 });
        let mut failed = false;
        'events: loop {
            let batch = match read_ready(wait) {
                Ok(b) if b.is_empty() => break,
                Ok(b) => b,
                Err(_) => {
                    failed = true;
                    break;
                }
            };
            dirty = true;
            for (ev, at) in batch {
                textarea::stamp(Some(at));
                let tag = if engine::trace_on() { format!("start: {ev:?}") } else { String::new() };
                let _w = engine::watch(move || tag);
                match ev {
                    Event::Key(k) if k.kind != KeyEventKind::Release => app.key(k),
                    Event::Mouse(m) => app.mouse(m),
                    Event::Paste(s) => app.paste(&s),
                    _ => {}
                }
                if app.outcome.is_some() || app.suspend.is_some() {
                    textarea::stamp(None);
                    break 'events;
                }
            }
            textarea::stamp(None);
            wait = Duration::ZERO;
        }
        if failed {
            break;
        }
        let _w = engine::watch(|| "start: tick".to_string());
        dirty |= app.tick();
    }

    let outcome = app.outcome.take().unwrap_or(Outcome::Quit);
    let home_tab: Option<menu::Home> = match outcome {
        Outcome::Quit => { tty.leave(&mut out); break 'screens; }
        Outcome::Copy(paths) => {
            tty.leave(&mut out);
            if paths.is_empty() {
                return;
            }
            let text: String = paths.iter().map(|p| format!("{}\n", p.display())).collect();
            if clip::copy(&text) {
                println!("copied {} path(s)", paths.len());
            } else {
                print!("{text}");
                eprintln!("0 clipboard tools found (wl-copy, xclip, xsel). paths printed");
            }
            break 'screens;
        }
        Outcome::New { message, target, agents: picked, clone, flows } => {
            engine::trace("start: submit");
            // a home repo not on disk yet: clone it first
            if let Some((url, path)) = &clone {
                let _ = term.draw(|f| app::draw_status(f, &format!("cloning {url}…"), 0));
                if let Err(e) = home::clone(url, path) {
                    tty.leave(&mut out);
                    fail(&e, 1)
                }
            }
            let a = &app.cfg;
            // git first: switch the main checkout (existing branch not checked out anywhere, new branch)
            let mut prep: Option<Vec<String>> = None;
            // the switch fails on conflicting local changes: git stash, then switch again
            let mut stash = false;
            // a new linked worktree for the branch
            let mut worktree: Option<String> = None;
            let repo = a.repo.clone().unwrap_or(cwd.clone());
            let (dir, label) = match &target {
                Target::Checkout { branch: b, switch, stash: s } => {
                    let dir = match threads::checkouts(&repo).get(b) {
                        Some(p) => PathBuf::from(p),
                        None => {
                            if *switch {
                                prep = Some(["git", "-C", &repo.display().to_string(), "switch", b].map(String::from).to_vec());
                                stash = *s;
                            }
                            repo.clone()
                        }
                    };
                    (dir, b.clone())
                }
                Target::NewBranch(b) => {
                    prep = Some(["git", "-C", &repo.display().to_string(), "switch", "-c", b].map(String::from).to_vec());
                    (repo.clone(), b.clone())
                }
                Target::Branch(b) => {
                    worktree = Some(b.clone());
                    (repo.clone(), b.clone())
                }
                Target::Worktree { path, branch } => (PathBuf::from(path), branch.clone()),
                Target::Dir(d) => {
                    let p = PathBuf::from(d);
                    let label = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    (p, label)
                }
                Target::InPlace => (cwd.clone(), cwd.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
            };
            let title = message.lines().next().unwrap_or_default().to_string();
            if args.dry {
                tty.leave(&mut out);
                if let Some(g) = &prep {
                    let line = g.iter().map(|s| quote(s)).collect::<Vec<_>>().join(" ");
                    if stash {
                        println!("{line} || {{ git -C {} stash push --include-untracked -m 'codehead: before switch' && {line}; }}", quote(&repo.display().to_string()));
                    } else {
                        println!("{line}");
                    }
                }
                if let Some(b) = &worktree {
                    println!("git -C {} worktree add <codehead data>/worktrees/{} {}", quote(&repo.display().to_string()), quote(b), quote(b));
                }
                if worktree.is_none() && matches!(target, Target::Checkout { .. } | Target::Worktree { .. }) {
                    println!("git -C {} pull --ff-only    # on main / master with an upstream", quote(&dir.display().to_string()));
                }
                println!("thread in {} · agents {} · task:\n{message}", dir.display(), picked.join(", "));
                return;
            }
            if let Some(g) = &prep {
                let run = |g: &[String]| match Command::new(&g[0]).args(&g[1..]).stdin(Stdio::null()).output() {
                    Ok(o) if o.status.success() => Ok(()),
                    Ok(o) => Err(format!("{}: {}", g.join(" "), String::from_utf8_lossy(&o.stderr).trim())),
                    Err(e) => Err(format!("{}: {e}", g.join(" "))),
                };
                let mut res = run(g);
                if res.is_err() && stash {
                    let label = target_label(&target);
                    let st: Vec<String> = ["git", "-C", &repo.display().to_string(), "stash", "push", "--include-untracked", "-m", &format!("codehead: before switch to {label}")].map(String::from).to_vec();
                    res = run(&st).and_then(|_| run(g));
                }
                if let Err(e) = res {
                    tty.leave(&mut out);
                    fail(&e, 1)
                }
            }
            engine::trace("start: git ready");
            let _ = term.draw(|f| app::draw_status(f, &format!("starting {label} · {}…", picked.join(", ")), 0));
            let mut t = engine::store::Thread::new(&title, &message, &dir, picked.clone());
            // a thread on main in an existing checkout: the host pulls main (ff only) before the
            // 1st turn, the view opens at once; a failed pull: the agents get a note on it
            t.pull = worktree.is_none() && matches!(target, Target::Checkout { .. } | Target::Worktree { .. }) && (label == "main" || label == "master");
            if let Some(b) = &worktree {
                match threads::add_worktree(&repo, b) {
                    Ok(p) => {
                        // local files (.env …), a port block, the setup script before the 1st turn
                        crate::setup::copy_includes(&repo, &p);
                        t.port = Some(crate::setup::free_port(&engine::store::all()));
                        if crate::setup::script(&p, "setup").is_some() {
                            t.setup = Some("pending".into());
                        }
                        t.worktree = Some(p.display().to_string());
                        t.branch = Some(b.clone());
                    }
                    Err(e) => {
                        tty.leave(&mut out);
                        fail(&e, 1)
                    }
                }
            } else if !label.is_empty() && repo.join(".git").exists() {
                t.branch = Some(threads::current_branch(&dir)).filter(|b| !b.is_empty());
            }
            // each agent's flow: its start step (the 1st, else the owner's pick) goes to the agent
            // with the task (the host's `prompt`)
            if !flows.is_empty() {
                let lib = engine::flows::Library::load(Some(&t.work_dir()));
                for (agent, slug, start) in &flows {
                    if let Some(fl) = lib.flow(slug) {
                        t.set_flow(agent, Some(engine::flows::run::FlowState::new(fl, &lib, *start, engine::now_ms())));
                    }
                }
            }
            if let Err(e) = t.save() {
                tty.leave(&mut out);
                fail(&e, 1)
            }
            engine::trace("start: thread saved");
            // the task to every picked agent; the host starts with it. 0 host (a failed start):
            // the thread opens with the task as its prompt draft, never lost
            let note = match engine::client::call(&t.id, &serde_json::json!({"op": "prompt", "to": picked, "text": message})) {
                Ok(_) => None,
                Err(e) => {
                    view::save_prompt_draft(&t.id, &message);
                    Some(format!("task not sent: {} · the task waits in the prompt · ctrl+enter: send", e.lines().next().unwrap_or_default()))
                }
            };
            engine::trace("start: task sent");
            // the thread has the message (or its draft): the form starts empty again
            app.clear_task();
            open_view(&mut term, &mut tty, &mut out, &t.id, &t.work_dir(), &title, &cwd, note)
        }
        Outcome::Resume(w) => {
            let Some(t) = w.thread.clone() else { return tty.leave(&mut out) };
            let dir = PathBuf::from(&w.path);
            if args.dry {
                tty.leave(&mut out);
                println!("codehead --view {}", t.id);
                return;
            }
            wake(&t.id);
            open_view(&mut term, &mut tty, &mut out, &t.id, &dir, &t.title, &cwd, None)
        }
    };
    transition = Some(Instant::now());
    app.return_from_thread();
    if let Some(tab) = home_tab { app.go_home(tab); }
    dirty = true;
    }
}

/// Branch name of a checkout target (stash message).
fn target_label(t: &Target) -> String {
    match t {
        Target::Checkout { branch, .. } => branch.clone(),
        _ => String::new(),
    }
}
